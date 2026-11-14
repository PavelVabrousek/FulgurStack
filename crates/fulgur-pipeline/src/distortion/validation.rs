//! Absolute field acceptance, separate from optional nonlinear improvement.
use super::*;

/// How closely a frame must fit to join the stack (ADR-121). The defaults are
/// the limits of every earlier release.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct AccuracyLimits {
    /// RMS of all witnesses, of the held-out ones and of each outer quadrant.
    pub rms_px: u32,
    /// Largest error of a single witness.
    pub maximum_px: u32,
    /// Share of the stars the weaker of frame and reference shows in their
    /// shared field that must find their match.
    pub star_percent: u32,
    /// Matched stars no frame may have fewer of.
    pub min_stars: usize,
}

impl Default for AccuracyLimits {
    fn default() -> Self {
        Self::MAXIMUM_ACCURACY
    }
}

impl AccuracyLimits {
    pub const MAXIMUM_ACCURACY: Self = Self {
        rms_px: 1,
        maximum_px: 3,
        star_percent: 80,
        min_stars: 50,
    };
    pub const COMPROMISE: Self = Self {
        rms_px: 2,
        maximum_px: 6,
        star_percent: 60,
        min_stars: 30,
    };
    pub const MAXIMUM_COVERAGE: Self = Self {
        rms_px: 3,
        maximum_px: 9,
        star_percent: 40,
        min_stars: 20,
    };

    /// Enough of the expected stars found.
    pub(crate) fn covers(self, found: usize, expected: usize) -> bool {
        found * 100 >= expected * self.star_percent as usize
    }

    /// Witnesses a map was not fitted on; 10 at 50 stars, never below 4.
    fn held_out_stars(self) -> usize {
        (self.min_stars / 5).max(4)
    }

    /// Witnesses the map may be fitted on; 30 at 50 stars.
    fn training_stars(self) -> usize {
        self.min_stars * 3 / 5
    }

    /// The 12 px witness window pairs a lone star with a neighbour by chance,
    /// typically in a double star. Up to 3 px such a pair fails the limits and
    /// the 3 px catalog decides; looser limits would count it as a true error
    /// (ADR-121), so it goes, as in dense fields.
    pub(crate) fn without_chance_pairs(
        self,
        src: &[Star],
        dst: &[Star],
        pairs: Vec<Match>,
        map: impl Fn([f64; 2]) -> [f64; 2],
    ) -> Vec<Match> {
        if self.maximum_px > 3 {
            without_chance_pairs(src, dst, pairs, map)
        } else {
            pairs
        }
    }
}

/// Reference stars inside the frame's footprint. A frame's list can hold
/// fainter stars than the reference shows, so the expected count follows the
/// weaker side.
fn inside_footprint(dst: &[Star], field: Field, map: &impl Fn([f64; 2]) -> [f64; 2]) -> usize {
    let corners = field.corners().map(map);
    let side = |a: [f64; 2], b: [f64; 2], p: [f64; 2]| {
        (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
    };
    dst.iter()
        .filter(|s| {
            let p = [s.x_px, s.y_px];
            let sides: Vec<f64> = (0..4)
                .map(|i| side(corners[i], corners[(i + 1) % 4], p))
                .collect();
            sides.iter().all(|&v| v >= 0.) || sides.iter().all(|&v| v <= 0.)
        })
        .count()
}

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
        .filter(|m| crate::geometry::held_out(m))
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
/// failures. Both all-star and independently reserved errors must pass, over
/// `field`: the whole frame, or the part of a wider one the grid shows.
#[allow(clippy::too_many_arguments)] // Fixed witnesses, split, geometry and optional coverage.
pub(crate) fn assess(
    src: &[Star],
    dst: &[Star],
    pairs: &[Match],
    reserved: &BTreeSet<usize>,
    field: Field,
    expected: usize,
    covered: Option<usize>,
    map: impl Fn([f64; 2]) -> [f64; 2],
    limits: AccuracyLimits,
) -> FieldValidation {
    let expected = expected.min(inside_footprint(dst, field, &map));
    let size = field.size();
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
    if pairs.len() < limits.min_stars
        || train.len() < limits.training_stars()
        || validation.len() < limits.held_out_stars()
        || !limits.covers(covered.unwrap_or(pairs.len()), expected)
    {
        return result;
    }
    let region = field_region(src, pairs);
    if region[2] - region[0] < size[0] * 0.35
        || region[3] - region[1] < size[1] * 0.35
        || coverage(src, pairs, region) < 10
        || coverage(src, &validation, region) < limits.held_out_stars().min(6)
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
        || all > f64::from(limits.rms_px)
        || independent > f64::from(limits.rms_px)
        || maximum > f64::from(limits.maximum_px)
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
        if rmse(src, dst, &cell, &map) > f64::from(limits.rms_px) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn star(x: f64, y: f64) -> Star {
        Star {
            x_px: x,
            y_px: y,
            hfr_px: 2.,
            moment_fwhm_px: 4.,
            eccentricity: 0.1,
            flux_adu: 1000.,
            peak_adu: 500.,
            snr: 50.,
        }
    }

    #[test]
    fn limits_count_stars_on_the_weaker_side() {
        // 100 frame stars on a grid; 40 of them, spread over the field, match.
        let src: Vec<_> = (0..100)
            .map(|i| star(50. + (i % 10) as f64 * 100., 50. + (i / 10) as f64 * 100.))
            .collect();
        let chosen: Vec<usize> = (0..100).filter(|i| (i % 10 + i / 10) % 5 < 2).collect();
        assert_eq!(chosen.len(), 40);
        let shifted = |i: usize| star(src[i].x_px + 0.3, src[i].y_px);
        let pairs: Vec<_> = chosen
            .iter()
            .enumerate()
            .map(|(k, &i)| Match {
                source: i,
                reference: k,
                residual_px: 0.3,
            })
            .collect();
        let reserved = held_out(&src, &pairs);
        let sparse: Vec<_> = chosen.iter().map(|&i| shifted(i)).collect();
        let mut rich = sparse.clone();
        rich.extend((0..100).filter(|i| !chosen.contains(i)).map(shifted));
        let check = |dst: &[Star], limits| {
            assess(
                &src,
                dst,
                &pairs,
                &reserved,
                Field::sensor([1000, 1000]),
                100,
                None,
                |p| p,
                limits,
            )
        };
        // Today's limits need 50 matched stars.
        assert!(!check(&sparse, AccuracyLimits::MAXIMUM_ACCURACY).accepted);
        // The reference shows only these 40 stars, so all of them matched.
        let joined = check(&sparse, AccuracyLimits::COMPROMISE);
        assert!(joined.accepted, "{joined:?}");
        // A reference with all 100 stars needs 60 of them at this level.
        let refused = check(&rich, AccuracyLimits::COMPROMISE);
        assert_eq!(refused.reason, "insufficient_field_evidence");
        assert!(check(&rich, AccuracyLimits::MAXIMUM_COVERAGE).accepted);
        // An error above the RMS limit fails at every level that does not allow it.
        let far: Vec<_> = sparse.iter().map(|s| star(s.x_px + 1.5, s.y_px)).collect();
        assert_eq!(
            check(&far, AccuracyLimits::COMPROMISE).reason,
            "absolute_field_accuracy_confirmed"
        );
        assert_eq!(
            check(
                &far,
                AccuracyLimits {
                    min_stars: 30,
                    star_percent: 60,
                    ..AccuracyLimits::MAXIMUM_ACCURACY
                }
            )
            .reason,
            "absolute_field_accuracy_failed"
        );
    }

    #[test]
    fn a_wider_frame_is_judged_on_the_part_the_grid_shows() {
        let src: Vec<_> = (0..100)
            .map(|i| star(50. + (i % 10) as f64 * 100., 50. + (i / 10) as f64 * 100.))
            .collect();
        let dst: Vec<_> = src.iter().map(|s| star(s.x_px + 0.3, s.y_px)).collect();
        let pairs: Vec<_> = (0..100)
            .map(|i| Match {
                source: i,
                reference: i,
                residual_px: 0.3,
            })
            .collect();
        let reserved = held_out(&src, &pairs);
        let check = |field| {
            assess(
                &src,
                &dst,
                &pairs,
                &reserved,
                field,
                100,
                None,
                |p| [p[0] + 0.3, p[1]],
                AccuracyLimits::MAXIMUM_ACCURACY,
            )
        };
        // The stars fill the grid's 1000 px part of a 4000 px sensor; the
        // rest of the sensor never reaches the stack.
        let whole = check(Field::sensor([4000, 4000]));
        assert_eq!(whole.reason, "insufficient_field_evidence");
        let part = Field {
            min: [0., 0.],
            max: [1000., 1000.],
        };
        assert!(check(part).accepted, "{:?}", check(part));
    }
}
