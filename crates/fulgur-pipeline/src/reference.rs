//! External geometry-only FITS references; scientific photometry stays in the native anchor.
use crate::{geometry, register::RegistrationReport, stars};
use fulgur_core::{Error, Result, check_cancelled};
use rayon::prelude::*;
use serde::Serialize;
use std::{path::Path, sync::atomic::AtomicBool, time::Instant};

#[derive(Debug, Serialize)]
pub struct ReferenceStar {
    pub x_px: f64,
    pub y_px: f64,
    pub snr: f64,
}

/// Fixed labels are joined through the scientific anchor, before evaluating any
/// output candidate. A bad prediction cannot remove its own witnesses.
#[allow(clippy::too_many_arguments)]
fn validate_external_row(
    row: &mut crate::register::RegisteredFrame,
    source: &crate::inspect::QualityFrame,
    catalog: &[stars::Star],
    sensor: [usize; 2],
    anchor_correction: Option<crate::distortion::Warp>,
    context: Option<(&[stars::Star], &[geometry::Match])>,
    fit: &mut Option<geometry::Solution>,
) {
    use crate::distortion::{Decision, validation};
    let mut decision = row
        .geometric_correction
        .take()
        .unwrap_or_else(|| Decision::skipped("insufficient_field_evidence"));
    decision.initial_correction_reason = Some(decision.reason);
    let Some((anchor_stars, anchor_pairs)) = context else {
        decision.status = "rejected";
        decision.reason = "missing_independent_anchor_identities";
        decision.model = None;
        row.geometric_correction = Some(decision);
        *fit = None;
        return;
    };
    let labels: std::collections::BTreeMap<_, _> = anchor_pairs
        .iter()
        .map(|m| (m.source, m.reference))
        .collect();
    let pairs: Vec<_> = if row.photometric_reference == Some("direct_master_matches") {
        row.photometric_matches
            .iter()
            .filter_map(|m| {
                labels.get(&m.reference).map(|&reference| geometry::Match {
                    source: m.source,
                    reference,
                    residual_px: 0.,
                })
            })
            .collect()
    } else {
        vec![]
    };
    let native = row.source_to_photometric_anchor.unwrap();
    let expected = anchor_pairs
        .iter()
        .filter(|m| {
            let s = &anchor_stars[m.source];
            let p = native.inverse().apply([s.x_px, s.y_px]);
            p[0] >= 0. && p[1] >= 0. && p[0] < sensor[0] as f64 && p[1] < sensor[1] as f64
        })
        .count();
    let reserved = if std::ptr::eq(source.stars.as_slice(), anchor_stars) {
        // The calibration image itself must keep the calibration's original
        // held-out split; added spatial witnesses could have trained its warp.
        pairs
            .iter()
            .filter(|m| m.source % 5 == 0)
            .map(|m| m.source)
            .collect()
    } else {
        validation::held_out(&source.stars, &pairs)
    };
    let mut selected = None;
    if let Some(model) = decision.model {
        // The original independent cubic used source-index modulo five. Do not
        // call training stars held-out merely because the recovery split differs.
        let cubic_reserved = pairs
            .iter()
            .filter(|m| m.source % 5 == 0)
            .map(|m| m.source)
            .collect();
        let assessment = validation::assess(
            &source.stars,
            catalog,
            &pairs,
            &cubic_reserved,
            sensor,
            expected,
            None,
            |p| model.forward(p),
        );
        if assessment.accepted {
            selected = Some((Some(model), assessment, "external_cubic_validated"));
        }
    }
    // Recovery is a calibrated target warp plus a newly fitted native pose.
    // Reserve the source witnesses BEFORE fitting that pose; no held-out source
    // labels fit its parameters. Different optics still have to pass the same
    // absolute whole-field limits, so this is never unconditional model sharing.
    if selected.is_none() {
        if let Some(anchor_model) = anchor_correction {
            let training: Vec<_> = row
                .photometric_matches
                .iter()
                .filter(|m| labels.contains_key(&m.reference) && !reserved.contains(&m.source))
                .cloned()
                .collect();
            if let Some(pose) = geometry::procrustes(&source.stars, anchor_stars, &training, false)
            {
                if let Some(model) = anchor_model.compose_source(pose, sensor) {
                    let assessment = validation::assess(
                        &source.stars,
                        catalog,
                        &pairs,
                        &reserved,
                        sensor,
                        expected,
                        None,
                        |p| model.forward(p),
                    );
                    if assessment.accepted {
                        decision.reason = "anchor_composition_absolute_field_accuracy_confirmed";
                        selected =
                            Some((Some(model), assessment, "external_anchor_cubic_validated"));
                    }
                }
            }
        }
    }
    // A similarity is permitted only by its own absolute spatial validation.
    // It is fitted on training labels, not the tight subset it happened to match.
    if selected.is_none() {
        let training: Vec<_> = pairs
            .iter()
            .filter(|m| !reserved.contains(&m.source))
            .cloned()
            .collect();
        if let Some(t) = geometry::procrustes(&source.stars, catalog, &training, true) {
            let assessment = validation::assess(
                &source.stars,
                catalog,
                &pairs,
                &reserved,
                sensor,
                expected,
                None,
                |p| t.apply(p),
            );
            if assessment.accepted {
                *fit = Some(geometry::Solution {
                    transform: t,
                    pairs: vec![],
                    rmse: 0.,
                    method: "external_similarity_field_validated",
                });
                decision.reason = "not_needed_absolute_field_accuracy_confirmed";
                selected = Some((None, assessment, "external_similarity_field_validated"));
            } else {
                decision.registration_validation = Some(assessment);
            }
        }
    }
    if let Some((model, assessment, method)) = selected {
        let weight = row
            .normalized_noise_adu
            .zip(source.hfr_px)
            .and_then(|(noise, hfr)| {
                let value = 1. / (noise * noise * hfr * hfr);
                (noise > 0.
                    && hfr > 0.
                    && value.is_finite()
                    && value > 0.
                    && row
                        .photometric_scale
                        .is_some_and(|s| s.is_finite() && s > 0.)
                    && row.background_offset_adu.is_some_and(f64::is_finite))
                .then_some(value)
            });
        let Some(weight) = weight else {
            *fit = None;
            decision.status = "rejected";
            decision.reason = "invalid_native_photometry";
            decision.model = None;
            row.geometric_correction = Some(decision);
            return;
        };
        let t = model
            .map(|m| m.base)
            .or_else(|| fit.as_ref().map(|f| f.transform))
            .unwrap();
        let mut checked = pairs.clone();
        for m in &mut checked {
            let s = &source.stars[m.source];
            let target = &catalog[m.reference];
            let p = model.map_or_else(
                || t.apply([s.x_px, s.y_px]),
                |w| w.forward([s.x_px, s.y_px]),
            );
            m.residual_px = (p[0] - target.x_px).hypot(p[1] - target.y_px);
        }
        *fit = Some(geometry::Solution {
            transform: t,
            pairs: checked,
            rmse: assessment.rmse_px.unwrap(),
            method,
        });
        decision.status = if model.is_some() {
            "applied"
        } else {
            "skipped"
        };
        decision.model = model;
        decision.registration_validation = Some(assessment);
        row.registration_success = true;
        row.accepted = row.quality_accepted;
        // Re-establish all enabled-path weights in the same original native
        // units before the unchanged final normalization, including recovered
        // native geometry failures. Never mix normalized and raw weights.
        row.final_weight = if row.accepted { weight } else { 0. };
        row.rejection_reasons
            .retain(|reason| !reason.starts_with("native_") && reason != "registration_failed");
    } else {
        *fit = None;
        decision.status = "rejected";
        decision.model = None;
        decision.reason = decision
            .registration_validation
            .as_ref()
            .map_or("insufficient_field_evidence", |v| v.reason);
        row.rejection_reasons
            .push("external_whole_field_registration_failed".into());
    }
    row.geometric_correction = Some(decision);
}
#[derive(Debug, Serialize)]
pub struct ExternalReferenceReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registration_validation: Option<crate::distortion::FieldValidation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor_correction: Option<crate::distortion::Warp>,
    pub path: String,
    pub width: usize,
    pub height: usize,
    pub photometric_anchor: String,
    pub geometry_only: bool,
    pub scale_range: [f64; 2],
    pub anchor_to_reference: geometry::Transform,
    pub anchor_matches: Vec<geometry::Match>,
    pub anchor_rmse_px: f64,
    pub stars: Vec<ReferenceStar>,
    pub pixel_rule: &'static str,
}

pub(crate) fn align(
    report: &mut RegistrationReport,
    path: &Path,
    cancel: &AtomicBool,
) -> Result<()> {
    align_with_progress(report, path, cancel, None)
}

pub(crate) fn align_with_progress(
    report: &mut RegistrationReport,
    path: &Path,
    cancel: &AtomicBool,
    progress: Option<&crate::register::RegistrationProgress<'_>>,
) -> Result<()> {
    let start = Instant::now();
    let mut profile = crate::registration_timing::Timer::new();
    let plane = fulgur_core::fits::read_reference_plane(path, cancel)?;
    profile.lap("reference_fits_read");
    if let Some(progress) = progress {
        progress.advance();
    }
    let anchor = report
        .inspection
        .frames
        .iter()
        .find(|f| f.file_name == report.reference_anchor)
        .unwrap();
    let info = anchor.frame.as_ref().unwrap();
    // The reference contributes star geometry only; its pixels never enter the
    // stack, so its filter is irrelevant. A different target is a wrong file.
    if plane
        .object
        .as_ref()
        .zip(info.object.as_ref())
        .is_some_and(|(a, b)| a.trim() != b.trim())
    {
        return Err(Error::Invalid(
            "External reference shows a different target (OBJECT) than the light frames.",
        ));
    }
    profile.lap("reference_validation");
    let catalog = stars::reference_catalog(&plane, cancel)?;
    profile.lap("reference_star_catalog");
    if let Some(progress) = progress {
        progress.advance();
    }
    let ordinary_bridge = geometry::solve_similarity(&anchor.stars, &catalog, None, cancel)?;
    let (anchor_decision, corrected_pairs) = if report.geometric_correction_enabled {
        crate::distortion::solve(
            &anchor.stars,
            &catalog,
            ordinary_bridge.as_ref().map(|s| s.transform),
            [info.width, info.height],
            cancel,
        )?
    } else {
        (crate::distortion::Decision::skipped("disabled"), vec![])
    };
    let anchor_correction = anchor_decision.model;
    let bridge = if let Some(model) = anchor_correction {
        geometry::Solution {
            transform: model.base,
            rmse: (corrected_pairs
                .iter()
                .map(|m| m.residual_px.powi(2))
                .sum::<f64>()
                / corrected_pairs.len() as f64)
                .sqrt(),
            pairs: corrected_pairs,
            method: "external_cubic_bridge",
        }
    } else {
        ordinary_bridge.ok_or(Error::Invalid(
            "External reference could not be matched with at least ten stars at 30..333% scale.",
        ))?
    };
    let mut anchor_validation = None;
    if report.geometric_correction_enabled {
        use crate::distortion::validation;
        let map = |p| anchor_correction.map_or_else(|| bridge.transform.apply(p), |m| m.forward(p));
        let (pairs, expected) =
            validation::witness_pool(&anchor.stars, &catalog, [plane.width, plane.height], map);
        let reserved = if anchor_correction.is_some() {
            pairs
                .iter()
                .filter(|m| m.source % 5 == 0)
                .map(|m| m.source)
                .collect()
        } else {
            validation::held_out(&anchor.stars, &pairs)
        };
        let assessment = validation::assess(
            &anchor.stars,
            &catalog,
            &pairs,
            &reserved,
            [info.width, info.height],
            expected,
            None,
            map,
        );
        if !assessment.accepted {
            return Err(Error::Invalid(
                "External reference anchor has no confirmed whole-field mapping; integration was not started.",
            ));
        }
        anchor_validation = Some(assessment);
    }
    profile.lap("native_anchor_to_external_bridge");
    if let Some(progress) = progress {
        progress.advance();
    }
    let name = path.to_string_lossy().into_owned();
    align_rows_observed(
        &mut report.frames,
        &report.inspection.frames,
        &catalog,
        bridge.transform,
        &name,
        [info.width, info.height],
        cancel,
        progress,
        report.diagnose_excluded,
        report.geometric_correction_enabled,
        anchor_correction,
        Some((&anchor.stars, &bridge.pairs)),
    )?;
    profile.lap("parallel_source_refinements");
    let maximum = report
        .frames
        .iter()
        .map(|f| f.final_weight)
        .fold(0., f64::max);
    if maximum <= 0. {
        return Err(Error::Invalid(
            "No accepted sources registered to the external reference.",
        ));
    }
    for f in &mut report.frames {
        f.final_weight /= maximum;
    }
    let polygons: Vec<_> = report
        .frames
        .iter()
        .filter(|f| f.accepted)
        .map(|f| (f.footprint.clone(), f.final_weight))
        .collect();
    profile.lap("weight_normalization_and_footprints");
    let mut common = polygons[0].0.clone();
    for (p, _) in polygons.iter().skip(1) {
        common = geometry::intersection(common, p);
    }
    report.common_intersection = geometry::canvas(&[(common.clone(), 1.)], 1., cancel)?;
    report.common_intersection_polygon = common;
    profile.lap("common_intersection_canvas");
    report.coverage_optimized = geometry::canvas(&polygons, report.min_coverage, cancel)?;
    profile.lap("coverage_optimized_canvas");
    report.accepted_count = report.frames.iter().filter(|f| f.accepted).count();
    report.rejected_count = report.frame_count - report.accepted_count;
    report.registration_success_count = report
        .frames
        .iter()
        .filter(|f| f.registration_success)
        .count();
    report.accepted_exposure_seconds = report
        .frames
        .iter()
        .zip(&report.inspection.frames)
        .filter(|(f, _)| f.accepted)
        .map(|(_, f)| f.frame.as_ref().unwrap().exposure_seconds)
        .sum();
    report.reference_width = plane.width;
    report.reference_height = plane.height;
    report.reference_selection =
        "explicit external geometry-only FITS; native accepted-median photometric anchor retained";
    report.transform_convention = "source pixel centers -> external reference: q=scale*R(theta)*p+t; scale in [0.30,3.333]; zero-based top-down; session/local and photometric matches remain native anchor coordinates";
    if report.frames.iter().any(|f| {
        f.geometric_correction
            .as_ref()
            .is_some_and(|d| d.model.is_some())
    }) {
        report.transform_convention = "source pixel centers -> external reference: validated cubic model overrides similarity seed; native photometry and its optional model remain separately recorded; zero-based top-down";
    }
    report.external_reference = Some(ExternalReferenceReport {
        registration_validation: anchor_validation,
        anchor_correction,
        path: name,
        width: plane.width,
        height: plane.height,
        photometric_anchor: report.reference_anchor.clone(),
        geometry_only: true,
        scale_range: [
            fulgur_core::MIN_GEOMETRY_SCALE,
            fulgur_core::MAX_GEOMETRY_SCALE,
        ],
        anchor_to_reference: bridge.transform,
        anchor_matches: bridge.pairs,
        anchor_rmse_px: bridge.rmse,
        stars: catalog
            .iter()
            .map(|s| ReferenceStar {
                x_px: s.x_px,
                y_px: s.y_px,
                snr: s.snr,
            })
            .collect(),
        pixel_rule: "template defines geometry/crop only; intensities, exposure, GPS and weights come from native scientific inputs; no template pixels contribute; no area multiplier on resampled surface brightness",
    });
    report.phase_profile.external = Some(profile.finish());
    let elapsed = start.elapsed().as_secs_f64();
    report.registration_seconds += elapsed;
    report.wall_seconds += elapsed;
    report.peak_rss_bytes = fulgur_core::telemetry::peak_rss_bytes();
    if let Some(progress) = progress {
        progress.advance();
    }
    Ok(())
}

/// Independent source fits mutate disjoint rows, capped at sixteen chunks.
/// Every later weight/footprint/report reduction remains in original source order.
#[cfg(test)]
fn align_rows(
    rows: &mut [crate::register::RegisteredFrame],
    sources: &[crate::inspect::QualityFrame],
    catalog: &[stars::Star],
    bridge: geometry::Transform,
    name: &str,
    sensor: [usize; 2],
    cancel: &AtomicBool,
) -> Result<()> {
    align_rows_observed(
        rows, sources, catalog, bridge, name, sensor, cancel, None, true, false, None, None,
    )
}

#[allow(clippy::too_many_arguments)] // Unchanged fit context plus an optional UI observer.
fn align_rows_observed(
    rows: &mut [crate::register::RegisteredFrame],
    sources: &[crate::inspect::QualityFrame],
    catalog: &[stars::Star],
    bridge: geometry::Transform,
    name: &str,
    sensor: [usize; 2],
    cancel: &AtomicBool,
    progress: Option<&crate::register::RegistrationProgress<'_>>,
    diagnose_excluded: bool,
    geometric_correction: bool,
    anchor_correction: Option<crate::distortion::Warp>,
    anchor_context: Option<(&[stars::Star], &[geometry::Match])>,
) -> Result<()> {
    check_cancelled(cancel)?;
    if rows.len() != sources.len() {
        return Err(Error::Invalid(
            "External registration row/catalog counts differ.",
        ));
    }
    let workers = rayon::current_num_threads().min(16);
    let chunk = rows.len().div_ceil(workers).max(1);
    rows.par_chunks_mut(chunk)
        .zip(sources.par_chunks(chunk))
        .try_for_each(|(rows, sources)| -> Result<()> {
            for (row, source) in rows.iter_mut().zip(sources) {
                check_cancelled(cancel)?;
                if !row.quality_accepted && !row.registration_attempted {
                    // A no-anchor diagnostic row still consumed a scheduled native work unit.
                    if diagnose_excluded {
                        if let Some(progress) = progress {
                            progress.advance();
                        }
                    }
                    continue;
                }
                let Some(native) = row.transform else {
                    if let Some(progress) = progress {
                        progress.advance();
                    }
                    continue;
                };
                row.source_to_photometric_anchor = Some(native);
                row.source_to_photometric_anchor_correction =
                    row.geometric_correction.as_ref().and_then(|d| d.model);
                row.match_reference = name.to_owned();
                let mut fit = geometry::solve_similarity(
                    &source.stars,
                    catalog,
                    Some(bridge.compose(native)),
                    cancel,
                )?;
                if geometric_correction {
                    let (decision, pairs) = crate::distortion::solve_seeded(
                        &source.stars,
                        catalog,
                        fit.as_ref()
                            .map(|f| f.transform)
                            .or(Some(bridge.compose(native))),
                        anchor_correction.and_then(|m| m.compose_source(native, sensor)),
                        sensor,
                        cancel,
                    )?;
                    if let Some(model) = decision.model {
                        fit = Some(geometry::Solution {
                            transform: model.base,
                            rmse: (pairs.iter().map(|m| m.residual_px.powi(2)).sum::<f64>()
                                / pairs.len() as f64)
                                .sqrt(),
                            pairs,
                            method: "external_cubic_validated",
                        });
                    }
                    row.geometric_correction = Some(decision);
                    validate_external_row(
                        row,
                        source,
                        catalog,
                        sensor,
                        anchor_correction,
                        anchor_context,
                        &mut fit,
                    );
                }
                if let Some(fit) = fit {
                    row.transform = Some(fit.transform);
                    row.rotation_deg = Some(fit.transform.theta_rad.to_degrees());
                    row.matches = fit.pairs;
                    row.match_reference = name.to_owned();
                    row.method = Some(if geometric_correction {
                        fit.method
                    } else if row
                        .geometric_correction
                        .as_ref()
                        .is_some_and(|d| d.model.is_some())
                    {
                        "external_cubic_validated"
                    } else {
                        "external_similarity_direct"
                    });
                    row.inlier_count = row.matches.len();
                    row.inlier_rmse_px = Some(fit.rmse);
                    if row.accepted {
                        row.footprint = row
                            .geometric_correction
                            .as_ref()
                            .and_then(|d| d.model)
                            .map_or_else(
                                || geometry::footprint(fit.transform, sensor[0], sensor[1]),
                                |m| m.footprint(sensor),
                            );
                    }
                } else {
                    row.registration_success = false;
                    row.accepted = false;
                    row.final_weight = 0.;
                    row.transform = None;
                    row.rotation_deg = None;
                    row.method = None;
                    row.footprint.clear();
                    row.matches.clear();
                    row.inlier_count = 0;
                    row.inlier_rmse_px = None;
                    row.rejection_reasons
                        .push("external_reference_registration_failed".into());
                }
                if let Some(progress) = progress {
                    progress.advance();
                }
            }
            Ok(())
        })
}

#[cfg(test)]
#[path = "reference_parallel_tests.rs"]
mod parallel_tests;
