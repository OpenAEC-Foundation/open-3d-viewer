//! How a Revit file orders its views in the project browser: the folders each view lies in.
//!
//! A `BrowserOrganization` of the views (`m_type` 0) names the parameters it makes folders of
//! (`m_folderDefinitions`: `m_parameter::m_paramIdPath` and, when only the start of a value
//! counts, `m_numCharsToUse`). A view's folders are its values for those parameters, in that
//! order: its family, its type, its name, or a text parameter of the project that the view
//! stores (`ParamValueSetAString`).
//!
//! Which organization the browser shows is not found in the file yet: the one with the most
//! folder levels is taken, the last made of those.

use rvt_export::views::ViewInfo;
use rvt_model::{FieldValue, ObjectView};
use std::collections::HashMap;

/// The parameters a folder can be made of that are not stored as a text on the view.
const VIEW_NAME: i64 = -1005112;
const VIEW_FAMILY: i64 = -1012102;
const VIEW_TYPE: i64 = -1012106;
/// What the browser writes for a view without a value.
const NONE: &str = "???";

/// The folders of each view, by the view's element; empty when the file orders its views in no
/// folders.
pub(super) fn folders(doc: &rvt_read::Document, views: &[ViewInfo]) -> HashMap<u64, Vec<String>> {
    let levels = organization(doc);
    if levels.is_empty() {
        return HashMap::new();
    }
    views
        .iter()
        .filter_map(|v| {
            let o = doc.object(v.id)?;
            let path = levels
                .iter()
                .map(|&(parameter, chars)| {
                    let value = match parameter {
                        VIEW_NAME => Some(v.name.clone()),
                        VIEW_FAMILY => Some(family(&format!("{:?}", v.kind))),
                        VIEW_TYPE => kind_name(doc, &o),
                        _ => text(&o, parameter),
                    };
                    let value = value.filter(|s| !s.trim().is_empty()).unwrap_or_else(|| NONE.to_string());
                    if chars > 0 { value.chars().take(chars).collect() } else { value }
                })
                .collect();
            Some((v.id, path))
        })
        .collect()
}

/// The folder levels of the views' organization: each a parameter and how many letters of its
/// value count (0: all).
fn organization(doc: &rvt_read::Document) -> Vec<(i64, usize)> {
    let mut best: Vec<(i64, usize)> = Vec::new();
    for id in doc.object_ids_of_class("BrowserOrganization") {
        let Some(o) = doc.object(id) else { continue };
        if o.get("m_type").and_then(FieldValue::as_i64) != Some(0) {
            continue;
        }
        let levels: Vec<(i64, usize)> = o
            .get("m_folderDefinitions")
            .and_then(FieldValue::as_list)
            .unwrap_or(&[])
            .iter()
            .filter_map(FieldValue::as_object)
            .filter_map(|d| {
                let parameter = d.get("m_parameter")?.as_object()?.get("m_paramIdPath")?.as_list()?.last()?.element()?;
                let chars = d.get("m_numCharsToUse").and_then(FieldValue::as_i64).unwrap_or(0).max(0) as usize;
                Some((parameter, chars))
            })
            .collect();
        if levels.len() >= best.len() && !levels.is_empty() {
            best = levels;
        }
    }
    best
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

#[cfg(test)]
mod tests {
    /// A project whose browser orders its views in folders (`RVT_DRAWINGS`, read-only; skipped
    /// when absent).
    #[test]
    fn views_lie_in_the_folders_of_the_browser() {
        let Some(path) = std::env::var_os("RVT_DRAWINGS").map(std::path::PathBuf::from).filter(|p| p.exists()) else {
            eprintln!("skipped: set RVT_DRAWINGS to a Revit project whose browser has folders");
            return;
        };
        let doc = rvt_read::Document::open(&path).expect("rvt");
        let views = rvt_export::views::views(&doc);
        let folders = super::folders(&doc, &views);
        for v in views.iter().take(12) {
            eprintln!("{:?} in {:?}", v.name, folders.get(&v.id));
        }
        assert!(!folders.is_empty(), "no folders");
        assert!(folders.values().all(|f| !f.is_empty() && f.iter().all(|s| !s.is_empty())));
    }
}
