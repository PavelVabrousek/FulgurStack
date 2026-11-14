use super::*;

#[test]
fn a_second_pass_on_an_outside_grid_keeps_the_first_anchor() {
    let frames: Vec<_> = (0..4)
        .map(|i| {
            let date = format!("2026-10-06T22:{:02}:00", i * 5);
            crate::register::tests::planned_frame(i, &date, 10., Transform::IDENTITY, 1.)
        })
        .collect();
    assert_eq!(kept_anchor(&frames, Some("frame_3.fit"), false), Some(3));
    // A leading camera chooses by the run's strategy.
    assert_eq!(kept_anchor(&frames, Some("frame_3.fit"), true), None);
    // Another camera, or the first pass, finds its own anchor.
    assert_eq!(kept_anchor(&frames, Some("frame_9.fit"), false), None);
    assert_eq!(kept_anchor(&frames, None, false), None);
}

#[test]
fn a_stranded_set_keeps_its_second_pass_and_the_others_their_first() {
    let mut frames: Vec<_> = (0..6)
        .map(|i| {
            let t = Transform {
                scale_factor: 1.,
                theta_rad: i as f64 * 0.01,
                tx_px: i as f64 * 5.,
                ty_px: 0.,
            };
            let date = format!("2026-10-06T22:{:02}:00", i * 5);
            crate::register::tests::planned_frame(i, &date, 10., t, 1.)
        })
        .collect();
    for frame in &mut frames[3..] {
        frame.light_set = 1;
    }
    let input = crate::register::tests::report(frames);
    let cancel = AtomicBool::new(false);
    let pass = || {
        crate::register::register_catalogs(input.clone(), 0.8, Some(1), &cancel, Instant::now())
            .unwrap()
    };
    let refuse_set = |report: &mut RegistrationReport, set: usize| {
        for (row, q) in report.frames.iter_mut().zip(&report.inspection.frames) {
            if q.light_set == set {
                row.accepted = false;
                row.final_weight = 0.;
                row.rejection_reasons.push("registration_failed".into());
            }
        }
        report.accepted_count = report.frames.iter().filter(|f| f.accepted).count();
    };
    // First pass: set 1 stranded. Second pass: set 1 joins as its own
    // camera, while set 0 falls apart under another anchor.
    let mut first = pass();
    refuse_set(&mut first, 1);
    let mut second = pass();
    refuse_set(&mut second, 0);
    let camera = |anchor: &str, stacked| CameraGroup {
        width: 2160,
        height: 3840,
        frames: 3,
        stacked,
        anchor: Some(anchor.into()),
        brightness_scale: Some(1.),
        shared_stars: 20,
        refused: None,
        mirrored: false,
        bridge_frame: None,
    };
    second.camera_groups = vec![camera("frame_1.fit", 0), camera("frame_4.fit", 3)];
    for row in second.frames.iter_mut().skip(3) {
        row.session_id = 0;
    }
    let taken = taken_sets(&first, &second, &[1]);
    assert_eq!(taken, vec![1]);
    let first_sessions = first.sessions.len();
    let merged = merge_stranded(first, second, &taken, &cancel, Instant::now()).unwrap();
    assert_eq!(merged.accepted_count, 6);
    assert!(merged.frames.iter().all(|f| f.accepted));
    // The single first-pass camera keeps its three frames; set 1 has its own.
    let cameras: Vec<_> = merged
        .camera_groups
        .iter()
        .map(|c| (c.anchor.clone().unwrap(), c.frames, c.stacked))
        .collect();
    assert_eq!(
        cameras,
        [("frame_1.fit".into(), 3, 3), ("frame_4.fit".into(), 3, 3)]
    );
    assert_eq!(merged.sessions.len(), first_sessions + 1);
    assert!(
        merged.frames[3..]
            .iter()
            .all(|f| f.session_id == first_sessions)
    );
    assert!(merged.sessions.iter().enumerate().all(|(i, s)| s.id == i));
    let maximum = merged
        .frames
        .iter()
        .map(|f| f.final_weight)
        .fold(0., f64::max);
    assert!((maximum - 1.).abs() < 1e-12);
    // Another leading anchor means another grid: nothing is merged.
    let mut other = pass();
    other.reference_anchor = "frame_0.fit".into();
    other.camera_groups = vec![camera("frame_4.fit", 3)];
    assert!(taken_sets(&pass(), &other, &[1]).is_empty());
}

#[test]
fn brightness_follows_flux_per_reference_pixel_and_needs_shared_stars() {
    let main: Vec<[f64; 3]> = (0..30)
        .map(|i| [i as f64 * 40., i as f64 * 25., 1000. + i as f64 * 10.])
        .collect();
    // The same stars half as bright, 1 px away, and some of another field.
    let mut other: Vec<[f64; 3]> = main.iter().map(|s| [s[0] + 1., s[1], s[2] / 2.]).collect();
    other.push([5000., 5000., 1.]);
    let (scale, shared) = shared_scale(&main, &other);
    assert_eq!(shared, 30);
    assert!((scale.unwrap() - 2.).abs() < 1e-9);
    let (scale, shared) = shared_scale(&main, &other[..9]);
    assert_eq!((scale, shared), (None, 9));
    assert_eq!(shared_scale(&main, &[]).0, None);
}

#[test]
fn brightness_chains_through_the_camera_that_shares_most_stars() {
    let stars = |range: std::ops::Range<usize>, flux: f64| -> Vec<[f64; 3]> {
        range
            .map(|i| [i as f64 * 40., i as f64 * 25., flux * (1. + i as f64)])
            .collect()
    };
    let camera = |stars| {
        Some(CameraAnchor {
            stars,
            sky: [0.; 3],
        })
    };
    // The first and last cameras overlap only through the middle one.
    let anchors = [
        camera(stars(0..20, 100.)),
        camera(stars(0..40, 50.)),
        camera(stars(25..40, 10.)),
        None,
        camera(stars(100..120, 1.)),
    ];
    let scales = brightness(&anchors);
    assert_eq!(scales[0], (Some(1.), 0));
    assert!((scales[1].0.unwrap() - 2.).abs() < 1e-9 && scales[1].1 == 20);
    assert!((scales[2].0.unwrap() - 10.).abs() < 1e-9 && scales[2].1 == 15);
    assert_eq!(scales[3], (None, 0));
    assert_eq!(scales[4], (None, 0));
}

#[test]
fn rescaling_keeps_the_sky_of_the_first_camera() {
    let quality = QualityFrame::empty("a.fit".into());
    let mut row = empty(&quality, 0, "");
    row.photometric_scale = Some(1.5);
    row.background_offset_adu = Some(-200.);
    row.channel_offsets_adu = Some([-100., -200., -300.]);
    row.normalized_noise_adu = Some(4.);
    let sky = [1000., 1100., 1200.];
    let main_sky = [500., 800., 300.];
    let scale = 2.;
    let shift: [f64; 3] = std::array::from_fn(|c| main_sky[c] - scale * sky[c]);
    rescale(&mut row, scale, shift);
    let offsets = row.channel_offsets_adu.unwrap();
    let a = row.photometric_scale.unwrap();
    assert_eq!(a, 3.);
    // A frame sky of (sky + 100) / 1.5 reaches the first camera's sky.
    for c in 0..3 {
        let frame_sky = (sky[c] - [-100., -200., -300.][c]) / 1.5;
        assert!((a * frame_sky + offsets[c] - main_sky[c]).abs() < 1e-9);
    }
    assert_eq!(row.normalized_noise_adu, Some(8.));
}

#[test]
fn another_bridge_is_tried_only_where_star_lists_match() {
    let star = |i: u64| {
        let hash = (i + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (i / 1000).wrapping_mul(0xD1B5_4A32_D192_ED03);
        crate::stars::Star {
            x_px: 20. + ((hash >> 20) % 960) as f64 + 0.3,
            y_px: 20. + ((hash >> 40) % 960) as f64 + 0.7,
            hfr_px: 2.,
            moment_fwhm_px: 4.,
            eccentricity: 0.2,
            flux_adu: 1000.,
            peak_adu: 500.,
            snr: 50.,
        }
    };
    let field: Vec<_> = (0..60).map(star).collect();
    let other: Vec<_> = (1000..1060).map(star).collect();
    let mut base = crate::register::tests::report(vec![]);
    for (i, minute) in [10, 20, 30, 40].into_iter().enumerate() {
        let mut frame = QualityFrame::empty(format!("{i}.fit"));
        let mut info = crate::inspect::test_frame_info(1000, 1000);
        info.date_obs = Some(format!("2026-10-06T22:{minute}:00"));
        frame.frame = Some(info);
        frame.accepted = true;
        frame.hfr_px = Some(3. - i as f64 * 0.1);
        frame.stars = field.clone();
        base.frames.push(frame);
    }
    let lead = Lead {
        grid: FrameStars {
            name: "lead.fit".into(),
            size: [1000, 1000],
            stars: field.clone(),
            deep: field.clone(),
        },
        lists: vec![("lead.fit".into(), other), ("sharp.fit".into(), field)],
    };
    let calls = std::cell::RefCell::new(vec![]);
    let attempt = |subset: InspectReport, anchor: Option<usize>, template: Option<Template>| {
        let list = match template {
            Some(Template::Frame(_, stars)) => usize::from(std::ptr::eq(stars, &*lead.lists[1].1)),
            _ => 9,
        };
        let mirrored = subset.frames[0].frame.as_ref().unwrap().rows_mirrored;
        calls.borrow_mut().push((anchor, mirrored, list));
        Err(Error::Invalid("no match"))
    };
    let cancel = AtomicBool::new(false);
    let result = rejoin(&base, None, None, Some(&lead), &cancel, &attempt).unwrap();
    assert!(result.is_none());
    let calls = calls.into_inner();
    // The time median (frame 1) first, then the sharpest frames, each only
    // against the leading list that matches; the unmatched anchor list is
    // never registered as it stands, and the budget holds.
    assert_eq!(
        calls[..4],
        [
            (Some(1), false, 1),
            (Some(3), false, 1),
            (Some(2), false, 1),
            (Some(0), false, 1)
        ]
    );
    // Mirrored, only the anchor pair runs without a match of the lists.
    assert_eq!(calls[4..], [(Some(1), true, 0)]);
}

#[test]
fn the_largest_camera_comes_first_and_a_chosen_frame_leads() {
    let frame = |width, accepted: bool, reason: Option<&str>| {
        let mut q = QualityFrame::empty(format!("{width}.fit"));
        q.frame = Some(crate::inspect::test_frame_info(width, 100));
        q.accepted = accepted;
        q.rejection_reasons = reason.map(str::to_owned).into_iter().collect();
        q
    };
    let frames = vec![
        frame(200, true, None),
        frame(300, true, None),
        frame(300, true, None),
        frame(200, false, Some("low_star_count_in_exposure_group")),
    ];
    assert_eq!(groups(&frames, &[]), vec![vec![1, 2], vec![0, 3]]);
    // With equal usable frames the larger sensor leads.
    let tied = vec![frame(200, true, None), frame(300, true, None)];
    assert_eq!(groups(&tied, &[]), vec![vec![1], vec![0]]);
    // An unreadable frame and a lone refused size are no camera.
    let mut single = vec![frame(300, true, None), QualityFrame::empty("x.fit".into())];
    single.push(frame(200, false, Some("excessive_hfr")));
    assert_eq!(groups(&single, &[]), vec![vec![0, 1, 2]]);
    // A stranded light set leaves its equal sensor; a chosen frame leads.
    let mut sets = vec![
        frame(300, true, None),
        frame(300, true, None),
        frame(200, true, None),
        frame(300, true, None),
    ];
    sets[3].light_set = 2;
    sets[2].light_set = 1;
    let mut members = groups(&sets, &[2]);
    assert_eq!(members, vec![vec![0, 1], vec![3], vec![2]]);
    lead_with(&mut members, Some(2));
    assert_eq!(members, vec![vec![2], vec![0, 1], vec![3]]);
}
