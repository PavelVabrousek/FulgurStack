use fulgur_core::fits::{FitsHeader, read_raw, read_raw_sequential};
use fulgur_core::output::{OUTPUT_ADU_DIVISOR, atomic_write, write_rgb_atomic};
use fulgur_core::{BayerPattern, DemosaicMethod, Error, RgbImage};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};

fn fixture(extra: &[(&str, &str)], omit: &[&str]) -> Vec<u8> {
    let defaults = [
        ("SIMPLE", "T"),
        ("BITPIX", "16"),
        ("NAXIS", "2"),
        ("NAXIS1", "6"),
        ("NAXIS2", "6"),
        ("BZERO", "32768"),
        ("BSCALE", "1"),
        ("BAYERPAT", "'GRBG'"),
        ("EXPTIME", "10."),
        ("BIAS", "2064"),
        ("SITELAT", "12.3456789"),
        ("SITELONG", "45.6789123"),
        ("DATE-OBS", "'2026-10-06T18:45:12.145756'"),
        ("CRVAL1", "123.45"),
    ];
    let mut bytes = Vec::new();
    for (key, val) in defaults.iter().chain(extra) {
        if !omit.contains(key) {
            let text = format!("{key:<8}= {val:>20}");
            let mut card = [b' '; 80];
            card[..text.len()].copy_from_slice(text.as_bytes());
            bytes.extend(card);
        }
    }
    bytes.extend(format!("{:<80}", "END").as_bytes());
    bytes.resize(bytes.len().div_ceil(2880) * 2880, b' ');
    for index in 0..36 {
        let physical = [0_i32, 2064, 32767, 32768, 65535, 12345][index % 6];
        bytes.extend(((physical - 32768) as i16).to_be_bytes());
    }
    bytes.resize(bytes.len().div_ceil(2880) * 2880, 0);
    bytes
}

fn header(bytes: &[u8]) -> fulgur_core::Result<FitsHeader> {
    FitsHeader::read(&mut Cursor::new(bytes), bytes.len() as u64)
}

fn image() -> RgbImage {
    RgbImage {
        width: 6,
        height: 6,
        demosaic_method: DemosaicMethod::Mhc,
        pixels: (0..36).map(|i| [i as f32 + 0.125, 2.5, 65000.0]).collect(),
    }
}

#[test]
fn signed_big_endian_bzero_and_bias_are_correct() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("source.fit");
    fs::write(&input, fixture(&[], &[])).unwrap();
    let before = Sha256::digest(fs::read(&input).unwrap());
    let frame = read_raw(&input).unwrap();
    assert_eq!(read_raw_sequential(&input).unwrap().pixels, frame.pixels);
    assert_eq!(
        &frame.pixels[..6],
        &[0.0, 2064.0, 32767.0, 32768.0, 65535.0, 12345.0]
    );
    assert_eq!(frame.header.info.bayer_pattern, BayerPattern::GRBG);
    assert_eq!(before, Sha256::digest(fs::read(&input).unwrap()));
}

#[test]
fn quoted_slashes_and_escaped_quotes_are_preserved() {
    let bytes = fixture(
        &[
            ("OBJECT", "'M 33 / field ''A'''"),
            ("COMMENT", "'retained privately'"),
        ],
        &[],
    );
    let header = header(&bytes).unwrap();
    assert_eq!(header.info.object.as_deref(), Some("M 33 / field 'A'"));
    assert!(header.raw_card("COMMENT").is_some());
}

#[test]
fn header_debug_redacts_observing_coordinates() {
    let header = header(&fixture(&[], &[])).unwrap();
    let debug = format!("{header:?}");
    assert!(!debug.contains("12.3456789"));
    assert!(!debug.contains("45.6789123"));
    assert!(!debug.contains("SITELAT"));
    assert!(header.valid_gps_cards().is_some());
}

#[test]
fn invalid_and_duplicate_metadata_are_rejected_without_echoing_values() {
    for (extra, omit) in [
        (vec![("BITPIX", "32")], vec!["BITPIX"]),
        (vec![("BSCALE", "2")], vec!["BSCALE"]),
        (vec![("EXPTIME", "0")], vec!["EXPTIME"]),
        (vec![("EXPTIME", "NaN")], vec!["EXPTIME"]),
        (vec![("XBAYROFF", "1")], vec![]),
        (vec![("BLANK", "-32768")], vec![]),
        (vec![("NAXIS1", "999999999999999999")], vec!["NAXIS1"]),
        (vec![("EXPTIME", "20")], vec![]),
    ] {
        // Replace cards separately, while preserving intentionally duplicated EXPTIME.
        let mut bytes = fixture(&[], &omit);
        let end_index = bytes
            .chunks_exact(80)
            .position(|card| card.starts_with(b"END "))
            .unwrap();
        for (index, (key, val)) in extra.iter().enumerate() {
            let text = format!("{key:<8}= {val:>20}");
            let start = (end_index + index) * 80;
            bytes[start..start + 80].fill(b' ');
            bytes[start..start + text.len()].copy_from_slice(text.as_bytes());
        }
        let start = (end_index + extra.len()) * 80;
        bytes[start..start + 80].fill(b' ');
        bytes[start..start + 3].copy_from_slice(b"END");
        assert!(header(&bytes).is_err());
    }
}

#[test]
fn truncated_header_pixels_and_padding_fail() {
    let bytes = fixture(&[], &[]);
    assert!(header(&bytes[..100]).is_err());
    assert!(header(&bytes[..2880 + 71]).is_err());
    assert!(header(&bytes[..bytes.len() - 1]).is_err());
}

#[test]
fn missing_exposure_and_unsupported_cfa_fail() {
    assert!(header(&fixture(&[], &["EXPTIME"])).is_err());
    let mut bytes = fixture(&[], &[]);
    let pattern = bytes.windows(4).position(|chunk| chunk == b"GRBG").unwrap();
    bytes[pattern..pattern + 4].copy_from_slice(b"MONO");
    assert!(header(&bytes).is_err());
}

#[test]
fn rgb_export_is_planar_big_endian_linear_and_has_clean_metadata() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("rgb.fits");
    let source = header(&fixture(&[], &[])).unwrap();
    let rgb = image();
    write_rgb_atomic(&path, &rgb, &source, &AtomicBool::new(false)).unwrap();
    let bytes = fs::read(&path).unwrap();
    let text = String::from_utf8_lossy(&bytes[..2880]);
    for absent in ["BZERO", "BSCALE", "BIAS", "BAYERPAT", "CRVAL1", "CHECKSUM"] {
        assert!(!text.contains(absent) || absent == "BIAS"); // HISTORY explicitly says no BIAS subtraction.
        assert!(
            !bytes[..2880]
                .chunks_exact(80)
                .any(|card| { card[..8] == format!("{absent:<8}").as_bytes()[..] })
        );
    }
    assert!(text.contains("SITELAT"));
    assert!(text.contains("12.3456789"));
    assert!(text.contains("2026-10-06T18:45:12.145756"));
    assert!(text.contains("EXPTIME"));
    for channel in 0..3 {
        for index in 0..36 {
            let offset = 2880 + 4 * (channel * 36 + index);
            assert_eq!(
                f32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()),
                rgb.pixels[index][channel] / OUTPUT_ADU_DIVISOR
            );
        }
    }
    assert_eq!(bytes.len() % 2880, 0);
    assert!(!path.with_extension("fits.tmp").exists());
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn export_bounds_reconstruction_without_rescaling_valid_samples_or_mutating_pixels() {
    let directory = tempfile::tempdir().unwrap();
    let source = header(&fixture(&[], &[])).unwrap();
    let mut rgb = image();
    rgb.pixels[0] = [-5222.25, 0.0, 86902.59];
    rgb.pixels[1] = [65535.0, 0.125, 2064.0];
    let original = rgb.pixels.clone();
    for iteration in 0..2 {
        let path = directory
            .path()
            .join(format!("normalized-{iteration}.fits"));
        let stats = write_rgb_atomic(&path, &rgb, &source, &AtomicBool::new(false)).unwrap();
        let bytes = fs::read(path).unwrap();
        for channel in 0..3 {
            for index in 0..36 {
                let offset = 2880 + 4 * (channel * 36 + index);
                let value = f32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap());
                assert_eq!(
                    value,
                    (rgb.pixels[index][channel] / 65535.0).clamp(0.0, 1.0)
                );
            }
        }
        assert_eq!(stats.output_min, 0.0);
        assert_eq!(stats.output_max, 1.0);
        assert_eq!(stats.samples_below_zero, 0);
        assert_eq!(stats.samples_above_one, 0);
        assert_eq!(
            stats.samples_limited_low,
            if iteration == 0 { 1 } else { 2 }
        );
        assert_eq!(
            stats.samples_limited_high,
            if iteration == 0 { 1 } else { 3 }
        );
        assert_eq!(stats.samples_clipped, if iteration == 0 { 2 } else { 5 });
        let text = String::from_utf8_lossy(&bytes[..2880]);
        assert!(text.contains("adu/65535"));
        assert!(text.contains("FLXDIV"));
        assert!(text.contains("RGBBOUND"));
        assert!(text.contains("bounded to sensor range"));
        assert!(!text.contains("OpenFulgurStack.js"));
        assert!(text.contains("TOP-DOWN"));
        if iteration == 0 {
            assert_eq!(rgb.pixels, original); // Export never mutates reconstruction data.
        }
        // Alter image extrema and channel ratios; existing samples keep the same scale.
        rgb.pixels[35] = [-30000.0, 200000.0, 75000.0];
    }
}

#[test]
fn existing_source_and_hardlink_alias_are_never_overwritten() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("source.fit");
    let alias = directory.path().join("alias.fit");
    let bytes = fixture(&[], &[]);
    fs::write(&path, &bytes).unwrap();
    fs::hard_link(&path, &alias).unwrap();
    let source = header(&bytes).unwrap();
    for target in [&path, &alias] {
        assert!(write_rgb_atomic(target, &image(), &source, &AtomicBool::new(false)).is_err());
    }
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn partial_write_failure_removes_only_its_temporary_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("broken.fits");
    let result = atomic_write(&path, &AtomicBool::new(false), |writer| {
        writer.write_all(b"partial")?;
        Err(Error::Invalid("Injected write failure."))
    });
    assert!(result.is_err());
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn cancellation_before_publication_cleans_partial_output() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cancelled.fits");
    let cancel = AtomicBool::new(false);
    let result = atomic_write(&path, &cancel, |writer| {
        writer.write_all(b"partial")?;
        cancel.store(true, Ordering::Relaxed);
        Ok(())
    });
    assert!(matches!(result, Err(Error::Cancelled)));
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn destination_created_during_write_wins_without_clobbering() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("race.fits");
    assert!(
        atomic_write(&path, &AtomicBool::new(false), |writer| {
            writer.write_all(b"ours")?;
            fs::write(&path, b"other writer")?;
            Ok(())
        })
        .is_err()
    );
    assert_eq!(fs::read(&path).unwrap(), b"other writer");
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn stale_temporary_file_is_not_removed_or_replaced() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("output.fits");
    let stale = directory.path().join("output.fits.tmp");
    fs::write(&stale, b"existing partial file").unwrap();
    assert!(atomic_write(&path, &AtomicBool::new(false), |_| Ok(())).is_err());
    assert_eq!(fs::read(&stale).unwrap(), b"existing partial file");
    assert!(!path.exists());
}

#[test]
fn invalid_rgb_buffer_and_nonfinite_pixels_do_not_publish_output() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("invalid.fits");
    let source = header(&fixture(&[], &[])).unwrap();
    let mut rgb = image();
    rgb.pixels[0][0] = f32::NAN;
    assert!(write_rgb_atomic(&path, &rgb, &source, &AtomicBool::new(false)).is_err());
    rgb.pixels.pop();
    assert!(write_rgb_atomic(&path, &rgb, &source, &AtomicBool::new(false)).is_err());
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn optional_real_input_decodes_without_changing_source() {
    let Some(path) = std::env::var_os("FULGURSTACK_TEST_FITS") else {
        return;
    };
    let path = std::path::Path::new(&path);
    let before = fs::read(path).unwrap();
    let digest = Sha256::digest(&before);
    let frame = read_raw(path).unwrap();
    assert_eq!(read_raw_sequential(path).unwrap().pixels, frame.pixels);
    let offset = frame.header.data_offset as usize;
    for (pair, &actual) in before[offset..offset + frame.header.data_bytes]
        .chunks_exact(2)
        .zip(&frame.pixels)
    {
        let expected = f32::from(i16::from_be_bytes([pair[0], pair[1]])) + frame.header.info.bzero;
        assert_eq!(actual, expected);
    }
    assert_eq!(
        frame.pixels.len(),
        frame.header.info.width * frame.header.info.height
    );
    assert_eq!(digest, Sha256::digest(fs::read(path).unwrap()));
}

#[test]
fn large_finite_exposure_is_written_without_header_overflow() {
    let mut bytes = fixture(&[], &[]);
    let index = bytes
        .chunks_exact(80)
        .position(|card| card.starts_with(b"EXPTIME "))
        .unwrap();
    bytes[index * 80..(index + 1) * 80]
        .copy_from_slice(format!("{:<80}", "EXPTIME =              1.0E300").as_bytes());
    let source = header(&bytes).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("long-exposure.fits");
    write_rgb_atomic(&path, &image(), &source, &AtomicBool::new(false)).unwrap();
    let exported = fs::read(path).unwrap();
    let exposure_card = exported[..2880]
        .chunks_exact(80)
        .find(|card| card.starts_with(b"TEXPTIME"))
        .unwrap();
    let exposure = std::str::from_utf8(&exposure_card[10..])
        .unwrap()
        .trim()
        .parse::<f64>()
        .unwrap();
    assert!((exposure / 1.0E300 - 1.0).abs() < 1.0E-12);
}

#[test]
fn integrated_master_records_crop_exposure_and_anchor_gps_without_false_wcs() {
    let dir = tempfile::tempdir().unwrap();
    let source = header(&fixture(&[], &[])).unwrap();
    let path = dir.path().join("master.fits");
    let image = RgbImage {
        width: 3,
        height: 2,
        demosaic_method: DemosaicMethod::Vng,
        pixels: vec![[2000., 4000., 6000.]; 6],
    };
    let metadata = fulgur_core::output::MasterMetadata {
        geometric_correction_frames: 0,
        external_geometry_scale: None,
        accepted_frames: 27,
        total_exposure_seconds: 270.,
        origin_x_px: -4,
        origin_y_px: 7,
    };
    fulgur_core::output::write_master_atomic(
        &path,
        &image,
        &source,
        &metadata,
        &AtomicBool::new(false),
    )
    .unwrap();
    let bytes = fs::read(&path).unwrap();
    let text = String::from_utf8_lossy(&bytes[..5760]);
    for (key, value) in [
        ("NCOMBINE", "27"),
        ("CROPX", "-4"),
        ("CROPY", "7"),
        ("NAXIS1", "3"),
        ("NAXIS2", "2"),
    ] {
        assert!(
            text.as_bytes()
                .chunks(80)
                .any(|c| String::from_utf8_lossy(c).starts_with(key)
                    && String::from_utf8_lossy(c).contains(value))
        );
    }
    assert!(text.contains("2.700000000000000E2"));
    assert!(text.contains("SITELAT"));
    assert!(text.contains("SITELONG"));
    assert!(text.contains("EXPTIME =                  10."));
    assert!(!text.contains("CRVAL1"));
    assert!(!text.contains("BZERO"));
    assert_eq!(
        f32::from_be_bytes(
            bytes[((bytes
                .chunks_exact(80)
                .position(|c| c.starts_with(b"END "))
                .unwrap()
                + 1)
                * 80)
                .div_ceil(2880)
                * 2880..][..4]
                .try_into()
                .unwrap()
        ),
        2000. / 65535.
    );
}

#[test]
fn roi_decoder_checks_phase_bounds_and_cancellation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.fit");
    fs::write(&path, fixture(&[], &[])).unwrap();
    let source = fulgur_core::fits::read_header(&path).unwrap();
    let cancel = AtomicBool::new(false);
    let roi = fulgur_core::fits::read_roi(&path, &source, [0, 0, 6, 6], &cancel).unwrap();
    let full = read_raw(&path).unwrap();
    assert_eq!(roi.pixels, full.pixels);
    for bounds in [
        [1, 0, 6, 6],
        [0, 1, 6, 6],
        [0, 0, 7, 6],
        [0, 0, usize::MAX, 6],
    ] {
        assert!(fulgur_core::fits::read_roi(&path, &source, bounds, &cancel).is_err());
    }
    assert!(
        fulgur_core::fits::read_roi(&path, &source, [0, 0, 6, 6], &AtomicBool::new(true)).is_err()
    );
}

#[test]
fn fraction_maps_write_counts_directly_without_photometric_scaling_or_clipping() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("coverage.fits");
    let mut image = RgbImage {
        width: 2,
        height: 1,
        demosaic_method: DemosaicMethod::Vng,
        pixels: vec![[0.5, 0.75, 0.]; 2],
    };
    let cancel = AtomicBool::new(false);
    fulgur_core::output::write_fraction_map_atomic(&path, &image, "COVERAGE", 32, &cancel).unwrap();
    let bytes = fs::read(&path).unwrap();
    assert_eq!(
        f32::from_be_bytes(bytes[2880..2884].try_into().unwrap()),
        0.5
    );
    let header = String::from_utf8_lossy(&bytes[..2880]);
    assert!(header.contains("NINPUT"));
    assert!(header.contains("PCTFACT"));
    assert!(!header.contains("FLXDIV"));
    assert!(!header.contains("BZERO"));
    image.pixels[0][0] = 1.01;
    assert!(
        fulgur_core::output::write_fraction_map_atomic(
            &dir.path().join("invalid.fits"),
            &image,
            "COVERAGE",
            32,
            &cancel
        )
        .is_err()
    );
    assert!(!dir.path().join("invalid.fits").exists());
}

#[test]
fn geometry_reference_reads_rgb_green_scaling_orientation_and_rejects_invalid_data() {
    use fulgur_core::fits::read_reference_plane;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("reference.fits");
    let make = |order: &str, bad: bool| {
        let mut bytes = Vec::new();
        for (k, v) in [
            ("SIMPLE", "T"),
            ("BITPIX", "-32"),
            ("NAXIS", "3"),
            ("NAXIS1", "6"),
            ("NAXIS2", "8"),
            ("NAXIS3", "3"),
            ("ROWORDER", order),
            ("BSCALE", "2"),
            ("BZERO", "10"),
        ] {
            bytes.extend(format!("{:<80}", format!("{k:<8}= {v:>20}")).as_bytes());
        }
        bytes.extend(format!("{:<80}", "END").as_bytes());
        bytes.resize(2880, b' ');
        for channel in 0..3 {
            for i in 0..48 {
                let value = if bad && channel == 1 && i == 17 {
                    f32::NAN
                } else {
                    channel as f32 * 100. + i as f32
                };
                bytes.extend(value.to_be_bytes());
            }
        }
        bytes.resize(bytes.len().div_ceil(2880) * 2880, 0);
        bytes
    };
    let cancel = AtomicBool::new(false);
    let bytes = make("'TOP-DOWN'", false);
    fs::write(&path, &bytes).unwrap();
    let p = read_reference_plane(&path, &cancel).unwrap();
    assert_eq!((p.width, p.height), (6, 8));
    assert_eq!(p.pixels[0], 210.);
    assert_eq!(p.pixels[47], 304.);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    fs::write(&path, make("'BOTTOM-UP'", false)).unwrap();
    let p = read_reference_plane(&path, &cancel).unwrap();
    assert_eq!(p.pixels[0], 294.);
    assert_eq!(p.pixels[42], 210.);
    assert!(read_reference_plane(&path, &AtomicBool::new(true)).is_err());
    fs::write(&path, make("'TOP-DOWN'", true)).unwrap();
    assert!(read_reference_plane(&path, &cancel).is_err());
    fs::write(&path, make("'UNKNOWN'", false)).unwrap();
    assert!(read_reference_plane(&path, &cancel).is_err());
    fs::write(&path, &bytes[..bytes.len() - 1]).unwrap();
    assert!(read_reference_plane(&path, &cancel).is_err());
}
