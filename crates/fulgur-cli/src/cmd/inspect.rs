//! `inspect`: measure source quality into a private JSON catalog and review table.
use crate::CANCEL;
use crate::calibration::DarkArgs;
use crate::files::{peak_mib, save_csv, save_report, validate_csv, validate_report};
use crate::lights::{DerivedArgs, parse_layout, single_light_set};
use fulgur_core::{LayoutChoice, Result};
use std::path::PathBuf;

#[derive(clap::Args)]
pub(crate) struct InspectArgs {
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
}

pub(crate) fn run(args: InspectArgs) -> Result<()> {
    let InspectArgs {
        input,
        dark,
        derived,
        sensor,
        report,
        csv,
        hfr_sigma,
        min_star_fraction,
        max_eccentricity,
    } = args;
    validate_report(Some(&report))?;
    validate_csv(csv.as_deref())?;
    let config = fulgur_pipeline::inspect::InspectConfig {
        hfr_sigma,
        min_star_fraction,
        max_eccentricity,
        ..Default::default()
    };
    let set = single_light_set(std::slice::from_ref(&input), sensor, &dark, &derived)?;
    let result =
        if sensor == LayoutChoice::Auto && set.calibration.is_none() && derived.is_default() {
            fulgur_pipeline::inspect::inspect(&input, config, &CANCEL)?
        } else {
            fulgur_pipeline::inspect::inspect_sets(&[set], config, &CANCEL, None)?.0
        };
    save_csv(csv.as_deref(), || fulgur_pipeline::inspect::csv(&result))?;
    save_report(Some(&report), &result)?;
    if let Some(warning) = result.target_warning() {
        println!("Warning: {warning}.");
    }
    println!(
        "PASS: {} frames inspected in {:.3} s; {} accepted, {} rejected; peak RAM {} MiB.",
        result.frame_count,
        result.wall_seconds,
        result.accepted_count,
        result.rejected_count,
        peak_mib(result.peak_rss_bytes)
    );
    println!(
        "Weights are provisional; registration measures matched-star transparency and photometry."
    );
    Ok(())
}
