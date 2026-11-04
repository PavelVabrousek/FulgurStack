//! Gaia DR3 from the public archives, cached on disk: the ESA Gaia archive
//! first, the GAVO data centre (Heidelberg) as fallback. Queries are cones
//! with a magnitude limit and at most 1.75 degrees radius, which both
//! services answer synchronously; wider fields are tiled.
use fulgur_pipeline::sky::catalogue::{self, GaiaStar};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Gaia DR3 TAP services: name, endpoint, table.
pub const SERVICES: [(&str, &str, &str); 2] = [
    (
        "ESA Gaia archive",
        "https://gea.esac.esa.int/tap-server/tap/sync",
        "gaiadr3.gaia_source",
    ),
    (
        "GAVO data centre",
        "https://dc.zah.uni-heidelberg.de/tap/sync",
        "gaia.dr3lite",
    ),
];
const TILE_RADIUS: f64 = 1.75;
const TILE_STEP: f64 = 2.4;

pub(crate) fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(300))
        .user_agent("FulgurStack (sky registration catalogue)")
        .build()
}

/// A cached or downloaded cone and where it came from.
pub struct Answer {
    pub stars: Vec<GaiaStar>,
    /// "cache" or the service name, per tile.
    pub sources: Vec<String>,
    /// Failed services, in the order tried.
    pub failures: Vec<String>,
}

fn parse(text: &str) -> Result<Vec<GaiaStar>, String> {
    let mut lines = text.lines();
    let header: Vec<_> = lines
        .next()
        .ok_or("empty Gaia answer")?
        .split(',')
        .map(|h| h.trim().trim_matches('"').to_ascii_lowercase())
        .collect();
    let col = |name: &str| {
        header
            .iter()
            .position(|h| h == name)
            .ok_or(format!("Gaia answer lacks column {name}"))
    };
    let (id, ra, dec, g, pa, pd) = (
        col("source_id")?,
        col("ra")?,
        col("dec")?,
        col("phot_g_mean_mag")?,
        col("pmra")?,
        col("pmdec")?,
    );
    let mut stars = vec![];
    for line in lines.filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = line.split(',').collect();
        let num = |i: usize| f.get(i).and_then(|v| v.trim().parse::<f64>().ok());
        let (Some(r), Some(d), Some(m)) = (num(ra), num(dec), num(g)) else {
            continue;
        };
        stars.push(GaiaStar {
            source_id: f.get(id).and_then(|v| v.trim().parse().ok()).unwrap_or(0),
            ra: r,
            dec: d,
            pmra: num(pa).unwrap_or(0.),
            pmdec: num(pd).unwrap_or(0.),
            g: m,
        });
    }
    Ok(stars)
}

fn cache_file(dir: &Path, ra: f64, dec: f64, radius: f64, g_max: f64) -> PathBuf {
    dir.join(format!(
        "gaia_dr3_{ra:.4}_{dec:+.4}_{radius:.3}_g{g_max:.1}.csv"
    ))
}

/// One cone of at most `TILE_RADIUS`: from the cache, else from the services
/// in order. `fetch` performs a query (service URL, ADQL) and returns CSV;
/// tests pass their own.
fn tile(
    dir: &Path,
    ra: f64,
    dec: f64,
    radius: f64,
    g_max: f64,
    fetch: &dyn Fn(&str, &str) -> Result<String, String>,
    failures: &mut Vec<String>,
) -> Result<(Vec<GaiaStar>, String), String> {
    let file = cache_file(dir, ra, dec, radius, g_max);
    if let Ok(text) = std::fs::read_to_string(&file) {
        return Ok((parse(&text)?, "cache".into()));
    }
    for (service, url, table) in SERVICES {
        let query = format!(
            "SELECT source_id, ra, dec, phot_g_mean_mag, pmra, pmdec FROM {table} WHERE 1=CONTAINS(POINT('ICRS', ra, dec), CIRCLE('ICRS', {ra:.6}, {dec:.6}, {radius:.6})) AND phot_g_mean_mag < {g_max}"
        );
        match fetch(url, &query) {
            // A TAP error comes back as a VOTable, not as CSV.
            Ok(text) if text.trim_start().starts_with("source_id") => {
                let stars = parse(&text)?;
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                let tmp = file.with_extension("csv.tmp");
                std::fs::write(&tmp, &text).map_err(|e| e.to_string())?;
                std::fs::rename(&tmp, &file).map_err(|e| e.to_string())?;
                return Ok((stars, service.into()));
            }
            Ok(text) => failures.push(format!(
                "{service}: {}",
                text.chars().take(160).collect::<String>()
            )),
            Err(e) => failures.push(format!("{service}: {e}")),
        }
    }
    Err(format!(
        "no Gaia service answered for RA {ra:.3} Dec {dec:+.3}: {}",
        failures.join(" | ")
    ))
}

/// Default query over HTTPS.
pub fn http_fetch(url: &str, query: &str) -> Result<String, String> {
    agent()
        .post(url)
        .send_form(&[
            ("REQUEST", "doQuery"),
            ("LANG", "ADQL"),
            ("FORMAT", "csv"),
            ("MAXREC", "500000"),
            ("QUERY", query),
        ])
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())
}

/// Stars brighter than `g_max` within `radius` degrees, brightest first,
/// tiled into cones the services answer synchronously. Duplicates from
/// overlapping tiles are removed by source id.
pub fn cone(
    dir: &Path,
    ra: f64,
    dec: f64,
    radius: f64,
    g_max: f64,
    fetch: &dyn Fn(&str, &str) -> Result<String, String>,
) -> Result<Answer, String> {
    let mut failures = vec![];
    let mut sources = vec![];
    let mut stars = vec![];
    if radius <= TILE_RADIUS {
        let (s, src) = tile(dir, ra, dec, radius, g_max, fetch, &mut failures)?;
        stars = s;
        sources.push(src);
    } else {
        let reach = ((radius - TILE_RADIUS) / TILE_STEP).ceil() as i32 + 1;
        let mut seen = std::collections::HashSet::new();
        for i in -reach..=reach {
            for j in -reach..=reach {
                let (dx, dy) = (i as f64 * TILE_STEP, j as f64 * TILE_STEP);
                if dx.hypot(dy) > radius + TILE_RADIUS * 0.5 {
                    continue;
                }
                let (tra, tdec) = catalogue::deproject([dx * 3600., dy * 3600.], ra, dec);
                let (s, src) = tile(
                    dir,
                    (tra * 1e4).round() / 1e4,
                    (tdec * 1e4).round() / 1e4,
                    TILE_RADIUS,
                    g_max,
                    fetch,
                    &mut failures,
                )?;
                sources.push(src);
                stars.extend(s.into_iter().filter(|s| {
                    catalogue::separation(ra, dec, s.ra, s.dec) <= radius
                        && (s.source_id == 0 || seen.insert(s.source_id))
                }));
            }
        }
    }
    stars.sort_by(|a, b| a.g.total_cmp(&b.g));
    Ok(Answer {
        stars,
        sources,
        failures,
    })
}
