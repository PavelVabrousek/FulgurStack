//! M3 registration plan only: immutable CFA catalogs, no RGB or resampled images.
use crate::geometry;
pub use crate::geometry::{Canvas, Match, Transform};
use crate::inspect::{InspectConfig, InspectReport, QualityFrame};
use crate::stars::median;
use chrono::NaiveDateTime;
use fulgur_core::{Error, Result, check_cancelled};
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

#[derive(Debug, Serialize)]
pub struct RegisteredFrame {
    pub file_name: String,
    pub session_id: usize,
    pub quality_accepted: bool,
    pub registration_success: bool,
    pub accepted: bool,
    pub rejection_reasons: Vec<String>,
    pub method: Option<&'static str>,
    pub transform: Option<Transform>,
    pub rotation_deg: Option<f64>,
    pub inlier_count: usize,
    pub inlier_rmse_px: Option<f64>,
    pub matches: Vec<Match>,
    pub photometric_matches: Vec<Match>,
    pub match_reference: String,
    pub photometric_reference: Option<&'static str>,
    pub matched_flux_ratio: Option<f64>,
    pub exposure_corrected_transparency: Option<f64>,
    pub photometric_scale: Option<f64>,
    pub background_offset_adu: Option<f64>,
    pub normalized_noise_adu: Option<f64>,
    pub final_weight: f64,
    pub footprint: Vec<[f64; 2]>,
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
    pub schema_version: u32,
    pub command: &'static str,
    pub reference_anchor: String,
    pub frame_count: usize,
    pub quality_accepted_count: usize,
    pub registration_success_count: usize,
    pub accepted_count: usize,
    pub rejected_count: usize,
    pub accepted_exposure_seconds: f64,
    pub wall_seconds: f64,
    pub inspection_seconds: f64,
    pub registration_seconds: f64,
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
    pub inspection: InspectReport,
}
fn timestamp(row: &QualityFrame) -> Option<i64> {
    let text = row.frame.as_ref()?.date_obs.as_ref()?;
    // Header time only. A missing/unparseable timestamp is explicitly a single-session fallback.
    NaiveDateTime::parse_from_str(text.trim_end_matches('Z'), "%Y-%m-%dT%H:%M:%S%.f")
        .ok()
        .map(|d| d.and_utc().timestamp())
}
fn select_anchor(indices: &[usize], frames: &[QualityFrame]) -> usize {
    let eligible: Vec<_> = indices
        .iter()
        .copied()
        .filter(|&i| frames[i].accepted)
        .collect();
    let middle = eligible.len() / 2;
    let radius = (eligible.len() / 4).max(1);
    eligible
        .iter()
        .enumerate()
        .filter(|(p, _)| p.abs_diff(middle) <= radius)
        .max_by(|(_, a), (_, b)| {
            frames[**a]
                .provisional_weight
                .total_cmp(&frames[**b].provisional_weight)
                .then(b.cmp(a))
        })
        .map(|(_, i)| *i)
        .unwrap_or(indices[0])
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
        file_name: f.file_name.clone(),
        session_id: session,
        quality_accepted: f.accepted,
        registration_success: false,
        accepted: false,
        rejection_reasons: f.rejection_reasons.clone(),
        method: None,
        transform: None,
        rotation_deg: None,
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
        normalized_noise_adu: None,
        final_weight: 0.,
        footprint: vec![],
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
    let start = Instant::now();
    let inspection = crate::inspect::inspect(input, InspectConfig::default(), cancel)?;
    register_catalogs(inspection, min_coverage, cancel, start)
}
fn register_catalogs(
    inspection: InspectReport,
    min_coverage: f64,
    cancel: &AtomicBool,
    start: Instant,
) -> Result<RegistrationReport> {
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
        if groups.is_empty() || (timed && previous.zip(time).is_some_and(|(p, t)| t - p > 6 * 3600))
        {
            groups.push(vec![]);
        }
        groups.last_mut().unwrap().push(i);
        previous = time;
    }
    let all: Vec<_> = groups.iter().flatten().copied().collect();
    let master = select_anchor(&all, quality);
    let master_info = quality[master].frame.as_ref().unwrap();
    let reference_anchor = quality[master].file_name.clone();
    let mut rows: Vec<Option<RegisteredFrame>> = (0..quality.len()).map(|_| None).collect();
    let mut sessions = Vec::new();
    for (sid, group) in groups.iter().enumerate() {
        check_cancelled(cancel)?;
        let anchor = if group.contains(&master) {
            master
        } else {
            select_anchor(group, quality)
        };
        let anchor_name = &quality[anchor].file_name;
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
            geometry::solve(&quality[anchor].stars, &quality[master].stars, None, cancel)?
        };
        let bridge_ratio = bridge
            .as_ref()
            .and_then(|b| total_flux_ratio(&quality[anchor], &quality[master], &b.pairs));
        let bridge_scale = bridge
            .as_ref()
            .and_then(|b| photometry(&quality[anchor], &quality[master], &b.pairs));
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
                let same_sensor = f.frame.as_ref().is_some_and(|info| {
                    info.width == master_info.width
                        && info.height == master_info.height
                        && info.bayer_pattern == master_info.bayer_pattern
                        && info.object == master_info.object
                        && info.filter == master_info.filter
                        && info.gain == master_info.gain
                        && info.bzero == master_info.bzero
                });
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
                    geometry::solve(&f.stars, &quality[anchor].stars, prediction, cancel)?
                };
                if let (Some(solution), Some(bridge)) = (local, bridge.as_ref()) {
                    if i != anchor {
                        history.push((time, solution.transform));
                    }
                    let t = bridge.transform.compose(solution.transform);
                    let direct =
                        geometry::correspondences(&f.stars, &quality[master].stars, t, 1.5);
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
            }
        }
    }
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
    let mut common = polygons[0].0.clone();
    for (p, _) in polygons.iter().skip(1) {
        common = geometry::intersection(common, p);
    }
    let common_intersection = geometry::canvas(&[(common.clone(), 1.)], 1., cancel)?;
    let coverage_optimized = geometry::canvas(&polygons, min_coverage, cancel)?;
    let accepted_count = frames.iter().filter(|r| r.accepted).count();
    let accepted_exposure_seconds = frames
        .iter()
        .zip(quality)
        .filter(|(r, _)| r.accepted)
        .map(|(_, f)| f.frame.as_ref().unwrap().exposure_seconds)
        .sum();
    let wall_seconds = start.elapsed().as_secs_f64();
    Ok(RegistrationReport {
        schema_version: 1,
        command: "register",
        reference_anchor,
        frame_count: frames.len(),
        quality_accepted_count: inspection.accepted_count,
        registration_success_count: frames.iter().filter(|r| r.registration_success).count(),
        accepted_count,
        rejected_count: frames.len() - accepted_count,
        accepted_exposure_seconds,
        wall_seconds,
        inspection_seconds: inspection.wall_seconds,
        registration_seconds: wall_seconds - inspection.wall_seconds,
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
pub fn csv(report: &RegistrationReport) -> String {
    let mut text = String::from(
        "\u{feff}file_name;quality_accepted;registration_success;accepted;session_id;method;rotation_deg;tx_px;ty_px;inliers;rmse_px;flux_ratio;transparency;photometric_scale;background_offset_adu;normalized_noise_adu;final_weight;rejection_reasons\n",
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

    fn planned_frame(
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
            bayer_pattern: fulgur_core::BayerPattern::GRBG,
            exposure_seconds: exposure,
            object: Some("test".into()),
            filter: Some("test".into()),
            date_obs: Some(date.into()),
            gain: Some(200.),
            bias_informational: Some(2064.),
            creator: None,
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
    fn report(frames: Vec<QualityFrame>) -> InspectReport {
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
            frames,
        }
    }
    #[test]
    fn complete_two_session_plan_preserves_exposure_photometry_and_rejection() {
        let flip = Transform {
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
                    theta_rad: 0.001,
                    tx_px: 1.,
                    ty_px: 1.,
                }),
                1.8,
            ),
        ];
        // Keep a known master in the middle half, and retain one rejected frame as diagnostics only.
        frames[1].provisional_weight = 10.;
        frames[3].accepted = false;
        frames[3]
            .rejection_reasons
            .push("excessive_eccentricity".into());
        let r = register_catalogs(report(frames), 0.8, &AtomicBool::new(false), Instant::now())
            .unwrap();
        assert_eq!(r.reference_anchor, "frame_1.fit");
        assert_eq!(r.sessions.len(), 2);
        assert_eq!(r.registration_success_count, 4);
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
        assert!(r.frames.iter().all(|f| f.inlier_rmse_px.unwrap() < 1e-8));
        let mut none = planned_frame(0, "bad", 10., Transform::IDENTITY, 1.);
        none.accepted = false;
        assert!(
            register_catalogs(
                report(vec![none]),
                0.8,
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
            raw.header.info.bayer_pattern = match raw.header.info.bayer_pattern {
                fulgur_core::BayerPattern::GRBG => fulgur_core::BayerPattern::GBRG,
                fulgur_core::BayerPattern::GBRG => fulgur_core::BayerPattern::GRBG,
                fulgur_core::BayerPattern::RGGB => fulgur_core::BayerPattern::BGGR,
                fulgur_core::BayerPattern::BGGR => fulgur_core::BayerPattern::RGGB,
            };
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
