//! Rule sets: what a project agrees to check its models on, kept in one file.
//!
//! A rule set names the information checks (IDS files, the built-in model checks, the BIM basis
//! ILS profile) and the clash tests (which elements against which, crossing or clearance, how
//! strict). It is a JSON file (`.rules.json`) that travels with the project; the app comes with
//! a few to start from, among them the Dutch BIM basis ILS and the norm of the
//! Rijksvastgoedbedrijf as IDS files of its own (`builtin:` paths, see [`builtin_ids`]).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The IDS files the app comes with: their `builtin:` name, what they are called and their text.
pub const BUILTIN_IDS: [(&str, &str, &str); 2] = [
    ("builtin:nl-bim-basis-ils", "NL BIM Basis ILS", include_str!("../../assets/ids/NL_BIM_Basis_ILS_v2.ids")),
    ("builtin:rvb-bim-norm", "RVB BIM-norm 1.1", include_str!("../../assets/ids/RVB_BIM_Norm_v1.1.ids")),
];

/// The text of an IDS a rule set names: one the app comes with, or a file.
pub fn ids_text(path: &Path) -> Result<String, String> {
    match BUILTIN_IDS.iter().find(|(key, ..)| Path::new(key) == path) {
        Some((.., text)) => Ok(text.to_string()),
        None => std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display())),
    }
}

/// What an IDS a rule set names is called: the name of one the app comes with, or the file's.
pub fn ids_name(path: &Path) -> String {
    match BUILTIN_IDS.iter().find(|(key, ..)| Path::new(key) == path) {
        Some((_, name, _)) => name.to_string(),
        None => path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
    }
}

fn builtin_ids(key: &str) -> PathBuf {
    PathBuf::from(key)
}

fn is_builtin(path: &Path) -> bool {
    path.to_str().is_some_and(|p| p.starts_with("builtin:"))
}

/// Which elements a clash test takes: those of the models, layers and classes named. An empty
/// list takes all; a name matches when the model's, layer's or class's name holds it, in any
/// case, and `*` at an end stands for anything (`IfcWall*`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Selector {
    pub models: Vec<String>,
    pub layers: Vec<String>,
    pub classes: Vec<String>,
    /// Elements whose name fits one of these (`*exit*`): a sign has no class of its own.
    pub names: Vec<String>,
    /// For a test of the moment, not kept in a file: only these elements, and all but these.
    #[serde(skip)]
    pub only: Vec<u64>,
    #[serde(skip)]
    pub except: Vec<u64>,
}

impl Selector {
    /// Elements of these classes.
    pub fn classes(classes: &[&str]) -> Selector {
        Selector { classes: classes.iter().map(|c| c.to_string()).collect(), ..Selector::default() }
    }

    /// Whether it takes every element.
    pub fn is_all(&self) -> bool {
        self.models.is_empty() && self.layers.is_empty() && self.classes.is_empty() && self.names.is_empty() && self.only.is_empty() && self.except.is_empty()
    }

    /// Whether it takes element `id` of this model, layer and class.
    pub fn takes_element(&self, id: u64, model: &str, layer: &str, class: &str) -> bool {
        (self.only.is_empty() || self.only.contains(&id)) && !self.except.contains(&id) && self.takes(model, layer, class)
    }

    /// Whether it takes an element of this model, layer and class.
    pub fn takes(&self, model: &str, layer: &str, class: &str) -> bool {
        let any = |list: &[String], name: &str| list.is_empty() || list.iter().any(|pattern| matches(pattern, name));
        any(&self.models, model) && any(&self.layers, layer) && any(&self.classes, class)
    }

    /// Whether it takes an element of this name: any, unless it names some.
    pub fn takes_name(&self, name: &str) -> bool {
        self.names.is_empty() || self.names.iter().any(|pattern| matches(pattern, name))
    }

    /// What it takes, in words: `IfcBeam, IfcColumn`, or `everything`.
    pub fn summary(&self, everything: &str) -> String {
        let parts: Vec<&str> = self.models.iter().chain(&self.layers).chain(&self.classes).chain(&self.names).map(String::as_str).collect();
        if parts.is_empty() { everything.to_string() } else { parts.join(", ") }
    }
}

/// Whether `name` fits `pattern`: holds it, or with `*` at an end starts or ends with it.
pub fn matches(pattern: &str, name: &str) -> bool {
    let (pattern, name) = (pattern.trim().to_lowercase(), name.to_lowercase());
    match (pattern.strip_prefix('*'), pattern.strip_suffix('*')) {
        (Some(rest), _) if rest.ends_with('*') => name.contains(rest.trim_end_matches('*')),
        (Some(end), _) => name.ends_with(end),
        (_, Some(start)) => name.starts_with(start),
        _ => name == pattern || (!pattern.is_empty() && name.contains(&pattern)),
    }
}

/// What a clash test looks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClashKind {
    /// Elements that cross.
    #[default]
    Hard,
    /// Elements that come too near.
    Clearance,
    /// Elements that have to be seen (signs, or what a camera watches from: the camera) with
    /// something standing in the line of sight to them.
    Sight,
}

/// A clash test: the elements of `a` against those of `b`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClashRule {
    pub name: String,
    pub a: Selector,
    pub b: Selector,
    pub kind: ClashKind,
    /// How far elements may cross (millimetres).
    pub tolerance_mm: f32,
    /// How near elements may come (millimetres), for a clearance test.
    pub clearance_mm: f32,
    /// Whether the test runs with the rule set.
    pub on: bool,
    /// Only between models: with several models open, the elements of one model are not tested
    /// against each other (the parts of one supplier's assembly touch by design).
    pub between_models: bool,
    /// Only the elements of these storeys, by name (`00*, 01*`; several with a comma between;
    /// empty: all). An element is of the highest storey that begins at or under its foot.
    pub storeys: String,
    /// Only the elements whose property `property` has the value `value` (`*` at an end
    /// stands for anything); no property: all.
    pub property: String,
    pub value: String,
    /// How far two classes may cross, where that is more than the test allows:
    /// `IfcPipe* x IfcWall* = 25; IfcCovering x IfcDuct* = 40` (millimetres).
    pub pair_tolerances: String,
    /// A sight-line test: from how far the element has to be seen (metres), and the share of
    /// the places looked from that may be without sight of it (percent).
    pub sight_m: f32,
    pub sight_blocked_pct: f32,
    /// A sight-line test: only the elements whose name fits one of these have to be seen
    /// (`*exit*, *vlucht*`; several with a comma between; empty: all the first side takes).
    pub sight_names: String,
}

impl Default for ClashRule {
    fn default() -> Self {
        ClashRule { name: String::new(), a: Selector::default(), b: Selector::default(), kind: ClashKind::Hard, tolerance_mm: 2.0, clearance_mm: 50.0, on: true, between_models: false, storeys: String::new(), property: String::new(), value: String::new(), pair_tolerances: String::new(), sight_m: 20.0, sight_blocked_pct: 50.0, sight_names: String::new() }
    }
}

impl ClashRule {
    /// The geometric test it asks for.
    pub fn test(&self) -> super::clash::Test {
        match self.kind {
            ClashKind::Hard => super::clash::Test::Hard { tolerance: self.tolerance_mm.max(0.0) / 1000.0 },
            ClashKind::Clearance => super::clash::Test::Clearance { distance: self.clearance_mm.max(0.0) / 1000.0 },
            // a sight-line test looks along lines, not at crossings
            ClashKind::Sight => super::clash::Test::Hard { tolerance: 0.0 },
        }
    }

    /// How far classes `a` and `b` may cross by the tolerances of their pair (metres), where
    /// one is given for them, either way round.
    pub fn pair_tolerance(&self, a: &str, b: &str) -> Option<f32> {
        self.pair_tolerances.split(';').find_map(|part| {
            let (pair, mm) = part.split_once('=')?;
            let (x, y) = pair.split_once(" x ").or_else(|| pair.split_once('×'))?;
            let fits = (matches(x, a) && matches(y, b)) || (matches(x, b) && matches(y, a));
            fits.then(|| mm.trim().replace(',', ".").parse::<f32>().ok()).flatten().map(|mm| mm.max(0.0) / 1000.0)
        })
    }

    /// Whether an element of this name is one a sight-line test wants seen.
    pub fn takes_sight_name(&self, name: &str) -> bool {
        let wanted: Vec<&str> = self.sight_names.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
        wanted.is_empty() || wanted.iter().any(|pattern| matches(pattern, name))
    }

    /// Whether a storey of this name is in the test's scope.
    pub fn takes_storey(&self, storey: &str) -> bool {
        let wanted: Vec<&str> = self.storeys.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
        wanted.is_empty() || wanted.iter().any(|pattern| matches(pattern, storey))
    }

    /// Whether both sides take the same elements: each pair is then tested once.
    pub fn within(&self) -> bool {
        self.a == self.b
    }
}

/// A discipline of the project: what it is called and which elements are its own (its model, its
/// layers or its classes).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Discipline {
    pub name: String,
    pub take: Selector,
}

/// A set of elements kept under a name: those of a selection, by the keys they keep from one
/// version of a model to the next (GlobalId, else the file's own id), or those whose property
/// `property` has the value `value` (`*` at an end stands for anything).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SelectionSet {
    pub name: String,
    pub keys: Vec<String>,
    pub property: String,
    pub value: String,
}

/// An information takeoff: which elements (what they are, and a property with a value when
/// `property` is set), grouped by up to two properties, with how many there are and their
/// length, area and volume.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Takeoff {
    pub name: String,
    pub take: Selector,
    pub property: String,
    pub value: String,
    /// The properties the rows are grouped by; none: a row for each element.
    pub group_by: Vec<String>,
}

/// A clash as the rule set remembers it from the last run: the two elements by the keys they
/// keep from one version of a model to the next, what was decided on it, and what it was called.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClashMemo {
    pub key: String,
    /// open, accepted or rejected.
    pub decision: String,
    pub rule: String,
    pub a: String,
    pub b: String,
}

/// A run of the clash tests: when, how many clashes it found, how many of them the run before
/// did not have, how many of that run's it no longer found, and how many are not accepted.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClashRun {
    pub date: String,
    pub total: usize,
    pub new: usize,
    pub resolved: usize,
    pub open: usize,
}

/// A rule set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RuleSet {
    pub name: String,
    pub description: String,
    /// IDS files, as paths beside the rule set's file or absolute.
    pub ids: Vec<PathBuf>,
    /// The built-in model checks: project, spatial structure, GlobalIds, units, proxies, materials.
    pub model_checks: bool,
    /// The BIM basis ILS profile: file name, storeys, NL-SfB, load bearing, external, fire rating.
    pub ils: bool,
    /// The pattern storey names must fit for the ILS profile; empty: the profile's own.
    pub storey_pattern: String,
    pub clashes: Vec<ClashRule>,
    /// The disciplines of the matrix, and which of them are tested against which: each pair by
    /// the names of its two disciplines, a discipline against itself too.
    pub disciplines: Vec<Discipline>,
    pub matrix: Vec<[String; 2]>,
    /// How far the elements of the matrix may cross (millimetres), and whether a pair of the
    /// matrix is only tested between models.
    pub matrix_tolerance_mm: f32,
    pub matrix_between_models: bool,
    pub selection_sets: Vec<SelectionSet>,
    pub takeoffs: Vec<Takeoff>,
    /// The clashes of the last run, with what was decided on them: the next run takes the
    /// decisions over and tells what is new and what is solved. And the runs so far.
    pub clash_memory: Vec<ClashMemo>,
    pub clash_runs: Vec<ClashRun>,
    /// The room checks: what they ask, in the units the rules give them.
    pub rooms: RoomRules,
}

/// What the checks of rooms, doors, escape routes and fire separations ask. The values are those
/// the Dutch building regulations ask of a dwelling or a building of most uses; a project sets
/// its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RoomRules {
    pub on: bool,
    /// The least free height of a room (metres).
    pub room_height: f32,
    /// The least free width and height of a door (metres).
    pub door_width: f32,
    pub door_height: f32,
    /// The longest walk from a point of a room to a way out of its storey: a door to outside or
    /// a stair (metres).
    pub escape_distance: f32,
    /// The highest threshold of a door (metres), the steepest ramp (the run for a rise of one)
    /// and the circle a wheelchair turns in, that each room has to hold (metres).
    pub threshold: f32,
    pub ramp_run: f32,
    pub turning_circle: f32,
}

impl Default for RoomRules {
    fn default() -> Self {
        RoomRules { on: false, room_height: 2.6, door_width: 0.85, door_height: 2.3, escape_distance: 30.0, threshold: 0.02, ramp_run: 12.0, turning_circle: 1.5 }
    }
}

impl Default for RuleSet {
    fn default() -> Self {
        RuleSet {
            name: String::new(),
            description: String::new(),
            ids: Vec::new(),
            model_checks: true,
            ils: false,
            storey_pattern: String::new(),
            clashes: Vec::new(),
            disciplines: Vec::new(),
            matrix: Vec::new(),
            matrix_tolerance_mm: 2.0,
            matrix_between_models: true,
            selection_sets: Vec::new(),
            takeoffs: Vec::new(),
            clash_memory: Vec::new(),
            clash_runs: Vec::new(),
            rooms: RoomRules::default(),
        }
    }
}

pub const STRUCTURE: [&str; 9] = ["IfcBeam", "IfcColumn", "IfcSlab", "IfcWall*", "IfcFooting", "IfcPile", "IfcMember", "IfcPlate", "IfcStair*"];
pub const SERVICES: [&str; 12] = ["IfcDuct*", "IfcPipe*", "IfcCable*", "IfcFlow*", "IfcDistribution*", "IfcAirTerminal*", "IfcValve*", "IfcPump*", "IfcFan*", "IfcTank*", "IfcSanitaryTerminal*", "IfcEnergyConversionDevice*"];
pub const ARCHITECTURE: [&str; 9] = ["IfcDoor", "IfcWindow", "IfcCovering", "IfcCurtainWall", "IfcRailing", "IfcRoof", "IfcRamp*", "IfcFurnishingElement", "IfcFurniture"];

impl RuleSet {
    /// Whether discipline `a` is tested against `b` in the matrix (either way round).
    pub fn in_matrix(&self, a: &str, b: &str) -> bool {
        self.matrix.iter().any(|[x, y]| (x == a && y == b) || (x == b && y == a))
    }

    /// Ticks the pair of disciplines `a` and `b` in the matrix, or takes it out.
    pub fn toggle_matrix(&mut self, a: &str, b: &str) {
        if self.in_matrix(a, b) {
            self.matrix.retain(|[x, y]| !((x == a && y == b) || (x == b && y == a)));
        } else {
            self.matrix.push([a.to_string(), b.to_string()]);
        }
    }

    /// The clash tests to run: those of the list that are on, then one for each pair of the
    /// matrix, in the order of the disciplines.
    pub fn clash_rules(&self) -> Vec<ClashRule> {
        let mut rules: Vec<ClashRule> = self.clashes.iter().filter(|r| r.on).cloned().collect();
        for (i, a) in self.disciplines.iter().enumerate() {
            for b in &self.disciplines[i..] {
                if !self.in_matrix(&a.name, &b.name) {
                    continue;
                }
                let name = if a.name == b.name { a.name.clone() } else { format!("{} × {}", a.name, b.name) };
                rules.push(ClashRule { name, a: a.take.clone(), b: b.take.clone(), tolerance_mm: self.matrix_tolerance_mm, between_models: self.matrix_between_models, ..ClashRule::default() });
            }
        }
        rules
    }

    /// The rule sets the app comes with: `dutch` gives their names and descriptions in Dutch.
    pub fn built_in(dutch: bool) -> Vec<RuleSet> {
        let t = |nl: &str, en: &str| if dutch { nl.to_string() } else { en.to_string() };
        let rule = |name: String, a: &[&str], b: &[&str], kind: ClashKind| ClashRule { name, a: Selector::classes(a), b: Selector::classes(b), kind, ..ClashRule::default() };
        // coordination is between the models of the disciplines
        let between = |rule: ClashRule| ClashRule { between_models: true, ..rule };
        vec![
            RuleSet {
                name: t("Basiscontrole", "Basic check"),
                description: t("De ingebouwde modelcontroles en dubbele of doorsnijdende elementen.", "The built-in model checks, and elements that are double or cross."),
                clashes: vec![rule(t("Alle elementen onderling", "All elements with each other"), &[], &[], ClashKind::Hard)],
                ..RuleSet::default()
            },
            RuleSet {
                name: t("NL BIM Basis ILS", "NL BIM Basis ILS"),
                description: t(
                    "De modelcontroles en de IDS van de BIM basis ILS: bouwlagen, proxy's, NL-SfB, dragend, buiten en materialen.",
                    "The model checks and the IDS of the BIM basis ILS: storeys, proxies, NL-SfB, load bearing, external and materials.",
                ),
                ids: vec![builtin_ids("builtin:nl-bim-basis-ils")],
                ..RuleSet::default()
            },
            RuleSet {
                name: t("RVB BIM-norm 1.1", "RVB BIM norm 1.1"),
                description: t(
                    "De modelcontroles en de IDS van de BIM-norm van het Rijksvastgoedbedrijf: projectgegevens, bouwlagen, classificatie en eigenschappen.",
                    "The model checks and the IDS of the BIM norm of the Rijksvastgoedbedrijf: project data, storeys, classification and properties.",
                ),
                ids: vec![builtin_ids("builtin:rvb-bim-norm")],
                ..RuleSet::default()
            },
            RuleSet {
                name: t("Coördinatie constructie en installaties", "Coordination of structure and services"),
                description: t("Installaties door de constructie, constructie onderling en vrije ruimte rond leidingen.", "Services through the structure, the structure with itself, and clearance around pipes and ducts."),
                model_checks: false,
                clashes: vec![
                    between(rule(t("Constructie tegen installaties", "Structure against services"), &STRUCTURE, &SERVICES, ClashKind::Hard)),
                    between(rule(t("Constructie onderling", "Structure with itself"), &STRUCTURE, &STRUCTURE, ClashKind::Hard)),
                    between(rule(t("Installaties onderling", "Services with each other"), &SERVICES, &SERVICES, ClashKind::Hard)),
                    ClashRule { on: false, ..rule(t("Vrije ruimte rond installaties", "Clearance around services"), &SERVICES, &STRUCTURE, ClashKind::Clearance) },
                    // the escape route signs: seen from most of their room
                    ClashRule {
                        on: false,
                        kind: ClashKind::Sight,
                        sight_names: "*vlucht*, *nooduitgang*, *exit*, *emergency*".into(),
                        ..rule(t("Vluchtwegaanduiding zichtbaar", "Escape route signs in sight"), &[], &[], ClashKind::Sight)
                    },
                ],
                disciplines: vec![
                    Discipline { name: t("Bouwkundig", "Architecture"), take: Selector::classes(&ARCHITECTURE) },
                    Discipline { name: t("Constructie", "Structure"), take: Selector::classes(&STRUCTURE) },
                    Discipline { name: t("Installaties", "Services"), take: Selector::classes(&SERVICES) },
                ],
                matrix: vec![[t("Bouwkundig", "Architecture"), t("Installaties", "Services")]],
                ..RuleSet::default()
            },
        ]
    }

    /// Reads a rule set file; the paths of its IDS files are taken from where it lies.
    pub fn read(path: &Path) -> Result<RuleSet, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut set: RuleSet = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("{}: {e}", path.display()))?;
        let folder = path.parent().unwrap_or(Path::new("."));
        for ids in &mut set.ids {
            if ids.is_relative() && !is_builtin(ids) {
                *ids = folder.join(&*ids);
            }
        }
        if set.name.trim().is_empty() {
            set.name = path.file_stem().map(|s| s.to_string_lossy().trim_end_matches(".rules").to_string()).unwrap_or_default();
        }
        Ok(set)
    }

    /// Writes the rule set to a file; IDS files beside or under it are named from there.
    pub fn write(&self, path: &Path) -> Result<(), String> {
        let folder = path.parent().unwrap_or(Path::new("."));
        let mut set = self.clone();
        for ids in &mut set.ids {
            if let Ok(inside) = ids.strip_prefix(folder) {
                *ids = inside.to_path_buf();
            }
        }
        let text = serde_json::to_string_pretty(&set).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selector_takes_by_model_layer_and_class() {
        let all = Selector::default();
        assert!(all.is_all() && all.takes("a.ifc", "Walls", "IfcWall"));
        let walls = Selector::classes(&["IfcWall*", "IfcSlab"]);
        assert!(walls.takes("a.ifc", "x", "IfcWallStandardCase") && walls.takes("a.ifc", "x", "ifcslab") && !walls.takes("a.ifc", "x", "IfcBeam"));
        let model = Selector { models: vec!["constructie".into()], ..Selector::default() };
        assert!(model.takes("3070_Constructie.ifc", "x", "IfcBeam") && !model.takes("3070_installaties.ifc", "x", "IfcBeam"));
        assert!(matches("*terminal", "IfcAirTerminal") && !matches("*terminal", "IfcAirTerminalBox") && matches("*term*", "IfcAirTerminalBox"));
    }

    #[test]
    fn a_rule_set_reads_back_from_its_file_with_its_ids_beside_it() {
        let folder = std::env::temp_dir().join(format!("o3v-rules-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("project.rules.json");
        let mut set = RuleSet::built_in(false).remove(3);
        set.ids = vec![folder.join("agreements.ids")];
        set.write(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"agreements.ids\""), "{text}");
        assert_eq!(RuleSet::read(&path).unwrap(), set);
        // a file that says little gets the rest as it is by default
        std::fs::write(&path, "{\"clashes\": [{\"name\": \"All\"}]}").unwrap();
        let little = RuleSet::read(&path).unwrap();
        assert_eq!((little.name.as_str(), little.model_checks, little.clashes[0].tolerance_mm, little.clashes[0].on), ("project", true, 2.0, true));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_norms_come_with_the_app_and_check_ifc_4_3_too() {
        let sets = RuleSet::built_in(true);
        let named: Vec<&Path> = sets.iter().flat_map(|s| &s.ids).map(PathBuf::as_path).collect();
        assert_eq!(named.len(), 2);
        for ids in named {
            let text = ids_text(ids).unwrap();
            assert!(text.contains("<specification") && !text.contains("ifcVersion=\"IFC2X3 IFC4\""), "{}", ids.display());
            assert!(!ids_name(ids).is_empty());
        }
        // a rule set naming one keeps the name as it is, not taken from beside the file
        let folder = std::env::temp_dir().join(format!("o3v-norms-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("rvb.rules.json");
        sets[2].write(&path).unwrap();
        assert_eq!(RuleSet::read(&path).unwrap().ids, sets[2].ids);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_matrix_makes_a_test_for_each_pair_ticked() {
        let mut set = RuleSet::built_in(false).remove(3);
        let tests = |set: &RuleSet| set.clash_rules().into_iter().map(|r| r.name).collect::<Vec<_>>();
        // the list's tests that are on, then the matrix
        assert_eq!(tests(&set), ["Structure against services", "Structure with itself", "Services with each other", "Architecture × Services"]);
        set.toggle_matrix("Services", "Architecture");
        set.toggle_matrix("Structure", "Structure");
        set.matrix_tolerance_mm = 10.0;
        let rules = set.clash_rules();
        assert_eq!(rules.last().map(|r| (r.name.as_str(), r.within(), r.tolerance_mm, r.between_models)), Some(("Structure", true, 10.0, true)));
        assert!(!set.in_matrix("Architecture", "Services") && set.in_matrix("Structure", "Structure"));
        // the matrix goes with the file
        let folder = std::env::temp_dir().join(format!("o3v-matrix-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("m.rules.json");
        set.write(&path).unwrap();
        assert_eq!(RuleSet::read(&path).unwrap(), set);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_test_has_tolerances_per_pair_of_classes_and_a_scope_of_storeys() {
        let rule = ClashRule { pair_tolerances: "IfcPipe* x IfcWall* = 25; IfcCovering × IfcDuct*=40,5".into(), storeys: "00*, 01 eerste".into(), ..ClashRule::default() };
        assert_eq!(rule.pair_tolerance("IfcWallStandardCase", "IfcPipeSegment"), Some(0.025));
        assert_eq!(rule.pair_tolerance("IfcCovering", "IfcDuctFitting"), Some(0.0405));
        assert_eq!(rule.pair_tolerance("IfcBeam", "IfcPipeSegment"), None);
        assert!(rule.takes_storey("00 begane grond") && rule.takes_storey("01 eerste verdieping") && !rule.takes_storey("02 dak"));
        assert!(ClashRule::default().takes_storey("anything"));
    }

    #[test]
    fn the_built_in_rule_sets_have_names_in_both_languages() {
        let (nl, en) = (RuleSet::built_in(true), RuleSet::built_in(false));
        assert_eq!(nl.len(), en.len());
        assert!(nl.iter().chain(&en).all(|s| !s.name.is_empty() && !s.description.is_empty()));
        assert!(en[3].clashes.iter().all(|c| !c.name.is_empty()) && en[3].clashes[1].within() && en[3].clashes[1].between_models && !en[0].clashes[0].between_models);
    }
}
