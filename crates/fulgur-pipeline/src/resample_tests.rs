use super::*;
use crate::register::Transform;
use fulgur_core::DemosaicMethod;

fn require_vector() -> bool {
    #[cfg(target_arch = "x86_64")]
    let available = Kernel::detect().vector;
    #[cfg(not(target_arch = "x86_64"))]
    let available = false;
    if std::env::var("FULGURSTACK_TEST_SIMD").as_deref() == Ok("1") {
        assert!(
            available,
            "Explicitly required local AVX2 resampling is unavailable."
        );
    }
    available
}

#[test]
fn vector_rows_match_scalar_bits_masks_and_immutable_sources() {
    require_vector();
    let mut valid = 0;
    let mut invalid = 0;
    for field in 0..5 {
        let pixels: Vec<[f32; 3]> = (0..67 * 69)
            .map(|i| match field {
                0 => [1500.125, 1000.25, 700.5],
                1 => {
                    let v = ((i * 7919) % 65536) as f32;
                    [v - 0.03125, 65535. - v, (i % 37) as f32 * 0.00003125]
                }
                2 => {
                    let v = f32::from_bits((i % 31 + 1) as u32);
                    [v, -v, v * 2.]
                }
                3 => {
                    if i % 3 == 0 {
                        [65535., 40000., 60000.]
                    } else {
                        [0.; 3]
                    }
                }
                _ => [i as f32 * 13.0625 - 10000., -(i as f32) * 7.125, 0.03125],
            })
            .collect();
        for roi in [[0, 0, 67, 69], [6, 8, 51, 53]] {
            let rgb = RgbImage {
                width: roi[2],
                height: roi[3],
                demosaic_method: DemosaicMethod::Vng,
                pixels: (roi[1]..roi[1] + roi[3])
                    .flat_map(|y| {
                        pixels[y * 67 + roi[0]..y * 67 + roi[0] + roi[2]]
                            .iter()
                            .copied()
                    })
                    .collect(),
            };
            let original = rgb
                .pixels
                .iter()
                .map(|v| v.map(f32::to_bits))
                .collect::<Vec<_>>();
            let view = View {
                rgb: &rgb,
                roi,
                sensor: [67, 69],
            };
            for scale in [0.30, 0.33, 0.5, 1., 2., 3., 1. / 0.30] {
                for angle in [0_f64, 0.07, 15., 90., 179.5] {
                    let inverse = Transform {
                        scale_factor: scale,
                        theta_rad: angle.to_radians(),
                        tx_px: 7.3125,
                        ty_px: -8.125,
                    }
                    .inverse();
                    let (s, c) = inverse.theta_rad.sin_cos();
                    for origin in [[-19, -17], [0, 0], [41, 45]] {
                        for y in [0, 1, 19, 52] {
                            let qy = (origin[1] + y) as f64;
                            let t = RowTransform {
                                sy: s * qy,
                                cy: c * qy,
                                inverse_scale: inverse.scale_factor,
                                tx: inverse.tx_px,
                                ty: inverse.ty_px,
                                scale: if field == 2 { 1. } else { 0.73123456789 },
                                offset: if field == 2 { 0. } else { -0.123456789 },
                            };
                            for width in [0, 1, 2, 3, 4, 5, 7, 8, 9, 31, 33, 79] {
                                let columns: Vec<_> = (0..width)
                                    .map(|x| {
                                        let qx = (origin[0] + x as i32) as f64;
                                        [c * qx, s * qx]
                                    })
                                    .collect();
                                let mut expected = vec![[f32::NAN; 3]; width];
                                let mut actual = expected.clone();
                                scalar_row(view, t, &columns, &mut expected).unwrap();
                                Kernel::detect()
                                    .row(view, t, &columns, &mut actual)
                                    .unwrap();
                                for (a, b) in actual.iter().zip(expected) {
                                    if b.iter().all(|v| v.is_nan()) {
                                        assert!(a.iter().all(|v| v.is_nan()));
                                        invalid += 1;
                                    } else {
                                        assert_eq!(
                                            a.map(f32::to_bits),
                                            b.map(f32::to_bits),
                                            "field={field}, scale={scale}, angle={angle}, origin={origin:?}, y={y}, width={width}"
                                        );
                                        valid += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            assert_eq!(
                original,
                rgb.pixels
                    .iter()
                    .map(|v| v.map(f32::to_bits))
                    .collect::<Vec<_>>()
            );
        }
    }
    assert!(valid > 10000 && invalid > 10000);
}

#[test]
fn vector_keys_match_scalar_at_adjacent_representable_boundaries() {
    if !require_vector() {
        return;
    }
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::*;
        for base in [1_usize, 2, 3, 17, 127, 2157, 3837, 65535] {
            let low = base as f64;
            let high = (base + 1) as f64;
            let positions: Vec<_> = std::iter::once(low)
                .chain(std::iter::once(f64::from_bits(low.to_bits() + 1)))
                .chain(std::iter::once(f64::from_bits(high.to_bits() - 1)))
                .chain((1..1024).map(|i| low + i as f64 / 1024.))
                .collect();
            for chunk in positions.chunks(4) {
                let p: [f64; 4] = std::array::from_fn(|i| chunk[i % chunk.len()]);
                // SAFETY: require_vector confirmed local AVX2; each unaligned
                // store writes exactly four floats into a four-element array.
                let actual = unsafe {
                    let q = _mm256_set_pd(p[3], p[2], p[1], p[0]);
                    let weights = vector::weights(q, _mm256_set1_pd(low));
                    weights.map(|v| {
                        let mut a = [0.; 4];
                        _mm_storeu_ps(a.as_mut_ptr(), v);
                        a
                    })
                };
                for (i, weight) in actual.iter().enumerate() {
                    for (lane, p) in p.iter().enumerate() {
                        let d = (p - (base + i) as f64 + 1.).abs();
                        let scalar = if i == 1 || i == 2 {
                            ((1.5 * d - 2.5) * d * d + 1.) as f32
                        } else {
                            ((-0.5 * d + 2.5) * d * d - 4. * d + 2.) as f32
                        };
                        assert_eq!(weight[lane].to_bits(), scalar.to_bits());
                    }
                }
            }
        }
    }
}

#[test]
fn vector_gathers_last_backing_pixel_and_rejects_nonfinite_results() {
    require_vector();
    let mut rgb = RgbImage {
        width: 18,
        height: 19,
        demosaic_method: DemosaicMethod::Vng,
        pixels: (0..18 * 19)
            .map(|i| [i as f32, i as f32 * 0.125, -(i as f32)])
            .collect(),
    };
    let t = RowTransform {
        sy: 0.,
        cy: 0.,
        inverse_scale: 1.,
        tx: 0.,
        ty: 20.75,
        scale: 0.73123456789,
        offset: -0.123456789,
    };
    let columns = [[17.75, 0.]; 4];
    let roi = [2, 4, 18, 19];
    let sensor = [30, 40];
    let mut expected = [[f32::NAN; 3]; 4];
    let mut actual = expected;
    let view = View {
        rgb: &rgb,
        roi,
        sensor,
    };
    scalar_row(view, t, &columns, &mut expected).unwrap();
    Kernel::detect()
        .row(view, t, &columns, &mut actual)
        .unwrap();
    assert_eq!(
        actual.map(|v| v.map(f32::to_bits)),
        expected.map(|v| v.map(f32::to_bits))
    );
    assert!(actual.iter().flatten().all(|v| v.is_finite()));
    assert!(
        Kernel::detect()
            .row(view, t, &columns[..3], &mut actual)
            .is_err()
    );
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        rgb.pixels[18 * 19 - 1][2] = bad;
        let view = View {
            rgb: &rgb,
            roi,
            sensor,
        };
        assert!(matches!(
            scalar_row(view, t, &columns, &mut expected),
            Err(Error::Invalid("Nonfinite normalized integration sample."))
        ));
        assert!(matches!(
            Kernel::detect().row(view, t, &columns, &mut actual),
            Err(Error::Invalid("Nonfinite normalized integration sample."))
        ));
    }
}
