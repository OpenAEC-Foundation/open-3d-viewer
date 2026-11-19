//! Usual Dutch build-ups: the layered walls, floors and roofs a Dutch house or a small utility
//! building is commonly made of, for the Dutch library beside the single layers of the INB
//! template and of Ocondat.
//!
//! These are the suite's own examples, not taken from a source: constructions every Dutch
//! builder knows (a cavity wall of brick and sand-lime brick, a timber-frame facade, a
//! hollow-core floor with a screed, a warm flat roof), with layer thicknesses as they are
//! commonly built now. They are a start to change, not a specification: the thermal
//! resistance, the fire and sound performance and the structure of a build-up are the
//! designer's to check.
//!
//! - A build-up is named by the rule (its layers and its thickness), in every language.
//! - Its id is `nl.<role>.<what it is>`.
//! - Its NL-SfB code is the rule's, but for the internal walls the rule takes for external
//!   ones: a party wall of two leaves with a cavity, and partitions whose studs are filled
//!   with insulation.
//! - Its materials are the starter library's and the INB template's.

use crate::buildup::{BuildUp, Layer, Role};

/// The build-ups, walls first.
pub fn buildups() -> Vec<BuildUp> {
    use Role::*;
    let b = |id: &str, role, code: Option<&str>, layers: &[(&str, f64)]| BuildUp { code: code.map(str::to_string), ..BuildUp::new(&format!("nl.{id}"), role, layers.iter().map(|&(material, millimetres)| Layer::new(material, millimetres / 1000.0)).collect()) };
    vec![
        // external walls: cavity walls of brick, from the exterior in
        b("wall.cavity-mineral-wool-sand-lime-100", Wall, None, &[("brick", 100.0), ("air", 40.0), ("mineral-wool", 140.0), ("sand-lime", 100.0)]),
        b("wall.cavity-mineral-wool-sand-lime-120", Wall, None, &[("brick", 100.0), ("air", 40.0), ("mineral-wool", 140.0), ("sand-lime", 120.0)]),
        b("wall.cavity-mineral-wool-sand-lime-150", Wall, None, &[("brick", 100.0), ("air", 40.0), ("mineral-wool", 140.0), ("sand-lime", 150.0)]),
        b("wall.cavity-pir-sand-lime-100", Wall, None, &[("brick", 100.0), ("air", 40.0), ("insulation-pir", 120.0), ("sand-lime", 100.0)]),
        b("wall.cavity-pir-concrete-200", Wall, None, &[("brick", 100.0), ("air", 40.0), ("insulation-pir", 120.0), ("concrete", 200.0)]),
        b("wall.cavity-eps-porous-brick-100", Wall, None, &[("brick", 100.0), ("air", 40.0), ("insulation-eps", 140.0), ("brick-porous", 100.0)]),
        // external walls: timber frame, with cladding or with a brick outer leaf
        b("wall.timber-frame-cladding-184", Wall, None, &[("timber-cladding", 20.0), ("air", 28.0), ("breather-membrane", 0.5), ("timber-frame-insulated", 184.0), ("vapour-control-membrane", 0.2), ("gypsum-board", 12.5)]),
        b("wall.timber-frame-cladding-235", Wall, None, &[("timber-cladding", 20.0), ("air", 28.0), ("breather-membrane", 0.5), ("timber-frame-insulated", 235.0), ("vapour-control-membrane", 0.2), ("gypsum-board", 12.5)]),
        b("wall.timber-frame-brick-184", Wall, None, &[("brick", 100.0), ("air", 40.0), ("breather-membrane", 0.5), ("osb", 12.0), ("timber-frame-insulated", 184.0), ("vapour-control-membrane", 0.2), ("gypsum-board", 12.5)]),
        b("wall.clt-cladding-100", Wall, None, &[("timber-cladding", 20.0), ("air", 28.0), ("breather-membrane", 0.5), ("mineral-wool", 160.0), ("timber-clt", 100.0)]),
        // internal walls: a party wall of two leaves, and partitions
        b("wall.party-sand-lime-120-cavity-60", Wall, Some("22"), &[("sand-lime", 120.0), ("air", 60.0), ("sand-lime", 120.0)]),
        b("wall.metal-stud-100", Wall, Some("22"), &[("gypsum-board", 12.5), ("metal-stud-insulated", 75.0), ("gypsum-board", 12.5)]),
        b("wall.metal-stud-125-double-board", Wall, Some("22"), &[("gypsum-board", 12.5), ("gypsum-board", 12.5), ("metal-stud-insulated", 75.0), ("gypsum-board", 12.5), ("gypsum-board", 12.5)]),
        b("wall.metal-stud-150-double-board", Wall, Some("22"), &[("gypsum-board", 12.5), ("gypsum-board", 12.5), ("metal-stud-insulated", 100.0), ("gypsum-board", 12.5), ("gypsum-board", 12.5)]),
        b("wall.timber-stud-114", Wall, Some("22"), &[("gypsum-board", 12.5), ("timber-frame-insulated", 89.0), ("gypsum-board", 12.5)]),
        // floors, from the top down: on the ground with insulation under the slab, and storeys
        b("floor.ground-hollow-core-200-eps", Floor, None, &[("screed", 70.0), ("hollow-core-concrete", 200.0), ("insulation-eps", 150.0)]),
        b("floor.ground-concrete-200-eps", Floor, None, &[("screed", 70.0), ("concrete", 200.0), ("insulation-eps", 150.0)]),
        b("floor.ground-screed-pir-concrete", Floor, None, &[("screed", 70.0), ("insulation-pir", 100.0), ("concrete", 200.0)]),
        b("floor.storey-hollow-core-200", Floor, None, &[("screed", 70.0), ("hollow-core-concrete", 200.0)]),
        b("floor.storey-hollow-core-260", Floor, None, &[("screed", 70.0), ("hollow-core-concrete", 260.0)]),
        b("floor.storey-concrete-260", Floor, None, &[("screed", 70.0), ("concrete", 260.0)]),
        b("floor.storey-timber-joists-196", Floor, None, &[("plywood", 18.0), ("timber", 196.0), ("gypsum-board", 12.5)]),
        b("floor.storey-clt-160", Floor, None, &[("screed", 50.0), ("mineral-wool", 30.0), ("timber-clt", 160.0)]),
        // roofs, from the top down: warm flat roofs and an insulated pitched roof
        b("roof.flat-hollow-core-200-pir", Roof, None, &[("roofing", 10.0), ("insulation-pir", 160.0), ("vapour-control-membrane", 0.2), ("hollow-core-concrete", 200.0)]),
        b("roof.flat-concrete-200-pir", Roof, None, &[("roofing", 10.0), ("insulation-pir", 160.0), ("vapour-control-membrane", 0.2), ("concrete", 200.0)]),
        b("roof.flat-timber-joists-221-pir", Roof, None, &[("roofing", 10.0), ("insulation-pir", 160.0), ("vapour-control-membrane", 0.2), ("plywood", 18.0), ("timber", 221.0), ("gypsum-board", 12.5)]),
        b("roof.pitched-timber-insulated-235", Roof, None, &[("breather-membrane", 0.5), ("timber-frame-insulated", 235.0), ("vapour-control-membrane", 0.2), ("gypsum-board", 12.5)]),
        b("roof.flat-clt-160-pir", Roof, None, &[("roofing", 10.0), ("insulation-pir", 160.0), ("vapour-control-membrane", 0.2), ("timber-clt", 160.0)]),
    ]
}

/// The Dutch library: the types of the INB template, with them the materials and the types of
/// one material of Project Ocondat that the template lacks, and the usual Dutch layered
/// build-ups.
pub fn library() -> crate::Library {
    let mut library = crate::inb::library();
    library.merge(&crate::ocondat::library());
    let layered = crate::Library { materials: library.materials.clone(), buildups: buildups(), ..crate::Library::default() };
    library.merge(&layered);
    library
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Lang, Library};
    use std::collections::HashSet;

    #[test]
    fn every_build_up_checks_against_the_templates_materials_and_has_a_code() {
        let mut dutch = crate::inb::library();
        let all = buildups();
        let ids: HashSet<&str> = all.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids.len(), all.len(), "ids are unique");
        for b in &all {
            assert!(b.id.starts_with("nl.") && b.layers.len() > 1, "{}", b.id);
            dutch.add_buildup(b.clone()).unwrap_or_else(|e| panic!("{}: {e}", b.id));
            assert!(crate::nlsfb::valid(&b.nlsfb(&dutch.materials)), "{}", b.id);
        }
        // their names by rule are told apart in each role
        for role in Role::ALL {
            let names: Vec<String> = all.iter().filter(|b| b.role == role).map(|b| b.rule_name(&dutch.materials, Lang::Nl)).collect();
            assert_eq!(names.iter().collect::<HashSet<_>>().len(), names.len(), "{role:?}: {names:?}");
        }
    }

    #[test]
    fn a_cavity_wall_a_party_wall_and_a_ground_floor() {
        let dutch = {
            let mut library = crate::inb::library();
            library.merge(&Library { buildups: buildups(), materials: library.materials.clone(), ..Library::default() });
            library
        };
        let of = |id: &str| dutch.buildup(id).unwrap_or_else(|| panic!("{id}"));
        let cavity = of("nl.wall.cavity-mineral-wool-sand-lime-100");
        assert_eq!(dutch.name(&cavity.id, Lang::Nl).as_deref(), Some("Metselwerk-spouw-minerale wol-kalkzandsteen 380"));
        assert_eq!((cavity.thickness(), cavity.nlsfb(&dutch.materials).as_str()), (0.38, "21"));
        // a party wall has a cavity and is an internal wall all the same; a partition too
        let party = of("nl.wall.party-sand-lime-120-cavity-60");
        assert_eq!((party.rule_code(&dutch.materials), party.nlsfb(&dutch.materials).as_str(), party.thickness()), ("21", "22", 0.3));
        assert_eq!(of("nl.wall.metal-stud-125-double-board").nlsfb(&dutch.materials), "22");
        let ground = of("nl.floor.ground-hollow-core-200-eps");
        assert_eq!((dutch.name(&ground.id, Lang::En).as_deref(), ground.nlsfb(&dutch.materials).as_str()), (Some("Screed-hollow-core slab-EPS insulation 420"), "23"));
        assert_eq!(of("nl.roof.flat-hollow-core-200-pir").nlsfb(&dutch.materials), "27");
    }
}
