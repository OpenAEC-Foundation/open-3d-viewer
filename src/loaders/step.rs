//! STEP (`.stp`, `.step`; `.stpz` gzip-compressed or zipped) as CAD, MEP and steel suppliers send
//! it: AP203, AP214 and AP242, read by the pure-Rust `cadmpeg-codec-step` crate (Apache-2.0) into a
//! B-rep model, whose faces `cadmpeg-tessellate` triangulates within a chordal tolerance (planes,
//! cylinders, cones, spheres, tori, NURBS and other surfaces, trimmed and with holes). AP242
//! tessellated shapes (`TRIANGULATED_FACE`, `TRIANGULATED_SURFACE_SET`, ..) are drawn where a body
//! has no exact faces. The product structure is flattened: every placed part (an occurrence of a
//! product with a shape) is a pickable element with its product name, part number, description and
//! assembly path, placed with the transforms of its assembly; one layer per presentation layer
//! of the file, or else per product. Colours come from styled items on faces and bodies (and
//! appearance bindings), else the default grey; normals of the surfaces are kept (smooth curved
//! faces). Millimetres (or the file's unit) to metres, Z up. PMI, drawings, wireframes, points,
//! textures, hidden items and external references to other files are not drawn.
//!
//! IFC files are STEP files too: the IFC loader keeps `.ifc`; a `.stp` whose `FILE_SCHEMA` names
//! IFC is refused with a hint.

use super::{Loaded, Loader, Phase, Progress, Source, DEFAULT_COLOR};
use crate::model::{Layer, ProjectData};
use cadmpeg_codec_step::StepCodec;
use cadmpeg_ir::ids::{BodyId, FaceId, OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::{CadIr, Codec, DecodeOptions};
use cadmpeg_tessellate::{Options, Tessellator};
use glam::{DMat3, DMat4, DVec3};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read as _;

pub struct StepLoader;

impl Loader for StepLoader {
    fn name(&self) -> &'static str {
        "STEP"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["stp", "step", "stpz"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        let data = if data.starts_with(&[0x1f, 0x8b]) {
            let mut out = Vec::new();
            flate2::read::MultiGzDecoder::new(&data[..]).read_to_end(&mut out).map_err(|e| format!("STEP: cannot unpack the gzip file: {e}"))?;
            std::borrow::Cow::Owned(out)
        } else {
            data
        };
        let header = Header::read(&data);
        if !data.starts_with(b"PK\x03\x04") {
            let text = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&data);
            let start = text.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(0);
            if !text[start..].starts_with(b"ISO-10303-21") {
                return Err("STEP: expected an ISO-10303-21 file".into());
            }
        }
        if header.schema.iter().any(|s| s.to_ascii_uppercase().starts_with("IFC")) {
            return Err(format!("STEP: {} is an IFC file ({}); open it with the extension .ifc", source.name(), header.schema.join(", ")));
        }
        progress.set(Phase::Convert);
        let mut cursor = std::io::Cursor::new(data.as_ref());
        let decoded = StepCodec::default().decode(&mut cursor, &DecodeOptions::default()).map_err(|e| format!("STEP: {e}"))?;
        convert(decoded.ir(), &header)
    }
}

/// What the header says: schema and originating system.
#[derive(Default)]
struct Header {
    schema: Vec<String>,
    system: Option<String>,
}

impl Header {
    fn read(data: &[u8]) -> Self {
        let head = String::from_utf8_lossy(&data[..data.len().min(1 << 16)]);
        let end = head.find("ENDSEC").unwrap_or(head.len());
        let head = &head[..end];
        // the quoted strings of an entity in the header
        let strings = |name: &str| -> Vec<String> {
            let Some(at) = head.find(name) else { return Vec::new() };
            let rest = &head[at + name.len()..];
            let body = &rest[..rest.find(");").map_or(rest.len(), |e| e + 1)];
            body.split('\'').skip(1).step_by(2).map(str::to_string).collect()
        };
        let file_name = strings("FILE_NAME");
        Header {
            schema: strings("FILE_SCHEMA"),
            // FILE_NAME(name, time, (author), (organization), preprocessor, originating system, authorization)
            system: file_name.get(5).or(file_name.get(4)).filter(|s| !s.trim().is_empty()).cloned(),
        }
    }
}

/// A body's triangles in its own coordinates (millimetres).
#[derive(Default)]
struct Mesh {
    positions: Vec<DVec3>,
    normals: Vec<DVec3>,
    colors: Vec<[u8; 4]>,
    indices: Vec<u32>,
}

impl Mesh {
    fn push(&mut self, positions: impl Iterator<Item = DVec3>, normals: Vec<DVec3>, triangles: impl Iterator<Item = [usize; 3]>, rgba: [u8; 4]) {
        let base = self.positions.len();
        let positions: Vec<DVec3> = positions.collect();
        let n = positions.len();
        let mut flat = vec![DVec3::ZERO; n];
        let triangles: Vec<[usize; 3]> = triangles.filter(|t| t.iter().all(|&i| i < n)).collect();
        if normals.len() != n {
            // no surface normals: of the triangles around each vertex
            for t in &triangles {
                let c = (positions[t[1]] - positions[t[0]]).cross(positions[t[2]] - positions[t[0]]);
                t.iter().for_each(|&i| flat[i] += c);
            }
        }
        let normals = if normals.len() == n { normals } else { flat.into_iter().map(DVec3::normalize_or_zero).collect() };
        self.positions.extend(positions);
        self.normals.extend(normals);
        self.colors.extend(std::iter::repeat_n(rgba, n));
        self.indices.extend(triangles.iter().flat_map(|t| t.map(|i| (base + i) as u32)));
    }
}

fn rgba(color: cadmpeg_ir::topology::Color) -> [u8; 4] {
    [color.r(), color.g(), color.b(), color.a()].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn matrix(t: &cadmpeg_ir::transform::Transform) -> DMat4 {
    let r = t.rows();
    DMat4::from_cols_array_2d(&r).transpose()
}

struct Scene<'ir> {
    ir: &'ir CadIr,
    tessellator: Tessellator<'ir>,
    /// The faces of each body.
    faces: HashMap<&'ir BodyId, Vec<&'ir cadmpeg_ir::topology::Face>>,
    face_colors: HashMap<&'ir FaceId, [u8; 4]>,
    body_colors: HashMap<&'ir BodyId, [u8; 4]>,
    body_layers: HashMap<&'ir BodyId, &'ir str>,
    meshes: HashMap<&'ir BodyId, std::rc::Rc<Mesh>>,
    layers: BTreeMap<String, Layer>,
    written: HashSet<(String, u64)>,
    next_id: u64,
    refused: usize,
    tessellated: usize,
}

impl<'ir> Scene<'ir> {
    /// The triangles of a body: its faces, else its tessellation in the file.
    fn mesh(&mut self, body: &'ir BodyId) -> std::rc::Rc<Mesh> {
        if let Some(mesh) = self.meshes.get(body) {
            return mesh.clone();
        }
        let base = self.body_colors.get(body).copied().unwrap_or_else(|| {
            let c = DEFAULT_COLOR.map(|v| (v * 255.0).round() as u8);
            [c[0], c[1], c[2], 255]
        });
        let mut mesh = Mesh::default();
        for face in self.faces.get(body).cloned().unwrap_or_default() {
            match self.tessellator.face(&face.id) {
                Ok(face_mesh) => {
                    self.tessellated += 1;
                    let color = self.face_colors.get(&face.id).copied().unwrap_or(base);
                    let positions = face_mesh.positions.iter().map(|p| DVec3::new(p.x, p.y, p.z));
                    let normals = face_mesh.normals.iter().map(|n| DVec3::new(n.x, n.y, n.z)).collect();
                    mesh.push(positions, normals, face_mesh.triangles.iter().copied(), color);
                }
                Err(_) => self.refused += 1,
            }
        }
        if mesh.indices.is_empty() {
            for tessellation in self.ir.model.tessellations.iter().filter(|t| t.body.as_ref() == Some(body)) {
                push_tessellation(&mut mesh, tessellation, base);
            }
        }
        let mesh = std::rc::Rc::new(mesh);
        self.meshes.insert(body, mesh.clone());
        mesh
    }

    /// Adds a mesh placed by `world` (millimetres) to the layer as part of element `id`.
    fn emit(&mut self, layer_name: &str, mesh: &Mesh, world: DMat4, id: u64, props: &str) {
        if mesh.indices.is_empty() {
            return;
        }
        let layer = self.layers.entry(layer_name.to_string()).or_insert_with(|| Layer {
            name: layer_name.to_string(),
            color: [mesh.colors[0][0], mesh.colors[0][1], mesh.colors[0][2]].map(|v| v as f32 / 255.0),
            colors: Some(Vec::new()),
            ..Layer::default()
        });
        let to_metres = DMat4::from_scale(DVec3::splat(0.001)) * world;
        let normal_matrix = DMat3::from_mat4(world).inverse().transpose();
        let flip = world.determinant() < 0.0;
        let base = layer.vertex_count() as u32;
        let first = layer.indices.len() as u32;
        for (p, n) in mesh.positions.iter().zip(&mesh.normals) {
            layer.positions.extend(to_metres.transform_point3(*p).to_array().map(|v| v as f32));
            layer.normals.extend((normal_matrix * *n).normalize_or_zero().to_array().map(|v| v as f32));
        }
        layer.colors.as_mut().unwrap().extend_from_slice(&mesh.colors);
        for t in mesh.indices.chunks_exact(3) {
            let t = if flip { [t[0], t[2], t[1]] } else { [t[0], t[1], t[2]] };
            layer.indices.extend(t.map(|i| base + i));
        }
        let count = layer.indices.len() as u32 - first;
        match layer.elements.last_mut() {
            Some(last) if last.0 == id && last.1 + last.2 == first => last.2 += count,
            _ => layer.elements.push((id, first, count)),
        }
        if self.written.insert((layer_name.to_string(), id)) {
            layer.props.push((id, props.to_string()));
        }
    }
}

fn push_tessellation(mesh: &mut Mesh, tessellation: &cadmpeg_ir::tessellation::Tessellation, color: [u8; 4]) {
    let positions = tessellation.vertices().into_iter().map(|p| {
        let p = p.get();
        DVec3::new(p.x, p.y, p.z)
    });
    let normals = tessellation.vertex_normals().into_iter().map(|n| {
        let n = n.get();
        DVec3::new(n.x, n.y, n.z)
    });
    let triangles = tessellation.triangles().into_iter().map(|t| t.map(|i| i as usize));
    mesh.push(positions, normals.collect(), triangles, color);
}

fn convert(ir: &CadIr, header: &Header) -> Result<Loaded, String> {
    let model = &ir.model;
    // faces of each body: face -> shell -> region -> body
    let regions: HashMap<_, _> = model.regions.iter().map(|r| (&r.id, &r.body)).collect();
    let shells: HashMap<_, _> = model.shells.iter().filter_map(|s| Some((&s.id, *regions.get(&s.region)?))).collect();
    let mut faces: HashMap<&BodyId, Vec<_>> = HashMap::new();
    for face in &model.faces {
        if let Some(body) = shells.get(&face.shell) {
            faces.entry(*body).or_default().push(face);
        }
    }
    // colours: styled faces and bodies, then appearance bindings
    let appearances: HashMap<_, _> = model.appearances.iter().filter_map(|a| Some((&a.id, rgba(a.base_color?)))).collect();
    let mut face_colors: HashMap<&FaceId, [u8; 4]> = HashMap::new();
    let mut body_colors: HashMap<&BodyId, [u8; 4]> = HashMap::new();
    for binding in &model.appearance_bindings {
        let Some(color) = appearances.get(&binding.appearance) else { continue };
        match &binding.target {
            cadmpeg_ir::appearance::AppearanceTarget::Body(body) => {
                body_colors.insert(body, *color);
            }
            cadmpeg_ir::appearance::AppearanceTarget::Face(face) => {
                face_colors.insert(face, *color);
            }
            _ => {}
        }
    }
    for face in &model.faces {
        if let Some(color) = face.color {
            face_colors.insert(&face.id, rgba(color));
        }
    }
    for body in &model.bodies {
        if let Some(color) = body.color {
            body_colors.insert(&body.id, rgba(color));
        }
    }
    let mut body_layers = HashMap::new();
    for layer in &model.presentation_layers {
        for item in &layer.items {
            if let cadmpeg_ir::presentation::PresentationItem::Body { body } = item {
                body_layers.insert(body, layer.name.as_str());
            }
        }
    }
    // the chordal tolerance from the size of the model (millimetres)
    let mut min = DVec3::splat(f64::INFINITY);
    let mut max = DVec3::splat(f64::NEG_INFINITY);
    for point in &model.points {
        let p = point.position().get();
        min = min.min(DVec3::new(p.x, p.y, p.z));
        max = max.max(DVec3::new(p.x, p.y, p.z));
    }
    let size = if min.x.is_finite() { (max - min).length() } else { 1000.0 };
    // A face whose normals jump (a degree-1 NURBS) never meets the angle limit and refines up to
    // the vertex cap: a low cap keeps such faces from taking seconds each.
    let options = Options { chord: (size * 1e-3).clamp(0.01, 10.0), angle: 0.35, max_vertices: 10_000, ..Options::default() };
    let mut scene = Scene {
        ir,
        tessellator: Tessellator::new(ir, options),
        faces,
        face_colors,
        body_colors,
        body_layers,
        meshes: HashMap::new(),
        layers: BTreeMap::new(),
        written: HashSet::new(),
        next_id: 0,
        refused: 0,
        tessellated: 0,
    };

    // the assembly tree: occurrences under their parents
    let definitions: HashMap<&ProductDefinitionId, _> = model.product_definitions.iter().map(|d| (&d.id, d)).collect();
    let mut children: HashMap<Option<&OccurrenceId>, Vec<_>> = HashMap::new();
    for occurrence in &model.occurrences {
        let parent = match &occurrence.parent {
            cadmpeg_ir::products::OccurrenceParent::Occurrence { occurrence } => Some(occurrence),
            _ => None,
        };
        children.entry(parent).or_default().push(occurrence);
    }
    for list in children.values_mut() {
        list.sort_by_key(|o| o.ordinal);
    }
    let mut placed_bodies: HashSet<&BodyId> = HashSet::new();
    let mut stack: Vec<(&cadmpeg_ir::products::Occurrence, DMat4, String, usize)> =
        children.get(&None).into_iter().flatten().map(|o| (*o, DMat4::IDENTITY, String::new(), 0)).collect();
    stack.reverse();
    let mut parts = 0;
    while let Some((occurrence, parent, path, depth)) = stack.pop() {
        if depth > 64 || occurrence.visible == Some(false) {
            continue;
        }
        let scale = DVec3::from_array(occurrence.scale.map(|s| s.get()));
        let world = parent * matrix(&occurrence.transform) * DMat4::from_scale(scale);
        let definition = match &occurrence.prototype {
            cadmpeg_ir::products::PrototypeReference::Local { definition } => definitions.get(definition).copied(),
            _ => None,
        };
        let product = definition.and_then(|d| d.label.clone().or(d.source_name.clone())).unwrap_or_else(|| "Part".into());
        let name = occurrence.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| product.clone());
        let here = if path.is_empty() { name.clone() } else { format!("{path}/{name}") };
        if let Some(definition) = definition.filter(|d| !d.bodies.is_empty()) {
            scene.next_id += 1;
            parts += 1;
            let id = scene.next_id;
            let mut props = serde_json::Map::new();
            props.insert("Name".into(), name.clone().into());
            props.insert("Product".into(), product.clone().into());
            if let Some(number) = definition.part_number.as_ref().filter(|n| **n != product) {
                props.insert("Part number".into(), number.clone().into());
            }
            if let Some(description) = definition.description.as_ref().filter(|d| !d.trim().is_empty()) {
                props.insert("Description".into(), description.clone().into());
            }
            props.insert("Assembly path".into(), here.clone().into());
            for body_id in &definition.bodies {
                placed_bodies.insert(body_id);
                let Some(body) = model.bodies.iter().find(|b| &b.id == body_id) else { continue };
                if body.visible == Some(false) {
                    continue;
                }
                let layer = scene.body_layers.get(body_id).map_or(product.clone(), |l| l.to_string());
                let mut props = props.clone();
                if let Some(layer) = scene.body_layers.get(body_id) {
                    props.insert("Layer".into(), (*layer).into());
                }
                if let Some(body_name) = body.name.as_ref().filter(|n| !n.trim().is_empty()) {
                    props.insert("Body".into(), body_name.clone().into());
                }
                let local = body.transform.as_ref().map_or(DMat4::IDENTITY, matrix);
                let mesh = scene.mesh(body_id);
                scene.emit(&layer, &mesh, world * local, id, &serde_json::Value::Object(props).to_string());
            }
        }
        if let Some(list) = children.get(&Some(&occurrence.id)) {
            stack.extend(list.iter().rev().map(|o| (*o, world, here.clone(), depth + 1)));
        }
    }
    // bodies outside the product structure, and tessellations of no body, where the file has them
    for body in &model.bodies {
        if placed_bodies.contains(&body.id) || body.visible == Some(false) {
            continue;
        }
        scene.next_id += 1;
        let id = scene.next_id;
        let name = body.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| format!("Body {id}"));
        let layer = scene.body_layers.get(&body.id).map_or("Bodies".to_string(), |l| l.to_string());
        let props = serde_json::json!({ "Name": name }).to_string();
        let local = body.transform.as_ref().map_or(DMat4::IDENTITY, matrix);
        let mesh = scene.mesh(&body.id);
        scene.emit(&layer, &mesh, local, id, &props);
    }
    for (k, tessellation) in model.tessellations.iter().filter(|t| t.body.is_none()).enumerate() {
        scene.next_id += 1;
        let id = scene.next_id;
        let mut mesh = Mesh::default();
        let c = DEFAULT_COLOR.map(|v| (v * 255.0).round() as u8);
        push_tessellation(&mut mesh, tessellation, [c[0], c[1], c[2], 255]);
        let props = serde_json::json!({ "Name": format!("Tessellation {}", k + 1) }).to_string();
        scene.emit("Tessellations", &mesh, DMat4::IDENTITY, id, &props);
    }

    let mut layers: Vec<Layer> = scene.layers.into_values().collect();
    for layer in &mut layers {
        // one colour for the whole layer: no vertex colours
        let single = layer.colors.as_ref().and_then(|c| c.windows(2).all(|w| w[0] == w[1]).then(|| c.first().copied()).flatten());
        if let Some(c) = single.filter(|c| c[3] == 255) {
            layer.color = [c[0], c[1], c[2]].map(|v| v as f32 / 255.0);
            layer.colors = None;
        }
    }
    if layers.is_empty() {
        let reason = if scene.refused > 0 { format!("none of its {} faces could be triangulated", scene.refused) } else { "no solids, surfaces or tessellations found".into() };
        return Err(format!("STEP: {reason}"));
    }
    let mut info = vec![("STEP schema".to_string(), header.schema.join(", "))];
    if let Some(system) = &header.system {
        info.push(("STEP originating system".into(), system.clone()));
    }
    info.push(("STEP parts placed".into(), parts.to_string()));
    info.push(("STEP faces".into(), scene.tessellated.to_string()));
    if scene.refused > 0 {
        info.push(("STEP faces not triangulated".into(), scene.refused.to_string()));
    }
    Ok(Loaded { layers, views: Vec::new(), project: ProjectData { info, ..ProjectData::default() } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Writes STEP physical files (ISO 10303-21) entity by entity.
    struct Step {
        lines: Vec<String>,
        /// The representation context (millimetres) and the product context.
        context: usize,
        product_context: usize,
        definition_context: usize,
        origin: usize,
        dz: usize,
        dx: usize,
    }

    impl Step {
        pub fn new() -> Self {
            let mut s = Step { lines: Vec::new(), context: 0, product_context: 0, definition_context: 0, origin: 0, dz: 0, dx: 0 };
            let app = s.add("APPLICATION_CONTEXT('automotive design')".into());
            s.add(format!("APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,#{app})"));
            s.product_context = s.add(format!("PRODUCT_CONTEXT('',#{app},'mechanical')"));
            s.definition_context = s.add(format!("PRODUCT_DEFINITION_CONTEXT('part definition',#{app},'design')"));
            let mm = s.add("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))".into());
            let rad = s.add("(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.))".into());
            let sr = s.add("(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT())".into());
            let accuracy = s.add(format!("UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-07),#{mm},'distance_accuracy_value','confusion accuracy')"));
            s.context = s.add(format!(
                "(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{accuracy}))GLOBAL_UNIT_ASSIGNED_CONTEXT((#{mm},#{rad},#{sr}))REPRESENTATION_CONTEXT('Context','3D'))"
            ));
            s.origin = s.point([0.0; 3]);
            s.dz = s.direction([0.0, 0.0, 1.0]);
            s.dx = s.direction([1.0, 0.0, 0.0]);
            s
        }

        pub fn add(&mut self, entity: String) -> usize {
            self.lines.push(entity);
            self.lines.len()
        }

        pub fn point(&mut self, p: [f64; 3]) -> usize {
            self.add(format!("CARTESIAN_POINT('',({:?},{:?},{:?}))", p[0], p[1], p[2]))
        }

        pub fn direction(&mut self, d: [f64; 3]) -> usize {
            self.add(format!("DIRECTION('',({:?},{:?},{:?}))", d[0], d[1], d[2]))
        }

        pub fn axis(&mut self, at: [f64; 3], z: [f64; 3], x: [f64; 3]) -> usize {
            let (p, z, x) = (self.point(at), self.direction(z), self.direction(x));
            self.add(format!("AXIS2_PLACEMENT_3D('',#{p},#{z},#{x})"))
        }

        fn vertex(&mut self, p: [f64; 3]) -> usize {
            let p = self.point(p);
            self.add(format!("VERTEX_POINT('',#{p})"))
        }

        fn line_edge(&mut self, a: usize, b: usize, pa: [f64; 3], pb: [f64; 3]) -> usize {
            let d = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
            let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            let p = self.point(pa);
            let dir = self.direction(d.map(|v| v / len));
            let v = self.add(format!("VECTOR('',#{dir},{len:?})"));
            let line = self.add(format!("LINE('',#{p},#{v})"));
            self.add(format!("EDGE_CURVE('',#{a},#{b},#{line},.T.)"))
        }

        fn face(&mut self, edges: &[(usize, bool)], surface: usize) -> usize {
            let oriented: Vec<String> = edges
                .iter()
                .map(|&(e, forward)| format!("#{}", self.add(format!("ORIENTED_EDGE('',*,*,#{e},{})", if forward { ".T." } else { ".F." }))))
                .collect();
            let edge_loop = self.add(format!("EDGE_LOOP('',({}))", oriented.join(",")));
            let bound = self.add(format!("FACE_OUTER_BOUND('',#{edge_loop},.T.)"));
            self.add(format!("ADVANCED_FACE('',(#{bound}),#{surface},.T.)"))
        }

        fn plane(&mut self, at: [f64; 3], normal: [f64; 3], x: [f64; 3]) -> usize {
            let a = self.axis(at, normal, x);
            self.add(format!("PLANE('',#{a})"))
        }

        /// A box from the origin to `size` as a closed shell of six planar faces.
        pub fn block(&mut self, size: [f64; 3]) -> usize {
            let corner = |i: usize| [(i & 1) as f64 * size[0], (i >> 1 & 1) as f64 * size[1], (i >> 2 & 1) as f64 * size[2]];
            let vertices: Vec<usize> = (0..8).map(|i| self.vertex(corner(i))).collect();
            let mut edges = std::collections::HashMap::new();
            for a in 0..8usize {
                for bit in [1, 2, 4] {
                    let b = a | bit;
                    if b != a {
                        let e = self.line_edge(vertices[a], vertices[b], corner(a), corner(b));
                        edges.insert((a, b), e);
                    }
                }
            }
            // corners counterclockwise seen from outside, the outward normal, an in-plane direction
            let faces: [([usize; 4], [f64; 3]); 6] = [
                ([0, 2, 3, 1], [0.0, 0.0, -1.0]),
                ([4, 5, 7, 6], [0.0, 0.0, 1.0]),
                ([0, 1, 5, 4], [0.0, -1.0, 0.0]),
                ([2, 6, 7, 3], [0.0, 1.0, 0.0]),
                ([0, 4, 6, 2], [-1.0, 0.0, 0.0]),
                ([1, 3, 7, 5], [1.0, 0.0, 0.0]),
            ];
            let mut face_ids = Vec::new();
            for (corners, normal) in faces {
                let loop_edges: Vec<(usize, bool)> = (0..4)
                    .map(|k| {
                        let (a, b) = (corners[k], corners[(k + 1) % 4]);
                        if a < b { (edges[&(a, b)], true) } else { (edges[&(b, a)], false) }
                    })
                    .collect();
                let x = if normal[0] != 0.0 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
                let plane = self.plane(corner(corners[0]), normal, x);
                face_ids.push(self.face(&loop_edges, plane));
            }
            self.solid(&face_ids)
        }

        fn solid(&mut self, faces: &[usize]) -> usize {
            let list: Vec<String> = faces.iter().map(|f| format!("#{f}")).collect();
            let shell = self.add(format!("CLOSED_SHELL('',({}))", list.join(",")));
            self.add(format!("MANIFOLD_SOLID_BREP('',#{shell})"))
        }

        /// A cylinder of radius `r` and height `h` standing on the origin: a cylindrical face with a
        /// seam and two discs.
        pub fn cylinder(&mut self, r: f64, h: f64) -> usize {
            let (v0, v1) = (self.vertex([r, 0.0, 0.0]), self.vertex([r, 0.0, h]));
            let circle = |s: &mut Self, z: f64, v: usize| {
                let a = s.axis([0.0, 0.0, z], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
                let c = s.add(format!("CIRCLE('',#{a},{r:?})"));
                s.add(format!("EDGE_CURVE('',#{v},#{v},#{c},.T.)"))
            };
            let bottom = circle(self, 0.0, v0);
            let top = circle(self, h, v1);
            let seam = self.line_edge(v0, v1, [r, 0.0, 0.0], [r, 0.0, h]);
            let a = self.axis([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
            let surface = self.add(format!("CYLINDRICAL_SURFACE('',#{a},{r:?})"));
            let side = self.face(&[(bottom, true), (seam, true), (top, false), (seam, false)], surface);
            let bottom_plane = self.plane([0.0; 3], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]);
            let bottom_face = self.face(&[(bottom, false)], bottom_plane);
            let top_plane = self.plane([0.0, 0.0, h], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
            let top_face = self.face(&[(top, true)], top_plane);
            self.solid(&[side, bottom_face, top_face])
        }

        /// A part: product, definition and a B-rep shape representation with `items` and an origin
        /// placement. Returns (product definition, shape representation, origin placement).
        pub fn part(&mut self, name: &str, items: &[usize], kind: &str) -> (usize, usize, usize) {
            let product = self.add(format!("PRODUCT('{name}','{name}','',(#{}))", self.product_context));
            let formation = self.add(format!("PRODUCT_DEFINITION_FORMATION('','',#{product})"));
            let definition = self.add(format!("PRODUCT_DEFINITION('design','',#{formation},#{})", self.definition_context));
            let shape = self.add(format!("PRODUCT_DEFINITION_SHAPE('','',#{definition})"));
            let origin = self.add(format!("AXIS2_PLACEMENT_3D('',#{},#{},#{})", self.origin, self.dz, self.dx));
            let mut list: Vec<String> = items.iter().map(|i| format!("#{i}")).collect();
            list.push(format!("#{origin}"));
            let rep = self.add(format!("{kind}('{name}',({}),#{})", list.join(","), self.context));
            self.add(format!("SHAPE_DEFINITION_REPRESENTATION(#{shape},#{rep})"));
            (definition, rep, origin)
        }

        /// Places `child` in `parent` at `placement` (an AXIS2_PLACEMENT_3D in the parent).
        pub fn occurrence(&mut self, name: &str, parent: (usize, usize), child: (usize, usize, usize), placement: usize) {
            let usage = self.add(format!("NEXT_ASSEMBLY_USAGE_OCCURRENCE('{name}','{name}','',#{},#{},$)", parent.0, child.0));
            let shape = self.add(format!("PRODUCT_DEFINITION_SHAPE('','',#{usage})"));
            let transform = self.add(format!("ITEM_DEFINED_TRANSFORMATION('','',#{},#{placement})", child.2));
            let relation = self.add(format!(
                "(REPRESENTATION_RELATIONSHIP('','',#{},#{})REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION(#{transform})SHAPE_REPRESENTATION_RELATIONSHIP())",
                child.1, parent.1
            ));
            self.add(format!("CONTEXT_DEPENDENT_SHAPE_REPRESENTATION(#{relation},#{shape})"));
        }

        /// A surface colour on `item`.
        pub fn colour(&mut self, item: usize, rgb: [f64; 3]) -> usize {
            let c = self.add(format!("COLOUR_RGB('',{:?},{:?},{:?})", rgb[0], rgb[1], rgb[2]));
            let fill_colour = self.add(format!("FILL_AREA_STYLE_COLOUR('',#{c})"));
            let fill = self.add(format!("FILL_AREA_STYLE('',(#{fill_colour}))"));
            let area = self.add(format!("SURFACE_STYLE_FILL_AREA(#{fill})"));
            let side = self.add(format!("SURFACE_SIDE_STYLE('',(#{area}))"));
            let usage = self.add(format!("SURFACE_STYLE_USAGE(.BOTH.,#{side})"));
            let assignment = self.add(format!("PRESENTATION_STYLE_ASSIGNMENT((#{usage}))"));
            let styled = self.add(format!("STYLED_ITEM('color',(#{assignment}),#{item})"));
            self.add(format!("MECHANICAL_DESIGN_GEOMETRIC_PRESENTATION_REPRESENTATION('',(#{styled}),#{})", self.context))
        }

        pub fn file(&self, name: &str, schema: &str) -> String {
            let mut out = format!(
                "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('test'),'2;1');\nFILE_NAME('{name}','2026-11-05T00:00:00',('OpenAEC'),('OpenAEC'),'hand written','test','');\nFILE_SCHEMA(('{schema}'));\nENDSEC;\nDATA;\n"
            );
            for (i, line) in self.lines.iter().enumerate() {
                out.push_str(&format!("#{}={line};\n", i + 1));
            }
            out.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
            out
        }
    }

    const AP214: &str = "AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }";

    fn block() -> String {
        let mut s = Step::new();
        let solid = s.block([100.0, 50.0, 20.0]);
        s.part("Block", &[solid], "ADVANCED_BREP_SHAPE_REPRESENTATION");
        s.file("block.stp", AP214)
    }

    fn cylinder() -> String {
        let mut s = Step::new();
        let solid = s.cylinder(10.0, 40.0);
        s.part("Pin", &[solid], "ADVANCED_BREP_SHAPE_REPRESENTATION");
        s.file("cylinder.stp", "CONFIG_CONTROL_DESIGN")
    }

    fn load(name: &str, data: Vec<u8>) -> crate::model::Model {
        let source = Source::Bytes { name: name.into(), data: Arc::new(data) };
        crate::loaders::load_with(&source, &Progress::default()).expect("STEP")
    }

    fn close(a: [f32; 3], b: [f64; 3]) -> bool {
        a.iter().zip(b).all(|(a, b)| (*a as f64 - b).abs() < 1e-4)
    }

    /// Volume enclosed by a closed triangle list (divergence theorem).
    fn volume(layer: &Layer) -> f64 {
        let p = |i: u32| std::array::from_fn::<f64, 3, _>(|k| layer.positions[3 * i as usize + k] as f64);
        layer.indices.chunks_exact(3).map(|t| {
            let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
            a[0] * (b[1] * c[2] - b[2] * c[1]) + a[1] * (b[2] * c[0] - b[0] * c[2]) + a[2] * (b[0] * c[1] - b[1] * c[0])
        }).sum::<f64>() / 6.0
    }

    #[test]
    fn reads_a_block_in_metres_as_one_pickable_part() {
        let model = load("block.STP", block().into_bytes());
        assert_eq!(model.format, "STEP");
        assert_eq!(model.layers.len(), 1);
        let layer = &model.layers[0];
        assert_eq!(layer.name, "Block");
        assert_eq!(layer.triangle_count(), 12);
        assert!(close(model.bounds.min, [0.0; 3]) && close(model.bounds.max, [0.1, 0.05, 0.02]), "{:?}", model.bounds);
        assert!((volume(layer) - 1e-4).abs() < 1e-9, "closed and facing out: {}", volume(layer));
        assert_eq!(layer.elements.len(), 1);
        let props = &model.props[&layer.elements[0].0];
        assert!(props.contains("\"Name\":\"Block\"") && props.contains("\"Assembly path\":\"Block\""), "{props}");
        assert!(model.project.info.iter().any(|(k, v)| k == "STEP schema" && v.starts_with("AUTOMOTIVE_DESIGN")));
    }

    #[test]
    fn triangulates_a_cylinder_with_smooth_normals() {
        let model = load("pin.step", cylinder().into_bytes());
        let layer = &model.layers[0];
        assert!(layer.triangle_count() > 40, "{}", layer.triangle_count());
        assert!(close(model.bounds.min, [-0.01, -0.01, 0.0]) && close(model.bounds.max, [0.01, 0.01, 0.04]), "{:?}", model.bounds);
        let exact = std::f64::consts::PI * 0.01 * 0.01 * 0.04;
        assert!((volume(layer) / exact - 1.0).abs() < 0.02, "{} vs {exact}", volume(layer));
        // the side keeps the surface normals: horizontal, pointing out
        let side = layer.positions.chunks_exact(3).zip(layer.normals.chunks_exact(3)).filter(|(p, _)| p[2] > 0.001 && p[2] < 0.039);
        for (p, n) in side {
            assert!(n[2].abs() < 1e-3 && (p[0] * n[0] + p[1] * n[1]) > 0.0, "{p:?} {n:?}");
        }
    }

    #[test]
    fn places_assembly_parts_with_colours_and_layers() {
        let mut s = Step::new();
        let block = s.block([100.0, 50.0, 20.0]);
        s.colour(block, [1.0, 0.0, 0.0]);
        let block_part = s.part("Block", &[block], "ADVANCED_BREP_SHAPE_REPRESENTATION");
        let pin = s.cylinder(10.0, 40.0);
        s.add(format!("PRESENTATION_LAYER_ASSIGNMENT('Steel','',(#{pin}))"));
        let pin_part = s.part("Pin", &[pin], "ADVANCED_BREP_SHAPE_REPRESENTATION");
        let at_origin = s.axis([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        // the pin lies along x, 120 mm along and in the middle of the block's side
        let along_x = s.axis([120.0, 25.0, 10.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let second = s.axis([0.0, 0.0, 20.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        let assembly = s.part("Bracket", &[at_origin, along_x, second], "SHAPE_REPRESENTATION");
        s.occurrence("Block:1", (assembly.0, assembly.1), block_part, at_origin);
        s.occurrence("Pin:1", (assembly.0, assembly.1), pin_part, along_x);
        // the same block again, on top of the first
        s.occurrence("Block:2", (assembly.0, assembly.1), block_part, second);
        let model = load("bracket.step", s.file("bracket.step", "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }").into_bytes());
        let names: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Block", "Steel"]);
        let block = &model.layers[0];
        assert_eq!(block.color, [1.0, 0.0, 0.0]);
        assert_eq!(block.triangle_count(), 24);
        assert_eq!(block.elements.len(), 2);
        let pin = &model.layers[1];
        let (lo, hi) = pin.positions.chunks_exact(3).fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), p| {
            (std::array::from_fn(|k| lo[k].min(p[k])), std::array::from_fn(|k| hi[k].max(p[k])))
        });
        assert!(close(lo, [0.12, 0.015, 0.0]) && close(hi, [0.16, 0.035, 0.02]), "{lo:?} {hi:?}");
        assert!(close(model.bounds.max, [0.16, 0.05, 0.04]), "{:?}", model.bounds);
        let props: Vec<&String> = model.props.values().collect();
        assert!(props.iter().any(|p| p.contains("\"Name\":\"Pin:1\"") && p.contains("\"Assembly path\":\"Bracket/Pin:1\"") && p.contains("\"Layer\":\"Steel\"")), "{props:?}");
        assert!(props.iter().any(|p| p.contains("\"Name\":\"Block:2\"") && p.contains("\"Product\":\"Block\"")), "{props:?}");
    }

    #[test]
    fn draws_tessellated_shapes_and_gzip_files() {
        let mut s = Step::new();
        let coordinates = s.add("COORDINATES_LIST('',4,((0.,0.,0.),(30.,0.,0.),(30.,20.,0.),(0.,20.,0.)))".into());
        let set = s.add(format!("TRIANGULATED_SURFACE_SET('plate',#{coordinates},0,$,(),((1,2,3),(1,3,4)))"));
        s.part("Plate", &[set], "TESSELLATED_SHAPE_REPRESENTATION");
        let model = load("plate.stp", s.file("plate.stp", "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }").into_bytes());
        assert_eq!(model.triangle_count(), 2);
        assert!(close(model.bounds.max, [0.03, 0.02, 0.0]), "{:?}", model.bounds);
        assert!(model.layers[0].normals.chunks_exact(3).all(|n| n[2] > 0.99));
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, block().as_bytes()).unwrap();
        let model = load("block.stpz", gz.finish().unwrap());
        assert_eq!(model.triangle_count(), 12);
    }

    #[test]
    fn leaves_ifc_to_the_ifc_loader() {
        let ifc = block().replace(AP214, "IFC4");
        let source = Source::Bytes { name: "model.stp".into(), data: Arc::new(ifc.into_bytes()) };
        let error = StepLoader.load(&source).unwrap_err();
        assert!(error.contains("IFC file"), "{error}");
        let source = Source::Bytes { name: "x.step".into(), data: Arc::new(b"solid x\nendsolid x\n".to_vec()) };
        assert!(StepLoader.load(&source).unwrap_err().contains("ISO-10303-21"));
        let registry = crate::loaders::registry();
        let loader = |ext: &str| registry.iter().find(|l| l.extensions().contains(&ext)).map(|l| l.name());
        assert_eq!(loader("stp"), Some("STEP"));
        assert_eq!(loader("step"), Some("STEP"));
        #[cfg(not(target_arch = "wasm32"))]
        assert_eq!(loader("ifc"), Some("IFC"));
    }

    /// Loads every STEP file under `O3V_STEP_DIR` and prints what came through, for timings on
    /// files that are not in the repository: `cargo test --release step_folder -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn step_folder() {
        let Ok(dir) = std::env::var("O3V_STEP_DIR") else { return };
        let mut files = Vec::new();
        let mut dirs = vec![std::path::PathBuf::from(dir)];
        while let Some(d) = dirs.pop() {
            for entry in std::fs::read_dir(d).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if StepLoader.extensions().contains(&Source::File(path.clone()).extension().as_str()) {
                    files.push(path);
                }
            }
        }
        files.sort();
        let (mut ok, mut failed) = (0, 0);
        for path in files {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            match crate::loaders::load(&path) {
                Ok(model) => {
                    ok += 1;
                    let info = |key: &str| model.project.info.iter().find(|(k, _)| k == key).map_or("0".to_string(), |(_, v)| v.clone());
                    println!("ok   {:>6.2} s {:>7} KB {:>8} tris {:>3} layers faces {} skipped {}  {}", model.load_time.as_secs_f32(), size / 1024, model.triangle_count(), model.layers.len(), info("STEP faces"), info("STEP faces not triangulated"), path.display());
                }
                Err(e) => {
                    failed += 1;
                    println!("FAIL {e}  {}", path.display());
                }
            }
        }
        println!("{ok} opened, {failed} failed");
    }
}
