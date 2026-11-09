//! A 2D drawing as a vector PDF: one page (a sheet's paper, or what a view draws at its scale),
//! its lines with the pens' line weights and its texts in Helvetica. A small writer of its own:
//! PDF 1.4, no compression, WinAnsi text (a character outside it becomes `?`).

use crate::drawing2d::{Align, Drawing2d, Pen, Text2d};
use std::fmt::Write;

/// Points per millimetre.
const POINTS: f64 = 72.0 / 25.4;
/// Helvetica's capitals per em, and about how wide a character is per em.
const CAPITALS: f64 = 0.718;
const CHARACTER: f64 = 0.55;

/// A number with at most `decimals` decimals and no trailing zeros.
fn num_to(v: f64, decimals: usize) -> String {
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
fn literal(text: &str) -> String {
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
        let width = t.text.chars().count() as f64 * size * CHARACTER;
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
        format!("<< /Title {} /Producer (Open 3D Viewer Studio) >>", literal(&d.name)),
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
