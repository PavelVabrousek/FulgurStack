//! Green-CFA photometry; no RGB reconstruction or intermediate images.

use fulgur_core::{BayerPattern, Color, RawFrame, Result, check_cancelled};
use serde::Serialize;
use std::sync::atomic::AtomicBool;

pub(crate) fn median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mid = values.len() / 2;
    let even = mid * 2 == values.len();
    let (lower, center, _) = values.select_nth_unstable_by(mid, f64::total_cmp);
    if even {
        (lower.iter().copied().fold(f64::NEG_INFINITY, f64::max) + *center) * 0.5
    } else {
        *center
    }
}

fn location_noise(values: &mut Vec<f64>) -> (f64, f64) {
    let mut center = median(values);
    let mut residuals: Vec<_> = values.iter().map(|v| (v - center).abs()).collect();
    let mut noise = 1.4826 * median(&mut residuals);
    if noise > 0.0 {
        values.retain(|v| (v - center).abs() <= 3.0 * noise);
        center = median(values);
        residuals.clear();
        residuals.extend(values.iter().map(|v| (v - center).abs()));
        noise = 1.4826 * median(&mut residuals);
    }
    (center, noise)
}

struct Sky {
    cells: Vec<(f64, f64)>,
    width: usize,
    height: usize,
    background: f64,
    noise: f64,
}

impl Sky {
    fn new(grid: &[f32], width: usize, height: usize) -> Self {
        let mut cells = Vec::with_capacity(256);
        for cy in 0..16 {
            for cx in 0..16 {
                let (x0, x1) = (cx * width / 16, (cx + 1) * width / 16);
                let (y0, y1) = (cy * height / 16, (cy + 1) * height / 16);
                let (nx, ny) = ((x1 - x0).min(16), (y1 - y0).min(16));
                let mut samples = Vec::with_capacity(nx * ny);
                for sy in 0..ny {
                    for sx in 0..nx {
                        let x = x0 + (2 * sx + 1) * (x1 - x0) / (2 * nx);
                        let y = y0 + (2 * sy + 1) * (y1 - y0) / (2 * ny);
                        samples.push(f64::from(grid[y * width + x]));
                    }
                }
                cells.push(location_noise(&mut samples));
            }
        }
        let background = median(&mut cells.iter().map(|v| v.0).collect::<Vec<_>>());
        let noise = median(&mut cells.iter().map(|v| v.1).collect::<Vec<_>>());
        Self {
            cells,
            width,
            height,
            background,
            noise,
        }
    }

    fn at(&self, x: usize, y: usize) -> (f64, f64) {
        self.cells[(y * 16 / self.height).min(15) * 16 + (x * 16 / self.width).min(15)]
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Star {
    pub x_px: f64,
    pub y_px: f64,
    pub hfr_px: f64,
    pub moment_fwhm_px: f64,
    pub eccentricity: f64,
    pub flux_adu: f64,
    pub peak_adu: f64,
    pub snr: f64,
}

/// Geometry-only centroids on an already reconstructed reference plane.
/// No quality classification or reference photometry is inferred from these stars.
pub(crate) fn reference_catalog(
    plane: &fulgur_core::fits::ReferencePlane,
    cancel: &AtomicBool,
) -> Result<Vec<Star>> {
    let (width, height) = (plane.width, plane.height);
    if width < 32 || height < 32 {
        return Ok(Vec::new());
    }
    let data = &plane.pixels;
    let sky = Sky::new(data, width, height);
    let mut buckets: [Vec<(f32, usize, usize)>; 16] = std::array::from_fn(|_| Vec::new());
    for y in 12..height - 12 {
        check_cancelled(cancel)?;
        for x in 12..width - 12 {
            let v = data[y * width + x];
            let (bg, noise) = sky.at(x, y);
            if v as f64 <= bg + 6. * noise.max(1e-12) {
                continue;
            }
            let peak = (-1_isize..=1).all(|dy| {
                (-1_isize..=1).all(|dx| {
                    if dx == 0 && dy == 0 {
                        return true;
                    }
                    let other =
                        data[(y as isize + dy) as usize * width + (x as isize + dx) as usize];
                    other < v || (other == v && (dy > 0 || (dy == 0 && dx > 0)))
                })
            });
            if peak {
                buckets[(y * 4 / height).min(3) * 4 + (x * 4 / width).min(3)].push((v, x, y));
            }
        }
    }
    for b in &mut buckets {
        b.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.2.cmp(&b.2)).then(a.1.cmp(&b.1)));
        b.truncate(64);
    }
    let mut stars: Vec<Star> = Vec::new();
    for rank in 0..64 {
        for bucket in &buckets {
            check_cancelled(cancel)?;
            let Some(&(peak, x, y)) = bucket.get(rank) else {
                continue;
            };
            let mut ring = Vec::new();
            for yy in y - 12..=y + 12 {
                for xx in x - 12..=x + 12 {
                    let r = (xx as f64 - x as f64).hypot(yy as f64 - y as f64);
                    if (9. ..=12.).contains(&r) {
                        ring.push(data[yy * width + xx] as f64);
                    }
                }
            }
            let bg = median(&mut ring);
            let mut center = [x as f64, y as f64];
            let mut flux = 0.;
            for _ in 0..3 {
                let cx = center[0].round() as usize;
                let cy = center[1].round() as usize;
                if cx < 7 || cy < 7 || cx + 7 >= width || cy + 7 >= height {
                    break;
                }
                let mut sum = [0.; 3];
                for yy in cy - 6..=cy + 6 {
                    for xx in cx - 6..=cx + 6 {
                        if (xx as f64 - center[0]).hypot(yy as f64 - center[1]) > 6. {
                            continue;
                        }
                        let f = (data[yy * width + xx] as f64 - bg).max(0.);
                        sum[0] += f;
                        sum[1] += f * xx as f64;
                        sum[2] += f * yy as f64;
                    }
                }
                if sum[0] <= 0. {
                    break;
                }
                flux = sum[0];
                center = [sum[1] / flux, sum[2] / flux];
            }
            if flux <= 0.
                || stars
                    .iter()
                    .any(|s| (s.x_px - center[0]).hypot(s.y_px - center[1]) < 5.)
            {
                continue;
            }
            let noise = sky.at(x, y).1.max(1e-12);
            stars.push(Star {
                x_px: center[0],
                y_px: center[1],
                hfr_px: 0.,
                moment_fwhm_px: 0.,
                eccentricity: 0.,
                flux_adu: 0.,
                peak_adu: 0.,
                snr: ((peak as f64 - bg) / noise).max(1.),
            });
            if stars.len() == 500 {
                return Ok(stars);
            }
        }
    }
    Ok(stars)
}

struct Green<'a> {
    raw: &'a [f32],
    width: usize,
    height: usize,
    pattern: BayerPattern,
    plans: [Vec<(isize, f64)>; 16],
}

static PHASE_WEIGHTS: std::sync::LazyLock<[[f64; 6]; 4]> = std::sync::LazyLock::new(|| {
    std::array::from_fn(|phase| {
        let mut weights: [f64; 6] = std::array::from_fn(|i| {
            let x = phase as f64 * 0.25 - (i as f64 - 2.0);
            if x.abs() < 1e-12 {
                1.0
            } else if x.abs() >= 3.0 {
                0.0
            } else {
                let p = std::f64::consts::PI * x;
                (p.sin() / p) * ((p / 3.0).sin() / (p / 3.0))
            }
        });
        let sum: f64 = weights.iter().sum();
        for value in &mut weights {
            *value /= sum;
        }
        weights
    })
});

impl<'a> Green<'a> {
    fn new(raw: &'a [f32], width: usize, height: usize, pattern: BayerPattern) -> Self {
        let phase = if pattern.color(0, 0) == Color::Green {
            0
        } else {
            1
        };
        let plans = std::array::from_fn(|n| {
            let (x, y) = ((n % 4) as f64 * 0.5, (n / 4) as f64 * 0.5);
            let (u, v) = ((x + y - phase as f64) * 0.5, (x - y - phase as f64) * 0.5);
            let (iu, iv) = (u.floor() as isize, v.floor() as isize);
            let wx = &PHASE_WEIGHTS[((u - iu as f64) * 4.0).round() as usize];
            let wy = &PHASE_WEIGHTS[((v - iv as f64) * 4.0).round() as usize];
            let mut taps = Vec::new();
            for (j, &w_y) in wy.iter().enumerate() {
                for (i, &w_x) in wx.iter().enumerate() {
                    let weight = w_x * w_y;
                    if weight.abs() < 1e-12 {
                        continue;
                    }
                    let (a, b) = (iu + i as isize - 2, iv + j as isize - 2);
                    let (dx, dy) = (a + b + phase, a - b);
                    taps.push((dy * width as isize + dx, weight));
                }
            }
            taps
        });
        Self {
            raw,
            width,
            height,
            pattern,
            plans,
        }
    }

    // The sixteen half-pixel phases have fixed native-green offsets/weights.
    // Precomputing them once per frame removes coordinate transforms, kernel
    // products and row multiplications from every stellar quadrature sample.
    fn interpolate(&self, x: f64, y: f64) -> f64 {
        let (ix, iy) = ((x * 2.0) as usize, (y * 2.0) as usize);
        let base = (iy / 4 * 2 * self.width + ix / 4 * 2) as isize;
        self.plans[(iy % 4) * 4 + ix % 4]
            .iter()
            .map(|&(offset, weight)| weight * f64::from(self.raw[(base + offset) as usize]))
            .sum()
    }

    fn measure(&self, x: f64, y: f64, background: f64, noise: f64) -> Option<Star> {
        if x < 14.0 || y < 14.0 || x + 14.0 >= self.width as f64 || y + 14.0 >= self.height as f64 {
            return None;
        }
        let (mut cx, mut cy) = (x, y);
        let mut peak = 0.0_f64;
        let mut support = 0;
        for _ in 0..3 {
            let (mut flux, mut sx, mut sy) = (0.0, 0.0, 0.0);
            for yy in (cy as usize - 6)..=(cy as usize + 6) {
                for xx in (cx as usize - 6)..=(cx as usize + 6) {
                    if (xx as f64 - cx).powi(2) + (yy as f64 - cy).powi(2) > 36.0 {
                        continue;
                    }
                    let value = f64::from(self.raw[yy * self.width + xx]);
                    if (xx as f64 - cx).abs() <= 2.0
                        && (yy as f64 - cy).abs() <= 2.0
                        && value > 62000.0
                    {
                        return None;
                    }
                    if self.pattern.color(xx, yy) != Color::Green {
                        continue;
                    }
                    peak = peak.max(value);
                    if value > background + 3.0 * noise {
                        support += 1;
                    }
                    let f = (value - background).max(0.0);
                    flux += f;
                    sx += f * xx as f64;
                    sy += f * yy as f64;
                }
            }
            if flux <= 0.0 {
                return None;
            }
            cx = sx / flux;
            cy = sy / flux;
            if (cx - x).abs() > 3.0 || (cy - y).abs() > 3.0 {
                return None;
            }
        }
        if support < 9 || peak < background + 10.0 * noise {
            return None;
        }
        if cx < 14.0
            || cy < 14.0
            || cx + 14.0 >= self.width as f64
            || cy + 14.0 >= self.height as f64
        {
            return None;
        }
        // Half-pixel quadrature of positive, locally sky-subtracted green
        // flux. HFR is the interpolated cumulative half-flux radius, not a
        // mean radial distance or a Gaussian-width proxy.
        let mut radial = vec![0.0; 640];
        let (mut flux, mut xx, mut yy, mut xy) = (0.0, 0.0, 0.0, 0.0);
        for j in -12..=12 {
            for i in -12..=12 {
                let px = (cx * 2.0).floor() * 0.5 + i as f64 * 0.5;
                let py = (cy * 2.0).floor() * 0.5 + j as f64 * 0.5;
                let (dx, dy) = (px - cx, py - cy);
                let r2 = dx * dx + dy * dy;
                if r2 > 36.0 {
                    continue;
                }
                let f = (self.interpolate(px, py) - background).max(0.0) * 0.25;
                flux += f;
                xx += f * dx * dx;
                yy += f * dy * dy;
                xy += f * dx * dy;
                // Fractional pixel-area contribution smooths the cumulative
                // curve without confusing point-sample radii with aperture
                // boundaries. Histogram bins are 0.01 full-resolution pixel.
                for oy in [-0.125, 0.125] {
                    for ox in [-0.125, 0.125] {
                        let radius = ((dx + ox).powi(2) + (dy + oy).powi(2)).sqrt();
                        radial[(radius * 100.0) as usize] += f / 4.0;
                    }
                }
            }
        }
        let snr = flux / (noise.max(1e-12) * 56.0_f64.sqrt() * 2.0);
        if flux <= 0.0 || snr < 15.0 {
            return None;
        }
        let trace = (xx + yy) / flux;
        let delta = (((xx - yy) / flux).powi(2) + 4.0 * (xy / flux).powi(2)).sqrt();
        let (major, minor) = ((trace + delta) * 0.5, (trace - delta) * 0.5);
        if minor <= 0.1 || major <= 0.0 {
            return None;
        }
        let (mut cumulative, mut hfr) = (0.0, 0.0);
        for (bin, f) in radial.into_iter().enumerate() {
            if cumulative + f >= flux * 0.5 && f > 0.0 {
                hfr = (bin as f64 + (flux * 0.5 - cumulative) / f) / 100.0;
                break;
            }
            cumulative += f;
        }
        if !(0.5..5.0).contains(&hfr) {
            return None;
        }
        Some(Star {
            x_px: cx,
            y_px: cy,
            hfr_px: hfr,
            moment_fwhm_px: 2.354820045 * (trace * 0.5).sqrt(),
            eccentricity: (1.0 - minor / major).max(0.0).sqrt(),
            flux_adu: flux,
            peak_adu: peak,
            snr,
        })
    }
}

pub(crate) struct Metrics {
    pub background: f64,
    pub noise: f64,
    pub detected_peaks: usize,
    pub detected_stars: usize,
    pub stars: Vec<Star>,
}

pub(crate) fn analyze(frame: &RawFrame, cancel: &AtomicBool) -> Result<Metrics> {
    let (width, height) = (frame.header.info.width, frame.header.info.height);
    let pattern = frame.header.info.bayer_pattern;
    let (gw, gh) = (width / 2, height / 2);
    let diagonal = pattern.color(0, 0) == Color::Green;
    let mut grid = vec![0.0; gw * gh];
    for y in 0..gh {
        check_cancelled(cancel)?;
        for x in 0..gw {
            let i = 2 * y * width + 2 * x;
            grid[y * gw + x] = if diagonal {
                (frame.pixels[i] + frame.pixels[i + width + 1]) * 0.5
            } else {
                (frame.pixels[i + 1] + frame.pixels[i + width]) * 0.5
            };
        }
    }
    let sky = Sky::new(&grid, gw, gh);
    let mut buckets: [Vec<(f32, usize, usize)>; 16] = std::array::from_fn(|_| Vec::new());
    let mut detected_peaks = 0;
    let mut detected_stars = 0;
    for y in 7..gh - 7 {
        check_cancelled(cancel)?;
        for x in 7..gw - 7 {
            let v = grid[y * gw + x];
            let (bg, noise) = sky.at(x, y);
            if f64::from(v) <= (bg + 6.0 * noise).max(sky.background + 6.0 * sky.noise) {
                continue;
            }
            let mut is_peak = true;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let neighbor =
                        grid[(y as isize + dy) as usize * gw + (x as isize + dx) as usize];
                    if neighbor > v || (neighbor == v && (dy < 0 || (dy == 0 && dx < 0))) {
                        is_peak = false;
                    }
                }
            }
            if !is_peak {
                continue;
            }
            detected_peaks += 1;
            let mut supported = 0;
            for yy in y - 1..=y + 1 {
                for xx in x - 1..=x + 1 {
                    supported += usize::from(f64::from(grid[yy * gw + xx]) > bg + 3.0 * noise);
                }
            }
            if supported < 3 {
                continue;
            }
            let mut saturated = false;
            for yy in 2 * y - 1..=2 * y + 2 {
                for xx in 2 * x - 1..=2 * x + 2 {
                    saturated |= frame.pixels[yy * width + xx] > 62000.0;
                }
            }
            if saturated {
                continue;
            }
            detected_stars += 1;
            let bucket = &mut buckets[(y * 4 / gh).min(3) * 4 + (x * 4 / gw).min(3)];
            bucket.push((v, x, y));
        }
    }
    for bucket in &mut buckets {
        bucket.sort_unstable_by(|a, b| b.0.total_cmp(&a.0).then(a.2.cmp(&b.2)).then(a.1.cmp(&b.1)));
        bucket.truncate(64);
    }
    let plane = Green::new(&frame.pixels, width, height, pattern);
    let mut stars: Vec<Star> = Vec::new();
    for rank in 0..64 {
        check_cancelled(cancel)?;
        for bucket in &buckets {
            let Some(&(_, x, y)) = bucket.get(rank) else {
                continue;
            };
            let (cx, cy) = (2.0 * x as f64 + 0.5, 2.0 * y as f64 + 0.5);
            if stars
                .iter()
                .any(|s| (s.x_px - cx).powi(2) + (s.y_px - cy).powi(2) < 144.0)
            {
                continue;
            }
            let mut ring = Vec::new();
            for yy in (cy as usize - 10)..=(cy as usize + 10) {
                for xx in (cx as usize - 10)..=(cx as usize + 10) {
                    let r2 = (xx as f64 - cx).powi(2) + (yy as f64 - cy).powi(2);
                    if (49.0..=100.0).contains(&r2) && pattern.color(xx, yy) == Color::Green {
                        ring.push(f64::from(frame.pixels[yy * width + xx]));
                    }
                }
            }
            let local = median(&mut ring);
            if let Some(star) = plane.measure(cx, cy, local, sky.noise * std::f64::consts::SQRT_2) {
                stars.push(star);
                if stars.len() == 250 {
                    break;
                }
            }
        }
        if stars.len() == 250 {
            break;
        }
    }
    Ok(Metrics {
        background: sky.background,
        noise: sky.noise,
        detected_peaks,
        detected_stars,
        stars,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn gaussian(sx: f64, sy: f64, phase_x: f64, phase_y: f64) -> (Vec<f32>, f64, f64) {
        let (cx, cy) = (64.0 + phase_x, 64.0 + phase_y);
        let pixels = (0..128 * 128)
            .map(|i| {
                let (x, y) = ((i % 128) as f64, (i / 128) as f64);
                (2000.0
                    + 10000.0
                        * (-0.5 * ((x - cx).powi(2) / sx.powi(2) + (y - cy).powi(2) / sy.powi(2)))
                            .exp()) as f32
            })
            .collect();
        (pixels, cx, cy)
    }
    #[test]
    fn true_hfr_recovers_gaussian_with_subpixel_phases() {
        let mut maximum_error = 0.0_f64;
        for pattern in [
            BayerPattern::GRBG,
            BayerPattern::RGGB,
            BayerPattern::GBRG,
            BayerPattern::BGGR,
        ] {
            for sigma in [1.25, 1.5, 2.0] {
                for phase_x in [0.0, 0.25, 0.5, 0.75] {
                    for phase_y in [0.0, 0.25, 0.5, 0.75] {
                        let (raw, x, y) = gaussian(sigma, sigma, phase_x, phase_y);
                        let plane = Green::new(&raw, 128, 128, pattern);
                        let star = plane.measure(x, y, 2000.0, 0.0).unwrap();
                        let expected = (2.0 * 2.0_f64.ln()).sqrt() * sigma;
                        let error = (star.hfr_px / expected - 1.0).abs();
                        maximum_error = maximum_error.max(error);
                        assert!(
                            error < 0.03,
                            "pattern={pattern:?} sigma={sigma}, phase={phase_x},{phase_y}, hfr={}, expected={expected}",
                            star.hfr_px
                        );
                        assert!((star.x_px - x).abs() < 0.05 && (star.y_px - y).abs() < 0.05);
                        assert!(star.eccentricity < 0.15);
                    }
                }
            }
        }
        println!(
            "HFR gate: 192 Gaussian/CFA/phase cases, maximum relative error={maximum_error:.8}"
        );
    }
    #[test]
    fn moment_eccentricity_recovers_elongated_star() {
        let (raw, x, y) = gaussian(2.0, 1.5, 0.25, 0.5);
        let plane = Green::new(&raw, 128, 128, BayerPattern::GRBG);
        let star = plane.measure(x, y, 2000.0, 0.0).unwrap();
        let expected = (1.0_f64 - (1.5_f64 / 2.0).powi(2)).sqrt();
        assert!((star.eccentricity - expected).abs() < 0.03);
    }
    #[test]
    fn robust_sky_uses_pixel_noise_not_noise_of_cell_medians() {
        let grid: Vec<_> = (0..128 * 128)
            .map(|i| 2000.0 + ((i * 17 + i / 128 * 13) % 31) as f32 - 15.0)
            .collect();
        let sky = Sky::new(&grid, 128, 128);
        assert!((sky.background - 2000.0).abs() < 2.0);
        assert!(sky.noise > 8.0 && sky.noise < 15.0);
    }
}
