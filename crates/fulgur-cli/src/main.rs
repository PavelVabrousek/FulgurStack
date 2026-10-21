use clap::{Parser, Subcommand};
use fulgur_core::fits::{FrameInfo, read_raw};
use fulgur_core::output::{
    OUTPUT_ADU_DIVISOR, RgbExportStats, atomic_write, ensure_new_destination, write_rgb_atomic,
};
use fulgur_core::{DemosaicMethod, Error, Result};
use fulgur_pipeline::debayer::{DebayerStats, debayer};
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
