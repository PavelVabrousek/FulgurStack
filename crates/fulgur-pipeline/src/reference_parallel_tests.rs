use super::*;
use crate::geometry::{Match, Transform};
use crate::inspect::QualityFrame;
use crate::register::RegisteredFrame;

fn dense_case() -> (
    RegisteredFrame,
    QualityFrame,
    Vec<stars::Star>,
    Vec<stars::Star>,
    crate::distortion::Warp,
    Vec<Match>,
) {
    let (mut rows, _, _) = fixture(Transform::IDENTITY);
    let mut source = QualityFrame::empty("dense.fit".into());
    source.hfr_px = Some(1.7);
    source.accepted = true;
    source.stars = (0..240)
        .map(|i| stars::Star {
            x_px: 80. + ((i * 137 + i * i * 17) % 2000) as f64,
            y_px: 80. + ((i * 271 + i * i * 23) % 3680) as f64,
            hfr_px: 1.7,
            moment_fwhm_px: 3.4,
            eccentricity: 0.1,
            flux_adu: 10000. + i as f64,
            peak_adu: 20000.,
            snr: 90.,
        })
        .collect();
    let anchor = source.stars.clone();
    let mut model = crate::distortion::Warp {
        base: Transform {
            scale_factor: 1.,
            ..Transform::IDENTITY
        },
        center: [1079.5, 1919.5],
        normalization: [1079.5, 1919.5],
        coefficients: [[0.; 10]; 2],
    };
    let r2 = model.center[0].powi(2) + model.center[1].powi(2);
    model.coefficients[0][6] = 0.03 * model.center[0].powi(3) / r2;
    model.coefficients[0][8] = 0.03 * model.center[0] * model.center[1].powi(2) / r2;
    model.coefficients[1][7] = 0.03 * model.center[1] * model.center[0].powi(2) / r2;
    model.coefficients[1][9] = 0.03 * model.center[1].powi(3) / r2;
    let catalog = anchor
        .iter()
        .cloned()
        .map(|mut s| {
            let p = model.forward([s.x_px, s.y_px]);
            s.x_px = p[0];
            s.y_px = p[1];
            s
        })
        .collect();
    let identities: Vec<_> = (0..240)
        .map(|i| Match {
            source: i,
            reference: i,
            residual_px: 0.,
        })
        .collect();
    let mut row = rows.remove(1);
    row.accepted = true;
    row.quality_accepted = true;
    row.transform = Some(Transform::IDENTITY);
    row.source_to_photometric_anchor = Some(Transform::IDENTITY);
    row.photometric_matches = identities.clone();
    row.geometric_correction = Some(crate::distortion::Decision::skipped(
        "outer_quadrant_not_validated",
    ));
    (row, source, anchor, catalog, model, identities)
}

#[test]
fn skipped_correction_never_accepts_a_small_linear_inlier_cluster() {
    let (mut row, source, anchor, catalog, model, identities) = dense_case();
    let mut fit = geometry::solve_similarity(
        &source.stars,
        &catalog,
        Some(model.base),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(
        fit.is_some(),
        "the old fallback would have accepted a local cluster"
    );
    validate_external_row(
        &mut row,
        &source,
        crate::distortion::Field::sensor([2160, 3840]),
        &mut fit,
        &FitContext {
            catalog: &catalog,
            bridge: crate::geometry::Transform::IDENTITY,
            name: "",
            sensor: [2160, 3840],
            reference: [2160, 3840],
            diagnose_excluded: false,
            geometric_correction: true,
            anchor_correction: None,
            anchor_context: Some((&anchor, &identities)),
            partial: false,
            limits: crate::distortion::AccuracyLimits::default(),
            deep: None,
            files: None,
            cancel: &std::sync::atomic::AtomicBool::new(false),
        },
    );
    assert!(fit.is_none());
    assert_eq!(row.geometric_correction.unwrap().status, "rejected");
    assert!(
        row.rejection_reasons
            .iter()
            .any(|r| r == "external_whole_field_registration_failed")
    );
}

#[test]
fn recovery_and_linear_paths_share_one_target_and_hold_out_source_pose_labels() {
    let mut models = vec![];
    for offset in [0., 0.2] {
        let (mut row, mut source, anchor, catalog, model, identities) = dense_case();
        let reserved = crate::distortion::validation::held_out(&source.stars, &identities);
        for (i, s) in source.stars.iter_mut().enumerate() {
            if reserved.contains(&i) {
                s.y_px += offset;
            }
        }
        let mut fit = None;
        validate_external_row(
            &mut row,
            &source,
            crate::distortion::Field::sensor([2160, 3840]),
            &mut fit,
            &FitContext {
                catalog: &catalog,
                bridge: crate::geometry::Transform::IDENTITY,
                name: "",
                sensor: [2160, 3840],
                reference: [2160, 3840],
                diagnose_excluded: false,
                geometric_correction: true,
                anchor_correction: Some(model),
                anchor_context: Some((&anchor, &identities)),
                partial: false,
                limits: crate::distortion::AccuracyLimits::default(),
                deep: None,
                files: None,
                cancel: &std::sync::atomic::AtomicBool::new(false),
            },
        );
        let fit = fit.unwrap();
        assert_eq!(fit.method, "external_anchor_cubic_validated");
        assert!(fit.rmse < 0.3);
        let decision = row.geometric_correction.unwrap();
        assert!(decision.registration_validation.unwrap().accepted);
        models.push(serde_json::to_string(&decision.model).unwrap());
    }
    assert_eq!(
        models[0], models[1],
        "held-out labels must not fit the recovery pose"
    );
    let (mut row, source, anchor, _, _, identities) = dense_case();
    let mut fit = None;
    validate_external_row(
        &mut row,
        &source,
        crate::distortion::Field::sensor([2160, 3840]),
        &mut fit,
        &FitContext {
            catalog: &anchor,
            bridge: crate::geometry::Transform::IDENTITY,
            name: "",
            sensor: [2160, 3840],
            reference: [2160, 3840],
            diagnose_excluded: false,
            geometric_correction: true,
            anchor_correction: None,
            anchor_context: Some((&anchor, &identities)),
            partial: false,
            limits: crate::distortion::AccuracyLimits::default(),
            deep: None,
            files: None,
            cancel: &std::sync::atomic::AtomicBool::new(false),
        },
    );
    assert_eq!(fit.unwrap().method, "external_similarity_field_validated");
    assert!(
        row.geometric_correction
            .unwrap()
            .registration_validation
            .unwrap()
            .accepted
    );
}

#[test]
fn recovery_does_not_blindly_share_distortion_between_different_optics() {
    let (mut row, mut source, anchor, catalog, model, identities) = dense_case();
    for s in &mut source.stars {
        let p = model.forward([s.x_px, s.y_px]);
        s.x_px = p[0];
        s.y_px = p[1];
    }
    let mut fit = None;
    validate_external_row(
        &mut row,
        &source,
        crate::distortion::Field::sensor([2160, 3840]),
        &mut fit,
        &FitContext {
            catalog: &catalog,
            bridge: crate::geometry::Transform::IDENTITY,
            name: "",
            sensor: [2160, 3840],
            reference: [2160, 3840],
            diagnose_excluded: false,
            geometric_correction: true,
            anchor_correction: Some(model),
            anchor_context: Some((&anchor, &identities)),
            partial: false,
            limits: crate::distortion::AccuracyLimits::default(),
            deep: None,
            files: None,
            cancel: &std::sync::atomic::AtomicBool::new(false),
        },
    );
    // The ordinary map is actually correct here, and may pass its own gate;
    // an incorrect shared nonlinear model must never be applied.
    assert!(row.geometric_correction.as_ref().unwrap().model.is_none());
    if let Some(fit) = fit {
        assert_eq!(fit.method, "external_similarity_field_validated");
    }
}

#[test]
fn confirmed_external_mapping_recovers_native_failure_with_consistent_weight_units() {
    let (mut row, source, anchor, catalog, model, identities) = dense_case();
    row.accepted = false;
    row.registration_success = false;
    row.final_weight = 0.;
    row.rejection_reasons
        .push("native_insufficient_field_evidence".into());
    let mut fit = None;
    validate_external_row(
        &mut row,
        &source,
        crate::distortion::Field::sensor([2160, 3840]),
        &mut fit,
        &FitContext {
            catalog: &catalog,
            bridge: crate::geometry::Transform::IDENTITY,
            name: "",
            sensor: [2160, 3840],
            reference: [2160, 3840],
            diagnose_excluded: false,
            geometric_correction: true,
            anchor_correction: Some(model),
            anchor_context: Some((&anchor, &identities)),
            partial: false,
            limits: crate::distortion::AccuracyLimits::default(),
            deep: None,
            files: None,
            cancel: &std::sync::atomic::AtomicBool::new(false),
        },
    );
    assert!(fit.is_some() && row.accepted && row.registration_success);
    let expected = 1. / (22_f64.powi(2) * 1.7_f64.powi(2));
    assert!((row.final_weight / expected - 1.).abs() < 4. * f64::EPSILON);
    assert!(row.rejection_reasons.is_empty());
}

#[test]
fn partial_frames_keep_the_most_accurate_refused_map_in_reference_pixels() {
    let (mut row, source, anchor, catalog, model, identities) = dense_case();
    row.accepted = false;
    row.registration_success = false;
    row.final_weight = 0.;
    row.rejection_reasons
        .push("native_insufficient_field_evidence".into());
    let mut fit = geometry::solve_similarity(
        &source.stars,
        &catalog,
        Some(model.base),
        &AtomicBool::new(false),
    )
    .unwrap();
    validate_external_row(
        &mut row,
        &source,
        crate::distortion::Field::sensor([2160, 3840]),
        &mut fit,
        &FitContext {
            catalog: &catalog,
            bridge: crate::geometry::Transform::IDENTITY,
            name: "",
            sensor: [2160, 3840],
            reference: [2160, 3840],
            diagnose_excluded: false,
            geometric_correction: true,
            anchor_correction: None,
            anchor_context: Some((&anchor, &identities)),
            partial: true,
            limits: crate::distortion::AccuracyLimits::default(),
            deep: None,
            files: None,
            cancel: &std::sync::atomic::AtomicBool::new(false),
        },
    );
    let fit = fit.expect("the accurate tiles keep the frame");
    assert_eq!(fit.method, "external_partial_field");
    assert!(row.accepted && row.registration_success && row.final_weight > 0.);
    assert!(row.rejection_reasons.is_empty());
    let decision = row.geometric_correction.unwrap();
    assert_eq!(decision.status, "partial");
    assert!(decision.model.is_none());
    // Witness errors are measured on the reference, against its own stars.
    let field = row.partial_field.unwrap();
    assert_eq!(field.stars.len(), identities.len());
    assert_eq!(field.fit, "similarity");
    assert!(field.fit_rmse_px <= field.fit_maximum_error_px);
    let centre = field
        .stars
        .iter()
        .filter(|s| (s[0] - 1079.5).hypot(s[1] - 1919.5) < 500.)
        .map(|s| s[2])
        .fold(0., f64::max);
    let edge = field.stars.iter().map(|s| s[2]).fold(0., f64::max);
    assert!(edge > 3. * centre.max(1e-6));
    // The default keeps refusing the same frame.
    let (mut row, source, anchor, catalog, model, identities) = dense_case();
    let mut fit = geometry::solve_similarity(
        &source.stars,
        &catalog,
        Some(model.base),
        &AtomicBool::new(false),
    )
    .unwrap();
    validate_external_row(
        &mut row,
        &source,
        crate::distortion::Field::sensor([2160, 3840]),
        &mut fit,
        &FitContext {
            catalog: &catalog,
            bridge: crate::geometry::Transform::IDENTITY,
            name: "",
            sensor: [2160, 3840],
            reference: [2160, 3840],
            diagnose_excluded: false,
            geometric_correction: true,
            anchor_correction: None,
            anchor_context: Some((&anchor, &identities)),
            partial: false,
            limits: crate::distortion::AccuracyLimits::default(),
            deep: None,
            files: None,
            cancel: &std::sync::atomic::AtomicBool::new(false),
        },
    );
    assert!(fit.is_none() && row.partial_field.is_none());
}

#[test]
fn only_an_isolated_pair_counts_as_a_chance_match() {
    use crate::distortion::validation::without_chance_pairs;
    let (.., catalog, _, _) = dense_case();
    let identity = |p: [f64; 2]| p;
    let star = |x: f64, y: f64| stars::Star {
        x_px: x,
        y_px: y,
        ..catalog[0].clone()
    };
    let grid: Vec<_> = (0..100)
        .map(|i| star(100. + (i % 10) as f64 * 300., 100. + (i / 10) as f64 * 300.))
        .collect();
    let pairs: Vec<_> = (0..100)
        .map(|i| Match {
            source: i,
            reference: i,
            residual_px: 0.,
        })
        .collect();
    // One star off on its own goes; the rest stay.
    let mut shifted = grid.clone();
    shifted[44].x_px += 9.;
    assert_eq!(
        without_chance_pairs(&grid, &shifted, pairs.clone(), identity).len(),
        99
    );
    // A whole corner off is a true map error and its witnesses stay.
    let mut corner = grid.clone();
    for s in corner
        .iter_mut()
        .filter(|s| s.x_px > 1800. && s.y_px > 1800.)
    {
        s.x_px += 9.;
    }
    assert!(without_chance_pairs(&grid, &corner, pairs.clone(), identity).len() >= 97);
}

// Frozen prior serial per-source loop: no shared helper with production dispatch.
fn prior_serial_rows(
    rows: &mut [RegisteredFrame],
    sources: &[QualityFrame],
    catalog: &[stars::Star],
    bridge: Transform,
    name: &str,
    sensor: [usize; 2],
    cancel: &AtomicBool,
) -> Result<()> {
    for (row, source) in rows.iter_mut().zip(sources) {
        check_cancelled(cancel)?;
        let Some(native) = row.transform else {
            continue;
        };
        row.source_to_photometric_anchor = Some(native);
        row.match_reference = name.to_owned();
        let fit = geometry::solve_similarity(
            &source.stars,
            catalog,
            Some(bridge.compose(native)),
            cancel,
        )?;
        if let Some(fit) = fit {
            row.transform = Some(fit.transform);
            row.rotation_deg = Some(fit.transform.theta_rad.to_degrees());
            row.matches = fit.pairs;
            row.match_reference = name.to_owned();
            row.method = Some("external_similarity_direct");
            row.inlier_count = row.matches.len();
            row.inlier_rmse_px = Some(fit.rmse);
            if row.accepted {
                row.footprint = geometry::footprint(fit.transform, sensor[0], sensor[1]);
            }
        } else {
            row.registration_success = false;
            row.accepted = false;
            row.final_weight = 0.;
            row.transform = None;
            row.rotation_deg = None;
            row.method = None;
            row.footprint.clear();
            row.matches.clear();
            row.inlier_count = 0;
            row.inlier_rmse_px = None;
            row.rejection_reasons
                .push("external_reference_registration_failed".into());
        }
    }
    Ok(())
}

fn fixture(bridge: Transform) -> (Vec<RegisteredFrame>, Vec<QualityFrame>, Vec<stars::Star>) {
    let base: Vec<_> = (0..34)
        .map(|i| stars::Star {
            x_px: 50. + ((i * 137 + i * i * 11) % 1900) as f64,
            y_px: 50. + ((i * 241 + i * i * 7) % 3400) as f64,
            hfr_px: 1.7,
            moment_fwhm_px: 3.4,
            eccentricity: 0.2,
            flux_adu: 10000. + i as f64 * 71.,
            peak_adu: 20000.,
            snr: 100.,
        })
        .collect();
    let target = base
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let p = bridge.apply([s.x_px, s.y_px]);
            let mut s = s.clone();
            s.x_px = p[0] + (i as f64).sin() * 0.001;
            s.y_px = p[1] + (i as f64).cos() * 0.001;
            s
        })
        .collect();
    let mut rows = Vec::new();
    let mut sources = Vec::new();
    for i in 0..19 {
        let native = Transform {
            scale_factor: 1.,
            theta_rad: (i as f64 - 9.) * 0.01,
            tx_px: i as f64 * 0.3,
            ty_px: i as f64 * -0.7,
        };
        let mut source = QualityFrame::empty(format!("synthetic_{i:03}.fit"));
        source.stars = base
            .iter()
            .enumerate()
            .filter(|(j, _)| (j + i) % 9 != 0)
            .map(|(_, s)| {
                let p = native.inverse().apply([s.x_px, s.y_px]);
                let mut s = s.clone();
                s.x_px = p[0];
                s.y_px = p[1];
                s
            })
            .collect();
        if i % 11 == 0 {
            source.stars.truncate(8);
        } // Deterministic external failure.
        if i % 5 == 0 {
            let mut s = base[0].clone();
            s.x_px = -1000.;
            s.y_px = 9000.;
            source.stars.push(s);
        }
        source.accepted = i % 6 != 0;
        rows.push(RegisteredFrame {
            geometric_correction: None,
            source_to_photometric_anchor_correction: None,
            file_name: source.file_name.clone(),
            session_id: i / 7,
            quality_accepted: source.accepted,
            registration_attempted: true,
            registration_success: true,
            accepted: source.accepted,
            rejection_reasons: if source.accepted {
                vec![]
            } else {
                vec!["synthetic_quality_rejection".into()]
            },
            method: Some("native_synthetic"),
            transform: if i % 7 == 0 { None } else { Some(native) },
            rotation_deg: Some(native.theta_rad.to_degrees()),
            source_to_photometric_anchor: None,
            inlier_count: 10,
            inlier_rmse_px: Some(0.125),
            matches: vec![Match {
                source: 0,
                reference: 1,
                residual_px: 0.125,
            }],
            photometric_matches: vec![Match {
                source: 2,
                reference: 3,
                residual_px: 0.0625,
            }],
            match_reference: "native_anchor".into(),
            photometric_reference: Some("direct_master_matches"),
            matched_flux_ratio: Some(0.9),
            exposure_corrected_transparency: Some(0.8),
            photometric_scale: Some(1.1),
            background_offset_adu: Some(-0.),
            channel_offsets_adu: None,
            partial_field: None,
            normalized_noise_adu: Some(22.),
            final_weight: if source.accepted {
                0.5 + i as f64 / 100.
            } else {
                0.
            },
            footprint: if source.accepted {
                geometry::footprint(native, 2160, 3840)
            } else {
                vec![]
            },
            sky: None,
        });
        sources.push(source);
    }
    (rows, sources, target)
}

#[test]
fn parallel_external_rows_match_frozen_serial_bits_order_failures_and_photometry() {
    for threads in [1, 3, 16, 32] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        for scale in [0.33, 1., 2., 3.] {
            let bridge = Transform {
                scale_factor: scale,
                theta_rad: 0.37,
                tx_px: -17.125,
                ty_px: 64.75,
            };
            let (mut actual, sources, catalog) = fixture(bridge);
            let (mut expected, _, _) = fixture(bridge);
            let before = format!("{sources:?}");
            let cancel = AtomicBool::new(false);
            prior_serial_rows(
                &mut expected,
                &sources,
                &catalog,
                bridge,
                "external_geometry.fit",
                [2160, 3840],
                &cancel,
            )
            .unwrap();
            pool.install(|| {
                align_rows(
                    &mut actual,
                    &sources,
                    &catalog,
                    bridge,
                    "external_geometry.fit",
                    [2160, 3840],
                    &cancel,
                )
            })
            .unwrap();
            assert_eq!(
                format!("{actual:?}"),
                format!("{expected:?}"),
                "threads={threads}, scale={scale}"
            );
            for (a, e) in actual.iter().zip(&expected) {
                let bits = |t: Option<Transform>| {
                    t.map(|t| {
                        [
                            t.scale_factor.to_bits(),
                            t.theta_rad.to_bits(),
                            t.tx_px.to_bits(),
                            t.ty_px.to_bits(),
                        ]
                    })
                };
                assert_eq!(bits(a.transform), bits(e.transform));
                assert_eq!(
                    bits(a.source_to_photometric_anchor),
                    bits(e.source_to_photometric_anchor)
                );
                assert_eq!(
                    a.inlier_rmse_px.map(f64::to_bits),
                    e.inlier_rmse_px.map(f64::to_bits)
                );
                assert_eq!(
                    a.matches
                        .iter()
                        .map(|m| m.residual_px.to_bits())
                        .collect::<Vec<_>>(),
                    e.matches
                        .iter()
                        .map(|m| m.residual_px.to_bits())
                        .collect::<Vec<_>>()
                );
            }
            assert!(
                actual
                    .iter()
                    .any(|r| r.method == Some("external_similarity_direct") && r.accepted)
            );
            assert!(actual.iter().any(|r| {
                r.rejection_reasons
                    .iter()
                    .any(|s| s == "external_reference_registration_failed")
            }));
            assert_eq!(format!("{sources:?}"), before);
        }
    }
}

#[test]
fn parallel_external_rows_cancel_and_reject_malformed_lengths_without_publication() {
    let (mut rows, sources, catalog) = fixture(Transform::IDENTITY);
    let before = format!("{rows:?}");
    assert!(matches!(
        align_rows(
            &mut rows,
            &sources,
            &catalog,
            Transform::IDENTITY,
            "external.fit",
            [2160, 3840],
            &AtomicBool::new(true)
        ),
        Err(Error::Cancelled)
    ));
    assert_eq!(format!("{rows:?}"), before);
    assert!(
        align_rows(
            &mut rows,
            &sources[..sources.len() - 1],
            &catalog,
            Transform::IDENTITY,
            "external.fit",
            [2160, 3840],
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert_eq!(format!("{rows:?}"), before);
    align_rows(
        &mut [],
        &[],
        &catalog,
        Transform::IDENTITY,
        "external.fit",
        [2160, 3840],
        &AtomicBool::new(false),
    )
    .unwrap();
}

#[test]
fn a_sparse_wide_anchor_finds_its_pose_on_the_brighter_part_of_a_rich_template() {
    let star = |x: f64, y: f64| stars::Star {
        x_px: x,
        y_px: y,
        hfr_px: 1.7,
        moment_fwhm_px: 3.4,
        eccentricity: 0.1,
        flux_adu: 1e4,
        peak_adu: 2e4,
        snr: 90.,
    };
    // A rich template, brighter stars first, as a synthetic star field gives.
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    let catalog: Vec<_> = (0..4000)
        .map(|_| star(40. + next() * 4576., 40. + next() * 3440.))
        .collect();
    // A wider camera shows only the sixty brightest, at 1/1.6 of the scale.
    let truth = Transform {
        scale_factor: 1.6,
        theta_rad: 0.3,
        tx_px: 900.,
        ty_px: -400.,
    };
    let inverse = truth.inverse();
    let anchor: Vec<_> = catalog[..60]
        .iter()
        .map(|s| {
            let p = inverse.apply([s.x_px, s.y_px]);
            star(p[0], p[1])
        })
        .collect();
    let cancel = AtomicBool::new(false);
    // Neighbour triangles of the whole list do not meet the anchor's.
    assert!(
        geometry::solve_similarity(&anchor, &catalog, None, &cancel)
            .unwrap()
            .is_none()
    );
    let found = similarity_bridge(&anchor, &catalog, None, &cancel)
        .unwrap()
        .expect("the brighter part finds the pose");
    assert!((found.transform.scale_factor - 1.6).abs() < 1e-6);
    assert!((found.transform.theta_rad - 0.3).abs() < 1e-6);
    assert!(found.pairs.len() >= 50, "{} pairs", found.pairs.len());
}
