use crate::document::Document;
use crate::drawing2d::{Align, Line2d, Pen, Text2d};

/// Title-block geometry in paper metres, shared by the canvas and drawing exports.
pub(crate) struct TitleBlock {
    pub lines: Vec<Line2d>,
    pub texts: Vec<Text2d>,
    pub logo_box: Option<[[f64; 2]; 2]>,
}

pub(crate) fn title_block(
    doc: &Document,
    sheet: &crate::document::Sheet,
    style: &bim_library::house::HouseStyle,
) -> TitleBlock {
    use bim_library::house::Field;
    let block = &style.title_block;
    let right = sheet.paper[0] - 0.010;
    let top = 0.010 + block.height() / 1000.0;
    let left = right - block.width / 1000.0;
    let mut out = TitleBlock {
        lines: Vec::new(),
        texts: Vec::new(),
        logo_box: None,
    };
    let mut text = |s: &str, x: f64, y: f64, h: f64, width: f64| {
        if s.is_empty() {
            return;
        }
        let height = h.min(width / (s.chars().count() as f64 * 0.95));
        out.texts.push(Text2d {
            pen: Pen::Text,
            at: [x, y],
            height,
            text: s.to_owned(),
            align: Align::Left,
            angle: 0.0,
        });
    };
    for (field, [x, y, w, h]) in block.cells() {
        let (x, y, w, h) = (
            left + x / 1000.0,
            top - (y + h) / 1000.0,
            w / 1000.0,
            h / 1000.0,
        );
        let corners = [[x, y], [x + w, y], [x + w, y + h], [x, y + h]];
        for i in 0..4 {
            out.lines
                .push(Line2d::new(Pen::Frame, corners[i], corners[(i + 1) % 4]));
        }
        let pad = 0.0015_f64.min(w / 6.0).min(h / 6.0);
        let width = w - 2.0 * pad;
        let address = &style.company;
        if let Some((caption, _)) = field.label() {
            text(
                caption,
                x + pad,
                y + h - pad - h.min(0.008) * 0.2,
                0.0016_f64.min(h * 0.2),
                width,
            );
        }
        let value = match field {
            Field::Logo => {
                out.logo_box = Some([[x + pad, y + pad], [x + w - pad, y + h - pad]]);
                continue;
            }
            Field::Company => address.name.clone(),
            Field::Address => {
                let lines = [
                    address.address.clone(),
                    format!("{} {}", address.postcode, address.city)
                        .trim()
                        .to_owned(),
                    address.website.clone(),
                ];
                for (i, line) in lines.iter().enumerate() {
                    text(
                        line,
                        x + pad,
                        y + h * (0.68 - 0.23 * i as f64),
                        0.0025_f64.min(h * 0.17),
                        width,
                    );
                }
                continue;
            }
            Field::Format => style
                .sheets
                .iter()
                .find(|s| {
                    (s.width - sheet.paper[0] * 1000.0).abs() < 0.1
                        && (s.height - sheet.paper[1] * 1000.0).abs() < 0.1
                })
                .map_or_else(
                    || {
                        format!(
                            "{:.0} × {:.0}",
                            sheet.paper[0] * 1000.0,
                            sheet.paper[1] * 1000.0
                        )
                    },
                    |s| s.key.clone(),
                ),
            Field::Scale => match sheet.views.first() {
                Some(first) if sheet.views.iter().all(|v| v.scale == first.scale) => {
                    format!("1:{}", first.scale)
                }
                Some(_) => "div.".into(),
                None => String::new(),
            },
            _ => doc.title_value(sheet, field).to_owned(),
        };
        text(&value, x + pad, y + pad, 0.003_f64.min(h * 0.36), width);
    }
    out
}
