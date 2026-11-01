//! Minimal primary-image FITS reader. Header cards are retained privately.

use crate::derive::{self, DerivedValue};
use crate::xisf::{self, XisfImage};
use crate::{
    BayerPattern, DemosaicMethod, Error, LayoutChoice, Mosaic, RawFrame, Result, RgbImage,
};
use rayon::prelude::*;
use serde::Serialize;
use std::fmt;
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;
use std::sync::Arc;

/// File name extensions read as light and calibration frames.
pub const INPUT_EXTENSIONS: [&str; 5] = ["fit", "fits", "fts", "xisf", "xish"];

/// True for a readable frame; `._name` files are macOS sidecars, not images.
pub fn is_input(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        INPUT_EXTENSIONS
            .iter()
            .any(|expected| extension.eq_ignore_ascii_case(expected))
    }) && !path
        .file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with("._"))
}

pub const BLOCK_SIZE: usize = 2880;
const MAX_HEADER_BLOCKS: usize = 360;
const MAX_RAW_BYTES: usize = 512 * 1024 * 1024;
/// Bounded original-byte read-ahead only, released before CFA reconstruction.
pub const ROI_READ_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Serialize)]
pub struct FrameInfo {
    pub width: usize,
    pub height: usize,
    pub bitpix: i32,
    pub bzero: f32,
    pub bscale: f32,
    /// Present only for colour (Bayer) frames.
    pub bayer_pattern: Option<BayerPattern>,
    pub mosaic: Mosaic,
    /// "header" when BAYERPAT decided the layout, "user" for an owner choice;
    /// "header_alias", "file_name" or "camera_catalog" when derived (ADR-084).
    pub mosaic_source: &'static str,
    pub exposure_seconds: f64,
    pub object: Option<String>,
    pub filter: Option<String>,
    pub date_obs: Option<String>,
    pub gain: Option<f64>,
    pub bias_informational: Option<f64>,
    pub creator: Option<String>,
    /// Sample where this sensor clips, when it lies below the 16-bit range
    /// (ADR-106); None keeps the 16-bit limits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saturation_adu: Option<f32>,
    /// Values missing from the header and reconstructed (ADR-084).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub derived: Vec<DerivedValue>,
}

impl FrameInfo {
    /// Apply a light set's layout choice. `Auto` keeps the header decision:
    /// a BAYERPAT card means colour, its absence means mono (unless derived).
    /// Frames already debayered have no sensor layout to choose.
    pub fn apply_layout(&mut self, choice: LayoutChoice) {
        if self.mosaic == Mosaic::Rgb {
            return;
        }
        if choice != LayoutChoice::Auto {
            self.derived.retain(|d| d.field != derive::LAYOUT);
        }
        match choice {
            LayoutChoice::Auto => {}
            LayoutChoice::Mono => {
                self.bayer_pattern = None;
                self.mosaic = Mosaic::Mono;
                self.mosaic_source = "user";
            }
            LayoutChoice::Bayer(pattern) => {
                self.bayer_pattern = Some(pattern);
                self.mosaic = Mosaic::Bayer;
                self.mosaic_source = "user";
            }
        }
    }

    /// Apply a light set's exposure entered by the owner, replacing the
    /// header value or a derived one.
    pub fn apply_exposure(&mut self, seconds: Option<f64>) {
        if let Some(seconds) = seconds.filter(|s| s.is_finite() && *s > 0.0) {
            self.exposure_seconds = seconds;
            self.derived.retain(|d| d.field != derive::EXPOSURE);
        }
    }

    /// True when a derived value would change processing and needs the
    /// owner's confirmation.
    pub fn needs_confirmation(&self) -> bool {
        self.derived.iter().any(|d| d.needs_confirmation)
    }
}

#[derive(Clone)]
pub struct FitsHeader {
    cards: Vec<[u8; 80]>,
    pub info: FrameInfo,
    pub data_offset: u64,
    pub data_bytes: usize,
    pub file_bytes: u64,
    pub(crate) bzero: f32,
    /// Pixels of an XISF source; None for FITS.
    pub xisf: Option<Arc<XisfImage>>,
    /// `ROWORDER = 'BOTTOM-UP'`: stored rows run upwards. Readers return them
    /// top-down, the order BAYERPAT and every later stage assume.
    pub(crate) bottom_up: bool,
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

fn read_cards(reader: &mut impl Read) -> Result<Vec<[u8; 80]>> {
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
    Ok(cards)
}

impl FitsHeader {
    pub fn read(reader: &mut (impl Read + Seek), file_bytes: u64) -> Result<Self> {
        Self::read_with(reader, file_bytes, false, None)
    }

    /// As `read` for a light frame; values missing from the header are
    /// reconstructed from other cards, the file name and the camera catalogue.
    pub fn read_named(
        reader: &mut (impl Read + Seek),
        file_bytes: u64,
        file_name: &str,
    ) -> Result<Self> {
        Self::read_with(reader, file_bytes, false, Some(file_name))
    }

    /// As `read`, for bias frames: a missing or zero EXPTIME reads as 0 s.
    pub fn read_calibration(reader: &mut (impl Read + Seek), file_bytes: u64) -> Result<Self> {
        Self::read_with(reader, file_bytes, true, None)
    }

    fn read_with(
        reader: &mut (impl Read + Seek),
        file_bytes: u64,
        zero_exposure: bool,
        name: Option<&str>,
    ) -> Result<Self> {
        let cards = read_cards(reader)?;
        let mut header = Self::from_cards(cards, zero_exposure, name, false)?;
        header.data_offset = reader.stream_position()?;
        header.file_bytes = file_bytes;
        let padded_data = header.data_bytes.div_ceil(BLOCK_SIZE) * BLOCK_SIZE;
        if file_bytes < header.data_offset + padded_data as u64 {
            return Err(Error::Invalid(
                "Truncated FITS pixel array or data padding.",
            ));
        }
        Ok(header)
    }

    /// Metadata shared by FITS headers and the cards made from an XISF header.
    /// `rgb` marks frames debayered by another program.
    fn from_cards(
        cards: Vec<[u8; 80]>,
        zero_exposure: bool,
        name: Option<&str>,
        rgb: bool,
    ) -> Result<Self> {
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
            return Err(Error::Invalid("Input must be a two-axis BITPIX=16 image."));
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
        let mut derived = vec![];
        let real = |key: &str| number(&cards, key, None).ok();
        let text = |key: &str| find(&cards, key).filter(|v| !v.trim().is_empty());
        let has_exptime = find(&cards, "EXPTIME").is_some();
        let exposure_seconds = match number(&cards, "EXPTIME", None) {
            Ok(value) if zero_exposure && value >= 0.0 => value,
            Err(_) if zero_exposure => 0.0,
            // A light without any exposure source reads as 0 s and is
            // rejected by inspection with "missing_exposure" (ADR-084).
            Err(_) if !has_exptime => match derive::exposure(real, name) {
                Some((value, d)) => {
                    derived.push(d);
                    value
                }
                None => 0.0,
            },
            value => value?,
        };
        if exposure_seconds <= 0.0 && !zero_exposure && has_exptime {
            return Err(Error::Invalid("EXPTIME must be positive."));
        }
        // A light set may still override this choice (colour pattern or mono).
        let mut bayer_pattern = match find(&cards, "BAYERPAT") {
            Some(value) if !value.trim().is_empty() => Some(BayerPattern::parse(&value)?),
            _ => None,
        };
        // Owner decision (ADR-078): no BAYERPAT means a monochrome sensor,
        // unless other cards, the file name or the camera say colour (ADR-084).
        let (mosaic, mosaic_source) = match bayer_pattern {
            _ if rgb => (Mosaic::Rgb, "debayered_source"),
            Some(_) => (Mosaic::Bayer, "header"),
            None if zero_exposure => (Mosaic::Mono, "no_bayer_header"),
            None => match derive::layout(text, name, width, height) {
                Some(layout) => {
                    derived.push(layout.derived);
                    bayer_pattern = layout.pattern;
                    let mosaic = if bayer_pattern.is_some() {
                        Mosaic::Bayer
                    } else {
                        Mosaic::Unknown
                    };
                    (mosaic, layout.source)
                }
                None => (Mosaic::Mono, "no_bayer_header"),
            },
        };
        let mut gain = real("GAIN");
        let mut object = find(&cards, "OBJECT");
        if !zero_exposure {
            if gain.is_none() {
                gain = derive::gain(name).map(|(value, d)| {
                    derived.push(d);
                    value
                });
            }
            if object.is_none() {
                object = derive::object(name).map(|(value, d)| {
                    derived.push(d);
                    value
                });
            }
        }
        let info = FrameInfo {
            width,
            height,
            bitpix: 16,
            bzero: bzero as f32,
            bscale: bscale as f32,
            bayer_pattern,
            mosaic,
            mosaic_source,
            exposure_seconds,
            object,
            filter: find(&cards, "FILTER"),
            date_obs: find(&cards, "DATE-OBS"),
            gain,
            bias_informational: number(&cards, "BIAS", None).ok(),
            creator: find(&cards, "CREATOR"),
            saturation_adu: None,
            derived,
        };
        let bottom_up = find(&cards, "ROWORDER").as_deref() == Some("BOTTOM-UP");
        Ok(Self {
            cards,
            info,
            data_offset: 0,
            data_bytes,
            file_bytes: 0,
            bzero: bzero as f32,
            xisf: None,
            bottom_up,
        })
    }

    /// Header of an XISF unit, over the same card logic as a FITS frame.
    fn from_xisf(path: &Path, zero_exposure: bool, name: Option<&str>) -> Result<Self> {
        let opened = xisf::open(path)?;
        let rgb = opened.image.channels == 3;
        let mut header = Self::from_cards(opened.cards, zero_exposure, name, rgb)?;
        header.info.bitpix = opened.image.bitpix();
        header.info.bzero = 0.;
        header.bzero = 0.;
        header.file_bytes = std::fs::metadata(path)?.len();
        header.xisf = Some(opened.image);
        Ok(header)
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

/// Header-only description used to plan light and calibration sets. It is
/// lenient about BITPIX/NAXIS (calibration masters are often 32-bit float),
/// never reads pixels and never carries location cards.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileMetadata {
    pub bitpix: i32,
    pub width: usize,
    pub height: usize,
    pub planes: usize,
    pub bayer_pattern: Option<BayerPattern>,
    pub image_type: Option<String>,
    pub exposure_seconds: Option<f64>,
    pub filter: Option<String>,
    pub instrument: Option<String>,
    pub creator: Option<String>,
    pub telescope: Option<String>,
    pub gain: Option<f64>,
    pub ccd_temperature: Option<f64>,
    pub date_obs: Option<String>,
    pub object: Option<String>,
    pub combined_frames: Option<u32>,
    /// Camera rotator angle in degrees (ROTATOR or ROTATANG), when recorded.
    pub rotator_degrees: Option<f64>,
    /// Light frames only (`read_light_metadata`): reconstructed values.
    pub derived: Vec<DerivedValue>,
    /// Light frames only: a colour camera whose Bayer pattern is unknown.
    pub colour_without_pattern: bool,
}

impl FileMetadata {
    /// Best camera label: INSTRUME, else a Seestar capture-software name.
    pub fn camera(&self) -> Option<String> {
        self.instrument.clone().or_else(|| {
            self.creator
                .as_deref()
                .filter(|c| c.to_ascii_lowercase().contains("seestar"))
                .map(|c| c.trim_start_matches("ZWO ").trim().to_owned())
        })
    }
}

/// Header cards of a file; cards made from XISF come with its image.
type FileCards = (Vec<[u8; 80]>, Option<Arc<XisfImage>>);

fn file_cards(path: &Path) -> Result<FileCards> {
    if xisf::is_xisf(path) {
        let opened = xisf::open(path)?;
        return Ok((opened.cards, Some(opened.image)));
    }
    let file = File::open(path)?;
    Ok((
        read_cards(&mut BufReader::with_capacity(BLOCK_SIZE, file))?,
        None,
    ))
}

pub fn read_metadata(path: &Path) -> Result<FileMetadata> {
    let (cards, image) = file_cards(path)?;
    if cards.first().map(keyword) != Some("SIMPLE") {
        return Err(Error::Invalid("Not a primary FITS image."));
    }
    let integer = |key| find(&cards, key).and_then(|v| v.trim().parse::<i64>().ok());
    let real = |key| number(&cards, key, None).ok();
    let axes = integer("NAXIS").unwrap_or(0);
    if !(2..=3).contains(&axes) {
        return Err(Error::Invalid("FITS image must have two or three axes."));
    }
    let text = |key| find(&cards, key).filter(|v| !v.trim().is_empty());
    let rgb = image.as_ref().is_some_and(|i| i.channels == 3);
    Ok(FileMetadata {
        bitpix: image
            .as_ref()
            .map_or(integer("BITPIX").unwrap_or(0) as i32, |i| i.bitpix()),
        width: integer("NAXIS1").unwrap_or(0).max(0) as usize,
        height: integer("NAXIS2").unwrap_or(0).max(0) as usize,
        planes: if rgb {
            3
        } else if axes == 3 {
            integer("NAXIS3").unwrap_or(1).max(1) as usize
        } else {
            1
        },
        bayer_pattern: text("BAYERPAT")
            .filter(|_| !rgb)
            .and_then(|v| BayerPattern::parse(&v).ok()),
        image_type: text("IMAGETYP").or_else(|| text("FRAME")),
        exposure_seconds: real("EXPTIME").or_else(|| real("EXPOSURE")),
        filter: text("FILTER"),
        instrument: text("INSTRUME"),
        creator: text("CREATOR"),
        telescope: text("TELESCOP"),
        gain: real("GAIN"),
        ccd_temperature: real("CCD-TEMP")
            .or_else(|| real("DET-TEMP"))
            .or_else(|| real("SET-TEMP")),
        date_obs: text("DATE-OBS"),
        object: text("OBJECT"),
        rotator_degrees: real("ROTATOR").or_else(|| real("ROTATANG")),
        combined_frames: integer("NCOMBINE")
            .filter(|n| *n > 0)
            .map(|n| n.min(u32::MAX as i64) as u32),
        derived: vec![],
        colour_without_pattern: false,
    })
}

/// As `read_metadata` for a light frame, with the reconstruction that
/// `read_header` applies (ADR-084), so planning shows the same values.
pub fn read_light_metadata(path: &Path) -> Result<FileMetadata> {
    let mut meta = read_metadata(path)?;
    let (cards, _) = file_cards(path)?;
    let name = file_name(path);
    let name = Some(name.as_str());
    let real = |key: &str| number(&cards, key, None).ok();
    let text = |key: &str| find(&cards, key).filter(|v| !v.trim().is_empty());
    if find(&cards, "EXPTIME").is_none() {
        meta.exposure_seconds = derive::exposure(real, name).map(|(value, d)| {
            meta.derived.push(d);
            value
        });
    }
    if meta.bayer_pattern.is_none() && meta.planes == 1 {
        if let Some(layout) = derive::layout(text, name, meta.width, meta.height) {
            meta.bayer_pattern = layout.pattern;
            meta.colour_without_pattern = layout.pattern.is_none();
            meta.derived.push(layout.derived);
        }
    }
    if meta.gain.is_none() {
        meta.gain = derive::gain(name).map(|(value, d)| {
            meta.derived.push(d);
            value
        });
    }
    if meta.object.is_none() {
        meta.object = derive::object(name).map(|(value, d)| {
            meta.derived.push(d);
            value
        });
    }
    Ok(meta)
}

pub fn read_header(path: &Path) -> Result<FitsHeader> {
    if xisf::is_xisf(path) {
        return FitsHeader::from_xisf(path, false, Some(&file_name(path)));
    }
    let file = File::open(path)?;
    let len = file.metadata()?.len();
    FitsHeader::read_named(&mut BufReader::new(file), len, &file_name(path))
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Header of a calibration frame; bias frames may have no or zero EXPTIME.
pub fn read_calibration_header(path: &Path) -> Result<FitsHeader> {
    if xisf::is_xisf(path) {
        return FitsHeader::from_xisf(path, true, None);
    }
    let file = File::open(path)?;
    let len = file.metadata()?.len();
    FitsHeader::read_calibration(&mut BufReader::new(file), len)
}

/// Geometry-only reference plane: RGB uses green, monochrome uses its only channel.
/// This importer does not relax the science-input CFA contract or infer exposure/GPS.
pub struct ReferencePlane {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<f32>,
    pub object: Option<String>,
    pub filter: Option<String>,
}

pub fn read_reference_plane(
    path: &Path,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<ReferencePlane> {
    if xisf::is_xisf(path) {
        let (cards, image) = file_cards(path)?;
        let image = image.ok_or(Error::Invalid("XISF unit has no image."))?;
        if image.width.saturating_mul(image.height) > 96_000_000 {
            return Err(Error::Invalid(
                "Reference plane exceeds the bounded 96-million-pixel limit.",
            ));
        }
        return Ok(ReferencePlane {
            width: image.width,
            height: image.height,
            pixels: image.read_plane(image.channels / 2, cancel)?,
            object: find(&cards, "OBJECT"),
            filter: find(&cards, "FILTER"),
        });
    }
    let file = File::open(path)?;
    let file_bytes = file.metadata()?.len();
    let mut reader = BufReader::new(file);
    let cards = read_cards(&mut reader)?;
    for key in [
        "SIMPLE", "BITPIX", "NAXIS", "NAXIS1", "NAXIS2", "NAXIS3", "BZERO", "BSCALE", "ROWORDER",
    ] {
        if cards.iter().filter(|c| keyword(c) == key).count() > 1 {
            return Err(Error::Invalid("Duplicate reference FITS keyword."));
        }
    }
    if cards.first().map(keyword) != Some("SIMPLE")
        || find(&cards, "SIMPLE").as_deref() != Some("T")
        || find(&cards, "BLANK").is_some()
    {
        return Err(Error::Invalid(
            "Reference must be a SIMPLE primary image without BLANK pixels.",
        ));
    }
    let channels = match find(&cards, "NAXIS").as_deref() {
        Some("2") => 1,
        Some("3") => match find(&cards, "NAXIS3").as_deref() {
            Some("1") => 1,
            Some("3") => 3,
            _ => {
                return Err(Error::Invalid(
                    "Reference must be monochrome or planar RGB.",
                ));
            }
        },
        _ => {
            return Err(Error::Invalid(
                "Reference must be monochrome or planar RGB.",
            ));
        }
    };
    let bytes = match find(&cards, "BITPIX").as_deref() {
        Some("-32") => 4,
        Some("16") => 2,
        _ => return Err(Error::Invalid("Reference supports BITPIX=-32 or 16 only.")),
    };
    let width = dimension(&cards, "NAXIS1")?;
    let height = dimension(&cards, "NAXIS2")?;
    let count = width
        .checked_mul(height)
        .filter(|n| *n <= 96_000_000)
        .ok_or(Error::Invalid(
            "Reference plane exceeds the bounded 96-million-pixel limit.",
        ))?;
    let size = count
        .checked_mul(bytes)
        .and_then(|n| n.checked_mul(channels))
        .ok_or(Error::Invalid("Reference pixel length overflow."))?;
    let offset = reader.stream_position()?;
    if file_bytes < offset + (size.div_ceil(BLOCK_SIZE) * BLOCK_SIZE) as u64 {
        return Err(Error::Invalid("Truncated reference FITS data/padding."));
    }
    let bottom_up = match find(&cards, "ROWORDER").as_deref().unwrap_or("BOTTOM-UP") {
        "TOP-DOWN" => false,
        "BOTTOM-UP" => true,
        _ => return Err(Error::Invalid("Unsupported reference ROWORDER.")),
    };
    let bzero = number(&cards, "BZERO", Some(0.))?;
    let bscale = number(&cards, "BSCALE", Some(1.))?;
    if bscale <= 0. {
        return Err(Error::Invalid("Reference BSCALE must be positive."));
    }
    // Seek directly to green; never retain three full reference channels.
    reader.seek(std::io::SeekFrom::Start(
        offset
            + if channels == 3 {
                (count * bytes) as u64
            } else {
                0
            },
    ))?;
    let mut pixels = vec![0.; count];
    let mut row = vec![0; width * bytes];
    for y in 0..height {
        crate::check_cancelled(cancel)?;
        reader.read_exact(&mut row)?;
        let target_y = if bottom_up { height - 1 - y } else { y };
        for (x, chunk) in row.chunks_exact(bytes).enumerate() {
            let stored = if bytes == 4 {
                f32::from_be_bytes(chunk.try_into().unwrap()) as f64
            } else {
                i16::from_be_bytes(chunk.try_into().unwrap()) as f64
            };
            let v = (stored * bscale + bzero) as f32;
            if !v.is_finite() {
                return Err(Error::Invalid("Nonfinite reference plane sample."));
            }
            pixels[target_y * width + x] = v;
        }
    }
    Ok(ReferencePlane {
        width,
        height,
        pixels,
        object: find(&cards, "OBJECT"),
        filter: find(&cards, "FILTER"),
    })
}

pub fn read_raw(path: &Path) -> Result<RawFrame> {
    read_raw_with_decode(path, true, &mut ReadBuffers::default())
}

/// Decode locally when the caller already parallelizes independent frames.
/// This avoids nested Rayon work stealing retaining/queuing entire frames.
pub fn read_raw_sequential(path: &Path) -> Result<RawFrame> {
    read_raw_with_decode(path, false, &mut ReadBuffers::default())
}

/// Memory one worker reuses across frames, because fresh frame-sized memory
/// costs far more to fault in than the copies themselves.
#[derive(Default)]
pub struct ReadBuffers {
    chunk: Vec<u8>,
    pixels: Vec<f32>,
}

impl ReadBuffers {
    /// Takes back the pixels of a finished frame for the next read.
    pub fn give_back(&mut self, pixels: Vec<f32>) {
        self.pixels = pixels;
    }
}

/// `read_raw_sequential` on a worker's own memory.
pub fn read_raw_buffered(path: &Path, buffers: &mut ReadBuffers) -> Result<RawFrame> {
    read_raw_with_decode(path, false, buffers)
}

fn read_raw_with_decode(
    path: &Path,
    parallel: bool,
    buffers: &mut ReadBuffers,
) -> Result<RawFrame> {
    if xisf::is_xisf(path) {
        let header = read_header(path)?;
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let image = header.xisf.as_ref().unwrap();
        // The measurement plane; colour planes come through `read_channel`.
        let pixels = image.read_plane(image.channels / 2, &cancel)?;
        return Ok(RawFrame { header, pixels });
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    if !parallel {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_FLAG_SEQUENTIAL_SCAN: per-handle read-ahead/cache hint only.
        // No global cache settings, write access or bypassed safety checks.
        options.custom_flags(0x08000000);
    }
    let file = options.open(path)?;
    let len = file.metadata()?.len();
    let mut reader = BufReader::with_capacity(BLOCK_SIZE, file);
    let header = FitsHeader::read_named(&mut reader, len, &file_name(path))?;
    // Header parsing leaves the logical cursor exactly at the pixel array.
    // Seeking back would discard prefetched bytes and break forward read-ahead.
    // BIAS is intentionally never used. Source values remain in physical ADU.
    let decode = |pair: &[u8]| f32::from(i16::from_be_bytes([pair[0], pair[1]])) + header.bzero;
    let mut pixels: Vec<f32> = if parallel {
        let mut bytes = vec![0; header.data_bytes];
        for chunk in bytes.chunks_mut(1024 * 1024) {
            reader.read_exact(chunk)?;
        }
        bytes.par_chunks_exact(2).map(decode).collect()
    } else {
        let mut pixels = std::mem::take(&mut buffers.pixels);
        pixels.clear();
        pixels.reserve_exact(header.data_bytes / 2);
        buffers.chunk.resize(1024 * 1024, 0);
        let mut remaining = header.data_bytes;
        while remaining > 0 {
            let chunk = &mut buffers.chunk[..remaining.min(1024 * 1024)];
            reader.read_exact(chunk)?;
            pixels.extend(chunk.chunks_exact(2).map(decode));
            remaining -= chunk.len();
        }
        pixels
    };
    if header.bottom_up {
        flip_rows(&mut pixels, header.info.width);
    }
    Ok(RawFrame { header, pixels })
}

/// Samples sharing the top value before it counts as a clipped sensor limit.
const SATURATED_SAMPLES: usize = 16;
/// Tops at or above this are 16-bit data, handled by the fixed limits.
const FULL_RANGE_ADU: f32 = 62000.;

/// Where a sensor below 16 bits clips, read from a raw frame: clipped samples
/// pile up on one integer value, for example 16383 from a 14-bit camera. A
/// frame with no clipped sample has no such pile and needs no limit.
pub fn saturation_level(pixels: &[f32], bitpix: i32) -> Option<f32> {
    if bitpix <= 0 {
        return None;
    }
    let top = pixels.iter().copied().fold(0., f32::max);
    (top > 0. && top < FULL_RANGE_ADU)
        .then(|| pixels.iter().filter(|&&v| v == top).count())
        .filter(|&count| count >= SATURATED_SAMPLES)
        .map(|_| top)
}

/// Reverse the row order of a plane in place.
pub fn flip_rows(pixels: &mut [f32], width: usize) {
    let height = pixels.len() / width;
    for y in 0..height / 2 {
        let (upper, lower) = pixels.split_at_mut((height - 1 - y) * width);
        upper[y * width..(y + 1) * width].swap_with_slice(&mut lower[..width]);
    }
}

/// One stored channel of an XISF frame (0 red, 1 green, 2 blue).
pub fn read_channel(
    source: &FitsHeader,
    channel: usize,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Vec<f32>> {
    source
        .xisf
        .as_ref()
        .ok_or(Error::Invalid("Only XISF frames hold colour planes."))?
        .read_plane(channel, cancel)
}

/// A rectangle of a frame debayered by another program, in `pixels` storage.
pub fn read_rgb_roi(
    source: &FitsHeader,
    [x, y, width, height]: [usize; 4],
    cancel: &std::sync::atomic::AtomicBool,
    mut pixels: Vec<[f32; 3]>,
) -> Result<RgbImage> {
    let image = source
        .xisf
        .as_ref()
        .ok_or(Error::Invalid("Only XISF frames hold colour planes."))?;
    pixels.clear();
    pixels.resize(width * height, [0.; 3]);
    let mut plane = vec![0.; width * height];
    for channel in 0..3 {
        image.read_region(channel, [x, y, width, height], &mut plane, cancel)?;
        for (pixel, value) in pixels.iter_mut().zip(&plane) {
            pixel[channel] = *value;
        }
    }
    Ok(RgbImage {
        width,
        height,
        demosaic_method: DemosaicMethod::External,
        pixels,
    })
}

/// Decode a bounded original-CFA rectangle; offsets must preserve Bayer phase.
/// File is opened read-only and no whole source image is materialized.
pub fn read_roi(
    path: &Path,
    source: &FitsHeader,
    bounds: [usize; 4],
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<RawFrame> {
    read_roi_with_stats(path, source, bounds, cancel).map(|(frame, _, _)| frame)
}

struct RoiReader {
    file: File,
    calls: u64,
    bytes: u64,
}
impl Read for RoiReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let n = self.file.read(buffer)?;
        if n > 0 {
            self.calls += 1;
            self.bytes += n as u64;
        }
        Ok(n)
    }
}
impl Seek for RoiReader {
    fn seek(&mut self, position: std::io::SeekFrom) -> std::io::Result<u64> {
        self.file.seek(position)
    }
}

/// Counts underlying read calls/bytes, including read-ahead over row gaps.
/// These remain application I/O counters, not physical SSD traffic.
pub(crate) fn read_roi_with_stats(
    path: &Path,
    source: &FitsHeader,
    bounds: [usize; 4],
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<(RawFrame, u64, u64)> {
    read_roi_with_stats_into(path, source, bounds, cancel, Vec::new())
}

/// As `read_roi_with_stats`, decoding into reused storage. Every returned
/// sample is overwritten, so earlier contents of `pixels` never leak through.
pub(crate) fn read_roi_with_stats_into(
    path: &Path,
    source: &FitsHeader,
    bounds: [usize; 4],
    cancel: &std::sync::atomic::AtomicBool,
    mut pixels: Vec<f32>,
) -> Result<(RawFrame, u64, u64)> {
    use std::io::SeekFrom;
    validate_roi(source, bounds)?;
    let [x, y, width, height] = bounds;
    if let Some(image) = &source.xisf {
        pixels.clear();
        pixels.resize(width * height, 0.);
        image.read_region(image.channels / 2, bounds, &mut pixels, cancel)?;
        let mut header = source.clone();
        header.info.width = width;
        header.info.height = height;
        return Ok((RawFrame { header, pixels }, 0, 0));
    }
    let mut file = File::open(path)?;
    if file.metadata()?.len() != source.file_bytes {
        return Err(Error::Invalid(
            "Source FITS size changed during processing.",
        ));
    }
    let stored_y = if source.bottom_up {
        source.info.height - y - height
    } else {
        y
    };
    file.seek(SeekFrom::Start(
        source.data_offset + (stored_y * source.info.width + x) as u64 * 2,
    ))?;
    let mut file = BufReader::with_capacity(
        ROI_READ_BUFFER_BYTES,
        RoiReader {
            file,
            calls: 0,
            bytes: 0,
        },
    );
    pixels.clear();
    pixels.resize(width * height, 0.);
    let mut bytes = vec![0u8; width * 2];
    for row in 0..height {
        crate::check_cancelled(cancel)?;
        file.read_exact(&mut bytes)?;
        for (out, pair) in pixels[row * width..(row + 1) * width]
            .iter_mut()
            .zip(bytes.chunks_exact(2))
        {
            *out = f32::from(i16::from_be_bytes([pair[0], pair[1]])) + source.bzero;
        }
        if row + 1 < height {
            file.seek_relative(((source.info.width - width) * 2) as i64)?;
        }
    }
    if source.bottom_up {
        flip_rows(&mut pixels, width);
    }
    let mut header = source.clone();
    header.info.width = width;
    header.info.height = height;
    Ok((
        RawFrame { header, pixels },
        file.get_ref().calls,
        file.get_ref().bytes,
    ))
}

pub(crate) fn validate_roi(source: &FitsHeader, bounds: [usize; 4]) -> Result<()> {
    let [x, y, width, height] = bounds;
    if x % 2 != 0
        || y % 2 != 0
        || width < 6
        || height < 6
        || x.checked_add(width).is_none_or(|v| v > source.info.width)
        || y.checked_add(height).is_none_or(|v| v > source.info.height)
    {
        return Err(Error::Invalid("Invalid phase-preserving CFA ROI."));
    }
    Ok(())
}
