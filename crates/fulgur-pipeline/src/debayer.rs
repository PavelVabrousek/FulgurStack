//! Independently implemented MHC equations from Malvar, He & Cutler (2004).
//! https://www.microsoft.com/en-us/research/wp-content/uploads/2016/02/Demosaicing_ICASSP04.pdf

use fulgur_core::{
    Color, DemosaicMethod, Error, Mosaic, RawFrame, Result, RgbImage, check_cancelled,
};
use rayon::prelude::*;
use serde::Serialize;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

#[derive(Debug, Serialize)]
pub struct DebayerStats {
    pub demosaic_method: DemosaicMethod,
    pub cosmetic_seconds: f64,
    pub demosaic_seconds: f64,
    pub corrected_pixels: usize,
}

fn median8(values: &mut [f32; 8]) -> f32 {
    values.sort_unstable_by(f32::total_cmp);
    (values[3] + values[4]) * 0.5
}

#[inline]
fn neighbors(pixels: &[f32], width: usize, x: usize, y: usize) -> [f32; 8] {
    let above = (y - 2) * width + x;
    let center = y * width + x;
    let below = (y + 2) * width + x;
    [
        pixels[above - 2],
        pixels[above],
        pixels[above + 2],
        pixels[center - 2],
        pixels[center + 2],
        pixels[below - 2],
        pixels[below],
        pixels[below + 2],
    ]
}

#[inline]
fn local_noise(neighbors: [f32; 8], median: f32) -> f32 {
    let mut deviations = neighbors.map(|value| (value - median).abs());
    // One ADU prevents a zero threshold in quantized, locally constant backgrounds.
    (1.4826 * median8(&mut deviations)).max(1.0)
}

fn neighborhood(pixels: &[f32], width: usize, x: usize, y: usize) -> (f32, f32) {
    let mut neighbors = neighbors(pixels, width, x, y);
    let median = median8(&mut neighbors);
    (median, local_noise(neighbors, median))
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
            if supported == 2 {
                return true;
            }
        }
    }
    false
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
    cosmetic_correct_into(frame, cancel, Vec::new())
}

/// As `cosmetic_correct`, writing into reused storage. The buffer first
/// receives an exact copy of the original CFA, so earlier contents never leak.
pub(crate) fn cosmetic_correct_into(
    frame: &RawFrame,
    cancel: &AtomicBool,
    cleaned: Vec<f32>,
) -> Result<(Vec<f32>, usize)> {
    cosmetic_correct_spans(frame, cancel, cleaned, None)
}

/// As `cosmetic_correct_into`, correcting only the given columns of each row.
fn cosmetic_correct_spans(
    frame: &RawFrame,
    cancel: &AtomicBool,
    mut cleaned: Vec<f32>,
    spans: Option<&[[usize; 2]]>,
) -> Result<(Vec<f32>, usize)> {
    let width = frame.header.info.width;
    let height = frame.header.info.height;
    if width.checked_mul(height) != Some(frame.pixels.len()) || width < 6 || height < 6 {
        return Err(Error::Invalid("Invalid CFA buffer dimensions."));
    }
    cleaned.clear();
    cleaned.extend_from_slice(&frame.pixels);
    let kernel = cosmetic_simd::Kernel::detect();
    let corrected = cleaned
        .par_chunks_mut(width)
        .enumerate()
        .map(|(y, row)| {
            check_cancelled(cancel)?;
            if y < 4 || y + 4 >= height || width <= 8 {
                return Ok(0);
            }
            let [from, to] = spans.map_or([0, width], |spans| spans[y]);
            let (from, to) = (from.max(4), to.min(width - 4));
            if from >= to {
                return Ok(0);
            }
            Ok(kernel.row(frame, y, row, from, to))
        })
        .collect::<Result<Vec<usize>>>()?
        .into_iter()
        .sum();
    Ok((cleaned, corrected))
}

#[path = "cosmetic_simd.rs"]
mod cosmetic_simd;

/// Demosaicing needs a declared Bayer pattern; mono and unknown layouts never
/// reach the CFA reconstructors.
pub(crate) fn bayer_pattern(frame: &RawFrame) -> Result<fulgur_core::BayerPattern> {
    match (frame.header.info.mosaic, frame.header.info.bayer_pattern) {
        (Mosaic::Bayer, Some(pattern)) => Ok(pattern),
        (Mosaic::Unknown, _) => Err(Error::Invalid(
            "Sensor layout is unknown; choose colour or mono for this light set.",
        )),
        _ => Err(Error::Invalid(
            "Demosaicing requires a colour (Bayer) frame.",
        )),
    }
}

#[inline]
pub(super) fn reflect(coordinate: isize, length: usize) -> usize {
    if coordinate < 0 {
        (-coordinate) as usize
    } else if coordinate >= length as isize {
        (2 * (length as isize - 1) - coordinate) as usize
    } else {
        coordinate as usize
    }
}

pub fn mhc_demosaic(frame: &RawFrame, cfa: &[f32], cancel: &AtomicBool) -> Result<RgbImage> {
    let width = frame.header.info.width;
    let height = frame.header.info.height;
    let pattern = bayer_pattern(frame)?;
    if width < 6 || height < 6 || width.checked_mul(height) != Some(cfa.len()) {
        return Err(Error::Invalid("Invalid CFA reconstruction dimensions."));
    }
    let mut pixels = vec![[0.0; 3]; cfa.len()];
    pixels
        .par_chunks_mut(width)
        .enumerate()
        .try_for_each(|(y, row)| -> Result<()> {
            check_cancelled(cancel)?;
            for (x, destination) in row.iter_mut().enumerate() {
                let sample = |dx: isize, dy: isize| {
                    cfa[reflect(y as isize + dy, height) * width + reflect(x as isize + dx, width)]
                };
                let center = sample(0, 0);
                let horizontal2 = sample(-2, 0) + sample(2, 0);
                let vertical2 = sample(0, -2) + sample(0, 2);
                let diagonals = sample(-1, -1) + sample(1, -1) + sample(-1, 1) + sample(1, 1);
                *destination = match pattern.color(x, y) {
                    Color::Red | Color::Blue => {
                        let green = (4.0 * center
                            + 2.0 * (sample(-1, 0) + sample(1, 0) + sample(0, -1) + sample(0, 1))
                            - horizontal2
                            - vertical2)
                            * 0.125;
                        let opposite = (6.0 * center + 2.0 * diagonals
                            - 1.5 * (horizontal2 + vertical2))
                            * 0.125;
                        if pattern.color(x, y) == Color::Red {
                            [center, green, opposite]
                        } else {
                            [opposite, green, center]
                        }
                    }
                    Color::Green => {
                        let horizontal = (5.0 * center + 4.0 * (sample(-1, 0) + sample(1, 0))
                            - horizontal2
                            - diagonals
                            + 0.5 * vertical2)
                            * 0.125;
                        let vertical = (5.0 * center + 4.0 * (sample(0, -1) + sample(0, 1))
                            - vertical2
                            - diagonals
                            + 0.5 * horizontal2)
                            * 0.125;
                        if pattern.color(x ^ 1, y) == Color::Red {
                            [horizontal, center, vertical]
                        } else {
                            [vertical, center, horizontal]
                        }
                    }
                };
            }
            Ok(())
        })?;
    Ok(RgbImage {
        width,
        height,
        demosaic_method: DemosaicMethod::Mhc,
        pixels,
    })
}

pub fn debayer(
    frame: &RawFrame,
    cancel: &AtomicBool,
    method: DemosaicMethod,
) -> Result<(RgbImage, DebayerStats)> {
    debayer_reusing(frame, cancel, method, &mut DebayerBuffers::default())
}

/// Reusable cosmetic CFA and RGB storage. Contents are always fully
/// overwritten before use; only the allocated capacity is carried over.
#[derive(Default)]
pub struct DebayerBuffers {
    pub cleaned: Vec<f32>,
    pub rgb: Vec<[f32; 3]>,
}

/// As `debayer`, reusing `buffers`. The returned image owns the RGB storage;
/// return it with `buffers.rgb = image.pixels` once the image is consumed.
///
/// Monochrome frames receive the same isolated hot-pixel correction (its
/// neighbourhoods are pattern independent) and skip demosaicing: every output
/// channel holds the corrected sample. Unknown layouts are refused.
pub(crate) fn debayer_reusing(
    frame: &RawFrame,
    cancel: &AtomicBool,
    method: DemosaicMethod,
    buffers: &mut DebayerBuffers,
) -> Result<(RgbImage, DebayerStats)> {
    debayer_spans(frame, cancel, method, buffers, None)
}

/// As `debayer_reusing`, reconstructing only the columns in `spans` of each
/// row; the rest of the returned image holds unspecified values.
pub(crate) fn debayer_spans(
    frame: &RawFrame,
    cancel: &AtomicBool,
    method: DemosaicMethod,
    buffers: &mut DebayerBuffers,
    spans: Option<&crate::roi_spans::RowSpans>,
) -> Result<(RgbImage, DebayerStats)> {
    if frame.header.info.mosaic == Mosaic::Unknown {
        bayer_pattern(frame)?;
    }
    if frame.header.info.mosaic == Mosaic::Rgb {
        return Err(Error::Invalid("This frame is already debayered."));
    }
    let mono = frame.header.info.mosaic == Mosaic::Mono;
    let start = Instant::now();
    let (cleaned, corrected_pixels) = cosmetic_correct_spans(
        frame,
        cancel,
        std::mem::take(&mut buffers.cleaned),
        spans.map(|spans| spans.cosmetic.as_slice()),
    )?;
    buffers.cleaned = cleaned;
    let cleaned = &buffers.cleaned;
    let cosmetic_seconds = start.elapsed().as_secs_f64();
    let start = Instant::now();
    let method = if mono { DemosaicMethod::None } else { method };
    let rgb = match method {
        DemosaicMethod::Mhc => mhc_demosaic(frame, cleaned, cancel)?,
        DemosaicMethod::Vng => crate::vng::vng_demosaic_spans(
            frame,
            cleaned,
            cancel,
            std::mem::take(&mut buffers.rgb),
            spans.map(|spans| spans.demosaic.as_slice()),
        )?,
        DemosaicMethod::None | DemosaicMethod::External => {
            let mut pixels = std::mem::take(&mut buffers.rgb);
            pixels.clear();
            pixels.extend(cleaned.iter().map(|&v| [v; 3]));
            check_cancelled(cancel)?;
            RgbImage {
                width: frame.header.info.width,
                height: frame.header.info.height,
                demosaic_method: DemosaicMethod::None,
                pixels,
            }
        }
    };
    Ok((
        rgb,
        DebayerStats {
            demosaic_method: method,
            cosmetic_seconds,
            demosaic_seconds: start.elapsed().as_secs_f64(),
            corrected_pixels,
        },
    ))
}

#[cfg(test)]
#[path = "cosmetic_reference.rs"]
mod cosmetic_reference;
#[cfg(test)]
#[path = "cosmetic_tests.rs"]
mod cosmetic_tests;
