//! Reconstruction of light-frame metadata missing from FITS headers (ADR-084).
//!
//! Priority: the primary header card; other header cards that carry the same
//! value; tokens of the file name; the embedded camera catalogue. Values that
//! change processing (exposure, colour layout) must be confirmed by the owner.

use crate::BayerPattern;
use crate::cameras::{self, CameraModel};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DerivedValue {
    /// "exposure_seconds", "sensor_layout", "gain" or "object".
    pub field: &'static str,
    pub value: String,
    /// Where it came from, e.g. "file name token '30.0s'".
    pub source: String,
    /// Exposure and colour layout change the result and need the owner's
    /// confirmation; gain and object are labels only.
    pub needs_confirmation: bool,
}

pub const EXPOSURE: &str = "exposure_seconds";
pub const LAYOUT: &str = "sensor_layout";

/// Header cards that some capture programs use instead of BAYERPAT.
const PATTERN_ALIASES: [&str; 4] = ["COLORTYP", "CFAPAT", "CFA-PAT", "CFATYPE"];
/// Header cards that may name the camera, in priority order.
const CAMERA_CARDS: [&str; 4] = ["INSTRUME", "CAMERA", "TELESCOP", "CREATOR"];

fn stem(name: &str) -> &str {
    let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

fn tokens(name: &str) -> impl Iterator<Item = &str> {
    stem(name).split(['_', ' ']).filter(|t| !t.is_empty())
}

fn positive(text: &str) -> Option<f64> {
    if !text.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    text.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v > 0.0)
}

/// Exposure from a token such as "30.0s", "300s", "120sec" or "500ms".
fn exposure_token(token: &str) -> Option<f64> {
    let token = token.to_ascii_lowercase();
    if let Some(ms) = token.strip_suffix("ms") {
        return positive(ms).map(|v| v / 1000.0);
    }
    let seconds = token
        .strip_suffix("sec")
        .or_else(|| token.strip_suffix('s'))?;
    positive(seconds)
}

/// A file name gives a value only when it names exactly one.
fn single<T: PartialEq + Copy>(values: impl Iterator<Item = (T, String)>) -> Option<(T, String)> {
    let mut found: Option<(T, String)> = None;
    for (value, token) in values {
        match &found {
            None => found = Some((value, token)),
            Some((first, _)) if *first == value => {}
            Some(_) => return None,
        }
    }
    found
}

/// One exposure card read as seconds. `note` says how its unit was settled
/// when that was not plain seconds, or why it is doubtful.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ExposureCard {
    pub seconds: f64,
    pub note: Option<String>,
}

/// A single frame longer than this (an hour) with no stated unit is more
/// likely milliseconds than seconds.
pub(crate) const MAX_PLAUSIBLE_EXPOSURE_S: f64 = 3600.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unit {
    Seconds,
    Milliseconds,
}

/// The unit a card's comment declares, such as "[ms] Total Exposure Time" or
/// "exposure in seconds". A bracketed unit leads; without brackets the
/// comment must name one kind of unit only, else it declares nothing.
pub(crate) fn unit_in_comment(comment: &str) -> Option<Unit> {
    let lower = comment.to_ascii_lowercase();
    let word = |text: &str| match text.trim() {
        "ms" | "msec" | "msecs" | "millisecond" | "milliseconds" => Some(Unit::Milliseconds),
        "s" | "sec" | "secs" | "second" | "seconds" => Some(Unit::Seconds),
        _ => None,
    };
    if let (Some(open), Some(close)) = (lower.find('['), lower.find(']')) {
        if open < close {
            if let Some(unit) = word(&lower[open + 1..close]) {
                return Some(unit);
            }
        }
    }
    let mut found = None;
    for token in lower.split(|c: char| !c.is_ascii_alphanumeric()) {
        if let Some(unit) = word(token) {
            match found {
                None => found = Some(unit),
                Some(first) if first != unit => return None,
                _ => {}
            }
        }
    }
    found
}

/// The derived entry for an exposure card whose unit was not plain seconds:
/// the converted value, for the owner to confirm like any reconstruction.
pub(crate) fn unit_note(key: &str, card: &ExposureCard) -> Option<DerivedValue> {
    card.note.as_ref().map(|note| DerivedValue {
        field: EXPOSURE,
        value: format!("{} s", trim(card.seconds)),
        source: format!("header card {key} ({note})"),
        needs_confirmation: true,
    })
}

/// Exposure when EXPTIME is missing: the EXPOSURE card, else the file name.
pub(crate) fn exposure(
    card: impl Fn(&str) -> Option<ExposureCard>,
    name: Option<&str>,
) -> Option<(f64, DerivedValue)> {
    let derived = |value: f64, source: String| DerivedValue {
        field: EXPOSURE,
        value: format!("{} s", trim(value)),
        source,
        needs_confirmation: true,
    };
    if let Some(found) = card("EXPOSURE").filter(|c| c.seconds.is_finite() && c.seconds > 0.0) {
        let source = match &found.note {
            Some(note) => format!("header card EXPOSURE ({note})"),
            None => "header card EXPOSURE".into(),
        };
        return Some((found.seconds, derived(found.seconds, source)));
    }
    let (value, token) = single(
        tokens(name?).filter_map(|t| exposure_token(t).map(|v| (v.to_bits(), t.to_owned()))),
    )?;
    let value = f64::from_bits(value);
    Some((value, derived(value, format!("file name token '{token}'"))))
}

/// Gain when the GAIN card is missing: a "gain100" file-name token.
pub(crate) fn gain(name: Option<&str>) -> Option<(f64, DerivedValue)> {
    let all: Vec<&str> = tokens(name?).collect();
    let (value, token) = single(all.iter().enumerate().filter_map(|(i, t)| {
        let lower = t.to_ascii_lowercase();
        let rest = lower.strip_prefix("gain")?;
        let text = if rest.is_empty() {
            all.get(i + 1)?.to_string()
        } else {
            rest.to_owned()
        };
        let value = text
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && *v >= 0.0)?;
        Some((value.to_bits(), t.to_string()))
    }))?;
    let value = f64::from_bits(value);
    Some((
        value,
        DerivedValue {
            field: "gain",
            value: trim(value),
            source: format!("file name token '{token}'"),
            needs_confirmation: false,
        },
    ))
}

/// Target when OBJECT is missing: the part after "Light_" in ASIAIR/N.I.N.A. names.
pub(crate) fn object(name: Option<&str>) -> Option<(String, DerivedValue)> {
    let parts: Vec<&str> = stem(name?).split('_').collect();
    let index = parts
        .iter()
        .position(|p| p.eq_ignore_ascii_case("light") || p.eq_ignore_ascii_case("lights"))?;
    let target = parts.get(index + 1)?.trim();
    if target.is_empty() || exposure_token(target).is_some() || positive(target).is_some() {
        return None;
    }
    Some((
        target.to_owned(),
        DerivedValue {
            field: "object",
            value: target.to_owned(),
            source: "file name (after 'Light_')".into(),
            needs_confirmation: false,
        },
    ))
}

pub(crate) struct Layout {
    /// None: a colour sensor whose pattern the owner must choose.
    pub pattern: Option<BayerPattern>,
    pub source: &'static str,
    pub derived: DerivedValue,
}

/// Colour layout when BAYERPAT is missing. None keeps the mono default
/// (ADR-078), also when the catalogue identifies a monochrome camera.
pub(crate) fn layout(
    card: impl Fn(&str) -> Option<String>,
    name: Option<&str>,
    width: usize,
    height: usize,
) -> Option<Layout> {
    let derived = |value: String, source: String| DerivedValue {
        field: LAYOUT,
        value,
        source,
        needs_confirmation: true,
    };
    for key in PATTERN_ALIASES {
        if let Some(pattern) = card(key).and_then(|v| BayerPattern::parse(&v).ok()) {
            return Some(Layout {
                pattern: Some(pattern),
                source: "header_alias",
                derived: derived(
                    format!("colour {}", pattern.name()),
                    format!("header card {key}"),
                ),
            });
        }
    }
    if let Some(name) = name {
        let found = single(
            tokens(name).filter_map(|t| BayerPattern::parse(t).ok().map(|p| (p, t.to_owned()))),
        );
        if let Some((pattern, token)) = found {
            return Some(Layout {
                pattern: Some(pattern),
                source: "file_name",
                derived: derived(
                    format!("colour {}", pattern.name()),
                    format!("file name token '{token}'"),
                ),
            });
        }
    }
    let (camera, from) = CAMERA_CARDS
        .iter()
        .find_map(|key| {
            let text = card(key)?;
            cameras::identify(&text).map(|c| (c, format!("header card {key} '{text}'")))
        })
        .or_else(|| {
            tokens(name?).find_map(|t| {
                cameras::identify_token(t).map(|c| (c, format!("file name token '{t}'")))
            })
        })?;
    if !camera.is_colour() {
        return None;
    }
    let pattern = camera.bayer_pattern();
    Some(Layout {
        pattern,
        source: "camera_catalog",
        derived: derived(
            match pattern {
                Some(p) => format!("colour {}", p.name()),
                None => "colour, Bayer pattern not catalogued (choose it)".into(),
            },
            catalogue_source(camera, &from, width, height),
        ),
    })
}

fn catalogue_source(camera: &CameraModel, from: &str, width: usize, height: usize) -> String {
    let mut text = format!(
        "camera catalogue: {} ({}), identified from {from}; confidence {}",
        camera.label(),
        camera.sensor,
        camera.confidence
    );
    if camera.native_size(width, height) == Some(false) {
        text.push_str(&format!(
            "; frame {width}x{height} differs from the native {}x{}, the pattern may be shifted",
            camera.width.unwrap_or(0),
            camera.height.unwrap_or(0)
        ));
    }
    text
}

pub(crate) fn trim(value: f64) -> String {
    let text = format!("{value:.4}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASIAIR: &str =
        "c_Light_M 42_270deg_30.0s_Bin1_2600MC_gain100_20260119-200800_-20.0C_SQA_0001.fit";

    #[test]
    fn asiair_file_names_give_exposure_gain_object_and_camera() {
        let none = |_: &str| None;
        let (exposure, d) = exposure(none, Some(ASIAIR)).unwrap();
        assert_eq!(exposure, 30.0);
        assert_eq!(d.source, "file name token '30.0s'");
        assert!(d.needs_confirmation);
        assert_eq!(gain(Some(ASIAIR)).unwrap().0, 100.0);
        assert_eq!(object(Some(ASIAIR)).unwrap().0, "M 42");
        let layout = layout(|_: &str| None, Some(ASIAIR), 6248, 4176).unwrap();
        assert_eq!(
            (layout.pattern, layout.source),
            (Some(BayerPattern::RGGB), "camera_catalog")
        );
        assert!(layout.derived.source.contains("file name token '2600MC'"));
    }

    #[test]
    fn header_cards_win_over_the_file_name_and_the_catalogue() {
        let cards = |key: &str| match key {
            "EXPOSURE" => Some(ExposureCard {
                seconds: 60.0,
                note: None,
            }),
            _ => None,
        };
        assert_eq!(exposure(cards, Some(ASIAIR)).unwrap().0, 60.0);
        let text = |key: &str| match key {
            "COLORTYP" => Some("GBRG".to_owned()),
            "INSTRUME" => Some("ZWO ASI2600MC Air".to_owned()),
            _ => None,
        };
        let layout = layout(text, Some(ASIAIR), 6248, 4176).unwrap();
        assert_eq!(layout.pattern, Some(BayerPattern::GBRG));
        assert_eq!(layout.derived.source, "header card COLORTYP");
        let instrument = |key: &str| (key == "INSTRUME").then(|| "ZWO ASI2600MC Air".to_owned());
        let named = layout_of(instrument, "Light_x_BGGR_10s.fit");
        assert_eq!(named.pattern, Some(BayerPattern::BGGR));
        let catalogue = layout_of(instrument, "Light_x_10s.fit");
        assert!(
            catalogue
                .derived
                .source
                .contains("INSTRUME 'ZWO ASI2600MC Air'")
        );
    }

    fn layout_of(card: impl Fn(&str) -> Option<String>, name: &str) -> Layout {
        layout(card, Some(name), 6248, 4176).unwrap()
    }

    #[test]
    fn a_comment_declares_the_unit_only_when_it_is_unambiguous() {
        use Unit::{Milliseconds, Seconds};
        assert_eq!(
            unit_in_comment(" [ms] Total Exposure Time"),
            Some(Milliseconds)
        );
        assert_eq!(unit_in_comment("[s] exposure time"), Some(Seconds));
        assert_eq!(unit_in_comment("Exposure time in seconds"), Some(Seconds));
        assert_eq!(
            unit_in_comment("exposure, milliseconds"),
            Some(Milliseconds)
        );
        // A bracketed unit outranks other words.
        assert_eq!(unit_in_comment("[ms] (precision 1 s)"), Some(Milliseconds));
        // Both kinds without brackets, or neither: nothing is declared.
        assert_eq!(unit_in_comment("exposure in ms, not seconds"), None);
        assert_eq!(unit_in_comment("Total Exposure Time"), None);
        assert_eq!(unit_in_comment(""), None);
        // Words that merely contain "ms" or "s" are no units.
        assert_eq!(unit_in_comment("items stored"), None);
    }

    #[test]
    fn mono_cameras_unknown_names_and_ambiguous_tokens_derive_nothing() {
        let mono = |key: &str| (key == "INSTRUME").then(|| "ZWO ASI2600MM Air".to_owned());
        assert!(layout(mono, Some("Light_x_300.0s.fit"), 6248, 4176).is_none());
        assert!(layout(|_: &str| None, Some("frame_0001.fit"), 100, 100).is_none());
        assert!(exposure(|_: &str| None, Some("frame_0001.fit")).is_none());
        assert!(exposure(|_: &str| None, Some("a_30s_60s.fit")).is_none());
        assert_eq!(
            exposure(|_: &str| None, Some("x_500ms.fits")).unwrap().0,
            0.5
        );
        assert!(object(Some("Light_30.0s_1.fit")).is_none());
        let unknown = |key: &str| (key == "INSTRUME").then(|| "ZWO ASI120MC-S".to_owned());
        let colour = layout(unknown, None, 1280, 960).unwrap();
        assert_eq!(colour.pattern, None);
        let cropped = |key: &str| (key == "INSTRUME").then(|| "ZWO ASI2600MC Pro".to_owned());
        let shifted = layout(cropped, None, 3000, 2000).unwrap();
        assert!(shifted.derived.source.contains("may be shifted"));
    }
}
