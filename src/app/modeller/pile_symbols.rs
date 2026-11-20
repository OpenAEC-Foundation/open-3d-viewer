//! Plan symbols measured from the pile head and its actual profile.
use crate::document::{Align, Note, PlanView, NOTE_HEIGHT};
use crate::model::ElementKey;
use crate::units::UnitKey;
use bim_core::{Model, Shape};
use glam::DVec2;

pub(super) fn draw(model: &Model, plan: &mut PlanView, elevation: f64, source: u32, notes: &mut Vec<Note>, lines: &mut Vec<[[f64; 2]; 2]>) {
    let up = plan.kind.looks_up();
    let cut = elevation + plan.cut;
    for e in model.iter() {
        let Some(number) = e.pile_number else { continue };
        if plan.hidden.contains(&UnitKey::Element(ElementKey { model: source, source: e.id.0 })) { continue; }
        let Shape::Linear(l) = e.shape_in_project(&model.grid) else { continue };
        let (head, toe) = if l.end.z >= l.start.z { (l.end, l.start) } else { (l.start, l.end) };
        if up && (head.z < cut || toe.z > cut + plan.depth) || !up && head.z > cut { continue; }
        let centre = head.truncate();
        if plan.crop.is_some_and(|(lo, hi)| centre.x < lo[0] || centre.y < lo[1] || centre.x > hi[0] || centre.y > hi[1]) { continue; }
        let (Ok((axes, _)), Ok(profile)) = (bim_core::element::linear_axes(&l), l.profile.profile()) else { continue };
        let outline: Vec<DVec2> = profile.outer().iter().map(|p| centre + (axes.x * p.x + axes.y * p.y).truncate()).collect();
        let radius = outline.iter().map(|p| p.distance(centre)).fold(0.0, f64::max);
        let add = |lines: &mut Vec<_>, a: DVec2, b: DVec2| lines.push([a.to_array(), b.to_array()]);
        let mut phase = 0.0;
        let dot = (plan.scale * 0.0003).min(radius * 0.25);
        for (&a, &b) in outline.iter().zip(outline.iter().cycle().skip(1)) {
            if up { add(lines, a, b); continue; }
            let length = a.distance(b);
            if length <= 1e-10 || dot <= 0.0 { continue; }
            let mut at = -phase;
            while at < length {
                if at + dot > 0.0 { add(lines, a.lerp(b, at.max(0.0) / length), a.lerp(b, (at + dot).min(length) / length)); }
                at += 4.0 * dot;
            }
            phase = (phase + length) % (4.0 * dot);
        }
        if !up { continue; }
        if outline.len() == 4 {
            add(lines, outline[0], outline[2]);
            add(lines, outline[1], outline[3]);
        } else {
            for dir in [DVec2::X, DVec2::Y] { add(lines, centre - dir * radius, centre + dir * radius); }
        }
        let offset = plan.pile_offsets.get(&e.id.0).copied().map(DVec2::from_array)
            .unwrap_or_else(|| DVec2::splat(radius + plan.scale * 0.001));
        if offset.length() > radius + plan.scale * 0.004 {
            let along = offset.normalize_or_zero();
            add(lines, centre + along * radius, centre + offset - along * plan.scale * 0.0008);
        }
        plan.pile_tags.push((notes.len(), e.id.0, centre.to_array()));
        let text = e.pile_mark.as_ref().filter(|s| !s.is_empty()).map_or_else(|| format!("{number:02}"), |mark| format!("{number:02} {mark}"));
        notes.push(Note { at: (centre + offset).to_array(), text, size: NOTE_HEIGHT, align: Align::Left, angle: 0.0, level: false, face: Default::default() });
    }
}
