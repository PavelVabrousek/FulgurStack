use super::*;

fn prior_median(values: &mut [f32]) -> f64 {
    let n = values.len();
    let mid = n / 2;
    let (lo, m, _) = values.select_nth_unstable_by(mid, f32::total_cmp);
    if n % 2 == 0 {
        (f64::from(*m) + f64::from(lo.iter().copied().fold(f32::NEG_INFINITY, f32::max))) * 0.5
    } else {
        f64::from(*m)
    }
}
fn prior_combine(
    values: &[(f32, usize)],
    weights: &[f64],
    scratch: &mut ClippingScratch,
    config: &IntegrationConfig,
) -> (f32, u64, u64, f64) {
    if values.is_empty() {
        return (0., 0, 0, 0.);
    }
    let mut low = f64::NEG_INFINITY;
    let mut high = f64::INFINITY;
    if values.len() >= 5 {
        let ClippingScratch {
            winsorized,
            deviations,
            ..
        } = scratch;
        winsorized.clear();
        winsorized.extend(values.iter().map(|v| v.0));
        for _ in 0..3 {
            let center = prior_median(winsorized);
            deviations.clear();
            deviations.extend(winsorized.iter().map(|v| (*v as f64 - center).abs() as f32));
            let sigma = (1.4826 * prior_median(deviations)).max(1e-6);
            low = center - config.sigma_low * sigma;
            high = center + config.sigma_high * sigma;
            let mut changed = false;
            for x in winsorized.iter_mut() {
                let next = (*x as f64).clamp(low, high) as f32;
                changed |= next != *x;
                *x = next;
            }
            if !changed {
                break;
            }
        }
    }
    let mut numerator = Sum::default();
    let mut denominator = Sum::default();
    let mut rejected_low = 0;
    let mut rejected_high = 0;
    for &(v, index) in values {
        if (v as f64) < low {
            rejected_low += 1;
        } else if (v as f64) > high {
            rejected_high += 1;
        } else {
            numerator.add(v as f64 * weights[index]);
            denominator.add(weights[index]);
        }
    }
    let weight = denominator.value();
    if weight == 0. {
        (0., rejected_low, rejected_high, 0.)
    } else {
        (
            (numerator.value() / weight) as f32,
            rejected_low,
            rejected_high,
            weight,
        )
    }
}

#[test]
fn certified_clipping_matches_frozen_three_iteration_reference() {
    let mut seed = 0x123456789abcdef_u64;
    let weights: Vec<_> = (0..1500)
        .map(|i| 0.001 + (i * 37 % 100) as f64 / 101.)
        .collect();
    let mut actual_scratch = ClippingScratch::with_capacity(1500);
    let mut prior_scratch = ClippingScratch::with_capacity(1500);
    for round in 0..96 {
        for n in [0, 1, 2, 4, 5, 6, 7, 8, 27, 32, 33, 940, 1140, 1500] {
            let values: Vec<_> = (0..n)
                .map(|i| {
                    seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                    let bits = (seed >> 32) as u32;
                    let v = match round % 8 {
                        0 => f32::from_bits(bits & 0xfeffffff),
                        1 => f32::from_bits(bits & 0x807fffff),
                        2 => {
                            if i % 2 == 0 {
                                -0.
                            } else {
                                0.
                            }
                        }
                        3 => 65535. + (bits % 500) as f32 / 8192.,
                        4 => {
                            if i % 19 == 0 {
                                60000.
                            } else {
                                2064.
                            }
                        }
                        5 => (bits % 10000) as f32 / 8192. - 0.5,
                        6 => {
                            if i % 7 == 0 {
                                -f32::MAX
                            } else {
                                f32::MAX
                            }
                        }
                        _ => (bits % 10000) as f32 - 2000.,
                    };
                    (v, i)
                })
                .collect();
            for (sigma_low, sigma_high) in [
                (3., 2.5),
                (1., 1.),
                (0.01, 0.03),
                (0.3, 0.6),
                (5., 3.),
                (1e-10, 100.),
                (100., 1e-10),
            ] {
                let config = IntegrationConfig {
                    sigma_low,
                    sigma_high,
                    ..Default::default()
                };
                let a = combine(&values, &weights, &mut actual_scratch, &config);
                let e = prior_combine(&values, &weights, &mut prior_scratch, &config);
                assert_eq!(
                    (a.0.to_bits(), a.1, a.2, a.3.to_bits()),
                    (e.0.to_bits(), e.1, e.2, e.3.to_bits()),
                    "round={round}, n={n}, sigma={sigma_low}/{sigma_high}"
                );
            }
        }
    }
}

#[test]
fn fixed_point_certificate_checks_rounded_endpoints_and_central_ranks() {
    assert!(clipping_fixed_point(
        2000.,
        [1999., 2001.],
        2.,
        1990.,
        2010.
    ));
    assert!(!clipping_fixed_point(
        2000.,
        [1999., 2001.],
        2.,
        1999.5,
        2010.
    ));
    assert!(!clipping_fixed_point(
        2000.,
        [1999., 2001.],
        2.,
        1990.,
        2000.5
    ));
    assert!(!clipping_fixed_point(
        2000.,
        [1999., 2001.],
        2.,
        1998.5,
        2010.
    ));
    assert!(!clipping_fixed_point(
        2000.,
        [1999., 2001.],
        2.,
        1990.,
        2001.5
    ));
    assert!(!clipping_fixed_point(
        0.,
        [0., 0.],
        f32::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY
    ));
    let x = 65535_f32;
    let ulp = f32::from_bits(x.to_bits() + 1) - x;
    assert!(!clipping_fixed_point(
        f64::from(x),
        [x, x],
        ulp,
        f64::from(x) - f64::from(ulp) * 0.49,
        f64::from(x) + 1.
    ));
}
