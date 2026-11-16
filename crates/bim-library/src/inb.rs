//! The types of the INB template as a library: the Dutch wall, floor, roof, column and beam
//! types a project can add to its own.
//!
//! The catalogue (`data/inb-catalogue.json`) was extracted from the INB template of the OpenAEC
//! Foundation (`INB-Template 0.6.ifc`, see the file's `source`); it is compiled in, so nothing is
//! read at run time. It holds every wall and slab type of the template with a material layer
//! set, each a single layer, and the column and beam types of a solid rectangular or round
//! profile that is not steel (steel follows the profile catalogue).
//!
//! - A type keeps the name the template gives it (`kzst_100`, `kpv_200`, `BK 300x300`).
//! - Its id is `inb.<role>.<name>`, so the types of the catalogue are told from a project's own.
//! - The catalogue's materials are the starter materials where those have them, and nine of its
//!   own (CLT, glass wool, PUR, EPS, porous brick, precast and unreinforced concrete among them).

use crate::buildup::{BuildUp, Layer, Role};
use crate::library::Library;
use crate::material::{self, HatchKind, Material, Names};
use crate::profile::{LinearRole, LinearType, ProfileShape};
use serde_json::Value;

/// The catalogue as extracted from the template.
const CATALOGUE: &str = include_str!("../data/inb-catalogue.json");

/// The prefix of the id of every type of the catalogue.
pub const PREFIX: &str = "inb.";

/// The library material (its key) of each material the catalogue names.
const MATERIALS: [(&str, &str); 16] = [
    ("concrete", "concrete"),
    ("precast concrete", "precast-concrete"),
    ("unreinforced concrete", "unreinforced-concrete"),
    ("brick", "brick"),
    ("brick porous", "brick-porous"),
    ("sand-lime", "sand-lime"),
    ("gypsum board", "gypsum-board"),
    ("insulation", "insulation"),
    ("insulation PIR", "insulation-pir"),
    ("insulation PUR", "insulation-pur"),
    ("insulation EPS", "insulation-eps"),
    ("insulation glass wool", "glass-wool"),
    ("timber", "timber"),
    ("timber CLT", "timber-clt"),
    ("timber plywood", "plywood"),
    ("generic", "generic"),
];

/// The materials of the catalogue that the starter library lacks.
fn own_materials() -> Vec<Material> {
    use HatchKind::*;
    let m = |key: &str, nl: &str, en: &str, de: &str, fr: &str, hatch, colour| Material { key: key.into(), names: Names::new(nl, en, de, fr), hatch, colour };
    vec![
        m("precast-concrete", "prefab beton", "precast concrete", "Betonfertigteil", "béton préfabriqué", Concrete, [175, 175, 175]),
        m("unreinforced-concrete", "ongewapend beton", "unreinforced concrete", "unbewehrter Beton", "béton non armé", Concrete, [195, 195, 190]),
        m("brick-porous", "poriso", "porous brick", "Porenziegel", "brique alvéolée", Brick, [200, 120, 95]),
        m("insulation-pur", "isolatie PUR", "PUR insulation", "PUR-Dämmung", "isolant PUR", Insulation, [240, 225, 150]),
        m("insulation-eps", "isolatie EPS", "EPS insulation", "EPS-Dämmung", "isolant PSE", Insulation, [245, 240, 215]),
        m("glass-wool", "glaswol", "glass wool", "Glaswolle", "laine de verre", Insulation, [240, 215, 110]),
        m("timber-clt", "CLT", "CLT", "Brettsperrholz", "CLT", Timber, [215, 180, 130]),
    ]
}

/// A name as part of an id: lower case, letters, digits and points, hyphens between.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() || c == '.' || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// The library of the INB template: its materials (the starter's and its own) and its types. A
/// type of the catalogue whose material is unknown, or that does not check, is left out.
pub fn library() -> Library {
    let mut library = Library { materials: material::starter(), ..Library::default() };
    for m in own_materials() {
        let _ = library.add_material(m);
    }
    let catalogue: Value = serde_json::from_str(CATALOGUE).unwrap_or(Value::Null);
    let key = |material: Option<&Value>| MATERIALS.iter().find(|(name, _)| Some(*name) == material.and_then(Value::as_str)).map(|(_, key)| *key);
    for t in catalogue["types"].as_array().into_iter().flatten() {
        let (Some(name), Some(role)) = (t["name"].as_str(), t["role"].as_str()) else { continue };
        let id = format!("{PREFIX}{role}.{}", slug(name));
        match role {
            "wall" | "floor" | "roof" => {
                let role = match role {
                    "wall" => Role::Wall,
                    "floor" => Role::Floor,
                    _ => Role::Roof,
                };
                let layers: Option<Vec<Layer>> = t["layers"].as_array().into_iter().flatten().map(|l| Some(Layer::new(key(l.get("material"))?, l["thickness_m"].as_f64()?))).collect();
                if let Some(layers) = layers.filter(|l| !l.is_empty()) {
                    let _ = library.add_buildup(BuildUp { name: Some(name.to_string()), ..BuildUp::new(&id, role, layers) });
                }
            }
            "column" | "beam" => {
                let role = if role == "column" { LinearRole::Column } else { LinearRole::Beam };
                let p = &t["profile"];
                let (Some(material), Some(width), Some(depth)) = (key(p.get("material")), p["width_m"].as_f64(), p["depth_m"].as_f64()) else { continue };
                let profile = if p["round"].as_bool() == Some(true) { ProfileShape::Circle { diameter: width } } else { ProfileShape::Rectangle { width, depth } };
                let _ = library.add_linear_type(LinearType { name: Some(name.to_string()), ..LinearType::new(&id, role, material, profile) });
            }
            _ => {}
        }
    }
    library
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Lang;

    #[test]
    fn the_catalogue_gives_the_templates_types_under_their_own_names() {
        let inb = library();
        let count = |role| inb.buildups_of(role).count();
        // 39 walls with layers (the movable wall has none), 31 floors, 2 roofs, 7 columns, 7 beams
        assert_eq!((count(Role::Wall), count(Role::Floor), count(Role::Roof)), (39, 31, 2));
        assert_eq!((inb.linear_types_of(LinearRole::Column).count(), inb.linear_types_of(LinearRole::Beam).count()), (7, 7));
        assert!(inb.buildups.iter().all(|b| b.id.starts_with(PREFIX)) && inb.linear_types.iter().all(|t| t.id.starts_with(PREFIX)));
        let wall = inb.buildup("inb.wall.kzst_100").expect("a sand-lime wall of 100");
        assert_eq!((wall.layers.len(), wall.layers[0].material.as_str(), wall.layers[0].thickness), (1, "sand-lime", 0.1));
        assert_eq!(inb.name("inb.wall.kzst_100", Lang::Nl).as_deref(), Some("kzst_100"));
        let column = inb.linear_type("inb.column.bk-300x300").expect("a concrete column");
        assert_eq!((column.material.as_str(), column.profile.clone()), ("concrete", ProfileShape::Rectangle { width: 0.3, depth: 0.3 }));
        assert_eq!(inb.linear_name("inb.column.bk-300x300", Lang::En).as_deref(), Some("BK 300x300"));
        // the same name as a wall and as a floor: a type of each
        assert!(inb.buildup("inb.wall.clt_100").is_some() && inb.buildup("inb.floor.clt_100").is_some());
    }

    #[test]
    fn a_project_takes_the_catalogue_once_beside_its_own_types() {
        let mut project = Library::starter();
        let before = (project.materials.len(), project.buildups.len(), project.linear_types.len());
        let added = project.merge(&library());
        // seven materials, 72 planar types and 14 linear types, whatever the starter has like them
        assert_eq!(added, 7 + 72 + 14);
        assert_eq!((project.materials.len(), project.buildups.len(), project.linear_types.len()), (before.0 + 7, before.1 + 72, before.2 + 14));
        assert_eq!(project.name("inb.floor.kpv_200", Lang::En).as_deref(), Some("kpv_200"));
        // a second time adds nothing
        assert_eq!(project.merge(&library()), 0);
    }
}
