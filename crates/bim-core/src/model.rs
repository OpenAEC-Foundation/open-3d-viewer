//! The model: elements by id over a tile grid, and their evaluation to solids.
//!
//! Elements hold definitions; [`Model::evaluate`] derives their solids. A connection joins linear
//! elements ([`crate::join`]): it trims their ends, takes material out of them and adds to them, so
//! a member is evaluated with the connections that name it; the connection's own parts (plates,
//! bolts) are its body. A connection that cannot be made is refused by its own evaluation and
//! leaves its members as they are.

use crate::element::{planar_parts, point_solid, system_parts, ConnectionDef, ConnectionKind, Element, ElementId, LinearDef, Shape};
use crate::join::{self, JoinKind, JoinResult, Member, MemberEnd};
use crate::solid::{Plane, Solid};
use crate::tile::{TileGrid, TileKey};
use crate::boolean::{boolean, BooleanOp};
use crate::Refusal;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How far apart (metres) the ends of members may be and still be joined.
pub const JOIN_TOLERANCE: f64 = 1e-3;

/// A solid of an element, in the frame of the element's tile.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    /// The IFC class of the part (a curtain wall's members are `IfcMember`).
    pub class: &'static str,
    pub tile: TileKey,
    pub solid: Solid,
    /// What it is made of: the material key of its layer, for a planar element with a build-up.
    pub material: Option<String>,
}

/// The elements of a project over its tile grid.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub grid: TileGrid,
    elements: BTreeMap<ElementId, Element>,
    next: u64,
}

impl Model {
    pub fn new(grid: TileGrid) -> Self {
        Self { grid, elements: BTreeMap::new(), next: 1 }
    }

    pub fn len(&self) -> usize {
        self.elements.len()
    }

    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    pub fn get(&self, id: ElementId) -> Option<&Element> {
        self.elements.get(&id)
    }

    /// The elements in id order.
    pub fn iter(&self) -> impl Iterator<Item = &Element> {
        self.elements.values()
    }

    /// Adds `element` under a new id, if it evaluates; a refusal leaves the model as it was.
    pub fn add(&mut self, mut element: Element) -> Result<ElementId, Refusal> {
        let id = ElementId(self.next.max(1));
        element.id = id;
        element.rehome(&self.grid);
        self.evaluate_element(&element)?;
        self.elements.insert(id, element);
        self.next = id.0 + 1;
        Ok(id)
    }

    /// Puts `element` back under its own id (reading a file): kept even when it does not evaluate,
    /// so nothing is lost; its evaluation reports the refusal.
    pub fn insert(&mut self, element: Element) {
        self.next = self.next.max(element.id.0 + 1);
        self.elements.insert(element.id, element);
    }

    /// Changes an element, if the change evaluates.
    pub fn replace(&mut self, mut element: Element) -> Result<(), Refusal> {
        if !self.elements.contains_key(&element.id) {
            return Err(Refusal::Missing);
        }
        element.rehome(&self.grid);
        self.evaluate_element(&element)?;
        self.elements.insert(element.id, element);
        Ok(())
    }

    /// Removes an element and the connections that join it; returns what was removed.
    pub fn remove(&mut self, id: ElementId) -> Vec<Element> {
        let Some(element) = self.elements.remove(&id) else { return Vec::new() };
        let joined: Vec<ElementId> = self
            .elements
            .values()
            .filter(|e| matches!(&e.shape, Shape::Connection(c) if c.primary == id || c.secondary == id))
            .map(|e| e.id)
            .collect();
        std::iter::once(element).chain(joined.into_iter().filter_map(|j| self.elements.remove(&j))).collect()
    }

    /// The elements per tile.
    pub fn tiles(&self) -> BTreeMap<TileKey, Vec<ElementId>> {
        let mut tiles: BTreeMap<TileKey, Vec<ElementId>> = BTreeMap::new();
        for e in self.elements.values() {
            tiles.entry(e.tile).or_default().push(e.id);
        }
        tiles
    }

    /// The solids of element `id`, in its tile's frame.
    pub fn evaluate(&self, id: ElementId) -> Result<Vec<Part>, Refusal> {
        self.evaluate_element(self.elements.get(&id).ok_or(Refusal::Missing)?)
    }

    fn evaluate_element(&self, element: &Element) -> Result<Vec<Part>, Refusal> {
        if let Shape::Connection(c) = &element.shape {
            self.check_connection(c)?;
            // its own parts (plates, bolts), from the frame of tile (0, 0) to its own
            let Ok((_, result)) = self.join(c) else { return Ok(Vec::new()) };
            let offset = self.grid.offset(TileKey::default(), element.tile);
            return Ok(result.parts.into_iter().map(|p| Part { class: p.class, tile: element.tile, solid: p.solid.translated(offset), material: p.material }).collect());
        }
        let mut parts = self.body(element)?;
        // the cuts made in it, in id order; a cut that cannot be made leaves it as it is
        for e in self.elements.values() {
            if let Shape::Connection(c) = &e.shape {
                if c.kind == ConnectionKind::Cut && c.secondary == element.id {
                    if let Ok(cut) = self.cut(&parts, element, c.primary) {
                        parts = cut;
                    }
                }
            }
        }
        Ok(parts)
    }

    /// An element's solids before any cut is made in it (with the ends its joints give it). A
    /// cutter cuts with this body, so cuts never depend on each other.
    fn body(&self, element: &Element) -> Result<Vec<Part>, Refusal> {
        let class = element.class();
        let part = |solid: Solid| Part { class, tile: element.tile, solid, material: None };
        match &element.shape {
            Shape::Point(p) => Ok(vec![part(point_solid(p)?)]),
            Shape::Planar(p) => {
                // a wall meeting other walls is cut to its corners; when that cannot be made, it is
                // made as it is
                let joined = crate::joins::wall_join(self, element).and_then(|join| joined_parts(p, &join).ok());
                let parts = match joined {
                    Some(parts) => parts,
                    None => planar_parts(p)?,
                };
                Ok(parts.into_iter().map(|(material, solid)| Part { material, ..part(solid) }).collect())
            }
            Shape::System(s) => Ok(system_parts(s)?.into_iter().map(|p| Part { class: p.class, tile: element.tile, solid: p.solid, material: p.material }).collect()),
            Shape::Linear(l) => {
                let joined = self.joined(element.id, element.tile);
                let body = join::finish(&Member::of(l)?, joined.start, joined.end, &joined.additions, &joined.removals);
                // additions or removals that cannot be made leave the member with its trims only
                let body = match body {
                    Ok(body) => body,
                    Err(_) => join::finish(&Member::of(l)?, joined.start, joined.end, &[], &[])?,
                };
                Ok(vec![part(body)])
            }
            Shape::Connection(_) => Err(Refusal::NoBody),
        }
    }

    /// `parts` of `target` with the body of element `cutter` taken out. A part the cutter takes
    /// wholly goes; when nothing is left, or the cutter does not reach the target, it is refused.
    fn cut(&self, parts: &[Part], target: &Element, cutter: ElementId) -> Result<Vec<Part>, Refusal> {
        let tool = self.elements.get(&cutter).ok_or(Refusal::Missing)?;
        let offset = self.grid.offset(tool.tile, target.tile);
        let tools: Vec<Solid> = self.body(tool)?.iter().map(|p| p.solid.translated(offset)).collect();
        let overlaps = |a: &Solid, b: &Solid| {
            let ((alo, ahi), (blo, bhi)) = (a.bounds(), b.bounds());
            alo.cmplt(bhi).all() && blo.cmplt(ahi).all()
        };
        if !tools.iter().any(|t| parts.iter().any(|p| overlaps(t, &p.solid))) {
            return Err(Refusal::NotTouching);
        }
        let mut out = Vec::with_capacity(parts.len());
        for part in parts {
            let mut solid = Some(part.solid.clone());
            for t in tools.iter().filter(|t| overlaps(t, &part.solid)) {
                solid = match solid.map(|s| boolean(&s, t, BooleanOp::Difference)) {
                    Some(Ok(rest)) => Some(rest),
                    Some(Err(Refusal::Empty)) | None => None,
                    Some(Err(other)) => return Err(other),
                };
            }
            if let Some(solid) = solid {
                out.push(Part { solid, ..part.clone() });
            }
        }
        if out.is_empty() {
            return Err(Refusal::Empty);
        }
        Ok(out)
    }

    /// Whether a connection can be made.
    fn check_connection(&self, c: &ConnectionDef) -> Result<(), Refusal> {
        if c.primary == c.secondary {
            return Err(Refusal::SameMember);
        }
        match c.kind {
            ConnectionKind::Butt | ConnectionKind::Mitre => self.join(c).map(|_| ()),
            ConnectionKind::Cut => {
                let target = self.elements.get(&c.secondary).ok_or(Refusal::Missing)?;
                let parts = self.body(target)?;
                self.cut(&parts, target, c.primary).map(|_| ())
            }
        }
    }

    /// What the connections joining member `id` do to it (in its tile's frame), where they can be
    /// made: its end planes, and what they add to it and take out of it.
    fn joined(&self, id: ElementId, tile: TileKey) -> Joined {
        let mut joined = Joined::default();
        let offset = self.grid.offset(TileKey::default(), tile);
        for e in self.elements.values() {
            let Shape::Connection(c) = &e.shape else { continue };
            if c.primary != id && c.secondary != id {
                continue;
            }
            let Ok((members, result)) = self.join(c) else { continue };
            let mine = |k: usize| members.get(k) == Some(&id);
            for (k, which, plane) in result.trims {
                if mine(k) {
                    let plane = Plane { point: plane.point + offset, normal: plane.normal };
                    match which {
                        MemberEnd::Start => joined.start = Some(plane),
                        MemberEnd::End => joined.end = Some(plane),
                    }
                }
            }
            joined.additions.extend(result.additions.into_iter().filter(|(k, _)| mine(*k)).map(|(_, s)| s.translated(offset)));
            joined.removals.extend(result.removals.into_iter().filter(|(k, _)| mine(*k)).map(|(_, s)| s.translated(offset)));
        }
        joined
    }

    /// A member of a connection: its definition in the frame of tile (0, 0), so both members
    /// share one frame.
    fn member(&self, id: ElementId) -> Result<LinearDef, Refusal> {
        let e = self.elements.get(&id).ok_or(Refusal::Missing)?;
        let Shape::Linear(l) = &e.shape else { return Err(Refusal::NotLinear) };
        let offset = self.grid.origin(e.tile);
        Ok(LinearDef { start: l.start + offset, end: l.end + offset, ..l.clone() })
    }

    /// What a connection does to its members (their ids, in order) in the frame of tile (0, 0);
    /// nothing for a cut, which [`Model::cut`] makes.
    fn join(&self, c: &ConnectionDef) -> Result<(Vec<ElementId>, JoinResult), Refusal> {
        if c.primary == c.secondary {
            return Err(Refusal::SameMember);
        }
        let kind = match c.kind {
            ConnectionKind::Cut => return Ok((Vec::new(), JoinResult::default())),
            ConnectionKind::Butt => JoinKind::Butt,
            ConnectionKind::Mitre => JoinKind::Mitre,
        };
        let members = [Member::of(&self.member(c.primary)?)?, Member::of(&self.member(c.secondary)?)?];
        Ok((vec![c.primary, c.secondary], join::cuts(kind, &members)?))
    }
}

/// What the connections joining a member do to it, in its tile's frame.
#[derive(Debug, Default)]
struct Joined {
    start: Option<Plane>,
    end: Option<Plane>,
    additions: Vec<Solid>,
    removals: Vec<Solid>,
}

/// A wall's layers reaching past its ends, cut back to its footprint at its corners.
fn joined_parts(p: &crate::element::PlanarDef, join: &crate::joins::WallJoin) -> Result<Vec<(Option<String>, Solid)>, Refusal> {
    let prism = join.prism()?;
    crate::element::planar_parts_extended(p, join.extend)?
        .into_iter()
        .map(|(material, solid)| Ok((material, boolean(&solid, &prism, BooleanOp::Intersection)?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{DVec2, DVec3};
    use crate::element::{PlanarDef, PlanarForm, PointDef, SystemDef};
    use crate::mesh::Mesh;
    use crate::profile::ProfileDef;

    fn beam(start: DVec3, end: DVec3) -> Shape {
        Shape::Linear(LinearDef { start, end, profile: ProfileDef::Rectangle { width: 0.2, height: 0.4 }, roll: 0.0 })
    }

    fn add(model: &mut Model, shape: Shape) -> ElementId {
        let grid = model.grid;
        model.add(Element::in_project(&grid, "e", None, shape)).unwrap()
    }

    #[test]
    fn ids_are_never_reused() {
        let mut m = Model::new(TileGrid::default());
        let a = add(&mut m, beam(DVec3::ZERO, DVec3::X));
        m.remove(a);
        let b = add(&mut m, beam(DVec3::ZERO, DVec3::X));
        assert!(b > a);
    }

    #[test]
    fn a_refused_element_is_not_added() {
        let mut m = Model::new(TileGrid::default());
        let grid = m.grid;
        let bad = Element::in_project(&grid, "e", None, beam(DVec3::ONE, DVec3::ONE));
        assert_eq!(m.add(bad), Err(Refusal::ZeroLength));
        assert!(m.is_empty());
    }

    #[test]
    fn a_butt_stops_the_secondary_at_the_primary_face() {
        let mut m = Model::new(TileGrid::default());
        // a column at the origin, 400 wide along x (its profile's x is the project's x)
        let column = add(
            &mut m,
            Shape::Linear(LinearDef { start: DVec3::ZERO, end: DVec3::new(0.0, 0.0, 3.0), profile: ProfileDef::Rectangle { width: 0.4, height: 0.4 }, roll: 0.0 }),
        );
        let b = add(&mut m, beam(DVec3::new(0.0, 0.0, 2.5), DVec3::new(5.0, 0.0, 2.5)));
        add(&mut m, Shape::Connection(ConnectionDef { kind: ConnectionKind::Butt, primary: column, secondary: b }));
        let (lo, hi) = m.evaluate(b).unwrap()[0].solid.bounds();
        assert!((lo.x - 0.2).abs() < 1e-12 && (hi.x - 5.0).abs() < 1e-12, "{lo} {hi}");
        let (lo, _) = m.evaluate(column).unwrap()[0].solid.bounds();
        assert!((lo.x + 0.2).abs() < 1e-12, "the primary runs through");
    }

    #[test]
    fn a_mitre_cuts_both_members_on_the_bisector() {
        let mut m = Model::new(TileGrid::default());
        let a = add(&mut m, beam(DVec3::new(0.0, 0.0, 0.0), DVec3::new(4.0, 0.0, 0.0)));
        let b = add(&mut m, beam(DVec3::new(4.0, 0.0, 0.0), DVec3::new(4.0, 3.0, 0.0)));
        add(&mut m, Shape::Connection(ConnectionDef { kind: ConnectionKind::Mitre, primary: a, secondary: b }));
        let va = m.evaluate(a).unwrap()[0].solid.volume();
        let vb = m.evaluate(b).unwrap()[0].solid.volume();
        // a mitre on the bisector takes off as much on the inside as it adds on the outside
        assert!((va - 4.0 * 0.2 * 0.4).abs() < 1e-12, "{va}");
        assert!((vb - 3.0 * 0.2 * 0.4).abs() < 1e-12, "{vb}");
        let (_, hi) = m.evaluate(a).unwrap()[0].solid.bounds();
        assert!((hi.x - 4.1).abs() < 1e-12, "the outer corner reaches the outside of the other member: {hi}");
    }

    #[test]
    fn connections_that_cannot_be_made_are_refused() {
        let mut m = Model::new(TileGrid::default());
        let a = add(&mut m, beam(DVec3::ZERO, DVec3::new(4.0, 0.0, 0.0)));
        let parallel = add(&mut m, beam(DVec3::new(0.0, 1.0, 0.0), DVec3::new(4.0, 1.0, 0.0)));
        let far = add(&mut m, beam(DVec3::new(10.0, 5.0, 0.0), DVec3::new(10.0, 8.0, 0.0)));
        let wall = add(
            &mut m,
            Shape::Planar(PlanarDef { form: PlanarForm::Wall { start: DVec2::ZERO, end: DVec2::Y, base: 0.0, height: 1.0 }, thickness: 0.1, openings: Vec::new(), build_up: None }),
        );
        let grid = m.grid;
        let try_join = |m: &mut Model, kind, primary, secondary| m.add(Element::in_project(&grid, "c", None, Shape::Connection(ConnectionDef { kind, primary, secondary })));
        assert_eq!(try_join(&mut m, ConnectionKind::Butt, a, parallel), Err(Refusal::Parallel));
        assert_eq!(try_join(&mut m, ConnectionKind::Butt, a, far), Err(Refusal::NotTouching));
        assert_eq!(try_join(&mut m, ConnectionKind::Mitre, a, far), Err(Refusal::NotTouching));
        assert_eq!(try_join(&mut m, ConnectionKind::Butt, a, a), Err(Refusal::SameMember));
        assert_eq!(try_join(&mut m, ConnectionKind::Butt, a, wall), Err(Refusal::NotLinear));
        assert_eq!(try_join(&mut m, ConnectionKind::Butt, a, ElementId(999)), Err(Refusal::Missing));
    }

    #[test]
    fn removing_a_member_removes_its_connections() {
        let mut m = Model::new(TileGrid::default());
        let a = add(&mut m, beam(DVec3::ZERO, DVec3::new(4.0, 0.0, 0.0)));
        let b = add(&mut m, beam(DVec3::new(4.0, 0.0, 0.0), DVec3::new(4.0, 3.0, 0.0)));
        add(&mut m, Shape::Connection(ConnectionDef { kind: ConnectionKind::Mitre, primary: a, secondary: b }));
        assert_eq!(m.remove(b).len(), 2);
        assert_eq!(m.len(), 1);
        assert!((m.evaluate(a).unwrap()[0].solid.volume() - 4.0 * 0.08).abs() < 1e-12, "square again");
    }

    fn far_wall(o: DVec2) -> Shape {
        Shape::Planar(PlanarDef {
            form: PlanarForm::Wall { start: DVec2::new(0.123, 0.456) + o, end: DVec2::new(7.891, 3.21) + o, base: 0.0, height: 2.8 },
            thickness: 0.3,
            openings: Vec::new(),
            build_up: None,
        })
    }

    #[test]
    fn far_from_the_origin_a_tile_frame_gives_the_same_solid_bit_for_bit() {
        // the same definition in tile (500, 300), 500 km out, as in tile (0, 0): the kernel only
        // ever sees the small local coordinates, so the solids are identical
        let mut m = Model::new(TileGrid::default());
        let near = add(&mut m, far_wall(DVec2::ZERO));
        let mut far = m.get(near).unwrap().clone();
        far.tile = TileKey { x: 500, y: 300 };
        let far = m.add(far).unwrap();
        let (pn, pf) = (&m.evaluate(near).unwrap()[0], &m.evaluate(far).unwrap()[0]);
        assert_eq!(pf.tile, TileKey { x: 500, y: 300 });
        assert_eq!(pn.solid, pf.solid);
        let mesh = Mesh::of(&pf.solid).unwrap();
        assert!(mesh.positions.iter().all(|p| p.length() < 20.0), "mesh coordinates stay small in the tile frame");
    }

    #[test]
    fn far_from_the_origin_project_input_is_off_by_no_more_than_f64_resolves() {
        // typing the wall in project coordinates 500 km out rounds each coordinate once when it is
        // entered (f64 resolves about 6e-11 m there); the tile frame adds nothing to that
        let mut m = Model::new(TileGrid::default());
        let near = add(&mut m, far_wall(DVec2::ZERO));
        let far = add(&mut m, far_wall(DVec2::new(500_000.0, 300_000.0)));
        let (pn, pf) = (&m.evaluate(near).unwrap()[0], &m.evaluate(far).unwrap()[0]);
        let worst = pn.solid.positions.iter().zip(&pf.solid.positions).map(|(a, b)| (*a - *b).length()).fold(0.0, f64::max);
        assert!(worst < 2.0 * 500_000f64 * f64::EPSILON, "{worst}");
        assert!(worst < crate::LINEAR_TOLERANCE * 1e-4);
    }

    #[test]
    fn members_in_different_tiles_join_in_one_frame() {
        let mut m = Model::new(TileGrid::new(10).unwrap());
        // a beam crossing the tile boundary at x = 10 onto a column in the next tile
        let column = add(
            &mut m,
            Shape::Linear(LinearDef { start: DVec3::new(12.0, 0.0, 0.0), end: DVec3::new(12.0, 0.0, 3.0), profile: ProfileDef::Rectangle { width: 0.4, height: 0.4 }, roll: 0.0 }),
        );
        let b = add(&mut m, beam(DVec3::new(6.0, 0.0, 2.5), DVec3::new(12.0, 0.0, 2.5)));
        assert_ne!(m.get(column).unwrap().tile, m.get(b).unwrap().tile);
        add(&mut m, Shape::Connection(ConnectionDef { kind: ConnectionKind::Butt, primary: column, secondary: b }));
        let part = &m.evaluate(b).unwrap()[0];
        let (_, hi) = part.solid.bounds();
        // the beam's tile is (0, 0): its end stops at x = 11.8 in that frame
        assert!((hi.x - 11.8).abs() < 1e-12, "{hi}");
    }

    #[test]
    fn every_base_element_evaluates() {
        let mut m = Model::new(TileGrid::default());
        let point = add(&mut m, Shape::Point(PointDef { at: DVec3::new(1.0, 1.0, 0.0), rotation: 0.5, size: DVec3::new(0.6, 0.6, 0.9) }));
        let system = add(
            &mut m,
            Shape::System(SystemDef { start: DVec2::new(0.0, 5.0), end: DVec2::new(6.0, 5.0), base: 0.0, height: 3.0, spacing: DVec2::new(1.2, 1.5), member: DVec2::new(0.05, 0.15), panel: 0.02 }),
        );
        assert!((m.evaluate(point).unwrap()[0].solid.volume() - 0.6 * 0.6 * 0.9).abs() < 1e-12);
        assert!(m.evaluate(system).unwrap().len() > 10);
        for id in [point, system] {
            for part in m.evaluate(id).unwrap() {
                Mesh::of(&part.solid).unwrap();
            }
        }
        assert_eq!(m.tiles().len(), 1);
    }

    fn wall_along_x() -> Shape {
        Shape::Planar(PlanarDef { form: PlanarForm::Wall { start: DVec2::ZERO, end: DVec2::new(6.0, 0.0), base: 0.0, height: 3.0 }, thickness: 0.2, openings: Vec::new(), build_up: None })
    }

    #[test]
    fn a_beam_through_a_wall_cuts_its_way_through() {
        let mut m = Model::new(TileGrid::default());
        let wall = add(&mut m, wall_along_x());
        let b = add(&mut m, beam(DVec3::new(3.0, -2.0, 2.5), DVec3::new(3.0, 2.0, 2.5)));
        let before = m.evaluate(wall).unwrap()[0].solid.volume();
        add(&mut m, Shape::Connection(ConnectionDef { kind: ConnectionKind::Cut, primary: b, secondary: wall }));
        let after = m.evaluate(wall).unwrap()[0].solid.volume();
        assert!((before - after - 0.2 * 0.4 * 0.2).abs() < 1e-9, "{before} - {after}");
        assert!((m.evaluate(b).unwrap()[0].solid.volume() - 4.0 * 0.08).abs() < 1e-9, "the cutter keeps its body");
        // removing the beam removes the cut, and the wall is whole again
        m.remove(b);
        assert!((m.evaluate(wall).unwrap()[0].solid.volume() - before).abs() < 1e-12);
    }

    #[test]
    fn cuts_that_cannot_be_made_are_refused() {
        let mut m = Model::new(TileGrid::default());
        let wall = add(&mut m, wall_along_x());
        let far = add(&mut m, beam(DVec3::new(3.0, 5.0, 2.5), DVec3::new(3.0, 9.0, 2.5)));
        let block = add(&mut m, Shape::Point(PointDef { at: DVec3::new(3.0, 0.0, -1.0), rotation: 0.0, size: DVec3::new(8.0, 2.0, 6.0) }));
        let grid = m.grid;
        let try_cut = |m: &mut Model, primary, secondary| m.add(Element::in_project(&grid, "c", None, Shape::Connection(ConnectionDef { kind: ConnectionKind::Cut, primary, secondary })));
        assert_eq!(try_cut(&mut m, far, wall), Err(Refusal::NotTouching));
        assert_eq!(try_cut(&mut m, block, wall), Err(Refusal::Empty), "it would take the whole wall");
        assert_eq!(try_cut(&mut m, wall, wall), Err(Refusal::SameMember));
        let joint = try_cut(&mut m, block, far).map(|_| ()).err();
        assert_eq!(joint, Some(Refusal::NotTouching));
    }

    #[test]
    fn two_elements_cutting_each_other_do_not_go_round_in_circles() {
        let mut m = Model::new(TileGrid::default());
        let a = add(&mut m, wall_along_x());
        let b = add(&mut m, Shape::Planar(PlanarDef { form: PlanarForm::Wall { start: DVec2::new(3.0, -2.0), end: DVec2::new(3.0, 2.0), base: 0.0, height: 3.0 }, thickness: 0.2, openings: Vec::new(), build_up: None }));
        add(&mut m, Shape::Connection(ConnectionDef { kind: ConnectionKind::Cut, primary: a, secondary: b }));
        add(&mut m, Shape::Connection(ConnectionDef { kind: ConnectionKind::Cut, primary: b, secondary: a }));
        // each is cut by the other's whole body: the crossing goes from both
        let (va, vb) = (m.evaluate(a).unwrap()[0].solid.volume(), m.evaluate(b).unwrap()[0].solid.volume());
        assert!((va - (6.0 - 0.2) * 0.2 * 3.0).abs() < 1e-9 && (vb - (4.0 - 0.2) * 0.2 * 3.0).abs() < 1e-9, "{va} {vb}");
    }

    #[test]
    fn a_curtain_wall_cut_by_a_beam_loses_the_parts_it_takes() {
        let mut m = Model::new(TileGrid::default());
        let s = add(
            &mut m,
            Shape::System(SystemDef { start: DVec2::ZERO, end: DVec2::new(3.0, 0.0), base: 0.0, height: 3.0, spacing: DVec2::new(1.5, 1.5), member: DVec2::new(0.05, 0.15), panel: 0.02 }),
        );
        let parts = m.evaluate(s).unwrap().len();
        let b = add(&mut m, beam(DVec3::new(0.75, -1.0, 2.0), DVec3::new(0.75, 1.0, 2.0)));
        add(&mut m, Shape::Connection(ConnectionDef { kind: ConnectionKind::Cut, primary: b, secondary: s }));
        let cut = m.evaluate(s).unwrap();
        assert_eq!(cut.len(), parts, "the beam cuts a hole in one panel, no part goes");
        let glass: f64 = cut.iter().filter(|p| p.class == "IfcPlate").map(|p| p.solid.volume()).sum();
        assert!((glass - (4.0 * 1.45 * 1.45 * 0.02 - 0.2 * 0.4 * 0.02)).abs() < 1e-9, "{glass}");
    }

    #[test]
    fn a_model_round_trips_through_json() {
        let mut m = Model::new(TileGrid::default());
        let a = add(&mut m, beam(DVec3::new(1000.5, 2.0, 0.0), DVec3::new(1004.5, 2.0, 0.0)));
        let b = add(&mut m, beam(DVec3::new(1004.5, 2.0, 0.0), DVec3::new(1004.5, 5.0, 0.0)));
        add(&mut m, Shape::Connection(ConnectionDef { kind: ConnectionKind::Mitre, primary: a, secondary: b }));
        let json = serde_json::to_string(&m).unwrap();
        let back: Model = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);
        assert_eq!(back.evaluate(a), m.evaluate(a));
    }
}
