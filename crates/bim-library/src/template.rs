//! View templates: what a drawing of a kind shows in a phase, and how.
//!
//! A view template is for a kind of drawing of the drawing standard (a floor plan, a section),
//! a design phase and a scale. It says per class of element whether it shows, in what detail,
//! with which pens, hatched or not and in which colour, cut and seen; and which drawing set
//! annotates it ([`crate::annotate`]). The house style says how things look in general (a pen,
//! a text style); a view template says what a drawing of a kind shows and picks from those.
//!
//! - A **rule** is a filter ([`crate::annotate::Filter`]: IFC class, NL-SfB code, type,
//!   property) and a look. The rules are tried in order and the first that takes an element
//!   gives its look; what no rule takes has the template's own look (`otherwise`).
//! - A **look** leaves open what it does not set: the kind of drawing's own pens and hatching
//!   hold there.
//! - The **detail** of an element rises with the scale and the phase: an outline, one hatch for
//!   the whole element, its layers each with their hatch, or all of it with its parts.
//!
//! [`ViewTemplate::look`] says how one element is drawn; it needs no view.
//!
//! In IFCX a view template is a node with its English name and `openaec::viewtemplate` (all of
//! it). A library holds its view templates ([`crate::Library::view_templates`]).

use crate::annotate::{Described, Filter};
use crate::material::Names;
use crate::{uuid, Lang, LibraryError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The attribute of a view template's node.
pub const VIEW_TEMPLATE: &str = "openaec::viewtemplate";

/// The design phases of a Dutch building project, as the standard task description (DNR-STB)
/// has them, and the permit application between the final and the technical design.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    /// Structuurontwerp (SO).
    Sketch,
    /// Voorontwerp (VO).
    Preliminary,
    /// Definitief ontwerp (DO).
    Final,
    /// The application for the building permit (omgevingsvergunning).
    Permit,
    /// Technisch ontwerp (TO).
    Technical,
    /// Uitvoeringsgereed ontwerp (UO).
    Construction,
}

impl Phase {
    pub const ALL: [Phase; 6] = [Phase::Sketch, Phase::Preliminary, Phase::Final, Phase::Permit, Phase::Technical, Phase::Construction];

    /// Its Dutch abbreviation, as drawings are named and numbered by it.
    pub fn code(self) -> &'static str {
        match self {
            Phase::Sketch => "SO",
            Phase::Preliminary => "VO",
            Phase::Final => "DO",
            Phase::Permit => "OV",
            Phase::Technical => "TO",
            Phase::Construction => "UO",
        }
    }

    /// Its name in Dutch and in English.
    pub fn name(self, lang: Lang) -> &'static str {
        let (nl, en) = match self {
            Phase::Sketch => ("structuurontwerp", "sketch design"),
            Phase::Preliminary => ("voorontwerp", "preliminary design"),
            Phase::Final => ("definitief ontwerp", "final design"),
            Phase::Permit => ("omgevingsvergunning", "building permit"),
            Phase::Technical => ("technisch ontwerp", "technical design"),
            Phase::Construction => ("uitvoeringsgereed ontwerp", "construction design"),
        };
        if lang == Lang::Nl {
            nl
        } else {
            en
        }
    }
}

/// How much of an element a drawing shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Detail {
    /// Its outline, filled in one tone where it is cut.
    Outline,
    /// One hatch for the whole element, by its main material.
    #[default]
    Coarse,
    /// Its layers, each with its material's hatch.
    Layers,
    /// All of it: its layers and its parts (frames, studs, fixings).
    Full,
}

/// How an element is drawn. What is none is left to the kind of drawing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Look {
    /// Not drawn at all.
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<Detail>,
    /// The pen (a key of the drawing standard) of what is cut, and of what is seen beyond.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beyond: Option<String>,
    /// Whether what is cut is hatched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hatched: Option<bool>,
    /// A colour of its own for its lines and its fill, in place of its material's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<[u8; 3]>,
    /// Drawn faint (in halftone): what is there already, or of another discipline.
    #[serde(default, skip_serializing_if = "is_false")]
    pub faint: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl Look {
    /// Not drawn.
    pub fn hidden() -> Self {
        Self { hidden: true, ..Self::default() }
    }

    /// `self` where it sets something, else `under`.
    pub fn over(&self, under: &Look) -> Look {
        Look {
            hidden: self.hidden || under.hidden,
            detail: self.detail.or(under.detail),
            cut: self.cut.clone().or_else(|| under.cut.clone()),
            beyond: self.beyond.clone().or_else(|| under.beyond.clone()),
            hatched: self.hatched.or(under.hatched),
            colour: self.colour.or(under.colour),
            faint: self.faint || under.faint,
        }
    }
}

/// A rule of a view template: the elements a filter takes, and their look.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub key: String,
    pub names: Names,
    #[serde(default)]
    pub filter: Filter,
    #[serde(default)]
    pub look: Look,
}

/// A view template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewTemplate {
    /// Stable, lower case with hyphens.
    pub key: String,
    pub names: Names,
    /// The key of the kind of drawing of the drawing standard it is for ("floor-plan").
    pub kind: String,
    pub phase: Phase,
    /// The scale it is made for, as 1 to this.
    pub scale: u32,
    /// The look of what no rule takes.
    #[serde(default)]
    pub otherwise: Look,
    pub rules: Vec<Rule>,
    /// Whether dimensions and whether levels (peil) are written.
    #[serde(default)]
    pub dimensions: bool,
    #[serde(default)]
    pub levels: bool,
    /// The key of the drawing set that annotates it, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drawing_set: Option<String>,
}

impl ViewTemplate {
    pub fn name(&self, lang: Lang) -> &str {
        self.names.get(lang)
    }

    /// How `element` is drawn: the look of the first rule that takes it over the template's
    /// own, else the template's own.
    pub fn look(&self, element: &Described) -> Look {
        match self.rules.iter().find(|rule| rule.filter.takes(element)) {
            Some(rule) => rule.look.over(&self.otherwise),
            None => self.otherwise.clone(),
        }
    }

    /// Whether it holds together: a key, a kind, a scale, and rules each once.
    pub fn check(&self) -> Result<(), LibraryError> {
        if self.key.trim().is_empty() || self.kind.trim().is_empty() {
            return Err(LibraryError::NoKey);
        }
        if self.scale == 0 {
            return Err(LibraryError::UnknownSetting("a scale".into()));
        }
        for (k, rule) in self.rules.iter().enumerate() {
            if rule.key.trim().is_empty() || self.rules[..k].iter().any(|r| r.key == rule.key) {
                return Err(LibraryError::Duplicate(rule.key.clone()));
            }
        }
        Ok(())
    }
}

/// The path of the node of view template `key`.
pub fn path(key: &str) -> String {
    uuid(&format!("openaec-library/viewtemplate/{key}"))
}

/// The node of a view template.
pub fn node(template: &ViewTemplate) -> Value {
    json!({"path": path(&template.key), "attributes": {"bsi::ifc::prop::Name": template.name(Lang::En), VIEW_TEMPLATE: template}})
}

/// How much of an element a drawing at 1 : `scale` shows, by common Dutch practice: an outline
/// at 1:200 and smaller, one hatch at 1:100, the layers at 1:50, all of it from 1:20.
pub fn detail_at(scale: u32) -> Detail {
    match scale {
        0..=20 => Detail::Full,
        21..=50 => Detail::Layers,
        51..=100 => Detail::Coarse,
        _ => Detail::Outline,
    }
}

/// The view templates a library starts with: for each phase a floor plan, a section and an
/// elevation at the first scale the standard task description (DNR-STB 2014) or the law names
/// for it, and a detail at 1:5 from the final design on. What they show per phase and per scale
/// and where that comes from is in `docs/standards/drawing-rules-per-phase.md`.
///
/// - The sketch design has a plan alone, at 1:200; the preliminary design is at 1:200, the
///   final design, the permit and the technical design at 1:100, the construction design at
///   1:50.
/// - Detail follows the scale ([`detail_at`]); what is cut is hatched from 1:100, never in an
///   elevation.
/// - Fixed furnishing and grid lines show from the final design; services show faint from the
///   technical design; the parts of elements (studs, fixings) show from 1:50.
/// - Dimensions and levels are written from the final design, as the permit asks them.
/// - A plan and a section name the drawing set that annotates them where the library has one
///   for their scale.
pub fn starter() -> Vec<ViewTemplate> {
    use Phase::*;
    let classes = |classes: &[&str]| Filter { classes: classes.iter().map(|c| c.to_string()).collect(), ..Filter::default() };
    let rule = |key: &str, nl: &str, en: &str, de: &str, fr: &str, filter: Filter, look: Look| Rule { key: key.into(), names: Names::new(nl, en, de, fr), filter, look };
    let faint = || Look { faint: true, ..Look::default() };
    let kinds = [
        ("floor-plan", ["plattegrond", "floor plan", "Grundriss", "plan d'étage"]),
        ("section", ["doorsnede", "section", "Schnitt", "coupe"]),
        ("elevation", ["aanzicht", "elevation", "Ansicht", "élévation"]),
        ("detail", ["detail", "detail", "Detail", "détail"]),
    ];
    let of = |kind: usize, phase: Phase, scale: u32| -> ViewTemplate {
        let (key, names) = kinds[kind];
        let detail = detail_at(scale);
        let name = |k: usize| format!("{} {} 1:{scale}", names[k], phase.code());
        let hatched = scale <= 100 && key != "elevation";
        let openings = match detail {
            Detail::Outline => Detail::Outline,
            Detail::Coarse => Detail::Coarse,
            _ => Detail::Full,
        };
        let mut rules = vec![
            rule("furnishing", "vaste inrichting", "fixed furnishing", "feste Einrichtung", "aménagement fixe", classes(&["IfcFurnishingElement", "IfcFurniture", "IfcSanitaryTerminal"]), if phase >= Final { faint() } else { Look::hidden() }),
            rule("services", "installaties", "services", "Haustechnik", "installations", classes(&["IfcPipeSegment", "IfcDuctSegment", "IfcCableCarrierSegment", "IfcFlowSegment", "IfcFlowTerminal"]), if phase >= Technical { faint() } else { Look::hidden() }),
            rule("grids", "stramienen", "grid lines", "Rasterlinien", "trames", classes(&["IfcGridAxis"]), if phase >= Final { Look::default() } else { Look::hidden() }),
            rule("openings", "deuren en ramen", "doors and windows", "Türen und Fenster", "portes et fenêtres", classes(&["IfcDoor", "IfcWindow"]), Look { detail: Some(openings), hatched: Some(false), ..Look::default() }),
            rule("parts", "onderdelen", "parts", "Bauteile", "pièces", classes(&["IfcMember", "IfcPlate", "IfcFastener", "IfcMechanicalFastener"]), if detail >= Detail::Layers { Look::default() } else { Look::hidden() }),
        ];
        if key == "elevation" {
            // what stands behind the facade is not cut: an elevation shows what is seen
            rules.push(rule("rooms", "ruimten", "rooms", "Räume", "pièces", classes(&["IfcSpace"]), Look::hidden()));
        }
        let drawing_set = match (key, scale, phase >= Final) {
            ("floor-plan", 100, true) => Some("architectural-plan-100"),
            ("floor-plan", 50, true) => Some("architectural-plan-50"),
            ("section", 50, true) => Some("architectural-section-50"),
            _ => None,
        };
        ViewTemplate {
            key: format!("{key}-{}-{scale}", phase.code().to_lowercase()),
            names: Names::new(&name(0), &name(1), &name(2), &name(3)),
            kind: key.into(),
            phase,
            scale,
            otherwise: Look { detail: Some(detail), hatched: Some(hatched), ..Look::default() },
            rules,
            dimensions: phase >= Final,
            levels: phase >= Final && key != "detail",
            drawing_set: drawing_set.map(str::to_string),
        }
    };
    let mut templates = vec![of(0, Sketch, 200)];
    for (phase, scale) in [(Preliminary, 200), (Final, 100), (Permit, 100), (Technical, 100), (Construction, 50)] {
        templates.extend((0..3).map(|kind| of(kind, phase, scale)));
        if phase >= Final {
            templates.push(of(3, phase, 5));
        }
    }
    templates
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn element(class: &str) -> Described {
        Described { class: class.into(), ..Described::default() }
    }

    fn template(key: &str) -> ViewTemplate {
        starter().into_iter().find(|t| t.key == key).unwrap_or_else(|| panic!("{key}"))
    }

    #[test]
    fn the_starter_templates_hold_together_and_are_for_the_dutch_standard() {
        let standard = crate::drawing::Standard::netherlands();
        let sets = crate::annotate::starter();
        let templates = starter();
        assert_eq!(templates.len(), 20, "a plan in the sketch design, three views in five phases, a detail from the final design on");
        assert_eq!(templates.iter().map(|t| t.key.as_str()).collect::<HashSet<_>>().len(), templates.len());
        for t in &templates {
            t.check().unwrap_or_else(|e| panic!("{}: {e}", t.key));
            assert!(standard.kinds.iter().any(|k| k.key == t.kind), "{}: no kind {}", t.key, t.kind);
            // the drawing set it names is one of the library's, for its kind and its scale
            if let Some(set) = &t.drawing_set {
                let of = sets.iter().find(|s| s.key == *set).unwrap_or_else(|| panic!("{}: no drawing set {set}", t.key));
                assert_eq!((of.kind.as_str(), of.scale), (t.kind.as_str(), Some(t.scale)), "{}", t.key);
            }
        }
        // the scales of the standard task description and of the law, per phase
        let plans: Vec<(Phase, u32)> = templates.iter().filter(|t| t.kind == "floor-plan").map(|t| (t.phase, t.scale)).collect();
        assert_eq!(plans, [(Phase::Sketch, 200), (Phase::Preliminary, 200), (Phase::Final, 100), (Phase::Permit, 100), (Phase::Technical, 100), (Phase::Construction, 50)]);
        assert!(templates.iter().filter(|t| t.kind == "detail").all(|t| t.scale == 5 && t.phase >= Phase::Final));
        assert_eq!((template("floor-plan-do-100").name(Lang::Nl), template("section-uo-50").name(Lang::En)), ("plattegrond DO 1:100", "section UO 1:50"));
        assert_eq!(Phase::ALL.map(Phase::code), ["SO", "VO", "DO", "OV", "TO", "UO"]);
    }

    #[test]
    fn detail_rises_with_the_scale() {
        assert_eq!([500, 200, 100, 50, 20, 5, 1].map(detail_at), [Detail::Outline, Detail::Outline, Detail::Coarse, Detail::Layers, Detail::Full, Detail::Full, Detail::Full]);
        let wall = element("IfcWall");
        let look = |key: &str| template(key).look(&wall);
        // a wall: an outline without hatch in the sketch, one hatch in the final design, its
        // layers in the construction design, all of it in a detail; never hatched in an elevation
        assert_eq!((look("floor-plan-so-200").detail, look("floor-plan-so-200").hatched), (Some(Detail::Outline), Some(false)));
        assert_eq!((look("floor-plan-do-100").detail, look("floor-plan-do-100").hatched), (Some(Detail::Coarse), Some(true)));
        assert_eq!((look("floor-plan-uo-50").detail, look("detail-to-5").detail), (Some(Detail::Layers), Some(Detail::Full)));
        assert_eq!(look("elevation-do-100").hatched, Some(false));
    }

    #[test]
    fn what_shows_follows_the_phase() {
        let shows = |key: &str, class: &str| {
            let look = template(key).look(&element(class));
            (!look.hidden, look.faint)
        };
        // furnishing and grid lines from the final design, services faint from the technical one
        assert_eq!((shows("floor-plan-vo-200", "IfcFurniture"), shows("floor-plan-do-100", "IfcFurniture")), ((false, false), (true, true)));
        assert_eq!((shows("floor-plan-vo-200", "IfcGridAxis"), shows("floor-plan-ov-100", "IfcGridAxis")), ((false, false), (true, false)));
        assert_eq!((shows("floor-plan-do-100", "IfcDuctSegment"), shows("floor-plan-to-100", "IfcDuctSegment")), ((false, false), (true, true)));
        // the parts of elements from 1:50
        assert_eq!((shows("section-to-100", "IfcMember"), shows("section-uo-50", "IfcMember")), ((false, false), (true, false)));
        // a room shows in a plan, not in an elevation
        assert_eq!((shows("floor-plan-vo-200", "IfcSpace"), shows("elevation-vo-200", "IfcSpace")), ((true, false), (false, false)));
        // doors and windows: an opening at 1:200, with their frames at 1:50
        let door = |key: &str| template(key).look(&element("IfcDoor")).detail;
        assert_eq!((door("floor-plan-vo-200"), door("floor-plan-do-100"), door("floor-plan-uo-50")), (Some(Detail::Outline), Some(Detail::Coarse), Some(Detail::Full)));
        // dimensions and levels from the final design, as the permit asks them
        let written = |key: &str| (template(key).dimensions, template(key).levels);
        assert_eq!((written("floor-plan-vo-200"), written("floor-plan-ov-100"), written("detail-do-5")), ((false, false), (true, true), (true, false)));
        assert_eq!((template("floor-plan-ov-100").drawing_set.as_deref(), template("floor-plan-uo-50").drawing_set.as_deref(), template("floor-plan-vo-200").drawing_set), (Some("architectural-plan-100"), Some("architectural-plan-50"), None));
    }

    #[test]
    fn a_rules_look_stands_over_the_templates_own_and_a_template_reads_back() {
        let mut t = template("floor-plan-do-100");
        // an office draws loadbearing walls in a heavier pen: the first rule that takes a wall
        t.rules.insert(0, Rule { key: "bearing".into(), names: Names::same("dragend"), filter: Filter { classes: vec!["IfcWall".into()], property: Some(("LoadBearing".into(), "true".into())), ..Filter::default() }, look: Look { cut: Some("0.7".into()), ..Look::default() } });
        t.check().unwrap();
        let bearing = Described { class: "IfcWall".into(), props: vec![("LoadBearing".into(), "true".into())], ..Described::default() };
        assert_eq!(t.look(&bearing), Look { cut: Some("0.7".into()), detail: Some(Detail::Coarse), hatched: Some(true), ..Look::default() });
        assert_eq!(t.look(&element("IfcWall")).cut, None);
        // what does not hold together is refused
        let mut twice = t.clone();
        twice.rules.push(twice.rules[0].clone());
        assert!(twice.check().is_err() && ViewTemplate { scale: 0, ..t.clone() }.check().is_err() && ViewTemplate { key: String::new(), ..t.clone() }.check().is_err());
        let written = node(&t);
        assert_eq!(written["attributes"]["bsi::ifc::prop::Name"], "floor plan DO 1:100");
        // a look writes what it sets and no more
        assert_eq!(written["attributes"][VIEW_TEMPLATE]["rules"][0]["look"], json!({"cut": "0.7"}));
        assert_eq!(serde_json::from_value::<ViewTemplate>(written["attributes"][VIEW_TEMPLATE].clone()).unwrap(), t);
    }
}
