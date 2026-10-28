//! File format loaders. Each loader turns a [`Source`] (a file, or the bytes of a file picked in
//! the browser) into [`Layer`]s in Z-up coordinates; [`load`] picks the loader by extension and
//! wraps the result in a [`Model`].
//!
//! To add a format (DWG, E57, IFC, ...): implement [`Loader`] in a new module and add it to
//! [`registry`]. Additional loaders that live outside this crate are added with
//! [`add`] before the app starts.

mod gltf;
mod obj;
mod stl;
#[cfg(not(target_arch = "wasm32"))]
mod rvt;

use crate::model::Model;
use std::path::Path;
use std::sync::OnceLock;
pub use openaec_model::{Loaded, Loader, Phase, Progress, Source, DEFAULT_COLOR};
use skp_reader as skp;
use tekla_reader as tekla;
use ply_reader as ply;

/// Loaders from other crates, see [`add`].
static EXTRA: OnceLock<Vec<&'static dyn Loader>> = OnceLock::new();

/// Adds additional loaders that live in other crates. Call once, before the app starts;
/// they come before the built-in loaders.
pub fn add(loaders: Vec<&'static dyn Loader>) {
    let _ = EXTRA.set(loaders);
}

/// All available loaders.
pub fn registry() -> Vec<&'static dyn Loader> {
    let builtin: Vec<&'static dyn Loader> = vec![
        &skp::SkpLoader, &tekla::TeklaLoader, &gltf::GltfLoader, &obj::ObjLoader, &stl::StlLoader, &ply::PlyLoader,
        #[cfg(not(target_arch = "wasm32"))]
        &rvt::RvtLoader,
    ];
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
    fn extracted_readers_remain_registered() {
        for (extension, name) in [("skp", "SketchUp"), ("db1", "Tekla Structures"), ("ply", "PLY")] {
            let loader = registry().into_iter().find(|l| l.extensions().contains(&extension)).expect(extension);
            assert_eq!(loader.name(), name);
        }
    }

    #[test]
    fn extracted_skp_reader_loads_files_and_browser_bytes() {
        let path = repo_file("test-models/TestModel.skp");
        let file = load(&path).expect("SketchUp file");
        let source = Source::Bytes { name: "TestModel.SKP".into(), data: Arc::new(std::fs::read(path).unwrap()) };
        let bytes = load_with(&source, &Progress::default()).expect("SketchUp browser bytes");
        assert!(file.triangle_count() > 100);
        assert_eq!(file.triangle_count(), bytes.triangle_count());
        assert_eq!(file.bounds, bytes.bounds);
        assert_eq!(file.props, bytes.props);
        assert!(bytes.layers.iter().all(|l| l.colors.as_ref().is_some_and(|c| c.len() == l.vertex_count())));
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn default_desktop_registers_revit_in_the_open_dialog() {
        let extensions = supported_extensions();
        assert!(extensions.contains(&"rvt"));
        assert!(extensions.contains(&"rfa"));
        let source = Source::Bytes { name: "test.RVT".into(), data: Arc::new(Vec::new()) };
        let error = load_with(&source, &Progress::default()).unwrap_err();
        assert!(error.contains("Revit files open in the desktop app only"), "{error}");
    }

    #[test]
    fn rejects_unknown_extension() {
        assert!(load(Path::new("x.unknownext")).is_err());
    }
}
