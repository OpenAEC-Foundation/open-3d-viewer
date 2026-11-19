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
//! The steel profiles of Ocondat stand in the steel table ([`crate::profile`]).

use crate::material::Material;
use serde::Deserialize;
use std::sync::OnceLock;

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
    use crate::{Lang, Library};
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
