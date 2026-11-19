//! The IFCX form of a library. As in Open BIM Studio, a library file is an IFCX file that holds
//! only types, and a project file holds its library the same way, as its types.
//!
//! - A material is a node with `bsi::ifc::material` (`code`: its key, `uri`:
//!   `urn:openaec:material:<key>`), its English name, its colour as
//!   `bsi::ifc::presentation::diffuseColor`, and `openaec::material` (its names, hatch and colour).
//!   An element's body reaches it through `inherits`, as IFC5 does.
//! - A build-up is a node of class `IfcWallType`, `IfcSlabType` or `IfcCoveringType` with its
//!   predefined type, its English name and `openaec::buildup` (its id, role and layers, each a
//!   material key and a thickness in metres). IFCX has no layer-set schema yet.
//! - A linear type is a node of its role's type class (`IfcColumnType`, `IfcBeamType`, ...) with
//!   its predefined type, its English name and `openaec::lineartype` (its id, role, material key,
//!   profile and steel designation). Its profile stands beside it as IFC has it, in
//!   `openaec::profile`: the `class` of its parametric profile definition
//!   (`IfcIShapeProfileDef`), `ProfileType`, `ProfileName` (a profile of the steel table) and the
//!   definition's attributes by their IFC names (`OverallWidth`, `OverallDepth`, ...), lengths in
//!   metres and a slope in radians. IFCX has no schema for profiles yet. Reading takes the
//!   profile from there when the node has one, so another program can change a profile by its
//!   IFC attributes; a node without one reads as before.
//! - A system type is a node of its group's type class (`IfcCurtainWallType`, `IfcWallType`,
//!   `IfcCoveringType`, ...) with its predefined type, its English name and `openaec::systemtype`
//!   (its id, group, grid, members, panels, layers and level).
//!
//! - Every type's node has its NL-SfB code as `nlsfb::class` (see [`crate::nlsfb`]). Reading takes
//!   a type's code from there, so another program can change it: a code that is not the rule's
//!   is the type's own.
//! - A code of the library is a node with `nlsfb::class`, its English name and
//!   `openaec::classification`.
//! - A drawing set is a node with its English name and `openaec::drawingset`
//!   (see [`crate::annotate`]).
//!
//! Paths are UUIDs derived from the key or id, so a library written twice has the same paths.
//! Reading takes these nodes from any IFCX file and leaves the rest.

use crate::buildup::BuildUp;
use crate::library::Library;
use crate::material::{HatchKind, Material, Names};
use crate::nlsfb::{self, Code};
use crate::profile::{steel, LinearType, ProfileShape};
use crate::system::SystemType;
use crate::{uuid, Lang, LibraryError};
use serde_json::{json, Map, Value};

/// The attribute of a material node with the library's own data.
pub const MATERIAL: &str = "openaec::material";
/// The attribute of a type node with its build-up.
pub const BUILDUP: &str = "openaec::buildup";
/// The attribute of a type node with its linear type.
pub const LINEAR_TYPE: &str = "openaec::lineartype";
/// The attribute of a linear type's node with its profile as IFC has it.
pub const PROFILE: &str = "openaec::profile";
/// The attribute of a type node with its system type.
pub const SYSTEM_TYPE: &str = "openaec::systemtype";

/// The path of the node of material `key`.
pub fn material_path(key: &str) -> String {
    uuid(&format!("openaec-library/material/{key}"))
}

/// The path of the node of build-up `id`.
pub fn buildup_path(id: &str) -> String {
    uuid(&format!("openaec-library/buildup/{id}"))
}

/// The path of the node of linear type `id`.
pub fn linear_type_path(id: &str) -> String {
    uuid(&format!("openaec-library/lineartype/{id}"))
}

/// The path of the node of system type `id`.
pub fn system_type_path(id: &str) -> String {
    uuid(&format!("openaec-library/systemtype/{id}"))
}

/// The node of a material.
pub fn material_node(material: &Material) -> Value {
    json!({
        "path": material_path(&material.key),
        "attributes": {
            "bsi::ifc::material": {"code": material.key, "uri": format!("urn:openaec:material:{}", material.key)},
            "bsi::ifc::prop::Name": material.name(Lang::En),
            "bsi::ifc::presentation::diffuseColor": material.colour_f64(),
            MATERIAL: material,
        }
    })
}

/// A profile as IFC has it: the `class` of its parametric profile definition, `ProfileType`
/// (an area), `ProfileName` when it has one (the designation of a profile of the steel table),
/// and the definition's attributes by their IFC names: lengths in metres, a slope in radians.
pub fn profile_value(profile: &ProfileShape, name: Option<&str>) -> Value {
    let (class, attributes) = profile.ifc();
    let mut value = Map::new();
    value.insert("class".into(), json!(class));
    value.insert("ProfileType".into(), json!("AREA"));
    if let Some(name) = name {
        value.insert("ProfileName".into(), json!(name));
    }
    for (attribute, of) in attributes {
        value.insert(attribute.into(), json!(of));
    }
    Value::Object(value)
}

/// The profile of such a value. Attributes IFC leaves optional may be missing: no fillet or toe
/// radius, no slope, and an angle without a `Width` has equal legs. None when it names a
/// definition the library does not hold, or lacks an attribute IFC requires.
pub fn profile_of(value: &Value) -> Option<ProfileShape> {
    let class = value.get("class")?.as_str()?;
    let number = |attribute: &str| value.get(attribute).filter(|v| !v.is_null()).map(Value::as_f64);
    let values: Option<Vec<f64>> = ProfileShape::ifc_attributes(class)?
        .iter()
        .map(|&attribute| match number(attribute) {
            Some(given) => given,
            None if attribute != "Radius" && (attribute.ends_with("Radius") || attribute.ends_with("Slope")) => Some(0.0),
            None if attribute == "Width" => number("Depth").flatten(),
            None => None,
        })
        .collect();
    ProfileShape::from_ifc(class, &values?)
}

/// The nodes of a library: its materials, then its build-ups, linear types and system types as
/// types.
pub fn nodes(library: &Library) -> Vec<Value> {
    let mut nodes: Vec<Value> = library.materials.iter().map(material_node).collect();
    for b in &library.buildups {
        let name = library.name(&b.id, Lang::En).unwrap_or_default();
        nodes.push(json!({
            "path": buildup_path(&b.id),
            "attributes": {
                "bsi::ifc::class": {"code": b.role.ifc_type_class()},
                "bsi::ifc::prop::Name": name,
                "bsi::ifc::prop::PredefinedType": b.role.predefined_type(),
                nlsfb::CLASS: nlsfb::class_value(&b.nlsfb(&library.materials)),
                BUILDUP: b,
            }
        }));
    }
    for t in &library.linear_types {
        let name = library.linear_name(&t.id, Lang::En).unwrap_or_default();
        let (_, class, predefined) = t.role.ifc();
        nodes.push(json!({
            "path": linear_type_path(&t.id),
            "attributes": {
                "bsi::ifc::class": {"code": class},
                "bsi::ifc::prop::Name": name,
                "bsi::ifc::prop::PredefinedType": predefined,
                nlsfb::CLASS: nlsfb::class_value(&t.nlsfb()),
                LINEAR_TYPE: t,
                PROFILE: profile_value(&t.profile, t.designation.as_deref()),
            }
        }));
    }
    for t in &library.system_types {
        let name = library.system_name(&t.id, Lang::En).unwrap_or_default();
        let (class, predefined) = t.group.ifc_type();
        nodes.push(json!({
            "path": system_type_path(&t.id),
            "attributes": {
                "bsi::ifc::class": {"code": class},
                "bsi::ifc::prop::Name": name,
                "bsi::ifc::prop::PredefinedType": predefined,
                nlsfb::CLASS: nlsfb::class_value(&t.nlsfb()),
                SYSTEM_TYPE: t,
            }
        }));
    }
    nodes.extend(library.codes.iter().map(nlsfb::code_node));
    nodes.extend(library.drawing_sets.iter().map(crate::annotate::node));
    nodes
}

/// A library file: an IFCX file of the library's nodes alone, with `id`, `author` and
/// `timestamp` (`YYYY-MM-DDTHH:MM:SSZ`) in its header.
pub fn file(library: &Library, id: &str, author: &str, timestamp: &str) -> Value {
    json!({
        "header": {"id": id, "ifcxVersion": "ifcx_alpha", "dataVersion": "1.0.0", "author": author, "timestamp": timestamp},
        "imports": [],
        "schemas": {},
        "data": nodes(library),
    })
}

/// The library in an IFCX file: its material nodes (also those without the library's own data,
/// named by their code), its build-ups, linear types and system types. The attributes of nodes with the same path are
/// composed, a later one winning, as IFCX layers them. Refused when the file has no data, or a
/// type does not check against the file's materials.
pub fn read(file: &Value) -> Result<Library, LibraryError> {
    let data = file.get("data").and_then(Value::as_array).ok_or_else(|| LibraryError::Unreadable("no data".into()))?;
    // the attributes per path, in the order the paths first appear
    let mut order: Vec<&str> = Vec::new();
    let mut composed: std::collections::HashMap<&str, Map<String, Value>> = std::collections::HashMap::new();
    for node in data {
        let Some(path) = node.get("path").and_then(Value::as_str) else { continue };
        let attributes = composed.entry(path).or_insert_with(|| {
            order.push(path);
            Map::new()
        });
        if let Some(own) = node.get("attributes").and_then(Value::as_object) {
            for (key, value) in own {
                if value.is_null() {
                    attributes.remove(key);
                } else {
                    attributes.insert(key.clone(), value.clone());
                }
            }
        }
    }
    let mut library = Library::default();
    let mut buildups: Vec<(BuildUp, Option<String>)> = Vec::new();
    let mut linear_types: Vec<LinearType> = Vec::new();
    let mut system_types: Vec<SystemType> = Vec::new();
    for path in order {
        let attributes = &composed[path];
        if let Some(own) = attributes.get(MATERIAL) {
            let material: Material = serde_json::from_value(own.clone()).map_err(|e| LibraryError::Unreadable(format!("material {path}: {e}")))?;
            library.add_material(material)?;
        } else if let Some(code) = attributes.get("bsi::ifc::material").and_then(|m| m.get("code").or(Some(m))).and_then(Value::as_str) {
            if library.material(code).is_none() {
                let name = attributes.get("bsi::ifc::prop::Name").and_then(Value::as_str).unwrap_or(code);
                let colour = attributes
                    .get("bsi::ifc::presentation::diffuseColor")
                    .and_then(|c| serde_json::from_value::<[f64; 3]>(c.clone()).ok())
                    .map_or([175, 182, 195], |c| c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
                library.add_material(Material::new(code, Names::same(name), HatchKind::Generic, colour))?;
            }
        }
        if let Some(own) = attributes.get(nlsfb::CLASSIFICATION).and_then(|own| own.get("system").and_then(Value::as_str).is_none_or(|system| system == "NL-SfB").then_some(own)) {
            let code: Code = serde_json::from_value(own.clone()).map_err(|e| LibraryError::Unreadable(format!("code {path}: {e}")))?;
            library.add_code(code)?;
            continue;
        }
        if let Some(own) = attributes.get(crate::annotate::DRAWING_SET) {
            let set: crate::annotate::DrawingSet = serde_json::from_value(own.clone()).map_err(|e| LibraryError::Unreadable(format!("drawing set {path}: {e}")))?;
            library.add_drawing_set(set)?;
            continue;
        }
        // the code the node has as an element would, when it is one
        let class = nlsfb::class_of(attributes).map(str::to_string);
        if let Some(own) = attributes.get(BUILDUP) {
            let buildup: BuildUp = serde_json::from_value(own.clone()).map_err(|e| LibraryError::Unreadable(format!("type {path}: {e}")))?;
            buildups.push((buildup, class.clone()));
        }
        if let Some(own) = attributes.get(LINEAR_TYPE) {
            let mut linear: LinearType = serde_json::from_value(own.clone()).map_err(|e| LibraryError::Unreadable(format!("type {path}: {e}")))?;
            // its profile as IFC has it is its profile, when the node has one; one that was
            // changed there is a profile of the steel table only when it is named as one and
            // is that one
            if let Some((as_ifc, profile)) = attributes.get(PROFILE).and_then(|as_ifc| Some((as_ifc, profile_of(as_ifc)?))) {
                if !profile.same(&linear.profile) {
                    let table = as_ifc.get("ProfileName").and_then(Value::as_str).and_then(steel).filter(|table| table.shape.same(&profile));
                    linear.designation = table.map(|table| table.designation.clone());
                    linear.profile = table.map_or(profile, |table| table.shape.clone());
                }
            }
            if let Some(class) = &class {
                linear.code = Some(class.clone()).filter(|class| class != linear.rule_code());
            }
            linear_types.push(linear);
        }
        if let Some(own) = attributes.get(SYSTEM_TYPE) {
            let mut system: SystemType = serde_json::from_value(own.clone()).map_err(|e| LibraryError::Unreadable(format!("type {path}: {e}")))?;
            if let Some(class) = &class {
                system.code = Some(class.clone()).filter(|class| class != system.rule_code());
            }
            system_types.push(system);
        }
    }
    // a build-up's rule needs the materials, which may stand after it in the file
    for (mut b, class) in buildups {
        if let Some(class) = class {
            b.code = Some(class).filter(|class| class != b.rule_code(&library.materials));
        }
        library.add_buildup(b)?;
    }
    for t in linear_types {
        library.add_linear_type(t)?;
    }
    for t in system_types {
        library.add_system_type(t)?;
    }
    Ok(library)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buildup::{Layer, Role};
    use crate::profile::LinearRole;

    fn reread(library: &Library) -> Library {
        let text = serde_json::to_string(&file(library, "test.ifcx", "test", "2026-11-09T00:00:00Z")).unwrap();
        read(&serde_json::from_str(&text).unwrap()).unwrap()
    }

    #[test]
    fn a_library_reads_back_as_it_was_written() {
        let mut library = Library::starter();
        // a thickness that is no round number of millimetres
        library.add_buildup(BuildUp::new("wall.odd", Role::Wall, vec![Layer::new("timber", 0.1 / 3.0)])).unwrap();
        // steel profiles with tapered flanges and corner radii
        for (id, role, designation) in [("beam.ipn-200", LinearRole::Beam, "IPN 200"), ("column.shs", LinearRole::Column, "SHS 100x100x5"), ("member.l", LinearRole::Member, "L 100x50x8")] {
            library.add_linear_type(LinearType::steel(id, role, designation).unwrap()).unwrap();
        }
        assert_eq!(reread(&library), library);
    }

    #[test]
    fn materials_and_types_are_ifc_nodes() {
        let library = Library::starter();
        let nodes = nodes(&library);
        let concrete = nodes.iter().find(|n| n["path"] == material_path("concrete")).unwrap();
        assert_eq!(concrete["attributes"]["bsi::ifc::material"]["code"], "concrete");
        assert_eq!(concrete["attributes"]["bsi::ifc::prop::Name"], "concrete");
        let roof = nodes.iter().find(|n| n["path"] == buildup_path("roof.flat-concrete-pir")).unwrap();
        assert_eq!(roof["attributes"]["bsi::ifc::class"]["code"], "IfcSlabType");
        assert_eq!(roof["attributes"]["bsi::ifc::prop::PredefinedType"], "ROOF");
        assert_eq!(roof["attributes"]["bsi::ifc::prop::Name"], "PIR insulation-concrete 320");
        assert_eq!(roof["attributes"][BUILDUP]["layers"][0]["material"], "insulation-pir");
        let pipe = nodes.iter().find(|n| n["path"] == linear_type_path("pipe.generic-110")).unwrap();
        assert_eq!(pipe["attributes"]["bsi::ifc::class"]["code"], "IfcPipeSegmentType");
        assert_eq!(pipe["attributes"]["bsi::ifc::prop::Name"], "Generic round 110");
        assert_eq!(pipe["attributes"][LINEAR_TYPE]["profile"]["kind"], "circle");
        assert_eq!(pipe["attributes"][PROFILE], json!({"class": "IfcCircleProfileDef", "ProfileType": "AREA", "Radius": 0.055}));
        let masonry = nodes.iter().find(|n| n["path"] == system_type_path("system.masonry-waal-stretcher")).unwrap();
        assert_eq!(masonry["attributes"]["bsi::ifc::class"]["code"], "IfcWallType");
        assert_eq!(masonry["attributes"]["bsi::ifc::prop::Name"], "Brickwork 100 220×62.5 stretcher bond");
        assert_eq!(masonry["attributes"][SYSTEM_TYPE]["grid"]["pattern"], "running_bond");
        let ceiling = nodes.iter().find(|n| n["path"] == system_type_path("system.ceiling-600x600")).unwrap();
        assert_eq!(ceiling["attributes"]["bsi::ifc::prop::PredefinedType"], "CEILING");
        assert_eq!(nodes.len(), library.materials.len() + library.buildups.len() + library.linear_types.len() + library.system_types.len() + library.codes.len() + library.drawing_sets.len());
        // paths are stable
        assert_eq!(nodes, self::nodes(&Library::starter()));
    }

    #[test]
    fn a_profile_is_written_as_ifc_has_it_and_read_from_there() {
        let mut library = Library::starter();
        library.add_linear_type(LinearType::steel("beam.hea-200", LinearRole::Beam, "HEA 200").unwrap()).unwrap();
        let path = linear_type_path("beam.hea-200");
        let node = nodes(&library).into_iter().find(|n| n["path"] == path).unwrap();
        // the definition's class, what kind of profile it is, its name, and its attributes by
        // their IFC names, in metres
        let expected = json!({
            "class": "IfcIShapeProfileDef", "ProfileType": "AREA", "ProfileName": "HEA 200",
            "OverallWidth": 0.2, "OverallDepth": 0.19, "WebThickness": 0.0065, "FlangeThickness": 0.01, "FilletRadius": 0.018, "FlangeSlope": 0.0,
        });
        assert_eq!(node["attributes"][PROFILE], expected);
        assert_eq!(profile_of(&expected).as_ref(), Some(&steel("HEA 200").unwrap().shape));
        // a later layer changes the profile by its IFC attributes: it is that profile, and no
        // profile of the steel table any more
        let layered = |later: Value| {
            let mut data = nodes(&library);
            data.push(json!({"path": path, "attributes": {PROFILE: later}}));
            json!({ "data": data })
        };
        let read_with = |later: Value| read(&layered(later)).unwrap();
        let mut deeper = expected.clone();
        deeper["OverallDepth"] = json!(0.25);
        let changed = read_with(deeper);
        let t = changed.linear_type("beam.hea-200").unwrap();
        assert!(matches!(t.profile, ProfileShape::I { h, b, .. } if h == 0.25 && b == 0.2));
        assert_eq!((t.designation.as_deref(), changed.linear_name("beam.hea-200", Lang::En).as_deref()), (None, Some("Steel 200x250")));
        // another profile of the table, named as one: that profile of the table
        let heb = profile_value(&steel("HEB 300").unwrap().shape, Some("HEB 300"));
        assert_eq!(read_with(heb).linear_type("beam.hea-200").unwrap(), &LinearType::steel("beam.hea-200", LinearRole::Beam, "HEB 300").unwrap());
        // what IFC leaves optional may be missing: no fillet, no slope, an angle of equal legs
        let plain = read_with(json!({"class": "IfcIShapeProfileDef", "OverallWidth": 0.1, "OverallDepth": 0.3, "WebThickness": 0.01, "FlangeThickness": 0.02, "FlangeSlope": null}));
        assert_eq!(plain.linear_type("beam.hea-200").unwrap().profile, ProfileShape::I { h: 0.3, b: 0.1, tw: 0.01, tf: 0.02, r: 0.0, slope: 0.0 });
        assert_eq!(profile_of(&json!({"class": "IfcLShapeProfileDef", "Depth": 0.08, "Thickness": 0.008})), Some(ProfileShape::L { h: 0.08, b: 0.08, t: 0.008, r1: 0.0, r2: 0.0 }));
        // a definition the library does not hold, or one that lacks what IFC requires, leaves
        // the type as it was; so does a node without the attribute, as older files have them
        for other in [json!({"class": "IfcZShapeProfileDef", "Depth": 0.2}), json!({"class": "IfcCircleProfileDef"}), json!({"class": "IfcRectangleProfileDef", "XDim": 0.2}), Value::Null] {
            assert_eq!(read_with(other).linear_type("beam.hea-200"), library.linear_type("beam.hea-200"));
        }
        // a profile that cannot be made is refused, as a type that does not check is
        assert!(matches!(read(&layered(json!({"class": "IfcCircleProfileDef", "Radius": 0.0}))), Err(LibraryError::BadProfile(_))));
    }

    #[test]
    fn a_project_file_gives_its_types_and_plain_material_nodes() {
        // a project file: an element, a plain IFC5 material node and one of our types
        let mut wall_type = nodes(&Library::starter()).into_iter().find(|n| n["path"] == buildup_path("wall.brick-100")).unwrap();
        wall_type["attributes"][BUILDUP]["layers"][0]["material"] = json!("BRICK");
        let file = json!({"data": [
            {"path": "w1", "inherits": {"material": "m1"}, "attributes": {"bsi::ifc::class": {"code": "IfcWall"}}},
            {"path": "m1", "attributes": {"bsi::ifc::material": {"code": "BRICK"}, "bsi::ifc::presentation::diffuseColor": [0.8, 0.4, 0.2]}},
            wall_type,
        ]});
        let library = read(&file).unwrap();
        assert_eq!(library.materials.len(), 1);
        assert_eq!(library.materials[0].key, "BRICK");
        assert_eq!(library.materials[0].colour, [204, 102, 51]);
        assert_eq!(library.buildups.len(), 1);
        assert_eq!(library.name("wall.brick-100", Lang::Nl).unwrap(), "BRICK 100");
    }

    #[test]
    fn later_nodes_of_a_path_win_and_null_removes() {
        let concrete = material_node(&Library::starter().materials[0]);
        let path = concrete["path"].clone();
        let mut renamed = Library::starter().materials[0].clone();
        renamed.names.nl = "gewapend beton".into();
        let file = json!({"data": [concrete, {"path": path, "attributes": {MATERIAL: renamed}}]});
        assert_eq!(read(&file).unwrap().materials[0].names.nl, "gewapend beton");
        let gone = json!({"data": [material_node(&Library::starter().materials[0]), {"path": path, "attributes": {MATERIAL: null, "bsi::ifc::material": null}}]});
        assert!(read(&gone).unwrap().materials.is_empty());
    }

    #[test]
    fn a_file_that_is_no_library_is_refused_with_its_reason() {
        assert!(matches!(read(&json!({"header": {}})), Err(LibraryError::Unreadable(_))));
        let broken = json!({"data": [{"path": "t", "attributes": {BUILDUP: {"id": "x", "role": "wall", "layers": [{"material": "nothing", "thickness": 0.1}]}}}]});
        assert_eq!(read(&broken), Err(LibraryError::UnknownMaterial("nothing".into())));
        let wrong = json!({"data": [{"path": "t", "attributes": {BUILDUP: {"id": "x", "role": "door"}}}]});
        assert!(matches!(read(&wrong), Err(LibraryError::Unreadable(_))));
    }
}

#[cfg(test)]
mod code_tests {
    use super::*;
    use crate::material::Names;

    #[test]
    fn codes_are_written_on_every_type_and_read_from_there() {
        let mut library = Library::starter();
        library.add_code(Code { code: "22.11".into(), names: Names::new("binnenwanden, niet constructief, massief", "internal walls, non-loadbearing, solid", "x", "y") }).unwrap();
        library.buildups.iter_mut().find(|b| b.id == "wall.metal-stud-75").unwrap().code = Some("22.11".into());
        let written = file(&library, "codes.ifcx", "test", "2026-11-19T00:00:00Z");
        let node = |path: String| written["data"].as_array().unwrap().iter().find(|n| n["path"] == path).unwrap().clone();
        // a type's node has its code as an element has it, its own or the rule's
        assert_eq!(node(buildup_path("wall.metal-stud-75"))["attributes"]["nlsfb::class"]["code"], "22.11");
        assert_eq!(node(buildup_path("wall.cavity-pir-360"))["attributes"]["nlsfb::class"]["code"], "21");
        let column = library.linear_types.iter().find(|t| t.role == crate::LinearRole::Column).unwrap().id.clone();
        assert_eq!(node(linear_type_path(&column))["attributes"]["nlsfb::class"], nlsfb::class_value("28"));
        // a code of the library is a node of its own
        let code = node(nlsfb::code_path("22.11"));
        assert_eq!((code["attributes"]["nlsfb::class"]["code"].as_str(), code["attributes"]["bsi::ifc::prop::Name"].as_str()), (Some("22.11"), Some("internal walls, non-loadbearing, solid")));
        let read_back = read(&written).unwrap();
        assert_eq!(read_back, library);
        assert_eq!(read_back.coded("22.11", false), ["wall.metal-stud-75"]);
        assert!(read_back.coded("22", true).contains(&"wall.metal-stud-75") && !read_back.coded("22", false).contains(&"wall.metal-stud-75"));

        // another program changes the code on the node: the type has it as its own
        let mut changed = written.clone();
        let path = buildup_path("wall.cavity-pir-360");
        for n in changed["data"].as_array_mut().unwrap() {
            if n["path"] == path {
                n["attributes"]["nlsfb::class"] = json!({"code": "21.22"});
            }
        }
        let read_changed = read(&changed).unwrap();
        assert_eq!(read_changed.buildup("wall.cavity-pir-360").unwrap().code.as_deref(), Some("21.22"));
        // and one set back to the rule's has none of its own
        for n in changed["data"].as_array_mut().unwrap() {
            if n["path"] == buildup_path("wall.metal-stud-75") {
                n["attributes"]["nlsfb::class"] = json!({"code": "21"});
            }
        }
        assert_eq!(read(&changed).unwrap().buildup("wall.metal-stud-75").unwrap().code, None);
    }

    #[test]
    fn one_code_has_many_types_and_a_library_takes_the_codes_of_another() {
        let library = Library::starter();
        let walls = library.coded("21", false);
        assert!(walls.len() > 3 && walls.contains(&"wall.cavity-pir-360"), "{walls:?}");
        assert_eq!(library.code_name("21", Lang::Nl).as_deref(), Some("buitenwanden"));
        assert_eq!(library.code_name("21.22", Lang::En).as_deref(), Some("external walls"), "a code the library lacks is named by its group");
        let mut other = Library::default();
        other.add_code(Code { code: "21.22".into(), names: Names::same("spouwwanden") }).unwrap();
        let mut project = Library::starter();
        project.merge(&other);
        assert_eq!(project.code_name("21.22", Lang::Nl).as_deref(), Some("spouwwanden"));
        let at = project.codes.iter().position(|c| c.code == "21.22").unwrap();
        assert_eq!((project.codes[at - 1].code.as_str(), project.codes[at + 1].code.as_str()), ("21", "22"), "codes stand in order");
        assert_eq!(project.add_code(Code { code: "21.22".into(), names: Names::same("x") }), Err(LibraryError::Duplicate("21.22".into())));
        assert_eq!(project.add_code(Code { code: "wand".into(), names: Names::same("x") }), Err(LibraryError::BadCode("wand".into())));
    }
}
