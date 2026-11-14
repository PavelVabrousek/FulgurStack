use super::*;
fn gaussian(sx: f64, sy: f64, phase_x: f64, phase_y: f64) -> (Vec<f32>, f64, f64) {
    let (cx, cy) = (64.0 + phase_x, 64.0 + phase_y);
    let pixels = (0..128 * 128)
        .map(|i| {
            let (x, y) = ((i % 128) as f64, (i / 128) as f64);
            (2000.0
                + 10000.0
                    * (-0.5 * ((x - cx).powi(2) / sx.powi(2) + (y - cy).powi(2) / sy.powi(2)))
                        .exp()) as f32
        })
        .collect();
    (pixels, cx, cy)
}
/// Reproducible scattered values.
fn scatter(count: usize, seed: usize) -> Vec<f32> {
    (0..count)
        .map(|i| ((i * 7919 + seed) * 104729 % 65521) as f32 * 0.03125)
        .collect()
}

#[test]
fn channel_sky_reads_the_same_samples_as_a_whole_plane() {
    let (width, height) = (301, 257);
    let pixels = scatter(width * height, 3);
    for pattern in [
        BayerPattern::RGGB,
        BayerPattern::BGGR,
        BayerPattern::GRBG,
        BayerPattern::GBRG,
    ] {
        let (gw, gh) = (width / 2, height / 2);
        let plane = |color: Color| {
            let (px, py) = [(0, 0), (1, 0), (0, 1), (1, 1)]
                .into_iter()
                .find(|&(x, y)| pattern.color(x, y) == color)
                .unwrap();
            let samples: Vec<f32> = (0..gh)
                .flat_map(|y| (0..gw).map(move |x| (2 * y + py) * width + 2 * x + px))
                .map(|i| pixels[i])
                .collect();
            Sky::new(&samples, gw, gh).background
        };
        let sky = channel_sky(&pixels, width, height, pattern, 7.5);
        assert_eq!(
            sky.map(f64::to_bits),
            [plane(Color::Red), 7.5, plane(Color::Blue)].map(f64::to_bits)
        );
    }
}

#[test]
fn suppressed_defects_leave_the_grid_of_the_copy() {
    let cancel = AtomicBool::new(false);
    for (width, height) in [(64, 64), (65, 67)] {
        let pixels = scatter(width * height, 11);
        let mut fixes: Vec<(usize, f32)> = (0..40)
            .map(|n| ((n * 397 + 5) % pixels.len(), n as f32))
            .collect();
        // Last row and column, which odd sizes leave outside the grid.
        fixes.push((width * height - 1, 1.5));
        fixes.push((width - 1, 2.5));
        for pattern in [None, Some(BayerPattern::RGGB), Some(BayerPattern::GRBG)] {
            let mut grid = detection_grid(&pixels, width, pattern, &cancel).unwrap();
            let mut copy = vec![9.; 3];
            suppress(&pixels, width, pattern, &fixes, &mut grid, &mut copy);
            let mut expected = pixels.clone();
            for &(i, value) in &fixes {
                expected[i] = value;
            }
            assert_eq!(copy, expected);
            assert_eq!(
                grid,
                detection_grid(&expected, width, pattern, &cancel).unwrap()
            );
        }
    }
}

#[test]
fn flux_ranks_stars_by_total_light_and_keeps_clipped_centroids_true() {
    let (width, height) = (512, 512);
    let mut pixels: Vec<f32> = (0..width * height)
        .map(|i| 1000. + ((i * 7919) % 31) as f32 - 15.)
        .collect();
    // Clipped star at (100.3, 120.7), wider and far brighter than the rest.
    let truth = [
        (100.3_f64, 120.7_f64, 900_000., 3.),
        (300.2, 300.4, 30_000., 1.6),
        (400.6, 80.1, 15_000., 1.6),
    ];
    for y in 0..height {
        for x in 0..width {
            let mut value = f64::from(pixels[y * width + x]);
            for (cx, cy, amplitude, sigma) in truth {
                let r2 = ((x as f64 - cx).powi(2) + (y as f64 - cy).powi(2)) / (sigma * sigma);
                value += amplitude * (-0.5 * r2).exp();
            }
            pixels[y * width + x] = value.min(65535.) as f32;
        }
    }
    let cancel = AtomicBool::new(false);
    let grid = detection_grid(&pixels, width, None, &cancel).unwrap();
    let sky = Sky::new(&grid, 256, 256);
    let stars = flux_stars(&grid, (256, 256), &pixels, width, &sky, 62000., &cancel).unwrap();
    assert_eq!(stars.len(), 3);
    assert!(stars[0].saturated && !stars[1].saturated && !stars[2].saturated);
    for (star, (x, y, _, _)) in stars.iter().zip(truth) {
        assert!(
            (star.x_px - x).abs() < 0.2 && (star.y_px - y).abs() < 0.2,
            "{}",
            star.x_px
        );
    }
    assert!(stars[0].flux > stars[1].flux && stars[1].flux > stars[2].flux);
    let catalog = dense_catalog(&stars, (width, height), |_, _| None);
    assert_eq!(
        catalog.len(),
        3,
        "geometry keeps positions without a measurement"
    );
    assert!(
        catalog.iter().all(|s| s.flux_adu == 0.),
        "no photometry from positions"
    );
    assert!(!is_dense(19_999) && is_dense(20_000));
}

#[test]
fn stars_clipped_by_a_low_sensor_limit_leave_the_catalog() {
    let (width, height) = (512, 512);
    let mut pixels: Vec<f32> = (0..width * height)
        .map(|i| 2000. + ((i * 7919) % 31) as f32 - 15.)
        .collect();
    for (index, (cx, cy)) in (0..16)
        .map(|i| (64 + (i % 4) * 128, 64 + (i / 4) * 128))
        .enumerate()
    {
        // The first four stars overflow 14 bits and clip at 16383.
        let amplitude = if index < 4 { 90000. } else { 8000. };
        for y in cy - 16..cy + 16 {
            for x in cx - 16..cx + 16 {
                let r2 = ((x as f32 - cx as f32).powi(2) + (y as f32 - cy as f32).powi(2)) / 4.;
                let value = &mut pixels[y * width + x];
                *value = (*value + amplitude * (-0.5 * r2).exp()).min(16383.);
            }
        }
    }
    let cancel = AtomicBool::new(false);
    let scan = |saturation| {
        scan_plane(
            &pixels,
            width,
            height,
            None,
            &cancel,
            250,
            false,
            saturation,
            &mut Vec::new(),
        )
        .unwrap()
    };
    let brightest = |m: &Metrics| m.stars.iter().map(|s| s.peak_adu).fold(0., f64::max);
    assert!(
        brightest(&scan(None)) > 16000.,
        "fixed 16-bit limit misses them"
    );
    let level = fulgur_core::fits::saturation_level(&pixels, 16);
    assert_eq!(level, Some(16383.));
    let limited = scan(level);
    assert!(brightest(&limited) < 15000., "{}", brightest(&limited));
    assert_eq!(limited.stars.len(), 12);
    assert_eq!(fulgur_core::fits::saturation_level(&pixels, -32), None);
    let mut full = pixels.clone();
    full[0] = 65535.;
    assert_eq!(fulgur_core::fits::saturation_level(&full, 16), None);
    let mut few = pixels.clone();
    for value in few.iter_mut().filter(|v| **v >= 16383.).skip(3) {
        *value = 16000.;
    }
    assert_eq!(fulgur_core::fits::saturation_level(&few, 16), None);
}

#[test]
fn channel_sky_finds_each_colour_on_every_bayer_pattern() {
    let (width, height) = (320, 240);
    for pattern in [
        BayerPattern::RGGB,
        BayerPattern::GRBG,
        BayerPattern::GBRG,
        BayerPattern::BGGR,
    ] {
        let mut pixels: Vec<f32> = (0..width * height)
            .map(|i| {
                let (x, y) = (i % width, i / width);
                let level = match pattern.color(x, y) {
                    Color::Red => 500.,
                    Color::Green => 1000.,
                    Color::Blue => 300.,
                };
                level + ((x * 71 + y * 23 + x * y * 11) % 29) as f32 - 14.
            })
            .collect();
        // Stars and hot pixels do not move a robust sky level.
        for i in (0..pixels.len()).step_by(97) {
            pixels[i] = 40000.;
        }
        let [r, g, b] = channel_sky(&pixels, width, height, pattern, 1001.);
        assert_eq!(g, 1001., "green stays the detection level");
        assert!((r - 500.).abs() < 3. && (b - 300.).abs() < 3., "{r} {b}");
    }
}
#[test]
fn external_reference_centroids_support_rendered_33_and_300_percent_stars() {
    let sources: Vec<_> = (0..25)
        .map(|i| Star {
            x_px: 50.2 + (i % 5) as f64 * 90. + ((i * 17) % 19) as f64,
            y_px: 50.3 + (i / 5) as f64 * 90. + ((i * 11) % 17) as f64,
            hfr_px: 1.7,
            moment_fwhm_px: 3.4,
            eccentricity: 0.,
            flux_adu: 10000.,
            peak_adu: 10000.,
            snr: 100.,
        })
        .collect();
    let cancel = AtomicBool::new(false);
    for scale in [0.30, 0.33, 3., 1. / 0.30] {
        let width = (512. * scale) as usize;
        let mut pixels: Vec<_> = (0..width * width)
            .map(|i| 0.05 + ((i * 17 + i / width * 11) % 23) as f32 * 0.000001)
            .collect();
        for star in &sources {
            let (cx, cy) = (scale * star.x_px, scale * star.y_px);
            let radius = (6. * scale).ceil() as usize + 2;
            for y in (cy as usize).saturating_sub(radius)..((cy as usize + radius + 1).min(width)) {
                for x in
                    (cx as usize).saturating_sub(radius)..((cx as usize + radius + 1).min(width))
                {
                    pixels[y * width + x] += (0.5
                        * (-((x as f64 - cx).powi(2) + (y as f64 - cy).powi(2))
                            / (2. * (1.4 * scale).powi(2)))
                        .exp()) as f32;
                }
            }
        }
        let plane = fulgur_core::fits::ReferencePlane {
            width,
            height: width,
            pixels,
            object: None,
            filter: None,
        };
        let target = reference_catalog(&plane, &cancel).unwrap();
        let fit = crate::geometry::solve_similarity(&sources, &target, None, &cancel)
            .unwrap()
            .expect("rendered endpoint reference");
        assert!((fit.transform.scale_factor - scale).abs() < 0.001);
        assert!(fit.rmse < 0.15, "scale {scale}: {}", fit.rmse);
        assert!(fit.pairs.len() >= 20);
    }
}
#[test]
fn true_hfr_recovers_gaussian_with_subpixel_phases() {
    let mut maximum_error = 0.0_f64;
    for pattern in [
        BayerPattern::GRBG,
        BayerPattern::RGGB,
        BayerPattern::GBRG,
        BayerPattern::BGGR,
    ] {
        for sigma in [1.25, 1.5, 2.0] {
            for phase_x in [0.0, 0.25, 0.5, 0.75] {
                for phase_y in [0.0, 0.25, 0.5, 0.75] {
                    let (raw, x, y) = gaussian(sigma, sigma, phase_x, phase_y);
                    let plane = Green::new(&raw, 128, 128, pattern);
                    let star = plane.measure(x, y, 2000.0, 0.0).unwrap();
                    let expected = (2.0 * 2.0_f64.ln()).sqrt() * sigma;
                    let error = (star.hfr_px / expected - 1.0).abs();
                    maximum_error = maximum_error.max(error);
                    assert!(
                        error < 0.03,
                        "pattern={pattern:?} sigma={sigma}, phase={phase_x},{phase_y}, hfr={}, expected={expected}",
                        star.hfr_px
                    );
                    assert!((star.x_px - x).abs() < 0.05 && (star.y_px - y).abs() < 0.05);
                    assert!(star.eccentricity < 0.15);
                }
            }
        }
    }
    println!("HFR gate: 192 Gaussian/CFA/phase cases, maximum relative error={maximum_error:.8}");
}
#[test]
fn mono_plane_recovers_gaussian_hfr_centroid_and_shape() {
    let mut maximum_error = 0.0_f64;
    for sigma in [1.25, 1.5, 2.0] {
        for phase_x in [0.0, 0.25, 0.5, 0.75] {
            for phase_y in [0.0, 0.25, 0.5, 0.75] {
                let (raw, x, y) = gaussian(sigma, sigma, phase_x, phase_y);
                let plane = Green::mono(&raw, 128, 128);
                let star = plane.measure(x, y, 2000.0, 0.0).unwrap();
                let expected = (2.0 * 2.0_f64.ln()).sqrt() * sigma;
                let error = (star.hfr_px / expected - 1.0).abs();
                maximum_error = maximum_error.max(error);
                assert!(
                    error < 0.03,
                    "sigma={sigma}, phase={phase_x},{phase_y}, hfr={}, expected={expected}",
                    star.hfr_px
                );
                assert!((star.x_px - x).abs() < 0.05 && (star.y_px - y).abs() < 0.05);
                assert!(star.eccentricity < 0.15);
            }
        }
    }
    let (raw, x, y) = gaussian(2.0, 1.5, 0.25, 0.5);
    let star = Green::mono(&raw, 128, 128)
        .measure(x, y, 2000.0, 0.0)
        .unwrap();
    let expected = (1.0_f64 - 1.5_f64.powi(2) / 2.0_f64.powi(2)).sqrt();
    assert!((star.eccentricity - expected).abs() < 0.03);
    println!("Mono HFR gate: 48 Gaussian/phase cases, maximum relative error={maximum_error:.8}");
}
#[test]
fn mono_frames_are_analyzed_on_the_full_plane_and_unknown_layouts_are_refused() {
    // Headers without BAYERPAT are mono; an explicit Unknown is still refused.
    use fulgur_core::fits::ImageHeader;
    let mut bytes = Vec::new();
    for (key, value) in [
        ("SIMPLE", "T"),
        ("BITPIX", "16"),
        ("NAXIS", "2"),
        ("NAXIS1", "256"),
        ("NAXIS2", "256"),
        ("EXPTIME", "10"),
    ] {
        bytes.extend(format!("{:<80}", format!("{key:<8}= {value:>20}")).as_bytes());
    }
    bytes.extend(format!("{:<80}", "END").as_bytes());
    bytes.resize(2880, b' ');
    let mut header = ImageHeader::read(
        &mut std::io::Cursor::new(bytes),
        (2880 + (256 * 256 * 2_usize).div_ceil(2880) * 2880) as u64,
    )
    .unwrap();
    assert_eq!(header.info.mosaic, Mosaic::Mono);
    header.info.mosaic = Mosaic::Unknown;
    let centres: Vec<(f64, f64)> = (0..25)
        .map(|i| (40.3 + (i % 5) as f64 * 42., 41.7 + (i / 5) as f64 * 42.))
        .collect();
    let pixels: Vec<f32> = (0..256 * 256)
        .map(|i| {
            let (x, y) = ((i % 256) as f64, (i / 256) as f64);
            let noise = ((i * 7919) % 97) as f64 / 97. * 8.;
            let mut v = 1000. + noise;
            for (cx, cy) in &centres {
                v += 9000. * (-0.5 * ((x - cx).powi(2) + (y - cy).powi(2)) / 1.6_f64.powi(2)).exp();
            }
            v as f32
        })
        .collect();
    let cancel = AtomicBool::new(false);
    let unknown = RawFrame {
        header: header.clone(),
        pixels: pixels.clone(),
    };
    assert!(analyze(&unknown, &cancel).is_err());
    header.info.apply_layout(fulgur_core::LayoutChoice::Mono);
    let metrics = analyze(&RawFrame { header, pixels }, &cancel).unwrap();
    assert_eq!(metrics.stars.len(), 25, "every rendered star is measured");
    for star in &metrics.stars {
        let nearest = centres
            .iter()
            .map(|(cx, cy)| ((star.x_px - cx).powi(2) + (star.y_px - cy).powi(2)).sqrt())
            .fold(f64::INFINITY, f64::min);
        assert!(nearest < 0.1, "mono centroid error {nearest}");
        let expected = (2.0 * 2.0_f64.ln()).sqrt() * 1.6;
        assert!((star.hfr_px / expected - 1.0).abs() < 0.05);
    }
}
#[test]
fn a_longer_list_than_sixteen_cells_of_64_measures_more_stars() {
    // 2500 stars on a 40 px grid; the usual lists keep their size (ADR-125).
    use fulgur_core::fits::ImageHeader;
    let size = 2048_usize;
    let mut bytes = Vec::new();
    for (key, value) in [
        ("SIMPLE", "T".to_owned()),
        ("BITPIX", "16".to_owned()),
        ("NAXIS", "2".to_owned()),
        ("NAXIS1", size.to_string()),
        ("NAXIS2", size.to_string()),
        ("EXPTIME", "10".to_owned()),
    ] {
        bytes.extend(format!("{:<80}", format!("{key:<8}= {value:>20}")).as_bytes());
    }
    bytes.extend(format!("{:<80}", "END").as_bytes());
    bytes.resize(2880, b' ');
    let mut header = ImageHeader::read(
        &mut std::io::Cursor::new(bytes),
        (2880 + (size * size * 2).div_ceil(2880) * 2880) as u64,
    )
    .unwrap();
    header.info.apply_layout(fulgur_core::LayoutChoice::Mono);
    let mut pixels: Vec<f32> = (0..size * size)
        .map(|i| 1000. + ((i * 7919) % 97) as f32 / 97. * 8.)
        .collect();
    for j in 0..50 {
        for i in 0..50 {
            let (cx, cy) = (40.3 + i as f64 * 40., 41.7 + j as f64 * 40.);
            let peak = 3000. + ((i * 31 + j * 17) % 40) as f64 * 150.;
            for y in (cy as usize - 8)..(cy as usize + 9) {
                for x in (cx as usize - 8)..(cx as usize + 9) {
                    let r2 = (x as f64 - cx).powi(2) + (y as f64 - cy).powi(2);
                    pixels[y * size + x] += (peak * (-0.5 * r2 / 1.6_f64.powi(2)).exp()) as f32;
                }
            }
        }
    }
    let frame = RawFrame { header, pixels };
    let cancel = AtomicBool::new(false);
    let count = |limit| {
        analyze_with_limit(&frame, &cancel, limit)
            .unwrap()
            .stars
            .len()
    };
    assert_eq!(count(250), 250);
    assert_eq!(count(1000), 1000);
    assert!(count(5000) > 2000, "{}", count(5000));
}

#[test]
fn hot_pixels_are_refused_while_sharp_and_broad_stars_are_kept() {
    use fulgur_core::fits::ImageHeader;
    let cancel = AtomicBool::new(false);
    for bayer in [Some("RGGB"), Some("GRBG"), None] {
        let mut bytes = Vec::new();
        let mut cards = vec![
            ("SIMPLE", "T".to_owned()),
            ("BITPIX", "16".to_owned()),
            ("NAXIS", "2".to_owned()),
            ("NAXIS1", "256".to_owned()),
            ("NAXIS2", "256".to_owned()),
            ("EXPTIME", "60".to_owned()),
        ];
        if let Some(pattern) = bayer {
            cards.push(("BAYERPAT", format!("'{pattern}'")));
        }
        for (key, value) in cards {
            bytes.extend(format!("{:<80}", format!("{key:<8}= {value:>20}")).as_bytes());
        }
        bytes.extend(format!("{:<80}", "END").as_bytes());
        bytes.resize(2880, b' ');
        let mut header = ImageHeader::read(
            &mut std::io::Cursor::new(bytes),
            (2880 + (256 * 256 * 2_usize).div_ceil(2880) * 2880) as u64,
        )
        .unwrap();
        if bayer.is_none() {
            header.info.apply_layout(fulgur_core::LayoutChoice::Mono);
        }
        let pattern = header.info.bayer_pattern;
        // Alternate undersampled (sigma 1.0) and broad (sigma 1.8) stars.
        let centres: Vec<(f64, f64, f64)> = (0..16)
            .map(|i| {
                let sigma = if i % 2 == 0 { 1.0 } else { 1.8 };
                (
                    40.3 + (i % 4) as f64 * 58.,
                    38.6 + (i / 4) as f64 * 58.,
                    sigma,
                )
            })
            .collect();
        let mut pixels: Vec<f32> = (0..256 * 256)
            .map(|i| {
                let (x, y) = ((i % 256) as f64, (i / 256) as f64);
                let mut v = 200. + ((i * 7919) % 97) as f64 / 97. * 8.;
                for (cx, cy, sigma) in &centres {
                    v += 6000.
                        * (-0.5 * ((x - cx).powi(2) + (y - cy).powi(2)) / sigma.powi(2)).exp();
                }
                v as f32
            })
            .collect();
        // Uncalibrated defect clusters between the stars: one bright
        // plane sample plus two warm same-lattice defects in adjacent
        // detection cells, which satisfy the 3x3 cell-support gate.
        let mut planted = 0;
        for i in 0..9 {
            let (mut x, y) = (68 + (i % 3) * 58, 66 + (i / 3) * 58);
            if pattern.is_some_and(|p| p.color(x, y) != Color::Green) {
                x += 1;
            }
            pixels[y * 256 + x] += 3800.;
            pixels[y * 256 + x + 2] += 300.;
            pixels[(y + 2) * 256 + x] += 300.;
            planted += 1;
        }
        // Defects inside the apertures of the sharp stars would pull the
        // flux-weighted centroid by about a pixel if left in place.
        for (cx, cy, _) in centres.iter().step_by(2) {
            let (mut x, y) = (*cx as usize + 4, *cy as usize);
            if pattern.is_some_and(|p| p.color(x, y) != Color::Green) {
                x += 1;
            }
            pixels[y * 256 + x] += 3800.;
        }
        let metrics = analyze(&RawFrame { header, pixels }, &cancel).unwrap();
        // The bright sample and both warm satellites are suppressed.
        assert_eq!(
            metrics.hot_pixels,
            3 * planted + centres.len() / 2,
            "{bayer:?}: every defect is suppressed"
        );
        assert_eq!(
            metrics.defect_detections, 0,
            "{bayer:?}: no defect reaches detection"
        );
        assert_eq!(metrics.stars.len(), 16, "{bayer:?}: every star is kept");
        for star in &metrics.stars {
            let nearest = centres
                .iter()
                .map(|(cx, cy, _)| (star.x_px - cx).hypot(star.y_px - cy))
                .fold(f64::INFINITY, f64::min);
            assert!(nearest < 0.1, "{bayer:?}: centroid error {nearest}");
        }
    }
}
#[test]
fn moment_eccentricity_recovers_elongated_star() {
    let (raw, x, y) = gaussian(2.0, 1.5, 0.25, 0.5);
    let plane = Green::new(&raw, 128, 128, BayerPattern::GRBG);
    let star = plane.measure(x, y, 2000.0, 0.0).unwrap();
    let expected = (1.0_f64 - (1.5_f64 / 2.0).powi(2)).sqrt();
    assert!((star.eccentricity - expected).abs() < 0.03);
}
#[test]
fn robust_sky_uses_pixel_noise_not_noise_of_cell_medians() {
    let grid: Vec<_> = (0..128 * 128)
        .map(|i| 2000.0 + ((i * 17 + i / 128 * 13) % 31) as f32 - 15.0)
        .collect();
    let sky = Sky::new(&grid, 128, 128);
    assert!((sky.background - 2000.0).abs() < 2.0);
    assert!(sky.noise > 8.0 && sky.noise < 15.0);
}

#[test]
fn witness_positions_include_saturated_stars_the_catalog_skips() {
    use fulgur_core::fits::ImageHeader;
    let mut bytes = Vec::new();
    for (key, value) in [
        ("SIMPLE", "T"),
        ("BITPIX", "16"),
        ("NAXIS", "2"),
        ("NAXIS1", "256"),
        ("NAXIS2", "256"),
        ("EXPTIME", "10"),
    ] {
        bytes.extend(format!("{:<80}", format!("{key:<8}= {value:>20}")).as_bytes());
    }
    bytes.extend(format!("{:<80}", "END").as_bytes());
    bytes.resize(2880, b' ');
    let header = ImageHeader::read(
        &mut std::io::Cursor::new(bytes),
        (2880 + (256 * 256 * 2_usize).div_ceil(2880) * 2880) as u64,
    )
    .unwrap();
    // Sixteen stars; the first four have saturated cores.
    let centres: Vec<(f64, f64, f64)> = (0..16)
        .map(|i| {
            let amplitude = if i < 4 { 200000. } else { 8000. };
            (
                40.3 + (i % 4) as f64 * 58.,
                38.6 + (i / 4) as f64 * 58.,
                amplitude,
            )
        })
        .collect();
    let pixels: Vec<f32> = (0..256 * 256)
        .map(|i| {
            let (x, y) = ((i % 256) as f64, (i / 256) as f64);
            let mut v = 1000. + ((i * 7919) % 97) as f64 / 97. * 8.;
            for (cx, cy, a) in &centres {
                v += a * (-0.5 * ((x - cx).powi(2) + (y - cy).powi(2)) / 1.6_f64.powi(2)).exp();
            }
            v.min(65535.) as f32
        })
        .collect();
    let frame = RawFrame { header, pixels };
    let cancel = AtomicBool::new(false);
    assert_eq!(analyze(&frame, &cancel).unwrap().stars.len(), 12);
    let witnesses = witness_positions(&frame, &cancel, 5000).unwrap();
    assert_eq!(witnesses.len(), 16);
    for (cx, cy, _) in &centres {
        let nearest = witnesses
            .iter()
            .map(|s| (s.x_px - cx).hypot(s.y_px - cy))
            .fold(f64::INFINITY, f64::min);
        assert!(nearest < 1.5, "witness position error {nearest}");
    }
    assert_eq!(witness_positions(&frame, &cancel, 5).unwrap().len(), 5);
}
