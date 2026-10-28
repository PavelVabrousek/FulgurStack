use fulgur_core::fits::FitsHeader;
use fulgur_core::{BayerPattern, Color, RawFrame};
use fulgur_pipeline::debayer::{cosmetic_correct, mhc_demosaic};
use fulgur_pipeline::vng::vng_demosaic;
use std::io::Cursor;
use std::sync::atomic::AtomicBool;

#[test]
fn monochrome_frames_skip_demosaicing_and_unknown_layouts_are_refused() {
    use fulgur_core::{DemosaicMethod, LayoutChoice, Mosaic};
    let mut input = frame(BayerPattern::GRBG, 24, 24, |x, y, _| {
        1000. + (x * 3 + y * 5) as f32 + if (x, y) == (12, 12) { 9000. } else { 0. }
    });
    input.header.info.bayer_pattern = None;
    input.header.info.mosaic = Mosaic::Unknown;
    let cancel = AtomicBool::new(false);
    assert!(fulgur_pipeline::debayer::debayer(&input, &cancel, DemosaicMethod::Vng).is_err());
    input.header.info.apply_layout(LayoutChoice::Mono);
    let (rgb, stats) =
        fulgur_pipeline::debayer::debayer(&input, &cancel, DemosaicMethod::Vng).unwrap();
    assert_eq!(rgb.demosaic_method, DemosaicMethod::None);
    assert_eq!(stats.demosaic_method, DemosaicMethod::None);
    assert_eq!(stats.corrected_pixels, 1, "isolated hot pixel corrected");
    let (cleaned, _) = cosmetic_correct(&input, &cancel).unwrap();
    for (pixel, sample) in rgb.pixels.iter().zip(&cleaned) {
        assert_eq!(
            *pixel, [*sample; 3],
            "every channel is the corrected sample"
        );
    }
    assert!(vng_demosaic(&input, &input.pixels, &cancel).is_err());
    assert!(mhc_demosaic(&input, &input.pixels, &cancel).is_err());
}

fn frame(
    pattern: BayerPattern,
    width: usize,
    height: usize,
    signal: impl Fn(usize, usize, Color) -> f32,
) -> RawFrame {
    let mut bytes = Vec::new();
    for (key, val) in [
        ("SIMPLE", "T".to_owned()),
        ("BITPIX", "16".to_owned()),
        ("NAXIS", "2".to_owned()),
        ("NAXIS1", width.to_string()),
        ("NAXIS2", height.to_string()),
        ("EXPTIME", "10".to_owned()),
        ("BAYERPAT", format!("'{pattern:?}'")),
    ] {
        bytes.extend(format!("{:<80}", format!("{key:<8}= {val:>20}")).as_bytes());
    }
    bytes.extend(format!("{:<80}", "END").as_bytes());
    bytes.resize(2880, b' ');
    let header = FitsHeader::read(
        &mut Cursor::new(bytes),
        (2880 + (width * height * 2).div_ceil(2880) * 2880) as u64,
    )
    .unwrap();
    let mut pixels = Vec::new();
    for y in 0..height {
        for x in 0..width {
            pixels.push(signal(x, y, pattern.color(x, y)));
        }
    }
    RawFrame { header, pixels }
}

#[test]
fn all_bayer_phases_reconstruct_distinct_constant_colors_including_edges() {
    for pattern in [
        BayerPattern::GRBG,
        BayerPattern::RGGB,
        BayerPattern::GBRG,
        BayerPattern::BGGR,
    ] {
        let frame = frame(pattern, 17, 19, |_, _, color| match color {
            Color::Red => 1500.125,
            Color::Green => 1000.25,
            Color::Blue => 700.5,
        });
        let rgb = mhc_demosaic(&frame, &frame.pixels, &AtomicBool::new(false)).unwrap();
        for pixel in rgb.pixels {
            assert_eq!(pixel, [1500.125, 1000.25, 700.5]);
        }
    }
}

#[test]
fn linear_rgb_gradients_reconstruct_without_channel_or_axis_swaps() {
    let expected = |x: usize, y: usize| {
        [
            1500.0 + 2.0 * x as f32 + 3.0 * y as f32,
            1000.0 + 4.0 * x as f32 - y as f32,
            700.0 - 3.0 * x as f32 + 2.0 * y as f32,
        ]
    };
    let frame = frame(BayerPattern::GRBG, 24, 20, |x, y, color| {
        expected(x, y)[match color {
            Color::Red => 0,
            Color::Green => 1,
            Color::Blue => 2,
        }]
    });
    let rgb = mhc_demosaic(&frame, &frame.pixels, &AtomicBool::new(false)).unwrap();
    for y in 2..18 {
        for x in 2..22 {
            assert_eq!(rgb.pixels[y * 24 + x], expected(x, y));
        }
    }
}

#[test]
fn mhc_impulse_matches_independent_published_coefficients_without_clipping() {
    let frame = frame(BayerPattern::GRBG, 12, 12, |x, y, _| {
        if x == 5 && y == 4 { 8.0 } else { 0.0 }
    });
    let rgb = mhc_demosaic(&frame, &frame.pixels, &AtomicBool::new(false)).unwrap();
    assert_eq!(rgb.pixels[4 * 12 + 5], [8.0, 4.0, 6.0]);
    assert_eq!(rgb.pixels[4 * 12 + 4], [4.0, 0.0, 0.0]);
    assert!(rgb.pixels.iter().flatten().any(|value| *value < 0.0));
}

#[test]
fn sensor_samples_are_preserved_exactly_by_mhc() {
    let frame = frame(BayerPattern::GRBG, 16, 16, |x, y, _| {
        ((x * 37 + y * 113) % 1000) as f32 + 0.125
    });
    let rgb = mhc_demosaic(&frame, &frame.pixels, &AtomicBool::new(false)).unwrap();
    for y in 0..16 {
        for x in 0..16 {
            let channel = match frame.header.info.bayer_pattern.unwrap().color(x, y) {
                Color::Red => 0,
                Color::Green => 1,
                Color::Blue => 2,
            };
            assert_eq!(rgb.pixels[y * 16 + x][channel], frame.pixels[y * 16 + x]);
        }
    }
}

#[test]
fn isolated_hot_pixel_does_not_produce_a_demosaic_cross() {
    let frame = frame(BayerPattern::GRBG, 16, 16, |x, y, _| {
        if x == 7 && y == 8 { 65535.0 } else { 1000.0 }
    });
    let original = frame.pixels.clone();
    let (cleaned, count) = cosmetic_correct(&frame, &AtomicBool::new(false)).unwrap();
    assert_eq!(count, 1);
    assert_eq!(frame.pixels, original);
    let rgb = mhc_demosaic(&frame, &cleaned, &AtomicBool::new(false)).unwrap();
    assert!(rgb.pixels.iter().all(|pixel| *pixel == [1000.0; 3]));
}

#[test]
fn spatially_supported_undersampled_star_core_is_not_erased() {
    let frame = frame(BayerPattern::GRBG, 32, 32, |x, y, _| {
        let r2 = (x as f32 - 15.2).powi(2) + (y as f32 - 14.7).powi(2);
        1000.0 + 45000.0 * (-r2 / (2.0 * 0.8_f32.powi(2))).exp()
    });
    let (cleaned, _) = cosmetic_correct(&frame, &AtomicBool::new(false)).unwrap();
    let peak = frame
        .pixels
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .unwrap()
        .0;
    assert_eq!(cleaned[peak], frame.pixels[peak]);
}

#[test]
fn saturated_real_star_neighborhood_keeps_its_measured_core_samples() {
    // Anonymized intensities from a DS-B stellar neighborhood. The former
    // MAD-only support guard replaced its 65535 ADU red core with 11403.5 ADU.
    let patch = [
        [
            2752., 2760., 3175., 2722., 3533., 3040., 3137., 3073., 3384.,
        ],
        [
            2371., 4047., 3003., 6119., 3145., 9588., 3732., 7302., 3214.,
        ],
        [
            2987., 2708., 4276., 4759., 10110., 9692., 12697., 5431., 4570.,
        ],
        [
            2644., 5660., 4370., 15604., 36423., 65535., 41841., 37893., 6217.,
        ],
        [
            3281., 3359., 5516., 16352., 65535., 48207., 65535., 48817., 21688.,
        ],
        [
            2915., 5865., 5726., 21452., 48931., 65535., 49818., 65535., 34612.,
        ],
        [
            4030., 3544., 6045., 13689., 45479., 48363., 65535., 49148., 28555.,
        ],
        [
            2963., 5691., 5297., 11635., 18278., 25954., 28871., 18807., 13702.,
        ],
        [
            3328., 3070., 5306., 5121., 9881., 11086., 16810., 10189., 10034.,
        ],
    ];
    let input = frame(BayerPattern::GRBG, 24, 24, |x, y, _| {
        if (5..14).contains(&x) && (4..13).contains(&y) {
            patch[y - 4][x - 5]
        } else {
            3000.0
        }
    });
    let original = input.pixels.clone();
    let (cleaned, _) = cosmetic_correct(&input, &AtomicBool::new(false)).unwrap();
    let rgb = mhc_demosaic(&input, &cleaned, &AtomicBool::new(false)).unwrap();
    for y in 7..=9 {
        for x in 8..=10 {
            let index = y * 24 + x;
            assert_eq!(cleaned[index], original[index], "Stellar core at {x},{y}");
        }
    }
    assert_eq!(rgb.pixels[8 * 24 + 9][0], 65535.0);
    assert_eq!(input.pixels, original);
}

#[test]
fn colored_star_core_survives_width_phase_and_saturation_sweep() {
    for sigma in [0.6_f32, 0.8, 1.2, 1.8, 2.5] {
        for amplitude in [5000.0_f32, 45000.0, 150000.0, 500000.0] {
            for (cx, cy) in [(15.0_f32, 16.0_f32), (15.3, 16.7), (15.8, 16.2)] {
                let input = frame(BayerPattern::GRBG, 40, 40, |x, y, color| {
                    let response = match color {
                        Color::Red => 0.6,
                        Color::Green => 1.0,
                        Color::Blue => 1.4,
                    };
                    let r2 = (x as f32 - cx).powi(2) + (y as f32 - cy).powi(2);
                    (2000.0 + response * amplitude * (-r2 / (2.0 * sigma.powi(2))).exp())
                        .min(65535.0)
                });
                let (cleaned, _) = cosmetic_correct(&input, &AtomicBool::new(false)).unwrap();
                for y in 12..=20 {
                    for x in 11..=19 {
                        let index = y * 40 + x;
                        if input.pixels[index] > 2500.0 {
                            assert_eq!(
                                cleaned[index], input.pixels[index],
                                "Core loss: sigma={sigma}, amplitude={amplitude}, center={cx},{cy}, pixel={x},{y}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn fractional_faint_signal_and_color_background_survive_cosmetics() {
    let frame = frame(BayerPattern::GRBG, 16, 16, |_, _, color| match color {
        Color::Red => 2500.125,
        Color::Green => 1000.25,
        Color::Blue => 500.5,
    });
    let (cleaned, count) = cosmetic_correct(&frame, &AtomicBool::new(false)).unwrap();
    assert_eq!(count, 0);
    assert_eq!(cleaned, frame.pixels);
}

#[test]
fn cancelled_or_mismatched_reconstruction_fails() {
    let mut frame = frame(BayerPattern::GRBG, 8, 8, |_, _, _| 1000.0);
    assert!(mhc_demosaic(&frame, &frame.pixels, &AtomicBool::new(true)).is_err());
    assert!(cosmetic_correct(&frame, &AtomicBool::new(true)).is_err());
    assert!(vng_demosaic(&frame, &frame.pixels, &AtomicBool::new(true)).is_err());
    frame.pixels.pop();
    assert!(cosmetic_correct(&frame, &AtomicBool::new(false)).is_err());
    assert!(mhc_demosaic(&frame, &frame.pixels, &AtomicBool::new(false)).is_err());
    assert!(vng_demosaic(&frame, &frame.pixels, &AtomicBool::new(false)).is_err());
}

#[test]
fn vng_all_phases_keep_flat_fractional_colors_and_native_samples() {
    for pattern in [
        BayerPattern::GRBG,
        BayerPattern::RGGB,
        BayerPattern::GBRG,
        BayerPattern::BGGR,
    ] {
        let input = frame(pattern, 17, 19, |_, _, color| match color {
            Color::Red => 1500.125,
            Color::Green => 1000.25,
            Color::Blue => 700.5,
        });
        let rgb = vng_demosaic(&input, &input.pixels, &AtomicBool::new(false)).unwrap();
        for pixel in rgb.pixels {
            for (actual, expected) in pixel.into_iter().zip([1500.125, 1000.25, 700.5]) {
                assert!((actual - expected).abs() < 0.002);
            }
        }
        let input = frame(pattern, 17, 19, |x, y, _| {
            ((x * 31 + y * 17) % 1000) as f32 + 0.125
        });
        let original = input.pixels.clone();
        let rgb = vng_demosaic(&input, &input.pixels, &AtomicBool::new(false)).unwrap();
        for y in 0..19 {
            for x in 0..17 {
                let c = match pattern.color(x, y) {
                    Color::Red => 0,
                    Color::Green => 1,
                    Color::Blue => 2,
                };
                assert_eq!(rgb.pixels[y * 17 + x][c], input.pixels[y * 17 + x]);
            }
        }
        assert_eq!(input.pixels, original);
    }
}

#[test]
fn vng_is_equivariant_under_quarter_turns() {
    let input = frame(BayerPattern::GRBG, 21, 21, |x, y, _| {
        ((x * 31 + y * 103 + x * y * 7) % 5000) as f32 + 0.125
    });
    let turned = frame(BayerPattern::GBRG, 21, 21, |x, y, _| {
        input.pixels[(20 - x) * 21 + y]
    });
    let first = vng_demosaic(&input, &input.pixels, &AtomicBool::new(false)).unwrap();
    let second = vng_demosaic(&turned, &turned.pixels, &AtomicBool::new(false)).unwrap();
    for y in 0..21 {
        for x in 0..21 {
            for c in 0..3 {
                assert!(
                    (second.pixels[y * 21 + x][c] - first.pixels[(20 - x) * 21 + y][c]).abs()
                        < 0.01
                );
            }
        }
    }
}

#[test]
fn vng_preserves_common_gain_offset_and_unbounded_samples() {
    let input = frame(BayerPattern::GRBG, 20, 20, |x, y, _| {
        ((x * 113 + y * 157 + x * y * 43) % 10000) as f32
    });
    let shifted = frame(BayerPattern::GRBG, 20, 20, |x, y, _| {
        16.0 * input.pixels[y * 20 + x] - 30000.0
    });
    let first = vng_demosaic(&input, &input.pixels, &AtomicBool::new(false)).unwrap();
    let second = vng_demosaic(&shifted, &shifted.pixels, &AtomicBool::new(false)).unwrap();
    for (a, b) in first
        .pixels
        .iter()
        .flatten()
        .zip(second.pixels.iter().flatten())
    {
        assert!((16.0 * a - 30000.0 - b).abs() < 0.06);
    }
    assert!(second.pixels.iter().flatten().any(|&v| v < 0.0));
    assert!(second.pixels.iter().flatten().any(|&v| v > 65535.0));
}

#[test]
fn vng_reconstructs_common_spatial_gradient_without_false_color() {
    let input = frame(BayerPattern::GRBG, 20, 20, |x, y, color| {
        let offset = match color {
            Color::Red => 200.,
            Color::Green => 100.,
            Color::Blue => 50.,
        };
        offset + 3.0 * x as f32 + 7.0 * y as f32
    });
    let rgb = vng_demosaic(&input, &input.pixels, &AtomicBool::new(false)).unwrap();
    for y in 2..18 {
        for x in 2..18 {
            for (actual, offset) in rgb.pixels[y * 20 + x].into_iter().zip([200., 100., 50.]) {
                assert!((actual - (offset + 3.0 * x as f32 + 7.0 * y as f32)).abs() < 0.002);
            }
        }
    }
}

#[test]
fn vng_removes_the_measured_chromatic_satellite_without_clipping() {
    // Small anonymized sensor neighborhood at a satellite in the owner's
    // screenshot. Expected VNG values were calculated independently in ADU.
    let patch = [
        [7445., 3120., 2848., 2809., 2745.],
        [19498., 6756., 3174., 3340., 2641.],
        [54844., 10832., 4858., 2826., 2564.],
        [19924., 7977., 4371., 3112., 2548.],
        [10385., 5396., 4290., 2659., 2624.],
    ];
    let input = frame(BayerPattern::GRBG, 12, 12, |x, y, _| {
        if (3..8).contains(&x) && (2..7).contains(&y) {
            patch[y - 2][x - 3]
        } else {
            2500.0
        }
    });
    let baseline = mhc_demosaic(&input, &input.pixels, &AtomicBool::new(false)).unwrap();
    assert!(baseline.pixels[4 * 12 + 5][1] < 0.0 && baseline.pixels[4 * 12 + 5][2] < 0.0);
    let rgb = vng_demosaic(&input, &input.pixels, &AtomicBool::new(false)).unwrap();
    for (actual, expected) in rgb.pixels[4 * 12 + 5]
        .into_iter()
        .zip([4858.0, 5167.6787, 5079.4287])
    {
        assert!((actual - expected).abs() < 0.01);
    }
}

#[test]
fn vng_keeps_interpolation_overshoot_from_bounded_sensor_data() {
    let mut overshoot = false;
    for amplitude in [80000.0_f32, 120000.0, 300000.0] {
        let input = frame(BayerPattern::GRBG, 32, 32, |x, y, color| {
            let response = if color == Color::Green { 0.65 } else { 1.0 };
            let r2 = (x as f32 - 15.3).powi(2) + (y as f32 - 16.7).powi(2);
            (2000.0 + response * amplitude * (-r2 / (2.0 * 1.1_f32.powi(2))).exp()).min(65535.0)
        });
        let rgb = vng_demosaic(&input, &input.pixels, &AtomicBool::new(false)).unwrap();
        overshoot |= rgb.pixels.iter().flatten().any(|&value| value > 65535.0);
    }
    assert!(
        overshoot,
        "Interpolation excursions must survive, not be clamped to the sensor range."
    );
}

#[test]
fn ordinary_gaussian_noise_is_not_mistaken_for_hot_pixels() {
    let mut state = 42_u64;
    let mut next = || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((state >> 32) as f64 + 0.5) / 4294967296.0
    };
    let pixels: Vec<f32> = (0..256 * 256)
        .map(|_| {
            let noise = (-2.0 * next().ln()).sqrt() * (std::f64::consts::TAU * next()).cos();
            1000.0 + 30.0 * noise as f32
        })
        .collect();
    let mut frame = frame(BayerPattern::GRBG, 256, 256, |_, _, _| 0.0);
    frame.pixels = pixels;
    let (_, corrected) = cosmetic_correct(&frame, &AtomicBool::new(false)).unwrap();
    assert!(
        corrected < frame.pixels.len() / 1000,
        "Noise false-positive count: {corrected}"
    );
}
