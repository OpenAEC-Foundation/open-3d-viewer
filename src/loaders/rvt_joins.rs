//! The joins of a Revit project: which elements are joined to which. An element keeps, among the
//! steps that shape it, one for every element it is joined with (their geometry cleaned up
//! where they meet) and one for every element that cuts it or that it cuts. A drawing leaves out
//! the line between two joined elements of one material; the elements themselves stay apart.

use std::collections::HashSet;

/// The steps of an element that name another it is joined with.
const STEPS: [&str; 2] = ["CleanPairGStep", "CutPairGStep"];

/// The pairs of joined elements among `elements` (the smaller id first).
pub fn joins(doc: &rvt_read::Document, elements: impl Iterator<Item = u64>) -> HashSet<(u64, u64)> {
    let mut out = HashSet::new();
    for id in elements {
        let Some(o) = doc.object(id) else { continue };
        for step in o.objects.iter().filter(|s| STEPS.contains(&s.class.as_str())) {
            // a step that does nothing joins nothing
            let idle = matches!(step.get("m_doNothing"), Some(rvt_model::FieldValue::Bool(true)));
            let other = step.get("m_otherId").and_then(|v| v.element()).and_then(|e| u64::try_from(e).ok());
            if let Some(other) = other.filter(|other| !idle && *other != id) {
                out.insert((id.min(other), id.max(other)));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A project with joined elements (`RVT_DRAWINGS`, read-only; skipped when absent): its walls
    /// name the elements they are joined with.
    #[test]
    fn elements_name_what_they_are_joined_with() {
        let Some(path) = std::env::var_os("RVT_DRAWINGS").map(std::path::PathBuf::from).filter(|p| p.exists()) else {
            eprintln!("skipped: set RVT_DRAWINGS to a Revit project with joined elements");
            return;
        };
        let doc = rvt_read::Document::open(&path).expect("rvt");
        let walls = doc.object_ids_of_class("SWall");
        let pairs = joins(&doc, walls.iter().copied());
        eprintln!("{} walls, {} joined pairs", walls.len(), pairs.len());
        assert!(!pairs.is_empty());
        assert!(pairs.iter().all(|(a, b)| a < b), "the smaller id first, and none with itself");
        assert!(pairs.iter().any(|(a, b)| walls.contains(a) != walls.contains(b)), "a wall joined with another kind of element");
    }
}
