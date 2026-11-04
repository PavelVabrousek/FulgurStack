//! Built-in deep-sky table (Messier, NGC, IC and aliases from OpenNGC,
//! CC BY-SA 4.0; see `data/README.md`).
use std::collections::HashMap;
use std::sync::OnceLock;

const TABLE: &str = include_str!("../data/deep_sky.csv");

#[derive(Clone, Debug, PartialEq)]
pub struct DeepSky {
    /// Catalogue name, e.g. "NGC 598".
    pub name: String,
    pub ra: f64,
    pub dec: f64,
}

/// Comparable form of a name: letters and digits only, upper case, leading
/// zeros of numbers dropped, catalogue words shortened ("Messier 33",
/// "M033" and "M 33" all become "M33").
pub fn normalize(name: &str) -> String {
    let upper = name.to_uppercase();
    let words = [
        ("MESSIER", "M"),
        ("CALDWELL", "C"),
        ("SHARPLESS", "SH2"),
        ("BARNARD", "B"),
    ];
    let mut text = upper.clone();
    for (long, short) in words {
        if let Some(rest) = text.strip_prefix(long) {
            text = format!("{short}{rest}");
        }
    }
    let compact: String = text.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    // Sh2-xxx keeps its "2" as part of the catalogue name.
    let (prefix, body) = match compact.strip_prefix("SH2") {
        Some(rest) => ("SH2", rest.to_string()),
        None => ("", compact),
    };
    let mut out = String::from(prefix);
    let mut previous_letter = true;
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '0' && previous_letter && chars.peek().is_some_and(|n| n.is_ascii_digit()) {
            continue;
        }
        previous_letter = c.is_ascii_alphabetic();
        out.push(c);
    }
    out
}

fn table() -> &'static HashMap<String, DeepSky> {
    static MAP: OnceLock<HashMap<String, DeepSky>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut map = HashMap::new();
        for line in TABLE.lines().filter(|l| !l.starts_with('#')) {
            let f: Vec<&str> = line.split(';').collect();
            if f.len() < 4 {
                continue;
            }
            let (Ok(ra), Ok(dec)) = (f[1].parse::<f64>(), f[2].parse::<f64>()) else {
                continue;
            };
            let object = DeepSky {
                name: f[0].to_string(),
                ra,
                dec,
            };
            // The catalogue name wins over an alias of another object.
            map.insert(normalize(f[0]), object.clone());
            for alias in f[3].split('|').filter(|a| !a.is_empty()) {
                map.entry(normalize(alias))
                    .or_insert_with(|| object.clone());
            }
        }
        map
    })
}

/// The object a name denotes, if the table knows it.
pub fn find(name: &str) -> Option<DeepSky> {
    table().get(&normalize(name)).cloned()
}

/// Number of names (catalogue names and aliases) the table answers.
pub fn names() -> usize {
    table().len()
}
