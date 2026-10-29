//! Application state, messages and the main window layout:
//! ribbon on top, layers left, 3D viewport centre, properties right, status bar bottom.

use crate::explode::{ExplodeIndex, ExplodeMode};
use crate::export;
use crate::icons;
use crate::loaders;
use crate::measure::format::{AreaUnit, Category, LengthUnit, VolumeUnit, MAX_DECIMALS};
use crate::measure::{MeasureState, Tool};
use crate::model::Model;
use crate::theme::{srgb3, srgb4, Palette, ThemeKind};
use crate::viewport::camera::{Camera, ViewPreset};
use crate::viewport::render::{transform_bounds, RenderMode, RenderSettings};
use crate::model::{Bounds, ViewKind};
use crate::section::{PlaneMove, PlaneOverlay, SectionBox, SectionEvent, SectionOverlay, SectionPlane};
use crate::view_cube::{view_from, CubeEvent, CubeOverlay, CubeTarget};
use crate::viewport::{Navigate, Viewport3D};
use glam::{Mat4, Vec3};
use iced::widget::text::Wrapping;
use iced::{Background, Border, Color, Padding, Shadow};
use openaec_ui::Language;
use iced::alignment::Vertical;
use iced::widget::{
    button, canvas, center, column, container, row, rule, scrollable, shader, slider, stack, svg, text, text_input, Space,
};
use iced::{event, keyboard, window, Center, Element, Fill, Font, Length, Subscription, Task, Theme};
use std::path::PathBuf;
use std::sync::Arc;
use web_time::Instant;

pub const FONT: Font = Font::with_name("Inter");
pub const FONT_BOLD: Font = Font { weight: iced::font::Weight::Semibold, ..FONT };

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RibbonTab {
    #[default]
    Home,
    View,
}

#[derive(Debug, Clone)]
pub enum Status {
    Ready,
    Loading { file: String, since: Instant, progress: Arc<loaders::Progress> },
    Saving { file: String, since: Instant },
    Loaded(String),
    Error(String),
}

pub struct App {
    pub theme: ThemeKind,
    pub model: Option<Arc<Model>>,
    pub visible: Vec<bool>,
    pub camera: Camera,
    pub mode: RenderMode,
    pub grid: bool,
    pub flip_up: bool,
    pub tab: RibbonTab,
    pub show_layers: bool,
    pub show_properties: bool,
    pub layer_filter: String,
    pub status: Status,
    pub viewport_size: (f32, f32),
    /// View preset to apply when the next model arrives (command line).
    pub initial_view: Option<ViewPreset>,
    /// `--window-screenshot`: capture the real window once the model is shown, then exit.
    pub window_shot: Option<PathBuf>,
    /// A file to open once the window runs (command line; the OpenAEC runtime has no boot task).
    pub pending_open: Option<PathBuf>,
    /// The picked layer and, for BIM formats, the element in it.
    pub selected: Option<(usize, Option<u64>)>,
    /// Index ranges of the selection, for the highlight: (layer, first index, index count).
    pub selection: Vec<(usize, u32, u32)>,
    /// A running camera transition: from, to, start.
    pub anim: Option<(Camera, Camera, Instant)>,
    /// The section box (world); kept while switched off, so switching on restores it.
    pub section: Option<SectionBox>,
    pub section_on: bool,
    /// Fill the cut faces of solids.
    pub section_fill: bool,
    /// The section plane (world), when placed.
    pub plane: Option<SectionPlane>,
    /// The section plane as it was placed, before moves and flips: what Reset brings back.
    pub plane_placed: Option<SectionPlane>,
    /// Waiting for a click on a face to place the section plane.
    pub plane_picking: bool,
    /// The saved view last shown.
    pub active_view: Option<usize>,
    /// Show the model's grid lines with their bubbles.
    pub show_grids: bool,
    /// Explorer groups opened: views, rooms, layers.
    pub explorer_open: [bool; 3],
    /// Measurements shown in the viewport (made through remote control).
    pub measurements: Vec<crate::annotations::Measurement>,
    /// Where the viewport sits in the window (logical pixels), to cut it out of screenshots.
    pub viewport_origin: (f32, f32),
    /// Remote control (the MCP server): listening, and the calls waiting for an answer.
    pub remote: crate::remote::RemoteState,
    /// The exploded view: on or off (the amounts are kept while off, as the section box), the
    /// mode asked for and the amount per axis (percent).
    pub explode_on: bool,
    pub explode_mode: ExplodeMode,
    pub explode: [f32; 3],
    /// The units of the model for the exploded view, built when it is loaded.
    pub explode_index: Option<Arc<ExplodeIndex>>,
    /// The offset (world) of every unit as drawn, and the world bounds while apart; shared with
    /// the renderer, replaced when they change.
    pub explode_offsets: Arc<Vec<[f32; 4]>>,
    pub explode_bounds: Option<Bounds>,
    /// The measure tools and their results.
    pub measure: MeasureState,
}

#[derive(Debug, Clone)]
pub enum Message {
    OpenDialog,
    Open(PathBuf),
    Picked(Option<loaders::Source>),
    Loaded(Result<Arc<Model>, String>),
    SaveDialog,
    SavePicked(Option<PathBuf>),
    /// A save finished: the file written and how long it took, or the error.
    Saved(Result<(PathBuf, f32), String>),
    Mode(RenderMode),
    ToggleGrid,
    ToggleFlipUp,
    Theme(ThemeKind),
    Tab(RibbonTab),
    Preset(ViewPreset),
    Ortho(bool),
    Fit,
    ToggleLayersPanel,
    TogglePropertiesPanel,
    Layer(usize, bool),
    AllLayers(bool),
    LayerFilter(String),
    Navigate(Navigate),
    Cube(CubeEvent),
    /// Show a view saved in the file.
    ApplyView(usize),
    ToggleGrids,
    /// Open or close an explorer group (0 views, 1 rooms, 2 layers).
    ToggleExplorer(usize),
    ToggleSection,
    ResetSection,
    SectionToSelection,
    ToggleSectionFill,
    Section(SectionEvent),
    /// A face set from the properties pane: axis, min face, percent of the model.
    SectionSlider(usize, bool, f32),
    /// The turn from the properties pane, degrees.
    SectionTurn(f32),
    /// Switch the section plane on (wait for a click on a face) or off.
    TogglePlane,
    /// Cut away the other side of the section plane.
    FlipPlane,
    /// Move the section plane along its normal (world units).
    PlaneMove(f32),
    /// The section plane from the properties pane: percent between its extremes.
    PlaneSlider(f32),
    /// Switch the exploded view on or off.
    ToggleExplode,
    ExplodeMode(ExplodeMode),
    /// An axis of the exploded view: axis, percent.
    Explode(usize, f32),
    /// All axes of the exploded view back to 0 %.
    ResetExplode,
    /// Switch a measure tool on, or off when it is on.
    Measure(Tool),
    /// Enter: close the polygon being measured.
    MeasureFinish,
    MeasureDelete(usize),
    MeasureClear,
    MeasureLength(LengthUnit),
    MeasureArea(AreaUnit),
    MeasureVolume(VolumeUnit),
    /// Decimals shown for a category.
    MeasureDecimals(Category, u8),
    /// Esc: stop placing the section plane, else stop measuring, else clear the selection.
    Escape,
    /// A frame while loading (animates the loading card).
    Tick,
    TakeShot,
    Shot(window::Screenshot),
    /// A command from remote control (the MCP server).
    Remote(crate::remote::RemoteCall),
    /// The window captured for a remote `screenshot`.
    RemoteShot(window::Screenshot),
    ClearMeasurements,
}

impl From<Navigate> for Message {
    fn from(n: Navigate) -> Self {
        Message::Navigate(n)
    }
}

impl From<SectionEvent> for Message {
    fn from(e: SectionEvent) -> Self {
        Message::Section(e)
    }
}

impl From<PlaneMove> for Message {
    fn from(m: PlaneMove) -> Self {
        Message::PlaneMove(m.0)
    }
}

impl From<CubeEvent> for Message {
    fn from(e: CubeEvent) -> Self {
        Message::Cube(e)
    }
}

/// Length of a camera transition (view cube, presets).
const ANIM_SECONDS: f32 = 0.45;

impl Default for App {
    fn default() -> Self {
        Self {
            theme: ThemeKind::Light,
            model: None,
            visible: Vec::new(),
            camera: Camera::default(),
            mode: RenderMode::Solid,
            grid: true,
            flip_up: false,
            tab: RibbonTab::Home,
            show_layers: true,
            show_properties: true,
            layer_filter: String::new(),
            status: Status::Ready,
            viewport_size: (1000.0, 700.0),
            initial_view: None,
            window_shot: None,
            pending_open: None,
            selected: None,
            selection: Vec::new(),
            anim: None,
            section: None,
            section_on: false,
            section_fill: true,
            plane: None,
            plane_placed: None,
            plane_picking: false,
            active_view: None,
            show_grids: true,
            explorer_open: [false, false, true],
            measurements: Vec::new(),
            viewport_origin: (0.0, 0.0),
            remote: Default::default(),
            explode_on: false,
            explode_mode: ExplodeMode::Storeys,
            explode: [0.0; 3],
            explode_index: None,
            explode_offsets: Arc::default(),
            explode_bounds: None,
            measure: MeasureState::default(),
        }
    }
}

/// Loads a file on a worker thread.
#[cfg(not(target_arch = "wasm32"))]
pub fn load_task(source: loaders::Source, progress: Arc<loaders::Progress>) -> Task<Message> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(loaders::load_with(&source, &progress).map(Arc::new));
            });
            rx.await.unwrap_or_else(|_| Err("loader thread stopped".to_string()))
        },
        Message::Loaded,
    )
}

/// Loads a file in the browser (no threads there): after the next frame, so the loading card
/// shows first.
#[cfg(target_arch = "wasm32")]
pub fn load_task(source: loaders::Source, progress: Arc<loaders::Progress>) -> Task<Message> {
    Task::perform(
        async move {
            crate::web::next_frame().await;
            loaders::load_with(&source, &progress).map(Arc::new)
        },
        Message::Loaded,
    )
}

/// Asks for a model file: the native dialog on the desktop, the browser's file picker on the web
/// (which hands over the bytes, not a path).
fn pick_model_task() -> Task<Message> {
    let mut exts = loaders::supported_extensions();
    exts.sort_unstable();
    Task::perform(
        async move {
            let file = rfd::AsyncFileDialog::new()
                .set_title("Open 3D model")
                .add_filter("3D models", &exts)
                .add_filter("All files", &["*"])
                .pick_file()
                .await?;
            #[cfg(not(target_arch = "wasm32"))]
            return Some(loaders::Source::File(file.path().to_path_buf()));
            #[cfg(target_arch = "wasm32")]
            return Some(loaders::Source::Bytes { name: file.file_name(), data: Arc::new(file.read().await) });
        },
        Message::Picked,
    )
}

/// The file name offered for "Save as": the model's name in the first writer's format.
fn save_name(model: &Model) -> String {
    let stem = model.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "model".into());
    format!("{stem}.{}", export::registry()[0].extensions()[0])
}

/// Asks where to save the model, one filter per writer.
#[cfg(not(target_arch = "wasm32"))]
fn save_dialog_task(model: &Model) -> Task<Message> {
    let mut dialog = rfd::AsyncFileDialog::new().set_title("Save as").set_file_name(save_name(model));
    if let Some(dir) = model.path.parent() {
        dialog = dialog.set_directory(dir);
    }
    for w in &export::registry() {
        dialog = dialog.add_filter(w.name(), w.extensions());
    }
    Task::perform(async move { dialog.save_file().await.map(|f| f.path().to_path_buf()) }, Message::SavePicked)
}

/// The browser has no save dialog: the file goes to the downloads.
#[cfg(target_arch = "wasm32")]
fn save_dialog_task(model: &Model) -> Task<Message> {
    Task::done(Message::SavePicked(Some(PathBuf::from(save_name(model)))))
}

/// Saves the model on a worker thread.
#[cfg(not(target_arch = "wasm32"))]
pub fn save_task(model: Arc<Model>, path: PathBuf) -> Task<Message> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let start = Instant::now();
                let _ = tx.send(export::save(&model, &path).map(|()| (path, start.elapsed().as_secs_f32())));
            });
            rx.await.unwrap_or_else(|_| Err("save thread stopped".to_string()))
        },
        Message::Saved,
    )
}

/// Saves the model in the browser: written in memory and handed over as a download.
#[cfg(target_arch = "wasm32")]
pub fn save_task(model: Arc<Model>, path: PathBuf) -> Task<Message> {
    Task::perform(
        async move {
            crate::web::next_frame().await;
            let start = Instant::now();
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let bytes = export::to_bytes(&model, &name)?;
            crate::web::download(&name, &bytes, export::writer_for(&name)?.mime())?;
            Ok((path, start.elapsed().as_secs_f32()))
        },
        Message::Saved,
    )
}

impl App {
    pub fn title(&self) -> String {
        match &self.model {
            Some(m) => format!("{} - Open 3D Viewer Studio", m.file_name()),
            None => "Open 3D Viewer Studio".to_string(),
        }
    }

    pub fn theme(&self) -> Theme {
        self.theme.theme()
    }

    pub(crate) fn model_matrix(&self) -> Mat4 {
        match (&self.model, self.flip_up) {
            (Some(m), true) if m.source_y_up => Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2),
            (Some(_), true) => Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2),
            _ => Mat4::IDENTITY,
        }
    }

    pub(crate) fn aspect(&self) -> f32 {
        self.viewport_size.0 / self.viewport_size.1.max(1.0)
    }

    pub fn fit(&mut self) {
        self.anim = None;
        self.camera = self.fitted(self.camera);
    }

    /// `camera` framing the whole model.
    fn fitted(&self, mut camera: Camera) -> Camera {
        if let Some(b) = self.world_bounds() {
            camera.fit(&b, self.aspect());
        }
        camera
    }

    /// The model's bounds in world space (after the up-axis override), as drawn: pulled apart
    /// by the exploded view.
    pub(crate) fn world_bounds(&self) -> Option<Bounds> {
        let model = self.model.as_ref()?;
        self.explode_bounds.or_else(|| Some(transform_bounds(&model.bounds, &self.model_matrix())))
    }

    /// The offset (world) of vertex `i` of layer `l` in the exploded view.
    pub(crate) fn explode_offset(&self, l: usize, i: u32) -> Vec3 {
        let unit = self.explode_index.as_ref().map_or(0, |e| e.unit(l, i));
        self.explode_offsets.get(unit).map_or(Vec3::ZERO, |o| Vec3::new(o[0], o[1], o[2]))
    }

    /// Recomputes where the units of the exploded view are drawn, after its switch, mode or
    /// amounts, or the up axis changed; the far plane makes room for the model pulled apart.
    fn update_explode(&mut self) {
        let Some(index) = self.explode_index.clone() else { return };
        let amount = if self.explode_on { self.explode.map(|v| v / 100.0) } else { [0.0; 3] };
        let mm = self.model_matrix();
        self.explode_offsets = Arc::new(index.offsets(self.explode_mode, amount, &mm));
        self.explode_bounds = (amount != [0.0; 3]).then(|| index.bounds(&self.explode_offsets, &mm)).filter(Bounds::is_valid);
        if let Some(b) = self.world_bounds() {
            self.camera.scene_radius = self.camera.scene_radius.max(b.radius());
        }
    }

    /// World bounds of the selected triangles.
    pub(crate) fn selection_bounds(&self) -> Option<Bounds> {
        let model = self.model.as_ref()?;
        let mm = self.model_matrix();
        let mut b = Bounds::empty();
        for &(l, first, count) in &self.selection {
            let layer = model.layers.get(l)?;
            for &i in layer.indices.get(first as usize..(first + count) as usize)? {
                let at = 3 * i as usize;
                if let Some(p) = layer.positions.get(at..at + 3) {
                    b.extend((mm.transform_point3(Vec3::new(p[0], p[1], p[2])) + self.explode_offset(l, i)).to_array());
                }
            }
        }
        b.is_valid().then_some(b)
    }

    /// The section box when it is on.
    pub fn active_section(&self) -> Option<SectionBox> {
        self.section.filter(|_| self.section_on)
    }

    /// Shows a saved view, animated: its camera (a 3D view's eye and lens, the direction of a
    /// section or elevation, from above for a plan) and what it shows (a 3D view's section box,
    /// a section's crop, a plan's range from the view depth up to the cut plane) as the section
    /// box.
    pub fn apply_view(&mut self, i: usize) {
        let Some(model) = self.model.clone() else { return };
        let Some(view) = model.views.get(i).cloned() else { return };
        // views are in the file's coordinates, of the model at rest
        if self.flip_up {
            self.flip_up = false;
            self.section = None;
        }
        if self.explode_on {
            self.explode_on = false;
            self.update_explode();
        }
        let Some(world) = self.world_bounds() else { return };
        self.active_view = Some(i);
        let clip = match (view.plan, view.clip) {
            (Some(plan), _) => {
                let mut b = SectionBox::around(&world);
                b.min.z = plan.low;
                b.max.z = plan.high.max(plan.low + 0.01);
                Some(b)
            }
            (None, Some(c)) => Some(c.clamped_to(&world)),
            _ => None,
        };
        if clip.is_some() {
            self.section = clip;
        }
        self.section_on = clip.is_some();
        let frame = clip.map(|c| c.world_bounds()).unwrap_or(world);

        let max_pitch = 89.9f32.to_radians();
        let (yaw, pitch) = match (view.plan, view.camera) {
            (Some(plan), _) => if plan.from_below { ViewPreset::Bottom } else { ViewPreset::Top }.orientation(),
            (None, Some(c)) => {
                let f = Vec3::from_array(c.forward);
                ((-f.y).atan2(-f.x), (-f.z).clamp(-1.0, 1.0).asin())
            }
            _ => ViewPreset::Iso.orientation(),
        };
        let mut to = self.camera;
        to.yaw = yaw;
        to.pitch = pitch.clamp(-max_pitch, max_pitch);
        to.fov_y = 45f32.to_radians();
        match view.camera.filter(|c| c.perspective) {
            // a camera: its own eye and lens
            Some(c) => {
                let (eye, f) = (Vec3::from_array(c.eye), Vec3::from_array(c.forward));
                let d = (Vec3::from_array(frame.center()) - eye).dot(f).max(1.0);
                to.ortho = false;
                to.target = eye + f * d;
                to.distance = d;
                to.scene_radius = world.radius();
                if let Some(fov) = c.fov_y {
                    to.fov_y = fov.clamp(10f32.to_radians(), 120f32.to_radians());
                }
            }
            // Revit's other views are orthographic
            None => {
                to.ortho = true;
                to.fit(&frame, self.aspect());
            }
        }
        self.anim = Some((self.camera, to, Instant::now()));
    }

    /// Turns the camera to (yaw, pitch) and frames the model, animated.
    fn turn_to(&mut self, yaw: f32, pitch: f32) {
        let mut to = self.camera;
        to.yaw = yaw;
        to.pitch = pitch.clamp(-89.9f32.to_radians(), 89.9f32.to_radians());
        let to = self.fitted(to);
        self.anim = Some((self.camera, to, Instant::now()));
    }

    /// Installs a freshly loaded model (also used by the headless screenshot path).
    pub fn set_model(&mut self, model: Arc<Model>) {
        self.visible = model.layers.iter().map(|l| !l.hidden).collect();
        self.status = Status::Loaded(format!(
            "Loaded {} in {:.2} s",
            model.file_name(),
            model.load_time.as_secs_f32()
        ));
        self.model = Some(model);
        self.section = None;
        self.section_on = false;
        self.plane = None;
        self.plane_picking = false;
        self.active_view = None;
        self.explorer_open[0] = false;
        self.selected = None;
        self.selection.clear();
        self.measure.reset();
        self.measurements.clear();
        self.flip_up = false;
        self.layer_filter.clear();
        self.explode_index = self.model.as_deref().map(|m| Arc::new(ExplodeIndex::new(m)));
        self.explode_on = false;
        self.explode = [0.0; 3];
        self.update_explode();
        self.camera.set_preset(self.initial_view.take().unwrap_or(ViewPreset::Iso));
        self.fit();
    }

    /// For `--window-screenshot`: give the window time to render, then capture it.
    pub fn shot_after_delay(&self) -> Task<Message> {
        if self.window_shot.is_none() {
            return Task::none();
        }
        Task::perform(async { std::thread::sleep(std::time::Duration::from_millis(1500)) }, |_| Message::TakeShot)
    }

    /// Starts loading a model, unless one is loading already.
    fn open(&mut self, source: loaders::Source) -> Task<Message> {
        self.pending_open = None;
        if matches!(self.status, Status::Loading { .. }) {
            return Task::none();
        }
        let progress = Arc::new(loaders::Progress::default());
        self.status = Status::Loading { file: source.name(), since: Instant::now(), progress: progress.clone() };
        load_task(source, progress)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenDialog => return pick_model_task(),
            Message::Picked(None) => {}
            Message::Picked(Some(source)) => return self.open(source),
            Message::Open(path) => return self.open(loaders::Source::File(path)),
            Message::Loaded(Ok(model)) => {
                self.set_model(model);
                self.remote_loaded();
                return self.shot_after_delay();
            }
            Message::Loaded(Err(e)) => {
                self.status = Status::Error(e);
                self.remote_loaded();
                return self.shot_after_delay();
            }
            Message::Remote(call) => return self.remote(call),
            Message::RemoteShot(shot) => self.remote_shot(shot),
            Message::ClearMeasurements => self.measurements.clear(),
            Message::SaveDialog => {
                let Some(model) = &self.model else { return Task::none() };
                if matches!(self.status, Status::Loading { .. } | Status::Saving { .. }) {
                    return Task::none();
                }
                return save_dialog_task(model);
            }
            Message::SavePicked(None) => {}
            Message::SavePicked(Some(mut path)) => {
                let Some(model) = self.model.clone() else { return Task::none() };
                if path.extension().is_none() {
                    path.set_extension(export::registry()[0].extensions()[0]);
                }
                let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.status = Status::Saving { file, since: Instant::now() };
                return save_task(model, path);
            }
            Message::Saved(Ok((path, secs))) => {
                let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.status = Status::Loaded(format!("Saved {file} in {secs:.2} s"));
            }
            Message::Saved(Err(e)) => {
                self.status = Status::Error(e);
            }
            Message::TakeShot => {
                return window::latest().and_then(window::screenshot).map(Message::Shot);
            }
            Message::Shot(shot) => {
                if let Some(path) = self.window_shot.take() {
                    match crate::write_png(&path, shot.size.width, shot.size.height, &shot.rgba) {
                        Ok(()) => eprintln!("wrote {}", path.display()),
                        Err(e) => eprintln!("error: {e}"),
                    }
                    return iced::exit();
                }
            }
            Message::Mode(m) => self.mode = m,
            Message::ToggleGrid => self.grid = !self.grid,
            Message::ToggleFlipUp => {
                self.flip_up = !self.flip_up;
                self.update_explode();
                // the box, the plane and the measurements are in world space: start again
                self.section = None;
                self.plane = None;
                self.measure.reset();
                if self.section_on {
                    self.section = self.world_bounds().map(|b| SectionBox::around(&b));
                }
                self.fit();
            }
            Message::Theme(t) => self.theme = t,
            Message::Tab(t) => self.tab = t,
            Message::Preset(p) => {
                let (yaw, pitch) = p.orientation();
                self.turn_to(yaw, pitch);
            }
            Message::Cube(CubeEvent::Orbit { dx, dy }) => {
                self.anim = None;
                self.camera.orbit(dx, dy);
            }
            Message::Cube(CubeEvent::Snap(target)) => match target {
                CubeTarget::Face(p) => {
                    let (yaw, pitch) = p.orientation();
                    self.turn_to(yaw, pitch);
                }
                CubeTarget::Edge(d) | CubeTarget::Corner(d) => {
                    let (yaw, pitch) = view_from(d);
                    self.turn_to(yaw, pitch);
                }
                CubeTarget::Home => {
                    let (yaw, pitch) = ViewPreset::Iso.orientation();
                    self.turn_to(yaw, pitch);
                }
                CubeTarget::Lens => self.camera.ortho = !self.camera.ortho,
            },
            Message::Ortho(o) => self.camera.ortho = o,
            Message::Fit => self.fit(),
            Message::ToggleLayersPanel => self.show_layers = !self.show_layers,
            Message::TogglePropertiesPanel => self.show_properties = !self.show_properties,
            Message::Layer(i, v) => {
                if let Some(slot) = self.visible.get_mut(i) {
                    *slot = v;
                }
            }
            Message::AllLayers(v) => {
                let filter = self.layer_filter.to_lowercase();
                if let Some(m) = &self.model {
                    for (i, layer) in m.layers.iter().enumerate() {
                        if filter.is_empty() || layer.name.to_lowercase().contains(&filter) {
                            self.visible[i] = v;
                        }
                    }
                }
            }
            Message::LayerFilter(f) => self.layer_filter = f,
            Message::Navigate(n) => match n {
                Navigate::Orbit { dx, dy } => {
                    self.anim = None;
                    self.camera.orbit(dx, dy)
                }
                Navigate::Pan { dx, dy, height } => {
                    self.anim = None;
                    self.camera.pan(dx, dy, height)
                }
                Navigate::Zoom { steps, x, y, width, height } => {
                    self.anim = None;
                    let ndc = (2.0 * x / width.max(1.0) - 1.0, 1.0 - 2.0 * y / height.max(1.0));
                    self.camera.zoom_at(steps, ndc, width / height.max(1.0));
                }
                Navigate::Resized { x, y, width, height } => {
                    self.viewport_origin = (x, y);
                    self.viewport_size = (width, height);
                }
                Navigate::Pick { x, y, width, height } => self.pick(x, y, width, height),
            },
            Message::ApplyView(i) => self.apply_view(i),
            Message::ToggleGrids => self.show_grids = !self.show_grids,
            Message::ToggleExplorer(i) => {
                if let Some(open) = self.explorer_open.get_mut(i) {
                    *open = !*open;
                }
            }
            Message::ToggleSection => {
                let world = self.world_bounds();
                // switched on over a level or upright section plane: the plane becomes a face of
                // a box round the model, instead of a second cut
                if !self.section_on {
                    if let Some(b) = self.plane.zip(world).and_then(|(plane, world)| SectionBox::from_plane(&plane, &world)) {
                        self.section = Some(b);
                        self.plane = None;
                    }
                }
                if self.section.is_none() {
                    self.section = world.map(|b| SectionBox::around(&b));
                }
                self.section_on = !self.section_on && self.section.is_some();
            }
            Message::ResetSection => {
                if self.section_on {
                    self.section = self.world_bounds().map(|b| SectionBox::around(&b));
                }
                // the section plane back where it was placed: moves and flips undone
                if self.plane.is_some() {
                    self.plane = self.plane_placed;
                }
            }
            Message::SectionToSelection => {
                if let Some(b) = self.selection_bounds() {
                    self.section = Some(SectionBox::around(&b));
                    self.section_on = true;
                }
            }
            Message::ToggleSectionFill => self.section_fill = !self.section_fill,
            Message::Section(event) => {
                if let Some(s) = self.section.as_mut() {
                    match event {
                        SectionEvent::Move { axis, is_min, delta } => s.move_face(axis, is_min, delta),
                        SectionEvent::Turn(radians) => s.turn_by(radians),
                    }
                }
            }
            Message::SectionSlider(axis, is_min, percent) => {
                if let (Some(world), Some(s)) = (self.world_bounds(), self.section.as_mut()) {
                    let r = s.local_bounds(&world);
                    let to = r.min[axis] + (r.max[axis] - r.min[axis]) * percent / 100.0;
                    let now = if is_min { s.min[axis] } else { s.max[axis] };
                    s.move_face(axis, is_min, to - now);
                }
            }
            Message::SectionTurn(degrees) => {
                if let Some(s) = self.section.as_mut() {
                    s.turn_by(degrees.to_radians() - s.turn);
                }
            }
            Message::Tick => {
                if let Some((from, to, start)) = self.anim {
                    let t = (start.elapsed().as_secs_f32() / ANIM_SECONDS).min(1.0);
                    let eased = t * t * (3.0 - 2.0 * t);
                    self.camera = from.lerp(&to, eased);
                    if t >= 1.0 {
                        self.anim = None;
                    }
                }
            }
            Message::TogglePlane => {
                if self.plane.is_some() || self.plane_picking {
                    self.plane = None;
                    self.plane_picking = false;
                } else {
                    self.plane_picking = self.model.is_some();
                    self.measure.tool = None;
                    self.measure.picks.clear();
                }
            }
            Message::FlipPlane => {
                if let Some(plane) = self.plane.as_mut() {
                    plane.flip();
                }
            }
            Message::PlaneMove(delta) => {
                if let (Some(world), Some(plane)) = (self.world_bounds(), self.plane.as_mut()) {
                    plane.move_by(delta, &world);
                }
            }
            Message::PlaneSlider(percent) => {
                if let (Some(world), Some(plane)) = (self.world_bounds(), self.plane.as_mut()) {
                    let (lo, hi) = plane.range(&world);
                    plane.move_by(lo + (hi - lo) * percent / 100.0 - plane.offset, &world);
                }
            }
            Message::ToggleExplode => {
                self.explode_on = !self.explode_on && self.explode_index.is_some();
                // measurements are on the model as it is, not taken apart
                if self.explode_on {
                    self.measure.tool = None;
                    self.measure.picks.clear();
                }
                self.update_explode();
            }
            Message::ExplodeMode(mode) => {
                self.explode_mode = mode;
                self.update_explode();
            }
            Message::Explode(axis, percent) => {
                if let Some(a) = self.explode.get_mut(axis) {
                    *a = percent.clamp(0.0, 100.0);
                }
                self.update_explode();
            }
            Message::ResetExplode => {
                self.explode = [0.0; 3];
                self.update_explode();
            }
            Message::Measure(tool) => {
                self.measure.toggle(tool);
                if self.measure.tool.is_some() {
                    self.plane_picking = false;
                    if self.explode_on {
                        self.explode_on = false;
                        self.update_explode();
                    }
                }
            }
            Message::MeasureFinish => self.measure.finish(),
            Message::MeasureDelete(i) => {
                if i < self.measure.results.len() {
                    self.measure.results.remove(i);
                }
            }
            Message::MeasureClear => {
                self.measure.results.clear();
                self.measure.picks.clear();
                self.measurements.clear();
            }
            Message::MeasureLength(u) => self.measure.units.length = u,
            Message::MeasureArea(u) => self.measure.units.area = u,
            Message::MeasureVolume(u) => self.measure.units.volume = u,
            Message::MeasureDecimals(category, n) => {
                let n = n.min(MAX_DECIMALS);
                let u = &mut self.measure.units;
                match category {
                    Category::Length => u.length_decimals = n,
                    Category::Area => u.area_decimals = n,
                    Category::Volume => u.volume_decimals = n,
                    Category::Angle => u.angle_decimals = n,
                }
            }
            Message::Escape if self.plane_picking => self.plane_picking = false,
            Message::Escape => {
                if !self.measure.escape() {
                    self.selected = None;
                    self.selection.clear();
                }
            }
        }
        Task::none()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let ticks = if matches!(self.status, Status::Loading { .. } | Status::Saving { .. }) || self.anim.is_some() {
            window::frames().map(|_| Message::Tick)
        } else {
            Subscription::none()
        };
        let events = event::listen_with(|event, status, _window| match event {
            iced::Event::Window(window::Event::FileDropped(path)) => Some(Message::Open(path)),
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                if status == event::Status::Ignored =>
            {
                use keyboard::key::Named;
                use keyboard::Key;
                match key.as_ref() {
                    Key::Character("o") if modifiers.command() => Some(Message::OpenDialog),
                    Key::Character("s") if modifiers.command() && modifiers.shift() => Some(Message::SaveDialog),
                    Key::Character("f") => Some(Message::Fit),
                    Key::Character("g") => Some(Message::ToggleGrid),
                    Key::Character("p") => Some(Message::Ortho(false)),
                    Key::Character("o") => Some(Message::Ortho(true)),
                    Key::Character("w") => Some(Message::Mode(RenderMode::Wireframe)),
                    Key::Character("s") => Some(Message::Mode(RenderMode::Solid)),
                    Key::Character("e") => Some(Message::Mode(RenderMode::SolidEdges)),
                    Key::Character("1") => Some(Message::Preset(ViewPreset::Top)),
                    Key::Character("2") => Some(Message::Preset(ViewPreset::Front)),
                    Key::Character("3") => Some(Message::Preset(ViewPreset::Right)),
                    Key::Character("0") | Key::Named(Named::Home) => Some(Message::Preset(ViewPreset::Iso)),
                    Key::Named(Named::Escape) => Some(Message::Escape),
                    Key::Named(Named::Enter) => Some(Message::MeasureFinish),
                    _ => None,
                }
            }
            _ => None,
        });
        let remote = if self.remote.enabled { Subscription::run(crate::remote::serve) } else { Subscription::none() };
        // files from the page: `?file=URL` and drag and drop
        #[cfg(target_arch = "wasm32")]
        let remote = Subscription::batch([remote, crate::web::subscription()]);
        Subscription::batch([events, ticks, remote])
    }

    pub fn view(&self) -> Element<'_, Message> {
        let p = self.theme.palette();
        let body = row![]
            .push(self.show_layers.then(|| self.layers_panel(p)))
            .push(self.viewport_view(p, standalone_language()))
            .push(self.show_properties.then(|| self.properties_panel(p)))
            .height(Fill);
        column![self.ribbon(p), body, self.status_bar(p)].into()
    }

    /// Selects what is under the cursor: the nearest visible triangle, then its element (or the
    /// whole layer when the format has no elements); empty space clears the selection. While
    /// placing the section plane, puts it on the face under the cursor instead; while measuring,
    /// hands the click to the measure tool.
    pub(crate) fn pick(&mut self, x: f32, y: f32, width: f32, height: f32) {
        let Some(model) = self.model.clone() else { return };
        let (width, height) = (width.max(1.0), height.max(1.0));
        let inv = (self.camera.view_proj(width / height) * self.model_matrix()).inverse();
        let (nx, ny) = (2.0 * x / width - 1.0, 1.0 - 2.0 * y / height);
        let origin = inv.project_point3(Vec3::new(nx, ny, 0.0));
        let dir = inv.project_point3(Vec3::new(nx, ny, 1.0)) - origin;
        let mm = self.model_matrix();
        let (section, plane) = (self.active_section(), self.plane);
        let keep = |p: Vec3| {
            let w = mm.transform_point3(p);
            section.is_none_or(|s| s.contains(w)) && plane.is_none_or(|pl| !pl.cuts(w))
        };
        // the exploded view moves the triangles: their offsets back in model space
        let back = mm.inverse();
        let shift = |l: usize, i: u32| back.transform_vector3(self.explode_offset(l, i));
        let hit = pick_triangle(&model, &self.visible, origin, dir, &shift, &keep);
        if self.plane_picking {
            if let (Some((l, t)), Some(world)) = (hit, self.world_bounds()) {
                let s = model.layers[l].indices.get(3 * t).map_or(Vec3::ZERO, |&i| shift(l, i));
                if let Some((point, normal)) = face_at(&model.layers[l], t, origin - s, dir) {
                    let (point, mut normal) = (mm.transform_point3(point + s), mm.transform_vector3(normal));
                    // the face as seen: its normal towards the viewer
                    if normal.dot(mm.transform_vector3(dir)) > 0.0 {
                        normal = -normal;
                    }
                    if let Some(p) = SectionPlane::on_face(point, normal, &world) {
                        self.plane = Some(p);
                        self.plane_placed = Some(p);
                        self.plane_picking = false;
                    }
                }
            }
            return;
        }
        if let Some(tool) = self.measure.tool {
            let Some((l, t)) = hit else { return };
            // the point on the triangle as it is (the exploded view is off while measuring)
            let s = model.layers[l].indices.get(3 * t).map_or(Vec3::ZERO, |&i| shift(l, i));
            let Some((point, _)) = face_at(&model.layers[l], t, origin - s, dir) else { return };
            let pixel = crate::measure::pixel_size(&self.camera.view_proj(width / height), (width, height), mm.transform_point3(point));
            self.measure.click(&model, l, t, point, &mm, pixel);
            // the measured element is also selected (shows its properties)
            if tool != Tool::Volume {
                return;
            }
        }
        self.selected = hit.map(|(l, t)| (l, model.layers[l].element_at(t)));
        self.selection = match self.selected {
            Some((_, Some(id))) => model.element_ranges(id),
            Some((l, None)) => vec![(l, 0, model.layers[l].indices.len() as u32)],
            None => Vec::new(),
        };
    }

    // ---- ribbon ---------------------------------------------------------------------------

    fn ribbon(&self, p: Palette) -> Element<'_, Message> {
        let language = standalone_language();
        let tabs = crate::shell::model_ribbon(self, language);
        let theme = match self.theme {
            ThemeKind::Light => openaec_ui::Theme::Light,
            ThemeKind::Forge => openaec_ui::Theme::Forge,
            ThemeKind::Night => openaec_ui::Theme::OpenAec,
            ThemeKind::Blueprint => openaec_ui::Theme::Blueprint,
            ThemeKind::Contrast => openaec_ui::Theme::Contrast,
        };
        let heading = row![
            button(text(language.text("Openen", "Open")).size(11).font(FONT_BOLD))
                .padding([4, 10]).style(move |_, status| p.primary_button(status)).on_press(Message::OpenDialog),
            text("Model").size(11).font(FONT_BOLD).color(p.text),
            Space::new().width(Fill),
            text("Open 3D Viewer Studio").size(11).font(FONT_BOLD).color(p.accent),
        ].spacing(8).align_y(Center).padding([2, 8]);
        column![
            container(heading).height(28).width(Fill).style(move |_| p.title_bar()),
            openaec_ui::ribbon_body(theme, &tabs[0].groups),
        ].into()
    }

    // ---- panels ---------------------------------------------------------------------------

    fn layers_panel(&self, p: Palette) -> Element<'_, Message> {
        let header = panel_header(p, "Layers", self.model.as_ref().map(|m| m.layers.len().to_string()));
        panel(p, header, self.layers_body(p, standalone_language()), 220)
    }

    /// The explorer pane, organised as Revit's project browser: one search field, then
    /// collapsible groups (views by kind, rooms by level, layers) with their counts.
    pub fn layers_body(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let Some(model) = &self.model else {
            return text(l.text("Geen model geopend", "No model loaded")).size(12).color(p.text_muted).into();
        };
        let query = self.layer_filter.to_lowercase();
        let searching = !query.is_empty();
        let hit = |s: &str| !searching || s.to_lowercase().contains(&query);
        let indent = |content: Element<'static, Message>| -> Element<'static, Message> {
            container(content).padding(Padding { top: 0.0, right: 0.0, bottom: 0.0, left: 14.0 }).width(Fill).into()
        };
        let subheader = |label: &str| -> Element<'static, Message> {
            container(text(label.to_uppercase()).size(10).font(FONT_BOLD).color(p.group_label))
                .padding(Padding { top: 6.0, right: 0.0, bottom: 2.0, left: 20.0 })
                .into()
        };
        let mut body = column![].spacing(1);

        // views, by kind
        if !model.views.is_empty() {
            let kinds = [
                (ViewKind::ThreeD, l.text("3D-aanzichten", "3D views")),
                (ViewKind::Plan, l.text("Plattegronden", "Floor plans")),
                (ViewKind::Section, l.text("Doorsneden", "Sections")),
                (ViewKind::Elevation, l.text("Aanzichten", "Elevations")),
                (ViewKind::Detail, l.text("Details", "Details")),
            ];
            let shown: Vec<usize> = (0..model.views.len())
                .filter(|&i| hit(&model.views[i].name) || model.views[i].level.as_deref().is_some_and(|lv| hit(lv)))
                .collect();
            let open = self.explorer_open[0] || searching;
            body = body.push(group_header(p, l.text("Weergaven", "Views"), Some("beta"), shown.len(), open, Message::ToggleExplorer(0)));
            if open {
                for (kind, label) in kinds {
                    let items: Vec<usize> = shown.iter().copied().filter(|&i| model.views[i].kind == kind).collect();
                    if items.is_empty() {
                        continue;
                    }
                    body = body.push(subheader(label));
                    for i in items {
                        let active = self.active_view == Some(i);
                        let item = button(
                            text(model.views[i].name.clone())
                                .size(12)
                                .color(if active { p.accent } else { p.text })
                                .wrapping(Wrapping::WordOrGlyph)
                                .width(Fill),
                        )
                        .padding([3, 6])
                        .width(Fill)
                        .style(move |_, s| if active { p.ribbon_button(true, s) } else { p.row_button(s) })
                        .on_press(Message::ApplyView(i));
                        body = body.push(indent(item.into()));
                    }
                }
            }
        }

        // rooms, by level (low to high, then rooms without a level)
        let rooms = &model.project.rooms;
        if !rooms.is_empty() {
            let shown: Vec<&crate::model::Room> = rooms
                .iter()
                .filter(|r| hit(&r.name) || hit(&r.number) || r.extra.iter().any(|(_, v)| hit(v)))
                .collect();
            let open = self.explorer_open[1] || searching;
            body = body.push(group_header(p, l.text("Ruimten", "Rooms"), None, shown.len(), open, Message::ToggleExplorer(1)));
            if open {
                let mut levels: Vec<Option<String>> = model.project.levels.iter().map(|(n, _)| Some(n.clone())).collect();
                levels.push(None);
                for level in levels {
                    let items: Vec<&&crate::model::Room> = shown.iter().filter(|r| r.level == level).collect();
                    if items.is_empty() {
                        continue;
                    }
                    body = body.push(subheader(level.as_deref().unwrap_or(l.text("Zonder niveau", "No level"))));
                    for r in items {
                        let extra = r.extra.iter().map(|(_, v)| v.as_str()).collect::<Vec<_>>().join(" · ");
                        let item = row![
                            text(r.number.clone()).size(11).color(p.text_muted).width(30),
                            column![text(r.name.clone()).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph)]
                                .push((!extra.is_empty()).then(|| text(extra).size(10).color(p.text_muted).wrapping(Wrapping::WordOrGlyph)))
                                .width(Fill),
                        ]
                        .spacing(4)
                        .padding([2, 6]);
                        body = body.push(indent(item.into()));
                    }
                }
            }
        }

        // layers (categories)
        let mut order: Vec<usize> = (0..model.layers.len()).filter(|&i| hit(&model.layers[i].name)).collect();
        order.sort_by(|&a, &b| model.layers[a].name.to_lowercase().cmp(&model.layers[b].name.to_lowercase()));
        let open = self.explorer_open[2] || searching;
        body = body.push(group_header(p, l.text("Lagen", "Layers"), None, order.len(), open, Message::ToggleExplorer(2)));
        if open {
            let small = |label: &'static str, msg: Message| {
                button(text(label).size(11)).padding([2, 8]).style(move |_, s| p.ribbon_button(false, s)).on_press(msg)
            };
            body = body.push(indent(
                row![
                    small(l.text("Alles tonen", "Show all"), Message::AllLayers(true)),
                    small(l.text("Alles verbergen", "Hide all"), Message::AllLayers(false)),
                ]
                .spacing(4)
                .padding([4, 0])
                .into(),
            ));
            for i in order {
                let layer = &model.layers[i];
                let visible = self.visible.get(i).copied().unwrap_or(true);
                let [r, g, b] = layer.color;
                let swatch = container(Space::new().width(10).height(10)).style(move |_| Palette::swatch(iced::Color::from_rgb(r, g, b)));
                let eye = svg(icons::handle(if visible { icons::EYE } else { icons::EYE_OFF }))
                    .width(14)
                    .height(14)
                    .style(move |_, _| svg::Style { color: Some(if visible { p.text_secondary } else { p.text_muted }) });
                let item = button(
                    row![
                        eye,
                        swatch,
                        text(layer.name.clone())
                            .size(12)
                            .color(if visible { p.text } else { p.text_muted })
                            .wrapping(Wrapping::WordOrGlyph)
                            .width(Fill),
                        text(format_count(layer.triangle_count())).size(10).color(p.text_muted),
                    ]
                    .spacing(6)
                    .align_y(Center),
                )
                .padding([3, 6])
                .width(Fill)
                .style(move |_, s| p.row_button(s))
                .on_press(Message::Layer(i, !visible));
                body = body.push(indent(item.into()));
            }
        }

        let search = text_input(l.text("Zoeken in verkenner…", "Search explorer…"), &self.layer_filter)
            .on_input(Message::LayerFilter)
            .size(12)
            .padding([5, 8]);
        column![search, scrollable(body.padding(Padding { top: 0.0, right: 4.0, bottom: 8.0, left: 0.0 })).height(Fill).spacing(4)]
            .spacing(8)
            .into()
    }

    fn properties_panel(&self, p: Palette) -> Element<'_, Message> {
        let header = panel_header(p, "Properties", None);
        panel(p, header, self.properties_body(p, standalone_language()), 290)
    }

    /// The properties pane: project data (nothing selected) or the selection, the section box,
    /// then file, geometry and bounds. Labels and values share the width 2 : 3 and wrap, so long
    /// words never run into each other.
    pub fn properties_body(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let Some(model) = &self.model else {
            return text(l.text("Open een model om de eigenschappen te zien", "Open a model to see its properties"))
                .size(12)
                .color(p.text_muted)
                .into();
        };
        let b = transform_bounds(&model.bounds, &self.model_matrix());
        let size = b.size();
        let visible_tris: usize = model
            .layers
            .iter()
            .zip(&self.visible)
            .filter(|(_, v)| **v)
            .map(|(l, _)| l.triangle_count())
            .sum();
        let mut content = column![].spacing(5).padding(Padding { top: 0.0, right: 6.0, bottom: 8.0, left: 0.0 });
        let section = |title: &str| -> Element<'static, Message> {
            container(text(title.to_uppercase()).size(11).font(FONT_BOLD).color(p.group_label))
                .padding(Padding { top: 6.0, right: 0.0, bottom: 2.0, left: 0.0 })
                .into()
        };
        let prop = |k: &str, v: String| prop_row(p, k.to_string(), v);

        // project data while nothing is selected
        let pd = &model.project;
        if self.selected.is_none() {
            if !pd.info.is_empty() {
                content = content.push(section(l.text("Project", "Project")));
                for (k, v) in &pd.info {
                    let label = match k.as_str() {
                        "number" => l.text("Projectnummer", "Project number"),
                        "name" => l.text("Projectnaam", "Project name"),
                        "client" => l.text("Opdrachtgever", "Client"),
                        "status" => l.text("Status", "Status"),
                        other => other,
                    };
                    content = content.push(prop(label, v.clone()));
                }
                content = content.push(rule::horizontal(1));
            }
            if !pd.levels.is_empty() {
                content = content.push(section(l.text("Niveaus", "Levels")));
                for (name, z) in pd.levels.iter().rev() {
                    content = content.push(prop(name, format!("{z:+.3} m")));
                }
                content = content.push(rule::horizontal(1));
            }
            if !pd.phases.is_empty() {
                content = content
                    .push(section(l.text("Fasen", "Phases")))
                    .push(text(pd.phases.join(" → ")).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph))
                    .push(rule::horizontal(1));
            }
        }

        // the selection
        if let Some((li, element)) = self.selected {
            content = content.push(section(l.text("Selectie", "Selection")));
            let mut props = element.map(|id| model.element_props(id)).unwrap_or_default();
            if props.is_empty() {
                props.push(("category".into(), model.layers[li].name.clone()));
                if let Some(id) = element {
                    props.push(("id".into(), id.to_string()));
                }
            }
            // the order Revit's properties palette uses, unknown keys after
            props.sort_by_key(|(k, _)| PROP_ORDER.iter().position(|o| o.0 == k).unwrap_or(PROP_ORDER.len()));
            for (k, v) in props {
                let label = PROP_ORDER.iter().find(|o| o.0 == k).map_or(k.as_str(), |o| l.text(o.1, o.2)).to_string();
                content = content.push(prop_row(p, label, v));
            }
            content = content.push(rule::horizontal(1));
        }

        // the measurements
        if self.measure.tool.is_some() || !self.measure.results.is_empty() {
            content = content.push(self.measurements_body(p, l)).push(rule::horizontal(1));
        }

        // the section box
        if let (Some(s), true, Some(world)) = (self.section, self.section_on, self.world_bounds()) {
            let r = s.local_bounds(&world);
            content = content.push(section(l.text("Doorsnedebox", "Section box")));
            let slider_row = |label: String, control: Element<'static, Message>, value: String| -> Element<'static, Message> {
                column![
                    row![
                        text(label).size(12).color(p.text_secondary).width(Fill),
                        text(value).size(11).color(p.text_muted),
                    ]
                    .spacing(6),
                    control,
                ]
                .spacing(2)
                .into()
            };
            for (axis, name) in ["X", "Y", "Z"].into_iter().enumerate() {
                let span = (r.max[axis] - r.min[axis]).max(1e-6);
                for is_min in [true, false] {
                    let v = if is_min { s.min[axis] } else { s.max[axis] };
                    let percent = ((v - r.min[axis]) / span * 100.0).clamp(0.0, 100.0);
                    content = content.push(slider_row(
                        format!("{name} {}", if is_min { "min" } else { "max" }),
                        slider(0.0..=100.0, percent, move |v| Message::SectionSlider(axis, is_min, v)).step(0.1_f32).width(Fill).into(),
                        format!("{v:.2} m"),
                    ));
                }
            }
            let degrees = s.turn.to_degrees();
            content = content
                .push(slider_row(
                    l.text("Draaiing", "Turn").to_string(),
                    slider(-180.0..=180.0, degrees, Message::SectionTurn).step(0.5_f32).width(Fill).into(),
                    format!("{degrees:.1}°"),
                ))
                .push(rule::horizontal(1));
        }

        // the section plane
        if let (Some(plane), Some(world)) = (self.plane, self.world_bounds()) {
            let (lo, hi) = plane.range(&world);
            let percent = ((plane.offset - lo) / (hi - lo).max(1e-6) * 100.0).clamp(0.0, 100.0);
            let n = plane.normal;
            content = content
                .push(section(l.text("Doorsnedevlak", "Section plane")))
                .push(prop(l.text("Normaal", "Normal"), format!("{:.2}, {:.2}, {:.2}", n.x, n.y, n.z)))
                .push(
                    column![
                        row![
                            text(l.text("Positie", "Position")).size(12).color(p.text_secondary).width(Fill),
                            text(format!("{:.2} m", plane.offset)).size(11).color(p.text_muted),
                        ]
                        .spacing(6),
                        slider(0.0..=100.0, percent, Message::PlaneSlider).step(0.1_f32).width(Fill),
                    ]
                    .spacing(2),
                )
                .push(rule::horizontal(1));
        }

        // the exploded view: the mode (storeys only with levels), an amount per axis, reset
        if let (true, Some(index)) = (self.explode_on, &self.explode_index) {
            let mode = index.mode(self.explode_mode);
            let small = |label: &'static str, active: bool, msg: Option<Message>| {
                button(text(label).size(11)).padding([3, 8]).style(move |_, s| p.ribbon_button(active, s)).on_press_maybe(msg)
            };
            let storeys = (index.storeys() > 0).then_some(Message::ExplodeMode(ExplodeMode::Storeys));
            content = content.push(section("Explode")).push(
                row![
                    small(l.text("Verdiepingen", "Storeys"), mode == ExplodeMode::Storeys, storeys),
                    small(l.text("Elementen", "Elements"), mode == ExplodeMode::Elements, Some(Message::ExplodeMode(ExplodeMode::Elements))),
                    Space::new().width(Fill),
                    small(l.text("Herstellen", "Reset"), false, (self.explode != [0.0; 3]).then_some(Message::ResetExplode)),
                ]
                .spacing(4)
                .align_y(Center),
            );
            for (axis, name) in ["X", "Y", "Z"].into_iter().enumerate() {
                let v = self.explode[axis];
                content = content.push(
                    row![
                        text(name).size(12).font(FONT_BOLD).color(AXIS_COLORS[axis]).width(14),
                        slider(0.0..=100.0, v, move |v| Message::Explode(axis, v)).step(1.0_f32).width(Fill),
                        text(format!("{v:.0}%")).size(11).color(p.text_muted).width(34).align_x(iced::alignment::Horizontal::Right),
                    ]
                    .spacing(8)
                    .align_y(Center),
                );
            }
            content = content.push(rule::horizontal(1));
        }

        content = content
            .push(section(l.text("Bestand", "File")))
            .push(prop(l.text("Naam", "Name"), model.file_name()))
            .push(prop(l.text("Formaat", "Format"), model.format.clone()))
            .push(prop(l.text("Grootte", "Size"), format_bytes(model.file_size)))
            .push(prop(l.text("Map", "Folder"), model.path.parent().map(|d| d.display().to_string()).unwrap_or_default()))
            .push(prop(l.text("Laadtijd", "Load time"), format!("{:.2} s", model.load_time.as_secs_f32())))
            .push(rule::horizontal(1))
            .push(section(l.text("Geometrie", "Geometry")))
            .push(prop(l.text("Lagen", "Layers"), model.layers.len().to_string()))
            .push(prop(l.text("Hoekpunten", "Vertices"), format_count(model.vertex_count())))
            .push(prop(l.text("Driehoeken", "Triangles"), format_count(model.triangle_count())))
            .push(prop(l.text("Zichtbaar", "Visible"), format_count(visible_tris)))
            .push(rule::horizontal(1))
            .push(section(l.text("Grenzen (Z omhoog)", "Bounds (Z up)")))
            .push(prop("Min", format_vec(b.min)))
            .push(prop("Max", format_vec(b.max)))
            .push(prop(l.text("Afmeting", "Size"), format_vec(size)))
            .push(prop(
                l.text("Opwaartse as", "Up axis"),
                if model.source_y_up != self.flip_up { l.text("Y (omgezet)", "Y (converted)").into() } else { "Z".into() },
            ));
        // the scrollbar gets its own strip next to the text
        scrollable(content).height(Fill).spacing(4).into()
    }

    /// The measurements in the properties pane: each result with its details and a delete button,
    /// the totals, and the units with their decimals.
    fn measurements_body(&self, p: Palette, l: Language) -> Element<'static, Message> {
        let m = &self.measure;
        let units = &m.units;
        let heading = |title: &str| -> Element<'static, Message> {
            container(text(title.to_uppercase()).size(11).font(FONT_BOLD).color(p.group_label))
                .padding(Padding { top: 6.0, right: 0.0, bottom: 2.0, left: 0.0 })
                .into()
        };
        let chip = |label: String, active: bool, msg: Option<Message>| {
            button(text(label).size(11)).padding([2, 6]).style(move |_, s| p.ribbon_button(active, s)).on_press_maybe(msg)
        };
        let mut body = column![heading(l.text("Metingen", "Measurements"))].spacing(5);
        if m.results.is_empty() {
            body = body.push(
                text(l.text("Nog geen metingen", "No measurements yet")).size(12).color(p.text_muted),
            );
        }
        for (i, result) in m.results.iter().enumerate() {
            body = body.push(
                row![
                    text(format!("{}. {}", i + 1, result.title(l)))
                        .size(12)
                        .color(p.text_secondary)
                        .width(Fill)
                        .wrapping(Wrapping::WordOrGlyph),
                    text(result.label(units, l)).size(12).font(FONT_BOLD).color(p.text),
                    chip("×".into(), false, Some(Message::MeasureDelete(i))),
                ]
                .spacing(6)
                .align_y(Center),
            );
            for (k, v) in result.details(units, l) {
                body = body.push(container(prop_row(p, k.to_string(), v)).padding(Padding { left: 14.0, ..Padding::ZERO }));
            }
        }
        for (category, total, count) in m.totals() {
            if count > 1 {
                body = body.push(prop_row(
                    p,
                    format!("{} {} ({count})", l.text("Totaal", "Total"), category.label(l).to_lowercase()),
                    units.format(l, category, total),
                ));
            }
        }
        if !m.results.is_empty() {
            body = body.push(chip(l.text("Alles wissen", "Clear all").into(), false, Some(Message::MeasureClear)));
        }

        // units and decimals
        body = body.push(heading(l.text("Eenheden", "Units")));
        let unit_row = |category: Category, choices: Element<'static, Message>, decimals: u8| -> Element<'static, Message> {
            let step = |label: &str, to: Option<u8>| chip(label.into(), false, to.map(|n| Message::MeasureDecimals(category, n)));
            row![
                text(category.label(l)).size(12).color(p.text_secondary).width(Fill),
                choices,
                step("−", decimals.checked_sub(1)),
                text(decimals.to_string()).size(11).color(p.text).width(12).align_x(Center),
                step("+", (decimals < MAX_DECIMALS).then_some(decimals + 1)),
            ]
            .spacing(3)
            .align_y(Center)
            .into()
        };
        body = body
            .push(unit_row(
                Category::Length,
                row(LengthUnit::ALL.map(|u| chip(u.label().into(), units.length == u, Some(Message::MeasureLength(u))).into())).spacing(2).into(),
                units.length_decimals,
            ))
            .push(unit_row(
                Category::Area,
                row(AreaUnit::ALL.map(|u| chip(u.label().into(), units.area == u, Some(Message::MeasureArea(u))).into())).spacing(2).into(),
                units.area_decimals,
            ))
            .push(unit_row(
                Category::Volume,
                row(VolumeUnit::ALL.map(|u| chip(u.label().into(), units.volume == u, Some(Message::MeasureVolume(u))).into())).spacing(2).into(),
                units.volume_decimals,
            ))
            .push(unit_row(Category::Angle, text("°").size(11).color(p.text_muted).into(), units.angle_decimals));
        body.into()
    }

    // ---- viewport -------------------------------------------------------------------------

    /// The 3D viewport with its overlays; the shell's workspace.
    pub fn viewport_view(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let settings = RenderSettings {
            mode: self.mode,
            grid: self.grid,
            background: srgb3(p.viewport),
            grid_minor: srgb4(p.grid_minor),
            grid_major: srgb4(p.grid_major),
            model_matrix: self.model_matrix(),
            section: self.active_section(),
            plane: self.plane.zip(self.world_bounds()).map(|(p, w)| p.uniform(&w)),
            caps: self.section_fill,
            grids: self.show_grids && !self.flip_up,
            bounds: self.world_bounds(),
        };
        let scene = shader(Viewport3D {
            model: self.model.as_ref(),
            visible: &self.visible,
            selection: &self.selection,
            camera: self.camera,
            settings,
            explode: self.explode_index.as_ref(),
            offsets: &self.explode_offsets,
        })
            .width(Fill)
            .height(Fill);

        let overlay: Option<Element<'_, Message>> = match (&self.model, &self.status) {
            (_, Status::Loading { file, since, progress }) => {
                Some(center(loading_card(p, l, file, *since, progress)).into())
            }
            (None, _) => Some(center(self.welcome(p)).into()),
            _ => None,
        };
        let hint = container(
            text(l.text(
                "Klik: selecteren · Esc: wissen · Middelste knop: pannen · Shift + middelste of links slepen: orbit · Wiel: zoomen · F: passend",
                "Click: select · Esc: clear · Middle drag: pan · Shift + middle or left drag: orbit · Wheel: zoom · F: fit",
            ))
                .size(11)
                .color(p.text_muted),
        )
        .padding(8)
        .align_bottom(Fill)
        .align_left(Fill);
        let cube = (self.model.is_some() && overlay.is_none()).then(|| {
            canvas(CubeOverlay { yaw: self.camera.yaw, pitch: self.camera.pitch, ortho: self.camera.ortho })
                .width(Fill)
                .height(Fill)
        });
        let section = self.active_section().filter(|_| overlay.is_none()).map(|section| {
            canvas(SectionOverlay { section, view_proj: self.camera.view_proj(self.aspect()) }).width(Fill).height(Fill)
        });
        let plane = self.plane.zip(self.world_bounds()).filter(|_| overlay.is_none() && !self.plane_picking).map(|(plane, world)| {
            canvas(PlaneOverlay { plane, world, view_proj: self.camera.view_proj(self.aspect()) }).width(Fill).height(Fill)
        });
        let banner = if self.plane_picking {
            Some(l.text("Klik op een vlak om door te snijden · Esc: annuleren", "Click a face to cut there · Esc: cancel").to_string())
        } else {
            self.measure.tool.map(|tool| {
                format!("{}: {} · Esc: {}", tool.label(l), tool.hint(l, self.measure.picks.len()), l.text("stoppen", "stop"))
            })
        };
        // hidden while the model is taken apart: they belong to the model as it is
        let measures = (overlay.is_none()
            && !self.explode_on
            && (!self.measure.results.is_empty() || !self.measure.picks.is_empty()))
        .then(|| {
            canvas(crate::measure::overlay::MeasureOverlay {
                results: &self.measure.results,
                picks: &self.measure.picks,
                units: self.measure.units,
                language: l,
                view_proj: self.camera.view_proj(self.aspect()),
                ink: p.accent,
                label_bg: p.surface,
                label_text: p.text,
            })
            .width(Fill)
            .height(Fill)
        });
        let picking = banner.filter(|_| overlay.is_none()).map(|banner| {
            container(
                container(text(banner).size(12).color(Color::WHITE))
                    .padding([6, 12])
                    .style(|_| container::Style {
                        background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.6))),
                        border: Border { radius: 4.0.into(), ..Default::default() },
                        ..Default::default()
                    }),
            )
            .padding(12)
            .align_top(Fill)
            .center_x(Fill)
        });
        let grids = self
            .model
            .as_ref()
            .filter(|m| self.show_grids && !self.flip_up && overlay.is_none() && !m.project.grids.is_empty())
            .map(|m| {
                canvas(crate::annotations::GridOverlay {
                    grids: &m.project.grids,
                    view_proj: self.camera.view_proj(self.aspect()),
                    fill: p.surface,
                    ink: p.text_secondary,
                })
                .width(Fill)
                .height(Fill)
            });
        let measurements = (!self.measurements.is_empty() && overlay.is_none()).then(|| {
            canvas(crate::annotations::MeasureOverlay {
                items: &self.measurements,
                view_proj: self.camera.view_proj(self.aspect()),
                ink: p.accent,
                fill: p.surface,
                text: p.text,
            })
            .width(Fill)
            .height(Fill)
        });
        stack![scene, hint]
            .push(grids)
            .push(section)
            .push(plane)
            .push(measures)
            .push(measurements)
            .push(picking)
            .push(cube)
            .push(overlay)
            .width(Fill)
            .height(Fill)
            .into()
    }

    fn welcome(&self, p: Palette) -> Element<'_, Message> {
        let mut exts: Vec<String> = loaders::supported_extensions().iter().map(|e| format!(".{e}")).collect();
        exts.sort();
        container(
            column![
                text("Open 3D Viewer Studio").size(24).font(FONT_BOLD).color(p.text),
                text("Rust + iced edition").size(13).color(p.accent),
                Space::new().height(8),
                text("Open a model or drop a file on the window.").size(13).color(p.text_secondary),
                text(exts.join("  ")).size(12).color(p.text_muted),
                Space::new().height(8),
                button(
                    row![
                        svg(icons::handle(icons::OPEN))
                            .width(18)
                            .height(18)
                            .style(move |_, _| svg::Style { color: Some(p.accent_text) }),
                        text("Open file…").size(13).font(FONT_BOLD),
                    ]
                    .spacing(8)
                    .align_y(Center)
                )
                .padding([8, 18])
                .style(move |_, s| p.primary_button(s))
                .on_press(Message::OpenDialog),
            ]
            .spacing(6)
            .align_x(Center),
        )
        .padding([28, 40])
        .style(move |_| p.overlay())
        .into()
    }

    // ---- status bar -----------------------------------------------------------------------

    fn status_bar(&self, p: Palette) -> Element<'_, Message> {
        let (msg, color) = match &self.status {
            Status::Ready => ("Ready".to_string(), p.status_text),
            Status::Loading { file, since, .. } => {
                (format!("Loading {file}… {:.0} s", since.elapsed().as_secs_f32()), p.accent)
            }
            Status::Saving { file, since } => (format!("Saving {file}… {:.0} s", since.elapsed().as_secs_f32()), p.accent),
            Status::Loaded(s) => (s.clone(), p.status_text),
            Status::Error(e) => (format!("Error: {e}"), iced::Color::from_rgb8(0xF8, 0x71, 0x71)),
        };
        let item = |label: &'static str, value: String| {
            row![text(format!("{label}:")).size(11).color(Palette::dim(p.status_text)), text(value).size(11)].spacing(4)
        };
        let mut right = row![].spacing(16).align_y(Center);
        if let Some(m) = &self.model {
            right = right
                .push(item("Layers", m.layers.len().to_string()))
                .push(item("Vertices", format_count(m.vertex_count())))
                .push(item("Triangles", format_count(m.triangle_count())));
        }
        right = right.push(item(
            "Mode",
            match self.mode {
                RenderMode::Solid => "Solid",
                RenderMode::Wireframe => "Wireframe",
                RenderMode::SolidEdges => "Edges",
            }
            .into(),
        ));
        container(
            row![text(msg).size(11).color(color).width(Fill), right]
                .align_y(Vertical::Center)
                .padding([0, 10]),
        )
        .height(24)
        .align_y(Center)
        .width(Fill)
        .style(move |_| p.status_bar())
        .into()
    }
}

/// The axes on the exploded view's sliders: X and Y as the axis lines of the ground grid (red,
/// green), Z blue.
const AXIS_COLORS: [Color; 3] = [Color::from_rgb8(217, 70, 60), Color::from_rgb8(80, 170, 80), Color::from_rgb8(60, 120, 216)];

/// Element property keys (from the loaders) and their labels, in display order.
const PROP_ORDER: [(&str, &str, &str); 7] = [
    ("category", "Categorie", "Category"),
    ("family", "Familie", "Family"),
    ("type", "Type", "Type"),
    ("level", "Niveau", "Level"),
    ("materials", "Materialen", "Materials"),
    ("id", "Element-id", "Element id"),
    ("class", "Klasse", "Class"),
];

/// Overall progress of a load: each step owns a share of the bar and fills it asymptotically
/// with its running time (the loaders report steps, not percentages).
fn load_fraction(phase: loaders::Phase, seconds: f32, slow: bool) -> f32 {
    use loaders::Phase;
    let (start, share, tau) = match phase {
        Phase::Read => (0.0, 0.3, if slow { 3.0 } else { 0.8 }),
        Phase::Convert => (0.3, 0.6, if slow { 15.0 } else { 1.5 }),
        Phase::Scene => (0.9, 0.09, 0.5),
    };
    start + share * (1.0 - (-seconds / tau).exp())
}

/// Where the ray `origin + t·dir` meets triangle `t` of `layer`, and the triangle's normal
/// (model space, not oriented).
pub(crate) fn face_at(layer: &crate::model::Layer, t: usize, origin: Vec3, dir: Vec3) -> Option<(Vec3, Vec3)> {
    let tri = layer.indices.get(3 * t..3 * t + 3)?;
    let at = |i: u32| layer.positions.get(3 * i as usize..3 * i as usize + 3).map(|p| Vec3::new(p[0], p[1], p[2]));
    let (a, b, c) = (at(tri[0])?, at(tri[1])?, at(tri[2])?);
    let normal = (b - a).cross(c - a).try_normalize()?;
    let along = normal.dot(dir);
    if along.abs() < 1e-12 {
        return None;
    }
    Some((origin + dir * (normal.dot(a - origin) / along), normal))
}

/// The nearest triangle hit by the ray `origin + t·dir` (t > 0) among the visible layers:
/// (layer, triangle), skipping hits that `keep` rejects (the section box). A triangle is where
/// `shift` (layer, vertex) moves its first vertex: the exploded view, whose units never share a
/// vertex. Möller–Trumbore, two-sided.
pub fn pick_triangle(
    model: &Model,
    visible: &[bool],
    origin: Vec3,
    dir: Vec3,
    shift: &dyn Fn(usize, u32) -> Vec3,
    keep: &dyn Fn(Vec3) -> bool,
) -> Option<(usize, usize)> {
    let mut best: Option<(f32, usize, usize)> = None;
    for (l, layer) in model.layers.iter().enumerate() {
        if !visible.get(l).copied().unwrap_or(true) {
            continue;
        }
        let pos = &layer.positions;
        let at = |i: u32| {
            let i = 3 * i as usize;
            Vec3::new(pos[i], pos[i + 1], pos[i + 2])
        };
        for (t, tri) in layer.indices.chunks_exact(3).enumerate() {
            if tri.iter().any(|&i| 3 * i as usize + 2 >= pos.len()) {
                continue;
            }
            let (a, b, c) = (at(tri[0]), at(tri[1]), at(tri[2]));
            let (e1, e2) = (b - a, c - a);
            let h = dir.cross(e2);
            let det = e1.dot(h);
            if det.abs() < 1e-12 {
                continue;
            }
            let f = 1.0 / det;
            // the ray moved back instead of the triangle
            let s = origin - shift(l, tri[0]) - a;
            let u = f * s.dot(h);
            if !(0.0..=1.0).contains(&u) {
                continue;
            }
            let q = s.cross(e1);
            let v = f * dir.dot(q);
            if v < 0.0 || u + v > 1.0 {
                continue;
            }
            let d = f * e2.dot(q);
            if d > 0.0 && best.is_none_or(|(bd, _, _)| d < bd) && keep(origin + dir * d) {
                best = Some((d, l, t));
            }
        }
    }
    best.map(|(_, l, t)| (l, t))
}

// ---- small view helpers ---------------------------------------------------------------------

/// The loading card (as the OpenAEC dialogs: 12 px radius, amber accent): format badge, file name
/// and elapsed time; the three steps; a bar that eases within the current step with a moving
/// sheen; the step and percentage; for Revit after a few seconds a hint why it takes a while.
fn loading_card<'a>(
    p: Palette,
    l: Language,
    file: &'a str,
    since: Instant,
    progress: &loaders::Progress,
) -> Element<'a, Message> {
    let mono = openaec_ui::fonts::MONO;
    let (phase, in_phase) = progress.get();
    let elapsed = since.elapsed().as_secs_f32();
    let ext = std::path::Path::new(file).extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_default();
    let slow = matches!(ext.as_str(), "RVT" | "RFA");
    let fraction = load_fraction(phase, in_phase.as_secs_f32(), slow);

    let badge = container(text(ext).size(10).font(mono).color(p.accent)).padding([3, 6]).style(move |_| container::Style {
        background: Some(Background::Color(Color { a: 0.12, ..p.accent })),
        border: Border { color: Color { a: 0.35, ..p.accent }, width: 1.0, radius: 4.0.into() },
        ..Default::default()
    });
    let name = container(text(file).size(13).font(FONT_BOLD).color(p.text).wrapping(text::Wrapping::None))
        .width(Fill)
        .clip(true);
    let head = row![badge, name, text(format!("{elapsed:.1} s")).size(11).font(mono).color(p.text_muted)]
        .spacing(10)
        .align_y(Center);

    let labels = [
        l.text("Bestand lezen", "Read file"),
        l.text("Geometrie omzetten", "Convert geometry"),
        l.text("Scène opbouwen", "Build scene"),
    ];
    let current = phase as usize;
    let steps = row(labels.iter().enumerate().map(|(i, label)| {
        let (done, active) = (i < current, i == current);
        let number = text((i + 1).to_string()).size(9).font(mono).color(if done {
            p.accent_text
        } else if active {
            p.accent
        } else {
            p.text_muted
        });
        let dot = container(number).center(16).style(move |_| container::Style {
            background: done.then_some(Background::Color(p.accent)),
            border: Border {
                color: if done || active { p.accent } else { p.border },
                width: if active { 2.0 } else { 1.0 },
                radius: 8.0.into(),
            },
            ..Default::default()
        });
        let color = if active {
            p.text
        } else if done {
            p.text_secondary
        } else {
            p.text_muted
        };
        row![dot, text(*label).size(11).font(if active { FONT_BOLD } else { FONT }).color(color).wrapping(text::Wrapping::None)]
            .spacing(6)
            .align_y(Center)
            .width(Fill)
            .into()
    }))
    .spacing(6);

    // bar: the fill in thousandths, a sheen sweeping through it every 1.4 s
    let k = ((fraction * 1000.0).round() as u16).clamp(1, 999);
    let sweep = ((elapsed / 1.4).fract() * 1300.0) as u16;
    let sheen = row![
        Space::new().width(Length::FillPortion(sweep.max(1))),
        container(Space::new().height(6)).width(Length::FillPortion(300)).style(move |_| container::Style {
            background: Some(Background::Color(Color { a: 0.35, ..Color::WHITE })),
            ..Default::default()
        }),
        Space::new().width(Length::FillPortion(1300u16.saturating_sub(sweep).max(1))),
    ];
    let fill = container(sheen).width(Length::FillPortion(k)).height(6).clip(true).style(move |_| container::Style {
        background: Some(Background::Color(p.accent)),
        border: Border { radius: 3.0.into(), ..Default::default() },
        ..Default::default()
    });
    let bar = container(row![fill, Space::new().width(Length::FillPortion(1000 - k))])
        .height(6)
        .width(Fill)
        .clip(true)
        .style(move |_| container::Style {
            background: Some(Background::Color(Color { a: 0.6, ..p.border })),
            border: Border { radius: 3.0.into(), ..Default::default() },
            ..Default::default()
        });
    let step = match phase {
        loaders::Phase::Read => l.text("Bestand lezen…", "Reading file…"),
        loaders::Phase::Convert => l.text("Geometrie omzetten…", "Converting geometry…"),
        loaders::Phase::Scene => l.text("Scène opbouwen…", "Building scene…"),
    };
    let foot = row![
        text(step).size(11).color(p.text_secondary).width(Fill),
        text(format!("{:.0}%", fraction * 100.0)).size(11).font(mono).color(p.text),
    ]
    .align_y(Center);

    let hint = (slow && elapsed > 4.0).then(|| {
        container(
            text(l.text(
                "Revit-modellen worden uit de opgeslagen graphics van het bestand opgebouwd; grote projecten duren tot een minuut.",
                "Revit models are built from the file's saved graphics; large projects take up to a minute.",
            ))
            .size(11)
            .color(p.text_secondary),
        )
        .padding([8, 10])
        .width(Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(Color { a: 0.08, ..p.text_muted })),
            border: Border { radius: 8.0.into(), ..Default::default() },
            ..Default::default()
        })
    });

    container(column![head, steps, bar, foot].push(hint).spacing(12))
        .width(420)
        .padding([18, 20])
        .style(move |_| container::Style {
            text_color: Some(p.text),
            background: Some(Background::Color(p.surface)),
            border: Border { color: p.border, width: 1.0, radius: 12.0.into() },
            shadow: Shadow { color: Color { a: 0.18, ..Color::BLACK }, offset: iced::Vector::new(0.0, 8.0), blur_radius: 28.0 },
            ..Default::default()
        })
        .into()
}

/// A property: label and value side by side (2 : 3), both wrapping, so long words never overlap;
/// a long label (a project parameter such as "opdrachtgever_projectmanager") gets its own line
/// with the value below it, instead of being broken mid-word.
fn prop_row(p: Palette, label: String, value: String) -> Element<'static, Message> {
    if label.chars().count() > 16 {
        return column![
            text(label).size(12).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph),
            container(text(value).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph))
                .padding(Padding { top: 0.0, right: 0.0, bottom: 0.0, left: 10.0 }),
        ]
        .spacing(1)
        .into();
    }
    row![
        text(label).size(12).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph).width(Length::FillPortion(2)),
        text(value).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph).width(Length::FillPortion(3)),
    ]
    .spacing(8)
    .into()
}

/// The header of an explorer group: open/closed arrow, title, optional tag, count.
fn group_header(p: Palette, title: &str, tag: Option<&'static str>, count: usize, open: bool, msg: Message) -> Element<'static, Message> {
    use openaec_ui::icons::Icon;
    let arrow = svg(svg::Handle::from_memory(if open { Icon::DisclosureExpanded } else { Icon::DisclosureCollapsed }.bytes()))
        .width(12)
        .height(12)
        .style(move |_, _| svg::Style { color: Some(p.text_secondary) });
    button(
        row![arrow, text(title.to_string()).size(12).font(FONT_BOLD).color(p.text)]
            .push(tag.map(|t| text(t).size(10).color(p.accent)))
            .push(Space::new().width(Fill))
            .push(text(count.to_string()).size(11).color(p.text_muted))
            .spacing(6)
            .align_y(Center),
    )
    .padding([6, 4])
    .width(Fill)
    .style(move |_, s| p.row_button(s))
    .on_press(msg)
    .into()
}

fn panel_header<'a>(p: Palette, title: &'static str, badge: Option<String>) -> Element<'a, Message> {
    container(
        row![text(title).size(12).font(FONT_BOLD).color(p.text).width(Fill)]
            .push(badge.map(|b| text(b).size(11).color(p.text_muted)))
            .align_y(Center),
    )
    .padding([7, 10])
    .width(Fill)
    .style(move |_| p.panel_header())
    .into()
}

/// The language of the stand-alone layout (`--screenshot`): English, Dutch with `O3V_LANG=nl`.
pub(crate) fn standalone_language() -> Language {
    match std::env::var("O3V_LANG").as_deref() {
        Ok("nl") => Language::Dutch,
        _ => Language::English,
    }
}

/// A side panel of the stand-alone layout, as wide as the shell's pane (explorer 220, properties
/// 290), so `--screenshot` shows what the window shows.
fn panel<'a>(p: Palette, header: Element<'a, Message>, content: Element<'a, Message>, width: u32) -> Element<'a, Message> {
    container(column![header, container(content).padding(10).height(Fill)])
        .width(width)
        .height(Fill)
        .style(move |_| p.panel())
        .into()
}

pub fn format_count(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

fn format_bytes(n: u64) -> String {
    match n {
        0..1024 => format!("{n} B"),
        1024..1_048_576 => format!("{:.1} KB", n as f64 / 1024.0),
        _ => format!("{:.1} MB", n as f64 / 1_048_576.0),
    }
}

fn format_vec(v: [f32; 3]) -> String {
    format!("{:.2}, {:.2}, {:.2}", v[0], v[1], v[2])
}

impl Palette {
    fn dim(c: iced::Color) -> iced::Color {
        iced::Color { a: c.a * 0.7, ..c }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simulator(app: &App) -> iced_test::Simulator<'_, Message> {
        iced_test::Simulator::with_size(iced::Settings::default(), iced::Size::new(1400.0, 900.0), app.view())
    }

    #[test]
    fn ribbon_buttons_send_messages() {
        let mut app = App::default();
        app.set_model(two_squares());
        let mut ui = simulator(&app);
        ui.click("Wireframe").expect("Wireframe button");
        ui.click("Grid").expect("Grid button");
        ui.click("Save as").expect("Save as button");
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(matches!(messages[0], Message::Mode(RenderMode::Wireframe)), "{messages:?}");
        assert!(matches!(messages[1], Message::ToggleGrid), "{messages:?}");
        assert!(matches!(messages[2], Message::SaveDialog), "{messages:?}");
    }

    #[test]
    fn dragging_and_scrolling_the_viewport_navigates() {
        use iced::mouse;
        let mut app = App::default();
        app.status = Status::Loaded(String::new()); // no overlay over the viewport
        app.model = None;
        let mut ui = simulator(&app);
        let start = iced::Point::new(700.0, 300.0);
        ui.point_at(start);
        ui.simulate([iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))]);
        ui.point_at(iced::Point::new(740.0, 310.0));
        ui.simulate([iced::Event::Mouse(mouse::Event::CursorMoved { position: iced::Point::new(740.0, 310.0) })]);
        ui.simulate([iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))]);
        ui.simulate([iced::Event::Mouse(mouse::Event::WheelScrolled { delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 } })]);
        let nav: Vec<Navigate> = ui
            .into_messages()
            .filter_map(|m| match m {
                Message::Navigate(n) => Some(n),
                _ => None,
            })
            .collect();
        assert!(nav.iter().any(|n| matches!(n, Navigate::Orbit { dx, dy } if *dx == 40.0 && *dy == 10.0)), "{nav:?}");
        assert!(nav.iter().any(|n| matches!(n, Navigate::Zoom { steps, .. } if *steps == 1.0)), "{nav:?}");
    }

    #[test]
    fn picks_the_nearest_triangle_and_its_element() {
        use crate::model::Layer;
        // two unit squares facing +Z, at z = 0 (element 10) and z = 1 (element 20)
        let mut positions = Vec::new();
        for z in [0.0, 1.0] {
            positions.extend_from_slice(&[0.0, 0.0, z, 1.0, 0.0, z, 1.0, 1.0, z, 0.0, 1.0, z]);
        }
        let indices = vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7];
        let layer = Layer {
            name: "Walls".into(),
            positions,
            indices,
            elements: vec![(10, 0, 6), (20, 6, 6)],
            props: vec![(20, r#"{"id":"20","category":"Walls","level":"00 begane grond"}"#.into())],
            ..Layer::default()
        };
        let model = Model::new(PathBuf::from("x.rvt"), "Revit", vec![layer]);
        let still = |_: usize, _: u32| Vec3::ZERO;
        let hit = pick_triangle(&model, &[true], Vec3::new(0.3, 0.6, 5.0), Vec3::new(0.0, 0.0, -1.0), &still, &|_| true);
        let (l, t) = hit.expect("hit");
        assert_eq!(model.layers[l].element_at(t), Some(20));
        assert_eq!(model.element_ranges(20), vec![(0, 6, 6)]);
        let props = model.element_props(20);
        assert!(props.contains(&("level".to_string(), "00 begane grond".to_string())), "{props:?}");
        assert!(pick_triangle(&model, &[false], Vec3::new(0.3, 0.6, 5.0), Vec3::new(0.0, 0.0, -1.0), &still, &|_| true).is_none());
        assert!(pick_triangle(&model, &[true], Vec3::new(3.0, 0.6, 5.0), Vec3::new(0.0, 0.0, -1.0), &still, &|_| true).is_none());
        // a section box below z = 0.5 cuts the upper square away: the lower one is hit
        let below = |p: Vec3| p.z < 0.5;
        let (l, t) = pick_triangle(&model, &[true], Vec3::new(0.3, 0.6, 5.0), Vec3::new(0.0, 0.0, -1.0), &still, &below).expect("hit");
        assert_eq!(model.layers[l].element_at(t), Some(10));
    }

    /// Two unit squares facing +Z side by side: element 10 at x 0..1, element 20 at x 2..3.
    fn two_squares() -> Arc<Model> {
        use crate::model::Layer;
        let mut positions = Vec::new();
        for x in [0.0, 2.0] {
            positions.extend_from_slice(&[x, 0.0, 0.0, x + 1.0, 0.0, 0.0, x + 1.0, 1.0, 0.0, x, 1.0, 0.0]);
        }
        let layer = Layer {
            name: "Walls".into(),
            positions,
            indices: vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7],
            elements: vec![(10, 0, 6), (20, 6, 6)],
            ..Layer::default()
        };
        Arc::new(Model::new(PathBuf::from("x.rvt"), "Revit", vec![layer]))
    }

    #[test]
    fn the_exploded_view_is_picked_and_framed_where_it_is_drawn() {
        let mut app = App::default();
        app.set_model(two_squares());
        let model = app.model.clone().unwrap();
        let _ = app.update(Message::ToggleExplode);
        let _ = app.update(Message::Explode(0, 100.0));
        // no levels: elements mode; the middle is x 1.5, so the squares move by -2 and +2
        let down = Vec3::new(0.0, 0.0, -1.0);
        let shift = |l: usize, i: u32| app.explode_offset(l, i);
        let hit = pick_triangle(&model, &[true], Vec3::new(4.5, 0.5, 5.0), down, &shift, &|_| true);
        assert_eq!(hit.map(|(l, t)| model.layers[l].element_at(t)), Some(Some(20)));
        assert!(pick_triangle(&model, &[true], Vec3::new(2.5, 0.5, 5.0), down, &shift, &|_| true).is_none());
        let b = app.world_bounds().unwrap();
        assert_eq!((b.min[0], b.max[0]), (-2.0, 5.0));
        app.selection = model.element_ranges(20);
        let s = app.selection_bounds().unwrap();
        assert_eq!((s.min[0], s.max[0]), (4.0, 5.0));
        // off: back at rest, the amounts kept for the next time
        let _ = app.update(Message::ToggleExplode);
        let b = app.world_bounds().unwrap();
        assert_eq!((b.min[0], b.max[0], app.explode[0]), (0.0, 3.0, 100.0));
        assert!(app.explode_offsets.iter().all(|o| *o == [0.0; 4]));
    }

    #[test]
    fn explode_controls_send_messages() {
        let mut app = App::default();
        app.set_model(two_squares());
        let _ = app.update(Message::ToggleExplode);
        let _ = app.update(Message::Explode(2, 40.0));
        let mut ui = simulator(&app);
        // no levels: the storeys button is disabled
        ui.click("Storeys").expect("Storeys button");
        ui.click("Elements").expect("Elements button");
        ui.click("Reset").expect("Reset button");
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(matches!(messages[..], [Message::ExplodeMode(ExplodeMode::Elements), Message::ResetExplode]), "{messages:?}");
    }

    #[test]
    fn measures_between_two_clicked_corners() {
        use crate::model::Layer;
        let layer = Layer {
            name: "Plate".into(),
            positions: vec![0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 2.0, 1.0, 0.0, 0.0, 1.0, 0.0],
            indices: vec![0, 1, 2, 0, 2, 3],
            ..Layer::default()
        };
        let mut app = App::default();
        app.set_model(Arc::new(Model::new(PathBuf::from("x.obj"), "OBJ", vec![layer])));
        app.camera.set_preset(ViewPreset::Top);
        app.fit();
        let (w, h) = app.viewport_size;
        let vp = app.camera.view_proj(w / h);
        let screen = |p: Vec3| {
            let n = vp.project_point3(p);
            ((n.x + 1.0) * 0.5 * w, (1.0 - n.y) * 0.5 * h)
        };
        let _ = app.update(Message::Measure(Tool::Distance));
        // a few pixels off the corners: they snap
        for (corner, off) in [(Vec3::new(0.0, 0.0, 0.0), 0.01), (Vec3::new(2.0, 1.0, 0.0), -0.01)] {
            let (x, y) = screen(corner + Vec3::new(off, off, 0.0));
            let _ = app.update(Message::Navigate(Navigate::Pick { x, y, width: w, height: h }));
        }
        assert_eq!(app.measure.results.len(), 1);
        let (_, d) = app.measure.results[0].value();
        assert!((d - 5f64.sqrt()).abs() < 1e-5, "{d}");
        assert!(app.selected.is_none(), "measuring does not select");
        let _ = app.update(Message::Escape);
        assert!(app.measure.tool.is_none());
    }

    #[test]
    fn load_fraction_grows_and_stays_below_one() {
        use loaders::Phase;
        let mut last = 0.0;
        for (phase, t) in [(Phase::Read, 0.0), (Phase::Read, 5.0), (Phase::Convert, 0.0), (Phase::Convert, 60.0), (Phase::Scene, 9.0)] {
            let f = load_fraction(phase, t, true);
            assert!(f >= last && f < 1.0, "{phase:?} {t}: {f}");
            last = f;
        }
    }

    #[test]
    fn the_section_box_takes_over_a_level_or_upright_plane() {
        use crate::model::Layer;
        // a unit cube's top and bottom
        let layer = Layer {
            name: "Cube".into(),
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            indices: vec![0, 1, 2, 3, 4, 5],
            ..Layer::default()
        };
        let mut app = App::default();
        app.set_model(Arc::new(Model::new(PathBuf::from("x.obj"), "OBJ", vec![layer])));
        app.plane = Some(SectionPlane { normal: Vec3::Z, offset: 0.4 });
        let _ = app.update(Message::ToggleSection);
        let b = app.active_section().expect("box on");
        assert!(app.plane.is_none());
        assert!((b.max.z - 0.4).abs() < 1e-6, "{b:?}");
        // a sloped plane stays next to the box
        let _ = app.update(Message::ToggleSection);
        app.plane = Some(SectionPlane { normal: Vec3::new(0.0, 0.6, 0.8), offset: 0.4 });
        let _ = app.update(Message::ToggleSection);
        assert!(app.active_section().is_some() && app.plane.is_some());
    }

    #[test]
    fn reset_puts_the_section_plane_back_where_it_was_placed() {
        use crate::model::Layer;
        let layer = Layer {
            name: "Cube".into(),
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            indices: vec![0, 1, 2, 3, 4, 5],
            ..Layer::default()
        };
        let mut app = App::default();
        app.set_model(Arc::new(Model::new(PathBuf::from("x.obj"), "OBJ", vec![layer])));
        let placed = SectionPlane { normal: Vec3::new(0.0, 0.6, 0.8), offset: 0.4 };
        (app.plane, app.plane_placed) = (Some(placed), Some(placed));
        let _ = app.update(Message::PlaneMove(0.2));
        let _ = app.update(Message::FlipPlane);
        assert_ne!(app.plane, Some(placed));
        let _ = app.update(Message::ResetSection);
        assert_eq!(app.plane, Some(placed));
        assert!(!app.plane_picking);
    }

    #[test]
    fn counts_are_grouped() {
        assert_eq!(format_count(1234567), "1 234 567");
        assert_eq!(format_count(12), "12");
    }
}
