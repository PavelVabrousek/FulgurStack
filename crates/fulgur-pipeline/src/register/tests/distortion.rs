use super::*;

#[test]
fn integration_refuses_an_unvalidated_accepted_opt_in_source_before_reading_pixels() {
    let mut plan = register_catalogs(
        report(vec![planned_frame(
            0,
            "2026-10-06T22:00:00",
            10.,
            Transform::IDENTITY,
            1.,
        )]),
        0.8,
        Some(0),
        &AtomicBool::new(false),
        Instant::now(),
    )
    .unwrap();
    plan.geometric_correction_enabled = true;
    assert!(matches!(
        crate::integrate::sources(Path::new("missing-input-root"), &plan),
        Err(Error::Invalid(
            "Accepted source has no confirmed whole-field registration."
        ))
    ));
}

/// Explicit owner-run regression: catalogs only, never a full image benchmark.
#[test]
fn stored_distorted_reference_whole_field_replay() {
    let Some(path) = std::env::var_os("FULGURSTACK_DISTORTION_REPLAY") else {
        return;
    };
    if cfg!(debug_assertions) {
        panic!("Use --release for real-data replay");
    }
    let root = std::path::PathBuf::from(
        std::env::var_os("FULGURSTACK_DISTORTION_INPUT_DIR")
            .or_else(|| std::env::var_os("FULGURSTACK_TEST_FITS_DIR"))
            .unwrap(),
    );
    let value: serde_json::Value =
        serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(path).unwrap()))
            .unwrap();
    let old = &value["registration"];
    let old_rows = old["frames"].as_array().unwrap();
    let frames: Vec<_> = old["inspection"]["frames"]
        .as_array()
        .unwrap()
        .iter()
        .zip(old_rows)
        .map(|(v, previous)| {
            let mut f = QualityFrame::empty(v["file_name"].as_str().unwrap().into());
            let header = fulgur_core::fits::read_header(&root.join(&f.file_name)).unwrap();
            assert_eq!(serde_json::to_value(&header.info).unwrap(), v["frame"]);
            f.frame = Some(header.info);
            f.accepted = previous["quality_accepted"].as_bool().unwrap();
            f.background_adu = v["background_adu"].as_f64();
            f.noise_adu = v["noise_adu"].as_f64();
            f.hfr_px = v["hfr_px"].as_f64();
            f.rejection_reasons = v["rejection_reasons"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r.as_str().unwrap().into())
                .collect();
            f.stars = v["stars"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| crate::stars::Star {
                    x_px: s["x_px"].as_f64().unwrap(),
                    y_px: s["y_px"].as_f64().unwrap(),
                    hfr_px: s["hfr_px"].as_f64().unwrap(),
                    moment_fwhm_px: s["moment_fwhm_px"].as_f64().unwrap(),
                    eccentricity: s["eccentricity"].as_f64().unwrap(),
                    flux_adu: s["flux_adu"].as_f64().unwrap(),
                    peak_adu: s["peak_adu"].as_f64().unwrap(),
                    snr: s["snr"].as_f64().unwrap(),
                })
                .collect();
            f
        })
        .collect();
    let anchor = frames
        .iter()
        .position(|f| f.file_name == old["reference_anchor"].as_str().unwrap())
        .unwrap();
    let input = report(frames);
    let cancel = AtomicBool::new(false);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(24)
        .build()
        .unwrap();
    let start = Instant::now();
    let mut plan = pool
        .install(|| {
            register_catalogs_observed(
                input,
                0.8,
                Some(anchor),
                RegistrationOptions {
                    geometric_correction: true,
                    ..Default::default()
                },
                &cancel,
                start,
                None,
            )
        })
        .unwrap();
    println!(
        "Validated native replay: {}/{}",
        plan.accepted_count, plan.quality_accepted_count
    );
    pool.install(|| {
        crate::reference::align(
            &mut plan,
            Path::new(old["external_reference"]["path"].as_str().unwrap()),
            &cancel,
        )
    })
    .unwrap();
    assert_eq!(plan.frame_count, old_rows.len());
    assert_eq!(
        plan.quality_accepted_count,
        old["quality_accepted_count"].as_u64().unwrap() as usize
    );
    let mut recovered = 0;
    let mut unsafe_linear = 0;
    for (row, previous) in plan.frames.iter().zip(old_rows) {
        assert_eq!(
            row.quality_accepted,
            previous["quality_accepted"].as_bool().unwrap()
        );
        if row.accepted {
            let validation = row
                .geometric_correction
                .as_ref()
                .unwrap()
                .registration_validation
                .as_ref()
                .unwrap();
            assert!(
                validation.accepted
                    && validation.rmse_px.unwrap() <= 1.
                    && validation.maximum_error_px.unwrap() <= 3.
            );
            if row.method == Some("external_anchor_cubic_validated") {
                recovered += 1;
            }
            if row.geometric_correction.as_ref().unwrap().model.is_none() {
                unsafe_linear += 1;
            }
        }
    }
    assert_eq!(
        unsafe_linear, 0,
        "a 3% distorted target must not accept incompatible linear rows"
    );
    assert!(recovered > 0);
    let elapsed = start.elapsed().as_secs_f64();
    let evidence = serde_json::json!({"status":"PASS","scope":"stored owner-run catalogs; registration only; no full integration benchmark",
        "originals":plan.frame_count,"selected":plan.quality_accepted_count,"accepted":plan.accepted_count,
        "geometric_failures":plan.quality_accepted_count-plan.accepted_count,"recovered_anchor_compositions":recovered,
        "unsafe_linear":unsafe_linear,"wall_seconds":elapsed,"peak_rss_bytes":fulgur_core::telemetry::peak_rss_bytes()});
    println!("{evidence}");
    if let Some(dir) = std::env::var_os("FULGURSTACK_DISTORTION_EVIDENCE") {
        std::fs::create_dir(&dir).unwrap();
        for (name, v) in [
            ("plan.json", serde_json::to_value(&plan).unwrap()),
            ("evidence.json", evidence),
        ] {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(Path::new(&dir).join(name))
                .unwrap();
            serde_json::to_writer_pretty(std::io::BufWriter::new(file), &v).unwrap();
        }
    }
}

#[test]
fn opt_in_native_correction_recovers_radial_geometry_and_unchanged_fields_skip() {
    let target = planned_frame(0, "2026-10-06T22:00:00", 10., Transform::IDENTITY, 1.);
    let mut source = planned_frame(1, "2026-10-06T22:01:00", 10., Transform::IDENTITY, 1.);
    let mut model = crate::distortion::Warp {
        base: Transform::IDENTITY,
        center: [1079.5, 1919.5],
        normalization: [1079.5, 1919.5],
        coefficients: [[0.; 10]; 2],
    };
    let r2 = model.center[0].powi(2) + model.center[1].powi(2);
    model.coefficients[0][6] = 0.03 * model.center[0].powi(3) / r2;
    model.coefficients[0][8] = 0.03 * model.center[0] * model.center[1].powi(2) / r2;
    model.coefficients[1][7] = 0.03 * model.center[1] * model.center[0].powi(2) / r2;
    model.coefficients[1][9] = 0.03 * model.center[1].powi(3) / r2;
    for s in &mut source.stars {
        let p = model.inverse([s.x_px, s.y_px]).unwrap();
        s.x_px = p[0];
        s.y_px = p[1];
    }
    let corrected = register_catalogs_observed(
        report(vec![target.clone(), source]),
        0.8,
        Some(0),
        RegistrationOptions {
            geometric_correction: true,
            ..Default::default()
        },
        &AtomicBool::new(false),
        Instant::now(),
        None,
    )
    .unwrap();
    assert!(corrected.frames[1].accepted);
    assert_eq!(corrected.frames[1].method, Some("native_cubic_validated"));
    assert!(corrected.frames[1].inlier_rmse_px.unwrap() < 1e-5);
    assert!(
        corrected.frames[1]
            .geometric_correction
            .as_ref()
            .unwrap()
            .model
            .is_some()
    );
    let clean = planned_frame(1, "2026-10-06T22:01:00", 10., Transform::IDENTITY, 1.);
    let input = report(vec![target, clean]);
    let ordinary = register_catalogs(
        input.clone(),
        0.8,
        Some(0),
        &AtomicBool::new(false),
        Instant::now(),
    )
    .unwrap();
    let enabled = register_catalogs_observed(
        input,
        0.8,
        Some(0),
        RegistrationOptions {
            geometric_correction: true,
            ..Default::default()
        },
        &AtomicBool::new(false),
        Instant::now(),
        None,
    )
    .unwrap();
    for (a, b) in ordinary.frames.iter().zip(&enabled.frames) {
        assert!(b.geometric_correction.as_ref().unwrap().model.is_none());
        let a = serde_json::to_value(a).unwrap();
        let mut b = serde_json::to_value(b).unwrap();
        b.as_object_mut().unwrap().remove("geometric_correction");
        assert_eq!(a, b);
    }
}

#[test]
fn optional_validation_progress_counts_completed_rows_once_and_finishes_exactly() {
    let frames = vec![
        planned_frame(0, "2026-10-06T22:00:00", 10., Transform::IDENTITY, 1.),
        planned_frame(1, "2026-10-06T22:01:00", 10., Transform::IDENTITY, 1.),
    ];
    for geometric_correction in [false, true] {
        let events = Mutex::new(vec![]);
        let (plan, _) = register_inspected_with_options(
            report(frames.clone()),
            ".".into(),
            0.8,
            None,
            RegistrationOptions {
                geometric_correction,
                ..Default::default()
            },
            &AtomicBool::new(false),
            Instant::now(),
            Some(&|n, total| events.lock().unwrap().push((n, total))),
        )
        .unwrap();
        assert_eq!(plan.accepted_count, 2);
        // Rows, finalization, validation rows and the second-pass decision.
        let expected = if geometric_correction { 6 } else { 4 };
        let events = events.into_inner().unwrap();
        assert_eq!(
            events,
            (0..=expected).map(|n| (n, expected)).collect::<Vec<_>>()
        );
    }
}

/// A plate-scale change (refocus, temperature) defeats the rigid pose at the
/// field edges; the validated similarity fallback must recover it, and a frame
/// at another image scale that rigid matching cannot register at all must be
/// recovered by the external-reference similarity search (30..333%).
#[test]
fn rigid_failures_recover_through_validated_similarity_within_supported_scale() {
    let target = planned_frame(0, "2026-10-06T22:00:00", 10., Transform::IDENTITY, 1.);
    let center = [1079.5, 1919.5];
    let about_center = |scale: f64, theta: f64| {
        let (s, c) = theta.sin_cos();
        Transform {
            scale_factor: scale,
            theta_rad: theta,
            tx_px: center[0] - scale * (c * center[0] - s * center[1]),
            ty_px: center[1] - scale * (s * center[0] + c * center[1]),
        }
    };
    for (truth, method) in [
        (about_center(1.0012, 0.002), "native_similarity_validated"),
        (about_center(2.0, 0.5), "native_similarity_recovered"),
        (about_center(0.5, -0.3), "native_similarity_recovered"),
    ] {
        let source = planned_frame(1, "2026-10-06T22:01:00", 10., truth, 1.);
        let plan = register_catalogs_observed(
            report(vec![target.clone(), source]),
            0.8,
            Some(0),
            RegistrationOptions {
                geometric_correction: true,
                ..Default::default()
            },
            &AtomicBool::new(false),
            Instant::now(),
            None,
        )
        .unwrap();
        let row = &plan.frames[1];
        assert!(row.accepted, "{method}: {:?}", row.rejection_reasons);
        assert_eq!(row.method, Some(method));
        let t = row.transform.unwrap();
        assert!((t.scale_factor / truth.scale_factor - 1.).abs() < 1e-6);
        assert!((t.theta_rad - truth.theta_rad).abs() < 1e-6);
        let validation = row
            .geometric_correction
            .as_ref()
            .unwrap()
            .registration_validation
            .as_ref()
            .unwrap();
        assert!(validation.accepted && validation.maximum_error_px.unwrap() < 1e-3);
        assert!(!row.footprint.is_empty() && row.final_weight > 0.);
        // The scale-change case proves the rigid pose was refused first.
        let initial = row
            .geometric_correction
            .as_ref()
            .unwrap()
            .initial_correction_reason;
        assert_eq!(
            initial.is_some(),
            method == "native_similarity_validated",
            "{method}: {initial:?}"
        );
    }
    // Outside 30..333% nothing is accepted.
    let source = planned_frame(1, "2026-10-06T22:01:00", 10., about_center(4.0, 0.), 1.);
    let plan = register_catalogs_observed(
        report(vec![target, source]),
        0.8,
        Some(0),
        RegistrationOptions {
            geometric_correction: true,
            ..Default::default()
        },
        &AtomicBool::new(false),
        Instant::now(),
        None,
    );
    assert!(plan.is_err() || !plan.unwrap().frames[1].accepted);
}

/// Dense fields: two truncated catalogs share only part of their stars. The
/// extended reference catalog restores coverage; it never relaxes accuracy.
#[test]
fn extended_witness_catalog_counts_coverage_without_weakening_accuracy() {
    let source = planned_frame(1, "2026-10-06T22:01:00", 10., Transform::IDENTITY, 1.);
    let mut target = planned_frame(0, "2026-10-06T22:00:00", 10., Transform::IDENTITY, 1.);
    let all = target.stars.clone();
    // The reference's own catalog lost a third of the stars (other picks).
    target.stars = all
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 3 != 1)
        .map(|(_, s)| s.clone())
        .collect();
    let witnesses = Witnesses {
        positions: all.clone(),
        precise: all.clone(),
        deep_precise: vec![],
    };
    let run = |extended: Option<&Witnesses>, source: &QualityFrame| {
        let mut row = empty(source, 0, &target.file_name);
        row.registration_attempted = true;
        row.registration_success = true;
        row.accepted = true;
        row.transform = Some(Transform::IDENTITY);
        row.final_weight = 1.;
        apply_native_correction(
            &mut row,
            source,
            &target,
            extended,
            None,
            false,
            crate::distortion::AccuracyLimits::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        row
    };
    // The reference shows only 60 of the frame's 90 stars; the share counts
    // the weaker side, so all of them matched (ADR-121). Before, 60 of 90
    // failed the 80 % rule here and only the extended catalog passed.
    let validation = |row: &RegisteredFrame| {
        let v = row
            .geometric_correction
            .as_ref()
            .unwrap()
            .registration_validation
            .clone()
            .unwrap();
        (v.fixed_stars, v.covered_stars, v.expected_stars)
    };
    let ordinary = run(None, &source);
    assert!(ordinary.accepted, "{:?}", ordinary.rejection_reasons);
    assert_eq!(validation(&ordinary), (60, None, 60));
    let extended = run(Some(&witnesses), &source);
    assert!(extended.accepted, "{:?}", extended.rejection_reasons);
    assert_eq!(validation(&extended), (60, Some(90), 60));
    // A wrong starting pose is either corrected or refused: a frame is only
    // accepted on a map that puts its stars on the catalog.
    let shifted = planned_frame(
        1,
        "2026-10-06T22:01:00",
        10.,
        Transform {
            tx_px: 40.,
            ..Transform::IDENTITY
        },
        1.,
    );
    let moved = run(Some(&witnesses), &shifted);
    if moved.accepted {
        let t = moved.transform.unwrap();
        let on_catalog = shifted
            .stars
            .iter()
            .filter(|s| {
                let p = t.apply([s.x_px, s.y_px]);
                all.iter()
                    .any(|r| (p[0] - r.x_px).hypot(p[1] - r.y_px) <= 1.)
            })
            .count();
        assert!(on_catalog * 5 >= shifted.stars.len() * 4, "{on_catalog}");
    }
    // Stars of another field fit no map and stay refused.
    let mut other = source.clone();
    for (i, s) in other.stars.iter_mut().enumerate() {
        s.x_px += 4. + (i * 37 % 7) as f64;
        s.y_px -= 4. + (i * 53 % 7) as f64;
    }
    assert!(!run(Some(&witnesses), &other).accepted);
}

/// Experimental partial frames (ADR-109): a tile joins only on accurate stars
/// around it; too few stars never count as accurate.
#[test]
fn a_partial_frame_joins_only_tiles_with_accurate_stars_around_them() {
    // A grid of witnesses: accurate on the left half, 2 px off on the right.
    let mut stars = vec![];
    for y in (0..4000).step_by(200) {
        for x in (0..6000).step_by(200) {
            stars.push([x as f64, y as f64, if x < 3000 { 0.3 } else { 2. }]);
        }
    }
    let field = PartialField::new(
        stars,
        "similarity",
        crate::distortion::AccuracyLimits::default(),
    );
    assert!(field.admits([0, 1000], [512, 512]), "accurate side");
    assert!(!field.admits([5000, 1000], [512, 512]), "2 px RMS fails");
    assert!(
        !field.admits([20000, 1000], [512, 512]),
        "no stars, no proof"
    );
    let single = PartialField::new(
        vec![[100., 100., 0.1]; 4],
        "rigid",
        crate::distortion::AccuracyLimits::default(),
    );
    assert!(!single.admits([0, 0], [512, 512]), "four stars are too few");
    let worst = PartialField::new(
        (0..20)
            .map(|i| [100. + i as f64, 100., if i == 0 { 3.5 } else { 0. }])
            .collect(),
        "rigid",
        crate::distortion::AccuracyLimits::default(),
    );
    assert!(!worst.admits([0, 0], [512, 512]), "one star beyond 3 px");
    use std::sync::atomic::Ordering::Relaxed;
    assert_eq!(field.tiles_used.load(Relaxed), 1);
    assert_eq!(field.tiles_refused.load(Relaxed), 2);
}

/// The report says where a partial frame stayed out, why, and how accurate
/// the fit it stacks with really is (ADR-116).
#[test]
fn a_partial_frame_reports_its_refused_tiles_and_the_fit_it_uses() {
    let mut stars: Vec<_> = (0..40).map(|i| [4000. + f64::from(i), 100., 2.]).collect();
    stars.extend((0..20).map(|i| [100. + f64::from(i), 100., if i == 0 { 3.5 } else { 0. }]));
    let field = PartialField::new(
        stars,
        "similarity",
        crate::distortion::AccuracyLimits::default(),
    );
    assert_eq!(field.witnesses, 60);
    assert!((field.fit_maximum_error_px - 3.5).abs() < 1e-12);
    let squares = 40. * 4. + 3.5 * 3.5;
    assert!((field.fit_rmse_px - (squares / 60_f64).sqrt()).abs() < 1e-12);
    // Tiles are tested in any order; the report is sorted by row, then column.
    assert!(!field.admits([20000, 2000], [512, 400]));
    assert!(!field.admits([4000, 0], [512, 512]));
    assert!(!field.admits([0, 0], [512, 512]));
    let report = serde_json::to_value(&field).unwrap();
    assert_eq!(report["fit"], "similarity");
    assert_eq!(report["tiles_refused"], 3);
    assert!(report.get("stars").is_none());
    let tiles = report["refused_tiles"].as_array().unwrap();
    let got: Vec<_> = tiles
        .iter()
        .map(|t| (t["x_px"].as_i64().unwrap(), t["reason"].as_str().unwrap()))
        .collect();
    assert_eq!(
        got,
        [
            (0, "star_above_3px"),
            (4000, "rms_above_1px"),
            (20000, "too_few_stars"),
        ]
    );
    assert_eq!(tiles[0]["stars"], 20);
    assert_eq!(tiles[0]["maximum_error_px"], 3.5);
    assert_eq!(tiles[1]["stars"], 40);
    assert_eq!(tiles[1]["rms_error_px"], 2.);
    assert_eq!(tiles[2]["y_px"], 2000);
    assert_eq!(tiles[2]["width_px"], 512);
    assert_eq!(tiles[2]["height_px"], 400);
    assert_eq!(tiles[2]["stars"], 0);
    assert!(tiles[2]["rms_error_px"].is_null() && tiles[2]["maximum_error_px"].is_null());
}

/// Dense fields (ADR-085): a 12 px witness window can pair a star whose
/// counterpart the other truncated catalog lacks with a neighbour. The
/// anchor's precise catalog decides instead, with unchanged accuracy limits.
#[test]
fn a_misidentified_witness_is_rechecked_against_the_precise_catalog() {
    let source = planned_frame(1, "2026-10-06T22:01:00", 10., Transform::IDENTITY, 1.);
    let mut target = planned_frame(0, "2026-10-06T22:00:00", 10., Transform::IDENTITY, 1.);
    let all = target.stars.clone();
    // The reference catalog holds a neighbour 8 px from one source star.
    target.stars[7].x_px += 8.;
    let witnesses = Witnesses {
        positions: all.clone(),
        precise: all.clone(),
        deep_precise: vec![],
    };
    let run = |extended: Option<&Witnesses>, source: &QualityFrame| {
        let mut row = empty(source, 0, &target.file_name);
        row.registration_attempted = true;
        row.registration_success = true;
        row.accepted = true;
        row.transform = Some(Transform::IDENTITY);
        row.final_weight = 1.;
        apply_native_correction(
            &mut row,
            source,
            &target,
            extended,
            None,
            false,
            crate::distortion::AccuracyLimits::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
        row
    };
    let validation = |row: &RegisteredFrame| {
        row.geometric_correction
            .as_ref()
            .unwrap()
            .registration_validation
            .clone()
            .unwrap()
    };
    let ordinary = run(None, &source);
    assert!(!ordinary.accepted);
    assert_eq!(
        validation(&ordinary).reason,
        "absolute_field_accuracy_failed"
    );
    let rechecked = run(Some(&witnesses), &source);
    assert!(rechecked.accepted, "{:?}", rechecked.rejection_reasons);
    let v = validation(&rechecked);
    assert!(v.extended_witnesses && v.maximum_error_px.unwrap() < 1e-3);
    // A pose 2 px off never passes as it is: the limits refuse it and the
    // ADR-074 refit must recover the true pose from the witnesses.
    let shifted = planned_frame(
        1,
        "2026-10-06T22:01:00",
        10.,
        Transform {
            tx_px: 2.,
            ..Transform::IDENTITY
        },
        1.,
    );
    let refit = run(Some(&witnesses), &shifted);
    let initial = refit
        .geometric_correction
        .as_ref()
        .unwrap()
        .initial_correction_reason;
    assert!(initial.is_some(), "identity pose refused first");
    if refit.accepted {
        let t = refit.transform.unwrap();
        assert!((t.tx_px.abs() - 2.).abs() < 0.05, "{t:?}");
        assert!(validation(&refit).rmse_px.unwrap() < 0.05);
    }
}
