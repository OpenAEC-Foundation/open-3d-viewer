//! The interface languages beyond Dutch and English: catalogues in `locales/`, keyed by the English
//! text that the call sites pass to `Language::text(nl, en)` or `Language::translate(en)`.
//!
//! A catalogue is a JSON file `{"locale": "de", "name": "Deutsch", "messages": {English: text}}`.
//! It is compiled into the app and read the first time its language is used. A text it lacks, or
//! leaves empty, shows in English. Texts with `{name}` placeholders keep them; the call site fills
//! them in after the lookup.

use openaec_ui::Language;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

/// The catalogues, by language: the file's text.
const CATALOGUES: [(Language, &str); 8] = [
    (Language::German, include_str!("../locales/de.json")),
    (Language::French, include_str!("../locales/fr.json")),
    (Language::Spanish, include_str!("../locales/es.json")),
    (Language::Italian, include_str!("../locales/it.json")),
    (Language::Portuguese, include_str!("../locales/pt.json")),
    (Language::Polish, include_str!("../locales/pl.json")),
    (Language::Swedish, include_str!("../locales/sv.json")),
    (Language::Turkish, include_str!("../locales/tr.json")),
];

#[derive(Deserialize)]
struct File {
    locale: String,
    messages: HashMap<String, String>,
}

/// A catalogue read: English text to the language's text.
type Messages = HashMap<&'static str, &'static str>;

/// Makes the catalogues known to the interface library. Safe to call more than once.
pub fn install() {
    openaec_ui::locale::install_catalogue(lookup);
}

fn lookup(l: Language, en: &'static str) -> Option<&'static str> {
    messages(l)?.get(en).copied()
}

/// The catalogue of `l`, read once (`None` for Dutch, English and a language without one).
fn messages(l: Language) -> Option<&'static Messages> {
    static READ: OnceLock<HashMap<Language, Messages>> = OnceLock::new();
    READ.get_or_init(|| {
        CATALOGUES
            .iter()
            .filter_map(|(l, json)| match parse(json) {
                Ok((locale, messages)) if locale == l.code() => Some((*l, messages)),
                Ok((locale, _)) => {
                    eprintln!("catalogue for {}: its locale is {locale}", l.code());
                    None
                }
                Err(e) => {
                    eprintln!("catalogue for {}: {e}", l.code());
                    None
                }
            })
            .collect()
    })
    .get(&l)
}

/// The locale and the non-empty messages of a catalogue, kept for the app's lifetime.
fn parse(json: &str) -> Result<(String, Messages), serde_json::Error> {
    let file: File = serde_json::from_str(json)?;
    let keep = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
    let messages = file.messages.into_iter().filter(|(_, text)| !text.trim().is_empty()).map(|(en, text)| (keep(en), keep(text))).collect();
    Ok((file.locale, messages))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The English texts of the interface: every `text(nl, en)` and `translate(en)` with literal
    /// strings in the app and the interface library.
    fn english_texts() -> Vec<String> {
        let mut texts = Vec::new();
        for root in ["src", "vendor/openaec-ui/src"] {
            let mut stack = vec![std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(root)];
            while let Some(dir) = stack.pop() {
                for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        stack.push(path);
                    } else if path.extension().is_some_and(|e| e == "rs") {
                        // tests hold example calls, not interface texts
                        let source = std::fs::read_to_string(&path).unwrap();
                        texts.extend(literal_calls(shipped(&source)));
                    }
                }
            }
        }
        texts.sort();
        texts.dedup();
        texts
    }

    /// `source` up to its test module (`#[cfg(test)] mod …`), whose calls are examples.
    fn shipped(source: &str) -> &str {
        let mut from = 0;
        while let Some(at) = source[from..].find("#[cfg(test)]") {
            let at = from + at;
            if source[at + "#[cfg(test)]".len()..].trim_start().starts_with("mod ") {
                return &source[..at];
            }
            from = at + 1;
        }
        source
    }

    /// The English argument of each `.text("…", "…")` and `.translate("…")` in `source`.
    fn literal_calls(source: &str) -> Vec<String> {
        let mut found = Vec::new();
        for (marker, english_at) in [(".text(", 1), (".translate(", 0)] {
            let mut rest = source;
            while let Some(at) = rest.find(marker) {
                rest = &rest[at + marker.len()..];
                let mut cursor = rest;
                let mut strings = Vec::new();
                while strings.len() <= english_at {
                    let Some((s, after)) = string_literal(cursor) else { break };
                    strings.push(s);
                    cursor = after.trim_start();
                    match cursor.strip_prefix(',') {
                        Some(after_comma) if strings.len() <= english_at => cursor = after_comma,
                        _ => break,
                    }
                }
                // a trailing comma may close the arguments
                let close = cursor.trim_start();
                let close = close.strip_prefix(',').map_or(close, str::trim_start);
                if strings.len() == english_at + 1 && close.starts_with(')') {
                    found.push(strings.pop().unwrap());
                }
            }
        }
        found
    }

    /// A string literal at the start of `s` (after white space): its value and what follows it.
    fn string_literal(s: &str) -> Option<(String, &str)> {
        let s = s.trim_start().strip_prefix('"')?;
        let mut value = String::new();
        let mut chars = s.char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => return Some((value, &s[i + 1..])),
                '\\' => match chars.next()?.1 {
                    'n' => value.push('\n'),
                    't' => value.push('\t'),
                    '\\' => value.push('\\'),
                    '"' => value.push('"'),
                    '\'' => value.push('\''),
                    '0' => value.push('\0'),
                    'u' => {
                        let rest = &s[i + 2..];
                        let close = rest.find('}')?;
                        value.push(char::from_u32(u32::from_str_radix(&rest[1..close], 16).ok()?)?);
                        for _ in 0..close + 1 {
                            chars.next();
                        }
                    }
                    '\n' => {
                        // a line continuation: the next line's leading white space goes
                        while chars.clone().next().is_some_and(|(_, c)| c.is_whitespace()) {
                            chars.next();
                        }
                    }
                    other => value.push(other),
                },
                c => value.push(c),
            }
        }
        None
    }

    #[test]
    fn literal_calls_are_found() {
        let found = literal_calls(r#"l.text("Openen", "Open") l.text(nl, en) l.translate("Saved \"{n}\"") x.text("a",
            "b\u{b2}") l.text(
                "c",
                "d",
            )"#);
        assert_eq!(found, vec!["Open", "b\u{b2}", "d", "Saved \"{n}\""]);
        assert_eq!(shipped("a #[cfg(test)]\nfn b() {}\n#[cfg(test)]\nmod tests {}"), "a #[cfg(test)]\nfn b() {}\n");
    }

    #[test]
    fn every_catalogue_reads_and_names_its_language() {
        for (l, json) in CATALOGUES {
            let (locale, messages) = parse(json).unwrap_or_else(|e| panic!("{}: {e}", l.code()));
            assert_eq!(locale, l.code());
            assert!(!messages.is_empty(), "{}", l.code());
        }
    }

    #[test]
    fn every_interface_text_is_translated() {
        let texts = english_texts();
        assert!(texts.len() > 200, "{} texts found", texts.len());
        for (l, json) in CATALOGUES {
            let (_, messages) = parse(json).unwrap();
            let missing: Vec<&String> = texts.iter().filter(|t| !messages.contains_key(t.as_str())).collect();
            assert!(missing.is_empty(), "{} lacks {} of {} texts, e.g. {:?}", l.code(), missing.len(), texts.len(), &missing[..missing.len().min(10)]);
        }
    }

    #[test]
    fn placeholders_survive_translation() {
        for (l, json) in CATALOGUES {
            let (_, messages) = parse(json).unwrap();
            for (en, text) in &messages {
                let names = |s: &str| -> Vec<String> {
                    let mut names: Vec<String> = s.split('{').skip(1).filter_map(|p| p.split_once('}').map(|(n, _)| n.to_string())).collect();
                    names.sort();
                    names
                };
                assert_eq!(names(en), names(text), "{}: {en:?} -> {text:?}", l.code());
            }
        }
    }

    #[test]
    fn the_installed_catalogue_translates() {
        install();
        assert_eq!(Language::German.text("Openen", "Open"), "Öffnen");
        assert_eq!(Language::French.text("Openen", "Open"), "Ouvrir");
        assert_eq!(Language::German.translate("a text no catalogue has"), "a text no catalogue has");
    }
}
