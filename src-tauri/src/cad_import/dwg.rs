//! AutoCAD DWG import (3D view of model space) using the acadrust reader.
//!
//! Model space is walked with block references (INSERT, MINSERT) expanded recursively.
//! Faces (3DFACE, 2D SOLID/TRACE, MESH, polyface and polygon meshes, ACIS 3DSOLID, BODY,
//! REGION and SURFACE) become triangle meshes; curves (LINE, ARC, CIRCLE, ELLIPSE,
//! polylines, SPLINE, HELIX) become line segments, and curves with a thickness are also
//! extruded into faces. Geometry is grouped per layer with the AutoCAD colour of the
//! entity (ByLayer/ByBlock resolved), scaled to metres and returned as an "O3DS" buffer.

use super::acis;
use super::encode::{Scene, KIND_LINES, KIND_MESH};
use acadrust::types::{Matrix3, Matrix4, Vector3};
use acadrust::{CadDocument, Color, DwgReadOptions, DwgReader, EntityType};
use std::collections::BTreeMap;

const MAX_DEPTH: u32 = 16;
const MAX_ARRAY_INSTANCES: usize = 10_000;
/// Stop adding geometry beyond this many triangles / line segments (keeps memory bounded).
const MAX_TRIANGLES: usize = 30_000_000;
const MAX_SEGMENTS: usize = 20_000_000;
/// Bound on entities visited through nested block references (guards against pathological nesting).
const MAX_VISITS: usize = 20_000_000;
const ARC_STEP: f64 = std::f64::consts::PI / 18.0;

type V = [f64; 3];

fn v(p: Vector3) -> V {
    [p.x, p.y, p.z]
}

/// DWG release from the 6-byte file signature.
fn release_name(sig: &str) -> Option<&'static str> {
    Some(match sig {
        "AC1009" => "R11/R12",
        "AC1012" => "R13",
        "AC1014" => "R14",
        "AC1015" => "2000",
        "AC1018" => "2004",
        "AC1021" => "2007",
        "AC1024" => "2010",
        "AC1027" => "2013",
        "AC1032" => "2018",
        _ => return None,
    })
}

/// Convert DWG file bytes into an O3DS scene buffer.
pub fn convert(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let scene = scene(bytes)?;
    if scene.is_empty() {
        return Err("The DWG file contains no 3D geometry or curves in model space".to_string());
    }
    Ok(scene.encode())
}

/// Read a DWG file and collect its model space geometry.
pub fn scene(bytes: &[u8]) -> Result<Scene, String> {
    if bytes.len() < 6 || &bytes[0..2] != b"AC" {
        return Err("Not a DWG file (missing the AutoCAD signature)".to_string());
    }
    let sig = String::from_utf8_lossy(&bytes[0..6]).to_string();
    let release = release_name(&sig).ok_or_else(|| {
        format!("Unsupported DWG version {sig}: supported are AutoCAD R12 to 2018+ drawings (AC1009-AC1032)")
    })?;

    // strict first; damaged files get a second, forgiving read that keeps what it can
    let (doc, read_error) = match DwgReader::from_stream(std::io::Cursor::new(bytes)).read() {
        Ok(doc) => (doc, None),
        Err(strict) => {
            let doc = DwgReader::from_stream_with_options(std::io::Cursor::new(bytes), DwgReadOptions::failsafe())
                .read()
                .map_err(|e| format!("Cannot read DWG {release} file ({sig}): {strict}; recovery failed: {e}"))?;
            (doc, Some(strict.to_string()))
        }
    };

    let mut scene = Scene::new();
    let (scale, unit) = unit_scale(doc.header.insertion_units);
    scene.notes.push(format!("DWG {release} ({sig}), drawing units: {unit}"));

    let mut walker = Walker { doc: &doc, scene: &mut scene, stats: Stats::default(), layer_cache: BTreeMap::new() };
    let root = Matrix4::scaling(scale, scale, scale);
    let top = Inherit { layer: None, color: None };
    for e in doc.model_space_entities() {
        walker.entity(e, &root, &top, 0);
    }
    let stats = walker.stats;
    scene.notes.extend(stats.notes());
    if let Some(e) = read_error {
        if scene.is_empty() {
            return Err(format!("Cannot read DWG {release} file ({sig}): {e}"));
        }
        scene.notes.push(format!("The file is damaged; showing what could be recovered ({e})"));
    }
    Ok(scene)
}

/// Metres per drawing unit (INSUNITS) and a description.
fn unit_scale(insunits: i16) -> (f64, String) {
    let (s, name) = match insunits {
        1 => (0.0254, "inches"),
        2 => (0.3048, "feet"),
        3 => (1609.344, "miles"),
        4 => (0.001, "millimetres"),
        5 => (0.01, "centimetres"),
        6 => (1.0, "metres"),
        7 => (1000.0, "kilometres"),
        8 => (0.0254e-6, "microinches"),
        9 => (0.0254e-3, "mils"),
        10 => (0.9144, "yards"),
        11 => (1e-10, "angstroms"),
        12 => (1e-9, "nanometres"),
        13 => (1e-6, "microns"),
        14 => (0.1, "decimetres"),
        15 => (10.0, "decametres"),
        16 => (100.0, "hectometres"),
        17 => (1e9, "gigametres"),
        18 => (1.495_978_707e11, "astronomical units"),
        19 => (9.460_730_472_580_8e15, "light years"),
        20 => (3.085_677_581_49e16, "parsecs"),
        21 => (0.304_800_609_601_219_2, "US survey feet"),
        _ => return (0.001, "unitless (assumed millimetres)".to_string()),
    };
    (s, name.to_string())
}

#[derive(Default)]
struct Stats {
    triangles: usize,
    segments: usize,
    skipped: BTreeMap<&'static str, usize>,
    acis_faces_ok: usize,
    acis_faces_failed: usize,
    acis_unreadable: usize,
    xrefs: usize,
    missing_blocks: usize,
    depth_limit: usize,
    budget_hit: bool,
    visits: usize,
}

impl Stats {
    fn notes(&self) -> Vec<String> {
        let mut n = Vec::new();
        n.push(format!("{} triangles, {} line segments", self.triangles, self.segments));
        if self.acis_faces_ok + self.acis_faces_failed > 0 {
            n.push(format!(
                "ACIS faces: {} triangulated, {} shown as edges only",
                self.acis_faces_ok, self.acis_faces_failed
            ));
        }
        if self.acis_unreadable > 0 {
            n.push(format!("{} ACIS solids/surfaces could not be decoded", self.acis_unreadable));
        }
        if self.xrefs > 0 {
            n.push(format!("{} external references (xrefs) skipped", self.xrefs));
        }
        if self.missing_blocks > 0 {
            n.push(format!("{} block references to missing blocks skipped", self.missing_blocks));
        }
        if self.depth_limit > 0 {
            n.push(format!("{} nested blocks beyond depth {} skipped", self.depth_limit, MAX_DEPTH));
        }
        if self.budget_hit {
            n.push("Geometry limit reached: the model was truncated".to_string());
        }
        if !self.skipped.is_empty() {
            let list: Vec<String> = self.skipped.iter().map(|(k, v)| format!("{k} {v}")).collect();
            n.push(format!("Not shown: {}", list.join(", ")));
        }
        n
    }
}

/// Layer and colour an entity inherits from the block reference it is inside.
#[derive(Clone)]
struct Inherit {
    layer: Option<String>,
    color: Option<[f32; 3]>,
}

struct LayerInfo {
    visible: bool,
    color: [f32; 3],
}

struct Walker<'a> {
    doc: &'a CadDocument,
    scene: &'a mut Scene,
    stats: Stats,
    layer_cache: BTreeMap<String, LayerInfo>,
}

fn rgb(c: (u8, u8, u8)) -> [f32; 3] {
    [c.0 as f32 / 255.0, c.1 as f32 / 255.0, c.2 as f32 / 255.0]
}

const DEFAULT_COLOR: [f32; 3] = [0.75, 0.75, 0.75];

fn xf(m: &Matrix4, p: V) -> V {
    v(m.transform_point(Vector3::new(p[0], p[1], p[2])))
}

fn xf_dir(m: &Matrix4, d: V) -> V {
    v(m.transform_direction(Vector3::new(d[0], d[1], d[2])))
}

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: V, s: f64) -> V {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn cross(a: V, b: V) -> V {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn length(a: V) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: V) -> V {
    let l = length(a);
    if l > 1e-300 {
        scale(a, 1.0 / l)
    } else {
        [0.0, 0.0, 1.0]
    }
}

/// Lines in white/black (ACI 7) would vanish against the viewer background; use dark grey.
fn line_color(c: [f32; 3]) -> [f32; 3] {
    if c.iter().all(|x| *x > 0.92) {
        [0.2, 0.2, 0.2]
    } else {
        c
    }
}

/// Faces in pure black show no shading; lighten slightly.
fn face_color(c: [f32; 3]) -> [f32; 3] {
    if c.iter().all(|x| *x < 0.06) {
        [0.25, 0.25, 0.25]
    } else {
        c
    }
}

impl<'a> Walker<'a> {
    fn layer(&mut self, name: &str) -> &LayerInfo {
        if !self.layer_cache.contains_key(name) {
            let info = match self.doc.layers.get(name) {
                Some(l) => LayerInfo {
                    visible: !l.flags.off && !l.flags.frozen,
                    color: l.color.rgb().map(rgb).unwrap_or(DEFAULT_COLOR),
                },
                None => LayerInfo { visible: true, color: DEFAULT_COLOR },
            };
            self.layer_cache.insert(name.to_string(), info);
        }
        &self.layer_cache[name]
    }

    /// Effective layer name and colour of an entity.
    fn resolve(&mut self, layer: &str, color: Color, inh: &Inherit) -> Option<(String, [f32; 3])> {
        let name = match &inh.layer {
            Some(parent) if layer == "0" || layer.is_empty() => parent.clone(),
            _ => layer.to_string(),
        };
        let info = self.layer(&name);
        if !info.visible {
            return None;
        }
        let layer_color = info.color;
        let c = match color {
            Color::ByLayer | Color::None => layer_color,
            Color::ByBlock => inh.color.unwrap_or(DEFAULT_COLOR),
            other => other.rgb().map(rgb).unwrap_or(layer_color),
        };
        Some((name, c))
    }

    fn skip(&mut self, what: &'static str) {
        *self.stats.skipped.entry(what).or_default() += 1;
    }

    fn entity(&mut self, e: &EntityType, m: &Matrix4, inh: &Inherit, depth: u32) {
        let common = e.common();
        if common.invisible {
            return;
        }
        self.stats.visits += 1;
        if self.stats.visits > MAX_VISITS {
            self.stats.budget_hit = true;
        }
        if self.stats.budget_hit {
            return;
        }
        // Block references are expanded before layer visibility: the insert's layer only
        // matters for members on layer "0".
        if let EntityType::Insert(ins) = e {
            self.insert(ins, m, inh, depth);
            return;
        }
        let Some((layer, color)) = self.resolve(&common.layer, common.color, inh) else {
            return;
        };
        match e {
            EntityType::Face3D(f) => {
                let pts: Vec<V> = f.corners().into_iter().map(|p| xf(m, v(p))).collect();
                self.polygon(&layer, color, &pts);
            }
            EntityType::Solid(s) => {
                let ocs = Matrix3::arbitrary_axis(s.normal);
                let pts: Vec<V> = s.boundary_corners().into_iter().map(|p| xf(m, v(ocs * p))).collect();
                if s.thickness.abs() > 1e-12 {
                    let off = xf_dir(m, scale(v(s.normal), s.thickness));
                    self.extrude(&layer, color, &pts, off, true);
                }
                self.polygon(&layer, color, &pts);
            }
            EntityType::Mesh(_) | EntityType::PolyfaceMesh(_) | EntityType::PolygonMesh(_) => {
                for part in e.flatten() {
                    if let EntityType::Face3D(f) = part {
                        let pts: Vec<V> = f.corners().into_iter().map(|p| xf(m, v(p))).collect();
                        self.polygon(&layer, color, &pts);
                    }
                }
            }
            EntityType::Solid3D(s) => self.acis(&layer, color, &s.acis_data, m),
            EntityType::Body(s) => self.acis(&layer, color, &s.acis_data, m),
            EntityType::Region(s) => self.acis(&layer, color, &s.acis_data, m),
            EntityType::Surface(s) => self.acis(&layer, color, &s.acis_data, m),
            EntityType::Line(l) => {
                let pts = vec![xf(m, v(l.start)), xf(m, v(l.end))];
                self.curve(&layer, color, &pts, l.thickness, l.normal, m, false);
            }
            EntityType::Arc(a) => {
                let mut sweep = (a.end_angle - a.start_angle).rem_euclid(std::f64::consts::TAU);
                if sweep < 1e-9 {
                    sweep = std::f64::consts::TAU;
                }
                let pts = ocs_arc(a.center, a.radius, a.start_angle, sweep, a.normal, m);
                self.curve(&layer, color, &pts, a.thickness, a.normal, m, false);
            }
            EntityType::Circle(c) => {
                let mut pts = ocs_arc(c.center, c.radius, 0.0, std::f64::consts::TAU, c.normal, m);
                pts.pop();
                self.curve(&layer, color, &pts, c.thickness, c.normal, m, true);
            }
            EntityType::Ellipse(el) => {
                let c = v(el.center);
                let maj = v(el.major_axis);
                let minor = scale(cross(unit(v(el.normal)), maj), el.minor_axis_ratio);
                let mut sweep = el.end_parameter - el.start_parameter;
                if sweep <= 1e-9 {
                    sweep += std::f64::consts::TAU;
                }
                let n = ((sweep / ARC_STEP).ceil() as usize).max(2);
                let pts: Vec<V> = (0..=n)
                    .map(|i| {
                        let t = el.start_parameter + sweep * i as f64 / n as f64;
                        xf(m, add(c, add(scale(maj, t.cos()), scale(minor, t.sin()))))
                    })
                    .collect();
                self.curve(&layer, color, &pts, 0.0, el.normal, m, false);
            }
            EntityType::LwPolyline(p) => {
                let verts: Vec<(f64, f64, f64, f64)> =
                    p.vertices.iter().map(|v| (v.location.x, v.location.y, p.elevation, v.bulge)).collect();
                let pts = bulge_polyline(&verts, p.is_closed, p.normal, m);
                self.curve(&layer, color, &pts, p.thickness, p.normal, m, p.is_closed);
            }
            EntityType::Polyline2D(p) => {
                let closed = p.flags.is_closed();
                let verts: Vec<(f64, f64, f64, f64)> =
                    p.vertices.iter().map(|v| (v.location.x, v.location.y, v.location.z, v.bulge)).collect();
                let pts = bulge_polyline(&verts, closed, p.normal, m);
                self.curve(&layer, color, &pts, p.thickness, p.normal, m, closed);
            }
            EntityType::Polyline(_) | EntityType::Polyline3D(_) => {
                // exploded into lines and arcs that keep the polyline's layer, colour and thickness
                let child = Inherit { layer: Some(layer.clone()), color: Some(color) };
                for part in e.explode() {
                    if matches!(part, EntityType::Line(_) | EntityType::Arc(_) | EntityType::Spline(_)) {
                        let mut part = part;
                        let c = part.common_mut();
                        c.layer = "0".to_string();
                        c.color = Color::ByBlock;
                        self.entity(&part, m, &child, depth);
                    }
                }
            }
            EntityType::Spline(_) | EntityType::Helix(_) => {
                let mut segs: Vec<V> = Vec::new();
                for part in e.flatten() {
                    if let EntityType::Line(l) = part {
                        segs.push(xf(m, v(l.start)));
                        segs.push(xf(m, v(l.end)));
                    }
                }
                self.segments(&layer, color, &segs);
            }
            EntityType::Text(_) | EntityType::MText(_) => self.skip("text"),
            EntityType::Dimension(_) => self.skip("dimensions"),
            EntityType::Hatch(_) => self.skip("hatches"),
            EntityType::Leader(_) | EntityType::MultiLeader(_) => self.skip("leaders"),
            EntityType::Point(_) => self.skip("points"),
            EntityType::RasterImage(_) | EntityType::Underlay(_) | EntityType::Ole2Frame(_) => self.skip("images/underlays"),
            EntityType::Viewport(_)
            | EntityType::Light(_)
            | EntityType::Block(_)
            | EntityType::BlockEnd(_)
            | EntityType::Seqend(_)
            | EntityType::AttributeDefinition(_)
            | EntityType::AttributeEntity(_)
            | EntityType::XLine(_)
            | EntityType::Ray(_) => {}
            _ => self.skip("other entities"),
        }
    }

    fn insert(&mut self, ins: &acadrust::entities::Insert, m: &Matrix4, inh: &Inherit, depth: u32) {
        if depth >= MAX_DEPTH {
            self.stats.depth_limit += 1;
            return;
        }
        let doc = self.doc;
        let Some(br) = doc.block_records.get(&ins.block_name) else {
            self.stats.missing_blocks += 1;
            return;
        };
        if br.flags.is_xref || br.flags.is_xref_overlay {
            self.stats.xrefs += 1;
            return;
        }
        // the insert's own effective layer and colour, inherited by members on layer 0 / ByBlock
        let Some((layer, color)) = self.resolve(&ins.common.layer, ins.common.color, inh) else {
            return;
        };
        let child = Inherit { layer: Some(layer), color: Some(color) };

        let base = doc
            .get_entity(br.block_entity_handle)
            .and_then(|e| match e {
                EntityType::Block(b) => Some(b.base_point),
                _ => None,
            })
            .unwrap_or(br.base_point);
        let to_base = Matrix4::translation(-base.x, -base.y, -base.z);

        let transforms: Vec<Matrix4> = if ins.is_minsert() && ins.instance_count() > 1 {
            if ins.instance_count() > MAX_ARRAY_INSTANCES {
                self.skip("huge MINSERT arrays");
                return;
            }
            // same as acadrust's array transforms: OCS * translate(cell) * rotate * scale
            let ocs = Matrix4::from_matrix3(Matrix3::arbitrary_axis(ins.normal));
            let rot = Matrix4::rotation_z(ins.rotation);
            let scl = Matrix4::scaling(ins.x_scale(), ins.y_scale(), ins.z_scale());
            ins.array_points().into_iter().map(|p| ocs * Matrix4::translation(p.x, p.y, p.z) * rot * scl).collect()
        } else {
            vec![ins.get_transform().matrix]
        };
        for t in transforms {
            let world = *m * t * to_base;
            for h in &br.entity_handles {
                if self.stats.budget_hit {
                    return;
                }
                if let Some(e) = doc.get_entity(*h) {
                    self.entity(e, &world, &child, depth + 1);
                }
            }
        }
    }

    fn acis(&mut self, layer: &str, color: [f32; 3], data: &acadrust::entities::AcisData, m: &Matrix4) {
        let Some(doc) = acis::sat_document(data) else {
            if data.sab_data.is_empty() && data.sat_data.trim().is_empty() {
                return; // empty proxy solid
            }
            self.stats.acis_unreadable += 1;
            return;
        };
        let mut t = acis::Tess::default();
        acis::tessellate(&doc, &mut t);
        self.stats.acis_faces_ok += t.faces_ok;
        self.stats.acis_faces_failed += t.faces_failed;
        if !t.indices.is_empty() {
            self.stats.triangles += t.indices.len() / 3;
            if self.stats.triangles > MAX_TRIANGLES {
                self.stats.budget_hit = true;
                return;
            }
            let normal_m = normal_matrix(m);
            let flip = det3(m) < 0.0;
            let item = self.scene.item(layer, KIND_MESH, face_color(color));
            let base = item.vertex_count() as u32;
            for (p, n) in t.positions.iter().zip(t.normals.iter()) {
                let q = xf(m, *p);
                item.positions.extend_from_slice(&q);
                let nn = unit(mat3_mul(&normal_m, *n));
                item.normals.extend_from_slice(&[nn[0] as f32, nn[1] as f32, nn[2] as f32]);
            }
            for tri in t.indices.chunks_exact(3) {
                if flip {
                    item.indices.extend_from_slice(&[base + tri[0], base + tri[2], base + tri[1]]);
                } else {
                    item.indices.extend_from_slice(&[base + tri[0], base + tri[1], base + tri[2]]);
                }
            }
        }
        if !t.lines.is_empty() {
            let segs: Vec<V> = t.lines.iter().map(|p| xf(m, *p)).collect();
            self.segments(layer, color, &segs);
        }
    }

    /// A planar (or nearly planar) polygon in world coordinates, fan triangulated.
    fn polygon(&mut self, layer: &str, color: [f32; 3], pts: &[V]) {
        let mut p: Vec<V> = Vec::with_capacity(pts.len());
        for q in pts {
            if p.last().is_none_or(|l| length(sub(*q, *l)) > 1e-12) {
                p.push(*q);
            }
        }
        while p.len() > 1 && length(sub(p[0], *p.last().unwrap())) <= 1e-12 {
            p.pop();
        }
        if p.len() < 3 {
            return;
        }
        self.stats.triangles += p.len() - 2;
        if self.stats.triangles > MAX_TRIANGLES {
            self.stats.budget_hit = true;
            return;
        }
        let item = self.scene.item(layer, KIND_MESH, face_color(color));
        let base = item.vertex_count() as u32;
        // a quad may be non-planar: give each triangle its own normal
        for i in 1..p.len() - 1 {
            let (a, b, c) = (p[0], p[i], p[i + 1]);
            let n = cross(sub(b, a), sub(c, a));
            if length(n) <= 1e-300 {
                continue;
            }
            let n = unit(n);
            let k = base + (item.vertex_count() as u32 - base);
            for q in [a, b, c] {
                item.positions.extend_from_slice(&q);
                item.normals.extend_from_slice(&[n[0] as f32, n[1] as f32, n[2] as f32]);
            }
            item.indices.extend_from_slice(&[k, k + 1, k + 2]);
        }
    }

    /// Side faces of a polygon or polyline extruded by `off`.
    fn extrude(&mut self, layer: &str, color: [f32; 3], pts: &[V], off: V, closed: bool) {
        let n = pts.len();
        if n < 2 {
            return;
        }
        let count = if closed { n } else { n - 1 };
        for i in 0..count {
            let a = pts[i];
            let b = pts[(i + 1) % n];
            self.polygon(layer, color, &[a, b, add(b, off), add(a, off)]);
        }
        if closed && n >= 3 {
            let top: Vec<V> = pts.iter().map(|p| add(*p, off)).collect();
            self.polygon(layer, color, &top);
        }
    }

    /// A polyline in world coordinates; extruded into faces when it has a thickness.
    #[allow(clippy::too_many_arguments)]
    fn curve(&mut self, layer: &str, color: [f32; 3], pts: &[V], thickness: f64, normal: Vector3, m: &Matrix4, closed: bool) {
        if pts.len() < 2 {
            return;
        }
        let mut segs = Vec::with_capacity(pts.len() * 2);
        for w in pts.windows(2) {
            segs.push(w[0]);
            segs.push(w[1]);
        }
        if closed && pts.len() > 2 {
            segs.push(*pts.last().unwrap());
            segs.push(pts[0]);
        }
        if thickness.abs() > 1e-12 {
            let off = xf_dir(m, scale(v(normal), thickness));
            // side walls only; a closed circle gets no caps (AutoCAD draws an open tube)
            let n = pts.len();
            let count = if closed { n } else { n - 1 };
            for i in 0..count {
                let a = pts[i];
                let b = pts[(i + 1) % n];
                self.polygon(layer, color, &[a, b, add(b, off), add(a, off)]);
            }
            let top: Vec<V> = segs.iter().map(|p| add(*p, off)).collect();
            segs.extend(top);
        }
        self.segments(layer, color, &segs);
    }

    fn segments(&mut self, layer: &str, color: [f32; 3], segs: &[V]) {
        if segs.len() < 2 {
            return;
        }
        self.stats.segments += segs.len() / 2;
        if self.stats.segments > MAX_SEGMENTS {
            self.stats.budget_hit = true;
            return;
        }
        let item = self.scene.item(layer, KIND_LINES, line_color(color));
        for p in &segs[..segs.len() / 2 * 2] {
            item.positions.extend_from_slice(p);
        }
    }
}

/// World points of a 2D polyline with bulges (x, y, elevation, bulge in OCS). For a closed
/// polyline the closing arc is included and the first point is not repeated.
fn bulge_polyline(verts: &[(f64, f64, f64, f64)], closed: bool, normal: Vector3, m: &Matrix4) -> Vec<V> {
    let ocs = Matrix3::arbitrary_axis(normal);
    let n = verts.len();
    let mut out: Vec<V> = Vec::with_capacity(n * 2);
    let world = |x: f64, y: f64, z: f64| xf(m, v(ocs * Vector3::new(x, y, z)));
    let segs = if closed { n } else { n.saturating_sub(1) };
    for i in 0..n {
        let (x1, y1, z1, b) = verts[i];
        out.push(world(x1, y1, z1));
        if i >= segs || b.abs() < 1e-10 {
            continue;
        }
        let (x2, y2, _, _) = verts[(i + 1) % n];
        let (dx, dy) = (x2 - x1, y2 - y1);
        let d = (dx * dx + dy * dy).sqrt();
        if d < 1e-12 {
            continue;
        }
        let theta = 4.0 * b.atan(); // signed included angle, CCW positive
        let (mx, my) = ((x1 + x2) / 2.0, (y1 + y2) / 2.0);
        let (nx, ny) = (-dy / d, dx / d); // left of the chord
        let off = (d / 2.0) / (theta / 2.0).tan();
        let (cx, cy) = (mx + nx * off, my + ny * off);
        let r = ((x1 - cx).powi(2) + (y1 - cy).powi(2)).sqrt();
        let a1 = (y1 - cy).atan2(x1 - cx);
        let k = ((theta.abs() / ARC_STEP).ceil() as usize).max(1);
        for j in 1..k {
            let a = a1 + theta * j as f64 / k as f64;
            out.push(world(cx + r * a.cos(), cy + r * a.sin(), z1));
        }
    }
    out
}

/// Points of an arc given in OCS (centre and angles in the plane of `normal`).
fn ocs_arc(center: Vector3, radius: f64, start: f64, sweep: f64, normal: Vector3, m: &Matrix4) -> Vec<V> {
    let ocs = Matrix3::arbitrary_axis(normal);
    let n = ((sweep.abs() / ARC_STEP).ceil() as usize).max(2);
    (0..=n)
        .map(|i| {
            let t = start + sweep * i as f64 / n as f64;
            let p = Vector3::new(center.x + radius * t.cos(), center.y + radius * t.sin(), center.z);
            xf(m, v(ocs * p))
        })
        .collect()
}

fn det3(m: &Matrix4) -> f64 {
    let a = &m.m;
    a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0])
}

/// Inverse transpose of the linear part (normals under non-uniform scale).
fn normal_matrix(m: &Matrix4) -> [[f64; 3]; 3] {
    let a = &m.m;
    let r0 = [a[0][0], a[0][1], a[0][2]];
    let r1 = [a[1][0], a[1][1], a[1][2]];
    let r2 = [a[2][0], a[2][1], a[2][2]];
    // inverse transpose = cofactor matrix / det
    let c0 = cross(r1, r2);
    let c1 = cross(r2, r0);
    let c2 = cross(r0, r1);
    let det = dot(r0, c0);
    let s = if det.abs() > 1e-300 { 1.0 / det } else { 1.0 };
    [scale(c0, s), scale(c1, s), scale(c2, s)]
}

fn mat3_mul(m: &[[f64; 3]; 3], p: V) -> V {
    [dot(m[0], p), dot(m[1], p), dot(m[2], p)]
}
