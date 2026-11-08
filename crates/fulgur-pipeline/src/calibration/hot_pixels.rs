//! Hot pixel maps (ADR-163): the brightest pixels of any dark or bias master,
//! replaced in every light by an approximation from their neighbours of the
//! same colour.
//!
//! The owner names the share of the sensor to repair (typically 0.01 %). The
//! pixels with the highest excess over the median of their own Bayer phase
//! are the map. Ranking by excess keeps one colour's higher offset from
//! filling the map alone. A map is a calibration file of its own: an 8-bit
//! FITS image holding 1 at each hot pixel, so it can be saved and reused
//! without the frames it came from.
use super::{MasterKind, collect_frames, median_f32};
use fulgur_core::{Error, Mosaic, RawFrame, Result, check_cancelled};
use rayon::prelude::*;
use serde::Serialize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

/// Share of all pixels repaired when the owner does not say otherwise (percent).
pub const DEFAULT_PERCENT: f64 = 0.01;
/// Largest share accepted (percent): a map is for defects, not for a sensor.
pub const MAX_PERCENT: f64 = 5.0;
const IMAGE_TYPE: &str = "Hot Pixel Map";

/// Where a map came from, for the report.
#[derive(Clone, Debug, Serialize)]
pub struct HotPixelSummary {
    pub pixels: usize,
    pub percent: f64,
    pub width: usize,
    pub height: usize,
    /// "frames" (built here from darks or bias frames) or "map file".
    pub origin: &'static str,
    pub source_files: usize,
    pub rule: &'static str,
}

const RULE: &str = "hot pixels of the map are replaced by the median of their same-colour neighbours (two pixels away on a Bayer mask, one on a mono sensor), neighbours that are hot themselves left out";

#[derive(Debug)]
pub struct HotPixelMap {
    pub width: usize,
    pub height: usize,
    /// Pixel indices (`y * width + x`), ascending.
    indices: Vec<u32>,
    flagged: Vec<u64>,
    pub summary: HotPixelSummary,
}

impl HotPixelMap {
    fn from_indices(
        width: usize,
        height: usize,
        mut indices: Vec<u32>,
        percent: f64,
        origin: &'static str,
        source_files: usize,
    ) -> Self {
        indices.sort_unstable();
        indices.dedup();
        let mut flagged = vec![0_u64; (width * height).div_ceil(64)];
        for &i in &indices {
            flagged[i as usize / 64] |= 1 << (i % 64);
        }
        let summary = HotPixelSummary {
            pixels: indices.len(),
            percent,
            width,
            height,
            origin,
            source_files,
            rule: RULE,
        };
        Self {
            width,
            height,
            indices,
            flagged,
            summary,
        }
    }

    pub fn len(&self) -> usize {
        self.indices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    /// True when the pixel at column `x`, row `y` of the sensor is in the map.
    pub fn contains_pixel(&self, x: usize, y: usize) -> bool {
        x < self.width && y < self.height && self.is_hot(y * self.width + x)
    }

    fn is_hot(&self, index: usize) -> bool {
        self.flagged[index / 64] >> (index % 64) & 1 == 1
    }

    /// The `percent` of pixels (0 < percent <= 5) with the highest excess over
    /// the median of their 2 x 2 phase in `pixels`.
    pub fn from_pixels(
        pixels: &[f32],
        width: usize,
        height: usize,
        percent: f64,
        source_files: usize,
    ) -> Result<Self> {
        if !(percent.is_finite() && percent > 0. && percent <= MAX_PERCENT) {
            return Err(Error::Invalid(
                "The hot pixel share must be above 0 and at most 5 percent.",
            ));
        }
        if width < 4 || height < 4 || pixels.len() != width * height {
            return Err(Error::Invalid("Hot pixel source has an invalid size."));
        }
        let total = width * height;
        let wanted = ((total as f64 * percent / 100.).ceil() as usize).clamp(1, total);
        let mut medians = [0.0_f32; 4];
        for (phase, median) in medians.iter_mut().enumerate() {
            let (px, py) = (phase % 2, phase / 2);
            let mut samples: Vec<f32> = (py..height)
                .step_by(2)
                .flat_map(|y| {
                    pixels[y * width + px..(y + 1) * width]
                        .iter()
                        .step_by(2)
                        .copied()
                })
                .filter(|v| v.is_finite())
                .collect();
            *median = median_f32(&mut samples) as f32;
        }
        let excess: Vec<f32> = pixels
            .par_iter()
            .enumerate()
            .map(|(i, &v)| {
                let phase = (i % width) % 2 + 2 * ((i / width) % 2);
                if v.is_finite() {
                    v - medians[phase]
                } else {
                    f32::NEG_INFINITY
                }
            })
            .collect();
        let mut ranked = excess.clone();
        ranked.select_nth_unstable_by(wanted - 1, |a, b| b.total_cmp(a));
        let threshold = ranked[wanted - 1];
        let mut chosen: Vec<u32> = excess
            .iter()
            .enumerate()
            .filter(|(_, v)| **v > threshold)
            .map(|(i, _)| i as u32)
            .collect();
        for (i, v) in excess.iter().enumerate() {
            if chosen.len() >= wanted {
                break;
            }
            if *v == threshold {
                chosen.push(i as u32);
            }
        }
        Ok(Self::from_indices(
            width,
            height,
            chosen,
            percent,
            "frames",
            source_files,
        ))
    }

    /// Combine dark or bias frames (any exposure, files or folders) by their
    /// per-pixel median and take the brightest `percent` of the pixels.
    pub fn from_frames(inputs: &[PathBuf], percent: f64, cancel: &AtomicBool) -> Result<Self> {
        let files = collect_frames(inputs)?;
        if files.is_empty() {
            return Err(Error::Invalid(
                "No dark or bias FITS or XISF files found for the hot pixel map.",
            ));
        }
        let master = super::build_master(
            &files,
            MasterKind::Bias,
            super::CombineMethod::Median,
            cancel,
        )?;
        Self::from_pixels(
            &master.pixels,
            master.width,
            master.height,
            percent,
            files.len(),
        )
    }

    /// A saved map file, or else dark or bias frames, as the files say.
    pub fn from_inputs(inputs: &[PathBuf], percent: f64, cancel: &AtomicBool) -> Result<Self> {
        match inputs {
            [one] if one.is_file() && is_map_file(one) => Self::load(one),
            _ => Self::from_frames(inputs, percent, cancel),
        }
    }

    /// Replace the map's pixels inside the frame (a full frame or a ROI at
    /// `origin` of the sensor) by the median of their same-colour neighbours
    /// that are not hot. Returns how many were replaced.
    pub fn repair(&self, frame: &mut RawFrame, origin: [usize; 2]) -> usize {
        let (w, h) = (frame.header.info.width, frame.header.info.height);
        let step: isize = if frame.header.info.mosaic == Mosaic::Bayer {
            2
        } else {
            1
        };
        let mut found = vec![];
        for y in 0..h {
            let row = (origin[1] + y) * self.width + origin[0];
            let (from, to) = (
                self.indices.partition_point(|&i| (i as usize) < row),
                self.indices.partition_point(|&i| (i as usize) < row + w),
            );
            for &i in &self.indices[from..to] {
                found.push((i as usize - row, y));
            }
        }
        let mut replacements = Vec::with_capacity(found.len());
        for &(x, y) in &found {
            let mut near: Vec<f32> = Vec::with_capacity(8);
            for dy in [-step, 0, step] {
                for dx in [-step, 0, step] {
                    let (nx, ny) = (x as isize + dx, y as isize + dy);
                    if (dx, dy) == (0, 0)
                        || nx < 0
                        || ny < 0
                        || nx as usize >= w
                        || ny as usize >= h
                    {
                        continue;
                    }
                    let global = (origin[1] + ny as usize) * self.width + origin[0] + nx as usize;
                    if !self.is_hot(global) {
                        near.push(frame.pixels[ny as usize * w + nx as usize]);
                    }
                }
            }
            replacements.push((!near.is_empty()).then(|| median_f32(&mut near) as f32));
        }
        let mut replaced = 0;
        for (&(x, y), value) in found.iter().zip(replacements) {
            if let Some(v) = value {
                frame.pixels[y * w + x] = v;
                replaced += 1;
            }
        }
        replaced
    }

    /// Write the map as an 8-bit FITS image (1 = hot); atomic `.tmp` rename,
    /// never overwriting an existing file.
    pub fn save(&self, path: &Path, cancel: &AtomicBool) -> Result<()> {
        check_cancelled(cancel)?;
        if path.exists() {
            return Err(Error::Invalid(
                "Destination already exists; existing files are never overwritten.",
            ));
        }
        let card = |text: String| {
            let mut c = [b' '; 80];
            c[..text.len().min(80)].copy_from_slice(&text.as_bytes()[..text.len().min(80)]);
            c
        };
        let mut header: Vec<[u8; 80]> = vec![
            card(format!("{:<8}= {:>20}", "SIMPLE", "T")),
            card(format!("{:<8}= {:>20}", "BITPIX", 8)),
            card(format!("{:<8}= {:>20}", "NAXIS", 2)),
            card(format!("{:<8}= {:>20}", "NAXIS1", self.width)),
            card(format!("{:<8}= {:>20}", "NAXIS2", self.height)),
            card(format!("{:<8}= '{IMAGE_TYPE}'", "IMAGETYP")),
            card(format!("{:<8}= {:>20}", "HOTCOUNT", self.len())),
            card(format!("{:<8}= {:>20.6}", "HOTPCT", self.summary.percent)),
            card(format!("{:<8}= 'FulgurStack'", "CREATOR")),
            card("END".to_owned()),
        ];
        let mut bytes: Vec<u8> = header.drain(..).flatten().collect();
        bytes.resize(bytes.len().div_ceil(2880) * 2880, b' ');
        let mut data = vec![0_u8; self.width * self.height];
        for &i in &self.indices {
            data[i as usize] = 1;
        }
        bytes.extend(data);
        bytes.resize(bytes.len().div_ceil(2880) * 2880, 0);
        let tmp = path.with_extension("fit.tmp");
        let result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            std::fs::rename(&tmp, path)?;
            Ok::<(), std::io::Error>(())
        })();
        if let Err(error) = result {
            let _ = std::fs::remove_file(&tmp);
            return Err(error.into());
        }
        Ok(())
    }

    /// Read a map file written by `save`.
    pub fn load(path: &Path) -> Result<Self> {
        let mut file = std::fs::File::open(path)?;
        let mut header = Vec::new();
        let mut block = [0_u8; 2880];
        let mut ended = false;
        while !ended {
            file.read_exact(&mut block)
                .map_err(|_| Error::Invalid("Hot pixel map file ends inside its header."))?;
            ended = block.chunks(80).any(|c| c.starts_with(b"END "));
            header.extend_from_slice(&block);
            if header.len() > 2880 * 40 {
                return Err(Error::Invalid("Hot pixel map header is too long."));
            }
        }
        let value = |key: &str| {
            header.chunks(80).find_map(|c| {
                let text = String::from_utf8_lossy(c);
                (text.starts_with(key) && text.as_bytes().get(8) == Some(&b'=')).then(|| {
                    text[10..]
                        .split('/')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .trim_matches('\'')
                        .trim()
                        .to_owned()
                })
            })
        };
        let number = |key: &str| value(key).and_then(|v| v.parse::<f64>().ok());
        if value("IMAGETYP").as_deref() != Some(IMAGE_TYPE) || number("BITPIX") != Some(8.) {
            return Err(Error::Invalid("Not a FulgurStack hot pixel map."));
        }
        let (width, height) = (
            number("NAXIS1").unwrap_or(0.) as usize,
            number("NAXIS2").unwrap_or(0.) as usize,
        );
        if width < 4 || height < 4 || width * height > u32::MAX as usize {
            return Err(Error::Invalid("Hot pixel map has an invalid size."));
        }
        let mut data = vec![0_u8; width * height];
        file.read_exact(&mut data)
            .map_err(|_| Error::Invalid("Hot pixel map file ends inside its image."))?;
        let indices: Vec<u32> = data
            .iter()
            .enumerate()
            .filter(|(_, v)| **v != 0)
            .map(|(i, _)| i as u32)
            .collect();
        let percent = number("HOTPCT").unwrap_or(indices.len() as f64 * 100. / data.len() as f64);
        Ok(Self::from_indices(
            width, height, indices, percent, "map file", 0,
        ))
    }
}

/// True when the file is a hot pixel map written by this program.
pub fn is_map_file(path: &Path) -> bool {
    let mut block = [0_u8; 2880];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut block))
        .is_ok()
        && block
            .chunks(80)
            .any(|c| String::from_utf8_lossy(c).contains(&format!("'{IMAGE_TYPE}'")))
}
