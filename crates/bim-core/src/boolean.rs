//! Booleans of planar solids: union, difference and intersection.
//!
//! The solids' faces are cut into convex polygons and sorted in binary space partitioning trees
//! (the classic polygon CSG of Naylor, Amanatides and Thibault). Every decision on which side of a
//! plane a point lies is made with the exact `orient3d` predicate against three *input* points that
//! span the plane: a polygon split off another keeps its parent's plane, so no decision depends on a
//! constructed point's plane. Where an edge crosses a plane, the crossing point is constructed in
//! f64 and rounded once (ADR-0006: exact predicates over floating-point constructions).
//!
//! The polygons that come out are put back together: points closer than the linear tolerance are
//! one point, a point lying on another polygon's edge splits that edge (no T-junctions), and the
//! result must pass [`Solid::check`] (every edge has exactly one twin, volume positive). When it does
//! not, the operation is refused; it never returns a solid it has not checked.

use crate::predicates::orient3d;
use crate::solid::{Face, Solid};
use crate::{Refusal, LINEAR_TOLERANCE};
use glam::DVec3;
use std::cmp::Ordering;
use std::collections::HashMap;

/// A boolean operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanOp {
    /// What lies in either solid.
    Union,
    /// What lies in the first solid and not in the second.
    Difference,
    /// What lies in both solids.
    Intersection,
}

/// The deepest a partitioning tree may grow before the operation is refused.
const MAX_DEPTH: usize = 20_000;

/// A plane spanned by three input points, oriented by them: its front is the side the normal
/// `(b − a) × (c − a)` points to.
#[derive(Debug, Clone, Copy)]
struct Plane3 {
    a: DVec3,
    b: DVec3,
    c: DVec3,
    /// The (unnormalised) normal and offset, for constructing crossing points only.
    normal: DVec3,
    offset: f64,
}

impl Plane3 {
    fn new(a: DVec3, b: DVec3, c: DVec3) -> Option<Self> {
        let normal = (b - a).cross(c - a);
        (normal.length_squared() > 0.0).then(|| Self { a, b, c, normal, offset: normal.dot(a) })
    }

    /// Which side `p` lies on. Exact, except that a point within the linear tolerance of the plane
    /// lies in it: a corner constructed by an earlier cut was rounded once, and is the same point as
    /// one in the plane when it lies that close (the tolerance that welds points, ADR-0006).
    fn side(&self, p: DVec3) -> Ordering {
        match orient3d(self.a, self.b, self.c, p) {
            Ordering::Equal => Ordering::Equal,
            side if self.distance(p).abs() <= LINEAR_TOLERANCE * self.normal.length() => {
                let _ = side;
                Ordering::Equal
            }
            side => side,
        }
    }

    fn flipped(&self) -> Self {
        Self { a: self.a, b: self.c, c: self.b, normal: -self.normal, offset: -self.offset }
    }

    /// Whether `other` is this very plane (spanned by the same input points): `Some(true)` facing
    /// the same way, `Some(false)` flipped. A polygon split off another keeps its plane, and its
    /// constructed corners lie in it by construction even where rounding moved them off it.
    fn identity(&self, other: &Plane3) -> Option<bool> {
        if self.a != other.a {
            return None;
        }
        if self.b == other.b && self.c == other.c {
            Some(true)
        } else if self.b == other.c && self.c == other.b {
            Some(false)
        } else {
            None
        }
    }

    /// The signed distance of `p`, scaled by the normal's length (for interpolation).
    fn distance(&self, p: DVec3) -> f64 {
        self.normal.dot(p) - self.offset
    }
}

/// A convex polygon and the input plane it lies in.
#[derive(Debug, Clone)]
struct Polygon {
    points: Vec<DVec3>,
    plane: Plane3,
}

impl Polygon {
    fn flip(&mut self) {
        self.points.reverse();
        self.plane = self.plane.flipped();
    }
}

/// Where `splitting` sorts a polygon.
struct Sorted<'a> {
    coplanar_front: &'a mut Vec<Polygon>,
    coplanar_back: &'a mut Vec<Polygon>,
    front: &'a mut Vec<Polygon>,
    back: &'a mut Vec<Polygon>,
}

/// Sorts `polygon` against `plane`: wholly in front, behind, in the plane (facing the same way or
/// not), or split in two.
fn split(plane: &Plane3, polygon: Polygon, to: Sorted<'_>) {
    // a piece of a polygon of this very plane: in it, whatever rounding did to its corners
    match plane.identity(&polygon.plane) {
        Some(true) => return to.coplanar_front.push(polygon),
        Some(false) => return to.coplanar_back.push(polygon),
        None => {}
    }
    let sides: Vec<Ordering> = polygon.points.iter().map(|p| plane.side(*p)).collect();
    let front = sides.contains(&Ordering::Greater);
    let back = sides.contains(&Ordering::Less);
    match (front, back) {
        (false, false) => {
            // in the plane: the normals are parallel, so their dot product is far from zero
            if plane.normal.dot(polygon.plane.normal) > 0.0 {
                to.coplanar_front.push(polygon);
            } else {
                to.coplanar_back.push(polygon);
            }
        }
        (true, false) => to.front.push(polygon),
        (false, true) => to.back.push(polygon),
        (true, true) => {
            let n = polygon.points.len();
            let (mut f, mut b) = (Vec::with_capacity(n + 1), Vec::with_capacity(n + 1));
            for i in 0..n {
                let j = (i + 1) % n;
                let (si, sj) = (sides[i], sides[j]);
                let (pi, pj) = (polygon.points[i], polygon.points[j]);
                if si != Ordering::Less {
                    f.push(pi);
                }
                if si != Ordering::Greater {
                    b.push(pi);
                }
                // the edge crosses the plane: one rounded point on it, shared by both halves
                if (si == Ordering::Greater && sj == Ordering::Less) || (si == Ordering::Less && sj == Ordering::Greater) {
                    let (di, dj) = (plane.distance(pi), plane.distance(pj));
                    let t = if di != dj { (di / (di - dj)).clamp(0.0, 1.0) } else { 0.5 };
                    let crossing = pi + (pj - pi) * t;
                    f.push(crossing);
                    b.push(crossing);
                }
            }
            if f.len() >= 3 {
                to.front.push(Polygon { points: f, plane: polygon.plane });
            }
            if b.len() >= 3 {
                to.back.push(Polygon { points: b, plane: polygon.plane });
            }
        }
    }
}

/// A node of a partitioning tree: its plane, the polygons in it, and what lies in front and behind.
#[derive(Debug, Default)]
struct Node {
    plane: Option<Plane3>,
    polygons: Vec<Polygon>,
    front: Option<Box<Node>>,
    back: Option<Box<Node>>,
}

impl Node {
    fn new(polygons: Vec<Polygon>) -> Result<Self, Refusal> {
        let mut node = Node::default();
        node.build(polygons, 0)?;
        Ok(node)
    }

    /// Turns the solid inside out.
    fn invert(&mut self) {
        for p in &mut self.polygons {
            p.flip();
        }
        self.plane = self.plane.map(|p| p.flipped());
        if let Some(f) = &mut self.front {
            f.invert();
        }
        if let Some(b) = &mut self.back {
            b.invert();
        }
        std::mem::swap(&mut self.front, &mut self.back);
    }

    /// What of `polygons` lies outside this tree's solid.
    fn clip_polygons(&self, polygons: Vec<Polygon>) -> Vec<Polygon> {
        let Some(plane) = &self.plane else { return polygons };
        let (mut front, mut back) = (Vec::new(), Vec::new());
        for p in polygons {
            let (mut cf, mut cb) = (Vec::new(), Vec::new());
            split(plane, p, Sorted { coplanar_front: &mut cf, coplanar_back: &mut cb, front: &mut front, back: &mut back });
            front.append(&mut cf);
            back.append(&mut cb);
        }
        let mut front = match &self.front {
            Some(f) => f.clip_polygons(front),
            None => front,
        };
        let back = match &self.back {
            Some(b) => b.clip_polygons(back),
            None => Vec::new(),
        };
        front.extend(back);
        front
    }

    /// Removes the polygons of this tree that lie inside `other`'s solid.
    fn clip_to(&mut self, other: &Node) {
        self.polygons = other.clip_polygons(std::mem::take(&mut self.polygons));
        if let Some(f) = &mut self.front {
            f.clip_to(other);
        }
        if let Some(b) = &mut self.back {
            b.clip_to(other);
        }
    }

    fn all_polygons(&self, out: &mut Vec<Polygon>) {
        out.extend(self.polygons.iter().cloned());
        if let Some(f) = &self.front {
            f.all_polygons(out);
        }
        if let Some(b) = &self.back {
            b.all_polygons(out);
        }
    }

    fn build(&mut self, polygons: Vec<Polygon>, depth: usize) -> Result<(), Refusal> {
        if polygons.is_empty() {
            return Ok(());
        }
        if depth > MAX_DEPTH {
            return Err(Refusal::BooleanFailed);
        }
        let plane = *self.plane.get_or_insert(polygons[0].plane);
        let (mut front, mut back) = (Vec::new(), Vec::new());
        let mut coplanar = Vec::new();
        for p in polygons {
            let mut cb = Vec::new();
            split(&plane, p, Sorted { coplanar_front: &mut coplanar, coplanar_back: &mut cb, front: &mut front, back: &mut back });
            coplanar.append(&mut cb);
        }
        self.polygons.append(&mut coplanar);
        if !front.is_empty() {
            self.front.get_or_insert_with(Default::default).build(front, depth + 1)?;
        }
        if !back.is_empty() {
            self.back.get_or_insert_with(Default::default).build(back, depth + 1)?;
        }
        Ok(())
    }
}

/// The convex polygons of a solid's faces. A convex face goes in whole; any other is cut into
/// triangles, and a triangle thinner than the linear tolerance is left out (it covers nothing).
/// Every polygon of a face takes the face's plane, spanned by three of its corners far apart, so
/// the pieces of one face are one plane to the partitioning.
fn polygons(solid: &Solid) -> Result<Vec<Polygon>, Refusal> {
    let mut out = Vec::new();
    for face in &solid.faces {
        let rings: Vec<Vec<DVec3>> = face.rings.iter().map(|r| r.iter().map(|&i| solid.positions[i as usize]).collect()).collect();
        let Some(plane) = face_plane(&rings[0]) else { continue };
        if rings.len() == 1 && convex(&rings[0], plane.normal) {
            out.push(Polygon { points: rings[0].clone(), plane });
            continue;
        }
        for [a, b, c] in crate::mesh::triangles(&rings, plane.normal)? {
            let longest = (b - a).length().max((c - b).length()).max((a - c).length());
            if (b - a).cross(c - a).length() > LINEAR_TOLERANCE * longest {
                out.push(Polygon { points: vec![a, b, c], plane });
            }
        }
    }
    Ok(out)
}

/// The plane of a face's outer ring, spanned by three of its corners far apart and facing the way
/// the ring runs.
fn face_plane(ring: &[DVec3]) -> Option<Plane3> {
    let a = ring[0];
    let b = *ring.iter().max_by(|p, q| p.distance_squared(a).total_cmp(&q.distance_squared(a)))?;
    let c = *ring.iter().max_by(|p, q| (b - a).cross(**p - a).length_squared().total_cmp(&(b - a).cross(**q - a).length_squared()))?;
    let plane = Plane3::new(a, b, c)?;
    let newell = ring.iter().zip(ring.iter().cycle().skip(1)).fold(DVec3::ZERO, |n, (p, q)| n + p.cross(*q));
    Some(if plane.normal.dot(newell) < 0.0 { plane.flipped() } else { plane })
}

/// Whether a planar ring is convex (points on a straight line between their neighbours allowed).
fn convex(ring: &[DVec3], normal: DVec3) -> bool {
    let n = ring.len();
    let scale = normal.length();
    (0..n).all(|i| {
        let (p, q, r) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
        (q - p).cross(r - q).dot(normal) >= -LINEAR_TOLERANCE * scale * (q - p).length().max((r - q).length())
    })
}

/// `a` combined with `b` by `op`. Both must be closed; the result is checked, and refused when it
/// would be empty ([`Refusal::Empty`]) or could not be put back together closed
/// ([`Refusal::BooleanFailed`]).
pub fn boolean(a: &Solid, b: &Solid, op: BooleanOp) -> Result<Solid, Refusal> {
    a.check()?;
    b.check()?;
    let mut na = Node::new(polygons(a)?)?;
    let mut nb = Node::new(polygons(b)?)?;
    let mut rest = Vec::new();
    match op {
        BooleanOp::Union => {
            na.clip_to(&nb);
            nb.clip_to(&na);
            nb.invert();
            nb.clip_to(&na);
            nb.invert();
            nb.all_polygons(&mut rest);
            na.build(rest, 0)?;
        }
        BooleanOp::Difference => {
            na.invert();
            na.clip_to(&nb);
            nb.clip_to(&na);
            nb.invert();
            nb.clip_to(&na);
            nb.invert();
            nb.all_polygons(&mut rest);
            na.build(rest, 0)?;
            na.invert();
        }
        BooleanOp::Intersection => {
            na.invert();
            nb.clip_to(&na);
            nb.invert();
            na.clip_to(&nb);
            nb.clip_to(&na);
            nb.all_polygons(&mut rest);
            na.build(rest, 0)?;
            na.invert();
        }
    }
    let mut out = Vec::new();
    na.all_polygons(&mut out);
    assemble(&out)
}

/// The solid of `polygons`: their points welded within the linear tolerance, every edge split at the
/// points lying on it, and the whole checked.
fn assemble(polygons: &[Polygon]) -> Result<Solid, Refusal> {
    if polygons.is_empty() {
        return Err(Refusal::Empty);
    }
    let mut welder = Welder::default();
    let mut rings: Vec<Vec<u32>> = Vec::with_capacity(polygons.len());
    for p in polygons {
        let mut ring: Vec<u32> = Vec::with_capacity(p.points.len());
        for point in &p.points {
            let i = welder.index(*point);
            if ring.last() != Some(&i) {
                ring.push(i);
            }
        }
        while ring.len() > 1 && ring.first() == ring.last() {
            ring.pop();
        }
        if ring.len() >= 3 {
            rings.push(ring);
        }
    }
    let positions = welder.points;
    // T-junctions: a point on an edge of another polygon splits that edge, so every edge meets its
    // twin end to end
    let tol2 = LINEAR_TOLERANCE * LINEAR_TOLERANCE;
    let mut fixed: Vec<Vec<u32>> = Vec::with_capacity(rings.len());
    for ring in &rings {
        let mut out = Vec::with_capacity(ring.len());
        for k in 0..ring.len() {
            let (i, j) = (ring[k], ring[(k + 1) % ring.len()]);
            out.push(i);
            let (a, b) = (positions[i as usize], positions[j as usize]);
            let ab = b - a;
            let len2 = ab.length_squared();
            if len2 <= tol2 {
                continue;
            }
            let (lo, hi) = (a.min(b) - DVec3::splat(LINEAR_TOLERANCE), a.max(b) + DVec3::splat(LINEAR_TOLERANCE));
            let mut on: Vec<(f64, u32)> = positions
                .iter()
                .enumerate()
                .filter(|&(m, p)| m as u32 != i && m as u32 != j && p.cmpge(lo).all() && p.cmple(hi).all())
                .filter_map(|(m, p)| {
                    let t = (*p - a).dot(ab) / len2;
                    let off = a + ab * t - *p;
                    (t > 0.0 && t < 1.0 && off.length_squared() <= tol2).then_some((t, m as u32))
                })
                .collect();
            on.sort_by(|x, y| x.0.total_cmp(&y.0));
            out.extend(on.into_iter().map(|(_, m)| m));
        }
        fixed.push(out);
    }
    let solid = Solid { positions, faces: fixed.into_iter().map(|r| Face { rings: vec![r] }).collect() };
    #[cfg(test)]
    if solid.check().is_err() {
        report_open_edges(&solid);
    }
    solid.check().map_err(|_| Refusal::BooleanFailed)?;
    Ok(solid)
}

#[cfg(test)]
fn report_open_edges(solid: &Solid) {
    let mut edges: HashMap<(u32, u32), i32> = HashMap::new();
    for f in &solid.faces {
        for r in &f.rings {
            for k in 0..r.len() {
                *edges.entry((r[k], r[(k + 1) % r.len()])).or_default() += 1;
            }
        }
    }
    let bad: Vec<_> = edges.iter().filter(|(&(a, b), &n)| n != 1 || edges.get(&(b, a)) != Some(&1)).collect();
    eprintln!("{} faces, {} points, {} open edges, volume {}", solid.faces.len(), solid.positions.len(), bad.len(), solid.volume());
    for ((a, b), n) in bad.iter().take(16) {
        eprintln!("  {a} -> {b} x{n}  {:?} {:?}", solid.positions[*a as usize], solid.positions[*b as usize]);
    }
}

/// Gives points within the linear tolerance of each other one index.
#[derive(Default)]
struct Welder {
    points: Vec<DVec3>,
    cells: HashMap<[i64; 3], Vec<u32>>,
}

impl Welder {
    fn cell(p: DVec3) -> [i64; 3] {
        let size = 4.0 * LINEAR_TOLERANCE;
        [(p.x / size).floor() as i64, (p.y / size).floor() as i64, (p.z / size).floor() as i64]
    }

    fn index(&mut self, p: DVec3) -> u32 {
        let c = Self::cell(p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(found) = self.cells.get(&[c[0] + dx, c[1] + dy, c[2] + dz]) {
                        if let Some(&i) = found.iter().find(|&&i| self.points[i as usize].distance(p) <= LINEAR_TOLERANCE) {
                            return i;
                        }
                    }
                }
            }
        }
        let i = self.points.len() as u32;
        self.points.push(p);
        self.cells.entry(c).or_default().push(i);
        i
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Profile;
    use crate::solid::Axes;

    /// The box from `lo` to `hi`.
    fn cuboid(lo: DVec3, hi: DVec3) -> Solid {
        let size = hi - lo;
        let axes = Axes::new(DVec3::new((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, lo.z), DVec3::X, DVec3::Z).unwrap();
        Solid::extrude(&Profile::rectangle(size.x, size.y).unwrap(), &axes, size.z).unwrap()
    }

    /// A box `size` centred on `centre`, turned `angle` radians about the vertical.
    fn turned(centre: DVec3, size: DVec3, angle: f64) -> Solid {
        let (s, c) = angle.sin_cos();
        let axes = Axes::new(centre - DVec3::Z * size.z / 2.0, DVec3::new(c, s, 0.0), DVec3::Z).unwrap();
        Solid::extrude(&Profile::rectangle(size.x, size.y).unwrap(), &axes, size.z).unwrap()
    }

    fn v(x: f64, y: f64, z: f64) -> DVec3 {
        DVec3::new(x, y, z)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9 * a.abs().max(b.abs()).max(1.0)
    }

    #[test]
    fn overlapping_boxes_give_the_volumes_they_should() {
        let a = cuboid(v(0.0, 0.0, 0.0), v(2.0, 2.0, 2.0));
        let b = cuboid(v(1.0, 1.0, 1.0), v(3.0, 3.0, 3.0));
        assert!(close(boolean(&a, &b, BooleanOp::Union).unwrap().volume(), 15.0));
        assert!(close(boolean(&a, &b, BooleanOp::Difference).unwrap().volume(), 7.0));
        assert!(close(boolean(&a, &b, BooleanOp::Intersection).unwrap().volume(), 1.0));
    }

    #[test]
    fn a_hole_right_through_a_wall() {
        let wall = cuboid(v(0.0, 0.0, 0.0), v(5.0, 0.2, 3.0));
        let cutter = cuboid(v(1.0, -0.1, 1.0), v(2.0, 0.3, 2.0));
        let s = boolean(&wall, &cutter, BooleanOp::Difference).unwrap();
        assert!(close(s.volume(), 3.0 - 0.2), "{}", s.volume());
    }

    #[test]
    fn a_cutter_flush_with_both_faces_of_the_wall() {
        // the hard case: the cutter's faces lie exactly in the wall's
        let wall = cuboid(v(0.0, 0.0, 0.0), v(5.0, 0.2, 3.0));
        let cutter = cuboid(v(1.0, 0.0, 1.0), v(2.0, 0.2, 2.0));
        let s = boolean(&wall, &cutter, BooleanOp::Difference).unwrap();
        assert!(close(s.volume(), 3.0 - 0.2), "{}", s.volume());
    }

    #[test]
    fn boxes_touching_on_a_face_unite_into_one() {
        let a = cuboid(v(0.0, 0.0, 0.0), v(1.0, 1.0, 1.0));
        let b = cuboid(v(1.0, 0.0, 0.0), v(2.0, 1.0, 1.0));
        let s = boolean(&a, &b, BooleanOp::Union).unwrap();
        assert!(close(s.volume(), 2.0), "{}", s.volume());
        // a box sharing only part of a face, offset in two directions
        let c = cuboid(v(1.0, 0.5, 0.25), v(2.0, 1.5, 0.75));
        assert!(close(boolean(&a, &c, BooleanOp::Union).unwrap().volume(), 1.5));
    }

    #[test]
    fn a_cutter_that_misses_leaves_the_solid_whole() {
        let a = cuboid(v(0.0, 0.0, 0.0), v(1.0, 1.0, 1.0));
        let far = cuboid(v(5.0, 5.0, 5.0), v(6.0, 6.0, 6.0));
        assert!(close(boolean(&a, &far, BooleanOp::Difference).unwrap().volume(), 1.0));
        assert!(close(boolean(&a, &far, BooleanOp::Union).unwrap().volume(), 2.0), "two lumps");
        assert_eq!(boolean(&a, &far, BooleanOp::Intersection), Err(Refusal::Empty));
    }

    #[test]
    fn a_cutter_that_takes_everything_is_refused() {
        let a = cuboid(v(0.0, 0.0, 0.0), v(1.0, 1.0, 1.0));
        let big = cuboid(v(-1.0, -1.0, -1.0), v(2.0, 2.0, 2.0));
        assert_eq!(boolean(&a, &big, BooleanOp::Difference), Err(Refusal::Empty));
    }

    #[test]
    fn a_wall_with_twelve_openings() {
        // Open BIM Studio's torture case: twelve openings, cut one after another
        let mut wall = cuboid(v(0.0, 0.0, 0.0), v(12.4, 0.3, 3.0));
        for i in 0..12 {
            let x = 0.4 + i as f64;
            let cutter = cuboid(v(x, -0.05, 0.9), v(x + 0.6, 0.35, 2.1));
            wall = boolean(&wall, &cutter, BooleanOp::Difference).unwrap_or_else(|e| panic!("opening {i}: {e}"));
        }
        let expected = 12.4 * 0.3 * 3.0 - 12.0 * 0.6 * 1.2 * 0.3;
        assert!(close(wall.volume(), expected), "{} vs {expected}", wall.volume());
    }

    #[test]
    fn turned_boxes_keep_the_volume_identities() {
        // whatever the angle: |A − B| + |A ∩ B| = |A| and |A ∪ B| = |A| + |B| − |A ∩ B|
        let a = cuboid(v(0.0, 0.0, 0.0), v(4.0, 0.3, 3.0));
        for (k, angle) in [0.1f64, 0.5, std::f64::consts::FRAC_PI_4, 1.2, 2.9].into_iter().enumerate() {
            let b = turned(v(2.0 + 0.1 * k as f64, 0.15, 1.5), v(1.0, 0.8, 1.2), angle);
            let diff = boolean(&a, &b, BooleanOp::Difference).unwrap_or_else(|e| panic!("{angle}: {e}")).volume();
            let both = boolean(&a, &b, BooleanOp::Intersection).unwrap_or_else(|e| panic!("{angle}: {e}")).volume();
            let union = boolean(&a, &b, BooleanOp::Union).unwrap_or_else(|e| panic!("{angle}: {e}")).volume();
            assert!(close(diff + both, a.volume()), "{angle}: {diff} + {both} vs {}", a.volume());
            assert!(close(union, a.volume() + b.volume() - both), "{angle}: {union}");
        }
    }

    #[test]
    fn a_beam_through_a_column_corner() {
        // a cutter whose edge runs through the corner of the other: crossings at shared points
        let column = cuboid(v(0.0, 0.0, 0.0), v(0.3, 0.3, 3.0));
        let beam = turned(v(0.3, 0.3, 2.5), v(2.0, 0.2, 0.4), std::f64::consts::FRAC_PI_4);
        let diff = boolean(&column, &beam, BooleanOp::Difference).unwrap().volume();
        let both = boolean(&column, &beam, BooleanOp::Intersection).unwrap().volume();
        assert!(close(diff + both, column.volume()), "{diff} + {both}");
    }

    #[test]
    fn a_second_cutter_overlapping_the_first_hole() {
        let wall = cuboid(v(0.0, 0.0, 0.0), v(5.0, 0.2, 3.0));
        let first = cuboid(v(1.0, -0.1, 1.0), v(2.0, 0.3, 2.0));
        let second = cuboid(v(1.5, -0.1, 1.5), v(2.5, 0.3, 2.5));
        let once = boolean(&wall, &first, BooleanOp::Difference).unwrap();
        let twice = boolean(&once, &second, BooleanOp::Difference).unwrap();
        // the two holes overlap in a 0.5 × 0.5 square
        assert!(close(twice.volume(), 3.0 - (1.0 + 1.0 - 0.25) * 0.2), "{}", twice.volume());
    }

    #[test]
    fn a_face_within_the_tolerance_counts_as_flush() {
        // the cutter's faces lie 1e-7 m inside the wall's: the same plane, so a clean hole
        let wall = cuboid(v(0.0, 0.0, 0.0), v(5.0, 0.2, 3.0));
        let cutter = cuboid(v(1.0, 1e-7, 1.0), v(2.0, 0.2 - 1e-7, 2.0));
        let s = boolean(&wall, &cutter, BooleanOp::Difference).unwrap();
        assert!((s.volume() - 2.8).abs() < 1e-6, "{}", s.volume());
    }

    #[test]
    fn a_skin_just_thicker_than_the_tolerance_is_kept_or_refused_never_wrong() {
        // 3 µm of wall left on one side: either it is kept, closed and with the right volume, or
        // the cut is refused
        let wall = cuboid(v(0.0, 0.0, 0.0), v(5.0, 0.2, 3.0));
        let cutter = cuboid(v(1.0, 3e-6, 1.0), v(2.0, 0.3, 2.0));
        match boolean(&wall, &cutter, BooleanOp::Difference) {
            Ok(s) => assert!((s.volume() - (3.0 - (0.2 - 3e-6))).abs() < 1e-6, "{}", s.volume()),
            Err(e) => assert_eq!(e, Refusal::BooleanFailed),
        }
    }

    #[test]
    fn far_out_in_a_tile_frame_the_result_is_the_same() {
        // the kernel only sees tile-local coordinates; this checks the operation does not depend on
        // where the frame is (all coordinates within 1 km of the tile origin)
        let shift = v(999.0, 999.0, 0.0);
        let a = cuboid(v(0.0, 0.0, 0.0), v(2.0, 2.0, 2.0));
        let b = cuboid(v(1.0, 1.0, 1.0), v(3.0, 3.0, 3.0));
        let near = boolean(&a, &b, BooleanOp::Difference).unwrap().volume();
        let far = boolean(&a.translated(shift), &b.translated(shift), BooleanOp::Difference).unwrap().volume();
        assert!((near - far).abs() < 1e-9, "{near} vs {far}");
    }
}
