#![cfg(feature = "gpu")]
use fulgur_core::{BayerPattern, Color, DemosaicMethod, RawFrame};
use fulgur_gpu::GpuReconstructor;
use fulgur_pipeline::{debayer::debayer, vng::gpu_plans};
use std::{io::Cursor, sync::atomic::AtomicBool};

fn frame(pattern: BayerPattern, size: [usize; 2], scene: usize) -> RawFrame {
    let mut bytes = Vec::new();
    for (k, v) in [
        ("SIMPLE", "T".to_owned()),
        ("BITPIX", "16".into()),
        ("NAXIS", "2".into()),
        ("NAXIS1", size[0].to_string()),
        ("NAXIS2", size[1].to_string()),
        ("BZERO", "32768".into()),
        ("BAYERPAT", format!("'{pattern:?}'")),
        ("EXPTIME", "10".into()),
    ] {
        bytes.extend(format!("{:<80}", format!("{k:<8}= {v:>20}")).as_bytes());
    }
    bytes.extend(format!("{:<80}", "END").as_bytes());
    bytes.resize(2880, b' ');
    bytes.resize((2880 + size[0] * size[1] * 2).div_ceil(2880) * 2880, 0);
    let header =
        fulgur_core::fits::FitsHeader::read(&mut Cursor::new(&bytes), bytes.len() as u64).unwrap();
    let pixels = (0..size[0] * size[1])
        .map(|i| {
            let (x, y) = (i % size[0], i / size[0]);
            let color = pattern.color(x, y);
            let base = match color {
                Color::Red => 2200.,
                Color::Green => 2400.,
                Color::Blue => 2600.,
            };
            if scene == 0 {
                return base + 0.125;
            }
            let noise = ((i * 71 + y * 13) % 59) as f32 - 29.;
            let amplitude = if scene == 2 { 95000. } else { 18000. };
            let sigma = if scene == 3 { 0.9_f32 } else { 1.6 };
            let profile = (-(((x as f32 - size[0] as f32 * 0.47 - 0.23).powi(2)
                + (y as f32 - size[1] as f32 * 0.53 - 0.37).powi(2))
                / (2. * sigma * sigma)))
                .exp();
            let mut v = (base + noise + amplitude * profile).min(65535.);
            if scene == 1 && x == size[0] / 4 && y == size[1] / 4 {
                v = 60000.;
            }
            if scene == 4 {
                v = v * 2. + 0.125;
            }
            v
        })
        .collect();
    RawFrame { header, pixels }
}
fn compare(gpu: &mut GpuReconstructor, frames: &[&RawFrame], cancel: &AtomicBool) {
    let output = gpu
        .reconstruct(frames, cancel)
        .expect("hardware reconstruction");
    for (raw, (got, corrected)) in frames.iter().zip(output) {
        let (expected, stats) = debayer(raw, cancel, DemosaicMethod::Vng).unwrap();
        assert_eq!(
            corrected, stats.corrected_pixels,
            "cosmetic selection must agree"
        );
        let mut maximum = 0_f32;
        let mut squares = 0_f64;
        for (i, (a, b)) in expected.pixels.iter().zip(&got.pixels).enumerate() {
            let c = match raw
                .header
                .info
                .bayer_pattern
                .color(i % got.width, i / got.width)
            {
                Color::Red => 0,
                Color::Green => 1,
                Color::Blue => 2,
            };
            assert_eq!(a[c], b[c], "native cleaned sample");
            for c in 0..3 {
                assert!(b[c].is_finite());
                let d = (a[c] - b[c]).abs();
                maximum = maximum.max(d);
                squares += (d as f64).powi(2);
            }
        }
        let rms = (squares / (got.pixels.len() * 3) as f64).sqrt();
        println!(
            "T-NUM-01 {:?} {}x{}: max={maximum:.9} ADU RMS={rms:.9} ADU corrected={corrected}",
            raw.header.info.bayer_pattern, got.width, got.height
        );
        assert!(
            maximum <= 0.05,
            "stage difference {maximum} ADU exceeds predeclared 0.05"
        );
    }
}
#[test]
fn hardware_cosmetic_vng_matches_cpu_for_phases_stars_noise_borders_and_real_frame() {
    if std::env::var("FULGURSTACK_TEST_GPU").as_deref() != Ok("1") {
        eprintln!("SKIP hardware GPU gate; set FULGURSTACK_TEST_GPU=1 to require a real adapter.");
        return;
    }
    let cancel = AtomicBool::new(false);
    let mut gpu = GpuReconstructor::new(&gpu_plans())
        .expect("explicit hardware GPU gate must not silently skip/fallback");
    let mut inputs = Vec::new();
    for pattern in [
        BayerPattern::GRBG,
        BayerPattern::RGGB,
        BayerPattern::GBRG,
        BayerPattern::BGGR,
    ] {
        for scene in 0..5 {
            inputs.push(frame(pattern, [128, 96], scene));
        }
        inputs.push(frame(pattern, [6, 6], 0));
        inputs.push(frame(pattern, [37, 29], 1));
    }
    for batch in inputs.chunks(16) {
        compare(&mut gpu, &batch.iter().collect::<Vec<_>>(), &cancel);
    }
    assert!(
        gpu.reconstruct(&[&inputs[0]], &AtomicBool::new(true))
            .is_err()
    );
    if let Some(path) = std::env::var_os("FULGURSTACK_TEST_FITS") {
        let raw = fulgur_core::fits::read_raw(std::path::Path::new(&path)).unwrap();
        compare(&mut gpu, &[&raw], &cancel);
    }
    println!("GPU telemetry: {:?}", gpu.telemetry());
    assert!(gpu.telemetry().batches > 0);
}
