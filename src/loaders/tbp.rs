//! Project packages of a retired model viewer (`.tbp`): the models of a project put together,
//! each placed where the project put it.
//!
//! A package is a zip with one folder named after the project. In it lies the project itself (a
//! small XML file named as the folder), and folders of more such XML files: `modelreferences`
//! (each model: its name, the file it is in, and where it is placed: three axes and a position
//! in the project's unit), `modelgroups` (the models in the order the project lists them),
//! `links` and `persons`. The models are IFC files, zipped, in `files/Models`.
//!
//! Each model is read as an IFC file of its own and becomes layers named after it. The elements
//! of the second model and on are numbered apart, so no two models share an id. What else a
//! package holds (folders this reader does not know) is named in the project information.

use super::{ifc::IfcLoader, Loaded, Loader, Phase, Progress, Source};
use crate::model::{GridLine, ProjectData};
use glam::{DMat3, DVec3};
use std::io::Read;
use std::sync::Arc;

pub struct TbpLoader;

/// Where a model of the package lies and what it is called.
#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    pub name: String,
    /// The path of its file in the package, from the project's folder.
    pub location: String,
    /// Its axes (columns) and where its origin lies (in the project's unit).
    pub axes: [[f64; 3]; 3],
    pub position: [f64; 3],
}

/// What the project says of itself: its name and its unit as metres.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub name: String,
    pub unit: f64,
}

/// The text of the first child `name` of `node`.
fn child<'a>(node: roxmltree::Node<'a, 'a>, name: &str) -> Option<roxmltree::Node<'a, 'a>> {
    node.children().find(|c| c.has_tag_name(name))
}

fn text(node: roxmltree::Node, name: &str) -> String {
    child(node, name).and_then(|c| c.text()).unwrap_or("").trim().to_string()
}

fn point(node: roxmltree::Node, name: &str) -> Option<[f64; 3]> {
    let p = child(node, name)?;
    let n = |k: &str| text(p, k).parse::<f64>().ok();
    Some([n("X")?, n("Y")?, n("Z")?])
}

fn document(xml: &str) -> Result<roxmltree::Document<'_>, String> {
    roxmltree::Document::parse(xml.trim_start_matches('\u{feff}')).map_err(|e| e.to_string())
}

/// The project of its XML file.
pub fn project(xml: &str) -> Result<Project, String> {
    let doc = document(xml)?;
    let root = doc.root_element();
    if !root.has_tag_name("Project") {
        return Err("not a project".into());
    }
    let unit = match text(root, "UnitType").to_ascii_lowercase().as_str() {
        "meter" | "metre" => 1.0,
        "centimeter" | "centimetre" => 0.01,
        "inch" => 0.0254,
        "foot" | "feet" => 0.3048,
        _ => 0.001,
    };
    Ok(Project { name: text(root, "Name"), unit })
}

/// A model of its XML file; `None` when it names no file.
pub fn reference(xml: &str) -> Result<Option<Reference>, String> {
    let doc = document(xml)?;
    let root = doc.root_element();
    let location = text(root, "Location").replace('\\', "/");
    if !root.has_tag_name("ModelFile") || location.is_empty() {
        return Ok(None);
    }
    let axes = [point(root, "AxisX").unwrap_or([1.0, 0.0, 0.0]), point(root, "AxisY").unwrap_or([0.0, 1.0, 0.0]), point(root, "AxisZ").unwrap_or([0.0, 0.0, 1.0])];
    let name = text(root, "Name");
    let name = name.rsplit_once('.').filter(|(_, ext)| ext.eq_ignore_ascii_case("ifczip") || ext.eq_ignore_ascii_case("ifc")).map_or(name.as_str(), |(stem, _)| stem).to_string();
    Ok(Some(Reference { name, location, axes, position: point(root, "Position").unwrap_or([0.0; 3]) }))
}

/// The order of the models in the project's groups (their identifiers), as the project lists them.
fn group_order(xml: &str) -> Vec<String> {
    let Ok(doc) = document(xml) else { return Vec::new() };
    let root = doc.root_element();
    child(root, "ModelFiles").map(|list| list.children().filter_map(|c| c.text()).map(|t| t.trim().to_string()).collect()).unwrap_or_default()
}

/// Turns a point of a model (world metres) into the project's frame.
#[derive(Debug, Clone, Copy)]
struct Place {
    turn: DMat3,
    shift: DVec3,
}

impl Place {
    fn of(r: &Reference, unit: f64) -> Place {
        Place { turn: DMat3::from_cols(r.axes[0].into(), r.axes[1].into(), r.axes[2].into()), shift: DVec3::from_array(r.position) * unit }
    }

    fn still(&self) -> bool {
        self.turn == DMat3::IDENTITY && self.shift == DVec3::ZERO
    }

    fn point(&self, p: DVec3) -> DVec3 {
        self.turn * p + self.shift
    }
}

/// The id of element `id` of the model at `k`: the first keeps its ids, the others are numbered
/// apart (an IFC numbers its entities below 2^31; 2^40 apart leaves room for 8000 models below
/// the largest id a JSON number holds exactly).
fn element_id(k: usize, id: u64) -> u64 {
    ((k as u64) << 40) | id
}

impl Loader for TbpLoader {
    fn name(&self) -> &'static str {
        "Project package (TBP)"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["tbp"]
    }

    fn load(&self, source: &Source) -> Result<Vec<crate::model::Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&data[..])).map_err(|e| format!("TBP: not a project package: {e}"))?;
        let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
        for k in 0..zip.len() {
            let mut entry = zip.by_index(k).map_err(|e| format!("TBP: {e}"))?;
            if entry.is_dir() {
                continue;
            }
            let mut bytes = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut bytes).map_err(|e| format!("TBP: {e}"))?;
            entries.push((entry.name().map_err(|e| format!("TBP: {e}"))?.replace('\\', "/"), bytes));
        }
        let as_text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
        // the project: the file named as its folder
        let (folder, project) = entries
            .iter()
            .find_map(|(name, bytes)| {
                let (folder, file) = name.split_once('/')?;
                (folder == file).then(|| project(&as_text(bytes)).ok().map(|p| (folder.to_string(), p))).flatten()
            })
            .ok_or("TBP: the package names no project")?;
        let inside = |name: &str| name.strip_prefix(&format!("{folder}/")).map(str::to_string);
        // the models, in the order of the project's groups
        let mut models: Vec<(String, Reference)> = Vec::new();
        let mut order: Vec<String> = Vec::new();
        let mut unknown: Vec<String> = Vec::new();
        for (name, bytes) in &entries {
            let Some(path) = inside(name) else { continue };
            let (kind, id) = path.split_once('/').unwrap_or(("", &path));
            match kind {
                "modelreferences" => {
                    if let Some(r) = reference(&as_text(bytes))? {
                        models.push((format!("project://modelreferences/{id}"), r));
                    }
                }
                "modelgroups" => order.extend(group_order(&as_text(bytes))),
                "" | "links" | "persons" | "files" => {}
                other => {
                    if !unknown.iter().any(|u| u == other) {
                        unknown.push(other.to_string());
                    }
                }
            }
        }
        models.sort_by_key(|(id, r)| (order.iter().position(|o| o == id).unwrap_or(usize::MAX), r.name.clone()));
        if models.is_empty() {
            return Err("TBP: the package holds no models".into());
        }
        progress.set(Phase::Convert);
        let mut loaded = Loaded { layers: Vec::new(), views: Vec::new(), project: ProjectData::default(), structural: Vec::new(), origin: None };
        let mut grids: Vec<GridLine> = Vec::new();
        let mut levels: Vec<(String, f32)> = Vec::new();
        let mut failed: Vec<String> = Vec::new();
        let mut read = 0usize;
        for (k, (_, r)) in models.iter().enumerate() {
            let Some((_, bytes)) = entries.iter().find(|(name, _)| inside(name).as_deref() == Some(r.location.as_str())) else {
                failed.push(format!("{}: {}", r.name, "the file is not in the package"));
                continue;
            };
            let model = Source::Bytes { name: format!("{}.ifczip", r.name), data: Arc::new(bytes.clone()) };
            let one = match IfcLoader.load_with(&model, &Progress::default()) {
                Ok(one) => one,
                Err(e) => {
                    failed.push(format!("{}: {e}", r.name));
                    continue;
                }
            };
            // the model's own anchor, then where the project put it, less the first anchor
            let own = one.origin.unwrap_or(DVec3::ZERO);
            let first = *loaded.origin.get_or_insert(own);
            let place = Place::of(r, project.unit);
            let to_scene = |p: [f32; 3]| (place.point(DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64) + own) - first).as_vec3().to_array();
            for mut layer in one.layers {
                layer.name = format!("{} · {}", r.name, layer.name);
                for p in layer.positions.chunks_exact_mut(3) {
                    p.copy_from_slice(&to_scene([p[0], p[1], p[2]]));
                }
                if !place.still() {
                    let turn = place.turn.as_mat3();
                    for n in layer.normals.chunks_exact_mut(3) {
                        n.copy_from_slice(&(turn * glam::Vec3::new(n[0], n[1], n[2])).normalize_or_zero().to_array());
                    }
                }
                for e in &mut layer.elements {
                    e.0 = element_id(read, e.0);
                }
                for (id, props) in &mut layer.props {
                    *id = element_id(read, *id);
                    // the model it came from among its properties
                    if let Ok(serde_json::Value::Object(mut map)) = serde_json::from_str::<serde_json::Value>(props) {
                        map.insert("Model".into(), r.name.clone().into());
                        *props = serde_json::Value::Object(map).to_string();
                    }
                }
                loaded.layers.push(layer);
            }
            for g in one.project.grids {
                let (a, b) = (to_scene(g.a), to_scene(g.b));
                if !grids.iter().any(|h| h.name == g.name && h.a == a && h.b == b) {
                    grids.push(GridLine { name: g.name, a, b });
                }
            }
            for (name, z) in one.project.levels {
                let z = to_scene([0.0, 0.0, z])[2];
                if !levels.iter().any(|(n, y)| *n == name && (y - z).abs() < 0.001) {
                    levels.push((name, z));
                }
            }
            if k == 0 || read == 0 {
                loaded.project.info.extend(one.project.info.into_iter().filter(|(key, _)| key.starts_with("IFC schema") || key.starts_with("Geo")));
            }
            read += 1;
        }
        if read == 0 {
            return Err(format!("TBP: no model could be read ({})", failed.join("; ")));
        }
        levels.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        grids.sort_by(|a, b| a.name.cmp(&b.name));
        loaded.project.levels = levels;
        loaded.project.grids = grids;
        let info = &mut loaded.project.info;
        info.insert(0, ("Project".into(), project.name.clone()));
        info.insert(1, ("Models".into(), models.iter().map(|(_, r)| r.name.as_str()).collect::<Vec<_>>().join(", ")));
        if !failed.is_empty() {
            info.push(("Models not read".into(), failed.join("; ")));
        }
        if !unknown.is_empty() {
            info.push(("Not read from the package".into(), unknown.join(", ")));
        }
        Ok(loaded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROJECT: &str = "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<Project SchemaVersion=\"6\"><Identifier>project://projects/p</Identifier><Name>Kapconstructie</Name><UnitType>Millimeter</UnitType></Project>";

    fn model(name: &str, location: &str, position: [f64; 3]) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?><ModelFile SchemaVersion=\"6\"><AxisX><X>0</X><Y>1</Y><Z>0</Z></AxisX><AxisY><X>-1</X><Y>0</Y><Z>0</Z></AxisY><AxisZ><X>0</X><Y>0</Y><Z>1</Z></AxisZ><Extension>.ifcZIP</Extension><Location>{location}</Location><Name>{name}</Name><Position><X>{}</X><Y>{}</Y><Z>{}</Z></Position></ModelFile>",
            position[0], position[1], position[2]
        )
    }

    #[test]
    fn the_project_and_its_models_read_from_their_files() {
        assert_eq!(project(PROJECT).unwrap(), Project { name: "Kapconstructie".into(), unit: 0.001 });
        assert!(project("<Link/>").is_err());
        let r = reference(&model("grid.ifcZIP", "files/Models/a.ifcZIP", [1000.0, 0.0, 250.0])).unwrap().unwrap();
        assert_eq!((r.name.as_str(), r.location.as_str(), r.position), ("grid", "files/Models/a.ifcZIP", [1000.0, 0.0, 250.0]));
        // turned a quarter: the model's x lies along the project's y
        let place = Place::of(&r, 0.001);
        let p = place.point(DVec3::new(2.0, 0.0, 0.0));
        assert!((p - DVec3::new(1.0, 2.0, 0.25)).length() < 1e-9, "{p}");
        assert!(reference("<ModelFile><Name>x</Name></ModelFile>").unwrap().is_none());
    }

    #[test]
    fn the_models_follow_the_order_of_their_group_and_keep_their_ids_apart() {
        let group = "<ModelGroup><ModelFiles><string>project://modelreferences/b</string><string>project://modelreferences/a</string></ModelFiles></ModelGroup>";
        assert_eq!(group_order(group), ["project://modelreferences/b", "project://modelreferences/a"]);
        assert_eq!(element_id(0, 77), 77);
        assert_ne!(element_id(1, 77), element_id(2, 77));
        assert!(element_id(8000, u64::from(u32::MAX)) < 1 << 53);
    }

    /// A package of a real project, when there is one at `O3V_TBP` (not in the repository).
    #[test]
    fn a_package_opens() {
        let Ok(path) = std::env::var("O3V_TBP") else { return };
        let loaded = TbpLoader.load_with(&Source::File(path.into()), &Progress::default()).unwrap();
        assert!(!loaded.layers.is_empty());
        eprintln!("{:?}", loaded.project.info);
        for layer in &loaded.layers {
            eprintln!("{} · {} triangles", layer.name, layer.triangle_count());
        }
    }
}
