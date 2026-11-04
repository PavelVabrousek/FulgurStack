//! Online name lookup: NED (NASA/IPAC), then SIMBAD through its CfA mirror,
//! then CDS Sesame. Each failure is kept for the report.

/// Name, URL template ("{}" is the encoded name) and parser per service.
type Parser = fn(&str) -> Option<(f64, f64)>;

pub const SERVICES: [(&str, &str); 3] = [
    (
        "NED",
        "https://ned.ipac.caltech.edu/srs/ObjectLookup?name={}",
    ),
    (
        "SIMBAD (CfA mirror)",
        "https://simbad.cfa.harvard.edu/simbad/sim-id?Ident={}&output.format=ASCII",
    ),
    (
        "CDS Sesame",
        "https://cds.unistra.fr/cgi-bin/nph-sesame/-oI/A?{}",
    ),
];
const PARSERS: [Parser; 3] = [parse_ned, parse_simbad, parse_sesame];

pub fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// NED's JSON: `Preferred.Position.RA/Dec` in degrees.
pub fn parse_ned(text: &str) -> Option<(f64, f64)> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let p = &v["Preferred"]["Position"];
    Some((p["RA"].as_f64()?, p["Dec"].as_f64()?))
}

fn sexagesimal(parts: &[&str], hours: bool) -> Option<f64> {
    let sign = if parts.first()?.starts_with('-') {
        -1.
    } else {
        1.
    };
    let n: Vec<f64> = parts
        .iter()
        .map(|p| p.trim_start_matches(['+', '-']).parse::<f64>())
        .collect::<Result<_, _>>()
        .ok()?;
    let v = n.first()? + n.get(1).unwrap_or(&0.) / 60. + n.get(2).unwrap_or(&0.) / 3600.;
    Some(sign * v * if hours { 15. } else { 1. })
}

/// SIMBAD's ASCII page: the ICRS J2000 line in sexagesimal hours/degrees.
pub fn parse_simbad(text: &str) -> Option<(f64, f64)> {
    let line = text
        .lines()
        .find(|l| l.starts_with("Coordinates(ICRS,ep=J2000,eq=2000):"))?;
    let values: Vec<&str> = line.split(':').nth(1)?.split_whitespace().collect();
    let dec_at = values.iter().position(|v| v.starts_with(['+', '-']))?;
    Some((
        sexagesimal(&values[..dec_at], true)?,
        // Up to three numbers; a quality note such as "(Optical)" may follow.
        sexagesimal(
            &values[dec_at..]
                .iter()
                .take(3)
                .take_while(|v| v.trim_start_matches(['+', '-']).parse::<f64>().is_ok())
                .copied()
                .collect::<Vec<_>>(),
            false,
        )?,
    ))
}

/// Sesame: the "%J ra dec" line in degrees.
pub fn parse_sesame(text: &str) -> Option<(f64, f64)> {
    text.lines().find_map(|l| {
        let mut it = l
            .strip_prefix("%J ")?
            .split_whitespace()
            .map(str::parse::<f64>);
        Some((it.next()?.ok()?, it.next()?.ok()?))
    })
}

/// Default request over HTTPS.
pub fn http_get(url: &str) -> Result<String, String> {
    crate::download::agent()
        .get(url)
        .timeout(std::time::Duration::from_secs(20))
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())
}

/// Position of `name` from the first service that knows it: (RA, Dec,
/// service), or None; `failures` lists every service that did not answer.
pub fn find(
    name: &str,
    get: &dyn Fn(&str) -> Result<String, String>,
    failures: &mut Vec<String>,
) -> Option<(f64, f64, &'static str)> {
    for ((service, template), parse) in SERVICES.iter().zip(PARSERS) {
        match get(&template.replace("{}", &encode(name))) {
            Ok(text) => match parse(&text) {
                Some((ra, dec)) => return Some((ra, dec, service)),
                None => failures.push(format!("{service}: no position for '{name}'")),
            },
            Err(e) => failures.push(format!("{service}: {e}")),
        }
    }
    None
}
