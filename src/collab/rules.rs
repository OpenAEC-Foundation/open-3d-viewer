//! Rule sets: what a project agrees to check its models on, kept in one file.
//!
//! A rule set names the information checks (IDS files, the built-in model checks, the BIM basis
//! ILS profile) and the clash tests (which elements against which, crossing or clearance, how
//! strict). It is a JSON file (`.rules.json`) that travels with the project; the app comes with
//! a few to start from.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Which elements a clash test takes: those of the models, layers and classes named. An empty
/// list takes all; a name matches when the model's, layer's or class's name holds it, in any
/// case, and `*` at an end stands for anything (`IfcWall*`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Selector {
    pub models: Vec<String>,
    pub layers: Vec<String>,
    pub classes: Vec<String>,
}

impl Selector {
    /// Elements of these classes.
    pub fn classes(classes: &[&str]) -> Selector {
        Selector { classes: classes.iter().map(|c| c.to_string()).collect(), ..Selector::default() }
    }

    /// Whether it takes every element.
    pub fn is_all(&self) -> bool {
        self.models.is_empty() && self.layers.is_empty() && self.classes.is_empty()
    }

    /// Whether it takes an element of this model, layer and class.
    pub fn takes(&self, model: &str, layer: &str, class: &str) -> bool {
        let any = |list: &[String], name: &str| list.is_empty() || list.iter().any(|pattern| matches(pattern, name));
        any(&self.models, model) && any(&self.layers, layer) && any(&self.classes, class)
    }

    /// What it takes, in words: `IfcBeam, IfcColumn`, or `everything`.
    pub fn summary(&self, everything: &str) -> String {
        let parts: Vec<&str> = self.models.iter().chain(&self.layers).chain(&self.classes).map(String::as_str).collect();
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
}

impl Default for ClashRule {
    fn default() -> Self {
        ClashRule { name: String::new(), a: Selector::default(), b: Selector::default(), kind: ClashKind::Hard, tolerance_mm: 2.0, clearance_mm: 50.0, on: true }
    }
}

impl ClashRule {
    /// The geometric test it asks for.
    pub fn test(&self) -> super::clash::Test {
        match self.kind {
            ClashKind::Hard => super::clash::Test::Hard { tolerance: self.tolerance_mm.max(0.0) / 1000.0 },
            ClashKind::Clearance => super::clash::Test::Clearance { distance: self.clearance_mm.max(0.0) / 1000.0 },
        }
    }

    /// Whether both sides take the same elements: each pair is then tested once.
    pub fn within(&self) -> bool {
        self.a == self.b
    }
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
}

impl Default for RuleSet {
    fn default() -> Self {
        RuleSet { name: String::new(), description: String::new(), ids: Vec::new(), model_checks: true, ils: false, storey_pattern: String::new(), clashes: Vec::new() }
    }
}

const STRUCTURE: [&str; 9] = ["IfcBeam", "IfcColumn", "IfcSlab", "IfcWall*", "IfcFooting", "IfcPile", "IfcMember", "IfcPlate", "IfcStair*"];
const SERVICES: [&str; 12] = ["IfcDuct*", "IfcPipe*", "IfcCable*", "IfcFlow*", "IfcDistribution*", "IfcAirTerminal*", "IfcValve*", "IfcPump*", "IfcFan*", "IfcTank*", "IfcSanitaryTerminal*", "IfcEnergyConversionDevice*"];

impl RuleSet {
    /// The rule sets the app comes with: `dutch` gives their names and descriptions in Dutch.
    pub fn built_in(dutch: bool) -> Vec<RuleSet> {
        let t = |nl: &str, en: &str| if dutch { nl.to_string() } else { en.to_string() };
        let rule = |name: String, a: &[&str], b: &[&str], kind: ClashKind| ClashRule { name, a: Selector::classes(a), b: Selector::classes(b), kind, ..ClashRule::default() };
        vec![
            RuleSet {
                name: t("Basiscontrole", "Basic check"),
                description: t("De ingebouwde modelcontroles en dubbele of doorsnijdende elementen.", "The built-in model checks, and elements that are double or cross."),
                clashes: vec![rule(t("Alle elementen onderling", "All elements with each other"), &[], &[], ClashKind::Hard)],
                ..RuleSet::default()
            },
            RuleSet {
                name: t("BIM basis ILS", "BIM basis ILS"),
                description: t("De modelcontroles en de afspraken van de BIM basis ILS.", "The model checks and the agreements of the BIM basis ILS."),
                ils: true,
                ..RuleSet::default()
            },
            RuleSet {
                name: t("Coördinatie constructie en installaties", "Coordination of structure and services"),
                description: t("Installaties door de constructie, constructie onderling en vrije ruimte rond leidingen.", "Services through the structure, the structure with itself, and clearance around pipes and ducts."),
                model_checks: false,
                clashes: vec![
                    rule(t("Constructie tegen installaties", "Structure against services"), &STRUCTURE, &SERVICES, ClashKind::Hard),
                    rule(t("Constructie onderling", "Structure with itself"), &STRUCTURE, &STRUCTURE, ClashKind::Hard),
                    rule(t("Installaties onderling", "Services with each other"), &SERVICES, &SERVICES, ClashKind::Hard),
                    ClashRule { on: false, ..rule(t("Vrije ruimte rond installaties", "Clearance around services"), &SERVICES, &STRUCTURE, ClashKind::Clearance) },
                ],
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
            if ids.is_relative() {
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
        let mut set = RuleSet::built_in(false).remove(2);
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
    fn the_built_in_rule_sets_have_names_in_both_languages() {
        let (nl, en) = (RuleSet::built_in(true), RuleSet::built_in(false));
        assert_eq!(nl.len(), en.len());
        assert!(nl.iter().chain(&en).all(|s| !s.name.is_empty() && !s.description.is_empty()));
        assert!(en[2].clashes.iter().all(|c| !c.name.is_empty()) && en[2].clashes[1].within());
    }
}
