//! ESD with a trail mask (ADR-187). Plain ESD judges every sample on its own,
//! so a moving object (asteroid, satellite) loses only its bright core: the
//! faint edges of its image stay below the critical value in each frame and
//! add up along the path in the master. Here an upper ESD rejection marks its
//! frame, and samples of that frame within `radius_px` are rejected too: all
//! of them (`Grow`), or those raised above the pixel's survivor mean by
//! `sigma` single-frame standard deviations (`Hysteresis`).
//!
//! Only paired upper rejections seed the mask: an upper rejection with
//! another one beside it in the same frame. A trail is several pixels wide;
//! noise, cosmic rays and hot pixels give single ones.
//!
//! Stars and compact bright structure are protected: an upper rejection there
//! is usually seeing (a sharper frame), so it neither seeds nor receives the
//! mask. A pixel is protected when its survivor mean stands more than
//! `PROTECT_SIGMA` single-frame sigmas above the local background, the median
//! of survivor means in its `BACKGROUND_BLOCK` block.
//!
//! The tile is read with a halo of `radius_px`, so the mask crosses tile edges;
//! only the inner pixels are written.

use super::rejection_methods::{self, MethodScratch};
use super::{IntegratedPixel, ReductionContext, RowStats, Sum, TrailMode};
use fulgur_core::{Result, check_cancelled};
use rayon::prelude::*;

/// A pixel brighter than its local background by this many single-frame
/// sigmas is a star or compact structure, never part of the trail mask.
const PROTECT_SIGMA: f64 = 1.0;
/// Side of the blocks whose median survivor mean is the local background.
const BACKGROUND_BLOCK: usize = 32;

/// ESD outcome of one pixel, kept between the passes.
#[derive(Clone, Copy, Default)]
struct PixelInfo {
    mean: [f32; 3],
    sd: [f32; 3],
    /// All three channels hold the same samples (a monochrome stack).
    mono: bool,
    protected: bool,
}

/// Per-pixel frame bit sets, `words` u64 per pixel and channel.
struct Bits {
    words: usize,
}

impl Bits {
    fn set(row: &mut [u64], at: usize, frame: usize) {
        row[at + frame / 64] |= 1 << (frame % 64);
    }
    fn get(row: &[u64], at: usize, frame: usize) -> bool {
        row[at + frame / 64] & (1 << (frame % 64)) != 0
    }
}

/// Reduces a tile read with a halo. `frames` hold `size` pixels; `inner` is
/// the part written to `output` (`[x, y, width, height]` within the tile).
pub(super) fn reduce_tile(
    frames: &[Vec<[f32; 3]>],
    size: [usize; 2],
    inner: [usize; 4],
    output: &mut Vec<IntegratedPixel>,
    context: &ReductionContext<'_>,
) -> Result<(Vec<RowStats>, u64)> {
    let ReductionContext {
        weights,
        original_input_count,
        total_weight,
        config,
        cancel,
    } = *context;
    let trail = config.trail;
    let [width, height] = size;
    let count = frames.len();
    let words = count.div_ceil(64).max(1);
    let bits = Bits { words };
    let pixels = width * height;
    // Pass 1: ESD per pixel and channel; which frames were rejected, below or
    // above, and the survivors' mean and single-frame sigma.
    let mut info = vec![PixelInfo::default(); pixels];
    let mut seed = vec![0_u64; pixels * words];
    let mut rejected_low = vec![0_u64; pixels * 3 * words];
    let mut rejected_high = vec![0_u64; pixels * 3 * words];
    info.par_chunks_mut(width)
        .zip(seed.par_chunks_mut(width * words))
        .zip(rejected_low.par_chunks_mut(width * 3 * words))
        .zip(rejected_high.par_chunks_mut(width * 3 * words))
        .enumerate()
        .try_for_each(|(y, (((info, seed), low), high))| -> Result<()> {
            check_cancelled(cancel)?;
            let mut scratch = MethodScratch::default();
            let mut values = Vec::with_capacity(count);
            let mut sources = Vec::with_capacity(count);
            let mut outcome = Vec::with_capacity(count);
            for (x, pixel) in info.iter_mut().enumerate() {
                let p = y * width + x;
                let mono = frames.iter().all(|f| {
                    let v = f[p];
                    v[0].to_bits() == v[1].to_bits() && v[1].to_bits() == v[2].to_bits()
                });
                pixel.mono = mono;
                for c in 0..if mono { 1 } else { 3 } {
                    values.clear();
                    sources.clear();
                    for (index, frame) in frames.iter().enumerate() {
                        let v = frame[p][c];
                        if v.is_finite() {
                            values.push(v);
                            sources.push(index);
                        }
                    }
                    let (mean, sd) =
                        rejection_methods::esd_outcome(&values, &mut scratch, &mut outcome);
                    pixel.mean[c] = mean as f32;
                    pixel.sd[c] = sd as f32;
                    let at = (x * 3 + c) * bits.words;
                    for (&state, &frame) in outcome.iter().zip(&sources) {
                        match state {
                            1 => Bits::set(low, at, frame),
                            2 => {
                                Bits::set(high, at, frame);
                                Bits::set(seed, x * bits.words, frame);
                            }
                            _ => {}
                        }
                    }
                }
                if mono {
                    pixel.mean = [pixel.mean[0]; 3];
                    pixel.sd = [pixel.sd[0]; 3];
                }
            }
            Ok(())
        })?;
    // Local background: the median survivor mean of each block.
    let blocks = [
        width.div_ceil(BACKGROUND_BLOCK),
        height.div_ceil(BACKGROUND_BLOCK),
    ];
    let background: Vec<[f32; 3]> = (0..blocks[0] * blocks[1])
        .into_par_iter()
        .map(|b| {
            let (bx, by) = (b % blocks[0], b / blocks[0]);
            let mut level = [0_f32; 3];
            let mut means = Vec::with_capacity(BACKGROUND_BLOCK * BACKGROUND_BLOCK);
            for (c, level) in level.iter_mut().enumerate() {
                means.clear();
                for y in by * BACKGROUND_BLOCK..((by + 1) * BACKGROUND_BLOCK).min(height) {
                    for x in bx * BACKGROUND_BLOCK..((bx + 1) * BACKGROUND_BLOCK).min(width) {
                        let i = info[y * width + x];
                        if i.sd[c] > 0. {
                            means.push(i.mean[c]);
                        }
                    }
                }
                if !means.is_empty() {
                    let middle = means.len() / 2;
                    *level = *means.select_nth_unstable_by(middle, f32::total_cmp).1;
                }
            }
            level
        })
        .collect();
    info.par_chunks_mut(width)
        .zip(seed.par_chunks_mut(width * words))
        .enumerate()
        .for_each(|(y, (info, seed))| {
            for (x, pixel) in info.iter_mut().enumerate() {
                let level = background[(y / BACKGROUND_BLOCK) * blocks[0] + x / BACKGROUND_BLOCK];
                pixel.protected = (0..3).any(|c| {
                    pixel.sd[c] > 0.
                        && f64::from(pixel.mean[c] - level[c])
                            > PROTECT_SIGMA * f64::from(pixel.sd[c])
                });
                if pixel.protected {
                    seed[x * words..(x + 1) * words].fill(0);
                }
            }
        });
    // A seed is an upper rejection with another one next to it in the same
    // frame: a trail is several pixels wide, while noise, cosmic rays and hot
    // pixels give single ones.
    let seed = {
        let raw = &seed;
        let mut paired = vec![0_u64; pixels * words];
        paired
            .par_chunks_mut(width * words)
            .enumerate()
            .for_each(|(y, row)| {
                for x in 0..width {
                    let own = &raw[(y * width + x) * words..(y * width + x + 1) * words];
                    if own.iter().all(|&w| w == 0) {
                        continue;
                    }
                    let out = &mut row[x * words..(x + 1) * words];
                    for dy in -1_isize..=1 {
                        for dx in -1_isize..=1 {
                            let (qx, qy) = (x as isize + dx, y as isize + dy);
                            if (dx, dy) == (0, 0)
                                || qx < 0
                                || qy < 0
                                || qx >= width as isize
                                || qy >= height as isize
                            {
                                continue;
                            }
                            let q = qy as usize * width + qx as usize;
                            for ((o, a), b) in out
                                .iter_mut()
                                .zip(own)
                                .zip(&raw[q * words..(q + 1) * words])
                            {
                                *o |= a & b;
                            }
                        }
                    }
                }
            });
        paired
    };
    // Pass 2: frames with a seed within the radius.
    let radius = trail.radius_px;
    let reach = radius.ceil() as isize;
    let offsets: Vec<(isize, isize)> = (-reach..=reach)
        .flat_map(|dy| (-reach..=reach).map(move |dx| (dx, dy)))
        .filter(|&(dx, dy)| ((dx * dx + dy * dy) as f64) <= radius * radius)
        .collect();
    let [ix, iy, iw, ih] = inner;
    let mut near = vec![0_u64; iw * ih * words];
    near.par_chunks_mut(iw * words)
        .enumerate()
        .for_each(|(row, near)| {
            let y = (iy + row) as isize;
            for col in 0..iw {
                let x = (ix + col) as isize;
                let out = &mut near[col * words..(col + 1) * words];
                for &(dx, dy) in &offsets {
                    let (qx, qy) = (x + dx, y + dy);
                    if qx < 0 || qy < 0 || qx >= width as isize || qy >= height as isize {
                        continue;
                    }
                    let q = qy as usize * width + qx as usize;
                    for (o, s) in out.iter_mut().zip(&seed[q * words..(q + 1) * words]) {
                        *o |= s;
                    }
                }
            }
        });
    // Pass 3: weighted mean of the survivors of ESD and the mask.
    output.clear();
    output.resize(iw * ih, ([0.; 3], [0.; 3], [0.; 3]));
    let grow = trail.mode == TrailMode::Grow;
    let results: Vec<(RowStats, u64)> = output
        .par_chunks_mut(iw)
        .zip(near.par_chunks(iw * words))
        .enumerate()
        .map(|(row, (out, near))| -> Result<(RowStats, u64)> {
            check_cancelled(cancel)?;
            let mut stats = RowStats::new(count);
            let mut masked = 0_u64;
            let y = iy + row;
            for (col, pixel) in out.iter_mut().enumerate() {
                let x = ix + col;
                let p = y * width + x;
                let i = info[p];
                let near = &near[col * words..(col + 1) * words];
                let mut covered = Sum::default();
                let mut n = 0;
                for (frame, w) in frames.iter().zip(weights) {
                    if frame[p][0].is_finite() {
                        covered.add(*w);
                        n += 1;
                    }
                }
                stats.min_n = stats.min_n.min(n);
                stats.max_n = stats.max_n.max(n);
                stats.min_fraction = stats.min_fraction.min(covered.value() / total_weight);
                if n == 0 {
                    stats.zero += 1;
                }
                for c in 0..3 {
                    let source = if i.mono { 0 } else { c };
                    let at = (x * 3 + source) * words;
                    let low_row = &rejected_low[y * width * 3 * words..];
                    let high_row = &rejected_high[y * width * 3 * words..];
                    let limit = f64::from(i.mean[source]) + trail.sigma * f64::from(i.sd[source]);
                    let (mut valid, mut low, mut high) = (0_u64, 0_u64, 0_u64);
                    let mut numerator = Sum::default();
                    let mut denominator = Sum::default();
                    for (f, (frame, &w)) in frames.iter().zip(weights).enumerate() {
                        let v = frame[p][source];
                        if !v.is_finite() {
                            continue;
                        }
                        valid += 1;
                        if Bits::get(low_row, at, f) {
                            low += 1;
                        } else if Bits::get(high_row, at, f) {
                            high += 1;
                        } else if !i.protected
                            && Bits::get(near, 0, f)
                            && (grow || f64::from(v) > limit)
                        {
                            high += 1;
                            masked += 1;
                        } else {
                            numerator.add(f64::from(v) * w);
                            denominator.add(w);
                        }
                    }
                    let surviving = denominator.value();
                    pixel.0[c] = if surviving == 0. {
                        0.
                    } else {
                        (numerator.value() / surviving) as f32
                    };
                    pixel.1[c] = ((valid - low - high) as f64 / original_input_count as f64) as f32;
                    pixel.2[c] = (high as f64 / original_input_count as f64) as f32;
                    stats.valid += valid;
                    stats.low += low;
                    stats.high += high;
                    stats.min_surviving = stats.min_surviving.min(surviving / total_weight);
                }
            }
            Ok((stats, masked))
        })
        .collect::<Result<_>>()?;
    let masked = results.iter().map(|r| r.1).sum();
    Ok((results.into_iter().map(|r| r.0).collect(), masked))
}

#[cfg(test)]
#[path = "trail_mask_tests.rs"]
mod tests;
