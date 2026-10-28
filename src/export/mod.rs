//! File format writers for "Save as". Each writer turns a loaded [`Model`] into a file;
//! [`save`] picks the writer by extension.
//!
//! To add a format: implement [`Writer`] in a new module and add it to [`registry`].

mod ifc;

use crate::model::Model;
use std::io;
use std::path::{Path, PathBuf};

/// A file format writer.
pub trait Writer: Send + Sync {
    /// Human-readable format name, e.g. "IFC 4".
    fn name(&self) -> &'static str;
    /// Lower-case file extensions without the dot; the first is the default.
    fn extensions(&self) -> &'static [&'static str];
    /// MIME type of the file, for the browser download.
    fn mime(&self) -> &'static str {
        "application/octet-stream"
    }
    /// Writes the model as the file `file_name` to `out`.
    fn write(&self, model: &Model, file_name: &str, out: &mut dyn io::Write) -> io::Result<()>;
}

/// All available writers, in the order of the save dialog's filters.
pub fn registry() -> Vec<Box<dyn Writer>> {
    vec![Box::new(ifc::IfcWriter)]
}

/// The writer for the extension of `file_name`.
pub fn writer_for(file_name: &str) -> Result<Box<dyn Writer>, String> {
    let ext = Path::new(file_name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    registry()
        .into_iter()
        .find(|w| w.extensions().contains(&ext.as_str()))
        .ok_or_else(|| format!("Cannot save as .{ext}"))
}

/// Saves a model with the writer matching the extension of `path`. Writes next to the target and
/// renames, so a failed save leaves no half file behind.
pub fn save(model: &Model, path: &Path) -> Result<(), String> {
    let file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let writer = writer_for(&file_name)?;
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    let written = (|| -> io::Result<()> {
        let mut out = io::BufWriter::with_capacity(1 << 20, std::fs::File::create(&part)?);
        writer.write(model, &file_name, &mut out)?;
        out.into_inner().map_err(|e| e.into_error())?.sync_all()
    })();
    match written.and_then(|()| std::fs::rename(&part, path)) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(format!("Cannot save {file_name}: {e}"))
        }
    }
}

/// The model as the file `file_name`, in memory (the web app hands it to the browser).
pub fn to_bytes(model: &Model, file_name: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    writer_for(file_name)?.write(model, file_name, &mut out).map_err(|e| format!("Cannot save {file_name}: {e}"))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use crate::loaders;
    use std::path::PathBuf;

    fn repo_file(rel: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
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
