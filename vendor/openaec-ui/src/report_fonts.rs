//! Fonts for reports and PDF exports (issue #26).
//!
//! Every OpenAEC report embeds the same files as the interface, from the font
//! package in `assets/fonts`. The roles below fix which file a report uses for
//! what; [`runs`] splits text so each piece goes to a font that has its
//! characters, because PDF writers do not fall back to other fonts themselves.
//!
//! All fonts are SIL OFL 1.1 without a Reserved Font Name, so embedding a
//! subset is allowed. Ship [`ReportFont::licence`] with the application.
use iced::advanced::graphics::text::cosmic_text::skrifa::{self, MetadataProvider};

/// What a piece of report text is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Running text, table cells with words, captions.
    Body,
    /// Emphasis in running text and table headers.
    Strong,
    /// Section headings, as `fonts::HEADING` in the interface.
    Heading,
    /// The report title on the cover or first page.
    Title,
    /// Columns of numbers and amounts, in Inter AEC: Inter's own digits differ
    /// in width (1 is narrower than 0) and PDF writers apply no tabular figures,
    /// so Inter AEC has them frozen in (issue #28).
    Numbers,
    /// File names, codes and other literal text.
    Code,
}

/// One embeddable font file.
#[derive(Debug)]
pub struct ReportFont {
    /// Family name as in the interface and CSS, such as `"Inter"`.
    pub family: &'static str,
    pub weight: u16,
    /// The PostScript name inside the file. A PDF names the font by this (after
    /// the six-letter subset tag), so the name and the embedded data agree.
    pub postscript_name: &'static str,
    pub file: &'static str,
    pub bytes: &'static [u8],
    /// The SIL Open Font License text with the copyright line of this font.
    pub licence: &'static str,
}

impl ReportFont {
    /// Whether this font has a glyph for `ch`.
    pub fn covers(&self, ch: char) -> bool {
        skrifa::FontRef::new(self.bytes).is_ok_and(|font| font.charmap().map(ch).is_some())
    }
}

macro_rules! report_font {
    ($family:literal, $weight:literal, $postscript:literal, $file:literal, $licence:literal) => {
        ReportFont {
            family: $family,
            weight: $weight,
            postscript_name: $postscript,
            file: $file,
            bytes: include_bytes!(concat!("../assets/fonts/", $file)),
            licence: include_str!(concat!("../licenses/", $licence)),
        }
    };
}

pub static INTER_REGULAR: ReportFont = report_font!("Inter", 400, "Inter-Regular", "Inter-Regular.ttf", "Inter-OFL.txt");
pub static INTER_SEMIBOLD: ReportFont = report_font!("Inter", 600, "Inter-SemiBold", "Inter-SemiBold.ttf", "Inter-OFL.txt");
pub static SPACE_GROTESK_MEDIUM: ReportFont = report_font!("Space Grotesk", 500, "SpaceGrotesk-Medium", "SpaceGrotesk-Medium.ttf", "SpaceGrotesk-OFL.txt");
pub static SPACE_GROTESK_BOLD: ReportFont = report_font!("Space Grotesk", 700, "SpaceGrotesk-Bold", "SpaceGrotesk-Bold.ttf", "SpaceGrotesk-OFL.txt");
pub static INTER_AEC: ReportFont = report_font!("Inter AEC", 400, "InterAEC-Regular", "InterAEC-Regular.ttf", "Inter-OFL.txt");
pub static JETBRAINS_MONO: ReportFont = report_font!("JetBrains Mono", 400, "JetBrainsMono-Regular", "JetBrainsMono-Regular.ttf", "JetBrainsMono-OFL.txt");
pub static NOTO_SANS: ReportFont = report_font!("Noto Sans", 400, "NotoSans-Regular", "NotoSans-Regular.ttf", "NotoSans-OFL.txt");
pub static NOTO_SANS_SYMBOLS: ReportFont = report_font!("Noto Sans Symbols", 400, "NotoSansSymbols-Regular", "NotoSansSymbols-Regular.ttf", "NotoSansSymbols-OFL.txt");

/// Fonts for characters the role's font lacks (⌀, Greek in Space Grotesk, Ĳ in
/// JetBrains Mono), in the order [`runs`] tries them.
pub static FALLBACK: [&ReportFont; 2] = [&NOTO_SANS, &NOTO_SANS_SYMBOLS];

/// Every file a report may embed: the role fonts and the fallback.
pub static ALL: [&ReportFont; 8] = [&INTER_REGULAR, &INTER_SEMIBOLD, &SPACE_GROTESK_MEDIUM, &SPACE_GROTESK_BOLD, &INTER_AEC, &JETBRAINS_MONO, &NOTO_SANS, &NOTO_SANS_SYMBOLS];

/// The font for a role.
pub fn font(role: Role) -> &'static ReportFont {
    match role {
        Role::Body => &INTER_REGULAR,
        Role::Strong => &INTER_SEMIBOLD,
        Role::Heading => &SPACE_GROTESK_MEDIUM,
        Role::Title => &SPACE_GROTESK_BOLD,
        Role::Numbers => &INTER_AEC,
        Role::Code => &JETBRAINS_MONO,
    }
}

/// Splits `text` into pieces that each have one font with all their
/// characters: the role's font where it can, otherwise the first fallback that
/// has the character. A character no font has stays with the role's font.
pub fn runs(text: &str, role: Role) -> Vec<(&'static ReportFont, &str)> {
    let primary = font(role);
    let pick = |ch: char| {
        if ch.is_whitespace() || primary.covers(ch) {
            return primary;
        }
        FALLBACK.iter().copied().find(|f| f.covers(ch)).unwrap_or(primary)
    };
    let mut out: Vec<(&'static ReportFont, &str)> = Vec::new();
    let mut start = 0;
    let mut current: Option<&'static ReportFont> = None;
    for (at, ch) in text.char_indices() {
        // Spaces stay in the run they are in, so "⌀ 12" does not split twice.
        let next = if ch.is_whitespace() { current.unwrap_or(primary) } else { pick(ch) };
        match current {
            Some(font) if std::ptr::eq(font, next) => {}
            Some(font) => {
                out.push((font, &text[start..at]));
                start = at;
                current = Some(next);
            }
            None => current = Some(next),
        }
    }
    if let Some(font) = current {
        out.push((font, &text[start..]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_fonts_name_their_own_files() {
        for font in ALL {
            let data = skrifa::FontRef::new(font.bytes).expect("font");
            let name = data
                .localized_strings(skrifa::string::StringId::POSTSCRIPT_NAME)
                .english_or_first()
                .expect("PostScript name")
                .chars()
                .collect::<String>();
            assert_eq!(name, font.postscript_name, "{}", font.file);
            assert!(font.licence.contains("SIL Open Font License"), "{}", font.file);
        }
    }

    #[test]
    fn numbers_use_digits_of_one_width() {
        let data = skrifa::FontRef::new(font(Role::Numbers).bytes).expect("font");
        let metrics = data.glyph_metrics(skrifa::instance::Size::unscaled(), skrifa::instance::LocationRef::default());
        let width = |chars: &str| chars.chars().map(|c| metrics.advance_width(data.charmap().map(c).expect("glyph"))).collect::<Vec<_>>();
        // Every digit is as wide as the others, and so are the separators, so
        // "1.234,56" and "9.876,54" line up in a column.
        let (digits, separators) = (width("0123456789"), width(",."));
        assert!(digits.windows(2).all(|w| w[0] == w[1]), "{digits:?}");
        assert!(separators.windows(2).all(|w| w[0] == w[1]), "{separators:?}");
        // Inter's own digits differ: that is why reports use Inter AEC.
        let inter = skrifa::FontRef::new(INTER_REGULAR.bytes).expect("font");
        let inter_metrics = inter.glyph_metrics(skrifa::instance::Size::unscaled(), skrifa::instance::LocationRef::default());
        let one = inter_metrics.advance_width(inter.charmap().map('1').expect("one"));
        let zero = inter_metrics.advance_width(inter.charmap().map('0').expect("zero"));
        assert_ne!(one, zero);
    }

    #[test]
    fn inter_aec_tells_i_l_and_one_apart() {
        // The capital I and small l are their own glyphs in Inter AEC, not Inter's.
        let glyph = |font: &ReportFont, c| skrifa::FontRef::new(font.bytes).expect("font").charmap().map(c).expect("glyph");
        for c in ['I', 'l'] {
            assert_ne!(glyph(&INTER_AEC, c), glyph(&INTER_REGULAR, c), "{c}");
        }
    }

    #[test]
    fn runs_send_missing_characters_to_the_fallback() {
        let pieces = |text, role| runs(text, role).into_iter().map(|(f, s)| (f.postscript_name, s)).collect::<Vec<_>>();
        assert_eq!(pieces("Paal ⌀ 400 mm", Role::Body), [("Inter-Regular", "Paal "), ("NotoSansSymbols-Regular", "⌀ "), ("Inter-Regular", "400 mm")]);
        assert_eq!(pieces("Spanning σ en λ", Role::Heading), [("SpaceGrotesk-Medium", "Spanning "), ("NotoSans-Regular", "σ "), ("SpaceGrotesk-Medium", "en "), ("NotoSans-Regular", "λ")]);
        assert_eq!(pieces("Ĳsselmeer", Role::Code), [("NotoSans-Regular", "Ĳ"), ("JetBrainsMono-Regular", "sselmeer")]);
        assert_eq!(pieces("1.234,56", Role::Numbers), [("InterAEC-Regular", "1.234,56")]);
        assert!(runs("", Role::Body).is_empty());
    }
}
