//! Geometry for the measure tools.
//!
//! Per layer (cached, model space): welded vertices, triangle neighbours and the feature lines —
//! the real edges of the model (creases sharper than 25° and open borders, not the diagonals of
//! the triangulation), with collinear pieces merged so each line runs corner to corner.
//!
//! On top of that, in world space: faces (flat or smoothly curved), volumes, perpendicular
//! distances, angles and polygon areas.

use crate::model::Layer;
use glam::{Mat4, Vec3};
use std::collections::HashMap;

/// Crease angle above which a triangle edge is a model edge (also the bend a curved face may make
/// between neighbouring triangles).
const FEATURE_DEG: f32 = 25.0;
/// Pieces within this angle are merged into one line.
const COLLINEAR_DEG: f32 = 0.5;
/// Vertices closer than 0.1 mm are welded.
const WELD: f32 = 1e4;
/// Faces with more triangles than this are cut off (keeps the UI responsive).
const MAX_REGION_TRIANGLES: usize = 500_000;

pub struct LayerTopology {
    /// Neighbour triangle across edge (corner i → i+1) per index, -1 at an open border.
    pub neighbors: Vec<i32>,
    /// Feature lines (model space).
    pub lines: Vec<(Vec3, Vec3)>,
}

fn weld_key(p: Vec3) -> (i64, i64, i64) {
    ((p.x * WELD).round() as i64, (p.y * WELD).round() as i64, (p.z * WELD).round() as i64)
}

fn vertex(layer: &Layer, i: u32) -> Vec3 {
    let i = 3 * i as usize;
    Vec3::new(layer.positions[i], layer.positions[i + 1], layer.positions[i + 2])
}

/// The three corners of triangle `t` (model space).
pub fn triangle(layer: &Layer, t: usize) -> [Vec3; 3] {
    let tri = &layer.indices[3 * t..3 * t + 3];
    [vertex(layer, tri[0]), vertex(layer, tri[1]), vertex(layer, tri[2])]
}

fn normal_of(v: &[Vec3; 3]) -> Vec3 {
    (v[1] - v[0]).cross(v[2] - v[0])
}

impl LayerTopology {
    pub fn build(layer: &Layer) -> Self {
        let n = layer.indices.len() / 3 * 3;
        let valid = |i: u32| 3 * (i as usize) + 2 < layer.positions.len();
        let mut ids: HashMap<(i64, i64, i64), u32> = HashMap::new();
        let mut welded: Vec<Vec3> = Vec::new();
        let mut corners = vec![0u32; n];
        for (c, &i) in layer.indices[..n].iter().enumerate() {
            let p = if valid(i) { vertex(layer, i) } else { Vec3::ZERO };
            let next = welded.len() as u32;
            let id = *ids.entry(weld_key(p)).or_insert_with(|| {
                welded.push(p);
                next
            });
            corners[c] = id;
        }

        // pair the triangles over shared (undirected) edges
        let mut owner: HashMap<(u32, u32), (usize, u32)> = HashMap::new();
        let mut neighbors = vec![-1i32; n];
        for t in 0..n / 3 {
            for e in 0..3 {
                let (a, b) = (corners[3 * t + e], corners[3 * t + (e + 1) % 3]);
                if a == b {
                    continue;
                }
                let key = (a.min(b), a.max(b));
                let entry = owner.entry(key).or_insert((3 * t + e, 0));
                entry.1 += 1;
                if entry.1 == 2 {
                    let first = entry.0;
                    neighbors[first] = t as i32;
                    neighbors[3 * t + e] = (first / 3) as i32;
                }
            }
        }

        // feature edges: borders, non-manifold edges and creases
        let cos_feature = FEATURE_DEG.to_radians().cos();
        let tri_normal = |t: usize| normal_of(&triangle(layer, t)).normalize_or_zero();
        let mut segments: Vec<(u32, u32)> = Vec::new();
        for (&(a, b), &(first, uses)) in &owner {
            let t = first / 3;
            let feature = if uses != 2 {
                true
            } else {
                let other = neighbors[first];
                other < 0 || tri_normal(t).dot(tri_normal(other as usize)) < cos_feature
            };
            if feature {
                segments.push((a, b));
            }
        }
        let lines = merge_collinear(&welded, &segments);
        Self { neighbors, lines }
    }
}

/// Merges chains of collinear segments into single lines. A chain also runs through
/// T-junctions, so a long edge stays one line where another edge touches it halfway.
fn merge_collinear(vertices: &[Vec3], segments: &[(u32, u32)]) -> Vec<(Vec3, Vec3)> {
    let mut adjacency: HashMap<u32, Vec<usize>> = HashMap::new();
    for (s, &(a, b)) in segments.iter().enumerate() {
        adjacency.entry(a).or_default().push(s);
        adjacency.entry(b).or_default().push(s);
    }
    let cos = COLLINEAR_DEG.to_radians().cos();
    let dir = |s: usize| {
        let (a, b) = segments[s];
        (vertices[b as usize] - vertices[a as usize]).normalize_or_zero()
    };
    let mut visited = vec![false; segments.len()];
    let mut out = Vec::new();
    for s in 0..segments.len() {
        if visited[s] {
            continue;
        }
        visited[s] = true;
        let d = dir(s);
        let mut extend = |mut v: u32| loop {
            let next = adjacency[&v].iter().copied().find(|&t| !visited[t] && dir(t).dot(d).abs() > cos);
            let Some(t) = next else { return v };
            visited[t] = true;
            let (a, b) = segments[t];
            v = if a == v { b } else { a };
        };
        let (a, b) = segments[s];
        let (a, b) = (extend(a), extend(b));
        out.push((vertices[a as usize], vertices[b as usize]));
    }
    out
}

// ---- faces ------------------------------------------------------------------------------------

/// A face picked for its area (world space).
#[derive(Debug, Clone)]
pub struct FaceRegion {
    /// Triangle indices of the face within its layer.
    pub faces: Vec<usize>,
    /// World triangles, for the highlight.
    pub triangles: Vec<[Vec3; 3]>,
    /// Outline segments (world).
    pub outline: Vec<(Vec3, Vec3)>,
    pub area: f64,
    pub perimeter: f64,
    /// Where the label goes: the area-weighted centre, or for a curved face the picked triangle.
    pub anchor: Vec3,
    /// A curved surface (the side of a cylinder, a dome).
    pub curved: bool,
}

/// Grows the face around triangle `start` up to the model edges: connected triangles that bend
/// less than 25° from their neighbour. Flat faces stay flat; a tessellated curved surface is
/// measured as a whole instead of one facet. `allowed` limits the search (the element's triangles).
pub fn face_region(layer: &Layer, topo: &LayerTopology, start: usize, allowed: std::ops::Range<usize>, mm: &Mat4) -> FaceRegion {
    grow_face(layer, topo, start, allowed, mm, false)
}

/// The flat face around triangle `start`: connected triangles within 1° of its plane. On a curved
/// surface that is the facet (the plane a distance or angle is taken to).
pub fn flat_face(layer: &Layer, topo: &LayerTopology, start: usize, allowed: std::ops::Range<usize>, mm: &Mat4) -> FaceRegion {
    grow_face(layer, topo, start, allowed, mm, true)
}

fn grow_face(layer: &Layer, topo: &LayerTopology, start: usize, allowed: std::ops::Range<usize>, mm: &Mat4, flat: bool) -> FaceRegion {
    let cos_smooth = FEATURE_DEG.to_radians().cos();
    let cos_flat = 1f32.to_radians().cos();
    let world = |t: usize| triangle(layer, t).map(|p| mm.transform_point3(p));
    let start_normal = normal_of(&world(start)).normalize_or_zero();
    let mut in_region = HashMap::new();
    in_region.insert(start, ());
    let mut queue = vec![start];
    let mut region = FaceRegion {
        faces: Vec::new(),
        triangles: Vec::new(),
        outline: Vec::new(),
        area: 0.0,
        perimeter: 0.0,
        anchor: Vec3::ZERO,
        curved: false,
    };
    let mut weighted = glam::DVec3::ZERO;

    while let Some(t) = queue.pop() {
        if region.faces.len() >= MAX_REGION_TRIANGLES {
            break;
        }
        region.faces.push(t);
        let v = world(t);
        let n = normal_of(&v);
        let a = n.length() as f64 / 2.0;
        let n = n.normalize_or_zero();
        region.area += a;
        weighted += (v[0] + v[1] + v[2]).as_dvec3() / 3.0 * a;
        if a > 0.0 && n.dot(start_normal) < cos_flat {
            region.curved = true;
        }
        region.triangles.push(v);

        for e in 0..3 {
            let nb = topo.neighbors[3 * t + e];
            let mut border = nb < 0 || !allowed.contains(&(nb as usize));
            if !border && !in_region.contains_key(&(nb as usize)) {
                let nn = normal_of(&world(nb as usize));
                let len = nn.length();
                // degenerate slivers are taken along; otherwise the bend must be smooth (or none)
                let joins = if flat { nn.dot(start_normal) / len > cos_flat } else { nn.dot(n) / len > cos_smooth };
                if len == 0.0 || a == 0.0 || joins {
                    in_region.insert(nb as usize, ());
                    queue.push(nb as usize);
                } else {
                    border = true;
                }
            }
            if border {
                let (p, q) = (v[e], v[(e + 1) % 3]);
                region.outline.push((p, q));
                region.perimeter += p.distance(q) as f64;
            }
        }
    }
    region.anchor = if region.curved || region.area == 0.0 {
        let s = world(start);
        (s[0] + s[1] + s[2]) / 3.0
    } else {
        (weighted / region.area).as_vec3()
    };
    region
}

// ---- volume -----------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct VolumeResult {
    pub volume: f64,
    pub surface_area: f64,
    /// Every edge shared by exactly two triangles: the volume is exact only then.
    pub closed: bool,
    /// Centre of the world bounding box (label anchor).
    pub center: Vec3,
}

/// Volume (signed tetrahedra, per part) and total surface area of index ranges of layers.
pub fn volume(parts: &[(&Layer, std::ops::Range<usize>)], mm: &Mat4) -> VolumeResult {
    let mut volume = 0.0;
    let mut surface_area = 0.0;
    let mut closed = !parts.is_empty();
    let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for (layer, range) in parts {
        let mut part = 0.0f64;
        let mut edges: HashMap<((i64, i64, i64), (i64, i64, i64)), u32> = HashMap::new();
        for t in range.clone() {
            let v = triangle(layer, t).map(|p| mm.transform_point3(p));
            let (a, b, c) = (v[0].as_dvec3(), v[1].as_dvec3(), v[2].as_dvec3());
            part += a.dot(b.cross(c)) / 6.0;
            surface_area += (b - a).cross(c - a).length() / 2.0;
            for p in v {
                lo = lo.min(p);
                hi = hi.max(p);
            }
            for e in 0..3 {
                let (p, q) = (weld_key(v[e]), weld_key(v[(e + 1) % 3]));
                if p != q {
                    *edges.entry((p.min(q), p.max(q))).or_insert(0) += 1;
                }
            }
        }
        // each part may be wound either way
        volume += part.abs();
        if edges.is_empty() || edges.values().any(|&n| n != 2) {
            closed = false;
        }
    }
    let center = if lo.x.is_finite() { (lo + hi) / 2.0 } else { Vec3::ZERO };
    VolumeResult { volume, surface_area, closed, center }
}

// ---- distances --------------------------------------------------------------------------------

/// What a measurement starts or ends on (world space).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Entity {
    Point(Vec3),
    /// An edge from a to b, picked at p.
    Line { a: Vec3, b: Vec3, p: Vec3 },
    /// A flat face through p with unit normal n.
    Plane { n: Vec3, p: Vec3 },
}

impl Entity {
    pub fn pick(&self) -> Vec3 {
        match *self {
            Entity::Point(p) | Entity::Line { p, .. } | Entity::Plane { p, .. } => p,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Distance {
    pub p1: Vec3,
    pub p2: Vec3,
    /// Helper lines where a foot point lies on the extension of an edge.
    pub extensions: Vec<(Vec3, Vec3)>,
}

fn foot_on_line(p: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let d = b - a;
    let len2 = d.length_squared();
    if len2 < 1e-12 {
        return a;
    }
    a + d * ((p - a).dot(d) / len2)
}

fn foot_on_plane(p: Vec3, n: Vec3, origin: Vec3) -> Vec3 {
    p - n * (p - origin).dot(n)
}

/// Closest points between two infinite lines, or None when they are parallel.
fn closest_between_lines(a1: Vec3, b1: Vec3, a2: Vec3, b2: Vec3) -> Option<(Vec3, Vec3)> {
    let (d1, d2, r) = ((b1 - a1).as_dvec3(), (b2 - a2).as_dvec3(), (a1 - a2).as_dvec3());
    let (a, e, b) = (d1.dot(d1), d2.dot(d2), d1.dot(d2));
    let (c, f) = (d1.dot(r), d2.dot(r));
    let denom = a * e - b * b;
    if a < 1e-12 || e < 1e-12 || denom < 1e-9 * a * e {
        return None;
    }
    let s = (b * f - c * e) / denom;
    let t = (a * f - b * c) / denom;
    Some(((a1.as_dvec3() + d1 * s).as_vec3(), (a2.as_dvec3() + d2 * t).as_vec3()))
}

/// The helper line from the nearest end of an edge to a point on its extension.
fn extension(a: Vec3, b: Vec3, p: Vec3) -> Option<(Vec3, Vec3)> {
    let d = b - a;
    let len2 = d.length_squared();
    if len2 < 1e-12 {
        return None;
    }
    let t = (p - a).dot(d) / len2;
    if t < -1e-5 {
        Some((a, p))
    } else if t > 1.0 + 1e-5 {
        Some((b, p))
    } else {
        None
    }
}

/// Perpendicular distance between two entities (as Trimble Connect): to or from a face it is
/// perpendicular to that face (the first face wins), to or from an edge perpendicular to the
/// edge, edge to edge along their common perpendicular, point to point straight.
pub fn perpendicular(a: &Entity, b: &Entity) -> Distance {
    let (p1, p2) = match (*a, *b) {
        (Entity::Plane { n, p }, other) => (foot_on_plane(other.pick(), n, p), other.pick()),
        (other, Entity::Plane { n, p }) => (other.pick(), foot_on_plane(other.pick(), n, p)),
        (Entity::Line { a: a1, b: b1, .. }, Entity::Line { a: a2, b: b2, p }) => {
            closest_between_lines(a1, b1, a2, b2).unwrap_or((foot_on_line(p, a1, b1), p))
        }
        (Entity::Line { a: la, b: lb, .. }, other) => (foot_on_line(other.pick(), la, lb), other.pick()),
        (other, Entity::Line { a: la, b: lb, .. }) => (other.pick(), foot_on_line(other.pick(), la, lb)),
        (x, y) => (x.pick(), y.pick()),
    };
    let mut extensions = Vec::new();
    if let Entity::Line { a: la, b: lb, .. } = *a {
        extensions.extend(extension(la, lb, p1));
    }
    if let Entity::Line { a: la, b: lb, .. } = *b {
        extensions.extend(extension(la, lb, p2));
    }
    Distance { p1, p2, extensions }
}

// ---- angles -----------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Angle {
    /// Radians, 0..π.
    pub angle: f32,
    /// Apex.
    pub center: Vec3,
    /// Unit directions of the legs, from the first entity (v1) to the second (v2).
    pub v1: Vec3,
    pub v2: Vec3,
    /// Leg lengths (to where the entities were picked).
    pub len1: f32,
    pub len2: f32,
}

fn any_perpendicular(d: Vec3) -> Vec3 {
    let r = if d.z.abs() < 0.9 { Vec3::Z } else { Vec3::X };
    d.cross(r).normalize_or_zero()
}

/// Direction along an edge from `from` towards where it was picked (or its far end when picked
/// right at `from`), and the leg length.
fn edge_leg(a: Vec3, b: Vec3, p: Vec3, from: Vec3) -> (Vec3, f32) {
    let to_pick = p - from;
    if to_pick.length() > 1e-6 * (1.0 + a.distance(b)) {
        let mut d = (b - a).normalize_or_zero();
        if to_pick.dot(d) < 0.0 {
            d = -d;
        }
        return (d, to_pick.length());
    }
    let far = if from.distance(a) > from.distance(b) { a } else { b };
    ((far - from).normalize_or_zero(), far.distance(from))
}

/// Leg in a plane, perpendicular to `axis`, towards the pick.
fn plane_leg(n: Vec3, p: Vec3, from: Vec3, axis: Vec3) -> (Vec3, f32) {
    let mut to_pick = p - from;
    to_pick -= axis * to_pick.dot(axis);
    let len = to_pick.length();
    if len > 1e-6 {
        (to_pick / len, len)
    } else {
        (n.cross(axis).normalize_or_zero(), 0.0)
    }
}

fn angle_of(center: Vec3, v1: Vec3, len1: f32, v2: Vec3, len2: f32) -> Angle {
    Angle { angle: v1.angle_between(v2), center, v1, v2, len1, len2 }
}

/// The angle between two edges and/or faces, from A to B: face–face at their intersection line,
/// edge–edge at their (closest) crossing, edge–face between the edge and the face. None for
/// points or parallel entities.
pub fn angle_between(a: &Entity, b: &Entity) -> Option<Angle> {
    match (*a, *b) {
        (Entity::Point(_), _) | (_, Entity::Point(_)) => None,
        (Entity::Plane { n: n1, p: p1 }, Entity::Plane { n: n2, p: p2 }) => {
            let axis = n1.cross(n2);
            if axis.length() < 1e-6 {
                return None;
            }
            let axis = axis.normalize();
            let c = n1.dot(n2);
            let (h1, h2) = (n1.dot(p1), n2.dot(p2));
            let origin = n1 * ((h1 - h2 * c) / (1.0 - c * c)) + n2 * ((h2 - h1 * c) / (1.0 - c * c));
            let mid = (p1 + p2) / 2.0;
            let center = origin + axis * (mid - origin).dot(axis);
            let (v1, len1) = plane_leg(n1, p1, center, axis);
            let (v2, len2) = plane_leg(n2, p2, center, axis);
            Some(angle_of(center, v1, len1, v2, len2))
        }
        (Entity::Line { a: a1, b: b1, p: q1 }, Entity::Line { a: a2, b: b2, p: q2 }) => {
            let (c1, c2) = closest_between_lines(a1, b1, a2, b2)?;
            let (v1, len1) = edge_leg(a1, b1, q1, c1);
            let (v2, len2) = edge_leg(a2, b2, q2, c2);
            Some(angle_of((c1 + c2) / 2.0, v1, len1, v2, len2))
        }
        (Entity::Line { a: la, b: lb, p: lp }, Entity::Plane { n, p })
        | (Entity::Plane { n, p }, Entity::Line { a: la, b: lb, p: lp }) => {
            let d = (lb - la).normalize_or_zero();
            let denom = n.dot(d);
            if denom.abs() < 1e-6 {
                return None;
            }
            let center = la + d * (n.dot(p - la) / denom);
            let (edge_dir, edge_len) = edge_leg(la, lb, lp, center);
            // the face leg: the edge projected onto the face (or towards the pick when the edge
            // stands perpendicular on it)
            let mut face_dir = edge_dir - n * edge_dir.dot(n);
            if face_dir.length() < 1e-6 {
                let to_pick = p - center;
                face_dir = to_pick - n * to_pick.dot(n);
                if face_dir.length() < 1e-6 {
                    face_dir = any_perpendicular(n);
                }
            }
            let face_dir = face_dir.normalize();
            let face_len = p.distance(center);
            Some(if matches!(*a, Entity::Line { .. }) {
                angle_of(center, edge_dir, edge_len, face_dir, face_len)
            } else {
                angle_of(center, face_dir, face_len, edge_dir, edge_len)
            })
        }
    }
}

/// 0° between parallel entities: legs along the line between the picks.
pub fn parallel_angle(a: Vec3, b: Vec3) -> Angle {
    let d = (b - a).try_normalize().unwrap_or(Vec3::X);
    let len = a.distance(b);
    Angle { angle: 0.0, center: a, v1: d, v2: d, len1: len, len2: len }
}

/// Angle ABC at vertex B.
pub fn angle_three_points(a: Vec3, b: Vec3, c: Vec3) -> Option<Angle> {
    let (v1, v2) = (a - b, c - b);
    let (len1, len2) = (v1.length(), v2.length());
    if len1 < 1e-6 || len2 < 1e-6 {
        return None;
    }
    Some(angle_of(b, v1 / len1, len1, v2 / len2, len2))
}

// ---- polygon ----------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Polygon {
    pub area: f64,
    pub perimeter: f64,
    pub centroid: Vec3,
}

/// Area of a closed polygon through 3D points (Newell's method): for points that are not exactly
/// coplanar, the area projected on the best-fit plane.
pub fn polygon_area(points: &[Vec3]) -> Polygon {
    let mut normal = glam::DVec3::ZERO;
    let mut perimeter = 0.0;
    let mut centroid = Vec3::ZERO;
    for (i, p) in points.iter().enumerate() {
        let q = points[(i + 1) % points.len()];
        let (p, q) = (p.as_dvec3(), q.as_dvec3());
        normal.x += (p.y - q.y) * (p.z + q.z);
        normal.y += (p.z - q.z) * (p.x + q.x);
        normal.z += (p.x - q.x) * (p.y + q.y);
        perimeter += p.distance(q);
        centroid += p.as_vec3();
    }
    if !points.is_empty() {
        centroid /= points.len() as f32;
    }
    Polygon { area: normal.length() / 2.0, perimeter, centroid }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit cube from (0,0,0) to (1,1,1), outward winding, separate vertices per face.
    fn cube() -> Layer {
        let quads: [[[f32; 3]; 4]; 6] = [
            [[0., 0., 0.], [0., 1., 0.], [1., 1., 0.], [1., 0., 0.]],
            [[0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]],
            [[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]],
            [[0., 1., 0.], [0., 1., 1.], [1., 1., 1.], [1., 1., 0.]],
            [[0., 0., 0.], [0., 0., 1.], [0., 1., 1.], [0., 1., 0.]],
            [[1., 0., 0.], [1., 1., 0.], [1., 1., 1.], [1., 0., 1.]],
        ];
        let mut positions = Vec::new();
        let mut indices = Vec::new();
        for q in quads {
            let base = (positions.len() / 3) as u32;
            for p in q {
                positions.extend_from_slice(&p);
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        Layer {
            name: "cube".into(),
            color: [1.0; 3],
            normals: vec![0.0; positions.len()],
            positions,
            colors: None,
            indices,
            elements: Vec::new(),
            props: Vec::new(),
            hidden: false,
        }
    }

    #[test]
    fn cube_has_twelve_edges() {
        let topo = LayerTopology::build(&cube());
        assert_eq!(topo.lines.len(), 12, "{:?}", topo.lines);
        assert!(topo.neighbors.iter().all(|&n| n >= 0));
    }

    #[test]
    fn cube_face_volume_and_area() {
        let layer = cube();
        let topo = LayerTopology::build(&layer);
        let face = face_region(&layer, &topo, 0, 0..12, &Mat4::IDENTITY);
        assert_eq!(face.faces.len(), 2);
        assert!((face.area - 1.0).abs() < 1e-6 && (face.perimeter - 4.0).abs() < 1e-6 && !face.curved);
        let v = volume(&[(&layer, 0..12)], &Mat4::IDENTITY);
        assert!((v.volume - 1.0).abs() < 1e-6 && (v.surface_area - 6.0).abs() < 1e-6 && v.closed);
    }

    #[test]
    fn perpendicular_to_a_face_and_an_edge() {
        let top = Entity::Plane { n: Vec3::Z, p: Vec3::new(0.5, 0.5, 1.0) };
        let corner = Entity::Point(Vec3::new(1.0, 0.0, 0.0));
        let d = perpendicular(&top, &corner);
        assert!((d.p1.distance(d.p2) - 1.0).abs() < 1e-6);
        let edge = Entity::Line { a: Vec3::ZERO, b: Vec3::X, p: Vec3::new(0.5, 0.0, 0.0) };
        let d = perpendicular(&edge, &Entity::Point(Vec3::new(3.0, 2.0, 0.0)));
        assert_eq!(d.p1, Vec3::new(3.0, 0.0, 0.0));
        assert_eq!(d.extensions.len(), 1, "the foot lies on the extension of the edge");
    }

    #[test]
    fn angles() {
        let floor = Entity::Plane { n: Vec3::Z, p: Vec3::new(0.5, 0.5, 0.0) };
        let wall = Entity::Plane { n: Vec3::X, p: Vec3::new(0.0, 0.5, 0.5) };
        let a = angle_between(&floor, &wall).unwrap();
        assert!((a.angle.to_degrees() - 90.0).abs() < 1e-3);
        let edge = Entity::Line { a: Vec3::ZERO, b: Vec3::new(1.0, 0.0, 1.0), p: Vec3::new(0.5, 0.0, 0.5) };
        let a = angle_between(&edge, &floor).unwrap();
        assert!((a.angle.to_degrees() - 45.0).abs() < 1e-3);
        assert!(angle_between(&floor, &Entity::Plane { n: Vec3::Z, p: Vec3::Z }).is_none());
        let a = angle_three_points(Vec3::X, Vec3::ZERO, Vec3::Y).unwrap();
        assert!((a.angle.to_degrees() - 90.0).abs() < 1e-3);
    }

    #[test]
    fn square_polygon() {
        let p = polygon_area(&[Vec3::ZERO, Vec3::X * 2.0, Vec3::new(2.0, 3.0, 0.0), Vec3::Y * 3.0]);
        assert!((p.area - 6.0).abs() < 1e-9 && (p.perimeter - 10.0).abs() < 1e-6);
    }
}
