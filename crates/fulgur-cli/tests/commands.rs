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
    assert_eq!(report["schema_version"], 2);
    assert_eq!(report["output_adu_divisor"], 65535.0);
    assert_eq!(report["output_offset"], 0.0);
    assert_eq!(report["samples_clipped"], 0);
    assert_eq!(report["samples_below_zero"], 0);
    assert_eq!(report["samples_above_one"], 0);
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
