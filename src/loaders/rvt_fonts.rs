//! The typefaces of the texts of a Revit file ([`crate::typeface::Face`]).
//!
//! Read through the generic object view, named schema fields only:
//! - a text type (`TextNoteAttributes`, and the type of a label) holds a `LineAndTextAttr` as
//!   `m_lineAndTextAttr`, a dimension type a pointer to one as `m_pLineAndTextAttr`: its
//!   `m_fontId` names a `FontElem`, whose `m_pFont` has the family as `m_name`, and its
//!   `m_bBold`, `m_bItalic` and `m_bUnderline` say how the family is set;
//! - how wide the letters are to what the font draws is a value of the type, by parameter;
//! - a text's own wording is RTF: bold, italic and underline switched on for all of it count
//!   for the text (a text with some words bold is set as its type says).

use crate::typeface::Face;
use rvt_model::{FieldValue, ObjectView};

/// The parameter of a type that says how wide its letters are (1 as the font draws them).
const WIDTH: i64 = -1006327;
/// The parameter of a text type that says how far apart its tab stops are (feet on paper).
const TAB: i64 = -1006326;
/// What a letter's em is to the text size of a type, as [`super::rvt_drawings`] sets it.
const EM: f64 = 4.0 * 25.4 / 72.0;

/// The typeface of the texts of a type (a text type, the type of a label, a dimension type);
/// none for a type that names no font.
pub(super) fn face(doc: &rvt_read::Document, kind: i64) -> Option<Face> {
    let o = doc.object(u64::try_from(kind).ok()?)?;
    face_of(doc, &o)
}

fn face_of(doc: &rvt_read::Document, o: &ObjectView) -> Option<Face> {
    let attr = |name: &str| o.path(&["m_lineAndTextAttr", name]).or_else(|| o.path(&["m_pLineAndTextAttr", name]));
    let font = attr("m_fontId")?.element().and_then(|f| doc.object(u64::try_from(f).ok()?))?;
    let family = font.path(&["m_pFont", "m_name"]).or_else(|| font.get("m_name"))?.as_str()?.trim().to_string();
    if family.is_empty() {
        return None;
    }
    let on = |name: &str| matches!(attr(name), Some(FieldValue::Bool(true)));
    let value = |parameter: i64| {
        o.path(&["m_pParamValueSetDouble", "m_paramSet"])
            .and_then(FieldValue::as_list)
            .unwrap_or(&[])
            .iter()
            .filter_map(|p| p.as_object())
            .find(|p| p.get("m_paramId").and_then(FieldValue::element) == Some(parameter))
            .and_then(|p| p.get("m_value")?.as_f64())
            .filter(|v| v.is_finite() && *v > 0.0)
    };
    let width = value(WIDTH).filter(|w| (0.1..=10.0).contains(w)).unwrap_or(1.0);
    // the stops of its tabs lie a length on paper apart: in ems of its letters
    let size = font.path(&["m_pFont", "m_size"]).or_else(|| font.get("m_size")).and_then(FieldValue::as_f64).filter(|s| s.is_finite() && *s > 0.0);
    let tab = value(TAB).zip(size).map_or(0.0, |(tab, size)| tab / (size * EM)).clamp(0.0, 1000.0);
    Some(Face { family: family.into(), bold: on("m_bBold"), italic: on("m_bItalic"), underline: on("m_bUnderline"), width: width as f32, tab: tab as f32 })
}

/// What the RTF of a text switches on for all of its letters: bold, italic, underline.
pub(super) fn rtf_marks(rtf: &[u8]) -> [bool; 3] {
    // per mark: whether it is on, the letters set with it and those set without
    let mut on = [false; 3];
    let mut with = [0usize; 3];
    let mut letters = 0usize;
    let mut depth = 0usize;
    // the groups that hold no wording: the font table, the colours, the generator
    let mut skip: Option<usize> = None;
    let mut i = 0;
    while i < rtf.len() {
        match rtf[i] {
            b'{' => {
                depth += 1;
                if skip.is_none() && depth > 1 {
                    let rest = &rtf[i + 1..];
                    if [&b"\\fonttbl"[..], b"\\colortbl", b"\\*", b"\\stylesheet", b"\\info", b"\\pntext"].iter().any(|g| rest.starts_with(g)) {
                        skip = Some(depth);
                    }
                }
                i += 1;
            }
            b'}' => {
                if skip == Some(depth) {
                    skip = None;
                }
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b'\\' => {
                let rest = &rtf[i + 1..];
                let name = rest.iter().take_while(|b| b.is_ascii_alphabetic()).count();
                if name == 0 {
                    // an escaped character; `\'hh` is a letter by its code
                    let taken = if rest.first() == Some(&b'\'') { 3 } else { 1 };
                    if skip.is_none() && matches!(rest.first(), Some(b'\'' | b'\\' | b'{' | b'}')) {
                        letters += 1;
                        (0..3).filter(|&k| on[k]).for_each(|k| with[k] += 1);
                    }
                    i += 1 + taken.min(rest.len());
                    continue;
                }
                let digits = rest[name..].iter().take_while(|b| b.is_ascii_digit() || **b == b'-').count();
                let off = &rest[name..name + digits] == b"0";
                if skip.is_none() {
                    match &rest[..name] {
                        b"b" => on[0] = !off,
                        b"i" => on[1] = !off,
                        b"ul" => on[2] = !off,
                        b"ulnone" => on[2] = false,
                        b"plain" => on = [false; 3],
                        _ => {}
                    }
                }
                i += 1 + name + digits;
                // the space that ends a control word is not a letter
                if rtf.get(i) == Some(&b' ') {
                    i += 1;
                }
            }
            c => {
                // the stream may end in a zero byte: no letter
                if skip.is_none() && !c.is_ascii_whitespace() && !c.is_ascii_control() {
                    letters += 1;
                    (0..3).filter(|&k| on[k]).for_each(|k| with[k] += 1);
                }
                i += 1;
            }
        }
    }
    [0, 1, 2].map(|k| letters > 0 && with[k] == letters)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn rtf_tells_what_all_of_a_text_is_set_with() {
        let head = "{\\rtf1\\ansi{\\fonttbl{\\f0\\fnil\\fcharset0 Segoe UI;}}\n{\\colortbl ;\\red0\\green0\\blue0;}\n{\\*\\generator Riched20 10.0}\\viewkind4\\uc1 \n";
        let marks = |body: &str| rtf_marks(format!("{head}{body}}}\n").as_bytes());
        assert_eq!(marks("\\pard\\cf1\\f0\\fs19 plain text\\par\n"), [false, false, false]);
        assert_eq!(marks("\\pard\\cf1\\b\\f0\\fs28 PAALSTAAT\\b0\\par\n"), [true, false, false]);
        assert_eq!(rtf_marks(format!("{head}\\pard\\cf1\\b\\f0\\fs26\\lang2057 PAALSTAAT\\b0\\par\r\n}}\r\n\0").as_bytes()), [true, false, false]);
        assert_eq!(marks("\\pard\\cf1\\i\\f0\\fs14 geen\\i0\\par\n"), [false, true, false]);
        assert_eq!(marks("\\pard\\cf1\\ul\\f0\\fs26 PRINCIPE.\\ulnone\\par\n"), [false, false, true]);
        // some words bold: the text is as its type says
        assert_eq!(marks("\\pard\\cf1\\b\\f0\\fs19 1 Doorsnede \\b0 van \\i architect \\i0 afstemmen \\par\n"), [false, false, false]);
    }

    /// The typefaces a project's texts are set in (`RVT_DRAWINGS`, read-only; skipped when absent).
    #[test]
    fn the_types_of_a_file_name_their_typefaces() {
        let Some(path) = std::env::var_os("RVT_DRAWINGS").map(std::path::PathBuf::from).filter(|p| p.exists()) else {
            eprintln!("skipped: set RVT_DRAWINGS to a Revit project with texts");
            return;
        };
        let doc = rvt_read::Document::open(&path).expect("rvt");
        let type_of = |o: &ObjectView, field: &str| o.get(field).and_then(FieldValue::element);
        // per use: the faces of the types in use, with how many texts are of each
        let mut uses: BTreeMap<String, usize> = BTreeMap::new();
        let mut count = |what: &str, kind: Option<i64>| {
            let said = match kind.map(|k| (k, face(&doc, k))) {
                Some((_, Some(f))) => format!("{} bold={} italic={} underline={} width={} installed={}", f.family, f.bold, f.italic, f.underline, f.width, f.installed()),
                Some((k, None)) => format!("type {k} names no font"),
                None => "no type".to_string(),
            };
            *uses.entry(format!("{what}: {said}")).or_default() += 1;
        };
        for id in doc.object_ids_of_class("TextNote") {
            let Some(o) = doc.object(id) else { continue };
            count("text", type_of(&o, "m_attrId"));
        }
        for id in doc.object_ids_of_class("LinearConstraint") {
            let Some(o) = doc.object(id).filter(|o| o.category == Some(-2000260)) else { continue };
            count("dimension", type_of(&o, "m_styleSymbolId"));
        }
        for (said, n) in &uses {
            eprintln!("{n:6}  {said}");
        }
        assert!(uses.keys().any(|k| k.starts_with("text: ") && !k.contains("no")));
    }
}
