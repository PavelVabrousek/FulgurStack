//! Embedded catalogue of astronomy cameras, smart telescopes and DSLRs
//! (`data/cameras.json`, ADR-084). It is the last-resort source for a light
//! frame's sensor layout when the FITS header has no BAYERPAT; every value
//! taken from it must be confirmed by the owner before processing.

use crate::BayerPattern;
use serde::Deserialize;
use std::sync::OnceLock;

const CATALOGUE: &str = include_str!("../data/cameras.json");

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct CameraModel {
    pub manufacturer: String,
    pub model: String,
    #[serde(rename = "match")]
    pub tokens: Vec<String>,
    pub sensor: String,
    /// "colour" or "mono".
    #[serde(rename = "type")]
    pub kind: String,
    /// Absent for colour sensors whose pattern is not catalogued.
    pub bayer: Option<String>,
    pub width: Option<usize>,
    pub height: Option<usize>,
    pub pixel_um: Option<f64>,
    pub adc_bits: Option<u32>,
    pub confidence: String,
    #[serde(default)]
    pub source: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
    /// "ms" for a camera that writes its exposure in milliseconds into
    /// EXPTIME or EXPOSURE (the Vaonis Stellina); absent means seconds.
    #[serde(default)]
    pub exposure_unit: Option<String>,
}

impl CameraModel {
    /// The unit of this camera's exposure cards when the card's own comment
    /// states none.
    pub(crate) fn exposure_unit(&self) -> Option<crate::derive::Unit> {
        match self.exposure_unit.as_deref() {
            Some("ms") => Some(crate::derive::Unit::Milliseconds),
            _ => None,
        }
    }

    pub fn is_colour(&self) -> bool {
        self.kind == "colour"
    }

    pub fn bayer_pattern(&self) -> Option<BayerPattern> {
        self.bayer
            .as_deref()
            .and_then(|p| BayerPattern::parse(p).ok())
    }

    pub fn label(&self) -> String {
        format!("{} {}", self.manufacturer, self.model)
    }

    /// True when the frame has the catalogue's full-sensor size (either orientation).
    pub fn native_size(&self, width: usize, height: usize) -> Option<bool> {
        let (w, h) = (self.width?, self.height?);
        Some((w, h) == (width, height) || (h, w) == (width, height))
    }
}

#[derive(Deserialize)]
struct Catalogue {
    cameras: Vec<CameraModel>,
}

pub fn catalogue() -> &'static [CameraModel] {
    static CAMERAS: OnceLock<Vec<CameraModel>> = OnceLock::new();
    CAMERAS.get_or_init(|| {
        serde_json::from_str::<Catalogue>(CATALOGUE)
            .map(|c| c.cameras)
            .unwrap_or_default()
    })
}

fn normalized(text: &str) -> String {
    text.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Camera named anywhere in a header value such as INSTRUME "ZWO ASI2600MC Air".
/// The longest matching token wins, so "ASI2600MC" never matches "ASI2600MM".
pub fn identify(text: &str) -> Option<&'static CameraModel> {
    let text = normalized(text);
    best(|token| text.contains(token))
}

/// Camera named by a whole file-name token such as "2600MC" (ASIAIR names).
pub fn identify_token(token: &str) -> Option<&'static CameraModel> {
    let token = normalized(token);
    best(|candidate| candidate == token)
}

fn best(matches: impl Fn(&str) -> bool) -> Option<&'static CameraModel> {
    catalogue()
        .iter()
        .flat_map(|camera| camera.tokens.iter().map(move |t| (camera, normalized(t))))
        .filter(|(_, token)| !token.is_empty() && matches(token))
        .max_by_key(|(_, token)| token.len())
        .map(|(camera, _)| camera)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_parses_with_valid_patterns_and_unique_tokens() {
        let cameras = catalogue();
        assert!(cameras.len() >= 60, "{} cameras", cameras.len());
        let mut tokens = std::collections::HashSet::new();
        for camera in cameras {
            assert!(["colour", "mono"].contains(&camera.kind.as_str()));
            assert!(
                [
                    "verified_local",
                    "vendor",
                    "retailer",
                    "community",
                    "typical"
                ]
                .contains(&camera.confidence.as_str())
            );
            if let Some(pattern) = &camera.bayer {
                assert!(BayerPattern::parse(pattern).is_ok(), "{}", camera.model);
                assert!(camera.is_colour());
            }
            for token in &camera.tokens {
                assert!(tokens.insert(normalized(token)), "duplicate {token}");
            }
        }
    }

    #[test]
    fn models_are_told_apart_by_the_longest_token() {
        let colour = identify("ZWO ASI2600MC Air").unwrap();
        assert_eq!(
            (colour.model.as_str(), colour.bayer_pattern()),
            ("ASI2600MC", Some(BayerPattern::RGGB))
        );
        assert!(!identify("ZWO ASI2600MM Air").unwrap().is_colour());
        assert_eq!(
            identify("ZWO Seestar S50 Pro").unwrap().model,
            "Seestar S50 Pro"
        );
        assert_eq!(identify("Seestar S50").unwrap().model, "Seestar S50");
        assert_eq!(identify("Canon EOS Ra").unwrap().model, "EOS Ra");
        assert_eq!(identify("Canon EOS 800D").unwrap().model, "EOS (generic)");
        assert_eq!(identify_token("2600MC").unwrap().model, "ASI2600MC");
        assert!(identify_token("gain100").is_none());
        let stellina = identify("stellina-43ab43").unwrap();
        assert_eq!(stellina.label(), "Vaonis Stellina");
        assert_eq!(
            stellina.exposure_unit(),
            Some(crate::derive::Unit::Milliseconds)
        );
        assert_eq!(colour.exposure_unit(), None);
        assert!(identify("Unknown camera").is_none());
        assert!(
            identify("ZWO ASI120MC-S")
                .unwrap()
                .bayer_pattern()
                .is_none()
        );
    }
}
