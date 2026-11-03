//! What a click would pick, found while the cursor moves over the model.
//!
//! The snap is measured on screen, in pixels: a corner or the middle of an edge within
//! [`SNAP_PX`](super::SNAP_PX) of the cursor wins, else a model edge within that distance, else the
//! face under the cursor. Only edges of the layer under the cursor count, and only where they are
//! not behind the face under the cursor (a hidden back edge does not catch the cursor) and not cut
//! away by the section box or plane. The click takes the hover as it is, so it picks exactly what
//! was highlighted.

use super::geometry::{self, Entity, FaceRegion, LayerTopology, LINE_BLOCK};
use super::{pixel_size, Snap};
use crate::model::Layer;
use glam::{Mat4, Vec2, Vec3, Vec4};
use openaec_ui::Language;
use std::sync::Arc;

/// The view a pick is made in: world to clip space and the viewport size (logical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screen {
    pub view_proj: Mat4,
    pub size: (f32, f32),
}

impl Screen {
    fn to_pixels(&self, clip: Vec4) -> Vec2 {
        let ndc = clip.truncate() / clip.w;
        Vec2::new((ndc.x + 1.0) * 0.5 * self.size.0, (1.0 - ndc.y) * 0.5 * self.size.1)
    }

    /// Where a world point is on screen, or None behind the camera.
    pub fn project(&self, world: Vec3) -> Option<Vec2> {
        let clip = self.view_proj * world.extend(1.0);
        (clip.w > 1e-6).then(|| self.to_pixels(clip))
    }

    /// A world segment on screen, cut off where it goes behind the camera.
    fn segment(&self, a: Vec3, b: Vec3) -> Option<(Vec2, Vec2)> {
        const NEAR: f32 = 1e-6;
        let (mut ca, mut cb) = (self.view_proj * a.extend(1.0), self.view_proj * b.extend(1.0));
        if ca.w <= NEAR && cb.w <= NEAR {
            return None;
        }
        if ca.w <= NEAR {
            ca = cb + (ca - cb) * ((cb.w - NEAR) / (cb.w - ca.w));
        } else if cb.w <= NEAR {
            cb = ca + (cb - ca) * ((ca.w - NEAR) / (ca.w - cb.w));
        }
        Some((self.to_pixels(ca), self.to_pixels(cb)))
    }

    /// Whether the box `lo`..`hi` (model space; `mm` takes it to world) lies on screen farther than
    /// `margin` pixels from `at`. False when part of it is behind the camera.
    fn box_away(&self, lo: Vec3, hi: Vec3, mm: &Mat4, at: Vec2, margin: f32) -> bool {
        let (mut min, mut max) = (Vec2::INFINITY, Vec2::NEG_INFINITY);
        for i in 0..8 {
            let corner = Vec3::new([lo.x, hi.x][i & 1], [lo.y, hi.y][(i >> 1) & 1], [lo.z, hi.z][i >> 2]);
            let Some(p) = self.project(mm.transform_point3(corner)) else { return false };
            (min, max) = (min.min(p), max.max(p));
        }
        at.x < min.x - margin || at.y < min.y - margin || at.x > max.x + margin || at.y > max.y + margin
    }

    /// The ray through viewport point `at`: a point on the near plane and the unit direction.
    pub fn ray(&self, at: Vec2) -> (Vec3, Vec3) {
        let (w, h) = (self.size.0.max(1.0), self.size.1.max(1.0));
        let (nx, ny) = (2.0 * at.x / w - 1.0, 1.0 - 2.0 * at.y / h);
        let inv = self.view_proj.inverse();
        let origin = inv.project_point3(Vec3::new(nx, ny, 0.0));
        let far = inv.project_point3(Vec3::new(nx, ny, 1.0));
        (origin, (far - origin).normalize_or_zero())
    }
}

/// Where the cursor is, and which world points are shown there (not cut away by the section box
/// or plane).
pub struct Pointer<'a> {
    pub screen: Screen,
    /// Viewport logical pixels.
    pub cursor: Vec2,
    pub shown: &'a dyn Fn(Vec3) -> bool,
}

/// What the cursor snapped to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapKind {
    Corner,
    /// The middle of a model edge.
    Middle,
    Edge,
    Face,
}

impl SnapKind {
    pub fn label(self, l: Language) -> &'static str {
        match self {
            SnapKind::Corner => l.text("Punt", "Point"),
            SnapKind::Middle => l.text("Midden", "Midpoint"),
            SnapKind::Edge => l.text("Rand", "Edge"),
            SnapKind::Face => l.text("Vlak", "Face"),
        }
    }
}

fn distance_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let d = b - a;
    let len2 = d.length_squared();
    let t = if len2 < 1e-12 { 0.0 } else { ((p - a).dot(d) / len2).clamp(0.0, 1.0) };
    p.distance(a + d * t)
}

/// The point of segment a–b nearest to the ray `origin + t·dir`.
fn nearest_to_ray(a: Vec3, b: Vec3, origin: Vec3, dir: Vec3) -> Vec3 {
    let (u, v, w) = ((b - a).as_dvec3(), dir.as_dvec3(), (a - origin).as_dvec3());
    let (uu, uv, vv, uw, vw) = (u.dot(u), u.dot(v), v.dot(v), u.dot(w), v.dot(w));
    if uu < 1e-18 {
        return a;
    }
    let denom = uu * vv - uv * uv;
    // along the view direction: the end nearest to the eye
    let s = if denom < 1e-9 * uu * vv { -uw / uu } else { (uv * vw - vv * uw) / denom };
    (a.as_dvec3() + u * s.clamp(0.0, 1.0)).as_vec3()
}

/// Snaps the cursor, whose ray hits triangle `t` of `layer` at `hit` (model space; `mm` takes it
/// to world): the nearest corner or edge middle within `tolerance` pixels, else the nearest model
/// edge within it, else the face. Without the layer's edges (`topo` still being built) only the
/// corners of the triangle snap.
pub fn snap(
    layer: &Layer,
    topo: Option<&LayerTopology>,
    t: usize,
    hit: Vec3,
    mm: &Mat4,
    pointer: &Pointer,
    tolerance: f32,
) -> (Snap, SnapKind) {
    let (screen, cursor) = (&pointer.screen, pointer.cursor);
    let hit = mm.transform_point3(hit);
    let tri = geometry::triangle(layer, t).map(|p| mm.transform_point3(p));
    let n = (tri[1] - tri[0]).cross(tri[2] - tri[0]).normalize_or_zero();
    let face = Entity::Plane { n, p: hit };
    let (eye, dir) = screen.ray(cursor);

    // seen: on the viewer's side of the face under the cursor, give or take a pixel's depth, and
    // not cut away
    let toward_viewer = if n.dot(eye - hit) < 0.0 { -n } else { n };
    let slack = pixel_size(&screen.view_proj, screen.size, hit);
    let seen = |q: Vec3| toward_viewer.dot(q - hit) >= -slack && (pointer.shown)(q);

    let mut point: Option<(f32, Vec3, SnapKind)> = None;
    let mut consider = |q: Vec3, kind: SnapKind| {
        let Some(at) = screen.project(q) else { return };
        let d = at.distance(cursor);
        if d <= tolerance && point.is_none_or(|p| d < p.0) && seen(q) {
            point = Some((d, q, kind));
        }
    };
    let mut edge: Option<(f32, Vec3, Vec3, Vec3)> = None;
    let (lines, blocks) = topo.map_or((&[][..], &[][..]), |topo| (&topo.lines[..], &topo.blocks[..]));
    let near = blocks.iter().enumerate().filter(|&(_, &(lo, hi))| !screen.box_away(lo, hi, mm, cursor, tolerance));
    for &(a, b) in near.flat_map(|(i, _)| &lines[i * LINE_BLOCK..((i + 1) * LINE_BLOCK).min(lines.len())]) {
        let (a, b) = (mm.transform_point3(a), mm.transform_point3(b));
        let Some((sa, sb)) = screen.segment(a, b) else { continue };
        let d = distance_to_segment(cursor, sa, sb);
        if d > tolerance {
            continue;
        }
        consider(a, SnapKind::Corner);
        consider(b, SnapKind::Corner);
        consider((a + b) / 2.0, SnapKind::Middle);
        let p = nearest_to_ray(a, b, eye, dir);
        if edge.is_none_or(|e| d < e.0) && seen(p) {
            edge = Some((d, a, b, p));
        }
    }
    for c in tri {
        consider(c, SnapKind::Corner);
    }

    let (entity, kind) = match (point, edge) {
        (Some((_, q, kind)), _) => (Entity::Point(q), kind),
        (None, Some((_, a, b, p))) => (Entity::Line { a, b, p }, SnapKind::Edge),
        (None, None) => (face, SnapKind::Face),
    };
    (Snap { entity, face }, kind)
}

/// What the cursor is over while measuring, as the click will take it.
#[derive(Debug, Clone)]
pub struct Hover {
    /// The cursor (viewport logical pixels) and the view it was found in.
    pub cursor: Vec2,
    pub screen: Screen,
    pub layer: usize,
    pub triangle: usize,
    /// World size of one logical pixel at the hit.
    pub pixel: f32,
    pub snap: Snap,
    pub kind: SnapKind,
    /// The face that is highlighted: what the area tool measures, or the flat face of a face pick.
    pub face: Option<Arc<FaceRegion>>,
}

impl Hover {
    /// Whether a click at `at` in `screen` takes this hover: the same view and within the few
    /// pixels a click may jitter.
    pub fn holds_for(&self, screen: &Screen, at: Vec2) -> bool {
        self.screen == *screen && self.cursor.distance(at) <= HOLD_PX
    }
}

/// A click this close to where the hover was found takes the hover (as the viewport tells a
/// click from a drag).
pub const HOLD_PX: f32 = 4.0;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport::camera::Camera;

    /// An axis-aligned box from `lo` to `hi`, outward winding, separate vertices per face.
    fn cuboid(lo: Vec3, hi: Vec3) -> Layer {
        let c = |x: usize, y: usize, z: usize| {
            [[lo.x, hi.x][x], [lo.y, hi.y][y], [lo.z, hi.z][z]]
        };
        let quads = [
            [c(0, 0, 0), c(0, 1, 0), c(1, 1, 0), c(1, 0, 0)],
            [c(0, 0, 1), c(1, 0, 1), c(1, 1, 1), c(0, 1, 1)],
            [c(0, 0, 0), c(1, 0, 0), c(1, 0, 1), c(0, 0, 1)],
            [c(0, 1, 0), c(0, 1, 1), c(1, 1, 1), c(1, 1, 0)],
            [c(0, 0, 0), c(0, 0, 1), c(0, 1, 1), c(0, 1, 0)],
            [c(1, 0, 0), c(1, 1, 0), c(1, 1, 1), c(1, 0, 1)],
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
        Layer { name: "box".into(), normals: vec![0.0; positions.len()], positions, indices, ..Layer::default() }
    }

    /// A camera looking at `target` from `yaw`/`pitch` (degrees) at `distance`, 800 × 600 pixels.
    fn view(target: Vec3, yaw: f32, pitch: f32, distance: f32, ortho: bool) -> Screen {
        let mut cam = Camera::default();
        cam.target = target;
        cam.yaw = yaw.to_radians();
        cam.pitch = pitch.to_radians();
        cam.distance = distance;
        cam.ortho = ortho;
        cam.scene_radius = 10.0;
        Screen { view_proj: cam.view_proj(800.0 / 600.0), size: (800.0, 600.0) }
    }

    /// Hovers the cursor at `cursor` the way the app does: cast the ray, then snap.
    fn hover(layer: &Layer, screen: &Screen, cursor: Vec2) -> Option<(Snap, SnapKind)> {
        hover_cut(layer, screen, cursor, &|_| true)
    }

    /// The same with a section that shows only the points `shown` lets through.
    fn hover_cut(layer: &Layer, screen: &Screen, cursor: Vec2, shown: &dyn Fn(Vec3) -> bool) -> Option<(Snap, SnapKind)> {
        let topo = LayerTopology::build(layer);
        let (origin, dir) = screen.ray(cursor);
        let mut best: Option<(f32, usize)> = None;
        for t in 0..layer.indices.len() / 3 {
            let [a, b, c] = geometry::triangle(layer, t);
            let (e1, e2) = (b - a, c - a);
            let h = dir.cross(e2);
            let det = e1.dot(h);
            if det.abs() < 1e-12 {
                continue;
            }
            let s = origin - a;
            let u = s.dot(h) / det;
            let q = s.cross(e1);
            let v = dir.dot(q) / det;
            let d = e2.dot(q) / det;
            let shows = shown(origin + dir * d);
            if u >= 0.0 && v >= 0.0 && u + v <= 1.0 && d > 0.0 && best.is_none_or(|b| d < b.0) && shows {
                best = Some((d, t));
            }
        }
        let (d, t) = best?;
        let pointer = Pointer { screen: *screen, cursor, shown };
        Some(snap(layer, Some(&topo), t, origin + dir * d, &Mat4::IDENTITY, &pointer, super::super::SNAP_PX))
    }

    fn at(screen: &Screen, p: Vec3) -> Vec2 {
        screen.project(p).unwrap()
    }

    /// The top of a 2 × 2 slab seen from straight above.
    fn plate() -> (Layer, Screen) {
        let layer = cuboid(Vec3::ZERO, Vec3::new(2.0, 2.0, 0.2));
        // pitch 90°: looking down
        (layer, view(Vec3::new(1.0, 1.0, 0.2), 0.0, 89.9, 5.0, true))
    }

    #[test]
    fn corner_beats_edge_beats_face() {
        let (layer, screen) = plate();
        let corner = at(&screen, Vec3::new(2.0, 0.0, 0.2));
        let along = (at(&screen, Vec3::new(0.0, 0.0, 0.2)) - corner).normalize();
        let inward = (at(&screen, Vec3::new(2.0, 2.0, 0.2)) - corner).normalize();

        // 6 px from the corner along the edge, right on the edge: the corner wins
        let (s, kind) = hover(&layer, &screen, corner + along * 6.0 + inward * 0.5).unwrap();
        assert_eq!((s.entity, kind), (Entity::Point(Vec3::new(2.0, 0.0, 0.2)), SnapKind::Corner));

        // well along the edge, 5 px inside: the edge, picked where the cursor is
        let (s, kind) = hover(&layer, &screen, corner + along * 60.0 + inward * 5.0).unwrap();
        assert_eq!(kind, SnapKind::Edge);
        let Entity::Line { a, b, p } = s.entity else { panic!("{s:?}") };
        assert!(a.y.abs() < 1e-5 && b.y.abs() < 1e-5 && (a.z - 0.2).abs() < 1e-5, "the front top edge: {a} {b}");
        assert!(p.y.abs() < 1e-5 && p.x < 2.0 && p.x > 1.0, "{p}");

        // near the middle of that edge: the middle
        let middle = at(&screen, Vec3::new(1.0, 0.0, 0.2));
        let (s, kind) = hover(&layer, &screen, middle + along * 3.0 + inward * 3.0).unwrap();
        assert_eq!((s.entity, kind), (Entity::Point(Vec3::new(1.0, 0.0, 0.2)), SnapKind::Middle));

        // 11 px inside: the face, through the cursor
        let (s, kind) = hover(&layer, &screen, corner + along * 60.0 + inward * 16.0).unwrap();
        assert_eq!(kind, SnapKind::Face);
        assert!(matches!(s.entity, Entity::Plane { n, .. } if n.z.abs() > 0.99), "{s:?}");

        // the diagonal of the top is no model edge
        let (_, kind) = hover(&layer, &screen, at(&screen, Vec3::new(1.2, 1.2, 0.2)) + Vec2::new(1.0, 0.0)).unwrap();
        assert_eq!(kind, SnapKind::Face);
    }

    #[test]
    fn the_tolerance_is_in_pixels() {
        let layer = cuboid(Vec3::ZERO, Vec3::new(2.0, 2.0, 0.2));
        // the same 3 cm from the corner (6 px zoomed out, 60 px zoomed in)
        let off = Vec3::new(1.97, 0.03, 0.2);
        let far = view(Vec3::new(1.0, 1.0, 0.2), 0.0, 89.9, 5.0, true);
        assert_eq!(hover(&layer, &far, at(&far, off)).unwrap().1, SnapKind::Corner);
        let near = view(Vec3::new(1.9, 0.1, 0.2), 0.0, 89.9, 0.5, true);
        assert_eq!(hover(&layer, &near, at(&near, off)).unwrap().1, SnapKind::Face);
    }

    #[test]
    fn hidden_edges_do_not_snap() {
        // a unit cube seen from the front (looking along +Y) in perspective: its back edges show
        // just inside the front face
        let layer = cuboid(Vec3::ZERO, Vec3::ONE);
        let screen = view(Vec3::new(0.5, 0.5, 0.5), -90.0, 0.0, 3.0, false);
        let back = at(&screen, Vec3::new(0.5, 1.0, 1.0));
        let front = at(&screen, Vec3::new(0.5, 0.0, 1.0));
        assert!(front.distance(back) > 12.0, "the back edge shows inside the front face");
        let (s, kind) = hover(&layer, &screen, back + Vec2::new(30.0, 0.0)).unwrap();
        assert_eq!(kind, SnapKind::Face, "{s:?}");
    }

    /// Boxes from `lo` to `hi` in one layer.
    fn boxes(spans: &[(Vec3, Vec3)]) -> Layer {
        let mut layer = Layer::default();
        for &(lo, hi) in spans {
            let b = cuboid(lo, hi);
            let base = (layer.positions.len() / 3) as u32;
            layer.positions.extend_from_slice(&b.positions);
            layer.indices.extend(b.indices.iter().map(|i| i + base));
        }
        layer
    }

    #[test]
    fn corners_cut_away_by_a_section_do_not_snap() {
        // a facade in front of a wide back wall, seen from the front; the section keeps y > 0.5,
        // so the cursor looks through the cut facade onto the back wall, right at a facade corner
        let layer = boxes(&[(Vec3::ZERO, Vec3::new(1.0, 0.1, 1.0)), (Vec3::new(-1.0, 0.9, -1.0), Vec3::new(2.0, 1.0, 2.0))]);
        let screen = view(Vec3::new(0.5, 0.5, 0.5), -90.0, 0.0, 3.0, false);
        let corner = at(&screen, Vec3::new(1.0, 0.0, 1.0)) + Vec2::new(-2.0, 2.0);
        let (s, kind) = hover(&layer, &screen, corner).unwrap();
        assert_eq!((s.entity, kind), (Entity::Point(Vec3::new(1.0, 0.0, 1.0)), SnapKind::Corner), "uncut");
        let (s, kind) = hover_cut(&layer, &screen, corner, &|p| p.y > 0.5).unwrap();
        assert_eq!(kind, SnapKind::Face, "{s:?}");
        assert!((s.entity.pick().y - 0.9).abs() < 1e-4, "on the back wall: {s:?}");
    }

    #[test]
    fn far_blocks_of_edges_are_skipped_not_lost() {
        // a row of 300 boxes: many blocks of edges; the edge under the cursor still snaps
        let spans: Vec<_> = (0..300).map(|i| (Vec3::new(2.0 * i as f32, 0.0, 0.0), Vec3::new(2.0 * i as f32 + 1.0, 1.0, 1.0))).collect();
        let layer = boxes(&spans);
        let topo = LayerTopology::build(&layer);
        assert_eq!(topo.lines.len(), 300 * 12);
        assert!(topo.blocks.len() > 50);
        let screen = view(Vec3::new(400.5, 0.5, 0.5), -90.0, 20.0, 4.0, false);
        let edge = at(&screen, Vec3::new(400.3, 0.0, 1.0));
        let (s, kind) = hover(&layer, &screen, edge + Vec2::new(0.0, 2.0)).unwrap();
        assert_eq!(kind, SnapKind::Edge, "{s:?}");
    }

    #[test]
    fn an_edge_in_front_of_a_receding_face_snaps() {
        // the top of a long box seen nearly edge-on: just above its front edge the cursor hits the
        // top far behind, yet the edge is only pixels away on screen
        let layer = cuboid(Vec3::ZERO, Vec3::new(1.0, 10.0, 1.0));
        let screen = view(Vec3::new(0.5, 0.0, 1.0), -90.0, 3.0, 4.0, false);
        let edge = at(&screen, Vec3::new(0.3, 0.0, 1.0));
        let (s, kind) = hover(&layer, &screen, edge - Vec2::new(0.0, 3.0)).unwrap();
        assert_eq!(kind, SnapKind::Edge, "{s:?}");
        assert!(s.entity.pick().y.abs() < 1e-3 && (s.entity.pick().z - 1.0).abs() < 1e-3, "{s:?}");
    }
}
