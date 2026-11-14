//! `master-dark`: combine dark frames into a master dark FITS for later runs.
use crate::CANCEL;
use crate::calibration::{COMBINE_METHODS, combine_method};
use crate::files::filename;
use fulgur_core::output::ensure_new_destination;
use fulgur_core::{Error, Result};
use std::path::PathBuf;

#[derive(clap::Args)]
pub(crate) struct MasterDarkArgs {
    /// Dark frames (files or folders).
    #[arg(long, required = true)]
    dark: Vec<PathBuf>,
    #[arg(long, default_value = "sigma-kappa-2", value_parser = COMBINE_METHODS)]
    combine: String,
    #[arg(long)]
    output: PathBuf,
}

pub(crate) fn run(args: MasterDarkArgs) -> Result<()> {
    let MasterDarkArgs {
        dark,
        combine,
        output,
    } = args;
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
    Ok(())
}
