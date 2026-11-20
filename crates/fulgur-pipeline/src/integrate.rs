//! Output-tile-major CPU integration of immutable CFA source ROIs.
use crate::register::{Canvas, RegistrationReport, Transform};
pub use crate::tile_plan::MemoryPlan;
use fulgur_core::fits::{ImageHeader, read_header};
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

/// Pixel rejection before the weighted mean (ADR-087).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Rejection {
    /// Chosen by the number of stacked frames (see `resolve`).
    Auto,
    /// Median/MAD winsorized sigma clipping (`sigma_low` / `sigma_high`).
    #[default]
    Winsorized,
    /// Median of the samples; robust, about 25% more noise than a mean.
    Median,
    /// Linear fit clipping: tolerates level spreads between frames.
    LinearFit,
    /// Generalized extreme studentized deviate test, for large stacks.
    Esd,
}

impl Rejection {
    pub const ALL: [Self; 5] = [
        Self::Auto,
        Self::Winsorized,
        Self::LinearFit,
        Self::Esd,
        Self::Median,
    ];

    pub fn parse(value: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|r| r.key() == value.trim().to_ascii_lowercase())
            .ok_or(Error::Invalid(
                "Rejection must be auto, winsorized, linear-fit, esd or median.",
            ))
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Winsorized => "winsorized",
            Self::Median => "median",
            Self::LinearFit => "linear-fit",
            Self::Esd => "esd",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto (by frame count)",
            Self::Winsorized => "Winsorized sigma clipping",
            Self::Median => "Median",
            Self::LinearFit => "Linear fit clipping",
            Self::Esd => "Generalized ESD",
        }
    }

    /// The method `Auto` uses for a stack of `frames` sources: median below
    /// 6, winsorized below 25, ESD from 25 frames. On real 27–856-frame
    /// stacks ESD gave the lowest master noise and was faster than linear
    /// fit, which stays available as an explicit choice (ADR-087).
    pub fn resolve(self, frames: usize) -> Self {
        match self {
            Self::Auto if frames < 6 => Self::Median,
            Self::Auto if frames < 25 => Self::Winsorized,
            Self::Auto => Self::Esd,
            other => other,
        }
    }

    /// Parameters as recorded in reports.
    pub fn rule(self, config: &IntegrationConfig) -> String {
        match self {
            Self::Auto => "auto by frame count: <6 median, <25 winsorized, >=25 ESD".into(),
            Self::Winsorized => format!(
                "median/MAD winsorized sigma clipping, low {} / high {} sigma, weighted mean of survivors",
                config.sigma_low, config.sigma_high
            ),
            Self::Median => "unweighted median of all samples; no rejection counted".into(),
            Self::LinearFit => format!(
                "line fitted to the sorted samples, reject below {} / above {} robust sigmas (1.2533 x mean absolute residual), iterated; weighted mean of survivors",
                rejection_methods::LINEAR_FIT_LOW,
                rejection_methods::LINEAR_FIT_HIGH
            ),
            Self::Esd => format!(
                "generalized ESD (Rosner 1983), alpha {}, at most {:.0}% of the stack; weighted mean of survivors",
                rejection_methods::ESD_ALPHA,
                rejection_methods::ESD_MAX_OUTLIERS * 100.
            ),
        }
    }
}

/// What the trail mask does around ESD's upper rejections (ADR-187).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TrailMode {
    /// Plain ESD, sample by sample.
    #[default]
    Off,
    /// Near an upper rejection of the same frame, also reject samples above
    /// the pixel's mean by `TrailMask::sigma` single-frame sigmas.
    Hysteresis,
    /// Near an upper rejection of the same frame, reject every sample.
    Grow,
}

impl TrailMode {
    pub const ALL: [Self; 3] = [Self::Off, Self::Hysteresis, Self::Grow];

    pub fn parse(value: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|m| m.key() == value.trim().to_ascii_lowercase())
            .ok_or(Error::Invalid(
                "Trail mask must be off, hysteresis or grow.",
            ))
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Hysteresis => "hysteresis",
            Self::Grow => "grow",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Hysteresis => "Hysteresis (raised neighbours)",
            Self::Grow => "Grow (whole neighbourhood)",
        }
    }
}

/// Extends ESD's upper rejections to the faint edges of moving objects
/// (asteroids, satellites) in the same frame (ADR-187).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct TrailMask {
    pub mode: TrailMode,
    /// Neighbourhood radius around an upper rejection, in output pixels.
    pub radius_px: f64,
    /// Hysteresis threshold above the pixel's survivor mean, in its
    /// single-frame standard deviations.
    pub sigma: f64,
}

impl Default for TrailMask {
    fn default() -> Self {
        Self {
            mode: TrailMode::Off,
            radius_px: 4.,
            sigma: 2.,
        }
    }
}

impl TrailMask {
    /// Halo of output pixels read around each tile, so the mask crosses tile
    /// edges: the radius and the one pixel that pairs a seed.
    pub fn halo(&self) -> usize {
        if self.mode == TrailMode::Off {
            0
        } else {
            self.radius_px.ceil() as usize + 1
        }
    }

    pub fn rule(&self) -> String {
        match self.mode {
            TrailMode::Off => "off".into(),
            TrailMode::Hysteresis => format!(
                "hysteresis: within {} px of an upper ESD rejection in the same frame, samples above the survivor mean + {} single-frame sigma are rejected too; not on stars or compact structure",
                self.radius_px, self.sigma
            ),
            TrailMode::Grow => format!(
                "grow: within {} px of an upper ESD rejection in the same frame, every sample is rejected; not on stars or compact structure",
                self.radius_px
            ),
        }
    }
}

/// Drizzle integration (ADR-188): each input pixel is shrunk to a square
/// drop of `pixfrac` pixels and its overlap with output pixels of 1/`scale`
/// reference pixels weights its contribution.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Drizzle {
    /// Output pixels per reference pixel along each axis.
    pub scale: u8,
    /// Side of the drop in input pixels.
    pub pixfrac: f64,
    /// Colour frames drop their raw CFA samples into their own channel
    /// (Bayer drizzle); false drizzles the VNG-demosaiced image instead.
    pub bayer: bool,
}

impl Default for Drizzle {
    fn default() -> Self {
        Self {
            scale: 2,
            pixfrac: 0.86,
            bayer: true,
        }
    }
}

impl Drizzle {
    pub fn rule(&self) -> String {
        format!(
            "drizzle {}x, square drop {} px, {}; weighted by drop overlap",
            self.scale,
            self.pixfrac,
            if self.bayer {
                "colour frames from raw CFA samples (Bayer drizzle)"
            } else {
                "colour frames after VNG demosaicing"
            }
        )
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct IntegrationConfig {
    /// Pixel rejection; `sigma_low` / `sigma_high` apply to winsorized.
    pub rejection: Rejection,
    pub sigma_low: f64,
    pub sigma_high: f64,
    pub memory_budget_mib: usize,
    pub fits_cache_mib: usize,
    pub tile_mode: TileMode,
    pub tile_width: usize,
    pub tile_height: usize,
    /// Trail mask around ESD's upper rejections; ESD only (ADR-187).
    pub trail: TrailMask,
    /// Drizzle integration onto a finer grid (ADR-188); None = resampling.
    pub drizzle: Option<Drizzle>,
}
impl Default for IntegrationConfig {
    fn default() -> Self {
        Self {
            rejection: Rejection::Winsorized,
            sigma_low: 3.,
            sigma_high: 2.5,
            memory_budget_mib: 12288,
            fits_cache_mib: 0,
            tile_mode: TileMode::Auto,
            tile_width: 512,
            tile_height: 512,
            trail: TrailMask::default(),
            drizzle: None,
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
            || !(1.0..=16.).contains(&self.trail.radius_px)
            || !(0.5..=5.).contains(&self.trail.sigma)
            || self
                .drizzle
                .is_some_and(|d| d.scale != 2 || !(0.3..=1.).contains(&d.pixfrac))
        {
            return Err(Error::Invalid(
                "Invalid clipping, tile geometry, trail mask (radius 1..16 px, 0.5..5 sigma), drizzle (2x, drop 0.3..1 px) or memory budget (1024..12288 MiB).",
            ));
        }
        Ok(())
    }
}
pub struct IntegrationSource {
    /// Calibration of the source's light set, applied right after decoding.
    pub calibration: Option<std::sync::Arc<crate::calibration::Calibration>>,
    pub distortion: Option<crate::distortion::Warp>,
    /// Tiles this frame may join, when refused as a whole (ADR-109).
    pub partial: Option<std::sync::Arc<crate::register::PartialField>>,
    pub path: PathBuf,
    pub header: ImageHeader,
    pub transform: Transform,
    pub scale: f64,
    /// Sky offset per colour channel (R, G, B); y[c] = scale * x[c] + offset[c].
    pub offset: [f64; 3],
    pub weight: f64,
}

/// What normalization did to the frames of one master (ADR-102).
#[derive(Clone, Debug, Serialize)]
pub struct NormalizationSummary {
    pub method: &'static str,
    pub rule: &'static str,
    pub frames: usize,
    /// Smallest and largest scale.
    pub scale_range: [f64; 2],
    /// Smallest and largest sky offset per channel (R, G, B), in ADU.
    pub offset_range_adu: [[f64; 2]; 3],
}

impl NormalizationSummary {
    pub fn of(sources: &[IntegrationSource]) -> Self {
        let range = |value: &dyn Fn(&IntegrationSource) -> f64| {
            sources
                .iter()
                .map(value)
                .fold([f64::INFINITY, f64::NEG_INFINITY], |[low, high], v| {
                    [low.min(v), high.max(v)]
                })
        };
        Self {
            method: "additive with scaling",
            rule: "before rejection y[c]=a*x[c]+b[c]; a=median flux ratio of matched stars, one scale for all channels; b[c] matches the sky of each colour channel",
            frames: sources.len(),
            scale_range: range(&|s| s.scale),
            offset_range_adu: [0, 1, 2].map(|c| range(&|s| s.offset[c])),
        }
    }

    /// One line for the console and Results; a mono stack has one offset.
    pub fn describe(&self) -> String {
        let [r, g, b] = self.offset_range_adu;
        let span = |v: [f64; 2]| format!("{:.0} to {:.0}", v[0], v[1]);
        let offsets = if r == g && g == b {
            span(g)
        } else {
            format!("R {}, G {}, B {}", span(r), span(g), span(b))
        };
        format!(
            "scale {:.3} to {:.3}, sky offset {offsets} ADU",
            self.scale_range[0], self.scale_range[1]
        )
    }
}
#[derive(Default, Debug, Serialize)]
pub struct IntegrationStats {
    pub backend: &'static str,
    /// Rejection method applied (after `Auto` resolution) and its parameters.
    pub rejection: &'static str,
    pub rejection_rule: String,
    #[cfg(feature = "gpu")]
    pub gpu: Option<fulgur_gpu::GpuTelemetry>,
    #[cfg(feature = "gpu")]
    pub gpu_reduction: Option<fulgur_gpu::reduce::GpuReductionTelemetry>,
    pub wall_seconds: f64,
    pub cpu_profile: Option<IntegrationCpuProfile>,
    /// Non-overlapping wall time for tile reconstruction, including scheduling and I/O.
    pub reconstruction_wall_seconds: f64,
    /// Non-overlapping wall time for sample gathering, clipping, accumulation and map generation.
    pub reduction_wall_seconds: f64,
    pub reduction_layout: &'static str,
    pub rejection_block_pixels: usize,
    pub max_concurrent_reduction_bands: usize,
    pub rejection_packing_buffer_bytes: usize,
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
    /// Upper rejections added by the trail mask, included in the high count
    /// (ADR-187).
    pub trail_masked_channel_samples: u64,
    /// The trail mask as applied, or "off".
    pub trail_rule: String,
    /// Drizzle as applied, or "off" (ADR-188).
    pub drizzle_rule: String,
    pub zero_coverage_pixels: u64,
    pub min_valid_frames: usize,
    pub max_valid_frames: usize,
    pub minimum_geometric_weight_fraction: f64,
    pub minimum_surviving_weight_fraction: f64,
    pub intermediate_images_written: usize,
}

/// Process CPU counters are additive CPU seconds, not overlapping ROI elapsed timers.
#[derive(Default, Debug, Serialize)]
pub struct IntegrationCpuProfile {
    pub available_logical_processors: usize,
    pub pool_threads: usize,
    pub reconstruction_workers: usize,
    pub reduction_workers: usize,
    pub process: fulgur_core::telemetry::ProcessCpuTime,
    pub reconstruction_cpu_seconds: f64,
    pub reduction_cpu_seconds: f64,
    pub output_copy_cpu_seconds: f64,
}

fn cpu_elapsed(
    start: Option<fulgur_core::telemetry::ProcessCpuTime>,
) -> Option<fulgur_core::telemetry::ProcessCpuTime> {
    start
        .zip(fulgur_core::telemetry::process_cpu_time())
        .map(|(a, b)| b.since(a))
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
/// Receives every finished output tile instead of the in-memory canvas, so
/// that a master larger than memory can be written as it is made (ADR-189).
/// A tile holds per pixel the master value, the coverage and the
/// high-rejection fractions.
pub trait TileSink: Send {
    fn tile(&mut self, at: [usize; 2], size: [usize; 2], tile: &[IntegratedPixel]) -> Result<()>;
}

pub trait IntegrationEngine {
    /// As `integrate_with_progress` (progress optional), handing every tile
    /// to `sink`; the result's images are then empty.
    #[allow(clippy::too_many_arguments)]
    fn integrate_into(
        &self,
        sources: &[IntegrationSource],
        canvas: &Canvas,
        config: &IntegrationConfig,
        original_input_count: usize,
        cancel: &AtomicBool,
        progress: Option<&(dyn Fn(usize, usize) + Sync)>,
        sink: &mut dyn TileSink,
    ) -> Result<IntegrationResult>;
    /// Completed output tiles; callbacks must remain bounded and nonblocking.
    /// Required of every engine: a default would drop the callback silently.
    fn integrate_with_progress(
        &self,
        sources: &[IntegrationSource],
        canvas: &Canvas,
        config: &IntegrationConfig,
        original_input_count: usize,
        cancel: &AtomicBool,
        progress: &(dyn Fn(usize, usize) + Sync),
    ) -> Result<IntegrationResult>;
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
/// Pixel rejection and weighted mean on the GPU (ADR-090). Uninhabited
/// without the `gpu` feature, so CPU-only builds always reduce on the CPU.
#[cfg(feature = "gpu")]
type GpuReduction = std::sync::Mutex<fulgur_gpu::reduce::GpuReducer>;
#[cfg(not(feature = "gpu"))]
enum GpuReduction {}

/// Which integration stages the GPU engine runs on the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuStages {
    /// Pixel rejection and weighted mean (default: fastest measured split).
    Reduction,
    /// Cosmetic correction and VNG demosaicing (the first M5 slice).
    Reconstruction,
    /// Both.
    All,
}
impl GpuStages {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "reduction" => Some(Self::Reduction),
            "reconstruction" => Some(Self::Reconstruction),
            "all" => Some(Self::All),
            _ => None,
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Reduction => "reduction",
            Self::Reconstruction => "reconstruction",
            Self::All => "all",
        }
    }
}
#[cfg(feature = "gpu")]
pub struct GpuIntegrationEngine {
    backend: ReconstructionEngine,
    reducer: Option<GpuReduction>,
}
#[cfg(feature = "gpu")]
impl GpuIntegrationEngine {
    /// The default split: CPU reconstruction, GPU pixel rejection.
    pub fn new() -> Result<Self> {
        Self::with_stages(GpuStages::Reduction)
    }
    pub fn with_stages(stages: GpuStages) -> Result<Self> {
        let backend = if stages == GpuStages::Reduction {
            ReconstructionEngine::Cpu
        } else {
            let gpu =
                fulgur_gpu::GpuReconstructor::new(&crate::vng::gpu_plans()).map_err(gpu_error)?;
            ReconstructionEngine::Gpu(Box::new(std::sync::Mutex::new(gpu)))
        };
        let reducer = if stages == GpuStages::Reconstruction {
            None
        } else {
            Some(std::sync::Mutex::new(
                fulgur_gpu::reduce::GpuReducer::new().map_err(gpu_error)?,
            ))
        };
        Ok(Self { backend, reducer })
    }
}
#[cfg(feature = "gpu")]
fn gpu_error(error: String) -> Error {
    Error::Message(error)
}
#[cfg(feature = "gpu")]
impl IntegrationEngine for GpuIntegrationEngine {
    fn integrate_into(
        &self,
        sources: &[IntegrationSource],
        canvas: &Canvas,
        config: &IntegrationConfig,
        original_input_count: usize,
        cancel: &AtomicBool,
        progress: Option<&(dyn Fn(usize, usize) + Sync)>,
        sink: &mut dyn TileSink,
    ) -> Result<IntegrationResult> {
        integrate_common_progress(
            sources,
            canvas,
            config,
            original_input_count,
            cancel,
            &self.backend,
            self.reducer.as_ref(),
            progress,
            Some(sink),
        )
    }
    fn integrate_with_progress(
        &self,
        sources: &[IntegrationSource],
        canvas: &Canvas,
        config: &IntegrationConfig,
        original_input_count: usize,
        cancel: &AtomicBool,
        progress: &(dyn Fn(usize, usize) + Sync),
    ) -> Result<IntegrationResult> {
        integrate_common_progress(
            sources,
            canvas,
            config,
            original_input_count,
            cancel,
            &self.backend,
            self.reducer.as_ref(),
            Some(progress),
            None,
        )
    }
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
            self.reducer.as_ref(),
        )
    }
}
pub fn sources(input: &Path, report: &RegistrationReport) -> Result<Vec<IntegrationSource>> {
    calibrated_sources(input, report, &[])
}

/// As `sources`, with each light set's in-RAM master dark (indexed like
/// `InspectReport::light_sets`). A frame measured on dark-calibrated pixels
/// must integrate with the same master; a missing master is an error.
pub fn calibrated_sources(
    input: &Path,
    report: &RegistrationReport,
    calibrations: &[Option<std::sync::Arc<crate::calibration::Calibration>>],
) -> Result<Vec<IntegrationSource>> {
    let root = if input.is_dir() {
        input
    } else {
        input.parent().unwrap_or(Path::new("."))
    };
    report
        .frames
        .iter()
        .zip(&report.inspection.frames)
        .filter(|(f, _)| f.accepted)
        .map(|(f, inspected)| {
            if report.geometric_correction_enabled
                && !f
                    .geometric_correction
                    .as_ref()
                    .and_then(|d| d.registration_validation.as_ref())
                    .is_some_and(|v| v.accepted)
                && f.partial_field.is_none()
            {
                return Err(Error::Invalid(
                    "Accepted source has no confirmed whole-field registration.",
                ));
            }
            let path = root.join(&f.file_name);
            let mut header = read_header(&path)?;
            // The light set's declared layout, as inspected, governs reconstruction.
            if let Some(info) = inspected.frame.as_ref() {
                if inspected.file_name != f.file_name {
                    return Err(Error::Invalid("Inspection and registration rows differ."));
                }
                header.info.bayer_pattern = info.bayer_pattern;
                header.info.mosaic = info.mosaic;
                header.info.mosaic_source = info.mosaic_source;
                header.info.saturation_adu = info.saturation_adu;
                // Owner-entered or confirmed exposure, as inspected (ADR-084).
                header.info.exposure_seconds = info.exposure_seconds;
                if info.rows_mirrored {
                    header.reverse_row_order();
                }
            }
            let calibration = if inspected.calibrated {
                Some(
                    calibrations
                        .get(inspected.light_set)
                        .cloned()
                        .flatten()
                        .ok_or(Error::Invalid(
                            "A calibrated frame has no calibration masters for integration.",
                        ))?,
                )
            } else {
                None
            };
            Ok(IntegrationSource {
                calibration,
                distortion: f.geometric_correction.as_ref().and_then(|d| d.model),
                partial: f.partial_field.clone(),
                path,
                header,
                transform: f
                    .transform
                    .ok_or(Error::Invalid("Missing accepted transform."))?,
                scale: f
                    .photometric_scale
                    .ok_or(Error::Invalid("Missing accepted photometry."))?,
                offset: match f.channel_offsets_adu {
                    Some(offsets) => offsets,
                    None => {
                        [f.background_offset_adu
                            .ok_or(Error::Invalid("Missing accepted sky offset."))?;
                            3]
                    }
                },
                weight: f.final_weight,
            })
        })
        .collect()
}
fn source_bounds(
    source: &IntegrationSource,
    origin: [i32; 2],
    size: [usize; 2],
    sensor: [usize; 2],
) -> Option<[usize; 4]> {
    if source
        .partial
        .as_ref()
        .is_some_and(|p| !p.admits(origin, size))
    {
        return None;
    }
    if let Some(model) = source.distortion {
        let b = model.inverse_region(origin, size)?;
        let x0 = (((b[0].floor() - 8.).max(0.).min(sensor[0] as f64)) as usize) / 2 * 2;
        let y0 = (((b[1].floor() - 8.).max(0.).min(sensor[1] as f64)) as usize) / 2 * 2;
        let x1 = ((b[2].ceil() + 9.).max(0.) as usize).min(sensor[0]);
        let y1 = ((b[3].ceil() + 9.).max(0.) as usize).min(sensor[1]);
        (x1 >= x0 + 6 && y1 >= y0 + 6).then_some([
            x0,
            y0,
            x1.saturating_sub(x0),
            y1.saturating_sub(y0),
        ])
    } else {
        bounds(source.transform, origin, size, sensor)
    }
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
pub(super) fn sample(
    rgb: &RgbImage,
    roi: [usize; 4],
    sensor: [usize; 2],
    p: [f64; 2],
) -> Option<[f32; 3]> {
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

/// One worker's reusable ROI storage. Every use overwrites all contents;
/// only capacity is carried between sources and tiles.
#[derive(Default)]
struct RoiScratch {
    raw: Vec<f32>,
    debayer: crate::debayer::DebayerBuffers,
    spans: crate::roi_spans::RowSpans,
    /// Source positions of a distorted frame's tile edge (ADR-144).
    coordinates: Vec<[f64; 2]>,
    /// Reconstructs the whole ROI, the reference for the spans.
    #[cfg(test)]
    whole_roi: bool,
}

/// Source outline of a tile on a rigid or similarity frame, in ROI coordinates.
fn tile_outline(
    source: &IntegrationSource,
    origin: [i32; 2],
    size: [usize; 2],
    roi: [usize; 4],
) -> [[f64; 2]; 4] {
    let inverse = source.transform.inverse();
    let (x, y) = (origin[0] as f64, origin[1] as f64);
    let (w, h) = ((size[0] - 1) as f64, (size[1] - 1) as f64);
    [[x, y], [x + w, y], [x + w, y + h], [x, y + h]].map(|p| {
        let q = inverse.apply(p);
        [q[0] - roi[0] as f64, q[1] - roi[1] as f64]
    })
}

/// `output` may be a previous tile's sample buffer; it is fully reinitialized.
fn warp(
    source: &IntegrationSource,
    origin: [i32; 2],
    size: [usize; 2],
    cancel: &AtomicBool,
    cache: &FitsBlockCache,
    scratch: &mut RoiScratch,
    mut output: Vec<[f32; 3]>,
) -> Result<WarpedTile> {
    let sensor = [source.header.info.width, source.header.info.height];
    let Some(roi) = source_bounds(source, origin, size, sensor) else {
        output.clear();
        output.resize(size[0] * size[1], [f32::NAN; 3]);
        return Ok((output, 0, 0, RoiTaskElapsed::default()));
    };
    let read_start = Instant::now();
    if source.header.info.mosaic == fulgur_core::Mosaic::Rgb {
        // Debayered elsewhere: no calibration, cosmetics or demosaicing here.
        let rgb = fulgur_core::fits::read_rgb_roi(
            &source.header,
            roi,
            cancel,
            std::mem::take(&mut scratch.debayer.rgb),
        )?;
        let read_seconds = read_start.elapsed().as_secs_f64();
        let warp_start = Instant::now();
        let output = warp_rgb_into(source, origin, size, roi, &rgb, cancel, output)?;
        scratch.debayer.rgb = rgb.pixels;
        return Ok((
            output,
            (roi[2] * roi[3]) as u64,
            0,
            RoiTaskElapsed {
                read_seconds,
                resampling_seconds: warp_start.elapsed().as_secs_f64(),
                ..Default::default()
            },
        ));
    }
    let mut raw = cache.read_roi_into(
        &source.path,
        &source.header,
        roi,
        cancel,
        std::mem::take(&mut scratch.raw),
    )?;
    if let Some(calibration) = &source.calibration {
        calibration.apply(&mut raw, [roi[0], roi[1]])?;
    }
    let read_seconds = read_start.elapsed().as_secs_f64();
    #[cfg(not(test))]
    let whole = false;
    #[cfg(test)]
    let whole = scratch.whole_roi;
    // A rigid frame's tile has a straight outline; a distorted frame's spans
    // come from the source position of each output sample (ADR-144).
    // A distorted frame's tile outline is curved: its spans come from the
    // solved source positions of the tile's edge samples (ADR-144).
    let curved = match source.distortion {
        Some(model) if !whole => {
            distorted_outline(&model, origin, size, &mut scratch.coordinates);
            let usable = scratch
                .coordinates
                .iter()
                .all(|p| p[0].is_finite() && p[1].is_finite());
            if usable {
                scratch
                    .spans
                    .fill_outline(&scratch.coordinates, [roi[0], roi[1]], roi[2], roi[3]);
            }
            usable
        }
        _ => false,
    };
    let rigid = source.distortion.is_none() && !whole;
    if rigid {
        let outline = tile_outline(source, origin, size, roi);
        scratch.spans.fill(outline, roi[2], roi[3]);
    }
    let (rgb, stats) = crate::debayer::debayer_spans(
        &raw,
        cancel,
        DemosaicMethod::Vng,
        &mut scratch.debayer,
        (rigid || curved).then_some(&scratch.spans),
    )?;
    scratch.raw = raw.pixels;
    let warp_start = Instant::now();
    let output = warp_rgb_into(source, origin, size, roi, &rgb, cancel, output)?;
    scratch.debayer.rgb = rgb.pixels;
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

/// Source positions of the edge samples of a tile on a distorted frame: its
/// first and last rows, and the first and last sample of every row, solved
/// as the resampler solves them. A position the model cannot solve is not
/// finite.
fn distorted_outline(
    model: &crate::distortion::Warp,
    origin: [i32; 2],
    size: [usize; 2],
    out: &mut Vec<[f64; 2]>,
) {
    let rotation = model.base.theta_rad.sin_cos();
    let outside = [f64::NEG_INFINITY; 2];
    let row = |y: usize| (origin[1] + y as i32) as f64;
    out.clear();
    out.resize(2 * size[0] + 2 * size[1], outside);
    let (top, rest) = out.split_at_mut(size[0]);
    let (bottom, sides) = rest.split_at_mut(size[0]);
    model.inverse_row(origin[0], row(0), rotation, outside, top);
    model.inverse_row(origin[0], row(size[1] - 1), rotation, outside, bottom);
    let last = origin[0] + size[0] as i32 - 1;
    for (y, pair) in sides.chunks_exact_mut(2).enumerate() {
        model.inverse_row(origin[0], row(y), rotation, outside, &mut pair[..1]);
        model.inverse_row(last, row(y), rotation, outside, &mut pair[1..]);
    }
}
#[cfg(test)]
fn warp_rgb(
    source: &IntegrationSource,
    origin: [i32; 2],
    size: [usize; 2],
    roi: [usize; 4],
    rgb: &RgbImage,
    cancel: &AtomicBool,
) -> Result<Vec<[f32; 3]>> {
    warp_rgb_into(source, origin, size, roi, rgb, cancel, Vec::new())
}

/// As `warp_rgb`, reinitializing reused storage to NaN (no sample) first.
fn warp_rgb_into(
    source: &IntegrationSource,
    origin: [i32; 2],
    size: [usize; 2],
    roi: [usize; 4],
    rgb: &RgbImage,
    cancel: &AtomicBool,
    mut output: Vec<[f32; 3]>,
) -> Result<Vec<[f32; 3]>> {
    let sensor = [source.header.info.width, source.header.info.height];
    output.clear();
    output.resize(size[0] * size[1], [f32::NAN; 3]);
    if let Some(model) = source.distortion {
        let rotation = model.base.theta_rad.sin_cos();
        let kernel = crate::resample::Kernel::detect();
        let view = crate::resample::View { rgb, roi, sensor };
        let identity = crate::resample::RowTransform {
            sy: 0.,
            cy: 0.,
            inverse_scale: 1.,
            tx: 0.,
            ty: 0.,
            scale: source.scale,
            offset: source.offset,
        };
        let mut coordinates = vec![[f64::NEG_INFINITY; 2]; size[0]];
        for (y, row) in output.chunks_exact_mut(size[0]).enumerate() {
            check_cancelled(cancel)?;
            model.inverse_row(
                origin[0],
                (origin[1] + y as i32) as f64,
                rotation,
                [f64::NEG_INFINITY; 2],
                &mut coordinates,
            );
            // Reuse the accepted SIMD/scalar sampler and photometric arithmetic.
            // Coordinates are already in source space; no second interpolation.
            kernel.row(view, identity, &coordinates, row)?;
        }
        return Ok(output);
    }
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
    let kernel = crate::resample::Kernel::detect();
    let view = crate::resample::View { rgb, roi, sensor };
    for (y, row) in output.chunks_exact_mut(size[0]).enumerate() {
        check_cancelled(cancel)?;
        let qy = (origin[1] + y as i32) as f64;
        let transform = crate::resample::RowTransform {
            sy: s * qy,
            cy: c * qy,
            inverse_scale: inverse.scale_factor,
            tx: inverse.tx_px,
            ty: inverse.ty_px,
            scale: source.scale,
            offset: source.offset,
        };
        kernel.row(view, transform, &columns, row)?;
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
            let data = if let Some(roi) = source_bounds(source, origin, size, sensor) {
                let mut raw = cache.read_roi(&source.path, &source.header, roi, cancel)?;
                if let Some(calibration) = &source.calibration {
                    calibration.apply(&mut raw, [roi[0], roi[1]])?;
                }
                Some((roi, raw))
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
    let reconstructed = gpu.reconstruct(&raw, cancel).map_err(|e| {
        if check_cancelled(cancel).is_err() {
            Error::Cancelled
        } else {
            gpu_error(e)
        }
    })?;
    drop(raw);
    drop(gpu);
    if reconstructed.len() != prepared.iter().filter(|(p, _)| p.is_some()).count() {
        return Err(Error::Invalid(
            "The GPU returned a different number of frames than it was given.",
        ));
    }
    let mut reconstructed = reconstructed.into_iter();
    let rgb: Vec<_> = prepared
        .iter()
        .map(|(p, _)| p.as_ref().and_then(|_| reconstructed.next()))
        .collect();
    prepared
        .into_par_iter()
        .zip(sources.par_iter())
        .zip(rgb)
        .map(|(((data, read_seconds), source), rgb)| {
            if let (Some((roi, _raw)), Some((rgb, corrected))) = (data, rgb) {
                let start = Instant::now();
                let output = warp_rgb_into(source, origin, size, roi, &rgb, cancel, Vec::new())?;
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
/// Neumaier compensated sum: the one f64 accumulator of the engine.
#[derive(Default)]
pub(crate) struct Sum {
    total: f64,
    correction: f64,
}
impl Sum {
    pub(crate) fn add(&mut self, x: f64) {
        let next = self.total + x;
        self.correction += if self.total.abs() >= x.abs() {
            (self.total - next) + x
        } else {
            (x - next) + self.total
        };
        self.total = next;
    }
    pub(crate) fn value(&self) -> f64 {
        self.total + self.correction
    }
}
#[cfg(test)]
fn median(values: &mut [f32]) -> f64 {
    crate::calibration::median_f32(values)
}
/// Retain the central order statistics to certify an exact clipping fixed point.
#[cfg(test)]
fn median_bounds(values: &mut [f32]) -> (f64, [f32; 2]) {
    let n = values.len();
    let (lo, m, _) = values.select_nth_unstable_by(n / 2, f32::total_cmp);
    let upper = *m;
    let lower = if n % 2 == 0 {
        lo.iter().copied().fold(f32::NEG_INFINITY, f32::max)
    } else {
        upper
    };
    let center = if n % 2 == 0 {
        (f64::from(upper) + f64::from(lower)) * 0.5
    } else {
        f64::from(upper)
    };
    (center, [lower, upper])
}

/// Clamping cannot change either central rank when these bounds hold.
/// Rounded f32 endpoint deviations must also stay beyond the upper MAD rank.
/// Then the next median/MAD and both thresholds are bit-identical; remaining
/// iterations can only repeat the same idempotent clamp on the scratch copy.
fn clipping_fixed_point(
    center: f64,
    middle: [f32; 2],
    mad_upper: f32,
    low: f64,
    high: f64,
) -> bool {
    let left = low as f32;
    let right = high as f32;
    left.is_finite()
        && right.is_finite()
        && left <= middle[0]
        && right >= middle[1]
        && ((f64::from(left) - center).abs() as f32) >= mad_upper
        && ((f64::from(right) - center).abs() as f32) >= mad_upper
}

/// Row-local storage reused across pixels, channels and clipping iterations.
struct ClippingScratch {
    /// Winsorized samples and their deviations, as sample keys.
    sample_keys: Vec<i32>,
    deviation_keys: Vec<i32>,
    /// Storage of the reference implementation in `combine`.
    #[cfg(test)]
    winsorized: Vec<f32>,
    #[cfg(test)]
    deviations: Vec<f32>,
    methods: rejection_methods::MethodScratch,
}
impl ClippingScratch {
    fn with_capacity(samples: usize) -> Self {
        Self {
            sample_keys: Vec::with_capacity(samples),
            deviation_keys: Vec::with_capacity(samples),
            #[cfg(test)]
            winsorized: Vec::with_capacity(samples),
            #[cfg(test)]
            deviations: Vec::with_capacity(samples),
            methods: Default::default(),
        }
    }
}
/// Median/MAD winsorized estimates; reject original excursions, average original survivors.
#[cfg(test)]
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
    if values.len() >= rejection_methods::MIN_SAMPLES {
        let ClippingScratch {
            winsorized,
            deviations,
            ..
        } = scratch;
        winsorized.clear();
        winsorized.extend(values.iter().map(|v| v.0));
        for _ in 0..rejection_methods::WINSORIZED_ROUNDS {
            let (center, middle) = median_bounds(winsorized);
            deviations.clear();
            deviations.extend(winsorized.iter().map(|v| (*v as f64 - center).abs() as f32));
            let (mad, mad_middle) = median_bounds(deviations);
            let sigma =
                (rejection_methods::MAD_TO_SIGMA * mad).max(rejection_methods::MIN_CLIP_SIGMA);
            low = center - config.sigma_low * sigma;
            high = center + config.sigma_high * sigma;
            if clipping_fixed_point(center, middle, mad_middle[1], low, high) {
                break;
            }
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
impl RowStats {
    /// Statistics of a band of a stack of `frames`, before any pixel.
    fn new(frames: usize) -> Self {
        Self {
            min_n: frames,
            min_fraction: 1.,
            min_surviving: 1.,
            ..Default::default()
        }
    }
}
impl IntegrationStats {
    /// Adds one reduced band's statistics.
    fn add_rows(&mut self, r: RowStats) {
        self.valid_channel_samples += r.valid;
        self.rejected_low_channel_samples += r.low;
        self.rejected_high_channel_samples += r.high;
        self.zero_coverage_pixels += r.zero;
        self.min_valid_frames = self.min_valid_frames.min(r.min_n);
        self.max_valid_frames = self.max_valid_frames.max(r.max_n);
        self.minimum_geometric_weight_fraction =
            self.minimum_geometric_weight_fraction.min(r.min_fraction);
        self.minimum_surviving_weight_fraction =
            self.minimum_surviving_weight_fraction.min(r.min_surviving);
    }
}
impl IntegrationEngine for CpuIntegrationEngine {
    fn integrate_into(
        &self,
        sources: &[IntegrationSource],
        canvas: &Canvas,
        config: &IntegrationConfig,
        original_input_count: usize,
        cancel: &AtomicBool,
        progress: Option<&(dyn Fn(usize, usize) + Sync)>,
        sink: &mut dyn TileSink,
    ) -> Result<IntegrationResult> {
        integrate_common_progress(
            sources,
            canvas,
            config,
            original_input_count,
            cancel,
            &ReconstructionEngine::Cpu,
            None,
            progress,
            Some(sink),
        )
    }
    fn integrate_with_progress(
        &self,
        sources: &[IntegrationSource],
        canvas: &Canvas,
        config: &IntegrationConfig,
        original_input_count: usize,
        cancel: &AtomicBool,
        progress: &(dyn Fn(usize, usize) + Sync),
    ) -> Result<IntegrationResult> {
        integrate_common_progress(
            sources,
            canvas,
            config,
            original_input_count,
            cancel,
            &ReconstructionEngine::Cpu,
            None,
            Some(progress),
            None,
        )
    }
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
            None,
        )
    }
}
/// One tile handed to the GPU reduction worker, and its result.
struct GpuJob {
    at: [usize; 2],
    size: [usize; 2],
    frames: Vec<Vec<[f32; 3]>>,
    tile: Vec<IntegratedPixel>,
}
struct GpuDone {
    at: [usize; 2],
    size: [usize; 2],
    frames: Vec<Vec<[f32; 3]>>,
    tile: Vec<IntegratedPixel>,
    rows: RowStats,
}
/// Destination of finished tiles: master, coverage and high-rejection maps.
struct CanvasOutput<'a, 's> {
    width: usize,
    image: &'a mut RgbImage,
    coverage: &'a mut RgbImage,
    high_rejection: &'a mut RgbImage,
    /// When set, tiles go here and the images stay empty (ADR-189).
    sink: Option<&'s mut dyn TileSink>,
}
impl CanvasOutput<'_, '_> {
    fn copy(&mut self, at: [usize; 2], size: [usize; 2], tile: &[IntegratedPixel]) -> Result<()> {
        if let Some(sink) = self.sink.as_deref_mut() {
            return sink.tile(at, size, tile);
        }
        for y in 0..size[1] {
            for x in 0..size[0] {
                let target = (at[1] + y) * self.width + at[0] + x;
                let (master, used, high) = tile[y * size[0] + x];
                self.image.pixels[target] = master;
                self.coverage.pixels[target] = used;
                self.high_rejection.pixels[target] = high;
            }
        }
        Ok(())
    }
}

/// One stack's GPU reduction: frame weights, the resolved method and the ESD
/// critical values for every possible sample count, prepared once.
struct GpuTile<'a> {
    #[cfg(feature = "gpu")]
    reducer: &'a GpuReduction,
    #[cfg(not(feature = "gpu"))]
    _never: std::marker::PhantomData<&'a GpuReduction>,
    #[cfg(feature = "gpu")]
    method: fulgur_gpu::reduce::Method,
    #[cfg(feature = "gpu")]
    weights: Vec<f32>,
    #[cfg(feature = "gpu")]
    lambda: Vec<f32>,
    #[cfg(feature = "gpu")]
    offsets: Vec<u32>,
    reserved_bytes: usize,
}
#[cfg(feature = "gpu")]
impl<'a> GpuTile<'a> {
    fn new(reducer: &'a GpuReduction, weights: &[f64], config: &IntegrationConfig) -> Result<Self> {
        use fulgur_gpu::reduce::Method;
        let lock = || Error::Invalid("GPU reduction lock failed.");
        let n = weights.len();
        let method = match config.rejection {
            Rejection::Median => Method::Median,
            Rejection::LinearFit => Method::LinearFit,
            Rejection::Esd => Method::Esd,
            Rejection::Winsorized | Rejection::Auto => Method::Winsorized {
                sigma_low: config.sigma_low as f32,
                sigma_high: config.sigma_high as f32,
            },
        };
        // Critical values depend only on the number of finite samples, which
        // varies per pixel; tabulate every count up to the stack size.
        let per_count: Vec<Vec<f32>> = if method == Method::Esd {
            (0..=n)
                .into_par_iter()
                .map(|count| {
                    let tests =
                        (count as f64 * rejection_methods::ESD_MAX_OUTLIERS).floor() as usize;
                    if count < rejection_methods::MIN_SAMPLES || tests == 0 {
                        Vec::new()
                    } else {
                        rejection_methods::esd_critical_values(
                            count,
                            tests,
                            rejection_methods::ESD_ALPHA,
                        )
                        .into_iter()
                        .map(|v| v as f32)
                        .collect()
                    }
                })
                .collect()
        } else {
            vec![Vec::new(); n + 1]
        };
        let mut offsets = Vec::with_capacity(n + 2);
        let mut lambda = Vec::new();
        for values in &per_count {
            offsets.push(lambda.len() as u32);
            lambda.extend_from_slice(values);
        }
        offsets.push(lambda.len() as u32);
        let guard = reducer.lock().map_err(|_| lock())?;
        if n > guard.max_frames() {
            return Err(Error::Invalid(
                "The stack has more frames than GPU pixel rejection can sort; use the CPU engine.",
            ));
        }
        let reserved_bytes = guard.reserved_bytes(n, lambda.len()) as usize;
        drop(guard);
        Ok(Self {
            reducer,
            method,
            weights: weights.iter().map(|&w| w as f32).collect(),
            lambda,
            offsets,
            reserved_bytes,
        })
    }

    /// Reduce one tile on the device and fill the master/coverage/high-rejection
    /// samples and statistics the CPU reducer would produce.
    fn reduce(
        &self,
        frames: &[Vec<[f32; 3]>],
        tile: &mut [IntegratedPixel],
        context: &ReductionContext<'_>,
    ) -> Result<RowStats> {
        let ReductionContext {
            original_input_count,
            total_weight,
            cancel,
            ..
        } = *context;
        let views: Vec<&[[f32; 3]]> = frames.iter().map(|f| f.as_slice()).collect();
        let reduced = self
            .reducer
            .lock()
            .map_err(|_| Error::Invalid("GPU reduction lock failed."))?
            .reduce(
                &views,
                &self.weights,
                self.method,
                &self.lambda,
                &self.offsets,
                cancel,
            )
            .map_err(gpu_error)?;
        if reduced.len() != tile.len() {
            return Err(Error::Invalid("GPU reduction returned a wrong tile size."));
        }
        let mut result = RowStats::new(frames.len());
        let inputs = original_input_count as f64;
        for (pixel, channels) in tile.iter_mut().zip(&reduced) {
            let red = channels[0];
            let n = red.samples as usize;
            result.min_n = result.min_n.min(n);
            result.max_n = result.max_n.max(n);
            result.min_fraction = result
                .min_fraction
                .min(f64::from(red.covered_weight) / total_weight);
            if n == 0 {
                result.zero += 1;
            }
            for (c, r) in channels.iter().enumerate() {
                if !r.value.is_finite() {
                    return Err(Error::Invalid("Nonfinite GPU reduction result."));
                }
                let (low, high) = (u64::from(r.rejected_low), u64::from(r.rejected_high));
                pixel.0[c] = r.value;
                let surviving = u64::from(r.samples)
                    .checked_sub(low + high)
                    .ok_or(Error::Invalid("Inconsistent GPU rejection counts."))?;
                pixel.1[c] = (surviving as f64 / inputs) as f32;
                pixel.2[c] = (high as f64 / inputs) as f32;
                result.valid += u64::from(r.samples);
                result.low += low;
                result.high += high;
                result.min_surviving = result
                    .min_surviving
                    .min(f64::from(r.surviving_weight) / total_weight);
            }
        }
        Ok(result)
    }

    fn telemetry(&self) -> Result<fulgur_gpu::reduce::GpuReductionTelemetry> {
        Ok(self
            .reducer
            .lock()
            .map_err(|_| Error::Invalid("GPU telemetry lock failed."))?
            .telemetry())
    }
}
#[cfg(not(feature = "gpu"))]
impl GpuTile<'_> {
    fn reduce(
        &self,
        _: &[Vec<[f32; 3]>],
        _: &mut [IntegratedPixel],
        _: &ReductionContext<'_>,
    ) -> Result<RowStats> {
        unreachable!("GPU reduction is not compiled")
    }
}

// Bound transposed samples to a small block, never a second registered tile.
pub(crate) const REJECTION_BLOCK_PIXELS: usize = 32;
pub(crate) fn rejection_packing_bytes(sources: usize, workers: usize) -> usize {
    sources
        .saturating_mul(workers)
        .saturating_mul(REJECTION_BLOCK_PIXELS)
        .saturating_mul(std::mem::size_of::<[f32; 3]>())
}

pub type IntegratedPixel = ([f32; 3], [f32; 3], [f32; 3]);

fn pack_rejection_block(frames: &[Vec<[f32; 3]>], offset: usize, packed: &mut [[f32; 3]]) {
    let count = frames.len();
    let pixels = packed.len() / count;
    for (index, frame) in frames.iter().enumerate() {
        // Read a contiguous short run from each frame; retain exact f32 bits
        // and the original frame index in each contiguous destination stack.
        for (sample, stack) in frame[offset..offset + pixels]
            .iter()
            .zip(packed.chunks_exact_mut(count))
        {
            stack[index] = *sample;
        }
    }
}

/// What every band reduction of one stack shares (I-03).
#[derive(Clone, Copy)]
struct ReductionContext<'a> {
    /// Frame weights, in source order.
    weights: &'a [f64],
    /// Frames before frame rejection: the denominator of the maps.
    original_input_count: usize,
    total_weight: f64,
    config: &'a IntegrationConfig,
    cancel: &'a AtomicBool,
}

fn reduce_band(
    frames: &[Vec<[f32; 3]>],
    offset: usize,
    output: &mut [IntegratedPixel],
    context: &ReductionContext<'_>,
) -> Result<RowStats> {
    let ReductionContext {
        weights,
        original_input_count,
        total_weight,
        config,
        cancel,
    } = *context;
    if config.rejection == Rejection::Esd {
        return esd_columns::reduce_band(frames, offset, output, context);
    }
    let mut result = RowStats::new(frames.len());
    let mut rgb = rgb_rejection::PreparedRgb::with_capacity(frames.len());
    let mut scratch = ClippingScratch::with_capacity(frames.len());
    let mut packed = vec![[0.; 3]; REJECTION_BLOCK_PIXELS.min(output.len()) * frames.len()];
    for (block_index, block) in output.chunks_mut(REJECTION_BLOCK_PIXELS).enumerate() {
        check_cancelled(cancel)?;
        let packed = &mut packed[..block.len() * frames.len()];
        pack_rejection_block(
            frames,
            offset + block_index * REJECTION_BLOCK_PIXELS,
            packed,
        );
        for (pixel, stack) in block.iter_mut().zip(packed.chunks_exact(frames.len())) {
            let covered = rgb.prepare(stack, weights);
            let n = rgb.samples[0].len();
            result.min_n = result.min_n.min(n);
            result.max_n = result.max_n.max(n);
            result.min_fraction = result.min_fraction.min(covered / total_weight);
            if n == 0 {
                result.zero += 1;
            }
            // A monochrome stack repeats one plane in all three channels.
            let repeated = rgb.identical_channels();
            let mut combined = (0., 0, 0, 0.);
            for (c, out) in pixel.0.iter_mut().enumerate() {
                let values = &rgb.samples[c];
                result.valid += values.len() as u64;
                if c == 0 || !repeated {
                    combined = if config.rejection == Rejection::Median {
                        let total = rgb.channel_total(c, covered);
                        rejection_methods::median_of(values, total, &mut scratch.methods)
                    } else {
                        rgb_rejection::combine_dense(
                            values,
                            rgb.channel_weights(c),
                            &mut scratch,
                            config,
                        )
                    };
                }
                let (value, low, high, surviving) = combined;
                *out = value;
                pixel.1[c] = ((values.len() as u64 - low - high) as f64
                    / original_input_count as f64) as f32;
                pixel.2[c] = (high as f64 / original_input_count as f64) as f32;
                result.low += low;
                result.high += high;
                result.min_surviving = result.min_surviving.min(surviving / total_weight);
            }
        }
    }
    Ok(result)
}

/// Reduces one reconstructed tile on the CPU, in at most `workers` bands
/// even in a larger external Rayon pool. Scratch is reused across all rows
/// and pixels belonging to one band.
fn reduce_tile(
    frames: &[Vec<[f32; 3]>],
    tile: &mut Vec<IntegratedPixel>,
    size: [usize; 2],
    workers: usize,
    context: &ReductionContext<'_>,
) -> Result<Vec<RowStats>> {
    tile.clear();
    tile.resize(size[0] * size[1], ([0.; 3], [0.; 3], [0.; 3]));
    let band_pixels = size[0] * size[1].div_ceil(workers);
    tile.par_chunks_mut(band_pixels)
        .enumerate()
        .map(|(band, output)| reduce_band(frames, band * band_pixels, output, context))
        .collect()
}

/// The GPU reduction worker: reduces the tiles it is sent, one at a time,
/// and stops at the first failure.
fn reduce_on_device(
    gpu: &GpuTile<'_>,
    queue: std::sync::mpsc::Receiver<GpuJob>,
    done: std::sync::mpsc::Sender<Result<GpuDone>>,
    context: &ReductionContext<'_>,
) {
    for job in queue {
        let GpuJob {
            at,
            size,
            frames,
            mut tile,
        } = job;
        tile.clear();
        tile.resize(size[0] * size[1], ([0.; 3], [0.; 3], [0.; 3]));
        let result = gpu.reduce(&frames, &mut tile, context).map(|rows| GpuDone {
            at,
            size,
            frames,
            tile,
            rows,
        });
        let failed = result.is_err();
        if done.send(result).is_err() || failed {
            break;
        }
    }
}

/// Refuses a plan the integration cannot honour, before memory is planned.
fn check_plan(
    sources: &[IntegrationSource],
    canvas: &Canvas,
    original_input_count: usize,
    is_gpu: bool,
) -> Result<()> {
    if original_input_count < sources.len()
        || original_input_count == 0
        || sources.is_empty()
        || canvas.width == 0
        || canvas.height == 0
        || sources.iter().any(|s| {
            s.distortion.is_some_and(|m| {
                !m.valid() || !m.fits_sensor([s.header.info.width, s.header.info.height])
            }) || s.weight <= 0.
                || s.weight > 1.
                || s.scale <= 0.
                || !fulgur_core::geometry_scale_supported(s.transform.scale_factor)
                || s.header.info.width < 6
                || s.header.info.height < 6
                || ![
                    s.scale,
                    s.offset[0],
                    s.offset[1],
                    s.offset[2],
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
    if sources
        .iter()
        .any(|s| s.header.info.mosaic == fulgur_core::Mosaic::Unknown)
    {
        return Err(Error::Invalid(
            "Sensor layout is unknown; choose colour or mono for this light set.",
        ));
    }
    if is_gpu
        && sources
            .iter()
            .any(|s| s.header.info.mosaic != fulgur_core::Mosaic::Bayer)
    {
        return Err(Error::Invalid(
            "GPU reconstruction supports colour (Bayer) frames only. Use the CPU engine for monochrome or debayered light sets.",
        ));
    }
    Ok(())
}

/// A purely monochrome stack keeps identical channels and is written as one
/// plane; any colour source makes the result an RGB master.
pub(crate) fn output_method(sources: &[IntegrationSource]) -> DemosaicMethod {
    if sources
        .iter()
        .all(|s| s.header.info.mosaic == fulgur_core::Mosaic::Mono)
    {
        DemosaicMethod::None
    } else if sources
        .iter()
        .all(|s| s.header.info.mosaic == fulgur_core::Mosaic::Rgb)
    {
        DemosaicMethod::External
    } else {
        DemosaicMethod::Vng
    }
}

/// An all-zero output plane of the canvas.
fn blank(canvas: &Canvas, method: DemosaicMethod) -> RgbImage {
    RgbImage {
        width: canvas.width,
        height: canvas.height,
        demosaic_method: method,
        pixels: vec![[0.; 3]; canvas.width * canvas.height],
    }
}

/// The tile size and memory plan, the FITS block cache, and the decoded-row
/// budget of compressed XISF sources.
fn plan_tiles(
    sources: &[IntegrationSource],
    canvas: &Canvas,
    config: &IntegrationConfig,
    workers: usize,
    is_gpu: bool,
    gpu_reduction_bytes: usize,
    halo: usize,
) -> Result<(MemoryPlan, FitsBlockCache)> {
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
        gpu_reduction_bytes,
        distortion_margin: sources
            .iter()
            .filter_map(|s| s.distortion)
            .flat_map(|m| m.source_margin())
            .fold(0_f64, f64::max)
            .ceil() as usize,
        halo,
        drizzle_scale: config.drizzle.map_or(0, |d| d.scale),
    }
    .plan(config, fulgur_core::telemetry::system_memory())?;
    let paths: Vec<_> = sources.iter().map(|s| s.path.clone()).collect();
    let max_width = sources.iter().map(|s| s.header.info.width).max().unwrap();
    memory_plan.allocate_cache(config.fits_cache_mib, sources.len(), workers, max_width);
    // Compressed XISF cannot be read by rows: half of the free memory keeps a
    // band of decoded rows per frame, the other half whole frames being decoded.
    let band_row_bytes: usize = sources
        .iter()
        .filter_map(|s| s.header.xisf.as_ref())
        .map(|image| image.band_row_bytes())
        .sum();
    let free = memory_plan
        .effective_budget_bytes
        .saturating_sub(memory_plan.estimated_buffer_bytes as u64) as usize;
    if let Some(rows) = (free / 2).checked_div(band_row_bytes) {
        let frame = sources
            .iter()
            .filter_map(|s| s.header.xisf.as_ref())
            .map(|image| image.decoded_bytes())
            .max()
            .unwrap_or(1)
            .max(1);
        fulgur_core::xisf::set_decode_limit((free / 2 / frame).clamp(1, workers));
        for image in sources.iter().filter_map(|s| s.header.xisf.as_ref()) {
            image.set_band_rows(rows);
        }
    }
    let cache = FitsBlockCache::new(&paths, memory_plan.fits_cache_capacity_bytes);
    Ok((memory_plan, cache))
}

/// The memory plan on the console (CLI only: native GUI processes need not
/// have a stderr handle).
fn print_plan(memory_plan: &MemoryPlan, config: &IntegrationConfig) {
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
        memory_plan.selected_width,
        memory_plan.selected_height,
        memory_plan.estimated_buffer_bytes as f64 / 1048576.,
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
}

/// Reads, calibrates and warps every source onto one output tile (I-02).
struct TileReader<'a> {
    sources: &'a [IntegrationSource],
    backend: &'a ReconstructionEngine,
    workers: usize,
    /// The configured memory budget in bytes, checked against measured RSS.
    budget: usize,
    cache: &'a FitsBlockCache,
    /// Reused allocations instead of fresh, OS-zeroed pages for every source
    /// and tile: at most one worker scratch per active slot.
    scratch: std::sync::Mutex<Vec<RoiScratch>>,
    cancel: &'a AtomicBool,
    /// Drizzle instead of resampling; tile coordinates are then output pixels
    /// of the finer grid (ADR-188).
    drizzle: Option<Drizzle>,
}

impl TileReader<'_> {
    fn check_memory(&self) -> Result<()> {
        if fulgur_core::telemetry::peak_rss_bytes().is_some_and(|v| v > self.budget as u64) {
            Err(Error::Invalid(
                "Measured process memory exceeded the configured budget.",
            ))
        } else {
            Ok(())
        }
    }

    /// Every source on the tile at `origin`, reusing `recycled` sample
    /// buffers as output storage.
    fn read(
        &self,
        recycled: &mut Vec<Vec<[f32; 3]>>,
        origin: [i32; 2],
        size: [usize; 2],
    ) -> Result<Vec<WarpedTile>> {
        let (sources, workers, cancel) = (self.sources, self.workers, self.cancel);
        Ok(match self.backend {
            ReconstructionEngine::Cpu => {
                use std::sync::Mutex;
                use std::sync::atomic::{AtomicUsize, Ordering};
                let completed = AtomicUsize::new(0);
                let lock_failed = || Error::Invalid("Integration buffer lock failed.");
                // Each index is claimed exactly once, so each recycled
                // buffer has a single owner.
                let outputs: Vec<_> = recycled.drain(..).map(Mutex::new).collect();
                crate::cpu_schedule::map_bounded(sources.len(), workers, cancel, |index| {
                    let output = match outputs.get(index) {
                        Some(slot) => std::mem::take(&mut *slot.lock().map_err(|_| lock_failed())?),
                        None => Vec::new(),
                    };
                    let mut scratch = self
                        .scratch
                        .lock()
                        .map_err(|_| lock_failed())?
                        .pop()
                        .unwrap_or_default();
                    let result = match self.drizzle {
                        Some(drizzle) => drizzle::warp(
                            &sources[index],
                            origin,
                            size,
                            drizzle,
                            cancel,
                            self.cache,
                            &mut scratch,
                            output,
                        ),
                        None => warp(
                            &sources[index],
                            origin,
                            size,
                            cancel,
                            self.cache,
                            &mut scratch,
                            output,
                        ),
                    };
                    self.scratch
                        .lock()
                        .map_err(|_| lock_failed())?
                        .push(scratch);
                    let result = result?;
                    let count = completed.fetch_add(1, Ordering::Relaxed) + 1;
                    // Same check frequency as the prior groups, including
                    // the final partial group; measured peak RSS is retained.
                    if count % workers == 0 || count == sources.len() {
                        self.check_memory()?;
                    }
                    Ok(result)
                })?
            }
            #[cfg(feature = "gpu")]
            ReconstructionEngine::Gpu(gpu) => {
                // Keep device dispatch/readback in the original bounded batches.
                let mut buffers = Vec::with_capacity(sources.len());
                for batch in sources.chunks(workers) {
                    buffers.extend(gpu_batch(gpu, batch, origin, size, cancel, self.cache)?);
                    self.check_memory()?;
                }
                buffers
            }
        })
    }
}

fn integrate_common(
    sources: &[IntegrationSource],
    canvas: &Canvas,
    config: &IntegrationConfig,
    original_input_count: usize,
    cancel: &AtomicBool,
    backend: &ReconstructionEngine,
    reducer: Option<&GpuReduction>,
) -> Result<IntegrationResult> {
    integrate_common_progress(
        sources,
        canvas,
        config,
        original_input_count,
        cancel,
        backend,
        reducer,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn integrate_common_progress(
    sources: &[IntegrationSource],
    canvas: &Canvas,
    config: &IntegrationConfig,
    original_input_count: usize,
    cancel: &AtomicBool,
    backend: &ReconstructionEngine,
    reducer: Option<&GpuReduction>,
    on_progress: Option<&(dyn Fn(usize, usize) + Sync)>,
    mut sink: Option<&mut dyn TileSink>,
) -> Result<IntegrationResult> {
    config.validate()?;
    check_cancelled(cancel)?;
    // `Auto` becomes one concrete method for the whole stack (ADR-087).
    let rejection = config.rejection.resolve(sources.len());
    // The trail mask extends ESD's rejections; other methods ignore it.
    let trail = if rejection == Rejection::Esd && config.drizzle.is_none() {
        config.trail
    } else {
        TrailMask {
            mode: TrailMode::Off,
            ..config.trail
        }
    };
    let resolved = IntegrationConfig {
        rejection,
        trail,
        ..config.clone()
    };
    let config = &resolved;
    let halo = config.trail.halo();
    // The trail mask needs whole tiles on the CPU, so pixel rejection stays
    // there when it is on.
    let reducer = reducer.filter(|_| halo == 0 && config.drizzle.is_none());
    if config.drizzle.is_some() && !matches!(backend, ReconstructionEngine::Cpu) {
        return Err(Error::Invalid(
            "Drizzle integration runs on the CPU engine; choose the CPU engine.",
        ));
    }
    let is_gpu = !matches!(backend, ReconstructionEngine::Cpu);
    check_plan(sources, canvas, original_input_count, is_gpu)?;
    let start = Instant::now();
    let budget = config.memory_budget_mib * 1024 * 1024;
    let output_method = output_method(sources);
    #[cfg(feature = "gpu")]
    let gpu_reduction = match reducer {
        Some(reducer) => Some(GpuTile::new(
            reducer,
            &sources.iter().map(|s| s.weight).collect::<Vec<_>>(),
            config,
        )?),
        None => None,
    };
    #[cfg(not(feature = "gpu"))]
    let gpu_reduction: Option<GpuTile> = reducer.map(|never| match *never {});
    let workers = rayon::current_num_threads().min(if is_gpu {
        16
    } else {
        crate::cpu_schedule::MAX_CPU_WORKERS
    });
    let cpu_start = fulgur_core::telemetry::process_cpu_time();
    let mut cpu_profile = IntegrationCpuProfile {
        available_logical_processors: std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1),
        pool_threads: rayon::current_num_threads(),
        reconstruction_workers: workers.min(sources.len()),
        reduction_workers: workers.min(canvas.height),
        ..Default::default()
    };
    let (memory_plan, cache) = plan_tiles(
        sources,
        canvas,
        config,
        workers,
        is_gpu,
        gpu_reduction.as_ref().map_or(0, |g| g.reserved_bytes),
        halo,
    )?;
    let tile_width = memory_plan.selected_width;
    let tile_height = memory_plan.selected_height;
    let planned = memory_plan.estimated_buffer_bytes;
    // The observer is the GUI's progress channel; console diagnostics stay on
    // the unchanged CLI path only.
    if on_progress.is_none() {
        print_plan(&memory_plan, config);
    }
    // With a sink the whole images are never made.
    let streamed = sink.is_some();
    let output = |method| {
        if streamed {
            RgbImage {
                width: 0,
                height: 0,
                demosaic_method: method,
                pixels: vec![],
            }
        } else {
            blank(canvas, method)
        }
    };
    let mut image = output(output_method);
    let mut coverage = output(output_method);
    let mut high_rejection = output(output_method);
    let weights: Vec<_> = sources.iter().map(|s| s.weight).collect();
    let mut total_weight = Sum::default();
    for w in &weights {
        total_weight.add(*w);
    }
    let reduction = ReductionContext {
        weights: &weights,
        original_input_count,
        total_weight: total_weight.value(),
        config,
        cancel,
    };
    let mut stats = IntegrationStats {
        backend: match (is_gpu, gpu_reduction.is_some()) {
            (true, true) => "gpu-cosmetic-vng-cpu-warp-gpu-reduction",
            (true, false) => "gpu-cosmetic-vng-cpu-warp-reduction",
            (false, true) => "cpu-reconstruction-gpu-reduction",
            (false, false) => "cpu",
        },
        tile_width,
        tile_height,
        max_concurrent_rois: workers.min(sources.len()),
        planned_buffer_bytes: planned,
        reduction_layout: "pixel-major-blocks",
        rejection: config.rejection.key(),
        rejection_rule: config.rejection.rule(config),
        trail_rule: config.trail.rule(),
        drizzle_rule: config.drizzle.map_or("off".into(), |d| d.rule()),
        rejection_block_pixels: REJECTION_BLOCK_PIXELS,
        max_concurrent_reduction_bands: workers.min(canvas.height),
        rejection_packing_buffer_bytes: rejection_packing_bytes(sources.len(), workers),
        memory_plan: Some(memory_plan),
        min_valid_frames: sources.len(),
        minimum_geometric_weight_fraction: 1.,
        minimum_surviving_weight_fraction: 1.,
        ..Default::default()
    };
    let mut progress = Instant::now();
    let total_tiles = canvas.width.div_ceil(tile_width) * canvas.height.div_ceil(tile_height);
    // One set of tile samples exists at a time (the previous tile's, consumed
    // as the next is produced). With GPU pixel rejection the device reduces
    // tile k while the CPU reconstructs tile k + 1, so two sample sets exist
    // (ADR-090).
    let reader = TileReader {
        sources,
        backend,
        workers,
        budget,
        cache: &cache,
        scratch: std::sync::Mutex::new(Vec::with_capacity(workers)),
        cancel,
        drizzle: config.drizzle,
    };
    let mut recycled: Vec<Vec<[f32; 3]>> = Vec::new();
    let mut tile = Vec::new();
    let mut canvas_out = CanvasOutput {
        width: canvas.width,
        image: &mut image,
        coverage: &mut coverage,
        high_rejection: &mut high_rejection,
        sink: sink.take(),
    };
    std::thread::scope(|scope| -> Result<()> {
        let mut device = None;
        if let Some(gpu) = &gpu_reduction {
            let (jobs, queue) = std::sync::mpsc::sync_channel::<GpuJob>(1);
            let (done, finished) = std::sync::mpsc::channel::<Result<GpuDone>>();
            scope.spawn(move || reduce_on_device(gpu, queue, done, &reduction));
            device = Some((jobs, finished));
        }
        let mut in_flight = false;
        let mut spare_tile = Vec::new();
        let mut publish = |stats: &mut IntegrationStats,
                           cpu_profile: &mut IntegrationCpuProfile,
                           at: [usize; 2],
                           size: [usize; 2],
                           tile: &[IntegratedPixel],
                           rows: Vec<RowStats>|
         -> Result<()> {
            for r in rows {
                stats.add_rows(r);
            }
            let copy_start = Instant::now();
            let copy_cpu_start = fulgur_core::telemetry::process_cpu_time();
            canvas_out.copy(at, size, tile)?;
            stats.output_copy_wall_seconds += copy_start.elapsed().as_secs_f64();
            cpu_profile.output_copy_cpu_seconds +=
                cpu_elapsed(copy_cpu_start).map_or(0., |v| v.total());
            if on_progress.is_none() && progress.elapsed().as_secs() >= 30 {
                eprintln!(
                    "Integration: {}/{} tiles completed in {:.1} s.",
                    stats.tile_count + 1,
                    total_tiles,
                    start.elapsed().as_secs_f64()
                );
                progress = Instant::now();
            }
            stats.tile_count += 1;
            if let Some(notify) = on_progress {
                notify(stats.tile_count, total_tiles);
            }
            Ok(())
        };
        // Waits for the tile on the device; only the wait not hidden behind
        // reconstruction counts as reduction wall time.
        let receive = |stats: &mut IntegrationStats,
                       finished: &std::sync::mpsc::Receiver<Result<GpuDone>>|
         -> Result<GpuDone> {
            let wait = Instant::now();
            let done = finished
                .recv()
                .map_err(|_| Error::Invalid("GPU reduction worker stopped."))??;
            stats.reduction_wall_seconds += wait.elapsed().as_secs_f64();
            Ok(done)
        };
        for y0 in (0..canvas.height).step_by(tile_height) {
            for x0 in (0..canvas.width).step_by(tile_width) {
                check_cancelled(cancel)?;
                let size = [
                    tile_width.min(canvas.width - x0),
                    tile_height.min(canvas.height - y0),
                ];
                // With the trail mask the tile is read with a halo, within the
                // canvas; only `inner` is written.
                let left = x0.min(halo);
                let top = y0.min(halo);
                let read_size = [
                    left + size[0] + halo.min(canvas.width - x0 - size[0]),
                    top + size[1] + halo.min(canvas.height - y0 - size[1]),
                ];
                let origin = [
                    canvas.origin_x_px + (x0 - left) as i32,
                    canvas.origin_y_px + (y0 - top) as i32,
                ];
                let reconstruction_start = Instant::now();
                let reconstruction_cpu_start = fulgur_core::telemetry::process_cpu_time();
                let buffers = reader.read(&mut recycled, origin, read_size)?;
                let mut frames = Vec::with_capacity(sources.len());
                for (pixels, decoded, corrected, timing) in buffers {
                    stats.roi_task_elapsed.add(timing);
                    frames.push(pixels);
                    stats.decoded_roi_pixels += decoded;
                    stats.corrected_roi_pixels += corrected;
                }
                stats.reconstruction_wall_seconds += reconstruction_start.elapsed().as_secs_f64();
                cpu_profile.reconstruction_cpu_seconds +=
                    cpu_elapsed(reconstruction_cpu_start).map_or(0., |v| v.total());
                if let Some((jobs, finished)) = &device {
                    if in_flight {
                        let done = receive(&mut stats, finished)?;
                        publish(
                            &mut stats,
                            &mut cpu_profile,
                            done.at,
                            done.size,
                            &done.tile,
                            vec![done.rows],
                        )?;
                        recycled = done.frames;
                        spare_tile = done.tile;
                    }
                    jobs.send(GpuJob {
                        at: [x0, y0],
                        size,
                        frames,
                        tile: std::mem::take(&mut spare_tile),
                    })
                    .map_err(|_| Error::Invalid("GPU reduction worker stopped."))?;
                    in_flight = true;
                    continue;
                }
                let reduction_start = Instant::now();
                let reduction_cpu_start = fulgur_core::telemetry::process_cpu_time();
                let rows = if config.drizzle.is_some() {
                    drizzle::reduce_tile(&frames, &mut tile, size, &reduction)?
                } else if halo > 0 {
                    let (rows, masked) = trail_mask::reduce_tile(
                        &frames,
                        read_size,
                        [left, top, size[0], size[1]],
                        &mut tile,
                        &reduction,
                    )?;
                    stats.trail_masked_channel_samples += masked;
                    rows
                } else {
                    reduce_tile(&frames, &mut tile, size, workers, &reduction)?
                };
                // The reduced samples become the next tile's output storage.
                recycled = frames;
                stats.reduction_wall_seconds += reduction_start.elapsed().as_secs_f64();
                cpu_profile.reduction_cpu_seconds +=
                    cpu_elapsed(reduction_cpu_start).map_or(0., |v| v.total());
                publish(&mut stats, &mut cpu_profile, [x0, y0], size, &tile, rows)?;
            }
        }
        if let Some((jobs, finished)) = device {
            drop(jobs);
            if in_flight {
                let done = receive(&mut stats, &finished)?;
                publish(
                    &mut stats,
                    &mut cpu_profile,
                    done.at,
                    done.size,
                    &done.tile,
                    vec![done.rows],
                )?;
            }
        }
        Ok(())
    })?;
    #[cfg(feature = "gpu")]
    if let Some(gpu) = &gpu_reduction {
        stats.gpu_reduction = Some(gpu.telemetry()?);
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
    stats.cpu_profile = cpu_elapsed(cpu_start).map(|process| {
        cpu_profile.process = process;
        cpu_profile
    });
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
#[path = "integrate_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "rejection_tests.rs"]
mod rejection_tests;

#[path = "drizzle.rs"]
mod drizzle;
#[path = "esd_columns.rs"]
mod esd_columns;
#[path = "rejection_methods.rs"]
mod rejection_methods;
#[path = "rgb_rejection.rs"]
mod rgb_rejection;
#[path = "trail_mask.rs"]
mod trail_mask;

#[cfg(test)]
#[path = "integrate_distortion_tests.rs"]
mod distortion_tests;
