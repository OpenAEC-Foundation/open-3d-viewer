//! The formats a view's or sheet's drawing ([`crate::drawing2d`]) is exported in, and their
//! writers.

use crate::drawing2d::Drawing2d;
use crate::export::{cad, pdf, svg};

/// A PNG's resolution (dots per inch).
pub const PNG_DPI: f64 = 200.0;

/// A format to export a drawing in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawingFormat {
    Dwg,
    Dxf,
    /// IFCCAD, the open 2D drawing profile on IFCX.
    Ifccad,
    Pdf,
    Png,
    Svg,
}

impl DrawingFormat {
    pub const ALL: [DrawingFormat; 6] = [DrawingFormat::Dwg, DrawingFormat::Dxf, DrawingFormat::Ifccad, DrawingFormat::Pdf, DrawingFormat::Png, DrawingFormat::Svg];

    pub fn name(self) -> &'static str {
        match self {
            DrawingFormat::Dwg => "DWG",
            DrawingFormat::Dxf => "DXF",
            DrawingFormat::Ifccad => "IFCCAD",
            DrawingFormat::Pdf => "PDF",
            DrawingFormat::Png => "PNG",
            DrawingFormat::Svg => "SVG",
        }
    }

    /// The file extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            DrawingFormat::Dwg => "dwg",
            DrawingFormat::Dxf => "dxf",
            DrawingFormat::Ifccad => "ifcx",
            DrawingFormat::Pdf => "pdf",
            DrawingFormat::Png => "png",
            DrawingFormat::Svg => "svg",
        }
    }

    /// The MIME type, for the browser's download.
    pub fn mime(self) -> &'static str {
        match self {
            DrawingFormat::Dwg => "image/vnd.dwg",
            DrawingFormat::Dxf => "image/vnd.dxf",
            DrawingFormat::Ifccad => "application/json",
            DrawingFormat::Pdf => "application/pdf",
            DrawingFormat::Png => "image/png",
            DrawingFormat::Svg => "image/svg+xml",
        }
    }
}

/// The drawing as a file in `format`.
pub fn write(d: &Drawing2d, format: DrawingFormat) -> Result<Vec<u8>, String> {
    match format {
        DrawingFormat::Dwg => cad::dwg(d),
        DrawingFormat::Dxf => cad::dxf(d),
        DrawingFormat::Ifccad => cad::ifccad(d),
        DrawingFormat::Pdf => Ok(pdf::pdf(d)),
        DrawingFormat::Png => svg::png(d, PNG_DPI),
        DrawingFormat::Svg => Ok(svg::svg(d).into_bytes()),
    }
}

/// The file name offered for the drawing: its name, without what a file name cannot have, and
/// the format's extension.
pub fn file_name(d: &Drawing2d, format: DrawingFormat) -> String {
    let name: String = d.name.trim().chars().map(|c| if r#"/\:*?"<>|"#.contains(c) || c.is_control() { '-' } else { c }).collect();
    let name = if name.is_empty() { "drawing" } else { name.as_str() };
    format!("{name}.{}", format.extension())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::A3_LANDSCAPE;
    use crate::drawing2d::{Line2d, Pen, Space};

    fn sheet(name: &str) -> Drawing2d {
        Drawing2d {
            name: name.into(),
            space: Space::Paper,
            paper: Some(A3_LANDSCAPE),
            lines: vec![Line2d { pen: Pen::Cut, a: [0.05, 0.05], b: [0.30, 0.05] }],
            texts: Vec::new(),
            dimensions: Vec::new(),
        }
    }

    #[test]
    fn every_format_writes_a_file_with_its_signature() {
        let d = sheet("A01 Plattegronden");
        let starts = |format: DrawingFormat| write(&d, format).unwrap();
        assert!(starts(DrawingFormat::Dwg).starts_with(b"AC10"), "DWG");
        assert!(String::from_utf8_lossy(&starts(DrawingFormat::Dxf)).contains("SECTION"), "DXF");
        assert!(starts(DrawingFormat::Ifccad).starts_with(b"{"), "IFCCAD");
        assert!(starts(DrawingFormat::Pdf).starts_with(b"%PDF"), "PDF");
        assert!(starts(DrawingFormat::Png).starts_with(b"\x89PNG"), "PNG");
        assert!(starts(DrawingFormat::Svg).starts_with(b"<svg"), "SVG");
    }

    #[test]
    fn formats_have_their_name_extension_and_type() {
        assert_eq!(DrawingFormat::ALL.map(DrawingFormat::name), ["DWG", "DXF", "IFCCAD", "PDF", "PNG", "SVG"]);
        assert_eq!(DrawingFormat::ALL.map(DrawingFormat::extension), ["dwg", "dxf", "ifcx", "pdf", "png", "svg"]);
        assert_eq!(DrawingFormat::Pdf.mime(), "application/pdf");
        assert_eq!(DrawingFormat::Svg.mime(), "image/svg+xml");
    }

    #[test]
    fn file_names_keep_the_name_without_characters_files_cannot_have() {
        assert_eq!(file_name(&sheet("A01 Plattegronden"), DrawingFormat::Dwg), "A01 Plattegronden.dwg");
        assert_eq!(file_name(&sheet("a/b:c*?"), DrawingFormat::Pdf), "a-b-c--.pdf");
        assert_eq!(file_name(&sheet("  "), DrawingFormat::Png), "drawing.png");
    }
}
