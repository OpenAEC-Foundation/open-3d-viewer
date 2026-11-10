//! Corners of framed systems: where two timber frames meet end to end at a right angle, every face
//! of both ends where the corner puts it, so the frames and their layers meet without a gap or an
//! overlap.
//!
//! - **Which meet.** Two framed systems with one end of each within [`JOIN_TOLERANCE`] of the
//!   other's, no third framed system ending there, on one base, as high as each other and square
//!   to each other. An end against another's side is a T junction ([`super::tee`]). Anything else
//!   is left as it is: three at a point, a corner that is not square.
//! - **Which runs through.** The longer one; of two as long as each other, the one made first. Its
//!   frame runs on to the outer face of the other's frame, which stops against its inner face.
//! - **The layers.** On the outside of the corner the layers of the one that runs through run on
//!   around it, each as far out as it lies from its own frame, and the other's stop against them.
//!   On the inside a layer of the one that runs through stops against what the other has between
//!   its frame and that layer's depth, and a layer of the other stops against the layers it would
//!   cross.
//! - **Backing.** A board on the inside that ends short of the end of the members it is fixed to
//!   gets a member more behind its edge: the corner stud of a frame, a batten more in a service
//!   zone.
//!
//! A corner is made only when both systems can be made with it (an opening too near the end leaves
//! it unmade) and without it, so the two always agree on it and each stays when the other goes.
//! The same holds for a T junction, and a system is made with all it meets together: the corners
//! at its ends, the system it stands against there, and those that stand against its own side.

use super::tee::{self, Tee};
use super::{build, faces, parts, plan, units, Ends, FaceEnd};
use crate::element::{Element, ElementId, PartSolid, Shape, SystemDef};
use crate::model::{Model, JOIN_TOLERANCE};
use crate::tile::TileKey;
use crate::{Refusal, LINEAR_TOLERANCE};
use glam::DVec2;
use std::iter::once;

/// Two systems are square to each other when the cosine of the angle between them is within this.
pub(super) const SQUARE: f64 = 1e-6;

/// How a framed system meets another at one of its ends: the element it meets, and whether it is
/// the one that runs through (the other stops against it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Meeting {
    pub with: ElementId,
    pub through: bool,
}

/// A layer beside a frame: which face of the system it is, from where to where it lies out from
/// the frame's face, the width of its members when it is framed, and whether it is boards.
#[derive(Debug, Clone, Copy)]
pub(super) struct Band {
    pub(super) face: usize,
    pub(super) x1: f64,
    pub(super) x2: f64,
    pub(super) framed: Option<f64>,
    pub(super) boards: bool,
}

/// A system across its depth: half the depth of its frame, the width of its studs, how many faces
/// it has, and the layers to the right of its frame and to the left of it. A layer within the
/// frame's depth ends as the frame does.
pub(super) struct Section {
    pub(super) half: f64,
    pub(super) width: f64,
    pub(super) faces: usize,
    pub(super) sides: [Vec<Band>; 2],
}

impl Section {
    pub(super) fn of(s: &SystemDef) -> Self {
        let faces = faces(s);
        let own = &faces[0];
        let (width, depth) = own.member.map_or((0.0, own.panel.0), |(m, _)| (m.x, m.y.max(own.panel.0)));
        let half = depth / 2.0;
        let mut sides = [Vec::new(), Vec::new()];
        for (i, f) in faces.iter().enumerate().skip(1) {
            let depth = f.member.map_or(f.panel.0, |(m, _)| m.y.max(f.panel.0));
            let x1 = (f.offset.abs() - depth / 2.0 - half).max(0.0);
            let x2 = (f.offset.abs() + depth / 2.0 - half).max(x1);
            if x2 > LINEAR_TOLERANCE {
                let framed = f.framing.and(f.member).map(|(m, _)| m.x);
                sides[usize::from(f.offset < 0.0)].push(Band { face: i, x1, x2, framed, boards: f.member.is_none() });
            }
        }
        Self { half, width, faces: faces.len(), sides }
    }

    /// Where the layers on `side` start and end, out from the frame's face, in order.
    pub(super) fn bounds(&self, side: usize) -> Vec<f64> {
        let mut bounds: Vec<f64> = once(0.0).chain(self.sides[side].iter().flat_map(|l| [l.x1, l.x2])).collect();
        bounds.sort_by(f64::total_cmp);
        bounds
    }
}

/// The last of `bounds` at or before `x`.
pub(super) fn before(bounds: &[f64], x: f64) -> f64 {
    bounds.iter().copied().filter(|b| *b <= x + LINEAR_TOLERANCE).fold(0.0, f64::max)
}

/// The first of `bounds` at or after `x`; the last of them when none is.
pub(super) fn after(bounds: &[f64], x: f64) -> f64 {
    bounds.iter().copied().find(|b| *b >= x - LINEAR_TOLERANCE).or(bounds.last().copied()).unwrap_or(0.0)
}

/// A framed system and the ends of its line, in one tile's frame.
#[derive(Clone, Copy)]
pub(super) struct Framed<'a> {
    pub(super) id: ElementId,
    pub(super) def: &'a SystemDef,
    pub(super) ends: [DVec2; 2],
}

impl<'a> Framed<'a> {
    pub(super) fn of(model: &Model, e: &'a Element, tile: TileKey) -> Option<Self> {
        let Shape::System(s) = &e.shape else { return None };
        Self::new(model, e.id, e.tile, s, tile)
    }

    /// System `s` of element `id`, its coordinates in tile `from`, in the frame of `tile`; `None`
    /// when it is not framed.
    pub(super) fn new(model: &Model, id: ElementId, from: TileKey, s: &'a SystemDef, tile: TileKey) -> Option<Self> {
        s.system_type.as_ref()?.framing?;
        let offset = model.grid.offset(from, tile).truncate();
        let ends = [s.start + offset, s.end + offset];
        ((ends[1] - ends[0]).length() > LINEAR_TOLERANCE).then_some(Self { id, def: s, ends })
    }

    pub(super) fn length(&self) -> f64 {
        (self.ends[1] - self.ends[0]).length()
    }

    /// The direction from end `end` into the system.
    pub(super) fn inward(&self, end: usize) -> DVec2 {
        let dir = (self.ends[1] - self.ends[0]) / self.length();
        if end == 0 { dir } else { -dir }
    }

    /// To the right of its line: its exterior.
    pub(super) fn right(&self) -> DVec2 {
        let dir = self.inward(0);
        DVec2::new(dir.y, -dir.x)
    }
}

/// The one other framed system with an end at end `end` of `me`, and which of its ends; `None`
/// when there is none or more than one.
fn at_end<'a>(model: &'a Model, me: &Framed<'_>, end: usize, tile: TileKey) -> Option<(Framed<'a>, usize)> {
    let mut found = None;
    for e in model.iter().filter(|e| e.id != me.id) {
        let Some(other) = Framed::of(model, e, tile) else { continue };
        for k in 0..2 {
            if (other.ends[k] - me.ends[end]).length() <= JOIN_TOLERANCE {
                if found.is_some() {
                    return None;
                }
                found = Some((other, k));
            }
        }
    }
    found
}

/// A corner as one of its two systems sees it: which of its ends, the other system and its end,
/// whether this one runs through, which side of each is the outside of the corner (0 its right, 1
/// its left), and how far the other's end lies past this one's, along this one's line.
#[derive(Clone, Copy)]
pub(super) struct Join<'a> {
    end: usize,
    other: Framed<'a>,
    other_end: usize,
    through: bool,
    outer: usize,
    other_outer: usize,
    shift: f64,
}

/// The corner of end `end` of `me` and end `other_end` of `other`, which lie at one point; `None`
/// when they are not on one base, not as high as each other or not square to each other.
fn join<'a>(me: &Framed<'_>, end: usize, other: Framed<'a>, other_end: usize) -> Option<Join<'a>> {
    let tol = LINEAR_TOLERANCE;
    if (me.def.base - other.def.base).abs() > tol || (me.def.height - other.def.height).abs() > tol {
        return None;
    }
    let (a, b) = (me.inward(end), other.inward(other_end));
    if a.dot(b).abs() > SQUARE {
        return None;
    }
    let (mine, theirs) = (me.length(), other.length());
    let through = mine > theirs + tol || (mine >= theirs - tol && me.id < other.id);
    // the outside of the corner is the side away from the other system
    let outer = usize::from(me.right().dot(b) > 0.0);
    let other_outer = usize::from(other.right().dot(a) > 0.0);
    Some(Join { end, other, other_end, through, outer, other_outer, shift: (me.ends[end] - other.ends[other_end]).dot(a) })
}

/// The corner at end `end` of `me`, as the systems lie: one other framed system ends there, and
/// `me` is the one system that ends at that end of it.
fn geometric<'a>(model: &'a Model, me: &Framed<'_>, end: usize, tile: TileKey) -> Option<Join<'a>> {
    let (other, other_end) = at_end(model, me, end, tile)?;
    let (back, back_end) = at_end(model, &other, other_end, tile)?;
    if back.id != me.id || back_end != end {
        return None;
    }
    join(me, end, other, other_end)
}

/// What a framed system meets: a corner at its start and at its end, or the system against whose
/// side it stands there, and the systems that stand against its own side, in order along it.
#[derive(Clone, Default)]
pub(super) struct Joins<'a> {
    pub(super) corners: [Option<Join<'a>>; 2],
    pub(super) butts: [Option<Tee<'a>>; 2],
    pub(super) sides: Vec<Tee<'a>>,
}

impl<'a> Joins<'a> {
    /// Corner `j` alone, at its end.
    fn corner(j: Join<'a>) -> Self {
        let mut joins = Self::default();
        joins.corners[j.end] = Some(j);
        joins
    }

    /// T junction `t` alone, as the system that stands against the other sees it.
    fn butt(t: Tee<'a>) -> Self {
        let mut joins = Self::default();
        joins.butts[t.end] = Some(t);
        joins
    }

    /// T junction `t` alone, as the system that runs through sees it.
    fn side(t: Tee<'a>) -> Self {
        Self { sides: vec![t], ..Self::default() }
    }

    fn is_empty(&self) -> bool {
        self.corners.iter().all(Option::is_none) && self.butts.iter().all(Option::is_none) && self.sides.is_empty()
    }
}

/// How far short of the end of face `face`, its members `w` wide, the first board of `layers` out
/// from `from` ends, where that is a member's width or more: there a member more backs the board's
/// edge. The faces end as `ends` say.
pub(super) fn short(layers: &[Band], ends: &[FaceEnd], face: usize, w: f64, from: f64) -> Option<f64> {
    let tol = LINEAR_TOLERANCE;
    let board = layers.iter().filter(|l| l.boards && l.x1 >= from - tol).min_by(|a, b| a.x1.total_cmp(&b.x1))?;
    Some(ends[face].reach - ends[board.face].reach).filter(|d| *d >= w - tol)
}

/// How the faces of a system with section `mine` end at corner `j` with a system with section
/// `theirs`: one for every face.
fn face_ends(mine: &Section, theirs: &Section, j: &Join) -> Vec<FaceEnd> {
    let h = theirs.half;
    // the frame, and what lies within its depth: on to the other frame's outer face, or up to
    // this side of it
    let mut ends = vec![FaceEnd { reach: if j.through { h } else { -h }, backing: None }; mine.faces];
    let (their_outer, their_inner) = (theirs.bounds(j.other_outer), theirs.bounds(1 - j.other_outer));
    let around = their_outer.last().copied().unwrap_or(0.0);
    for l in &mine.sides[j.outer] {
        ends[l.face].reach = h + if j.through { l.x2.min(around) } else { before(&their_outer, l.x1) };
    }
    let inner = &mine.sides[1 - j.outer];
    for l in inner {
        ends[l.face].reach = -(h + if j.through { before(&their_inner, l.x1) } else { after(&their_inner, l.x2) });
    }
    // the boards on the inside are fixed to the members behind them: the frame's studs, or a framed
    // layer's; where they end a member's width or more short of those, a member more backs them
    let framed = inner.iter().filter_map(|l| l.framed.map(|w| (l.face, w, l.x2)));
    for (face, w, from) in once((0, mine.width, 0.0)).chain(framed) {
        if let Some(d) = short(inner, &ends, face, w, from) {
            ends[face].backing = Some(d);
        }
    }
    for e in &mut ends {
        e.reach += j.shift;
    }
    ends
}

/// The ends of the faces of system `s` with what it meets, `joins`; `None` where it cannot stand
/// against another's side as its layers lie.
fn ends_of(s: &SystemDef, joins: &Joins) -> Option<Ends> {
    let mine = Section::of(s);
    let at = |end: usize| match (&joins.corners[end], &joins.butts[end]) {
        (Some(j), _) => Some(face_ends(&mine, &Section::of(j.other.def), j)),
        (None, Some(t)) => tee::butt_ends(&mine, &Section::of(t.runs.def), t),
        (None, None) => Some(Vec::new()),
    };
    let breaks = joins.sides.iter().map(|t| tee::breaks(&mine, &Section::of(t.stands.def), t)).collect();
    Some(Ends { start: at(0)?, end: at(1)?, breaks })
}

/// Whether system `s` can be made with what it meets, `joins`.
fn fits(s: &SystemDef, joins: &Joins) -> bool {
    ends_of(s, joins).is_some_and(|ends| plan(s, &ends).is_ok())
}

/// The corners and the T junctions of `me` that could be made: each one alone fits both of its
/// systems, and both can be made without it too, so that each stays when the other goes.
fn candidates<'a>(model: &'a Model, me: &Framed<'a>, tile: TileKey) -> Joins<'a> {
    let makes = |s: &SystemDef, joins: Joins| fits(s, &Joins::default()) && fits(s, &joins);
    let corners = [0, 1].map(|end| geometric(model, me, end, tile).filter(|j| makes(me.def, Joins::corner(*j)) && join(&j.other, j.other_end, *me, j.end).is_some_and(|back| makes(j.other.def, Joins::corner(back)))));
    let made = |t: &Tee| makes(t.runs.def, Joins::side(*t)) && makes(t.stands.def, Joins::butt(*t));
    let butts = [0, 1].map(|end| tee::geometric(model, *me, end, tile).filter(|t| made(t)));
    let sides = tee::standing(model, *me, tile).into_iter().filter(|t| made(t)).collect();
    Joins { corners, butts, sides }
}

/// The corners and the T junctions of `me` that are made: those that could be, where `me` fits
/// with all of its own and the other system with all of its own. Both systems of a junction come
/// to the same answer.
pub(super) fn applied<'a>(model: &'a Model, me: &Framed<'a>, tile: TileKey) -> Joins<'a> {
    let mine = candidates(model, me, tile);
    if !fits(me.def, &mine) {
        return Joins::default();
    }
    let stays = |other: &Framed<'a>| fits(other.def, &candidates(model, other, tile));
    Joins {
        corners: mine.corners.map(|j| j.filter(|j| stays(&j.other))),
        butts: mine.butts.map(|t| t.filter(|t| stays(&t.runs))),
        sides: mine.sides.into_iter().filter(|t| stays(&t.stands)).collect(),
    }
}

/// The ends of the faces of system `s` of `element` at what it meets in `model`; `None` when it
/// meets nothing.
fn joined_ends(model: &Model, element: &Element, s: &SystemDef) -> Option<Ends> {
    let me = Framed::new(model, element.id, element.tile, s, element.tile)?;
    let joins = applied(model, &me, element.tile);
    if joins.is_empty() {
        return None;
    }
    ends_of(s, &joins)
}

/// The members and panels of system `s` of `element`, its faces ending at its corners and its T
/// junctions with the other framed systems of `model`. A system that cannot be made with them
/// after all is made as it is without them.
pub(crate) fn joined_parts(model: &Model, element: &Element, s: &SystemDef) -> Result<Vec<PartSolid>, Refusal> {
    match joined_ends(model, element, s) {
        Some(ends) => build(s, &ends).or_else(|_| parts(s)),
        None => parts(s),
    }
}

/// The lengths of the prefab units `element` is made of in `model`, from its frame's start: as
/// [`units`], with its frame ending at its corners and its T junctions. None for an element that
/// is not a system.
pub fn joined_units(model: &Model, element: &Element) -> Result<Vec<f64>, Refusal> {
    let Shape::System(s) = &element.shape else { return Ok(Vec::new()) };
    match joined_ends(model, element, s).and_then(|ends| plan(s, &ends).ok()) {
        Some(plan) => Ok(plan.units()),
        None => units(s),
    }
}

/// The corners `element` makes with other framed systems of `model`: at its start and at its end.
pub fn corners(model: &Model, element: &Element) -> [Option<Meeting>; 2] {
    let Shape::System(s) = &element.shape else { return [None, None] };
    let Some(me) = Framed::new(model, element.id, element.tile, s, element.tile) else { return [None, None] };
    applied(model, &me, element.tile).corners.map(|j| j.map(|j| Meeting { with: j.other.id, through: j.through }))
}

#[cfg(test)]
pub(super) mod tests {
    use super::super::{parts, tees, units, Tees};
    use super::{corners, joined_units, Meeting};
    use crate::element::{Opening, Shape, SystemDef};
    use crate::model::Model;
    use crate::tile::TileGrid;
    use crate::{Element, ElementId, Refusal};
    use bim_library::Library;
    use glam::{DVec2, DVec3};

    pub(in crate::system) const EXTERIOR: &str = "system.timber-frame-140";
    pub(in crate::system) const INTERIOR: &str = "system.timber-frame-internal-89";
    /// Where the corner of [`corner`] lies along x.
    pub(in crate::system) const L: f64 = 4.8;
    /// The width of a stud and of a service batten.
    pub(in crate::system) const STUD: f64 = 0.038;
    pub(in crate::system) const BATTEN: f64 = 0.045;

    pub(in crate::system) fn def(a: (f64, f64), b: (f64, f64), height: f64, id: &str) -> SystemDef {
        let s = SystemDef { start: DVec2::new(a.0, a.1), end: DVec2::new(b.0, b.1), base: 0.0, height, spacing: DVec2::new(1.5, 1.5), member: DVec2::new(0.05, 0.15), panel: 0.02, system_type: None, openings: Vec::new() };
        s.typed_from(&Library::starter(), id).unwrap()
    }

    /// An exterior timber frame 2.7 m high from `a` to `b`.
    pub(in crate::system) fn frame(a: (f64, f64), b: (f64, f64)) -> Shape {
        Shape::System(def(a, b, 2.7, EXTERIOR))
    }

    pub(in crate::system) fn add(m: &mut Model, shape: Shape) -> ElementId {
        let grid = m.grid;
        m.add(Element::in_project(&grid, "w", None, shape)).unwrap()
    }

    /// Two exterior frames at a corner: the first along +x to (4.8, 0), the second from there
    /// along +y, 3 m long.
    fn corner() -> (Model, ElementId, ElementId) {
        let mut m = Model::new(TileGrid::default());
        let x = add(&mut m, frame((0.0, 0.0), (L, 0.0)));
        let y = add(&mut m, frame((L, 0.0), (L, 3.0)));
        (m, x, y)
    }

    /// A part's material and its bounds in project coordinates.
    #[derive(Debug, Clone, PartialEq)]
    pub(in crate::system) struct Piece {
        pub(in crate::system) material: Option<String>,
        pub(in crate::system) lo: DVec3,
        pub(in crate::system) hi: DVec3,
    }

    impl Piece {
        pub(in crate::system) fn is(&self, material: &str) -> bool {
            self.material.as_deref() == Some(material)
        }

        pub(in crate::system) fn size(&self, axis: usize) -> f64 {
            self.hi[axis] - self.lo[axis]
        }

        /// A member of `material`, `deep` on `axis` (across its face).
        pub(in crate::system) fn member(&self, material: &str, axis: usize, deep: f64) -> bool {
            self.is(material) && (self.size(axis) - deep).abs() < 1e-9
        }
    }

    pub(in crate::system) fn pieces(m: &Model, id: ElementId) -> Vec<Piece> {
        let piece = |p: crate::model::Part| {
            let (origin, (lo, hi)) = (m.grid.origin(p.tile), p.solid.bounds());
            Piece { material: p.material, lo: lo + origin, hi: hi + origin }
        };
        m.evaluate(id).unwrap().into_iter().map(piece).collect()
    }

    /// From where to where the pieces of `material` lie along `axis`; of those `deep` on another
    /// axis only, when given (a frame's studs and plates, a service zone's battens).
    pub(in crate::system) fn extent(pieces: &[Piece], material: &str, axis: usize, deep: Option<(usize, f64)>) -> (f64, f64) {
        let picked = pieces.iter().filter(|p| deep.map_or(p.is(material), |(across, d)| p.member(material, across, d)));
        picked.fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.lo[axis]), hi.max(p.hi[axis])))
    }

    pub(in crate::system) fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
    }

    pub(in crate::system) fn overlapping(a: &Piece, b: &Piece) -> bool {
        (0..3).all(|k| a.lo[k].max(b.lo[k]) + 1e-9 < a.hi[k].min(b.hi[k]))
    }

    /// The first piece of `a` and piece of `b` that lie in one another.
    pub(in crate::system) fn overlap(a: &[Piece], b: &[Piece]) -> Option<(Piece, Piece)> {
        a.iter().flat_map(|p| b.iter().map(move |q| (p, q))).find(|(p, q)| overlapping(p, q)).map(|(p, q)| (p.clone(), q.clone()))
    }

    /// The first two pieces of `a` that lie in one another.
    pub(in crate::system) fn overlap_within(a: &[Piece]) -> Option<(Piece, Piece)> {
        a.iter().enumerate().flat_map(|(i, p)| a[i + 1..].iter().map(move |q| (p, q))).find(|(p, q)| overlapping(p, q)).map(|(p, q)| (p.clone(), q.clone()))
    }

    pub(in crate::system) fn system(m: &Model, id: ElementId) -> &SystemDef {
        match &m.get(id).unwrap().shape {
            Shape::System(s) => s,
            _ => panic!("not a system"),
        }
    }

    /// Whether element `id` is made as it is alone: no corner at either end, no T junction.
    pub(in crate::system) fn plain(m: &Model, id: ElementId) -> bool {
        let made: Vec<_> = m.evaluate(id).unwrap().iter().map(|p| p.solid.bounds()).collect();
        let alone: Vec<_> = parts(system(m, id)).unwrap().iter().map(|p| p.solid.bounds()).collect();
        made == alone && corners(m, m.get(id).unwrap()) == [None, None] && tees(m, m.get(id).unwrap()) == Tees::default()
    }

    #[test]
    fn the_longer_frame_runs_through_a_corner() {
        let (m, x, y) = corner();
        assert_eq!(corners(&m, m.get(x).unwrap()), [None, Some(Meeting { with: y, through: true })]);
        assert_eq!(corners(&m, m.get(y).unwrap()), [Some(Meeting { with: x, through: false }), None]);
        let (xs, ys) = (pieces(&m, x), pieces(&m, y));
        // the frame that runs through reaches the outer face of the other's frame, which starts at
        // its inner face
        assert!(close(extent(&xs, "timber", 0, Some((1, 0.14))), (0.0, L + 0.07)), "{:?}", extent(&xs, "timber", 0, Some((1, 0.14))));
        assert!(close(extent(&ys, "timber", 1, Some((0, 0.14))), (0.07, 3.0)), "{:?}", extent(&ys, "timber", 1, Some((0, 0.14))));
        assert_eq!(overlap(&xs, &ys), None);
        assert_eq!((overlap_within(&xs), overlap_within(&ys)), (None, None));
        // the shorter one runs through when it is the longer: as long as each other, the first made
        for (first, second, through) in [(2.4, 3.0, false), (3.0, 3.0, true)] {
            let mut m = Model::new(TileGrid::default());
            let x = add(&mut m, frame((0.0, 0.0), (first, 0.0)));
            let y = add(&mut m, frame((first, 0.0), (first, second)));
            assert_eq!(corners(&m, m.get(x).unwrap())[1], Some(Meeting { with: y, through }), "{first} and {second}");
            assert_eq!(corners(&m, m.get(y).unwrap())[0], Some(Meeting { with: x, through: !through }), "{first} and {second}");
            assert_eq!(overlap(&pieces(&m, x), &pieces(&m, y)), None);
        }
        let mut m = Model::new(TileGrid::default());
        let y = add(&mut m, frame((3.0, 0.0), (3.0, 3.0)));
        let x = add(&mut m, frame((0.0, 0.0), (3.0, 0.0)));
        assert_eq!(corners(&m, m.get(y).unwrap())[0], Some(Meeting { with: x, through: true }));
    }

    #[test]
    fn the_layers_of_a_corner_nest() {
        let (m, x, y) = corner();
        let (xs, ys) = (pieces(&m, x), pieces(&m, y));
        // the one that runs through: its membrane around the corner outside; inside, its plywood
        // against the other's frame and every layer after it against the other's layers before it
        let ends = [("breather-membrane", None, L + 0.0705), ("plywood", None, L - 0.07), ("vapour-control-membrane", None, L - 0.088), ("timber", Some((1, BATTEN)), L - 0.0882), ("gypsum-board", None, L - 0.1332)];
        for (material, deep, end) in ends {
            let (from, to) = extent(&xs, material, 0, deep);
            assert!(close((from, to), (0.0, end)), "{material}: {from} to {to}");
        }
        // the one that stops: its membrane from the other's frame face, its layers inside each
        // from the far face of the other's layer it would cross
        let starts = [("breather-membrane", None, -0.07), ("plywood", None, 0.088), ("vapour-control-membrane", None, 0.0882), ("timber", Some((0, BATTEN)), 0.1332), ("gypsum-board", None, 0.1457)];
        for (material, deep, start) in starts {
            let (from, to) = extent(&ys, material, 1, deep);
            assert!(close((from, to), (start, 3.0)), "{material}: {from} to {to}");
        }
    }

    #[test]
    fn a_corner_is_made_whichever_way_its_frames_are_drawn() {
        for case in 0..8 {
            let (flip_x, down, flip_y) = (case & 1 != 0, case & 2 != 0, case & 4 != 0);
            let far = if down { -3.0 } else { 3.0 };
            let (xa, xb) = if flip_x { ((L, 0.0), (0.0, 0.0)) } else { ((0.0, 0.0), (L, 0.0)) };
            let (ya, yb) = if flip_y { ((L, far), (L, 0.0)) } else { ((L, 0.0), (L, far)) };
            let mut m = Model::new(TileGrid::default());
            let x = add(&mut m, frame(xa, xb));
            let y = add(&mut m, frame(ya, yb));
            let mut expected = [None, None];
            expected[usize::from(!flip_x)] = Some(Meeting { with: y, through: true });
            assert_eq!(corners(&m, m.get(x).unwrap()), expected, "case {case}");
            let mut expected = [None, None];
            expected[usize::from(flip_y)] = Some(Meeting { with: x, through: false });
            assert_eq!(corners(&m, m.get(y).unwrap()), expected, "case {case}");
            let (xs, ys) = (pieces(&m, x), pieces(&m, y));
            assert!(close(extent(&xs, "timber", 0, Some((1, 0.14))), (0.0, L + 0.07)), "case {case}");
            let stops = if down { (-3.0, -0.07) } else { (0.07, 3.0) };
            assert!(close(extent(&ys, "timber", 1, Some((0, 0.14))), stops), "case {case}");
            assert_eq!(overlap(&xs, &ys), None, "case {case}");
            assert_eq!((overlap_within(&xs), overlap_within(&ys)), (None, None), "case {case}");
        }
    }

    #[test]
    fn a_member_more_stands_behind_a_board_that_ends_short_of_its_frame() {
        let (m, x, y) = corner();
        let (xs, ys) = (pieces(&m, x), pieces(&m, y));
        let upright = |deep: f64, from: f64, to: f64| xs.iter().any(|p| p.member("timber", 1, deep) && p.size(2) > 2.0 && close((p.lo.x, p.hi.x), (from, to)));
        // the frame's end stud at the corner, and the corner stud behind the plywood's edge
        assert!(upright(0.14, L + 0.07 - STUD, L + 0.07) && upright(0.14, L - 0.07 - STUD, L - 0.07));
        // the pocket between the two is insulated, either side of the stud on the frame's line there
        let wool = |from: f64, to: f64| xs.iter().any(|p| p.member("mineral-wool", 1, 0.14) && close((p.lo.x, p.hi.x), (from, to)));
        assert!(upright(0.14, L - STUD / 2.0, L + STUD / 2.0));
        assert!(wool(L - 0.07, L - STUD / 2.0) && wool(L + STUD / 2.0, L + 0.07 - STUD));
        // the service zone's end batten, and a batten more behind the gypsum board's edge
        assert!(upright(BATTEN, L - 0.0882 - BATTEN, L - 0.0882) && upright(BATTEN, L - 0.1332 - BATTEN, L - 0.1332));
        // the frame that stops needs none: its end stud is behind the edges of its boards
        let studs = ys.iter().filter(|p| p.member("timber", 0, 0.14) && p.size(2) > 2.0).count();
        assert_eq!(studs, 2 + 4);
    }

    #[test]
    fn the_boards_of_a_frame_that_stops_at_a_corner_keep_their_seams_on_its_studs() {
        let (m, _, y) = corner();
        let ys = pieces(&m, y);
        // the studs at their centres from the frame's own start, 70 mm along the line
        let centres = |deep: f64| -> Vec<f64> { ys.iter().filter(|p| p.member("timber", 0, deep) && p.size(2) > 2.0).map(|p| (p.lo.y + p.hi.y) / 2.0).collect() };
        for line in [0.67, 1.27, 1.87, 2.47] {
            assert!(centres(0.14).iter().any(|c| (c - line).abs() < 1e-9), "a stud at {line}: {:?}", centres(0.14));
            assert!(centres(BATTEN).iter().any(|c| (c - line).abs() < 1e-9), "a batten at {line}: {:?}", centres(BATTEN));
        }
        // the first board cut to reach a stud line, whole boards from there
        for (material, start) in [("plywood", 0.088), ("gypsum-board", 0.1457)] {
            let mut boards: Vec<(f64, f64)> = ys.iter().filter(|p| p.is(material)).map(|p| (p.lo.y, p.hi.y)).collect();
            boards.sort_by(|a, b| a.0.total_cmp(&b.0));
            let expected = [(start, 1.27), (1.27, 2.47), (2.47, 3.0)];
            assert!(boards.len() == 3 && boards.iter().zip(expected).all(|(a, b)| close(*a, b)), "{material}: {boards:?}");
        }
    }

    #[test]
    fn frames_that_do_not_meet_in_a_simple_corner_are_left_as_they_are() {
        // three at one point
        let (mut m, x, y) = corner();
        let z = add(&mut m, frame((L, 0.0), (L + 3.0, 0.0)));
        assert!([x, y, z].into_iter().all(|id| plain(&m, id)));
        let others = [
            // higher, and on another base
            Shape::System(def((L, 0.0), (L, 3.0), 3.0, EXTERIOR)),
            Shape::System(SystemDef { base: 0.5, ..def((L, 0.0), (L, 3.0), 2.7, EXTERIOR) }),
            // not square
            frame((L, 0.0), (L + 2.0, 2.0)),
            // across the other
            frame((2.4, -1.5), (2.4, 1.5)),
            // a system that is not framed
            Shape::System(def((L, 0.0), (L, 3.6), 2.7, "system.curtain-wall-aluminium-1500")),
        ];
        for (i, other) in others.into_iter().enumerate() {
            let mut m = Model::new(TileGrid::default());
            let x = add(&mut m, frame((0.0, 0.0), (L, 0.0)));
            let y = add(&mut m, other);
            assert!(plain(&m, x) && plain(&m, y), "case {i}");
        }
    }

    #[test]
    fn a_corner_follows_its_frames_as_they_change() {
        let (mut m, x, y) = corner();
        let heightened = |m: &Model, height: f64| {
            let mut e = m.get(y).unwrap().clone();
            e.shape = Shape::System(SystemDef { height, ..system(m, y).clone() });
            e
        };
        let e = heightened(&m, 3.0);
        m.replace(e).unwrap();
        assert!(plain(&m, x) && plain(&m, y));
        let e = heightened(&m, 2.7);
        m.replace(e).unwrap();
        assert_eq!(corners(&m, m.get(x).unwrap())[1], Some(Meeting { with: y, through: true }));
        assert_eq!(overlap(&pieces(&m, x), &pieces(&m, y)), None);
        m.remove(y);
        assert!(plain(&m, x));
    }

    #[test]
    fn a_corner_an_opening_leaves_no_room_for_is_not_made() {
        let window = |along: f64| Opening { along, sill: 0.9, width: 1.2, height: 1.2 };
        let opened = |a: (f64, f64), b: (f64, f64), along: f64| Shape::System(SystemDef { openings: vec![window(along)], ..def(a, b, 2.7, EXTERIOR) });
        // a window whose framing would stand in the corner's: both frames stay as they are
        let mut m = Model::new(TileGrid::default());
        let x = add(&mut m, opened((0.0, 0.0), (L, 0.0), 3.4));
        let y = add(&mut m, frame((L, 0.0), (L, 3.0)));
        assert!(plain(&m, x) && plain(&m, y));
        // with room for it the corner is made, and the opening stays where it is on the line
        let mut m = Model::new(TileGrid::default());
        let x = add(&mut m, opened((0.0, 0.0), (L, 0.0), 3.2));
        let y = add(&mut m, opened((L, 0.0), (L, 3.0), 1.0));
        assert_eq!(corners(&m, m.get(x).unwrap())[1], Some(Meeting { with: y, through: true }));
        let (xs, ys) = (pieces(&m, x), pieces(&m, y));
        assert_eq!(overlap(&xs, &ys), None);
        assert_eq!((overlap_within(&xs), overlap_within(&ys)), (None, None));
        let void = |axis: usize, from: f64| Piece { material: None, lo: DVec3::splat(-1.0).with_z(0.9), hi: DVec3::splat(5.0).with_z(2.1) }.along(axis, from, from + 1.2);
        assert_eq!(xs.iter().find(|p| overlapping(p, &void(0, 3.2))), None);
        assert_eq!(ys.iter().find(|p| overlapping(p, &void(1, 1.0))), None);
        // the trimmers either side of the second one's opening
        let stud = |from: f64| ys.iter().any(|p| p.member("timber", 0, 0.14) && close((p.lo.y, p.hi.y), (from, from + STUD)));
        assert!(stud(1.0 - STUD) && stud(2.2));
    }

    #[test]
    fn a_frame_that_cannot_be_made_alone_is_not_made_at_a_corner_either() {
        // bare frames, no layers: one that runs through is 70 mm longer at its corner and has no
        // corner stud, so a window too near its end to be framed alone would find room there
        let bare = |a: (f64, f64), b: (f64, f64), along: Option<f64>| {
            let mut s = def(a, b, 2.7, EXTERIOR);
            s.system_type.as_mut().unwrap().layers.clear();
            s.openings = along.map(|along| Opening { along, sill: 0.9, width: 1.2, height: 1.2 }).into_iter().collect();
            Shape::System(s)
        };
        let grid = TileGrid::default();
        let lone = |along: f64| Model::new(grid).add(Element::in_project(&grid, "w", None, bare((0.0, 0.0), (L, 0.0), Some(along))));
        assert!(lone(3.4).is_ok() && lone(3.52).is_err());
        let mut m = Model::new(grid);
        let y = add(&mut m, bare((L, 0.0), (L, 3.0), None));
        let x = add(&mut m, bare((0.0, 0.0), (L, 0.0), Some(3.4)));
        let through = [None, Some(Meeting { with: y, through: true })];
        assert_eq!(corners(&m, m.get(x).unwrap()), through);
        // its window moved to where only the corner leaves room for it: refused, as it is alone,
        // or the frame would be lost when the other one goes
        let mut moved = m.get(x).unwrap().clone();
        moved.shape = bare((0.0, 0.0), (L, 0.0), Some(3.52));
        assert_eq!(m.replace(moved.clone()), Err(Refusal::OpeningOutside));
        assert_eq!(corners(&m, m.get(x).unwrap()), through);
        // read from a file it is kept, not made, and the other frame is made as it is alone
        m.insert(moved);
        assert_eq!(m.evaluate(x).err(), Some(Refusal::OpeningOutside));
        assert_eq!(corners(&m, m.get(x).unwrap()), [None, None]);
        assert!(plain(&m, y));
    }

    impl Piece {
        /// This piece from `from` to `to` along `axis`.
        pub(in crate::system) fn along(mut self, axis: usize, from: f64, to: f64) -> Self {
            self.lo[axis] = from;
            self.hi[axis] = to;
            self
        }
    }

    #[test]
    fn the_prefab_units_of_a_frame_follow_its_corners() {
        let near = |a: &[f64], b: &[f64]| a.len() == b.len() && a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-9);
        let (m, x, y) = corner();
        let lengths = |m: &Model, id: ElementId| joined_units(m, m.get(id).unwrap()).unwrap();
        // 4.87 m with the corner: two units; the one that stops is 2.93 m, one unit
        assert!(near(&lengths(&m, x), &[2.4, 2.47]), "{:?}", lengths(&m, x));
        assert!(near(&lengths(&m, y), &[2.93]), "{:?}", lengths(&m, y));
        // a frame that stops and is made in units: its joints on its own stud lines, end studs
        // either side of them, and no board or membrane across them
        let mut m = Model::new(TileGrid::default());
        let x = add(&mut m, frame((0.0, 0.0), (9.6, 0.0)));
        let y = add(&mut m, frame((9.6, 0.0), (9.6, 7.2)));
        assert_eq!(corners(&m, m.get(y).unwrap())[0], Some(Meeting { with: x, through: false }));
        assert!(near(&lengths(&m, y), &[2.4, 2.4, 2.33]), "{:?}", lengths(&m, y));
        let ys = pieces(&m, y);
        for joint in [2.47, 4.87] {
            let stud = |from: f64| ys.iter().any(|p| p.member("timber", 0, 0.14) && p.size(2) > 2.0 && close((p.lo.y, p.hi.y), (from, from + STUD)));
            assert!(stud(joint - STUD) && stud(joint), "end studs either side of {joint}");
            for material in ["plywood", "gypsum-board", "breather-membrane", "vapour-control-membrane"] {
                assert!(ys.iter().filter(|p| p.is(material)).all(|p| p.hi.y <= joint + 1e-9 || p.lo.y >= joint - 1e-9), "{material} across {joint}");
            }
        }
        assert_eq!(overlap(&pieces(&m, x), &ys), None);
        // a frame alone, and anything that is not a system
        let mut m = Model::new(TileGrid::default());
        let x = add(&mut m, frame((0.0, 0.0), (7.2, 0.0)));
        assert_eq!(lengths(&m, x), units(system(&m, x)).unwrap());
    }

    #[test]
    fn a_corner_is_made_across_tiles() {
        let mut m = Model::new(TileGrid::default());
        let x = add(&mut m, frame((1000.0 - L, 0.0), (1000.0, 0.0)));
        let y = add(&mut m, frame((1000.0, 0.0), (1000.0, 3.0)));
        assert_ne!(m.get(x).unwrap().tile, m.get(y).unwrap().tile);
        assert_eq!(corners(&m, m.get(x).unwrap())[1], Some(Meeting { with: y, through: true }));
        assert_eq!(corners(&m, m.get(y).unwrap())[0], Some(Meeting { with: x, through: false }));
        let (xs, ys) = (pieces(&m, x), pieces(&m, y));
        assert!(close(extent(&xs, "timber", 0, Some((1, 0.14))), (1000.0 - L, 1000.07)));
        assert!(close(extent(&ys, "timber", 1, Some((0, 0.14))), (0.07, 3.0)));
        assert_eq!(overlap(&xs, &ys), None);
    }

    #[test]
    fn ends_that_meet_within_the_join_tolerance_make_a_corner() {
        let mut m = Model::new(TileGrid::default());
        let x = add(&mut m, frame((0.0, 0.0), (L, 0.0)));
        let y = add(&mut m, frame((L + 0.0005, 0.0003), (L + 0.0005, 3.0)));
        assert_eq!(corners(&m, m.get(x).unwrap())[1], Some(Meeting { with: y, through: true }));
        // each ends at the other's face, where that is
        let (xs, ys) = (pieces(&m, x), pieces(&m, y));
        assert!(close(extent(&xs, "timber", 0, Some((1, 0.14))), (0.0, L + 0.0005 + 0.07)));
        assert!(close(extent(&ys, "timber", 1, Some((0, 0.14))), (0.07, 3.0)));
        assert_eq!(overlap(&xs, &ys), None);
        // farther apart they do not meet
        let mut m = Model::new(TileGrid::default());
        let x = add(&mut m, frame((0.0, 0.0), (L, 0.0)));
        let y = add(&mut m, frame((L + 0.002, 0.0), (L + 0.002, 3.0)));
        assert!(plain(&m, x) && plain(&m, y));
    }

    #[test]
    fn frames_of_two_types_meet_in_a_corner() {
        for (long, short) in [(EXTERIOR, INTERIOR), (INTERIOR, EXTERIOR)] {
            let depth = |id: &str| if id == EXTERIOR { 0.14 } else { 0.089 };
            let mut m = Model::new(TileGrid::default());
            let x = add(&mut m, Shape::System(def((0.0, 0.0), (L, 0.0), 2.7, long)));
            let y = add(&mut m, Shape::System(def((L, 0.0), (L, 3.0), 2.7, short)));
            assert_eq!(corners(&m, m.get(x).unwrap())[1], Some(Meeting { with: y, through: true }), "{long}");
            let (xs, ys) = (pieces(&m, x), pieces(&m, y));
            // each frame ends at a face of the other's, whatever their depths
            assert!(close(extent(&xs, "timber", 0, Some((1, depth(long)))), (0.0, L + depth(short) / 2.0)), "{long}");
            assert!(close(extent(&ys, "timber", 1, Some((0, depth(short)))), (depth(long) / 2.0, 3.0)), "{long}");
            assert_eq!(overlap(&xs, &ys), None, "{long}");
            assert_eq!((overlap_within(&xs), overlap_within(&ys)), (None, None), "{long}");
            // the corner stud behind the edge of the first board inside
            let inner = L - depth(short) / 2.0;
            assert!(xs.iter().any(|p| p.member("timber", 1, depth(long)) && p.size(2) > 2.0 && close((p.lo.x, p.hi.x), (inner - STUD, inner))), "{long}");
        }
    }
}
