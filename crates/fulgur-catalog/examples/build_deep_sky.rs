//! Builds `data/deep_sky.csv` from the OpenNGC database files (NGC.csv and
//! addendum.csv, CC BY-SA 4.0) and complete catalogues exported from SIMBAD
//! (LBN, Sharpless Sh2, Barnard; see `data/README.md`).
//! Usage: build_deep_sky NGC.csv addendum.csv OUT [SIMBAD.csv ...]
//!
//! Each output line: NAME;RA;DEC;ALIASES with positions in degrees and
//! aliases separated by '|': the Messier number, common names and the
//! identifiers of the catalogues an observer names targets by.
use std::collections::BTreeMap;

const KEPT: [&str; 10] = [
    "C ", "LBN ", "LDN ", "Sh2-", "B ", "Mel ", "Cr ", "vdB ", "Ced ", "Abell ",
];

fn sexagesimal(text: &str, hours: bool) -> Option<f64> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let sign = if t.starts_with('-') { -1. } else { 1. };
    let parts: Vec<f64> = t
        .trim_start_matches(['+', '-'])
        .split(':')
        .map(|p| p.parse::<f64>())
        .collect::<Result<_, _>>()
        .ok()?;
    let v =
        parts.first()? + parts.get(1).unwrap_or(&0.) / 60. + parts.get(2).unwrap_or(&0.) / 3600.;
    Some(sign * v * if hours { 15. } else { 1. })
}

/// "NGC0224" -> "NGC 224", "M031" -> "M 31", "IC0001" -> "IC 1".
fn display(name: &str) -> String {
    let split = name
        .find(|c: char| c.is_ascii_digit())
        .unwrap_or(name.len());
    let (prefix, number) = name.split_at(split);
    let trimmed = number.trim_start_matches('0');
    if number.is_empty() {
        return name.to_string();
    }
    format!(
        "{prefix} {}",
        if trimmed.is_empty() { "0" } else { trimmed }
    )
}

/// Name, position (if any), aliases, and the object a duplicate repeats.
type Row = (String, Option<(f64, f64)>, Vec<String>, String);

/// SIMBAD's padded identifiers in this table's spelling: "LBN     1" ->
/// "LBN 1", "SH  2-10" -> "Sh2-10", "Barnard   1" -> "B 1". Only plain
/// catalogue numbers are kept (LBN also has coordinate names).
fn simbad_name(id: &str) -> Option<(String, Option<String>)> {
    let words: Vec<&str> = id.split_whitespace().collect();
    match words.as_slice() {
        ["LBN", n] if n.chars().all(|c| c.is_ascii_digit()) => Some((format!("LBN {n}"), None)),
        ["SH", rest] if rest.starts_with("2-") => {
            let n = &rest[2..];
            n.chars()
                .all(|c| c.is_ascii_alphanumeric())
                .then(|| (format!("Sh2-{n}"), Some(format!("Sharpless {n}"))))
        }
        ["Barnard", n] if n.chars().all(|c| c.is_ascii_alphanumeric()) => {
            Some((format!("B {n}"), Some(format!("Barnard {n}"))))
        }
        _ => None,
    }
}

/// A SIMBAD main identifier worth keeping as alias: common names ("NAME
/// Horsehead Nebula") and the catalogues of this table.
fn simbad_alias(main: &str) -> Option<String> {
    let words = main.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some(name) = words.strip_prefix("NAME ") {
        return Some(name.to_string());
    }
    if let Some((name, _)) = simbad_name(&words) {
        return Some(name);
    }
    ["M ", "NGC ", "IC "]
        .iter()
        .any(|p| words.starts_with(p))
        .then_some(words)
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut rows: Vec<Row> = vec![];
    for file in &a[..2] {
        let text = std::fs::read_to_string(file).unwrap();
        let mut lines = text.lines();
        let head: Vec<&str> = lines.next().unwrap().split(';').collect();
        let col = |n: &str| head.iter().position(|h| *h == n).unwrap();
        let (cn, ct, cr, cd, cm, cngc, cic, cid, ccom) = (
            col("Name"),
            col("Type"),
            col("RA"),
            col("Dec"),
            col("M"),
            col("NGC"),
            col("IC"),
            col("Identifiers"),
            col("Common names"),
        );
        for line in lines {
            let f: Vec<&str> = line.split(';').collect();
            if f.len() <= ccom || f[cn].is_empty() {
                continue;
            }
            let pos = sexagesimal(f[cr], true).zip(sexagesimal(f[cd], false));
            let mut aliases = vec![];
            if !f[cm].trim().is_empty() {
                aliases.push(format!("M {}", f[cm].trim().trim_start_matches('0')));
            }
            for c in f[ccom].split(',').map(str::trim).filter(|c| !c.is_empty()) {
                aliases.push(c.to_string());
            }
            for id in f[cid].split(',').map(str::trim) {
                if KEPT.iter().any(|k| id.starts_with(k)) {
                    aliases.push(id.to_string());
                }
            }
            // A duplicate entry points at the object it repeats.
            let original = if f[ct] == "Dup" {
                [(cngc, "NGC"), (cic, "IC")]
                    .iter()
                    .find_map(|&(c, p)| {
                        let n = f[c].trim();
                        (!n.is_empty()).then(|| format!("{p}{n:0>4}"))
                    })
                    .unwrap_or_default()
            } else {
                String::new()
            };
            rows.push((f[cn].to_string(), pos, aliases, original));
        }
    }
    let positions: BTreeMap<String, (f64, f64)> = rows
        .iter()
        .filter_map(|r| r.1.map(|p| (r.0.clone(), p)))
        .collect();
    let mut out = String::from(
        "# FulgurStack deep-sky table built from OpenNGC (https://github.com/mattiaverga/OpenNGC),\n# CC BY-SA 4.0, and LBN, Sh2 and Barnard positions from SIMBAD (CDS, Strasbourg).\n# NAME;RA_deg;DEC_deg;ALIASES (|-separated)\n",
    );
    let mut written = 0;
    for (name, pos, aliases, original) in &rows {
        let Some((ra, dec)) = pos.or_else(|| positions.get(original).copied()) else {
            continue;
        };
        out.push_str(&format!(
            "{};{ra:.5};{dec:+.5};{}\n",
            display(name),
            aliases.join("|")
        ));
        written += 1;
    }
    // Complete LBN, Sh2 and Barnard catalogues; a name OpenNGC already has,
    // as an object or an alias, keeps OpenNGC's entry.
    use fulgur_catalog::dso::normalize;
    let mut known: std::collections::HashSet<String> = rows
        .iter()
        .flat_map(|r| std::iter::once(display(&r.0)).chain(r.2.iter().cloned()))
        .map(|n| normalize(&n))
        .collect();
    for file in &a[3..] {
        let text = std::fs::read_to_string(file).unwrap();
        let mut lines = text.lines();
        lines.next();
        let mut added = 0;
        let mut by_name: BTreeMap<String, (f64, f64, Vec<String>)> = BTreeMap::new();
        for line in lines {
            // id, ra, dec, main_id; ids and names are quoted.
            let f: Vec<&str> = line
                .split(',')
                .map(|v| v.trim().trim_matches('"'))
                .collect();
            if f.len() < 4 {
                continue;
            }
            let (Some((name, extra)), Ok(ra), Ok(dec)) =
                (simbad_name(f[0]), f[1].parse::<f64>(), f[2].parse::<f64>())
            else {
                continue;
            };
            let mut aliases: Vec<String> = extra.into_iter().collect();
            if let Some(alias) = simbad_alias(f[3]).filter(|a| *a != name) {
                aliases.push(alias);
            }
            by_name.entry(name).or_insert((ra, dec, aliases));
        }
        for (name, (ra, dec, aliases)) in by_name {
            if !known.insert(normalize(&name)) {
                continue;
            }
            out.push_str(&format!("{name};{ra:.5};{dec:+.5};{}\n", aliases.join("|")));
            added += 1;
        }
        println!("{added} objects added from {file}");
        written += added;
    }
    std::fs::write(&a[2], out).unwrap();
    println!("{written} objects written to {}", a[2]);
}
