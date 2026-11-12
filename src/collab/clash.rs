//! Clash detection between the elements of the open models, on their meshes.
//!
//! Two elements clash **hard** when their surfaces cross and one reaches further than the
//! tolerance through the other's surface: elements that only touch (a wall on a floor) do not
//! clash. An element that lies wholly inside another clashes too. Two elements clash on
//! **clearance** when they do not cross and come nearer than the clearance asked for.
//!
//! The work is in two steps: the boxes of the elements, swept along x, give the pairs that can
//! clash; for each pair the triangles in the part their boxes share are tested one against one.

use glam::Vec3;
use std::sync::atomic::{AtomicBool, Ordering};

pub type Triangle = [Vec3; 3];

/// An element as the clash test sees it: its mesh and its box (metres).
#[derive(Debug, Clone)]
pub struct Solid {
    pub triangles: Vec<Triangle>,
    pub min: Vec3,
    pub max: Vec3,
}

impl Solid {
    /// An element of these triangles; none without one.
    pub fn new(triangles: Vec<Triangle>) -> Option<Solid> {
        let first = *triangles.first()?;
        let (min, max) = triangles.iter().flatten().fold((first[0], first[0]), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        Some(Solid { triangles, min, max })
    }
}

/// What is looked for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Test {
    /// Elements that cross by more than `tolerance` (metres).
    Hard { tolerance: f32 },
    /// Elements nearer than `distance` (metres) that do not cross.
    Clearance { distance: f32 },
}

/// How two elements clash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Their surfaces cross.
    Crossing,
    /// The first lies inside the second, or the second inside the first.
    Inside,
    /// They are nearer than the clearance.
    Near,
}

/// A clash of element `a` of the first set with element `b` of the second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clash {
    pub a: usize,
    pub b: usize,
    pub kind: Kind,
    /// Where: the middle of where they cross, or between their nearest points.
    pub at: Vec3,
    /// How far they cross, or how far they are apart (metres).
    pub distance: f32,
}

/// The clashes of the elements of `first` with those of `second`; with `within`, the two are the
/// same set and each pair is tested once. Stops early, with what it found, when `cancel` is set.
pub fn clashes(first: &[Solid], second: &[Solid], within: bool, test: Test, cancel: &AtomicBool) -> Vec<Clash> {
    clashes_but(first, second, within, test, &|_, _| false, cancel)
}

/// As [`clashes`], leaving out the pairs `skip` names (by their places in `first` and `second`)
/// before they are tested: elements of one model, say, in a test between models.
pub fn clashes_but(first: &[Solid], second: &[Solid], within: bool, test: Test, skip: &dyn Fn(usize, usize) -> bool, cancel: &AtomicBool) -> Vec<Clash> {
    let reach = match test {
        Test::Hard { .. } => 0.0,
        Test::Clearance { distance } => distance.max(0.0),
    };
    // the second set by where its boxes start along x
    let mut order: Vec<usize> = (0..second.len()).collect();
    order.sort_by(|&i, &j| second[i].min.x.total_cmp(&second[j].min.x));
    let widest = second.iter().map(|s| s.max.x - s.min.x).fold(0.0f32, f32::max);
    let mut out = Vec::new();
    for (i, a) in first.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        // those that can start before `a` ends and end after it starts
        let from = order.partition_point(|&j| second[j].min.x < a.min.x - reach - widest);
        for &j in order[from..].iter().take_while(|&&j| second[j].min.x <= a.max.x + reach) {
            if within && j <= i {
                continue;
            }
            let b = &second[j];
            let apart = (a.min - b.max).max(b.min - a.max);
            if apart.max_element() > reach || skip(i, j) {
                continue;
            }
            let found = match test {
                Test::Hard { tolerance } => hard(a, b, tolerance.max(0.0)),
                Test::Clearance { distance } => near(a, b, distance),
            };
            out.extend(found.map(|(kind, at, distance)| Clash { a: i, b: j, kind, at, distance }));
        }
    }
    out
}

/// The triangles of a solid that reach into a box.
fn within(solid: &Solid, lo: Vec3, hi: Vec3) -> Vec<Triangle> {
    let reaches = |t: &Triangle| {
        let (min, max) = (t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2]));
        min.cmple(hi).all() && max.cmpge(lo).all()
    };
    solid.triangles.iter().filter(|t| reaches(t)).copied().collect()
}

fn normal(t: &Triangle) -> Vec3 {
    (t[1] - t[0]).cross(t[2] - t[0])
}

/// Where triangle `t` crosses the plane through `origin` with unit normal `n`: the two points on
/// the plane, and how far `t` reaches to each side of it.
fn across(t: &Triangle, origin: Vec3, n: Vec3) -> Option<([Vec3; 2], f32)> {
    let d = t.map(|p| (p - origin).dot(n));
    let (above, below) = (d.iter().fold(0.0f32, |m, &x| m.max(x)), d.iter().fold(0.0f32, |m, &x| m.max(-x)));
    if above <= 0.0 || below <= 0.0 {
        return None;
    }
    let mut points = Vec::with_capacity(2);
    for k in 0..3 {
        let (i, j) = (k, (k + 1) % 3);
        if (d[i] > 0.0) != (d[j] > 0.0) && d[i] != d[j] {
            points.push(t[i] + (t[j] - t[i]) * (d[i] / (d[i] - d[j])));
        }
    }
    (points.len() == 2).then(|| ([points[0], points[1]], above.min(below)))
}

/// Where two triangles cross: the middle of the line they share, its length, and how far the
/// one that reaches furthest goes through the other's plane. None when they do not cross, also
/// when they lie in one plane.
fn crossing(s: &Triangle, t: &Triangle) -> Option<(Vec3, f32, f32)> {
    let (ns, nt) = (normal(s).try_normalize()?, normal(t).try_normalize()?);
    let (ps, through_t) = across(s, t[0], nt)?;
    let (pt, through_s) = across(t, s[0], ns)?;
    // both pairs lie on the line the planes share: the part of it in both triangles
    let line = ns.cross(nt).try_normalize()?;
    let along = |p: Vec3| p.dot(line);
    let (s0, s1) = (along(ps[0]).min(along(ps[1])), along(ps[0]).max(along(ps[1])));
    let (t0, t1) = (along(pt[0]).min(along(pt[1])), along(pt[0]).max(along(pt[1])));
    let (from, to) = (s0.max(t0), s1.min(t1));
    if to <= from {
        return None;
    }
    // a point of the line: one of the crossing points, moved along it
    let base = ps[0] + line * ((from + to) / 2.0 - along(ps[0]));
    Some((base, to - from, through_t.max(through_s)))
}

/// Whether a point lies inside a closed mesh: a ray from it crosses the mesh an odd number of
/// times. The ray's direction is skew, so it seldom runs along an edge.
fn inside(p: Vec3, solid: &Solid) -> bool {
    if p.cmplt(solid.min).any() || p.cmpgt(solid.max).any() {
        return false;
    }
    let dir = Vec3::new(0.577_215_7, 0.301_029_99, 0.757_872_1).normalize();
    let mut hits = 0;
    for t in &solid.triangles {
        // Möller–Trumbore
        let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
        let h = dir.cross(e2);
        let det = e1.dot(h);
        if det.abs() < 1e-12 {
            continue;
        }
        let s = p - t[0];
        let u = s.dot(h) / det;
        let q = s.cross(e1);
        let v = dir.dot(q) / det;
        if u < 0.0 || v < 0.0 || u + v > 1.0 {
            continue;
        }
        if e2.dot(q) / det > 1e-7 {
            hits += 1;
        }
    }
    hits % 2 == 1
}

/// How two solids clash hard, where, and by how much.
fn hard(a: &Solid, b: &Solid, tolerance: f32) -> Option<(Kind, Vec3, f32)> {
    let (lo, hi) = (a.min.max(b.min), a.max.min(b.max));
    // how far the boxes share: no more than that can cross
    let shared = (hi - lo).min_element();
    if shared <= tolerance {
        return None;
    }
    let (ta, tb) = (within(a, lo - Vec3::splat(1e-4), hi + Vec3::splat(1e-4)), within(b, lo - Vec3::splat(1e-4), hi + Vec3::splat(1e-4)));
    let (mut sum, mut weight, mut deepest) = (Vec3::ZERO, 0.0f32, 0.0f32);
    for s in &ta {
        let (smin, smax) = (s[0].min(s[1]).min(s[2]), s[0].max(s[1]).max(s[2]));
        for t in &tb {
            let (tmin, tmax) = (t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2]));
            if smin.cmpgt(tmax).any() || tmin.cmpgt(smax).any() {
                continue;
            }
            if let Some((at, length, through)) = crossing(s, t) {
                if through > tolerance && length > 1e-6 {
                    sum += at * length;
                    weight += length;
                    deepest = deepest.max(through);
                }
            }
        }
    }
    if weight > 0.0 {
        return Some((Kind::Crossing, sum / weight, deepest.min(shared)));
    }
    // no surface crosses: one may lie inside the other
    let middle = |s: &Solid| s.triangles.first().map(|t| (t[0] + t[1] + t[2]) / 3.0);
    for (inner, outer) in [(a, b), (b, a)] {
        let fits = inner.min.cmpge(outer.min - Vec3::splat(1e-4)).all() && inner.max.cmple(outer.max + Vec3::splat(1e-4)).all();
        if middle(inner).is_some_and(|p| fits && inside(p, outer)) {
            let size = (inner.max - inner.min).min_element();
            return (size > tolerance).then_some((Kind::Inside, (inner.min + inner.max) / 2.0, size));
        }
    }
    None
}

/// The nearest points of a point and a triangle.
fn nearest_on(p: Vec3, t: &Triangle) -> Vec3 {
    // Ericson, Real-Time Collision Detection 5.1.5
    let (a, b, c) = (t[0], t[1], t[2]);
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

/// The nearest points of two segments.
fn nearest_between(p1: Vec3, q1: Vec3, p2: Vec3, q2: Vec3) -> (Vec3, Vec3) {
    // Ericson, Real-Time Collision Detection 5.1.9
    let (d1, d2, r) = (q1 - p1, q2 - p2, p1 - p2);
    let (a, e, f) = (d1.length_squared(), d2.length_squared(), d2.dot(r));
    let (s, t);
    if a <= 1e-12 && e <= 1e-12 {
        return (p1, p2);
    }
    if a <= 1e-12 {
        s = 0.0;
        t = (f / e).clamp(0.0, 1.0);
    } else {
        let c = d1.dot(r);
        if e <= 1e-12 {
            t = 0.0;
            s = (-c / a).clamp(0.0, 1.0);
        } else {
            let b = d1.dot(d2);
            let denom = a * e - b * b;
            let mut s0 = if denom > 1e-12 { ((b * f - c * e) / denom).clamp(0.0, 1.0) } else { 0.0 };
            let mut t0 = (b * s0 + f) / e;
            if t0 < 0.0 {
                t0 = 0.0;
                s0 = (-c / a).clamp(0.0, 1.0);
            } else if t0 > 1.0 {
                t0 = 1.0;
                s0 = ((b - c) / a).clamp(0.0, 1.0);
            }
            s = s0;
            t = t0;
        }
    }
    (p1 + d1 * s, p2 + d2 * t)
}

/// The nearest points of two triangles that do not cross: a corner of one and the other, or two
/// of their edges.
fn nearest(s: &Triangle, t: &Triangle) -> (Vec3, Vec3) {
    let mut best = (s[0], nearest_on(s[0], t));
    let mut least = best.0.distance_squared(best.1);
    let mut take = |p: Vec3, q: Vec3| {
        let d = p.distance_squared(q);
        if d < least {
            least = d;
            best = (p, q);
        }
    };
    for k in 0..3 {
        take(s[k], nearest_on(s[k], t));
        let on_s = nearest_on(t[k], s);
        take(on_s, t[k]);
        for m in 0..3 {
            let (p, q) = nearest_between(s[k], s[(k + 1) % 3], t[m], t[(m + 1) % 3]);
            take(p, q);
        }
    }
    best
}

/// Whether two solids that do not cross are nearer than `distance`: where, and how far apart.
fn near(a: &Solid, b: &Solid, distance: f32) -> Option<(Kind, Vec3, f32)> {
    let grow = Vec3::splat(distance.max(0.0));
    let (lo, hi) = (a.min.max(b.min) - grow, a.max.min(b.max) + grow);
    let (ta, tb) = (within(a, lo, hi), within(b, lo, hi));
    let mut best: Option<(Vec3, f32)> = None;
    for s in &ta {
        let (smin, smax) = (s[0].min(s[1]).min(s[2]) - grow, s[0].max(s[1]).max(s[2]) + grow);
        for t in &tb {
            let (tmin, tmax) = (t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2]));
            if smin.cmpgt(tmax).any() || tmin.cmpgt(smax).any() {
                continue;
            }
            // crossing elements are a hard clash, not one on clearance
            if crossing(s, t).is_some_and(|(_, length, through)| length > 1e-6 && through > 1e-5) {
                return None;
            }
            let (p, q) = nearest(s, t);
            let d = p.distance(q);
            if d < distance && best.is_none_or(|(_, least)| d < least) {
                best = Some(((p + q) / 2.0, d));
            }
        }
    }
    best.map(|(at, d)| (Kind::Near, at, d))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box from `lo` to `hi` as twelve triangles, facing out.
    pub(crate) fn block(lo: [f32; 3], hi: [f32; 3]) -> Solid {
        let p = |i: usize| Vec3::new(if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] });
        let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
        let triangles = quads.iter().flat_map(|q| [[p(q[0]), p(q[1]), p(q[2])], [p(q[0]), p(q[2]), p(q[3])]]).collect();
        Solid::new(triangles).unwrap()
    }

    fn run(a: Solid, b: Solid, test: Test) -> Vec<Clash> {
        clashes(&[a], &[b], false, test, &AtomicBool::new(false))
    }

    #[test]
    fn a_wall_standing_on_a_floor_does_not_clash_and_one_sunk_into_it_does() {
        let floor = || block([0.0, 0.0, -0.2], [5.0, 5.0, 0.0]);
        let hard = Test::Hard { tolerance: 0.002 };
        assert!(run(block([1.0, 1.0, 0.0], [1.2, 4.0, 3.0]), floor(), hard).is_empty(), "touching");
        // sunk 1 mm: within the tolerance
        assert!(run(block([1.0, 1.0, -0.001], [1.2, 4.0, 3.0]), floor(), hard).is_empty(), "within tolerance");
        let found = run(block([1.0, 1.0, -0.05], [1.2, 4.0, 3.0]), floor(), hard);
        assert_eq!(found.len(), 1);
        let c = found[0];
        assert_eq!(c.kind, Kind::Crossing);
        assert!((c.distance - 0.05).abs() < 1e-3, "{c:?}");
        assert!(c.at.x > 0.9 && c.at.x < 1.3 && c.at.y > 0.9 && c.at.y < 4.1 && c.at.z.abs() < 0.06, "{c:?}");
    }

    #[test]
    fn a_duct_through_a_beam_clashes_and_one_inside_a_wall_too() {
        let beam = block([0.0, 0.0, 2.6], [6.0, 0.3, 3.0]);
        let duct = block([2.0, -1.0, 2.7], [2.4, 2.0, 2.9]);
        let found = run(duct, beam, Test::Hard { tolerance: 0.002 });
        assert_eq!((found.len(), found[0].kind), (1, Kind::Crossing));
        // a pipe cast wholly into a wall: no surface crosses
        let found = run(block([1.0, 0.05, 1.0], [2.0, 0.15, 1.1]), block([0.0, 0.0, 0.0], [5.0, 0.2, 3.0]), Test::Hard { tolerance: 0.002 });
        assert_eq!((found.len(), found[0].kind), (1, Kind::Inside));
    }

    #[test]
    fn elements_nearer_than_the_clearance_are_found_with_their_distance() {
        let (pipe, wall) = (|| block([0.0, 0.03, 0.0], [3.0, 0.08, 0.05]), || block([0.0, -0.2, 0.0], [3.0, 0.0, 3.0]));
        let found = run(pipe(), wall(), Test::Clearance { distance: 0.05 });
        assert_eq!((found.len(), found[0].kind), (1, Kind::Near));
        assert!((found[0].distance - 0.03).abs() < 1e-4, "{:?}", found[0]);
        assert!(run(pipe(), wall(), Test::Clearance { distance: 0.02 }).is_empty());
        // far apart: no hard clash either
        assert!(run(pipe(), wall(), Test::Hard { tolerance: 0.002 }).is_empty());
    }

    #[test]
    fn pairs_left_out_are_not_found() {
        let set = [block([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]), block([0.5, 0.5, 0.5], [1.5, 1.5, 1.5]), block([0.8, 0.8, 0.8], [2.0, 2.0, 2.0])];
        let test = Test::Hard { tolerance: 0.002 };
        assert_eq!(clashes(&set, &set, true, test, &AtomicBool::new(false)).len(), 3);
        // the first two are of one model
        let found = clashes_but(&set, &set, true, test, &|i, j| i.max(j) < 2, &AtomicBool::new(false));
        assert_eq!(found.iter().map(|c| (c.a, c.b)).collect::<Vec<_>>(), [(0, 2), (1, 2)]);
    }

    #[test]
    fn a_set_against_itself_gives_each_pair_once() {
        let set = [block([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]), block([0.5, 0.5, 0.5], [1.5, 1.5, 1.5]), block([5.0, 5.0, 5.0], [6.0, 6.0, 6.0])];
        let found = clashes(&set, &set, true, Test::Hard { tolerance: 0.002 }, &AtomicBool::new(false));
        assert_eq!(found.iter().map(|c| (c.a, c.b)).collect::<Vec<_>>(), [(0, 1)]);
    }
}
