//! The project document in IfcX (IFC5) form, kept beside the loaded model: what the app
//! recognises in a model and what the user adds, in the shape of IfcX nodes, so that it can be
//! written as `.ifcx` (the native format to come): levels and the floor plans made on them; sheets
//! follow.
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

/// How far a floor plan cuts above its level (metres), as is usual for floor plans.
pub const PLAN_CUT: f64 = 1.2;
/// How far a floor plan looks below its level (metres), so the floor it stands on shows.
pub const PLAN_DEPTH: f64 = 0.3;

/// A floor plan: the model seen from above, cut at [`PlanView::cut`] above its level.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanView {
    /// The IfcX path, a UUID.
    pub path: String,
    pub name: String,
    /// The path of its level.
    pub level: String,
    /// The cut above the level (metres).
    pub cut: f64,
    /// How far it looks below the level (metres).
    pub depth: f64,
    /// Its dimensions, shown wherever it is placed.
    pub dimensions: Vec<Dimension>,
    /// What it shows of the model in plan: lower left and upper right (x, y, metres); `None`:
    /// all of it.
    pub crop: Option<([f64; 2], [f64; 2])>,
}

/// A dimension on a floor plan: the distance between two points of the plan (x, y in the
/// model's frame, metres).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dimension {
    pub from: [f64; 2],
    pub to: [f64; 2],
}

impl Dimension {
    /// Its length (metres).
    pub fn length(&self) -> f64 {
        (self.to[0] - self.from[0]).hypot(self.to[1] - self.from[1])
    }
}

/// A3 landscape, the paper of a new sheet (metres).
pub const A3_LANDSCAPE: [f64; 2] = [0.420, 0.297];
/// The ISO papers a sheet can have, landscape: name, width and height (metres).
pub const PAPERS: [(&str, [f64; 2]); 5] =
    [("A4", [0.297, 0.210]), ("A3", A3_LANDSCAPE), ("A2", [0.594, 0.420]), ("A1", [0.841, 0.594]), ("A0", [1.189, 0.841])];
/// The scales a placed view can have (1 : this).
pub const SCALES: [f64; 6] = [20.0, 50.0, 100.0, 200.0, 500.0, 1000.0];
/// The scale a view gets when it is placed on a sheet (1 : this).
pub const SHEET_SCALE: f64 = 100.0;

/// A sheet: paper with views placed on it.
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    /// The IfcX path, a UUID.
    pub path: String,
    pub number: String,
    pub name: String,
    /// Number and name, as a tab shows them.
    pub title: String,
    /// Width and height (metres).
    pub paper: [f64; 2],
    pub views: Vec<PlacedView>,
}

/// A view on a sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedView {
    /// The path of the floor plan.
    pub plan: String,
    /// Where its middle is on the paper (metres from the lower left corner).
    pub at: [f64; 2],
    /// 1 : `scale`.
    pub scale: f64,
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
    /// Floor plans, in the order they were made.
    plans: Vec<PlanView>,
    /// Sheets, in the order they were made.
    sheets: Vec<Sheet>,
    /// Levels and views made so far, for the paths of new ones.
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
        self.drop_orphan_plans();
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
        self.drop_orphan_plans();
        self.levels.len() < before
    }

    /// The level at `path`.
    pub fn level(&self, path: &str) -> Option<&Level> {
        self.levels.iter().find(|l| l.path == path)
    }

    /// The floor plans, in the order they were made.
    pub fn plans(&self) -> &[PlanView] {
        &self.plans
    }

    /// The floor plan at `path`.
    pub fn plan(&self, path: &str) -> Option<&PlanView> {
        self.plans.iter().find(|p| p.path == path)
    }

    /// Makes a floor plan of the level at `path`, named after the level (with a number when the
    /// level has one already), and gives its path.
    pub fn add_plan(&mut self, level: &str) -> Option<String> {
        let base = self.level(level)?.name.clone();
        let mut name = base.clone();
        let mut n = 1;
        while self.plans.iter().any(|p| same_name(&p.name, &name)) {
            n += 1;
            name = format!("{base} ({n})");
        }
        self.added += 1;
        let path = uuid(&format!("plan/{}/{level}", self.added));
        self.plans.push(PlanView { path: path.clone(), name, level: level.to_string(), cut: PLAN_CUT, depth: PLAN_DEPTH, dimensions: Vec::new(), crop: None });
        Some(path)
    }

    /// Removes the floor plan at `path`, and it from the sheets.
    pub fn remove_plan(&mut self, path: &str) -> bool {
        let before = self.plans.len();
        self.plans.retain(|p| p.path != path);
        self.drop_orphan_plans();
        self.plans.len() < before
    }

    /// Adds a dimension between two points to a floor plan; two points less than a millimetre
    /// apart, or not finite, measure nothing.
    pub fn add_dimension(&mut self, plan: &str, from: [f64; 2], to: [f64; 2]) -> bool {
        let dimension = Dimension { from, to };
        if !(from.iter().chain(&to).all(|v| v.is_finite()) && dimension.length() >= 0.001) {
            return false;
        }
        let Some(plan) = self.plans.iter_mut().find(|p| p.path == plan) else { return false };
        plan.dimensions.push(dimension);
        true
    }

    /// Removes a dimension of a floor plan.
    pub fn remove_dimension(&mut self, plan: &str, index: usize) -> bool {
        let Some(plan) = self.plans.iter_mut().find(|p| p.path == plan) else { return false };
        if index >= plan.dimensions.len() {
            return false;
        }
        plan.dimensions.remove(index);
        true
    }

    /// The sheets, in the order they were made.
    pub fn sheets(&self) -> &[Sheet] {
        &self.sheets
    }

    /// The sheet at `path`.
    pub fn sheet(&self, path: &str) -> Option<&Sheet> {
        self.sheets.iter().find(|s| s.path == path)
    }

    /// Makes an empty A3 sheet numbered after the last (`A01`, `A02`, ..) and named `name`, and
    /// gives its path.
    pub fn add_sheet(&mut self, name: &str) -> String {
        let mut n = self.sheets.len() + 1;
        while self.sheets.iter().any(|s| s.number == format!("A{n:02}")) {
            n += 1;
        }
        self.added += 1;
        let path = uuid(&format!("sheet/{}/{n}", self.added));
        let number = format!("A{n:02}");
        let title = format!("{number} {name}");
        self.sheets.push(Sheet { path: path.clone(), number, name: name.to_string(), title, paper: A3_LANDSCAPE, views: Vec::new() });
        path
    }

    /// Removes the sheet at `path`.
    pub fn remove_sheet(&mut self, path: &str) -> bool {
        let before = self.sheets.len();
        self.sheets.retain(|s| s.path != path);
        self.sheets.len() < before
    }

    /// Places a floor plan in the middle of a sheet at 1:100 and gives its index on the sheet.
    pub fn place_plan(&mut self, sheet: &str, plan: &str) -> Option<usize> {
        self.plan(plan)?;
        let sheet = self.sheets.iter_mut().find(|s| s.path == sheet)?;
        let at = [sheet.paper[0] / 2.0, sheet.paper[1] / 2.0];
        sheet.views.push(PlacedView { plan: plan.to_string(), at, scale: SHEET_SCALE });
        Some(sheet.views.len() - 1)
    }

    /// Moves a placed view so that its middle is at `at`, kept on the paper.
    pub fn move_placed(&mut self, sheet: &str, index: usize, at: [f64; 2]) -> bool {
        let Some(sheet) = self.sheets.iter_mut().find(|s| s.path == sheet) else { return false };
        let paper = sheet.paper;
        let Some(view) = sheet.views.get_mut(index) else { return false };
        if !(at[0].is_finite() && at[1].is_finite()) {
            return false;
        }
        view.at = [at[0].clamp(0.0, paper[0]), at[1].clamp(0.0, paper[1])];
        true
    }

    /// Sets the scale of a placed view (1 : `scale`).
    pub fn set_scale(&mut self, sheet: &str, index: usize, scale: f64) -> bool {
        if !(scale.is_finite() && scale > 0.0) {
            return false;
        }
        let Some(view) = self.sheets.iter_mut().find(|s| s.path == sheet).and_then(|s| s.views.get_mut(index)) else { return false };
        view.scale = scale;
        true
    }

    /// Gives a sheet another paper; its views stay on it.
    pub fn set_paper(&mut self, sheet: &str, paper: [f64; 2]) -> bool {
        if !(paper.iter().all(|v| v.is_finite() && *v > 0.0)) {
            return false;
        }
        let Some(sheet) = self.sheets.iter_mut().find(|s| s.path == sheet) else { return false };
        sheet.paper = paper;
        for view in &mut sheet.views {
            view.at = [view.at[0].min(paper[0]), view.at[1].min(paper[1])];
        }
        true
    }

    /// Sets how far a floor plan cuts above its level (metres, above its view depth).
    pub fn set_cut(&mut self, plan: &str, cut: f64) -> bool {
        let Some(plan) = self.plans.iter_mut().find(|p| p.path == plan) else { return false };
        if !(cut.is_finite() && cut > -plan.depth) {
            return false;
        }
        plan.cut = cut;
        true
    }

    /// Crops a floor plan to a rectangle in plan (lower left, upper right; at least 10 cm each
    /// way), or shows all of it again (`None`).
    pub fn set_crop(&mut self, plan: &str, crop: Option<([f64; 2], [f64; 2])>) -> bool {
        if let Some((lo, hi)) = crop {
            if !(lo.iter().chain(&hi).all(|v| v.is_finite()) && hi[0] - lo[0] >= 0.1 && hi[1] - lo[1] >= 0.1) {
                return false;
            }
        }
        let Some(plan) = self.plans.iter_mut().find(|p| p.path == plan) else { return false };
        plan.crop = crop;
        true
    }

    /// Takes a view off a sheet.
    pub fn remove_placed(&mut self, sheet: &str, index: usize) -> bool {
        let Some(sheet) = self.sheets.iter_mut().find(|s| s.path == sheet) else { return false };
        if index >= sheet.views.len() {
            return false;
        }
        sheet.views.remove(index);
        true
    }

    /// The heights a floor plan shows, from low to high (metres, the model's frame).
    pub fn plan_range(&self, plan: &PlanView) -> Option<(f64, f64)> {
        let z = self.level(&plan.level)?.elevation;
        Some((z - plan.depth, z + plan.cut))
    }

    /// A floor plan goes with its level, and a placed view with its floor plan.
    fn drop_orphan_plans(&mut self) {
        let levels = &self.levels;
        self.plans.retain(|p| levels.iter().any(|l| l.path == p.level));
        let plans = &self.plans;
        for sheet in &mut self.sheets {
            sheet.views.retain(|v| plans.iter().any(|p| p.path == v.plan));
        }
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
    fn makes_floor_plans_of_levels_named_after_them() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0), ("01", 3.0)]));
        let first = document.levels()[1].path.clone();
        let plan = document.add_plan(&first).expect("a plan");
        let again = document.add_plan(&first).expect("a second plan");
        assert_ne!(plan, again);
        let names: Vec<&str> = document.plans().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["01", "01 (2)"]);
        let view = document.plan(&plan).unwrap();
        assert_eq!((view.level.as_str(), view.cut, view.depth), (first.as_str(), PLAN_CUT, PLAN_DEPTH));
        // from just below the floor up to the cut
        assert_eq!(document.plan_range(view), Some((3.0 - PLAN_DEPTH, 3.0 + PLAN_CUT)));
        assert_eq!(document.add_plan("no such level"), None);
        assert!(document.remove_plan(&again));
        assert_eq!(document.plans().len(), 1);
    }

    #[test]
    fn a_floor_plan_goes_with_its_level() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0)]));
        document.add_level("Roof", 6.0).unwrap();
        let ground = document.levels()[0].path.clone();
        let roof = document.levels()[1].path.clone();
        document.add_plan(&ground).unwrap();
        document.add_plan(&roof).unwrap();
        document.remove_level(&roof);
        assert_eq!(document.plans().iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["00"]);
        // the model read again keeps the plan of a level it still has, not of one it lost
        document.recognise(&model_with(&[("00", 0.0), ("01", 3.0)]));
        assert_eq!(document.plans().len(), 1);
        document.recognise(&model_with(&[("01", 3.0)]));
        assert!(document.plans().is_empty());
    }

    #[test]
    fn sheets_hold_floor_plans_that_move_on_the_paper() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0), ("01", 3.0)]));
        let sheet = document.add_sheet("Floor plans");
        let second = document.add_sheet("Details");
        let numbers: Vec<&str> = document.sheets().iter().map(|s| s.number.as_str()).collect();
        assert_eq!(numbers, ["A01", "A02"]);
        assert_eq!(document.sheet(&sheet).unwrap().paper, A3_LANDSCAPE);
        let ground = document.levels()[0].path.clone();
        let plan = document.add_plan(&ground).unwrap();
        assert_eq!(document.place_plan(&sheet, &plan), Some(0));
        assert_eq!(document.place_plan(&sheet, "no plan"), None);
        assert_eq!(document.place_plan("no sheet", &plan), None);
        let placed = &document.sheet(&sheet).unwrap().views[0];
        assert_eq!((placed.at, placed.scale), ([0.21, 0.1485], SHEET_SCALE));
        // moved, and kept on the paper
        assert!(document.move_placed(&sheet, 0, [0.1, 0.08]));
        assert_eq!(document.sheet(&sheet).unwrap().views[0].at, [0.1, 0.08]);
        assert!(document.move_placed(&sheet, 0, [-1.0, 9.0]));
        assert_eq!(document.sheet(&sheet).unwrap().views[0].at, [0.0, 0.297]);
        assert!(!document.move_placed(&sheet, 1, [0.1, 0.1]));
        // a removed plan leaves its sheets
        document.remove_plan(&plan);
        assert!(document.sheet(&sheet).unwrap().views.is_empty());
        assert!(document.remove_sheet(&second));
        assert_eq!(document.add_sheet("Again").len(), 36);
        assert_eq!(document.sheets()[1].number, "A02");
    }

    #[test]
    fn scale_paper_and_cut_can_change() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0)]));
        let level = document.levels()[0].path.clone();
        let plan = document.add_plan(&level).unwrap();
        let sheet = document.add_sheet("Plans");
        document.place_plan(&sheet, &plan).unwrap();
        assert!(document.set_scale(&sheet, 0, 50.0));
        assert!(!document.set_scale(&sheet, 0, 0.0));
        assert!(!document.set_scale(&sheet, 3, 50.0));
        assert_eq!(document.sheet(&sheet).unwrap().views[0].scale, 50.0);
        // a smaller paper keeps the view on it
        assert!(document.set_paper(&sheet, PAPERS[0].1));
        let s = document.sheet(&sheet).unwrap();
        assert_eq!((s.paper, s.views[0].at), ([0.297, 0.210], [0.21, 0.1485]));
        assert!(document.set_paper(&sheet, [0.1, 0.1]));
        assert_eq!(document.sheet(&sheet).unwrap().views[0].at, [0.1, 0.1]);
        assert!(!document.set_paper(&sheet, [0.0, 0.1]));
        // the cut, not below the view depth
        assert!(document.set_cut(&plan, 2.0));
        assert_eq!(document.plan_range(document.plan(&plan).unwrap()), Some((-PLAN_DEPTH, 2.0)));
        assert!(!document.set_cut(&plan, -0.5));
        assert!(!document.set_cut(&plan, f64::NAN));
    }

    #[test]
    fn a_floor_plan_is_cropped_to_a_rectangle() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0)]));
        let level = document.levels()[0].path.clone();
        let plan = document.add_plan(&level).unwrap();
        assert!(document.set_crop(&plan, Some(([0.0, 0.0], [4.0, 3.0]))));
        assert_eq!(document.plan(&plan).unwrap().crop, Some(([0.0, 0.0], [4.0, 3.0])));
        assert!(!document.set_crop(&plan, Some(([0.0, 0.0], [0.05, 3.0]))), "under 10 cm");
        assert!(!document.set_crop(&plan, Some(([2.0, 0.0], [1.0, 3.0]))), "inside out");
        assert!(document.set_crop(&plan, None));
        assert_eq!(document.plan(&plan).unwrap().crop, None);
    }

    #[test]
    fn floor_plans_keep_their_dimensions() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0)]));
        let level = document.levels()[0].path.clone();
        let plan = document.add_plan(&level).unwrap();
        assert!(document.add_dimension(&plan, [0.0, 0.0], [3.6, 0.0]));
        assert!(document.add_dimension(&plan, [0.0, 0.0], [3.0, 4.0]));
        assert!(!document.add_dimension(&plan, [1.0, 1.0], [1.0, 1.0005]), "less than a millimetre");
        assert!(!document.add_dimension(&plan, [0.0, f64::NAN], [1.0, 1.0]));
        assert!(!document.add_dimension("no plan", [0.0, 0.0], [1.0, 0.0]));
        let lengths: Vec<f64> = document.plan(&plan).unwrap().dimensions.iter().map(Dimension::length).collect();
        assert_eq!(lengths, [3.6, 5.0]);
        assert!(document.remove_dimension(&plan, 0));
        assert!(!document.remove_dimension(&plan, 5));
        assert_eq!(document.plan(&plan).unwrap().dimensions.len(), 1);
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
