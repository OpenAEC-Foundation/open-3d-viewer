//! How an IFCX file is built up, for the IFC tab: its text as lines (in the file's own order,
//! objects opened up, lists of numbers on one line); its `data` entries (path, children,
//! inherits and attributes) with the line each starts on; the nodes they make (the entries of one
//! path put together, the later winning); the tree of those nodes from the roots along their
//! children and inherits; and the relations between them, named after the IFC relation they
//! stand for.

use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

/// Characters of a line kept for showing; a longer line (a mesh's points) is cut there.
pub const LINE_CHARS: usize = 1500;

/// One line of the text.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// Indentation steps.
    pub depth: u16,
    /// The line without its indentation, cut at [`LINE_CHARS`].
    pub text: String,
    /// Its length before it was cut (characters).
    pub chars: usize,
    /// The `data` entry it belongs to.
    pub entry: Option<u32>,
}

/// One entry of `data`: a node, or a layer of a node given more than once.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Entry {
    pub path: String,
    /// By name; `None` removes the child a weaker layer gave.
    pub children: Vec<(String, Option<String>)>,
    pub inherits: Vec<(String, Option<String>)>,
    /// Attribute keys with a short form of their value.
    pub attributes: Vec<(String, String)>,
    /// The `bsi::ifc::class` code and the `bsi::ifc::prop::Name`, when it gives them.
    pub class: Option<String>,
    pub name: Option<String>,
    /// The first line of the entry in the text.
    pub line: u32,
}

/// The entries of one path put together: children and inherits by name (the later entry
/// winning, `null` taking one out), the attribute keys, the class and name.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Node {
    pub path: String,
    pub children: Vec<(String, String)>,
    pub inherits: Vec<(String, String)>,
    pub attributes: Vec<String>,
    pub class: Option<String>,
    pub name: Option<String>,
    /// The entries it is made of, in file order.
    pub entries: Vec<u32>,
}

impl Node {
    /// Its name, else its class, else its path.
    pub fn label(&self) -> &str {
        self.name.as_deref().or(self.class.as_deref()).unwrap_or(&self.path)
    }
}

/// What a link between two nodes stands for in IFC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Relation {
    /// A spatial element made of others (project, site, building, storey), or an element of
    /// parts: `IfcRelAggregates`.
    Aggregates,
    /// An element in a storey or space: `IfcRelContainedInSpatialStructure`.
    Contains,
    /// An opening in an element: `IfcRelVoidsElement`.
    Voids,
    /// A door or window in an opening: `IfcRelFillsElement`.
    Fills,
    /// A child without a class: its geometry or another part of it.
    Part,
    /// Inherits a classified node, its type: `IfcRelDefinesByType`.
    Type,
    /// Inherits a material: `IfcRelAssociatesMaterial`.
    Material,
    /// Inherits another node.
    Inherits,
}

impl Relation {
    pub const ALL: [Relation; 8] = [Relation::Aggregates, Relation::Contains, Relation::Voids, Relation::Fills, Relation::Part, Relation::Type, Relation::Material, Relation::Inherits];

    /// The IFC relation it stands for.
    pub fn ifc(self) -> &'static str {
        match self {
            Relation::Aggregates => "IfcRelAggregates",
            Relation::Contains => "IfcRelContainedInSpatialStructure",
            Relation::Voids => "IfcRelVoidsElement",
            Relation::Fills => "IfcRelFillsElement",
            Relation::Part => "children",
            Relation::Type => "IfcRelDefinesByType",
            Relation::Material => "IfcRelAssociatesMaterial",
            Relation::Inherits => "inherits",
        }
    }
}

/// A link from a node to another, by its name there.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub from: u32,
    pub name: String,
    pub kind: Relation,
    /// The path it points at, and that node when the file has it.
    pub target: String,
    pub to: Option<u32>,
}

/// How a row of the tree hangs from the one above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Root,
    Child,
    Inherits,
}

/// One row of the tree: a node where it hangs.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeRow {
    /// Where it hangs: the names from its root, joined by [`KEY_SEP`] (an inherit's with `^`).
    pub key: String,
    pub depth: u16,
    /// Its name as a child (a root's path).
    pub label: String,
    pub via: Via,
    pub target: String,
    pub node: Option<u32>,
    /// It has children or inherits; and they show.
    pub kids: bool,
    pub open: bool,
    /// It holds itself (further up): not opened.
    pub cycle: bool,
}

/// Joins the names of a tree key.
pub const KEY_SEP: char = '\u{1f}';

/// An IFCX file taken apart.
#[derive(Debug, Clone, Default)]
pub struct Structure {
    /// Where it comes from: the file's name, or the IFCX Save as writes.
    pub source: String,
    pub bytes: usize,
    pub lines: Vec<Line>,
    pub entries: Vec<Entry>,
    /// The nodes, in the order their path first comes in the file, and by path.
    pub nodes: Vec<Node>,
    pub index: HashMap<String, u32>,
    /// The nodes no other holds or inherits.
    pub roots: Vec<u32>,
    pub links: Vec<Link>,
    /// Where each node first hangs in the tree along children (its tree key).
    pub keys: HashMap<u32, String>,
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    data: Vec<RawNode>,
}

#[derive(Deserialize)]
struct RawNode {
    path: String,
    #[serde(default)]
    children: Option<BTreeMap<String, Option<String>>>,
    #[serde(default)]
    inherits: Option<BTreeMap<String, Option<String>>>,
    #[serde(default)]
    attributes: Option<serde_json::Map<String, Value>>,
}

impl Structure {
    /// Takes an IFCX file apart; `source` says where it comes from.
    pub fn read(source: &str, data: &[u8]) -> Result<Structure, String> {
        let data = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(data);
        let text = std::str::from_utf8(data).map_err(|e| format!("IFCX: not UTF-8 ({e})"))?;
        let file: File = serde_json::from_str(text).map_err(|e| format!("IFCX: not an IFCX file ({e})"))?;
        let (lines, starts) = format_lines(text);
        let entries = file
            .data
            .into_iter()
            .enumerate()
            .map(|(i, node)| {
                let attributes = node.attributes.unwrap_or_default();
                let class = attributes.get("bsi::ifc::class").and_then(|c| c.get("code").unwrap_or(c).as_str()).map(str::to_string);
                let name = attributes.get("bsi::ifc::prop::Name").and_then(Value::as_str).map(str::to_string);
                Entry {
                    path: node.path,
                    children: node.children.unwrap_or_default().into_iter().collect(),
                    inherits: node.inherits.unwrap_or_default().into_iter().collect(),
                    attributes: attributes.iter().map(|(k, v)| (k.clone(), short(v))).collect(),
                    class,
                    name,
                    line: starts.get(i).copied().unwrap_or(0),
                }
            })
            .collect::<Vec<Entry>>();
        let mut s = Structure { source: source.to_string(), bytes: data.len(), lines, entries, ..Structure::default() };
        s.put_together(&file_nulls(text));
        Ok(s)
    }

    /// The nodes from the entries, the roots, the links and the tree keys.
    fn put_together(&mut self, nulls: &HashSet<(u32, String)>) {
        let mut children: Vec<BTreeMap<String, Option<String>>> = Vec::new();
        let mut inherits: Vec<BTreeMap<String, Option<String>>> = Vec::new();
        let mut attributes: Vec<BTreeMap<String, bool>> = Vec::new();
        for (i, e) in self.entries.iter().enumerate() {
            let k = match self.index.get(&e.path) {
                Some(&k) => k as usize,
                None => {
                    self.index.insert(e.path.clone(), self.nodes.len() as u32);
                    self.nodes.push(Node { path: e.path.clone(), ..Node::default() });
                    children.push(BTreeMap::new());
                    inherits.push(BTreeMap::new());
                    attributes.push(BTreeMap::new());
                    self.nodes.len() - 1
                }
            };
            let node = &mut self.nodes[k];
            node.entries.push(i as u32);
            children[k].extend(e.children.iter().cloned());
            inherits[k].extend(e.inherits.iter().cloned());
            for (key, _) in &e.attributes {
                let removed = nulls.contains(&(i as u32, key.clone()));
                attributes[k].insert(key.clone(), !removed);
                if key == "bsi::ifc::class" {
                    node.class = if removed { None } else { e.class.clone() };
                }
                if key == "bsi::ifc::prop::Name" {
                    node.name = if removed { None } else { e.name.clone() };
                }
            }
        }
        for (k, node) in self.nodes.iter_mut().enumerate() {
            node.children = children[k].iter().filter_map(|(n, t)| Some((n.clone(), t.clone()?))).collect();
            node.inherits = inherits[k].iter().filter_map(|(n, t)| Some((n.clone(), t.clone()?))).collect();
            node.attributes = attributes[k].iter().filter(|(_, on)| **on).map(|(a, _)| a.clone()).collect();
        }
        // roots: what no node holds or inherits (by the first part of the path pointed at)
        let used: HashSet<&str> = self.nodes.iter().flat_map(|n| n.children.iter().chain(&n.inherits)).map(|(_, t)| t.split('/').next().unwrap_or(t)).collect();
        self.roots = (0..self.nodes.len() as u32)
            .filter(|&k| {
                let p = &self.nodes[k as usize].path;
                !p.contains('/') && !used.contains(p.as_str())
            })
            .collect();
        // the links, named after what they stand for
        let mut links = Vec::new();
        for (k, node) in self.nodes.iter().enumerate() {
            for (name, target) in &node.children {
                let to = self.index.get(target).copied();
                let kind = child_relation(node.class.as_deref(), to.and_then(|t| self.nodes[t as usize].class.as_deref()));
                links.push(Link { from: k as u32, name: name.clone(), kind, target: target.clone(), to });
            }
            for (name, target) in &node.inherits {
                let to = self.index.get(target).copied();
                let t = to.map(|t| &self.nodes[t as usize]);
                let kind = if t.is_some_and(|t| t.class.is_some()) {
                    Relation::Type
                } else if t.is_some_and(|t| t.attributes.iter().any(|a| a == "bsi::ifc::material")) || name == "material" {
                    Relation::Material
                } else {
                    Relation::Inherits
                };
                links.push(Link { from: k as u32, name: name.clone(), kind, target: target.clone(), to });
            }
        }
        self.links = links;
        // where each node first hangs: along children, breadth first from the roots
        let mut queue: VecDeque<(u32, String)> = self.roots.iter().map(|&r| (r, self.nodes[r as usize].path.clone())).collect();
        while let Some((k, key)) = queue.pop_front() {
            if self.keys.contains_key(&k) {
                continue;
            }
            for (name, target) in &self.nodes[k as usize].children {
                if let Some(&t) = self.index.get(target) {
                    if !self.keys.contains_key(&t) {
                        queue.push_back((t, format!("{key}{KEY_SEP}{name}")));
                    }
                }
            }
            self.keys.insert(k, key);
        }
    }

    /// The rows of the tree with the keys in `open` opened.
    pub fn tree_rows(&self, open: &HashSet<String>) -> Vec<TreeRow> {
        let mut rows = Vec::new();
        let mut stack: Vec<u32> = Vec::new();
        for &r in &self.roots {
            let path = self.nodes[r as usize].path.clone();
            self.row(&mut rows, &mut stack, open, path.clone(), 0, path.clone(), Via::Root, &path);
        }
        rows
    }

    #[allow(clippy::too_many_arguments)]
    fn row(&self, rows: &mut Vec<TreeRow>, stack: &mut Vec<u32>, open: &HashSet<String>, key: String, depth: u16, label: String, via: Via, target: &str) {
        let node = self.index.get(target).copied();
        let n = node.map(|k| &self.nodes[k as usize]);
        let kids = n.is_some_and(|n| !n.children.is_empty() || !n.inherits.is_empty());
        let cycle = node.is_some_and(|k| stack.contains(&k));
        let is_open = kids && !cycle && open.contains(&key);
        rows.push(TreeRow { key: key.clone(), depth, label, via, target: target.to_string(), node, kids, open: is_open, cycle });
        if !is_open || rows.len() > 500_000 {
            return;
        }
        let (Some(k), Some(n)) = (node, n) else { return };
        stack.push(k);
        for (name, t) in &n.children {
            self.row(rows, stack, open, format!("{key}{KEY_SEP}{name}"), depth + 1, name.clone(), Via::Child, t);
        }
        for (name, t) in &n.inherits {
            self.row(rows, stack, open, format!("{key}{KEY_SEP}^{name}"), depth + 1, name.clone(), Via::Inherits, t);
        }
        stack.pop();
    }

    /// The keys to open so that a node shows: those above where it first hangs.
    pub fn keys_above(&self, node: u32) -> Vec<String> {
        let Some(key) = self.keys.get(&node) else { return Vec::new() };
        let parts: Vec<&str> = key.split(KEY_SEP).collect();
        (1..parts.len()).map(|n| parts[..n].join(&KEY_SEP.to_string())).collect()
    }

    /// The nodes above where a node first hangs, from its root down to its parent.
    pub fn nodes_above(&self, node: u32) -> Vec<u32> {
        let Some(key) = self.keys.get(&node) else { return Vec::new() };
        let mut parts = key.split(KEY_SEP);
        let Some(mut at) = parts.next().and_then(|root| self.index.get(root).copied()) else { return Vec::new() };
        let mut out = Vec::new();
        for name in parts {
            out.push(at);
            let next = self.nodes[at as usize].children.iter().find(|(n, _)| n == name).and_then(|(_, t)| self.index.get(t).copied());
            let Some(next) = next else { break };
            at = next;
        }
        out
    }

    /// A node and those under it along children (each once), at most `limit`.
    pub fn below(&self, node: u32, limit: usize) -> Vec<u32> {
        let mut seen = HashSet::from([node]);
        let mut out = vec![node];
        let mut k = 0;
        while k < out.len() && out.len() < limit {
            for (_, t) in &self.nodes[out[k] as usize].children {
                if let Some(&t) = self.index.get(t) {
                    if seen.insert(t) {
                        out.push(t);
                    }
                }
            }
            k += 1;
        }
        out.truncate(limit);
        out
    }
}

/// The relation a child stands for, from the classes of the two nodes.
fn child_relation(parent: Option<&str>, child: Option<&str>) -> Relation {
    let spatial = |c: &str| {
        let c = c.to_ascii_lowercase();
        ["ifcproject", "ifcsite", "ifcbuilding", "ifcbuildingstorey", "ifcspace", "ifcfacility", "ifcfacilitypart", "ifcbridge", "ifcroad", "ifcrailway", "ifcmarinefacility", "ifcspatialzone"]
            .contains(&c.as_str())
    };
    let opening = |c: &str| ["ifcopeningelement", "ifcopeningstandardcase", "ifcvoidingfeature"].contains(&c.to_ascii_lowercase().as_str());
    match (parent, child) {
        (Some(p), Some(c)) if spatial(p) && spatial(c) => Relation::Aggregates,
        (Some(p), Some(_)) if spatial(p) => Relation::Contains,
        (Some(_), Some(c)) if opening(c) => Relation::Voids,
        (Some(p), Some(_)) if opening(p) => Relation::Fills,
        (Some(_), Some(_)) => Relation::Aggregates,
        _ => Relation::Part,
    }
}

/// The attributes an entry sets to `null` (they take out what a weaker entry gave): entry, key.
fn file_nulls(text: &str) -> HashSet<(u32, String)> {
    #[derive(Deserialize)]
    struct Nulls {
        #[serde(default)]
        data: Vec<NullNode>,
    }
    #[derive(Deserialize)]
    struct NullNode {
        #[serde(default)]
        attributes: Option<serde_json::Map<String, Value>>,
    }
    let Ok(file) = serde_json::from_str::<Nulls>(text) else { return HashSet::new() };
    file.data
        .iter()
        .enumerate()
        .flat_map(|(i, n)| n.attributes.iter().flatten().filter(|(_, v)| v.is_null()).map(move |(k, _)| (i as u32, k.clone())))
        .collect()
}

/// A value in a few words: a text or number as it is, a list or object by its size.
pub fn short(value: &Value) -> String {
    let cut = |s: String| if s.chars().count() > 60 { s.chars().take(59).chain(['…']).collect() } else { s };
    match value {
        Value::String(s) => cut(s.clone()),
        Value::Array(items) if items.iter().all(|v| !v.is_array() && !v.is_object()) && items.len() <= 4 => cut(value.to_string()),
        Value::Array(items) => format!("[{} items]", items.len()),
        Value::Object(fields) => match fields.get("code").and_then(Value::as_str) {
            Some(code) => cut(code.to_string()),
            None => format!("{{{} fields}}", fields.len()),
        },
        other => cut(other.to_string()),
    }
}

/// The text as lines: every object member on its own line, a list on one line when it holds no
/// object (the points of a mesh), else one item per line; and the line each `data` entry starts on.
/// Works on the text itself, so the members keep the file's order.
pub fn format_lines(text: &str) -> (Vec<Line>, Vec<u32>) {
    let b = text.as_bytes();
    let mut lines = Vec::new();
    let mut starts = Vec::new();
    let mut cur = String::new();
    let mut depth: u16 = 0;
    // the containers we are in: true for an object
    let mut stack: Vec<bool> = Vec::new();
    // the top-level member we are in, and the entry of `data` with the stack depth it opened at
    let mut top_key: Option<String> = None;
    let mut entry: Option<(u32, usize)> = None;
    let mut entries: u32 = 0;
    let flush = |cur: &mut String, depth: u16, entry: Option<(u32, usize)>, lines: &mut Vec<Line>| {
        if cur.is_empty() {
            return;
        }
        let chars = cur.chars().count();
        let text = if chars > LINE_CHARS { cur.chars().take(LINE_CHARS).collect() } else { cur.clone() };
        lines.push(Line { depth, text, chars, entry: entry.map(|e| e.0) });
        cur.clear();
    };
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            b' ' | b'\t' | b'\r' | b'\n' => {
                i += 1;
            }
            b'{' => {
                // an entry of `data` starts here
                if stack.len() == 2 && stack == [true, false] && top_key.as_deref() == Some("data") {
                    flush(&mut cur, depth, entry, &mut lines);
                    entry = Some((entries, stack.len() + 1));
                    starts.push(lines.len() as u32);
                    entries += 1;
                }
                cur.push('{');
                i += 1;
                if next_byte(b, i) == Some(b'}') {
                    cur.push('}');
                    i = skip_ws(b, i) + 1;
                    // an empty entry is one line
                    if entry.is_some_and(|(_, at)| at == stack.len() + 1) {
                        if next_byte(b, i) == Some(b',') {
                            cur.push(',');
                            i = skip_ws(b, i) + 1;
                        }
                        flush(&mut cur, depth, entry, &mut lines);
                        entry = None;
                    }
                } else {
                    stack.push(true);
                    flush(&mut cur, depth, entry, &mut lines);
                    depth += 1;
                }
            }
            b'[' => {
                let end = matching(b, i);
                if end.is_some_and(|end| !holds_object(&b[i..end])) {
                    let end = end.unwrap_or(b.len() - 1);
                    compact(&text[i..=end], &mut cur);
                    i = end + 1;
                } else {
                    cur.push('[');
                    i += 1;
                    stack.push(false);
                    flush(&mut cur, depth, entry, &mut lines);
                    depth += 1;
                }
            }
            b'}' | b']' => {
                flush(&mut cur, depth, entry, &mut lines);
                depth = depth.saturating_sub(1);
                cur.push(c as char);
                i += 1;
                let closed = stack.len();
                stack.pop();
                if entry.is_some_and(|(_, at)| at == closed) {
                    // the entry's closing line (and a comma after it) is still its own
                    if next_byte(b, i) == Some(b',') {
                        cur.push(',');
                        i = skip_ws(b, i) + 1;
                    }
                    flush(&mut cur, depth, entry, &mut lines);
                    entry = None;
                }
            }
            b',' => {
                cur.push(',');
                i += 1;
                flush(&mut cur, depth, entry, &mut lines);
            }
            b':' => {
                cur.push_str(": ");
                i += 1;
            }
            b'"' => {
                let end = string_end(b, i);
                let s = &text[i..end];
                // a key of the top-level object
                if stack.len() == 1 && next_byte(b, end) == Some(b':') {
                    top_key = serde_json::from_str::<String>(s).ok();
                }
                cur.push_str(s);
                i = end;
            }
            _ => {
                let start = i;
                while i < b.len() && !matches!(b[i], b',' | b'}' | b']' | b':' | b' ' | b'\t' | b'\r' | b'\n') {
                    i += 1;
                }
                cur.push_str(&text[start..i]);
                if i == start {
                    i += 1;
                }
            }
        }
    }
    flush(&mut cur, depth, entry, &mut lines);
    (lines, starts)
}

/// The next byte that is not white space.
fn next_byte(b: &[u8], i: usize) -> Option<u8> {
    b.get(skip_ws(b, i)).copied()
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\r' | b'\n') {
        i += 1;
    }
    i
}

/// The index just past the string starting at `i` (its opening quote).
fn string_end(b: &[u8], i: usize) -> usize {
    let mut k = i + 1;
    while k < b.len() {
        match b[k] {
            b'\\' => k += 2,
            b'"' => return k + 1,
            _ => k += 1,
        }
    }
    b.len()
}

/// The index of the bracket closing the list that opens at `i`.
fn matching(b: &[u8], i: usize) -> Option<usize> {
    let mut level = 0usize;
    let mut k = i;
    while k < b.len() {
        match b[k] {
            b'"' => {
                k = string_end(b, k);
                continue;
            }
            b'[' | b'{' => level += 1,
            b']' | b'}' => {
                level -= 1;
                if level == 0 {
                    return Some(k);
                }
            }
            _ => {}
        }
        k += 1;
    }
    None
}

/// Whether a list holds an object (outside its strings).
fn holds_object(b: &[u8]) -> bool {
    let mut k = 0;
    while k < b.len() {
        match b[k] {
            b'"' => {
                k = string_end(b, k);
                continue;
            }
            b'{' => return true,
            _ => {}
        }
        k += 1;
    }
    false
}

/// A list on one line: no white space but a space after each comma.
fn compact(s: &str, out: &mut String) {
    let b = s.as_bytes();
    let mut k = 0;
    while k < b.len() {
        match b[k] {
            b'"' => {
                let end = string_end(b, k).min(b.len());
                out.push_str(&s[k..end]);
                k = end;
                continue;
            }
            b' ' | b'\t' | b'\r' | b'\n' => {}
            b',' => out.push_str(", "),
            _ => {
                // copy a run of plain bytes at once (they are ASCII here: digits, signs, letters)
                let start = k;
                while k < b.len() && !matches!(b[k], b'"' | b' ' | b'\t' | b'\r' | b'\n' | b',') {
                    k += 1;
                }
                out.push_str(&s[start..k]);
                continue;
            }
        }
        k += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Header members in an order that is not alphabetical, two entries, a mesh.
    const FILE: &str = r#"{"header": {"ifcxVersion": "ifcx_alpha", "id": "x"},
 "imports": [], "schemas": {},
 "data": [
  {"path": "p1", "children": {"Body": "m1"}, "attributes": {"bsi::ifc::class": {"code": "IfcWall"}, "bsi::ifc::prop::Name": "Wall"}},
  {"path": "m1", "inherits": {"type": "t1"}, "attributes": {"usd::usdgeom::mesh": {"points": [[0,0,0],[1, 0,0]], "faceVertexIndices": [0,1,1]}}}
 ]}"#;

    fn texts(lines: &[Line]) -> Vec<String> {
        lines.iter().map(|l| format!("{}{}", "  ".repeat(l.depth as usize), l.text)).collect()
    }

    #[test]
    fn the_text_keeps_the_files_order_and_puts_lists_of_numbers_on_one_line() {
        let (lines, _) = format_lines(FILE);
        let t = texts(&lines);
        assert_eq!(&t[..5], ["{", "  \"header\": {", "    \"ifcxVersion\": \"ifcx_alpha\",", "    \"id\": \"x\"", "  },"], "{t:#?}");
        assert!(t.contains(&"  \"imports\": [],".to_string()) && t.contains(&"  \"schemas\": {},".to_string()), "{t:#?}");
        assert!(t.contains(&"          \"points\": [[0, 0, 0], [1, 0, 0]],".to_string()), "{t:#?}");
        assert_eq!(t.last().map(String::as_str), Some("}"));
    }

    #[test]
    fn every_line_of_an_entry_knows_it() {
        let (lines, starts) = format_lines(FILE);
        assert_eq!(starts.len(), 2);
        assert_eq!(lines[starts[0] as usize].text, "{");
        assert_eq!(lines[starts[1] as usize - 1].text, "},", "the first entry ends on the line before the second");
        assert!(lines[starts[0] as usize..starts[1] as usize].iter().all(|l| l.entry == Some(0)));
        assert!(lines[starts[1] as usize..].iter().take_while(|l| l.text != "]").all(|l| l.entry == Some(1)));
        assert_eq!(lines.iter().filter(|l| l.entry.is_none()).map(|l| l.text.as_str()).last(), Some("}"));
    }

    #[test]
    fn a_long_line_is_cut_but_keeps_its_length() {
        let points: Vec<String> = (0..2000).map(|k| format!("[{k},0,0]")).collect();
        let file = format!(r#"{{"data": [{{"path": "m", "attributes": {{"points": [{}]}}}}]}}"#, points.join(","));
        let (lines, _) = format_lines(&file);
        let long = lines.iter().find(|l| l.text.starts_with("\"points\"")).unwrap();
        assert_eq!(long.text.chars().count(), LINE_CHARS);
        assert!(long.chars > 10_000);
    }

    #[test]
    fn the_entries_with_their_children_inherits_and_attributes() {
        let s = Structure::read("x.ifcx", FILE.as_bytes()).unwrap();
        assert_eq!(s.entries.len(), 2);
        let wall = &s.entries[0];
        assert_eq!((wall.path.as_str(), wall.class.as_deref(), wall.name.as_deref()), ("p1", Some("IfcWall"), Some("Wall")));
        assert_eq!(wall.children, [("Body".to_string(), Some("m1".to_string()))]);
        assert_eq!(s.lines[wall.line as usize].text, "{");
        let mesh = &s.entries[1];
        assert_eq!(mesh.inherits, [("type".to_string(), Some("t1".to_string()))]);
        assert_eq!(mesh.attributes, [("usd::usdgeom::mesh".to_string(), "{2 fields}".to_string())]);
        assert!(Structure::read("y.ifcx", b"{\"data\": 3}").is_err());
    }

    /// A project with a building and a storey holding a wall with an opening, a door in the
    /// opening, the wall's body, a wall type and a material; the wall is given twice, the second
    /// entry taking its name out and adding a child.
    const HOUSE: &str = r#"{"data": [
      {"path": "p", "children": {"Building": "b"}, "attributes": {"bsi::ifc::class": {"code": "IfcProject"}}},
      {"path": "b", "children": {"Ground": "s"}, "attributes": {"bsi::ifc::class": {"code": "IfcBuilding"}}},
      {"path": "s", "children": {"Wall": "w"}, "attributes": {"bsi::ifc::class": {"code": "IfcBuildingStorey"}}},
      {"path": "w", "children": {"Body": "w-body", "Opening": "o"}, "inherits": {"type": "wt", "material": "brick"},
       "attributes": {"bsi::ifc::class": {"code": "IfcWall"}, "bsi::ifc::prop::Name": "Wall"}},
      {"path": "w-body", "attributes": {"usd::usdgeom::mesh": {"points": [[0,0,0]], "faceVertexIndices": []}}},
      {"path": "o", "children": {"Door": "d"}, "attributes": {"bsi::ifc::class": {"code": "IfcOpeningElement"}}},
      {"path": "d", "attributes": {"bsi::ifc::class": {"code": "IfcDoor"}}},
      {"path": "wt", "attributes": {"bsi::ifc::class": {"code": "IfcWallType"}}},
      {"path": "brick", "attributes": {"bsi::ifc::material": {"code": "BRICK"}}},
      {"path": "w", "children": {"Sign": "x"}, "attributes": {"bsi::ifc::prop::Name": null}}
    ]}"#;

    #[test]
    fn the_entries_of_a_path_make_one_node() {
        let s = Structure::read("house.ifcx", HOUSE.as_bytes()).unwrap();
        assert_eq!((s.entries.len(), s.nodes.len()), (10, 9));
        let wall = &s.nodes[s.index["w"] as usize];
        assert_eq!(wall.entries, [3, 9]);
        assert_eq!(wall.name, None, "the later entry takes the name out");
        assert_eq!((wall.class.as_deref(), wall.label()), (Some("IfcWall"), "IfcWall"));
        assert_eq!(wall.children.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(), ["Body", "Opening", "Sign"]);
        assert_eq!(wall.attributes, ["bsi::ifc::class"]);
        // the project is the only root: the type and the material are inherited
        assert_eq!(s.roots, [s.index["p"]]);
    }

    #[test]
    fn the_links_are_named_after_the_ifc_relations() {
        let s = Structure::read("house.ifcx", HOUSE.as_bytes()).unwrap();
        let kind = |from: &str, name: &str| s.links.iter().find(|l| s.nodes[l.from as usize].path == from && l.name == name).map(|l| l.kind);
        assert_eq!(kind("p", "Building"), Some(Relation::Aggregates));
        assert_eq!(kind("b", "Ground"), Some(Relation::Aggregates));
        assert_eq!(kind("s", "Wall"), Some(Relation::Contains));
        assert_eq!(kind("w", "Opening"), Some(Relation::Voids));
        assert_eq!(kind("o", "Door"), Some(Relation::Fills));
        assert_eq!(kind("w", "Body"), Some(Relation::Part));
        assert_eq!(kind("w", "type"), Some(Relation::Type));
        assert_eq!(kind("w", "material"), Some(Relation::Material));
        // a child the file does not have is still a link, to no node
        let sign = s.links.iter().find(|l| l.name == "Sign").unwrap();
        assert_eq!((sign.target.as_str(), sign.to), ("x", None));
    }

    #[test]
    fn the_tree_opens_where_it_is_asked_and_finds_a_node() {
        let s = Structure::read("house.ifcx", HOUSE.as_bytes()).unwrap();
        let rows = s.tree_rows(&HashSet::new());
        assert_eq!(rows.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(), ["p"]);
        assert!(rows[0].kids && !rows[0].open);
        // open down to the wall: where it first hangs
        let door = s.index["d"];
        let open: HashSet<String> = s.keys_above(door).into_iter().collect();
        let rows = s.tree_rows(&open);
        let labels: Vec<(u16, &str, Via)> = rows.iter().map(|r| (r.depth, r.label.as_str(), r.via)).collect();
        assert_eq!(
            labels,
            [
                (0, "p", Via::Root),
                (1, "Building", Via::Child),
                (2, "Ground", Via::Child),
                (3, "Wall", Via::Child),
                (4, "Body", Via::Child),
                (4, "Opening", Via::Child),
                (5, "Door", Via::Child),
                (4, "Sign", Via::Child),
                (4, "material", Via::Inherits),
                (4, "type", Via::Inherits),
            ]
        );
        assert_eq!(rows[6].node, Some(door));
        assert_eq!(rows[7].node, None);
        let above: Vec<&str> = s.nodes_above(door).iter().map(|&k| s.nodes[k as usize].path.as_str()).collect();
        assert_eq!(above, ["p", "b", "s", "w", "o"]);
        // what hangs under the storey, along children
        let below: Vec<&str> = s.below(s.index["s"], 100).iter().map(|&k| s.nodes[k as usize].path.as_str()).collect();
        assert_eq!(below, ["s", "w", "w-body", "o", "d"]);
    }

    #[test]
    fn a_node_that_holds_itself_is_not_opened_again() {
        let file = r#"{"data": [{"path": "r", "children": {"A": "a"}}, {"path": "a", "children": {"Back": "a"}}]}"#;
        let s = Structure::read("loop.ifcx", file.as_bytes()).unwrap();
        let open: HashSet<String> = ["r".to_string(), format!("r{KEY_SEP}A"), format!("r{KEY_SEP}A{KEY_SEP}Back")].into();
        let rows = s.tree_rows(&open);
        assert_eq!(rows.len(), 3);
        assert!(rows[2].cycle && !rows[2].open);
    }
}
