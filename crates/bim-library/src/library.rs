//! A library: materials and the types made of them. A project has one (its types); a library file
//! holds one; importing one adds what the project lacks ([`Library::merge`]).

use crate::buildup::{self, BuildUp, Role};
use crate::material::{self, Material};
use crate::{Lang, LibraryError};
use serde::{Deserialize, Serialize};

/// Two layers of one material are the same when their thicknesses differ by no more than this
/// (metres): 0.01 mm.
pub const SAME_THICKNESS: f64 = 1e-5;

/// Materials and build-ups, in the order they were added.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Library {
    #[serde(default)]
    pub materials: Vec<Material>,
    #[serde(default)]
    pub buildups: Vec<BuildUp>,
}

impl Library {
    /// The library a new project starts with: Open BIM Studio's starter materials and planar types.
    pub fn starter() -> Self {
        Self { materials: material::starter(), buildups: buildup::starter() }
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
        let target = self.buildup(id)?;
        let name = target.rule_name(&self.materials, lang);
        let same = self.buildups_of(target.role).take_while(|b| b.id != id).filter(|b| b.rule_name(&self.materials, lang) == name).count();
        Some(if same == 0 { name } else { format!("{name} ({})", same + 1) })
    }

    /// The build-ups of `role` with their names in `lang`, in library order.
    pub fn names(&self, role: Role, lang: Lang) -> Vec<(&str, String)> {
        let mut seen: Vec<String> = Vec::new();
        self.buildups_of(role)
            .map(|b| {
                let name = b.rule_name(&self.materials, lang);
                let same = seen.iter().filter(|n| **n == name).count();
                seen.push(name.clone());
                (b.id.as_str(), if same == 0 { name } else { format!("{name} ({})", same + 1) })
            })
            .collect()
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
    /// materials by key, build-ups by role and layers ([`Library::matching`]). Nothing this
    /// library has is changed. A build-up whose id is taken gets a new one; one that does not
    /// check is left out. Returns how many materials and build-ups were added.
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
            let mut n = 1;
            while self.buildup(&fresh.id).is_some() {
                n += 1;
                fresh.id = format!("{}-{n}", b.id);
            }
            if self.add_buildup(fresh).is_ok() {
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
        let mut fresh = Library { materials: lib.materials.clone(), buildups: Vec::new() };
        for b in &lib.buildups {
            fresh.add_buildup(b.clone()).unwrap_or_else(|e| panic!("{}: {e}", b.id));
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
    fn a_build_up_of_an_unknown_material_is_left_out_of_a_merge() {
        let mut project = Library::starter();
        let other = Library { materials: Vec::new(), buildups: vec![BuildUp::new("wall.x", Role::Wall, vec![Layer::new("unknown", 0.1)])] };
        assert_eq!(project.merge(&other), 0);
    }
}
