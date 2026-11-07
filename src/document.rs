//! The project document in IfcX (IFC5) form, kept beside the loaded model: what the app
//! recognises in a model and what the user adds, in the shape of IfcX nodes, so that it can be
//! written as `.ifcx` (the native format to come). Levels for now; floor plans and sheets follow.
//!
//! Headless: no GUI or GPU, so the MCP server and a later BIM kernel crate can use it as it is.

use crate::model::Model;
use serde_json::{json, Value};

/// The IfcX class of a level.
pub const STOREY_CLASS: &str = "IfcBuildingStorey";
/// The buildingSMART identifier of [`STOREY_CLASS`] in IFC5.
pub const STOREY_URI: &str = "https://identifier.buildingsmart.org/uri/buildingsmart/ifc/5/class/IfcBuildingStorey";

/// A level (storey) of the building.
#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    /// The IfcX path of the node, a UUID.
    pub path: String,
    pub name: String,
    /// Metres, Z up, in the frame of the model's display coordinates.
    pub elevation: f64,
    /// Made in the app, not read from the model: only these can be removed.
    pub added: bool,
}

/// Why a level was not added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelError {
    EmptyName,
    /// Another level has this name (case does not count).
    DuplicateName,
    /// The elevation is not a finite number.
    BadElevation,
}

/// The project document.
#[derive(Debug, Clone, Default)]
pub struct Document {
    /// Low to high; equal elevations by name.
    levels: Vec<Level>,
    /// Levels added so far, for the paths of new ones.
    added: u64,
}

impl Document {
    /// The document of a model: its levels, recognised as IfcX levels.
    pub fn from_model(model: &Model) -> Self {
        let mut document = Self::default();
        document.recognise(model);
        document
    }

    /// The levels, low to high.
    pub fn levels(&self) -> &[Level] {
        &self.levels
    }

    /// Takes the levels of `model` (in place of those recognised before) and keeps the added
    /// ones, except an added level whose name the model now has.
    pub fn recognise(&mut self, model: &Model) {
        let mut levels: Vec<Level> = model
            .project
            .levels
            .iter()
            .filter(|(_, z)| z.is_finite())
            .map(|(name, z)| {
                let elevation = micrometres(*z as f64);
                Level { path: uuid(&format!("level/{name}/{elevation}")), name: name.clone(), elevation, added: false }
            })
            .collect();
        let taken = |name: &str, levels: &[Level]| levels.iter().any(|l| same_name(&l.name, name));
        let kept: Vec<Level> = self.levels.drain(..).filter(|l| l.added).collect();
        for level in kept {
            if !taken(&level.name, &levels) {
                levels.push(level);
            }
        }
        self.levels = levels;
        self.sort();
    }

    /// Adds a level and gives its index in [`Self::levels`].
    pub fn add_level(&mut self, name: &str, elevation: f64) -> Result<usize, LevelError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(LevelError::EmptyName);
        }
        if self.levels.iter().any(|l| same_name(&l.name, name)) {
            return Err(LevelError::DuplicateName);
        }
        if !elevation.is_finite() {
            return Err(LevelError::BadElevation);
        }
        self.added += 1;
        let path = uuid(&format!("added/{}/{name}", self.added));
        self.levels.push(Level { path: path.clone(), name: name.to_string(), elevation: micrometres(elevation), added: true });
        self.sort();
        Ok(self.levels.iter().position(|l| l.path == path).unwrap_or_default())
    }

    /// Removes the added level at `path`; a level of the model stays (false).
    pub fn remove_level(&mut self, path: &str) -> bool {
        let before = self.levels.len();
        self.levels.retain(|l| !(l.added && l.path == path));
        self.levels.len() < before
    }

    /// The levels added in the app, low to high.
    pub fn added_levels(&self) -> impl Iterator<Item = &Level> {
        self.levels.iter().filter(|l| l.added)
    }

    /// What a new level gets unless the user types otherwise: `<word> <n>` with the first free
    /// `n` from the number of levels up, 3 m above the highest level (0 without levels).
    pub fn next_level(&self, word: &str) -> (String, f64) {
        let mut n = self.levels.len();
        let name = loop {
            let name = format!("{word} {n}");
            if !self.levels.iter().any(|l| same_name(&l.name, &name)) {
                break name;
            }
            n += 1;
        };
        let elevation = self.levels.iter().map(|l| l.elevation).reduce(f64::max).map_or(0.0, |z| z + 3.0);
        (name, elevation)
    }

    /// The levels as IfcX nodes: class, name, elevation and the matching placement.
    pub fn ifcx_nodes(&self) -> Vec<Value> {
        self.levels
            .iter()
            .map(|l| {
                json!({
                    "path": l.path,
                    "attributes": {
                        "bsi::ifc::class": {"code": STOREY_CLASS, "uri": STOREY_URI},
                        "bsi::ifc::prop::Name": l.name,
                        "bsi::ifc::prop::Elevation": l.elevation,
                        "usd::xformop": {"transform": [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, l.elevation, 1.0]]},
                    }
                })
            })
            .collect()
    }

    fn sort(&mut self) {
        self.levels.sort_by(|a, b| a.elevation.total_cmp(&b.elevation).then_with(|| a.name.cmp(&b.name)));
    }
}

/// An elevation as the user types it: metres, a comma or a point, an optional `m`.
pub fn parse_elevation(text: &str) -> Option<f64> {
    let text = text.trim();
    let text = text.strip_suffix('m').unwrap_or(text).trim().replace(',', ".");
    text.parse::<f64>().ok().filter(|z| z.is_finite())
}

fn same_name(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// Rounded to micrometres: an f32 elevation of a loader reads as the value it was written as.
fn micrometres(z: f64) -> f64 {
    (z * 1e6).round() / 1e6
}

/// A UUID (version 8, RFC 9562) derived from `seed`: the same seed gives the same path.
fn uuid(seed: &str) -> String {
    let fnv = |offset: u64| seed.bytes().fold(offset, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3));
    let (hi, lo) = (fnv(0xcbf2_9ce4_8422_2325), fnv(0x84222325_cbf29ce4));
    let hi = (hi & !0xf000) | 0x8000; // version 8
    let lo = (lo & !(0b11 << 62)) | (0b10 << 62); // variant RFC 9562
    format!("{:08x}-{:04x}-{:04x}-{:04x}-{:012x}", hi >> 32, (hi >> 16) & 0xffff, hi & 0xffff, lo >> 48, lo & 0xffff_ffff_ffff)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn model_with(levels: &[(&str, f32)]) -> Model {
        let mut model = Model::new(PathBuf::from("house.rvt"), "Revit", Vec::new());
        model.project.levels = levels.iter().map(|(n, z)| (n.to_string(), *z)).collect();
        model
    }

    fn names(document: &Document) -> Vec<&str> {
        document.levels().iter().map(|l| l.name.as_str()).collect()
    }

    #[test]
    fn recognises_the_levels_of_a_model_low_to_high_with_stable_paths() {
        let model = model_with(&[("01 first floor", 3.1), ("00 ground floor", 0.0), ("-1 basement", -2.8)]);
        let document = Document::from_model(&model);
        assert_eq!(names(&document), ["-1 basement", "00 ground floor", "01 first floor"]);
        let first = &document.levels()[2];
        assert_eq!(first.elevation, 3.1, "an f32 elevation reads as written");
        assert!(!first.added);
        // a UUID, the same every time the model is read
        assert_eq!(first.path.len(), 36);
        assert_eq!(first.path.matches('-').count(), 4);
        assert_eq!(&first.path[14..15], "8", "version 8");
        assert_eq!(Document::from_model(&model).levels(), document.levels());
        let paths: std::collections::HashSet<&str> = document.levels().iter().map(|l| l.path.as_str()).collect();
        assert_eq!(paths.len(), 3);
    }

    #[test]
    fn adds_levels_in_their_place_by_elevation() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0), ("02", 6.0)]));
        assert_eq!(document.add_level("  01  ", 3.0), Ok(1));
        assert_eq!(names(&document), ["00", "01", "02"]);
        let added = &document.levels()[1];
        assert!(added.added);
        assert_eq!(added.elevation, 3.0);
        assert_ne!(added.path, document.levels()[0].path);
        assert_eq!(document.add_level("roof", 9.5), Ok(3));
        assert_eq!(document.added_levels().count(), 2);
    }

    #[test]
    fn refuses_an_empty_or_taken_name_and_a_bad_elevation() {
        let mut document = Document::from_model(&model_with(&[("Ground Floor", 0.0)]));
        assert_eq!(document.add_level("   ", 3.0), Err(LevelError::EmptyName));
        assert_eq!(document.add_level("ground floor", 3.0), Err(LevelError::DuplicateName));
        assert_eq!(document.add_level("Roof", f64::NAN), Err(LevelError::BadElevation));
        assert_eq!(document.add_level("Roof", f64::INFINITY), Err(LevelError::BadElevation));
        assert_eq!(document.levels().len(), 1);
    }

    #[test]
    fn removes_only_added_levels() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0)]));
        document.add_level("01", 3.0).unwrap();
        let model_level = document.levels()[0].path.clone();
        let added = document.levels()[1].path.clone();
        assert!(!document.remove_level(&model_level));
        assert!(document.remove_level(&added));
        assert!(!document.remove_level(&added));
        assert_eq!(names(&document), ["00"]);
    }

    #[test]
    fn recognising_again_keeps_added_levels_unless_the_model_has_the_name() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0)]));
        document.add_level("01", 3.0).unwrap();
        document.add_level("Roof", 9.0).unwrap();
        // a second model added: its levels join, the added ones stay
        document.recognise(&model_with(&[("00", 0.0), ("02", 6.0), ("roof", 9.2)]));
        assert_eq!(names(&document), ["00", "01", "02", "roof"]);
        assert_eq!(document.added_levels().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["01"]);
    }

    #[test]
    fn suggests_the_next_free_name_three_metres_above_the_highest_level() {
        assert_eq!(Document::default().next_level("Level"), ("Level 0".to_string(), 0.0));
        let mut document = Document::from_model(&model_with(&[("Level 0", 0.0), ("Level 2", 3.0)]));
        assert_eq!(document.next_level("Level"), ("Level 3".to_string(), 6.0));
        document.add_level("Level 3", 7.5).unwrap();
        assert_eq!(document.next_level("Niveau"), ("Niveau 3".to_string(), 10.5));
    }

    #[test]
    fn parses_elevations_as_typed() {
        assert_eq!(parse_elevation("3"), Some(3.0));
        assert_eq!(parse_elevation(" -2,75 "), Some(-2.75));
        assert_eq!(parse_elevation("+6.000 m"), Some(6.0));
        assert_eq!(parse_elevation("6m"), Some(6.0));
        assert_eq!(parse_elevation(""), None);
        assert_eq!(parse_elevation("three"), None);
        assert_eq!(parse_elevation("inf"), None);
    }

    #[test]
    fn levels_are_ifcx_storey_nodes() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0)]));
        document.add_level("01", 3.25).unwrap();
        let nodes = document.ifcx_nodes();
        assert_eq!(nodes.len(), 2);
        let node = &nodes[1];
        assert_eq!(node["path"], document.levels()[1].path.as_str());
        let attributes = &node["attributes"];
        assert_eq!(attributes["bsi::ifc::class"]["code"], "IfcBuildingStorey");
        assert_eq!(attributes["bsi::ifc::class"]["uri"], STOREY_URI);
        assert_eq!(attributes["bsi::ifc::prop::Name"], "01");
        assert_eq!(attributes["bsi::ifc::prop::Elevation"], 3.25);
        assert_eq!(attributes["usd::xformop"]["transform"][3], json!([0.0, 0.0, 3.25, 1.0]));
    }
}
