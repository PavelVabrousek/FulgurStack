use super::*;

fn field() -> Vec<Star> {
    (0..240)
        .map(|i| Star {
            x_px: 80. + ((i * 137 + i * i * 17) % 2000) as f64,
            y_px: 80. + ((i * 271 + i * i * 23) % 3680) as f64,
            hfr_px: 1.7,
            moment_fwhm_px: 3.4,
            eccentricity: 0.1,
            flux_adu: 10000. + i as f64,
            peak_adu: 20000.,
            snr: 90.,
        })
        .collect()
}
fn radial() -> Warp {
    let mut w = Warp {
        base: Transform {
            scale_factor: 1.8,
            theta_rad: 0.12,
            tx_px: 300.,
            ty_px: 100.,
        },
        center: [1079.5, 1919.5],
        normalization: [1079.5, 1919.5],
        coefficients: [[0.; 10]; 2],
    };
    let r2 = w.center[0].powi(2) + w.center[1].powi(2);
    let (s, c) = w.base.theta_rad.sin_cos();
    for (i, dx, dy) in [
        (6, 0.03 * w.center[0].powi(3) / r2, 0.),
        (8, 0.03 * w.center[0] * w.center[1].powi(2) / r2, 0.),
        (7, 0., 0.03 * w.center[1] * w.center[0].powi(2) / r2),
        (9, 0., 0.03 * w.center[1].powi(3) / r2),
    ] {
        w.coefficients[0][i] = w.base.scale_factor * (c * dx - s * dy);
        w.coefficients[1][i] = w.base.scale_factor * (s * dx + c * dy);
    }
    w
}

#[test]
fn analytic_three_percent_radial_geometry_recovers_held_out_corners_and_inverse() {
    let src = field();
    let truth = radial();
    let dst: Vec<_> = src
        .iter()
        .cloned()
        .map(|mut s| {
            let q = truth.forward([s.x_px, s.y_px]);
            s.x_px = q[0];
            s.y_px = q[1];
            s
        })
        .collect();
    let (decision, pairs) = solve(
        &src,
        &dst,
        Some(truth.base),
        [2160, 3840],
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(decision.status, "applied", "{decision:?}");
    assert!(pairs.len() > 220);
    let recovered = decision.model.unwrap();
    assert!(decision.corrected_validation_rmse_px.unwrap() < 1e-6);
    assert!(decision.baseline_validation_rmse_px.unwrap() > 2.);
    let margin = recovered.source_margin();
    for y in 0..=20 {
        for x in 0..=20 {
            let p = [x as f64 / 20. * 2159., y as f64 / 20. * 3839.];
            let q = truth.forward(p);
            let fitted = recovered.forward(p);
            assert!((q[0] - fitted[0]).hypot(q[1] - fitted[1]) < 1e-5);
            let back = recovered.inverse(q).unwrap();
            assert!((back[0] - p[0]).hypot(back[1] - p[1]) < 1e-5);
            let linear = recovered.base.inverse().apply(q);
            assert!((linear[0] - back[0]).abs() <= margin[0] + 1e-6);
            assert!((linear[1] - back[1]).abs() <= margin[1] + 1e-6);
        }
    }
}

#[test]
fn unchanged_linear_fields_sparse_or_degenerate_data_and_cancellation_skip() {
    let src = field();
    let base = radial().base;
    let dst: Vec<_> = src
        .iter()
        .cloned()
        .map(|mut s| {
            let q = base.apply([s.x_px, s.y_px]);
            s.x_px = q[0];
            s.y_px = q[1];
            s
        })
        .collect();
    let (d, _) = solve(
        &src,
        &dst,
        Some(base),
        [2160, 3840],
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(d.status, "skipped");
    assert!(d.model.is_none());
    assert_eq!(d.reason, "no_confirmed_held_out_improvement");
    let (d, _) = solve(
        &src[..35],
        &dst,
        Some(base),
        [2160, 3840],
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(d.model.is_none());
    assert!(solve(&src, &dst, Some(base), [2160, 3840], &AtomicBool::new(true)).is_err());
    let mut invalid = radial();
    invalid.coefficients[0][1] = -5000.;
    assert!(!invalid.valid());
    invalid.coefficients[0][1] = f64::NAN;
    assert!(!invalid.valid());
    let line: Vec<_> = src
        .iter()
        .cloned()
        .map(|mut s| {
            s.y_px = 100.;
            s
        })
        .collect();
    let (d, _) = solve(&line, &dst, None, [2160, 3840], &AtomicBool::new(false)).unwrap();
    assert!(d.model.is_none());
}

#[test]
fn held_out_stars_cannot_be_hidden_by_training_inlier_selection() {
    let src = field();
    let truth = radial();
    let dst: Vec<_> = src
        .iter()
        .cloned()
        .enumerate()
        .map(|(i, mut s)| {
            let q = truth.forward([s.x_px, s.y_px]);
            s.x_px = q[0] + if i % 5 == 0 { 5. } else { 0. };
            s.y_px = q[1];
            s
        })
        .collect();
    let (decision, _) = solve_seeded(
        &src,
        &dst,
        Some(truth.base),
        Some(truth),
        [2160, 3840],
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(decision.model.is_none(), "{decision:?}");
}

#[test]
fn composed_anchor_warp_is_only_a_seed_and_preserves_known_polynomial() {
    let truth = radial();
    let pose = Transform {
        scale_factor: 1.,
        theta_rad: 0.17,
        tx_px: 43.,
        ty_px: -21.,
    };
    let composed = truth.compose_source(pose, [2160, 3840]).unwrap();
    for s in field() {
        let a = truth.forward(pose.apply([s.x_px, s.y_px]));
        let b = composed.forward([s.x_px, s.y_px]);
        assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 1e-8);
    }
}

#[test]
fn applied_coefficients_never_fit_the_held_out_validation_labels() {
    let src = field();
    let truth = radial();
    let mut models = vec![];
    for held_out_offset in [0., 0.2] {
        let dst: Vec<_> = src
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, mut s)| {
                let q = truth.forward([s.x_px, s.y_px]);
                s.x_px = q[0];
                s.y_px = q[1] + if i % 5 == 0 { held_out_offset } else { 0. };
                s
            })
            .collect();
        let (decision, _) = solve_seeded(
            &src,
            &dst,
            Some(truth.base),
            Some(truth),
            [2160, 3840],
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(decision.status, "applied", "{decision:?}");
        models.push(serde_json::to_string(&decision.model).unwrap());
    }
    assert_eq!(models[0], models[1]);
}

#[test]
fn interval_tile_enclosures_cover_curved_inverse_regions_under_rotation_and_scale() {
    let model = radial();
    let points: Vec<_> = (0..=30)
        .flat_map(|y| (0..=20).map(move |x| [x as f64 / 20. * 2159., y as f64 / 30. * 3839.]))
        .collect();
    let mut checked = 0;
    for y in (0..7500).step_by(512) {
        for x in (-512..4500).step_by(512) {
            let bounds = model.inverse_region([x, y], [512, 512]);
            for &p in &points {
                let q = model.forward(p);
                if q[0] >= x as f64
                    && q[0] <= x as f64 + 511.
                    && q[1] >= y as f64
                    && q[1] <= y as f64 + 511.
                {
                    let b = bounds.expect("curved region must not be dropped");
                    assert!(
                        p[0] >= b[0] - 1e-6
                            && p[1] >= b[1] - 1e-6
                            && p[0] <= b[2] + 1e-6
                            && p[1] <= b[3] + 1e-6
                    );
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 500);
}
