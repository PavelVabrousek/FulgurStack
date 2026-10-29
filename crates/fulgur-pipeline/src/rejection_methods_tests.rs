use super::*;

/// Deterministic Gaussian samples (Box-Muller on a fixed LCG).
fn gaussian(n: usize, mean: f64, sigma: f64, seed: u64) -> Vec<f32> {
    let mut state = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let mut uniform = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((state >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    };
    (0..n)
        .map(|_| {
            let (u, v) = (uniform(), uniform());
            (mean + sigma * (-2. * u.ln()).sqrt() * (2. * std::f64::consts::PI * v).cos()) as f32
        })
        .collect()
}

#[test]
fn student_t_quantiles_and_esd_critical_values_match_tables() {
    for (p, df, expected) in [
        (0.975, 1., 12.7062),
        (0.975, 10., 2.2281),
        (0.995, 5., 4.0321),
        (0.95, 30., 1.6973),
    ] {
        let t = student_t_quantile(p, df);
        assert!((t - expected).abs() < 1e-3, "t({p}, {df}) = {t}");
    }
    // NIST/SEMATECH e-Handbook generalized ESD example: n = 54, alpha 0.05.
    let lambda = esd_critical_values(54, 10, 0.05);
    assert!((lambda[0] - 3.159).abs() < 0.002, "{lambda:?}");
    assert!((lambda[9] - 3.085).abs() < 0.002, "{lambda:?}");
}

#[test]
fn satellite_trails_are_rejected_and_the_clean_level_survives() {
    let mut values = gaussian(40, 1000., 10., 7);
    values.extend([5000., 4800., 5200.]);
    let weights = vec![1.; values.len()];
    let mut scratch = MethodScratch::default();
    for (name, combined) in [
        ("linear fit", linear_fit(&values, &weights, &mut scratch)),
        ("esd", esd(&values, &weights, &mut scratch)),
    ] {
        let (value, low, high, weight) = combined;
        assert_eq!(high, 3, "{name}: trail samples rejected");
        assert!(low <= 1, "{name}: at most one Gaussian tail sample ({low})");
        assert!((value - 1000.).abs() < 6., "{name}: {value}");
        assert!((weight - (43 - high - low) as f64).abs() < 1e-9);
    }
    let (value, low, high, weight) = median(&values, &weights, &mut scratch);
    assert!((value - 1000.).abs() < 6. && low == 0 && high == 0 && weight == 43.);
}

#[test]
fn gradual_level_spread_survives_linear_fit_and_weights_are_kept() {
    // Frames from several nights: sky levels spread evenly 900..1100.
    let values: Vec<f32> = (0..30).map(|i| 900. + i as f32 * 200. / 29.).collect();
    let weights: Vec<f64> = (0..30).map(|i| 1. + i as f64 / 30.).collect();
    let mut scratch = MethodScratch::default();
    let (value, low, high, weight) = linear_fit(&values, &weights, &mut scratch);
    assert_eq!((low, high), (0, 0));
    let expected = values
        .iter()
        .zip(&weights)
        .map(|(&v, &w)| f64::from(v) * w)
        .sum::<f64>()
        / weights.iter().sum::<f64>();
    assert!(
        (f64::from(value) - expected).abs() < 1e-3,
        "{value} vs {expected}"
    );
    assert!((weight - weights.iter().sum::<f64>()).abs() < 1e-9);
    // Pure Gaussian noise: ESD keeps (almost) everything.
    let clean = gaussian(200, 500., 5., 11);
    let (_, low, high, _) = esd(&clean, &vec![1.; 200], &mut scratch);
    assert!(low + high <= 2, "{low} + {high}");
}

#[test]
fn small_stacks_are_averaged_without_rejection() {
    let values = [100., 101., 5000., 99.];
    let weights = [1., 1., 1., 1.];
    let mut scratch = MethodScratch::default();
    for combined in [
        linear_fit(&values, &weights, &mut scratch),
        esd(&values, &weights, &mut scratch),
    ] {
        assert_eq!((combined.1, combined.2), (0, 0));
        assert!((combined.0 - 1325.).abs() < 1e-3);
    }
    assert_eq!(median(&values, &weights, &mut scratch).0, 100.5);
    assert_eq!(esd(&[], &[], &mut scratch), (0., 0, 0, 0.));
}

#[test]
fn auto_resolves_by_frame_count_and_the_stack_dispatches_each_method() {
    use super::super::{ClippingScratch, IntegrationConfig, Rejection, rgb_rejection};
    for (frames, method) in [
        (3, Rejection::Median),
        (5, Rejection::Median),
        (6, Rejection::Winsorized),
        (24, Rejection::Winsorized),
        (25, Rejection::Esd),
        (49, Rejection::Esd),
        (50, Rejection::Esd),
        (1000, Rejection::Esd),
    ] {
        assert_eq!(Rejection::Auto.resolve(frames), method, "{frames}");
        assert_eq!(method.resolve(frames), method);
    }
    for method in Rejection::ALL {
        assert_eq!(Rejection::parse(method.key()).unwrap(), method);
    }
    assert!(Rejection::parse("kappa").is_err());
    // One trail sample among 60: every rejecting method removes it.
    let mut values = gaussian(59, 2000., 15., 3);
    values.push(12000.);
    let weights = vec![1.; 60];
    let mut scratch = ClippingScratch::with_capacity(60);
    for method in [Rejection::Winsorized, Rejection::LinearFit, Rejection::Esd] {
        let config = IntegrationConfig {
            rejection: method,
            ..IntegrationConfig::default()
        };
        let (value, _, high, _) =
            rgb_rejection::combine_dense(&values, &weights, &mut scratch, &config);
        assert!(
            high >= 1 && (value - 2000.).abs() < 10.,
            "{method:?}: {value} {high}"
        );
    }
    let config = IntegrationConfig {
        rejection: Rejection::Median,
        ..IntegrationConfig::default()
    };
    let (value, low, high, _) =
        rgb_rejection::combine_dense(&values, &weights, &mut scratch, &config);
    assert!((value - 2000.).abs() < 10. && low + high == 0);
}
