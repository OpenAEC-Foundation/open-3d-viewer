//! The SketchUp 2021+ container: a header, then a ZIP archive with `model.dat` (a tree of
//! tag-length-value records: 2-byte tag, u32 size, payload) and a `material.xml` per material.

use super::{
    f64_at, find_bytes, u16_at, u32_at, Definition, Face, Instance, Material, Parsed, Tag, DEFAULT_TAG,
    DEFAULT_TAG_COLOR, IDENTITY,
};
use std::collections::HashMap;
use std::io::Read;

/// Records whose payload is itself a list of records.
const CONTAINERS: &[u16] = &[
    0xF401, 0xF701, 0xD430, 0xD530, 0xC832, 0x7C15, 0x8813, 0x8913, 0x8A13, 0x8B13, 0x8C13, 0x8D13, 0x4C1D,
    0x6419, 0xF901, 0x7017, 0x7117, 0xD007, 0xC409, 0x9411, 0x9511, 0x0F01, 0x384A, 0xB80B, 0x9713, 0x2C4C,
    0xAC0D, 0xAE0D, 0xF601, 0xF801, 0x983A, 0x993A, 0x8C3C, 0x8D3C,
    // An image placed in the model wraps its instance in 9013 -> 401F
    0x9013, 0x401F,
];

/// The containers inside an instance's attribute block (DC05).
const ATTRIBUTE_CONTAINERS: &[u16] = &[0xDD05, 0xB536, 0xB136, 0xB236, 0xB336, 0xB036, 0xA438];

struct Node<'a> {
    tag: u16,
    children: Vec<Node<'a>>,
    /// Empty for a container with children.
    payload: &'a [u8],
}

fn parse_records<'a>(data: &'a [u8], start: usize, end: usize, containers: &[u16]) -> Vec<Node<'a>> {
    let mut out = Vec::new();
    let mut pos = start;
    while pos + 6 <= end {
        let tag = u16::from_be_bytes([data[pos], data[pos + 1]]);
        let body_end = pos + 6 + u32_at(data, pos + 2) as usize;
        if body_end > end {
            break;
        }
        let children = if body_end > pos + 6 && containers.contains(&tag) {
            parse_records(data, pos + 6, body_end, containers)
        } else {
            Vec::new()
        };
        let payload = if children.is_empty() { &data[pos + 6..body_end] } else { &[][..] };
        out.push(Node { tag, children, payload });
        pos = body_end;
    }
    out
}

/// (tag, start, end) of the records in `data[start..end]`, without their children.
fn record_spans(data: &[u8], start: usize, end: usize) -> Vec<(u16, usize, usize)> {
    let mut out = Vec::new();
    let mut pos = start;
    while pos + 6 <= end {
        let body_end = pos + 6 + u32_at(data, pos + 2) as usize;
        if body_end > end {
            break;
        }
        out.push((u16::from_be_bytes([data[pos], data[pos + 1]]), pos, body_end));
        pos = body_end;
    }
    out
}

/// Little-endian unsigned integer of any width up to 8 bytes.
fn var_int(bytes: &[u8]) -> u64 {
    bytes.iter().take(8).enumerate().fold(0, |v, (i, &b)| v | (b as u64) << (8 * i))
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace('\0', "").trim().to_string()
}

/// First node with `tag`, depth first.
fn find<'n, 'a>(nodes: &'n [Node<'a>], tag: u16) -> Option<&'n Node<'a>> {
    nodes.iter().find_map(|n| if n.tag == tag { Some(n) } else { find(&n.children, tag) })
}

fn find_all<'n, 'a>(nodes: &'n [Node<'a>], tag: u16, out: &mut Vec<&'n Node<'a>>) {
    for n in nodes {
        if n.tag == tag {
            out.push(n);
        }
        find_all(&n.children, tag, out);
    }
}

fn child<'n, 'a>(node: &'n Node<'a>, tag: u16) -> Option<&'n Node<'a>> {
    node.children.iter().find(|c| c.tag == tag)
}

/// The id in a DC05 block: a DE05 record inside, or the whole payload.
fn dc05_id(p: &[u8]) -> u64 {
    if p.len() >= 6 && p[0] == 0xDE && p[1] == 0x05 {
        let n = u32_at(p, 2) as usize;
        var_int(&p[6..(6 + n).min(p.len())])
    } else {
        var_int(p)
    }
}

/// The entity id of a record: its first DE05 or DC05, searched breadth first per level.
fn entity_id(node: &Node) -> Option<u64> {
    for c in &node.children {
        match c.tag {
            0xDE05 => return Some(var_int(c.payload)),
            0xDC05 => return Some(dc05_id(c.payload)),
            _ => {}
        }
    }
    node.children.iter().find_map(entity_id)
}

/// Vertices, edges, faces and instances in `nodes` (not descending into them).
fn extract_geometry(nodes: &[Node], def: &mut Definition) {
    for el in nodes {
        match el.tag {
            0xC409 => {
                if let (Some(id), Some(c)) = (entity_id(el), find(&el.children, 0xC509)) {
                    if c.payload.len() >= 24 {
                        def.vertices.insert(id, [0, 8, 16].map(|o| f64_at(c.payload, o)));
                    }
                }
            }
            0xB80B => {
                if let Some(id) = entity_id(el) {
                    let v1 = find(&el.children, 0xB90B).map(|n| var_int(n.payload));
                    let v2 = find(&el.children, 0xBA0B).map(|n| var_int(n.payload));
                    def.edges.insert(id, (v1, v2));
                }
            }
            0xAC0D => {
                if let Some(id) = entity_id(el) {
                    def.set_face(id, read_face(el));
                }
            }
            0x6419 => def.instances.push(read_instance(el)),
            _ if !el.children.is_empty() => extract_geometry(&el.children, def),
            _ => {}
        }
    }
}

fn read_face(el: &Node) -> Face {
    let normal = match find(&el.children, 0xAD0D) {
        Some(n) if n.payload.len() >= 24 => [0, 8, 16].map(|o| f64_at(n.payload, o)),
        _ => [0.0, 0.0, 1.0],
    };
    let mut loops = Vec::new();
    if let Some(ae0d) = find(&el.children, 0xAE0D) {
        let mut loop_nodes = Vec::new();
        find_all(&ae0d.children, 0x9411, &mut loop_nodes);
        for ln in loop_nodes {
            let mut uses_nodes = Vec::new();
            find_all(&ln.children, 0xA00F, &mut uses_nodes);
            let uses: Vec<(u64, bool)> = uses_nodes
                .into_iter()
                .filter_map(|cn| {
                    // A10F: edge id, A20F: 0 when the edge runs along the loop
                    let p = cn.payload;
                    let (mut edge, mut orient) = (None, None);
                    let mut pos = 0;
                    while pos + 6 < p.len() {
                        let size = u32_at(p, pos + 2) as usize;
                        if pos + 6 + size <= p.len() {
                            let value = var_int(&p[pos + 6..pos + 6 + size]);
                            match (p[pos], p[pos + 1]) {
                                (0xA1, 0x0F) => edge = Some(value),
                                (0xA2, 0x0F) => orient = Some(value),
                                _ => {}
                            }
                        }
                        pos += 6 + size;
                    }
                    Some((edge?, orient? != 0))
                })
                .collect();
            if !uses.is_empty() {
                loops.push(uses);
            }
        }
    }
    let material = child(el, 0xD007).and_then(|d| child(d, 0xD107)).map(|n| var_int(n.payload));
    let back_material = child(el, 0xAF0D).filter(|n| !n.payload.is_empty()).map(|n| var_int(n.payload));
    Face { loops, normal, material, back_material }
}

fn read_instance(el: &Node) -> Instance {
    let nodes = if el.children.is_empty() { std::slice::from_ref(el) } else { &el.children[..] };
    let mut matrix = IDENTITY;
    if let Some(n) = find(nodes, 0x6619).filter(|n| n.payload.len() >= 104) {
        for (i, v) in matrix.iter_mut().enumerate() {
            *v = f64_at(n.payload, 8 * i);
        }
    }
    let d007 = child(el, 0xD007);
    let field = |tag| d007.and_then(|d| child(d, tag)).filter(|n| !n.payload.is_empty());
    Instance {
        name: find(nodes, 0x6519).map(|n| text(n.payload)).unwrap_or_default(),
        definition: find(nodes, 0x6719).map(|n| var_int(n.payload)),
        matrix,
        material: d007.and_then(|d| child(d, 0xD107)).map(|n| var_int(n.payload)),
        tag: field(0xD207).map(|n| var_int(n.payload)),
        hidden: field(0xD307).is_some_and(|n| n.payload[0] & 1 != 0),
        props: d007.map(dynamic_props).unwrap_or_default(),
    }
}

/// Dynamic component attributes: B636 key, AD38 value.
fn dynamic_props(d007: &Node) -> Vec<(String, String)> {
    let Some(dc05) = child(d007, 0xDC05) else { return Vec::new() };
    let nodes = parse_records(dc05.payload, 0, dc05.payload.len(), ATTRIBUTE_CONTAINERS);
    fn walk(nodes: &[Node], key: &mut Option<String>, out: &mut Vec<(String, String)>) {
        for n in nodes {
            match n.tag {
                0xB636 => *key = Some(text(n.payload)),
                0xAD38 if key.as_ref().is_some_and(|k| !k.is_empty()) => {
                    out.push((key.take().unwrap_or_default(), text(n.payload)));
                }
                _ => {}
            }
            walk(&n.children, key, out);
        }
    }
    let mut out = Vec::new();
    walk(&nodes, &mut None, &mut out);
    out
}

fn collect_tags(nodes: &[Node], names: &mut HashMap<u64, String>, hidden: &mut HashMap<String, bool>) {
    for el in nodes {
        if el.tag == 0x993A {
            for c in el.children.iter().filter(|c| c.tag == 0x8C3C) {
                if let (Some(dc05), Some(name)) = (find(&c.children, 0xDC05), find(&c.children, 0x8D3C)) {
                    let name = text(name.payload);
                    if let Some(h) = find(&c.children, 0x8E3C).filter(|h| !h.payload.is_empty()) {
                        hidden.insert(name.clone(), h.payload[0] == 1);
                    }
                    names.insert(dc05_id(dc05.payload), name);
                }
            }
        }
        collect_tags(&el.children, names, hidden);
    }
}

fn collect_material_ids(nodes: &[Node], out: &mut HashMap<u64, String>) {
    for el in nodes {
        if el.tag == 0xC832 {
            if let (Some(dc05), Some(name)) = (find(&el.children, 0xDC05), find(&el.children, 0xCC32)) {
                out.insert(dc05_id(dc05.payload), String::from_utf8_lossy(name.payload).into_owned());
            }
        }
        collect_material_ids(&el.children, out);
    }
}

fn collect_definitions(nodes: &[Node], out: &mut HashMap<u64, Definition>) {
    for el in nodes {
        if el.tag == 0x7C15 {
            if let Some(id) = entity_id(el) {
                let mut def = Definition {
                    name: el.children.iter().find(|c| c.tag == 0x7E15).map(|c| text(c.payload)).unwrap_or_default(),
                    ..Definition::default()
                };
                extract_geometry(&el.children, &mut def);
                out.insert(id, def);
            }
        }
        collect_definitions(&el.children, out);
    }
}

pub(super) fn parse(data: &[u8]) -> Result<Parsed, String> {
    let zip_at = find_bytes(data, b"PK\x03\x04", 0, data.len()).ok_or("No embedded ZIP archive in the SketchUp file")?;
    let entries = unzip(&data[zip_at..], |name| {
        let lower = name.to_lowercase();
        lower == "model.dat" || lower.ends_with("/model.dat") || (lower.starts_with("materials/") && lower.ends_with("material.xml"))
    })?;
    let mut model = None;
    let mut by_name: HashMap<String, Material> = HashMap::new();
    let mut by_folder: HashMap<String, Material> = HashMap::new();
    let mut tags: HashMap<String, Tag> = HashMap::new();
    for (name, bytes) in entries {
        if name.to_lowercase().ends_with("model.dat") {
            model = Some(bytes);
            continue;
        }
        let Some((mat_name, material)) = parse_material_xml(&String::from_utf8_lossy(&bytes)) else { continue };
        if let Some(folder) = name.split('/').nth(1).filter(|f| !f.is_empty()) {
            by_folder.insert(folder.to_string(), material);
        }
        // Tag colours are stored as materials named "Layer_<tag>"
        if let Some(tag) = mat_name.strip_prefix("Layer_") {
            let [r, g, b, _] = material.rgba;
            tags.insert(tag.to_string(), Tag { color: [r, g, b], hidden: false });
        }
        by_name.insert(mat_name, material);
    }
    let model = model.ok_or("The SketchUp archive has no model.dat")?;

    let mut parsed = Parsed::default();
    let mut material_names = HashMap::new();
    let mut hidden = HashMap::new();
    let mut spans = record_spans(&model, 0, model.len());
    if let [(0xF401, start, end)] = spans[..] {
        spans = record_spans(&model, start + 6, end);
    }
    for (_, start, end) in spans {
        let nodes = parse_records(&model, start, end, CONTAINERS);
        let Some(el) = nodes.first() else { continue };
        let one = std::slice::from_ref(el);
        collect_tags(one, &mut parsed.tag_names, &mut hidden);
        collect_material_ids(one, &mut material_names);
        collect_definitions(one, &mut parsed.definitions);
        if el.tag == 0xF601 {
            extract_geometry(&el.children, &mut parsed.root);
        }
    }
    parsed.tag_names.entry(1).or_insert_with(|| DEFAULT_TAG.to_string());
    tags.entry(DEFAULT_TAG.to_string()).or_insert(Tag { color: DEFAULT_TAG_COLOR, hidden: false });
    for (name, h) in hidden {
        if let Some(t) = tags.get_mut(&name) {
            t.hidden = h;
        } else {
            tags.insert(name, Tag { color: DEFAULT_TAG_COLOR, hidden: h });
        }
    }
    parsed.tags = tags;
    parsed.materials = material_names
        .into_iter()
        .filter_map(|(id, name)| by_name.get(&name).or_else(|| by_folder.get(&name)).map(|m| (id, *m)))
        .collect();
    Ok(parsed)
}

/// Name, colour and opacity from a material.xml.
fn parse_material_xml(xml: &str) -> Option<(String, Material)> {
    let attrs = element_attributes(xml, "material")?;
    let int = |name: &str| attribute(attrs, name).and_then(|v| v.trim().parse::<i64>().ok()).unwrap_or(128).clamp(0, 255) as u8;
    let opacity = if attribute(attrs, "useTrans").as_deref() == Some("1") {
        let trans = attribute(attrs, "trans").and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(0.0);
        (1.0 - trans).clamp(0.0, 1.0)
    } else {
        1.0
    };
    let name = attribute(attrs, "name").filter(|n| !n.is_empty()).unwrap_or_else(|| "unknown".into());
    Some((name, Material { rgba: [int("colorRed"), int("colorGreen"), int("colorBlue"), 255], opacity }))
}

/// The attribute text of the first `<name ...>` (or `<prefix:name ...>`) element.
fn element_attributes<'x>(xml: &'x str, name: &str) -> Option<&'x str> {
    let bytes = xml.as_bytes();
    let mut at = 0;
    while let Some(lt) = xml[at..].find('<').map(|i| at + i) {
        let tag_end = xml[lt + 1..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == ':'))
            .map(|i| lt + 1 + i)
            .unwrap_or(xml.len());
        let tag = &xml[lt + 1..tag_end];
        let local = tag.rsplit(':').next().unwrap_or(tag);
        if local.eq_ignore_ascii_case(name) && tag_end < bytes.len() {
            let close = xml[tag_end..].find('>').map(|i| tag_end + i)?;
            return Some(&xml[tag_end..close]);
        }
        at = lt + 1;
    }
    None
}

/// The value of attribute `name` (a whole word) in an element's attribute text.
fn attribute(attrs: &str, name: &str) -> Option<String> {
    let bytes = attrs.as_bytes();
    let mut from = 0;
    while let Some(i) = attrs[from..].find(name).map(|i| from + i) {
        from = i + name.len();
        let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        if (i > 0 && word(bytes[i - 1])) || bytes.get(from).is_some_and(|&b| word(b)) {
            continue;
        }
        let rest = attrs[from..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else { continue };
        let rest = rest.trim_start();
        let quote = rest.chars().next().filter(|&q| q == '"' || q == '\'')?;
        let end = rest[1..].find(quote)?;
        return Some(decode_entities(&rest[1..1 + end]));
    }
    None
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let decoded = rest.find(';').and_then(|semi| {
            let entity = &rest[1..semi];
            let c = match entity {
                "lt" => Some('<'),
                "gt" => Some('>'),
                "amp" => Some('&'),
                "apos" => Some('\''),
                "quot" => Some('"'),
                _ => entity
                    .strip_prefix("#x")
                    .or_else(|| entity.strip_prefix("#X"))
                    .map(|h| u32::from_str_radix(h, 16))
                    .or_else(|| entity.strip_prefix('#').map(|d| d.parse::<u32>()))
                    .and_then(|r| r.ok())
                    .and_then(char::from_u32),
            };
            c.map(|c| (c, semi))
        });
        match decoded {
            Some((c, semi)) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The entries of a ZIP archive whose name passes `wanted`, from its central directory.
fn unzip(zip: &[u8], wanted: impl Fn(&str) -> bool) -> Result<Vec<(String, Vec<u8>)>, String> {
    const EOCD: &[u8] = b"PK\x05\x06";
    let search_from = zip.len().saturating_sub(22 + 65_535);
    let eocd = (search_from..zip.len().saturating_sub(21))
        .rev()
        .find(|&i| &zip[i..i + 4] == EOCD)
        .ok_or("The SketchUp archive has no ZIP directory")?;
    let count = u16_at(zip, eocd + 10) as usize;
    let mut p = u32_at(zip, eocd + 16) as usize;
    let mut out = Vec::new();
    for _ in 0..count {
        if zip.get(p..p + 4) != Some(b"PK\x01\x02") || p + 46 > zip.len() {
            return Err("Damaged ZIP directory in the SketchUp file".into());
        }
        let method = u16_at(zip, p + 10);
        let compressed = u32_at(zip, p + 20) as usize;
        let size = u32_at(zip, p + 24) as usize;
        let name_len = u16_at(zip, p + 28) as usize;
        let skip = name_len + u16_at(zip, p + 30) as usize + u16_at(zip, p + 32) as usize;
        let local = u32_at(zip, p + 42) as usize;
        let name = String::from_utf8_lossy(zip.get(p + 46..p + 46 + name_len).unwrap_or_default()).into_owned();
        p += 46 + skip;
        if !wanted(&name) {
            continue;
        }
        if zip.get(local..local + 4) != Some(b"PK\x03\x04") {
            return Err(format!("Damaged ZIP entry {name} in the SketchUp file"));
        }
        let start = local + 30 + u16_at(zip, local + 26) as usize + u16_at(zip, local + 28) as usize;
        let raw = zip.get(start..start + compressed).ok_or_else(|| format!("Truncated ZIP entry {name}"))?;
        let bytes = match method {
            0 => raw.to_vec(),
            8 => {
                let mut buf = Vec::with_capacity(size.min(1 << 30));
                flate2::read::DeflateDecoder::new(raw)
                    .take(size as u64)
                    .read_to_end(&mut buf)
                    .map_err(|e| format!("Cannot decompress {name}: {e}"))?;
                buf
            }
            m => return Err(format!("Unsupported ZIP compression {m} for {name}")),
        };
        out.push((name, bytes));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn material_xml() {
        let xml = r#"<?xml version="1.0"?><Material name="Glass &amp; Co" type="0" colorRed="10" colorGreen="20" colorBlue="30" useTrans="1" trans="0.25"><Texture textureFilename="x.png"/></Material>"#;
        let (name, m) = parse_material_xml(xml).unwrap();
        assert_eq!(name, "Glass & Co");
        assert_eq!(m.rgba, [10, 20, 30, 255]);
        assert!((m.opacity - 0.75).abs() < 1e-9);
    }

    #[test]
    fn attribute_is_a_whole_word() {
        assert_eq!(attribute(r#" textureFilename="a" name='b'"#, "name").as_deref(), Some("b"));
    }
}
