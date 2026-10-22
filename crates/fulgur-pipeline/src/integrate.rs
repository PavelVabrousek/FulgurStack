//! Output-tile-major CPU integration of immutable CFA source ROIs.
use crate::register::{Canvas, RegistrationReport, Transform};
pub use crate::tile_plan::MemoryPlan;
use fulgur_core::fits::{FitsHeader, read_header};
use fulgur_core::fits_cache::{CacheStats, FitsBlockCache};
use fulgur_core::{DemosaicMethod, Error, Result, RgbImage, check_cancelled};
use rayon::prelude::*;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TileMode {
    Auto,
    Manual,
}

#[derive(Clone, Debug, Serialize)]
pub struct IntegrationConfig {
    pub sigma_low: f64,
    pub sigma_high: f64,
    pub memory_budget_mib: usize,
    pub fits_cache_mib: usize,
    pub tile_mode: TileMode,
    pub tile_width: usize,
    pub tile_height: usize,
}
impl Default for IntegrationConfig {
    fn default() -> Self {
        Self {
            sigma_low: 3.,
            sigma_high: 2.5,
            memory_budget_mib: 12288,
            fits_cache_mib: 0,
            tile_mode: TileMode::Auto,
            tile_width: 512,
            tile_height: 512,
        }
    }
}
impl IntegrationConfig {
    pub fn validate(&self) -> Result<()> {
        if !self.sigma_low.is_finite()
            || !self.sigma_high.is_finite()
            || self.sigma_low <= 0.
            || self.sigma_high <= 0.
            || !(1024..=12288).contains(&self.memory_budget_mib)
            || self.fits_cache_mib > 1024
            || self.tile_width == 0
            || self.tile_width > 1024
            || self.tile_height == 0
            || self.tile_height > 512
        {
            return Err(Error::Invalid(
                "Invalid clipping, tile geometry or memory budget (1024..12288 MiB).",
            ));
        }
        Ok(())
    }
}
pub struct IntegrationSource {
    pub path: PathBuf,
    pub header: FitsHeader,
    pub transform: Transform,
    pub scale: f64,
    pub offset: f64,
    pub weight: f64,
}
#[derive(Default, Debug, Serialize)]
pub struct IntegrationStats {
    pub backend: &'static str,
    #[cfg(feature = "gpu")]
    pub gpu: Option<fulgur_gpu::GpuTelemetry>,
    pub wall_seconds: f64,
    /// Non-overlapping wall time for tile reconstruction batches, including scheduling and I/O.
    pub reconstruction_wall_seconds: f64,
    /// Non-overlapping wall time for sample gathering, clipping, accumulation and map generation.
    pub reduction_wall_seconds: f64,
    pub output_copy_wall_seconds: f64,
    /// Sums of elapsed ROI task times. Tasks overlap; these are neither CPU times nor wall partitions.
    pub roi_task_elapsed: RoiTaskElapsed,
    pub tile_count: usize,
    pub tile_width: usize,
    pub tile_height: usize,
    pub max_concurrent_rois: usize,
    pub planned_buffer_bytes: usize,
    pub memory_plan: Option<MemoryPlan>,
    pub fits_cache: CacheStats,
    pub peak_rss_bytes: Option<u64>,
    pub decoded_roi_pixels: u64,
    pub corrected_roi_pixels: u64,
    pub valid_channel_samples: u64,
    pub rejected_low_channel_samples: u64,
    pub rejected_high_channel_samples: u64,
    pub zero_coverage_pixels: u64,
    pub min_valid_frames: usize,
    pub max_valid_frames: usize,
    pub minimum_geometric_weight_fraction: f64,
    pub minimum_surviving_weight_fraction: f64,
    pub intermediate_images_written: usize,
}
#[derive(Default, Debug, Serialize)]
pub struct RoiTaskElapsed {
    pub read_seconds: f64,
    pub cosmetic_seconds: f64,
    pub demosaic_seconds: f64,
    pub resampling_seconds: f64,
}
impl RoiTaskElapsed {
    fn add(&mut self, other: Self) {
        self.read_seconds += other.read_seconds;
        self.cosmetic_seconds += other.cosmetic_seconds;
        self.demosaic_seconds += other.demosaic_seconds;
        self.resampling_seconds += other.resampling_seconds;
    }
}
/// Integration boundary consumes raw source views and geometry/photometry, never registered RGB frames.
pub struct IntegrationResult {
    pub image: RgbImage,
    /// Per-channel surviving sample count / original input count, not a weight or exposure map.
    pub coverage: RgbImage,
    /// Per-channel upper rejected sample count / original input count.
    pub high_rejection: RgbImage,
    pub stats: IntegrationStats,
}
pub trait IntegrationEngine {
    fn integrate(
        &self,
        sources: &[IntegrationSource],
        canvas: &Canvas,
        config: &IntegrationConfig,
        cancel: &AtomicBool,
    ) -> Result<(RgbImage, IntegrationStats)> {
        let result = self.integrate_with_maps(sources, canvas, config, sources.len(), cancel)?;
        Ok((result.image, result.stats))
    }
    fn integrate_with_maps(
        &self,
        sources: &[IntegrationSource],
        canvas: &Canvas,
        config: &IntegrationConfig,
        original_input_count: usize,
        cancel: &AtomicBool,
    ) -> Result<IntegrationResult>;
}
pub struct CpuIntegrationEngine;

enum ReconstructionEngine {
    Cpu,
    #[cfg(feature = "gpu")]
    Gpu(Box<std::sync::Mutex<fulgur_gpu::GpuReconstructor>>),
}
#[cfg(feature = "gpu")]
pub struct GpuIntegrationEngine {
    backend: ReconstructionEngine,
}
#[cfg(feature = "gpu")]
impl GpuIntegrationEngine {
    pub fn new() -> Result<Self> {
        let gpu = fulgur_gpu::GpuReconstructor::new(&crate::vng::gpu_plans()).map_err(gpu_error)?;
        Ok(Self {
            backend: ReconstructionEngine::Gpu(Box::new(std::sync::Mutex::new(gpu))),
        })
    }
}
#[cfg(feature = "gpu")]
fn gpu_error(error: String) -> Error {
    Error::Io(std::io::Error::other(error))
}
#[cfg(feature = "gpu")]
impl IntegrationEngine for GpuIntegrationEngine {
    fn integrate_with_maps(
        &self,
        sources: &[IntegrationSource],
        canvas: &Canvas,
        config: &IntegrationConfig,
        original_input_count: usize,
        cancel: &AtomicBool,
    ) -> Result<IntegrationResult> {
        integrate_common(
            sources,
            canvas,
            config,
            original_input_count,
            cancel,
            &self.backend,
        )
    }
}
pub fn sources(input: &Path, report: &RegistrationReport) -> Result<Vec<IntegrationSource>> {
    let root = if input.is_dir() {
        input
    } else {
        input.parent().unwrap_or(Path::new("."))
    };
    report
        .frames
        .iter()
        .filter(|f| f.accepted)
        .map(|f| {
            let path = root.join(&f.file_name);
            let header = read_header(&path)?;
            Ok(IntegrationSource {
                path,
                header,
                transform: f
                    .transform
                    .ok_or(Error::Invalid("Missing accepted transform."))?,
                scale: f
                    .photometric_scale
                    .ok_or(Error::Invalid("Missing accepted photometry."))?,
                offset: f
                    .background_offset_adu
                    .ok_or(Error::Invalid("Missing accepted sky offset."))?,
                weight: f.final_weight,
            })
        })
        .collect()
}
fn bounds(
    t: Transform,
    origin: [i32; 2],
    size: [usize; 2],
    sensor: [usize; 2],
) -> Option<[usize; 4]> {
    let x = origin[0] as f64;
    let y = origin[1] as f64;
    let w = (size[0] - 1) as f64;
    let h = (size[1] - 1) as f64;
    let corners = [[x, y], [x + w, y], [x + w, y + h], [x, y + h]].map(|p| t.inverse().apply(p));
    let minx = corners
        .iter()
        .map(|p| p[0])
        .fold(f64::INFINITY, f64::min)
        .floor()
        - 8.;
    let miny = corners
        .iter()
        .map(|p| p[1])
        .fold(f64::INFINITY, f64::min)
        .floor()
        - 8.;
    let maxx = corners
        .iter()
        .map(|p| p[0])
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil()
        + 9.;
    let maxy = corners
        .iter()
        .map(|p| p[1])
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil()
        + 9.;
    let x0 = (minx.max(0.).min(sensor[0] as f64) as usize) / 2 * 2;
    let y0 = (miny.max(0.).min(sensor[1] as f64) as usize) / 2 * 2;
    let x1 = (maxx.max(0.) as usize).min(sensor[0]);
    let y1 = (maxy.max(0.) as usize).min(sensor[1]);
    if x1 < x0 + 6 || y1 < y0 + 6 {
        None
    } else {
        Some([x0, y0, x1 - x0, y1 - y0])
    }
}
#[cfg(test)]
fn keys(x: f64) -> f32 {
    let x = x.abs();
    if x < 1. {
        ((1.5 * x - 2.5) * x * x + 1.) as f32
    } else if x < 2. {
        ((-0.5 * x + 2.5) * x * x - 4. * x + 2.) as f32
    } else {
        0.
    }
}
/// Exact Keys polynomials for the known positions of a positive 4-pixel footprint.
fn keys_weights(coordinate: f64, base: usize) -> [f32; 4] {
    let d: [f64; 4] = std::array::from_fn(|i| (coordinate - (base + i) as f64 + 1.).abs());
    let inner = |x: f64| ((1.5 * x - 2.5) * x * x + 1.) as f32;
    let outer = |x: f64| ((-0.5 * x + 2.5) * x * x - 4. * x + 2.) as f32;
    [outer(d[0]), inner(d[1]), inner(d[2]), outer(d[3])]
}
/// Return None for an incomplete sensor footprint, never pad/blend invalid samples.
fn sample(rgb: &RgbImage, roi: [usize; 4], sensor: [usize; 2], p: [f64; 2]) -> Option<[f32; 3]> {
    if p[0] < 1. || p[1] < 1. || p[0] >= (sensor[0] - 2) as f64 || p[1] >= (sensor[1] - 2) as f64 {
        return None;
    }
    // On valid positive coordinates truncation is exactly floor.
    let x = p[0] as usize;
    let y = p[1] as usize;
    if x < roi[0] + 1 || y < roi[1] + 1 || x + 2 >= roi[0] + roi[2] || y + 2 >= roi[1] + roi[3] {
        return None;
    }
    let wx = keys_weights(p[0], x);
    let wy = keys_weights(p[1], y);
    let mut result = [0.; 3];
    let first = (y - 1 - roi[1]) * rgb.width + x - 1 - roi[0];
    for (j, yy) in wy.iter().enumerate() {
        let row_start = first + j * rgb.width;
        let row: &[[f32; 3]; 4] = rgb.pixels[row_start..row_start + 4].try_into().unwrap();
        for (value, xx) in row.iter().zip(wx) {
            let w = xx * yy;
            // Keep all sixteen row-major f32 contributions in the original order.
            result[0] += w * value[0];
            result[1] += w * value[1];
            result[2] += w * value[2];
        }
    }
    Some(result)
}
type WarpedTile = (Vec<[f32; 3]>, u64, u64, RoiTaskElapsed);
fn warp(
    source: &IntegrationSource,
    origin: [i32; 2],
    size: [usize; 2],
    cancel: &AtomicBool,
    cache: &FitsBlockCache,
) -> Result<WarpedTile> {
    let sensor = [source.header.info.width, source.header.info.height];
    let Some(roi) = bounds(source.transform, origin, size, sensor) else {
        return Ok((
            vec![[f32::NAN; 3]; size[0] * size[1]],
            0,
            0,
            RoiTaskElapsed::default(),
        ));
    };
    let read_start = Instant::now();
    let raw = cache.read_roi(&source.path, &source.header, roi, cancel)?;
    let read_seconds = read_start.elapsed().as_secs_f64();
    let (rgb, stats) = crate::debayer::debayer(&raw, cancel, DemosaicMethod::Vng)?;
    let warp_start = Instant::now();
    let output = warp_rgb(source, origin, size, roi, &rgb, cancel)?;
    Ok((
        output,
        (roi[2] * roi[3]) as u64,
        stats.corrected_pixels as u64,
        RoiTaskElapsed {
            read_seconds,
            cosmetic_seconds: stats.cosmetic_seconds,
            demosaic_seconds: stats.demosaic_seconds,
            resampling_seconds: warp_start.elapsed().as_secs_f64(),
        },
    ))
}
fn warp_rgb(
    source: &IntegrationSource,
    origin: [i32; 2],
    size: [usize; 2],
    roi: [usize; 4],
    rgb: &RgbImage,
    cancel: &AtomicBool,
) -> Result<Vec<[f32; 3]>> {
    let sensor = [source.header.info.width, source.header.info.height];
    let mut output = vec![[f32::NAN; 3]; size[0] * size[1]];
    let inverse = source.transform.inverse();
    let (s, c) = inverse.theta_rad.sin_cos();
    // Reuse exact coordinate products, never stepped/rounded source coordinates.
    // Apply scale and translation in Transform::apply's original arithmetic order.
    let columns: Vec<_> = (0..size[0])
        .map(|x| {
            let qx = (origin[0] + x as i32) as f64;
            [c * qx, s * qx]
        })
        .collect();
    for (y, row) in output.chunks_exact_mut(size[0]).enumerate() {
        check_cancelled(cancel)?;
        let qy = (origin[1] + y as i32) as f64;
        let sy = s * qy;
        let cy = c * qy;
        for (pixel, &[cx, sx]) in row.iter_mut().zip(&columns) {
            let p = [
                inverse.scale_factor * (cx - sy) + inverse.tx_px,
                inverse.scale_factor * (sx + cy) + inverse.ty_px,
            ];
            if let Some(v) = sample(rgb, roi, sensor, p) {
                let normalized = v.map(|v| (source.scale * v as f64 + source.offset) as f32);
                if normalized.iter().any(|v| !v.is_finite()) {
                    return Err(Error::Invalid("Nonfinite normalized integration sample."));
                }
                *pixel = normalized;
            }
        }
    }
    Ok(output)
}
#[cfg(feature = "gpu")]
fn gpu_batch(
    gpu: &std::sync::Mutex<fulgur_gpu::GpuReconstructor>,
    sources: &[IntegrationSource],
    origin: [i32; 2],
    size: [usize; 2],
    cancel: &AtomicBool,
    cache: &FitsBlockCache,
) -> Result<Vec<WarpedTile>> {
    let prepared = sources
        .par_iter()
        .map(|source| {
            let start = Instant::now();
            let sensor = [source.header.info.width, source.header.info.height];
            let data = if let Some(roi) = bounds(source.transform, origin, size, sensor) {
                Some((
                    roi,
                    cache.read_roi(&source.path, &source.header, roi, cancel)?,
                ))
            } else {
                None
            };
            Ok((data, start.elapsed().as_secs_f64()))
        })
        .collect::<Result<Vec<_>>>()?;
    let raw: Vec<_> = prepared
        .iter()
        .filter_map(|(p, _)| p.as_ref().map(|(_, f)| f))
        .collect();
    let mut gpu = gpu
        .lock()
        .map_err(|_| Error::Invalid("GPU context lock failed."))?;
    let mut reconstructed = gpu
        .reconstruct(&raw, cancel)
        .map_err(|e| {
            if check_cancelled(cancel).is_err() {
                Error::Cancelled
            } else {
                gpu_error(e)
            }
        })?
        .into_iter();
    drop(raw);
    drop(gpu);
    let rgb: Vec<_> = prepared
        .iter()
        .map(|(p, _)| p.as_ref().map(|_| reconstructed.next().unwrap()))
        .collect();
    prepared
        .into_par_iter()
        .zip(sources.par_iter())
        .zip(rgb)
        .map(|(((data, read_seconds), source), rgb)| {
            if let (Some((roi, _raw)), Some((rgb, corrected))) = (data, rgb) {
                let start = Instant::now();
                let output = warp_rgb(source, origin, size, roi, &rgb, cancel)?;
                Ok((
                    output,
                    (roi[2] * roi[3]) as u64,
                    corrected as u64,
                    RoiTaskElapsed {
                        read_seconds,
                        resampling_seconds: start.elapsed().as_secs_f64(),
                        ..Default::default()
                    },
                ))
            } else {
                Ok((
                    vec![[f32::NAN; 3]; size[0] * size[1]],
                    0,
                    0,
                    RoiTaskElapsed::default(),
                ))
            }
        })
        .collect()
}
#[derive(Default)]
struct Sum {
    total: f64,
    correction: f64,
}
impl Sum {
    fn add(&mut self, x: f64) {
        let next = self.total + x;
        self.correction += if self.total.abs() >= x.abs() {
            (self.total - next) + x
        } else {
            (x - next) + self.total
        };
        self.total = next;
    }
    fn value(&self) -> f64 {
        self.total + self.correction
    }
}
fn median(values: &mut [f32]) -> f64 {
    let n = values.len();
    let mid = n / 2;
    let (lo, m, _) = values.select_nth_unstable_by(mid, f32::total_cmp);
    if n % 2 == 0 {
        (f64::from(*m) + f64::from(lo.iter().copied().fold(f32::NEG_INFINITY, f32::max))) * 0.5
    } else {
        f64::from(*m)
    }
}
/// Row-local storage reused across pixels, channels and clipping iterations.
struct ClippingScratch {
    winsorized: Vec<f32>,
    deviations: Vec<f32>,
}
impl ClippingScratch {
    fn with_capacity(samples: usize) -> Self {
        Self {
            winsorized: Vec::with_capacity(samples),
            deviations: Vec::with_capacity(samples),
        }
    }
}
/// Median/MAD winsorized estimates; reject original excursions, average original survivors.
fn combine(
    values: &[(f32, usize)],
    weights: &[f64],
    scratch: &mut ClippingScratch,
    config: &IntegrationConfig,
) -> (f32, u64, u64, f64) {
    if values.is_empty() {
        return (0., 0, 0, 0.);
    }
    let mut low = f64::NEG_INFINITY;
    let mut high = f64::INFINITY;
    if values.len() >= 5 {
        let ClippingScratch {
            winsorized,
            deviations,
        } = scratch;
        winsorized.clear();
        winsorized.extend(values.iter().map(|v| v.0));
        for _ in 0..3 {
            let center = median(winsorized);
            deviations.clear();
            deviations.extend(winsorized.iter().map(|v| (*v as f64 - center).abs() as f32));
            let sigma = (1.4826 * median(deviations)).max(1e-6);
            low = center - config.sigma_low * sigma;
            high = center + config.sigma_high * sigma;
            let mut changed = false;
            for x in winsorized.iter_mut() {
                let next = (*x as f64).clamp(low, high) as f32;
                changed |= next != *x;
                *x = next;
            }
            if !changed {
                break;
            }
        }
    }
    let mut numerator = Sum::default();
    let mut denominator = Sum::default();
    let mut rejected_low = 0;
    let mut rejected_high = 0;
    for &(v, index) in values {
        if (v as f64) < low {
            rejected_low += 1;
        } else if (v as f64) > high {
            rejected_high += 1;
        } else {
            numerator.add(v as f64 * weights[index]);
            denominator.add(weights[index]);
        }
    }
    let weight = denominator.value();
    if weight == 0. {
        (0., rejected_low, rejected_high, 0.)
    } else {
        (
            (numerator.value() / weight) as f32,
            rejected_low,
            rejected_high,
            weight,
        )
    }
}
#[derive(Default)]
struct RowStats {
    valid: u64,
    low: u64,
    high: u64,
    zero: u64,
    min_n: usize,
    max_n: usize,
    min_fraction: f64,
    min_surviving: f64,
}
impl IntegrationEngine for CpuIntegrationEngine {
    fn integrate_with_maps(
        &self,
        sources: &[IntegrationSource],
        canvas: &Canvas,
        config: &IntegrationConfig,
        original_input_count: usize,
        cancel: &AtomicBool,
    ) -> Result<IntegrationResult> {
        integrate_common(
            sources,
            canvas,
            config,
            original_input_count,
            cancel,
            &ReconstructionEngine::Cpu,
        )
    }
}
fn integrate_common(
    sources: &[IntegrationSource],
    canvas: &Canvas,
    config: &IntegrationConfig,
    original_input_count: usize,
    cancel: &AtomicBool,
    backend: &ReconstructionEngine,
) -> Result<IntegrationResult> {
    config.validate()?;
    check_cancelled(cancel)?;
    if original_input_count < sources.len()
        || original_input_count == 0
        || sources.is_empty()
        || canvas.width == 0
        || canvas.height == 0
        || sources.iter().any(|s| {
            s.weight <= 0.
                || s.weight > 1.
                || s.scale <= 0.
                || !(0.33..=3.).contains(&s.transform.scale_factor)
                || s.header.info.width < 6
                || s.header.info.height < 6
                || ![
                    s.scale,
                    s.offset,
                    s.weight,
                    s.transform.scale_factor,
                    s.transform.theta_rad,
                    s.transform.tx_px,
                    s.transform.ty_px,
                ]
                .iter()
                .all(|v| v.is_finite())
        })
    {
        return Err(Error::Invalid("Invalid integration plan."));
    }
    let start = Instant::now();
    let budget = config.memory_budget_mib * 1024 * 1024;
    let workers = rayon::current_num_threads().min(16);
    let is_gpu = !matches!(backend, ReconstructionEngine::Cpu);
    let mut memory_plan = crate::tile_plan::TileWorkload {
        width: canvas.width,
        height: canvas.height,
        sources: sources.len(),
        workers,
        inverse_scale: sources
            .iter()
            .map(|s| 1. / s.transform.scale_factor)
            .fold(1., f64::max),
        gpu: is_gpu,
    }
    .plan(config, fulgur_core::telemetry::system_memory())?;
    let paths: Vec<_> = sources.iter().map(|s| s.path.clone()).collect();
    let max_width = sources.iter().map(|s| s.header.info.width).max().unwrap();
    memory_plan.allocate_cache(config.fits_cache_mib, sources.len(), workers, max_width);
    let cache = FitsBlockCache::new(&paths, memory_plan.fits_cache_capacity_bytes);
    let tile_width = memory_plan.selected_width;
    let tile_height = memory_plan.selected_height;
    let planned = memory_plan.estimated_buffer_bytes;
    if let Some(memory) = memory_plan.system_memory {
        eprintln!(
            "System RAM before integration: {:.1} MiB available of {:.1} MiB physical memory.",
            memory.available_bytes as f64 / 1048576.,
            memory.total_bytes as f64 / 1048576.
        );
    }
    eprintln!(
        "Tile plan: {:?}, {} x {} px; estimated {:.1} MiB, effective budget {:.1} MiB, system reserve {} MiB.",
        memory_plan.mode,
        tile_width,
        tile_height,
        planned as f64 / 1048576.,
        memory_plan.effective_budget_bytes as f64 / 1048576.,
        memory_plan
            .system_reserve_bytes
            .map(|n| format!("{:.1}", n as f64 / 1048576.))
            .unwrap_or_else(|| "unavailable (manual configured budget only)".into())
    );
    eprintln!(
        "FITS block cache: {:.1} MiB accounted capacity (requested {} MiB).",
        memory_plan.fits_cache_capacity_bytes as f64 / 1048576.,
        config.fits_cache_mib
    );
    let mut image = RgbImage {
        width: canvas.width,
        height: canvas.height,
        demosaic_method: DemosaicMethod::Vng,
        pixels: vec![[0.; 3]; canvas.width * canvas.height],
    };
    let mut coverage = RgbImage {
        width: canvas.width,
        height: canvas.height,
        demosaic_method: DemosaicMethod::Vng,
        pixels: vec![[0.; 3]; canvas.width * canvas.height],
    };
    let mut high_rejection = RgbImage {
        width: canvas.width,
        height: canvas.height,
        demosaic_method: DemosaicMethod::Vng,
        pixels: vec![[0.; 3]; canvas.width * canvas.height],
    };
    let weights: Vec<_> = sources.iter().map(|s| s.weight).collect();
    let mut total_weight = Sum::default();
    for w in &weights {
        total_weight.add(*w);
    }
    let total_weight = total_weight.value();
    let mut stats = IntegrationStats {
        backend: if is_gpu {
            "gpu-cosmetic-vng-cpu-warp-reduction"
        } else {
            "cpu"
        },
        tile_width,
        tile_height,
        max_concurrent_rois: workers.min(sources.len()),
        planned_buffer_bytes: planned,
        memory_plan: Some(memory_plan),
        min_valid_frames: sources.len(),
        minimum_geometric_weight_fraction: 1.,
        minimum_surviving_weight_fraction: 1.,
        ..Default::default()
    };
    let mut progress = Instant::now();
    let total_tiles = canvas.width.div_ceil(tile_width) * canvas.height.div_ceil(tile_height);
    for y0 in (0..canvas.height).step_by(tile_height) {
        for x0 in (0..canvas.width).step_by(tile_width) {
            check_cancelled(cancel)?;
            let size = [
                tile_width.min(canvas.width - x0),
                tile_height.min(canvas.height - y0),
            ];
            let origin = [
                canvas.origin_x_px + x0 as i32,
                canvas.origin_y_px + y0 as i32,
            ];
            let reconstruction_start = Instant::now();
            let mut frames = Vec::with_capacity(sources.len());
            // Bound nested reconstruction work to one batch, independently of total source count.
            for batch in sources.chunks(workers) {
                let buffers = match backend {
                    ReconstructionEngine::Cpu => batch
                        .par_iter()
                        .map(|s| warp(s, origin, size, cancel, &cache))
                        .collect::<Result<Vec<_>>>()?,
                    #[cfg(feature = "gpu")]
                    ReconstructionEngine::Gpu(gpu) => {
                        gpu_batch(gpu, batch, origin, size, cancel, &cache)?
                    }
                };
                for (pixels, decoded, corrected, timing) in buffers {
                    stats.roi_task_elapsed.add(timing);
                    frames.push(pixels);
                    stats.decoded_roi_pixels += decoded;
                    stats.corrected_roi_pixels += corrected;
                }
                if fulgur_core::telemetry::peak_rss_bytes().is_some_and(|v| v > budget as u64) {
                    return Err(Error::Invalid(
                        "Measured process memory exceeded the configured budget.",
                    ));
                }
            }
            stats.reconstruction_wall_seconds += reconstruction_start.elapsed().as_secs_f64();
            let reduction_start = Instant::now();
            let mut tile = vec![([0.; 3], [0.; 3], [0.; 3]); size[0] * size[1]];
            let rows =
                tile.par_chunks_mut(size[0])
                    .enumerate()
                    .map(|(y, row)| -> Result<RowStats> {
                        check_cancelled(cancel)?;
                        let mut result = RowStats {
                            min_n: sources.len(),
                            min_fraction: 1.,
                            min_surviving: 1.,
                            ..Default::default()
                        };
                        let mut values = Vec::with_capacity(sources.len());
                        let mut scratch = ClippingScratch::with_capacity(sources.len());
                        for (x, pixel) in row.iter_mut().enumerate() {
                            let p = y * size[0] + x;
                            let mut covered = Sum::default();
                            let mut n = 0;
                            for (frame, w) in frames.iter().zip(&weights) {
                                if frame[p][0].is_finite() {
                                    covered.add(*w);
                                    n += 1;
                                }
                            }
                            result.min_n = result.min_n.min(n);
                            result.max_n = result.max_n.max(n);
                            result.min_fraction =
                                result.min_fraction.min(covered.value() / total_weight);
                            if n == 0 {
                                result.zero += 1;
                            }
                            for (c, out) in pixel.0.iter_mut().enumerate() {
                                values.clear();
                                values.extend(frames.iter().enumerate().filter_map(|(i, f)| {
                                    f[p][c].is_finite().then_some((f[p][c], i))
                                }));
                                result.valid += values.len() as u64;
                                let (value, low, high, surviving) =
                                    combine(&values, &weights, &mut scratch, config);
                                *out = value;
                                pixel.1[c] = ((values.len() as u64 - low - high) as f64
                                    / original_input_count as f64)
                                    as f32;
                                pixel.2[c] = (high as f64 / original_input_count as f64) as f32;
                                result.low += low;
                                result.high += high;
                                result.min_surviving =
                                    result.min_surviving.min(surviving / total_weight);
                            }
                        }
                        Ok(result)
                    })
                    .collect::<Result<Vec<_>>>()?;
            for r in rows {
                stats.valid_channel_samples += r.valid;
                stats.rejected_low_channel_samples += r.low;
                stats.rejected_high_channel_samples += r.high;
                stats.zero_coverage_pixels += r.zero;
                stats.min_valid_frames = stats.min_valid_frames.min(r.min_n);
                stats.max_valid_frames = stats.max_valid_frames.max(r.max_n);
                stats.minimum_geometric_weight_fraction =
                    stats.minimum_geometric_weight_fraction.min(r.min_fraction);
                stats.minimum_surviving_weight_fraction =
                    stats.minimum_surviving_weight_fraction.min(r.min_surviving);
            }
            stats.reduction_wall_seconds += reduction_start.elapsed().as_secs_f64();
            let copy_start = Instant::now();
            for y in 0..size[1] {
                for x in 0..size[0] {
                    let target = (y0 + y) * canvas.width + x0 + x;
                    let (master, used, high) = tile[y * size[0] + x];
                    image.pixels[target] = master;
                    coverage.pixels[target] = used;
                    high_rejection.pixels[target] = high;
                }
            }
            stats.output_copy_wall_seconds += copy_start.elapsed().as_secs_f64();
            if progress.elapsed().as_secs() >= 30 {
                eprintln!(
                    "Integration: {}/{} tiles completed in {:.1} s.",
                    stats.tile_count + 1,
                    total_tiles,
                    start.elapsed().as_secs_f64()
                );
                progress = Instant::now();
            }
            stats.tile_count += 1;
        }
    }
    #[cfg(feature = "gpu")]
    if let ReconstructionEngine::Gpu(gpu) = backend {
        stats.gpu = Some(
            gpu.lock()
                .map_err(|_| Error::Invalid("GPU telemetry lock failed."))?
                .telemetry(),
        );
    }
    stats.wall_seconds = start.elapsed().as_secs_f64();
    stats.fits_cache = cache.stats()?;
    stats.peak_rss_bytes = fulgur_core::telemetry::peak_rss_bytes();
    Ok(IntegrationResult {
        image,
        coverage,
        high_rejection,
        stats,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fulgur_core::fits::read_roi;
    use std::fs;
    fn synthetic(path: &Path, t: Transform, exposure: f64, transparency: f64, trail: bool) {
        let mut bytes = vec![];
        for (key, value) in [
            ("SIMPLE", "T".to_owned()),
            ("BITPIX", "16".into()),
            ("NAXIS", "2".into()),
            ("NAXIS1", "128".into()),
            ("NAXIS2", "128".into()),
            ("BZERO", "32768".into()),
            ("BAYERPAT", "'GRBG'".into()),
            ("EXPTIME", exposure.to_string()),
            ("BIAS", "2064".into()),
        ] {
            bytes.extend(format!("{:<80}", format!("{key:<8}= {value:>20}")).as_bytes());
        }
        bytes.extend(format!("{:<80}", "END").as_bytes());
        bytes.resize(2880, b' ');
        for y in 0..128 {
            for x in 0..128 {
                let q = t.apply([x as f64, y as f64]);
                let profile = (-((q[0] - 62.37).powi(2) + (q[1] - 64.81).powi(2))
                    / (2. * 1.5_f64.powi(2)))
                .exp();
                let amplitude = match fulgur_core::BayerPattern::GRBG.color(x, y) {
                    fulgur_core::Color::Red => 10000.,
                    fulgur_core::Color::Green => 8000.,
                    fulgur_core::Color::Blue => 6000.,
                };
                let satellite = if trail && (q[1] - 80.).abs() < 1. {
                    10000.
                } else {
                    0.
                };
                let physical = (2000.
                    + exposure / 10. * transparency * (amplitude * profile + satellite))
                    .round() as i32;
                bytes.extend(((physical - 32768) as i16).to_be_bytes());
            }
        }
        bytes.resize(bytes.len().div_ceil(2880) * 2880, 0);
        fs::write(path, bytes).unwrap();
    }
    #[test]
    fn specialized_keys_weights_match_scalar_at_integer_boundaries() {
        for base in [1_usize, 2, 3, 17, 127, 2157, 3837, 65535] {
            let low = base as f64;
            let high = (base + 1) as f64;
            let positions = std::iter::once(low)
                .chain(std::iter::once(f64::from_bits(low.to_bits() + 1)))
                .chain(std::iter::once(f64::from_bits(high.to_bits() - 1)))
                .chain((1..1024).map(|i| low + i as f64 / 1024.));
            for p in positions {
                let expected: [f32; 4] = std::array::from_fn(|i| keys(p - (base + i) as f64 + 1.));
                assert_eq!(
                    keys_weights(p, base).map(f32::to_bits),
                    expected.map(f32::to_bits),
                    "p={p}, base={base}"
                );
            }
        }
    }

    #[test]
    fn cpu_resampling_matches_prior_scalar_bits_and_masks() {
        fn scalar_sample(
            rgb: &RgbImage,
            roi: [usize; 4],
            sensor: [usize; 2],
            p: [f64; 2],
        ) -> Option<[f32; 3]> {
            if p[0] < 1.
                || p[1] < 1.
                || p[0] >= (sensor[0] - 2) as f64
                || p[1] >= (sensor[1] - 2) as f64
            {
                return None;
            }
            let x = p[0].floor() as usize;
            let y = p[1].floor() as usize;
            if x < roi[0] + 1
                || y < roi[1] + 1
                || x + 2 >= roi[0] + roi[2]
                || y + 2 >= roi[1] + roi[3]
            {
                return None;
            }
            let wx: [f32; 4] = std::array::from_fn(|i| keys(p[0] - (x + i) as f64 + 1.));
            let wy: [f32; 4] = std::array::from_fn(|i| keys(p[1] - (y + i) as f64 + 1.));
            let mut result = [0.; 3];
            for (j, yy) in wy.iter().enumerate() {
                for (i, xx) in wx.iter().enumerate() {
                    let value = rgb.pixels[(y + j - 1 - roi[1]) * rgb.width + x + i - 1 - roi[0]];
                    let w = xx * yy;
                    for c in 0..3 {
                        result[c] += w * value[c];
                    }
                }
            }
            Some(result)
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.fit");
        synthetic(&path, Transform::IDENTITY, 10., 1., false);
        let header = read_header(&path).unwrap();
        let cancel = AtomicBool::new(false);
        let full = RgbImage {
            width: 128,
            height: 128,
            demosaic_method: DemosaicMethod::Vng,
            pixels: (0..128 * 128)
                .map(|i| {
                    let v = ((i * 7919) % 65536) as f32;
                    [v - 0.03125, 65535. - v, (i % 37) as f32 * 0.00003125]
                })
                .collect(),
        };
        let mut valid = 0;
        let mut invalid = 0;
        for scale_factor in [0.33, 0.5, 1., 2., 3.] {
            for angle in [0_f64, 0.07, 15., 90., 179.5] {
                for origin in [[-19, -17], [0, 0], [61, 59]] {
                    let source = IntegrationSource {
                        path: path.clone(),
                        header: header.clone(),
                        transform: Transform {
                            scale_factor,
                            theta_rad: angle.to_radians(),
                            tx_px: 7.3125,
                            ty_px: -8.125,
                        },
                        scale: 0.731_234_567_89,
                        offset: -0.123_456_789,
                        weight: 1.,
                    };
                    let size = [79, 63];
                    for roi in [[0, 0, 128, 128], [16, 22, 93, 87]] {
                        let rgb = RgbImage {
                            width: roi[2],
                            height: roi[3],
                            demosaic_method: DemosaicMethod::Vng,
                            pixels: (roi[1]..roi[1] + roi[3])
                                .flat_map(|y| {
                                    full.pixels[y * 128 + roi[0]..y * 128 + roi[0] + roi[2]]
                                        .iter()
                                        .copied()
                                })
                                .collect(),
                        };
                        let actual = warp_rgb(&source, origin, size, roi, &rgb, &cancel).unwrap();
                        let inverse = source.transform.inverse();
                        for (i, pixel) in actual.iter().enumerate() {
                            let q = [
                                (origin[0] + (i % size[0]) as i32) as f64,
                                (origin[1] + (i / size[0]) as i32) as f64,
                            ];
                            match scalar_sample(&rgb, roi, [128, 128], inverse.apply(q)) {
                                Some(v) => {
                                    let expected =
                                        v.map(|v| (source.scale * v as f64 + source.offset) as f32);
                                    assert_eq!(
                                        pixel.map(f32::to_bits),
                                        expected.map(f32::to_bits),
                                        "scale={scale_factor}, angle={angle}, origin={origin:?}, roi={roi:?}, pixel={i}"
                                    );
                                    valid += 1;
                                }
                                None => {
                                    assert!(pixel.iter().all(|v| v.is_nan()));
                                    invalid += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(valid > 10000 && invalid > 10000);
        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        let source = IntegrationSource {
            path,
            header,
            transform: Transform::IDENTITY,
            scale: 1.,
            offset: 0.,
            weight: 1.,
        };
        assert!(matches!(
            warp_rgb(&source, [0, 0], [79, 63], [0, 0, 128, 128], &full, &cancel),
            Err(Error::Cancelled)
        ));
    }

    #[test]
    fn reusable_clipping_scratch_matches_allocating_baseline_bit_for_bit() {
        fn allocating_reference(
            values: &[(f32, usize)],
            weights: &[f64],
            scratch: &mut Vec<f32>,
            config: &IntegrationConfig,
        ) -> (f32, u64, u64, f64) {
            if values.is_empty() {
                return (0., 0, 0, 0.);
            }
            let mut low = f64::NEG_INFINITY;
            let mut high = f64::INFINITY;
            if values.len() >= 5 {
                scratch.clear();
                scratch.extend(values.iter().map(|v| v.0));
                for _ in 0..3 {
                    let center = median(scratch);
                    let mut deviations: Vec<_> = scratch
                        .iter()
                        .map(|v| (*v as f64 - center).abs() as f32)
                        .collect();
                    let sigma = (1.4826 * median(&mut deviations)).max(1e-6);
                    low = center - config.sigma_low * sigma;
                    high = center + config.sigma_high * sigma;
                    let mut changed = false;
                    for x in scratch.iter_mut() {
                        let next = (*x as f64).clamp(low, high) as f32;
                        changed |= next != *x;
                        *x = next;
                    }
                    if !changed {
                        break;
                    }
                }
            }
            let mut numerator = Sum::default();
            let mut denominator = Sum::default();
            let mut rejected_low = 0;
            let mut rejected_high = 0;
            for &(v, index) in values {
                if (v as f64) < low {
                    rejected_low += 1;
                } else if (v as f64) > high {
                    rejected_high += 1;
                } else {
                    numerator.add(v as f64 * weights[index]);
                    denominator.add(weights[index]);
                }
            }
            let weight = denominator.value();
            if weight == 0. {
                (0., rejected_low, rejected_high, 0.)
            } else {
                (
                    (numerator.value() / weight) as f32,
                    rejected_low,
                    rejected_high,
                    weight,
                )
            }
        }

        let weights: Vec<_> = (0..1500)
            .map(|i| 0.01 + ((i * 37) % 100) as f64 / 101.)
            .collect();
        let mut scratch = ClippingScratch::with_capacity(weights.len());
        let addresses = (scratch.winsorized.as_ptr(), scratch.deviations.as_ptr());
        let capacities = (scratch.winsorized.capacity(), scratch.deviations.capacity());
        let mut seed = 0x27182818_u64;
        for (sigma_low, sigma_high) in [(3., 2.5), (1., 1.), (5., 3.), (0.3, 0.6)] {
            let config = IntegrationConfig {
                sigma_low,
                sigma_high,
                ..Default::default()
            };
            for round in 0..12 {
                for n in [0, 1, 2, 4, 5, 6, 27, 32, 940, 1140, 1500, 940, 5, 0] {
                    let values: Vec<_> = (0..n)
                        .map(|i| {
                            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                            let noise = ((seed >> 32) as u32 % 4096) as f32 / 512.;
                            let value = match round % 4 {
                                0 => {
                                    if i % 19 == 0 {
                                        65000.
                                    } else {
                                        2064.
                                    }
                                }
                                1 => 2064. + noise / 8192.,
                                2 => {
                                    if i % 11 == 0 {
                                        -100.
                                    } else {
                                        65535. - noise
                                    }
                                }
                                _ => {
                                    if i % 13 == 0 {
                                        60000.
                                    } else {
                                        2000. + noise
                                    }
                                }
                            };
                            (value, i)
                        })
                        .collect();
                    let expected =
                        allocating_reference(&values, &weights, &mut Vec::new(), &config);
                    let actual = combine(&values, &weights, &mut scratch, &config);
                    assert_eq!(
                        (actual.0.to_bits(), actual.1, actual.2, actual.3.to_bits()),
                        (
                            expected.0.to_bits(),
                            expected.1,
                            expected.2,
                            expected.3.to_bits()
                        ),
                        "n={n}, round={round}, sigma={sigma_low}/{sigma_high}"
                    );
                    assert_eq!(
                        (scratch.winsorized.as_ptr(), scratch.deviations.as_ptr()),
                        addresses
                    );
                    assert_eq!(
                        (scratch.winsorized.capacity(), scratch.deviations.capacity()),
                        capacities
                    );
                }
            }
        }
    }
    #[test]
    fn clipping_removes_trails_preserves_clean_core_and_handles_sparse_coverage() {
        let config = IntegrationConfig::default();
        let weights = [1.; 9];
        let mut scratch = ClippingScratch::with_capacity(weights.len());
        for core in [2000_f32, 40000., 65535.] {
            let values: Vec<_> = (0..9)
                .map(|i| (if i == 8 { core + 10000. } else { core }, i))
                .collect();
            let got = combine(&values, &weights, &mut scratch, &config);
            assert_eq!(got.0, core);
            assert_eq!(got.2, 1);
            assert_eq!(got.3, 8.);
        }
        let got = combine(&[(1000., 0), (9000., 1)], &weights, &mut scratch, &config);
        assert_eq!(got.0, 5000.);
        assert_eq!(got.1 + got.2, 0);
        assert_eq!(combine(&[], &weights, &mut scratch, &config).0, 0.);
        let mut sum = Sum::default();
        for v in [1e16, 1., -1e16] {
            sum.add(v);
        }
        assert_eq!(sum.value(), 1.);
    }
    #[test]
    fn keys_preserves_fields_and_invalid_sensor_footprints_are_not_padding() {
        let rgb = RgbImage {
            width: 20,
            height: 20,
            demosaic_method: DemosaicMethod::Vng,
            pixels: (0..400)
                .map(|i| {
                    let x = (i % 20) as f32;
                    let y = (i / 20) as f32;
                    [2000., 3. * x + 2. * y, 100. + x]
                })
                .collect(),
        };
        for i in 0..32 {
            let x = 5. + i as f64 / 16.;
            let y = 9.23;
            let v = sample(&rgb, [0, 0, 20, 20], [20, 20], [x, y]).unwrap();
            assert!((v[0] - 2000.).abs() < 0.001);
            assert!((v[1] as f64 - 3. * x - 2. * y).abs() < 0.00002);
        }
        assert!(sample(&rgb, [0, 0, 20, 20], [20, 20], [0.999, 8.]).is_none());
        assert!(sample(&rgb, [0, 0, 20, 20], [20, 20], [18., 8.]).is_none());
    }

    #[cfg(feature = "gpu")]
    #[test]
    fn hardware_gpu_integration_preserves_flux_masks_and_rejection() {
        if std::env::var("FULGURSTACK_TEST_GPU").as_deref() != Ok("1") {
            eprintln!("SKIP GPU integration hardware gate; set FULGURSTACK_TEST_GPU=1");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let gpu = GpuIntegrationEngine::new().expect("required real GPU");
        let mut sources = Vec::new();
        for i in 0..6 {
            let path = dir.path().join(format!("light_{i}.fit"));
            synthetic(&path, Transform::IDENTITY, 10., 1., i == 5);
            sources.push(IntegrationSource {
                header: read_header(&path).unwrap(),
                path,
                transform: Transform::IDENTITY,
                scale: 1.,
                offset: 0.,
                weight: 1.,
            });
        }
        for scale in [0.33, 1., 2., 3.] {
            for source in &mut sources {
                source.transform = Transform {
                    scale_factor: scale,
                    theta_rad: 0.07,
                    tx_px: 7.3,
                    ty_px: 8.1,
                };
            }
            let canvas = Canvas {
                origin_x_px: 0,
                origin_y_px: 0,
                width: (128. * scale) as usize + 20,
                height: (128. * scale) as usize + 20,
                minimum_effective_weight_fraction: 0.,
                coordinate_rule: "GPU test",
            };
            let config = IntegrationConfig {
                tile_mode: TileMode::Manual,
                fits_cache_mib: 1024,
                tile_width: 64,
                tile_height: 64,
                ..Default::default()
            };
            let cpu = CpuIntegrationEngine
                .integrate_with_maps(&sources, &canvas, &config, 8, &cancel)
                .unwrap();
            let actual = gpu
                .integrate_with_maps(&sources, &canvas, &config, 8, &cancel)
                .unwrap();
            let mut squares = 0.;
            let mut maximum = 0_f64;
            let mut a_flux = 0.;
            let mut b_flux = 0.;
            for (a, b) in cpu.image.pixels.iter().zip(&actual.image.pixels) {
                for c in 0..3 {
                    let d = (a[c] as f64 - b[c] as f64).abs() / 65535.;
                    maximum = maximum.max(d);
                    squares += d * d;
                    a_flux += (a[c] as f64 - 2000.).max(0.);
                    b_flux += (b[c] as f64 - 2000.).max(0.);
                }
            }
            let rms = (squares / (canvas.width * canvas.height * 3) as f64).sqrt();
            assert!(
                maximum <= 1e-3 && rms <= 1e-5,
                "scale {scale}: max={maximum}, rms={rms}"
            );
            assert!((a_flux / b_flux - 1.).abs() <= 0.001, "flux parity");
            assert_eq!(
                cpu.stats.valid_channel_samples,
                actual.stats.valid_channel_samples
            );
            assert_eq!(
                cpu.stats.rejected_high_channel_samples,
                actual.stats.rejected_high_channel_samples
            );
            assert_eq!(cpu.coverage.pixels, actual.coverage.pixels);
            assert_eq!(cpu.high_rejection.pixels, actual.high_rejection.pixels);
            assert_eq!(
                cpu.stats.corrected_roi_pixels,
                actual.stats.corrected_roi_pixels
            );
            println!("GPU integration gate scale={scale}: normalized RMS={rms}, max={maximum}");
        }
    }
    #[test]
    fn scaled_inverse_warp_matches_full_reconstruction_and_is_tile_invariant() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.fit");
        synthetic(&path, Transform::IDENTITY, 10., 1., false);
        let before = fs::read(&path).unwrap();
        let cancel = AtomicBool::new(false);
        let raw = fulgur_core::fits::read_raw(&path).unwrap();
        let (full, _) = crate::debayer::debayer(&raw, &cancel, DemosaicMethod::Vng).unwrap();
        for scale in [0.33, 0.5, 2., 3.] {
            let transform = Transform {
                scale_factor: scale,
                theta_rad: 0.08,
                tx_px: 7.3,
                ty_px: 8.1,
            };
            let source = IntegrationSource {
                path: path.clone(),
                header: raw.header.clone(),
                transform,
                scale: 1.,
                offset: 0.,
                weight: 1.,
            };
            let canvas = Canvas {
                origin_x_px: 0,
                origin_y_px: 0,
                width: (128. * scale) as usize + 20,
                height: (128. * scale) as usize + 20,
                minimum_effective_weight_fraction: 0.,
                coordinate_rule: "test reference",
            };
            let config = IntegrationConfig {
                tile_mode: TileMode::Manual,
                tile_width: 37,
                tile_height: 25,
                ..Default::default()
            };
            let result = CpuIntegrationEngine
                .integrate_with_maps(std::slice::from_ref(&source), &canvas, &config, 1, &cancel)
                .unwrap();
            for y in 0..canvas.height {
                for x in 0..canvas.width {
                    let p = transform.inverse().apply([x as f64, y as f64]);
                    let expected = sample(&full, [0, 0, 128, 128], [128, 128], p);
                    let index = y * canvas.width + x;
                    assert_eq!(result.image.pixels[index], expected.unwrap_or([0.; 3]));
                    assert_eq!(
                        result.coverage.pixels[index],
                        if expected.is_some() { [1.; 3] } else { [0.; 3] }
                    );
                    assert_eq!(result.high_rejection.pixels[index], [0.; 3]);
                }
            }
            for (width, height) in [
                (64, 64),
                (128, 256),
                (256, 128),
                (256, 256),
                (384, 192),
                (512, 128),
                (512, 256),
                (256, 512),
                (384, 384),
                (512, 512),
            ] {
                let other = IntegrationConfig {
                    tile_mode: TileMode::Manual,
                    tile_width: width,
                    tile_height: height,
                    ..Default::default()
                };
                let second = CpuIntegrationEngine
                    .integrate_with_maps(std::slice::from_ref(&source), &canvas, &other, 1, &cancel)
                    .unwrap();
                assert_eq!(result.image.pixels, second.image.pixels);
                assert_eq!(result.coverage.pixels, second.coverage.pixels);
                assert_eq!(result.high_rejection.pixels, second.high_rejection.pixels);
            }
        }
        assert_eq!(fs::read(path).unwrap(), before);
    }
    #[test]
    fn diagnostic_maps_count_original_samples_per_channel_after_rejection() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let mut sources = vec![];
        for i in 0..6 {
            let path = dir.path().join(format!("constant_{i}.fit"));
            synthetic(&path, Transform::IDENTITY, 10., 1., false);
            let mut bytes = fs::read(&path).unwrap();
            for (j, pair) in bytes[2880..2880 + 128 * 128 * 2]
                .chunks_exact_mut(2)
                .enumerate()
            {
                let red = fulgur_core::BayerPattern::GRBG.color(j % 128, j / 128)
                    == fulgur_core::Color::Red;
                let physical = if i == 5 && red { 5000 } else { 2000 };
                pair.copy_from_slice(&((physical - 32768) as i16).to_be_bytes());
            }
            fs::write(&path, bytes).unwrap();
            sources.push(IntegrationSource {
                header: read_header(&path).unwrap(),
                path,
                transform: Transform::IDENTITY,
                scale: 1.,
                offset: 0.,
                weight: if i == 5 { 0.001 } else { 1. },
            });
        }
        let canvas = Canvas {
            origin_x_px: 0,
            origin_y_px: 0,
            width: 128,
            height: 128,
            minimum_effective_weight_fraction: 0.,
            coordinate_rule: "reference",
        };
        let config = IntegrationConfig {
            tile_mode: TileMode::Manual,
            tile_width: 64,
            tile_height: 64,
            ..Default::default()
        };
        let result = CpuIntegrationEngine
            .integrate_with_maps(&sources, &canvas, &config, 8, &cancel)
            .unwrap();
        assert_eq!(result.image.pixels[64 * 128 + 64], [2000.; 3]);
        assert_eq!(
            result.coverage.pixels[64 * 128 + 64],
            [5. / 8., 6. / 8., 6. / 8.]
        );
        assert_eq!(
            result.high_rejection.pixels[64 * 128 + 64],
            [1. / 8., 0., 0.]
        );
        assert_eq!(result.coverage.pixels[0], [0.; 3]);
        assert_eq!(result.image.pixels[0], [0.; 3]);
        assert_eq!(result.high_rejection.pixels[0], [0.; 3]);
        let used: f64 = result
            .coverage
            .pixels
            .iter()
            .flatten()
            .map(|v| *v as f64 * 8.)
            .sum();
        let high: f64 = result
            .high_rejection
            .pixels
            .iter()
            .flatten()
            .map(|v| *v as f64 * 8.)
            .sum();
        assert_eq!(high, result.stats.rejected_high_channel_samples as f64);
        assert_eq!(
            used + high + result.stats.rejected_low_channel_samples as f64,
            result.stats.valid_channel_samples as f64
        );
        for (width, height) in [
            (37, 25),
            (128, 256),
            (256, 128),
            (256, 256),
            (384, 192),
            (512, 128),
            (512, 256),
            (256, 512),
            (384, 384),
            (512, 512),
        ] {
            let other = IntegrationConfig {
                tile_mode: TileMode::Manual,
                tile_width: width,
                tile_height: height,
                ..config.clone()
            };
            let tiled = CpuIntegrationEngine
                .integrate_with_maps(&sources, &canvas, &other, 8, &cancel)
                .unwrap();
            assert_eq!(result.image.pixels, tiled.image.pixels);
            assert_eq!(result.coverage.pixels, tiled.coverage.pixels);
            assert_eq!(result.high_rejection.pixels, tiled.high_rejection.pixels);
        }
        assert!(
            CpuIntegrationEngine
                .integrate_with_maps(&sources, &canvas, &config, 5, &cancel)
                .is_err()
        );
    }
    #[test]
    fn real_roi_reconstruction_matches_full_frame_without_internal_edge_artifacts() {
        let Some(path) = std::env::var_os("FULGURSTACK_TEST_FITS") else {
            return;
        };
        let cancel = AtomicBool::new(false);
        let raw = fulgur_core::fits::read_raw(Path::new(&path)).unwrap();
        let (full, _) = crate::debayer::debayer(&raw, &cancel, DemosaicMethod::Vng).unwrap();
        for roi in [
            [0, 0, 200, 200],
            [1600, 1200, 250, 220],
            [1960, 3640, 200, 200],
        ] {
            let cropped = read_roi(Path::new(&path), &raw.header, roi, &cancel).unwrap();
            let (rgb, _) = crate::debayer::debayer(&cropped, &cancel, DemosaicMethod::Vng).unwrap();
            for y in 0..roi[3] {
                for x in 0..roi[2] {
                    let gx = x + roi[0];
                    let gy = y + roi[1];
                    if (x >= 6 || roi[0] == 0)
                        && (y >= 6 || roi[1] == 0)
                        && (x + 6 < roi[2] || roi[0] + roi[2] == full.width)
                        && (y + 6 < roi[3] || roi[1] + roi[3] == full.height)
                    {
                        assert_eq!(
                            rgb.pixels[y * roi[2] + x],
                            full.pixels[gy * full.width + gx],
                            "ROI mismatch at {gx},{gy}"
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn mixed_exposure_fractional_shifts_preserve_star_flux_and_tile_seams() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let mut sources = vec![];
        for i in 0..8 {
            let shift = i as f64 / 8.;
            let t = Transform {
                scale_factor: 1.,
                theta_rad: 0.,
                tx_px: shift,
                ty_px: shift * 0.7,
            };
            let exposure = [5., 10., 20.][i % 3];
            let transmission = if i % 2 == 0 { 0.7 } else { 1.1 };
            let path = dir.path().join(format!("frame{i}.fit"));
            synthetic(&path, t, exposure, transmission, i == 7);
            let scale = 10. / (exposure * transmission);
            sources.push(IntegrationSource {
                header: read_header(&path).unwrap(),
                path,
                transform: t,
                scale,
                offset: 2000. - 2000. * scale,
                weight: 1.,
            });
        }
        let canvas = Canvas {
            origin_x_px: 20,
            origin_y_px: 20,
            width: 88,
            height: 88,
            minimum_effective_weight_fraction: 1.,
            coordinate_rule: "test",
        };
        let config = IntegrationConfig {
            tile_mode: TileMode::Manual,
            tile_width: 23,
            tile_height: 19,
            ..Default::default()
        };
        let (image, stats) = CpuIntegrationEngine
            .integrate(&sources, &canvas, &config, &cancel)
            .unwrap();
        let measured = stats.reconstruction_wall_seconds
            + stats.reduction_wall_seconds
            + stats.output_copy_wall_seconds;
        assert!(measured > 0. && measured <= stats.wall_seconds);
        assert!(stats.roi_task_elapsed.read_seconds > 0.);
        assert!(stats.roi_task_elapsed.cosmetic_seconds > 0.);
        assert!(stats.roi_task_elapsed.demosaic_seconds > 0.);
        assert!(stats.roi_task_elapsed.resampling_seconds > 0.);
        assert_eq!(stats.zero_coverage_pixels, 0);
        assert!(stats.rejected_high_channel_samples > 0);
        assert!(stats.minimum_geometric_weight_fraction > 0.999999);
        let mut flux = [0.; 3];
        for y in 0..88 {
            for x in 0..88 {
                let r =
                    ((x as f64 + 20. - 62.37).powi(2) + (y as f64 + 20. - 64.81).powi(2)).sqrt();
                if r < 8. {
                    for (c, f) in flux.iter_mut().enumerate() {
                        *f += image.pixels[y * 88 + x][c] as f64 - 2000.;
                    }
                }
            }
        }
        let baseline_path = dir.path().join("baseline.fit");
        synthetic(&baseline_path, Transform::IDENTITY, 10., 1., false);
        let baseline_raw = fulgur_core::fits::read_raw(&baseline_path).unwrap();
        let (baseline, _) =
            crate::debayer::debayer(&baseline_raw, &cancel, DemosaicMethod::Vng).unwrap();
        let mut baseline_flux = [0.; 3];
        for y in 0..128 {
            for x in 0..128 {
                if ((x as f64 - 62.37).powi(2) + (y as f64 - 64.81).powi(2)).sqrt() < 8. {
                    for (c, f) in baseline_flux.iter_mut().enumerate() {
                        *f += baseline.pixels[y * 128 + x][c] as f64 - 2000.;
                    }
                }
            }
        }
        println!(
            "Integrated flux {:?}; accepted M1 reconstruction flux {:?}",
            flux, baseline_flux
        );
        for (c, amp) in [10000., 8000., 6000.].iter().enumerate() {
            let expected = 2. * std::f64::consts::PI * 1.5_f64.powi(2) * amp;
            let error = (flux[c] / expected - 1.).abs();
            println!("Mixed exposure star channel {c}: relative flux error {error:.8}");
            let integration_error = (flux[c] / baseline_flux[c] - 1.).abs();
            assert!(
                integration_error <= 0.01,
                "channel {c} changes accepted reconstruction flux by {integration_error}"
            );
        }
        let mut other = config.clone();
        other.tile_width = 88;
        other.tile_height = 88;
        let (whole, _) = CpuIntegrationEngine
            .integrate(&sources, &canvas, &other, &cancel)
            .unwrap();
        assert_eq!(image.pixels, whole.pixels);
        let mut bad = config.clone();
        bad.memory_budget_mib = 32;
        assert!(
            CpuIntegrationEngine
                .integrate(&sources, &canvas, &bad, &cancel)
                .is_err()
        );
        assert!(
            CpuIntegrationEngine
                .integrate(&sources, &canvas, &config, &AtomicBool::new(true))
                .is_err()
        );
    }
}
