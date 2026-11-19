//! Element drawings: the drawing a prefab unit of a timber frame is made from in the workshop.
//!
//! One drawing for every unit of a framed system, made straight from the parts the kernel
//! generates, without a view, a cut or the graphics card: the frame seen from its interior, every
//! member as its outline; the unit's length, its height, its studs centre to centre and its
//! openings dimensioned; and under it the list of its members, alike ones counted together. A
//! drawing is a [`Drawing2d`], so it is written in every format a view's drawing is.
//!
//! What is drawn is the frame: the system's own face. Its boards, membranes and battens are not.

use crate::drawing2d::{Align, Dim2d, Drawing2d, Line2d, Pen, Space, Text2d};
use bim_core::{Element, Model, Shape, LINEAR_TOLERANCE};
use glam::DVec2;

/// The scale an element drawing is drawn at: 1 : 20.
pub const SCALE: f64 = 20.0;
/// Text on paper: 2.5 mm capitals (metres), and how far a dimension line stands off (metres).
const TEXT: f64 = 0.0025;
const OFFSET: f64 = 0.006;
/// Studs nearer to one another than this (metres) are dimensioned as one.
const APART: f64 = 0.1;

/// A member of a frame in the frame's face: from where to where along the frame and up it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Member {
    u: (f64, f64),
    v: (f64, f64),
}

impl Member {
    /// Its size in the face and its length (metres): a stud's width and height, a plate's height
    /// and length.
    fn section(&self) -> (f64, f64) {
        let (du, dv) = (self.u.1 - self.u.0, self.v.1 - self.v.0);
        (du.min(dv), du.max(dv))
    }

    fn upright(&self) -> bool {
        self.v.1 - self.v.0 > self.u.1 - self.u.0
    }
}

/// Millimetres, whole.
fn mm(metres: f64) -> i64 {
    (metres * 1000.0).round() as i64
}

/// The drawings of `element` of `model`, one for every prefab unit it is made of, in order from
/// its start. None for an element that is not a framed standing system, or that the kernel
/// refuses.
pub fn element_drawings(model: &Model, element: &Element) -> Vec<Drawing2d> {
    let Shape::System(s) = &element.shape else { return Vec::new() };
    let Some(t) = s.system_type.as_ref().filter(|t| t.framing.is_some() && !t.lying) else { return Vec::new() };
    let Some(dir) = (s.end - s.start).try_normalize() else { return Vec::new() };
    let (Ok(parts), Ok(units)) = (model.evaluate(element.id), bim_core::system::joined_units(model, element)) else { return Vec::new() };
    let left = dir.perp();
    let (half, tol) = (s.member.y / 2.0, 1e-6);
    // the members of the frame itself: of the frame's timber, as deep as the frame and inside it
    let mut members: Vec<Member> = Vec::new();
    for p in parts.iter().filter(|p| p.class == "IfcMember" && p.tile == element.tile && p.material.as_deref() == Some(t.member_material.as_str())) {
        let (mut u, mut v, mut across) = ((f64::INFINITY, f64::NEG_INFINITY), (f64::INFINITY, f64::NEG_INFINITY), (f64::INFINITY, f64::NEG_INFINITY));
        for q in &p.solid.positions {
            let flat = DVec2::new(q.x, q.y) - s.start;
            let (a, b, c) = (flat.dot(dir), q.z - s.base, flat.dot(left));
            u = (u.0.min(a), u.1.max(a));
            v = (v.0.min(b), v.1.max(b));
            across = (across.0.min(c), across.1.max(c));
        }
        if across.0 >= -half - tol && across.1 <= half + tol && (across.1 - across.0 - s.member.y).abs() < tol {
            members.push(Member { u, v });
        }
    }
    if members.is_empty() {
        return Vec::new();
    }
    // the frame starts where its first member does: at a corner that is not at its line's start
    let start = members.iter().map(|m| m.u.0).fold(f64::INFINITY, f64::min);
    let (text, off) = (TEXT * SCALE, OFFSET * SCALE);
    let mut drawings = Vec::with_capacity(units.len());
    let mut lo = start;
    for (k, length) in units.iter().enumerate() {
        let hi = lo + length;
        let mut d = Drawing2d { name: format!("{} {}-{}", element.name, element.id.0, k + 1), space: Space::Model { scale: SCALE }, paper: None, lines: Vec::new(), texts: Vec::new(), dimensions: Vec::new() };
        let mut own: Vec<Member> = members.iter().copied().filter(|m| (m.u.0 + m.u.1) / 2.0 > lo - tol && (m.u.0 + m.u.1) / 2.0 < hi + tol).collect();
        own.sort_by(|a, b| a.u.0.total_cmp(&b.u.0).then(a.v.0.total_cmp(&b.v.0)));
        let mut line = |d: &mut Drawing2d, pen: Pen, a: [f64; 2], b: [f64; 2]| d.lines.push(Line2d { pen, a, b });
        for m in &own {
            let (x0, x1, y0, y1) = (m.u.0 - lo, m.u.1 - lo, m.v.0, m.v.1);
            for (a, b) in [([x0, y0], [x1, y0]), ([x1, y0], [x1, y1]), ([x1, y1], [x0, y1]), ([x0, y1], [x0, y0])] {
                line(&mut d, Pen::Cut, a, b);
            }
        }
        let dim = |d: &mut Drawing2d, from: [f64; 2], to: [f64; 2], rows: f64| {
            let value = (to[0] - from[0]).hypot(to[1] - from[1]);
            if value > LINEAR_TOLERANCE {
                d.dimensions.push(Dim2d { from, to, value, offset: off * rows, text_height: text });
            }
        };
        let height = s.height;
        // over it: its studs from plate to plate, centre to centre from either end
        let mut studs: Vec<f64> = own.iter().filter(|m| m.upright() && m.v.1 - m.v.0 > height * 0.9).map(|m| (m.u.0 + m.u.1) / 2.0 - lo).collect();
        studs.dedup_by(|a, b| (*a - *b).abs() < tol);
        // studs that stand against one another, or against the unit's end, are one place to measure to
        let mut chain: Vec<f64> = vec![0.0];
        for x in studs.iter().copied().filter(|x| *x > APART && *x < length - APART) {
            if chain.last().is_some_and(|last| x - last >= APART) {
                chain.push(x);
            }
        }
        chain.push(*length);
        for pair in chain.windows(2) {
            dim(&mut d, [pair[0], height], [pair[1], height], 1.0);
        }
        // under it: its openings along it, and its whole length (drawn from right to left, so
        // the dimension lines stand below)
        let inside: Vec<_> = s.openings.iter().filter(|o| o.along - start > lo - start - tol && o.along + o.width - start < hi - start + tol).filter(|o| o.along > lo - tol && o.along + o.width < hi + tol).collect();
        let mut along: Vec<f64> = vec![0.0];
        for o in &inside {
            let (a, b, foot, head) = (o.along - lo, o.along + o.width - lo, o.sill, o.sill + o.height);
            along.extend([a, b]);
            // the opening: its outline and a cross through it
            for (p, q) in [([a, foot], [b, foot]), ([b, foot], [b, head]), ([b, head], [a, head]), ([a, head], [a, foot]), ([a, foot], [b, head]), ([a, head], [b, foot])] {
                line(&mut d, Pen::Detail, p, q);
            }
        }
        along.push(*length);
        along.sort_by(f64::total_cmp);
        let rows = if inside.is_empty() { 1.0 } else { 2.0 };
        if !inside.is_empty() {
            for pair in along.windows(2) {
                dim(&mut d, [pair[1], 0.0], [pair[0], 0.0], 1.0);
            }
        }
        dim(&mut d, [*length, 0.0], [0.0, 0.0], rows);
        // to its left its height; to its right the sill and the head of its first opening
        dim(&mut d, [0.0, 0.0], [0.0, height], 1.0);
        if let Some(o) = inside.first() {
            let mut up: Vec<f64> = vec![0.0, o.sill, o.sill + o.height, height];
            up.dedup_by(|a, b| (*a - *b).abs() < tol);
            for pair in up.windows(2) {
                dim(&mut d, [*length, pair[1]], [*length, pair[0]], 1.0);
            }
        }
        // its name, and the list of its members: alike ones counted together, the longest first
        let depth = mm(s.member.y);
        let mut list: Vec<((i64, i64), usize)> = Vec::new();
        for m in &own {
            let (width, long) = m.section();
            let key = (mm(width), mm(long));
            match list.iter_mut().find(|(k, _)| *k == key) {
                Some((_, n)) => *n += 1,
                None => list.push((key, 1)),
            }
        }
        list.sort_by(|a, b| b.0 .1.cmp(&a.0 .1).then(b.0 .0.cmp(&a.0 .0)));
        let mut y = -(off * (rows + 1.5) + text);
        let mut write = |d: &mut Drawing2d, size: f64, line: String| {
            d.texts.push(Text2d { pen: Pen::Text, at: [0.0, y], height: size, text: line, align: Align::Left });
            y -= size * 1.8;
        };
        write(&mut d, text * 1.4, format!("{} · {}/{} · {} × {} mm · 1:{}", element.name, k + 1, units.len(), mm(*length), mm(height), SCALE as i64));
        for ((width, long), n) in &list {
            write(&mut d, text, format!("{n} × {width}×{depth} L={long}"));
        }
        drawings.push(d);
        lo = hi;
    }
    drawings
}

#[cfg(test)]
mod tests {
    use super::*;
    use bim_core::{Opening, SystemDef, TileGrid};
    use bim_library::Library;

    fn frame(length: f64, openings: Vec<Opening>) -> (Model, bim_core::ElementId) {
        let generic = SystemDef { start: DVec2::new(2.0, 1.0), end: DVec2::new(2.0, 1.0 + length), base: 0.5, height: 2.7, spacing: DVec2::new(1.5, 1.5), member: DVec2::new(0.05, 0.15), panel: 0.02, system_type: None, openings, breaks: Vec::new() };
        let s = generic.typed_from(&Library::starter(), "system.timber-frame-140").unwrap();
        let mut m = Model::new(TileGrid::default());
        let grid = m.grid;
        let id = m.add(Element::in_project(&grid, "Frame", None, Shape::System(s))).unwrap();
        (m, id)
    }

    #[test]
    fn a_frame_gets_a_drawing_for_every_unit_with_its_members_dimensions_and_list() {
        // 7.2 m along +y from (2, 1), its base at 0.5: three units of 2400
        let (m, id) = frame(7.2, Vec::new());
        let drawings = element_drawings(&m, m.get(id).unwrap());
        assert_eq!(drawings.len(), 3);
        for (k, d) in drawings.iter().enumerate() {
            // two plates, two end studs and the studs at 600 between them: 7 members, 4 lines each
            assert_eq!(d.lines.iter().filter(|l| l.pen == Pen::Cut).count(), 7 * 4, "unit {k}");
            // drawn from its own corner, whatever its place and direction in the model
            let (lo, hi) = d.extent().unwrap();
            assert!(lo[0] < 0.0 && hi[0] > 2.4 && hi[0] < 3.0 && hi[1] > 2.7, "unit {k}: {lo:?} {hi:?}");
            // its length, its height, and its studs centre to centre
            let values: Vec<i64> = d.dimensions.iter().map(|x| mm(x.value)).collect();
            assert!(values.contains(&2400) && values.contains(&2700) && values.iter().filter(|v| **v == 600).count() >= 2, "unit {k}: {values:?}");
            // the list: the plates, the studs
            assert!(d.texts.iter().any(|t| t.text == "2 × 38×140 L=2400"), "unit {k}: {:?}", d.texts.iter().map(|t| &t.text).collect::<Vec<_>>());
            assert!(d.texts.iter().any(|t| t.text == "5 × 38×140 L=2624"), "unit {k}");
            assert!(d.texts[0].text.contains(&format!("{}/3", k + 1)) && d.texts[0].text.contains("2400 × 2700"));
        }
    }

    #[test]
    fn an_opening_is_drawn_and_dimensioned_in_the_unit_it_is_in() {
        let window = Opening { along: 0.9, sill: 0.9, width: 1.2, height: 1.2, framing: None };
        let (m, id) = frame(3.3, vec![window]);
        let drawings = element_drawings(&m, m.get(id).unwrap());
        assert_eq!(drawings.len(), 1);
        let d = &drawings[0];
        // its outline and its cross
        assert_eq!(d.lines.iter().filter(|l| l.pen == Pen::Detail).count(), 6);
        let values: Vec<i64> = d.dimensions.iter().map(|x| mm(x.value)).collect();
        for want in [900, 1200, 3300, 2700, 600] {
            assert!(values.contains(&want), "{want} in {values:?}");
        }
        // the header is in the list, as wide as the opening and its trimmers
        assert!(d.texts.iter().any(|t| t.text.ends_with(&format!("L={}", 1200 + 2 * 38))), "{:?}", d.texts.iter().map(|t| &t.text).collect::<Vec<_>>());
    }

    #[test]
    fn what_is_not_a_frame_has_no_element_drawing() {
        let curtain = SystemDef { start: DVec2::ZERO, end: DVec2::new(3.0, 0.0), base: 0.0, height: 3.0, spacing: DVec2::new(1.5, 1.5), member: DVec2::new(0.05, 0.15), panel: 0.02, system_type: None, openings: Vec::new(), breaks: Vec::new() };
        let mut m = Model::new(TileGrid::default());
        let grid = m.grid;
        let id = m.add(Element::in_project(&grid, "Curtain wall", None, Shape::System(curtain))).unwrap();
        assert!(element_drawings(&m, m.get(id).unwrap()).is_empty());
    }
}
