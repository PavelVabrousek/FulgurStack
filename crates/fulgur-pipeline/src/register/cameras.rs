//! One stack from several cameras: each sensor size registers on its own, and
//! the external reference joins them on one grid (ADR-110). Star flux decides
//! the brightness scale between cameras, per reference pixel, because a
//! resampled star spreads over the area its source pixels cover.
use super::*;
use crate::inspect::OTHER_SENSOR_REASON;

/// Stars both cameras must share to set their brightness scale.
const MIN_SHARED_STARS: usize = 10;
/// Search radius in reference pixels when pairing stars of two cameras.
const PAIR_RADIUS_PX: f64 = 3.;
/// Star peaks above this are clipped and carry no brightness.
const SATURATED_PEAK_ADU: f64 = 62000.;

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
}

/// An external reference gives every camera a common grid, so frames refused
/// only for their sensor size take part after all.
pub(super) fn admit_other_sensors(inspection: &mut InspectReport) {
    for frame in &mut inspection.frames {
        let before = frame.rejection_reasons.len();
        frame.rejection_reasons.retain(|r| r != OTHER_SENSOR_REASON);
        if frame.rejection_reasons.len() != before && frame.rejection_reasons.is_empty() {
            frame.accepted = true;
        }
    }
    inspection.accepted_count = inspection.frames.iter().filter(|f| f.accepted).count();
}

/// Frame indices per sensor size: most usable frames first, then the larger sensor.
/// Unreadable frames and sizes without a usable frame are no camera of their
/// own: they join the first group, so a single-camera run stays one group.
pub(super) fn groups(frames: &[QualityFrame]) -> Vec<Vec<usize>> {
    let mut sizes: Vec<((usize, usize), Vec<usize>)> = vec![];
    let mut spare = vec![];
    for (i, frame) in frames.iter().enumerate() {
        let Some(info) = frame.frame.as_ref() else {
            spare.push(i);
            continue;
        };
        let size = (info.width, info.height);
        match sizes.iter_mut().find(|(known, _)| *known == size) {
            Some((_, members)) => members.push(i),
            None => sizes.push((size, vec![i])),
        }
    }
    let usable = |members: &[usize]| members.iter().filter(|&&i| frames[i].accepted).count();
    sizes.sort_by_key(|((width, height), members)| {
        std::cmp::Reverse((usable(members), width * height))
    });
    let (mut groups, rest): (Vec<_>, Vec<_>) = sizes
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

#[allow(clippy::too_many_arguments)] // One registration per camera, then the join.
pub(super) fn register(
    inspection: InspectReport,
    reference: &Path,
    min_coverage: f64,
    options: RegistrationOptions,
    partial: bool,
    cancel: &AtomicBool,
    start: Instant,
    progress: Option<&RegistrationProgress<'_>>,
    witness: Option<&WitnessCatalog<'_>>,
) -> Result<RegistrationReport> {
    let mut members = groups(&inspection.frames);
    let mut parts = Vec::with_capacity(members.len());
    let mut failures = vec![None; members.len()];
    let mut first_error = None;
    for (position, group) in members.iter().enumerate() {
        let mut subset = inspection.clone();
        subset.frames = group
            .iter()
            .map(|&i| inspection.frames[i].clone())
            .collect();
        let report = register_catalogs_witnessed(
            subset,
            min_coverage,
            None,
            options,
            cancel,
            start,
            progress,
            witness,
        )
        .and_then(|mut report| {
            crate::reference::align_with_progress(
                &mut report,
                reference,
                cancel,
                progress,
                partial,
            )?;
            Ok(report)
        });
        match report {
            Ok(report) => parts.push(Some(report)),
            Err(Error::Cancelled) => return Err(Error::Cancelled),
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
            "No camera could be registered to the external reference.",
        )));
    };
    members.swap(0, first);
    parts.swap(0, first);
    failures.swap(0, first);
    join(inspection, members, parts, failures, witness, cancel, start)
}

fn join(
    inspection: InspectReport,
    members: Vec<Vec<usize>>,
    mut parts: Vec<Option<RegistrationReport>>,
    mut failures: Vec<Option<String>>,
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
    let mut frames: Vec<_> = rows.into_iter().map(Option::unwrap).collect();
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
            "No accepted sources registered to the external reference.",
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
    main.inspection = inspection;
    main.sessions = sessions;
    main.camera_groups = cameras;
    main.reference_selection =
        "explicit external geometry-only FITS; each sensor size registered on its own anchor";
    main.wall_seconds = start.elapsed().as_secs_f64();
    main.registration_seconds = main.wall_seconds - main.inspection_seconds;
    main.peak_rss_bytes = fulgur_core::telemetry::peak_rss_bytes();
    Ok(main)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn the_largest_camera_comes_first_and_sensor_size_refusals_lift() {
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
        assert_eq!(groups(&frames), vec![vec![1, 2], vec![0, 3]]);
        // With equal usable frames the larger sensor leads.
        let tied = vec![frame(200, true, None), frame(300, true, None)];
        assert_eq!(groups(&tied), vec![vec![1], vec![0]]);
        // An unreadable frame and a lone refused size are no camera.
        let mut single = vec![frame(300, true, None), QualityFrame::empty("x.fit".into())];
        single.push(frame(200, false, Some("excessive_hfr")));
        assert_eq!(groups(&single), vec![vec![0, 1, 2]]);
        let mut with_reason = frames;
        with_reason[0] = frame(200, false, Some(OTHER_SENSOR_REASON));
        with_reason[1] = frame(300, false, Some(OTHER_SENSOR_REASON));
        with_reason[1]
            .rejection_reasons
            .push("excessive_hfr".to_owned());
        let lifted: Vec<_> = with_reason
            .into_iter()
            .map(|mut f| {
                let before = f.rejection_reasons.len();
                f.rejection_reasons.retain(|r| r != OTHER_SENSOR_REASON);
                if f.rejection_reasons.len() != before && f.rejection_reasons.is_empty() {
                    f.accepted = true;
                }
                f.accepted
            })
            .collect();
        assert_eq!(lifted, [true, false, true, false]);
    }
}
