//! Frozen scalar cosmetic implementation preceding the M5 optimization.

use fulgur_core::{Error, RawFrame, Result, check_cancelled};
use rayon::prelude::*;
use std::sync::atomic::AtomicBool;

fn median8(values: &mut [f32; 8]) -> f32 {
    values.sort_unstable_by(f32::total_cmp);
    (values[3] + values[4]) * 0.5
}

fn neighborhood(pixels: &[f32], width: usize, x: usize, y: usize) -> (f32, f32) {
    let mut neighbors = [0.0; 8];
    let mut index = 0;
    for dy in [-2_isize, 0, 2] {
        for dx in [-2_isize, 0, 2] {
            if dx != 0 || dy != 0 {
                neighbors[index] =
                    pixels[(y as isize + dy) as usize * width + (x as isize + dx) as usize];
                index += 1;
            }
        }
    }
    let median = median8(&mut neighbors);
    let mut deviations = neighbors.map(|value| (value - median).abs());
    // One ADU prevents a zero threshold in quantized, locally constant backgrounds.
    let noise = (1.4826 * median8(&mut deviations)).max(1.0);
    (median, noise)
}

fn has_stellar_support(
    pixels: &[f32],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    candidate_excess: f32,
) -> bool {
    let mut supported = 0;
    for (dx, dy) in [(-1_isize, 0_isize), (1, 0), (0, -1), (0, 1)] {
        let nx = (x as isize + dx) as usize;
        let ny = (y as isize + dy) as usize;
        if nx < 2 || ny < 2 || nx + 2 >= width || ny + 2 >= height {
            continue;
        }
        let (local, noise) = neighborhood(pixels, width, nx, ny);
        // A star's slope/flat saturated core inflates a neighbor's MAD. Cap
        // that threshold relative to the candidate's excess so coherent PSF
        // support is not mistaken for noise. Isolated defects on flat sky
        // still have no supporting neighbors. Prefer retaining an ambiguous
        // defect over replacing a real stellar sample with a background value.
        let threshold = (3.0 * noise).min(0.125 * candidate_excess);
        if pixels[ny * width + nx] > local + threshold {
            supported += 1;
        }
    }
    supported >= 2
}

fn refined_noise(pixels: &[f32], width: usize, x: usize, y: usize) -> f32 {
    // Eight samples give an unstable MAD on ordinary noise. Confirm candidates
    // against 24 same-phase neighbors before altering faint astronomical data.
    let mut neighbors = [0.0_f32; 24];
    let mut index = 0;
    for dy in [-4_isize, -2, 0, 2, 4] {
        for dx in [-4_isize, -2, 0, 2, 4] {
            if dx != 0 || dy != 0 {
                neighbors[index] =
                    pixels[(y as isize + dy) as usize * width + (x as isize + dx) as usize];
                index += 1;
            }
        }
    }
    neighbors.sort_unstable_by(f32::total_cmp);
    let median = 0.5 * (neighbors[11] + neighbors[12]);
    let mut deviations = neighbors.map(|value| (value - median).abs());
    deviations.sort_unstable_by(f32::total_cmp);
    (1.4826 * 0.5 * (deviations[11] + deviations[12])).max(1.0)
}

pub fn cosmetic_correct(frame: &RawFrame, cancel: &AtomicBool) -> Result<(Vec<f32>, usize)> {
    let width = frame.header.info.width;
    let height = frame.header.info.height;
    if width.checked_mul(height) != Some(frame.pixels.len()) || width < 6 || height < 6 {
        return Err(Error::Invalid("Invalid CFA buffer dimensions."));
    }
    let mut cleaned = vec![0.0; frame.pixels.len()];
    let corrected = cleaned
        .par_chunks_mut(width)
        .enumerate()
        .map(|(y, row)| {
            check_cancelled(cancel)?;
            let mut count = 0;
            for (x, destination) in row.iter_mut().enumerate() {
                let original = frame.pixels[y * width + x];
                *destination = original;
                if x < 4 || y < 4 || x + 4 >= width || y + 4 >= height {
                    continue;
                }
                let (median, noise) = neighborhood(&frame.pixels, width, x, y);
                if original > median + 5.0 * noise
                    && original
                        > median + 5.0 * noise.max(refined_noise(&frame.pixels, width, x, y))
                    && !has_stellar_support(&frame.pixels, width, height, x, y, original - median)
                {
                    *destination = median;
                    count += 1;
                }
            }
            Ok(count)
        })
        .collect::<Result<Vec<usize>>>()?
        .into_iter()
        .sum();
    Ok((cleaned, corrected))
}
