//! Exact four-pixel CPU resampling; AVX2 is runtime-selected, scalar stays portable.
use crate::integrate::sample;
use fulgur_core::{Error, Result, RgbImage};

#[derive(Clone, Copy)]
pub(super) struct View<'a> {
    pub rgb: &'a RgbImage,
    pub roi: [usize; 4],
    pub sensor: [usize; 2],
}
#[derive(Clone, Copy)]
pub(super) struct RowTransform {
    pub sy: f64,
    pub cy: f64,
    pub inverse_scale: f64,
    pub tx: f64,
    pub ty: f64,
    pub scale: f64,
    pub offset: f64,
}

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
    pub fn row(
        &self,
        view: View<'_>,
        t: RowTransform,
        columns: &[[f64; 2]],
        row: &mut [[f32; 3]],
    ) -> Result<()> {
        if columns.len() != row.len() {
            return Err(Error::Invalid("Mismatched resampling row lengths."));
        }
        #[cfg(target_arch = "x86_64")]
        if self.vector {
            // SAFETY: only detect() constructs a vector kernel, after AVX2 and
            // OS vector-state detection. Every source load footprint is checked below.
            return unsafe { vector::row(view, t, columns, row) };
        }
        scalar_row(view, t, columns, row)
    }
}
fn scalar_row(
    view: View<'_>,
    t: RowTransform,
    columns: &[[f64; 2]],
    row: &mut [[f32; 3]],
) -> Result<()> {
    for (pixel, &[cx, sx]) in row.iter_mut().zip(columns) {
        let p = [
            t.inverse_scale * (cx - t.sy) + t.tx,
            t.inverse_scale * (sx + t.cy) + t.ty,
        ];
        if let Some(v) = sample(view.rgb, view.roi, view.sensor, p) {
            let normalized = v.map(|v| (t.scale * v as f64 + t.offset) as f32);
            if normalized.iter().any(|v| !v.is_finite()) {
                return Err(Error::Invalid("Nonfinite normalized integration sample."));
            }
            *pixel = normalized;
        }
    }
    Ok(())
}

#[cfg(target_arch = "x86_64")]
mod vector {
    use super::*;
    use std::arch::x86_64::*;

    // Same strict sensor/ROI mask as the scalar sampler, plus an explicit
    // whole-footprint backing-buffer check for the unsafe load boundary.
    fn footprint(view: View<'_>, p: [f64; 2]) -> Option<usize> {
        if p[0] < 1.
            || p[1] < 1.
            || p[0] >= (view.sensor[0] - 2) as f64
            || p[1] >= (view.sensor[1] - 2) as f64
        {
            return None;
        }
        let x = p[0] as usize;
        let y = p[1] as usize;
        // Preserve integer-to-f64 weight coordinates even for extreme global
        // ROI origins; ordinary FITS sources are far below this exactness limit.
        if x > (1_usize << 53) - 4 || y > (1_usize << 53) - 4 {
            return None;
        }
        let [rx, ry, rw, rh] = view.roi;
        if x < rx + 1 || y < ry + 1 || x + 2 >= rx + rw || y + 2 >= ry + rh {
            return None;
        }
        let first = (y - 1 - ry) * view.rgb.width + x - 1 - rx;
        let last = first
            .checked_add(3_usize.checked_mul(view.rgb.width)?)?
            .checked_add(3)?;
        (last < view.rgb.pixels.len()).then_some(first)
    }

    #[target_feature(enable = "avx2")]
    unsafe fn load_rgb(rgb: &RgbImage, index: usize) -> __m128 {
        if index + 1 < rgb.pixels.len() {
            // SAFETY: caller checked index within a full footprint; the next
            // pixel exists, so all four contiguous floats belong to the Vec.
            unsafe { _mm_loadu_ps(rgb.pixels.as_ptr().add(index).cast::<f32>()) }
        } else {
            let v = rgb.pixels[index];
            _mm_set_ps(0., v[2], v[1], v[0])
        }
    }

    #[target_feature(enable = "avx2")]
    pub(super) unsafe fn weights(p: __m256d, base: __m256d) -> [__m128; 4] {
        let one = _mm256_set1_pd(1.);
        let mut result = [_mm_setzero_ps(); 4];
        for (i, out) in result.iter_mut().enumerate() {
            let d = _mm256_andnot_pd(
                _mm256_set1_pd(-0.),
                _mm256_add_pd(
                    _mm256_sub_pd(p, _mm256_add_pd(base, _mm256_set1_pd(i as f64))),
                    one,
                ),
            );
            let value = if i == 1 || i == 2 {
                // ((1.5*d - 2.5)*d*d + 1), with no fused operations.
                _mm256_add_pd(
                    _mm256_mul_pd(
                        _mm256_mul_pd(
                            _mm256_sub_pd(
                                _mm256_mul_pd(_mm256_set1_pd(1.5), d),
                                _mm256_set1_pd(2.5),
                            ),
                            d,
                        ),
                        d,
                    ),
                    one,
                )
            } else {
                // ((-0.5*d + 2.5)*d*d - 4*d + 2).
                _mm256_add_pd(
                    _mm256_sub_pd(
                        _mm256_mul_pd(
                            _mm256_mul_pd(
                                _mm256_add_pd(
                                    _mm256_mul_pd(_mm256_set1_pd(-0.5), d),
                                    _mm256_set1_pd(2.5),
                                ),
                                d,
                            ),
                            d,
                        ),
                        _mm256_mul_pd(_mm256_set1_pd(4.), d),
                    ),
                    _mm256_set1_pd(2.),
                )
            };
            *out = _mm256_cvtpd_ps(value);
        }
        result
    }

    #[target_feature(enable = "avx2")]
    pub(super) unsafe fn row(
        view: View<'_>,
        t: RowTransform,
        columns: &[[f64; 2]],
        row: &mut [[f32; 3]],
    ) -> Result<()> {
        let full = columns.len() / 4 * 4;
        for start in (0..full).step_by(4) {
            let c = &columns[start..start + 4];
            let cx = _mm256_set_pd(c[3][0], c[2][0], c[1][0], c[0][0]);
            let sx = _mm256_set_pd(c[3][1], c[2][1], c[1][1], c[0][1]);
            let px = _mm256_add_pd(
                _mm256_mul_pd(
                    _mm256_set1_pd(t.inverse_scale),
                    _mm256_sub_pd(cx, _mm256_set1_pd(t.sy)),
                ),
                _mm256_set1_pd(t.tx),
            );
            let py = _mm256_add_pd(
                _mm256_mul_pd(
                    _mm256_set1_pd(t.inverse_scale),
                    _mm256_add_pd(sx, _mm256_set1_pd(t.cy)),
                ),
                _mm256_set1_pd(t.ty),
            );
            let mut xs = [0.; 4];
            let mut ys = [0.; 4];
            // SAFETY: unaligned stores each write four f64 values into four-element arrays.
            unsafe {
                _mm256_storeu_pd(xs.as_mut_ptr(), px);
                _mm256_storeu_pd(ys.as_mut_ptr(), py);
            }
            let first: [Option<usize>; 4] =
                std::array::from_fn(|i| footprint(view, [xs[i], ys[i]]));
            if first.iter().any(Option::is_none) {
                scalar_row(view, t, c, &mut row[start..start + 4])?;
                continue;
            }
            let base_x = _mm256_set_pd(
                xs[3] as usize as f64,
                xs[2] as usize as f64,
                xs[1] as usize as f64,
                xs[0] as usize as f64,
            );
            let base_y = _mm256_set_pd(
                ys[3] as usize as f64,
                ys[2] as usize as f64,
                ys[1] as usize as f64,
                ys[0] as usize as f64,
            );
            // SAFETY: this function has AVX2 enabled, as required by weights().
            let (wx, wy) = unsafe { (weights(px, base_x), weights(py, base_y)) };
            let first = first.map(Option::unwrap);
            let mut accum = [_mm256_setzero_ps(); 2];
            for (j, yy) in wy.iter().enumerate() {
                for (i, xx) in wx.iter().enumerate() {
                    let w = _mm_mul_ps(*xx, *yy);
                    // Two RGB triplets plus ignored padding lanes per register.
                    let weights = [
                        _mm256_set_m128(_mm_shuffle_ps::<0x55>(w, w), _mm_shuffle_ps::<0x00>(w, w)),
                        _mm256_set_m128(_mm_shuffle_ps::<0xff>(w, w), _mm_shuffle_ps::<0xaa>(w, w)),
                    ];
                    let displacement = j * view.rgb.width + i;
                    for (pair, sum) in accum.iter_mut().enumerate() {
                        // SAFETY: every full 4x4 footprint was validated. The
                        // loader checks whether a fourth float is available and
                        // uses exact three-value assembly for the last triplet.
                        let values = unsafe {
                            _mm256_set_m128(
                                load_rgb(view.rgb, first[2 * pair + 1] + displacement),
                                load_rgb(view.rgb, first[2 * pair] + displacement),
                            )
                        };
                        // Preserve sixteen independent row-major f32 additions.
                        *sum = _mm256_add_ps(*sum, _mm256_mul_ps(weights[pair], values));
                    }
                }
            }
            let pixels = [
                _mm256_castps256_ps128(accum[0]),
                _mm256_extractf128_ps::<1>(accum[0]),
                _mm256_castps256_ps128(accum[1]),
                _mm256_extractf128_ps::<1>(accum[1]),
            ];
            for (lane, pixel) in pixels.into_iter().enumerate() {
                let value = _mm256_add_pd(
                    _mm256_mul_pd(_mm256_set1_pd(t.scale), _mm256_cvtps_pd(pixel)),
                    _mm256_set1_pd(t.offset),
                );
                let mut normalized = [0.; 4];
                // SAFETY: write exactly four floats; only the first RGB triplet
                // is scientific data. The fourth independent lane is ignored.
                unsafe {
                    _mm_storeu_ps(normalized.as_mut_ptr(), _mm256_cvtpd_ps(value));
                }
                let pixel = [normalized[0], normalized[1], normalized[2]];
                if pixel.iter().any(|v| !v.is_finite()) {
                    return Err(Error::Invalid("Nonfinite normalized integration sample."));
                }
                row[start + lane] = pixel;
            }
        }
        scalar_row(view, t, &columns[full..], &mut row[full..])
    }
}

#[cfg(test)]
#[path = "resample_tests.rs"]
mod tests;
