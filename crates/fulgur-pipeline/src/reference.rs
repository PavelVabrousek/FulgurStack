//! External geometry-only FITS references; scientific photometry stays in the native anchor.
use crate::{geometry, register::RegistrationReport, stars};
use fulgur_core::{Error, Result, check_cancelled};
use rayon::prelude::*;
use serde::Serialize;
use std::{path::Path, sync::atomic::AtomicBool, time::Instant};

/// First search radius, in grid pixels, of a wider anchor's cubic around its
/// validated similarity: the optics bend its stars by a few pixels at most.
const TIGHT_ANCHOR_RADIUS_PX: f64 = 12.;

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
    field: crate::distortion::Field,
    reference: [usize; 2],
    bridge: geometry::Transform,
    anchor_correction: Option<crate::distortion::Warp>,
    context: Option<(&[stars::Star], &[geometry::Match])>,
    fit: &mut Option<geometry::Solution>,
    partial: bool,
    limits: crate::distortion::AccuracyLimits,
    deep: Option<&[stars::Star]>,
    files: Option<&crate::register::WitnessCatalog<'_>>,
    cancel: &AtomicBool,
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
    let mut pairs: Vec<_> = if row.photometric_reference == Some("direct_master_matches") {
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
    // Maps the whole field refused; the partial path keeps the tiles they fit.
    let mut refused: Vec<_> = fit.iter().map(|f| (f.transform, None)).collect();
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
            field,
            expected,
            None,
            |p| model.forward(p),
            limits,
        );
        if assessment.accepted {
            selected = Some((Some(model), assessment, "external_cubic_validated"));
        }
        refused.push((model.base, Some(model)));
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
                if let Some(model) = anchor_model.compose_source(pose, field) {
                    let assessment = validation::assess(
                        &source.stars,
                        catalog,
                        &pairs,
                        &reserved,
                        field,
                        expected,
                        None,
                        |p| model.forward(p),
                        limits,
                    );
                    if assessment.accepted {
                        decision.reason = "anchor_composition_absolute_field_accuracy_confirmed";
                        selected =
                            Some((Some(model), assessment, "external_anchor_cubic_validated"));
                    }
                    refused.push((model.base, Some(model)));
                }
            }
        }
    }
    // A frame shares few stars with the anchor's list when its seeing or sampling
    // differs. The reference itself then decides under the same limits; stars no
    // candidate was fitted on are the held-out ones (ADR-110). A wider frame's
    // own list may show other objects than the grid's bright stars, as a
    // narrowband frame does; the grid's longer list then names them (ADR-122).
    let wider = field != crate::distortion::Field::sensor(sensor);
    let fitted: std::collections::BTreeSet<_> = pairs.iter().map(|m| m.source).collect();
    let lists: Vec<&[stars::Star]> = std::iter::once(catalog)
        .chain(deep.filter(|_| wider))
        .collect();
    let direct_check = |t: geometry::Transform,
                        model: Option<crate::distortion::Warp>,
                        carried: bool| {
        let map = |p: [f64; 2]| {
            model.map_or_else(|| t.apply(p), |w: crate::distortion::Warp| w.forward(p))
        };
        for (k, &list) in lists.iter().enumerate() {
            let (direct, in_field) = validation::witness_pool(&source.stars, list, reference, map);
            let direct = validation::without_chance_pairs(&source.stars, list, direct, map);
            // No star of the frame fitted the carried map, so the usual spatial
            // split of the similarity reserves its witnesses, and the grid's
            // longer list tells which of its stars it shows. A wider frame's
            // other maps keep the stars they were fitted on out of that split.
            let (held_out, covered) = if carried {
                let covered = deep
                    .filter(|_| k == 0)
                    .map(|d| validation::covered(&source.stars, d, reference, map));
                (validation::held_out(&source.stars, &direct), covered)
            } else if wider {
                let mut held_out = validation::held_out(&source.stars, &direct);
                held_out.retain(|s| !fitted.contains(s));
                (held_out, None)
            } else {
                let held_out = direct
                    .iter()
                    .map(|m| m.source)
                    .filter(|s| !fitted.contains(s))
                    .collect();
                (held_out, None)
            };
            let mut assessment = validation::assess(
                &source.stars,
                list,
                &direct,
                &held_out,
                field,
                in_field,
                covered,
                map,
                limits,
            );
            assessment.extended_witnesses = k > 0;
            if assessment.accepted {
                return Some((assessment, direct, list));
            }
        }
        None
    };
    // The stars the witnesses of the selected map name.
    let mut witnesses = catalog;
    // A wider frame holds too few labelled stars in the grid's part to fit a
    // pose of its own; its pose on the anchor carries the anchor's validated
    // map, and the reference stars judge the result; the more accurate of it
    // and a similarity joins (ADR-122).
    let carried = anchor_correction
        .filter(|_| selected.is_none() && wider)
        .and_then(|m| m.compose_source(native, field));
    let carried_pass = carried.and_then(|model| direct_check(model.base, Some(model), true));
    let error = |v: &crate::distortion::FieldValidation| v.rmse_px.unwrap_or(f64::INFINITY);
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
                field,
                expected,
                None,
                |p| t.apply(p),
                limits,
            );
            if assessment.accepted
                && carried_pass
                    .as_ref()
                    .is_some_and(|(c, _, _)| error(c) < error(&assessment))
            {
                refused.push((t, None));
            } else if assessment.accepted {
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
                refused.push((t, None));
            }
        }
    }
    if let (None, Some(model), Some((assessment, direct, list))) =
        (&selected, carried, carried_pass)
    {
        *fit = Some(geometry::Solution {
            transform: model.base,
            pairs: vec![],
            rmse: 0.,
            method: "external_direct_field_validated",
        });
        decision.reason = "direct_reference_field_accuracy_confirmed";
        selected = Some((Some(model), assessment, "external_direct_field_validated"));
        pairs = direct;
        witnesses = list;
    }
    if selected.is_none() {
        for &(t, model) in &refused {
            if let Some((assessment, direct, list)) = direct_check(t, model, false) {
                *fit = Some(geometry::Solution {
                    transform: t,
                    pairs: vec![],
                    rmse: 0.,
                    method: "external_direct_field_validated",
                });
                decision.reason = "direct_reference_field_accuracy_confirmed";
                selected = Some((model, assessment, "external_direct_field_validated"));
                pairs = direct;
                witnesses = list;
                break;
            }
        }
    }
    // A narrowband filter or another night can bend the field otherwise than
    // the anchor's map. A refused wider frame then fits a cubic of its own on
    // its longer list, measured once more from its file, against the grid's
    // longer list. Its held-out stars fit nothing, and the limits stay (ADR-125).
    // A frame that shares too few stars to fit any map of its own starts from
    // its native pose carried over the anchor's bridge (ADR-126).
    let mut longer = None;
    if let (None, true, Some(files), Some(grid)) = (&selected, wider, files, deep) {
        longer = files.longer_list(source, cancel);
        let seed = carried
            .map(|m| m.base)
            .or_else(|| refused.first().map(|r| r.0))
            .unwrap_or_else(|| bridge.compose(native));
        if let Some(list) = longer.as_deref() {
            if let Ok((own, _)) = crate::distortion::solve_within(
                list,
                grid,
                Some(seed),
                carried,
                field,
                Some(TIGHT_ANCHOR_RADIUS_PX),
                cancel,
            ) {
                // The cubic's own 1 px improvement rule is for correcting a
                // frame that already registers; here the run's limits judge
                // its held-out stars, as for every other map (ADR-066).
                if let Some(model) = own.model.or(own.candidate) {
                    let map = |p| model.forward(p);
                    let (direct, in_field) = validation::witness_pool(list, grid, reference, map);
                    let direct = validation::without_chance_pairs(list, grid, direct, map);
                    let held_out = direct
                        .iter()
                        .filter(|m| m.source % 5 == 0)
                        .map(|m| m.source)
                        .collect();
                    let mut assessment = validation::assess(
                        list, grid, &direct, &held_out, field, in_field, None, map, limits,
                    );
                    assessment.extended_witnesses = true;
                    if assessment.accepted {
                        *fit = Some(geometry::Solution {
                            transform: model.base,
                            pairs: vec![],
                            rmse: 0.,
                            method: "external_longer_cubic_validated",
                        });
                        decision.reason = "own_longer_list_field_accuracy_confirmed";
                        selected =
                            Some((Some(model), assessment, "external_longer_cubic_validated"));
                        pairs = direct;
                        witnesses = grid;
                    }
                }
            }
        }
    }
    // The selected pairs name stars of the frame's longer list when it decided.
    let src: &[stars::Star] = match (&selected, longer.as_deref()) {
        (Some((_, _, "external_longer_cubic_validated")), Some(list)) => list,
        _ => &source.stars,
    };
    // The tiles may still use the carried map.
    if let (None, Some(model)) = (&selected, carried) {
        refused.push((model.base, Some(model)));
    }
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
    if partial && selected.is_none() {
        // Experimental (ADR-109): the tiles decide, in reference pixels, by
        // the most accurate refused map.
        let best = refused
            .into_iter()
            .map(|(t, model)| {
                let mut checked = pairs.clone();
                let witnesses: Vec<_> = checked
                    .iter_mut()
                    .map(|m| {
                        let s = &source.stars[m.source];
                        let r = &catalog[m.reference];
                        let q = model.map_or_else(
                            || t.apply([s.x_px, s.y_px]),
                            |w: crate::distortion::Warp| w.forward([s.x_px, s.y_px]),
                        );
                        m.residual_px = (q[0] - r.x_px).hypot(q[1] - r.y_px);
                        [r.x_px, r.y_px, m.residual_px]
                    })
                    .collect();
                let rmse = (witnesses.iter().map(|w| w[2] * w[2]).sum::<f64>()
                    / witnesses.len().max(1) as f64)
                    .sqrt();
                (t, model, checked, witnesses, rmse)
            })
            .min_by(|a, b| a.4.total_cmp(&b.4));
        if let (Some((t, model, checked, witnesses, rmse)), Some(weight)) = (best, weight) {
            if crate::register::PartialField::admissible(&witnesses, limits) {
                *fit = Some(geometry::Solution {
                    transform: t,
                    pairs: checked,
                    rmse,
                    method: "external_partial_field",
                });
                row.partial_field = Some(std::sync::Arc::new(crate::register::PartialField::new(
                    witnesses,
                    model.map_or("similarity", |_| "cubic"),
                    limits,
                )));
                decision.status = "partial";
                decision.reason = "accurate_tiles_only";
                decision.model = model;
                row.registration_success = true;
                row.accepted = row.quality_accepted;
                row.final_weight = if row.accepted { weight } else { 0. };
                row.rejection_reasons
                    .retain(|r| !r.starts_with("native_") && r != "registration_failed");
                row.geometric_correction = Some(decision);
                return;
            }
        }
    }
    if let Some((model, assessment, method)) = selected {
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
            let s = &src[m.source];
            let target = &witnesses[m.reference];
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

#[cfg(test)]
pub(crate) fn align(
    report: &mut RegistrationReport,
    path: &Path,
    cancel: &AtomicBool,
) -> Result<()> {
    align_with_progress(report, Template::File(path), cancel, None, false, None)
}

/// Smallest brighter part of a template list tried for a blind match: two stars
/// in each of the list's sixteen cells.
const FIRST_BRIGHT_PART: usize = 32;

/// The anchor's similarity on a template. A blind match pairs triangles of
/// near neighbours, so both lists need a like density where they overlap. A
/// wider camera shows few stars on the grid against a rich template, as a
/// synthetic star field is. When the whole list finds nothing, its brighter
/// parts try in turn. Each part is a fixed number of stars per cell, because
/// the catalog lists cell by cell, rank by rank. A pose found there is
/// refined on the whole list (ADR-129).
fn similarity_bridge(
    anchor: &[stars::Star],
    catalog: &[stars::Star],
    seed: Option<geometry::Transform>,
    cancel: &AtomicBool,
) -> Result<Option<geometry::Solution>> {
    if let Some(found) = geometry::solve_similarity(anchor, catalog, seed, cancel)? {
        return Ok(Some(found));
    }
    if seed.is_some() {
        return Ok(None);
    }
    let mut part = FIRST_BRIGHT_PART;
    while part < catalog.len() {
        if let Some(found) = geometry::solve_similarity(anchor, &catalog[..part], None, cancel)? {
            let refined =
                geometry::solve_similarity(anchor, catalog, Some(found.transform), cancel)?
                    .filter(|whole| whole.pairs.len() >= found.pairs.len());
            // The part's indices are those of the whole list.
            return Ok(Some(refined.unwrap_or(found)));
        }
        part *= 2;
    }
    Ok(None)
}

/// The anchor's map onto the reference, and its validation when geometry is corrected.
#[allow(clippy::too_many_arguments)] // Catalogs, search context and the run's limits.
fn bridge_anchor<'a>(
    anchor: &'a crate::inspect::QualityFrame,
    catalog: &[stars::Star],
    reference: [usize; 2],
    corrected: bool,
    dense: bool,
    seed: Option<geometry::Transform>,
    limits: crate::distortion::AccuracyLimits,
    longer: impl Fn() -> Option<&'a [stars::Star]>,
    cancel: &AtomicBool,
) -> Result<(
    geometry::Solution,
    Option<crate::distortion::Warp>,
    Option<crate::distortion::validation::FieldValidation>,
)> {
    use crate::distortion::Field;
    let info = anchor.frame.as_ref().unwrap();
    let sensor = [info.width, info.height];
    let ordinary_bridge = similarity_bridge(&anchor.stars, catalog, seed, cancel)?;
    let field = ordinary_bridge.as_ref().map_or(Field::sensor(sensor), |b| {
        Field::shared(sensor, reference, b.transform)
    });
    // A wider frame shows the grid on part of its sensor, where its
    // registration list holds few stars; its longer list fits and judges the
    // map there (ADR-122).
    let src = if field == Field::sensor(sensor) {
        &anchor.stars[..]
    } else {
        longer().unwrap_or(&anchor.stars)
    };
    let (anchor_decision, corrected_pairs) = if corrected {
        crate::distortion::solve_seeded(
            src,
            catalog,
            ordinary_bridge.as_ref().map(|s| s.transform),
            None,
            field,
            cancel,
        )?
    } else {
        (crate::distortion::Decision::skipped("disabled"), vec![])
    };
    // The grid's dense list gives chance pairs within the first search radius;
    // the validated similarity already places a wider anchor's stars within
    // a few pixels, so a tight first radius fits its cubic there (ADR-124).
    let (anchor_decision, corrected_pairs) = match &ordinary_bridge {
        Some(bridge)
            if corrected && anchor_decision.model.is_none() && field != Field::sensor(sensor) =>
        {
            let tight = crate::distortion::solve_within(
                src,
                catalog,
                Some(bridge.transform),
                None,
                field,
                Some(TIGHT_ANCHOR_RADIUS_PX),
                cancel,
            )?;
            if tight.0.model.is_some() {
                tight
            } else {
                (anchor_decision, corrected_pairs)
            }
        }
        _ => (anchor_decision, corrected_pairs),
    };
    let anchor_correction = anchor_decision.model;
    let bridge = if let Some(model) = anchor_correction {
        // The labels of the other frames are the anchor's registration list.
        let corrected_pairs = if std::ptr::eq(src, anchor.stars.as_slice()) {
            corrected_pairs
        } else {
            crate::distortion::matches(&anchor.stars, catalog, |p| model.forward(p), 1.5)
        };
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
    if corrected {
        use crate::distortion::validation;
        let map = |p| anchor_correction.map_or_else(|| bridge.transform.apply(p), |m| m.forward(p));
        let (mut pairs, expected) = validation::witness_pool(src, catalog, reference, map);
        if dense {
            pairs = validation::without_chance_pairs(src, catalog, pairs, map);
        }
        let reserved = if anchor_correction.is_some() {
            pairs
                .iter()
                .filter(|m| m.source % 5 == 0)
                .map(|m| m.source)
                .collect()
        } else {
            validation::held_out(src, &pairs)
        };
        let assessment = validation::assess(
            src, catalog, &pairs, &reserved, field, expected, None, map, limits,
        );
        if !assessment.accepted {
            return Err(Error::Invalid(
                "External reference anchor has no confirmed whole-field mapping; integration was not started.",
            ));
        }
        anchor_validation = Some(assessment);
    }
    Ok((bridge, anchor_correction, anchor_validation))
}

/// A frame of the stack's inputs as a grid: the stars it was measured with, in
/// its own pixels, so its file need not be read again (ADR-120).
pub(crate) struct FrameStars {
    pub(crate) name: String,
    pub(crate) size: [usize; 2],
    pub(crate) stars: Vec<stars::Star>,
    /// The longer list that decides when the ordinary one finds no match.
    pub(crate) deep: Vec<stars::Star>,
}

/// What the rows of a registration are aligned to.
#[derive(Clone, Copy)]
pub(crate) enum Template<'a> {
    File(&'a Path),
    /// A frame's grid, with a registration list in its pixels where a first
    /// match starts: two such lists hold the same kind of bright stars.
    Frame(&'a FrameStars, &'a [stars::Star]),
}

enum Plane<'a> {
    File(fulgur_core::fits::ReferencePlane),
    Frame(&'a FrameStars),
}

pub(crate) fn align_with_progress(
    report: &mut RegistrationReport,
    template: Template<'_>,
    cancel: &AtomicBool,
    progress: Option<&crate::register::RegistrationProgress<'_>>,
    partial: bool,
    witness: Option<&crate::register::WitnessCatalog<'_>>,
) -> Result<()> {
    let start = Instant::now();
    let mut profile = crate::registration_timing::Timer::new();
    let plane = match template {
        Template::File(path) => Plane::File(fulgur_core::fits::read_reference_plane(path, cancel)?),
        Template::Frame(frame, _) => Plane::Frame(frame),
    };
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
    // stack, so its filter is irrelevant, and stars decide what matches: its
    // OBJECT card is a label, like those of the frames (ADR-093).
    profile.lap("reference_validation");
    let (size, mut catalog) = match &plane {
        Plane::File(plane) => (
            [plane.width, plane.height],
            stars::reference_catalog(plane, cancel)?,
        ),
        Plane::Frame(frame) => (frame.size, frame.stars.clone()),
    };
    profile.lap("reference_star_catalog");
    if let Some(progress) = progress {
        progress.advance();
    }
    let seed = match template {
        Template::File(_) => None,
        Template::Frame(_, brightest) => {
            geometry::solve_similarity(&anchor.stars, brightest, None, cancel)?
                .map(|solution| solution.transform)
        }
    };
    // Read again only for a wider camera, once.
    let deeper = std::cell::OnceCell::new();
    let longer = || {
        deeper
            .get_or_init(|| witness.and_then(|w| w.precise(anchor, cancel)))
            .as_deref()
    };
    let bridged = bridge_anchor(
        anchor,
        &catalog,
        size,
        report.geometric_correction_enabled,
        false,
        seed,
        report.accuracy,
        longer,
        cancel,
    );
    // The catalog keeps a fixed number of stars per cell, so a rich field or a
    // smaller camera hides true stars; a deeper list decides (ADR-110).
    let (bridge, anchor_correction, anchor_validation) = match bridged {
        Err(Error::Cancelled) => return Err(Error::Cancelled),
        Err(_) => {
            catalog = match &plane {
                Plane::File(plane) => stars::reference_witnesses(plane, cancel)?,
                Plane::Frame(frame) => frame.deep.clone(),
            };
            bridge_anchor(
                anchor,
                &catalog,
                size,
                report.geometric_correction_enabled,
                true,
                seed,
                report.accuracy,
                longer,
                cancel,
            )?
        }
        Ok(bridged) => bridged,
    };
    profile.lap("native_anchor_to_external_bridge");
    if let Some(progress) = progress {
        progress.advance();
    }
    let name = match template {
        Template::File(path) => path.to_string_lossy().into_owned(),
        Template::Frame(frame, _) => frame.name.clone(),
    };
    // A wider camera's frames meet the grid's longer list (ADR-122).
    let sensor = [info.width, info.height];
    let wider = crate::distortion::Field::shared(sensor, size, bridge.transform)
        != crate::distortion::Field::sensor(sensor);
    let deep = match &plane {
        Plane::Frame(frame) => Some(frame.deep.clone()),
        Plane::File(plane) if wider => Some(stars::reference_witnesses(plane, cancel)?),
        Plane::File(_) => None,
    };
    align_rows_observed(
        &mut report.frames,
        &report.inspection.frames,
        &catalog,
        bridge.transform,
        &name,
        [info.width, info.height],
        size,
        cancel,
        progress,
        report.diagnose_excluded,
        report.geometric_correction_enabled,
        anchor_correction,
        Some((&anchor.stars, &bridge.pairs)),
        partial,
        report.accuracy,
        deep.as_deref(),
        witness,
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
    report.reference_width = size[0];
    report.reference_height = size[1];
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
        width: size[0],
        height: size[1],
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
        rows,
        sources,
        catalog,
        bridge,
        name,
        sensor,
        sensor,
        cancel,
        None,
        true,
        false,
        None,
        None,
        false,
        crate::distortion::AccuracyLimits::default(),
        None,
        None,
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
    reference: [usize; 2],
    cancel: &AtomicBool,
    progress: Option<&crate::register::RegistrationProgress<'_>>,
    diagnose_excluded: bool,
    geometric_correction: bool,
    anchor_correction: Option<crate::distortion::Warp>,
    anchor_context: Option<(&[stars::Star], &[geometry::Match])>,
    partial: bool,
    limits: crate::distortion::AccuracyLimits,
    deep: Option<&[stars::Star]>,
    files: Option<&crate::register::WitnessCatalog<'_>>,
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
                    let field =
                        crate::distortion::Field::shared(sensor, reference, bridge.compose(native));
                    let (decision, pairs) = crate::distortion::solve_seeded(
                        &source.stars,
                        catalog,
                        fit.as_ref()
                            .map(|f| f.transform)
                            .or(Some(bridge.compose(native))),
                        anchor_correction.and_then(|m| m.compose_source(native, field)),
                        field,
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
                        field,
                        reference,
                        bridge,
                        anchor_correction,
                        anchor_context,
                        &mut fit,
                        partial,
                        limits,
                        deep,
                        files,
                        cancel,
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
                                |m| m.footprint(),
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
