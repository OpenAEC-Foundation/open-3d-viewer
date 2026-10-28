//! Measuring in the viewport, as Trimble Connect: distances (perpendicular between points, edges
//! and faces), angles, the area of a face, the volume of an element and the area of a polygon.
//!
//! A click snaps to what is under the cursor: a corner, the middle of an edge, an edge (the real
//! model edges from `geometry`), else the face. Results are kept in world space until they are
//! deleted, the model changes or the up axis is flipped.

pub mod format;
pub mod geometry;
pub mod overlay;

use crate::model::{Layer, Model};
use format::{Category, UnitSettings};
use geometry::{Angle, Distance, Entity, FaceRegion, LayerTopology, Polygon, VolumeResult};
use glam::{Mat4, Vec3};
use openaec_ui::Language;
use std::collections::HashMap;
use std::sync::Arc;

/// Snap distance (logical pixels).
pub const SNAP_PX: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Distance,
    Angle,
    Area,
    Volume,
    Polygon,
}

impl Tool {
    pub const ALL: [Tool; 5] = [Tool::Distance, Tool::Angle, Tool::Area, Tool::Volume, Tool::Polygon];

    pub fn label(self, l: Language) -> &'static str {
        match self {
            Tool::Distance => l.text("Afstand", "Distance"),
            Tool::Angle => l.text("Hoek", "Angle"),
            Tool::Area => l.text("Oppervlak", "Area"),
            Tool::Volume => l.text("Volume", "Volume"),
            Tool::Polygon => l.text("Polygoon", "Polygon"),
        }
    }

    /// What to click, shown over the viewport while the tool is on.
    pub fn hint(self, l: Language, picks: usize) -> &'static str {
        match (self, picks) {
            (Tool::Distance, 0) => l.text("Klik op een punt, rand of vlak", "Click a point, edge or face"),
            (Tool::Distance, _) => l.text("Klik op het tweede punt, rand of vlak", "Click the second point, edge or face"),
            (Tool::Angle, 0) => l.text(
                "Klik op een rand of vlak, of op drie punten (hoekpunt in het midden)",
                "Click an edge or face, or three points (apex second)",
            ),
            (Tool::Angle, _) => l.text("Klik op de tweede rand, vlak of punt", "Click the second edge, face or point"),
            (Tool::Area, _) => l.text("Klik op een vlak", "Click a face"),
            (Tool::Volume, _) => l.text("Klik op een element", "Click an element"),
            (Tool::Polygon, n) if n < 3 => l.text("Klik op de hoekpunten", "Click the corners"),
            (Tool::Polygon, _) => l.text(
                "Klik op het eerste punt of druk Enter om te sluiten",
                "Click the first point or press Enter to close",
            ),
        }
    }
}

/// What a click snapped to, and the face under it (for an angle against a face).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snap {
    pub entity: Entity,
    pub face: Entity,
}

#[derive(Debug, Clone)]
pub enum Measurement {
    Distance(Distance),
    Angle(Angle),
    Area(FaceRegion),
    Volume { result: VolumeResult, name: String },
    Polygon { points: Vec<Vec3>, polygon: Polygon },
}

impl Measurement {
    /// The main value (SI).
    pub fn value(&self) -> (Category, f64) {
        match self {
            Measurement::Distance(d) => (Category::Length, d.p1.distance(d.p2) as f64),
            Measurement::Angle(a) => (Category::Angle, a.angle as f64),
            Measurement::Area(r) => (Category::Area, r.area),
            Measurement::Volume { result, .. } => (Category::Volume, result.volume),
            Measurement::Polygon { polygon, .. } => (Category::Area, polygon.area),
        }
    }

    pub fn title(&self, l: Language) -> String {
        match self {
            Measurement::Distance(_) => l.text("Afstand", "Distance").into(),
            Measurement::Angle(_) => l.text("Hoek", "Angle").into(),
            Measurement::Area(r) if r.curved => l.text("Oppervlak (gebogen)", "Area (curved)").into(),
            Measurement::Area(_) => l.text("Oppervlak", "Area").into(),
            Measurement::Volume { name, .. } => format!("{} · {name}", l.text("Volume", "Volume")),
            Measurement::Polygon { .. } => l.text("Polygoon", "Polygon").into(),
        }
    }

    /// The value as shown on the label in the viewport.
    pub fn label(&self, units: &UnitSettings, l: Language) -> String {
        let (category, value) = self.value();
        units.format(l, category, value)
    }

    /// Further values for the properties pane: (label, value).
    pub fn details(&self, units: &UnitSettings, l: Language) -> Vec<(&'static str, String)> {
        let len = |v: f64| units.format(l, Category::Length, v);
        match self {
            Measurement::Distance(d) => {
                let delta = (d.p2 - d.p1).abs();
                vec![("ΔX", len(delta.x as f64)), ("ΔY", len(delta.y as f64)), ("ΔZ", len(delta.z as f64))]
            }
            Measurement::Angle(_) => Vec::new(),
            Measurement::Area(r) => vec![(l.text("Omtrek", "Perimeter"), len(r.perimeter))],
            Measurement::Volume { result, .. } => {
                let mut out = vec![(l.text("Buitenoppervlak", "Surface area"), units.format(l, Category::Area, result.surface_area))];
                if !result.closed {
                    out.push((
                        l.text("Let op", "Note"),
                        l.text("niet gesloten, volume bij benadering", "not closed, volume approximate").into(),
                    ));
                }
                out
            }
            Measurement::Polygon { points, polygon } => vec![
                (l.text("Omtrek", "Perimeter"), len(polygon.perimeter)),
                (l.text("Punten", "Points"), points.len().to_string()),
            ],
        }
    }

    /// Where the label goes (world).
    pub fn anchor(&self) -> Vec3 {
        match self {
            Measurement::Distance(d) => (d.p1 + d.p2) / 2.0,
            Measurement::Angle(a) => {
                let r = arc_radius(a);
                a.center + (a.v1 + a.v2).try_normalize().unwrap_or(a.v1) * r * 1.4
            }
            Measurement::Area(r) => r.anchor,
            Measurement::Volume { result, .. } => result.center,
            Measurement::Polygon { polygon, .. } => polygon.centroid,
        }
    }
}

/// Radius of the arc drawn for an angle (world): a third of the shorter leg.
pub fn arc_radius(a: &Angle) -> f32 {
    let shorter = a.len1.min(a.len2);
    if shorter > 1e-6 { shorter / 3.0 } else { a.len1.max(a.len2).max(1e-3) / 3.0 }
}

#[derive(Default)]
pub struct MeasureState {
    pub tool: Option<Tool>,
    /// Picks of the measurement in progress.
    pub picks: Vec<Snap>,
    pub results: Vec<Measurement>,
    pub units: UnitSettings,
    /// Edges and neighbours per layer of the current model, built on first use.
    topology: HashMap<usize, Arc<LayerTopology>>,
}

impl MeasureState {
    /// Switches a tool on, or off when it is on already.
    pub fn toggle(&mut self, tool: Tool) {
        self.tool = if self.tool == Some(tool) { None } else { Some(tool) };
        self.picks.clear();
    }

    /// Forgets everything that belongs to the current model (results are in world space).
    pub fn reset(&mut self) {
        self.picks.clear();
        self.results.clear();
        self.topology.clear();
    }

    /// Esc: drop the picks in progress, else switch the tool off. False when there was nothing
    /// to stop.
    pub fn escape(&mut self) -> bool {
        if !self.picks.is_empty() {
            self.picks.clear();
            true
        } else {
            self.tool.take().is_some()
        }
    }

    fn topology(&mut self, layer: &Layer, l: usize) -> Arc<LayerTopology> {
        self.topology.entry(l).or_insert_with(|| Arc::new(LayerTopology::build(layer))).clone()
    }

    /// A click on triangle `t` of layer `l` at `hit` (model space). `pixel` is the world size of
    /// one logical pixel at the hit.
    pub fn click(&mut self, model: &Model, l: usize, t: usize, hit: Vec3, mm: &Mat4, pixel: f32) {
        let Some(tool) = self.tool else { return };
        let layer = &model.layers[l];
        let topo = self.topology(layer, l);
        let tolerance = SNAP_PX * pixel;
        match tool {
            Tool::Area => {
                let allowed = element_triangles(layer, t);
                self.results.push(Measurement::Area(geometry::face_region(layer, &topo, t, allowed, mm)));
            }
            Tool::Volume => {
                let element = layer.element_at(t);
                let ranges = match element {
                    Some(id) => model.element_ranges(id),
                    None => vec![(l, 0, layer.indices.len() as u32)],
                };
                let parts: Vec<_> = ranges
                    .iter()
                    .map(|&(li, first, count)| (&model.layers[li], first as usize / 3..(first + count) as usize / 3))
                    .collect();
                let name = element
                    .and_then(|id| {
                        let props = model.element_props(id);
                        ["type", "name", "category"]
                            .iter()
                            .find_map(|k| props.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone()))
                    })
                    .unwrap_or_else(|| layer.name.clone());
                self.results.push(Measurement::Volume { result: geometry::volume(&parts, mm), name });
            }
            Tool::Distance | Tool::Angle | Tool::Polygon => {
                let s = snap(layer, &topo, t, hit, mm, tolerance);
                self.add_pick(tool, s, tolerance);
            }
        }
    }

    fn add_pick(&mut self, tool: Tool, s: Snap, tolerance: f32) {
        match tool {
            Tool::Distance => {
                if let Some(first) = self.picks.pop() {
                    self.results.push(Measurement::Distance(geometry::perpendicular(&first.entity, &s.entity)));
                } else {
                    self.picks.push(s);
                }
            }
            Tool::Angle => match self.picks.first().map(|f| f.entity) {
                None => self.picks.push(s),
                // three points: A, apex, C
                Some(Entity::Point(_)) => {
                    self.picks.push(s);
                    if let [a, b, c] = self.picks[..] {
                        let angle = geometry::angle_three_points(a.entity.pick(), b.entity.pick(), c.entity.pick());
                        self.results.extend(angle.map(Measurement::Angle));
                        self.picks.clear();
                    }
                }
                // an edge or face against an edge or face (a corner counts as its face)
                Some(first) => {
                    let second = if matches!(s.entity, Entity::Point(_)) { s.face } else { s.entity };
                    let angle = geometry::angle_between(&first, &second)
                        .unwrap_or_else(|| geometry::parallel_angle(first.pick(), second.pick()));
                    self.results.push(Measurement::Angle(angle));
                    self.picks.clear();
                }
            },
            Tool::Polygon => {
                let p = s.entity.pick();
                let closes = self.picks.len() >= 3 && self.picks[0].entity.pick().distance(p) <= tolerance;
                if closes {
                    self.finish();
                } else {
                    self.picks.push(s);
                }
            }
            Tool::Area | Tool::Volume => {}
        }
    }

    /// Enter: close the polygon in progress.
    pub fn finish(&mut self) {
        if self.tool == Some(Tool::Polygon) && self.picks.len() >= 3 {
            let points: Vec<Vec3> = self.picks.iter().map(|s| s.entity.pick()).collect();
            let polygon = geometry::polygon_area(&points);
            self.results.push(Measurement::Polygon { points, polygon });
            self.picks.clear();
        }
    }

    /// Sum of the results that can be added up, per category, in the order of `Category::ALL`.
    pub fn totals(&self) -> Vec<(Category, f64, usize)> {
        let mut out: Vec<(Category, f64, usize)> = Vec::new();
        for m in &self.results {
            let (c, v) = m.value();
            if !c.summable() {
                continue;
            }
            match out.iter_mut().find(|t| t.0 == c) {
                Some(t) => {
                    t.1 += v;
                    t.2 += 1;
                }
                None => out.push((c, v, 1)),
            }
        }
        out.sort_by_key(|t| Category::ALL.iter().position(|c| *c == t.0));
        out
    }
}

/// The triangles of the element that triangle `t` belongs to (the whole layer without elements).
fn element_triangles(layer: &Layer, t: usize) -> std::ops::Range<usize> {
    let at = (3 * t) as u32;
    layer
        .elements
        .iter()
        .find(|e| e.1 <= at && at < e.1 + e.2)
        .map(|e| e.1 as usize / 3..(e.1 + e.2) as usize / 3)
        .unwrap_or(0..layer.indices.len() / 3)
}

fn closest_on_segment(p: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let d = b - a;
    let len2 = d.length_squared();
    if len2 < 1e-12 {
        return a;
    }
    a + d * ((p - a).dot(d) / len2).clamp(0.0, 1.0)
}

/// Snaps a click on triangle `t` at `hit` (model space): the nearest corner or edge middle within
/// `tolerance` (world), else the nearest model edge, else the face.
pub fn snap(layer: &Layer, topo: &LayerTopology, t: usize, hit: Vec3, mm: &Mat4, tolerance: f32) -> Snap {
    let hit = mm.transform_point3(hit);
    let tri = geometry::triangle(layer, t).map(|p| mm.transform_point3(p));
    let n = (tri[1] - tri[0]).cross(tri[2] - tri[0]).normalize_or_zero();
    let face = Entity::Plane { n, p: hit };

    // the nearest model edge
    let mut edge: Option<(f32, Vec3, Vec3)> = None;
    for &(a, b) in &topo.lines {
        let (a, b) = (mm.transform_point3(a), mm.transform_point3(b));
        let d = closest_on_segment(hit, a, b).distance(hit);
        if d <= tolerance && edge.is_none_or(|e| d < e.0) {
            edge = Some((d, a, b));
        }
    }

    // corners: of the triangle, and the ends and middle of that edge
    let mut corners = tri.to_vec();
    if let Some((_, a, b)) = edge {
        corners.extend([a, b, (a + b) / 2.0]);
    }
    let corner = corners
        .into_iter()
        .map(|c| (c.distance(hit), c))
        .filter(|(d, _)| *d <= tolerance)
        .min_by(|x, y| x.0.total_cmp(&y.0));

    let entity = match (corner, edge) {
        (Some((_, c)), _) => Entity::Point(c),
        (None, Some((_, a, b))) => Entity::Line { a, b, p: closest_on_segment(hit, a, b) },
        (None, None) => face,
    };
    Snap { entity, face }
}

/// World size of one logical pixel at `world` in a viewport of `size`.
pub fn pixel_size(view_proj: &Mat4, size: (f32, f32), world: Vec3) -> f32 {
    let clip = *view_proj * world.extend(1.0);
    if clip.w.abs() < 1e-9 {
        return 0.0;
    }
    let ndc = clip.truncate() / clip.w;
    let inv = view_proj.inverse();
    let moved = inv.project_point3(ndc + Vec3::new(2.0 / size.0.max(1.0), 0.0, 0.0));
    moved.distance(world)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square_layer() -> Layer {
        // a 2 × 2 square in the XY plane, two triangles
        Layer {
            name: "plate".into(),
            color: [1.0; 3],
            positions: vec![0., 0., 0., 2., 0., 0., 2., 2., 0., 0., 2., 0.],
            normals: vec![0.0; 12],
            colors: None,
            indices: vec![0, 1, 2, 0, 2, 3],
            elements: Vec::new(),
            props: Vec::new(),
            hidden: false,
        }
    }

    #[test]
    fn snaps_to_corner_edge_and_face() {
        let layer = square_layer();
        let topo = LayerTopology::build(&layer);
        let s = snap(&layer, &topo, 0, Vec3::new(1.95, 0.05, 0.0), &Mat4::IDENTITY, 0.1);
        assert_eq!(s.entity, Entity::Point(Vec3::new(2.0, 0.0, 0.0)));
        let s = snap(&layer, &topo, 0, Vec3::new(0.5, 0.05, 0.0), &Mat4::IDENTITY, 0.1);
        assert!(matches!(s.entity, Entity::Line { .. }), "{s:?}");
        let s = snap(&layer, &topo, 0, Vec3::new(1.03, 0.05, 0.0), &Mat4::IDENTITY, 0.1);
        assert_eq!(s.entity, Entity::Point(Vec3::new(1.0, 0.0, 0.0)), "the middle of the edge: {s:?}");
        let s = snap(&layer, &topo, 0, Vec3::new(1.0, 0.98, 0.0), &Mat4::IDENTITY, 0.1);
        assert!(matches!(s.entity, Entity::Plane { .. }), "the diagonal is no model edge: {s:?}");
        let s = snap(&layer, &topo, 0, Vec3::new(1.5, 0.7, 0.0), &Mat4::IDENTITY, 0.1);
        assert!(matches!(s.entity, Entity::Plane { .. }), "{s:?}");
    }

    #[test]
    fn polygon_closes_on_the_first_point() {
        let mut m = MeasureState { tool: Some(Tool::Polygon), ..Default::default() };
        let face = Entity::Plane { n: Vec3::Z, p: Vec3::ZERO };
        for p in [Vec3::ZERO, Vec3::X, Vec3::new(1.0, 1.0, 0.0), Vec3::Y, Vec3::new(0.01, 0.0, 0.0)] {
            m.add_pick(Tool::Polygon, Snap { entity: Entity::Point(p), face }, 0.05);
        }
        assert!(m.picks.is_empty());
        let (c, area) = m.results[0].value();
        assert_eq!(c, Category::Area);
        assert!((area - 1.0).abs() < 1e-9);
    }

    #[test]
    fn angle_from_three_points_and_from_faces() {
        let mut m = MeasureState { tool: Some(Tool::Angle), ..Default::default() };
        let floor = Entity::Plane { n: Vec3::Z, p: Vec3::ZERO };
        for p in [Vec3::X, Vec3::ZERO, Vec3::new(1.0, 1.0, 0.0)] {
            m.add_pick(Tool::Angle, Snap { entity: Entity::Point(p), face: floor }, 0.01);
        }
        assert!((m.results[0].value().1.to_degrees() - 45.0).abs() < 1e-3);
        let wall = Entity::Plane { n: Vec3::X, p: Vec3::new(0.0, 0.5, 0.5) };
        m.add_pick(Tool::Angle, Snap { entity: floor, face: floor }, 0.01);
        m.add_pick(Tool::Angle, Snap { entity: wall, face: wall }, 0.01);
        assert!((m.results[1].value().1.to_degrees() - 90.0).abs() < 1e-3);
    }

    #[test]
    fn pixel_size_matches_the_view() {
        let mut cam = crate::viewport::camera::Camera::default();
        cam.ortho = true;
        cam.target = Vec3::ZERO;
        cam.distance = 10.0;
        let vp = cam.view_proj(1.0);
        let h = 2.0 * 10.0 * (cam.fov_y / 2.0).tan();
        assert!((pixel_size(&vp, (500.0, 500.0), Vec3::ZERO) - h / 500.0).abs() < 1e-4);
    }
}
