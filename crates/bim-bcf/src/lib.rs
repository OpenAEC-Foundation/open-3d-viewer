//! Native BCF 2.1 interoperability.
pub mod issues;
use roxmltree::{Document, Node};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read, Write},
};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

pub(crate) const MAX_COMPRESSED: usize = 50 * 1024 * 1024;
pub(crate) const MAX_EXPANDED: usize = 100 * 1024 * 1024;
pub(crate) const MAX_ENTRIES: usize = 2000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Camera {
    pub position: [f64; 3],
    pub direction: [f64; 3],
    pub up: [f64; 3],
    pub field_of_view: Option<f64>,
    pub view_to_world_scale: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TopicSummary {
    pub id: String,
    pub title: String,
    pub status: String,
    pub description: String,
    pub component_guids: Vec<String>,
    pub camera: Option<Camera>,
    /// Counts aggregate all viewpoints; only the first camera is exposed above.
    pub viewpoint_count: usize,
    pub visibility_count: usize,
    pub coloring_count: usize,
    pub clipping_plane_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewTopic {
    pub id: String,
    pub title: String,
    pub status: String,
    pub description: String,
    pub component_guids: Vec<String>,
    pub creation_date: String,
    pub creation_author: String,
}

#[derive(Debug, Clone)]
pub struct BcfArchive {
    files: BTreeMap<String, Vec<u8>>,
    topics: Vec<TopicSummary>,
    markup_paths: BTreeMap<String, String>,
}

pub(crate) fn safe_path(path: &str) -> Result<(), String> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains(['\\', ':', '\0'])
        || path
            .trim_end_matches('/')
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(format!("Invalid BCF archive path: {path}"));
    }
    Ok(())
}
pub(crate) fn xml(bytes: &[u8]) -> Result<Document<'_>, String> {
    let source = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    let upper = source.to_ascii_uppercase();
    if upper.contains("<!DOCTYPE") || upper.contains("<!ENTITY") {
        return Err("XML document types and entities are not supported".into());
    }
    Document::parse(source).map_err(|e| format!("Invalid BCF XML: {e}"))
}
fn child_text(node: Node<'_, '_>, name: &str) -> String {
    node.children()
        .find(|n| n.has_tag_name(name))
        .and_then(|n| n.text())
        .unwrap_or("")
        .to_owned()
}
pub(crate) fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
        .replace('\r', "&#13;")
        .replace('\n', "&#10;")
        .replace('\t', "&#9;")
}
fn valid_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}
fn parse_camera(doc: &Document<'_>) -> Result<Option<Camera>, String> {
    let Some(node) = doc
        .descendants()
        .find(|n| n.has_tag_name("PerspectiveCamera") || n.has_tag_name("OrthogonalCamera"))
    else {
        return Ok(None);
    };
    let vector = |name: &str| -> Result<[f64; 3], String> {
        let parent = node
            .children()
            .find(|n| n.has_tag_name(name))
            .ok_or_else(|| format!("Missing camera {name}"))?;
        let mut result = [0.; 3];
        for (i, axis) in ["X", "Y", "Z"].iter().enumerate() {
            result[i] = child_text(parent, axis)
                .trim()
                .parse::<f64>()
                .map_err(|_| "Invalid camera coordinate")?;
            if !result[i].is_finite() {
                return Err("Nonfinite camera coordinate".into());
            }
        }
        Ok(result)
    };
    let perspective = node.has_tag_name("PerspectiveCamera");
    let scale = child_text(
        node,
        if perspective {
            "FieldOfView"
        } else {
            "ViewToWorldScale"
        },
    )
    .trim()
    .parse::<f64>()
    .map_err(|_| "Invalid camera scale")?;
    if !scale.is_finite() || scale <= 0. || (perspective && scale >= 180.) {
        return Err("Invalid camera scale".into());
    }
    let direction = vector("CameraDirection")?;
    let up = vector("CameraUpVector")?;
    let normalize = |v: [f64; 3]| -> Result<[f64; 3], String> {
        let largest = v.iter().copied().map(f64::abs).fold(0., f64::max);
        if largest == 0. {
            return Err("Zero camera direction or up vector".into());
        }
        let v = v.map(|x| x / largest);
        let length = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        Ok(v.map(|x| x / length))
    };
    let d = normalize(direction)?;
    let u = normalize(up)?;
    if (d.iter().zip(u).map(|(a, b)| a * b).sum::<f64>()).abs() > 1. - 1e-12 {
        return Err("Parallel camera direction and up vector".into());
    }
    Ok(Some(Camera {
        position: vector("CameraViewPoint")?,
        direction,
        up,
        field_of_view: perspective.then_some(scale),
        view_to_world_scale: (!perspective).then_some(scale),
    }))
}

impl BcfArchive {
    /// Read with both metadata and streamed decompression bounds. All files are retained.
    pub fn read(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_COMPRESSED {
            return Err("BCF exceeds 50 MB compressed limit".into());
        }
        let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
        if archive.len() > MAX_ENTRIES {
            return Err("BCF exceeds 2000 entries".into());
        }
        let mut files = BTreeMap::new();
        let mut paths = BTreeSet::new();
        let mut remaining = MAX_EXPANDED;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
            let name = entry.name().to_owned();
            safe_path(&name)?;
            if !paths.insert(name.clone()) {
                return Err("Duplicate archive entry".into());
            }
            if entry.is_dir() {
                continue;
            }
            if entry.size() > remaining as u64 {
                return Err("BCF exceeds 100 MB expanded limit".into());
            }
            let mut data = Vec::new();
            (&mut entry)
                .take(remaining as u64 + 1)
                .read_to_end(&mut data)
                .map_err(|e| e.to_string())?;
            if data.len() > remaining {
                return Err("BCF exceeds 100 MB expanded limit".into());
            }
            remaining -= data.len();
            files.insert(name, data);
        }
        Self::from_files(files)
    }

    fn from_files(files: BTreeMap<String, Vec<u8>>) -> Result<Self, String> {
        for (name, bytes) in &files {
            if [".xml", ".bcf", ".bcfv", ".bcfp", ".version"]
                .iter()
                .any(|ext| name.to_ascii_lowercase().ends_with(ext))
            {
                xml(bytes)?;
            }
        }
        let version = xml(files.get("bcf.version").ok_or("Missing bcf.version")?)?;
        if !version.root_element().has_tag_name("Version")
            || version.root_element().attribute("VersionId") != Some("2.1")
        {
            return Err("Only BCF 2.1 is supported".into());
        }
        let mut topics = Vec::new();
        let mut markup_paths = BTreeMap::new();
        let mut ids = BTreeSet::new();
        for (path, bytes) in &files {
            let Some((folder, filename)) = path.split_once('/') else {
                continue;
            };
            if filename != "markup.bcf" {
                continue;
            }
            let doc = xml(bytes)?;
            if !doc.root_element().has_tag_name("Markup") {
                return Err("Invalid markup root".into());
            }
            let topic_nodes: Vec<_> = doc
                .root_element()
                .children()
                .filter(|n| n.has_tag_name("Topic"))
                .collect();
            if topic_nodes.len() != 1 {
                return Err("Expected exactly one topic per markup".into());
            }
            let node = topic_nodes[0];
            let id = node.attribute("Guid").unwrap_or("");
            if !valid_id(id) || !ids.insert(id.to_ascii_lowercase()) {
                return Err("Invalid or duplicate topic ID".into());
            }
            let title = child_text(node, "Title");
            if title.trim().is_empty() {
                return Err("Missing topic title".into());
            }
            let mut local_ids = BTreeSet::new();
            for element in doc
                .root_element()
                .children()
                .filter(|n| n.has_tag_name("Comment") || n.has_tag_name("Viewpoints"))
            {
                if let Some(guid) = element.attribute("Guid") {
                    if !valid_id(guid) || !local_ids.insert(guid.to_ascii_lowercase()) {
                        return Err("Invalid or duplicate markup ID".into());
                    }
                } else {
                    return Err("Missing markup GUID".into());
                }
            }
            let mut components = BTreeSet::new();
            let mut camera = None;
            let mut viewpoint_count = 0;
            let mut visibility_count = 0;
            let mut coloring_count = 0;
            let mut clipping_plane_count = 0;
            for (view_index, reference) in doc
                .descendants()
                .filter(|n| {
                    (n.has_tag_name("Viewpoints") && n.attribute("Guid").is_some())
                        || n.has_tag_name("ViewPoint")
                })
                .enumerate()
            {
                viewpoint_count += 1;
                let asset = child_text(reference, "Viewpoint");
                if !asset.is_empty() {
                    safe_path(&asset)?;
                    let view = xml(files
                        .get(&format!("{folder}/{asset}"))
                        .ok_or("Missing viewpoint asset")?)?;
                    if !view.root_element().has_tag_name("VisualizationInfo") {
                        return Err("Invalid viewpoint root".into());
                    }
                    if !view.root_element().attribute("Guid").is_some_and(valid_id) {
                        return Err("Invalid viewpoint GUID".into());
                    }
                    visibility_count += view
                        .descendants()
                        .filter(|n| n.has_tag_name("Visibility"))
                        .count();
                    coloring_count += view
                        .descendants()
                        .filter(|n| n.has_tag_name("Color"))
                        .count();
                    clipping_plane_count += view
                        .descendants()
                        .filter(|n| n.has_tag_name("ClippingPlane"))
                        .count();
                    if let (Some(a), Some(b)) = (
                        reference.attribute("Guid"),
                        view.root_element().attribute("Guid"),
                    ) {
                        if !a.eq_ignore_ascii_case(b) {
                            return Err("Viewpoint ID does not match reference".into());
                        }
                    }
                    let parsed = parse_camera(&view)?;
                    if view_index == 0 {
                        camera = parsed;
                    }
                    for component in view.descendants().filter(|n| {
                        n.has_tag_name("Component")
                            && n.parent().is_some_and(|p| p.has_tag_name("Selection"))
                    }) {
                        if let Some(guid) = component.attribute("IfcGuid").filter(|g| !g.is_empty())
                        {
                            components.insert(guid.to_owned());
                        }
                    }
                }
                let snapshot = child_text(reference, "Snapshot");
                if !snapshot.is_empty() {
                    safe_path(&snapshot)?;
                    if !files.contains_key(&format!("{folder}/{snapshot}")) {
                        return Err("Missing snapshot asset".into());
                    }
                }
            }
            markup_paths.insert(id.to_owned(), path.clone());
            topics.push(TopicSummary {
                id: id.into(),
                title,
                status: node.attribute("TopicStatus").unwrap_or("Open").into(),
                description: child_text(node, "Description"),
                component_guids: components.into_iter().collect(),
                camera,
                viewpoint_count,
                visibility_count,
                coloring_count,
                clipping_plane_count,
            });
        }
        if topics.is_empty() {
            return Err("BCF contains no topics".into());
        }
        Ok(Self {
            files,
            topics,
            markup_paths,
        })
    }

    pub fn topics(&self) -> Vec<TopicSummary> {
        self.topics.clone()
    }

    /// Change only the status attribute; retain comments, namespaces and original bytes elsewhere.
    pub fn set_status(&mut self, id: &str, status: &str) -> Result<(), String> {
        let path = self.markup_paths.get(id).ok_or("Unknown topic ID")?;
        let bytes = self.files.get(path).ok_or("Missing markup")?;
        let mut source = std::str::from_utf8(bytes)
            .map_err(|e| e.to_string())?
            .to_owned();
        let doc = xml(bytes)?;
        let topic = doc
            .root_element()
            .children()
            .find(|n| n.has_tag_name("Topic"))
            .ok_or("Missing topic")?;
        if let Some(attr) = topic.attributes().find(|a| a.name() == "TopicStatus") {
            // Compute from the full source range: parser value offsets have a
            // compact whitespace counter and cannot represent very long padding.
            let range = attr.range();
            let equals = source[range.clone()]
                .find('=')
                .ok_or("Invalid status attribute")?
                + range.start;
            let quote = source[equals + 1..range.end]
                .find(['\'', '"'])
                .ok_or("Invalid status quote")?
                + equals
                + 1;
            source.replace_range(quote + 1..range.end - 1, &escape(status));
        } else {
            let offset = topic.range().start
                + source[topic.range().start..]
                    .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
                    .ok_or("Invalid topic")?;
            source.insert_str(offset, &format!(" TopicStatus=\"{}\"", escape(status)));
        }
        xml(source.as_bytes())?;
        self.files.insert(path.clone(), source.into_bytes());
        self.topics
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or("Unknown topic")?
            .status = status.into();
        Ok(())
    }

    pub fn write(&self) -> Result<Vec<u8>, String> {
        if self.files.values().map(Vec::len).sum::<usize>() > MAX_EXPANDED {
            return Err("BCF exceeds 100 MB expanded limit".into());
        }
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in &self.files {
            writer
                .start_file(name, SimpleFileOptions::default())
                .map_err(|e| e.to_string())?;
            writer.write_all(bytes).map_err(|e| e.to_string())?;
        }
        let result = writer.finish().map_err(|e| e.to_string())?.into_inner();
        if result.len() > MAX_COMPRESSED {
            return Err("BCF exceeds 50 MB compressed limit".into());
        }
        Ok(result)
    }

    pub fn from_topics(topics: &[NewTopic]) -> Result<Self, String> {
        let mut files = BTreeMap::new();
        files.insert(
            "bcf.version".into(),
            br#"<?xml version="1.0" encoding="UTF-8"?><Version VersionId="2.1"/>"#.to_vec(),
        );
        let mut ids = BTreeSet::new();
        for topic in topics {
            if !valid_id(&topic.id) || !ids.insert(topic.id.to_ascii_lowercase()) {
                return Err("Invalid or duplicate topic ID".into());
            }
            let id = &topic.id;
            chrono::DateTime::parse_from_rfc3339(&topic.creation_date)
                .map_err(|_| "Creation date must be RFC3339")?;
            if topic.component_guids.iter().any(|id| {
                id.len() != 22
                    || !id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$')
            }) {
                return Err("Invalid IFC component GlobalId".into());
            }
            let viewpoint_id = uuid::Uuid::new_v5(
                &uuid::Uuid::NAMESPACE_OID,
                format!("{id}/viewpoint").as_bytes(),
            );
            let markup = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><Markup><Topic Guid=\"{id}\" TopicType=\"Issue\" TopicStatus=\"{}\"><Title>{}</Title><CreationDate>{}</CreationDate><CreationAuthor>{}</CreationAuthor><Description>{}</Description></Topic><Viewpoints Guid=\"{viewpoint_id}\"><Viewpoint>viewpoint.bcfv</Viewpoint></Viewpoints></Markup>", escape(&topic.status), escape(&topic.title), escape(&topic.creation_date), escape(&topic.creation_author), escape(&topic.description));
            let components: String = topic
                .component_guids
                .iter()
                .map(|id| format!("<Component IfcGuid=\"{}\"/>", escape(id)))
                .collect();
            let selection = if components.is_empty() {
                String::new()
            } else {
                format!("<Selection>{components}</Selection>")
            };
            let view = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><VisualizationInfo Guid=\"{viewpoint_id}\"><Components>{selection}<Visibility DefaultVisibility=\"true\"/></Components></VisualizationInfo>");
            files.insert(format!("{id}/markup.bcf"), markup.into_bytes());
            files.insert(format!("{id}/viewpoint.bcfv"), view.into_bytes());
        }
        if files.len() > MAX_ENTRIES || files.values().map(Vec::len).sum::<usize>() > MAX_EXPANDED {
            return Err("Generated BCF exceeds archive limits".into());
        }
        Self::from_files(files)
    }
}
