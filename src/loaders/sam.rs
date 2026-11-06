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

/// Notes listed one by one in the project information; the rest are counted (all of them are in
/// `get_structural_issues`).
const LISTED_NOTES: usize = 50;

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
        Ok(Loaded {
            project: ProjectData { info: info(&doc), ..ProjectData::default() },
            layers: Vec::new(),
            views: Vec::new(),
            origin: Some(doc.origin),
            structural: doc.models,
        })
    }
}

/// The file's summary and model notes for the project information.
fn info(doc: &bim_structural::SamDocument) -> Vec<(String, String)> {
    let mut info = vec![("Format".to_string(), bim_structural::exchange::FORMAT.to_string())];
    if let Some(app) = &doc.source.application {
        info.push(("Source application".into(), app.clone()));
    }
    if !doc.source.file.is_empty() {
        info.push(("Source file".into(), doc.source.file.clone()));
    }
    info.push(("World origin (metres)".into(), format!("{}, {}, {}", doc.origin.x, doc.origin.y, doc.origin.z)));
    let count = |f: fn(&bim_structural::AnalysisModel) -> usize| doc.models.iter().map(f).sum::<usize>().to_string();
    info.push(("Analysis models".into(), doc.models.iter().map(|m| m.name.clone()).collect::<Vec<_>>().join(", ")));
    info.push(("Nodes".into(), count(|m| m.nodes.len())));
    info.push(("Members".into(), count(|m| m.members.len())));
    info.push(("Surfaces".into(), count(|m| m.surfaces.len())));
    info.push(("Load cases".into(), count(|m| m.load_cases.len())));
    info.push(("Result sets".into(), count(|m| m.results.len())));
    let notes: Vec<&bim_structural::Diagnostic> = doc.models.iter().flat_map(|m| &m.diagnostics).collect();
    info.push(("Model notes".into(), notes.len().to_string()));
    for (k, d) in notes.iter().take(LISTED_NOTES).enumerate() {
        let object = d.object.map(|id| format!(" (object {id})")).unwrap_or_default();
        info.push((format!("Note {}", k + 1), format!("{}: {}{object}", d.code, d.message)));
    }
    if notes.len() > LISTED_NOTES {
        info.push(("More notes".into(), format!("{} more: get_structural_issues lists all", notes.len() - LISTED_NOTES)));
    }
    info
}
