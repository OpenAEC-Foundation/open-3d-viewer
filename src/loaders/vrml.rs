//! VRML 2.0 / VRML97 (`.wrl`, also gzipped `.wrz`) and X3D in its XML (`.x3d`) and classic
//! (`.x3dv`) encodings, gzipped too (`.x3dz`, `.x3dvz`). The scene graph is flattened to world
//! space: `Transform` (with centre and scale orientation), `Group`, `Switch` (the chosen child),
//! `LOD` (the most detailed level), `Anchor`, `Billboard`, `Collision` and the CAD nodes; `DEF`
//! and `USE` instances. Geometry: `IndexedFaceSet` (polygons fan triangulated, `ccw`, colours per
//! vertex or face), the triangle, fan, strip and quad sets, `ElevationGrid`, and the primitives
//! `Box`, `Sphere`, `Cylinder` and `Cone` tessellated; `PointSet` as dots. One layer per material
//! (its `DEF` name, else its colour), in the `Material` diffuse colour; each `Shape` a pickable
//! element named after its own or nearest `DEF`. Y up and metres, as the standards say, turned
//! to Z up; flat shaded. Textures, `Extrusion`, lines, text, `Inline` files, `PROTO` instances,
//! scripts and animation are not read. The parsers are written for this viewer from the public
//! ISO/IEC 14772 and 19775/19776 specifications.

use super::{Loader, Source, DEFAULT_COLOR};
use crate::model::Layer;
use glam::{Mat4, Quat, Vec3};
use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::rc::Rc;

pub struct VrmlLoader;

impl Loader for VrmlLoader {
    fn name(&self) -> &'static str {
        "VRML / X3D"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["wrl", "wrz", "x3d", "x3dv", "x3dz", "x3dvz"]
    }

    fn source_y_up(&self) -> bool {
        true
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        let data = source.read()?;
        let data = gunzip(&data)?;
        let text = String::from_utf8_lossy(&data);
        let text = text.trim_start_matches('\u{feff}');
        let (roots, units) = if text.trim_start().starts_with('<') {
            xml(text)?
        } else {
            classic(text)?
        };
        let mut scene = Scene::default();
        // Y up to Z up: (x, y, z) becomes (x, -z, y)
        let world = Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2) * Mat4::from_scale(Vec3::splat(units));
        for node in &roots {
            scene.node(node, world, None, 0);
        }
        let layers: Vec<Layer> = scene.layers.into_values().filter(|l| l.vertex_count() > 0).collect();
        if layers.is_empty() {
            return Err(format!("{}: no geometry found (lines, text, Inline files and PROTO instances are not read)", source.name()));
        }
        Ok(layers)
    }
}

/// The file, unpacked when it is gzipped.
fn gunzip(data: &[u8]) -> Result<std::borrow::Cow<'_, [u8]>, String> {
    if !data.starts_with(&[0x1f, 0x8b]) {
        return Ok(std::borrow::Cow::Borrowed(data));
    }
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(data).read_to_end(&mut out).map_err(|e| format!("VRML: gzip: {e}"))?;
    Ok(std::borrow::Cow::Owned(out))
}

/// A field value: numbers (also booleans as 0 and 1), strings or nodes.
#[derive(Debug, Clone, Default)]
enum Value {
    #[default]
    None,
    Numbers(Vec<f64>),
    Strings(#[allow(dead_code)] Vec<String>),
    Nodes(Vec<Rc<Node>>),
}

#[derive(Debug, Default)]
struct Node {
    kind: String,
    def: Option<String>,
    fields: Vec<(String, Value)>,
}

impl Node {
    fn get(&self, name: &str) -> Option<&Value> {
        self.fields.iter().rev().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    fn numbers(&self, name: &str) -> &[f64] {
        match self.get(name) {
            Some(Value::Numbers(v)) => v,
            _ => &[],
        }
    }

    fn number(&self, name: &str, default: f64) -> f64 {
        self.numbers(name).first().copied().unwrap_or(default)
    }

    fn flag(&self, name: &str, default: bool) -> bool {
        self.numbers(name).first().map_or(default, |v| *v != 0.0)
    }

    fn vec3(&self, name: &str, default: [f32; 3]) -> Vec3 {
        match self.numbers(name) {
            [x, y, z, ..] => Vec3::new(*x as f32, *y as f32, *z as f32),
            _ => Vec3::from(default),
        }
    }

    /// An axis-angle rotation (`0 0 1 0` when absent).
    fn rotation(&self, name: &str) -> Quat {
        match self.numbers(name) {
            [x, y, z, a, ..] => {
                let axis = Vec3::new(*x as f32, *y as f32, *z as f32);
                if axis.length_squared() < 1e-12 {
                    Quat::IDENTITY
                } else {
                    Quat::from_axis_angle(axis.normalize(), *a as f32)
                }
            }
            _ => Quat::IDENTITY,
        }
    }

    fn nodes(&self, name: &str) -> &[Rc<Node>] {
        match self.get(name) {
            Some(Value::Nodes(n)) => n,
            _ => &[],
        }
    }

    fn node(&self, name: &str) -> Option<&Rc<Node>> {
        self.nodes(name).first()
    }
}

// ---- classic encoding (VRML97, X3D classic) ----

#[derive(Debug, Clone, PartialEq)]
enum Token<'a> {
    Word(&'a str),
    Str(String),
    Open,
    Close,
    OpenList,
    CloseList,
}

fn tokens(text: &str) -> Vec<Token<'_>> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b' ' | b'\t' | b'\r' | b'\n' | b',' => i += 1,
            b'#' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'{' => (out.push(Token::Open), i += 1).1,
            b'}' => (out.push(Token::Close), i += 1).1,
            b'[' => (out.push(Token::OpenList), i += 1).1,
            b']' => (out.push(Token::CloseList), i += 1).1,
            b'"' => {
                let mut s = String::new();
                i += 1;
                let start = i;
                let mut last = start;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        s.push_str(&text[last..i]);
                        last = i + 1;
                        i += 2;
                        continue;
                    }
                    i += 1;
                }
                s.push_str(&text[last..i.min(bytes.len())]);
                out.push(Token::Str(s));
                i += 1;
            }
            _ => {
                let start = i;
                while i < bytes.len() && !matches!(bytes[i], b' ' | b'\t' | b'\r' | b'\n' | b',' | b'#' | b'{' | b'}' | b'[' | b']' | b'"') {
                    i += 1;
                }
                out.push(Token::Word(&text[start..i]));
            }
        }
    }
    out
}

struct Parser<'a> {
    tokens: Vec<Token<'a>>,
    at: usize,
    defs: HashMap<String, Rc<Node>>,
    depth: usize,
}

fn number(word: &str) -> Option<f64> {
    if let Some(hex) = word.strip_prefix("0x").or_else(|| word.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).ok().map(|v| v as f64);
    }
    word.parse().ok()
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Token<'a>> {
        self.tokens.get(self.at)
    }

    fn next(&mut self) -> Option<Token<'a>> {
        let t = self.tokens.get(self.at).cloned();
        self.at += 1;
        t
    }

    fn word(&mut self) -> Option<&'a str> {
        match self.next() {
            Some(Token::Word(w)) => Some(w),
            _ => None,
        }
    }

    /// Skips a `{ .. }` or `[ .. ]` block whose opening token was just read.
    fn skip_block(&mut self) {
        let mut depth = 1;
        while depth > 0 {
            match self.next() {
                Some(Token::Open | Token::OpenList) => depth += 1,
                Some(Token::Close | Token::CloseList) => depth -= 1,
                None => return,
                _ => {}
            }
        }
    }

    /// Statements up to the closing token (or the end): nodes, with prototypes, routes and the
    /// X3D header statements skipped.
    fn statements(&mut self) -> Vec<Rc<Node>> {
        let mut nodes = Vec::new();
        while let Some(token) = self.peek().cloned() {
            match token {
                Token::Close | Token::CloseList => break,
                Token::Word(w) => {
                    if !self.statement(w) {
                        if let Some(node) = self.node() {
                            nodes.push(node);
                        }
                    }
                }
                _ => self.at += 1,
            }
        }
        nodes
    }

    /// Reads a statement that is not a node; false when the next token starts a node.
    fn statement(&mut self, word: &str) -> bool {
        match word {
            "PROTO" => {
                self.at += 2;
                // interface [ .. ] and body { .. }
                for _ in 0..2 {
                    if matches!(self.next(), Some(Token::Open | Token::OpenList)) {
                        self.skip_block();
                    }
                }
            }
            "EXTERNPROTO" => {
                self.at += 2;
                if matches!(self.next(), Some(Token::OpenList)) {
                    self.skip_block();
                }
                match self.next() {
                    Some(Token::OpenList) => self.skip_block(),
                    _ => {}
                }
            }
            // ROUTE a.b TO c.d; IMPORT a.b AS c; EXPORT a AS b
            "ROUTE" | "IMPORT" => self.at += 4,
            "EXPORT" => self.at += 4,
            "PROFILE" | "COMPONENT" => self.at += 2,
            "META" => self.at += 3,
            "UNIT" => self.at += 4,
            _ => return false,
        }
        true
    }

    /// A node: `DEF name Type { .. }`, `USE name`, `NULL` or `Type { .. }`.
    fn node(&mut self) -> Option<Rc<Node>> {
        let word = self.word()?;
        match word {
            "USE" => {
                let name = self.word()?;
                self.defs.get(name).cloned()
            }
            "NULL" => None,
            "DEF" => {
                let name = self.word()?.to_string();
                let kind = self.word()?;
                let node = self.body(kind, Some(name.clone()))?;
                self.defs.insert(name, node.clone());
                Some(node)
            }
            kind => self.body(kind, None),
        }
    }

    fn body(&mut self, kind: &str, def: Option<String>) -> Option<Rc<Node>> {
        if self.next() != Some(Token::Open) {
            return None;
        }
        self.depth += 1;
        let mut node = Node { kind: kind.to_string(), def, fields: Vec::new() };
        if self.depth > 200 {
            self.skip_block();
            self.depth -= 1;
            return Some(Rc::new(node));
        }
        loop {
            match self.next() {
                None | Some(Token::Close) => break,
                Some(Token::Word(w)) => {
                    if matches!(w, "ROUTE" | "PROTO" | "EXTERNPROTO") {
                        self.at -= 1;
                        self.statement(w);
                        continue;
                    }
                    // interface declarations of scripts: `field SFFloat name value`
                    if matches!(w, "field" | "exposedField" | "initializeOnly" | "inputOutput") {
                        self.at += 1;
                        let name = self.word().unwrap_or("").to_string();
                        let value = self.value();
                        node.fields.push((name, value));
                        continue;
                    }
                    if matches!(w, "eventIn" | "eventOut" | "inputOnly" | "outputOnly") {
                        self.at += 2;
                        continue;
                    }
                    if self.peek() == Some(&Token::Word("IS")) {
                        self.at += 2;
                        continue;
                    }
                    let value = self.value();
                    node.fields.push((w.to_string(), value));
                }
                Some(Token::Open | Token::OpenList) => self.skip_block(),
                _ => {}
            }
        }
        self.depth -= 1;
        Some(Rc::new(node))
    }

    /// Whether the word at the cursor starts a node.
    fn at_node(&self) -> bool {
        match self.tokens.get(self.at) {
            Some(Token::Word("DEF" | "USE" | "NULL")) => true,
            Some(Token::Word(_)) => self.tokens.get(self.at + 1) == Some(&Token::Open),
            _ => false,
        }
    }

    fn value(&mut self) -> Value {
        match self.peek().cloned() {
            Some(Token::OpenList) => {
                self.at += 1;
                if self.at_node() {
                    let nodes = self.statements();
                    self.at += 1;
                    return Value::Nodes(nodes);
                }
                let (mut numbers, mut strings) = (Vec::new(), Vec::new());
                loop {
                    match self.next() {
                        None | Some(Token::CloseList) => break,
                        Some(Token::Str(s)) => strings.push(s),
                        Some(Token::Word(w)) => match w {
                            "TRUE" | "true" => numbers.push(1.0),
                            "FALSE" | "false" => numbers.push(0.0),
                            _ => numbers.extend(number(w)),
                        },
                        Some(Token::Open) => self.skip_block(),
                        _ => {}
                    }
                }
                if strings.is_empty() {
                    Value::Numbers(numbers)
                } else {
                    Value::Strings(strings)
                }
            }
            Some(Token::Str(_)) => match self.next() {
                Some(Token::Str(s)) => Value::Strings(vec![s]),
                _ => Value::None,
            },
            Some(Token::Word(w)) => {
                if self.at_node() {
                    return self.node().map_or(Value::None, |n| Value::Nodes(vec![n]));
                }
                match w {
                    "TRUE" => (self.at += 1, Value::Numbers(vec![1.0])).1,
                    "FALSE" => (self.at += 1, Value::Numbers(vec![0.0])).1,
                    _ => {
                        let mut numbers = Vec::new();
                        while let Some(Token::Word(w)) = self.peek() {
                            let Some(v) = number(w) else { break };
                            numbers.push(v);
                            self.at += 1;
                        }
                        if numbers.is_empty() {
                            // an enumeration value
                            self.at += 1;
                            return Value::Strings(vec![w.to_string()]);
                        }
                        Value::Numbers(numbers)
                    }
                }
            }
            _ => {
                self.at += 1;
                Value::None
            }
        }
    }
}

/// Parses the classic encoding: the root nodes and the length unit in metres.
fn classic(text: &str) -> Result<(Vec<Rc<Node>>, f32), String> {
    let first = text.lines().next().unwrap_or("");
    if first.starts_with("#VRML V1.0") {
        return Err("VRML 1.0 files are not supported (only VRML 2.0 / VRML97 and X3D)".into());
    }
    if !first.starts_with("#VRML") && !first.starts_with("#X3D") {
        return Err("VRML: no #VRML V2.0 or #X3D header".into());
    }
    let mut parser = Parser { tokens: tokens(text), at: 0, defs: HashMap::new(), depth: 0 };
    let mut units = 1.0;
    // X3D 3.3: UNIT length metres 0.001
    for k in 0..parser.tokens.len().saturating_sub(3) {
        if parser.tokens[k] == Token::Word("UNIT") && parser.tokens[k + 1] == Token::Word("length") {
            if let Token::Word(f) = &parser.tokens[k + 3] {
                units = f.parse().unwrap_or(1.0);
            }
        }
    }
    let mut roots = Vec::new();
    while parser.at < parser.tokens.len() {
        roots.extend(parser.statements());
        // a stray closing token
        parser.at += 1;
    }
    Ok((roots, units))
}

// ---- XML encoding (X3D) ----

/// The field an XML child goes into, unless it says so with `containerField`.
fn container(kind: &str) -> &'static str {
    match kind {
        "Appearance" => "appearance",
        "Material" | "PhysicalMaterial" | "TwoSidedMaterial" | "UnlitMaterial" => "material",
        "Coordinate" | "CoordinateDouble" | "GeoCoordinate" => "coord",
        "Color" | "ColorRGBA" => "color",
        "Normal" => "normal",
        "TextureCoordinate" | "MultiTextureCoordinate" => "texCoord",
        "ImageTexture" | "PixelTexture" | "MovieTexture" | "MultiTexture" => "texture",
        "TextureTransform" => "textureTransform",
        "Box" | "Sphere" | "Cylinder" | "Cone" | "IndexedFaceSet" | "IndexedTriangleSet" | "TriangleSet" | "IndexedTriangleFanSet"
        | "IndexedTriangleStripSet" | "TriangleFanSet" | "TriangleStripSet" | "IndexedQuadSet" | "QuadSet" | "ElevationGrid"
        | "Extrusion" | "PointSet" | "IndexedLineSet" | "LineSet" | "Text" | "Rectangle2D" | "Disk2D" => "geometry",
        "FontStyle" => "fontStyle",
        _ => "children",
    }
}

/// An attribute value: numbers (`true` and `false` as 1 and 0) or strings (`"a" "b"`).
fn attribute(text: &str) -> Value {
    let words: Vec<&str> = text.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()).collect();
    let numbers: Option<Vec<f64>> = words
        .iter()
        .map(|w| match *w {
            "true" | "TRUE" => Some(1.0),
            "false" | "FALSE" => Some(0.0),
            w => number(w),
        })
        .collect();
    match numbers {
        Some(n) => Value::Numbers(n),
        None if text.trim_start().starts_with('"') => {
            Value::Strings(tokens(text).into_iter().filter_map(|t| if let Token::Str(s) = t { Some(s) } else { None }).collect())
        }
        None => Value::Strings(vec![text.trim().to_string()]),
    }
}

fn xml_node(e: roxmltree::Node, defs: &mut HashMap<String, Rc<Node>>, depth: usize) -> Option<Rc<Node>> {
    if let Some(name) = e.attribute("USE") {
        return defs.get(name).cloned();
    }
    if depth > 200 {
        return None;
    }
    let kind = e.tag_name().name();
    if matches!(kind, "ProtoDeclare" | "ExternProtoDeclare" | "ProtoInstance" | "ROUTE" | "Script" | "IS") {
        return None;
    }
    let mut node = Node { kind: kind.to_string(), def: e.attribute("DEF").map(str::to_string), fields: Vec::new() };
    for a in e.attributes() {
        if !matches!(a.name(), "DEF" | "USE" | "containerField" | "class" | "id" | "style") {
            node.fields.push((a.name().to_string(), attribute(a.value())));
        }
    }
    let mut children: BTreeMap<String, Vec<Rc<Node>>> = BTreeMap::new();
    for c in e.children().filter(roxmltree::Node::is_element) {
        let field = c.attribute("containerField").unwrap_or_else(|| container(c.tag_name().name())).to_string();
        if let Some(child) = xml_node(c, defs, depth + 1) {
            children.entry(field).or_default().push(child);
        }
    }
    node.fields.extend(children.into_iter().map(|(k, v)| (k, Value::Nodes(v))));
    let node = Rc::new(node);
    if let Some(def) = &node.def {
        defs.insert(def.clone(), node.clone());
    }
    Some(node)
}

/// Parses the XML encoding: the nodes of `<Scene>` and the length unit in metres.
fn xml(text: &str) -> Result<(Vec<Rc<Node>>, f32), String> {
    let doc = roxmltree::Document::parse_with_options(text, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() })
        .map_err(|e| format!("X3D: {e}"))?;
    let root = doc.root_element();
    if !root.has_tag_name("X3D") {
        return Err("X3D: no <X3D> root element".into());
    }
    let units = root
        .descendants()
        .find(|n| n.has_tag_name("unit") && n.attribute("category") == Some("length"))
        .and_then(|n| n.attribute("conversionFactor"))
        .and_then(|f| f.trim().parse().ok())
        .unwrap_or(1.0);
    let scene = root.children().find(|n| n.has_tag_name("Scene")).ok_or("X3D: no <Scene>")?;
    let mut defs = HashMap::new();
    let roots = scene.children().filter(roxmltree::Node::is_element).filter_map(|c| xml_node(c, &mut defs, 0)).collect();
    Ok((roots, units))
}

// ---- the scene ----

#[derive(Default)]
struct Scene {
    /// Layers by material name.
    layers: BTreeMap<String, Layer>,
    next_id: u64,
}

/// The material of a shape: layer name and RGBA8 colour.
fn material(shape: &Node) -> (String, [u8; 4]) {
    let appearance = shape.node("appearance");
    let material = appearance.and_then(|a| a.node("material"));
    let to8 = |c: Vec3| c.to_array().map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    let Some(m) = material else {
        // no material: unlit white in the standards; the default grey here
        let d = DEFAULT_COLOR.map(|v| (v * 255.0).round() as u8);
        return ("Default".into(), [d[0], d[1], d[2], 255]);
    };
    let mut color = match m.kind.as_str() {
        "PhysicalMaterial" => m.vec3("baseColor", [1.0; 3]),
        "UnlitMaterial" => m.vec3("emissiveColor", [1.0; 3]),
        _ => m.vec3("diffuseColor", [0.8; 3]),
    };
    // a black diffuse with an emissive colour: the colour is in the emission
    let emissive = m.vec3("emissiveColor", [0.0; 3]);
    if color.max_element() < 0.01 && emissive.max_element() > 0.01 {
        color = emissive;
    }
    let alpha = (1.0 - m.number("transparency", 0.0).clamp(0.0, 1.0)) as f32;
    let [r, g, b] = to8(color);
    let a = (alpha * 255.0).round() as u8;
    let name = m.def.clone().or_else(|| appearance.and_then(|a| a.def.clone())).unwrap_or_else(|| format!("Colour #{r:02X}{g:02X}{b:02X}"));
    (name, [r, g, b, a])
}

/// Triangles of a shape in its local coordinates, with a colour per corner when the geometry has
/// its own colours.
#[derive(Default)]
struct Mesh {
    triangles: Vec<[Vec3; 3]>,
    colors: Vec<[[u8; 4]; 3]>,
    points: Vec<Vec3>,
    point_colors: Vec<[u8; 4]>,
}

fn points3(node: Option<&Rc<Node>>, field: &str) -> Vec<Vec3> {
    node.map(|n| n.numbers(field).chunks_exact(3).map(|c| Vec3::new(c[0] as f32, c[1] as f32, c[2] as f32)).collect()).unwrap_or_default()
}

/// The colours of a `Color` or `ColorRGBA` node.
fn colors(node: Option<&Rc<Node>>) -> Vec<[u8; 4]> {
    let Some(n) = node else { return Vec::new() };
    let stride = if n.kind == "ColorRGBA" { 4 } else { 3 };
    n.numbers("color")
        .chunks_exact(stride)
        .map(|c| {
            let v = |i: usize| (c.get(i).copied().unwrap_or(1.0).clamp(0.0, 1.0) * 255.0).round() as u8;
            [v(0), v(1), v(2), if stride == 4 { v(3) } else { 255 }]
        })
        .collect()
}

/// Polygons of an index list with -1 between them.
fn faces(index: &[f64]) -> Vec<Vec<usize>> {
    index.split(|&i| i < 0.0).filter(|f| !f.is_empty()).map(|f| f.iter().map(|&i| i as usize).collect()).collect()
}

impl Mesh {
    fn triangle(&mut self, t: [Vec3; 3], c: Option<[[u8; 4]; 3]>) {
        self.triangles.push(t);
        if let Some(c) = c {
            self.colors.push(c);
        }
    }

    /// Fan-triangulated polygons, with colours per vertex (`per_vertex`) or per face.
    fn polygons(&mut self, coords: &[Vec3], polygons: &[Vec<usize>], colors: &[[u8; 4]], color_index: &[f64], per_vertex: bool, ccw: bool) {
        let use_colors = !colors.is_empty();
        let mut corner = 0;
        for (f, poly) in polygons.iter().enumerate() {
            let color_of = |k: usize, v: usize| -> [u8; 4] {
                let i = if per_vertex {
                    if color_index.is_empty() { v } else { color_index.get(corner + k + f).map_or(v, |&c| c as usize) }
                } else if color_index.is_empty() {
                    f
                } else {
                    color_index.get(f).map_or(f, |&c| c as usize)
                };
                colors.get(i).copied().unwrap_or([255; 4])
            };
            if poly.len() >= 3 && poly.iter().all(|&i| i < coords.len()) {
                for k in 1..poly.len() - 1 {
                    let mut ids = [0, k, k + 1];
                    if !ccw {
                        ids.swap(1, 2);
                    }
                    let t = ids.map(|i| coords[poly[i]]);
                    let c = use_colors.then(|| ids.map(|i| color_of(i, poly[i])));
                    self.triangle(t, c);
                }
            }
            corner += poly.len();
        }
    }
}

/// The triangles of a geometry node.
fn geometry(g: &Node) -> Mesh {
    let mut mesh = Mesh::default();
    let ccw = g.flag("ccw", true);
    let coord = g.node("coord");
    let coords = points3(coord, "point");
    let cols = colors(g.node("color"));
    let per_vertex = g.flag("colorPerVertex", true);
    let segments = 24;
    match g.kind.as_str() {
        "IndexedFaceSet" => {
            let polys = faces(g.numbers("coordIndex"));
            mesh.polygons(&coords, &polys, &cols, g.numbers("colorIndex"), per_vertex, ccw);
        }
        "IndexedTriangleSet" | "IndexedQuadSet" => {
            let n = if g.kind == "IndexedTriangleSet" { 3 } else { 4 };
            let polys: Vec<Vec<usize>> = g.numbers("index").chunks_exact(n).map(|c| c.iter().map(|&i| i as usize).collect()).collect();
            mesh.polygons(&coords, &polys, &cols, &[], per_vertex, ccw);
        }
        "TriangleSet" | "QuadSet" => {
            let n = if g.kind == "TriangleSet" { 3 } else { 4 };
            let polys: Vec<Vec<usize>> = (0..coords.len() / n).map(|f| (f * n..f * n + n).collect()).collect();
            mesh.polygons(&coords, &polys, &cols, &[], per_vertex, ccw);
        }
        "IndexedTriangleFanSet" | "IndexedTriangleStripSet" | "TriangleFanSet" | "TriangleStripSet" => {
            let runs: Vec<Vec<usize>> = if g.kind.starts_with("Indexed") {
                faces(g.numbers("index"))
            } else {
                let counts = g.numbers(if g.kind == "TriangleFanSet" { "fanCount" } else { "stripCount" });
                let mut at = 0;
                counts.iter().map(|&c| {
                    let run = (at..at + c as usize).collect();
                    at += c as usize;
                    run
                }).collect()
            };
            let fan = g.kind.contains("Fan");
            for run in runs {
                for k in 2..run.len() {
                    let ids = if fan { [run[0], run[k - 1], run[k]] } else if k % 2 == 0 { [run[k - 2], run[k - 1], run[k]] } else { [run[k - 1], run[k - 2], run[k]] };
                    if ids.iter().all(|&i| i < coords.len()) {
                        let mut t = ids.map(|i| coords[i]);
                        if !ccw {
                            t.swap(1, 2);
                        }
                        let c = (!cols.is_empty() && per_vertex).then(|| ids.map(|i| cols.get(i).copied().unwrap_or([255; 4])));
                        mesh.triangle(t, c);
                    }
                }
            }
        }
        "ElevationGrid" => {
            let (nx, nz) = (g.number("xDimension", 0.0) as usize, g.number("zDimension", 0.0) as usize);
            let (sx, sz) = (g.number("xSpacing", 1.0) as f32, g.number("zSpacing", 1.0) as f32);
            let h = g.numbers("height");
            if nx >= 2 && nz >= 2 && h.len() >= nx * nz {
                let p = |i: usize, j: usize| Vec3::new(i as f32 * sx, h[j * nx + i] as f32, j as f32 * sz);
                let coords: Vec<Vec3> = (0..nz).flat_map(|j| (0..nx).map(move |i| (i, j))).map(|(i, j)| p(i, j)).collect();
                let mut polys = Vec::new();
                for j in 0..nz - 1 {
                    for i in 0..nx - 1 {
                        // counter-clockwise seen from above (+Y)
                        polys.push(vec![j * nx + i, (j + 1) * nx + i, (j + 1) * nx + i + 1, j * nx + i + 1]);
                    }
                }
                // colours per vertex, or per quad
                let polys_colors = if per_vertex { cols.clone() } else { Vec::new() };
                let quad_colors = if per_vertex { Vec::new() } else { cols.clone() };
                if per_vertex {
                    mesh.polygons(&coords, &polys, &polys_colors, &[], true, ccw);
                } else {
                    mesh.polygons(&coords, &polys, &quad_colors, &[], false, ccw);
                }
            }
        }
        "Box" => {
            let s = g.vec3("size", [2.0; 3]) / 2.0;
            let corners: Vec<Vec3> = (0..8).map(|i| Vec3::new(if i & 1 == 0 { -s.x } else { s.x }, if i & 2 == 0 { -s.y } else { s.y }, if i & 4 == 0 { -s.z } else { s.z })).collect();
            // outward, counter-clockwise
            let quads = [[0, 4, 6, 2], [1, 3, 7, 5], [0, 1, 5, 4], [2, 6, 7, 3], [0, 2, 3, 1], [4, 5, 7, 6]];
            let polys: Vec<Vec<usize>> = quads.iter().map(|q| q.to_vec()).collect();
            mesh.polygons(&corners, &polys, &[], &[], true, true);
        }
        "Sphere" => {
            let r = g.number("radius", 1.0) as f32;
            let rings = segments / 2;
            let at = |i: usize, j: usize| {
                let (theta, phi) = (std::f32::consts::PI * j as f32 / rings as f32, std::f32::consts::TAU * i as f32 / segments as f32);
                Vec3::new(-theta.sin() * phi.sin(), theta.cos(), -theta.sin() * phi.cos()) * r
            };
            for j in 0..rings {
                for i in 0..segments {
                    let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
                    if j > 0 {
                        mesh.triangle([a, d, b], None);
                    }
                    if j + 1 < rings {
                        mesh.triangle([b, d, c], None);
                    }
                }
            }
        }
        "Cylinder" | "Cone" => {
            let cone = g.kind == "Cone";
            let h = g.number("height", 2.0) as f32 / 2.0;
            let r = g.number(if cone { "bottomRadius" } else { "radius" }, 1.0) as f32;
            let top_r = if cone { 0.0 } else { r };
            let rim = |i: usize, y: f32, r: f32| {
                let a = std::f32::consts::TAU * i as f32 / segments as f32;
                Vec3::new(r * a.sin(), y, r * a.cos())
            };
            for i in 0..segments {
                let (b0, b1, t0, t1) = (rim(i, -h, r), rim(i + 1, -h, r), rim(i, h, top_r), rim(i + 1, h, top_r));
                if g.flag("side", true) {
                    mesh.triangle([b0, b1, t1], None);
                    if !cone {
                        mesh.triangle([b0, t1, t0], None);
                    }
                }
                if g.flag("bottom", true) {
                    mesh.triangle([Vec3::new(0.0, -h, 0.0), b1, b0], None);
                }
                if !cone && g.flag("top", true) {
                    mesh.triangle([Vec3::new(0.0, h, 0.0), t0, t1], None);
                }
            }
        }
        "PointSet" => {
            mesh.points = coords;
            mesh.point_colors = cols;
        }
        _ => {}
    }
    mesh
}

impl Scene {
    fn node(&mut self, node: &Rc<Node>, parent: Mat4, name: Option<&str>, depth: usize) {
        if depth > 100 {
            return;
        }
        let name = node.def.as_deref().or(name);
        let mut world = parent;
        match node.kind.as_str() {
            "Transform" | "CADPart" | "HAnimJoint" | "HAnimSegment" => {
                let (c, sr) = (node.vec3("center", [0.0; 3]), node.rotation("scaleOrientation"));
                world = parent
                    * Mat4::from_translation(node.vec3("translation", [0.0; 3]) + c)
                    * Mat4::from_quat(node.rotation("rotation"))
                    * Mat4::from_quat(sr)
                    * Mat4::from_scale(node.vec3("scale", [1.0; 3]))
                    * Mat4::from_quat(sr.inverse())
                    * Mat4::from_translation(-c);
            }
            "Shape" => return self.shape(node, world, name),
            "Switch" => {
                let choice = node.number("whichChoice", -1.0);
                let children = if node.get("choice").is_some() { node.nodes("choice") } else { node.nodes("children") };
                if choice >= 0.0 {
                    if let Some(c) = children.get(choice as usize) {
                        self.node(c, world, name, depth + 1);
                    }
                }
                return;
            }
            "LOD" => {
                // the first level is the most detailed
                let levels = if node.get("level").is_some() { node.nodes("level") } else { node.nodes("children") };
                if let Some(c) = levels.first() {
                    self.node(c, world, name, depth + 1);
                }
                return;
            }
            _ => {}
        }
        for field in ["children", "shape"] {
            for c in node.nodes(field) {
                self.node(c, world, name, depth + 1);
            }
        }
    }

    fn shape(&mut self, shape: &Node, world: Mat4, name: Option<&str>) {
        let Some(g) = shape.node("geometry") else { return };
        let mesh = geometry(g);
        if mesh.triangles.is_empty() && mesh.points.is_empty() {
            return;
        }
        let (material, rgba) = material(shape);
        let id = self.next_id + 1;
        self.next_id = id;
        if !mesh.points.is_empty() {
            let key = format!("{material} (points)");
            let layer = self.layers.entry(key.clone()).or_insert_with(|| Layer {
                name: key,
                color: [rgba[0], rgba[1], rgba[2]].map(|c| c as f32 / 255.0),
                colors: Some(Vec::new()),
                points: true,
                ..Layer::default()
            });
            for (k, p) in mesh.points.iter().enumerate() {
                layer.positions.extend_from_slice(&world.transform_point3(*p).to_array());
                layer.colors.as_mut().unwrap().push(mesh.point_colors.get(k).copied().unwrap_or(rgba));
            }
            return;
        }
        let layer = self.layers.entry(material.clone()).or_insert_with(|| Layer {
            name: material.clone(),
            color: [rgba[0], rgba[1], rgba[2]].map(|c| c as f32 / 255.0),
            colors: Some(Vec::new()),
            ..Layer::default()
        });
        let mirrored = world.determinant() < 0.0;
        let first = layer.indices.len() as u32;
        for (k, t) in mesh.triangles.iter().enumerate() {
            let mut p = t.map(|v| world.transform_point3(v));
            let mut c = mesh.colors.get(k).copied().unwrap_or([rgba; 3]);
            if mirrored {
                p.swap(1, 2);
                c.swap(1, 2);
            }
            let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
            if n == Vec3::ZERO {
                continue;
            }
            for i in 0..3 {
                layer.indices.push(layer.vertex_count() as u32);
                layer.positions.extend_from_slice(&p[i].to_array());
                layer.normals.extend_from_slice(&n.to_array());
                // the material's transparency stays with the per-vertex colours
                let mut ci = c[i];
                ci[3] = ci[3].min(rgba[3]);
                layer.colors.as_mut().unwrap().push(ci);
            }
        }
        let count = layer.indices.len() as u32 - first;
        if count > 0 {
            layer.elements.push((id, first, count));
            let mut props = serde_json::Map::new();
            props.insert("Name".into(), name.unwrap_or(&g.kind).into());
            props.insert("Geometry".into(), g.kind.clone().into());
            props.insert("Material".into(), material.into());
            layer.props.push((id, serde_json::Value::Object(props).to_string()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loaders::{load_with, Progress};
    use std::sync::Arc;

    fn load(name: &str, data: &[u8]) -> crate::model::Model {
        load_with(&Source::Bytes { name: name.into(), data: Arc::new(data.to_vec()) }, &Progress::default()).expect(name)
    }

    /// A red unit square used twice (DEF/USE), once moved 5 m up (Y), a box of 2 x 4 x 6 m with
    /// a material without a name, and a script and a route that are skipped.
    const WRL: &str = r#"#VRML V2.0 utf8
# a comment
DEF Root Transform {
  children [
    DEF Floor Shape {
      appearance Appearance { material DEF Red Material { diffuseColor 1 0 0 } }
      geometry IndexedFaceSet { coord Coordinate { point [ 0 0 0, 1 0 0, 1 0 1, 0 0 1 ] } coordIndex [ 0 3 2 1 -1 ] }
    }
    Transform { translation 0 5 0 children [ USE Floor ] }
    DEF Crate Transform {
      translation 10 0 0
      children Shape { appearance Appearance { material Material { diffuseColor 0 0 1 transparency 0.5 } } geometry Box { size 2 4 6 } }
    }
    DEF Clock TimeSensor { cycleInterval 4 loop TRUE }
    Script { field SFFloat speed 1.5 eventIn SFTime tick url "javascript: function tick() {}" }
  ]
}
ROUTE Clock.fraction_changed TO Root.set_rotation
"#;

    #[test]
    fn reads_vrml_with_def_use_and_primitives() {
        let model = load("scene.WRL", WRL.as_bytes());
        assert_eq!(model.format, "VRML / X3D");
        let names: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Colour #0000FF", "Red"]);
        let red = &model.layers[1];
        assert_eq!(red.triangle_count(), 4);
        assert_eq!(red.color, [1.0, 0.0, 0.0]);
        // Y up becomes Z up: the copy moved 5 up is at z = 5, facing up
        let z: Vec<f32> = red.positions.chunks_exact(3).map(|p| p[2]).collect();
        assert!(z.iter().any(|&z| (z - 5.0).abs() < 1e-5));
        assert!(red.normals.chunks_exact(3).all(|n| n[2] > 0.99), "{:?}", red.normals);
        // the box: 12 triangles, 2 m along x, 6 m along y (-z in the file), 4 m high
        let boxes = &model.layers[0];
        assert_eq!(boxes.triangle_count(), 12);
        assert_eq!(boxes.colors.as_ref().unwrap()[0], [0, 0, 255, 128]);
        assert!(model.props.values().any(|p| p.contains("\"Name\":\"Crate\"") && p.contains("\"Geometry\":\"Box\"")));
        assert!(model.props.values().any(|p| p.contains("\"Name\":\"Floor\"")));
        let size = model.bounds.size();
        // x 0..11, y -3..3 (the box), z -2..5 (the box and the copy 5 m up)
        assert!((size[0] - 11.0).abs() < 1e-4 && (size[1] - 6.0).abs() < 1e-4 && (size[2] - 7.0).abs() < 1e-4, "{size:?}");
    }

    #[test]
    fn reads_x3d_xml_and_classic_alike() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE X3D PUBLIC "ISO//Web3D//DTD X3D 3.3//EN" "https://www.web3d.org/specifications/x3d-3.3.dtd">
<X3D profile="Interchange" version="3.3"><head><meta name="title" content="t"/></head><Scene>
  <Transform DEF="Part" translation="1 0 0" rotation="0 1 0 1.5708">
    <Shape><Appearance><Material DEF="Steel" diffuseColor="0.5 0.5 0.5"/></Appearance>
      <IndexedTriangleSet index="0 1 2"><Coordinate point="0 0 0, 1 0 0, 0 1 0"/></IndexedTriangleSet></Shape>
  </Transform>
  <Transform translation="0 0 3"><Shape USE="NotThere"/><Shape><Appearance><Material USE="Steel"/></Appearance><Sphere radius="0.5"/></Shape></Transform>
  <Switch whichChoice="-1"><Shape><Box/></Shape></Switch>
</Scene></X3D>"#;
        let model = load("part.x3d", xml.as_bytes());
        let names: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Steel"]);
        let steel = &model.layers[0];
        // the triangle and a 24 x 12 sphere; the hidden box is not drawn
        assert_eq!(steel.triangle_count(), 1 + 24 * 11 * 2);
        assert!(model.props.values().any(|p| p.contains("\"Name\":\"Part\"")));

        let classic = "#X3D V3.3 utf8\nPROFILE Interchange\nCOMPONENT Geometry3D:2\nMETA \"title\" \"t\"\nUNIT length millimetre 0.001\n\
            DEF Part Transform { translation 1000 0 0 children Shape { geometry Cylinder { radius 500 height 2000 } } }\n";
        let model = load("part.x3dv", classic.as_bytes());
        // 24 sides of 2 triangles, two caps of 24: in metres, 1 x 1 x 2 m
        assert_eq!(model.triangle_count(), 24 * 4);
        let size = model.bounds.size();
        assert!((size[0] - 1.0).abs() < 1e-3 && (size[2] - 2.0).abs() < 1e-3, "{size:?}");
        assert_eq!(model.layers[0].name, "Default");
    }

    #[test]
    fn colours_per_face_elevation_grid_and_gzip() {
        let wrl = "#VRML V2.0 utf8\nShape { geometry IndexedFaceSet { coord Coordinate { point [0 0 0 1 0 0 1 1 0 0 1 0] }\n\
            coordIndex [0 1 2 -1 0 2 3 -1] color Color { color [1 0 0, 0 1 0] } colorPerVertex FALSE } }\n\
            Shape { geometry ElevationGrid { xDimension 3 zDimension 2 xSpacing 1 zSpacing 1 height [0 1 0 0 1 0] } }\n";
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, wrl.as_bytes()).unwrap();
        let model = load("grid.wrz", &gz.finish().unwrap());
        let layer = &model.layers[0];
        assert_eq!(layer.triangle_count(), 2 + 4);
        let c = layer.colors.as_ref().unwrap();
        assert_eq!((c[0], c[3]), ([255, 0, 0, 255], [0, 255, 0, 255]));
        // the grid rises to 1 m in the middle (y up: z in the model)
        assert!((model.bounds.max[2] - 1.0).abs() < 1e-5);
        // the grid faces up
        assert!(layer.normals[18..].chunks_exact(3).all(|n| n[2] > 0.0), "{:?}", &layer.normals[18..]);
    }

    #[test]
    fn rejects_vrml1_and_other_text() {
        let source = Source::Bytes { name: "old.wrl".into(), data: Arc::new(b"#VRML V1.0 ascii\nSeparator { Cube {} }\n".to_vec()) };
        assert!(VrmlLoader.load(&source).unwrap_err().contains("1.0"));
        let source = Source::Bytes { name: "x.x3d".into(), data: Arc::new(b"<svg/>".to_vec()) };
        assert!(VrmlLoader.load(&source).is_err());
    }
}
