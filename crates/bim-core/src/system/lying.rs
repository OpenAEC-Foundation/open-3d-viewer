//! Lying systems (floors, ceilings) beyond the plain rectangle: one laid in an outline, with
//! openings in it, and with the steel trimmers that carry the slabs an opening cuts through.
//!
//! - **An outline.** A lying system with an `outline` is laid over the rectangle that holds the
//!   outline, along its line, and then cut to the outline: a slab the outline crosses is cut on
//!   it, one outside it is left out.
//! - **Openings.** `along` is measured along the system's line from its start and `sill` across
//!   it, to its left, as a wall's sill is measured up. A rectangular opening cuts the panels and
//!   the joints it lies in; a round one (its `width` the diameter) is drilled through whatever it
//!   meets.
//! - **Trimmers.** A type with a [`Trimmer`] rule (a hollow-core floor) does not let an opening
//!   wider than the rule's `over` cut a slab lengthwise: the opening is widened to the seams of
//!   the slabs it touches, so whole slabs stop at it, and the ends of those slabs hang in a steel
//!   trimmer either side of it: an angle under the cut ends, with a lug on the next slab at each
//!   end.

use super::Face;
use crate::boolean::{boolean, BooleanOp};
use crate::element::{Opening, PartSolid, SystemDef};
use crate::profile::Profile;
use crate::solid::{Axes, Solid};
use crate::{Refusal, LINEAR_TOLERANCE};
use glam::{DVec2, DVec3};

/// How far past a lying system its cutting solids reach up and down (metres).
const THROUGH: f64 = 5.0;
/// How far a cut that removes a joint beside an opening reaches past the joint (metres).
const PAST: f64 = 1e-4;

/// Whether opening `o` is round: drilled, its `width` the diameter.
pub(super) fn round(o: &Opening) -> bool {
    o.framing.is_some_and(|f| f.round)
}

/// A lying system `s` with an outline as the rectangle that holds the outline, laid along its
/// line: that rectangle as a system, its openings measured from the rectangle's corner, and the
/// outline in the rectangle's frame (along it, across it). `None` for one without an outline.
pub(super) fn bounded(s: &SystemDef) -> Result<Option<(SystemDef, Vec<DVec2>)>, Refusal> {
    if s.outline.is_empty() || !s.lies() {
        return Ok(None);
    }
    if s.outline.len() < 3 {
        return Err(Refusal::TooFewPoints);
    }
    let dir = (s.end - s.start).try_normalize().ok_or(Refusal::ZeroLength)?;
    let up = dir.perp();
    let local: Vec<DVec2> = s.outline.iter().map(|p| DVec2::new((*p - s.start).dot(dir), (*p - s.start).dot(up))).collect();
    let (lo, hi) = local.iter().fold((DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    if !(hi.x - lo.x > LINEAR_TOLERANCE && hi.y - lo.y > LINEAR_TOLERANCE) {
        return Err(Refusal::Degenerate);
    }
    let start = s.start + dir * lo.x + up * lo.y;
    let openings = s.openings.iter().map(|o| Opening { along: o.along - lo.x, sill: o.sill - lo.y, ..*o }).collect();
    let rectangle = SystemDef { start, end: start + dir * (hi.x - lo.x), height: hi.y - lo.y, openings, outline: Vec::new(), ..s.clone() };
    Ok(Some((rectangle, local.into_iter().map(|p| p - lo).collect())))
}

/// The slabs' seams either side of opening `o` in a face of `bays` bays `bay` wide with joints
/// `w` wide: where the first slab it touches starts and the last one ends.
fn seams(o: &Opening, bays: usize, bay: f64, w: f64) -> (f64, f64) {
    let tol = LINEAR_TOLERANCE;
    let first = ((o.along + tol) / bay).floor().clamp(0.0, bays as f64 - 1.0);
    let last = ((o.along + o.width - tol) / bay).ceil().clamp(first + 1.0, bays as f64);
    (first * bay + w / 2.0, last * bay - w / 2.0)
}

/// Whether opening `o` of lying system `s` takes whole slabs out and hangs their ends in
/// trimmers: a rectangular one wider than its type's trimmer rule lets a slab be cut.
fn trimmed(s: &SystemDef, o: &Opening) -> bool {
    !round(o) && s.system_type.as_ref().and_then(|t| t.trimmer).is_some_and(|t| o.width > t.over + LINEAR_TOLERANCE)
}

/// The rectangular openings of lying system `s` (a rectangle `length` by its height) as they are
/// cut in its face `f`: each as it is, or widened to the seams of the slabs it touches where it
/// is trimmed. Round ones are left out: they are drilled afterwards. Refuses an opening that is
/// not inside the system, and a trimmed one with no slab beside it to hang its trimmer on.
pub(super) fn cut(s: &SystemDef, f: &Face, length: f64) -> Result<Vec<Opening>, Refusal> {
    let tol = LINEAR_TOLERANCE;
    let (bays, _, bay, _) = f.division(length, s.height);
    let mut made = Vec::with_capacity(s.openings.len());
    for o in &s.openings {
        let sized = o.width > tol && (round(o) || o.height > tol);
        let (across, deep) = (o.sill, if round(o) { o.width } else { o.height });
        if !sized || o.along < -tol || o.along + o.width > length + tol || across < -tol || across + deep > s.height + tol {
            return Err(Refusal::OpeningOutside);
        }
        if round(o) {
            continue;
        }
        if !trimmed(s, o) {
            made.push(*o);
            continue;
        }
        let (a, b) = seams(o, bays, bay, f.width());
        if a < bay - tol || b > length - bay + tol {
            return Err(Refusal::OpeningNotFramed);
        }
        made.push(Opening { along: a, width: b - a, ..*o });
    }
    Ok(made)
}

/// The prism over `ring`, given along and across lying system `s`, through the whole of it.
fn prism(s: &SystemDef, dir: DVec2, ring: Vec<DVec2>) -> Result<Solid, Refusal> {
    let axes = Axes::new(s.start.extend(s.base - THROUGH), dir.extend(0.0), DVec3::Z)?;
    Solid::extrude(&Profile::new(ring, Vec::new())?, &axes, 2.0 * THROUGH)
}

/// A box from (`u0`, `v0`) to (`u1`, `v1`) along and across lying system `s`, from `z0` up to `z1`.
fn slab(s: &SystemDef, dir: DVec2, [u0, v0, u1, v1]: [f64; 4], z0: f64, z1: f64) -> Result<Solid, Refusal> {
    let axes = Axes::new(s.start.extend(z0), dir.extend(0.0), DVec3::Z)?;
    let p = DVec2::new;
    Solid::extrude(&Profile::new(vec![p(u0, v0), p(u1, v0), p(u1, v1), p(u0, v1)], Vec::new())?, &axes, z1 - z0)
}

/// Where a part lies along and across lying system `s`: `[u0, v0, u1, v1]`.
fn footprint(s: &SystemDef, dir: DVec2, solid: &Solid) -> [f64; 4] {
    let up = dir.perp();
    solid.positions.iter().fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |r, q| {
        let flat = DVec2::new(q.x, q.y) - s.start;
        let (u, v) = (flat.dot(dir), flat.dot(up));
        [r[0].min(u), r[1].min(v), r[2].max(u), r[3].max(v)]
    })
}

/// How a rectangle lies to an outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lies {
    Inside,
    Outside,
    Across,
}

/// How rectangle `r` lies to `ring`: across it when an edge of the ring runs through its inside,
/// else in it or out of it as its middle is.
fn lies(r: [f64; 4], ring: &[DVec2]) -> Lies {
    let tol = 1e-7;
    let (lo, hi) = (DVec2::new(r[0] + tol, r[1] + tol), DVec2::new(r[2] - tol, r[3] - tol));
    let n = ring.len();
    let through = |a: DVec2, b: DVec2| {
        // the part of the edge inside the rectangle, by its parameter along the edge
        let d = b - a;
        let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
        for (p, q) in [(-d.x, a.x - lo.x), (d.x, hi.x - a.x), (-d.y, a.y - lo.y), (d.y, hi.y - a.y)] {
            if p.abs() < 1e-15 {
                if q < 0.0 {
                    return false;
                }
            } else if p < 0.0 {
                t0 = t0.max(q / p);
            } else {
                t1 = t1.min(q / p);
            }
        }
        (t1 - t0) * d.length() > tol
    };
    if hi.x <= lo.x || hi.y <= lo.y {
        return Lies::Outside;
    }
    if (0..n).any(|i| through(ring[i], ring[(i + 1) % n])) {
        return Lies::Across;
    }
    let c = (lo + hi) / 2.0;
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        if (a.y > c.y) != (b.y > c.y) && c.x < a.x + (c.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    if inside {
        Lies::Inside
    } else {
        Lies::Outside
    }
}

/// `part` less `tool`; `None` when nothing of it is left.
fn less(part: &PartSolid, tool: &Solid) -> Result<Option<PartSolid>, Refusal> {
    match boolean(&part.solid, tool, BooleanOp::Difference) {
        Ok(solid) => Ok(Some(PartSolid { solid, ..part.clone() })),
        Err(Refusal::Empty) => Ok(None),
        Err(refusal) => Err(refusal),
    }
}

/// The parts of lying system `s` (a rectangle `length` by its height, generated with the
/// rectangular openings of [`cut`]) finished: its joints cut where an opening lies, its round
/// openings drilled, all of it cut to `outline` (in the rectangle's frame) when it has one, and
/// the trimmers of its trimmed openings added. `top` is the height of the top of its own face, a
/// slab `depth` thick with joints `w` wide.
pub(super) fn finish(s: &SystemDef, f: &Face, length: f64, top: f64, outline: Option<&[DVec2]>, parts: Vec<PartSolid>) -> Result<Vec<PartSolid>, Refusal> {
    if s.openings.is_empty() && outline.is_none() {
        return Ok(parts);
    }
    let tol = LINEAR_TOLERANCE;
    let dir = (s.end - s.start).try_normalize().ok_or(Refusal::ZeroLength)?;
    let w = f.width();
    let (bays, _, bay, _) = f.division(length, s.height);
    // what cuts the joints: every rectangular opening, a trimmed one with the joints beside it;
    // and what is drilled through everything
    let mut blocks: Vec<([f64; 4], Solid)> = Vec::new();
    let mut drills: Vec<([f64; 4], Solid)> = Vec::new();
    let mut trimmers: Vec<PartSolid> = Vec::new();
    for o in &s.openings {
        if round(o) {
            let (r, c) = (o.width / 2.0, DVec2::new(o.along + o.width / 2.0, o.sill + o.width / 2.0));
            let axes = Axes::new((s.start + dir * c.x + dir.perp() * c.y).extend(s.base - THROUGH), dir.extend(0.0), DVec3::Z)?;
            drills.push(([c.x - r, c.y - r, c.x + r, c.y + r], Solid::extrude(&Profile::circle(o.width)?, &axes, 2.0 * THROUGH)?));
            continue;
        }
        let (v0, v1) = (o.sill, o.sill + o.height);
        let r = if trimmed(s, o) {
            let (a, b) = seams(o, bays, bay, w);
            trimmers.extend(hung(s, dir, [a, v0, b, v1], top)?);
            [a - w - PAST, v0, b + w + PAST, v1]
        } else {
            [o.along, v0, o.along + o.width, v1]
        };
        blocks.push((r, prism(s, dir, vec![DVec2::new(r[0], r[1]), DVec2::new(r[2], r[1]), DVec2::new(r[2], r[3]), DVec2::new(r[0], r[3])])?));
    }
    let edge = outline.map(|ring| prism(s, dir, ring.to_vec())).transpose()?;
    let meet = |a: [f64; 4], b: [f64; 4]| a[0] < b[2] - tol && b[0] < a[2] - tol && a[1] < b[3] - tol && b[1] < a[3] - tol;
    let mut made = Vec::with_capacity(parts.len() + trimmers.len());
    'parts: for part in parts {
        let mut part = part;
        let at = footprint(s, dir, &part.solid);
        let joint = part.class == "IfcMember";
        let cuts = blocks.iter().filter(|_| joint).chain(drills.iter());
        for (r, tool) in cuts {
            if meet(at, *r) {
                match less(&part, tool)? {
                    Some(left) => part = left,
                    None => continue 'parts,
                }
            }
        }
        if let (Some(ring), Some(edge)) = (outline, &edge) {
            match lies(at, ring) {
                Lies::Inside => {}
                Lies::Outside => continue,
                Lies::Across => match boolean(&part.solid, edge, BooleanOp::Intersection) {
                    Ok(solid) => part.solid = solid,
                    Err(Refusal::Empty) => continue,
                    Err(refusal) => return Err(refusal),
                },
            }
        }
        made.push(part);
    }
    made.extend(trimmers);
    Ok(made)
}

/// The trimmers of an opening from (`u0`, `v0`) to (`u1`, `v1`) that whole slabs stop at: at
/// either end of it across the slabs, where a slab is cut (not at the system's own edge), an
/// angle with its upright leg against the cut ends and its seat under them, and a lug on the
/// next slab at each of its ends.
fn hung(s: &SystemDef, dir: DVec2, [u0, v0, u1, v1]: [f64; 4], top: f64) -> Result<Vec<PartSolid>, Refusal> {
    let Some(rule) = s.system_type.as_ref().and_then(|t| t.trimmer) else { return Ok(Vec::new()) };
    let tol = LINEAR_TOLERANCE;
    let depth = s.member.y.max(s.panel);
    let (t, seat, bearing) = (rule.thickness, rule.seat, rule.bearing);
    let steel = |solid: Solid| PartSolid { class: "IfcMember", solid, material: Some("steel".to_string()) };
    let mut parts = Vec::new();
    // at `v`, the opening to the side `into` of it (1 or -1 across)
    for (v, into) in [(v0, 1.0), (v1, -1.0)] {
        if (into > 0.0 && v < tol) || (into < 0.0 && v > s.height - tol) {
            continue;
        }
        // the angle, along the slabs' ends: its section across the system and up
        let (leg, under) = (v + into * t, v - into * seat);
        let p = DVec2::new;
        let section = vec![p(under, top - depth - t), p(leg, top - depth - t), p(leg, top), p(v, top), p(v, top - depth), p(under, top - depth)];
        let axes = Axes::new((s.start + dir * u0).extend(0.0), dir.perp().extend(0.0), dir.extend(0.0))?;
        parts.push(steel(Solid::extrude(&Profile::new(section, Vec::new())?, &axes, u1 - u0)?));
        // the lugs, on the slabs beside the opening
        let (lo, hi) = (under.min(leg), under.max(leg));
        for (a, b) in [(u0 - bearing, u0), (u1, u1 + bearing)] {
            parts.push(steel(slab(s, dir, [a, lo, b, hi], top, top + t)?));
        }
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::super::parts;
    use crate::boolean::{boolean, BooleanOp};
    use crate::element::{Opening, OpeningFraming, PartSolid, SystemDef};
    use crate::Refusal;
    use bim_library::Library;
    use glam::DVec2;

    /// A floor of type `id`, `length` along x from the origin and `width` to its left, its top at 3.0.
    fn floor(length: f64, width: f64, id: &str) -> SystemDef {
        let generic = SystemDef { start: DVec2::ZERO, end: DVec2::new(length, 0.0), base: 3.0, height: width, spacing: DVec2::new(1.5, 1.5), member: DVec2::new(0.05, 0.15), panel: 0.02, system_type: None, openings: Vec::new(), breaks: Vec::new(), outline: Vec::new() };
        generic.laid(Library::starter().system_type(id).unwrap()).unwrap()
    }

    fn of<'a>(parts: &'a [PartSolid], material: &str) -> Vec<&'a PartSolid> {
        parts.iter().filter(|p| p.material.as_deref() == Some(material)).collect()
    }

    fn volume(parts: &[&PartSolid]) -> f64 {
        parts.iter().map(|p| p.solid.volume()).sum()
    }

    fn rect(along: f64, sill: f64, width: f64, height: f64) -> Opening {
        Opening { along, sill, width, height, framing: None }
    }

    fn drilled(along: f64, sill: f64, diameter: f64) -> Opening {
        Opening { along, sill, width: diameter, height: diameter, framing: Some(OpeningFraming { round: true, ..Default::default() }) }
    }

    /// No two of `parts` share volume.
    fn apart(parts: &[PartSolid]) {
        for (i, a) in parts.iter().enumerate() {
            for b in &parts[i + 1..] {
                let ((alo, ahi), (blo, bhi)) = (a.solid.bounds(), b.solid.bounds());
                if alo.cmplt(bhi - 1e-6).all() && blo.cmplt(ahi - 1e-6).all() {
                    match boolean(&a.solid, &b.solid, BooleanOp::Intersection) {
                        Ok(common) => assert!(common.volume() < 1e-9, "{:?} and {:?} share {}", a.material, b.material, common.volume()),
                        Err(Refusal::Empty) => {}
                        Err(e) => panic!("{e:?}"),
                    }
                }
            }
        }
    }

    #[test]
    fn a_floor_in_an_outline_is_cut_to_it() {
        // four slabs 1200 wide: an L, two of them 6 m long and two 3 m
        let p = DVec2::new;
        let plain = floor(4.8, 6.0, "system.hollow-core-200");
        let whole = parts(&plain).unwrap();
        let l = SystemDef { outline: vec![p(0.0, 0.0), p(4.8, 0.0), p(4.8, 3.0), p(2.4, 3.0), p(2.4, 6.0), p(0.0, 6.0)], ..plain.clone() };
        let made = parts(&l).unwrap();
        let slabs = of(&made, "hollow-core-concrete");
        assert_eq!(slabs.len(), 4);
        let long = slabs.iter().filter(|s| s.solid.bounds().1.y > 5.9).count();
        assert_eq!(long, 2, "{:?}", slabs.iter().map(|s| s.solid.bounds()).collect::<Vec<_>>());
        assert!(slabs.iter().all(|s| s.solid.bounds().0.x > 2.4 || s.solid.bounds().1.y > 5.9 || s.solid.bounds().1.y < 3.0 + 1e-9));
        // nothing of it outside the outline: slabs and joints fill it, the joints half outside
        let all: Vec<&PartSolid> = made.iter().collect();
        let area = 4.8 * 3.0 + 2.4 * 3.0;
        assert!(volume(&all) > (area - 0.3) * 0.2 && volume(&all) < (area + 0.3) * 0.2, "{}", volume(&all));
        assert!(made.iter().all(|q| q.solid.bounds().1.x < 2.4 + 0.011 || q.solid.bounds().1.y < 3.0 + 0.011), "the corner is left out");
        apart(&made);
        // the rectangle as an outline is the rectangle, wherever the outline starts
        let same = SystemDef { outline: vec![p(4.8, 6.0), p(0.0, 6.0), p(0.0, 0.0), p(4.8, 0.0)], ..plain.clone() };
        let again = parts(&same).unwrap();
        // (the joints round its edge end on the outline: half of them is left)
        assert_eq!(again.len(), whole.len());
        let less = volume(&whole.iter().collect::<Vec<_>>()) - volume(&again.iter().collect::<Vec<_>>());
        assert!(less > 0.0 && less < 2.0 * (4.8 + 6.0) * 0.011 * 0.2, "{less}");
        assert_eq!(of(&again, "hollow-core-concrete").len(), 4);
        // an outline that is none is refused
        assert_eq!(parts(&SystemDef { outline: vec![p(0.0, 0.0), p(1.0, 0.0)], ..plain.clone() }), Err(Refusal::TooFewPoints));
        assert_eq!(parts(&SystemDef { outline: vec![p(0.0, 0.0), p(4.0, 0.0), p(0.0, 4.0), p(4.0, 4.0)], ..plain }), Err(Refusal::SelfIntersecting));
    }

    #[test]
    fn a_small_opening_is_cut_in_its_slab_and_a_round_one_drilled() {
        let plain = floor(4.8, 6.0, "system.hollow-core-200");
        let whole = volume(&of(&parts(&plain).unwrap(), "hollow-core-concrete"));
        // 300 by 300 in the second slab: no trimmer
        let notched = SystemDef { openings: vec![rect(1.6, 2.0, 0.3, 0.3)], ..plain.clone() };
        let made = parts(&notched).unwrap();
        assert!(of(&made, "steel").is_empty());
        assert!((whole - volume(&of(&made, "hollow-core-concrete")) - 0.3 * 0.3 * 0.2).abs() < 1e-9);
        // a core of 150 drilled through a slab, and one of 200 through a joint between two
        let cored = SystemDef { openings: vec![drilled(0.5, 1.0, 0.15), drilled(2.3, 4.0, 0.2)], ..plain.clone() };
        let made = parts(&cored).unwrap();
        let (a, b) = (std::f64::consts::PI * 0.075 * 0.075, std::f64::consts::PI * 0.1 * 0.1);
        let all: Vec<&PartSolid> = made.iter().collect();
        let before: f64 = parts(&plain).unwrap().iter().map(|p| p.solid.volume()).sum();
        let gone = before - volume(&all);
        assert!((gone - (a + b) * 0.2).abs() < 0.002 * 0.2, "{gone} of {}", (a + b) * 0.2);
        assert_eq!(of(&made, "hollow-core-concrete").len(), 4);
        // an opening outside the floor is refused
        assert_eq!(parts(&SystemDef { openings: vec![rect(4.6, 2.0, 0.3, 0.3)], ..plain.clone() }), Err(Refusal::OpeningOutside));
        assert_eq!(parts(&SystemDef { openings: vec![drilled(1.0, 5.9, 0.2)], ..plain }), Err(Refusal::OpeningOutside));
    }

    #[test]
    fn a_wide_opening_takes_whole_slabs_out_and_hangs_their_ends_in_trimmers() {
        // 800 wide from 1.5 along: wider than a slab may be cut, so the second slab (1210 to
        // 2390) stops either side of it
        let plain = floor(4.8, 6.0, "system.hollow-core-200");
        let opened = SystemDef { openings: vec![rect(1.5, 2.0, 0.8, 1.0)], ..plain.clone() };
        let made = parts(&opened).unwrap();
        let slabs: Vec<_> = of(&made, "hollow-core-concrete").iter().map(|p| p.solid.bounds()).collect();
        assert_eq!(slabs.len(), 5, "the second slab in two");
        let cut: Vec<_> = slabs.iter().filter(|(lo, hi)| lo.x > 1.2 && hi.x < 2.4).collect();
        assert_eq!(cut.len(), 2);
        assert!(cut.iter().any(|(lo, hi)| lo.y.abs() < 1e-9 + 0.011 && (hi.y - 2.0).abs() < 1e-9) && cut.iter().any(|(lo, hi)| (lo.y - 3.0).abs() < 1e-9 && hi.y > 5.9), "{cut:?}");
        // two angles as long as the slab is wide, their seat under the cut ends, and four lugs on
        // the slabs beside it
        let steel: Vec<_> = of(&made, "steel").iter().map(|p| p.solid.bounds()).collect();
        assert_eq!(steel.len(), 6);
        let angles: Vec<_> = steel.iter().filter(|(lo, hi)| (hi.x - lo.x - 1.18).abs() < 1e-9).collect();
        assert_eq!(angles.len(), 2, "{steel:?}");
        for (lo, hi) in &angles {
            assert!((lo.z - (2.8 - 0.008)).abs() < 1e-9 && (hi.z - 3.0).abs() < 1e-9 && (hi.y - lo.y - 0.088).abs() < 1e-9, "{lo} {hi}");
        }
        let lugs: Vec<_> = steel.iter().filter(|(lo, hi)| (hi.x - lo.x - 0.15).abs() < 1e-9).collect();
        assert_eq!(lugs.len(), 4);
        assert!(lugs.iter().all(|(lo, hi)| (lo.z - 3.0).abs() < 1e-9 && (hi.z - 3.008).abs() < 1e-9 && (hi.x < 1.21 + 1e-9 || lo.x > 2.39 - 1e-9)));
        // the joints beside the opening stop at it, and nothing shares volume
        let grout = volume(&of(&parts(&plain).unwrap(), "grout")) - volume(&of(&made, "grout"));
        assert!((grout - 2.0 * 1.0 * 0.02 * 0.2).abs() < 1e-9, "{grout}");
        apart(&made);
        // against the floor's side there is no slab to hang it on
        assert_eq!(parts(&SystemDef { openings: vec![rect(0.2, 2.0, 0.8, 1.0)], ..plain.clone() }), Err(Refusal::OpeningNotFramed));
        // at the floor's end only the far side of it has a trimmer
        let at_end = SystemDef { openings: vec![rect(1.5, 0.0, 0.8, 1.0)], ..plain };
        assert_eq!(of(&parts(&at_end).unwrap(), "steel").len(), 3);
    }

    #[test]
    fn a_wide_slab_floor_has_its_topping_on_its_planks_under_its_top() {
        let plain = floor(4.8, 6.0, "system.wide-slab-200");
        let made = parts(&plain).unwrap();
        let planks: Vec<_> = of(&made, "wide-slab").iter().map(|p| p.solid.bounds()).collect();
        assert_eq!(planks.len(), 2, "two planks 2400 wide");
        assert!(planks.iter().all(|(lo, hi)| (lo.z - 2.8).abs() < 1e-9 && (hi.z - 2.86).abs() < 1e-9), "{planks:?}");
        // the topping is one piece over them, its top the floor's
        let topping: Vec<_> = of(&made, "topping-concrete").iter().map(|p| p.solid.bounds()).filter(|(lo, _)| lo.z > 2.85).collect();
        assert_eq!(topping.len(), 1);
        assert!((topping[0].0.z - 2.86).abs() < 1e-9 && (topping[0].1.z - 3.0).abs() < 1e-9 && topping[0].1.x - topping[0].0.x > 4.79);
        apart(&made);
        // an opening goes through both, with no trimmer; a core too
        let opened = SystemDef { openings: vec![rect(1.0, 2.0, 0.9, 0.6), drilled(3.5, 1.0, 0.2)], ..plain.clone() };
        let cut = parts(&opened).unwrap();
        assert!(of(&cut, "steel").is_empty());
        let (before, after): (f64, f64) = (made.iter().map(|p| p.solid.volume()).sum(), cut.iter().map(|p| p.solid.volume()).sum());
        let gone = (0.9 * 0.6 + std::f64::consts::PI * 0.01) * 0.2;
        assert!((before - after - gone).abs() < 0.001, "{} of {gone}", before - after);
        // and it is laid in an outline as any floor
        let p = DVec2::new;
        let l = SystemDef { outline: vec![p(0.0, 0.0), p(4.8, 0.0), p(4.8, 3.0), p(2.0, 3.0), p(2.0, 6.0), p(0.0, 6.0)], ..plain };
        let made = parts(&l).unwrap();
        let all: f64 = made.iter().map(|p| p.solid.volume()).sum();
        let area = 4.8 * 3.0 + 2.0 * 3.0;
        assert!((all - area * 0.2).abs() < 0.02, "{all} of {}", area * 0.2);
    }
}
