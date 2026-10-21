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
