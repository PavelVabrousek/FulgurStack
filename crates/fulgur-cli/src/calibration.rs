//! Calibration options and the in-RAM calibration masters they build.
use crate::CANCEL;
use fulgur_core::{Error, Result};
use fulgur_pipeline::calibration as cal;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(crate) const COMBINE_METHODS: [&str; 3] = ["average", "median", "sigma-kappa-2"];

/// Dark calibration options shared by stack, register and inspect.
#[derive(clap::Args, Debug, Clone)]
pub(crate) struct DarkArgs {
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
    /// Hot pixel map: a map file saved earlier, or dark or bias frames (files
    /// or folders, any exposure). The brightest pixels are replaced in every
    /// light by the median of their same-colour neighbours.
    #[arg(long)]
    hotpixel_map: Vec<PathBuf>,
    /// Share of all pixels the hot pixel map repairs, in percent (above 0, at
    /// most 5). Used when the map is built from frames.
    #[arg(long, default_value_t = cal::HOT_PIXEL_DEFAULT_PERCENT)]
    hotpixel_percent: f64,
    /// Save the hot pixel map built from frames as a map file (never
    /// overwrites).
    #[arg(long, requires = "hotpixel_map")]
    hotpixel_map_save: Option<PathBuf>,
    /// How dark frames are combined per pixel.
    #[arg(long, default_value = "sigma-kappa-2", value_parser = COMBINE_METHODS)]
    dark_combine: String,
    /// How bias, flat and dark-flat frames are combined per pixel.
    #[arg(long, default_value = "sigma-kappa-2", value_parser = COMBINE_METHODS)]
    calibration_combine: String,
}

pub(crate) fn combine_method(text: &str) -> cal::CombineMethod {
    cal::CombineMethod::parse(text).expect("clap validates the method")
}

fn report_master(m: &cal::MasterDark) {
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
    kind: cal::MasterKind,
    method: &str,
    offset: Option<&cal::MasterDark>,
) -> Result<Option<Arc<cal::MasterDark>>> {
    let master = if let Some(path) = file {
        cal::load_master(path, kind, &CANCEL)?
    } else if frames.is_empty() {
        return Ok(None);
    } else {
        let files = cal::collect_frames(frames)?;
        if files.is_empty() {
            return Err(Error::Invalid("No calibration FITS or XISF files found."));
        }
        if kind == cal::MasterKind::Flat {
            cal::build_flat_master(&files, offset, combine_method(method), &CANCEL, &|_| {})?
        } else {
            cal::build_master(&files, kind, combine_method(method), &CANCEL)?
        }
    };
    report_master(&master);
    Ok(Some(Arc::new(master)))
}

/// Build or load the calibration masters once and report them without
/// printing paths.
pub(crate) fn master_dark(args: &DarkArgs) -> Result<Option<Arc<cal::Calibration>>> {
    use cal::MasterKind;
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
    if bias.is_none() && dark.is_none() && flat.is_none() && args.hotpixel_map.is_empty() {
        return Ok(None);
    }
    let mut calibration = cal::Calibration::new(bias, dark, flat, offset)?;
    if !args.hotpixel_map.is_empty() {
        let started = std::time::Instant::now();
        let map =
            cal::HotPixelMap::from_inputs(&args.hotpixel_map, args.hotpixel_percent, &CANCEL)?;
        println!(
            "Hot pixel map: {} pixels ({:.4} % of {} x {}) from {}, ready in {:.2} s.",
            map.len(),
            map.summary.percent,
            map.width,
            map.height,
            map.summary.origin,
            started.elapsed().as_secs_f64()
        );
        if let Some(path) = &args.hotpixel_map_save {
            map.save(path, &CANCEL)?;
            println!("Hot pixel map saved.");
        }
        calibration = calibration.with_hot_pixels(Arc::new(map))?;
    }
    println!("Calibration: {}.", calibration.summary.formula);
    for warning in &calibration.summary.warnings {
        println!("Warning: {warning}.");
    }
    Ok(Some(Arc::new(calibration)))
}
