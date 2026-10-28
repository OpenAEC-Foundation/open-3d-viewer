//! The window shell from the shared OpenAEC UI crate (`openaec-ui`, as in Open Pointcloud
//! Studio): title strip, ribbon with the File backstage, explorer and properties panes, document
//! band, status bar, settings dialog and the five OpenAEC themes. `Studio` implements
//! `openaec_ui::Application` around the viewer state (`App`); the viewport is the workspace.

use crate::app::{App, Message, Status};
use crate::theme::ThemeKind;
use crate::viewport::camera::ViewPreset;
use crate::viewport::render::RenderMode;
use openaec_ui::icons::{DomainIcon, Icon};
use openaec_ui::iced::{Element, Subscription, Task};
use openaec_ui::{Application, Command, Language, RibbonGroup, RibbonTab, Theme};

/// Version: OpenAEC calendar versioning (year.ISO week.patch, as `openaec-ui` and Pointcloud).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct Studio {
    pub app: App,
}

/// The viewport palette for an OpenAEC theme (the five shell themes onto the four viewport ones).
pub fn viewport_theme(theme: Theme) -> ThemeKind {
    match theme {
        Theme::Light => ThemeKind::Light,
        Theme::Forge => ThemeKind::Forge,
        Theme::OpenAec | Theme::Contrast => ThemeKind::Night,
        Theme::Blueprint => ThemeKind::Blueprint,
    }
}

impl Application for Studio {
    type Command = Message;

    fn title(&self) -> String {
        match &self.app.model {
            Some(m) => format!("{} - Open 3D Viewer Studio {VERSION}", m.file_name()),
            None => format!("Open 3D Viewer Studio {VERSION}"),
        }
    }

    fn ribbon(&self, l: Language) -> Vec<RibbonTab<Message>> {
        let app = &self.app;
        let loaded = app.model.is_some();
        vec![
            RibbonTab {
                id: "home".into(),
                label: l.text("Start", "Home").into(),
                groups: vec![
                    RibbonGroup {
                        label: l.text("Bestand", "File").into(),
                        commands: vec![Command::new(l.text("Openen", "Open"), Icon::Open, Message::OpenDialog).shortcut("Ctrl+O")],
                    },
                    RibbonGroup {
                        label: l.text("Weergave", "Display").into(),
                        commands: vec![
                            Command::new(l.text("Volledig", "Solid"), Icon::Cube, Message::Mode(RenderMode::Solid))
                                .selected(app.mode == RenderMode::Solid)
                                .shortcut("S"),
                            Command::new(l.text("Draadmodel", "Wireframe"), Icon::Grid, Message::Mode(RenderMode::Wireframe))
                                .selected(app.mode == RenderMode::Wireframe)
                                .shortcut("W"),
                            Command::new(l.text("Randen", "Edges"), Icon::Controls, Message::Mode(RenderMode::SolidEdges))
                                .selected(app.mode == RenderMode::SolidEdges)
                                .shortcut("E"),
                        ],
                    },
                    RibbonGroup {
                        label: l.text("Tonen", "Show").into(),
                        commands: vec![
                            Command::new(l.text("Raster", "Grid"), Icon::Grid, Message::ToggleGrid).selected(app.grid).shortcut("G"),
                            Command::new(l.text("Stramien", "Grid lines"), Icon::Measure, Message::ToggleGrids)
                                .selected(app.show_grids)
                                .enabled(app.model.as_ref().is_some_and(|m| !m.project.grids.is_empty())),
                            Command::new(l.text("Z-as omdraaien", "Flip up"), Icon::Rename, Message::ToggleFlipUp)
                                .selected(app.flip_up)
                                .enabled(loaded),
                        ],
                    },
                ],
            },
            RibbonTab {
                id: "view".into(),
                label: l.text("Beeld", "View").into(),
                groups: vec![
                    RibbonGroup {
                        label: l.text("Camera", "Camera").into(),
                        commands: vec![
                            Command::new(l.text("Passend", "Fit"), Icon::Fit, Message::Fit).enabled(loaded).shortcut("F"),
                            Command::new(l.text("Iso", "Iso"), Icon::Cube, Message::Preset(ViewPreset::Iso)).shortcut("0"),
                            Command::new(l.text("Boven", "Top"), Icon::Cube, Message::Preset(ViewPreset::Top)).shortcut("1"),
                            Command::new(l.text("Voor", "Front"), Icon::Cube, Message::Preset(ViewPreset::Front)).shortcut("2"),
                            Command::new(l.text("Rechts", "Right"), Icon::Cube, Message::Preset(ViewPreset::Right)).shortcut("3"),
                        ],
                    },
                    RibbonGroup {
                        label: l.text("Doorsnede", "Section").into(),
                        commands: vec![
                            Command::new(l.text("Doorsnedebox", "Section box"), Icon::Cube, Message::ToggleSection)
                                .selected(app.section_on)
                                .enabled(loaded),
                            Command::new(l.text("Doorsnedevlak", "Section plane"), DomainIcon::DrawingTrim, Message::TogglePlane)
                                .selected(app.plane.is_some() || app.plane_picking)
                                .enabled(loaded),
                            Command::new(l.text("Omdraaien", "Flip"), DomainIcon::TransformMirror, Message::FlipPlane)
                                .enabled(app.plane.is_some()),
                            Command::new(l.text("Om selectie", "To selection"), Icon::FitSelection, Message::SectionToSelection)
                                .enabled(!app.selection.is_empty()),
                            Command::new(l.text("Snijvlak vullen", "Fill cut"), Icon::Grid, Message::ToggleSectionFill)
                                .selected(app.section_fill)
                                .enabled(app.section_on || app.plane.is_some()),
                            Command::new(l.text("Herstellen", "Reset"), Icon::Undo, Message::ResetSection)
                                .enabled(app.section_on || app.plane.is_some()),
                        ],
                    },
                    RibbonGroup {
                        label: l.text("Projectie", "Projection").into(),
                        commands: vec![
                            Command::new(l.text("Perspectief", "Perspective"), Icon::ZoomIn, Message::Ortho(false))
                                .selected(!app.camera.ortho)
                                .shortcut("P"),
                            Command::new(l.text("Orthografisch", "Orthographic"), Icon::ZoomOut, Message::Ortho(true))
                                .selected(app.camera.ortho)
                                .shortcut("O"),
                        ],
                    },
                ],
            },
        ]
    }

    fn workspace(&self, theme: Theme, _l: Language) -> Element<'_, Message> {
        self.app.viewport_view(viewport_theme(theme).palette(), _l)
    }

    fn explorer(&self, theme: Theme, _l: Language) -> Element<'_, Message> {
        self.app.layers_body(viewport_theme(theme).palette(), _l)
    }

    fn properties(&self, theme: Theme, _l: Language) -> Element<'_, Message> {
        self.app.properties_body(viewport_theme(theme).palette(), _l)
    }

    fn file_commands(&self, l: Language) -> Vec<Command<Message>> {
        vec![Command::new(l.text("Openen", "Open"), Icon::Open, Message::OpenDialog)]
    }

    fn status(&self, l: Language) -> String {
        match &self.app.status {
            Status::Ready => l.text("Gereed", "Ready").into(),
            Status::Loading { file, since, .. } => format!("{} {file}… {:.0} s", l.text("Laden", "Loading"), since.elapsed().as_secs_f32()),
            Status::Loaded(s) => s.clone(),
            Status::Error(e) => format!("{}: {e}", l.text("Fout", "Error")),
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        self.app.update(message)
    }

    fn subscription(&self) -> Subscription<Message> {
        // the command-line file, once: a subscription that yields Open and ends with it
        let open = match &self.app.pending_open {
            Some(path) => Subscription::run_with(path.clone(), |p: &std::path::PathBuf| {
                openaec_ui::iced::futures::stream::once(std::future::ready(Message::Open(p.clone())))
            }),
            None => Subscription::none(),
        };
        Subscription::batch([self.app.subscription(), open])
    }
}
