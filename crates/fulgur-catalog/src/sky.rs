//! Everything sky registration needs from outside the pipeline (plan phase 4,
//! ADR-138): the field centre and a catalogue provider over the configured
//! Gaia sources. The CLI and the application build the same options here.
use crate::{GaiaSources, Getter, Position};
use fulgur_pipeline::inspect::InspectReport;
use fulgur_pipeline::register::SkyOptions;
use std::sync::Arc;

/// The owner's choices for one sky registration run.
#[derive(Clone, Debug, Default)]
pub struct SkyChoice {
    pub sources: GaiaSources,
    /// Field centre in degrees, when entered.
    pub centre: Option<(f64, f64)>,
    /// Target name; otherwise the most common OBJECT card.
    pub object: Option<String>,
    /// Ask NED and SIMBAD when the built-in table lacks the name.
    pub online: bool,
}

/// The OBJECT card most quality-accepted frames carry.
pub fn common_object(inspection: &InspectReport) -> Option<String> {
    let mut names: std::collections::BTreeMap<String, usize> = Default::default();
    for frame in inspection.frames.iter().filter(|f| f.accepted) {
        if let Some(object) = frame.frame.as_ref().and_then(|i| i.object.clone()) {
            let object = object.trim().to_string();
            if !object.is_empty() {
                *names.entry(object).or_default() += 1;
            }
        }
    }
    names.into_iter().max_by_key(|(_, n)| *n).map(|(o, _)| o)
}

/// Field centre and catalogue provider for `inspection`. `say` receives one
/// line per catalogue cone read. Err explains, in the owner's terms, what is
/// missing.
pub fn sky_options(
    choice: &SkyChoice,
    inspection: &InspectReport,
    say: Arc<dyn Fn(String) + Send + Sync>,
) -> Result<(SkyOptions, Position), String> {
    if choice.sources.pixinsight.is_none() && !choice.sources.download {
        return Err(
            "Sky registration needs Gaia stars: choose the folder of the PixInsight Gaia DR3 files or allow downloading.".into(),
        );
    }
    let object = choice
        .object
        .clone()
        .filter(|o| !o.trim().is_empty())
        .or_else(|| common_object(inspection));
    let online: Option<&Getter> = if choice.online {
        Some(&crate::lookup::http_get)
    } else {
        None
    };
    let position =
        crate::position(choice.centre, None, object.as_deref(), online).map_err(|notes| {
            format!(
                "No field centre for sky registration: {}.",
                notes.join("; ")
            )
        })?;
    let sources = choice.sources.clone();
    let provider = move |ra: f64, dec: f64, radius: f64, g: f64| {
        let field = crate::gaia_field(&sources, ra, dec, radius, g)?;
        say(format!(
            "{} Gaia stars to G {g} within {radius} degrees from {}",
            field.stars.len(),
            field.source
        ));
        Ok(field.stars)
    };
    Ok((
        SkyOptions::new((position.ra, position.dec), Arc::new(provider)),
        position,
    ))
}

/// Where a position came from, in words.
pub fn source_label(position: &Position) -> String {
    match &position.source {
        crate::PositionSource::Manual => "coordinates entered".into(),
        crate::PositionSource::Header => "FITS header".into(),
        crate::PositionSource::DeepSky(name) => format!("built-in table ({name})"),
        crate::PositionSource::Online(service) => format!("online ({service})"),
    }
}

/// Right ascension in degrees from decimal degrees ("23.462") or
/// sexagesimal hours ("01:33:50.9", "1h33m50.9s", "01 33 50.9").
pub fn parse_ra(text: &str) -> Option<f64> {
    let t = text.trim();
    let hours = t.contains(':') || t.contains('h') || t.split_whitespace().count() > 1;
    let value = if hours {
        sexagesimal(t)? * 15.
    } else {
        t.trim_end_matches(['°', 'd'])
            .replace(',', ".")
            .parse()
            .ok()?
    };
    (0. ..360.).contains(&value).then_some(value)
}

/// Declination in degrees from decimal degrees ("+30.66") or sexagesimal
/// degrees ("+30:39:36", "30d39m36s", "30 39 36").
pub fn parse_dec(text: &str) -> Option<f64> {
    let t = text.trim();
    let parts = t.contains(':') || t.contains('d') || t.split_whitespace().count() > 1;
    let value = if parts {
        sexagesimal(t)?
    } else {
        t.trim_end_matches('°').replace(',', ".").parse().ok()?
    };
    (-90. ..=90.).contains(&value).then_some(value)
}

fn sexagesimal(text: &str) -> Option<f64> {
    let negative = text.starts_with('-');
    let cleaned: String = text
        .trim_start_matches(['+', '-'])
        .chars()
        .map(|c| match c {
            ':' | 'h' | 'd' | 'm' | 's' | '°' | '\'' | '"' => ' ',
            ',' => '.',
            c => c,
        })
        .collect();
    let numbers: Vec<f64> = cleaned
        .split_whitespace()
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    if numbers.is_empty()
        || numbers.len() > 3
        || numbers[1..].iter().any(|v| !(0. ..60.).contains(v))
    {
        return None;
    }
    let value = numbers
        .iter()
        .zip([1., 60., 3600.])
        .map(|(v, d)| v / d)
        .sum::<f64>();
    Some(if negative { -value } else { value })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinates_read_as_decimal_degrees_or_sexagesimal() {
        let close = |a: Option<f64>, b: f64| a.is_some_and(|a| (a - b).abs() < 1e-6);
        assert!(close(parse_ra("23.4621"), 23.4621));
        assert!(close(
            parse_ra("01:33:50.9"),
            (1. + 33. / 60. + 50.9 / 3600.) * 15.
        ));
        assert!(close(
            parse_ra("1h33m50.9s"),
            (1. + 33. / 60. + 50.9 / 3600.) * 15.
        ));
        assert!(close(
            parse_ra("01 33 50.9"),
            (1. + 33. / 60. + 50.9 / 3600.) * 15.
        ));
        assert!(close(parse_dec("+30.66"), 30.66));
        assert!(close(parse_dec("+30:39:36"), 30.66));
        assert!(close(
            parse_dec("-05 23 28"),
            -(5. + 23. / 60. + 28. / 3600.)
        ));
        assert!(close(parse_dec("30d39m36s"), 30.66));
        assert!(parse_ra("400").is_none() && parse_ra("25:00:00").is_none());
        assert!(parse_dec("91").is_none() && parse_dec("30:61:00").is_none());
        assert!(parse_ra("").is_none() && parse_dec("north").is_none());
    }
}
