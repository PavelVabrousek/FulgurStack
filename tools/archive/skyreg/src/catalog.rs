//! Gaia DR3 from the ESA archive (TAP) and object positions from CDS Sesame.
//! Answers are cached as CSV so reruns do not query again.
use std::path::Path;
use std::time::Duration;

/// Gaia DR3 TAP services and their tables, tried in turn: the ESA archive,
/// then the GAVO data centre (Heidelberg) with the same columns.
const GAIA_TAP: [(&str, &str, &str); 2] = [
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
const SESAME: [&str; 2] = [
    "https://cds.unistra.fr/cgi-bin/nph-sesame/-oI/A?",
    "https://vizier.cfa.harvard.edu/viz-bin/nph-sesame/-oI/A?",
];
pub const GAIA_EPOCH: f64 = 2016.0;
const ARCSEC: f64 = 180. / std::f64::consts::PI * 3600.;

#[derive(Clone, Debug)]
pub struct CatStar {
    pub source_id: u64,
    pub ra: f64,
    pub dec: f64,
    /// mu_alpha* (mas/yr, already multiplied by cos dec) and mu_delta.
    pub pmra: f64,
    pub pmdec: f64,
    pub g: f64,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(300))
        .user_agent("skyreg/0.1 (FulgurStack registration experiment)")
        .build()
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub fn resolve(object: &str) -> Result<(f64, f64), String> {
    let mut failures = vec![];
    for base in SESAME {
        let answer = agent()
            .get(&format!("{base}{}", encode(object)))
            .call()
            .map_err(|e| e.to_string())
            .and_then(|r| r.into_string().map_err(|e| e.to_string()));
        match answer {
            Ok(body) => {
                for line in body.lines() {
                    if let Some(rest) = line.strip_prefix("%J ") {
                        let mut it = rest.split_whitespace().map(str::parse::<f64>);
                        if let (Some(Ok(ra)), Some(Ok(dec))) = (it.next(), it.next()) {
                            return Ok((ra, dec));
                        }
                    }
                }
                failures.push(format!("{base}: no position"));
            }
            Err(e) => failures.push(e),
        }
    }
    Err(format!(
        "Sesame failed ({}); give --ra/--dec",
        failures.join("; ")
    ))
}

/// Gaia DR3 stars brighter than `g_max` within `radius` degrees. A magnitude
/// limit instead of an ordered TOP keeps each query fast on every service.
pub fn cone(
    ra: f64,
    dec: f64,
    radius: f64,
    g_max: f64,
    cache: &Path,
    log: &dyn Fn(String),
) -> Result<Vec<CatStar>, String> {
    let file = cache.join(format!(
        "gaia_dr3_{ra:.4}_{dec:+.4}_{radius:.3}_g{g_max:.1}.csv"
    ));
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => {
            log(format!(
                "catalogue: cached Gaia DR3 cone RA {ra:.3} Dec {dec:+.3} r={radius:.2} deg G<{g_max} ({})",
                file.display()
            ));
            text
        }
        Err(_) => {
            let mut failures = vec![];
            let mut answer = None;
            for (service, url, table) in GAIA_TAP {
                let query = format!(
                    "SELECT source_id, ra, dec, phot_g_mean_mag, pmra, pmdec FROM {table} WHERE 1=CONTAINS(POINT('ICRS', ra, dec), CIRCLE('ICRS', {ra:.6}, {dec:.6}, {radius:.6})) AND phot_g_mean_mag < {g_max}"
                );
                log(format!(
                    "catalogue: querying {service} ({table}), cone RA {ra:.3} Dec {dec:+.3} r={radius:.2} deg, G<{g_max}"
                ));
                let result = agent()
                    .post(url)
                    .send_form(&[
                        ("REQUEST", "doQuery"),
                        ("LANG", "ADQL"),
                        ("FORMAT", "csv"),
                        ("MAXREC", "500000"),
                        ("QUERY", &query),
                    ])
                    .map_err(|e| e.to_string())
                    .and_then(|r| r.into_string().map_err(|e| e.to_string()));
                match result {
                    // A TAP error comes back as a VOTable, not as CSV.
                    Ok(text) if text.trim_start().starts_with("source_id") => {
                        log(format!("catalogue: {service} answered"));
                        answer = Some(text);
                        break;
                    }
                    Ok(text) => failures.push(format!(
                        "{service}: {}",
                        text.chars().take(160).collect::<String>()
                    )),
                    Err(e) => failures.push(format!("{service}: {e}")),
                }
                log(format!("catalogue: {} failed", service));
            }
            let text = answer.ok_or(format!(
                "no Gaia service answered: {}",
                failures.join(" | ")
            ))?;
            let tmp = file.with_extension("csv.tmp");
            std::fs::write(&tmp, &text).map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, &file).map_err(|e| e.to_string())?;
            text
        }
    };
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
            .ok_or(format!("Gaia answer lacks {name}"))
    };
    let (id, c_ra, c_dec, c_g, c_pa, c_pd) = (
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
        let (Some(ra), Some(dec), Some(g)) = (num(c_ra), num(c_dec), num(c_g)) else {
            continue;
        };
        stars.push(CatStar {
            source_id: f.get(id).and_then(|v| v.trim().parse().ok()).unwrap_or(0),
            ra,
            dec,
            pmra: num(c_pa).unwrap_or(0.),
            pmdec: num(c_pd).unwrap_or(0.),
            g,
        });
    }
    Ok(stars)
}

pub fn deproject(p: [f64; 2], ra0: f64, dec0: f64) -> (f64, f64) {
    let (xi, eta) = (p[0] / ARCSEC, p[1] / ARCSEC);
    let (a0, d0) = (ra0.to_radians(), dec0.to_radians());
    let rho = xi.hypot(eta);
    if rho == 0. {
        return (ra0, dec0);
    }
    let c = rho.atan();
    let dec = (c.cos() * d0.sin() + eta * c.sin() * d0.cos() / rho).asin();
    let ra = a0 + (xi * c.sin()).atan2(rho * d0.cos() * c.cos() - eta * d0.sin() * c.sin());
    (ra.to_degrees().rem_euclid(360.), dec.to_degrees())
}
