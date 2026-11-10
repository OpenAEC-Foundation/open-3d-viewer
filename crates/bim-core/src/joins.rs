//! Wall joins: where walls meet end to end (a corner) or end to side (a T), their ends are cut so
//! they meet without a gap or an overlap, whatever their thicknesses, as a wall's corner is drawn.
//!
//! - **A corner.** Two walls whose axis ends lie within [`JOIN_TOLERANCE`] of each other: both are
//!   cut along the vertical plane through their inner corner (where their inner faces cross) and
//!   their outer corner (where their outer faces cross). For walls of one thickness that is the
//!   mitre; for walls of two thicknesses it still meets face to face.
//! - **A T.** A wall whose axis end lies on another wall's axis (within that wall's half thickness),
//!   away from its ends: the end stops at the other wall's face; the other runs through.
//!
//! Three or more walls at one point, walls at heights that do not overlap, and corners so sharp
//! that the cut would reach far beyond the joint are left as they are.
//!
//! A joined wall is made with its ends reaching past its corners, then cut back to its footprint
//! in plan (a prism through its whole height) by a boolean intersection, layer by layer.

use crate::element::{Element, PlanarDef, PlanarForm, Shape};
use crate::model::{Model, JOIN_TOLERANCE};
use crate::profile::Profile;
use crate::solid::{Axes, Solid};
use crate::tile::TileKey;
use crate::{Refusal, LINEAR_TOLERANCE};
use glam::{DVec2, DVec3};

/// How far past its ends a joined wall's footprint and solid reach, so a cut never touches an end
/// face (metres).
const MARGIN: f64 = 0.01;
/// A corner whose cut would reach farther from the joint than this many wall thicknesses is left as
/// it is (a very sharp angle).
const REACH: f64 = 8.0;

/// A wall's axis, heights and half thickness, in one tile's frame.
#[derive(Debug, Clone, Copy)]
struct Wall {
    start: DVec2,
    end: DVec2,
    base: f64,
    top: f64,
    half: f64,
}

impl Wall {
    fn dir(&self) -> DVec2 {
        (self.end - self.start).normalize_or_zero()
    }

    fn left(&self) -> DVec2 {
        self.dir().perp()
    }

    fn length(&self) -> f64 {
        (self.end - self.start).length()
    }

    /// A face of the wall as a line (a point and a direction): its left face for `side` 1, its
    /// right for -1.
    fn face(&self, side: f64) -> (DVec2, DVec2) {
        (self.start + self.left() * (self.half * side), self.dir())
    }
}

/// The wall that element `e` is, in the frame of tile `tile`.
fn wall_of(model: &Model, e: &Element, tile: TileKey) -> Option<Wall> {
    let Shape::Planar(p @ PlanarDef { form: PlanarForm::Wall { start, end, base, height }, .. }) = &e.shape else { return None };
    let offset = model.grid.offset(e.tile, tile).truncate();
    let wall = Wall { start: *start + offset, end: *end + offset, base: *base, top: base + height, half: p.total_thickness() / 2.0 };
    (wall.length() > LINEAR_TOLERANCE && wall.half > 0.0).then_some(wall)
}

/// Where two lines (a point and a direction each) cross; `None` when they run parallel.
fn crossing(a: (DVec2, DVec2), b: (DVec2, DVec2)) -> Option<DVec2> {
    let across = a.1.perp_dot(b.1);
    if across.abs() < 1e-9 {
        return None;
    }
    Some(a.0 + a.1 * ((b.0 - a.0).perp_dot(b.1) / across))
}

/// What a wall's joints make of it: how far its solid reaches past its start and its end, and
/// its footprint in plan (start left, end left, end right, start right).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WallJoin {
    pub extend: [f64; 2],
    pub footprint: [DVec2; 4],
    pub base: f64,
    pub top: f64,
}

impl WallJoin {
    /// The footprint as a prism through the wall's whole height and a metre beyond.
    pub fn prism(&self) -> Result<Solid, Refusal> {
        let outline = Profile::new(self.footprint.to_vec(), Vec::new())?;
        let axes = Axes::new(DVec3::new(0.0, 0.0, self.base - 1.0), DVec3::X, DVec3::Z)?;
        Solid::extrude(&outline, &axes, self.top - self.base + 2.0)
    }
}

/// The joints of wall `element` with the other walls of `model`, in its tile's frame; `None` when
/// it meets none (it is made as it is).
pub(crate) fn wall_join(model: &Model, element: &Element) -> Option<WallJoin> {
    let me = wall_of(model, element, element.tile)?;
    let others: Vec<Wall> = model
        .iter()
        .filter(|e| e.id != element.id)
        .filter_map(|e| wall_of(model, e, element.tile))
        .filter(|o| o.top.min(me.top) - o.base.max(me.base) > LINEAR_TOLERANCE)
        .collect();
    if others.is_empty() {
        return None;
    }
    let (d, l) = (me.dir(), me.left() * me.half);
    let mut footprint = [me.start + l, me.end + l, me.end - l, me.start - l];
    let mut joined = [false; 2];
    for (k, at_end) in [(0, false), (1, true)] {
        let j = if at_end { me.end } else { me.start };
        if let Some((left, right)) = end_corners(&me, at_end, j, &others) {
            if at_end {
                footprint[1] = left;
                footprint[2] = right;
            } else {
                footprint[0] = left;
                footprint[3] = right;
            }
            joined[k] = true;
        }
    }
    if !joined.contains(&true) {
        return None;
    }
    // an end without a joint: the footprint reaches a little past it, so the cut leaves it alone
    if !joined[0] {
        footprint[0] -= d * MARGIN;
        footprint[3] -= d * MARGIN;
    }
    if !joined[1] {
        footprint[1] += d * MARGIN;
        footprint[2] += d * MARGIN;
    }
    // the solid reaches past the farthest corner at a joined end
    let along = |p: DVec2| (p - me.start).dot(d);
    let length = me.length();
    let extend = [
        if joined[0] { (-along(footprint[0]).min(along(footprint[3]))).max(0.0) + MARGIN } else { 0.0 },
        if joined[1] { (along(footprint[1]).max(along(footprint[2])) - length).max(0.0) + MARGIN } else { 0.0 },
    ];
    Some(WallJoin { extend, footprint, base: me.base, top: me.top })
}

/// The corners of `me`'s end at `j` (its end when `at_end`, else its start) where another wall
/// meets it there: its left and right corner.
fn end_corners(me: &Wall, at_end: bool, j: DVec2, others: &[Wall]) -> Option<(DVec2, DVec2)> {
    let (my_left, my_right) = (me.face(1.0), me.face(-1.0));
    let ends: Vec<(&Wall, bool)> = others
        .iter()
        .filter_map(|o| {
            if o.start.distance(j) <= JOIN_TOLERANCE {
                Some((o, false))
            } else if o.end.distance(j) <= JOIN_TOLERANCE {
                Some((o, true))
            } else {
                None
            }
        })
        .collect();
    let (left, right, other) = match ends[..] {
        // a corner: one end meets another's start (the faces on the same side meet), or two ends
        // or two starts meet (opposite sides meet)
        [(o, o_at_end)] => {
            let same = at_end != o_at_end;
            let (ol, or) = (o.face(1.0), o.face(-1.0));
            let (to_left, to_right) = if same { (ol, or) } else { (or, ol) };
            (crossing(my_left, to_left)?, crossing(my_right, to_right)?, o)
        }
        // a T: the end on another wall's side, away from its ends; it stops at the face on its side
        [] => {
            let o = others
                .iter()
                .filter(|o| {
                    let t = (j - o.start).dot(o.dir());
                    t > JOIN_TOLERANCE && t < o.length() - JOIN_TOLERANCE && (j - o.start).dot(o.left()).abs() <= o.half + JOIN_TOLERANCE
                })
                .min_by(|a, b| (j - a.start).dot(a.left()).abs().total_cmp(&(j - b.start).dot(b.left()).abs()))?;
            let body = if at_end { me.start } else { me.end };
            let side = if (body - o.start).dot(o.left()) >= 0.0 { 1.0 } else { -1.0 };
            let face = o.face(side);
            (crossing(my_left, face)?, crossing(my_right, face)?, o)
        }
        // three or more walls at one point
        _ => return None,
    };
    let reach = REACH * 2.0 * (me.half + other.half);
    (left.distance(j) <= reach && right.distance(j) <= reach).then_some((left, right))
}

#[cfg(test)]
mod tests {
    use crate::element::{BuildUpDef, LayerDef, Opening, PlanarDef, PlanarForm, Shape};
    use crate::model::Model;
    use crate::tile::TileGrid;
    use crate::{Element, ElementId};
    use glam::DVec2;

    fn wall(a: (f64, f64), b: (f64, f64), thickness: f64) -> Shape {
        Shape::Planar(PlanarDef {
            form: PlanarForm::Wall { start: DVec2::new(a.0, a.1), end: DVec2::new(b.0, b.1), base: 0.0, height: 3.0 },
            thickness,
            openings: Vec::new(),
            build_up: None,
        })
    }

    fn add(m: &mut Model, shape: Shape) -> ElementId {
        let grid = m.grid;
        m.add(Element::in_project(&grid, "w", None, shape)).unwrap()
    }

    fn volume(m: &Model, id: ElementId) -> f64 {
        m.evaluate(id).unwrap().iter().map(|p| p.solid.volume()).sum()
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn two_walls_meet_in_a_mitre_without_overlap_or_gap() {
        let mut m = Model::new(TileGrid::default());
        let a = add(&mut m, wall((0.0, 0.0), (5.0, 0.0), 0.2));
        let b = add(&mut m, wall((5.0, 0.0), (5.0, 4.0), 0.2));
        // footprints: trapezia 4.9 | 5.1 and 3.9 | 4.1 long, 0.2 wide
        assert!(close(volume(&m, a), 1.0 * 3.0), "{}", volume(&m, a));
        assert!(close(volume(&m, b), 0.8 * 3.0), "{}", volume(&m, b));
        // together they cover the L exactly: 5.1 × 0.2 plus 0.2 × 3.9
        assert!(close(volume(&m, a) + volume(&m, b), (5.1 * 0.2 + 0.2 * 3.9) * 3.0));
    }

    #[test]
    fn a_wall_drawn_the_other_way_meets_the_same() {
        let mut m = Model::new(TileGrid::default());
        let a = add(&mut m, wall((0.0, 0.0), (5.0, 0.0), 0.2));
        let b = add(&mut m, wall((5.0, 4.0), (5.0, 0.0), 0.2));
        assert!(close(volume(&m, a), 3.0) && close(volume(&m, b), 2.4), "{} {}", volume(&m, a), volume(&m, b));
        // and two starts at one point
        let mut m = Model::new(TileGrid::default());
        let a = add(&mut m, wall((5.0, 0.0), (0.0, 0.0), 0.2));
        let b = add(&mut m, wall((5.0, 0.0), (5.0, 4.0), 0.2));
        assert!(close(volume(&m, a), 3.0) && close(volume(&m, b), 2.4), "{} {}", volume(&m, a), volume(&m, b));
    }

    #[test]
    fn a_door_at_a_corner_goes_through_the_walls_reach_into_it() {
        let mut m = Model::new(TileGrid::default());
        let mut shape = wall((0.0, 0.0), (5.0, 0.0), 0.2);
        let Shape::Planar(p) = &mut shape else { unreachable!() };
        p.openings = vec![Opening { along: 4.5, sill: 0.0, width: 0.5, height: 2.1 }];
        let a = add(&mut m, shape);
        add(&mut m, wall((5.0, 0.0), (5.0, 4.0), 0.2));
        // the mitred footprint from 4.5 on is 0.4 | 0.6 long, 0.2 wide: 0.1 m², 2.1 high
        assert!(close(volume(&m, a), 3.0 - 0.1 * 2.1), "{}", volume(&m, a));
    }

    #[test]
    fn walls_of_two_thicknesses_meet_face_to_face() {
        let mut m = Model::new(TileGrid::default());
        let a = add(&mut m, wall((0.0, 0.0), (5.0, 0.0), 0.3));
        let b = add(&mut m, wall((5.0, 0.0), (5.0, 4.0), 0.1));
        // the L's footprint: 5.05 × 0.3 plus 0.1 × 3.85, shared without overlap
        let total = volume(&m, a) + volume(&m, b);
        assert!(close(total, (5.05 * 0.3 + 0.1 * 3.85) * 3.0), "{total}");
        assert!(close(volume(&m, a), 1.5 * 3.0), "{}", volume(&m, a));
    }

    #[test]
    fn a_wall_ending_on_another_stops_at_its_face() {
        let mut m = Model::new(TileGrid::default());
        let a = add(&mut m, wall((0.0, 0.0), (10.0, 0.0), 0.2));
        let b = add(&mut m, wall((5.0, 0.0), (5.0, 4.0), 0.2));
        assert!(close(volume(&m, a), 10.0 * 0.2 * 3.0), "the wall it ends on runs through");
        assert!(close(volume(&m, b), 3.9 * 0.2 * 3.0), "{}", volume(&m, b));
        // from the other side
        let c = add(&mut m, wall((6.0, -3.0), (6.0, 0.0), 0.2));
        assert!(close(volume(&m, c), 2.9 * 0.2 * 3.0), "{}", volume(&m, c));
    }

    #[test]
    fn a_joined_wall_keeps_its_openings() {
        let mut m = Model::new(TileGrid::default());
        let window = Opening { along: 1.0, sill: 0.9, width: 1.2, height: 1.2 };
        let shape = Shape::Planar(PlanarDef {
            form: PlanarForm::Wall { start: DVec2::ZERO, end: DVec2::new(5.0, 0.0), base: 0.0, height: 3.0 },
            thickness: 0.2,
            openings: vec![window],
            build_up: None,
        });
        let a = add(&mut m, shape);
        add(&mut m, wall((5.0, 0.0), (5.0, 4.0), 0.2));
        assert!(close(volume(&m, a), 3.0 - 1.2 * 1.2 * 0.2), "{}", volume(&m, a));
    }

    #[test]
    fn layered_walls_join_layer_by_layer() {
        let layered = |a: (f64, f64), b: (f64, f64)| {
            let layers = vec![
                LayerDef { material: "brick".into(), thickness: 0.1, cavity: false },
                LayerDef { material: "air".into(), thickness: 0.05, cavity: true },
                LayerDef { material: "block".into(), thickness: 0.1, cavity: false },
            ];
            let Shape::Planar(p) = wall(a, b, 0.25) else { unreachable!() };
            Shape::Planar(p.with_build_up(Some(BuildUpDef { id: "cavity wall".into(), layers })))
        };
        let mut m = Model::new(TileGrid::default());
        let a = add(&mut m, layered((0.0, 0.0), (5.0, 0.0)));
        add(&mut m, layered((5.0, 0.0), (5.0, 4.0)));
        let parts = m.evaluate(a).unwrap();
        assert_eq!(parts.len(), 2, "two layers, the cavity is empty");
        assert!(parts.iter().all(|p| p.material.is_some()));
        // the whole wall's footprint is the trapezium 4.875 | 5.125 long, 0.25 wide; the layers fill
        // it but for the cavity, which is 0.05 wide along the same mitre
        let whole = (4.875 + 5.125) / 2.0 * 0.25 * 3.0;
        let cavity = 5.0 * 0.05 * 3.0;
        assert!(close(volume(&m, a), whole - cavity), "{} vs {}", volume(&m, a), whole - cavity);
    }

    #[test]
    fn walls_that_do_not_share_a_height_or_meet_too_sharply_stay_as_they_are() {
        let mut m = Model::new(TileGrid::default());
        let a = add(&mut m, wall((0.0, 0.0), (5.0, 0.0), 0.2));
        let Shape::Planar(mut p) = wall((5.0, 0.0), (5.0, 4.0), 0.2) else { unreachable!() };
        if let PlanarForm::Wall { base, .. } = &mut p.form {
            *base = 3.0;
        }
        add(&mut m, Shape::Planar(p));
        assert!(close(volume(&m, a), 5.0 * 0.2 * 3.0), "the other stands on top of it");
        // 2° apart: the mitre would reach metres out
        let mut m = Model::new(TileGrid::default());
        let a = add(&mut m, wall((0.0, 0.0), (5.0, 0.0), 0.2));
        let back = 2f64.to_radians();
        add(&mut m, wall((5.0, 0.0), (5.0 - 5.0 * back.cos(), 5.0 * back.sin()), 0.2));
        assert!(close(volume(&m, a), 5.0 * 0.2 * 3.0));
    }

    #[test]
    fn three_walls_at_one_point_stay_as_they_are() {
        let mut m = Model::new(TileGrid::default());
        let a = add(&mut m, wall((0.0, 0.0), (5.0, 0.0), 0.2));
        add(&mut m, wall((5.0, 0.0), (5.0, 4.0), 0.2));
        add(&mut m, wall((5.0, 0.0), (9.0, 0.0), 0.2));
        assert!(close(volume(&m, a), 5.0 * 0.2 * 3.0));
    }
}
