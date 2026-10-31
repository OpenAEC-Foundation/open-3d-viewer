//! MicroStation design files (`.dgn`): version 7 and earlier (Intergraph ISFF) and V8 (V8, V8i
//! and CONNECT: an OLE2 compound file holding zlib-compressed element streams).
//!
//! The V7 reader is written from Intergraph's public "Intergraph Standard File Formats (Element
//! Structure)" and the clean-room notes of the DGN analysis (design file header, colour table and
//! quaternion layout were measured on sample files). The V8 reader is written only from the
//! clean-room specification of the DGN analysis (`SPEC_v0.1.md` §B) and Microsoft's public
//! [MS-CFB] description of the compound file. V8 dimensions, multi-lines, tags, surfaces, solids,
//! meshes, further models and other elements the specification does not describe yet are skipped
//! and counted.
//!
//! The scene draws triangles only, so lines, arcs, curves and text boxes become thin ribbons: flat
//! in 2D files, two crossed ribbons in 3D files, as wide as the line weight on a scale set by the
//! drawing extent. Closed planar elements, cones, extruded solids and B-spline surfaces of 3D files
//! become faces. Each level is a layer; each top-level element (a whole cell, shared cell, text
//! node or complex chain) is one pickable element.

use super::{Loaded, Loader, Phase, Progress, Source, DEFAULT_COLOR};
use crate::model::{Layer, ProjectData};
use glam::{DMat3, DQuat, DVec3, DVec4};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;

const MAX_ELEMENTS: usize = 2_000_000;
const MAX_TRIANGLES: usize = 4_000_000;
const MAX_DEPTH: usize = 16;
/// Elements drawn at most, counting each placement of a shared cell definition again.
const MAX_VISITS: usize = 4 * MAX_ELEMENTS;
/// The largest decompressed element stream of a V8 file.
const MAX_UNPACKED: usize = 1 << 30;
const OLE_SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
/// Fixed-point unit of quaternions, B-spline knots and weights: 2^31 - 1.
const UNIT: f64 = 2_147_483_647.0;

pub struct DgnLoader;

impl Loader for DgnLoader {
    fn name(&self) -> &'static str { "MicroStation DGN" }
    fn extensions(&self) -> &'static [&'static str] { &["dgn"] }
    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }
    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        progress.set(Phase::Convert);
        read(&data)
    }
}

/// Reads a whole design file: V8 files are compound files, everything else is read as V7.
pub fn read(data: &[u8]) -> Result<Loaded, String> {
    if data.starts_with(&OLE_SIGNATURE) { read_v8(data) } else { read_v7(data) }
}

fn read_v7(data: &[u8]) -> Result<Loaded, String> {
    let elements = elements(data)?;
    let design = Design::new(&elements)?;
    let mut converter = Converter { design: &design, scene: Scene::default() };
    let mut i = 0;
    while i < elements.len() {
        i = converter.top_level(&elements, i);
    }
    converter.scene.finish(&Summary {
        version: format!("V7 (ISFF), {}", if design.three_d { "3D" } else { "2D" }),
        three_d: design.three_d,
        units: design.units.clone(),
        metres_per_uor: design.metres_per_uor,
        origin: design.origin,
        level_names: BTreeMap::new(),
    })
}

// ---------------------------------------------------------------------------------------------
// Shared by both versions: the scene, colours and geometry.

/// What the project information reports, and the names of the layers.
struct Summary {
    version: String,
    three_d: bool,
    units: String,
    metres_per_uor: f64,
    /// Global origin in units of resolution.
    origin: DVec3,
    /// Level names by level number (V7) or level ID (V8); others are "Level n".
    level_names: BTreeMap<u32, String>,
}

enum Shape {
    /// A polyline, drawn as a ribbon; one point is a point marker.
    Stroke(Vec<DVec3>),
    Faces(Vec<[DVec3; 3]>),
}

/// Geometry of one element in world metres.
struct Prim {
    id: u64,
    level: u32,
    color: [u8; 3],
    weight: u8,
    shape: Shape,
}

/// What a design file draws, in world metres, with the properties of each pickable element and
/// counts of drawn and skipped elements.
#[derive(Default)]
struct Scene {
    prims: Vec<Prim>,
    props: Vec<(u64, u32, String)>,
    next_id: u64,
    /// Points kept so far, to stop before shared cells multiply a drawing past the triangle limit.
    points: usize,
    counts: BTreeMap<&'static str, usize>,
    omitted: BTreeMap<String, usize>,
}

impl Scene {
    fn omit(&mut self, what: &str) {
        match self.omitted.get_mut(what) {
            Some(count) => *count += 1,
            None => { self.omitted.insert(what.to_string(), 1); }
        }
    }

    fn new_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// The drawing has outgrown the triangle limit.
    fn full(&self) -> bool { self.points > 3 * MAX_TRIANGLES }

    /// Adds a shape in design coordinates, which `world` maps to metres; empty and non-finite
    /// shapes are dropped.
    fn push(&mut self, id: u64, level: u32, color: [u8; 3], weight: u8, shape: Shape, world: impl Fn(DVec3) -> DVec3) {
        let shape = match shape {
            Shape::Stroke(points) => Shape::Stroke(points.into_iter().map(&world).collect()),
            Shape::Faces(triangles) => Shape::Faces(triangles.into_iter().map(|t| t.map(&world)).collect()),
        };
        let count = match &shape {
            Shape::Stroke(points) if !points.is_empty() && points.iter().all(|p| p.is_finite()) => points.len(),
            Shape::Faces(triangles) if !triangles.is_empty() && triangles.iter().flatten().all(|p| p.is_finite()) => 3 * triangles.len(),
            _ => return,
        };
        self.points += count;
        if !self.full() { self.prims.push(Prim { id, level, color, weight, shape }); }
    }

    /// Counts a drawn top-level element and keeps its properties.
    fn record(&mut self, id: u64, kind: u16, level: u32, props: serde_json::Value) {
        *self.counts.entry(kind_name(kind)).or_default() += 1;
        self.props.push((id, level, props.to_string()));
    }

    fn finish(self, summary: &Summary) -> Result<Loaded, String> {
        if self.full() { return Err("MicroStation DGN: triangle limit exceeded".into()); }
        let omitted = self.omitted.iter().map(|(name, count)| format!("{name}: {count}")).collect::<Vec<_>>().join(", ");
        if self.prims.is_empty() {
            return Err(format!("MicroStation DGN: no drawable elements. Skipped: {}", if omitted.is_empty() { "none" } else { &omitted }));
        }
        let half_width = line_half_width(&self.prims);
        let origin = self.prims.iter().find_map(|prim| match &prim.shape {
            Shape::Stroke(points) => points.first().copied(),
            Shape::Faces(triangles) => triangles.first().map(|t| t[0]),
        }).unwrap_or(DVec3::ZERO);
        let mut layers: BTreeMap<u32, Layer> = BTreeMap::new();
        let mut triangles = 0usize;
        for prim in &self.prims {
            let (geometry, color) = match &prim.shape {
                Shape::Stroke(points) => (ribbons(points, half_width * (1.0 + 0.35 * f64::from(prim.weight)), summary.three_d), ink(prim.color)),
                Shape::Faces(faces) => (faces.iter().filter_map(|&t| {
                    let normal = (t[1] - t[0]).cross(t[2] - t[0]);
                    (normal.length_squared() > 1e-24).then(|| (t, normal.normalize()))
                }).collect(), prim.color),
            };
            triangles += geometry.len();
            if triangles > MAX_TRIANGLES { return Err("MicroStation DGN: triangle limit exceeded".into()); }
            let layer = layers.entry(prim.level).or_insert_with(|| Layer {
                name: summary.level_names.get(&prim.level).cloned().unwrap_or_else(|| format!("Level {}", prim.level)),
                color: DEFAULT_COLOR, colors: Some(Vec::new()), ..Layer::default()
            });
            let first = u32::try_from(layer.indices.len()).map_err(|_| "MicroStation DGN: layer index limit exceeded")?;
            for (triangle, normal) in geometry {
                for p in triangle {
                    let local = (p - origin).as_vec3();
                    if !local.is_finite() { return Err("MicroStation DGN: model extent exceeds supported coordinates".into()); }
                    layer.indices.push(u32::try_from(layer.positions.len() / 3).map_err(|_| "MicroStation DGN: layer vertex limit exceeded")?);
                    layer.positions.extend(local.to_array());
                    layer.normals.extend(normal.as_vec3().to_array());
                    layer.colors.as_mut().unwrap().push([color[0], color[1], color[2], 255]);
                }
            }
            let count = layer.indices.len() as u32 - first;
            match layer.elements.last_mut() {
                Some(last) if last.0 == prim.id && last.1 + last.2 == first => last.2 += count,
                _ if count > 0 => layer.elements.push((prim.id, first, count)),
                _ => {}
            }
        }
        // Properties go with the first layer that holds the element.
        let mut placed = BTreeSet::new();
        for (id, level, props) in self.props {
            let target = if layers.get(&level).is_some_and(|l| l.elements.iter().any(|e| e.0 == id)) { Some(level) }
                else { layers.iter().find(|(_, l)| l.elements.iter().any(|e| e.0 == id)).map(|(&k, _)| k) };
            if let Some(level) = target.filter(|_| placed.insert(id)) {
                layers.get_mut(&level).unwrap().props.push((id, props));
            }
        }
        let counts = self.counts.iter().map(|(name, count)| format!("{name}: {count}")).collect::<Vec<_>>().join(", ");
        let mut project = ProjectData::default();
        project.info = vec![
            ("DGN version".into(), summary.version.clone()),
            ("DGN units".into(), summary.units.clone()),
            ("DGN metres per UOR".into(), summary.metres_per_uor.to_string()),
            ("DGN global origin (UOR)".into(), format!("{}, {}, {}", summary.origin.x, summary.origin.y, summary.origin.z)),
            ("DGN world origin (metres)".into(), format!("{}, {}, {}", origin.x, origin.y, origin.z)),
            ("DGN elements".into(), counts),
            ("DGN skipped elements".into(), if omitted.is_empty() { "0".into() } else { omitted }),
        ];
        Ok(Loaded { layers: layers.into_values().filter(|l| !l.indices.is_empty()).collect(), views: Vec::new(), project })
    }
}

fn metres(unit: &str) -> Option<f64> {
    Some(match unit.to_ascii_uppercase().as_str() {
        "M" | "MT" | "METER" | "METRE" => 1.0,
        "MM" => 0.001,
        "CM" => 0.01,
        "DM" => 0.1,
        "KM" => 1000.0,
        "FT" | "'" | "IF" => 0.3048,
        "SF" | "USFT" | "SFT" => 1200.0 / 3937.0,
        "IN" | "\"" => 0.0254,
        "YD" => 0.9144,
        "MI" => 1609.344,
        _ => return None,
    })
}

/// 256 RGB triples from byte `at` of a colour table element.
fn color_table(bytes: &[u8], at: usize) -> Option<Vec<[u8; 3]>> {
    bytes.get(at..at + 768).map(|table| table.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect())
}

/// Colour `index` from a stored table, or from our own palette without one.
fn table_color(table: Option<&[[u8; 3]]>, index: u8) -> [u8; 3] {
    match table {
        // Slot 0 of a stored table is the background; colour c is slot c + 1.
        Some(table) => table[(usize::from(index) + 1) % table.len()],
        None => default_color(index),
    }
}

/// Our own palette for files without a colour table: the documented first eight colours, then
/// evenly spread hues.
fn default_color(index: u8) -> [u8; 3] {
    const FIRST: [[u8; 3]; 8] = [
        [255, 255, 255], [0, 0, 255], [0, 255, 0], [255, 0, 0], [255, 255, 0], [255, 0, 255], [255, 127, 0], [0, 255, 255],
    ];
    if let Some(color) = FIRST.get(usize::from(index)) { return *color; }
    let hue = (f64::from(index) * 137.508) % 360.0;
    let (value, saturation) = (if index % 2 == 0 { 0.85 } else { 0.6 }, 0.75);
    let c = value * saturation;
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let (r, g, b) = match (hue / 60.0) as u32 { 0 => (c, x, 0.), 1 => (x, c, 0.), 2 => (0., c, x), 3 => (0., x, c), 4 => (x, 0., c), _ => (c, 0., x) };
    [r, g, b].map(|v| ((v + value - c) * 255.0).round() as u8)
}

/// The rotation of a stored quaternion (w, x, y, z); the columns are the element's axes in design
/// coordinates. The file stores the inverse rotation: a text turned +30 degrees about Z has
/// (cos 15, 0, 0, -sin 15).
fn quaternion(w: f64, x: f64, y: f64, z: f64) -> DMat3 {
    let q = DQuat::from_xyzw(-x, -y, -z, w);
    if q.length_squared() >= 1e-12 { DMat3::from_quat(q.normalize()) } else { DMat3::IDENTITY }
}

/// A 2D rotation angle in radians: one unit is 1/360000 degree.
fn angle(value: i32) -> f64 { (f64::from(value) / 360_000.0).to_radians() }

/// An arc sweep: sign-magnitude according to ISFF, two's complement in some writers; 0 is a full turn.
fn sweep(raw: u32) -> f64 {
    let magnitude = raw & 0x7FFF_FFFF;
    let value = if magnitude > 360 * 360_000 { raw as i32 } else if raw & 0x8000_0000 != 0 { -(magnitude as i32) } else { magnitude as i32 };
    if value == 0 { std::f64::consts::TAU } else { angle(value) }
}

/// Six characters in DEC Radix-50, two words.
fn radix50(words: [u16; 2]) -> String {
    const CHARS: &[u8; 40] = b" ABCDEFGHIJKLMNOPQRSTUVWXYZ$.%0123456789";
    words.iter().flat_map(|&w| [w / 1600, (w / 40) % 40, w % 40]).map(|c| CHARS.get(usize::from(c)).map_or('?', |&c| c as char))
        .collect::<String>().trim().to_string()
}

fn kind_name(kind: u16) -> &'static str {
    match kind {
        2 => "Cell", 3 => "Line", 4 => "Line string", 6 => "Shape", 7 => "Text node", 11 => "Curve",
        12 => "Complex chain", 14 => "Complex shape", 15 => "Ellipse", 16 => "Arc", 17 => "Text",
        18 => "Surface", 19 => "Solid", 21 => "B-spline poles", 22 => "Point string", 23 => "Cone",
        24 => "B-spline surface", 25 => "B-spline boundary", 26 => "B-spline knots", 27 => "B-spline curve",
        28 => "B-spline weights", 33 => "Dimension", 34 => "Shared cell definition", 35 => "Shared cell",
        36 => "Multi-line", 37 => "Tag", 87 => "Raster header", 88 => "Raster data",
        _ => "Unknown element",
    }
}

/// The name of an element type for the skipped counts; unknown types by number.
fn kind_label(kind: u16) -> String {
    match kind_name(kind) {
        "Unknown element" => format!("Element type {kind}"),
        name => name.to_string(),
    }
}

/// A closed outline: faces when `filled` (closed elements of 3D files that are not holes) and it
/// triangulates, a closed polyline otherwise.
fn closed_shape(mut points: Vec<DVec3>, filled: bool) -> Shape {
    if points.len() > 1 && points.first() != points.last() { points.push(points[0]); }
    if filled {
        let triangles = triangulate(&points);
        if !triangles.is_empty() { return Shape::Faces(triangles); }
    }
    Shape::Stroke(points)
}

/// The box of a text, from its origin (the lower left) along its rotated axes.
fn text_box(origin: DVec3, rotation: DMat3, width: f64, height: f64) -> Vec<DVec3> {
    let (x, y) = (rotation.x_axis * width, rotation.y_axis * height);
    vec![origin, origin + x, origin + x + y, origin + y, origin]
}

/// Points of an elliptical arc: `center + a cos t + b sin t`.
fn ellipse(center: DVec3, a: DVec3, b: DVec3, start: f64, sweep: f64) -> Vec<DVec3> {
    let segments = ((sweep.abs() / std::f64::consts::TAU * 72.0).ceil() as usize).clamp(4, 72);
    (0..=segments).map(|k| {
        let t = start + sweep * k as f64 / segments as f64;
        center + a * t.cos() + b * t.sin()
    }).collect()
}

/// The ISFF curve (type 11): piecewise cubics through points 2..n-3 with Akima-style tangents;
/// the first two and last two points only shape the ends.
fn akima(points: &[DVec3]) -> Vec<DVec3> {
    let n = points.len();
    let lengths: Vec<f64> = points.windows(2).map(|p| p[0].distance(p[1])).collect();
    let slope = |k: usize| if lengths[k] > 0.0 { (points[k + 1] - points[k]) / lengths[k] } else { DVec3::ZERO };
    let tangent = |k: usize| {
        let (m2, m1, m0, p1) = (slope(k - 2), slope(k - 1), slope(k), slope(k + 1));
        let axis = |a: f64, b: f64, c: f64, d: f64| {
            let (w1, w2) = ((d - c).abs(), (b - a).abs());
            if w1 + w2 > 0.0 { (b * w1 + c * w2) / (w1 + w2) } else { (b + c) / 2.0 }
        };
        DVec3::new(axis(m2.x, m1.x, m0.x, p1.x), axis(m2.y, m1.y, m0.y, p1.y), axis(m2.z, m1.z, m0.z, p1.z))
    };
    let mut out = vec![points[2]];
    for k in 2..n - 3 {
        let length = lengths[k];
        if length <= 0.0 { continue; }
        let (t0, t1, p0, p1) = (tangent(k), tangent(k + 1), points[k], points[k + 1]);
        let chord = (p1 - p0) / length;
        let b = (chord * 3.0 - t0 * 2.0 - t1) / length;
        let a = (t0 + t1 - chord * 2.0) / (length * length);
        for s in 1..=8 {
            let d = length * f64::from(s) / 8.0;
            out.push(p0 + t0 * d + b * d * d + a * d * d * d);
        }
    }
    out
}

/// A full knot vector for `poles` poles of `order`; open curves are clamped. Closed curves are
/// treated as uniform periodic and need their first `order - 1` poles repeated.
fn knot_vector(poles: usize, order: usize, interior: Option<Vec<f64>>, closed: bool) -> Vec<f64> {
    if closed {
        return (0..poles + order - 1 + order).map(|k| k as f64).collect();
    }
    let inner = interior.filter(|v| v.len() == poles - order && v.windows(2).all(|w| w[0] <= w[1]))
        .unwrap_or_else(|| (1..=poles - order).map(|k| k as f64 / (poles - order + 1) as f64).collect());
    std::iter::repeat_n(0.0, order).chain(inner).chain(std::iter::repeat_n(1.0, order)).collect()
}

/// De Boor's algorithm on homogeneous poles.
fn de_boor(poles: &[DVec4], knots: &[f64], order: usize, t: f64) -> DVec4 {
    let n = poles.len();
    let mut s = order - 1;
    while s + 1 < n && knots[s + 1] <= t { s += 1; }
    let mut d: Vec<DVec4> = (0..order).map(|j| poles[j + s + 1 - order]).collect();
    for r in 1..order {
        for j in (r..order).rev() {
            let i = j + s + 1 - order;
            let denominator = knots[i + order - r] - knots[i];
            let alpha = if denominator.abs() < 1e-15 { 0.0 } else { (t - knots[i]) / denominator };
            d[j] = d[j - 1] * (1.0 - alpha) + d[j] * alpha;
        }
    }
    d[order - 1]
}

/// Samples a B-spline over its domain; `poles` already include a periodic wrap.
fn sample(poles: &[DVec4], knots: &[f64], order: usize, samples: usize) -> Vec<DVec4> {
    let (lo, hi) = (knots[order - 1], knots[poles.len()]);
    (0..=samples).map(|k| de_boor(poles, knots, order, lo + (hi - lo) * k as f64 / samples as f64)).collect()
}

fn wrap(mut poles: Vec<DVec4>, order: usize, closed: bool) -> Vec<DVec4> {
    if closed { for k in 0..order - 1 { poles.push(poles[k % poles.len()]); } }
    poles
}

/// Points along a B-spline curve with homogeneous `poles` and the stored interior knots; with
/// fewer poles than the order, the control polygon.
fn bspline_points(poles: Vec<DVec4>, order: usize, closed: bool, interior: Option<Vec<f64>>) -> Vec<DVec3> {
    if poles.len() < order { return poles.iter().map(|p| p.truncate() / p.w).collect(); }
    let knots = knot_vector(poles.len(), order, interior, closed);
    let poles = wrap(poles, order, closed);
    let samples = (poles.len() * 8).clamp(16, 400);
    sample(&poles, &knots, order, samples).into_iter().map(|p| p.truncate() / p.w).collect()
}

/// Triangulates a planar polygon (closing point optional).
fn triangulate(points: &[DVec3]) -> Vec<[DVec3; 3]> {
    let mut points = points.to_vec();
    points.dedup();
    if points.len() > 1 && points.first() == points.last() { points.pop(); }
    if points.len() < 3 { return Vec::new(); }
    let origin = points[0];
    let normal = (0..points.len()).fold(DVec3::ZERO, |sum, i| sum + (points[i] - origin).cross(points[(i + 1) % points.len()] - origin));
    if normal.length_squared() <= 1e-24 { return Vec::new(); }
    let axis = if normal.x.abs() >= normal.y.abs() && normal.x.abs() >= normal.z.abs() { 0 }
        else if normal.y.abs() >= normal.z.abs() { 1 } else { 2 };
    let coords: Vec<f64> = points.iter().flat_map(|p| {
        let p = *p - origin;
        match axis { 0 => [p.y, p.z], 1 => [p.x, p.z], _ => [p.x, p.y] }
    }).collect();
    let Ok(indices) = earcutr::earcut(&coords, &[], 2) else { return Vec::new() };
    indices.chunks_exact(3).map(|t| [points[t[0]], points[t[1]], points[t[2]]]).collect()
}

/// Half the width of a weight-0 line: a fixed share of the drawing extent, measured between the
/// 2nd and 98th percentile so a few stray far-away elements do not make every line fat.
fn line_half_width(prims: &[Prim]) -> f64 {
    let points: Vec<DVec3> = prims.iter().flat_map(|prim| match &prim.shape {
        Shape::Stroke(points) => points.clone(),
        Shape::Faces(triangles) => triangles.iter().flatten().copied().collect(),
    }).collect();
    let step = (points.len() / 100_000).max(1);
    let mut extent = DVec3::ZERO;
    for axis in 0..3 {
        let mut values: Vec<f64> = points.iter().step_by(step).map(|p| p[axis]).collect();
        values.sort_by(f64::total_cmp);
        let at = |f: f64| values[((values.len() - 1) as f64 * f).round() as usize];
        extent[axis] = at(0.98) - at(0.02);
    }
    let diagonal = extent.length();
    if diagonal > 0.0 { diagonal / 3000.0 } else { 0.01 }
}

/// Line colours as ink on paper: very light colours (white is colour 0) draw dark grey.
fn ink(color: [u8; 3]) -> [u8; 3] {
    let luma = 0.2126 * f64::from(color[0]) + 0.7152 * f64::from(color[1]) + 0.0722 * f64::from(color[2]);
    if luma > 215.0 { [45, 45, 48] } else { color }
}

/// A polyline as thin quads with square ends: flat (facing +Z) in 2D files, two crossed quads per
/// segment in 3D files. A single point becomes a small square.
fn ribbons(points: &[DVec3], half: f64, three_d: bool) -> Vec<([DVec3; 3], DVec3)> {
    let mut out = Vec::new();
    let mut quad = |a: DVec3, b: DVec3, offset: DVec3, normal: DVec3| {
        let (p, q, r, s) = (a + offset, b + offset, b - offset, a - offset);
        out.push(([p, q, r], normal));
        out.push(([p, r, s], normal));
    };
    let mut points = points.to_vec();
    points.dedup();
    if points.len() == 1 {
        let p = points[0];
        quad(p - DVec3::X * half * 2.0, p + DVec3::X * half * 2.0, DVec3::Y * half * 2.0, DVec3::Z);
        if three_d { quad(p - DVec3::X * half * 2.0, p + DVec3::X * half * 2.0, DVec3::Z * half * 2.0, DVec3::Y); }
        return out;
    }
    for pair in points.windows(2) {
        let direction = (pair[1] - pair[0]).normalize();
        let (a, b) = (pair[0] - direction * half, pair[1] + direction * half);
        if three_d {
            let side = if direction.cross(DVec3::Z).length_squared() > 1e-6 { direction.cross(DVec3::Z).normalize() } else { DVec3::X };
            let up = direction.cross(side).normalize();
            quad(a, b, side * half, up);
            quad(a, b, up * half, side);
        } else {
            quad(a, b, DVec3::new(-direction.y, direction.x, 0.0) * half, DVec3::Z);
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// V7: Intergraph ISFF.

/// One element: type, level, flags and its words (including the header).
#[derive(Clone, Copy)]
struct Element<'a> {
    kind: u8,
    level: u8,
    complex: bool,
    offset: usize,
    bytes: &'a [u8],
}

impl Element<'_> {
    fn words(&self) -> usize { self.bytes.len() / 2 }
    fn word(&self, i: usize) -> u16 {
        self.bytes.get(2 * i..2 * i + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
    }
    /// 32-bit integer, high word first ("middle-endian").
    fn ulong(&self, i: usize) -> u32 { (u32::from(self.word(i)) << 16) | u32::from(self.word(i + 1)) }
    fn long(&self, i: usize) -> i32 { self.ulong(i) as i32 }
    /// VAX D-float, most significant word first.
    fn double(&self, i: usize) -> f64 {
        let u = (0..4).fold(0u64, |u, k| (u << 16) | u64::from(self.word(i + k)));
        let exponent = (u >> 55) & 0xFF;
        if exponent == 0 { return 0.0; }
        f64::from_bits((u & (1 << 63)) | ((exponent + 894) << 52) | ((u & ((1 << 55) - 1)) >> 3))
    }
    fn properties(&self) -> u16 { self.word(16) }
    fn class(&self) -> u16 { self.properties() & 0xF }
    /// The H bit: hole, orphan cell, infinite line or disjoint point string.
    fn h_bit(&self) -> bool { self.properties() & 0x8000 != 0 }
    fn color(&self) -> u8 { (self.word(17) >> 8) as u8 }
    fn weight(&self) -> u8 { ((self.word(17) >> 3) & 0x1F) as u8 }
    fn style(&self) -> u8 { (self.word(17) & 7) as u8 }
    fn graphic(&self) -> bool { !matches!(self.kind, 0 | 1 | 5 | 8 | 9 | 10 | 66) && self.words() >= 18 }
}

fn elements(data: &[u8]) -> Result<Vec<Element<'_>>, String> {
    let mut out = Vec::new();
    let mut offset = 0;
    while offset + 4 <= data.len() {
        let first = u16::from_le_bytes([data[offset], data[offset + 1]]);
        if first == 0xFFFF { break; }
        let words = usize::from(u16::from_le_bytes([data[offset + 2], data[offset + 3]])) + 2;
        let end = (offset + 2 * words).min(data.len());
        if first & 0x8000 == 0 {
            out.push(Element {
                kind: ((first >> 8) & 0x7F) as u8, level: (first & 0x3F) as u8, complex: first & 0x80 != 0,
                offset, bytes: &data[offset..end],
            });
            if out.len() > MAX_ELEMENTS { return Err("MicroStation DGN: element limit exceeded".into()); }
        }
        offset += 2 * words;
    }
    match out.first() {
        Some(tcb) if tcb.kind == 9 && tcb.level == 8 => Ok(out),
        _ => Err("MicroStation DGN: not a design file (no design file header)".into()),
    }
}

/// The design file header (TCB), units and colour table.
struct Design {
    three_d: bool,
    /// Global origin in units of resolution.
    origin: DVec3,
    metres_per_uor: f64,
    units: String,
    colors: Option<Vec<[u8; 3]>>,
}

impl Design {
    fn new(elements: &[Element]) -> Result<Self, String> {
        let tcb = elements[0];
        let three_d = tcb.bytes[0] & 0x40 != 0;
        let sub_per_master = f64::from(tcb.long(556));
        let uor_per_sub = f64::from(tcb.long(558));
        let name = |word: usize| -> String {
            tcb.bytes.get(2 * word..2 * word + 2).map(|b| b.iter().filter(|c| c.is_ascii_graphic()).map(|&c| c as char).collect())
                .unwrap_or_default()
        };
        let (master, sub) = (name(560), name(561));
        let (uor_per_master, units) = if sub_per_master > 0.0 && uor_per_sub > 0.0 {
            (sub_per_master * uor_per_sub, format!("1 {master} = {sub_per_master} {sub} = {} UOR", sub_per_master * uor_per_sub))
        } else {
            (1.0, "none in the file header (1 UOR = 1 m assumed)".to_string())
        };
        let metres_per_master = metres(&master)
            .or_else(|| metres(&sub).map(|m| m * sub_per_master.max(1.0)))
            .unwrap_or(1.0);
        let origin = DVec3::new(tcb.double(620), tcb.double(624), tcb.double(628));
        if !origin.is_finite() { return Err("MicroStation DGN: invalid global origin".into()); }
        let colors = elements.iter().filter(|e| e.kind == 5 && e.level == 1).find_map(|e| color_table(e.bytes, 38));
        Ok(Design { three_d, origin, metres_per_uor: metres_per_master / uor_per_master, units, colors })
    }

    /// Design coordinates (UOR) to world metres.
    fn world(&self, p: DVec3) -> DVec3 { (p - self.origin) * self.metres_per_uor }

    /// A point of longs at word `i`.
    fn point(&self, e: &Element, i: usize) -> DVec3 {
        DVec3::new(f64::from(e.long(i)), f64::from(e.long(i + 2)), if self.three_d { f64::from(e.long(i + 4)) } else { 0.0 })
    }
    /// A point of doubles at word `i`.
    fn dpoint(&self, e: &Element, i: usize) -> DVec3 {
        DVec3::new(e.double(i), e.double(i + 4), if self.three_d { e.double(i + 8) } else { 0.0 })
    }
    fn point_words(&self) -> usize { if self.three_d { 6 } else { 4 } }
    fn dpoint_words(&self) -> usize { if self.three_d { 12 } else { 8 } }
    /// Orientation at word `i`: a 2D angle (one long) or a 3D quaternion (four longs); the columns
    /// are the element's axes in design coordinates.
    fn rotation(&self, e: &Element, i: usize) -> DMat3 {
        if self.three_d {
            let [w, x, y, z] = [0, 2, 4, 6].map(|k| f64::from(e.long(i + k)) / UNIT);
            quaternion(w, x, y, z)
        } else {
            DMat3::from_rotation_z(angle(e.long(i)))
        }
    }
    fn rotation_words(&self) -> usize { if self.three_d { 8 } else { 2 } }

    fn rgb(&self, index: u8) -> [u8; 3] { table_color(self.colors.as_deref(), index) }
}

struct Converter<'a> {
    design: &'a Design,
    scene: Scene,
}

impl Converter<'_> {
    fn omit(&mut self, what: &str) { self.scene.omit(what); }

    /// Converts the top-level element at `i` with its components; returns the next element.
    fn top_level(&mut self, all: &[Element], i: usize) -> usize {
        let e = all[i];
        let next = span(all, i, 0);
        if !e.graphic() { return next; }
        let id = self.scene.new_id();
        let before = self.scene.prims.len();
        let mut texts = Vec::new();
        self.draw(all, i, id, 0, &mut texts);
        if self.scene.prims.len() == before { return next; }
        let mut props = serde_json::json!({
            "DGN element": kind_name(e.kind.into()), "Level": e.level, "Colour index": e.color(),
            "Line weight": e.weight(), "Line style": e.style(), "Class": e.class(),
        });
        if e.word(14) != 0 { props["Graphic group"] = e.word(14).into(); }
        if e.kind == 2 { props["Cell"] = radix50([e.word(19), e.word(20)]).into(); }
        if !texts.is_empty() { props["Text"] = texts.join("\n").into(); }
        self.scene.record(id, e.kind.into(), e.level.into(), props);
        next
    }

    fn push(&mut self, e: &Element, id: u64, shape: Shape) {
        let design = self.design;
        self.scene.push(id, e.level.into(), design.rgb(e.color()), e.weight(), shape, |p| design.world(p));
    }

    /// Draws element `i` (and its components) as part of pickable element `id`.
    fn draw(&mut self, all: &[Element], i: usize, id: u64, depth: usize, texts: &mut Vec<String>) {
        let e = all[i];
        if depth > MAX_DEPTH { self.omit("Too deeply nested element"); return; }
        let filled = self.design.three_d && !e.h_bit();
        match e.kind {
            2 | 7 | 12 => {
                // Cells keep their components in design coordinates; text nodes hold text elements.
                let end = span(all, i, 0);
                let mut j = i + 1;
                while j < end {
                    self.draw(all, j, id, depth + 1, texts);
                    j = span(all, j, depth + 1);
                }
            }
            14 => {
                let mut outline = Vec::new();
                let end = span(all, i, 0);
                let mut j = i + 1;
                while j < end {
                    if let Some((points, _)) = self.outline(all, j) { outline.extend(points); }
                    else { self.omit("Unsupported complex shape component"); }
                    j = span(all, j, depth + 1);
                }
                self.push(&e, id, closed_shape(outline, filled));
            }
            3 | 4 | 11 | 15 | 16 | 27 | 6 => match self.outline(all, i) {
                Some((points, closed)) if closed && e.kind != 3 => self.push(&e, id, closed_shape(points, filled)),
                Some((points, _)) => self.push(&e, id, Shape::Stroke(points)),
                None => self.omit("Malformed element"),
            },
            17 => self.text(&e, id, texts),
            18 | 19 => self.surface(all, i, id),
            22 => {
                let points = self.vertices(&e);
                if e.h_bit() { for p in points { self.push(&e, id, Shape::Stroke(vec![p])); } } else { self.push(&e, id, Shape::Stroke(points)); }
            }
            23 => self.cone(&e, id),
            24 => self.bspline_surface(all, i, id),
            kind => self.omit(&kind_label(kind.into())),
        }
    }

    /// Vertices of a line, line string, shape, curve, pole or point string element.
    fn vertices(&self, e: &Element) -> Vec<DVec3> {
        let size = self.design.point_words();
        let (first, count) = if e.kind == 3 { (18, 2) } else { (19, usize::from(e.word(18))) };
        let available = e.words().saturating_sub(first) / size;
        (0..count.min(available)).map(|k| self.design.point(e, first + k * size)).collect()
    }

    /// The points (design coordinates) of a line-like element, and whether it is closed.
    fn outline(&self, all: &[Element], i: usize) -> Option<(Vec<DVec3>, bool)> {
        let e = &all[i];
        let d = self.design;
        match e.kind {
            3 | 4 | 6 => {
                let points = self.vertices(e);
                (points.len() >= if e.kind == 3 { 2 } else { 1 }).then_some((points, e.kind == 6))
            }
            11 => {
                let points = self.vertices(e);
                (points.len() >= 6).then(|| (akima(&points), false))
            }
            15 | 16 => {
                let (start, sweep_angle, at) = if e.kind == 16 { (angle(e.long(18)), sweep(e.ulong(20)), 22) } else { (0.0, std::f64::consts::TAU, 18) };
                let (primary, secondary) = (e.double(at), e.double(at + 4));
                let rotation = d.rotation(e, at + 8);
                let center = d.dpoint(e, at + 8 + d.rotation_words());
                if e.words() < at + 8 + d.rotation_words() + d.dpoint_words() { return None; }
                Some((ellipse(center, rotation.x_axis * primary, rotation.y_axis * secondary, start, sweep_angle), e.kind == 15))
            }
            27 => bspline_curve(d, all, i).map(|points| (points, false)),
            _ => None,
        }
    }

    /// Text: its box (from the origin, which is the lower left, and the size multipliers) as an
    /// outline, the characters in the element's properties.
    fn text(&mut self, e: &Element, id: u64, texts: &mut Vec<String>) {
        let d = self.design;
        let rotation = d.rotation(e, 23);
        let origin_at = 23 + d.rotation_words();
        let count_byte = 2 * (origin_at + d.point_words());
        let Some(&count) = e.bytes.get(count_byte) else { self.omit("Malformed element"); return };
        let text: String = e.bytes.iter().skip(count_byte + 2).take(usize::from(count)).map(|&c| c as char).collect();
        // The basic character cell is 6 UOR; multipliers are thousandths.
        let height = f64::from(e.ulong(21)) / 1000.0 * 6.0;
        let width = f64::from(e.ulong(19)) / 1000.0 * 6.0 * f64::from(count).max(1.0);
        self.push(e, id, Shape::Stroke(text_box(d.point(e, origin_at), rotation, width, height)));
        if !text.trim().is_empty() { texts.push(text); }
    }

    /// Surfaces and solids of projection: faces between equal boundary outlines, capped for a solid.
    /// Others (surfaces of revolution, unequal boundaries) are drawn as their outlines.
    fn surface(&mut self, all: &[Element], i: usize, id: u64) {
        let e = all[i];
        let end = span(all, i, 0);
        let surface_type = e.bytes.get(40).copied().unwrap_or(0);
        let revolution = if e.kind == 18 { surface_type == 8 } else { surface_type == 1 };
        let mut boundaries = Vec::new();
        let mut lines = Vec::new();
        for j in i + 1..end {
            let Some((points, _)) = self.outline(all, j) else { continue };
            if all[j].class() == 0 { boundaries.push(points.clone()); }
            lines.push((j, points));
        }
        let ruled = !revolution && boundaries.len() >= 2 && boundaries.windows(2).all(|pair| pair[0].len() == pair[1].len() && pair[0].len() >= 2);
        if !ruled {
            if revolution { self.omit("Surface of revolution (outline drawn)"); }
            for (j, points) in lines { self.push(&all[j], id, Shape::Stroke(points)); }
            return;
        }
        let mut triangles = Vec::new();
        for pair in boundaries.windows(2) {
            for k in 0..pair[0].len() - 1 {
                let (a, b, c, d) = (pair[0][k], pair[0][k + 1], pair[1][k + 1], pair[1][k]);
                triangles.push([a, b, c]);
                triangles.push([a, c, d]);
            }
        }
        if e.kind == 19 {
            for cap in [&boundaries[0], &boundaries[boundaries.len() - 1]] {
                if cap.first() == cap.last() { triangles.extend(triangulate(cap)); }
            }
        }
        self.push(&e, id, Shape::Faces(triangles));
    }

    fn cone(&mut self, e: &Element, id: u64) {
        if e.words() < 59 || !self.design.three_d { self.omit("Malformed element"); return; }
        let rotation = self.design.rotation(e, 19);
        let circle = |at: usize| {
            let (center, radius) = (DVec3::new(e.double(at), e.double(at + 4), e.double(at + 8)), e.double(at + 12));
            ellipse(center, rotation.x_axis * radius, rotation.y_axis * radius, 0.0, std::f64::consts::TAU)
        };
        let (bottom, top) = (circle(27), circle(43));
        let mut triangles = Vec::new();
        for k in 0..bottom.len() - 1 {
            triangles.push([bottom[k], bottom[k + 1], top[k + 1]]);
            triangles.push([bottom[k], top[k + 1], top[k]]);
        }
        if e.word(18) & 0x8000 == 0 {
            triangles.extend(triangulate(&bottom));
            triangles.extend(triangulate(&top));
        }
        self.push(e, id, Shape::Faces(triangles));
    }

    fn bspline_surface(&mut self, all: &[Element], i: usize, id: u64) {
        let e = all[i];
        match bspline_surface(self.design, all, i) {
            Some(triangles) => {
                if all[i + 1..span(all, i, 0)].iter().any(|c| c.kind == 25) { self.omit("B-spline surface trim boundary (ignored)"); }
                self.push(&e, id, Shape::Faces(triangles));
            }
            None => self.omit("Malformed B-spline surface"),
        }
    }
}

/// The index after element `i` and its components. Complex headers count `19 + totlength` words
/// from their start; components carry the complex bit. B-spline headers are followed by their
/// knot, boundary, pole and weight elements.
fn span(all: &[Element], i: usize, depth: usize) -> usize {
    let e = all[i];
    let mut j = i + 1;
    match e.kind {
        2 | 7 | 12 | 14 | 18 | 19 if depth <= MAX_DEPTH => {
            let end = e.offset + 2 * (19 + usize::from(e.word(18)));
            while j < all.len() && all[j].offset < end && all[j].complex {
                j = span(all, j, depth + 1);
            }
        }
        24 | 27 => while j < all.len() && matches!(all[j].kind, 21 | 25 | 26 | 28) { j += 1 },
        _ => {}
    }
    j
}

/// Interior knots (fractions) from a knot element, or none.
fn knots(element: Option<&Element>, count: usize, skip: usize) -> Option<Vec<f64>> {
    let e = element?;
    let values: Vec<f64> = (0..count).map(|k| f64::from(e.long(18 + 2 * (skip + k))) / UNIT).collect();
    (e.words() >= 18 + 2 * (skip + count)).then_some(values)
}

/// Order, closed and rational flags from a B-spline flags word.
fn bspline_flags(word: u16) -> (usize, bool, bool) {
    (usize::from(word & 0xF) + 2, word & 0x80 != 0, word & 0x40 != 0)
}

/// Pole rows (with weights) following a B-spline header.
fn pole_rows(design: &Design, all: &[Element], i: usize) -> Vec<Vec<DVec4>> {
    let mut rows: Vec<Vec<DVec4>> = Vec::new();
    for e in &all[i + 1..span(all, i, 0)] {
        match e.kind {
            21 => {
                let size = design.point_words();
                let count = usize::from(e.word(18)).min(e.words().saturating_sub(19) / size);
                rows.push((0..count).map(|k| design.point(e, 19 + k * size).extend(1.0)).collect());
            }
            28 => if let Some(row) = rows.last_mut() {
                for (k, pole) in row.iter_mut().enumerate() {
                    let weight = f64::from(e.long(18 + 2 * k)) / UNIT;
                    if weight > 0.0 && e.words() >= 20 + 2 * k { *pole = (pole.truncate() * weight).extend(weight); }
                }
            },
            _ => {}
        }
    }
    rows
}

fn bspline_curve(design: &Design, all: &[Element], i: usize) -> Option<Vec<DVec3>> {
    let (order, closed, _) = bspline_flags(all[i].word(20));
    let poles = pole_rows(design, all, i).into_iter().next()?;
    let knot_element = all[i + 1..span(all, i, 0)].iter().find(|c| c.kind == 26);
    let interior = knots(knot_element, poles.len().saturating_sub(order), 0);
    let points = bspline_points(poles, order, closed, interior);
    (points.len() >= 2).then_some(points)
}

fn bspline_surface(design: &Design, all: &[Element], i: usize) -> Option<Vec<[DVec3; 3]>> {
    let e = all[i];
    let (order_u, closed_u, _) = bspline_flags(e.word(20));
    let (order_v, closed_v, _) = bspline_flags(e.word(24));
    let rows = pole_rows(design, all, i);
    let nu = rows.first()?.len();
    if nu < order_u || rows.len() < order_v || rows.iter().any(|r| r.len() != nu) { return None; }
    let knot_element = all[i + 1..span(all, i, 0)].iter().find(|c| c.kind == 26);
    let knots_u = knot_vector(nu, order_u, knots(knot_element, nu - order_u, 0), closed_u);
    let knots_v = knot_vector(rows.len(), order_v, knots(knot_element, rows.len() - order_v, nu - order_u), closed_v);
    let (su, sv) = ((nu * 4).clamp(8, 64), (rows.len() * 4).clamp(8, 64));
    let along_u: Vec<Vec<DVec4>> = rows.into_iter().map(|row| sample(&wrap(row, order_u, closed_u), &knots_u, order_u, su)).collect();
    let grid: Vec<Vec<DVec3>> = (0..=su).map(|k| {
        let column: Vec<DVec4> = along_u.iter().map(|row| row[k]).collect();
        sample(&wrap(column, order_v, closed_v), &knots_v, order_v, sv).into_iter().map(|p| p.truncate() / p.w).collect()
    }).collect();
    let mut triangles = Vec::new();
    for a in 0..su {
        for b in 0..sv {
            let (p, q, r, s) = (grid[a][b], grid[a + 1][b], grid[a + 1][b + 1], grid[a][b + 1]);
            triangles.push([p, q, r]);
            triangles.push([p, r, s]);
        }
    }
    Some(triangles)
}

// ---------------------------------------------------------------------------------------------
// V8: the compound file ([MS-CFB]).

/// Sector numbers above this are markers: free, end of chain, FAT or DIFAT sector.
const MAX_SECTOR: u32 = 0xFFFF_FFFA;
const END_OF_CHAIN: u32 = 0xFFFF_FFFE;

fn le16(data: &[u8], at: usize) -> u16 { data.get(at..at + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]])) }
fn le32(data: &[u8], at: usize) -> u32 { data.get(at..at + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])) }
fn le64(data: &[u8], at: usize) -> u64 { u64::from(le32(data, at)) | (u64::from(le32(data, at + 4)) << 32) }
/// An IEEE double; NaN outside the data, so a short element gives a non-finite shape.
fn lef64(data: &[u8], at: usize) -> f64 {
    if at.checked_add(8).is_some_and(|end| end <= data.len()) { f64::from_bits(le64(data, at)) } else { f64::NAN }
}

/// A directory entry: a storage (1), stream (2) or the root (5).
struct Entry {
    name: String,
    kind: u8,
    left: u32,
    right: u32,
    child: u32,
    start: u32,
    size: u64,
}

/// A compound file read from memory: its allocation tables, directory and mini stream.
struct Compound<'a> {
    data: &'a [u8],
    shift: usize,
    fat: Vec<u32>,
    mini_fat: Vec<u32>,
    mini_stream: Vec<u8>,
    /// Streams smaller than this live in the mini stream.
    cutoff: u64,
    entries: Vec<Entry>,
}

/// The sectors of the chain that starts at `start`; a cycle ends after as many steps as the table has.
fn chain(table: &[u32], start: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut sector = start;
    while sector <= MAX_SECTOR && out.len() < table.len() {
        out.push(sector);
        sector = table.get(sector as usize).copied().unwrap_or(END_OF_CHAIN);
    }
    out
}

fn u32s(bytes: &[u8]) -> impl Iterator<Item = u32> + '_ {
    bytes.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
}

impl<'a> Compound<'a> {
    fn open(data: &'a [u8]) -> Result<Self, String> {
        let invalid = |what: &str| format!("MicroStation DGN: not a readable V8 design file ({what})");
        if data.len() < 512 || !data.starts_with(&OLE_SIGNATURE) { return Err(invalid("too short for a compound file")); }
        let shift = usize::from(le16(data, 0x1E));
        if shift != 9 && shift != 12 { return Err(invalid("unknown sector size")); }
        if le16(data, 0x20) != 6 { return Err(invalid("unknown mini sector size")); }
        let mut file = Compound {
            data, shift, fat: Vec::new(), mini_fat: Vec::new(), mini_stream: Vec::new(), cutoff: u64::from(le32(data, 0x38)), entries: Vec::new(),
        };
        // The FAT sectors: 109 in the header, the rest in a chain of DIFAT sectors whose last
        // number points to the next one.
        let mut fat_sectors: Vec<u32> = u32s(&data[0x4C..0x200]).collect();
        let mut next = le32(data, 0x44);
        let mut difat = 0;
        while next <= MAX_SECTOR && difat <= data.len() >> shift {
            let Some(sector) = file.sector(next) else { break };
            let per = sector.len() / 4 - 1;
            fat_sectors.extend(u32s(&sector[..4 * per]));
            next = le32(sector, 4 * per);
            difat += 1;
        }
        let fat: Vec<u32> = fat_sectors.into_iter().filter(|&s| s <= MAX_SECTOR).filter_map(|s| file.sector(s)).flat_map(|sector| u32s(sector)).collect();
        file.fat = fat;
        let directory = file.regular(le32(data, 0x30), None);
        let version = le16(data, 0x1A);
        file.entries = directory.chunks_exact(128).map(|d| {
            let chars = (usize::from(le16(d, 0x40)) / 2).saturating_sub(1).min(31);
            Entry {
                name: String::from_utf16_lossy(&(0..chars).map(|k| le16(d, 2 * k)).collect::<Vec<_>>()),
                kind: d[0x42], left: le32(d, 0x44), right: le32(d, 0x48), child: le32(d, 0x4C), start: le32(d, 0x74),
                // Version 3 files may leave garbage in the high half of the size.
                size: if version == 3 { u64::from(le32(d, 0x78)) } else { le64(d, 0x78) },
            }
        }).collect();
        let Some(root) = file.entries.first().filter(|root| root.kind == 5) else { return Err(invalid("no root directory entry")) };
        let (root_start, root_size) = (root.start, root.size);
        file.mini_fat = u32s(&file.regular(le32(data, 0x3C), None)).collect();
        file.mini_stream = file.regular(root_start, Some(root_size));
        Ok(file)
    }

    /// Sector `n` (the last one of a truncated file may be short).
    fn sector(&self, n: u32) -> Option<&'a [u8]> {
        let start = (n as usize).checked_add(1)?.checked_mul(1 << self.shift)?;
        let data: &'a [u8] = self.data;
        data.get(start..data.len().min(start + (1 << self.shift))).filter(|s| s.len() >= 4)
    }

    /// The bytes of a chain of sectors, at most `size`.
    fn regular(&self, start: u32, size: Option<u64>) -> Vec<u8> {
        let limit = size.map_or(usize::MAX, |s| usize::try_from(s).unwrap_or(usize::MAX));
        let mut out = Vec::new();
        for sector in chain(&self.fat, start) {
            if out.len() >= limit { break; }
            match self.sector(sector) { Some(bytes) => out.extend_from_slice(bytes), None => break }
        }
        out.truncate(limit);
        out
    }

    /// The bytes of a chain of 64-byte sectors in the mini stream.
    fn mini(&self, start: u32, size: u64) -> Vec<u8> {
        let limit = usize::try_from(size).unwrap_or(usize::MAX);
        let mut out = Vec::new();
        for sector in chain(&self.mini_fat, start) {
            let at = sector as usize * 64;
            match self.mini_stream.get(at..self.mini_stream.len().min(at + 64)) {
                Some(bytes) if !bytes.is_empty() && out.len() < limit => out.extend_from_slice(bytes),
                _ => break,
            }
        }
        out.truncate(limit);
        out
    }

    /// The contents of stream `entry`.
    fn read(&self, entry: usize) -> Vec<u8> {
        let e = &self.entries[entry];
        if e.size < self.cutoff { self.mini(e.start, e.size) } else { self.regular(e.start, Some(e.size)) }
    }

    /// The entries of storage `parent`: a tree through the left and right siblings of its child.
    fn children(&self, parent: usize) -> Vec<usize> {
        let mut seen = vec![false; self.entries.len()];
        let mut stack = vec![self.entries[parent].child];
        let mut out = Vec::new();
        while let Some(k) = stack.pop() {
            let k = k as usize;
            let Some(e) = self.entries.get(k) else { continue };
            if std::mem::replace(&mut seen[k], true) { continue; }
            out.push(k);
            stack.extend([e.right, e.left]);
        }
        out
    }

    fn child(&self, parent: usize, name: &str) -> Option<usize> {
        self.children(parent).into_iter().find(|&k| self.entries[k].name == name)
    }

    /// The entry at a path of names below the root.
    fn path(&self, names: &[&str]) -> Option<usize> {
        names.iter().try_fold(0, |at, name| self.child(at, name))
    }
}

/// Inflates zlib data onto `out`, up to the stream size limit.
fn inflate(packed: &[u8], out: &mut Vec<u8>) -> Result<(), &'static str> {
    let room = MAX_UNPACKED.saturating_sub(out.len());
    let result = flate2::read::ZlibDecoder::new(packed).take(room as u64 + 1).read_to_end(out);
    if out.len() > MAX_UNPACKED {
        out.truncate(MAX_UNPACKED);
        return Err("Element streams too large (rest of the model)");
    }
    result.map(|_| ()).map_err(|_| "Unreadable element stream (rest of the model)")
}

/// The element streams `$1`, `$2`, ... of a storage, inflated and joined in numeric order: one
/// logical element stream, in which elements run across the stream boundaries (B.2).
fn element_stream(file: &Compound, storage: Option<usize>, scene: &mut Scene) -> Vec<u8> {
    let Some(storage) = storage else { return Vec::new() };
    let mut parts: Vec<(u64, usize)> = file.children(storage).into_iter().filter_map(|k| {
        let digits = file.entries[k].name.strip_prefix('$')?;
        let number = digits.bytes().all(|b| b.is_ascii_digit()).then(|| digits.parse().ok()).flatten()?;
        (file.entries[k].kind == 2).then_some((number, k))
    }).collect();
    parts.sort_unstable();
    let mut out = Vec::new();
    for (_, k) in parts {
        // 16 bytes of header (element counts, version, sequence number), then zlib data; a
        // stream of only the header is empty.
        let block = file.read(k);
        if block.len() <= 16 { continue; }
        if let Err(error) = inflate(&block[16..], &mut out) {
            scene.omit(error);
            break;
        }
    }
    out
}

/// One V8 element (B.3): little-endian integers and IEEE doubles at byte offsets from its start.
#[derive(Clone, Copy)]
struct V8Element<'a> {
    bytes: &'a [u8],
}

impl<'a> V8Element<'a> {
    fn len(&self) -> usize { self.bytes.len() }
    fn u16(&self, at: usize) -> u16 { le16(self.bytes, at) }
    fn u32(&self, at: usize) -> u32 { le32(self.bytes, at) }
    fn f64(&self, at: usize) -> f64 { lef64(self.bytes, at) }
    fn kind(&self) -> u16 { self.u16(0) }
    fn flags(&self) -> u16 { self.u16(2) }
    /// A component of a complex element.
    fn component(&self) -> bool { self.flags() & 0x4000 != 0 }
    /// A header with components: flagged, or of a type that always has them.
    fn complex(&self) -> bool { self.flags() & 0x2000 != 0 || matches!(self.kind(), 2 | 7 | 12 | 14 | 18 | 19 | 24 | 27 | 34) }
    /// The number of components a header counts (at +0x68), where the specification gives it.
    fn component_count(&self) -> Option<u32> { (matches!(self.kind(), 2 | 12 | 14 | 27) && self.len() >= 0x6C).then(|| self.u32(0x68)) }
    /// The level ID (graphic elements) or kind of table entry (non-graphic elements).
    fn level(&self) -> u32 { self.u32(0x0C) }
    fn graphic(&self) -> bool {
        self.flags() & 0x0400 == 0 && !matches!(self.kind(), 0 | 1 | 5 | 8 | 9 | 10 | 26 | 28 | 66 | 95..=99) && self.len() >= 0x68
    }
    fn properties(&self) -> u32 { self.u32(0x28) }
    /// Two or three coordinates per point: per element in V8.
    fn three_d(&self) -> bool { self.properties() & 0x0800 != 0 }
    /// The H bit: hole or disjoint point string.
    fn h_bit(&self) -> bool { self.properties() & 0x8000 != 0 }
    fn class(&self) -> u32 { self.properties() & 0xF }
    fn style(&self) -> u32 { self.u32(0x2C) }
    fn weight(&self) -> u8 { self.u32(0x30).min(31) as u8 }
    fn color(&self) -> u32 { self.u32(0x34) }
    fn point_size(&self) -> usize { if self.three_d() { 24 } else { 16 } }
    /// A point of two or three doubles at byte `at`.
    fn point(&self, at: usize) -> DVec3 {
        DVec3::new(self.f64(at), self.f64(at + 8), if self.three_d() { self.f64(at + 16) } else { 0.0 })
    }
    /// Orientation at byte `at`: a 2D angle in radians or a 3D quaternion of four doubles.
    fn rotation(&self, at: usize) -> DMat3 {
        if self.three_d() { quaternion(self.f64(at), self.f64(at + 8), self.f64(at + 16), self.f64(at + 24)) }
        else { DMat3::from_rotation_z(self.f64(at)) }
    }
    fn rotation_size(&self) -> usize { if self.three_d() { 32 } else { 8 } }
    /// The vertices of a line (two points at +0x68) or of a line string, shape, curve, pole or
    /// point string element (a count at +0x68, points from +0x70).
    fn vertices(&self) -> Vec<DVec3> {
        let size = self.point_size();
        let (first, count) = if self.kind() == 3 { (0x68, 2) } else { (0x70, self.u32(0x68) as usize) };
        let available = self.len().saturating_sub(first) / size;
        (0..count.min(available)).map(|k| self.point(first + k * size)).collect()
    }
    /// The linkages behind the element data, from byte `2 × attindx` (B.7): ID and the whole
    /// linkage including its header.
    fn linkages(&self) -> Vec<(u16, &'a [u8])> {
        let bytes: &'a [u8] = self.bytes;
        let mut at = (self.u32(8) as usize).saturating_mul(2);
        let mut out = Vec::new();
        while at.saturating_add(4) <= bytes.len() {
            let words = usize::from(self.u16(at) & 0xFF);
            if words == 0 { break; }
            let end = bytes.len().min(at + 2 * words);
            out.push((self.u16(at + 2), &bytes[at..end]));
            at += 2 * words;
        }
        out
    }
    /// String linkages: key and text.
    fn strings(&self) -> Vec<(u32, String)> {
        self.linkages().into_iter().filter(|(id, _)| *id == 0x56D2).map(|(_, l)| {
            let count = le32(l, 8) as usize;
            (le32(l, 4), linkage_text(l.get(12..l.len().min(12usize.saturating_add(count))).unwrap_or_default()))
        }).collect()
    }
    /// The name of a cell or shared cell: its first string linkage.
    fn name(&self) -> Option<String> { self.strings().into_iter().next().map(|(_, name)| name).filter(|name| !name.is_empty()) }
}

fn latin1(bytes: &[u8]) -> String { bytes.iter().map(|&c| char::from(c)).collect() }

/// The text of a string linkage: UTF-16LE after `FF FD`, Latin-1 otherwise; trailing NULs dropped.
fn linkage_text(bytes: &[u8]) -> String {
    let text = match bytes {
        [0xFF, 0xFD, rest @ ..] => String::from_utf16_lossy(&u16s(rest)),
        _ => latin1(bytes),
    };
    text.trim_end_matches('\0').to_string()
}

fn u16s(bytes: &[u8]) -> Vec<u16> { bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect() }

/// The characters of a text element (B.8): Latin-1 after the marker `FF FE 01 00` or without a
/// marker; other markers are not described yet, so their text is not shown (None).
fn v8_text(bytes: &[u8]) -> Option<String> {
    let bytes = match bytes {
        [0xFF, 0xFE, 0x01, 0x00, rest @ ..] => rest,
        [0xFF, 0xFD | 0xFE, ..] => return None,
        _ => bytes,
    };
    Some(latin1(bytes).trim_end_matches('\0').to_string())
}

/// Splits a logical element stream: each element follows a 4-byte prefix and is `2 × wtf` bytes
/// long, wtf being the u32 at +4.
fn v8_elements<'a>(stream: &'a [u8], scene: &mut Scene) -> Result<Vec<V8Element<'a>>, String> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 12 <= stream.len() {
        let start = at + 4;
        let size = (le32(stream, start + 4) as usize).saturating_mul(2);
        let end = start.saturating_add(size);
        if size < 8 || end > stream.len() { break; }
        out.push(V8Element { bytes: &stream[start..end] });
        if out.len() > MAX_ELEMENTS { return Err("MicroStation DGN: element limit exceeded".into()); }
        at = end;
    }
    if stream[at.min(stream.len())..].iter().any(|&b| b != 0) { scene.omit("Malformed element stream (rest skipped)"); }
    Ok(out)
}

/// The index after V8 element `i` and its components, at most `end`. A top-level element owns
/// all components that follow it; a nested one as many as its header counts (a text node: the
/// text elements that follow). B-spline headers also own their knot, pole and weight elements.
fn v8_span(all: &[V8Element], i: usize, end: usize, depth: usize) -> usize {
    let e = all[i];
    let mut j = i + 1;
    if !e.complex() || depth > MAX_DEPTH { return j; }
    let belongs = |c: &V8Element| c.component() || (e.kind() == 27 && matches!(c.kind(), 21 | 26 | 28));
    if depth == 0 {
        while j < end && belongs(&all[j]) { j += 1; }
    } else if let Some(count) = e.component_count() {
        for _ in 0..count {
            if j >= end || !belongs(&all[j]) { break; }
            j = v8_span(all, j, end, depth + 1);
        }
    } else if e.kind() == 7 {
        while j < end && all[j].component() && all[j].kind() == 17 { j += 1; }
    }
    j
}

/// The points (design coordinates) of a line-like V8 element, whose components end before
/// `stop`, and whether it is closed.
fn v8_outline(all: &[V8Element], i: usize, stop: usize) -> Option<(Vec<DVec3>, bool)> {
    let e = all[i];
    let (points, closed) = match e.kind() {
        3 | 4 | 6 => {
            let points = e.vertices();
            if points.len() < if e.kind() == 3 { 2 } else { 1 } { return None; }
            (points, e.kind() == 6)
        }
        11 => {
            let points = e.vertices();
            if points.len() < 6 { return None; }
            (akima(&points), false)
        }
        15 | 16 => {
            // Angles are radians; the sign of the sweep is the direction, and 0 a full turn as in V7.
            let arc = e.kind() == 16;
            let (start, sweep, at) = if arc { (e.f64(0x68), e.f64(0x70), 0x78) } else { (0.0, std::f64::consts::TAU, 0x68) };
            let sweep = if sweep == 0.0 { std::f64::consts::TAU } else { sweep };
            let rotation = e.rotation(at + 16);
            let center_at = at + 16 + e.rotation_size();
            if e.len() < center_at + e.point_size() { return None; }
            (ellipse(e.point(center_at), rotation.x_axis * e.f64(at), rotation.y_axis * e.f64(at + 8), start, sweep), !arc)
        }
        27 => (v8_bspline(e, &all[i + 1..stop.max(i + 1)])?, false),
        _ => return None,
    };
    points.iter().all(|p| p.is_finite()).then_some((points, closed))
}

/// A B-spline curve from its header and the knot, pole and weight elements after it (B.4).
fn v8_bspline(header: V8Element, parts: &[V8Element]) -> Option<Vec<DVec3>> {
    let flags = header.u32(0x6C);
    let (order, closed) = ((flags & 0xF) as usize + 2, flags & 0x100 != 0);
    let poles = parts.iter().find(|p| p.kind() == 21)?.vertices();
    // Knots and weights are doubles from +0x20 to the end of their element.
    let doubles = |kind: u16| parts.iter().find(|p| p.kind() == kind).map(|p| (0..p.len().saturating_sub(0x20) / 8).map(|k| p.f64(0x20 + 8 * k)).collect::<Vec<_>>());
    let ends_meet = poles.len() > 1 && poles.first() == poles.last();
    let mut homogeneous: Vec<DVec4> = poles.iter().map(|p| p.extend(1.0)).collect();
    if let Some(weights) = doubles(28) {
        for (pole, &w) in homogeneous.iter_mut().zip(&weights) {
            if w > 0.0 && w.is_finite() { *pole = (pole.truncate() * w).extend(w); }
        }
    }
    let n = poles.len();
    let (closed, interior) = match doubles(26) {
        // A closed curve whose ends meet (such as the rational circle) is an open, clamped curve
        // with the stored knots strictly between 0 and 1.
        knots if closed && ends_meet => (false, knots.map(|k| k.into_iter().take(n.saturating_sub(1)).filter(|&t| t > 0.0 && t < 1.0).collect())),
        // Other closed curves: uniform periodic.
        _ if closed => (true, None),
        // An open curve stores its interior knots.
        knots => (false, knots.map(|k| k.into_iter().take(n.saturating_sub(order)).collect())),
    };
    let points = bspline_points(homogeneous, order, closed, interior);
    (points.len() >= 2).then_some(points)
}

/// What a V8 file holds beside the model elements: units, global origin, levels, colours and
/// shared cell definitions.
struct V8Design {
    /// Global origin in units of resolution.
    origin: DVec3,
    metres_per_uor: f64,
    units: String,
    three_d: bool,
    /// Level names by level ID.
    levels: BTreeMap<u32, String>,
    colors: Option<Vec<[u8; 3]>>,
    /// Shared cell definitions by name: the definition and its components in the non-model elements.
    definitions: BTreeMap<String, (usize, usize)>,
}

impl V8Design {
    fn new(header: Option<&[u8]>, tables: &[V8Element], model: &[V8Element]) -> Self {
        let (origin, metres_per_uor, units) = model_units(header)
            .unwrap_or((DVec3::ZERO, 1.0, "none in the model header (1 UOR = 1 m assumed)".into()));
        // Levels: type 95 entries of kind 1, with the ID at +0x20 and the name in string linkage 1.
        let mut levels = BTreeMap::new();
        for e in tables.iter().filter(|e| e.kind() == 95 && e.level() == 1) {
            if let Some((_, name)) = e.strings().into_iter().find(|(key, name)| *key == 1 && !name.is_empty()) {
                levels.entry(e.u32(0x20)).or_insert(name);
            }
        }
        let colors = tables.iter().filter(|e| e.kind() == 5 && e.level() == 1).find_map(|e| color_table(e.bytes, 0x22));
        let mut definitions = BTreeMap::new();
        let mut i = 0;
        while i < tables.len() {
            let next = v8_span(tables, i, tables.len(), 0);
            if tables[i].kind() == 34 {
                if let Some(name) = tables[i].name() { definitions.entry(name).or_insert((i, next)); }
            }
            i = next;
        }
        let three_d = model.iter().any(|e| e.graphic() && e.three_d());
        V8Design { origin, metres_per_uor, units, three_d, levels, colors, definitions }
    }

    /// Design coordinates (UOR) to world metres.
    fn world(&self, p: DVec3) -> DVec3 { (p - self.origin) * self.metres_per_uor }

    fn rgb(&self, color: u32) -> [u8; 3] {
        // Extended colours (256 and up) are not described yet: white.
        u8::try_from(color).map_or(default_color(0), |index| table_color(self.colors.as_deref(), index))
    }

    fn level_name(&self, level: u32) -> String { self.levels.get(&level).cloned().unwrap_or_else(|| format!("Level {level}")) }

    fn definition(&self, name: &str) -> Option<(usize, usize)> {
        self.definitions.get(name).copied()
            .or_else(|| self.definitions.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, &range)| range))
    }
}

/// Global origin, metres per UOR and a description of the units from the model header (B.5):
/// zlib data of 4096 bytes, a 4-byte prefix and an element of type 66.
fn model_units(packed: Option<&[u8]>) -> Option<(DVec3, f64, String)> {
    let mut unpacked = Vec::new();
    inflate(packed?, &mut unpacked).ok()?;
    let e = V8Element { bytes: unpacked.get(0x1004..)? };
    if e.kind() != 66 { return None; }
    // Units per metre are stored as a numerator and a denominator.
    let per_metre = |at: usize| e.f64(at) / e.f64(at + 8);
    let uor_per_storage = e.f64(0xE0);
    let metres_per_uor = 1.0 / (uor_per_storage * per_metre(0xE8));
    let origin = DVec3::new(e.f64(0x70), e.f64(0x78), e.f64(0x80));
    if !(metres_per_uor.is_finite() && metres_per_uor > 0.0 && origin.is_finite()) { return None; }
    let units = format!(
        "1 master unit = {} m, 1 sub unit = {} m, 1 UOR = {metres_per_uor} m ({uor_per_storage} UOR per storage unit of {} m)",
        1.0 / per_metre(0x50), 1.0 / per_metre(0x60), 1.0 / per_metre(0xE8),
    );
    Some((origin, metres_per_uor, units))
}

fn read_v8(data: &[u8]) -> Result<Loaded, String> {
    let file = Compound::open(data)?;
    let mut scene = Scene::default();
    let model = file.path(&["Dgn-Md", "#000000"]).ok_or("MicroStation DGN: this V8 file has no design model (Dgn-Md/#000000)")?;
    // Only the first model is described yet: sheets and further design models are counted.
    if let Some(models) = file.path(&["Dgn-Md"]) {
        for k in file.children(models) {
            if k != model && file.entries[k].kind == 1 { scene.omit("Further model (not read)"); }
        }
    }
    let graphic_bytes = element_stream(&file, file.child(model, "Dgn^G"), &mut scene);
    let table_bytes = element_stream(&file, file.path(&["Dgn^Nm"]), &mut scene);
    let header = file.child(model, "Dgn~Mh").map(|k| file.read(k));
    let graphics = v8_elements(&graphic_bytes, &mut scene)?;
    let tables = v8_elements(&table_bytes, &mut scene)?;
    let design = V8Design::new(header.as_deref(), &tables, &graphics);
    let mut converter = V8Converter {
        design: &design, tables: &tables, scene, placement: (DMat3::IDENTITY, DVec3::ZERO), instance_level: None, active: Vec::new(), visits: 0,
    };
    let mut i = 0;
    while i < graphics.len() {
        i = converter.top_level(&graphics, i);
    }
    converter.scene.finish(&Summary {
        version: format!("V8, {}", if design.three_d { "3D" } else { "2D" }),
        three_d: design.three_d,
        units: design.units.clone(),
        metres_per_uor: design.metres_per_uor,
        origin: design.origin,
        level_names: design.levels.clone(),
    })
}

struct V8Converter<'a> {
    design: &'a V8Design,
    /// The non-model elements, which hold the shared cell definitions.
    tables: &'a [V8Element<'a>],
    scene: Scene,
    /// Placement of the shared cell definition being drawn: design point = matrix · point + offset.
    placement: (DMat3, DVec3),
    /// The level of the shared cell being drawn, for definition components on level 0.
    instance_level: Option<u32>,
    /// The shared cell definitions being drawn, to stop a definition that holds itself.
    active: Vec<usize>,
    visits: usize,
}

impl V8Converter<'_> {
    fn omit(&mut self, what: &str) { self.scene.omit(what); }

    fn level(&self, e: &V8Element) -> u32 {
        match (e.level(), self.instance_level) { (0, Some(level)) => level, (level, _) => level }
    }

    /// Converts the top-level element at `i` with its components; returns the next element.
    fn top_level(&mut self, all: &[V8Element], i: usize) -> usize {
        let e = all[i];
        let next = v8_span(all, i, all.len(), 0);
        if !e.graphic() { return next; }
        let id = self.scene.new_id();
        let before = self.scene.prims.len();
        let mut texts = Vec::new();
        self.draw(all, i, all.len(), id, 0, &mut texts);
        if self.scene.prims.len() == before { return next; }
        let level = e.level();
        let mut props = serde_json::json!({
            "DGN element": kind_name(e.kind()), "Level": self.design.level_name(level), "Level ID": level,
            "Colour index": e.color(), "Line weight": e.weight(), "Line style": e.style(), "Class": e.class(),
            "Element ID": le64(e.bytes, 0x10),
        });
        if e.u32(0x20) != 0 { props["Graphic group"] = e.u32(0x20).into(); }
        if matches!(e.kind(), 2 | 35) { if let Some(name) = e.name() { props["Cell"] = name.into(); } }
        if !texts.is_empty() { props["Text"] = texts.join("\n").into(); }
        self.scene.record(id, e.kind(), level, props);
        next
    }

    fn push(&mut self, e: &V8Element, id: u64, shape: Shape) {
        let (design, (matrix, offset)) = (self.design, self.placement);
        let level = self.level(e);
        self.scene.push(id, level, design.rgb(e.color()), e.weight(), shape, |p| design.world(matrix * p + offset));
    }

    /// Draws element `i` (and its components, which end at most at `end`) as part of pickable
    /// element `id`.
    fn draw(&mut self, all: &[V8Element], i: usize, end: usize, id: u64, depth: usize, texts: &mut Vec<String>) {
        let e = all[i];
        if depth > MAX_DEPTH { self.omit("Too deeply nested element"); return; }
        if !e.graphic() || self.scene.full() { return; }
        self.visits += 1;
        if self.visits > MAX_VISITS {
            if self.visits == MAX_VISITS + 1 { self.omit("Element limit exceeded (rest skipped)"); }
            return;
        }
        let stop = v8_span(all, i, end, depth);
        let filled = e.three_d() && !e.h_bit();
        match e.kind() {
            2 | 7 | 12 => {
                // Cells keep their components in world coordinates; text nodes hold text elements.
                let mut j = i + 1;
                while j < stop {
                    self.draw(all, j, stop, id, depth + 1, texts);
                    j = v8_span(all, j, stop, depth + 1);
                }
            }
            14 => {
                let mut outline = Vec::new();
                let mut j = i + 1;
                while j < stop {
                    let next = v8_span(all, j, stop, depth + 1);
                    match v8_outline(all, j, next) {
                        Some((points, _)) => outline.extend(points),
                        None => self.omit("Unsupported complex shape component"),
                    }
                    j = next;
                }
                self.push(&e, id, closed_shape(outline, filled));
            }
            3 | 4 | 6 | 11 | 15 | 16 | 27 => match v8_outline(all, i, stop) {
                Some((points, true)) if e.kind() != 3 => self.push(&e, id, closed_shape(points, filled)),
                Some((points, _)) => self.push(&e, id, Shape::Stroke(points)),
                None => self.omit("Malformed element"),
            },
            17 => self.text(&e, id, texts),
            22 => {
                let points = e.vertices();
                if e.h_bit() { for p in points { self.push(&e, id, Shape::Stroke(vec![p])); } } else { self.push(&e, id, Shape::Stroke(points)); }
            }
            35 => self.shared_cell(&e, id, depth, texts),
            kind => self.omit(&kind_label(kind)),
        }
    }

    /// Text (B.4, B.8): its box from the origin (the lower left) and the size multipliers as an
    /// outline, the characters in the element's properties.
    fn text(&mut self, e: &V8Element, id: u64, texts: &mut Vec<String>) {
        let count = usize::from(e.u16(0x6E));
        let (origin_at, start) = if e.three_d() { (0xB0, 0xCA) } else { (0x98, 0xAA) };
        let Some(bytes) = e.bytes.get(start..start + count) else { self.omit("Malformed element"); return };
        let text = v8_text(bytes);
        let characters = text.as_ref().map_or(count, |t| t.chars().count()).max(1);
        // As in V7: the basic character cell is 6 UOR and the multipliers are thousandths.
        let (width, height) = (e.f64(0x70) / 1000.0 * 6.0 * characters as f64, e.f64(0x78) / 1000.0 * 6.0);
        self.push(e, id, Shape::Stroke(text_box(e.point(origin_at), e.rotation(0x90), width, height)));
        if let Some(text) = text.filter(|t| !t.trim().is_empty()) { texts.push(text); }
    }

    /// A shared cell: the components of its definition (among the non-model elements), placed by
    /// the instance's matrix (stored row by row) and origin.
    fn shared_cell(&mut self, e: &V8Element, id: u64, depth: usize, texts: &mut Vec<String>) {
        if e.len() < 0x100 { self.omit("Malformed element"); return; }
        let Some((first, stop)) = e.name().and_then(|name| self.design.definition(&name)) else {
            self.omit("Shared cell without definition");
            return;
        };
        if self.active.contains(&first) { self.omit("Shared cell that contains itself"); return; }
        let rows: [f64; 9] = std::array::from_fn(|k| e.f64(0xA0 + 8 * k));
        let matrix = DMat3::from_cols_array(&rows).transpose();
        let origin = DVec3::new(e.f64(0xE8), e.f64(0xF0), e.f64(0xF8));
        if !matrix.is_finite() || !origin.is_finite() { self.omit("Malformed element"); return; }
        let saved = (self.placement, self.instance_level);
        let (outer, offset) = self.placement;
        self.placement = (outer * matrix, outer * origin + offset);
        self.instance_level = Some(self.level(e));
        self.active.push(first);
        let tables = self.tables;
        let mut j = first + 1;
        while j < stop {
            self.draw(tables, j, stop, id, depth + 1, texts);
            j = v8_span(tables, j, stop, depth + 1);
        }
        self.active.pop();
        (self.placement, self.instance_level) = saved;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Writes design files from the ISFF element layouts.
    struct Writer {
        words: Vec<u16>,
        three_d: bool,
    }

    fn long(v: i32) -> [u16; 2] { [((v as u32) >> 16) as u16, v as u32 as u16] }

    fn dfloat(v: f64) -> [u16; 4] {
        if v == 0.0 { return [0; 4]; }
        let bits = v.to_bits();
        let exponent = ((bits >> 52) & 0x7FF) - 894;
        let u = (bits & (1 << 63)) | (exponent << 55) | ((bits & ((1 << 52) - 1)) << 3);
        [(u >> 48) as u16, (u >> 32) as u16, (u >> 16) as u16, u as u16]
    }

    impl Writer {
        /// A design file header: `master` and `sub` unit names, subunits per master, UOR per subunit.
        fn new(three_d: bool, master: &str, sub: &str, sub_per_master: i32, uor_per_sub: i32, origin: [f64; 3]) -> Self {
            let mut tcb = vec![0u16; 768];
            tcb[0] = 0x0908 | if three_d { 0xC0 } else { 0 };
            tcb[1] = 766;
            tcb[556..558].copy_from_slice(&long(sub_per_master));
            tcb[558..560].copy_from_slice(&long(uor_per_sub));
            let name = |s: &str| { let b = s.as_bytes(); u16::from(*b.first().unwrap_or(&0)) | (u16::from(*b.get(1).unwrap_or(&0)) << 8) };
            tcb[560] = name(master);
            tcb[561] = name(sub);
            for (k, &v) in origin.iter().enumerate() { tcb[620 + 4 * k..624 + 4 * k].copy_from_slice(&dfloat(v)); }
            Writer { words: tcb, three_d }
        }

        /// An element: header (range left zero), display header, then `body`.
        fn element(&mut self, kind: u8, level: u8, color: u8, weight: u8, flags: u16, body: &[u16]) -> &mut Self {
            let complex = flags & 0x80;
            let mut e = vec![u16::from(kind) << 8 | u16::from(level) | complex, 0];
            e.extend([0u16; 12]);
            e.extend([0, 0, flags & 0xFF0F, u16::from(color) << 8 | u16::from(weight) << 3]);
            e.extend_from_slice(body);
            e[1] = (e.len() - 2) as u16;
            e[15] = (e.len() - 16) as u16;
            self.words.extend(e);
            self
        }

        fn points(&self, points: &[[i32; 3]]) -> Vec<u16> {
            points.iter().flat_map(|p| p[..if self.three_d { 3 } else { 2 }].iter().flat_map(|&v| long(v)).collect::<Vec<_>>()).collect()
        }

        fn bytes(&self) -> Vec<u8> {
            self.words.iter().chain([0xFFFF].iter()).flat_map(|w| w.to_le_bytes()).collect()
        }
    }

    fn load(bytes: Vec<u8>) -> Result<Loaded, String> {
        DgnLoader.load_with(&Source::Bytes { name: "drawing.dgn".into(), data: Arc::new(bytes) }, &Progress::default())
    }

    fn info<'a>(loaded: &'a Loaded, key: &str) -> &'a str {
        &loaded.project.info.iter().find(|(k, _)| k == key).unwrap().1
    }

    fn bounds(layer: &Layer) -> ([f32; 3], [f32; 3]) {
        layer.positions.chunks_exact(3).fold(([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]), |(mut lo, mut hi), p| {
            for k in 0..3 { lo[k] = lo[k].min(p[k]); hi[k] = hi[k].max(p[k]); }
            (lo, hi)
        })
    }

    #[test]
    fn numbers_decode_as_documented() {
        let mut bytes = Vec::new();
        for w in long(-123_456).iter().chain(dfloat(1.0).iter()).chain(dfloat(-2.5e6).iter()) { bytes.extend(w.to_le_bytes()); }
        let e = Element { kind: 3, level: 1, complex: false, offset: 0, bytes: &bytes };
        assert_eq!(e.long(0), -123_456);
        assert_eq!(e.word(2), 0x4080, "1.0 is 4080 0000 0000 0000 in D-float");
        assert_eq!(e.double(2), 1.0);
        assert_eq!(e.double(6), -2.5e6);
        assert_eq!(radix50([1683, 0]), "ABC");
    }

    #[test]
    fn reads_a_2d_drawing_in_metres_by_level_with_colours_and_text() {
        // 1 FT = 10 TH = 1000 UOR, origin shifted like a State Plane drawing.
        let mut w = Writer::new(false, "FT", "TH", 10, 100, [-1000.0, -2000.0, 0.0]);
        let mut table = vec![0u16];
        let mut rgb = vec![[0u8, 0, 0]; 256];
        rgb[4] = [200, 10, 20];
        let flat: Vec<u8> = rgb.concat();
        table.extend(flat.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])));
        w.element(5, 1, 0, 0, 0, &table);
        w.element(3, 5, 3, 2, 0, &w.points(&[[-1000, -2000, 0], [9000, -2000, 0]]));
        let mut string = vec![3];
        string.extend(w.points(&[[0, 0, 0], [1000, 0, 0], [1000, 1000, 0]]));
        w.element(4, 6, 0, 0, 0, &string);
        // A deleted line far away must not count.
        let far = w.points(&[[90_000_000, 0, 0], [91_000_000, 0, 0]]);
        w.element(3, 6, 0, 0, 0, &far);
        let deleted = w.words.len() - far.len() - 18;
        w.words[deleted] |= 0x8000;
        // Text "AB" of height 6 * 500 / 1000 * ... = 300 UOR at (0, 3000).
        let mut text = vec![0x0000];
        text.extend(long(50_000));
        text.extend(long(50_000));
        text.extend(long(0));
        text.extend(w.points(&[[0, 3000, 0]]));
        text.extend([2, u16::from_le_bytes(*b"AB")]);
        w.element(17, 7, 0, 0, 0, &text);
        let loaded = load(w.bytes()).unwrap();
        assert_eq!(loaded.layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Level 5", "Level 6", "Level 7"]);
        assert!(info(&loaded, "DGN version").contains("2D"));
        assert_eq!(info(&loaded, "DGN units"), "1 FT = 10 TH = 1000 UOR");
        assert!(info(&loaded, "DGN elements").contains("Line: 1"));
        // The line from the global origin runs 10 ft east: 3.048 m plus the square ends.
        let (lo, hi) = bounds(&loaded.layers[0]);
        assert!((hi[0] - lo[0] - 3.048).abs() < 0.02, "{lo:?} {hi:?}");
        assert!(lo[2] == 0.0 && hi[2] == 0.0);
        // Colour 3 is slot 4 of the stored table.
        assert_eq!(loaded.layers[0].colors.as_ref().unwrap()[0], [200, 10, 20, 255]);
        assert_eq!(loaded.layers[0].elements.len(), 1);
        // Origin (world) is the first point, which is the global origin: (0, 0) in metres.
        assert_eq!(info(&loaded, "DGN world origin (metres)"), "0, 0, 0");
        let (lo, hi) = bounds(&loaded.layers[1]);
        assert!(hi[0] < 4.0 && lo[0] > 0.0, "deleted line is skipped: {lo:?} {hi:?}");
        let text_layer = &loaded.layers[2];
        assert!(text_layer.props[0].1.contains("\"Text\":\"AB\""), "{}", text_layer.props[0].1);
        let (lo, hi) = bounds(text_layer);
        // Box 2 characters x 300 UOR wide, 300 UOR high: 0.18 by 0.09 m, above y = (3000 + 2000) UOR.
        assert!((hi[0] - lo[0] - 0.18).abs() < 0.02 && (hi[1] - lo[1] - 0.09).abs() < 0.02, "{lo:?} {hi:?}");
        assert!((lo[1] - 1.524).abs() < 0.02, "{lo:?}");
    }

    #[test]
    fn arcs_ellipses_and_curves_follow_their_geometry() {
        let mut w = Writer::new(false, "M", "MM", 1000, 1, [0.0; 3]);
        // A quarter arc of radius 1 m around (0, 0), from 0 to 90 degrees.
        let mut arc = Vec::new();
        arc.extend(long(0));
        arc.extend(long(90 * 360_000));
        arc.extend(dfloat(1000.0));
        arc.extend(dfloat(1000.0));
        arc.extend(long(0));
        arc.extend(dfloat(0.0));
        arc.extend(dfloat(0.0));
        w.element(16, 1, 1, 0, 0, &arc);
        // An ellipse 4 x 2 m turned 90 degrees, around (10, 0).
        let mut ellipse = Vec::new();
        ellipse.extend(dfloat(2000.0));
        ellipse.extend(dfloat(1000.0));
        ellipse.extend(long(90 * 360_000));
        ellipse.extend(dfloat(10_000.0));
        ellipse.extend(dfloat(0.0));
        w.element(15, 2, 1, 0, 0, &ellipse);
        let loaded = load(w.bytes()).unwrap();
        // The world origin is the arc's first point (1, 0): the arc spans x -1..0 and y 0..1.
        let (lo, hi) = bounds(&loaded.layers[0]);
        assert!((lo[0] + 1.0).abs() < 0.02 && hi[0].abs() < 0.02 && lo[1].abs() < 0.02 && (hi[1] - 1.0).abs() < 0.02, "{lo:?} {hi:?}");
        let (lo, hi) = bounds(&loaded.layers[1]);
        assert!((hi[0] - lo[0] - 2.0).abs() < 0.02 && (hi[1] - lo[1] - 4.0).abs() < 0.02, "{lo:?} {hi:?}");
        assert!(((hi[0] + lo[0]) / 2.0 - 9.0).abs() < 0.01);

        let curve = akima(&(0..8).map(|k| DVec3::new(k as f64, (k as f64 * 0.7).sin(), 0.0)).collect::<Vec<_>>());
        assert_eq!(curve.first().unwrap().x, 2.0);
        assert!((curve.last().unwrap().x - 5.0).abs() < 1e-9);
    }

    #[test]
    fn cells_and_complex_shapes_are_one_element_each() {
        let mut w = Writer::new(false, "M", "MM", 1000, 1, [0.0; 3]);
        let line = w.points(&[[0, 0, 0], [1000, 0, 0]]);
        let line2 = w.points(&[[0, 1000, 0], [1000, 1000, 0]]);
        // A cell: totlength counts the header words after word 18 and the components.
        let mut cell = vec![0u16];
        cell.extend(long(0x0693_0000)); // RADIX-50 "ABC"
        cell.extend([0u16; 5 + 8 + 8 + 4]);
        cell[0] = (18 + cell.len() + 2 * (18 + line.len()) - 19) as u16;
        w.element(2, 3, 0, 0, 0, &cell);
        w.element(3, 3, 0, 0, 0x80, &line);
        w.element(3, 4, 2, 0, 0x80, &line2);
        // A complex shape of two line strings.
        let mut shape = vec![0u16, 2, 0, 0, 0, 0];
        let mut a = vec![2u16];
        a.extend(w.points(&[[5000, 0, 0], [6000, 0, 0]]));
        let mut b = vec![3u16];
        b.extend(w.points(&[[6000, 0, 0], [6000, 1000, 0], [5000, 0, 0]]));
        shape[0] = ((18 + shape.len()) + (18 + a.len()) + (18 + b.len()) - 19) as u16;
        w.element(14, 5, 0, 0, 0, &shape);
        w.element(4, 5, 0, 0, 0x80, &a);
        w.element(4, 5, 0, 0, 0x80, &b);
        let loaded = load(w.bytes()).unwrap();
        let ids: BTreeSet<u64> = loaded.layers.iter().flat_map(|l| l.elements.iter().map(|e| e.0)).collect();
        assert_eq!(ids.len(), 2, "{:?}", loaded.layers.iter().map(|l| &l.elements).collect::<Vec<_>>());
        assert_eq!(loaded.layers.len(), 3, "cell components keep their own levels");
        assert!(loaded.layers[0].props[0].1.contains("\"Cell\""));
        assert!(info(&loaded, "DGN elements").contains("Cell: 1") && info(&loaded, "DGN elements").contains("Complex shape: 1"));
    }

    #[test]
    fn solids_cones_and_shapes_of_3d_files_are_faces() {
        let mut w = Writer::new(true, "M", "MM", 1000, 1, [0.0; 3]);
        // A unit cube as a solid of projection: two square boundaries and four rule lines.
        let square = |z: i32| {
            let mut s = vec![5u16];
            s.extend(w.points(&[[0, 0, z], [1000, 0, z], [1000, 1000, z], [0, 1000, z], [0, 0, z]]));
            s
        };
        let (bottom, top) = (square(0), square(1000));
        let rule = w.points(&[[0, 0, 0], [0, 0, 1000]]);
        let mut solid = vec![0u16, 3, 0, 0, 0, 0, 0];
        solid[0] = ((18 + solid.len()) + (18 + bottom.len()) + (18 + top.len()) + (18 + rule.len()) - 19) as u16;
        w.element(19, 2, 0, 0, 0, &solid);
        w.element(6, 2, 0, 0, 0x80, &bottom);
        w.element(6, 2, 0, 0, 0x80, &top);
        w.element(3, 2, 0, 0, 0x80 | 4, &rule);
        // A cylinder of radius 1 m and height 2 m with an identity quaternion, beside it.
        let mut cone = vec![0u16];
        cone.extend(long(i32::MAX));
        cone.extend([0u16; 6]);
        for v in [5000.0, 0.0, 0.0, 1000.0, 5000.0, 0.0, 2000.0, 1000.0] { cone.extend(dfloat(v)); }
        w.element(23, 3, 0, 0, 0, &cone);
        let loaded = load(w.bytes()).unwrap();
        let cube = &loaded.layers[0];
        assert_eq!(cube.triangle_count(), 12);
        assert!(!cube.solid_indices().is_empty(), "the capped solid encloses volume");
        let (lo, hi) = bounds(&loaded.layers[1]);
        assert!((hi[2] - lo[2] - 2.0).abs() < 1e-4 && (hi[0] - lo[0] - 2.0).abs() < 1e-3, "{lo:?} {hi:?}");
        assert!(info(&loaded, "DGN version").contains("3D"));
    }

    #[test]
    fn rotations_of_3d_elements_use_the_stored_quaternion() {
        let mut w = Writer::new(true, "M", "MM", 1000, 1, [0.0; 3]);
        // A circle of radius 1 m turned 90 degrees about X lies in the XZ plane.
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let mut ellipse = Vec::new();
        ellipse.extend(dfloat(1000.0));
        ellipse.extend(dfloat(1000.0));
        for q in [s, -s, 0.0, 0.0] { ellipse.extend(long((q * UNIT) as i32)); }
        for _ in 0..3 { ellipse.extend(dfloat(0.0)); }
        w.element(15, 1, 0, 0, 0x8000, &ellipse);
        let loaded = load(w.bytes()).unwrap();
        let (lo, hi) = bounds(&loaded.layers[0]);
        assert!((hi[2] - lo[2] - 2.0).abs() < 0.05 && (hi[1] - lo[1]) < 0.05, "{lo:?} {hi:?}");
    }

    #[test]
    fn bspline_curves_are_clamped_to_their_end_poles() {
        let mut w = Writer::new(false, "M", "MM", 1000, 1, [0.0; 3]);
        let mut header = vec![0u16, 0, 2, 4, 0];
        header[1] = 0;
        w.element(27, 1, 0, 0, 0, &header);
        let mut poles = vec![4u16];
        poles.extend(w.points(&[[0, 0, 0], [1000, 2000, 0], [3000, 2000, 0], [4000, 0, 0]]));
        w.element(21, 1, 0, 0, 0x80, &poles);
        let loaded = load(w.bytes()).unwrap();
        let (lo, hi) = bounds(&loaded.layers[0]);
        assert!((hi[0] - lo[0] - 4.0).abs() < 0.02, "{lo:?} {hi:?}");
        assert!(hi[1] < 1.6 && hi[1] > 1.4, "a cubic stays inside its control polygon: {hi:?}");
        assert_eq!(loaded.layers[0].elements.len(), 1);
    }

    /// Loads every `.dgn` below `$DGN_SAMPLES` (files that are not in the repository) and prints
    /// what came through: `DGN_SAMPLES=dir cargo test --lib dgn -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn samples_from_environment() {
        let Ok(root) = std::env::var("DGN_SAMPLES") else { return };
        let mut stack = vec![std::path::PathBuf::from(root)];
        while let Some(path) = stack.pop() {
            if path.is_dir() {
                stack.extend(std::fs::read_dir(&path).unwrap().map(|entry| entry.unwrap().path()));
            } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("dgn")) {
                match load(std::fs::read(&path).unwrap()) {
                    Ok(loaded) => {
                        let triangles: usize = loaded.layers.iter().map(Layer::triangle_count).sum();
                        println!("{}: {} layers, {triangles} triangles", path.display(), loaded.layers.len());
                        for (key, value) in &loaded.project.info { println!("    {key}: {value}"); }
                    }
                    Err(error) => println!("{}: {error}", path.display()),
                }
            }
        }
    }

    #[test]
    fn broken_v8_and_foreign_files_are_refused_clearly() {
        let mut v8 = OLE_SIGNATURE.to_vec();
        v8.resize(512, 0);
        assert!(load(v8).err().unwrap().contains("V8"));
        let other = compound(&[("WordDocument".into(), vec![1; 100])]);
        assert!(load(other).err().unwrap().contains("no design model"));
        assert!(load(b"not a design file at all".to_vec()).err().unwrap().contains("not a design file"));
        let header_only = Writer::new(false, "M", "MM", 1000, 1, [0.0; 3]).bytes();
        assert!(load(header_only).err().unwrap().contains("no drawable elements"));
    }

    // V8 files, written from the clean-room specification (SPEC_v0.1.md §B) and [MS-CFB].

    /// Writes a compound file (version 3, 512-byte sectors) holding `streams` by path. Streams
    /// under 4096 bytes go into the mini stream; the FAT sectors come last.
    fn compound(streams: &[(String, Vec<u8>)]) -> Vec<u8> {
        const FREE: u32 = 0xFFFF_FFFF;
        struct Node { name: String, kind: u8, children: Vec<usize>, data: Vec<u8>, start: u32 }
        /// Appends `bytes` as a chain of `size`-byte sectors.
        fn chain_of(table: &mut Vec<u32>, sectors: &mut Vec<u8>, bytes: &[u8], size: usize) -> u32 {
            if bytes.is_empty() { return END_OF_CHAIN; }
            let first = table.len() as u32;
            let count = bytes.len().div_ceil(size);
            for k in 0..count { table.push(if k + 1 == count { END_OF_CHAIN } else { first + k as u32 + 1 }); }
            sectors.extend_from_slice(bytes);
            sectors.resize(table.len() * size, 0);
            first
        }
        /// Siblings as a binary tree through left and right; returns the middle one.
        fn tree(list: &[usize], links: &mut [(u32, u32)]) -> u32 {
            if list.is_empty() { return FREE; }
            let middle = list.len() / 2;
            links[list[middle]] = (tree(&list[..middle], links), tree(&list[middle + 1..], links));
            list[middle] as u32
        }
        let mut nodes = vec![Node { name: "Root Entry".into(), kind: 5, children: Vec::new(), data: Vec::new(), start: END_OF_CHAIN }];
        for (path, data) in streams {
            let parts: Vec<&str> = path.split('/').collect();
            let mut parent = 0;
            for (k, part) in parts.iter().enumerate() {
                let stream = k + 1 == parts.len();
                parent = match nodes[parent].children.iter().copied().find(|&c| nodes[c].name == *part) {
                    Some(child) => child,
                    None => {
                        let data = if stream { data.clone() } else { Vec::new() };
                        nodes.push(Node { name: part.to_string(), kind: if stream { 2 } else { 1 }, children: Vec::new(), data, start: END_OF_CHAIN });
                        let child = nodes.len() - 1;
                        nodes[parent].children.push(child);
                        child
                    }
                };
            }
        }
        let (mut mini, mut mini_fat, mut sectors, mut fat) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for node in nodes.iter_mut().filter(|n| n.kind == 2) {
            node.start = if node.data.len() < 4096 { chain_of(&mut mini_fat, &mut mini, &node.data, 64) } else { chain_of(&mut fat, &mut sectors, &node.data, 512) };
        }
        let mini_fat_bytes: Vec<u8> = mini_fat.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mini_fat_start = chain_of(&mut fat, &mut sectors, &mini_fat_bytes, 512);
        nodes[0].start = chain_of(&mut fat, &mut sectors, &mini, 512);
        let mut links = vec![(FREE, FREE); nodes.len()];
        let children: Vec<u32> = nodes.iter().map(|n| tree(&n.children, &mut links)).collect();
        let mut directory = Vec::new();
        for (k, node) in nodes.iter().enumerate() {
            let mut entry = [0u8; 128];
            let name: Vec<u16> = node.name.encode_utf16().collect();
            for (j, c) in name.iter().enumerate() { entry[2 * j..2 * j + 2].copy_from_slice(&c.to_le_bytes()); }
            entry[0x40..0x42].copy_from_slice(&(2 * (name.len() as u16 + 1)).to_le_bytes());
            entry[0x42] = node.kind;
            entry[0x43] = 1;
            for (at, value) in [(0x44, links[k].0), (0x48, links[k].1), (0x4C, children[k]), (0x74, node.start)] {
                entry[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
            let size = if node.kind == 5 { mini.len() } else { node.data.len() };
            entry[0x78..0x80].copy_from_slice(&(size as u64).to_le_bytes());
            directory.extend(entry);
        }
        let directory_start = chain_of(&mut fat, &mut sectors, &directory, 512);
        // The FAT describes all sectors, its own included.
        let mut fat_count = 1;
        while fat_count * 128 < fat.len() + fat_count { fat_count += 1; }
        assert!(fat_count <= 109);
        let first_fat = fat.len() as u32;
        fat.extend(std::iter::repeat_n(0xFFFF_FFFD, fat_count));
        fat.resize(fat_count * 128, FREE);
        let mut out = vec![0u8; 512];
        out[..8].copy_from_slice(&OLE_SIGNATURE);
        for (at, value) in [(0x18, 0x3Eu16), (0x1A, 3), (0x1C, 0xFFFE), (0x1E, 9), (0x20, 6)] { out[at..at + 2].copy_from_slice(&value.to_le_bytes()); }
        let header = [
            (0x2C, fat_count as u32), (0x30, directory_start), (0x38, 4096), (0x3C, mini_fat_start),
            (0x40, mini_fat_bytes.len().div_ceil(512) as u32), (0x44, END_OF_CHAIN), (0x48, 0),
        ];
        for (at, value) in header { out[at..at + 4].copy_from_slice(&value.to_le_bytes()); }
        for k in 0..109 {
            let value = if k < fat_count { first_fat + k as u32 } else { FREE };
            out[0x4C + 4 * k..0x50 + 4 * k].copy_from_slice(&value.to_le_bytes());
        }
        out.extend(sectors);
        out.extend(fat.iter().flat_map(|s| s.to_le_bytes()));
        out
    }

    const D3: u32 = 0x0800;
    const HOLE: u32 = 0x8000;

    /// A V8 element (B.3): the header, a display header for graphic elements (`display`:
    /// properties, colour, weight), the data from +0x68 (or +0x20), then string linkages (B.7).
    fn v8(kind: u16, flags: u16, level: u32, display: Option<[u32; 3]>, data: &[u8], strings: &[(u32, &str)]) -> Vec<u8> {
        let mut e = Vec::new();
        e.extend(kind.to_le_bytes());
        e.extend(flags.to_le_bytes());
        e.extend([0; 8]);
        e.extend(level.to_le_bytes());
        e.extend(0x1234u64.to_le_bytes());
        e.extend(0f64.to_le_bytes());
        if let Some([properties, color, weight]) = display {
            for value in [0, 0x8000_0000, properties, 0, weight, color] { e.extend(value.to_le_bytes()); }
            e.extend([0; 48]);
        }
        e.extend_from_slice(data);
        if e.len() % 2 == 1 { e.push(0); }
        let attindx = (e.len() / 2) as u32;
        for (key, text) in strings {
            let mut linkage = vec![0, 0, 0xD2, 0x56];
            linkage.extend(key.to_le_bytes());
            linkage.extend((text.len() as u32).to_le_bytes());
            linkage.extend(text.as_bytes());
            if linkage.len() % 2 == 1 { linkage.push(0); }
            let words = (linkage.len() / 2) as u16;
            linkage[..2].copy_from_slice(&(0x1000 | words).to_le_bytes());
            e.extend(linkage);
        }
        let words = (e.len() / 2) as u32;
        e[4..8].copy_from_slice(&words.to_le_bytes());
        e[8..12].copy_from_slice(&attindx.to_le_bytes());
        e
    }

    fn shown(properties: u32, color: u32) -> Option<[u32; 3]> { Some([properties, color, 0]) }

    fn doubles(values: &[f64]) -> Vec<u8> { values.iter().flat_map(|v| v.to_le_bytes()).collect() }

    /// A count, four bytes of padding and the coordinates of the points.
    fn counted(count: u32, coordinates: &[f64]) -> Vec<u8> {
        let mut data = count.to_le_bytes().to_vec();
        data.extend([0; 4]);
        data.extend(doubles(coordinates));
        data
    }

    fn level_entry(id: u32, name: &str) -> Vec<u8> {
        let mut data = id.to_le_bytes().to_vec();
        data.extend((-1i32).to_le_bytes());
        v8(95, 0x0400, 1, None, &data, &[(1, name)])
    }

    fn color_entry(slots: &[(usize, [u8; 3])]) -> Vec<u8> {
        let mut data = vec![0u8; 2 + 768];
        for &(slot, rgb) in slots { data[2 + 3 * slot..5 + 3 * slot].copy_from_slice(&rgb); }
        v8(5, 0x0400, 1, None, &data, &[])
    }

    /// Text data (B.4) from +0x68: 2D, at `origin`, with `bytes` as its characters.
    fn text_data(size: f64, origin: [f64; 2], bytes: &[u8]) -> Vec<u8> {
        let mut data = 0u32.to_le_bytes().to_vec();
        data.extend(0u16.to_le_bytes());
        data.extend((bytes.len() as u16).to_le_bytes());
        data.extend(doubles(&[size, size, 0.0, 0.0, 0.0, origin[0], origin[1]]));
        data.extend(0u16.to_le_bytes());
        data.extend(bytes);
        data
    }

    /// The header of a shared cell or its definition (B.4): range, row-major matrix, origin.
    fn shared_data(matrix: [f64; 9], origin: [f64; 3]) -> Vec<u8> {
        let mut data = vec![0u8; 8];
        data.extend(doubles(&[0.0; 6]));
        data.extend(doubles(&matrix));
        data.extend(doubles(&origin));
        data
    }

    fn zlib(data: &[u8], level: u32) -> Vec<u8> {
        use std::io::Write;
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(level));
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    /// A V8 design file: the model header, the graphic elements of the model, the non-model
    /// elements and any further streams.
    struct V8File {
        header: Vec<u8>,
        graphics: Vec<u8>,
        tables: Vec<u8>,
        extra: Vec<(String, Vec<u8>)>,
    }

    impl V8File {
        /// `uor` UOR per storage unit, `numerator / denominator` storage units per metre, the
        /// global origin in UOR (B.5).
        fn new(uor: f64, numerator: f64, denominator: f64, origin: [f64; 3]) -> Self {
            let mut e = vec![0u8; 0x100];
            e[..2].copy_from_slice(&66u16.to_le_bytes());
            e[2..4].copy_from_slice(&0x0400u16.to_le_bytes());
            e[4..8].copy_from_slice(&0x80u32.to_le_bytes());
            e[8..12].copy_from_slice(&0x80u32.to_le_bytes());
            let fields = [
                (0x50, 1.0), (0x58, 1.0), (0x60, 1000.0), (0x68, 1.0), (0x70, origin[0]), (0x78, origin[1]), (0x80, origin[2]),
                (0xE0, uor), (0xE8, numerator), (0xF0, denominator),
            ];
            for (at, value) in fields { e[at..at + 8].copy_from_slice(&f64::to_le_bytes(value)); }
            let mut plain = vec![0u8; 4096 + 4];
            plain.extend(e);
            V8File { header: zlib(&plain, 6), graphics: Vec::new(), tables: Vec::new(), extra: Vec::new() }
        }

        fn graphic(&mut self, element: Vec<u8>) -> &mut Self {
            self.graphics.extend([0; 4]);
            self.graphics.extend(element);
            self
        }

        fn table(&mut self, element: Vec<u8>) -> &mut Self {
            self.tables.extend([0; 4]);
            self.tables.extend(element);
            self
        }

        /// The compound file. The graphic elements are cut at arbitrary bytes into `$1` (stored,
        /// up to 5000 bytes, so it can outgrow the mini stream) and `$2` to `$11` (compressed):
        /// elements run across stream boundaries and `$10` must follow `$9`.
        fn bytes(&self) -> Vec<u8> {
            let block = |data: &[u8], level: u32| {
                let mut block: Vec<u8> = [1u32, 3, 1, 1].iter().flat_map(|v| v.to_le_bytes()).collect();
                block.extend(zlib(data, level));
                block
            };
            let first = (self.graphics.len() / 2).min(5000);
            let rest = &self.graphics[first..];
            let size = rest.len().div_ceil(10);
            let mut streams = vec![("Dgn-Md/#000000/Dgn^G/$1".to_string(), block(&self.graphics[..first], 0))];
            for k in 0..10 {
                let part = &rest[(k * size).min(rest.len())..((k + 1) * size).min(rest.len())];
                streams.push((format!("Dgn-Md/#000000/Dgn^G/${}", k + 2), block(part, 6)));
            }
            streams.push(("Dgn^Nm/$1".into(), block(&self.tables, 6)));
            streams.push(("Dgn-Md/#000000/Dgn~Mh".into(), self.header.clone()));
            streams.push(("Dgn-Md/#000000/Dgn^C/$1".into(), vec![0; 16]));
            streams.push(("Dgn~H".into(), vec![0; 80]));
            streams.extend(self.extra.iter().cloned());
            compound(&streams)
        }
    }

    fn element_bounds(layer: &Layer, k: usize) -> ([f32; 3], [f32; 3]) {
        let (_, first, count) = layer.elements[k];
        layer.indices[first as usize..(first + count) as usize].iter().fold(([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]), |(mut lo, mut hi), &i| {
            for a in 0..3 {
                lo[a] = lo[a].min(layer.positions[3 * i as usize + a]);
                hi[a] = hi[a].max(layer.positions[3 * i as usize + a]);
            }
            (lo, hi)
        })
    }

    #[test]
    fn v8_files_open_with_units_levels_and_colours() {
        // 1000 UOR per metre; the global origin is (-1000, -2000, 0) UOR.
        let mut f = V8File::new(1000.0, 1.0, 1.0, [-1000.0, -2000.0, 0.0]);
        f.table(level_entry(64, "Default")).table(level_entry(70, "Walls")).table(color_entry(&[(4, [200, 10, 20])]));
        // A 2D line 10 m long from the global origin, colour 3, on "Walls".
        f.graphic(v8(3, 0x1000, 70, shown(0, 3), &doubles(&[-1000.0, -2000.0, 9000.0, -2000.0]), &[]));
        // A non-graphic element among the graphics is passed over.
        f.graphic(v8(66, 0x0400, 9, None, &[0; 16], &[]));
        // A long 3D line string, which runs across several streams, on "Default".
        let coordinates: Vec<f64> = (0..400).flat_map(|k| [f64::from(k) * 10.0, 5000.0, f64::from(k % 2) * 10.0]).collect();
        f.graphic(v8(4, 0x1000, 64, shown(D3, 1), &counted(400, &coordinates), &[]));
        // A line on a level without a name.
        f.graphic(v8(3, 0x1000, 99, shown(0, 0), &doubles(&[0.0, 0.0, 0.0, 1000.0]), &[]));
        let loaded = load(f.bytes()).unwrap();
        assert_eq!(loaded.layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Default", "Walls", "Level 99"]);
        assert_eq!(info(&loaded, "DGN version"), "V8, 3D");
        assert!(info(&loaded, "DGN units").contains("1 UOR = 0.001 m"), "{}", info(&loaded, "DGN units"));
        assert_eq!(info(&loaded, "DGN world origin (metres)"), "0, 0, 0");
        assert_eq!(info(&loaded, "DGN elements"), "Line: 2, Line string: 1");
        assert_eq!(info(&loaded, "DGN skipped elements"), "0");
        let walls = &loaded.layers[1];
        let (lo, hi) = bounds(walls);
        assert!((hi[0] - lo[0] - 10.0).abs() < 0.05 && lo[0] < 0.0, "{lo:?} {hi:?}");
        // Colour 3 is slot 4 of the stored table.
        assert_eq!(walls.colors.as_ref().unwrap()[0], [200, 10, 20, 255]);
        assert!(walls.props[0].1.contains("\"Level\":\"Walls\""), "{}", walls.props[0].1);
        // The line string is 3.99 m long at y = 7 m (5000 + 2000 UOR).
        let (lo, hi) = bounds(&loaded.layers[0]);
        assert!((hi[0] - lo[0] - 3.99).abs() < 0.05 && ((lo[1] + hi[1]) / 2.0 - 7.0).abs() < 0.01, "{lo:?} {hi:?}");
    }

    #[test]
    fn v8_shapes_arcs_text_cells_and_skipped_elements() {
        let mut f = V8File::new(1000.0, 1.0, 1.0, [0.0; 3]);
        let square: Vec<f64> = [[0.0, 0.0], [1000.0, 0.0], [1000.0, 1000.0], [0.0, 1000.0], [0.0, 0.0]].iter().flat_map(|p| [p[0], p[1], 0.0]).collect();
        // A closed 3D shape is a face; a hole is its outline.
        f.graphic(v8(6, 0x1000, 1, shown(D3, 0), &counted(5, &square), &[]));
        f.graphic(v8(6, 0x1000, 2, shown(D3 | HOLE, 0), &counted(5, &square), &[]));
        // A quarter arc (2D) of radius 1 m around (10, 0) m, from 0 to 90 degrees.
        f.graphic(v8(16, 0x1000, 3, shown(0, 0), &doubles(&[0.0, std::f64::consts::FRAC_PI_2, 1000.0, 1000.0, 0.0, 10_000.0, 0.0]), &[]));
        // A 3D ellipse of 4 x 2 m with the identity quaternion: a filled disc.
        f.graphic(v8(15, 0x1000, 4, shown(D3, 0), &doubles(&[2000.0, 1000.0, 1.0, 0.0, 0.0, 0.0, 20_000.0, 0.0, 0.0]), &[]));
        // Text in Latin-1 after the marker, characters of 300 UOR.
        let mut characters = vec![0xFF, 0xFE, 0x01, 0x00];
        characters.extend(b"myT\xE9xt");
        f.graphic(v8(17, 0x1000, 5, shown(0, 0), &text_data(50_000.0, [0.0, 5000.0], &characters), &[]));
        // A cell of two lines on levels 6 and 7.
        f.graphic(v8(2, 0x3000, 6, shown(0, 0), &[2, 0, 0, 0, 0, 0, 0, 0], &[(0, "DOOR")]));
        f.graphic(v8(3, 0x5000, 6, shown(0, 0), &doubles(&[0.0, 8000.0, 1000.0, 8000.0]), &[]));
        f.graphic(v8(3, 0x5000, 7, shown(0, 2), &doubles(&[0.0, 9000.0, 1000.0, 9000.0]), &[]));
        // A 3D complex shape of two line strings: one triangle.
        f.graphic(v8(14, 0x3000, 8, shown(D3, 0), &2u32.to_le_bytes(), &[]));
        f.graphic(v8(4, 0x5000, 8, shown(D3, 0), &counted(2, &[30_000.0, 0.0, 0.0, 31_000.0, 0.0, 0.0]), &[]));
        f.graphic(v8(4, 0x5000, 8, shown(D3, 0), &counted(3, &[31_000.0, 0.0, 0.0, 31_000.0, 1000.0, 0.0, 30_000.0, 0.0, 0.0]), &[]));
        // A disjoint point string: three point markers.
        f.graphic(v8(22, 0x1000, 9, shown(D3 | HOLE, 0), &counted(3, &[0.0, 12_000.0, 0.0, 500.0, 12_000.0, 0.0, 1000.0, 12_000.0, 0.0]), &[]));
        // A text node of two texts.
        f.graphic(v8(7, 0x3000, 10, shown(0, 0), &[0; 8], &[]));
        f.graphic(v8(17, 0x5000, 10, shown(0, 0), &text_data(50_000.0, [0.0, 14_000.0], b"A"), &[]));
        f.graphic(v8(17, 0x5000, 10, shown(0, 0), &text_data(50_000.0, [0.0, 15_000.0], b"B\0"), &[]));
        // Not described yet: a dimension, an unknown type and a second model.
        f.graphic(v8(33, 0x1000, 11, shown(0, 0), &[0; 64], &[]));
        f.graphic(v8(105, 0x1000, 11, shown(0, 0), &[0; 64], &[]));
        f.extra.push(("Dgn-Md/#000001/Dgn^G/$1".into(), vec![0; 16]));
        let loaded = load(f.bytes()).unwrap();
        let layer = |name: &str| loaded.layers.iter().find(|l| l.name == name).unwrap_or_else(|| panic!("no layer {name}"));
        assert_eq!(layer("Level 1").triangle_count(), 2, "a closed 3D shape is a face");
        assert_eq!(layer("Level 2").triangle_count(), 16, "a hole is four crossed ribbons");
        let (lo, hi) = bounds(layer("Level 3"));
        // Sizes of ribbons include their width (about 0.01 m at this drawing extent).
        assert!((hi[0] - lo[0] - 1.0).abs() < 0.05 && (hi[1] - lo[1] - 1.0).abs() < 0.05, "{lo:?} {hi:?}");
        let (lo, hi) = bounds(layer("Level 4"));
        assert!((hi[0] - lo[0] - 4.0).abs() < 0.01 && (hi[1] - lo[1] - 2.0).abs() < 0.01 && hi[2] == lo[2], "{lo:?} {hi:?}");
        let text = layer("Level 5");
        assert!(text.props[0].1.contains("\"Text\":\"myTéxt\""), "{}", text.props[0].1);
        let (lo, hi) = bounds(text);
        assert!((hi[0] - lo[0] - 1.8).abs() < 0.05 && (hi[1] - lo[1] - 0.3).abs() < 0.05, "six characters of 0.3 m: {lo:?} {hi:?}");
        let (door, door_frame) = (layer("Level 6"), layer("Level 7"));
        assert_eq!(door.elements[0].0, door_frame.elements[0].0, "a cell is one pickable element");
        assert!(door.props[0].1.contains("\"Cell\":\"DOOR\""), "{}", door.props[0].1);
        assert_eq!(layer("Level 8").triangle_count(), 1);
        assert_eq!(layer("Level 9").triangle_count(), 12, "three point markers");
        assert!(layer("Level 10").props[0].1.contains("\"Text\":\"A\\nB\""), "{}", layer("Level 10").props[0].1);
        let ids: BTreeSet<u64> = loaded.layers.iter().flat_map(|l| l.elements.iter().map(|e| e.0)).collect();
        assert_eq!(ids.len(), 9);
        let elements = info(&loaded, "DGN elements");
        for drawn in ["Arc: 1", "Cell: 1", "Complex shape: 1", "Ellipse: 1", "Point string: 1", "Shape: 2", "Text: 1", "Text node: 1"] {
            assert!(elements.contains(drawn), "{drawn} in {elements}");
        }
        assert_eq!(info(&loaded, "DGN skipped elements"), "Dimension: 1, Element type 105: 1, Further model (not read): 1");
    }

    #[test]
    fn v8_shared_cells_place_their_definition_by_the_row_major_matrix() {
        const IDENTITY: [f64; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let mut f = V8File::new(1000.0, 1.0, 1.0, [0.0; 3]);
        f.table(level_entry(70, "Furniture"));
        // The definition: a line of 1 m along X, on level 0.
        f.table(v8(34, 0x3000, 0, shown(0, 0), &shared_data(IDENTITY, [0.0; 3]), &[(0, "ARROW")]));
        f.table(v8(3, 0x5000, 0, shown(0, 0), &doubles(&[0.0, 0.0, 1000.0, 0.0]), &[]));
        // Turned +90 degrees about Z (row by row) and placed at (5, 0) m: the line runs along +Y.
        f.graphic(v8(35, 0x1000, 70, shown(0, 0), &shared_data([0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0], [5000.0, 0.0, 0.0]), &[(0, "ARROW")]));
        // Twice as long, at (0, 3) m.
        f.graphic(v8(35, 0x1000, 70, shown(0, 0), &shared_data([2.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 1.0], [0.0, 3000.0, 0.0]), &[(0, "arrow")]));
        f.graphic(v8(35, 0x1000, 70, shown(0, 0), &shared_data(IDENTITY, [0.0; 3]), &[(0, "MISSING")]));
        let loaded = load(f.bytes()).unwrap();
        assert_eq!(loaded.layers.len(), 1, "components on level 0 take the level of the instance");
        let layer = &loaded.layers[0];
        assert_eq!(layer.name, "Furniture");
        assert_eq!(layer.elements.len(), 2);
        assert!(layer.props[0].1.contains("\"Cell\":\"ARROW\""), "{}", layer.props[0].1);
        // The world origin is the first point of the first instance, (5, 0) m.
        let (lo, hi) = element_bounds(layer, 0);
        assert!(lo[1] > -0.01 && (hi[1] - 1.0).abs() < 0.01 && hi[0] - lo[0] < 0.01, "{lo:?} {hi:?}");
        let (lo, hi) = element_bounds(layer, 1);
        assert!((lo[0] + 5.0).abs() < 0.01 && (hi[0] - lo[0] - 2.0).abs() < 0.01 && (lo[1] - 3.0).abs() < 0.01, "{lo:?} {hi:?}");
        assert_eq!(info(&loaded, "DGN elements"), "Shared cell: 2");
        assert_eq!(info(&loaded, "DGN skipped elements"), "Shared cell without definition: 1");
    }

    #[test]
    fn v8_rational_closed_bspline_is_a_circle() {
        let mut f = V8File::new(1000.0, 1.0, 1.0, [0.0; 3]);
        // A circle of radius 1 m: three arcs of 120 degrees, order 3, weights 1 and 0.5.
        let (r, s) = (1000.0, 3f64.sqrt());
        let poles = [r, 0.0, r, s * r, -r / 2.0, s / 2.0 * r, -2.0 * r, 0.0, -r / 2.0, -s / 2.0 * r, r, -s * r, r, 0.0];
        let header: Vec<u8> = [3u32, 1 | 0x80 | 0x100, 7, 6].iter().flat_map(|v| v.to_le_bytes()).collect();
        f.graphic(v8(27, 0x3000, 1, shown(0, 0), &header, &[]));
        f.graphic(v8(26, 0x4400, 0, None, &doubles(&[0.0, 1.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0, 1.0]), &[]));
        f.graphic(v8(21, 0x5000, 1, shown(0, 0), &counted(7, &poles), &[]));
        f.graphic(v8(28, 0x4400, 0, None, &doubles(&[1.0, 0.5, 1.0, 0.5, 1.0, 0.5, 1.0]), &[]));
        let loaded = load(f.bytes()).unwrap();
        // The world origin is the first point, (1, 0) m.
        let (lo, hi) = bounds(&loaded.layers[0]);
        assert!((lo[0] + 2.0).abs() < 0.01 && hi[0].abs() < 0.01 && (lo[1] + 1.0).abs() < 0.01 && (hi[1] - 1.0).abs() < 0.01, "{lo:?} {hi:?}");
        assert_eq!(info(&loaded, "DGN elements"), "B-spline curve: 1");
        assert_eq!(loaded.layers[0].elements.len(), 1);
    }
}

