//! Shared scene model and loader contract for OpenAEC format crates.
pub mod model;
pub mod section;
pub use model::*;
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
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

/// Loads a source through an explicit reader, without a global registry.
pub fn load_with_loader(loader: &dyn Loader, source: &Source, progress: &Progress) -> Result<Model, String> {
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
