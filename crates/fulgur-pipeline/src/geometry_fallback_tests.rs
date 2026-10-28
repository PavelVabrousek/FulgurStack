use super::*;

fn catalog() -> Vec<Star> {
    (0..48)
        .map(|i| Star {
            x_px: 40. + ((i * 137 + i * i * 11) % 1900) as f64,
            y_px: 40. + ((i * 241 + i * i * 7) % 3400) as f64,
            hfr_px: 1.7,
            moment_fwhm_px: 3.4,
            eccentricity: 0.2,
            flux_adu: 10000. + i as f64 * 71.,
            peak_adu: 15000.,
            snr: 10. + (i % 17) as f64 * 11.,
        })
        .collect()
}

fn exact(actual: Option<Solution>, expected: Option<Solution>) {
    // Snapshot checks every field and source/match order; bits additionally check signed zero.
    assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
    if let (Some(a), Some(b)) = (actual, expected) {
        let t = |s: &Solution| {
            [
                s.transform.scale_factor,
                s.transform.theta_rad,
                s.transform.tx_px,
                s.transform.ty_px,
                s.rmse,
            ]
            .map(f64::to_bits)
        };
        assert_eq!(t(&a), t(&b));
        assert_eq!(a.method, b.method);
        for (a, b) in a.pairs.iter().zip(&b.pairs) {
            assert_eq!(
                (a.source, a.reference, a.residual_px.to_bits()),
                (b.source, b.reference, b.residual_px.to_bits())
            );
        }
    }
}

#[test]
fn prepared_fallback_and_reused_consensus_match_frozen_solver_bits() {
    let dst = catalog();
    let cancel = AtomicBool::new(false);
    for similarity in [false, true] {
        let matcher = Matcher::new(&dst, similarity);
        let scales: &[f64] = if similarity {
            &[0.30, 0.33, 0.5, 1., 2., 3., 1. / 0.30]
        } else {
            &[1.]
        };
        for &scale in scales {
            for degrees in [0_f64, 15., 90., 179.5, -179.5] {
                let transform = Transform {
                    scale_factor: scale,
                    theta_rad: degrees.to_radians(),
                    tx_px: 132.7,
                    ty_px: -81.3,
                };
                let mut src: Vec<_> = dst
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| i % 7 != 0)
                    .map(|(i, s)| {
                        let p = transform.inverse().apply(point(s));
                        let mut s = s.clone();
                        s.x_px = p[0] + (i as f64 * 1.7).sin() * 0.015;
                        s.y_px = p[1] + (i as f64 * 0.9).cos() * 0.015;
                        s
                    })
                    .collect();
                let mut extras = catalog();
                for s in &mut extras {
                    s.x_px += 45.3;
                    s.y_px -= 31.7;
                }
                src.extend(extras.into_iter().take(7));
                src.reverse();
                for prediction in [None, Some(Transform::IDENTITY), Some(transform)] {
                    let prior =
                        fallback_oracle::solve_model(&src, &dst, prediction, &cancel, similarity)
                            .unwrap();
                    let actual = matcher.solve(&src, prediction, &cancel).unwrap();
                    exact(actual, prior);
                }
            }
        }
        // Duplicate/equidistant positions exercise nearest-star and ranking ties.
        let mut tied = dst.clone();
        tied.push(dst[0].clone());
        tied[1].x_px = tied[0].x_px + 1.5;
        tied[1].y_px = tied[0].y_px;
        for src in [&tied[..], &dst[..9], &dst[..0]] {
            exact(
                matcher.solve(src, None, &cancel).unwrap(),
                fallback_oracle::solve_model(src, &dst, None, &cancel, similarity).unwrap(),
            );
        }
    }
}

#[test]
fn prepared_target_is_lazy_reused_and_does_not_modify_catalogs_or_skip_cancellation() {
    let dst = catalog();
    let snapshot = format!("{dst:?}");
    let cancel = AtomicBool::new(false);
    let matcher = Matcher::new(&dst, false);
    assert!(matcher.fallback.get().is_none());
    assert!(matcher.solve(&dst, None, &AtomicBool::new(true)).is_err());
    assert!(matcher.fallback.get().is_none());
    exact(
        matcher
            .solve(&dst, Some(Transform::IDENTITY), &cancel)
            .unwrap(),
        fallback_oracle::solve(&dst, &dst, Some(Transform::IDENTITY), &cancel).unwrap(),
    );
    assert!(
        matcher.fallback.get().is_none(),
        "successful prediction must not build triangles"
    );
    exact(
        matcher.solve(&dst, None, &cancel).unwrap(),
        fallback_oracle::solve(&dst, &dst, None, &cancel).unwrap(),
    );
    let prepared = matcher.fallback.get().unwrap() as *const FallbackIndex;
    let triangle_storage = matcher.fallback.get().unwrap().triangles.as_ptr();
    for _ in 0..3 {
        exact(
            matcher.solve(&dst, None, &cancel).unwrap(),
            fallback_oracle::solve(&dst, &dst, None, &cancel).unwrap(),
        );
        assert_eq!(
            matcher.fallback.get().unwrap() as *const FallbackIndex,
            prepared
        );
        assert_eq!(
            matcher.fallback.get().unwrap().triangles.as_ptr(),
            triangle_storage
        );
    }
    assert!(matcher.solve(&dst, None, &AtomicBool::new(true)).is_err());
    assert_eq!(format!("{dst:?}"), snapshot);
    let other = catalog();
    let separate = Matcher::new(&other, false);
    assert!(
        separate.fallback.get().is_none(),
        "no global catalog retention"
    );
}

#[test]
fn real_native_catalog_fallback_and_similarity_match_frozen_solver_bits() {
    let Some(root) = std::env::var_os("FULGURSTACK_TEST_FITS_DIR") else {
        return;
    };
    let mut paths = vec![];
    crate::survey::collect_inputs(std::path::Path::new(&root), &mut paths).unwrap();
    paths.sort();
    assert!(paths.len() >= 3);
    let cancel = AtomicBool::new(false);
    for path in paths.iter().take(3) {
        let raw = fulgur_core::fits::read_raw_sequential(path).unwrap();
        let dst = crate::stars::analyze(&raw, &cancel).unwrap().stars;
        for similarity in [false, true] {
            let matcher = Matcher::new(&dst, similarity);
            let expected = Transform {
                scale_factor: if similarity { 2. } else { 1. },
                theta_rad: 0.31,
                tx_px: 76.3,
                ty_px: -34.9,
            };
            let src: Vec<_> = dst
                .iter()
                .enumerate()
                .filter(|(i, _)| i % 6 != 0)
                .map(|(_, s)| {
                    let p = expected.inverse().apply(point(s));
                    let mut s = s.clone();
                    s.x_px = p[0];
                    s.y_px = p[1];
                    s
                })
                .collect();
            let prior = if similarity {
                fallback_oracle::solve_similarity(&src, &dst, None, &cancel)
            } else {
                fallback_oracle::solve(&src, &dst, None, &cancel)
            }
            .unwrap();
            exact(matcher.solve(&src, None, &cancel).unwrap(), prior);
            // Same prepared target, now with a direct prediction and the same immutable sources.
            exact(
                matcher.solve(&src, Some(expected), &cancel).unwrap(),
                fallback_oracle::solve_model(&src, &dst, Some(expected), &cancel, similarity)
                    .unwrap(),
            );
        }
        assert_eq!(
            fulgur_core::fits::read_raw_sequential(path).unwrap().pixels,
            raw.pixels
        );
    }
}
