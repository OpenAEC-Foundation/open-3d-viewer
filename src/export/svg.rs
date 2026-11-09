//! A 2D drawing as SVG, and as PNG through that SVG: rendered with resvg (MPL-2.0, already built
//! for the app's icons) with the app's own fonts. Millimetres of paper, black on white, each pen
//! one path with its line weight.

use crate::drawing2d::{Align, Drawing2d, Pen, Text2d};
use resvg::{tiny_skia, usvg};
use std::fmt::Write;
use std::sync::Arc;

/// The longest side a PNG gets (pixels): an A0 sheet at 300 dpi would be 14 000.
pub const MAX_PIXELS: u32 = 10_000;
/// Capitals' height per em of the font (Inter).
const CAPITALS: f64 = 0.72;

/// A number with at most three decimals and no trailing zeros.
fn num(v: f64) -> String {
    let s = format!("{:.3}", (v * 1000.0).round() / 1000.0);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".to_string() } else { s.to_string() }
}

/// Text for XML.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Width and height of the drawing's page in millimetres of paper.
fn page_mm(d: &Drawing2d) -> (f64, f64) {
    let (lo, hi) = d.page();
    let k = d.paper_per_unit() * 1000.0;
    ((hi[0] - lo[0]) * k, (hi[1] - lo[1]) * k)
}

/// The drawing as an SVG document, its page in millimetres of paper.
pub fn svg(d: &Drawing2d) -> String {
    let (lo, hi) = d.page();
    let k = d.paper_per_unit() * 1000.0;
    let (w, h) = page_mm(d);
    let at = |q: [f64; 2]| (num((q[0] - lo[0]) * k), num((hi[1] - q[1]) * k));
    let mut s = String::new();
    let _ = write!(s, r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}mm" height="{h}mm" viewBox="0 0 {w} {h}">"#, w = num(w), h = num(h));
    let _ = write!(s, r##"<rect width="{}" height="{}" fill="#fff"/>"##, num(w), num(h));
    for pen in Pen::ALL {
        let mut path = String::new();
        let mut segment = |a: [f64; 2], b: [f64; 2]| {
            let ((ax, ay), (bx, by)) = (at(a), at(b));
            let _ = write!(path, "M{ax} {ay}L{bx} {by}");
        };
        for l in d.lines.iter().filter(|l| l.pen == pen) {
            segment(l.a, l.b);
        }
        if pen == Pen::Dimension {
            for [a, b] in d.dimensions.iter().flat_map(|dim| dim.lines()) {
                segment(a, b);
            }
        }
        if !path.is_empty() {
            let _ = write!(s, r##"<path d="{path}" fill="none" stroke="#000" stroke-width="{}" stroke-linecap="round"/>"##, num(pen.weight_mm()));
        }
    }
    let labels: Vec<Text2d> = d.dimensions.iter().map(|dim| dim.label()).collect();
    for t in d.texts.iter().chain(&labels) {
        let (x, y) = at(t.at);
        let anchor = match t.align {
            Align::Left => "start",
            Align::Center => "middle",
            Align::Right => "end",
        };
        let size = num(t.height * k / CAPITALS);
        let _ = write!(s, r##"<text x="{x}" y="{y}" font-family="Inter, sans-serif" font-size="{size}" text-anchor="{anchor}" fill="#000">{}</text>"##, escape(&t.text));
    }
    s.push_str("</svg>");
    s
}

/// The drawing as a PNG image of its page at `dpi`, its longer side at most [`MAX_PIXELS`].
pub fn png(d: &Drawing2d, dpi: f64) -> Result<Vec<u8>, String> {
    let mut fonts = usvg::fontdb::Database::new();
    for bytes in openaec_ui::fonts::FILES {
        fonts.load_font_data(bytes.to_vec());
    }
    let options = usvg::Options { fontdb: Arc::new(fonts), ..usvg::Options::default() };
    let tree = usvg::Tree::from_str(&svg(d), &options).map_err(|e| format!("Cannot draw the drawing: {e}"))?;
    let (w, h) = page_mm(d);
    let mut per_mm = dpi / 25.4;
    let longest = w.max(h) * per_mm;
    if longest > MAX_PIXELS as f64 {
        per_mm *= MAX_PIXELS as f64 / longest;
    }
    let (pw, ph) = (((w * per_mm).round() as u32).max(1), ((h * per_mm).round() as u32).max(1));
    let mut pixmap = tiny_skia::Pixmap::new(pw, ph).ok_or("The image is too large")?;
    pixmap.fill(tiny_skia::Color::WHITE);
    let size = tree.size();
    let transform = tiny_skia::Transform::from_scale(pw as f32 / size.width(), ph as f32 / size.height());
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, pw, ph);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(pixmap.data()).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{A3_LANDSCAPE, PAPERS};
    use crate::drawing2d::{Align, Line2d, Pen, Space, Text2d};

    fn sheet(paper: [f64; 2]) -> Drawing2d {
        Drawing2d {
            name: "A01 Plattegronden".into(),
            space: Space::Paper,
            paper: Some(paper),
            lines: vec![Line2d { pen: Pen::Cut, a: [0.05, 0.05], b: [0.30, 0.05] }, Line2d { pen: Pen::Beyond, a: [0.05, 0.10], b: [0.30, 0.10] }],
            texts: vec![Text2d { pen: Pen::Text, at: [0.05, 0.2], height: 0.005, text: "A & <B> ë".into(), align: Align::Left }],
            dimensions: Vec::new(),
        }
    }

    /// Width, height and RGBA pixels of a PNG.
    fn decode(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
        let mut reader = ::png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
        let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buffer).unwrap();
        buffer.truncate(info.buffer_size());
        (info.width, info.height, buffer)
    }

    #[test]
    fn svg_has_a_path_per_pen_and_escapes_text() {
        let s = svg(&sheet(A3_LANDSCAPE));
        assert!(s.starts_with("<svg") || s.starts_with("<?xml"), "{s}");
        assert!(s.contains("viewBox=\"0 0 420 297\""), "{s}");
        assert!(s.contains("stroke-width=\"0.5\"") && s.contains("stroke-width=\"0.18\""), "{s}");
        assert!(s.contains("A &amp; &lt;B&gt; ë"), "{s}");
        // the paper's y goes up, the SVG's down: the line at 5 cm stands 247 mm from the top
        assert!(s.contains("M50 247L300 247"), "{s}");
    }

    #[test]
    fn png_is_the_page_at_200_dpi_and_not_blank() {
        let (w, h, pixels) = decode(&png(&sheet(A3_LANDSCAPE), 200.0).unwrap());
        assert_eq!((w, h), (3307, 2339));
        let dark = pixels.chunks(4).filter(|p| p[0] < 100).count();
        assert!(dark > 1000, "{dark} dark pixels");
        assert_eq!(&pixels[..4], &[255, 255, 255, 255], "white paper");
    }

    #[test]
    fn a0_png_is_capped() {
        let (w, h, _) = decode(&png(&sheet(PAPERS[4].1), 300.0).unwrap());
        assert_eq!(w, MAX_PIXELS);
        assert!(h < MAX_PIXELS && h > 7000, "{h}");
    }
}
