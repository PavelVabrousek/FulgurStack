use super::*;
use fulgur_core::fits::read_roi;
use std::fs;
pub(super) fn synthetic(path: &Path, t: Transform, exposure: f64, transparency: f64, trail: bool) {
    let mut bytes = vec![];
    for (key, value) in [
        ("SIMPLE", "T".to_owned()),
        ("BITPIX", "16".into()),
        ("NAXIS", "2".into()),
        ("NAXIS1", "128".into()),
        ("NAXIS2", "128".into()),
        ("BZERO", "32768".into()),
        ("BAYERPAT", "'GRBG'".into()),
        ("EXPTIME", exposure.to_string()),
        ("BIAS", "2064".into()),
    ] {
        bytes.extend(format!("{:<80}", format!("{key:<8}= {value:>20}")).as_bytes());
    }
    bytes.extend(format!("{:<80}", "END").as_bytes());
    bytes.resize(2880, b' ');
    for y in 0..128 {
        for x in 0..128 {
            let q = t.apply([x as f64, y as f64]);
            let profile =
                (-((q[0] - 62.37).powi(2) + (q[1] - 64.81).powi(2)) / (2. * 1.5_f64.powi(2))).exp();
            let amplitude = match fulgur_core::BayerPattern::GRBG.color(x, y) {
                fulgur_core::Color::Red => 10000.,
                fulgur_core::Color::Green => 8000.,
                fulgur_core::Color::Blue => 6000.,
            };
            let satellite = if trail && (q[1] - 80.).abs() < 1. {
                10000.
            } else {
                0.
            };
            let physical = (2000.
                + exposure / 10. * transparency * (amplitude * profile + satellite))
                .round() as i32;
            bytes.extend(((physical - 32768) as i16).to_be_bytes());
        }
    }
    bytes.resize(bytes.len().div_ceil(2880) * 2880, 0);
    fs::write(path, bytes).unwrap();
}
#[test]
fn packed_rejection_matches_prior_frame_major_bits_maps_and_statistics() {
    #[allow(clippy::too_many_arguments)]
    fn prior_scalar_band(
        frames: &[Vec<[f32; 3]>],
        weights: &[f64],
        offset: usize,
        output: &mut [IntegratedPixel],
        original_input_count: usize,
        total_weight: f64,
        config: &IntegrationConfig,
    ) -> Result<RowStats> {
        let mut result = RowStats {
            min_n: frames.len(),
            min_fraction: 1.,
            min_surviving: 1.,
            ..Default::default()
        };
        let mut values = Vec::with_capacity(frames.len());
        let mut scratch = ClippingScratch::with_capacity(frames.len());
        for (x, pixel) in output.iter_mut().enumerate() {
            let p = offset + x;
            let mut covered = Sum::default();
            let mut n = 0;
            for (frame, w) in frames.iter().zip(weights) {
                if frame[p][0].is_finite() {
                    covered.add(*w);
                    n += 1;
                }
            }
            result.min_n = result.min_n.min(n);
            result.max_n = result.max_n.max(n);
            result.min_fraction = result.min_fraction.min(covered.value() / total_weight);
            if n == 0 {
                result.zero += 1;
            }
            for (c, out) in pixel.0.iter_mut().enumerate() {
                values.clear();
                values.extend(
                    frames
                        .iter()
                        .enumerate()
                        .filter_map(|(i, f)| f[p][c].is_finite().then_some((f[p][c], i))),
                );
                result.valid += values.len() as u64;
                let (value, low, high, surviving) = combine(&values, weights, &mut scratch, config);
                *out = value;
                pixel.1[c] = ((values.len() as u64 - low - high) as f64
                    / original_input_count as f64) as f32;
                pixel.2[c] = (high as f64 / original_input_count as f64) as f32;
                result.low += low;
                result.high += high;
                result.min_surviving = result.min_surviving.min(surviving / total_weight);
            }
        }
        Ok(result)
    }
    let cancel = AtomicBool::new(false);
    for count in [1, 2, 4, 5, 27, 32, 940, 1500] {
        let frames: Vec<Vec<[f32; 3]>> = (0..count)
            .map(|i| {
                (0..97)
                    .map(|p| {
                        std::array::from_fn(|c| {
                            if p % 19 == 0 {
                                f32::NAN
                            } else if (i + p + c) % 23 == 0 {
                                f32::INFINITY
                            } else if (i + p + c) % 29 == 0 {
                                f32::NEG_INFINITY
                            } else if p % 11 == 0 {
                                65535.
                            } else {
                                ((i * 7919 + p * 59 + c * 31) % 65536) as f32 * 0.03125 - 100.125
                            }
                        })
                    })
                    .collect()
            })
            .collect();
        let before: Vec<_> = frames
            .iter()
            .flatten()
            .map(|v| v.map(f32::to_bits))
            .collect();
        let weights: Vec<_> = (0..count)
            .map(|i| 0.01 + ((i * 37) % 100) as f64 / 101.)
            .collect();
        let mut total = Sum::default();
        for weight in &weights {
            total.add(*weight);
        }
        for (sigma_low, sigma_high) in [(3., 2.5), (1., 1.), (5., 3.), (0.3, 0.6)] {
            let config = IntegrationConfig {
                sigma_low,
                sigma_high,
                ..Default::default()
            };
            for length in [1, 31, 32, 33, 73] {
                let mut expected = vec![([0.; 3], [0.; 3], [0.; 3]); length];
                let mut actual = expected.clone();
                let e = prior_scalar_band(
                    &frames,
                    &weights,
                    7,
                    &mut expected,
                    count + 7,
                    total.value(),
                    &config,
                )
                .unwrap();
                let a = reduce_band(
                    &frames,
                    7,
                    &mut actual,
                    &ReductionContext {
                        weights: &weights,
                        original_input_count: count + 7,
                        total_weight: total.value(),
                        config: &config,
                        cancel: &cancel,
                    },
                )
                .unwrap();
                for (i, (a, e)) in actual.iter().zip(&expected).enumerate() {
                    assert_eq!(
                        (
                            a.0.map(f32::to_bits),
                            a.1.map(f32::to_bits),
                            a.2.map(f32::to_bits)
                        ),
                        (
                            e.0.map(f32::to_bits),
                            e.1.map(f32::to_bits),
                            e.2.map(f32::to_bits)
                        ),
                        "count={count}, length={length}, pixel={i}"
                    );
                }
                assert_eq!(
                    (a.valid, a.low, a.high, a.zero, a.min_n, a.max_n),
                    (e.valid, e.low, e.high, e.zero, e.min_n, e.max_n)
                );
                assert_eq!(
                    (a.min_fraction.to_bits(), a.min_surviving.to_bits()),
                    (e.min_fraction.to_bits(), e.min_surviving.to_bits())
                );
            }
        }
        assert_eq!(
            frames
                .iter()
                .flatten()
                .map(|v| v.map(f32::to_bits))
                .collect::<Vec<_>>(),
            before
        );
    }
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    let frames = vec![vec![[1.; 3]; 1]];
    let mut output = vec![([0.; 3], [0.; 3], [0.; 3]); 1];
    assert!(matches!(
        reduce_band(
            &frames,
            0,
            &mut output,
            &ReductionContext {
                weights: &[1.],
                original_input_count: 1,
                total_weight: 1.,
                config: &IntegrationConfig::default(),
                cancel: &cancel
            }
        ),
        Err(Error::Cancelled)
    ));
}

#[test]
fn specialized_keys_weights_match_scalar_at_integer_boundaries() {
    for base in [1_usize, 2, 3, 17, 127, 2157, 3837, 65535] {
        let low = base as f64;
        let high = (base + 1) as f64;
        let positions = std::iter::once(low)
            .chain(std::iter::once(f64::from_bits(low.to_bits() + 1)))
            .chain(std::iter::once(f64::from_bits(high.to_bits() - 1)))
            .chain((1..1024).map(|i| low + i as f64 / 1024.));
        for p in positions {
            let expected: [f32; 4] = std::array::from_fn(|i| keys(p - (base + i) as f64 + 1.));
            assert_eq!(
                keys_weights(p, base).map(f32::to_bits),
                expected.map(f32::to_bits),
                "p={p}, base={base}"
            );
        }
    }
}

#[test]
fn cpu_resampling_matches_prior_scalar_bits_and_masks() {
    fn scalar_sample(
        rgb: &RgbImage,
        roi: [usize; 4],
        sensor: [usize; 2],
        p: [f64; 2],
    ) -> Option<[f32; 3]> {
        if p[0] < 1.
            || p[1] < 1.
            || p[0] >= (sensor[0] - 2) as f64
            || p[1] >= (sensor[1] - 2) as f64
        {
            return None;
        }
        let x = p[0].floor() as usize;
        let y = p[1].floor() as usize;
        if x < roi[0] + 1 || y < roi[1] + 1 || x + 2 >= roi[0] + roi[2] || y + 2 >= roi[1] + roi[3]
        {
            return None;
        }
        let wx: [f32; 4] = std::array::from_fn(|i| keys(p[0] - (x + i) as f64 + 1.));
        let wy: [f32; 4] = std::array::from_fn(|i| keys(p[1] - (y + i) as f64 + 1.));
        let mut result = [0.; 3];
        for (j, yy) in wy.iter().enumerate() {
            for (i, xx) in wx.iter().enumerate() {
                let value = rgb.pixels[(y + j - 1 - roi[1]) * rgb.width + x + i - 1 - roi[0]];
                let w = xx * yy;
                for c in 0..3 {
                    result[c] += w * value[c];
                }
            }
        }
        Some(result)
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.fit");
    synthetic(&path, Transform::IDENTITY, 10., 1., false);
    let header = read_header(&path).unwrap();
    let cancel = AtomicBool::new(false);
    let full = RgbImage {
        width: 128,
        height: 128,
        demosaic_method: DemosaicMethod::Vng,
        pixels: (0..128 * 128)
            .map(|i| {
                let v = ((i * 7919) % 65536) as f32;
                [v - 0.03125, 65535. - v, (i % 37) as f32 * 0.00003125]
            })
            .collect(),
    };
    let mut valid = 0;
    let mut invalid = 0;
    for scale_factor in [0.30, 0.33, 0.5, 1., 2., 3., 1. / 0.30] {
        for angle in [0_f64, 0.07, 15., 90., 179.5] {
            for origin in [[-19, -17], [0, 0], [61, 59]] {
                let source = IntegrationSource {
                    calibration: None,
                    distortion: None,
                    partial: None,
                    path: path.clone(),
                    header: header.clone(),
                    transform: Transform {
                        scale_factor,
                        theta_rad: angle.to_radians(),
                        tx_px: 7.3125,
                        ty_px: -8.125,
                    },
                    scale: 0.731_234_567_89,
                    offset: [-0.123_456_789; 3],
                    weight: 1.,
                };
                let size = [79, 63];
                for roi in [[0, 0, 128, 128], [16, 22, 93, 87]] {
                    let rgb = RgbImage {
                        width: roi[2],
                        height: roi[3],
                        demosaic_method: DemosaicMethod::Vng,
                        pixels: (roi[1]..roi[1] + roi[3])
                            .flat_map(|y| {
                                full.pixels[y * 128 + roi[0]..y * 128 + roi[0] + roi[2]]
                                    .iter()
                                    .copied()
                            })
                            .collect(),
                    };
                    let actual = warp_rgb(&source, origin, size, roi, &rgb, &cancel).unwrap();
                    let inverse = source.transform.inverse();
                    for (i, pixel) in actual.iter().enumerate() {
                        let q = [
                            (origin[0] + (i % size[0]) as i32) as f64,
                            (origin[1] + (i / size[0]) as i32) as f64,
                        ];
                        match scalar_sample(&rgb, roi, [128, 128], inverse.apply(q)) {
                            Some(v) => {
                                let expected = std::array::from_fn(|c| {
                                    (source.scale * v[c] as f64 + source.offset[c]) as f32
                                });
                                assert_eq!(
                                    pixel.map(f32::to_bits),
                                    expected.map(f32::to_bits),
                                    "scale={scale_factor}, angle={angle}, origin={origin:?}, roi={roi:?}, pixel={i}"
                                );
                                valid += 1;
                            }
                            None => {
                                assert!(pixel.iter().all(|v| v.is_nan()));
                                invalid += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(valid > 10000 && invalid > 10000);
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    let source = IntegrationSource {
        calibration: None,
        distortion: None,
        partial: None,
        path,
        header,
        transform: Transform::IDENTITY,
        scale: 1.,
        offset: [0.; 3],
        weight: 1.,
    };
    assert!(matches!(
        warp_rgb(&source, [0, 0], [79, 63], [0, 0, 128, 128], &full, &cancel),
        Err(Error::Cancelled)
    ));
}

#[test]
fn reusable_clipping_scratch_matches_allocating_baseline_bit_for_bit() {
    fn allocating_reference(
        values: &[(f32, usize)],
        weights: &[f64],
        scratch: &mut Vec<f32>,
        config: &IntegrationConfig,
    ) -> (f32, u64, u64, f64) {
        if values.is_empty() {
            return (0., 0, 0, 0.);
        }
        let mut low = f64::NEG_INFINITY;
        let mut high = f64::INFINITY;
        if values.len() >= 5 {
            scratch.clear();
            scratch.extend(values.iter().map(|v| v.0));
            for _ in 0..3 {
                let center = median(scratch);
                let mut deviations: Vec<_> = scratch
                    .iter()
                    .map(|v| (*v as f64 - center).abs() as f32)
                    .collect();
                let sigma = (1.4826 * median(&mut deviations)).max(1e-6);
                low = center - config.sigma_low * sigma;
                high = center + config.sigma_high * sigma;
                let mut changed = false;
                for x in scratch.iter_mut() {
                    let next = (*x as f64).clamp(low, high) as f32;
                    changed |= next != *x;
                    *x = next;
                }
                if !changed {
                    break;
                }
            }
        }
        let mut numerator = Sum::default();
        let mut denominator = Sum::default();
        let mut rejected_low = 0;
        let mut rejected_high = 0;
        for &(v, index) in values {
            if (v as f64) < low {
                rejected_low += 1;
            } else if (v as f64) > high {
                rejected_high += 1;
            } else {
                numerator.add(v as f64 * weights[index]);
                denominator.add(weights[index]);
            }
        }
        let weight = denominator.value();
        if weight == 0. {
            (0., rejected_low, rejected_high, 0.)
        } else {
            (
                (numerator.value() / weight) as f32,
                rejected_low,
                rejected_high,
                weight,
            )
        }
    }

    let weights: Vec<_> = (0..1500)
        .map(|i| 0.01 + ((i * 37) % 100) as f64 / 101.)
        .collect();
    let mut scratch = ClippingScratch::with_capacity(weights.len());
    let addresses = (scratch.winsorized.as_ptr(), scratch.deviations.as_ptr());
    let capacities = (scratch.winsorized.capacity(), scratch.deviations.capacity());
    let mut seed = 0x27182818_u64;
    for (sigma_low, sigma_high) in [(3., 2.5), (1., 1.), (5., 3.), (0.3, 0.6)] {
        let config = IntegrationConfig {
            sigma_low,
            sigma_high,
            ..Default::default()
        };
        for round in 0..12 {
            for n in [0, 1, 2, 4, 5, 6, 27, 32, 940, 1140, 1500, 940, 5, 0] {
                let values: Vec<_> = (0..n)
                    .map(|i| {
                        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                        let noise = ((seed >> 32) as u32 % 4096) as f32 / 512.;
                        let value = match round % 4 {
                            0 => {
                                if i % 19 == 0 {
                                    65000.
                                } else {
                                    2064.
                                }
                            }
                            1 => 2064. + noise / 8192.,
                            2 => {
                                if i % 11 == 0 {
                                    -100.
                                } else {
                                    65535. - noise
                                }
                            }
                            _ => {
                                if i % 13 == 0 {
                                    60000.
                                } else {
                                    2000. + noise
                                }
                            }
                        };
                        (value, i)
                    })
                    .collect();
                let expected = allocating_reference(&values, &weights, &mut Vec::new(), &config);
                let actual = combine(&values, &weights, &mut scratch, &config);
                assert_eq!(
                    (actual.0.to_bits(), actual.1, actual.2, actual.3.to_bits()),
                    (
                        expected.0.to_bits(),
                        expected.1,
                        expected.2,
                        expected.3.to_bits()
                    ),
                    "n={n}, round={round}, sigma={sigma_low}/{sigma_high}"
                );
                assert_eq!(
                    (scratch.winsorized.as_ptr(), scratch.deviations.as_ptr()),
                    addresses
                );
                assert_eq!(
                    (scratch.winsorized.capacity(), scratch.deviations.capacity()),
                    capacities
                );
            }
        }
    }
}
#[test]
fn clipping_removes_trails_preserves_clean_core_and_handles_sparse_coverage() {
    let config = IntegrationConfig::default();
    let weights = [1.; 9];
    let mut scratch = ClippingScratch::with_capacity(weights.len());
    for core in [2000_f32, 40000., 65535.] {
        let values: Vec<_> = (0..9)
            .map(|i| (if i == 8 { core + 10000. } else { core }, i))
            .collect();
        let got = combine(&values, &weights, &mut scratch, &config);
        assert_eq!(got.0, core);
        assert_eq!(got.2, 1);
        assert_eq!(got.3, 8.);
    }
    let got = combine(&[(1000., 0), (9000., 1)], &weights, &mut scratch, &config);
    assert_eq!(got.0, 5000.);
    assert_eq!(got.1 + got.2, 0);
    assert_eq!(combine(&[], &weights, &mut scratch, &config).0, 0.);
    let mut sum = Sum::default();
    for v in [1e16, 1., -1e16] {
        sum.add(v);
    }
    assert_eq!(sum.value(), 1.);
}
#[test]
fn keys_preserves_fields_and_invalid_sensor_footprints_are_not_padding() {
    let rgb = RgbImage {
        width: 20,
        height: 20,
        demosaic_method: DemosaicMethod::Vng,
        pixels: (0..400)
            .map(|i| {
                let x = (i % 20) as f32;
                let y = (i / 20) as f32;
                [2000., 3. * x + 2. * y, 100. + x]
            })
            .collect(),
    };
    for i in 0..32 {
        let x = 5. + i as f64 / 16.;
        let y = 9.23;
        let v = sample(&rgb, [0, 0, 20, 20], [20, 20], [x, y]).unwrap();
        assert!((v[0] - 2000.).abs() < 0.001);
        assert!((v[1] as f64 - 3. * x - 2. * y).abs() < 0.00002);
    }
    assert!(sample(&rgb, [0, 0, 20, 20], [20, 20], [0.999, 8.]).is_none());
    assert!(sample(&rgb, [0, 0, 20, 20], [20, 20], [18., 8.]).is_none());
}

#[cfg(feature = "gpu")]
#[test]
fn hardware_gpu_integration_preserves_flux_masks_and_rejection() {
    if std::env::var("FULGURSTACK_TEST_GPU").as_deref() != Ok("1") {
        eprintln!("SKIP GPU integration hardware gate; set FULGURSTACK_TEST_GPU=1");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    let gpu =
        GpuIntegrationEngine::with_stages(GpuStages::Reconstruction).expect("required real GPU");
    let mut sources = Vec::new();
    for i in 0..6 {
        let path = dir.path().join(format!("light_{i}.fit"));
        synthetic(&path, Transform::IDENTITY, 10., 1., i == 5);
        sources.push(IntegrationSource {
            calibration: None,
            distortion: None,
            partial: None,
            header: read_header(&path).unwrap(),
            path,
            transform: Transform::IDENTITY,
            scale: 1.,
            offset: [0.; 3],
            weight: 1.,
        });
    }
    for scale in [0.33, 1., 2., 3.] {
        for source in &mut sources {
            source.transform = Transform {
                scale_factor: scale,
                theta_rad: 0.07,
                tx_px: 7.3,
                ty_px: 8.1,
            };
        }
        let canvas = Canvas {
            origin_x_px: 0,
            origin_y_px: 0,
            width: (128. * scale) as usize + 20,
            height: (128. * scale) as usize + 20,
            minimum_effective_weight_fraction: 0.,
            coordinate_rule: "GPU test",
        };
        let config = IntegrationConfig {
            tile_mode: TileMode::Manual,
            fits_cache_mib: 1024,
            tile_width: 64,
            tile_height: 64,
            ..Default::default()
        };
        let cpu = CpuIntegrationEngine
            .integrate_with_maps(&sources, &canvas, &config, 8, &cancel)
            .unwrap();
        let actual = gpu
            .integrate_with_maps(&sources, &canvas, &config, 8, &cancel)
            .unwrap();
        let mut squares = 0.;
        let mut maximum = 0_f64;
        let mut a_flux = 0.;
        let mut b_flux = 0.;
        for (a, b) in cpu.image.pixels.iter().zip(&actual.image.pixels) {
            for c in 0..3 {
                let d = (a[c] as f64 - b[c] as f64).abs() / 65535.;
                maximum = maximum.max(d);
                squares += d * d;
                a_flux += (a[c] as f64 - 2000.).max(0.);
                b_flux += (b[c] as f64 - 2000.).max(0.);
            }
        }
        let rms = (squares / (canvas.width * canvas.height * 3) as f64).sqrt();
        assert!(
            maximum <= 1e-3 && rms <= 1e-5,
            "scale {scale}: max={maximum}, rms={rms}"
        );
        assert!((a_flux / b_flux - 1.).abs() <= 0.001, "flux parity");
        assert_eq!(
            cpu.stats.valid_channel_samples,
            actual.stats.valid_channel_samples
        );
        assert_eq!(
            cpu.stats.rejected_high_channel_samples,
            actual.stats.rejected_high_channel_samples
        );
        assert_eq!(cpu.coverage.pixels, actual.coverage.pixels);
        assert_eq!(cpu.high_rejection.pixels, actual.high_rejection.pixels);
        // The CPU reconstructs only what the rotated tile reads (ADR-113).
        assert!(cpu.stats.corrected_roi_pixels <= actual.stats.corrected_roi_pixels);
        println!("GPU integration gate scale={scale}: normalized RMS={rms}, max={maximum}");
    }
}

/// Random stacks with noise, outliers, invalid samples and unequal weights.
#[cfg(feature = "gpu")]
fn random_stack(frames: usize, pixels: usize, seed: u64) -> (Vec<Vec<[f32; 3]>>, Vec<f64>) {
    let mut state = seed | 1;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    let weights = (0..frames).map(|_| 0.2 + 0.8 * next()).collect();
    let stack = (0..frames)
        .map(|f| {
            (0..pixels)
                .map(|p| {
                    let sky = 1500. + (p % 97) as f64 * 3. + f as f64 * 0.4;
                    std::array::from_fn(|_| {
                        let u = next();
                        if u < 0.02 {
                            f32::NAN
                        } else if u < 0.03 {
                            (sky + 6000. * next()) as f32
                        } else if u < 0.035 {
                            (sky - 800. * next()) as f32
                        } else {
                            (sky + 25. * (next() + next() + next() + next() - 2.)) as f32
                        }
                    })
                })
                .collect()
        })
        .collect();
    (stack, weights)
}

#[cfg(feature = "gpu")]
#[test]
fn hardware_gpu_pixel_rejection_matches_cpu_for_every_method_and_stack_size() {
    if std::env::var("FULGURSTACK_TEST_GPU").as_deref() != Ok("1") {
        eprintln!("SKIP GPU pixel rejection hardware gate; set FULGURSTACK_TEST_GPU=1");
        return;
    }
    let reducer =
        std::sync::Mutex::new(fulgur_gpu::reduce::GpuReducer::new().expect("required real GPU"));
    let cancel = AtomicBool::new(false);
    for (frames, seed) in [
        (1, 3),
        (4, 5),
        (5, 7),
        (27, 11),
        (40, 13),
        (300, 17),
        (856, 19),
    ] {
        let pixels = 1500;
        let (stack, weights) = random_stack(frames, pixels, seed);
        let mut total = Sum::default();
        for w in &weights {
            total.add(*w);
        }
        let total = total.value();
        for rejection in [
            Rejection::Winsorized,
            Rejection::Median,
            Rejection::LinearFit,
            Rejection::Esd,
        ] {
            let config = IntegrationConfig {
                rejection,
                ..Default::default()
            };
            let mut cpu = vec![([0.; 3], [0.; 3], [0.; 3]); pixels];
            let expected = reduce_band(
                &stack,
                0,
                &mut cpu,
                &ReductionContext {
                    weights: &weights,
                    original_input_count: frames + 2,
                    total_weight: total,
                    config: &config,
                    cancel: &cancel,
                },
            )
            .unwrap();
            let tile = GpuTile::new(&reducer, &weights, &config).unwrap();
            let mut gpu = vec![([0.; 3], [0.; 3], [0.; 3]); pixels];
            let actual = tile
                .reduce(
                    &stack,
                    &mut gpu,
                    &ReductionContext {
                        weights: &[],
                        original_input_count: frames + 2,
                        total_weight: total,
                        config: &IntegrationConfig::default(),
                        cancel: &cancel,
                    },
                )
                .unwrap();
            let mut squares = 0.;
            let mut maximum = 0_f64;
            for (a, b) in cpu.iter().zip(&gpu) {
                for c in 0..3 {
                    let d = (a.0[c] as f64 - b.0[c] as f64).abs() / 65535.;
                    maximum = maximum.max(d);
                    squares += d * d;
                }
            }
            let rms = (squares / (pixels * 3) as f64).sqrt();
            let label = format!("{rejection:?} with {frames} frames");
            assert!(
                maximum <= 1e-3 && rms <= 1e-5,
                "{label}: max={maximum}, rms={rms}"
            );
            assert_eq!(expected.valid, actual.valid, "{label}");
            assert_eq!(
                (expected.zero, expected.min_n, expected.max_n),
                (actual.zero, actual.min_n, actual.max_n),
                "{label}"
            );
            let rejected = |r: &RowStats| (r.low + r.high) as f64;
            assert!(
                (rejected(&expected) - rejected(&actual)).abs()
                    <= 1. + 0.002 * expected.valid as f64,
                "{label}: rejected {} vs {}",
                rejected(&expected),
                rejected(&actual)
            );
            assert!(
                (expected.min_fraction - actual.min_fraction).abs() < 1e-5,
                "{label}"
            );
            println!(
                "GPU pixel rejection {label}: RMS={rms:.2e}, max={maximum:.2e}, rejected {} vs {}",
                rejected(&expected),
                rejected(&actual)
            );
        }
    }
}
#[test]
fn scaled_inverse_warp_matches_full_reconstruction_and_is_tile_invariant() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.fit");
    synthetic(&path, Transform::IDENTITY, 10., 1., false);
    let before = fs::read(&path).unwrap();
    let cancel = AtomicBool::new(false);
    let raw = fulgur_core::fits::read_raw(&path).unwrap();
    let (full, _) = crate::debayer::debayer(&raw, &cancel, DemosaicMethod::Vng).unwrap();
    for scale in [0.33, 0.5, 2., 3.] {
        let transform = Transform {
            scale_factor: scale,
            theta_rad: 0.08,
            tx_px: 7.3,
            ty_px: 8.1,
        };
        let source = IntegrationSource {
            calibration: None,
            distortion: None,
            partial: None,
            path: path.clone(),
            header: raw.header.clone(),
            transform,
            scale: 1.,
            offset: [0.; 3],
            weight: 1.,
        };
        let canvas = Canvas {
            origin_x_px: 0,
            origin_y_px: 0,
            width: (128. * scale) as usize + 20,
            height: (128. * scale) as usize + 20,
            minimum_effective_weight_fraction: 0.,
            coordinate_rule: "test reference",
        };
        let config = IntegrationConfig {
            tile_mode: TileMode::Manual,
            tile_width: 37,
            tile_height: 25,
            ..Default::default()
        };
        let result = CpuIntegrationEngine
            .integrate_with_maps(std::slice::from_ref(&source), &canvas, &config, 1, &cancel)
            .unwrap();
        for y in 0..canvas.height {
            for x in 0..canvas.width {
                let p = transform.inverse().apply([x as f64, y as f64]);
                let expected = sample(&full, [0, 0, 128, 128], [128, 128], p);
                let index = y * canvas.width + x;
                assert_eq!(result.image.pixels[index], expected.unwrap_or([0.; 3]));
                assert_eq!(
                    result.coverage.pixels[index],
                    if expected.is_some() { [1.; 3] } else { [0.; 3] }
                );
                assert_eq!(result.high_rejection.pixels[index], [0.; 3]);
            }
        }
        for (width, height) in [
            (64, 64),
            (128, 256),
            (256, 128),
            (256, 256),
            (384, 192),
            (512, 128),
            (512, 256),
            (256, 512),
            (384, 384),
            (512, 512),
        ] {
            let other = IntegrationConfig {
                tile_mode: TileMode::Manual,
                tile_width: width,
                tile_height: height,
                ..Default::default()
            };
            let second = CpuIntegrationEngine
                .integrate_with_maps(std::slice::from_ref(&source), &canvas, &other, 1, &cancel)
                .unwrap();
            assert_eq!(result.image.pixels, second.image.pixels);
            assert_eq!(result.coverage.pixels, second.coverage.pixels);
            assert_eq!(result.high_rejection.pixels, second.high_rejection.pixels);
        }
    }
    assert_eq!(fs::read(path).unwrap(), before);
}
#[test]
fn reused_roi_scratch_and_tile_buffers_match_fresh_allocations_bit_for_bit() {
    let dir = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    let transforms = [
        Transform::IDENTITY,
        Transform {
            scale_factor: 1.,
            theta_rad: 0.31,
            tx_px: 9.25,
            ty_px: -6.5,
        },
        Transform {
            scale_factor: 2.,
            theta_rad: -0.12,
            tx_px: 40.75,
            ty_px: 3.125,
        },
    ];
    let mut sources = vec![];
    for (i, transform) in transforms.into_iter().enumerate() {
        let path = dir.path().join(format!("source{i}.fit"));
        synthetic(&path, transform.inverse(), 10., 1., i == 1);
        sources.push(IntegrationSource {
            calibration: None,
            distortion: None,
            partial: None,
            header: fulgur_core::fits::read_header(&path).unwrap(),
            path,
            transform,
            scale: 1.25,
            offset: [-3.5; 3],
            weight: 1.,
        });
    }
    // A rotated frame with a cubic model: its spans come from the solved
    // source position of every output sample (ADR-144).
    let path = dir.path().join("distorted.fit");
    let base = Transform {
        scale_factor: 1.,
        theta_rad: 0.21,
        tx_px: 6.5,
        ty_px: -4.25,
    };
    synthetic(&path, base.inverse(), 10., 1., false);
    sources.push(IntegrationSource {
        calibration: None,
        distortion: Some(crate::distortion::Warp {
            base,
            center: [64., 64.],
            normalization: [64., 64.],
            coefficients: [
                [0.4, 0.6, -0.3, 0.5, 0.7, -0.4, 0.3, 0.2, -0.25, 0.35],
                [-0.5, 0.2, 0.4, -0.3, 0.25, 0.6, -0.2, 0.3, 0.15, -0.45],
            ],
        }),
        partial: None,
        header: fulgur_core::fits::read_header(&path).unwrap(),
        path,
        transform: base,
        scale: 0.75,
        offset: [2.25; 3],
        weight: 1.,
    });
    let before: Vec<_> = sources.iter().map(|s| fs::read(&s.path).unwrap()).collect();
    let paths: Vec<_> = sources.iter().map(|s| s.path.clone()).collect();
    let fresh_cache = FitsBlockCache::new(&paths, 0);
    // Mixed growth, shrinkage, partial and entirely uncovered tiles.
    let tiles = [
        ([0, 0], [37, 25]),
        ([90, 70], [64, 64]),
        ([-20, -20], [30, 30]),
        ([200, 0], [40, 128]),
        ([500, 500], [16, 16]),
        ([3, 5], [128, 9]),
        ([60, 60], [7, 61]),
    ];
    let mut distorted_samples = 0;
    for capacity in [0, 64 * 1024 * 1024] {
        let cache = FitsBlockCache::new(&paths, capacity);
        let mut scratch = RoiScratch::default();
        let mut recycled: Vec<Vec<[f32; 3]>> = vec![];
        for (origin, size) in tiles {
            let mut next = vec![];
            for source in &sources {
                let expected = warp(
                    source,
                    origin,
                    size,
                    &cancel,
                    &fresh_cache,
                    &mut RoiScratch::default(),
                    Vec::new(),
                )
                .unwrap();
                // Poison every retained buffer so any unwritten sample shows.
                let mut output = recycled.pop().unwrap_or_default();
                output.fill([f32::from_bits(0x7fc0_beef); 3]);
                scratch.raw.fill(f32::from_bits(0x7fc0_0001));
                scratch.debayer.cleaned.fill(f32::from_bits(0x7fc0_0002));
                scratch.debayer.rgb.fill([f32::from_bits(0x7fc0_0003); 3]);
                let actual =
                    warp(source, origin, size, &cancel, &cache, &mut scratch, output).unwrap();
                let bits =
                    |v: &[[f32; 3]]| -> Vec<_> { v.iter().map(|p| p.map(f32::to_bits)).collect() };
                assert_eq!(bits(&actual.0), bits(&expected.0), "{origin:?} {size:?}");
                assert_eq!((actual.1, actual.2), (expected.1, expected.2));
                let whole = warp(
                    source,
                    origin,
                    size,
                    &cancel,
                    &fresh_cache,
                    &mut RoiScratch {
                        whole_roi: true,
                        ..Default::default()
                    },
                    Vec::new(),
                )
                .unwrap();
                assert_eq!(bits(&actual.0), bits(&whole.0), "spans {origin:?} {size:?}");
                if source.distortion.is_some() {
                    distorted_samples += actual.0.iter().filter(|p| p[0].is_finite()).count();
                }
                next.push(actual.0);
            }
            recycled = next;
        }
    }
    assert!(distorted_samples > 5_000, "{distorted_samples}");
    for (source, before) in sources.iter().zip(before) {
        assert_eq!(fs::read(&source.path).unwrap(), before);
    }
}
#[test]
fn diagnostic_maps_count_original_samples_per_channel_after_rejection() {
    let dir = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    let mut sources = vec![];
    for i in 0..6 {
        let path = dir.path().join(format!("constant_{i}.fit"));
        synthetic(&path, Transform::IDENTITY, 10., 1., false);
        let mut bytes = fs::read(&path).unwrap();
        for (j, pair) in bytes[2880..2880 + 128 * 128 * 2]
            .chunks_exact_mut(2)
            .enumerate()
        {
            let red =
                fulgur_core::BayerPattern::GRBG.color(j % 128, j / 128) == fulgur_core::Color::Red;
            let physical = if i == 5 && red { 5000 } else { 2000 };
            pair.copy_from_slice(&((physical - 32768) as i16).to_be_bytes());
        }
        fs::write(&path, bytes).unwrap();
        sources.push(IntegrationSource {
            calibration: None,
            distortion: None,
            partial: None,
            header: read_header(&path).unwrap(),
            path,
            transform: Transform::IDENTITY,
            scale: 1.,
            offset: [0.; 3],
            weight: if i == 5 { 0.001 } else { 1. },
        });
    }
    let canvas = Canvas {
        origin_x_px: 0,
        origin_y_px: 0,
        width: 128,
        height: 128,
        minimum_effective_weight_fraction: 0.,
        coordinate_rule: "reference",
    };
    let config = IntegrationConfig {
        tile_mode: TileMode::Manual,
        tile_width: 64,
        tile_height: 64,
        ..Default::default()
    };
    let result = CpuIntegrationEngine
        .integrate_with_maps(&sources, &canvas, &config, 8, &cancel)
        .unwrap();
    assert_eq!(result.image.pixels[64 * 128 + 64], [2000.; 3]);
    assert_eq!(
        result.coverage.pixels[64 * 128 + 64],
        [5. / 8., 6. / 8., 6. / 8.]
    );
    assert_eq!(
        result.high_rejection.pixels[64 * 128 + 64],
        [1. / 8., 0., 0.]
    );
    assert_eq!(result.coverage.pixels[0], [0.; 3]);
    assert_eq!(result.image.pixels[0], [0.; 3]);
    assert_eq!(result.high_rejection.pixels[0], [0.; 3]);
    let used: f64 = result
        .coverage
        .pixels
        .iter()
        .flatten()
        .map(|v| *v as f64 * 8.)
        .sum();
    let high: f64 = result
        .high_rejection
        .pixels
        .iter()
        .flatten()
        .map(|v| *v as f64 * 8.)
        .sum();
    assert_eq!(high, result.stats.rejected_high_channel_samples as f64);
    assert_eq!(
        used + high + result.stats.rejected_low_channel_samples as f64,
        result.stats.valid_channel_samples as f64
    );
    for (width, height) in [
        (37, 25),
        (128, 256),
        (256, 128),
        (256, 256),
        (384, 192),
        (512, 128),
        (512, 256),
        (256, 512),
        (384, 384),
        (512, 512),
    ] {
        let other = IntegrationConfig {
            tile_mode: TileMode::Manual,
            tile_width: width,
            tile_height: height,
            ..config.clone()
        };
        let tiled = CpuIntegrationEngine
            .integrate_with_maps(&sources, &canvas, &other, 8, &cancel)
            .unwrap();
        assert_eq!(result.image.pixels, tiled.image.pixels);
        assert_eq!(result.coverage.pixels, tiled.coverage.pixels);
        assert_eq!(result.high_rejection.pixels, tiled.high_rejection.pixels);
    }
    assert!(
        CpuIntegrationEngine
            .integrate_with_maps(&sources, &canvas, &config, 5, &cancel)
            .is_err()
    );
}

#[test]
fn twelve_sixteen_and_twenty_four_workers_preserve_all_samples_maps_and_inputs() {
    let dir = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    let mut sources = vec![];
    let mut original = vec![];
    for i in 0..30 {
        let path = dir.path().join(format!("source_{i}.fit"));
        let transform = Transform {
            scale_factor: 1.,
            theta_rad: i as f64 * 0.001,
            tx_px: i as f64 * 0.1,
            ty_px: -(i as f64) * 0.05,
        };
        synthetic(&path, transform, 10., 1., i == 29);
        original.push(fs::read(&path).unwrap());
        sources.push(IntegrationSource {
            calibration: None,
            distortion: None,
            partial: None,
            header: read_header(&path).unwrap(),
            path,
            transform,
            scale: 1.,
            offset: [0.; 3],
            weight: (1. + i as f64 * 0.001) / 1.029,
        });
    }
    let canvas = Canvas {
        origin_x_px: 16,
        origin_y_px: 16,
        width: 96,
        height: 96,
        minimum_effective_weight_fraction: 0.,
        coordinate_rule: "thread-count equality gate",
    };
    let config = IntegrationConfig {
        tile_mode: TileMode::Manual,
        tile_width: 64,
        tile_height: 64,
        ..Default::default()
    };
    let mut previous: Option<IntegrationResult> = None;
    for threads in [12, 16, 24] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        let result = pool
            .install(|| {
                CpuIntegrationEngine.integrate_with_maps(&sources, &canvas, &config, 32, &cancel)
            })
            .unwrap();
        assert_eq!(result.stats.max_concurrent_rois, threads);
        assert_eq!(result.stats.max_concurrent_reduction_bands, threads);
        if let Some(p) = &result.stats.cpu_profile {
            assert_eq!(p.pool_threads, threads);
            assert!(p.process.total().is_finite() && p.process.total() >= 0.);
        }
        if let Some(p) = previous {
            assert_eq!(p.image.pixels, result.image.pixels);
            assert_eq!(p.coverage.pixels, result.coverage.pixels);
            assert_eq!(p.high_rejection.pixels, result.high_rejection.pixels);
            assert_eq!(
                p.stats.rejected_low_channel_samples,
                result.stats.rejected_low_channel_samples
            );
            assert_eq!(
                p.stats.rejected_high_channel_samples,
                result.stats.rejected_high_channel_samples
            );
            assert!(result.stats.planned_buffer_bytes > p.stats.planned_buffer_bytes);
        }
        previous = Some(result);
    }
    for (source, bytes) in sources.iter().zip(original) {
        assert_eq!(fs::read(&source.path).unwrap(), bytes);
    }
}
#[test]
fn real_roi_reconstruction_matches_full_frame_without_internal_edge_artifacts() {
    let Some(path) = std::env::var_os("FULGURSTACK_TEST_FITS") else {
        return;
    };
    let cancel = AtomicBool::new(false);
    let raw = fulgur_core::fits::read_raw(Path::new(&path)).unwrap();
    let (full, _) = crate::debayer::debayer(&raw, &cancel, DemosaicMethod::Vng).unwrap();
    for roi in [
        [0, 0, 200, 200],
        [1600, 1200, 250, 220],
        [1960, 3640, 200, 200],
    ] {
        let cropped = read_roi(Path::new(&path), &raw.header, roi, &cancel).unwrap();
        let (rgb, _) = crate::debayer::debayer(&cropped, &cancel, DemosaicMethod::Vng).unwrap();
        for y in 0..roi[3] {
            for x in 0..roi[2] {
                let gx = x + roi[0];
                let gy = y + roi[1];
                if (x >= 6 || roi[0] == 0)
                    && (y >= 6 || roi[1] == 0)
                    && (x + 6 < roi[2] || roi[0] + roi[2] == full.width)
                    && (y + 6 < roi[3] || roi[1] + roi[3] == full.height)
                {
                    assert_eq!(
                        rgb.pixels[y * roi[2] + x],
                        full.pixels[gy * full.width + gx],
                        "ROI mismatch at {gx},{gy}"
                    );
                }
            }
        }
    }
}
#[test]
fn mixed_exposure_fractional_shifts_preserve_star_flux_and_tile_seams() {
    let dir = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    let mut sources = vec![];
    for i in 0..8 {
        let shift = i as f64 / 8.;
        let t = Transform {
            scale_factor: 1.,
            theta_rad: 0.,
            tx_px: shift,
            ty_px: shift * 0.7,
        };
        let exposure = [5., 10., 20.][i % 3];
        let transmission = if i % 2 == 0 { 0.7 } else { 1.1 };
        let path = dir.path().join(format!("frame{i}.fit"));
        synthetic(&path, t, exposure, transmission, i == 7);
        let scale = 10. / (exposure * transmission);
        sources.push(IntegrationSource {
            calibration: None,
            distortion: None,
            partial: None,
            header: read_header(&path).unwrap(),
            path,
            transform: t,
            scale,
            offset: [2000. - 2000. * scale; 3],
            weight: 1.,
        });
    }
    let canvas = Canvas {
        origin_x_px: 20,
        origin_y_px: 20,
        width: 88,
        height: 88,
        minimum_effective_weight_fraction: 1.,
        coordinate_rule: "test",
    };
    let config = IntegrationConfig {
        tile_mode: TileMode::Manual,
        tile_width: 23,
        tile_height: 19,
        ..Default::default()
    };
    let (image, stats) = CpuIntegrationEngine
        .integrate(&sources, &canvas, &config, &cancel)
        .unwrap();
    let measured = stats.reconstruction_wall_seconds
        + stats.reduction_wall_seconds
        + stats.output_copy_wall_seconds;
    assert!(measured > 0. && measured <= stats.wall_seconds);
    assert!(stats.roi_task_elapsed.read_seconds > 0.);
    assert!(stats.roi_task_elapsed.cosmetic_seconds > 0.);
    assert!(stats.roi_task_elapsed.demosaic_seconds > 0.);
    assert!(stats.roi_task_elapsed.resampling_seconds > 0.);
    assert_eq!(stats.zero_coverage_pixels, 0);
    assert!(stats.rejected_high_channel_samples > 0);
    assert!(stats.minimum_geometric_weight_fraction > 0.999999);
    let mut flux = [0.; 3];
    for y in 0..88 {
        for x in 0..88 {
            let r = ((x as f64 + 20. - 62.37).powi(2) + (y as f64 + 20. - 64.81).powi(2)).sqrt();
            if r < 8. {
                for (c, f) in flux.iter_mut().enumerate() {
                    *f += image.pixels[y * 88 + x][c] as f64 - 2000.;
                }
            }
        }
    }
    let baseline_path = dir.path().join("baseline.fit");
    synthetic(&baseline_path, Transform::IDENTITY, 10., 1., false);
    let baseline_raw = fulgur_core::fits::read_raw(&baseline_path).unwrap();
    let (baseline, _) =
        crate::debayer::debayer(&baseline_raw, &cancel, DemosaicMethod::Vng).unwrap();
    let mut baseline_flux = [0.; 3];
    for y in 0..128 {
        for x in 0..128 {
            if ((x as f64 - 62.37).powi(2) + (y as f64 - 64.81).powi(2)).sqrt() < 8. {
                for (c, f) in baseline_flux.iter_mut().enumerate() {
                    *f += baseline.pixels[y * 128 + x][c] as f64 - 2000.;
                }
            }
        }
    }
    println!(
        "Integrated flux {:?}; accepted M1 reconstruction flux {:?}",
        flux, baseline_flux
    );
    for (c, amp) in [10000., 8000., 6000.].iter().enumerate() {
        let expected = 2. * std::f64::consts::PI * 1.5_f64.powi(2) * amp;
        let error = (flux[c] / expected - 1.).abs();
        println!("Mixed exposure star channel {c}: relative flux error {error:.8}");
        let integration_error = (flux[c] / baseline_flux[c] - 1.).abs();
        assert!(
            integration_error <= 0.01,
            "channel {c} changes accepted reconstruction flux by {integration_error}"
        );
    }
    let mut other = config.clone();
    other.tile_width = 88;
    other.tile_height = 88;
    let (whole, _) = CpuIntegrationEngine
        .integrate(&sources, &canvas, &other, &cancel)
        .unwrap();
    assert_eq!(image.pixels, whole.pixels);
    let mut bad = config.clone();
    bad.memory_budget_mib = 32;
    assert!(
        CpuIntegrationEngine
            .integrate(&sources, &canvas, &bad, &cancel)
            .is_err()
    );
    assert!(
        CpuIntegrationEngine
            .integrate(&sources, &canvas, &config, &AtomicBool::new(true))
            .is_err()
    );
}
