//! A library: materials and the types made of them. A project has one (its types); a library file
//! holds one; importing one adds what the project lacks ([`Library::merge`]).

use crate::buildup::{self, BuildUp, Role};
use crate::material::{self, Material};
use crate::profile::{self, LinearRole, LinearType};
use crate::system::{self, SystemGroup, SystemType};
use crate::{Lang, LibraryError};
use serde::{Deserialize, Serialize};

/// Two layers of one material are the same when their thicknesses differ by no more than this
/// (metres): 0.01 mm.
pub const SAME_THICKNESS: f64 = 1e-5;

/// Materials, build-ups, linear types and system types, in the order they were added.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Library {
    #[serde(default)]
    pub materials: Vec<Material>,
    #[serde(default)]
    pub buildups: Vec<BuildUp>,
    #[serde(default)]
    pub linear_types: Vec<LinearType>,
    #[serde(default)]
    pub system_types: Vec<SystemType>,
}

/// Names made unique: " (2)", " (3)" after a name given before, in order.
fn numbered(names: impl Iterator<Item = String>) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    names
        .map(|name| {
            let same = seen.iter().filter(|n| **n == name).count();
            seen.push(name.clone());
            if same == 0 {
                name
            } else {
                format!("{name} ({})", same + 1)
            }
        })
        .collect()
}

/// `id`, or the first of `id-2`, `id-3`, ... that `taken` says is free.
fn free_id(id: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut fresh = id.to_string();
    let mut n = 1;
    while taken(&fresh) {
        n += 1;
        fresh = format!("{id}-{n}");
    }
    fresh
}

impl Library {
    /// The library a new project starts with: Open BIM Studio's starter materials, planar types,
    /// linear types and system types. The steel profiles are a catalogue
    /// ([`profile::steel_profiles`]) a project takes types from, not starter types.
    pub fn starter() -> Self {
        Self { materials: material::starter(), buildups: buildup::starter(), linear_types: profile::starter(), system_types: system::starter() }
    }

    pub fn material(&self, key: &str) -> Option<&Material> {
        self.materials.iter().find(|m| m.key == key)
    }

    pub fn buildup(&self, id: &str) -> Option<&BuildUp> {
        self.buildups.iter().find(|b| b.id == id)
    }

    /// The build-ups of `role`, in library order.
    pub fn buildups_of(&self, role: Role) -> impl Iterator<Item = &BuildUp> {
        self.buildups.iter().filter(move |b| b.role == role)
    }

    /// Adds a material under a key of its own.
    pub fn add_material(&mut self, material: Material) -> Result<(), LibraryError> {
        if material.key.trim().is_empty() {
            return Err(LibraryError::NoKey);
        }
        if self.material(&material.key).is_some() {
            return Err(LibraryError::Duplicate(material.key));
        }
        self.materials.push(material);
        Ok(())
    }

    /// Adds a build-up under an id of its own, once it checks against the library's materials.
    pub fn add_buildup(&mut self, buildup: BuildUp) -> Result<(), LibraryError> {
        buildup.check(&self.materials)?;
        if self.buildup(&buildup.id).is_some() {
            return Err(LibraryError::Duplicate(buildup.id));
        }
        self.buildups.push(buildup);
        Ok(())
    }

    /// The name of build-up `id` in `lang`: its rule name, with " (2)", " (3)" when build-ups of
    /// its role before it have the same rule name.
    pub fn name(&self, id: &str, lang: Lang) -> Option<String> {
        let role = self.buildup(id)?.role;
        self.names(role, lang).into_iter().find(|(i, _)| *i == id).map(|(_, name)| name)
    }

    /// The build-ups of `role` with their names in `lang`, in library order.
    pub fn names(&self, role: Role, lang: Lang) -> Vec<(&str, String)> {
        let names = numbered(self.buildups_of(role).map(|b| b.rule_name(&self.materials, lang)));
        self.buildups_of(role).map(|b| b.id.as_str()).zip(names).collect()
    }

    pub fn linear_type(&self, id: &str) -> Option<&LinearType> {
        self.linear_types.iter().find(|t| t.id == id)
    }

    /// The linear types of `role`, in library order.
    pub fn linear_types_of(&self, role: LinearRole) -> impl Iterator<Item = &LinearType> {
        self.linear_types.iter().filter(move |t| t.role == role)
    }

    /// Adds a linear type under an id of its own, once it checks against the library's materials.
    pub fn add_linear_type(&mut self, linear: LinearType) -> Result<(), LibraryError> {
        linear.check(&self.materials)?;
        if self.linear_type(&linear.id).is_some() {
            return Err(LibraryError::Duplicate(linear.id));
        }
        self.linear_types.push(linear);
        Ok(())
    }

    /// The name of linear type `id` in `lang`, made unique within its role as a build-up's.
    pub fn linear_name(&self, id: &str, lang: Lang) -> Option<String> {
        let role = self.linear_type(id)?.role;
        self.linear_names(role, lang).into_iter().find(|(i, _)| *i == id).map(|(_, name)| name)
    }

    /// The linear types of `role` with their names in `lang`, in library order.
    pub fn linear_names(&self, role: LinearRole, lang: Lang) -> Vec<(&str, String)> {
        let names = numbered(self.linear_types_of(role).map(|t| t.rule_name(&self.materials, lang)));
        self.linear_types_of(role).map(|t| t.id.as_str()).zip(names).collect()
    }

    /// The linear type this library already has with the role, material, profile and designation
    /// of `linear` (the profile within 0.01 mm).
    pub fn matching_linear(&self, linear: &LinearType) -> Option<&LinearType> {
        self.linear_types_of(linear.role).find(|t| t.material == linear.material && t.designation == linear.designation && t.profile.same(&linear.profile))
    }

    pub fn system_type(&self, id: &str) -> Option<&SystemType> {
        self.system_types.iter().find(|t| t.id == id)
    }

    /// The system types of `group`, in library order.
    pub fn system_types_of(&self, group: SystemGroup) -> impl Iterator<Item = &SystemType> {
        self.system_types.iter().filter(move |t| t.group == group)
    }

    /// Adds a system type under an id of its own, once it checks against the library's materials.
    pub fn add_system_type(&mut self, system: SystemType) -> Result<(), LibraryError> {
        system.check(&self.materials)?;
        if self.system_type(&system.id).is_some() {
            return Err(LibraryError::Duplicate(system.id));
        }
        self.system_types.push(system);
        Ok(())
    }

    /// The name of system type `id` in `lang`, made unique within its group as a build-up's.
    pub fn system_name(&self, id: &str, lang: Lang) -> Option<String> {
        let group = self.system_type(id)?.group;
        self.system_names(group, lang).into_iter().find(|(i, _)| *i == id).map(|(_, name)| name)
    }

    /// The system types of `group` with their names in `lang`, in library order.
    pub fn system_names(&self, group: SystemGroup, lang: Lang) -> Vec<(&str, String)> {
        let names = numbered(self.system_types_of(group).map(|t| t.rule_name(&self.materials, lang)));
        self.system_types_of(group).map(|t| t.id.as_str()).zip(names).collect()
    }

    /// The system type this library already has that makes the same system as `system`: the
    /// same group, grids, members (and their framing), panels, layers, corners and level, lengths
    /// within 0.01 mm.
    pub fn matching_system(&self, system: &SystemType) -> Option<&SystemType> {
        let near = |a: f64, b: f64| (a - b).abs() <= SAME_THICKNESS;
        let same_grid = |a: &system::Grid, b: &system::Grid| a.pattern == b.pattern && near(a.bay, b.bay) && near(a.row, b.row);
        let same_members = |a: Option<&system::Members>, b: Option<&system::Members>| match (a, b) {
            (Some(a), Some(b)) => a.material == b.material && a.framing == b.framing && near(a.width, b.width) && near(a.depth, b.depth),
            (None, None) => true,
            _ => false,
        };
        let same = |t: &SystemType| {
            t.corners == system.corners
                && t.lying.is_some() == system.lying.is_some()
                && near(t.lying.unwrap_or(0.0), system.lying.unwrap_or(0.0))
                && t.faces().count() == system.faces().count()
                && t.faces().zip(system.faces()).all(|((oa, ga, ma, pa), (ob, gb, mb, pb))| {
                    near(oa, ob) && same_grid(ga, gb) && same_members(ma, mb) && pa.material == pb.material && near(pa.thickness, pb.thickness)
                })
        };
        self.system_types_of(system.group).find(|t| same(t))
    }

    /// The build-up this library already has with the role and the layers of `buildup`: the same
    /// materials in the same order, each as thick within 0.01 mm. A name says less than the layers
    /// (two layers of brick are named as one), so the layers decide.
    pub fn matching(&self, buildup: &BuildUp) -> Option<&BuildUp> {
        let same = |a: &BuildUp| {
            a.layers.len() == buildup.layers.len()
                && a.layers.iter().zip(&buildup.layers).all(|(x, y)| x.material == y.material && (x.thickness - y.thickness).abs() <= SAME_THICKNESS)
        };
        self.buildups_of(buildup.role).find(|b| same(b))
    }

    /// Adds what `other` has and this library lacks, as Open BIM Studio imports a library:
    /// materials by key, build-ups by role and layers ([`Library::matching`]), linear types by
    /// role, material and profile ([`Library::matching_linear`]), system types by what they make
    /// ([`Library::matching_system`]). Nothing this library has is
    /// changed. A type whose id is taken gets a new one; one that does not check is left out.
    /// Returns how many materials and types were added.
    pub fn merge(&mut self, other: &Library) -> usize {
        let mut added = 0;
        for m in &other.materials {
            if self.material(&m.key).is_none() && self.add_material(m.clone()).is_ok() {
                added += 1;
            }
        }
        for b in &other.buildups {
            if self.matching(b).is_some() {
                continue;
            }
            let mut fresh = b.clone();
            fresh.id = free_id(&b.id, |id| self.buildup(id).is_some());
            if self.add_buildup(fresh).is_ok() {
                added += 1;
            }
        }
        for t in &other.linear_types {
            if self.matching_linear(t).is_some() {
                continue;
            }
            let mut fresh = t.clone();
            fresh.id = free_id(&t.id, |id| self.linear_type(id).is_some());
            if self.add_linear_type(fresh).is_ok() {
                added += 1;
            }
        }
        for t in &other.system_types {
            if self.matching_system(t).is_some() {
                continue;
            }
            let mut fresh = t.clone();
            fresh.id = free_id(&t.id, |id| self.system_type(id).is_some());
            if self.add_system_type(fresh).is_ok() {
                added += 1;
            }
        }
        added
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buildup::Layer;
    use crate::material::{HatchKind, Names};

    #[test]
    fn the_starter_library_checks() {
        let lib = Library::starter();
        let mut fresh = Library { materials: lib.materials.clone(), ..Library::default() };
        for b in &lib.buildups {
            fresh.add_buildup(b.clone()).unwrap_or_else(|e| panic!("{}: {e}", b.id));
        }
        for t in &lib.linear_types {
            fresh.add_linear_type(t.clone()).unwrap_or_else(|e| panic!("{}: {e}", t.id));
        }
        for t in &lib.system_types {
            fresh.add_system_type(t.clone()).unwrap_or_else(|e| panic!("{}: {e}", t.id));
        }
        assert_eq!(fresh, lib);
        assert_eq!(lib.name("roof.flat-concrete-pir", Lang::Nl).unwrap(), "Isolatie PIR-beton 320");
        assert_eq!(lib.name("floor.hollow-core-260", Lang::En).unwrap(), "Hollow-core slab 260");
    }

    #[test]
    fn equal_names_of_one_role_are_told_apart() {
        let mut lib = Library::starter();
        // the same materials and total as the 360 cavity wall, but other thicknesses
        let other = BuildUp::new("wall.cavity-pir-360-b", Role::Wall, vec![Layer::new("brick", 0.1), Layer::new("air", 0.05), Layer::new("insulation-pir", 0.09), Layer::new("sand-lime", 0.12)]);
        lib.add_buildup(other).unwrap();
        assert_eq!(lib.name("wall.cavity-pir-360", Lang::Nl).unwrap(), "Metselwerk-spouw-isolatie PIR-kalkzandsteen 360");
        assert_eq!(lib.name("wall.cavity-pir-360-b", Lang::Nl).unwrap(), "Metselwerk-spouw-isolatie PIR-kalkzandsteen 360 (2)");
        let names = lib.names(Role::Wall, Lang::Nl);
        assert_eq!(names.last().unwrap().1, "Metselwerk-spouw-isolatie PIR-kalkzandsteen 360 (2)");
        // a floor of the same name is another role: no number
        let floor = lib.names(Role::Floor, Lang::Nl);
        assert!(floor.iter().all(|(_, n)| !n.ends_with(')')), "{floor:?}");
    }

    #[test]
    fn ids_and_keys_are_unique() {
        let mut lib = Library::starter();
        let copy = lib.buildups[0].clone();
        assert_eq!(lib.add_buildup(copy.clone()), Err(LibraryError::Duplicate(copy.id)));
        let concrete = lib.materials[0].clone();
        assert_eq!(lib.add_material(concrete), Err(LibraryError::Duplicate("concrete".into())));
        let nameless = Material { key: "".into(), names: Names::same("x"), hatch: HatchKind::Generic, colour: [0, 0, 0] };
        assert_eq!(lib.add_material(nameless), Err(LibraryError::NoKey));
    }

    #[test]
    fn merging_adds_only_what_is_missing() {
        let mut project = Library::starter();
        let before = project.clone();
        let mut other = Library::starter();
        // a material and a build-up the project lacks, and one it has under another id
        other.materials.push(Material { key: "cork".into(), names: Names::new("kurk", "cork", "Kork", "liège"), hatch: HatchKind::Insulation, colour: [160, 120, 80] });
        other.materials[0].colour = [1, 2, 3];
        other.buildups.push(BuildUp::new("wall.cork-200", Role::Wall, vec![Layer::new("cork", 0.2)]));
        other.buildups.push(BuildUp::new("wall.concrete-250-copy", Role::Wall, vec![Layer::new("concrete", 0.250001)]));
        // a new one whose id the project already uses for something else
        other.buildups.push(BuildUp::new("wall.concrete-150", Role::Wall, vec![Layer::new("concrete", 0.175)]));
        // the name of the 360 cavity wall, other layers
        other.buildups.push(BuildUp::new("wall.cavity-b", Role::Wall, vec![Layer::new("brick", 0.1), Layer::new("air", 0.06), Layer::new("insulation-pir", 0.08), Layer::new("sand-lime", 0.12)]));
        assert_eq!(project.merge(&other), 4, "cork, the cork wall, the 175 wall and the other cavity wall");
        assert_eq!(project.material("concrete"), before.material("concrete"), "the project's concrete stays as it was");
        assert!(project.buildup("wall.concrete-250-copy").is_none(), "the 250 wall was there, within 0.01 mm");
        assert_eq!(project.name("wall.cavity-b", Lang::Nl).unwrap(), "Metselwerk-spouw-isolatie PIR-kalkzandsteen 360 (2)");
        assert_eq!(project.buildup("wall.concrete-150"), before.buildup("wall.concrete-150"));
        assert_eq!(project.buildup("wall.concrete-150-2").unwrap().layers[0].thickness, 0.175);
        assert_eq!(project.merge(&other), 0, "merging twice adds nothing");
    }

    #[test]
    fn linear_types_merge_by_role_material_and_profile() {
        let mut project = Library::starter();
        let mut other = Library::starter();
        other.linear_types.push(LinearType::steel("column.hea-200", LinearRole::Column, "HEA 200").unwrap());
        other.linear_types.push(LinearType::steel("beam.hea-200", LinearRole::Beam, "HEA 200").unwrap());
        // the project's 300x300 concrete column, under another id
        let mut copy = project.linear_types[0].clone();
        copy.id = "column.copy".into();
        other.linear_types.push(copy);
        assert_eq!(project.merge(&other), 2, "the HEA 200 as a column and as a beam");
        assert_eq!(project.linear_name("column.hea-200", Lang::Nl).unwrap(), "HEA 200");
        assert!(project.linear_type("column.copy").is_none());
        // a second 200x200 steel column of another id gets a number in the list
        project.add_linear_type(LinearType::new("column.steel-200x200-b", LinearRole::Column, "steel", crate::ProfileShape::Rectangle { width: 0.2, depth: 0.2 })).unwrap();
        assert_eq!(project.linear_name("column.steel-200x200-b", Lang::Nl).unwrap(), "Staal 200x200 (2)");
        assert_eq!(project.linear_names(LinearRole::Pipe, Lang::En), vec![("pipe.generic-110", "Generic round 110".to_string())]);
    }

    #[test]
    fn system_types_merge_by_what_they_make() {
        let mut project = Library::starter();
        let mut other = Library::starter();
        // the project's aluminium curtain wall under another id, 0.001 mm off
        let mut copy = project.system_types[0].clone();
        copy.id = "system.copy".into();
        copy.grid.bay += 1e-6;
        other.system_types.push(copy);
        // a curtain wall at 1800 instead, under an id the project uses
        let mut wider = project.system_types[0].clone();
        wider.grid.bay = 1.8;
        other.system_types.push(wider);
        assert_eq!(project.merge(&other), 1, "only the curtain wall at 1800");
        assert!(project.system_type("system.copy").is_none());
        assert_eq!(project.system_name("system.curtain-wall-aluminium-1500-2", Lang::Nl).unwrap(), "Vliesgevel aluminium 50×150 h.o.h. 1800");
        // the same name twice in a group gets a number
        let mut twin = project.system_types[0].clone();
        twin.id = "system.twin".into();
        twin.grid.row = 2.0;
        project.add_system_type(twin).unwrap();
        assert_eq!(project.system_name("system.twin", Lang::Nl).unwrap(), "Vliesgevel aluminium 50×150 h.o.h. 1500 (2)");
        assert_eq!(project.system_names(SystemGroup::Ceiling, Lang::En), vec![("system.ceiling-600x600", "Mineral fibre 15 600×600".to_string())]);
        let other = Library { system_types: vec![SystemType { id: "system.x".into(), ..project.system_types[0].clone() }], ..Library::default() };
        let mut bare = Library::default();
        assert_eq!(bare.merge(&other), 0, "a type of materials the library lacks is left out");
    }

    #[test]
    fn a_build_up_of_an_unknown_material_is_left_out_of_a_merge() {
        let mut project = Library::starter();
        let other = Library { buildups: vec![BuildUp::new("wall.x", Role::Wall, vec![Layer::new("unknown", 0.1)])], ..Library::default() };
        assert_eq!(project.merge(&other), 0);
    }
}
