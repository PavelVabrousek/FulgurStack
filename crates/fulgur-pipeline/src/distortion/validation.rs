//! Absolute field acceptance, separate from optional nonlinear improvement.
use super::*;

#[derive(Clone, Debug, Serialize)]
pub struct FieldValidation {
    pub accepted: bool,
    pub reason: &'static str,
    pub fixed_stars: usize,
    pub expected_stars: usize,
    /// Source stars with a counterpart within 3 px in the extended reference
    /// catalog; replaces `fixed_stars` in the coverage rule when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub covered_stars: Option<usize>,
    /// Accuracy witnesses came from the anchor's extended precise catalog
    /// (3 px window) because the ordinary catalogs shared too few stars.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub extended_witnesses: bool,
    pub validation_stars: usize,
    pub rmse_px: Option<f64>,
    pub validation_rmse_px: Option<f64>,
    pub maximum_error_px: Option<f64>,
}

/// Reserve spatial witnesses before fitting the recovery pose. Membership uses
/// source positions and identities only, never candidate residuals. Additional
/// outer-quadrant witnesses prevent catalog order from leaving a corner untested.
pub(crate) fn held_out(src: &[Star], pairs: &[Match]) -> BTreeSet<usize> {
    let region = field_region(src, pairs);
    let mut result: BTreeSet<_> = pairs
        .iter()
        .filter(|m| m.source % 5 == 0)
        .map(|m| m.source)
        .collect();
    for quadrant in 0..4 {
        let mut witnesses: Vec<_> = pairs
            .iter()
            .filter(|m| {
                let s = &src[m.source];
                outer(s, region) && quadrant_of(s, region) == quadrant
            })
            .map(|m| m.source)
            .collect();
        witnesses.sort_unstable();
        for i in witnesses.into_iter().take(2) {
            result.insert(i);
        }
    }
    result
}

fn quadrant_of(s: &Star, region: [f64; 4]) -> usize {
    usize::from(s.x_px > (region[0] + region[2]) / 2.)
        + 2 * usize::from(s.y_px > (region[1] + region[3]) / 2.)
}

/// This function never rematches, trims, or fits. Missing witnesses remain
/// failures. Both all-star and independently reserved errors must pass.
#[allow(clippy::too_many_arguments)] // Fixed witnesses, split, geometry and optional coverage.
pub(crate) fn assess(
    src: &[Star],
    dst: &[Star],
    pairs: &[Match],
    reserved: &BTreeSet<usize>,
    sensor: [usize; 2],
    expected: usize,
    covered: Option<usize>,
    map: impl Fn([f64; 2]) -> [f64; 2],
) -> FieldValidation {
    let validation: Vec<_> = pairs
        .iter()
        .filter(|m| reserved.contains(&m.source))
        .cloned()
        .collect();
    let train: Vec<_> = pairs
        .iter()
        .filter(|m| !reserved.contains(&m.source))
        .cloned()
        .collect();
    let mut result = FieldValidation {
        accepted: false,
        reason: "insufficient_field_evidence",
        fixed_stars: pairs.len(),
        expected_stars: expected,
        covered_stars: covered,
        extended_witnesses: false,
        validation_stars: validation.len(),
        rmse_px: None,
        validation_rmse_px: None,
        maximum_error_px: None,
    };
    if pairs.len() < 50
        || train.len() < 30
        || validation.len() < 10
        || covered.unwrap_or(pairs.len()) * 5 < expected * 4
    {
        return result;
    }
    let region = field_region(src, pairs);
    if region[2] - region[0] < sensor[0] as f64 * 0.35
        || region[3] - region[1] < sensor[1] as f64 * 0.35
        || coverage(src, pairs, region) < 10
        || coverage(src, &validation, region) < 6
    {
        return result;
    }
    let errors: Vec<_> = pairs
        .iter()
        .map(|m| {
            let s = &src[m.source];
            let target = &dst[m.reference];
            let p = map([s.x_px, s.y_px]);
            (p[0] - target.x_px).hypot(p[1] - target.y_px)
        })
        .collect();
    let all = rmse(src, dst, pairs, &map);
    let independent = rmse(src, dst, &validation, &map);
    let maximum = errors.into_iter().fold(0., f64::max);
    result.rmse_px = all.is_finite().then_some(all);
    result.validation_rmse_px = independent.is_finite().then_some(independent);
    result.maximum_error_px = maximum.is_finite().then_some(maximum);
    if !all.is_finite()
        || !independent.is_finite()
        || !maximum.is_finite()
        || all > 1.
        || independent > 1.
        || maximum > 3.
    {
        result.reason = "absolute_field_accuracy_failed";
        return result;
    }
    for quadrant in 0..4 {
        let cell: Vec<_> = validation
            .iter()
            .filter(|m| {
                outer(&src[m.source], region) && quadrant_of(&src[m.source], region) == quadrant
            })
            .cloned()
            .collect();
        if cell.is_empty() {
            result.reason = "insufficient_outer_field_evidence";
            return result;
        }
        if rmse(src, dst, &cell, &map) > 1. {
            result.reason = "outer_field_accuracy_failed";
            return result;
        }
    }
    result.accepted = true;
    result.reason = "absolute_field_accuracy_confirmed";
    result
}

/// Source stars (mapped into the target canvas) with an extended-catalog
/// star within 3 px. Two independently truncated catalogs of a dense field
/// share only part of their stars; a larger reference catalog measures
/// coverage without matching the accuracy witnesses against faint stars.
pub(crate) fn covered(
    src: &[Star],
    extended: &[Star],
    target: [usize; 2],
    map: impl Fn([f64; 2]) -> [f64; 2],
) -> usize {
    src.iter()
        .filter(|s| {
            let p = map([s.x_px, s.y_px]);
            p[0] >= 0.
                && p[1] >= 0.
                && p[0] < target[0] as f64
                && p[1] < target[1] as f64
                && extended
                    .iter()
                    .any(|r| (p[0] - r.x_px).hypot(p[1] - r.y_px) <= 3.)
        })
        .count()
}

/// A dense catalog pairs a few stars by chance inside the broad window. A true
/// local error is shared by the neighbouring stars and a chance pair stands
/// alone, so only that one goes; more than a few mean a bad map and all stay.
pub(crate) fn without_chance_pairs(
    src: &[Star],
    dst: &[Star],
    pairs: Vec<Match>,
    map: impl Fn([f64; 2]) -> [f64; 2],
) -> Vec<Match> {
    const NEIGHBOURS: usize = 8;
    let errors: Vec<f64> = pairs
        .iter()
        .map(|m| {
            let s = &src[m.source];
            let target = &dst[m.reference];
            let p = map([s.x_px, s.y_px]);
            (p[0] - target.x_px).hypot(p[1] - target.y_px)
        })
        .collect();
    let chance: BTreeSet<usize> = (0..pairs.len())
        .filter(|&i| errors[i] > 3.)
        .filter(|&i| {
            let here = &src[pairs[i].source];
            let mut near: Vec<_> = (0..pairs.len())
                .filter(|&j| j != i)
                .map(|j| {
                    let s = &src[pairs[j].source];
                    ((s.x_px - here.x_px).hypot(s.y_px - here.y_px), errors[j])
                })
                .collect();
            near.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut local: Vec<_> = near.iter().take(NEIGHBOURS).map(|n| n.1).collect();
            local.sort_by(f64::total_cmp);
            local.len() == NEIGHBOURS && local[NEIGHBOURS / 2 - 1] < 1.
        })
        .collect();
    if chance.len() * 20 > pairs.len() {
        return pairs;
    }
    pairs
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !chance.contains(i))
        .map(|(_, m)| m)
        .collect()
}

/// Build a fixed broad-window witness pool once. Expected source count includes
/// all catalog stars projected into the target canvas, not only fitted inliers.
pub(crate) fn witness_pool(
    src: &[Star],
    dst: &[Star],
    target: [usize; 2],
    map: impl Fn([f64; 2]) -> [f64; 2],
) -> (Vec<Match>, usize) {
    witness_pool_within(src, dst, target, map, 12.)
}

/// As `witness_pool` with another matching window.
pub(crate) fn witness_pool_within(
    src: &[Star],
    dst: &[Star],
    target: [usize; 2],
    map: impl Fn([f64; 2]) -> [f64; 2],
    radius: f64,
) -> (Vec<Match>, usize) {
    let expected = src
        .iter()
        .filter(|s| {
            let p = map([s.x_px, s.y_px]);
            p[0] >= 0. && p[1] >= 0. && p[0] < target[0] as f64 && p[1] < target[1] as f64
        })
        .count();
    (matches(src, dst, map, radius), expected)
}
