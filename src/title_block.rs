use crate::document::Document;
use crate::drawing2d::{Align, Line2d, Pen, Text2d};

/// Title-block geometry in paper metres, shared by the canvas and drawing exports.
pub(crate) struct TitleBlock {
    pub bounds: [[f64; 2]; 2],
    pub lines: Vec<Line2d>,
    pub texts: Vec<Text2d>,
    pub logo: Option<crate::raster::PlacedImage>,
    pub polygons: Vec<crate::drawing2d::Polygon2d>,
}

pub(crate) fn title_block(doc: &Document, sheet: &crate::document::Sheet, style: &bim_library::house::HouseStyle) -> TitleBlock {
    use bim_library::house::Field;
    let block = &style.title_block;
    let right = sheet.paper[0] - 0.010;
    let top = 0.010 + block.height() / 1000.0;
    let left = right - block.width / 1000.0;
    let mut out = TitleBlock {
        bounds: [[left, 0.010], [right, top]],
        lines: Vec::new(),
        texts: Vec::new(),
        logo: None,
        polygons: Vec::new(),
    };
    if style.key.eq_ignore_ascii_case("3bm") {
        office_stamp(&mut out, doc, sheet, style, left, top);
        return out;
    }
    let mut text = |s: &str, x: f64, y: f64, h: f64, width: f64| {
        if s.is_empty() {
            return;
        }
        let height = h.min(width / (s.chars().count() as f64 * 0.95));
        out.texts.push(Text2d {
            color: None,
            pen: Pen::Text,
            at: [x, y],
            height,
            text: s.to_owned(),
            align: Align::Left,
            angle: 0.0,
        });
    };
    for (field, [x, y, w, h]) in block.cells() {
        let (x, y, w, h) = (left + x / 1000.0, top - (y + h) / 1000.0, w / 1000.0, h / 1000.0);
        let corners = [[x, y], [x + w, y], [x + w, y + h], [x, y + h]];
        for i in 0..4 {
            out.lines.push(Line2d::new(Pen::Frame, corners[i], corners[(i + 1) % 4]));
        }
        let pad = 0.0015_f64.min(w / 6.0).min(h / 6.0);
        let width = w - 2.0 * pad;
        let address = &style.company;
        if let Some((caption, _)) = field.label() {
            text(caption, x + pad, y + h - pad - h.min(0.008) * 0.2, 0.0016_f64.min(h * 0.2), width);
        }
        let value = match field {
            Field::Logo => {
                out.logo = fitted_logo(style.logo.as_deref(), [[x + pad, y + pad], [x + w - pad, y + h - pad]]);
                continue;
            }
            Field::Company => address.name.clone(),
            Field::Address => {
                let lines = [
                    address.address.clone(),
                    format!("{} {}", address.postcode, address.city).trim().to_owned(),
                    address.website.clone(),
                ];
                for (i, line) in lines.iter().enumerate() {
                    text(line, x + pad, y + h * (0.68 - 0.23 * i as f64), 0.0025_f64.min(h * 0.17), width);
                }
                continue;
            }
            Field::Format => paper_format(sheet, style),
            Field::Scale => scale_label(sheet),
            _ => doc.title_value(sheet, field).to_owned(),
        };
        text(&value, x + pad, y + pad, 0.003_f64.min(h * 0.36), width);
    }
    out
}

/// Decode an office logo once and preserve its aspect ratio inside the allotted paper rectangle.
fn fitted_logo(uri: Option<&str>, [lo, hi]: [[f64; 2]; 2]) -> Option<crate::raster::PlacedImage> {
    use crate::raster::PlacedImage;
    use base64::{engine::general_purpose::STANDARD, Engine};
    use std::cell::RefCell;
    thread_local! { static LAST: RefCell<Option<(String, PlacedImage)>> = const { RefCell::new(None) }; }
    let uri = uri?;
    LAST.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.as_ref().is_none_or(|(known, _)| known != uri) {
            let (mime, data) = uri.split_once(',')?;
            if !matches!(mime, "data:image/png;base64" | "data:image/jpeg;base64") || data.len() > 44 * 1024 * 1024 {
                return None;
            }
            let image = PlacedImage::new(STANDARD.decode(data).ok()?, [[0., 0.], [1., 1.]]).ok()?;
            *cache = Some((uri.to_owned(), image));
        }
        let image = &cache.as_ref()?.1;
        let [px, py] = image.pixels.map(f64::from);
        let scale = ((hi[0] - lo[0]) / px).min((hi[1] - lo[1]) / py);
        let (w, h) = (px * scale, py * scale);
        let (x, y) = ((lo[0] + hi[0] - w) / 2.0, (lo[1] + hi[1] - h) / 2.0);
        image.placed(None, |q| [x + q[0] * w, y + q[1] * h])
    })
}

/// Office stamp in its house colours and dimensions. All lettering remains drawing text.
fn office_stamp(out: &mut TitleBlock, doc: &Document, sheet: &crate::document::Sheet, style: &bim_library::house::HouseStyle, left: f64, top: f64) {
    use bim_library::house::Field;
    let w = style.title_block.width / 1000.0;
    let h = style.title_block.height() / 1000.0;
    let at = |x: f64, y: f64| [left + x * w, top - y * h];
    let purple = style.colour("primary").unwrap_or([50, 17, 51]);
    let accent = style.colour("accent").unwrap_or([69, 180, 167]);
    let mut shape = |points: &[[f64; 2]], color| {
        out.polygons.push(crate::drawing2d::Polygon2d {
            points: points.iter().map(|p| at(p[0], p[1])).collect(),
            color,
        });
    };
    shape(&[[0., 0.], [0.78, 0.], [0.69, 0.18], [0., 0.18]], purple);
    for y in [0.34, 0.46] {
        shape(&[[0.035, y], [0.685, y], [0.685, y + 0.002], [0.035, y + 0.002]], accent);
    }
    // The folded ribbon at the lower right, independent of the office logo supplied as an image.
    shape(&[[0.76, 0.64], [0.82, 0.52], [0.82, 0.66], [0.76, 0.78]], purple);
    shape(&[[0.82, 0.52], [0.85, 0.46], [0.91, 0.58], [0.91, 0.70]], accent);
    shape(&[[0.91, 0.70], [0.95, 0.62], [1.0, 0.72], [1.0, 0.88]], purple);
    shape(&[[0.82, 0.88], [0.91, 0.70], [0.91, 0.84], [0.85, 0.96]], accent);

    let mut text = |s: &str, x: f64, y: f64, size: f64, width: f64, color: [u8; 3]| {
        if s.trim().is_empty() {
            return;
        }
        let height = (size * h).min(width * w / (s.chars().count() as f64 * 0.95));
        out.texts.push(Text2d {
            pen: Pen::Text,
            color: Some(color),
            at: at(x, y),
            height,
            text: s.to_owned(),
            align: Align::Left,
            angle: 0.0,
        });
    };
    text(&sheet.name, 0.035, 0.12, 0.068, 0.62, [255, 255, 255]);
    text(doc.title_value(sheet, Field::Project), 0.035, 0.285, 0.05, 0.65, purple);
    text("Opdrachtgever", 0.035, 0.415, 0.03, 0.13, purple);
    text(doc.title_value(sheet, Field::Client), 0.175, 0.415, 0.033, 0.51, purple);
    let office = &style.company;
    let address = format!("{} {}", office.postcode, office.city);
    for (i, line) in [&office.name, &office.address, &address, &office.phone, &office.email, &office.website]
        .iter()
        .enumerate()
    {
        text(line, 0.805, 0.06 + i as f64 * 0.043, 0.026, 0.193, purple);
    }
    let format = paper_format(sheet, style);
    let scale = scale_label(sheet);
    for (column, fields) in [
        (
            0.035,
            [
                ("Projectnr", doc.title_value(sheet, Field::ProjectNumber)),
                ("Fase", doc.title_value(sheet, Field::Phase)),
                ("Auteur", doc.title_value(sheet, Field::DrawnBy)),
                ("Controle", doc.title_value(sheet, Field::CheckedBy)),
                ("Revisie", doc.title_value(sheet, Field::Revision)),
            ],
        ),
        (
            0.415,
            [
                ("Formaat", format.as_str()),
                ("Schaal", scale.as_str()),
                ("Datum", doc.title_value(sheet, Field::Date)),
                ("Status", doc.title_value(sheet, Field::Status)),
                ("Tekeningnr", sheet.number.as_str()),
            ],
        ),
    ] {
        for (i, (caption, value)) in fields.iter().enumerate() {
            let y = 0.54 + i as f64 * 0.055;
            text(caption, column, y, 0.029, 0.112, purple);
            text(value, column + 0.12, y, 0.032, if column < 0.1 { 0.24 } else { 0.19 }, purple);
        }
    }
    out.logo = fitted_logo(style.logo.as_deref(), [at(0.035, 0.975), at(0.24, 0.80)]);
}

fn paper_format(sheet: &crate::document::Sheet, style: &bim_library::house::HouseStyle) -> String {
    style
        .sheets
        .iter()
        .find(|s| (s.width - sheet.paper[0] * 1000.0).abs() < 0.1 && (s.height - sheet.paper[1] * 1000.0).abs() < 0.1)
        .map_or_else(|| format!("{:.0} × {:.0}", sheet.paper[0] * 1000.0, sheet.paper[1] * 1000.0), |s| s.key.clone())
}

fn scale_label(sheet: &crate::document::Sheet) -> String {
    match sheet.views.first() {
        Some(first) if sheet.views.iter().all(|v| v.scale == first.scale) => format!("1:{}", first.scale),
        Some(_) => "div.".to_owned(),
        None => String::new(),
    }
}
