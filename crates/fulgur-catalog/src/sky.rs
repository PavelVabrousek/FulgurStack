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
    let online: Option<&Getter> = if choice.online {
        Some(&crate::lookup::http_get)
    } else {
        None
    };
    let (position, fallback) = field_centres(
        choice.centre,
        choice.object.as_deref(),
        common_object(inspection).as_deref(),
        &|centre, object| crate::position(centre, None, object, online),
        &*say,
    )?;
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
    let mut options = SkyOptions::new((position.ra, position.dec), Arc::new(provider));
    options.fallback_centres = fallback.iter().map(|f| (f.ra, f.dec)).collect();
    Ok((options, position))
}

/// Turns a manual centre or an object name into a position.
type Resolver<'a> = dyn Fn(Option<(f64, f64)>, Option<&str>) -> Result<Position, Vec<String>> + 'a;

/// The field centre to search and, when the owner's choice names another
/// field than the frames' own OBJECT, the frames' centre as a fallback: a
/// centre or object left over from another target finds no solution
/// (ADR-158). A choice that resolves nowhere falls back to the frames'
/// OBJECT at once. `resolve` turns a manual centre or a name into a position.
fn field_centres(
    centre: Option<(f64, f64)>,
    object: Option<&str>,
    frames_object: Option<&str>,
    resolve: &Resolver<'_>,
    say: &(dyn Fn(String) + Send + Sync),
) -> Result<(Position, Option<Position>), String> {
    let chosen = object.map(str::trim).filter(|o| !o.is_empty());
    let names_other_field = centre.is_some()
        || chosen.is_some_and(|chosen| frames_object.is_some_and(|own| !same_name(chosen, own)));
    let frames_position = || {
        frames_object
            .filter(|_| names_other_field)
            .and_then(|own| resolve(None, Some(own)).ok())
    };
    match resolve(centre, chosen.or(frames_object)) {
        Ok(position) => {
            let fallback =
                frames_position().filter(|own| separation_deg(&position, own) > SAME_FIELD_DEG);
            Ok((position, fallback))
        }
        Err(notes) => match frames_position() {
            Some(own) => {
                say(format!(
                    "the field chosen in Settings gives no centre ({}); the frames' OBJECT {} does",
                    notes.join("; "),
                    frames_object.unwrap_or_default()
                ));
                Ok((own, None))
            }
            None => Err(format!(
                "No field centre for sky registration: {}.",
                notes.join("; ")
            )),
        },
    }
}
/// Two centres closer than this name the same field.
const SAME_FIELD_DEG: f64 = 0.25;

/// Object names that differ only in case, spaces or punctuation ("M 45",
/// "m45") are the same name.
fn same_name(a: &str, b: &str) -> bool {
    let key = |s: &str| {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    key(a) == key(b)
}

/// Angular distance of two positions in degrees.
fn separation_deg(a: &Position, b: &Position) -> f64 {
    let (ra1, dec1, ra2, dec2) = (
        a.ra.to_radians(),
        a.dec.to_radians(),
        b.ra.to_radians(),
        b.dec.to_radians(),
    );
    let h = ((dec2 - dec1) / 2.).sin().powi(2)
        + dec1.cos() * dec2.cos() * ((ra2 - ra1) / 2.).sin().powi(2);
    2. * h.sqrt().min(1.).asin().to_degrees()
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

    /// A built-in table of two fields, as `position` resolves names.
    fn resolve(centre: Option<(f64, f64)>, object: Option<&str>) -> Result<Position, Vec<String>> {
        let at = |ra, dec, name: &str| Position {
            ra,
            dec,
            source: crate::PositionSource::DeepSky(name.into()),
            notes: vec![],
        };
        if let Some((ra, dec)) = centre {
            return Ok(Position {
                ra,
                dec,
                source: crate::PositionSource::Manual,
                notes: vec![],
            });
        }
        match object.map(|o| o.to_lowercase().replace(' ', "")) {
            Some(o) if o == "m45" => Ok(at(56.87, 24.11, "Mel 22")),
            Some(o) if o == "m33" => Ok(at(23.46, 30.66, "NGC 598")),
            _ => Err(vec!["unknown object".into()]),
        }
    }

    #[test]
    fn a_field_left_from_another_target_keeps_the_frames_centre_as_fallback() {
        let quiet = |_: String| {};
        // The frames' own object, written differently: no fallback.
        let (p, f) = field_centres(None, Some("m45"), Some("M 45"), &resolve, &quiet).unwrap();
        assert_eq!((p.ra, f.is_none()), (56.87, true));
        // No object chosen: the frames' OBJECT, no fallback.
        let (p, f) = field_centres(None, None, Some("M 45"), &resolve, &quiet).unwrap();
        assert_eq!((p.ra, f.is_none()), (56.87, true));
        // An object left from M 33: searched first, the frames' M 45 after.
        let (p, f) = field_centres(None, Some("M 33"), Some("M 45"), &resolve, &quiet).unwrap();
        assert_eq!((p.ra, f.map(|f| f.ra)), (23.46, Some(56.87)));
        // Coordinates entered for another field: the same.
        let (p, f) = field_centres(Some((10., 41.)), None, Some("M 45"), &resolve, &quiet).unwrap();
        assert_eq!((p.ra, f.map(|f| f.ra)), (10., Some(56.87)));
        // Coordinates within the frames' field: no second search.
        let (_, f) =
            field_centres(Some((56.9, 24.1)), None, Some("M 45"), &resolve, &quiet).unwrap();
        assert!(f.is_none());
        // A chosen name nothing knows: the frames' OBJECT at once, said.
        let said = std::sync::Mutex::new(vec![]);
        let say = |line: String| said.lock().unwrap().push(line);
        let (p, f) = field_centres(None, Some("Olyhen"), Some("M 45"), &resolve, &say).unwrap();
        assert_eq!((p.ra, f.is_none()), (56.87, true));
        assert!(said.lock().unwrap()[0].contains("the frames' OBJECT M 45"));
        // Neither resolves: the run is told why.
        assert!(field_centres(None, Some("Olyhen"), Some("Phi"), &resolve, &quiet).is_err());
        assert!(field_centres(None, None, None, &resolve, &quiet).is_err());
    }

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
