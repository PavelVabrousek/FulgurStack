//! Frozen serial planner from commit 64a5dbe. Scientific differential oracle only.
use super::super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn serial_catalogs(
    inspection: InspectReport,
    min_coverage: f64,
    selected: Option<usize>,
    options: RegistrationOptions,
    cancel: &AtomicBool,
    start: Instant,
    progress: Option<&RegistrationProgress<'_>>,
) -> Result<RegistrationReport> {
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
    let mut fit_details = crate::registration_timing::NativeFitDetails::default();
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
        for walk in walks {
            let mut history = vec![(
                timestamp(&quality[anchor]).unwrap_or(anchor as i64),
                Transform::IDENTITY,
            )];
            for i in walk {
                check_cancelled(cancel)?;
                let f = &quality[i];
                if !f.accepted && !options.diagnose_excluded {
                    rows[i] = Some(empty(f, sid, anchor_name));
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
                rows[i] = Some(row);
                if let Some(progress) = progress {
                    progress.advance();
                }
            }
        }
    }
    profile.lap("native_geometry_and_photometry");
    let mut frames: Vec<_> = rows.into_iter().map(Option::unwrap).collect();
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
    Ok(RegistrationReport {
        auto_reference: None,
        geometric_correction_enabled: false,
        schema_version: 2,
        command: "register",
        reference_width: master_info.width,
        reference_height: master_info.height,
        external_reference: None,
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
        transform_convention: "source integer pixel centers -> master anchor: q=R(theta)*p+t; rigid scale=1; radians and pixels; zero-based top-down",
        session_rule: if timed {
            "DATE-OBS chronological order; gap >6h starts session"
        } else {
            "single session fallback: at least one DATE-OBS missing or invalid; sorted source paths"
        },
        photometry_rule: "y=a*x+b in source ADU; a=median(reference_flux/source_flux); flux_ratio=median(source_flux/reference_flux); transparency=flux_ratio*reference_exposure/source_exposure; no second exposure correction",
        weights_status: "final: inverse(normalized_noise^2 * HFR^2), maximum accepted weight=1",
        sessions,
        common_intersection_polygon: common,
        common_intersection,
        coverage_optimized,
        min_coverage,
        frames,
        inspection,
    })
}
