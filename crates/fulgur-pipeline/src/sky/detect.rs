//! Bright-star list for plate solving (plan phase 3). Inspection's own
//! catalogue leaves saturated stars out and picks stars per cell, which the
//! blind search cannot use (ADR-133, spike S2); this list keeps the brightest
//! detections, saturated ones included, with proper centroids.
//!
//! Local background mesh, maxima above 5 sigma with bright neighbours,
//! intensity-weighted centroids. A colour mosaic (declared, or recognised by
//! the alternation of neighbouring pixel levels) is measured on 2 x 2
//! superpixels. Coordinates are zero-based pixel centres of the full plane.

/// Stars kept, brightest first.
pub const MAX_STARS: usize = 1500;

/// One detection: x, y (zero-based pixel centres of the full plane), FWHM
/// (px), eccentricity, flux above the local background (ADU summed over a
/// 9 x 9 box of the measured plane) and the highest raw pixel within 2 px.
pub type SkyStar = [f64; 6];

fn median(v: &mut [f32]) -> f64 {
    let m = v.len() / 2;
    v.select_nth_unstable_by(m, f32::total_cmp);
    v[m] as f64
}

/// A colour mosaic alternates channel levels between neighbouring pixels in
/// both directions; a monochrome sensor shows no alternation. An odd sample
/// step visits both pixel parities.
pub fn looks_like_mosaic(p: &[f32], w: usize, h: usize) -> bool {
    if w < 16 || h < 16 {
        return false;
    }
    let (x0, x1) = ((w / 4) & !1, (3 * w / 4) & !1);
    let (y0, y1) = ((h / 4) & !1, (3 * h / 4) & !1);
    let (mut even_x, mut odd_x, mut even_y, mut odd_y, mut same) =
        (vec![], vec![], vec![], vec![], vec![]);
    for y in (y0..y1).step_by(7) {
        for x in (x0..x1).step_by(7) {
            let v = p[y * w + x];
            let dx = v - p[y * w + x + 1];
            let dy = v - p[(y + 1) * w + x];
            if x % 2 == 0 { &mut even_x } else { &mut odd_x }.push(dx);
            if y % 2 == 0 { &mut even_y } else { &mut odd_y }.push(dy);
            same.push((v - p[y * w + x + 2]).abs());
        }
    }
    let sigma = (median(&mut same) * 1.4826 / std::f64::consts::SQRT_2).max(1e-9);
    let sx = (median(&mut even_x) - median(&mut odd_x)).abs() / 2. / sigma;
    let sy = (median(&mut even_y) - median(&mut odd_y)).abs() / 2. / sigma;
    sx > 0.25 && sy > 0.25
}

/// The brightest detections ([`SkyStar`]) and whether the plane was measured
/// as a colour mosaic.
pub fn bright_stars(
    pixels: &[f32],
    width: usize,
    height: usize,
    declared_mosaic: bool,
) -> (Vec<SkyStar>, bool) {
    let mosaic = declared_mosaic || looks_like_mosaic(pixels, width, height);
    let peak = |s: &mut SkyStar| {
        let (cx, cy) = (s[0].round() as isize, s[1].round() as isize);
        let mut top = f32::NEG_INFINITY;
        for y in (cy - 2).max(0)..=(cy + 2).min(height as isize - 1) {
            for x in (cx - 2).max(0)..=(cx + 2).min(width as isize - 1) {
                top = top.max(pixels[y as usize * width + x as usize]);
            }
        }
        s[5] = top as f64;
    };
    if !mosaic {
        let mut stars = detect(pixels, width, height);
        stars.iter_mut().for_each(peak);
        return (stars, false);
    }
    let (bw, bh) = (width / 2, height / 2);
    let mut binned = vec![0f32; bw * bh];
    for y in 0..bh {
        for x in 0..bw {
            let i = 2 * y * width + 2 * x;
            binned[y * bw + x] =
                pixels[i] + pixels[i + 1] + pixels[i + width] + pixels[i + width + 1];
        }
    }
    let stars = detect(&binned, bw, bh)
        .into_iter()
        .map(|s| {
            let mut star = [2. * s[0] + 0.5, 2. * s[1] + 0.5, 2. * s[2], s[3], s[4], 0.];
            peak(&mut star);
            star
        })
        .collect();
    (stars, true)
}

fn detect(p: &[f32], w: usize, h: usize) -> Vec<SkyStar> {
    const TILE: usize = 64;
    const EDGE: usize = 8;
    if w <= 2 * EDGE || h <= 2 * EDGE {
        return vec![];
    }
    let (tw, th) = (w.div_ceil(TILE), h.div_ceil(TILE));
    let mut level = vec![0.; tw * th];
    let mut sigma = vec![0f32; tw * th];
    let mut buf = Vec::with_capacity(TILE * TILE / 16);
    for ty in 0..th {
        for tx in 0..tw {
            // Every fourth pixel of every fourth row: 256 samples a tile
            // place the level within a tenth of the noise.
            buf.clear();
            for y in (ty * TILE..((ty + 1) * TILE).min(h)).step_by(4) {
                buf.extend(
                    p[y * w + tx * TILE..y * w + ((tx + 1) * TILE).min(w)]
                        .iter()
                        .step_by(4)
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
    let threshold = (5. * noise) as f32;
    let xs: Vec<(usize, usize, f32)> = (0..w)
        .map(|x| {
            let fx = ((x as f64 + 0.5) / TILE as f64 - 0.5).clamp(0., (tw - 1) as f64);
            let x0 = fx.floor() as usize;
            (x0, (x0 + 1).min(tw - 1), (fx - fx.floor()) as f32)
        })
        .collect();
    // The interpolated background never falls below the lowest of the four
    // tile levels around a pixel; that bound only changes every TILE rows
    // and rejects nearly every pixel before the background is interpolated.
    let mut floor = vec![0f32; w];
    let mut floor_rows = (usize::MAX, usize::MAX);
    let mut stars = vec![];
    for y in EDGE..h - EDGE {
        let fy = ((y as f64 + 0.5) / TILE as f64 - 0.5).clamp(0., (th - 1) as f64);
        let (y0, ay) = (fy.floor() as usize, (fy - fy.floor()) as f32);
        let y1 = (y0 + 1).min(th - 1);
        if floor_rows != (y0, y1) {
            floor_rows = (y0, y1);
            for (f, &(x0, x1, _)) in floor.iter_mut().zip(&xs) {
                let low = [
                    level[y0 * tw + x0],
                    level[y0 * tw + x1],
                    level[y1 * tw + x0],
                    level[y1 * tw + x1],
                ]
                .into_iter()
                .fold(f64::INFINITY, f64::min);
                *f = low as f32 + threshold;
            }
        }
        let row = &p[y * w..(y + 1) * w];
        for x in EDGE..w - EDGE {
            if row[x] < floor[x] {
                continue;
            }
            let (x0, x1, ax) = xs[x];
            let top = level[y0 * tw + x0] as f32 * (1. - ax) + level[y0 * tw + x1] as f32 * ax;
            let bottom = level[y1 * tw + x0] as f32 * (1. - ax) + level[y1 * tw + x1] as f32 * ax;
            let background = top * (1. - ay) + bottom * ay;
            if row[x] - background < threshold {
                continue;
            }
            // The background is smooth over a star: the candidate's serves.
            let bg = background as f64;
            let at = |x: isize, y: isize| p[y as usize * w + x as usize] as f64 - bg;
            let (xi, yi) = (x as isize, y as isize);
            let v = at(xi, yi);
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
            // A single hot pixel has no bright neighbours.
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
            stars.push((
                sw,
                [
                    x as f64 + cx,
                    y as f64 + cy,
                    2.3548 * ((mxx + myy) / 2.).sqrt(),
                    if major > 0. {
                        (1. - minor / major).sqrt()
                    } else {
                        0.
                    },
                    sw,
                    0.,
                ],
            ));
        }
    }
    stars.sort_by(|a, b| b.0.total_cmp(&a.0));
    stars.truncate(MAX_STARS);
    stars.into_iter().map(|s| s.1).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gaussian stars on a flat, noisy background; brightest first, with
    /// sub-pixel centroids.
    #[test]
    fn stars_are_found_brightest_first_with_subpixel_centroids() {
        let (w, h) = (400usize, 300usize);
        let mut p = vec![0f32; w * h];
        let mut state = 0x1234_5678u64;
        for v in &mut p {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *v = 1000. + ((state >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 20.;
        }
        let truth = [
            (100.3, 80.7, 5000.),
            (250.6, 150.2, 2000.),
            (320.1, 220.9, 800.),
        ];
        for &(x0, y0, a) in &truth {
            for y in 0..h {
                for x in 0..w {
                    let r2 = (x as f64 - x0).powi(2) + (y as f64 - y0).powi(2);
                    p[y * w + x] += (a * (-r2 / (2. * 1.5f64.powi(2))).exp()) as f32;
                }
            }
        }
        let (stars, mosaic) = bright_stars(&p, w, h, false);
        assert!(!mosaic);
        assert_eq!(stars.len(), 3);
        for (s, t) in stars.iter().zip(&truth) {
            assert!(
                (s[0] - t.0).abs() < 0.05 && (s[1] - t.1).abs() < 0.05,
                "{s:?} vs {t:?}"
            );
            assert!((s[2] - 2.3548 * 1.5).abs() < 0.5 && s[3] < 0.3);
            // Flux of the Gaussian within the 9 x 9 box, and the raw peak.
            let flux = t.2 * std::f64::consts::TAU * 1.5 * 1.5;
            assert!((s[4] / flux - 1.).abs() < 0.02, "{} vs {flux}", s[4]);
            assert!((s[5] - 1000. - t.2).abs() < 0.1 * t.2, "{}", s[5]);
        }
    }
}
