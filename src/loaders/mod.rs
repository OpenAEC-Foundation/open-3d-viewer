//! File format loaders. Each loader turns a [`Source`] (a file, or the bytes of a file picked in
//! the browser) into [`Layer`]s in Z-up coordinates; [`load`] picks the loader by extension and
//! wraps the result in a [`Model`].
//!
//! To add a format (DWG, IFC, ...): implement [`Loader`] in a new module and add it to
//! [`registry`]. Additional loaders that live outside this crate are added with
//! [`add`] before the app starts.

mod gltf;
mod obj;
mod stl;
#[cfg(not(target_arch = "wasm32"))]
mod rvt;
#[cfg(not(target_arch = "wasm32"))]
mod rvt_drawings;
#[cfg(not(target_arch = "wasm32"))]
mod rvt_drafts;
#[cfg(not(target_arch = "wasm32"))]
mod rvt_tags;
#[cfg(not(target_arch = "wasm32"))]
mod rvt_schedules;
#[cfg(not(target_arch = "wasm32"))]
mod rvt_browser;
#[cfg(not(target_arch = "wasm32"))]
mod rvt_images;
#[cfg(not(target_arch = "wasm32"))]
mod rvt_props;
#[cfg(not(target_arch = "wasm32"))]
mod rvt_fonts;
#[cfg(not(target_arch = "wasm32"))]
mod rvt_materials;
#[cfg(not(target_arch = "wasm32"))]
mod rvt_phases;
#[cfg(not(target_arch = "wasm32"))]
mod rvt_joins;
mod ifc;

mod skp;
#[cfg(not(target_arch = "wasm32"))]
mod smc;
mod tekla;
mod ply;
mod cityjson;
mod landxml;
pub(crate) mod cad;
mod allplan;
mod dgn;
mod archicad;
#[cfg(not(target_arch = "wasm32"))]
mod vertex;
mod threemf;
mod collada;
mod fbx;
mod off;
mod max3ds;
mod geojson;
mod citygml;
mod usd;
mod pointcloud;
mod las;
mod e57;
mod xyz;
mod ptx;
mod pcd;
mod ascgrid;
mod shapefile;
mod vrml;
mod amf;
mod ifcx;
mod step;
mod sam;
mod rhino;
mod blend;
mod fcstd;

use crate::model::{Layer, Model, Origin, ProjectData, SavedView};
use crate::structural::StructuralPart;
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use web_time::Instant;

/// A file that the model `model` refers to (a glTF buffer or image, an OBJ material library). Where
/// the AI connection is held to the work folder (`managed::confined`), a model in the work folder
/// may only refer to files in it, so opening it reads nothing outside. A model the user opened
/// elsewhere is the user's own choice and is not limited.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn check_reference(model: &Path, file: &Path) -> Result<(), String> {
    if crate::managed::confined() && crate::managed::confine(model).is_ok() {
        crate::managed::confine(file).map_err(|e| format!("refers to a file outside the work folder: {e}"))?;
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn check_reference(_: &Path, _: &Path) -> Result<(), String> {
    Ok(())
}


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
        self.load(source).map(|layers| Loaded { structural: Vec::new(), origin: None, layers, views: Vec::new(), project: ProjectData::default() })
    }
    /// True when the format is Y up and `load` converted it to Z up.
    fn source_y_up(&self) -> bool {
        false
    }
    /// Lower-case extensions of folders this loader opens as a whole (e.g. an Allplan `.prj`
    /// project folder dropped on the window); not part of the file dialog filter.
    fn folder_extensions(&self) -> &'static [&'static str] {
        &[]
    }
    /// True when files of this format layer over each other into one model (IFCX): a file added
    /// to an open model of this format composes with it, see [`Loader::load_stack`].
    fn composes(&self) -> bool {
        false
    }
    /// Reads several files as one model, the later ones layered over the earlier ones (only
    /// when [`Loader::composes`]; by default the last file alone).
    fn load_stack(&self, sources: &[Source], progress: &Progress) -> Result<Loaded, String> {
        self.load_with(sources.last().ok_or("no file")?, progress)
    }
    /// Where the format stands among those the start screen names (lower first); `None` for the
    /// formats only the file dialog lists. See [`featured_formats`].
    fn featured(&self) -> Option<u8> {
        None
    }
}

/// What a loader read: the layers, the views saved in the file and the structural analysis
/// models.
pub struct Loaded {
    pub layers: Vec<Layer>,
    pub views: Vec<SavedView>,
    pub project: ProjectData,
    /// Structural analysis models, in f64 world metres of the file (no anchor subtracted).
    pub structural: Vec<bim_structural::AnalysisModel>,
    /// The anchor the loader subtracted from the layer positions (world metres, Z up), when the
    /// format has one; it places the model when models are combined.
    pub origin: Option<glam::DVec3>,
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
        &cityjson::CityJsonLoader, &landxml::LandXmlLoader, &threemf::ThreeMfLoader, &collada::ColladaLoader,
        &fbx::FbxLoader, &off::OffLoader, &max3ds::Max3dsLoader, &geojson::GeoJsonLoader, &citygml::CityGmlLoader, &usd::UsdLoader,
        &las::LasLoader, &e57::E57Loader, &xyz::PtsLoader, &ptx::PtxLoader, &pcd::PcdLoader,
        &shapefile::ShapefileLoader, &vrml::VrmlLoader, &amf::AmfLoader,
        &ifcx::IfcxLoader, &step::StepLoader, &sam::SamLoader, &rhino::RhinoLoader, &blend::BlendLoader, &fcstd::FcstdLoader,
        &cad::CadLoader, &allplan::AllplanLoader, &dgn::DgnLoader, &archicad::ArchicadLoader,
        #[cfg(not(target_arch = "wasm32"))]
        &rvt::RvtLoader,
        #[cfg(not(target_arch = "wasm32"))]
        &vertex::VertexLoader,
        #[cfg(not(target_arch = "wasm32"))]
        &vertex::BtlLoader,
        #[cfg(not(target_arch = "wasm32"))]
        &smc::SmcLoader,
        &ifc::IfcLoader,
        // last: `.txt` and `.csv` are only point clouds when nothing else claims them
        &xyz::XyzLoader,
    ];
    EXTRA.get().into_iter().flatten().copied().chain(builtin).collect()
}

/// The issues a coordination file brought along when it was loaded, once.
#[cfg(not(target_arch = "wasm32"))]
pub fn take_file_issues() -> Option<(std::path::PathBuf, bim_bcf::issues::Issues)> {
    smc::take_issues()
}

/// All supported extensions, for the file dialog filter.
pub fn supported_extensions() -> Vec<&'static str> {
    registry().iter().flat_map(|l| l.extensions().iter().copied()).collect()
}

/// The formats the start screen names, from the same registry as the file dialog (so a build
/// names only what it opens): the featured format names in order, and how many other formats
/// this build opens.
pub fn featured_formats() -> (Vec<&'static str>, usize) {
    let loaders = registry();
    let mut featured: Vec<(u8, &'static str)> = loaders.iter().filter_map(|l| l.featured().map(|rank| (rank, l.name()))).collect();
    featured.sort();
    featured.dedup_by_key(|(_, name)| *name);
    let featured: Vec<&'static str> = featured.into_iter().map(|(_, name)| name).collect();
    let mut others: Vec<&'static str> = loaders.iter().map(|l| l.name()).filter(|name| !featured.contains(name)).collect();
    others.sort_unstable();
    others.dedup();
    (featured, others.len())
}

/// Where the desktop app is downloaded.
pub const DESKTOP_DOWNLOAD: &str = "https://github.com/OpenAEC-Foundation/open-3d-viewer/releases/latest";

/// Formats the desktop app opens but the browser build cannot (their readers open the file in
/// parts from disk or load a native library): the format name and its extensions.
const DESKTOP_ONLY: &[(&str, &[&str])] = &[("Revit", &["rvt", "rfa"]), ("Vertex BD", &["vxz", "vxm"]), ("BTL", &["btl"]), ("Coordination model (SMC)", &["smc"])];

/// The desktop-only format of files with extension `ext` (lower case) when none of `loaders`
/// (this build's registry) opens them.
fn desktop_only(ext: &str, loaders: &[&'static dyn Loader]) -> Option<&'static str> {
    if loaders.iter().any(|l| l.extensions().contains(&ext)) {
        return None;
    }
    DESKTOP_ONLY.iter().find(|(_, extensions)| extensions.contains(&ext)).map(|(name, _)| *name)
}

/// The message for a file of a desktop-only format, in Dutch or English: short, so it fits the
/// status bar of a phone; the shell then offers the download ([`DESKTOP_DOWNLOAD`]) as a button.
pub fn desktop_only_message(format: &str, dutch: bool) -> String {
    match dutch {
        true => format!("{format} vereist de desktop-app"),
        false => format!("{format} needs the desktop app"),
    }
}

/// The desktop-only format a load error is about, so the status bar can show the message in the
/// chosen language (load errors are English).
pub fn desktop_only_error(error: &str) -> Option<&'static str> {
    DESKTOP_ONLY.iter().map(|(name, _)| *name).find(|name| error == desktop_only_message(name, false))
}

/// The error for a file whose extension none of `loaders` claims: a pointer to the desktop app
/// for the formats only it opens, otherwise unsupported.
fn unsupported(ext: &str, loaders: &[&'static dyn Loader]) -> String {
    match desktop_only(ext, loaders) {
        Some(format) => desktop_only_message(format, false),
        None => format!("Unsupported file type: .{ext}"),
    }
}

/// Loads a file with the loader matching its extension.
pub fn load(path: &Path) -> Result<Model, String> {
    load_with(&Source::from(path), &Progress::default())
}

/// As [`load`], for any source, reporting the steps to `progress`.
pub fn load_with(source: &Source, progress: &Progress) -> Result<Model, String> {
    load_stack(std::slice::from_ref(source), progress)
}

/// The loader for a file (or a project folder).
fn loader_for(source: &Source) -> Result<&'static dyn Loader, String> {
    let ext = source.extension();
    // A folder goes to the loader that opens folders with its extension.
    let folder = source.path().is_some_and(Path::is_dir);
    let loaders = registry();
    loaders
        .iter()
        .copied()
        .find(|l| if folder { l.folder_extensions() } else { l.extensions() }.contains(&ext.as_str()))
        .ok_or_else(|| match folder {
            true => format!("{}: folders open only as a project folder of a supported format", source.name()),
            false => unsupported(&ext, &loaders),
        })
}

/// True when `source` is of a format whose files layer over each other (IFCX).
pub fn composes(source: &Source) -> bool {
    loader_for(source).is_ok_and(|l| l.composes())
}

/// Loads files that layer over each other as one model, the later ones stronger (see
/// [`Loader::load_stack`]); one file loads as [`load_with`]. The model is named after the first.
pub fn load_stack(sources: &[Source], progress: &Progress) -> Result<Model, String> {
    let source = sources.first().ok_or("no file to open")?;
    let loader = loader_for(sources.last().unwrap_or(source))?;
    let start = Instant::now();
    let loaded = match sources {
        [one] => loader.load_with(one, progress)?,
        _ => loader.load_stack(sources, progress)?,
    };
    progress.set(Phase::Scene);
    let mut model = Model::new(source.model_path(), loader.name(), loaded.layers);
    model.file_size = sources.iter().map(Source::len).sum();
    model.views = loaded.views;
    model.project = loaded.project;
    // one origin per analysis model (the first shared with the layers), all at the file's anchor:
    // the origin index is the `model` of a structural object's key, so two analysis models of one
    // file that number their objects alike keep apart
    let origin = Origin { anchor: loaded.origin, shift: glam::DVec3::ZERO };
    model.origins = vec![origin; loaded.structural.len().max(1)];
    if !loaded.structural.is_empty() {
        let parts = loaded.structural.into_iter().enumerate().map(|(k, m)| StructuralPart::new(k as u32, Arc::new(m))).collect();
        model.set_structural(parts);
    }
    model.load_time = start.elapsed();
    model.source_y_up = loader.source_y_up();
    // a file of only analysis objects (nodes and members, or only sections, materials or load
    // cases, no triangles) opens too, as does one of only modelled elements (their meshes are
    // made again from their definitions)
    if model.layers.is_empty() && model.structural_index.is_empty() && model.project.elements.is_empty() {
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
        for (extension, name) in [("skp", "SketchUp"), ("db1", "Tekla Structures"), ("ply", "PLY"), ("ndw", "Allplan"), ("dgn", "MicroStation DGN"), ("pln", "Archicad"), ("pla", "Archicad"), ("3mf", "3MF"), ("dae", "COLLADA"), ("fbx", "FBX"), ("off", "OFF"), ("3ds", "3DS"), ("geojson", "GeoJSON"), ("gml", "CityGML"), ("citygml", "CityGML"), ("usdz", "USD"), ("las", "LAS"), ("laz", "LAS"), ("e57", "E57"), ("xyz", "XYZ"), ("txt", "XYZ"), ("csv", "XYZ"), ("asc", "XYZ"), ("pts", "PTS"), ("ptx", "PTX"), ("pcd", "PCD"), ("shp", "Shapefile"), ("wrl", "VRML / X3D"), ("x3d", "VRML / X3D"), ("x3dv", "VRML / X3D"), ("amf", "AMF"), ("ifcx", "IFCX"), ("stp", "STEP"), ("step", "STEP"), ("stpz", "STEP"), ("3dm", "Rhino")] {
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
        let error = load(Path::new("x.unknownext")).unwrap_err();
        assert_eq!(error, "Unsupported file type: .unknownext");
        assert_eq!(desktop_only_error(&error), None);
    }

    #[test]
    fn registers_ifc_on_every_target() {
        // the browser build opens IFC too (issue #92): the file dialog and the start screen offer it
        let loader = registry().into_iter().find(|l| l.extensions().contains(&"ifc")).expect("IFC loader");
        assert_eq!(loader.name(), "IFC");
        assert!(loader.extensions().contains(&"ifczip"));
        assert!(featured_formats().0.contains(&"IFC"));
        assert_eq!(desktop_only("ifc", &registry()), None);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn desktop_only_formats_are_the_desktop_loaders() {
        // each format the browser points to the desktop app for is one the desktop opens
        let desktop = registry();
        for (name, extensions) in DESKTOP_ONLY {
            for extension in *extensions {
                let loader = desktop.iter().find(|l| l.extensions().contains(extension)).expect(extension);
                assert_eq!(loader.name(), *name);
                assert_eq!(desktop_only(extension, &desktop), None, "{extension}");
            }
        }
        assert!(featured_formats().0.contains(&"Revit"));
    }

    #[test]
    fn points_desktop_only_formats_to_the_desktop_app() {
        // as in the browser build: the registry without the desktop-only loaders
        let web: Vec<&'static dyn Loader> =
            registry().into_iter().filter(|l| !DESKTOP_ONLY.iter().any(|(name, _)| *name == l.name())).collect();
        for (extension, format) in [("rvt", "Revit"), ("rfa", "Revit"), ("vxz", "Vertex BD"), ("vxm", "Vertex BD"), ("btl", "BTL")] {
            let error = unsupported(extension, &web);
            assert_eq!(error, format!("{format} needs the desktop app"));
            assert_eq!(desktop_only_error(&error), Some(format));
            assert!(desktop_only_message(format, true).contains("vereist de desktop-app"));
        }
        assert_eq!(unsupported("abc", &web), "Unsupported file type: .abc");
    }

    #[test]
    #[cfg(target_arch = "wasm32")]
    fn browser_points_revit_to_the_desktop_app() {
        let source = Source::Bytes { name: "house.RVT".into(), data: Arc::new(Vec::new()) };
        let error = load_with(&source, &Progress::default()).unwrap_err();
        assert_eq!(desktop_only_error(&error), Some("Revit"));
        assert!(!featured_formats().0.contains(&"Revit"));
    }

    #[test]
    fn featured_formats_come_from_the_registry_in_order() {
        let (featured, others) = featured_formats();
        let names: Vec<&str> = registry().iter().map(|l| l.name()).collect();
        assert!(featured.iter().all(|name| names.contains(name)));
        assert_eq!(&featured[..2], ["IFC", if cfg!(target_arch = "wasm32") { "SketchUp" } else { "Revit" }]);
        assert!(featured.ends_with(&["SketchUp", "glTF", "OBJ", "STL", "PLY"]), "{featured:?}");
        assert!(others > 20, "{others}");
    }
}
