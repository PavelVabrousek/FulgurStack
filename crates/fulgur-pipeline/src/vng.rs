//! Threshold-based VNG reconstruction before final export sensor-range bounding.
//!
//! Independent geometric formulation of the Chang/Cheung/Pan VNG equations.
//! Generate directional stencils by rotation/reflection instead of copying
//! another implementation's lookup tables. See ADR-012 for method/evidence.

use crate::debayer::reflect;
use fulgur_core::{Color, DemosaicMethod, Error, RawFrame, Result, RgbImage, check_cancelled};
use rayon::prelude::*;
use std::sync::atomic::AtomicBool;

#[cfg(feature = "gpu")]
#[path = "vng_stencil.rs"]
mod stencil;
#[cfg(feature = "gpu")]
use fulgur_core::BayerPattern;
#[cfg(feature = "gpu")]
use stencil::direction;

#[cfg(feature = "gpu")]
fn channel(color: Color) -> usize {
    match color {
        Color::Red => 0,
        Color::Green => 1,
        Color::Blue => 2,
    }
}

#[cfg(feature = "gpu")]
fn pattern_channels(pattern: BayerPattern) -> [usize; 4] {
    std::array::from_fn(|i| channel(pattern.color(i % 2, i / 2)))
}

include!(concat!(env!("OUT_DIR"), "/vng_kernels.rs"));

/// Pack the same independently generated CPU stencils for the GPU, without duplicated coefficients.
#[cfg(feature = "gpu")]
pub fn gpu_plans() -> Vec<u32> {
    let mut packed = Vec::new();
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
    for pattern in [
        BayerPattern::GRBG,
        BayerPattern::RGGB,
        BayerPattern::GBRG,
        BayerPattern::BGGR,
    ] {
        for phase in 0..4 {
            for d in directions {
                let plan = direction(pattern_channels(pattern), phase, d);
                let mut words = [0u32; 68];
                words[0] = plan.differences.len() as u32;
                words[1] = plan.samples.len() as u32;
                for (i, (a, b, w)) in plan.differences.iter().enumerate() {
                    let k = 4 + i * 4;
                    words[k] = *a as u32;
                    words[k + 1] = *b as u32;
                    words[k + 2] = w.to_bits();
                }
                for (i, (a, c, w)) in plan.samples.iter().enumerate() {
                    let k = 36 + i * 4;
                    words[k] = *a as u32;
                    words[k + 1] = *c as u32;
                    words[k + 2] = w.to_bits();
                }
                packed.extend(words);
            }
        }
    }
    packed
}

#[path = "vng_simd.rs"]
mod simd;

pub fn vng_demosaic(frame: &RawFrame, cfa: &[f32], cancel: &AtomicBool) -> Result<RgbImage> {
    vng_demosaic_into(frame, cfa, cancel, Vec::new())
}

/// As `vng_demosaic`, writing into reused storage. Every output pixel is
/// reconstructed, so earlier contents of `pixels` never leak through.
pub(crate) fn vng_demosaic_into(
    frame: &RawFrame,
    cfa: &[f32],
    cancel: &AtomicBool,
    pixels: Vec<[f32; 3]>,
) -> Result<RgbImage> {
    vng_demosaic_with(frame, cfa, cancel, &simd::Kernel::detect(), pixels, None)
}

/// As `vng_demosaic_into`, reconstructing only columns `[from, to)` of each
/// row. Other pixels keep whatever `pixels` held.
pub(crate) fn vng_demosaic_spans(
    frame: &RawFrame,
    cfa: &[f32],
    cancel: &AtomicBool,
    pixels: Vec<[f32; 3]>,
    spans: Option<&[[usize; 2]]>,
) -> Result<RgbImage> {
    vng_demosaic_with(frame, cfa, cancel, &simd::Kernel::detect(), pixels, spans)
}

fn vng_demosaic_with(
    frame: &RawFrame,
    cfa: &[f32],
    cancel: &AtomicBool,
    kernel: &simd::Kernel,
    mut pixels: Vec<[f32; 3]>,
    spans: Option<&[[usize; 2]]>,
) -> Result<RgbImage> {
    let width = frame.header.info.width;
    let height = frame.header.info.height;
    let pattern = crate::debayer::bayer_pattern(frame)?;
    if width < 6 || height < 6 || width.checked_mul(height) != Some(cfa.len()) {
        return Err(Error::Invalid("Invalid CFA reconstruction dimensions."));
    }
    // Rows outside the vector kernel's finite, bounded domain stay scalar.
    let bounded: Vec<bool> = if kernel.vector {
        cfa.par_chunks(width).map(simd::bounded).collect()
    } else {
        vec![false; height]
    };
    pixels.resize(cfa.len(), [0.0; 3]);
    pixels
        .par_chunks_mut(width)
        .enumerate()
        .try_for_each(|(y, row)| -> Result<()> {
            check_cancelled(cancel)?;
            let span = spans.map_or([0, width], |spans| spans[y]);
            if span[0] >= span[1] {
                return Ok(());
            }
            // Reflection in y is shared across the row; x reflection is only
            // needed at its first/last two pixels. Internal ROI halos stay unchanged.
            let sources: [usize; 5] =
                std::array::from_fn(|i| reflect(y as isize + i as isize - 2, height));
            let rows: [&[f32]; 5] = sources.map(|yy| &cfa[yy * width..(yy + 1) * width]);
            let vector = sources.iter().all(|&yy| bounded[yy]).then_some(kernel);
            let center = pattern.color(0, y);
            let right = pattern.color(1, y);
            match (center, right) {
                (Color::Green, Color::Red) => reconstruct_row::<0, 1>(&rows, row, vector, span),
                (Color::Red, Color::Green) => reconstruct_row::<1, 0>(&rows, row, vector, span),
                (Color::Green, Color::Blue) => reconstruct_row::<2, 3>(&rows, row, vector, span),
                (Color::Blue, Color::Green) => reconstruct_row::<3, 2>(&rows, row, vector, span),
                _ => unreachable!("A Bayer row alternates green and red/blue."),
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

#[inline]
fn interior_window(rows: &[&[f32]; 5], x: usize) -> [f32; 25] {
    let mut window = [0.; 25];
    for (j, row) in rows.iter().enumerate() {
        window[j * 5..j * 5 + 5].copy_from_slice(&row[x - 2..x + 3]);
    }
    window
}

fn reflected_window(rows: &[&[f32]; 5], x: usize, width: usize) -> [f32; 25] {
    std::array::from_fn(|i| rows[i / 5][reflect(x as isize + (i % 5) as isize - 2, width)])
}

fn reconstruct_row<const LEFT: usize, const RIGHT: usize>(
    rows: &[&[f32]; 5],
    row: &mut [[f32; 3]],
    vector: Option<&simd::Kernel>,
    [from, to]: [usize; 2],
) {
    let width = row.len();
    if from < 2 {
        row[0] = reconstruct::<LEFT>(&reflected_window(rows, 0, width));
        row[1] = reconstruct::<RIGHT>(&reflected_window(rows, 1, width));
    }
    // Pairs keep their colour phase, so the interior starts and ends on even
    // columns; two more columns on each side give its 5x5 windows.
    let first = (from / 2 * 2).max(2);
    let last = (to.div_ceil(2) * 2).min(width - 2);
    if first < last {
        let bounds = first - 2..last + 2;
        let interior = rows.map(|r| &r[bounds.clone()]);
        reconstruct_interior::<LEFT, RIGHT>(&interior, &mut row[bounds], vector);
    }
    if to > width - 2 {
        if width % 2 == 0 {
            row[width - 2] = reconstruct::<LEFT>(&reflected_window(rows, width - 2, width));
            row[width - 1] = reconstruct::<RIGHT>(&reflected_window(rows, width - 1, width));
        } else {
            row[width - 2] = reconstruct::<RIGHT>(&reflected_window(rows, width - 2, width));
            row[width - 1] = reconstruct::<LEFT>(&reflected_window(rows, width - 1, width));
        }
    }
}

/// Columns 2..width - 2 of the given columns, column 2 being a LEFT colour.
fn reconstruct_interior<const LEFT: usize, const RIGHT: usize>(
    rows: &[&[f32]; 5],
    row: &mut [[f32; 3]],
    vector: Option<&simd::Kernel>,
) {
    let width = row.len();
    // Whole groups of interior pairs may be vectorized; scalar finishes the rest.
    let done = vector.map_or(0, |kernel| kernel.pairs::<LEFT, RIGHT>(rows, row));
    let mut pairs = row[2 + 2 * done..width - 2].chunks_exact_mut(2);
    for (i, pair) in pairs.by_ref().enumerate() {
        let x = 2 + (done + i) * 2;
        pair[0] = reconstruct::<LEFT>(&interior_window(rows, x));
        pair[1] = reconstruct::<RIGHT>(&interior_window(rows, x + 1));
    }
    if let Some(last) = pairs.into_remainder().first_mut() {
        *last = reconstruct::<LEFT>(&interior_window(rows, width - 3));
    }
}

#[cfg(test)]
#[path = "vng_reference.rs"]
mod reference;
#[cfg(test)]
#[path = "vng_tests.rs"]
mod tests;
