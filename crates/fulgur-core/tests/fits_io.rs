use fulgur_core::fits::{
    ImageHeader, ReadBuffers, read_raw, read_raw_buffered, read_raw_sequential,
};
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

fn header(bytes: &[u8]) -> fulgur_core::Result<ImageHeader> {
    ImageHeader::read(&mut Cursor::new(bytes), bytes.len() as u64)
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
    let mut buffers = ReadBuffers::default();
    for _ in 0..2 {
        let again = read_raw_buffered(&input, &mut buffers).unwrap();
        assert_eq!(again.pixels, frame.pixels);
        buffers.give_back(again.pixels);
    }
    assert_eq!(
        &frame.pixels[..6],
        &[0.0, 2064.0, 32767.0, 32768.0, 65535.0, 12345.0]
    );
    assert_eq!(frame.header.info.bayer_pattern, Some(BayerPattern::GRBG));
    assert_eq!(frame.header.info.mosaic, fulgur_core::Mosaic::Bayer);
    assert_eq!(frame.header.info.mosaic_source, "header");
    assert_eq!(before, Sha256::digest(fs::read(&input).unwrap()));
}

#[test]
fn missing_bayer_pattern_means_mono_unless_the_owner_declares_another_layout() {
    use fulgur_core::{LayoutChoice, Mosaic};
    let mut header = header(&fixture(&[], &["BAYERPAT"])).unwrap();
    assert_eq!(header.info.mosaic, Mosaic::Mono);
    assert_eq!(header.info.mosaic_source, "no_bayer_header");
    assert_eq!(header.info.bayer_pattern, None);
    header.info.apply_layout(LayoutChoice::Auto);
    assert_eq!(
        header.info.mosaic,
        Mosaic::Mono,
        "auto keeps the header rule"
    );
    header.info.apply_layout(LayoutChoice::Mono);
    assert_eq!(
        (
            header.info.mosaic,
            header.info.bayer_pattern,
            header.info.mosaic_source
        ),
        (Mosaic::Mono, None, "user")
    );
    header
        .info
        .apply_layout(LayoutChoice::parse("rggb").unwrap());
    assert_eq!(header.info.bayer_pattern, Some(BayerPattern::RGGB));
    assert_eq!(header.info.mosaic, Mosaic::Bayer);
    assert!(LayoutChoice::parse("cmyk").is_err());
    assert_eq!(LayoutChoice::parse("MONO").unwrap().name(), "mono");
}

#[test]
fn set_metadata_reads_calibration_cards_without_pixels_or_location() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("flat.fit");
    fs::write(
        &path,
        fixture(
            &[
                ("IMAGETYP", "'Flat Field'"),
                ("FILTER", "'IRCUT'"),
                ("INSTRUME", "'Seestar S50'"),
                ("CCD-TEMP", "14.5"),
                ("NCOMBINE", "40"),
            ],
            &[],
        ),
    )
    .unwrap();
    let meta = fulgur_core::fits::read_metadata(&path).unwrap();
    assert_eq!(meta.image_type.as_deref(), Some("Flat Field"));
    assert_eq!(meta.filter.as_deref(), Some("IRCUT"));
    assert_eq!(meta.instrument.as_deref(), Some("Seestar S50"));
    assert_eq!(meta.camera().as_deref(), Some("Seestar S50"));
    let seestar = fulgur_core::fits::FileMetadata {
        creator: Some("ZWO Seestar S50 Pro".into()),
        ..Default::default()
    };
    assert_eq!(seestar.camera().as_deref(), Some("Seestar S50 Pro"));
    assert_eq!(meta.ccd_temperature, Some(14.5));
    assert_eq!(meta.combined_frames, Some(40));
    assert_eq!(meta.bayer_pattern, Some(BayerPattern::GRBG));
    assert_eq!((meta.bitpix, meta.planes), (16, 1));
    assert!(!format!("{meta:?}").contains("12.3456789"));
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
fn invalid_exposure_and_unsupported_cfa_fail() {
    // A missing EXPTIME reads as 0 s (inspection rejects it, ADR-084); a
    // recorded but non-positive or non-numeric one is still invalid.
    let missing = header(&fixture(&[], &["EXPTIME"])).unwrap();
    assert_eq!(missing.info.exposure_seconds, 0.0);
    assert!(missing.info.derived.is_empty());
    for value in ["0.", "-5.", "'long'"] {
        let mut bytes = fixture(&[], &[]);
        let card = bytes.windows(8).position(|w| w == b"EXPTIME ").unwrap();
        bytes[card + 10..card + 30].copy_from_slice(format!("{value:>20}").as_bytes());
        assert!(header(&bytes).is_err(), "{value}");
    }
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
        rejection: "winsorized",
        geometric_correction_frames: 0,
        external_geometry_scale: None,
        accepted_frames: 27,
        total_exposure_seconds: 270.,
        origin_x_px: -4,
        origin_y_px: 7,
        monochrome_frames: 0,
        calibrated_frames: 0,
        calibration_formula: None,
        astrometry: None,
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

#[test]
fn missing_header_values_are_derived_from_cards_file_name_and_camera_catalogue() {
    use fulgur_core::fits::{read_header, read_light_metadata};
    use fulgur_core::{LayoutChoice, Mosaic};
    let directory = tempfile::tempdir().unwrap();
    let name = "c_Light_M 42_270deg_30.0s_Bin1_2600MC_gain100_20260119-200800_-20.0C_SQA_0001.fit";
    let path = directory.path().join(name);
    let camera = [("INSTRUME", "'ZWO ASI2600MC Air'")];
    fs::write(&path, fixture(&camera, &["BAYERPAT", "EXPTIME"])).unwrap();
    let before = Sha256::digest(fs::read(&path).unwrap());
    let info = read_header(&path).unwrap().info;
    assert_eq!(info.exposure_seconds, 30.0);
    assert_eq!(info.bayer_pattern, Some(BayerPattern::RGGB));
    assert_eq!(
        (info.mosaic, info.mosaic_source),
        (Mosaic::Bayer, "camera_catalog")
    );
    assert_eq!(info.gain, Some(100.0));
    assert_eq!(info.object.as_deref(), Some("M 42"));
    assert!(info.needs_confirmation());
    let fields: Vec<_> = info.derived.iter().map(|d| d.field).collect();
    assert_eq!(
        fields,
        ["exposure_seconds", "sensor_layout", "gain", "object"]
    );
    assert!(
        info.derived[1]
            .source
            .contains("INSTRUME 'ZWO ASI2600MC Air'")
    );
    // The decoder path derives the same values; the source stays untouched.
    let raw = read_raw(&path).unwrap();
    assert_eq!(raw.header.info.derived, info.derived);
    assert_eq!(before, Sha256::digest(fs::read(&path).unwrap()));
    // Planning metadata shows what processing will use.
    let meta = read_light_metadata(&path).unwrap();
    assert_eq!(meta.exposure_seconds, Some(30.0));
    assert_eq!(meta.bayer_pattern, Some(BayerPattern::RGGB));
    assert_eq!(meta.derived, info.derived);
    // Owner corrections replace the derived values they correct.
    let mut corrected = info.clone();
    corrected.apply_layout(LayoutChoice::Mono);
    corrected.apply_exposure(Some(31.5));
    assert_eq!(corrected.exposure_seconds, 31.5);
    assert!(!corrected.needs_confirmation(), "only labels remain");
    // Other header cards win over the file name.
    fs::write(
        &path,
        fixture(
            &[camera[0], ("EXPOSURE", "60."), ("COLORTYP", "'GBRG'")],
            &["BAYERPAT", "EXPTIME"],
        ),
    )
    .unwrap();
    let info = read_header(&path).unwrap().info;
    assert_eq!(info.exposure_seconds, 60.0);
    assert_eq!(info.derived[0].source, "header card EXPOSURE");
    assert_eq!(info.bayer_pattern, Some(BayerPattern::GBRG));
    // A catalogued mono camera keeps the ADR-078 default and needs nothing.
    let plain = directory.path().join("frame_0001.fit");
    fs::write(
        &plain,
        fixture(&[("INSTRUME", "'ZWO ASI2600MM Air'")], &["BAYERPAT"]),
    )
    .unwrap();
    let info = read_header(&plain).unwrap().info;
    assert_eq!(
        (info.mosaic, info.mosaic_source),
        (Mosaic::Mono, "no_bayer_header")
    );
    assert!(info.derived.is_empty());
    // A colour camera with an uncatalogued pattern must be declared.
    fs::write(
        &plain,
        fixture(&[("INSTRUME", "'ZWO ASI120MC-S'")], &["BAYERPAT"]),
    )
    .unwrap();
    let info = read_header(&plain).unwrap().info;
    assert_eq!(info.mosaic, Mosaic::Unknown);
    assert!(info.needs_confirmation());
}

/// The data block of an XISF unit written by FulgurStack, with its header.
fn xisf_parts(bytes: &[u8]) -> (String, &[u8]) {
    assert_eq!(&bytes[..8], b"XISF0100");
    let length = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let xml = String::from_utf8(bytes[16..16 + length].to_vec()).unwrap();
    let location = xml.split("location=\"attachment:").nth(1).unwrap();
    let mut numbers = location.split('"').next().unwrap().split(':');
    let position: usize = numbers.next().unwrap().parse().unwrap();
    let size: usize = numbers.next().unwrap().parse().unwrap();
    assert_eq!(position % 4096, 0, "blocks start on a 4096-byte boundary");
    (xml, &bytes[position..position + size])
}

#[test]
fn xisf_master_holds_the_fits_samples_and_keywords_and_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let source = header(&fixture(&[("OBJECT", "'M 33'")], &[])).unwrap();
    let cancel = AtomicBool::new(false);
    let metadata = fulgur_core::output::MasterMetadata {
        rejection: "winsorized",
        geometric_correction_frames: 3,
        external_geometry_scale: None,
        accepted_frames: 27,
        total_exposure_seconds: 270.,
        origin_x_px: -4,
        origin_y_px: 7,
        monochrome_frames: 0,
        calibrated_frames: 0,
        calibration_formula: None,
        astrometry: None,
    };
    let mut rgb = image();
    rgb.pixels[1] = [-10., 70_000., 3.];
    let mut mono = image();
    mono.demosaic_method = DemosaicMethod::None;
    for (name, picture, planes) in [("rgb", &rgb, 3), ("mono", &mono, 1)] {
        let fits = dir.path().join(format!("{name}.fits"));
        let xisf = dir.path().join(format!("{name}.xisf"));
        let a =
            fulgur_core::output::write_master_atomic(&fits, picture, &source, &metadata, &cancel)
                .unwrap();
        let b =
            fulgur_core::output::write_master_atomic(&xisf, picture, &source, &metadata, &cancel)
                .unwrap();
        assert_eq!(a.samples_clipped, b.samples_clipped);
        assert_eq!((a.output_min, a.output_max), (b.output_min, b.output_max));
        let fits_bytes = fs::read(&fits).unwrap();
        let end = fits_bytes
            .chunks_exact(80)
            .position(|c| c.starts_with(b"END "))
            .unwrap();
        let start = ((end + 1) * 80).div_ceil(2880) * 2880;
        let samples = 36 * planes * 4;
        let swapped: Vec<u8> = fits_bytes[start..start + samples]
            .chunks_exact(4)
            .flat_map(|c| [c[3], c[2], c[1], c[0]])
            .collect();
        let xisf_bytes = fs::read(&xisf).unwrap();
        let (xml, block) = xisf_parts(&xisf_bytes);
        assert_eq!(
            block,
            swapped.as_slice(),
            "{name}: same samples, little-endian"
        );
        assert!(xml.contains(&format!("geometry=\"6:6:{planes}\"")));
        assert!(xml.contains("sampleFormat=\"Float32\" bounds=\"0:1\""));
        assert!(xml.contains(if planes == 3 {
            "colorSpace=\"RGB\""
        } else {
            "colorSpace=\"Gray\""
        }));
        for keyword in [
            "name=\"NCOMBINE\" value=\"27\"",
            "name=\"OBJECT\" value=\"&apos;M 33&apos;\"",
            "name=\"CROPX\" value=\"-4\"",
            "name=\"NDISTCOR\" value=\"3\"",
            "name=\"SITELAT\"",
            "name=\"HISTORY\" value=\"\" comment=\"Weighted linear mean",
            "XISF:CreatorApplication",
        ] {
            assert!(xml.contains(keyword), "{name}: {keyword}");
        }
        for storage in [
            "\"BITPIX\"",
            "\"NAXIS\"",
            "\"ROWORDER\"",
            "\"END\"",
            "CRVAL1",
        ] {
            assert!(!xml.contains(storage), "{name}: {storage}");
        }
        let meta = fulgur_core::fits::read_metadata(&xisf).unwrap();
        assert_eq!((meta.width, meta.height, meta.planes), (6, 6, planes));
        assert_eq!(meta.combined_frames, Some(27));
        assert_eq!(meta.object.as_deref(), Some("M 33"));
        let from_xisf = fulgur_core::fits::read_reference_plane(&xisf, &cancel).unwrap();
        let from_fits = fulgur_core::fits::read_reference_plane(&fits, &cancel).unwrap();
        // The reference reader keeps float FITS in 0..1 and scales XISF to
        // source ADU through its bounds; the samples are the same.
        assert_eq!(from_xisf.pixels.len(), from_fits.pixels.len());
        for (x, f) in from_xisf.pixels.iter().zip(&from_fits.pixels) {
            assert!(
                (x - f * 65535.).abs() <= 1e-3 * x.abs().max(1.),
                "{name}: {x} vs {f}"
            );
        }
    }
}

#[test]
fn xisf_maps_follow_the_fits_maps_and_unknown_extensions_write_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    let map = RgbImage {
        width: 2,
        height: 1,
        demosaic_method: DemosaicMethod::Vng,
        pixels: vec![[0.5, 0.75, 0.]; 2],
    };
    let path = dir.path().join("master_coverage.xisf");
    fulgur_core::output::write_fraction_map_atomic(&path, &map, "COVERAGE", 32, &cancel).unwrap();
    let bytes = fs::read(&path).unwrap();
    let (xml, block) = xisf_parts(&bytes);
    assert_eq!(f32::from_le_bytes(block[..4].try_into().unwrap()), 0.5);
    assert_eq!(f32::from_le_bytes(block[8..12].try_into().unwrap()), 0.75);
    assert!(xml.contains("name=\"MAPTYPE\" value=\"&apos;COVERAGE&apos;\""));
    assert!(xml.contains("name=\"NINPUT\" value=\"32\""));
    let png = dir.path().join("master.png");
    assert!(
        fulgur_core::output::write_fraction_map_atomic(&png, &map, "COVERAGE", 32, &cancel)
            .is_err()
    );
    let source = header(&fixture(&[], &[])).unwrap();
    assert!(write_rgb_atomic(&png, &image(), &source, &cancel).is_err());
    assert!(!png.exists());
    assert!(!dir.path().join("master.png.tmp").exists());
}

#[test]
fn a_plate_solution_goes_into_fits_and_xisf_headers_and_replaces_the_no_wcs_note() {
    let dir = tempfile::tempdir().unwrap();
    let source = header(&fixture(&[], &[])).unwrap();
    let cancel = AtomicBool::new(false);
    let mut metadata = fulgur_core::output::MasterMetadata {
        rejection: "winsorized",
        geometric_correction_frames: 0,
        external_geometry_scale: None,
        accepted_frames: 3,
        total_exposure_seconds: 30.,
        origin_x_px: 0,
        origin_y_px: 0,
        monochrome_frames: 0,
        calibrated_frames: 0,
        calibration_formula: None,
        astrometry: None,
    };
    let plain = dir.path().join("plain.fits");
    fulgur_core::output::write_master_atomic(&plain, &image(), &source, &metadata, &cancel)
        .unwrap();
    let text = String::from_utf8_lossy(&fs::read(&plain).unwrap()[..5760]).into_owned();
    assert!(text.contains("No plate solution"));
    assert!(!text.contains("CTYPE1"));
    metadata.astrometry = Some(fulgur_core::output::Astrometry {
        cards: vec![
            (
                "CTYPE1".into(),
                "'RA---TAN-SIP'".into(),
                "gnomonic projection".into(),
            ),
            (
                "CRVAL1".into(),
                "2.346000000000000E1".into(),
                "[deg] RA of the tangent point".into(),
            ),
            (
                "A_2_0".into(),
                "-8.024516354650018E-8".into(),
                String::new(),
            ),
        ],
        history: vec!["WCS: Gaia DR3 solution of reference.fit".into()],
    });
    let fits = dir.path().join("solved.fits");
    fulgur_core::output::write_master_atomic(&fits, &image(), &source, &metadata, &cancel).unwrap();
    let text = String::from_utf8_lossy(&fs::read(&fits).unwrap()[..5760]).into_owned();
    assert!(!text.contains("No plate solution"));
    assert!(text.contains("CTYPE1  = 'RA---TAN-SIP' / gnomonic projection"));
    assert!(text.contains("CRVAL1  =  2.346000000000000E1 / [deg] RA of the tangent point"));
    assert!(text.contains("A_2_0   = -8.024516354650018E-8"));
    assert!(text.contains("HISTORY WCS: Gaia DR3 solution of reference.fit"));
    let xisf = dir.path().join("solved.xisf");
    fulgur_core::output::write_master_atomic(&xisf, &image(), &source, &metadata, &cancel).unwrap();
    let (xml, _) = xisf_parts(&fs::read(&xisf).unwrap());
    let xml = xml.to_owned();
    assert!(xml.contains(
        "name=\"CTYPE1\" value=\"&apos;RA---TAN-SIP&apos;\" comment=\"gnomonic projection\""
    ));
    assert!(xml.contains("name=\"CRVAL1\" value=\"2.346000000000000E1\""));
    assert!(xml.contains("name=\"A_2_0\" value=\"-8.024516354650018E-8\""));
}

#[test]
fn master_header_names_the_rejection_actually_applied_and_the_application_version() {
    let source = header(&fixture(&[], &[])).unwrap();
    let image = RgbImage {
        width: 3,
        height: 2,
        demosaic_method: DemosaicMethod::Vng,
        pixels: vec![[2000., 4000., 6000.]; 6],
    };
    for (key, card) in [
        ("winsorized", "'WINSOR-MAD'"),
        ("linear-fit", "'LINEAR-FIT'"),
        ("esd", "'ESD'"),
        ("median", "'MEDIAN'"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("master.fits");
        let metadata = fulgur_core::output::MasterMetadata {
            rejection: key,
            geometric_correction_frames: 0,
            external_geometry_scale: None,
            accepted_frames: 5,
            total_exposure_seconds: 50.,
            origin_x_px: 0,
            origin_y_px: 0,
            monochrome_frames: 0,
            calibrated_frames: 0,
            calibration_formula: None,
            astrometry: None,
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
        let cards: Vec<String> = bytes[..5760]
            .chunks(80)
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect();
        let find = |k: &str| {
            cards
                .iter()
                .find(|c| c.starts_with(k))
                .unwrap_or_else(|| panic!("{k} missing"))
        };
        assert!(find("REJECT").contains(card), "{key}: {}", find("REJECT"));
        assert!(
            find("CREATOR").contains(&format!("FulgurStack {}", env!("CARGO_PKG_VERSION"))),
            "{}",
            find("CREATOR")
        );
    }
}

#[test]
fn header_pointing_comes_from_the_wcs_centre_then_ra_dec_then_object_cards() {
    let dir = tempfile::tempdir().unwrap();
    let read = |name: &str, extra: &[(&str, &str)]| {
        let path = dir.path().join(name);
        let mut bytes = fixture(extra, &[]);
        bytes.extend(std::iter::repeat_n(0_u8, 72));
        fs::write(&path, bytes).unwrap();
        fulgur_core::fits::read_sky_position(&path)
    };
    let near = |got: Option<(f64, f64)>, want: (f64, f64)| {
        let (ra, dec) = got.expect("a position");
        assert!(
            (ra - want.0).abs() < 1e-3 && (dec - want.1).abs() < 1e-3,
            "{ra} {dec} vs {want:?}"
        );
    };
    // The WCS wins over RA and DEC: the centre of a 6 x 6 frame is pixel 3.5.
    let wcs = [
        ("CRVAL2", "28."),
        ("CRPIX1", "3.5"),
        ("CRPIX2", "3.5"),
        ("CD1_1", "-0.001"),
        ("CD1_2", "0."),
        ("CD2_1", "0."),
        ("CD2_2", "0.001"),
        ("RA", "10."),
        ("DEC", "10."),
    ];
    near(read("wcs.fit", &wcs), (123.45, 28.));
    // A reference pixel away from the centre moves the pointing by whole pixels.
    let shifted = [("CRPIX1", "-996.5"), ("CRPIX2", "3.5")];
    let mut cards = wcs.to_vec();
    cards.retain(|(k, _)| !shifted.iter().any(|(s, _)| s == k));
    cards.extend(shifted);
    let (ra, dec) = read("shifted.fit", &cards).unwrap();
    assert!(
        (ra - 123.45).abs() > 0.5 && (ra - 123.45).abs() < 1.5 && (dec - 28.).abs() < 0.1,
        "{ra} {dec}"
    );
    near(
        read("deg.fit", &[("RA", "64.5"), ("DEC", "27.8")]),
        (64.5, 27.8),
    );
    near(
        read(
            "sexagesimal.fit",
            &[("OBJCTRA", "'04 18 00'"), ("OBJCTDEC", "'-27 30 00'")],
        ),
        (64.5, -27.5),
    );
    assert_eq!(read("none.fit", &[]), None);
    assert_eq!(read("bad.fit", &[("RA", "64."), ("DEC", "123.")]), None);
}

/// Exposure in milliseconds (ADR-166): the Vaonis Stellina writes
/// `EXPOSURE = 10000 / [ms] Total Exposure Time` and no EXPTIME.
#[test]
fn exposure_in_milliseconds_is_converted_and_asks_the_owner() {
    let stellina = [("INSTRUME", "'stellina-43ab43'")];
    // The comment declares the unit.
    let with_comment = fixture(
        &[
            stellina[0],
            ("EXPOSURE", "10000 / [ms] Total Exposure Time"),
        ],
        &["EXPTIME"],
    );
    let info = header(&with_comment).unwrap().info;
    assert_eq!(info.exposure_seconds, 10.0);
    assert_eq!(info.derived.len(), 1);
    assert_eq!(info.derived[0].field, "exposure_seconds");
    assert_eq!(info.derived[0].value, "10 s");
    assert!(info.derived[0].source.contains("10000 ms"));
    assert!(info.derived[0].source.contains("card's comment"));
    assert!(info.needs_confirmation());
    // Without a comment the camera catalogue knows the Stellina.
    let bare = fixture(&[stellina[0], ("EXPOSURE", "10000")], &["EXPTIME"]);
    let info = header(&bare).unwrap().info;
    assert_eq!(info.exposure_seconds, 10.0);
    assert!(info.derived[0].source.contains("Vaonis Stellina"));
    // EXPTIME in milliseconds by its comment, from any camera.
    let exptime = with_exptime("500 / [ms] exposure", &[]);
    let info = header(&exptime).unwrap().info;
    assert_eq!(info.exposure_seconds, 0.5);
    assert!(info.needs_confirmation());
    // A unit the comment declares wins over the camera's habit.
    let declared = with_exptime("10 / [s] exposure", &stellina);
    let info = header(&declared).unwrap().info;
    assert_eq!(info.exposure_seconds, 10.0);
    assert!(info.derived.is_empty());
}

#[test]
fn an_exposure_over_an_hour_without_a_unit_asks_but_is_not_changed() {
    let info = header(&with_exptime("10000", &[])).unwrap().info;
    assert_eq!(info.exposure_seconds, 10000.0);
    assert_eq!(info.derived.len(), 1);
    assert!(info.derived[0].source.contains("unit unknown"));
    assert!(info.needs_confirmation());
    // The owner's own exposure replaces it, as for any derived value.
    let mut corrected = info.clone();
    corrected.apply_exposure(Some(10.0));
    assert_eq!(corrected.exposure_seconds, 10.0);
    assert!(!corrected.needs_confirmation());
    // Ordinary long exposures and plain seconds need nothing.
    for text in ["10.", "300", "1800 / [s] exposure", "3600"] {
        let info = header(&with_exptime(text, &[])).unwrap().info;
        assert!(info.derived.is_empty(), "{text}");
    }
}

#[test]
fn planning_metadata_shows_the_converted_exposure() {
    use fulgur_core::fits::{read_light_metadata, read_metadata};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("img-0001.fits");
    fs::write(
        &path,
        fixture(
            &[
                ("INSTRUME", "'stellina-43ab43'"),
                ("EXPOSURE", "10000 / [ms] Total Exposure Time"),
            ],
            &["EXPTIME"],
        ),
    )
    .unwrap();
    assert_eq!(read_metadata(&path).unwrap().exposure_seconds, Some(10.0));
    let meta = read_light_metadata(&path).unwrap();
    assert_eq!(meta.exposure_seconds, Some(10.0));
    assert_eq!(meta.derived[0].field, "exposure_seconds");
    assert_eq!(meta.derived, header_from(&path).derived);
}

fn header_from(path: &std::path::Path) -> fulgur_core::fits::FrameInfo {
    fulgur_core::fits::read_header(path).unwrap().info
}

/// The fixture with its EXPTIME card replaced (value and comment) and extra cards.
fn with_exptime(text: &str, extra: &[(&str, &str)]) -> Vec<u8> {
    let mut bytes = fixture(extra, &[]);
    let index = bytes
        .chunks_exact(80)
        .position(|card| card.starts_with(b"EXPTIME "))
        .unwrap();
    bytes[index * 80..(index + 1) * 80]
        .copy_from_slice(format!("{:<80}", format!("EXPTIME = {text}")).as_bytes());
    bytes
}
