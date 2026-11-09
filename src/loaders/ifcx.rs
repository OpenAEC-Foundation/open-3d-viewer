//! IFC5 / IFCX (`.ifcx`), buildingSMART's JSON format in development (`ifcx_alpha`), written from
//! its public schema (`schema/ifcx.tsp`) and the composition rules of its MIT-licensed reference
//! viewer (IFC5-development). A file is a list of nodes `{path, children, inherits, attributes}`;
//! nodes with the same path are layered (later wins, `null` removes a child, inherit or
//! attribute; a child taken out is left out, not drawn as a root of its own), `children` and `inherits` pull in other nodes (an occurrence inherits the children
//! and attributes of its type and material), and a path `a/b` adds to child `b` of `a`. Several
//! files compose the same way, the later stronger: local `imports` (a relative `.ifcx` next to the
//! file, desktop only) go under the file, and a file added to an open IFCX model goes on top of it.
//!
//! Every node with a `bsi::ifc::class` is a pickable element with its name, class, tree path and
//! attributes (`bsi::ifc::prop::*` without their prefix, its material, other namespaces as they
//! are); one layer per class, spaces, openings and virtual elements hidden as in IFC. Meshes
//! (`usd::usdgeom::mesh`: `points`, `faceVertexIndices`, optional `faceVertexCounts`) are placed with
//! the `usd::xformop` transforms of their node and its parents, coloured by the nearest
//! `bsi::ifc::presentation::diffuseColor` and `opacity` (or a glTF `baseColorFactor`), flat
//! shaded; `invisible` nodes are left out. Point clouds (`points::array`, `points::base64`) are
//! drawn as points. Metres, Z up. Curves (`usd::usdgeom::basiscurves`), `pcd::base64` clouds,
//! textures, georeferencing and imports from the web (the schema files on ifcx.dev) are not read.

use super::{Loaded, Loader, Phase, Progress, Source, DEFAULT_COLOR};
use crate::model::{Layer, ProjectData};
use glam::{DMat4, DVec3};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

pub struct IfcxLoader;

impl Loader for IfcxLoader {
    fn name(&self) -> &'static str {
        "IFCX"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["ifcx"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        self.load_stack(std::slice::from_ref(source), progress)
    }

    fn composes(&self) -> bool {
        true
    }

    fn load_stack(&self, sources: &[Source], progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let mut files = Vec::new();
        let mut seen = HashSet::new();
        for source in sources {
            read_with_imports(source, &mut files, &mut seen, 0)?;
        }
        progress.set(Phase::Convert);
        convert(&files)
    }
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    header: Option<Header>,
    #[serde(default)]
    imports: Option<Vec<Import>>,
    data: Vec<Node>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Header {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    ifcx_version: Option<String>,
    #[serde(default)]
    author: Option<String>,
}

#[derive(Deserialize)]
struct Import {
    uri: String,
}

#[derive(Deserialize)]
struct Node {
    path: String,
    #[serde(default)]
    children: Option<BTreeMap<String, Option<String>>>,
    #[serde(default)]
    inherits: Option<BTreeMap<String, Option<String>>>,
    #[serde(default)]
    attributes: Option<serde_json::Map<String, Value>>,
}

/// One read file and its name.
struct Read {
    name: String,
    file: File,
}

fn parse(source: &Source) -> Result<File, String> {
    let data = source.read()?;
    let data = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&data);
    serde_json::from_slice::<File>(data).map_err(|e| format!("IFCX: {} is not an IFCX file ({e})", source.name()))
}

/// Reads a file after the local files it imports (they are layered under it).
fn read_with_imports(source: &Source, files: &mut Vec<Read>, seen: &mut HashSet<String>, depth: usize) -> Result<(), String> {
    let key = source.path().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| source.name());
    if depth > 16 || !seen.insert(key) {
        return Ok(());
    }
    let file = parse(source)?;
    if let Some(dir) = source.path().and_then(|p| p.parent()) {
        for import in file.imports.iter().flatten() {
            let uri = import.uri.trim();
            if uri.contains("://") || !uri.to_ascii_lowercase().ends_with(".ifcx") {
                continue;
            }
            let path = dir.join(uri.trim_start_matches("./"));
            if path.is_file() {
                read_with_imports(&Source::File(path), files, seen, depth + 1)?;
            }
        }
    }
    files.push(Read { name: source.name(), file });
    Ok(())
}

/// The data of all nodes with one path, layered.
#[derive(Default)]
struct Flat {
    /// `None` removes the child the weaker layers gave.
    children: BTreeMap<String, Option<String>>,
    inherits: BTreeMap<String, String>,
    attributes: BTreeMap<String, Rc<Value>>,
}

/// A node after composition: its attributes and its children by name.
#[derive(Clone, Default)]
struct Composed {
    /// The path of the node it was made from.
    source: Option<String>,
    attributes: BTreeMap<String, Rc<Value>>,
    children: Vec<(String, Rc<Composed>)>,
}

impl Composed {
    fn attribute(&self, name: &str) -> Option<&Value> {
        self.attributes.get(name).map(|v| v.as_ref()).filter(|v| !v.is_null())
    }

    fn set_child(&mut self, name: &str, child: Rc<Composed>) {
        match self.children.iter_mut().find(|(n, _)| n == name) {
            Some(slot) => slot.1 = child,
            None => self.children.push((name.to_string(), child)),
        }
    }
}

struct Composer<'a> {
    flat: &'a HashMap<String, Flat>,
    /// The paths under which a node with a deeper path (`a/b/c`) adds data: `a` and `a/b`.
    deep: HashSet<String>,
    memo: HashMap<String, Rc<Composed>>,
    busy: HashSet<String>,
}

impl Composer<'_> {
    fn compose(&mut self, path: &str) -> Rc<Composed> {
        if let Some(node) = self.memo.get(path) {
            return node.clone();
        }
        // a node that (indirectly) holds or inherits itself: left empty there
        if !self.busy.insert(path.to_string()) {
            return Rc::default();
        }
        let mut node = Composed { source: Some(path.to_string()), ..Composed::default() };
        if let Some(flat) = self.flat.get(path) {
            self.add(flat, &mut node);
        }
        self.overrides(path, &mut node);
        self.busy.remove(path);
        let node = Rc::new(node);
        self.memo.insert(path.to_string(), node.clone());
        node
    }

    /// The node at `head/child/..`.
    fn resolve(&mut self, target: &str) -> Option<Rc<Composed>> {
        let mut parts = target.split('/').filter(|p| !p.is_empty());
        let mut node = self.compose(parts.next()?);
        for part in parts {
            node = node.children.iter().find(|(name, _)| name == part)?.1.clone();
        }
        Some(node)
    }

    /// Inherited children and attributes, then the node's own children and attributes.
    fn add(&mut self, flat: &Flat, node: &mut Composed) {
        for target in flat.inherits.values() {
            let Some(base) = self.resolve(target) else { continue };
            for (name, child) in &base.children {
                node.set_child(name, child.clone());
            }
            for (key, value) in &base.attributes {
                node.attributes.insert(key.clone(), value.clone());
            }
        }
        for (name, child) in &flat.children {
            match child {
                Some(target) => {
                    if let Some(child) = self.resolve(target) {
                        node.set_child(name, child);
                    }
                }
                None => node.children.retain(|(n, _)| n != name),
            }
        }
        for (key, value) in &flat.attributes {
            match value.is_null() {
                true => node.attributes.remove(key),
                false => node.attributes.insert(key.clone(), value.clone()),
            };
        }
    }

    /// Adds the nodes with a deeper path (`path/child/..`) to the children of `node`.
    fn overrides(&mut self, path: &str, node: &mut Composed) {
        if !self.deep.contains(path) {
            return;
        }
        let flat = self.flat;
        for i in 0..node.children.len() {
            let child_path = format!("{path}/{}", node.children[i].0);
            let own = flat.get(&child_path);
            if own.is_none() && !self.deep.contains(&child_path) {
                continue;
            }
            let mut child = (*node.children[i].1).clone();
            if let Some(own) = own {
                self.add(own, &mut child);
            }
            self.overrides(&child_path, &mut child);
            node.children[i].1 = Rc::new(child);
        }
    }
}

/// Geometry that marks out a volume instead of something built: starts hidden, as in IFC.
fn helper(class: &str) -> bool {
    ["IfcSpace", "IfcSpatialZone", "IfcExternalSpatialElement", "IfcOpeningElement", "IfcOpeningStandardCase", "IfcVoidingFeature", "IfcVirtualElement"]
        .iter()
        .any(|c| c.eq_ignore_ascii_case(class))
}

/// `[[a, b, c, d], ..]` with the translation in the last row (as USD): read as columns, it is ours.
fn transform(node: &Composed) -> Option<DMat4> {
    let rows = node.attribute("usd::xformop")?.get("transform")?.as_array()?;
    let m: Vec<f64> = rows.iter().flat_map(|r| r.as_array().into_iter().flatten()).filter_map(Value::as_f64).collect();
    (m.len() == 16).then(|| DMat4::from_cols_slice(&m)).filter(|m| m.is_finite())
}

fn invisible(node: &Composed) -> bool {
    let v = node.attribute("usd::usdgeom::visibility");
    v.and_then(|v| v.get("visibility")).or(v).and_then(Value::as_str) == Some("invisible")
}

/// The colour (RGBA, 0..1) a node sets for itself and its children.
fn color(node: &Composed) -> Option<[f32; 4]> {
    let number = |v: &Value| v.as_f64().or_else(|| v.as_str()?.trim().parse().ok());
    let rgb = |v: &Value| -> Option<[f32; 4]> {
        let c: Vec<f64> = v.as_array()?.iter().filter_map(number).collect();
        (c.len() >= 3).then(|| [c[0], c[1], c[2], c.get(3).copied().unwrap_or(1.0)].map(|v| v.clamp(0.0, 1.0) as f32))
    };
    if let Some(c) = node.attribute("gltf::material").and_then(|m| m.get("pbrMetallicRoughness")?.get("baseColorFactor")).and_then(rgb) {
        return Some(c);
    }
    let mut c = node.attribute("bsi::ifc::presentation::diffuseColor").and_then(rgb)?;
    if let Some(opacity) = node.attribute("bsi::ifc::presentation::opacity").and_then(number) {
        c[3] = opacity.clamp(0.0, 1.0) as f32;
    }
    Some(c)
}

fn class(node: &Composed) -> Option<String> {
    let class = node.attribute("bsi::ifc::class")?;
    class.get("code").unwrap_or(class).as_str().map(str::to_string)
}

/// A storey as a level: its `Name` (else its name as a child) and its `Elevation` (else the Z of
/// its placement).
fn storey(node: &Composed, name: &str, world: DMat4) -> (String, f32) {
    let own_name = node.attribute("bsi::ifc::prop::Name").map(text).filter(|n| !n.trim().is_empty());
    let elevation = node.attribute("bsi::ifc::prop::Elevation").and_then(|v| v.as_f64().or_else(|| v.as_str()?.trim().parse().ok()));
    let z = elevation.filter(|z: &f64| z.is_finite()).unwrap_or(world.w_axis.z);
    (own_name.unwrap_or_else(|| name.to_string()), z as f32)
}

/// A value for the properties panel.
fn text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// The element's properties: name, class, tree path, source path and attributes.
fn properties(node: &Composed, name: &str, class: &str, tree: &str) -> String {
    let mut props = serde_json::Map::new();
    let own_name = node.attribute("bsi::ifc::prop::Name").map(text).filter(|n| !n.is_empty());
    props.insert("Name".into(), own_name.unwrap_or_else(|| name.to_string()).into());
    props.insert("Class".into(), class.into());
    props.insert("Path".into(), tree.into());
    if let Some(source) = &node.source {
        props.insert("Id".into(), source.clone().into());
    }
    for (key, value) in &node.attributes {
        let skip = ["usd::", "points::", "pcd::", "gltf::", "customdata", "__", "bsi::ifc::presentation::"];
        if value.is_null() || key == "bsi::ifc::class" || key == "bsi::ifc::prop::Name" || skip.iter().any(|s| key.starts_with(s)) {
            continue;
        }
        if key == "bsi::ifc::material" {
            props.insert("Material".into(), text(value.get("code").unwrap_or(value)).into());
            continue;
        }
        let key = key.strip_prefix("bsi::ifc::prop::").unwrap_or(key);
        match value.as_ref() {
            Value::Object(fields) => {
                for (field, v) in fields {
                    props.insert(format!("{key}::{field}"), text(v).into());
                }
            }
            v => {
                props.insert(key.to_string(), text(v).into());
            }
        }
    }
    Value::Object(props).to_string()
}

/// xyz triples of `[[x, y, z], ..]` or `[x, y, z, ..]`.
fn points(value: &Value) -> Vec<DVec3> {
    let Some(items) = value.as_array() else { return Vec::new() };
    let flat: Vec<f64> = match items.first() {
        Some(Value::Array(_)) => items.iter().flat_map(|p| p.as_array().into_iter().flatten().take(3)).filter_map(Value::as_f64).collect(),
        _ => items.iter().filter_map(Value::as_f64).collect(),
    };
    flat.chunks_exact(3).map(|c| DVec3::new(c[0], c[1], c[2])).collect()
}

fn ints(value: Option<&Value>) -> Vec<i64> {
    value.and_then(Value::as_array).map(|v| v.iter().filter_map(Value::as_i64).collect()).unwrap_or_default()
}

/// Standard base64 (no padding needed).
fn base64(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => continue,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

/// Little-endian 32-bit floats.
fn floats(bytes: &[u8]) -> Vec<f64> {
    bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64).collect()
}

fn to_rgba(c: [f32; 4]) -> [u8; 4] {
    c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

struct Element {
    id: u64,
    class: String,
    props: String,
}

struct Scene {
    layers: BTreeMap<(bool, String), Layer>,
    clouds: BTreeMap<String, Layer>,
    /// Elements whose properties a layer already holds.
    written: HashSet<(String, u64)>,
    next_id: u64,
    elements: usize,
    /// The storeys (`IfcBuildingStorey`): name and elevation.
    levels: Vec<(String, f32)>,
    /// The definitions of elements this viewer modelled (`openaec::element`).
    definitions: Vec<Value>,
}

impl Scene {
    #[allow(clippy::too_many_arguments)]
    fn walk(&mut self, node: &Composed, name: &str, tree: &str, parent: DMat4, color: Option<[f32; 4]>, element: Option<&Element>, depth: usize) {
        if depth > 256 || invisible(node) {
            return;
        }
        // a modelled element: its definitions (its own and its connections') come back, its mesh is
        // made from them again
        if let Some(definition) = node.attribute("openaec::element") {
            match definition {
                Value::Array(all) => self.definitions.extend(all.iter().cloned()),
                one => self.definitions.push(one.clone()),
            }
            return;
        }
        let world = transform(node).map_or(parent, |m| parent * m);
        let color = self::color(node).or(color);
        let own;
        let element = match class(node) {
            Some(class) => {
                if class.eq_ignore_ascii_case(crate::document::STOREY_CLASS) {
                    self.levels.push(storey(node, name, world));
                }
                self.next_id += 1;
                self.elements += 1;
                own = Element { id: self.next_id, props: properties(node, name, &class, tree), class };
                Some(&own)
            }
            None => element,
        };
        if let Some(mesh) = node.attribute("usd::usdgeom::mesh") {
            self.mesh(mesh, node, name, tree, world, color, element);
        }
        for kind in ["points::array", "points::base64"] {
            if let Some(cloud) = node.attribute(kind) {
                self.cloud(kind, cloud, name, world, color, element);
            }
        }
        for (child, node) in &node.children {
            self.walk(node, child, &format!("{tree}/{child}"), world, color, element, depth + 1);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn mesh(&mut self, mesh: &Value, node: &Composed, name: &str, tree: &str, world: DMat4, color: Option<[f32; 4]>, element: Option<&Element>) {
        let points: Vec<DVec3> = mesh.get("points").map(points).unwrap_or_default().into_iter().map(|p| world.transform_point3(p)).collect();
        let indices = ints(mesh.get("faceVertexIndices"));
        let mut counts = ints(mesh.get("faceVertexCounts"));
        if counts.is_empty() {
            counts = vec![3; indices.len() / 3];
        }
        if points.is_empty() || indices.is_empty() {
            return;
        }
        // a mesh outside any classified element is its own element
        let fallback;
        let element = match element {
            Some(e) => e,
            None => {
                self.next_id += 1;
                fallback = Element { id: self.next_id, class: "Unclassified".into(), props: properties(node, name, "", tree) };
                &fallback
            }
        };
        let rgba = to_rgba(color.unwrap_or([DEFAULT_COLOR[0], DEFAULT_COLOR[1], DEFAULT_COLOR[2], 1.0]));
        let hidden = helper(&element.class);
        let layer = self.layers.entry((hidden, element.class.clone())).or_insert_with(|| Layer {
            name: if hidden { format!("Helpers: {}", element.class) } else { element.class.clone() },
            color: [rgba[0], rgba[1], rgba[2]].map(|v| v as f32 / 255.0),
            colors: Some(Vec::new()),
            hidden,
            ..Layer::default()
        });
        let flip = world.determinant() < 0.0;
        let first = layer.indices.len() as u32;
        let mut start = 0usize;
        for &count in &counts {
            let count = count.max(0) as usize;
            let corners = indices.get(start..start + count);
            start += count;
            let Some(corners) = corners else { break };
            let Some(p) = corners.iter().map(|&i| points.get(usize::try_from(i).ok()?).copied()).collect::<Option<Vec<_>>>() else { continue };
            for k in 1..count.saturating_sub(1) {
                let mut t = [p[0], p[k], p[k + 1]];
                if flip {
                    t.swap(1, 2);
                }
                let n = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero();
                if n == DVec3::ZERO {
                    continue;
                }
                for v in t {
                    layer.indices.push(layer.vertex_count() as u32);
                    layer.positions.extend(v.to_array().map(|c| c as f32));
                    layer.normals.extend(n.to_array().map(|c| c as f32));
                    layer.colors.as_mut().unwrap().push(rgba);
                }
            }
        }
        let count = layer.indices.len() as u32 - first;
        if count == 0 {
            return;
        }
        match layer.elements.last_mut() {
            Some(last) if last.0 == element.id && last.1 + last.2 == first => last.2 += count,
            _ => layer.elements.push((element.id, first, count)),
        }
        if self.written.insert((layer.name.clone(), element.id)) {
            layer.props.push((element.id, element.props.clone()));
        }
    }

    fn cloud(&mut self, kind: &str, cloud: &Value, name: &str, world: DMat4, color: Option<[f32; 4]>, element: Option<&Element>) {
        let (positions, colors) = if kind == "points::array" {
            (cloud.get("positions").map(points).unwrap_or_default(), cloud.get("colors").map(points).unwrap_or_default())
        } else {
            let decode = |key: &str| -> Vec<DVec3> {
                let values = cloud.get(key).and_then(Value::as_str).map(|s| floats(&base64(s))).unwrap_or_default();
                values.chunks_exact(3).map(|c| DVec3::new(c[0], c[1], c[2])).collect()
            };
            (decode("positions"), decode("colors"))
        };
        if positions.is_empty() {
            return;
        }
        let base = to_rgba(color.unwrap_or([0.2, 0.2, 0.2, 1.0]));
        let label = element.map_or(name.to_string(), |e| e.class.clone());
        let layer = self.clouds.entry(label.clone()).or_insert_with(|| Layer {
            name: format!("Point cloud: {label}"),
            color: [base[0], base[1], base[2]].map(|v| v as f32 / 255.0),
            colors: Some(Vec::new()),
            points: true,
            ..Layer::default()
        });
        for (i, p) in positions.iter().enumerate() {
            let p = world.transform_point3(*p);
            if !p.is_finite() {
                continue;
            }
            layer.positions.extend(p.to_array().map(|c| c as f32));
            let c = colors.get(i).map_or(base, |c| to_rgba([c.x as f32, c.y as f32, c.z as f32, 1.0]));
            layer.colors.as_mut().unwrap().push(c);
        }
    }
}

/// Composes the files (the later stronger) and draws the tree.
fn convert(files: &[Read]) -> Result<Loaded, String> {
    let mut flat: HashMap<String, Flat> = HashMap::new();
    // roots: top-level paths that no node holds or inherits; also not a child a stronger layer
    // took out (that one is gone, not a root of its own)
    let mut used = HashSet::new();
    for read in files {
        for node in &read.file.data {
            let targets = node.children.iter().flatten().filter_map(|(_, t)| t.as_ref());
            let targets = targets.chain(node.inherits.iter().flatten().filter_map(|(_, t)| t.as_ref()));
            used.extend(targets.map(|t| t.split('/').next().unwrap_or(t).to_string()));
            let entry = flat.entry(node.path.clone()).or_default();
            for (name, child) in node.children.iter().flatten() {
                entry.children.insert(name.clone(), child.clone());
            }
            for (name, target) in node.inherits.iter().flatten() {
                match target {
                    Some(target) => entry.inherits.insert(name.clone(), target.clone()),
                    None => entry.inherits.remove(name),
                };
            }
            for (key, value) in node.attributes.iter().flatten() {
                entry.attributes.insert(key.clone(), Rc::new(value.clone()));
            }
        }
    }
    let mut deep = HashSet::new();
    for path in flat.keys() {
        let parts: Vec<&str> = path.split('/').collect();
        for n in 1..parts.len() {
            deep.insert(parts[..n].join("/"));
        }
    }
    let mut roots: Vec<&String> = flat.keys().filter(|p| !p.contains('/') && !used.contains(*p)).collect();
    roots.sort();
    if roots.is_empty() && !flat.is_empty() {
        return Err("IFCX: every node is held by another (a cycle); nothing to draw".into());
    }
    let mut composer = Composer { flat: &flat, deep, memo: HashMap::new(), busy: HashSet::new() };
    let mut scene = Scene { layers: BTreeMap::new(), clouds: BTreeMap::new(), written: HashSet::new(), next_id: 0, elements: 0, levels: Vec::new(), definitions: Vec::new() };
    for root in roots {
        let node = composer.compose(root);
        scene.walk(&node, root, root, DMat4::IDENTITY, None, None, 0);
    }
    let definitions = std::mem::take(&mut scene.definitions);
    let mut layers: Vec<Layer> = scene.layers.into_values().filter(|l| !l.indices.is_empty()).collect();
    if layers.iter().all(|l| l.hidden) {
        // a file of only spaces or openings: show them
        layers.iter_mut().for_each(|l| l.hidden = false);
    }
    for layer in &mut layers {
        // one opaque colour for the whole layer: no vertex colours
        let single = layer.colors.as_ref().and_then(|c| c.windows(2).all(|w| w[0] == w[1]).then(|| c.first().copied()).flatten());
        if let Some(c) = single.filter(|c| c[3] == 255) {
            layer.color = [c[0], c[1], c[2]].map(|v| v as f32 / 255.0);
            layer.colors = None;
        }
    }
    layers.extend(scene.clouds.into_values());
    let header = files.last().and_then(|r| r.file.header.as_ref());
    let mut info = vec![
        ("IFCX version".to_string(), header.and_then(|h| h.ifcx_version.clone()).unwrap_or_default()),
        ("IFCX files".to_string(), files.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(", ")),
        ("IFCX elements".to_string(), scene.elements.to_string()),
    ];
    if let Some(id) = files.first().and_then(|r| r.file.header.as_ref()?.id.clone()) {
        info.push(("IFCX id".into(), id));
    }
    if let Some(author) = header.and_then(|h| h.author.clone()) {
        info.push(("IFCX author".into(), author));
    }
    let remote: usize = files.iter().flat_map(|r| r.file.imports.iter().flatten()).filter(|i| i.uri.contains("://")).count();
    if remote > 0 {
        info.push(("IFCX imports not read (web)".into(), remote.to_string()));
    }
    if layers.is_empty() && definitions.is_empty() {
        return Err("IFCX: no meshes found".into());
    }
    let mut levels = scene.levels;
    levels.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    levels.dedup();
    // the drawings this viewer wrote (`o3v::` attributes), in file order, the later file winning
    let mut drawings: Vec<(String, Value)> = Vec::new();
    for node in files.iter().flat_map(|r| &r.file.data) {
        let own: serde_json::Map<String, Value> =
            node.attributes.iter().flatten().filter(|(k, _)| k.starts_with("o3v::")).map(|(k, v)| (k.clone(), v.clone())).collect();
        if own.is_empty() {
            continue;
        }
        match drawings.iter_mut().find(|(p, _)| *p == node.path) {
            Some((_, attributes)) => attributes.as_object_mut().into_iter().for_each(|a| a.extend(own.clone())),
            None => drawings.push((node.path.clone(), Value::Object(own))),
        }
    }
    let project = ProjectData { info, levels, drawings, elements: definitions, ..ProjectData::default() };
    Ok(Loaded { structural: Vec::new(), origin: None, layers, views: Vec::new(), project })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A building with a brick wall (a box 4 x 0.2 x 3 m moved 2 m along x, its material
    /// inherited, a hidden void child), a door that inherits its type's panel (a quad, moved 1 m up
    /// the wall), and a translucent room; the wall's node is given twice, the later one winning.
    /// Our own sample, written after the structure of the public IFC5 examples.
    const BASE: &str = r#"{
  "header": {"id": "test/base.ifcx", "ifcxVersion": "ifcx_alpha", "dataVersion": "1.0.0", "author": "test@example.org", "timestamp": "2026-11-05"},
  "imports": [{"uri": "https://ifcx.dev/@standards.buildingsmart.org/ifc/core/ifc@v5a.ifcx"}],
  "schemas": {},
  "data": [
    {"path": "p1", "children": {"Site_Building": "b1"}, "attributes": {"bsi::ifc::class": {"code": "IfcProject", "uri": "https://example.org/IfcProject"}}},
    {"path": "b1", "children": {"Wall": "w1", "Door": "d1", "Room": "s1"}, "attributes": {"bsi::ifc::class": {"code": "IfcBuilding"}}},
    {"path": "w1", "children": {"Body": "w1-body", "Void": "w1-void"}, "inherits": {"material": "brick"},
     "attributes": {"bsi::ifc::class": {"code": "IfcWall"}, "bsi::ifc::prop::IsExternal": true,
                    "usd::xformop": {"transform": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[2,0,0,1]]}}},
    {"path": "w1-body", "attributes": {"usd::usdgeom::mesh": {
      "points": [[0,0,0],[4,0,0],[4,0.2,0],[0,0.2,0],[0,0,3],[4,0,3],[4,0.2,3],[0,0.2,3]],
      "faceVertexIndices": [0,2,1, 0,3,2, 4,5,6, 4,6,7, 0,1,5, 0,5,4, 1,2,6, 1,6,5, 2,3,7, 2,7,6, 3,0,4, 3,4,7]}}},
    {"path": "w1-void", "attributes": {"usd::usdgeom::visibility": {"visibility": "invisible"},
      "usd::usdgeom::mesh": {"points": [[0,0,0],[1,0,0],[0,0,1]], "faceVertexIndices": [0,1,2]}}},
    {"path": "brick", "attributes": {"bsi::ifc::presentation::diffuseColor": [0.8, 0.4, 0.2], "bsi::ifc::material": {"code": "BRICK"}}},
    {"path": "door-type", "children": {"Panel": "panel"}, "attributes": {"bsi::ifc::class": {"code": "IfcDoor"}, "bsi::ifc::prop::TypeName": "D1"}},
    {"path": "panel", "attributes": {"bsi::ifc::presentation::diffuseColor": [0.4, 0.25, 0.1],
      "usd::usdgeom::mesh": {"points": [0,-0.01,0, 1,-0.01,0, 1,-0.01,2, 0,-0.01,2], "faceVertexCounts": [4], "faceVertexIndices": [0,1,2,3]}}},
    {"path": "d1", "inherits": {"type": "door-type"}, "attributes": {"bsi::ifc::prop::Name": "Front door",
      "usd::xformop": {"transform": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[3,0,0,1]]}}},
    {"path": "d1/Panel", "attributes": {"bsi::ifc::presentation::diffuseColor": [0, 0, 1]}},
    {"path": "s1", "children": {"Body": "s1-body"}, "attributes": {"bsi::ifc::class": {"code": "IfcSpace"},
      "bsi::ifc::presentation::diffuseColor": [0.6, 0.7, 0.8], "bsi::ifc::presentation::opacity": 0.3}},
    {"path": "s1-body", "attributes": {"usd::usdgeom::mesh": {"points": [[0,0,0],[4,0,0],[4,4,0]], "faceVertexIndices": [0,1,2]}}},
    {"path": "w1", "attributes": {"bsi::ifc::prop::IsExternal": false, "nlsfb::class": {"code": "21.21"}}}
  ]
}"#;

    /// An overlay: a fire rating on the wall, and the door taken out of the building.
    const OVERLAY: &str = r#"{
  "header": {"id": "test/overlay.ifcx", "ifcxVersion": "ifcx_alpha", "dataVersion": "1.0.0", "author": "test@example.org", "timestamp": "2026-11-05"},
  "imports": [], "schemas": {},
  "data": [
    {"path": "w1", "attributes": {"bsi::ifc::prop::FireRating": "R60"}},
    {"path": "b1", "children": {"Door": null}}
  ]
}"#;

    fn bytes(name: &str, text: &str) -> Source {
        Source::Bytes { name: name.into(), data: Arc::new(text.as_bytes().to_vec()) }
    }

    fn layer<'a>(model: &'a crate::model::Model, name: &str) -> &'a Layer {
        model.layers.iter().find(|l| l.name == name).unwrap_or_else(|| panic!("no layer {name}"))
    }

    #[test]
    fn composes_classes_types_materials_and_transforms() {
        let model = crate::loaders::load_with(&bytes("base.IFCX", BASE), &Progress::default()).expect("IFCX");
        assert_eq!(model.format, "IFCX");
        let names: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["IfcDoor", "IfcWall", "Helpers: IfcSpace"]);
        // the wall: 12 triangles, moved 2 m, brick red from its material; the invisible void left out
        let wall = layer(&model, "IfcWall");
        assert_eq!(wall.triangle_count(), 12);
        assert_eq!(wall.color, [0.8, 0.4, 0.2].map(|v: f32| (v * 255.0).round() / 255.0));
        let xs = wall.positions.chunks_exact(3).map(|p| p[0]);
        assert!(xs.clone().all(|x| (2.0..=6.0).contains(&x)));
        // closed and facing out
        assert!(wall.solid_indices().len() == 36);
        let props = &model.props[&wall.elements[0].0];
        assert!(props.contains("\"Class\":\"IfcWall\"") && props.contains("\"Material\":\"BRICK\""), "{props}");
        assert!(props.contains("\"IsExternal\":\"false\"") && props.contains("\"nlsfb::class::code\":\"21.21\""), "{props}");
        assert!(props.contains("\"Path\":\"p1/Site_Building/Wall\"") && props.contains("\"Name\":\"Wall\""), "{props}");
        // the door: its type's panel, a quad of two triangles, 3 m along x, blue through `d1/Panel`
        let door = layer(&model, "IfcDoor");
        assert_eq!(door.triangle_count(), 2);
        assert_eq!(door.color, [0.0, 0.0, 1.0]);
        assert!(door.positions.chunks_exact(3).all(|p| (3.0..=4.0).contains(&p[0])));
        assert!(door.normals.chunks_exact(3).all(|n| n[1] < -0.99));
        let props = &model.props[&door.elements[0].0];
        assert!(props.contains("\"Name\":\"Front door\"") && props.contains("\"TypeName\":\"D1\""), "{props}");
        // the room starts hidden and translucent
        let room = layer(&model, "Helpers: IfcSpace");
        assert!(room.hidden);
        assert_eq!(room.colors.as_ref().unwrap()[0][3], 77);
        assert!(model.project.info.iter().any(|(k, v)| k == "IFCX imports not read (web)" && v == "1"));
    }

    #[test]
    fn later_files_layer_over_earlier_ones() {
        let model = crate::loaders::load_stack(&[bytes("base.ifcx", BASE), bytes("overlay.ifcx", OVERLAY)], &Progress::default()).expect("IFCX stack");
        let names: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["IfcWall", "Helpers: IfcSpace"], "the overlay removes the door");
        let wall = layer(&model, "IfcWall");
        let props = &model.props[&wall.elements[0].0];
        assert!(props.contains("\"FireRating\":\"R60\""), "{props}");
        assert!(model.file_name().starts_with("base"));
        assert!(model.project.info.iter().any(|(k, v)| k == "IFCX files" && v == "base.ifcx, overlay.ifcx"));
        // the overlay alone has nothing to draw
        let error = crate::loaders::load_with(&bytes("overlay.ifcx", OVERLAY), &Progress::default()).unwrap_err();
        assert!(error.contains("no geometry") || error.contains("no meshes"), "{error}");
    }

    #[test]
    fn reads_local_imports_under_the_file() {
        let dir = std::env::temp_dir().join(format!("o3v-ifcx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("base.ifcx"), BASE).unwrap();
        let overlay = OVERLAY.replace("\"imports\": []", "\"imports\": [{\"uri\": \"./base.ifcx\"}]");
        std::fs::write(dir.join("overlay.ifcx"), overlay).unwrap();
        let model = crate::loaders::load(&dir.join("overlay.ifcx")).expect("IFCX with a local import");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(model.layers.iter().any(|l| l.name == "IfcWall"));
        assert!(!model.layers.iter().any(|l| l.name == "IfcDoor"));
        assert!(model.props.values().any(|p| p.contains("R60")));
    }

    #[test]
    fn reads_point_clouds_and_survives_cycles() {
        let raw: Vec<u8> = [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0].iter().flat_map(|f| f.to_le_bytes()).collect();
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut encoded = String::new();
        for chunk in raw.chunks(3) {
            let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));
            for k in 0..=chunk.len() {
                encoded.push(ALPHABET[(n >> (18 - 6 * k) & 63) as usize] as char);
            }
        }
        assert_eq!(floats(&base64(&encoded)), [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        let text = format!(r#"{{"header": {{"ifcxVersion": "ifcx_alpha"}}, "imports": [], "schemas": {{}}, "data": [
            {{"path": "scan", "children": {{"A": "a", "B": "b"}}, "attributes": {{"usd::xformop": {{"transform": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,10,1]]}}}}}},
            {{"path": "a", "attributes": {{"points::array": {{"positions": [[0,0,0],[1,0,0]], "colors": [[1,0,0],[0,1,0]]}}}}}},
            {{"path": "b", "attributes": {{"points::base64": {{"positions": "{encoded}"}}}}}},
            {{"path": "loop1", "children": {{"x": "loop2"}}}}, {{"path": "loop2", "children": {{"y": "loop1"}}}},
            {{"path": "self", "inherits": {{"me": "self"}}}}
        ]}}"#);
        let model = crate::loaders::load_with(&bytes("cloud.ifcx", &text), &Progress::default()).expect("IFCX points");
        assert_eq!(model.point_count(), 4);
        assert!((model.bounds.min[2] - 10.0).abs() < 1e-6 && (model.bounds.max[2] - 16.0).abs() < 1e-6, "{:?}", model.bounds);
        assert_eq!(model.layers[0].colors.as_ref().unwrap()[0], [255, 0, 0, 255]);
    }

    #[test]
    fn rejects_other_json() {
        let error = IfcxLoader.load(&bytes("x.ifcx", "{\"type\": \"FeatureCollection\"}")).unwrap_err();
        assert!(error.contains("not an IFCX file"), "{error}");
    }

    /// A building 1 m up with three storeys: one with an `Elevation` (which wins over its
    /// placement), one placed only (its composed Z counts, the building's metre included), and
    /// one named by its child name; a slab gives the file a mesh.
    const STOREYS: &str = r#"{
  "header": {"id": "test/storeys.ifcx", "ifcxVersion": "ifcx_alpha", "dataVersion": "1.0.0", "author": "test@example.org", "timestamp": "2026-11-07"},
  "imports": [], "schemas": {},
  "data": [
    {"path": "b1", "children": {"Ground": "g", "First": "f", "Roof": "r"}, "attributes": {"bsi::ifc::class": {"code": "IfcBuilding"},
      "usd::xformop": {"transform": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,1,1]]}}},
    {"path": "g", "children": {"Slab": "slab"}, "attributes": {"bsi::ifc::class": {"code": "IfcBuildingStorey"}, "bsi::ifc::prop::Name": "00 Ground floor",
      "bsi::ifc::prop::Elevation": 0.0, "usd::xformop": {"transform": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,5,1]]}}},
    {"path": "f", "attributes": {"bsi::ifc::class": {"code": "IfcBuildingStorey"}, "bsi::ifc::prop::Name": "01 First floor",
      "usd::xformop": {"transform": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,3,1]]}}},
    {"path": "r", "attributes": {"bsi::ifc::class": {"code": "IfcBuildingStorey"}, "bsi::ifc::prop::Elevation": "7.5"}},
    {"path": "slab", "attributes": {"bsi::ifc::class": {"code": "IfcSlab"},
      "usd::usdgeom::mesh": {"points": [[0,0,0],[4,0,0],[4,4,0]], "faceVertexIndices": [0,1,2]}}}
  ]
}"#;

    #[test]
    fn reads_building_storeys_as_levels() {
        let model = crate::loaders::load_with(&bytes("storeys.ifcx", STOREYS), &Progress::default()).expect("IFCX");
        let levels: Vec<(&str, f32)> = model.project.levels.iter().map(|(n, z)| (n.as_str(), *z)).collect();
        assert_eq!(levels, [("00 Ground floor", 0.0), ("01 First floor", 4.0), ("Roof", 7.5)]);
    }

    #[test]
    fn the_documents_ifcx_levels_read_back_as_the_same_levels() {
        let mut model = crate::model::Model::new("house.rvt".into(), "Revit", Vec::new());
        model.project.levels = vec![("00".into(), 0.0), ("01".into(), 3.0)];
        let mut document = crate::document::Document::from_model(&model);
        document.add_level("Roof", 6.25).unwrap();
        // the levels under a building, with a slab for a mesh
        let mut data = document.ifcx_nodes();
        let children: serde_json::Map<String, Value> =
            document.levels().iter().map(|l| (l.name.clone(), Value::from(l.path.clone()))).collect();
        data.push(serde_json::json!({"path": "b1", "children": children, "attributes": {"bsi::ifc::class": {"code": "IfcBuilding"}}}));
        data.push(serde_json::json!({"path": "slab", "attributes": {"bsi::ifc::class": {"code": "IfcSlab"},
            "usd::usdgeom::mesh": {"points": [[0,0,0],[4,0,0],[4,4,0]], "faceVertexIndices": [0,1,2]}}}));
        let file = serde_json::json!({
            "header": {"id": "test/document.ifcx", "ifcxVersion": "ifcx_alpha", "dataVersion": "1.0.0", "author": "test@example.org", "timestamp": "2026-11-07"},
            "imports": [], "schemas": {}, "data": data,
        });
        let loaded = crate::loaders::load_with(&bytes("document.ifcx", &file.to_string()), &Progress::default()).expect("IFCX");
        let again = crate::document::Document::from_model(&loaded);
        let read: Vec<(&str, f64)> = again.levels().iter().map(|l| (l.name.as_str(), l.elevation)).collect();
        assert_eq!(read, [("00", 0.0), ("01", 3.0), ("Roof", 6.25)]);
    }
}
