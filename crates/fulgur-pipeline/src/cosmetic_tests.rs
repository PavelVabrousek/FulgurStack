use super::*;
use fulgur_core::{BayerPattern, fits::ImageHeader};
use std::io::Cursor;
use std::sync::atomic::Ordering;

pub(super) fn frame(pattern: BayerPattern, width: usize, height: usize, field: usize) -> RawFrame {
    let mut bytes = Vec::new();
    for (key, value) in [
        ("SIMPLE", "T".to_owned()),
        ("BITPIX", "16".into()),
        ("NAXIS", "2".into()),
        ("NAXIS1", width.to_string()),
        ("NAXIS2", height.to_string()),
        ("BAYERPAT", format!("'{pattern:?}'")),
        ("EXPTIME", "10".into()),
    ] {
        bytes.extend(format!("{:<80}", format!("{key:<8}= {value:>20}")).as_bytes());
    }
    bytes.extend(format!("{:<80}", "END").as_bytes());
    bytes.resize(2880, b' ');
    let header = ImageHeader::read(
        &mut Cursor::new(bytes),
        (2880 + (width * height * 2).div_ceil(2880) * 2880) as u64,
    )
    .unwrap();
    let pixels = (0..width * height)
        .map(|i| {
            let x = i % width;
            let y = i / width;
            let color = match pattern.color(x, y) {
                Color::Red => 0,
                Color::Green => 1,
                Color::Blue => 2,
            };
            match field {
                0 => [1500.125, 1000.25, 700.5][color],
                1 => ((i * 7919 + 12345) % 65536) as f32 - (32768. + 0.03125),
                2 => ((i * 59) % 37) as f32 * 0.00003125,
                3 => {
                    if (x + y) % 3 == 0 {
                        65535.
                    } else {
                        0.
                    }
                }
                4 => {
                    let r2 = (x as f64 - width as f64 * 0.51).powi(2)
                        + (y as f64 - height as f64 * 0.47).powi(2);
                    (2000. + [90000., 70000., 40000.][color] * (-r2 / 3.).exp()).min(65535.) as f32
                }
                _ => x as f32 * 13.0625 - y as f32 * 7.125 + color as f32 * 0.03125,
            }
        })
        .collect();
    RawFrame { header, pixels }
}

fn compare(raw: &RawFrame) {
    let original = raw.pixels.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
    let cancel = AtomicBool::new(false);
    let (expected, expected_count) = cosmetic_reference::cosmetic_correct(raw, &cancel).unwrap();
    let (actual, actual_count) = cosmetic_correct(raw, &cancel).unwrap();
    assert_eq!(actual_count, expected_count);
    assert_eq!(
        actual.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        expected.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
    );
    assert_eq!(
        raw.pixels.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        original
    );
}

#[test]
fn cosmetic_matches_scalar_bits_for_noise_defects_stars_phases_and_edges() {
    for pattern in [
        BayerPattern::GRBG,
        BayerPattern::RGGB,
        BayerPattern::GBRG,
        BayerPattern::BGGR,
    ] {
        for [width, height] in [[6, 6], [7, 9], [9, 9], [18, 19], [65, 64], [127, 131]] {
            for field in 0..6 {
                let mut raw = frame(pattern, width, height, field);
                compare(&raw);
                // Isolated, border and adjacent defects against the same immutable CFA.
                for (x, y) in [
                    (0, 0),
                    (3, 4),
                    (4, 4),
                    (width / 2, height / 2),
                    (width - 4, height - 4),
                ] {
                    if x < width && y < height {
                        raw.pixels[y * width + x] = 65535.;
                    }
                }
                compare(&raw);
            }
        }
    }
    let mut raw = frame(BayerPattern::GRBG, 9, 9, 0);
    let cancel = AtomicBool::new(true);
    assert!(matches!(
        cosmetic_correct(&raw, &cancel),
        Err(Error::Cancelled)
    ));
    cancel.store(false, Ordering::Relaxed);
    raw.pixels.pop();
    assert!(cosmetic_correct(&raw, &cancel).is_err());
}

#[test]
fn cosmetic_matches_scalar_real_frame_and_preserves_source() {
    let Ok(path) = std::env::var("FULGURSTACK_TEST_FITS") else {
        return;
    };
    let raw = fulgur_core::fits::read_raw(std::path::Path::new(&path)).unwrap();
    compare(&raw);
}

#[test]
fn cosmetic_noise_floor_shortcut_matches_scalar_at_fractional_boundaries() {
    let mut raw = frame(BayerPattern::GRBG, 33, 35, 0);
    for background in [0., -1000.25, 10000.125, f32::MAX * 0.25, -f32::MAX * 0.25] {
        for delta in [-100., -0.0001, 0., 4.999, 5., 5.001, 100., 65535.] {
            for noise in [0., 0.125, 1., 8., 64.] {
                for (i, v) in raw.pixels.iter_mut().enumerate() {
                    *v = background + ((i * 7919 % 17) as f32 - 8.) * noise;
                }
                raw.pixels[17 * 33 + 16] = background + delta;
                compare(&raw);
            }
        }
    }
}
