//! Per-pixel pixel rejection and weighted mean on the GPU (ADR-090). The CPU
//! keeps reconstruction, geometry and photometry; this reducer receives one
//! output tile of registered samples and returns per-channel results.
//!
//! The tile is processed in pixel chunks through two upload slots: while the
//! device reduces one chunk, CPU threads write the next chunk's samples as one
//! contiguous run of all frames per pixel channel, directly into mapped device
//! memory where the adapter allows it (integrated GPUs share system memory).
use serde::Serialize;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use super::{GpuResult, cancelled};

pub const SHADER: &str = include_str!("reduce.wgsl");

/// Rejection method, with the same meaning as the CPU reducer's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Method {
    Winsorized { sigma_low: f32, sigma_high: f32 },
    Median,
    LinearFit,
    Esd,
}
impl Method {
    fn index(self) -> u32 {
        match self {
            Method::Winsorized { .. } => 0,
            Method::Median => 1,
            Method::LinearFit => 2,
            Method::Esd => 3,
        }
    }
}

/// Result of one output pixel channel.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reduced {
    pub value: f32,
    pub rejected_low: u32,
    pub rejected_high: u32,
    /// Summed frame weight of the survivors (all finite samples for median).
    pub surviving_weight: f32,
    /// Finite samples.
    pub samples: u32,
    /// Summed frame weight of the finite samples.
    pub covered_weight: f32,
}

#[derive(Clone, Default, Debug, Serialize)]
pub struct GpuReductionTelemetry {
    pub adapter: String,
    pub backend: String,
    pub device_type: String,
    pub driver: String,
    pub initialization_seconds: f64,
    /// Samples written straight into mapped device memory (no staging copy).
    pub mapped_uploads: bool,
    pub sort_size: u32,
    pub workgroup_threads: u32,
    pub chunks: u64,
    pub reduced_pixel_channels: u64,
    pub uploaded_bytes: u64,
    pub downloaded_bytes: u64,
    /// Time spent writing samples (overlaps device work of the other slot).
    pub upload_seconds: f64,
    /// Time spent waiting for device results.
    pub gpu_wait_seconds: f64,
    pub peak_reserved_buffer_bytes: u64,
    pub numeric_policy: &'static str,
}

/// Threads per reduction workgroup at most.
const MAX_THREADS: u32 = 256;
/// Upper bound of one upload slot; the tile is processed in pixel chunks.
const CHUNK_BYTES: u64 = 128 << 20;
const RESULT_WORDS: usize = 6;
/// CPU threads writing an upload slot, and pixels per contiguous write block.
const PACK_THREADS: usize = 4;
const PACK_PIXELS: usize = 16;

/// Sort size and threads for `frames` frames if one workgroup can hold them:
/// keys, ESD critical values and (counted always, used by linear fit) frame
/// indices for a power-of-two sort, plus three partial sums per thread.
fn sort_plan(frames: usize, workgroup_bytes: u32, max_threads: u32) -> Option<(u32, u32)> {
    let sort = frames.max(2).next_power_of_two() as u32;
    let threads = (sort / 2).clamp(32, MAX_THREADS).min(max_threads);
    let needed = sort as u64 * 10 + threads as u64 * 12 + 64;
    (needed <= workgroup_bytes as u64).then_some((sort, threads))
}

pub fn specialized_shader(sort: u32, threads: u32, method: Method) -> String {
    let slots = if method == Method::LinearFit { sort } else { 1 };
    SHADER
        .replace("{{SLOTS}}", &slots.to_string())
        .replace("{{P}}", &sort.to_string())
        .replace("{{WG}}", &threads.to_string())
        .replace("{{METHOD}}", &method.index().to_string())
}

/// Writes pixels `start..start + n` of every frame as runs: for each pixel
/// and channel, the samples of all frames in frame order.
fn pack(frames: &[&[[f32; 3]]], start: usize, n: usize, out: &mut [f32]) {
    let count = frames.len();
    let block = PACK_PIXELS * 3 * count;
    let out = &mut out[..n * 3 * count];
    let per_thread = n.div_ceil(PACK_THREADS).div_ceil(PACK_PIXELS) * block;
    std::thread::scope(|scope| {
        for (t, part) in out.chunks_mut(per_thread.max(block)).enumerate() {
            let first = start + t * per_thread / (3 * count);
            scope.spawn(move || {
                // Scatter in a cache-resident block, then write it in one
                // sequential copy: mapped device memory is often write-combined.
                let mut local = vec![0_f32; block];
                for (b, runs) in part.chunks_mut(block).enumerate() {
                    let p0 = first + b * PACK_PIXELS;
                    let pixels = runs.len() / (3 * count);
                    let local = &mut local[..runs.len()];
                    for (f, frame) in frames.iter().enumerate() {
                        for (i, s) in frame[p0..p0 + pixels].iter().enumerate() {
                            let o = i * 3 * count + f;
                            local[o] = s[0];
                            local[o + count] = s[1];
                            local[o + 2 * count] = s[2];
                        }
                    }
                    runs.copy_from_slice(local);
                }
            });
        }
    });
}

struct Pipeline {
    key: (u32, u32, u32),
    pipeline: wgpu::ComputePipeline,
}
/// One upload slot: samples, parameters, results and their readback.
struct Slot {
    samples: wgpu::Buffer,
    params: wgpu::Buffer,
    results: wgpu::Buffer,
    readback: wgpu::Buffer,
    group: wgpu::BindGroup,
}
struct Buffers {
    frames: usize,
    lambda_values: usize,
    weights: wgpu::Buffer,
    slots: [Slot; 2],
}

pub struct GpuReducer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    pipeline: Option<Pipeline>,
    buffers: Option<Buffers>,
    mapped: bool,
    staging: Vec<f32>,
    max_chunk_bytes: u64,
    telemetry: GpuReductionTelemetry,
}

/// Maps `slice`. A buffer the device still uses needs `wait` (all submitted
/// work completes); an idle one is mapped without waiting for other work.
fn wait_map(
    device: &wgpu::Device,
    slice: wgpu::BufferSlice<'_>,
    mode: wgpu::MapMode,
    wait: bool,
) -> GpuResult<()> {
    let (send, receive) = std::sync::mpsc::channel();
    slice.map_async(mode, move |result| {
        let _ = send.send(result);
    });
    let deadline = Instant::now() + std::time::Duration::from_secs(60);
    loop {
        device
            .poll(if wait {
                wgpu::PollType::Wait
            } else {
                wgpu::PollType::Poll
            })
            .map_err(|e| format!("GPU device polling failed: {e}"))?;
        match receive.try_recv() {
            Ok(result) => return result.map_err(|e| format!("GPU mapping failed: {e}")),
            Err(std::sync::mpsc::TryRecvError::Empty) if Instant::now() < deadline => {
                std::thread::yield_now()
            }
            Err(e) => return Err(format!("GPU mapping did not complete: {e}")),
        }
    }
}

impl GpuReducer {
    pub fn new() -> GpuResult<Self> {
        let start = Instant::now();
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
        let supported = adapter.limits();
        let limits = wgpu::Limits {
            max_compute_workgroup_storage_size: supported.max_compute_workgroup_storage_size,
            max_storage_buffer_binding_size: supported.max_storage_buffer_binding_size,
            max_buffer_size: supported.max_buffer_size,
            max_compute_invocations_per_workgroup: supported.max_compute_invocations_per_workgroup,
            max_compute_workgroup_size_x: supported.max_compute_workgroup_size_x,
            ..wgpu::Limits::default()
        };
        // Mapping a storage buffer avoids the staging copy; worthwhile where
        // device memory is system memory.
        let mapped = info.device_type == wgpu::DeviceType::IntegratedGpu
            && adapter
                .features()
                .contains(wgpu::Features::MAPPABLE_PRIMARY_BUFFERS);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("FulgurStack GPU pixel rejection"),
            required_features: if mapped {
                wgpu::Features::MAPPABLE_PRIMARY_BUFFERS
            } else {
                wgpu::Features::empty()
            },
            required_limits: limits,
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| format!("GPU initialization failed: {e}"))?;
        let entries: Vec<_> = (0..6)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: if binding == 5 {
                        wgpu::BufferBindingType::Uniform
                    } else {
                        wgpu::BufferBindingType::Storage {
                            read_only: binding != 4,
                        }
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pixel rejection buffers"),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pixel rejection layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let device_limits = device.limits();
        let max_chunk_bytes = CHUNK_BYTES
            .min(device_limits.max_storage_buffer_binding_size as u64)
            .min(device_limits.max_buffer_size);
        let telemetry = GpuReductionTelemetry {
            adapter: info.name,
            backend: format!("{:?}", info.backend),
            device_type: format!("{:?}", info.device_type),
            driver: format!("{} {}", info.driver, info.driver_info),
            initialization_seconds: start.elapsed().as_secs_f64(),
            mapped_uploads: mapped,
            numeric_policy: "f32 samples/weights, workgroup bitonic sort, f32 Kahan sums around the sample median; no f16",
            ..Default::default()
        };
        Ok(Self {
            device,
            queue,
            layout,
            pipeline_layout,
            pipeline: None,
            buffers: None,
            mapped,
            staging: Vec::new(),
            max_chunk_bytes,
            telemetry,
        })
    }

    pub fn telemetry(&self) -> GpuReductionTelemetry {
        self.telemetry.clone()
    }

    /// Largest frame count this device can reduce.
    pub fn max_frames(&self) -> usize {
        let limits = self.device.limits();
        let mut frames = 2;
        while sort_plan(
            frames * 2,
            limits.max_compute_workgroup_storage_size,
            limits.max_compute_invocations_per_workgroup,
        )
        .is_some()
        {
            frames *= 2;
        }
        frames
    }

    /// Pixels per upload slot for `frames` frames.
    pub fn chunk_pixels(&self, frames: usize) -> usize {
        ((self.max_chunk_bytes / (frames.max(1) as u64 * 12)) as usize).max(1)
    }

    /// Device memory reserved for a stack of `frames` frames (for planning;
    /// on an integrated GPU it is system memory), plus the CPU staging copy
    /// where uploads are not mapped.
    pub fn reserved_bytes(&self, frames: usize, lambda_values: usize) -> u64 {
        let chunk = self.chunk_pixels(frames) as u64;
        let samples = chunk * frames as u64 * 12;
        let slot = samples + 2 * chunk * 3 * RESULT_WORDS as u64 * 4 + 64;
        2 * slot
            + if self.mapped { 0 } else { samples }
            + (lambda_values + 2 * frames + 1) as u64 * 4
    }

    fn prepare(&mut self, frames: usize, method: Method) -> GpuResult<()> {
        let limits = self.device.limits();
        let (sort, threads) = sort_plan(
            frames,
            limits.max_compute_workgroup_storage_size,
            limits.max_compute_invocations_per_workgroup,
        )
        .ok_or_else(|| {
            format!(
                "{frames} frames exceed the GPU workgroup memory (at most {} frames); use --engine cpu.",
                self.max_frames()
            )
        })?;
        let key = (sort, threads, method.index());
        if self.pipeline.as_ref().is_none_or(|p| p.key != key) {
            self.device.push_error_scope(wgpu::ErrorFilter::Validation);
            let module = self
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("pixel rejection"),
                    source: wgpu::ShaderSource::Wgsl(
                        specialized_shader(sort, threads, method).into(),
                    ),
                });
            let pipeline = self
                .device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("reduce"),
                    layout: Some(&self.pipeline_layout),
                    module: &module,
                    entry_point: Some("reduce"),
                    compilation_options: Default::default(),
                    cache: None,
                });
            if let Some(e) = pollster::block_on(self.device.pop_error_scope()) {
                return Err(format!("GPU shader validation failed: {e}"));
            }
            self.pipeline = Some(Pipeline { key, pipeline });
        }
        self.telemetry.sort_size = sort;
        self.telemetry.workgroup_threads = threads;
        Ok(())
    }

    fn allocate(&mut self, frames: usize, lambda: &[f32], offsets: &[u32]) {
        if self
            .buffers
            .as_ref()
            .is_some_and(|b| b.frames == frames && b.lambda_values == lambda.len())
        {
            return;
        }
        let chunk = self.chunk_pixels(frames);
        let make = |label, size: u64, usage| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size.max(16),
                usage,
                mapped_at_creation: false,
            })
        };
        let storage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        let sample_bytes = (chunk * frames * 12) as u64;
        let result_bytes = (chunk * 3 * RESULT_WORDS * 4) as u64;
        let weights = make("frame weights", (frames * 4) as u64, storage);
        let lambda_buffer = make("ESD critical values", (lambda.len() * 4) as u64, storage);
        let offsets_buffer = make("ESD offsets", (offsets.len() * 4) as u64, storage);
        self.queue
            .write_buffer(&lambda_buffer, 0, bytemuck::cast_slice(lambda));
        self.queue
            .write_buffer(&offsets_buffer, 0, bytemuck::cast_slice(offsets));
        let sample_usage = if self.mapped {
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::MAP_WRITE
        } else {
            storage
        };
        let slot = |_| {
            let samples = make("per-channel sample runs", sample_bytes, sample_usage);
            let params = make(
                "reduction parameters",
                32,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            );
            let results = make(
                "per-channel results",
                result_bytes,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            );
            let readback = make(
                "result readback",
                result_bytes,
                wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            );
            let all = [
                &samples,
                &weights,
                &lambda_buffer,
                &offsets_buffer,
                &results,
                &params,
            ];
            let entries: Vec<_> = all
                .iter()
                .enumerate()
                .map(|(i, b)| wgpu::BindGroupEntry {
                    binding: i as u32,
                    resource: b.as_entire_binding(),
                })
                .collect();
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("pixel rejection"),
                layout: &self.layout,
                entries: &entries,
            });
            Slot {
                samples,
                params,
                results,
                readback,
                group,
            }
        };
        let slots = [slot(0), slot(1)];
        self.telemetry.peak_reserved_buffer_bytes = self
            .telemetry
            .peak_reserved_buffer_bytes
            .max(self.reserved_bytes(frames, lambda.len()));
        self.buffers = Some(Buffers {
            frames,
            lambda_values: lambda.len(),
            weights,
            slots,
        });
    }

    /// Reduce one tile. `frames[f][p]` is the registered RGB sample of frame
    /// `f` at tile pixel `p` (NaN where invalid); every frame has the same
    /// length. `lambda`/`offsets` hold the ESD critical values per stack size
    /// (`lambda[offsets[n] + step]`) and are ignored by the other methods.
    pub fn reduce(
        &mut self,
        frames: &[&[[f32; 3]]],
        weights: &[f32],
        method: Method,
        lambda: &[f32],
        offsets: &[u32],
        cancel: &AtomicBool,
    ) -> GpuResult<Vec<[Reduced; 3]>> {
        cancelled(cancel)?;
        let count = frames.len();
        let pixels = frames.first().map_or(0, |f| f.len());
        if count == 0 || weights.len() != count || frames.iter().any(|f| f.len() != pixels) {
            return Err("Invalid GPU reduction tile.".into());
        }
        if offsets.len() <= count + 1 {
            return Err("ESD offsets must cover every stack size.".into());
        }
        self.prepare(count, method)?;
        self.allocate(count, lambda, offsets);
        let chunk = self.chunk_pixels(count);
        let (sigma_low, sigma_high) = match method {
            Method::Winsorized {
                sigma_low,
                sigma_high,
            } => (sigma_low, sigma_high),
            _ => (0., 0.),
        };
        let max_dispatch = self.device.limits().max_compute_workgroups_per_dimension;
        let b = self.buffers.take().unwrap();
        self.queue
            .write_buffer(&b.weights, 0, bytemuck::cast_slice(weights));
        let chunks: Vec<(usize, usize)> = (0..pixels)
            .step_by(chunk)
            .map(|s| (s, chunk.min(pixels - s)))
            .collect();
        let mut output = Vec::with_capacity(pixels);
        let mut pending: Option<(usize, usize)> = None;
        let result = (|| -> GpuResult<()> {
            for (index, &(start, n)) in chunks.iter().enumerate() {
                cancelled(cancel)?;
                let slot = &b.slots[index % 2];
                let upload = Instant::now();
                if self.mapped {
                    // The slot's previous chunk was collected two steps ago.
                    let range = ..(n * count * 12) as u64;
                    wait_map(
                        &self.device,
                        slot.samples.slice(range),
                        wgpu::MapMode::Write,
                        false,
                    )?;
                    {
                        let mut mapped = slot.samples.slice(range).get_mapped_range_mut();
                        pack(frames, start, n, bytemuck::cast_slice_mut(&mut mapped));
                    }
                    slot.samples.unmap();
                } else {
                    self.staging.resize(n * count * 3, 0.);
                    pack(frames, start, n, &mut self.staging);
                    self.queue
                        .write_buffer(&slot.samples, 0, bytemuck::cast_slice(&self.staging));
                }
                let jobs = (n * 3) as u32;
                let dispatch_x = jobs.min(max_dispatch);
                let mut params = [0u32; 8];
                params[0] = n as u32;
                params[1] = count as u32;
                params[2] = dispatch_x;
                params[4] = sigma_low.to_bits();
                params[5] = sigma_high.to_bits();
                self.queue
                    .write_buffer(&slot.params, 0, bytemuck::cast_slice(&params));
                self.telemetry.upload_seconds += upload.elapsed().as_secs_f64();
                let mut encoder =
                    self.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("pixel rejection"),
                        });
                {
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("reduce"),
                        timestamp_writes: None,
                    });
                    pass.set_pipeline(&self.pipeline.as_ref().unwrap().pipeline);
                    pass.set_bind_group(0, &slot.group, &[]);
                    pass.dispatch_workgroups(dispatch_x, jobs.div_ceil(dispatch_x), 1);
                }
                let result_bytes = (n * 3 * RESULT_WORDS * 4) as u64;
                encoder.copy_buffer_to_buffer(&slot.results, 0, &slot.readback, 0, result_bytes);
                self.queue.submit([encoder.finish()]);
                if let Some(previous) = pending.replace((index, n)) {
                    self.collect(&b, previous, &mut output)?;
                }
                self.telemetry.chunks += 1;
                self.telemetry.reduced_pixel_channels += jobs as u64;
                self.telemetry.uploaded_bytes += (n * count * 12 + 32) as u64;
                self.telemetry.downloaded_bytes += result_bytes;
            }
            if let Some(last) = pending.take() {
                self.collect(&b, last, &mut output)?;
            }
            Ok(())
        })();
        self.buffers = Some(b);
        result?;
        cancelled(cancel)?;
        Ok(output)
    }

    /// Waits for chunk `index` (of `n` pixels) and appends its results.
    fn collect(
        &mut self,
        b: &Buffers,
        (index, n): (usize, usize),
        output: &mut Vec<[Reduced; 3]>,
    ) -> GpuResult<()> {
        let wait = Instant::now();
        let slot = &b.slots[index % 2];
        let range = ..(n * 3 * RESULT_WORDS * 4) as u64;
        wait_map(
            &self.device,
            slot.readback.slice(range),
            wgpu::MapMode::Read,
            true,
        )?;
        {
            let mapped = slot.readback.slice(range).get_mapped_range();
            let words: &[u32] = bytemuck::cast_slice(&mapped);
            for pixel in words.chunks_exact(3 * RESULT_WORDS) {
                output.push(std::array::from_fn(|c| {
                    let w = &pixel[c * RESULT_WORDS..(c + 1) * RESULT_WORDS];
                    Reduced {
                        value: f32::from_bits(w[0]),
                        rejected_low: w[1],
                        rejected_high: w[2],
                        surviving_weight: f32::from_bits(w[3]),
                        samples: w[4],
                        covered_weight: f32::from_bits(w[5]),
                    }
                }));
            }
        }
        slot.readback.unmap();
        self.telemetry.gpu_wait_seconds += wait.elapsed().as_secs_f64();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduction_shader_parses_and_validates_for_every_method_and_size() {
        for method in [
            Method::Winsorized {
                sigma_low: 3.,
                sigma_high: 2.5,
            },
            Method::Median,
            Method::LinearFit,
            Method::Esd,
        ] {
            for (sort, threads) in [(32, 32), (1024, 256), (4096, 256)] {
                let source = specialized_shader(sort, threads, method);
                let module = naga::front::wgsl::parse_str(&source).expect("WGSL syntax");
                naga::valid::Validator::new(
                    naga::valid::ValidationFlags::all(),
                    naga::valid::Capabilities::empty(),
                )
                .validate(&module)
                .expect("WGSL validation");
            }
        }
    }

    #[test]
    fn sort_plans_fit_workgroup_memory() {
        assert_eq!(sort_plan(27, 16384, 256), Some((32, 32)));
        assert_eq!(sort_plan(856, 16384, 256), Some((1024, 256)));
        assert_eq!(sort_plan(1140, 16384, 256), None);
        assert_eq!(sort_plan(1140, 32768, 256), Some((2048, 256)));
    }

    #[test]
    fn packing_writes_one_run_of_all_frames_per_pixel_channel() {
        let frames: Vec<Vec<[f32; 3]>> = (0..5)
            .map(|f| {
                (0..70)
                    .map(|p| std::array::from_fn(|c| (f * 1000 + p * 10 + c) as f32))
                    .collect()
            })
            .collect();
        let views: Vec<&[[f32; 3]]> = frames.iter().map(|f| f.as_slice()).collect();
        let mut out = vec![0.; 50 * 3 * 5 + 7];
        pack(&views, 13, 50, &mut out);
        for p in 0..50 {
            for c in 0..3 {
                for f in 0..5 {
                    assert_eq!(
                        out[(p * 3 + c) * 5 + f],
                        (f * 1000 + (13 + p) * 10 + c) as f32
                    );
                }
            }
        }
        assert!(out[750..].iter().all(|&v| v == 0.));
    }
}
