use super::*;
use fulgur_core::fits::{read_header, read_raw};

const W: usize = 64;
/// Not a multiple of the strip height: the remainder joins the last strip.
const H: usize = 70;

fn write(path: &std::path::Path, exposure: f64, gain: f64, value: impl Fn(usize, usize) -> f64) {
    let mut bytes = Vec::new();
    for (key, v) in [
        ("SIMPLE", "T".to_owned()),
        ("BITPIX", "16".to_owned()),
        ("NAXIS", "2".to_owned()),
        ("NAXIS1", W.to_string()),
        ("NAXIS2", H.to_string()),
        ("BZERO", "32768".to_owned()),
        ("BSCALE", "1".to_owned()),
        ("EXPTIME", format!("{exposure:.1}")),
        ("GAIN", format!("{gain:.0}")),
        ("DET-TEMP", "27".to_owned()),
        ("BAYERPAT", "'RGGB'".to_owned()),
    ] {
        bytes.extend(format!("{:<80}", format!("{key:<8}= {v:>20}")).as_bytes());
    }
    bytes.extend(format!("{:<80}", "END").as_bytes());
    bytes.resize(2880, b' ');
    for y in 0..H {
        for x in 0..W {
            let stored = (value(x, y).round() - 32768.) as i16;
            bytes.extend(stored.to_be_bytes());
        }
    }
    bytes.resize(bytes.len().div_ceil(2880) * 2880, 0);
    std::fs::write(path, bytes).unwrap();
}

/// Bias level, a column ramp (amplifier glow) and three hot pixels.
fn truth(x: usize, y: usize) -> f64 {
    let hot = [(5, 7), (40, 33), (63, 69)].contains(&(x, y));
    190. + (x % 7) as f64 + if hot { 3000. } else { 0. }
}

fn darks(dir: &std::path::Path, count: usize, exposure: f64) -> Vec<PathBuf> {
    (0..count)
        .map(|i| {
            let path = dir.join(format!("Dark_{i}.fits"));
            write(&path, exposure, 60., |x, y| {
                // Deterministic +-1 ADU read noise; one cosmic ray in frame 2.
                let noise = ((x * 7 + y * 13 + i * 5) % 3) as f64 - 1.;
                let cosmic = if i == 2 && (x, y) == (10, 10) {
                    5000.
                } else {
                    0.
                };
                truth(x, y) + noise + cosmic
            });
            path
        })
        .collect()
}

#[test]
fn master_dark_keeps_fixed_pattern_rejects_transients_and_stays_in_ram() {
    let dir = tempfile::tempdir().unwrap();
    let files = darks(dir.path(), 7, 60.);
    let before: Vec<_> = files.iter().map(|p| std::fs::read(p).unwrap()).collect();
    let master =
        build_master_dark(&files, CombineMethod::default(), &AtomicBool::new(false)).unwrap();
    assert_eq!((master.width, master.height), (W, H));
    for y in 0..H {
        for x in 0..W {
            let error = (f64::from(master.pixels[y * W + x]) - truth(x, y)).abs();
            assert!(error < 0.5, "({x},{y}) error {error}");
        }
    }
    assert!(master.summary.clipped_samples >= 1, "cosmic ray clipped");
    assert_eq!(master.summary.frames, 7);
    assert_eq!(master.summary.temperature_c, Some([27., 27.]));
    assert!(!master.summary.written_to_disk);
    // Median of 190 + (x % 7) is 193; the pedestal is the rounded median.
    assert_eq!(master.pedestal, 193.);
    // Sources are only read; no other file appears.
    for (path, bytes) in files.iter().zip(&before) {
        assert_eq!(&std::fs::read(path).unwrap(), bytes);
    }
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 7);
}

#[test]
fn subtraction_removes_the_pattern_identically_for_frames_and_rois() {
    let dir = tempfile::tempdir().unwrap();
    // Five frames: a single cosmic ray is at most (n-1)/sqrt(n) = 1.8 sigma
    // from the mean, so sigma-kappa 2 cannot clip it; the median can.
    let master = build_master_dark(
        &darks(dir.path(), 5, 60.),
        CombineMethod::Median,
        &AtomicBool::new(false),
    )
    .unwrap();
    let light = dir.path().join("light.fits");
    // Sky 250 ADU above bias plus a star-like bump, on top of the dark signal.
    write(&light, 60., 60., |x, y| {
        let star = 800. * (-(((x as f64 - 30.).powi(2) + (y as f64 - 40.).powi(2)) / 4.)).exp();
        truth(x, y) + 250. + star
    });
    let mut full = read_raw(&light).unwrap();
    master.subtract(&mut full, [0, 0]).unwrap();
    for y in 0..H {
        for x in 0..W {
            let star = 800. * (-(((x as f64 - 30.).powi(2) + (y as f64 - 40.).powi(2)) / 4.)).exp();
            let expected = 250. + star + f64::from(master.pedestal);
            // Median of +-1 ADU read noise plus light rounding.
            assert!((f64::from(full.pixels[y * W + x]) - expected).abs() <= 1.5);
        }
    }
    let header = read_header(&light).unwrap();
    let mut roi = read_roi(&light, &header, [8, 12, 20, 16], &AtomicBool::new(false)).unwrap();
    master.subtract(&mut roi, [8, 12]).unwrap();
    for y in 0..16 {
        for x in 0..20 {
            assert_eq!(roi.pixels[y * 20 + x], full.pixels[(y + 12) * W + x + 8]);
        }
    }
    let mut outside = read_roi(&light, &header, [50, 0, 14, 8], &AtomicBool::new(false)).unwrap();
    assert!(master.subtract(&mut outside, [52, 0]).is_err());
}

#[test]
fn mismatched_lights_and_dark_sets_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let files = darks(dir.path(), 4, 60.);
    assert!(
        build_master_dark(
            &files[..2],
            CombineMethod::default(),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    let other = dir.path().join("Dark_30s.fits");
    write(&other, 30., 60., truth);
    let mut mixed = files.clone();
    mixed.push(other);
    // Owner decision: mixed exposures (or gains) build a master with a warning.
    let warned =
        build_master_dark(&mixed, CombineMethod::default(), &AtomicBool::new(false)).unwrap();
    assert_eq!(
        warned.summary.warnings,
        ["dark frames have different exposures (30 s, 60 s)"]
    );
    assert_eq!(warned.summary.exposure_seconds, 60., "median exposure");
    let master =
        build_master_dark(&files, CombineMethod::default(), &AtomicBool::new(false)).unwrap();
    assert!(master.summary.warnings.is_empty());
    let mut info = read_header(&files[0]).unwrap().info;
    assert_eq!(incompatibility(&master.summary, &info), None);
    info.exposure_seconds = 30.;
    assert_eq!(
        incompatibility(&master.summary, &info),
        None,
        "another exposure only warns (see light_warnings)"
    );
    info.width = 32;
    assert_eq!(
        incompatibility(&master.summary, &info),
        Some("dark_size_mismatch")
    );
    assert!(build_master_dark(&files, CombineMethod::default(), &AtomicBool::new(true)).is_err());
}

#[test]
fn combine_methods_match_their_definitions() {
    let m = |method, values: &[f32]| combine(method, &mut values.to_vec());
    // Eight steady samples and one cosmic ray.
    let pixel = [100., 101., 99., 100., 102., 98., 100., 101., 900.];
    let average = m(CombineMethod::Average, &pixel);
    assert!((average.0 - 1701. / 9.).abs() < 1e-3 && average.1 == 0);
    assert_eq!(m(CombineMethod::Median, &pixel), (100., 0));
    let (clipped, rejected) = m(CombineMethod::SigmaKappa2, &pixel);
    assert_eq!(rejected, 1, "only the cosmic ray is clipped");
    assert!((clipped - 100.125).abs() < 1e-3);
    // Even counts use the mean of the two central values.
    assert_eq!(m(CombineMethod::Median, &[1., 4., 2., 3.]).0, 2.5);
    // Identical samples never divide by a zero sigma.
    assert_eq!(m(CombineMethod::SigmaKappa2, &[7.; 5]), (7., 0));
    // With five samples one outlier cannot exceed 2 sigma (documented limit).
    assert_eq!(m(CombineMethod::SigmaKappa2, &[0., 0., 0., 0., 500.]).1, 0);
    for method in CombineMethod::ALL {
        assert_eq!(CombineMethod::parse(method.key()), Some(method));
    }
    assert_eq!(
        CombineMethod::parse("sigma-kappa-2"),
        Some(CombineMethod::SigmaKappa2)
    );
}

#[test]
fn saved_master_reloads_exactly_and_foreign_masters_are_scaled_or_refused() {
    let dir = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    let files = darks(dir.path(), 5, 60.);
    for method in CombineMethod::ALL {
        let master = build_master_dark(&files, method, &cancel).unwrap();
        let path = dir.path().join(master.suggested_name());
        assert!(
            path.to_string_lossy()
                .ends_with(&format!("MasterDark_60s_gain60_5x_{}.fits", method.key()))
        );
        save_master_dark(&master, &path, &cancel).unwrap();
        // Never overwritten; no temporary file left behind.
        assert!(save_master_dark(&master, &path, &cancel).is_err());
        let loaded = load_master_dark(&path, &cancel).unwrap();
        assert_eq!(
            loaded.pixels, master.pixels,
            "{method:?}: exact f32 round trip"
        );
        assert_eq!(loaded.pedestal, master.pedestal);
        let s = &loaded.summary;
        assert_eq!((s.frames, s.exposure_seconds, s.gain), (5, 60., Some(60.)));
        assert_eq!(s.combine, Some(method));
        assert_eq!(s.temperature_c, Some([27., 27.]));
        assert!(!s.normalized_input_scaled);
        assert_eq!(loaded.bayer_pattern, Some(fulgur_core::BayerPattern::RGGB));
        let meta = read_metadata(&path).unwrap();
        assert_eq!(meta.image_type.as_deref(), Some("Master Dark"));
        assert_eq!(meta.combined_frames, Some(5));
    }
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().all(|n| !n.contains(".tmp")));
    // A 16-bit master in ADU loads like a light.
    let integer = dir.path().join("master16.fits");
    write(&integer, 60., 60., truth);
    let loaded = load_master_dark(&integer, &cancel).unwrap();
    assert_eq!(f64::from(loaded.pixels[7 * W + 5]), truth(5, 7));
    // A float master normalized to 0..1 (e.g. PixInsight) is scaled to ADU.
    let master = build_master_dark(&files, CombineMethod::Median, &cancel).unwrap();
    let normalized = dir.path().join("normalized.fits");
    let mut bytes = std::fs::read(dir.path().join(master.suggested_name())).unwrap();
    for c in bytes[..2880].chunks_exact_mut(80) {
        if c.starts_with(b"BUNIT") {
            c.copy_from_slice(&[b' '; 80]);
            c[..7].copy_from_slice(b"COMMENT");
        }
    }
    let data = 2880;
    for (i, v) in master.pixels.iter().enumerate() {
        let at = data + i * 4;
        bytes[at..at + 4].copy_from_slice(&(v / 65535.).to_be_bytes());
    }
    std::fs::write(&normalized, bytes).unwrap();
    let scaled = load_master_dark(&normalized, &cancel).unwrap();
    assert!(scaled.summary.normalized_input_scaled);
    for (a, b) in scaled.pixels.iter().zip(&master.pixels) {
        assert!((a - b).abs() < 0.01);
    }
    // Missing exposure, three axes or text are refused.
    let mut no_exposure = std::fs::read(&integer).unwrap();
    for c in no_exposure[..2880].chunks_exact_mut(80) {
        if c.starts_with(b"EXPTIME") {
            c[..7].copy_from_slice(b"XPTIMEX");
        }
    }
    let path = dir.path().join("no_exposure.fits");
    std::fs::write(&path, no_exposure).unwrap();
    let unknown = load_master_dark(&path, &cancel).unwrap();
    assert_eq!(unknown.summary.exposure_seconds, 0.);
    assert_eq!(
        unknown.summary.warnings.len(),
        1,
        "missing exposure only warns"
    );
    std::fs::write(dir.path().join("text.fits"), b"not a FITS file").unwrap();
    assert!(load_master_dark(&dir.path().join("text.fits"), &cancel).is_err());
}

#[test]
fn three_identical_planes_load_as_one_and_colour_masters_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    // A stacker's mono master written as normalized float RGB planes.
    let write_planes = |path: &std::path::Path, offsets: [f32; 3]| {
        let mut bytes = Vec::new();
        for (key, v) in [
            ("SIMPLE", "T"),
            ("BITPIX", "-32"),
            ("NAXIS", "3"),
            ("NAXIS1", &W.to_string()),
            ("NAXIS2", &H.to_string()),
            ("NAXIS3", "3"),
            ("EXPTIME", "300.0"),
            ("GAIN", "100"),
            ("BUNIT", "'normalized'"),
        ] {
            bytes.extend(format!("{:<80}", format!("{key:<8}= {v:>20}")).as_bytes());
        }
        bytes.extend(format!("{:<80}", "END").as_bytes());
        bytes.resize(2880, b' ');
        for offset in offsets {
            for y in 0..H {
                for x in 0..W {
                    bytes.extend(((truth(x, y) as f32 + offset) / 65535.).to_be_bytes());
                }
            }
        }
        bytes.resize(bytes.len().div_ceil(2880) * 2880, 0);
        std::fs::write(path, bytes).unwrap();
    };
    let mono = dir.path().join("MasterDARK_rgb_planes.fit");
    write_planes(&mono, [0.; 3]);
    let master = load_master_dark(&mono, &cancel).unwrap();
    assert!(master.summary.normalized_input_scaled);
    assert_eq!(master.summary.exposure_seconds, 300.);
    assert!((f64::from(master.pixels[7 * W + 5]) - truth(5, 7)).abs() < 0.01);
    let colour = dir.path().join("MasterDARK_colour.fit");
    write_planes(&colour, [0., 40., 0.]);
    let error = load_master_dark(&colour, &cancel).unwrap_err().to_string();
    assert!(error.contains("colour"), "{error}");
}

#[test]
fn bias_flat_and_dark_flat_masters_follow_their_rules() {
    let dir = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    // Bias frames: zero exposure is valid for bias only.
    let bias: Vec<_> = (0..5)
        .map(|i| {
            let path = dir.path().join(format!("Bias_{i}.fits"));
            write(&path, 0., 60., |x, y| {
                190. + (x % 7) as f64 + ((x + y + i) % 3) as f64 - 1.
            });
            path
        })
        .collect();
    let master = build_master(&bias, MasterKind::Bias, CombineMethod::Median, &cancel).unwrap();
    assert_eq!(master.summary.kind, MasterKind::Bias);
    assert_eq!(master.summary.exposure_seconds, 0.);
    assert!(master.suggested_name().starts_with("MasterBias_gain60_5x"));
    // Bias frames as darks: zero exposure is reported, not fatal.
    let as_dark = build_master_dark(&bias, CombineMethod::Median, &cancel).unwrap();
    assert!(as_dark.summary.warnings[0].contains("record no exposure"));
    // Flats at different brightness are scaled to their mean median first.
    let flats: Vec<_> = [1.0, 0.5, 1.5, 1.2]
        .iter()
        .enumerate()
        .map(|(i, gain)| {
            let path = dir.path().join(format!("Flat_{i}.fits"));
            write(&path, 2. + i as f64, 60., |x, y| {
                // Vignetting: brighter centre, same shape in every flat.
                let r2 = ((x as f64 - 32.).powi(2) + (y as f64 - 35.).powi(2)) / 2000.;
                gain * 20000. * (1. - 0.3 * r2)
            });
            path
        })
        .collect();
    let flat = build_master(&flats, MasterKind::Flat, CombineMethod::Average, &cancel).unwrap();
    assert!(flat.summary.flat_normalization.is_some());
    let centre = f64::from(flat.pixels[35 * W + 32]);
    let corner = f64::from(flat.pixels[0]);
    let expected_ratio = 1. - 0.3 * ((32_f64).powi(2) + (35_f64).powi(2)) / 2000.;
    assert!(
        (corner / centre - expected_ratio).abs() < 1e-3,
        "shape kept after normalization"
    );
    // The level is the mean of the frames' medians (about 1.05 x 20000 x shape).
    assert!(centre > 20000. && centre < 22000., "{centre}");
    // Dark flats need one exposure, like darks.
    let mut dark_flats: Vec<_> = (0..3)
        .map(|i| {
            let path = dir.path().join(format!("DarkFlat_{i}.fits"));
            write(&path, 2., 60., truth);
            path
        })
        .collect();
    assert!(
        build_master(
            &dark_flats,
            MasterKind::DarkFlat,
            CombineMethod::Average,
            &cancel
        )
        .is_ok()
    );
    let other = dir.path().join("DarkFlat_3s.fits");
    write(&other, 3., 60., truth);
    dark_flats.push(other);
    let mixed = build_master(
        &dark_flats,
        MasterKind::DarkFlat,
        CombineMethod::Average,
        &cancel,
    )
    .unwrap();
    assert!(mixed.summary.warnings[0].contains("different exposures (2 s, 3 s)"));
    // Saved masters carry their type.
    for (m, image_type) in [(&master, "Master Bias"), (&flat, "Master Flat")] {
        let path = dir.path().join(m.suggested_name());
        save_master(m, &path, &cancel).unwrap();
        assert_eq!(
            read_metadata(&path).unwrap().image_type.as_deref(),
            Some(image_type)
        );
    }
}

/// Sensor model used by the calibration-math tests: bias pattern, thermal
/// rate with hot pixels, vignetting, per-phase (CFA) response.
fn bias_level(x: usize, y: usize) -> f64 {
    500. + (x % 5) as f64 + (y % 3) as f64
}
fn thermal_rate(x: usize, y: usize) -> f64 {
    0.05 + if (x * 7 + y * 3) % 97 == 0 { 40. } else { 0. }
}
fn vignetting(x: usize, y: usize) -> f64 {
    let r2 = ((x as f64 - 32.).powi(2) + (y as f64 - 35.).powi(2)) / 2400.;
    1. - 0.35 * r2
}
fn phase_gain(x: usize, y: usize, gains: [f64; 4]) -> f64 {
    gains[x % 2 + 2 * (y % 2)]
}

fn frames(
    dir: &std::path::Path,
    name: &str,
    count: usize,
    exposure: f64,
    value: impl Fn(usize, usize, usize) -> f64,
) -> Vec<PathBuf> {
    (0..count)
        .map(|i| {
            let path = dir.join(format!("{name}_{i}.fits"));
            write(&path, exposure, 60., |x, y| value(x, y, i));
            path
        })
        .collect()
}

#[test]
fn bias_scaled_dark_and_flat_recover_a_flat_sky_without_negative_samples() {
    let dir = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    let noise = |x: usize, y: usize, i: usize| ((x * 3 + y * 5 + i * 7) % 3) as f64 - 1.;
    let bias = frames(dir.path(), "Bias", 8, 0., |x, y, i| {
        bias_level(x, y) + noise(x, y, i)
    });
    let darks = frames(dir.path(), "Dark", 8, 60., |x, y, i| {
        bias_level(x, y) + thermal_rate(x, y) * 60. + noise(x, y, i)
    });
    let flats = frames(dir.path(), "Flat", 8, 2., |x, y, i| {
        bias_level(x, y) + (18000. + 500. * i as f64) * vignetting(x, y) + noise(x, y, i)
    });
    let bias = std::sync::Arc::new(
        build_master(&bias, MasterKind::Bias, CombineMethod::Median, &cancel).unwrap(),
    );
    let dark =
        std::sync::Arc::new(build_master_dark(&darks, CombineMethod::Median, &cancel).unwrap());
    let flat = std::sync::Arc::new(
        build_flat_master(
            &flats,
            Some(&bias),
            CombineMethod::Average,
            &cancel,
            &|_| {},
        )
        .unwrap(),
    );
    assert!(flat.offset_removed);
    let calibration =
        Calibration::new(Some(bias.clone()), Some(dark), Some(flat), Some(bias)).unwrap();
    assert_eq!(
        calibration.summary.formula,
        "(light - bias - (dark - bias) * t_light/t_dark) / normalized flat + pedestal"
    );
    // A 180 s light: twice... three times the dark's thermal signal.
    let light = dir.path().join("light.fits");
    write(&light, 180., 60., |x, y| {
        bias_level(x, y) + thermal_rate(x, y) * 180. + 300. * vignetting(x, y)
    });
    let mut frame = read_raw(&light).unwrap();
    assert_eq!(
        calibration.light_warnings(&frame.header.info),
        ["dark_scaled_by_exposure"]
    );
    let negative = calibration
        .apply(&mut frame, [0, 0])
        .unwrap()
        .negative_samples;
    assert_eq!(negative, 0);
    // Flat sky at the normalized level plus the bias pedestal, hot pixels gone.
    let values: Vec<f64> = frame.pixels.iter().map(|&v| f64::from(v)).collect();
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let worst = values.iter().map(|v| (v - mean).abs()).fold(0., f64::max);
    assert!(
        worst < 3.,
        "calibrated sky deviates by {worst} ADU around {mean}"
    );
    assert!((mean - (300. * 1. + 501.)).abs() < 40., "level {mean}");
}

#[test]
fn per_phase_flat_normalization_keeps_the_sensor_colour_response() {
    let dir = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    // The flat panel is strongly coloured; the sky is not.
    let lamp = [0.4, 1.0, 1.0, 0.8];
    let flats = frames(dir.path(), "Flat", 4, 1., |x, y, _| {
        500. + 20000. * phase_gain(x, y, lamp) * vignetting(x, y)
    });
    let offset = std::sync::Arc::new(
        build_master(
            &frames(dir.path(), "Bias", 4, 0., |_, _, _| 500.),
            MasterKind::Bias,
            CombineMethod::Average,
            &cancel,
        )
        .unwrap(),
    );
    let flat = std::sync::Arc::new(
        build_flat_master(
            &flats,
            Some(&offset),
            CombineMethod::Average,
            &cancel,
            &|_| {},
        )
        .unwrap(),
    );
    let calibration =
        Calibration::new(Some(offset.clone()), None, Some(flat), Some(offset)).unwrap();
    let response = [0.9, 1.0, 1.0, 0.7];
    let light = dir.path().join("light.fits");
    write(&light, 60., 60., |x, y| {
        500. + 1000. * phase_gain(x, y, response) * vignetting(x, y)
    });
    let mut frame = read_raw(&light).unwrap();
    calibration.apply(&mut frame, [0, 0]).unwrap();
    let level = |phase: usize, x: usize, y: usize| {
        f64::from(frame.pixels[(y + phase / 2) * W + x + phase % 2]) - 500.
    };
    for phase in 0..4 {
        // Normalization by each phase's median keeps the field's mean level;
        // the lamp colour is gone, the sensor response ratio remains.
        let ratio = level(phase, 30, 30) / level(1, 30, 30);
        assert!(
            (ratio - response[phase] / response[1]).abs() < 0.01,
            "phase {phase}: colour ratio {ratio}"
        );
        // And the vignetting is divided out within every phase.
        assert!(
            (level(phase, 2, 2) / level(phase, 30, 30) - 1.).abs() < 0.01,
            "phase {phase} vignetting left"
        );
    }
}

#[test]
fn saturated_raw_samples_stay_saturated_and_raw_master_flats_lose_their_offset() {
    let dir = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    let bias = std::sync::Arc::new(
        build_master(
            &frames(dir.path(), "Bias", 3, 0., |_, _, _| 500.),
            MasterKind::Bias,
            CombineMethod::Average,
            &cancel,
        )
        .unwrap(),
    );
    // A saturated hot pixel in a short dark.
    let dark = std::sync::Arc::new(
        build_master_dark(
            &frames(dir.path(), "Dark", 3, 60., |x, y, _| {
                if (x, y) == (10, 12) { 65535. } else { 520. }
            }),
            CombineMethod::Average,
            &cancel,
        )
        .unwrap(),
    );
    // An integer master flat from another program still contains the offset.
    let raw_flat = dir.path().join("MasterFlat_int.fits");
    write(&raw_flat, 1., 60., |x, y| 500. + 20000. * vignetting(x, y));
    let flat = std::sync::Arc::new(load_master(&raw_flat, MasterKind::Flat, &cancel).unwrap());
    assert!(
        !flat.offset_removed,
        "integer master flats are treated as raw"
    );
    let calibration =
        Calibration::new(Some(bias.clone()), Some(dark), Some(flat), Some(bias)).unwrap();
    assert!(calibration.summary.flat_offset_subtracted_here);
    let light = dir.path().join("light.fits");
    write(&light, 300., 60., |x, y| {
        if (x, y) == (10, 12) {
            65535.
        } else {
            500. + 20. * 5. + 400. * vignetting(x, y)
        }
    });
    let mut frame = read_raw(&light).unwrap();
    let negative = calibration
        .apply(&mut frame, [0, 0])
        .unwrap()
        .negative_samples;
    assert_eq!(
        negative, 0,
        "the exposure-scaled saturated dark is not subtracted"
    );
    assert_eq!(frame.pixels[12 * W + 10], 65535.);
    // A 14-bit sensor clips at its own limit and is kept there.
    let mut low = read_raw(&light).unwrap();
    low.pixels[12 * W + 10] = 16383.;
    low.header.info.saturation_adu = Some(16383.);
    let negative = calibration
        .apply(&mut low, [0, 0])
        .unwrap()
        .negative_samples;
    assert_eq!(negative, 0);
    assert_eq!(low.pixels[12 * W + 10], 16383.);
    // With the offset removed from the flat, vignetting divides out exactly.
    let centre = f64::from(frame.pixels[35 * W + 32]);
    let corner = f64::from(frame.pixels[2 * W + 2]);
    assert!((corner / centre - 1.).abs() < 0.003, "{corner} vs {centre}");
    // Without a bias the dark is subtracted unscaled and the light is warned.
    let unscaled = Calibration::new(None, calibration.dark.clone(), None, None).unwrap();
    assert_eq!(
        unscaled.light_warnings(&frame.header.info),
        ["dark_exposure_differs"]
    );
    // Another gain than the dark's also only warns (ADR-080).
    let summary = &unscaled.dark.as_ref().unwrap().summary;
    let mut other_gain = frame.header.info.clone();
    other_gain.exposure_seconds = summary.exposure_seconds;
    other_gain.gain = Some(summary.gain.expect("test darks carry a gain") + 50.);
    assert_eq!(unscaled.light_warnings(&other_gain), ["dark_gain_differs"]);
    assert_eq!(unscaled.summary.formula, "light - dark + pedestal");
}

#[test]
fn dark_hot_pixels_missing_from_the_light_are_repaired_not_left_negative() {
    let dir = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    // An aged dark library: a strong hot pixel that the light no longer has,
    // and a hot pixel the light still shows.
    let dark = std::sync::Arc::new(
        build_master_dark(
            &frames(dir.path(), "Dark", 3, 60., |x, y, _| match (x, y) {
                (20, 20) => 3000.,
                (40, 30) => 2500.,
                _ => 200.,
            }),
            CombineMethod::Average,
            &cancel,
        )
        .unwrap(),
    );
    let calibration = Calibration::new(None, Some(dark), None, None).unwrap();
    let light = dir.path().join("light.fits");
    write(&light, 60., 60., |x, y| match (x, y) {
        (40, 30) => 2510.,
        _ => 210. + (x % 3) as f64,
    });
    let mut frame = read_raw(&light).unwrap();
    let stats = calibration.apply(&mut frame, [0, 0]).unwrap();
    assert_eq!(stats.negative_samples, 0);
    assert_eq!(
        stats.repaired_dark_defects, 1,
        "only the vanished hot pixel"
    );
    // Replaced by its same-phase neighbours (x +-2 keeps x % 3 varying).
    let repaired = frame.pixels[20 * W + 20];
    assert!(
        (repaired - (210. + 1. + 200.) + 200.).abs() < 2.5,
        "{repaired}"
    );
    // The persisting hot pixel is calibrated normally.
    assert_eq!(frame.pixels[30 * W + 40], 2510. - 2500. + 200.);
}

#[test]
fn hot_pixel_map_takes_the_brightest_share_and_replaces_them_from_same_colour_neighbours() {
    let dir = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    let files = darks(dir.path(), 7, 60.);
    // 3 of 4,480 pixels are 0.067 %; the cosmic ray of frame 2 is not a hot pixel.
    let map = HotPixelMap::from_frames(&files, 0.066, &cancel).unwrap();
    assert_eq!(map.len(), 3);
    for (x, y) in [(5, 7), (40, 33), (63, 69)] {
        assert!(map.contains_pixel(x, y), "({x},{y}) missing");
    }
    assert!(HotPixelMap::from_frames(&files, 0., &cancel).is_err());
    assert!(HotPixelMap::from_frames(&files, 6., &cancel).is_err());

    // A light with the three map pixels hot, and a fourth hot pixel that is not mapped.
    let light = dir.path().join("light.fits");
    let value = |x: usize, y: usize| {
        let hot = [(5, 7), (40, 33), (63, 69), (20, 20)].contains(&(x, y));
        1000. + (x % 7) as f64 + if hot { 3000. } else { 0. }
    };
    write(&light, 60., 60., value);
    let mut frame = read_raw(&light).unwrap();
    assert_eq!(map.repair(&mut frame, [0, 0]), 3);
    for (x, y) in [(5, 7), (40, 33), (63, 69)] {
        let got = f64::from(frame.pixels[y * W + x]);
        assert!((1000. ..1007.).contains(&got), "({x},{y}) became {got}");
    }
    assert!(
        frame.pixels[20 * W + 20] > 4000.,
        "an unmapped pixel is kept"
    );
    // Neighbours of the repaired pixels are untouched.
    assert_eq!(frame.pixels[7 * W + 6], value(6, 7) as f32);

    // A ROI at an origin gives the same replacement as the full frame.
    let full = read_raw(&light).unwrap();
    let mut whole = read_raw(&light).unwrap();
    map.repair(&mut whole, [0, 0]);
    let roi = fulgur_core::fits::read_roi(
        &light,
        &read_header(&light).unwrap(),
        [32, 20, 32, 40],
        &cancel,
    )
    .unwrap();
    let mut roi_frame = roi;
    assert_eq!(map.repair(&mut roi_frame, [32, 20]), 1);
    assert_eq!(
        roi_frame.pixels[(33 - 20) * 32 + (40 - 32)],
        whole.pixels[33 * W + 40]
    );
    drop(full);

    // A saved map reads back identically and is never overwritten.
    let path = dir.path().join("hot.fit");
    map.save(&path, &cancel).unwrap();
    assert!(is_hot_pixel_map(&path));
    assert!(map.save(&path, &cancel).is_err());
    let loaded = HotPixelMap::load(&path).unwrap();
    assert_eq!((loaded.width, loaded.height, loaded.len()), (W, H, 3));
    assert!(loaded.contains_pixel(40, 33));
    assert!(!is_hot_pixel_map(&files[0]));
    // Inputs that are a map file load it, anything else is built from frames.
    let again = HotPixelMap::from_inputs(std::slice::from_ref(&path), 0.5, &cancel).unwrap();
    assert_eq!(again.len(), 3);

    // Through a calibration: the formula and report name it, apply replaces.
    let calibration = Calibration::new(None, None, None, None)
        .unwrap()
        .with_hot_pixels(std::sync::Arc::new(loaded))
        .unwrap();
    assert_eq!(calibration.summary.formula, "light with hot pixel map");
    assert_eq!(calibration.summary.hot_pixels.as_ref().unwrap().pixels, 3);
    let mut frame = read_raw(&light).unwrap();
    let stats = calibration.apply(&mut frame, [0, 0]).unwrap();
    assert_eq!(stats.replaced_hot_pixels, 3);
}
