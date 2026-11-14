//! `debayer-single`: reconstruct one CFA FITS into a linear float RGB FITS.
use crate::CANCEL;
use crate::files::{filename, peak_mib, save_report, validate_report};
use crate::lights::parse_layout;
use fulgur_core::fits::{FrameInfo, read_raw};
use fulgur_core::output::{
    OUTPUT_ADU_DIVISOR, RgbExportStats, ensure_new_destination, write_rgb_atomic,
};
use fulgur_core::{DemosaicMethod, Error, LayoutChoice, Result};
use fulgur_pipeline::debayer::{DebayerStats, debayer};
use serde::Serialize;
use std::path::PathBuf;
use std::time::Instant;

#[derive(clap::Args)]
pub(crate) struct DebayerSingleArgs {
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

pub(crate) fn run(args: DebayerSingleArgs, threads: usize) -> Result<()> {
    let DebayerSingleArgs {
        input,
        output,
        report,
        demosaic,
        sensor,
    } = args;
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
        peak_mib(result.peak_rss_bytes)
    );
    println!(
        "PixInsight: File > Open; RGB range 0..1 on the fixed reference scale. {} reconstructed channel samples limited to sensor range.",
        result.export.samples_clipped
    );
    Ok(())
}
