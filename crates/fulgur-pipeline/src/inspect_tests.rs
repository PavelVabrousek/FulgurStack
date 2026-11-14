use super::*;
use fulgur_core::BayerPattern;

fn frame(exposure: f64, count: usize, hfr: f64, ecc: f64, noise: f64) -> QualityFrame {
    let mut row = QualityFrame::empty("test.fit".to_owned());
    row.frame = Some(FrameInfo {
        width: 2160,
        height: 3840,
        bitpix: 16,
        bzero: 32768.0,
        bscale: 1.0,
        bayer_pattern: Some(BayerPattern::GRBG),
        mosaic: Mosaic::Bayer,
        mosaic_source: "header",
        exposure_seconds: exposure,
        object: Some("M33".to_owned()),
        filter: Some("IRCUT".to_owned()),
        date_obs: None,
        gain: Some(200.0),
        bias_informational: Some(2064.0),
        saturation_adu: None,
        creator: None,
        derived: vec![],
        rows_mirrored: false,
    });
    row.star_count = count;
    row.measured_star_count = count.min(250);
    row.hfr_px = Some(hfr);
    row.eccentricity = Some(ecc);
    row.noise_adu = Some(noise);
    row.noise_adu_per_second = Some(noise / exposure);
    row
}

#[test]
fn mixed_exposure_groups_keep_shorter_frames_and_use_rate_noise_once() {
    let mut frames: Vec<_> = (0..5)
        .map(|_| frame(5.0, 20, 2.0, 0.3, 10.0))
        .chain((0..5).map(|_| frame(20.0, 200, 2.0, 0.3, 20.0)))
        .collect();
    classify(&mut frames, &InspectConfig::default());
    assert!(frames.iter().all(|f| f.accepted));
    assert!((frames[0].provisional_weight - 0.25).abs() < 1e-12);
    assert_eq!(frames[5].provisional_weight, 1.0);
    assert_eq!(frames[0].star_count_rejection_limit, Some(9.0));
    assert_eq!(frames[5].star_count_rejection_limit, Some(90.0));
}

#[test]
fn another_target_name_stays_in_the_set_and_is_listed_for_the_warning() {
    // Nights of one target saved as "M 33" and "M 33 b" in one folder.
    let mut frames: Vec<_> = (0..8).map(|_| frame(10.0, 200, 2.0, 0.3, 20.0)).collect();
    for f in &mut frames[..5] {
        f.frame.as_mut().unwrap().object = Some("M 33".into());
    }
    for f in &mut frames[5..] {
        f.frame.as_mut().unwrap().object = Some(" M 33 b ".into());
    }
    classify_sets(&mut frames, 1, &InspectConfig::default());
    assert!(frames.iter().all(|f| f.accepted));
    assert_eq!(target_names(&frames), ["M 33", "M 33 b"]);
    // The quality limits use the whole set, both nights together.
    assert_eq!(frames[7].exposure_group_size, 8);
}

#[test]
fn light_sets_are_classified_separately_and_may_differ_in_filter_gain_and_layout() {
    // Set 0: 8 IRCUT colour frames, HFR 2.0. Set 1: 6 LP mono frames with
    // larger stars and another gain; a joint population would reject them.
    let mut frames: Vec<_> = (0..8).map(|_| frame(10.0, 200, 2.0, 0.3, 20.0)).collect();
    for _ in 0..6 {
        let mut f = frame(30.0, 400, 3.2, 0.3, 30.0);
        f.light_set = 1;
        let info = f.frame.as_mut().unwrap();
        info.filter = Some("LP".into());
        info.gain = Some(80.0);
        info.apply_layout(LayoutChoice::Mono);
        frames.push(f);
    }
    // A third set on another sensor and one with another target name.
    let mut other_sensor = frame(10.0, 200, 2.0, 0.3, 20.0);
    other_sensor.light_set = 2;
    other_sensor.frame.as_mut().unwrap().width = 3840;
    let mut other_target = frame(10.0, 200, 2.0, 0.3, 20.0);
    other_target.light_set = 3;
    other_target.frame.as_mut().unwrap().object = Some("M31".into());
    frames.extend([other_sensor, other_target]);
    classify_sets(&mut frames, 4, &InspectConfig::default());
    assert!(
        frames[..14].iter().all(|f| f.accepted),
        "both sets keep their frames"
    );
    assert_eq!(
        frames[13].hfr_rejection_limit_px.map(|l| l > 3.2),
        Some(true)
    );
    assert!(
        frames[14].accepted,
        "another sensor registers as its own camera (ADR-120): {:?}",
        frames[14].rejection_reasons
    );
    assert!(
        frames[15].accepted,
        "another target name only warns: {:?}",
        frames[15].rejection_reasons
    );
    // Weights are normalized across all sets, not per set.
    let maximum = frames
        .iter()
        .map(|f| f.provisional_weight)
        .fold(0.0, f64::max);
    assert_eq!(maximum, 1.0);
    assert!(
        frames[..8].iter().all(|f| f.provisional_weight < 1.0)
            || frames[8..14].iter().all(|f| f.provisional_weight < 1.0)
    );
    // One set reproduces the single-population result exactly.
    let mut single: Vec<_> = (0..8).map(|_| frame(10.0, 200, 2.0, 0.3, 20.0)).collect();
    let mut joint = single.clone();
    classify(&mut single, &InspectConfig::default());
    classify_sets(&mut joint, 1, &InspectConfig::default());
    for (a, b) in single.iter().zip(&joint) {
        assert_eq!(
            a.provisional_weight.to_bits(),
            b.provisional_weight.to_bits()
        );
        assert_eq!(a.rejection_reasons, b.rejection_reasons);
    }
}

#[test]
fn independent_reasons_identify_blur_clouds_and_tracking() {
    let mut frames: Vec<_> = (0..10).map(|_| frame(10.0, 200, 2.0, 0.3, 20.0)).collect();
    frames.push(frame(10.0, 200, 3.0, 0.3, 20.0));
    frames.push(frame(10.0, 20, 2.0, 0.3, 20.0));
    frames.push(frame(10.0, 200, 2.0, 0.8, 20.0));
    classify(&mut frames, &InspectConfig::default());
    assert!(frames[..10].iter().all(|f| f.accepted));
    for (index, reason) in [
        (10, "excessive_hfr"),
        (11, "low_star_count_in_exposure_group"),
        (12, "excessive_eccentricity"),
    ] {
        assert!(frames[index].rejection_reasons.iter().any(|r| r == reason));
        assert_eq!(frames[index].provisional_weight, 0.0);
    }
}

#[test]
fn insufficient_exposure_group_skips_count_threshold() {
    let mut frames: Vec<_> = (0..5).map(|_| frame(20.0, 200, 2.0, 0.3, 20.0)).collect();
    frames.push(frame(5.0, 12, 2.05, 0.3, 10.0));
    classify(&mut frames, &InspectConfig::default());
    assert!(frames[5].accepted);
    assert_eq!(frames[5].exposure_group_size, 1);
    assert!(frames[5].star_count_rejection_limit.is_none());
}

#[test]
fn zero_noise_and_extreme_exposure_never_produce_infinite_weights() {
    let mut frames = vec![
        frame(10.0, 100, 2.0, 0.3, 0.0),
        frame(1e300, 100, 2.0, 0.3, 20.0),
    ];
    classify(&mut frames, &InspectConfig::default());
    assert!(
        frames
            .iter()
            .all(|f| !f.accepted && f.provisional_weight == 0.0)
    );
    assert!(
        frames[0]
            .rejection_reasons
            .iter()
            .any(|r| r == "unusable_noise")
    );
    assert!(
        frames[1]
            .rejection_reasons
            .iter()
            .any(|r| r == "nonfinite_provisional_weight")
    );
}

#[test]
fn first_incompatible_frame_cannot_poison_majority_selection() {
    let mut frames: Vec<_> = (0..4).map(|_| frame(10.0, 100, 2.0, 0.3, 20.0)).collect();
    frames[0].frame.as_mut().unwrap().filter = Some("other".to_owned());
    classify(&mut frames, &InspectConfig::default());
    assert!(!frames[0].accepted);
    assert!(frames[1..].iter().all(|f| f.accepted));
}

#[test]
fn invalid_thresholds_and_csv_formula_injection_are_rejected() {
    let config = InspectConfig {
        max_eccentricity: f64::NAN,
        ..Default::default()
    };
    assert!(config.validate().is_err());
    assert_eq!(csv_text("=SUM(1;2)"), "\"'=SUM(1;2)\"");
    assert_eq!(csv_text("a\"b.fit"), "\"a\"\"b.fit\"");
}

#[test]
fn a_mixed_folder_falls_into_the_groups_inspection_keeps_apart() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, width: usize, extra: &[&str]| {
        let mut bytes = Vec::new();
        let mut cards = vec![
            "SIMPLE  =                    T".to_owned(),
            "BITPIX  =                   16".to_owned(),
            "NAXIS   =                    2".to_owned(),
            format!("NAXIS1  = {width:20}"),
            "NAXIS2  =                   64".to_owned(),
            "BZERO   =                32768".to_owned(),
            "EXPTIME =                  10.".to_owned(),
        ];
        cards.extend(extra.iter().map(|c| c.to_string()));
        cards.push("END".into());
        for card in cards {
            bytes.extend(format!("{card:<80}").bytes());
        }
        bytes.resize(2880, b' ');
        bytes.resize(2880 + (width * 64 * 2).div_ceil(2880) * 2880, 0);
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    };
    let mut files = vec![];
    for i in 0..3 {
        files.push(write(&format!("a{i}.fit"), 96, &["FILTER  = 'L       '"]));
    }
    for i in 0..2 {
        files.push(write(&format!("b{i}.fit"), 96, &["FILTER  = 'Ha      '"]));
    }
    files.push(write(
        "c0.fit",
        128,
        &["FILTER  = 'L       '", "BAYERPAT= 'RGGB    '"],
    ));
    let broken = dir.path().join("broken.fit");
    std::fs::write(&broken, b"not a fits file").unwrap();
    files.push(broken.clone());
    let groups = compatibility_groups(&files, LayoutChoice::Auto);
    let labels: Vec<String> = groups.iter().map(FrameGroup::label).collect();
    assert_eq!(
        labels,
        ["96×64 mono · L", "96×64 mono · Ha", "128×64 colour · L"]
    );
    assert_eq!(
        groups.iter().map(|g| g.files.len()).collect::<Vec<_>>(),
        [4, 2, 1]
    );
    assert!(
        groups[0].files.contains(&broken),
        "unreadable files stay with the largest group"
    );
    // A declared layout makes colour and mono alike, never sensor sizes.
    let mono = compatibility_groups(&files[3..6], LayoutChoice::Mono);
    assert_eq!(mono.len(), 2);
    // A mono and a colour camera of one sensor size stay two groups under a
    // declared layout, and each keeps its own header layout (ASI2600MM + MC).
    let colour_twin = write(
        "d0.fit",
        96,
        &["FILTER  = 'L       '", "BAYERPAT= 'RGGB    '"],
    );
    let pair = [files[0].clone(), colour_twin];
    for layout in [LayoutChoice::Auto, LayoutChoice::Mono] {
        let groups = compatibility_groups(&pair, layout);
        assert_eq!(groups.len(), 2, "{layout:?}");
        assert_eq!(
            groups.iter().map(|g| g.colour).collect::<Vec<_>>(),
            [false, true]
        );
    }
}

#[test]
fn frames_keep_file_order_when_workers_finish_in_any_order() {
    let dir = tempfile::tempdir().unwrap();
    let (width, height) = (96_usize, 80_usize);
    for frame in 0..40_usize {
        let mut bytes = Vec::new();
        for card in [
            "SIMPLE  =                    T".to_owned(),
            "BITPIX  =                   16".to_owned(),
            "NAXIS   =                    2".to_owned(),
            format!("NAXIS1  = {width:20}"),
            format!("NAXIS2  = {height:20}"),
            "BSCALE  =                    1".to_owned(),
            "BZERO   =                32768".to_owned(),
            "EXPTIME =                  10.".to_owned(),
            "END".to_owned(),
        ] {
            bytes.extend(format!("{card:<80}").bytes());
        }
        bytes.resize(2880, b' ');
        for i in 0..width * height {
            let value = 1000 + 10 * frame + (i * 7) % 5;
            bytes.extend(((value as i32 - 32768) as i16).to_be_bytes());
        }
        bytes.resize(bytes.len().div_ceil(2880) * 2880, 0);
        let path = dir.path().join(format!("frame_{frame:02}.fit"));
        std::fs::write(path, bytes).unwrap();
    }
    let counts = Mutex::new(Vec::new());
    let (report, _) = inspect_sets(
        &[single_set(&[dir.path().to_path_buf()], LayoutChoice::Auto)],
        InspectConfig::default(),
        &AtomicBool::new(false),
        Some(&|done: usize, _: usize| counts.lock().unwrap().push(done)),
    )
    .unwrap();
    assert_eq!(report.frames.len(), 40);
    for (frame, result) in report.frames.iter().enumerate() {
        assert_eq!(result.file_name, format!("frame_{frame:02}.fit"));
        let background = result.background_adu.unwrap();
        let expected = (1000 + 10 * frame) as f64;
        assert!((background - expected).abs() < 5., "{background}");
    }
    assert_eq!(
        counts.into_inner().unwrap(),
        (0..=40).collect::<Vec<usize>>()
    );
}

/// A mono frame with Gaussian stars on a textured sky.
fn starry_frame(path: &Path, shift: f64) {
    let (width, height) = (256_usize, 224_usize);
    let mut bytes = Vec::new();
    for card in [
        "SIMPLE  =                    T".to_owned(),
        "BITPIX  =                   16".to_owned(),
        "NAXIS   =                    2".to_owned(),
        format!("NAXIS1  = {width:20}"),
        format!("NAXIS2  = {height:20}"),
        "BSCALE  =                    1".to_owned(),
        "BZERO   =                32768".to_owned(),
        "EXPTIME =                  10.".to_owned(),
        "END".to_owned(),
    ] {
        bytes.extend(format!("{card:<80}").bytes());
    }
    bytes.resize(2880, b' ');
    let stars: Vec<(f64, f64, f64)> = (0..40)
        .map(|k| {
            let k = k as f64;
            (
                12. + (k * 37.3) % 230. + shift,
                12. + (k * 53.9) % 198.,
                3000. + (k * 911.) % 20000.,
            )
        })
        .collect();
    for y in 0..height {
        for x in 0..width {
            let mut v = 1000. + ((x * 7 + y * 13) % 11) as f64;
            for &(sx, sy, a) in &stars {
                let d2 = (x as f64 - sx).powi(2) + (y as f64 - sy).powi(2);
                v += a * (-d2 / (2. * 1.6 * 1.6)).exp();
            }
            let stored = (v.min(65535.).round() as i32 - 32768) as i16;
            bytes.extend(stored.to_be_bytes());
        }
    }
    bytes.resize(bytes.len().div_ceil(2880) * 2880, 0);
    std::fs::write(path, bytes).unwrap();
}

/// Lists added to an inspection without them equal those of an
/// inspection that kept them, and nothing else changes (ADR-154).
fn assert_added_lists_match(sets: &[InputSet]) {
    let cancel = AtomicBool::new(false);
    let with = InspectConfig {
        sky_stars: true,
        ..InspectConfig::default()
    };
    let (expected, _) = inspect_sets(sets, with, &cancel, None).unwrap();
    let (mut actual, root) = inspect_sets(sets, InspectConfig::default(), &cancel, None).unwrap();
    assert!(actual.frames.iter().all(|f| f.sky_stars.is_empty()));
    let before = serde_json::to_string(&actual.frames).unwrap();
    let counts = Mutex::new(vec![]);
    add_sky_stars(
        &mut actual,
        &root,
        sets,
        &cancel,
        Some(&|done: usize, _: usize| counts.lock().unwrap().push(done)),
    )
    .unwrap();
    assert_eq!(serde_json::to_string(&actual.frames).unwrap(), before);
    let mut listed = 0;
    for (a, e) in actual.frames.iter().zip(&expected.frames) {
        let bits = |list: &[crate::sky::detect::SkyStar]| -> Vec<[u64; 6]> {
            list.iter().map(|s| s.map(f64::to_bits)).collect()
        };
        assert_eq!(bits(&a.sky_stars), bits(&e.sky_stars), "{}", a.file_name);
        listed += usize::from(!a.sky_stars.is_empty());
    }
    assert!(listed > 0, "no frame had a bright-star list");
    let measured = actual
        .frames
        .iter()
        .filter(|f| f.background_adu.is_some())
        .count();
    assert_eq!(
        counts.into_inner().unwrap(),
        (0..=measured).collect::<Vec<_>>()
    );
}

#[test]
fn sky_star_lists_added_later_equal_those_of_a_sky_inspection() {
    let dir = tempfile::tempdir().unwrap();
    for frame in 0..6 {
        starry_frame(
            &dir.path().join(format!("light_{frame}.fit")),
            frame as f64 * 0.37,
        );
    }
    assert_added_lists_match(&[single_set(&[dir.path().to_path_buf()], LayoutChoice::Auto)]);
    let wrong = vec![single_set(&[dir.path().to_path_buf()], LayoutChoice::Auto); 2];
    let (mut report, root) = inspect_sets(
        &wrong[..1],
        InspectConfig::default(),
        &AtomicBool::new(false),
        None,
    )
    .unwrap();
    assert!(add_sky_stars(&mut report, &root, &wrong, &AtomicBool::new(false), None).is_err());
}

#[test]
fn sky_star_lists_added_later_match_on_real_frames() {
    let Some(dir) = std::env::var_os("FULGURSTACK_TEST_FITS_DIR") else {
        return;
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| fulgur_core::fits::is_input(p))
        .collect();
    files.sort();
    files.truncate(4);
    assert_added_lists_match(&[single_set(&files, LayoutChoice::Auto)]);
}

#[test]
fn cancellation_stops_before_input_decoding() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("bad.fit"), b"not FITS").unwrap();
    let result = inspect(dir.path(), InspectConfig::default(), &AtomicBool::new(true));
    assert!(matches!(result, Err(Error::Cancelled)));
}
