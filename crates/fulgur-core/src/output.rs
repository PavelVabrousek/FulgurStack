//! Atomic no-clobber output; only the final FITS/report uses a .tmp file.

use crate::fits::{BLOCK_SIZE, FitsHeader};
use crate::{Error, Result, RgbImage, check_cancelled};
use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::AtomicBool;

/// Fixed sensor full scale, shared by every image/channel; never an image extremum.
pub const OUTPUT_ADU_DIVISOR: f32 = 65_535.0;

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

fn card(key: &str, value: &str) -> [u8; 80] {
    let text = if value.starts_with('\'') {
        format!("{key:<8}= {value:<20}")
    } else {
        format!("{key:<8}= {value:>20}")
    };
    let mut card = [b' '; 80];
    card[..text.len()].copy_from_slice(text.as_bytes());
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
pub struct MasterMetadata {
    pub accepted_frames: usize,
    pub total_exposure_seconds: f64,
    pub origin_x_px: i32,
    pub origin_y_px: i32,
}

pub fn write_rgb_atomic(
    path: &Path,
    image: &RgbImage,
    source: &FitsHeader,
    cancel: &AtomicBool,
) -> Result<RgbExportStats> {
    write_rgb_with_metadata(path, image, source, None, cancel)
}

pub fn write_master_atomic(
    path: &Path,
    image: &RgbImage,
    source: &FitsHeader,
    metadata: &MasterMetadata,
    cancel: &AtomicBool,
) -> Result<RgbExportStats> {
    if metadata.accepted_frames == 0
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
    source: &FitsHeader,
    metadata: Option<&MasterMetadata>,
    cancel: &AtomicBool,
) -> Result<RgbExportStats> {
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
    let mut cards = vec![
        card("SIMPLE", "T"),
        card("BITPIX", "-32"),
        card("NAXIS", "3"),
        card("NAXIS1", &image.width.to_string()),
        card("NAXIS2", &image.height.to_string()),
        card("NAXIS3", "3"),
        card("EXTEND", "T"),
        card("CREATOR", "'FulgurStack 0.1.0'"),
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
    ];
    if let Some(m) = metadata {
        cards.push(card("CROPX", &m.origin_x_px.to_string()));
        cards.push(card("CROPY", &m.origin_y_px.to_string()));
        cards.push(card("REGMODEL", "'SE(2)'"));
        cards.push(card("REJECT", "'WINSOR-MAD'"));
        cards.push(history(
            "Weighted linear mean on the reference-anchor intensity scale.",
        ));
        cards.push(history(
            "EXPTIME is anchor exposure; TEXPTIME is accepted acquisition total.",
        ));
        cards.push(history(
            "CROPX/CROPY locate output (0,0) in reference-anchor pixel centers.",
        ));
        cards.push(history(
            "No plate solution; crop offsets are not a celestial WCS.",
        ));
    }
    // Do not propagate CFA, scaling, checksum, display-range, or unverified WCS cards.
    for key in [
        "OBJECT", "FILTER", "DATE-OBS", "EXPTIME", "INSTRUME", "TELESCOP", "OBSERVER", "FOCALLEN",
        "XPIXSZ", "YPIXSZ", "GAIN", "ISOSPEED", "CCD-TEMP",
    ] {
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
    cards.push(history(
        "Conservative same-color isolated hot-pixel correction.",
    ));
    cards.push(history(&format!(
        "5x5 {} RGB reconstruction; native CFA samples retained after cosmetics.",
        image.demosaic_method.name()
    )));
    cards.push(history(if metadata.is_some() {
        "Registered in anchor top-down coordinates; no display flip applied."
    } else {
        "Sensor row and column order preserved; no image flip applied."
    }));
    let mut end = [b' '; 80];
    end[..3].copy_from_slice(b"END");
    cards.push(end);
    atomic_write(path, cancel, |writer| {
        for card in &cards {
            writer.write_all(card)?;
        }
        let header_padding = (BLOCK_SIZE - cards.len() * 80 % BLOCK_SIZE) % BLOCK_SIZE;
        writer.write_all(&vec![b' '; header_padding])?;
        let mut row = vec![0; image.width * 4];
        for channel in 0..3 {
            for pixels in image.pixels.chunks_exact(image.width) {
                check_cancelled(cancel)?;
                for (bytes, pixel) in row.chunks_exact_mut(4).zip(pixels) {
                    let reconstructed = pixel[channel] / OUTPUT_ADU_DIVISOR;
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
                    bytes.copy_from_slice(&value.to_be_bytes());
                }
                writer.write_all(&row)?;
            }
        }
        let bytes = image.pixels.len() * 12;
        let padding = (BLOCK_SIZE - bytes % BLOCK_SIZE) % BLOCK_SIZE;
        writer.write_all(&vec![0; padding])?;
        Ok(())
    })?;
    Ok(stats)
}
