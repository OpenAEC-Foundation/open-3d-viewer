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
/// At or below this width (a phone, or a narrow browser window) the layout is compact: the ribbon
/// scrolls sideways and folds away, and a side panel takes the place of the 3D view.
const COMPACT_WIDTH: f32 = 720.0;

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
    /// A file of this format (such as Revit) opens only in the desktop app: where to get it.
    DesktopOnly(&'static str),
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
    /// A message for the project shown (from its view and subscriptions).
    App(Message),
    /// A message for project `id`, shown or not: what its tasks (a load, say) return.
    Project(u64, Message),
    /// The project tabs: show or close tab `i`.
    SelectProject(usize),
    CloseProject(usize),
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
    /// The window's width (logical pixels), for the compact layout.
    Resized(f32),
    Minimize,
    Maximize,
    CloseWindow,
    Update,
    /// Opens the desktop app's download page (from [`Dialog::DesktopOnly`]).
    DownloadDesktop,
}

pub struct Studio {
    /// The project shown.
    pub app: App,
    /// Its id, the other open projects with theirs, and the order of the project tabs.
    project: u64,
    parked: Vec<(u64, App)>,
    tabs: Vec<u64>,
    next_project: u64,
    prefs: Prefs,
    /// Where the preferences and own shortcuts are kept; `None` on the web and in tests.
    prefs_path: Option<PathBuf>,
    keymap_path: Option<PathBuf>,
    keymap: Keymap,
    shortcuts: sc::Editor,
    backstage: bool,
    dialog: Option<Dialog>,
    settings_page: SettingsPage,
    update: Option<Available>,
    maximized: bool,
    last_title_press: Option<Instant>,
    /// The splitter being dragged, with the cursor and panel width where it started.
    drag: Option<(Side, Option<f32>, f32)>,
    /// The window's width in layout units: iced reports it already divided by the interface size.
    width: f32,
    /// In the compact layout: the side panel shown instead of the 3D view, and the folded ribbon.
    compact_panel: Option<Side>,
    ribbon_folded: bool,
    /// A dev build started by `scripts/dev.ps1`: the open projects kept for its next start.
    dev: Option<DevSession>,
}

/// The open projects of a dev build, kept for its next start: `scripts/dev.ps1` restarts the viewer
/// after every change to its source, and the new build opens the same files, each in its tab, and
/// shows the same tab. The file (`O3V_DEV_SESSION`) holds a line `tab N`, then a file per tab.
pub struct DevSession {
    path: PathBuf,
    /// What the file holds now.
    written: String,
    /// The files still to open at the start, each in a tab of its own, and the tab to show then.
    to_open: Vec<PathBuf>,
    tab: Option<usize>,
    /// The file each project opened.
    opened: Vec<(u64, PathBuf)>,
    /// What the window title adds, such as the branch (`O3V_DEV_LABEL`).
    label: Option<String>,
}

impl DevSession {
    /// The files and the tab shown in a session file.
    fn parse(text: &str) -> (Vec<PathBuf>, usize) {
        let (mut files, mut tab) = (Vec::new(), 0);
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            match line.strip_prefix("tab ") {
                Some(n) => tab = n.parse().unwrap_or(0),
                None => files.push(PathBuf::from(line)),
            }
        }
        (files, tab)
    }
}

impl Studio {
    pub fn new(app: App) -> Self {
        Self {
            app,
            project: 0,
            parked: Vec::new(),
            tabs: vec![0],
            next_project: 1,
            prefs: Prefs::default(),
            prefs_path: None,
            keymap_path: None,
            keymap: Keymap::default(),
            shortcuts: sc::Editor::default(),
            backstage: false,
            dialog: None,
            settings_page: SettingsPage::General,
            update: None,
            maximized: false,
            last_title_press: None,
            drag: None,
            width: 1280.0,
            compact_panel: None,
            ribbon_folded: false,
            dev: None,
        }
    }

    /// A dev build: keeps its open projects in `path` and, without a file of its own to open,
    /// opens the ones kept there by the build before.
    pub fn with_dev_session(mut self, path: PathBuf, label: Option<String>) -> Self {
        let mut dev = DevSession { path, written: String::new(), to_open: Vec::new(), tab: None, opened: Vec::new(), label };
        if self.app.pending_open.is_none() {
            if let Ok(text) = std::fs::read_to_string(&dev.path) {
                let (mut files, tab) = DevSession::parse(&text);
                files.retain(|f| f.exists());
                if !files.is_empty() {
                    self.app.pending_open = Some(files.remove(0));
                    dev.to_open = files;
                    dev.tab = Some(tab);
                    dev.written = text;
                }
            }
        }
        self.dev = Some(dev);
        self
    }

    /// Project `id`: the one shown or a parked one.
    fn project_app(&self, id: u64) -> &App {
        if id == self.project {
            return &self.app;
        }
        self.parked.iter().find(|(p, _)| *p == id).map_or(&self.app, |(_, a)| a)
    }

    /// Writes the open projects of a dev build to its session file when they changed, once the
    /// files of the start are all open.
    fn keep_dev_session(&mut self) {
        let Some(dev) = &self.dev else { return };
        if !dev.to_open.is_empty() || self.app.pending_open.is_some() {
            return;
        }
        let open: Vec<(u64, &PathBuf)> = self
            .tabs
            .iter()
            .filter(|&&id| self.project_app(id).busy())
            .filter_map(|&id| dev.opened.iter().find(|(p, _)| *p == id).map(|(_, f)| (id, f)))
            .collect();
        let tab = open.iter().position(|(id, _)| *id == self.project).unwrap_or(0);
        let mut text = format!("tab {tab}\n");
        for (_, f) in &open {
            text += &format!("{}\n", std::path::absolute(f).unwrap_or_else(|_| f.to_path_buf()).display());
        }
        if text != dev.written {
            let path = dev.path.clone();
            if let Err(e) = std::fs::write(&path, &text) {
                eprintln!("dev session {}: {e}", path.display());
            }
            if let Some(dev) = &mut self.dev {
                dev.written = text;
            }
        }
    }

    /// A phone or a narrow window: see [`COMPACT_WIDTH`].
    fn compact(&self) -> bool {
        self.width <= COMPACT_WIDTH
    }

    /// Whether the side panel shows: from the preferences, or in the compact layout the one panel
    /// that takes the place of the 3D view.
    fn panel_shown(&self, side: Side) -> bool {
        match (self.compact(), side) {
            (true, side) => self.compact_panel == Some(side),
            (false, Side::Left) => self.prefs.left,
            (false, Side::Right) => self.prefs.right,
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
            Command::new("file.add", l.text("Toevoegen…", "Add…"), go(Message::AddDialog))
                .icon(small("file.import"), large("file.import"))
                .shortcut(Shortcut::ctrl_shift('o'))
                .dialog(),
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
            Command::new("view.left", l.text("Verkenner", "Explorer"), Msg::ToggleLeft).icon(small("panel.left"), large("panel.left")).shortcut(Shortcut::ctrl('1')).toggle(self.panel_shown(Side::Left)),
            Command::new("view.right", l.text("Eigenschappen", "Properties"), Msg::ToggleRight)
                .icon(small("panel.right"), large("panel.right"))
                .shortcut(Shortcut::ctrl('2'))
                .toggle(self.panel_shown(Side::Right)),
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
            Command::new("edit.undo", l.text("Ongedaan maken", "Undo"), go(Message::Undo)).shortcut(Shortcut::ctrl('z')).enabled(app.history.can_undo()),
            Command::new("edit.redo", l.text("Opnieuw", "Redo"), go(Message::Redo)).shortcut(Shortcut::ctrl('y')).enabled(app.history.can_redo()),
            Command::new("selection.layer", l.text("Laag selecteren", "Select layer"), go(Message::SelectLayerOfSelection))
                .icon(small("view.fit-selection"), large("view.fit-selection"))
                .enabled(!app.sel.is_empty()),
            Command::new("object.hide", l.text("Verbergen", "Hide"), go(Message::HideSelection))
                .icon(small("object.hide"), large("object.hide"))
                .shortcut(key('h'))
                .enabled(!app.sel.is_empty()),
            Command::new("object.isolate", l.text("Isoleren", "Isolate"), go(Message::IsolateSelection))
                .icon(small("object.show"), large("object.show"))
                .shortcut(key('i'))
                .enabled(!app.sel.is_empty()),
            Command::new("object.show-all", l.text("Alles tonen", "Show all"), go(Message::ShowAllElements))
                .icon(small("object.show"), large("object.show"))
                .shortcut(Shortcut::ctrl_shift('h'))
                .enabled(!app.hidden.is_empty()),
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

    /// A new, empty project in a tab after the others, shown.
    fn new_project(&mut self) {
        let fresh = self.app.new_project();
        self.parked.push((self.project, std::mem::replace(&mut self.app, fresh)));
        self.project = self.next_project;
        self.next_project += 1;
        self.tabs.push(self.project);
    }

    /// Shows project `id`: the window's settings go with it.
    fn show_project(&mut self, id: u64) {
        let Some(k) = self.parked.iter().position(|(p, _)| *p == id) else { return };
        let (_, mut app) = self.parked.swap_remove(k);
        app.take_window_settings(&self.app);
        self.parked.push((self.project, std::mem::replace(&mut self.app, app)));
        self.project = id;
    }

    /// Closes project tab `i`: the tab beside it shows; the last one leaves an empty project.
    fn close_project(&mut self, i: usize) {
        let Some(&id) = self.tabs.get(i) else { return };
        if self.tabs.len() == 1 {
            self.app = self.app.new_project();
            return;
        }
        if id == self.project {
            let next = self.tabs[if i + 1 < self.tabs.len() { i + 1 } else { i - 1 }];
            self.show_project(next);
        }
        self.parked.retain(|(p, _)| *p != id);
        self.tabs.remove(i);
    }

    /// A dev build: notes the file project `id` opens, and whether that was the last file of the
    /// start (its tab is shown then).
    fn dev_opens(&mut self, id: u64, message: &Message) -> bool {
        let Some(dev) = &mut self.dev else { return false };
        let file = match message {
            Message::Open(path) | Message::Picked(Some(crate::loaders::Source::File(path))) => path,
            _ => return false,
        };
        dev.opened.retain(|(p, _)| *p != id);
        dev.opened.push((id, file.clone()));
        if dev.to_open.first() == Some(file) {
            dev.to_open.remove(0);
            return dev.to_open.is_empty();
        }
        // the first file of the start, when it is the only one
        dev.to_open.is_empty() && dev.tab.is_some()
    }

    /// Whether the project tabs show: with more than one project, or one with a model (or loading).
    fn shows_project_tabs(&self) -> bool {
        self.tabs.len() > 1 || self.app.busy()
    }

    /// The title of a project tab: its file, the file it loads, or a new project.
    fn project_title(app: &App, l: Language) -> &str {
        match (&app.status, &app.model) {
            (Status::Loading { file, .. }, _) => file,
            (_, Some(m)) => m.path.file_name().and_then(|n| n.to_str()).unwrap_or("3D"),
            _ => l.text("Nieuw project", "New project"),
        }
    }

    fn command(&self, id: &str) -> Command<Msg> {
        self.commands().into_iter().find(|c| c.id == id).unwrap_or_else(|| panic!("command {id}"))
    }

    pub fn update(&mut self, message: Msg) -> Task<Msg> {
        let task = self.handle(message);
        self.keep_dev_session();
        task
    }

    fn handle(&mut self, message: Msg) -> Task<Msg> {
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
                // a format only the desktop app opens: say where to get it, with a working link,
                // which the status bar has no room for on a phone
                let desktop_only = match &message {
                    Message::Loaded(Err(e)) => crate::loaders::desktop_only_error(e),
                    _ => None,
                };
                // another file beside a model (or one loading) opens in a tab of its own, and
                // loads at the same time
                if matches!(message, Message::Open(_) | Message::Picked(Some(_))) && self.app.busy() {
                    self.new_project();
                }
                // what the project's tasks return goes back to it, even when another is shown
                let id = self.project;
                let started = self.dev_opens(id, &message);
                let task = self.app.update(message).map(move |m| Msg::Project(id, m));
                if started {
                    if let Some(&shown) = self.dev.as_mut().and_then(|d| d.tab.take()).and_then(|t| self.tabs.get(t)) {
                        self.show_project(shown);
                    }
                }
                if let Some(format) = desktop_only {
                    return Task::batch([task, self.update(Msg::Dialog(Some(Dialog::DesktopOnly(format))))]);
                }
                return task;
            }
            Msg::Project(id, message) if id == self.project => return self.update(Msg::App(message)),
            Msg::Project(id, message) => {
                // a project in the background; a closed one is gone
                if let Some((_, app)) = self.parked.iter_mut().find(|(p, _)| *p == id) {
                    return app.update(message).map(move |m| Msg::Project(id, m));
                }
            }
            Msg::SelectProject(i) => {
                if let Some(&id) = self.tabs.get(i) {
                    self.show_project(id);
                }
            }
            Msg::CloseProject(i) => self.close_project(i),
            // one ribbon tab: choosing it closes Bestand; in the compact layout it also folds the
            // ribbon away and back
            Msg::RibbonTab(_) => {
                self.ribbon_folded = self.compact() && !self.backstage && !self.ribbon_folded;
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
                // the window keeps its size, and iced reports no resize: the width in layout units
                // grows as the interface shrinks
                let old = self.prefs.scale();
                self.prefs.scale = scale;
                self.width *= (old / self.prefs.scale()) as f32;
                self.save_prefs();
            }
            // the compact layout shows one panel at a time and leaves the preferences as they are
            Msg::ToggleLeft | Msg::ToggleRight if self.compact() => {
                let side = if matches!(message, Msg::ToggleLeft) { Side::Left } else { Side::Right };
                self.compact_panel = (self.compact_panel != Some(side)).then_some(side);
            }
            Msg::ToggleLeft => {
                self.prefs.left = !self.prefs.left;
                self.save_prefs();
            }
            Msg::ToggleRight => {
                self.prefs.right = !self.prefs.right;
                self.save_prefs();
            }
            Msg::Resized(width) => self.width = width,
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
            // document tabs: the 3D view, the open schedules, then the open sheets
            Msg::SelectTab(i) => {
                let schedules = self.app.open_schedules.len();
                if let Some(&sheet) = i.checked_sub(1 + schedules).and_then(|k| self.app.open_sheets.get(k)) {
                    return self.update(Msg::App(Message::ShowSheet(Some(sheet))));
                }
                let schedule = i.checked_sub(1).and_then(|i| self.app.open_schedules.get(i).copied());
                self.app.active_sheet = None;
                return self.update(Msg::App(Message::ShowSchedule(schedule)));
            }
            Msg::CloseTab(i) => {
                // the 3D view (tab 0) stays open
                let schedules = self.app.open_schedules.len();
                if let Some(&sheet) = i.checked_sub(1 + schedules).and_then(|k| self.app.open_sheets.get(k)) {
                    return self.update(Msg::App(Message::CloseSheet(sheet)));
                }
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
            Msg::DownloadDesktop => {
                if let Err(e) = open_url(crate::loaders::DESKTOP_DOWNLOAD) {
                    self.app.status = Status::Error(e);
                }
                return self.update(Msg::Dialog(None));
            }
        }
        Task::none()
    }

    pub fn subscription(&self) -> Subscription<Msg> {
        // the command-line file, once: a subscription that yields Open and ends with it
        // a dev build's other files of the start, one after the other once the first loads
        let next = self.dev.as_ref().and_then(|d| d.to_open.first());
        let open = match (&self.app.pending_open, next) {
            (Some(path), _) => Subscription::run_with(path.clone(), |p: &PathBuf| iced::futures::stream::once(std::future::ready(Message::Open(p.clone())))),
            (None, Some(path)) => Subscription::run_with((path.clone(), "dev"), |(p, _): &(PathBuf, &str)| iced::futures::stream::once(std::future::ready(Message::Open(p.clone())))),
            (None, None) => Subscription::none(),
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
        let resized = iced::event::listen_with(|event, _, _| match event {
            iced::Event::Window(iced::window::Event::Opened { size, .. } | iced::window::Event::Resized(size)) => Some(Msg::Resized(size.width)),
            _ => None,
        });
        Subscription::batch([
            self.app.subscription().map(Msg::App),
            resized,
            open.map(Msg::App),
            drag,
            openaec_ui::keyboard::presses().map(Msg::Key),
            openaec_ui::keyboard::navigation().map(Msg::Navigate),
            iced::window::close_requests().map(|_| Msg::CloseWindow),
        ])
    }

    pub fn title(&self) -> String {
        let title = match &self.app.model {
            Some(m) => format!("{} · {NAME}", m.file_name()),
            None => NAME.into(),
        };
        match self.dev.as_ref().and_then(|d| d.label.as_deref()) {
            Some(label) => format!("{title} · {label}"),
            None => title,
        }
    }

    fn status_text(&self) -> String {
        let l = self.l();
        match &self.app.status {
            Status::Ready => l.text("Gereed", "Ready").into(),
            Status::Loading { file, since, .. } => format!("{} {file}… {:.0} s", l.text("Laden", "Loading"), since.elapsed().as_secs_f32()),
            Status::Saving { file, since } => format!("{} {file}… {:.0} s", l.text("Opslaan", "Saving"), since.elapsed().as_secs_f32()),
            Status::Loaded(s) => s.clone(),
            Status::Error(e) => match crate::loaders::desktop_only_error(e) {
                // a format only the desktop app opens: the short message in the chosen language
                // (the download link is in the dialog that opens with it)
                Some(format) => format!("{}: {}", l.text("Fout", "Error"), crate::loaders::desktop_only_message(format, l == Language::Dutch)),
                None => format!("{}: {e}", l.text("Fout", "Error")),
            },
        }
    }

    pub fn view(&self) -> Element<'_, Msg> {
        let look = self.look();
        let l = self.l();
        let c = look.color;
        let tabs = [l.text("Start", "Home")];
        let mut left = vec![self.status_text()];
        let selected = self.app.selection.len() + self.app.structural.selected.len();
        if selected > 0 {
            left.push(format!("{selected} {}", l.text("geselecteerd", "selected")));
        }
        let compact = self.compact();
        let folded = compact && self.ribbon_folded;
        let ribbon: Element<'_, Msg> = if folded { Space::new().into() } else { self.ribbon(look) };
        // a phone has no room for the version, and the web app has no remote control
        let status: Element<'_, Msg> = if compact { w::status_bar(look, vec![one_line(&left.join(" · "), self.width)], Vec::new()) } else { row![w::status_bar(look, left, vec![format!("{NAME} {VERSION}")]), self.mcp_lamp(look)].into() };
        let screen = column![
            self.title_bar(look),
            w::ribbon_tabs(look, l.text("Bestand", "File"), Some(Msg::Backstage(!self.backstage)), &tabs, if folded { usize::MAX } else { 0 }, Msg::RibbonTab),
            // Bestand takes the place of the ribbon and the documents until it closes
            if self.backstage { self.backstage_view(look) } else { column![ribbon, self.document_area(look)].into() },
            status,
        ]
        .height(Length::Fill);
        let mut layers = vec![focus::inert(container(screen).style(move |_| container::Style { background: Some(c.frame.into()), ..Default::default() }), self.dialog.is_some())];
        if let Some(dialog) = self.dialog {
            layers.push(mouse_area(w::overlay(look, self.dialog_card(look, dialog))).into());
        }
        iced::widget::Stack::with_children(layers).into()
    }

    /// The lamp at the right of the status bar: grey when remote control is off, amber when the
    /// MCP server can connect, green while it is connected.
    fn mcp_lamp(&self, look: Look) -> Element<'_, Msg> {
        use crate::remote::{link_status, LinkStatus};
        let l = self.l();
        let c = look.color;
        let status = if self.app.remote.enabled { link_status() } else { LinkStatus::Off };
        let (color, label, tip) = match status {
            LinkStatus::Connected => (iced::Color::from_rgb8(0x2e, 0xa0, 0x43), l.text("MCP verbonden", "MCP connected"), l.text("Een AI-agent is verbonden via open-3d-viewer-mcp", "An AI agent is connected through open-3d-viewer-mcp")),
            LinkStatus::Ready => (iced::Color::from_rgb8(0xd2, 0x99, 0x22), l.text("MCP beschikbaar", "MCP available"), l.text("Klaar voor een AI-agent via open-3d-viewer-mcp", "Ready for an AI agent through open-3d-viewer-mcp")),
            LinkStatus::Off => (c.statusbar_text, l.text("MCP uit", "MCP off"), l.text("Afstandsbediening staat uit (--no-remote of O3V_REMOTE=0)", "Remote control is off (--no-remote or O3V_REMOTE=0)")),
        };
        let dot = container(Space::new().width(8).height(8)).style(move |_| container::Style {
            background: Some(color.into()),
            border: iced::Border { radius: 4.0.into(), ..Default::default() },
            ..Default::default()
        });
        let line = row![dot, iced::widget::text(label).size(11).font(fonts::MONO).color(c.statusbar_text)].spacing(6).align_y(iced::Alignment::Center);
        let cell = column![
            w::divider(look),
            container(line)
                .height(openaec_ui::design::tokens::shell::STATUSBAR - 1.0)
                .padding([0, 10])
                .align_y(iced::alignment::Vertical::Center)
                .style(move |_| container::Style { background: Some(c.statusbar.into()), ..Default::default() }),
        ]
        .width(Length::Shrink);
        w::with_tooltip(look, cell.into(), tip, None)
    }

    fn title_bar(&self, look: Look) -> Element<'_, Msg> {
        let quick = vec![cm::icon_button(look, &self.command("file.open"), 28.0), cm::icon_button(look, &self.command("file.save-as"), 28.0)];
        let mut extra = Vec::new();
        if let Some(available) = &self.update {
            extra.push(container(upd::notice(look, available, Msg::Update)).padding([0, 8]).into());
        }
        if self.compact() {
            // the panels have no room beside the 3D view: these buttons swap them in and out
            extra.push(cm::icon_button(look, &self.command("view.left"), 28.0));
            extra.push(cm::icon_button(look, &self.command("view.right"), 28.0));
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
            // a phone has no room for the file name next to the buttons (the status bar has it)
            let name: Element<'_, Msg> = if self.compact() { Space::new().width(Length::Fill).into() } else { container(w::body(look, name)).width(Length::Fill).center_x(Length::Fill).into() };
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
        // One tab: the main command of a group large, the rest small in columns of three
        let tools: Vec<&str> = Tool::ALL.iter().map(|t| Self::tool_id(*t)).collect();
        let (first_tools, more_tools) = tools.split_at(3.min(tools.len()));
        let mut measure = vec![smalls(first_tools)];
        if !more_tools.is_empty() {
            measure.push(smalls(more_tools));
        }
        measure.push(smalls(&["measure.clear", "measure.clear-saved"]));
        let groups = vec![
            w::ribbon_group(look, l.text("Bestand", "File"), vec![large("file.open"), smalls(&["file.add", "file.save-as"])]),
            w::ribbon_group(look, l.text("Weergave", "Display"), vec![smalls(&["view.solid", "view.wireframe", "view.edges"])]),
            w::ribbon_group(look, l.text("Tonen", "Show"), vec![smalls(&["view.grid", "view.grid-lines", "view.flip-up"])]),
            w::ribbon_group(look, l.text("Camera", "Camera"), vec![large("camera.fit"), smalls(&["camera.iso", "camera.top", "camera.front"]), smalls(&["camera.right", "camera.perspective", "camera.ortho"])]),
            w::ribbon_group(look, l.text("Selectie", "Selection"), vec![smalls(&["object.hide", "object.isolate", "object.show-all"]), smalls(&["selection.layer"])]),
            w::ribbon_group(look, l.text("Doorsnede", "Section"), vec![large("section.box"), large("section.plane"), smalls(&["section.flip", "section.selection", "section.fill"]), smalls(&["section.reset"])]),
            w::ribbon_group(look, l.text("Meten", "Measure"), measure),
            w::ribbon_group(look, l.text("Extra", "More"), vec![large("view.explode"), smalls(&["view.left", "view.right"])]),
        ];
        if self.compact() {
            // narrower than the ribbon: it scrolls sideways (a finger drags it)
            use iced::widget::scrollable::{Direction, Scrollbar};
            let c = look.color;
            let id = iced::widget::Id::new("ribbon");
            let band = iced::widget::scrollable(row(groups).height(Length::Fill)).id(id.clone()).direction(Direction::Horizontal(Scrollbar::new().width(3).scroller_width(3))).width(Length::Fill);
            return container(crate::swipe::swipe(id, true, band))
                .height(openaec_ui::design::tokens::shell::RIBBON)
                .width(Length::Fill)
                .style(move |_| container::Style { background: Some(c.panel.into()), ..Default::default() })
                .into();
        }
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
            t::file_nav_row(look, small("file.import"), l.text("Toevoegen", "Add"), false, Some(Msg::App(Message::AddDialog))),
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
        let page = iced::widget::scrollable(page).width(Length::Fill);
        // a phone: the commands above the recent files
        let content: Element<'_, Msg> = if self.compact() {
            column![container(nav.width(Length::Fill)).style(move |_| w::panel_style(look)), page].into()
        } else {
            row![container(nav).height(Length::Fill).style(move |_| w::panel_style(look)), page].into()
        };
        container(content)
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
                formats_hint(l),
                Some(open),
            ))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| container::Style { background: Some(look.color.workspace.into()), ..Default::default() })
            .into()
        } else {
            app.workspace_view(p, l).map(Msg::App)
        };
        // document tabs once a schedule or sheet is open: the model (the 3D view, pinned), the
        // schedules and the sheets
        let model_name = app.model.as_ref().and_then(|m| m.path.file_name()).and_then(|n| n.to_str()).unwrap_or("3D");
        let mut middle = column![].width(Length::Fill);
        // the open projects, as soon as one holds a model: its tab says which file this is
        if self.shows_project_tabs() {
            let tabs: Vec<DocumentTab<'_>> = self
                .tabs
                .iter()
                .map(|&id| if id == self.project { &self.app } else { self.parked.iter().find(|(p, _)| *p == id).map_or(&self.app, |(_, a)| a) })
                .map(|a| DocumentTab::new(Self::project_title(a, l), false))
                .collect();
            let active = self.tabs.iter().position(|&id| id == self.project).unwrap_or(0);
            middle = middle.push(w::document_tabs(look, &tabs, active, small("window.close"), Msg::SelectProject, Msg::CloseProject, Msg::SelectProject));
        }
        let all_sheets = app.model.as_ref().map_or(&[][..], |m| &m.project.sheets[..]);
        if app.model.is_some() && (!app.open_schedules.is_empty() || !app.open_sheets.is_empty()) {
            let schedules = app.open_schedules.iter().filter_map(|&i| app.schedules.get(i)).map(|s| DocumentTab::new(&s.name, false));
            let sheets = app.open_sheets.iter().filter_map(|&i| all_sheets.get(i)).map(|s| DocumentTab::new(&s.title, false));
            let tabs: Vec<DocumentTab<'_>> = std::iter::once(DocumentTab::pinned(model_name)).chain(schedules).chain(sheets).collect();
            let active = match (app.active_sheet, app.active_schedule) {
                (Some(s), _) => app.open_sheets.iter().position(|&o| o == s).map_or(0, |i| i + 1 + app.open_schedules.len()),
                (None, Some(s)) => app.open_schedules.iter().position(|&o| o == s).map_or(0, |i| i + 1),
                _ => 0,
            };
            middle = middle.push(w::document_tabs(look, &tabs, active, small("window.close"), Msg::SelectTab, Msg::CloseTab, Msg::SelectTab));
        }
        let middle = middle.push(workspace);
        if self.compact() {
            // one panel at a time, the full width, in place of the 3D view
            let width = self.width;
            return match self.compact_panel {
                Some(Side::Left) => {
                    let hide = l.text("Verkenner verbergen", "Hide explorer");
                    let actions = vec![w::icon_button(look, small("panel.left"), 20.0, true, hide, Some(Msg::ToggleLeft))];
                    w::panel(look, l.text("Verkenner", "Explorer"), width, actions, Self::pane(app.layers_body(p, l)))
                }
                Some(Side::Right) => {
                    let hide = l.text("Eigenschappen verbergen", "Hide properties");
                    let actions = vec![w::icon_button(look, small("panel.right"), 20.0, true, hide, Some(Msg::ToggleRight))];
                    w::panel(look, l.text("Eigenschappen", "Properties"), width, actions, Self::pane(app.properties_body(p, l)))
                }
                None => middle.into(),
            };
        }
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
            Dialog::DesktopOnly(format) => w::dialog(
                look,
                l.text("Desktop-app nodig", "Desktop app needed"),
                close_icon,
                Some(Msg::Dialog(None)),
                w::body(look, desktop_only_text(l, format)).into(),
                None,
                vec![
                    w::button(look, l.text("Sluiten", "Close"), K::Quiet, D::Regular, None, Some(Msg::Dialog(None))),
                    w::button(look, l.text("Downloaden", "Download"), K::Primary, D::Regular, None, Some(Msg::DownloadDesktop)),
                ],
                false,
            ),
        }
    }
}

/// What the desktop-only dialog says about a `format` file.
fn desktop_only_text(l: Language, format: &str) -> String {
    match l {
        Language::Dutch => format!(
            "{format}-bestanden openen alleen in de desktop-app; de browserversie kan ze niet lezen. De desktop-app is gratis, voor Windows, macOS en Linux: Downloaden opent de releasepagina op GitHub."
        ),
        Language::English => format!(
            "{format} files open only in the desktop app; the browser version cannot read them. The desktop app is free, for Windows, macOS and Linux: Download opens the releases page on GitHub."
        ),
    }
}

/// Opens `url` in the browser: a new tab on the web, the default browser on the desktop.
fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_arch = "wasm32")]
    return crate::web::open_url(url);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let program = match std::env::consts::OS {
            "windows" => "explorer",
            "macos" => "open",
            _ => "xdg-open",
        };
        std::process::Command::new(program).arg(url).spawn().map(|_| ()).map_err(|e| format!("Open {url}: {e}"))
    }
}

/// The start screen's hint: the featured formats this build opens, from the loader registry
/// (as the file dialog), and how many more it opens. Built once per language: the registry is
/// fixed before the app starts.
fn formats_hint(l: Language) -> &'static str {
    static HINTS: std::sync::OnceLock<[String; 2]> = std::sync::OnceLock::new();
    let hints = HINTS.get_or_init(|| {
        let (featured, others) = crate::loaders::featured_formats();
        [Language::Dutch, Language::English].map(|l| hint_text(l, &featured, others))
    });
    &hints[usize::from(l == Language::English)]
}

/// The start screen's hint in language `l`: the `featured` format names, then how many `others`.
fn hint_text(l: Language, featured: &[&str], others: usize) -> String {
    let or = l.text("of", "or");
    let mut items: Vec<String> = featured.iter().map(|s| s.to_string()).collect();
    match others {
        0 => {}
        1 => items.push(l.text("een ander formaat", "one other format").into()),
        n => items.push(match l {
            Language::Dutch => format!("een van {n} andere formaten"),
            Language::English => format!("one of {n} other formats"),
        }),
    }
    let list = match items.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} {or} {last}", rest.join(", ")),
    };
    format!("{}: {list}.", l.text("Open een bestand of sleep het hierheen", "Open a file or drop it here"))
}

/// `text` cut to what fits on one line of the status bar `width` wide (logical pixels).
fn one_line(text: &str, width: f32) -> String {
    // the status bar's 11 px text is about 6.5 px a character, between 10 px of padding each side
    let fits = ((width - 20.0) / 6.5).max(8.0) as usize;
    if text.chars().count() <= fits {
        return text.to_string();
    }
    text.chars().take(fits - 1).chain(std::iter::once('…')).collect()
}

/// Starts the viewer in its window: the shell around `studio`, with the shared fonts and no
/// system title bar (the shell draws its own).
/// `antialiasing`: smooth the overlay's shapes (the view cube, measurements, gizmos) with MSAA.
pub fn run(studio: Studio, icon: Option<iced::window::Icon>, antialiasing: bool) -> iced::Result {
    fonts::install_fallback();
    // the installer reports updates; the app only shows them, and not in managed mode
    #[cfg(not(target_arch = "wasm32"))]
    let studio = Studio { update: if crate::managed::managed() { None } else { upd::check_installed(APP_ID, VERSION) }, ..studio };
    let initial = std::sync::Mutex::new(Some(studio));
    let builder = iced::application(move || (initial.lock().unwrap_or_else(|e| e.into_inner()).take().expect("the application boots once"), Task::none()), Studio::update, Studio::view)
        .title(Studio::title)
        .subscription(Studio::subscription)
        .theme(|s: &Studio| s.prefs.theme().iced())
        .scale_factor(|s: &Studio| s.prefs.scale() as f32)
        .font(crate::INTER_REGULAR)
        .font(crate::INTER_SEMIBOLD)
        .antialiasing(antialiasing)
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

    #[test]
    fn start_screen_names_only_formats_this_build_opens() {
        let featured = ["IFC", "SketchUp", "glTF", "OBJ", "STL", "PLY"];
        assert_eq!(
            hint_text(Language::English, &featured, 29),
            "Open a file or drop it here: IFC, SketchUp, glTF, OBJ, STL, PLY or one of 29 other formats."
        );
        assert_eq!(
            hint_text(Language::Dutch, &featured, 29),
            "Open een bestand of sleep het hierheen: IFC, SketchUp, glTF, OBJ, STL, PLY of een van 29 andere formaten."
        );
        assert_eq!(hint_text(Language::English, &["IFC", "PLY"], 0), "Open a file or drop it here: IFC or PLY.");
        // from the registry: this build's featured formats, the desktop-only ones on the desktop alone
        let (names, others) = crate::loaders::featured_formats();
        let hint = formats_hint(Language::English);
        assert!(names.iter().all(|name| hint.contains(name)), "{hint}");
        assert!(hint.contains("IFC") && hint.contains(&format!("{others} other formats")), "{hint}");
        assert_eq!(hint.contains("Revit"), cfg!(not(target_arch = "wasm32")), "{hint}");
    }

    #[test]
    fn status_bar_shows_the_desktop_app_message_in_the_chosen_language() {
        let mut studio = Studio::new(App::default());
        studio.app.status = Status::Error(crate::loaders::desktop_only_message("Revit", false));
        studio.prefs.language = Language::Dutch;
        assert_eq!(studio.status_text(), "Fout: Revit vereist de desktop-app");
        studio.prefs.language = Language::English;
        assert_eq!(studio.status_text(), "Error: Revit needs the desktop app");
        // short enough for one status-bar line on a phone (390 px), where one_line cuts the rest
        assert_eq!(one_line(&studio.status_text(), 390.0), studio.status_text());
        studio.app.status = Status::Error("Unsupported file type: .abc".into());
        assert_eq!(studio.status_text(), "Error: Unsupported file type: .abc");
    }

    /// A model of one triangle, opened from file `name`.
    fn triangle(name: &str) -> std::sync::Arc<crate::model::Model> {
        let layer = crate::model::Layer {
            name: "Walls".into(),
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0],
            indices: vec![0, 1, 2],
            elements: vec![(1, 0, 3)],
            ..Default::default()
        };
        std::sync::Arc::new(crate::model::Model::new(PathBuf::from(name), "Revit", vec![layer]))
    }

    #[test]
    fn every_file_opened_beside_a_model_gets_a_tab_of_its_own() {
        let mut studio = Studio::new(App::default());
        // the first file goes in the empty project
        let _ = studio.update(Msg::App(Message::Open(PathBuf::from("a.rvt"))));
        assert_eq!(studio.tabs.len(), 1);
        let a = studio.project;
        // two more dropped while it loads: each in a tab of its own, loading at the same time
        let _ = studio.update(Msg::App(Message::Open(PathBuf::from("b.ifc"))));
        let _ = studio.update(Msg::App(Message::Open(PathBuf::from("c.skp"))));
        assert_eq!(studio.tabs.len(), 3);
        let (b, c) = (studio.tabs[1], studio.tabs[2]);
        assert_eq!(studio.project, c, "the newest tab is shown");
        assert!(matches!(studio.app.status, Status::Loading { .. }));
        assert!(studio.parked.iter().all(|(_, app)| matches!(app.status, Status::Loading { .. })));
        // the loads end in any order, each in its own tab
        let _ = studio.update(Msg::Project(a, Message::Loaded(Ok(triangle("a.rvt")))));
        let _ = studio.update(Msg::Project(c, Message::Loaded(Ok(triangle("c.skp")))));
        assert_eq!(studio.app.model.as_ref().map(|m| m.file_name()).as_deref(), Some("c.skp"));
        let _ = studio.update(Msg::SelectProject(0));
        assert_eq!(studio.project, a);
        assert_eq!(studio.app.model.as_ref().map(|m| m.file_name()).as_deref(), Some("a.rvt"));
        assert!(matches!(studio.parked.iter().find(|(p, _)| *p == b).map(|(_, app)| &app.status), Some(Status::Loading { .. })));
        // the tabs carry the file names
        studio.prefs.language = Language::English;
        let names = accessibility(&studio).summary();
        assert!(["a.rvt", "b.ifc", "c.skp"].iter().all(|f| names.iter().any(|n| n.contains(f))), "{names:?}");
    }

    #[test]
    fn one_open_model_has_its_tab_already() {
        let mut studio = Studio::new(App::default());
        studio.prefs.language = Language::English;
        assert!(!studio.shows_project_tabs(), "no tab over the start page");
        let _ = studio.update(Msg::App(Message::Open(PathBuf::from("a.rvt"))));
        assert!(studio.shows_project_tabs(), "a tab while the file loads");
        let a = studio.project;
        let _ = studio.update(Msg::Project(a, Message::Loaded(Ok(triangle("a.rvt")))));
        let names = accessibility(&studio).summary();
        assert!(names.iter().any(|n| n.contains("a.rvt")), "{names:?}");
    }

    #[test]
    fn a_dev_build_opens_the_projects_the_build_before_had_open() {
        let dir = std::env::temp_dir().join(format!("o3v-dev-session-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b, session) = (dir.join("a.rvt"), dir.join("b.ifc"), dir.join("session.txt"));
        for f in [&a, &b] {
            std::fs::write(f, b"").unwrap();
        }
        let _ = std::fs::remove_file(&session);
        let kept = || std::fs::read_to_string(&session).unwrap();
        // the build before: two files in tabs, the first one shown
        let mut before = Studio::new(App::default()).with_dev_session(session.clone(), None);
        let _ = before.update(Msg::App(Message::Open(a.clone())));
        let _ = before.update(Msg::App(Message::Open(b.clone())));
        assert_eq!(kept(), format!("tab 1\n{}\n{}\n", a.display(), b.display()));
        let _ = before.update(Msg::SelectProject(0));
        assert_eq!(kept(), format!("tab 0\n{}\n{}\n", a.display(), b.display()));
        // the next build opens them again, in that order, and shows the same tab
        let mut after = Studio::new(App::default()).with_dev_session(session.clone(), Some("dev".into()));
        assert_eq!(after.app.pending_open.as_ref(), Some(&a));
        let _ = after.update(Msg::App(Message::Open(a.clone())));
        assert_eq!(after.dev.as_ref().map(|d| d.to_open.clone()), Some(vec![b.clone()]));
        let _ = after.update(Msg::App(Message::Open(b.clone())));
        assert_eq!(after.tabs.len(), 2);
        assert_eq!(after.project, after.tabs[0], "the tab shown before");
        assert!(after.title().ends_with(" · dev"), "{}", after.title());
        // a closed tab leaves the session
        let _ = after.update(Msg::CloseProject(0));
        assert_eq!(kept(), format!("tab 0\n{}\n", b.display()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn closing_a_tab_shows_its_neighbour_and_the_last_leaves_an_empty_project() {
        let mut studio = Studio::new(App::default());
        studio.app.set_model(triangle("a.rvt"));
        let _ = studio.update(Msg::App(Message::Open(PathBuf::from("b.ifc"))));
        let b = studio.project;
        let _ = studio.update(Msg::Project(b, Message::Loaded(Ok(triangle("b.ifc")))));
        let _ = studio.update(Msg::CloseProject(1));
        assert_eq!(studio.tabs.len(), 1);
        assert_eq!(studio.app.model.as_ref().map(|m| m.file_name()).as_deref(), Some("a.rvt"));
        // a load for the closed tab that still comes in goes nowhere
        let _ = studio.update(Msg::Project(b, Message::Loaded(Ok(triangle("b.ifc")))));
        assert_eq!(studio.app.model.as_ref().map(|m| m.file_name()).as_deref(), Some("a.rvt"));
        let _ = studio.update(Msg::CloseProject(0));
        assert_eq!(studio.tabs.len(), 1);
        assert!(studio.app.model.is_none(), "an empty project in its place");
    }

    #[test]
    fn desktop_only_file_opens_a_dialog_with_the_download_link() {
        let mut studio = Studio::new(App::default());
        let _ = studio.update(Msg::App(Message::Loaded(Err(crate::loaders::desktop_only_message("Vertex BD", false)))));
        assert_eq!(studio.dialog, Some(Dialog::DesktopOnly("Vertex BD")));
        assert!(matches!(&studio.app.status, Status::Error(e) if e == "Vertex BD needs the desktop app"));
        // the dialog names the format and the button, in both languages, and is reachable by assistive tech
        assert!(desktop_only_text(Language::English, "Vertex BD").starts_with("Vertex BD files open only in the desktop app"));
        assert!(desktop_only_text(Language::Dutch, "Vertex BD").contains("Downloaden opent de releasepagina"));
        studio.prefs.language = Language::English;
        let names = accessibility(&studio).summary();
        assert!(names.iter().any(|n| n.ends_with(": Desktop app needed")) && names.iter().any(|n| n.ends_with(": Download")), "{names:?}");
        let _ = studio.update(Msg::Dialog(None));
        // any other load error leaves the dialog closed
        let _ = studio.update(Msg::App(Message::Loaded(Err("Unsupported file type: .abc".into()))));
        assert_eq!(studio.dialog, None);
    }

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
        let studio = Studio::new(App::default());
        let _ = studio.view();
        let _ = studio.backstage_view(studio.look());
    }

    #[test]
    fn every_control_has_a_name_in_every_view() {
        let mut studio = Studio::new(App::default());
        studio.update = Some(Available { app: APP_ID.into(), current: VERSION.into(), latest: "2099.1.0".into(), page_url: None, installer: None });
        for (backstage, dialog, page) in [(false, None, SettingsPage::General), (true, None, SettingsPage::General), (false, Some(Dialog::Settings), SettingsPage::General), (false, Some(Dialog::Settings), SettingsPage::Shortcuts), (false, Some(Dialog::DesktopOnly("Revit")), SettingsPage::General)] {
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
    fn the_compact_layout_follows_the_width_in_layout_units() {
        let mut studio = Studio::new(App::default());
        // a 1500 px window at interface size 1.5: iced reports 1000 layout units
        let _ = studio.update(Msg::Scale(1.5));
        let _ = studio.update(Msg::Resized(1000.0));
        assert!(!studio.compact());
        // twice the interface size on the same window: 750 units, still room for the panels
        let _ = studio.update(Msg::Scale(2.0));
        assert_eq!(studio.width, 750.0);
        assert!(!studio.compact());
        // a 412 px phone at size 1: compact; at 0.75 the same phone is 549 units wide
        let _ = studio.update(Msg::Scale(1.0));
        let _ = studio.update(Msg::Resized(412.0));
        assert!(studio.compact());
        let _ = studio.update(Msg::Scale(0.75));
        assert!((studio.width - 412.0 / 0.75).abs() < 0.01 && studio.compact());
    }

    #[test]
    fn old_preferences_still_load() {
        let old = r#"{"theme":"Forge","language":"Dutch","scale":1.25,"explorer_visible":false}"#;
        let prefs: Prefs = serde_json::from_str(old).unwrap();
        assert_eq!((prefs.theme(), prefs.language, prefs.scale()), (ThemeId::Light, Language::Dutch, 1.25));
    }
}
