//! Materials: what a layer is made of. A material has a stable English key that layers and files
//! refer to, a name in four languages, a colour for the 3D view, and a hatch kind that says how a
//! cut through it is drawn. The pattern of a hatch kind depends on the drawing standard
//! ([`HatchKind::pattern`]), so the drawing settings of a country choose it.
//!
//! The materials, their colours and their hatches are Open BIM Studio's.

use crate::Lang;
use serde::{Deserialize, Serialize};

/// A name in each language.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Names {
    pub nl: String,
    pub en: String,
    pub de: String,
    pub fr: String,
}

impl Names {
    pub fn new(nl: &str, en: &str, de: &str, fr: &str) -> Self {
        Self { nl: nl.into(), en: en.into(), de: de.into(), fr: fr.into() }
    }

    /// The same name in every language (a code such as "OSB", or a name read from a file).
    pub fn same(name: &str) -> Self {
        Self::new(name, name, name, name)
    }

    pub fn get(&self, lang: Lang) -> &str {
        match lang {
            Lang::Nl => &self.nl,
            Lang::En => &self.en,
            Lang::De => &self.de,
            Lang::Fr => &self.fr,
        }
    }
}

/// How a cut through a material is hatched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HatchKind {
    Brick,
    SandLime,
    Concrete,
    Timber,
    Steel,
    Insulation,
    Gypsum,
    /// A cavity: left empty.
    Air,
    Generic,
}

impl HatchKind {
    pub const ALL: [HatchKind; 9] = [
        HatchKind::Brick,
        HatchKind::SandLime,
        HatchKind::Concrete,
        HatchKind::Timber,
        HatchKind::Steel,
        HatchKind::Insulation,
        HatchKind::Gypsum,
        HatchKind::Air,
        HatchKind::Generic,
    ];

    /// The hatch of this kind under `standard`: NEN 47 in colour; DIN 1356, ISO 128 and ANSI in
    /// black on white, each with its own spacing and its own sign for masonry, concrete and timber.
    pub fn pattern(self, standard: DrawingStandard) -> HatchPattern {
        use DrawingStandard::*;
        use HatchKind::*;
        const WHITE: [u8; 3] = [255, 255, 255];
        const BLACK: [u8; 3] = [0, 0, 0];
        const GREY: [u8; 3] = [128, 128, 128];
        let lines = |fill, line, angle, spacing| HatchPattern { fill, line, angle, spacing, crossed: false, zigzag: false };
        let crossed = |fill, line, angle, spacing| HatchPattern { crossed: true, ..lines(fill, line, angle, spacing) };
        let solid = |fill| lines(fill, fill, 45.0, 0.0);
        let zigzag = |fill| HatchPattern { zigzag: true, ..lines(fill, BLACK, 0.0, 0.0) };
        match (standard, self) {
            (Nen47, Brick) => lines([212, 144, 143], BLACK, 45.0, 0.08),
            (Nen47, Insulation) => zigzag([255, 253, 224]),
            (Nen47, Concrete) => solid([192, 192, 192]),
            (Nen47, SandLime) => lines([200, 192, 176], [168, 160, 144], 45.0, 0.08),
            (Nen47, Timber) => lines([240, 220, 185], BLACK, 45.0, 0.075),
            (Nen47, Steel) => solid(BLACK),
            (Nen47, Gypsum) => lines(WHITE, GREY, 45.0, 0.03),
            (Nen47, Air) => solid(WHITE),
            (Nen47, Generic) => lines(WHITE, GREY, 45.0, 0.06),
            (_, Insulation) => zigzag(WHITE),
            (_, Air) => solid(WHITE),
            (_, Steel) => solid(BLACK),
            (Din1356, Brick | SandLime) => lines(WHITE, BLACK, 45.0, 0.05),
            (Din1356, Concrete) => crossed(WHITE, BLACK, 45.0, 0.05),
            (Din1356, Timber) => lines(WHITE, BLACK, 0.0, 0.03),
            (Iso128, Brick | SandLime) => lines(WHITE, BLACK, 45.0, 0.04),
            (Iso128, Concrete) => crossed(WHITE, BLACK, 45.0, 0.08),
            (Iso128, Timber) => lines(WHITE, BLACK, 135.0, 0.04),
            (Ansi, Brick | SandLime) => lines(WHITE, BLACK, 45.0, 0.025),
            (Ansi, Concrete) => solid([210, 210, 210]),
            (Ansi, Timber) => lines(WHITE, BLACK, 45.0, 0.06),
            (_, Gypsum | Generic) => lines(WHITE, BLACK, 45.0, 0.06),
        }
    }
}

/// The drawing standards whose hatches are known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DrawingStandard {
    /// NEN 47 (the Netherlands), in colour.
    #[default]
    Nen47,
    /// DIN 1356 (Germany).
    Din1356,
    /// ISO 128 / BS 1192.
    Iso128,
    /// ANSI (the United States).
    Ansi,
}

impl DrawingStandard {
    pub const ALL: [DrawingStandard; 4] = [DrawingStandard::Nen47, DrawingStandard::Din1356, DrawingStandard::Iso128, DrawingStandard::Ansi];
}

/// A hatch: the fill behind it, its line colour, the angle of its lines (degrees on paper,
/// anticlockwise from the x axis) and their spacing (metres on the model, so a hatch scales with
/// the drawing; 0 for a solid fill without lines), crossed lines, or one zigzag across the layer.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HatchPattern {
    pub fill: [u8; 3],
    pub line: [u8; 3],
    pub angle: f64,
    pub spacing: f64,
    pub crossed: bool,
    pub zigzag: bool,
}

/// A material.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Material {
    /// Stable, English, lower case with hyphens ("sand-lime"): what layers and files refer to.
    pub key: String,
    /// Its names, as written in a type's name: lower case but for codes ("isolatie PIR").
    pub names: Names,
    pub hatch: HatchKind,
    /// Its colour in the 3D view, red, green and blue.
    pub colour: [u8; 3],
}

impl Material {
    pub fn name(&self, lang: Lang) -> &str {
        self.names.get(lang)
    }

    /// Its colour as fractions of one, for IFCX and the renderer.
    pub fn colour_f64(&self) -> [f64; 3] {
        self.colour.map(|c| f64::from(c) / 255.0)
    }
}

/// The materials a library starts with: Open BIM Studio's.
pub fn starter() -> Vec<Material> {
    use HatchKind::*;
    let m = |key: &str, nl: &str, en: &str, de: &str, fr: &str, hatch, colour| Material { key: key.into(), names: Names::new(nl, en, de, fr), hatch, colour };
    const CONCRETE: [u8; 3] = [185, 185, 185];
    const INSULATION: [u8; 3] = [240, 225, 150];
    const TIMBER: [u8; 3] = [205, 165, 115];
    const GENERIC: [u8; 3] = [175, 182, 195];
    vec![
        m("concrete", "beton", "concrete", "Beton", "béton", Concrete, CONCRETE),
        m("hollow-core-concrete", "kanaalplaat", "hollow-core slab", "Spannbeton-Hohldiele", "dalle alvéolée", Concrete, CONCRETE),
        m("brick", "metselwerk", "brickwork", "Mauerwerk", "maçonnerie", Brick, [205, 140, 115]),
        m("sand-lime", "kalkzandsteen", "sand-lime brick", "Kalksandstein", "brique silico-calcaire", SandLime, [225, 220, 205]),
        m("insulation", "isolatie", "insulation", "Dämmung", "isolant", Insulation, INSULATION),
        m("insulation-pir", "isolatie PIR", "PIR insulation", "PIR-Dämmung", "isolant PIR", Insulation, INSULATION),
        m("mineral-wool", "minerale wol", "mineral wool", "Mineralwolle", "laine minérale", Insulation, [236, 214, 120]),
        m("timber", "hout", "timber", "Holz", "bois", Timber, TIMBER),
        m("timber-cladding", "houten gevelbekleding", "timber cladding", "Holzverschalung", "bardage bois", Timber, TIMBER),
        m("timber-frame-insulated", "houtskelet met isolatie", "timber frame with insulation", "Holzständerwerk mit Dämmung", "ossature bois isolée", Insulation, INSULATION),
        m("osb", "OSB", "OSB", "OSB", "OSB", Timber, [214, 182, 128]),
        m("plywood", "multiplex", "plywood", "Sperrholz", "contreplaqué", Timber, [222, 196, 150]),
        m("gypsum-board", "gipsplaat", "gypsum board", "Gipskartonplatte", "plaque de plâtre", Gypsum, [240, 240, 236]),
        m("breather-membrane", "dampopen folie", "breather membrane", "diffusionsoffene Bahn", "pare-pluie", Generic, [62, 66, 72]),
        m("vapour-control-membrane", "dampremmende folie", "vapour control layer", "Dampfbremse", "pare-vapeur", Generic, [196, 214, 226]),
        m("metal-stud-insulated", "metal stud met isolatie", "metal studs with insulation", "Metallständerwerk mit Dämmung", "ossature métallique isolée", Insulation, INSULATION),
        m("steel", "staal", "steel", "Stahl", "acier", Steel, [95, 100, 110]),
        m("aluminium", "aluminium", "aluminium", "Aluminium", "aluminium", Generic, [196, 200, 206]),
        m("zinc", "zink", "zinc", "Zink", "zinc", Generic, [150, 160, 168]),
        m("glass", "glas", "glass", "Glas", "verre", Generic, [160, 200, 225]),
        m("air", "spouw", "air cavity", "Luftschicht", "lame d'air", Air, GENERIC),
        m("mortar", "mortel", "mortar", "Mörtel", "mortier", Generic, [228, 224, 212]),
        m("glue", "lijm", "adhesive", "Klebstoff", "colle", Generic, [210, 208, 200]),
        m("grout", "voegmortel", "grout", "Fugenmörtel", "coulis", Generic, [210, 208, 200]),
        m("screed", "dekvloer", "screed", "Estrich", "chape", Generic, [200, 196, 188]),
        m("mineral-fibre", "mineraalvezel", "mineral fibre", "Mineralfaser", "fibre minérale", Generic, [238, 238, 232]),
        m("hpl", "HPL", "HPL", "HPL", "HPL", Generic, [72, 76, 82]),
        m("pvc", "PVC", "PVC", "PVC", "PVC", Generic, [244, 244, 240]),
        m("roofing", "dakbedekking", "roofing", "Dachabdichtung", "étanchéité de toiture", Generic, [42, 46, 49]),
        m("generic", "algemeen", "generic", "allgemein", "générique", Generic, GENERIC),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn starter_materials_have_unique_keys_and_every_name() {
        let materials = starter();
        let keys: HashSet<&str> = materials.iter().map(|m| m.key.as_str()).collect();
        assert_eq!(keys.len(), materials.len(), "keys are unique");
        for m in &materials {
            assert!(m.key.chars().all(|c| c.is_ascii_lowercase() || c == '-'), "{}", m.key);
            for lang in Lang::ALL {
                assert!(!m.name(lang).is_empty(), "{} has a name in {lang:?}", m.key);
            }
        }
    }

    #[test]
    fn hatches_follow_open_bim_studio() {
        let brick = HatchKind::Brick.pattern(DrawingStandard::Nen47);
        assert_eq!((brick.fill, brick.angle, brick.spacing), ([212, 144, 143], 45.0, 0.08));
        assert!(HatchKind::Insulation.pattern(DrawingStandard::Nen47).zigzag);
        assert!(HatchKind::Concrete.pattern(DrawingStandard::Din1356).crossed, "reinforced concrete is crossed in DIN 1356");
        assert_eq!(HatchKind::Timber.pattern(DrawingStandard::Iso128).angle, 135.0);
        assert_eq!(HatchKind::Concrete.pattern(DrawingStandard::Ansi).fill, [210, 210, 210], "a grey tone in ANSI");
        for standard in DrawingStandard::ALL {
            let steel = HatchKind::Steel.pattern(standard);
            assert_eq!((steel.fill, steel.spacing), ([0, 0, 0], 0.0), "steel is solid black in {standard:?}");
            let air = HatchKind::Air.pattern(standard);
            assert_eq!((air.fill, air.spacing), ([255, 255, 255], 0.0), "a cavity is left empty in {standard:?}");
            for kind in HatchKind::ALL {
                let p = kind.pattern(standard);
                assert!(p.spacing >= 0.0 && p.angle.is_finite());
            }
        }
    }

    #[test]
    fn a_material_has_its_colour_as_fractions() {
        let concrete = starter().into_iter().find(|m| m.key == "concrete").unwrap();
        assert_eq!(concrete.colour_f64(), [185.0 / 255.0; 3]);
        assert_eq!(concrete.name(Lang::De), "Beton");
    }
}
