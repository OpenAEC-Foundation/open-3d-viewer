//! Plan symbols measured from the pile head and its actual profile.
use crate::document::{Align, Note, PlanView, NOTE_HEIGHT};
use crate::model::ElementKey;
use crate::units::UnitKey;
use bim_core::{Model, Shape};
use glam::DVec2;

pub(super) fn draw(
    model: &Model,
    plan: &mut PlanView,
    elevation: f64,
    source: u32,
    office_style: bool,
    notes: &mut Vec<Note>,
    lines: &mut Vec<[[f64; 2]; 2]>,
) {
    let up = plan.kind.looks_up();
    let cut = elevation + plan.cut;
    plan.corner_labels = up && office_style;
    for e in model.iter() {
        let Some(number) = e.pile_number else {
            continue;
        };
        if plan.hidden.contains(&UnitKey::Element(ElementKey {
            model: source,
            source: e.id.0,
        })) {
            continue;
        }
        let Shape::Linear(l) = e.shape_in_project(&model.grid) else {
            continue;
        };
        let (head, toe) = if l.end.z >= l.start.z {
            (l.end, l.start)
        } else {
            (l.start, l.end)
        };
        if up && (head.z < cut || toe.z > cut + plan.depth) || !up && head.z > cut {
            continue;
        }
        let centre = head.truncate();
        if plan.crop.is_some_and(|(lo, hi)| {
            centre.x < lo[0] || centre.y < lo[1] || centre.x > hi[0] || centre.y > hi[1]
        }) {
            continue;
        }
        let Some(symbol) = HeadSymbol::from_member(&l) else { continue };
        let outline = &symbol.rings[0];
        let radius = symbol.radius;
        let add = |lines: &mut Vec<_>, a: DVec2, b: DVec2| lines.push([a.to_array(), b.to_array()]);
        if up {
            symbol.draw(centre, 1.0, office_style, lines, &mut plan.symbol_fills);
            if office_style { plan.office_symbol_lines = lines.len(); }
        } else {
            let dot = (plan.scale * 0.0003).min(radius * 0.25);
            for ring in &symbol.rings {
                let mut phase = 0.0;
                for (&a, &b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
                    let length = a.distance(b);
                    if length <= 1e-10 || dot <= 0.0 { continue; }
                    let mut at = -phase;
                    while at < length {
                        if at + dot > 0.0 {
                            add(lines, centre + a.lerp(b, at.max(0.0) / length),
                                centre + a.lerp(b, (at + dot).min(length) / length));
                        }
                        at += 4.0 * dot;
                    }
                    phase = (phase + length) % (4.0 * dot);
                }
            }
            if e.level.as_deref() != Some(&plan.level) { continue; }
        }
        let offset = if plan.corner_labels {
            let hi = outline.iter().copied().fold(DVec2::ZERO, DVec2::max);
            let previous = plan.presentation.get("pile_label_offsets").and_then(|v| v.get(e.id.0.to_string()));
            previous.and_then(|v| serde_json::from_value::<[f64; 2]>(v.clone()).ok()).map(DVec2::from_array)
                .unwrap_or(hi + plan.scale * DVec2::new(0.00135, 0.00145))
        } else {
            plan.pile_offsets.get(&e.id.0).copied().map(DVec2::from_array)
                .unwrap_or_else(|| DVec2::splat(radius + plan.scale * 0.001))
        };
        if !plan.corner_labels && offset.length() > radius + plan.scale * 0.004 {
            let along = offset.normalize_or_zero();
            add(
                lines,
                centre + along * radius,
                centre + offset - along * plan.scale * 0.0008,
            );
        }
        plan.pile_tags
            .push((notes.len(), e.id.0, centre.to_array()));
        let text = if !up {
            number.to_string()
        } else {
            e.pile_mark
                .as_ref()
                .filter(|s| !s.is_empty() && plan.pile_tag_marks)
                .map_or_else(
                    || format!("{number:02}"),
                    |mark| format!("{number:02} {mark}"),
                )
        };
        notes.push(Note {
            at: (centre + offset).to_array(),
            text,
            size: if plan.corner_labels { 7.0 * 0.0254 / 72.0 } else { NOTE_HEIGHT },
            align: if plan.corner_labels { Align::Left } else { Align::Center },
            angle: 0.0,
            level: false,
            face: if plan.corner_labels { crate::typeface::Face::of("Swis721 Cn BT") } else { Default::default() },
        });
    }
}

/// Projected profile rings relative to the head, shared by the plan and its legend.
pub(super) struct HeadSymbol {
    rings: Vec<Vec<DVec2>>,
    pub(super) radius: f64,
}

impl HeadSymbol {
    pub(super) fn from_member(member: &bim_core::LinearDef) -> Option<Self> {
        let (axes, _) = bim_core::element::linear_axes(member).ok()?;
        let profile = member.profile.profile().ok()?;
        let rings: Vec<Vec<_>> = std::iter::once(profile.outer()).chain(profile.holes().iter().map(Vec::as_slice))
            .map(|ring| ring.iter().map(|p| (axes.x * p.x + axes.y * p.y).truncate()).collect()).collect();
        let radius = rings[0].iter().map(|p| p.length()).fold(0.0, f64::max);
        (radius > 0.0).then_some(Self { rings, radius })
    }

    pub(super) fn draw(&self, centre: DVec2, scale: f64, office: bool, lines: &mut Vec<[[f64; 2]; 2]>, fills: &mut Vec<crate::document::Fill>) {
        let outer: Vec<_> = self.rings[0].iter().map(|p| centre + *p * scale).collect();
        if office && outer.len() == 4 && self.rings.len() == 1 {
            office_symbol(centre, &outer, lines, fills);
            return;
        }
        for ring in &self.rings {
            for (&a, &b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
                lines.push([(centre + a * scale).to_array(), (centre + b * scale).to_array()]);
            }
        }
        if outer.len() == 4 {
            lines.push([outer[0].to_array(), outer[2].to_array()]);
            lines.push([outer[1].to_array(), outer[3].to_array()]);
        } else {
            for axis in [DVec2::X, DVec2::Y] {
                let reach = axis * self.radius * scale;
                lines.push([(centre - reach).to_array(), (centre + reach).to_array()]);
            }
        }
    }
}

mod migrate;
pub(super) use migrate::adopt_registered_symbols;

/// Square shaft, solid upper-right quarter and two axes extending half a shaft beyond each face.
fn office_symbol(centre: DVec2, outline: &[DVec2], lines: &mut Vec<[[f64; 2]; 2]>, fills: &mut Vec<crate::document::Fill>) {
    for (&a, &b) in outline.iter().zip(outline.iter().cycle().skip(1)) {
        lines.push([a.to_array(), b.to_array()]);
    }
    let axis = |i: usize| (outline[i] + outline[(i+1) % 4]) * 0.5 - centre;
    for i in 0..2 {
        let reach = axis(i) * 2.0;
        lines.push([(centre - reach).to_array(), (centre + reach).to_array()]);
    }
    let k = (0..4).max_by(|&a, &b| outline[a].element_sum().total_cmp(&outline[b].element_sum())).unwrap();
    let ring = [centre, centre + axis((k+3)%4), outline[k], centre + axis(k)];
    fills.push(crate::document::Fill { loops: vec![ring.map(|p| p.to_array()).to_vec()], color: [41,49,51] });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Document, PlanKind};
    use bim_core::{Element, LinearDef, ProfileDef, TileGrid};
    use glam::DVec3;

    #[test]
    fn pile_view_symbols_follow_the_head_and_remember_the_dragged_label() {
        let grid = TileGrid::default();
        let mut model = Model::new(grid);
        let mut doc = Document::default();
        doc.add_level("Foundation", 0.0).unwrap();
        let level = doc.levels()[0].path.clone();
        let shape = Shape::Linear(LinearDef {
            start: DVec3::new(1.0, 2.0, -12.0),
            end: DVec3::new(1.0, 2.0, -0.5),
            profile: ProfileDef::Rectangle {
                width: 0.3,
                height: 0.3,
            },
            roll: 0.0,
        });
        let mut pile = Element::in_project(&grid, "Pile", Some(level.clone()), shape);
        pile.pile_number = Some(7);
        pile.pile_mark = Some("A".into());
        let id = bim_core::ElementId(1);
        pile.id = id;
        model.insert(pile);
        let plan = doc
            .add_plan_of(&level, PlanKind::StructuralUp, "Piles")
            .unwrap();
        let regenerate = |doc: &mut Document| {
            doc.set_plan_tags(|p| {
                p.pile_tag_marks = true;
                let (mut notes, mut lines) = (Vec::new(), Vec::new());
                draw(&model, p, 0.0, 0, false, &mut notes, &mut lines);
                (notes, lines)
            });
        };
        regenerate(&mut doc);
        assert_eq!(doc.view_tags(&plan)[0].text, "07 A");
        assert!(doc.move_pile_tag(&plan, 0, [4.0, 5.0]));
        regenerate(&mut doc);
        assert_eq!(doc.view_tags(&plan)[0].at, [4.0, 5.0]);
        assert_eq!(doc.plan(&plan).unwrap().pile_offsets[&id.0], [3.0, 3.0]);
        assert_eq!(doc.view_symbols(&plan).len(), 7); // profile, cross and leader
        let office = |doc: &mut Document| doc.set_plan_tags(|p| {
            let (mut notes, mut lines) = (Vec::new(), Vec::new());
            draw(&model, p, 0.0, 0, true, &mut notes, &mut lines);
            (notes, lines)
        });
        office(&mut doc);
        let tag = &doc.view_tags(&plan)[0];
        assert!(tag.at[0] > 1.15 && tag.at[1] > 2.15);
        assert_eq!(tag.align, Align::Left);
        assert!((tag.size * 72.0 / 0.0254 - 7.0).abs() < 1e-10);
        let lines = doc.view_symbols(&plan);
        assert_eq!(lines.len(), 6); // four shaft edges and two centre lines
        assert_eq!(doc.view_fills(&plan).count(), 1);
        assert!(doc.move_pile_tag(&plan, 0, [4.0, 5.0]));
        office(&mut doc);
        assert_eq!(doc.view_symbols(&plan).len(), 6);
        assert_eq!(doc.view_tags(&plan)[0].at, [4.0, 5.0]);

    }
}
