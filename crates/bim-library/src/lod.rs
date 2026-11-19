//! Levels of development: how far a type is worked out, from LOD 200 to LOD 400.
//!
//! The levels are those of the BIMForum Level of Development Specification, as offices use
//! them to say what a model element may be relied on for:
//!
//! - **LOD 200**: a generic element of about the right size, shape and place.
//! - **LOD 300**: a specific element: its size, its shape and what it is made of are those it
//!   will have.
//! - **LOD 350**: as LOD 300, with what joins it to the elements round it: its members, its
//!   supports, its connections.
//! - **LOD 400**: worked out to be made: every part and fixing, as it is fabricated and
//!   assembled.
//!
//! A type of the library has its level by rule ([`crate::BuildUp::lod`] and its likes): a
//! build-up or a linear type of the generic material is LOD 200, one of real materials LOD 300,
//! a system with its members LOD 350, a frame worked out to its plates and prefab units
//! LOD 400. An object type says its own ([`crate::object::ObjectType::lod`]).
//!
//! The geometry the suite draws for an object is a box of its size at every level: the level
//! says how far its size, its material and its properties are settled, not how fine it is
//! drawn.

use crate::buildup::BuildUp;
use crate::profile::LinearType;
use crate::system::{SystemGroup, SystemType};
use serde::{Deserialize, Serialize};

/// A level of development.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Lod {
    #[serde(rename = "200")]
    L200,
    #[serde(rename = "300")]
    L300,
    #[serde(rename = "350")]
    L350,
    #[serde(rename = "400")]
    L400,
}

impl Lod {
    pub const ALL: [Lod; 4] = [Lod::L200, Lod::L300, Lod::L350, Lod::L400];

    /// Its number: 200, 300, 350 or 400.
    pub fn number(self) -> u32 {
        match self {
            Lod::L200 => 200,
            Lod::L300 => 300,
            Lod::L350 => 350,
            Lod::L400 => 400,
        }
    }

    /// As it is written: "LOD 300".
    pub fn name(self) -> &'static str {
        match self {
            Lod::L200 => "LOD 200",
            Lod::L300 => "LOD 300",
            Lod::L350 => "LOD 350",
            Lod::L400 => "LOD 400",
        }
    }

    /// What it says of an element, in Dutch and in English.
    pub fn says(self) -> (&'static str, &'static str) {
        match self {
            Lod::L200 => ("algemeen element, bij benadering", "generic element, approximate"),
            Lod::L300 => ("specifiek element, exact", "specific element, exact"),
            Lod::L350 => ("met aansluitingen op andere elementen", "with its interfaces to other elements"),
            Lod::L400 => ("uitgewerkt voor fabricage en montage", "worked out for fabrication and assembly"),
        }
    }

    /// The level with `number`.
    pub fn of(number: u32) -> Option<Lod> {
        Lod::ALL.into_iter().find(|lod| lod.number() == number)
    }
}

/// Whether a material is the generic one: what a type is of before it is known what it is of.
fn generic(material: &str) -> bool {
    material == "generic" || material == "unknown"
}

impl BuildUp {
    /// Its level by rule: LOD 200 while every layer is of the generic material, else LOD 300.
    pub fn lod(&self) -> Lod {
        if self.layers.iter().all(|layer| generic(&layer.material)) {
            Lod::L200
        } else {
            Lod::L300
        }
    }
}

impl LinearType {
    /// Its level by rule: LOD 200 of the generic material, else LOD 300.
    pub fn lod(&self) -> Lod {
        if generic(&self.material) {
            Lod::L200
        } else {
            Lod::L300
        }
    }
}

impl SystemType {
    /// Its level by rule: a system is made with its members and panels, so LOD 350; a frame
    /// with layers across it (its boards, its membranes, its cladding) is worked out to be
    /// made, LOD 400.
    pub fn lod(&self) -> Lod {
        if matches!(self.group, SystemGroup::TimberFrame | SystemGroup::MetalStud) && !self.layers.is_empty() {
            Lod::L400
        } else {
            Lod::L350
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Library;

    #[test]
    fn the_levels_are_four_in_order() {
        assert_eq!(Lod::ALL.map(Lod::number), [200, 300, 350, 400]);
        assert!(Lod::L200 < Lod::L300 && Lod::L350 < Lod::L400);
        assert_eq!((Lod::of(350), Lod::of(100), Lod::L300.name()), (Some(Lod::L350), None, "LOD 300"));
        assert_eq!(serde_json::to_string(&Lod::L350).unwrap(), "\"350\"");
        assert_eq!(serde_json::from_str::<Lod>("\"400\"").unwrap(), Lod::L400);
    }

    #[test]
    fn a_type_has_its_level_by_rule() {
        let library = Library::starter();
        let ocondat = crate::ocondat::library();
        // a wall of the generic material is a placeholder; one of real layers is specific
        assert_eq!(ocondat.buildup("ocondat.wall.gen_100").map(BuildUp::lod), Some(Lod::L200));
        assert_eq!(library.buildup("wall.cavity-pir-360").map(BuildUp::lod), Some(Lod::L300));
        assert_eq!(ocondat.linear_type("ocondat.column.gen-r150").map(LinearType::lod), Some(Lod::L200));
        assert_eq!(ocondat.linear_type("ocondat.beam.hb-71x196").map(LinearType::lod), Some(Lod::L300));
        // a system is made with its members; a frame with its layers is worked out to be made
        for t in &library.system_types {
            let expected = if matches!(t.group, SystemGroup::TimberFrame | SystemGroup::MetalStud) && !t.layers.is_empty() { Lod::L400 } else { Lod::L350 };
            assert_eq!(t.lod(), expected, "{}", t.id);
        }
        assert!(library.system_types.iter().any(|t| t.lod() == Lod::L350));
    }
}
