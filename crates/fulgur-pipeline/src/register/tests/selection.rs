use super::*;

#[test]
fn diagnostics_cannot_change_selected_fits_weights_sessions_or_canvases() {
    let mut frames: Vec<_> = (0..9)
        .map(|i| {
            planned_frame(
                i,
                &format!("2026-10-06T22:0{i}:00"),
                10.,
                Transform {
                    scale_factor: 1.,
                    theta_rad: i as f64 * 0.003,
                    tx_px: i as f64,
                    ty_px: -(i as f64),
                },
                1. + i as f64 * 0.01,
            )
        })
        .collect();
    for i in [1, 3, 5, 7] {
        frames[i].accepted = false;
        frames[i].rejection_reasons.push("manual_use_off".into());
    }
    // An excluded-only night must not require or invent an accepted anchor.
    let mut excluded = planned_frame(9, "2026-10-07T22:00:00", 10., Transform::IDENTITY, 1.);
    excluded.accepted = false;
    frames.push(excluded);
    let input = report(frames);
    let mut plans = vec![];
    for diagnose_excluded in [false, true] {
        let seen = Mutex::new(vec![]);
        let (r, _) = register_inspected_with_options(
            input.clone(),
            ".".into(),
            0.8,
            None,
            RegistrationOptions {
                diagnose_excluded,
                ..Default::default()
            },
            &AtomicBool::new(false),
            Instant::now(),
            Some(&|n, total| seen.lock().unwrap().push((n, total))),
        )
        .unwrap();
        let events = seen.into_inner().unwrap();
        assert_eq!(events[0], (0, if diagnose_excluded { 11 } else { 6 }));
        assert_eq!(events.last().unwrap().0, events.last().unwrap().1);
        assert!(events.windows(2).all(|p| p[0].0 < p[1].0));
        assert_eq!(r.frame_count, 10);
        assert_eq!(r.accepted_count, 5);
        assert_eq!(
            r.registration_success_count,
            if diagnose_excluded { 9 } else { 5 }
        );
        assert_eq!(r.registration_attempted_count, r.registration_success_count);
        assert_eq!(
            r.registration_skipped_count,
            if diagnose_excluded { 1 } else { 5 }
        );
        for f in r.frames.iter().filter(|f| !f.quality_accepted) {
            assert!(!f.accepted && f.final_weight == 0. && f.footprint.is_empty());
            if !diagnose_excluded {
                assert!(!f.registration_attempted && f.transform.is_none() && f.matches.is_empty());
                assert!(
                    !f.rejection_reasons
                        .iter()
                        .any(|r| r == "registration_failed")
                );
            }
        }
        plans.push(r);
    }
    for (a, b) in plans[0]
        .frames
        .iter()
        .zip(&plans[1].frames)
        .filter(|(a, _)| a.quality_accepted)
    {
        assert_eq!(
            serde_json::to_value(a).unwrap(),
            serde_json::to_value(b).unwrap()
        );
        assert!(a.inlier_rmse_px.unwrap() < 1e-8);
    }
    for field in [
        "reference_anchor",
        "sessions",
        "common_intersection",
        "coverage_optimized",
        "accepted_exposure_seconds",
    ] {
        assert_eq!(
            serde_json::to_value(&plans[0]).unwrap()[field],
            serde_json::to_value(&plans[1]).unwrap()[field]
        );
    }
}

#[test]
fn scheduled_failure_and_cancellation_are_not_reported_as_exclusion() {
    let mut frames: Vec<_> = (0..3)
        .map(|i| {
            planned_frame(
                i,
                &format!("2026-10-06T22:0{i}:00"),
                10.,
                Transform::IDENTITY,
                1.,
            )
        })
        .collect();
    frames[0].stars.truncate(4);
    let r = register_catalogs(
        report(frames.clone()),
        0.8,
        None,
        &AtomicBool::new(false),
        Instant::now(),
    )
    .unwrap();
    assert_eq!(r.registration_attempted_count, 3);
    assert_eq!(r.registration_success_count, 2);
    assert_eq!(r.registration_skipped_count, 0);
    assert!(
        r.frames[0]
            .rejection_reasons
            .iter()
            .any(|r| r == "registration_failed")
    );
    assert!(matches!(
        register_catalogs(
            report(frames),
            0.8,
            None,
            &AtomicBool::new(true),
            Instant::now()
        ),
        Err(Error::Cancelled)
    ));
}

/// Opt-in replay of the owner's stored full-run CFA catalogs, without another
/// inspection or integration. Read real headers and the external FITS read-only.
/// Gates fixed before measurement: identical success/membership, <=0.05 reference
/// pixel corner displacement, <=1.5 px residual, <=1e-10 relative photometry/weight
/// changes; diagnostics must preserve every selected row exactly.
#[test]
fn stored_registration_selection_accuracy_gate() {
    let Some(path) = std::env::var_os("FULGURSTACK_REGISTRATION_BASELINE") else {
        return;
    };
    let root = std::path::PathBuf::from(
        std::env::var_os("FULGURSTACK_TEST_FITS_DIR").expect("Source root required"),
    );
    let original: serde_json::Value =
        serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(path).unwrap()))
            .unwrap();
    let baseline = &original["registration"];
    let old = baseline["frames"].as_array().unwrap();
    let catalog = baseline["inspection"]["frames"].as_array().unwrap();
    assert_eq!(old.len(), catalog.len());
    let metadata: Vec<_> = catalog
        .iter()
        .map(|v| {
            let p = root.join(v["file_name"].as_str().unwrap());
            let m = std::fs::metadata(&p).unwrap();
            (p, m.len(), m.modified().unwrap())
        })
        .collect();
    let frames = catalog
        .iter()
        .zip(old)
        .map(|(v, old)| {
            let mut f = QualityFrame::empty(v["file_name"].as_str().unwrap().into());
            let header = fulgur_core::fits::read_header(&root.join(&f.file_name)).unwrap();
            assert_eq!(serde_json::to_value(&header.info).unwrap(), v["frame"]);
            f.frame = Some(header.info);
            f.accepted = old["quality_accepted"].as_bool().unwrap();
            f.background_adu = v["background_adu"].as_f64();
            f.noise_adu = v["noise_adu"].as_f64();
            f.hfr_px = v["hfr_px"].as_f64();
            f.rejection_reasons = old["rejection_reasons"]
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
    let input = report(frames);
    let external =
        std::path::PathBuf::from(baseline["external_reference"]["path"].as_str().unwrap());
    let cancel = AtomicBool::new(false);
    let mut plans = vec![];
    for diagnose_excluded in [false, true] {
        let seen = Mutex::new(vec![]);
        let (r, _) = register_inspected_with_options(
            input.clone(),
            root.clone(),
            0.8,
            Some(&external),
            RegistrationOptions {
                diagnose_excluded,
                ..Default::default()
            },
            &cancel,
            Instant::now(),
            Some(&|n, total| seen.lock().unwrap().push((n, total))),
        )
        .unwrap();
        let events = seen.into_inner().unwrap();
        assert_eq!(events.last().unwrap().0, events.last().unwrap().1);
        assert!(
            events
                .windows(2)
                .all(|p| p[0].0 < p[1].0 && p[0].1 == p[1].1)
        );
        assert_eq!(
            r.accepted_count,
            baseline["accepted_count"].as_u64().unwrap() as usize
        );
        plans.push(r);
    }
    let mut max_displacement = 0_f64;
    let mut max_native_displacement = 0_f64;
    let mut max_rmse = 0_f64;
    let mut max_rmse_increase = 0_f64;
    let mut max_relative = 0_f64;
    let mut exact_rows = 0;
    for ((f, diag), old) in plans[0].frames.iter().zip(&plans[1].frames).zip(old) {
        assert_eq!(f.accepted, old["accepted"].as_bool().unwrap());
        if !f.quality_accepted {
            continue;
        }
        assert_eq!(
            serde_json::to_value(f).unwrap(),
            serde_json::to_value(diag).unwrap()
        );
        assert_eq!(
            f.registration_success,
            old["registration_success"].as_bool().unwrap()
        );
        let t = f.transform.unwrap();
        let v = &old["transform"];
        let prev = Transform {
            scale_factor: v["scale_factor"].as_f64().unwrap(),
            theta_rad: v["theta_rad"].as_f64().unwrap(),
            tx_px: v["tx_px"].as_f64().unwrap(),
            ty_px: v["ty_px"].as_f64().unwrap(),
        };
        for p in [[0., 0.], [2159., 0.], [0., 3839.], [2159., 3839.]] {
            let a = t.apply(p);
            let b = prev.apply(p);
            max_displacement = max_displacement.max((a[0] - b[0]).hypot(a[1] - b[1]));
        }
        let native = f.source_to_photometric_anchor.unwrap();
        let v = &old["source_to_photometric_anchor"];
        let previous_native = Transform {
            scale_factor: v["scale_factor"].as_f64().unwrap(),
            theta_rad: v["theta_rad"].as_f64().unwrap(),
            tx_px: v["tx_px"].as_f64().unwrap(),
            ty_px: v["ty_px"].as_f64().unwrap(),
        };
        for p in [[0., 0.], [2159., 0.], [0., 3839.], [2159., 3839.]] {
            let a = native.apply(p);
            let b = previous_native.apply(p);
            max_native_displacement = max_native_displacement.max((a[0] - b[0]).hypot(a[1] - b[1]));
        }
        max_rmse = max_rmse.max(f.inlier_rmse_px.unwrap());
        max_rmse_increase = max_rmse_increase
            .max(f.inlier_rmse_px.unwrap() - old["inlier_rmse_px"].as_f64().unwrap());
        let mut current = serde_json::to_value(f).unwrap();
        current
            .as_object_mut()
            .unwrap()
            .remove("registration_attempted");
        if &current == old {
            exact_rows += 1;
        }
        for field in [
            "photometric_scale",
            "matched_flux_ratio",
            "exposure_corrected_transparency",
            "background_offset_adu",
            "normalized_noise_adu",
            "final_weight",
        ] {
            let a = current[field].as_f64().unwrap();
            let b = old[field].as_f64().unwrap();
            max_relative = max_relative.max((a - b).abs() / b.abs().max(1.));
        }
    }
    assert!(max_displacement <= 0.05, "displacement={max_displacement}");
    assert!(
        max_native_displacement <= 0.05,
        "native displacement={max_native_displacement}"
    );
    assert!(
        max_rmse_increase <= 0.001,
        "RMSE increase={max_rmse_increase}"
    );
    assert!(max_rmse <= 1.5, "rmse={max_rmse}");
    assert!(max_relative <= 1e-10, "relative={max_relative}");
    for (p, len, modified) in metadata {
        let m = std::fs::metadata(p).unwrap();
        assert_eq!((m.len(), m.modified().unwrap()), (len, modified));
    }
    let evidence = serde_json::json!({"gate":"PASS", "scope":"stored full CFA catalogs; no reinspection, integration or performance benchmark", "original_frames":plans[0].frame_count,"selected_frames":plans[0].quality_accepted_count,"registered_selected":plans[0].registration_success_count,"skipped":plans[0].registration_skipped_count,"diagnostic_registered":plans[1].registration_success_count,"selected_rows_exact_between_modes":true,"selected_rows_exact_to_stored_report":exact_rows,"max_corner_displacement_reference_px":max_displacement,"max_native_corner_displacement_px":max_native_displacement,"max_inlier_rmse_reference_px":max_rmse,"max_inlier_rmse_increase_reference_px":max_rmse_increase,"max_normalization_relative_difference":max_relative,"peak_rss_bytes":fulgur_core::telemetry::peak_rss_bytes(),"source_metadata_unchanged":true});
    println!("{evidence}");
    if let Some(path) = std::env::var_os("FULGURSTACK_REGISTRATION_EVIDENCE") {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        serde_json::to_writer_pretty(file, &evidence).unwrap();
    }
}
