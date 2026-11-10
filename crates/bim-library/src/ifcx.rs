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
//!   profile and steel designation).
//! - A system type is a node of its group's type class (`IfcCurtainWallType`, `IfcWallType`,
//!   `IfcCoveringType`, ...) with its predefined type, its English name and `openaec::systemtype`
//!   (its id, group, grid, members, panels, layers and level).
//!
//! Paths are UUIDs derived from the key or id, so a library written twice has the same paths.
//! Reading takes these nodes from any IFCX file and leaves the rest.

use crate::buildup::BuildUp;
use crate::library::Library;
use crate::material::{HatchKind, Material, Names};
use crate::profile::LinearType;
use crate::system::SystemType;
use crate::{uuid, Lang, LibraryError};
use serde_json::{json, Map, Value};

/// The attribute of a material node with the library's own data.
pub const MATERIAL: &str = "openaec::material";
/// The attribute of a type node with its build-up.
pub const BUILDUP: &str = "openaec::buildup";
/// The attribute of a type node with its linear type.
pub const LINEAR_TYPE: &str = "openaec::lineartype";
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
                LINEAR_TYPE: t,
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
                SYSTEM_TYPE: t,
            }
        }));
    }
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
    let mut buildups: Vec<BuildUp> = Vec::new();
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
                library.add_material(Material { key: code.into(), names: Names::same(name), hatch: HatchKind::Generic, colour })?;
            }
        }
        if let Some(own) = attributes.get(BUILDUP) {
            buildups.push(serde_json::from_value(own.clone()).map_err(|e| LibraryError::Unreadable(format!("type {path}: {e}")))?);
        }
        if let Some(own) = attributes.get(LINEAR_TYPE) {
            linear_types.push(serde_json::from_value(own.clone()).map_err(|e| LibraryError::Unreadable(format!("type {path}: {e}")))?);
        }
        if let Some(own) = attributes.get(SYSTEM_TYPE) {
            system_types.push(serde_json::from_value(own.clone()).map_err(|e| LibraryError::Unreadable(format!("type {path}: {e}")))?);
        }
    }
    for b in buildups {
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
        let masonry = nodes.iter().find(|n| n["path"] == system_type_path("system.masonry-waal-stretcher")).unwrap();
        assert_eq!(masonry["attributes"]["bsi::ifc::class"]["code"], "IfcWallType");
        assert_eq!(masonry["attributes"]["bsi::ifc::prop::Name"], "Brickwork 100 220×62.5 stretcher bond");
        assert_eq!(masonry["attributes"][SYSTEM_TYPE]["grid"]["pattern"], "running_bond");
        let ceiling = nodes.iter().find(|n| n["path"] == system_type_path("system.ceiling-600x600")).unwrap();
        assert_eq!(ceiling["attributes"]["bsi::ifc::prop::PredefinedType"], "CEILING");
        assert_eq!(nodes.len(), library.materials.len() + library.buildups.len() + library.linear_types.len() + library.system_types.len());
        // paths are stable
        assert_eq!(nodes, self::nodes(&Library::starter()));
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
