//! Drawing sets: the rules by which a drawing of a kind is annotated by itself.
//!
//! A drawing set is a named set for a kind of drawing (an architectural floor plan at 1:100, a
//! structural plan): its label types, and its rules, each saying which elements get which label
//! or which dimension. A modeller applies the rules to a view; the library keeps them, so an
//! office sets up its drawings once.
//!
//! - A **label type** says what a label reads from its element (fields, joined by a separator),
//!   how it is written (a text style of the drawing standard, a frame, a leader) and where it
//!   stands by default.
//! - A **filter** picks elements: by their IFC class, by the group of their NL-SfB code, by the
//!   name of their type, and by a property and its value. What a filter leaves open, it does
//!   not ask; an empty filter takes every element.
//! - A **rule** is a filter and what the elements it takes get: a label of a type, or a
//!   dimension (of their thickness, their length, their openings, or the grids).
//! - The rules are tried in order. An element gets the label of the first label rule that takes
//!   it, and the dimensions of every dimension rule that does.
//!
//! [`DrawingSet::annotate`] says what one element gets; it needs no view, so the rules are
//! tested and previewed without one.
//!
//! In IFCX a drawing set is a node with its English name and `openaec::drawingset` (all of it).
//! A library holds its drawing sets ([`crate::Library::drawing_sets`]).

use crate::material::Names;
use crate::{uuid, Lang, LibraryError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The attribute of a drawing set's node.
pub const DRAWING_SET: &str = "openaec::drawingset";

/// What a label reads from its element.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "field", content = "key")]
pub enum LabelField {
    /// The element's own name or mark.
    Name,
    /// The name of its type.
    Type,
    /// Its NL-SfB code.
    Code,
    Material,
    /// The thickness of a wall, a floor or a roof, in millimetres.
    Thickness,
    /// The profile of a column or a beam ("HEA 200").
    Profile,
    Length,
    Area,
    Level,
    /// A property of the element, by its name.
    Property(String),
    /// A fixed text ("Ø", "d=").
    Text(String),
}

/// What is drawn round a label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Frame {
    #[default]
    None,
    Rectangle,
    Rounded,
    Circle,
    Hexagon,
}

/// Where a label stands by its element until it is moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Place {
    /// On the element, at its middle.
    #[default]
    Centre,
    /// Along the element, above its line (a beam, a wall).
    Along,
    Above,
    Below,
    Left,
    Right,
}

/// A label type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabelType {
    pub key: String,
    pub names: Names,
    pub fields: Vec<LabelField>,
    /// What stands between two fields that have something to say.
    #[serde(default = "space")]
    pub separator: String,
    /// The key of a text style of the drawing standard.
    pub text_style: String,
    #[serde(default)]
    pub frame: Frame,
    /// A line from the label to its element.
    #[serde(default)]
    pub leader: bool,
    #[serde(default)]
    pub place: Place,
}

fn space() -> String {
    " ".into()
}

/// Which elements a rule takes. Every part that is given must hold.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Filter {
    /// IFC classes ("IfcWall"); empty: any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub classes: Vec<String>,
    /// NL-SfB codes: an element of code 21.22 is of "21" and of "21.2"; empty: any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub codes: Vec<String>,
    /// A text the name of its type contains, whatever its case; none: any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_contains: Option<String>,
    /// A property and its value (whatever its case); an empty value: the property is there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub property: Option<(String, String)>,
}

/// What a dimension rule measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Measure {
    /// Across the element: a wall's thickness, layer by layer where `chain` is set.
    Thickness,
    /// Along the element.
    Length,
    /// The openings in the element, and the stretches between them.
    Openings,
    /// From the element to the nearest grids.
    Grids,
}

/// What the elements a rule takes get.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "gives")]
pub enum Gives {
    /// A label of this type (its key).
    Label { label: String },
    /// A dimension; `chain`: one line of dimensions end to end, else each on its own.
    Dimension { measure: Measure, chain: bool },
}

/// A rule: a filter and what its elements get.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub key: String,
    pub names: Names,
    #[serde(default)]
    pub filter: Filter,
    #[serde(flatten)]
    pub gives: Gives,
}

/// A drawing set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrawingSet {
    /// Stable, lower case with hyphens.
    pub key: String,
    pub names: Names,
    /// The key of the kind of drawing of the drawing standard it is for ("floor-plan").
    pub kind: String,
    /// The scale it is made for, as 1 to this; none: every scale of its kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<u32>,
    pub labels: Vec<LabelType>,
    pub rules: Vec<Rule>,
}

/// An element as the rules see it: what a view knows of it without its geometry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Described {
    pub class: String,
    pub name: String,
    pub type_name: String,
    pub code: String,
    /// Its properties, by name; `Material`, `Thickness`, `Profile`, `Length`, `Area` and `Level`
    /// are those of the element's properties.
    pub props: Vec<(String, String)>,
}

impl Described {
    fn prop(&self, key: &str) -> Option<&str> {
        self.props.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.trim()).filter(|v| !v.is_empty())
    }
}

/// What a drawing set gives one element: its label as text with the type it is of, and the
/// dimensions asked of it, each with the key of its rule.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Annotation {
    pub label: Option<(String, String)>,
    pub dimensions: Vec<(String, Measure, bool)>,
}

impl Filter {
    /// Whether it takes `element`.
    pub fn takes(&self, element: &Described) -> bool {
        let class = self.classes.is_empty() || self.classes.iter().any(|c| c.eq_ignore_ascii_case(&element.class));
        let code = self.codes.is_empty() || self.codes.iter().any(|c| element.code == *c || element.code.strip_prefix(c.as_str()).is_some_and(|rest| rest.starts_with('.') || c.contains('.')));
        let of_type = self.type_contains.as_ref().is_none_or(|part| element.type_name.to_lowercase().contains(&part.to_lowercase()));
        let property = self.property.as_ref().is_none_or(|(key, value)| element.prop(key).is_some_and(|has| value.is_empty() || has.eq_ignore_ascii_case(value)));
        class && code && of_type && property
    }
}

impl LabelType {
    /// The text of a label of this type on `element`: its fields that have something to say,
    /// with the separator between them. A fixed text stands against the field after it.
    pub fn text(&self, element: &Described) -> String {
        let mut out = String::new();
        let mut glue = false;
        for field in &self.fields {
            let value = match field {
                LabelField::Name => Some(element.name.as_str()).filter(|v| !v.is_empty()).map(str::to_string),
                LabelField::Type => Some(element.type_name.as_str()).filter(|v| !v.is_empty()).map(str::to_string),
                LabelField::Code => Some(element.code.as_str()).filter(|v| !v.is_empty()).map(str::to_string),
                LabelField::Material => element.prop("Material").map(str::to_string),
                LabelField::Thickness => element.prop("Thickness").map(millimetres),
                LabelField::Profile => element.prop("Profile").map(str::to_string),
                LabelField::Length => element.prop("Length").map(millimetres),
                LabelField::Area => element.prop("Area").map(str::to_string),
                LabelField::Level => element.prop("Level").map(str::to_string),
                LabelField::Property(key) => element.prop(key).map(str::to_string),
                LabelField::Text(text) => Some(text.clone()),
            };
            let Some(value) = value else { continue };
            if !out.is_empty() && !glue {
                out.push_str(&self.separator);
            }
            out.push_str(&value);
            glue = matches!(field, LabelField::Text(_));
        }
        // (a fixed text with nothing after it says nothing)
        if glue && self.fields.iter().all(|f| matches!(f, LabelField::Text(_))) {
            return out;
        }
        if glue {
            let fixed = self.fields.iter().rev().find_map(|f| if let LabelField::Text(t) = f { Some(t.len()) } else { None }).unwrap_or(0);
            out.truncate(out.len() - fixed);
            return out.trim_end_matches(self.separator.as_str()).to_string();
        }
        out
    }
}

/// A length as a property has it ("0.360 m") in millimetres ("360"); what is no such length
/// as it is.
fn millimetres(value: &str) -> String {
    match value.trim().strip_suffix('m').map(str::trim).and_then(|metres| metres.replace(',', ".").parse::<f64>().ok()) {
        Some(metres) => crate::millimetres(metres, Lang::En),
        None => value.to_string(),
    }
}

impl DrawingSet {
    pub fn name(&self, lang: Lang) -> &str {
        self.names.get(lang)
    }

    pub fn label(&self, key: &str) -> Option<&LabelType> {
        self.labels.iter().find(|l| l.key == key)
    }

    /// What `element` gets: the label of the first label rule that takes it (when that label
    /// has something to say), and the dimension of every dimension rule that takes it.
    pub fn annotate(&self, element: &Described) -> Annotation {
        let mut out = Annotation::default();
        let mut labelled = false;
        for rule in self.rules.iter().filter(|rule| rule.filter.takes(element)) {
            match &rule.gives {
                Gives::Label { label } if !labelled => {
                    labelled = true;
                    out.label = self.label(label).map(|of| (label.clone(), of.text(element))).filter(|(_, text)| !text.is_empty());
                }
                Gives::Label { .. } => {}
                Gives::Dimension { measure, chain } => out.dimensions.push((rule.key.clone(), *measure, *chain)),
            }
        }
        out
    }

    /// Whether it holds together: keys each once, labels with fields, and rules that name a
    /// label it has.
    pub fn check(&self) -> Result<(), LibraryError> {
        if self.key.trim().is_empty() || self.kind.trim().is_empty() {
            return Err(LibraryError::NoKey);
        }
        for (k, label) in self.labels.iter().enumerate() {
            if label.key.trim().is_empty() || self.labels[..k].iter().any(|l| l.key == label.key) {
                return Err(LibraryError::Duplicate(label.key.clone()));
            }
            if label.fields.is_empty() || label.text_style.trim().is_empty() {
                return Err(LibraryError::UnknownSetting(format!("label {}: fields and a text style", label.key)));
            }
        }
        for (k, rule) in self.rules.iter().enumerate() {
            if rule.key.trim().is_empty() || self.rules[..k].iter().any(|r| r.key == rule.key) {
                return Err(LibraryError::Duplicate(rule.key.clone()));
            }
            if let Gives::Label { label } = &rule.gives {
                if self.label(label).is_none() {
                    return Err(LibraryError::UnknownSetting(label.clone()));
                }
            }
        }
        Ok(())
    }
}

/// The drawing sets a library starts with, after the Dutch drawing standard: an architectural
/// floor plan at 1:100 and at 1:50, a structural plan and a section. They are the suite's own
/// start, to be made an office's.
pub fn starter() -> Vec<DrawingSet> {
    let label = |key: &str, nl: &str, en: &str, de: &str, fr: &str, fields: Vec<LabelField>, text_style: &str, frame, leader, place| LabelType { key: key.into(), names: Names::new(nl, en, de, fr), fields, separator: " ".into(), text_style: text_style.into(), frame, leader, place };
    let filter = |classes: &[&str], codes: &[&str]| Filter { classes: classes.iter().map(|c| c.to_string()).collect(), codes: codes.iter().map(|c| c.to_string()).collect(), ..Filter::default() };
    let rule = |key: &str, nl: &str, en: &str, de: &str, fr: &str, filter: Filter, gives| Rule { key: key.into(), names: Names::new(nl, en, de, fr), filter, gives };
    let labelled = |label: &str| Gives::Label { label: label.into() };
    let measured = |measure, chain| Gives::Dimension { measure, chain };
    use LabelField::*;
    let wall_type = || label("wall-type", "wandtype", "wall type", "Wandtyp", "type de mur", vec![Type], "2.5", Frame::None, true, Place::Along);
    let wall_code = || label("wall-code", "wandcode", "wall code", "Wandcode", "code de mur", vec![Code, Thickness], "2.5", Frame::Hexagon, true, Place::Centre);
    let room = || label("room", "ruimte", "room", "Raum", "pièce", vec![Name, Area], "3.5", Frame::None, false, Place::Centre);
    let opening = || label("opening", "kozijnmerk", "opening mark", "Öffnungsnummer", "repère de baie", vec![Name], "2.5", Frame::Circle, false, Place::Centre);
    let member = || label("member", "profiel", "profile", "Profil", "profilé", vec![Name, Profile], "2.5", Frame::None, false, Place::Along);
    let column = || label("column", "kolom", "column", "Stütze", "poteau", vec![Name, Profile], "2.5", Frame::None, true, Place::Right);
    let slab = || label("slab", "vloer", "floor", "Decke", "plancher", vec![Type, Text("d=".into()), Thickness], "2.5", Frame::Rectangle, false, Place::Centre);
    let pile = || label("pile", "paal", "pile", "Pfahl", "pieu", vec![Name], "2.5", Frame::Circle, false, Place::Right);
    let rooms = || rule("rooms", "ruimten", "rooms", "Räume", "pièces", filter(&["IfcSpace"], &[]), labelled("room"));
    let openings = || rule("openings", "kozijnen", "doors and windows", "Türen und Fenster", "portes et fenêtres", filter(&["IfcDoor", "IfcWindow"], &[]), labelled("opening"));
    let grids = || rule("grids", "maten op stramien", "dimensions to the grids", "Maße zum Raster", "cotes sur trames", filter(&["IfcColumn"], &[]), measured(Measure::Grids, true));
    let plan = |key: &str, nl: &str, en: &str, de: &str, fr: &str, scale: u32, labels: Vec<LabelType>, rules: Vec<Rule>| DrawingSet { key: key.into(), names: Names::new(nl, en, de, fr), kind: "floor-plan".into(), scale: Some(scale), labels, rules };
    vec![
        plan(
            "architectural-plan-100",
            "bouwkundige plattegrond 1:100",
            "architectural floor plan 1:100",
            "Grundriss Architektur 1:100",
            "plan d'architecte 1:100",
            100,
            vec![room(), opening(), wall_code()],
            vec![
                rooms(),
                openings(),
                rule("wall-codes", "wandcodes", "wall codes", "Wandcodes", "codes des murs", filter(&["IfcWall"], &[]), labelled("wall-code")),
                rule("external-walls", "buitenwanden", "external walls", "Außenwände", "murs extérieurs", filter(&["IfcWall"], &["21"]), measured(Measure::Openings, true)),
                rule("wall-lengths", "wandlengten", "wall lengths", "Wandlängen", "longueurs des murs", filter(&["IfcWall"], &[]), measured(Measure::Length, false)),
            ],
        ),
        plan(
            "architectural-plan-50",
            "bouwkundige plattegrond 1:50",
            "architectural floor plan 1:50",
            "Grundriss Architektur 1:50",
            "plan d'architecte 1:50",
            50,
            vec![room(), opening(), wall_type()],
            vec![
                rooms(),
                openings(),
                rule("wall-types", "wandtypen", "wall types", "Wandtypen", "types de murs", filter(&["IfcWall"], &[]), labelled("wall-type")),
                rule("wall-thickness", "wanddikten", "wall thicknesses", "Wanddicken", "épaisseurs des murs", filter(&["IfcWall"], &[]), measured(Measure::Thickness, true)),
                rule("external-walls", "buitenwanden", "external walls", "Außenwände", "murs extérieurs", filter(&["IfcWall"], &["21"]), measured(Measure::Openings, true)),
                rule("wall-lengths", "wandlengten", "wall lengths", "Wandlängen", "longueurs des murs", filter(&["IfcWall"], &[]), measured(Measure::Length, false)),
            ],
        ),
        DrawingSet {
            key: "structural-plan-100".into(),
            names: Names::new("constructieve plattegrond 1:100", "structural plan 1:100", "Tragwerksplan 1:100", "plan de structure 1:100"),
            kind: "structural-plan".into(),
            scale: Some(100),
            labels: vec![member(), column(), slab(), pile()],
            rules: vec![
                rule("beams", "liggers", "beams", "Träger", "poutres", filter(&["IfcBeam", "IfcMember"], &[]), labelled("member")),
                rule("columns", "kolommen", "columns", "Stützen", "poteaux", filter(&["IfcColumn"], &[]), labelled("column")),
                rule("piles", "palen", "piles", "Pfähle", "pieux", filter(&["IfcPile"], &[]), labelled("pile")),
                rule("slabs", "vloeren", "floors", "Decken", "planchers", filter(&["IfcSlab"], &["23"]), labelled("slab")),
                grids(),
                rule("bearing-walls", "dragende wanden", "loadbearing walls", "tragende Wände", "murs porteurs", Filter { property: Some(("LoadBearing".into(), "true".into())), ..filter(&["IfcWall"], &[]) }, measured(Measure::Thickness, false)),
            ],
        },
        DrawingSet {
            key: "architectural-section-50".into(),
            names: Names::new("bouwkundige doorsnede 1:50", "architectural section 1:50", "Schnitt Architektur 1:50", "coupe d'architecte 1:50"),
            kind: "section".into(),
            scale: Some(50),
            labels: vec![room(), slab()],
            rules: vec![
                rooms(),
                rule("slabs", "vloeren en daken", "floors and roofs", "Decken und Dächer", "planchers et toits", filter(&["IfcSlab", "IfcRoof"], &[]), labelled("slab")),
                rule("slab-thickness", "vloerdikten", "floor thicknesses", "Deckendicken", "épaisseurs des planchers", filter(&["IfcSlab", "IfcRoof"], &[]), measured(Measure::Thickness, true)),
                rule("storey-heights", "verdiepingshoogten", "storey heights", "Geschosshöhen", "hauteurs d'étage", filter(&["IfcWall"], &["21"]), measured(Measure::Length, true)),
            ],
        },
    ]
}

/// The path of the node of drawing set `key`.
pub fn path(key: &str) -> String {
    uuid(&format!("openaec-library/drawingset/{key}"))
}

/// The node of a drawing set.
pub fn node(set: &DrawingSet) -> Value {
    json!({"path": path(&set.key), "attributes": {"bsi::ifc::prop::Name": set.name(Lang::En), DRAWING_SET: set}})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall(code: &str, type_name: &str) -> Described {
        Described { class: "IfcWall".into(), name: "W1".into(), type_name: type_name.into(), code: code.into(), props: vec![("Thickness".into(), "0.360 m".into()), ("Length".into(), "4.000 m".into())] }
    }

    fn set(key: &str) -> DrawingSet {
        starter().into_iter().find(|s| s.key == key).unwrap_or_else(|| panic!("{key}"))
    }

    #[test]
    fn the_starter_sets_hold_together_and_are_for_kinds_of_the_dutch_standard() {
        let standard = crate::drawing::Standard::netherlands();
        let sets = starter();
        assert_eq!(sets.len(), 4);
        for s in &sets {
            s.check().unwrap_or_else(|e| panic!("{}: {e}", s.key));
            let kind = standard.kinds.iter().find(|k| k.key == s.kind).unwrap_or_else(|| panic!("{}: no kind {}", s.key, s.kind));
            assert!(s.scale.is_none_or(|scale| kind.scales.contains(&scale) || standard.scales.contains(&scale)), "{}", s.key);
            for label in &s.labels {
                assert!(standard.text_style(&label.text_style).is_some(), "{}: no text style {}", label.key, label.text_style);
                // every label of a set is used by a rule of it
                assert!(s.rules.iter().any(|r| r.gives == Gives::Label { label: label.key.clone() }), "{}: {} is not used", s.key, label.key);
            }
        }
    }

    #[test]
    fn a_filter_takes_by_class_code_type_and_property() {
        let outer = wall("21.22", "Spouwmuur 360");
        let by = |f: Filter| f.takes(&outer);
        assert!(by(Filter::default()), "an empty filter takes every element");
        assert!(by(Filter { classes: vec!["ifcwall".into()], ..Filter::default() }) && !by(Filter { classes: vec!["IfcSlab".into()], ..Filter::default() }));
        // a code takes the codes under it, not those that only start alike
        for (code, takes) in [("21", true), ("21.2", true), ("21.22", true), ("2", false), ("21.3", false), ("22", false)] {
            assert_eq!(by(Filter { codes: vec![code.into()], ..Filter::default() }), takes, "{code}");
        }
        assert!(by(Filter { type_contains: Some("spouw".into()), ..Filter::default() }) && !by(Filter { type_contains: Some("metal stud".into()), ..Filter::default() }));
        // a property by its value, or by being there
        let length = |value: &str| Filter { property: Some(("length".into(), value.into())), ..Filter::default() };
        assert!(by(length("4.000 M")) && by(length("")) && !by(length("5.000 m")));
        assert!(!by(Filter { property: Some(("LoadBearing".into(), String::new())), ..Filter::default() }));
        // every part given must hold
        assert!(!by(Filter { classes: vec!["IfcWall".into()], codes: vec!["22".into()], ..Filter::default() }));
    }

    #[test]
    fn a_label_reads_its_element() {
        let plan = set("architectural-plan-50");
        let outer = wall("21", "Spouwmuur 360");
        assert_eq!(plan.label("wall-type").unwrap().text(&outer), "Spouwmuur 360");
        // a length of the properties is written in millimetres
        let coded = set("architectural-plan-100").label("wall-code").unwrap().clone();
        assert_eq!(coded.text(&outer), "21 360");
        // a field with nothing to say is left out, with its separator
        assert_eq!(coded.text(&Described { code: String::new(), ..outer.clone() }), "360");
        // a fixed text stands against the field after it, and is left out with it
        let slab = set("structural-plan-100").label("slab").unwrap().clone();
        let floor = Described { class: "IfcSlab".into(), type_name: "Kanaalplaat 200".into(), code: "23".into(), props: vec![("Thickness".into(), "0.200 m".into())], ..Described::default() };
        assert_eq!(slab.text(&floor), "Kanaalplaat 200 d=200");
        assert_eq!(slab.text(&Described { props: Vec::new(), ..floor }), "Kanaalplaat 200");
    }

    #[test]
    fn an_element_gets_its_first_label_and_every_dimension() {
        let plan = set("architectural-plan-50");
        let outer = plan.annotate(&wall("21", "Spouwmuur 360"));
        assert_eq!(outer.label, Some(("wall-type".into(), "Spouwmuur 360".into())));
        assert_eq!(outer.dimensions.iter().map(|d| (d.0.as_str(), d.1, d.2)).collect::<Vec<_>>(), [("wall-thickness", Measure::Thickness, true), ("external-walls", Measure::Openings, true), ("wall-lengths", Measure::Length, false)]);
        // an internal wall is not measured as an external one
        let inner = plan.annotate(&wall("22", "Kalkzandsteen 100"));
        assert!(inner.dimensions.iter().all(|d| d.0 != "external-walls") && inner.dimensions.len() == 2);
        // a room is labelled by its name and area, a door by its mark; a stair gets nothing
        let room = Described { class: "IfcSpace".into(), name: "Woonkamer".into(), props: vec![("Area".into(), "32.5 m²".into())], ..Described::default() };
        assert_eq!(plan.annotate(&room).label, Some(("room".into(), "Woonkamer 32.5 m²".into())));
        assert_eq!(plan.annotate(&Described { class: "IfcStair".into(), ..Described::default() }), Annotation::default());
        // a structural plan: a beam by its mark and profile, a loadbearing wall measured
        let structural = set("structural-plan-100");
        let beam = Described { class: "IfcBeam".into(), name: "L1".into(), props: vec![("Profile".into(), "HEA 200".into())], ..Described::default() };
        assert_eq!(structural.annotate(&beam).label, Some(("member".into(), "L1 HEA 200".into())));
        let bearing = Described { props: vec![("LoadBearing".into(), "True".into())], ..wall("22", "Kalkzandsteen 214") };
        assert_eq!((structural.annotate(&bearing).dimensions.len(), structural.annotate(&wall("22", "x")).dimensions.len()), (1, 0));
    }

    #[test]
    fn what_does_not_hold_together_is_refused_and_a_set_reads_back() {
        let ok = set("architectural-plan-100");
        let refused = |change: &dyn Fn(&mut DrawingSet)| {
            let mut s = ok.clone();
            change(&mut s);
            s.check().is_err()
        };
        assert!(refused(&|s| s.key.clear()));
        assert!(refused(&|s| s.labels.push(s.labels[0].clone())), "a label twice");
        assert!(refused(&|s| s.rules.push(s.rules[0].clone())), "a rule twice");
        assert!(refused(&|s| s.labels[0].fields.clear()));
        assert!(refused(&|s| s.rules[0].gives = Gives::Label { label: "none".into() }), "a rule names a label the set has");
        let written = node(&ok);
        assert_eq!((written["path"].as_str(), written["attributes"]["bsi::ifc::prop::Name"].as_str()), (Some(path("architectural-plan-100").as_str()), Some("architectural floor plan 1:100")));
        // a rule is written flat: its filter, and what it gives
        assert_eq!(written["attributes"][DRAWING_SET]["rules"][3], json!({"key": "external-walls", "names": {"nl": "buitenwanden", "en": "external walls", "de": "Außenwände", "fr": "murs extérieurs"}, "filter": {"classes": ["IfcWall"], "codes": ["21"]}, "gives": "dimension", "measure": "openings", "chain": true}));
        let back: DrawingSet = serde_json::from_value(written["attributes"][DRAWING_SET].clone()).unwrap();
        assert_eq!(back, ok);
    }
}
