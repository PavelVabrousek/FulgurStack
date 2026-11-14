//! Light-set options shared by stack, register and inspect: sensor layout,
//! values missing from the headers (ADR-084), and registration limits.
use crate::calibration::{DarkArgs, master_dark};
use fulgur_core::{Error, LayoutChoice, Result};
use fulgur_pipeline::inspect::InputSet;
use std::path::PathBuf;

/// Reference choice when no reference frame is given.
#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum ReferenceStrategyArg {
    Auto,
    ChronologicalMedian,
}

impl From<ReferenceStrategyArg> for fulgur_pipeline::register::ReferenceStrategy {
    fn from(value: ReferenceStrategyArg) -> Self {
        match value {
            ReferenceStrategyArg::Auto => Self::Auto,
            ReferenceStrategyArg::ChronologicalMedian => Self::ChronologicalMedian,
        }
    }
}

pub(crate) fn parse_accuracy(
    value: &str,
) -> std::result::Result<fulgur_pipeline::register::AccuracyLimits, String> {
    use fulgur_pipeline::register::AccuracyLimits;
    match value {
        "maximum-accuracy" => return Ok(AccuracyLimits::MAXIMUM_ACCURACY),
        "compromise" => return Ok(AccuracyLimits::COMPROMISE),
        "maximum-coverage" => return Ok(AccuracyLimits::MAXIMUM_COVERAGE),
        _ => {}
    }
    let parts: Vec<u32> = value
        .split(',')
        .map(|p| p.trim().parse::<u32>())
        .collect::<std::result::Result<_, _>>()
        .map_err(|_| "use a preset or RMS,PERCENT,MIN_STARS".to_owned())?;
    match parts[..] {
        [rms, percent, min_stars]
            if (1..=3).contains(&rms)
                && (30..=80).contains(&percent)
                && [20, 30, 50].contains(&min_stars) =>
        {
            Ok(AccuracyLimits {
                rms_px: rms,
                maximum_px: rms * 3,
                star_percent: percent,
                min_stars: min_stars as usize,
            })
        }
        _ => Err("RMS 1-3 px, PERCENT 30-80, MIN_STARS 20, 30 or 50".to_owned()),
    }
}

pub(crate) fn parse_layout(value: &str) -> std::result::Result<LayoutChoice, String> {
    LayoutChoice::parse(value).map_err(|e| e.to_string())
}

/// Values missing from the FITS headers (ADR-084), shared by stack,
/// register and inspect.
#[derive(clap::Args, Debug, Clone)]
pub(crate) struct DerivedArgs {
    /// Use the values reconstructed for missing header cards (exposure from
    /// EXPOSURE or the file name, colour layout from other cards, the file
    /// name or the camera catalogue) after checking the list a run prints.
    #[arg(long)]
    pub(crate) accept_derived: bool,
    /// Exposure in seconds, replacing header or derived values: one value for
    /// every light, or with --input-sets one value per --input, in order.
    #[arg(long, value_parser = parse_seconds)]
    pub(crate) exposure: Vec<f64>,
}

impl DerivedArgs {
    pub(crate) fn is_default(&self) -> bool {
        !self.accept_derived && self.exposure.is_empty()
    }

    /// The exposure given for light set `index` of `sets`: one value for all
    /// sets, or one value per set (B-21).
    pub(crate) fn exposure_for(&self, index: usize, sets: usize) -> Result<Option<f64>> {
        match self.exposure.len() {
            0 => Ok(None),
            1 => Ok(Some(self.exposure[0])),
            n if n == sets => Ok(Some(self.exposure[index])),
            _ => Err(Error::Invalid(
                "Give one --exposure for every light, or with --input-sets one --exposure per --input.",
            )),
        }
    }

    pub(crate) fn apply(&self, set: &mut InputSet) -> Result<()> {
        set.exposure = self.exposure_for(0, 1)?;
        set.accept_derived = self.accept_derived;
        Ok(())
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
pub(crate) fn check_derived(sets: &[InputSet], args: &DerivedArgs) -> Result<()> {
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

/// One light set from the inputs, calibrated as asked, with its derived
/// header values checked (register and inspect).
pub(crate) fn single_light_set(
    inputs: &[PathBuf],
    sensor: LayoutChoice,
    dark: &DarkArgs,
    derived: &DerivedArgs,
) -> Result<InputSet> {
    let calibration = master_dark(dark)?;
    let mut set = fulgur_pipeline::inspect::single_set(inputs, sensor);
    set.calibration = calibration;
    derived.apply(&mut set)?;
    check_derived(std::slice::from_ref(&set), derived)?;
    Ok(set)
}
