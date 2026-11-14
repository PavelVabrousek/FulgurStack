//! `stack`: inspect, register and integrate lights into linear masters.
use crate::CANCEL;
use crate::calibration::{DarkArgs, master_dark};
use crate::files::{
    companion, destination_identity, filename, labelled, peak_mib, save_report, validate_report,
};
use crate::lights::{
    DerivedArgs, ReferenceStrategyArg, check_derived, parse_accuracy, parse_layout,
};
use crate::sky::SkyArgs;
use fulgur_core::output::{OutputFormat, RgbExportStats, ensure_new_destination};
use fulgur_core::{Error, LayoutChoice, Result};
use fulgur_pipeline::integrate::{IntegrationConfig, IntegrationEngine, TileMode};
use fulgur_pipeline::master_groups::{MasterRun, MasterSplit};
use fulgur_pipeline::register::{Canvas, RegistrationReport};
use fulgur_pipeline::stack::PublishedMaster;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Integration engine.
#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum EngineArg {
    Cpu,
    Gpu,
}

/// Stages the GPU engine runs on the device.
#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum GpuStagesArg {
    Reduction,
    Reconstruction,
    All,
}

impl GpuStagesArg {
    #[cfg_attr(not(feature = "gpu"), allow(dead_code))]
    fn key(self) -> &'static str {
        match self {
            Self::Reduction => "reduction",
            Self::Reconstruction => "reconstruction",
            Self::All => "all",
        }
    }
}

/// Registration engine.
#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum RegistrationEngineArg {
    Stars,
    Sky,
}

/// Output canvas policy.
#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum CanvasArg {
    Reference,
    CoverageOptimized,
    CommonIntersection,
}

impl CanvasArg {
    /// The name written into the report.
    fn key(self) -> &'static str {
        match self {
            Self::Reference => "reference",
            Self::CoverageOptimized => "coverage-optimized",
            Self::CommonIntersection => "common-intersection",
        }
    }
}

/// Square manual tile size, or automatic.
#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum TileSizeArg {
    Auto,
    #[value(name = "128")]
    Px128,
    #[value(name = "256")]
    Px256,
    #[value(name = "512")]
    Px512,
}

impl TileSizeArg {
    fn pixels(self) -> Option<usize> {
        match self {
            Self::Auto => None,
            Self::Px128 => Some(128),
            Self::Px256 => Some(256),
            Self::Px512 => Some(512),
        }
    }
}

fn parse_masters(value: &str) -> std::result::Result<MasterSplit, String> {
    MasterSplit::parse(value).ok_or_else(|| "use combined, per-filter or per-set".to_owned())
}

fn parse_rejection(
    value: &str,
) -> std::result::Result<fulgur_pipeline::integrate::Rejection, String> {
    fulgur_pipeline::integrate::Rejection::parse(value).map_err(|e| e.to_string())
}

#[derive(clap::Args)]
pub(crate) struct StackArgs {
    /// Auto uses the median for one night and geometric coverage for multiple nights.
    #[arg(long, value_enum, default_value = "auto")]
    reference_strategy: ReferenceStrategyArg,
    /// Explicit integration engine. GPU requires --features gpu; never silently falls back.
    #[arg(long, value_enum, default_value = "cpu")]
    engine: EngineArg,
    /// Stages the GPU engine runs on the device: pixel rejection and
    /// weighted mean (reduction), cosmetics and VNG (reconstruction), or all.
    #[arg(long, value_enum, default_value = "reduction")]
    gpu_stages: GpuStagesArg,
    #[arg(long, required = true)]
    input: Vec<PathBuf>,
    /// Sensor layout: auto (BAYERPAT header), mono (no demosaicing) or a Bayer pattern.
    #[arg(long, default_value = "auto", value_parser = parse_layout)]
    sensor: LayoutChoice,
    /// Treat each --input as its own light set (own quality limits; filters,
    /// gain and exposure may differ between sets).
    #[arg(long)]
    input_sets: bool,
    /// Masters of this run, all on one pixel grid: combined, per-filter or per-set.
    #[arg(long, default_value = "combined", value_parser = parse_masters)]
    masters: MasterSplit,
    #[command(flatten)]
    dark: Box<DarkArgs>,
    #[command(flatten)]
    derived: DerivedArgs,
    /// Registration engine: stars (default) or sky, every frame plate-solved
    /// against Gaia DR3 (for mixed cameras; geometric correction always on).
    #[arg(long, value_enum, default_value = "stars")]
    registration_engine: RegistrationEngineArg,
    #[command(flatten)]
    sky: Box<SkyArgs>,
    #[arg(long)]
    output: PathBuf,
    #[arg(long)]
    report: PathBuf,
    #[arg(long, value_enum, default_value = "reference")]
    canvas: CanvasArg,
    /// Accepted input or external geometry-only FITS (30..333% scale); overrides reference strategy.
    #[arg(long)]
    reference: Option<PathBuf>,
    /// Also fit excluded frames for diagnostics; never include them in integration.
    #[arg(long)]
    diagnose_excluded: bool,
    /// Correct optical distortion only when held-out stars confirm improvement.
    #[arg(long)]
    geometric_correction: bool,
    /// Experimental: a frame refused by whole-field validation still joins the
    /// tiles around which its stars stay accurate.
    #[arg(long, requires = "geometric_correction")]
    partial_frames: bool,
    /// Whole-field limits: maximum-accuracy (1 px RMS, 3 px, 80 %, 50 stars),
    /// compromise (2, 6, 60 %, 30), maximum-coverage (3, 9, 40 %, 20), or
    /// RMS,PERCENT,MIN_STARS such as 2,50,30.
    #[arg(long, default_value = "maximum-accuracy", value_parser = parse_accuracy)]
    accuracy: fulgur_pipeline::register::AccuracyLimits,
    /// Optional map destinations; otherwise derived from the master filename.
    #[arg(long)]
    coverage_output: Option<PathBuf>,
    #[arg(long)]
    high_rejection_output: Option<PathBuf>,
    #[arg(long, default_value_t = 0.80)]
    min_coverage: f64,
    /// Pixel rejection: auto (by frame count), winsorized, linear-fit, esd or median.
    #[arg(long, default_value = "auto", value_parser = parse_rejection)]
    rejection: fulgur_pipeline::integrate::Rejection,
    /// Winsorized clipping limits in sigmas.
    #[arg(long, default_value_t = 3.0)]
    sigma_low: f64,
    #[arg(long, default_value_t = 2.5)]
    sigma_high: f64,
    #[arg(long, default_value_t = 12288)]
    memory_budget_mib: usize,
    /// Maximum original-byte FITS block cache, reduced to available RAM; 0 disables it.
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u16).range(0..=1024))]
    fits_cache_mib: u16,
    /// Default: auto-select using available RAM and a system reserve, 1024 x 256 on frames wider than 4000 px, else 512 x 256, then smaller. A number selects a manual square.
    #[arg(long, value_enum, conflicts_with_all = ["tile_width", "tile_height"])]
    tile_size: Option<TileSizeArg>,
    /// Manual width; omitted companion height defaults to 512. Fails if memory cannot fit.
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..=1024))]
    tile_width: Option<u16>,
    /// Manual height; omitted companion width defaults to 512. Fails if memory cannot fit.
    #[arg(long, value_parser = clap::value_parser!(u16).range(1..=512))]
    tile_height: Option<u16>,
}

impl StackArgs {
    /// GPU reconstruction dispatches at most 16 regions at once, so the CPU
    /// pool is capped lower for it.
    pub(crate) fn gpu_reconstruction(&self) -> bool {
        self.engine == EngineArg::Gpu && self.gpu_stages != GpuStagesArg::Reduction
    }

    /// The tile choice and rejection settings, validated.
    fn integration_config(&self) -> Result<IntegrationConfig> {
        let manual_size = self.tile_size.and_then(TileSizeArg::pixels);
        let tile_mode =
            if manual_size.is_some() || self.tile_width.is_some() || self.tile_height.is_some() {
                TileMode::Manual
            } else {
                TileMode::Auto
            };
        let config = IntegrationConfig {
            rejection: self.rejection,
            sigma_low: self.sigma_low,
            sigma_high: self.sigma_high,
            memory_budget_mib: self.memory_budget_mib,
            fits_cache_mib: usize::from(self.fits_cache_mib),
            tile_mode,
            tile_width: manual_size.unwrap_or(usize::from(self.tile_width.unwrap_or(512))),
            tile_height: manual_size.unwrap_or(usize::from(self.tile_height.unwrap_or(512))),
        };
        config.validate()?;
        Ok(config)
    }

    fn integration_engine(&self) -> Result<Box<dyn IntegrationEngine>> {
        #[cfg(not(feature = "gpu"))]
        let _ = &self.gpu_stages;
        Ok(match self.engine {
            EngineArg::Cpu => Box::new(fulgur_pipeline::integrate::CpuIntegrationEngine),
            #[cfg(feature = "gpu")]
            EngineArg::Gpu => Box::new(
                fulgur_pipeline::integrate::GpuIntegrationEngine::with_stages(
                    fulgur_pipeline::integrate::GpuStages::parse(self.gpu_stages.key())
                        .ok_or(Error::Invalid("Unknown GPU stages."))?,
                )?,
            ),
            #[cfg(not(feature = "gpu"))]
            EngineArg::Gpu => {
                return Err(Error::Invalid(
                    "GPU backend is not compiled; add --features gpu or use --engine cpu.",
                ));
            }
        })
    }
}

/// Master, coverage map, high-rejection map and report of one master.
type Destinations = [PathBuf; 4];

/// Checks every destination before anything is processed: the master's
/// format, new files only, map formats that follow the master (ADR-142), and
/// four distinct files.
fn check_destinations(args: &StackArgs) -> Result<Destinations> {
    validate_report(Some(&args.report))?;
    // The extension chooses FITS or XISF; maps follow the master (ADR-142).
    let Some(format) = OutputFormat::of(&args.output) else {
        return Err(Error::Invalid(
            "Master output must end in .fit, .fits, .fts or .xisf.",
        ));
    };
    ensure_new_destination(&args.output)?;
    let maps = format.extension();
    let coverage = args
        .coverage_output
        .clone()
        .unwrap_or_else(|| companion(&args.output, &format!("_coverage.{maps}")));
    let high_rejection = args
        .high_rejection_output
        .clone()
        .unwrap_or_else(|| companion(&args.output, &format!("_high_rejection.{maps}")));
    for path in [&coverage, &high_rejection] {
        if OutputFormat::of(path).is_none() {
            return Err(Error::Invalid(
                "Diagnostic maps require FITS or XISF destinations.",
            ));
        }
        ensure_new_destination(path)?;
    }
    let all = [
        args.output.clone(),
        coverage,
        high_rejection,
        args.report.clone(),
    ];
    let mut identities = std::collections::BTreeSet::new();
    for path in [&all[0], &all[3], &all[1], &all[2]] {
        if !identities.insert(destination_identity(path)?) {
            return Err(Error::Invalid(
                "Master, maps and report require distinct destinations.",
            ));
        }
    }
    Ok(all)
}

/// The light sets: one per input with `--input-sets`, else one set.
fn light_sets(
    args: &StackArgs,
    calibration: &Option<std::sync::Arc<fulgur_pipeline::calibration::Calibration>>,
) -> Result<Vec<fulgur_pipeline::inspect::InputSet>> {
    let sets: Vec<_> = if args.input_sets {
        let count = args.input.len();
        args.input
            .iter()
            .enumerate()
            .map(|(index, path)| {
                Ok(fulgur_pipeline::inspect::InputSet {
                    label: filename(path),
                    inputs: vec![path.clone()],
                    layout: args.sensor,
                    exposure: args.derived.exposure_for(index, count)?,
                    accept_derived: args.derived.accept_derived,
                    calibration: calibration.clone(),
                })
            })
            .collect::<Result<_>>()?
    } else {
        let mut set = fulgur_pipeline::inspect::single_set(&args.input, args.sensor);
        set.calibration = calibration.clone();
        args.derived.apply(&mut set)?;
        vec![set]
    };
    check_derived(&sets, &args.derived)?;
    Ok(sets)
}

fn print_registration(plan: &RegistrationReport) {
    if let Some(summary) = &plan.sky {
        println!(
            "Sky registration: {}/{} frames plate-solved in {:.1} s, {} registered through the sky, {} by stars; reference {} ({:.3} arcsec/px).",
            summary.solved,
            summary.frames_to_solve,
            summary.solve_seconds,
            summary.registered_by_sky,
            summary.registered_by_stars,
            filename(Path::new(&summary.reference)),
            summary.reference_arcsec_per_px
        );
        if let Some(note) = &summary.fallback_note {
            println!("Sky registration: {note}.");
        }
    }
    if let Some(warning) = plan.inspection.target_warning() {
        println!("Warning: {warning}.");
    }
    if let Some(warning) = plan.auto_reference.as_ref().and_then(|a| a.warning()) {
        println!("Warning: {warning}.");
    }
    if let Some(line) = plan.camera_summary() {
        println!("Cameras: {line}.");
    }
    if let Some(line) = plan.set_photometry_summary() {
        println!("Brightness: {line}.");
    }
}

pub(crate) fn run(args: StackArgs) -> Result<()> {
    let [output, coverage_output, high_rejection_output, report] = check_destinations(&args)?;
    let config = args.integration_config()?;
    let start = Instant::now();
    fulgur_core::telemetry::timeline("stack start");
    let integration_engine = args.integration_engine()?;
    let calibration = master_dark(&args.dark)?;
    let sky = (*args.sky).clone().with_folders(&args.input);
    let sets = light_sets(&args, &calibration)?;
    let options = fulgur_pipeline::register::RegistrationOptions {
        reference_strategy: args.reference_strategy.into(),
        diagnose_excluded: args.diagnose_excluded,
        geometric_correction: args.geometric_correction,
        partial_frames: args.partial_frames,
        accuracy: args.accuracy,
    };
    let sky_registration = args.registration_engine == RegistrationEngineArg::Sky;
    let sky_for = |inspection: &fulgur_pipeline::inspect::InspectReport| sky.options(inspection);
    let (plan, root) = fulgur_pipeline::register::register_sets(
        &sets,
        args.min_coverage,
        args.reference.as_deref(),
        options,
        sky_registration.then_some(&sky_for as _),
        &CANCEL,
    )?;
    fulgur_core::telemetry::timeline("registration done");
    print_registration(&plan);
    let reference_canvas = Canvas {
        origin_x_px: 0,
        origin_y_px: 0,
        width: plan.reference_width,
        height: plan.reference_height,
        minimum_effective_weight_fraction: 0.,
        coordinate_rule: "exact reference FITS dimensions and sensor pixel grid; no automatic crop",
    };
    let chosen = match args.canvas {
        CanvasArg::Reference => Some(&reference_canvas),
        CanvasArg::CommonIntersection => plan.common_intersection.as_ref(),
        CanvasArg::CoverageOptimized => plan.coverage_optimized.as_ref(),
    }
    .ok_or(Error::Invalid(
        "No safe canvas for the selected coverage policy.",
    ))?;
    // The grid's plate solution for the masters' WCS (ADR-155): sky
    // registration has it; a star registration solves its reference
    // when a Gaia catalogue was given. Why a tried plate solve gave no
    // WCS goes into the report (ADR-158).
    let wanted = sky_registration || sky.has_catalogue();
    let grid = fulgur_pipeline::stack::grid_solution(
        &plan,
        &root,
        &sets,
        wanted.then(|| sky.options(&plan.inspection).map_err(|e| e.to_string())),
        &CANCEL,
    )?;
    if let Some(reason) = &grid.failure {
        println!("Astrometry: no WCS in the master: {reason}");
    }
    fulgur_core::telemetry::timeline("grid plate solution");
    let calibrations: Vec<_> = sets.iter().map(|s| s.calibration.clone()).collect();
    let runs = fulgur_pipeline::stack::plan_masters(&plan, &root, &calibrations, args.masters)?;
    let destinations: Vec<Destinations> = runs
        .iter()
        .map(|run| match &run.label {
            Some(label) => [
                labelled(&output, label),
                labelled(&coverage_output, label),
                labelled(&high_rejection_output, label),
                labelled(&report, label),
            ],
            None => [
                output.clone(),
                coverage_output.clone(),
                high_rejection_output.clone(),
                report.clone(),
            ],
        })
        .collect();
    if runs.len() > 1 {
        for path in destinations.iter().flatten() {
            ensure_new_destination(path)?;
        }
    }
    let context = fulgur_pipeline::stack::MasterContext {
        plan: &plan,
        root: &root,
        canvas: chosen,
        config: &config,
        engine: integration_engine.as_ref(),
        grid: &grid,
        calibration_formula: fulgur_pipeline::stack::calibration_formula(&calibrations),
        cancel: &CANCEL,
    };
    for (run, [output, coverage_output, high_rejection_output, report]) in
        runs.iter().zip(&destinations)
    {
        let published = fulgur_pipeline::stack::publish_master(
            &context,
            run,
            &fulgur_pipeline::stack::MasterTargets {
                master: output,
                coverage: Some(coverage_output),
                high_rejection: Some(high_rejection_output),
            },
            None,
            &|_| {},
        )?;
        let master = MasterReportInput {
            plan: &plan,
            run,
            canvas: chosen,
            canvas_mode: args.canvas.key(),
            config: &config,
            calibration: calibration.as_deref(),
            astrometry_failure: grid.failure.as_deref(),
            files: [output, coverage_output, high_rejection_output],
            wall_seconds: start.elapsed().as_secs_f64(),
        };
        let astrometry = published.astrometry.as_ref().map(|a| a.summary());
        let result = master.report(published);
        fulgur_core::telemetry::timeline("report assembled");
        save_report(Some(report), &result)?;
        fulgur_core::telemetry::timeline("report written");
        print_master(&result, run, &plan, astrometry);
    }
    Ok(())
}

/// Which part of a split run a master holds (ADR-101).
#[derive(Serialize)]
struct MasterGroupReport {
    label: String,
    light_sets: Vec<String>,
    photometric_anchor: String,
}

#[derive(Serialize)]
struct StackReport<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    master_group: Option<MasterGroupReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    calibration: Option<fulgur_pipeline::calibration::CalibrationSummary>,
    /// Star size, shape and sky level of the master (ADR-086).
    #[serde(skip_serializing_if = "Option::is_none")]
    master_quality: Option<fulgur_pipeline::master_quality::MasterQuality>,
    /// Scales and sky offsets applied before rejection (ADR-102).
    normalization: fulgur_pipeline::integrate::NormalizationSummary,
    schema_version: u32,
    command: &'static str,
    output_file: String,
    coverage_file: String,
    high_rejection_file: String,
    map_denominator_frames: usize,
    coverage_definition: &'static str,
    high_rejection_definition: &'static str,
    canvas_mode: String,
    origin_x_px: i32,
    origin_y_px: i32,
    width: usize,
    height: usize,
    config: IntegrationConfig,
    integration: fulgur_pipeline::integrate::IntegrationStats,
    export: RgbExportStats,
    registration: &'a RegistrationReport,
    write_seconds: f64,
    wall_seconds: f64,
    frames_per_second: f64,
    peak_rss_bytes: Option<u64>,
    output_bitpix: i32,
    /// "fits" or "xisf", from the master extension (ADR-142).
    output_format: &'static str,
    /// The master's plate solution and its check (ADR-155).
    #[serde(skip_serializing_if = "Option::is_none")]
    astrometry: Option<serde_json::Value>,
    output_units: &'static str,
    bias_subtracted: bool,
    gps_copied_to_fits: bool,
    intermediate_images_written: usize,
}

/// What the report of one master is built from, besides the master itself.
struct MasterReportInput<'a> {
    plan: &'a RegistrationReport,
    run: &'a MasterRun,
    canvas: &'a Canvas,
    canvas_mode: &'static str,
    config: &'a IntegrationConfig,
    calibration: Option<&'a fulgur_pipeline::calibration::Calibration>,
    astrometry_failure: Option<&'a str>,
    /// Master, coverage map and high-rejection map.
    files: [&'a Path; 3],
    wall_seconds: f64,
}

impl<'a> MasterReportInput<'a> {
    fn report(&self, published: PublishedMaster) -> StackReport<'a> {
        let PublishedMaster {
            maps,
            anchor,
            export,
            astrometry,
            master_quality,
            write_seconds,
            ..
        } = published;
        let (run, plan) = (self.run, self.plan);
        let [output, coverage_output, high_rejection_output] = self.files;
        StackReport {
            master_group: run.label.as_ref().map(|label| MasterGroupReport {
                label: label.clone(),
                light_sets: run
                    .sets
                    .iter()
                    .map(|&s| plan.inspection.light_sets[s].label.clone())
                    .collect(),
                photometric_anchor: run.anchor.clone(),
            }),
            master_quality,
            normalization: fulgur_pipeline::integrate::NormalizationSummary::of(&run.sources),
            schema_version: 2,
            command: "stack",
            output_file: filename(output),
            coverage_file: filename(coverage_output),
            high_rejection_file: filename(high_rejection_output),
            map_denominator_frames: run.input_frames,
            coverage_definition: "RGB surviving sample count / all original input frames, after frame/geometry/low/high rejection; fraction 0..1 = 0..100%; not weight or exposure",
            high_rejection_definition: "RGB upper rejected sample count / all original input frames; fraction 0..1 = 0..100%; excludes lower rejection and invalid edges",
            canvas_mode: self.canvas_mode.to_owned(),
            origin_x_px: self.canvas.origin_x_px,
            origin_y_px: self.canvas.origin_y_px,
            width: maps.image.width,
            height: maps.image.height,
            config: self.config.clone(),
            integration: maps.stats,
            export,
            frames_per_second: plan.frame_count as f64 / self.wall_seconds,
            registration: plan,
            write_seconds,
            wall_seconds: self.wall_seconds,
            peak_rss_bytes: fulgur_core::telemetry::peak_rss_bytes(),
            output_bitpix: -32,
            output_format: OutputFormat::of(output).unwrap_or_default().key(),
            astrometry: match &astrometry {
                Some(a) => Some(serde_json::json!(a)),
                None => self
                    .astrometry_failure
                    .map(|reason| serde_json::json!({ "written": false, "reason": reason })),
            },
            output_units: "linear reference-anchor source ADU / 65535; bounded 0..1",
            bias_subtracted: false,
            calibration: self.calibration.map(|c| c.summary.clone()),
            gps_copied_to_fits: anchor.valid_gps_cards().is_some(),
            intermediate_images_written: 0,
        }
    }
}

/// Per-pixel sky noise: one value for a mono master, R/G/B for colour.
fn noise_text(q: &fulgur_pipeline::master_quality::MasterQuality) -> String {
    let [r, g, b] = q.channel_noise_adu;
    if q.plane == "mono" {
        format!("{g:.2} ADU")
    } else {
        format!("R {r:.2} / G {g:.2} / B {b:.2} ADU")
    }
}

/// One console line with the master's star metrics and the frame medians.
fn master_quality_line(q: &fulgur_pipeline::master_quality::MasterQuality) -> String {
    let pair = |master: Option<f64>, frames: Option<f64>, digits: usize| {
        let show = |v: Option<f64>| v.map_or("-".to_owned(), |v| format!("{v:.digits$}"));
        format!("{} (frames {})", show(master), show(frames))
    };
    format!(
        "Master quality: HFR {} px, FWHM {} px, eccentricity {}, sky {:.1} ADU ({:.5}), noise {}, {} stars measured.",
        pair(q.hfr_px, q.frames_hfr_px, 2),
        pair(q.fwhm_px, q.frames_fwhm_px, 2),
        pair(q.eccentricity, q.frames_eccentricity, 3),
        q.background_adu,
        q.background_normalized,
        noise_text(q),
        q.measured_stars
    )
}

/// The console summary of one master.
fn print_master(
    result: &StackReport<'_>,
    run: &MasterRun,
    plan: &RegistrationReport,
    astrometry: Option<String>,
) {
    if let Some(label) = &run.label {
        println!("Master {label}:");
    }
    println!(
        "Pixel rejection: {} ({}).",
        result.integration.rejection, result.integration.rejection_rule
    );
    println!(
        "Normalization: {}; {}.",
        result.normalization.method,
        result.normalization.describe()
    );
    if let Some(q) = &result.master_quality {
        println!("{}", master_quality_line(q));
    }
    if let Some(astrometry) = astrometry {
        println!("Astrometry: {astrometry}.");
    }
    if let Some(line) = plan.partial_summary() {
        println!("{line}");
    }
    println!(
        "PASS: {} x {} RGB master from {}/{} frames in {:.3} s; peak RAM {} MiB.",
        result.width,
        result.height,
        run.accepted_frames,
        run.input_frames,
        result.wall_seconds,
        peak_mib(result.peak_rss_bytes)
    );
    println!(
        "PixInsight: File > Open; fixed linear range 0..1. Rejected {} low and {} high channel samples.",
        result.integration.rejected_low_channel_samples,
        result.integration.rejected_high_channel_samples
    );
}
