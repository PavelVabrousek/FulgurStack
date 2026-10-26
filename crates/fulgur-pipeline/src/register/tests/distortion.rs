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
        let expected = if geometric_correction { 5 } else { 3 };
        let events = events.into_inner().unwrap();
        assert_eq!(
            events,
            (0..=expected).map(|n| (n, expected)).collect::<Vec<_>>()
        );
    }
}
