//! Generalized ESD over one reduction band, with the outcome of
//! `rejection_methods::esd` (ADR-111). Sample columns of neighbouring pixels
//! are sorted together by a data-oblivious network, each sorted column is
//! tested, and the survivors are summed in frame order by their value range.

use super::rejection_methods::{self, MIN_SAMPLES, MethodScratch, esd_limits};
use super::rgb_rejection::PreparedRgb;
use super::{IntegratedPixel, REJECTION_BLOCK_PIXELS, RowStats, Sum, pack_rejection_block};
use fulgur_core::{Result, check_cancelled};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

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
        }
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

    /// Value, rejected low and high, and surviving weight of `self.sorted`,
    /// the sorted finite samples of `channel` in `stack`.
    fn combine(
        &mut self,
        stack: &[[f32; 3]],
        channel: usize,
        weights: &[f64],
    ) -> (f32, u64, u64, f64) {
        let n = self.sorted.len();
        if n == 0 {
            return (0., 0, 0, 0.);
        }
        let (first, last) = if n >= MIN_SAMPLES {
            let lambda = self.limits[n].get_or_insert_with(|| esd_limits(n));
            survivor_window(&self.sorted, lambda, &self.inverse)
        } else {
            (0, n - 1)
        };
        let sorted = &self.sorted;
        if (first > 0 && sorted[first - 1] == sorted[first])
            || (last + 1 < n && sorted[last + 1] == sorted[last])
        {
            return self.combine_tied(stack, channel, weights);
        }
        // Non-finite samples fall outside this finite range.
        let (floor, ceiling) = (sorted[first], sorted[last]);
        let mut numerator = Sum::default();
        let mut denominator = Sum::default();
        for (sample, &weight) in stack.iter().zip(weights) {
            let v = sample[channel];
            if v >= floor && v <= ceiling {
                numerator.add(f64::from(v) * weight);
                denominator.add(weight);
            }
        }
        let (low, high) = (first as u64, (n - 1 - last) as u64);
        let weight = denominator.value();
        if weight == 0. {
            (0., low, high, 0.)
        } else {
            ((numerator.value() / weight) as f32, low, high, weight)
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
            let (value, low, high, surviving) =
                tester.combine(stack, column.channels.start, self.weights);
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
}

/// `integrate::reduce_band` for the ESD method.
pub(super) fn reduce_band(
    frames: &[Vec<[f32; 3]>],
    weights: &[f64],
    offset: usize,
    output: &mut [IntegratedPixel],
    original_input_count: usize,
    total_weight: f64,
    cancel: &AtomicBool,
) -> Result<RowStats> {
    reduce_band_with(
        Kernel::detect(),
        frames,
        weights,
        offset,
        output,
        original_input_count,
        total_weight,
        cancel,
    )
}

#[allow(clippy::too_many_arguments)]
fn reduce_band_with(
    kernel: Kernel,
    frames: &[Vec<[f32; 3]>],
    weights: &[f64],
    offset: usize,
    output: &mut [IntegratedPixel],
    original_input_count: usize,
    total_weight: f64,
    cancel: &AtomicBool,
) -> Result<RowStats> {
    let mut result = RowStats {
        min_n: frames.len(),
        min_fraction: 1.,
        min_surviving: 1.,
        ..Default::default()
    };
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
