//! What an `.ifcfemresults` file holds beside its IFC entities: the project of the program that
//! wrote it, as JSON in a property (`Inhoud` of the set `OpenFEM_Projectbestand`). Read from it:
//!
//! - per member its steel profile by name (`profileName`: "HEA 300", "CHS 88.9x4") and its
//!   material; the profile's shape comes from the steel table of `bim-library`. A name the table
//!   does not hold is kept as a section without a shape;
//! - the grid (`structuralGrid`): axes across x and across y in millimetres, drawn over the
//!   extent of the nodes, and the levels along z.
//!
//! The members with a profile are also given as solids, a layer per profile: the profile's outline
//! swept along the member, its depth along the member's local z (the IFC `Axis`, which holds the
//! member's turn about its own line already).

use crate::model::{GridLine, Layer, ProjectData};
use bim_structural::*;
use glam::DVec3;
use std::collections::BTreeMap;

/// Arcs of a profile's outline stay within this of the true curve (metres).
const CHORD: f64 = 0.0005;
/// How far a grid line runs past the nodes, as a part of the extent across it, and at least this
/// many metres.
const GRID_PAST: (f64, f64) = (0.08, 1.0);
const STEEL: [f32; 3] = [0.58, 0.62, 0.68];

/// The project in the file, if it holds one that reads as JSON.
pub fn find(text: &str) -> Option<serde_json::Value> {
    let line = text.lines().find(|l| l.contains("=IFCPROPERTYSINGLEVALUE('Inhoud'"))?;
    let from = line.find("IFCTEXT(")? + "IFCTEXT(".len();
    let to = line.rfind("')")? + 1;
    let json = super::ifcfem::string(line.get(from..to)?)?.replace("\\\\", "\\");
    serde_json::from_str(&json).ok()
}

/// Gives the members of `model` their section and material from `project`; `number` is a member's
/// number as the project counts it.
pub fn sections(model: &mut AnalysisModel, project: &serde_json::Value) {
    let Some(beams) = project["model3d"]["beams"].as_array() else { return };
    let mut section_of: BTreeMap<String, Id> = BTreeMap::new();
    let mut material_of: BTreeMap<String, Id> = BTreeMap::new();
    for beam in beams {
        let Some(number) = beam["id"].as_u64().map(|n| n.to_string()) else { continue };
        let Some(member) = model.members.iter_mut().find(|m| m.number == number) else { continue };
        if let Some(name) = beam["material"].as_str().filter(|n| !n.is_empty()) {
            let next = sam_id(IdKind::Material, material_of.len() as u64 + 1);
            let id = *material_of.entry(name.to_string()).or_insert_with(|| {
                model.materials.push(Material { id: next, name: name.to_string(), e: None, g: None, nu: None, density: None, alpha_t: None });
                next
            });
            member.material = Some(id);
        }
        if let Some(name) = beam["profileName"].as_str().filter(|n| !n.is_empty()) {
            let next = sam_id(IdKind::Section, section_of.len() as u64 + 1);
            let material = member.material;
            let id = *section_of.entry(name.to_string()).or_insert_with(|| {
                let shape = bim_library::profile::steel(name).map_or(Shape::Unknown, |p| p.shape.structural());
                let (props, props_source) = section::compute(&shape, None);
                model.sections.push(Section { id: next, name: name.to_string(), shape, props, props_source, imported: None, material });
                next
            });
            member.section = Some((id, None));
        }
    }
}

/// The grid lines and levels of `project`, the lines over the extent of the nodes of `model`, in
/// display coordinates (`origin` subtracted).
pub fn grid(model: &AnalysisModel, project: &serde_json::Value, origin: DVec3, into: &mut ProjectData) {
    let grid = &project["structuralGrid"];
    if grid["enabled"].as_bool() == Some(false) {
        return;
    }
    let (low, high) = model.nodes.iter().fold((DVec3::INFINITY, DVec3::NEG_INFINITY), |(lo, hi), n| (lo.min(n.p), hi.max(n.p)));
    if !low.cmple(high).all() {
        return;
    }
    let axes = |key: &str| -> Vec<(String, f64)> {
        grid[key].as_array().into_iter().flatten().filter_map(|a| Some((a["label"].as_str().or(a["id"].as_str())?.to_string(), a["position"].as_f64()? / 1000.0))).collect()
    };
    let at = |p: DVec3| (p - origin).as_vec3().to_array();
    let past = |extent: f64| (extent * GRID_PAST.0).max(GRID_PAST.1);
    let (px, py) = (past(high.x - low.x), past(high.y - low.y));
    for (name, x) in axes("xAxes") {
        into.grids.push(GridLine { name, a: at(DVec3::new(x, low.y - py, low.z)), b: at(DVec3::new(x, high.y + py, low.z)) });
    }
    for (name, y) in axes("yAxes") {
        into.grids.push(GridLine { name, a: at(DVec3::new(low.x - px, y, low.z)), b: at(DVec3::new(high.x + px, y, low.z)) });
    }
    let mut levels: Vec<(String, f32)> = axes("zAxes").into_iter().map(|(name, z)| (name, (z - origin.z) as f32)).collect();
    levels.sort_by(|a, b| a.1.total_cmp(&b.1));
    into.levels = levels;
}

/// The members with a profile as solids, a layer per profile, in display coordinates.
pub fn solids(model: &AnalysisModel, origin: DVec3) -> Vec<Layer> {
    let mut layers: BTreeMap<String, Layer> = BTreeMap::new();
    for member in &model.members {
        let Some(section) = member.section.and_then(|(id, _)| model.section(id)) else { continue };
        let Ok(outline) = section::outline(&section.shape, CHORD, 12) else { continue };
        let Some((a, b)) = model.member_ends(member) else { continue };
        let Ok(f) = frame::member_frame(a, b, member.z_ref).or_else(|_| frame::member_frame(a, b, frame::default_z_ref(b - a))) else { continue };
        let layer = layers.entry(section.name.clone()).or_insert_with(|| Layer { name: section.name.clone(), color: STEEL, ..Layer::default() });
        for (corners, normal) in crate::structural::solid::sweep(&outline.loops(), &f) {
            let first = layer.vertex_count() as u32;
            for c in corners {
                layer.positions.extend((c - origin).as_vec3().to_array());
                layer.normals.extend(normal.as_vec3().to_array());
            }
            layer.indices.extend([first, first + 1, first + 2]);
        }
    }
    layers.into_values().filter(|l| !l.indices.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A column of 3 m up from the origin with number 4, and a project that gives it a profile.
    fn column() -> (AnalysisModel, serde_json::Value) {
        let mut m = AnalysisModel::new(1, "kolom", SourceInfo::default());
        let node = |k: u64, z: f64| Node { id: sam_id(IdKind::Node, k), number: k.to_string(), p: DVec3::new(0.0, 0.0, z), frame: None, support: None, origin: NodeOrigin::Analytical };
        m.nodes = vec![node(1, 0.0), node(2, 3.0)];
        m.members.push(Member { id: sam_id(IdKind::Member, 1), number: "4".into(), kind: MemberKind::Beam, nodes: vec![m.nodes[0].id, m.nodes[1].id], geometry: Curve::Line, z_ref: DVec3::Y, orientation: Orientation::IfcAxis, section: None, section_rotation: 0.0, insertion: Insertion::default(), releases: [None, None], material: None, links: Vec::new() });
        m.rebuild_index();
        let project = serde_json::json!({
            "model3d": { "beams": [{ "id": 4, "profileName": "HEA 300", "material": "S355" }, { "id": 9, "profileName": "IPE 200" }] },
            "structuralGrid": { "enabled": true, "xAxes": [{ "id": "a", "label": "1", "position": 0 }, { "id": "b", "label": "2", "position": 6000 }], "yAxes": [{ "id": "c", "label": "A", "position": 500 }], "zAxes": [{ "id": "n", "label": "Nok", "position": 7000 }, { "id": "p", "label": "Peil 0", "position": 0 }] },
        });
        (m, project)
    }

    #[test]
    fn a_member_takes_its_profile_and_material_from_the_project() {
        let (mut model, project) = column();
        sections(&mut model, &project);
        model.rebuild_index();
        // the beam of the project that the model does not have gives no section
        assert_eq!((model.sections.len(), model.materials.len()), (1, 1));
        let section = &model.sections[0];
        assert_eq!(section.name, "HEA 300");
        assert!(matches!(section.shape, Shape::I { h, b, .. } if (h - 0.29).abs() < 1e-9 && (b - 0.3).abs() < 1e-9), "{:?}", section.shape);
        assert_eq!((model.members[0].section, model.members[0].material, model.materials[0].name.as_str()), (Some((section.id, None)), Some(model.materials[0].id), "S355"));
    }

    #[test]
    fn the_profile_is_a_solid_along_the_member_with_its_depth_along_local_z() {
        let (mut model, project) = column();
        sections(&mut model, &project);
        model.rebuild_index();
        let layers = solids(&model, DVec3::new(0.0, 0.0, 1.0));
        assert_eq!(layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["HEA 300"]);
        let layer = &layers[0];
        assert!(layer.triangle_count() > 20 && layer.positions.len() == layer.normals.len());
        let (mut low, mut high) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in layer.positions.chunks_exact(3) {
            for k in 0..3 {
                low[k] = low[k].min(p[k]);
                high[k] = high[k].max(p[k]);
            }
        }
        // local z is +y: the depth of 290 mm lies along y, the flange of 300 mm along x; the
        // member runs from z = 0 to 3 m, less the origin's metre
        let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
        assert!(near(high[1] - low[1], 0.29) && near(high[0] - low[0], 0.3), "{low:?} {high:?}");
        assert!(near(low[2], -1.0) && near(high[2], 2.0), "{low:?} {high:?}");
    }

    #[test]
    fn the_grid_runs_past_the_nodes_and_the_levels_are_sorted() {
        let (model, project) = column();
        let mut data = ProjectData::default();
        grid(&model, &project, DVec3::ZERO, &mut data);
        assert_eq!(data.grids.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), ["1", "2", "A"]);
        // line 2 lies at x = 6 m and runs a metre past the nodes along y; A lies at y = 0.5 m
        assert_eq!((data.grids[1].a, data.grids[1].b), ([6.0, -1.0, 0.0], [6.0, 1.0, 0.0]));
        assert_eq!((data.grids[2].a[1], data.grids[2].b[1]), (0.5, 0.5));
        assert_eq!(data.levels, [("Peil 0".to_string(), 0.0), ("Nok".to_string(), 7.0)]);
    }
}
