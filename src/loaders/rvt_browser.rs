//! How a Revit file orders its views in the project browser: its browser organizations of the
//! views, and the folders each view lies in under each of them.
//!
//! A `BrowserOrganization` of the views (`m_type` 0) has a name (`m_symbolInfo::m_name`), the
//! parameters it makes folders of (`m_folderDefinitions`, each `m_parameter::m_paramIdPath` in
//! newer files and `m_paramId` in older ones, and, when only the start of a value counts,
//! `m_numCharsToUse`), and filters that leave views out (`m_filters`: a parameter, a relation and
//! a value). A view's folders are its values for those parameters, in that order: its family, its
//! type, its name, its discipline, or a text parameter of the project that the view stores
//! (`ParamValueSetAString`).
//!
//! Which organization the browser shows is not found in the file yet: the one with the most
//! folder levels is taken, the last made of those.

use rvt_export::views::ViewInfo;
use rvt_model::{FieldValue, NestedObject, ObjectView};
use std::collections::HashSet;

/// The parameters a folder can be made of that are not stored as a text on the view.
const VIEW_NAME: i64 = -1005112;
const VIEW_FAMILY: i64 = -1012102;
const VIEW_TYPE: i64 = -1012106;
const VIEW_DISCIPLINE: i64 = -1005163;
/// The number of the sheet a view is placed on (none: not on a sheet).
const SHEET_NUMBER: i64 = -1005207;
/// What the browser writes for a view without a value.
const NONE: &str = "???";

use crate::model::ViewOrder as Organization;

/// The browser organizations of the views that order them in folders or leave some out, each
/// with the folders of `views`; and which of them the browser is taken to show.
pub(super) fn organizations(doc: &rvt_read::Document, views: &[ViewInfo]) -> (Vec<Organization>, Option<usize>) {
    let kinds = definitions(doc);
    let filtered = kinds.iter().any(|k| k.filters.iter().any(|f| f.parameter == SHEET_NUMBER));
    let placed = if filtered { on_sheets(doc) } else { HashSet::new() };
    let objects: Vec<Option<ObjectView>> = views.iter().map(|v| doc.object(v.id)).collect();
    let mut out: Vec<Organization> = Vec::new();
    let mut shown: Option<(usize, usize)> = None;
    for kind in &kinds {
        // one of the same name and folders as one listed already adds nothing
        let folders: Vec<Option<Vec<String>>> = views
            .iter()
            .zip(&objects)
            .map(|(v, o)| {
                let o = o.as_ref()?;
                let value = |parameter: i64| match parameter {
                    VIEW_NAME => Some(v.name.clone()),
                    VIEW_FAMILY => Some(family(&format!("{:?}", v.kind))),
                    VIEW_TYPE => kind_name(doc, o),
                    VIEW_DISCIPLINE => integer(o, parameter).map(discipline),
                    SHEET_NUMBER => Some(if placed.contains(&v.id) { "on".to_string() } else { String::new() }),
                    _ => text(o, parameter),
                };
                if !kind.filters.iter().all(|f| f.passes(value(f.parameter).as_deref())) {
                    return None;
                }
                let path = kind
                    .levels
                    .iter()
                    .map(|&(parameter, chars)| {
                        let value = value(parameter).filter(|s| !s.trim().is_empty()).unwrap_or_else(|| NONE.to_string());
                        if chars > 0 { value.chars().take(chars).collect() } else { value }
                    })
                    .collect();
                Some(path)
            })
            .collect();
        if out.iter().any(|o| o.name == kind.name && o.folders == folders) {
            continue;
        }
        if !kind.levels.is_empty() && shown.is_none_or(|(_, most)| kind.levels.len() >= most) {
            shown = Some((out.len(), kind.levels.len()));
        }
        out.push(Organization { name: kind.name.clone(), folders });
    }
    (out, shown.map(|(k, _)| k))
}

/// A filter of an organization: a view shows where its value for `parameter` stands in
/// `relation` to `value` (0 equals, 1 does not equal, 6 contains, 7 does not contain, 8 begins
/// with, 10 ends with; others are not read and let every view pass).
struct Filter {
    parameter: i64,
    relation: i64,
    value: String,
}

impl Filter {
    fn passes(&self, value: Option<&str>) -> bool {
        let Some(value) = value else { return true };
        let (have, want) = (value.to_lowercase(), self.value.to_lowercase());
        match self.relation {
            0 => have == want,
            1 => have != want,
            6 => have.contains(&want),
            7 => !have.contains(&want),
            8 => have.starts_with(&want),
            10 => have.ends_with(&want),
            _ => true,
        }
    }
}

/// An organization as the file defines it.
struct Definition {
    name: String,
    levels: Vec<(i64, usize)>,
    filters: Vec<Filter>,
}

/// The organizations of the views the file defines, in the order they were made: each its
/// name, its folder levels (a parameter and how many letters of its value count; 0: all) and its
/// filters. One without folders or filters lists the views as the file's kinds do and is left out.
fn definitions(doc: &rvt_read::Document) -> Vec<Definition> {
    let mut out = Vec::new();
    let mut ids = doc.object_ids_of_class("BrowserOrganization");
    ids.sort_unstable();
    for id in ids {
        let Some(o) = doc.object(id) else { continue };
        if o.get("m_type").and_then(FieldValue::as_i64) != Some(0) {
            continue;
        }
        let parameter = |d: &NestedObject| d.get("m_parameter").and_then(FieldValue::as_object).and_then(|p| p.get("m_paramIdPath")?.as_list()?.last()?.element()).or_else(|| d.get("m_paramId")?.element());
        let list = |name: &str| o.get(name).and_then(FieldValue::as_list).unwrap_or(&[]).iter().filter_map(FieldValue::as_object);
        let levels: Vec<(i64, usize)> = list("m_folderDefinitions")
            .filter_map(|d| Some((parameter(d)?, d.get("m_numCharsToUse").and_then(FieldValue::as_i64).unwrap_or(0).max(0) as usize)))
            .collect();
        let filters: Vec<Filter> = list("m_filters")
            .filter_map(|f| {
                let value = f.get("m_value").and_then(FieldValue::as_object);
                let text = value.and_then(|v| v.get("m_str")?.as_object()?.get("m_str")?.as_str().map(str::to_string)).unwrap_or_default();
                Some(Filter { parameter: parameter(f)?, relation: f.get("m_relation").and_then(FieldValue::as_i64).unwrap_or(0), value: text })
            })
            .collect();
        if levels.is_empty() && filters.is_empty() {
            continue;
        }
        let name = o.path(&["m_symbolInfo", "m_name"]).and_then(FieldValue::as_str).unwrap_or_default().to_string();
        out.push(Definition { name, levels, filters });
    }
    out
}

/// The views placed on a sheet.
fn on_sheets(doc: &rvt_read::Document) -> HashSet<u64> {
    let mut out = HashSet::new();
    for id in doc.object_ids_of_class("DBViewDrafting") {
        let Some(o) = doc.object(id) else { continue };
        if o.get("m_sheetNumber").and_then(FieldValue::as_str).is_none() {
            continue;
        }
        let Some(drawing) = o.get("m_dbDrawingId").and_then(FieldValue::element).and_then(|d| u64::try_from(d).ok()) else { continue };
        let Some(d) = doc.object_root(drawing).or_else(|| doc.object(drawing)) else { continue };
        for vp in d.get("m_viewports").and_then(FieldValue::as_list).unwrap_or(&[]) {
            let Some(vp) = vp.element().and_then(|e| u64::try_from(e).ok()) else { continue };
            let Some(v) = doc.object_root(vp).or_else(|| doc.object(vp)) else { continue };
            if let Some(view) = v.get("m_dbViewId").and_then(FieldValue::element).and_then(|e| u64::try_from(e).ok()).filter(|&view| view != id) {
                out.insert(view);
            }
        }
    }
    out
}

/// A view family as the browser names it, from the kind of the view.
fn family(kind: &str) -> String {
    match kind {
        "ThreeD" => "3D Views".to_string(),
        "FloorPlan" => "Floor Plans".to_string(),
        "CeilingPlan" => "Ceiling Plans".to_string(),
        "StructuralPlan" => "Structural Plans".to_string(),
        "AreaPlan" => "Area Plans".to_string(),
        "Section" => "Sections".to_string(),
        "Elevation" => "Elevations".to_string(),
        "Detail" => "Detail Views".to_string(),
        other => other.to_string(),
    }
}

/// A view's discipline as the browser names it.
fn discipline(value: i64) -> String {
    match value {
        1 => "Architectural",
        2 => "Structural",
        4 => "Mechanical",
        8 => "Electrical",
        16 => "Plumbing",
        4095 => "Coordination",
        _ => NONE,
    }
    .to_string()
}

/// The name of a view's type.
fn kind_name(doc: &rvt_read::Document, view: &ObjectView) -> Option<String> {
    let kind = doc.object(u64::try_from(view.get("m_dbViewTypeId")?.element()?).ok()?)?;
    Some(kind.path(&["m_symbolInfo", "m_name"])?.as_str()?.to_string())
}

/// The text a view stores for a parameter.
fn text(view: &ObjectView, parameter: i64) -> Option<String> {
    let set = view.get("m_pParamValueSetAString").and_then(|v| view.deref(v))?;
    let values = set.get("m_paramSet")?.as_list()?.iter().filter_map(FieldValue::as_object);
    let value = values.filter(|p| p.get("m_paramId").and_then(FieldValue::element) == Some(parameter)).find_map(|p| p.get("m_value")?.as_str().map(str::to_string));
    value
}

/// The whole number a view stores for a parameter.
fn integer(view: &ObjectView, parameter: i64) -> Option<i64> {
    let set = view.get("m_pParamValueSetInt").and_then(|v| view.deref(v))?;
    let values = set.get("m_paramSet")?.as_list()?.iter().filter_map(FieldValue::as_object);
    let value = values.filter(|p| p.get("m_paramId").and_then(FieldValue::element) == Some(parameter)).find_map(|p| p.get("m_value")?.as_i64());
    value
}

#[cfg(test)]
mod tests {
    /// A project whose browser orders its views in folders (`RVT_DRAWINGS`, read-only; skipped
    /// when absent).
    #[test]
    fn views_lie_in_the_folders_of_each_organization() {
        let Some(path) = std::env::var_os("RVT_DRAWINGS").map(std::path::PathBuf::from).filter(|p| p.exists()) else {
            eprintln!("skipped: set RVT_DRAWINGS to a Revit project whose browser has folders");
            return;
        };
        let doc = rvt_read::Document::open(&path).expect("rvt");
        let views = rvt_export::views::views(&doc);
        let t = std::time::Instant::now();
        let (organizations, shown) = super::organizations(&doc, &views);
        eprintln!("{} organizations in {:?}, shown {shown:?}", organizations.len(), t.elapsed());
        for o in &organizations {
            let listed = o.folders.iter().filter(|f| f.is_some()).count();
            let some: Vec<_> = views.iter().zip(&o.folders).filter_map(|(v, f)| Some((v.name.as_str(), f.as_ref()?))).take(4).collect();
            eprintln!("{:?}: {listed} of {} views; {some:?}", o.name, views.len());
        }
        assert!(!organizations.is_empty(), "no organizations");
        let shown = &organizations[shown.expect("one shown")];
        assert!(shown.folders.iter().flatten().all(|f| !f.is_empty() && f.iter().all(|s| !s.is_empty())));
    }
}
