use super::*;
fn catalog() -> Vec<Star> {
    (0..90)
        .map(|i| {
            let x = 40. + ((i * 137 + i * i * 11) % 1900) as f64;
            let y = 40. + ((i * 241 + i * i * 7) % 3400) as f64;
            Star {
                x_px: x,
                y_px: y,
                hfr_px: 1.7,
                moment_fwhm_px: 3.4,
                eccentricity: 0.2,
                flux_adu: 10000. + i as f64 * 71.,
                peak_adu: 15000.,
                snr: 100.,
            }
        })
        .collect()
}
#[test]
fn similarity_scale_rotation_sweep_and_composition() {
    let src = catalog();
    let cancel = AtomicBool::new(false);
    for scale in [
        MIN_GEOMETRY_SCALE,
        0.33,
        0.5,
        1.,
        2.,
        3.,
        3.25,
        MAX_GEOMETRY_SCALE,
    ] {
        for degrees in [0_f64, 15., 90., 179.5] {
            let expected = Transform {
                scale_factor: scale,
                theta_rad: degrees.to_radians(),
                tx_px: 177.3,
                ty_px: -64.9,
            };
            let mut dst: Vec<_> = src
                .iter()
                .enumerate()
                .filter(|(i, _)| i % 6 != 0)
                .map(|(i, s)| {
                    let p = expected.apply(point(s));
                    let mut s = s.clone();
                    s.x_px = p[0] + (i as f64).sin() * 0.015;
                    s.y_px = p[1] + (i as f64).cos() * 0.015;
                    s
                })
                .collect();
            let mut outliers = catalog();
            for s in &mut outliers {
                s.x_px += 52.31;
                s.y_px -= 173.94;
            }
            dst.extend(outliers.into_iter().take(12));
            dst.reverse();
            let got = solve_similarity(&src, &dst, None, &cancel)
                .unwrap()
                .expect("similarity match");
            let error = (src
                .iter()
                .map(|s| distance(expected.apply(point(s)), got.transform.apply(point(s))).powi(2))
                .sum::<f64>()
                / src.len() as f64)
                .sqrt();
            assert!(error < 0.15, "scale={scale} angle={degrees}: {error}");
            assert!((got.transform.scale_factor - scale).abs() < 0.0001);
            assert!(got.pairs.len() >= 70);
            let p = [101.7, 277.3];
            assert!(distance(got.transform.inverse().apply(got.transform.apply(p)), p) < 1e-9);
            let next = Transform {
                scale_factor: 0.7,
                theta_rad: 0.12,
                tx_px: -13.,
                ty_px: 9.,
            };
            assert!(
                distance(
                    got.transform.compose(next).apply(p),
                    got.transform.apply(next.apply(p))
                ) < 1e-9
            );
        }
    }
}
#[test]
fn similarity_rejects_out_of_range_reflection_and_insufficient_matches() {
    let src = catalog();
    let cancel = AtomicBool::new(false);
    for scale in [0.27, 3.7, -1.] {
        let dst: Vec<_> = src
            .iter()
            .map(|s| {
                let mut s = s.clone();
                s.x_px *= scale;
                s.y_px *= scale.abs();
                s
            })
            .collect();
        assert!(
            solve_similarity(&src, &dst, None, &cancel)
                .unwrap()
                .is_none()
        );
    }
    assert!(
        solve_similarity(&src[..9], &src, None, &cancel)
            .unwrap()
            .is_none()
    );
    assert!(solve_similarity(&src, &src, None, &AtomicBool::new(true)).is_err());
}
#[test]
fn rotation_sweep_with_missing_stars_false_detections_and_centroid_noise() {
    let dst = catalog();
    let cancel = AtomicBool::new(false);
    for angle in [0_f64, 15., 90., 179.5, -179.5] {
        let expected = Transform {
            scale_factor: 1.,
            theta_rad: angle.to_radians(),
            tx_px: 132.7,
            ty_px: -81.3,
        };
        let mut src: Vec<_> = dst
            .iter()
            .enumerate()
            .filter(|(i, _)| i % 5 != 0)
            .map(|(i, s)| {
                let p = expected.inverse().apply(point(s));
                let mut s = s.clone();
                s.x_px = p[0] + (i as f64 * 1.7).sin() * 0.02;
                s.y_px = p[1] + (i as f64 * 0.9).cos() * 0.02;
                s
            })
            .collect();
        let mut extras = catalog();
        for s in extras.iter_mut().take(15) {
            s.x_px += 45.3;
            s.y_px -= 31.7;
        }
        src.extend(extras.into_iter().take(15));
        src.reverse();
        let got = solve(&src, &dst, None, &cancel)
            .unwrap()
            .expect("rotation fallback");
        let error = (src
            .iter()
            .take(72)
            .map(|s| distance(got.transform.apply(point(s)), expected.apply(point(s))).powi(2))
            .sum::<f64>()
            / 72.)
            .sqrt();
        println!(
            "Rotation gate {angle} deg: transform recovery {error:.9}px, inlier residual {:.9}px",
            got.rmse
        );
        assert!(error < 0.15, "angle={angle}, recovered RMSE={error}");
        assert!(got.pairs.len() >= 65);
        assert!(got.rmse < 0.04);
    }
}
#[test]
fn prediction_fallback_and_composition_do_not_accumulate_drift() {
    let dst = catalog();
    let t = Transform {
        scale_factor: 1.,
        theta_rad: 0.002,
        tx_px: 2.3,
        ty_px: -1.7,
    };
    let src: Vec<_> = dst
        .iter()
        .map(|s| {
            let p = t.inverse().apply(point(s));
            let mut s = s.clone();
            s.x_px = p[0];
            s.y_px = p[1];
            s
        })
        .collect();
    let s = solve(
        &src,
        &dst,
        Some(Transform::IDENTITY),
        &AtomicBool::new(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!(s.method, "sequential_direct");
    assert!(s.rmse < 1e-9);
    let bridge = Transform {
        scale_factor: 1.,
        theta_rad: 3.13,
        tx_px: 1800.,
        ty_px: 3500.,
    };
    let p = [120., 140.];
    assert!(distance(bridge.compose(t).apply(p), bridge.apply(t.apply(p))) < 1e-9);
    assert!(distance(t.inverse().apply(t.apply(p)), p) < 1e-9);
    assert!(
        solve(&src[..9], &dst, None, &AtomicBool::new(false))
            .unwrap()
            .is_none()
    );
    assert!(solve(&src, &dst, None, &AtomicBool::new(true)).is_err());
}
#[test]
fn canvas_matches_brute_force_weighted_coverage_and_polygon_intersection() {
    let a = footprint(Transform::IDENTITY, 24, 20);
    let b = footprint(
        Transform {
            scale_factor: 1.,
            theta_rad: 0.,
            tx_px: 4.,
            ty_px: 2.,
        },
        24,
        20,
    );
    let common = intersection(a.clone(), &b);
    let cancel = AtomicBool::new(false);
    let c = canvas(&[(common, 1.)], 1., &cancel).unwrap().unwrap();
    assert_eq!(
        (c.origin_x_px, c.origin_y_px, c.width, c.height),
        (5, 3, 17, 15)
    );
    let polygons = vec![(a, 0.8), (b, 0.2)];
    let c = canvas(&polygons, 0.8, &cancel).unwrap().unwrap();
    assert_eq!(
        (c.origin_x_px, c.origin_y_px, c.width, c.height),
        (1, 1, 21, 17)
    );
    let c = canvas(&polygons, 1., &cancel).unwrap().unwrap();
    assert_eq!(
        (c.origin_x_px, c.origin_y_px, c.width, c.height),
        (5, 3, 17, 15)
    );
    let rotated = footprint(
        Transform {
            scale_factor: 1.,
            theta_rad: 0.4,
            tx_px: 10.,
            ty_px: -2.,
        },
        24,
        20,
    );
    let ps = vec![(polygons[0].0.clone(), 0.6), (rotated, 0.4)];
    for threshold in [0.6, 0.8, 1.] {
        if let Some(c) = canvas(&ps, threshold, &cancel).unwrap() {
            for y in c.origin_y_px..c.origin_y_px + c.height as i32 {
                for x in c.origin_x_px..c.origin_x_px + c.width as i32 {
                    let weight: f64 = ps
                        .iter()
                        .filter(|(p, _)| {
                            (0..p.len()).all(|i| {
                                cross(p[i], p[(i + 1) % p.len()], [x as f64, y as f64]) >= -1e-8
                            })
                        })
                        .map(|(_, w)| w)
                        .sum();
                    assert!(weight + 1e-9 >= threshold);
                }
            }
        }
    }
    assert!(canvas(&[], 0.8, &cancel).unwrap().is_none());
    assert!(canvas(&ps, 0.8, &AtomicBool::new(true)).is_err());
}
