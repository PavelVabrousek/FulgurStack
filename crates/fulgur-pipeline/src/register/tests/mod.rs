use super::*;
mod auto_selection;
mod distortion;
mod parallel;
mod selection;
mod serial_oracle;
mod set_photometry;
mod sky;

#[test]
fn mixed_exposure_transparency_scale_offset_and_saturation() {
    let mut a = crate::inspect::QualityFrame::empty("synthetic.fit".into());
    let mut b = crate::inspect::QualityFrame::empty("synthetic.fit".into());
    a.stars = (0..20)
        .map(|i| crate::stars::Star {
            x_px: i as f64 * 30.,
            y_px: (i * i % 7) as f64 * 30.,
            hfr_px: 1.7,
            moment_fwhm_px: 3.4,
            eccentricity: 0.2,
            flux_adu: 20000. + i as f64 * 100.,
            peak_adu: 20000.,
            snr: 100.,
        })
        .collect();
    b.stars = a.stars.clone();
    for s in &mut a.stars {
        s.flux_adu *= 1.5;
    } // 20s exposure, 0.75 transparency relative to 10s reference.
    let pairs: Vec<_> = (0..20)
        .map(|i| Match {
            source: i,
            reference: i,
            residual_px: 0.,
        })
        .collect();
    let scale = photometry(&a, &b, &pairs).unwrap();
    assert!((scale - 2. / 3.).abs() < 1e-12);
    let transparency = (1. / scale) * 10. / 20.;
    assert!((transparency - 0.75).abs() < 1e-12);
    let offset = 2000. - scale * 3000.;
    assert!(offset.abs() < 1e-10);
    for i in 0..11 {
        a.stars[i].peak_adu = 65535.;
        a.stars[i].flux_adu = 999999.;
    }
    assert!(photometry(&a, &b, &pairs).is_none());
    let missing = crate::inspect::single_set(
        &[Path::new("missing").to_path_buf()],
        fulgur_core::LayoutChoice::Auto,
    );
    let refused = register_sets(
        &[missing],
        f64::NAN,
        None,
        RegistrationOptions::default(),
        None,
        &AtomicBool::new(false),
    );
    assert!(refused.is_err());
}

#[test]
fn clipped_stars_of_a_sensor_below_16_bits_carry_no_brightness() {
    let star = |i: usize| crate::stars::Star {
        x_px: i as f64 * 30.,
        y_px: (i * i % 7) as f64 * 30.,
        hfr_px: 1.7,
        moment_fwhm_px: 3.4,
        eccentricity: 0.2,
        flux_adu: 20000. + i as f64 * 100.,
        peak_adu: 8000.,
        snr: 100.,
    };
    let fourteen_bit = |index: usize| {
        let mut f = planned_frame(index, "2026-10-06T20:00:00", 10., Transform::IDENTITY, 1.);
        f.frame.as_mut().unwrap().saturation_adu = Some(16383.);
        f.stars = (0..20).map(star).collect();
        f
    };
    let (mut a, b) = (fourteen_bit(0), fourteen_bit(1));
    let pairs: Vec<_> = (0..20)
        .map(|i| Match {
            source: i,
            reference: i,
            residual_px: 0.,
        })
        .collect();
    assert!((photometry(&a, &b, &pairs).unwrap() - 1.).abs() < 1e-12);
    // Eleven stars clipped at the 14-bit ceiling: far below 62000 ADU, but
    // saturated, so too few stars are left for a brightness scale.
    for s in &mut a.stars[..11] {
        s.peak_adu = 16383.;
        s.flux_adu = 999_999.;
    }
    assert!(photometry(&a, &b, &pairs).is_none());
    // 16-bit data keep the fixed limit, so their masters do not change.
    let sixteen_bit = planned_frame(2, "2026-10-06T20:00:00", 10., Transform::IDENTITY, 1.);
    assert_eq!(sixteen_bit.peak_limit_adu(), 62000.);
    assert!(a.peak_limit_adu() < 16383. && a.peak_limit_adu() > 15000.);
}

#[test]
fn a_frame_that_is_not_accepted_never_bridges_two_nights() {
    let at = |index: usize, time: &str, accepted: bool| {
        let mut f = planned_frame(index, time, 10., Transform::IDENTITY, 1.);
        f.accepted = accepted;
        f
    };
    // Night one, a refused frame every few hours through the day, night two.
    let frames = vec![
        at(0, "2026-10-06T21:00:00", true),
        at(1, "2026-10-06T23:00:00", true),
        at(2, "2026-10-07T03:00:00", false),
        at(3, "2026-10-07T08:00:00", false),
        at(4, "2026-10-07T13:00:00", false),
        at(5, "2026-10-07T18:00:00", false),
        at(6, "2026-10-07T21:30:00", true),
        at(7, "2026-10-07T22:00:00", true),
    ];
    let groups = sessions(&frames, true);
    assert_eq!(groups, vec![vec![0, 1, 2, 3, 4, 5], vec![6, 7]]);
    // A night that starts with a refused frame keeps it in its own session.
    let late = vec![
        at(0, "2026-10-06T21:00:00", true),
        at(1, "2026-10-06T23:00:00", true),
        at(2, "2026-10-07T21:00:00", false),
        at(3, "2026-10-07T21:10:00", true),
        at(4, "2026-10-07T21:20:00", true),
    ];
    assert_eq!(sessions(&late, true), vec![vec![0, 1], vec![2, 3, 4]]);
    // The same split as the Auto reference's nights.
    let nights = auto_reference::nights(&frames).unwrap();
    assert_eq!(nights, vec![vec![0, 1], vec![6, 7]]);
    // Without a time on every frame there is one session.
    let mut untimed = frames.clone();
    untimed[3].frame.as_mut().unwrap().date_obs = None;
    assert_eq!(sessions(&untimed, false), vec![(0..8).collect::<Vec<_>>()]);
}

pub(super) fn planned_frame(
    index: usize,
    date: &str,
    exposure: f64,
    t: Transform,
    flux_factor: f64,
) -> QualityFrame {
    use fulgur_core::fits::FrameInfo;
    let mut f = QualityFrame::empty(format!("frame_{index}.fit"));
    f.frame = Some(FrameInfo {
        width: 2160,
        height: 3840,
        bitpix: 16,
        bzero: 32768.,
        bscale: 1.,
        bayer_pattern: Some(fulgur_core::BayerPattern::GRBG),
        mosaic: fulgur_core::Mosaic::Bayer,
        mosaic_source: "header",
        exposure_seconds: exposure,
        object: Some("test".into()),
        filter: Some("test".into()),
        date_obs: Some(date.into()),
        gain: Some(200.),
        bias_informational: Some(2064.),
        saturation_adu: None,
        creator: None,
        derived: vec![],
        rows_mirrored: false,
    });
    f.background_adu = Some(2000. * flux_factor);
    f.noise_adu = Some(20. * flux_factor);
    f.hfr_px = Some(1.7);
    f.accepted = true;
    f.provisional_weight = 1.;
    f.stars = (0..90)
        .map(|i| {
            let p = t.inverse().apply([
                50. + ((i * 137 + i * i * 11) % 1900) as f64,
                50. + ((i * 241 + i * i * 7) % 3400) as f64,
            ]);
            crate::stars::Star {
                x_px: p[0],
                y_px: p[1],
                hfr_px: 1.7,
                moment_fwhm_px: 3.4,
                eccentricity: 0.2,
                flux_adu: (10000. + i as f64 * 71.) * flux_factor,
                peak_adu: 20000.,
                snr: 100.,
            }
        })
        .collect();
    f
}
pub(super) fn report(frames: Vec<QualityFrame>) -> InspectReport {
    InspectReport {
        schema_version: 1,
        command: "inspect",
        frame_count: frames.len(),
        accepted_count: frames.iter().filter(|f| f.accepted).count(),
        rejected_count: 0,
        total_exposure_seconds: 0.,
        accepted_exposure_seconds: 0.,
        wall_seconds: 0.,
        frames_per_second: 0.,
        peak_rss_bytes: None,
        max_concurrent_frames: 1,
        cpu_threads: 1,
        config: InspectConfig::default(),
        weights_status: "test",
        photometry_status: "test",
        coordinate_system: "test",
        noise_units: "test",
        registration_status: "test",
        timing_scope: "test",
        bias_subtracted: false,
        intermediate_images_written: 0,
        target_names: vec![],
        light_sets: vec![],
        frames,
    }
}
#[test]
fn complete_two_session_plan_preserves_exposure_photometry_and_rejection() {
    let flip = Transform {
        scale_factor: 1.,
        theta_rad: std::f64::consts::PI,
        tx_px: 2165.,
        ty_px: 3825.,
    };
    let mut frames = vec![
        planned_frame(0, "2026-10-06T22:00:00", 10., Transform::IDENTITY, 1.),
        planned_frame(
            1,
            "2026-10-06T22:10:00",
            20.,
            Transform {
                scale_factor: 1.,
                theta_rad: 0.01,
                tx_px: 2.,
                ty_px: -3.,
            },
            1.5,
        ),
        planned_frame(2, "2026-10-07T22:00:00", 10., flip, 0.8),
        planned_frame(
            3,
            "2026-10-07T22:10:00",
            20.,
            flip.compose(Transform {
                scale_factor: 1.,
                theta_rad: 0.001,
                tx_px: 1.,
                ty_px: 1.,
            }),
            1.8,
        ),
    ];
    // The accepted chronological median is frame 1; one rejected frame is skipped by default.
    frames[1].provisional_weight = 10.;
    frames[3].accepted = false;
    frames[3]
        .rejection_reasons
        .push("excessive_eccentricity".into());
    let r = register_catalogs(
        report(frames),
        0.8,
        None,
        &AtomicBool::new(false),
        Instant::now(),
    )
    .unwrap();
    assert_eq!(r.reference_anchor, "frame_1.fit");
    assert_eq!(r.sessions.len(), 2);
    assert_eq!(r.registration_success_count, 3);
    assert_eq!(r.accepted_count, 3);
    assert_eq!(r.accepted_exposure_seconds, 40.);
    let master = &r.frames[1];
    assert!((master.photometric_scale.unwrap() - 1.).abs() < 1e-12);
    assert!((master.exposure_corrected_transparency.unwrap() - 1.).abs() < 1e-12);
    assert!((r.frames[0].photometric_scale.unwrap() - 1.5).abs() < 1e-10);
    assert!((r.frames[0].exposure_corrected_transparency.unwrap() - 4. / 3.).abs() < 1e-10);
    assert!((r.frames[2].photometric_scale.unwrap() - 1.875).abs() < 1e-10);
    assert!((r.frames[2].exposure_corrected_transparency.unwrap() - 16. / 15.).abs() < 1e-10);
    assert_eq!(r.frames[3].final_weight, 0.);
    assert!(r.frames[3].footprint.is_empty());
    assert!(r.common_intersection.is_some());
    assert!(r.coverage_optimized.is_some());
    assert!(
        r.frames
            .iter()
            .filter(|f| f.registration_success)
            .all(|f| f.inlier_rmse_px.unwrap() < 1e-8)
    );
    let mut none = planned_frame(0, "bad", 10., Transform::IDENTITY, 1.);
    none.accepted = false;
    assert!(
        register_catalogs(
            report(vec![none]),
            0.8,
            None,
            &AtomicBool::new(false),
            Instant::now()
        )
        .is_err()
    );
}
#[test]
fn prediction_uses_header_time_and_rotates_about_sensor_center() {
    let a = Transform::IDENTITY;
    let b = Transform {
        scale_factor: 1.,
        theta_rad: 0.01,
        tx_px: 2.,
        ty_px: 3.,
    };
    let c = predict(&[(100, a), (200, b)], 300, [1080., 1920.]);
    assert!((c.theta_rad - 0.02).abs() < 1e-12);
    let x = a.apply([1080., 1920.]);
    let y = b.apply([1080., 1920.]);
    let z = c.apply([1080., 1920.]);
    assert!((z[0] - (2. * y[0] - x[0])).abs() < 1e-10);
}

#[test]
fn median_anchor_uses_full_header_time_ignores_weights_and_quality_failures() {
    let mut frames = vec![
        planned_frame(
            0,
            "2026-10-06T22:00:00.900000",
            10.,
            Transform::IDENTITY,
            1.,
        ),
        planned_frame(
            1,
            "2026-10-06T22:00:00.100000",
            10.,
            Transform::IDENTITY,
            1.,
        ),
        planned_frame(
            2,
            "2026-10-06T22:00:00.500000",
            10.,
            Transform::IDENTITY,
            1.,
        ),
        planned_frame(
            3,
            "2026-10-06T22:00:00.400000",
            10.,
            Transform::IDENTITY,
            1.,
        ),
    ];
    frames[0].provisional_weight = 999.;
    frames[3].accepted = false;
    assert_eq!(select_anchor(&[0, 1, 2, 3], &frames).unwrap(), 2);
    assert_eq!(select_anchor(&[0, 1], &frames).unwrap(), 1); // Earlier central frame for even count.
    frames[2].frame.as_mut().unwrap().date_obs = None;
    assert!(select_anchor(&[0, 1, 2], &frames).is_err());
    // On an outside grid an undated stack takes its sharpest usable frame;
    // a chosen frame, an own grid and a dated stack keep their rules.
    for (i, hfr) in [2.4, 1.9, 2.1, 1.2].into_iter().enumerate() {
        frames[i].hfr_px = Some(hfr);
    }
    assert_eq!(outside_grid_anchor(&frames, None, true), Some(1));
    assert_eq!(outside_grid_anchor(&frames, Some(0), true), Some(0));
    assert_eq!(outside_grid_anchor(&frames, None, false), None);
    frames[2].frame.as_mut().unwrap().date_obs = Some("2026-10-06T22:00:00.500000".into());
    assert_eq!(outside_grid_anchor(&frames, None, true), None);
    frames[2].frame.as_mut().unwrap().date_obs = None;
    assert_eq!(
        manual_reference(&frames, Path::new("."), Path::new("frame_0.fit")).unwrap(),
        0
    );
    // A frame out of the stack is refused, and the message says why.
    let refused = manual_reference(&frames, Path::new("."), Path::new("frame_3.fit"));
    assert!(refused.unwrap_err().to_string().contains("quality"));
    frames[3].rejection_reasons = vec!["manually_excluded".into()];
    let refused = manual_reference(&frames, Path::new("."), Path::new("frame_3.fit"));
    assert!(refused.unwrap_err().to_string().contains("switched off"));
    assert!(manual_reference(&frames, Path::new("."), Path::new("missing.fit")).is_err());
    frames[0].file_name = "a/same.fit".into();
    frames[1].file_name = "b/same.fit".into();
    assert!(manual_reference(&frames, Path::new("."), Path::new("same.fit")).is_err());
    assert_eq!(
        manual_reference(&frames, Path::new("."), Path::new("b/same.fit")).unwrap(),
        1
    );
}
#[test]
fn real_cfa_pseudo_second_night_rotation_and_shift() {
    let Some(root) = std::env::var_os("FULGURSTACK_TEST_FITS_DIR") else {
        return;
    };
    let mut paths = vec![];
    crate::survey::collect_inputs(Path::new(&root), &mut paths).unwrap();
    paths.sort();
    let cancel = AtomicBool::new(false);
    for path in paths.iter().take(3) {
        let mut raw = fulgur_core::fits::read_raw_sequential(path).unwrap();
        let original = crate::stars::analyze(&raw, &cancel).unwrap();
        let w = raw.header.info.width;
        let h = raw.header.info.height;
        raw.pixels.reverse();
        raw.header.info.bayer_pattern = raw.header.info.bayer_pattern.map(|p| match p {
            fulgur_core::BayerPattern::GRBG => fulgur_core::BayerPattern::GBRG,
            fulgur_core::BayerPattern::GBRG => fulgur_core::BayerPattern::GRBG,
            fulgur_core::BayerPattern::RGGB => fulgur_core::BayerPattern::BGGR,
            fulgur_core::BayerPattern::BGGR => fulgur_core::BayerPattern::RGGB,
        });
        let mut shifted = vec![2000.; raw.pixels.len()];
        for y in 18..h {
            for x in 0..w - 24 {
                shifted[(y - 18) * w + x + 24] = raw.pixels[y * w + x];
            }
        }
        raw.pixels = shifted;
        let transformed = crate::stars::analyze(&raw, &cancel).unwrap();
        let solution = geometry::solve(&transformed.stars, &original.stars, None, &cancel)
            .unwrap()
            .expect("real CFA flip recovery");
        // q = -p + [width-1+24, height-1-18]. No image is saved.
        let expected = Transform {
            scale_factor: 1.,
            theta_rad: std::f64::consts::PI,
            tx_px: (w - 1 + 24) as f64,
            ty_px: (h - 1 - 18) as f64,
        };
        let error = (transformed
            .stars
            .iter()
            .map(|s| {
                let p = [s.x_px, s.y_px];
                let a = solution.transform.apply(p);
                let b = expected.apply(p);
                (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
            })
            .sum::<f64>()
            / transformed.stars.len() as f64)
            .sqrt();
        println!(
            "Pseudo-night real CFA: {} inliers, residual {:.6}px, transform recovery {:.6}px",
            solution.pairs.len(),
            solution.rmse,
            error
        );
        assert!(error < 0.15, "actual recovery error {error}");
    }
}
