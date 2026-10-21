//! FITS I/O, CFA geometry, immutable frame storage, and safe output publication.

pub mod fits;
pub mod output;
pub mod telemetry;

use serde::Serialize;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Invalid(&'static str),
    Cancelled,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Invalid(message) => f.write_str(message),
            Self::Cancelled => f.write_str("Processing cancelled; unfinished output removed."),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn check_cancelled(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    Red,
    Green,
    Blue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum BayerPattern {
    GRBG,
    RGGB,
    GBRG,
    BGGR,
}

impl BayerPattern {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "GRBG" => Ok(Self::GRBG),
            "RGGB" => Ok(Self::RGGB),
            "GBRG" => Ok(Self::GBRG),
            "BGGR" => Ok(Self::BGGR),
            _ => Err(Error::Invalid("Unsupported or missing BAYERPAT.")),
        }
    }

    #[inline]
    pub fn color(self, x: usize, y: usize) -> Color {
        let red = match self {
            Self::GRBG => (1, 0),
            Self::RGGB => (0, 0),
            Self::GBRG => (0, 1),
            Self::BGGR => (1, 1),
        };
        if x % 2 == red.0 && y % 2 == red.1 {
            Color::Red
        } else if x % 2 != red.0 && y % 2 != red.1 {
            Color::Blue
        } else {
            Color::Green
        }
    }
}

pub struct RawFrame {
    pub header: fits::FitsHeader,
    pub pixels: Vec<f32>,
}

pub struct RgbImage {
    pub width: usize,
    pub height: usize,
    /// Interleaved linear RGB in physical source ADU; never stretched or clipped.
    pub pixels: Vec<[f32; 3]>,
}
