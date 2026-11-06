//! Structural analysis models in `sam-json` v1 (`.samjson`), the vendor-neutral exchange format
//! of `bim-structural`: nodes, members, surfaces, supports, releases, sections, loads and
//! results in SI units and world metres. Read with `bim_structural::exchange::read` on every
//! target, the browser included.
//!
//! The file gives no triangles: the model is its structural parts (see `crate::structural`),
//! drawn from the document. What the reader skipped, guessed or converted (its diagnostics)
//! shows as model notes in the project information.

use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{Layer, ProjectData};
use glam::DVec3;

/// Notes listed one by one in the project information; the rest are counted (all of them are in
/// `get_structural_issues`).
const LISTED_NOTES: usize = 50;

/// A file whose origin is zero (or left out) and whose nodes lie farther than this from it (in
/// metres, on any axis) gets an anchor of its own, as the IFC loader chooses one: f32 display
/// coordinates far from zero lose millimetres.
const AUTO_ANCHOR: f64 = 1000.0;

pub struct SamLoader;

impl Loader for SamLoader {
    fn name(&self) -> &'static str {
        "Structural analysis model (sam-json)"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["samjson"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        let text = std::str::from_utf8(data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&data))
            .map_err(|_| format!("{}: sam-json must be UTF-8", source.name()))?;
        progress.set(Phase::Convert);
        let doc = bim_structural::exchange::read(text).map_err(|e| format!("{}: {e}", source.name()))?;
        let origin = anchor(&doc);
        Ok(Loaded {
            project: ProjectData { info: info(&doc, origin), ..ProjectData::default() },
            layers: Vec::new(),
            views: Vec::new(),
            origin: Some(origin),
            structural: doc.models,
        })
    }
}

/// The anchor subtracted for display: the file's origin, or, when that is zero (or left out) and
/// the nodes lie far from it, the middle of the nodes rounded to whole metres.
fn anchor(doc: &bim_structural::SamDocument) -> DVec3 {
    if doc.origin != DVec3::ZERO {
        return doc.origin;
    }
    let (mut min, mut max) = (DVec3::INFINITY, DVec3::NEG_INFINITY);
    for p in doc.models.iter().flat_map(|m| m.nodes.iter().map(|n| n.p)).filter(|p| p.is_finite()) {
        min = min.min(p);
        max = max.max(p);
    }
    if min.cmple(max).all() && min.abs().max(max.abs()).max_element() > AUTO_ANCHOR {
        ((min + max) * 0.5).round()
    } else {
        DVec3::ZERO
    }
}

/// The file's summary and model notes for the project information.
fn info(doc: &bim_structural::SamDocument, anchor: DVec3) -> Vec<(String, String)> {
    let mut info = vec![("Format".to_string(), bim_structural::exchange::FORMAT.to_string())];
    if let Some(app) = &doc.source.application {
        info.push(("Source application".into(), app.clone()));
    }
    if !doc.source.file.is_empty() {
        info.push(("Source file".into(), doc.source.file.clone()));
    }
    let chosen = if anchor != doc.origin { " (chosen: the file gives none)" } else { "" };
    info.push(("World origin (metres)".into(), format!("{}, {}, {}{chosen}", anchor.x, anchor.y, anchor.z)));
    let count = |f: fn(&bim_structural::AnalysisModel) -> usize| doc.models.iter().map(f).sum::<usize>().to_string();
    info.push(("Analysis models".into(), doc.models.iter().map(|m| m.name.clone()).collect::<Vec<_>>().join(", ")));
    info.push(("Nodes".into(), count(|m| m.nodes.len())));
    info.push(("Members".into(), count(|m| m.members.len())));
    info.push(("Surfaces".into(), count(|m| m.surfaces.len())));
    info.push(("Load cases".into(), count(|m| m.load_cases.len())));
    info.push(("Result sets".into(), count(|m| m.results.len())));
    // each note names its object by kind and source number (`node 1`), which the viewer's
    // element ids do not change, and by analysis model when the file has several
    let notes: Vec<(&bim_structural::AnalysisModel, &bim_structural::Diagnostic)> =
        doc.models.iter().flat_map(|m| m.diagnostics.iter().map(move |d| (m, d))).collect();
    info.push(("Model notes".into(), notes.len().to_string()));
    for (k, (m, d)) in notes.iter().take(LISTED_NOTES).enumerate() {
        let object = d.object.map(|id| {
            let label = crate::structural::object_label(m, id);
            if doc.models.len() > 1 { format!(" ({label}, {})", m.name) } else { format!(" ({label})") }
        });
        info.push((format!("Note {}", k + 1), format!("{}: {}{}", d.code, d.message, object.unwrap_or_default())));
    }
    if notes.len() > LISTED_NOTES {
        info.push(("More notes".into(), format!("{} more: get_structural_issues lists all", notes.len() - LISTED_NOTES)));
    }
    info
}
