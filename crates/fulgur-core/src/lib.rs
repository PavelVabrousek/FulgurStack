//! FITS and XISF input, CFA geometry, immutable frame storage, and safe output publication.

pub mod cameras;
pub mod derive;
pub mod fits;
pub mod fits_cache;
pub mod output;
pub mod telemetry;
pub mod xisf;
mod xisf_write;

use serde::Serialize;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

pub type Result<T> = std::result::Result<T, Error>;

/// Supported uniform scale from a source frame to a geometry reference: 30–333 %
/// (owner range; reciprocal bounds, so 0.30 and 1/0.30 are both reachable).
pub const MIN_GEOMETRY_SCALE: f64 = 0.30;
pub const MAX_GEOMETRY_SCALE: f64 = 1. / MIN_GEOMETRY_SCALE;

pub fn geometry_scale_supported(scale: f64) -> bool {
    scale.is_finite() && (MIN_GEOMETRY_SCALE..=MAX_GEOMETRY_SCALE).contains(&scale)
}

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

    pub fn name(self) -> &'static str {
        match self {
            Self::GRBG => "GRBG",
            Self::RGGB => "RGGB",
            Self::GBRG => "GBRG",
            Self::BGGR => "BGGR",
        }
    }

    /// The same cell with its two rows exchanged.
    pub fn rows_swapped(self) -> Self {
        match self {
            Self::GRBG => Self::BGGR,
            Self::RGGB => Self::GBRG,
            Self::GBRG => Self::RGGB,
            Self::BGGR => Self::GRBG,
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

/// Sensor colour layout of a science frame. `Unknown` means the header has no
/// BAYERPAT and the owner has not yet declared whether the light set is colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mosaic {
    Bayer,
    Mono,
    /// Debayered before FulgurStack: three stored colour planes, no CFA.
    Rgb,
    Unknown,
}

/// Per light-set sensor layout: the FITS header, or an explicit owner choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LayoutChoice {
    #[default]
    Auto,
    Mono,
    Bayer(BayerPattern),
}

impl LayoutChoice {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "mono" => Ok(Self::Mono),
            other => BayerPattern::parse(other).map(Self::Bayer).map_err(|_| {
                Error::Invalid("Sensor layout must be auto, mono or a Bayer pattern.")
            }),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Mono => "mono",
            Self::Bayer(pattern) => pattern.name(),
        }
    }
}

impl Serialize for LayoutChoice {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

pub struct RawFrame {
    pub header: fits::FitsHeader,
    pub pixels: Vec<f32>,
}

pub struct RgbImage {
    pub width: usize,
    pub height: usize,
    pub demosaic_method: DemosaicMethod,
    /// Interleaved unstretched RGB in source ADU, before final sensor-range bounding.
    pub pixels: Vec<[f32; 3]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DemosaicMethod {
    Mhc,
    #[default]
    Vng,
    /// Monochrome sources: no demosaicing; every channel holds the same sample.
    None,
    /// Sources were debayered by another program.
    External,
}

impl DemosaicMethod {
    pub fn name(self) -> &'static str {
        match self {
            Self::Mhc => "MHC",
            Self::Vng => "VNG",
            Self::None => "NONE",
            Self::External => "EXTERNAL",
        }
    }
}

impl std::str::FromStr for DemosaicMethod {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "mhc" => Ok(Self::Mhc),
            "vng" => Ok(Self::Vng),
            _ => Err(Error::Invalid("Demosaic method must be vng or mhc.")),
        }
    }
}
