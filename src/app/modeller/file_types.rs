//! The type of a wall taken over from an opened file: its layers become a build-up of the
//! project's library, so the editable wall is made of what the file's wall was made of.
//!
//! - **Materials.** A layer's material is the library's material of the same name (in any of its
//!   languages, capitals aside), else a new one named as the file names it, with the file's colour
//!   and a hatch guessed from its name; a name that says air or cavity makes the layer a cavity.
//!   A layer without a material is of the material "unknown".
//! - **The type.** The library's wall type with the same materials and thicknesses in the same
//!   order is reused; else one is made, its id after the file's type name.
//! - **The side.** A build-up's layers run from the exterior face in, the exterior on the right
//!   walking from the wall's start to its end. A file's wall with its exterior on the left is
//!   therefore taken over with its ends changed round, and its openings counted from the other
//!   end; it stands where it stood.
//! - Layers thinner than 0.1 mm (membranes drawn without thickness) are left out. When what is
//!   left is not as thick as the wall, within a millimetre, the wall gets no build-up: it keeps
//!   its thickness as one layer.

use crate::model::SourceLayer;
use bim_core::element::PlanarForm;
use bim_core::{BuildUpDef, PlanarDef};
use bim_library::{BuildUp, HatchKind, Lang, Library, Material, Role};

/// A layer thinner than this (metres) is left out, as the library takes none thinner.
const THINNEST: f64 = 1e-4;
/// The layers must be as thick together as the wall, within this (metres).
const SAME_WALL: f64 = 1e-3;
/// The colour of a material the file gives none for.
const NO_COLOUR: [u8; 3] = [175, 182, 195];
/// The key of the material of a layer without one.
const UNKNOWN: &str = "unknown";

/// A name as a key: lower case, letters and digits, hyphens between.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// `id`, or the first of `id-2`, `id-3`, ... that `taken` says is free.
fn free(id: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut fresh = id.to_string();
    let mut n = 1;
    while taken(&fresh) {
        n += 1;
        fresh = format!("{id}-{n}");
    }
    fresh
}

/// How a material is hatched, by what its name says.
fn hatch_of(name: &str) -> HatchKind {
    let name = name.to_lowercase();
    let says = |words: &[&str]| words.iter().any(|w| name.contains(w));
    if says(&["lucht", "spouw", "air", "cavity", "luft"]) {
        HatchKind::Air
    } else if says(&["kalkzandsteen", "sand-lime", "sand lime", "kalksandstein", "silico"]) {
        HatchKind::SandLime
    } else if says(&["isolatie", "insulation", "dämm", "isolant", "wol", "wool", "pir", "eps", "xps"]) {
        HatchKind::Insulation
    } else if says(&["beton", "concrete"]) {
        HatchKind::Concrete
    } else if says(&["metselwerk", "baksteen", "brick", "masonry", "mauerwerk", "maçonnerie"]) {
        HatchKind::Brick
    } else if says(&["hout", "timber", "wood", "holz", "bois", "multiplex", "osb"]) {
        HatchKind::Timber
    } else if says(&["staal", "steel", "stahl", "acier"]) {
        HatchKind::Steel
    } else if says(&["gips", "gypsum", "plaster", "plâtre", "stuc"]) {
        HatchKind::Gypsum
    } else {
        HatchKind::Generic
    }
}

/// The key of the library's material for `layer`: the one of that name, else one added for it.
fn material_of(library: &mut Library, layer: &SourceLayer) -> Option<String> {
    let name = layer.material.trim();
    if name.is_empty() {
        if library.material(UNKNOWN).is_none() {
            let names = bim_library::Names::new("onbekend", "unknown", "unbekannt", "inconnu");
            library.add_material(Material { key: UNKNOWN.into(), names, hatch: HatchKind::Generic, colour: NO_COLOUR }).ok()?;
        }
        return Some(UNKNOWN.into());
    }
    let same = |m: &Material| [Lang::Nl, Lang::En, Lang::De, Lang::Fr].iter().any(|l| m.name(*l).to_lowercase() == name.to_lowercase());
    if let Some(m) = library.materials.iter().find(|m| same(m)) {
        return Some(m.key.clone());
    }
    let base = Some(slug(name)).filter(|s| !s.is_empty()).unwrap_or_else(|| "material".into());
    let key = free(&base, |k| library.material(k).is_some());
    let material = Material { key: key.clone(), names: bim_library::Names::same(name), hatch: hatch_of(name), colour: layer.colour.unwrap_or(NO_COLOUR) };
    library.add_material(material).ok()?;
    Some(key)
}

/// The build-up of a wall of a file, `thickness` thick, of type `type_name` with `layers` (from
/// the exterior in): the library's wall type of those layers, made when it has none. `None` when
/// the file gives no layers, or layers that are not the wall's thickness together.
pub(in crate::app) fn file_build_up(library: &mut Library, type_name: &str, thickness: f64, layers: &[SourceLayer]) -> Option<BuildUpDef> {
    let kept: Vec<&SourceLayer> = layers.iter().filter(|l| l.thickness >= THINNEST).collect();
    if kept.is_empty() || (kept.iter().map(|l| l.thickness).sum::<f64>() - thickness).abs() > SAME_WALL {
        return None;
    }
    let mut made = Vec::new();
    for layer in kept {
        made.push(bim_library::Layer::new(&material_of(library, layer)?, layer.thickness));
    }
    let wanted = BuildUp::new("", Role::Wall, made);
    let id = match library.matching(&wanted) {
        Some(same) => same.id.clone(),
        None => {
            let base = format!("wall.{}", Some(slug(type_name)).filter(|s| !s.is_empty()).unwrap_or_else(|| "file".into()));
            let id = free(&base, |i| library.buildup(i).is_some());
            library.add_buildup(BuildUp { id: id.clone(), ..wanted }).ok()?;
            id
        }
    };
    BuildUpDef::from_library(library, &id)
}

/// `wall` with its ends changed round: it stands where it stood, its openings counted from its
/// other end, and what was on its left is on its right.
pub(in crate::app) fn turned_round(mut wall: PlanarDef) -> PlanarDef {
    if let PlanarForm::Wall { start, end, .. } = &mut wall.form {
        let length = start.distance(*end);
        std::mem::swap(start, end);
        for o in &mut wall.openings {
            o.along = length - o.along - o.width;
        }
        wall.openings.sort_by(|a, b| a.along.total_cmp(&b.along));
    }
    wall
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec2;

    fn layer(material: &str, thickness: f64, colour: Option<[u8; 3]>) -> SourceLayer {
        SourceLayer { material: material.into(), thickness, function: "Structure".into(), colour }
    }

    #[test]
    fn a_files_layers_become_a_wall_type_that_is_reused() {
        let mut library = Library::starter();
        let (materials, types) = (library.materials.len(), library.buildups.len());
        // "Metselwerk" is the library's brickwork; the two others are the file's own
        let layers = [layer("Metselwerk", 0.1, None), layer("Luchtspouw 40", 0.04, None), layer("Wandplaat X", 0.06, Some([10, 20, 30])), layer("", 0.1, None), layer("Folie", 0.0, None)];
        let b = file_build_up(&mut library, "Spouwmuur 300", 0.3, &layers).expect("a build-up");
        assert_eq!(b.id, "wall.spouwmuur-300");
        assert_eq!(b.layers.iter().map(|l| l.material.as_str()).collect::<Vec<_>>(), ["brick", "luchtspouw-40", "wandplaat-x", "unknown"]);
        assert_eq!(b.layers.iter().map(|l| l.cavity).collect::<Vec<_>>(), [false, true, false, false]);
        assert!((b.thickness() - 0.3).abs() < 1e-12);
        assert_eq!((library.materials.len(), library.buildups.len()), (materials + 3, types + 1));
        let plate = library.material("wandplaat-x").unwrap();
        assert_eq!((plate.name(Lang::En), plate.colour, plate.hatch), ("Wandplaat X", [10, 20, 30], HatchKind::Generic));
        // a second wall of the same layers, whatever its type is called, is of the same type
        let again = file_build_up(&mut library, "Another name", 0.3, &layers).unwrap();
        assert_eq!(again.id, b.id);
        assert_eq!((library.materials.len(), library.buildups.len()), (materials + 3, types + 1));
        // other layers under the same type name: a type of its own
        let other = file_build_up(&mut library, "Spouwmuur 300", 0.2, &[layer("Metselwerk", 0.2, None)]).unwrap();
        assert_ne!(other.id, b.id);
    }

    #[test]
    fn layers_that_are_not_the_walls_thickness_give_no_build_up() {
        let mut library = Library::starter();
        let before = library.clone();
        assert!(file_build_up(&mut library, "T", 0.3, &[]).is_none());
        assert!(file_build_up(&mut library, "T", 0.3, &[layer("Metselwerk", 0.1, None)]).is_none());
        assert_eq!(library, before, "nothing added");
    }

    #[test]
    fn a_wall_turned_round_stands_where_it_stood() {
        let door = bim_core::Opening { along: 1.0, sill: 0.0, width: 0.9, height: 2.1 };
        let form = PlanarForm::Wall { start: DVec2::ZERO, end: DVec2::new(6.0, 0.0), base: 0.0, height: 3.0 };
        let wall = turned_round(PlanarDef { form, thickness: 0.3, openings: vec![door], build_up: None });
        let PlanarForm::Wall { start, end, .. } = wall.form else { panic!() };
        assert_eq!((start, end), (DVec2::new(6.0, 0.0), DVec2::ZERO));
        // the door from 1.0 to 1.9 m of the old start is from 4.1 to 5.0 m of the new
        assert!((wall.openings[0].along - 4.1).abs() < 1e-12);
    }
}
