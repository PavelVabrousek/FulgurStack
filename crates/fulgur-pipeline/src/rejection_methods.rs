//! Pixel rejection methods besides winsorized sigma clipping (ADR-087):
//! median, linear fit clipping and the generalized extreme studentized
//! deviate (ESD) test. Each decides which samples of one pixel survive;
//! survivors are then averaged with their frame weights as before.

use super::Sum;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

/// Linear fit clipping limits, in robust sigmas of the fit residuals.
pub(super) const LINEAR_FIT_LOW: f64 = 5.0;
pub(super) const LINEAR_FIT_HIGH: f64 = 4.0;
const LINEAR_FIT_ITERATIONS: usize = 10;
/// ESD significance and the largest share of a stack it may reject.
pub(super) const ESD_ALPHA: f64 = 0.05;
pub(super) const ESD_MAX_OUTLIERS: f64 = 0.3;
/// Stacks smaller than this are averaged without rejection, as winsorized.
pub(super) const MIN_SAMPLES: usize = 5;

/// Per-worker storage reused across pixels and channels.
#[derive(Default)]
pub(super) struct MethodScratch {
    sorted: Vec<(f32, f64)>,
    keys: Vec<i32>,
    keep: Vec<bool>,
    residuals: Vec<f64>,
    /// ESD critical values per stack size, from `esd_limits`.
    esd_lambda: HashMap<usize, Arc<[f64]>>,
}

type Combined = (f32, u64, u64, f64);

fn total_weight(weights: &[f64]) -> f64 {
    let mut sum = Sum::default();
    for &w in weights {
        sum.add(w);
    }
    sum.value()
}

/// Integer whose order is `f32::total_cmp`; selecting on it is much faster
/// than on the floats and gives the same order statistics.
#[inline]
pub(super) fn sample_key(value: f32) -> i32 {
    let bits = value.to_bits() as i32;
    bits ^ (((bits >> 31) as u32) >> 1) as i32
}

/// The sample of a `sample_key`; the mapping is its own inverse.
#[inline]
pub(super) fn key_sample(key: i32) -> f32 {
    f32::from_bits((key ^ (((key >> 31) as u32) >> 1) as i32) as u32)
}

/// Median of the samples (unweighted; robust, no rejection counted).
pub(super) fn median(values: &[f32], weights: &[f64], scratch: &mut MethodScratch) -> Combined {
    if values.is_empty() {
        return (0., 0, 0, 0.);
    }
    median_of(values, total_weight(weights), scratch)
}

/// As `median`, with the total weight of the samples already known. Selects
/// the middle sample and needs no full sort.
pub(super) fn median_of(values: &[f32], weight: f64, scratch: &mut MethodScratch) -> Combined {
    if values.is_empty() {
        return (0., 0, 0, 0.);
    }
    let keys = &mut scratch.keys;
    keys.clear();
    keys.extend(values.iter().map(|&v| sample_key(v)));
    let n = keys.len();
    let (below, middle, _) = keys.select_nth_unstable(n / 2);
    let upper = f64::from(key_sample(*middle));
    let value = match below.iter().max() {
        Some(&lower) if n % 2 == 0 => (f64::from(key_sample(lower)) + upper) * 0.5,
        _ => upper,
    };
    (value as f32, 0, 0, weight)
}

/// Weighted mean of the sorted samples flagged in `keep`, with the number of
/// rejected samples below and above the survivors' median.
fn survivors(sorted: &[(f32, f64)], keep: &[bool]) -> Combined {
    let mut numerator = Sum::default();
    let mut denominator = Sum::default();
    let kept: Vec<usize> = (0..sorted.len()).filter(|&i| keep[i]).collect();
    let middle = kept
        .get(kept.len() / 2)
        .copied()
        .unwrap_or(sorted.len() / 2);
    let (mut low, mut high) = (0, 0);
    for (i, &(v, w)) in sorted.iter().enumerate() {
        if keep[i] {
            numerator.add(f64::from(v) * w);
            denominator.add(w);
        } else if i < middle {
            low += 1;
        } else {
            high += 1;
        }
    }
    let weight = denominator.value();
    if weight == 0. {
        (0., low, high, 0.)
    } else {
        ((numerator.value() / weight) as f32, low, high, weight)
    }
}

fn prepare(values: &[f32], weights: &[f64], scratch: &mut MethodScratch) {
    scratch.sorted.clear();
    scratch
        .sorted
        .extend(values.iter().copied().zip(weights.iter().copied()));
    scratch.sorted.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
    scratch.keep.clear();
    scratch.keep.resize(values.len(), true);
}

/// Linear fit clipping: a straight line is fitted to the sorted surviving
/// samples against their rank; samples farther than `LINEAR_FIT_LOW` below or
/// `LINEAR_FIT_HIGH` above it (in robust sigmas: 1.2533 x mean absolute
/// residual) are rejected, until nothing changes. A gradual spread of levels
/// between frames (sky gradients, several nights) follows the line and stays.
pub(super) fn linear_fit(values: &[f32], weights: &[f64], scratch: &mut MethodScratch) -> Combined {
    if values.is_empty() {
        return (0., 0, 0, 0.);
    }
    prepare(values, weights, scratch);
    let MethodScratch {
        sorted,
        keep,
        residuals,
        ..
    } = scratch;
    if sorted.len() >= MIN_SAMPLES {
        for _ in 0..LINEAR_FIT_ITERATIONS {
            let alive: Vec<usize> = (0..sorted.len()).filter(|&i| keep[i]).collect();
            if alive.len() < 3 {
                break;
            }
            let m = alive.len() as f64;
            let (mut sx, mut sy, mut sxx, mut sxy) = (0., 0., 0., 0.);
            for (rank, &i) in alive.iter().enumerate() {
                let (x, y) = (rank as f64, f64::from(sorted[i].0));
                sx += x;
                sy += y;
                sxx += x * x;
                sxy += x * y;
            }
            let denominator = m * sxx - sx * sx;
            let slope = if denominator.abs() > 0. {
                (m * sxy - sx * sy) / denominator
            } else {
                0.
            };
            let intercept = (sy - slope * sx) / m;
            residuals.clear();
            residuals.extend(
                alive
                    .iter()
                    .enumerate()
                    .map(|(rank, &i)| f64::from(sorted[i].0) - (intercept + slope * rank as f64)),
            );
            let sigma = 1.2533 * residuals.iter().map(|r| r.abs()).sum::<f64>() / m;
            if sigma <= 0. || !sigma.is_finite() {
                break;
            }
            let mut changed = false;
            for (r, &i) in residuals.iter().zip(&alive) {
                if *r < -LINEAR_FIT_LOW * sigma || *r > LINEAR_FIT_HIGH * sigma {
                    keep[i] = false;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }
    survivors(sorted, keep)
}

/// Generalized ESD test (Rosner 1983): up to `ESD_MAX_OUTLIERS` of the stack
/// is tested, removing the most extreme sample (either end) each step; the
/// outliers are the first `k` removals, `k` being the last step whose
/// studentized deviation exceeds its critical value at significance
/// `ESD_ALPHA`. The samples are sorted once and running sums give each step
/// in constant time.
pub(super) fn esd(values: &[f32], weights: &[f64], scratch: &mut MethodScratch) -> Combined {
    if values.is_empty() {
        return (0., 0, 0, 0.);
    }
    prepare(values, weights, scratch);
    let n = scratch.sorted.len();
    let tests = (n as f64 * ESD_MAX_OUTLIERS).floor() as usize;
    if n >= MIN_SAMPLES && tests > 0 {
        let lambda = scratch.esd_lambda.entry(n).or_insert_with(|| esd_limits(n));
        let sorted = &scratch.sorted;
        // Shift by the median so the running variance keeps its precision.
        let shift = f64::from(sorted[n / 2].0);
        let (mut s1, mut s2) = (0., 0.);
        for &(v, _) in sorted.iter() {
            let d = f64::from(v) - shift;
            s1 += d;
            s2 += d * d;
        }
        let (mut lo, mut hi) = (0, n - 1);
        let mut removed = Vec::with_capacity(tests);
        let mut outliers = 0;
        for (step, &critical) in lambda.iter().enumerate() {
            let m = (n - step) as f64;
            let mean = s1 / m;
            let variance = (s2 / m - mean * mean) * m / (m - 1.);
            if variance <= 0. || !variance.is_finite() {
                break;
            }
            let sd = variance.sqrt();
            let low_dev = mean - (f64::from(sorted[lo].0) - shift);
            let high_dev = (f64::from(sorted[hi].0) - shift) - mean;
            let (index, deviation) = if high_dev >= low_dev {
                (hi, high_dev)
            } else {
                (lo, low_dev)
            };
            if deviation / sd > critical {
                outliers = step + 1;
            }
            removed.push(index);
            let d = f64::from(sorted[index].0) - shift;
            s1 -= d;
            s2 -= d * d;
            if index == hi {
                hi -= 1;
            } else {
                lo += 1;
            }
            if lo >= hi {
                break;
            }
        }
        for &i in &removed[..outliers] {
            scratch.keep[i] = false;
        }
    }
    survivors(&scratch.sorted, &scratch.keep)
}

/// ESD critical values for n samples. They depend on n only and take
/// milliseconds each, while a tile can hold hundreds of sample counts, so
/// all workers and stacks of the process share them.
pub(super) fn esd_limits(n: usize) -> Arc<[f64]> {
    static LIMITS: LazyLock<Mutex<HashMap<usize, Arc<[f64]>>>> = LazyLock::new(Default::default);
    let cached = LIMITS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&n)
        .cloned();
    cached.unwrap_or_else(|| {
        let tests = (n as f64 * ESD_MAX_OUTLIERS).floor() as usize;
        let limits = esd_critical_values(n, tests, ESD_ALPHA).into();
        LIMITS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(n)
            .or_insert(limits)
            .clone()
    })
}

/// ESD critical values lambda_i, i = 1..tests, for a stack of n samples.
pub(super) fn esd_critical_values(n: usize, tests: usize, alpha: f64) -> Vec<f64> {
    (1..=tests.min(n.saturating_sub(3)))
        .map(|i| {
            let remaining = (n - i + 1) as f64;
            let p = 1. - alpha / (2. * remaining);
            let df = remaining - 2.;
            let t = student_t_quantile(p, df);
            (remaining - 1.) * t / ((df + t * t) * remaining).sqrt()
        })
        .collect()
}

/// Quantile of Student's t distribution by bisection on its CDF.
pub(super) fn student_t_quantile(p: f64, df: f64) -> f64 {
    let (mut lo, mut hi) = (0., 1e4);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if student_t_cdf(mid, df) < p {
            lo = mid;
        } else {
            hi = mid;
        }
        if hi - lo < 1e-10 * hi.max(1.) {
            break;
        }
    }
    0.5 * (lo + hi)
}

fn student_t_cdf(t: f64, df: f64) -> f64 {
    let x = df / (df + t * t);
    let tail = 0.5 * incomplete_beta(0.5 * df, 0.5, x);
    if t >= 0. { 1. - tail } else { tail }
}

/// Regularized incomplete beta I_x(a, b) (continued fraction, Lentz).
fn incomplete_beta(a: f64, b: f64, x: f64) -> f64 {
    if x <= 0. {
        return 0.;
    }
    if x >= 1. {
        return 1.;
    }
    let front =
        (ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1. - x).ln()).exp();
    if x < (a + 1.) / (a + b + 2.) {
        front * beta_fraction(a, b, x) / a
    } else {
        1. - front * beta_fraction(b, a, 1. - x) / b
    }
}

fn beta_fraction(a: f64, b: f64, x: f64) -> f64 {
    const TINY: f64 = 1e-300;
    let (qab, qap, qam) = (a + b, a + 1., a - 1.);
    let mut c = 1.;
    let mut d = 1. - qab * x / qap;
    if d.abs() < TINY {
        d = TINY;
    }
    d = 1. / d;
    let mut h = d;
    for m in 1..400 {
        let m = m as f64;
        let m2 = 2. * m;
        let aa = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = 1. + aa * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1. + aa / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1. / d;
        h *= d * c;
        let aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = 1. + aa * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1. + aa / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1. / d;
        let delta = d * c;
        h *= delta;
        if (delta - 1.).abs() < 1e-14 {
            break;
        }
    }
    h
}

/// ln Gamma(x) for x > 0 (Lanczos, g = 7).
fn ln_gamma(x: f64) -> f64 {
    const C: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        let pi = std::f64::consts::PI;
        return (pi / (pi * x).sin()).ln() - ln_gamma(1. - x);
    }
    let x = x - 1.;
    let mut a = C[0];
    let t = x + 7.5;
    for (i, &c) in C.iter().enumerate().skip(1) {
        a += c / (x + i as f64);
    }
    0.5 * (2. * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
}

#[cfg(test)]
#[path = "rejection_methods_tests.rs"]
mod tests;
