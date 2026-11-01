//! Four inverse solves at once. Each lane repeats the IEEE operations of
//! `Warp::inverse_with_rotation` in the same order, so results stay identical.
use super::Warp;
use std::arch::x86_64::*;

#[target_feature(enable = "avx2")]
#[inline]
fn set(value: f64) -> __m256d {
    _mm256_set1_pd(value)
}

#[target_feature(enable = "avx2")]
#[inline]
fn scaled(coefficient: f64, value: __m256d) -> __m256d {
    _mm256_mul_pd(set(coefficient), value)
}

/// Lanes where |value| <= limit; NaN and infinity are outside any finite limit.
#[target_feature(enable = "avx2")]
#[inline]
fn within(value: __m256d, limit: f64) -> i32 {
    let magnitude = _mm256_andnot_pd(set(-0.), value);
    _mm256_movemask_pd(_mm256_cmp_pd::<_CMP_LE_OQ>(magnitude, set(limit)))
}

/// Writes `out[i]` for target pixels (first_x + i, y) in whole groups of four
/// and returns how many it wrote; a pixel without a solution gets `outside`.
///
/// # Safety
/// The CPU must support AVX2.
#[target_feature(enable = "avx2")]
pub(super) unsafe fn row(
    model: &Warp,
    first_x: i32,
    y: f64,
    (s, c): (f64, f64),
    outside: [f64; 2],
    out: &mut [[f64; 2]],
) -> usize {
    let base = model.base;
    let scale = base.scale_factor;
    let inverse_scale = 1. / scale;
    let (tx, ty) = (base.tx_px, base.ty_px);
    let start = [(-c * tx - s * ty) / scale, (s * tx - c * ty) / scale];
    let [a, b] = &model.coefficients;
    let center = [set(model.center[0]), set(model.center[1])];
    let norm = [set(model.normalization[0]), set(model.normalization[1])];
    let q1 = set(y);
    let groups = out.len() / 4;
    for (group, lanes) in out.chunks_exact_mut(4).enumerate() {
        let x = first_x + (group * 4) as i32;
        let q0 = _mm256_set_pd((x + 3) as f64, (x + 2) as f64, (x + 1) as f64, x as f64);
        let mut p0 = _mm256_add_pd(
            scaled(inverse_scale, _mm256_add_pd(scaled(c, q0), scaled(s, q1))),
            set(start[0]),
        );
        let mut p1 = _mm256_add_pd(
            scaled(inverse_scale, _mm256_add_pd(scaled(-s, q0), scaled(c, q1))),
            set(start[1]),
        );
        lanes.fill(outside);
        let mut active = 0b1111;
        for _ in 0..8 {
            let nx = _mm256_div_pd(_mm256_sub_pd(p0, center[0]), norm[0]);
            let ny = _mm256_div_pd(_mm256_sub_pd(p1, center[1]), norm[1]);
            active &= within(nx, 2.) & within(ny, 2.);
            if active == 0 {
                break;
            }
            let xy = _mm256_mul_pd(nx, ny);
            let xx = _mm256_mul_pd(nx, nx);
            let yy = _mm256_mul_pd(ny, ny);
            let xxx = _mm256_mul_pd(xx, nx);
            let xxy = _mm256_mul_pd(xx, ny);
            let xyy = _mm256_mul_pd(xy, ny);
            let yyy = _mm256_mul_pd(yy, ny);
            let residual = |k: &[f64; 10]| {
                let mut sum = _mm256_add_pd(set(k[0]), scaled(k[1], nx));
                for (coefficient, term) in k[2..].iter().zip([ny, xy, xx, yy, xxx, xxy, xyy, yyy]) {
                    sum = _mm256_add_pd(sum, scaled(*coefficient, term));
                }
                sum
            };
            let f0 = _mm256_add_pd(
                _mm256_add_pd(
                    scaled(scale, _mm256_sub_pd(scaled(c, p0), scaled(s, p1))),
                    set(tx),
                ),
                residual(a),
            );
            let f1 = _mm256_add_pd(
                _mm256_add_pd(
                    scaled(scale, _mm256_add_pd(scaled(s, p0), scaled(c, p1))),
                    set(ty),
                ),
                residual(b),
            );
            let e0 = _mm256_sub_pd(f0, q0);
            let e1 = _mm256_sub_pd(f1, q1);
            let squared = _mm256_add_pd(_mm256_mul_pd(e0, e0), _mm256_mul_pd(e1, e1));
            let done =
                active & _mm256_movemask_pd(_mm256_cmp_pd::<_CMP_LT_OQ>(squared, set(1e-14)));
            if done != 0 {
                let inside = within(nx, 1. + 1e-9) & within(ny, 1. + 1e-9);
                let mut xs = [0.; 4];
                let mut ys = [0.; 4];
                // SAFETY: both destinations hold exactly four f64 lanes.
                unsafe {
                    _mm256_storeu_pd(xs.as_mut_ptr(), p0);
                    _mm256_storeu_pd(ys.as_mut_ptr(), p1);
                }
                for (lane, pixel) in lanes.iter_mut().enumerate() {
                    if ((done & inside) >> lane) & 1 == 1 {
                        *pixel = [xs[lane], ys[lane]];
                    }
                }
                active &= !done;
                if active == 0 {
                    break;
                }
            }
            let two_x = scaled(2., nx);
            let two_y = scaled(2., ny);
            let two_xy = _mm256_mul_pd(two_x, ny);
            let three_xx = _mm256_mul_pd(scaled(3., nx), nx);
            let three_yy = _mm256_mul_pd(scaled(3., ny), ny);
            let along_x = |k: &[f64; 10]| {
                let mut sum = _mm256_add_pd(set(k[1]), scaled(k[3], ny));
                sum = _mm256_add_pd(sum, scaled(k[4], two_x));
                sum = _mm256_add_pd(sum, scaled(k[6], three_xx));
                sum = _mm256_add_pd(sum, scaled(k[7], two_xy));
                _mm256_add_pd(sum, scaled(k[8], yy))
            };
            let along_y = |k: &[f64; 10]| {
                let mut sum = _mm256_add_pd(set(k[2]), scaled(k[3], nx));
                sum = _mm256_add_pd(sum, scaled(k[5], two_y));
                sum = _mm256_add_pd(sum, scaled(k[7], xx));
                sum = _mm256_add_pd(sum, scaled(k[8], two_xy));
                _mm256_add_pd(sum, scaled(k[9], three_yy))
            };
            let j00 = _mm256_add_pd(set(scale * c), _mm256_div_pd(along_x(a), norm[0]));
            let j01 = _mm256_add_pd(set(-scale * s), _mm256_div_pd(along_y(a), norm[1]));
            let j10 = _mm256_add_pd(set(scale * s), _mm256_div_pd(along_x(b), norm[0]));
            let j11 = _mm256_add_pd(set(scale * c), _mm256_div_pd(along_y(b), norm[1]));
            let det = _mm256_sub_pd(_mm256_mul_pd(j00, j11), _mm256_mul_pd(j01, j10));
            active &= _mm256_movemask_pd(_mm256_cmp_pd::<_CMP_GT_OQ>(det, set(1e-8)))
                & within(det, f64::MAX);
            if active == 0 {
                break;
            }
            p0 = _mm256_sub_pd(
                p0,
                _mm256_div_pd(
                    _mm256_sub_pd(_mm256_mul_pd(j11, e0), _mm256_mul_pd(j01, e1)),
                    det,
                ),
            );
            p1 = _mm256_sub_pd(
                p1,
                _mm256_div_pd(
                    _mm256_add_pd(
                        _mm256_mul_pd(_mm256_sub_pd(set(-0.), j10), e0),
                        _mm256_mul_pd(j00, e1),
                    ),
                    det,
                ),
            );
        }
    }
    groups * 4
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Transform;

    fn model(seed: u64, amplitude: f64) -> Warp {
        let mut state = seed;
        let mut next = move || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 11) as f64 / (1u64 << 53) as f64 - 0.5
        };
        let base = Transform {
            scale_factor: 1. + 0.5 * next(),
            theta_rad: 7. * next(),
            tx_px: 300. * next(),
            ty_px: 300. * next(),
        };
        Warp {
            base,
            center: [319.5, 239.5],
            normalization: [319.5, 239.5],
            coefficients: std::array::from_fn(|_| std::array::from_fn(|_| amplitude * next())),
        }
    }

    #[test]
    fn lanes_match_the_scalar_solver_bit_for_bit() {
        let available = std::is_x86_feature_detected!("avx2");
        if std::env::var("FULGURSTACK_TEST_SIMD").as_deref() == Ok("1") {
            assert!(
                available,
                "Explicitly required local AVX2 inverse is unavailable."
            );
        }
        if !available {
            return;
        }
        let outside = [f64::NEG_INFINITY; 2];
        let mut solved = 0;
        let mut refused = 0;
        for (seed, amplitude) in [(1, 0.), (2, 4.), (3, 20.), (4, 100.), (5, 600.), (6, 3000.)] {
            let warp = model(seed, amplitude);
            let rotation = warp.base.theta_rad.sin_cos();
            for y in (-200..700).step_by(37) {
                let mut row = vec![[0.; 2]; 803];
                // SAFETY: AVX2 was detected above.
                let done =
                    unsafe { super::row(&warp, -100, y as f64, rotation, outside, &mut row) };
                assert_eq!(done, 800);
                for (i, p) in row[..done].iter().enumerate() {
                    let q = [(i as i32 - 100) as f64, y as f64];
                    let expected = warp.inverse_with_rotation(q, rotation);
                    assert_eq!(
                        p.map(f64::to_bits),
                        expected.unwrap_or(outside).map(f64::to_bits),
                        "seed {seed}, {q:?}"
                    );
                    if expected.is_some() {
                        solved += 1;
                    } else {
                        refused += 1;
                    }
                }
                let mut whole = vec![[0.; 2]; 803];
                warp.inverse_row(-100, y as f64, rotation, outside, &mut whole);
                assert_eq!(whole[..done], row[..done]);
                let q = [702., y as f64];
                let tail = warp.inverse_with_rotation(q, rotation).unwrap_or(outside);
                assert_eq!(whole[802].map(f64::to_bits), tail.map(f64::to_bits));
            }
        }
        assert!(solved > 10_000 && refused > 10_000, "{solved} {refused}");
    }
}
