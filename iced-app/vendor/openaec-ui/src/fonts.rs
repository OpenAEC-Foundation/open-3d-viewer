//! Embedded fonts ensure that host machine fonts do not change the shell.
use iced::advanced::graphics::text::cosmic_text;
use iced::{font::Weight, Font};
pub const BODY: Font = Font::with_name("Inter");
pub const MEDIUM: Font = Font { weight: Weight::Medium, ..BODY };
pub const STRONG: Font = Font { weight: Weight::Semibold, ..BODY };
pub const HEADING: Font = Font { weight: Weight::Medium, ..Font::with_name("Space Grotesk") };
pub const MONO: Font = Font::with_name("JetBrains Mono");
/// Numbers in tables, number fields and reports: Inter with digits of one width
/// and a distinct I and l frozen in (issue #28), as iced applies no OpenType
/// features. Made by `scripts/build_inter_aec.py`.
pub const NUMBERS: Font = Font::with_name("Inter AEC");
pub const NUMBERS_STRONG: Font = Font { weight: Weight::Semibold, ..NUMBERS };
/// Characters every OpenAEC font must show (issue #25), grouped for the gallery.
/// `scripts/check_font_glyphs.py` reads this list; keep one group per line.
/// The last group holds look-alikes that must stay distinguishable.
pub const AEC_TEST_SET: [(&str, &str); 6] = [
    ("Maten", "⌀ Ø ø m² m³ °"),
    ("Rekenen", "± × − – ≤ ≥ €"),
    ("Grieks", "α β σ λ"),
    ("Nederlands", "Ĳ ĳ é ë ç"),
    ("Turks", "İ ı Ğ Ş"),
    ("Verwisselbaar", "I l 1 O 0 rn m 5 S 2 Z"),
];
pub const FILES: [&[u8]; 9] = [
    include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../assets/fonts/SpaceGrotesk-Medium.ttf"),
    include_bytes!("../assets/fonts/SpaceGrotesk-Bold.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../assets/fonts/InterAEC-Regular.ttf"),
    include_bytes!("../assets/fonts/InterAEC-Medium.ttf"),
    include_bytes!("../assets/fonts/InterAEC-SemiBold.ttf"),
];
/// Fallback fonts for characters the fonts above lack (issue #25): Greek and Ĳ
/// from Noto Sans, the diameter sign ⌀ from Noto Sans Symbols.
pub const FALLBACK_FAMILIES: [&str; 2] = ["Noto Sans", "Noto Sans Symbols"];
pub const FALLBACK_FILES: [&[u8]; 2] = [
    include_bytes!("../assets/fonts/NotoSans-Regular.ttf"),
    include_bytes!("../assets/fonts/NotoSansSymbols-Regular.ttf"),
];

/// Makes every computer show missing characters from the bundled Noto fonts
/// instead of whatever system font it has. Call once at start, before the
/// first frame; calling again does nothing.
///
/// cosmic-text only takes a fallback face with exactly the requested weight,
/// so each Noto face is registered for every weight from 100 to 900, and its
/// fallback list puts the Noto families before the platform ones.
pub fn install_fallback() {
    use iced::advanced::graphics::text::font_system;
    use cosmic_text::fontdb;
    use std::sync::{Arc, Once};
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        let mut system = font_system().write().expect("font system");
        let raw = system.raw();
        let locale = raw.locale().to_owned();
        let mut db = raw.db().clone();
        for bytes in FALLBACK_FILES {
            for id in db.load_font_source(fontdb::Source::Binary(Arc::new(bytes))) {
                let Some(face) = db.face(id).cloned() else { continue };
                for weight in (100..=900).step_by(100).filter(|w| *w != face.weight.0) {
                    db.push_face_info(fontdb::FaceInfo { weight: fontdb::Weight(weight), ..face.clone() });
                }
            }
        }
        *raw = cosmic_text::FontSystem::new_with_locale_and_db_and_fallback(locale, db, Fallback::new());
    });
}

/// Loads a variable font unchanged and makes it available at each of
/// `weights`. cosmic-text only picks a face whose weight matches exactly, and a
/// variable font registers only its default weight, so each weight gets its own
/// entry for the same file; cosmic-text then draws that weight from the `wght`
/// axis. The file stays unmodified, which matters for fonts with a Reserved Font
/// Name. Call it once at start, after [`install_fallback`], before the first frame.
pub fn load_variable(bytes: &'static [u8], weights: &[u16]) {
    use cosmic_text::fontdb;
    use iced::advanced::graphics::text::font_system;
    use std::sync::Arc;
    let mut system = font_system().write().expect("font system");
    let db = system.raw().db_mut();
    for id in db.load_font_source(fontdb::Source::Binary(Arc::new(bytes))) {
        let Some(face) = db.face(id).cloned() else { continue };
        for &weight in weights.iter().filter(|w| **w != face.weight.0) {
            db.push_face_info(fontdb::FaceInfo { weight: fontdb::Weight(weight), ..face.clone() });
        }
    }
}

/// The bundled fallback families first, then the platform's own list.
struct Fallback {
    common: Vec<&'static str>,
    platform: cosmic_text::PlatformFallback,
}

impl Fallback {
    fn new() -> Self {
        use cosmic_text::Fallback as _;
        let platform = cosmic_text::PlatformFallback;
        let common = FALLBACK_FAMILIES.iter().copied().chain(platform.common_fallback().iter().copied()).collect();
        Self { common, platform }
    }
}

impl cosmic_text::Fallback for Fallback {
    fn common_fallback(&self) -> &[&'static str] {
        &self.common
    }

    fn forbidden_fallback(&self) -> &[&'static str] {
        self.platform.forbidden_fallback()
    }

    fn script_fallback(&self, script: unicode_script::Script, locale: &str) -> &[&'static str] {
        self.platform.script_fallback(script, locale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic_text::{Attrs, Buffer, Family, Metrics, Shaping};
    use iced::advanced::graphics::text::font_system;

    #[test]
    fn missing_characters_come_from_the_bundled_fallback() {
        install_fallback();
        let mut system = font_system().write().expect("font system");
        for bytes in FILES {
            system.load_font(std::borrow::Cow::Borrowed(bytes));
        }
        let raw = system.raw();
        // A heading in Space Grotesk Medium has no weight-500 system face to fall
        // back on; before issue #25 that gave a random font per computer.
        for (family, weight, text, expected) in [
            ("Space Grotesk", 500, "α", "Noto Sans"),
            ("Space Grotesk", 700, "λ", "Noto Sans"),
            ("Inter", 400, "⌀", "Noto Sans Symbols"),
            ("Inter", 600, "⌀", "Noto Sans Symbols"),
            ("JetBrains Mono", 400, "Ĳ", "Noto Sans"),
            ("Inter", 400, "Ø", "Inter"),
        ] {
            let attrs = Attrs::new().family(Family::Name(family)).weight(cosmic_text::Weight(weight));
            let mut buffer = Buffer::new(raw, Metrics::new(14.0, 20.0));
            buffer.set_text(raw, text, &attrs, Shaping::Advanced, None);
            buffer.shape_until_scroll(raw, false);
            let glyph = buffer.layout_runs().next().expect("one line").glyphs[0].font_id;
            let face = raw.db().face(glyph).expect("face");
            assert_eq!(face.families[0].0, expected, "{text} in {family} {weight}");
        }
    }
}
