//! One stack from several cameras: each camera registers on its own and a
//! common reference joins them on one grid (ADR-110, ADR-120). Star flux decides
//! the brightness scale between cameras, per reference pixel, because a
//! resampled star spreads over the area its source pixels cover.
use super::*;
use crate::reference::{FrameStars, Template, align_with_progress};

/// Stars both cameras must share to set their brightness scale.
const MIN_SHARED_STARS: usize = 10;
/// Search radius in reference pixels when pairing stars of two cameras.
const PAIR_RADIUS_PX: f64 = 3.;
/// Star peaks above this are clipped and carry no brightness.
const SATURATED_PEAK_ADU: f64 = 62000.;
/// Sharpest frames of each camera that may bridge two cameras when their
/// anchors share too few bright stars.
const BRIDGE_FRAMES: usize = 5;
/// Full registrations a camera may spend on other bridges.
const BRIDGE_ATTEMPTS: usize = 6;

#[derive(Debug, Serialize)]
pub struct CameraGroup {
    pub width: usize,
    pub height: usize,
    pub frames: usize,
    pub stacked: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
    /// Brightness of this camera against the first one, per reference pixel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brightness_scale: Option<f64>,
    pub shared_stars: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused: Option<String>,
    /// It matched the reference only as a mirror image; its rows were reversed.
    pub mirrored: bool,
    /// The first camera's frame whose stars started the match, when its
    /// anchor's stars did not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bridge_frame: Option<String>,
}

/// The leading camera's grid and the registration lists that may start a
/// match with another camera, in grid pixels: its anchor first, then its
/// sharpest frames through their validated maps.
struct Lead {
    grid: FrameStars,
    lists: Vec<(String, Vec<crate::stars::Star>)>,
}

impl Lead {
    fn new(
        report: &RegistrationReport,
        witness: &WitnessCatalog<'_>,
        cancel: &AtomicBool,
    ) -> Result<Self> {
        let anchor = report
            .inspection
            .frames
            .iter()
            .find(|f| f.file_name == report.reference_anchor)
            .ok_or(Error::Invalid("The leading camera has no anchor."))?;
        let mut rows: Vec<_> = report
            .frames
            .iter()
            .zip(&report.inspection.frames)
            .filter(|(row, q)| {
                row.accepted && row.transform.is_some() && q.file_name != report.reference_anchor
            })
            .collect();
        rows.sort_by(|a, b| sharpness(a.1).total_cmp(&sharpness(b.1)));
        let mut lists = vec![(anchor.file_name.clone(), anchor.stars.clone())];
        for (row, quality) in rows.into_iter().take(BRIDGE_FRAMES) {
            let transform = row.transform.unwrap();
            let model = row.geometric_correction.as_ref().and_then(|d| d.model);
            let stars = quality
                .stars
                .iter()
                .map(|s| {
                    let p = model.map_or_else(
                        || transform.apply([s.x_px, s.y_px]),
                        |m| m.forward([s.x_px, s.y_px]),
                    );
                    crate::stars::Star {
                        x_px: p[0],
                        y_px: p[1],
                        ..s.clone()
                    }
                })
                .collect();
            lists.push((quality.file_name.clone(), stars));
        }
        Ok(Self {
            grid: witness.frame_stars(anchor, cancel)?,
            lists,
        })
    }
}

fn sharpness(frame: &QualityFrame) -> f64 {
    frame.hfr_px.unwrap_or(f64::INFINITY)
}

/// Where a stack from several cameras takes its grid.
#[derive(Clone, Copy)]
pub(super) enum Grid<'a> {
    /// A file outside the stack: geometry only.
    External(&'a Path),
    /// A stacked frame: its camera leads and keeps its own grid.
    Member(usize),
    /// The anchor the leading camera finds by the run's reference strategy.
    Automatic,
}

/// Frame indices per camera: a sensor size, and each light set of `split` on
/// its own. Most usable frames first, then the larger sensor. Unreadable frames
/// and cameras without a usable frame join the first group, so a single-camera
/// run stays one group.
pub(super) fn groups(frames: &[QualityFrame], split: &[usize]) -> Vec<Vec<usize>> {
    type Camera = ((usize, usize), Option<usize>);
    let mut cameras: Vec<(Camera, Vec<usize>)> = vec![];
    let mut spare = vec![];
    for (i, frame) in frames.iter().enumerate() {
        let Some(info) = frame.frame.as_ref() else {
            spare.push(i);
            continue;
        };
        let set = split.contains(&frame.light_set).then_some(frame.light_set);
        let camera = ((info.width, info.height), set);
        match cameras.iter_mut().find(|(known, _)| *known == camera) {
            Some((_, members)) => members.push(i),
            None => cameras.push((camera, vec![i])),
        }
    }
    let usable = |members: &[usize]| members.iter().filter(|&&i| frames[i].accepted).count();
    cameras.sort_by_key(|(((width, height), _), members)| {
        std::cmp::Reverse((usable(members), width * height))
    });
    let (mut groups, rest): (Vec<_>, Vec<_>) = cameras
        .into_iter()
        .map(|(_, members)| members)
        .partition(|members| usable(members) > 0);
    spare.extend(rest.into_iter().flatten());
    match groups.first_mut() {
        Some(first) => first.extend(spare),
        None if !spare.is_empty() => groups.push(spare),
        None => {}
    }
    groups
}

/// A camera without capture times has no median frame; its sharpest frame
/// bridges it to the reference.
pub(super) fn undated_anchor(frames: &[QualityFrame]) -> Option<usize> {
    let usable = || frames.iter().enumerate().filter(|(_, f)| f.accepted);
    if usable().all(|(_, f)| timestamp(f).is_some()) {
        return None;
    }
    usable()
        .min_by(|(_, a), (_, b)| sharpness(a).total_cmp(&sharpness(b)))
        .map(|(i, _)| i)
}

/// The earlier pass's anchor, in the camera that holds it, on an outside grid
/// only: a leading camera keeps choosing its own. A camera without that frame
/// finds its own anchor.
fn kept_anchor(frames: &[QualityFrame], keep: Option<&str>, leads: bool) -> Option<usize> {
    keep.filter(|_| !leads)
        .and_then(|name| frames.iter().position(|f| f.file_name == name))
}

/// The frame as its mirror image: every later stage, the readers included, sees
/// it in reversed rows. Two optical trains or two capture programs may disagree
/// on this, so a camera tries it when it finds no match as it stands (ADR-120).
pub(super) fn mirror(frame: &mut QualityFrame) {
    let Some(info) = frame.frame.as_mut() else {
        return;
    };
    info.mirror_rows();
    let bottom = (info.height - 1) as f64;
    for star in &mut frame.stars {
        star.y_px = bottom - star.y_px;
    }
}

/// The chosen frame's camera moves to the front: grid, brightness and master
/// header follow it.
pub(super) fn lead_with(groups: &mut [Vec<usize>], frame: Option<usize>) {
    let lead = frame.and_then(|f| groups.iter().position(|g| g.contains(&f)));
    if let Some(lead) = lead {
        groups[..=lead].rotate_right(1);
    }
}

/// Light sets that lost every usable frame beside a set that stacked. Sensor
/// size alone cannot tell other optics on an equal sensor, so such a set gets
/// another try as a camera of its own (ADR-120).
pub(super) fn stranded(report: &RegistrationReport) -> Vec<usize> {
    let quality = &report.inspection.frames;
    let sets = quality.iter().map(|f| f.light_set + 1).max().unwrap_or(0);
    let (mut usable, mut stacked) = (vec![0usize; sets], vec![0usize; sets]);
    for (row, frame) in report.frames.iter().zip(quality) {
        let camera_failed = row
            .rejection_reasons
            .iter()
            .any(|r| r == "camera_not_registered" || r == "no_shared_stars_between_cameras");
        usable[frame.light_set] += usize::from(frame.accepted && !camera_failed);
        stacked[frame.light_set] += usize::from(row.accepted);
    }
    if stacked.iter().all(|&count| count == 0) {
        return vec![];
    }
    (0..sets)
        .filter(|&set| usable[set] > 0 && stacked[set] == 0)
        .collect()
}

struct CameraAnchor {
    /// Reference position and flux per reference pixel area of each usable star.
    stars: Vec<[f64; 3]>,
    sky: [f64; 3],
}

fn camera_anchor(
    report: &RegistrationReport,
    witness: Option<&WitnessCatalog<'_>>,
    cancel: &AtomicBool,
) -> Option<CameraAnchor> {
    let index = report
        .frames
        .iter()
        .position(|f| f.file_name == report.reference_anchor)?;
    let row = &report.frames[index];
    let quality = &report.inspection.frames[index];
    let transform = row.transform?;
    let model = row.geometric_correction.as_ref().and_then(|d| d.model);
    let area = transform.scale_factor * transform.scale_factor;
    // The lists of two cameras are short and rarely meet in a small overlap.
    let longer = witness.and_then(|w| w.stars(quality, cancel));
    let stars = longer
        .as_ref()
        .map_or(&quality.stars, |w| &w.precise)
        .iter()
        .filter(|s| s.flux_adu > 0. && s.peak_adu <= SATURATED_PEAK_ADU)
        .map(|s| {
            let p = model.map_or_else(
                || transform.apply([s.x_px, s.y_px]),
                |m| m.forward([s.x_px, s.y_px]),
            );
            [p[0], p[1], s.flux_adu * area]
        })
        .collect();
    let sky = quality
        .channel_background_adu
        .unwrap_or([quality.background_adu?; 3]);
    Some(CameraAnchor { stars, sky })
}

/// Median flux ratio of stars at the same reference position, and their number.
fn shared_scale(main: &[[f64; 3]], other: &[[f64; 3]]) -> (Option<f64>, usize) {
    let mut ratios: Vec<f64> = other
        .iter()
        .filter_map(|o| {
            main.iter()
                .map(|m| ((m[0] - o[0]).hypot(m[1] - o[1]), m[2]))
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .filter(|(distance, _)| *distance <= PAIR_RADIUS_PX)
                .map(|(_, flux)| flux / o[2])
        })
        .filter(|r| r.is_finite() && *r > 0.)
        .collect();
    let shared = ratios.len();
    (
        (shared >= MIN_SHARED_STARS).then(|| median(&mut ratios)),
        shared,
    )
}

/// Brightness of each camera against the first, and the stars that set it. Two
/// small fields may overlap only through a larger one, so each camera joins
/// through the known camera it shares most stars with.
fn brightness(anchors: &[Option<CameraAnchor>]) -> Vec<(Option<f64>, usize)> {
    let mut known = vec![(None, 0); anchors.len()];
    known[0].0 = Some(1.);
    loop {
        let mut best: Option<(usize, f64, usize)> = None;
        for (camera, anchor) in anchors.iter().enumerate() {
            let Some(anchor) = anchor.as_ref().filter(|_| known[camera].0.is_none()) else {
                continue;
            };
            for (other, joined) in anchors.iter().enumerate() {
                let (Some(joined), Some(base)) = (joined, known[other].0) else {
                    continue;
                };
                if let (Some(ratio), shared) = shared_scale(&joined.stars, &anchor.stars) {
                    if best.is_none_or(|b| shared > b.2) {
                        best = Some((camera, base * ratio, shared));
                    }
                }
            }
        }
        let Some((camera, scale, shared)) = best else {
            return known;
        };
        known[camera] = (Some(scale), shared);
    }
}

/// Bring a frame normalized to its camera's anchor onto the first camera.
fn rescale(row: &mut RegisteredFrame, scale: f64, shift: [f64; 3]) {
    let Some(a) = row.photometric_scale else {
        return;
    };
    let base = row.background_offset_adu.unwrap_or(0.);
    let offsets = row.channel_offsets_adu.unwrap_or([base; 3]);
    row.photometric_scale = Some(a * scale);
    row.background_offset_adu = Some(scale * base + shift[1]);
    row.channel_offsets_adu = Some(std::array::from_fn(|c| scale * offsets[c] + shift[c]));
    row.normalized_noise_adu = row.normalized_noise_adu.map(|n| n * scale);
}

fn refuse(row: &mut RegisteredFrame, reason: &'static str) {
    row.accepted = false;
    row.registration_success = false;
    row.final_weight = 0.;
    row.footprint.clear();
    row.rejection_reasons.push(reason.into());
}

/// `keep` names the photometric anchor of an earlier pass: on an outside
/// grid its camera keeps it, so a second pass stays on the same brightness
/// and its rows can join the first pass (ADR-128).
#[allow(clippy::too_many_arguments)] // One registration per camera, then the join.
pub(super) fn register(
    mut inspection: InspectReport,
    grid: Grid<'_>,
    split: &[usize],
    keep: Option<&str>,
    min_coverage: f64,
    options: RegistrationOptions,
    partial: bool,
    cancel: &AtomicBool,
    start: Instant,
    progress: Option<&RegistrationProgress<'_>>,
    witness: &WitnessCatalog<'_>,
) -> Result<RegistrationReport> {
    let mut members = groups(&inspection.frames, split);
    let chosen = match grid {
        Grid::Member(frame) => Some(frame),
        _ => None,
    };
    lead_with(&mut members, chosen);
    let mut parts = Vec::with_capacity(members.len());
    let mut failures = vec![None; members.len()];
    let mut first_error = None;
    let outside = match grid {
        Grid::External(path) => Some(Template::File(path)),
        _ => None,
    };
    let mut leading: Option<Lead> = None;
    let mut bridges = vec![None; members.len()];
    let partial = partial || options.partial_frames;
    for (position, group) in members.iter().enumerate() {
        let mut subset = inspection.clone();
        subset.frames = group
            .iter()
            .map(|&i| inspection.frames[i].clone())
            .collect();
        // Without an outside grid the leading camera registers as in any other
        // stack, and its anchor is the reference of the others.
        let reference = outside.or(leading
            .as_ref()
            .map(|lead| Template::Frame(&lead.grid, &lead.lists[0].1)));
        let leads = reference.is_none();
        let selected = chosen
            .filter(|_| leads)
            .and_then(|frame| group.iter().position(|&i| i == frame));
        let anchor = selected
            .or(kept_anchor(&subset.frames, keep, leads))
            .or_else(|| {
                if leads {
                    None
                } else {
                    undated_anchor(&subset.frames)
                }
            });
        let native = RegistrationOptions {
            reference_strategy: if leads {
                options.reference_strategy
            } else {
                ReferenceStrategy::ChronologicalMedian
            },
            partial_frames: leads && options.partial_frames,
            ..options
        };
        // The first try of a camera is planned; each further try adds its work,
        // and a try that ends early counts the rest, so the bar stays true.
        let tries = std::cell::Cell::new(0);
        let attempt = |subset: InspectReport, anchor: Option<usize>, template: Option<Template>| {
            let work = attempt_work(&subset, native);
            let begin = progress.map(|p| {
                if tries.get() > 0 {
                    p.add(work);
                }
                p.completed()
            });
            tries.set(tries.get() + 1);
            let result = (|| {
                let mut report = register_catalogs_witnessed(
                    subset,
                    min_coverage,
                    anchor,
                    native,
                    cancel,
                    start,
                    progress,
                    Some(witness),
                )?;
                if let Some(template) = template {
                    align_with_progress(
                        &mut report,
                        template,
                        cancel,
                        progress,
                        partial,
                        Some(witness),
                    )?;
                }
                Ok(report)
            })();
            if let (Some(p), Some(begin), Some(_)) = (progress, begin, template) {
                p.reach(begin + work);
            }
            result
        };
        let mut report = attempt(subset.clone(), anchor, reference);
        if let (Err(error), false) = (&report, leads) {
            if !matches!(error, Error::Cancelled) {
                let joined = rejoin(&subset, anchor, outside, leading.as_ref(), cancel, &attempt)?;
                if let Some((joined, mirrored, bridge)) = joined {
                    report = Ok(joined);
                    bridges[position] = bridge;
                    if mirrored {
                        for &i in group {
                            mirror(&mut inspection.frames[i]);
                        }
                    }
                }
            }
        }
        let report = report.and_then(|report| {
            if leads {
                leading = Some(Lead::new(&report, witness, cancel)?);
            }
            Ok(report)
        });
        match report {
            Ok(report) => parts.push(Some(report)),
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            // The chosen frame gives no grid without its camera.
            Err(error) if selected.is_some() => return Err(error),
            // A camera that fails only loses its own frames.
            Err(error) => {
                failures[position] = Some(error.to_string());
                first_error.get_or_insert(error);
                parts.push(None);
            }
        }
    }
    let Some(first) = parts.iter().position(Option::is_some) else {
        return Err(first_error.unwrap_or(Error::Invalid(
            "No camera could be registered to the reference.",
        )));
    };
    members.swap(0, first);
    parts.swap(0, first);
    failures.swap(0, first);
    bridges.swap(0, first);
    join(
        inspection,
        grid,
        members,
        parts,
        failures,
        bridges,
        Some(witness),
        cancel,
        start,
    )
}

/// Planned progress steps of registering a camera on a grid: its native rows
/// and validation, then the read, catalog and bridge steps, a row per frame
/// on the grid and finalization.
fn attempt_work(subset: &InspectReport, options: RegistrationOptions) -> usize {
    let rows = subset
        .frames
        .iter()
        .filter(|f| f.accepted || options.diagnose_excluded)
        .count();
    rows * (2 + usize::from(options.geometric_correction)) + 4
}

/// One full registration of a camera's frames on an anchor and a grid.
type Attempt<'a> =
    dyn Fn(InspectReport, Option<usize>, Option<Template<'_>>) -> Result<RegistrationReport> + 'a;

/// A camera that missed the reference as it stands tries again from its
/// sharpest frames, in both orientations. Its frames see different stars of a
/// partly shared field, so one may pass where the anchor did not. A frame is
/// registered in full only when its registration list matches one of the
/// leading camera's sharpest frames, which only starts the match; every
/// attempt meets the usual limits. Gives the registration, whether it is
/// mirrored and the leading frame that started the match when it is not the
/// anchor.
fn rejoin(
    base: &InspectReport,
    first: Option<usize>,
    outside: Option<Template<'_>>,
    lead: Option<&Lead>,
    cancel: &AtomicBool,
    attempt: &Attempt<'_>,
) -> Result<Option<(RegistrationReport, bool, Option<String>)>> {
    // Calibration masters keep the stored row order, so a calibrated camera
    // stays as it is.
    let orientations: &[bool] = if base.frames.iter().any(|f| f.calibrated) {
        &[false]
    } else {
        &[false, true]
    };
    let mut usable: Vec<usize> = (0..base.frames.len())
        .filter(|&i| base.frames[i].accepted)
        .collect();
    usable.sort_by(|&a, &b| sharpness(&base.frames[a]).total_cmp(&sharpness(&base.frames[b])));
    let first = first.or_else(|| select_anchor(&usable, &base.frames).ok());
    let candidates: Vec<usize> = first
        .into_iter()
        .chain(
            usable
                .into_iter()
                .filter(|&i| Some(i) != first)
                .take(BRIDGE_FRAMES),
        )
        .collect();
    let mut spent = 0;
    for &mirrored in orientations {
        let mut subset = base.clone();
        if mirrored {
            subset.frames.iter_mut().for_each(mirror);
        }
        for &frame in &candidates {
            let lists = lead.map_or(1, |lead| lead.lists.len());
            for list in 0..lists {
                // The anchor as it stands against the leading anchor was the first try.
                if !mirrored && Some(frame) == first && list == 0 {
                    continue;
                }
                let template = match (outside, lead) {
                    (Some(template), _) => template,
                    (None, Some(lead)) => {
                        let stars = &lead.lists[list].1;
                        let matched = geometry::solve_similarity(
                            &subset.frames[frame].stars,
                            stars,
                            None,
                            cancel,
                        )?;
                        // Only the anchor pair of the mirrored camera is tried without a match.
                        if matched.is_none() && !(Some(frame) == first && list == 0) {
                            continue;
                        }
                        Template::Frame(&lead.grid, stars)
                    }
                    (None, None) => return Ok(None),
                };
                spent += 1;
                match attempt(subset.clone(), Some(frame), Some(template)) {
                    Ok(report) => {
                        let bridge = lead
                            .filter(|_| list > 0)
                            .map(|lead| lead.lists[list].0.clone());
                        return Ok(Some((report, mirrored, bridge)));
                    }
                    Err(Error::Cancelled) => return Err(Error::Cancelled),
                    Err(_) if spent >= BRIDGE_ATTEMPTS => return Ok(None),
                    // The result depends on this frame and the grid; another
                    // leading list would only start the same match again.
                    Err(_) => break,
                }
            }
        }
    }
    Ok(None)
}

#[allow(clippy::too_many_arguments)] // The registered cameras and their context.
fn join(
    inspection: InspectReport,
    grid: Grid<'_>,
    members: Vec<Vec<usize>>,
    mut parts: Vec<Option<RegistrationReport>>,
    mut failures: Vec<Option<String>>,
    mut bridges: Vec<Option<String>>,
    witness: Option<&WitnessCatalog<'_>>,
    cancel: &AtomicBool,
    start: Instant,
) -> Result<RegistrationReport> {
    let mut main = parts[0]
        .take()
        .ok_or(Error::Invalid("The first camera has no registration."))?;
    let main_anchor = camera_anchor(&main, witness, cancel).ok_or(Error::Invalid(
        "The first camera's anchor has no usable stars.",
    ))?;
    let main_sky = main_anchor.sky;
    let mut anchors: Vec<_> = parts
        .iter()
        .map(|part| {
            part.as_ref()
                .and_then(|p| camera_anchor(p, witness, cancel))
        })
        .collect();
    anchors[0] = Some(main_anchor);
    let scales = brightness(&anchors);
    let mut rows: Vec<Option<RegisteredFrame>> =
        (0..inspection.frames.len()).map(|_| None).collect();
    let mut sessions = vec![];
    let mut cameras = vec![];
    for (position, group) in members.iter().enumerate() {
        let first = inspection.frames[group[0]].frame.as_ref();
        let mut camera = CameraGroup {
            width: first.map_or(0, |f| f.width),
            height: first.map_or(0, |f| f.height),
            frames: group.len(),
            stacked: 0,
            anchor: None,
            brightness_scale: None,
            shared_stars: 0,
            refused: None,
            mirrored: first.is_some_and(|f| f.rows_mirrored),
            bridge_frame: bridges[position].take(),
        };
        let part = if position == 0 {
            Some(&mut main)
        } else {
            parts[position].as_mut()
        };
        let Some(part) = part else {
            for &i in group {
                let mut row = empty(&inspection.frames[i], 0, "");
                row.rejection_reasons.push("camera_not_registered".into());
                rows[i] = Some(row);
            }
            camera.refused = failures[position].take();
            cameras.push(camera);
            continue;
        };
        camera.anchor = Some(part.reference_anchor.clone());
        (camera.brightness_scale, camera.shared_stars) = scales[position];
        let joined = (position > 0).then(|| {
            camera
                .brightness_scale
                .zip(anchors[position].as_ref())
                .map(|(scale, anchor)| {
                    let shift: [f64; 3] =
                        std::array::from_fn(|c| main_sky[c] - scale * anchor.sky[c]);
                    (scale, shift)
                })
        });
        let offset = sessions.len();
        for mut session in part.sessions.drain(..) {
            session.id += offset;
            sessions.push(session);
        }
        for (&i, mut row) in group.iter().zip(part.frames.drain(..)) {
            row.session_id += offset;
            match joined {
                Some(Some((scale, shift))) => rescale(&mut row, scale, shift),
                Some(None) if row.accepted => refuse(&mut row, "no_shared_stars_between_cameras"),
                _ => {}
            }
            camera.stacked += usize::from(row.accepted);
            rows[i] = Some(row);
        }
        if matches!(joined, Some(None)) {
            camera.refused = Some("no_shared_stars_between_cameras".to_owned());
        }
        cameras.push(camera);
    }
    let frames: Vec<_> = rows.into_iter().map(Option::unwrap).collect();
    main.inspection = inspection;
    main.sessions = sessions;
    main.camera_groups = cameras;
    main.reference_selection = match grid {
        Grid::External(_) => {
            "explicit external geometry-only FITS; each camera registered on its own anchor"
        }
        Grid::Member(_) => "manual frame; other cameras joined on its grid by shared stars",
        Grid::Automatic => {
            "anchor of the camera with most usable frames; other cameras joined on its grid by shared stars"
        }
    };
    if main.external_reference.is_none() {
        main.transform_convention = "source pixel centers -> leading camera's anchor: q=scale*R(theta)*p+t or a validated cubic model; other cameras reach it through their own native anchor; zero-based top-down";
    }
    settle(&mut main, frames, cancel, start)?;
    Ok(main)
}

/// Weights, canvases and counts of the joined rows, over the report's inspection.
fn settle(
    main: &mut RegistrationReport,
    mut frames: Vec<RegisteredFrame>,
    cancel: &AtomicBool,
    start: Instant,
) -> Result<()> {
    let inspection = &main.inspection;
    // Weights of different pixel scales compare in reference pixels.
    for (row, quality) in frames.iter_mut().zip(&inspection.frames) {
        if let (true, Some(noise), Some(hfr), Some(t)) = (
            row.accepted,
            row.normalized_noise_adu,
            quality.hfr_px,
            row.transform,
        ) {
            let size = hfr * t.scale_factor;
            row.final_weight = 1. / (noise * noise * size * size);
        }
    }
    let maximum = frames.iter().map(|f| f.final_weight).fold(0., f64::max);
    if maximum <= 0. {
        return Err(Error::Invalid(
            "No accepted sources registered to the reference.",
        ));
    }
    for row in &mut frames {
        row.final_weight /= maximum;
    }
    let polygons: Vec<_> = frames
        .iter()
        .filter(|f| f.accepted)
        .map(|f| (f.footprint.clone(), f.final_weight))
        .collect();
    let mut common = polygons[0].0.clone();
    for (polygon, _) in polygons.iter().skip(1) {
        common = geometry::intersection(common, polygon);
    }
    main.common_intersection = geometry::canvas(&[(common.clone(), 1.)], 1., cancel)?;
    main.common_intersection_polygon = common;
    main.coverage_optimized = geometry::canvas(&polygons, main.min_coverage, cancel)?;
    main.frame_count = frames.len();
    main.quality_accepted_count = frames.iter().filter(|f| f.quality_accepted).count();
    main.registration_attempted_count = frames.iter().filter(|f| f.registration_attempted).count();
    main.registration_skipped_count = frames.len() - main.registration_attempted_count;
    main.registration_success_count = frames.iter().filter(|f| f.registration_success).count();
    main.accepted_count = frames.iter().filter(|f| f.accepted).count();
    main.rejected_count = frames.len() - main.accepted_count;
    main.accepted_exposure_seconds = frames
        .iter()
        .zip(&inspection.frames)
        .filter(|(f, _)| f.accepted)
        .filter_map(|(_, q)| q.frame.as_ref())
        .map(|f| f.exposure_seconds)
        .sum();
    main.frames = frames;
    main.wall_seconds = start.elapsed().as_secs_f64();
    main.registration_seconds = main.wall_seconds - main.inspection_seconds;
    main.peak_rss_bytes = fulgur_core::telemetry::peak_rss_bytes();
    Ok(())
}

/// Stranded light sets whose own camera registered in the second pass, when
/// both passes share a grid through the same leading anchor. A refused set
/// keeps its first-pass rows.
pub(super) fn taken_sets(
    first: &RegistrationReport,
    second: &RegistrationReport,
    split: &[usize],
) -> Vec<usize> {
    if first.reference_anchor != second.reference_anchor
        || (first.reference_width, first.reference_height)
            != (second.reference_width, second.reference_height)
        || first.frames.len() != second.frames.len()
    {
        return vec![];
    }
    let quality = &second.inspection.frames;
    second
        .camera_groups
        .iter()
        .filter_map(|c| {
            let anchor = c.anchor.as_ref()?;
            quality.iter().find(|q| &q.file_name == anchor)
        })
        .map(|q| q.light_set)
        .filter(|s| split.contains(s))
        .collect()
}

/// The second pass of stranded light sets keeps only what it found for them:
/// the other sets keep their first-pass rows, which a second pass with another
/// camera anchor can lose (M33 run v80, ADR-123).
pub(super) fn merge_stranded(
    mut first: RegistrationReport,
    mut second: RegistrationReport,
    taken: &[usize],
    cancel: &AtomicBool,
    start: Instant,
) -> Result<RegistrationReport> {
    let from_second: Vec<bool> = second
        .inspection
        .frames
        .iter()
        .map(|q| taken.contains(&q.light_set))
        .collect();
    let count = first.frames.len();
    // The first pass's cameras lose the taken frames to their own cameras; a
    // first pass with a single camera reports none, so it gets its entry here.
    let mut cameras = std::mem::take(&mut first.camera_groups);
    let single = cameras.is_empty();
    if single {
        let info = first
            .inspection
            .frames
            .iter()
            .find(|q| q.file_name == first.reference_anchor)
            .and_then(|q| q.frame.as_ref());
        cameras.push(CameraGroup {
            width: info.map_or(first.reference_width, |f| f.width),
            height: info.map_or(first.reference_height, |f| f.height),
            frames: count,
            stacked: 0,
            anchor: Some(first.reference_anchor.clone()),
            brightness_scale: Some(1.),
            shared_stars: 0,
            refused: None,
            mirrored: info.is_some_and(|f| f.rows_mirrored),
            bridge_frame: None,
        });
    }
    for i in (0..count).filter(|&i| from_second[i]) {
        let size = first.inspection.frames[i]
            .frame
            .as_ref()
            .map(|f| (f.width, f.height));
        // Unreadable frames joined the first camera.
        let position = cameras
            .iter()
            .position(|c| Some((c.width, c.height)) == size)
            .unwrap_or(0);
        cameras[position].frames -= 1;
        cameras[position].stacked -= usize::from(first.frames[i].accepted);
    }
    if single {
        cameras[0].stacked = (0..count)
            .filter(|&i| !from_second[i] && first.frames[i].accepted)
            .count();
    }
    let quality = &second.inspection.frames;
    cameras.extend(second.camera_groups.drain(..).filter(|c| {
        c.anchor.as_ref().is_some_and(|anchor| {
            quality
                .iter()
                .any(|q| &q.file_name == anchor && taken.contains(&q.light_set))
        })
    }));
    // Sessions of the taken rows follow the first pass's sessions.
    let mut sessions: Vec<Option<Session>> = second.sessions.drain(..).map(Some).collect();
    let mut renumbered = std::collections::BTreeMap::new();
    let mut frames = Vec::with_capacity(count);
    for (i, (row, other)) in first
        .frames
        .drain(..)
        .zip(second.frames.drain(..))
        .enumerate()
    {
        if !from_second[i] {
            frames.push(row);
            continue;
        }
        let mut row = other;
        let next = first.sessions.len();
        let id = *renumbered.entry(row.session_id).or_insert_with(|| {
            if let Some(mut session) = sessions.get_mut(row.session_id).and_then(Option::take) {
                session.id = next;
                first.sessions.push(session);
            }
            next
        });
        row.session_id = id;
        first.inspection.frames[i] = second.inspection.frames[i].clone();
        frames.push(row);
    }
    first.camera_groups = cameras;
    first.reference_selection = second.reference_selection;
    first.transform_convention = second.transform_convention;
    settle(&mut first, frames, cancel, start)?;
    Ok(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_pass_on_an_outside_grid_keeps_the_first_anchor() {
        let frames: Vec<_> = (0..4)
            .map(|i| {
                let date = format!("2026-10-06T22:{:02}:00", i * 5);
                crate::register::tests::planned_frame(i, &date, 10., Transform::IDENTITY, 1.)
            })
            .collect();
        assert_eq!(kept_anchor(&frames, Some("frame_3.fit"), false), Some(3));
        // A leading camera chooses by the run's strategy.
        assert_eq!(kept_anchor(&frames, Some("frame_3.fit"), true), None);
        // Another camera, or the first pass, finds its own anchor.
        assert_eq!(kept_anchor(&frames, Some("frame_9.fit"), false), None);
        assert_eq!(kept_anchor(&frames, None, false), None);
    }

    #[test]
    fn a_stranded_set_keeps_its_second_pass_and_the_others_their_first() {
        let mut frames: Vec<_> = (0..6)
            .map(|i| {
                let t = Transform {
                    scale_factor: 1.,
                    theta_rad: i as f64 * 0.01,
                    tx_px: i as f64 * 5.,
                    ty_px: 0.,
                };
                let date = format!("2026-10-06T22:{:02}:00", i * 5);
                crate::register::tests::planned_frame(i, &date, 10., t, 1.)
            })
            .collect();
        for frame in &mut frames[3..] {
            frame.light_set = 1;
        }
        let input = crate::register::tests::report(frames);
        let cancel = AtomicBool::new(false);
        let pass = || {
            crate::register::register_catalogs(input.clone(), 0.8, Some(1), &cancel, Instant::now())
                .unwrap()
        };
        let refuse_set = |report: &mut RegistrationReport, set: usize| {
            for (row, q) in report.frames.iter_mut().zip(&report.inspection.frames) {
                if q.light_set == set {
                    row.accepted = false;
                    row.final_weight = 0.;
                    row.rejection_reasons.push("registration_failed".into());
                }
            }
            report.accepted_count = report.frames.iter().filter(|f| f.accepted).count();
        };
        // First pass: set 1 stranded. Second pass: set 1 joins as its own
        // camera, while set 0 falls apart under another anchor.
        let mut first = pass();
        refuse_set(&mut first, 1);
        let mut second = pass();
        refuse_set(&mut second, 0);
        let camera = |anchor: &str, stacked| CameraGroup {
            width: 2160,
            height: 3840,
            frames: 3,
            stacked,
            anchor: Some(anchor.into()),
            brightness_scale: Some(1.),
            shared_stars: 20,
            refused: None,
            mirrored: false,
            bridge_frame: None,
        };
        second.camera_groups = vec![camera("frame_1.fit", 0), camera("frame_4.fit", 3)];
        for row in second.frames.iter_mut().skip(3) {
            row.session_id = 0;
        }
        let taken = taken_sets(&first, &second, &[1]);
        assert_eq!(taken, vec![1]);
        let first_sessions = first.sessions.len();
        let merged = merge_stranded(first, second, &taken, &cancel, Instant::now()).unwrap();
        assert_eq!(merged.accepted_count, 6);
        assert!(merged.frames.iter().all(|f| f.accepted));
        // The single first-pass camera keeps its three frames; set 1 has its own.
        let cameras: Vec<_> = merged
            .camera_groups
            .iter()
            .map(|c| (c.anchor.clone().unwrap(), c.frames, c.stacked))
            .collect();
        assert_eq!(
            cameras,
            [("frame_1.fit".into(), 3, 3), ("frame_4.fit".into(), 3, 3)]
        );
        assert_eq!(merged.sessions.len(), first_sessions + 1);
        assert!(
            merged.frames[3..]
                .iter()
                .all(|f| f.session_id == first_sessions)
        );
        assert!(merged.sessions.iter().enumerate().all(|(i, s)| s.id == i));
        let maximum = merged
            .frames
            .iter()
            .map(|f| f.final_weight)
            .fold(0., f64::max);
        assert!((maximum - 1.).abs() < 1e-12);
        // Another leading anchor means another grid: nothing is merged.
        let mut other = pass();
        other.reference_anchor = "frame_0.fit".into();
        other.camera_groups = vec![camera("frame_4.fit", 3)];
        assert!(taken_sets(&pass(), &other, &[1]).is_empty());
    }

    #[test]
    fn brightness_follows_flux_per_reference_pixel_and_needs_shared_stars() {
        let main: Vec<[f64; 3]> = (0..30)
            .map(|i| [i as f64 * 40., i as f64 * 25., 1000. + i as f64 * 10.])
            .collect();
        // The same stars half as bright, 1 px away, and some of another field.
        let mut other: Vec<[f64; 3]> = main.iter().map(|s| [s[0] + 1., s[1], s[2] / 2.]).collect();
        other.push([5000., 5000., 1.]);
        let (scale, shared) = shared_scale(&main, &other);
        assert_eq!(shared, 30);
        assert!((scale.unwrap() - 2.).abs() < 1e-9);
        let (scale, shared) = shared_scale(&main, &other[..9]);
        assert_eq!((scale, shared), (None, 9));
        assert_eq!(shared_scale(&main, &[]).0, None);
    }

    #[test]
    fn brightness_chains_through_the_camera_that_shares_most_stars() {
        let stars = |range: std::ops::Range<usize>, flux: f64| -> Vec<[f64; 3]> {
            range
                .map(|i| [i as f64 * 40., i as f64 * 25., flux * (1. + i as f64)])
                .collect()
        };
        let camera = |stars| {
            Some(CameraAnchor {
                stars,
                sky: [0.; 3],
            })
        };
        // The first and last cameras overlap only through the middle one.
        let anchors = [
            camera(stars(0..20, 100.)),
            camera(stars(0..40, 50.)),
            camera(stars(25..40, 10.)),
            None,
            camera(stars(100..120, 1.)),
        ];
        let scales = brightness(&anchors);
        assert_eq!(scales[0], (Some(1.), 0));
        assert!((scales[1].0.unwrap() - 2.).abs() < 1e-9 && scales[1].1 == 20);
        assert!((scales[2].0.unwrap() - 10.).abs() < 1e-9 && scales[2].1 == 15);
        assert_eq!(scales[3], (None, 0));
        assert_eq!(scales[4], (None, 0));
    }

    #[test]
    fn rescaling_keeps_the_sky_of_the_first_camera() {
        let quality = QualityFrame::empty("a.fit".into());
        let mut row = empty(&quality, 0, "");
        row.photometric_scale = Some(1.5);
        row.background_offset_adu = Some(-200.);
        row.channel_offsets_adu = Some([-100., -200., -300.]);
        row.normalized_noise_adu = Some(4.);
        let sky = [1000., 1100., 1200.];
        let main_sky = [500., 800., 300.];
        let scale = 2.;
        let shift: [f64; 3] = std::array::from_fn(|c| main_sky[c] - scale * sky[c]);
        rescale(&mut row, scale, shift);
        let offsets = row.channel_offsets_adu.unwrap();
        let a = row.photometric_scale.unwrap();
        assert_eq!(a, 3.);
        // A frame sky of (sky + 100) / 1.5 reaches the first camera's sky.
        for c in 0..3 {
            let frame_sky = (sky[c] - [-100., -200., -300.][c]) / 1.5;
            assert!((a * frame_sky + offsets[c] - main_sky[c]).abs() < 1e-9);
        }
        assert_eq!(row.normalized_noise_adu, Some(8.));
    }

    #[test]
    fn another_bridge_is_tried_only_where_star_lists_match() {
        let star = |i: u64| {
            let hash = (i + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15)
                ^ (i / 1000).wrapping_mul(0xD1B5_4A32_D192_ED03);
            crate::stars::Star {
                x_px: 20. + ((hash >> 20) % 960) as f64 + 0.3,
                y_px: 20. + ((hash >> 40) % 960) as f64 + 0.7,
                hfr_px: 2.,
                moment_fwhm_px: 4.,
                eccentricity: 0.2,
                flux_adu: 1000.,
                peak_adu: 500.,
                snr: 50.,
            }
        };
        let field: Vec<_> = (0..60).map(star).collect();
        let other: Vec<_> = (1000..1060).map(star).collect();
        let mut base = crate::register::tests::report(vec![]);
        for (i, minute) in [10, 20, 30, 40].into_iter().enumerate() {
            let mut frame = QualityFrame::empty(format!("{i}.fit"));
            let mut info = crate::inspect::test_frame_info(1000, 1000);
            info.date_obs = Some(format!("2026-10-06T22:{minute}:00"));
            frame.frame = Some(info);
            frame.accepted = true;
            frame.hfr_px = Some(3. - i as f64 * 0.1);
            frame.stars = field.clone();
            base.frames.push(frame);
        }
        let lead = Lead {
            grid: FrameStars {
                name: "lead.fit".into(),
                size: [1000, 1000],
                stars: field.clone(),
                deep: field.clone(),
            },
            lists: vec![("lead.fit".into(), other), ("sharp.fit".into(), field)],
        };
        let calls = std::cell::RefCell::new(vec![]);
        let attempt = |subset: InspectReport, anchor: Option<usize>, template: Option<Template>| {
            let list = match template {
                Some(Template::Frame(_, stars)) => {
                    usize::from(std::ptr::eq(stars, &*lead.lists[1].1))
                }
                _ => 9,
            };
            let mirrored = subset.frames[0].frame.as_ref().unwrap().rows_mirrored;
            calls.borrow_mut().push((anchor, mirrored, list));
            Err(Error::Invalid("no match"))
        };
        let cancel = AtomicBool::new(false);
        let result = rejoin(&base, None, None, Some(&lead), &cancel, &attempt).unwrap();
        assert!(result.is_none());
        let calls = calls.into_inner();
        // The time median (frame 1) first, then the sharpest frames, each only
        // against the leading list that matches; the unmatched anchor list is
        // never registered as it stands, and the budget holds.
        assert_eq!(
            calls[..4],
            [
                (Some(1), false, 1),
                (Some(3), false, 1),
                (Some(2), false, 1),
                (Some(0), false, 1)
            ]
        );
        // Mirrored, only the anchor pair runs without a match of the lists.
        assert_eq!(calls[4..], [(Some(1), true, 0)]);
    }

    #[test]
    fn the_largest_camera_comes_first_and_a_chosen_frame_leads() {
        let frame = |width, accepted: bool, reason: Option<&str>| {
            let mut q = QualityFrame::empty(format!("{width}.fit"));
            q.frame = Some(crate::inspect::test_frame_info(width, 100));
            q.accepted = accepted;
            q.rejection_reasons = reason.map(str::to_owned).into_iter().collect();
            q
        };
        let frames = vec![
            frame(200, true, None),
            frame(300, true, None),
            frame(300, true, None),
            frame(200, false, Some("low_star_count_in_exposure_group")),
        ];
        assert_eq!(groups(&frames, &[]), vec![vec![1, 2], vec![0, 3]]);
        // With equal usable frames the larger sensor leads.
        let tied = vec![frame(200, true, None), frame(300, true, None)];
        assert_eq!(groups(&tied, &[]), vec![vec![1], vec![0]]);
        // An unreadable frame and a lone refused size are no camera.
        let mut single = vec![frame(300, true, None), QualityFrame::empty("x.fit".into())];
        single.push(frame(200, false, Some("excessive_hfr")));
        assert_eq!(groups(&single, &[]), vec![vec![0, 1, 2]]);
        // A stranded light set leaves its equal sensor; a chosen frame leads.
        let mut sets = vec![
            frame(300, true, None),
            frame(300, true, None),
            frame(200, true, None),
            frame(300, true, None),
        ];
        sets[3].light_set = 2;
        sets[2].light_set = 1;
        let mut members = groups(&sets, &[2]);
        assert_eq!(members, vec![vec![0, 1], vec![3], vec![2]]);
        lead_with(&mut members, Some(2));
        assert_eq!(members, vec![vec![2], vec![0, 1], vec![3]]);
    }
}
