//! Public catalogue access: CDS Sesame for an object's position and the ESA
//! Gaia archive (TAP, Gaia DR3) for stars. Answers are cached as CSV files in
//! the output folder so a rerun does not query again.
use serde::Serialize;
use std::path::Path;
use std::time::Duration;

const GAIA_TAP: &str = "https://gea.esac.esa.int/tap-server/tap/sync";
/// CDS Sesame and its CfA mirror.
const SESAME: [&str; 2] = [
    "https://cds.unistra.fr/cgi-bin/nph-sesame/-oI/A?",
    "https://vizier.cfa.harvard.edu/viz-bin/nph-sesame/-oI/A?",
];
/// Reference epoch of Gaia DR3 positions.
pub const GAIA_EPOCH: f64 = 2016.0;

#[derive(Clone, Debug, Serialize)]
pub struct CatStar {
    pub source_id: u64,
    /// ICRS position at the frame's epoch (proper motion applied).
    pub ra_deg: f64,
    pub dec_deg: f64,
    pub g_mag: f64,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(180))
        .user_agent("synthref/0.1 (FulgurStack registration experiment)")
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

/// Position of a named object from CDS Sesame (J2000 degrees).
pub fn resolve(object: &str) -> Result<(f64, f64), String> {
    let mut failures = vec![];
    let body = SESAME
        .iter()
        .find_map(|base| {
            match agent()
                .get(&format!("{base}{}", encode(object)))
                .call()
                .map_err(|e| e.to_string())
                .and_then(|r| r.into_string().map_err(|e| e.to_string()))
            {
                Ok(body) => Some(body),
                Err(e) => {
                    failures.push(e);
                    None
                }
            }
        })
        .ok_or_else(|| {
            format!(
                "Sesame query failed ({}); give --ra/--dec instead",
                failures.join("; ")
            )
        })?;
    for line in body.lines() {
        if let Some(rest) = line.strip_prefix("%J ") {
            let mut it = rest.split_whitespace().map(str::parse::<f64>);
            if let (Some(Ok(ra)), Some(Ok(dec))) = (it.next(), it.next()) {
                return Ok((ra, dec));
            }
        }
    }
    Err(format!("Sesame does not know '{object}'"))
}

/// The brightest `top` Gaia DR3 stars within `radius_deg` of a position,
/// brightest first, moved by proper motion to `epoch`.
pub fn cone(
    ra: f64,
    dec: f64,
    radius_deg: f64,
    top: usize,
    epoch: f64,
    cache: &Path,
) -> Result<Vec<CatStar>, String> {
    let file = cache.join(format!(
        "gaia_dr3_{ra:.4}_{dec:+.4}_{radius_deg:.4}_{top}.csv"
    ));
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(_) => {
            let query = format!(
                "SELECT TOP {top} source_id, ra, dec, phot_g_mean_mag, pmra, pmdec \
                 FROM gaiadr3.gaia_source \
                 WHERE 1=CONTAINS(POINT('ICRS', ra, dec), CIRCLE('ICRS', {ra:.6}, {dec:.6}, {radius_deg:.6})) \
                 AND phot_g_mean_mag IS NOT NULL ORDER BY phot_g_mean_mag ASC"
            );
            eprintln!("  Gaia DR3 query: radius {radius_deg:.3} deg, brightest {top}");
            let text = agent()
                .post(GAIA_TAP)
                .send_form(&[
                    ("REQUEST", "doQuery"),
                    ("LANG", "ADQL"),
                    ("FORMAT", "csv"),
                    ("QUERY", &query),
                ])
                .map_err(|e| format!("Gaia archive query failed: {e}"))?
                .into_string()
                .map_err(|e| e.to_string())?;
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
        header.iter().position(|h| h == name).ok_or(format!(
            "Gaia answer lacks column {name}: {}",
            header.join(",")
        ))
    };
    let (id, c_ra, c_dec, c_g, c_pa, c_pd) = (
        col("source_id")?,
        col("ra")?,
        col("dec")?,
        col("phot_g_mean_mag")?,
        col("pmra")?,
        col("pmdec")?,
    );
    let dt = epoch - GAIA_EPOCH;
    let mut stars = Vec::new();
    for line in lines.filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = line.split(',').collect();
        let num = |i: usize| f.get(i).and_then(|v| v.trim().parse::<f64>().ok());
        let (Some(ra0), Some(dec0), Some(g)) = (num(c_ra), num(c_dec), num(c_g)) else {
            continue;
        };
        let (pa, pd) = (num(c_pa).unwrap_or(0.), num(c_pd).unwrap_or(0.));
        // pmra is mu_alpha* = mu_alpha cos(dec), in mas/yr.
        let dec = dec0 + pd * dt / 3.6e6;
        let ra = ra0 + pa * dt / 3.6e6 / dec0.to_radians().cos();
        stars.push(CatStar {
            source_id: f.get(id).and_then(|v| v.trim().parse().ok()).unwrap_or(0),
            ra_deg: ra,
            dec_deg: dec,
            g_mag: g,
        });
    }
    stars.sort_by(|a, b| a.g_mag.total_cmp(&b.g_mag));
    Ok(stars)
}

/// Observation epoch (decimal year) from DATE-OBS of a FITS header. Only this
/// card is read; site coordinates are never touched.
pub fn epoch_of(path: &Path) -> Option<f64> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut block = [0u8; 2880];
    for _ in 0..64 {
        file.read_exact(&mut block).ok()?;
        for card in block.chunks(80) {
            let key = std::str::from_utf8(&card[..8]).ok()?.trim();
            if key == "END" {
                return None;
            }
            if key == "DATE-OBS" {
                let value = std::str::from_utf8(&card[10..]).ok()?;
                let value = value.split('\'').nth(1)?;
                let year: f64 = value.get(0..4)?.parse().ok()?;
                let month: usize = value.get(5..7)?.parse().ok()?;
                let day: f64 = value.get(8..10)?.parse().ok()?;
                const BEFORE: [f64; 12] = [
                    0., 31., 59., 90., 120., 151., 181., 212., 243., 273., 304., 334.,
                ];
                return Some(year + (BEFORE[month.clamp(1, 12) - 1] + day - 0.5) / 365.25);
            }
        }
    }
    None
}

/// Gnomonic (TAN) projection about (ra0, dec0): standard coordinates in
/// arcseconds, xi towards east, eta towards north.
pub fn project(ra: f64, dec: f64, ra0: f64, dec0: f64) -> Option<[f64; 2]> {
    let (a, d, a0, d0) = (
        ra.to_radians(),
        dec.to_radians(),
        ra0.to_radians(),
        dec0.to_radians(),
    );
    let cos_c = d0.sin() * d.sin() + d0.cos() * d.cos() * (a - a0).cos();
    if cos_c <= 0.1 {
        return None;
    }
    let xi = d.cos() * (a - a0).sin() / cos_c;
    let eta = (d0.cos() * d.sin() - d0.sin() * d.cos() * (a - a0).cos()) / cos_c;
    let k = 180. / std::f64::consts::PI * 3600.;
    Some([xi * k, eta * k])
}

/// Inverse of [`project`].
pub fn deproject(p: [f64; 2], ra0: f64, dec0: f64) -> (f64, f64) {
    let k = std::f64::consts::PI / 180. / 3600.;
    let (xi, eta) = (p[0] * k, p[1] * k);
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
