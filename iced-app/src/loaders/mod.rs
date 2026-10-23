//! File format loaders. Each loader turns a file into [`Layer`]s in Z-up coordinates;
//! [`load`] picks the loader by extension and wraps the result in a [`Model`].
//!
//! To add a format (DWG, E57, IFC, ...): implement [`Loader`] in a new module and add it to
//! [`registry`].

mod gltf;
mod obj;
mod ply;
#[cfg(feature = "rvt")]
mod rvt;
mod stl;

use crate::model::{Layer, Model};
use std::path::Path;
use std::time::Instant;

/// A file format reader.
pub trait Loader: Send + Sync {
    /// Human-readable format name, e.g. "glTF".
    fn name(&self) -> &'static str;
    /// Lower-case file extensions without the dot.
    fn extensions(&self) -> &'static [&'static str];
    /// Reads the file into layers (Z up).
    fn load(&self, path: &Path) -> Result<Vec<Layer>, String>;
    /// True when the format is Y up and `load` converted it to Z up.
    fn source_y_up(&self) -> bool {
        false
    }
}

/// All available loaders.
pub fn registry() -> Vec<Box<dyn Loader>> {
    vec![
        #[cfg(feature = "rvt")]
        Box::new(rvt::RvtLoader),
        Box::new(gltf::GltfLoader),
        Box::new(obj::ObjLoader),
        Box::new(stl::StlLoader),
        Box::new(ply::PlyLoader),
    ]
}

/// All supported extensions, for the file dialog filter.
pub fn supported_extensions() -> Vec<&'static str> {
    registry().iter().flat_map(|l| l.extensions().iter().copied()).collect()
}

/// Loads a file with the loader matching its extension.
pub fn load(path: &Path) -> Result<Model, String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let loader = registry()
        .into_iter()
        .find(|l| l.extensions().contains(&ext.as_str()))
        .ok_or_else(|| format!("Unsupported file type: .{ext}"))?;
    let start = Instant::now();
    let layers = loader.load(path)?;
    let mut model = Model::new(path.to_path_buf(), loader.name(), layers);
    model.load_time = start.elapsed();
    model.source_y_up = loader.source_y_up();
    if model.layers.is_empty() {
        return Err(format!("{}: no geometry found", model.file_name()));
    }
    Ok(model)
}

/// Default grey for meshes without a material.
pub const DEFAULT_COLOR: [f32; 3] = [0.72, 0.72, 0.74];

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn repo_file(rel: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join(rel)
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
    fn rejects_unknown_extension() {
        assert!(load(Path::new("x.unknownext")).is_err());
    }
}
