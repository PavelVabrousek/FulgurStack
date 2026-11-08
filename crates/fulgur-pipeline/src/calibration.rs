//! Dark calibration: an in-RAM master dark, its optional FITS file, and its
//! subtraction.
//!
//! A master is built in RAM; it reaches disk only when the owner saves it.
//! Lights become `light - master + pedestal`, where the constant pedestal is
//! the master's median: hot pixels, amplifier glow and fixed pattern are
//! removed while the sky stays above zero, so the 0..1 export never truncates
//! calibrated background noise. A constant offset cancels in the existing
//! per-frame background normalization.
use fulgur_core::fits::{
    BLOCK_SIZE, FitsHeader, FrameInfo, read_calibration_header, read_metadata, read_roi,
};
use fulgur_core::{Error, RawFrame, Result, check_cancelled};
use rayon::prelude::*;
use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

/// Rows combined per task; bounded work memory is frames x rows x width x 4 B.
const STRIP_ROWS: usize = 32;
/// Dark exposure must match the lights within this relative tolerance.
const EXPOSURE_TOLERANCE: f64 = 0.01;
/// Sigma-kappa clipping: kappa and the iteration limit.
const KAPPA: f64 = 2.0;
const KAPPA_ITERATIONS: usize = 5;
/// Units marker written to our master files.
const UNITS_CARD: &str = "ADU";

/// How dark frames are combined per pixel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CombineMethod {
    Average,
    Median,
    /// Iterative mean/standard-deviation clipping at 2 sigma, then the mean.
    #[default]
    SigmaKappa2,
}

impl CombineMethod {
    pub const ALL: [Self; 3] = [Self::Average, Self::Median, Self::SigmaKappa2];
    pub fn key(self) -> &'static str {
        match self {
            Self::Average => "average",
            Self::Median => "median",
            Self::SigmaKappa2 => "sigma_kappa_2",
        }
    }
    pub fn parse(text: &str) -> Option<Self> {
        let t = text.trim().to_ascii_lowercase().replace('-', "_");
        Self::ALL.into_iter().find(|m| m.key() == t)
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Average => "Average",
            Self::Median => "Median",
            Self::SigmaKappa2 => "Average, sigma-kappa 2",
        }
    }
    fn rule(self) -> &'static str {
        match self {
            Self::Average => "per-pixel mean of all frames; f64 compensated sums",
            Self::Median => "per-pixel median (mean of the two central values for an even count)",
            Self::SigmaKappa2 => {
                "per-pixel mean after iterative clipping at mean +- 2 standard deviations (at most 5 passes); f64 compensated sums"
            }
        }
    }
}

/// Type of calibration master. Only darks are applied to lights so far.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MasterKind {
    Bias,
    #[default]
    Dark,
    Flat,
    DarkFlat,
}

impl MasterKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Bias => "bias",
            Self::Dark => "dark",
            Self::Flat => "flat",
            Self::DarkFlat => "dark_flat",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Bias => "bias",
            Self::Dark => "dark",
            Self::Flat => "flat",
            Self::DarkFlat => "dark flat",
        }
    }
    fn image_type(self) -> &'static str {
        match self {
            Self::Bias => "Master Bias",
            Self::Dark => "Master Dark",
            Self::Flat => "Master Flat",
            Self::DarkFlat => "Master Dark Flat",
        }
    }
    fn file_stem(self) -> &'static str {
        match self {
            Self::Bias => "MasterBias",
            Self::Dark => "MasterDark",
            Self::Flat => "MasterFlat",
            Self::DarkFlat => "MasterDarkFlat",
        }
    }
    /// Thermal masters require one exposure; bias and (normalized) flats do not.
    fn same_exposure(self) -> bool {
        matches!(self, Self::Dark | Self::DarkFlat)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct DarkSummary {
    pub kind: MasterKind,
    /// Combined frames (NCOMBINE of a loaded master; 0 when unknown).
    pub frames: usize,
    pub width: usize,
    pub height: usize,
    pub exposure_seconds: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gain: Option<f64>,
    /// Lowest and highest sensor temperature of the dark frames (CCD-TEMP/DET-TEMP).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature_c: Option<[f64; 2]>,
    pub pedestal_adu: f64,
    pub median_adu: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub combine: Option<CombineMethod>,
    pub clipped_samples: u64,
    pub combine_rule: &'static str,
    pub calibration_rule: &'static str,
    pub source_files: Vec<String>,
    /// File name of a master loaded from disk instead of built from frames.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loaded_from: Option<String>,
    /// Loaded float masters normalized to 0..1 are scaled to ADU by 65535.
    pub normalized_input_scaled: bool,
    /// Flats only: how frames were brought to one level before combining.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flat_normalization: Option<&'static str>,
    /// Owner-facing warnings: mixed exposures or gains, unknown exposure.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    pub build_seconds: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_rss_bytes: Option<u64>,
    pub written_to_disk: bool,
}

pub struct MasterDark {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<f32>,
    pub pedestal: f32,
    pub bayer_pattern: Option<fulgur_core::BayerPattern>,
    /// Flats: the sensor offset (dark flat or bias) is already removed.
    pub offset_removed: bool,
    pub summary: DarkSummary,
}

impl std::fmt::Debug for MasterDark {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MasterDark")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("pedestal", &self.pedestal)
            .field("summary", &self.summary)
            .finish()
    }
}

/// Why a light cannot use this master at all (other dimensions); `None`
/// when it can be subtracted.
pub fn incompatibility(dark: &DarkSummary, light: &FrameInfo) -> Option<&'static str> {
    (light.width != dark.width || light.height != dark.height).then_some("dark_size_mismatch")
}

/// Distinct values (to 1e-6 relative) for warning texts.
fn distinct(values: impl Iterator<Item = f64>) -> Vec<f64> {
    let mut out: Vec<f64> = vec![];
    for v in values {
        if !out.iter().any(|o| (o - v).abs() <= 1e-6 * o.abs().max(1.)) {
            out.push(v);
        }
    }
    out.sort_by(f64::total_cmp);
    out
}

fn list(values: &[f64], unit: &str) -> String {
    values
        .iter()
        .map(|v| format!("{}{unit}", trim_number(*v)))
        .collect::<Vec<_>>()
        .join(", ")
}

impl MasterDark {
    /// Calibrate a full frame or a phase-preserving ROI at `origin` in place.
    pub fn subtract(&self, frame: &mut RawFrame, origin: [usize; 2]) -> Result<()> {
        let (w, h) = (frame.header.info.width, frame.header.info.height);
        if origin[0] + w > self.width || origin[1] + h > self.height || frame.pixels.len() != w * h
        {
            return Err(Error::Invalid("Light region lies outside the master dark."));
        }
        for (y, row) in frame.pixels.chunks_exact_mut(w).enumerate() {
            let start = (origin[1] + y) * self.width + origin[0];
            for (value, dark) in row.iter_mut().zip(&self.pixels[start..start + w]) {
                *value = *value - dark + self.pedestal;
            }
        }
        Ok(())
    }

    /// Suggested file name, e.g. `MasterDark_60s_gain60_37x_sigma_kappa_2.fits`.
    pub fn suggested_name(&self) -> String {
        let s = &self.summary;
        let mut name = s.kind.file_stem().to_owned();
        if s.exposure_seconds > 0. {
            name.push_str(&format!("_{}s", trim_number(s.exposure_seconds)));
        }
        if let Some(g) = s.gain {
            name.push_str(&format!("_gain{}", trim_number(g)));
        }
        if s.frames > 0 {
            name.push_str(&format!("_{}x", s.frames));
        }
        if let Some(m) = s.combine {
            name.push('_');
            name.push_str(m.key());
        }
        name + ".fits"
    }
}

fn trim_number(value: f64) -> String {
    let text = format!("{value:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// FITS files of the given files/folders, sorted and deduplicated.
pub fn collect_frames(inputs: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files = vec![];
    for input in inputs {
        crate::survey::collect_inputs(input, &mut files)?;
    }
    let mut files = files
        .into_iter()
        .map(|p| p.canonicalize())
        .collect::<std::io::Result<Vec<_>>>()?;
    files.sort();
    files.dedup();
    Ok(files)
}

fn neumaier(values: impl Iterator<Item = f64>) -> f64 {
    let (mut sum, mut compensation) = (0.0_f64, 0.0_f64);
    for v in values {
        let t = sum + v;
        compensation += if sum.abs() >= v.abs() {
            (sum - t) + v
        } else {
            (v - t) + sum
        };
        sum = t;
    }
    sum + compensation
}

fn median_f32(values: &mut [f32]) -> f64 {
    let (len, mid) = (values.len(), values.len() / 2);
    let (lower, center, _) = values.select_nth_unstable_by(mid, f32::total_cmp);
    if len % 2 == 0 {
        let below = lower.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        (f64::from(below) + f64::from(*center)) * 0.5
    } else {
        f64::from(*center)
    }
}

/// One pixel's combined value and the number of clipped samples.
fn combine(method: CombineMethod, samples: &mut [f32]) -> (f32, u32) {
    let n = samples.len();
    match method {
        CombineMethod::Average => (
            (neumaier(samples.iter().map(|&v| f64::from(v))) / n as f64) as f32,
            0,
        ),
        CombineMethod::Median => (median_f32(samples) as f32, 0),
        CombineMethod::SigmaKappa2 => {
            let (mut low, mut high) = (f64::NEG_INFINITY, f64::INFINITY);
            let mut kept = n;
            let mut mean = 0.0;
            for _ in 0..KAPPA_ITERATIONS {
                let inside = || {
                    samples
                        .iter()
                        .map(|&v| f64::from(v))
                        .filter(move |v| *v >= low && *v <= high)
                };
                let count = inside().count();
                mean = neumaier(inside()) / count as f64;
                let variance = neumaier(inside().map(|v| (v - mean) * (v - mean))) / count as f64;
                let sigma = variance.sqrt();
                kept = count;
                let (next_low, next_high) = (mean - KAPPA * sigma, mean + KAPPA * sigma);
                let next = samples
                    .iter()
                    .map(|&v| f64::from(v))
                    .filter(|v| *v >= next_low && *v <= next_high)
                    .count();
                // Stop when nothing more would be clipped, or before clipping
                // below half of the frames (keeps the estimate defined).
                if next == count || next * 2 < n || sigma == 0. {
                    break;
                }
                (low, high) = (next_low, next_high);
            }
            (mean as f32, (n - kept) as u32)
        }
    }
}

fn temperature_range(values: &[f64]) -> Option<[f64; 2]> {
    (!values.is_empty()).then(|| {
        [
            values.iter().copied().fold(f64::INFINITY, f64::min),
            values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        ]
    })
}

const CALIBRATION_RULE: &str = "light - master dark + pedestal (rounded master median); before cosmetic correction, star measurement and demosaicing; no BIAS card used";

/// Combine dark frames into a master in RAM (see `build_master`).
pub fn build_master_dark(
    paths: &[PathBuf],
    method: CombineMethod,
    cancel: &AtomicBool,
) -> Result<MasterDark> {
    build_master(paths, MasterKind::Dark, method, cancel)
}

/// Median of sparse full-width row bands: a flat's level for normalization.
fn frame_level(
    path: &Path,
    header: &FitsHeader,
    offset: Option<&MasterDark>,
    cancel: &AtomicBool,
) -> Result<f64> {
    let (width, height) = (header.info.width, header.info.height);
    let mut samples = Vec::new();
    for band in 0..16 {
        let y = (band * height / 16).min(height - 6) & !1;
        let pixels = read_roi(path, header, [0, y, width, 6], cancel)?.pixels;
        match offset {
            Some(o) => samples.extend(
                pixels
                    .iter()
                    .zip(&o.pixels[y * width..(y + 6) * width])
                    .map(|(v, z)| v - z),
            ),
            None => samples.extend(pixels),
        }
    }
    Ok(median_f32(&mut samples))
}

/// Combine calibration frames into a master in RAM, strip by strip, never
/// holding whole frames. Sources are opened read-only. Flats are first scaled
/// to their mean median level (no offset is removed yet); darks and dark flats
/// require one exposure; bias frames may have zero exposure.
pub fn build_master(
    paths: &[PathBuf],
    kind: MasterKind,
    method: CombineMethod,
    cancel: &AtomicBool,
) -> Result<MasterDark> {
    build_master_with_progress(paths, kind, method, cancel, &|_| {})
}

/// As `build_master`, reporting the completed fraction (0..1) after each
/// flat level and each combined strip. Reports may arrive from any worker.
pub fn build_master_with_progress(
    paths: &[PathBuf],
    kind: MasterKind,
    method: CombineMethod,
    cancel: &AtomicBool,
    progress: &(dyn Fn(f32) + Sync),
) -> Result<MasterDark> {
    build(paths, kind, method, None, cancel, progress)
}

/// Master flat: each frame minus `offset` (master dark flat, else master
/// bias), scaled to the mean of the frames' offset-free medians, combined.
/// Without an offset master the flat keeps the sensor offset (warned).
pub fn build_flat_master(
    paths: &[PathBuf],
    offset: Option<&MasterDark>,
    method: CombineMethod,
    cancel: &AtomicBool,
    progress: &(dyn Fn(f32) + Sync),
) -> Result<MasterDark> {
    build(paths, MasterKind::Flat, method, offset, cancel, progress)
}

fn build(
    paths: &[PathBuf],
    kind: MasterKind,
    method: CombineMethod,
    offset: Option<&MasterDark>,
    cancel: &AtomicBool,
    progress: &(dyn Fn(f32) + Sync),
) -> Result<MasterDark> {
    let start = Instant::now();
    if paths.len() < 3 {
        return Err(Error::Invalid(
            "A calibration master needs at least three frames.",
        ));
    }
    let headers: Vec<FitsHeader> = paths
        .iter()
        .map(|p| read_calibration_header(p))
        .collect::<Result<_>>()?;
    let first = &headers[0].info;
    let (width, height) = (first.width, first.height);
    if headers
        .iter()
        .any(|h| h.info.width != width || h.info.height != height)
    {
        return Err(Error::Invalid(
            "Calibration frames have different dimensions.",
        ));
    }
    // Owner decision (ADR-080): mixed exposures or gains only warn.
    let mut warnings = vec![];
    let exposures = distinct(headers.iter().map(|h| h.info.exposure_seconds));
    if kind.same_exposure() && exposures.len() > 1 {
        warnings.push(format!(
            "{} frames have different exposures ({})",
            kind.title(),
            list(&exposures, " s")
        ));
    }
    if kind.same_exposure() && exposures.iter().any(|e| *e <= 0.) {
        warnings.push(format!("some {} frames record no exposure", kind.title()));
    }
    let gains = distinct(headers.iter().filter_map(|h| h.info.gain));
    if gains.len() > 1 {
        warnings.push(format!(
            "{} frames have different gains ({})",
            kind.title(),
            list(&gains, "")
        ));
    }
    let mut sorted: Vec<f64> = headers.iter().map(|h| h.info.exposure_seconds).collect();
    sorted.sort_by(f64::total_cmp);
    let exposure_seconds = sorted[sorted.len() / 2];
    if height < 6 || width < 6 || height % 2 != 0 {
        return Err(Error::Invalid("Unsupported calibration frame dimensions."));
    }
    let offset = offset.filter(|_| kind == MasterKind::Flat);
    if offset.is_some_and(|o| o.width != width || o.height != height) {
        return Err(Error::Invalid(
            "The flat offset master has other dimensions than the flats.",
        ));
    }
    if kind == MasterKind::Flat && offset.is_none() {
        warnings.push("flat offset not removed (no dark flat or bias master)".to_owned());
    }
    let strip_count = (height / STRIP_ROWS).max(1);
    let level_steps = if kind == MasterKind::Flat {
        paths.len()
    } else {
        0
    };
    let steps = (level_steps + strip_count) as f32;
    let done = std::sync::atomic::AtomicUsize::new(0);
    let step = || {
        let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        progress(n as f32 / steps);
    };
    let scales: Vec<f32> = if kind == MasterKind::Flat {
        let levels = paths
            .par_iter()
            .zip(&headers)
            .map(|(p, h)| {
                let level = frame_level(p, h, offset, cancel);
                step();
                level
            })
            .collect::<Result<Vec<_>>>()?;
        if levels.iter().any(|l| *l <= 0. || !l.is_finite()) {
            return Err(Error::Invalid("A flat frame has no positive signal."));
        }
        let mean = levels.iter().sum::<f64>() / levels.len() as f64;
        levels.iter().map(|l| (mean / l) as f32).collect()
    } else {
        vec![1.; paths.len()]
    };
    let temperatures: Vec<f64> = paths
        .iter()
        .filter_map(|p| read_metadata(p).ok().and_then(|m| m.ccd_temperature))
        .collect();
    // Strips start on even rows; the remainder joins the last strip so every
    // ROI keeps Bayer phase and the reader's minimum height.
    let count = (height / STRIP_ROWS).max(1);
    let strips: Vec<[usize; 2]> = (0..count)
        .map(|i| {
            let y = i * STRIP_ROWS;
            [
                y,
                if i + 1 == count {
                    height - y
                } else {
                    STRIP_ROWS
                },
            ]
        })
        .collect();
    let n = paths.len();
    let rows: Vec<(Vec<f32>, u64)> = strips
        .par_iter()
        .map(|&[y, rows]| -> Result<(Vec<f32>, u64)> {
            check_cancelled(cancel)?;
            let mut stack = Vec::with_capacity(n);
            for (path, header) in paths.iter().zip(&headers) {
                stack.push(read_roi(path, header, [0, y, width, rows], cancel)?.pixels);
            }
            let mut out = vec![0_f32; width * rows];
            let mut samples = vec![0_f32; n];
            let mut clipped = 0_u64;
            for (i, value) in out.iter_mut().enumerate() {
                let zero = offset.map_or(0., |o| o.pixels[y * width + i]);
                for ((sample, frame), scale) in samples.iter_mut().zip(&stack).zip(&scales) {
                    *sample = (frame[i] - zero) * scale;
                }
                let (combined, rejected) = combine(method, &mut samples);
                *value = combined;
                clipped += u64::from(rejected);
            }
            step();
            Ok((out, clipped))
        })
        .collect::<Result<_>>()?;
    let mut pixels = Vec::with_capacity(width * height);
    let mut clipped_samples = 0;
    for (strip, clipped) in rows {
        pixels.extend_from_slice(&strip);
        clipped_samples += clipped;
    }
    let median = median_f32(&mut pixels.clone());
    let pedestal = median.round() as f32;
    Ok(MasterDark {
        width,
        height,
        pixels,
        pedestal,
        bayer_pattern: first.bayer_pattern,
        offset_removed: kind != MasterKind::Flat || offset.is_some(),
        summary: DarkSummary {
            kind,
            frames: n,
            width,
            height,
            exposure_seconds,
            gain: first.gain,
            temperature_c: temperature_range(&temperatures),
            pedestal_adu: f64::from(pedestal),
            median_adu: median,
            combine: Some(method),
            clipped_samples,
            combine_rule: method.rule(),
            calibration_rule: CALIBRATION_RULE,
            source_files: paths
                .iter()
                .map(|p| {
                    p.file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
                })
                .collect(),
            loaded_from: None,
            normalized_input_scaled: false,
            warnings,
            flat_normalization: (kind == MasterKind::Flat).then_some(if offset.is_some() {
                "offset master subtracted per frame, then each frame scaled to the mean of the frames' medians (16 sparse row bands)"
            } else {
                "each frame scaled to the mean of the frames' medians (16 sparse row bands); no offset removed"
            }),
            build_seconds: start.elapsed().as_secs_f64(),
            peak_rss_bytes: fulgur_core::telemetry::peak_rss_bytes(),
            written_to_disk: false,
        },
    })
}

/// Build a master from files or folders (convenience for the CLI and GUI).
pub fn build_master_dark_from(
    inputs: &[PathBuf],
    method: CombineMethod,
    cancel: &AtomicBool,
) -> Result<MasterDark> {
    let files = collect_frames(inputs)?;
    if files.is_empty() {
        return Err(Error::Invalid("No dark FITS files found."));
    }
    build_master_dark(&files, method, cancel)
}

fn card(key: &str, value: &str) -> [u8; 80] {
    let text = if key == "HISTORY" || key == "END" {
        format!("{key:<8}{value}")
    } else {
        format!("{key:<8}= {value:>20}")
    };
    let mut card = [b' '; 80];
    let bytes = text.as_bytes();
    card[..bytes.len().min(80)].copy_from_slice(&bytes[..bytes.len().min(80)]);
    card
}

fn quoted(text: &str) -> String {
    format!("'{:<8}'", text.replace('\'', "''"))
}

/// Write a master dark (see `save_master`).
pub fn save_master_dark(master: &MasterDark, path: &Path, cancel: &AtomicBool) -> Result<()> {
    save_master(master, path, cancel)
}

/// Write the master as a 32-bit float FITS in ADU (no normalization), with
/// its provenance; atomic `.tmp` rename, never overwriting an existing file.
pub fn save_master(master: &MasterDark, path: &Path, cancel: &AtomicBool) -> Result<()> {
    let s = &master.summary;
    let mut cards = vec![
        card("SIMPLE", "T"),
        card("BITPIX", "-32"),
        card("NAXIS", "2"),
        card("NAXIS1", &master.width.to_string()),
        card("NAXIS2", &master.height.to_string()),
        card("BZERO", "0"),
        card("BSCALE", "1"),
        card("IMAGETYP", &quoted(s.kind.image_type())),
        card("EXPTIME", &format!("{:.6}", s.exposure_seconds)),
        card("BUNIT", &quoted(UNITS_CARD)),
        card("CREATOR", &quoted("FulgurStack")),
        card("PEDESTAL", &format!("{:.1}", s.pedestal_adu)),
    ];
    if s.frames > 0 {
        cards.push(card("NCOMBINE", &s.frames.to_string()));
    }
    if let Some(g) = s.gain {
        cards.push(card("GAIN", &format!("{g}")));
    }
    if let Some(p) = master.bayer_pattern {
        cards.push(card("BAYERPAT", &quoted(p.name())));
    }
    if let Some([low, high]) = s.temperature_c {
        cards.push(card("CCD-TEMP", &format!("{:.1}", (low + high) / 2.)));
        cards.push(card("TEMPMIN", &format!("{low:.1}")));
        cards.push(card("TEMPMAX", &format!("{high:.1}")));
    }
    if let Some(m) = s.combine {
        cards.push(card("COMBINE", &quoted(m.key())));
    }
    cards.push(card(
        "HISTORY",
        &format!(
            "Master {} in ADU; values are not normalized.",
            s.kind.title()
        ),
    ));
    match s.kind {
        MasterKind::Dark => cards.push(card(
            "HISTORY",
            "Use: light - master dark + PEDESTAL (FulgurStack).",
        )),
        MasterKind::Flat => {
            cards.push(card("FLATNORM", &quoted("median")));
            cards.push(card(
                "OFFSETRM",
                if master.offset_removed { "T" } else { "F" },
            ));
            cards.push(card(
                "HISTORY",
                if master.offset_removed {
                    "Flats minus offset master, scaled to a common median."
                } else {
                    "Flats scaled to a common median; no offset removed."
                },
            ));
        }
        _ => {}
    }
    cards.push(card("END", ""));
    fulgur_core::output::atomic_write(path, cancel, |writer| {
        for c in &cards {
            writer.write_all(c)?;
        }
        let padding = (BLOCK_SIZE - cards.len() * 80 % BLOCK_SIZE) % BLOCK_SIZE;
        writer.write_all(&vec![b' '; padding])?;
        let mut row = Vec::with_capacity(master.width * 4);
        for line in master.pixels.chunks_exact(master.width) {
            check_cancelled(cancel)?;
            row.clear();
            for v in line {
                row.extend_from_slice(&v.to_be_bytes());
            }
            writer.write_all(&row)?;
        }
        let data = master.pixels.len() * 4;
        writer.write_all(&vec![0; (BLOCK_SIZE - data % BLOCK_SIZE) % BLOCK_SIZE])?;
        Ok(())
    })
}

/// Read a master dark FITS (BITPIX 8/16/32/-32/-64; one plane, or three
/// identical planes as some stackers write mono masters). Float masters
/// without `BUNIT='ADU'` whose values all lie within 0..1 are treated as
/// normalized (e.g. PixInsight) and scaled by 65535.
pub fn load_master_dark(path: &Path, cancel: &AtomicBool) -> Result<MasterDark> {
    load_master(path, MasterKind::Dark, cancel)
}

/// Read a master of any type (see `load_master_dark`). A loaded flat counts as
/// offset-free when it says so (`OFFSETRM = T`) or is floating point
/// (calibrated by its stacker); integer flats are treated as raw and get the
/// set's offset master subtracted when calibration is prepared.
pub fn load_master(path: &Path, kind: MasterKind, cancel: &AtomicBool) -> Result<MasterDark> {
    let start = Instant::now();
    if fulgur_core::xisf::is_xisf(path) {
        return load_xisf_master(path, kind, cancel, start);
    }
    let mut file = std::io::BufReader::new(std::fs::File::open(path)?);
    let mut cards: Vec<(String, String)> = vec![];
    let mut header_bytes = 0;
    'blocks: loop {
        let mut block = [0u8; BLOCK_SIZE];
        file.read_exact(&mut block)?;
        header_bytes += BLOCK_SIZE;
        for raw in block.chunks_exact(80) {
            let text = String::from_utf8_lossy(raw);
            let key = text[..8].trim().to_owned();
            if key == "END" {
                break 'blocks;
            }
            if &text[8..10] == "= " {
                let value = text[10..].split(" /").next().unwrap_or("").trim();
                cards.push((key, value.trim_matches('\'').trim().to_owned()));
            }
        }
        if header_bytes > 100 * BLOCK_SIZE {
            return Err(Error::Invalid("Master dark header is too long."));
        }
    }
    let get = |k: &str| {
        cards
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.as_str())
    };
    let number = |k: &str| get(k).and_then(|v| v.parse::<f64>().ok());
    let bitpix = number("BITPIX").unwrap_or(0.) as i32;
    let axes = number("NAXIS").unwrap_or(0.) as usize;
    let planes = if axes == 3 {
        number("NAXIS3").unwrap_or(0.) as usize
    } else {
        1
    };
    let bytes_per_sample = match bitpix {
        8 => 1,
        16 => 2,
        32 | -32 => 4,
        -64 => 8,
        _ => 0,
    };
    if get("SIMPLE") != Some("T")
        || !(axes == 2 || (axes == 3 && matches!(planes, 1 | 3)))
        || bytes_per_sample == 0
    {
        return Err(Error::Invalid(
            "A master dark must be a FITS image with one plane (or three identical planes).",
        ));
    }
    let width = number("NAXIS1").unwrap_or(0.) as usize;
    let height = number("NAXIS2").unwrap_or(0.) as usize;
    if width < 6 || height < 6 {
        return Err(Error::Invalid("Unsupported master dark dimensions."));
    }
    let exposure_seconds = number("EXPTIME")
        .or_else(|| number("EXPOSURE"))
        .filter(|e| *e > 0.)
        .unwrap_or(0.);
    let (bzero, bscale) = (
        number("BZERO").unwrap_or(0.),
        number("BSCALE").unwrap_or(1.),
    );
    let decode = |b: &[u8]| -> f64 {
        let stored = match bitpix {
            8 => f64::from(b[0]),
            16 => f64::from(i16::from_be_bytes([b[0], b[1]])),
            32 => f64::from(i32::from_be_bytes([b[0], b[1], b[2], b[3]])),
            -32 => f64::from(f32::from_be_bytes([b[0], b[1], b[2], b[3]])),
            _ => f64::from_be_bytes(b.try_into().unwrap()),
        };
        stored * bscale + bzero
    };
    let plane_bytes = width * height * bytes_per_sample;
    let mut bytes = vec![0u8; plane_bytes];
    for chunk in bytes.chunks_mut(1 << 20) {
        check_cancelled(cancel)?;
        file.read_exact(chunk)?;
    }
    let mut pixels: Vec<f32> = bytes
        .chunks_exact(bytes_per_sample)
        .map(|b| decode(b) as f32)
        .collect();
    // Some stackers write a monochrome master as three identical RGB planes.
    // Accept those; a genuinely debayered colour master cannot calibrate raw
    // CFA or mono samples.
    for _ in 1..planes {
        for (i, chunk) in bytes.chunks_mut(1 << 20).enumerate() {
            check_cancelled(cancel)?;
            file.read_exact(chunk)?;
            let first = i * (1 << 20) / bytes_per_sample;
            for (j, b) in chunk.chunks_exact(bytes_per_sample).enumerate() {
                let (a, v) = (f64::from(pixels[first + j]), decode(b));
                if (a - v).abs() > 1e-5 * a.abs().max(1.) {
                    return Err(Error::Invalid(
                        "Master dark is a colour (debayered) image; use a one-plane master or the dark frames.",
                    ));
                }
            }
        }
    }
    if pixels.iter().any(|v| !v.is_finite()) {
        return Err(Error::Invalid("Master dark contains non-finite values."));
    }
    // Frames are read top-down, so a master must arrive in the same order.
    if get("ROWORDER") == Some("BOTTOM-UP") {
        fulgur_core::fits::flip_rows(&mut pixels, width);
    }
    let adu = get("BUNIT").is_some_and(|u| u.eq_ignore_ascii_case(UNITS_CARD));
    let normalized = bitpix < 0 && !adu && pixels.iter().all(|v| (0.0..=1.0).contains(v));
    if normalized {
        for v in &mut pixels {
            *v *= 65535.;
        }
    }
    Ok(master_from(
        path,
        kind,
        start,
        width,
        height,
        pixels,
        normalized,
        bitpix,
        exposure_seconds,
        &get,
    ))
}

/// A master in XISF: samples arrive in ADU, whatever their stored format.
fn load_xisf_master(
    path: &Path,
    kind: MasterKind,
    cancel: &AtomicBool,
    start: Instant,
) -> Result<MasterDark> {
    let header = read_calibration_header(path)?;
    let meta = read_metadata(path)?;
    let image = header
        .xisf
        .as_ref()
        .ok_or(Error::Invalid("XISF unit has no image."))?;
    let (width, height) = (image.width, image.height);
    let pixels = image.read_plane(0, cancel)?;
    if image.channels == 3 {
        for channel in 1..3 {
            let plane = image.read_plane(channel, cancel)?;
            if pixels
                .iter()
                .zip(&plane)
                .any(|(a, v)| (a - v).abs() > 1e-5 * a.abs().max(1.))
            {
                return Err(Error::Invalid(
                    "Master dark is a colour (debayered) image; use a one-plane master or the dark frames.",
                ));
            }
        }
    }
    if pixels.iter().any(|v| !v.is_finite()) {
        return Err(Error::Invalid("Master dark contains non-finite values."));
    }
    let keywords: Vec<(&str, String)> = [
        ("BAYERPAT", meta.bayer_pattern.map(|p| p.name().to_owned())),
        ("GAIN", meta.gain.map(|g| g.to_string())),
        ("NCOMBINE", meta.combined_frames.map(|n| n.to_string())),
        ("CCD-TEMP", meta.ccd_temperature.map(|t| t.to_string())),
    ]
    .into_iter()
    .filter_map(|(key, value)| Some((key, value?)))
    .collect();
    let get = |key: &str| {
        keywords
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.as_str())
    };
    Ok(master_from(
        path,
        kind,
        start,
        width,
        height,
        pixels,
        image.bitpix() < 0,
        image.bitpix(),
        meta.exposure_seconds.filter(|e| *e > 0.).unwrap_or(0.),
        &get,
    ))
}

#[allow(clippy::too_many_arguments)] // One loaded master and its stored facts.
fn master_from<'a>(
    path: &Path,
    kind: MasterKind,
    start: Instant,
    width: usize,
    height: usize,
    pixels: Vec<f32>,
    normalized: bool,
    bitpix: i32,
    exposure_seconds: f64,
    get: &dyn Fn(&str) -> Option<&'a str>,
) -> MasterDark {
    let number = |k: &str| get(k).and_then(|v| v.parse::<f64>().ok());
    let median = median_f32(&mut pixels.clone());
    let pedestal = median.round() as f32;
    let temperatures: Vec<f64> = ["TEMPMIN", "TEMPMAX"]
        .iter()
        .filter_map(|k| number(k))
        .collect();
    let temperatures = if temperatures.is_empty() {
        ["CCD-TEMP", "DET-TEMP", "SET-TEMP"]
            .iter()
            .find_map(|k| number(k))
            .into_iter()
            .collect()
    } else {
        temperatures
    };
    let offset_removed = kind != MasterKind::Flat
        || get("OFFSETRM").map_or(bitpix < 0, |v| v.eq_ignore_ascii_case("T"));
    MasterDark {
        width,
        height,
        pixels,
        pedestal,
        bayer_pattern: get("BAYERPAT").and_then(|p| fulgur_core::BayerPattern::parse(p).ok()),
        offset_removed,
        summary: DarkSummary {
            kind,
            frames: number("NCOMBINE").map_or(0, |n| n.max(0.) as usize),
            width,
            height,
            exposure_seconds,
            gain: number("GAIN"),
            temperature_c: temperature_range(&temperatures),
            pedestal_adu: f64::from(pedestal),
            median_adu: median,
            combine: get("COMBINE").and_then(CombineMethod::parse),
            clipped_samples: 0,
            combine_rule: "loaded master file",
            calibration_rule: CALIBRATION_RULE,
            source_files: vec![],
            loaded_from: path.file_name().map(|n| n.to_string_lossy().into_owned()),
            normalized_input_scaled: normalized,
            flat_normalization: None,
            warnings: if exposure_seconds > 0. || !kind.same_exposure() {
                vec![]
            } else {
                vec![format!(
                    "master {} records no exposure (EXPTIME); assumed equal to the lights",
                    kind.title()
                )]
            },
            build_seconds: start.elapsed().as_secs_f64(),
            peak_rss_bytes: fulgur_core::telemetry::peak_rss_bytes(),
            written_to_disk: false,
        },
    }
}

/// Raw samples at or above this level are saturated: their true value is
/// unknown, so calibration keeps them saturated instead of subtracting a
/// (possibly also saturated, exposure-scaled) dark or dividing by a flat.
const SATURATED_ADU: f32 = 65000.;
/// Normalized flat values outside this range are defects (dead pixels, dust
/// cores, frame edges) and are not divided by.
const FLAT_RANGE: (f32, f32) = (0.05, 20.);
/// Dark defects: scaled thermal signal above this many ADU makes a pixel a
/// known hot pixel; when the light holds less than half of it, that hot pixel
/// is absent or unstable in the light (aged dark library, other temperature).
const HOT_DARK_ADU: f32 = 50.;

/// What calibrating one frame or ROI changed.
#[derive(Clone, Copy, Debug, Default)]
pub struct CalibrationStats {
    /// Calibrated samples still below zero after dark-defect repair.
    pub negative_samples: usize,
    /// Over-subtracted master-dark hot pixels replaced by the median of their
    /// same-phase neighbours.
    pub repaired_dark_defects: usize,
    /// Pixels of the hot pixel map replaced from their neighbours (ADR-163).
    pub replaced_hot_pixels: usize,
}

/// Normalized master flat ready for division: 1 / (flat / phase median).
pub struct FlatField {
    pub master: std::sync::Arc<MasterDark>,
    inverse: Vec<f32>,
    /// Median of each 2x2 phase [(0,0), (1,0), (0,1), (1,1)] in ADU.
    pub phase_medians: [f64; 4],
    pub defects: usize,
    /// The offset master was subtracted here (raw integer master flat).
    pub offset_subtracted: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct CalibrationSummary {
    pub formula: String,
    pub pedestal_adu: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bias: Option<DarkSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dark: Option<DarkSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flat: Option<DarkSummary>,
    /// Flat normalization: per 2x2 Bayer phase (also harmless for mono).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flat_phase_medians_adu: Option<[f64; 4]>,
    pub flat_defects_not_divided: usize,
    pub flat_offset_subtracted_here: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hot_pixels: Option<HotPixelSummary>,
    pub warnings: Vec<String>,
    pub written_to_disk: bool,
}

/// One light set's calibration: masters in RAM and how they are applied.
pub struct Calibration {
    pub bias: Option<std::sync::Arc<MasterDark>>,
    pub dark: Option<std::sync::Arc<MasterDark>>,
    pub flat: Option<FlatField>,
    /// Hot pixels replaced from their neighbours after calibration (ADR-163).
    pub hot_pixels: Option<std::sync::Arc<HotPixelMap>>,
    pub pedestal: f32,
    pub summary: CalibrationSummary,
}

impl std::fmt::Debug for Calibration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Calibration")
            .field("summary", &self.summary)
            .finish()
    }
}

impl Calibration {
    /// Prepare a light set's calibration. `flat_offset` (master dark flat,
    /// else master bias) is subtracted from a raw integer master flat that
    /// still carries the sensor offset. All masters must share dimensions.
    pub fn new(
        bias: Option<std::sync::Arc<MasterDark>>,
        dark: Option<std::sync::Arc<MasterDark>>,
        flat: Option<std::sync::Arc<MasterDark>>,
        flat_offset: Option<std::sync::Arc<MasterDark>>,
    ) -> Result<Self> {
        let sizes: Vec<_> = [&bias, &dark, &flat, &flat_offset]
            .into_iter()
            .flatten()
            .map(|m| (m.width, m.height))
            .collect();
        if sizes.windows(2).any(|w| w[0] != w[1]) {
            return Err(Error::Invalid(
                "Calibration masters have different dimensions.",
            ));
        }
        let mut warnings = vec![];
        for m in [&bias, &dark, &flat].into_iter().flatten() {
            warnings.extend(m.summary.warnings.iter().cloned());
        }
        let flat = match flat {
            None => None,
            Some(master) => {
                let subtract = !master.offset_removed && flat_offset.is_some();
                if !master.offset_removed && flat_offset.is_none() {
                    warnings.push(
                        "master flat still contains the sensor offset and no dark flat or bias is available".into(),
                    );
                }
                let (w, h) = (master.width, master.height);
                let mut values: Vec<f32> = match (&flat_offset, subtract) {
                    (Some(o), true) => master
                        .pixels
                        .iter()
                        .zip(&o.pixels)
                        .map(|(f, z)| f - z)
                        .collect(),
                    _ => master.pixels.clone(),
                };
                let mut phase_medians = [0.; 4];
                for (phase, median) in phase_medians.iter_mut().enumerate() {
                    let (px, py) = (phase % 2, phase / 2);
                    let mut samples: Vec<f32> = (py..h)
                        .step_by(2)
                        .flat_map(|y| values[y * w + px..(y + 1) * w].iter().step_by(2).copied())
                        .collect();
                    *median = median_f32(&mut samples);
                    if !(median.is_finite() && *median > 0.) {
                        return Err(Error::Invalid("The master flat has no positive signal."));
                    }
                }
                let mut defects = 0;
                for (i, v) in values.iter_mut().enumerate() {
                    let phase = (i % w) % 2 + 2 * ((i / w) % 2);
                    let normalized = *v / phase_medians[phase] as f32;
                    *v = if (FLAT_RANGE.0..=FLAT_RANGE.1).contains(&normalized) {
                        1. / normalized
                    } else {
                        defects += 1;
                        1.
                    };
                }
                Some(FlatField {
                    master,
                    inverse: values,
                    phase_medians,
                    defects,
                    offset_subtracted: subtract,
                })
            }
        };
        if flat.is_some() && bias.is_none() && dark.is_none() {
            warnings.push(
                "flat without bias or dark: the light's offset is divided by the flat".into(),
            );
        }
        let pedestal = bias
            .as_ref()
            .map(|b| b.pedestal)
            .or(dark.as_ref().map(|d| d.pedestal))
            .unwrap_or(0.);
        let offset = match (&bias, &dark) {
            (Some(_), Some(d)) if d.summary.exposure_seconds > 0. => {
                "light - bias - (dark - bias) * t_light/t_dark"
            }
            (Some(_), Some(_)) => "light - dark (dark exposure unknown: not scaled)",
            (None, Some(_)) => "light - dark",
            (Some(_), None) => "light - bias",
            (None, None) => "light",
        };
        let formula = if flat.is_some() {
            format!("({offset}) / normalized flat + pedestal")
        } else if bias.is_some() || dark.is_some() {
            format!("{offset} + pedestal")
        } else {
            "light".into()
        };
        let summary = CalibrationSummary {
            formula,
            pedestal_adu: f64::from(pedestal),
            bias: bias.as_ref().map(|m| m.summary.clone()),
            dark: dark.as_ref().map(|m| m.summary.clone()),
            flat: flat.as_ref().map(|f| f.master.summary.clone()),
            flat_phase_medians_adu: flat.as_ref().map(|f| f.phase_medians),
            flat_defects_not_divided: flat.as_ref().map_or(0, |f| f.defects),
            flat_offset_subtracted_here: flat.as_ref().is_some_and(|f| f.offset_subtracted),
            hot_pixels: None,
            warnings,
            written_to_disk: false,
        };
        Ok(Self {
            bias,
            dark,
            flat,
            hot_pixels: None,
            pedestal,
            summary,
        })
    }

    /// Add a hot pixel map: its pixels are replaced from their same-colour
    /// neighbours after everything else (ADR-163). It must match the masters'
    /// dimensions. Also valid for a calibration made of nothing else.
    pub fn with_hot_pixels(mut self, map: std::sync::Arc<HotPixelMap>) -> Result<Self> {
        if self
            .dimensions()
            .is_some_and(|d| d != (map.width, map.height))
        {
            return Err(Error::Invalid(
                "The hot pixel map and the calibration masters have different dimensions.",
            ));
        }
        self.summary.formula = if self.summary.formula == "light" {
            "light with hot pixel map".to_owned()
        } else {
            format!("{} + hot pixel map", self.summary.formula)
        };
        self.summary.hot_pixels = Some(map.summary.clone());
        self.hot_pixels = Some(map);
        Ok(self)
    }

    fn dimensions(&self) -> Option<(usize, usize)> {
        self.hot_pixels
            .as_ref()
            .map(|m| (m.width, m.height))
            .or_else(|| self.master_dimensions())
    }

    fn master_dimensions(&self) -> Option<(usize, usize)> {
        self.bias
            .as_ref()
            .or(self.dark.as_ref())
            .map(|m| (m.width, m.height))
            .or(self
                .flat
                .as_ref()
                .map(|f| (f.master.width, f.master.height)))
    }

    /// Why a light cannot be calibrated at all; `None` when it can.
    pub fn incompatibility(&self, light: &FrameInfo) -> Option<&'static str> {
        if light.mosaic == fulgur_core::Mosaic::Rgb {
            // Bias, darks and flats act on sensor samples, before debayering.
            return self
                .dimensions()
                .map(|_| "debayered_frame_cannot_be_calibrated");
        }
        self.dimensions()
            .is_some_and(|d| d != (light.width, light.height))
            .then_some("calibration_size_mismatch")
    }

    /// Thermal scale for a light, when a bias makes scaling possible.
    fn dark_scale(&self, exposure: f64) -> Option<f32> {
        let dark = self.dark.as_ref()?;
        self.bias.as_ref()?;
        let t = dark.summary.exposure_seconds;
        (t > 0. && (exposure / t - 1.).abs() > EXPOSURE_TOLERANCE).then_some((exposure / t) as f32)
    }

    /// Owner-facing per-light warnings (ADR-080: never fatal).
    pub fn light_warnings(&self, light: &FrameInfo) -> Vec<&'static str> {
        let Some(dark) = &self.dark else {
            return vec![];
        };
        let mut warnings = vec![];
        let t = dark.summary.exposure_seconds;
        if t <= 0. {
            warnings.push("dark_exposure_unknown");
        } else if (light.exposure_seconds / t - 1.).abs() > EXPOSURE_TOLERANCE {
            warnings.push(if self.bias.is_some() {
                "dark_scaled_by_exposure"
            } else {
                "dark_exposure_differs"
            });
        }
        if light
            .gain
            .zip(dark.summary.gain)
            .is_some_and(|(a, b)| (a - b).abs() > 1e-6)
        {
            warnings.push("dark_gain_differs");
        }
        warnings
    }

    /// Calibrate a full frame or a phase-preserving ROI at `origin` in place.
    /// Master-dark hot pixels whose light holds less than half of the dark's
    /// thermal signal are replaced by the median of their calibrated
    /// same-phase neighbours, as a cosmetic correction would.
    pub fn apply(&self, frame: &mut RawFrame, origin: [usize; 2]) -> Result<CalibrationStats> {
        let (w, h) = (frame.header.info.width, frame.header.info.height);
        let Some((width, height)) = self.dimensions() else {
            return Ok(CalibrationStats::default());
        };
        if origin[0] + w > width || origin[1] + h > height || frame.pixels.len() != w * h {
            return Err(Error::Invalid(
                "Light region lies outside the calibration masters.",
            ));
        }
        let scale = self.dark_scale(frame.header.info.exposure_seconds);
        let (bias, dark) = (self.bias.as_deref(), self.dark.as_deref());
        let inverse = self.flat.as_ref().map(|f| f.inverse.as_slice());
        // A sensor below 16 bits clips earlier, and stays at its own limit.
        let level = frame.header.info.saturation_adu;
        let saturated_from = level.map_or(SATURATED_ADU, |l| l * (SATURATED_ADU / 65535.));
        let saturated_value = level.unwrap_or(65535.);
        let mut defects = vec![];
        for (y, row) in frame.pixels.chunks_exact_mut(w).enumerate() {
            let start = (origin[1] + y) * width + origin[0];
            for (x, value) in row.iter_mut().enumerate() {
                let i = start + x;
                let raw = *value;
                // Same arithmetic as ADR-076 when only a dark is present.
                let mut v = match (dark, bias, scale) {
                    (Some(d), Some(b), Some(k)) => {
                        raw - (b.pixels[i] + (d.pixels[i] - b.pixels[i]) * k)
                    }
                    (Some(d), _, _) => raw - d.pixels[i],
                    (None, Some(b), _) => raw - b.pixels[i],
                    (None, None, _) => raw,
                };
                let offset_free = v;
                if let Some(inverse) = inverse {
                    v *= inverse[i];
                }
                if offset_free < 0. && raw < saturated_from {
                    let thermal = match (dark, bias, scale) {
                        (Some(d), Some(b), Some(k)) => (d.pixels[i] - b.pixels[i]) * k,
                        (Some(d), Some(b), None) => d.pixels[i] - b.pixels[i],
                        (Some(d), None, _) => d.pixels[i] - d.pedestal,
                        _ => 0.,
                    };
                    // The light holds less than half the dark's thermal signal
                    // (the hot pixel is absent or unstable in this light), or the
                    // over-subtraction would leave a negative sample.
                    if thermal > HOT_DARK_ADU
                        && (offset_free < -0.5 * thermal || offset_free + self.pedestal < 0.)
                    {
                        defects.push(y * w + x);
                    }
                }
                v += self.pedestal;
                if raw >= saturated_from {
                    v = v.max(saturated_value);
                }
                *value = v;
            }
        }
        // Same-phase neighbours (two pixels away) that are not defects themselves.
        let mut repaired = 0;
        let mut replacement = Vec::with_capacity(defects.len());
        for &j in &defects {
            let (x, y) = ((j % w) as isize, (j / w) as isize);
            let mut near: Vec<f32> = [
                (-2, 0),
                (2, 0),
                (0, -2),
                (0, 2),
                (-2, -2),
                (2, -2),
                (-2, 2),
                (2, 2),
            ]
            .iter()
            .map(|(dx, dy)| (x + dx, y + dy))
            .filter(|&(nx, ny)| nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h)
            .map(|(nx, ny)| ny as usize * w + nx as usize)
            .filter(|n| defects.binary_search(n).is_err())
            .map(|n| frame.pixels[n])
            .collect();
            replacement.push((!near.is_empty()).then(|| median_f32(&mut near) as f32));
        }
        for (&j, value) in defects.iter().zip(replacement) {
            if let Some(v) = value {
                frame.pixels[j] = v;
                repaired += 1;
            }
        }
        let replaced_hot_pixels = self
            .hot_pixels
            .as_ref()
            .map_or(0, |map| map.repair(frame, origin));
        Ok(CalibrationStats {
            negative_samples: frame.pixels.iter().filter(|v| **v < 0.).count(),
            repaired_dark_defects: repaired,
            replaced_hot_pixels,
        })
    }
}

mod hot_pixels;
pub use hot_pixels::{
    DEFAULT_PERCENT as HOT_PIXEL_DEFAULT_PERCENT, HotPixelMap, HotPixelSummary,
    MAX_PERCENT as HOT_PIXEL_MAX_PERCENT, is_map_file as is_hot_pixel_map,
};

#[cfg(test)]
mod tests;
