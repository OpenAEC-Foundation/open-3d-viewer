//! The phasing of a Revit project: the order of its phases, the phase every element is made and
//! demolished in, the override the project gives each status (halftone or not), and for every
//! view its phase and what its phase filter does with each status.

use crate::phasing::{Phasing, ViewPhase};
use rvt_model::FieldValue;
use std::collections::HashMap;

/// The parameters of a view that name its phase filter and its phase.
const PHASE_FILTER: i64 = -1012103;
const PHASE: i64 = -1012102;

/// An element a view names in one of its parameters.
fn named(o: &rvt_model::ObjectView, parameter: i64) -> Option<i64> {
    let set = o.deref(o.get("m_pParamValueSetElementId")?)?;
    set.get("m_paramSet")?.as_list()?.iter().filter_map(FieldValue::as_object).find_map(|p| {
        (p.get("m_paramId")?.element()? == parameter).then(|| p.get("m_value")?.element()).flatten()
    })
}

/// What a phase filter does with each status, by the status's number.
fn filter(doc: &rvt_read::Document, id: i64) -> Option<[u8; 7]> {
    let o = doc.object(u64::try_from(id).ok()?)?;
    let list = o.get("m_phaseStatusPresentation")?.as_list()?;
    let mut show = [0u8; 7];
    for (k, v) in list.iter().take(7).enumerate() {
        show[k] = v.as_i64()? as u8;
    }
    Some(show)
}

/// The phasing of the project, its views named by `path` (a view's element to its path in the
/// document); empty for a project without phases.
pub fn phasing(doc: &rvt_read::Document, views: &[u64], path: &dyn Fn(u64) -> String) -> Phasing {
    let mut out = Phasing::default();
    let Some(all) = doc.object_ids_of_class("AllProjectPhases").first().and_then(|id| doc.object(*id)) else { return out };
    // the phases in the project's order
    let order: HashMap<i64, u16> = all
        .get("m_phaseIds")
        .and_then(FieldValue::as_list)
        .unwrap_or_default()
        .iter()
        .filter_map(FieldValue::element)
        .enumerate()
        .map(|(k, id)| (id, k as u16))
        .collect();
    if order.is_empty() {
        return out;
    }
    // the override of each status: halftone or not
    for o in all.get("m_arrPhasingOverrides").and_then(FieldValue::as_list).unwrap_or_default().iter().filter_map(|v| all.deref(v)) {
        let status = o.get("m_elementOnPhaseStatus").and_then(FieldValue::as_i64).unwrap_or(-1);
        let halftone = o.get("m_oOverrideGraphicSettings").and_then(|v| all.deref(v)).and_then(|s| s.get("m_bHalftone")).and_then(|v| match v {
            FieldValue::Bool(b) => Some(*b),
            _ => None,
        });
        if let (Ok(status @ 0..=6), Some(halftone)) = (usize::try_from(status), halftone) {
            out.halftone[status] = halftone;
        }
    }
    for (element, (created, demolished)) in doc.element_phases() {
        if let Some(&created) = order.get(&created) {
            out.elements.insert(element, (created, demolished.and_then(|d| order.get(&d).copied())));
        }
    }
    let mut filters: HashMap<i64, Option<[u8; 7]>> = HashMap::new();
    for &view in views {
        let Some(o) = doc.object(view) else { continue };
        let (Some(phase), Some(id)) = (named(&o, PHASE).and_then(|p| order.get(&p).copied()), named(&o, PHASE_FILTER)) else { continue };
        if let Some(show) = *filters.entry(id).or_insert_with(|| filter(doc, id)) {
            out.views.insert(path(view), ViewPhase { phase, show });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phasing::Look;

    /// A project with phases (`RVT_DRAWINGS`, read-only; skipped when absent): its views have a
    /// phase and a filter, and existing work is drawn in halftone where the filter overrides it.
    #[test]
    fn views_draw_by_their_phase_and_filter() {
        let Some(file) = std::env::var_os("RVT_DRAWINGS").map(std::path::PathBuf::from).filter(|p| p.exists()) else {
            eprintln!("skipped: set RVT_DRAWINGS to a Revit project with phases");
            return;
        };
        let doc = rvt_read::Document::open(&file).expect("rvt");
        let infos = rvt_export::views::views(&doc);
        let views: Vec<u64> = infos.iter().map(|v| v.id).collect();
        let p = phasing(&doc, &views, &|id| id.to_string());
        eprintln!("{} elements with a phase, {} views with a phase, halftone {:?}", p.elements.len(), p.views.len(), p.halftone);
        assert!(!p.elements.is_empty() && !p.views.is_empty());
        assert!(p.halftone[2], "existing work is overridden in halftone");
        // per view: how many of the elements it draws each way
        for v in infos.iter().filter(|v| p.views.contains_key(&v.id.to_string())).take(60) {
            let path = v.id.to_string();
            let count = |look: Look| p.elements.keys().filter(|e| p.look(&path, **e) == look).count();
            eprintln!("  {:?}: {:?} normal {} halftone {} hidden {}", v.name, p.views[&path], count(Look::Normal), count(Look::Halftone), count(Look::Hidden));
        }
    }
}
