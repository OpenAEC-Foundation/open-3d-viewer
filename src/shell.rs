//! The window shell after the OpenAEC Blueprint standard application (`src/bin/blueprint.rs` in
//! OpenAEC-Blueprint 2026.44.1), built from library 0.4 (`openaec_ui::design`): title bar with
//! quick actions and the update notice, ribbon with Bestand, document tabs, explorer and
//! properties with splitters, status bar, settings with themes and own shortcuts, and one command
//! list for buttons, menus and shortcuts. The viewer (`App`) fills the workspace and the panels.

use crate::app::{App, Message, Status};
use crate::b2::{large, small};
use crate::measure::Tool;
use crate::theme::{Palette, ThemeKind};
use crate::viewport::camera::ViewPreset;
use crate::viewport::render::RenderMode;
use openaec_ui::design::chrome::{self as ch, WindowActions, WindowIcons};
use openaec_ui::design::command::{self as cm, Command, Shortcut, ShortcutKey};
use openaec_ui::design::shortcuts::{self as sc, Keymap};
use openaec_ui::design::tiles as t;
use openaec_ui::design::update::{self as upd, Available};
use openaec_ui::design::widgets::{self as w, ButtonKind as K, Density as D, DocumentTab};
use openaec_ui::design::{focus, Look, StatusbarSetting, ThemeId, WorkspaceSetting};
use openaec_ui::iced::widget::{column, container, mouse_area, row, Space};
use openaec_ui::iced::{self, Element, Length, Point, Subscription, Task};
use openaec_ui::keyboard::{KeyPress, Navigation};
use openaec_ui::{fonts, Language};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use web_time::{Duration, Instant};

/// Version: OpenAEC calendar versioning (year.ISO week.patch).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const NAME: &str = "Open 3D Viewer Studio";
/// The id of this app in the installer's catalogue, for the update notice (desktop only).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
const APP_ID: &str = "open-3d-viewer";
const PANEL_MIN: f32 = 190.0;
const PANEL_MAX: f32 = 520.0;
const RECENT: usize = 8;

/// The viewport palette for a theme: the 3D background and grid of the nearest viewport theme.
pub(crate) fn viewport_kind(theme: ThemeId) -> ThemeKind {
    match theme {
        ThemeId::Light | ThemeId::HighContrastLight => ThemeKind::Light,
        ThemeId::Dark | ThemeId::HighContrastDark => ThemeKind::Night,
        ThemeId::Blueprint => ThemeKind::Blueprint,
    }
}

/// The palette the viewer draws its panels and workspace with: the 0.4 tokens of the theme, and
/// the 3D colours of its viewport theme.
pub fn palette(look: Look) -> Palette {
    let c = look.color;
    let kind = viewport_kind(look.theme).palette();
    Palette {
        bg: c.panel,
        surface: c.raised,
        docbar: c.docbar,
        border: c.border,
        text: c.text,
        text_secondary: c.text_secondary,
        text_muted: c.text_disabled,
        accent: c.accent,
        accent_hover: c.accent_hover,
        accent_text: c.accent_text,
        hover: c.hover,
        active: c.selected,
        group_label: c.text_secondary,
        status_bg: c.statusbar,
        status_text: c.statusbar_text,
        ..kind
    }
}

/// The interface size in the settings.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Scale(f64);

impl Scale {
    const ALL: [Scale; 5] = [Scale(0.9), Scale(1.0), Scale(1.1), Scale(1.25), Scale(1.5)];
}

impl std::fmt::Display for Scale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.0} %", self.0 * 100.0)
    }
}

/// A setting with its name in the language of the interface (the library names them in Dutch).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Named<T> {
    value: T,
    label: &'static str,
}

impl<T> std::fmt::Display for Named<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label)
    }
}

const fn named<T>(value: T, label: &'static str) -> Named<T> {
    Named { value, label }
}

/// The settings per language: Dutch, English.
const THEMES: [[Named<ThemeId>; 5]; 2] = [
    [
        named(ThemeId::Light, "Licht"),
        named(ThemeId::Dark, "Donker"),
        named(ThemeId::Blueprint, "Blueprint"),
        named(ThemeId::HighContrastDark, "High contrast donker"),
        named(ThemeId::HighContrastLight, "High contrast licht"),
    ],
    [
        named(ThemeId::Light, "Light"),
        named(ThemeId::Dark, "Dark"),
        named(ThemeId::Blueprint, "Blueprint"),
        named(ThemeId::HighContrastDark, "High contrast dark"),
        named(ThemeId::HighContrastLight, "High contrast light"),
    ],
];
const WORKSPACES: [[Named<WorkspaceSetting>; 3]; 2] = [
    [named(WorkspaceSetting::Theme, "Volgt thema"), named(WorkspaceSetting::Light, "Altijd licht"), named(WorkspaceSetting::Dark, "Altijd donker")],
    [named(WorkspaceSetting::Theme, "Follows the theme"), named(WorkspaceSetting::Light, "Always light"), named(WorkspaceSetting::Dark, "Always dark")],
];
const STATUSBARS: [[Named<StatusbarSetting>; 2]; 2] = [
    [named(StatusbarSetting::Theme, "Volgt thema"), named(StatusbarSetting::Dark, "Altijd donker")],
    [named(StatusbarSetting::Theme, "Follows the theme"), named(StatusbarSetting::Dark, "Always dark")],
];

/// The options of a setting in `language`, and the one that is chosen.
fn options<T: Copy + PartialEq, const N: usize>(lists: &'static [[Named<T>; N]; 2], language: Language, chosen: T) -> (&'static [Named<T>], Option<Named<T>>) {
    let list = &lists[usize::from(language == Language::English)];
    (list, list.iter().find(|n| n.value == chosen).copied())
}

/// What the viewer keeps between sessions (`preferences.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Prefs {
    theme: String,
    workspace: String,
    statusbar: String,
    language: Language,
    scale: f64,
    left: bool,
    right: bool,
    left_width: f32,
    right_width: f32,
    recent: Vec<PathBuf>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            theme: ThemeId::Light.key().into(),
            workspace: WorkspaceSetting::Theme.to_string(),
            statusbar: StatusbarSetting::Theme.to_string(),
            language: Language::English,
            scale: 1.0,
            left: true,
            right: true,
            left_width: 260.0,
            right_width: 280.0,
            recent: Vec::new(),
        }
    }
}

impl Prefs {
    fn load(path: &std::path::Path) -> Self {
        std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    }

    fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        // the whole file first, then replace the old one
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, serde_json::to_vec_pretty(self).unwrap_or_default())?;
        std::fs::rename(temp, path)
    }

    fn theme(&self) -> ThemeId {
        ThemeId::from_key(&self.theme).unwrap_or_default()
    }

    fn workspace(&self) -> WorkspaceSetting {
        WorkspaceSetting::ALL.into_iter().find(|s| s.to_string() == self.workspace).unwrap_or(WorkspaceSetting::Theme)
    }

    fn statusbar(&self) -> StatusbarSetting {
        StatusbarSetting::ALL.into_iter().find(|s| s.to_string() == self.statusbar).unwrap_or(StatusbarSetting::Theme)
    }

    fn scale(&self) -> f64 {
        if self.scale.is_finite() { self.scale.clamp(0.75, 2.0) } else { 1.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialog {
    Settings,
}

/// What a screenshot shows: the workspace, the Bestand page or a page of the settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Workspace,
    File,
    Settings,
    Shortcuts,
}

impl Page {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "workspace" => Page::Workspace,
            "file" => Page::File,
            "settings" => Page::Settings,
            "shortcuts" => Page::Shortcuts,
            _ => return None,
        })
    }
}

/// A theme by its Blueprint key (`light`, `dark`, `blueprint`, `high-contrast-dark`,
/// `high-contrast-light`) or by the name of an earlier viewer theme.
pub fn theme_from_arg(s: &str) -> Option<ThemeId> {
    ThemeId::from_key(s).or(match s.to_ascii_lowercase().as_str() {
        "night" | "forge" => Some(ThemeId::Dark),
        "light" => Some(ThemeId::Light),
        "blueprint" => Some(ThemeId::Blueprint),
        _ => None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsPage {
    General,
    Shortcuts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

#[derive(Debug, Clone)]
pub enum Msg {
    App(Message),
    RibbonTab(usize),
    Backstage(bool),
    OpenRecent(PathBuf),
    Dialog(Option<Dialog>),
    SettingsPage(SettingsPage),
    Shortcuts(sc::Message),
    Theme(ThemeId),
    Workspace(WorkspaceSetting),
    Statusbar(StatusbarSetting),
    Language(Language),
    Scale(f64),
    ToggleLeft,
    ToggleRight,
    SplitterPress(Side),
    SplitterMove(Point),
    SplitterRelease,
    SelectTab(usize),
    CloseTab(usize),
    Key(KeyPress),
    Navigate(Navigation),
    TitlePress,
    Minimize,
    Maximize,
    CloseWindow,
    Update,
}

pub struct Studio {
    pub app: App,
    prefs: Prefs,
    /// Where the preferences and own shortcuts are kept; `None` on the web and in tests.
    prefs_path: Option<PathBuf>,
    keymap_path: Option<PathBuf>,
    keymap: Keymap,
    shortcuts: sc::Editor,
    ribbon_tab: usize,
    backstage: bool,
    dialog: Option<Dialog>,
    settings_page: SettingsPage,
    update: Option<Available>,
    maximized: bool,
    last_title_press: Option<Instant>,
    /// The splitter being dragged, with the cursor and panel width where it started.
    drag: Option<(Side, Option<f32>, f32)>,
}

impl Studio {
    pub fn new(app: App) -> Self {
        Self {
            app,
            prefs: Prefs::default(),
            prefs_path: None,
            keymap_path: None,
            keymap: Keymap::default(),
            shortcuts: sc::Editor::default(),
            ribbon_tab: 0,
            backstage: false,
            dialog: None,
            settings_page: SettingsPage::General,
            update: None,
            maximized: false,
            last_title_press: None,
            drag: None,
        }
    }

    /// Reads the preferences and own shortcuts in `dir`, and keeps them there.
    pub fn with_settings_in(mut self, dir: PathBuf) -> Self {
        let prefs = dir.join("preferences.json");
        self.prefs = Prefs::load(&prefs);
        self.prefs_path = Some(prefs);
        let keymap = dir.join("shortcuts.json");
        self.keymap = Keymap::load(&keymap);
        self.keymap_path = Some(keymap);
        self
    }

    /// Theme and language from the command line, kept until the user changes them; the
    /// preferences file is not written for them.
    pub fn set_look(&mut self, theme: Option<ThemeId>, language: Option<Language>) {
        if let Some(theme) = theme {
            self.prefs.theme = theme.key().into();
        }
        if let Some(language) = language {
            self.prefs.language = language;
        }
    }

    /// Shows a page over the workspace, for `--screenshot`.
    pub fn show(&mut self, page: Page) {
        self.backstage = page == Page::File;
        self.dialog = matches!(page, Page::Settings | Page::Shortcuts).then_some(Dialog::Settings);
        self.settings_page = if page == Page::Shortcuts { SettingsPage::Shortcuts } else { SettingsPage::General };
    }

    /// The size of the 3D view in a window of `width` x `height`, with the panels as they are.
    pub fn viewport_in(&self, width: f32, height: f32) -> (f32, f32) {
        use openaec_ui::design::shell::{DOCUMENT_TABS, RIBBON, RIBBON_TABS, STATUSBAR, TITLEBAR};
        let panels = [(self.prefs.left, self.prefs.left_width), (self.prefs.right, self.prefs.right_width)];
        let side: f32 = panels.iter().filter(|(shown, _)| *shown).map(|(_, w)| w + 4.0).sum();
        let bars = TITLEBAR + RIBBON_TABS + RIBBON + DOCUMENT_TABS + STATUSBAR;
        ((width - side).max(1.0), (height - bars).max(1.0))
    }

    /// Renders the window without opening it (iced_test on an offscreen wgpu device) and writes
    /// a PNG to `out`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn snapshot(&self, size: iced::Size, out: &std::path::Path) -> Result<(), String> {
        let settings = iced::Settings {
            fonts: [crate::INTER_REGULAR, crate::INTER_SEMIBOLD].into_iter().chain(fonts::FILES.iter().copied()).map(std::borrow::Cow::Borrowed).collect(),
            default_font: fonts::BODY,
            antialiasing: true,
            ..Default::default()
        };
        let mut ui = iced_test::Simulator::with_size(settings, size, self.view());
        let snapshot = ui.snapshot(&self.prefs.theme().iced()).map_err(|e| e.to_string())?;
        // iced_test writes "<stem>-<renderer>.png" and keeps one that is there; rename it to OUT
        let stem = out.file_stem().unwrap_or_default().to_string_lossy();
        let written = out.with_file_name(format!("{stem}-wgpu.png"));
        let _ = std::fs::remove_file(&written);
        snapshot.matches_image(out).map_err(|e| e.to_string())?;
        if !written.exists() {
            let _ = std::fs::remove_file(out.with_file_name(format!("{stem}-tiny-skia.png")));
            return Err("no wgpu adapter for the headless renderer (tiny-skia cannot draw the 3D view)".into());
        }
        std::fs::rename(&written, out).map_err(|e| e.to_string())
    }

    fn look(&self) -> Look {
        Look::new(self.prefs.theme(), self.prefs.workspace(), self.prefs.statusbar())
    }

    fn l(&self) -> Language {
        self.prefs.language
    }

    fn save_prefs(&self) {
        if let Some(path) = &self.prefs_path {
            if let Err(e) = self.prefs.save(path) {
                eprintln!("preferences: {e}");
            }
        }
    }

    /// Puts a file at the top of the recent files.
    pub fn remember(&mut self, path: PathBuf) {
        self.prefs.recent.retain(|p| p != &path);
        self.prefs.recent.insert(0, path);
        self.prefs.recent.truncate(RECENT);
        self.save_prefs();
    }

    /// Every command of the viewer with its default shortcut: the ribbon, the title bar, the
    /// Bestand page and the keyboard all use this list.
    fn default_commands(&self) -> Vec<Command<Msg>> {
        let l = self.l();
        let app = &self.app;
        let loaded = app.model.is_some();
        let has_grids = app.model.as_ref().is_some_and(|m| !m.project.grids.is_empty());
        let key = |c: char| Shortcut { key: ShortcutKey::Char(c), ctrl: false, shift: false, alt: false };
        let go = |m: Message| Msg::App(m);
        let mut list = vec![
            Command::new("file.open", l.text("Openen…", "Open…"), go(Message::OpenDialog)).icon(small("file.open"), large("file.open")).shortcut(Shortcut::ctrl('o')).dialog(),
            Command::new("file.save-as", l.text("Opslaan als…", "Save as…"), go(Message::SaveDialog))
                .icon(small("file.save-as"), large("file.save-as"))
                .shortcut(Shortcut::ctrl_shift('s'))
                .enabled(loaded)
                .dialog(),
            Command::new("view.solid", l.text("Volledig", "Solid"), go(Message::Mode(RenderMode::Solid)))
                .icon(small("mesh.surface"), large("mesh.surface"))
                .shortcut(key('s'))
                .mode(app.mode == RenderMode::Solid),
            Command::new("view.wireframe", l.text("Draadmodel", "Wireframe"), go(Message::Mode(RenderMode::Wireframe)))
                .icon(small("mesh.faces"), large("mesh.faces"))
                .shortcut(key('w'))
                .mode(app.mode == RenderMode::Wireframe),
            Command::new("view.edges", l.text("Randen", "Edges"), go(Message::Mode(RenderMode::SolidEdges)))
                .icon(small("mesh.closed"), large("mesh.closed"))
                .shortcut(key('e'))
                .mode(app.mode == RenderMode::SolidEdges),
            Command::new("view.grid", l.text("Raster", "Grid"), go(Message::ToggleGrid)).icon(small("view.grid"), large("view.grid")).shortcut(key('g')).toggle(app.grid),
            Command::new("view.grid-lines", l.text("Stramien", "Grid lines"), go(Message::ToggleGrids))
                .icon(small("measure.distance"), large("measure.distance"))
                .toggle(app.show_grids && has_grids)
                .enabled(has_grids),
            Command::new("view.flip-up", l.text("Z-as omdraaien", "Flip up"), go(Message::ToggleFlipUp))
                .icon(small("transform.flip-vertical"), large("transform.flip-vertical"))
                .toggle(app.flip_up)
                .enabled(loaded),
            Command::new("view.left", l.text("Verkenner", "Explorer"), Msg::ToggleLeft).icon(small("panel.left"), large("panel.left")).shortcut(Shortcut::ctrl('1')).toggle(self.prefs.left),
            Command::new("view.right", l.text("Eigenschappen", "Properties"), Msg::ToggleRight)
                .icon(small("panel.right"), large("panel.right"))
                .shortcut(Shortcut::ctrl('2'))
                .toggle(self.prefs.right),
            Command::new("camera.fit", l.text("Passend", "Fit"), go(Message::Fit)).icon(small("view.fit"), large("view.fit")).shortcut(key('f')).enabled(loaded),
            Command::new("camera.iso", l.text("Iso", "Iso"), go(Message::Preset(ViewPreset::Iso))).icon(small("view.isometric"), large("view.isometric")).shortcut(key('0')),
            Command::new("camera.top", l.text("Boven", "Top"), go(Message::Preset(ViewPreset::Top))).icon(small("view.top"), large("view.top")).shortcut(key('1')),
            Command::new("camera.front", l.text("Voor", "Front"), go(Message::Preset(ViewPreset::Front))).icon(small("view.front"), large("view.front")).shortcut(key('2')),
            Command::new("camera.right", l.text("Rechts", "Right"), go(Message::Preset(ViewPreset::Right))).icon(small("view.right"), large("view.right")).shortcut(key('3')),
            Command::new("camera.perspective", l.text("Perspectief", "Perspective"), go(Message::Ortho(false)))
                .icon(small("view.orbit"), large("view.orbit"))
                .shortcut(key('p'))
                .mode(!app.camera.ortho),
            Command::new("camera.ortho", l.text("Orthografisch", "Orthographic"), go(Message::Ortho(true)))
                .icon(small("view.zoom-out"), large("view.zoom-out"))
                .shortcut(key('o'))
                .mode(app.camera.ortho),
            Command::new("section.box", l.text("Doorsnedebox", "Section box"), go(Message::ToggleSection)).icon(small("section.box"), large("section.box")).toggle(app.section_on).enabled(loaded),
            Command::new("section.plane", l.text("Doorsnedevlak", "Section plane"), go(Message::TogglePlane))
                .icon(small("section.plane-z"), large("section.plane-z"))
                .toggle(app.plane.is_some() || app.plane_picking)
                .enabled(loaded),
            Command::new("section.flip", l.text("Omdraaien", "Flip"), go(Message::FlipPlane)).icon(small("transform.mirror"), large("transform.mirror")).enabled(app.plane.is_some()),
            Command::new("section.selection", l.text("Om selectie", "To selection"), go(Message::SectionToSelection))
                .icon(small("section.fit-selection"), large("section.fit-selection"))
                .enabled(!app.selection.is_empty()),
            Command::new("section.fill", l.text("Snijvlak vullen", "Fill cut"), go(Message::ToggleSectionFill))
                .icon(small("section.clear"), large("section.clear"))
                .toggle(app.section_fill)
                .enabled(app.section_on || app.plane.is_some()),
            Command::new("section.reset", l.text("Herstellen", "Reset"), go(Message::ResetSection))
                .icon(small("section.reset-box"), large("section.reset-box"))
                .enabled(app.section_on || app.plane.is_some()),
            Command::new("view.explode", l.text("Uit elkaar", "Explode"), go(Message::ToggleExplode))
                .icon(small("transform.explode"), large("transform.explode"))
                .toggle(app.explode_on)
                .enabled(loaded),
            Command::new("measure.clear-saved", l.text("Metingen wissen", "Clear measurements"), go(Message::ClearMeasurements))
                .icon(small("edit.clear-all"), large("edit.clear-all"))
                .enabled(!app.measurements.is_empty()),
            Command::new("measure.clear", l.text("Alles wissen", "Clear all"), go(Message::MeasureClear))
                .icon(small("edit.clear-all"), large("edit.clear-all"))
                .enabled(!app.measure.results.is_empty()),
            Command::new("app.settings", l.text("Instellingen…", "Settings…"), Msg::Dialog(Some(Dialog::Settings))).icon(small("ui.settings"), large("ui.settings")).shortcut(Shortcut::ctrl(',')).dialog(),
        ];
        for tool in Tool::ALL {
            let (id, icon) = match tool {
                Tool::Distance => ("measure.distance", "measure.distance"),
                Tool::Angle => ("measure.angle", "measure.angle"),
                Tool::Area => ("measure.area", "measure.area"),
                Tool::Volume => ("measure.volume", "measure.takeoff"),
                Tool::Polygon => ("measure.polygon", "measure.perimeter"),
            };
            list.push(Command::new(id, tool.label(l), go(Message::Measure(tool))).icon(small(icon), large(icon)).mode(app.measure.tool == Some(tool)).enabled(loaded));
        }
        list
    }

    /// The commands with the user's own shortcuts.
    fn commands(&self) -> Vec<Command<Msg>> {
        let mut commands = self.default_commands();
        self.keymap.apply(&mut commands);
        commands
    }

    fn command(&self, id: &str) -> Command<Msg> {
        self.commands().into_iter().find(|c| c.id == id).unwrap_or_else(|| panic!("command {id}"))
    }

    pub fn update(&mut self, message: Msg) -> Task<Msg> {
        match message {
            Msg::App(message) => {
                let opened = match &message {
                    Message::Open(path) => Some(path.clone()),
                    _ => None,
                };
                if opened.is_some() || matches!(message, Message::OpenDialog | Message::SaveDialog) {
                    self.backstage = false;
                }
                if let Some(path) = opened {
                    self.remember(path);
                }
                return self.app.update(message).map(Msg::App);
            }
            Msg::RibbonTab(i) => {
                self.ribbon_tab = i;
                self.backstage = false;
            }
            Msg::Backstage(open) => self.backstage = open,
            Msg::OpenRecent(path) => return self.update(Msg::App(Message::Open(path))),
            Msg::Dialog(dialog) => {
                let opening = dialog.is_some() && self.dialog.is_none();
                self.dialog = dialog;
                if dialog.is_none() {
                    self.shortcuts = sc::Editor::default();
                } else {
                    self.backstage = false;
                }
                if opening {
                    return openaec_ui::keyboard::navigate(Navigation::Next);
                }
            }
            Msg::SettingsPage(page) => self.settings_page = page,
            Msg::Shortcuts(message) => {
                let defaults = self.default_commands();
                if self.shortcuts.update(message, &mut self.keymap, &defaults) {
                    if let Some(path) = &self.keymap_path {
                        if let Err(e) = self.keymap.save(path) {
                            eprintln!("shortcuts: {e}");
                        }
                    }
                }
            }
            Msg::Theme(theme) => {
                self.prefs.theme = theme.key().into();
                self.save_prefs();
            }
            Msg::Workspace(setting) => {
                self.prefs.workspace = setting.to_string();
                self.save_prefs();
            }
            Msg::Statusbar(setting) => {
                self.prefs.statusbar = setting.to_string();
                self.save_prefs();
            }
            Msg::Language(language) => {
                self.prefs.language = language;
                self.save_prefs();
            }
            Msg::Scale(scale) => {
                self.prefs.scale = scale;
                self.save_prefs();
            }
            Msg::ToggleLeft => {
                self.prefs.left = !self.prefs.left;
                self.save_prefs();
            }
            Msg::ToggleRight => {
                self.prefs.right = !self.prefs.right;
                self.save_prefs();
            }
            Msg::SplitterPress(side) => {
                let width = match side {
                    Side::Left => self.prefs.left_width,
                    Side::Right => self.prefs.right_width,
                };
                self.drag = Some((side, None, width));
            }
            Msg::SplitterMove(at) => {
                if let Some((side, start, width)) = &mut self.drag {
                    let from = *start.get_or_insert(at.x);
                    let new = match side {
                        Side::Left => *width + (at.x - from),
                        Side::Right => *width - (at.x - from),
                    }
                    .clamp(PANEL_MIN, PANEL_MAX);
                    match side {
                        Side::Left => self.prefs.left_width = new,
                        Side::Right => self.prefs.right_width = new,
                    }
                }
            }
            Msg::SplitterRelease => {
                if self.drag.take().is_some() {
                    self.save_prefs();
                }
            }
            Msg::SelectTab(i) => {
                let schedule = i.checked_sub(1).and_then(|i| self.app.open_schedules.get(i).copied());
                return self.update(Msg::App(Message::ShowSchedule(schedule)));
            }
            Msg::CloseTab(i) => {
                // the 3D view (tab 0) stays open
                if let Some(&schedule) = i.checked_sub(1).and_then(|i| self.app.open_schedules.get(i)) {
                    return self.update(Msg::App(Message::CloseSchedule(schedule)));
                }
            }
            Msg::Key(press) if self.shortcuts.recording() => return self.update(Msg::Shortcuts(sc::Message::Key(press))),
            Msg::Key(press) => {
                use iced::keyboard::{key::Named, Key};
                if press.key == Key::Named(Named::Escape) {
                    if self.dialog.is_some() {
                        return self.update(Msg::Dialog(None));
                    }
                    if self.backstage {
                        self.backstage = false;
                        return Task::none();
                    }
                    return self.update(Msg::App(Message::Escape));
                }
                if self.dialog.is_none() {
                    if let Some(message) = cm::find(&self.commands(), &press.key, press.modifiers) {
                        return self.update(message);
                    }
                }
            }
            Msg::Navigate(step) => return openaec_ui::keyboard::navigate(step),
            Msg::TitlePress => {
                let now = Instant::now();
                let double = self.last_title_press.is_some_and(|t| now.duration_since(t) < Duration::from_millis(400));
                self.last_title_press = Some(now);
                if double {
                    self.last_title_press = None;
                    return self.update(Msg::Maximize);
                }
                return iced::window::latest().and_then(iced::window::drag);
            }
            Msg::Minimize => return iced::window::latest().and_then(|id| iced::window::minimize(id, true)),
            Msg::Maximize => {
                self.maximized = !self.maximized;
                return iced::window::latest().and_then(iced::window::toggle_maximize);
            }
            // the viewer changes no files, so closing needs no questions
            Msg::CloseWindow => return iced::exit(),
            Msg::Update => {
                if let Some(available) = &self.update {
                    if let Err(e) = upd::open_installer(available) {
                        eprintln!("installer: {e}");
                    } else {
                        return iced::exit();
                    }
                }
            }
        }
        Task::none()
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        // the command-line file, once: a subscription that yields Open and ends with it
        let open = match &self.app.pending_open {
            Some(path) => Subscription::run_with(path.clone(), |p: &PathBuf| iced::futures::stream::once(std::future::ready(Message::Open(p.clone())))),
            None => Subscription::none(),
        };
        let drag = if self.drag.is_some() {
            iced::event::listen_with(|event, _, _| match event {
                iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => Some(Msg::SplitterMove(position)),
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => Some(Msg::SplitterRelease),
                _ => None,
            })
        } else {
            Subscription::none()
        };
        Subscription::batch([
            self.app.subscription().map(Msg::App),
            open.map(Msg::App),
            drag,
            openaec_ui::keyboard::presses().map(Msg::Key),
            openaec_ui::keyboard::navigation().map(Msg::Navigate),
            iced::window::close_requests().map(|_| Msg::CloseWindow),
        ])
    }

    pub fn title(&self) -> String {
        match &self.app.model {
            Some(m) => format!("{} · {NAME}", m.file_name()),
            None => NAME.into(),
        }
    }

    fn status_text(&self) -> String {
        let l = self.l();
        match &self.app.status {
            Status::Ready => l.text("Gereed", "Ready").into(),
            Status::Loading { file, since, .. } => format!("{} {file}… {:.0} s", l.text("Laden", "Loading"), since.elapsed().as_secs_f32()),
            Status::Saving { file, since } => format!("{} {file}… {:.0} s", l.text("Opslaan", "Saving"), since.elapsed().as_secs_f32()),
            Status::Loaded(s) => s.clone(),
            Status::Error(e) => format!("{}: {e}", l.text("Fout", "Error")),
        }
    }

    pub fn view(&self) -> Element<'_, Msg> {
        let look = self.look();
        let l = self.l();
        let c = look.color;
        let tabs = [l.text("Start", "Home"), l.text("Beeld", "View"), l.text("Meten", "Measure")];
        let mut left = vec![self.status_text()];
        if !self.app.selection.is_empty() {
            left.push(format!("{} {}", self.app.selection.len(), l.text("geselecteerd", "selected")));
        }
        let screen = column![
            self.title_bar(look),
            w::ribbon_tabs(look, l.text("Bestand", "File"), Some(Msg::Backstage(!self.backstage)), &tabs, self.ribbon_tab, Msg::RibbonTab),
            // Bestand takes the place of the ribbon and the documents until it closes
            if self.backstage { self.backstage_view(look) } else { column![self.ribbon(look), self.document_area(look)].into() },
            w::status_bar(look, left, vec![format!("{NAME} {VERSION}")]),
        ]
        .height(Length::Fill);
        let mut layers = vec![focus::inert(container(screen).style(move |_| container::Style { background: Some(c.frame.into()), ..Default::default() }), self.dialog.is_some())];
        if let Some(dialog) = self.dialog {
            layers.push(mouse_area(w::overlay(look, self.dialog_card(look, dialog))).into());
        }
        iced::widget::Stack::with_children(layers).into()
    }

    fn title_bar(&self, look: Look) -> Element<'_, Msg> {
        let quick = vec![cm::icon_button(look, &self.command("file.open"), 28.0), cm::icon_button(look, &self.command("file.save-as"), 28.0)];
        let mut extra = Vec::new();
        if let Some(available) = &self.update {
            extra.push(container(upd::notice(look, available, Msg::Update)).padding([0, 8]).into());
        }
        extra.push(cm::icon_button(look, &self.command("app.settings"), 28.0));
        let name = self.title();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let window = WindowActions { drag: Msg::TitlePress, minimize: Msg::Minimize, maximize: Msg::Maximize, close: Msg::CloseWindow, maximized: self.maximized };
            let icons = WindowIcons { minimize: small("window.minimize"), maximize: small("window.maximize"), restore: small("window.restore"), close: small("window.close") };
            ch::title_bar(look, name, quick, extra, window, icons)
        }
        // in the browser the page is the window: no drag area and no window buttons
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (WindowActions::<Msg>::clone, WindowIcons::clone);
            let c = look.color;
            let name = container(w::body(look, name)).width(Length::Fill).center_x(Length::Fill);
            container(row(quick).spacing(2).push(name).extend(extra).spacing(2).padding([0, 4]).align_y(iced::Alignment::Center))
                .height(32)
                .width(Length::Fill)
                .style(move |_| container::Style { background: Some(c.frame.into()), ..Default::default() })
                .into()
        }
    }

    fn ribbon(&self, look: Look) -> Element<'_, Msg> {
        let l = self.l();
        let large = |id: &str| cm::ribbon_large(look, &self.command(id));
        let smalls = |ids: &[&str]| -> Element<'_, Msg> { column(ids.iter().map(|id| cm::ribbon_small(look, &self.command(id)))).spacing(2).into() };
        let groups = match self.ribbon_tab {
            0 => vec![
                w::ribbon_group(look, l.text("Bestand", "File"), vec![large("file.open"), large("file.save-as")]),
                w::ribbon_group(look, l.text("Weergave", "Display"), vec![large("view.solid"), large("view.wireframe"), large("view.edges")]),
                w::ribbon_group(look, l.text("Tonen", "Show"), vec![large("view.grid"), smalls(&["view.grid-lines", "view.flip-up"])]),
                w::ribbon_group(look, l.text("Panelen", "Panels"), vec![smalls(&["view.left", "view.right"])]),
            ],
            1 => vec![
                w::ribbon_group(look, l.text("Camera", "Camera"), vec![large("camera.fit"), smalls(&["camera.iso", "camera.top", "camera.front"]), smalls(&["camera.right"])]),
                w::ribbon_group(look, l.text("Projectie", "Projection"), vec![smalls(&["camera.perspective", "camera.ortho"])]),
                w::ribbon_group(look, l.text("Doorsnede", "Section"), vec![large("section.box"), large("section.plane"), smalls(&["section.flip", "section.selection", "section.fill"]), smalls(&["section.reset"])]),
                w::ribbon_group(look, l.text("Uit elkaar", "Explode"), vec![large("view.explode")]),
                w::ribbon_group(look, l.text("Meten", "Measure"), vec![large("measure.clear-saved")]),
            ],
            _ => vec![
                w::ribbon_group(look, l.text("Meten", "Measure"), Tool::ALL.iter().map(|t| large(Self::tool_id(*t))).collect()),
                w::ribbon_group(look, l.text("Metingen", "Measurements"), vec![large("measure.clear")]),
            ],
        };
        w::ribbon(look, groups)
    }

    fn tool_id(tool: Tool) -> &'static str {
        match tool {
            Tool::Distance => "measure.distance",
            Tool::Angle => "measure.angle",
            Tool::Area => "measure.area",
            Tool::Volume => "measure.volume",
            Tool::Polygon => "measure.polygon",
        }
    }

    /// Bestand: open, save as, settings, and the files opened last.
    fn backstage_view(&self, look: Look) -> Element<'_, Msg> {
        let l = self.l();
        let c = look.color;
        let loaded = self.app.model.is_some();
        let nav = column![
            t::file_nav_row(look, small("file.open"), l.text("Openen", "Open"), false, Some(Msg::App(Message::OpenDialog))),
            t::file_nav_row(look, small("file.save-as"), l.text("Opslaan als", "Save as"), false, loaded.then_some(Msg::App(Message::SaveDialog))),
            t::file_nav_row(look, small("ui.settings"), l.text("Instellingen", "Settings"), false, Some(Msg::Dialog(Some(Dialog::Settings)))),
        ]
        .width(260);
        let files: Vec<t::RecentFile<'_, Msg>> = self
            .prefs
            .recent
            .iter()
            .map(|path| t::RecentFile {
                icon: small("file.open"),
                name: path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
                path: path.parent().and_then(|p| p.to_str()).unwrap_or(""),
                date: "",
                on_press: Some(Msg::OpenRecent(path.clone())),
            })
            .collect();
        let recent: Element<'_, Msg> = if files.is_empty() {
            w::secondary(look, l.text("Nog geen bestanden. Hier komen de laatst geopende modellen.", "No files yet. The models you open last appear here.")).into()
        } else {
            t::recent_files(look, files)
        };
        let about = t::info_card(
            look,
            NAME,
            vec![
                (l.text("Versie", "Version"), VERSION.to_string()),
                (l.text("Licentie", "License"), "LGPL-3.0".to_string()),
                ("Blueprint", "OpenAEC UI 0.4".to_string()),
            ],
        );
        let page = column![w::heading(look, l.text("Recente bestanden", "Recent files"), 16.0), recent, Space::new().height(16), about].spacing(8).padding(24).max_width(720);
        container(row![container(nav).height(Length::Fill).style(move |_| w::panel_style(look)), iced::widget::scrollable(page).width(Length::Fill)])
            .height(Length::Fill)
            .style(move |_| container::Style { background: Some(c.panel.into()), ..Default::default() })
            .into()
    }

    fn document_area(&self, look: Look) -> Element<'_, Msg> {
        let l = self.l();
        let p = palette(look);
        let app = &self.app;
        let workspace: Element<'_, Msg> = if app.model.is_none() && !matches!(app.status, Status::Loading { .. }) {
            let open = cm::button(look, &self.command("file.open"), K::Primary, D::Regular);
            container(w::empty_state(
                look,
                large("project.folder"),
                l.text("Geen model geopend", "No model open"),
                l.text(
                    "Open een bestand of sleep het hierheen: IFC, Revit, SketchUp, glTF, OBJ, STL of PLY.",
                    "Open a file or drop it here: IFC, Revit, SketchUp, glTF, OBJ, STL or PLY.",
                ),
                Some(open),
            ))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| container::Style { background: Some(look.color.workspace.into()), ..Default::default() })
            .into()
        } else {
            app.workspace_view(p, l).map(Msg::App)
        };
        // document tabs once a schedule is open: the model (the 3D view, pinned) and the schedules
        let model_name = app.model.as_ref().and_then(|m| m.path.file_name()).and_then(|n| n.to_str()).unwrap_or("3D");
        let mut middle = column![].width(Length::Fill);
        if app.model.is_some() && !app.open_schedules.is_empty() {
            let schedules = app.open_schedules.iter().filter_map(|&i| app.schedules.get(i)).map(|s| DocumentTab::new(&s.name, false));
            let tabs: Vec<DocumentTab<'_>> = std::iter::once(DocumentTab::pinned(model_name)).chain(schedules).collect();
            let active = app.active_schedule.and_then(|s| app.open_schedules.iter().position(|&o| o == s)).map_or(0, |i| i + 1);
            middle = middle.push(w::document_tabs(look, &tabs, active, small("window.close"), Msg::SelectTab, Msg::CloseTab, Msg::SelectTab));
        }
        let middle = middle.push(workspace);
        let dragging = |side| self.drag.is_some_and(|(s, ..)| s == side);
        let mut body = row![].height(Length::Fill);
        if self.prefs.left {
            let hide = l.text("Verkenner verbergen", "Hide explorer");
            let actions = vec![w::with_tooltip(look, w::icon_button(look, small("panel.left"), 20.0, true, hide, Some(Msg::ToggleLeft)), hide, Some(self.command("view.left").shortcut_label().unwrap_or("")))];
            body = body
                .push(w::panel(look, l.text("Verkenner", "Explorer"), self.prefs.left_width, actions, Self::pane(app.layers_body(p, l))))
                .push(ch::splitter(look, dragging(Side::Left), Msg::SplitterPress(Side::Left)));
        }
        body = body.push(middle);
        if self.prefs.right {
            let hide = l.text("Eigenschappen verbergen", "Hide properties");
            let actions = vec![w::with_tooltip(look, w::icon_button(look, small("panel.right"), 20.0, true, hide, Some(Msg::ToggleRight)), hide, Some(self.command("view.right").shortcut_label().unwrap_or("")))];
            body = body
                .push(ch::splitter(look, dragging(Side::Right), Msg::SplitterPress(Side::Right)))
                .push(w::panel(look, l.text("Eigenschappen", "Properties"), self.prefs.right_width, actions, Self::pane(app.properties_body(p, l))));
        }
        body.into()
    }

    /// The viewer's content of a side panel, clear of the panel's edges.
    fn pane(content: Element<'_, Message>) -> Element<'_, Msg> {
        container(content.map(Msg::App)).padding([4, 8]).width(Length::Fill).height(Length::Fill).into()
    }

    fn dialog_card(&self, look: Look, dialog: Dialog) -> Element<'_, Msg> {
        let l = self.l();
        let close_icon = small("window.close");
        match dialog {
            Dialog::Settings => {
                let tab = |page, label| w::button(look, label, if self.settings_page == page { K::Secondary } else { K::Quiet }, D::Compact, None, Some(Msg::SettingsPage(page)));
                let tabs = row![tab(SettingsPage::General, l.text("Algemeen", "General")), tab(SettingsPage::Shortcuts, l.text("Sneltoetsen", "Shortcuts"))].spacing(4);
                let scale = Scale::ALL.into_iter().find(|s| (s.0 - self.prefs.scale()).abs() < 0.01);
                let groups = [
                    ("file", l.text("Bestand", "File")),
                    ("view", l.text("Weergave", "Display")),
                    ("camera", l.text("Camera", "Camera")),
                    ("section", l.text("Doorsnede", "Section")),
                    ("measure", l.text("Meten", "Measure")),
                    ("app", l.text("Programma", "Application")),
                ];
                let themes = options(&THEMES, l, self.prefs.theme());
                let workspaces = options(&WORKSPACES, l, self.prefs.workspace());
                let statusbars = options(&STATUSBARS, l, self.prefs.statusbar());
                let body: Element<'_, Msg> = match self.settings_page {
                    SettingsPage::General => column![
                        w::mono_label(look, l.text("Weergave", "Appearance")),
                        w::property_row(look, l.text("Thema", "Theme"), w::select(look, themes.0, themes.1, |n| Msg::Theme(n.value), D::Regular)),
                        w::property_row(look, l.text("Werkruimte", "Workspace"), w::select(look, workspaces.0, workspaces.1, |n| Msg::Workspace(n.value), D::Regular)),
                        w::property_row(look, l.text("Statusbalk", "Status bar"), w::select(look, statusbars.0, statusbars.1, |n| Msg::Statusbar(n.value), D::Regular)),
                        w::property_row(look, l.text("Interfacegrootte", "Interface size"), w::select(look, &Scale::ALL, scale, |s: Scale| Msg::Scale(s.0), D::Regular)),
                        w::mono_label(look, l.text("Taal", "Language")),
                        w::property_row(look, l.text("Taal", "Language"), w::select(look, &Language::ALL, Some(l), Msg::Language, D::Regular)),
                    ]
                    .spacing(6)
                    .into(),
                    SettingsPage::Shortcuts => sc::page(look, &self.shortcuts, &self.commands(), &self.keymap, &groups, Msg::Shortcuts),
                };
                w::dialog(
                    look,
                    l.text("Instellingen", "Settings"),
                    close_icon,
                    Some(Msg::Dialog(None)),
                    column![tabs, body].spacing(14).into(),
                    None,
                    vec![w::button(look, l.text("Sluiten", "Close"), K::Primary, D::Regular, None, Some(Msg::Dialog(None)))],
                    true,
                )
            }
        }
    }
}

/// Starts the viewer in its window: the shell around `studio`, with the shared fonts and no
/// system title bar (the shell draws its own).
pub fn run(studio: Studio, icon: Option<iced::window::Icon>) -> iced::Result {
    fonts::install_fallback();
    // the installer reports updates; the app only shows them
    #[cfg(not(target_arch = "wasm32"))]
    let studio = Studio { update: upd::check_installed(APP_ID, VERSION), ..studio };
    let initial = std::sync::Mutex::new(Some(studio));
    let builder = iced::application(move || (initial.lock().unwrap_or_else(|e| e.into_inner()).take().expect("the application boots once"), Task::none()), Studio::update, Studio::view)
        .title(Studio::title)
        .subscription(Studio::subscription)
        .theme(|s: &Studio| s.prefs.theme().iced())
        .scale_factor(|s: &Studio| s.prefs.scale() as f32)
        .font(crate::INTER_REGULAR)
        .font(crate::INTER_SEMIBOLD)
        .antialiasing(true)
        .default_font(fonts::BODY);
    fonts::FILES
        .iter()
        .fold(builder, |builder, file| builder.font(*file))
        .window(iced::window::Settings {
            size: iced::Size::new(1280.0, 800.0),
            min_size: Some(iced::Size::new(960.0, 600.0)),
            decorations: false,
            exit_on_close_request: false,
            icon,
            ..Default::default()
        })
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use openaec_ui::design::a11y::Collect;
    use openaec_ui::iced::advanced::layout::{self, Layout};
    use openaec_ui::iced::advanced::widget::Tree;

    fn accessibility(studio: &Studio) -> Collect {
        let mut element = studio.view();
        let mut tree = Tree::new(&element);
        let renderer = iced::Renderer::Secondary(iced_tiny_skia::Renderer::new(fonts::BODY, iced::Pixels(12.0)));
        let limits = layout::Limits::new(iced::Size::ZERO, iced::Size::new(1600.0, 900.0));
        let node = element.as_widget_mut().layout(&mut tree, &renderer, &limits);
        let mut collect = Collect::default();
        element.as_widget_mut().operate(&mut tree, Layout::new(&node), &renderer, &mut collect);
        collect
    }

    /// Renders the shell's screens to `target/shell-renders` for review:
    /// `cargo test --release --lib shell -- --ignored`.
    #[test]
    #[ignore = "writes PNG files"]
    fn export_renders() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/shell-renders");
        std::fs::create_dir_all(&dir).expect("folder");
        let mut studio = Studio::new(App::default());
        studio.prefs.language = Language::Dutch;
        studio.prefs.recent = vec![PathBuf::from("C:/Projecten/Kade 12/kade12.ifc"), PathBuf::from("C:/Projecten/Snowdon/snowdon_arch_2025.rvt")];
        let shots: [(&str, ThemeId, bool, Option<Dialog>, SettingsPage); 6] = [
            ("leeg-light", ThemeId::Light, false, None, SettingsPage::General),
            ("leeg-dark", ThemeId::Dark, false, None, SettingsPage::General),
            ("leeg-blueprint", ThemeId::Blueprint, false, None, SettingsPage::General),
            ("bestand", ThemeId::Light, true, None, SettingsPage::General),
            ("instellingen", ThemeId::Light, false, Some(Dialog::Settings), SettingsPage::General),
            ("sneltoetsen", ThemeId::Light, false, Some(Dialog::Settings), SettingsPage::Shortcuts),
        ];
        for (name, theme, backstage, dialog, page) in shots {
            studio.prefs.theme = theme.key().into();
            studio.backstage = backstage;
            studio.dialog = dialog;
            studio.settings_page = page;
            studio.snapshot(iced::Size::new(1280.0, 800.0), &dir.join(format!("{name}.png"))).expect("snapshot");
        }
    }

    #[test]
    fn every_setting_has_a_name_in_both_languages() {
        for i in 0..2 {
            assert_eq!(THEMES[i].map(|n| n.value), ThemeId::ALL);
            assert_eq!(WORKSPACES[i].map(|n| n.value), WorkspaceSetting::ALL);
            assert_eq!(STATUSBARS[i].map(|n| n.value), StatusbarSetting::ALL);
        }
        // the Dutch names are the library's
        assert!(THEMES[0].iter().all(|n| n.label == n.value.name()));
        assert!(WORKSPACES[0].iter().all(|n| n.label == n.value.name()));
        assert!(STATUSBARS[0].iter().all(|n| n.label == n.value.name()));
    }

    #[test]
    fn no_two_commands_share_a_shortcut() {
        let studio = Studio::new(App::default());
        assert_eq!(cm::conflicts(&studio.default_commands()), Vec::<(&str, &str)>::new());
    }

    #[test]
    fn every_ribbon_and_title_command_exists() {
        let mut studio = Studio::new(App::default());
        for tab in 0..3 {
            studio.ribbon_tab = tab;
            let _ = studio.view();
        }
        let _ = studio.backstage_view(studio.look());
    }

    #[test]
    fn every_control_has_a_name_in_every_view() {
        let mut studio = Studio::new(App::default());
        studio.update = Some(Available { app: APP_ID.into(), current: VERSION.into(), latest: "2099.1.0".into(), page_url: None, installer: None });
        for (backstage, dialog, page) in [(false, None, SettingsPage::General), (true, None, SettingsPage::General), (false, Some(Dialog::Settings), SettingsPage::General), (false, Some(Dialog::Settings), SettingsPage::Shortcuts)] {
            studio.backstage = backstage;
            studio.dialog = dialog;
            studio.settings_page = page;
            let unnamed = accessibility(&studio).unnamed;
            assert!(unnamed.is_empty(), "unnamed controls with backstage {backstage}, dialog {dialog:?}, {page:?}: {unnamed:?}");
        }
    }

    #[test]
    fn keys_go_through_the_command_list() {
        use iced::keyboard::{Key, Modifiers};
        let mut studio = Studio::new(App::default());
        let _ = studio.update(Msg::Key(KeyPress { key: Key::Character("g".into()), modifiers: Modifiers::empty() }));
        assert!(!studio.app.grid || App::default().grid != studio.app.grid);
        let left = studio.prefs.left;
        let _ = studio.update(Msg::Key(KeyPress { key: Key::Character("1".into()), modifiers: Modifiers::CTRL }));
        assert_eq!(studio.prefs.left, !left);
    }

    #[test]
    fn a_splitter_drag_follows_the_cursor_within_bounds() {
        let mut studio = Studio::new(App::default());
        let start = studio.prefs.left_width;
        let _ = studio.update(Msg::SplitterPress(Side::Left));
        let _ = studio.update(Msg::SplitterMove(Point::new(300.0, 10.0)));
        let _ = studio.update(Msg::SplitterMove(Point::new(340.0, 10.0)));
        assert_eq!(studio.prefs.left_width, start + 40.0);
        let _ = studio.update(Msg::SplitterMove(Point::new(5000.0, 10.0)));
        assert_eq!(studio.prefs.left_width, PANEL_MAX);
        let _ = studio.update(Msg::SplitterRelease);
        assert!(studio.drag.is_none());
    }

    #[test]
    fn old_preferences_still_load() {
        let old = r#"{"theme":"Forge","language":"Dutch","scale":1.25,"explorer_visible":false}"#;
        let prefs: Prefs = serde_json::from_str(old).unwrap();
        assert_eq!((prefs.theme(), prefs.language, prefs.scale()), (ThemeId::Light, Language::Dutch, 1.25));
    }
}
