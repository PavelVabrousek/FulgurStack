use clap::{Parser, Subcommand};
use fulgur_core::fits::{FrameInfo, read_raw};
use fulgur_core::output::{
    OUTPUT_ADU_DIVISOR, RgbExportStats, atomic_write, ensure_new_destination, write_rgb_atomic,
};
use fulgur_core::{DemosaicMethod, Error, LayoutChoice, Result};
use fulgur_pipeline::debayer::{DebayerStats, debayer};
use fulgur_pipeline::integrate::IntegrationEngine;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

static CANCEL: AtomicBool = AtomicBool::new(false);

#[derive(Parser)]
#[command(
    name = "fulgurstack",
    version,
    about = "Linear Seestar FITS processing: reconstruction, quality inspection and survey."
)]
struct Cli {
    /// CPU pool threads (default: available parallelism, capped at 24; GPU at 16).
    #[arg(long, global = true, value_parser = clap::value_parser!(u16).range(1..=256))]
    threads: Option<u16>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Inspect, register and integrate immutable CFA lights into a linear master.
    Stack {
        /// Auto uses the median for one night and geometric coverage for multiple nights.
        #[arg(long, default_value="auto", value_parser=["auto","chronological-median"])]
        reference_strategy: String,
        /// Explicit integration engine. GPU requires --features gpu; never silently falls back.
        #[arg(long,default_value="cpu",value_parser=["cpu","gpu"])]
        engine: String,
        /// Stages the GPU engine runs on the device: pixel rejection and
        /// weighted mean (reduction), cosmetics and VNG (reconstruction), or all.
        #[arg(long, default_value = "reduction", value_parser = ["reduction", "reconstruction", "all"])]
        gpu_stages: String,
        #[arg(long, required = true)]
        input: Vec<PathBuf>,
        /// Sensor layout: auto (BAYERPAT header), mono (no demosaicing) or a Bayer pattern.
        #[arg(long, default_value = "auto", value_parser = parse_layout)]
        sensor: LayoutChoice,
        /// Treat each --input as its own light set (own quality limits; filters,
        /// gain and exposure may differ between sets).
        #[arg(long)]
        input_sets: bool,
        #[command(flatten)]
        dark: DarkArgs,
        #[command(flatten)]
        derived: DerivedArgs,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        report: PathBuf,
        #[arg(long, default_value = "reference", value_parser = ["reference", "coverage-optimized", "common-intersection"])]
        canvas: String,
        /// Accepted input or external geometry-only FITS (30..333% scale); overrides reference strategy.
        #[arg(long)]
        reference: Option<PathBuf>,
        /// Also fit excluded frames for diagnostics; never include them in integration.
        #[arg(long)]
        diagnose_excluded: bool,
        /// Correct optical distortion only when held-out stars confirm improvement.
        #[arg(long)]
        geometric_correction: bool,
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
        /// Default: auto-select 128, 256 or 512 square pixels using available RAM and a system reserve.
        #[arg(long, value_parser = ["auto", "128", "256", "512"], conflicts_with_all = ["tile_width", "tile_height"])]
        tile_size: Option<String>,
        /// Manual width; omitted companion height defaults to 512. Fails if memory cannot fit.
        #[arg(long, value_parser = clap::value_parser!(u16).range(1..=1024))]
        tile_width: Option<u16>,
        /// Manual height; omitted companion width defaults to 512. Fails if memory cannot fit.
        #[arg(long, value_parser = clap::value_parser!(u16).range(1..=512))]
        tile_height: Option<u16>,
    },
    /// Register CFA catalogs to an automatic anchor and plan output canvases; no image files.
    Register {
        #[arg(long, default_value="auto", value_parser=["auto","chronological-median"])]
        reference_strategy: String,
        #[arg(long)]
        reference: Option<PathBuf>,
        /// Also fit excluded frames for diagnostics; never include them in integration.
        #[arg(long)]
        diagnose_excluded: bool,
        /// Correct optical distortion only when held-out stars confirm improvement.
        #[arg(long)]
        geometric_correction: bool,
        #[arg(long)]
        input: PathBuf,
        #[command(flatten)]
        dark: DarkArgs,
        #[command(flatten)]
        derived: DerivedArgs,
        /// Sensor layout: auto (BAYERPAT header), mono or a Bayer pattern.
        #[arg(long, default_value = "auto", value_parser = parse_layout)]
        sensor: LayoutChoice,
        #[arg(long)]
        report: PathBuf,
        #[arg(long)]
        csv: Option<PathBuf>,
        #[arg(long, default_value_t = 0.80)]
        min_coverage: f64,
    },
    /// Inspect source CFA quality and produce a private JSON catalog and review table.
    Inspect {
        #[arg(long)]
        input: PathBuf,
        #[command(flatten)]
        dark: DarkArgs,
        #[command(flatten)]
        derived: DerivedArgs,

        /// Sensor layout: auto (BAYERPAT header), mono or a Bayer pattern.
        #[arg(long, default_value = "auto", value_parser = parse_layout)]
        sensor: LayoutChoice,
        #[arg(long)]
        report: PathBuf,
        /// Optional new semicolon-delimited UTF-8 CSV for manual review.
        #[arg(long)]
        csv: Option<PathBuf>,
        #[arg(long, default_value_t = 2.5)]
        hfr_sigma: f64,
        #[arg(long, default_value_t = 0.45)]
        min_star_fraction: f64,
        #[arg(long, default_value_t = 0.60)]
        max_eccentricity: f64,
    },
    /// Combine dark frames into a master dark FITS (32-bit float ADU) for later runs.
    MasterDark {
        /// Dark frames (files or folders).
        #[arg(long, required = true)]
        dark: Vec<PathBuf>,
        #[arg(long, default_value = "sigma-kappa-2", value_parser = COMBINE_METHODS)]
        combine: String,
        #[arg(long)]
        output: PathBuf,
    },
    /// Reconstruct one CFA FITS into a linear float RGB FITS without intermediate images.
    DebayerSingle {
        #[arg(long)]
        input: PathBuf,
        /// New output filename. Existing files are never overwritten.
        #[arg(long)]
        output: PathBuf,
        /// Optional new JSON report; GPS values are excluded.
        #[arg(long)]
        report: Option<PathBuf>,
        /// Reconstruction method: vng (default) or mhc (diagnostic baseline).
        #[arg(long, default_value = "vng")]
        demosaic: DemosaicMethod,
        /// Sensor layout: auto (BAYERPAT header), mono (no demosaicing) or a Bayer pattern.
        #[arg(long, default_value = "auto", value_parser = parse_layout)]
        sensor: LayoutChoice,
    },
    /// Survey FITS headers and measure full-file read throughput without writing images.
    Survey {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        report: Option<PathBuf>,
    },
}

#[derive(Serialize)]
struct SingleReport {
    schema_version: u32,
    command: &'static str,
    input_file: String,
    output_file: String,
    frame: FrameInfo,
    read_seconds: f64,
    #[serde(flatten)]
    processing: DebayerStats,
    write_seconds: f64,
    wall_seconds: f64,
    peak_rss_bytes: Option<u64>,
    cpu_threads: usize,
    output_bitpix: i32,
    output_channels: usize,
    output_units: &'static str,
    output_adu_divisor: f32,
    output_offset: f32,
    #[serde(flatten)]
    export: RgbExportStats,
    output_range_policy: &'static str,
    pixinsight_open_method: &'static str,
    bias_subtracted: bool,
    gps_copied_to_fits: bool,
    intermediate_images_written: usize,
}

#[derive(Serialize)]
struct StackReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    calibration: Option<fulgur_pipeline::calibration::CalibrationSummary>,
    /// Star size, shape and sky level of the master (ADR-086).
    #[serde(skip_serializing_if = "Option::is_none")]
    master_quality: Option<fulgur_pipeline::master_quality::MasterQuality>,
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
    config: fulgur_pipeline::integrate::IntegrationConfig,
    integration: fulgur_pipeline::integrate::IntegrationStats,
    export: RgbExportStats,
    registration: fulgur_pipeline::register::RegistrationReport,
    write_seconds: f64,
    wall_seconds: f64,
    frames_per_second: f64,
    peak_rss_bytes: Option<u64>,
    output_bitpix: i32,
    output_units: &'static str,
    bias_subtracted: bool,
    gps_copied_to_fits: bool,
    intermediate_images_written: usize,
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

fn parse_rejection(
    value: &str,
) -> std::result::Result<fulgur_pipeline::integrate::Rejection, String> {
    fulgur_pipeline::integrate::Rejection::parse(value).map_err(|e| e.to_string())
}

fn parse_layout(value: &str) -> std::result::Result<LayoutChoice, String> {
    LayoutChoice::parse(value).map_err(|e| e.to_string())
}

fn validate_report(report: Option<&Path>) -> Result<()> {
    if let Some(path) = report {
        if !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        {
            return Err(Error::Invalid(
                "Report filename must have a .json extension.",
            ));
        }
        ensure_new_destination(path)?;
    }
    Ok(())
}

fn save_report(report: Option<&Path>, value: &impl Serialize) -> Result<()> {
    if let Some(path) = report {
        let bytes = serde_json::to_vec_pretty(value)
            .map_err(|_| Error::Invalid("JSON serialization failed."))?;
        atomic_write(path, &CANCEL, |writer| {
            writer.write_all(&bytes)?;
            writer.write_all(b"\n")?;
            Ok(())
        })?;
    }
    Ok(())
}

fn filename(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn companion(path: &Path, suffix: &str) -> PathBuf {
    let mut stem = path.file_stem().unwrap_or_default().to_os_string();
    stem.push(suffix);
    path.with_file_name(stem)
}
fn destination_identity(path: &Path) -> Result<String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for c in absolute.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(c.as_os_str()),
        }
    }
    let mut base = normalized.clone();
    let mut suffix = vec![];
    while !base.exists() {
        suffix.push(
            base.file_name()
                .ok_or(Error::Invalid("Invalid destination path."))?
                .to_os_string(),
        );
        base.pop();
    }
    let mut resolved = base.canonicalize()?;
    for c in suffix.into_iter().rev() {
        resolved.push(c);
    }
    let text = resolved.to_string_lossy().into_owned();
    #[cfg(windows)]
    {
        Ok(text.to_lowercase())
    }
    #[cfg(not(windows))]
    {
        Ok(text)
    }
}

const COMBINE_METHODS: [&str; 3] = ["average", "median", "sigma-kappa-2"];

/// Values missing from the FITS headers (ADR-084), shared by stack,
/// register and inspect.
#[derive(clap::Args, Debug, Clone)]
struct DerivedArgs {
    /// Use the values reconstructed for missing header cards (exposure from
    /// EXPOSURE or the file name, colour layout from other cards, the file
    /// name or the camera catalogue) after checking the list a run prints.
    #[arg(long)]
    accept_derived: bool,
    /// Exposure in seconds for every light, replacing header or derived values.
    #[arg(long, value_parser = parse_seconds)]
    exposure: Option<f64>,
}

impl DerivedArgs {
    fn is_default(&self) -> bool {
        !self.accept_derived && self.exposure.is_none()
    }

    fn apply(&self, set: &mut fulgur_pipeline::inspect::InputSet) {
        set.exposure = self.exposure;
        set.accept_derived = self.accept_derived;
    }
}

fn parse_seconds(value: &str) -> std::result::Result<f64, String> {
    value
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or_else(|| "Exposure must be a positive number of seconds.".to_owned())
}

/// Print values reconstructed for missing header cards and stop unless the
/// owner confirmed them or corrected them (ADR-084).
fn check_derived(sets: &[fulgur_pipeline::inspect::InputSet], args: &DerivedArgs) -> Result<()> {
    let groups = fulgur_pipeline::inspect::derived_metadata(sets)?;
    if groups.is_empty() {
        return Ok(());
    }
    println!("Values missing from the FITS headers were derived:");
    for g in &groups {
        let set = if sets.len() > 1 {
            format!("[{}] ", sets[g.light_set].label)
        } else {
            String::new()
        };
        println!(
            "  {set}{}: {} - {} ({} file{}){}",
            g.field,
            g.value,
            g.source,
            g.files,
            if g.files == 1 { "" } else { "s" },
            if g.needs_confirmation {
                ""
            } else {
                " [label only]"
            }
        );
    }
    if groups.iter().any(|g| g.value == "missing") {
        return Err(Error::Invalid(
            "Some lights have no exposure time in the header or file name; enter it with --exposure <seconds>.",
        ));
    }
    if groups.iter().any(|g| g.needs_confirmation) {
        if !args.accept_derived {
            return Err(Error::Invalid(
                "Derived values need your confirmation: check them, then re-run with --accept-derived, or correct them with --sensor <mono|rggb|grbg|gbrg|bggr> and --exposure <seconds>.",
            ));
        }
        println!("Derived values confirmed with --accept-derived.");
    }
    Ok(())
}

/// Dark calibration options shared by stack, register and inspect.
#[derive(clap::Args, Debug, Clone)]
struct DarkArgs {
    /// Bias frames (files or folders) combined into an in-RAM master bias.
    #[arg(long)]
    bias: Vec<PathBuf>,
    /// Existing master bias FITS instead of bias frames.
    #[arg(long, conflicts_with = "bias")]
    bias_master: Option<PathBuf>,
    /// Dark frames (files or folders): combined into an in-RAM master dark
    /// and subtracted from every light before measurement and stacking.
    #[arg(long)]
    dark: Vec<PathBuf>,
    /// Existing master dark FITS (BITPIX 16 or -32) instead of dark frames.
    #[arg(long, conflicts_with = "dark")]
    dark_master: Option<PathBuf>,
    /// Flat frames (files or folders) combined into an in-RAM master flat.
    #[arg(long)]
    flat: Vec<PathBuf>,
    /// Existing master flat FITS instead of flat frames.
    #[arg(long, conflicts_with = "flat")]
    flat_master: Option<PathBuf>,
    /// Dark-flat frames (the flats' offset; the master bias is used otherwise).
    #[arg(long)]
    dark_flat: Vec<PathBuf>,
    /// Existing master dark flat FITS instead of dark-flat frames.
    #[arg(long, conflicts_with = "dark_flat")]
    dark_flat_master: Option<PathBuf>,
    /// How dark frames are combined per pixel.
    #[arg(long, default_value = "sigma-kappa-2", value_parser = COMBINE_METHODS)]
    dark_combine: String,
    /// How bias, flat and dark-flat frames are combined per pixel.
    #[arg(long, default_value = "sigma-kappa-2", value_parser = COMBINE_METHODS)]
    calibration_combine: String,
}

fn combine_method(text: &str) -> fulgur_pipeline::calibration::CombineMethod {
    fulgur_pipeline::calibration::CombineMethod::parse(text).expect("clap validates the method")
}

fn report_master(m: &fulgur_pipeline::calibration::MasterDark) {
    let s = &m.summary;
    if s.loaded_from.is_some() {
        println!(
            "Master {} file: {:.3} s exposure, median {:.0} ADU{}, loaded in {:.2} s.",
            s.kind.title(),
            s.exposure_seconds,
            s.median_adu,
            if s.normalized_input_scaled {
                " (normalized input scaled by 65535)"
            } else {
                ""
            },
            s.build_seconds
        );
    } else {
        println!(
            "Master {}: {} frames, {}, {:.3} s exposure, median {:.0} ADU, built in RAM in {:.2} s (not written to disk).",
            s.kind.title(),
            s.frames,
            s.combine.map_or("", |m| m.label()),
            s.exposure_seconds,
            s.median_adu,
            s.build_seconds
        );
    }
}

/// Build or load one master from frames or a file; `None` when not given.
fn master(
    frames: &[PathBuf],
    file: Option<&Path>,
    kind: fulgur_pipeline::calibration::MasterKind,
    method: &str,
    offset: Option<&fulgur_pipeline::calibration::MasterDark>,
) -> Result<Option<std::sync::Arc<fulgur_pipeline::calibration::MasterDark>>> {
    use fulgur_pipeline::calibration as cal;
    let master = if let Some(path) = file {
        cal::load_master(path, kind, &CANCEL)?
    } else if frames.is_empty() {
        return Ok(None);
    } else {
        let files = cal::collect_frames(frames)?;
        if files.is_empty() {
            return Err(Error::Invalid("No calibration FITS files found."));
        }
        if kind == cal::MasterKind::Flat {
            cal::build_flat_master(&files, offset, combine_method(method), &CANCEL, &|_| {})?
        } else {
            cal::build_master(&files, kind, combine_method(method), &CANCEL)?
        }
    };
    report_master(&master);
    Ok(Some(std::sync::Arc::new(master)))
}

/// Build or load the calibration masters once and report them without
/// printing paths.
fn master_dark(
    args: &DarkArgs,
) -> Result<Option<std::sync::Arc<fulgur_pipeline::calibration::Calibration>>> {
    use fulgur_pipeline::calibration::MasterKind;
    let combine = args.calibration_combine.as_str();
    let bias = master(
        &args.bias,
        args.bias_master.as_deref(),
        MasterKind::Bias,
        combine,
        None,
    )?;
    let dark = master(
        &args.dark,
        args.dark_master.as_deref(),
        MasterKind::Dark,
        &args.dark_combine,
        None,
    )?;
    let dark_flat = master(
        &args.dark_flat,
        args.dark_flat_master.as_deref(),
        MasterKind::DarkFlat,
        combine,
        None,
    )?;
    let offset = dark_flat.clone().or_else(|| bias.clone());
    let flat = master(
        &args.flat,
        args.flat_master.as_deref(),
        MasterKind::Flat,
        combine,
        offset.as_deref(),
    )?;
    if bias.is_none() && dark.is_none() && flat.is_none() {
        return Ok(None);
    }
    let calibration = fulgur_pipeline::calibration::Calibration::new(bias, dark, flat, offset)?;
    println!("Calibration: {}.", calibration.summary.formula);
    for warning in &calibration.summary.warnings {
        println!("Warning: {warning}.");
    }
    Ok(Some(std::sync::Arc::new(calibration)))
}

fn run(cli: Cli) -> Result<()> {
    // GPU reconstruction dispatches at most 16 ROIs; CPU reconstruction uses 24.
    let default_limit = if matches!(&cli.command, Command::Stack {engine, gpu_stages, ..} if engine == "gpu" && gpu_stages != "reduction")
    {
        16
    } else {
        24
    };
    let threads = cli.threads.map(usize::from).unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1)
            .min(default_limit)
    });
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build_global()
        .map_err(|_| Error::Invalid("Could not initialize CPU workers."))?;
    ctrlc::set_handler(|| CANCEL.store(true, Ordering::Relaxed))
        .map_err(|_| Error::Invalid("Could not install cancellation handler."))?;
    match cli.command {
        Command::Stack {
            reference_strategy,
            engine,
            gpu_stages,
            input,
            sensor,
            input_sets,
            dark,
            derived,
            output,
            report,
            canvas,
            reference,
            diagnose_excluded,
            geometric_correction,
            coverage_output,
            high_rejection_output,
            min_coverage,
            rejection,
            sigma_low,
            sigma_high,
            memory_budget_mib,
            fits_cache_mib,
            tile_size,
            tile_width,
            tile_height,
        } => {
            validate_report(Some(&report))?;
            if !output.extension().is_some_and(|e| {
                ["fit", "fits", "fts"]
                    .iter()
                    .any(|s| e.eq_ignore_ascii_case(s))
            }) {
                return Err(Error::Invalid(
                    "Master output must end in .fit, .fits or .fts.",
                ));
            }
            ensure_new_destination(&output)?;
            let coverage_output =
                coverage_output.unwrap_or_else(|| companion(&output, "_coverage.fits"));
            let high_rejection_output =
                high_rejection_output.unwrap_or_else(|| companion(&output, "_high_rejection.fits"));
            for path in [&coverage_output, &high_rejection_output] {
                if !path.extension().is_some_and(|e| {
                    ["fit", "fits", "fts"]
                        .iter()
                        .any(|s| e.eq_ignore_ascii_case(s))
                }) {
                    return Err(Error::Invalid("Diagnostic maps require FITS destinations."));
                }
                ensure_new_destination(path)?;
            }
            let mut destinations = std::collections::BTreeSet::new();
            for path in [&output, &report, &coverage_output, &high_rejection_output] {
                if !destinations.insert(destination_identity(path)?) {
                    return Err(Error::Invalid(
                        "Master, maps and report require distinct destinations.",
                    ));
                }
            }
            use fulgur_pipeline::integrate::TileMode;
            let manual_size = tile_size
                .as_deref()
                .filter(|v| *v != "auto")
                .map(|v| v.parse::<usize>().expect("clap validates tile size"));
            let tile_mode =
                if manual_size.is_some() || tile_width.is_some() || tile_height.is_some() {
                    TileMode::Manual
                } else {
                    TileMode::Auto
                };
            let config = fulgur_pipeline::integrate::IntegrationConfig {
                rejection,
                sigma_low,
                sigma_high,
                memory_budget_mib,
                fits_cache_mib: usize::from(fits_cache_mib),
                tile_mode,
                tile_width: manual_size.unwrap_or(usize::from(tile_width.unwrap_or(512))),
                tile_height: manual_size.unwrap_or(usize::from(tile_height.unwrap_or(512))),
            };
            config.validate()?;
            let start = Instant::now();
            #[cfg(not(feature = "gpu"))]
            let _ = &gpu_stages;
            let integration_engine: Box<dyn IntegrationEngine> = match engine.as_str() {
                "cpu" => Box::new(fulgur_pipeline::integrate::CpuIntegrationEngine),
                #[cfg(feature = "gpu")]
                "gpu" => Box::new(
                    fulgur_pipeline::integrate::GpuIntegrationEngine::with_stages(
                        fulgur_pipeline::integrate::GpuStages::parse(&gpu_stages)
                            .ok_or(Error::Invalid("Unknown GPU stages."))?,
                    )?,
                ),
                _ => {
                    return Err(Error::Invalid(
                        "GPU backend is not compiled; add --features gpu or use --engine cpu.",
                    ));
                }
            };
            let master_dark = master_dark(&dark)?;
            let sets: Vec<_> = if input_sets {
                input
                    .iter()
                    .map(|path| fulgur_pipeline::inspect::InputSet {
                        label: filename(path),
                        inputs: vec![path.clone()],
                        layout: sensor,
                        exposure: derived.exposure,
                        accept_derived: derived.accept_derived,
                        calibration: master_dark.clone(),
                    })
                    .collect()
            } else {
                let mut set = fulgur_pipeline::inspect::single_set(&input, sensor);
                set.calibration = master_dark.clone();
                derived.apply(&mut set);
                vec![set]
            };
            check_derived(&sets, &derived)?;
            let (plan, root) = fulgur_pipeline::register::register_sets_with_options(
                &sets,
                min_coverage,
                reference.as_deref(),
                fulgur_pipeline::register::RegistrationOptions {
                    reference_strategy: if reference_strategy == "auto" {
                        fulgur_pipeline::register::ReferenceStrategy::Auto
                    } else {
                        fulgur_pipeline::register::ReferenceStrategy::ChronologicalMedian
                    },
                    diagnose_excluded,
                    geometric_correction,
                },
                &CANCEL,
            )?;
            let anchor = fulgur_core::fits::read_header(&root.join(&plan.reference_anchor))?;
            let reference_canvas = fulgur_pipeline::register::Canvas {
                origin_x_px: 0,
                origin_y_px: 0,
                width: plan.reference_width,
                height: plan.reference_height,
                minimum_effective_weight_fraction: 0.,
                coordinate_rule: "exact reference FITS dimensions and sensor pixel grid; no automatic crop",
            };
            let chosen = match canvas.as_str() {
                "reference" => Some(&reference_canvas),
                "common-intersection" => plan.common_intersection.as_ref(),
                _ => plan.coverage_optimized.as_ref(),
            }
            .ok_or(Error::Invalid(
                "No safe canvas for the selected coverage policy.",
            ))?;
            let origin_x_px = chosen.origin_x_px;
            let origin_y_px = chosen.origin_y_px;
            let calibrations: Vec<_> = sets.iter().map(|s| s.calibration.clone()).collect();
            let sources =
                fulgur_pipeline::integrate::calibrated_sources(&root, &plan, &calibrations)?;
            let calibrated_frames = sources.iter().filter(|s| s.calibration.is_some()).count();
            let maps = integration_engine.integrate_with_maps(
                &sources,
                chosen,
                &config,
                plan.frame_count,
                &CANCEL,
            )?;
            let image = maps.image;
            let integration = maps.stats;
            let metadata = fulgur_core::output::MasterMetadata {
                geometric_correction_frames: plan
                    .frames
                    .iter()
                    .filter(|f| {
                        f.accepted
                            && f.geometric_correction
                                .as_ref()
                                .is_some_and(|d| d.model.is_some())
                    })
                    .count(),
                external_geometry_scale: plan
                    .external_reference
                    .as_ref()
                    .map(|r| r.anchor_to_reference.scale_factor),
                accepted_frames: plan.accepted_count,
                total_exposure_seconds: plan.accepted_exposure_seconds,
                origin_x_px,
                origin_y_px,
                monochrome_frames: plan.accepted_monochrome_frames(),
                calibrated_frames,
                calibration_formula: master_dark.as_ref().map(|c| c.summary.formula.clone()),
            };
            let write_start = Instant::now();
            let export = fulgur_core::output::write_master_atomic(
                &output, &image, &anchor, &metadata, &CANCEL,
            )?;
            fulgur_core::output::write_fraction_map_atomic(
                &coverage_output,
                &maps.coverage,
                "COVERAGE",
                plan.frame_count,
                &CANCEL,
            )?;
            fulgur_core::output::write_fraction_map_atomic(
                &high_rejection_output,
                &maps.high_rejection,
                "HIGHREJ",
                plan.frame_count,
                &CANCEL,
            )?;
            let write_seconds = write_start.elapsed().as_secs_f64();
            let master_quality = fulgur_pipeline::master_quality::measure(
                &image,
                &maps.coverage,
                metadata.monochrome_frames == metadata.accepted_frames,
                &plan,
                &CANCEL,
            )
            .ok();
            let wall_seconds = start.elapsed().as_secs_f64();
            let result = StackReport {
                master_quality,
                schema_version: 2,
                command: "stack",
                output_file: filename(&output),
                coverage_file: filename(&coverage_output),
                high_rejection_file: filename(&high_rejection_output),
                map_denominator_frames: plan.frame_count,
                coverage_definition: "RGB surviving sample count / all original input frames, after frame/geometry/low/high rejection; fraction 0..1 = 0..100%; not weight or exposure",
                high_rejection_definition: "RGB upper rejected sample count / all original input frames; fraction 0..1 = 0..100%; excludes lower rejection and invalid edges",
                canvas_mode: canvas,
                origin_x_px,
                origin_y_px,
                width: image.width,
                height: image.height,
                config,
                integration,
                export,
                frames_per_second: plan.frame_count as f64 / wall_seconds,
                registration: plan,
                write_seconds,
                wall_seconds,
                peak_rss_bytes: fulgur_core::telemetry::peak_rss_bytes(),
                output_bitpix: -32,
                output_units: "linear reference-anchor source ADU / 65535; bounded 0..1",
                bias_subtracted: false,
                calibration: master_dark.as_ref().map(|c| c.summary.clone()),
                gps_copied_to_fits: anchor.valid_gps_cards().is_some(),
                intermediate_images_written: 0,
            };
            save_report(Some(&report), &result)?;
            println!(
                "Pixel rejection: {} ({}).",
                result.integration.rejection, result.integration.rejection_rule
            );
            if let Some(q) = &result.master_quality {
                println!("{}", master_quality_line(q));
            }
            println!(
                "PASS: {} x {} RGB master from {}/{} frames in {:.3} s; peak RAM {} MiB.",
                result.width,
                result.height,
                result.registration.accepted_count,
                result.registration.frame_count,
                result.wall_seconds,
                result
                    .peak_rss_bytes
                    .map(|b| format!("{:.1}", b as f64 / 1048576.))
                    .unwrap_or_else(|| "unavailable".into())
            );
            println!(
                "PixInsight: File > Open; fixed linear range 0..1. Rejected {} low and {} high channel samples.",
                result.integration.rejected_low_channel_samples,
                result.integration.rejected_high_channel_samples
            );
        }
        Command::Register {
            reference_strategy,
            reference,
            diagnose_excluded,
            geometric_correction,
            input,
            dark,
            derived,
            sensor,
            report,
            csv,
            min_coverage,
        } => {
            validate_report(Some(&report))?;
            if let Some(path) = &csv {
                if !path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("csv"))
                {
                    return Err(Error::Invalid("Review table must have a .csv extension."));
                }
                ensure_new_destination(path)?;
            }
            let options = fulgur_pipeline::register::RegistrationOptions {
                reference_strategy: if reference_strategy == "auto" {
                    fulgur_pipeline::register::ReferenceStrategy::Auto
                } else {
                    fulgur_pipeline::register::ReferenceStrategy::ChronologicalMedian
                },
                diagnose_excluded,
                geometric_correction,
            };
            // The header path keeps its established relative source names.
            let master_dark = master_dark(&dark)?;
            let mut set =
                fulgur_pipeline::inspect::single_set(std::slice::from_ref(&input), sensor);
            set.calibration = master_dark;
            derived.apply(&mut set);
            check_derived(std::slice::from_ref(&set), &derived)?;
            let result = if sensor == LayoutChoice::Auto
                && set.calibration.is_none()
                && derived.is_default()
            {
                fulgur_pipeline::register::register_with_options(
                    &input,
                    min_coverage,
                    reference.as_deref(),
                    options,
                    &CANCEL,
                )?
            } else {
                fulgur_pipeline::register::register_sets_with_options(
                    &[set],
                    min_coverage,
                    reference.as_deref(),
                    options,
                    &CANCEL,
                )?
                .0
            };
            if let Some(path) = csv {
                let table = fulgur_pipeline::register::csv(&result);
                atomic_write(&path, &CANCEL, |writer| {
                    writer.write_all(table.as_bytes())?;
                    Ok(())
                })?;
            }
            save_report(Some(&report), &result)?;
            println!(
                "PASS: {}/{} scheduled frames registered in {:.3} s; {} skipped, {} accepted, {} rejected; peak RAM {} MiB.",
                result.registration_success_count,
                result.registration_attempted_count,
                result.wall_seconds,
                result.registration_skipped_count,
                result.accepted_count,
                result.rejected_count,
                result
                    .peak_rss_bytes
                    .map(|bytes| format!("{:.1}", bytes as f64 / 1048576.))
                    .unwrap_or_else(|| "unavailable".to_owned())
            );
            if let Some(external) = &result.external_reference {
                println!(
                    "Geometry reference: {}; fitted anchor scale {:.4}%; native photometric anchor: {}",
                    Path::new(&external.path)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy(),
                    external.anchor_to_reference.scale_factor * 100.,
                    result.reference_anchor
                );
            } else {
                println!("Reference anchor: {}", result.reference_anchor);
            }
        }
        Command::Inspect {
            input,
            dark,
            derived,
            sensor,
            report,
            csv,
            hfr_sigma,
            min_star_fraction,
            max_eccentricity,
        } => {
            validate_report(Some(&report))?;
            if let Some(path) = &csv {
                if !path
                    .extension()
                    .is_some_and(|s| s.eq_ignore_ascii_case("csv"))
                {
                    return Err(Error::Invalid("Review table must have a .csv extension."));
                }
                ensure_new_destination(path)?;
            }
            let config = fulgur_pipeline::inspect::InspectConfig {
                hfr_sigma,
                min_star_fraction,
                max_eccentricity,
                ..Default::default()
            };
            let master_dark = master_dark(&dark)?;
            let mut set =
                fulgur_pipeline::inspect::single_set(std::slice::from_ref(&input), sensor);
            set.calibration = master_dark;
            derived.apply(&mut set);
            check_derived(std::slice::from_ref(&set), &derived)?;
            let result = if sensor == LayoutChoice::Auto
                && set.calibration.is_none()
                && derived.is_default()
            {
                fulgur_pipeline::inspect::inspect(&input, config, &CANCEL)?
            } else {
                fulgur_pipeline::inspect::inspect_sets(&[set], config, &CANCEL, None)?.0
            };
            if let Some(path) = csv {
                let table = fulgur_pipeline::inspect::csv(&result);
                atomic_write(&path, &CANCEL, |writer| {
                    writer.write_all(table.as_bytes())?;
                    Ok(())
                })?;
            }
            save_report(Some(&report), &result)?;
            println!(
                "PASS: {} frames inspected in {:.3} s; {} accepted, {} rejected; peak RAM {} MiB.",
                result.frame_count,
                result.wall_seconds,
                result.accepted_count,
                result.rejected_count,
                result
                    .peak_rss_bytes
                    .map(|b| format!("{:.1}", b as f64 / 1048576.0))
                    .unwrap_or_else(|| "unavailable".to_owned())
            );
            println!(
                "Weights are provisional. M3 must finalize matched-star transparency and photometry."
            );
        }
        Command::MasterDark {
            dark,
            combine,
            output,
        } => {
            if !output.extension().is_some_and(|e| {
                ["fit", "fits", "fts"]
                    .iter()
                    .any(|s| e.eq_ignore_ascii_case(s))
            }) {
                return Err(Error::Invalid(
                    "Master dark output must end in .fit, .fits or .fts.",
                ));
            }
            ensure_new_destination(&output)?;
            let master = fulgur_pipeline::calibration::build_master_dark_from(
                &dark,
                combine_method(&combine),
                &CANCEL,
            )?;
            fulgur_pipeline::calibration::save_master_dark(&master, &output, &CANCEL)?;
            let s = &master.summary;
            for warning in &s.warnings {
                println!("Warning: {warning}.");
            }
            println!(
                "PASS: master dark from {} frames ({}), {:.1} s exposure, pedestal {:.0} ADU, {} clipped samples; saved as {}.",
                s.frames,
                s.combine.map_or("", |m| m.label()),
                s.exposure_seconds,
                s.pedestal_adu,
                s.clipped_samples,
                filename(&output)
            );
        }
        Command::DebayerSingle {
            input,
            output,
            report,
            demosaic,
            sensor,
        } => {
            if !output.extension().is_some_and(|extension| {
                ["fit", "fits", "fts"]
                    .iter()
                    .any(|expected| extension.eq_ignore_ascii_case(expected))
            }) {
                return Err(Error::Invalid(
                    "RGB output filename must end in .fit, .fits, or .fts.",
                ));
            }
            ensure_new_destination(&output)?;
            validate_report(report.as_deref())?;
            let start = Instant::now();
            let mut frame = read_raw(&input)?;
            frame.header.info.apply_layout(sensor);
            let read_seconds = start.elapsed().as_secs_f64();
            let (rgb, processing) = debayer(&frame, &CANCEL, demosaic)?;
            let write_start = Instant::now();
            let export = write_rgb_atomic(&output, &rgb, &frame.header, &CANCEL)?;
            let write_seconds = write_start.elapsed().as_secs_f64();
            let result = SingleReport {
                schema_version: 3,
                command: "debayer-single",
                input_file: filename(&input),
                output_file: filename(&output),
                frame: frame.header.info.clone(),
                read_seconds,
                processing,
                write_seconds,
                wall_seconds: start.elapsed().as_secs_f64(),
                peak_rss_bytes: fulgur_core::telemetry::peak_rss_bytes(),
                cpu_threads: threads,
                output_bitpix: -32,
                output_channels: if rgb.demosaic_method == DemosaicMethod::None {
                    1
                } else {
                    3
                },
                output_units: "linear source ADU / 65535",
                output_adu_divisor: OUTPUT_ADU_DIVISOR,
                output_offset: 0.0,
                export,
                output_range_policy: "sensor saturation to 0..1",
                pixinsight_open_method: "File > Open",
                bias_subtracted: false,
                gps_copied_to_fits: frame.header.valid_gps_cards().is_some(),
                intermediate_images_written: 0,
            };
            save_report(report.as_deref(), &result)?;
            println!(
                "PASS: RGB FITS exported in {:.3} s; {} isolated hot pixels corrected; peak RAM {} MiB.",
                result.wall_seconds,
                result.processing.corrected_pixels,
                result
                    .peak_rss_bytes
                    .map(|bytes| format!("{:.1}", bytes as f64 / 1048576.0))
                    .unwrap_or_else(|| "unavailable".to_owned())
            );
            println!(
                "PixInsight: File > Open; RGB range 0..1 on the fixed reference scale. {} reconstructed channel samples limited to sensor range.",
                result.export.samples_clipped
            );
        }
        Command::Survey { input, report } => {
            validate_report(report.as_deref())?;
            let result = fulgur_pipeline::survey::survey(&input, &CANCEL)?;
            save_report(report.as_deref(), &result)?;
            println!(
                "PASS: {} FITS frames, {:.3} s, {:.1} MiB/s effective read throughput (OS cache uncontrolled).",
                result.frame_count, result.wall_seconds, result.read_mib_per_second
            );
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("FAIL: {error}");
            if matches!(error, Error::Cancelled) {
                ExitCode::from(130)
            } else {
                ExitCode::FAILURE
            }
        }
    }
}
