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
    let peak_limit = quality.peak_limit_adu();
    // The lists of two cameras are short and rarely meet in a small overlap.
    let longer = witness.and_then(|w| w.stars(quality, cancel));
    let stars = longer
        .as_ref()
        .map_or(&quality.stars, |w| &w.precise)
        .iter()
        .filter(|s| s.flux_adu > 0. && s.peak_adu <= peak_limit)
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
    // Every accepted row gets an absolute weight; a row without a usable one
    // is refused rather than keeping a camera-local weight or an infinite one.
    for (row, quality) in frames.iter_mut().zip(&inspection.frames) {
        if !row.accepted {
            continue;
        }
        let weight = match (row.normalized_noise_adu, quality.hfr_px, row.transform) {
            (Some(noise), Some(hfr), Some(t)) => {
                let size = hfr * t.scale_factor;
                1. / (noise * noise * size * size)
            }
            _ => f64::NAN,
        };
        if weight.is_finite() && weight > 0. {
            row.final_weight = weight;
        } else {
            row.accepted = false;
            row.final_weight = 0.;
            row.rejection_reasons
                .push("invalid_photometric_normalization".into());
        }
    }
    super::normalize_weights(
        &mut frames,
        "No accepted sources registered to the reference.",
    )?;
    let polygons = super::accepted_polygons(&frames);
    let (common, common_canvas) = super::common_intersection(&polygons, cancel)?;
    main.common_intersection = common_canvas;
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
#[path = "cameras_tests.rs"]
mod tests;
