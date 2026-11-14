//! `survey`: survey FITS headers and measure read throughput.
use crate::CANCEL;
use crate::files::{save_report, validate_report};
use fulgur_core::Result;
use std::path::PathBuf;

#[derive(clap::Args)]
pub(crate) struct SurveyArgs {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    report: Option<PathBuf>,
}

pub(crate) fn run(args: SurveyArgs) -> Result<()> {
    let SurveyArgs { input, report } = args;
    validate_report(report.as_deref())?;
    let result = fulgur_pipeline::survey::survey(&input, &CANCEL)?;
    save_report(report.as_deref(), &result)?;
    println!(
        "PASS: {} FITS frames, {:.3} s, {:.1} MiB/s effective read throughput (OS cache uncontrolled).",
        result.frame_count, result.wall_seconds, result.read_mib_per_second
    );
    Ok(())
}
