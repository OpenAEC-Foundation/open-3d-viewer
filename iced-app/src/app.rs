//! Application state, messages and the main window layout:
//! ribbon on top, layers left, 3D viewport centre, properties right, status bar bottom.

use crate::icons;
use crate::loaders;
use crate::model::Model;
use crate::theme::{srgb3, srgb4, Palette, ThemeKind};
use crate::viewport::camera::{Camera, ViewPreset};
use crate::viewport::render::{transform_bounds, RenderMode, RenderSettings};
use crate::viewport::{Navigate, Viewport3D};
use glam::Mat4;
use iced::alignment::Vertical;
use iced::widget::{
    button, center, column, container, row, rule, scrollable, shader, stack, svg, text, text_input, Space,
};
use iced::{event, keyboard, window, Center, Element, Fill, Font, Length, Subscription, Task, Theme};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

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
    Loading { file: String, since: Instant },
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
}

#[derive(Debug, Clone)]
pub enum Message {
    OpenDialog,
    Open(PathBuf),
    Picked(Option<PathBuf>),
    Loaded(Result<Arc<Model>, String>),
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
    TakeShot,
    Shot(window::Screenshot),
}

impl From<Navigate> for Message {
    fn from(n: Navigate) -> Self {
        Message::Navigate(n)
    }
}

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
        }
    }
}

/// Loads a file on a worker thread.
pub fn load_task(path: PathBuf) -> Task<Message> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(loaders::load(&path).map(Arc::new));
            });
            rx.await.unwrap_or_else(|_| Err("loader thread stopped".to_string()))
        },
        Message::Loaded,
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

    fn model_matrix(&self) -> Mat4 {
        match (&self.model, self.flip_up) {
            (Some(m), true) if m.source_y_up => Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2),
            (Some(_), true) => Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2),
            _ => Mat4::IDENTITY,
        }
    }

    fn aspect(&self) -> f32 {
        self.viewport_size.0 / self.viewport_size.1.max(1.0)
    }

    pub fn fit(&mut self) {
        if let Some(m) = &self.model {
            let b = transform_bounds(&m.bounds, &self.model_matrix());
            self.camera.fit(&b, self.aspect());
        }
    }

    /// Installs a freshly loaded model (also used by the headless screenshot path).
    pub fn set_model(&mut self, model: Arc<Model>) {
        self.visible = vec![true; model.layers.len()];
        self.status = Status::Loaded(format!(
            "Loaded {} in {:.2} s",
            model.file_name(),
            model.load_time.as_secs_f32()
        ));
        self.model = Some(model);
        self.flip_up = false;
        self.layer_filter.clear();
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

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenDialog => {
                let mut exts = loaders::supported_extensions();
                exts.sort_unstable();
                return Task::perform(
                    async move {
                        rfd::AsyncFileDialog::new()
                            .set_title("Open 3D model")
                            .add_filter("3D models", &exts)
                            .add_filter("All files", &["*"])
                            .pick_file()
                            .await
                            .map(|f| f.path().to_path_buf())
                    },
                    Message::Picked,
                );
            }
            Message::Picked(None) => {}
            Message::Picked(Some(path)) | Message::Open(path) => {
                if matches!(self.status, Status::Loading { .. }) {
                    return Task::none();
                }
                let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.status = Status::Loading { file, since: Instant::now() };
                return load_task(path);
            }
            Message::Loaded(Ok(model)) => {
                self.set_model(model);
                return self.shot_after_delay();
            }
            Message::Loaded(Err(e)) => {
                self.status = Status::Error(e);
                return self.shot_after_delay();
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
                self.fit();
            }
            Message::Theme(t) => self.theme = t,
            Message::Tab(t) => self.tab = t,
            Message::Preset(p) => {
                self.camera.set_preset(p);
                self.fit();
            }
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
                Navigate::Orbit { dx, dy } => self.camera.orbit(dx, dy),
                Navigate::Pan { dx, dy, height } => self.camera.pan(dx, dy, height),
                Navigate::Zoom { steps } => self.camera.zoom(steps),
                Navigate::Resized { width, height } => self.viewport_size = (width, height),
            },
        }
        Task::none()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        event::listen_with(|event, status, _window| match event {
            iced::Event::Window(window::Event::FileDropped(path)) => Some(Message::Open(path)),
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. })
                if status == event::Status::Ignored =>
            {
                use keyboard::key::Named;
                use keyboard::Key;
                match key.as_ref() {
                    Key::Character("o") if modifiers.command() => Some(Message::OpenDialog),
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
                    _ => None,
                }
            }
            _ => None,
        })
    }

    pub fn view(&self) -> Element<'_, Message> {
        let p = self.theme.palette();
        let body = row![]
            .push(self.show_layers.then(|| self.layers_panel(p)))
            .push(self.viewport(p))
            .push(self.show_properties.then(|| self.properties_panel(p)))
            .height(Fill);
        column![self.ribbon(p), body, self.status_bar(p)].into()
    }

    // ---- ribbon ---------------------------------------------------------------------------

    fn ribbon(&self, p: Palette) -> Element<'_, Message> {
        let tab = |label: &'static str, t: RibbonTab| {
            let active = self.tab == t;
            button(text(label).size(12).font(if active { FONT_BOLD } else { FONT }))
                .padding([4, 14])
                .style(move |_, s| p.ribbon_button(active, s))
                .on_press(Message::Tab(t))
        };
        let file_tab = button(text("Open").size(12).font(FONT_BOLD))
            .padding([4, 14])
            .style(move |_, s| p.primary_button(s))
            .on_press(Message::OpenDialog);
        let tabs = row![
            file_tab,
            tab("Home", RibbonTab::Home),
            tab("View", RibbonTab::View),
            Space::new().width(Fill),
            text("Open 3D Viewer Studio").size(12).font(FONT_BOLD).color(p.accent),
            text(" · iced").size(12).color(p.text_muted),
        ]
        .spacing(4)
        .align_y(Center)
        .padding([4, 8]);

        let content: Element<'_, Message> = match self.tab {
            RibbonTab::Home => row![
                group(p, "File", row![big_button(p, icons::OPEN, "Import", Message::OpenDialog, false)].into()),
                separator(p),
                group(
                    p,
                    "Render mode",
                    row![
                        big_button(p, icons::SOLID, "Solid", Message::Mode(RenderMode::Solid), self.mode == RenderMode::Solid),
                        big_button(
                            p,
                            icons::WIREFRAME,
                            "Wireframe",
                            Message::Mode(RenderMode::Wireframe),
                            self.mode == RenderMode::Wireframe
                        ),
                        big_button(
                            p,
                            icons::EDGES,
                            "Edges",
                            Message::Mode(RenderMode::SolidEdges),
                            self.mode == RenderMode::SolidEdges
                        ),
                    ]
                    .spacing(2)
                    .into()
                ),
                separator(p),
                group(
                    p,
                    "Display",
                    row![
                        big_button(p, icons::GRID, "Grid", Message::ToggleGrid, self.grid),
                        big_button(p, icons::UP_AXIS, "Flip up", Message::ToggleFlipUp, self.flip_up),
                    ]
                    .spacing(2)
                    .into()
                ),
                separator(p),
                group(
                    p,
                    "Theme",
                    row(ThemeKind::ALL.map(|t| {
                        let active = self.theme == t;
                        let swatch = container(Space::new().width(14).height(14))
                            .style(move |_| Palette::swatch(t.palette().viewport));
                        button(column![swatch, text(t.label()).size(11)].spacing(4).align_x(Center))
                            .padding([6, 6])
                            .width(64)
                            .style(move |_, s| p.ribbon_button(active, s))
                            .on_press(Message::Theme(t))
                            .into()
                    }))
                    .spacing(2)
                    .into()
                ),
            ]
            .into(),
            RibbonTab::View => row![
                group(
                    p,
                    "Camera",
                    row![
                        big_button(p, icons::FIT, "Fit", Message::Fit, false),
                        big_button(p, icons::VIEW_ISO, "Iso", Message::Preset(ViewPreset::Iso), false),
                        big_button(p, icons::VIEW_TOP, "Top", Message::Preset(ViewPreset::Top), false),
                        big_button(p, icons::VIEW_FRONT, "Front", Message::Preset(ViewPreset::Front), false),
                        big_button(p, icons::VIEW_RIGHT, "Right", Message::Preset(ViewPreset::Right), false),
                    ]
                    .spacing(2)
                    .into()
                ),
                separator(p),
                group(
                    p,
                    "Projection",
                    row![
                        big_button(p, icons::PERSPECTIVE, "Perspective", Message::Ortho(false), !self.camera.ortho),
                        big_button(p, icons::ORTHO, "Orthographic", Message::Ortho(true), self.camera.ortho),
                    ]
                    .spacing(2)
                    .into()
                ),
                separator(p),
                group(
                    p,
                    "Panels",
                    row![
                        big_button(p, icons::LAYERS, "Layers", Message::ToggleLayersPanel, self.show_layers),
                        big_button(p, icons::INFO, "Properties", Message::TogglePropertiesPanel, self.show_properties),
                    ]
                    .spacing(2)
                    .into()
                ),
            ]
            .into(),
        };

        column![
            container(tabs).width(Fill).style(move |_| p.title_bar()),
            container(content).width(Fill).height(84).padding([4, 6]).style(move |_| p.ribbon()),
        ]
        .into()
    }

    // ---- panels ---------------------------------------------------------------------------

    fn layers_panel(&self, p: Palette) -> Element<'_, Message> {
        let header = panel_header(p, "Layers", self.model.as_ref().map(|m| m.layers.len().to_string()));
        let Some(model) = &self.model else {
            return panel(p, header, text("No model loaded").size(12).color(p.text_muted).into());
        };
        let filter = self.layer_filter.to_lowercase();
        let mut order: Vec<usize> = (0..model.layers.len())
            .filter(|&i| filter.is_empty() || model.layers[i].name.to_lowercase().contains(&filter))
            .collect();
        order.sort_by(|&a, &b| model.layers[a].name.to_lowercase().cmp(&model.layers[b].name.to_lowercase()));

        let rows = column(order.into_iter().map(|i| {
            let layer = &model.layers[i];
            let visible = self.visible.get(i).copied().unwrap_or(true);
            let [r, g, b] = layer.color;
            let swatch = container(Space::new().width(12).height(12))
                .style(move |_| Palette::swatch(iced::Color::from_rgb(r, g, b)));
            let eye = svg(icons::handle(if visible { icons::EYE } else { icons::EYE_OFF }))
                .width(16)
                .height(16)
                .style(move |_, _| svg::Style { color: Some(if visible { p.text_secondary } else { p.text_muted }) });
            let name_color = if visible { p.text } else { p.text_muted };
            button(
                row![
                    eye,
                    swatch,
                    text(layer.name.as_str()).size(12).color(name_color).width(Fill),
                    text(format_count(layer.triangle_count())).size(11).color(p.text_muted),
                ]
                .spacing(8)
                .align_y(Center),
            )
            .padding([3, 6])
            .width(Fill)
            .style(move |_, s| p.row_button(s))
            .on_press(Message::Layer(i, !visible))
            .into()
        }))
        .spacing(1);

        let small = |label: &'static str, msg: Message| {
            button(text(label).size(11)).padding([2, 8]).style(move |_, s| p.ribbon_button(false, s)).on_press(msg)
        };
        let tools = column![
            text_input("Filter layers…", &self.layer_filter)
                .on_input(Message::LayerFilter)
                .size(12)
                .padding([4, 8]),
            row![small("Show all", Message::AllLayers(true)), small("Hide all", Message::AllLayers(false))].spacing(4),
        ]
        .spacing(6);

        panel(
            p,
            header,
            column![tools, scrollable(rows).height(Fill).spacing(4)].spacing(8).into(),
        )
    }

    fn properties_panel(&self, p: Palette) -> Element<'_, Message> {
        let header = panel_header(p, "Properties", None);
        let Some(model) = &self.model else {
            return panel(p, header, text("Open a model to see its properties").size(12).color(p.text_muted).into());
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
        let section = |title: &'static str| text(title).size(11).font(FONT_BOLD).color(p.group_label);
        let prop = |k: &'static str, v: String| -> Element<'_, Message> {
            row![
                text(k).size(12).color(p.text_secondary).width(84),
                text(v).size(12).color(p.text).width(Fill),
            ]
            .spacing(6)
            .into()
        };
        let content = column![
            section("FILE"),
            prop("Name", model.file_name()),
            prop("Format", model.format.clone()),
            prop("Size", format_bytes(model.file_size)),
            prop("Folder", model.path.parent().map(|d| d.display().to_string()).unwrap_or_default()),
            prop("Load time", format!("{:.2} s", model.load_time.as_secs_f32())),
            rule::horizontal(1),
            section("GEOMETRY"),
            prop("Layers", model.layers.len().to_string()),
            prop("Vertices", format_count(model.vertex_count())),
            prop("Triangles", format_count(model.triangle_count())),
            prop("Visible", format_count(visible_tris)),
            rule::horizontal(1),
            section("BOUNDS (Z UP)"),
            prop("Min", format_vec(b.min)),
            prop("Max", format_vec(b.max)),
            prop("Size", format_vec(size)),
            prop("Up axis", if model.source_y_up != self.flip_up { "Y (converted)".into() } else { "Z".into() }),
        ]
        .spacing(6);
        panel(p, header, scrollable(content).height(Fill).into())
    }

    // ---- viewport -------------------------------------------------------------------------

    fn viewport(&self, p: Palette) -> Element<'_, Message> {
        let settings = RenderSettings {
            mode: self.mode,
            grid: self.grid,
            background: srgb3(p.viewport),
            grid_minor: srgb4(p.grid_minor),
            grid_major: srgb4(p.grid_major),
            model_matrix: self.model_matrix(),
        };
        let scene = shader(Viewport3D { model: self.model.as_ref(), visible: &self.visible, camera: self.camera, settings })
            .width(Fill)
            .height(Fill);

        let overlay: Option<Element<'_, Message>> = match (&self.model, &self.status) {
            (_, Status::Loading { file, .. }) => Some(
                center(
                    container(
                        column![
                            text("Loading").size(12).color(p.text_muted),
                            text(file.as_str()).size(16).font(FONT_BOLD),
                        ]
                        .spacing(4)
                        .align_x(Center),
                    )
                    .padding([16, 28])
                    .style(move |_| p.overlay()),
                )
                .into(),
            ),
            (None, _) => Some(center(self.welcome(p)).into()),
            _ => None,
        };
        let hint = container(
            text("Left drag: orbit · Right drag: pan · Wheel: zoom · F: fit · 1/2/3/0: views · P/O: perspective/ortho")
                .size(11)
                .color(p.text_muted),
        )
        .padding(8)
        .align_bottom(Fill)
        .align_left(Fill);
        stack![scene, hint].push(overlay).width(Fill).height(Fill).into()
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
            Status::Loading { file, since } => {
                (format!("Loading {file}… {:.0} s", since.elapsed().as_secs_f32()), p.accent)
            }
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

// ---- small view helpers ---------------------------------------------------------------------

fn big_button<'a>(p: Palette, icon: &'static str, label: &'static str, msg: Message, active: bool) -> Element<'a, Message> {
    let tint = if active { p.accent } else { p.text };
    button(
        column![
            svg(icons::handle(icon))
                .width(24)
                .height(24)
                .style(move |_, status| svg::Style {
                    color: Some(if matches!(status, svg::Status::Hovered) { p.accent } else { tint })
                }),
            text(label).size(11),
        ]
        .spacing(4)
        .align_x(Center),
    )
    .padding([6, 8])
    .width(Length::Shrink)
    .style(move |_, s| p.ribbon_button(active, s))
    .on_press(msg)
    .into()
}

fn group<'a>(p: Palette, label: &'static str, content: Element<'a, Message>) -> Element<'a, Message> {
    column![
        container(content).height(Fill).align_y(Center),
        text(label).size(10).color(p.group_label).font(FONT_BOLD),
    ]
    .spacing(2)
    .padding([0, 6])
    .align_x(Center)
    .into()
}

fn separator<'a>(p: Palette) -> Element<'a, Message> {
    container(rule::vertical(1).style(move |_| rule::Style {
        color: p.border,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }))
    .padding([6, 2])
    .height(Fill)
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

fn panel<'a>(p: Palette, header: Element<'a, Message>, content: Element<'a, Message>) -> Element<'a, Message> {
    container(column![header, container(content).padding(10).height(Fill)])
        .width(270)
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
        let app = App::default();
        let mut ui = simulator(&app);
        ui.click("Wireframe").expect("Wireframe button");
        ui.click("Grid").expect("Grid button");
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(matches!(messages[0], Message::Mode(RenderMode::Wireframe)), "{messages:?}");
        assert!(matches!(messages[1], Message::ToggleGrid), "{messages:?}");
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
        assert!(nav.iter().any(|n| matches!(n, Navigate::Zoom { steps } if *steps == 1.0)), "{nav:?}");
    }

    #[test]
    fn counts_are_grouped() {
        assert_eq!(format_count(1234567), "1 234 567");
        assert_eq!(format_count(12), "12");
    }
}
