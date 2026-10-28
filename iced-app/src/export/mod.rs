//! File format writers for "Save as". Each writer turns a loaded [`Model`] into a file;
//! [`save`] picks the writer by extension.
//!
//! To add a format: implement [`Writer`] in a new module and add it to [`registry`].

mod ifc;

use crate::model::Model;
use std::path::Path;

/// A file format writer.
pub trait Writer: Send + Sync {
    /// Human-readable format name, e.g. "IFC 4".
    fn name(&self) -> &'static str;
    /// Lower-case file extensions without the dot; the first is the default.
    fn extensions(&self) -> &'static [&'static str];
    /// Writes the model to `path`.
    fn save(&self, model: &Model, path: &Path) -> Result<(), String>;
}

/// All available writers, in the order of the save dialog's filters.
pub fn registry() -> Vec<Box<dyn Writer>> {
    vec![Box::new(ifc::IfcWriter)]
}

/// Saves a model with the writer matching the extension of `path`.
pub fn save(model: &Model, path: &Path) -> Result<(), String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let writer = registry()
        .into_iter()
        .find(|w| w.extensions().contains(&ext.as_str()))
        .ok_or_else(|| format!("Cannot save as .{ext}"))?;
    writer.save(model, path)
}

#[cfg(test)]
mod tests {
    use crate::loaders;
    use std::path::PathBuf;

    fn repo_file(rel: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join(rel)
    }

    /// Every test model of the repository (all formats the viewer opens) saves as IFC.
    #[test]
    fn saves_test_models_as_ifc() {
        let dir = std::env::temp_dir().join(format!("o3v-ifc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut saved = 0;
        for entry in std::fs::read_dir(repo_file("test-models")).unwrap() {
            let path = entry.unwrap().path();
            let Ok(model) = loaders::load(&path) else { continue };
            let out = dir.join(path.file_stem().unwrap()).with_extension("ifc");
            super::save(&model, &out).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let text = std::fs::read_to_string(&out).unwrap();
            assert!(text.starts_with("ISO-10303-21;") && text.trim_end().ends_with("END-ISO-10303-21;"));
            assert!(text.contains("IFCTRIANGULATEDFACESET("), "{}: no geometry", path.display());
            saved += 1;
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(saved >= 3, "only {saved} test models saved");
    }
}
