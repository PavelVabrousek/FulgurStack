//! Catalogue and target position sources for sky-based registration (plan
//! phase 2, ADR-132, ADR-134):
//! - [`gaia_field`]: Gaia DR3 stars from the PixInsight files on disk when
//!   they cover the request, otherwise from the cache, otherwise downloaded
//!   (ESA, then GAVO) into the cache;
//! - [`position`]: the target position from a manual centre, header RA/Dec,
//!   the built-in deep-sky table or an online lookup, in that order.
pub mod download;
pub mod dso;
pub mod guess;
pub mod lookup;
pub mod sky;
pub mod xpsd;

use fulgur_pipeline::sky::catalogue::GaiaStar;
use serde::Serialize;
use std::path::PathBuf;

/// Where Gaia stars may come from.
#[derive(Clone, Debug, Default)]
pub struct GaiaSources {
    /// Folder of the PixInsight Gaia DR3 XPSD files, if any.
    pub pixinsight: Option<PathBuf>,
    /// FulgurStack's own catalogue cache folder.
    pub cache: PathBuf,
    /// Whether an uncached field may be downloaded.
    pub download: bool,
}

/// Stars of a field and where they came from.
#[derive(Debug)]
pub struct GaiaField {
    pub stars: Vec<GaiaStar>,
    pub source: String,
    /// Sources that were tried and failed, in order.
    pub failures: Vec<String>,
}

/// Stars brighter than `g_max` within `radius` degrees, brightest first,
/// positions at the Gaia DR3 epoch with proper motions.
pub fn gaia_field(
    sources: &GaiaSources,
    ra: f64,
    dec: f64,
    radius: f64,
    g_max: f64,
) -> Result<GaiaField, String> {
    gaia_field_with(sources, ra, dec, radius, g_max, &download::http_fetch)
}

/// As [`gaia_field`] with a given archive query function (tests).
pub fn gaia_field_with(
    sources: &GaiaSources,
    ra: f64,
    dec: f64,
    radius: f64,
    g_max: f64,
    fetch: &dyn Fn(&str, &str) -> Result<String, String>,
) -> Result<GaiaField, String> {
    let mut failures = vec![];
    if let Some(dir) = &sources.pixinsight {
        match xpsd::XpsdCatalogue::open(dir) {
            Ok(cat) if cat.limit() >= g_max => {
                return Ok(GaiaField {
                    stars: cat.cone(ra, dec, radius, g_max)?,
                    source: format!("PixInsight Gaia DR3 files ({})", dir.display()),
                    failures,
                });
            }
            Ok(cat) => failures.push(format!(
                "PixInsight Gaia DR3 files reach G {:.2} only, G {g_max} requested",
                cat.limit()
            )),
            Err(e) => failures.push(format!("PixInsight Gaia DR3 files: {e}")),
        }
    }
    let offline = |_: &str, _: &str| Err("download not allowed".to_string());
    let answer = download::cone(
        &sources.cache,
        ra,
        dec,
        radius,
        g_max,
        if sources.download { fetch } else { &offline },
    )
    .map_err(|e| {
        let mut all = failures.clone();
        all.push(e);
        all.join(" | ")
    })?;
    failures.extend(answer.failures);
    let mut tiles = answer.sources.clone();
    tiles.sort();
    tiles.dedup();
    Ok(GaiaField {
        stars: answer.stars,
        source: format!("Gaia DR3 {}", tiles.join(" + ")),
        failures,
    })
}

/// An HTTP GET returning the body or an error message.
pub type Getter = dyn Fn(&str) -> Result<String, String>;

/// Where a target position came from.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum PositionSource {
    Manual,
    Header,
    /// Built-in deep-sky table, with the catalogue name found.
    DeepSky(String),
    /// Online service.
    Online(String),
}

#[derive(Clone, Debug, Serialize)]
pub struct Position {
    pub ra: f64,
    pub dec: f64,
    pub source: PositionSource,
    /// Sources tried before, and why they gave nothing.
    pub notes: Vec<String>,
}

/// The target position, by precedence: manual centre, header RA/Dec, the
/// built-in deep-sky table for `object`, then the online services (when
/// `online` is given). Err lists why every source failed.
pub fn position(
    manual: Option<(f64, f64)>,
    header: Option<(f64, f64)>,
    object: Option<&str>,
    online: Option<&Getter>,
) -> Result<Position, Vec<String>> {
    let valid = |p: &(f64, f64)| (0. ..360.).contains(&p.0) && (-90. ..=90.).contains(&p.1);
    let mut notes = vec![];
    match manual {
        Some(p) if valid(&p) => {
            return Ok(Position {
                ra: p.0,
                dec: p.1,
                source: PositionSource::Manual,
                notes,
            });
        }
        Some(p) => notes.push(format!("manual centre {p:?} is out of range")),
        None => notes.push("no manual centre".into()),
    }
    match header {
        Some(p) if valid(&p) => {
            return Ok(Position {
                ra: p.0,
                dec: p.1,
                source: PositionSource::Header,
                notes,
            });
        }
        Some(p) => notes.push(format!("header RA/Dec {p:?} is out of range")),
        None => notes.push("no RA/Dec in the headers".into()),
    }
    let Some(name) = object.map(str::trim).filter(|n| !n.is_empty()) else {
        notes.push("no object name".into());
        return Err(notes);
    };
    if let Some(d) = dso::find(name) {
        return Ok(Position {
            ra: d.ra,
            dec: d.dec,
            source: PositionSource::DeepSky(d.name),
            notes,
        });
    }
    notes.push(format!("'{name}' is not in the built-in deep-sky table"));
    let Some(get) = online else {
        notes.push("online lookup not allowed".into());
        return Err(notes);
    };
    match lookup::find(name, get, &mut notes) {
        Some((ra, dec, service)) => Ok(Position {
            ra,
            dec,
            source: PositionSource::Online(service.into()),
            notes,
        }),
        None => Err(notes),
    }
}

#[cfg(test)]
mod tests;
