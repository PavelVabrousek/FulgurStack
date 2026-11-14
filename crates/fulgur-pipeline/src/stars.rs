//! Green-CFA (or full monochrome plane) photometry; no RGB reconstruction or
//! intermediate images.

use fulgur_core::{BayerPattern, Color, Error, Mosaic, RawFrame, Result, check_cancelled};
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
        Self::sampled(width, height, |x, y| grid[y * width + x])
    }

    /// Reads only the sparse samples, so a plane need not exist as an image.
    fn sampled(width: usize, height: usize, value: impl Fn(usize, usize) -> f32) -> Self {
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
                        samples.push(f64::from(value(x, y)));
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

/// Sky level of each colour channel (R, G, B) of a CFA frame, with the frame
/// sky's own cells and robust location (ADR-102). Green stays `green`, the
/// level of the detection grid, so the two can never disagree.
pub(crate) fn channel_sky(
    pixels: &[f32],
    width: usize,
    height: usize,
    pattern: BayerPattern,
    green: f64,
) -> [f64; 3] {
    let (gw, gh) = (width / 2, height / 2);
    let level = |color: Color| {
        let (px, py) = [(0, 0), (1, 0), (0, 1), (1, 1)]
            .into_iter()
            .find(|&(x, y)| pattern.color(x, y) == color)
            .unwrap_or((0, 0));
        Sky::sampled(gw, gh, |x, y| pixels[(2 * y + py) * width + 2 * x + px]).background
    };
    [level(Color::Red), green, level(Color::Blue)]
}

/// Sky level of a whole colour plane of a frame debayered elsewhere.
pub(crate) fn plane_level(pixels: &[f32], width: usize, height: usize) -> f64 {
    Sky::new(pixels, width, height).background
}

/// Per-pixel sky level and noise of a full-resolution plane read through
/// `value(x, y)` (`None` outside the stacked footprint): robust local
/// estimates on the same 16x16 cell grid and sparse sampling as the frame
/// sky, so stars and nebulae barely touch them. Cells with fewer than 32
/// covered samples or no noise are ignored.
pub(crate) fn plane_sky(
    width: usize,
    height: usize,
    value: impl Fn(usize, usize) -> Option<f64>,
) -> Option<(f64, f64)> {
    let mut levels = Vec::with_capacity(256);
    let mut noises = Vec::with_capacity(256);
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
                    if let Some(v) = value(x, y).filter(|v| v.is_finite()) {
                        samples.push(v);
                    }
                }
            }
            if samples.len() < 32 {
                continue;
            }
            let (level, noise) = location_noise(&mut samples);
            if noise > 0.0 {
                levels.push(level);
                noises.push(noise);
            }
        }
    }
    (!levels.is_empty()).then(|| (median(&mut levels), median(&mut noises)))
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
    reference_stars(plane, 64, cancel)
}

/// The same list, deep enough that a rich cell does not hide true stars from
/// an accuracy check.
pub(crate) fn reference_witnesses(
    plane: &fulgur_core::fits::ReferencePlane,
    cancel: &AtomicBool,
) -> Result<Vec<Star>> {
    reference_stars(plane, 256, cancel)
}

fn reference_stars(
    plane: &fulgur_core::fits::ReferencePlane,
    per_cell: usize,
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
        b.truncate(per_cell);
    }
    let mut stars: Vec<Star> = Vec::new();
    for rank in 0..per_cell {
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
            if stars.len() == per_cell * 500 / 64 {
                return Ok(stars);
            }
        }
    }
    Ok(stars)
}

/// Measurement plane: the native green CFA lattice, or (pattern `None`) every
/// pixel of a monochrome frame.
struct Green<'a> {
    raw: &'a [f32],
    width: usize,
    height: usize,
    pattern: Option<BayerPattern>,
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
    fn mono(raw: &'a [f32], width: usize, height: usize) -> Self {
        Self {
            raw,
            width,
            height,
            pattern: None,
            plans: std::array::from_fn(|_| Vec::new()),
        }
    }

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
            pattern: Some(pattern),
            plans,
        }
    }

    // The sixteen half-pixel phases have fixed native-green offsets/weights.
    // Precomputing them once per frame removes coordinate transforms, kernel
    // products and row multiplications from every stellar quadrature sample.
    fn interpolate(&self, x: f64, y: f64) -> f64 {
        if self.pattern.is_none() {
            return self.interpolate_mono(x, y);
        }
        let (ix, iy) = ((x * 2.0) as usize, (y * 2.0) as usize);
        let base = (iy / 4 * 2 * self.width + ix / 4 * 2) as isize;
        self.plans[(iy % 4) * 4 + ix % 4]
            .iter()
            .map(|&(offset, weight)| weight * f64::from(self.raw[(base + offset) as usize]))
            .sum()
    }

    /// Same Lanczos-3 window on the full monochrome grid; quadrature positions
    /// are whole or half pixels, so only phases 0 and 0.5 occur.
    fn interpolate_mono(&self, x: f64, y: f64) -> f64 {
        let (ix, iy) = (x.floor() as isize, y.floor() as isize);
        let wx = &PHASE_WEIGHTS[((x - ix as f64) * 4.0).round() as usize];
        let wy = &PHASE_WEIGHTS[((y - iy as f64) * 4.0).round() as usize];
        let mut sum = 0.0;
        for (j, &w_y) in wy.iter().enumerate() {
            let row = (iy + j as isize - 2) as usize * self.width;
            for (i, &w_x) in wx.iter().enumerate() {
                sum += w_x * w_y * f64::from(self.raw[row + (ix + i as isize - 2) as usize]);
            }
        }
        sum
    }

    fn measure(&self, x: f64, y: f64, background: f64, noise: f64) -> Option<Star> {
        // A monochrome aperture holds every pixel, a CFA aperture only greens.
        let (minimum_support, aperture_samples) = if self.pattern.is_some() {
            (9, 56.0_f64)
        } else {
            (18, 113.0_f64)
        };
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
                    if self
                        .pattern
                        .is_some_and(|pattern| pattern.color(xx, yy) != Color::Green)
                    {
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
        if support < minimum_support || peak < background + 10.0 * noise {
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
        let snr = flux / (noise.max(1e-12) * aperture_samples.sqrt() * 2.0);
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
    /// Plane samples cosmetically suppressed before detection/measurement.
    pub hot_pixels: usize,
    /// Remaining detections refused for a single-sample profile.
    pub defect_detections: usize,
    /// Registration catalog of a dense field; empty otherwise (ADR-108).
    pub dense_catalog: Vec<Star>,
    pub stars: Vec<Star>,
}

/// Smallest share of the peak excess that an optical point-spread profile
/// leaves on its nearest measurement-plane neighbours. Uncalibrated Dwarf3
/// sensor-fixed defects stay below 0.04 (99th percentile); real Dwarf3 and
/// Seestar stars sit near 0.4-0.55 (Seestar 1st percentile 0.23).
const MIN_NEIGHBOUR_SHARE: f64 = 0.1;

/// Hot pixels carry their excess in one sample; any star spreads light onto
/// adjacent samples. The brightest plane sample of the raw 4x4 window around a
/// detection cell is compared with the median of its four nearest plane
/// neighbours (diagonal greens on a CFA, orthogonal pixels on mono), both
/// above the local plane-sample sky ring.
fn natural_profile(
    raw: &[f32],
    width: usize,
    pattern: Option<BayerPattern>,
    x: usize,
    y: usize,
) -> bool {
    let on_plane = |xx: usize, yy: usize| pattern.is_none_or(|p| p.color(xx, yy) == Color::Green);
    let mut best = (2 * x, 2 * y);
    let mut peak = f32::NEG_INFINITY;
    for yy in 2 * y - 1..=2 * y + 2 {
        for xx in 2 * x - 1..=2 * x + 2 {
            if on_plane(xx, yy) && raw[yy * width + xx] > peak {
                peak = raw[yy * width + xx];
                best = (xx, yy);
            }
        }
    }
    let (px, py) = best;
    let mut ring = Vec::new();
    for yy in py - 10..=py + 10 {
        for xx in px - 10..=px + 10 {
            let r2 = (xx.abs_diff(px).pow(2) + yy.abs_diff(py).pow(2)) as f64;
            if (49.0..=100.0).contains(&r2) && on_plane(xx, yy) {
                ring.push(f64::from(raw[yy * width + xx]));
            }
        }
    }
    let background = median(&mut ring);
    let mut near = plane_neighbours(pattern).map(|(dx, dy)| {
        let (xx, yy) = (px as isize + dx, py as isize + dy);
        f64::from(raw[yy as usize * width + xx as usize]) - background
    });
    near.sort_unstable_by(f64::total_cmp);
    let excess = f64::from(peak) - background;
    excess > 0.0 && (near[1] + near[2]) * 0.5 >= MIN_NEIGHBOUR_SHARE * excess
}

/// Which pixels a detection cell averages: all four, or which two greens.
fn cell_kind(pattern: Option<BayerPattern>) -> (bool, bool) {
    (
        pattern.is_none(),
        pattern.is_some_and(|p| p.color(0, 0) == Color::Green),
    )
}

#[inline]
fn grid_cell(pixels: &[f32], width: usize, kind: (bool, bool), x: usize, y: usize) -> f32 {
    let i = 2 * y * width + 2 * x;
    if kind.0 {
        (pixels[i] + pixels[i + 1] + pixels[i + width] + pixels[i + width + 1]) * 0.25
    } else if kind.1 {
        (pixels[i] + pixels[i + width + 1]) * 0.5
    } else {
        (pixels[i + 1] + pixels[i + width]) * 0.5
    }
}

/// Detection grid: mean of the two greens of each CFA cell, or of all four
/// pixels of a monochrome 2x2 cell. Both keep sensor coordinates at 2x.
fn detection_grid(
    pixels: &[f32],
    width: usize,
    pattern: Option<BayerPattern>,
    cancel: &AtomicBool,
) -> Result<Vec<f32>> {
    let (gw, gh) = (width / 2, pixels.len() / width / 2);
    let kind = cell_kind(pattern);
    let mut grid = vec![0.0; gw * gh];
    for y in 0..gh {
        check_cancelled(cancel)?;
        for x in 0..gw {
            grid[y * gw + x] = grid_cell(pixels, width, kind, x, y);
        }
    }
    Ok(grid)
}

/// Writes the frame with its defects replaced to `copy` and updates the cells
/// of `grid` they touch, which leaves `grid` the detection grid of `copy`.
fn suppress(
    pixels: &[f32],
    width: usize,
    pattern: Option<BayerPattern>,
    fixes: &[(usize, f32)],
    grid: &mut [f32],
    copy: &mut Vec<f32>,
) {
    copy.clear();
    copy.extend_from_slice(pixels);
    for &(i, value) in fixes {
        copy[i] = value;
    }
    let (gw, gh) = (width / 2, pixels.len() / width / 2);
    let kind = cell_kind(pattern);
    for &(i, _) in fixes {
        let (x, y) = (i % width / 2, i / width / 2);
        if x < gw && y < gh {
            grid[y * gw + x] = grid_cell(copy, width, kind, x, y);
        }
    }
}

/// Nearest measurement-plane neighbours: diagonal greens on a CFA, orthogonal
/// pixels on a monochrome sensor.
fn plane_neighbours(pattern: Option<BayerPattern>) -> [(isize, isize); 4] {
    if pattern.is_some() {
        [(-1, -1), (1, -1), (-1, 1), (1, 1)]
    } else {
        [(0, -1), (-1, 0), (1, 0), (0, 1)]
    }
}

/// Uncalibrated sensors leave hot pixels inside stellar apertures, where they
/// bias centroids, HFR and eccentricity. A plane sample is a defect when it
/// exceeds the median of its four nearest plane neighbours by more than ten
/// pixel sigmas while those neighbours hold less than `MIN_NEIGHBOUR_SHARE`
/// of its excess over the sky cell; it is replaced by that median. Returns the
/// replacements computed from the untouched frame; source pixels never change.
fn defects(
    pixels: &[f32],
    width: usize,
    height: usize,
    pattern: Option<BayerPattern>,
    sky: &Sky,
    pixel_noise: f64,
    cancel: &AtomicBool,
) -> Result<Vec<(usize, f32)>> {
    let offsets = plane_neighbours(pattern).map(|(dx, dy)| dy * width as isize + dx);
    let limit = 10.0 * pixel_noise;
    let mut fixes = Vec::new();
    for y in 1..height - 1 {
        check_cancelled(cancel)?;
        let row_sky = y / 2;
        // Greens alternate along a CFA row; mono visits every pixel.
        let (first, step) = match pattern {
            Some(p) if p.color(1, y) == Color::Green => (1, 2),
            Some(_) => (2, 2),
            None => (1, 1),
        };
        for x in (first..width - 1).step_by(step) {
            let i = y * width + x;
            // The neighbour median is at least the second-smallest neighbour,
            // so fewer than two neighbours below value-limit rules it out.
            let floor = pixels[i] - limit as f32;
            if offsets
                .iter()
                .filter(|&&o| pixels[(i as isize + o) as usize] < floor)
                .count()
                < 2
            {
                continue;
            }
            let value = f64::from(pixels[i]);
            let mut near = offsets.map(|o| f64::from(pixels[(i as isize + o) as usize]));
            near.sort_unstable_by(f64::total_cmp);
            let middle = (near[1] + near[2]) * 0.5;
            if value - middle <= limit {
                continue;
            }
            let background = sky
                .at((x / 2).min(sky.width - 1), row_sky.min(sky.height - 1))
                .0;
            if middle - background < MIN_NEIGHBOUR_SHARE * (value - background) {
                fixes.push((i, middle as f32));
            }
        }
    }
    Ok(fixes)
}

#[cfg(test)]
pub(crate) fn analyze(frame: &RawFrame, cancel: &AtomicBool) -> Result<Metrics> {
    analyze_with_limit(frame, cancel, CATALOG_STARS)
}

/// Stars kept per frame catalog (quality and registration).
pub(crate) const CATALOG_STARS: usize = 250;
/// Detected stars from which a field counts as dense. With far more stars
/// than a catalog holds, the brightest by peak change with seeing and frames
/// share too few; flux keeps the same stars in the same order (ADR-108).
const DENSE_FIELD_STARS: usize = 20_000;
/// Limit meaning: the ordinary catalog, plus registration lists in a dense field.
const FIELD_CATALOG: usize = 0;
/// Stars per 4x4 cell in each registration list of a dense field.
const DENSE_CELL_STARS: usize = 15;
/// Flux rank in each cell where the photometry list starts, well below
/// saturation. A fixed rank picks the same stars in every frame; the rank of
/// the faintest saturated star moves with seeing and would not.
const DENSE_BAND_RANK: usize = 250;
/// Flux detection threshold in sky sigmas, and the largest star in grid cells.
const FLUX_SIGMAS: f64 = 40.;
const FLUX_MAX_CELLS: usize = 3000;

pub(crate) fn is_dense(detected_stars: usize) -> bool {
    detected_stars >= DENSE_FIELD_STARS
}

/// One bright object of the detection grid.
struct FluxStar {
    x_px: f64,
    y_px: f64,
    flux: f64,
    /// Brightest grid cell, for the precise measurement.
    peak: (usize, usize),
    saturated: bool,
}

/// Connected bright regions of the grid, brightest flux first. Total flux
/// does not depend on seeing, and the centroid of a clipped star stays true.
#[allow(clippy::too_many_arguments)] // One grid and its detection context.
fn flux_stars(
    grid: &[f32],
    (gw, gh): (usize, usize),
    pixels: &[f32],
    width: usize,
    sky: &Sky,
    saturated_above: f32,
    cancel: &AtomicBool,
) -> Result<Vec<FluxStar>> {
    let mut seen = vec![false; gw * gh];
    let mut stack = vec![];
    let mut stars = vec![];
    for y in 7..gh - 7 {
        check_cancelled(cancel)?;
        for x in 7..gw - 7 {
            let start = y * gw + x;
            if seen[start] {
                continue;
            }
            let background = sky.at(x, y).0;
            let limit = (background + FLUX_SIGMAS * sky.noise) as f32;
            if grid[start] <= limit {
                continue;
            }
            seen[start] = true;
            stack.push(start);
            let (mut flux, mut sum_x, mut sum_y, mut cells) = (0., 0., 0., 0);
            let (mut peak, mut brightest) = ((x, y), f32::MIN);
            let mut saturated = false;
            while let Some(i) = stack.pop() {
                let (cx, cy) = (i % gw, i / gw);
                let value = grid[i];
                let above = f64::from(value) - background;
                flux += above;
                sum_x += above * cx as f64;
                sum_y += above * cy as f64;
                cells += 1;
                if value > brightest {
                    brightest = value;
                    peak = (cx, cy);
                }
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    saturated |= pixels[(2 * cy + dy) * width + 2 * cx + dx] > saturated_above;
                }
                for next in [i - 1, i + 1, i - gw, i + gw] {
                    if next < seen.len() && !seen[next] && grid[next] > limit {
                        seen[next] = true;
                        stack.push(next);
                    }
                }
            }
            if (4..FLUX_MAX_CELLS).contains(&cells) && flux > 0. {
                stars.push(FluxStar {
                    x_px: 2. * sum_x / flux + 0.5,
                    y_px: 2. * sum_y / flux + 0.5,
                    flux,
                    peak,
                    saturated,
                });
            }
        }
    }
    stars.sort_unstable_by(|a, b| b.flux.total_cmp(&a.flux));
    Ok(stars)
}

fn flux_cell(star: &FluxStar, size: (usize, usize)) -> usize {
    (star.y_px as usize * 4 / size.1).min(3) * 4 + (star.x_px as usize * 4 / size.0).min(3)
}

/// Indices brightest first, at most `each` per 4x4 cell, after `skip` per cell.
fn per_cell(
    stars: &[FluxStar],
    skip: [usize; 16],
    each: usize,
    size: (usize, usize),
    taken: &mut [bool],
    usable: impl Fn(&FluxStar) -> bool,
) -> Vec<usize> {
    let mut ranks = [0; 16];
    let mut counts = [0; 16];
    let mut chosen = vec![];
    for (i, star) in stars.iter().enumerate() {
        let cell = flux_cell(star, size);
        ranks[cell] += 1;
        if ranks[cell] > skip[cell] && !taken[i] && usable(star) && counts[cell] < each {
            counts[cell] += 1;
            taken[i] = true;
            chosen.push(i);
        }
    }
    chosen
}

fn position_only(x_px: f64, y_px: f64) -> Star {
    Star {
        x_px,
        y_px,
        hfr_px: 0.,
        moment_fwhm_px: 0.,
        eccentricity: 0.,
        flux_adu: 0.,
        peak_adu: 0.,
        snr: 0.,
    }
}

/// Registration catalog of a dense field: the brightest stars by flux for
/// geometry, saturated ones included as positions, then unsaturated stars
/// from just below saturation for photometry.
fn dense_catalog(
    stars: &[FluxStar],
    size: (usize, usize),
    measure: impl Fn(usize, usize) -> Option<Star>,
) -> Vec<Star> {
    let mut taken = vec![false; stars.len()];
    let geometry = per_cell(stars, [0; 16], DENSE_CELL_STARS, size, &mut taken, |_| true);
    // A sparse cell starts halfway, so that it still lends stars.
    let mut skip = [0; 16];
    for star in stars {
        skip[flux_cell(star, size)] += 1;
    }
    let band = per_cell(
        stars,
        skip.map(|count| DENSE_BAND_RANK.min(count / 2)),
        DENSE_CELL_STARS,
        size,
        &mut taken,
        |s| !s.saturated,
    );
    let mut catalog = Vec::with_capacity(geometry.len() + band.len());
    for i in geometry {
        let s = &stars[i];
        catalog.push(
            (!s.saturated)
                .then(|| measure(s.peak.0, s.peak.1))
                .flatten()
                .unwrap_or_else(|| position_only(s.x_px, s.y_px)),
        );
    }
    catalog.extend(
        band.into_iter()
            .filter_map(|i| measure(stars[i].peak.0, stars[i].peak.1)),
    );
    catalog
}

/// The brightest stars by flux as positions, for the witnesses of a dense anchor.
pub(crate) fn flux_witnesses(
    frame: &RawFrame,
    cancel: &AtomicBool,
    each: usize,
) -> Result<Vec<Star>> {
    let (width, height) = (frame.header.info.width, frame.header.info.height);
    let pattern = frame_pattern(frame)?;
    let (gw, gh) = (width / 2, height / 2);
    let grid = detection_grid(&frame.pixels, width, pattern, cancel)?;
    let sky = Sky::new(&grid, gw, gh);
    let stars = flux_stars(
        &grid,
        (gw, gh),
        &frame.pixels,
        width,
        &sky,
        saturated_limit(frame.header.info.saturation_adu),
        cancel,
    )?;
    let mut taken = vec![false; stars.len()];
    Ok(
        per_cell(&stars, [0; 16], each, (width, height), &mut taken, |_| true)
            .into_iter()
            .map(|i| position_only(stars[i].x_px, stars[i].y_px))
            .collect(),
    )
}

pub(crate) fn saturated_limit(saturation: Option<f32>) -> f32 {
    saturation.map_or(SATURATED_STAR_ADU, |level| {
        level * (SATURATED_STAR_ADU / 65535.)
    })
}

/// The frame catalog for quality and registration; `copy` is a worker's
/// reusable storage for the defect-suppressed frame.
pub(crate) fn analyze_field(
    frame: &RawFrame,
    copy: &mut Vec<f32>,
    cancel: &AtomicBool,
) -> Result<Metrics> {
    scan(frame, cancel, FIELD_CATALOG, false, copy)
}

/// As `analyze`, keeping up to `limit` stars. The first `CATALOG_STARS` are
/// identical to the ordinary catalog (same order and positions).
pub(crate) fn analyze_with_limit(
    frame: &RawFrame,
    cancel: &AtomicBool,
    limit: usize,
) -> Result<Metrics> {
    scan(frame, cancel, limit, false, &mut Vec::new())
}

/// Positions of the `limit` brightest detections, saturated stars included,
/// for whole-field coverage counting only: a 3x3 detection-grid centroid
/// (about 1 px accuracy), no photometry, no per-region quota.
pub(crate) fn witness_positions(
    frame: &RawFrame,
    cancel: &AtomicBool,
    limit: usize,
) -> Result<Vec<Star>> {
    scan(frame, cancel, limit, true, &mut Vec::new()).map(|m| m.stars)
}

fn scan(
    frame: &RawFrame,
    cancel: &AtomicBool,
    limit: usize,
    positions_only: bool,
    copy: &mut Vec<f32>,
) -> Result<Metrics> {
    let (width, height) = (frame.header.info.width, frame.header.info.height);
    let pattern = frame_pattern(frame)?;
    scan_plane(
        &frame.pixels,
        width,
        height,
        pattern,
        cancel,
        limit,
        positions_only,
        frame.header.info.saturation_adu,
        copy,
    )
}

fn frame_pattern(frame: &RawFrame) -> Result<Option<BayerPattern>> {
    match frame.header.info.mosaic {
        Mosaic::Bayer => frame
            .header
            .info
            .bayer_pattern
            .map(Some)
            .ok_or(Error::Invalid("Bayer frame is missing its pattern.")),
        Mosaic::Mono | Mosaic::Rgb => Ok(None),
        Mosaic::Unknown => Err(Error::Invalid(
            "Sensor layout is unknown; choose colour or mono for this light set.",
        )),
    }
}

/// Fixed 16-bit star limit, and the same share of a lower sensor limit.
const SATURATED_STAR_ADU: f32 = 62000.;

/// As `scan` on a bare plane: a CFA with its pattern, or a monochrome plane
/// (pattern `None`), e.g. the green channel of a stacked master.
#[allow(clippy::too_many_arguments)] // One plane and its detection context.
pub(crate) fn scan_plane(
    frame_pixels: &[f32],
    width: usize,
    height: usize,
    pattern: Option<BayerPattern>,
    cancel: &AtomicBool,
    limit: usize,
    positions_only: bool,
    saturation: Option<f32>,
    copy: &mut Vec<f32>,
) -> Result<Metrics> {
    let saturated_above = saturated_limit(saturation);
    let (gw, gh) = (width / 2, height / 2);
    let mut grid = detection_grid(frame_pixels, width, pattern, cancel)?;
    let sky = Sky::new(&grid, gw, gh);
    // Per-pixel noise: the grid averages two (CFA) or four (mono) pixels.
    let pixel_noise = if pattern.is_some() {
        sky.noise * std::f64::consts::SQRT_2
    } else {
        sky.noise * 2.0
    };
    // Sky statistics come from the untouched frame; only star detection and
    // measurement see the cosmetically suppressed copy.
    let fixes = defects(
        frame_pixels,
        width,
        height,
        pattern,
        &sky,
        pixel_noise,
        cancel,
    )?;
    let hot_pixels = fixes.len();
    let pixels: &[f32] = if fixes.is_empty() {
        frame_pixels
    } else {
        suppress(frame_pixels, width, pattern, &fixes, &mut grid, copy);
        copy
    };
    // The peak limit of each sky cell, for the scan of every grid cell.
    let peak_limits: Vec<f64> = sky
        .cells
        .iter()
        .map(|&(bg, noise)| (bg + 6.0 * noise).max(sky.background + 6.0 * sky.noise))
        .collect();
    let column_cells: Vec<usize> = (0..gw).map(|x| (x * 16 / gw).min(15)).collect();
    let mut buckets: [Vec<(f32, usize, usize)>; 16] = std::array::from_fn(|_| Vec::new());
    let mut positions: Vec<(f32, usize, usize)> = Vec::new();
    let mut detected_peaks = 0;
    let mut detected_stars = 0;
    let mut defect_detections = 0;
    for y in 7..gh - 7 {
        check_cancelled(cancel)?;
        let row_cells = (y * 16 / gh).min(15) * 16;
        for x in 7..gw - 7 {
            let v = grid[y * gw + x];
            if f64::from(v) <= peak_limits[row_cells + column_cells[x]] {
                continue;
            }
            let (bg, noise) = sky.at(x, y);
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
                    saturated |= pixels[yy * width + xx] > saturated_above;
                }
            }
            if saturated && !positions_only {
                continue;
            }
            if !saturated && !natural_profile(pixels, width, pattern, x, y) {
                defect_detections += 1;
                continue;
            }
            if positions_only {
                positions.push((v, x, y));
                continue;
            }
            detected_stars += 1;
            let bucket = &mut buckets[(y * 4 / gh).min(3) * 4 + (x * 4 / gw).min(3)];
            bucket.push((v, x, y));
        }
    }
    if positions_only {
        positions
            .sort_unstable_by(|a, b| b.0.total_cmp(&a.0).then(a.2.cmp(&b.2)).then(a.1.cmp(&b.1)));
        positions.truncate(limit);
        let stars = positions
            .into_iter()
            .map(|(_, x, y)| {
                let bg = sky.at(x, y).0;
                let (mut w, mut sx, mut sy) = (0., 0., 0.);
                for yy in y - 1..=y + 1 {
                    for xx in x - 1..=x + 1 {
                        let f = (f64::from(grid[yy * gw + xx]) - bg).max(0.);
                        w += f;
                        sx += f * xx as f64;
                        sy += f * yy as f64;
                    }
                }
                let (gx, gy) = if w > 0. {
                    (sx / w, sy / w)
                } else {
                    (x as f64, y as f64)
                };
                Star {
                    x_px: 2. * gx + 0.5,
                    y_px: 2. * gy + 0.5,
                    hfr_px: 0.,
                    moment_fwhm_px: 0.,
                    eccentricity: 0.,
                    flux_adu: 0.,
                    peak_adu: 0.,
                    snr: 0.,
                }
            })
            .collect();
        return Ok(Metrics {
            background: sky.background,
            noise: sky.noise,
            detected_peaks,
            detected_stars,
            hot_pixels,
            defect_detections,
            stars,
            dense_catalog: vec![],
        });
    }
    let wants_dense = limit == FIELD_CATALOG && is_dense(detected_stars);
    let limit = if limit == FIELD_CATALOG {
        CATALOG_STARS
    } else {
        limit
    };
    // 64 candidates per cell; a longer list than 16 cells of 64 takes more.
    let per_cell = limit.div_ceil(16).max(64);
    for bucket in &mut buckets {
        bucket.sort_unstable_by(|a, b| b.0.total_cmp(&a.0).then(a.2.cmp(&b.2)).then(a.1.cmp(&b.1)));
        bucket.truncate(per_cell);
    }
    let plane = match pattern {
        Some(pattern) => Green::new(pixels, width, height, pattern),
        None => Green::mono(pixels, width, height),
    };
    let measure = |x: usize, y: usize| {
        let (cx, cy) = (2.0 * x as f64 + 0.5, 2.0 * y as f64 + 0.5);
        let mut ring = Vec::new();
        for yy in (cy as usize - 10)..=(cy as usize + 10) {
            for xx in (cx as usize - 10)..=(cx as usize + 10) {
                let r2 = (xx as f64 - cx).powi(2) + (yy as f64 - cy).powi(2);
                if (49.0..=100.0).contains(&r2)
                    && pattern.is_none_or(|p| p.color(xx, yy) == Color::Green)
                {
                    ring.push(f64::from(pixels[yy * width + xx]));
                }
            }
        }
        let local = median(&mut ring);
        plane.measure(cx, cy, local, pixel_noise)
    };
    let mut stars: Vec<Star> = Vec::new();
    for rank in 0..per_cell {
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
            if let Some(star) = measure(x, y) {
                stars.push(star);
                if stars.len() == limit {
                    break;
                }
            }
        }
        if stars.len() == limit {
            break;
        }
    }
    let dense_catalog = if wants_dense {
        let bright = flux_stars(
            &grid,
            (gw, gh),
            pixels,
            width,
            &sky,
            saturated_above,
            cancel,
        )?;
        self::dense_catalog(&bright, (width, height), measure)
    } else {
        vec![]
    };
    Ok(Metrics {
        background: sky.background,
        noise: sky.noise,
        detected_peaks,
        detected_stars,
        hot_pixels,
        defect_detections,
        stars,
        dense_catalog,
    })
}

#[cfg(test)]
#[path = "stars_tests.rs"]
mod tests;
