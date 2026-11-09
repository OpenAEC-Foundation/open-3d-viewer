//! Layered build-ups: the planar types of walls, floors, roofs and ceilings, as in Open BIM
//! Studio. A build-up is a role and its layers, each a material and a thickness. A wall's layers
//! run from its exterior face inwards; a floor's, roof's or ceiling's from its top face down.

use crate::material::Material;
use crate::{millimetres, Lang, LibraryError, MIN_THICKNESS};
use serde::{Deserialize, Serialize};

/// What a planar type is, and so its IFC class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Wall,
    Floor,
    Roof,
    Ceiling,
}

impl Role {
    pub const ALL: [Role; 4] = [Role::Wall, Role::Floor, Role::Roof, Role::Ceiling];

    /// Its English name, as data (property values, file keys).
    pub fn name(self) -> &'static str {
        match self {
            Role::Wall => "Wall",
            Role::Floor => "Floor",
            Role::Roof => "Roof",
            Role::Ceiling => "Ceiling",
        }
    }

    /// The IFC class (IFC4.3) of its elements.
    pub fn ifc_class(self) -> &'static str {
        match self {
            Role::Wall => "IfcWall",
            Role::Floor | Role::Roof => "IfcSlab",
            Role::Ceiling => "IfcCovering",
        }
    }

    /// The IFC class of its type.
    pub fn ifc_type_class(self) -> &'static str {
        match self {
            Role::Wall => "IfcWallType",
            Role::Floor | Role::Roof => "IfcSlabType",
            Role::Ceiling => "IfcCoveringType",
        }
    }

    /// The IFC predefined type of its elements and its type.
    pub fn predefined_type(self) -> &'static str {
        match self {
            Role::Wall => "STANDARD",
            Role::Floor => "FLOOR",
            Role::Roof => "ROOF",
            Role::Ceiling => "CEILING",
        }
    }

    /// The face its layers are counted from: the exterior for a wall, the top otherwise.
    pub fn counted_from_top(self) -> bool {
        self != Role::Wall
    }
}

/// A layer: a material (its key) and a thickness in metres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub material: String,
    pub thickness: f64,
}

impl Layer {
    pub fn new(material: &str, thickness: f64) -> Self {
        Self { material: material.into(), thickness }
    }
}

/// A layered build-up: a planar type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildUp {
    /// Stable and never shown: what elements refer to.
    pub id: String,
    pub role: Role,
    /// From the exterior face in (wall) or from the top face down (floor, roof, ceiling).
    pub layers: Vec<Layer>,
}

impl BuildUp {
    pub fn new(id: &str, role: Role, layers: Vec<Layer>) -> Self {
        Self { id: id.into(), role, layers }
    }

    /// The total thickness.
    pub fn thickness(&self) -> f64 {
        self.layers.iter().map(|l| l.thickness).sum()
    }

    /// Each layer's two faces, measured from the face the layers are counted from.
    pub fn faces(&self) -> Vec<(f64, f64)> {
        let mut at = 0.0;
        self.layers
            .iter()
            .map(|l| {
                let from = at;
                at += l.thickness;
                (from, at)
            })
            .collect()
    }

    /// Checks it against `materials`: an id, at least one layer, every layer at least 0.1 mm
    /// thick and of a known material.
    pub fn check(&self, materials: &[Material]) -> Result<(), LibraryError> {
        if self.id.trim().is_empty() {
            return Err(LibraryError::NoKey);
        }
        if self.layers.is_empty() {
            return Err(LibraryError::NoLayers);
        }
        for layer in &self.layers {
            if !layer.thickness.is_finite() || layer.thickness < MIN_THICKNESS {
                return Err(LibraryError::NotPositive);
            }
            if !materials.iter().any(|m| m.key == layer.material) {
                return Err(LibraryError::UnknownMaterial(layer.material.clone()));
            }
        }
        Ok(())
    }

    /// Its name by Open BIM Studio's rule, in `lang`: the names of its layers' materials joined by
    /// hyphens (a material next to itself named once), the first letter a capital, then its
    /// thickness in millimetres. "Metselwerk-spouw-isolatie PIR-kalkzandsteen 360". A material
    /// missing from `materials` is named by its key.
    pub fn rule_name(&self, materials: &[Material], lang: Lang) -> String {
        let mut parts: Vec<&str> = Vec::new();
        for layer in &self.layers {
            let name = materials.iter().find(|m| m.key == layer.material).map_or(layer.material.as_str(), |m| m.name(lang));
            if parts.last() != Some(&name) {
                parts.push(name);
            }
        }
        let joined = parts.join("-");
        let mut chars = joined.chars();
        let capitalised: String = chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default();
        format!("{capitalised} {}", millimetres(self.thickness(), lang))
    }
}

/// The build-ups a library starts with: Open BIM Studio's starter planar types, in their layer
/// order and thicknesses.
pub fn starter() -> Vec<BuildUp> {
    let b = |id: &str, role, layers: &[(&str, f64)]| BuildUp::new(id, role, layers.iter().map(|&(m, t)| Layer::new(m, t)).collect());
    let one = |id: &str, role, material: &str, thickness| b(id, role, &[(material, thickness)]);
    use Role::*;
    vec![
        b("wall.cavity-pir-360", Wall, &[("brick", 0.1), ("air", 0.04), ("insulation-pir", 0.1), ("sand-lime", 0.12)]),
        b("wall.cavity-300", Wall, &[("brick", 0.1), ("air", 0.03), ("insulation", 0.05), ("sand-lime", 0.12)]),
        b("wall.timber-frame-outer", Wall, &[("timber-cladding", 0.02), ("air", 0.02), ("osb", 0.012), ("timber-frame-insulated", 0.14), ("gypsum-board", 0.0125)]),
        b("wall.metal-stud-75", Wall, &[("gypsum-board", 0.0125), ("metal-stud-insulated", 0.075), ("gypsum-board", 0.0125)]),
        one("wall.concrete-150", Wall, "concrete", 0.15),
        one("wall.concrete-200", Wall, "concrete", 0.2),
        one("wall.concrete-250", Wall, "concrete", 0.25),
        one("wall.concrete-300", Wall, "concrete", 0.3),
        one("wall.sand-lime-100", Wall, "sand-lime", 0.1),
        one("wall.sand-lime-150", Wall, "sand-lime", 0.15),
        one("wall.sand-lime-214", Wall, "sand-lime", 0.214),
        one("wall.brick-100", Wall, "brick", 0.1),
        one("wall.brick-210", Wall, "brick", 0.21),
        one("wall.timber-120", Wall, "timber", 0.12),
        one("wall.timber-170", Wall, "timber", 0.17),
        one("wall.steel-10", Wall, "steel", 0.01),
        one("wall.insulation-184", Wall, "insulation", 0.184),
        one("floor.concrete-150", Floor, "concrete", 0.15),
        one("floor.concrete-200", Floor, "concrete", 0.2),
        one("floor.concrete-250", Floor, "concrete", 0.25),
        one("floor.concrete-300", Floor, "concrete", 0.3),
        one("floor.hollow-core-200", Floor, "hollow-core-concrete", 0.2),
        one("floor.hollow-core-260", Floor, "hollow-core-concrete", 0.26),
        one("floor.hollow-core-320", Floor, "hollow-core-concrete", 0.32),
        one("floor.timber-200", Floor, "timber", 0.2),
        b("floor.screed-concrete-insulation-350", Floor, &[("screed", 0.05), ("concrete", 0.2), ("insulation", 0.1)]),
        b("roof.flat-concrete-pir", Roof, &[("insulation-pir", 0.12), ("concrete", 0.2)]),
        b("roof.pitched-timber", Roof, &[("insulation", 0.15), ("timber", 0.05)]),
        one("ceiling.gypsum-12.5", Ceiling, "gypsum-board", 0.0125),
        one("ceiling.suspended-20", Ceiling, "gypsum-board", 0.02),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material;

    fn cavity() -> BuildUp {
        starter().into_iter().find(|b| b.id == "wall.cavity-pir-360").unwrap()
    }

    #[test]
    fn a_build_up_is_named_by_its_layers_and_thickness() {
        let materials = material::starter();
        let wall = cavity();
        assert_eq!(wall.rule_name(&materials, Lang::Nl), "Metselwerk-spouw-isolatie PIR-kalkzandsteen 360");
        assert_eq!(wall.rule_name(&materials, Lang::En), "Brickwork-air cavity-PIR insulation-sand-lime brick 360");
        assert_eq!(wall.rule_name(&materials, Lang::De), "Mauerwerk-Luftschicht-PIR-Dämmung-Kalksandstein 360");
        let partition = starter().into_iter().find(|b| b.id == "wall.metal-stud-75").unwrap();
        assert_eq!(partition.rule_name(&materials, Lang::Nl), "Gipsplaat-metal stud met isolatie-gipsplaat 100");
        let timber = starter().into_iter().find(|b| b.id == "wall.timber-frame-outer").unwrap();
        assert_eq!(timber.rule_name(&materials, Lang::Fr), "Bardage bois-lame d'air-OSB-ossature bois isolée-plaque de plâtre 204,5");
    }

    #[test]
    fn a_material_next_to_itself_is_named_once() {
        let materials = material::starter();
        let double = BuildUp::new("x", Role::Wall, vec![Layer::new("brick", 0.1), Layer::new("brick", 0.11), Layer::new("concrete", 0.2)]);
        assert_eq!(double.rule_name(&materials, Lang::Nl), "Metselwerk-beton 410");
    }

    #[test]
    fn layers_are_counted_from_the_exterior_or_the_top() {
        let wall = cavity();
        assert!((wall.thickness() - 0.36).abs() < 1e-12);
        let faces = wall.faces();
        assert_eq!(faces.len(), 4);
        assert_eq!(faces[0], (0.0, 0.1));
        assert!((faces[3].0 - 0.24).abs() < 1e-12 && (faces[3].1 - 0.36).abs() < 1e-12);
        assert!(!Role::Wall.counted_from_top() && Role::Roof.counted_from_top());
    }

    #[test]
    fn roles_have_their_ifc_classes() {
        assert_eq!((Role::Wall.ifc_class(), Role::Wall.ifc_type_class()), ("IfcWall", "IfcWallType"));
        assert_eq!((Role::Roof.ifc_class(), Role::Roof.predefined_type()), ("IfcSlab", "ROOF"));
        assert_eq!((Role::Ceiling.ifc_type_class(), Role::Ceiling.predefined_type()), ("IfcCoveringType", "CEILING"));
    }

    #[test]
    fn a_build_up_is_checked() {
        let materials = material::starter();
        assert_eq!(cavity().check(&materials), Ok(()));
        let layers = |l: Vec<Layer>| BuildUp::new("x", Role::Floor, l);
        assert_eq!(layers(Vec::new()).check(&materials), Err(LibraryError::NoLayers));
        assert_eq!(layers(vec![Layer::new("concrete", 0.0)]).check(&materials), Err(LibraryError::NotPositive));
        assert_eq!(layers(vec![Layer::new("concrete", 0.00005)]).check(&materials), Err(LibraryError::NotPositive), "below 0.1 mm");
        assert_eq!(layers(vec![Layer::new("concrete", f64::NAN)]).check(&materials), Err(LibraryError::NotPositive));
        assert_eq!(layers(vec![Layer::new("unobtainium", 0.1)]).check(&materials), Err(LibraryError::UnknownMaterial("unobtainium".into())));
        assert_eq!(BuildUp::new(" ", Role::Wall, vec![Layer::new("concrete", 0.1)]).check(&materials), Err(LibraryError::NoKey));
    }

    #[test]
    fn every_starter_build_up_checks_and_has_its_role_in_its_id() {
        let materials = material::starter();
        let all = starter();
        for b in &all {
            assert_eq!(b.check(&materials), Ok(()), "{}", b.id);
            assert!(b.id.starts_with(&b.role.name().to_lowercase()), "{}", b.id);
        }
        for role in Role::ALL {
            assert!(all.iter().any(|b| b.role == role), "a {role:?} to start with");
        }
    }
}
