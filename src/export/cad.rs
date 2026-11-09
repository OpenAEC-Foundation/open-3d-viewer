//! A 2D drawing as a CAD drawing in the shared OpenCAD drawing model (`opencaddocument`, MPL-2.0),
//! which writes it as DWG and DXF through its acadrust codec and as IFCCAD, the open 2D drawing
//! profile on IFCX, through ifccad. Millimetres: a view 1:1, a sheet at paper size. One layer per
//! pen, with the pen's line weight.

use crate::drawing2d::{Align, Drawing2d, Pen, Space};
use opencaddocument::entities::{Dimension, DimensionAligned, Line, Text, TextHorizontalAlignment};
use opencaddocument::tables::{DimStyle, Layer};
use opencaddocument::{Document, Dwg, Dxf, EntityType, Format, Ifcx, LineWeight, Vector3};

/// Millimetres per metre.
const MM: f64 = 1000.0;
/// The dimension style of the drawing's dimensions.
const STYLE: &str = "O3V";
/// `INSUNITS` of millimetres.
const MILLIMETRES: i16 = 4;

/// A drawing point in millimetres.
fn mm(q: [f64; 2]) -> Vector3 {
    Vector3::new(q[0] * MM, q[1] * MM, 0.0)
}

/// The drawing in the shared CAD model.
pub fn cad_document(d: &Drawing2d) -> Document {
    let mut doc = Document::new();
    doc.header.insertion_units = MILLIMETRES;
    for pen in Pen::ALL {
        let mut layer = Layer::new(pen.name());
        layer.handle = doc.allocate_handle();
        layer.line_weight = LineWeight::Value((pen.weight_mm() * 100.0).round() as i16);
        let _ = doc.layers.add(layer);
    }
    // 2.5 mm text and ticks on paper, in the drawing's millimetres
    let unit = MM / d.paper_per_unit();
    let mut style = DimStyle::new(STYLE);
    style.handle = doc.allocate_handle();
    style.dimtxt = 0.0025 * unit;
    style.dimasz = 0.0025 * unit;
    style.dimtsz = 0.0012 * unit;
    style.dimexo = 0.0015 * unit;
    style.dimexe = 0.0015 * unit;
    style.dimgap = 0.001 * unit;
    style.dimdec = 0;
    style.dimtad = 1;
    style.dimtih = false;
    style.dimtoh = false;
    let _ = doc.dim_styles.add(style);

    for l in &d.lines {
        let mut line = Line::from_points(mm(l.a), mm(l.b));
        line.common.layer = l.pen.name().into();
        let _ = doc.add_entity(EntityType::Line(line));
    }
    for t in &d.texts {
        let mut text = Text::with_value(t.text.clone(), mm(t.at)).with_height(t.height * MM);
        text.common.layer = t.pen.name().into();
        let align = match t.align {
            Align::Left => None,
            Align::Center => Some(TextHorizontalAlignment::Center),
            Align::Right => Some(TextHorizontalAlignment::Right),
        };
        if let Some(align) = align {
            text.horizontal_alignment = align;
            text.alignment_point = Some(mm(t.at));
        }
        let _ = doc.add_entity(EntityType::Text(text));
    }
    for dim in &d.dimensions {
        let Some((_, on_line)) = dim.line() else { continue };
        let mut aligned = DimensionAligned::new(mm(dim.from), mm(dim.to));
        aligned.definition_point = mm(on_line);
        let base = &mut aligned.base;
        base.style_name = STYLE.into();
        base.common.layer = Pen::Dimension.name().into();
        base.text_middle_point = mm(dim.label().at);
        // on paper a dimension measures the model, not the paper
        if d.space == Space::Paper {
            base.set_text_override(Some(dim.label().text));
        }
        let _ = doc.add_entity(EntityType::Dimension(Box::new(Dimension::Aligned(aligned))));
    }
    doc
}

/// Writes the drawing in `format`.
fn write(d: &Drawing2d, format: &dyn Format) -> Result<Vec<u8>, String> {
    format.write(&cad_document(d)).map(|(bytes, _losses)| bytes).map_err(|e| format!("Cannot write {}: {e}", format.name()))
}

/// The drawing as a DWG file.
pub fn dwg(d: &Drawing2d) -> Result<Vec<u8>, String> {
    write(d, &Dwg)
}

/// The drawing as a DXF file.
pub fn dxf(d: &Drawing2d) -> Result<Vec<u8>, String> {
    write(d, &Dxf)
}

/// The drawing as an IFCCAD drawing (IFCX).
pub fn ifccad(d: &Drawing2d) -> Result<Vec<u8>, String> {
    write(d, &Ifcx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawing2d::{Align, Dim2d, Line2d, Pen, Space, Text2d};
    use opencaddocument::{EntityType, Format, LineWeight};

    /// Two cut walls of 8 m and 5 m, a line beyond, a text and an 8 m dimension, at 1:100.
    fn drawing() -> Drawing2d {
        Drawing2d {
            name: "Begane grond".into(),
            space: Space::Model { scale: 100.0 },
            paper: None,
            lines: vec![
                Line2d { pen: Pen::Cut, a: [0.0, 0.0], b: [8.0, 0.0] },
                Line2d { pen: Pen::Cut, a: [8.0, 0.0], b: [8.0, 5.0] },
                Line2d { pen: Pen::Beyond, a: [1.0, 1.0], b: [7.0, 1.0] },
            ],
            texts: vec![Text2d { pen: Pen::Text, at: [1.0, 2.0], height: 0.25, text: "Kamer ë".into(), align: Align::Left }],
            dimensions: vec![Dim2d { from: [0.0, 0.0], to: [8.0, 0.0], value: 8.0, offset: 0.6, text_height: 0.25 }],
        }
    }

    /// The lines on a layer and the texts of a CAD drawing.
    fn contents(doc: &opencaddocument::Document, layer: &str) -> (Vec<(f64, f64, f64, f64)>, Vec<String>) {
        let mut lines = Vec::new();
        let mut texts = Vec::new();
        for e in doc.entities() {
            match e {
                EntityType::Line(l) if l.common.layer == layer => lines.push((l.start.x, l.start.y, l.end.x, l.end.y)),
                EntityType::Text(t) => texts.push(t.value.clone()),
                _ => {}
            }
        }
        (lines, texts)
    }

    #[test]
    fn units_are_millimetres() {
        let doc = cad_document(&drawing());
        assert_eq!(doc.header.insertion_units, 4);
        let (cut, _) = contents(&doc, "O3V-Cut");
        assert!(cut.contains(&(0.0, 0.0, 8000.0, 0.0)), "{cut:?}");
    }

    #[test]
    fn a_drawing_round_trips_through_dwg_and_dxf() {
        let d = drawing();
        for (bytes, format) in [(dwg(&d).unwrap(), &opencaddocument::Dwg as &dyn Format), (dxf(&d).unwrap(), &opencaddocument::Dxf)] {
            let (read, _) = format.read(&bytes).unwrap();
            let (cut, texts) = contents(&read, "O3V-Cut");
            let (beyond, _) = contents(&read, "O3V-Beyond");
            assert_eq!((cut.len(), beyond.len()), (2, 1), "{}", format.name());
            assert!(texts.iter().any(|t| t == "Kamer ë"), "{}: {texts:?}", format.name());
            let layer = read.layers.get("O3V-Cut").expect("the cut layer");
            assert_eq!(layer.line_weight, LineWeight::Value(50), "{}", format.name());
            assert!(read.entities().any(|e| matches!(e, EntityType::Dimension(_))), "{}", format.name());
        }
    }

    #[test]
    fn the_ifccad_drawing_reads_back() {
        let bytes = ifccad(&drawing()).unwrap();
        assert!(std::str::from_utf8(&bytes).is_ok_and(|s| s.contains("\"header\"")), "IFCX JSON");
        let (read, _) = opencaddocument::Ifcx.read(&bytes).unwrap();
        let (cut, texts) = contents(&read, "O3V-Cut");
        assert_eq!(cut.len(), 2);
        assert!(texts.iter().any(|t| t == "Kamer ë"), "{texts:?}");
    }
}
