//! A target the built-in deep-sky table recognises in the names around a
//! stack: the object name in a header, the file name, the folder name (the
//! owner's request of 2026-11-09, ADR-167). A recognised name is only a
//! suggestion; the owner confirms it or enters the target.
use crate::dso::{self, DeepSky};

/// The target found and where it was written.
#[derive(Clone, Debug, PartialEq)]
pub struct TargetGuess {
    /// The name as the text writes it ("M27", "NGC 7000").
    pub matched: String,
    /// The catalogue's own name for the object ("NGC 6853").
    pub catalogue: String,
    /// Where it was found, for the owner ("folder name 'M27_Stellina'").
    pub from: String,
}

/// The names of the table found in one text. Names of one to three
/// consecutive words are tried ("M", "27" read as "M 27"), longest first.
fn objects_in(text: &str) -> Vec<(String, DeepSky)> {
    let tokens: Vec<&str> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();
    let mut found: Vec<(String, DeepSky)> = vec![];
    let mut at = 0;
    while at < tokens.len() {
        let hit = (1..=3.min(tokens.len() - at)).rev().find_map(|length| {
            let words = &tokens[at..at + length];
            // A bare number or a single letter is no name.
            let joined = words.concat();
            (joined.chars().any(|c| c.is_alphabetic()) && joined.chars().count() >= 2)
                .then(|| dso::find(&joined).map(|object| (words.join(" "), object, length)))
                .flatten()
        });
        match hit {
            Some((matched, object, length)) => {
                found.push((matched, object));
                at += length;
            }
            None => at += 1,
        }
    }
    found
}

/// The one object a list of texts names: every text that names an object
/// must name the same one. Two different objects are no answer.
fn unique(texts: &[String]) -> Option<(String, DeepSky, String)> {
    let mut answer: Option<(String, DeepSky, String)> = None;
    for text in texts {
        let found = objects_in(text);
        let mut names: Vec<&str> = found.iter().map(|(_, o)| o.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        if names.len() > 1 {
            return None;
        }
        if let Some((matched, object)) = found.into_iter().next() {
            match &answer {
                None => answer = Some((matched, object, text.clone())),
                Some((_, first, _)) if first.name != object.name => return None,
                _ => {}
            }
        }
    }
    answer
}

/// The target the texts name, by precedence: the object names in the
/// headers, the file names, the folder names. A source that names two
/// different objects is skipped.
pub fn guess_target(
    header_objects: &[String],
    file_names: &[String],
    folder_names: &[String],
) -> Option<TargetGuess> {
    let sources: [(&str, &[String]); 3] = [
        ("header object name", header_objects),
        ("file name", file_names),
        ("folder name", folder_names),
    ];
    sources.iter().find_map(|(label, texts)| {
        unique(texts).map(|(matched, object, text)| TargetGuess {
            matched,
            catalogue: object.name,
            from: format!("{label} '{text}'"),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn names_are_found_in_headers_file_names_and_folders() {
        let none = texts(&[]);
        let guess = guess_target(&none, &none, &texts(&["M27_Stellina_PsionDataForTest"])).unwrap();
        assert_eq!(guess.matched, "M27");
        assert!(guess.from.starts_with("folder name"));
        let spaced = guess_target(
            &none,
            &texts(&["Light_NGC 7000_300.0s_Bin1_0001.fit"]),
            &none,
        )
        .unwrap();
        assert_eq!(spaced.matched, "NGC 7000");
        assert!(spaced.from.starts_with("file name"));
        let header = guess_target(&texts(&["Messier 33 galaxy"]), &none, &none).unwrap();
        assert_eq!(header.matched, "Messier 33");
        assert!(header.from.starts_with("header object name"));
    }

    #[test]
    fn the_header_comes_before_the_file_name_and_the_file_name_before_the_folder() {
        let guess = guess_target(
            &texts(&["M 31"]),
            &texts(&["M27_0001.fit"]),
            &texts(&["M33 night"]),
        )
        .unwrap();
        assert_eq!(guess.matched, "M 31");
        let guess = guess_target(&[], &texts(&["M27_0001.fit"]), &texts(&["M33 night"])).unwrap();
        assert_eq!(guess.matched, "M27");
    }

    #[test]
    fn ambiguous_or_nameless_texts_give_nothing() {
        let none: Vec<String> = vec![];
        // Two objects in one folder name, and a second source decides.
        assert!(guess_target(&none, &none, &texts(&["M31 and M33"])).is_none());
        let guess =
            guess_target(&none, &texts(&["M27_001.fit"]), &texts(&["M31 and M33"])).unwrap();
        assert_eq!(guess.matched, "M27");
        // Different files naming different objects.
        assert!(guess_target(&none, &texts(&["M27_001.fit", "M31_002.fit"]), &none).is_none());
        // Ordinary file names carry no object.
        assert!(
            guess_target(
                &none,
                &texts(&["img-0001.fits", "Light_300.0s_Bin1_gain100_0001.fit"]),
                &texts(&["2026", "11 November", "Lights"])
            )
            .is_none()
        );
    }

    #[test]
    fn the_same_object_under_two_names_is_one_answer() {
        let none: Vec<String> = vec![];
        let guess = guess_target(&none, &texts(&["M27_NGC6853_001.fit"]), &none).unwrap();
        assert_eq!(guess.catalogue, dso::find("M27").unwrap().name);
    }

    #[test]
    fn real_capture_names_give_the_target_or_nothing() {
        let none: Vec<String> = vec![];
        let one = |name: &str| guess_target(&none, &texts(&[name]), &none).map(|g| g.matched);
        assert_eq!(
            one(
                "c_Light_M 42_270deg_30.0s_Bin1_2600MC_gain100_20260119-200800_-20.0C_SQA_0001.fit"
            ),
            Some("M 42".to_owned())
        );
        assert_eq!(
            one("Light_M 33_10.0s_IRCUT_20261006-220000.fit"),
            Some("M 33".to_owned())
        );
        for name in [
            "Light_Phi Tauri_300.0s_Bin1_20261106-014213_1deg_0001.fit",
            "img-0001.fits",
            "Light_Dark_Flat_Bias_gain100_0001.fit",
        ] {
            assert_eq!(one(name), None, "{name}");
        }
        let folder = |name: &str| guess_target(&none, &none, &texts(&[name])).map(|g| g.matched);
        assert_eq!(folder("10261105 Phi Tauri ASI2600MM L"), None);
        assert_eq!(folder("20261107 Phi Tauri ASI2600MC nofilter"), None);
        assert_eq!(
            folder("M27_Stellina_PsionDataForTestFUlgurStack"),
            Some("M27".to_owned())
        );
    }
}
