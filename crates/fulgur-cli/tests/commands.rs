use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn source(path: &Path) {
    let mut bytes = Vec::new();
    for (key, value) in [
        ("SIMPLE", "T"),
        ("BITPIX", "16"),
        ("NAXIS", "2"),
        ("NAXIS1", "16"),
        ("NAXIS2", "16"),
        ("BZERO", "32768"),
        ("BAYERPAT", "'GRBG'"),
        ("EXPTIME", "10"),
        ("BIAS", "2064"),
        ("SITELAT", "12.3456789"),
        ("SITELONG", "45.6789123"),
    ] {
        bytes.extend(format!("{:<80}", format!("{key:<8}= {value:>20}")).as_bytes());
    }
    bytes.extend(format!("{:<80}", "END").as_bytes());
    bytes.resize(2880, b' ');
    for _ in 0..256 {
        bytes.extend((-31768_i16).to_be_bytes());
    }
    bytes.resize(5760, 0);
    fs::write(path, bytes).unwrap();
}

fn invoke(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fulgurstack"))
        .args(["--threads", "2"])
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn invalid_tile_dimensions_fail_before_processing_or_publishing() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.fit");
    source(&input);
    let original = fs::read(&input).unwrap();
    let output = dir.path().join("master.fits");
    let report = dir.path().join("report.json");
    for (width, height) in [("0", "128"), ("1025", "128"), ("256", "0"), ("256", "513")] {
        let result = invoke(&[
            "stack",
            "--input",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--report",
            report.to_str().unwrap(),
            "--tile-width",
            width,
            "--tile-height",
            height,
        ]);
        assert_eq!(result.status.code(), Some(2));
        assert!(!output.exists() && !report.exists());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    assert_eq!(fs::read(input).unwrap(), original);
}

#[cfg(not(feature = "gpu"))]
#[test]
fn gpu_request_without_build_feature_fails_without_silent_fallback_or_outputs() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.fit");
    source(&input);
    let output = dir.path().join("gpu.fits");
    let report = dir.path().join("gpu.json");
    let original = fs::read(&input).unwrap();
    let result = invoke(&[
        "stack",
        "--engine",
        "gpu",
        "--input",
        input.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
        "--report",
        report.to_str().unwrap(),
    ]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("--features gpu"));
    assert!(!output.exists() && !report.exists());
    assert_eq!(fs::read(input).unwrap(), original);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

fn quality_source(path: &Path, elongated: bool) {
    let size = 288;
    let mut bytes = Vec::new();
    for (key, value) in [
        ("SIMPLE", "T"),
        ("BITPIX", "16"),
        ("NAXIS", "2"),
        ("NAXIS1", "288"),
        ("NAXIS2", "288"),
        ("BZERO", "32768"),
        ("BAYERPAT", "'GRBG'"),
        ("EXPTIME", "10"),
        ("OBJECT", "'M33'"),
        ("DATE-OBS", "'2026-10-06T22:00:00.000000'"),
        ("FILTER", "'IRCUT'"),
        ("SITELAT", "12.3456789"),
        ("SITELONG", "45.6789123"),
    ] {
        bytes.extend(format!("{:<80}", format!("{key:<8}= {value:>20}")).as_bytes());
    }
    bytes.extend(format!("{:<80}", "END").as_bytes());
    bytes.resize(2880, b' ');
    for y in 0..size {
        for x in 0..size {
            let mut value = 2000.0 + ((x * 71 + y * 23 + x * y * 11) % 29) as f64 - 14.0;
            for cy in [40.3, 88.3, 136.3, 184.3, 232.3] {
                for cx in [40.2, 88.2, 136.2, 184.2, 232.2] {
                    let sx = if elongated { 2.8_f64 } else { 1.4_f64 };
                    value += 10000.0
                        * (-0.5
                            * (((x as f64 - cx) / sx).powi(2) + ((y as f64 - cy) / 1.4).powi(2)))
                        .exp();
                }
            }
            bytes.extend(((value.round() as i32 - 32768) as i16).to_be_bytes());
        }
    }
    bytes.resize(bytes.len().div_ceil(2880) * 2880, 0);
    fs::write(path, bytes).unwrap();
}

#[test]
fn inspect_measures_sources_reports_tracking_and_bad_inputs_without_intermediates_or_gps() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input");
    fs::create_dir(&input).unwrap();
    quality_source(&input.join("round_999s.fit"), false);
    quality_source(&input.join("elongated.fit"), true);
    fs::write(input.join("invalid.fit"), b"bad FITS").unwrap();
    let paths: Vec<_> = fs::read_dir(&input)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    let hashes: Vec<_> = paths
        .iter()
        .map(|p| Sha256::digest(fs::read(p).unwrap()))
        .collect();
    let output = dir.path().join("quality.json");
    let csv = dir.path().join("quality.csv");
    let result = invoke(&[
        "inspect",
        "--input",
        input.to_str().unwrap(),
        "--report",
        output.to_str().unwrap(),
        "--csv",
        csv.to_str().unwrap(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let text = fs::read_to_string(&output).unwrap();
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["frame_count"], 3);
    assert_eq!(report["accepted_count"], 1);
    assert_eq!(report["rejected_count"], 2);
    assert_eq!(report["intermediate_images_written"], 0);
    let rows = report["frames"].as_array().unwrap();
    let round = rows
        .iter()
        .find(|f| f["file_name"] == "round_999s.fit")
        .unwrap();
    assert_eq!(round["frame"]["exposure_seconds"], 10.0);
    assert!(round["star_count"].as_u64().unwrap() >= 20);
    assert!(round["measured_star_count"].as_u64().unwrap() >= 20);
    assert!((1.4..1.9).contains(&round["hfr_px"].as_f64().unwrap()));
    assert!(round["eccentricity"].as_f64().unwrap() < 0.25);
    assert_eq!(round["provisional_weight"], 1.0);
    let elongated = rows
        .iter()
        .find(|f| f["file_name"] == "elongated.fit")
        .unwrap();
    assert!(
        elongated["rejection_reasons"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("excessive_eccentricity"))
    );
    let invalid = rows
        .iter()
        .find(|f| f["file_name"] == "invalid.fit")
        .unwrap();
    assert!(
        invalid["rejection_reasons"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("invalid_or_unsupported_fits"))
    );
    let table = fs::read_to_string(csv).unwrap();
    assert!(table.starts_with('\u{feff}'));
    assert_eq!(table.lines().count(), 4);
    assert_eq!(table.lines().next().unwrap().split(';').count(), 22);
    for private in ["SITELAT", "SITELONG", "12.3456789", "45.6789123"] {
        assert!(!text.contains(private) && !table.contains(private));
        assert!(!String::from_utf8_lossy(&result.stdout).contains(private));
    }
    for (path, hash) in paths.iter().zip(hashes) {
        assert_eq!(Sha256::digest(fs::read(path).unwrap()), hash);
    }
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
    assert_eq!(fs::read_dir(input).unwrap().count(), 3);
}

#[test]
fn inspect_checks_all_report_destinations_before_processing() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.fit");
    source(&input);
    let json = dir.path().join("new.json");
    let csv = dir.path().join("existing.csv");
    fs::write(&csv, b"preserve").unwrap();
    let output = invoke(&[
        "inspect",
        "--input",
        input.to_str().unwrap(),
        "--report",
        json.to_str().unwrap(),
        "--csv",
        csv.to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    assert!(!json.exists());
    assert_eq!(fs::read(csv).unwrap(), b"preserve");
    let output = invoke(&[
        "inspect",
        "--input",
        input.to_str().unwrap(),
        "--report",
        json.to_str().unwrap(),
        "--max-eccentricity",
        "NaN",
    ]);
    assert!(!output.status.success());
    assert!(!json.exists());
}

#[test]
fn export_report_is_private_linear_and_creates_only_requested_files() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.fit");
    let output = dir.path().join("rgb.fits");
    let report = dir.path().join("run.json");
    source(&input);
    let digest = Sha256::digest(fs::read(&input).unwrap());
    let result = invoke(&[
        "debayer-single",
        "--input",
        input.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
        "--report",
        report.to_str().unwrap(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(digest, Sha256::digest(fs::read(&input).unwrap()));
    let text = fs::read_to_string(&report).unwrap();
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["bias_subtracted"], false);
    assert_eq!(report["gps_copied_to_fits"], true);
    assert_eq!(report["output_bitpix"], -32);
    assert_eq!(report["intermediate_images_written"], 0);
    assert_eq!(report["schema_version"], 3);
    assert_eq!(report["output_adu_divisor"], 65535.0);
    assert_eq!(report["output_offset"], 0.0);
    assert_eq!(report["demosaic_method"], "vng");
    assert_eq!(report["samples_clipped"], 0);
    assert_eq!(report["samples_below_zero"], 0);
    assert_eq!(report["samples_above_one"], 0);
    assert_eq!(report["samples_limited_low"], 0);
    assert_eq!(report["samples_limited_high"], 0);
    assert_eq!(report["output_range_policy"], "sensor saturation to 0..1");
    assert_eq!(report["pixinsight_open_method"], "File > Open");
    for private in ["SITELAT", "SITELONG", "12.3456789", "45.6789123"] {
        assert!(!text.contains(private));
        assert!(!String::from_utf8_lossy(&result.stdout).contains(private));
        assert!(!String::from_utf8_lossy(&result.stderr).contains(private));
    }
    let rgb = fs::read(output).unwrap();
    assert_eq!(
        f32::from_be_bytes(rgb[2880..2884].try_into().unwrap()),
        1000.0 / 65535.0
    );
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
}

#[test]
fn using_input_as_output_fails_without_modifying_source() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.fit");
    source(&input);
    let before = fs::read(&input).unwrap();
    let result = invoke(&[
        "debayer-single",
        "--input",
        input.to_str().unwrap(),
        "--output",
        input.to_str().unwrap(),
    ]);
    assert!(!result.status.success());
    assert_eq!(before, fs::read(&input).unwrap());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn invalid_report_path_fails_before_publishing_rgb() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.fit");
    let output = dir.path().join("rgb.fits");
    source(&input);
    let result = invoke(&[
        "debayer-single",
        "--input",
        input.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
        "--report",
        input.to_str().unwrap(),
    ]);
    assert!(!result.status.success());
    assert!(!output.exists());
}

#[test]
fn existing_report_fails_before_publishing_rgb() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.fit");
    let output = dir.path().join("rgb.fits");
    let report = dir.path().join("run.json");
    source(&input);
    fs::write(&report, b"preserve this report").unwrap();
    let result = invoke(&[
        "debayer-single",
        "--input",
        input.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
        "--report",
        report.to_str().unwrap(),
    ]);
    assert!(!result.status.success());
    assert!(!output.exists());
    assert_eq!(fs::read(&report).unwrap(), b"preserve this report");
}

#[test]
fn survey_reads_full_files_recursively_and_reports_no_gps() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("night2")).unwrap();
    source(&dir.path().join("first.fit"));
    source(&dir.path().join("night2/second.FITS"));
    fs::write(dir.path().join("ignore.txt"), b"not an image").unwrap();
    let report = dir.path().join("survey.json");
    let result = invoke(&[
        "survey",
        "--input",
        dir.path().to_str().unwrap(),
        "--report",
        report.to_str().unwrap(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let text = fs::read_to_string(report).unwrap();
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["frame_count"], 2);
    assert_eq!(report["total_bytes_read"], 11520);
    assert_eq!(report["total_exposure_seconds"], 20.0);
    assert!(!text.contains("SITELAT"));
    assert!(!text.contains("12.3456789"));
}

#[test]
fn invalid_input_and_empty_survey_have_actionable_failures() {
    let dir = tempfile::tempdir().unwrap();
    let result = invoke(&["survey", "--input", dir.path().to_str().unwrap()]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("No FITS input files"));
    let invalid = dir.path().join("invalid.fit");
    fs::write(&invalid, b"not FITS").unwrap();
    let output = dir.path().join("output.fits");
    let result = invoke(&[
        "debayer-single",
        "--input",
        invalid.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ]);
    assert!(!result.status.success());
    assert!(!output.exists());
}

#[test]
fn diagnostic_mhc_selection_is_explicit_and_unknown_methods_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.fit");
    let output = dir.path().join("rgb.fits");
    let report = dir.path().join("run.json");
    source(&input);
    let args = [
        "debayer-single",
        "--input",
        input.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
        "--report",
        report.to_str().unwrap(),
        "--demosaic",
    ];
    let mut invalid_args = args.to_vec();
    invalid_args.push("unknown");
    assert!(!invoke(&invalid_args).status.success());
    assert!(!output.exists());
    let mut valid_args = args.to_vec();
    valid_args.push("mhc");
    let result = invoke(&valid_args);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(report).unwrap()).unwrap();
    assert_eq!(json["demosaic_method"], "mhc");
    let data = fs::read(output).unwrap();
    let method_card = data[..2880]
        .chunks_exact(80)
        .find(|card| card.starts_with(b"DEBAYER "))
        .unwrap();
    assert!(String::from_utf8_lossy(method_card).contains("'MHC'"));
}

#[test]
fn registration_cli_preserves_sources_redacts_gps_and_publishes_only_reports() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input");
    fs::create_dir(&input).unwrap();
    let fit = input.join("round.fit");
    quality_source(&fit, false);
    let before = Sha256::digest(fs::read(&fit).unwrap());
    let json = dir.path().join("plan.json");
    let csv = dir.path().join("plan.csv");
    let args = [
        "register",
        "--input",
        input.to_str().unwrap(),
        "--report",
        json.to_str().unwrap(),
        "--csv",
        csv.to_str().unwrap(),
    ];
    let output = invoke(&args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = fs::read_to_string(&json).unwrap();
    let r: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(r["registration_success_count"], 1);
    assert_eq!(r["accepted_count"], 1);
    assert_eq!(r["intermediate_images_written"], 0);
    assert_eq!(r["frames"][0]["photometric_scale"], 1.0);
    for secret in ["SITELAT", "SITELONG", "12.3456789", "45.6789123"] {
        assert!(!text.contains(secret));
        assert!(!String::from_utf8_lossy(&output.stdout).contains(secret));
    }
    assert_eq!(before, Sha256::digest(fs::read(&fit).unwrap()));
    assert_eq!(fs::read_dir(&input).unwrap().count(), 1);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
    let preserved = fs::read(&json).unwrap();
    assert!(!invoke(&args).status.success());
    assert_eq!(preserved, fs::read(&json).unwrap());
    let nojson = dir.path().join("unpublished.json");
    let bad = [
        "register",
        "--input",
        input.to_str().unwrap(),
        "--report",
        nojson.to_str().unwrap(),
        "--csv",
        fit.to_str().unwrap(),
    ];
    assert!(!invoke(&bad).status.success());
    assert!(!nojson.exists());
    let invalid = [
        "register",
        "--input",
        input.to_str().unwrap(),
        "--report",
        nojson.to_str().unwrap(),
        "--min-coverage",
        "NaN",
    ];
    assert!(!invoke(&invalid).status.success());
    assert!(!nojson.exists());
}

#[test]
fn registration_diagnostics_is_opt_in_and_keeps_excluded_sources_out_of_selection() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input");
    fs::create_dir(&input).unwrap();
    let selected = input.join("round.fit");
    let excluded = input.join("elongated.fit");
    quality_source(&selected, false);
    quality_source(&excluded, true);
    let original = Sha256::digest(fs::read(&excluded).unwrap());
    for diagnostics in [false, true] {
        let json = dir.path().join(if diagnostics {
            "diagnostics.json"
        } else {
            "default.json"
        });
        let mut args = vec![
            "register",
            "--input",
            input.to_str().unwrap(),
            "--report",
            json.to_str().unwrap(),
        ];
        if diagnostics {
            args.push("--diagnose-excluded");
        }
        let output = invoke(&args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let r: serde_json::Value = serde_json::from_slice(&fs::read(json).unwrap()).unwrap();
        assert_eq!(r["frame_count"], 2);
        assert_eq!(r["quality_accepted_count"], 1);
        assert_eq!(r["accepted_count"], 1);
        assert_eq!(r["diagnose_excluded"], diagnostics);
        assert_eq!(
            r["registration_attempted_count"],
            if diagnostics { 2 } else { 1 }
        );
        assert_eq!(
            r["registration_skipped_count"],
            if diagnostics { 0 } else { 1 }
        );
        let row = r["frames"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["file_name"] == "elongated.fit")
            .unwrap();
        assert_eq!(row["registration_attempted"], diagnostics);
        assert_eq!(row["accepted"], false);
        assert_eq!(row["final_weight"], 0.);
        assert!(row["footprint"].as_array().unwrap().is_empty());
    }
    assert_eq!(original, Sha256::digest(fs::read(excluded).unwrap()));
}

#[test]
fn stack_cli_integrates_mixed_exposures_without_bias_or_intermediates() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input");
    fs::create_dir(&input).unwrap();
    let a = input.join("a.fit");
    let b = input.join("b.fit");
    quality_source(&a, false);
    let mut bytes = fs::read(&a).unwrap();
    for card in bytes[..2880].chunks_exact_mut(80) {
        if card.starts_with(b"EXPTIME ") {
            card.copy_from_slice(format!("{:<80}", "EXPTIME =                   20").as_bytes());
        }
    }
    for pair in bytes[2880..2880 + 288 * 288 * 2].chunks_exact_mut(2) {
        let v = i16::from_be_bytes([pair[0], pair[1]]) as i32 + 32768;
        let v = 2000 + 2 * (v - 2000);
        pair.copy_from_slice(&((v - 32768) as i16).to_be_bytes());
    }
    fs::write(&b, bytes).unwrap();
    let hashes = [
        Sha256::digest(fs::read(&a).unwrap()),
        Sha256::digest(fs::read(&b).unwrap()),
    ];
    let output = dir.path().join("master.fits");
    let report = dir.path().join("stack.json");
    let args = [
        "stack",
        "--input",
        a.to_str().unwrap(),
        "--input",
        b.to_str().unwrap(),
        "--input",
        a.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
        "--report",
        report.to_str().unwrap(),
        "--tile-width",
        "512",
        "--tile-height",
        "512",
    ];
    let result = invoke(&args);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let text = fs::read_to_string(&report).unwrap();
    let r: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(r["registration"]["accepted_count"], 2);
    assert_eq!(r["registration"]["accepted_exposure_seconds"], 30.);
    assert!(r["integration"]["zero_coverage_pixels"].as_u64().unwrap() > 0);
    assert_eq!(r["width"], 288);
    assert_eq!(r["height"], 288);
    assert_eq!(r["config"]["tile_width"], 512);
    assert_eq!(r["config"]["tile_height"], 512);
    assert_eq!(r["config"]["tile_mode"], "manual");
    assert_eq!(r["integration"]["memory_plan"]["mode"], "manual");
    assert_eq!(r["integration"]["tile_width"], 288);
    assert_eq!(r["integration"]["tile_height"], 288);
    assert_eq!(r["origin_x_px"], 0);
    assert_eq!(r["origin_y_px"], 0);
    assert_eq!(r["map_denominator_frames"], 2);
    assert_eq!(r["canvas_mode"], "reference");
    assert_eq!(r["bias_subtracted"], false);
    assert_eq!(r["intermediate_images_written"], 0);
    for frame in r["registration"]["frames"].as_array().unwrap() {
        assert!((frame["exposure_corrected_transparency"].as_f64().unwrap() - 1.).abs() < 0.01);
    }
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 5);
    assert_eq!(fs::read_dir(&input).unwrap().count(), 2);
    assert_eq!(hashes[0], Sha256::digest(fs::read(&a).unwrap()));
    assert_eq!(hashes[1], Sha256::digest(fs::read(&b).unwrap()));
    for secret in ["SITELAT", "SITELONG", "12.3456789", "45.6789123"] {
        assert!(!text.contains(secret));
    }
    let previous = fs::read(&output).unwrap();
    assert!(!invoke(&args).status.success());
    assert_eq!(previous, fs::read(&output).unwrap());
    let noout = dir.path().join("unpublished.fits");
    let invalid = [
        "stack",
        "--input",
        input.to_str().unwrap(),
        "--output",
        noout.to_str().unwrap(),
        "--report",
        report.to_str().unwrap(),
    ];
    assert!(!invoke(&invalid).status.success());
    assert!(!noout.exists());
}

/// The quality fixture with its BAYERPAT card blanked: a sensor whose headers
/// do not say whether it carries a colour mask.
fn mono_source(path: &Path, shift: (usize, usize)) {
    quality_source(path, false);
    let mut bytes = fs::read(path).unwrap();
    for card in bytes[..2880].chunks_exact_mut(80) {
        if card.starts_with(b"BAYERPAT") {
            card.copy_from_slice(format!("{:<80}", "COMMENT monochrome test frame").as_bytes());
        }
    }
    // A small translation keeps registration non-trivial.
    let original = bytes[2880..2880 + 288 * 288 * 2].to_vec();
    for y in 0..288_usize {
        for x in 0..288_usize {
            let (sx, sy) = (x.saturating_sub(shift.0), y.saturating_sub(shift.1));
            let source = (sy * 288 + sx) * 2;
            let target = 2880 + (y * 288 + x) * 2;
            bytes[target..target + 2].copy_from_slice(&original[source..source + 2]);
        }
    }
    fs::write(path, bytes).unwrap();
}

fn fits_card(path: &Path, key: &str) -> Option<String> {
    let bytes = fs::read(path).unwrap();
    bytes[..2880]
        .chunks_exact(80)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .find(|c| c.starts_with(&format!("{key:<8}=")))
        .map(|c| {
            c[10..]
                .split('/')
                .next()
                .unwrap()
                .trim()
                .trim_matches('\'')
                .trim()
                .to_owned()
        })
}

#[test]
fn lights_without_bayer_pattern_stack_as_mono_and_skip_demosaicing() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("mono");
    fs::create_dir(&input).unwrap();
    for (i, shift) in [(0, 0), (2, 1), (1, 3)].into_iter().enumerate() {
        mono_source(&input.join(format!("l{i}.fit")), shift);
    }
    let hashes: Vec<_> = (0..3)
        .map(|i| Sha256::digest(fs::read(input.join(format!("l{i}.fit"))).unwrap()))
        .collect();
    let output = dir.path().join("mono_master.fits");
    let report = dir.path().join("mono.json");
    let base = [
        "stack",
        "--input",
        input.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
        "--report",
        report.to_str().unwrap(),
    ];
    // No BAYERPAT: processed as mono without any declaration.
    let result = invoke(&base);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(fits_card(&output, "NAXIS").as_deref(), Some("2"));
    assert_eq!(fits_card(&output, "NAXIS3"), None);
    assert_eq!(fits_card(&output, "DEBAYER").as_deref(), Some("NONE"));
    let size = fs::metadata(&output).unwrap().len() as usize;
    assert_eq!(size, 2880 * (size / 2880), "FITS blocks");
    assert!(
        size < 2880 * 4 + 288 * 288 * 4 + 2880,
        "one plane only: {size}"
    );
    for map in [
        "mono_master_coverage.fits",
        "mono_master_high_rejection.fits",
    ] {
        assert_eq!(
            fits_card(&dir.path().join(map), "NAXIS").as_deref(),
            Some("2")
        );
    }
    let r: serde_json::Value = serde_json::from_str(&fs::read_to_string(&report).unwrap()).unwrap();
    assert_eq!(r["registration"]["accepted_count"], 3);
    for frame in r["registration"]["inspection"]["frames"]
        .as_array()
        .unwrap()
    {
        assert_eq!(frame["frame"]["mosaic"], "mono");
        assert_eq!(frame["frame"]["mosaic_source"], "no_bayer_header");
        assert!(frame["frame"]["bayer_pattern"].is_null());
    }
    assert_eq!(
        r["registration"]["inspection"]["light_sets"][0]["layout"],
        "auto"
    );
    // An explicit --sensor mono gives the same master.
    let declared = dir.path().join("declared.fits");
    let result = invoke(&[
        "stack",
        "--input",
        input.to_str().unwrap(),
        "--sensor",
        "mono",
        "--output",
        declared.to_str().unwrap(),
        "--report",
        dir.path().join("declared.json").to_str().unwrap(),
    ]);
    assert!(result.status.success());
    assert_eq!(fs::read(&declared).unwrap(), fs::read(&output).unwrap());
    for (i, hash) in hashes.iter().enumerate() {
        assert_eq!(
            *hash,
            Sha256::digest(fs::read(input.join(format!("l{i}.fit"))).unwrap())
        );
    }
}

#[test]
fn stack_manual_reference_and_map_destinations_are_validated_before_publication() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input");
    fs::create_dir(&input).unwrap();
    quality_source(&input.join("a.fit"), false);
    quality_source(&input.join("b.fit"), false);
    quality_source(&input.join("bad.fit"), true);
    let output = dir.path().join("manual.fits");
    let report = dir.path().join("manual.json");
    let args = [
        "stack",
        "--input",
        input.to_str().unwrap(),
        "--reference",
        "b.fit",
        "--output",
        output.to_str().unwrap(),
        "--report",
        report.to_str().unwrap(),
    ];
    let result = invoke(&args);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let r: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert_eq!(r["registration"]["reference_anchor"], "b.fit");
    assert_eq!(
        r["registration"]["reference_selection"],
        "manual_quality_accepted_input"
    );
    assert_eq!(r["width"], 288);
    let blocked = dir.path().join("blocked.fits");
    let nojson = dir.path().join("blocked.json");
    fs::write(dir.path().join("blocked_coverage.fits"), b"preserve").unwrap();
    let conflict = [
        "stack",
        "--input",
        input.to_str().unwrap(),
        "--output",
        blocked.to_str().unwrap(),
        "--report",
        nojson.to_str().unwrap(),
    ];
    assert!(!invoke(&conflict).status.success());
    assert!(!blocked.exists());
    assert!(!nojson.exists());
    let out = dir.path().join("new.fits");
    let bad = [
        "stack",
        "--input",
        input.to_str().unwrap(),
        "--reference",
        "bad.fit",
        "--output",
        out.to_str().unwrap(),
        "--report",
        nojson.to_str().unwrap(),
    ];
    assert!(!invoke(&bad).status.success());
    assert!(!out.exists());
    let alias = [
        "stack",
        "--input",
        input.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
        "--coverage-output",
        out.to_str().unwrap(),
        "--report",
        nojson.to_str().unwrap(),
    ];
    assert!(!invoke(&alias).status.success());
    assert!(!out.exists());
}

#[test]
fn external_scaled_rgb_reference_is_geometry_only_and_preserves_its_canvas() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input");
    fs::create_dir(&input).unwrap();
    let source_path = input.join("a.fit");
    quality_source(&source_path, false);
    let centers: Vec<_> = (0..32)
        .map(|i| {
            (
                24.2 + ((i * 37 + i * i * 3) % 238) as f64,
                24.3 + ((i * 73 + i * i * 7) % 238) as f64,
            )
        })
        .collect();
    let signal = |x: f64, y: f64| {
        centers
            .iter()
            .map(|&(cx, cy)| {
                (-((x - cx).powi(2) + (y - cy).powi(2)) / (2. * 1.4_f64.powi(2))).exp()
            })
            .sum::<f64>()
    };
    let mut original = fs::read(&source_path).unwrap();
    for y in 0..288 {
        for x in 0..288 {
            let v = (2000. + ((x * 13 + y * 17) % 23) as f64 - 11.
                + 10000. * signal(x as f64, y as f64))
            .round() as i32;
            let i = 2880 + (y * 288 + x) * 2;
            original[i..i + 2].copy_from_slice(&((v - 32768) as i16).to_be_bytes());
        }
    }
    fs::write(&source_path, &original).unwrap();
    fs::write(input.join("b.fit"), &original).unwrap();
    let template = dir.path().join("template.fits");
    let mut bytes = Vec::new();
    for (k, v) in [
        ("SIMPLE", "T"),
        ("BITPIX", "-32"),
        ("NAXIS", "3"),
        ("NAXIS1", "576"),
        ("NAXIS2", "512"),
        ("NAXIS3", "3"),
        ("ROWORDER", "'TOP-DOWN'"),
        // Geometry only: another filter than the IRCUT lights is irrelevant.
        ("OBJECT", "'M33'"),
        ("FILTER", "'LP'"),
    ] {
        bytes.extend(format!("{:<80}", format!("{k:<8}= {v:>20}")).as_bytes());
    }
    bytes.extend(format!("{:<80}", "END").as_bytes());
    bytes.resize(2880, b' ');
    for _ in 0..3 {
        for y in 0..512 {
            for x in 0..576 {
                let v = (0.1 + 0.5 * signal((x as f64 - 0.5) / 2., (y as f64 + 39.5) / 2.)) as f32;
                bytes.extend(v.to_be_bytes());
            }
        }
    }
    bytes.resize(bytes.len().div_ceil(2880) * 2880, 0);
    fs::write(&template, &bytes).unwrap();
    let output = dir.path().join("master.fits");
    let report = dir.path().join("stack.json");
    let result = invoke(&[
        "stack",
        "--input",
        input.to_str().unwrap(),
        "--reference",
        template.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
        "--report",
        report.to_str().unwrap(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let text = fs::read_to_string(&report).unwrap();
    let r: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(r["width"], 576);
    assert_eq!(r["height"], 512);
    assert_eq!(r["origin_x_px"], 0);
    assert_eq!(r["origin_y_px"], 0);
    assert_eq!(r["registration"]["accepted_count"], 2);
    assert_eq!(r["map_denominator_frames"], 2);
    assert_eq!(r["registration"]["accepted_exposure_seconds"], 20.);
    assert_eq!(
        r["registration"]["external_reference"]["geometry_only"],
        true
    );
    for f in r["registration"]["frames"].as_array().unwrap() {
        assert!((f["transform"]["scale_factor"].as_f64().unwrap() - 2.).abs() < 0.01);
        assert!((f["photometric_scale"].as_f64().unwrap() - 1.).abs() < 1e-9);
    }
    let data = fs::read(output).unwrap();
    let header = String::from_utf8_lossy(&data[..5760.min(data.len())]);
    assert!(header.contains("GEOMSCL"));
    assert!(header.contains("SIM(2)"));
    assert!(!header.contains("XPIXSZ"));
    assert!(
        header.contains("IRCUT") && !header.contains("'LP"),
        "filter comes from the lights"
    );
    for secret in ["SITELAT", "SITELONG", "12.3456789", "45.6789123"] {
        assert!(!text.contains(secret));
    }
    assert_eq!(fs::read(&template).unwrap(), bytes);
    assert_eq!(fs::read(source_path).unwrap(), original);
    assert_eq!(fs::read(input.join("b.fit")).unwrap(), original);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 6);
    // Another target is still a wrong reference file.
    let other = dir.path().join("other_target.fits");
    let mut wrong = bytes.clone();
    let at = wrong.windows(5).position(|w| w == b"'M33'").unwrap();
    wrong[at..at + 5].copy_from_slice(b"'M31'");
    fs::write(&other, wrong).unwrap();
    let refused_output = dir.path().join("refused.fits");
    let refused = invoke(&[
        "stack",
        "--input",
        input.to_str().unwrap(),
        "--reference",
        other.to_str().unwrap(),
        "--output",
        refused_output.to_str().unwrap(),
        "--report",
        dir.path().join("refused.json").to_str().unwrap(),
    ]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("different target"));
    assert!(!refused_output.exists());
}

#[test]
fn tile_selection_cli_supports_auto_manual_and_rejects_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.fit");
    quality_source(&input, false);
    let original = Sha256::digest(fs::read(&input).unwrap());
    let mut products = Vec::new();
    for (name, options) in [
        ("auto", vec![]),
        (
            "manual",
            vec!["--tile-size", "128", "--fits-cache-mib", "1024"],
        ),
        ("uncached", vec!["--fits-cache-mib", "0"]),
    ] {
        let output = dir.path().join(format!("{name}.fits"));
        let report = dir.path().join(format!("{name}.json"));
        let mut args = vec![
            "stack",
            "--input",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--report",
            report.to_str().unwrap(),
        ];
        args.extend(options);
        let result = invoke(&args);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let r: serde_json::Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
        let p = &r["integration"]["memory_plan"];
        let c = &r["integration"]["fits_cache"];
        assert!(c["capacity_accounted_bytes"].as_u64().unwrap() <= 1024 * 1024 * 1024);
        assert!(
            c["sum_source_peak_accounted_bytes"].as_u64().unwrap()
                <= c["capacity_accounted_bytes"].as_u64().unwrap()
        );
        if name != "manual" {
            assert_eq!(c["enabled"], false);
            assert_eq!(r["config"]["fits_cache_mib"], 0);
        }

        assert_eq!(p["mode"], if name == "manual" { "manual" } else { "auto" });
        let budget = p["effective_budget_bytes"].as_u64().unwrap();
        assert!(p["estimated_buffer_bytes"].as_u64().unwrap() <= budget);
        assert!(budget <= p["configured_budget_bytes"].as_u64().unwrap());
        assert!(p["system_reserve_bytes"].as_u64().unwrap() >= 2 * 1024 * 1024 * 1024);
        if name == "manual" {
            assert_eq!(r["integration"]["tile_width"], 128);
        }
        products.push(
            [".fits", "_coverage.fits", "_high_rejection.fits"].map(|suffix| {
                Sha256::digest(fs::read(dir.path().join(format!("{name}{suffix}"))).unwrap())
            }),
        );
    }
    assert_eq!(products[0], products[1]);
    assert_eq!(products[0], products[2]);
    for options in [
        vec!["--tile-size", "265"],
        vec!["--fits-cache-mib", "1025"],
        vec!["--fits-cache-mib", "-1"],
        vec!["--tile-size", "auto", "--tile-width", "256"],
        vec!["--tile-size", "256", "--tile-height", "256"],
    ] {
        let output = dir.path().join("unpublished.fits");
        let report = dir.path().join("unpublished.json");
        let mut args = vec![
            "stack",
            "--input",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--report",
            report.to_str().unwrap(),
        ];
        args.extend(options);
        assert!(!invoke(&args).status.success());
        assert!(!output.exists() && !report.exists());
    }
    assert_eq!(original, Sha256::digest(fs::read(&input).unwrap()));
    assert!(
        !fs::read_dir(dir.path()).unwrap().any(|p| p
            .unwrap()
            .path()
            .extension()
            .is_some_and(|e| e == "tmp"))
    );
}

/// Rewrite the 288 x 288 pixel block of a synthetic source in place.
fn map_pixels(path: &Path, f: impl Fn(usize, usize, f64) -> f64) {
    let mut bytes = fs::read(path).unwrap();
    for y in 0..288_usize {
        for x in 0..288_usize {
            let at = 2880 + (y * 288 + x) * 2;
            let old = f64::from(i16::from_be_bytes([bytes[at], bytes[at + 1]])) + 32768.;
            let new = (f(x, y, old).round() - 32768.) as i16;
            bytes[at..at + 2].copy_from_slice(&new.to_be_bytes());
        }
    }
    fs::write(path, bytes).unwrap();
}

fn set_card(path: &Path, key: &str, value: &str) {
    let mut bytes = fs::read(path).unwrap();
    for card in bytes[..2880].chunks_exact_mut(80) {
        if card.starts_with(format!("{key:<8}=").as_bytes()) {
            card.copy_from_slice(format!("{:<80}", format!("{key:<8}= {value:>20}")).as_bytes());
        }
    }
    fs::write(path, bytes).unwrap();
}

/// Sensor-fixed warm/hot pixels between the stars, as in uncalibrated lights.
fn hot_pattern(x: usize, y: usize) -> f64 {
    if (x * 37 + y * 101) % 211 == 0 && x % 48 > 8 && y % 48 > 8 {
        3500.
    } else {
        0.
    }
}

#[test]
fn stack_with_darks_builds_a_ram_master_calibrates_lights_and_keeps_sources() {
    let dir = tempfile::tempdir().unwrap();
    let (input, darks) = (dir.path().join("Light"), dir.path().join("Dark"));
    fs::create_dir(&input).unwrap();
    fs::create_dir(&darks).unwrap();
    for i in 0..3 {
        let light = input.join(format!("l{i}.fit"));
        quality_source(&light, false);
        map_pixels(&light, |x, y, v| v + hot_pattern(x, y));
    }
    for i in 0..3 {
        let dark = darks.join(format!("Dark_{i}.fit"));
        quality_source(&dark, false);
        map_pixels(&dark, |x, y, _| {
            200. + hot_pattern(x, y) + ((x * 3 + y * 5 + i) % 3) as f64 - 1.
        });
    }
    let all: Vec<_> = [&input, &darks]
        .iter()
        .flat_map(|d| fs::read_dir(d).unwrap().map(|e| e.unwrap().path()))
        .collect();
    let hashes: Vec<_> = all
        .iter()
        .map(|p| Sha256::digest(fs::read(p).unwrap()))
        .collect();
    let output = dir.path().join("dark_master.fits");
    let report = dir.path().join("dark.json");
    let result = invoke(&[
        "stack",
        "--input",
        input.to_str().unwrap(),
        "--dark",
        darks.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
        "--report",
        report.to_str().unwrap(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("Master dark: 3 frames") && stdout.contains("not written to disk"));
    let r: serde_json::Value = serde_json::from_str(&fs::read_to_string(&report).unwrap()).unwrap();
    assert_eq!(r["calibration"]["dark"]["frames"], 3);
    assert_eq!(r["calibration"]["written_to_disk"], false);
    assert_eq!(r["calibration"]["pedestal_adu"], 200.);
    assert_eq!(r["registration"]["accepted_count"], 3);
    let inspection = &r["registration"]["inspection"];
    assert_eq!(
        inspection["light_sets"][0]["calibration"]["dark"]["frames"],
        3
    );
    for frame in inspection["frames"].as_array().unwrap() {
        assert_eq!(frame["calibrated"], true);
        // The dark removed the fixed pattern before star measurement.
        assert_eq!(frame["suppressed_hot_pixels"], 0);
    }
    let header = String::from_utf8_lossy(&fs::read(&output).unwrap()[..2880 * 4]).into_owned();
    assert!(header.contains("3 frames calibrated before cosmetics and demosaicing:"));
    // Only the requested products exist; sources are byte-identical.
    let mut produced: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    produced.sort();
    assert_eq!(
        produced,
        [
            "Dark",
            "Light",
            "dark.json",
            "dark_master.fits",
            "dark_master_coverage.fits",
            "dark_master_high_rejection.fits"
        ]
    );
    for (path, hash) in all.iter().zip(&hashes) {
        assert_eq!(*hash, Sha256::digest(fs::read(path).unwrap()));
    }
    // A dark of another exposure still calibrates, with a recorded warning.
    for i in 0..3 {
        set_card(&darks.join(format!("Dark_{i}.fit")), "EXPTIME", "30");
    }
    let warned_output = dir.path().join("warned.fits");
    let warned_report = dir.path().join("warned.json");
    let warned = invoke(&[
        "stack",
        "--input",
        input.to_str().unwrap(),
        "--dark",
        darks.to_str().unwrap(),
        "--output",
        warned_output.to_str().unwrap(),
        "--report",
        warned_report.to_str().unwrap(),
    ]);
    assert!(
        warned.status.success(),
        "{}",
        String::from_utf8_lossy(&warned.stderr)
    );
    let r: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&warned_report).unwrap()).unwrap();
    for frame in r["registration"]["inspection"]["frames"]
        .as_array()
        .unwrap()
    {
        assert_eq!(frame["calibrated"], true);
        assert_eq!(frame["calibration_warnings"][0], "dark_exposure_differs");
    }
}

#[test]
fn a_saved_master_dark_stacks_exactly_like_its_frames() {
    let dir = tempfile::tempdir().unwrap();
    let (input, darks) = (dir.path().join("Light"), dir.path().join("Dark"));
    fs::create_dir(&input).unwrap();
    fs::create_dir(&darks).unwrap();
    for i in 0..3 {
        let light = input.join(format!("l{i}.fit"));
        quality_source(&light, false);
        map_pixels(&light, |x, y, v| v + hot_pattern(x, y));
    }
    for i in 0..7 {
        let dark = darks.join(format!("Dark_{i}.fit"));
        quality_source(&dark, false);
        map_pixels(&dark, |x, y, _| {
            200. + hot_pattern(x, y) + ((x * 3 + y * 5 + i) % 3) as f64 - 1.
        });
    }
    let master = dir.path().join("MasterDark.fits");
    let saved = invoke(&[
        "master-dark",
        "--dark",
        darks.to_str().unwrap(),
        "--combine",
        "median",
        "--output",
        master.to_str().unwrap(),
    ]);
    assert!(
        saved.status.success(),
        "{}",
        String::from_utf8_lossy(&saved.stderr)
    );
    assert!(String::from_utf8_lossy(&saved.stdout).contains("master dark from 7 frames (Median)"));
    assert_eq!(fits_card(&master, "BITPIX").as_deref(), Some("-32"));
    assert_eq!(
        fits_card(&master, "IMAGETYP").as_deref(),
        Some("Master Dark")
    );
    assert_eq!(fits_card(&master, "NCOMBINE").as_deref(), Some("7"));
    // The saved master never overwrites an existing file.
    let again = invoke(&[
        "master-dark",
        "--dark",
        darks.to_str().unwrap(),
        "--output",
        master.to_str().unwrap(),
    ]);
    assert!(!again.status.success());
    let run = |name: &str, dark: Vec<&str>| {
        let output = dir.path().join(format!("{name}.fits"));
        let mut args = vec![
            "stack",
            "--input",
            input.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
        ];
        let report = dir.path().join(format!("{name}.json"));
        args.extend(["--report", report.to_str().unwrap()]);
        args.extend(dark);
        let result = invoke(&args);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        (fs::read(output).unwrap(), report)
    };
    let (from_frames, _) = run(
        "frames",
        vec![
            "--dark",
            darks.to_str().unwrap(),
            "--dark-combine",
            "median",
        ],
    );
    let (from_file, report) = run("file", vec!["--dark-master", master.to_str().unwrap()]);
    assert_eq!(from_frames, from_file, "identical calibrated master");
    let r: serde_json::Value = serde_json::from_str(&fs::read_to_string(report).unwrap()).unwrap();
    assert_eq!(r["calibration"]["dark"]["loaded_from"], "MasterDark.fits");
    assert_eq!(r["calibration"]["dark"]["combine"], "median");
}

#[test]
fn stack_with_bias_darks_and_flats_applies_the_full_calibration() {
    let dir = tempfile::tempdir().unwrap();
    let folders: Vec<_> = ["Light", "Bias", "Dark", "Flat"]
        .iter()
        .map(|n| {
            let d = dir.path().join(n);
            fs::create_dir(&d).unwrap();
            d
        })
        .collect();
    let vignetting = |x: usize, y: usize| {
        1. - 0.2 * ((x as f64 - 144.).powi(2) + (y as f64 - 144.).powi(2)) / 41472.
    };
    for i in 0..3 {
        let light = folders[0].join(format!("l{i}.fit"));
        quality_source(&light, false);
        map_pixels(&light, |x, y, v| {
            (v - 2000.) * vignetting(x, y) + 2000. + hot_pattern(x, y)
        });
        let bias = folders[1].join(format!("b{i}.fit"));
        quality_source(&bias, false);
        map_pixels(&bias, |x, y, _| 200. + ((x + y + i) % 3) as f64 - 1.);
        set_card(&bias, "EXPTIME", "0.001");
        let dark = folders[2].join(format!("d{i}.fit"));
        quality_source(&dark, false);
        // Half the light exposure: the dark is scaled by two with the bias.
        map_pixels(&dark, |x, y, _| 200. + hot_pattern(x, y) / 2.);
        set_card(&dark, "EXPTIME", "5");
        let flat = folders[3].join(format!("f{i}.fit"));
        quality_source(&flat, false);
        map_pixels(&flat, |x, y, _| 200. + 20000. * vignetting(x, y));
        set_card(&flat, "EXPTIME", "1");
    }
    let output = dir.path().join("full.fits");
    let report = dir.path().join("full.json");
    let result = invoke(&[
        "stack",
        "--input",
        folders[0].to_str().unwrap(),
        "--bias",
        folders[1].to_str().unwrap(),
        "--dark",
        folders[2].to_str().unwrap(),
        "--flat",
        folders[3].to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
        "--report",
        report.to_str().unwrap(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let r: serde_json::Value = serde_json::from_str(&fs::read_to_string(&report).unwrap()).unwrap();
    assert_eq!(
        r["calibration"]["formula"],
        "(light - bias - (dark - bias) * t_light/t_dark) / normalized flat + pedestal"
    );
    assert_eq!(r["calibration"]["pedestal_adu"], 200.);
    assert_eq!(r["calibration"]["flat"]["frames"], 3);
    for frame in r["registration"]["inspection"]["frames"]
        .as_array()
        .unwrap()
    {
        assert_eq!(frame["calibrated"], true);
        assert_eq!(frame["calibration_warnings"][0], "dark_scaled_by_exposure");
        assert!(
            frame["calibration_negative_samples"].is_null(),
            "no negative samples"
        );
        assert_eq!(
            frame["suppressed_hot_pixels"], 0,
            "scaled dark removed the pattern"
        );
    }
    let header = String::from_utf8_lossy(&fs::read(&output).unwrap()[..2880 * 4]).into_owned();
    // The formula is wrapped over 72-character HISTORY cards.
    assert!(header.contains("3 frames calibrated before cosmetics and demosaicing:"));
    assert!(header.contains("normalized") && header.contains("pedestal"));
}
