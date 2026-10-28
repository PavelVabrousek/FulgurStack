//! Bounded batched f32 cosmetics/VNG compute. No image files or science accumulation.
use fulgur_core::{BayerPattern, DemosaicMethod, RawFrame, RgbImage};
use serde::Serialize;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};
use wgpu::util::DeviceExt;

pub const SHADER: &str = include_str!("reconstruct.wgsl");
type GpuResult<T> = std::result::Result<T, String>;

// Specialize the shared stencils into constant register indices. Dynamic array
// indexing in the generic shader spills the 5x5 window on integrated GPUs.
fn specialized_shader(plans: &[u32]) -> String {
    use std::fmt::Write;
    let mut helper = String::from(
        "\nfn reconstruct_window(w:array<f32,25>, key:u32)->vec3<f32>{\nswitch key {\n",
    );
    for key in 0..16 {
        writeln!(helper, "case {key}u: {{").unwrap();
        for d in 0..8 {
            let b = (key * 8 + d) * 68;
            writeln!(helper, "var g{d}=0.0;").unwrap();
            for i in 0..plans[b] as usize {
                let k = b + 4 + i * 4;
                writeln!(
                    helper,
                    "g{d}+=bitcast<f32>({}u)*abs(w[{}]-w[{}]);",
                    plans[k + 2],
                    plans[k],
                    plans[k + 1]
                )
                .unwrap();
            }
        }
        helper.push_str("var lo=g0;var hi=g0;\n");
        for d in 1..8 {
            writeln!(helper, "lo=min(lo,g{d});hi=max(hi,g{d});").unwrap();
        }
        helper.push_str("let t=lo+0.5*hi;var s=vec3<f32>(0.0);var n=0u;\n");
        for d in 0..8 {
            let b = (key * 8 + d) * 68;
            writeln!(helper, "if(g{d}<=t) {{").unwrap();
            for i in 0..plans[b + 1] as usize {
                let k = b + 36 + i * 4;
                let c = ["x", "y", "z"][plans[k + 1] as usize];
                writeln!(
                    helper,
                    "s.{c}+=w[{}]*bitcast<f32>({}u);",
                    plans[k],
                    plans[k + 2]
                )
                .unwrap();
            }
            helper.push_str("n++;}\n");
        }
        let red = [1, 0, 2, 3][key / 4];
        let measured = if key % 4 == red {
            "x"
        } else if key % 4 == 3 - red {
            "z"
        } else {
            "y"
        };
        writeln!(helper,"var v=vec3<f32>(w[12])+(s-vec3<f32>(s.{measured}))/f32(n);v.{measured}=w[12];return v;}}").unwrap();
    }
    helper.push_str("default:{return vec3<f32>(0.0);}}}\n");
    let start = SHADER.find("    var gradients:").unwrap();
    let mut shader = SHADER[..start].to_string();
    shader.push_str("    let v=reconstruct_window(window,job.pattern*4u+phase);\n    let out_index=(job.offset+y*job.width+x)*3u;\n    rgb[out_index]=v.x;rgb[out_index+1u]=v.y;rgb[out_index+2u]=v.z;\n}\n");
    shader.push_str(&helper);
    shader
}

#[derive(Clone, Default, Debug, Serialize)]
pub struct GpuTelemetry {
    pub adapter: String,
    pub backend: String,
    pub device_type: String,
    pub driver: String,
    pub initialization_seconds: f64,
    pub batches: u64,
    pub reconstructed_pixels: u64,
    pub uploaded_bytes: u64,
    pub downloaded_bytes: u64,
    pub batch_wall_seconds: f64,
    pub peak_reserved_buffer_bytes: u64,
    pub numeric_policy: &'static str,
}
struct Buffers {
    capacity: usize,
    raw: wgpu::Buffer,
    _clean: wgpu::Buffer,
    rgb: wgpu::Buffer,
    jobs: wgpu::Buffer,
    counts: wgpu::Buffer,
    readback: wgpu::Buffer,
    group: wgpu::BindGroup,
}
pub struct GpuReconstructor {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    plans: wgpu::Buffer,
    cosmetic: wgpu::ComputePipeline,
    vng: wgpu::ComputePipeline,
    buffers: Option<Buffers>,
    max_pixels: usize,
    telemetry: GpuTelemetry,
}
fn cancelled(cancel: &AtomicBool) -> GpuResult<()> {
    if cancel.load(Ordering::Relaxed) {
        Err("GPU processing cancelled.".into())
    } else {
        Ok(())
    }
}
impl GpuReconstructor {
    pub fn new(plans: &[u32]) -> GpuResult<Self> {
        let start = Instant::now();
        if plans.len() != 4 * 4 * 8 * 68 {
            return Err("Invalid shared VNG plans.".into());
        }
        for plan in plans.chunks_exact(68) {
            if plan[0] > 8
                || plan[1] > 8
                || (0..plan[0] as usize).any(|i| {
                    let k = 4 + i * 4;
                    plan[k] >= 25 || plan[k + 1] >= 25 || !f32::from_bits(plan[k + 2]).is_finite()
                })
                || (0..plan[1] as usize).any(|i| {
                    let k = 36 + i * 4;
                    plan[k] >= 25 || plan[k + 1] >= 3 || !f32::from_bits(plan[k + 2]).is_finite()
                })
            {
                return Err("Invalid shared VNG stencil.".into());
            }
        }
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12 | wgpu::Backends::VULKAN | wgpu::Backends::METAL,
            ..Default::default()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .map_err(|e| format!("No compatible hardware GPU: {e}"))?;
        let info = adapter.get_info();
        if info.device_type == wgpu::DeviceType::Cpu {
            return Err("Software rendering is not GPU acceleration; use --engine cpu.".into());
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("FulgurStack GPU reconstruction"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| format!("GPU initialization failed: {e}"))?;
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let entries: Vec<_> = (0..6)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage {
                        read_only: matches!(binding, 0 | 3 | 4),
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("reconstruction buffers"),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("reconstruction layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("accepted cosmetics/VNG"),
            source: wgpu::ShaderSource::Wgsl(specialized_shader(plans).into()),
        });
        let pipeline = |entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let cosmetic = pipeline("cosmetic");
        let vng = pipeline("vng");
        if let Some(e) = pollster::block_on(device.pop_error_scope()) {
            return Err(format!("GPU shader validation failed: {e}"));
        }
        let plans = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("shared CPU-generated VNG plans"),
            contents: bytemuck::cast_slice(plans),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let limits = device.limits();
        let max_pixels = (limits.max_storage_buffer_binding_size as usize / 12)
            .min(limits.max_buffer_size.saturating_sub(64) as usize / 12)
            .min(limits.max_compute_workgroups_per_dimension as usize * 128);
        let telemetry = GpuTelemetry {
            adapter: info.name,
            backend: format!("{:?}", info.backend),
            device_type: format!("{:?}", info.device_type),
            driver: format!("{} {}", info.driver, info.driver_info),
            initialization_seconds: start.elapsed().as_secs_f64(),
            numeric_policy: "f32 cosmetics/VNG; no f16; CPU f64 geometry/photometry/Neumaier accumulation retained",
            ..Default::default()
        };
        Ok(Self {
            device,
            queue,
            layout,
            plans,
            cosmetic,
            vng,
            buffers: None,
            max_pixels,
            telemetry,
        })
    }
    pub fn telemetry(&self) -> GpuTelemetry {
        self.telemetry.clone()
    }
    fn allocate(&mut self, needed: usize) {
        if self.buffers.as_ref().is_some_and(|b| b.capacity >= needed) {
            return;
        }
        let capacity = needed.next_power_of_two().min(self.max_pixels);
        let make = |label, size, usage| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        let storage = wgpu::BufferUsages::STORAGE;
        let raw = make(
            "raw ADU",
            capacity as u64 * 4,
            storage | wgpu::BufferUsages::COPY_DST,
        );
        let clean = make("cleaned ADU", capacity as u64 * 4, storage);
        let rgb = make(
            "linear RGB ADU",
            capacity as u64 * 12,
            storage | wgpu::BufferUsages::COPY_SRC,
        );
        let jobs = make(
            "bounded ROI descriptors",
            256,
            storage | wgpu::BufferUsages::COPY_DST,
        );
        let counts = make(
            "correction counts",
            64,
            storage | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
        );
        let readback = make(
            "RGB/count readback",
            capacity as u64 * 12 + 64,
            wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        );
        let buffers = [&raw, &clean, &rgb, &jobs, &self.plans, &counts];
        let bindings: Vec<_> = buffers
            .iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: b.as_entire_binding(),
            })
            .collect();
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("reconstruction batch"),
            layout: &self.layout,
            entries: &bindings,
        });
        let previous = self
            .buffers
            .as_ref()
            .map_or(0, |b| b.capacity as u64 * 32 + 384);
        self.telemetry.peak_reserved_buffer_bytes =
            (previous + capacity as u64 * 32 + 384 + 4 * 4 * 8 * 68 * 4)
                .max(self.telemetry.peak_reserved_buffer_bytes);
        self.buffers = Some(Buffers {
            capacity,
            raw,
            _clean: clean,
            rgb,
            jobs,
            counts,
            readback,
            group,
        });
    }
    pub fn reconstruct(
        &mut self,
        frames: &[&RawFrame],
        cancel: &AtomicBool,
    ) -> GpuResult<Vec<(RgbImage, usize)>> {
        cancelled(cancel)?;
        if frames.len() > 16 {
            return Err("GPU ROI concurrency exceeds 16.".into());
        }
        let mut output = Vec::with_capacity(frames.len());
        let mut from = 0;
        while from < frames.len() {
            let mut end = from;
            let mut pixels = 0;
            while end < frames.len() && pixels + frames[end].pixels.len() <= self.max_pixels {
                pixels += frames[end].pixels.len();
                end += 1;
            }
            if end == from {
                return Err("One ROI exceeds GPU storage/dispatch limits; use a smaller tile or --engine cpu.".into());
            }
            output.extend(self.batch(&frames[from..end], pixels, cancel)?);
            from = end;
        }
        Ok(output)
    }
    fn batch(
        &mut self,
        frames: &[&RawFrame],
        pixels: usize,
        cancel: &AtomicBool,
    ) -> GpuResult<Vec<(RgbImage, usize)>> {
        cancelled(cancel)?;
        let start = Instant::now();
        self.allocate(pixels);
        let b = self.buffers.as_ref().unwrap();
        let mut raw = Vec::with_capacity(pixels);
        let mut jobs = Vec::new();
        let mut largest = 0;
        let mut largest_phase = 0;
        for frame in frames {
            let info = &frame.header.info;
            if info.width < 6
                || info.height < 6
                || info.width.checked_mul(info.height) != Some(frame.pixels.len())
                || frame.pixels.iter().any(|v| !v.is_finite())
            {
                return Err("Invalid GPU CFA buffer.".into());
            }
            let pattern = match info.bayer_pattern {
                Some(BayerPattern::GRBG) => 0,
                Some(BayerPattern::RGGB) => 1,
                Some(BayerPattern::GBRG) => 2,
                Some(BayerPattern::BGGR) => 3,
                None => {
                    return Err("GPU reconstruction supports colour (Bayer) frames only.".into());
                }
            };
            jobs.extend([
                raw.len() as u32,
                info.width as u32,
                info.height as u32,
                pattern,
            ]);
            raw.extend_from_slice(&frame.pixels);
            largest = largest.max(frame.pixels.len());
            largest_phase = largest_phase.max(info.width.div_ceil(2) * info.height.div_ceil(2));
        }
        self.queue
            .write_buffer(&b.raw, 0, bytemuck::cast_slice(&raw));
        self.queue
            .write_buffer(&b.jobs, 0, bytemuck::cast_slice(&jobs));
        self.queue.write_buffer(&b.counts, 0, &[0; 64]);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cosmetics + VNG + readback"),
            });
        for (pipeline, invocations, phases) in
            [(&self.cosmetic, largest, 1), (&self.vng, largest_phase, 4)]
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("reconstruction pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &b.group, &[]);
            pass.dispatch_workgroups(
                invocations.div_ceil(128) as u32,
                frames.len() as u32,
                phases,
            );
        }
        encoder.copy_buffer_to_buffer(&b.rgb, 0, &b.readback, 0, pixels as u64 * 12);
        let counts_offset = b.capacity as u64 * 12;
        encoder.copy_buffer_to_buffer(&b.counts, 0, &b.readback, counts_offset, 64);
        let submission = self.queue.submit([encoder.finish()]);
        let (send, receive) = std::sync::mpsc::channel();
        b.readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = send.send(result);
            });
        self.device
            .poll(wgpu::PollType::wait_for(submission))
            .map_err(|e| format!("GPU device polling failed: {e}"))?;
        receive
            .recv_timeout(std::time::Duration::from_secs(30))
            .map_err(|e| format!("GPU readback did not complete: {e}"))?
            .map_err(|e| format!("GPU readback failed: {e}"))?;
        let mapped = b.readback.slice(..).get_mapped_range();
        let values: &[f32] = bytemuck::cast_slice(&mapped[..pixels * 12]);
        let counts: &[u32] =
            bytemuck::cast_slice(&mapped[counts_offset as usize..counts_offset as usize + 64]);
        let mut result = Vec::new();
        let mut offset = 0;
        for (i, frame) in frames.iter().enumerate() {
            let n = frame.pixels.len();
            let data: Vec<[f32; 3]> = values[offset * 3..(offset + n) * 3]
                .chunks_exact(3)
                .map(|v| [v[0], v[1], v[2]])
                .collect();
            if data.iter().flatten().any(|v| !v.is_finite()) {
                drop(mapped);
                b.readback.unmap();
                return Err("Nonfinite GPU reconstruction result.".into());
            }
            result.push((
                RgbImage {
                    width: frame.header.info.width,
                    height: frame.header.info.height,
                    demosaic_method: DemosaicMethod::Vng,
                    pixels: data,
                },
                counts[i] as usize,
            ));
            offset += n;
        }
        drop(mapped);
        b.readback.unmap();
        cancelled(cancel)?;
        self.telemetry.batches += 1;
        self.telemetry.reconstructed_pixels += pixels as u64;
        self.telemetry.uploaded_bytes += (pixels * 4 + jobs.len() * 4 + 64) as u64;
        self.telemetry.downloaded_bytes += (pixels * 12 + 64) as u64;
        self.telemetry.batch_wall_seconds += start.elapsed().as_secs_f64();
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn shader_parses_and_validates_without_gpu_hardware() {
        let module = naga::front::wgsl::parse_str(super::SHADER).expect("WGSL syntax");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL validation");
        let plans = vec![0; 4 * 4 * 8 * 68];
        let generated = super::specialized_shader(&plans);
        let module = naga::front::wgsl::parse_str(&generated).expect("specialized WGSL syntax");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("specialized WGSL validation");
    }
}
