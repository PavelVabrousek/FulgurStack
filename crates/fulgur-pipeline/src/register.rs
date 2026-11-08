//! M3 registration plan only: immutable CFA catalogs, no RGB or resampled images.
use crate::geometry;
pub use crate::geometry::{Canvas, Match, Transform};
use crate::inspect::{InspectConfig, InspectReport, QualityFrame};
pub use crate::registration_timing::RegistrationProfile;
use crate::stars::median;
use chrono::NaiveDateTime;
use fulgur_core::{Error, Result, check_cancelled};
use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

mod auto_reference;
pub mod cameras;
pub mod set_photometry;
pub mod sky_engine;
pub use auto_reference::AutoReferenceReport;
pub use sky_engine::{CatalogueProvider, SkyOptions, register_sets_sky, register_sky};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReferenceStrategy {
    #[default]
    Auto,
    ChronologicalMedian,
}

/// Counts completed registration work, including the optional external-reference pass.
/// Serialize publication so parallel source fits cannot move the UI backwards.
pub(crate) struct RegistrationProgress<'a> {
    completed: Mutex<usize>,
    /// Planned work; it grows when a stranded light set gets a second pass.
    total: AtomicUsize,
    observer: &'a (dyn Fn(usize, usize) + Sync),
}
impl RegistrationProgress<'_> {
    pub(crate) fn advance(&self) {
        let mut completed = self.completed.lock().unwrap();
        *completed += 1;
        let total = self.total.load(Ordering::Relaxed);
        // The plan is an estimate; the bar never shows more than its total.
        (self.observer)((*completed).min(total), total);
    }

    pub(crate) fn completed(&self) -> usize {
        *self.completed.lock().unwrap()
    }

    /// Plans another pass after the finished one and shows the new share at once.
    fn next_pass(&self, work: usize) {
        let completed = self.completed.lock().unwrap();
        // The finished pass counts as it ran, whatever its estimate was.
        let total = *completed + work;
        self.total.store(total, Ordering::Relaxed);
        (self.observer)(*completed, total);
    }

    /// Plans work that was not foreseen, such as another try of a camera.
    pub(crate) fn add(&self, work: usize) {
        let completed = self.completed.lock().unwrap();
        let total = self.total.fetch_add(work, Ordering::Relaxed) + work;
        (self.observer)((*completed).min(total), total);
    }

    /// Counts the rest of a planned attempt that ended early, as a failed match does.
    pub(crate) fn reach(&self, target: usize) {
        let mut completed = self.completed.lock().unwrap();
        if *completed < target {
            *completed = target;
            let total = self.total.load(Ordering::Relaxed);
            (self.observer)((*completed).min(total), total);
        }
    }
}

/// Pixel access for the extended witness catalog of the master anchor: the
/// positions of its `WITNESS_STARS` brightest detections (saturated included),
/// with its light set's master dark when it was measured dark-calibrated.
pub struct WitnessCatalog<'a> {
    pub root: &'a Path,
    pub calibrations: &'a [Option<std::sync::Arc<crate::calibration::Calibration>>],
}

const WITNESS_STARS: usize = 5000;
/// Precisely measured anchor stars for accuracy witnesses in dense fields.
const PRECISE_WITNESS_STARS: usize = 1000;
/// Flux-ranked anchor stars per 4x4 cell as witnesses of a dense field.
const DENSE_WITNESS_CELL_STARS: usize = 125;

/// Precisely measured anchor stars when the first list still shares too few
/// with a frame of another filter (ADR-125).
const DEEP_PRECISE_WITNESS_STARS: usize = 5000;

/// The anchor's extended catalogs: detection positions (coverage) and
/// precisely measured stars (accuracy witnesses when catalogs overlap little).
pub(crate) struct Witnesses {
    positions: Vec<crate::stars::Star>,
    precise: Vec<crate::stars::Star>,
    /// A longer precisely measured list, empty in a dense field.
    deep_precise: Vec<crate::stars::Star>,
}

impl WitnessCatalog<'_> {
    /// `None` (ordinary coverage rule) when the anchor cannot be read again or
    /// its dark is not available; never an error.
    fn stars(&self, frame: &QualityFrame, cancel: &AtomicBool) -> Option<Witnesses> {
        self.measure(frame, false, cancel)
    }

    /// With the longer precisely measured list, for the native field checks.
    fn deep_stars(&self, frame: &QualityFrame, cancel: &AtomicBool) -> Option<Witnesses> {
        self.measure(frame, true, cancel)
    }

    /// The frame's pixels as it was measured: its rows, layout and dark.
    fn raw(&self, frame: &QualityFrame) -> Option<fulgur_core::RawFrame> {
        let info = frame.frame.as_ref()?;
        let calibration = if frame.calibrated {
            Some(self.calibrations.get(frame.light_set)?.as_ref()?)
        } else {
            None
        };
        let mut raw = fulgur_core::fits::read_raw(&self.root.join(&frame.file_name)).ok()?;
        if [raw.header.info.width, raw.header.info.height] != [info.width, info.height] {
            return None;
        }
        if info.rows_mirrored {
            fulgur_core::fits::flip_rows(&mut raw.pixels, info.width);
        }
        raw.header.info.bayer_pattern = info.bayer_pattern;
        raw.header.info.mosaic = info.mosaic;
        raw.header.info.exposure_seconds = info.exposure_seconds;
        raw.header.info.saturation_adu = info.saturation_adu;
        if let Some(calibration) = calibration {
            calibration.apply(&mut raw, [0, 0]).ok()?;
        }
        Some(raw)
    }

    /// A refused frame's own longer precisely measured list, read once more
    /// from its file (ADR-125); `None` in a dense field or when unreadable.
    pub(crate) fn longer_list(
        &self,
        frame: &QualityFrame,
        cancel: &AtomicBool,
    ) -> Option<Vec<crate::stars::Star>> {
        if crate::stars::is_dense(frame.star_count) {
            return None;
        }
        let raw = self.raw(frame)?;
        crate::stars::analyze_with_limit(&raw, cancel, DEEP_PRECISE_WITNESS_STARS)
            .ok()
            .map(|m| m.stars)
    }

    fn measure(&self, frame: &QualityFrame, deep: bool, cancel: &AtomicBool) -> Option<Witnesses> {
        let raw = self.raw(frame)?;
        let dense = crate::stars::is_dense(frame.star_count);
        let precise = if dense {
            crate::stars::flux_witnesses(&raw, cancel, DENSE_WITNESS_CELL_STARS).ok()?
        } else {
            crate::stars::analyze_with_limit(&raw, cancel, PRECISE_WITNESS_STARS)
                .ok()?
                .stars
        };
        let deep_precise = if dense || !deep {
            vec![]
        } else {
            crate::stars::analyze_with_limit(&raw, cancel, DEEP_PRECISE_WITNESS_STARS)
                .ok()?
                .stars
        };
        Some(Witnesses {
            positions: crate::stars::witness_positions(&raw, cancel, WITNESS_STARS).ok()?,
            precise,
            deep_precise,
        })
    }
}

/// Optional excluded-frame fits never contribute to selected-frame prediction history.
#[derive(Clone, Copy, Debug, Default)]
pub struct RegistrationOptions {
    pub reference_strategy: ReferenceStrategy,
    pub geometric_correction: bool,
    pub diagnose_excluded: bool,
    /// Experimental: frames refused by whole-field validation still join the
    /// tiles around which their stars stay accurate (ADR-109).
    pub partial_frames: bool,
    /// Whole-field limits every frame must meet (ADR-121).
    pub accuracy: AccuracyLimits,
}

pub use crate::distortion::AccuracyLimits;

/// Witness stars must reach this far around a tile to judge it: a 512 px tile
/// alone holds about one star of a registration list.
const TILE_WITNESS_MARGIN_PX: f64 = 768.;
const TILE_MIN_WITNESSES: usize = 5;

/// A processing tile a partial frame stayed out of. Position and size are in
/// output pixels; the stars and errors are those of its test window, which
/// reaches beyond the tile (ADR-116).
#[derive(Clone, Copy, Debug, Serialize)]
pub struct RefusedTile {
    pub x_px: i32,
    pub y_px: i32,
    pub width_px: usize,
    pub height_px: usize,
    /// `too_few_stars`, `rms_above_1px`, or `star_above_3px` when only single
    /// stars miss and the window as a whole stays within the RMS limit. The
    /// names keep the default limits; the run's limits are in its report.
    pub reason: &'static str,
    pub stars: usize,
    pub rms_error_px: Option<f64>,
    pub maximum_error_px: Option<f64>,
}

/// A frame refused as a whole, kept for the tiles that pass the same accuracy
/// limits. The decision follows the processing tiles, so the master depends
/// on tile size; the owner accepts that for this experiment (ADR-109).
#[derive(Debug, Serialize)]
pub struct PartialField {
    pub witnesses: usize,
    /// The fit the frame stacks with, which can differ from the one in
    /// `registration_validation`; the two error values below belong to it.
    pub fit: &'static str,
    pub fit_rmse_px: f64,
    pub fit_maximum_error_px: f64,
    pub tiles_used: AtomicUsize,
    pub tiles_refused: AtomicUsize,
    /// Filled by the integration tiles in any order, written sorted.
    #[serde(serialize_with = "sorted_tiles")]
    pub refused_tiles: Mutex<Vec<RefusedTile>>,
    /// Reference x, y and registration error of each witness star.
    #[serde(skip)]
    pub(crate) stars: Vec<[f64; 3]>,
    #[serde(skip)]
    limits: AccuracyLimits,
}

impl PartialField {
    /// A refused frame needs the run's minimum of matched stars to be
    /// considered at all.
    pub(crate) fn admissible(stars: &[[f64; 3]], limits: AccuracyLimits) -> bool {
        stars.len() >= limits.min_stars
    }

    pub(crate) fn new(stars: Vec<[f64; 3]>, fit: &'static str, limits: AccuracyLimits) -> Self {
        let squares: f64 = stars.iter().map(|s| s[2] * s[2]).sum();
        Self {
            witnesses: stars.len(),
            fit,
            fit_rmse_px: (squares / stars.len().max(1) as f64).sqrt(),
            fit_maximum_error_px: stars.iter().map(|s| s[2]).fold(0., f64::max),
            tiles_used: AtomicUsize::new(0),
            tiles_refused: AtomicUsize::new(0),
            refused_tiles: Mutex::new(Vec::new()),
            stars,
            limits,
        }
    }

    /// True when the stars around an output tile meet the whole-field limits.
    pub fn admits(&self, origin: [i32; 2], size: [usize; 2]) -> bool {
        use std::sync::atomic::Ordering::Relaxed;
        let near = |value: f64, start: i32, length: usize| {
            value >= f64::from(start) - TILE_WITNESS_MARGIN_PX
                && value < f64::from(start) + length as f64 + TILE_WITNESS_MARGIN_PX
        };
        let (mut count, mut sum, mut worst) = (0, 0., 0_f64);
        for [x, y, error] in &self.stars {
            if near(*x, origin[0], size[0]) && near(*y, origin[1], size[1]) {
                count += 1;
                sum += error * error;
                worst = worst.max(*error);
            }
        }
        let rms = (count > 0).then(|| (sum / count as f64).sqrt());
        let rms_passes = rms.is_some_and(|r| r <= f64::from(self.limits.rms_px));
        if count >= TILE_MIN_WITNESSES && rms_passes && worst <= f64::from(self.limits.maximum_px) {
            self.tiles_used.fetch_add(1, Relaxed);
            return true;
        }
        self.tiles_refused.fetch_add(1, Relaxed);
        let reason = if count < TILE_MIN_WITNESSES {
            "too_few_stars"
        } else if rms_passes {
            "star_above_3px"
        } else {
            "rms_above_1px"
        };
        if let Ok(mut tiles) = self.refused_tiles.lock() {
            tiles.push(RefusedTile {
                x_px: origin[0],
                y_px: origin[1],
                width_px: size[0],
                height_px: size[1],
                reason,
                stars: count,
                rms_error_px: rms,
                maximum_error_px: (count > 0).then_some(worst),
            });
        }
        false
    }

    fn refused_for(&self, reason: &str) -> usize {
        self.refused_tiles
            .lock()
            .map_or(0, |t| t.iter().filter(|t| t.reason == reason).count())
    }
}

fn sorted_tiles<S: serde::Serializer>(
    tiles: &Mutex<Vec<RefusedTile>>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    let mut sorted = tiles.lock().map_or_else(|_| Vec::new(), |t| t.clone());
    sorted.sort_by_key(|t| (t.y_px, t.x_px));
    sorted.serialize(serializer)
}

fn shared_field<S: serde::Serializer>(
    field: &Option<std::sync::Arc<PartialField>>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    field.as_deref().serialize(serializer)
}

#[derive(Debug, Serialize)]
pub struct RegisteredFrame {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geometric_correction: Option<crate::distortion::Decision>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_to_photometric_anchor_correction: Option<crate::distortion::Warp>,
    pub file_name: String,
    pub session_id: usize,
    pub quality_accepted: bool,
    pub registration_attempted: bool,
    pub registration_success: bool,
    pub accepted: bool,
    pub rejection_reasons: Vec<String>,
    pub method: Option<&'static str>,
    pub transform: Option<Transform>,
    pub rotation_deg: Option<f64>,
    pub source_to_photometric_anchor: Option<Transform>,
    pub inlier_count: usize,
    pub inlier_rmse_px: Option<f64>,
    pub matches: Vec<Match>,
    pub photometric_matches: Vec<Match>,
    pub match_reference: String,
    pub photometric_reference: Option<&'static str>,
    pub matched_flux_ratio: Option<f64>,
    pub exposure_corrected_transparency: Option<f64>,
    pub photometric_scale: Option<f64>,
    /// Sky offset of the detection plane (green on a colour sensor).
    pub background_offset_adu: Option<f64>,
    /// Sky offset per colour channel (R, G, B); without it all channels use
    /// `background_offset_adu` (mono frames; ADR-102).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_offsets_adu: Option<[f64; 3]>,
    /// Refused as a whole, kept for accurate tiles (experimental, ADR-109).
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "shared_field"
    )]
    pub partial_field: Option<std::sync::Arc<PartialField>>,
    pub normalized_noise_adu: Option<f64>,
    pub final_weight: f64,
    pub footprint: Vec<[f64; 2]>,
    /// Plate solution, check and carrier of a sky registration (ADR-137).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sky: Option<sky_engine::SkyFrameReport>,
}
#[derive(Debug, Serialize)]
pub struct Session {
    pub id: usize,
    pub anchor: String,
    pub frame_count: usize,
    pub to_master: Option<Transform>,
    pub anchor_inliers: usize,
    pub anchor_rmse_px: Option<f64>,
    pub matches: Vec<Match>,
}
#[derive(Debug, Serialize)]
pub struct RegistrationReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_reference: Option<AutoReferenceReport>,
    #[serde(skip_serializing_if = "is_false")]
    pub geometric_correction_enabled: bool,
    /// Whole-field limits every frame had to meet (ADR-121).
    pub accuracy: AccuracyLimits,
    pub schema_version: u32,
    pub command: &'static str,
    pub reference_anchor: String,
    pub reference_selection: &'static str,
    pub reference_date_obs: Option<String>,
    pub reference_width: usize,
    pub reference_height: usize,
    pub external_reference: Option<crate::reference::ExternalReferenceReport>,
    /// Sensor sizes of a stack from several cameras (ADR-110).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub camera_groups: Vec<cameras::CameraGroup>,
    pub frame_count: usize,
    pub quality_accepted_count: usize,
    pub diagnose_excluded: bool,
    pub registration_attempted_count: usize,
    pub registration_skipped_count: usize,
    pub registration_success_count: usize,
    pub accepted_count: usize,
    pub rejected_count: usize,
    pub accepted_exposure_seconds: f64,
    pub wall_seconds: f64,
    pub inspection_seconds: f64,
    pub registration_seconds: f64,
    pub phase_profile: RegistrationProfile,
    pub peak_rss_bytes: Option<u64>,
    pub intermediate_images_written: usize,
    pub transform_convention: &'static str,
    pub session_rule: &'static str,
    pub photometry_rule: &'static str,
    pub weights_status: &'static str,
    pub sessions: Vec<Session>,
    pub common_intersection_polygon: Vec<[f64; 2]>,
    pub common_intersection: Option<Canvas>,
    pub coverage_optimized: Option<Canvas>,
    pub min_coverage: f64,
    pub frames: Vec<RegisteredFrame>,
    /// The run of a sky registration (ADR-137).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sky: Option<sky_engine::SkyReport>,
    /// Light sets brought onto their own anchor because their frames share
    /// no stars with the run's photometric anchor (ADR-156).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub set_photometry: Vec<set_photometry::SetBrightness>,
    pub inspection: InspectReport,
}
fn is_false(value: &bool) -> bool {
    !*value
}

fn timestamp(row: &QualityFrame) -> Option<i64> {
    let text = row.frame.as_ref()?.date_obs.as_ref()?;
    // Header time only. A missing/unparseable timestamp is explicitly a single-session fallback.
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|d| d.timestamp_micros())
        .or_else(|| {
            NaiveDateTime::parse_from_str(text.trim_end_matches('Z'), "%Y-%m-%dT%H:%M:%S%.f")
                .ok()
                .map(|d| d.and_utc().timestamp_micros())
        })
}
/// On an outside grid the native anchor is the median by capture time. A stack
/// whose frames lack capture times takes its sharpest frame instead, as one
/// camera of several does (ADR-120, ADR-130).
fn outside_grid_anchor(
    frames: &[QualityFrame],
    selected: Option<usize>,
    outside: bool,
) -> Option<usize> {
    selected.or_else(|| outside.then(|| cameras::undated_anchor(frames)).flatten())
}
fn select_anchor(indices: &[usize], frames: &[QualityFrame]) -> Result<usize> {
    let mut eligible: Vec<_> = indices
        .iter()
        .copied()
        .filter(|&i| frames[i].accepted)
        .collect();
    if eligible.is_empty() {
        return Err(Error::Invalid(
            "No quality-accepted reference anchor available.",
        ));
    }
    if eligible.iter().any(|&i| timestamp(&frames[i]).is_none()) {
        return Err(Error::Invalid(
            "Automatic reference selection requires valid DATE-OBS on every accepted frame; provide --reference explicitly.",
        ));
    }
    eligible.sort_by(|&a, &b| {
        timestamp(&frames[a])
            .cmp(&timestamp(&frames[b]))
            .then_with(|| frames[a].file_name.cmp(&frames[b].file_name))
    });
    // An even population has two central frames. Pick the earlier one deterministically.
    Ok(eligible[(eligible.len() - 1) / 2])
}
fn manual_reference(frames: &[QualityFrame], root: &Path, requested: &Path) -> Result<usize> {
    let resolved = requested
        .canonicalize()
        .or_else(|_| root.join(requested).canonicalize())
        .ok();
    let mut matches: Vec<_> = frames
        .iter()
        .enumerate()
        .filter_map(|(i, f)| {
            let same = Path::new(&f.file_name) == requested
                || resolved.as_ref().is_some_and(|p| {
                    root.join(&f.file_name).canonicalize().ok().as_ref() == Some(p)
                });
            same.then_some(i)
        })
        .collect();
    if matches.is_empty() && requested.components().count() == 1 {
        matches = frames
            .iter()
            .enumerate()
            .filter_map(|(i, f)| {
                (Path::new(&f.file_name).file_name() == requested.file_name()).then_some(i)
            })
            .collect();
    }
    if matches.len() != 1 {
        return Err(Error::Invalid(
            "Reference must identify one input frame uniquely (source path or unique filename).",
        ));
    }
    let i = matches[0];
    if !frames[i].accepted {
        let switched_off = frames[i]
            .rejection_reasons
            .iter()
            .any(|r| r == "manually_excluded");
        return Err(Error::Invalid(if switched_off {
            "The requested reference is switched off in Use; switch it on or choose another frame."
        } else {
            "The requested reference was refused by the quality checks; choose another frame."
        }));
    }
    Ok(i)
}
fn resolve_reference(
    frames: &[QualityFrame],
    root: &Path,
    reference: Option<&Path>,
) -> Result<(Option<usize>, Option<std::path::PathBuf>)> {
    let Some(requested) = reference else {
        return Ok((None, None));
    };
    if let Ok(path) = requested
        .canonicalize()
        .or_else(|_| root.join(requested).canonicalize())
    {
        if !frames
            .iter()
            .any(|f| root.join(&f.file_name).canonicalize().ok().as_ref() == Some(&path))
        {
            return Ok((None, Some(path)));
        }
    }
    Ok((Some(manual_reference(frames, root, requested)?), None))
}

impl WitnessCatalog<'_> {
    /// The frame's longer list of precisely measured stars.
    pub(crate) fn precise(
        &self,
        frame: &QualityFrame,
        cancel: &AtomicBool,
    ) -> Option<Vec<crate::stars::Star>> {
        self.stars(frame, cancel).map(|w| w.precise)
    }

    /// A frame as the grid of other frames, on its measured stars.
    pub(crate) fn frame_stars(
        &self,
        frame: &QualityFrame,
        cancel: &AtomicBool,
    ) -> Result<crate::reference::FrameStars> {
        let info = frame
            .frame
            .as_ref()
            .ok_or(Error::Invalid("The reference frame could not be read."))?;
        let longer = self.stars(frame, cancel);
        let stars = longer
            .as_ref()
            .map_or_else(|| frame.stars.clone(), |w| w.precise.clone());
        Ok(crate::reference::FrameStars {
            name: frame.file_name.clone(),
            size: [info.width, info.height],
            deep: longer.map_or_else(|| stars.clone(), |w| w.positions),
            stars,
        })
    }
}
fn photometry(src: &QualityFrame, dst: &QualityFrame, matches: &[Match]) -> Option<f64> {
    let mut scales: Vec<_> = matches
        .iter()
        .filter_map(|m| {
            let a = &src.stars[m.source];
            let b = &dst.stars[m.reference];
            (a.peak_adu <= 62000. && b.peak_adu <= 62000. && a.flux_adu > 0. && b.flux_adu > 0.)
                .then_some(b.flux_adu / a.flux_adu)
        })
        .filter(|a| a.is_finite() && *a > 0.)
        .collect();
    (scales.len() >= 10).then(|| median(&mut scales))
}
fn total_flux_ratio(src: &QualityFrame, dst: &QualityFrame, matches: &[Match]) -> Option<f64> {
    let reverse: Vec<_> = matches
        .iter()
        .map(|m| Match {
            source: m.reference,
            reference: m.source,
            residual_px: m.residual_px,
        })
        .collect();
    photometry(dst, src, &reverse)
}

/// Extrapolate angular velocity and the transformed sensor center using header times.
/// Predictions only seed matching; they never become unfitted chained transforms.
fn predict(history: &[(i64, Transform)], time: i64, center: [f64; 2]) -> Transform {
    let &(last_time, last) = history.last().unwrap();
    if history.len() < 2 {
        return last;
    }
    let (previous_time, previous) = history[history.len() - 2];
    let dt = last_time - previous_time;
    if dt == 0 {
        return last;
    }
    let factor = (time - last_time) as f64 / dt as f64;
    if !(-0.1..=4.).contains(&factor) {
        return last;
    }
    let delta = (last.theta_rad - previous.theta_rad)
        .sin()
        .atan2((last.theta_rad - previous.theta_rad).cos());
    let theta = last.theta_rad + factor * delta;
    let p = previous.apply(center);
    let q = last.apply(center);
    let desired = [q[0] + factor * (q[0] - p[0]), q[1] + factor * (q[1] - p[1])];
    let mut result = Transform {
        scale_factor: 1.,
        theta_rad: theta,
        tx_px: 0.,
        ty_px: 0.,
    };
    let rotated = result.apply(center);
    result.tx_px = desired[0] - rotated[0];
    result.ty_px = desired[1] - rotated[1];
    result
}

fn empty(f: &QualityFrame, session: usize, anchor: &str) -> RegisteredFrame {
    RegisteredFrame {
        geometric_correction: None,
        source_to_photometric_anchor_correction: None,
        file_name: f.file_name.clone(),
        session_id: session,
        quality_accepted: f.accepted,
        registration_attempted: false,
        registration_success: false,
        accepted: false,
        rejection_reasons: f.rejection_reasons.clone(),
        method: None,
        transform: None,
        rotation_deg: None,
        source_to_photometric_anchor: None,
        inlier_count: 0,
        inlier_rmse_px: None,
        matches: vec![],
        photometric_matches: vec![],
        match_reference: anchor.to_owned(),
        photometric_reference: None,
        matched_flux_ratio: None,
        exposure_corrected_transparency: None,
        photometric_scale: None,
        background_offset_adu: None,
        channel_offsets_adu: None,
        partial_field: None,
        normalized_noise_adu: None,
        final_weight: 0.,
        footprint: vec![],
        sky: None,
    }
}

pub fn register(
    input: &Path,
    min_coverage: f64,
    cancel: &AtomicBool,
) -> Result<RegistrationReport> {
    if !min_coverage.is_finite() || !(0.0..=1.).contains(&min_coverage) || min_coverage == 0. {
        return Err(Error::Invalid("Coverage must be finite and in (0,1]."));
    }
    register_with_reference(input, min_coverage, None, cancel)
}
pub fn register_with_reference(
    input: &Path,
    min_coverage: f64,
    reference: Option<&Path>,
    cancel: &AtomicBool,
) -> Result<RegistrationReport> {
    register_with_options(
        input,
        min_coverage,
        reference,
        RegistrationOptions::default(),
        cancel,
    )
}
pub fn register_with_options(
    input: &Path,
    min_coverage: f64,
    reference: Option<&Path>,
    mut options: RegistrationOptions,
    cancel: &AtomicBool,
) -> Result<RegistrationReport> {
    if !min_coverage.is_finite() || !(0.0..=1.).contains(&min_coverage) || min_coverage == 0. {
        return Err(Error::Invalid("Coverage must be finite and in (0,1]."));
    }
    let start = Instant::now();
    let inspection = crate::inspect::inspect(input, InspectConfig::default(), cancel)?;
    let root = if input.is_dir() {
        input
    } else {
        input.parent().unwrap_or(Path::new("."))
    };
    let (selected, external) = resolve_reference(&inspection.frames, root, reference)?;
    if external.is_some() {
        options.reference_strategy = ReferenceStrategy::ChronologicalMedian;
    }
    // The external stage has the final word and decides on partial frames.
    let partial = external.is_some() && options.partial_frames;
    if partial {
        options.partial_frames = false;
    }
    let witness = WitnessCatalog {
        root,
        calibrations: &[],
    };
    let anchor = outside_grid_anchor(&inspection.frames, selected, external.is_some());
    let mut report = register_catalogs_witnessed(
        inspection,
        min_coverage,
        anchor,
        options,
        cancel,
        start,
        None,
        Some(&witness),
    )?;
    if let Some(path) = external {
        crate::reference::align_with_progress(
            &mut report,
            crate::reference::Template::File(&path),
            cancel,
            None,
            partial,
            Some(&witness),
        )?;
    }
    Ok(report)
}
impl RegistrationReport {
    /// Owner line for experimental partial frames, after integration.
    pub fn partial_summary(&self) -> Option<String> {
        use std::sync::atomic::Ordering::Relaxed;
        let fields: Vec<_> = self
            .frames
            .iter()
            .filter_map(|f| f.partial_field.as_ref())
            .collect();
        let used: usize = fields.iter().map(|p| p.tiles_used.load(Relaxed)).sum();
        let refused: usize = fields.iter().map(|p| p.tiles_refused.load(Relaxed)).sum();
        let refused_for = |reason| -> usize { fields.iter().map(|p| p.refused_for(reason)).sum() };
        (!fields.is_empty()).then(|| {
            format!(
                "Partial frames: {} frames joined {used} of {} tiles; refused {} for too few stars, {} inaccurate, {} for single stars.",
                fields.len(),
                used + refused,
                refused_for("too_few_stars"),
                refused_for("rms_above_1px"),
                refused_for("star_above_3px"),
            )
        })
    }

    /// Accepted sources reconstructed without demosaicing (monochrome sets).
    /// Light sets measured on their own anchor (ADR-156), for the console
    /// and Results.
    pub fn set_photometry_summary(&self) -> Option<String> {
        (!self.set_photometry.is_empty()).then(|| {
            self.set_photometry
                .iter()
                .map(|s| match s.link_scale {
                    Some(scale) => format!(
                        "{}: {} frames on their own set's anchor, linked to the reference by {} stars (scale {scale:.4})",
                        s.label, s.frames, s.link_pairs
                    ),
                    None => format!(
                        "{}: {} frames on their own set's anchor, no link to the reference filter (one master per set or filter only)",
                        s.label, s.frames
                    ),
                })
                .collect::<Vec<_>>()
                .join("; ")
        })
    }

    pub fn camera_summary(&self) -> Option<String> {
        (self.camera_groups.len() > 1).then(|| {
            self.camera_groups
                .iter()
                .map(|c| {
                    let scale = c.brightness_scale.map_or_else(
                        || format!("refused: {}", c.refused.as_deref().unwrap_or("no match")),
                        |s| format!("brightness × {s:.3}"),
                    );
                    format!(
                        "{}×{}{} {}/{} frames, {scale}",
                        c.width,
                        c.height,
                        if c.mirrored { " mirrored" } else { "" },
                        c.stacked,
                        c.frames
                    )
                })
                .collect::<Vec<_>>()
                .join(" · ")
        })
    }
}

/// Light sets with declared layouts; see `inspect::inspect_sets`.
pub fn register_sets_with_options(
    sets: &[crate::inspect::InputSet],
    min_coverage: f64,
    reference: Option<&Path>,
    options: RegistrationOptions,
    cancel: &AtomicBool,
) -> Result<(RegistrationReport, std::path::PathBuf)> {
    if !min_coverage.is_finite() || !(0.0..=1.).contains(&min_coverage) || min_coverage == 0. {
        return Err(Error::Invalid("Coverage must be finite and in (0,1]."));
    }
    let start = Instant::now();
    let (inspection, root) =
        crate::inspect::inspect_sets(sets, InspectConfig::default(), cancel, None)?;
    fulgur_core::telemetry::timeline("inspection done");
    let calibrations: Vec<_> = sets.iter().map(|s| s.calibration.clone()).collect();
    register_inspected_with_calibration(
        inspection,
        root,
        min_coverage,
        reference,
        options,
        cancel,
        start,
        None,
        &calibrations,
    )
}

/// Continue an immutable inspection without rereading/reclassifying the dataset.
/// Callers must validate the source identities again after a user review pause.
#[allow(clippy::too_many_arguments)] // Existing continuation context plus a UI-only observer.
pub fn register_inspected_with_options(
    inspection: InspectReport,
    root: std::path::PathBuf,
    min_coverage: f64,
    reference: Option<&Path>,
    options: RegistrationOptions,
    cancel: &AtomicBool,
    start: Instant,
    progress: Option<&(dyn Fn(usize, usize) + Sync)>,
) -> Result<(RegistrationReport, std::path::PathBuf)> {
    register_inspected_with_calibration(
        inspection,
        root,
        min_coverage,
        reference,
        options,
        cancel,
        start,
        progress,
        &[],
    )
}

/// As `register_inspected_with_options`, with each light set's master dark
/// (indexed like `InspectReport::light_sets`) for the extended witness catalog.
#[allow(clippy::too_many_arguments)] // Continuation context, observer and calibration.
pub fn register_inspected_with_calibration(
    inspection: InspectReport,
    root: std::path::PathBuf,
    min_coverage: f64,
    reference: Option<&Path>,
    mut options: RegistrationOptions,
    cancel: &AtomicBool,
    start: Instant,
    progress: Option<&(dyn Fn(usize, usize) + Sync)>,
    calibrations: &[Option<std::sync::Arc<crate::calibration::Calibration>>],
) -> Result<(RegistrationReport, std::path::PathBuf)> {
    if !min_coverage.is_finite() || !(0.0..=1.).contains(&min_coverage) || min_coverage == 0. {
        return Err(Error::Invalid("Coverage must be finite and in (0,1]."));
    }
    fulgur_core::check_cancelled(cancel)?;
    let (selected, external) = resolve_reference(&inspection.frames, &root, reference)?;
    let witness = WitnessCatalog {
        root: &root,
        calibrations,
    };
    let grid = match (&external, selected) {
        (Some(path), _) => cameras::Grid::External(path),
        (None, Some(frame)) => cameras::Grid::Member(frame),
        (None, None) => cameras::Grid::Automatic,
    };
    let template = external.as_deref().map(crate::reference::Template::File);
    if template.is_some() {
        options.reference_strategy = ReferenceStrategy::ChronologicalMedian;
    }
    let partial = template.is_some() && options.partial_frames;
    if partial {
        options.partial_frames = false;
    }
    // Planned work of one pass, with the `split` light sets as cameras of their own.
    let planned = |frames: &[QualityFrame], split: &[usize]| -> Result<usize> {
        let mut members = cameras::groups(frames, split);
        cameras::lead_with(&mut members, selected);
        let several = members.len() > 1;
        let scheduled = |frame: &QualityFrame| frame.accepted || options.diagnose_excluded;
        // Only scheduled candidates count as row work; excluded report rows remain intact.
        let candidates = frames.iter().filter(|f| scheduled(f)).count();
        // Without an outside grid the leading camera has no second row pass.
        let leading: Vec<_> = if several && template.is_none() {
            members[0].iter().map(|&i| frames[i].clone()).collect()
        } else {
            vec![]
        };
        let lead = leading.iter().filter(|f| scheduled(f)).count();
        // One native row per candidate and, for a single camera, one native
        // finalization step. A second grid adds read/catalog/bridge steps, a
        // second row pass and finalization (`cameras::attempt_work`).
        let total = if template.is_some() {
            candidates * 2 + if several { 4 * members.len() } else { 5 }
        } else if several {
            (candidates - lead) * 2 + lead + 4 * (members.len() - 1)
        } else {
            candidates + 1
        };
        let total = total
            + if options.geometric_correction {
                candidates
            } else {
                0
            };
        let chooser = if several { leading.as_slice() } else { frames };
        // A leading camera without capture times gives way to the next one.
        let multi_night =
            if selected.is_none() && options.reference_strategy == ReferenceStrategy::Auto {
                match auto_reference::nights(chooser) {
                    Ok(nights) => nights.len() > 1,
                    Err(_) if several => false,
                    Err(error) => return Err(error),
                }
            } else {
                false
            };
        Ok(total
            + if multi_night {
                let chosen = if several { lead } else { candidates };
                chosen * (1 + usize::from(options.geometric_correction)) + 2
            } else {
                0
            })
    };
    // The last step decides whether a stranded light set needs a second pass,
    // so the bar shows 100 % only when registration is over.
    let total = planned(&inspection.frames, &[])? + 1;
    let progress = progress.map(|observer| {
        observer(0, total);
        RegistrationProgress {
            completed: Mutex::new(0),
            total: AtomicUsize::new(total),
            observer,
        }
    });
    let run = |inspection: InspectReport,
               split: &[usize],
               keep: Option<&str>|
     -> Result<RegistrationReport> {
        if cameras::groups(&inspection.frames, split).len() > 1 {
            return cameras::register(
                inspection,
                grid,
                split,
                keep,
                min_coverage,
                options,
                partial,
                cancel,
                start,
                progress.as_ref(),
                &witness,
            );
        }
        let anchor = outside_grid_anchor(&inspection.frames, selected, template.is_some());
        let mut report = register_catalogs_witnessed(
            inspection,
            min_coverage,
            anchor,
            options,
            cancel,
            start,
            progress.as_ref(),
            Some(&witness),
        )?;
        if let Some(progress) = &progress {
            progress.advance();
        }
        if let Some(template) = template {
            crate::reference::align_with_progress(
                &mut report,
                template,
                cancel,
                progress.as_ref(),
                partial,
                Some(&witness),
            )?;
        }
        Ok(report)
    };
    let report = run(inspection, &[], None)?;
    let split = cameras::stranded(&report);
    let frames = &report.inspection.frames;
    if cameras::groups(frames, &split) == cameras::groups(frames, &[]) {
        if let Some(progress) = &progress {
            progress.advance();
        }
        return Ok((report, root));
    }
    // Other optics on an equal sensor: the set tries again as its own camera.
    // It keeps what that pass found for it, and the other sets keep their first
    // rows; the whole second pass stands only where it stacks still more. The
    // first anchor stays, though the stranded set left its camera (ADR-128).
    if let (Some(progress), Ok(work)) = (&progress, planned(frames, &split)) {
        progress.next_pass(work);
    }
    let second = match run(
        report.inspection.clone(),
        &split,
        Some(&report.reference_anchor),
    ) {
        Ok(second) => second,
        Err(Error::Cancelled) => return Err(Error::Cancelled),
        Err(_) => return Ok((report, root)),
    };
    let taken = cameras::taken_sets(&report, &second, &split);
    let gained = second
        .frames
        .iter()
        .zip(&second.inspection.frames)
        .filter(|(f, q)| f.accepted && taken.contains(&q.light_set))
        .count();
    // The stranded sets stacked nothing in the first pass.
    let merged_count = report.accepted_count + gained;
    let chosen = if second.accepted_count > merged_count {
        second
    } else if gained > 0 {
        cameras::merge_stranded(report, second, &taken, cancel, start)?
    } else {
        report
    };
    Ok((chosen, root))
}
#[cfg(test)]
fn register_catalogs(
    inspection: InspectReport,
    min_coverage: f64,
    selected: Option<usize>,
    cancel: &AtomicBool,
    start: Instant,
) -> Result<RegistrationReport> {
    register_catalogs_observed(
        inspection,
        min_coverage,
        selected,
        RegistrationOptions {
            reference_strategy: ReferenceStrategy::ChronologicalMedian,
            ..Default::default()
        },
        cancel,
        start,
        None,
    )
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn register_catalogs_observed(
    inspection: InspectReport,
    min_coverage: f64,
    selected: Option<usize>,
    options: RegistrationOptions,
    cancel: &AtomicBool,
    start: Instant,
    progress: Option<&RegistrationProgress<'_>>,
) -> Result<RegistrationReport> {
    register_catalogs_witnessed(
        inspection,
        min_coverage,
        selected,
        options,
        cancel,
        start,
        progress,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn register_catalogs_witnessed(
    inspection: InspectReport,
    min_coverage: f64,
    selected: Option<usize>,
    options: RegistrationOptions,
    cancel: &AtomicBool,
    start: Instant,
    progress: Option<&RegistrationProgress<'_>>,
    witness: Option<&WitnessCatalog<'_>>,
) -> Result<RegistrationReport> {
    check_cancelled(cancel)?;
    if selected.is_none() && options.reference_strategy == ReferenceStrategy::Auto {
        let nights = auto_reference::nights(&inspection.frames)?;
        if nights.len() > 1 {
            let preliminary_start = Instant::now();
            let preliminary = register_catalogs_witnessed(
                inspection,
                min_coverage,
                None,
                RegistrationOptions {
                    reference_strategy: ReferenceStrategy::ChronologicalMedian,
                    ..options
                },
                cancel,
                start,
                progress,
                witness,
            )?;
            let preliminary_seconds = preliminary_start.elapsed().as_secs_f64();
            if let Some(p) = progress {
                p.advance();
            }
            let (winner, mut selection) = auto_reference::choose(&preliminary, &nights, cancel)?;
            selection.preliminary_seconds = preliminary_seconds;
            if let Some(p) = progress {
                p.advance();
            }
            let same = preliminary.frames[winner].file_name == preliminary.reference_anchor;
            let mut result = if same {
                if let Some(p) = progress {
                    let count = preliminary
                        .inspection
                        .frames
                        .iter()
                        .filter(|f| f.accepted || options.diagnose_excluded)
                        .count()
                        * (1 + usize::from(options.geometric_correction));
                    for _ in 0..count {
                        check_cancelled(cancel)?;
                        p.advance();
                    }
                }
                preliminary
            } else {
                let mapped =
                    |r: &RegistrationReport| r.frames.iter().filter(|f| f.accepted).count();
                let preliminary_mapped = mapped(&preliminary);
                let preliminary_anchor = preliminary
                    .frames
                    .iter()
                    .position(|f| f.file_name == preliminary.reference_anchor)
                    .unwrap();
                let options = RegistrationOptions {
                    reference_strategy: ReferenceStrategy::ChronologicalMedian,
                    ..options
                };
                let chosen = register_catalogs_witnessed(
                    preliminary.inspection,
                    min_coverage,
                    Some(winner),
                    options,
                    cancel,
                    start,
                    progress,
                    witness,
                )?;
                // The coverage estimate cannot see catalog depth: an anchor with
                // ten times the exposure may share too few unsaturated stars with
                // short frames or another night. Never trade mapped frames for
                // predicted area; keep the preliminary reference then (ADR-085).
                if mapped(&chosen) < preliminary_mapped {
                    selection.kept_preliminary_reference = true;
                    selection.chosen_reference_mapped_frames = Some(mapped(&chosen));
                    register_catalogs_witnessed(
                        chosen.inspection,
                        min_coverage,
                        Some(preliminary_anchor),
                        options,
                        cancel,
                        start,
                        None,
                        witness,
                    )?
                } else {
                    chosen
                }
            };
            // Frames that stay unmapped are reported; the mapped ones still stack.
            selection.unmapped_frames = nights
                .iter()
                .flatten()
                .filter(|&&i| !result.frames[i].accepted)
                .count();
            selection.unmapped_nights = nights
                .iter()
                .filter(|night| !night.iter().any(|&i| result.frames[i].accepted))
                .count();
            result.reference_selection = "automatic multi-night quality, plate scale and coverage; chronological median for single night";
            result.auto_reference = Some(selection);
            result.wall_seconds = start.elapsed().as_secs_f64();
            result.registration_seconds = result.wall_seconds - result.inspection_seconds;
            result.peak_rss_bytes = fulgur_core::telemetry::peak_rss_bytes();
            return Ok(result);
        }
    }
    let mut profile = crate::registration_timing::Timer::new();
    let quality = &inspection.frames;
    if !quality.iter().any(|f| f.accepted) {
        return Err(Error::Invalid(
            "No quality-accepted reference anchor available.",
        ));
    }
    let timed = quality.iter().all(|f| timestamp(f).is_some());
    let mut order: Vec<_> = (0..quality.len()).collect();
    if timed {
        order.sort_by_key(|&i| timestamp(&quality[i]));
    }
    let mut groups: Vec<Vec<usize>> = vec![];
    let mut previous = None;
    for i in order {
        let time = timestamp(&quality[i]);
        if groups.is_empty()
            || (timed
                && previous
                    .zip(time)
                    .is_some_and(|(p, t)| t - p > 6 * 3600 * 1_000_000))
        {
            groups.push(vec![]);
        }
        groups.last_mut().unwrap().push(i);
        previous = time;
    }
    let all: Vec<_> = groups.iter().flatten().copied().collect();
    let master = if let Some(i) = selected {
        if i >= quality.len() || !quality[i].accepted {
            return Err(Error::Invalid("Invalid selected reference."));
        }
        i
    } else {
        select_anchor(&all, quality)?
    };
    let master_info = quality[master].frame.as_ref().unwrap();
    let reference_anchor = quality[master].file_name.clone();
    let mut rows: Vec<Option<RegisteredFrame>> = (0..quality.len()).map(|_| None).collect();
    let master_matcher = geometry::Matcher::new(&quality[master].stars, false);
    let mut sessions = Vec::new();
    let mut fit_details = crate::registration_timing::NativeFitDetails {
        timing_scope: Some("overlapping walk task elapsed; phase intervals remain wall time"),
        ..Default::default()
    };
    profile.lap("selection_and_session_setup");
    for (sid, group) in groups.iter().enumerate() {
        check_cancelled(cancel)?;
        if !group.iter().any(|&i| quality[i].accepted) {
            sessions.push(Session {
                id: sid,
                anchor: String::new(),
                frame_count: group.len(),
                to_master: None,
                anchor_inliers: 0,
                anchor_rmse_px: None,
                matches: vec![],
            });
            for &i in group {
                check_cancelled(cancel)?;
                let mut row = empty(&quality[i], sid, "");
                row.rejection_reasons
                    .push("registration_skipped_no_accepted_session_anchor".into());
                rows[i] = Some(row);
                if options.diagnose_excluded {
                    if let Some(progress) = progress {
                        progress.advance();
                    }
                }
            }
            continue;
        }
        let anchor = if group.contains(&master) {
            master
        } else {
            select_anchor(group, quality)?
        };
        let anchor_name = &quality[anchor].file_name;
        let session_matcher = geometry::Matcher::new(&quality[anchor].stars, false);
        let matcher = if anchor == master {
            &master_matcher
        } else {
            &session_matcher
        };
        let bridge_start = Instant::now();
        let bridge = if anchor == master {
            Some(geometry::Solution {
                transform: Transform::IDENTITY,
                pairs: geometry::correspondences(
                    &quality[anchor].stars,
                    &quality[master].stars,
                    Transform::IDENTITY,
                    1.5,
                ),
                rmse: 0.,
                method: "master_anchor",
            })
        } else {
            master_matcher.solve(&quality[anchor].stars, None, cancel)?
        };
        fit_details.session_bridge_geometry_seconds += bridge_start.elapsed().as_secs_f64();
        let photometry_start = Instant::now();
        let bridge_ratio = bridge
            .as_ref()
            .and_then(|b| total_flux_ratio(&quality[anchor], &quality[master], &b.pairs));
        let bridge_scale = bridge
            .as_ref()
            .and_then(|b| photometry(&quality[anchor], &quality[master], &b.pairs));
        fit_details.photometry_seconds += photometry_start.elapsed().as_secs_f64();
        sessions.push(Session {
            id: sid,
            anchor: anchor_name.clone(),
            frame_count: group.len(),
            to_master: bridge.as_ref().map(|b| b.transform),
            anchor_inliers: bridge.as_ref().map_or(0, |b| b.pairs.len()),
            anchor_rmse_px: bridge.as_ref().map(|b| b.rmse),
            matches: bridge.as_ref().map(|b| b.pairs.clone()).unwrap_or_default(),
        });
        // Predict outwards from the anchor, but every solution is fitted directly to the session anchor.
        let pos = group.iter().position(|i| *i == anchor).unwrap();
        let walks = [
            group[pos..].to_vec(),
            group[..pos].iter().rev().copied().collect(),
        ];
        let run_walk = |walk: Vec<usize>| -> Result<_> {
            let walk_start = Instant::now();
            let mut walk_rows = Vec::with_capacity(walk.len());
            let mut fit_details = crate::registration_timing::NativeFitDetails::default();
            let mut history = vec![(
                timestamp(&quality[anchor]).unwrap_or(anchor as i64),
                Transform::IDENTITY,
            )];
            for i in walk {
                check_cancelled(cancel)?;
                let f = &quality[i];
                if !f.accepted && !options.diagnose_excluded {
                    walk_rows.push((i, empty(f, sid, anchor_name)));
                    continue;
                }
                let time = timestamp(f).unwrap_or(i as i64);
                let prediction = Some(predict(
                    &history,
                    time,
                    [
                        master_info.width as f64 / 2.,
                        master_info.height as f64 / 2.,
                    ],
                ));
                let mut row = empty(f, sid, anchor_name);
                row.registration_attempted = true;
                // Light sets may differ in target name, filter, gain, encoding
                // and colour layout (stars are measured in sensor coordinates
                // either way); the sensor dimensions must match the anchor and
                // the stars decide whether a frame shows the same sky.
                let same_sensor = f.frame.as_ref().is_some_and(|info| {
                    info.width == master_info.width
                        && info.height == master_info.height
                        && info.mosaic != fulgur_core::Mosaic::Unknown
                });
                let geometry_start = Instant::now();
                let local = if !same_sensor || f.stars.len() < 10 || bridge.is_none() {
                    None
                } else if i == anchor {
                    Some(geometry::Solution {
                        transform: Transform::IDENTITY,
                        pairs: geometry::correspondences(
                            &f.stars,
                            &quality[anchor].stars,
                            Transform::IDENTITY,
                            1.5,
                        ),
                        rmse: 0.,
                        method: "session_anchor",
                    })
                } else {
                    matcher.solve(&f.stars, prediction, cancel)?
                };
                let geometry_seconds = geometry_start.elapsed().as_secs_f64();
                fit_details
                    .frame_fits
                    .push(crate::registration_timing::FrameFitTiming {
                        source_index: i,
                        wall_seconds: geometry_seconds,
                        method: local.as_ref().map(|s| s.method),
                    });
                if let (Some(solution), Some(bridge)) = (local, bridge.as_ref()) {
                    if i != anchor && f.accepted {
                        history.push((time, solution.transform));
                    }
                    let t = bridge.transform.compose(solution.transform);
                    let direct_start = Instant::now();
                    let direct =
                        geometry::correspondences(&f.stars, &quality[master].stars, t, 1.5);
                    fit_details.direct_master_matches_seconds +=
                        direct_start.elapsed().as_secs_f64();
                    let photometry_start = Instant::now();
                    let (scale, ratio, provenance) =
                        if let Some(a) = photometry(f, &quality[master], &direct) {
                            (
                                Some(a),
                                total_flux_ratio(f, &quality[master], &direct),
                                "direct_master_matches",
                            )
                        } else {
                            (
                                photometry(f, &quality[anchor], &solution.pairs)
                                    .zip(bridge_scale)
                                    .map(|(a, b)| a * b),
                                total_flux_ratio(f, &quality[anchor], &solution.pairs)
                                    .zip(bridge_ratio)
                                    .map(|(a, b)| a * b),
                                "session_bridge_product",
                            )
                        };
                    fit_details.photometry_seconds += photometry_start.elapsed().as_secs_f64();
                    row.registration_success = true;
                    row.method = Some(solution.method);
                    row.transform = Some(t);
                    row.rotation_deg = Some(t.theta_rad.to_degrees());
                    row.inlier_count = solution.pairs.len();
                    row.inlier_rmse_px = Some(solution.rmse);
                    row.matches = solution.pairs;
                    if provenance == "direct_master_matches" {
                        row.photometric_matches = direct;
                    }
                    if let Some((a, s)) = scale.zip(ratio) {
                        let exposure = f.frame.as_ref().unwrap().exposure_seconds;
                        let transmission = s * master_info.exposure_seconds / exposure;
                        let offset = quality[master].background_adu.unwrap()
                            - a * f.background_adu.unwrap_or(0.);
                        let noise = a * f.noise_adu.unwrap_or(0.);
                        let hfr = f.hfr_px.unwrap_or(0.);
                        let weight = 1. / (noise * noise * hfr * hfr);
                        if [a, s, transmission, offset, noise, weight]
                            .iter()
                            .all(|v| v.is_finite())
                            && noise > 0.
                            && weight > 0.
                        {
                            row.photometric_reference = Some(provenance);
                            row.photometric_scale = Some(a);
                            row.matched_flux_ratio = Some(s);
                            row.exposure_corrected_transparency = Some(transmission);
                            row.background_offset_adu = Some(offset);
                            row.channel_offsets_adu = channel_offsets(a, f, &quality[master]);
                            row.normalized_noise_adu = Some(noise);
                            row.accepted = f.accepted;
                            if row.accepted {
                                row.final_weight = weight;
                                row.footprint =
                                    geometry::footprint(t, master_info.width, master_info.height);
                            }
                        } else {
                            row.rejection_reasons
                                .push("invalid_photometric_normalization".into());
                        }
                    } else {
                        row.rejection_reasons
                            .push("insufficient_photometric_matches".into());
                    }
                } else {
                    row.rejection_reasons.push("registration_failed".into());
                }
                walk_rows.push((i, row));
                if let Some(progress) = progress {
                    progress.advance();
                }
            }
            Ok((walk_rows, fit_details, walk_start.elapsed().as_secs_f64()))
        };
        let [forward, backward] = walks;
        let walk_group_start = Instant::now();
        // Both walks retain their original independent prediction histories.
        // join waits for both tasks even when either returns an error.
        let (forward, backward) = rayon::join(|| run_walk(forward), || run_walk(backward));
        fit_details.walk_group_wall_seconds += walk_group_start.elapsed().as_secs_f64();
        for result in [forward, backward] {
            let (walk_rows, detail, elapsed) = result?;
            fit_details.walk_task_elapsed_seconds += elapsed;
            fit_details.direct_master_matches_seconds += detail.direct_master_matches_seconds;
            fit_details.photometry_seconds += detail.photometry_seconds;
            fit_details.frame_fits.extend(detail.frame_fits);
            for (i, row) in walk_rows {
                rows[i] = Some(row);
            }
        }
    }
    profile.lap("native_geometry_and_photometry");
    let mut frames: Vec<_> = rows.into_iter().map(Option::unwrap).collect();
    // Frames of another filter that share no stars with the photometric
    // anchor, through an anchor of their own light set (ADR-156).
    let labels: Vec<String> = inspection
        .light_sets
        .iter()
        .map(|s| s.label.clone())
        .collect();
    let set_brightness = set_photometry::rescue(
        &mut frames.iter_mut().collect::<Vec<_>>(),
        quality,
        master,
        witness,
        &|row| {
            row.transform
                .map(|t| geometry::footprint(t, master_info.width, master_info.height))
                .unwrap_or_default()
        },
        &|_| 1.,
        &labels,
        cancel,
    );
    if options.geometric_correction {
        use rayon::prelude::*;
        let extended = witness.and_then(|w| w.deep_stars(&quality[master], cancel));
        frames.par_iter_mut().zip(quality.par_iter()).try_for_each(
            |(row, source)| -> Result<()> {
                apply_native_correction(
                    row,
                    source,
                    &quality[master],
                    extended.as_ref(),
                    witness,
                    options.partial_frames,
                    options.accuracy,
                    cancel,
                )?;
                if source.accepted || options.diagnose_excluded {
                    if let Some(progress) = progress {
                        progress.advance();
                    }
                }
                Ok(())
            },
        )?;
        profile.lap("geometric_correction_validation");
    }
    let max = frames.iter().map(|r| r.final_weight).fold(0., f64::max);
    if max <= 0. {
        return Err(Error::Invalid(
            "No registered frames with usable final weights.",
        ));
    }
    for r in &mut frames {
        r.final_weight /= max;
    }
    let polygons: Vec<_> = frames
        .iter()
        .filter(|f| f.accepted)
        .map(|f| (f.footprint.clone(), f.final_weight))
        .collect();
    profile.lap("weight_normalization_and_footprints");
    let mut common = polygons[0].0.clone();
    for (p, _) in polygons.iter().skip(1) {
        common = geometry::intersection(common, p);
    }
    let common_intersection = geometry::canvas(&[(common.clone(), 1.)], 1., cancel)?;
    profile.lap("common_intersection_canvas");
    let coverage_optimized = geometry::canvas(&polygons, min_coverage, cancel)?;
    profile.lap("coverage_optimized_canvas");
    let accepted_count = frames.iter().filter(|r| r.accepted).count();
    let accepted_exposure_seconds = frames
        .iter()
        .zip(quality)
        .filter(|(r, _)| r.accepted)
        .map(|(_, f)| f.frame.as_ref().unwrap().exposure_seconds)
        .sum();
    let native = profile.finish();
    fit_details.bookkeeping_seconds = native.phases[1].wall_seconds
        - fit_details.walk_group_wall_seconds
        + fit_details.walk_task_elapsed_seconds
        - fit_details.session_bridge_geometry_seconds
        - fit_details.direct_master_matches_seconds
        - fit_details.photometry_seconds
        - fit_details
            .frame_fits
            .iter()
            .map(|f| f.wall_seconds)
            .sum::<f64>();
    let phase_profile = RegistrationProfile {
        native,
        native_fit_details: fit_details,
        external: None,
    };
    let wall_seconds = start.elapsed().as_secs_f64();
    let corrected = frames.iter().any(|f| {
        f.geometric_correction
            .as_ref()
            .is_some_and(|d| d.model.is_some())
            || f.method == Some("native_similarity_validated")
            || f.method == Some("native_similarity_recovered")
    });
    Ok(RegistrationReport {
        auto_reference: None,
        geometric_correction_enabled: options.geometric_correction,
        accuracy: options.accuracy,
        schema_version: 2,
        command: "register",
        reference_width: master_info.width,
        reference_height: master_info.height,
        external_reference: None,
        camera_groups: vec![],
        reference_anchor,
        reference_selection: if selected.is_some() {
            "manual_quality_accepted_input"
        } else {
            "DATE-OBS median of quality-accepted frames; earlier central frame for even count"
        },
        reference_date_obs: quality[master]
            .frame
            .as_ref()
            .and_then(|f| f.date_obs.clone()),
        frame_count: frames.len(),
        quality_accepted_count: inspection.accepted_count,
        diagnose_excluded: options.diagnose_excluded,
        registration_attempted_count: frames.iter().filter(|r| r.registration_attempted).count(),
        registration_skipped_count: frames.iter().filter(|r| !r.registration_attempted).count(),
        registration_success_count: frames.iter().filter(|r| r.registration_success).count(),
        accepted_count,
        rejected_count: frames.len() - accepted_count,
        accepted_exposure_seconds,
        wall_seconds,
        inspection_seconds: inspection.wall_seconds,
        registration_seconds: wall_seconds - inspection.wall_seconds,
        phase_profile,
        peak_rss_bytes: fulgur_core::telemetry::peak_rss_bytes(),
        intermediate_images_written: 0,
        transform_convention: if corrected {
            "source integer pixel centers -> master anchor: validated cubic model overrides its similarity seed; session/local rigid transforms are preliminary; a frame whose rigid mapping fails absolute whole-field validation may use a validated similarity (scale 0.30..3.333) or cubic fallback; zero-based top-down"
        } else {
            "source integer pixel centers -> master anchor: q=R(theta)*p+t; rigid scale=1; radians and pixels; zero-based top-down"
        },
        session_rule: if timed {
            "DATE-OBS chronological order; gap >6h starts session"
        } else {
            "single session fallback: at least one DATE-OBS missing or invalid; sorted source paths"
        },
        photometry_rule: "y=a*x+b[c] per colour channel c in source ADU; a=median(reference_flux/source_flux), one scale for all channels; b[c]=reference_sky[c]-a*source_sky[c]; flux_ratio=median(source_flux/reference_flux); transparency=flux_ratio*reference_exposure/source_exposure; no second exposure correction",
        weights_status: "final: inverse(normalized_noise^2 * HFR^2), maximum accepted weight=1",
        sessions,
        common_intersection_polygon: common,
        common_intersection,
        coverage_optimized,
        min_coverage,
        frames,
        sky: None,
        set_photometry: set_brightness,
        inspection,
    })
}
/// Sky offset of each colour channel: one star scale, the sky matched per
/// channel (ADR-102). None unless both frames have channel sky levels.
pub(crate) fn channel_offsets(
    scale: f64,
    source: &QualityFrame,
    target: &QualityFrame,
) -> Option<[f64; 3]> {
    let (source, target) = (
        source.channel_background_adu?,
        target.channel_background_adu?,
    );
    let offsets = std::array::from_fn(|c| target[c] - scale * source[c]);
    offsets
        .iter()
        .all(|v: &f64| v.is_finite())
        .then_some(offsets)
}

/// Direct source-to-master geometry replaces a failed or preliminary native
/// pose; photometry is re-established on its direct master matches. Returns
/// false (row unchanged) when that photometry is not usable.
fn adopt_direct_fit(
    row: &mut RegisteredFrame,
    source: &QualityFrame,
    target: &QualityFrame,
    transform: Transform,
    pairs: Vec<Match>,
    method: &'static str,
    footprint: Vec<[f64; 2]>,
) -> bool {
    let (Some(info), Some(target_info)) = (source.frame.as_ref(), target.frame.as_ref()) else {
        return false;
    };
    let scale = photometry(source, target, &pairs);
    let ratio = total_flux_ratio(source, target, &pairs);
    let values = scale.zip(ratio).and_then(|(a, s)| {
        let offset = target.background_adu? - a * source.background_adu?;
        let noise = a * source.noise_adu?;
        let hfr = source.hfr_px?;
        let weight = 1. / (noise * noise * hfr * hfr);
        let transparency = s * target_info.exposure_seconds / info.exposure_seconds;
        ([a, s, offset, noise, weight, transparency]
            .iter()
            .all(|v| v.is_finite())
            && noise > 0.
            && weight > 0.)
            .then_some((a, s, offset, noise, weight, transparency))
    });
    let Some((a, s, offset, noise, weight, transparency)) = values else {
        return false;
    };
    row.transform = Some(transform);
    row.rotation_deg = Some(transform.theta_rad.to_degrees());
    row.registration_success = true;
    row.accepted = source.accepted;
    row.rejection_reasons.retain(|r| {
        r != "registration_failed"
            && r != "insufficient_photometric_matches"
            && r != "invalid_photometric_normalization"
    });
    row.method = Some(method);
    row.inlier_count = pairs.len();
    row.inlier_rmse_px = Some(
        (pairs.iter().map(|m| m.residual_px.powi(2)).sum::<f64>() / pairs.len() as f64).sqrt(),
    );
    row.matches = pairs.clone();
    row.photometric_matches = pairs;
    row.match_reference = target.file_name.clone();
    row.photometric_reference = Some("direct_master_matches");
    row.photometric_scale = Some(a);
    row.matched_flux_ratio = Some(s);
    row.background_offset_adu = Some(offset);
    row.channel_offsets_adu = channel_offsets(a, source, target);
    row.normalized_noise_adu = Some(noise);
    row.exposure_corrected_transparency = Some(transparency);
    row.final_weight = if row.accepted { weight } else { 0. };
    row.footprint = if row.accepted { footprint } else { vec![] };
    true
}

#[allow(clippy::too_many_arguments)] // Frame, anchor, witness lists, their files and limits.
fn apply_native_correction(
    row: &mut RegisteredFrame,
    source: &QualityFrame,
    target: &QualityFrame,
    extended: Option<&Witnesses>,
    files: Option<&WitnessCatalog<'_>>,
    partial: bool,
    limits: AccuracyLimits,
    cancel: &AtomicBool,
) -> Result<()> {
    if !row.registration_attempted {
        return Ok(());
    }
    let Some(info) = source.frame.as_ref() else {
        return Ok(());
    };
    let target_info = target.frame.as_ref().unwrap();
    if info.width != target_info.width || info.height != target_info.height {
        row.geometric_correction =
            Some(crate::distortion::Decision::skipped("incompatible_source"));
        return Ok(());
    }
    let (mut decision, pairs) = crate::distortion::solve(
        &source.stars,
        &target.stars,
        row.transform,
        [info.width, info.height],
        cancel,
    )?;
    if let Some(model) = decision.model {
        if !adopt_direct_fit(
            row,
            source,
            target,
            model.base,
            pairs,
            "native_cubic_validated",
            model.footprint(),
        ) {
            decision.status = "skipped";
            decision.reason = "invalid_native_photometry";
            decision.model = None;
        }
    } else if row.transform.is_none() {
        // Shift and rotation found no consensus: use the external-reference
        // similarity search (30..333% scale) against the master anchor. The
        // result enters integration only through the absolute validation below.
        if let Some(fit) = geometry::solve_similarity(&source.stars, &target.stars, None, cancel)? {
            adopt_direct_fit(
                row,
                source,
                target,
                fit.transform,
                fit.pairs,
                "native_similarity_recovered",
                geometry::footprint(fit.transform, info.width, info.height),
            );
        }
    }
    // Nonlinear rejection never certifies the ordinary mapping. Require broad
    // absolute support before a requested geometry can enter integration.
    if row.accepted {
        use crate::distortion::validation;
        let model = decision.model;
        let t = row.transform.unwrap();
        let map = |p| model.map_or_else(|| t.apply(p), |m| m.forward(p));
        let (mut pairs, expected) = validation::witness_pool(
            &source.stars,
            &target.stars,
            [target_info.width, target_info.height],
            map,
        );
        pairs = limits.without_chance_pairs(&source.stars, &target.stars, pairs, map);
        let mut reference: &[crate::stars::Star] = &target.stars;
        let mut window = 12.;
        let mut reserved = if model.is_some() {
            pairs
                .iter()
                .filter(|m| m.source % 5 == 0)
                .map(|m| m.source)
                .collect()
        } else {
            validation::held_out(&source.stars, &pairs)
        };
        let covered = extended.map(|w| {
            validation::covered(
                &source.stars,
                &w.positions,
                [target_info.width, target_info.height],
                map,
            )
        });
        let mut assessment = validation::assess(
            &source.stars,
            &target.stars,
            &pairs,
            &reserved,
            crate::distortion::Field::sensor([info.width, info.height]),
            expected,
            covered,
            map,
            limits,
        );
        // Dense fields: two 250-star catalogs may share too few stars for the
        // accuracy witnesses although coverage is complete, or the 12 px
        // window pairs a star whose counterpart the other truncated catalog
        // lacks with a neighbour 10 px away (ADR-085). Then use the anchor's
        // precisely measured extended catalog with a 3 px window; the accuracy
        // limits are unchanged, so a misregistered frame still fails (errors
        // above 3 px leave too few witnesses, smaller ones exceed the RMSE).
        if let Some(w) = extended.filter(|_| {
            !assessment.accepted
                && matches!(
                    assessment.reason,
                    "insufficient_field_evidence" | "absolute_field_accuracy_failed"
                )
                && covered.is_some_and(|c| limits.covers(c, expected))
        }) {
            let (extended_pairs, _) = validation::witness_pool_within(
                &source.stars,
                &w.precise,
                [target_info.width, target_info.height],
                map,
                3.,
            );
            reserved = if model.is_some() {
                extended_pairs
                    .iter()
                    .filter(|m| m.source % 5 == 0)
                    .map(|m| m.source)
                    .collect()
            } else {
                validation::held_out(&source.stars, &extended_pairs)
            };
            pairs = extended_pairs;
            reference = &w.precise;
            window = 3.;
            assessment = validation::assess(
                &source.stars,
                reference,
                &pairs,
                &reserved,
                crate::distortion::Field::sensor([info.width, info.height]),
                expected,
                covered,
                map,
                limits,
            );
            assessment.extended_witnesses = true;
        }
        // The frame's own registration list may hold only the objects its
        // filter shows; its longer list, measured once more from its file,
        // names the stars both frames share. It only judges the mapping
        // already found, under the same limits (ADR-125).
        if let (Some(w), Some(files)) = (extended, files) {
            if !assessment.accepted
                && matches!(
                    assessment.reason,
                    "insufficient_field_evidence" | "insufficient_outer_field_evidence"
                )
                && !w.deep_precise.is_empty()
                && covered.is_some_and(|c| limits.covers(c, expected))
            {
                if let Some(longer) = files.longer_list(source, cancel) {
                    let size = [target_info.width, target_info.height];
                    let longer_covered = validation::covered(&longer, &w.positions, size, map);
                    // The anchor's precisely measured stars first, then its
                    // detections (about 1 px): their own error only adds to
                    // the measured one, so a map never passes through them more
                    // easily than it is accurate.
                    for witnesses in [&w.deep_precise, &w.positions] {
                        let (longer_pairs, longer_expected) =
                            validation::witness_pool_within(&longer, witnesses, size, map, 3.);
                        let longer_reserved = if model.is_some() {
                            longer_pairs
                                .iter()
                                .filter(|m| m.source % 5 == 0)
                                .map(|m| m.source)
                                .collect()
                        } else {
                            validation::held_out(&longer, &longer_pairs)
                        };
                        let mut judged = validation::assess(
                            &longer,
                            witnesses,
                            &longer_pairs,
                            &longer_reserved,
                            crate::distortion::Field::sensor([info.width, info.height]),
                            longer_expected,
                            Some(longer_covered),
                            map,
                            limits,
                        );
                        judged.extended_witnesses = true;
                        if judged.accepted {
                            assessment = judged;
                            break;
                        }
                    }
                }
            }
        }
        let mut refused = None;
        if !assessment.accepted {
            let extended_witnesses = assessment.extended_witnesses;
            match native_fallback(
                source,
                reference,
                &pairs,
                [info.width, info.height],
                window,
                extended,
                decision.candidate,
                limits,
            ) {
                Err(failure) => {
                    decision.fallback_failure = failure.reason;
                    refused = failure.similarity;
                }
                Ok((transform, model, mut fallback, method, witnesses)) => {
                    decision.initial_correction_reason = Some(assessment.reason);
                    fallback.extended_witnesses = extended_witnesses;
                    assessment = fallback;
                    decision.status = if model.is_some() {
                        "applied"
                    } else {
                        "skipped"
                    };
                    decision.reason = method;
                    decision.model = model;
                    let map = |p| model.map_or_else(|| transform.apply(p), |m| m.forward(p));
                    let mut checked = witnesses;
                    for m in &mut checked {
                        let s = &source.stars[m.source];
                        let r = &reference[m.reference];
                        let q = map([s.x_px, s.y_px]);
                        m.residual_px = (q[0] - r.x_px).hypot(q[1] - r.y_px);
                    }
                    row.transform = Some(transform);
                    row.rotation_deg = Some(transform.theta_rad.to_degrees());
                    row.method = Some(method);
                    row.inlier_count = checked.len();
                    row.inlier_rmse_px = assessment.rmse_px;
                    row.matches = checked;
                    row.match_reference = target.file_name.clone();
                    row.footprint = model.map_or_else(
                        || geometry::footprint(transform, info.width, info.height),
                        |m| m.footprint(),
                    );
                }
            }
        }
        if !assessment.accepted && partial {
            let t_rigid = row.transform.unwrap();
            let kind = if refused.is_some() {
                "similarity"
            } else {
                "rigid"
            };
            let (t, witnesses) = refused.unwrap_or((t_rigid, pairs));
            let stars: Vec<_> = witnesses
                .iter()
                .map(|m| {
                    let s = &source.stars[m.source];
                    let r = &reference[m.reference];
                    let q = t.apply([s.x_px, s.y_px]);
                    [r.x_px, r.y_px, (q[0] - r.x_px).hypot(q[1] - r.y_px)]
                })
                .collect();
            if PartialField::admissible(&stars, limits) {
                row.transform = Some(t);
                row.rotation_deg = Some(t.theta_rad.to_degrees());
                row.method = Some("native_partial_field");
                row.footprint = geometry::footprint(t, info.width, info.height);
                row.partial_field =
                    Some(std::sync::Arc::new(PartialField::new(stars, kind, limits)));
                decision.status = "partial";
                decision.reason = "accurate_tiles_only";
                decision.model = None;
                decision.registration_validation = Some(assessment);
                row.geometric_correction = Some(decision);
                return Ok(());
            }
            row.accepted = false;
            row.registration_success = false;
            row.final_weight = 0.;
            row.footprint.clear();
            row.rejection_reasons
                .push(format!("native_{}", assessment.reason));
            decision.status = "rejected";
            decision.model = None;
        } else if !assessment.accepted {
            row.accepted = false;
            row.registration_success = false;
            row.final_weight = 0.;
            row.footprint.clear();
            row.rejection_reasons
                .push(format!("native_{}", assessment.reason));
            decision.status = "rejected";
            decision.model = None;
        } else if decision.model.is_none() && decision.initial_correction_reason.is_none() {
            decision.reason = "not_needed_absolute_field_accuracy_confirmed";
        }
        decision.registration_validation = Some(assessment);
    }
    row.geometric_correction = Some(decision);
    Ok(())
}

type Fallback = (
    Transform,
    Option<crate::distortion::Warp>,
    crate::distortion::FieldValidation,
    &'static str,
    Vec<Match>,
);

/// Why no fallback passed, and the scale fit a partial frame can still use.
struct Refused {
    reason: Option<&'static str>,
    similarity: Option<(Transform, Vec<Match>)>,
}

/// A rigid pose can fail whole-field validation when focus, temperature or
/// refraction changes the plate scale between nights. Then try a similarity
/// in the supported 30..333% range, and after it the bounded cubic candidate.
/// Both are fitted on training labels only and must pass the unchanged
/// absolute gates. Each candidate is measured by its own map: witnesses and
/// coverage found through the rigid pose would count the edge stars it
/// misplaces as missing (ADR-108). The window stays the same, and a star
/// outside the fit counts as held out.
#[allow(clippy::too_many_arguments)] // Fit context plus the run's limits.
fn native_fallback(
    source: &QualityFrame,
    reference: &[crate::stars::Star],
    pairs: &[Match],
    sensor: [usize; 2],
    window: f64,
    extended: Option<&Witnesses>,
    candidate: Option<crate::distortion::Warp>,
    limits: AccuracyLimits,
) -> std::result::Result<Fallback, Refused> {
    use crate::distortion::validation;
    let evidence = |map: &dyn Fn([f64; 2]) -> [f64; 2]| {
        let (pairs, expected) =
            validation::witness_pool_within(&source.stars, reference, sensor, map, window);
        let pairs = limits.without_chance_pairs(&source.stars, reference, pairs, map);
        let covered =
            extended.map(|w| validation::covered(&source.stars, &w.positions, sensor, map));
        (pairs, expected, covered)
    };
    let (src, dst) = (&source.stars, reference);
    let reserved = validation::held_out(src, pairs);
    let mut training: Vec<_> = pairs
        .iter()
        .filter(|m| !reserved.contains(&m.source))
        .cloned()
        .collect();
    let residual = |t: Transform, m: &Match| {
        let s = &src[m.source];
        let r = &dst[m.reference];
        let q = t.apply([s.x_px, s.y_px]);
        (q[0] - r.x_px).hypot(q[1] - r.y_px)
    };
    // One trimmed refit keeps a mismatched training pair from tilting the fit;
    // held-out witnesses are never trimmed.
    let similarity = geometry::procrustes(src, dst, &training, true).and_then(|first| {
        let mut residuals: Vec<_> = training.iter().map(|m| residual(first, m)).collect();
        let limit = (median(&mut residuals) * 3.).max(1.5);
        training.retain(|m| residual(first, m) <= limit);
        geometry::procrustes(src, dst, &training, true)
    });
    let mut failure = None;
    let mut refused_fit = None;
    if let Some(t) = similarity.filter(|t| fulgur_core::geometry_scale_supported(t.scale_factor)) {
        let (witnesses, expected, covered) = evidence(&|p| t.apply(p));
        let fitted: std::collections::BTreeSet<_> = training.iter().map(|m| m.source).collect();
        let held_out = witnesses
            .iter()
            .map(|m| m.source)
            .filter(|s| !fitted.contains(s))
            .collect();
        let assessment = validation::assess(
            src,
            dst,
            &witnesses,
            &held_out,
            crate::distortion::Field::sensor(sensor),
            expected,
            covered,
            |p| t.apply(p),
            limits,
        );
        if assessment.accepted {
            return Ok((
                t,
                None,
                assessment,
                "native_similarity_validated",
                witnesses,
            ));
        }
        failure = Some(assessment.reason);
        refused_fit = Some((t, witnesses));
    }
    let Some(model) =
        candidate.filter(|m| fulgur_core::geometry_scale_supported(m.base.scale_factor))
    else {
        return Err(Refused {
            reason: failure,
            similarity: refused_fit,
        });
    };
    let (witnesses, expected, covered) = evidence(&|p| model.forward(p));
    // The cubic candidate was trained on source indices not divisible by five.
    let cubic_reserved = witnesses
        .iter()
        .filter(|m| m.source % 5 == 0)
        .map(|m| m.source)
        .collect();
    let assessment = validation::assess(
        src,
        dst,
        &witnesses,
        &cubic_reserved,
        crate::distortion::Field::sensor(sensor),
        expected,
        covered,
        |p| model.forward(p),
        limits,
    );
    if assessment.accepted {
        Ok((
            model.base,
            Some(model),
            assessment,
            "native_cubic_fallback_validated",
            witnesses,
        ))
    } else {
        Err(Refused {
            reason: failure.or(Some(assessment.reason)),
            similarity: refused_fit,
        })
    }
}

pub fn csv(report: &RegistrationReport) -> String {
    let mut text = String::from(
        "\u{feff}file_name;quality_accepted;registration_success;accepted;session_id;method;rotation_deg;geometric_scale;tx_px;ty_px;inliers;rmse_px;flux_ratio;transparency;photometric_scale;background_offset_adu;normalized_noise_adu;final_weight;rejection_reasons;registration_attempted;offset_r_adu;offset_g_adu;offset_b_adu\n",
    );
    fn num(x: Option<f64>) -> String {
        x.map(|v| format!("{v:.8}")).unwrap_or_default()
    }
    fn quote(s: &str) -> String {
        let prefix = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
            "'"
        } else {
            ""
        };
        format!("\"{}{}\"", prefix, s.replace('"', "\"\""))
    }
    for f in &report.frames {
        let t = f.transform;
        text.push_str(
            &[
                quote(&f.file_name),
                f.quality_accepted.to_string(),
                f.registration_success.to_string(),
                f.accepted.to_string(),
                f.session_id.to_string(),
                f.method.unwrap_or("").into(),
                num(f.rotation_deg),
                num(f.transform.map(|t| t.scale_factor)),
                num(t.map(|t| t.tx_px)),
                num(t.map(|t| t.ty_px)),
                f.inlier_count.to_string(),
                num(f.inlier_rmse_px),
                num(f.matched_flux_ratio),
                num(f.exposure_corrected_transparency),
                num(f.photometric_scale),
                num(f.background_offset_adu),
                num(f.normalized_noise_adu),
                format!("{:.8}", f.final_weight),
                quote(&f.rejection_reasons.join("|")),
                f.registration_attempted.to_string(),
                num(f
                    .channel_offsets_adu
                    .map(|o| o[0])
                    .or(f.background_offset_adu)),
                num(f
                    .channel_offsets_adu
                    .map(|o| o[1])
                    .or(f.background_offset_adu)),
                num(f
                    .channel_offsets_adu
                    .map(|o| o[2])
                    .or(f.background_offset_adu)),
            ]
            .join(";"),
        );
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    mod auto_selection;
    mod distortion;
    mod parallel;
    mod selection;
    mod serial_oracle;
    mod set_photometry;
    mod sky;

    #[test]
    fn mixed_exposure_transparency_scale_offset_and_saturation() {
        let mut a = crate::inspect::QualityFrame::empty("synthetic.fit".into());
        let mut b = crate::inspect::QualityFrame::empty("synthetic.fit".into());
        a.stars = (0..20)
            .map(|i| crate::stars::Star {
                x_px: i as f64 * 30.,
                y_px: (i * i % 7) as f64 * 30.,
                hfr_px: 1.7,
                moment_fwhm_px: 3.4,
                eccentricity: 0.2,
                flux_adu: 20000. + i as f64 * 100.,
                peak_adu: 20000.,
                snr: 100.,
            })
            .collect();
        b.stars = a.stars.clone();
        for s in &mut a.stars {
            s.flux_adu *= 1.5;
        } // 20s exposure, 0.75 transparency relative to 10s reference.
        let pairs: Vec<_> = (0..20)
            .map(|i| Match {
                source: i,
                reference: i,
                residual_px: 0.,
            })
            .collect();
        let scale = photometry(&a, &b, &pairs).unwrap();
        assert!((scale - 2. / 3.).abs() < 1e-12);
        let transparency = (1. / scale) * 10. / 20.;
        assert!((transparency - 0.75).abs() < 1e-12);
        let offset = 2000. - scale * 3000.;
        assert!(offset.abs() < 1e-10);
        for i in 0..11 {
            a.stars[i].peak_adu = 65535.;
            a.stars[i].flux_adu = 999999.;
        }
        assert!(photometry(&a, &b, &pairs).is_none());
        assert!(register(Path::new("missing"), f64::NAN, &AtomicBool::new(false)).is_err());
    }

    pub(super) fn planned_frame(
        index: usize,
        date: &str,
        exposure: f64,
        t: Transform,
        flux_factor: f64,
    ) -> QualityFrame {
        use fulgur_core::fits::FrameInfo;
        let mut f = QualityFrame::empty(format!("frame_{index}.fit"));
        f.frame = Some(FrameInfo {
            width: 2160,
            height: 3840,
            bitpix: 16,
            bzero: 32768.,
            bscale: 1.,
            bayer_pattern: Some(fulgur_core::BayerPattern::GRBG),
            mosaic: fulgur_core::Mosaic::Bayer,
            mosaic_source: "header",
            exposure_seconds: exposure,
            object: Some("test".into()),
            filter: Some("test".into()),
            date_obs: Some(date.into()),
            gain: Some(200.),
            bias_informational: Some(2064.),
            saturation_adu: None,
            creator: None,
            derived: vec![],
            rows_mirrored: false,
        });
        f.background_adu = Some(2000. * flux_factor);
        f.noise_adu = Some(20. * flux_factor);
        f.hfr_px = Some(1.7);
        f.accepted = true;
        f.provisional_weight = 1.;
        f.stars = (0..90)
            .map(|i| {
                let p = t.inverse().apply([
                    50. + ((i * 137 + i * i * 11) % 1900) as f64,
                    50. + ((i * 241 + i * i * 7) % 3400) as f64,
                ]);
                crate::stars::Star {
                    x_px: p[0],
                    y_px: p[1],
                    hfr_px: 1.7,
                    moment_fwhm_px: 3.4,
                    eccentricity: 0.2,
                    flux_adu: (10000. + i as f64 * 71.) * flux_factor,
                    peak_adu: 20000.,
                    snr: 100.,
                }
            })
            .collect();
        f
    }
    pub(super) fn report(frames: Vec<QualityFrame>) -> InspectReport {
        InspectReport {
            schema_version: 1,
            command: "inspect",
            frame_count: frames.len(),
            accepted_count: frames.iter().filter(|f| f.accepted).count(),
            rejected_count: 0,
            total_exposure_seconds: 0.,
            accepted_exposure_seconds: 0.,
            wall_seconds: 0.,
            frames_per_second: 0.,
            peak_rss_bytes: None,
            max_concurrent_frames: 1,
            cpu_threads: 1,
            config: InspectConfig::default(),
            weights_status: "test",
            photometry_status: "test",
            coordinate_system: "test",
            noise_units: "test",
            registration_status: "test",
            timing_scope: "test",
            bias_subtracted: false,
            intermediate_images_written: 0,
            target_names: vec![],
            light_sets: vec![],
            frames,
        }
    }
    #[test]
    fn complete_two_session_plan_preserves_exposure_photometry_and_rejection() {
        let flip = Transform {
            scale_factor: 1.,
            theta_rad: std::f64::consts::PI,
            tx_px: 2165.,
            ty_px: 3825.,
        };
        let mut frames = vec![
            planned_frame(0, "2026-10-06T22:00:00", 10., Transform::IDENTITY, 1.),
            planned_frame(
                1,
                "2026-10-06T22:10:00",
                20.,
                Transform {
                    scale_factor: 1.,
                    theta_rad: 0.01,
                    tx_px: 2.,
                    ty_px: -3.,
                },
                1.5,
            ),
            planned_frame(2, "2026-10-07T22:00:00", 10., flip, 0.8),
            planned_frame(
                3,
                "2026-10-07T22:10:00",
                20.,
                flip.compose(Transform {
                    scale_factor: 1.,
                    theta_rad: 0.001,
                    tx_px: 1.,
                    ty_px: 1.,
                }),
                1.8,
            ),
        ];
        // The accepted chronological median is frame 1; one rejected frame is skipped by default.
        frames[1].provisional_weight = 10.;
        frames[3].accepted = false;
        frames[3]
            .rejection_reasons
            .push("excessive_eccentricity".into());
        let r = register_catalogs(
            report(frames),
            0.8,
            None,
            &AtomicBool::new(false),
            Instant::now(),
        )
        .unwrap();
        assert_eq!(r.reference_anchor, "frame_1.fit");
        assert_eq!(r.sessions.len(), 2);
        assert_eq!(r.registration_success_count, 3);
        assert_eq!(r.accepted_count, 3);
        assert_eq!(r.accepted_exposure_seconds, 40.);
        let master = &r.frames[1];
        assert!((master.photometric_scale.unwrap() - 1.).abs() < 1e-12);
        assert!((master.exposure_corrected_transparency.unwrap() - 1.).abs() < 1e-12);
        assert!((r.frames[0].photometric_scale.unwrap() - 1.5).abs() < 1e-10);
        assert!((r.frames[0].exposure_corrected_transparency.unwrap() - 4. / 3.).abs() < 1e-10);
        assert!((r.frames[2].photometric_scale.unwrap() - 1.875).abs() < 1e-10);
        assert!((r.frames[2].exposure_corrected_transparency.unwrap() - 16. / 15.).abs() < 1e-10);
        assert_eq!(r.frames[3].final_weight, 0.);
        assert!(r.frames[3].footprint.is_empty());
        assert!(r.common_intersection.is_some());
        assert!(r.coverage_optimized.is_some());
        assert!(
            r.frames
                .iter()
                .filter(|f| f.registration_success)
                .all(|f| f.inlier_rmse_px.unwrap() < 1e-8)
        );
        let mut none = planned_frame(0, "bad", 10., Transform::IDENTITY, 1.);
        none.accepted = false;
        assert!(
            register_catalogs(
                report(vec![none]),
                0.8,
                None,
                &AtomicBool::new(false),
                Instant::now()
            )
            .is_err()
        );
    }
    #[test]
    fn prediction_uses_header_time_and_rotates_about_sensor_center() {
        let a = Transform::IDENTITY;
        let b = Transform {
            scale_factor: 1.,
            theta_rad: 0.01,
            tx_px: 2.,
            ty_px: 3.,
        };
        let c = predict(&[(100, a), (200, b)], 300, [1080., 1920.]);
        assert!((c.theta_rad - 0.02).abs() < 1e-12);
        let x = a.apply([1080., 1920.]);
        let y = b.apply([1080., 1920.]);
        let z = c.apply([1080., 1920.]);
        assert!((z[0] - (2. * y[0] - x[0])).abs() < 1e-10);
    }

    #[test]
    fn median_anchor_uses_full_header_time_ignores_weights_and_quality_failures() {
        let mut frames = vec![
            planned_frame(
                0,
                "2026-10-06T22:00:00.900000",
                10.,
                Transform::IDENTITY,
                1.,
            ),
            planned_frame(
                1,
                "2026-10-06T22:00:00.100000",
                10.,
                Transform::IDENTITY,
                1.,
            ),
            planned_frame(
                2,
                "2026-10-06T22:00:00.500000",
                10.,
                Transform::IDENTITY,
                1.,
            ),
            planned_frame(
                3,
                "2026-10-06T22:00:00.400000",
                10.,
                Transform::IDENTITY,
                1.,
            ),
        ];
        frames[0].provisional_weight = 999.;
        frames[3].accepted = false;
        assert_eq!(select_anchor(&[0, 1, 2, 3], &frames).unwrap(), 2);
        assert_eq!(select_anchor(&[0, 1], &frames).unwrap(), 1); // Earlier central frame for even count.
        frames[2].frame.as_mut().unwrap().date_obs = None;
        assert!(select_anchor(&[0, 1, 2], &frames).is_err());
        // On an outside grid an undated stack takes its sharpest usable frame;
        // a chosen frame, an own grid and a dated stack keep their rules.
        for (i, hfr) in [2.4, 1.9, 2.1, 1.2].into_iter().enumerate() {
            frames[i].hfr_px = Some(hfr);
        }
        assert_eq!(outside_grid_anchor(&frames, None, true), Some(1));
        assert_eq!(outside_grid_anchor(&frames, Some(0), true), Some(0));
        assert_eq!(outside_grid_anchor(&frames, None, false), None);
        frames[2].frame.as_mut().unwrap().date_obs = Some("2026-10-06T22:00:00.500000".into());
        assert_eq!(outside_grid_anchor(&frames, None, true), None);
        frames[2].frame.as_mut().unwrap().date_obs = None;
        assert_eq!(
            manual_reference(&frames, Path::new("."), Path::new("frame_0.fit")).unwrap(),
            0
        );
        // A frame out of the stack is refused, and the message says why.
        let refused = manual_reference(&frames, Path::new("."), Path::new("frame_3.fit"));
        assert!(refused.unwrap_err().to_string().contains("quality"));
        frames[3].rejection_reasons = vec!["manually_excluded".into()];
        let refused = manual_reference(&frames, Path::new("."), Path::new("frame_3.fit"));
        assert!(refused.unwrap_err().to_string().contains("switched off"));
        assert!(manual_reference(&frames, Path::new("."), Path::new("missing.fit")).is_err());
        frames[0].file_name = "a/same.fit".into();
        frames[1].file_name = "b/same.fit".into();
        assert!(manual_reference(&frames, Path::new("."), Path::new("same.fit")).is_err());
        assert_eq!(
            manual_reference(&frames, Path::new("."), Path::new("b/same.fit")).unwrap(),
            1
        );
    }
    #[test]
    fn real_cfa_pseudo_second_night_rotation_and_shift() {
        let Some(root) = std::env::var_os("FULGURSTACK_TEST_FITS_DIR") else {
            return;
        };
        let mut paths = vec![];
        crate::survey::collect_inputs(Path::new(&root), &mut paths).unwrap();
        paths.sort();
        let cancel = AtomicBool::new(false);
        for path in paths.iter().take(3) {
            let mut raw = fulgur_core::fits::read_raw_sequential(path).unwrap();
            let original = crate::stars::analyze(&raw, &cancel).unwrap();
            let w = raw.header.info.width;
            let h = raw.header.info.height;
            raw.pixels.reverse();
            raw.header.info.bayer_pattern = raw.header.info.bayer_pattern.map(|p| match p {
                fulgur_core::BayerPattern::GRBG => fulgur_core::BayerPattern::GBRG,
                fulgur_core::BayerPattern::GBRG => fulgur_core::BayerPattern::GRBG,
                fulgur_core::BayerPattern::RGGB => fulgur_core::BayerPattern::BGGR,
                fulgur_core::BayerPattern::BGGR => fulgur_core::BayerPattern::RGGB,
            });
            let mut shifted = vec![2000.; raw.pixels.len()];
            for y in 18..h {
                for x in 0..w - 24 {
                    shifted[(y - 18) * w + x + 24] = raw.pixels[y * w + x];
                }
            }
            raw.pixels = shifted;
            let transformed = crate::stars::analyze(&raw, &cancel).unwrap();
            let solution = geometry::solve(&transformed.stars, &original.stars, None, &cancel)
                .unwrap()
                .expect("real CFA flip recovery");
            // q = -p + [width-1+24, height-1-18]. No image is saved.
            let expected = Transform {
                scale_factor: 1.,
                theta_rad: std::f64::consts::PI,
                tx_px: (w - 1 + 24) as f64,
                ty_px: (h - 1 - 18) as f64,
            };
            let error = (transformed
                .stars
                .iter()
                .map(|s| {
                    let p = [s.x_px, s.y_px];
                    let a = solution.transform.apply(p);
                    let b = expected.apply(p);
                    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
                })
                .sum::<f64>()
                / transformed.stars.len() as f64)
                .sqrt();
            println!(
                "Pseudo-night real CFA: {} inliers, residual {:.6}px, transform recovery {:.6}px",
                solution.pairs.len(),
                solution.rmse,
                error
            );
            assert!(error < 0.15, "actual recovery error {error}");
        }
    }
}
