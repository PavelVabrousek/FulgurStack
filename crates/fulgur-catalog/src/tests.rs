use super::*;
use std::cell::Cell;
use std::io::Write;

/// A minimal Gaia XPSD file: one equirectangular tree whose root is a leaf
/// with uncompressed 32-byte records, magnitudes up to G 16.
fn tiny_xpsd(dir: &std::path::Path, stars: &[(f64, f64, f64)]) {
    tiny_xpsd_file(dir, "test-01.xpsd", (-2., 16.), stars);
}

fn tiny_xpsd_file(dir: &std::path::Path, name: &str, range: (f64, f64), stars: &[(f64, f64, f64)]) {
    let (x0, y0, x1, y1) = (-23.0f64, 14.0f64, -21.0f64, 16.0f64); // RA 22..24, Dec 14..16
    let mut block = vec![];
    for &(ra, dec, g) in stars {
        let dx = (((ra - 45.) - x0) * 3600. * 1000. * 500.).round() as u32;
        let dy = ((dec - y0) * 3600. * 1000. * 500.).round() as u32;
        block.extend(dx.to_le_bytes());
        block.extend(dy.to_le_bytes());
        block.extend(1.5f32.to_le_bytes()); // parallax
        block.extend(10.0f32.to_le_bytes()); // pmra
        block.extend((-5.0f32).to_le_bytes()); // pmdec
        block.extend((((g + 1.5) * 1000.).round() as u16).to_le_bytes());
        block.extend(0u16.to_le_bytes());
        block.extend(0u16.to_le_bytes());
        block.extend(0i16.to_le_bytes());
        block.extend(0u32.to_le_bytes());
    }
    // Header length is fixed below so the positions inside it are known.
    let header_len = 1024usize;
    let tree_at = 16 + header_len;
    let data_at = tree_at + 48;
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><xpsd version="1.0"><Metadata><DatabaseIdentifier>GaiaDR3</DatabaseIdentifier></Metadata><Data magnitudeRange="{:.2},{:.2}" position="{data_at}" compression="" itemSize="32"/><Tree projection="Equirectangular" center="45,0" rootPosition="{tree_at}" nodeCount="1"/></xpsd>"#,
        range.0, range.1
    );
    let mut header = xml.into_bytes();
    header.resize(header_len, b' ');
    let mut file = std::fs::File::create(dir.join(name)).unwrap();
    file.write_all(b"XPSD0100").unwrap();
    file.write_all(&(header_len as u32).to_le_bytes()).unwrap();
    file.write_all(&[0; 4]).unwrap();
    file.write_all(&header).unwrap();
    for v in [x0, y0, x1, y1] {
        file.write_all(&v.to_le_bytes()).unwrap();
    }
    file.write_all(&(0x8000_0000_0000_0000u64).to_le_bytes())
        .unwrap(); // leaf, offset 0
    file.write_all(&(block.len() as u32).to_le_bytes()).unwrap();
    file.write_all(&(block.len() as u32).to_le_bytes()).unwrap();
    file.write_all(&block).unwrap();
}

const CSV: &str =
    "source_id,ra,dec,phot_g_mean_mag,pmra,pmdec\n7,23.0,15.0,9.5,1.0,2.0\n8,23.2,15.1,12.0,,\n";

#[test]
fn pixinsight_files_answer_without_any_download() {
    let dir = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    tiny_xpsd(
        dir.path(),
        &[(23.0, 15.0, 12.5), (23.5, 15.5, 8.25), (25.0, 15.0, 9.0)],
    );
    let calls = Cell::new(0);
    let fetch = |_: &str, _: &str| -> Result<String, String> {
        calls.set(calls.get() + 1);
        Err("no network in this test".into())
    };
    let sources = GaiaSources {
        pixinsight: Some(dir.path().to_path_buf()),
        cache: cache.path().to_path_buf(),
        download: true,
    };
    let field = gaia_field_with(&sources, 23.2, 15.2, 1.0, 15.0, &fetch).unwrap();
    assert_eq!(calls.get(), 0, "the PixInsight files cover G 15");
    assert!(field.source.starts_with("PixInsight"));
    // Two stars lie inside the cone, brightest first; the third is outside.
    assert_eq!(field.stars.len(), 2);
    assert!((field.stars[0].ra - 23.5).abs() < 1e-9 && (field.stars[0].dec - 15.5).abs() < 1e-9);
    assert!((field.stars[0].g - 8.25).abs() < 1e-3);
    assert!((field.stars[1].pmra - 10.).abs() < 1e-6 && (field.stars[1].pmdec + 5.).abs() < 1e-6);
    // Deeper than the files reach: the archive is asked.
    let deeper = gaia_field_with(&sources, 23.2, 15.2, 1.0, 17.0, &fetch);
    assert!(deeper.is_err());
    assert_eq!(calls.get(), 2, "ESA, then GAVO");
}

#[test]
fn the_download_falls_back_to_gavo_and_then_serves_the_cache() {
    let cache = tempfile::tempdir().unwrap();
    let fetch = |url: &str, query: &str| -> Result<String, String> {
        assert!(query.contains("phot_g_mean_mag < 16"));
        if url.contains("esac") {
            Ok("<VOTABLE><INFO name=\"QUERY_STATUS\" value=\"ERROR\">busy</INFO></VOTABLE>".into())
        } else {
            assert!(query.contains("gaia.dr3lite"));
            Ok(CSV.into())
        }
    };
    let sources = GaiaSources {
        pixinsight: None,
        cache: cache.path().to_path_buf(),
        download: true,
    };
    let first = gaia_field_with(&sources, 23.1, 15.05, 1.0, 16.0, &fetch).unwrap();
    assert_eq!(first.source, "Gaia DR3 GAVO data centre");
    assert!(
        first
            .failures
            .iter()
            .any(|f| f.starts_with("ESA Gaia archive"))
    );
    assert_eq!(first.stars.len(), 2);
    assert_eq!(first.stars[0].source_id, 7);
    let never = |_: &str, _: &str| -> Result<String, String> { panic!("cached") };
    let second = gaia_field_with(&sources, 23.1, 15.05, 1.0, 16.0, &never).unwrap();
    assert_eq!(second.source, "Gaia DR3 cache");
    // Without permission to download, an uncached field fails.
    let offline = GaiaSources {
        download: false,
        ..sources
    };
    assert!(gaia_field_with(&offline, 100., 15., 1.0, 16.0, &never).is_err());
}

#[test]
fn a_wide_field_is_tiled_and_duplicates_dropped() {
    let cache = tempfile::tempdir().unwrap();
    let queries = Cell::new(0);
    let fetch = |_: &str, _: &str| -> Result<String, String> {
        queries.set(queries.get() + 1);
        Ok(CSV.into())
    };
    let answer = download::cone(cache.path(), 23.0, 15.0, 4.0, 13.0, &fetch).unwrap();
    assert!(queries.get() > 1);
    assert_eq!(answer.stars.len(), 2, "the same two stars from every tile");
}

#[test]
fn the_deep_sky_table_knows_names_aliases_and_catalogue_spellings() {
    let m33 = dso::find("M33").unwrap();
    assert_eq!(m33.name, "NGC 598");
    assert!((m33.ra - 23.46204).abs() < 1e-4 && (m33.dec - 30.66022).abs() < 1e-4);
    for name in [
        "M 33",
        "Messier 33",
        "m033",
        "NGC 598",
        "NGC0598",
        "Triangulum Galaxy",
    ] {
        assert_eq!(dso::find(name).unwrap().name, "NGC 598", "{name}");
    }
    assert_eq!(dso::find("North America Nebula").unwrap().name, "NGC 7000");
    assert_eq!(dso::find("C 20").unwrap().name, "NGC 7000");
    assert_eq!(dso::find("IC1396").unwrap().name, "IC 1396");
    assert_eq!(dso::find("Orion Nebula").unwrap().name, "NGC 1976");
    assert_eq!(dso::find("LBN 974").unwrap().name, "NGC 1976");
    assert!(dso::names() > 14_000);
}

#[test]
fn lbn_sharpless_and_barnard_are_complete_catalogues() {
    let lbn = dso::find("LBN 437").unwrap();
    assert!((lbn.ra - 338.05).abs() < 1e-3 && (lbn.dec - 40.58333).abs() < 1e-3);
    for name in ["Sh2-155", "Sh 2-155", "SH2 155", "Sharpless 155"] {
        assert_eq!(dso::find(name).unwrap().name, "Sh2-155", "{name}");
    }
    for name in ["B 33", "Barnard 33", "B33", "Horsehead Nebula"] {
        assert_eq!(dso::find(name).unwrap().name, "B 33", "{name}");
    }
    assert_eq!(dso::find("Barnard 72").unwrap().name, "B 72");
    assert!(dso::find("LBN 1").is_some() && dso::find("Sh2-1").is_some());
    assert!(dso::names() > 15_800);
}

#[test]
fn online_answers_are_read_and_services_tried_in_order() {
    let ned = r#"{"Preferred": {"Name": "Messier 033", "Position": {"RA": 23.46204167, "Dec": 30.66022222}}}"#;
    assert_eq!(lookup::parse_ned(ned), Some((23.46204167, 30.66022222)));
    let simbad = "Object M 33\nCoordinates(ICRS,ep=J2000,eq=2000): 01 33 50.904 +30 39 35.79 (Optical) [ ]\n";
    let (ra, dec) = lookup::parse_simbad(simbad).unwrap();
    assert!((ra - 23.46210).abs() < 1e-4 && (dec - 30.65994).abs() < 1e-4);
    assert_eq!(
        lookup::parse_sesame("#=N=NED\n%J 23.46204 +30.66022 = 01:33:50.89 +30:39:36.8\n"),
        Some((23.46204, 30.66022))
    );
    // NED down, SIMBAD answers; the failure is reported.
    let get = |url: &str| -> Result<String, String> {
        if url.contains("ned.ipac") {
            Err("timeout".into())
        } else if url.contains("simbad") {
            assert!(url.contains("Ident=LBN%20437"));
            Ok("Coordinates(ICRS,ep=J2000,eq=2000): 22 32 24 +40 56.0 (Optical)\n".into())
        } else {
            panic!("Sesame must not be asked once SIMBAD answered")
        }
    };
    let mut failures = vec![];
    let (ra, dec, service) = lookup::find("LBN 437", &get, &mut failures).unwrap();
    assert_eq!(service, "SIMBAD (CfA mirror)");
    assert!((ra - 338.1).abs() < 1e-6 && (dec - 40.93333).abs() < 1e-4);
    assert_eq!(failures, vec!["NED: timeout".to_string()]);
}

#[test]
fn the_position_follows_the_owner_precedence() {
    let never = |_: &str| -> Result<String, String> { panic!("no lookup needed") };
    let manual = position(
        Some((10., 20.)),
        Some((30., 40.)),
        Some("M33"),
        Some(&never),
    )
    .unwrap();
    assert_eq!(manual.source, PositionSource::Manual);
    let header = position(None, Some((30., 40.)), Some("M33"), Some(&never)).unwrap();
    assert_eq!(header.source, PositionSource::Header);
    let table = position(None, None, Some("Messier 33"), Some(&never)).unwrap();
    assert_eq!(table.source, PositionSource::DeepSky("NGC 598".into()));
    let ned = |_: &str| -> Result<String, String> {
        Ok(r#"{"Preferred": {"Position": {"RA": 338.1, "Dec": 40.9}}}"#.into())
    };
    let online = position(None, None, Some("Simeis 147"), Some(&ned)).unwrap();
    assert_eq!(online.source, PositionSource::Online("NED".into()));
    let offline = position(None, None, Some("Simeis 147"), None).unwrap_err();
    assert!(
        offline
            .iter()
            .any(|n| n.contains("online lookup not allowed"))
    );
    let nothing = position(Some((400., 0.)), None, None, None).unwrap_err();
    assert!(nothing.iter().any(|n| n.contains("out of range")));
}

#[test]
fn the_installation_check_tells_complete_gaps_damage_and_missing_files() {
    let stars = [(23.0, 15.0, 12.5), (23.5, 15.5, 8.25)];
    // One readable file from the brightest stars on; a tiny file covers one
    // patch of sky only, so the sky probes report incomplete coverage.
    let one = tempfile::tempdir().unwrap();
    tiny_xpsd_file(one.path(), "gdr3-01.xpsd", (-2., 16.), &stars);
    let check = xpsd::check_installation(one.path(), true);
    assert_eq!(check.files.len(), 1);
    assert_eq!(check.files[0].sources, 2);
    assert_eq!(check.files[0].blocks_decoded, 1);
    assert!(check.files[0].problems.is_empty());
    assert_eq!(check.verdict, "incomplete");
    assert!(
        check
            .problems
            .iter()
            .all(|p| p.contains("sky is not covered"))
    );
    // A magnitude gap between two files.
    let gap = tempfile::tempdir().unwrap();
    tiny_xpsd_file(gap.path(), "gdr3-01.xpsd", (-2., 16.), &stars);
    tiny_xpsd_file(
        gap.path(),
        "gdr3-03.xpsd",
        (17., 18.),
        &[(23.0, 15.0, 17.5)],
    );
    let check = xpsd::check_installation(gap.path(), false);
    assert_eq!(check.verdict, "incomplete");
    assert!(
        check
            .problems
            .iter()
            .any(|p| p.contains("G 16.00..17.00 are missing"))
    );
    // A truncated file.
    let cut = tempfile::tempdir().unwrap();
    tiny_xpsd_file(cut.path(), "gdr3-01.xpsd", (-2., 16.), &stars);
    let path = cut.path().join("gdr3-01.xpsd");
    let bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, &bytes[..bytes.len() - 10]).unwrap();
    let check = xpsd::check_installation(cut.path(), false);
    assert_eq!(check.verdict, "damaged");
    assert!(
        check
            .problems
            .iter()
            .any(|p| p.contains("runs past the end"))
    );
    // Nothing there.
    let empty = tempfile::tempdir().unwrap();
    assert_eq!(
        xpsd::check_installation(empty.path(), false).verdict,
        "missing"
    );
}
