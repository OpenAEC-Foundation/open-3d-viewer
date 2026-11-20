//! The placeable pile legend is regenerated from the same members and head symbols as a plan.
use super::super::pile_symbols::HeadSymbol;
use crate::document::{Align, NOTE_HEIGHT, Note, PlanView};
use bim_core::{Model, ProfileDef, Shape};
use glam::DVec2;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) fn registered(presentation: &Value) -> bool {
    let legend = &presentation["legend"];
    legend["source"] == "openaec::element" && legend["symbolSource"] == "pile profile"
}

pub(in crate::app::modeller) fn is_pile_legend(plan: &PlanView) -> bool {
    plan.drawing.as_deref() == Some("pile-schedule") || registered(&plan.presentation)
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Group {
    kind: String,
    profile: String,
    grade: String,
    length: i64,
    toe: i64,
    head: i64,
}

struct Row {
    count: usize,
    size: [f64; 2],
    shaft: String,
    symbol: HeadSymbol,
}

fn mm(metres: f64) -> i64 {
    (metres * 1000.0).round() as i64
}

fn metres(mm: i64, sign: bool) -> String {
    let value = mm as f64 / 1000.0;
    let mut s = if sign {
        format!("{value:+.3}")
    } else {
        format!("{value:.3}")
    };
    if s.ends_with('0') {
        s.pop();
    }
    s.replace('.', ",")
}

// The original editable text includes explanatory notes below the supplied table.
// Its header and body are superseded by the live table. Keep its title and footnotes.
fn adopt_legend(plan: &mut PlanView) {
    let legend = &mut plan.presentation["legend"];
    let rows = legend["rows"].as_array().map_or(0, Vec::len);
    let count = 8 + 6 * rows;
    legend["typeAliases"] = json!(
        legend["rows"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| row["type"].as_str())
            .collect::<Vec<_>>()
    );
    if plan.notes.len() >= count
        && plan.notes.get(1).is_some_and(|n| n.text == "Type")
        && plan.notes.iter().take(8).any(|n| n.text == "Aantal")
    {
        legend["title"] = json!(plan.notes[0].text);
        legend["nap"] = json!(plan.notes.iter().take(8).any(|n| n.text.contains("NAP")));
        plan.notes.drain(..count);
        for n in &mut plan.notes {
            let Some((range, explanation)) = n.text.split_once('.') else {
                continue;
            };
            let words: Vec<_> = range.split_whitespace().collect();
            if let ["Paalnummers", first, "t/m", last] = words.as_slice() {
                if first.parse::<u64>().is_ok() && last.parse::<u64>().is_ok() {
                    let rest = explanation.trim();
                    n.text = if rest.is_empty() {
                        "Paalnummers".to_owned()
                    } else {
                        rest.to_owned()
                    };
                }
            }
        }
    }
    plan.lines.clear();
    plan.shapes.clear();
    plan.fills.clear();
}

pub(in crate::app::modeller) fn schedule_table(
    model: &Model,
    plan: &mut PlanView,
    office: bool,
    aliases: &[String],
) -> (Vec<Note>, Vec<[[f64; 2]; 2]>) {
    if registered(&plan.presentation) && plan.drawing.as_deref() != Some("pile-schedule") {
        adopt_legend(plan);
    }
    plan.drawing = Some("pile-schedule".into());
    if plan.presentation.is_null() {
        plan.presentation = json!({"origin":[0,0]});
    }
    plan.presentation["grids"] = json!(false);
    plan.presentation["section_marks"] = json!(false);
    let legend = &mut plan.presentation["legend"];
    if legend.is_null() {
        *legend = json!({"source":"openaec::element", "groupBy":["type","profile","length","toe","head"], "symbolSource":"pile profile"});
    }

    let mut groups: BTreeMap<Group, Row> = BTreeMap::new();
    let mut ids = Vec::new();
    for e in model.iter().filter(|e| e.pile_number.is_some()) {
        let Shape::Linear(member) = e.shape_in_project(&model.grid) else {
            continue;
        };
        let Ok(profile) = member.profile.profile() else {
            continue;
        };
        let Some(symbol) = HeadSymbol::from_member(&member) else {
            continue;
        };
        let lo = profile
            .outer()
            .iter()
            .copied()
            .fold(DVec2::splat(f64::INFINITY), DVec2::min);
        let hi = profile
            .outer()
            .iter()
            .copied()
            .fold(DVec2::splat(f64::NEG_INFINITY), DVec2::max);
        let diameter = match &member.profile {
            ProfileDef::Circle { diameter }
            | ProfileDef::Catalogue {
                shape: bim_library::ProfileShape::Circle { diameter },
                ..
            } => Some(*diameter),
            ProfileDef::Catalogue {
                shape: bim_library::ProfileShape::Chs { d, .. },
                ..
            } => Some(*d),
            _ => None,
        };
        let size = diameter.map_or(hi - lo, DVec2::splat);
        let name = member.profile.name();
        let kind = if matches!(member.profile, ProfileDef::Catalogue { .. }) {
            name.clone()
        } else {
            aliases
                .iter()
                .map(String::as_str)
                .filter(|kind| {
                    e.name.strip_prefix(*kind).is_some_and(|rest| {
                        rest.is_empty() || rest.starts_with(char::is_whitespace)
                    })
                })
                .max_by_key(|kind| kind.len())
                .unwrap_or_else(|| {
                    e.pile_mark
                        .as_deref()
                        .and_then(|mark| e.name.split_once(format!(" {mark}").as_str()))
                        .map_or(name.as_str(), |(kind, _)| kind)
                })
                .to_owned()
        };
        let shaft = if diameter.is_some() {
            format!("Ø{}", mm(size.max_element()))
        } else {
            format!("{} × {}", mm(size.x), mm(size.y))
        };
        let group = Group {
            kind,
            profile: serde_json::to_string(&member.profile).expect("finite profile"),
            grade: e.grade.clone().unwrap_or_default(),
            length: mm(member.start.distance(member.end)),
            toe: mm(member.start.z.min(member.end.z)),
            head: mm(member.start.z.max(member.end.z)),
        };
        groups
            .entry(group)
            .or_insert_with(|| Row {
                count: 0,
                size: size.to_array(),
                shaft,
                symbol,
            })
            .count += 1;
        ids.push(e.id.0);
    }
    let nap = legend["nap"].as_bool().unwrap_or(false);
    let title = legend["title"].as_str().unwrap_or("Paallegenda");
    let mut notes = Vec::new();
    let mut lines = Vec::new();
    let (left, top) = plan.crop.map_or((0.0, 0.0), |(lo, hi)| (lo[0], hi[1]));
    let unit = plan.scale / 1000.0;
    let face = if office {
        crate::typeface::Face::of("Swis721 Cn BT")
    } else {
        Default::default()
    };
    let note = |x: f64, y: f64, text: String, size: f64| Note {
        at: [left + x * unit, top - y * unit],
        text,
        size,
        align: Align::Left,
        angle: 0.0,
        level: false,
        face: face.clone(),
    };
    let headers = [
        "Type",
        "Symbool",
        "Schacht [mm]",
        "Aantal",
        "Lengte [m]",
        if nap { "Punt [m NAP]" } else { "Punt [m]" },
        if nap { "Kop [m NAP]" } else { "Kop [m]" },
    ];
    let mut width = [20.0_f64, 17.0, 25.0, 17.0, 23.0, 25.0, 25.0];
    let mut cells = Vec::new();
    let mut rows = Vec::new();
    for (g, r) in &groups {
        let kind = if g.grade.is_empty() {
            g.kind.clone()
        } else {
            format!("{} {}", g.kind, g.grade)
        };
        let values = [
            kind,
            String::new(),
            r.shaft.clone(),
            r.count.to_string(),
            metres(g.length, false),
            metres(g.toe, true),
            metres(g.head, true),
        ];
        for (col, value) in values.iter().enumerate() {
            width[col] = width[col].max(f64::from(face.measure(value, 25.0)) / 10.0 + 4.0);
        }
        cells.push(values);
        rows.push(json!({"type":g.kind, "width":r.size[0], "height":r.size[1], "count":r.count,
            "length":g.length as f64/1000.0, "toe":g.toe as f64/1000.0, "head":g.head as f64/1000.0}));
    }
    let mut x = vec![0.0];
    for w in width {
        x.push(x.last().unwrap() + w);
    }
    let table_width = *x.last().unwrap();
    notes.push(note(1.0, 9.0, title.to_owned(), 0.004));
    for (i, text) in headers.iter().enumerate() {
        notes.push(note(x[i] + 1.5, 19.0, (*text).into(), NOTE_HEIGHT));
    }
    for (i, (r, values)) in groups.values().zip(cells).enumerate() {
        let y = 27.5 + 8.5 * i as f64;
        let centre = DVec2::new(
            left + (x[1] + width[1] / 2.0) * unit,
            top - (y - 1.0) * unit,
        );
        r.symbol.draw(
            centre,
            unit * 2.0 / r.symbol.radius,
            office,
            &mut lines,
            &mut plan.symbol_fills,
        );
        for (j, text) in values.into_iter().enumerate().filter(|(j, _)| *j != 1) {
            notes.push(note(x[j] + 1.5, y, text, NOTE_HEIGHT));
        }
    }
    if office {
        plan.office_symbol_lines = lines.len();
    }
    let table_bottom = 22.0 + groups.len() as f64 * 8.5;
    let at = |x: f64, y: f64| [left + x * unit, top - y * unit];
    for column in x {
        lines.push([at(column, 13.5), at(column, table_bottom)]);
    }
    lines.push([at(0.0, 13.5), at(table_width, 13.5)]);
    for r in 0..=groups.len() {
        let y = 22.0 + r as f64 * 8.5;
        lines.push([at(0.0, y), at(table_width, y)]);
    }

    let bottom = top - table_bottom * unit;
    if let Some(previous) = legend["bodyBottom"].as_f64() {
        for n in &mut plan.notes {
            n.at[1] += bottom - previous;
        }
    } else if let Some(first) = plan.notes.iter().map(|n| n.at[1]).max_by(f64::total_cmp) {
        let shift = (bottom - 6.0 * unit - first).min(0.0);
        for n in &mut plan.notes {
            n.at[1] += shift;
        }
    }
    let mut right = left + table_width * unit;
    let mut foot = bottom;
    for n in plan.notes.iter().chain(&notes) {
        let width =
            f64::from(n.face.measure(&n.text, (n.size * 10000.0) as f32)) * plan.scale / 10000.0;
        right = right.max(n.at[0] + width);
        foot = foot.min(n.at[1] - n.size * plan.scale);
    }
    plan.crop = Some(([left, foot - unit], [right + unit, top]));
    legend["bodyBottom"] = json!(bottom);
    legend["elementIds"] = json!(ids);
    legend["rows"] = json!(rows);
    (notes, lines)
}
