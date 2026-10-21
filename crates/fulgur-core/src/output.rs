//! Atomic no-clobber output; only the final FITS/report uses a .tmp file.

use crate::fits::{BLOCK_SIZE, FitsHeader};
use crate::{Error, Result, RgbImage, check_cancelled};
use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::AtomicBool;

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

pub fn write_rgb_atomic(
    path: &Path,
    image: &RgbImage,
    source: &FitsHeader,
    cancel: &AtomicBool,
) -> Result<()> {
    if image.width != source.info.width
        || image.height != source.info.height
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
    let mut cards = vec![
        card("SIMPLE", "T"),
        card("BITPIX", "-32"),
        card("NAXIS", "3"),
        card("NAXIS1", &image.width.to_string()),
        card("NAXIS2", &image.height.to_string()),
        card("NAXIS3", "3"),
        card("EXTEND", "T"),
        card("CREATOR", "'FulgurStack 0.1.0'"),
        card("BUNIT", "'ADU'"),
        card("NCOMBINE", "1"),
        card("TEXPTIME", &source.info.exposure_seconds.to_string()),
    ];
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
        "Linear physical source ADU; no stretch, clipping, or BIAS subtraction.",
    ));
    cards.push(history(
        "Same-color isolated hot-pixel correction; 5x5 Malvar-He-Cutler RGB.",
    ));
    cards.push(history(
        "Sensor row and column order preserved; no image flip applied.",
    ));
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
                    bytes.copy_from_slice(&pixel[channel].to_be_bytes());
                }
                writer.write_all(&row)?;
            }
        }
        let bytes = image.pixels.len() * 12;
        let padding = (BLOCK_SIZE - bytes % BLOCK_SIZE) % BLOCK_SIZE;
        writer.write_all(&vec![0; padding])?;
        Ok(())
    })
}
