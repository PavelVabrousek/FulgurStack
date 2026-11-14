use super::*;

fn pose(degrees: f64, offset: [f64; 2]) -> Transform {
    let theta = degrees.to_radians();
    let (s, c) = theta.sin_cos();
    let center = [1079.5, 1919.5];
    Transform {
        scale_factor: 1.,
        theta_rad: theta,
        tx_px: center[0] + offset[0] - c * center[0] + s * center[1],
        ty_px: center[1] + offset[1] - s * center[0] - c * center[1],
    }
}

/// One frame of the common star field, seen through `t`.
fn sky_frame(index: usize, night: usize, minute: usize, t: Transform) -> QualityFrame {
    let mut f = planned_frame(
        index,
        &format!("2026-10-{night:02}T22:{minute:02}:00"),
        10.,
        t,
        1.,
    );
    f.stars = (0..240)
        .filter_map(|i| {
            let p = t.inverse().apply([
                80. + ((i * 137 + i * i * 17) % 2000) as f64,
                80. + ((i * 271 + i * i * 23) % 3680) as f64,
            ]);
            (p[0] > 2. && p[1] > 2. && p[0] < 2157. && p[1] < 3837.).then(|| crate::stars::Star {
                x_px: p[0],
                y_px: p[1],
                hfr_px: 1.7,
                moment_fwhm_px: 3.4,
                eccentricity: 0.1,
                flux_adu: 10000. + i as f64,
                peak_adu: 20000.,
                snr: 90.,
            })
        })
        .collect();
    f
}

fn fixture() -> InspectReport {
    let mut frames = vec![];
    for (night, count, degrees) in [(6, 5, 0.), (7, 1, 90.), (8, 5, 0.)] {
        for minute in 0..count {
            let t = pose(degrees, [0., 0.]);
            frames.push(sky_frame(frames.len(), night, minute, t));
        }
    }
    frames[3].hfr_px = Some(1.4);
    report(frames)
}

fn run(
    input: InspectReport,
    options: RegistrationOptions,
    selected: Option<usize>,
) -> Result<RegistrationReport> {
    register_catalogs_observed(
        input,
        0.8,
        selected,
        options,
        &AtomicBool::new(false),
        Instant::now(),
        None,
    )
}

#[test]
fn auto_multinight_chooses_majority_framing_instead_of_middle_date_and_quality_breaks_ties() {
    let input = fixture();
    let before = serde_json::to_string(&input).unwrap();
    let old = run(
        input.clone(),
        RegistrationOptions {
            reference_strategy: ReferenceStrategy::ChronologicalMedian,
            ..Default::default()
        },
        None,
    )
    .unwrap();
    let auto = run(input.clone(), RegistrationOptions::default(), None).unwrap();
    assert_eq!(old.reference_anchor, "frame_5.fit");
    assert_eq!(auto.reference_anchor, "frame_3.fit");
    assert_eq!(auto.accepted_count, 11);
    let a = auto.auto_reference.as_ref().unwrap();
    assert_eq!(
        (a.nights, a.selected_frames, a.candidate_count),
        (3, 11, 11)
    );
    assert!(
        a.predicted_rectangle_area_px > a.median_reference_rectangle_area_px * 1.6,
        "{a:?}"
    );
    assert!(a.predicted_mean_selected_coverage > a.median_reference_mean_selected_coverage);
    assert!(a.predicted_mean_selected_coverage <= 1.);
    assert_eq!(serde_json::to_string(&input).unwrap(), before);
    // Final, direct-to-chosen-reference registration still recovers known poses.
    for f in auto.frames.iter().filter(|f| f.accepted) {
        assert!(f.inlier_rmse_px.unwrap() < 1e-7);
    }
    if let Some(path) = std::env::var_os("FULGURSTACK_AUTO_REFERENCE_EVIDENCE") {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        serde_json::to_writer_pretty(file,&serde_json::json!({"status":"PASS","scope":"synthetic three-night catalogs; 5+1+5 frames, middle night rotated 90 degrees; not a real multi-night benchmark","selected":11,"registered":auto.accepted_count,"automatic":a})).unwrap();
    }
}

#[test]
fn auto_single_night_crossing_midnight_is_exactly_the_legacy_median_and_manual_wins() {
    let dates = [
        "2026-10-06T23:58:00",
        "2026-10-06T23:59:00",
        "2026-10-07T00:01:00",
        "2026-10-07T00:02:00",
    ];
    let input = report(
        dates
            .iter()
            .enumerate()
            .map(|(i, t)| planned_frame(i, t, 10., pose(i as f64, [0., 0.]), 1.))
            .collect(),
    );
    let old = run(
        input.clone(),
        RegistrationOptions {
            reference_strategy: ReferenceStrategy::ChronologicalMedian,
            ..Default::default()
        },
        None,
    )
    .unwrap();
    let auto = run(input, RegistrationOptions::default(), None).unwrap();
    assert_eq!(
        super::parallel::science(&old),
        super::parallel::science(&auto)
    );
    assert!(auto.auto_reference.is_none());
    assert_eq!(auto.reference_anchor, "frame_1.fit");
    let manual = run(fixture(), RegistrationOptions::default(), Some(5)).unwrap();
    assert!(manual.auto_reference.is_none());
    assert_eq!(manual.reference_anchor, "frame_5.fit");
}

#[test]
fn auto_exclusions_diagnostics_and_pool_size_do_not_reweight_nights_or_change_selection() {
    let mut input = fixture();
    for i in 11..22 {
        let mut f = planned_frame(i, "2026-10-09T22:00:00", 10., pose(90., [0., 0.]), 1.);
        f.accepted = false;
        input.frames.push(f);
    }
    input.frame_count = input.frames.len();
    let mut previous = None;
    for threads in [1, 4] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        for diagnose_excluded in [false, true] {
            let plan = pool
                .install(|| {
                    run(
                        input.clone(),
                        RegistrationOptions {
                            diagnose_excluded,
                            ..Default::default()
                        },
                        None,
                    )
                })
                .unwrap();
            assert_eq!(plan.reference_anchor, "frame_3.fit");
            let a = plan.auto_reference.as_ref().unwrap();
            assert_eq!((a.nights, a.selected_frames), (3, 11));
            let rows: Vec<_> = plan
                .frames
                .iter()
                .filter(|f| f.quality_accepted)
                .map(|f| serde_json::to_string(f).unwrap())
                .collect();
            if let Some(old) = &previous {
                assert_eq!(&rows, old);
            }
            previous = Some(rows);
        }
    }
}

#[test]
fn auto_reports_unmapped_nights_and_handles_angle_wrap_and_shifted_centers() {
    let mut input = fixture();
    for f in input.frames.iter_mut().skip(6) {
        f.stars.clear();
    }
    let total = input.frames.len();
    let plan = run(input, RegistrationOptions::default(), None).unwrap();
    let selection = plan.auto_reference.as_ref().unwrap();
    assert_eq!(selection.unmapped_frames, total - 6);
    assert_eq!(selection.unmapped_nights, 1);
    assert_eq!(plan.accepted_count, 6, "mapped frames still stack");
    assert!(selection.warning().unwrap().contains("one whole night"));
    let mut wrapped = fixture();
    for (i, f) in wrapped.frames.iter_mut().enumerate() {
        if i == 5 {
            continue;
        }
        let t = pose(if i < 5 { 179. } else { -1. }, [0., 0.]);
        for s in &mut f.stars {
            let p = t.inverse().apply([s.x_px, s.y_px]);
            s.x_px = p[0];
            s.y_px = p[1];
        }
        f.stars
            .retain(|s| s.x_px > 2. && s.x_px < 2157. && s.y_px > 2. && s.y_px < 3837.);
    }
    let wrapped_plan = run(wrapped, RegistrationOptions::default(), None).unwrap();
    assert_eq!(wrapped_plan.reference_anchor, "frame_3.fit");
    assert_eq!(wrapped_plan.accepted_count, 11);
    let mut input = fixture();
    // Identical angle, different centers: count coverage must see translation.
    let shifted = pose(0., [650., 0.]);
    let f = &mut input.frames[5];
    for s in &mut f.stars {
        let sky = pose(90., [0., 0.]).apply([s.x_px, s.y_px]);
        let p = shifted.inverse().apply(sky);
        s.x_px = p[0];
        s.y_px = p[1];
    }
    let plan = run(input, RegistrationOptions::default(), None).unwrap();
    assert_ne!(plan.reference_anchor, "frame_5.fit");
}

#[test]
fn auto_progress_completes_once_with_reuse_refit_diagnostics_and_optional_geometry() {
    for geometric_correction in [false, true] {
        let events = Mutex::new(vec![]);
        let (plan, _) = register_inspected(
            fixture(),
            ".".into(),
            Engine::Stars,
            &RunContext {
                min_coverage: 0.8,
                reference: None,
                options: RegistrationOptions {
                    geometric_correction,
                    ..Default::default()
                },
                cancel: &AtomicBool::new(false),
                start: Instant::now(),
                progress: Some(&|n, total| events.lock().unwrap().push((n, total))),
                calibrations: &[],
            },
        )
        .unwrap();
        assert!(plan.auto_reference.is_some());
        let e = events.into_inner().unwrap();
        let total = e[0].1;
        assert_eq!(e, (0..=total).map(|n| (n, total)).collect::<Vec<_>>());
    }
    let mut input = fixture();
    for f in &mut input.frames {
        f.hfr_px = Some(1.7);
    }
    // Make the chronological reference the best available candidate, testing reuse.
    input.frames[5].stars = input.frames[0].stars.clone();
    input.frames[5].hfr_px = Some(1.0);
    let events = Mutex::new(vec![]);
    let (plan, _) = register_inspected(
        input,
        ".".into(),
        Engine::Stars,
        &RunContext {
            min_coverage: 0.8,
            reference: None,
            options: RegistrationOptions::default(),
            cancel: &AtomicBool::new(false),
            start: Instant::now(),
            progress: Some(&|n, t| events.lock().unwrap().push((n, t))),
            calibrations: &[],
        },
    )
    .unwrap();
    assert_eq!(plan.reference_anchor, "frame_5.fit");
    let a = plan.auto_reference.unwrap();
    assert_eq!(a.selected_reference, a.preliminary_reference);
    let e = events.into_inner().unwrap();
    assert_eq!(e.last().unwrap().0, e.last().unwrap().1);
}

/// ADR-119: a night at the edge of the plate-scale range loses to the median
/// night even with the sharpest frame and the same framing, and an elongated
/// frame never anchors the stack.
#[test]
fn auto_prefers_the_median_plate_scale_and_refuses_the_worst_shapes() {
    let mut frames = vec![];
    for (night, count, scale) in [(6, 3, 1.0008), (7, 4, 1.0003), (8, 4, 1.)] {
        for minute in 0..count {
            let t = Transform {
                scale_factor: scale,
                theta_rad: 0.,
                tx_px: 1079.5 * (1. - scale),
                ty_px: 1919.5 * (1. - scale),
            };
            let mut f = sky_frame(frames.len(), night, minute, t);
            f.eccentricity = Some(0.3);
            f.star_count = 1000;
            frames.push(f);
        }
    }
    frames[0].hfr_px = Some(1.2);
    frames[3].hfr_px = Some(1.3);
    frames[3].eccentricity = Some(0.9);
    frames[4].hfr_px = Some(1.6);
    let plan = run(report(frames), RegistrationOptions::default(), None).unwrap();
    assert_eq!(plan.reference_anchor, "frame_4.fit");
    let a = plan.auto_reference.as_ref().unwrap();
    assert!((a.plate_scale_spread_ppm - 800.).abs() < 30., "{a:?}");
    assert!(a.selected_scale_offset_ppm.unwrap() < 30., "{a:?}");
    assert!(a.candidates_after_filters < a.candidate_count, "{a:?}");
}
