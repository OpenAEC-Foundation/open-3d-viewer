//! IFC5 / IFCX (`.ifcx`), the native file of the project document: the model as IfcX nodes
//! (a project, a building, its storeys from the document's levels, and one node per element
//! with its class, name, properties, colour and triangle mesh), and the document's floor
//! plans, sections, elevations and sheets as `o3v::` nodes, which the IFCX reader gives back to
//! the document. Coordinates are the model's (metres, Z up); an element under a storey is
//! placed relative to it, as the storey's transform moves its children.

use super::Writer;
use crate::document::{uuid, Document};
use crate::model::Model;
use crate::units::{UnitKey, UnitKeySet};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::Path;

pub struct IfcxWriter;

impl Writer for IfcxWriter {
    fn name(&self) -> &'static str {
        "IFC5 (IFCX)"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["ifcx"]
    }

    fn mime(&self) -> &'static str {
        "application/json"
    }

    fn write(&self, model: &Model, document: &Document, file_name: &str, mut out: &mut dyn Write) -> io::Result<()> {
        write(model, document, file_name, &mut out)
    }
}

/// Property keys that are written as their own attributes, not as properties.
const OWN_KEYS: [&str; 8] = ["Name", "name", "IfcType", "Class", "class", "Path", "Id", DEFINITION];

/// The property of a modelled element that holds its definitions (a JSON array: the element's
/// and its connections'); written as the node's `openaec::element` attribute, from which the
/// reader gives the elements back.
pub const DEFINITION: &str = "OpenAEC_definition";

/// The attribute of a grid line's node that holds its definition: its name and its ends in plan
/// (`start`, `end`: x, y in metres). The reader gives it back among the project's definitions,
/// under this key.
pub const GRID: &str = "openaec::grid";

/// The path of the node of a layer's element (`u64::MAX`: the layer without elements) in the
/// file named `file_name`.
pub fn element_path(file_name: &str, layer: usize, id: u64) -> String {
    uuid(&format!("{file_name}/element/{layer}/{id}"))
}

/// Writes the model and its document as an IFCX file named `file_name`.
pub fn write(model: &Model, document: &Document, file_name: &str, out: &mut impl Write) -> io::Result<()> {
    let stem = Path::new(file_name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let project = uuid(&format!("{file_name}/project"));
    let building = uuid(&format!("{file_name}/building"));
    let levels = document.levels();
    let mut data: Vec<Value> = Vec::new();
    // children of the building (elements without a storey) and of every storey
    let mut building_children = Map::new();
    let mut storey_children: Vec<Map<String, Value>> = vec![Map::new(); levels.len()];
    let add_child = |children: &mut Map<String, Value>, name: String, path: String| {
        let mut key = name.clone();
        let mut n = 1;
        while children.contains_key(&key) {
            n += 1;
            key = format!("{name} ({n})");
        }
        children.insert(key, path.into());
    };

    for (li, layer) in model.layers.iter().enumerate() {
        if layer.points {
            let path = uuid(&format!("{file_name}/cloud/{li}"));
            let positions: Vec<[f32; 3]> = layer.positions.chunks_exact(3).map(|p| [p[0], p[1], p[2]]).collect();
            let mut cloud = json!({"positions": positions});
            if let Some(colors) = &layer.colors {
                let colors: Vec<[f32; 3]> = colors.iter().map(|c| [c[0], c[1], c[2]].map(|v| v as f32 / 255.0)).collect();
                cloud["colors"] = json!(colors);
            }
            data.push(json!({"path": path, "attributes": {"bsi::ifc::prop::Name": layer.name, "points::array": cloud}}));
            add_child(&mut building_children, layer.name.clone(), path);
            continue;
        }
        // an element per range; a layer without elements is one element
        let whole = [(u64::MAX, 0u32, layer.indices.len() as u32)];
        let ranges: &[(u64, u32, u32)] = if layer.elements.is_empty() { &whole } else { &layer.elements };
        for &(id, first, count) in ranges {
            let Some(indices) = layer.indices.get(first as usize..(first + count) as usize) else { continue };
            let props = if id == u64::MAX { Vec::new() } else { model.element_props(id) };
            let prop = |key: &str| props.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty());
            let class = prop("IfcType").or(prop("Class")).or(prop("class")).filter(|c| c.starts_with("Ifc")).unwrap_or("IfcBuildingElementProxy").to_string();
            let name = prop("Name").or(prop("name")).map(str::to_string).unwrap_or_else(|| layer.name.clone());
            // its own vertices, numbered from 0, and its lowest point
            let mut number: HashMap<u32, u32> = HashMap::new();
            let mut points: Vec<[f32; 3]> = Vec::new();
            let mut faces: Vec<u32> = Vec::with_capacity(indices.len());
            let mut colour = [0u64; 4];
            for &k in indices {
                let n = *number.entry(k).or_insert_with(|| {
                    let p = &layer.positions[k as usize * 3..k as usize * 3 + 3];
                    points.push([p[0], p[1], p[2]]);
                    if let Some(c) = layer.colors.as_ref().and_then(|c| c.get(k as usize)) {
                        (0..4).for_each(|i| colour[i] += c[i] as u64);
                    }
                    points.len() as u32 - 1
                });
                faces.push(n);
            }
            if points.is_empty() {
                continue;
            }
            let base = points.iter().map(|p| p[2]).fold(f32::INFINITY, f32::min) as f64;
            // its storey: the level it names, else the highest at or below its lowest point
            let storey = prop("level")
                .and_then(|l| levels.iter().position(|level| level.name == l))
                .or_else(|| levels.iter().rposition(|level| level.elevation <= base + 0.01));
            let lift = storey.map_or(0.0, |s| levels[s].elevation) as f32;
            let points: Vec<[f32; 3]> = points.into_iter().map(|p| [p[0], p[1], p[2] - lift]).collect();
            let rgba = if layer.colors.is_some() {
                let n = number.len().max(1) as f64;
                colour.map(|c| c as f64 / n / 255.0)
            } else {
                [layer.color[0] as f64, layer.color[1] as f64, layer.color[2] as f64, 1.0]
            };
            let mut attributes = Map::new();
            attributes.insert("bsi::ifc::class".into(), json!({"code": class}));
            attributes.insert("bsi::ifc::prop::Name".into(), name.clone().into());
            for (key, value) in props.iter().filter(|(k, _)| !OWN_KEYS.contains(&k.as_str())) {
                attributes.insert(format!("bsi::ifc::prop::{key}"), value.clone().into());
            }
            if let Some(definition) = prop(DEFINITION).and_then(|d| serde_json::from_str::<Value>(d).ok()) {
                attributes.insert("openaec::element".into(), definition);
            }
            attributes.insert("bsi::ifc::presentation::diffuseColor".into(), json!([rgba[0], rgba[1], rgba[2]]));
            if rgba[3] < 0.999 {
                attributes.insert("bsi::ifc::presentation::opacity".into(), json!(rgba[3]));
            }
            attributes.insert("usd::usdgeom::mesh".into(), json!({"points": points, "faceVertexIndices": faces}));
            let path = element_path(file_name, li, id);
            data.push(json!({"path": path, "attributes": attributes}));
            let label = if id == u64::MAX { name } else { format!("{name} #{id}") };
            match storey {
                Some(s) => add_child(&mut storey_children[s], label, path),
                None => add_child(&mut building_children, label, path),
            }
        }
    }

    // the storeys, with their elements; the building and the project above them
    for (mut node, children) in document.ifcx_nodes().into_iter().zip(storey_children) {
        node["children"] = Value::Object(children);
        data.push(node);
    }
    for level in levels {
        let mut key = level.name.clone();
        let mut n = 1;
        while building_children.contains_key(&key) {
            n += 1;
            key = format!("{} ({n})", level.name);
        }
        building_children.insert(key, level.path.clone().into());
    }
    data.push(json!({"path": building, "children": building_children, "attributes": {"bsi::ifc::class": {"code": "IfcBuilding"}, "bsi::ifc::prop::Name": "Building"}}));
    // the grid lines, under the project: each its name and its ends in plan (`openaec::grid`),
    // from which the reader gives them back to the modelled project
    let mut project_children = Map::new();
    project_children.insert("Building".into(), building.clone().into());
    for (i, grid) in model.project.grids.iter().enumerate() {
        let path = uuid(&format!("{file_name}/grid/{i}"));
        // to a tenth of a millimetre: the scene keeps them in single precision
        let end = |p: [f32; 3]| json!([(f64::from(p[0]) * 1e4).round() / 1e4, (f64::from(p[1]) * 1e4).round() / 1e4]);
        let mut key = format!("Grid {}", grid.name);
        let mut n = 1;
        while project_children.contains_key(&key) {
            n += 1;
            key = format!("Grid {} ({n})", grid.name);
        }
        project_children.insert(key, path.clone().into());
        data.push(json!({"path": path, "attributes": {"bsi::ifc::class": {"code": "IfcGridAxis"}, "bsi::ifc::prop::Name": grid.name, GRID: {"name": grid.name, "start": end(grid.a), "end": end(grid.b)}}}));
    }
    data.push(json!({"path": project, "children": project_children, "attributes": {"bsi::ifc::class": {"code": "IfcProject"}, "bsi::ifc::prop::Name": stem}}));
    data.extend(document.ifcx_drawing_nodes_with(&|set| hidden_paths(model, file_name, set)));

    let file = json!({
        "header": {
            "id": file_name,
            "ifcxVersion": "ifcx_alpha",
            "dataVersion": "1.0.0",
            "author": format!("Open BIM Suite {}", env!("CARGO_PKG_VERSION")),
            "timestamp": timestamp(),
        },
        "imports": [],
        "schemas": {},
        "data": data,
    });
    serde_json::to_writer(&mut *out, &file).map_err(io::Error::other)?;
    Ok(())
}

/// The element nodes written for a model's units: (unit, node path), a unit once per layer it
/// is drawn on. A layer without elements is one node, the rest of that layer; the rest of a
/// layer of a combined model is not named, its layers not being told apart by model here.
fn element_nodes(model: &Model, file_name: &str) -> Vec<(UnitKey, String)> {
    let one_file = model.origins.len() <= 1;
    let mut nodes = Vec::new();
    for (li, layer) in model.layers.iter().enumerate().filter(|(_, l)| !l.points) {
        if layer.elements.is_empty() {
            if one_file {
                nodes.push((UnitKey::LayerRest { model: 0, layer: li as u32 }, element_path(file_name, li, u64::MAX)));
            }
            continue;
        }
        let mut seen = std::collections::HashSet::new();
        for &(id, _, _) in layer.elements.iter().filter(|e| seen.insert(e.0)) {
            nodes.push((UnitKey::Element(model.element_key(id)), element_path(file_name, li, id)));
        }
    }
    nodes
}

/// The attribute a view's node gets for the elements it hides: the paths of their nodes in the
/// file named `file_name` (`hidden`), or, when it hides most of them (Isolate), the paths of
/// those it shows (`shown`).
fn hidden_paths(model: &Model, file_name: &str, hidden: &UnitKeySet) -> Option<(&'static str, Value)> {
    let nodes = element_nodes(model, file_name);
    let (off, on): (Vec<_>, Vec<_>) = nodes.into_iter().partition(|(key, _)| hidden.contains(key));
    if off.is_empty() {
        return None;
    }
    let paths = |nodes: Vec<(UnitKey, String)>| Value::from(nodes.into_iter().map(|(_, path)| Value::from(path)).collect::<Vec<_>>());
    Some(if off.len() > on.len() { ("shown", paths(on)) } else { ("hidden", paths(off)) })
}

/// The elements a view of an IFCX file hides, from its node's attribute (`hidden` or `shown`,
/// see [`hidden_paths`]): the elements of `model`, read from that file, whose nodes are named.
pub fn hidden_in(model: &Model, view: &Value) -> UnitKeySet {
    let (list, shown) = match (view.get("hidden"), view.get("shown")) {
        (Some(list), _) => (list, false),
        (None, Some(list)) => (list, true),
        (None, None) => return UnitKeySet::default(),
    };
    let named: std::collections::HashSet<&str> = list.as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    // the reader keeps the path of an element's node as its `Id`
    let mut units = Vec::new();
    for (li, layer) in model.layers.iter().enumerate().filter(|(_, l)| !l.points) {
        if layer.elements.is_empty() {
            units.push((UnitKey::LayerRest { model: 0, layer: li as u32 }, None));
        }
        for &(id, _, _) in &layer.elements {
            units.push((UnitKey::Element(model.element_key(id)), Some(id)));
        }
    }
    let path_of = |id: u64| -> Option<String> {
        let props: Value = serde_json::from_str(model.props.get(&id)?).ok()?;
        Some(props.get("Id")?.as_str()?.to_string())
    };
    units
        .into_iter()
        .filter(|(_, id)| {
            let named = id.and_then(path_of).is_some_and(|p| named.contains(p.as_str()));
            named != shown
        })
        .map(|(key, _)| key)
        .collect()
}

/// Now, as `YYYY-MM-DDTHH:MM:SSZ`.
pub(crate) fn timestamp() -> String {
    let seconds = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    // days since 1970-01-01 to a civil date (H. Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rest / 3600, rest / 60 % 60, rest % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loaders::{Progress, Source};
    use crate::model::Layer;
    use std::path::PathBuf;
    use std::sync::Arc;

    /// Two storeys of a room, each its own element (8 x 5 x 3 m walls), with properties.
    fn house() -> Model {
        let walls = |z: f32| -> Vec<f32> {
            [[0.0, 0.0], [8.0, 0.0], [8.0, 5.0], [0.0, 5.0]].iter().flat_map(|&[x, y]| [x, y, z]).chain([[0.0, 0.0], [8.0, 0.0], [8.0, 5.0], [0.0, 5.0]].iter().flat_map(|&[x, y]| [x, y, z + 3.0])).collect()
        };
        let ring = [0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7, 6, 3, 0, 4, 3, 4, 7];
        let mut positions = walls(0.0);
        positions.extend(walls(3.0));
        let mut indices: Vec<u32> = ring.to_vec();
        indices.extend(ring.iter().map(|i| i + 8));
        let layer = Layer {
            name: "Walls".into(),
            color: [0.8, 0.5, 0.3],
            positions,
            indices,
            elements: vec![(10, 0, 24), (20, 24, 24)],
            props: vec![
                (10, r#"{"Name":"Ground walls","IfcType":"IfcWall","level":"00","FireRating":"R60"}"#.into()),
                (20, r#"{"Name":"First walls","category":"Walls"}"#.into()),
            ],
            ..Layer::default()
        };
        let mut model = Model::new(PathBuf::from("house.rvt"), "Revit", vec![layer]);
        model.project.levels = vec![("00".into(), 0.0), ("01".into(), 3.0)];
        model
    }

    fn reopen(bytes: Vec<u8>) -> Model {
        let source = Source::Bytes { name: "house.ifcx".into(), data: Arc::new(bytes) };
        crate::loaders::load_with(&source, &Progress::default()).expect("IFCX")
    }

    #[test]
    fn the_model_and_its_document_come_back_from_ifcx() {
        let model = house();
        let mut document = Document::from_model(&model);
        document.add_level("Roof", 6.0).unwrap();
        let ground = document.levels()[0].path.clone();
        let plan = document.add_plan(&ground).unwrap();
        document.add_dimension(&plan, [0.0, 0.0], [8.0, 0.0]);
        document.set_crop(&plan, Some(([-1.0, -1.0], [9.0, 6.0])));
        let section = document.add_section("Section A", [-1.0, 2.5], [9.0, 2.5], 20.0).unwrap();
        let sheet = document.add_sheet("Plans");
        document.place_plan(&sheet, &plan);
        document.place_plan(&sheet, &section);
        document.move_placed(&sheet, 1, [0.3, 0.2]);

        let mut bytes = Vec::new();
        write(&model, &document, "house.ifcx", &mut bytes).unwrap();
        let back = reopen(bytes);
        // the geometry where it was, by element, with class, name and properties
        assert_eq!(back.triangle_count(), model.triangle_count());
        for (got, want) in back.bounds.min.iter().chain(&back.bounds.max).zip(model.bounds.min.iter().chain(&model.bounds.max)) {
            assert!((got - want).abs() < 1e-4, "{:?} {:?}", back.bounds, model.bounds);
        }
        let props: Vec<&String> = back.props.values().collect();
        assert!(props.iter().any(|p| p.contains("\"Class\":\"IfcWall\"") && p.contains("\"Name\":\"Ground walls\"") && p.contains("\"FireRating\":\"R60\"")), "{props:?}");
        assert!(props.iter().any(|p| p.contains("\"Class\":\"IfcBuildingElementProxy\"") && p.contains("\"category\":\"Walls\"")), "{props:?}");
        // the levels, the added one too
        let levels: Vec<(&str, f32)> = back.project.levels.iter().map(|(n, z)| (n.as_str(), *z)).collect();
        assert_eq!(levels, [("00", 0.0), ("01", 3.0), ("Roof", 6.0)]);
        // the drawings: plan with its dimension and crop, the section, the sheet with both
        let again = Document::from_model(&back);
        let p = again.plan(&plan).expect("the floor plan");
        assert_eq!((p.name.as_str(), again.level(&p.level).map(|l| l.name.as_str())), ("00", Some("00")));
        assert_eq!((p.dimensions.len(), p.crop), (1, Some(([-1.0, -1.0], [9.0, 6.0]))));
        let s = again.section(&section).expect("the section");
        assert_eq!((s.name.as_str(), s.from, s.to, s.depth), ("Section A", [-1.0, 2.5], [9.0, 2.5], 20.0));
        let sh = again.sheet(&sheet).expect("the sheet");
        assert_eq!((sh.title.as_str(), sh.views.len(), sh.views[1].at), ("A01 Plans", 2, [0.3, 0.2]));
        assert!(timestamp().ends_with('Z') && timestamp().len() == 20);
    }

    #[test]
    fn the_elements_a_view_hides_come_back_from_ifcx() {
        let model = house();
        let mut document = Document::from_model(&model);
        let ground = document.levels()[0].path.clone();
        let (plan, section) = (document.add_plan(&ground).unwrap(), document.add_section("A", [-1.0, 2.5], [9.0, 2.5], 20.0).unwrap());
        let ground_walls = UnitKey::Element(model.element_key(10));
        let first_walls = UnitKey::Element(model.element_key(20));
        // the plan hides the ground walls; the section isolates them (it hides the rest)
        document.set_view_hidden(&plan, Arc::new(UnitKeySet::from_iter([ground_walls])));
        let mut bytes = Vec::new();
        write(&model, &document, "house.ifcx", &mut bytes).unwrap();
        let file: Value = serde_json::from_slice(&bytes).unwrap();
        let view = |path: &str| file["data"].as_array().unwrap().iter().find(|n| n["path"] == path).unwrap()["attributes"].clone();
        assert_eq!(view(&plan)["o3v::view::plan"]["hidden"], json!([element_path("house.ifcx", 0, 10)]), "the node of the element in this file");
        assert!(view(&section)["o3v::view::section"].get("hidden").is_none());
        let back = reopen(bytes);
        let named = |name: &str| {
            let id = *back.props.iter().find(|(_, p)| p.contains(&format!("\"Name\":\"{name}\""))).unwrap().0;
            UnitKey::Element(back.element_key(id))
        };
        let again = Document::from_model(&back);
        assert_eq!(again.view_hidden(&plan).unwrap().iter().collect::<Vec<_>>(), [&named("Ground walls")]);
        assert!(again.view_hidden(&section).unwrap().is_empty());
        // hiding most of them (here all), the file names those shown
        document.set_view_hidden(&section, Arc::new(UnitKeySet::from_iter([first_walls, ground_walls])));
        let mut bytes = Vec::new();
        write(&model, &document, "house.ifcx", &mut bytes).unwrap();
        let file: Value = serde_json::from_slice(&bytes).unwrap();
        let section_node = file["data"].as_array().unwrap().iter().find(|n| n["path"] == section.as_str()).unwrap()["attributes"]["o3v::view::section"].clone();
        assert_eq!(section_node["shown"], json!([]));
        let again = Document::from_model(&reopen(bytes));
        assert_eq!(again.view_hidden(&section).unwrap().len(), 2, "every element hidden");
    }

    #[test]
    fn ifcx_is_a_save_format() {
        let writer = crate::export::writer_for("house.ifcx").expect("an IFCX writer");
        assert_eq!(writer.extensions(), ["ifcx"]);
        let model = house();
        let bytes = crate::export::to_bytes(&model, &Document::from_model(&model), "house.ifcx").unwrap();
        let file: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(file["header"]["ifcxVersion"], "ifcx_alpha");
    }
}
