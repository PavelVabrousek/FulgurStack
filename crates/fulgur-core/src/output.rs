//! Atomic no-clobber output; only the final FITS/XISF/report uses a .tmp file.

use crate::fits::{BLOCK_SIZE, ImageHeader};
use crate::{Error, Result, RgbImage, check_cancelled};
use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::AtomicBool;

/// Fixed sensor full scale, shared by every image/channel; never an image extremum.
pub const OUTPUT_ADU_DIVISOR: f32 = 65_535.0;

/// File format of masters and diagnostic maps, chosen by the file extension
/// (ADR-142). Both hold the same 32-bit float samples and the same keywords.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    Fits,
    Xisf,
}

impl OutputFormat {
    pub const ALL: [Self; 2] = [Self::Fits, Self::Xisf];

    /// The format a destination's extension names: .fit, .fits or .fts for
    /// FITS, .xisf for XISF.
    pub fn of(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?;
        if ["fit", "fits", "fts"]
            .iter()
            .any(|e| extension.eq_ignore_ascii_case(e))
        {
            Some(Self::Fits)
        } else if extension.eq_ignore_ascii_case("xisf") {
            Some(Self::Xisf)
        } else {
            None
        }
    }

    /// Extension of new files: diagnostic maps and new master names.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Fits => "fits",
            Self::Xisf => "xisf",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Fits => "FITS",
            Self::Xisf => "XISF",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Fits => "fits",
            Self::Xisf => "xisf",
        }
    }
}

fn format_of(path: &Path) -> Result<OutputFormat> {
    OutputFormat::of(path).ok_or(Error::Invalid(
        "Output must end in .fit, .fits, .fts or .xisf.",
    ))
}

/// Planar 32-bit float samples, one row at a time; `sample` maps a stored
/// value to the written one.
fn write_planes(
    writer: &mut dyn Write,
    image: &RgbImage,
    channels: &[usize],
    big_endian: bool,
    cancel: &AtomicBool,
    mut sample: impl FnMut(f32) -> f32,
) -> Result<()> {
    let mut row = vec![0u8; image.width * 4];
    for &channel in channels {
        for pixels in image.pixels.chunks_exact(image.width) {
            check_cancelled(cancel)?;
            for (bytes, pixel) in row.chunks_exact_mut(4).zip(pixels) {
                let value = sample(pixel[channel]);
                bytes.copy_from_slice(&if big_endian {
                    value.to_be_bytes()
                } else {
                    value.to_le_bytes()
                });
            }
            writer.write_all(&row)?;
        }
    }
    Ok(())
}

/// One image as FITS (cards, padding, big-endian data, padding) or XISF.
fn write_image(
    path: &Path,
    format: OutputFormat,
    mut cards: Vec<[u8; 80]>,
    image: &RgbImage,
    channels: &[usize],
    cancel: &AtomicBool,
    sample: impl FnMut(f32) -> f32,
) -> Result<()> {
    match format {
        OutputFormat::Fits => {
            let mut end = [b' '; 80];
            end[..3].copy_from_slice(b"END");
            cards.push(end);
            atomic_write(path, cancel, |writer| {
                for card in &cards {
                    writer.write_all(card)?;
                }
                let header_padding = (BLOCK_SIZE - cards.len() * 80 % BLOCK_SIZE) % BLOCK_SIZE;
                writer.write_all(&vec![b' '; header_padding])?;
                write_planes(writer, image, channels, true, cancel, sample)?;
                let bytes = image.pixels.len() * 4 * channels.len();
                let padding = (BLOCK_SIZE - bytes % BLOCK_SIZE) % BLOCK_SIZE;
                writer.write_all(&vec![0; padding])?;
                Ok(())
            })
        }
        OutputFormat::Xisf => atomic_write(path, cancel, |writer| {
            crate::xisf_write::write_unit(
                writer,
                image.width,
                image.height,
                channels.len(),
                &cards,
                |writer| write_planes(writer, image, channels, false, cancel, sample),
            )
        }),
    }
}

#[derive(Debug, serde::Serialize)]
pub struct RgbExportStats {
    pub output_min: f32,
    pub output_max: f32,
    pub samples_below_zero: usize,
    pub samples_above_one: usize,
    /// Reconstruction samples limited to the normalized sensor range at export.
    pub samples_clipped: usize,
    pub samples_limited_low: usize,
    pub samples_limited_high: usize,
}

pub fn ensure_new_destination(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(Error::Invalid(
            "Destination already exists; existing files are never overwritten.",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub fn atomic_write(
    path: &Path,
    cancel: &AtomicBool,
    write: impl FnOnce(&mut dyn Write) -> Result<()>,
) -> Result<()> {
    check_cancelled(cancel)?;
    ensure_new_destination(path)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .ok_or(Error::Invalid("Output requires a filename."))?;
    // Zero random bytes gives exactly <output>.tmp. create_new protects stale files.
    let mut pending = tempfile::Builder::new()
        .prefix(name)
        .suffix(".tmp")
        .rand_bytes(0)
        .tempfile_in(parent)?;
    {
        let mut writer = BufWriter::with_capacity(1024 * 1024, pending.as_file_mut());
        write(&mut writer)?;
        writer.flush()?;
    }
    check_cancelled(cancel)?;
    pending.as_file().sync_all()?;
    check_cancelled(cancel)?;
    pending
        .persist_noclobber(path)
        .map_err(|error| Error::Io(error.error))?;
    Ok(())
}

/// A card with a comment after the value, when both fit in 80 columns.
fn card_with_comment(key: &str, value: &str, comment: &str) -> [u8; 80] {
    let mut out = card(key, value);
    let used = out.iter().rposition(|&b| b != b' ').map_or(0, |i| i + 1);
    let note = format!(" / {comment}");
    if !comment.is_empty() && used + note.len() <= 80 {
        out[used..used + note.len()].copy_from_slice(note.as_bytes());
    }
    out
}

fn card(key: &str, value: &str) -> [u8; 80] {
    let text = if value.starts_with('\'') {
        format!("{key:<8}= {value:<20}")
    } else {
        format!("{key:<8}= {value:>20}")
    };
    // Every value is built here and fits; tests catch a longer one, and a
    // release build cuts it at the card's end instead of stopping (C-07).
    debug_assert!(text.len() <= 80, "FITS card too long: {text}");
    let len = text.len().min(80);
    let mut card = [b' '; 80];
    card[..len].copy_from_slice(&text.as_bytes()[..len]);
    card
}

fn history(text: &str) -> [u8; 80] {
    let mut card = [b' '; 80];
    card[..8].copy_from_slice(b"HISTORY ");
    let len = text.len().min(72);
    card[8..8 + len].copy_from_slice(&text.as_bytes()[..len]);
    card
}

/// Integration provenance; EXPTIME remains the anchor intensity scale.
#[derive(Clone)]
pub struct MasterMetadata {
    pub geometric_correction_frames: usize,
    pub accepted_frames: usize,
    /// Pixel rejection key actually applied (`IntegrationStats::rejection`).
    pub rejection: &'static str,
    pub total_exposure_seconds: f64,
    pub origin_x_px: i32,
    pub origin_y_px: i32,
    pub external_geometry_scale: Option<f64>,
    /// Accepted monochrome sources; in a colour master they feed R, G and B equally.
    pub monochrome_frames: usize,
    /// Accepted sources calibrated with in-RAM masters, and the formula used.
    pub calibrated_frames: usize,
    pub calibration_formula: Option<String>,
    /// The grid's plate solution as FITS WCS cards, checked on the master
    /// (ADR-155); None leaves the master without a celestial WCS.
    pub astrometry: Option<Astrometry>,
    /// Median capture time (UTC, DATE-OBS format) of the stacked frames and
    /// how many dated frames it comes from; it replaces the anchor's DATE-OBS
    /// and is written as DATE-AVG too (ADR-185). None keeps the anchor's time.
    pub median_date_obs: Option<(String, usize)>,
}

/// DATE-OBS and DATE-AVG cards for the median capture time of a master.
fn median_time_cards(cards: &mut Vec<[u8; 80]>, median: &(String, usize)) {
    let (date, frames) = median;
    cards.push(card_with_comment(
        "DATE-OBS",
        &format!("'{date}'"),
        "[UTC] median start of stacked frames",
    ));
    cards.push(card_with_comment(
        "DATE-AVG",
        &format!("'{date}'"),
        "[UTC] median start of stacked frames",
    ));
    cards.push(history(&format!(
        "DATE-OBS/DATE-AVG: median start time of {frames} stacked frames."
    )));
}

/// WCS cards (keyword, value, comment) and history lines of a master.
#[derive(Clone, Debug, Default)]
pub struct Astrometry {
    pub cards: Vec<(String, String, String)>,
    pub history: Vec<String>,
}

/// A master reconstructed only from monochrome sources has identical channels
/// and is written as one plane; anything with colour sources stays planar RGB.
fn planes(image: &RgbImage) -> usize {
    if image.demosaic_method == crate::DemosaicMethod::None {
        1
    } else {
        3
    }
}

pub fn write_rgb_atomic(
    path: &Path,
    image: &RgbImage,
    source: &ImageHeader,
    cancel: &AtomicBool,
) -> Result<RgbExportStats> {
    write_rgb_with_metadata(path, image, source, None, cancel)
}

pub fn write_master_atomic(
    path: &Path,
    image: &RgbImage,
    source: &ImageHeader,
    metadata: &MasterMetadata,
    cancel: &AtomicBool,
) -> Result<RgbExportStats> {
    if metadata
        .external_geometry_scale
        .is_some_and(|v| !crate::geometry_scale_supported(v))
        || metadata.accepted_frames == 0
        || !metadata.total_exposure_seconds.is_finite()
        || metadata.total_exposure_seconds <= 0.
    {
        return Err(Error::Invalid("Invalid master exposure provenance."));
    }
    write_rgb_with_metadata(path, image, source, Some(metadata), cancel)
}

fn write_rgb_with_metadata(
    path: &Path,
    image: &RgbImage,
    source: &ImageHeader,
    metadata: Option<&MasterMetadata>,
    cancel: &AtomicBool,
) -> Result<RgbExportStats> {
    let format = format_of(path)?;
    if (metadata.is_none()
        && (image.width != source.info.width || image.height != source.info.height))
        || image.width == 0
        || image.height == 0
        || image.width.checked_mul(image.height) != Some(image.pixels.len())
        || image
            .pixels
            .iter()
            .flatten()
            .any(|value| !value.is_finite())
    {
        return Err(Error::Invalid(
            "RGB dimensions or finite pixel values are invalid.",
        ));
    }
    let mut stats = RgbExportStats {
        output_min: f32::INFINITY,
        output_max: f32::NEG_INFINITY,
        samples_below_zero: 0,
        samples_above_one: 0,
        samples_clipped: 0,
        samples_limited_low: 0,
        samples_limited_high: 0,
    };
    let planes = planes(image);
    let mono = planes == 1;
    let mut cards = vec![card("SIMPLE", "T"), card("BITPIX", "-32")];
    cards.push(card("NAXIS", if mono { "2" } else { "3" }));
    cards.push(card("NAXIS1", &image.width.to_string()));
    cards.push(card("NAXIS2", &image.height.to_string()));
    if !mono {
        cards.push(card("NAXIS3", "3"));
    }
    cards.extend([
        card("EXTEND", "T"),
        card(
            "CREATOR",
            &format!("'FulgurStack {}'", env!("CARGO_PKG_VERSION")),
        ),
        card("BUNIT", "'adu/65535'"),
        card("FLXDIV", "65535"),
        card("RGBBOUND", "'0:1'"),
        card("ROWORDER", "'TOP-DOWN'"),
        card("DEBAYER", &format!("'{}'", image.demosaic_method.name())),
        card(
            "NCOMBINE",
            &metadata.map_or(1, |m| m.accepted_frames).to_string(),
        ),
        card(
            "TEXPTIME",
            &format!(
                "{:.15E}",
                metadata.map_or(source.info.exposure_seconds, |m| m.total_exposure_seconds)
            ),
        ),
    ]);
    if let Some(m) = metadata {
        cards.push(card("CROPX", &m.origin_x_px.to_string()));
        cards.push(card("CROPY", &m.origin_y_px.to_string()));
        cards.push(card(
            "REGMODEL",
            if m.external_geometry_scale.is_some() {
                "'SIM(2)'"
            } else {
                "'SE(2)'"
            },
        ));
        let rejection = match m.rejection {
            "winsorized" => "WINSOR-MAD".to_owned(),
            "linear-fit" => "LINEAR-FIT".to_owned(),
            other => other.to_ascii_uppercase(),
        };
        cards.push(card("REJECT", &format!("'{rejection}'")));
        if m.geometric_correction_frames > 0 {
            cards.push(card("NDISTCOR", &m.geometric_correction_frames.to_string()));
            cards.push(history(
                "Validated cubic geometric correction; coefficients in JSON report.",
            ));
            cards.push(history(
                "Distortion and grid mapping composed; one image interpolation.",
            ));
        }
        cards.push(history(
            "Weighted linear mean on the reference-anchor intensity scale.",
        ));
        cards.push(history(
            "EXPTIME is anchor exposure; TEXPTIME is accepted acquisition total.",
        ));
        cards.push(history(if m.external_geometry_scale.is_some() {
            "CROPX/CROPY locate output (0,0) in external reference pixel centers."
        } else {
            "CROPX/CROPY locate output (0,0) in reference-anchor pixel centers."
        }));
        if let Some(scale) = m.external_geometry_scale {
            cards.push(card("GEOMSCL", &format!("{scale:.12}")));
            cards.push(history(
                "External reference supplies geometry only, never pixel intensities.",
            ));
            cards.push(history(
                "Exposure/GPS/photometry belong to the native accepted source anchor.",
            ));
            cards.push(history(
                "Native detector pixel-pitch cards omitted on the external grid.",
            ));
        }
        match &m.astrometry {
            Some(astrometry) => {
                for (key, value, comment) in &astrometry.cards {
                    cards.push(card_with_comment(key, value, comment));
                }
                for line in &astrometry.history {
                    cards.push(history(line));
                }
            }
            None => cards.push(history(
                "No plate solution; crop offsets are not a celestial WCS.",
            )),
        }
    }
    // Do not propagate CFA, scaling, checksum, display-range, or unverified WCS cards.
    let median_time = metadata.and_then(|m| m.median_date_obs.as_ref());
    if let Some(median) = median_time {
        median_time_cards(&mut cards, median);
    }
    for key in [
        "OBJECT", "FILTER", "DATE-OBS", "EXPTIME", "INSTRUME", "TELESCOP", "OBSERVER", "FOCALLEN",
        "XPIXSZ", "YPIXSZ", "GAIN", "ISOSPEED", "CCD-TEMP",
    ] {
        if metadata.is_some_and(|m| m.external_geometry_scale.is_some())
            && ["XPIXSZ", "YPIXSZ"].contains(&key)
        {
            continue;
        }
        if key == "DATE-OBS" && median_time.is_some() {
            continue;
        }
        if let Some(original) = source.raw_card(key) {
            cards.push(*original);
        }
    }
    if let Some(gps) = source.valid_gps_cards() {
        cards.extend(gps);
        cards.push(history(
            "Observing location copied from the reference anchor only.",
        ));
        cards.push(history(
            "Reference location may not describe every contributing session.",
        ));
    }
    cards.push(history(
        "Linear output = source ADU / 65535; same divisor for every RGB sample.",
    ));
    cards.push(history(
        "Zero offset; no stretch, image-extrema normalization or BIAS subtraction.",
    ));
    cards.push(history(
        "Reconstruction samples bounded to sensor range 0..1 (PI compatible).",
    ));
    if let Some(m) = metadata.filter(|m| m.calibrated_frames > 0) {
        cards.push(history(&format!(
            "{} frames calibrated before cosmetics and demosaicing:",
            m.calibrated_frames
        )));
        let formula = m.calibration_formula.as_deref().unwrap_or("light");
        let words: Vec<&str> = formula.split(' ').collect();
        let mut line = String::new();
        for word in words {
            if !line.is_empty() && line.len() + 1 + word.len() > 72 {
                cards.push(history(&line));
                line.clear();
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        if !line.is_empty() {
            cards.push(history(&line));
        }
    }
    if mono {
        cards.push(history("Conservative isolated hot-pixel correction."));
        cards.push(history(
            "Monochrome sources: no demosaicing; one linear plane.",
        ));
    } else {
        cards.push(history(
            "Conservative same-color isolated hot-pixel correction.",
        ));
        cards.push(history(&format!(
            "5x5 {} RGB reconstruction; native CFA samples retained after cosmetics.",
            image.demosaic_method.name()
        )));
        if let Some(m) = metadata.filter(|m| m.monochrome_frames > 0) {
            cards.push(history(&format!(
                "{} monochrome frames contribute equally to R, G and B.",
                m.monochrome_frames
            )));
        }
    }
    cards.push(history(
        if metadata.is_some_and(|m| m.external_geometry_scale.is_some()) {
            "Registered in external reference top-down coordinates."
        } else if metadata.is_some() {
            "Registered in anchor top-down coordinates; no display flip applied."
        } else {
            "Sensor row and column order preserved; no image flip applied."
        },
    ));
    // A monochrome master stores its (identical) green channel only.
    let channels: &[usize] = if mono { &[1] } else { &[0, 1, 2] };
    write_image(path, format, cards, image, channels, cancel, |stored| {
        let reconstructed = stored / OUTPUT_ADU_DIVISOR;
        // Owner-approved reconstruction saturation, not a display
        // workaround or a frame-dependent change of photometric scale.
        let value = reconstructed.clamp(0.0, 1.0);
        stats.samples_limited_low += usize::from(reconstructed < 0.0);
        stats.samples_limited_high += usize::from(reconstructed > 1.0);
        stats.samples_clipped += usize::from(reconstructed != value);
        stats.output_min = stats.output_min.min(value);
        stats.output_max = stats.output_max.max(value);
        stats.samples_below_zero += usize::from(value < 0.0);
        stats.samples_above_one += usize::from(value > 1.0);
        value
    })?;
    Ok(stats)
}

/// Diagnostic final RGB maps: normalized fractions, never ADU or scientific flux.
/// `master` is the companion master's metadata: the map lies on the master's
/// pixel grid, so its checked plate solution describes the map too (ADR-184),
/// and the map carries the master's median capture time (ADR-185).
pub fn write_fraction_map_atomic(
    path: &Path,
    image: &RgbImage,
    kind: &str,
    input_count: usize,
    master: Option<&MasterMetadata>,
    cancel: &AtomicBool,
) -> Result<()> {
    let format = format_of(path)?;
    if !["COVERAGE", "HIGHREJ"].contains(&kind)
        || input_count == 0
        || image.width == 0
        || image.height == 0
        || image.width.checked_mul(image.height) != Some(image.pixels.len())
        || image
            .pixels
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || !(0.0..=1.).contains(v))
    {
        return Err(Error::Invalid("Invalid fraction map."));
    }
    let planes = planes(image);
    let mut cards = vec![card("SIMPLE", "T"), card("BITPIX", "-32")];
    cards.push(card("NAXIS", if planes == 1 { "2" } else { "3" }));
    cards.push(card("NAXIS1", &image.width.to_string()));
    cards.push(card("NAXIS2", &image.height.to_string()));
    if planes == 3 {
        cards.push(card("NAXIS3", "3"));
    }
    cards.extend([
        card("EXTEND", "T"),
        card(
            "CREATOR",
            &format!("'FulgurStack {}'", env!("CARGO_PKG_VERSION")),
        ),
        card("BUNIT", "'fraction'"),
        card("MAPTYPE", &format!("'{kind}'")),
        card("NINPUT", &input_count.to_string()),
        card("PCTFACT", "100"),
        card("ROWORDER", "'TOP-DOWN'"),
        history("0..1 represents 0..100 percent of ALL original input frames."),
        history("Per-channel counts, not photometric flux, weights or exposure."),
        history("Map geometry and pixel centers match the companion master exactly."),
    ]);
    cards.push(history(if kind == "COVERAGE" {
        "Surviving samples after frame selection, geometry and pixel rejection."
    } else {
        "Upper rejected samples only; excludes low rejection and invalid edges."
    }));
    if let Some(median) = master.and_then(|m| m.median_date_obs.as_ref()) {
        median_time_cards(&mut cards, median);
    }
    if let Some(astrometry) = master
        .and_then(|m| m.astrometry.as_ref())
        .filter(|a| !a.cards.is_empty())
    {
        for (key, value, comment) in &astrometry.cards {
            cards.push(card_with_comment(key, value, comment));
        }
        for line in &astrometry.history {
            cards.push(history(line));
        }
        cards.push(history(
            "WCS copied from the companion master's plate solution.",
        ));
    }
    let channels: &[usize] = if planes == 1 { &[1] } else { &[0, 1, 2] };
    write_image(path, format, cards, image, channels, cancel, |value| value)
}
