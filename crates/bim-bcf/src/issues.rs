//! The issues of a BCF archive as values: topics with their comments and viewpoints, read from
//! BCF 2.0, 2.1 and 3.0 and written as BCF 2.1.
//!
//! A topic that is not changed is written back as it was read, byte for byte; a changed or new
//! one is written from its values. Files of the archive that belong to no topic (the project
//! file, extensions, documents) are kept as they are.

use crate::{escape, safe_path, xml, MAX_COMPRESSED, MAX_ENTRIES, MAX_EXPANDED};
use roxmltree::Node;
use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

/// The issues of a project.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Issues {
    /// The version the archive was read as (`2.0`, `2.1`, `3.0`); written as 2.1.
    pub version: String,
    /// The project's id and name, when the archive names one.
    pub project: Option<(String, String)>,
    pub topics: Vec<Topic>,
    /// The files that belong to no topic, by their path in the archive.
    other: BTreeMap<String, Vec<u8>>,
}

/// An issue: what is asked, by whom, of whom, and how it stands.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Topic {
    pub guid: String,
    pub title: String,
    pub kind: String,
    pub status: String,
    pub priority: String,
    pub labels: Vec<String>,
    pub description: String,
    pub creation_date: String,
    pub creation_author: String,
    pub modified_date: String,
    pub modified_author: String,
    pub due_date: String,
    pub assigned_to: String,
    pub stage: String,
    pub index: Option<i64>,
    pub comments: Vec<Comment>,
    pub viewpoints: Vec<Viewpoint>,
    /// The files of its folder as they were read; none once it is changed.
    original: Option<BTreeMap<String, Vec<u8>>>,
    /// The files of its folder that are no markup, viewpoint or snapshot (attachments).
    attachments: BTreeMap<String, Vec<u8>>,
}

/// A remark on a topic.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Comment {
    pub guid: String,
    pub date: String,
    pub author: String,
    pub text: String,
    /// The viewpoint it was made at.
    pub viewpoint: Option<String>,
    pub modified_date: String,
    pub modified_author: String,
}

/// How a camera looks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Lens {
    /// The angle it sees over its height (degrees).
    Perspective { field_of_view: f64 },
    /// The height it sees (model units).
    Orthogonal { view_to_world_scale: f64 },
}

/// Where a viewpoint looks from (the model's coordinates, Z up).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewCamera {
    pub position: [f64; 3],
    pub direction: [f64; 3],
    pub up: [f64; 3],
    pub lens: Lens,
}

/// A plane that cuts the model: what lies on the side its direction points to is left out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClippingPlane {
    pub location: [f64; 3],
    pub direction: [f64; 3],
}

/// A view of the model that goes with a topic: the camera, what is selected, shown and coloured,
/// where the model is cut, and a picture of it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Viewpoint {
    pub guid: String,
    pub camera: Option<ViewCamera>,
    /// The IFC GlobalIds of the selected elements.
    pub selection: Vec<String>,
    /// Whether elements show unless they are an exception.
    pub default_visibility: bool,
    /// The elements that show when the others do not, or do not when the others do.
    pub exceptions: Vec<String>,
    pub spaces_visible: bool,
    pub space_boundaries_visible: bool,
    pub openings_visible: bool,
    /// Colours (`AARRGGBB` or `RRGGBB`, hexadecimal) with the elements that have them.
    pub coloring: Vec<(String, Vec<String>)>,
    pub clipping_planes: Vec<ClippingPlane>,
    /// The picture: its file name in the topic's folder and its bytes (PNG or JPEG).
    pub snapshot: Option<(String, Vec<u8>)>,
}

/// A new identifier, as BCF writes them.
pub fn new_guid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// This moment, as BCF writes its dates.
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

impl Topic {
    /// A new, open topic by `author`.
    pub fn new(title: &str, author: &str) -> Topic {
        Topic { guid: new_guid(), title: title.to_string(), kind: "Issue".into(), status: "Open".into(), creation_date: now(), creation_author: author.to_string(), ..Topic::default() }
    }

    /// Marks it changed by `author`: it is written from its values from now on.
    pub fn touch(&mut self, author: &str) {
        self.original = None;
        self.modified_date = now();
        self.modified_author = author.to_string();
    }

    /// A new comment by `author`, at one of its viewpoints or at none.
    pub fn comment(&mut self, text: &str, author: &str, viewpoint: Option<String>) {
        self.comments.push(Comment { guid: new_guid(), date: now(), author: author.to_string(), text: text.to_string(), viewpoint, ..Comment::default() });
        self.touch(author);
    }

    /// Every element one of its viewpoints selects, each once, in order.
    pub fn elements(&self) -> Vec<&str> {
        let mut seen = std::collections::BTreeSet::new();
        self.viewpoints.iter().flat_map(|v| &v.selection).map(String::as_str).filter(|g| seen.insert(*g)).collect()
    }
}

impl Viewpoint {
    /// A new viewpoint that shows everything.
    pub fn new() -> Viewpoint {
        Viewpoint { guid: new_guid(), default_visibility: true, openings_visible: false, ..Viewpoint::default() }
    }
}

fn text(node: Node<'_, '_>, name: &str) -> String {
    node.children().find(|n| n.has_tag_name(name)).and_then(|n| n.text()).unwrap_or("").trim().to_owned()
}

fn attribute(node: Node<'_, '_>, name: &str) -> String {
    node.attribute(name).unwrap_or("").trim().to_owned()
}

fn flag(node: Node<'_, '_>, name: &str, or: bool) -> bool {
    match node.attribute(name).map(|v| v.trim().to_ascii_lowercase()) {
        Some(v) if v == "true" || v == "1" => true,
        Some(v) if v == "false" || v == "0" => false,
        _ => or,
    }
}

fn vector(node: Node<'_, '_>, name: &str) -> Option<[f64; 3]> {
    let parent = node.children().find(|n| n.has_tag_name(name))?;
    let mut out = [0.0; 3];
    for (k, axis) in ["X", "Y", "Z"].iter().enumerate() {
        out[k] = text(parent, axis).parse::<f64>().ok().filter(|v| v.is_finite())?;
    }
    Some(out)
}

fn guids(node: Node<'_, '_>) -> Vec<String> {
    node.descendants().filter(|n| n.has_tag_name("Component")).map(|n| attribute(n, "IfcGuid")).filter(|g| !g.is_empty()).collect()
}

/// A viewpoint file as a viewpoint, without its picture.
fn read_viewpoint(bytes: &[u8], guid: &str) -> Result<Viewpoint, String> {
    let doc = xml(bytes)?;
    let root = doc.root_element();
    let mut view = Viewpoint { guid: Some(attribute(root, "Guid")).filter(|g| !g.is_empty()).unwrap_or_else(|| guid.to_string()), default_visibility: true, ..Viewpoint::default() };
    if let Some(components) = root.children().find(|n| n.has_tag_name("Components")) {
        let child = |name: &str| components.children().find(|n| n.has_tag_name(name));
        match (child("Selection"), child("Visibility")) {
            (None, None) => {
                // BCF 2.0: every component says for itself whether it is selected and shown
                let all: Vec<Node> = components.children().filter(|n| n.has_tag_name("Component")).collect();
                view.selection = all.iter().filter(|n| flag(**n, "Selected", false)).map(|n| attribute(*n, "IfcGuid")).filter(|g| !g.is_empty()).collect();
                let shown: Vec<String> = all.iter().filter(|n| flag(**n, "Visible", true)).map(|n| attribute(*n, "IfcGuid")).filter(|g| !g.is_empty()).collect();
                let hidden: Vec<String> = all.iter().filter(|n| !flag(**n, "Visible", true)).map(|n| attribute(*n, "IfcGuid")).filter(|g| !g.is_empty()).collect();
                // only hidden ones named: the rest shows; else what is named is what shows
                if !hidden.is_empty() || shown.is_empty() {
                    view.exceptions = hidden;
                } else if view.selection.len() < shown.len() {
                    view.default_visibility = false;
                    view.exceptions = shown;
                }
                for n in all.iter().filter(|n| n.attribute("Color").is_some()) {
                    let (color, guid) = (attribute(*n, "Color"), attribute(*n, "IfcGuid"));
                    match view.coloring.iter_mut().find(|(c, _)| *c == color) {
                        Some((_, list)) => list.push(guid),
                        None => view.coloring.push((color, vec![guid])),
                    }
                }
            }
            (selection, visibility) => {
                view.selection = selection.map(guids).unwrap_or_default();
                if let Some(v) = visibility {
                    view.default_visibility = flag(v, "DefaultVisibility", true);
                    view.exceptions = v.children().find(|n| n.has_tag_name("Exceptions")).map(guids).unwrap_or_default();
                }
            }
        }
        // the hints: under the components (2.1) or under their visibility (3.0)
        if let Some(hints) = components.descendants().find(|n| n.has_tag_name("ViewSetupHints")) {
            view.spaces_visible = flag(hints, "SpacesVisible", false);
            view.space_boundaries_visible = flag(hints, "SpaceBoundariesVisible", false);
            view.openings_visible = flag(hints, "OpeningsVisible", false);
        }
        if let Some(coloring) = child("Coloring") {
            for color in coloring.children().filter(|n| n.has_tag_name("Color")) {
                view.coloring.push((attribute(color, "Color"), guids(color)));
            }
        }
    }
    let camera = |name: &str| root.children().find(|n| n.has_tag_name(name));
    let (perspective, orthogonal) = (camera("PerspectiveCamera"), camera("OrthogonalCamera"));
    if let Some(node) = perspective.or(orthogonal) {
        let number = |name: &str| text(node, name).parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0);
        let lens = if perspective.is_some() { Lens::Perspective { field_of_view: number("FieldOfView").unwrap_or(60.0) } } else { Lens::Orthogonal { view_to_world_scale: number("ViewToWorldScale").unwrap_or(10.0) } };
        if let (Some(position), Some(direction), Some(up)) = (vector(node, "CameraViewPoint"), vector(node, "CameraDirection"), vector(node, "CameraUpVector")) {
            if direction.iter().any(|v| v.abs() > 1e-12) {
                view.camera = Some(ViewCamera { position, direction, up, lens });
            }
        }
    }
    for plane in root.descendants().filter(|n| n.has_tag_name("ClippingPlane")) {
        if let (Some(location), Some(direction)) = (vector(plane, "Location"), vector(plane, "Direction")) {
            view.clipping_planes.push(ClippingPlane { location, direction });
        }
    }
    Ok(view)
}

/// A topic's folder as a topic.
fn read_topic(folder: &str, files: &BTreeMap<String, Vec<u8>>) -> Result<Topic, String> {
    let markup = files.get("markup.bcf").ok_or_else(|| format!("{folder}: no markup"))?;
    let doc = xml(markup)?;
    let node = doc.descendants().find(|n| n.has_tag_name("Topic")).ok_or_else(|| format!("{folder}: no topic"))?;
    let mut topic = Topic {
        guid: Some(attribute(node, "Guid")).filter(|g| !g.is_empty()).unwrap_or_else(|| folder.to_string()),
        title: text(node, "Title"),
        kind: attribute(node, "TopicType"),
        status: attribute(node, "TopicStatus"),
        priority: text(node, "Priority"),
        labels: node.descendants().filter(|n| n.has_tag_name("Labels") || n.has_tag_name("Label")).filter(|n| n.children().all(|c| !c.is_element())).filter_map(|n| n.text()).map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect(),
        description: text(node, "Description"),
        creation_date: text(node, "CreationDate"),
        creation_author: text(node, "CreationAuthor"),
        modified_date: text(node, "ModifiedDate"),
        modified_author: text(node, "ModifiedAuthor"),
        due_date: text(node, "DueDate"),
        assigned_to: text(node, "AssignedTo"),
        stage: text(node, "Stage"),
        index: text(node, "Index").parse().ok(),
        ..Topic::default()
    };
    // comments and viewpoints: beside the topic (2.x) or inside it (3.0)
    for c in doc.descendants().filter(|n| n.has_tag_name("Comment") && n.attribute("Guid").is_some()) {
        topic.comments.push(Comment {
            guid: attribute(c, "Guid"),
            date: text(c, "Date"),
            author: text(c, "Author"),
            text: text(c, "Comment"),
            viewpoint: c.children().find(|n| n.has_tag_name("Viewpoint")).map(|v| attribute(v, "Guid")).filter(|g| !g.is_empty()),
            modified_date: text(c, "ModifiedDate"),
            modified_author: text(c, "ModifiedAuthor"),
        });
    }
    let mut used: Vec<String> = vec!["markup.bcf".into()];
    let listed: Vec<Node> = doc.descendants().filter(|n| (n.has_tag_name("Viewpoints") || n.has_tag_name("ViewPoint")) && n.attribute("Guid").is_some()).collect();
    for v in &listed {
        let (file, picture) = (text(*v, "Viewpoint"), text(*v, "Snapshot"));
        let guid = attribute(*v, "Guid");
        let mut view = match files.get(&file) {
            Some(bytes) => read_viewpoint(bytes, &guid)?,
            None => Viewpoint { guid: guid.clone(), default_visibility: true, ..Viewpoint::default() },
        };
        view.guid = guid;
        if let Some(bytes) = files.get(&picture) {
            view.snapshot = Some((picture.clone(), bytes.clone()));
        }
        used.extend([file, picture]);
        topic.viewpoints.push(view);
    }
    // 2.0: one viewpoint and one picture under fixed names, not listed
    if listed.is_empty() {
        if let Some(bytes) = files.get("viewpoint.bcfv") {
            let mut view = read_viewpoint(bytes, &topic.guid)?;
            let picture = ["snapshot.png", "snapshot.jpg"].into_iter().find(|p| files.contains_key(*p));
            view.snapshot = picture.map(|p| (p.to_string(), files[p].clone()));
            used.extend(["viewpoint.bcfv".to_string(), picture.unwrap_or_default().to_string()]);
            topic.viewpoints.push(view);
        }
    }
    topic.attachments = files.iter().filter(|(name, _)| !used.contains(name)).map(|(n, b)| (n.clone(), b.clone())).collect();
    topic.original = Some(files.clone());
    Ok(topic)
}

fn point(name: &str, v: [f64; 3]) -> String {
    format!("<{name}><X>{}</X><Y>{}</Y><Z>{}</Z></{name}>", v[0], v[1], v[2])
}

fn components(list: &[String]) -> String {
    list.iter().map(|g| format!("<Component IfcGuid=\"{}\"/>", escape(g))).collect()
}

/// A viewpoint as its BCF 2.1 file.
fn write_viewpoint(view: &Viewpoint) -> String {
    let mut out = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<VisualizationInfo Guid=\"{}\">", escape(&view.guid));
    out += &format!(
        "<Components><ViewSetupHints SpacesVisible=\"{}\" SpaceBoundariesVisible=\"{}\" OpeningsVisible=\"{}\"/>",
        view.spaces_visible, view.space_boundaries_visible, view.openings_visible
    );
    if !view.selection.is_empty() {
        out += &format!("<Selection>{}</Selection>", components(&view.selection));
    }
    out += &format!("<Visibility DefaultVisibility=\"{}\">", view.default_visibility);
    if !view.exceptions.is_empty() {
        out += &format!("<Exceptions>{}</Exceptions>", components(&view.exceptions));
    }
    out += "</Visibility>";
    if !view.coloring.is_empty() {
        out += "<Coloring>";
        for (color, list) in &view.coloring {
            out += &format!("<Color Color=\"{}\">{}</Color>", escape(color), components(list));
        }
        out += "</Coloring>";
    }
    out += "</Components>";
    if let Some(c) = &view.camera {
        let (name, lens) = match c.lens {
            Lens::Perspective { field_of_view } => ("PerspectiveCamera", format!("<FieldOfView>{field_of_view}</FieldOfView>")),
            Lens::Orthogonal { view_to_world_scale } => ("OrthogonalCamera", format!("<ViewToWorldScale>{view_to_world_scale}</ViewToWorldScale>")),
        };
        out += &format!("<{name}>{}{}{}{lens}</{name}>", point("CameraViewPoint", c.position), point("CameraDirection", c.direction), point("CameraUpVector", c.up));
    }
    if !view.clipping_planes.is_empty() {
        out += "<ClippingPlanes>";
        for plane in &view.clipping_planes {
            out += &format!("<ClippingPlane>{}{}</ClippingPlane>", point("Location", plane.location), point("Direction", plane.direction));
        }
        out += "</ClippingPlanes>";
    }
    out + "</VisualizationInfo>\n"
}

/// The files of a topic's folder, written from its values.
fn write_topic(topic: &Topic) -> BTreeMap<String, Vec<u8>> {
    let mut files = topic.attachments.clone();
    let tag = |name: &str, value: &str| if value.is_empty() { String::new() } else { format!("<{name}>{}</{name}>", escape(value)) };
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Markup>");
    out += &format!("<Topic Guid=\"{}\" TopicType=\"{}\" TopicStatus=\"{}\">", escape(&topic.guid), escape(&topic.kind), escape(&topic.status));
    // in the order of the schema
    out += &format!("<Title>{}</Title>", escape(&topic.title));
    out += &tag("Priority", &topic.priority);
    out += &topic.index.map(|i| format!("<Index>{i}</Index>")).unwrap_or_default();
    out += &topic.labels.iter().map(|l| tag("Labels", l)).collect::<String>();
    out += &format!("<CreationDate>{}</CreationDate><CreationAuthor>{}</CreationAuthor>", escape(&topic.creation_date), escape(&topic.creation_author));
    out += &tag("ModifiedDate", &topic.modified_date);
    out += &tag("ModifiedAuthor", &topic.modified_author);
    out += &tag("DueDate", &topic.due_date);
    out += &tag("AssignedTo", &topic.assigned_to);
    out += &tag("Stage", &topic.stage);
    out += &tag("Description", &topic.description);
    out += "</Topic>";
    for c in &topic.comments {
        out += &format!("<Comment Guid=\"{}\"><Date>{}</Date><Author>{}</Author><Comment>{}</Comment>", escape(&c.guid), escape(&c.date), escape(&c.author), escape(&c.text));
        if let Some(v) = &c.viewpoint {
            out += &format!("<Viewpoint Guid=\"{}\"/>", escape(v));
        }
        out += &tag("ModifiedDate", &c.modified_date);
        out += &tag("ModifiedAuthor", &c.modified_author);
        out += "</Comment>";
    }
    for (k, view) in topic.viewpoints.iter().enumerate() {
        // the first under the names every reader knows
        let file = if k == 0 { "viewpoint.bcfv".to_string() } else { format!("{}.bcfv", view.guid) };
        out += &format!("<Viewpoints Guid=\"{}\"><Viewpoint>{}</Viewpoint>", escape(&view.guid), escape(&file));
        if let Some((name, bytes)) = &view.snapshot {
            let extension = if name.to_ascii_lowercase().ends_with(".jpg") || name.to_ascii_lowercase().ends_with(".jpeg") { "jpg" } else { "png" };
            let picture = if k == 0 { format!("snapshot.{extension}") } else { format!("{}.{extension}", view.guid) };
            out += &format!("<Snapshot>{}</Snapshot>", escape(&picture));
            files.insert(picture, bytes.clone());
        }
        out += "</Viewpoints>";
        files.insert(file, write_viewpoint(view).into_bytes());
    }
    out += "</Markup>\n";
    files.insert("markup.bcf".into(), out.into_bytes());
    files
}

impl Issues {
    /// No issues yet, to be written as BCF 2.1.
    pub fn new() -> Issues {
        Issues { version: "2.1".into(), ..Issues::default() }
    }

    /// Reads a `.bcfzip` / `.bcf` archive.
    pub fn read(bytes: &[u8]) -> Result<Issues, String> {
        if bytes.len() > MAX_COMPRESSED {
            return Err("BCF archive is too large".into());
        }
        let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("Invalid BCF archive: {e}"))?;
        if zip.len() > MAX_ENTRIES {
            return Err("BCF archive has too many entries".into());
        }
        let mut folders: BTreeMap<String, BTreeMap<String, Vec<u8>>> = BTreeMap::new();
        let mut other = BTreeMap::new();
        let mut total = 0usize;
        for k in 0..zip.len() {
            let mut entry = zip.by_index(k).map_err(|e| e.to_string())?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().replace('\\', "/");
            safe_path(&name)?;
            let mut data = Vec::new();
            (&mut entry).take((MAX_EXPANDED - total) as u64 + 1).read_to_end(&mut data).map_err(|e| e.to_string())?;
            total += data.len();
            if total > MAX_EXPANDED {
                return Err("BCF archive expands too far".into());
            }
            match name.split_once('/') {
                Some((folder, file)) if !file.contains('/') => {
                    folders.entry(folder.to_string()).or_default().insert(file.to_string(), data);
                }
                _ => {
                    other.insert(name, data);
                }
            }
        }
        let version = other
            .get("bcf.version")
            .and_then(|b| xml(b).ok().map(|d| attribute(d.root_element(), "VersionId")))
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "2.1".into());
        let project = other.iter().find(|(name, _)| name.ends_with(".bcfp")).and_then(|(_, b)| {
            let doc = xml(b).ok()?;
            let node = doc.descendants().find(|n| n.has_tag_name("Project"))?;
            Some((attribute(node, "ProjectId"), text(node, "Name")))
        });
        let mut topics = Vec::new();
        for (folder, files) in &folders {
            if files.contains_key("markup.bcf") {
                topics.push(read_topic(folder, files)?);
            } else {
                // a folder of documents: kept as it is
                other.extend(files.iter().map(|(name, b)| (format!("{folder}/{name}"), b.clone())));
            }
        }
        // as their numbers say, else by when they were made
        topics.sort_by(|a, b| (a.index.is_none(), a.index, &a.creation_date).cmp(&(b.index.is_none(), b.index, &b.creation_date)));
        Ok(Issues { version, project, topics, other })
    }

    /// Writes the archive as BCF 2.1; a topic read from 2.1 and not changed since goes back as
    /// it was.
    pub fn write(&self) -> Result<Vec<u8>, String> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default();
        let mut put = |name: &str, bytes: &[u8]| -> Result<(), String> {
            zip.start_file(name, options).map_err(|e| e.to_string())?;
            zip.write_all(bytes).map_err(|e| e.to_string())
        };
        put("bcf.version", b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Version VersionId=\"2.1\"><DetailedVersion>2.1</DetailedVersion></Version>\n")?;
        let as_read = self.version == "2.1";
        for (name, bytes) in self.other.iter().filter(|(name, _)| name.as_str() != "bcf.version") {
            // the project file of another version does not fit this one
            if as_read || !name.ends_with(".bcfp") {
                put(name, bytes)?;
            }
        }
        if let Some((id, name)) = self.project.as_ref().filter(|_| !self.other.keys().any(|n| n.ends_with(".bcfp")) || !as_read) {
            let project = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ProjectExtension><Project ProjectId=\"{}\"><Name>{}</Name></Project><ExtensionSchema/></ProjectExtension>\n", escape(id), escape(name));
            put("project.bcfp", project.as_bytes())?;
        }
        for topic in &self.topics {
            let files = match topic.original.as_ref().filter(|_| as_read) {
                Some(files) => files.clone(),
                None => write_topic(topic),
            };
            for (name, bytes) in &files {
                put(&format!("{}/{name}", topic.guid), bytes)?;
            }
        }
        Ok(zip.finish().map_err(|e| e.to_string())?.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Issues {
        let mut issues = Issues::new();
        let mut topic = Topic::new("Wall through beam", "an@example.org");
        topic.priority = "High".into();
        topic.labels = vec!["Structure".into(), "Level 01".into()];
        topic.assigned_to = "bo@example.org".into();
        topic.description = "The wall <W-12> & the beam overlap".into();
        let mut view = Viewpoint::new();
        view.camera = Some(ViewCamera { position: [1.0, -8.5, 3.25], direction: [0.0, 1.0, -0.2], up: [0.0, 0.2, 1.0], lens: Lens::Perspective { field_of_view: 55.0 } });
        view.selection = vec!["0LV8Pid0X3IA3jJLVDPidY".into(), "2O2Fr$t4X7Zf8NOew3FLOH".into()];
        view.default_visibility = false;
        view.exceptions = view.selection.clone();
        view.coloring = vec![("FFFF0000".into(), vec!["0LV8Pid0X3IA3jJLVDPidY".into()])];
        view.clipping_planes = vec![ClippingPlane { location: [0.0, 0.0, 2.5], direction: [0.0, 0.0, 1.0] }];
        view.snapshot = Some(("snapshot.png".into(), vec![0x89, b'P', b'N', b'G']));
        let at = view.guid.clone();
        topic.viewpoints.push(view);
        let mut second = Viewpoint::new();
        second.camera = Some(ViewCamera { position: [0.0, 0.0, 30.0], direction: [0.0, 0.0, -1.0], up: [0.0, 1.0, 0.0], lens: Lens::Orthogonal { view_to_world_scale: 12.0 } });
        topic.viewpoints.push(second);
        topic.comment("Move the wall 100 mm", "bo@example.org", Some(at));
        issues.topics.push(topic);
        issues
    }

    #[test]
    fn issues_written_read_back_as_they_were() {
        let issues = sample();
        let again = Issues::read(&issues.write().unwrap()).unwrap();
        assert_eq!(again.version, "2.1");
        assert_eq!(again.topics.len(), 1);
        let (a, b) = (&issues.topics[0], &again.topics[0]);
        assert_eq!((&a.guid, &a.title, &a.status, &a.priority, &a.labels, &a.assigned_to, &a.description), (&b.guid, &b.title, &b.status, &b.priority, &b.labels, &b.assigned_to, &b.description));
        assert_eq!(a.comments, b.comments);
        assert_eq!(a.viewpoints, b.viewpoints);
        assert_eq!(b.elements(), ["0LV8Pid0X3IA3jJLVDPidY", "2O2Fr$t4X7Zf8NOew3FLOH"]);
    }

    #[test]
    fn an_unchanged_topic_goes_back_byte_for_byte_and_a_changed_one_from_its_values() {
        let first = sample().write().unwrap();
        let mut read = Issues::read(&first).unwrap();
        let files = |bytes: &[u8]| {
            let mut zip = ZipArchive::new(Cursor::new(bytes.to_vec())).unwrap();
            (0..zip.len())
                .map(|k| {
                    let mut entry = zip.by_index(k).unwrap();
                    let mut data = Vec::new();
                    entry.read_to_end(&mut data).unwrap();
                    (entry.name().to_string(), data)
                })
                .collect::<BTreeMap<_, _>>()
        };
        assert_eq!(files(&first), files(&read.write().unwrap()));
        read.topics[0].status = "Closed".into();
        read.topics[0].touch("an@example.org");
        let again = Issues::read(&read.write().unwrap()).unwrap();
        assert_eq!((again.topics[0].status.as_str(), again.topics[0].modified_author.as_str()), ("Closed", "an@example.org"));
        assert_eq!(again.topics[0].viewpoints.len(), 2);
    }

    #[test]
    fn version_3_markup_and_version_2_components_read() {
        let pack = |files: &[(&str, &str)]| {
            let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
            for (name, body) in files {
                zip.start_file(*name, SimpleFileOptions::default()).unwrap();
                zip.write_all(body.as_bytes()).unwrap();
            }
            zip.finish().unwrap().into_inner()
        };
        let three = pack(&[
            ("bcf.version", "<Version VersionId=\"3.0\"/>"),
            ("a1/markup.bcf", "<Markup><Topic Guid=\"a1\" TopicType=\"Clash\" TopicStatus=\"Active\"><Title>Duct</Title><Labels><Label>MEP</Label></Labels><Comments><Comment Guid=\"c1\"><Date>2026-01-02T10:00:00Z</Date><Author>an</Author><Comment>See here</Comment><Viewpoint Guid=\"v1\"/></Comment></Comments><Viewpoints><ViewPoint Guid=\"v1\"><Viewpoint>v1.bcfv</Viewpoint><Snapshot>v1.png</Snapshot></ViewPoint></Viewpoints></Topic></Markup>"),
            ("a1/v1.bcfv", "<VisualizationInfo Guid=\"v1\"><Components><Selection><Component IfcGuid=\"G1\"/></Selection><Visibility DefaultVisibility=\"true\"><ViewSetupHints SpacesVisible=\"true\"/><Exceptions><Component IfcGuid=\"G2\"/></Exceptions></Visibility></Components><OrthogonalCamera><CameraViewPoint><X>1</X><Y>2</Y><Z>3</Z></CameraViewPoint><CameraDirection><X>0</X><Y>0</Y><Z>-1</Z></CameraDirection><CameraUpVector><X>0</X><Y>1</Y><Z>0</Z></CameraUpVector><ViewToWorldScale>20</ViewToWorldScale><AspectRatio>1.5</AspectRatio></OrthogonalCamera></VisualizationInfo>"),
            ("a1/v1.png", "png"),
        ]);
        let issues = Issues::read(&three).unwrap();
        let topic = &issues.topics[0];
        assert_eq!((issues.version.as_str(), topic.kind.as_str(), topic.labels.clone()), ("3.0", "Clash", vec!["MEP".to_string()]));
        assert_eq!((topic.comments.len(), topic.comments[0].viewpoint.as_deref()), (1, Some("v1")));
        let view = &topic.viewpoints[0];
        assert_eq!((view.selection.clone(), view.exceptions.clone(), view.spaces_visible), (vec!["G1".to_string()], vec!["G2".to_string()], true));
        assert_eq!(view.camera.map(|c| c.lens), Some(Lens::Orthogonal { view_to_world_scale: 20.0 }));
        assert!(view.snapshot.is_some());
        // written as 2.1 and read again: the same issue
        let again = Issues::read(&issues.write().unwrap()).unwrap();
        // the first picture goes under the name every reader knows
        let mut views = topic.viewpoints.clone();
        views[0].snapshot.as_mut().unwrap().0 = "snapshot.png".into();
        assert_eq!((again.version.as_str(), &again.topics[0].viewpoints, &again.topics[0].comments), ("2.1", &views, &topic.comments));

        let two = pack(&[
            ("bcf.version", "<Version VersionId=\"2.0\"/>"),
            ("b1/markup.bcf", "<Markup><Topic Guid=\"b1\" TopicStatus=\"Open\"><Title>Old</Title></Topic></Markup>"),
            ("b1/viewpoint.bcfv", "<VisualizationInfo><Components><Component IfcGuid=\"G1\" Selected=\"true\" Visible=\"true\"/><Component IfcGuid=\"G2\" Visible=\"false\"/></Components></VisualizationInfo>"),
            ("b1/snapshot.png", "png"),
        ]);
        let issues = Issues::read(&two).unwrap();
        let view = &issues.topics[0].viewpoints[0];
        assert_eq!((view.selection.clone(), view.default_visibility, view.exceptions.clone(), view.snapshot.is_some()), (vec!["G1".to_string()], true, vec!["G2".to_string()], true));
    }
}
