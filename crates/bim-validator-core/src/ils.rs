//! BIM basis ILS (digiGO, Netherlands) as a built-in rule set. Codes are `ILS-` plus a topic;
//! they name the ILS subject, not an official clause number. Requirements that need geometry or
//! georeferencing (origin, coordinates) are outside this profile.
use crate::checks::{is_nlsfb_system, material_rule, proxy_rule, tick, Model, Rule};
use crate::graph::{number, text, Node};
use crate::values::pattern_match;
use crate::{IlsOptions, RuleSource, Severity, SpecificationResult};
use ifc_lite_core::AttributeValue as A;
use std::collections::BTreeMap;

/// Reject unusable patterns before the IFC is parsed.
pub fn admit(o: &IlsOptions) -> Result<(), String> {
    pattern_match(&o.storey_name_pattern, "")
        .map_err(|e| format!("Invalid storey name pattern: {e}"))?;
    pattern_match(&o.file_name_pattern, "")
        .map_err(|e| format!("Invalid file name pattern: {e}"))?;
    Ok(())
}

// (class, common property set, IFC4 and later only).
const LOAD_BEARING: &[(&str, &str, bool)] = &[
    ("IFCBEAM", "Pset_BeamCommon", false),
    ("IFCCOLUMN", "Pset_ColumnCommon", false),
    ("IFCMEMBER", "Pset_MemberCommon", false),
    ("IFCPLATE", "Pset_PlateCommon", false),
    ("IFCSLAB", "Pset_SlabCommon", false),
    ("IFCWALL", "Pset_WallCommon", false),
    ("IFCFOOTING", "Pset_FootingCommon", true),
    ("IFCPILE", "Pset_PileCommon", true),
    ("IFCRAMP", "Pset_RampCommon", true),
    ("IFCROOF", "Pset_RoofCommon", true),
    ("IFCSTAIR", "Pset_StairCommon", true),
    ("IFCCHIMNEY", "Pset_ChimneyCommon", true),
];
const IS_EXTERNAL: &[(&str, &str, bool)] = &[
    ("IFCWALL", "Pset_WallCommon", false),
    ("IFCCURTAINWALL", "Pset_CurtainWallCommon", false),
    ("IFCSLAB", "Pset_SlabCommon", false),
    ("IFCROOF", "Pset_RoofCommon", false),
    ("IFCWINDOW", "Pset_WindowCommon", false),
    ("IFCDOOR", "Pset_DoorCommon", false),
];

pub fn run(m: &Model, o: &IlsOptions) -> Result<Vec<SpecificationResult>, String> {
    let mut out = vec![file_name(m, o)];
    tick(m.cancel)?;
    out.push(storey_names(m, o)?);
    out.push(storey_elevations(m)?);
    out.push(nlsfb(m)?);
    out.push(material_rule(
        m,
        rule(m, Severity::Error, "ILS-MATERIAL", "Material (materiaal)"),
    )?);
    out.push(boolean_property(
        m,
        "ILS-LOADBEARING",
        "Load bearing (dragend)",
        LOAD_BEARING,
        "LoadBearing",
    )?);
    out.push(boolean_property(
        m,
        "ILS-ISEXTERNAL",
        "External (buiten)",
        IS_EXTERNAL,
        "IsExternal",
    )?);
    out.push(fire_rating(m)?);
    out.push(proxy_rule(
        m,
        rule(
            m,
            Severity::Warning,
            "ILS-PROXY",
            "Proxy elements (IfcBuildingElementProxy)",
        ),
    )?);
    Ok(out)
}
fn rule<'a>(m: &Model<'a>, severity: Severity, code: &str, name: &str) -> Rule<'a> {
    Rule::new(m.g, RuleSource::Ils, severity, code, name)
}

fn file_name(m: &Model, o: &IlsOptions) -> SpecificationResult {
    let mut r = rule(m, Severity::Warning, "ILS-FILE", "File name (bestandsnaam)");
    match o.file_name.as_deref() {
        None => r.not_checkable("No file name supplied".into()),
        Some(name) => match pattern_match(&o.file_name_pattern, name) {
            Ok(true) => r.note(format!("'{name}' matches {}", o.file_name_pattern)),
            Ok(false) => r.fail(format!(
                "'{name}' does not match <project>_<discipline>_<part>.ifc ({})",
                o.file_name_pattern
            )),
            Err(e) => r.not_checkable(e),
        },
    }
    r.finish()
}

fn storeys<'a>(m: &Model<'a>) -> Vec<&'a Node> {
    let g: &'a crate::graph::Graph = m.g;
    g.nodes
        .values()
        .filter(|n| n.kind == "IFCBUILDINGSTOREY")
        .collect()
}
fn storey_names(m: &Model, o: &IlsOptions) -> Result<SpecificationResult, String> {
    let mut r = rule(
        m,
        Severity::Error,
        "ILS-STOREY-NAME",
        "Storey naming (bouwlaagnaam)",
    );
    let all = storeys(m);
    for n in &all {
        tick(m.cancel)?;
        let problems = match m.g.name(n) {
            None => vec!["Storey has no name".into()],
            Some(name) if !pattern_match(&o.storey_name_pattern, &name)? => vec![format!(
                "Storey name '{name}' does not match {} (e.g. '00 begane grond', '-01 kelder')",
                o.storey_name_pattern
            )],
            Some(_) => vec![],
        };
        r.check(n, "Name", problems);
    }
    if all.is_empty() {
        r.fail("The model contains no IfcBuildingStorey".into());
    }
    Ok(r.finish())
}

/// Storeys are compared within their building; elevations closer than 1 mm are equal.
fn storey_elevations(m: &Model) -> Result<SpecificationResult, String> {
    let g = m.g;
    let mut r = rule(
        m,
        Severity::Error,
        "ILS-STOREY-ELEV",
        "Unique storey elevations (bouwlaaghoogte)",
    );
    let tolerance = m.length_unit().1.map_or(1e-9, |scale| 0.001 / scale);
    let mut buildings: BTreeMap<Option<u32>, Vec<(&Node, Option<f64>)>> = BTreeMap::new();
    for n in storeys(m) {
        let mut building = None;
        let mut current = n.id;
        for _ in 0..64 {
            let Some(parent) = m.related(current, &["IFCRELAGGREGATES"]).first().copied() else {
                break;
            };
            if m.is(parent, "IFCBUILDING") {
                building = Some(parent.id);
                break;
            }
            current = parent.id;
        }
        buildings
            .entry(building)
            .or_default()
            .push((n, g.attr(n, "Elevation").and_then(number)));
    }
    for group in buildings.values() {
        for (n, elevation) in group {
            tick(m.cancel)?;
            let problems = match elevation {
                None => vec!["Elevation is missing".into()],
                Some(e) => group
                    .iter()
                    .filter(|(o, v)| o.id != n.id && v.is_some_and(|v| (v - e).abs() < tolerance))
                    .map(|(o, _)| {
                        format!(
                            "Elevation {e} equals storey #{} '{}'",
                            o.id,
                            g.name(o).unwrap_or_default()
                        )
                    })
                    .collect(),
            };
            r.check(n, "Elevation", problems);
        }
    }
    Ok(r.finish())
}

fn nlsfb(m: &Model) -> Result<SpecificationResult, String> {
    let mut r = rule(m, Severity::Error, "ILS-NLSFB", "NL-SfB classification");
    for n in &m.elements {
        tick(m.cancel)?;
        let problems = match m.nlsfb(n) {
            Ok(_) => vec![],
            Err(reason) => vec![reason],
        };
        r.check(n, "classification", problems);
    }
    let systems: Vec<_> = m
        .elements
        .iter()
        .flat_map(|n| m.classifications(n))
        .filter_map(|(s, _)| s)
        .filter(|s| !is_nlsfb_system(s))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    if !systems.is_empty() {
        r.note(format!(
            "Other classification systems present: {}",
            systems.join(", ")
        ));
    }
    Ok(r.finish())
}

fn boolean(a: &A) -> Option<bool> {
    match a {
        A::Enum(s) if s == "T" => Some(true),
        A::Enum(s) if s == "F" => Some(false),
        A::Typed { name, value } if matches!(name.as_ref(), "IFCBOOLEAN" | "IFCLOGICAL") => {
            boolean(value)
        }
        _ => None,
    }
}
fn boolean_property(
    m: &Model,
    code: &str,
    name: &str,
    classes: &[(&'static str, &str, bool)],
    property: &str,
) -> Result<SpecificationResult, String> {
    let ifc2x3 = m.g.schema == ifc_lite_core::SchemaVersion::Ifc2x3;
    let mut r = rule(m, Severity::Error, code, name);
    for n in &m.elements {
        tick(m.cancel)?;
        let Some((_, pset, _)) = classes
            .iter()
            .find(|(class, _, ifc4)| !(*ifc4 && ifc2x3) && m.is(n, class))
        else {
            continue;
        };
        let problems = match m.property(n, |s| s == *pset, property) {
            None => vec![format!("{pset}.{property} is missing")],
            Some((_, value)) if boolean(value).is_none() => vec![format!(
                "{pset}.{property} must be a boolean (found {})",
                text(value).unwrap_or_else(|| "no value".into())
            )],
            Some(_) => vec![],
        };
        r.check(n, "property", problems);
    }
    Ok(r.finish())
}

/// Fire rating in whole minutes: `60`, `60 min`, `60 minuten`. Numeric values are minutes.
pub fn fire_rating_minutes(a: &A) -> bool {
    if let Some(v) = number(a) {
        return v >= 0.;
    }
    let Some(s) = text(a) else {
        return false;
    };
    let t = s.trim().to_ascii_lowercase();
    let end = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
    end > 0 && matches!(t[end..].trim(), "" | "min" | "min." | "minutes" | "minuten")
}
fn fire_rating(m: &Model) -> Result<SpecificationResult, String> {
    let mut r = rule(
        m,
        Severity::Error,
        "ILS-FIRERATING",
        "Fire rating in minutes (brandwerendheid)",
    );
    let common = |s: &str| s.starts_with("Pset_") && s.ends_with("Common");
    for n in &m.elements {
        tick(m.cancel)?;
        let Some((pset, value)) = m.property(n, common, "FireRating") else {
            continue;
        };
        let problems = if fire_rating_minutes(value) {
            vec![]
        } else {
            vec![format!(
                "{pset}.FireRating '{}' is not a number of minutes (e.g. 30, 60, 90)",
                text(value).unwrap_or_default()
            )]
        };
        r.check(n, "property", problems);
    }
    Ok(r.finish())
}
