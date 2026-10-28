//! SketchUp (.skp): a Rust port of the reader in openskp 1.3 (MIT, Ahsan Mehmood,
//! https://github.com/iamahsanmehmood/openskp), the library the React app uses. It reads both
//! containers: the 2021+ ZIP ("VFF") format ([`vff`]) and the classic MFC archive of SketchUp 3
//! to 2020 ([`legacy`], at openskp 1.4.0). One layer per SketchUp tag, with material colours per
//! vertex (textures as the material's average colour) and each placed component or group as a
//! pickable element.
//!
//! SketchUp is Z up in inches; the layers are Z up in metres.

mod legacy;
mod vff;

use super::{Loaded, Loader, Phase, Progress, DEFAULT_COLOR, Source};
use crate::model::{Layer, ProjectData};
use glam::{DMat3, DVec3};
use std::collections::{HashMap, HashSet};

pub struct SkpLoader;

impl Loader for SkpLoader {
    fn name(&self) -> &'static str {
        "SketchUp"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["skp"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|l| l.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        progress.set(Phase::Convert);
        let parsed = parse(&data)?;
        let layers = SceneBuilder::new(&parsed).build();
        Ok(Loaded { layers, views: Vec::new(), project: ProjectData::default() })
    }
}

/// Reads either container into the shared model.
fn parse(data: &[u8]) -> Result<Parsed, String> {
    if !data.starts_with(&[0xFF, 0xFE, 0xFF, 0x0E]) {
        return Err("Not a valid SketchUp file (bad header)".into());
    }
    if legacy::is_legacy(data) {
        legacy::parse(data).map_err(|e| format!("classic SketchUp file: {e}"))
    } else {
        vff::parse(data)
    }
}

/// What both containers produce: definitions (the root model plus components and groups) with
/// their vertices, edges, faces and placements, and the materials and tags they refer to.
#[derive(Default)]
struct Parsed {
    /// Material id -> material.
    materials: HashMap<u64, Material>,
    /// Tag id -> tag name.
    tag_names: HashMap<u64, String>,
    /// Tag name -> colour and visibility.
    tags: HashMap<String, Tag>,
    definitions: HashMap<u64, Definition>,
    root: Definition,
}

#[derive(Clone, Copy)]
struct Material {
    rgba: [u8; 4],
    /// 0 (clear) to 1 (opaque).
    opacity: f64,
}

#[derive(Clone, Copy)]
struct Tag {
    color: [u8; 3],
    hidden: bool,
}

/// SketchUp's default tag; shown as "Untagged" since SketchUp 2020.
const DEFAULT_TAG: &str = "Layer0";
const DEFAULT_TAG_COLOR: [u8; 3] = [136, 136, 136];

#[derive(Default)]
struct Definition {
    name: String,
    vertices: HashMap<u64, [f64; 3]>,
    /// Edge id -> start and end vertex.
    edges: HashMap<u64, (Option<u64>, Option<u64>)>,
    faces: Vec<Face>,
    /// Face id -> index in `faces`; a face read twice replaces the first.
    face_index: HashMap<u64, usize>,
    instances: Vec<Instance>,
}

impl Definition {
    fn set_face(&mut self, id: u64, face: Face) {
        match self.face_index.get(&id) {
            Some(&i) => self.faces[i] = face,
            None => {
                self.face_index.insert(id, self.faces.len());
                self.faces.push(face);
            }
        }
    }
}

struct Face {
    /// Outer loop first, then the holes: (edge id, reversed) per edge use.
    loops: Vec<Vec<(u64, bool)>>,
    normal: [f64; 3],
    material: Option<u64>,
    back_material: Option<u64>,
}

/// A placed component, group or image.
struct Instance {
    name: String,
    definition: Option<u64>,
    matrix: Matrix,
    material: Option<u64>,
    tag: Option<u64>,
    hidden: bool,
    /// Dynamic component attributes.
    props: Vec<(String, String)>,
}

/// SketchUp's 13-value transform: a 3x3 matrix (rows), the translation, and a scale that is
/// not applied.
type Matrix = [f64; 13];

const IDENTITY: Matrix = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

fn transform_point(m: &Matrix, p: [f64; 3]) -> [f64; 3] {
    [
        m[0] * p[0] + m[1] * p[1] + m[2] * p[2] + m[9],
        m[3] * p[0] + m[4] * p[1] + m[5] * p[2] + m[10],
        m[6] * p[0] + m[7] * p[1] + m[8] * p[2] + m[11],
    ]
}

/// `a` applied after `b`.
fn multiply(a: &Matrix, b: &Matrix) -> Matrix {
    let rows = [[a[0], a[1], a[2], a[9]], [a[3], a[4], a[5], a[10]], [a[6], a[7], a[8], a[11]]];
    let cols = [[b[0], b[3], b[6], 0.0], [b[1], b[4], b[7], 0.0], [b[2], b[5], b[8], 0.0], [b[9], b[10], b[11], 1.0]];
    let dot = |r: &[f64; 4], c: &[f64; 4]| r[0] * c[0] + r[1] * c[1] + r[2] * c[2] + r[3] * c[3];
    let mut out = [0.0; 13];
    for (i, row) in rows.iter().enumerate() {
        for j in 0..3 {
            out[3 * i + j] = dot(row, &cols[j]);
        }
        out[9 + i] = dot(row, &cols[3]);
    }
    out[12] = a[12] * b[12];
    out
}

/// The matrix that takes normals along with `m` (inverse transpose of the 3x3 part).
fn normal_matrix(m: &Matrix) -> DMat3 {
    let linear = DMat3::from_cols_array(&[m[0], m[3], m[6], m[1], m[4], m[7], m[2], m[5], m[8]]);
    if linear.determinant().abs() > 1e-18 {
        linear.inverse().transpose()
    } else {
        linear
    }
}

/// The vertices around a face loop, from its edge uses.
fn loop_vertices(uses: &[(u64, bool)], edges: &HashMap<u64, (Option<u64>, Option<u64>)>) -> Vec<u64> {
    let mut out: Vec<u64> = Vec::new();
    for &(edge, reversed) in uses {
        let Some(&(v1, v2)) = edges.get(&edge) else { continue };
        if let Some(start) = if reversed { v2 } else { v1 } {
            if out.last() != Some(&start) {
                out.push(start);
            }
        }
    }
    if out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

/// Triangulates a planar face: `loops` are the outer boundary then the holes. Returns indices
/// into the loops' points taken in order.
fn triangulate(loops: &[Vec<[f64; 3]>], normal: [f64; 3]) -> Vec<[u32; 3]> {
    if loops.len() == 1 && loops[0].len() == 3 {
        return vec![[0, 1, 2]];
    }
    if loops.len() == 1 && loops[0].len() == 4 {
        return vec![[0, 1, 2], [0, 2, 3]];
    }
    let n = DVec3::from(normal);
    let n = if n.length() > 1e-6 { n.normalize() } else { DVec3::Z };
    let axis = if n.x.abs() < 0.9 { DVec3::X } else { DVec3::Y };
    let u = n.cross(axis);
    let u = if u.length() < 1e-12 { DVec3::X } else { u.normalize() };
    let v = n.cross(u);
    let v = if v.length() > 1e-12 { v.normalize() } else { v };
    let loops2d: Vec<Vec<[f64; 2]>> =
        loops.iter().map(|l| l.iter().map(|&p| [DVec3::from(p).dot(u), DVec3::from(p).dot(v)]).collect()).collect();
    let fan = |count: usize| (1..count.saturating_sub(1)).map(|i| [0, i as u32, i as u32 + 1]).collect::<Vec<_>>();
    if loops.len() > 2 && holes_overlap(&loops2d) {
        // Overlapping holes confuse earcut: triangulate the outer loop, drop what lies in a hole
        let outer = &loops2d[0];
        let flat: Vec<f64> = outer.iter().flatten().copied().collect();
        let Ok(tris) = earcutr::earcut(&flat, &[], 2) else { return fan(outer.len()) };
        return tris
            .chunks_exact(3)
            .filter(|t| {
                let c = [0, 1].map(|k| (outer[t[0]][k] + outer[t[1]][k] + outer[t[2]][k]) / 3.0);
                !loops2d[1..].iter().any(|hole| point_in_polygon(c, hole))
            })
            .map(|t| [t[0] as u32, t[1] as u32, t[2] as u32])
            .collect();
    }
    let flat: Vec<f64> = loops2d.iter().flatten().flatten().copied().collect();
    let mut holes = Vec::new();
    let mut offset = 0;
    for (i, l) in loops2d.iter().enumerate() {
        if i > 0 {
            holes.push(offset);
        }
        offset += l.len();
    }
    match earcutr::earcut(&flat, &holes, 2) {
        Ok(tris) => tris.chunks_exact(3).map(|t| [t[0] as u32, t[1] as u32, t[2] as u32]).collect(),
        Err(_) => fan(loops2d[0].len()),
    }
}

fn holes_overlap(loops2d: &[Vec<[f64; 2]>]) -> bool {
    let any_inside = |points: &[[f64; 2]], polygon: &[[f64; 2]]| points.iter().any(|&p| point_in_polygon(p, polygon));
    (1..loops2d.len()).any(|i| {
        (i + 1..loops2d.len()).any(|j| any_inside(&loops2d[i], &loops2d[j]) || any_inside(&loops2d[j], &loops2d[i]))
    })
}

fn point_in_polygon(p: [f64; 2], polygon: &[[f64; 2]]) -> bool {
    let mut inside = false;
    let mut j = polygon.len().wrapping_sub(1);
    for i in 0..polygon.len() {
        let (a, b) = (polygon[i], polygon[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// One face of a definition, triangulated in definition coordinates (inches).
struct FaceMesh {
    normal: [f64; 3],
    material: Option<u64>,
    back_material: Option<u64>,
    points: Vec<[f64; 3]>,
    triangles: Vec<[u32; 3]>,
}

fn face_meshes(def: &Definition) -> Vec<FaceMesh> {
    def.faces
        .iter()
        .filter_map(|face| {
            let loops: Vec<Vec<[f64; 3]>> = face
                .loops
                .iter()
                .map(|l| loop_vertices(l, &def.edges))
                .filter(|l| !l.is_empty())
                .map(|l| l.iter().map(|v| def.vertices.get(v).copied()).collect::<Option<Vec<_>>>())
                .collect::<Option<_>>()?;
            if loops.is_empty() {
                return None;
            }
            let triangles = triangulate(&loops, face.normal);
            (!triangles.is_empty()).then(|| FaceMesh {
                normal: face.normal,
                material: face.material,
                back_material: face.back_material,
                points: loops.concat(),
                triangles,
            })
        })
        .collect()
}

const INCH: f64 = 0.0254;

/// Places the definitions and collects the triangles per tag.
struct SceneBuilder<'p> {
    parsed: &'p Parsed,
    /// Triangulated faces per definition (`None` = root).
    meshes: HashMap<Option<u64>, Vec<FaceMesh>>,
    layers: Vec<Layer>,
    layer_of_tag: HashMap<String, usize>,
    next_element: u64,
    /// Definitions being placed, to stop on a definition that contains itself.
    active: HashSet<u64>,
}

impl<'p> SceneBuilder<'p> {
    fn new(parsed: &'p Parsed) -> Self {
        Self {
            parsed,
            meshes: HashMap::new(),
            layers: Vec::new(),
            layer_of_tag: HashMap::new(),
            next_element: 1,
            active: HashSet::new(),
        }
    }

    fn build(mut self) -> Vec<Layer> {
        let parsed = self.parsed;
        self.place(None, &parsed.root, &IDENTITY, DEFAULT_TAG, None, None);
        self.layers
    }

    fn tag(&self, name: &str) -> Tag {
        self.parsed.tags.get(name).copied().unwrap_or(Tag { color: DEFAULT_TAG_COLOR, hidden: false })
    }

    /// Adds the faces of `def` placed by `m` on `tag`, then its instances.
    fn place(
        &mut self,
        key: Option<u64>,
        def: &'p Definition,
        m: &Matrix,
        tag: &str,
        inherited: Option<Material>,
        element: Option<u64>,
    ) {
        self.add_faces(key, def, m, tag, inherited, element);
        for inst in &def.instances {
            let Some((id, child)) = inst.definition.and_then(|id| self.parsed.definitions.get(&id).map(|d| (id, d))) else {
                continue;
            };
            if inst.hidden || !self.active.insert(id) {
                continue;
            }
            // Untagged entities, and everything inside a hidden tag, follow their container
            let own = inst.tag.and_then(|t| self.parsed.tag_names.get(&t)).filter(|t| !t.is_empty());
            let child_tag = match own {
                Some(own) if own != DEFAULT_TAG && !self.tag(tag).hidden => own.clone(),
                _ => tag.to_string(),
            };
            let material = inst.material.and_then(|id| self.parsed.materials.get(&id).copied()).or(inherited);
            let element = self.next_element;
            self.next_element += 1;
            let name = if !inst.name.is_empty() {
                inst.name.clone()
            } else if !child.name.is_empty() && !is_generic_name(&child.name) {
                child.name.clone()
            } else {
                format!("Component_{id}")
            };
            let mut props = serde_json::Map::new();
            props.insert("Name".into(), name.into());
            props.insert("Definition".into(), child.name.clone().into());
            props.insert("Tag".into(), display_tag(&child_tag).into());
            for (k, v) in inst.props.iter().filter(|(k, _)| !k.starts_with('_')) {
                props.insert(k.clone(), v.clone().into());
            }
            let layer = self.layer_for(&child_tag);
            self.layers[layer].props.push((element, serde_json::Value::Object(props).to_string()));
            self.place(Some(id), child, &multiply(m, &inst.matrix), &child_tag, material, Some(element));
            self.active.remove(&id);
        }
    }

    fn layer_for(&mut self, tag: &str) -> usize {
        if let Some(&i) = self.layer_of_tag.get(tag) {
            return i;
        }
        let t = self.tag(tag);
        self.layers.push(Layer {
            name: display_tag(tag).to_string(),
            color: t.color.map(|c| c as f32 / 255.0),
            colors: Some(Vec::new()),
            hidden: t.hidden,
            ..Layer::default()
        });
        self.layer_of_tag.insert(tag.to_string(), self.layers.len() - 1);
        self.layers.len() - 1
    }

    fn add_faces(
        &mut self,
        key: Option<u64>,
        def: &Definition,
        m: &Matrix,
        tag: &str,
        inherited: Option<Material>,
        element: Option<u64>,
    ) {
        if def.faces.is_empty() {
            return;
        }
        if self.meshes.entry(key).or_insert_with(|| face_meshes(def)).is_empty() {
            return;
        }
        let li = self.layer_for(tag);
        let faces = &self.meshes[&key];
        let materials = &self.parsed.materials;
        let layer = &mut self.layers[li];
        let colors = layer.colors.get_or_insert_with(Vec::new);
        let first = layer.indices.len() as u32;
        let nm = normal_matrix(m);
        let [r, g, b] = DEFAULT_COLOR.map(|c| (c * 255.0).round() as u8);
        let unpainted = [r, g, b, 255];
        for face in faces {
            // Unpainted faces take the container's material, else the back material (the faces
            // are drawn from both sides), else the viewer's neutral grey, as SketchUp's default
            // style draws them (not the tag colour: SketchUp's own Layer0 is red)
            let material = face
                .material
                .and_then(|id| materials.get(&id).copied())
                .or(inherited)
                .or_else(|| face.back_material.and_then(|id| materials.get(&id).copied()));
            let rgba = match material {
                // The fourth colour byte is no alpha (2013-2020 files store opaque colours with 0)
                Some(mat) => [mat.rgba[0], mat.rgba[1], mat.rgba[2], (mat.opacity.clamp(0.0, 1.0) * 255.0).round() as u8],
                None => unpainted,
            };
            let n = (nm * DVec3::from(face.normal)).normalize_or_zero();
            let n = if n == DVec3::ZERO { DVec3::Z } else { n };
            let base = (layer.positions.len() / 3) as u32;
            for &p in &face.points {
                let w = transform_point(m, p);
                layer.positions.extend(w.map(|c| (c * INCH) as f32));
                layer.normals.extend([n.x as f32, n.y as f32, n.z as f32]);
                colors.push(rgba);
            }
            layer.indices.extend(face.triangles.iter().flat_map(|t| t.map(|i| base + i)));
        }
        if let Some(id) = element {
            let count = layer.indices.len() as u32 - first;
            if count > 0 {
                layer.elements.push((id, first, count));
            }
        }
    }
}

fn display_tag(tag: &str) -> &str {
    if tag == DEFAULT_TAG {
        "Untagged"
    } else {
        tag
    }
}

/// SketchUp's own names for unnamed definitions: "Group#12", "Component1#3".
fn is_generic_name(name: &str) -> bool {
    let rest = name.strip_prefix("Group").or_else(|| name.strip_prefix("Component"));
    let Some((digits, number)) = rest.and_then(|r| r.split_once('#')) else { return false };
    digits.bytes().all(|b| b.is_ascii_digit()) && !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())
}

/// Little-endian readers that return 0 past the end.
fn u16_at(data: &[u8], at: usize) -> u16 {
    data.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).unwrap_or(0)
}

fn u32_at(data: &[u8], at: usize) -> u32 {
    data.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).unwrap_or(0)
}

fn f64_at(data: &[u8], at: usize) -> f64 {
    data.get(at..at + 8).map(|b| f64::from_le_bytes(b.try_into().unwrap())).unwrap_or(0.0)
}

fn find_bytes(data: &[u8], needle: &[u8], start: usize, end: usize) -> Option<usize> {
    let end = end.min(data.len());
    if start >= end || needle.len() > end - start {
        return None;
    }
    data[start..end].windows(needle.len()).position(|w| w == needle).map(|i| start + i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn sample(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-models").join(name)
    }

    /// Statistics as openskp's instanced scene computes them: triangles with each definition
    /// counted once per inherited material and tag colour (drawn from one side, and from two
    /// sides where front and back are painted differently), and the size (metres, x/y/z) from
    /// each placement's local box corners. Hidden instances count too.
    struct Stats {
        single: usize,
        double: usize,
        lo: [f64; 3],
        hi: [f64; 3],
    }

    fn openskp_stats(parsed: &Parsed) -> (usize, usize, [f64; 3]) {
        type Variant = (Option<u64>, Option<u64>, [u8; 3]);
        #[allow(clippy::too_many_arguments)]
        fn walk(
            p: &Parsed,
            key: Option<u64>,
            def: &Definition,
            m: &Matrix,
            material: Option<u64>,
            tag: &str,
            seen: &mut HashSet<Variant>,
            s: &mut Stats,
        ) {
            let tag_color = p.tags.get(tag).map_or(DEFAULT_TAG_COLOR, |t| t.color);
            let first = seen.insert((key, material.filter(|id| p.materials.contains_key(id)), tag_color));
            let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
            for face in face_meshes(def) {
                if first {
                    let painted_differently = face.material != face.back_material;
                    s.single += face.triangles.len();
                    s.double += face.triangles.len() * if painted_differently { 2 } else { 1 };
                }
                for pt in &face.points {
                    for k in 0..3 {
                        lo[k] = lo[k].min(pt[k]);
                        hi[k] = hi[k].max(pt[k]);
                    }
                }
            }
            if lo[0].is_finite() {
                for c in 0..8 {
                    let corner = [0, 1, 2].map(|k| if c & (1 << k) != 0 { hi[k] } else { lo[k] });
                    let w = transform_point(m, corner);
                    for k in 0..3 {
                        s.lo[k] = s.lo[k].min(w[k] * INCH);
                        s.hi[k] = s.hi[k].max(w[k] * INCH);
                    }
                }
            }
            for inst in &def.instances {
                if let Some(child) = inst.definition.and_then(|d| p.definitions.get(&d)) {
                    let tag = inst.tag.and_then(|t| p.tag_names.get(&t)).filter(|t| !t.is_empty()).map_or(tag, String::as_str);
                    let material = inst.material.filter(|id| p.materials.contains_key(id)).or(material);
                    walk(p, inst.definition, child, &multiply(m, &inst.matrix), material, tag, seen, s);
                }
            }
        }
        let mut s = Stats { single: 0, double: 0, lo: [f64::INFINITY; 3], hi: [f64::NEG_INFINITY; 3] };
        walk(parsed, None, &parsed.root, &IDENTITY, None, DEFAULT_TAG, &mut HashSet::new(), &mut s);
        (s.single, s.double, [0, 1, 2].map(|k| s.hi[k] - s.lo[k]))
    }

    /// The samples against openskp's results (test-models/skp-expected.json, and openskp 1.4.0
    /// for solar-north-house and test-models/legacy: triangles and glTF size, which is x, z, y
    /// here). openskp splits a face in two where its resolved front and back colours differ, so
    /// its triangle count lies between the one- and two-sided count. test-models/legacy holds
    /// SketchUp 3 to 8 and 2018/2020 variants (see its README).
    #[test]
    fn samples_match_openskp() {
        let expected = [
            ("solar-north-house.skp", 29, [10.973, 5.639, 9.449]),
            ("legacy/openskp_legacy_v3_textured.skp", 4, [0.508, 0.0, 0.508]),
            ("legacy/openskp_legacy_v4_textured.skp", 4, [0.508, 0.0, 0.508]),
            ("legacy/openskp_legacy_v5_textured.skp", 4, [0.508, 0.0, 0.508]),
            ("legacy/openskp_nesting_v6.skp", 6, [1.27, 0.0, 0.254]),
            ("legacy/testup_skp-v6.skp", 12, [1.001, 0.551, 1.2]),
            ("legacy/quadface_uv-arch_v8.skp", 220, [13.553, 4.252, 1.05]),
            ("legacy/openskp_sdk_saved_2020.skp", 2, [0.254, 0.0, 0.254]),
            ("2021_lc.skp", 3905, [2.774, 2.15, 1.349]),
            ("TestModel.skp", 262, [14.0, 5.0, 17.0]),
            ("bathroom-3d.skp", 38623, [3.274, 2.698, 2.054]),
            ("cnc-toolbox.skp", 5872, [0.584, 0.408, 0.281]),
            ("colorize-example.skp", 8, [8.705, 0.0, 5.705]),
            ("instructor.skp", 24, [1.779, 1.779, 2.882]),
            ("jinyi.skp", 1079, [2.294, 3.053, 0.402]),
            ("skp2gltf-test.skp", 1500, [15.704, 2.955, 16.845]),
            ("solar-pv-80kw-design.skp", 36190, [424.298, 21.871, 224.6]),
            ("test-model-project-to-face.skp", 1527, [2.08, 1.902, 1.722]),
            ("test-kavel.skp", 95206, [29.158, 10.676, 37.254]),
        ];
        let mut failures = Vec::new();
        for (name, tris, size) in expected {
            let data = std::fs::read(sample(name)).unwrap();
            let parsed = match parse(&data) {
                Ok(p) => p,
                Err(e) => {
                    failures.push(format!("{name}: {e}"));
                    continue;
                }
            };
            let (single, double, got) = openskp_stats(&parsed);
            let got_gltf = [got[0], got[2], got[1]];
            let size_ok = (0..3).all(|k| (got_gltf[k] - size[k]).abs() < 0.002);
            // earcutr and openskp's earcut 3 can differ on degenerate polygons (cnc-toolbox: 4
            // triangles of 5872), hence the 0.2% margin
            if !size_ok || !(single..=double + double / 500).contains(&tris) {
                failures.push(format!(
                    "{name}: {single}..{double} triangles (openskp {tris}), size {got_gltf:.3?} (openskp {size:?})"
                ));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Files without faces that openskp 1.4.0 cannot read (SFSASM-test: a text whose delimiter
    /// block ends in 0) or reads without geometry: they parse, with their edges and instances.
    #[test]
    fn faceless_samples_parse() {
        let data = std::fs::read(sample("SFSASM-test.skp")).unwrap();
        let p = parse(&data).expect("SFSASM-test.skp");
        let instances: usize = p.definitions.values().chain([&p.root]).map(|d| d.instances.len()).sum();
        assert_eq!(instances, 5);
        let data = std::fs::read(sample("legacy/openskp_zero_material_custom_layer_v18.skp")).unwrap();
        assert!(parse(&data).is_ok());
    }

    /// Parses every .skp below `SKP_DIR` and prints one line per file (version, result, triangles,
    /// size in metres). A survey tool for files outside the repository, hence ignored:
    /// `SKP_DIR=... cargo test --release skp_dir_survey -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn skp_dir_survey() {
        fn files(dir: &Path, out: &mut Vec<PathBuf>) {
            for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    files(&p, out);
                } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("skp")) {
                    out.push(p);
                }
            }
        }
        let root = PathBuf::from(std::env::var("SKP_DIR").expect("SKP_DIR"));
        let mut all = Vec::new();
        files(&root, &mut all);
        all.sort();
        let (mut ok, mut failed) = (0, 0);
        for f in &all {
            let data = std::fs::read(f).unwrap();
            let ver = legacy::version_major(&data).map_or("?".to_string(), |v| v.to_string());
            let name = f.strip_prefix(&root).unwrap_or(f).display();
            match std::panic::catch_unwind(|| parse(&data)) {
                Ok(Ok(p)) => {
                    let (single, _, size) = openskp_stats(&p);
                    let defs = p.definitions.values().chain([&p.root]);
                    let (edges, instances) = defs.fold((0, 0), |(e, i), d| (e + d.edges.len(), i + d.instances.len()));
                    ok += 1;
                    println!("OK   v{ver:<3} {name}  tris {single} edges {edges} instances {instances} size {size:.3?}");
                }
                Ok(Err(e)) => {
                    failed += 1;
                    println!("FAIL v{ver:<3} {name}  {}", e.chars().take(140).collect::<String>());
                }
                Err(_) => {
                    failed += 1;
                    println!("PANIC v{ver:<3} {name}");
                }
            }
        }
        println!("{ok} ok, {failed} failed");
    }

    #[test]
    fn loads_sample_into_tag_layers() {
        let model = super::super::load(&sample("TestModel.skp")).expect("skp");
        assert!(model.triangle_count() > 100, "{} triangles", model.triangle_count());
        assert!(model.bounds.is_valid());
        assert!(model.layers.iter().all(|l| l.colors.as_ref().is_some_and(|c| c.len() == l.vertex_count())));
    }

    #[test]
    fn generic_definition_names() {
        assert!(is_generic_name("Group#12"));
        assert!(is_generic_name("Component1#3"));
        assert!(!is_generic_name("Chair"));
        assert!(!is_generic_name("Group#"));
    }
}
