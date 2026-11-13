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

/// The value of a parameter of an element: a name the model knows, else from its record.
fn said(element: &ObjectView, names: Option<&(String, String)>, parameter: i64) -> Option<Said> {
    match parameter {
        TYPE_NAME => return names.map(|n| Said::Text(n.0.clone())),
        FAMILY_NAME => return names.map(|n| Said::Text(n.1.clone())),
        _ => {}
    }
    for set in ["m_pParamValueSetInt", "m_pParamValueSetDouble", "m_pParamValueSetAString", "m_pParamValueSetElementId"] {
        let Some(values) = element.get(set).and_then(|v| object(element, v)) else { continue };
        for value in objects(element, values, "m_paramSet") {
            if value.get("m_paramId").and_then(|v| v.element()) != Some(parameter) {
                continue;
            }
            return match value.get("m_value")? {
                FieldValue::Str(s) => Some(Said::Text(s.clone())),
                FieldValue::Bool(b) => Some(Said::Number(f64::from(u8::from(*b)))),
                FieldValue::Element { element } => Some(Said::Number(*element as f64)),
                other => other.as_f64().or(other.as_i64().map(|i| i as f64)).map(Said::Number),
            };
        }
    }
    if parameter == WALL_STRUCTURAL {
        if let Some(FieldValue::Bool(b)) = element.get("m_isStructuralSignificant") {
            return Some(Said::Number(f64::from(u8::from(*b))));
        }
    }
    None
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
fn rule_holds(filter: &ObjectView, rule: &NestedObject, element: &ObjectView, names: Option<&(String, String)>) -> Option<bool> {
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
fn node_holds(filter: &ObjectView, node: &NestedObject, element: &ObjectView, names: Option<&(String, String)>) -> Option<bool> {
    let holds = match node.class.as_str() {
        "ElementParameterFilterData" => all(objects(filter, node, "m_rules").into_iter().map(|rule| rule_holds(filter, rule, element, names))),
        "LogicalAndFilterData" => all(objects(filter, node, "m_filters").into_iter().map(|inner| node_holds(filter, inner, element, names))),
        "LogicalOrFilterData" => any(objects(filter, node, "m_filters").into_iter().map(|inner| node_holds(filter, inner, element, names))),
        _ => None,
    };
    let inverted = matches!(node.get("m_inverted"), Some(FieldValue::Bool(true)));
    holds.map(|b| b != inverted)
}

/// The elements a view hides by its filters: of each filter that is on and hidden in the view,
/// the elements of its categories that its rules certainly hold for. `view` is the view's record
/// (or its template's), `by_category` the elements the model draws.
pub(super) fn hidden(doc: &rvt_read::Document, view: &ObjectView, by_category: &HashMap<i64, Vec<u64>>, names: &Names) -> HashSet<u64> {
    let mut out = HashSet::new();
    let Some(overrides) = view.get("m_oFilterOverrides").and_then(|v| object(view, v)) else { return out };
    for setting in objects(view, overrides, "m_oaSettings") {
        let flag = |name: &str| matches!(setting.get(name), Some(FieldValue::Bool(true)));
        if flag("m_bVisibilityFlag") || !flag("m_bEnabledFlag") {
            continue;
        }
        let Some(filter) = setting.get("m_filterElemId").and_then(|v| v.element()).and_then(|id| doc.object(u64::try_from(id).ok()?)) else { continue };
        let Some(categories) = filter.get("m_oCategoryRule").and_then(|v| object(&filter, v)) else { continue };
        let tree = filter.get("m_oRuleCombination").and_then(|v| object(&filter, v));
        for category in categories.get("m_categories").and_then(|v| v.as_list()).unwrap_or(&[]).iter().filter_map(|c| c.element()) {
            for &id in by_category.get(&category).into_iter().flatten() {
                let holds = match tree {
                    // a filter without rules takes its categories whole
                    None => true,
                    Some(tree) => doc.object(id).is_some_and(|element| node_holds(&filter, tree, &element, names.get(&id)) == Some(true)),
                };
                if holds {
                    out.insert(id);
                }
            }
        }
    }
    out
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

    #[test]
    fn an_element_says_its_names_and_what_its_record_holds() {
        let floor = record("Floor", serde_json::json!({ "m_pParamValueSetInt": { "class": "ParamValueSetInt", "fields": [{ "name": "m_paramSet", "type": "", "owner": "", "value": [{ "class": "ParamValueInt", "fields": [{ "name": "m_paramId", "type": "", "owner": "", "value": { "element": -1001954 } }, { "name": "m_value", "type": "", "owner": "", "value": 0 }] }] }] } }));
        let names = ("NLRS_23_FL_isolatie".to_string(), "Floor".to_string());
        assert_eq!(said(&floor, Some(&names), -1001954), Some(Said::Number(0.0)));
        assert_eq!(said(&floor, Some(&names), TYPE_NAME), Some(Said::Text("NLRS_23_FL_isolatie".into())));
        assert_eq!(said(&floor, Some(&names), -1001955), None);
        let wall = record("SWall", serde_json::json!({ "m_isStructuralSignificant": true }));
        assert_eq!(said(&wall, None, WALL_STRUCTURAL), Some(Said::Number(1.0)));
    }
}
