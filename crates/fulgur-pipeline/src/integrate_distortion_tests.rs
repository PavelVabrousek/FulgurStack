use super::*;

fn model() -> crate::distortion::Warp {
    let mut m = crate::distortion::Warp {
        base: Transform::IDENTITY,
        center: [63.5; 2],
        normalization: [63.5; 2],
        coefficients: [[0.; 10]; 2],
    };
    // Independent analytic outward radial field: at corners radius grows 3%.
    let a = 0.03 * 63.5 / 2.;
    m.coefficients[0][6] = a;
    m.coefficients[0][8] = a;
    m.coefficients[1][7] = a;
    m.coefficients[1][9] = a;
    m
}

#[test]
fn corrected_roi_contains_every_valid_inverse_sample_and_tiles_preserve_master_maps_sources() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.fit");
    tests::synthetic(&path, Transform::IDENTITY, 10., 1., false);
    let original = std::fs::read(&path).unwrap();
    let source = IntegrationSource {
        calibration: None,
        path: path.clone(),
        header: read_header(&path).unwrap(),
        transform: Transform::IDENTITY,
        scale: 1.,
        offset: 0.,
        weight: 1.,
        distortion: Some(model()),
    };
    let sensor = [128, 128];
    for origin in [[-20, -20], [0, 0], [47, 39], [95, 95]] {
        let size = [48, 48];
        let roi = source_bounds(&source, origin, size, sensor);
        for y in 0..size[1] {
            for x in 0..size[0] {
                let q = [(origin[0] + x as i32) as f64, (origin[1] + y as i32) as f64];
                if let Some(p) = model().inverse(q) {
                    if p[0] >= 1. && p[1] >= 1. && p[0] < 126. && p[1] < 126. {
                        let [rx, ry, rw, rh] = roi.expect("valid sample must have a source ROI");
                        assert!(p[0] >= rx as f64 + 1. && p[1] >= ry as f64 + 1.);
                        assert!(
                            p[0].floor() + 2. < (rx + rw) as f64
                                && p[1].floor() + 2. < (ry + rh) as f64
                        );
                    }
                }
            }
        }
    }
    let canvas = Canvas {
        origin_x_px: 0,
        origin_y_px: 0,
        width: 128,
        height: 128,
        minimum_effective_weight_fraction: 0.,
        coordinate_rule: "distortion tile invariance gate",
    };
    let cancel = AtomicBool::new(false);
    let mut previous: Option<IntegrationResult> = None;
    for size in [32, 64, 128] {
        let config = IntegrationConfig {
            tile_mode: TileMode::Manual,
            tile_width: size,
            tile_height: size,
            ..Default::default()
        };
        let result = CpuIntegrationEngine
            .integrate_with_maps(std::slice::from_ref(&source), &canvas, &config, 2, &cancel)
            .unwrap();
        if let Some(old) = &previous {
            assert_eq!(old.image.pixels, result.image.pixels);
            assert_eq!(old.coverage.pixels, result.coverage.pixels);
            assert_eq!(old.high_rejection.pixels, result.high_rejection.pixels);
            assert_eq!(
                old.stats.valid_channel_samples,
                result.stats.valid_channel_samples
            );
        }
        assert!(result.stats.valid_channel_samples > 40000);
        previous = Some(result);
    }
    assert_eq!(original, std::fs::read(path).unwrap());
    let mut invalid = source;
    invalid.distortion.as_mut().unwrap().coefficients[0][1] = -500.;
    assert!(
        CpuIntegrationEngine
            .integrate_with_maps(
                &[invalid],
                &canvas,
                &IntegrationConfig::default(),
                2,
                &cancel
            )
            .is_err()
    );
}

#[test]
fn nonlinear_warp_samples_the_known_analytic_source_once_without_tone_change() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.fit");
    tests::synthetic(&path, Transform::IDENTITY, 10., 1., false);
    let source = IntegrationSource {
        calibration: None,
        path,
        header: read_header(&dir.path().join("source.fit")).unwrap(),
        transform: Transform::IDENTITY,
        scale: 0.75,
        offset: 0.25,
        weight: 1.,
        distortion: Some(model()),
    };
    let image = RgbImage {
        width: 128,
        height: 128,
        demosaic_method: DemosaicMethod::Vng,
        pixels: (0..128 * 128)
            .map(|i| {
                let x = (i % 128) as f32;
                let y = (i / 128) as f32;
                [x + y, x * 0.5 + y * 0.25, 1.]
            })
            .collect(),
    };
    let actual = warp_rgb(
        &source,
        [0, 0],
        [128, 128],
        [0, 0, 128, 128],
        &image,
        &AtomicBool::new(false),
    )
    .unwrap();
    let center = 63.5;
    let k = 0.03 / (2. * center * center);
    for y in 8..120 {
        for x in 8..120 {
            // Independent one-dimensional radial inverse, not Warp::inverse.
            let dx = x as f64 - center;
            let dy = y as f64 - center;
            let rd = dx.hypot(dy);
            let mut r = rd;
            for _ in 0..12 {
                r -= (r * (1. + k * r * r) - rd) / (1. + 3. * k * r * r);
            }
            let p = [center + dx * r / rd, center + dy * r / rd];
            let expected = sample(&image, [0, 0, 128, 128], [128, 128], p)
                .unwrap()
                .map(|v| (0.75 * v as f64 + 0.25) as f32);
            for (a, e) in actual[y * 128 + x].iter().zip(expected) {
                assert!((*a - e).abs() < 0.0001);
            }
        }
    }
}

#[cfg(feature = "gpu")]
#[test]
fn corrected_geometry_is_shared_by_cpu_and_actual_gpu_reconstruction() {
    if std::env::var("FULGURSTACK_TEST_GPU").as_deref() != Ok("1") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.fit");
    tests::synthetic(&path, Transform::IDENTITY, 10., 1., false);
    let original = std::fs::read(&path).unwrap();
    let source = IntegrationSource {
        calibration: None,
        path: path.clone(),
        header: read_header(&path).unwrap(),
        transform: Transform::IDENTITY,
        scale: 1.,
        offset: 0.,
        weight: 1.,
        distortion: Some(model()),
    };
    let canvas = Canvas {
        origin_x_px: 8,
        origin_y_px: 8,
        width: 112,
        height: 112,
        minimum_effective_weight_fraction: 0.,
        coordinate_rule: "corrected GPU parity",
    };
    let config = IntegrationConfig {
        tile_mode: TileMode::Manual,
        tile_width: 64,
        tile_height: 64,
        ..Default::default()
    };
    let cancel = AtomicBool::new(false);
    let cpu = CpuIntegrationEngine
        .integrate_with_maps(std::slice::from_ref(&source), &canvas, &config, 2, &cancel)
        .unwrap();
    let gpu = GpuIntegrationEngine::with_stages(GpuStages::Reconstruction)
        .unwrap()
        .integrate_with_maps(&[source], &canvas, &config, 2, &cancel)
        .unwrap();
    assert_eq!(cpu.coverage.pixels, gpu.coverage.pixels);
    assert_eq!(cpu.high_rejection.pixels, gpu.high_rejection.pixels);
    for (a, b) in cpu
        .image
        .pixels
        .iter()
        .flatten()
        .zip(gpu.image.pixels.iter().flatten())
    {
        assert!((*a - *b).abs() <= 0.05);
    }
    assert_eq!(original, std::fs::read(path).unwrap());
}
