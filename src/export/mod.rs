//! File format writers for "Save as". Each writer turns a loaded [`Model`] into a file;
//! [`save`] picks the writer by extension.
//!
//! To add a format: implement [`Writer`] in a new module and add it to [`registry`].

mod ifc;

use crate::document::Document;
use crate::model::Model;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

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
    /// Writes the model, with what the document adds to it (levels), as the file `file_name` to
    /// `out`.
    fn write(&self, model: &Model, document: &Document, file_name: &str, out: &mut dyn io::Write) -> io::Result<()>;
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

/// Saves a model with the writer matching the extension of `path`, through [`write_file`]: a failed
/// save leaves no half file behind, and an existing file is only replaced when `replace`.
pub fn save(model: &Model, document: &Document, path: &Path, replace: bool) -> Result<(), String> {
    let file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let writer = writer_for(&file_name)?;
    write_file(path, replace, |out| writer.write(model, document, &file_name, out)).map_err(|e| format!("Cannot save {file_name}: {e}"))
}

/// The temporary files of the writes under way, so a stop (SIGTERM) can wait for them or clean up.
static WRITING: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// The temporary files of the writes under way.
pub fn writing() -> Vec<PathBuf> {
    WRITING.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Writes `path` through a temporary file next to it (`<path>.part`) that is renamed into place at
/// the end, so a failed write leaves no half file. The temporary file is always a new one, never
/// written through a link someone left at its name. Without `replace`, a file at `path` is never
/// replaced, also not one made while writing (a large model takes minutes to save).
pub fn write_file(path: &Path, replace: bool, write: impl FnOnce(&mut dyn io::Write) -> io::Result<()>) -> io::Result<()> {
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    let create = || std::fs::OpenOptions::new().write(true).create_new(true).open(&part);
    let file = match create() {
        // left by a stop, or a link: remove that name itself (not what a link leads to)
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            std::fs::remove_file(&part)?;
            create()?
        }
        other => other?,
    };
    WRITING.lock().unwrap_or_else(|e| e.into_inner()).push(part.clone());
    let written = (|| {
        let mut out = io::BufWriter::with_capacity(1 << 20, file);
        write(&mut out)?;
        out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        put_in_place(&part, path, replace)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    WRITING.lock().unwrap_or_else(|e| e.into_inner()).retain(|p| *p != part);
    written
}

fn put_in_place(part: &Path, path: &Path, replace: bool) -> io::Result<()> {
    if replace {
        return std::fs::rename(part, path);
    }
    let exists = || io::Error::new(io::ErrorKind::AlreadyExists, format!("{} exists (made while writing); nothing replaced", path.display()));
    // a hard link is never made over anything at `path`, which a rename would replace
    match std::fs::hard_link(part, path) {
        Ok(()) => {
            let _ = std::fs::remove_file(part);
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(exists()),
        // a file system without hard links: check, then rename
        Err(_) if path.symlink_metadata().is_ok() => Err(exists()),
        Err(_) => std::fs::rename(part, path),
    }
}

/// The model as the file `file_name`, in memory (the web app hands it to the browser).
pub fn to_bytes(model: &Model, document: &Document, file_name: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    writer_for(file_name)?.write(model, document, file_name, &mut out).map_err(|e| format!("Cannot save {file_name}: {e}"))?;
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
            // point clouds are left out of IFC: nothing to save
            if model.triangle_count() == 0 {
                continue;
            }
            let out = dir.join(path.file_stem().unwrap()).with_extension("ifc");
            super::save(&model, &crate::document::Document::from_model(&model), &out, true).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let text = std::fs::read_to_string(&out).unwrap();
            assert!(text.starts_with("ISO-10303-21;") && text.trim_end().ends_with("END-ISO-10303-21;"));
            assert!(text.contains("IFCTRIANGULATEDFACESET("), "{}: no geometry", path.display());
            saved += 1;
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(saved >= 3, "only {saved} test models saved");
    }

    /// Files are written new and put in place at the end: an existing file only with `replace`,
    /// and never through a link left at the temporary name.
    #[test]
    fn writes_new_files_and_replaces_only_when_asked() {
        let dir = std::env::temp_dir().join(format!("o3v-write-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.ifc");
        let write = |text: &'static [u8]| move |out: &mut dyn std::io::Write| out.write_all(text);
        super::write_file(&path, false, write(b"one")).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"one");
        // made while a save without overwrite ran: kept
        let err = super::write_file(&path, false, write(b"two")).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&path).unwrap(), b"one");
        super::write_file(&path, true, write(b"three")).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"three");
        // a stale temporary file is no obstacle, and nothing is left behind
        std::fs::write(dir.join("b.ifc.part"), b"stale").unwrap();
        super::write_file(&dir.join("b.ifc"), false, write(b"b")).unwrap();
        #[cfg(unix)]
        {
            let outside = dir.join("outside");
            std::fs::write(&outside, b"keep").unwrap();
            std::os::unix::fs::symlink(&outside, dir.join("c.ifc.part")).unwrap();
            super::write_file(&dir.join("c.ifc"), false, write(b"c")).unwrap();
            assert_eq!(std::fs::read(&outside).unwrap(), b"keep", "written through the link");
            assert_eq!(std::fs::read(dir.join("c.ifc")).unwrap(), b"c");
        }
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.path().extension().is_some_and(|x| x == "part")).collect();
        assert!(left.is_empty(), "{left:?}");
        assert!(!super::writing().iter().any(|p| p.starts_with(&dir)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
