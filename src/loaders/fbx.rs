//! FBX (.fbx), as Revit, Navisworks, 3ds Max, Twinmotion and Blender export it. Binary files
//! (6.1 and 7.x, zlib-compressed arrays through flate2) and ASCII files are read by a small node
//! parser written from the public description of the format (Blender's developer notes). Each
//! model with a mesh is a pickable element with its name and user properties, placed with its
//! parents and its full transform (Lcl translation/rotation/scaling with the rotation order,
//! pre/post rotation, pivots and offsets, geometric transform); one layer per material, in its
//! diffuse colour. Polygons are fan triangulated; normals of the file are kept, else flat
//! shaded. Units and axes from `GlobalSettings`; converted to metres, Z up. Textures, NURBS,
//! lines, cameras, lights, animations, skinning and blend shapes are not read.

use super::{Loader, Source, DEFAULT_COLOR};
use crate::model::Layer;
use glam::{DMat3, DMat4, DVec3};
use std::collections::{BTreeMap, HashMap};
use std::io::Read;

pub struct FbxLoader;

impl Loader for FbxLoader {
    fn name(&self) -> &'static str {
        "FBX"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["fbx"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        let data = source.read()?;
        let nodes = if data.starts_with(b"Kaydara FBX Binary") { parse_binary(&data)? } else { parse_ascii(&data)? };
        let scene = Scene::new(&nodes)?;
        Ok(scene.layers())
    }
}

/// A property value of a node; numbers of either kind convert to f64 / i64.
#[derive(Debug, Clone, PartialEq)]
enum Prop {
    Int(i64),
    Float(f64),
    Str(String),
    Ints(Vec<i64>),
    Floats(Vec<f64>),
    /// Raw bytes (embedded textures): not kept.
    Other,
}

impl Prop {
    fn f64(&self) -> Option<f64> {
        match self {
            Prop::Int(i) => Some(*i as f64),
            Prop::Float(f) => Some(*f),
            _ => None,
        }
    }

    fn str(&self) -> Option<&str> {
        match self {
            Prop::Str(s) => Some(s),
            _ => None,
        }
    }

    /// An object id: a number in 7.x files, the object's full name in 6.x files.
    fn id(&self) -> Option<String> {
        match self {
            Prop::Int(i) => Some(i.to_string()),
            Prop::Str(s) => Some(s.clone()),
            _ => None,
        }
    }
}

/// A node of the FBX tree: `Name: props { children }`.
#[derive(Debug, Default)]
struct Node {
    name: String,
    props: Vec<Prop>,
    children: Vec<Node>,
}

impl Node {
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    /// All numbers of the node: one array property (binary) or a list of numbers (ASCII).
    fn floats(&self) -> Vec<f64> {
        let mut out = Vec::new();
        for p in &self.props {
            match p {
                Prop::Floats(v) => out.extend_from_slice(v),
                Prop::Ints(v) => out.extend(v.iter().map(|&i| i as f64)),
                p => out.extend(p.f64()),
            }
        }
        out
    }

    fn ints(&self) -> Vec<i64> {
        let mut out = Vec::new();
        for p in &self.props {
            match p {
                Prop::Ints(v) => out.extend_from_slice(v),
                Prop::Floats(v) => out.extend(v.iter().map(|&f| f as i64)),
                Prop::Int(i) => out.push(*i),
                Prop::Float(f) => out.push(*f as i64),
                _ => {}
            }
        }
        out
    }

    fn text(&self, name: &str) -> Option<&str> {
        self.child(name)?.props.first()?.str()
    }

    /// The values of a property in `Properties70` (`P: name, type, label, flags, values..`) or
    /// `Properties60` (`Property: name, type, flags, values..`).
    fn property(&self, name: &str) -> Option<&[Prop]> {
        for (block, entry, skip) in [("Properties70", "P", 4), ("Properties60", "Property", 3)] {
            let Some(block) = self.child(block) else { continue };
            if let Some(p) = block.children.iter().find(|p| p.name == entry && p.props.first().and_then(Prop::str) == Some(name)) {
                return Some(p.props.get(skip..).unwrap_or(&[]));
            }
        }
        None
    }

    fn property_f64(&self, name: &str) -> Option<f64> {
        self.property(name)?.first()?.f64()
    }

    fn property_vec3(&self, name: &str) -> Option<DVec3> {
        let v = self.property(name)?;
        Some(DVec3::new(v.first()?.f64()?, v.get(1)?.f64()?, v.get(2)?.f64()?))
    }
}

/// Which nodes are kept: of the top level only the settings, objects and connections, of the
/// objects only models, geometry and materials (no animation curves, poses or textures).
fn wanted(parent: &str, name: &str) -> bool {
    match parent {
        "" => matches!(name, "GlobalSettings" | "Objects" | "Connections"),
        "Objects" => matches!(name, "Model" | "Geometry" | "Material"),
        _ => true,
    }
}

// ---------------------------------------------------------------------------------------------
// Binary files: a 27-byte header (magic, version), then node records: end offset, property
// count, property list length (u32, or u64 from version 7500), name; properties; nested
// records closed by an empty record.

fn parse_binary(data: &[u8]) -> Result<Vec<Node>, String> {
    if data.len() < 27 {
        return Err("FBX: the file is too short".into());
    }
    let version = u32::from_le_bytes([data[23], data[24], data[25], data[26]]);
    let mut reader = Reader { data, pos: 27, wide: version >= 7500 };
    reader.list("", data.len(), 0)
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    wide: bool,
}

impl Reader<'_> {
    fn bytes(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.data.len()).ok_or("FBX: unexpected end of file")?;
        let out = &self.data[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.bytes(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    /// A record header field: u32, or u64 in 7.5 and later.
    fn offset(&mut self) -> Result<usize, String> {
        if self.wide {
            Ok(u64::from_le_bytes(self.bytes(8)?.try_into().unwrap()) as usize)
        } else {
            Ok(self.u32()? as usize)
        }
    }

    fn list(&mut self, parent: &str, end: usize, depth: usize) -> Result<Vec<Node>, String> {
        if depth > 64 {
            return Err("FBX: nodes nested too deep".into());
        }
        let mut out = Vec::new();
        let header = if self.wide { 25 } else { 13 };
        while self.pos + header <= end {
            let start = self.pos;
            let record_end = self.offset()?;
            let count = self.offset()?;
            let _length = self.offset()?;
            let name_length = self.u8()? as usize;
            // the empty record that closes a list
            if record_end == 0 {
                break;
            }
            if record_end <= start || record_end > self.data.len() {
                return Err(format!("FBX: broken node record at byte {start}"));
            }
            let name = String::from_utf8_lossy(self.bytes(name_length)?).into_owned();
            if !wanted(parent, &name) {
                self.pos = record_end;
                continue;
            }
            let mut props = Vec::with_capacity(count.min(64));
            for _ in 0..count {
                props.push(self.prop()?);
            }
            let children = if self.pos < record_end { self.list(&name, record_end, depth + 1)? } else { Vec::new() };
            self.pos = record_end;
            out.push(Node { name, props, children });
        }
        Ok(out)
    }

    fn prop(&mut self) -> Result<Prop, String> {
        let kind = self.u8()?;
        Ok(match kind {
            b'Y' => Prop::Int(i16::from_le_bytes(self.bytes(2)?.try_into().unwrap()) as i64),
            b'C' => Prop::Int(self.u8()? as i64),
            b'I' => Prop::Int(i32::from_le_bytes(self.bytes(4)?.try_into().unwrap()) as i64),
            b'L' => Prop::Int(i64::from_le_bytes(self.bytes(8)?.try_into().unwrap())),
            b'F' => Prop::Float(f32::from_le_bytes(self.bytes(4)?.try_into().unwrap()) as f64),
            b'D' => Prop::Float(f64::from_le_bytes(self.bytes(8)?.try_into().unwrap())),
            b'S' => {
                let n = self.u32()? as usize;
                Prop::Str(String::from_utf8_lossy(self.bytes(n)?).into_owned())
            }
            b'R' => {
                let n = self.u32()? as usize;
                self.bytes(n)?;
                Prop::Other
            }
            b'f' | b'd' | b'l' | b'i' | b'b' => {
                let count = self.u32()? as usize;
                let encoding = self.u32()?;
                let length = self.u32()? as usize;
                let packed = self.bytes(length)?;
                let size = match kind {
                    b'd' | b'l' => 8,
                    b'f' | b'i' => 4,
                    _ => 1,
                };
                let wanted = count.checked_mul(size).ok_or("FBX: array too large")?;
                let raw: std::borrow::Cow<[u8]> = if encoding == 1 {
                    let mut out = Vec::with_capacity(wanted.min(1 << 30));
                    flate2::read::ZlibDecoder::new(packed).take(wanted as u64).read_to_end(&mut out).map_err(|e| format!("FBX: compressed array: {e}"))?;
                    out.into()
                } else {
                    packed.into()
                };
                if raw.len() < wanted {
                    return Err("FBX: array shorter than its length".into());
                }
                let raw = &raw[..wanted];
                match kind {
                    b'd' => Prop::Floats(raw.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect()),
                    b'f' => Prop::Floats(raw.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap()) as f64).collect()),
                    b'l' => Prop::Ints(raw.chunks_exact(8).map(|c| i64::from_le_bytes(c.try_into().unwrap())).collect()),
                    b'i' => Prop::Ints(raw.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap()) as i64).collect()),
                    _ => Prop::Ints(raw.iter().map(|&b| b as i64).collect()),
                }
            }
            other => return Err(format!("FBX: unknown property type {:?} at byte {}", other as char, self.pos - 1)),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// ASCII files: `Name: value, value, .. { children }`, `;` comments, arrays as `*count { a: .. }`.

#[derive(Debug, PartialEq)]
enum Token<'a> {
    Key(&'a str),
    Value(Prop),
    Comma,
    Open,
    Close,
}

fn tokens(text: &str) -> Vec<Token<'_>> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b if b.is_ascii_whitespace() => i += 1,
            b';' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'{' | b'}' | b',' => {
                out.push(match c {
                    b'{' => Token::Open,
                    b'}' => Token::Close,
                    _ => Token::Comma,
                });
                i += 1;
            }
            b'"' => {
                let end = text[i + 1..].find('"').map_or(bytes.len(), |e| i + 1 + e);
                out.push(Token::Value(Prop::Str(text[i + 1..end].replace("&quot;", "\""))));
                i = end + 1;
            }
            // the element count of an array, `*24 { a: .. }`
            b'*' => {
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            _ => {
                let start = i;
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() && !b"{},\";:".contains(&bytes[i]) {
                    i += 1;
                }
                if i == start {
                    // a stray ':'
                    i += 1;
                    continue;
                }
                let word = &text[start..i];
                if bytes.get(i) == Some(&b':') {
                    out.push(Token::Key(word));
                    i += 1;
                } else if let Ok(v) = word.parse::<i64>() {
                    out.push(Token::Value(Prop::Int(v)));
                } else if let Ok(v) = word.parse::<f64>() {
                    out.push(Token::Value(Prop::Float(v)));
                } else {
                    out.push(Token::Value(Prop::Str(word.to_string())));
                }
            }
        }
    }
    out
}

fn parse_ascii(data: &[u8]) -> Result<Vec<Node>, String> {
    let text = String::from_utf8_lossy(data);
    if !text.contains("FBXHeaderExtension") && !text.trim_start().starts_with("; FBX") {
        return Err("FBX: neither a binary nor an ASCII FBX file".into());
    }
    let mut tokens = tokens(&text).into_iter().peekable();
    Ok(ascii_list(&mut tokens, "", 0))
}

fn ascii_list<'a>(tokens: &mut std::iter::Peekable<impl Iterator<Item = Token<'a>>>, parent: &str, depth: usize) -> Vec<Node> {
    let mut out = Vec::new();
    while let Some(token) = tokens.next() {
        let name = match token {
            Token::Close => break,
            Token::Key(name) => name.to_string(),
            _ => continue,
        };
        let mut props = Vec::new();
        loop {
            match tokens.peek() {
                Some(Token::Value(_)) => {
                    if let Some(Token::Value(v)) = tokens.next() {
                        props.push(v);
                    }
                    if tokens.peek() != Some(&Token::Comma) {
                        break;
                    }
                    tokens.next();
                }
                Some(Token::Comma) => {
                    tokens.next();
                }
                _ => break,
            }
        }
        let mut children = Vec::new();
        if tokens.peek() == Some(&Token::Open) && depth < 64 {
            tokens.next();
            children = ascii_list(tokens, &name, depth + 1);
        }
        // `Vertices: *24 { a: .. }`: the values of `a` are the node's own
        if props.is_empty() && children.len() == 1 && children[0].name == "a" {
            props = std::mem::take(&mut children[0].props);
            children.clear();
        }
        if wanted(parent, &name) {
            out.push(Node { name, props, children });
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// The scene: objects by id, the object-object connections, transforms and meshes.

struct Object<'a> {
    /// `Model`, `Geometry` or `Material`.
    kind: &'a str,
    name: String,
    /// `Mesh`, `Null`, `LimbNode`, ..
    class: &'a str,
    node: &'a Node,
}

/// The object's name: `name\0\x01Class` (binary) or `Class::name` (ASCII).
fn object_name(full: &str) -> String {
    if let Some((name, _)) = full.split_once("\u{0}\u{1}") {
        name.to_string()
    } else if let Some((_, name)) = full.split_once("::") {
        name.to_string()
    } else {
        full.to_string()
    }
}

struct Scene<'a> {
    objects: HashMap<String, Object<'a>>,
    /// Object ids in file order (so layers and elements follow the file).
    order: Vec<String>,
    /// Child id to parent ids, parent id to child ids, both in connection order.
    parents: HashMap<String, Vec<String>>,
    children: HashMap<String, Vec<String>>,
    /// File axes and units to metres, Z up.
    root: DMat4,
}

impl<'a> Scene<'a> {
    fn new(nodes: &'a [Node]) -> Result<Self, String> {
        let objects_node = nodes.iter().find(|n| n.name == "Objects").ok_or("FBX: no Objects section")?;
        let mut objects = HashMap::new();
        let mut order = Vec::new();
        for node in &objects_node.children {
            // 7.x: id, "name\0\x01Class", "Mesh"; 6.x: "Class::name", "Mesh"
            let (id, full, class) = match node.props.first() {
                Some(Prop::Int(_)) => (node.props[0].id(), node.props.get(1).and_then(Prop::str), node.props.get(2).and_then(Prop::str)),
                Some(Prop::Str(s)) => (Some(s.clone()), Some(s.as_str()), node.props.get(1).and_then(Prop::str)),
                _ => continue,
            };
            let Some(id) = id else { continue };
            order.push(id.clone());
            objects.insert(id, Object { kind: &node.name, name: object_name(full.unwrap_or("")), class: class.unwrap_or(""), node });
        }
        let mut parents: HashMap<String, Vec<String>> = HashMap::new();
        let mut children: HashMap<String, Vec<String>> = HashMap::new();
        for c in nodes.iter().filter(|n| n.name == "Connections").flat_map(|n| &n.children) {
            if !matches!(c.name.as_str(), "C" | "Connect") || c.props.first().and_then(Prop::str) != Some("OO") {
                continue;
            }
            let (Some(child), Some(parent)) = (c.props.get(1).and_then(Prop::id), c.props.get(2).and_then(Prop::id)) else { continue };
            parents.entry(child.clone()).or_default().push(parent.clone());
            children.entry(parent).or_default().push(child);
        }
        let settings = nodes.iter().find(|n| n.name == "GlobalSettings");
        Ok(Scene { objects, order, parents, children, root: settings.map(axes_and_units).unwrap_or_else(|| axes_and_units(&Node::default())) })
    }

    fn connected(&self, id: &str, kind: &str) -> impl Iterator<Item = (&String, &Object<'a>)> + '_ {
        let kind = kind.to_string();
        self.children.get(id).into_iter().flatten().filter_map(move |c| self.objects.get(c).filter(|o| o.kind == kind).map(|o| (c, o)))
    }

    /// The model's world transform: its parents' and its own.
    fn world(&self, id: &str, memo: &mut HashMap<String, DMat4>, depth: usize) -> DMat4 {
        if let Some(m) = memo.get(id) {
            return *m;
        }
        let Some(object) = self.objects.get(id) else { return self.root };
        let parent = self.parents.get(id).into_iter().flatten().find(|p| self.objects.get(*p).is_some_and(|o| o.kind == "Model"));
        let parent = match parent {
            Some(p) if depth < 256 => self.world(p, memo, depth + 1),
            _ => self.root,
        };
        let m = parent * local_transform(object.node);
        memo.insert(id.to_string(), m);
        m
    }

    fn layers(&self) -> Vec<Layer> {
        let mut out = Output::default();
        let mut memo = HashMap::new();
        let mut next_id = 1;
        for id in &self.order {
            let object = &self.objects[id];
            if object.kind != "Model" {
                continue;
            }
            // 7.x: Geometry objects connected to the model; 6.x: the mesh is in the model itself
            let mut meshes: Vec<&Node> = self.connected(id, "Geometry").filter(|(_, g)| g.class == "Mesh" || g.class.is_empty()).map(|(_, g)| g.node).collect();
            if object.node.child("Vertices").is_some() {
                meshes.push(object.node);
            }
            if meshes.is_empty() {
                continue;
            }
            let materials: Vec<(String, [f32; 3])> = self.connected(id, "Material").map(|(_, m)| (m.name.clone(), diffuse(m.node))).collect();
            let world = self.world(id, &mut memo, 0) * geometric_transform(object.node);
            let element = next_id;
            next_id += 1;
            let mut props = serde_json::Map::new();
            props.insert("Name".into(), object.name.clone().into());
            if !object.class.is_empty() {
                props.insert("Type".into(), object.class.into());
            }
            props.extend(user_properties(object.node));
            let props = serde_json::Value::Object(props).to_string();
            let mut written = false;
            for mesh in meshes {
                written |= out.mesh(mesh, world, &materials, element, (!written).then_some(&props));
            }
        }
        out.layers.into_values().collect()
    }
}

/// The matrix from file coordinates to metres, Z up: `UnitScaleFactor` is centimetres per unit;
/// the up, front and coordinate axes (with their signs) map to +Z, -Y and +X. Default: Y up, cm.
fn axes_and_units(settings: &Node) -> DMat4 {
    let int = |name: &str, default: i64| settings.property_f64(name).map_or(default, |v| v as i64);
    let axis = |name: &str, default: i64| {
        let mut v = DVec3::ZERO;
        v[int(name, default).clamp(0, 2) as usize] = if int(&format!("{name}Sign"), 1) < 0 { -1.0 } else { 1.0 };
        v
    };
    let (up, front, right) = (axis("UpAxis", 1), axis("FrontAxis", 2), axis("CoordAxis", 0));
    // rows: world x = right, world y = -front, world z = up
    let rotation = DMat3::from_cols(right, -front, up).transpose();
    let unit = settings.property_f64("UnitScaleFactor").filter(|v| v.is_finite() && *v > 0.0).unwrap_or(1.0) / 100.0;
    DMat4::from_mat3(rotation) * DMat4::from_scale(DVec3::splat(unit))
}

/// A rotation of Euler angles in degrees; the order names the axis applied first
/// (0 = XYZ, the default, .. 5 = ZYX).
fn euler(degrees: DVec3, order: i64) -> DMat4 {
    let r = degrees * (std::f64::consts::PI / 180.0);
    let (x, y, z) = (DMat4::from_rotation_x(r.x), DMat4::from_rotation_y(r.y), DMat4::from_rotation_z(r.z));
    match order {
        1 => y * z * x,
        2 => x * z * y,
        3 => z * x * y,
        4 => y * x * z,
        5 => x * y * z,
        _ => z * y * x,
    }
}

/// The node's transform relative to its parent:
/// `T * Roff * Rp * Rpre * R * Rpost⁻¹ * Rp⁻¹ * Soff * Sp * S * Sp⁻¹`.
fn local_transform(node: &Node) -> DMat4 {
    let v = |name: &str| node.property_vec3(name).unwrap_or(DVec3::ZERO);
    let t = DMat4::from_translation;
    let order = node.property_f64("RotationOrder").unwrap_or(0.0) as i64;
    let scaling = node.property_vec3("Lcl Scaling").unwrap_or(DVec3::ONE);
    let (rp, sp) = (v("RotationPivot"), v("ScalingPivot"));
    t(v("Lcl Translation"))
        * t(v("RotationOffset"))
        * t(rp)
        * euler(v("PreRotation"), 0)
        * euler(v("Lcl Rotation"), order)
        * euler(v("PostRotation"), 0).inverse()
        * t(-rp)
        * t(v("ScalingOffset"))
        * t(sp)
        * DMat4::from_scale(scaling)
        * t(-sp)
}

/// The transform of the mesh within its model, not passed on to children.
fn geometric_transform(node: &Node) -> DMat4 {
    let v = |name: &str| node.property_vec3(name);
    DMat4::from_translation(v("GeometricTranslation").unwrap_or(DVec3::ZERO))
        * euler(v("GeometricRotation").unwrap_or(DVec3::ZERO), 0)
        * DMat4::from_scale(v("GeometricScaling").unwrap_or(DVec3::ONE))
}

/// The diffuse colour of a material: `DiffuseColor` (`Diffuse` in older files), else the colour of
/// an Autodesk generic material (`generic_diffuse`, 3ds Max and Revit) or a Maya PBR material.
fn diffuse(material: &Node) -> [f32; 3] {
    ["DiffuseColor", "Diffuse", "generic_diffuse", "Maya|baseColor", "Maya|base_color"]
        .iter()
        .find_map(|name| material.property_vec3(name))
        .map(|c| c.to_array().map(|v| v.clamp(0.0, 1.0) as f32))
        .unwrap_or(DEFAULT_COLOR)
}

/// User-defined properties of a model (flag `U`), as text.
fn user_properties(node: &Node) -> Vec<(String, serde_json::Value)> {
    let Some(block) = node.child("Properties70") else { return Vec::new() };
    block
        .children
        .iter()
        .filter(|p| p.name == "P" && p.props.get(3).and_then(Prop::str).is_some_and(|f| f.contains('U')))
        .filter_map(|p| {
            let name = p.props.first()?.str()?.to_string();
            let value = match p.props.get(4..)? {
                [Prop::Str(s)] => s.clone().into(),
                [v] => v.f64()?.into(),
                values => values.iter().filter_map(Prop::f64).map(|v| v.to_string()).collect::<Vec<_>>().join(", ").into(),
            };
            Some((name, value))
        })
        .collect()
}

/// A per-polygon-vertex layer element (normals, materials): its mapping, reference and values.
struct LayerElement {
    mapping: String,
    indexed: bool,
    values: Vec<f64>,
    index: Vec<i64>,
}

impl LayerElement {
    fn read(mesh: &Node, element: &str, values: &str, index: &str) -> Option<Self> {
        let node = mesh.child(element)?;
        Some(LayerElement {
            mapping: node.text("MappingInformationType").unwrap_or("AllSame").to_string(),
            indexed: node.text("ReferenceInformationType").is_some_and(|r| r.starts_with("Index")),
            values: node.child(values)?.floats(),
            index: node.child(index).map(Node::ints).unwrap_or_default(),
        })
    }

    /// The value slot for polygon-vertex `corner` of `polygon`, which uses control point `vertex`.
    fn slot(&self, corner: usize, polygon: usize, vertex: usize) -> Option<usize> {
        let i = match self.mapping.as_str() {
            "ByPolygonVertex" => corner,
            "ByVertice" | "ByVertex" | "ByControlPoint" => vertex,
            "ByPolygon" => polygon,
            _ => 0,
        };
        if self.indexed {
            usize::try_from(*self.index.get(i)?).ok()
        } else {
            Some(i)
        }
    }
}

#[derive(Default)]
struct Output {
    /// Layers by material name.
    layers: BTreeMap<String, Layer>,
}

impl Output {
    /// Adds the polygons of a mesh, grouped by material; true when anything was drawn.
    fn mesh(&mut self, mesh: &Node, world: DMat4, materials: &[(String, [f32; 3])], id: u64, props: Option<&String>) -> bool {
        let vertices = mesh.child("Vertices").map(Node::floats).unwrap_or_default();
        let corners = mesh.child("PolygonVertexIndex").map(Node::ints).unwrap_or_default();
        let normals = LayerElement::read(mesh, "LayerElementNormal", "Normals", "NormalsIndex")
            .or_else(|| LayerElement::read(mesh, "LayerElementNormal", "Normals", "NormalIndex"));
        let material = mesh.child("LayerElementMaterial").map(|m| (m.text("MappingInformationType").unwrap_or("AllSame").to_string(), m.child("Materials").map(Node::ints).unwrap_or_default()));
        let normal_matrix = DMat3::from_mat4(world).inverse().transpose();
        let mirrored = world.determinant() < 0.0;
        // polygons (first corner, corner count) per material index
        let mut polygons: BTreeMap<usize, Vec<(usize, usize, usize)>> = BTreeMap::new();
        let mut start = 0;
        for (k, &c) in corners.iter().enumerate() {
            if c < 0 {
                let polygon = polygons.values().map(Vec::len).sum::<usize>();
                let m = match &material {
                    Some((mapping, values)) if mapping == "ByPolygon" => values.get(polygon).copied().unwrap_or(0),
                    Some((_, values)) => values.first().copied().unwrap_or(0),
                    None => 0,
                };
                polygons.entry(m.max(0) as usize).or_default().push((polygon, start, k + 1 - start));
                start = k + 1;
            }
        }
        let point = |c: i64| -> Option<DVec3> {
            let v = if c < 0 { !c } else { c } as usize;
            Some(DVec3::new(*vertices.get(3 * v)?, *vertices.get(3 * v + 1)?, *vertices.get(3 * v + 2)?))
        };
        let mut drawn = false;
        let mut props = props;
        for (m, list) in polygons {
            let (name, color) = materials.get(m).cloned().unwrap_or_else(|| ("Default".into(), DEFAULT_COLOR));
            let layer = self.layers.entry(name.clone()).or_insert_with(|| Layer { name, color, ..Layer::default() });
            let first = layer.indices.len() as u32;
            for (polygon, start, count) in list {
                let Some(points) = (start..start + count).map(|k| point(corners[k]).map(|p| world.transform_point3(p))).collect::<Option<Vec<_>>>() else { continue };
                if count < 3 {
                    continue;
                }
                // Newell normal for flat shading
                let mut flat = DVec3::ZERO;
                for i in 0..count {
                    let (a, b) = (points[i], points[(i + 1) % count]);
                    flat += DVec3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y));
                }
                let flat = flat.normalize_or_zero();
                let base = layer.vertex_count() as u32;
                for (i, p) in points.iter().enumerate() {
                    let k = start + i;
                    let vertex = if corners[k] < 0 { !corners[k] } else { corners[k] } as usize;
                    let n = normals
                        .as_ref()
                        .and_then(|e| e.slot(k, polygon, vertex))
                        .and_then(|s| Some(DVec3::new(*normals.as_ref()?.values.get(3 * s)?, *normals.as_ref()?.values.get(3 * s + 1)?, *normals.as_ref()?.values.get(3 * s + 2)?)))
                        .map(|n| (normal_matrix * n).normalize_or_zero())
                        .filter(|n| *n != DVec3::ZERO)
                        .unwrap_or(flat);
                    layer.positions.extend(p.to_array().map(|v| v as f32));
                    layer.normals.extend(n.to_array().map(|v| v as f32));
                }
                for i in 1..count as u32 - 1 {
                    let (b, c) = if mirrored { (i + 1, i) } else { (i, i + 1) };
                    layer.indices.extend([base, base + b, base + c]);
                }
            }
            let count = layer.indices.len() as u32 - first;
            if count > 0 {
                layer.elements.push((id, first, count));
                if let Some(p) = props.take() {
                    layer.props.push((id, p.clone()));
                }
                drawn = true;
            }
        }
        drawn
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Arc;

    /// A unit quad (cm, Y up) in a model moved 100 cm along x, under a parent rotated 90° about
    /// y; one red material; a second model instances the same geometry, without material.
    const ASCII: &str = r#"; FBX 7.4.0 project file
FBXHeaderExtension:  {
	FBXHeaderVersion: 1003
}
GlobalSettings:  {
	Version: 1000
	Properties70:  {
		P: "UpAxis", "int", "Integer", "",1
		P: "UpAxisSign", "int", "Integer", "",1
		P: "FrontAxis", "int", "Integer", "",2
		P: "FrontAxisSign", "int", "Integer", "",1
		P: "CoordAxis", "int", "Integer", "",0
		P: "CoordAxisSign", "int", "Integer", "",1
		P: "UnitScaleFactor", "double", "Number", "",1
	}
}
Objects:  {
	Geometry: 10, "Geometry::Quad", "Mesh" {
		Vertices: *12 {
			a: 0,0,0,100,0,0,100,0,-100,0,0,-100
		}
		PolygonVertexIndex: *4 {
			a: 0,1,2,-4
		}
		LayerElementMaterial: 0 {
			MappingInformationType: "AllSame"
			ReferenceInformationType: "IndexToDirect"
			Materials: *1 {
				a: 0
			}
		}
	}
	Model: 20, "Model::Parent", "Null" {
		Properties70:  {
			P: "Lcl Rotation", "Lcl Rotation", "", "A",0,90,0
		}
	}
	Model: 21, "Model::Slab", "Mesh" {
		Properties70:  {
			P: "Lcl Translation", "Lcl Translation", "", "A",100,0,0
			P: "Fire rating", "KString", "", "U", "60 min"
		}
	}
	Model: 22, "Model::Copy", "Mesh" {
		Properties70:  {
			P: "Lcl Translation", "Lcl Translation", "", "A",0,50,0
		}
	}
	Material: 30, "Material::Red", "" {
		Properties70:  {
			P: "DiffuseColor", "Color", "", "A",1,0,0
		}
	}
}
Connections:  {
	C: "OO",20,0
	C: "OO",21,20
	C: "OO",10,21
	C: "OO",30,21
	C: "OO",22,0
	C: "OO",10,22
}
"#;

    fn load(name: &str, data: Vec<u8>) -> crate::model::Model {
        let source = Source::Bytes { name: name.into(), data: Arc::new(data) };
        crate::loaders::load_with(&source, &crate::loaders::Progress::default()).expect("FBX")
    }

    fn close(a: &[f32], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5)
    }

    #[test]
    fn ascii_hierarchy_units_axes_and_materials() {
        let model = load("quad.FBX", ASCII.as_bytes().to_vec());
        assert_eq!(model.format, "FBX");
        let names: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Default", "Red"]);
        let red = &model.layers[1];
        assert_eq!(red.color, [1.0, 0.0, 0.0]);
        assert_eq!(red.triangle_count(), 2);
        // the quad lies flat (file y = 0 becomes z = 0); the parent's 90° about the file's y
        // (now z) turns the 1 m offset along x into one along -file z, which is +y
        assert!(red.positions.chunks_exact(3).all(|p| p[2].abs() < 1e-5));
        let ys: Vec<f32> = red.positions.chunks_exact(3).map(|p| p[1]).collect();
        assert!(ys.iter().all(|&y| (0.0 - 1e-5..=2.0 + 1e-5).contains(&y)), "{:?}", red.positions);
        assert!(close(&red.positions[0..3], [0.0, 1.0, 0.0]), "{:?}", &red.positions[0..3]);
        assert!(red.normals.chunks_exact(3).all(|n| n[2] > 0.99), "{:?}", red.normals);
        // the copy, 50 cm up, unrotated
        let copy = &model.layers[0];
        assert!(copy.positions.chunks_exact(3).all(|p| (p[2] - 0.5).abs() < 1e-5));
        assert!(model.props.values().any(|p| p.contains("\"Name\":\"Slab\"") && p.contains("60 min")));
        assert_eq!(model.props.len(), 2);
    }

    /// Writes nodes as a binary FBX 7.5 file (u64 record headers, zlib-compressed float arrays).
    fn binary(nodes: &[Node]) -> Vec<u8> {
        fn record(out: &mut Vec<u8>, node: &Node) {
            let start = out.len();
            out.extend([0u8; 24]);
            out.push(node.name.len() as u8);
            out.extend(node.name.as_bytes());
            let props_start = out.len();
            for p in &node.props {
                match p {
                    Prop::Int(i) => {
                        out.push(b'L');
                        out.extend(i.to_le_bytes());
                    }
                    Prop::Float(f) => {
                        out.push(b'D');
                        out.extend(f.to_le_bytes());
                    }
                    Prop::Str(s) => {
                        out.push(b'S');
                        out.extend((s.len() as u32).to_le_bytes());
                        out.extend(s.as_bytes());
                    }
                    Prop::Floats(v) => {
                        let raw: Vec<u8> = v.iter().flat_map(|f| f.to_le_bytes()).collect();
                        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                        z.write_all(&raw).unwrap();
                        let z = z.finish().unwrap();
                        out.push(b'd');
                        out.extend((v.len() as u32).to_le_bytes());
                        out.extend(1u32.to_le_bytes());
                        out.extend((z.len() as u32).to_le_bytes());
                        out.extend(z);
                    }
                    Prop::Ints(v) => {
                        out.push(b'i');
                        out.extend((v.len() as u32).to_le_bytes());
                        out.extend(0u32.to_le_bytes());
                        out.extend((v.len() as u32 * 4).to_le_bytes());
                        out.extend(v.iter().flat_map(|&i| (i as i32).to_le_bytes()));
                    }
                    Prop::Other => {}
                }
            }
            let props_len = (out.len() - props_start) as u64;
            for child in &node.children {
                record(out, child);
            }
            if !node.children.is_empty() {
                out.extend([0u8; 25]);
            }
            let end = out.len() as u64;
            out[start..start + 8].copy_from_slice(&end.to_le_bytes());
            out[start + 8..start + 16].copy_from_slice(&(node.props.len() as u64).to_le_bytes());
            out[start + 16..start + 24].copy_from_slice(&props_len.to_le_bytes());
        }
        let mut out = b"Kaydara FBX Binary  \0\x1a\0".to_vec();
        out.extend(7500u32.to_le_bytes());
        for node in nodes {
            record(&mut out, node);
        }
        out.extend([0u8; 25]);
        out
    }

    fn node(name: &str, props: Vec<Prop>, children: Vec<Node>) -> Node {
        Node { name: name.into(), props, children }
    }

    #[test]
    fn binary_triangle_in_metres_z_up() {
        let s = |s: &str| Prop::Str(s.into());
        let p = |name: &str, value: Prop| node("P", vec![s(name), s(""), s(""), s(""), value], vec![]);
        let properties = |children| vec![node("Properties70", vec![], children)];
        // Z up (front -y), metres (100 cm per unit)
        let data = binary(&[
            node("FBXHeaderExtension", vec![], vec![node("FBXHeaderVersion", vec![Prop::Int(1003)], vec![])]),
            node("GlobalSettings", vec![], properties(vec![
                p("UpAxis", Prop::Int(2)), p("FrontAxis", Prop::Int(1)), p("FrontAxisSign", Prop::Int(-1)), p("UnitScaleFactor", Prop::Float(100.0)),
            ])),
            node("Objects", vec![], vec![
                node("Geometry", vec![Prop::Int(1), s("Tri\0\x01Geometry"), s("Mesh")], vec![
                    node("Vertices", vec![Prop::Floats(vec![0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 3.0, 0.0])], vec![]),
                    node("PolygonVertexIndex", vec![Prop::Ints(vec![0, 1, -3])], vec![]),
                ]),
                node("Model", vec![Prop::Int(2), s("Roof\0\x01Model"), s("Mesh")], properties(vec![
                    node("P", vec![s("Lcl Translation"), s(""), s(""), s("A"), Prop::Float(0.0), Prop::Float(0.0), Prop::Float(5.0)], vec![]),
                ])),
            ]),
            node("Connections", vec![], vec![node("C", vec![s("OO"), Prop::Int(1), Prop::Int(2)], vec![]), node("C", vec![s("OO"), Prop::Int(2), Prop::Int(0)], vec![])]),
        ]);
        let model = load("tri.fbx", data);
        assert_eq!(model.triangle_count(), 1);
        assert_eq!(model.layers[0].name, "Default");
        // metres, Z up: the file's axes stay
        assert_eq!(model.bounds.min, [0.0, 0.0, 5.0]);
        assert_eq!(model.bounds.max, [2.0, 3.0, 5.0]);
        assert!(model.props.values().any(|p| p.contains("Roof")));
        assert!(model.layers[0].normals.chunks_exact(3).all(|n| n[2] > 0.99));
    }

    #[test]
    fn rejects_other_files() {
        let source = Source::Bytes { name: "x.fbx".into(), data: Arc::new(b"hello".to_vec()) };
        assert!(FbxLoader.load(&source).unwrap_err().contains("FBX"));
    }
}

