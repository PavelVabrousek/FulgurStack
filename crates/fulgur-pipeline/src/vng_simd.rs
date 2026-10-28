//! Eight same-phase VNG lanes; AVX2 is runtime-selected, scalar stays portable.
//!
//! Each lane repeats the scalar kernel's IEEE operations in the same order.
//! Rows are vectorized only when all five inputs are finite and bounded, so
//! no NaN, infinity or overflow can reach the min/max/compare steps.

/// Larger magnitudes, and every non-finite value, keep the scalar row path.
const VECTOR_LIMIT: f32 = 1.0e30;

pub(super) struct Kernel {
    pub(super) vector: bool,
}
impl Kernel {
    pub fn detect() -> Self {
        #[cfg(target_arch = "x86_64")]
        let vector = std::is_x86_feature_detected!("avx2");
        #[cfg(not(target_arch = "x86_64"))]
        let vector = false;
        Self { vector }
    }
    /// Reconstruct complete groups of eight interior pixel pairs starting at
    /// x = 2. Returns the number of pairs written; the caller finishes the rest.
    pub fn pairs<const LEFT: usize, const RIGHT: usize>(
        &self,
        rows: &[&[f32]; 5],
        row: &mut [[f32; 3]],
    ) -> usize {
        #[cfg(target_arch = "x86_64")]
        if self.vector {
            // SAFETY: only detect() constructs a vector kernel, after AVX2 and
            // OS vector-state detection. Every load is a checked eight-float slice.
            return unsafe { vector::pairs::<LEFT, RIGHT>(rows, row) };
        }
        let _ = (rows, row);
        0
    }
}

/// True when the row may use the vector kernel.
pub(super) fn bounded(row: &[f32]) -> bool {
    row.iter().all(|v| v.abs() <= VECTOR_LIMIT)
}

#[cfg(target_arch = "x86_64")]
mod vector {
    use std::arch::x86_64::*;

    include!(concat!(env!("OUT_DIR"), "/vng_kernels_avx2.rs"));

    /// `weight * |window[a] - window[b]|`, as in the scalar gradient terms.
    #[target_feature(enable = "avx2")]
    #[inline]
    fn difference(window: &[__m256; 25], a: usize, b: usize, weight: u32) -> __m256 {
        let d = _mm256_sub_ps(window[a], window[b]);
        _mm256_mul_ps(
            _mm256_set1_ps(f32::from_bits(weight)),
            _mm256_andnot_ps(_mm256_set1_ps(-0.), d),
        )
    }

    /// Scalar folds: min from +infinity, max from +0, then `min + 0.5 * max`.
    /// Gradients are finite sums of non-negative terms, never NaN or -0.
    #[target_feature(enable = "avx2")]
    #[inline]
    fn start(gradients: &[__m256; 8]) -> (__m256, [__m256; 3], __m256i) {
        let mut min = _mm256_set1_ps(f32::INFINITY);
        let mut max = _mm256_setzero_ps();
        for &g in gradients {
            min = _mm256_min_ps(min, g);
            max = _mm256_max_ps(max, g);
        }
        let threshold = _mm256_add_ps(min, _mm256_mul_ps(_mm256_set1_ps(0.5), max));
        (threshold, [_mm256_setzero_ps(); 3], _mm256_setzero_si256())
    }

    /// `sum += sample * weight` only in lanes whose direction was selected.
    #[target_feature(enable = "avx2")]
    #[inline]
    fn add_if(sum: __m256, sample: __m256, weight: u32, taken: __m256) -> __m256 {
        let added = _mm256_add_ps(
            sum,
            _mm256_mul_ps(sample, _mm256_set1_ps(f32::from_bits(weight))),
        );
        _mm256_blendv_ps(sum, added, taken)
    }

    /// `center + (sums[c] - sums[native]) / count`; the native channel is the center.
    #[target_feature(enable = "avx2")]
    #[inline]
    fn finish(
        window: &[__m256; 25],
        sums: [__m256; 3],
        count: __m256i,
        native: usize,
    ) -> [__m256; 3] {
        let count = _mm256_cvtepi32_ps(count);
        let mut pixel = std::array::from_fn(|c| {
            _mm256_add_ps(
                window[12],
                _mm256_div_ps(_mm256_sub_ps(sums[c], sums[native]), count),
            )
        });
        pixel[native] = window[12];
        pixel
    }

    #[target_feature(enable = "avx2")]
    #[inline]
    fn load(plane: &[f32], start: usize) -> __m256 {
        let lanes = &plane[start..start + 8];
        // SAFETY: the checked slice holds exactly eight contiguous f32 values.
        unsafe { _mm256_loadu_ps(lanes.as_ptr()) }
    }

    /// Lane l of a LEFT window is pixel x = 2 + 2(k + l); RIGHT is x + 1.
    /// Window column c holds source x - 2 + c, read from the even/odd plane.
    #[target_feature(enable = "avx2")]
    fn window(planes: &[[Vec<f32>; 2]; 5], k: usize, right: bool) -> [__m256; 25] {
        std::array::from_fn(|i| {
            let (j, c) = (i / 5, i % 5);
            let source = c + usize::from(right);
            load(&planes[j][source % 2], k + source / 2)
        })
    }

    #[target_feature(enable = "avx2")]
    fn store(row: &mut [[f32; 3]], first: usize, pixel: [__m256; 3]) {
        let mut channels = [[0.; 8]; 3];
        for (lanes, value) in channels.iter_mut().zip(pixel) {
            // SAFETY: each destination contains exactly eight f32 lanes.
            unsafe { _mm256_storeu_ps(lanes.as_mut_ptr(), value) };
        }
        for lane in 0..8 {
            row[first + 2 * lane] = [channels[0][lane], channels[1][lane], channels[2][lane]];
        }
    }

    /// # Safety
    /// The CPU must support AVX2; `Kernel::detect()` establishes this.
    #[target_feature(enable = "avx2")]
    pub(super) unsafe fn pairs<const LEFT: usize, const RIGHT: usize>(
        rows: &[&[f32]; 5],
        row: &mut [[f32; 3]],
    ) -> usize {
        let width = row.len();
        let pairs = width.saturating_sub(4) / 2;
        let groups = pairs / 8;
        if groups == 0 {
            return 0;
        }
        // Even/odd source columns 0..=2 * pairs + 3, all inside each row.
        let planes: [[Vec<f32>; 2]; 5] = std::array::from_fn(|j| {
            std::array::from_fn(|parity| (0..pairs + 2).map(|m| rows[j][2 * m + parity]).collect())
        });
        for group in 0..groups {
            let k = group * 8;
            let left = reconstruct::<LEFT>(&window(&planes, k, false));
            store(row, 2 + 2 * k, left);
            let right = reconstruct::<RIGHT>(&window(&planes, k, true));
            store(row, 3 + 2 * k, right);
        }
        groups * 8
    }
}
