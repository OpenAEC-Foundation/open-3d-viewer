//! A 2D drawing as a vector PDF: one page (a sheet's paper, or what a view draws at its scale),
//! its lines with the pens' line weights, with installed Swiss condensed type embedded where specified.
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
    if c == '′' { return b'\''; }
    match c as u32 {
        0x2212 | 0x2010 | 0x2011 => b'-',
        0xfd3e => b'(',
        0xfd3f => b')',
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
struct DrawingFont {
    family: &'static str,
    bold: bool,
    resource: &'static str,
    name: &'static str,
    font: super::pdf_font::Font,
}

fn content(d: &Drawing2d, fonts: &[DrawingFont]) -> String {
    let (lo, _) = d.page();
    let k = d.paper_per_unit() * 1000.0 * POINTS;
    let at = |q: [f64; 2]| format!("{} {}", num((q[0] - lo[0]) * k), num((q[1] - lo[1]) * k));
    let mut s = String::from("1 J 1 j 0 G 0 g\n");
    for (i, image) in d.images.iter().enumerate() {
        let [a, b] = image.bounds;
        let [c, e] = image.clip;
        let _ = writeln!(s, "q {} {} {} re W n {} 0 0 {} {} cm /Im{} Do Q", at(c), num((e[0]-c[0])*k), num((e[1]-c[1])*k),
            num((b[0]-a[0])*k), num((b[1]-a[1])*k), at(a), i);
    }
    for polygon in &d.polygons {
        let Some((first, rest)) = polygon.points.split_first() else { continue };
        let [r, g, b] = polygon.color.map(|c| num(f64::from(c) / 255.0));
        let _ = writeln!(s, "{r} {g} {b} rg {} m", at(*first));
        for &point in rest { let _ = writeln!(s, "{} l", at(point)); }
        s.push_str("h f\n");
    }
    for pen in Pen::ALL {
        // the lines of a pen a width at a time: a view's kind of drawing gives what is cut and
        // what is seen beyond it a width of its own
        for (width, rgb, segments) in d.strokes(pen) {
            let [r, g, b] = rgb.map(|c| num(f64::from(c) / 255.0));
            let _ = writeln!(s, "{r} {g} {b} RG {} w", num(width * POINTS));
            for [a, b] in segments {
                let _ = writeln!(s, "{} m {} l", at(a), at(b));
            }
            s.push_str("S\n");
        }
    }
    let labels: Vec<Text2d> = d.dimensions.iter().map(|dim| dim.label()).collect();
    for t in d.texts.iter().chain(&labels) {
        let [r, g, b] = t.color.unwrap_or_default().map(|c| num(f64::from(c) / 255.0));
        let _ = writeln!(s, "{r} {g} {b} rg");
        let size = t.height * k / if t.family.is_some() { 1.0 } else { CAPITALS };
        let face = fonts.iter().find(|f| t.family.as_deref() == Some(f.family) && t.bold == f.bold);
        let (font, width) = match face {
            Some(f) => (f.resource, t.text.chars().map(|c| f64::from(f.font.widths[usize::from(win_ansi(c))])).sum::<f64>() * size / 1000.0),
            None => (if t.bold { "F2" } else { "F1" }, width(&t.text, size)),
        };
        let shift = match t.align {
            Align::Left => 0.0,
            Align::Center => width / 2.0,
            Align::Right => width,
        };
        // its start: back along its own direction from the point it stands on
        let (sin, cos) = t.angle.sin_cos();
        let x = (t.at[0] - lo[0]) * k - shift * cos;
        let y = (t.at[1] - lo[1]) * k - shift * sin;
        if t.angle == 0.0 {
            let _ = writeln!(s, "BT /{font} {} Tf {} {} Td {} Tj ET", num(size), num(x), num(y), literal(&t.text));
        } else {
            let _ = writeln!(s, "BT /{font} {} Tf {} {} {} {} {} {} Tm {} Tj ET", num(size), num(cos), num(sin), num(-sin), num(cos), num(x), num(y), literal(&t.text));
        }
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
    let choices: [(&str, bool, &str, &str, fn() -> Option<&'static [u8]>); 6] = [
        (crate::typeface::SWISS_CONDENSED, false, "F3", crate::typeface::SWISS_POSTSCRIPT, crate::typeface::swiss_condensed),
        (crate::typeface::ARIAL_NARROW, false, "F6", "ArialNarrow", crate::typeface::arial_narrow),
        ("Calibri", false, "F7", "Calibri", || crate::typeface::calibri(false)),
        ("Calibri", true, "F8", "Calibri-Bold", || crate::typeface::calibri(true)),
        ("Segoe UI", false, "F4", "SegoeUI", || crate::typeface::segoe_ui(false)),
        ("Segoe UI", true, "F5", "SegoeUI-Bold", || crate::typeface::segoe_ui(true)),
    ];
    let embedded: Vec<_> = choices.into_iter().filter_map(|(family, bold, resource, name, load)| {
        if !d.texts.iter().any(|t| t.family.as_deref() == Some(family) && t.bold == bold)
            && (bold || !d.dimensions.iter().any(|dim| dim.style.label_family().as_deref() == Some(family))) { return None; }
        let bytes = load()?;
        use ttf_parser::Permissions::{Editable, Installable, PreviewAndPrint};
        let face = ttf_parser::Face::parse(bytes, 0).ok()?;
        (face.is_outline_embedding_allowed() && matches!(face.permissions(), Some(Editable | Installable | PreviewAndPrint)))
            .then(|| DrawingFont { family, bold, resource, name, font: super::pdf_font::font(bytes) })
    }).collect();
    let stream = content(d, &embedded);
    let fonts = embedded.iter().enumerate().map(|(i,f)| format!("/{} {} 0 R", f.resource, 8 + d.images.len() + 3*i)).collect::<Vec<_>>().join(" ");
    let references = (0..d.images.len()).map(|i| format!("/Im{i} {} 0 R", i+8)).collect::<Vec<_>>().join(" ");
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] /Contents 4 0 R /Resources << /Font << /F1 5 0 R /F2 7 0 R {fonts} >> /XObject << {references} >> >> >>"),
        format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
        format!("<< /Title {} /Producer (Open BIM Suite) >>", utf16(&d.name)),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".to_string(),
    ];
    let mut objects = objects.into_iter().map(String::into_bytes).collect::<Vec<_>>();
    for image in &d.images {
        use std::io::Write as _;
        let (filter, stream) = if image.format == image::ImageFormat::Jpeg {
            ("DCTDecode", image.bytes.to_vec())
        } else {
            let rgba = image::load_from_memory_with_format(&image.bytes, image.format).expect("validated drawing image").into_rgba8();
            let mut encoded = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
            for px in rgba.pixels() {
                let a = u32::from(px[3]);
                let rgb = [0, 1, 2].map(|k| ((u32::from(px[k])*a + 255*(255-a) + 127)/255) as u8);
                encoded.write_all(&rgb).expect("in-memory stream");
            }
            ("FlateDecode", encoded.finish().expect("in-memory stream"))
        };
        let mut object = format!("<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /{filter} /Length {} >>\nstream\n", image.pixels[0], image.pixels[1], stream.len()).into_bytes();
        object.extend(stream);
        object.extend_from_slice(b"\nendstream");
        objects.push(object);
    }
    for f in &embedded { super::pdf_font::push_font(&mut objects, &f.font, f.name, if f.bold {120} else {80}); }
    use std::io::Write as _;
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        let _ = writeln!(out, "{} 0 obj", i+1);
        out.extend(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let _ = write!(out, "xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for offset in offsets { let _ = writeln!(out, "{offset:010} 00000 n "); }
    let _ = write!(out, "trailer\n<< /Size {} /Root 1 0 R /Info 6 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::A3_LANDSCAPE;
    use crate::drawing2d::{Align, Line2d, Pen, Space, Text2d};

    fn sheet() -> Drawing2d {
        Drawing2d {
            polygons: Vec::new(),
            images: Vec::new(),
            name: "A01 Plattegronden".into(),
            space: Space::Paper,
            paper: Some(A3_LANDSCAPE),
            lines: vec![Line2d::new(Pen::Cut, [0.05, 0.05], [0.30, 0.05]), Line2d::new(Pen::Beyond, [0.05, 0.10], [0.30, 0.10])],
            texts: vec![
                Text2d { family: None, bold: false, color: None, pen: Pen::Text, at: [0.05, 0.2], height: 0.005, text: "A & <B> ë".into(), align: Align::Left, angle: 0.0 },
                Text2d { family: None, bold: false, color: None, pen: Pen::Text, at: [0.40, 0.28], height: 0.004, text: "(1) \\ 2 €".into(), align: Align::Right, angle: 0.0 },
            ],
            dimensions: Vec::new(),
        }
    }

    fn text(bytes: &[u8]) -> String {
        bytes.iter().map(|&b| b as char).collect()
    }

    #[cfg(windows)]
    #[test]
    fn office_dimension_embeds_arial_narrow_at_nominal_point_size() {
        if crate::typeface::arial_narrow().is_none() { return; }
        let style = crate::document::DimensionStyle::from_ifcx(&serde_json::json!({"name":"3BM ronde stip","size":0.0025})).unwrap();
        let doc = &mut crate::document::Document::default();
        let level = doc.add_level("Ground", 0.0).unwrap();
        let plan = doc.add_plan(&doc.levels()[level].path.clone()).unwrap();
        let style_index = doc.set_dimension_style(style);
        doc.add_dimension(&plan,[0.,0.],[4.,0.]);
        assert!(doc.set_dimension_style_of(&plan,0,style_index));
        let sheet = doc.add_sheet("Foundation");
        doc.place_plan(&sheet,&plan);
        let drawing = crate::drawing2d::sheet_drawing(doc,&sheet,&|_| None,&|_| vec![],&|_| Default::default()).unwrap();
        let label = drawing.dimensions[0].label();
        assert_eq!(label.family.as_deref(), Some("Arial Narrow"));
        assert!((label.height*72./0.0254-6.8).abs()<1e-9);
        let rendered = text(&pdf(&drawing));
        assert!(rendered.contains("/BaseFont /ArialNarrow"));
        assert!(rendered.contains("BT /F6 6.8 Tf"));
    }

    #[cfg(windows)]
    #[test]
    fn installed_swiss_condensed_is_embedded_at_seven_points() {
        let Some(bytes) = crate::typeface::swiss_condensed() else { return };
        assert!(crate::typeface::Face::of(crate::typeface::SWISS_CONDENSED).installed());
        let mut page = sheet();
        page.texts[0].family = Some(crate::typeface::SWISS_CONDENSED.into());
        page.texts[0].height = 7.0 * 0.0254 / 72.0;
        let result = text(&pdf(&page));
        assert!(result.contains("/BaseFont /Swiss721BT-RomanCondensed"));
        assert!(result.contains("BT /F3 7 Tf"));
        assert!(result.contains(&format!("/Length1 {}", bytes.len())));
    }

    #[test]
    fn office_stamp_uses_distinct_embedded_regular_and_bold_resources() {
        if crate::typeface::segoe_ui(false).is_none() || crate::typeface::segoe_ui(true).is_none() { return; }
        let mut page = sheet();
        for (i,t) in page.texts.iter_mut().enumerate() {
            t.family = Some("Segoe UI".into());
            t.bold = i == 1;
            t.height = 0.00353;
        }
        let bytes = pdf(&page);
        let output = String::from_utf8_lossy(&bytes);
        assert!(output.contains("/BaseFont /SegoeUI ") && output.contains("/BaseFont /SegoeUI-Bold "));
        assert!(output.contains("BT /F4 10.006 Tf") && output.contains("BT /F5 10.006 Tf"));
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
    fn a_turned_text_is_written_with_its_matrix() {
        let mut d = sheet();
        // the same text level and from the left, and read from the right of the sheet, centred
        let level = Text2d { family: None, bold: false, color: None, pen: Pen::Text, at: [0.1, 0.1], height: 0.005, text: "HEA 200".into(), align: Align::Left, angle: 0.0 };
        d.texts = vec![level.clone(), Text2d { align: Align::Center, angle: std::f64::consts::FRAC_PI_2, ..level }];
        let s = text(&pdf(&d));
        let lines: Vec<&str> = s.lines().filter(|l| l.contains("(HEA 200) Tj")).collect();
        let place = |line: &str, op: &str| -> (f64, f64) {
            let before: Vec<&str> = line.split(op).next().unwrap().split_whitespace().collect();
            (before[before.len() - 2].parse().unwrap(), before[before.len() - 1].parse().unwrap())
        };
        let (x, y) = place(lines[0], " Td");
        // cos sin -sin cos, then its start: at the point's x, half its width under the point
        assert!(lines[1].contains(" 0 1 -1 0 "), "{}", lines[1]);
        let (tx, ty) = place(lines[1], " Tm");
        assert!((tx - x).abs() < 1e-6 && ty < y - 5.0 && ty > y - 40.0, "{lines:?}");
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
