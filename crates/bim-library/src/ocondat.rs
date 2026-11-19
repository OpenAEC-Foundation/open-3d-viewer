//! The materials of Project Ocondat, the OpenAEC Foundation's open building data: the Dutch
//! materials a project can add to its own, each with its colour and its own hatch.
//!
//! The catalogue (`data/ocondat-materials.json`, see the file's `source` and `licence`) was
//! extracted from the sheet `materials` of Ocondat's `BlenderBIM/Library.xlsx` by
//! `tools/ocondat-materials/extract.py`; it is compiled in, so nothing is read at run time.
//!
//! - Concrete, steel and reinforcing steel with their grades, the metals, timber and its
//!   species, boards, masonry, mortars, insulation, natural stone, ground and glass.
//! - A material of one kind the hatch catalogue has a hatch for (a species of wood, a kind of
//!   board, of insulation, of stone, of ground) names that hatch ([`Material::sign`]) and is
//!   called as the hatch is; another names the suite's hatch of its sign of NEN 47 where the
//!   catalogue has it. Solid fills (concrete cast in situ, steel, the other metals, bitumen)
//!   are no hatch of the catalogue: such a material is drawn by its hatch kind.
//! - A material the starter library has keeps that library's key (`concrete`, `steel`,
//!   `insulation-pir`), so it is the same material and a library has it once.
//!
//! Its types of one material (`data/ocondat-types.json`, from the same workbook) make a library
//! with those materials ([`library`]):
//!
//! - Beams and columns of a rectangular or round profile: the Dutch timber sizes in spruce
//!   ("HB 71x196", "SLS 38x140"), concrete beams and columns ("BB 300x400") and generic ones.
//!   A profile the sheet makes a beam and a column of is a type of each.
//! - Walls and floors of a single layer: masonry, sand-lime brick, concrete cast in situ and
//!   precast, hollow-core slabs, CLT, insulation, boards and screeds, by their thickness.
//! - A type keeps the name the sheet gives it (`kzst_100`, `kpv_200`, `HB 71x196`); its id is
//!   `ocondat.<role>.<name>`. Its NL-SfB code is the rule's ([`crate::nlsfb`]).
//!
//! Much of this is the INB template's, which was made from the same workbook: a library that
//! has a type of the template does not take Ocondat's twin of it ([`Library::merge`]).
//!
//! The steel profiles of Ocondat stand in the steel table ([`crate::profile`]).

use crate::buildup::{BuildUp, Layer, Role};
use crate::library::Library;
use crate::material::Material;
use crate::profile::{LinearRole, LinearType, ProfileShape};
use serde::Deserialize;
use std::sync::OnceLock;

/// The types of one material as extracted from Ocondat.
const TYPES: &str = include_str!("../data/ocondat-types.json");

/// What every id of the catalogue starts with.
pub const PREFIX: &str = "ocondat.";

#[derive(Deserialize)]
struct Types {
    linear: Vec<Linear>,
    planar: Vec<Planar>,
}

#[derive(Deserialize)]
struct Linear {
    name: String,
    roles: Vec<String>,
    material: String,
    #[serde(default)]
    width: f64,
    #[serde(default)]
    depth: f64,
    diameter: Option<f64>,
}

#[derive(Deserialize)]
struct Planar {
    name: String,
    role: String,
    material: String,
    thickness: f64,
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

/// The library of Ocondat: its materials and its types of one material. A type that does not
/// check is left out.
pub fn library() -> Library {
    let mut library = Library { materials: materials().to_vec(), codes: crate::nlsfb::starter(), ..Library::default() };
    let types: Types = serde_json::from_str(TYPES).expect("the types of Ocondat are compiled in");
    for t in &types.planar {
        let role = if t.role == "wall" { Role::Wall } else { Role::Floor };
        let id = format!("{PREFIX}{}.{}", t.role, slug(&t.name));
        let _ = library.add_buildup(BuildUp { name: Some(t.name.clone()), ..BuildUp::new(&id, role, vec![Layer::new(&t.material, t.thickness / 1000.0)]) });
    }
    for t in &types.linear {
        let profile = match t.diameter {
            Some(diameter) => ProfileShape::Circle { diameter: diameter / 1000.0 },
            None => ProfileShape::Rectangle { width: t.width / 1000.0, depth: t.depth / 1000.0 },
        };
        for role in &t.roles {
            let of = if role == "column" { LinearRole::Column } else { LinearRole::Beam };
            let id = format!("{PREFIX}{role}.{}", slug(&t.name));
            let _ = library.add_linear_type(LinearType { name: Some(t.name.clone()), ..LinearType::new(&id, of, &t.material, profile.clone()) });
        }
    }
    library
}

/// The catalogue as extracted from Ocondat.
const CATALOGUE: &str = include_str!("../data/ocondat-materials.json");

#[derive(Deserialize)]
struct File {
    materials: Vec<Material>,
}

/// Every material of Ocondat, in the order of its sheet.
pub fn materials() -> &'static [Material] {
    static MATERIALS: OnceLock<Vec<Material>> = OnceLock::new();
    MATERIALS.get_or_init(|| serde_json::from_str::<File>(CATALOGUE).expect("the materials of Ocondat are compiled in").materials)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material::{self, HatchKind};
    use crate::Lang;
    use std::collections::HashSet;

    #[test]
    fn every_material_has_a_key_its_names_and_a_hatch_the_catalogue_has() {
        let materials = materials();
        assert_eq!(materials.len(), 109);
        let keys: HashSet<&str> = materials.iter().map(|m| m.key.as_str()).collect();
        assert_eq!(keys.len(), materials.len(), "keys are unique");
        for m in materials {
            assert!(m.key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') && !m.key.is_empty(), "{}", m.key);
            for lang in Lang::ALL {
                assert!(!m.name(lang).is_empty(), "{} has a name in {lang:?}", m.key);
            }
            if let Some(sign) = &m.sign {
                assert!(m.catalogue_hatch().is_some(), "{}: the catalogue has no hatch {sign}", m.key);
            }
        }
        assert_eq!(materials.iter().filter(|m| m.sign.is_some()).count(), 69);
    }

    #[test]
    fn a_material_names_the_hatch_of_its_kind() {
        let of = |key: &str| materials().iter().find(|m| m.key == key).unwrap_or_else(|| panic!("{key}"));
        // a species of wood: the softwood sign in its own colour, and called as the hatch is
        let spruce = of("spruce");
        assert_eq!((spruce.sign.as_deref(), spruce.hatch, spruce.colour), (Some("openaec-12-spruce"), HatchKind::Timber, [240, 220, 185]));
        assert_eq!((spruce.name(Lang::Nl), spruce.name(Lang::En), spruce.catalogue_hatch().map(|h| h.sign)), ("vuren", "spruce", Some(12)));
        // a grade of it: the same hatch, the grade in its name
        assert_eq!((of("spruce-c18").name(Lang::Nl), of("spruce-c18").sign.as_deref()), ("vuren C18", Some("openaec-12-spruce")));
        // a kind of insulation, of stone and of ground
        for (key, sign) in [("stone-wool", "openaec-17-stone-wool"), ("bluestone", "openaec-9-bluestone"), ("peat", "openaec-25-peat"), ("sand", "openaec-26"), ("gravel", "openaec-27"), ("precast-concrete", "openaec-6")] {
            assert_eq!(of(key).sign.as_deref(), Some(sign), "{key}");
        }
        // solid fills are no hatch of the catalogue: the hatch kind draws them
        for (key, kind) in [("concrete", HatchKind::Concrete), ("steel-s355", HatchKind::Steel), ("reinforcing-steel-b500", HatchKind::Steel), ("copper", HatchKind::Generic), ("gypsum", HatchKind::Gypsum)] {
            assert_eq!((of(key).sign.as_deref(), of(key).hatch), (None, kind), "{key}");
        }
        assert_eq!(of("concrete-c20-25").name(Lang::De), "Beton C20/25");
    }

    #[test]
    fn the_types_of_one_material_are_a_library() {
        let ocondat = library();
        assert_eq!(ocondat.materials.len(), materials().len());
        // every wall and floor of the sheet, and a type per role of every profile
        assert_eq!((ocondat.buildups_of(Role::Wall).count(), ocondat.buildups_of(Role::Floor).count()), (36, 33));
        assert_eq!((ocondat.linear_types_of(LinearRole::Beam).count(), ocondat.linear_types_of(LinearRole::Column).count()), (37, 17));
        // Dutch timber: a joist of 71 by 196 in spruce, lying on its narrow side
        let joist = ocondat.linear_type("ocondat.beam.hb-71x196").expect("the joist");
        assert_eq!((joist.material.as_str(), &joist.profile, joist.name.as_deref()), ("spruce", &ProfileShape::Rectangle { width: 0.071, depth: 0.196 }, Some("HB 71x196")));
        assert_eq!((ocondat.linear_name(&joist.id, Lang::Nl).as_deref(), joist.nlsfb().as_str()), (Some("HB 71x196"), "28"));
        // the sheet has SLS 38x140 as 38 by 38: its name is followed
        assert_eq!(ocondat.linear_type("ocondat.beam.sls-38x140").map(|t| &t.profile), Some(&ProfileShape::Rectangle { width: 0.038, depth: 0.14 }));
        // a concrete column and beam of one profile, a round generic column
        assert!(ocondat.linear_type("ocondat.column.bb-300x300").is_some() && ocondat.linear_type("ocondat.beam.bb-300x300").is_some());
        assert_eq!(ocondat.linear_type("ocondat.column.gen-r150").map(|t| &t.profile), Some(&ProfileShape::Circle { diameter: 0.15 }));
        // walls and floors of a single layer, coded by rule
        let wall = ocondat.buildup("ocondat.wall.kzst_100").expect("a sand-lime wall");
        assert_eq!((wall.layers.len(), wall.layers[0].material.as_str(), wall.thickness(), wall.nlsfb(&ocondat.materials).as_str()), (1, "sand-lime", 0.1, "22"));
        let slab = ocondat.buildup("ocondat.floor.kpv_200").expect("a hollow-core slab");
        assert_eq!((slab.layers[0].material.as_str(), slab.thickness(), slab.nlsfb(&ocondat.materials).as_str()), ("precast-concrete", 0.2, "23"));
        assert_eq!(ocondat.buildup("ocondat.wall.isolatie_pir_100").unwrap().nlsfb(&ocondat.materials), "21");
        // each checks against the materials of Ocondat alone
        let mut fresh = Library { materials: ocondat.materials.clone(), ..Library::default() };
        for b in &ocondat.buildups {
            fresh.add_buildup(b.clone()).unwrap_or_else(|e| panic!("{}: {e}", b.id));
        }
        for t in &ocondat.linear_types {
            fresh.add_linear_type(t.clone()).unwrap_or_else(|e| panic!("{}: {e}", t.id));
        }
    }

    #[test]
    fn a_library_takes_them_beside_its_own() {
        let mut library = Library::starter();
        let own = library.materials.len();
        let shared = materials().iter().filter(|m| library.material(&m.key).is_some()).count();
        assert!(shared >= 10, "the starter library and Ocondat share materials by key: {shared}");
        let added = library.merge(&Library { materials: materials().to_vec(), ..Library::default() });
        assert_eq!((added, library.materials.len()), (materials().len() - shared, own + materials().len() - shared));
        // what the library had stays as it was
        assert_eq!(library.material("concrete"), material::starter().iter().find(|m| m.key == "concrete"));
    }
}

#[cfg(test)]
mod dutch_tests {
    use crate::{LinearRole, Role};

    #[test]
    fn the_template_keeps_its_types_and_takes_what_it_lacks() {
        let template = crate::inb::library();
        let mut dutch = template.clone();
        let added = dutch.merge(&super::library());
        let count = |l: &crate::Library| (l.buildups_of(Role::Wall).count(), l.buildups_of(Role::Floor).count(), l.linear_types_of(LinearRole::Beam).count(), l.linear_types_of(LinearRole::Column).count(), l.materials.len());
        // the template was made from the same workbook: most walls and floors are its own, the
        // timber sizes, the concrete sections and the graded materials are new to it
        assert_eq!((count(&template), count(&dutch), added), ((39, 31, 7, 7, 37), (48, 40, 42, 24, 126), 159));
        // nothing of the template changed, and it has no twin of a type it had
        for b in &template.buildups {
            assert_eq!(dutch.buildup(&b.id), Some(b));
        }
        assert!(dutch.linear_type("ocondat.beam.hb-71x196").is_some(), "the timber sizes are new to it");
        assert!(added > 100);
    }
}
