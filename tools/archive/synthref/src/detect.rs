//! Star detection for plate solving: a local background mesh, local maxima
//! above 5 sigma, intensity-weighted centroids. Coordinates are zero-based
//! pixel centres, top-down, as FulgurStack reads a reference plane.

pub struct Star {
    pub x: f64,
    pub y: f64,
    pub flux: f64,
    pub fwhm: f64,
}

pub struct Detection {
    pub stars: Vec<Star>,
    pub fwhm_px: f64,
}

fn median(v: &mut [f32]) -> f64 {
    let mid = v.len() / 2;
    v.select_nth_unstable_by(mid, f32::total_cmp);
    v[mid] as f64
}

pub fn detect(p: &[f32], w: usize, h: usize, max: usize) -> Detection {
    const TILE: usize = 64;
    let (tw, th) = (w.div_ceil(TILE), h.div_ceil(TILE));
    let mut level = vec![0.; tw * th];
    let mut sigma = vec![0.; tw * th];
    let mut buf = Vec::with_capacity(TILE * TILE);
    for ty in 0..th {
        for tx in 0..tw {
            buf.clear();
            for y in ty * TILE..((ty + 1) * TILE).min(h) {
                buf.extend_from_slice(&p[y * w + tx * TILE..y * w + ((tx + 1) * TILE).min(w)]);
            }
            let m = median(&mut buf);
            buf.iter_mut()
                .for_each(|v| *v = (*v as f64 - m).abs() as f32);
            level[ty * tw + tx] = m;
            sigma[ty * tw + tx] = median(&mut buf) * 1.4826;
        }
    }
    let mut sorted: Vec<f32> = sigma.iter().map(|&s| s as f32).collect();
    let noise = median(&mut sorted).max(1e-6);
    // Bilinear background between tile centres.
    let back = |x: usize, y: usize| {
        let fx = ((x as f64 + 0.5) / TILE as f64 - 0.5).clamp(0., (tw - 1) as f64);
        let fy = ((y as f64 + 0.5) / TILE as f64 - 0.5).clamp(0., (th - 1) as f64);
        let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(tw - 1), (y0 + 1).min(th - 1));
        let (ax, ay) = (fx - x0 as f64, fy - y0 as f64);
        let top = level[y0 * tw + x0] * (1. - ax) + level[y0 * tw + x1] * ax;
        let bottom = level[y1 * tw + x0] * (1. - ax) + level[y1 * tw + x1] * ax;
        top * (1. - ay) + bottom * ay
    };
    let r: Vec<f32> = (0..w * h)
        .map(|i| (p[i] as f64 - back(i % w, i / w)) as f32)
        .collect();
    let at = |x: isize, y: isize| r[y as usize * w + x as usize] as f64;
    let mut stars = Vec::new();
    const EDGE: usize = 8;
    for y in EDGE..h - EDGE {
        for x in EDGE..w - EDGE {
            let v = r[y * w + x] as f64;
            if v < 5. * noise {
                continue;
            }
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
            let mut m2 = 0.;
            for dy in -4..=4isize {
                for dx in -4..=4isize {
                    let v = at(xi + dx, yi + dy).max(0.);
                    m2 += v * ((dx as f64 - cx).powi(2) + (dy as f64 - cy).powi(2));
                }
            }
            stars.push(Star {
                x: x as f64 + cx,
                y: y as f64 + cy,
                flux: sw,
                fwhm: 2.3548 * (m2 / sw / 2.).sqrt(),
            });
        }
    }
    stars.sort_by(|a, b| b.flux.total_cmp(&a.flux));
    stars.truncate(max);
    let mut widths: Vec<f32> = stars.iter().take(150).map(|s| s.fwhm as f32).collect();
    let fwhm_px = if widths.is_empty() {
        3.
    } else {
        median(&mut widths).clamp(1.5, 10.)
    };
    Detection { stars, fwhm_px }
}
