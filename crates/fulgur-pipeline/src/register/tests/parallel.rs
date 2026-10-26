use super::*;

pub(super) fn science(r: &RegistrationReport) -> String {
    let mut value = serde_json::to_value(r).unwrap();
    for key in [
        "phase_profile",
        "wall_seconds",
        "registration_seconds",
        "peak_rss_bytes",
    ] {
        value.as_object_mut().unwrap().remove(key);
    }
    // String equality also distinguishes signed zero in serialized float fields.
    serde_json::to_string(&value).unwrap()
}

fn input() -> InspectReport {
    let mut frames = vec![];
    for night in 0..3 {
        for i in 0..11 {
            let t = Transform {
                scale_factor: 1.,
                theta_rad: night as f64 * 1.5 + i as f64 * 0.012,
                tx_px: i as f64 * 13.,
                ty_px: i as f64 * -7.,
            };
            let mut f = planned_frame(
                frames.len(),
                &format!("2026-10-{:02}T22:{i:02}:00", 6 + night),
                if i % 2 == 0 { 10. } else { 20. },
                t,
                0.8 + i as f64 * 0.03,
            );
            if i % 4 == 0 || night == 2 {
                f.accepted = false;
                f.rejection_reasons.push("manual_use_off".into());
            }
            if i == 3 {
                f.stars.truncate(4);
            }
            frames.push(f);
        }
    }
    report(frames)
}

#[test]
fn parallel_walks_match_frozen_serial_planner_across_pools_sessions_and_references() {
    for threads in [1, 2, 3, 12, 24, 32] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        for diagnose_excluded in [false, true] {
            for selected in [None, Some(1), Some(21)] {
                let input = input();
                let snapshot = serde_json::to_string(&input).unwrap();
                let options = RegistrationOptions {
                    reference_strategy: ReferenceStrategy::ChronologicalMedian,
                    diagnose_excluded,
                    ..Default::default()
                };
                let cancel = AtomicBool::new(false);
                let expected = serial_oracle::serial_catalogs(
                    input.clone(),
                    0.8,
                    selected,
                    options,
                    &cancel,
                    Instant::now(),
                    None,
                )
                .unwrap();
                let seen = Mutex::new(vec![]);
                let observe = |n, total| seen.lock().unwrap().push((n, total));
                let progress = RegistrationProgress {
                    completed: Mutex::new(0),
                    total: 100,
                    observer: &observe,
                };
                let actual = pool
                    .install(|| {
                        register_catalogs_observed(
                            input.clone(),
                            0.8,
                            selected,
                            options,
                            &cancel,
                            Instant::now(),
                            Some(&progress),
                        )
                    })
                    .unwrap();
                assert_eq!(science(&expected), science(&actual));
                assert_eq!(serde_json::to_string(&input).unwrap(), snapshot);
                let seen = seen.into_inner().unwrap();
                assert!(seen.windows(2).all(|p| p[1].0 == p[0].0 + 1));
                let detail = &actual.phase_profile.native_fit_details;
                let expected_order: Vec<_> = expected
                    .phase_profile
                    .native_fit_details
                    .frame_fits
                    .iter()
                    .map(|f| (f.source_index, f.method))
                    .collect();
                assert_eq!(
                    detail
                        .frame_fits
                        .iter()
                        .map(|f| (f.source_index, f.method))
                        .collect::<Vec<_>>(),
                    expected_order
                );
                let sum = detail.session_bridge_geometry_seconds
                    + detail.direct_master_matches_seconds
                    + detail.photometry_seconds
                    + detail.bookkeeping_seconds
                    + detail
                        .frame_fits
                        .iter()
                        .map(|f| f.wall_seconds)
                        .sum::<f64>();
                let envelope = actual.phase_profile.native.phases[1].wall_seconds
                    - detail.walk_group_wall_seconds
                    + detail.walk_task_elapsed_seconds;
                assert!((sum - envelope).abs() < 1e-9);
                assert!(detail.bookkeeping_seconds >= 0.);
                assert!(detail.walk_task_elapsed_seconds <= 2. * detail.walk_group_wall_seconds);
            }
        }
    }
    let mut input = input();
    input.frames[0].frame.as_mut().unwrap().date_obs = None;
    let cancel = AtomicBool::new(false);
    let expected = serial_oracle::serial_catalogs(
        input.clone(),
        0.8,
        Some(1),
        RegistrationOptions::default(),
        &cancel,
        Instant::now(),
        None,
    )
    .unwrap();
    let actual = register_catalogs_observed(
        input,
        0.8,
        Some(1),
        RegistrationOptions::default(),
        &cancel,
        Instant::now(),
        None,
    )
    .unwrap();
    assert_eq!(science(&expected), science(&actual));
}

#[test]
fn cancellation_from_parallel_progress_joins_without_publishing_a_plan() {
    for threads in [1, 2, 24] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        let cancel = AtomicBool::new(false);
        let events = Mutex::new(vec![]);
        let observe = |n, _| {
            events.lock().unwrap().push(n);
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        };
        let progress = RegistrationProgress {
            completed: Mutex::new(0),
            total: 100,
            observer: &observe,
        };
        let result = pool.install(|| {
            register_catalogs_observed(
                input(),
                0.8,
                None,
                RegistrationOptions::default(),
                &cancel,
                Instant::now(),
                Some(&progress),
            )
        });
        assert!(matches!(result, Err(Error::Cancelled)));
        let snapshot = events.lock().unwrap().clone();
        assert!(!snapshot.is_empty());
        // Returning joins both branches; no callback may remain live afterwards.
        assert_eq!(*events.lock().unwrap(), snapshot);
    }
}

/// Release-only opt-in real-catalog comparison. No inspection or integration.
#[test]
fn stored_native_catalogs_exact_and_registration_replay_measurement() {
    let Some(path) = std::env::var_os("FULGURSTACK_NATIVE_REGISTRATION_BASELINE") else {
        return;
    };
    if cfg!(debug_assertions) {
        panic!("Use release for measured real-data replay");
    }
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let root = std::path::PathBuf::from(std::env::var_os("FULGURSTACK_TEST_FITS_DIR").unwrap());
    let original: serde_json::Value =
        serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(path).unwrap()))
            .unwrap();
    let stored = &original["registration"];
    assert!(stored["external_reference"].is_null());
    let catalog = stored["inspection"]["frames"].as_array().unwrap();
    let old_rows = stored["frames"].as_array().unwrap();
    assert_eq!(catalog.len(), old_rows.len());
    let fingerprints = || {
        catalog
            .iter()
            .map(|v| {
                let path = root.join(v["file_name"].as_str().unwrap());
                let mut file = std::fs::File::open(path).unwrap();
                let m = file.metadata().unwrap();
                let mut hash = Sha256::new();
                let mut bytes = vec![0u8; 1024 * 1024];
                loop {
                    let n = file.read(&mut bytes).unwrap();
                    if n == 0 {
                        break;
                    }
                    hash.update(&bytes[..n]);
                }
                (
                    m.len(),
                    m.modified().unwrap(),
                    format!("{:x}", hash.finalize()),
                )
            })
            .collect::<Vec<_>>()
    };
    let before = fingerprints();
    let frames = catalog
        .iter()
        .zip(old_rows)
        .map(|(v, old)| {
            let mut f = QualityFrame::empty(v["file_name"].as_str().unwrap().into());
            let header = fulgur_core::fits::read_header(&root.join(&f.file_name)).unwrap();
            assert_eq!(serde_json::to_value(&header.info).unwrap(), v["frame"]);
            f.frame = Some(header.info);
            f.accepted = old["quality_accepted"].as_bool().unwrap();
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
    let input = report(frames);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(24)
        .build()
        .unwrap();
    let cancel = AtomicBool::new(false);
    let mut samples = vec![];
    let mut reference = None;
    let mut latest_parallel = None;
    // One excluded warmup per path, then three rotated paired rounds.
    for (round, order) in [
        (0, [false, true]),
        (1, [false, true]),
        (2, [true, false]),
        (3, [false, true]),
    ] {
        for parallel in order {
            let input = input.clone();
            let cpu_before = fulgur_core::telemetry::process_cpu_time();
            let start = Instant::now();
            let r = pool
                .install(|| {
                    if parallel {
                        register_catalogs_observed(
                            input,
                            0.8,
                            None,
                            RegistrationOptions::default(),
                            &cancel,
                            start,
                            None,
                        )
                    } else {
                        serial_oracle::serial_catalogs(
                            input,
                            0.8,
                            None,
                            RegistrationOptions::default(),
                            &cancel,
                            start,
                            None,
                        )
                    }
                })
                .unwrap();
            let elapsed = start.elapsed().as_secs_f64();
            let cpu = fulgur_core::telemetry::process_cpu_time()
                .zip(cpu_before)
                .map(|(a, b)| a.since(b).total());
            assert_eq!(
                r.registration_success_count,
                stored["registration_success_count"].as_u64().unwrap() as usize
            );
            assert_eq!(
                r.accepted_count,
                stored["accepted_count"].as_u64().unwrap() as usize
            );
            if let Some(expected) = &reference {
                assert_eq!(science(&r), science(expected));
            }
            let mut max_displacement = 0_f64;
            let mut max_rmse = 0_f64;
            for (f, old) in r.frames.iter().zip(old_rows) {
                assert_eq!(
                    f.registration_success,
                    old["registration_success"].as_bool().unwrap()
                );
                assert_eq!(f.accepted, old["accepted"].as_bool().unwrap());
                if let Some(t) = f.transform {
                    let prev = Transform {
                        scale_factor: old["transform"]["scale_factor"].as_f64().unwrap(),
                        theta_rad: old["transform"]["theta_rad"].as_f64().unwrap(),
                        tx_px: old["transform"]["tx_px"].as_f64().unwrap(),
                        ty_px: old["transform"]["ty_px"].as_f64().unwrap(),
                    };
                    for p in [[0., 0.], [2159., 0.], [0., 3839.], [2159., 3839.]] {
                        let a = t.apply(p);
                        let b = prev.apply(p);
                        max_displacement = max_displacement.max((a[0] - b[0]).hypot(a[1] - b[1]));
                    }
                    max_rmse = max_rmse.max(f.inlier_rmse_px.unwrap());
                    for (current, name) in [
                        (f.photometric_scale.unwrap(), "photometric_scale"),
                        (f.final_weight, "final_weight"),
                        (f.background_offset_adu.unwrap(), "background_offset_adu"),
                        (
                            f.exposure_corrected_transparency.unwrap(),
                            "exposure_corrected_transparency",
                        ),
                    ] {
                        let previous = old[name].as_f64().unwrap();
                        assert!((current - previous).abs() <= 1e-10 * previous.abs().max(1.));
                    }
                }
            }
            assert!(max_displacement <= 1e-8);
            assert!(max_rmse <= 1.5);
            println!("registration replay round={round} parallel={parallel} seconds={elapsed:.6}");
            samples.push(serde_json::json!({"round":round,"warmup":round==0,"parallel":parallel,"wall_seconds":elapsed,"process_cpu_seconds":cpu,"max_corner_displacement_vs_stored_px":max_displacement,"max_inlier_rmse_px":max_rmse}));
            if parallel {
                latest_parallel = Some(r);
            } else {
                reference = Some(r);
            }
        }
    }
    let serial = reference.unwrap();
    let parallel = latest_parallel.unwrap();
    assert_eq!(science(&serial), science(&parallel));
    // Excluded-frame diagnostics must not change any selected solution.
    let diagnostics = pool
        .install(|| {
            register_catalogs_observed(
                input,
                0.8,
                None,
                RegistrationOptions {
                    diagnose_excluded: true,
                    ..Default::default()
                },
                &cancel,
                Instant::now(),
                None,
            )
        })
        .unwrap();
    for (a, b) in parallel
        .frames
        .iter()
        .zip(&diagnostics.frames)
        .filter(|(a, _)| a.quality_accepted)
    {
        assert_eq!(
            serde_json::to_string(a).unwrap(),
            serde_json::to_string(b).unwrap()
        );
    }
    assert_eq!(before, fingerprints());
    let evidence = serde_json::json!({"status":"PASS","scope":"registration-only stored native CFA catalog replay; no inspection or integration","original_frames":parallel.frame_count,"selected_frames":parallel.quality_accepted_count,"registered_selected":parallel.registration_success_count,"skipped":parallel.registration_skipped_count,"all_scientific_fields_exact_to_serial":true,"diagnostic_selected_rows_exact":true,"all_source_hashes_sizes_mtimes_unchanged":true,"peak_rss_bytes":fulgur_core::telemetry::peak_rss_bytes(),"samples":samples,"parallel_fit_details":parallel.phase_profile.native_fit_details});
    if let Some(dir) = std::env::var_os("FULGURSTACK_NATIVE_REGISTRATION_EVIDENCE") {
        std::fs::create_dir(&dir).unwrap();
        for (name, value) in [
            ("serial_plan.json", serde_json::to_value(&serial).unwrap()),
            (
                "parallel_plan.json",
                serde_json::to_value(&parallel).unwrap(),
            ),
            ("evidence.json", evidence),
        ] {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(std::path::Path::new(&dir).join(name))
                .unwrap();
            let mut writer = std::io::BufWriter::new(file);
            serde_json::to_writer_pretty(&mut writer, &value).unwrap();
            std::io::Write::flush(&mut writer).unwrap();
        }
    }
}
