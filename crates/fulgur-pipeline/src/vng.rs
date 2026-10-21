//! Unclipped threshold-based variable-number-of-gradients reconstruction.
//!
//! Independent geometric formulation of the Chang/Cheung/Pan VNG equations.
//! Generate directional stencils by rotation/reflection instead of copying
//! another implementation's lookup tables. See ADR-012 for method/evidence.

use crate::debayer::reflect;
use fulgur_core::{
    BayerPattern, Color, DemosaicMethod, Error, RawFrame, Result, RgbImage, check_cancelled,
};
use rayon::prelude::*;
use std::sync::atomic::AtomicBool;

type Point = (isize, isize);

struct Direction {
    differences: Vec<(usize, usize, f32)>,
    samples: Vec<(usize, usize, f32)>,
}

fn index((x, y): Point) -> usize {
    ((y + 2) * 5 + x + 2) as usize
}

fn channel(color: Color) -> usize {
    match color {
        Color::Red => 0,
        Color::Green => 1,
        Color::Blue => 2,
    }
}

fn direction(pattern: BayerPattern, phase: usize, d: Point) -> Direction {
    let (x, y) = (phase % 2, phase / 2);
    let green = pattern.color(x, y) == Color::Green;
    let (dx, dy) = d;
    let double = (2 * dx, 2 * dy);
    let mut pairs = vec![(d, (-dx, -dy), 1.0), (double, (0, 0), 1.0)];
    let mut points;
    if dx == 0 || dy == 0 {
        let (px, py) = (-dy, dx);
        points = vec![(0, 0), d, double];
        for sign in [-1, 1] {
            let side = (sign * px, sign * py);
            let near = (dx + side.0, dy + side.1);
            let far = (2 * dx + side.0, 2 * dy + side.1);
            pairs.push((near, (-dx + side.0, -dy + side.1), 0.5));
            pairs.push((far, side, 0.5));
            if green {
                points.extend([side, far]);
            } else {
                points.push(near);
            }
        }
    } else {
        points = vec![d, (dx, 0), (0, dy), (2 * dx, dy), (dx, 2 * dy)];
        if green {
            pairs.extend([((dx, 2 * dy), (-dx, 0), 1.0), ((2 * dx, dy), (0, -dy), 1.0)]);
        } else {
            points.extend([(0, 0), double]);
            pairs.extend([
                ((0, dy), (-dx, 0), 0.5),
                ((dx, 0), (0, -dy), 0.5),
                ((dx, 2 * dy), (0, dy), 0.5),
                ((2 * dx, dy), (dx, 0), 0.5),
            ]);
        }
    }
    let color_at = |(dx, dy): Point| {
        channel(pattern.color(
            (x as isize + dx).rem_euclid(2) as usize,
            (y as isize + dy).rem_euclid(2) as usize,
        ))
    };
    let mut counts = [0; 3];
    for &point in &points {
        counts[color_at(point)] += 1;
    }
    debug_assert!(counts.iter().all(|&n| n > 0));
    Direction {
        differences: pairs
            .into_iter()
            .map(|(a, b, w)| (index(a), index(b), w))
            .collect(),
        samples: points
            .into_iter()
            .map(|p| {
                let color = color_at(p);
                (index(p), color, 1.0 / counts[color] as f32)
            })
            .collect(),
    }
}

pub fn vng_demosaic(frame: &RawFrame, cfa: &[f32], cancel: &AtomicBool) -> Result<RgbImage> {
    let width = frame.header.info.width;
    let height = frame.header.info.height;
    let pattern = frame.header.info.bayer_pattern;
    if width < 6 || height < 6 || width.checked_mul(height) != Some(cfa.len()) {
        return Err(Error::Invalid("Invalid CFA reconstruction dimensions."));
    }
    let directions = [
        (0, -1),
        (1, 0),
        (0, 1),
        (-1, 0),
        (1, -1),
        (1, 1),
        (-1, -1),
        (-1, 1),
    ];
    let plans: [[Direction; 8]; 4] = std::array::from_fn(|phase| {
        std::array::from_fn(|i| direction(pattern, phase, directions[i]))
    });
    let mut pixels = vec![[0.0; 3]; cfa.len()];
    pixels
        .par_chunks_mut(width)
        .enumerate()
        .try_for_each(|(y, row)| -> Result<()> {
            check_cancelled(cancel)?;
            for (x, pixel) in row.iter_mut().enumerate() {
                let phase = (y % 2) * 2 + x % 2;
                let mut window = [0.0; 25];
                for dy in -2..=2 {
                    for dx in -2..=2 {
                        window[index((dx, dy))] = cfa[reflect(y as isize + dy, height) * width
                            + reflect(x as isize + dx, width)];
                    }
                }
                let gradients: [f32; 8] = std::array::from_fn(|i| {
                    plans[phase][i]
                        .differences
                        .iter()
                        .map(|&(a, b, w)| w * (window[a] - window[b]).abs())
                        .sum()
                });
                let min = gradients.iter().copied().fold(f32::INFINITY, f32::min);
                let max = gradients.iter().copied().fold(0.0, f32::max);
                // 1.5 * min + 0.5 * (max - min), including all tied/flat directions.
                let threshold = min + 0.5 * max;
                let mut sums = [0.0; 3];
                let mut count = 0;
                for (plan, gradient) in plans[phase].iter().zip(gradients) {
                    if gradient <= threshold {
                        for &(sample, color, weight) in &plan.samples {
                            sums[color] += window[sample] * weight;
                        }
                        count += 1;
                    }
                }
                let measured = channel(pattern.color(x, y));
                for color in 0..3 {
                    pixel[color] = window[12] + (sums[color] - sums[measured]) / count as f32;
                }
                // No output clamp: measured samples and all interpolation excursions survive.
                pixel[measured] = window[12];
            }
            Ok(())
        })?;
    Ok(RgbImage {
        width,
        height,
        demosaic_method: DemosaicMethod::Vng,
        pixels,
    })
}
