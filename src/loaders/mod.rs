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
#[cfg(not(target_arch = "wasm32"))]
mod ifc;

mod skp;
mod tekla;
mod ply;
mod cityjson;
mod landxml;
mod cad;

use crate::model::{Layer, Model, ProjectData, SavedView};
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use web_time::Instant;


/// Where a model comes from: a file on disk (desktop, command line, remote control) or the bytes
/// of a file the browser handed over (web).
#[derive(Clone)]
pub enum Source {
    File(PathBuf),
    Bytes { name: String, data: Arc<Vec<u8>> },
}

impl Source {
    /// The file name, e.g. `house.skp`.
    pub fn name(&self) -> String {
        match self {
            Source::File(path) => path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            Source::Bytes { name, .. } => name.clone(),
        }
    }

    /// The file name without its extension.
    pub fn stem(&self) -> String {
        let name = self.name();
        Path::new(&name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(name)
    }

    /// The lower-case extension without the dot.
    pub fn extension(&self) -> String {
        Path::new(&self.name()).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
    }

    /// The file on disk, when there is one (needed for side files, such as the buffers of a
    /// `.gltf` or the `.mtl` of an `.obj`, and by readers that open the file themselves).
    pub fn path(&self) -> Option<&Path> {
        match self {
            Source::File(path) => Some(path),
            Source::Bytes { .. } => None,
        }
    }

    /// The whole file.
    pub fn read(&self) -> Result<Cow<'_, [u8]>, String> {
        match self {
            Source::File(path) => std::fs::read(path).map(Cow::Owned).map_err(|e| format!("Cannot read {}: {e}", self.name())),
            Source::Bytes { data, .. } => Ok(Cow::Borrowed(data.as_slice())),
        }
    }

    /// The size of the file in bytes.
    pub fn len(&self) -> u64 {
        match self {
            Source::File(path) => std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
            Source::Bytes { data, .. } => data.len() as u64,
        }
    }

    /// The path the model keeps: the file, or just its name for bytes.
    pub fn model_path(&self) -> PathBuf {
        match self {
            Source::File(path) => path.clone(),
            Source::Bytes { name, .. } => PathBuf::from(name),
        }
    }
}

impl From<PathBuf> for Source {
    fn from(path: PathBuf) -> Self {
        Source::File(path)
    }
}

impl From<&Path> for Source {
    fn from(path: &Path) -> Self {
        Source::File(path.to_path_buf())
    }
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::File(path) => write!(f, "File({})", path.display()),
            Source::Bytes { name, data } => write!(f, "Bytes({name}, {} bytes)", data.len()),
        }
    }
}

/// The steps of a load, shown on the loading card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Read = 0,
    Convert = 1,
    Scene = 2,
}

/// Progress of one load, shared by the loader thread and the UI.
#[derive(Debug)]
pub struct Progress {
    state: Mutex<(Phase, Instant)>,
}

impl Default for Progress {
    fn default() -> Self {
        Self { state: Mutex::new((Phase::Read, Instant::now())) }
    }
}

impl Progress {
    pub fn set(&self, phase: Phase) {
        if let Ok(mut s) = self.state.lock() {
            *s = (phase, Instant::now());
        }
    }

    /// The current step and how long it has been running.
    pub fn get(&self) -> (Phase, Duration) {
        self.state.lock().map(|s| (s.0, s.1.elapsed())).unwrap_or((Phase::Read, Duration::ZERO))
    }
}

/// A file format reader.
pub trait Loader: Send + Sync {
    /// Human-readable format name, e.g. "glTF".
    fn name(&self) -> &'static str;
    /// Lower-case file extensions without the dot.
    fn extensions(&self) -> &'static [&'static str];
    /// Reads the file into layers (Z up).
    fn load(&self, source: &Source) -> Result<Vec<Layer>, String>;
    /// As [`Loader::load`], reporting the read and convert steps (default: one read step), plus
    /// what else the file holds.
    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        self.load(source).map(|layers| Loaded { layers, views: Vec::new(), project: ProjectData::default() })
    }
    /// True when the format is Y up and `load` converted it to Z up.
    fn source_y_up(&self) -> bool {
        false
    }
}

/// What a loader read: the layers and the views saved in the file.
pub struct Loaded {
    pub layers: Vec<Layer>,
    pub views: Vec<SavedView>,
    pub project: ProjectData,
}

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
        &cityjson::CityJsonLoader, &landxml::LandXmlLoader,
        &cad::CadLoader,
        #[cfg(not(target_arch = "wasm32"))]
        &rvt::RvtLoader,
        #[cfg(not(target_arch = "wasm32"))]
        &ifc::IfcLoader,
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
    let start = Instant::now();
    let loaded = loader.load_with(source, progress)?;
    progress.set(Phase::Scene);
    let mut model = Model::new(source.model_path(), loader.name(), loaded.layers);
    model.file_size = source.len();
    model.views = loaded.views;
    model.project = loaded.project;
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
    use std::sync::Arc;

    fn repo_file(rel: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
    }

    #[test]
    fn imports_cityjson_landxml_and_cad_through_file_registry() {
        let city = load(&repo_file("test-models/cityjson-building.city.json")).expect("CityJSON fixture");
        assert_eq!(city.format, "CityJSON");
        assert_eq!(city.triangle_count(), 12);
        assert_eq!(city.bounds.size(), [10., 8., 6.]);
        assert!(city.props.values().any(|value| value.contains("demo-building")));
        let terrain = load(&repo_file("test-models/landxml-terrain.xml")).expect("LandXML fixture");
        assert_eq!(terrain.format, "LandXML");
        assert_eq!(terrain.triangle_count(), 2);
        let cad = load(&repo_file("test-models/cad-faces.dxf")).expect("independent DXF fixture");
        assert_eq!(cad.triangle_count(), 2);
        assert_eq!(cad.bounds.size(), [1., 1., 0.]);
        for extension in ["dwg", "dxf", "json", "cityjson", "jsonl", "landxml", "xml"] {
            assert!(supported_extensions().contains(&extension), "{extension}");
        }
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
    fn loads_ply_bytes_into_the_app_model() {
        let data = b"ply\nformat ascii 1.0\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0 255 0 0\n1 0 0 0 255 0\n1 1 0 0 0 255\n0 1 0 255 255 255\n4 0 1 2 3\n";
        let source = Source::Bytes { name: "scan.PLY".into(), data: Arc::new(data.to_vec()) };
        let model = load_with(&source, &Progress::default()).expect("PLY app adapter");
        assert_eq!(model.format, "PLY");
        assert_eq!(model.triangle_count(), 2);
        assert_eq!(model.bounds.min, [0.0; 3]);
        assert_eq!(model.bounds.max, [1.0, 1.0, 0.0]);
        let layer = &model.layers[0];
        assert_eq!(layer.name, "scan");
        assert_eq!(layer.normals, [0.0, 0.0, 1.0].repeat(4));
        assert_eq!(layer.colors.as_ref().unwrap()[0], [255, 0, 0, 255]);
        assert_eq!(layer.colors.as_ref().unwrap()[2], [0, 0, 255, 255]);
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
