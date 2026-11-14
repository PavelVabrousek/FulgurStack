//! Generalized ESD over one reduction band, with the outcome of
//! `rejection_methods::esd` (ADR-111). Sample columns of neighbouring pixels
//! are sorted together by a data-oblivious network, each sorted column is
//! tested, and the survivors are summed in frame order by their value range.
//! With AVX2 four columns are tested and summed at once, each lane repeating
//! the scalar arithmetic in the same order (ADR-146).

use super::rejection_methods::{self, MIN_SAMPLES, MethodScratch, esd_limits};
use super::rgb_rejection::PreparedRgb;
use super::{
    IntegratedPixel, REJECTION_BLOCK_PIXELS, ReductionContext, RowStats, Sum, pack_rejection_block,
};
use fulgur_core::{Result, check_cancelled};
use std::sync::Arc;

/// How sample columns are sorted. Every kernel gives the same sorted values,
/// so the choice changes speed only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kernel {
    Scalar,
    #[cfg(target_arch = "x86_64")]
    Avx2,
}

impl Kernel {
    pub fn detect() -> Self {
        #[cfg(target_arch = "x86_64")]
        {
            if std::is_x86_feature_detected!("avx2") {
                return Self::Avx2;
            }
        }
        Self::Scalar
    }

    fn lanes(self) -> usize {
        match self {
            Self::Scalar => 1,
            #[cfg(target_arch = "x86_64")]
            Self::Avx2 => 8,
        }
    }

    /// Sorts each lane of `rows` (row-major, `lanes()` values per row) over
    /// its first `n` rows. Values must not be NaN.
    fn sort(self, rows: &mut [f32], n: usize) {
        assert!(n * self.lanes() <= rows.len());
        match self {
            Self::Scalar => rows[..n].sort_unstable_by(f32::total_cmp),
            // SAFETY: only detect() or a detection check in the tests yields
            // these kernels, and the length is checked above.
            #[cfg(target_arch = "x86_64")]
            Self::Avx2 => unsafe { vector::sort_columns(rows, n) },
        }
    }
}

#[cfg(target_arch = "x86_64")]
mod vector {
    use std::arch::x86_64::*;

    const LANES: usize = 8;

    /// One compensated addition per lane where `mask` is set, the arithmetic
    /// of `Sum::add` in the same order; other lanes keep their state.
    #[target_feature(enable = "avx2")]
    #[inline]
    unsafe fn add(total: &mut __m256d, correction: &mut __m256d, x: __m256d, mask: __m256d) {
        let sign = _mm256_set1_pd(-0.0);
        let next = _mm256_add_pd(*total, x);
        let bigger =
            _mm256_cmp_pd::<_CMP_GE_OQ>(_mm256_andnot_pd(sign, *total), _mm256_andnot_pd(sign, x));
        let when_bigger = _mm256_add_pd(_mm256_sub_pd(*total, next), x);
        let otherwise = _mm256_add_pd(_mm256_sub_pd(x, next), *total);
        let corrected = _mm256_add_pd(
            *correction,
            _mm256_blendv_pd(otherwise, when_bigger, bigger),
        );
        *total = _mm256_blendv_pd(*total, next, mask);
        *correction = _mm256_blendv_pd(*correction, corrected, mask);
    }

    /// `survivor_window` for four lanes of the sorted `rows` (row-major,
    /// `LANES` values per row) from lane `base` on, each lane with its own
    /// sample count and critical values. A lane with count 0 is not tested.
    #[target_feature(enable = "avx2")]
    pub(super) unsafe fn survivor_windows(
        rows: &[f32],
        base: usize,
        counts: [usize; 4],
        lambdas: [&[f64]; 4],
        inverse: &[f64],
    ) -> [(usize, usize); 4] {
        let longest = counts.into_iter().max().unwrap_or(0);
        assert!(base + 4 <= LANES && longest * LANES <= rows.len());
        assert!(counts.iter().all(|&n| n < inverse.len()));
        let at = |row: usize, k: usize| f64::from(rows[row * LANES + base + k]);
        let shift: [f64; 4] = std::array::from_fn(|k| at(counts[k] / 2, k));
        // SAFETY (all loads and stores below): every pointer is to a local
        // array of four values, or to `rows` within the length checked above.
        let shifts = unsafe { _mm256_loadu_pd(shift.as_ptr()) };
        let lengths = _mm256_setr_pd(
            counts[0] as f64,
            counts[1] as f64,
            counts[2] as f64,
            counts[3] as f64,
        );
        // Same running sums, in the same order, as the scalar test.
        let (mut s1, mut s2) = (_mm256_setzero_pd(), _mm256_setzero_pd());
        for row in 0..longest {
            let v = unsafe { _mm256_cvtps_pd(_mm_loadu_ps(rows.as_ptr().add(row * LANES + base))) };
            let d = _mm256_sub_pd(v, shifts);
            let inside = _mm256_cmp_pd::<_CMP_LT_OQ>(_mm256_set1_pd(row as f64), lengths);
            s1 = _mm256_blendv_pd(s1, _mm256_add_pd(s1, d), inside);
            s2 = _mm256_blendv_pd(s2, _mm256_add_pd(s2, _mm256_mul_pd(d, d)), inside);
        }
        let mut lo = [0usize; 4];
        let mut hi: [usize; 4] = std::array::from_fn(|k| counts[k].saturating_sub(1));
        let mut window: [(usize, usize); 4] = std::array::from_fn(|k| (lo[k], hi[k]));
        let mut active: [bool; 4] = std::array::from_fn(|k| counts[k] > 0);
        let sign = _mm256_set1_pd(-0.0);
        let mut step = 0;
        loop {
            for k in 0..4 {
                active[k] &= step < lambdas[k].len();
            }
            if !active.contains(&true) {
                break;
            }
            let pick = |values: [f64; 4]| unsafe { _mm256_loadu_pd(values.as_ptr()) };
            let inverse_m = pick(std::array::from_fn(|k| {
                if active[k] {
                    inverse[counts[k] - step]
                } else {
                    0.
                }
            }));
            let inverse_m1 = pick(std::array::from_fn(|k| {
                if active[k] {
                    inverse[counts[k] - step - 1]
                } else {
                    0.
                }
            }));
            let mean = _mm256_mul_pd(s1, inverse_m);
            let variance = _mm256_mul_pd(_mm256_sub_pd(s2, _mm256_mul_pd(mean, s1)), inverse_m1);
            // Stop where the variance is not positive, or not finite.
            let stop = _mm256_or_pd(
                _mm256_cmp_pd::<_CMP_LE_OQ>(variance, _mm256_setzero_pd()),
                _mm256_cmp_pd::<_CMP_NLT_UQ>(
                    _mm256_andnot_pd(sign, variance),
                    _mm256_set1_pd(f64::INFINITY),
                ),
            );
            let stop = _mm256_movemask_pd(stop);
            for (k, lane) in active.iter_mut().enumerate() {
                *lane &= stop & (1 << k) == 0;
            }
            let low = _mm256_sub_pd(
                pick(std::array::from_fn(|k| {
                    if active[k] { at(lo[k], k) } else { 0. }
                })),
                shifts,
            );
            let high = _mm256_sub_pd(
                pick(std::array::from_fn(|k| {
                    if active[k] { at(hi[k], k) } else { 0. }
                })),
                shifts,
            );
            let (low_dev, high_dev) = (_mm256_sub_pd(mean, low), _mm256_sub_pd(high, mean));
            let take_high = _mm256_cmp_pd::<_CMP_GE_OQ>(high_dev, low_dev);
            let deviation = _mm256_blendv_pd(low_dev, high_dev, take_high);
            let removed = _mm256_blendv_pd(low, high, take_high);
            let live = pick(std::array::from_fn(|k| {
                if active[k] {
                    f64::from_bits(u64::MAX)
                } else {
                    0.
                }
            }));
            s1 = _mm256_blendv_pd(s1, _mm256_sub_pd(s1, removed), live);
            s2 = _mm256_blendv_pd(s2, _mm256_sub_pd(s2, _mm256_mul_pd(removed, removed)), live);
            let critical = pick(std::array::from_fn(|k| {
                if active[k] { lambdas[k][step] } else { 0. }
            }));
            // Squared, so a pixel without outliers costs no square root.
            let outlier = _mm256_cmp_pd::<_CMP_GT_OQ>(
                _mm256_mul_pd(deviation, deviation),
                _mm256_mul_pd(_mm256_mul_pd(critical, critical), variance),
            );
            let (take_high, outlier) = (_mm256_movemask_pd(take_high), _mm256_movemask_pd(outlier));
            for k in 0..4 {
                if !active[k] {
                    continue;
                }
                if take_high & (1 << k) != 0 {
                    hi[k] -= 1;
                } else {
                    lo[k] += 1;
                }
                if outlier & (1 << k) != 0 {
                    window[k] = (lo[k], hi[k]);
                }
                if lo[k] >= hi[k] {
                    active[k] = false;
                }
            }
            step += 1;
        }
        window
    }

    /// Survivor sums of four columns: per lane, the weighted sum of the
    /// samples of channel `channels[k]` of `stacks[k]` within
    /// `[floor[k], ceiling[k]]`, and the sum of their weights, each as
    /// (total, correction) of `Sum`, added in frame order.
    #[target_feature(enable = "avx2")]
    pub(super) unsafe fn survivor_sums(
        stacks: [&[[f32; 3]]; 4],
        channels: [usize; 4],
        floor: [f32; 4],
        ceiling: [f32; 4],
        weights: &[f64],
    ) -> [[f64; 4]; 4] {
        for stack in stacks {
            assert!(stack.len() >= weights.len());
        }
        // SAFETY: each array holds the four values loaded.
        let (low, high) = unsafe {
            (
                _mm256_cvtps_pd(_mm_loadu_ps(floor.as_ptr())),
                _mm256_cvtps_pd(_mm_loadu_ps(ceiling.as_ptr())),
            )
        };
        let zero = _mm256_setzero_pd();
        let (mut num_total, mut num_correction) = (zero, zero);
        let (mut den_total, mut den_correction) = (zero, zero);
        for (i, &weight) in weights.iter().enumerate() {
            let values = [
                stacks[0][i][channels[0]],
                stacks[1][i][channels[1]],
                stacks[2][i][channels[2]],
                stacks[3][i][channels[3]],
            ];
            // SAFETY: `values` holds the four samples loaded.
            let v = unsafe { _mm256_cvtps_pd(_mm_loadu_ps(values.as_ptr())) };
            // Ordered comparisons: a NaN sample is outside, as in the scalar test.
            let inside = _mm256_and_pd(
                _mm256_cmp_pd::<_CMP_GE_OQ>(v, low),
                _mm256_cmp_pd::<_CMP_LE_OQ>(v, high),
            );
            let w = _mm256_set1_pd(weight);
            unsafe {
                add(
                    &mut num_total,
                    &mut num_correction,
                    _mm256_mul_pd(v, w),
                    inside,
                );
                add(&mut den_total, &mut den_correction, w, inside);
            }
        }
        let mut out = [[0.; 4]; 4];
        // SAFETY: each row of `out` holds the four values stored.
        unsafe {
            _mm256_storeu_pd(out[0].as_mut_ptr(), num_total);
            _mm256_storeu_pd(out[1].as_mut_ptr(), num_correction);
            _mm256_storeu_pd(out[2].as_mut_ptr(), den_total);
            _mm256_storeu_pd(out[3].as_mut_ptr(), den_correction);
        }
        out
    }

    #[target_feature(enable = "avx2")]
    #[inline]
    unsafe fn exchange(rows: *mut f32, a: usize, b: usize) {
        // SAFETY: the caller passes rows a < b < n of a slice holding at
        // least n * LANES values.
        unsafe {
            let (low, high) = (rows.add(a * LANES), rows.add(b * LANES));
            let (x, y) = (_mm256_loadu_ps(low), _mm256_loadu_ps(high));
            _mm256_storeu_ps(low, _mm256_min_ps(x, y));
            _mm256_storeu_ps(high, _mm256_max_ps(x, y));
        }
    }

    /// Bitonic sorter of all lanes at once. Rows from `n` on stand for +inf,
    /// which makes every comparator that reaches them a no-op, so they are
    /// neither stored nor visited.
    #[target_feature(enable = "avx2")]
    pub(super) unsafe fn sort_columns(rows: &mut [f32], n: usize) {
        debug_assert!(n * LANES <= rows.len());
        let rows = rows.as_mut_ptr();
        let mut length = 2;
        while length < n.saturating_mul(2) {
            let mut start = 0;
            while start < n {
                let end = (start + length).min(n);
                for i in 0..length / 2 {
                    let mirror = start + length - 1 - i;
                    if mirror < end {
                        // SAFETY: start + i < mirror < end <= n.
                        unsafe { exchange(rows, start + i, mirror) };
                    }
                }
                let mut half = length / 4;
                while half >= 1 {
                    let mut first = start;
                    while first < end {
                        for i in 0..half.min(end.saturating_sub(first + half)) {
                            // SAFETY: first + i + half < end <= n.
                            unsafe { exchange(rows, first + i, first + i + half) };
                        }
                        first += 2 * half;
                    }
                    half /= 2;
                }
                start += length;
            }
            length *= 2;
        }
    }
}

/// Sorted positions of the first and last survivor of an ascending, finite
/// column, by the steps of `rejection_methods::esd`.
fn survivor_window(sorted: &[f32], lambda: &[f64], inverse: &[f64]) -> (usize, usize) {
    let n = sorted.len();
    // Same running sums, in the same order, as the scalar test.
    let shift = f64::from(sorted[n / 2]);
    let (mut s1, mut s2) = (0., 0.);
    for &v in sorted {
        let d = f64::from(v) - shift;
        s1 += d;
        s2 += d * d;
    }
    let (mut lo, mut hi) = (0, n - 1);
    let mut window = (lo, hi);
    for (step, &critical) in lambda.iter().enumerate() {
        let m = n - step;
        let mean = s1 * inverse[m];
        let variance = (s2 - mean * s1) * inverse[m - 1];
        if variance <= 0. || !variance.is_finite() {
            break;
        }
        let low = f64::from(sorted[lo]) - shift;
        let high = f64::from(sorted[hi]) - shift;
        let (low_dev, high_dev) = (mean - low, high - mean);
        let take_high = high_dev >= low_dev;
        let (deviation, removed) = if take_high {
            (high_dev, high)
        } else {
            (low_dev, low)
        };
        s1 -= removed;
        s2 -= removed * removed;
        hi -= usize::from(take_high);
        lo += usize::from(!take_high);
        // Squared, so a pixel without outliers costs no square root.
        if deviation * deviation > critical * critical * variance {
            window = (lo, hi);
        }
        if lo >= hi {
            break;
        }
    }
    window
}

/// Per-band storage for testing sorted columns.
struct Tester {
    /// Critical values per sample count, shared with every other worker.
    limits: Vec<Option<Arc<[f64]>>>,
    /// 1 / m for each remaining sample count m.
    inverse: Vec<f64>,
    sorted: Vec<f32>,
    values: Vec<f32>,
    weights: Vec<f64>,
    methods: MethodScratch,
    /// Columns waiting for the vector sums, and their results.
    queued: Vec<(Column, Survivors)>,
    finished: Vec<(f32, u64, u64, f64)>,
}

impl Tester {
    fn new(frames: usize) -> Self {
        Self {
            limits: vec![None; frames + 1],
            inverse: (0..=frames).map(|m| 1. / m as f64).collect(),
            sorted: Vec::with_capacity(frames),
            values: Vec::with_capacity(frames),
            weights: Vec::with_capacity(frames),
            methods: MethodScratch::default(),
            queued: Vec::with_capacity(8),
            finished: Vec::with_capacity(8),
        }
    }

    /// Critical values for `n` samples.
    fn lambda(&mut self, n: usize) -> Arc<[f64]> {
        self.limits[n].get_or_insert_with(|| esd_limits(n)).clone()
    }

    /// The scalar path decides which of several equal samples goes, because
    /// their frame weights differ and a value range cannot tell them apart.
    fn combine_tied(
        &mut self,
        stack: &[[f32; 3]],
        channel: usize,
        weights: &[f64],
    ) -> (f32, u64, u64, f64) {
        self.values.clear();
        self.weights.clear();
        for (sample, &weight) in stack.iter().zip(weights) {
            if sample[channel].is_finite() {
                self.values.push(sample[channel]);
                self.weights.push(weight);
            }
        }
        rejection_methods::esd(&self.values, &self.weights, &mut self.methods)
    }

    /// The survivors of `self.sorted` as a value range, or the result when
    /// it needs no sum by range: no samples, or ties at the rejection limit.
    /// `window` is the survivor window when the vector test found it already.
    fn range(
        &mut self,
        stack: &[[f32; 3]],
        channel: usize,
        weights: &[f64],
        window: Option<(usize, usize)>,
    ) -> Outcome {
        let n = self.sorted.len();
        if n == 0 {
            return Outcome::Done((0., 0, 0, 0.));
        }
        let (first, last) = match window {
            Some(window) => window,
            None if n >= MIN_SAMPLES => {
                let lambda = self.lambda(n);
                survivor_window(&self.sorted, &lambda, &self.inverse)
            }
            None => (0, n - 1),
        };
        let sorted = &self.sorted;
        if (first > 0 && sorted[first - 1] == sorted[first])
            || (last + 1 < n && sorted[last + 1] == sorted[last])
        {
            return Outcome::Done(self.combine_tied(stack, channel, weights));
        }
        // Non-finite samples fall outside this finite range.
        Outcome::Range(Survivors {
            floor: sorted[first],
            ceiling: sorted[last],
            low: first as u64,
            high: (n - 1 - last) as u64,
        })
    }
}

/// How a column's survivors are combined.
enum Outcome {
    Done((f32, u64, u64, f64)),
    Range(Survivors),
}

/// Survivors of a column by value range, and the rejected counts.
#[derive(Clone, Copy)]
struct Survivors {
    floor: f32,
    ceiling: f32,
    low: u64,
    high: u64,
}

impl Survivors {
    /// The weighted mean of the survivors in frame order, by `Sum`.
    fn sum(self, stack: &[[f32; 3]], channel: usize, weights: &[f64]) -> (f32, u64, u64, f64) {
        let mut numerator = Sum::default();
        let mut denominator = Sum::default();
        for (sample, &weight) in stack.iter().zip(weights) {
            let v = sample[channel];
            if v >= self.floor && v <= self.ceiling {
                numerator.add(f64::from(v) * weight);
                denominator.add(weight);
            }
        }
        self.finish(numerator.value(), denominator.value())
    }

    fn finish(self, numerator: f64, weight: f64) -> (f32, u64, u64, f64) {
        if weight == 0. {
            (0., self.low, self.high, 0.)
        } else {
            ((numerator / weight) as f32, self.low, self.high, weight)
        }
    }
}

/// One lane of the sort: `count` samples of `channels` of a block pixel.
struct Column {
    pixel: usize,
    channels: std::ops::Range<usize>,
    count: usize,
}

/// Sorts the pending columns together, then tests each one.
struct Flush<'a> {
    kernel: Kernel,
    packed: &'a [[f32; 3]],
    weights: &'a [f64],
    original_input_count: usize,
    total_weight: f64,
}

impl Flush<'_> {
    fn run(
        &self,
        rows: &mut [f32],
        pending: &mut Vec<Column>,
        block: &mut [IntegratedPixel],
        tester: &mut Tester,
        result: &mut RowStats,
    ) {
        let lanes = self.kernel.lanes();
        let frames = self.weights.len();
        let longest = pending.iter().map(|c| c.count).max().unwrap_or(0);
        for (lane, column) in pending.iter().enumerate() {
            for row in column.count..longest {
                rows[row * lanes + lane] = f32::INFINITY;
            }
        }
        self.kernel.sort(rows, longest);
        let windows = self.windows(rows, pending, tester);
        let mut queued = std::mem::take(&mut tester.queued);
        for (lane, column) in pending.drain(..).enumerate() {
            tester.sorted.clear();
            tester.sorted.extend(
                rows[lane..]
                    .iter()
                    .step_by(lanes)
                    .take(column.count)
                    .copied(),
            );
            let stack = &self.packed[column.pixel * frames..][..frames];
            match tester.range(stack, column.channels.start, self.weights, windows[lane]) {
                Outcome::Done(combined) => self.store(column, combined, block, result),
                Outcome::Range(range) if self.kernel == Kernel::Scalar => {
                    let combined = range.sum(stack, column.channels.start, self.weights);
                    self.store(column, combined, block, result);
                }
                Outcome::Range(range) => queued.push((column, range)),
            }
        }
        #[cfg(target_arch = "x86_64")]
        for group in queued.chunks(4) {
            let lane = |k: usize| &group[k.min(group.len() - 1)];
            let stack = |k: usize| &self.packed[lane(k).0.pixel * frames..][..frames];
            // Unused lanes repeat the last column with an empty range.
            let inside = |k: usize| k < group.len();
            // SAFETY: the Avx2 kernel is chosen only where AVX2 was detected.
            let sums = unsafe {
                vector::survivor_sums(
                    [stack(0), stack(1), stack(2), stack(3)],
                    std::array::from_fn(|k| lane(k).0.channels.start),
                    std::array::from_fn(|k| {
                        if inside(k) {
                            lane(k).1.floor
                        } else {
                            f32::INFINITY
                        }
                    }),
                    std::array::from_fn(|k| {
                        if inside(k) {
                            lane(k).1.ceiling
                        } else {
                            f32::NEG_INFINITY
                        }
                    }),
                    self.weights,
                )
            };
            for (k, (_, range)) in group.iter().enumerate() {
                let numerator = sums[0][k] + sums[1][k];
                let weight = sums[2][k] + sums[3][k];
                tester.finished.push(range.finish(numerator, weight));
            }
        }
        for ((column, _), combined) in queued.drain(..).zip(tester.finished.drain(..)) {
            self.store(column, combined, block, result);
        }
        tester.queued = queued;
    }

    /// Survivor windows of the sorted columns found four at a time; None
    /// for the scalar kernel and for columns too short to test.
    fn windows(
        &self,
        rows: &[f32],
        pending: &[Column],
        tester: &mut Tester,
    ) -> [Option<(usize, usize)>; 8] {
        let mut windows = [None; 8];
        #[cfg(target_arch = "x86_64")]
        if self.kernel != Kernel::Scalar {
            for base in [0, 4] {
                let tested = |k: usize| {
                    pending
                        .get(base + k)
                        .map_or(0, |c| if c.count >= MIN_SAMPLES { c.count } else { 0 })
                };
                let counts: [usize; 4] = std::array::from_fn(tested);
                if counts == [0; 4] {
                    continue;
                }
                let lambdas: [Arc<[f64]>; 4] = std::array::from_fn(|k| {
                    if counts[k] > 0 {
                        tester.lambda(counts[k])
                    } else {
                        Arc::from([])
                    }
                });
                // SAFETY: the Avx2 kernel is chosen only where AVX2 was detected.
                let found = unsafe {
                    vector::survivor_windows(
                        rows,
                        base,
                        counts,
                        std::array::from_fn(|k| &lambdas[k][..]),
                        &tester.inverse,
                    )
                };
                for k in 0..4 {
                    if counts[k] > 0 {
                        windows[base + k] = Some(found[k]);
                    }
                }
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        let _ = (rows, pending, tester);
        windows
    }

    /// Writes one column's result to its pixel and counts its samples.
    fn store(
        &self,
        column: Column,
        (value, low, high, surviving): (f32, u64, u64, f64),
        block: &mut [IntegratedPixel],
        result: &mut RowStats,
    ) {
        let pixel = &mut block[column.pixel];
        let denominator = self.original_input_count as f64;
        for c in column.channels {
            pixel.0[c] = value;
            pixel.1[c] = ((column.count as u64 - low - high) as f64 / denominator) as f32;
            pixel.2[c] = (high as f64 / denominator) as f32;
            result.valid += column.count as u64;
            result.low += low;
            result.high += high;
        }
        result.min_surviving = result.min_surviving.min(surviving / self.total_weight);
    }
}

/// `integrate::reduce_band` for the ESD method.
pub(super) fn reduce_band(
    frames: &[Vec<[f32; 3]>],
    offset: usize,
    output: &mut [IntegratedPixel],
    context: &ReductionContext<'_>,
) -> Result<RowStats> {
    reduce_band_with(Kernel::detect(), frames, offset, output, context)
}

fn reduce_band_with(
    kernel: Kernel,
    frames: &[Vec<[f32; 3]>],
    offset: usize,
    output: &mut [IntegratedPixel],
    context: &ReductionContext<'_>,
) -> Result<RowStats> {
    let ReductionContext {
        weights,
        original_input_count,
        total_weight,
        cancel,
        ..
    } = *context;
    let mut result = RowStats::new(frames.len());
    let lanes = kernel.lanes();
    let mut rgb = PreparedRgb::with_capacity(frames.len());
    let mut tester = Tester::new(frames.len());
    let mut packed = vec![[0.; 3]; REJECTION_BLOCK_PIXELS.min(output.len()) * frames.len()];
    let mut rows = vec![0.; frames.len() * lanes];
    let mut pending = Vec::with_capacity(lanes);
    for (block_index, block) in output.chunks_mut(REJECTION_BLOCK_PIXELS).enumerate() {
        check_cancelled(cancel)?;
        let packed = &mut packed[..block.len() * frames.len()];
        pack_rejection_block(
            frames,
            offset + block_index * REJECTION_BLOCK_PIXELS,
            packed,
        );
        let flush = Flush {
            kernel,
            packed,
            weights,
            original_input_count,
            total_weight,
        };
        for (pixel, stack) in packed.chunks_exact(frames.len()).enumerate() {
            let covered = rgb.prepare(stack, weights);
            let n = rgb.samples[0].len();
            result.min_n = result.min_n.min(n);
            result.max_n = result.max_n.max(n);
            result.min_fraction = result.min_fraction.min(covered / total_weight);
            if n == 0 {
                result.zero += 1;
            }
            // A monochrome stack repeats one plane three times.
            let columns = if rgb.identical_channels() {
                [Some(0..3), None, None]
            } else {
                [Some(0..1), Some(1..2), Some(2..3)]
            };
            for channels in columns.into_iter().flatten() {
                let values = &rgb.samples[channels.start];
                let lane = pending.len();
                for (row, &value) in rows[lane..].iter_mut().step_by(lanes).zip(values) {
                    *row = value;
                }
                pending.push(Column {
                    pixel,
                    channels,
                    count: values.len(),
                });
                if pending.len() == lanes {
                    flush.run(&mut rows, &mut pending, block, &mut tester, &mut result);
                }
            }
        }
        if !pending.is_empty() {
            flush.run(&mut rows, &mut pending, block, &mut tester, &mut result);
        }
    }
    Ok(result)
}

#[cfg(test)]
#[path = "esd_columns_tests.rs"]
mod tests;
