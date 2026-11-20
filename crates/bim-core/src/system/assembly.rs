//! Assemblies: the prefab units of a framed system, each with the parts it is made of.
//!
//! A framed system (a timber frame, a steel frame, a metal stud wall) is made in the workshop as
//! prefab units ([`super::joined_units`]): each is one assembly, lifted and placed as a whole. A
//! part belongs to the unit that holds its middle along the system's line, so what a layer does
//! not cut at the units' joints (a plate that runs on) is of the unit most of it lies in. A system
//! that is not framed (a curtain wall, a floor of slabs, a ceiling) is put together on site from
//! its parts and has no assemblies.

use crate::element::{Element, Shape};
use crate::model::{Model, Part};
use crate::Refusal;
use glam::DVec2;

/// A prefab unit of a framed system and what it is made of.
#[derive(Debug, Clone, PartialEq)]
pub struct Assembly {
    /// Which of the system's units it is, counted from 1 at the system's start.
    pub number: usize,
    /// From where to where it reaches along the system's line, from the line's start (metres).
    pub from: f64,
    pub to: f64,
    /// Its parts: their places among the parts [`Model::evaluate`] gives for the system.
    pub parts: Vec<usize>,
}

impl Assembly {
    /// Its mark: the system's name and number, and its own. The element drawings of the unit
    /// carry the same.
    pub fn mark(&self, element: &Element) -> String {
        mark(element, self.number)
    }

    /// How long it is (metres).
    pub fn length(&self) -> f64 {
        self.to - self.from
    }
}

/// The mark of unit `number` (from 1) of `element`.
pub fn mark(element: &Element, number: usize) -> String {
    format!("{} {}-{number}", element.name, element.id.0)
}

/// The assemblies `element` of `model` is made of, in order from its start, over its `parts` (as
/// [`Model::evaluate`] gives them): none for an element that is not a framed system. Every part
/// in the element's own tile is of exactly one of them.
pub fn assemblies(model: &Model, element: &Element, parts: &[Part]) -> Result<Vec<Assembly>, Refusal> {
    let Shape::System(s) = &element.shape else { return Ok(Vec::new()) };
    if s.system_type.as_ref().is_none_or(|t| t.framing.is_none()) {
        return Ok(Vec::new());
    }
    let Some(dir) = (s.end - s.start).try_normalize() else { return Ok(Vec::new()) };
    let (start, units) = super::joined_frame(model, element)?;
    let mut made = Vec::with_capacity(units.len());
    let mut from = start;
    for (k, length) in units.iter().enumerate() {
        made.push(Assembly { number: k + 1, from, to: from + length, parts: Vec::new() });
        from += length;
    }
    if made.is_empty() {
        return Ok(made);
    }
    for (i, part) in parts.iter().enumerate().filter(|(_, p)| p.tile == element.tile) {
        let (lo, hi) = part.solid.positions.iter().map(|q| (DVec2::new(q.x, q.y) - s.start).dot(dir)).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), u| (lo.min(u), hi.max(u)));
        let middle = (lo + hi) / 2.0;
        // past either end of the frame (a layer that runs on around a corner): of the end unit
        let k = made.iter().position(|a| middle < a.to).unwrap_or(made.len() - 1);
        made[k].parts.push(i);
    }
    Ok(made)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::SystemDef;
    use crate::tile::TileGrid;
    use bim_library::Library;

    fn frame(length: f64, id: &str) -> (Model, crate::ElementId) {
        let generic = SystemDef { start: DVec2::new(2.0, 1.0), end: DVec2::new(2.0, 1.0 + length), base: 0.5, height: 2.7, spacing: DVec2::new(1.5, 1.5), member: DVec2::new(0.05, 0.15), panel: 0.02, system_type: None, openings: Vec::new(), breaks: Vec::new(), outline: Vec::new(), division: None };
        let s = generic.typed_from(&Library::starter(), id).unwrap();
        let mut m = Model::new(TileGrid::default());
        let grid = m.grid;
        let id = m.add(Element::in_project(&grid, "Frame", None, Shape::System(s))).unwrap();
        (m, id)
    }

    #[test]
    fn a_frame_is_an_assembly_for_every_unit_and_every_part_is_of_one() {
        // 7.2 m of timber frame: three units of 2400
        let (m, id) = frame(7.2, "system.timber-frame-140");
        let element = m.get(id).unwrap();
        let parts = m.evaluate(id).unwrap();
        let made = assemblies(&m, element, &parts).unwrap();
        assert_eq!(made.iter().map(|a| a.number).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(made.iter().map(|a| (a.length() * 1000.0).round() as i64).collect::<Vec<_>>(), [2400, 2400, 2400]);
        assert_eq!(made[1].mark(element), format!("Frame {}-2", id.0));
        // every part once
        let mut all: Vec<usize> = made.iter().flat_map(|a| a.parts.iter().copied()).collect();
        all.sort_unstable();
        assert_eq!(all, (0..parts.len()).collect::<Vec<_>>());
        // each unit its own frame: two plates and five studs, whole within it
        for a in &made {
            // the members of the frame itself: within its depth of 140 about its line at x = 2
            let framed = |p: &&Part| p.class == "IfcMember" && p.material.as_deref() == Some("timber") && p.solid.positions.iter().all(|q| (q.x - 2.0).abs() < 0.07 + 1e-6);
            let own: Vec<&Part> = a.parts.iter().map(|&i| &parts[i]).filter(framed).collect();
            assert!(own.len() >= 7, "unit {}: {}", a.number, own.len());
            for p in own {
                let along: Vec<f64> = p.solid.positions.iter().map(|q| q.y - 1.0).collect();
                let (lo, hi) = along.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), u| (lo.min(*u), hi.max(*u)));
                assert!(lo > a.from - 1e-6 && hi < a.to + 1e-6, "unit {}: {lo} {hi}", a.number);
            }
        }
    }

    #[test]
    fn what_is_not_framed_has_no_assemblies() {
        let (m, id) = frame(7.2, "system.curtain-wall-aluminium-1500");
        let parts = m.evaluate(id).unwrap();
        assert!(assemblies(&m, m.get(id).unwrap(), &parts).unwrap().is_empty());
    }
}
