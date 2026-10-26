//! Absolute field acceptance, separate from optional nonlinear improvement.
use super::*;

#[derive(Clone, Debug, Serialize)]
pub struct FieldValidation {
    pub accepted: bool,
    pub reason: &'static str,
    pub fixed_stars: usize,
    pub expected_stars: usize,
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
pub(crate) fn assess(
    src: &[Star],
    dst: &[Star],
    pairs: &[Match],
    reserved: &BTreeSet<usize>,
    sensor: [usize; 2],
    expected: usize,
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
        validation_stars: validation.len(),
        rmse_px: None,
        validation_rmse_px: None,
        maximum_error_px: None,
    };
    if pairs.len() < 50
        || train.len() < 30
        || validation.len() < 10
        || pairs.len() * 5 < expected * 4
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

/// Build a fixed broad-window witness pool once. Expected source count includes
/// all catalog stars projected into the target canvas, not only fitted inliers.
pub(crate) fn witness_pool(
    src: &[Star],
    dst: &[Star],
    target: [usize; 2],
    map: impl Fn([f64; 2]) -> [f64; 2],
) -> (Vec<Match>, usize) {
    let expected = src
        .iter()
        .filter(|s| {
            let p = map([s.x_px, s.y_px]);
            p[0] >= 0. && p[1] >= 0. && p[0] < target[0] as f64 && p[1] < target[1] as f64
        })
        .count();
    (matches(src, dst, map, 12.), expected)
}
