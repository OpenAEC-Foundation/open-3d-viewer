//! Convert an imported pile plan only if every registered head has its complete, unique symbol.
use super::HeadSymbol;
use crate::document::PlanView;
use bim_core::{ElementId, Model, Shape};
use glam::DVec2;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

type Line = [[f64; 2]; 2];

// Imported drawing coordinates and model coordinates may round independently to sub-millimetres.
fn close(a: [f64; 2], b: [f64; 2]) -> bool {
    DVec2::from(a).distance_squared(DVec2::from(b)) < 0.0002_f64.powi(2)
}

fn same_line(a: Line, b: Line) -> bool {
    (close(a[0], b[0]) && close(a[1], b[1])) || (close(a[0], b[1]) && close(a[1], b[0]))
}

fn same_ring(a: &[[f64; 2]], b: &[[f64; 2]]) -> bool {
    a.len() == b.len()
        && !a.is_empty()
        && (0..b.len()).any(|start| {
            a.iter()
                .enumerate()
                .all(|(i, &p)| close(p, b[(start + i) % b.len()]))
                || a.iter()
                    .enumerate()
                    .all(|(i, &p)| close(p, b[(start + b.len() - i) % b.len()]))
        })
}

fn unique(mut matches: impl Iterator<Item = usize>) -> Option<usize> {
    let i = matches.next()?;
    matches.next().is_none().then_some(i)
}

pub(in crate::app::modeller) fn adopt_registered_symbols(
    model: &Model,
    ids: &[ElementId],
    plan: &mut PlanView,
) -> Option<()> {
    let mut lines = BTreeSet::new();
    let mut fills = BTreeSet::new();
    let mut notes = BTreeSet::new();
    let mut offsets = BTreeMap::new();
    for &id in ids {
        let element = model.get(id)?;
        let number = element.pile_number?;
        let Shape::Linear(member) = element.shape_in_project(&model.grid) else {
            return None;
        };
        let head = if member.end.z >= member.start.z {
            member.end
        } else {
            member.start
        }
        .truncate();
        let symbol = HeadSymbol::from_member(&member)?;
        let (mut edges, mut quarters) = (Vec::new(), Vec::new());
        symbol.draw(head, 1.0, true, &mut edges, &mut quarters);
        // The documented symbol consists of exactly four edges, two axes and a solid quarter.
        if edges.len() != 6 || quarters.len() != 1 {
            return None;
        }
        for edge in edges {
            let i = unique(
                plan.lines
                    .iter()
                    .enumerate()
                    .filter(|(_, l)| same_line(edge, **l))
                    .map(|(i, _)| i),
            )?;
            if !lines.insert(i) {
                return None;
            }
        }
        let expected = &quarters[0].loops[0];
        let i = unique(
            plan.fills
                .iter()
                .enumerate()
                .filter(|(_, f)| {
                    f.loops.len() == 1
                        && f.color.iter().all(|&v| v <= 55)
                        && same_ring(expected, &f.loops[0])
                })
                .map(|(i, _)| i),
        )?;
        if !fills.insert(i) {
            return None;
        }
        let i = unique(
            plan.notes
                .iter()
                .enumerate()
                .filter(|(_, n)| {
                    n.text.trim().parse::<u64>() == Ok(number)
                        && (DVec2::from(n.at) - head).length() <= symbol.radius + plan.scale * 0.005
                })
                .map(|(i, _)| i),
        )?;
        if !notes.insert(i) {
            return None;
        }
        offsets.insert(id.0, (DVec2::from(plan.notes[i].at) - head).to_array());
    }
    if offsets.is_empty() {
        return None;
    }

    let mut kept = vec![0; plan.lines.len() + 1];
    for i in 0..plan.lines.len() {
        kept[i + 1] = kept[i] + usize::from(!lines.contains(&i));
    }
    let mut shapes = Vec::new();
    for shape in &plan.shapes {
        let end = shape.first.checked_add(shape.count)?;
        let count = kept.get(end)? - kept.get(shape.first)?;
        if count == 0 {
            continue;
        }
        // A symbol sharing a shape with authored linework cannot be removed independently.
        if count != shape.count {
            return None;
        }
        shapes.push(crate::document::Shape {
            first: kept[shape.first],
            ..*shape
        });
    }
    let mut pens = None;
    if let Some(original) = plan.presentation.get("pens") {
        let mut by_start = BTreeMap::new();
        for pen in original.as_array()? {
            let start = usize::try_from(pen.get(0)?.as_u64()?).ok()?;
            if start >= plan.lines.len() {
                return None;
            }
            by_start.insert(start, pen.clone());
        }
        let mut remapped = BTreeMap::new();
        for (start, mut pen) in by_start {
            if kept[start] < *kept.last()? {
                pen[0] = json!(kept[start]);
                remapped.insert(kept[start], pen);
            }
        }
        pens = Some(Value::Array(remapped.into_values().collect()));
    }
    fn remove<T>(values: &mut Vec<T>, removed: &BTreeSet<usize>) {
        let mut index = 0;
        values.retain(|_| {
            let keep = !removed.contains(&index);
            index += 1;
            keep
        });
    }
    remove(&mut plan.lines, &lines);
    remove(&mut plan.fills, &fills);
    remove(&mut plan.notes, &notes);
    plan.shapes = shapes;
    if let Some(pens) = pens {
        plan.presentation["pens"] = pens;
    }
    plan.presentation["pile_label_offsets"] = json!(offsets);
    plan.pile_symbols = true;
    plan.pile_tag_marks = false;
    plan.layers.insert("IfcPile".into(), true);
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Document, PlanKind, ShapeKind};
    use bim_core::{Element, LinearDef, ProfileDef};
    use glam::DVec3;

    #[test]
    fn registered_symbols_require_a_complete_match_and_preserve_other_line_styles() {
        let mut model = Model::default();
        let l = LinearDef {
            start: DVec3::NEG_Z,
            end: DVec3::ZERO,
            profile: ProfileDef::Rectangle {
                width: 0.3,
                height: 0.3,
            },
            roll: 0.0,
        };
        let mut e = Element::in_project(&model.grid, "Pile", None, Shape::Linear(l.clone()));
        e.id = ElementId(8);
        e.pile_number = Some(7);
        model.insert(e);
        let mut doc = Document::default();
        doc.add_level("A", 0.0).unwrap();
        let level = doc.levels()[0].path.clone();
        let view = doc
            .add_plan_of(&level, PlanKind::StructuralUp, "Piles")
            .unwrap();
        doc.set_plan_tags(|p| {
            let unrelated = [[2.0, 2.0], [2.5, 2.0]];
            p.lines.push(unrelated);
            HeadSymbol::from_member(&l).unwrap().draw(
                DVec2::ZERO,
                1.0,
                true,
                &mut p.lines,
                &mut p.fills,
            );
            p.lines[3].reverse();
            p.fills[0].loops[0].reverse();
            p.lines.push(unrelated);
            p.presentation = json!({"pens":[[0,[0,1,2],0.3],[2,[4,5,6],0.5],[7,[7,8,9],0.7]]});
            p.shapes.push(crate::document::Shape {
                kind: ShapeKind::Polyline,
                first: 7,
                count: 1,
            });
            p.notes.push(crate::document::Note {
                at: [0.3, 0.3],
                text: "07".into(),
                size: 0.0025,
                align: crate::document::Align::Left,
                angle: 0.0,
                level: false,
                face: Default::default(),
            });
            let id = ElementId(8);
            let mut incomplete = p.clone();
            incomplete.fills.clear();
            assert!(adopt_registered_symbols(&model, &[id], &mut incomplete).is_none());
            assert_eq!(incomplete.lines, p.lines);
            assert!(adopt_registered_symbols(&model, &[id, ElementId(9)], p).is_none());
            assert_eq!(p.lines.len(), 8);
            assert!(adopt_registered_symbols(&model, &[id], p).is_some());
            (vec![], vec![])
        });
        let p = doc.plan(&view).unwrap();
        assert_eq!(p.lines.len(), 2);
        assert_eq!(p.shapes[0].first, 1);
        assert_eq!(
            p.presentation["pens"],
            json!([[0, [0, 1, 2], 0.3], [1, [7, 8, 9], 0.7]])
        );
        assert!(p.notes.is_empty() && p.fills.is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "requires a local copy of an issued registered pile plan"]
    fn issued_registered_symbols_follow_pile_edits_and_reopen() {
        use crate::app::App;
        use crate::loaders::{Progress, Source, load_with};
        use std::sync::Arc;
        let source = Source::File(std::env::var_os("PILE_REGISTERED_IFCX").unwrap().into());
        let mut app = App::default();
        app.set_model(Arc::new(load_with(&source, &Progress::default()).unwrap()));
        let view = app
            .document
            .plans()
            .iter()
            .find(|p| p.drawing.as_deref() == Some("pile-plan") && p.pile_tags.len() == 68)
            .expect("registered main pile plan")
            .path
            .clone();
        let legend = app
            .document
            .plans()
            .iter()
            .find(|p| p.presentation["legend"]["typeAliases"].is_array())
            .unwrap()
            .path
            .clone();
        let p = app.document.plan(&view).unwrap();
        assert_eq!(
            (
                p.lines.len(),
                p.notes.len(),
                p.fills.len(),
                p.symbols.len(),
                p.symbol_fills.len()
            ),
            (100, 1, 0, 408, 68)
        );
        let n_dims = p.dimensions.len();
        let id = ElementId(p.pile_tags[0].1);
        let mut e = app.modeller.model.get(id).unwrap().clone();
        let Shape::Linear(member) = &mut e.shape else {
            unreachable!()
        };
        member.profile = ProfileDef::Circle { diameter: 0.35 };
        app.modeller.model.replace(e.clone()).unwrap();
        app.show_modelled();
        assert_eq!(app.document.plan(&view).unwrap().symbol_fills.len(), 67);
        assert!(
            app.document
                .plan(&legend)
                .unwrap()
                .tags
                .iter()
                .any(|n| n.text == "Ø350")
        );
        let copy = app.modeller.model.add(e).unwrap();
        app.show_modelled();
        assert_eq!(app.document.plan(&view).unwrap().pile_tags.len(), 69);
        app.modeller.model.remove(copy);
        app.show_modelled();
        let p = app.document.plan(&view).unwrap();
        assert_eq!((p.pile_tags.len(), p.dimensions.len()), (68, n_dims));
        let bytes = crate::export::to_bytes(
            app.model.as_ref().unwrap(),
            &app.document_now(),
            "reopened.ifcx",
        )
        .unwrap();
        let mut back = App::default();
        back.set_model(Arc::new(
            load_with(
                &Source::Bytes {
                    name: "reopened.ifcx".into(),
                    data: Arc::new(bytes),
                },
                &Progress::default(),
            )
            .unwrap(),
        ));
        let restored = back.document.plan(&view).unwrap();
        assert_eq!(restored.pile_tags.len(), 68);
        assert_eq!(restored.symbols.len(), p.symbols.len());
        assert_eq!(restored.symbol_fills.len(), 67);
        assert_eq!(restored.lines, p.lines);
        assert_eq!(restored.notes, p.notes);
        assert_eq!(restored.dimensions.len(), n_dims);
    }
}
