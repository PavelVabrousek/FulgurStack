//! Drizzle integration (ADR-188), in the same single pass over output tiles as
//! resampling: no drizzle files, no second reading of the frames.
//!
//! Output pixels are 1/`scale` of a reference pixel. Every input pixel is a
//! square drop of `pixfrac` pixels around its centre. For each output pixel the
//! overlap of its footprint (mapped into the frame) with the drops around it is
//! computed exactly by polygon clipping; the frame's sample is the
//! overlap-weighted mean of those pixels, and its weight is the covered share
//! of the output pixel. Pull and push give the same sums; pulling keeps the
//! tile structure, the bounded memory and the parallel reconstruction.
//!
//! Bayer drizzle: a colour frame is not demosaiced. Each calibrated,
//! cosmetic-corrected CFA sample drops only into its own channel, so a channel
//! without a drop on an output pixel has no sample there (NaN), and the stack
//! of many dithered frames fills it.
//!
//! A tile buffer holds the samples of its pixels followed by their weights.

use super::{
    ClippingScratch, Drizzle, IntegratedPixel, IntegrationSource, ReductionContext, RoiScratch,
    RoiTaskElapsed, RowStats, Sum, WarpedTile, rgb_rejection, source_bounds,
};
use fulgur_core::fits_cache::FitsBlockCache;
use fulgur_core::{Color, DemosaicMethod, Error, Mosaic, Result, RgbImage, check_cancelled};
use rayon::prelude::*;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

/// The reference coordinate of output pixel `u` on a grid `scale` times finer.
#[inline]
pub(super) fn reference_coordinate(u: f64, scale: f64) -> f64 {
    (u + 0.5) / scale - 0.5
}

/// Reconstructs one frame's samples and weights on the drizzle tile at
/// `origin` (in output pixels). `output` may hold a previous tile's buffer.
#[allow(clippy::too_many_arguments)]
pub(super) fn warp(
    source: &IntegrationSource,
    origin: [i32; 2],
    size: [usize; 2],
    drizzle: Drizzle,
    cancel: &AtomicBool,
    cache: &FitsBlockCache,
    scratch: &mut RoiScratch,
    mut output: Vec<[f32; 3]>,
) -> Result<WarpedTile> {
    let pixels = size[0] * size[1];
    output.clear();
    output.resize(pixels, [f32::NAN; 3]);
    output.resize(2 * pixels, [0.; 3]);
    let scale = f64::from(drizzle.scale);
    let sensor = [source.header.info.width, source.header.info.height];
    // The reference pixels the tile covers, with a pixel of margin.
    let first = [
        reference_coordinate(f64::from(origin[0]), scale).floor() as i32 - 1,
        reference_coordinate(f64::from(origin[1]), scale).floor() as i32 - 1,
    ];
    let span = [
        (size[0] as f64 / scale).ceil() as usize + 3,
        (size[1] as f64 / scale).ceil() as usize + 3,
    ];
    let Some(roi) = source_bounds(source, first, span, sensor) else {
        return Ok((output, 0, 0, RoiTaskElapsed::default()));
    };
    let read_start = Instant::now();
    let (rgb, corrected, cosmetic_seconds, demosaic_seconds, read_seconds) =
        if source.header.info.mosaic == Mosaic::Rgb {
            let rgb = fulgur_core::fits::read_rgb_roi(
                &source.header,
                roi,
                cancel,
                std::mem::take(&mut scratch.debayer.rgb),
            )?;
            (rgb, 0, 0., 0., read_start.elapsed().as_secs_f64())
        } else {
            let mut raw = cache.read_roi_into(
                &source.path,
                &source.header,
                roi,
                cancel,
                std::mem::take(&mut scratch.raw),
            )?;
            if let Some(calibration) = &source.calibration {
                calibration.apply(&mut raw, [roi[0], roi[1]])?;
            }
            let read_seconds = read_start.elapsed().as_secs_f64();
            let bayer = source.header.info.mosaic == Mosaic::Bayer && drizzle.bayer;
            // Bayer drizzle: cosmetic correction only, each sample in its own
            // channel; otherwise the usual VNG reconstruction.
            let method = if bayer {
                DemosaicMethod::None
            } else {
                DemosaicMethod::Vng
            };
            let (mut rgb, stats) =
                crate::debayer::debayer_spans(&raw, cancel, method, &mut scratch.debayer, None)?;
            scratch.raw = raw.pixels;
            if bayer {
                let pattern = source.header.info.bayer_pattern.ok_or(Error::Invalid(
                    "Bayer drizzle needs the frame's Bayer pattern.",
                ))?;
                for (y, row) in rgb.pixels.chunks_exact_mut(roi[2]).enumerate() {
                    for (x, pixel) in row.iter_mut().enumerate() {
                        let keep = match pattern.color(roi[0] + x, roi[1] + y) {
                            Color::Red => 0,
                            Color::Green => 1,
                            Color::Blue => 2,
                        };
                        for (c, value) in pixel.iter_mut().enumerate() {
                            if c != keep {
                                *value = f32::NAN;
                            }
                        }
                    }
                }
            }
            (
                rgb,
                stats.corrected_pixels as u64,
                stats.cosmetic_seconds,
                stats.demosaic_seconds,
                read_seconds,
            )
        };
    let resampling_start = Instant::now();
    let frame = Frame {
        base: source.distortion.map_or(source.transform, |m| m.base),
        distortion: source.distortion.as_ref(),
        sensor: [source.header.info.width, source.header.info.height],
        scale: source.scale,
        offset: source.offset,
    };
    drop_into(
        &frame,
        origin,
        size,
        roi,
        &rgb,
        drizzle,
        cancel,
        &mut output,
    )?;
    scratch.debayer.rgb = rgb.pixels;
    Ok((
        output,
        (roi[2] * roi[3]) as u64,
        corrected,
        RoiTaskElapsed {
            read_seconds,
            cosmetic_seconds,
            demosaic_seconds,
            resampling_seconds: resampling_start.elapsed().as_secs_f64(),
        },
    ))
}

/// Area of the convex quadrilateral `quad` inside the square [-h, h]².
fn clipped_area(quad: &[[f64; 2]; 4], h: f64) -> f64 {
    let mut a = [[0.; 2]; 8];
    let mut b = [[0.; 2]; 8];
    a[..4].copy_from_slice(quad);
    let mut n = 4;
    // Each edge of the square in turn: keep the side `inside`.
    for edge in 0..4 {
        let inside = |p: [f64; 2]| match edge {
            0 => p[0] >= -h,
            1 => p[0] <= h,
            2 => p[1] >= -h,
            _ => p[1] <= h,
        };
        let cross = |p: [f64; 2], q: [f64; 2]| {
            let (axis, bound) = match edge {
                0 => (0, -h),
                1 => (0, h),
                2 => (1, -h),
                _ => (1, h),
            };
            let t = (bound - p[axis]) / (q[axis] - p[axis]);
            [p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1])]
        };
        let mut m = 0;
        for i in 0..n {
            let (p, q) = (a[i], a[(i + 1) % n]);
            match (inside(p), inside(q)) {
                (true, true) => {
                    b[m] = q;
                    m += 1;
                }
                (true, false) => {
                    b[m] = cross(p, q);
                    m += 1;
                }
                (false, true) => {
                    b[m] = cross(p, q);
                    b[m + 1] = q;
                    m += 2;
                }
                (false, false) => {}
            }
        }
        if m < 3 {
            return 0.;
        }
        a[..m].copy_from_slice(&b[..m]);
        n = m;
    }
    let mut twice = 0.;
    for i in 0..n {
        let (p, q) = (a[i], a[(i + 1) % n]);
        twice += p[0] * q[1] - q[0] * p[1];
    }
    (twice * 0.5).abs()
}

/// What `drop_into` needs of a frame: its mapping to the reference grid
/// (similarity, and the cubic model when distorted), sensor size and
/// photometric normalization y = scale * x + offset[c].
struct Frame<'a> {
    base: crate::geometry::Transform,
    distortion: Option<&'a crate::distortion::Warp>,
    sensor: [usize; 2],
    scale: f64,
    offset: [f64; 3],
}

/// Drops the pixels of `rgb` (the frame's region `roi`) onto the tile.
#[allow(clippy::too_many_arguments)]
fn drop_into(
    source: &Frame<'_>,
    origin: [i32; 2],
    size: [usize; 2],
    roi: [usize; 4],
    rgb: &RgbImage,
    drizzle: Drizzle,
    cancel: &AtomicBool,
    output: &mut [[f32; 3]],
) -> Result<()> {
    let scale = f64::from(drizzle.scale);
    let sensor = source.sensor;
    let base = source.base;
    let inverse = base.inverse();
    let o = inverse.apply([0., 0.]);
    let ex = {
        let p = inverse.apply([1., 0.]);
        [p[0] - o[0], p[1] - o[1]]
    };
    let ey = {
        let p = inverse.apply([0., 1.]);
        [p[0] - o[0], p[1] - o[1]]
    };
    // The output pixel's corners around its centre, in frame pixels.
    let half = 0.5 / scale;
    let corners = [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)].map(|(a, b)| {
        [
            half * (a * ex[0] + b * ey[0]),
            half * (a * ex[1] + b * ey[1]),
        ]
    });
    let reach = [
        corners.iter().map(|c| c[0].abs()).fold(0., f64::max),
        corners.iter().map(|c| c[1].abs()).fold(0., f64::max),
    ];
    let footprint = (ex[0] * ey[1] - ex[1] * ey[0]).abs() / (scale * scale);
    let h = drizzle.pixfrac * 0.5;
    let rotation = base.theta_rad.sin_cos();
    let aligned = (ex[1].abs() + ey[0].abs() <= 0.01 * (ex[0].abs() + ey[1].abs()))
        .then(|| [half * ex[0].hypot(ex[1]), half * ey[0].hypot(ey[1])]);
    let (values, weights) = output.split_at_mut(size[0] * size[1]);
    for y in 0..size[1] {
        check_cancelled(cancel)?;
        let ry = reference_coordinate(f64::from(origin[1]) + y as f64, scale);
        for x in 0..size[0] {
            let rx = reference_coordinate(f64::from(origin[0]) + x as f64, scale);
            let centre = match source.distortion {
                Some(model) => match model.inverse_with_rotation([rx, ry], rotation) {
                    Some(p) => p,
                    None => continue,
                },
                None => inverse.apply([rx, ry]),
            };
            let i0 = (centre[0] - reach[0] - h).ceil().max(0.) as usize;
            let i1 = (centre[0] + reach[0] + h).floor();
            let j0 = (centre[1] - reach[1] - h).ceil().max(0.) as usize;
            let j1 = (centre[1] + reach[1] + h).floor();
            if i1 < 0. || j1 < 0. {
                continue;
            }
            let i1 = (i1 as usize).min(sensor[0] - 1).min(roi[0] + roi[2] - 1);
            let j1 = (j1 as usize).min(sensor[1] - 1).min(roi[1] + roi[3] - 1);
            let (i0, j0) = (i0.max(roi[0]), j0.max(roi[1]));
            let mut sum = [0.; 3];
            let mut area = [0.; 3];
            for j in j0..=j1 {
                for i in i0..=i1 {
                    let a = match aligned {
                        // A frame turned by under 0.6 degrees: the footprint is
                        // a rectangle to within 0.005 px, and the overlap
                        // separates into two intervals.
                        Some([hx, hy]) => {
                            let (fi, fj) = (i as f64, j as f64);
                            let ox = ((centre[0] + hx).min(fi + h) - (centre[0] - hx).max(fi - h))
                                .max(0.);
                            let oy = ((centre[1] + hy).min(fj + h) - (centre[1] - hy).max(fj - h))
                                .max(0.);
                            ox * oy
                        }
                        None => {
                            let rel = [0, 1, 2, 3].map(|k| {
                                [
                                    centre[0] + corners[k][0] - i as f64,
                                    centre[1] + corners[k][1] - j as f64,
                                ]
                            });
                            clipped_area(&rel, h)
                        }
                    };
                    if a <= 0. {
                        continue;
                    }
                    let pixel = rgb.pixels[(j - roi[1]) * rgb.width + i - roi[0]];
                    for c in 0..3 {
                        if pixel[c].is_finite() {
                            sum[c] += a * f64::from(pixel[c]);
                            area[c] += a;
                        }
                    }
                }
            }
            let at = y * size[0] + x;
            for c in 0..3 {
                if area[c] > 0. {
                    values[at][c] = (source.scale * (sum[c] / area[c]) + source.offset[c]) as f32;
                    weights[at][c] = (area[c] / footprint) as f32;
                }
            }
        }
    }
    Ok(())
}

/// Reduces a drizzle tile: per pixel and channel, the rejection method on the
/// frames' samples, then the mean weighted by frame weight × drop coverage.
pub(super) fn reduce_tile(
    frames: &[Vec<[f32; 3]>],
    tile: &mut Vec<IntegratedPixel>,
    size: [usize; 2],
    context: &ReductionContext<'_>,
) -> Result<Vec<RowStats>> {
    let ReductionContext {
        weights,
        original_input_count,
        total_weight,
        config,
        cancel,
    } = *context;
    let pixels = size[0] * size[1];
    tile.clear();
    tile.resize(pixels, ([0.; 3], [0.; 3], [0.; 3]));
    tile.par_chunks_mut(size[0])
        .enumerate()
        .map(|(y, row)| -> Result<RowStats> {
            check_cancelled(cancel)?;
            let mut stats = RowStats::new(frames.len());
            let mut scratch = ClippingScratch::with_capacity(frames.len());
            let mut values: [Vec<f32>; 3] = Default::default();
            let mut sample_weights: [Vec<f64>; 3] = Default::default();
            for (x, pixel) in row.iter_mut().enumerate() {
                let p = y * size[0] + x;
                let mut covered = Sum::default();
                for c in 0..3 {
                    values[c].clear();
                    sample_weights[c].clear();
                }
                for (frame, &w) in frames.iter().zip(weights) {
                    let (v, coverage) = (frame[p], frame[pixels + p]);
                    if v[0].is_finite() || v[1].is_finite() || v[2].is_finite() {
                        covered.add(w);
                    }
                    for c in 0..3 {
                        if v[c].is_finite() && coverage[c] > 0. {
                            values[c].push(v[c]);
                            sample_weights[c].push(w * f64::from(coverage[c]));
                        }
                    }
                }
                let n = values[1].len();
                stats.min_n = stats.min_n.min(n);
                stats.max_n = stats.max_n.max(n);
                stats.min_fraction = stats.min_fraction.min(covered.value() / total_weight);
                if values.iter().all(Vec::is_empty) {
                    stats.zero += 1;
                }
                // A monochrome stack holds one plane in all three channels.
                let repeated = values[0] == values[1]
                    && values[1] == values[2]
                    && sample_weights[0] == sample_weights[1]
                    && sample_weights[1] == sample_weights[2];
                let mut combined = (0., 0, 0, 0.);
                for c in 0..3 {
                    if c == 0 || !repeated {
                        combined = rgb_rejection::combine_dense(
                            &values[c],
                            &sample_weights[c],
                            &mut scratch,
                            config,
                        );
                    }
                    let (value, low, high, surviving) = combined;
                    let count = values[c].len() as u64;
                    pixel.0[c] = value;
                    pixel.1[c] = ((count - low - high) as f64 / original_input_count as f64) as f32;
                    pixel.2[c] = (high as f64 / original_input_count as f64) as f32;
                    stats.valid += count;
                    stats.low += low;
                    stats.high += high;
                    stats.min_surviving = stats.min_surviving.min(surviving / total_weight);
                }
            }
            Ok(stats)
        })
        .collect()
}

#[cfg(test)]
#[path = "drizzle_tests.rs"]
mod tests;
