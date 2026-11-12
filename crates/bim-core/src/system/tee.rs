//! T junctions of framed systems: where a timber frame ends against the side of another, it stops
//! at the other's frame and the other's layers on that side make room for it, so the two meet
//! without a gap or an overlap.
//!
//! - **Which meet.** A framed system with an end on the line of another, between that one's ends
//!   (within [`JOIN_TOLERANCE`] of the line, farther than that from either end), no third framed
//!   system at that point, on one base, as high as each other and square to each other. Anything
//!   else is left as it is: two that cross, two that end at one point of a third's side.
//! - **The one that stands against the side.** Its frame stops at the face of the other's frame,
//!   and each of its layers at the far face of the other's layer it would cross, as at the inside
//!   of a corner ([`super::corner`]).
//! - **The one that runs through.** Its frame is whole. Each of its layers on that side is left
//!   out across the other: across its frame, and across what it has on either side between its
//!   frame and that layer's depth. A stud stands either side of the stretch its first board leaves
//!   open, behind the board's edges, and a framed layer ends there as it does at a corner.
//!
//! A T junction is made only when both systems can be made with it and without it, as a corner is.

use super::corner::{after, applied, before, short, Framed, Section, SQUARE};
use super::{FaceBreak, FaceEnd};
use crate::element::{Element, ElementId, Shape};
use crate::model::{Model, JOIN_TOLERANCE};
use crate::tile::TileKey;
use crate::LINEAR_TOLERANCE;
use glam::DVec2;

/// The T junctions a framed system makes with others: the element against whose side it stands at
/// its start and at its end, and those that stand against its own side, in order along it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Tees {
    pub ends: [Option<ElementId>; 2],
    pub sides: Vec<Abutment>,
}

/// A system that stands against the side of another: which element, how far along the other's
/// line from its start, and on which side of it (its left is its interior).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Abutment {
    pub with: ElementId,
    pub along: f64,
    pub left: bool,
}

/// A T junction: the system that runs through and the one that stands against its side, which end
/// of that one, how far along the first one's line from its start, on which side of it (0 its
/// right, 1 its left), which side of the one that stands there looks to the other's start, and how
/// far its end lies from the other's line.
#[derive(Clone, Copy)]
pub(super) struct Tee<'a> {
    pub(super) runs: Framed<'a>,
    pub(super) stands: Framed<'a>,
    pub(super) end: usize,
    at: f64,
    side: usize,
    low: usize,
    shift: f64,
}

/// How far along the line of `a` from its start point `p` lies, when it lies on that line between
/// its ends.
fn along(a: &Framed, p: DVec2) -> Option<f64> {
    let d = p - a.ends[0];
    let (t, off) = (d.dot(a.inward(0)), d.dot(a.right()).abs());
    (off <= JOIN_TOLERANCE && t > JOIN_TOLERANCE && t < a.length() - JOIN_TOLERANCE).then_some(t)
}

/// The T junction of end `end` of `stands` against the side of `runs`; `None` when it does not
/// lie on that one's line, or the two are not on one base, not as high as each other or not square
/// to each other.
fn between<'a>(runs: Framed<'a>, stands: Framed<'a>, end: usize) -> Option<Tee<'a>> {
    let tol = LINEAR_TOLERANCE;
    if (runs.def.base - stands.def.base).abs() > tol || (runs.def.height - stands.def.height).abs() > tol {
        return None;
    }
    let (dir, away) = (runs.inward(0), stands.inward(end));
    if dir.dot(away).abs() > SQUARE {
        return None;
    }
    let at = along(&runs, stands.ends[end])?;
    let foot = runs.ends[0] + dir * at;
    let side = usize::from(runs.right().dot(away) < 0.0);
    let low = usize::from(stands.right().dot(dir) > 0.0);
    Some(Tee { runs, stands, end, at, side, low, shift: (stands.ends[end] - foot).dot(away) })
}

/// The T junction at end `end` of `stands`, as the systems lie: it lies on the line of one other
/// framed system, and no other framed system ends there.
pub(super) fn geometric<'a>(model: &'a Model, stands: Framed<'a>, end: usize, tile: TileKey) -> Option<Tee<'a>> {
    let p = stands.ends[end];
    let mut runs = None;
    for e in model.iter().filter(|e| e.id != stands.id) {
        let Some(other) = Framed::of(model, e, tile) else { continue };
        if other.ends.iter().any(|q| (*q - p).length() <= JOIN_TOLERANCE) {
            return None;
        }
        if along(&other, p).is_some() {
            if runs.is_some() {
                return None;
            }
            runs = Some(other);
        }
    }
    between(runs?, stands, end)
}

/// The T junctions along `runs`, as the systems lie: every other framed system that stands against
/// its side, in order along it.
pub(super) fn standing<'a>(model: &'a Model, runs: Framed<'a>, tile: TileKey) -> Vec<Tee<'a>> {
    let mut tees = Vec::new();
    for e in model.iter().filter(|e| e.id != runs.id) {
        let Some(other) = Framed::of(model, e, tile) else { continue };
        for end in 0..2 {
            if along(&runs, other.ends[end]).is_some() && geometric(model, other, end, tile).is_some_and(|t| t.runs.id == runs.id) {
                tees.extend(between(runs, other, end));
            }
        }
    }
    tees.sort_by(|a, b| a.at.total_cmp(&b.at));
    tees
}

/// How the faces of a system with section `mine` end where it stands against the side of a system
/// with section `theirs`, at T junction `t`: one for every face. `None` when the boards on its two
/// sides would each have the frame's backing stud somewhere else.
pub(super) fn butt_ends(mine: &Section, theirs: &Section, t: &Tee) -> Option<Vec<FaceEnd>> {
    let h = theirs.half;
    let against = theirs.bounds(t.side);
    // the frame, and what lies within its depth: up to the face of the other's frame; a layer on
    // either side: up to the far face of the other's layer it would cross
    let mut ends = vec![FaceEnd { reach: -h, backing: None }; mine.faces];
    for l in mine.sides.iter().flatten() {
        ends[l.face].reach = -(h + after(&against, l.x2));
    }
    // a board that ends a member's width or more short of the members it is fixed to gets a member
    // more behind its edge: a batten more in a service zone, a stud more in the frame
    for layers in &mine.sides {
        for l in layers {
            if let Some(w) = l.framed {
                ends[l.face].backing = short(layers, &ends, l.face, w, l.x2);
            }
        }
    }
    let backs: Vec<f64> = mine.sides.iter().filter_map(|layers| short(layers, &ends, 0, mine.width, 0.0)).collect();
    if backs.len() == 2 && (backs[0] - backs[1]).abs() > LINEAR_TOLERANCE {
        return None;
    }
    ends[0].backing = backs.first().copied();
    for e in &mut ends {
        e.reach += t.shift;
    }
    Some(ends)
}

/// How the faces of a system with section `mine` are where a system with section `theirs` stands
/// against its side, at T junction `t`: one for every face.
pub(super) fn breaks(mine: &Section, theirs: &Section, t: &Tee) -> Vec<FaceBreak> {
    let tol = LINEAR_TOLERANCE;
    let h = theirs.half;
    // what the other has on the side that looks to this one's start, and on its other side
    let sides = [theirs.bounds(t.low), theirs.bounds(1 - t.low)];
    // the stretch a layer `x` out from this one's frame leaves open: the other's frame, and what
    // it has on either side between its frame and that depth
    let open = |x: f64| (t.at - h - before(&sides[0], x), t.at + h + before(&sides[1], x));
    let layers = &mine.sides[t.side];
    let board = |from: f64| layers.iter().filter(|l| l.boards && l.x1 >= from - tol).min_by(|a, b| a.x1.total_cmp(&b.x1));
    let mut breaks = vec![FaceBreak::default(); mine.faces];
    // the frame is whole; a stud either side of the stretch its first board leaves open
    breaks[0].posts = Some(open(board(0.0).map_or(0.0, |b| b.x1)));
    for l in layers {
        let own = open(l.x1);
        breaks[l.face].gap = Some(own);
        // a framed layer ends either side as at a corner: a member more behind the edge of a board
        // that ends a member's width or more short of it
        if let (Some(w), Some(b)) = (l.framed, board(l.x2)) {
            let theirs = open(b.x1);
            breaks[l.face].back = [own.0 - theirs.0, theirs.1 - own.1].map(|d| Some(d).filter(|d| *d >= w - tol));
        }
    }
    breaks
}

/// The T junctions `element` makes with other framed systems of `model`.
pub fn tees(model: &Model, element: &Element) -> Tees {
    let Shape::System(s) = &element.shape else { return Tees::default() };
    let Some(me) = Framed::new(model, element.id, element.tile, s, element.tile) else { return Tees::default() };
    let joins = applied(model, &me, element.tile);
    Tees {
        ends: joins.butts.map(|t| t.map(|t| t.runs.id)),
        sides: joins.sides.iter().map(|t| Abutment { with: t.stands.id, along: t.at, left: t.side == 1 }).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::corner::tests::{add, close, def, extent, frame, overlap, overlap_within, pieces, plain, system, Piece, BATTEN, EXTERIOR, INTERIOR, L, STUD};
    use super::super::{corners, joined_units, units, Meeting};
    use super::{tees, Abutment, Tees};
    use crate::element::{Opening, Shape, SystemDef};
    use crate::model::Model;
    use crate::tile::TileGrid;
    use crate::ElementId;

    /// An exterior frame along +x to (4.8, 0), and a frame of type `id` that stands against the
    /// middle of its inside, 3 m long along +y.
    fn tee(id: &str) -> (Model, ElementId, ElementId) {
        let mut m = Model::new(TileGrid::default());
        let x = add(&mut m, frame((0.0, 0.0), (L, 0.0)));
        let y = add(&mut m, Shape::System(def((2.4, 0.0), (2.4, 3.0), 2.7, id)));
        (m, x, y)
    }

    fn of(m: &Model, id: ElementId) -> Tees {
        tees(m, m.get(id).unwrap())
    }

    /// Whether `t` is one system against the side, `with`, `along` the line, on its left or not.
    fn one(t: &Tees, with: ElementId, along: f64, left: bool) -> bool {
        t.ends == [None, None] && matches!(t.sides[..], [Abutment { with: w, along: a, left: l }] if w == with && (a - along).abs() < 1e-9 && l == left)
    }

    /// The stretch around `at` along x that the pieces of `material` leave open, of those `deep`
    /// across the face only when given; from `at` to `at` where one runs on across it.
    fn open(pieces: &[Piece], material: &str, deep: Option<f64>, at: f64) -> (f64, f64) {
        let picked = pieces.iter().filter(|p| deep.map_or(p.is(material), |d| p.member(material, 1, d)));
        picked.fold((f64::MIN, f64::MAX), |(lo, hi), p| if p.hi.x <= at { (lo.max(p.hi.x), hi) } else if p.lo.x >= at { (lo, hi.min(p.lo.x)) } else { (at, at) })
    }

    /// Whether an upright of `pieces`, `deep` across the face, stands from `from` to `to` along x.
    fn upright(pieces: &[Piece], deep: f64, from: f64, to: f64) -> bool {
        pieces.iter().any(|p| p.member("timber", 1, deep) && p.size(2) > 2.0 && close((p.lo.x, p.hi.x), (from, to)))
    }

    #[test]
    fn a_frame_that_ends_on_the_line_of_another_stands_against_its_side() {
        let (m, x, y) = tee(INTERIOR);
        assert!(one(&of(&m, x), y, 2.4, true), "{:?}", of(&m, x));
        assert_eq!(of(&m, y), Tees { ends: [Some(x), None], sides: Vec::new() });
        assert_eq!((corners(&m, m.get(x).unwrap()), corners(&m, m.get(y).unwrap())), ([None, None], [None, None]));
        let (xs, ys) = (pieces(&m, x), pieces(&m, y));
        // the frame that runs through is whole, and the other stops at its face
        assert!(close(extent(&xs, "timber", 0, Some((1, 0.14))), (0.0, L)));
        assert!(close(open(&xs, "timber", Some(0.14), 2.4), (2.4, 2.4)));
        assert!(close(extent(&ys, "timber", 1, Some((0, 0.089))), (0.07, 3.0)));
        assert_eq!(overlap(&xs, &ys), None);
        assert_eq!((overlap_within(&xs), overlap_within(&ys)), (None, None));
    }

    #[test]
    fn the_layers_of_the_frame_that_runs_through_make_room_for_the_other() {
        // an inner wall, 89 mm deep with a gypsum board of 12.5 mm either side
        let (m, x, y) = tee(INTERIOR);
        let (xs, ys) = (pieces(&m, x), pieces(&m, y));
        // the plywood on the frame is left out across the other's frame, and what lies on it
        // across its boards too
        assert!(close(open(&xs, "plywood", None, 2.4), (2.3555, 2.4445)));
        assert!(close(open(&xs, "vapour-control-membrane", None, 2.4), (2.343, 2.457)));
        assert!(close(open(&xs, "timber", Some(BATTEN), 2.4), (2.343, 2.457)));
        assert!(close(open(&xs, "gypsum-board", None, 2.4), (2.343, 2.457)));
        // the other side is as it was
        assert!(close(open(&xs, "breather-membrane", None, 2.4), (2.4, 2.4)));
        // the other's boards start on the plywood
        assert!(close(extent(&ys, "gypsum-board", 1, None), (0.088, 3.0)));

        // an exterior frame against it: each layer stops at the layer of the other it would cross
        let (m, x, y) = tee(EXTERIOR);
        let (xs, ys) = (pieces(&m, x), pieces(&m, y));
        assert!(close(open(&xs, "plywood", None, 2.4), (2.33, 2.47)));
        assert!(close(open(&xs, "vapour-control-membrane", None, 2.4), (2.312, 2.4705)));
        assert!(close(open(&xs, "timber", Some(BATTEN), 2.4), (2.3118, 2.4705)));
        assert!(close(open(&xs, "gypsum-board", None, 2.4), (2.2668, 2.4705)));
        let from = |material: &str, deep: Option<(usize, f64)>| extent(&ys, material, 1, deep).0;
        let starts = [from("timber", Some((0, 0.14))), from("breather-membrane", None), from("plywood", None), from("vapour-control-membrane", None), from("timber", Some((0, BATTEN))), from("gypsum-board", None)];
        for (start, at) in starts.into_iter().zip([0.07, 0.088, 0.088, 0.0882, 0.1332, 0.1457]) {
            assert!((start - at).abs() < 1e-9, "{start} {at}");
        }
        assert_eq!(overlap(&xs, &ys), None);
        assert_eq!((overlap_within(&xs), overlap_within(&ys)), (None, None));
    }

    #[test]
    fn a_stud_stands_either_side_of_the_stretch_the_boards_leave_open() {
        let (m, x, _) = tee(INTERIOR);
        let xs = pieces(&m, x);
        // behind the edges of the plywood, and a batten at each edge of the service zone
        assert!(upright(&xs, 0.14, 2.3555 - STUD, 2.3555) && upright(&xs, 0.14, 2.4445, 2.4445 + STUD));
        assert!(upright(&xs, BATTEN, 2.343 - BATTEN, 2.343) && upright(&xs, BATTEN, 2.457, 2.457 + BATTEN));
        // the stud on the grid stays where it is, between them
        assert!(upright(&xs, 0.14, 2.4 - STUD / 2.0, 2.4 + STUD / 2.0));

        // where the gypsum board ends a batten's width short of the service zone, a batten more
        // stands behind its edge
        let (m, x, _) = tee(EXTERIOR);
        let xs = pieces(&m, x);
        assert!(upright(&xs, 0.14, 2.33 - STUD, 2.33) && upright(&xs, 0.14, 2.47, 2.47 + STUD));
        assert!(upright(&xs, BATTEN, 2.3118 - BATTEN, 2.3118) && upright(&xs, BATTEN, 2.3118 - 2.0 * BATTEN, 2.3118 - BATTEN));
        assert!(upright(&xs, BATTEN, 2.4705, 2.4705 + BATTEN) && !upright(&xs, BATTEN, 2.4705 + BATTEN, 2.4705 + 2.0 * BATTEN));
    }

    #[test]
    fn the_prefab_units_of_a_frame_keep_clear_of_what_stands_against_it() {
        let near = |a: &[f64], b: &[f64]| a.len() == b.len() && a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-9);
        let (m, x, y) = tee(EXTERIOR);
        let lengths = |id: ElementId| joined_units(&m, m.get(id).unwrap()).unwrap();
        // alone its two units meet in the middle, where the other stands now
        assert!(near(&units(system(&m, x)).unwrap(), &[2.4, 2.4]));
        assert!(near(&lengths(x), &[1.8, 3.0]), "{:?}", lengths(x));
        // the one that stands against it is 70 mm shorter
        assert!(near(&lengths(y), &[2.93]), "{:?}", lengths(y));
    }

    #[test]
    fn a_t_junction_is_made_whichever_way_its_frames_are_drawn() {
        let runs = [((0.0, 0.0), (L, 0.0)), ((L, 0.0), (0.0, 0.0))];
        let stands = [((2.4, 0.0), (2.4, 3.0)), ((2.4, 3.0), (2.4, 0.0)), ((2.4, 0.0), (2.4, -3.0)), ((2.4, -3.0), (2.4, 0.0))];
        for (i, (a, b)) in runs.into_iter().enumerate() {
            for (j, (c, d)) in stands.into_iter().enumerate() {
                for first in [true, false] {
                    let case = format!("case {i} {j} {first}");
                    let mut m = Model::new(TileGrid::default());
                    let (x, y) = if first {
                        let x = add(&mut m, frame(a, b));
                        (x, add(&mut m, frame(c, d)))
                    } else {
                        let y = add(&mut m, frame(c, d));
                        (add(&mut m, frame(a, b)), y)
                    };
                    // which end stands there, on which side of the other, and where along it
                    let (end, up) = (usize::from(c.1 != 0.0), c.1 + d.1 > 0.0);
                    assert!(one(&of(&m, x), y, 2.4, up == (b.0 > a.0)), "{case}: {:?}", of(&m, x));
                    let mut ends = [None, None];
                    ends[end] = Some(x);
                    assert_eq!(of(&m, y), Tees { ends, sides: Vec::new() }, "{case}");
                    let (xs, ys) = (pieces(&m, x), pieces(&m, y));
                    assert!(close(extent(&xs, "timber", 0, Some((1, 0.14))), (0.0, L)), "{case}");
                    assert!(close(extent(&ys, "timber", 1, Some((0, 0.14))), if up { (0.07, 3.0) } else { (-3.0, -0.07) }), "{case}");
                    assert_eq!(overlap(&xs, &ys), None, "{case}");
                    assert_eq!((overlap_within(&xs), overlap_within(&ys)), (None, None), "{case}");
                    // a stud either side of the other's frame, whichever side it stands on
                    assert!(upright(&xs, 0.14, 2.33 - STUD, 2.33) && upright(&xs, 0.14, 2.47, 2.47 + STUD), "{case}");
                }
            }
        }
    }

    #[test]
    fn a_frame_against_the_outside_of_another_parts_its_membrane() {
        let mut m = Model::new(TileGrid::default());
        let x = add(&mut m, frame((0.0, 0.0), (L, 0.0)));
        let y = add(&mut m, frame((2.4, 0.0), (2.4, -3.0)));
        assert!(one(&of(&m, x), y, 2.4, false), "{:?}", of(&m, x));
        let (xs, ys) = (pieces(&m, x), pieces(&m, y));
        assert!(close(open(&xs, "breather-membrane", None, 2.4), (2.33, 2.47)));
        // what it has inside is as it was
        for material in ["plywood", "vapour-control-membrane", "gypsum-board"] {
            assert!(close(open(&xs, material, None, 2.4), (2.4, 2.4)), "{material}");
        }
        // the other's frame up to the frame, its layers up to the membrane
        assert!(close(extent(&ys, "timber", 1, Some((0, 0.14))), (-3.0, -0.07)));
        for material in ["breather-membrane", "plywood", "vapour-control-membrane", "gypsum-board"] {
            assert!((extent(&ys, material, 1, None).1 + 0.0705).abs() < 1e-9, "{material}");
        }
    }

    #[test]
    fn a_t_junction_is_made_across_tiles_and_within_the_join_tolerance() {
        let mut m = Model::new(TileGrid::default());
        let x = add(&mut m, frame((1000.0 - 2.4, 0.0), (1000.0 + 2.4, 0.0)));
        let y = add(&mut m, frame((1000.0, 0.0), (1000.0, 3.0)));
        assert_ne!(m.get(x).unwrap().tile, m.get(y).unwrap().tile);
        assert!(one(&of(&m, x), y, 2.4, true), "{:?}", of(&m, x));
        assert_eq!(of(&m, y).ends, [Some(x), None]);
        let (xs, ys) = (pieces(&m, x), pieces(&m, y));
        assert!(close(open(&xs, "plywood", None, 1000.0), (999.93, 1000.07)));
        assert!(close(extent(&ys, "timber", 1, Some((0, 0.14))), (0.07, 3.0)));
        assert_eq!(overlap(&xs, &ys), None);

        // an end a little off the line: the frame still ends at the other's face, where that is
        let mut m = Model::new(TileGrid::default());
        let x = add(&mut m, frame((0.0, 0.0), (L, 0.0)));
        let y = add(&mut m, frame((2.4, 0.0005), (2.4, 3.0)));
        assert!(one(&of(&m, x), y, 2.4, true));
        assert!(close(extent(&pieces(&m, y), "timber", 1, Some((0, 0.14))), (0.07, 3.0)));
        assert_eq!(overlap(&pieces(&m, x), &pieces(&m, y)), None);
        // farther off they do not meet
        let mut m = Model::new(TileGrid::default());
        let x = add(&mut m, frame((0.0, 0.0), (L, 0.0)));
        let y = add(&mut m, frame((2.4, 0.002), (2.4, 3.0)));
        assert!(plain(&m, x) && plain(&m, y));
    }

    #[test]
    fn a_frame_is_made_with_all_it_meets_together() {
        // a corner at its end, an inner wall against its inside and a frame against its outside
        let mut m = Model::new(TileGrid::default());
        let x = add(&mut m, frame((0.0, 0.0), (L, 0.0)));
        let corner = add(&mut m, frame((L, 0.0), (L, 3.0)));
        let inner = add(&mut m, Shape::System(def((1.2, 3.0), (1.2, 0.0), 2.7, INTERIOR)));
        let outer = add(&mut m, frame((3.0, 0.0), (3.0, -2.4)));
        assert_eq!(corners(&m, m.get(x).unwrap()), [None, Some(Meeting { with: corner, through: true })]);
        let t = of(&m, x);
        assert_eq!(t.ends, [None, None]);
        assert!(matches!(t.sides[..], [a, b] if a.with == inner && a.left && (a.along - 1.2).abs() < 1e-9 && b.with == outer && !b.left && (b.along - 3.0).abs() < 1e-9), "{t:?}");
        assert_eq!((of(&m, inner).ends, of(&m, outer).ends), ([None, Some(x)], [Some(x), None]));
        let all: Vec<_> = [x, corner, inner, outer].map(|id| pieces(&m, id)).into_iter().collect();
        for (i, a) in all.iter().enumerate() {
            assert_eq!(overlap_within(a), None, "{i}");
            for b in &all[i + 1..] {
                assert_eq!(overlap(a, b), None, "{i}");
            }
        }
        // its prefab units meet clear of both
        let lengths = joined_units(&m, m.get(x).unwrap()).unwrap();
        assert!(matches!(lengths[..], [a, b] if (a - 1.8).abs() < 1e-9 || (a - 2.4).abs() < 1e-9 && (a + b - L - 0.07).abs() < 1e-9), "{lengths:?}");

        // a frame between two others stands against a side at both of its ends
        let mut m = Model::new(TileGrid::default());
        let near = add(&mut m, frame((0.0, 0.0), (L, 0.0)));
        let far = add(&mut m, frame((0.0, 3.0), (L, 3.0)));
        let y = add(&mut m, Shape::System(def((2.4, 0.0), (2.4, 3.0), 2.7, INTERIOR)));
        assert_eq!(of(&m, y), Tees { ends: [Some(near), Some(far)], sides: Vec::new() });
        assert!(one(&of(&m, near), y, 2.4, true) && one(&of(&m, far), y, 2.4, false));
        let ys = pieces(&m, y);
        assert!(close(extent(&ys, "timber", 1, Some((0, 0.089))), (0.07, 2.93)));
        // its boards from the plywood of the one to the membrane of the other
        assert!(close(extent(&ys, "gypsum-board", 1, None), (0.088, 2.9295)));
        assert_eq!((overlap(&pieces(&m, near), &ys), overlap(&pieces(&m, far), &ys)), (None, None));
    }

    #[test]
    fn frames_that_do_not_meet_in_a_simple_t_junction_are_left_as_they_are() {
        let others: [Vec<Shape>; 7] = [
            // two that end at one point of its side
            vec![frame((2.4, 0.0), (2.4, 3.0)), frame((2.4, 0.0), (2.4, -3.0))],
            // not square to it
            vec![frame((2.4, 0.0), (3.4, 3.0))],
            // higher than it, and on another base
            vec![Shape::System(def((2.4, 0.0), (2.4, 3.0), 3.0, EXTERIOR))],
            vec![Shape::System(SystemDef { base: 0.5, ..def((2.4, 0.0), (2.4, 3.0), 2.7, EXTERIOR) })],
            // an end short of its line, and one beyond its end
            vec![frame((2.4, 0.5), (2.4, 3.0))],
            vec![frame((L + 0.5, 0.0), (L + 0.5, 3.0))],
            // a system that is not framed
            vec![Shape::System(def((2.4, 0.0), (2.4, 3.6), 2.7, "system.curtain-wall-aluminium-1500"))],
        ];
        for (i, shapes) in others.into_iter().enumerate() {
            let mut m = Model::new(TileGrid::default());
            let x = add(&mut m, frame((0.0, 0.0), (L, 0.0)));
            let ids: Vec<_> = shapes.into_iter().map(|s| add(&mut m, s)).collect();
            assert!(plain(&m, x) && ids.iter().all(|id| plain(&m, *id)), "case {i}");
        }
    }

    #[test]
    fn a_t_junction_an_opening_leaves_no_room_for_is_not_made() {
        let window = |along: f64| vec![Opening { along, sill: 0.9, width: 1.2, height: 1.2 }];
        let both = |runs: Vec<Opening>, stands: Vec<Opening>| {
            let mut m = Model::new(TileGrid::default());
            let x = add(&mut m, Shape::System(SystemDef { openings: runs, ..def((0.0, 0.0), (L, 0.0), 2.7, EXTERIOR) }));
            let y = add(&mut m, Shape::System(SystemDef { openings: stands, ..def((2.4, 0.0), (2.4, 3.0), 2.7, EXTERIOR) }));
            (m, x, y)
        };
        // a window where the other stands, and one whose framing would stand in the studs beside
        // it: both frames stay as they are
        for along in [1.8, 2.5] {
            let (m, x, y) = both(window(along), Vec::new());
            assert!(plain(&m, x) && plain(&m, y), "{along}");
        }
        // a window of the other too near its end
        let (m, x, y) = both(Vec::new(), window(0.2));
        assert!(plain(&m, x) && plain(&m, y));
        // with room for them the T junction is made
        let (m, x, y) = both(window(0.6), window(1.0));
        assert!(one(&of(&m, x), y, 2.4, true));
        let (xs, ys) = (pieces(&m, x), pieces(&m, y));
        assert_eq!(overlap(&xs, &ys), None);
        assert_eq!((overlap_within(&xs), overlap_within(&ys)), (None, None));
    }

    #[test]
    fn a_t_junction_follows_its_frames_as_they_change() {
        let (mut m, x, y) = tee(INTERIOR);
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
        assert!(one(&of(&m, x), y, 2.4, true));
        assert_eq!(overlap(&pieces(&m, x), &pieces(&m, y)), None);
        m.remove(y);
        assert!(plain(&m, x));
    }
}
