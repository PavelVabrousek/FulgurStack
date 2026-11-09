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

/// Highest sample of a master's first plane, in the 0..1 FITS scale.
fn master_peak(path: &Path) -> f32 {
    let bytes = fs::read(path).unwrap();
    let cards = bytes
        .chunks_exact(2880)
        .position(|block| block.chunks_exact(80).any(|c| c.starts_with(b"END ")))
        .unwrap();
    bytes[(cards + 1) * 2880..(cards + 1) * 2880 + 288 * 288 * 4]
        .chunks_exact(4)
        .map(|v| f32::from_be_bytes(v.try_into().unwrap()))
        .fold(0., f32::max)
}

/// A monolithic XISF unit around little-endian samples.
fn xisf_unit(path: &Path, attributes: &str, keywords: &str, data: &[u8]) {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xisf version=\"1.0\" xmlns=\"http://www.pixinsight.com/xisf\">\n<Image {attributes} location=\"attachment:8192:{}\">\n{keywords}</Image>\n</xisf>",
        data.len()
    );
    let mut bytes = b"XISF0100".to_vec();
    bytes.extend((xml.len() as u32).to_le_bytes());
    bytes.extend([0; 4]);
    bytes.extend(xml.as_bytes());
    bytes.resize(8192, 0);
    bytes.extend(data);
    fs::write(path, bytes).unwrap();
}

/// Bytes of a FITS image after its header.
fn pixels_of(path: &Path) -> Vec<u8> {
    let bytes = fs::read(path).unwrap();
    let blocks = bytes
        .chunks_exact(2880)
        .position(|block| block.chunks_exact(80).any(|c| c.starts_with(b"END ")))
        .unwrap();
    bytes[(blocks + 1) * 2880..].to_vec()
}

#[test]
fn bottom_up_frames_stack_exactly_like_the_same_frames_stored_top_down() {
    let dir = tempfile::tempdir().unwrap();
    let (top, bottom) = (dir.path().join("top"), dir.path().join("bottom"));
    fs::create_dir_all(&top).unwrap();
    fs::create_dir_all(&bottom).unwrap();
    for i in 0..6 {
        let path = top.join(format!("f{i}.fit"));
        quality_source(&path, false);
        let bytes = fs::read(&path).unwrap();
        // Same image and BAYERPAT; only the stored row order and its card differ.
        let mut flipped = bytes.clone();
        let card = b"SITELONG=";
        let at = (0..2880 - 80)
            .step_by(80)
            .find(|&p| flipped[p..].starts_with(card))
            .unwrap();
        let text = format!("{:<80}", "ROWORDER= 'BOTTOM-UP'");
        flipped[at..at + 80].copy_from_slice(text.as_bytes());
        for (y, row) in bytes[2880..2880 + 288 * 288 * 2]
            .chunks_exact(288 * 2)
            .rev()
            .enumerate()
        {
            flipped[2880 + y * 288 * 2..2880 + (y + 1) * 288 * 2].copy_from_slice(row);
        }
        fs::write(bottom.join(format!("f{i}.fit")), flipped).unwrap();
    }
    let mut masters = vec![];
    for (name, input) in [("top", &top), ("bottom", &bottom)] {
        let output = dir.path().join(format!("{name}.fits"));
        let report = dir.path().join(format!("{name}.json"));
        let result = invoke(&[
            "stack",
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
        assert_eq!(fits_card(&output, "ROWORDER").unwrap(), "TOP-DOWN");
        masters.push(pixels_of(&output));
    }
    assert!(masters[0] == masters[1], "master pixels differ");
}

#[test]
fn xisf_frames_stack_exactly_like_the_same_fits_frames() {
    let dir = tempfile::tempdir().unwrap();
    let (fits, xisf) = (dir.path().join("fits"), dir.path().join("xisf"));
    fs::create_dir_all(&fits).unwrap();
    fs::create_dir_all(&xisf).unwrap();
    for i in 0..6 {
        let path = fits.join(format!("f{i}.fit"));
        quality_source(&path, false);
        let bytes = fs::read(&path).unwrap();
        let samples: Vec<u8> = bytes[2880..2880 + 288 * 288 * 2]
            .chunks_exact(2)
            .flat_map(|p| {
                let v = (i16::from_be_bytes([p[0], p[1]]) as i32 + 32768) as u16;
                v.to_le_bytes()
            })
            .collect();
        xisf_unit(
            &xisf.join(format!("f{i}.xisf")),
            "geometry=\"288:288:1\" sampleFormat=\"UInt16\" colorSpace=\"Gray\"",
            "<FITSKeyword name=\"BAYERPAT\" value=\"'GRBG'\" comment=\"\"/>\n<FITSKeyword name=\"EXPTIME\" value=\"10\" comment=\"\"/>\n<FITSKeyword name=\"OBJECT\" value=\"'M33'\" comment=\"\"/>\n<FITSKeyword name=\"DATE-OBS\" value=\"'2026-10-06T22:00:00.000000'\" comment=\"\"/>\n<FITSKeyword name=\"FILTER\" value=\"'IRCUT'\" comment=\"\"/>\n",
            &samples,
        );
    }
    let mut masters = vec![];
    for (name, input) in [("fits", &fits), ("xisf", &xisf)] {
        let output = dir.path().join(format!("{name}.fits"));
        let report = dir.path().join(format!("{name}.json"));
        let result = invoke(&[
            "stack",
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
        masters.push(pixels_of(&output));
    }
    assert!(masters[0] == masters[1], "master pixels differ");
}

#[test]
fn debayered_xisf_frames_stack_as_colour_without_a_second_debayering() {
    let dir = tempfile::tempdir().unwrap();
    let lights = dir.path().join("lights");
    fs::create_dir_all(&lights).unwrap();
    let dark = |path: &Path| {
        quality_source(path, false);
        let bytes = fs::read(path).unwrap();
        fs::remove_file(path).unwrap();
        bytes[2880..2880 + 288 * 288 * 2]
            .chunks_exact(2)
            .map(|p| (i16::from_be_bytes([p[0], p[1]]) as i32 + 32768) as f32 / 65535.)
            .collect::<Vec<f32>>()
    };
    let plane = dark(&dir.path().join("plane.fit"));
    // A leftover BAYERPAT card must not turn the planes into a mosaic.
    let keywords = "<FITSKeyword name=\"BAYERPAT\" value=\"'GRBG'\" comment=\"\"/>\n<FITSKeyword name=\"EXPTIME\" value=\"10\" comment=\"\"/>\n<FITSKeyword name=\"OBJECT\" value=\"'M33'\" comment=\"\"/>\n<FITSKeyword name=\"DATE-OBS\" value=\"'2026-10-06T22:00:00.000000'\" comment=\"\"/>\n";
    for i in 0..6 {
        let mut samples = vec![];
        for gain in [0.5_f32, 1., 0.25] {
            samples.extend(plane.iter().flat_map(|v| (v * gain).to_le_bytes()));
        }
        xisf_unit(
            &lights.join(format!("d{i}.xisf")),
            "geometry=\"288:288:3\" sampleFormat=\"Float32\" bounds=\"0:1\" colorSpace=\"RGB\"",
            keywords,
            &samples,
        );
    }
    let output = dir.path().join("master.fits");
    let report = dir.path().join("stack.json");
    let result = invoke(&[
        "stack",
        "--input",
        lights.to_str().unwrap(),
        "--rejection",
        "winsorized",
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
    assert_eq!(fits_card(&output, "DEBAYER").unwrap(), "EXTERNAL");
    assert_eq!(fits_card(&output, "NAXIS3").unwrap(), "3");
    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report).unwrap()).unwrap();
    assert_eq!(json["registration"]["accepted_count"], 6);
    assert_eq!(
        json["registration"]["inspection"]["frames"][0]["frame"]["mosaic"],
        "rgb"
    );
    // Each colour plane keeps its own level: 0.5, 1 and 0.25 of the same sky.
    let sky = &json["master_quality"]["channel_background_adu"];
    let [r, g, b] = [0, 1, 2].map(|c| sky[c].as_f64().unwrap());
    assert!(
        (r / g - 0.5).abs() < 0.01 && (b / g - 0.25).abs() < 0.01,
        "{r} {g} {b}"
    );
}

#[test]
fn colour_sky_is_matched_per_channel_so_no_frame_is_rejected_for_its_sky() {
    let dir = tempfile::tempdir().unwrap();
    let lights = dir.path().join("lights");
    fs::create_dir_all(&lights).unwrap();
    // Sky brighter in red and darker in blue, by a different amount per frame.
    // GRBG: red sits on even rows and odd columns, blue on odd rows, even columns.
    let shifts = [0, 90, 180, 270, 360, 450];
    for (i, shift) in shifts.iter().enumerate() {
        let path = lights.join(format!("f{i}.fit"));
        quality_source(&path, false);
        let mut bytes = fs::read(&path).unwrap();
        for (p, pair) in bytes[2880..2880 + 288 * 288 * 2]
            .chunks_exact_mut(2)
            .enumerate()
        {
            let (x, y) = (p % 288, p / 288);
            let shift = match (x % 2, y % 2) {
                (1, 0) => *shift,
                (0, 1) => -shift / 3,
                _ => 0,
            };
            let v = i16::from_be_bytes([pair[0], pair[1]]) as i32 + shift;
            pair.copy_from_slice(&(v as i16).to_be_bytes());
        }
        fs::write(&path, bytes).unwrap();
    }
    let hash = Sha256::digest(fs::read(lights.join("f0.fit")).unwrap());
    let output = dir.path().join("master.fits");
    let report = dir.path().join("stack.json");
    let result = invoke(&[
        "stack",
        "--input",
        lights.to_str().unwrap(),
        "--rejection",
        "winsorized",
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
    let text = String::from_utf8_lossy(&result.stdout);
    assert!(
        text.contains("Normalization: additive with scaling"),
        "{text}"
    );
    let json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&report).unwrap()).unwrap();
    let plan = &json["registration"];
    let anchor = plan["reference_anchor"].as_str().unwrap();
    let index = |name: &str| name[1..2].parse::<usize>().unwrap();
    let base = shifts[index(anchor)] as f64;
    for frame in plan["frames"].as_array().unwrap() {
        let shift = shifts[index(frame["file_name"].as_str().unwrap())] as f64;
        let offsets = &frame["channel_offsets_adu"];
        let scale = frame["photometric_scale"].as_f64().unwrap();
        assert!((scale - 1.).abs() < 0.01, "{scale}");
        let expected = [base - shift, 0., (shift - base) / 3.];
        for (c, expected) in expected.into_iter().enumerate() {
            let actual = offsets[c].as_f64().unwrap();
            assert!((actual - expected).abs() < 3., "{c}: {actual} {expected}");
        }
    }
    // The frames differ only in their sky, so rejection finds next to nothing.
    let integration = &json["integration"];
    let rejected = integration["rejected_low_channel_samples"]
        .as_f64()
        .unwrap()
        + integration["rejected_high_channel_samples"]
            .as_f64()
            .unwrap();
    let share = rejected / integration["valid_channel_samples"].as_f64().unwrap();
    assert!(share < 0.02, "rejected share {share}");
    let [low, high] = [0, 1].map(|i| json["normalization"]["offset_range_adu"][0][i].as_f64());
    assert!(
        high.unwrap() - low.unwrap() > 440.,
        "red offsets {low:?} {high:?}"
    );
    let sky = json["master_quality"]["channel_background_adu"][0]
        .as_f64()
        .unwrap();
    assert!((sky - (2000. + base)).abs() < 5., "red sky {sky}");
    assert_eq!(
        Sha256::digest(fs::read(lights.join("f0.fit")).unwrap()),
        hash
    );
}

#[test]
fn per_filter_masters_share_one_grid_and_keep_their_own_brightness() {
    let dir = tempfile::tempdir().unwrap();
    let (broad, narrow) = (dir.path().join("broad"), dir.path().join("narrow"));
    fs::create_dir_all(&broad).unwrap();
    fs::create_dir_all(&narrow).unwrap();
    for i in 0..3 {
        quality_source(&broad.join(format!("b{i}.fit")), false);
        let path = narrow.join(format!("n{i}.fit"));
        quality_source(&path, false);
        // The same sky through another filter, three times brighter.
        let mut bytes = fs::read(&path).unwrap();
        let card = bytes.windows(7).position(|w| w == b"'IRCUT'").unwrap();
        bytes[card..card + 7].copy_from_slice(b"   'Ha'");
        for pair in bytes[2880..2880 + 288 * 288 * 2].chunks_exact_mut(2) {
            let v = i16::from_be_bytes([pair[0], pair[1]]) as i32 + 32768;
            let v = 2000 + 3 * (v - 2000);
            pair.copy_from_slice(&((v - 32768) as i16).to_be_bytes());
        }
        fs::write(&path, bytes).unwrap();
    }
    let hash = Sha256::digest(fs::read(narrow.join("n0.fit")).unwrap());
    let output = dir.path().join("master.fits");
    let report = dir.path().join("stack.json");
    let result = invoke(&[
        "stack",
        "--input",
        broad.to_str().unwrap(),
        "--input",
        narrow.to_str().unwrap(),
        "--input-sets",
        "--masters",
        "per-filter",
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
    assert!(!output.exists() && !report.exists(), "no combined master");
    let master = |label: &str| dir.path().join(format!("master_{label}.fits"));
    assert_eq!(fits_card(&master("IRCUT"), "FILTER").unwrap(), "IRCUT");
    assert_eq!(fits_card(&master("Ha"), "FILTER").unwrap(), "Ha");
    for key in ["NAXIS1", "NAXIS2"] {
        assert_eq!(
            fits_card(&master("IRCUT"), key),
            fits_card(&master("Ha"), key)
        );
    }
    // Star flux above the sky keeps the 3:1 ratio of the sources.
    let sky = 2000. / 65535.;
    let ratio = (master_peak(&master("Ha")) - sky) / (master_peak(&master("IRCUT")) - sky);
    assert!((2.8..3.2).contains(&ratio), "peak ratio {ratio}");
    for label in ["IRCUT", "Ha"] {
        let text = fs::read_to_string(dir.path().join(format!("stack_{label}.json"))).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(json["master_group"]["label"], label);
        assert_eq!(json["map_denominator_frames"], 3);
        assert_eq!(json["registration"]["frame_count"], 6);
        assert!(
            dir.path()
                .join(format!("master_coverage_{label}.fits"))
                .exists()
                || dir
                    .path()
                    .join(format!("master_{label}_coverage.fits"))
                    .exists()
        );
    }
    assert_eq!(
        hash,
        Sha256::digest(fs::read(narrow.join("n0.fit")).unwrap())
    );
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
fn survey_reads_full_files_of_the_folder_but_not_its_sub_folders_and_reports_no_gps() {
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
    // A sub-folder is a light set of its own (ADR-140).
    assert_eq!(report["frame_count"], 1);
    assert_eq!(report["total_bytes_read"], 5760);
    assert_eq!(report["total_exposure_seconds"], 10.0);
    assert!(!text.contains("SITELAT"));
    assert!(!text.contains("12.3456789"));
}

#[test]
fn invalid_input_and_empty_survey_have_actionable_failures() {
    let dir = tempfile::tempdir().unwrap();
    let result = invoke(&["survey", "--input", dir.path().to_str().unwrap()]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("No FITS or XISF input files"));
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
    let result = invoke(&bad);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("refused by the quality checks"));
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
    // Stars decide, not the name: another OBJECT card stacks all the same.
    let other = dir.path().join("other_target.fits");
    let mut wrong = bytes.clone();
    let at = wrong.windows(5).position(|w| w == b"'M33'").unwrap();
    wrong[at..at + 5].copy_from_slice(b"'M31'");
    fs::write(&other, wrong).unwrap();
    let renamed = dir.path().join("renamed.fits");
    let result = invoke(&[
        "stack",
        "--input",
        input.to_str().unwrap(),
        "--reference",
        other.to_str().unwrap(),
        "--output",
        renamed.to_str().unwrap(),
        "--report",
        dir.path().join("renamed.json").to_str().unwrap(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

/// A monochrome frame of the star field at `scale` template pixels per pixel.
fn camera_frame(
    path: &Path,
    size: usize,
    scale: f64,
    shift: f64,
    sky: f64,
    peak: f64,
    centers: &[(f64, f64)],
) {
    let mut bytes = Vec::new();
    for (key, value) in [
        ("SIMPLE", "T".to_owned()),
        ("BITPIX", "16".to_owned()),
        ("NAXIS", "2".to_owned()),
        ("NAXIS1", size.to_string()),
        ("NAXIS2", size.to_string()),
        ("BZERO", "32768".to_owned()),
        ("EXPTIME", "10".to_owned()),
        ("DATE-OBS", "'2026-10-06T22:00:00.000000'".to_owned()),
    ] {
        bytes.extend(format!("{:<80}", format!("{key:<8}= {value:>20}")).as_bytes());
    }
    bytes.extend(format!("{:<80}", "END").as_bytes());
    bytes.resize(2880, b' ');
    for y in 0..size {
        for x in 0..size {
            let mut value = sky + ((x * 13 + y * 17) % 23) as f64 - 11.;
            for (cx, cy) in centers {
                let (dx, dy) = (
                    x as f64 - (cx - shift) / scale,
                    y as f64 - (cy - shift) / scale,
                );
                value += peak * (-(dx * dx + dy * dy) / (2. * 1.4 * 1.4)).exp();
            }
            bytes.extend(((value.round() as i32 - 32768) as i16).to_be_bytes());
        }
    }
    bytes.resize(bytes.len().div_ceil(2880) * 2880, 0);
    fs::write(path, bytes).unwrap();
}

#[test]
fn cameras_with_other_sensors_stack_on_any_reference_at_one_brightness() {
    let dir = tempfile::tempdir().unwrap();
    let (wide, narrow) = (dir.path().join("wide"), dir.path().join("narrow"));
    fs::create_dir(&wide).unwrap();
    fs::create_dir(&narrow).unwrap();
    let mut centers: Vec<(f64, f64)> = vec![];
    for i in 0..400_u64 {
        let hash = (i + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let x = 30.2 + ((hash >> 20) % 516) as f64;
        let y = 30.3 + ((hash >> 40) % 516) as f64;
        if centers.len() < 70 && centers.iter().all(|&(cx, cy)| (cx - x).hypot(cy - y) > 28.) {
            centers.push((x, y));
        }
    }
    for name in ["1.fit", "2.fit"] {
        camera_frame(&wide.join(name), 288, 2., 0., 2000., 8000., &centers);
        camera_frame(&narrow.join(name), 240, 1.5, 90., 1000., 16000., &centers);
    }
    let template = dir.path().join("template.fits");
    let mut bytes = Vec::new();
    for (k, v) in [
        ("SIMPLE", "T"),
        ("BITPIX", "-32"),
        ("NAXIS", "2"),
        ("NAXIS1", "576"),
        ("NAXIS2", "576"),
        ("ROWORDER", "'TOP-DOWN'"),
    ] {
        bytes.extend(format!("{:<80}", format!("{k:<8}= {v:>20}")).as_bytes());
    }
    bytes.extend(format!("{:<80}", "END").as_bytes());
    bytes.resize(2880, b' ');
    for y in 0..576 {
        for x in 0..576 {
            let signal: f64 = centers
                .iter()
                .map(|(cx, cy)| {
                    let (dx, dy) = (x as f64 - cx, y as f64 - cy);
                    (-(dx * dx + dy * dy) / (2. * 2.8 * 2.8)).exp()
                })
                .sum();
            bytes.extend(((0.1 + 0.5 * signal) as f32).to_be_bytes());
        }
    }
    bytes.resize(bytes.len().div_ceil(2880) * 2880, 0);
    fs::write(&template, &bytes).unwrap();
    let stack = |name: &str, reference: Option<&Path>| {
        let report = dir.path().join(format!("{name}.json"));
        let output = dir.path().join(format!("{name}.fits"));
        let mut args = vec![
            "stack",
            "--input",
            wide.to_str().unwrap(),
            "--input",
            narrow.to_str().unwrap(),
            "--input-sets",
            "--output",
            output.to_str().unwrap(),
            "--report",
            report.to_str().unwrap(),
        ];
        if let Some(reference) = reference {
            args.extend(["--reference", reference.to_str().unwrap()]);
        }
        let result = invoke(&args);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        serde_json::from_str::<serde_json::Value>(&fs::read_to_string(report).unwrap()).unwrap()
    };
    // Automatic: the larger camera leads, and its anchor is the grid.
    let automatic = stack("automatic", None);
    assert_eq!(
        (automatic["width"].as_u64(), automatic["height"].as_u64()),
        (Some(288), Some(288))
    );
    let registration = &automatic["registration"];
    assert_eq!(
        registration["accepted_count"], 4,
        "{}",
        registration["camera_groups"]
    );
    assert!(registration["external_reference"].is_null());
    let cameras = registration["camera_groups"].as_array().unwrap();
    assert_eq!(
        (cameras[0]["width"].as_u64(), cameras[1]["stacked"].as_u64()),
        (Some(288), Some(2))
    );
    // The first camera stays exactly on its own grid.
    for f in registration["frames"].as_array().unwrap() {
        let brightness = f["photometric_scale"].as_f64().unwrap();
        if !f["file_name"].as_str().unwrap().contains("narrow") {
            assert_eq!(f["transform"]["scale_factor"].as_f64(), Some(1.));
            assert!((brightness - 1.).abs() < 1e-9, "{brightness}");
        }
    }
    // A frame of the smaller camera can be the reference: its camera leads.
    let chosen = stack("chosen", Some(&narrow.join("2.fit")));
    assert_eq!(
        (chosen["width"].as_u64(), chosen["height"].as_u64()),
        (Some(240), Some(240))
    );
    let registration = &chosen["registration"];
    assert_eq!(registration["accepted_count"], 4);
    assert_eq!(registration["camera_groups"][0]["width"], 240);
    assert!(
        registration["reference_anchor"]
            .as_str()
            .unwrap()
            .ends_with("2.fit")
    );
    let r = stack("joined", Some(&template));
    assert_eq!(
        (r["width"].as_u64(), r["height"].as_u64()),
        (Some(576), Some(576))
    );
    let registration = &r["registration"];
    assert_eq!(registration["accepted_count"], 4);
    let cameras = registration["camera_groups"].as_array().unwrap();
    assert_eq!(cameras.len(), 2);
    assert_eq!(cameras[0]["width"], 288);
    assert_eq!(cameras[1]["width"], 240);
    assert_eq!(cameras[1]["stacked"], 2);
    // Equal star flux per template pixel: 8000 * 2 * 2 against 16000 * 1.5 * 1.5.
    let expected = 8000. * 4. / (16000. * 2.25);
    let measured = cameras[1]["brightness_scale"].as_f64().unwrap();
    assert!((measured / expected - 1.).abs() < 0.03, "{measured}");
    for f in registration["frames"].as_array().unwrap() {
        let scale = f["transform"]["scale_factor"].as_f64().unwrap();
        let brightness = f["photometric_scale"].as_f64().unwrap();
        if f["file_name"].as_str().unwrap().contains("narrow") {
            assert!((scale - 1.5).abs() < 0.01, "{scale}");
            assert!((brightness / expected - 1.).abs() < 0.03, "{brightness}");
            let offset = f["channel_offsets_adu"][1].as_f64().unwrap();
            // The narrow camera's sky lands on the wide camera's sky.
            assert!(
                (brightness * 1000. + offset - 2000.).abs() < 40.,
                "{offset}"
            );
        } else {
            assert!((scale - 2.).abs() < 0.01, "{scale}");
            assert!((brightness - 1.).abs() < 1e-9, "{brightness}");
        }
    }
    // Other optics on an equal sensor: the set fails together with the first
    // one, tries again as its own camera and joins the stack.
    let other = dir.path().join("other");
    fs::create_dir(&other).unwrap();
    for name in ["1.fit", "2.fit"] {
        camera_frame(&other.join(name), 288, 1.5, 90., 1000., 16000., &centers);
    }
    let report = dir.path().join("optics.json");
    let output = dir.path().join("optics.fits");
    let result = invoke(&[
        "stack",
        "--input",
        wide.to_str().unwrap(),
        "--input",
        other.to_str().unwrap(),
        "--input-sets",
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
    let r: serde_json::Value = serde_json::from_str(&fs::read_to_string(report).unwrap()).unwrap();
    assert_eq!(r["registration"]["accepted_count"], 4);
    assert_eq!(
        r["registration"]["camera_groups"].as_array().unwrap().len(),
        2
    );
    // The second pass is planned work: the bar leaves 100 % only at the end.
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let sets: Vec<_> = [&wide, &other]
        .into_iter()
        .map(|dir| {
            fulgur_pipeline::inspect::single_set(
                std::slice::from_ref(dir),
                fulgur_core::LayoutChoice::Auto,
            )
        })
        .collect();
    let (inspection, root) = fulgur_pipeline::inspect::inspect_sets(
        &sets,
        fulgur_pipeline::inspect::InspectConfig::default(),
        &cancel,
        None,
    )
    .unwrap();
    let seen = std::sync::Mutex::new(vec![]);
    let (plan, _) = fulgur_pipeline::register::register_inspected_with_options(
        inspection,
        root,
        0.8,
        None,
        fulgur_pipeline::register::RegistrationOptions::default(),
        &cancel,
        std::time::Instant::now(),
        Some(&|n, total| seen.lock().unwrap().push((n, total))),
    )
    .unwrap();
    assert_eq!(plan.accepted_count, 4);
    let seen = seen.into_inner().unwrap();
    let last = *seen.last().unwrap();
    assert!(seen.iter().any(|&(_, total)| total < last.1), "{seen:?}");
    assert!(seen.windows(2).all(|p| p[1].0 >= p[0].0), "{seen:?}");
    assert_eq!(last.0, last.1, "{seen:?}");
    assert!(
        seen[..seen.len() - 1].iter().all(|&(n, t)| n < t),
        "{seen:?}"
    );
    // A camera that sees the sky mirrored joins in reversed rows.
    let mirrored = dir.path().join("mirrored");
    fs::create_dir(&mirrored).unwrap();
    let flipped: Vec<_> = centers.iter().map(|&(x, y)| (x, 540. - y)).collect();
    for name in ["1.fit", "2.fit"] {
        camera_frame(&mirrored.join(name), 240, 1.5, 90., 1000., 16000., &flipped);
    }
    let report = dir.path().join("mirrored.json");
    let output = dir.path().join("mirrored.fits");
    let result = invoke(&[
        "stack",
        "--input",
        wide.to_str().unwrap(),
        "--input",
        mirrored.to_str().unwrap(),
        "--input-sets",
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
    let r: serde_json::Value = serde_json::from_str(&fs::read_to_string(report).unwrap()).unwrap();
    let registration = &r["registration"];
    assert_eq!(
        registration["accepted_count"], 4,
        "{}",
        registration["camera_groups"]
    );
    assert_eq!(registration["camera_groups"][0]["mirrored"], false);
    assert_eq!(registration["camera_groups"][1]["mirrored"], true);
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

/// Already calibrated lights whose header lost EXPTIME and BAYERPAT (ADR-084).
fn stripped_source(path: &Path, shift: (usize, usize)) {
    mono_source(path, shift);
    let mut bytes = fs::read(path).unwrap();
    for card in bytes[..2880].chunks_exact_mut(80) {
        if card.starts_with(b"EXPTIME ") {
            let text = format!("{:<8}= {:>20}", "INSTRUME", "'ZWO ASI2600MC Air'");
            card.copy_from_slice(format!("{text:<80}").as_bytes());
        }
    }
    fs::write(path, bytes).unwrap();
}

#[test]
fn derived_header_values_are_listed_and_used_only_after_confirmation_or_correction() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("lights");
    fs::create_dir(&input).unwrap();
    for (i, shift) in [(0, 0), (2, 1), (1, 3)].into_iter().enumerate() {
        let name =
            format!("c_Light_M33_10.0s_Bin1_2600MC_gain100_2026010{i}-200800_SQA_000{i}.fit");
        stripped_source(&input.join(name), shift);
    }
    let path = |name: &str| dir.path().join(name).to_str().unwrap().to_owned();
    let stack = |extra: &[&str], name: &str| {
        let mut args = vec![
            "stack".to_owned(),
            "--input".into(),
            input.to_str().unwrap().into(),
            "--output".into(),
            path(&format!("{name}.fits")),
            "--report".into(),
            path(&format!("{name}.json")),
        ];
        args.extend(extra.iter().map(|s| s.to_string()));
        invoke(&args.iter().map(String::as_str).collect::<Vec<_>>())
    };
    // Unconfirmed: the derived values are listed and nothing is written.
    let refused = stack(&[], "refused");
    let stdout = String::from_utf8_lossy(&refused.stdout);
    assert!(!refused.status.success());
    assert!(
        stdout.contains("exposure_seconds: 10 s - file name token '10.0s' (3 files)"),
        "{stdout}"
    );
    assert!(
        stdout.contains("sensor_layout: colour RGGB - camera catalogue: ZWO ASI2600MC"),
        "{stdout}"
    );
    assert!(stdout.contains("gain: 100 - file name token 'gain100' (3 files) [label only]"));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("--accept-derived"));
    assert!(!dir.path().join("refused.fits").exists());
    // Confirmed: colour RGGB from the catalogue, exposure from the file name.
    let accepted = stack(&["--accept-derived"], "accepted");
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    assert_eq!(
        fits_card(&dir.path().join("accepted.fits"), "NAXIS3").as_deref(),
        Some("3")
    );
    let r: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("accepted.json")).unwrap())
            .unwrap();
    let inspection = &r["registration"]["inspection"];
    assert_eq!(
        inspection["light_sets"][0]["derived_metadata_confirmed"],
        true
    );
    // Three frames: Auto rejection resolves to the median (ADR-087).
    assert_eq!(r["integration"]["rejection"], "median");
    assert_eq!(r["config"]["rejection"], "auto");
    // The master is measured like the frames (ADR-086).
    let quality = &r["master_quality"];
    assert!(
        quality["measured_stars"].as_u64().unwrap() >= 10,
        "{quality}"
    );
    let master = quality["hfr_px"].as_f64().unwrap();
    let frames = quality["frames_hfr_px"].as_f64().unwrap();
    assert!(
        master > 0.5 && (master / frames - 1.).abs() < 0.5,
        "{master} vs {frames}"
    );
    assert!(quality["background_adu"].as_f64().unwrap() > 1000.);
    for frame in inspection["frames"].as_array().unwrap() {
        assert_eq!(frame["frame"]["mosaic_source"], "camera_catalog");
        assert_eq!(frame["frame"]["exposure_seconds"], 10.0);
        assert_eq!(frame["frame"]["derived"][0]["field"], "exposure_seconds");
    }
    // Corrected: the owner's layout and exposure replace the derived values,
    // so only labels remain and no confirmation is needed.
    let corrected = stack(&["--sensor", "mono", "--exposure", "12"], "corrected");
    assert!(
        corrected.status.success(),
        "{}",
        String::from_utf8_lossy(&corrected.stderr)
    );
    assert_eq!(
        fits_card(&dir.path().join("corrected.fits"), "NAXIS").as_deref(),
        Some("2")
    );
    let r: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("corrected.json")).unwrap())
            .unwrap();
    let frame = &r["registration"]["inspection"]["frames"][0]["frame"];
    assert_eq!(
        (
            frame["exposure_seconds"].as_f64(),
            frame["mosaic_source"].as_str()
        ),
        (Some(12.0), Some("user"))
    );
    // No exposure anywhere: the owner must enter it.
    let bare = dir.path().join("bare");
    fs::create_dir(&bare).unwrap();
    stripped_source(&bare.join("frame_0001.fit"), (0, 0));
    let missing = invoke(&[
        "inspect",
        "--input",
        bare.to_str().unwrap(),
        "--accept-derived",
        "--report",
        &path("bare.json"),
    ]);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stdout).contains("exposure_seconds: missing"));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("--exposure <seconds>"));
}

/// The data block of an XISF unit written by FulgurStack, and its header.
fn xisf_block(path: &Path) -> (String, Vec<u8>) {
    let bytes = fs::read(path).unwrap();
    assert_eq!(&bytes[..8], b"XISF0100");
    let length = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let xml = String::from_utf8(bytes[16..16 + length].to_vec()).unwrap();
    let place = xml.split("location=\"attachment:").nth(1).unwrap();
    let mut numbers = place.split('"').next().unwrap().split(':');
    let position: usize = numbers.next().unwrap().parse().unwrap();
    let size: usize = numbers.next().unwrap().parse().unwrap();
    (xml, bytes[position..position + size].to_vec())
}

/// The data of a FulgurStack FITS file, as little-endian floats.
fn fits_block_le(path: &Path) -> Vec<u8> {
    let bytes = fs::read(path).unwrap();
    let end = bytes
        .chunks_exact(80)
        .position(|c| c.starts_with(b"END "))
        .unwrap();
    let start = ((end + 1) * 80).div_ceil(2880) * 2880;
    let planes = if fits_card(path, "NAXIS").as_deref() == Some("3") {
        3
    } else {
        1
    };
    let width: usize = fits_card(path, "NAXIS1").unwrap().parse().unwrap();
    let height: usize = fits_card(path, "NAXIS2").unwrap().parse().unwrap();
    bytes[start..start + width * height * planes * 4]
        .chunks_exact(4)
        .flat_map(|c| [c[3], c[2], c[1], c[0]])
        .collect()
}

#[test]
fn an_xisf_master_and_its_maps_hold_exactly_the_fits_samples() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("mono");
    fs::create_dir(&input).unwrap();
    for (i, shift) in [(0, 0), (2, 1), (1, 3)].into_iter().enumerate() {
        mono_source(&input.join(format!("l{i}.fit")), shift);
    }
    for name in ["fits_master.fits", "xisf_master.xisf"] {
        let output = dir.path().join(name);
        let report = dir.path().join(format!("{name}.json"));
        let result = invoke(&[
            "stack",
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
        let r: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&report).unwrap()).unwrap();
        let format = if name.ends_with("xisf") {
            "xisf"
        } else {
            "fits"
        };
        assert_eq!(r["output_format"], format);
        assert_eq!(
            r["coverage_file"],
            name.replace(&format!(".{format}"), &format!("_coverage.{format}"))
        );
    }
    for (fits, xisf) in [
        ("fits_master.fits", "xisf_master.xisf"),
        ("fits_master_coverage.fits", "xisf_master_coverage.xisf"),
        (
            "fits_master_high_rejection.fits",
            "xisf_master_high_rejection.xisf",
        ),
    ] {
        let (xml, block) = xisf_block(&dir.path().join(xisf));
        assert!(xml.contains("geometry=\"288:288:1\""), "{xisf}");
        assert!(xml.contains("colorSpace=\"Gray\""), "{xisf}");
        assert_eq!(block, fits_block_le(&dir.path().join(fits)), "{xisf}");
    }
    let wrong = dir.path().join("master.png");
    let result = invoke(&[
        "stack",
        "--input",
        input.to_str().unwrap(),
        "--output",
        wrong.to_str().unwrap(),
        "--report",
        dir.path().join("wrong.json").to_str().unwrap(),
    ]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains(".xisf"));
    assert!(!wrong.exists());
}

#[test]
fn hot_pixel_map_is_built_from_darks_saved_reused_and_reported() {
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
    let hashes: Vec<_> = fs::read_dir(&darks)
        .unwrap()
        .map(|e| Sha256::digest(fs::read(e.unwrap().path()).unwrap()))
        .collect();
    let map = dir.path().join("hot.fit");
    let run = |name: &str, extra: &[&str]| {
        let output = dir.path().join(format!("{name}.fits"));
        let report = dir.path().join(format!("{name}.json"));
        let mut args = vec!["stack", "--input", input.to_str().unwrap()];
        args.extend_from_slice(extra);
        args.extend(["--output", output.to_str().unwrap()]);
        args.extend(["--report", report.to_str().unwrap()]);
        let result = invoke(&args);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&report).unwrap()).unwrap();
        (String::from_utf8_lossy(&result.stdout).into_owned(), json)
    };
    // Built from dark frames, saved, and nothing else calibrates the lights.
    let (stdout, r) = run(
        "built",
        &[
            "--hotpixel-map",
            darks.to_str().unwrap(),
            "--hotpixel-percent",
            "0.05",
            "--hotpixel-map-save",
            map.to_str().unwrap(),
        ],
    );
    assert!(stdout.contains("Hot pixel map:") && stdout.contains("Hot pixel map saved."));
    let pixels = r["calibration"]["hot_pixels"]["pixels"].as_u64().unwrap();
    assert!(pixels >= 1);
    assert_eq!(r["calibration"]["hot_pixels"]["origin"], "frames");
    assert_eq!(r["calibration"]["formula"], "light with hot pixel map");
    assert_eq!(r["registration"]["accepted_count"], 3);
    assert!(map.exists());
    // The saved map is reused as it is and is never overwritten.
    let (stdout, r) = run("reused", &["--hotpixel-map", map.to_str().unwrap()]);
    assert!(stdout.contains("from map file"));
    assert_eq!(r["calibration"]["hot_pixels"]["pixels"], pixels);
    // Another share or a dark master alongside works; a bad share is refused.
    let (_, r) = run(
        "with_dark",
        &[
            "--dark",
            darks.to_str().unwrap(),
            "--hotpixel-map",
            map.to_str().unwrap(),
        ],
    );
    assert!(
        r["calibration"]["formula"]
            .as_str()
            .unwrap()
            .ends_with("+ hot pixel map")
    );
    let refused = invoke(&[
        "stack",
        "--input",
        input.to_str().unwrap(),
        "--hotpixel-map",
        darks.to_str().unwrap(),
        "--hotpixel-percent",
        "9",
        "--output",
        dir.path().join("bad.fits").to_str().unwrap(),
        "--report",
        dir.path().join("bad.json").to_str().unwrap(),
    ]);
    assert!(!refused.status.success());
    assert!(!dir.path().join("bad.fits").exists());
    // Dark frames stay byte-identical.
    let after: Vec<_> = fs::read_dir(&darks)
        .unwrap()
        .map(|e| Sha256::digest(fs::read(e.unwrap().path()).unwrap()))
        .collect();
    assert_eq!(hashes, after);
}

/// A 400 px monochrome frame of stars with their own peaks; peaks above the
/// 16-bit range are clipped, which saturates the core.
fn saturated_field_frame(path: &Path, date: &str, shift: f64, stars: &[(f64, f64, f64)]) {
    let size = 400;
    let mut bytes = Vec::new();
    for (key, value) in [
        ("SIMPLE", "T".to_owned()),
        ("BITPIX", "16".to_owned()),
        ("NAXIS", "2".to_owned()),
        ("NAXIS1", size.to_string()),
        ("NAXIS2", size.to_string()),
        ("BZERO", "32768".to_owned()),
        ("EXPTIME", "10".to_owned()),
        ("DATE-OBS", format!("'{date}'")),
    ] {
        bytes.extend(format!("{:<80}", format!("{key:<8}= {value:>20}")).as_bytes());
    }
    bytes.extend(format!("{:<80}", "END").as_bytes());
    bytes.resize(2880, b' ');
    for y in 0..size {
        for x in 0..size {
            let mut value = 1500. + ((x * 13 + y * 17) % 23) as f64 - 11.;
            for &(cx, cy, peak) in stars {
                let (dx, dy) = (x as f64 - (cx - shift), y as f64 - (cy - shift));
                value += peak * (-(dx * dx + dy * dy) / (2. * 1.4 * 1.4)).exp();
            }
            bytes.extend(((value.round().min(65535.) as i32 - 32768) as i16).to_be_bytes());
        }
    }
    bytes.resize(bytes.len().div_ceil(2880) * 2880, 0);
    fs::write(path, bytes).unwrap();
}

#[test]
fn two_nights_sharing_only_saturated_stars_register_through_their_positions() {
    let dir = tempfile::tempdir().unwrap();
    let (first, second) = (dir.path().join("night_1"), dir.path().join("night_2"));
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    // Sixty saturated stars both nights show; each night adds thirty
    // unsaturated stars of its own, so their registration lists share none.
    let mut shared: Vec<(f64, f64, f64)> = vec![];
    let (mut own_first, mut own_second): (Vec<_>, Vec<_>) = (vec![], vec![]);
    for i in 0..4000_u64 {
        // SplitMix64: a plain multiplicative hash lays the stars on a lattice,
        // which any similarity maps onto itself.
        let mut z = (i + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        let x = 20.2 + (z % 360) as f64;
        let y = 20.3 + ((z >> 32) % 360) as f64;
        let all = shared.iter().chain(&own_first).chain(&own_second);
        if all.clone().all(|&(cx, cy, _)| (cx - x).hypot(cy - y) > 22.) {
            match (shared.len() < 60, own_first.len() < 30) {
                (true, _) => shared.push((x, y, 800_000.)),
                (false, true) => own_first.push((x, y, 9000.)),
                _ if own_second.len() < 30 => own_second.push((x, y, 9000.)),
                _ => break,
            }
        }
    }
    assert_eq!(
        (shared.len(), own_first.len(), own_second.len()),
        (60, 30, 30)
    );
    let night = |own: &[(f64, f64, f64)]| [shared.clone(), own.to_vec()].concat();
    for (name, shift) in [("1.fit", 0.), ("2.fit", 2.5), ("3.fit", 5.)] {
        saturated_field_frame(
            &first.join(name),
            "2026-10-06T22:00:00.000000",
            shift,
            &night(&own_first),
        );
        saturated_field_frame(
            &second.join(name),
            "2026-10-08T22:00:00.000000",
            shift,
            &night(&own_second),
        );
    }
    let report = dir.path().join("report.json");
    let result = invoke(&[
        "stack",
        "--input",
        first.to_str().unwrap(),
        "--input",
        second.to_str().unwrap(),
        "--input-sets",
        "--masters",
        "per-set",
        "--output",
        dir.path().join("master.fits").to_str().unwrap(),
        "--report",
        report.to_str().unwrap(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("report_night_1.json")).unwrap())
            .unwrap();
    let registration = &report["registration"];
    // Their brightness cannot be linked without shared unsaturated stars, so
    // each night keeps its own master; the geometry is what the saturated
    // stars give.
    assert_eq!(registration["accepted_count"], 6, "{registration:#}");
}
