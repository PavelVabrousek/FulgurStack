//! `register`: register the lights and plan output canvases; no image files.
use crate::CANCEL;
use crate::calibration::DarkArgs;
use crate::files::{peak_mib, save_csv, save_report, validate_csv, validate_report};
use crate::lights::{
    DerivedArgs, ReferenceStrategyArg, parse_accuracy, parse_layout, single_light_set,
};
use fulgur_core::{LayoutChoice, Result};
use std::path::{Path, PathBuf};

#[derive(clap::Args)]
pub(crate) struct RegisterArgs {
    #[arg(long, value_enum, default_value = "auto")]
    reference_strategy: ReferenceStrategyArg,
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
    /// Whole-field limits, as for `stack --accuracy`.
    #[arg(long, default_value = "maximum-accuracy", value_parser = parse_accuracy)]
    accuracy: fulgur_pipeline::register::AccuracyLimits,
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
}

pub(crate) fn run(args: RegisterArgs) -> Result<()> {
    let RegisterArgs {
        reference_strategy,
        reference,
        diagnose_excluded,
        geometric_correction,
        partial_frames,
        accuracy,
        input,
        dark,
        derived,
        sensor,
        report,
        csv,
        min_coverage,
    } = args;
    validate_report(Some(&report))?;
    validate_csv(csv.as_deref())?;
    let options = fulgur_pipeline::register::RegistrationOptions {
        reference_strategy: reference_strategy.into(),
        diagnose_excluded,
        geometric_correction,
        partial_frames,
        accuracy,
    };
    // The same registration as `stack`: camera grouping, mirrored
    // cameras and the stranded-set second pass included (ADR-177).
    let set = single_light_set(std::slice::from_ref(&input), sensor, &dark, &derived)?;
    let result = fulgur_pipeline::register::register_sets(
        &[set],
        min_coverage,
        reference.as_deref(),
        options,
        None,
        &CANCEL,
    )?
    .0;
    save_csv(csv.as_deref(), || fulgur_pipeline::register::csv(&result))?;
    save_report(Some(&report), &result)?;
    if let Some(warning) = result.inspection.target_warning() {
        println!("Warning: {warning}.");
    }
    if let Some(warning) = result.auto_reference.as_ref().and_then(|a| a.warning()) {
        println!("Warning: {warning}.");
    }
    println!(
        "PASS: {}/{} scheduled frames registered in {:.3} s; {} skipped, {} accepted, {} rejected; peak RAM {} MiB.",
        result.registration_success_count,
        result.registration_attempted_count,
        result.wall_seconds,
        result.registration_skipped_count,
        result.accepted_count,
        result.rejected_count,
        peak_mib(result.peak_rss_bytes)
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
    Ok(())
}
