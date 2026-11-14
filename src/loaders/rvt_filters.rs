//! The filters of a Revit view: which elements a view hides by rule.
//!
//! A view lists filters (`FilterOverrides::m_oaSettings`), each switched on or off and shown or
//! hidden in it. A filter (`ParameterFilterElement`) names categories and, as a tree, what the
//! elements of those categories must say: rules on a parameter (a text that holds or equals
//! something, a number that equals or is larger), turned round, and combined with and / or. The
//! value a rule asks for stands with the element itself, by the parameter's id
//! (`m_pParamValueSet…`); the type's and the family's name come from what the model says of the
//! element.
//!
//! A rule whose value is not found is neither true nor false. A filter hides an element only
//! when it is certain to hold: what is not understood stays shown.

use rvt_model::{FieldValue, NestedObject, ObjectView};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// The type name and the family name of an element.
pub(super) type Names = HashMap<u64, (String, String)>;

/// Built-in parameters whose value is a name the model knows.
const TYPE_NAME: i64 = -1002001;
const FAMILY_NAME: i64 = -1002002;
/// A wall's "Structural": in the wall's own record.
const WALL_STRUCTURAL: i64 = -1001119;

/// An object a field points at or holds.
fn object<'a>(o: &'a ObjectView, v: &'a FieldValue) -> Option<&'a NestedObject> {
    v.as_object().or_else(|| o.deref(v))
}

/// The objects of a list field.
fn objects<'a>(o: &'a ObjectView, of: &'a NestedObject, field: &str) -> Vec<&'a NestedObject> {
    of.get(field).and_then(|v| v.as_list()).unwrap_or(&[]).iter().filter_map(|v| object(o, v)).collect()
}

/// Both hold: false when one is certain not to, unknown when one is unknown.
fn all(parts: impl Iterator<Item = Option<bool>>) -> Option<bool> {
    let mut unknown = false;
    for part in parts {
        match part {
            Some(false) => return Some(false),
            None => unknown = true,
            Some(true) => {}
        }
    }
    (!unknown).then_some(true)
}

/// One holds: true when one is certain to, unknown when none is and one is unknown.
fn any(parts: impl Iterator<Item = Option<bool>>) -> Option<bool> {
    all(parts.map(|p| p.map(|b| !b))).map(|none| !none)
}

/// What an element says for a parameter.
#[derive(Debug, Clone, PartialEq)]
enum Said {
    Text(String),
    Number(f64),
}

/// What an element's record says of its parameters, by the parameter's id: read once, as many
/// filters ask the same element.
type Values = HashMap<i64, Said>;

/// The values of the parameters an element's record holds (the first where one is held twice).
fn values(element: &ObjectView) -> Values {
    let mut out = Values::new();
    for set in ["m_pParamValueSetInt", "m_pParamValueSetDouble", "m_pParamValueSetAString", "m_pParamValueSetElementId"] {
        let Some(values) = element.get(set).and_then(|v| object(element, v)) else { continue };
        for value in objects(element, values, "m_paramSet") {
            let Some(parameter) = value.get("m_paramId").and_then(|v| v.element()) else { continue };
            let said = match value.get("m_value") {
                Some(FieldValue::Str(s)) => Some(Said::Text(s.clone())),
                Some(FieldValue::Bool(b)) => Some(Said::Number(f64::from(u8::from(*b)))),
                Some(FieldValue::Element { element }) => Some(Said::Number(*element as f64)),
                Some(other) => other.as_f64().or(other.as_i64().map(|i| i as f64)).map(Said::Number),
                None => None,
            };
            if let Some(said) = said {
                out.entry(parameter).or_insert(said);
            }
        }
    }
    if let Some(FieldValue::Bool(b)) = element.get("m_isStructuralSignificant") {
        out.entry(WALL_STRUCTURAL).or_insert(Said::Number(f64::from(u8::from(*b))));
    }
    out
}

/// The value of a parameter of an element: a name the model knows, else from its record.
fn said(element: &Values, names: Option<&(String, String)>, parameter: i64) -> Option<Said> {
    match parameter {
        TYPE_NAME => names.map(|n| Said::Text(n.0.clone())),
        FAMILY_NAME => names.map(|n| Said::Text(n.1.clone())),
        _ => element.get(&parameter).cloned(),
    }
}

/// A text against a rule's text, by the rule's kind (the end of its class name). Filters take
/// no notice of capitals.
fn text_holds(kind: &str, value: &str, rule: &str) -> Option<bool> {
    let (value, rule) = (value.to_lowercase(), rule.to_lowercase());
    Some(match kind {
        "Equals" => value == rule,
        "Contains" => value.contains(&rule),
        "BeginsWith" => value.starts_with(&rule),
        "EndsWith" => value.ends_with(&rule),
        "Greater" => value > rule,
        "GreaterOrEqual" => value >= rule,
        "Less" => value < rule,
        "LessOrEqual" => value <= rule,
        _ => return None,
    })
}

fn number_holds(kind: &str, value: f64, rule: f64, slack: f64) -> Option<bool> {
    Some(match kind {
        "Equals" => (value - rule).abs() <= slack,
        "Greater" => value > rule + slack,
        "GreaterOrEqual" => value >= rule - slack,
        "Less" => value < rule - slack,
        "LessOrEqual" => value <= rule + slack,
        _ => return None,
    })
}

/// Whether a rule holds for an element (`None`: not known).
fn rule_holds(filter: &ObjectView, rule: &NestedObject, element: &Values, names: Option<&(String, String)>) -> Option<bool> {
    if rule.class == "FilterInverseRule" {
        return rule_holds(filter, object(filter, rule.get("m_oInnerRule")?)?, element, names).map(|b| !b);
    }
    let parameter = object(filter, rule.get("m_oValueProvider")?)?.get("m_parameter")?.element()?;
    let evaluator = &object(filter, rule.get("m_oEvaluator")?)?.class;
    let value = said(element, names, parameter)?;
    match (rule.class.as_str(), value) {
        ("FilterStringRule", Said::Text(value)) => text_holds(evaluator.strip_prefix("FilterString")?, &value, rule.get("m_ruleString")?.as_str()?),
        ("FilterIntegerRule" | "FilterElementIdRule", Said::Number(value)) => {
            let wanted = rule.get("m_ruleValue")?;
            number_holds(evaluator.strip_prefix("FilterNumeric")?, value, wanted.as_i64().or(wanted.element())? as f64, 0.0)
        }
        ("FilterDoubleRule", Said::Number(value)) => {
            let slack = rule.get("m_epsilon").and_then(|v| v.as_f64()).unwrap_or(1e-9);
            number_holds(evaluator.strip_prefix("FilterNumeric")?, value, rule.get("m_ruleValue")?.as_f64()?, slack)
        }
        _ => None,
    }
}

/// Whether a node of a filter's tree holds for an element: rules that must all hold, or nodes
/// combined with and / or; either can be turned round.
fn node_holds(filter: &ObjectView, node: &NestedObject, element: &Values, names: Option<&(String, String)>) -> Option<bool> {
    let holds = match node.class.as_str() {
        "ElementParameterFilterData" => all(objects(filter, node, "m_rules").into_iter().map(|rule| rule_holds(filter, rule, element, names))),
        "LogicalAndFilterData" => all(objects(filter, node, "m_filters").into_iter().map(|inner| node_holds(filter, inner, element, names))),
        "LogicalOrFilterData" => any(objects(filter, node, "m_filters").into_iter().map(|inner| node_holds(filter, inner, element, names))),
        _ => None,
    };
    let inverted = matches!(node.get("m_inverted"), Some(FieldValue::Bool(true)));
    holds.map(|b| b != inverted)
}

/// A colour a view's override gives; this value stands for none.
const NO_COLOR: i64 = 0x0100_0000;

/// The fill patterns read: their grids and whether they are sized on paper.
type FillPatterns = HashMap<i64, Option<(Vec<crate::hatching::Grid>, bool)>>;

/// The filters of the views of a project as they are worked out: views and their templates share
/// filters, and filters ask the same elements, so what a filter holds for and what an element
/// says are each found once.
pub(super) struct Filters<'a> {
    doc: &'a rvt_read::Document,
    /// The elements the model draws, by category.
    by_category: &'a HashMap<i64, Vec<u64>>,
    names: &'a Names,
    /// What each filter (by its id) holds for.
    held: HashMap<i64, Arc<HashSet<u64>>>,
    /// What each element asked says of its parameters.
    values: HashMap<u64, Values>,
    fills: FillPatterns,
}

impl<'a> Filters<'a> {
    pub(super) fn new(doc: &'a rvt_read::Document, by_category: &'a HashMap<i64, Vec<u64>>, names: &'a Names) -> Self {
        Filters { doc, by_category, names, held: HashMap::new(), values: HashMap::new(), fills: HashMap::new() }
    }

    /// The elements of a filter's categories that its rules certainly hold for.
    fn held(&mut self, id: i64) -> Arc<HashSet<u64>> {
        if let Some(held) = self.held.get(&id) {
            return held.clone();
        }
        let mut elements = HashSet::new();
        if let Some(filter) = u64::try_from(id).ok().and_then(|id| self.doc.object(id)) {
            let tree = filter.get("m_oRuleCombination").and_then(|v| object(&filter, v));
            let categories = filter.get("m_oCategoryRule").and_then(|v| object(&filter, v));
            for category in categories.and_then(|c| c.get("m_categories")).and_then(|v| v.as_list()).unwrap_or(&[]).iter().filter_map(|c| c.element()) {
                for &element in self.by_category.get(&category).into_iter().flatten() {
                    let holds = match tree {
                        // a filter without rules takes its categories whole
                        None => true,
                        Some(tree) => {
                            let doc = self.doc;
                            let said = self.values.entry(element).or_insert_with(|| doc.object(element).map(|o| values(&o)).unwrap_or_default());
                            node_holds(&filter, tree, said, self.names.get(&element)) == Some(true)
                        }
                    };
                    if holds {
                        elements.insert(element);
                    }
                }
            }
        }
        let held = Arc::new(elements);
        self.held.insert(id, held.clone());
        held
    }

    /// The filter settings of a view (its record, or its template's) that are on, in the order
    /// of its list: the first decides where they differ.
    fn settings<'v>(view: &'v ObjectView) -> Vec<&'v NestedObject> {
        let Some(overrides) = view.get("m_oFilterOverrides").and_then(|v| object(view, v)) else { return Vec::new() };
        objects(view, overrides, "m_oaSettings").into_iter().filter(|s| matches!(s.get("m_bEnabledFlag"), Some(FieldValue::Bool(true)))).collect()
    }

    /// The elements a view hides by its filters: of each filter that is on and hidden in the
    /// view, the elements it holds for.
    pub(super) fn hidden(&mut self, view: &ObjectView) -> HashSet<u64> {
        let mut out = HashSet::new();
        for setting in Self::settings(view).into_iter().filter(|s| !shown(s)) {
            if let Some(id) = setting.get("m_filterElemId").and_then(|v| v.element()) {
                out.extend(self.held(id).iter().copied());
            }
        }
        out
    }

    /// How a view draws the elements its filters hold for otherwise (see
    /// [`crate::hatching::Look`]): of each filter that is on and shown in it, what its graphic
    /// override changes; where filters differ, the one higher in the view's list decides. The
    /// rules of a filter that changes nothing are not looked at.
    pub(super) fn looks(&mut self, view: &ObjectView) -> HashMap<u64, crate::hatching::Look> {
        let mut out: HashMap<u64, crate::hatching::Look> = HashMap::new();
        for setting in Self::settings(view).into_iter().filter(|s| shown(s)) {
            let look = setting.get("m_pOverrideGraphicSettings").and_then(|v| object(view, v)).map(|o| look_of(self.doc, view, o, &mut self.fills)).unwrap_or_default();
            let Some(id) = setting.get("m_filterElemId").and_then(|v| v.element()).filter(|_| !look.is_empty()) else { continue };
            for &element in self.held(id).iter() {
                let entry = out.entry(element).or_default();
                *entry = std::mem::take(entry).over(&look);
            }
        }
        out
    }
}

impl Filters<'_> {
    /// How a 3D view draws its elements otherwise than their materials: what its categories are
    /// set to (`GraphicOverrides`: the colour of their lines, halftone, transparency, the colour
    /// of a solid surface pattern), with its filters over that. `from` is the view's record or
    /// its template's.
    pub(super) fn looks_3d(&mut self, from: &ObjectView) -> HashMap<u64, crate::hatching::Look> {
        let mut by_category: HashMap<i64, crate::hatching::Look> = HashMap::new();
        if let Some(over) = from.get("m_oGraphicOverrides").and_then(|v| object(from, v)) {
            // the pairs of a list: a category and what it is set to
            let pairs = |field: &str| objects(from, over, field).into_iter().filter_map(|p| Some((p.get("first")?.element()?, p.get("second")?)));
            let ids = |field: &str| over.get(field).and_then(|v| v.as_list()).unwrap_or(&[]).iter().filter_map(|v| v.element()).collect::<HashSet<i64>>();
            let given = |c: i64| (c >= 0 && c != NO_COLOR).then(|| super::rvt_materials::rgb(c));
            for (category, style) in pairs("m_gStylesProj") {
                // black is what a line is anyway: an override that leaves the colour gives 0
                let color = object(from, style).and_then(|s| s.get("m_color")?.as_i64()).filter(|c| *c != 0).and_then(given);
                by_category.entry(category).or_default().seen_line.color = color;
            }
            for category in ids("m_halftones") {
                by_category.entry(category).or_default().halftone = true;
            }
            for (category, t) in pairs("m_surfacesTransparency") {
                by_category.entry(category).or_default().transparency = t.as_i64().map_or(0, |t| t.clamp(0, 100) as u8);
            }
            // a surface colour shows where a solid pattern covers the surface
            let hidden = ids("m_fillHiddenProj");
            let patterns: HashMap<i64, i64> = pairs("m_fillPatternsProj").filter_map(|(c, p)| Some((c, p.element().or_else(|| object(from, p)?.get("m_fillPatternId")?.element())?))).collect();
            for (category, c) in pairs("m_fillColorsProj") {
                let solid = patterns.get(&category).is_some_and(|&id| self.solid(id));
                if let (true, false, Some(color)) = (solid, hidden.contains(&category), c.as_i64().and_then(given)) {
                    by_category.entry(category).or_default().surface = Some(color);
                }
            }
        }
        let mut out: HashMap<u64, crate::hatching::Look> = HashMap::new();
        for (category, look) in by_category.into_iter().filter(|(_, l)| !l.is_empty()) {
            for &element in self.by_category.get(&category).into_iter().flatten() {
                out.insert(element, look.clone());
            }
        }
        for (element, look) in self.looks(from) {
            let under = out.remove(&element).unwrap_or_default();
            out.insert(element, look.over(&under));
        }
        out
    }

    /// Whether a fill pattern fills solid.
    fn solid(&mut self, id: i64) -> bool {
        let doc = self.doc;
        self.fills.entry(id).or_insert_with(|| super::rvt_materials::fill_pattern(doc, id)).as_ref().is_some_and(|(grids, _)| grids.is_empty())
    }

    /// What the discipline of a view (`from`: its record or its template's) does to its
    /// elements: a structural view leaves out the walls that carry nothing; a view of a building
    /// service (mechanical, electrical, plumbing) draws what is not of a service in halftone.
    /// The hidden, and the halftone.
    pub(super) fn discipline(&mut self, from: &ObjectView) -> (HashSet<u64>, HashSet<u64>) {
        let discipline = values(from).get(&DISCIPLINE).and_then(|s| match s {
            Said::Number(n) => Some(*n as i64),
            Said::Text(_) => None,
        });
        let (mut hidden, mut faint) = (HashSet::new(), HashSet::new());
        match discipline {
            Some(STRUCTURAL) => {
                for &wall in self.by_category.get(&WALLS).into_iter().flatten() {
                    let doc = self.doc;
                    let said = self.values.entry(wall).or_insert_with(|| doc.object(wall).map(|o| values(&o)).unwrap_or_default());
                    if said.get(&WALL_STRUCTURAL) == Some(&Said::Number(0.0)) {
                        hidden.insert(wall);
                    }
                }
            }
            Some(MECHANICAL | ELECTRICAL | PLUMBING) => {
                for elements in self.by_category.iter().filter(|(c, _)| !SERVICES.contains(c)).map(|(_, e)| e) {
                    faint.extend(elements.iter().copied());
                }
            }
            _ => {}
        }
        (hidden, faint)
    }
}

/// A view's discipline (`VIEW_DISCIPLINE`), and what it can be.
const DISCIPLINE: i64 = -1005163;
const STRUCTURAL: i64 = 2;
const MECHANICAL: i64 = 4;
const ELECTRICAL: i64 = 8;
const PLUMBING: i64 = 16;
/// The category of walls.
const WALLS: i64 = -2000011;
/// The categories of the building services: ducts, pipes, cable trays and conduits with their
/// fittings and accessories, and the equipment, fixtures and devices on them.
const SERVICES: [i64; 29] = [
    -2008000, -2008010, -2008016, -2008013, -2008020, -2008123, -2008122, -2008044, -2008049, -2008055, -2008050, -2001160, -2001140, -2001040, -2001060,
    -2001120, -2008087, -2008130, -2008126, -2008132, -2008128, -2008081, -2008083, -2008085, -2008079, -2008077, -2008075, -2008099, -2008039,
];

/// Whether a filter's setting shows what it holds for.
fn shown(setting: &NestedObject) -> bool {
    matches!(setting.get("m_bVisibilityFlag"), Some(FieldValue::Bool(true)))
}

/// What the graphic override of a filter (`OverrideGraphicSettings`) changes of the cut of the
/// elements it holds for: halftone, the pattern of the cut and its colour, the colour behind it,
/// and whether either is drawn; the colour and pen of their cut lines and of their lines seen
/// beyond the cut (`m_oCutPenColor`, `m_oCutPenNumber`, `m_oProj…`). `fills` keeps the fill
/// patterns read.
fn look_of(doc: &rvt_read::Document, view: &ObjectView, over: &NestedObject, fills: &mut FillPatterns) -> crate::hatching::Look {
    let flag = |name: &str| match over.get(name) {
        Some(FieldValue::Bool(b)) => Some(*b),
        _ => None,
    };
    // a part of the override: a pattern's id or a colour, where it gives one
    let inner = |part: &str, field: &str| over.get(part).and_then(|v| object(view, v)).and_then(|o| o.get(field).cloned());
    let color = |part: &str| inner(part, "m_color").and_then(|v| v.as_i64()).filter(|c| *c != NO_COLOR && *c >= 0).map(super::rvt_materials::rgb);
    let pattern = inner("m_oCutFillPattern", "m_fillPatternId").and_then(|v| v.element()).filter(|id| *id > 0);
    // a pen: none is -1
    let pen = |part: &str| inner(part, "m_penNumber").and_then(|v| v.as_i64()).filter(|p| *p > 0);
    let ink = |part: &str| crate::drawing::Ink { color: color(part), width: None };
    // the colour of its surfaces where a solid pattern covers them
    let solid = |id: i64| fills.entry(id).or_insert_with(|| super::rvt_materials::fill_pattern(doc, id)).as_ref().is_some_and(|(grids, _)| grids.is_empty());
    let surface_pattern = inner("m_oProjFillPattern", "m_fillPatternId").and_then(|v| v.element()).filter(|id| *id > 0);
    let surface = color("m_oProjFillColor").filter(|_| flag("m_bProjFillPatternVisible") != Some(false) && surface_pattern.is_some_and(solid));
    let transparency = inner("m_oSurfacesTransparency", "m_transparency").and_then(|v| v.as_i64()).map_or(0, |t| t.clamp(0, 100) as u8);
    crate::hatching::Look {
        cut_line: ink("m_oCutPenColor"),
        seen_line: ink("m_oProjPenColor"),
        pens: [pen("m_oCutPenNumber"), pen("m_oProjPenNumber")],
        halftone: flag("m_bHalftone") == Some(true),
        pattern: pattern.and_then(|id| fills.entry(id).or_insert_with(|| super::rvt_materials::fill_pattern(doc, id)).clone()),
        color: color("m_oCutFillColor"),
        behind: color("m_oCutBackgroundPatternColorOverrider"),
        no_pattern: flag("m_bCutFillPatternVisible") == Some(false),
        no_behind: flag("m_bCutBackgroundFillPatternVisible") == Some(false),
        surface,
        transparency,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_not_known_decides_nothing() {
        assert_eq!(all([Some(true), Some(true)].into_iter()), Some(true));
        assert_eq!(all([Some(true), None].into_iter()), None);
        assert_eq!(all([None, Some(false)].into_iter()), Some(false));
        assert_eq!(any([Some(false), None].into_iter()), None);
        assert_eq!(any([None, Some(true)].into_iter()), Some(true));
        assert_eq!(any([Some(false), Some(false)].into_iter()), Some(false));
        assert_eq!(all(std::iter::empty()), Some(true));
    }

    #[test]
    fn texts_and_numbers_are_held_against_a_rule() {
        assert_eq!(text_holds("Contains", "NLRS_22_WA_KZST_150", "kzst"), Some(true));
        assert_eq!(text_holds("Equals", "Basic Wall", "basic wall"), Some(true));
        assert_eq!(text_holds("BeginsWith", "28_balk", "29"), Some(false));
        assert_eq!(text_holds("Sounds", "a", "a"), None);
        assert_eq!(number_holds("Equals", 0.0, 0.0, 0.0), Some(true));
        assert_eq!(number_holds("Greater", 1.0, 1.0, 0.0), Some(false));
        assert_eq!(number_holds("LessOrEqual", 2.0, 2.0, 1e-9), Some(true));
    }

    /// A record by hand: a class and its fields.
    fn record(class: &str, fields: serde_json::Value) -> ObjectView {
        let fields: Vec<serde_json::Value> = fields.as_object().unwrap().iter().map(|(name, value)| serde_json::json!({ "name": name, "type": "", "owner": "", "value": value })).collect();
        serde_json::from_value(serde_json::json!({ "id": 1, "class": class, "parents": [], "complete": true, "fields": fields })).unwrap()
    }

    /// A project whose views hatch by their filters (`RVT_DRAWINGS`, read-only; skipped when
    /// absent): the views with a template that overrides cut patterns give their elements looks,
    /// some with a pattern of their own; and the filters are read in reasonable time.
    #[test]
    fn the_filters_of_a_view_give_its_elements_their_looks() {
        let Some(path) = std::env::var_os("RVT_DRAWINGS").map(std::path::PathBuf::from).filter(|p| p.exists()) else {
            eprintln!("skipped: set RVT_DRAWINGS to a Revit project whose views have filters");
            return;
        };
        let doc = rvt_read::Document::open(&path).expect("rvt");
        let scene = rvt_export::mesh::scene(&doc);
        let mut by_category: HashMap<i64, Vec<u64>> = HashMap::new();
        let mut names = Names::new();
        for batch in &scene.batches {
            for (id, json) in &batch.props {
                let said: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
                let name = |key: &str| said.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string();
                names.insert(*id, (name("type"), name("family")));
            }
            for id in batch.elements.iter().map(|e| e.0) {
                if let Some(e) = doc.element(id) {
                    by_category.entry(e.category as i64).or_default().push(id);
                }
            }
        }
        let started = std::time::Instant::now();
        let mut sources = HashSet::new();
        let mut filters = Filters::new(&doc, &by_category, &names);
        let (mut looked, mut patterned) = (0, 0);
        for v in rvt_export::views::views(&doc) {
            let Some(o) = doc.object(v.id) else { continue };
            let source = o.get("m_viewTemplateId").and_then(|v| v.element()).filter(|t| *t > 0).map_or(v.id, |t| t as u64);
            if !sources.insert(source) {
                continue;
            }
            let Some(from) = doc.object(source) else { continue };
            let looks = filters.looks(&from);
            looked += usize::from(!looks.is_empty());
            patterned += usize::from(looks.values().any(|l| l.pattern.is_some()));
        }
        eprintln!("{} templates and views: {looked} give looks, {patterned} cut patterns, in {:?}", sources.len(), started.elapsed());
        assert!(patterned > 0, "some view's filters give a cut pattern");
    }

    #[test]
    fn an_element_says_its_names_and_what_its_record_holds() {
        let floor = record("Floor", serde_json::json!({ "m_pParamValueSetInt": { "class": "ParamValueSetInt", "fields": [{ "name": "m_paramSet", "type": "", "owner": "", "value": [{ "class": "ParamValueInt", "fields": [{ "name": "m_paramId", "type": "", "owner": "", "value": { "element": -1001954 } }, { "name": "m_value", "type": "", "owner": "", "value": 0 }] }] }] } }));
        let names = ("NLRS_23_FL_isolatie".to_string(), "Floor".to_string());
        assert_eq!(said(&values(&floor), Some(&names), -1001954), Some(Said::Number(0.0)));
        assert_eq!(said(&values(&floor), Some(&names), TYPE_NAME), Some(Said::Text("NLRS_23_FL_isolatie".into())));
        assert_eq!(said(&values(&floor), Some(&names), -1001955), None);
        let wall = record("SWall", serde_json::json!({ "m_isStructuralSignificant": true }));
        assert_eq!(said(&values(&wall), None, WALL_STRUCTURAL), Some(Said::Number(1.0)));
    }
}

