//! Bounded, parallel M2 inspection and exposure-aware provisional selection.

use crate::stars::{Star, analyze_field, channel_sky, median, plane_level};
use fulgur_core::fits::{FrameInfo, ReadBuffers, read_raw_buffered};
use fulgur_core::{Error, LayoutChoice, Mosaic, Result, check_cancelled};
use rayon::prelude::*;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

#[derive(Clone, Debug, Serialize)]
pub struct InspectConfig {
    pub hfr_sigma: f64,
    pub min_star_fraction: f64,
    pub max_eccentricity: f64,
    pub min_comparison_frames: usize,
    pub minimum_stars: usize,
    pub hfr_scatter_floor_px: f64,
    /// Also keep the bright-star list sky registration solves with
    /// (`QualityFrame::sky_stars`); off for the star registration.
    pub sky_stars: bool,
}

impl Default for InspectConfig {
    fn default() -> Self {
        Self {
            hfr_sigma: 2.5,
            min_star_fraction: 0.45,
            max_eccentricity: 0.60,
            min_comparison_frames: 5,
            minimum_stars: 10,
            hfr_scatter_floor_px: 0.05,
            sky_stars: false,
        }
    }
}

impl InspectConfig {
    pub fn validate(&self) -> Result<()> {
        if !self.hfr_sigma.is_finite()
            || self.hfr_sigma <= 0.0
            || !(0.0..=1.0).contains(&self.min_star_fraction)
            || !(0.0..=1.0).contains(&self.max_eccentricity)
            || self.min_comparison_frames < 3
            || self.minimum_stars < 3
            || !self.hfr_scatter_floor_px.is_finite()
            || self.hfr_scatter_floor_px < 0.0
        {
            return Err(Error::Invalid("Invalid inspection thresholds."));
        }
        Ok(())
    }
}

/// One light set: its files/folders, its declared sensor layout and a label.
/// Quality limits are evaluated within a set; sets may differ in filter,
/// gain and exposure, but share the target and sensor dimensions.
#[derive(Clone)]
pub struct InputSet {
    pub label: String,
    pub inputs: Vec<PathBuf>,
    pub layout: LayoutChoice,
    /// Exposure entered by the owner for every frame of the set (ADR-084).
    pub exposure: Option<f64>,
    /// The owner confirmed the values reconstructed for missing header
    /// cards (ADR-084); without it such frames are rejected.
    pub accept_derived: bool,
    /// In-RAM calibration (bias, dark, flat) applied before star measurement
    /// and integration.
    pub calibration: Option<std::sync::Arc<crate::calibration::Calibration>>,
}

impl std::fmt::Debug for InputSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputSet")
            .field("label", &self.label)
            .field("inputs", &self.inputs)
            .field("layout", &self.layout)
            .field("exposure", &self.exposure)
            .field("accept_derived", &self.accept_derived)
            .field(
                "calibration",
                &self.calibration.as_ref().map(|c| &c.summary.formula),
            )
            .finish()
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct InspectedSet {
    pub label: String,
    pub layout: LayoutChoice,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exposure_override_seconds: Option<f64>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub derived_metadata_confirmed: bool,
    pub frame_count: usize,
    pub accepted_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calibration: Option<crate::calibration::CalibrationSummary>,
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

#[derive(Clone, Debug, Serialize)]
pub struct QualityFrame {
    pub file_name: String,
    /// Index into `InspectReport::light_sets`.
    pub light_set: usize,
    pub frame: Option<FrameInfo>,
    pub background_adu: Option<f64>,
    /// Sky level per colour channel (R, G, B) of a colour sensor (ADR-102).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_background_adu: Option<[f64; 3]>,
    /// Noise of the averaged green detection plane, in source ADU.
    pub noise_adu: Option<f64>,
    pub noise_adu_per_second: Option<f64>,
    pub detected_peaks: usize,
    pub star_count: usize,
    pub measured_star_count: usize,
    /// The set's master dark was subtracted before measurement (and must be
    /// subtracted again in integration).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub calibrated: bool,
    /// Calibrated samples below zero (should stay near zero; see ADR-082).
    #[serde(skip_serializing_if = "is_zero")]
    pub calibration_negative_samples: usize,
    /// Over-subtracted master-dark hot pixels repaired from their neighbours.
    #[serde(skip_serializing_if = "is_zero")]
    pub calibration_repaired_pixels: usize,
    /// Calibrated despite another exposure or gain than the dark (warning).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub calibration_warnings: Vec<String>,
    /// Hot-pixel samples suppressed in the measurement copy (sources unchanged).
    pub suppressed_hot_pixels: usize,
    /// Detections refused because their profile is a single-sample defect.
    pub refused_defect_detections: usize,
    pub catalog_capped: bool,
    pub hfr_px: Option<f64>,
    pub moment_fwhm_px: Option<f64>,
    pub eccentricity: Option<f64>,
    pub hfr_rejection_limit_px: Option<f64>,
    pub star_count_rejection_limit: Option<f64>,
    pub exposure_group_size: usize,
    pub accepted: bool,
    pub rejection_reasons: Vec<String>,
    pub provisional_weight: f64,
    pub wall_seconds: f64,
    pub read_seconds: f64,
    pub measurement_seconds: f64,
    /// Registration catalog: the ordinary stars, or the flux-ranked lists of
    /// a dense field, where saturated stars are positions with no flux.
    pub stars: Vec<Star>,
    /// Bright stars for sky registration (x, y, fwhm, eccentricity),
    /// brightest first, saturated ones included; empty unless requested.
    #[serde(skip)]
    pub sky_stars: Vec<crate::sky::detect::SkyStar>,
}

impl QualityFrame {
    pub(crate) fn empty(file_name: String) -> Self {
        Self {
            file_name,
            light_set: 0,
            frame: None,
            background_adu: None,
            channel_background_adu: None,
            noise_adu: None,
            noise_adu_per_second: None,
            detected_peaks: 0,
            star_count: 0,
            measured_star_count: 0,
            calibrated: false,
            calibration_negative_samples: 0,
            calibration_repaired_pixels: 0,
            calibration_warnings: Vec::new(),
            suppressed_hot_pixels: 0,
            refused_defect_detections: 0,
            catalog_capped: false,
            hfr_px: None,
            moment_fwhm_px: None,
            eccentricity: None,
            hfr_rejection_limit_px: None,
            star_count_rejection_limit: None,
            exposure_group_size: 0,
            accepted: false,
            rejection_reasons: Vec::new(),
            provisional_weight: 0.0,
            wall_seconds: 0.0,
            read_seconds: 0.0,
            measurement_seconds: 0.0,
            stars: Vec::new(),
            sky_stars: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct InspectReport {
    pub schema_version: u32,
    pub command: &'static str,
    pub frame_count: usize,
    pub accepted_count: usize,
    pub rejected_count: usize,
    pub total_exposure_seconds: f64,
    pub accepted_exposure_seconds: f64,
    pub wall_seconds: f64,
    pub frames_per_second: f64,
    pub peak_rss_bytes: Option<u64>,
    pub max_concurrent_frames: usize,
    pub cpu_threads: usize,
    pub config: InspectConfig,
    pub weights_status: &'static str,
    pub photometry_status: &'static str,
    pub coordinate_system: &'static str,
    pub noise_units: &'static str,
    pub registration_status: &'static str,
    pub timing_scope: &'static str,
    pub bias_subtracted: bool,
    pub intermediate_images_written: usize,
    /// Distinct OBJECT names; more than one is a warning for the owner.
    pub target_names: Vec<String>,
    pub light_sets: Vec<InspectedSet>,
    pub frames: Vec<QualityFrame>,
}

impl InspectReport {
    /// Owner warning when the frames carry several target names.
    pub fn target_warning(&self) -> Option<String> {
        (self.target_names.len() > 1).then(|| {
            format!(
                "Frames name different targets: {}. They are stacked as one target where their stars match; frames of another sky field fail registration",
                self.target_names.join(" + ")
            )
        })
    }
}

/// Frame-sized memory one worker reuses for the whole inspection.
#[derive(Default)]
struct WorkerMemory {
    read: ReadBuffers,
    suppressed: Vec<f32>,
}

fn one(
    path: &Path,
    root: &Path,
    set: &InputSet,
    memory: &mut WorkerMemory,
    sky_stars: bool,
    cancel: &AtomicBool,
) -> Result<QualityFrame> {
    let calibration = set.calibration.as_deref();
    let start = Instant::now();
    let name = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let mut result = QualityFrame::empty(name);
    let decoded = read_raw_buffered(path, &mut memory.read);
    result.read_seconds = start.elapsed().as_secs_f64();
    match decoded {
        Ok(mut frame) => {
            frame.header.info.apply_layout(set.layout);
            frame.header.info.apply_exposure(set.exposure);
            // Before calibration, which spreads the clipped samples apart.
            frame.header.info.saturation_adu =
                fulgur_core::fits::saturation_level(&frame.pixels, frame.header.info.bitpix);
            if frame.header.info.exposure_seconds <= 0.0 {
                // No EXPTIME, EXPOSURE or file-name exposure (ADR-084).
                result.rejection_reasons.push("missing_exposure".to_owned());
            } else if frame.header.info.needs_confirmation() && !set.accept_derived {
                // Reconstructed exposure or colour layout is never used unconfirmed.
                result
                    .rejection_reasons
                    .push("unconfirmed_derived_metadata".to_owned());
            } else if frame.header.info.mosaic == Mosaic::Unknown {
                // Never guess colour versus mono: the owner must declare it.
                result
                    .rejection_reasons
                    .push("unknown_sensor_layout".to_owned());
            } else if frame.header.info.width < 64 || frame.header.info.height < 64 {
                result
                    .rejection_reasons
                    .push("unsupported_inspection_dimensions".to_owned());
            } else if let Some(reason) =
                calibration.and_then(|c| c.incompatibility(&frame.header.info))
            {
                // A light the assigned dark cannot calibrate is never stacked raw.
                result.rejection_reasons.push(reason.to_owned());
            } else {
                if let Some(calibration) = calibration {
                    result.calibration_warnings = calibration
                        .light_warnings(&frame.header.info)
                        .into_iter()
                        .map(str::to_owned)
                        .collect();
                    let stats = calibration.apply(&mut frame, [0, 0])?;
                    result.calibration_negative_samples = stats.negative_samples;
                    result.calibration_repaired_pixels = stats.repaired_dark_defects;
                    result.calibrated = true;
                }
                let metrics = analyze_field(&frame, &mut memory.suppressed, cancel)?;
                if sky_stars {
                    result.sky_stars = bright_star_list(&frame);
                }
                result.background_adu = Some(metrics.background);
                result.channel_background_adu = frame
                    .header
                    .info
                    .bayer_pattern
                    .filter(|_| frame.header.info.mosaic == Mosaic::Bayer)
                    .map(|pattern| {
                        channel_sky(
                            &frame.pixels,
                            frame.header.info.width,
                            frame.header.info.height,
                            pattern,
                            metrics.background,
                        )
                    });
                if frame.header.info.mosaic == Mosaic::Rgb {
                    let (width, height) = (frame.header.info.width, frame.header.info.height);
                    let level = |channel| -> Result<f64> {
                        let plane =
                            fulgur_core::fits::read_channel(&frame.header, channel, cancel)?;
                        Ok(plane_level(&plane, width, height))
                    };
                    result.channel_background_adu =
                        Some([level(0)?, metrics.background, level(2)?]);
                }
                result.noise_adu = Some(metrics.noise);
                result.noise_adu_per_second =
                    Some(metrics.noise / frame.header.info.exposure_seconds);
                result.detected_peaks = metrics.detected_peaks;
                result.star_count = metrics.detected_stars;
                result.measured_star_count = metrics.stars.len();
                result.suppressed_hot_pixels = metrics.hot_pixels;
                result.refused_defect_detections = metrics.defect_detections;
                result.catalog_capped = result.measured_star_count == 250;
                if !metrics.stars.is_empty() {
                    result.hfr_px = Some(median(
                        &mut metrics.stars.iter().map(|s| s.hfr_px).collect::<Vec<_>>(),
                    ));
                    result.moment_fwhm_px = Some(median(
                        &mut metrics
                            .stars
                            .iter()
                            .map(|s| s.moment_fwhm_px)
                            .collect::<Vec<_>>(),
                    ));
                    result.eccentricity = Some(median(
                        &mut metrics
                            .stars
                            .iter()
                            .map(|s| s.eccentricity)
                            .collect::<Vec<_>>(),
                    ));
                }
                // Quality above comes from the ordinary catalog; a dense field
                // registers on its flux-ranked catalog (ADR-108).
                result.stars = if metrics.dense_catalog.is_empty() {
                    metrics.stars
                } else {
                    metrics.dense_catalog
                };
            }
            result.frame = Some(frame.header.info);
            memory.read.give_back(frame.pixels);
        }
        Err(Error::Cancelled) => return Err(Error::Cancelled),
        Err(_) => result
            .rejection_reasons
            .push("invalid_or_unsupported_fits".to_owned()),
    }
    result.wall_seconds = start.elapsed().as_secs_f64();
    result.measurement_seconds = result.wall_seconds - result.read_seconds;
    Ok(result)
}

/// The bright-star list sky registration solves with, from a frame's
/// calibrated pixels as inspection measures them.
fn bright_star_list(frame: &fulgur_core::RawFrame) -> Vec<crate::sky::detect::SkyStar> {
    crate::sky::detect::bright_stars(
        &frame.pixels,
        frame.header.info.width,
        frame.header.info.height,
        frame.header.info.mosaic == Mosaic::Bayer,
    )
    .0
}

/// The bright-star list of one measured frame, read and calibrated again
/// exactly as inspection read it (ADR-154, ADR-155).
pub fn sky_star_list_of(
    frame: &QualityFrame,
    root: &Path,
    set: &InputSet,
) -> Result<Vec<crate::sky::detect::SkyStar>> {
    sky_star_list(frame, root, set, &mut ReadBuffers::default())
}

fn sky_star_list(
    frame: &QualityFrame,
    root: &Path,
    set: &InputSet,
    read: &mut ReadBuffers,
) -> Result<Vec<crate::sky::detect::SkyStar>> {
    let mut raw = read_raw_buffered(&root.join(&frame.file_name), read)?;
    raw.header.info.apply_layout(set.layout);
    raw.header.info.apply_exposure(set.exposure);
    raw.header.info.saturation_adu =
        fulgur_core::fits::saturation_level(&raw.pixels, raw.header.info.bitpix);
    if let Some(calibration) = set.calibration.as_deref() {
        calibration.apply(&mut raw, [0, 0])?;
    }
    let list = bright_star_list(&raw);
    read.give_back(raw.pixels);
    Ok(list)
}

/// Adds the bright-star lists of sky registration to an inspection made
/// without them (ADR-154): every measured frame is read and calibrated again
/// exactly as inspection did, and only its list is computed; measurements,
/// classification and the owner's choices stay as they are. The lists equal
/// those of an inspection with `InspectConfig::sky_stars`.
pub fn add_sky_stars(
    report: &mut InspectReport,
    root: &Path,
    sets: &[InputSet],
    cancel: &AtomicBool,
    progress: Option<&(dyn Fn(usize, usize) + Sync)>,
) -> Result<()> {
    if sets.len() != report.light_sets.len() {
        return Err(Error::Invalid("Light sets differ from the inspection."));
    }
    // Frames inspection measured; others never get a list.
    let measured: Vec<usize> = (0..report.frames.len())
        .filter(|&i| report.frames[i].background_adu.is_some())
        .collect();
    let workers = rayon::current_num_threads().min(16);
    let next = AtomicUsize::new(0);
    let failed = AtomicBool::new(false);
    let completed = Mutex::new(0usize);
    if let Some(progress) = progress {
        progress(0, measured.len());
    }
    let frames = &report.frames;
    let mut lists: Vec<(usize, Result<Vec<crate::sky::detect::SkyStar>>)> = (0..workers)
        .into_par_iter()
        .flat_map_iter(|_| {
            let mut read = ReadBuffers::default();
            let mut done = Vec::new();
            while !failed.load(Ordering::Relaxed) {
                let Some(&index) = measured.get(next.fetch_add(1, Ordering::Relaxed)) else {
                    break;
                };
                let frame = &frames[index];
                let list = check_cancelled(cancel)
                    .and_then(|()| sky_star_list(frame, root, &sets[frame.light_set], &mut read));
                if list.is_err() {
                    failed.store(true, Ordering::Relaxed);
                } else if let Some(progress) = progress {
                    let mut count = completed.lock().unwrap();
                    *count += 1;
                    progress(*count, measured.len());
                }
                done.push((index, list));
            }
            done
        })
        .collect();
    lists.sort_unstable_by_key(|(index, _)| *index);
    for (index, list) in lists {
        report.frames[index].sky_stars = list?;
    }
    Ok(())
}

fn compatible(a: &FrameInfo, b: &FrameInfo) -> bool {
    a.width == b.width
        && a.height == b.height
        && a.mosaic == b.mosaic
        && a.bayer_pattern == b.bayer_pattern
        && a.bzero == b.bzero
        && a.gain == b.gain
        && a.filter == b.filter
}

/// Frames one light set keeps together: mutually compatible as inspection
/// judges them (`compatible`: sensor size, colour layout, BZERO, gain and
/// filter). A set holding several such groups keeps only the largest
/// (ADR-139).
#[derive(Clone, Debug, PartialEq)]
pub struct FrameGroup {
    pub files: Vec<PathBuf>,
    pub width: usize,
    pub height: usize,
    /// Colour as the headers declare it, whatever layout the owner declared
    /// for the set: a mono and a colour camera never form one group.
    pub colour: bool,
    /// Bayer pattern of the headers (before the owner's layout), if any.
    pub header_pattern: Option<fulgur_core::BayerPattern>,
    pub gain: Option<f64>,
    pub filter: Option<String>,
}

impl FrameGroup {
    /// "6248×4176 colour · gain 100 · No filter".
    pub fn label(&self) -> String {
        let mut parts = vec![format!(
            "{}×{} {}",
            self.width,
            self.height,
            if self.colour { "colour" } else { "mono" }
        )];
        if let Some(gain) = self.gain {
            parts.push(format!("gain {}", (gain * 1000.).round() / 1000.));
        }
        if let Some(filter) = self
            .filter
            .as_deref()
            .map(str::trim)
            .filter(|f| !f.is_empty())
        {
            parts.push(filter.to_string());
        }
        parts.join(" · ")
    }
}

/// The groups of `files` under the set's `layout`, largest first. Files whose
/// header cannot be read stay with the largest group, where inspection
/// reports them.
pub fn compatibility_groups(files: &[PathBuf], layout: LayoutChoice) -> Vec<FrameGroup> {
    let infos: Vec<Option<(FrameInfo, Option<fulgur_core::BayerPattern>)>> = files
        .par_iter()
        .map(|path| {
            fulgur_core::fits::read_header(path).ok().map(|header| {
                let mut info = header.info;
                // Compared before the declared layout hides it: a mono and a
                // colour camera look alike once both are declared mono.
                let header_pattern = info.bayer_pattern;
                info.apply_layout(layout);
                (info, header_pattern)
            })
        })
        .collect();
    let mut groups: Vec<(FrameInfo, FrameGroup)> = vec![];
    let mut unreadable = vec![];
    for (path, info) in files.iter().zip(infos) {
        let Some((info, header_pattern)) = info else {
            unreadable.push(path.clone());
            continue;
        };
        match groups.iter_mut().find(|(known, group)| {
            compatible(known, &info) && group.header_pattern == header_pattern
        }) {
            Some((_, group)) => group.files.push(path.clone()),
            None => groups.push((
                info.clone(),
                FrameGroup {
                    files: vec![path.clone()],
                    width: info.width,
                    height: info.height,
                    colour: header_pattern.is_some() || info.mosaic == Mosaic::Bayer,
                    header_pattern,
                    gain: info.gain,
                    filter: info.filter.clone(),
                },
            )),
        }
    }
    let mut groups: Vec<FrameGroup> = groups.into_iter().map(|(_, g)| g).collect();
    // Stable: equal sizes keep the order in which their first file came.
    groups.sort_by_key(|g| std::cmp::Reverse(g.files.len()));
    match groups.first_mut() {
        Some(first) => first.files.extend(unreadable),
        None if !unreadable.is_empty() => groups.push(FrameGroup {
            files: unreadable,
            width: 0,
            height: 0,
            colour: false,
            header_pattern: None,
            gain: None,
            filter: None,
        }),
        None => {}
    }
    groups
}

/// Distinct OBJECT names of the readable frames, in frame order. A name is
/// only a label: several names warn, and star matching decides what stacks.
fn target_names(frames: &[QualityFrame]) -> Vec<String> {
    let mut names: Vec<String> = vec![];
    for name in frames
        .iter()
        .filter_map(|f| f.frame.as_ref()?.object.as_deref())
        .map(str::trim)
    {
        if !names.iter().any(|known| known == name) {
            names.push(name.to_owned());
        }
    }
    names
}

#[cfg(test)]
pub(crate) fn test_frame_info(width: usize, height: usize) -> FrameInfo {
    FrameInfo {
        width,
        height,
        bitpix: 16,
        bzero: 32768.,
        bscale: 1.,
        bayer_pattern: None,
        mosaic: Mosaic::Mono,
        mosaic_source: "header",
        exposure_seconds: 10.,
        object: None,
        filter: None,
        date_obs: None,
        gain: None,
        bias_informational: None,
        creator: None,
        saturation_adu: None,
        derived: vec![],
        rows_mirrored: false,
    }
}

#[cfg(test)]
fn classify(frames: &mut [QualityFrame], config: &InspectConfig) {
    classify_set(frames, config);
    finalize(frames);
}

/// Classify each set on its own population. A single set is the whole
/// population, so its arithmetic and ordering are unchanged.
fn classify_sets(frames: &mut [QualityFrame], set_count: usize, config: &InspectConfig) {
    for set in 0..set_count {
        let members: Vec<usize> = (0..frames.len())
            .filter(|&i| frames[i].light_set == set)
            .collect();
        let mut subset: Vec<QualityFrame> = members
            .iter()
            .map(|&i| std::mem::replace(&mut frames[i], QualityFrame::empty(String::new())))
            .collect();
        classify_set(&mut subset, config);
        for (&i, frame) in members.iter().zip(subset) {
            frames[i] = frame;
        }
    }
    finalize(frames);
}

fn classify_set(frames: &mut [QualityFrame], config: &InspectConfig) {
    let mut anchor = None;
    let mut largest_group = 0;
    for info in frames.iter().filter_map(|f| f.frame.as_ref()) {
        let count = frames
            .iter()
            .filter_map(|f| f.frame.as_ref())
            .filter(|other| compatible(info, other))
            .count();
        if count > largest_group {
            largest_group = count;
            anchor = Some(info.clone());
        }
    }
    for row in frames.iter_mut() {
        if row
            .frame
            .as_ref()
            .zip(anchor.as_ref())
            .is_some_and(|(info, anchor)| !compatible(info, anchor))
        {
            row.rejection_reasons
                .push("incompatible_target_filter_or_sensor".to_owned());
        }
        if row.measured_star_count < config.minimum_stars {
            row.rejection_reasons.push("insufficient_stars".to_owned());
        }
        if row.noise_adu.is_none_or(|s| !s.is_finite() || s <= 0.0)
            || row
                .noise_adu_per_second
                .is_none_or(|s| !s.is_finite() || s <= 0.0)
        {
            row.rejection_reasons.push("unusable_noise".to_owned());
        }
        if row
            .eccentricity
            .is_some_and(|e| e > config.max_eccentricity)
        {
            row.rejection_reasons
                .push("excessive_eccentricity".to_owned());
        }
    }
    // Exposure groups protect shorter exposures from count-based rejection.
    // Relative gates use initially measurable, compatible frames, including
    // eccentric ones, so a genuine quality spike does not alter its own group.
    let eligible: Vec<usize> = frames
        .iter()
        .enumerate()
        .filter_map(|(i, f)| {
            (f.hfr_px.is_some()
                && f.frame.is_some()
                && !f.rejection_reasons.iter().any(|s| {
                    s == "incompatible_target_filter_or_sensor"
                        || s == "unusable_noise"
                        || s == "invalid_or_unsupported_fits"
                }))
            .then_some(i)
        })
        .collect();
    let hfr_limit = if eligible.len() >= config.min_comparison_frames {
        let mut sizes: Vec<_> = eligible
            .iter()
            .map(|&j| frames[j].hfr_px.unwrap())
            .collect();
        let center = median(&mut sizes);
        let mad = median(&mut sizes.iter().map(|h| (h - center).abs()).collect::<Vec<_>>());
        Some(center + config.hfr_sigma * (1.4826 * mad).max(config.hfr_scatter_floor_px))
    } else {
        None
    };
    for &i in &eligible {
        frames[i].hfr_rejection_limit_px = hfr_limit;
        if hfr_limit.is_some_and(|limit| frames[i].hfr_px.unwrap() > limit) {
            frames[i].rejection_reasons.push("excessive_hfr".to_owned());
        }
        let exposure = frames[i].frame.as_ref().unwrap().exposure_seconds;
        let group: Vec<_> = eligible
            .iter()
            .copied()
            .filter(|&j| {
                let other = frames[j].frame.as_ref().unwrap().exposure_seconds;
                (other / exposure - 1.0).abs() <= 1e-6
            })
            .collect();
        frames[i].exposure_group_size = group.len();
        if group.len() < config.min_comparison_frames {
            continue;
        }
        let count_limit = config.min_star_fraction
            * median(
                &mut group
                    .iter()
                    .map(|&j| frames[j].star_count as f64)
                    .collect::<Vec<_>>(),
            );
        frames[i].star_count_rejection_limit = Some(count_limit);
        if (frames[i].star_count as f64) < count_limit {
            frames[i]
                .rejection_reasons
                .push("low_star_count_in_exposure_group".to_owned());
        }
    }
}

/// Acceptance and provisional weights, normalized across all light sets.
fn finalize(frames: &mut [QualityFrame]) {
    let mut maximum = 0.0_f64;
    for frame in frames.iter_mut() {
        frame.accepted = frame.rejection_reasons.is_empty();
        if frame.accepted {
            let noise = frame.noise_adu_per_second.unwrap();
            let hfr = frame.hfr_px.unwrap_or(0.0);
            let weight = 1.0 / (noise * noise * hfr * hfr);
            if !weight.is_finite() || weight <= 0.0 {
                frame.accepted = false;
                frame
                    .rejection_reasons
                    .push("nonfinite_provisional_weight".to_owned());
            } else {
                frame.provisional_weight = weight;
                maximum = maximum.max(weight);
            }
        }
    }
    if maximum > 0.0 {
        for frame in frames {
            frame.provisional_weight /= maximum;
        }
    }
}

pub fn inspect(input: &Path, config: InspectConfig, cancel: &AtomicBool) -> Result<InspectReport> {
    config.validate()?;
    let start = Instant::now();
    let mut files: Vec<PathBuf> = Vec::new();
    crate::survey::collect_folder_inputs(input, &mut files)?;
    files.sort();
    if files.is_empty() {
        return Err(Error::Invalid("No FITS or XISF input files found."));
    }
    let root = if input.is_dir() {
        input
    } else {
        input.parent().unwrap_or(Path::new("."))
    };
    let set = single_set(&[input.to_path_buf()], LayoutChoice::Auto);
    let files = files.into_iter().map(|p| (p, 0)).collect();
    inspect_paths(files, &[set], root, config, cancel, start, None)
}

/// Single-set convenience used by the CLI: every input shares one population.
pub fn single_set(inputs: &[PathBuf], layout: LayoutChoice) -> InputSet {
    InputSet {
        label: "inputs".into(),
        inputs: inputs.to_vec(),
        layout,
        exposure: None,
        accept_derived: false,
        calibration: None,
    }
}

/// Values reconstructed for missing header cards, grouped per light set,
/// field, value and source, for the owner to confirm or correct (ADR-084).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DerivedGroup {
    pub light_set: usize,
    pub field: &'static str,
    pub value: String,
    pub source: String,
    pub files: usize,
    pub needs_confirmation: bool,
}

/// Header-only scan of every light set (no pixels are read). The set's
/// layout choice and exposure entry replace the values they correct.
/// Lights with no exposure source at all appear as value "missing".
pub fn derived_metadata(sets: &[InputSet]) -> Result<Vec<DerivedGroup>> {
    let mut files = vec![];
    for (index, set) in sets.iter().enumerate() {
        let mut paths = vec![];
        for input in &set.inputs {
            crate::survey::collect_folder_inputs(input, &mut paths)?;
        }
        paths.sort();
        paths.dedup();
        files.extend(paths.into_iter().map(|p| (index, p)));
    }
    let rows: Vec<_> = files
        .par_iter()
        .filter_map(|(index, path)| {
            let mut info = fulgur_core::fits::read_header(path).ok()?.info;
            info.apply_layout(sets[*index].layout);
            info.apply_exposure(sets[*index].exposure);
            let mut derived = std::mem::take(&mut info.derived);
            if info.exposure_seconds <= 0.0 {
                derived.push(fulgur_core::derive::DerivedValue {
                    field: fulgur_core::derive::EXPOSURE,
                    value: "missing".into(),
                    source:
                        "no EXPTIME or EXPOSURE card and no exposure in the file name; enter it"
                            .into(),
                    needs_confirmation: true,
                });
            }
            Some((*index, derived))
        })
        .collect();
    let mut groups: Vec<DerivedGroup> = vec![];
    for (light_set, derived) in rows {
        for d in derived {
            match groups.iter_mut().find(|g| {
                g.light_set == light_set
                    && g.field == d.field
                    && g.value == d.value
                    && g.source == d.source
            }) {
                Some(group) => group.files += 1,
                None => groups.push(DerivedGroup {
                    light_set,
                    field: d.field,
                    value: d.value,
                    source: d.source,
                    files: 1,
                    needs_confirmation: d.needs_confirmation,
                }),
            }
        }
    }
    // Numeric values (exposures) in numeric order, other values alphabetically.
    let number = |g: &DerivedGroup| {
        g.value
            .split(' ')
            .next()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(f64::INFINITY)
    };
    groups.sort_by(|a, b| {
        (a.light_set, a.field)
            .cmp(&(b.light_set, b.field))
            .then(number(a).total_cmp(&number(b)))
            .then(a.value.cmp(&b.value))
    });
    Ok(groups)
}

/// Canonical, sorted, deduplicated files of all sets with their set index; a
/// file listed by two sets belongs to the first.
pub fn set_files(sets: &[InputSet], cancel: &AtomicBool) -> Result<Vec<(PathBuf, usize)>> {
    let mut owner = std::collections::BTreeMap::new();
    for (index, set) in sets.iter().enumerate() {
        let mut files = vec![];
        for input in &set.inputs {
            check_cancelled(cancel)?;
            crate::survey::collect_folder_inputs(input, &mut files)?;
        }
        for file in files {
            owner.entry(file.canonicalize()?).or_insert(index);
        }
    }
    Ok(owner.into_iter().collect())
}

/// Light sets inspected together: quality limits per set, shared target and
/// sensor checks across sets, provisional weights normalized over all sets.
pub fn inspect_sets(
    sets: &[InputSet],
    config: InspectConfig,
    cancel: &AtomicBool,
    progress: Option<&(dyn Fn(usize, usize) + Sync)>,
) -> Result<(InspectReport, PathBuf)> {
    config.validate()?;
    check_cancelled(cancel)?;
    let start = Instant::now();
    let files = set_files(sets, cancel)?;
    let mut root = files
        .first()
        .and_then(|(p, _)| p.parent())
        .ok_or(Error::Invalid("No FITS input files found."))?
        .to_path_buf();
    while files.iter().any(|(p, _)| !p.starts_with(&root)) {
        if !root.pop() {
            return Err(Error::Invalid(
                "Input folders require a shared filesystem root.",
            ));
        }
    }
    let report = inspect_paths(files, sets, &root, config, cancel, start, progress)?;
    Ok((report, root))
}

fn inspect_paths(
    files: Vec<(PathBuf, usize)>,
    sets: &[InputSet],
    root: &Path,
    config: InspectConfig,
    cancel: &AtomicBool,
    start: Instant,
    progress: Option<&(dyn Fn(usize, usize) + Sync)>,
) -> Result<InspectReport> {
    // Bound resident raw/green images independently of total input count,
    // including nested Rayon work in the immutable FITS decoder. Each worker
    // takes the next frame when it is free, so no slow frame holds up others
    // and the workers do not all start reading at once.
    let concurrency = rayon::current_num_threads().min(16);
    let next = AtomicUsize::new(0);
    let failed = AtomicBool::new(false);
    let completed = Mutex::new(0usize);
    if let Some(progress) = progress {
        progress(0, files.len());
    }
    let mut finished: Vec<(usize, Result<QualityFrame>)> = (0..concurrency)
        .into_par_iter()
        .flat_map_iter(|_| {
            let mut memory = WorkerMemory::default();
            let mut done = Vec::new();
            while !failed.load(Ordering::Relaxed) {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some((path, set)) = files.get(index) else {
                    break;
                };
                let frame = check_cancelled(cancel)
                    .and_then(|()| {
                        one(
                            path,
                            root,
                            &sets[*set],
                            &mut memory,
                            config.sky_stars,
                            cancel,
                        )
                    })
                    .map(|mut frame| {
                        frame.light_set = *set;
                        if let Some(progress) = progress {
                            // Publish in completion order so parallel callbacks never go backwards.
                            let mut count = completed.lock().unwrap();
                            *count += 1;
                            progress(*count, files.len());
                        }
                        frame
                    });
                if frame.is_err() {
                    failed.store(true, Ordering::Relaxed);
                }
                done.push((index, frame));
            }
            done
        })
        .collect();
    finished.sort_unstable_by_key(|(index, _)| *index);
    let mut frames = finished
        .into_iter()
        .map(|(_, frame)| frame)
        .collect::<Result<Vec<_>>>()?;
    classify_sets(&mut frames, sets.len(), &config);
    let accepted_count = frames.iter().filter(|f| f.accepted).count();
    let total_exposure_seconds = frames
        .iter()
        .filter_map(|f| f.frame.as_ref().map(|i| i.exposure_seconds))
        .sum::<f64>();
    let accepted_exposure_seconds = frames
        .iter()
        .filter(|f| f.accepted)
        .filter_map(|f| f.frame.as_ref().map(|i| i.exposure_seconds))
        .sum::<f64>();
    if !total_exposure_seconds.is_finite() || !accepted_exposure_seconds.is_finite() {
        return Err(Error::Invalid(
            "Exposure totals exceed the supported finite range.",
        ));
    }
    let wall_seconds = start.elapsed().as_secs_f64();
    Ok(InspectReport {
        schema_version: 1,
        command: "inspect",
        frame_count: frames.len(),
        accepted_count,
        rejected_count: frames.len() - accepted_count,
        total_exposure_seconds,
        accepted_exposure_seconds,
        wall_seconds,
        frames_per_second: frames.len() as f64 / wall_seconds,
        peak_rss_bytes: fulgur_core::telemetry::peak_rss_bytes(),
        max_concurrent_frames: concurrency,
        cpu_threads: rayon::current_num_threads(),
        config,
        weights_status: "provisional; exposure-normalized noise only; no matched-star transparency",
        photometry_status: "M3 must finalize matched-star scale, transparency and weights before integration",
        coordinate_system: "zero-based source CFA pixels; original sensor row order",
        noise_units: "ADU of the averaged two-green detection grid (mono: 2x2 mean); noise_per_second divides by EXPTIME",
        registration_status: "not performed; registration-failure rejection belongs to M3",
        timing_scope: "input discovery, reading, measurement and classification; excludes report serialization and CLI startup",
        bias_subtracted: false,
        intermediate_images_written: 0,
        target_names: target_names(&frames),
        light_sets: sets
            .iter()
            .enumerate()
            .map(|(index, set)| InspectedSet {
                label: set.label.clone(),
                layout: set.layout,
                exposure_override_seconds: set.exposure,
                derived_metadata_confirmed: set.accept_derived,
                frame_count: frames.iter().filter(|f| f.light_set == index).count(),
                calibration: set.calibration.as_ref().map(|c| c.summary.clone()),
                accepted_count: frames
                    .iter()
                    .filter(|f| f.light_set == index && f.accepted)
                    .count(),
            })
            .collect(),
        frames,
    })
}

fn csv_text(value: &str) -> String {
    let safe = if value.starts_with(['=', '+', '-', '@']) {
        format!("'{value}")
    } else {
        value.to_owned()
    };
    format!("\"{}\"", safe.replace('"', "\"\""))
}

fn number(value: Option<f64>) -> String {
    value.map(|v| format!("{v:.6}")).unwrap_or_default()
}

pub fn csv(report: &InspectReport) -> String {
    let mut out = String::from(
        "\u{feff}file_name;exposure_s;detected_peaks;detected_stars;measured_stars;catalog_capped;hfr_px;moment_fwhm_px;eccentricity;background_adu;noise_adu;noise_adu_per_second;provisional_weight;accepted;rejection_reasons;hfr_limit_px;star_count_limit;exposure_group_size;processing_s;example_star_x_px;example_star_y_px;example_star_hfr_px\n",
    );
    for f in &report.frames {
        let example = f.stars.iter().min_by(|a, b| {
            (a.hfr_px - f.hfr_px.unwrap_or(0.0))
                .abs()
                .total_cmp(&(b.hfr_px - f.hfr_px.unwrap_or(0.0)).abs())
        });
        out.push_str(&format!(
            "{};{};{};{};{};{};{};{};{};{};{};{};{:.6};{};{};{};{};{};{:.6};{};{};{}\n",
            csv_text(&f.file_name),
            number(f.frame.as_ref().map(|i| i.exposure_seconds)),
            f.detected_peaks,
            f.star_count,
            f.measured_star_count,
            f.catalog_capped,
            number(f.hfr_px),
            number(f.moment_fwhm_px),
            number(f.eccentricity),
            number(f.background_adu),
            number(f.noise_adu),
            number(f.noise_adu_per_second),
            f.provisional_weight,
            f.accepted,
            csv_text(&f.rejection_reasons.join(" | ")),
            number(f.hfr_rejection_limit_px),
            number(f.star_count_rejection_limit),
            f.exposure_group_size,
            f.wall_seconds,
            number(example.map(|s| s.x_px)),
            number(example.map(|s| s.y_px)),
            number(example.map(|s| s.hfr_px))
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use fulgur_core::BayerPattern;

    fn frame(exposure: f64, count: usize, hfr: f64, ecc: f64, noise: f64) -> QualityFrame {
        let mut row = QualityFrame::empty("test.fit".to_owned());
        row.frame = Some(FrameInfo {
            width: 2160,
            height: 3840,
            bitpix: 16,
            bzero: 32768.0,
            bscale: 1.0,
            bayer_pattern: Some(BayerPattern::GRBG),
            mosaic: Mosaic::Bayer,
            mosaic_source: "header",
            exposure_seconds: exposure,
            object: Some("M33".to_owned()),
            filter: Some("IRCUT".to_owned()),
            date_obs: None,
            gain: Some(200.0),
            bias_informational: Some(2064.0),
            saturation_adu: None,
            creator: None,
            derived: vec![],
            rows_mirrored: false,
        });
        row.star_count = count;
        row.measured_star_count = count.min(250);
        row.hfr_px = Some(hfr);
        row.eccentricity = Some(ecc);
        row.noise_adu = Some(noise);
        row.noise_adu_per_second = Some(noise / exposure);
        row
    }

    #[test]
    fn mixed_exposure_groups_keep_shorter_frames_and_use_rate_noise_once() {
        let mut frames: Vec<_> = (0..5)
            .map(|_| frame(5.0, 20, 2.0, 0.3, 10.0))
            .chain((0..5).map(|_| frame(20.0, 200, 2.0, 0.3, 20.0)))
            .collect();
        classify(&mut frames, &InspectConfig::default());
        assert!(frames.iter().all(|f| f.accepted));
        assert!((frames[0].provisional_weight - 0.25).abs() < 1e-12);
        assert_eq!(frames[5].provisional_weight, 1.0);
        assert_eq!(frames[0].star_count_rejection_limit, Some(9.0));
        assert_eq!(frames[5].star_count_rejection_limit, Some(90.0));
    }

    #[test]
    fn another_target_name_stays_in_the_set_and_is_listed_for_the_warning() {
        // Nights of one target saved as "M 33" and "M 33 b" in one folder.
        let mut frames: Vec<_> = (0..8).map(|_| frame(10.0, 200, 2.0, 0.3, 20.0)).collect();
        for f in &mut frames[..5] {
            f.frame.as_mut().unwrap().object = Some("M 33".into());
        }
        for f in &mut frames[5..] {
            f.frame.as_mut().unwrap().object = Some(" M 33 b ".into());
        }
        classify_sets(&mut frames, 1, &InspectConfig::default());
        assert!(frames.iter().all(|f| f.accepted));
        assert_eq!(target_names(&frames), ["M 33", "M 33 b"]);
        // The quality limits use the whole set, both nights together.
        assert_eq!(frames[7].exposure_group_size, 8);
    }

    #[test]
    fn light_sets_are_classified_separately_and_may_differ_in_filter_gain_and_layout() {
        // Set 0: 8 IRCUT colour frames, HFR 2.0. Set 1: 6 LP mono frames with
        // larger stars and another gain; a joint population would reject them.
        let mut frames: Vec<_> = (0..8).map(|_| frame(10.0, 200, 2.0, 0.3, 20.0)).collect();
        for _ in 0..6 {
            let mut f = frame(30.0, 400, 3.2, 0.3, 30.0);
            f.light_set = 1;
            let info = f.frame.as_mut().unwrap();
            info.filter = Some("LP".into());
            info.gain = Some(80.0);
            info.apply_layout(LayoutChoice::Mono);
            frames.push(f);
        }
        // A third set on another sensor and one with another target name.
        let mut other_sensor = frame(10.0, 200, 2.0, 0.3, 20.0);
        other_sensor.light_set = 2;
        other_sensor.frame.as_mut().unwrap().width = 3840;
        let mut other_target = frame(10.0, 200, 2.0, 0.3, 20.0);
        other_target.light_set = 3;
        other_target.frame.as_mut().unwrap().object = Some("M31".into());
        frames.extend([other_sensor, other_target]);
        classify_sets(&mut frames, 4, &InspectConfig::default());
        assert!(
            frames[..14].iter().all(|f| f.accepted),
            "both sets keep their frames"
        );
        assert_eq!(
            frames[13].hfr_rejection_limit_px.map(|l| l > 3.2),
            Some(true)
        );
        assert!(
            frames[14].accepted,
            "another sensor registers as its own camera (ADR-120): {:?}",
            frames[14].rejection_reasons
        );
        assert!(
            frames[15].accepted,
            "another target name only warns: {:?}",
            frames[15].rejection_reasons
        );
        // Weights are normalized across all sets, not per set.
        let maximum = frames
            .iter()
            .map(|f| f.provisional_weight)
            .fold(0.0, f64::max);
        assert_eq!(maximum, 1.0);
        assert!(
            frames[..8].iter().all(|f| f.provisional_weight < 1.0)
                || frames[8..14].iter().all(|f| f.provisional_weight < 1.0)
        );
        // One set reproduces the single-population result exactly.
        let mut single: Vec<_> = (0..8).map(|_| frame(10.0, 200, 2.0, 0.3, 20.0)).collect();
        let mut joint = single.clone();
        classify(&mut single, &InspectConfig::default());
        classify_sets(&mut joint, 1, &InspectConfig::default());
        for (a, b) in single.iter().zip(&joint) {
            assert_eq!(
                a.provisional_weight.to_bits(),
                b.provisional_weight.to_bits()
            );
            assert_eq!(a.rejection_reasons, b.rejection_reasons);
        }
    }

    #[test]
    fn independent_reasons_identify_blur_clouds_and_tracking() {
        let mut frames: Vec<_> = (0..10).map(|_| frame(10.0, 200, 2.0, 0.3, 20.0)).collect();
        frames.push(frame(10.0, 200, 3.0, 0.3, 20.0));
        frames.push(frame(10.0, 20, 2.0, 0.3, 20.0));
        frames.push(frame(10.0, 200, 2.0, 0.8, 20.0));
        classify(&mut frames, &InspectConfig::default());
        assert!(frames[..10].iter().all(|f| f.accepted));
        for (index, reason) in [
            (10, "excessive_hfr"),
            (11, "low_star_count_in_exposure_group"),
            (12, "excessive_eccentricity"),
        ] {
            assert!(frames[index].rejection_reasons.iter().any(|r| r == reason));
            assert_eq!(frames[index].provisional_weight, 0.0);
        }
    }

    #[test]
    fn insufficient_exposure_group_skips_count_threshold() {
        let mut frames: Vec<_> = (0..5).map(|_| frame(20.0, 200, 2.0, 0.3, 20.0)).collect();
        frames.push(frame(5.0, 12, 2.05, 0.3, 10.0));
        classify(&mut frames, &InspectConfig::default());
        assert!(frames[5].accepted);
        assert_eq!(frames[5].exposure_group_size, 1);
        assert!(frames[5].star_count_rejection_limit.is_none());
    }

    #[test]
    fn zero_noise_and_extreme_exposure_never_produce_infinite_weights() {
        let mut frames = vec![
            frame(10.0, 100, 2.0, 0.3, 0.0),
            frame(1e300, 100, 2.0, 0.3, 20.0),
        ];
        classify(&mut frames, &InspectConfig::default());
        assert!(
            frames
                .iter()
                .all(|f| !f.accepted && f.provisional_weight == 0.0)
        );
        assert!(
            frames[0]
                .rejection_reasons
                .iter()
                .any(|r| r == "unusable_noise")
        );
        assert!(
            frames[1]
                .rejection_reasons
                .iter()
                .any(|r| r == "nonfinite_provisional_weight")
        );
    }

    #[test]
    fn first_incompatible_frame_cannot_poison_majority_selection() {
        let mut frames: Vec<_> = (0..4).map(|_| frame(10.0, 100, 2.0, 0.3, 20.0)).collect();
        frames[0].frame.as_mut().unwrap().filter = Some("other".to_owned());
        classify(&mut frames, &InspectConfig::default());
        assert!(!frames[0].accepted);
        assert!(frames[1..].iter().all(|f| f.accepted));
    }

    #[test]
    fn invalid_thresholds_and_csv_formula_injection_are_rejected() {
        let config = InspectConfig {
            max_eccentricity: f64::NAN,
            ..Default::default()
        };
        assert!(config.validate().is_err());
        assert_eq!(csv_text("=SUM(1;2)"), "\"'=SUM(1;2)\"");
        assert_eq!(csv_text("a\"b.fit"), "\"a\"\"b.fit\"");
    }

    #[test]
    fn a_mixed_folder_falls_into_the_groups_inspection_keeps_apart() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, width: usize, extra: &[&str]| {
            let mut bytes = Vec::new();
            let mut cards = vec![
                "SIMPLE  =                    T".to_owned(),
                "BITPIX  =                   16".to_owned(),
                "NAXIS   =                    2".to_owned(),
                format!("NAXIS1  = {width:20}"),
                "NAXIS2  =                   64".to_owned(),
                "BZERO   =                32768".to_owned(),
                "EXPTIME =                  10.".to_owned(),
            ];
            cards.extend(extra.iter().map(|c| c.to_string()));
            cards.push("END".into());
            for card in cards {
                bytes.extend(format!("{card:<80}").bytes());
            }
            bytes.resize(2880, b' ');
            bytes.resize(2880 + (width * 64 * 2).div_ceil(2880) * 2880, 0);
            let path = dir.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        };
        let mut files = vec![];
        for i in 0..3 {
            files.push(write(&format!("a{i}.fit"), 96, &["FILTER  = 'L       '"]));
        }
        for i in 0..2 {
            files.push(write(&format!("b{i}.fit"), 96, &["FILTER  = 'Ha      '"]));
        }
        files.push(write(
            "c0.fit",
            128,
            &["FILTER  = 'L       '", "BAYERPAT= 'RGGB    '"],
        ));
        let broken = dir.path().join("broken.fit");
        std::fs::write(&broken, b"not a fits file").unwrap();
        files.push(broken.clone());
        let groups = compatibility_groups(&files, LayoutChoice::Auto);
        let labels: Vec<String> = groups.iter().map(FrameGroup::label).collect();
        assert_eq!(
            labels,
            ["96×64 mono · L", "96×64 mono · Ha", "128×64 colour · L"]
        );
        assert_eq!(
            groups.iter().map(|g| g.files.len()).collect::<Vec<_>>(),
            [4, 2, 1]
        );
        assert!(
            groups[0].files.contains(&broken),
            "unreadable files stay with the largest group"
        );
        // A declared layout makes colour and mono alike, never sensor sizes.
        let mono = compatibility_groups(&files[3..6], LayoutChoice::Mono);
        assert_eq!(mono.len(), 2);
        // A mono and a colour camera of one sensor size stay two groups under a
        // declared layout, and each keeps its own header layout (ASI2600MM + MC).
        let colour_twin = write(
            "d0.fit",
            96,
            &["FILTER  = 'L       '", "BAYERPAT= 'RGGB    '"],
        );
        let pair = [files[0].clone(), colour_twin];
        for layout in [LayoutChoice::Auto, LayoutChoice::Mono] {
            let groups = compatibility_groups(&pair, layout);
            assert_eq!(groups.len(), 2, "{layout:?}");
            assert_eq!(
                groups.iter().map(|g| g.colour).collect::<Vec<_>>(),
                [false, true]
            );
        }
    }

    #[test]
    fn frames_keep_file_order_when_workers_finish_in_any_order() {
        let dir = tempfile::tempdir().unwrap();
        let (width, height) = (96_usize, 80_usize);
        for frame in 0..40_usize {
            let mut bytes = Vec::new();
            for card in [
                "SIMPLE  =                    T".to_owned(),
                "BITPIX  =                   16".to_owned(),
                "NAXIS   =                    2".to_owned(),
                format!("NAXIS1  = {width:20}"),
                format!("NAXIS2  = {height:20}"),
                "BSCALE  =                    1".to_owned(),
                "BZERO   =                32768".to_owned(),
                "EXPTIME =                  10.".to_owned(),
                "END".to_owned(),
            ] {
                bytes.extend(format!("{card:<80}").bytes());
            }
            bytes.resize(2880, b' ');
            for i in 0..width * height {
                let value = 1000 + 10 * frame + (i * 7) % 5;
                bytes.extend(((value as i32 - 32768) as i16).to_be_bytes());
            }
            bytes.resize(bytes.len().div_ceil(2880) * 2880, 0);
            let path = dir.path().join(format!("frame_{frame:02}.fit"));
            std::fs::write(path, bytes).unwrap();
        }
        let counts = Mutex::new(Vec::new());
        let (report, _) = inspect_sets(
            &[single_set(&[dir.path().to_path_buf()], LayoutChoice::Auto)],
            InspectConfig::default(),
            &AtomicBool::new(false),
            Some(&|done: usize, _: usize| counts.lock().unwrap().push(done)),
        )
        .unwrap();
        assert_eq!(report.frames.len(), 40);
        for (frame, result) in report.frames.iter().enumerate() {
            assert_eq!(result.file_name, format!("frame_{frame:02}.fit"));
            let background = result.background_adu.unwrap();
            let expected = (1000 + 10 * frame) as f64;
            assert!((background - expected).abs() < 5., "{background}");
        }
        assert_eq!(
            counts.into_inner().unwrap(),
            (0..=40).collect::<Vec<usize>>()
        );
    }

    /// A mono frame with Gaussian stars on a textured sky.
    fn starry_frame(path: &Path, shift: f64) {
        let (width, height) = (256_usize, 224_usize);
        let mut bytes = Vec::new();
        for card in [
            "SIMPLE  =                    T".to_owned(),
            "BITPIX  =                   16".to_owned(),
            "NAXIS   =                    2".to_owned(),
            format!("NAXIS1  = {width:20}"),
            format!("NAXIS2  = {height:20}"),
            "BSCALE  =                    1".to_owned(),
            "BZERO   =                32768".to_owned(),
            "EXPTIME =                  10.".to_owned(),
            "END".to_owned(),
        ] {
            bytes.extend(format!("{card:<80}").bytes());
        }
        bytes.resize(2880, b' ');
        let stars: Vec<(f64, f64, f64)> = (0..40)
            .map(|k| {
                let k = k as f64;
                (
                    12. + (k * 37.3) % 230. + shift,
                    12. + (k * 53.9) % 198.,
                    3000. + (k * 911.) % 20000.,
                )
            })
            .collect();
        for y in 0..height {
            for x in 0..width {
                let mut v = 1000. + ((x * 7 + y * 13) % 11) as f64;
                for &(sx, sy, a) in &stars {
                    let d2 = (x as f64 - sx).powi(2) + (y as f64 - sy).powi(2);
                    v += a * (-d2 / (2. * 1.6 * 1.6)).exp();
                }
                let stored = (v.min(65535.).round() as i32 - 32768) as i16;
                bytes.extend(stored.to_be_bytes());
            }
        }
        bytes.resize(bytes.len().div_ceil(2880) * 2880, 0);
        std::fs::write(path, bytes).unwrap();
    }

    /// Lists added to an inspection without them equal those of an
    /// inspection that kept them, and nothing else changes (ADR-154).
    fn assert_added_lists_match(sets: &[InputSet]) {
        let cancel = AtomicBool::new(false);
        let with = InspectConfig {
            sky_stars: true,
            ..InspectConfig::default()
        };
        let (expected, _) = inspect_sets(sets, with, &cancel, None).unwrap();
        let (mut actual, root) =
            inspect_sets(sets, InspectConfig::default(), &cancel, None).unwrap();
        assert!(actual.frames.iter().all(|f| f.sky_stars.is_empty()));
        let before = serde_json::to_string(&actual.frames).unwrap();
        let counts = Mutex::new(vec![]);
        add_sky_stars(
            &mut actual,
            &root,
            sets,
            &cancel,
            Some(&|done: usize, _: usize| counts.lock().unwrap().push(done)),
        )
        .unwrap();
        assert_eq!(serde_json::to_string(&actual.frames).unwrap(), before);
        let mut listed = 0;
        for (a, e) in actual.frames.iter().zip(&expected.frames) {
            let bits = |list: &[crate::sky::detect::SkyStar]| -> Vec<[u64; 6]> {
                list.iter().map(|s| s.map(f64::to_bits)).collect()
            };
            assert_eq!(bits(&a.sky_stars), bits(&e.sky_stars), "{}", a.file_name);
            listed += usize::from(!a.sky_stars.is_empty());
        }
        assert!(listed > 0, "no frame had a bright-star list");
        let measured = actual
            .frames
            .iter()
            .filter(|f| f.background_adu.is_some())
            .count();
        assert_eq!(
            counts.into_inner().unwrap(),
            (0..=measured).collect::<Vec<_>>()
        );
    }

    #[test]
    fn sky_star_lists_added_later_equal_those_of_a_sky_inspection() {
        let dir = tempfile::tempdir().unwrap();
        for frame in 0..6 {
            starry_frame(
                &dir.path().join(format!("light_{frame}.fit")),
                frame as f64 * 0.37,
            );
        }
        assert_added_lists_match(&[single_set(&[dir.path().to_path_buf()], LayoutChoice::Auto)]);
        let wrong = vec![single_set(&[dir.path().to_path_buf()], LayoutChoice::Auto); 2];
        let (mut report, root) = inspect_sets(
            &wrong[..1],
            InspectConfig::default(),
            &AtomicBool::new(false),
            None,
        )
        .unwrap();
        assert!(add_sky_stars(&mut report, &root, &wrong, &AtomicBool::new(false), None).is_err());
    }

    #[test]
    fn sky_star_lists_added_later_match_on_real_frames() {
        let Some(dir) = std::env::var_os("FULGURSTACK_TEST_FITS_DIR") else {
            return;
        };
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| fulgur_core::fits::is_input(p))
            .collect();
        files.sort();
        files.truncate(4);
        assert_added_lists_match(&[single_set(&files, LayoutChoice::Auto)]);
    }

    #[test]
    fn cancellation_stops_before_input_decoding() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bad.fit"), b"not FITS").unwrap();
        let result = inspect(dir.path(), InspectConfig::default(), &AtomicBool::new(true));
        assert!(matches!(result, Err(Error::Cancelled)));
    }
}
