use super::*;
use fulgur_core::{BayerPattern, fits::FitsHeader};
use std::io::Cursor;
use std::sync::atomic::Ordering;

fn frame(pattern: BayerPattern, width: usize, height: usize, field: usize) -> RawFrame {
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
    let header = FitsHeader::read(
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

#[test]
fn generated_vng_matches_prior_scalar_bits_for_phases_fields_and_edges() {
    let cancel = AtomicBool::new(false);
    for pattern in [
        BayerPattern::GRBG,
        BayerPattern::RGGB,
        BayerPattern::GBRG,
        BayerPattern::BGGR,
    ] {
        for [width, height] in [[6, 6], [7, 9], [18, 19], [65, 64], [127, 131]] {
            for field in 0..6 {
                let raw = frame(pattern, width, height, field);
                let before = raw.pixels.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
                let expected = reference::vng_demosaic(&raw, &raw.pixels, &cancel).unwrap();
                let actual = vng_demosaic(&raw, &raw.pixels, &cancel).unwrap();
                for (i, (a, b)) in actual.pixels.iter().zip(&expected.pixels).enumerate() {
                    assert_eq!(
                        a.map(f32::to_bits),
                        b.map(f32::to_bits),
                        "pattern={pattern:?}, size={width}x{height}, field={field}, pixel={i}"
                    );
                }
                assert_eq!(
                    raw.pixels.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
                    before
                );
            }
        }
    }
    let raw = frame(BayerPattern::GRBG, 7, 9, 1);
    cancel.store(true, Ordering::Relaxed);
    assert!(matches!(
        vng_demosaic(&raw, &raw.pixels, &cancel),
        Err(Error::Cancelled)
    ));
    assert!(vng_demosaic(&raw, &raw.pixels[..2], &AtomicBool::new(false)).is_err());
}

#[test]
fn generated_vng_matches_prior_real_frame_before_and_after_cosmetics() {
    let Ok(path) = std::env::var("FULGURSTACK_TEST_FITS") else {
        return;
    };
    let raw = fulgur_core::fits::read_raw(std::path::Path::new(&path)).unwrap();
    let cancel = AtomicBool::new(false);
    let (cleaned, _) = crate::debayer::cosmetic_correct(&raw, &cancel).unwrap();
    for cfa in [&raw.pixels, &cleaned] {
        let expected = reference::vng_demosaic(&raw, cfa, &cancel).unwrap();
        let actual = vng_demosaic(&raw, cfa, &cancel).unwrap();
        for (i, (a, b)) in actual.pixels.iter().zip(&expected.pixels).enumerate() {
            assert_eq!(
                a.map(f32::to_bits),
                b.map(f32::to_bits),
                "real frame pixel={i}"
            );
        }
    }
}

#[cfg(feature = "gpu")]
#[test]
fn shared_stencil_preserves_all_original_gpu_coefficient_words() {
    assert_eq!(gpu_plans(), reference::gpu_plans());
}
