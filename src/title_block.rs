use crate::document::Document;
use crate::drawing2d::{Align, Line2d, Pen, Text2d};

/// Title-block geometry in paper metres, shared by the canvas and drawing exports.
pub(crate) struct TitleBlock {
    pub owns_frame: bool,
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
        owns_frame: false,
        bounds: [[left, 0.010], [right, top]],
        lines: Vec::new(),
        texts: Vec::new(),
        logo: None,
        polygons: Vec::new(),
    };
    if style.key.eq_ignore_ascii_case("3bm") {
        office_stamp(&mut out, doc, sheet, style);
        return out;
    }
    let mut text = |s: &str, x: f64, y: f64, h: f64, width: f64| {
        if s.is_empty() {
            return;
        }
        let height = h.min(width / (s.chars().count() as f64 * 0.95));
        out.texts.push(Text2d {
            family: None,
            bold: false,
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
fn office_stamp(out: &mut TitleBlock, doc: &Document, sheet: &crate::document::Sheet, style: &bim_library::house::HouseStyle) {
    use bim_library::house::Field;
    // Physical dimensions, in mm from the lower right paper corner. The house width is
    // the straight 180 mm edge above the stamp, between its two 5 mm corner radii.
    let unit = (style.title_block.width / 180_000.0)
        .min((sheet.paper[0] - 0.02) / 280.0)
        .min((sheet.paper[1] - 0.03) / 110.0);
    let at = |x: f64, y: f64| [sheet.paper[0] + x * unit, y * unit];
    let purple = style.colour("primary").unwrap_or([53, 14, 53]);
    let accent = style.colour("accent").unwrap_or([69, 182, 168]);
    out.bounds = [at(-275., 0.), at(0., 98.)];
    out.owns_frame = true;
    office_frame(out, sheet.paper, unit, purple);
    let mut shape = |points: &[[f64; 2]], color| {
        out.polygons.push(crate::drawing2d::Polygon2d {
            points: points.iter().map(|p| at(p[0], p[1])).collect(),
            color,
        });
    };
    let mut banner = vec![[-62.8, 80.84], [-45.64, 98.]];
    banner.extend(quarter_arc([-195., 93.], 5., 90., 180.));
    banner.push([-200., 80.84]);
    shape(&banner, purple);
    for y in [61.57, 49.62] {
        shape(&[[-190.65, y + 0.065], [-62.8, y + 0.065], [-62.8, y - 0.065], [-190.65, y - 0.065]], accent);
    }
    shape(&[[-34.98, 51.64], [-51.35, 35.26], [-51.35, 31.93], [-44.40, 24.98], [-34.98, 34.40]], purple);
    shape(&[[-17.85, 33.30], [0., 15.45], [0., 32.50], [-7.95, 40.45], [-10.95, 40.45]], purple);
    shape(
        &[
            [-17.85, 33.30],
            [-17.85, 50.47],
            [-25.48, 58.10],
            [-28.51, 58.10],
            [-34.97, 51.64],
            [-34.97, 50.42],
        ],
        accent,
    );
    shape(&[[-17.71, 16.38], [-17.71, 33.44], [-33.99, 17.16], [-33.99, 14.54], [-27.02, 7.07]], accent);
    shape(&[[0., 0.], [0., 15.45], [-0.57, 16.02], [-16.59, 0.]], accent);
    let mut text = |s: &str, x, y, em: f64, width: f64, color, align| {
        if s.trim().is_empty() {
            return;
        }
        let bold = em >= 7.0 || matches!(s, "Projectnr:" | "Fase:" | "Auteur:");
        let family = if em >= 7.0 { &style.fonts.heading } else { &style.fonts.text };
        let advance = (family == "Segoe UI").then(|| crate::typeface::segoe_ui(bold)).flatten()
            .and_then(|bytes| ttf_parser::Face::parse(bytes, 0).ok())
            .map(|font| s.chars().filter_map(|c| font.glyph_hor_advance(font.glyph_index(c)?)).map(f64::from).sum::<f64>() / f64::from(font.units_per_em()))
            .unwrap_or_else(|| s.chars().count() as f64 * 0.55);
        let size = em.min(width / advance.max(0.01));
        out.texts.push(Text2d {
            family: Some(family.as_str().into()),
            bold,
            pen: Pen::Text,
            color: Some(color),
            at: at(x, y),
            height: size * unit,
            text: s.into(),
            align,
            angle: 0.,
        });
    };
    let l = Align::Left;
    text(&sheet.name, -190.75, 87.63, 7.23, 125., [255; 3], l);
    text(doc.title_value(sheet, Field::Project), -190.75, 72.68, 4.94, 127., purple, l);
    text(&doc.project_address, -190.75, 66.07, 4.94, 127., purple, l);
    text("Opdrachtgever:", -190.75, 54.35, 3.53, 25., purple, l);
    text(doc.title_value(sheet, Field::Client), -164.29, 54.35, 3.53, 99., purple, l);
    let office = &style.company;
    let address = format!("{} {}", office.postcode, office.city);
    let phone = if office.phone.is_empty() {
        String::new()
    } else {
        format!("Tel: {}", office.phone)
    };
    for (value, y) in [
        (&office.name, 90.31),
        (&office.address, 86.40),
        (&address, 82.50),
        (&phone, 74.70),
        (&office.email, 70.79),
        (&office.website, 66.89),
    ] {
        text(value, -40.08, y, 2.82, 39., purple, l);
    }
    let format = paper_format(sheet, style);
    let scale = scale_label(sheet);
    for (column, value_x, fields) in [
        (
            -190.75,
            -170.75,
            [
                ("Projectnr:", doc.title_value(sheet, Field::ProjectNumber), 41.77),
                ("Fase:", doc.title_value(sheet, Field::Phase), 37.74),
                ("Auteur:", doc.title_value(sheet, Field::DrawnBy), 33.45),
                ("Controle:", doc.title_value(sheet, Field::CheckedBy), 29.27),
                ("Revisie:", doc.title_value(sheet, Field::Revision), 25.06),
            ],
        ),
        (
            -116.60,
            -91.49,
            [
                ("Formaat:", format.as_str(), 42.),
                ("Schaal:", scale.as_str(), 37.76),
                ("1e datum:", doc.title_value(sheet, Field::Date), 33.53),
                ("Status:", doc.title_value(sheet, Field::Status), 29.27),
                ("Tekeningnr:", sheet.number.as_str(), 25.06),
            ],
        ),
    ] {
        for (label, value, y) in fields {
            if matches!(label, "Controle:" | "Revisie:") && value.is_empty() {
                continue;
            }
            text(label, column, y, 3.53, value_x - column - 1., purple, l);
            text(value, value_x, y, 3.53, if column < -150. { 52. } else { 38. }, purple, l);
        }
    }
    for (i, revision) in sheet.revisions.iter().enumerate() {
        let y = 92.24 - i as f64 * 15.;
        text(&format!("{}.", (b'A' + i as u8) as char), -206.22, y, 3.53, 5., purple, l);
        text(&revision.date, -208.5, y, 3.53, 64., purple, Align::Right);
        text(&revision.description, -208.5, y - 4.70, 3.53, 64., purple, Align::Right);
    }
    out.logo = fitted_logo(style.logo.as_deref(), [at(-191.8, 8.6), at(-153.1, 24.55)]);
}

/// Six chords per quadrant match the plotted 5 mm corners and remain vector lines at every zoom.
fn quarter_arc([cx, cy]: [f64; 2], radius: f64, start: f64, end: f64) -> Vec<[f64; 2]> {
    (0..=6)
        .map(|i| {
            let a = (start + (end - start) * f64::from(i) / 6.).to_radians();
            [cx + radius * a.cos(), cy + radius * a.sin()]
        })
        .collect()
}

fn office_frame(out: &mut TitleBlock, [w, h]: [f64; 2], unit: f64, color: [u8; 3]) {
    let [w, h] = [w / unit, h / unit];
    let corners = [
        ([15., 15.], 180., 270.),
        ([w - 205., 15.], 270., 360.),
        ([w - 195., 93.], 180., 90.),
        ([w - 15., 103.], 270., 360.),
        ([w - 15., h - 15.], 0., 90.),
        ([15., h - 15.], 90., 180.),
    ];
    let arcs = corners.map(|(c, a, b)| quarter_arc(c, 5., a, b));
    let mut segment = |a: [f64; 2], b: [f64; 2], width| {
        out.lines.push(Line2d {
            pen: Pen::Frame,
            a: a.map(|v| v * unit),
            b: b.map(|v| v * unit),
            width,
            color: Some(color),
        });
    };
    for (i, arc) in arcs.iter().enumerate() {
        let previous = &arcs[(i + arcs.len() - 1) % arcs.len()];
        segment(previous[6], arc[0], if i == 2 || i == 3 { 0.25 } else { 0.13 });
        for side in arc.windows(2) {
            segment(side[0], side[1], 0.25);
        }
    }
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
