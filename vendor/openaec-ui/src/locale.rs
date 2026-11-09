use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// The language of the interface. Dutch and English are written at the call site
/// ([`Language::text`]); every other language is looked up by the English text in a catalogue
/// the application installs ([`install_catalogue`]), and falls back to English where the
/// catalogue has no text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Language { #[default] Dutch, English, German, French }

/// Finds the text of an English `en` in a language's catalogue.
pub type Lookup = fn(Language, &'static str) -> Option<&'static str>;

static LOOKUP: OnceLock<Lookup> = OnceLock::new();

/// Installs the catalogues of the languages beyond Dutch and English. The first call counts;
/// before it, those languages show English.
pub fn install_catalogue(lookup: Lookup) {
    let _ = LOOKUP.set(lookup);
}

impl Language {
    pub const ALL: [Self; 4] = [Self::Dutch, Self::English, Self::German, Self::French];

    /// The text in this language: `nl` in Dutch, `en` in English, the catalogue's text for `en`
    /// in the others.
    pub fn text(self, nl: &'static str, en: &'static str) -> &'static str {
        match self { Self::Dutch => nl, _ => self.translate(en) }
    }

    /// An English text in this language: from the catalogue, or `en` itself in English, in
    /// Dutch and where the catalogue has none. For texts that have no Dutch at the call site.
    pub fn translate(self, en: &'static str) -> &'static str {
        match self {
            Self::Dutch | Self::English => en,
            _ => LOOKUP.get().and_then(|lookup| lookup(self, en)).unwrap_or(en),
        }
    }

    /// Its place in [`Language::ALL`].
    pub fn index(self) -> usize {
        self as usize
    }

    /// Its ISO 639-1 code.
    pub fn code(self) -> &'static str {
        match self { Self::Dutch => "nl", Self::English => "en", Self::German => "de", Self::French => "fr" }
    }

    /// The language of an ISO 639-1 code or a locale that starts with one (`de`, `fr-BE`).
    pub fn from_code(code: &str) -> Option<Self> {
        let code = code.trim().get(..2)?.to_ascii_lowercase();
        Self::ALL.into_iter().find(|l| l.code() == code)
    }

    /// Numbers are written with a decimal comma (and the thousands grouped by a point or a space).
    pub fn decimal_comma(self) -> bool {
        self != Self::English
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Dutch => "Nederlands",
            Self::English => "English",
            Self::German => "Deutsch",
            Self::French => "Français",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip() {
        for l in Language::ALL {
            assert_eq!(Language::from_code(l.code()), Some(l));
            assert_eq!(Language::ALL[l.index()], l);
        }
        assert_eq!(Language::from_code("fr-BE"), Some(Language::French));
        assert_eq!(Language::from_code("DE"), Some(Language::German));
        assert_eq!(Language::from_code("x"), None);
        assert_eq!(Language::from_code("es"), None);
    }

    #[test]
    fn without_a_catalogue_other_languages_fall_back_to_english() {
        assert_eq!(Language::Dutch.text("Openen", "Open"), "Openen");
        assert_eq!(Language::English.text("Openen", "Open"), "Open");
        assert_eq!(Language::French.translate("a text no catalogue has"), "a text no catalogue has");
    }
}
