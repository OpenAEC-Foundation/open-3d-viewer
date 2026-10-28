//! File format loaders. Each loader turns a [`Source`] (a file, or the bytes of a file picked in
//! the browser) into [`Layer`]s in Z-up coordinates; [`load`] picks the loader by extension and
//! wraps the result in a [`Model`].
//!
//! To add a format (DWG, E57, IFC, ...): implement [`Loader`] in a new module and add it to
//! [`registry`]. Loaders that live outside this crate (Revit, see `revit/`) are added with
//! [`add`] before the app starts.

mod gltf;
mod obj;
mod stl;

use crate::model::Model;
use std::path::Path;
use std::sync::OnceLock;
pub use openaec_model::{Loaded, Loader, Phase, Progress, Source, DEFAULT_COLOR};
use skp_reader as skp;
use tekla_reader as tekla;
use ply_reader as ply;

/// Loaders from other crates, see [`add`].
static EXTRA: OnceLock<Vec<&'static dyn Loader>> = OnceLock::new();

/// Adds loaders that live in other crates (the Revit build). Call once, before the app starts;
/// they come before the built-in loaders.
pub fn add(loaders: Vec<&'static dyn Loader>) {
    let _ = EXTRA.set(loaders);
}

/// All available loaders.
pub fn registry() -> Vec<&'static dyn Loader> {
    let builtin: [&'static dyn Loader; 6] =
        [&skp::SkpLoader, &tekla::TeklaLoader, &gltf::GltfLoader, &obj::ObjLoader, &stl::StlLoader, &ply::PlyLoader];
    EXTRA.get().into_iter().flatten().copied().chain(builtin).collect()
}

/// All supported extensions, for the file dialog filter.
pub fn supported_extensions() -> Vec<&'static str> {
    registry().iter().flat_map(|l| l.extensions().iter().copied()).collect()
}

/// Loads a file with the loader matching its extension.
pub fn load(path: &Path) -> Result<Model, String> {
    load_with(&Source::from(path), &Progress::default())
}

/// As [`load`], for any source, reporting the steps to `progress`.
pub fn load_with(source: &Source, progress: &Progress) -> Result<Model, String> {
    let ext = source.extension();
    let loader = registry()
        .into_iter()
        .find(|l| l.extensions().contains(&ext.as_str()))
        .ok_or_else(|| format!("Unsupported file type: .{ext}"))?;
    openaec_model::load_with_loader(loader, source, progress)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn repo_file(rel: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
    }

    #[test]
    fn loads_obj_cube() {
        let m = load(&repo_file("test-models/test-cube.obj")).expect("obj");
        assert!(m.triangle_count() >= 12, "{} triangles", m.triangle_count());
        assert!(m.bounds.is_valid());
    }

    #[test]
    fn loads_stl_cube() {
        let m = load(&repo_file("test-models/test-cube.stl")).expect("stl");
        assert_eq!(m.triangle_count(), 12);
        assert!(m.bounds.is_valid());
    }

    #[test]
    fn loads_bytes_as_from_a_browser() {
        let data = std::fs::read(repo_file("test-models/test-cube.stl")).unwrap();
        let source = Source::Bytes { name: "cube.STL".into(), data: Arc::new(data) };
        let m = load_with(&source, &Progress::default()).expect("stl bytes");
        assert_eq!(m.triangle_count(), 12);
        assert_eq!(m.file_name(), "cube.STL");
        assert!(m.file_size > 0);
    }

    #[test]
    fn rejects_unknown_extension() {
        assert!(load(Path::new("x.unknownext")).is_err());
    }
}
