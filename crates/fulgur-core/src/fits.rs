//! Minimal primary-image FITS reader. Header cards are retained privately.

use crate::{BayerPattern, Error, RawFrame, Result};
use rayon::prelude::*;
use serde::Serialize;
use std::fmt;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

pub const BLOCK_SIZE: usize = 2880;
const MAX_HEADER_BLOCKS: usize = 360;
const MAX_RAW_BYTES: usize = 512 * 1024 * 1024;

#[derive(Clone, Debug, Serialize)]
pub struct FrameInfo {
    pub width: usize,
    pub height: usize,
    pub bitpix: i32,
    pub bzero: f32,
    pub bscale: f32,
    pub bayer_pattern: BayerPattern,
    pub exposure_seconds: f64,
    pub object: Option<String>,
    pub filter: Option<String>,
    pub date_obs: Option<String>,
    pub gain: Option<f64>,
    pub bias_informational: Option<f64>,
    pub creator: Option<String>,
}

#[derive(Clone)]
pub struct FitsHeader {
    cards: Vec<[u8; 80]>,
    pub info: FrameInfo,
    pub data_offset: u64,
    pub data_bytes: usize,
    pub file_bytes: u64,
    bzero: f32,
}

// Raw cards can contain GPS. Neither Debug nor default reports expose them.
impl fmt::Debug for FitsHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FitsHeader")
            .field("info", &self.info)
            .field("data_offset", &self.data_offset)
            .finish_non_exhaustive()
    }
}

fn keyword(card: &[u8; 80]) -> &str {
    std::str::from_utf8(&card[..8]).unwrap_or("").trim()
}

fn value(card: &[u8; 80]) -> Option<String> {
    if card[8] != b'=' {
        return None;
    }
    let text = std::str::from_utf8(&card[10..]).ok()?;
    let mut quoted = false;
    let mut end = text.len();
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\'' {
            quoted = !quoted;
        } else if byte == b'/' && !quoted {
            end = index;
            break;
        }
    }
    let text = text[..end].trim();
    Some(
        if text.starts_with('\'') && text.ends_with('\'') && text.len() >= 2 {
            text[1..text.len() - 1]
                .replace("''", "'")
                .trim_end()
                .to_owned()
        } else {
            text.to_owned()
        },
    )
}

fn find(cards: &[[u8; 80]], key: &str) -> Option<String> {
    cards
        .iter()
        .find(|card| keyword(card) == key)
        .and_then(value)
}

fn number(cards: &[[u8; 80]], key: &str, default: Option<f64>) -> Result<f64> {
    let parsed = match find(cards, key) {
        Some(text) => text.replace(['D', 'd'], "E").parse::<f64>().ok(),
        None => default,
    };
    parsed
        .filter(|value| value.is_finite())
        .ok_or(Error::Invalid(
            "Missing, invalid, or non-finite numeric FITS metadata.",
        ))
}

fn dimension(cards: &[[u8; 80]], key: &str) -> Result<usize> {
    find(cards, key)
        .and_then(|text| text.parse::<usize>().ok())
        .filter(|value| *value >= 6)
        .ok_or(Error::Invalid(
            "Image dimensions must be integers of at least 6 pixels.",
        ))
}

impl FitsHeader {
    pub fn read(reader: &mut (impl Read + Seek), file_bytes: u64) -> Result<Self> {
        let mut cards = Vec::new();
        let mut end_found = false;
        for _ in 0..MAX_HEADER_BLOCKS {
            let mut block = [0_u8; BLOCK_SIZE];
            reader.read_exact(&mut block)?;
            for chunk in block.chunks_exact(80) {
                if chunk.iter().any(|byte| !(32..=126).contains(byte)) {
                    return Err(Error::Invalid(
                        "FITS header must contain printable ASCII cards.",
                    ));
                }
                let card: [u8; 80] = chunk.try_into().expect("80-byte card");
                if keyword(&card) == "END" {
                    end_found = true;
                    break;
                }
                cards.push(card);
            }
            if end_found {
                break;
            }
        }
        if !end_found {
            return Err(Error::Invalid(
                "FITS header END card not found within limit.",
            ));
        }
        for key in [
            "SIMPLE", "BITPIX", "NAXIS", "NAXIS1", "NAXIS2", "BZERO", "BSCALE", "BAYERPAT",
            "EXPTIME",
        ] {
            if cards.iter().filter(|card| keyword(card) == key).count() > 1 {
                return Err(Error::Invalid("Duplicate required FITS keyword."));
            }
        }
        if cards.first().map(keyword) != Some("SIMPLE")
            || find(&cards, "SIMPLE").as_deref() != Some("T")
        {
            return Err(Error::Invalid(
                "Only SIMPLE primary FITS images are supported.",
            ));
        }
        if find(&cards, "BITPIX").as_deref() != Some("16")
            || find(&cards, "NAXIS").as_deref() != Some("2")
        {
            return Err(Error::Invalid(
                "Input must be a two-axis BITPIX=16 CFA image.",
            ));
        }
        if find(&cards, "BLANK").is_some() {
            return Err(Error::Invalid(
                "Input images using BLANK pixels are not supported in M1.",
            ));
        }
        let width = dimension(&cards, "NAXIS1")?;
        let height = dimension(&cards, "NAXIS2")?;
        let data_bytes = width
            .checked_mul(height)
            .and_then(|n| n.checked_mul(2))
            .filter(|bytes| *bytes <= MAX_RAW_BYTES)
            .ok_or(Error::Invalid(
                "Image exceeds the M1 single-frame memory bound.",
            ))?;
        let bzero = number(&cards, "BZERO", Some(0.0))?;
        let bscale = number(&cards, "BSCALE", Some(1.0))?;
        if bscale != 1.0 || ![0.0, 32768.0].contains(&bzero) {
            return Err(Error::Invalid(
                "M1 supports BSCALE=1 and BZERO=0 or 32768 only.",
            ));
        }
        for key in ["XBAYROFF", "YBAYROFF"] {
            if number(&cards, key, Some(0.0))? != 0.0 {
                return Err(Error::Invalid(
                    "Nonzero Bayer offsets require an explicitly adjusted CFA pattern.",
                ));
            }
        }
        let exposure_seconds = number(&cards, "EXPTIME", None)?;
        if exposure_seconds <= 0.0 {
            return Err(Error::Invalid("EXPTIME must be positive."));
        }
        let info = FrameInfo {
            width,
            height,
            bitpix: 16,
            bzero: bzero as f32,
            bscale: bscale as f32,
            bayer_pattern: BayerPattern::parse(&find(&cards, "BAYERPAT").unwrap_or_default())?,
            exposure_seconds,
            object: find(&cards, "OBJECT"),
            filter: find(&cards, "FILTER"),
            date_obs: find(&cards, "DATE-OBS"),
            gain: number(&cards, "GAIN", None).ok(),
            bias_informational: number(&cards, "BIAS", None).ok(),
            creator: find(&cards, "CREATOR"),
        };
        let data_offset = reader.stream_position()?;
        let padded_data = data_bytes.div_ceil(BLOCK_SIZE) * BLOCK_SIZE;
        if file_bytes < data_offset + padded_data as u64 {
            return Err(Error::Invalid(
                "Truncated FITS pixel array or data padding.",
            ));
        }
        Ok(Self {
            cards,
            info,
            data_offset,
            data_bytes,
            file_bytes,
            bzero: bzero as f32,
        })
    }

    pub fn raw_card(&self, key: &str) -> Option<&[u8; 80]> {
        self.cards.iter().find(|card| keyword(card) == key)
    }

    pub fn valid_gps_cards(&self) -> Option<[[u8; 80]; 2]> {
        let lat = number(&self.cards, "SITELAT", None).ok()?;
        let lon = number(&self.cards, "SITELONG", None).ok()?;
        if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
            return None;
        }
        Some([*self.raw_card("SITELAT")?, *self.raw_card("SITELONG")?])
    }
}

pub fn read_header(path: &Path) -> Result<FitsHeader> {
    let file = File::open(path)?;
    let len = file.metadata()?.len();
    FitsHeader::read(&mut BufReader::new(file), len)
}

pub fn read_raw(path: &Path) -> Result<RawFrame> {
    let file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut reader = BufReader::with_capacity(1024 * 1024, file);
    let header = FitsHeader::read(&mut reader, len)?;
    reader.seek(SeekFrom::Start(header.data_offset))?;
    let mut bytes = vec![0; header.data_bytes];
    reader.read_exact(&mut bytes)?;
    // BIAS is intentionally never used. Source values remain in physical ADU.
    let pixels = bytes
        .par_chunks_exact(2)
        .map(|pair| f32::from(i16::from_be_bytes([pair[0], pair[1]])) + header.bzero)
        .collect();
    Ok(RawFrame { header, pixels })
}
