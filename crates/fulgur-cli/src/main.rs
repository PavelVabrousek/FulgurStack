use clap::{Parser, Subcommand};
use fulgur_core::fits::{FrameInfo, read_raw};
use fulgur_core::output::{
    OUTPUT_ADU_DIVISOR, RgbExportStats, atomic_write, ensure_new_destination, write_rgb_atomic,
};
use fulgur_core::{DemosaicMethod, Error, Result};
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
    /// CPU workers (default: available parallelism, capped at 16).
    #[arg(long, global = true, value_parser = clap::value_parser!(u16).range(1..=256))]
    threads: Option<u16>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Inspect, register and integrate immutable CFA lights into a linear master.
    Stack {
        /// Explicit reconstruction engine. GPU requires --features gpu; never silently falls back.
        #[arg(long,default_value="cpu",value_parser=["cpu","gpu"])]
        engine: String,
        #[arg(long, required = true)]
        input: Vec<PathBuf>,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        report: PathBuf,
        #[arg(long, default_value = "reference", value_parser = ["reference", "coverage-optimized", "common-intersection"])]
        canvas: String,
        /// Accepted input or external geometry-only FITS (33..300% scale); default: accepted median.
        #[arg(long)]
        reference: Option<PathBuf>,
        /// Also fit excluded frames for diagnostics; never include them in integration.
        #[arg(long)]
        diagnose_excluded: bool,
        /// Optional map destinations; otherwise derived from the master filename.
        #[arg(long)]
        coverage_output: Option<PathBuf>,
        #[arg(long)]
        high_rejection_output: Option<PathBuf>,
        #[arg(long, default_value_t = 0.80)]
        min_coverage: f64,
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
        #[arg(long)]
        reference: Option<PathBuf>,
        /// Also fit excluded frames for diagnostics; never include them in integration.
        #[arg(long)]
        diagnose_excluded: bool,
        #[arg(long)]
        input: PathBuf,
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

fn run(cli: Cli) -> Result<()> {
    let threads = cli.threads.map(usize::from).unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1)
            .min(16)
    });
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build_global()
        .map_err(|_| Error::Invalid("Could not initialize CPU workers."))?;
    ctrlc::set_handler(|| CANCEL.store(true, Ordering::Relaxed))
        .map_err(|_| Error::Invalid("Could not install cancellation handler."))?;
    match cli.command {
        Command::Stack {
            engine,
            input,
            output,
            report,
            canvas,
            reference,
            diagnose_excluded,
            coverage_output,
            high_rejection_output,
            min_coverage,
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
            let integration_engine: Box<dyn IntegrationEngine> = match engine.as_str() {
                "cpu" => Box::new(fulgur_pipeline::integrate::CpuIntegrationEngine),
                #[cfg(feature = "gpu")]
                "gpu" => Box::new(fulgur_pipeline::integrate::GpuIntegrationEngine::new()?),
                _ => {
                    return Err(Error::Invalid(
                        "GPU backend is not compiled; add --features gpu or use --engine cpu.",
                    ));
                }
            };
            let (plan, root) = fulgur_pipeline::register::register_inputs_with_options(
                &input,
                min_coverage,
                reference.as_deref(),
                fulgur_pipeline::register::RegistrationOptions { diagnose_excluded },
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
            let sources = fulgur_pipeline::integrate::sources(&root, &plan)?;
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
                external_geometry_scale: plan
                    .external_reference
                    .as_ref()
                    .map(|r| r.anchor_to_reference.scale_factor),
                accepted_frames: plan.accepted_count,
                total_exposure_seconds: plan.accepted_exposure_seconds,
                origin_x_px,
                origin_y_px,
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
            let wall_seconds = start.elapsed().as_secs_f64();
            let result = StackReport {
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
                gps_copied_to_fits: anchor.valid_gps_cards().is_some(),
                intermediate_images_written: 0,
            };
            save_report(Some(&report), &result)?;
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
            reference,
            diagnose_excluded,
            input,
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
            let result = fulgur_pipeline::register::register_with_options(
                &input,
                min_coverage,
                reference.as_deref(),
                fulgur_pipeline::register::RegistrationOptions { diagnose_excluded },
                &CANCEL,
            )?;
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
            let result = fulgur_pipeline::inspect::inspect(&input, config, &CANCEL)?;
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
        Command::DebayerSingle {
            input,
            output,
            report,
            demosaic,
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
            let frame = read_raw(&input)?;
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
                output_channels: 3,
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
