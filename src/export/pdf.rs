//! A 2D drawing as a vector PDF: one page (a sheet's paper, or what a view draws at its scale),
//! its lines with the pens' line weights and its texts in Helvetica. A small writer of its own:
//! PDF 1.4, no compression, WinAnsi text (a character outside it becomes `?`).

use crate::drawing2d::{Align, Drawing2d, Pen, Text2d};
use std::fmt::Write;

/// Points per millimetre.
const POINTS: f64 = 72.0 / 25.4;
/// Helvetica's capitals per em.
const CAPITALS: f64 = 0.718;

/// A number with at most `decimals` decimals and no trailing zeros.
pub(crate) fn num_to(v: f64, decimals: usize) -> String {
    let s = format!("{v:.decimals$}");
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.') } else { &s };
    if s == "-0" { "0".to_string() } else { s.to_string() }
}

fn num(v: f64) -> String {
    num_to(v, 3)
}

/// The WinAnsi code of a character; `?` for one it lacks.
fn win_ansi(c: char) -> u8 {
    const EXTRA: [(char, u8); 27] = [
        ('€', 0x80), ('‚', 0x82), ('ƒ', 0x83), ('„', 0x84), ('…', 0x85), ('†', 0x86), ('‡', 0x87), ('ˆ', 0x88), ('‰', 0x89),
        ('Š', 0x8A), ('‹', 0x8B), ('Œ', 0x8C), ('Ž', 0x8E), ('‘', 0x91), ('’', 0x92), ('“', 0x93), ('”', 0x94), ('•', 0x95),
        ('–', 0x96), ('—', 0x97), ('˜', 0x98), ('™', 0x99), ('š', 0x9A), ('›', 0x9B), ('œ', 0x9C), ('ž', 0x9E), ('Ÿ', 0x9F),
    ];
    match c as u32 {
        0x20..=0x7E | 0xA0..=0xFF => c as u8,
        _ => EXTRA.iter().find(|(e, _)| *e == c).map_or(b'?', |(_, b)| *b),
    }
}

/// A PDF string literal of `text` in WinAnsi, brackets and backslashes escaped, other than
/// printable ASCII as octal.
pub(crate) fn literal(text: &str) -> String {
    let mut s = String::from("(");
    for b in text.chars().map(win_ansi) {
        match b {
            b'(' | b')' | b'\\' => {
                s.push('\\');
                s.push(b as char);
            }
            0x20..=0x7E => s.push(b as char),
            _ => {
                let _ = write!(s, "\\{b:03o}");
            }
        }
    }
    s.push(')');
    s
}

/// The page's drawing operators.
fn content(d: &Drawing2d) -> String {
    let (lo, _) = d.page();
    let k = d.paper_per_unit() * 1000.0 * POINTS;
    let at = |q: [f64; 2]| format!("{} {}", num((q[0] - lo[0]) * k), num((q[1] - lo[1]) * k));
    let mut s = String::from("1 J 1 j 0 G 0 g\n");
    for pen in Pen::ALL {
        let mut segments: Vec<([f64; 2], [f64; 2])> = d.lines.iter().filter(|l| l.pen == pen).map(|l| (l.a, l.b)).collect();
        if pen == Pen::Dimension {
            segments.extend(d.dimensions.iter().flat_map(|dim| dim.lines()).map(|[a, b]| (a, b)));
        }
        if segments.is_empty() {
            continue;
        }
        let _ = writeln!(s, "{} w", num(pen.weight_mm() * POINTS));
        for (a, b) in segments {
            let _ = writeln!(s, "{} m {} l", at(a), at(b));
        }
        s.push_str("S\n");
    }
    let labels: Vec<Text2d> = d.dimensions.iter().map(|dim| dim.label()).collect();
    for t in d.texts.iter().chain(&labels) {
        let size = t.height * k / CAPITALS;
        let width = width(&t.text, size);
        let shift = match t.align {
            Align::Left => 0.0,
            Align::Center => width / 2.0,
            Align::Right => width,
        };
        let x = (t.at[0] - lo[0]) * k - shift;
        let y = (t.at[1] - lo[1]) * k;
        let _ = writeln!(s, "BT /F1 {} Tf {} {} Td {} Tj ET", num(size), num(x), num(y), literal(&t.text));
    }
    s
}

/// Helvetica's widths of the printable ASCII characters (space to tilde), thousandths of an em.
const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, // space to /
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, // 0 to ?
    1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, // @ to O
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, // P to _
    333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, // ` to o
    556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584, // p to ~
];

/// How wide `text` is in Helvetica at `size`: a letter with an accent as its letter, other
/// characters as an average one.
pub(crate) fn width(text: &str, size: f64) -> f64 {
    let em: u32 = text
        .chars()
        .map(|c| {
            let base = unaccented(c);
            match base as u32 {
                0x20..=0x7E => u32::from(HELVETICA[base as usize - 0x20]),
                _ => 556,
            }
        })
        .sum();
    f64::from(em) / 1000.0 * size
}

/// The letter under an accent (the Latin-1 ones); other characters as they are.
fn unaccented(c: char) -> char {
    const FROM: &str = "ÀÁÂÃÄÅàáâãäåÈÉÊËèéêëÌÍÎÏìíîïÒÓÔÕÖòóôõöÙÚÛÜùúûüÝýÿÑñÇç";
    const TO: &str = "AAAAAAaaaaaaEEEEeeeeIIIIiiiiOOOOOoooooUUUUuuuuYyyNnCc";
    FROM.chars().position(|f| f == c).and_then(|i| TO.chars().nth(i)).unwrap_or(c)
}

/// A PDF text string of `text` in UTF-16BE, for the document's information (any language).
pub(crate) fn utf16(text: &str) -> String {
    let mut s = String::from("<FEFF");
    for unit in text.encode_utf16() {
        let _ = write!(s, "{unit:04X}");
    }
    s.push('>');
    s
}

/// The drawing as a PDF file.
pub fn pdf(d: &Drawing2d) -> Vec<u8> {
    let (lo, hi) = d.page();
    let k = d.paper_per_unit() * 1000.0 * POINTS;
    let (w, h) = (num_to((hi[0] - lo[0]) * k, 2), num_to((hi[1] - lo[1]) * k, 2));
    let stream = content(d);
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>"),
        format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
        format!("<< /Title {} /Producer (Open BIM Suite) >>", utf16(&d.name)),
    ];
    let mut out = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        let _ = write!(out, "{} 0 obj\n{object}\nendobj\n", i + 1);
    }
    let xref = out.len();
    let _ = write!(out, "xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for offset in offsets {
        let _ = write!(out, "{offset:010} 00000 n \n");
    }
    let _ = write!(out, "trailer\n<< /Size {} /Root 1 0 R /Info 6 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1);
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::A3_LANDSCAPE;
    use crate::drawing2d::{Align, Line2d, Pen, Space, Text2d};

    fn sheet() -> Drawing2d {
        Drawing2d {
            name: "A01 Plattegronden".into(),
            space: Space::Paper,
            paper: Some(A3_LANDSCAPE),
            lines: vec![Line2d { pen: Pen::Cut, a: [0.05, 0.05], b: [0.30, 0.05] }, Line2d { pen: Pen::Beyond, a: [0.05, 0.10], b: [0.30, 0.10] }],
            texts: vec![
                Text2d { pen: Pen::Text, at: [0.05, 0.2], height: 0.005, text: "A & <B> ë".into(), align: Align::Left },
                Text2d { pen: Pen::Text, at: [0.40, 0.28], height: 0.004, text: "(1) \\ 2 €".into(), align: Align::Right },
            ],
            dimensions: Vec::new(),
        }
    }

    fn text(bytes: &[u8]) -> String {
        bytes.iter().map(|&b| b as char).collect()
    }

    #[test]
    fn pdf_has_one_page_of_the_paper() {
        let s = text(&pdf(&sheet()));
        assert!(s.starts_with("%PDF-1.4"), "{s}");
        assert!(s.contains("/MediaBox [0 0 1190.55 841.89]"), "{s}");
        assert!(s.contains("/Count 1"), "{s}");
        assert!(s.trim_end().ends_with("%%EOF"), "{s}");
    }

    #[test]
    fn pdf_draws_the_lines_and_the_text() {
        let s = text(&pdf(&sheet()));
        assert_eq!(s.matches(" l\n").count(), 2, "{s}");
        // the pens' weights in points: 0.5 mm and 0.18 mm
        assert!(s.contains("1.417 w") && s.contains("0.51 w"), "{s}");
        assert!(s.contains("(A & <B> \\353) Tj"), "{s}");
        // brackets and backslashes escaped, the euro in WinAnsi
        assert!(s.contains("(\\(1\\) \\\\ 2 \\200) Tj"), "{s}");
        assert!(s.contains("/BaseFont /Helvetica") && s.contains("/WinAnsiEncoding"), "{s}");
    }

    #[test]
    fn the_title_is_utf16_and_a_character_outside_winansi_a_question_mark() {
        let mut d = sheet();
        d.name = "A01 € Ω".into();
        d.texts[0].text = "Ω".into();
        let s = text(&pdf(&d));
        // FEFF, then A01 € Ω in UTF-16BE
        assert!(s.contains("/Title <FEFF004100300031002020AC002003A9>"), "{s}");
        assert!(s.contains("(?) Tj"), "{s}");
    }

    #[test]
    fn helvetica_widths_measure_text() {
        // W 944, i 222, l 222 thousandths of an em
        assert!((width("Wil", 10.0) - 13.88).abs() < 1e-9);
        // accented letters as their letter, others as the average
        assert!((width("ë", 10.0) - width("e", 10.0)).abs() < 1e-9);
    }

    #[test]
    fn pdf_xref_offsets_point_at_objects() {
        let bytes = pdf(&sheet());
        let s = text(&bytes);
        let xref = s.rfind("\nxref\n").expect("xref") + 1;
        let start: usize = s[s.rfind("startxref\n").unwrap() + 10..].lines().next().unwrap().trim().parse().unwrap();
        assert_eq!(start, xref);
        let table: Vec<&str> = s[xref..].lines().skip(2).take_while(|l| l.len() == 18 || l.len() == 19).collect();
        assert!(table.len() >= 5, "{table:?}");
        for (n, entry) in table.iter().enumerate().skip(1) {
            let offset: usize = entry[..10].parse().unwrap();
            assert!(s[offset..].starts_with(&format!("{n} 0 obj")), "object {n} at {offset}");
        }
    }
}
