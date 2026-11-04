//! Separate experiment for FulgurStack (the stacker itself is used unchanged).
//!
//! `synthref solve` plate-solves one registration frame against Gaia DR3 and
//! renders a synthetic star reference on that frame's pixel grid, an ideal
//! TAN projection without optical distortion, saved as JSON and as a
//! monochrome 32-bit FITS. `synthref register` registers light sets against
//! any reference with FulgurStack's own registration and prints per-set counts.
mod catalog;
mod detect;
mod fits_out;
mod solve;

use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

const USAGE: &str = "usage:
  synthref solve --frame FRAME.fit --out DIR [--object NAME | --ra DEG --dec DEG] [--stars 1000]
  synthref register --reference REF.fit --report REPORT.json [--limits RMS,MAX,PERCENT,MIN] [--partial] (DIR|@LIST.txt)[=EXPOSURE_S]...
    --limits defaults to Maximum coverage (3,9,40,20); the application's default is 1,3,80,50.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("solve") => solve_command(&args[1..]),
        Some("register") => register_command(&args[1..]),
        _ => Err(USAGE.into()),
    };
    if let Err(e) = result {
        eprintln!("synthref: {e}");
        std::process::exit(1);
    }
}

fn opt<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn number(args: &[String], name: &str) -> Result<Option<f64>, String> {
    opt(args, name)
        .map(|v| v.parse::<f64>().map_err(|e| format!("{name}: {e}")))
        .transpose()
}

fn solve_command(args: &[String]) -> Result<(), String> {
    let start = Instant::now();
    let frame = PathBuf::from(opt(args, "--frame").ok_or(USAGE)?);
    let out = PathBuf::from(opt(args, "--out").ok_or(USAGE)?);
    let count = number(args, "--stars")?.unwrap_or(1000.) as usize;
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let (hint, hint_source) = match (number(args, "--ra")?, number(args, "--dec")?) {
        (Some(ra), Some(dec)) => ((ra, dec), "command line".to_string()),
        _ => {
            let object = opt(args, "--object").ok_or("give --object NAME or --ra/--dec")?;
            (catalog::resolve(object)?, format!("CDS Sesame: {object}"))
        }
    };
    eprintln!(
        "target hint RA {:.4} Dec {:+.4} ({hint_source})",
        hint.0, hint.1
    );
    let cancel = AtomicBool::new(false);
    let plane =
        fulgur_core::fits::read_reference_plane(&frame, &cancel).map_err(|e| e.to_string())?;
    let (width, height) = (plane.width, plane.height);
    let epoch = catalog::epoch_of(&frame).unwrap_or(catalog::GAIA_EPOCH);
    let detection = detect::detect(&plane.pixels, width, height, 500);
    drop(plane);
    eprintln!(
        "{width} x {height}: {} stars detected, median FWHM {:.2} px, epoch {epoch:.3}",
        detection.stars.len(),
        detection.fwhm_px
    );
    let plate = solve::solve(
        &detection.stars,
        width,
        height,
        hint,
        epoch,
        (count * 5).max(5000),
        &out,
    )?;
    let a = &plate.affine;
    let centre = catalog::deproject(
        a.invert([width as f64 / 2., height as f64 / 2.]),
        plate.ra0,
        plate.dec0,
    );
    eprintln!(
        "solved: centre RA {:.5} Dec {:+.5}, {:.4} arcsec/px, north {:.2} deg, {}, {} stars, linear RMS {:.3} px",
        centre.0,
        centre.1,
        a.arcsec_per_px(),
        a.north_angle_deg(),
        if a.mirrored() {
            "mirrored"
        } else {
            "not mirrored"
        },
        plate.matched,
        plate.rms_px
    );

    // The brightest catalogue stars inside the frame, on the linear solution.
    const MARGIN: f64 = 2.;
    let chosen: Vec<_> = plate
        .catalogue
        .iter()
        .filter_map(|s| plate.pixel(s).map(|p| (s, p)))
        .filter(|(_, p)| {
            p[0] >= MARGIN
                && p[1] >= MARGIN
                && p[0] <= width as f64 - 1. - MARGIN
                && p[1] <= height as f64 - 1. - MARGIN
        })
        .take(count)
        .collect();
    if chosen.len() < count {
        eprintln!(
            "warning: only {} catalogue stars fall inside the frame",
            chosen.len()
        );
    }
    let brightest = chosen.first().map_or(0., |c| c.0.g_mag);
    // Half the magnitude exponent: the order of brightness stays, the faintest
    // stars stay far above the noise.
    let peak = |g: f64| 0.8 * 10f64.powf(-0.2 * (g - brightest));
    let rendered: Vec<_> = chosen
        .iter()
        .map(|(s, p)| (p[0], p[1], peak(s.g_mag)))
        .collect();
    let pixels = fits_out::render(width, height, &rendered, detection.fwhm_px);

    // FITS WCS: 1-based pixels, rows counted from the bottom.
    let m = &a.m;
    let det = m[1] * m[5] - m[2] * m[4];
    let inv = [m[5] / det, -m[2] / det, -m[4] / det, m[1] / det];
    let wcs = [
        ("CRPIX1", m[0] + 1.),
        ("CRPIX2", height as f64 - m[3]),
        ("CRVAL1", plate.ra0),
        ("CRVAL2", plate.dec0),
        ("CD1_1", inv[0] / 3600.),
        ("CD1_2", -inv[1] / 3600.),
        ("CD2_1", inv[2] / 3600.),
        ("CD2_2", -inv[3] / 3600.),
    ];
    let name = frame
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut cards = vec![
        fits_out::text(
            "OBJECT",
            "synthetic Gaia DR3 reference",
            "geometry-only template",
        ),
        fits_out::text("CREATOR", "synthref 0.1", "FulgurStack experiment"),
        fits_out::text("CTYPE1", "RA---TAN", "gnomonic projection"),
        fits_out::text("CTYPE2", "DEC--TAN", "gnomonic projection"),
        fits_out::text("RADESYS", "ICRS", "Gaia DR3 frame"),
        fits_out::number("EQUINOX", 2000., "equinox of coordinates"),
        fits_out::number("EPOCH", epoch, "position epoch, decimal year"),
    ];
    for (k, v) in wcs {
        cards.push(fits_out::number(k, v, "linear plate solution"));
    }
    cards.push(fits_out::history(&format!(
        "Synthetic stars: {} brightest Gaia DR3 stars in the field of {name}",
        chosen.len()
    )));
    cards.push(fits_out::history(&format!(
        "Plate solution: {} stars, linear RMS {:.3} px; positions moved to epoch {epoch:.3}",
        plate.matched, plate.rms_px
    )));
    cards.push(fits_out::history(&format!(
        "Gaussian PSF FWHM {:.2} px; peak 0.8*10^(-0.2*(G-Gmin)); pedestal 0.001, noise 0.0005",
        detection.fwhm_px
    )));
    let fit_path = out.join("synthetic_reference.fit");
    fits_out::write(&fit_path, width, height, &pixels, &cards).map_err(|e| e.to_string())?;

    let report = json!({
        "tool": "synthref 0.1",
        "frame": name,
        "frame_width": width,
        "frame_height": height,
        "coordinates": "zero-based pixel centres, top-down rows, as FulgurStack reads a reference plane",
        "catalogue": {
            "service": "ESA Gaia archive TAP (https://gea.esac.esa.int/tap-server/tap/sync)",
            "table": "gaiadr3.gaia_source",
            "epoch": epoch,
            "proper_motion": "applied from J2016.0 to the frame's DATE-OBS",
        },
        "target_hint": { "source": hint_source, "ra_deg": hint.0, "dec_deg": hint.1 },
        "plate_solution": {
            "method": "blind triangle asterisms near the hint over a field-size ladder, both parities; linear TAN fit with 3-sigma clipping",
            "detected_stars": detection.stars.len(),
            "measured_fwhm_px": detection.fwhm_px,
            "candidate_agreeing_stars": plate.candidate_inliers,
            "candidate_half_field_deg": plate.candidate_half_field_deg,
            "tangent_ra_deg": plate.ra0,
            "tangent_dec_deg": plate.dec0,
            "centre_ra_deg": centre.0,
            "centre_dec_deg": centre.1,
            "arcsec_per_px": a.arcsec_per_px(),
            "field_deg": [a.arcsec_per_px() * width as f64 / 3600., a.arcsec_per_px() * height as f64 / 3600.],
            "north_deg_clockwise_from_top": a.north_angle_deg(),
            "mirrored": a.mirrored(),
            "matched_stars": plate.matched,
            "linear_rms_px": plate.rms_px,
            "linear_max_px": plate.max_px,
            "cubic_rms_px": plate.cubic_rms_px,
            "standard_to_pixel": a.m,
            "wcs": wcs.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<_, _>>(),
        },
        "synthetic": {
            "file": "synthetic_reference.fit",
            "stars": chosen.len(),
            "faintest_g_mag": chosen.last().map(|c| c.0.g_mag),
            "brightest_g_mag": brightest,
            "fwhm_px": detection.fwhm_px,
            "geometry": "ideal linear TAN projection of the solved field; the frame's optical distortion is not copied",
            "peak_rule": "0.8 * 10^(-0.2 * (G - Gmin)) on 0.001 pedestal with 0.0005 seeded Gaussian noise; BITPIX -32, 0..1",
        },
        "stars": chosen.iter().zip(&rendered).map(|((s, p), r)| json!({
            "source_id": s.source_id,
            "ra_deg": s.ra_deg,
            "dec_deg": s.dec_deg,
            "g_mag": s.g_mag,
            "x_px": p[0],
            "y_px": p[1],
            "peak": r.2,
        })).collect::<Vec<_>>(),
    });
    let json_path = out.join("synthetic_reference.json");
    let tmp = json_path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&report).unwrap()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &json_path).map_err(|e| e.to_string())?;
    eprintln!(
        "wrote {} and {} ({} stars) in {:.1} s",
        fit_path.display(),
        json_path.display(),
        chosen.len(),
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

fn register_command(args: &[String]) -> Result<(), String> {
    use fulgur_pipeline::inspect::{InputSet, InspectConfig};
    use fulgur_pipeline::register::{AccuracyLimits, RegistrationOptions};
    let reference = PathBuf::from(opt(args, "--reference").ok_or(USAGE)?);
    let report = PathBuf::from(opt(args, "--report").ok_or(USAGE)?);
    let accuracy = match opt(args, "--limits") {
        None => AccuracyLimits::MAXIMUM_COVERAGE,
        Some(text) => {
            let v: Vec<usize> = text
                .split(',')
                .map(|s| {
                    s.trim()
                        .parse::<usize>()
                        .map_err(|e| format!("--limits: {e}"))
                })
                .collect::<Result<_, _>>()?;
            let [rms, max, percent, min] = v[..] else {
                return Err("--limits takes RMS,MAX,PERCENT,MIN".into());
            };
            AccuracyLimits {
                rms_px: rms as u32,
                maximum_px: max as u32,
                star_percent: percent as u32,
                min_stars: min,
            }
        }
    };
    let partial = args.iter().any(|a| a == "--partial");
    let mut sets = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if ["--reference", "--report", "--limits"].contains(&args[i].as_str()) {
            i += 2;
            continue;
        }
        if args[i] == "--partial" {
            i += 1;
            continue;
        }
        let (dir, exposure) = match args[i].split_once('=') {
            Some((d, e)) => (d, Some(e.parse::<f64>().map_err(|e| e.to_string())?)),
            None => (args[i].as_str(), None),
        };
        // `@LIST.txt` makes one set of the files it names, one per line.
        let inputs = match dir.strip_prefix('@') {
            Some(list) => std::fs::read_to_string(list)
                .map_err(|e| format!("{list}: {e}"))?
                .lines()
                .map(|l| l.trim().trim_start_matches('\u{feff}'))
                .filter(|l| !l.is_empty())
                .map(PathBuf::from)
                .collect(),
            None => vec![PathBuf::from(dir)],
        };
        sets.push(InputSet {
            label: dir.rsplit(['\\', '/']).next().unwrap_or(dir).into(),
            inputs,
            layout: fulgur_core::LayoutChoice::Auto,
            exposure,
            accept_derived: true,
            calibration: None,
        });
        i += 1;
    }
    let cancel = AtomicBool::new(false);
    let start = Instant::now();
    let (inspection, root) =
        fulgur_pipeline::inspect::inspect_sets(&sets, InspectConfig::default(), &cancel, None)
            .map_err(|e| e.to_string())?;
    let inspected = start.elapsed().as_secs_f64();
    let (plan, _) = fulgur_pipeline::register::register_inspected_with_options(
        inspection,
        root,
        0.8,
        Some(&reference),
        RegistrationOptions {
            geometric_correction: true,
            accuracy,
            partial_frames: partial,
            ..Default::default()
        },
        &cancel,
        Instant::now(),
        None,
    )
    .map_err(|e| e.to_string())?;
    let value = serde_json::to_value(&plan).map_err(|e| e.to_string())?;
    eprintln!(
        "inspect {inspected:.1} s, register {:.1} s, stacked {}/{}",
        start.elapsed().as_secs_f64() - inspected,
        plan.accepted_count,
        plan.quality_accepted_count
    );
    let mut per: std::collections::BTreeMap<String, (usize, usize)> = Default::default();
    let empty = vec![];
    let frames = value["frames"].as_array().unwrap_or(&empty);
    let quality = value["inspection"]["frames"].as_array().unwrap_or(&empty);
    for (f, q) in frames.iter().zip(quality) {
        if q["accepted"] != json!(true) {
            continue;
        }
        let folder = f["file_name"]
            .as_str()
            .unwrap_or("")
            .split('/')
            .next()
            .unwrap_or("");
        let entry = per.entry(folder.to_string()).or_default();
        entry.1 += 1;
        entry.0 += usize::from(f["accepted"] == json!(true));
    }
    for (folder, (stacked, usable)) in per {
        eprintln!("  {folder:<44} {stacked:>4}/{usable:<4}");
    }
    std::fs::write(
        &report,
        serde_json::to_vec(&json!({ "registration": value })).unwrap(),
    )
    .map_err(|e| e.to_string())
}
