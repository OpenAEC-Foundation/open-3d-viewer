//! Hatch standards: the named sets that say how a material is hatched, and the hatch a material
//! names under each of them.
//!
//! A standard ([`HatchStandard`]) is a name with a stable key ("nen47", "openaec") and, where it
//! is one of the catalogue's sets, that set. A library has several and one of them is chosen: a
//! drawing hatches by the chosen one. An office or a project adds its own (ISO 128, a house
//! standard) as a copy of one it has, and switches between them.
//!
//! A material names its hatch per standard ([`MaterialHatch`], in [`Material::hatches`] by the
//! standard's key): the hatch of a cut through it, the hatch of a face of it that is seen, the
//! scale from which the hatch is drawn, and what is shown on a smaller drawing (its fill, or
//! nothing). A material that names nothing under a standard gets what its own sign gives: the
//! hatch of that sign in the standard's set ([`HatchStandard::usual`]).
//!
//! Not yet: a drawing does not hatch a face that is seen; the hatch is kept and shown with the
//! material. A standard added by a project has no patterns of its own: it names hatches of the
//! catalogue.

use crate::hatch::{self, Hatch, HatchSet};
use crate::material::{Material, Names};
use crate::{uuid, Lang, LibraryError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The attribute of a standard's node with the standard itself.
pub const HATCH_STANDARD: &str = "openaec::hatchstandard";

/// A hatch standard: a named set of hatches that can be chosen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HatchStandard {
    /// Stable, lower case with hyphens ("nen47", "house").
    pub key: String,
    pub names: Names,
    /// The set of the catalogue a material's sign is looked up in when the material names no
    /// hatch under this standard.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set: Option<HatchSet>,
    /// The one a drawing hatches by: one of a library's standards.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub chosen: bool,
}

/// What a drawing smaller than the scale a hatch is drawn from shows in its place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Below {
    /// The hatch's colour, else the material's.
    #[default]
    Fill,
    /// The paper.
    Nothing,
}

/// The hatch of a material under one standard.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MaterialHatch {
    /// The key of the hatch of a cut through it; none: its hatch kind's pattern.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut: Option<String>,
    /// The key of the hatch of a face of it that is seen; none: no hatch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<String>,
    /// The hatch is drawn on drawings at 1 : `from` and larger; 0: at every scale.
    #[serde(default)]
    pub from: f64,
    /// What a smaller drawing shows.
    #[serde(default)]
    pub below: Below,
}

impl HatchStandard {
    pub fn new(key: &str, names: Names, set: Option<HatchSet>) -> Self {
        Self { key: key.into(), names, set, chosen: false }
    }

    pub fn name(&self, lang: Lang) -> &str {
        self.names.get(lang)
    }

    /// The hatch `material` has under this standard when it names none: that of its own sign
    /// in this standard's set (a kind of material of another set gives the sign itself).
    pub fn usual(&self, material: &Material) -> Option<&'static Hatch> {
        let own = material.catalogue_hatch()?;
        match self.set {
            Some(set) if own.set == set => Some(own),
            Some(set) => hatch::sign(set, own.sign),
            None => None,
        }
    }

    /// The hatch of `material` under this standard: what it names, else what its sign gives.
    pub fn of(&self, material: &Material) -> MaterialHatch {
        material.hatches.get(&self.key).cloned().unwrap_or_else(|| MaterialHatch { cut: self.usual(material).map(|h| h.key.clone()), ..MaterialHatch::default() })
    }

    /// Refused without a key or a name.
    pub fn check(&self) -> Result<(), LibraryError> {
        if self.key.trim().is_empty() {
            return Err(LibraryError::NoKey);
        }
        if Lang::ALL.iter().any(|lang| self.name(*lang).trim().is_empty()) {
            return Err(LibraryError::BadHatch("a name in every language".into()));
        }
        Ok(())
    }
}

impl MaterialHatch {
    /// The hatch of a cut, where it names one the catalogue has.
    pub fn cut_hatch(&self) -> Option<&'static Hatch> {
        self.cut.as_deref().and_then(hatch::hatch)
    }

    /// The hatch of a face seen, where it names one the catalogue has.
    pub fn face_hatch(&self) -> Option<&'static Hatch> {
        self.face.as_deref().and_then(hatch::hatch)
    }

    /// Whether a drawing at 1 : `scale` draws the hatch itself.
    pub fn drawn_at(&self, scale: f64) -> bool {
        self.from <= 0.0 || scale <= self.from + 1e-9
    }

    /// Refused when it names a hatch the catalogue lacks or a scale that is none.
    pub fn check(&self) -> Result<(), LibraryError> {
        for key in [&self.cut, &self.face].into_iter().flatten() {
            if hatch::hatch(key).is_none() {
                return Err(LibraryError::BadHatch(format!("a hatch of the catalogue, not \"{key}\"")));
            }
        }
        if !(self.from.is_finite() && self.from >= 0.0) {
            return Err(LibraryError::BadHatch("a scale of 0 or more".into()));
        }
        Ok(())
    }
}

/// The standards a library starts with: the suite's coloured hatches, which is chosen, and the
/// signs of NEN 47 in black on the paper.
pub fn starter() -> Vec<HatchStandard> {
    vec![
        HatchStandard { chosen: true, ..HatchStandard::new("openaec", Names::same("OpenAEC"), Some(HatchSet::OpenAec)) },
        HatchStandard::new("nen47", Names::same("NEN 47"), Some(HatchSet::Nen47)),
    ]
}

/// The path of the node of standard `key`.
pub fn path(key: &str) -> String {
    uuid(&format!("openaec-library/hatchstandard/{key}"))
}

/// A standard as a node of IFCX.
pub fn node(standard: &HatchStandard) -> Value {
    json!({
        "path": path(&standard.key),
        "attributes": {
            "bsi::ifc::prop::Name": standard.name(Lang::En),
            HATCH_STANDARD: standard,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Library;

    #[test]
    fn a_material_gets_the_hatch_of_its_sign_in_each_standard() {
        let library = Library::starter();
        let [openaec, nen47] = &library.hatch_standards[..] else { panic!("two standards") };
        assert_eq!((openaec.key.as_str(), openaec.chosen, nen47.key.as_str(), nen47.chosen), ("openaec", true, "nen47", false));
        let timber = library.material("timber").unwrap();
        assert_eq!(openaec.of(timber).cut.as_deref(), Some("openaec-12"));
        assert_eq!(nen47.of(timber).cut.as_deref(), Some("nen47-12"), "softwood, in black on the paper");
        let steel = library.material("steel").unwrap();
        assert_eq!(nen47.of(steel).cut_hatch().map(|h| (h.sign, h.marks.is_empty())), Some((18, true)), "a solid sign is in both sets");
        // every starter material with a sign has a hatch in both standards
        for m in library.materials.iter().filter(|m| m.sign.is_some()) {
            for s in &library.hatch_standards {
                assert!(s.of(m).cut_hatch().is_some_and(|h| Some(h.set) == s.set), "{} under {}", m.key, s.key);
            }
        }
        // a standard without a set gives nothing of itself
        let house = HatchStandard::new("house", Names::same("House"), None);
        assert_eq!(house.of(timber), MaterialHatch::default());
    }

    #[test]
    fn a_material_names_its_own_hatch_under_a_standard() {
        let mut library = Library::starter();
        let own = MaterialHatch { cut: Some("nen47-13".into()), face: Some("nen47-14".into()), from: 100.0, below: Below::Nothing };
        library.set_material_hatch("timber", "nen47", Some(own.clone())).unwrap();
        assert_eq!(library.material_hatch("timber"), Some(MaterialHatch { cut: Some("openaec-12".into()), ..MaterialHatch::default() }), "the chosen standard is another");
        assert!(library.choose_hatch_standard("nen47"));
        assert_eq!(library.hatch_standard().map(|s| s.key.as_str()), Some("nen47"));
        assert_eq!(library.material_hatch("timber"), Some(own.clone()));
        assert!(own.drawn_at(50.0) && own.drawn_at(100.0) && !own.drawn_at(200.0));
        assert!(MaterialHatch::default().drawn_at(1000.0));
        // what does not hold together is refused, and nothing changes
        let bad = MaterialHatch { cut: Some("nen47-999".into()), ..MaterialHatch::default() };
        assert!(matches!(library.set_material_hatch("timber", "nen47", Some(bad)), Err(LibraryError::BadHatch(_))));
        assert!(matches!(library.set_material_hatch("timber", "din", Some(own.clone())), Err(LibraryError::BadHatch(_))));
        assert!(matches!(library.set_material_hatch("nothing", "nen47", Some(own.clone())), Err(LibraryError::UnknownMaterial(_))));
        assert_eq!(library.material_hatch("timber"), Some(own));
        // taken away, the sign gives it again
        library.set_material_hatch("timber", "nen47", None).unwrap();
        assert_eq!(library.material_hatch("timber").and_then(|h| h.cut).as_deref(), Some("nen47-12"));
        assert!(!library.choose_hatch_standard("din"));
    }

    #[test]
    fn a_standard_is_added_as_a_copy_and_kept_in_a_file() {
        let mut library = Library::starter();
        library.set_material_hatch("timber", "nen47", Some(MaterialHatch { cut: Some("nen47-13".into()), from: 50.0, ..MaterialHatch::default() })).unwrap();
        let key = library.copy_hatch_standard("nen47", Names::same("House")).unwrap();
        assert_eq!(key, "house");
        let house = library.hatch_standards.last().unwrap();
        assert_eq!((house.set, house.chosen), (Some(HatchSet::Nen47), false));
        assert_eq!(house.of(library.material("timber").unwrap()).cut.as_deref(), Some("nen47-13"), "what the materials name comes along");
        assert_eq!(library.copy_hatch_standard("nen47", Names::same("House")).unwrap(), "house-2");
        assert!(matches!(library.add_hatch_standard(HatchStandard::new("house", Names::same("House"), None)), Err(LibraryError::Duplicate(_))));
        assert!(library.choose_hatch_standard("house"));
        // through a file and back
        let file = crate::ifcx::file(&library, "test", "test", "2026-11-20T00:00:00Z");
        let read = crate::ifcx::read(&file).unwrap();
        assert_eq!(read.hatch_standards, library.hatch_standards);
        assert_eq!(read.hatch_standard().map(|s| s.key.as_str()), Some("house"));
        assert_eq!(read.material("timber").unwrap().hatches, library.material("timber").unwrap().hatches);
        // a library without standards draws a material by its own sign
        let bare = Library { materials: library.materials.clone(), ..Library::default() };
        assert_eq!(bare.material_hatch("timber").and_then(|h| h.cut).as_deref(), Some("openaec-12"));
        // merged into a library that has a chosen one, a standard comes unchosen
        let mut other = Library::starter();
        other.merge(&library);
        assert_eq!(other.hatch_standards.iter().filter(|s| s.chosen).map(|s| s.key.as_str()).collect::<Vec<_>>(), ["openaec"]);
        assert_eq!(other.hatch_standards.len(), 4);
    }
}
