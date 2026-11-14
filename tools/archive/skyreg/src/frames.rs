//! Frame input: header facts, pixel plane, colour-mosaic test and star list.
//! Pixels are read once into memory and dropped after detection; nothing is
//! written. Coordinates are zero-based pixel centres of the full-resolution
//! plane, rows as FulgurStack's reader delivers them.
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

pub const MAX_STARS: usize = 1500;

#[derive(Clone, Debug, Serialize)]
pub struct Star {
    pub x: f64,
    pub y: f64,
    pub flux: f64,
    pub fwhm: f64,
    /// Elongation from second moments: 0 round, towards 1 a line.
    pub ecc: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Frame {
    #[serde(skip)]
    pub path: PathBuf,
    pub name: String,
    pub format: &'static str,
    pub reader: &'static str,
    pub width: usize,
    pub height: usize,
    pub instrument: String,
    pub filter: String,
    pub exposure_s: Option<f64>,
    pub epoch: f64,
    pub epoch_source: &'static str,
    /// Colour mosaic detected: stars are found on 2x2 superpixels.
    pub mosaic: bool,
    pub mosaic_source: String,
    /// RA/Dec cards of the header, used only as an extra search hint.
    #[serde(skip)]
    pub header_hint: Option<(f64, f64)>,
    #[serde(skip)]
    pub stars: Vec<Star>,
    pub detected: usize,
    pub fwhm_px: f64,
    pub read_s: f64,
    pub detect_s: f64,
}

pub fn is_image(path: &Path) -> bool {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    matches!(ext.as_str(), "fit" | "fits" | "fts" | "xisf")
}

/// Header cards of a FITS primary header or the FITS keywords of an XISF
/// header. Site coordinates are skipped and never kept.
fn cards(path: &Path) -> Result<(Vec<(String, String)>, bool), String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut out = vec![];
    let xisf = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("xisf"));
    if xisf {
        let mut head = [0u8; 16];
        file.read_exact(&mut head).map_err(|e| e.to_string())?;
        let len = u32::from_le_bytes(head[8..12].try_into().unwrap()) as usize;
        let mut xml = vec![0u8; len];
        file.read_exact(&mut xml).map_err(|e| e.to_string())?;
        let xml = String::from_utf8_lossy(&xml);
        let mut rest = xml.as_ref();
        while let Some(i) = rest.find("<FITSKeyword name=\"") {
            rest = &rest[i + 19..];
            let Some(end) = rest.find('"') else { break };
            let key = rest[..end].to_string();
            let value = rest
                .find("value=\"")
                .map(|v| {
                    let s = &rest[v + 7..];
                    s[..s.find('"').unwrap_or(0)].to_string()
                })
                .unwrap_or_default();
            out.push((key, value));
        }
        return Ok((out, xml.contains("ColorFilterArray")));
    }
    let mut block = [0u8; 2880];
    'blocks: for _ in 0..64 {
        file.read_exact(&mut block).map_err(|e| e.to_string())?;
        for card in block.chunks(80) {
            let text = String::from_utf8_lossy(card);
            let key = text[..8].trim().to_string();
            if key == "END" {
                break 'blocks;
            }
            if text.get(8..10) == Some("= ") {
                let value = text[10..].split('/').next().unwrap_or("").to_string();
                out.push((key, value));
            }
        }
    }
    Ok((out, false))
}

fn value(cards: &[(String, String)], key: &str) -> Option<String> {
    cards
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.trim().trim_matches('\'').trim().to_string())
        .filter(|v| !v.is_empty())
}

fn decimal_year(text: &str) -> Option<f64> {
    let digits: String = text.chars().filter(char::is_ascii_digit).collect();
    let year: f64 = digits.get(0..4)?.parse().ok()?;
    let month: usize = digits.get(4..6)?.parse().ok()?;
    let day: f64 = digits.get(6..8)?.parse().ok()?;
    if !(1990. ..2100.).contains(&year) || !(1..=12).contains(&month) || !(1. ..=31.).contains(&day)
    {
        return None;
    }
    const BEFORE: [f64; 12] = [
        0., 31., 59., 90., 120., 151., 181., 212., 243., 273., 304., 334.,
    ];
    Some(year + (BEFORE[month - 1] + day - 0.5) / 365.25)
}

/// A date in a file name: YYYY-MM-DD, YYYY_MM_DD or YYYYMMDD.
fn name_date(name: &str) -> Option<f64> {
    let b = name.as_bytes();
    for i in 0..b.len() {
        for len in [10, 8] {
            let Some(s) = name.get(i..i + len) else {
                continue;
            };
            let ok = if len == 10 {
                s.as_bytes()[4] == s.as_bytes()[7]
                    && matches!(s.as_bytes()[4], b'-' | b'_')
                    && s.chars()
                        .enumerate()
                        .all(|(k, c)| k == 4 || k == 7 || c.is_ascii_digit())
            } else {
                s.chars().all(|c| c.is_ascii_digit())
                    && (i == 0 || !b[i - 1].is_ascii_digit())
                    && b.get(i + 8).is_none_or(|c| !c.is_ascii_digit())
            };
            if ok {
                if let Some(y) = decimal_year(s) {
                    return Some(y);
                }
            }
        }
    }
    None
}

/// A colour mosaic alternates channel levels between neighbouring pixels in
/// both directions. Adjacent-pixel differences then have medians of opposite
/// sign on even and odd positions; a monochrome sensor shows none.
fn mosaic_test(p: &[f32], w: usize, h: usize) -> (bool, f64, f64) {
    let (x0, x1) = ((w / 4) & !1, (3 * w / 4) & !1);
    let (y0, y1) = ((h / 4) & !1, (3 * h / 4) & !1);
    let mut even_x = vec![];
    let mut odd_x = vec![];
    let mut even_y = vec![];
    let mut odd_y = vec![];
    let mut same = vec![];
    for y in (y0..y1).step_by(3) {
        for x in (x0..x1).step_by(3) {
            let v = p[y * w + x];
            let dx = v - p[y * w + x + 1];
            let dy = v - p[(y + 1) * w + x];
            if x % 2 == 0 { &mut even_x } else { &mut odd_x }.push(dx);
            if y % 2 == 0 { &mut even_y } else { &mut odd_y }.push(dy);
            same.push((v - p[y * w + x + 2]).abs());
        }
    }
    let med = |v: &mut Vec<f32>| {
        let m = v.len() / 2;
        v.select_nth_unstable_by(m, f32::total_cmp);
        v[m] as f64
    };
    let sigma = (med(&mut same) * 1.4826 / std::f64::consts::SQRT_2).max(1e-9);
    let sx = (med(&mut even_x) - med(&mut odd_x)).abs() / 2. / sigma;
    let sy = (med(&mut even_y) - med(&mut odd_y)).abs() / 2. / sigma;
    (sx > 0.25 && sy > 0.25, sx, sy)
}

fn median(v: &mut [f32]) -> f64 {
    let m = v.len() / 2;
    v.select_nth_unstable_by(m, f32::total_cmp);
    v[m] as f64
}

/// Local background mesh, maxima above 5 sigma with bright neighbours,
/// intensity-weighted centroids; brightest first.
fn detect(p: &[f32], w: usize, h: usize, max: usize) -> (Vec<Star>, f64) {
    const TILE: usize = 64;
    let (tw, th) = (w.div_ceil(TILE), h.div_ceil(TILE));
    let mut level = vec![0.; tw * th];
    let mut sigma = vec![0f32; tw * th];
    let mut buf = Vec::with_capacity(TILE * TILE);
    for ty in 0..th {
        for tx in 0..tw {
            // Every second pixel of every second row: a quarter of the work
            // for the same median within its noise.
            buf.clear();
            for y in (ty * TILE..((ty + 1) * TILE).min(h)).step_by(2) {
                buf.extend(
                    p[y * w + tx * TILE..y * w + ((tx + 1) * TILE).min(w)]
                        .iter()
                        .step_by(2)
                        .copied(),
                );
            }
            let m = median(&mut buf);
            buf.iter_mut()
                .for_each(|v| *v = (*v as f64 - m).abs() as f32);
            level[ty * tw + tx] = m;
            sigma[ty * tw + tx] = (median(&mut buf) * 1.4826) as f32;
        }
    }
    let noise = median(&mut sigma.clone()).max(1e-6);
    let mut stars = vec![];
    const EDGE: usize = 8;
    if w <= 2 * EDGE || h <= 2 * EDGE {
        return (stars, 3.);
    }
    // Background of one row at a time; no full background-free image. The
    // background is smooth over a star, so a candidate's own value serves
    // its neighbourhood.
    let mut row_bg = vec![0f32; w];
    let xs: Vec<(usize, usize, f32)> = (0..w)
        .map(|x| {
            let fx = ((x as f64 + 0.5) / TILE as f64 - 0.5).clamp(0., (tw - 1) as f64);
            let x0 = fx.floor() as usize;
            (x0, (x0 + 1).min(tw - 1), (fx - fx.floor()) as f32)
        })
        .collect();
    for y in EDGE..h - EDGE {
        let fy = ((y as f64 + 0.5) / TILE as f64 - 0.5).clamp(0., (th - 1) as f64);
        let (y0, ay) = (fy.floor() as usize, (fy - fy.floor()) as f32);
        let y1 = (y0 + 1).min(th - 1);
        for (x, &(x0, x1, ax)) in xs.iter().enumerate() {
            let top = level[y0 * tw + x0] as f32 * (1. - ax) + level[y0 * tw + x1] as f32 * ax;
            let bottom = level[y1 * tw + x0] as f32 * (1. - ax) + level[y1 * tw + x1] as f32 * ax;
            row_bg[x] = top * (1. - ay) + bottom * ay;
        }
        let threshold = (5. * noise) as f32;
        for x in EDGE..w - EDGE {
            if p[y * w + x] - row_bg[x] < threshold {
                continue;
            }
            let bg = row_bg[x] as f64;
            let at = |x: isize, y: isize| p[y as usize * w + x as usize] as f64 - bg;
            let v = at(x as isize, y as isize);
            let (xi, yi) = (x as isize, y as isize);
            let mut peak = true;
            let mut above = 0;
            'scan: for dy in -2..=2isize {
                for dx in -2..=2isize {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let n = at(xi + dx, yi + dy);
                    if n > v || (n == v && (dy < 0 || (dy == 0 && dx < 0))) {
                        peak = false;
                        break 'scan;
                    }
                    if dx.abs() <= 1 && dy.abs() <= 1 && n > 3. * noise {
                        above += 1;
                    }
                }
            }
            if !peak || above < 4 {
                continue;
            }
            let (mut sw, mut sx, mut sy) = (0., 0., 0.);
            for dy in -4..=4isize {
                for dx in -4..=4isize {
                    let v = at(xi + dx, yi + dy).max(0.);
                    sw += v;
                    sx += v * dx as f64;
                    sy += v * dy as f64;
                }
            }
            let (cx, cy) = (sx / sw, sy / sw);
            let (mut mxx, mut myy, mut mxy) = (0., 0., 0.);
            for dy in -4..=4isize {
                for dx in -4..=4isize {
                    let v = at(xi + dx, yi + dy).max(0.);
                    let (ux, uy) = (dx as f64 - cx, dy as f64 - cy);
                    mxx += v * ux * ux;
                    myy += v * uy * uy;
                    mxy += v * ux * uy;
                }
            }
            let (mxx, myy, mxy) = (mxx / sw, myy / sw, mxy / sw);
            let root = ((mxx - myy).powi(2) / 4. + mxy * mxy).sqrt();
            let (major, minor) = ((mxx + myy) / 2. + root, ((mxx + myy) / 2. - root).max(0.));
            stars.push(Star {
                x: x as f64 + cx,
                y: y as f64 + cy,
                flux: sw,
                fwhm: 2.3548 * ((mxx + myy) / 2.).sqrt(),
                ecc: if major > 0. {
                    (1. - minor / major).sqrt()
                } else {
                    0.
                },
            });
        }
    }
    stars.sort_by(|a, b| b.flux.total_cmp(&a.flux));
    stars.truncate(max);
    let mut widths: Vec<f32> = stars.iter().take(150).map(|s| s.fwhm as f32).collect();
    let fwhm = if widths.is_empty() {
        3.
    } else {
        median(&mut widths)
    };
    (stars, fwhm)
}

pub fn load(path: &Path) -> Result<Frame, String> {
    let start = Instant::now();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (cards, xisf_cfa) = cards(path)?;
    // FulgurStack's light-frame reader first (16-bit FITS and XISF, decoded
    // in one pass); the geometry-only reference reader for anything else,
    // such as 32-bit float templates. Each frame's solution lives in the
    // pixel order its reader delivers, so the two may differ in row order.
    struct Plane {
        width: usize,
        height: usize,
        pixels: Vec<f32>,
    }
    let (plane, reader) = match fulgur_core::fits::read_raw_sequential(path) {
        Ok(raw) => (
            Plane {
                width: raw.header.info.width,
                height: raw.header.info.height,
                pixels: raw.pixels,
            },
            "light reader",
        ),
        Err(_) => {
            let p = fulgur_core::fits::read_reference_plane(path, &AtomicBool::new(false))
                .map_err(|e| e.to_string())?;
            (
                Plane {
                    width: p.width,
                    height: p.height,
                    pixels: p.pixels,
                },
                "reference reader",
            )
        }
    };
    let read_s = start.elapsed().as_secs_f64();
    let (w, h) = (plane.width, plane.height);
    let bayer = value(&cards, "BAYERPAT").or_else(|| value(&cards, "COLORTYP"));
    let (mosaic, mosaic_source) = match (&bayer, xisf_cfa) {
        (Some(b), _) => (true, format!("header BAYERPAT {b}")),
        (None, true) => (true, "XISF ColorFilterArray".to_string()),
        (None, false) => {
            let (m, sx, sy) = mosaic_test(&plane.pixels, w, h);
            (m, format!("pixel test (alternation {sx:.2}/{sy:.2} sigma)"))
        }
    };
    let t = Instant::now();
    let (stars, fwhm) = if mosaic {
        let (bw, bh) = (w / 2, h / 2);
        let mut binned = vec![0f32; bw * bh];
        for y in 0..bh {
            for x in 0..bw {
                let p = &plane.pixels;
                binned[y * bw + x] = p[2 * y * w + 2 * x]
                    + p[2 * y * w + 2 * x + 1]
                    + p[(2 * y + 1) * w + 2 * x]
                    + p[(2 * y + 1) * w + 2 * x + 1];
            }
        }
        drop(plane);
        let (mut stars, fwhm) = detect(&binned, bw, bh, MAX_STARS);
        for s in &mut stars {
            s.x = 2. * s.x + 0.5;
            s.y = 2. * s.y + 0.5;
            s.fwhm *= 2.;
        }
        (stars, fwhm * 2.)
    } else {
        let result = detect(&plane.pixels, w, h, MAX_STARS);
        drop(plane);
        result
    };
    let detect_s = t.elapsed().as_secs_f64();
    let (epoch, epoch_source) = match value(&cards, "DATE-OBS").and_then(|d| decimal_year(&d)) {
        Some(e) => (e, "DATE-OBS"),
        None => match name_date(&name) {
            Some(e) => (e, "file name"),
            None => (crate::catalog::GAIA_EPOCH, "none (Gaia epoch)"),
        },
    };
    let num = |k: &str| value(&cards, k).and_then(|v| v.parse::<f64>().ok());
    Ok(Frame {
        path: path.to_path_buf(),
        format: if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("xisf"))
        {
            "XISF"
        } else {
            "FITS"
        },
        reader,
        width: w,
        height: h,
        instrument: value(&cards, "INSTRUME").unwrap_or_else(|| "unknown".into()),
        filter: value(&cards, "FILTER").unwrap_or_else(|| "-".into()),
        exposure_s: num("EXPTIME").or_else(|| num("EXPOSURE")),
        epoch,
        epoch_source,
        mosaic,
        mosaic_source,
        header_hint: num("RA")
            .zip(num("DEC"))
            .filter(|(r, d)| (0. ..360.).contains(r) && (-90. ..=90.).contains(d)),
        detected: stars.len(),
        stars,
        fwhm_px: fwhm,
        read_s,
        detect_s,
        name,
    })
}
