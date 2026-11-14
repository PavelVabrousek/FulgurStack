//! Rendering of the synthetic star field and a minimal FITS writer
//! (monochrome, BITPIX = -32, standard bottom-up row order, atomic `.tmp` rename).
use std::io::Write;
use std::path::Path;

/// Gaussian stars on a faint pedestal with a little seeded noise, so star
/// detectors find a finite noise level. Positions are zero-based top-down
/// pixel centres; `stars` holds (x, y, peak).
pub fn render(width: usize, height: usize, stars: &[(f64, f64, f64)], fwhm: f64) -> Vec<f32> {
    const PEDESTAL: f64 = 0.001;
    const NOISE: f64 = 0.0005;
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut uniform = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        ((state >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    };
    let mut pixels: Vec<f32> = (0..width * height)
        .map(|_| {
            let (u, v) = (uniform(), uniform());
            let gauss = (-2. * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos();
            (PEDESTAL + NOISE * gauss) as f32
        })
        .collect();
    let sigma = fwhm / 2.354_820_045;
    let reach = (4. * sigma).ceil() as isize + 1;
    for &(x, y, peak) in stars {
        let (cx, cy) = (x.round() as isize, y.round() as isize);
        for py in (cy - reach).max(0)..=(cy + reach).min(height as isize - 1) {
            for px in (cx - reach).max(0)..=(cx + reach).min(width as isize - 1) {
                let r2 = (px as f64 - x).powi(2) + (py as f64 - y).powi(2);
                pixels[py as usize * width + px as usize] +=
                    (peak * (-r2 / (2. * sigma * sigma)).exp()) as f32;
            }
        }
    }
    for p in &mut pixels {
        *p = p.clamp(0., 1.);
    }
    pixels
}

pub fn card(key: &str, value: &str, comment: &str) -> String {
    let mut s = format!("{key:<8}= {value:>20} / {comment}");
    s.truncate(80);
    s
}

pub fn text(key: &str, value: &str, comment: &str) -> String {
    let v = format!("'{:<8}'", value.replace('\'', "''"));
    let mut s = format!("{key:<8}= {v:<20} / {comment}");
    s.truncate(80);
    s
}

pub fn number(key: &str, value: f64, comment: &str) -> String {
    card(key, &format!("{value:.12E}"), comment)
}

pub fn history(line: &str) -> String {
    let mut s = format!("HISTORY {line}");
    s.truncate(80);
    s
}

/// `pixels` are top-down; FITS stores the bottom row first.
pub fn write(
    path: &Path,
    width: usize,
    height: usize,
    pixels: &[f32],
    extra: &[String],
) -> std::io::Result<()> {
    let mut cards = vec![
        card("SIMPLE", "T", "conforms to FITS standard"),
        card("BITPIX", "-32", "32-bit IEEE float"),
        card("NAXIS", "2", "monochrome image"),
        card("NAXIS1", &width.to_string(), "width"),
        card("NAXIS2", &height.to_string(), "height"),
    ];
    cards.extend(extra.iter().cloned());
    cards.push("END".into());
    let mut header = String::new();
    for c in &cards {
        header.push_str(&format!("{c:<80}"));
    }
    while header.len() % 2880 != 0 {
        header.push(' ');
    }
    let tmp = path.with_extension("fit.tmp");
    {
        let mut out = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
        out.write_all(header.as_bytes())?;
        for y in (0..height).rev() {
            for &v in &pixels[y * width..(y + 1) * width] {
                out.write_all(&v.to_be_bytes())?;
            }
        }
        let data = width * height * 4;
        out.write_all(&vec![0u8; (2880 - data % 2880) % 2880])?;
        out.flush()?;
    }
    std::fs::rename(&tmp, path)
}
