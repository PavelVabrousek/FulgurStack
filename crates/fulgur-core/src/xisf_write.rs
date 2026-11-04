//! XISF 1.0 writer for masters and diagnostic maps (ADR-142): one monolithic
//! unit holding one 32-bit float image, planar and little-endian, bounded
//! 0..1. Its FITS keywords are the same cards the FITS writer stores, so the
//! two formats carry the same provenance; storage keywords are left out,
//! because XISF describes the storage in the image element itself.

use crate::Result;
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

/// Data blocks start on this boundary, as PixInsight writes them.
const BLOCK_ALIGNMENT: usize = 4096;
/// Keywords that describe FITS storage; XISF states them in the image element.
const STORAGE_KEYWORDS: [&str; 12] = [
    "SIMPLE", "BITPIX", "NAXIS", "NAXIS1", "NAXIS2", "NAXIS3", "EXTEND", "BZERO", "BSCALE",
    "BLANK", "ROWORDER", "END",
];

/// Writes the unit: signature, header and the image block written by `data`,
/// which must write exactly `width * height * planes` little-endian floats.
pub(crate) fn write_unit(
    writer: &mut dyn Write,
    width: usize,
    height: usize,
    planes: usize,
    cards: &[[u8; 80]],
    data: impl FnOnce(&mut dyn Write) -> Result<()>,
) -> Result<()> {
    let size = width * height * planes * 4;
    let keywords = keywords(cards);
    let created = creation_time(SystemTime::now());
    let mut position = BLOCK_ALIGNMENT;
    let xml = loop {
        let xml = header(width, height, planes, &keywords, &created, position, size);
        if 16 + xml.len() <= position {
            break xml;
        }
        position = (16 + xml.len()).div_ceil(BLOCK_ALIGNMENT) * BLOCK_ALIGNMENT;
    };
    let length =
        u32::try_from(xml.len()).map_err(|_| crate::Error::Invalid("XISF header too long."))?;
    writer.write_all(b"XISF0100")?;
    writer.write_all(&length.to_le_bytes())?;
    writer.write_all(&[0; 4])?;
    writer.write_all(xml.as_bytes())?;
    writer.write_all(&vec![0; position - 16 - xml.len()])?;
    data(writer)
}

fn header(
    width: usize,
    height: usize,
    planes: usize,
    keywords: &str,
    created: &str,
    position: usize,
    size: usize,
) -> String {
    let colour = if planes == 3 { "RGB" } else { "Gray" };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<xisf version=\"1.0\" xmlns=\"http://www.pixinsight.com/xisf\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:schemaLocation=\"http://www.pixinsight.com/xisf http://pixinsight.com/xisf/xisf-1.0.xsd\">\n\
<Image geometry=\"{width}:{height}:{planes}\" sampleFormat=\"Float32\" bounds=\"0:1\" colorSpace=\"{colour}\" location=\"attachment:{position}:{size}\">\n\
{keywords}</Image>\n\
<Metadata>\n\
<Property id=\"XISF:CreationTime\" type=\"TimePoint\" value=\"{created}\"/>\n\
<Property id=\"XISF:CreatorApplication\" type=\"String\">FulgurStack {}</Property>\n\
<Property id=\"XISF:BlockAlignmentSize\" type=\"UInt16\" value=\"{BLOCK_ALIGNMENT}\"/>\n\
</Metadata>\n\
</xisf>",
        env!("CARGO_PKG_VERSION")
    )
}

/// The cards as FITSKeyword elements, in their order; string values keep
/// their quotes, as PixInsight stores them.
fn keywords(cards: &[[u8; 80]]) -> String {
    let mut out = String::new();
    for card in cards {
        let text = String::from_utf8_lossy(card);
        let name = text[..8].trim();
        if name.is_empty() || STORAGE_KEYWORDS.contains(&name) {
            continue;
        }
        let (value, comment) = if &text[8..10] == "= " && !["HISTORY", "COMMENT"].contains(&name) {
            split_value(&text[10..])
        } else {
            (String::new(), text[8..].trim().to_owned())
        };
        out.push_str(&format!(
            "<FITSKeyword name=\"{}\" value=\"{}\" comment=\"{}\"/>\n",
            escape(name),
            escape(&value),
            escape(&comment)
        ));
    }
    out
}

/// A card's value and comment; a quoted value ends at its closing quote,
/// where `''` stands for one quote inside it.
fn split_value(rest: &str) -> (String, String) {
    let rest = rest.trim_start();
    let (value, after) = if rest.starts_with('\'') {
        let bytes = rest.as_bytes();
        let mut end = 1;
        while end < bytes.len() {
            if bytes[end] == b'\'' {
                if bytes.get(end + 1) == Some(&b'\'') {
                    end += 2;
                    continue;
                }
                end += 1;
                break;
            }
            end += 1;
        }
        rest.split_at(end.min(rest.len()))
    } else {
        rest.split_once('/')
            .map_or((rest, ""), |(value, _)| (value, &rest[value.len()..]))
    };
    let comment = after.trim_start().strip_prefix('/').unwrap_or("").trim();
    (value.trim().to_owned(), comment.to_owned())
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// UTC time as ISO 8601, e.g. 2026-11-04T18:20:05Z.
fn creation_time(now: SystemTime) -> String {
    let seconds = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, time) = (seconds / 86_400, seconds % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        time / 3_600,
        time % 3_600 / 60,
        time % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn creation_time_is_utc_iso() {
        let at = UNIX_EPOCH + Duration::from_secs(1_793_816_405);
        assert_eq!(creation_time(at), "2026-11-04T18:20:05Z");
        assert_eq!(creation_time(UNIX_EPOCH), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn card_values_and_comments_split_like_fits() {
        assert_eq!(
            split_value("'It''s / not'         / a comment"),
            ("'It''s / not'".to_owned(), "a comment".to_owned())
        );
        assert_eq!(
            split_value("                10.0 / [s] exposure"),
            ("10.0".to_owned(), "[s] exposure".to_owned())
        );
        assert_eq!(
            split_value("                   T"),
            ("T".to_owned(), String::new())
        );
    }
}
