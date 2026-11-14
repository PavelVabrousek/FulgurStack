//! `gaia-check`: check a local Gaia catalogue installation.
use crate::files::{save_report, validate_report};
use fulgur_core::{Error, Result};
use std::path::PathBuf;

#[derive(clap::Args)]
pub(crate) struct GaiaCheckArgs {
    /// Folder holding the Gaia DR3 .xpsd files.
    #[arg(long)]
    gaia_db: PathBuf,
    /// Also decompress and decode a sample of blocks in every file.
    #[arg(long)]
    deep: bool,
    #[arg(long)]
    report: Option<PathBuf>,
}

pub(crate) fn run(args: GaiaCheckArgs) -> Result<()> {
    let GaiaCheckArgs {
        gaia_db,
        deep,
        report,
    } = args;
    validate_report(report.as_deref())?;
    let check = fulgur_catalog::xpsd::check_installation(&gaia_db, deep);
    save_report(report.as_deref(), &check)?;
    for f in &check.files {
        println!(
            "  {}: {} {}, G {:.2}..{:.2}, {} stars, {} leaves{}{}",
            f.file,
            f.identifier,
            f.version,
            f.magnitude.0,
            f.magnitude.1,
            f.sources,
            f.leaves,
            if deep {
                format!(", {} blocks decoded", f.blocks_decoded)
            } else {
                String::new()
            },
            if f.problems.is_empty() {
                ""
            } else {
                " - PROBLEMS"
            }
        );
    }
    for (label, stars, seconds) in &check.probes {
        println!("  probe {label}: {stars} stars to G 14 within 1 degree in {seconds:.2} s");
    }
    for note in &check.notes {
        println!("  note: {note}");
    }
    for problem in &check.problems {
        println!("  problem: {problem}");
    }
    let coverage = check.magnitude.map_or("no coverage".to_string(), |(a, b)| {
        format!("G {a:.2}..{b:.2}")
    });
    if check.verdict != "complete" {
        println!(
            "Local Gaia catalogue in {}: {} ({} files, {coverage}).",
            check.folder,
            check.verdict,
            check.files.len()
        );
        return Err(Error::Invalid(
            "The local Gaia catalogue is not complete; see the problems listed above.",
        ));
    }
    println!(
        "PASS: local Gaia catalogue in {} is complete: {} {}, {} files, {coverage}, {} stars, checked in {:.1} s.",
        check.folder,
        check.identifier.as_deref().unwrap_or("?"),
        check.version.as_deref().unwrap_or("?"),
        check.files.len(),
        check.sources,
        check.seconds
    );
    Ok(())
}
