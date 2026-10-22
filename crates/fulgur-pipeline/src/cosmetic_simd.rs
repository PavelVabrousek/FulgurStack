//! Eight independent CFA cosmetics lanes; exact total-order medians and scalar confirmation.
use super::{has_stellar_support, local_noise, median8, neighbors, refined_noise};
use fulgur_core::RawFrame;

pub(super) struct Kernel {
    #[cfg(target_arch = "x86_64")]
    vector: bool,
}
impl Kernel {
    pub fn detect() -> Self {
        Self {
            #[cfg(target_arch = "x86_64")]
            vector: std::is_x86_feature_detected!("avx2"),
        }
    }
    /// Caller has copied the original row and excluded the four-pixel border.
    pub fn row(&self, frame: &RawFrame, y: usize, row: &mut [f32]) -> usize {
        let width = frame.header.info.width;
        let height = frame.header.info.height;
        assert_eq!(width.checked_mul(height), Some(frame.pixels.len()));
        assert_eq!(row.len(), width);
        assert!(width > 8 && y >= 4 && y + 4 < height);
        #[cfg(target_arch = "x86_64")]
        if self.vector {
            // SAFETY: runtime AVX2/OS support, exact backing/row lengths and
            // four-pixel border checked above. vector::row bounds every group.
            return unsafe { vector::row(frame, y, row) };
        }
        scalar_tail(frame, y, row, 4)
    }
}

fn confirm(frame: &RawFrame, x: usize, y: usize, original: f32, median: f32, noise: f32) -> bool {
    let width = frame.header.info.width;
    original > median + 5.0 * noise
        && original > median + 5.0 * noise.max(refined_noise(&frame.pixels, width, x, y))
        && !has_stellar_support(
            &frame.pixels,
            width,
            frame.header.info.height,
            x,
            y,
            original - median,
        )
}

fn scalar_tail(frame: &RawFrame, y: usize, row: &mut [f32], from: usize) -> usize {
    let width = frame.header.info.width;
    let mut count = 0;
    for (offset, destination) in row[from..width - 4].iter_mut().enumerate() {
        let x = from + offset;
        let original = *destination;
        let mut samples = neighbors(&frame.pixels, width, x, y);
        let median = median8(&mut samples);
        if original <= median + 5.0 {
            continue;
        }
        let noise = local_noise(samples, median);
        if confirm(frame, x, y, original, median, noise) {
            *destination = median;
            count += 1;
        }
    }
    count
}

#[cfg(target_arch = "x86_64")]
mod vector {
    use super::*;
    use std::arch::x86_64::*;

    // Sorting keys implement f32::total_cmp exactly, including signed zero
    // and NaN encodings. The bit transform is its own inverse.
    #[target_feature(enable = "avx2")]
    fn keys(bits: __m256i) -> __m256i {
        _mm256_xor_si256(bits, _mm256_srli_epi32(_mm256_srai_epi32(bits, 31), 1))
    }

    #[target_feature(enable = "avx2")]
    pub(super) fn sort(mut v: [__m256i; 8]) -> [__m256i; 8] {
        macro_rules! pair {
            ($a:literal, $b:literal) => {{
                let a = v[$a];
                let b = v[$b];
                v[$a] = _mm256_min_epi32(a, b);
                v[$b] = _mm256_max_epi32(a, b);
            }};
        }
        pair!(0, 1);
        pair!(2, 3);
        pair!(4, 5);
        pair!(6, 7);
        pair!(0, 2);
        pair!(1, 3);
        pair!(4, 6);
        pair!(5, 7);
        pair!(1, 2);
        pair!(5, 6);
        pair!(0, 4);
        pair!(3, 7);
        pair!(1, 5);
        pair!(2, 6);
        pair!(1, 4);
        pair!(3, 6);
        pair!(2, 4);
        pair!(3, 5);
        pair!(3, 4);
        v
    }

    #[target_feature(enable = "avx2")]
    pub(super) fn median(v: [__m256; 8]) -> __m256 {
        let ordered = sort(v.map(|v| keys(_mm256_castps_si256(v))));
        let a = _mm256_castsi256_ps(keys(ordered[3]));
        let b = _mm256_castsi256_ps(keys(ordered[4]));
        _mm256_mul_ps(_mm256_add_ps(a, b), _mm256_set1_ps(0.5))
    }

    #[target_feature(enable = "avx2")]
    pub(super) unsafe fn row(frame: &RawFrame, y: usize, row: &mut [f32]) -> usize {
        let width = frame.header.info.width;
        let mut count = 0;
        let mut x = 4;
        while x + 8 <= width - 4 {
            let above = (y - 2) * width + x;
            let center = y * width + x;
            let below = (y + 2) * width + x;
            // SAFETY: x..x+8 is inside the four-pixel border. Each eight-float
            // neighbor load at +/-2 remains in its validated backing row.
            let samples = [
                above - 2,
                above,
                above + 2,
                center - 2,
                center + 2,
                below - 2,
                below,
                below + 2,
            ]
            .map(|i| unsafe { _mm256_loadu_ps(frame.pixels.as_ptr().add(i)) });
            let median = median(samples);
            // SAFETY: the native load uses the same checked eight-pixel group.
            let original = unsafe { _mm256_loadu_ps(frame.pixels.as_ptr().add(center)) };
            let first = _mm256_cmp_ps(
                original,
                _mm256_add_ps(median, _mm256_set1_ps(5.)),
                _CMP_GT_OQ,
            );
            if _mm256_movemask_ps(first) != 0 {
                let deviations = samples
                    .map(|v| _mm256_andnot_ps(_mm256_set1_ps(-0.), _mm256_sub_ps(v, median)));
                let noise = _mm256_max_ps(
                    _mm256_mul_ps(_mm256_set1_ps(1.4826), self::median(deviations)),
                    _mm256_set1_ps(1.),
                );
                let threshold = _mm256_add_ps(median, _mm256_mul_ps(_mm256_set1_ps(5.), noise));
                let mut mask =
                    _mm256_movemask_ps(_mm256_cmp_ps(original, threshold, _CMP_GT_OQ)) as u32;
                if mask != 0 {
                    let mut medians = [0.; 8];
                    let mut noises = [0.; 8];
                    // SAFETY: both destinations contain exactly eight f32 lanes.
                    unsafe {
                        _mm256_storeu_ps(medians.as_mut_ptr(), median);
                        _mm256_storeu_ps(noises.as_mut_ptr(), noise);
                    }
                    while mask != 0 {
                        let lane = mask.trailing_zeros() as usize;
                        mask &= mask - 1;
                        let p = x + lane;
                        if confirm(frame, p, y, row[p], medians[lane], noises[lane]) {
                            row[p] = medians[lane];
                            count += 1;
                        }
                    }
                }
            }
            x += 8;
        }
        count + scalar_tail(frame, y, row, x)
    }
}

#[cfg(test)]
#[path = "cosmetic_simd_tests.rs"]
mod tests;
