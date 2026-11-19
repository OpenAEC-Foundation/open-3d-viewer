//! The project file: the set-up of a session, so it opens again as it was left.
//!
//! A project is the files that are open together (models of several parties, analysis models,
//! point clouds), where each was moved to, which are shown, and how the 3D view looks at them.
//! The file names its sources and copies none: each stays where it is, and a newer one put in
//! its place is what opens next time. A source is named by its path from the project file where
//! it lies on the same drive, so a project folder can be moved or shared as a whole.
//!
//! It is an IFCX file with one node that holds the project (`openaec::project`), written first
//! so the start of the file says what it is.

use super::{App, Message, Status};
#[cfg(not(target_arch = "wasm32"))]
use crate::loaders;
use glam::{DVec3, Vec3};
use iced::Task;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// The attribute of the node that holds the project.
const MARK: &str = "openaec::project";
/// How much of a file is read to see whether it is a project.
const HEAD: usize = 4096;
/// The end of the name a project file is offered with.
pub const SUFFIX: &str = ".project.ifcx";

/// A project as its file holds it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Project {
    pub version: u32,
    /// The files, in the order they were opened.
    pub models: Vec<Source>,
    /// How the 3D view looks at them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<View>,
    /// The section plane: its normal and its place along it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plane: Option<([f32; 3], f32)>,
}

/// One file of a project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    /// Its path, from the project file's folder where it can be, with `/` between the folders.
    pub path: String,
    #[serde(default = "shown")]
    pub visible: bool,
    /// Where it was moved to (metres).
    #[serde(default)]
    pub moved: [f64; 3],
    /// Where its zero point lies in it (metres).
    #[serde(default)]
    pub zero: [f64; 3],
}

fn shown() -> bool {
    true
}

/// The camera of the 3D view.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub target: [f32; 3],
    pub distance: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub fov_y: f32,
    pub ortho: bool,
}

impl Project {
    /// The project as the text of its file.
    pub fn write(&self) -> String {
        let project = serde_json::to_value(self).unwrap_or_default();
        // the project's node first: the start of the file says what it is
        let file = serde_json::json!({
            "data": [{ "path": crate::document::uuid("openaec/project"), "attributes": { MARK: project } }],
            "header": { "id": "", "ifcxVersion": "ifcx_alpha", "dataVersion": "1.0.0", "author": "Open BIM Suite", "timestamp": "" },
            "imports": [],
            "schemas": {},
        });
        serde_json::to_string_pretty(&file).unwrap_or_default()
    }

    /// The project a file's text holds; none for a file that is not a project.
    pub fn read(text: &str) -> Option<Self> {
        let file: serde_json::Value = serde_json::from_str(text).ok()?;
        let node = file.get("data")?.as_array()?.iter().find_map(|node| node.get("attributes")?.get(MARK))?;
        serde_json::from_value(node.clone()).ok()
    }

    /// The project in the file at `path`, when it is one: only the start of the file is read to
    /// tell, so a large model is not read twice.
    pub fn peek(path: &Path) -> Option<Self> {
        use std::io::Read;
        if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ifcx")) {
            return None;
        }
        let mut head = vec![0u8; HEAD];
        let n = std::fs::File::open(path).ok()?.read(&mut head).ok()?;
        if !String::from_utf8_lossy(&head[..n]).contains(MARK) {
            return None;
        }
        Self::read(&std::fs::read_to_string(path).ok()?)
    }
}

/// `path` as a project file in `folder` names it: from that folder where both lie on one drive,
/// else in full; with `/` between the folders.
pub fn named(folder: &Path, path: &Path) -> String {
    let parts = |p: &Path| p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>();
    let (from, to) = (parts(folder), parts(path));
    let same_root = path.is_absolute() && folder.is_absolute() && from.first().map(|s| s.to_lowercase()) == to.first().map(|s| s.to_lowercase());
    if !same_root {
        return path.to_string_lossy().replace('\\', "/");
    }
    let common = from.iter().zip(&to).take_while(|(a, b)| a.eq_ignore_ascii_case(b)).count();
    let up = std::iter::repeat("..".to_string()).take(from.len() - common);
    up.chain(to[common..].iter().cloned()).collect::<Vec<_>>().join("/")
}

/// The file a project in `folder` names as `name`.
pub fn resolved(folder: &Path, name: &str) -> PathBuf {
    let named = PathBuf::from(name);
    if named.is_absolute() {
        return named;
    }
    let mut out = folder.to_path_buf();
    for part in named.components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(p) => out.push(p),
            _ => {}
        }
    }
    out
}

/// What is left of a project that is being opened: applied when its last file is in.
#[derive(Debug, Clone)]
pub struct Opening {
    pub project: Project,
    pub folder: PathBuf,
    /// The files it names that are not there.
    pub missing: Vec<String>,
}

impl App {
    /// The set-up of the session as a project whose file lies in `folder`. A model that came
    /// from no file (modelled here, or handed over as bytes) cannot be named and is left out.
    pub fn project_now(&self, folder: &Path) -> Project {
        let models = self
            .models
            .iter()
            .filter(|entry| entry.model.path.is_absolute())
            .map(|entry| Source { path: named(folder, &entry.model.path), visible: entry.visible, moved: entry.placed.moved.to_array(), zero: entry.placed.zero.to_array() })
            .collect();
        let c = &self.camera;
        Project {
            version: 1,
            models,
            camera: Some(View { target: c.target.to_array(), distance: c.distance, yaw: c.yaw, pitch: c.pitch, fov_y: c.fov_y, ortho: c.ortho }),
            plane: self.plane.map(|p| (p.normal.to_array(), p.offset)),
        }
    }

    /// Writes the project file at `path`.
    pub(super) fn save_project(&mut self, mut path: PathBuf) {
        if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ifcx")) {
            path.set_extension("ifcx");
        }
        let folder = path.parent().map(PathBuf::from).unwrap_or_default();
        let project = self.project_now(&folder);
        let left_out = self.models.len().saturating_sub(project.models.len());
        let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.status = match std::fs::write(&path, project.write()) {
            Ok(()) if left_out > 0 => Status::Loaded(format!("Saved project {file} with {} files; {left_out} without a file of their own are not in it", project.models.len())),
            Ok(()) => Status::Loaded(format!("Saved project {file} with {} files", project.models.len())),
            Err(e) => Status::Error(format!("{file}: {e}")),
        };
    }

    /// Opens the files of a project one after another; the set-up is put back when the last is in.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn open_project(&mut self, file: &Path, project: Project) -> Task<Message> {
        let (folder, there, missing) = Self::files(file, &project);
        let mut sources = there.into_iter().map(loaders::Source::File);
        let Some(first) = sources.next() else {
            self.status = Status::Error(match missing.as_slice() {
                [] => "The project names no files".to_string(),
                missing => format!("None of the project's files is there: {}", missing.join(", ")),
            });
            return Task::none();
        };
        self.add_queue = sources.collect();
        self.project_opening = Some(Opening { project, folder, missing });
        self.open(first, false)
    }

    /// The files of a project at `file` that are there, and those that are not.
    #[cfg(not(target_arch = "wasm32"))]
    fn files(file: &Path, project: &Project) -> (PathBuf, Vec<PathBuf>, Vec<String>) {
        let folder = file.parent().map(PathBuf::from).unwrap_or_default();
        let (there, missing): (Vec<PathBuf>, Vec<PathBuf>) = project.models.iter().map(|s| resolved(&folder, &s.path)).partition(|p| p.exists());
        (folder, there, missing.iter().map(|p| p.to_string_lossy().into_owned()).collect())
    }

    /// Opens a project here and now, without messages (the command line's screenshot): its
    /// files one after another, then the set-up.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_project_now(&mut self, file: &Path, project: Project) -> Result<(), String> {
        let (folder, there, missing) = Self::files(file, &project);
        if there.is_empty() {
            return Err(format!("none of the project's files is there: {}", missing.join(", ")));
        }
        for (k, path) in there.iter().enumerate() {
            let model = std::sync::Arc::new(loaders::load(path)?);
            if k == 0 {
                self.set_model(model);
            } else {
                self.add_model(model);
            }
        }
        for name in &missing {
            eprintln!("not there: {name}");
        }
        self.project_opening = Some(Opening { project, folder, missing: Vec::new() });
        self.settle_project();
        Ok(())
    }

    /// Puts the set-up of the project that was being opened back, now its files are in: each
    /// model where it was moved to and shown or not, the camera and the section plane.
    pub(super) fn settle_project(&mut self) {
        let Some(Opening { project, folder, missing }) = self.project_opening.take() else { return };
        let mut moved = false;
        for source in &project.models {
            let path = resolved(&folder, &source.path);
            let Some(entry) = self.models.iter_mut().find(|entry| entry.model.path == path) else { continue };
            entry.visible = source.visible;
            entry.placed.moved = DVec3::from_array(source.moved);
            entry.placed.zero = DVec3::from_array(source.zero);
            moved |= entry.placed.moved != DVec3::ZERO;
        }
        if moved {
            self.replace_models();
        } else {
            self.restore_model_visibility();
        }
        self.picked_model = None;
        if let Some(v) = project.camera {
            self.camera.target = Vec3::from_array(v.target);
            self.camera.distance = v.distance;
            self.camera.yaw = v.yaw;
            self.camera.pitch = v.pitch;
            self.camera.fov_y = v.fov_y;
            self.camera.ortho = v.ortho;
        }
        if let Some((normal, offset)) = project.plane {
            self.plane = Some(crate::section::SectionPlane { normal: Vec3::from_array(normal), offset });
        }
        if !missing.is_empty() {
            self.status = Status::Error(format!("The project opened without {} of its files: {}", missing.len(), missing.join(", ")));
        }
    }
}

/// Asks where to save the project, beside its first file.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn save_dialog_task(app: &App) -> Task<Message> {
    let first = app.models.iter().map(|entry| entry.model.path.clone()).find(|p| p.is_absolute());
    let name = first.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "project".into());
    let mut dialog = rfd::AsyncFileDialog::new().set_title("Save project").set_file_name(format!("{name}{SUFFIX}")).add_filter("Project (IFCX)", &["ifcx"]);
    if let Some(dir) = crate::managed::dialog_folder(first.as_ref().and_then(|p| p.parent()).map(PathBuf::from), crate::managed::work_folder()) {
        dialog = dialog.set_directory(dir);
    }
    Task::perform(async move { dialog.save_file().await.map(|f| f.path().to_path_buf()) }, Message::SaveProjectPicked)
}

/// The browser keeps no paths of the files it was given: there is no project to name them in.
#[cfg(target_arch = "wasm32")]
pub(super) fn save_dialog_task(_app: &App) -> Task<Message> {
    Task::none()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> Project {
        Project {
            version: 1,
            models: vec![
                Source { path: "models/architect.ifc".into(), visible: true, moved: [0.0; 3], zero: [0.0; 3] },
                Source { path: "../structure/frame.rvt".into(), visible: false, moved: [12.5, -3.0, 0.25], zero: [1.0, 2.0, 3.0] },
            ],
            camera: Some(View { target: [1.0, 2.0, 3.0], distance: 40.0, yaw: 0.5, pitch: 0.3, fov_y: 0.8, ortho: true }),
            plane: Some(([0.0, 0.0, 1.0], 2.5)),
        }
    }

    #[test]
    fn a_project_reads_back_as_it_was_written_and_says_at_its_start_what_it_is() {
        let text = project().write();
        assert_eq!(Project::read(&text), Some(project()));
        assert!(text[..HEAD.min(text.len())].contains(MARK), "the mark lies in the part that is read to tell");
        // an IFCX file like any other, and a model's file is no project
        let file: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(file["header"]["ifcxVersion"], "ifcx_alpha");
        assert_eq!(Project::read(r#"{"header":{},"data":[{"path":"a","attributes":{"bsi::ifc::class":{"code":"IfcWall"}}}]}"#), None);
        // a file written before a field was there still reads
        let old = r#"{"data":[{"path":"p","attributes":{"openaec::project":{"version":1,"models":[{"path":"a.ifc"}]}}}]}"#;
        let read = Project::read(old).expect("a project");
        assert_eq!(read.models, vec![Source { path: "a.ifc".into(), visible: true, moved: [0.0; 3], zero: [0.0; 3] }]);
        assert_eq!((read.camera, read.plane), (None, None));
    }

    /// A model of one square, from a file at `path`.
    fn square(path: &Path) -> std::sync::Arc<crate::model::Model> {
        let layer = crate::model::Layer {
            name: "Walls".into(),
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0],
            indices: vec![0, 1, 2, 0, 2, 3],
            elements: vec![(10, 0, 6)],
            ..Default::default()
        };
        std::sync::Arc::new(crate::model::Model::new(path.to_path_buf(), "IFC", vec![layer]))
    }

    #[test]
    fn a_session_saved_as_a_project_is_set_up_again_as_it_was() {
        let folder = std::env::temp_dir().join(format!("o3v-project-{}", std::process::id()));
        let (a, b) = (folder.join("models").join("architect.ifc"), folder.join("structure.ifc"));
        // the session: two models, the second moved, its zero point set, and hidden
        let mut app = App::default();
        app.set_model(square(&a));
        app.add_model(square(&b));
        app.models[1].placed.moved = DVec3::new(12.5, -3.0, 0.25);
        app.models[1].placed.zero = DVec3::new(1.0, 2.0, 3.0);
        app.replace_models();
        app.models[1].visible = false;
        app.restore_model_visibility();
        app.camera.yaw = 0.75;
        app.camera.distance = 42.0;
        app.plane = Some(crate::section::SectionPlane { normal: Vec3::Z, offset: 2.5 });
        let project = app.project_now(&folder);
        assert_eq!(project.models.iter().map(|s| s.path.as_str()).collect::<Vec<_>>(), ["models/architect.ifc", "structure.ifc"]);

        // opened again: the files load one after another, then the set-up is put back
        let project = Project::read(&project.write()).expect("the project reads back");
        let mut again = App::default();
        again.project_opening = Some(Opening { project, folder: folder.clone(), missing: Vec::new() });
        again.set_model(square(&a));
        again.add_model(square(&b));
        again.settle_project();
        assert!(again.project_opening.is_none());
        assert_eq!((again.models[0].visible, again.models[1].visible), (true, false));
        assert_eq!(again.models[1].placed.moved, DVec3::new(12.5, -3.0, 0.25));
        assert_eq!(again.models[1].placed.zero, DVec3::new(1.0, 2.0, 3.0));
        assert_eq!(again.models[0].placed.moved, DVec3::ZERO);
        // the moved model lies where it was moved to, and its layer is switched off
        let scene = again.model.as_ref().expect("a scene");
        let second = &scene.layers[again.models[1].layers.start];
        assert!((second.positions[0] - 12.5).abs() < 1e-4, "{:?}", &second.positions[..3]);
        assert!(again.visible[again.models[0].layers.start] && !again.visible[again.models[1].layers.start]);
        assert_eq!((again.camera.yaw, again.camera.distance), (0.75, 42.0));
        assert_eq!(again.plane.map(|p| p.offset), Some(2.5));
    }

    #[test]
    fn a_source_is_named_from_the_projects_folder_where_it_can_be() {
        let folder = Path::new(if cfg!(windows) { r"C:\work\2321\project" } else { "/work/2321/project" });
        let at = |rest: &str| if cfg!(windows) { PathBuf::from(format!(r"C:\work\2321\{}", rest.replace('/', r"\"))) } else { PathBuf::from(format!("/work/2321/{rest}")) };
        assert_eq!(named(folder, &at("project/models/a.ifc")), "models/a.ifc");
        assert_eq!(named(folder, &at("structure/frame.rvt")), "../structure/frame.rvt");
        // and back
        assert_eq!(resolved(folder, "models/a.ifc"), at("project/models/a.ifc"));
        assert_eq!(resolved(folder, "../structure/frame.rvt"), at("structure/frame.rvt"));
        // another drive: in full
        #[cfg(windows)]
        {
            assert_eq!(named(folder, Path::new(r"D:\scans\site.e57")), "D:/scans/site.e57");
            assert_eq!(resolved(folder, "D:/scans/site.e57"), PathBuf::from("D:/scans/site.e57"));
        }
    }
}
