//! Explicit owner-requested artificial geometry-only final FITS; never an input write.
use fulgur_core::{Error, Result, check_cancelled};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path, sync::atomic::AtomicBool};

fn hash(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut h = Sha256::new();
    let mut bytes = vec![0u8; 1024 * 1024];
    loop {
        let n = file.read(&mut bytes)?;
        if n == 0 {
            break;
        }
        h.update(&bytes[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}
fn cubic(x: f64) -> f64 {
    let x = x.abs();
    if x <= 1. {
        (1.5 * x - 2.5) * x * x + 1.
    } else if x < 2. {
        ((-0.5 * x + 2.5) * x - 4.) * x + 2.
    } else {
        0.
    }
}
fn card(key: &str, value: &str) -> [u8; 80] {
    let mut bytes = [b' '; 80];
    let s = if key == "HISTORY" {
        format!("HISTORY {value}")
    } else {
        format!("{key:<8}= {value}")
    };
    let n = s.len().min(80);
    bytes[..n].copy_from_slice(&s.as_bytes()[..n]);
    bytes
}
fn run() -> Result<()> {
    if cfg!(debug_assertions) {
        return Err(Error::Invalid(
            "Use --release for artificial FITS generation.",
        ));
    }
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err(Error::Invalid(
            "Expected input FITS, new artificial FITS, new JSON report.",
        ));
    }
    let input = Path::new(&args[0]);
    let output = Path::new(&args[1]);
    let report = Path::new(&args[2]);
    fulgur_core::output::ensure_new_destination(output)?;
    fulgur_core::output::ensure_new_destination(report)?;
    if input == output || input == report || output == report {
        return Err(Error::Invalid("All paths must be distinct."));
    }
    let before = hash(input)?;
    let cancel = AtomicBool::new(false);
    let plane = fulgur_core::fits::read_reference_plane(input, &cancel)?;
    let center = [
        (plane.width as f64 - 1.) / 2.,
        (plane.height as f64 - 1.) / 2.,
    ];
    let radius2 = center[0] * center[0] + center[1] * center[1];
    let k = 0.03 / radius2;
    let width = ((plane.width - 1) as f64 * 1.03).ceil() as usize + 1;
    let height = ((plane.height - 1) as f64 * 1.03).ceil() as usize + 1;
    let out_center = [(width as f64 - 1.) / 2., (height as f64 - 1.) / 2.];
    let mut header = vec![
        card("SIMPLE", "T"),
        card("BITPIX", "-32"),
        card("NAXIS", "2"),
        card("NAXIS1", &width.to_string()),
        card("NAXIS2", &height.to_string()),
        card("ROWORDER", "'TOP-DOWN'"),
        card("SYNTHET", "T"),
        card("RADPCT", "3.0"),
        card("SRCW", &plane.width.to_string()),
        card("SRCH", &plane.height.to_string()),
        card(
            "HISTORY",
            "ARTIFICIAL geometry-only test; original reference green plane.",
        ),
        card(
            "HISTORY",
            "Outward radial mapping: r_out=r_in*(1+0.03*r_in^2/R_corner^2).",
        ),
        card(
            "HISTORY",
            "Corner radius increased 3 percent; expanded centered canvas.",
        ),
        card(
            "HISTORY",
            "Single cubic interpolation; no defocus or intensity normalization.",
        ),
    ];
    for (key, value) in [
        ("OBJECT", plane.object.as_ref()),
        ("FILTER", plane.filter.as_ref()),
    ] {
        if let Some(value) = value {
            header.push(card(key, &format!("'{}'", value.replace('\'', "''"))));
        }
    }
    let mut end = [b' '; 80];
    end[..3].copy_from_slice(b"END");
    header.push(end);
    let mut outside = 0u64;
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;
    fulgur_core::output::atomic_write(output, &cancel, |writer| {
        for c in &header {
            writer.write_all(c)?;
        }
        writer.write_all(&vec![b' '; (2880 - header.len() * 80 % 2880) % 2880])?;
        let mut row = vec![0u8; width * 4];
        for y in 0..height {
            check_cancelled(&cancel)?;
            for x in 0..width {
                let dx = x as f64 - out_center[0];
                let dy = y as f64 - out_center[1];
                let rd = dx.hypot(dy);
                let mut r = rd;
                for _ in 0..8 {
                    r -= (r * (1. + k * r * r) - rd) / (1. + 3. * k * r * r);
                }
                let factor = if rd == 0. { 1. } else { r / rd };
                let p = [center[0] + dx * factor, center[1] + dy * factor];
                let value = if p[0] < 0.
                    || p[1] < 0.
                    || p[0] > plane.width as f64 - 1.
                    || p[1] > plane.height as f64 - 1.
                {
                    outside += 1;
                    0.
                } else {
                    let ix = p[0].floor() as isize;
                    let iy = p[1].floor() as isize;
                    let mut v = 0.;
                    for j in -1..=2 {
                        for i in -1..=2 {
                            let sx = (ix + i).clamp(0, plane.width as isize - 1) as usize;
                            let sy = (iy + j).clamp(0, plane.height as isize - 1) as usize;
                            v += plane.pixels[sy * plane.width + sx] as f64
                                * cubic(p[0] - (ix + i) as f64)
                                * cubic(p[1] - (iy + j) as f64);
                        }
                    }
                    v as f32
                };
                if !value.is_finite() {
                    return Err(Error::Invalid("Nonfinite artificial reference sample."));
                }
                min = min.min(value);
                max = max.max(value);
                row[x * 4..x * 4 + 4].copy_from_slice(&value.to_be_bytes());
            }
            writer.write_all(&row)?;
        }
        writer.write_all(&vec![0u8; (2880 - width * height * 4 % 2880) % 2880])?;
        Ok(())
    })?;
    assert_eq!(before, hash(input)?);
    let evidence = serde_json::json!({"status":"PASS","artificial":true,"geometry_only":true,"input_sha256":before,"output_sha256":hash(output)?,"original_unchanged":true,"source_width":plane.width,"source_height":plane.height,"width":width,"height":height,"source_center":center,"output_center":out_center,"radial_coefficient_per_pixel_squared":k,"corner_radius_increase_fraction":0.03,"corner_displacement_px":0.03*radius2.sqrt(),"outside_source_pixels":outside,"output_min":min,"output_max":max,"units":"unchanged reference plane values; no min/max normalization or ADU divisor","pixel_rule":"single cubic interpolation of original reference green plane; no intentional blur or photometric calibration"});
    fulgur_core::output::atomic_write(report, &cancel, |writer| {
        serde_json::to_writer_pretty(writer, &evidence).map_err(|e| std::io::Error::other(e).into())
    })?;
    println!("Artificial reference {width} x {height}; corner radius +3%; source integrity PASS.");
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("FAIL: {error}");
        std::process::exit(1);
    }
}
