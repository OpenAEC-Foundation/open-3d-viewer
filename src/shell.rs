//! The window shell after the OpenAEC Blueprint standard application (`src/bin/blueprint.rs` in
//! OpenAEC-Blueprint 2026.44.1), built from library 0.4 (`openaec_ui::design`): title bar with
//! quick actions and the update notice, ribbon with Bestand, document tabs, explorer and
//! properties with splitters, status bar, settings with themes and own shortcuts, and one command
//! list for buttons, menus and shortcuts. The viewer (`App`) fills the workspace and the panels.

use crate::app::{App, DocTab, Message, Status};
use crate::b2::{large, small};
use crate::measure::Tool;
use crate::product::{Edition, Product, RibbonPage, PLANNED};
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

/// The number of interface languages ([`Language::ALL`]).
const LANGUAGES: usize = Language::ALL.len();

/// The settings per language, in the order of [`Language::ALL`]: Dutch, English, German, French.
const THEMES: [[Named<ThemeId>; 5]; LANGUAGES] = [
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
    [
        named(ThemeId::Light, "Hell"),
        named(ThemeId::Dark, "Dunkel"),
        named(ThemeId::Blueprint, "Blueprint"),
        named(ThemeId::HighContrastDark, "Hoher Kontrast dunkel"),
        named(ThemeId::HighContrastLight, "Hoher Kontrast hell"),
    ],
    [
        named(ThemeId::Light, "Clair"),
        named(ThemeId::Dark, "Sombre"),
        named(ThemeId::Blueprint, "Blueprint"),
        named(ThemeId::HighContrastDark, "Contraste élevé sombre"),
        named(ThemeId::HighContrastLight, "Contraste élevé clair"),
    ],
];
const WORKSPACES: [[Named<WorkspaceSetting>; 3]; LANGUAGES] = [
    [named(WorkspaceSetting::Theme, "Volgt thema"), named(WorkspaceSetting::Light, "Altijd licht"), named(WorkspaceSetting::Dark, "Altijd donker")],
    [named(WorkspaceSetting::Theme, "Follows the theme"), named(WorkspaceSetting::Light, "Always light"), named(WorkspaceSetting::Dark, "Always dark")],
    [named(WorkspaceSetting::Theme, "Folgt dem Thema"), named(WorkspaceSetting::Light, "Immer hell"), named(WorkspaceSetting::Dark, "Immer dunkel")],
    [named(WorkspaceSetting::Theme, "Suit le thème"), named(WorkspaceSetting::Light, "Toujours clair"), named(WorkspaceSetting::Dark, "Toujours sombre")],
];
const STATUSBARS: [[Named<StatusbarSetting>; 2]; LANGUAGES] = [
    [named(StatusbarSetting::Theme, "Volgt thema"), named(StatusbarSetting::Dark, "Altijd donker")],
    [named(StatusbarSetting::Theme, "Follows the theme"), named(StatusbarSetting::Dark, "Always dark")],
    [named(StatusbarSetting::Theme, "Folgt dem Thema"), named(StatusbarSetting::Dark, "Immer dunkel")],
    [named(StatusbarSetting::Theme, "Suit le thème"), named(StatusbarSetting::Dark, "Toujours sombre")],
];

/// The options of a setting in `language`, and the one that is chosen.
fn options<T: Copy + PartialEq, const N: usize>(lists: &'static [[Named<T>; N]; LANGUAGES], language: Language, chosen: T) -> (&'static [Named<T>], Option<Named<T>>) {
    let list = &lists[language.index()];
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
    /// The product shown last.
    product: Product,
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
            product: Product::Viewer,
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
    /// The window's size changed (layout units).
    Sized(iced::Size),
    /// The window's size, asked for before a remote command on the window runs.
    #[cfg(not(target_arch = "wasm32"))]
    SizedFor(iced::Size, crate::remote::RemoteCall),
    /// A message for the project shown (from its view and subscriptions).
    App(Message),
    /// A message for project `id`, shown or not: what its tasks (a load, say) return.
    Project(u64, Message),
    /// The project tabs: show or close tab `i`.
    SelectProject(usize),
    CloseProject(usize),
    /// The switch at the top right: show this product.
    Product(Product),
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
    /// A press on an edge or corner of the window: the system resizes it from there.
    ResizeEdge(iced::window::Direction),
    /// The window opened: its native handle is asked for.
    WindowOpened(iced::window::Id),
    /// The window's native id (its `HWND` on Windows).
    NativeWindow(u64),
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
    /// Its height, likewise: with the width, the size a remote client's clicks are laid out at.
    height: f32,
    /// Whether the window has told its size (it does when it changes, not always when it opens).
    sized: bool,
    /// In the compact layout: the side panel shown instead of the 3D view, and the folded ribbon.
    compact_panel: Option<Side>,
    ribbon_folded: bool,
    /// What this build is (its name, its installer id), and the products it offers, in the order of
    /// the switch; the switch shows with two or more.
    edition: Edition,
    products: Vec<Product>,
    /// The ribbon's tab shown, of the product's [`Product::pages`].
    ribbon_tab: usize,
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
            height: 800.0,
            sized: false,
            compact_panel: None,
            ribbon_folded: false,
            edition: Edition::Studio,
            products: Edition::Studio.products(),
            ribbon_tab: 0,
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

    /// The product shown: the one chosen last, when this build offers it.
    pub fn product(&self) -> Product {
        if self.products.contains(&self.prefs.product) {
            self.prefs.product
        } else {
            self.products.first().copied().unwrap_or_default()
        }
    }

    /// Shows `product` (when this build offers it): its ribbon from its first tab, its panels and
    /// its start screen, over the same projects. A modelling tool that was on goes off outside the
    /// Modeller, in every project.
    pub fn set_product(&mut self, product: Product) {
        if !self.products.contains(&product) {
            return;
        }
        if product != self.product() {
            self.ribbon_tab = 0;
        }
        if product != Product::Modeller {
            for app in std::iter::once(&mut self.app).chain(self.parked.iter_mut().map(|(_, a)| a)) {
                if app.modeller.tool.is_some() {
                    let _ = app.update(Message::ModelTool(None));
                }
            }
        }
        self.prefs.product = product;
        self.backstage = false;
        self.ribbon_folded = false;
        self.save_prefs();
    }

    /// The ribbon tab shown.
    fn ribbon_page(&self) -> RibbonPage {
        let pages = self.product().pages();
        pages[self.ribbon_tab.min(pages.len() - 1)]
    }

    /// A build of `edition`: the Studio, or one product alone.
    pub fn with_edition(mut self, edition: Edition) -> Self {
        self.edition = edition;
        self.products = edition.products();
        self
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
        let panels = [(self.prefs.left, self.left_width()), (self.prefs.right, self.prefs.right_width)];
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

    /// The size the window is laid out at.
    fn layout_size(&self) -> iced::Size {
        // a window folded to the taskbar is laid out at the size it opens at
        if self.width < 640.0 || self.height < 400.0 {
            return iced::Size::new(1280.0, 800.0);
        }
        iced::Size::new(self.width, self.height)
    }

    /// The window's controls, as a remote client sees them (see [`crate::ui_control`]).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn ui_controls(&self) -> Vec<openaec_ui::design::a11y::Found> {
        crate::ui_control::controls(self.view(), self.layout_size())
    }

    /// A remote command on the window itself: `ui_controls` lists what it shows; `ui_press`
    /// clicks a control, `ui_type` types into a field and `ui_key` presses a key, through the
    /// widgets as a person's mouse and keyboard reach them. The messages they send go on as the
    /// window's would; with `dry_run` they are only told.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn ui_command(&mut self, call: crate::remote::RemoteCall) -> Task<Msg> {
        use crate::ui_control as ui;
        use serde_json::json;
        // the clicks are laid out at the window's size: asked for first when it is not known
        if !self.sized {
            self.sized = true;
            return iced::window::latest().and_then(iced::window::size).map(move |size| Msg::SizedFor(size, call.clone()));
        }
        let size = self.layout_size();
        let found = self.ui_controls();
        let args = call.args.clone();
        let answer: Result<serde_json::Value, String> = match call.cmd.as_str() {
            "ui_controls" => {
                let filter = args.get("filter").and_then(|v| v.as_str()).map(str::to_lowercase);
                let all = args.get("all").and_then(|v| v.as_bool()).unwrap_or(false);
                let list: Vec<serde_json::Value> = found
                    .iter()
                    .enumerate()
                    .filter(|(_, f)| all || ui::is_control(f))
                    .filter(|(_, f)| filter.as_deref().is_none_or(|q| f.node.name.to_lowercase().contains(q) || format!("{:?}", f.node.role).to_lowercase() == q))
                    .map(|(k, f)| ui::describe(k, f))
                    .collect();
                Ok(json!({ "size": [size.width, size.height], "product": self.product().key(), "count": list.len(), "controls": list }))
            }
            "ui_press" | "ui_type" | "ui_key" => {
                let dry = args.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);
                let target = if call.cmd == "ui_key" && args.get("name").is_none() && args.get("index").is_none() { None } else { Some(ui::find(&found, &args)) };
                match target {
                    Some(Err(e)) => Err(e),
                    Some(Ok((_, f))) if f.node.state.disabled => Err(format!("'{}' is disabled", f.node.name)),
                    target => {
                        let what = match &target {
                            Some(Ok((k, f))) => ui::describe(*k, f),
                            _ => serde_json::Value::Null,
                        };
                        // the click, on a copy of the window; `scroll`: first the list it lies in
                        // scrolls that far, so a control below the end of a panel comes in view
                        // the click, on a copy of the window laid out at `size`, on the control
                        // within `bounds`
                        let run = |size: iced::Size, bounds: Option<iced::Rectangle>| -> Vec<Msg> {
                            let settings = iced::Settings { fonts: [crate::INTER_REGULAR, crate::INTER_SEMIBOLD].into_iter().chain(fonts::FILES.iter().copied()).map(std::borrow::Cow::Borrowed).collect(), default_font: fonts::BODY, ..Default::default() };
                            let mut sim = iced_test::Simulator::with_size(settings, size, self.view());
                            if let Some(at) = bounds.and_then(|b| ui::aim(b, size)) {
                                sim.point_at(at);
                                let _ = sim.simulate(iced_test::simulator::click());
                            }
                        match call.cmd.as_str() {
                            "ui_type" => {
                                let text = args.get("text").and_then(|v| v.as_str()).unwrap_or_default();
                                // a field's old text goes first
                                if args.get("replace").and_then(|v| v.as_bool()).unwrap_or(true) {
                                    let old = target.as_ref().and_then(|t| t.as_ref().ok()).and_then(|(_, f)| f.node.value.as_ref()).map_or(64, |v| v.chars().count() + 2);
                                    for named in [iced::keyboard::key::Named::Backspace, iced::keyboard::key::Named::Delete] {
                                        for _ in 0..old {
                                            let _ = sim.simulate(iced_test::simulator::tap_key(iced::keyboard::Key::Named(named), None));
                                        }
                                    }
                                }
                                let _ = sim.typewrite(text);
                            }
                            "ui_key" => {
                                let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("Enter");
                                use iced::keyboard::key::Named;
                                let named = match key.to_ascii_lowercase().as_str() {
                                    "enter" | "return" => Some(Named::Enter),
                                    "escape" | "esc" => Some(Named::Escape),
                                    "tab" => Some(Named::Tab),
                                    "delete" => Some(Named::Delete),
                                    "backspace" => Some(Named::Backspace),
                                    _ => None,
                                };
                                let key = named.map_or_else(|| iced::keyboard::Key::Character(key.to_lowercase().into()), iced::keyboard::Key::Named);
                                let _ = sim.simulate(iced_test::simulator::tap_key(key, None));
                            }
                            _ => {}
                        }
                            sim.into_messages().collect()
                        };
                        let first = match &target {
                            Some(Ok((_, f))) => Some(f.bounds),
                            _ => None,
                        };
                        let mut messages = run(size, first);
                        // nothing answered: the control may lie past the end of the list it is in,
                        // out of view; the window is laid out tall enough to show it, and the same
                        // control (by its name and role, the same one of those) is clicked there
                        if messages.is_empty() && call.cmd != "ui_key" {
                            if let Some(Ok((k, f))) = &target {
                                let same = |g: &openaec_ui::design::a11y::Found| g.node.name == f.node.name && g.node.role == f.node.role;
                                let before = found[..*k].iter().filter(|g| same(g)).count();
                                let tall = iced::Size::new(size.width, 6000.0);
                                let there = crate::ui_control::controls(self.view(), tall);
                                if let Some(g) = there.iter().filter(|g| same(g)).nth(before) {
                                    messages = run(tall, Some(g.bounds));
                                }
                            }
                        }
                        let said: Vec<String> = messages.iter().map(|m| format!("{m:?}").chars().take(240).collect()).collect();
                        let answered = !messages.is_empty();
                        if dry {
                            Ok(json!({ "control": what, "messages": said, "answered": answered, "dry_run": true }))
                        } else {
                            let tasks: Vec<Task<Msg>> = messages.into_iter().map(|m| self.update(m)).collect();
                            call.answer(Ok(json!({ "control": what, "messages": said, "answered": answered })));
                            return Task::batch(tasks);
                        }
                    }
                }
            }
            other => Err(format!("unknown command {other}")),
        };
        call.answer(answer);
        Task::none()
    }

    fn look(&self) -> Look {
        Look::new(self.prefs.theme(), self.prefs.workspace(), self.prefs.statusbar())
    }

    /// The width of the left panel: the Collaborator's lists need more than the explorer's tree.
    fn left_width(&self) -> f32 {
        if self.product() == Product::Collaborator { self.prefs.left_width.max(340.0) } else { self.prefs.left_width }
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
            // an empty project to model in, with a first level; beside a model it opens in a tab of its own
            Command::new("file.new", l.text("Nieuw", "New"), go(Message::NewModel(l.text("Niveau 0", "Level 0")))).icon(small("file.new"), large("file.new")).shortcut(Shortcut::ctrl('n')),
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
            Command::new("view.views-3d", l.text("Weergaven in 3D", "Views in 3D"), go(Message::ToggleViewsIn3d))
                .icon(small("view.saved"), large("view.saved"))
                .toggle(app.views_in_3d)
                .enabled(loaded),
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
                .enabled(app.has_selection()),
            Command::new("object.isolate", l.text("Isoleren", "Isolate"), go(Message::IsolateSelection))
                .icon(small("object.show"), large("object.show"))
                .shortcut(key('i'))
                .enabled(app.has_selection()),
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
            Command::new("view.ifc", l.text("IFC-structuur", "IFC structure"), go(Message::OpenIfc))
                .icon(small("panel.model-tree"), large("panel.model-tree"))
                .toggle(app.ifc.shown)
                .enabled(loaded),
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
            Command::new("model.new", l.text("Nieuw model", "New model"), go(Message::NewModel(l.text("Niveau 0", "Level 0")))).icon(small("object.add"), large("object.add")),
            Command::new("model.butt", l.text("Stomp", "Butt"), go(Message::Connect(bim_core::ConnectionKind::Butt, l.text("Verbinding", "Connection"))))
                .icon(small("grid.end-couple"), large("grid.end-couple"))
                .enabled(app.selected_linear() == 2),
            Command::new("model.end-plate", l.text("Kopplaat", "End plate"), go(Message::Connect(bim_core::ConnectionKind::EndPlate(bim_core::join::EndPlate::default()), l.text("Kopplaat", "End plate"))))
                .icon(small("frame.combination"), large("frame.combination"))
                .enabled(app.selected_linear() == 2),
            Command::new("model.cut", l.text("Uitsparen", "Cut"), go(Message::Connect(bim_core::ConnectionKind::Cut, l.text("Uitsparing", "Cut-out"))))
                .icon(small("edit.cut"), large("edit.cut"))
                .enabled(app.selected_bodies() == 2),
            Command::new("model.mitre", l.text("Verstek", "Mitre"), go(Message::Connect(bim_core::ConnectionKind::Mitre, l.text("Verbinding", "Connection"))))
                .icon(small("draw.l-shape"), large("draw.l-shape"))
                .enabled(app.selected_linear() == 2),
            Command::new("model.edit", l.text("Bewerken", "Edit"), go(Message::EditMode))
                .icon(small("object.edit"), large("object.edit"))
                .toggle(app.modeller.edit.is_some())
                .enabled(app.wants_tab()),
            Command::new("model.delete", l.text("Verwijderen", "Delete"), go(Message::DeletePicked))
                .icon(small("edit.delete"), large("edit.delete"))
                .enabled(!app.selected_modelled().is_empty()),
        ];
        // the project's levels, views, sheets and library
        use crate::document::PlanKind;
        let plan = |id: &'static str, label: &'static str, kind: PlanKind| {
            Command::new(id, label, go(Message::PickLevelFor((app.views_picker != Some(kind)).then_some(kind)))).icon(small("drawing.plan"), large("drawing.plan")).mode(app.views_picker == Some(kind)).enabled(loaded)
        };
        list.push(Command::new("project.level", l.text("Niveau", "Level"), go(Message::NewLevel(l.text("Niveau", "Level")))).icon(small("resource.level"), large("resource.level")).enabled(loaded));
        list.push(plan("project.floor-plan", l.text("Plattegrond", "Floor plan"), PlanKind::Floor));
        list.push(plan("project.structural-plan", l.text("Constructieplan", "Structural plan"), PlanKind::Structural));
        list.push(plan("project.ceiling-plan", l.text("Plafondplan", "Ceiling plan"), PlanKind::Ceiling));
        list.push(Command::new("project.sheet", l.text("Blad", "Sheet"), go(Message::NewSheet(l.text("Blad", "Sheet")))).icon(small("drawing.sheet-new"), large("drawing.sheet-new")).enabled(loaded));
        list.push(
            Command::new("project.library", l.text("Bibliotheek", "Library"), go(Message::ExplorerTab(crate::app::ExplorerTab::Library)))
                .icon(small("construction.library"), large("construction.library"))
                .mode(app.explorer_tab == crate::app::ExplorerTab::Library),
        );
        // dimensions, texts and sections in the floor plan or on the sheet shown
        let (annotatable, sections) = app.annotatable();
        list.push(
            Command::new("annotate.dimension", l.text("Maatvoering", "Dimension"), go(Message::DimensionTool(!app.dimension_tool)))
                .icon(small("draw.dimension"), large("draw.dimension"))
                .toggle(app.dimension_tool && annotatable)
                .enabled(annotatable),
        );
        // the standard dimensions of the drawing view shown
        let plan_shown = app.shown_drawing();
        list.push(
            Command::new(
                "annotate.standard-dimensions",
                l.text("Standaard maatvoering", "Standard dimensions"),
                plan_shown.clone().map_or(Msg::Dialog(None), |v| go(Message::StandardDimensions(v))),
            )
            .icon(small("draw.dimension"), large("draw.dimension"))
            .enabled(plan_shown.is_some()),
        );
        list.push(
            Command::new("annotate.text", l.text("Tekst", "Text"), go(Message::NoteTool(!app.note_tool)))
                .icon(small("draw.text"), large("draw.text"))
                .toggle(app.note_tool && annotatable)
                .enabled(annotatable),
        );
        list.push(
            Command::new("annotate.section", l.text("Doorsnede", "Section"), go(Message::SectionTool(!app.section_tool)))
                .icon(small("drawing.section"), large("drawing.section"))
                .toggle(app.section_tool && sections)
                .enabled(sections),
        );
        for tool in crate::app::modeller::ModelTool::ALL.into_iter().chain(crate::app::modeller::ModelTool::OPENINGS) {
            let on = app.modeller.tool.is_some_and(|t| t.0 == tool);
            list.push(Command::new(tool.id(), tool.label(l), go(Message::ModelTool(Some((tool, tool.label(l)))))).icon(small(tool.icon()), large(tool.icon())).mode(on).enabled(loaded));
        }
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
        // the switch at the top right; no shortcuts of their own until one is set
        let shown = self.product();
        for &product in &self.products {
            list.push(Command::new(product.command(), product.label(), Msg::Product(product)).mode(product == shown).detail(product.purpose(l)));
        }
        // the Collaborator's commands: issues, checks, clashes and the rule set
        for planned in PLANNED {
            #[cfg(all(feature = "collaborator", not(target_arch = "wasm32")))]
            let message = planned.event().map(|e| go(Message::Collab(e)));
            #[cfg(not(all(feature = "collaborator", not(target_arch = "wasm32"))))]
            let message: Option<Msg> = None;
            list.push(Command::new(planned.id, planned.label(l), message.clone().unwrap_or(Msg::Dialog(None))).icon(small(planned.icon), large(planned.icon)).enabled(message.is_some()).detail(planned.detail(l)));
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
        #[cfg(all(feature = "collaborator", not(target_arch = "wasm32")))]
        {
            self.app.collab.language = self.l();
            self.app.collab.refresh();
        }
        // the app knows whether the Modeller is shown: there an opened file's walls have grips
        self.app.modeller.in_modeller = self.product() == Product::Modeller;
        let task = self.handle(message);
        self.app.modeller.in_modeller = self.product() == Product::Modeller;
        self.keep_dev_session();
        task
    }

    fn handle(&mut self, message: Msg) -> Task<Msg> {
        match message {
            // the window driven as a person drives it: its controls, a click, typing
            #[cfg(not(target_arch = "wasm32"))]
            Msg::App(Message::Remote(call)) if call.cmd.starts_with("ui_") => return self.ui_command(call),
            Msg::App(message) => {
                // what opens in the explorer shows it
                if matches!(message, Message::NewLevel(_) | Message::PickLevelFor(Some(_)) | Message::ExplorerTab(_)) && !self.prefs.left {
                    self.prefs.left = true;
                    self.save_prefs();
                }
                // joins made automatically are named in the interface language
                self.app.modeller.join_word = self.l().text("Verbinding", "Connection");
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
                if matches!(message, Message::Open(_) | Message::Picked(Some(_)) | Message::NewModel(_)) && self.app.busy() {
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
            Msg::Product(product) => self.set_product(product),
            // choosing a ribbon tab closes Bestand; in the compact layout choosing the tab shown
            // folds the ribbon away and back
            Msg::RibbonTab(tab) => {
                let same = tab == self.ribbon_tab;
                self.ribbon_folded = self.compact() && !self.backstage && same && !self.ribbon_folded;
                self.ribbon_tab = tab;
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
            Msg::Sized(size) => {
                self.width = size.width;
                self.height = size.height;
                self.sized = true;
            }
            #[cfg(not(target_arch = "wasm32"))]
            Msg::SizedFor(size, call) => {
                let _ = self.update(Msg::Sized(size));
                return self.ui_command(call);
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
            // document tabs: the 3D view, the open schedules, then the open sheets
            Msg::SelectTab(i) => return self.update(Msg::App(Message::SelectTab(i))),
            // the 3D view (tab 0) stays open
            Msg::CloseTab(i) => return self.update(Msg::App(Message::CloseTab(i))),
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
                // in edit mode G, X, Y, Z and A are its own keys
                if self.dialog.is_none() && self.app.modeller.edit.is_some() && !press.modifiers.control() && !press.modifiers.alt() {
                    if let Key::Character(c) = &press.key {
                        if let Some(c) = c.chars().next().filter(|c| "gxyzaGXYZA".contains(*c)) {
                            return self.update(Msg::App(Message::EditKey(c)));
                        }
                    }
                }
                if self.dialog.is_none() {
                    if let Some(message) = cm::find(&self.commands(), &press.key, press.modifiers) {
                        return self.update(message);
                    }
                }
            }
            // Tab on a selected modelled element opens its edit mode, as in Blender; else it moves
            // the focus
            Msg::Navigate(openaec_ui::keyboard::Navigation::Next) if self.dialog.is_none() && !self.backstage && self.app.wants_tab() => {
                return self.update(Msg::App(Message::EditMode));
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
            Msg::ResizeEdge(direction) => return iced::window::latest().and_then(move |id| iced::window::drag_resize(id, direction)),
            Msg::WindowOpened(id) => return iced::window::raw_id::<Msg>(id).map(Msg::NativeWindow),
            Msg::NativeWindow(_raw) => {
                // between displays with a different scale the window keeps its size (`dpi_drag`)
                #[cfg(windows)]
                crate::dpi_drag::install(_raw);
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
            iced::Event::Window(iced::window::Event::Opened { size, .. } | iced::window::Event::Resized(size)) => Some(Msg::Sized(size)),
            _ => None,
        });
        let opened = iced::event::listen_with(|event, _, id| match event {
            iced::Event::Window(iced::window::Event::Opened { .. }) => Some(Msg::WindowOpened(id)),
            _ => None,
        });
        Subscription::batch([
            self.app.subscription().map(Msg::App),
            resized,
            opened,
            open.map(Msg::App),
            drag,
            openaec_ui::keyboard::presses().map(Msg::Key),
            openaec_ui::keyboard::navigation().map(Msg::Navigate),
            iced::window::close_requests().map(|_| Msg::CloseWindow),
        ])
    }

    pub fn title(&self) -> String {
        let title = match &self.app.model {
            Some(m) => format!("{} · {}", m.file_name(), self.edition.name()),
            None => self.edition.name().into(),
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
                Some(format) => format!(
                    "{}: {}",
                    l.text("Fout", "Error"),
                    l.text("{format} vereist de desktop-app", "{format} needs the desktop app").replace("{format}", format)
                ),
                None => format!("{}: {e}", l.text("Fout", "Error")),
            },
        }
    }

    pub fn view(&self) -> Element<'_, Msg> {
        let look = self.look();
        let l = self.l();
        let c = look.color;
        // the ribbon's tabs are the product's
        let tabs: Vec<&str> = self.product().pages().iter().map(|p| p.label(l)).collect();
        let mut left = vec![self.status_text()];
        let selected = self.app.selection.len() + self.app.structural.selected.len();
        if selected > 0 {
            left.push(format!("{selected} {}", l.text("geselecteerd", "selected")));
        }
        let compact = self.compact();
        let folded = compact && self.ribbon_folded;
        let ribbon: Element<'_, Msg> = if folded { Space::new().into() } else { self.ribbon(look) };
        // a phone has no room for the version, and the web app has no remote control
        let status: Element<'_, Msg> = if compact { w::status_bar(look, vec![one_line(&left.join(" · "), self.width)], Vec::new()) } else { row![w::status_bar(look, left, vec![format!("{} {VERSION}", self.edition.name())]), self.mcp_lamp(look)].into() };
        let screen = column![
            self.title_bar(look),
            w::ribbon_tabs(look, l.text("Bestand", "File"), Some(Msg::Backstage(!self.backstage)), &tabs, if folded { usize::MAX } else { self.ribbon_tab.min(tabs.len() - 1) }, Msg::RibbonTab),
            // Bestand takes the place of the ribbon and the documents until it closes
            if self.backstage { self.backstage_view(look) } else { column![ribbon, self.document_area(look)].into() },
            status,
        ]
        .height(Length::Fill);
        let mut layers = vec![focus::inert(container(screen).style(move |_| container::Style { background: Some(c.frame.into()), ..Default::default() }), self.dialog.is_some())];
        if let Some(dialog) = self.dialog {
            layers.push(mouse_area(w::overlay(look, self.dialog_card(look, dialog))).into());
        }
        // the window has no system frame (the shell draws the title bar), so its edges resize it here
        crate::resize_frame::resize_frame(iced::widget::Stack::with_children(layers), !self.maximized && cfg!(not(target_arch = "wasm32")), Msg::ResizeEdge)
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
        let quick: Vec<Element<'_, Msg>> = ["file.new", "file.open", "file.save-as"].iter().map(|id| cm::icon_button(look, &self.command(id), 28.0)).collect();
        let mut extra = Vec::new();
        if let Some(available) = &self.update {
            extra.push(container(upd::notice(look, available, Msg::Update)).padding([0, 8]).into());
        }
        if self.compact() {
            // the panels have no room beside the 3D view: these buttons swap them in and out
            extra.push(cm::icon_button(look, &self.command("view.left"), 28.0));
            extra.push(cm::icon_button(look, &self.command("view.right"), 28.0));
        }
        // the products, at the top right
        if self.products.len() > 1 {
            extra.push(self.product_switch(look));
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

    /// The switch between the products of this build: a segment per product, the one shown marked.
    /// A phone has room for one button: the product shown, which moves on to the next.
    fn product_switch(&self, look: Look) -> Element<'_, Msg> {
        use iced::widget::{button, text};
        use openaec_ui::design::a11y::{self, Node, Role};
        use openaec_ui::design::tokens::{font, radius};
        let l = self.l();
        let c = look.color;
        let shown = self.product();
        let segment = move |product: Product, on: bool, message: Msg| -> Element<'_, Msg> {
            let label = text(product.label()).size(font::SIZE_BODY).font(if on { fonts::STRONG } else { fonts::MEDIUM }).color(if on { c.text } else { c.text_secondary });
            let control = button(container(label).height(Length::Fill).align_y(iced::alignment::Vertical::Center))
                .height(22)
                .padding([0, 10])
                .on_press(message)
                .style(move |_, status| button::Style {
                    snap: false,
                    background: if on {
                        Some(c.selected.into())
                    } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                        Some(c.hover.into())
                    } else {
                        None
                    },
                    text_color: c.text,
                    border: iced::Border { color: if on { c.accent } else { iced::Color::TRANSPARENT }, width: if on { 1.0 } else { 0.0 }, radius: (radius::MEDIUM - 2.0).into() },
                    shadow: iced::Shadow::default(),
                });
            a11y::node(focus::focusable(look, control, true, radius::MEDIUM - 2.0), Node::new(Role::Tab, product.label()).selected(on))
        };
        if self.compact() {
            let next = self.products[(self.products.iter().position(|&p| p == shown).unwrap_or(0) + 1) % self.products.len()];
            return w::with_tooltip(look, segment(shown, true, Msg::Product(next)), next.purpose(l), None);
        }
        let segments = self.products.iter().map(|&p| w::with_tooltip(look, segment(p, p == shown, Msg::Product(p)), p.purpose(l), None));
        container(row(segments).spacing(2).align_y(iced::Alignment::Center))
            .padding(2)
            .style(move |_| container::Style { border: iced::Border { color: c.border, width: 1.0, radius: radius::MEDIUM.into() }, ..Default::default() })
            .into()
    }

    fn ribbon(&self, look: Look) -> Element<'_, Msg> {
        let l = self.l();
        let large = |id: &str| cm::ribbon_large(look, &self.command(id));
        let smalls = |ids: &[&str]| -> Element<'_, Msg> { column(ids.iter().map(|id| cm::ribbon_small(look, &self.command(id)))).spacing(2).into() };
        // One tab: the main command of a group large, the rest small in columns of three
        let tools: Vec<&str> = Tool::ALL.iter().map(|t| Self::tool_id(*t)).collect();
        let (first_tools, more_tools) = tools.split_at(3.min(tools.len()));
        // the Viewer's tools, also the View tab of the Modeller and the Collaborator
        let view_groups = || {
            let mut measure = vec![smalls(first_tools)];
            if !more_tools.is_empty() {
                measure.push(smalls(more_tools));
            }
            measure.push(smalls(&["measure.clear", "measure.clear-saved"]));
            vec![
                w::ribbon_group(look, l.text("Weergave", "Display"), vec![smalls(&["view.solid", "view.wireframe", "view.edges"])]),
                w::ribbon_group(look, l.text("Tonen", "Show"), vec![smalls(&["view.grid", "view.grid-lines", "view.views-3d", "view.flip-up"])]),
                w::ribbon_group(look, "IFC", vec![large("view.ifc")]),
                w::ribbon_group(look, l.text("Camera", "Camera"), vec![large("camera.fit"), smalls(&["camera.iso", "camera.top", "camera.front"]), smalls(&["camera.right", "camera.perspective", "camera.ortho"])]),
                w::ribbon_group(look, l.text("Selectie", "Selection"), vec![smalls(&["object.hide", "object.isolate", "object.show-all"]), smalls(&["selection.layer"])]),
                w::ribbon_group(look, l.text("Doorsnede", "Section"), vec![large("section.box"), large("section.plane"), smalls(&["section.flip", "section.selection", "section.fill"]), smalls(&["section.reset"])]),
                w::ribbon_group(look, l.text("Meten", "Measure"), measure),
                w::ribbon_group(look, l.text("Extra", "More"), vec![large("view.explode"), smalls(&["view.left", "view.right"])]),
            ]
        };
        let groups = match self.ribbon_page() {
            RibbonPage::Home => {
                let mut groups = vec![w::ribbon_group(look, l.text("Bestand", "File"), vec![large("file.new"), large("file.open"), smalls(&["file.add", "file.save-as"])])];
                groups.extend(view_groups());
                groups
            }
            RibbonPage::View => view_groups(),
            // what is built, in the order of the five base elements: planar (wall, slab), linear
            // (column, beam), system, point; then the openings in walls, the connection that joins
            // two selected elements, and changes. New, open and save are on the File page and in
            // the title bar, as in other BIM modellers.
            RibbonPage::Model => vec![
                w::ribbon_group(look, l.text("Vlak", "Planar"), vec![large("model.wall"), large("model.slab")]),
                w::ribbon_group(look, l.text("Lijnvormig", "Linear"), vec![large("model.column"), large("model.beam")]),
                w::ribbon_group(look, l.text("Systeem", "System"), vec![large("model.system")]),
                w::ribbon_group(look, l.text("Puntvormig", "Point"), vec![large("model.point")]),
                w::ribbon_group(look, l.text("Openingen", "Openings"), vec![large("model.door"), large("model.window")]),
                w::ribbon_group(look, l.text("Verbinding", "Connection"), vec![smalls(&["model.butt", "model.mitre", "model.cut"]), smalls(&["model.end-plate"])]),
                w::ribbon_group(look, l.text("Wijzigen", "Modify"), vec![large("model.edit"), large("model.delete")]),
            ],
            // what the model is drawn on and in: its levels, its views and sheets, its types
            RibbonPage::Project => vec![
                w::ribbon_group(look, l.text("Niveaus", "Levels"), vec![large("project.level")]),
                w::ribbon_group(look, l.text("Weergaven", "Views"), vec![large("project.floor-plan"), smalls(&["project.structural-plan", "project.ceiling-plan", "annotate.section"])]),
                w::ribbon_group(look, l.text("Bladen", "Sheets"), vec![large("project.sheet")]),
                w::ribbon_group(look, l.text("Bibliotheek", "Library"), vec![large("project.library")]),
            ],
            // in the floor plan, section or sheet shown
            RibbonPage::Annotate => vec![
                w::ribbon_group(look, l.text("Maatvoering", "Dimension"), vec![large("annotate.dimension"), smalls(&["annotate.standard-dimensions"])]),
                w::ribbon_group(look, l.text("Tekst", "Text"), vec![large("annotate.text")]),
            ],
            // the project's models together; issues, checks, comments and sharing to come
            RibbonPage::Collaborate => vec![
                w::ribbon_group(look, l.text("Modellen", "Models"), vec![large("file.open"), smalls(&["file.add", "file.save-as"])]),
                w::ribbon_group(look, l.text("Issues", "Issues"), vec![large("collaborate.issue"), smalls(&["collaborate.bcf-import", "collaborate.bcf-export"])]),
                w::ribbon_group(look, l.text("Controle", "Check"), vec![large("collaborate.check"), smalls(&["collaborate.ids", "collaborate.report"])]),
                w::ribbon_group(look, l.text("Clashes", "Clashes"), vec![large("collaborate.clash")]),
                w::ribbon_group(look, l.text("Regelset", "Rule set"), vec![smalls(&["collaborate.rules-open", "collaborate.rules-save"])]),
            ],
        };
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

    /// Bestand: new, open, save as, settings, and the files opened last.
    fn backstage_view(&self, look: Look) -> Element<'_, Msg> {
        let l = self.l();
        let c = look.color;
        let loaded = self.app.model.is_some();
        let nav = column![
            t::file_nav_row(look, small("file.new"), l.text("Nieuw", "New"), false, Some(Msg::App(Message::NewModel(l.text("Niveau 0", "Level 0"))))),
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
            self.edition.name(),
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
            // each product starts its own way: open a model, make one, or open the project's
            let open = |kind| cm::button(look, &self.command("file.open"), kind, D::Regular);
            let (icon, title, explanation, action): (_, _, _, Element<'_, Msg>) = match self.product() {
                Product::Viewer => (
                    "project.folder",
                    l.text("Geen model geopend", "No model open"),
                    formats_hint(l),
                    // open a model, or start an empty project to model in
                    row![open(K::Primary), cm::button(look, &self.command("file.new"), K::Secondary, D::Regular)].spacing(8).into(),
                ),
                Product::Modeller => (
                    "object.add",
                    l.text("Nog geen model", "No model yet"),
                    l.text("Begin een nieuw model, of open een IFCX-bestand om verder te modelleren.", "Start a new model, or open an IFCX file to model on."),
                    row![cm::button(look, &self.command("model.new"), K::Primary, D::Regular), open(K::Secondary)].spacing(8).into(),
                ),
                Product::Collaborator => (
                    "team.add",
                    l.text("Geen modellen geopend", "No models open"),
                    l.text(
                        "Open de modellen van het project om ze samen te bekijken, te controleren en issues af te spreken.",
                        "Open the project's models to look at them together, check them and agree on issues.",
                    ),
                    open(K::Primary),
                ),
            };
            container(w::empty_state(look, large(icon), title, explanation, Some(action)))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(move |_| container::Style { background: Some(look.color.workspace.into()), ..Default::default() })
            .into()
        } else {
            app.workspace_view(p, l).map(Msg::App)
        };
        // document tabs once a view, schedule or sheet is open: the model (the 3D view, pinned),
        // the floor plans, the schedules and the sheets
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
        let doc_tabs = app.doc_tabs();
        if app.model.is_some() && doc_tabs.len() > 1 {
            let tabs: Vec<DocumentTab<'_>> = doc_tabs
                .iter()
                .map(|&t| if t == DocTab::Model { DocumentTab::pinned(model_name) } else { DocumentTab::new(app.tab_title(t), false) })
                .collect();
            middle = middle.push(w::document_tabs(look, &tabs, app.active_doc_tab(), small("window.close"), Msg::SelectTab, Msg::CloseTab, Msg::SelectTab));
        }
        let middle = middle.push(workspace);
        if self.compact() {
            // one panel at a time, the full width, in place of the 3D view
            let width = self.width;
            return match self.compact_panel {
                Some(Side::Left) => {
                    let hide = l.text("Verkenner verbergen", "Hide explorer");
                    let actions = vec![w::icon_button(look, small("panel.left"), 20.0, true, hide, Some(Msg::ToggleLeft))];
                    let (title, body) = self.left_panel(look);
                    w::panel(look, title, width, actions, body)
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
            let (title, content) = self.left_panel(look);
            body = body
                .push(w::panel(look, title, self.left_width(), actions, content))
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

    /// The left panel of the product shown, with its title: the explorer, or in the Collaborator
    /// the project's models, issues and checks.
    fn left_panel(&self, look: Look) -> (&'static str, Element<'_, Msg>) {
        let l = self.l();
        match self.product() {
            Product::Collaborator => {
                #[cfg(all(feature = "collaborator", not(target_arch = "wasm32")))]
                let body = container(Self::pane(self.app.collab_body(look, l))).padding([4, 8]).width(Length::Fill).height(Length::Fill);
                #[cfg(not(all(feature = "collaborator", not(target_arch = "wasm32"))))]
                let body = {
                    let models: Vec<&str> = self.app.models.iter().map(|m| m.model.path.file_name().and_then(|n| n.to_str()).unwrap_or("3D")).collect();
                    container(crate::product::collaboration_panel(look, l, models)).padding([4, 8]).width(Length::Fill).height(Length::Fill)
                };
                (l.text("Samenwerken", "Collaboration"), body.into())
            }
            Product::Viewer | Product::Modeller => (l.text("Verkenner", "Explorer"), Self::pane(self.app.explorer_body(palette(look), l))),
        }
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
    l.text(
        "{format}-bestanden openen alleen in de desktop-app; de browserversie kan ze niet lezen. De desktop-app is gratis, voor Windows, macOS en Linux: Downloaden opent de releasepagina op GitHub.",
        "{format} files open only in the desktop app; the browser version cannot read them. The desktop app is free, for Windows, macOS and Linux: Download opens the releases page on GitHub.",
    )
    .replace("{format}", format)
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
    static HINTS: std::sync::OnceLock<[String; LANGUAGES]> = std::sync::OnceLock::new();
    let hints = HINTS.get_or_init(|| {
        let (featured, others) = crate::loaders::featured_formats();
        Language::ALL.map(|l| hint_text(l, &featured, others))
    });
    &hints[l.index()]
}

/// The start screen's hint in language `l`: the `featured` format names, then how many `others`.
fn hint_text(l: Language, featured: &[&str], others: usize) -> String {
    let or = l.text("of", "or");
    let mut items: Vec<String> = featured.iter().map(|s| s.to_string()).collect();
    match others {
        0 => {}
        1 => items.push(l.text("een ander formaat", "one other format").into()),
        n => items.push(l.text("een van {n} andere formaten", "one of {n} other formats").replace("{n}", &n.to_string())),
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
    let studio = Studio { update: if crate::managed::managed() { None } else { upd::check_installed(studio.edition.app_id(), VERSION) }, ..studio };
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

    /// Sheets and views open as document tabs of the application's own window, beside the 3D
    /// view's, on the desktop as in the browser.
    #[test]
    fn sheets_and_views_are_tabs_of_the_main_window() {
        let mut studio = Studio::new(App::default());
        studio.prefs.language = Language::English;
        let _ = studio.update(Msg::App(Message::Loaded(Ok(triangle("a.rvt")))));
        let _ = studio.update(Msg::App(Message::NewSheet("Sheet")));
        assert_eq!(studio.app.doc_tabs(), [DocTab::Model, DocTab::DocSheet(0)]);
        assert_eq!(studio.app.active_doc_tab(), 1);
        let names = accessibility(&studio).summary();
        assert!(names.iter().any(|n| n.contains("A01 Sheet")), "{names:?}");
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

    /// File, New (Ctrl+N): an empty project with a first level, in the empty project shown, else
    /// in a tab of its own beside the model.
    #[test]
    fn new_starts_an_empty_project_with_a_level_beside_the_model_open() {
        use iced::keyboard::{Key, Modifiers};
        let ctrl_n = Msg::Key(KeyPress { key: Key::Character("n".into()), modifiers: Modifiers::CTRL });
        let mut studio = Studio::new(App::default());
        studio.prefs.language = Language::English;
        let _ = studio.update(ctrl_n.clone());
        assert_eq!(studio.tabs.len(), 1, "in the empty project");
        let model = studio.app.model.clone().expect("an empty model");
        assert!(model.layers.is_empty());
        assert_eq!(studio.app.document.levels().iter().map(|l| (l.name.as_str(), l.elevation)).collect::<Vec<_>>(), [("Level 0", 0.0)]);
        // beside it, the next new project in a tab of its own, shown
        let first = studio.project;
        let _ = studio.update(ctrl_n);
        assert_eq!(studio.tabs.len(), 2);
        assert_ne!(studio.project, first);
        assert_ne!(studio.app.model.as_ref().map(|m| m.id), Some(model.id));
        assert_eq!(studio.app.document.levels().len(), 1);
        // the File page has it first, and the start page beside Open
        studio.backstage = true;
        let names = accessibility(&studio).summary();
        assert!(names.iter().any(|n| n.contains("New")), "{names:?}");
        let mut blank = Studio::new(App::default());
        blank.prefs.language = Language::English;
        let names = accessibility(&blank).summary();
        assert!(names.iter().any(|n| n.contains("New")) && names.iter().any(|n| n.contains("Open…")), "{names:?}");
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
    #[ignore = "writes PNG files"]
    fn export_modeller_renders() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/shell-renders");
        std::fs::create_dir_all(&dir).expect("folder");
        for l in Language::ALL {
            let mut app = App::default();
            app.add_demo_building();
            // the wall with the door and the window, selected
            app.select_modelled(bim_core::ElementId(2));
            let mut studio = Studio::new(app);
            studio.prefs.language = l;
            studio.set_product(Product::Modeller);
            let _ = studio.update(Msg::App(Message::ModelTool(Some((crate::app::modeller::ModelTool::Wall, "Wall")))));
            studio.snapshot(iced::Size::new(1440.0, 900.0), &dir.join(format!("modeller-{}.png", l.code()))).expect("snapshot");
        }
        // edit mode (Tab) on the slab, its far corner picked
        let mut app = App::default();
        app.add_demo_building();
        app.select_modelled(bim_core::ElementId(1));
        let mut studio = Studio::new(app);
        studio.prefs.language = Language::Dutch;
        studio.set_product(Product::Modeller);
        let _ = studio.update(Msg::App(Message::EditMode));
        let _ = studio.update(Msg::App(Message::Edit(crate::app::edit_press(2))));
        studio.snapshot(iced::Size::new(1440.0, 900.0), &dir.join("modeller-edit-nl.png")).expect("snapshot");
        // the selected wall's grips, without edit mode
        let mut app = App::default();
        app.add_demo_building();
        app.select_modelled(bim_core::ElementId(2));
        let mut studio = Studio::new(app);
        studio.prefs.language = Language::Dutch;
        studio.set_product(Product::Modeller);
        studio.snapshot(iced::Size::new(1440.0, 900.0), &dir.join("modeller-grips-nl.png")).expect("snapshot");
        // a wall being placed: from the building's corner to the cursor
        let _ = studio.update(Msg::App(Message::ModelTool(Some((crate::app::modeller::ModelTool::Wall, "Wand")))));
        studio.app.modeller.points = vec![glam::DVec3::new(10.0, 0.0, 0.0)];
        studio.app.modeller.cursor = Some(glam::DVec3::new(10.0, -4.0, 0.0));
        studio.snapshot(iced::Size::new(1440.0, 900.0), &dir.join("modeller-placing-nl.png")).expect("snapshot");
        // a window aimed at the front wall's face
        let _ = studio.update(Msg::App(Message::ModelTool(Some((crate::app::modeller::ModelTool::Window, "Raam")))));
        let (w, h) = studio.viewport_in(1440.0, 900.0);
        studio.app.viewport_size = (w, h);
        let (x, y) = studio.app.on_screen(glam::DVec3::new(7.0, -0.1, 1.5), w, h).expect("on screen");
        let _ = studio.update(Msg::App(Message::Navigate(crate::viewport::Navigate::Hover { x, y, width: w, height: h })));
        assert!(studio.app.modeller.aim.is_some(), "the cursor is over the wall");
        studio.snapshot(iced::Size::new(1440.0, 900.0), &dir.join("modeller-window-nl.png")).expect("snapshot");
        // the Project tab, and the Annotate tab in a floor plan
        let _ = studio.update(Msg::App(Message::ModelTool(None)));
        studio.ribbon_tab = 1;
        studio.snapshot(iced::Size::new(1440.0, 900.0), &dir.join("modeller-project-nl.png")).expect("snapshot");
        let level: std::sync::Arc<str> = studio.app.document.levels()[0].path.as_str().into();
        let _ = studio.update(Msg::App(Message::NewPlan(level)));
        studio.ribbon_tab = 2;
        studio.snapshot(iced::Size::new(1440.0, 900.0), &dir.join("modeller-annotate-nl.png")).expect("snapshot");
        // a wall of an opened Revit file: its grips in the Modeller, then taken over by a drag of
        // its end, the project now an IFCX document
        let mut app = App::default();
        app.set_model(crate::app::revit_house());
        let mut studio = Studio::new(app);
        studio.prefs.language = Language::Dutch;
        studio.set_product(Product::Modeller);
        studio.app.modeller.in_modeller = true;
        studio.app.click(Some(crate::selection::Pickable::Element(7)), false);
        studio.snapshot(iced::Size::new(1440.0, 900.0), &dir.join("modeller-revit-wall-nl.png")).expect("snapshot");
        let _ = studio.update(Msg::App(Message::Edit(crate::app::edit_press(1))));
        if let Some(g) = studio.app.modeller.edit.as_mut().and_then(|e| e.grab.as_mut()) {
            g.delta = glam::DVec3::new(2.0, 0.0, 0.0);
        }
        let _ = studio.update(Msg::App(Message::Edit(crate::app::edit_release())));
        studio.snapshot(iced::Size::new(1440.0, 900.0), &dir.join("modeller-revit-taken-nl.png")).expect("snapshot");
        // the end plate joining the pergola's beam to its column, selected, seen close up
        let mut app = App::default();
        app.add_demo_building();
        let plate = app.modeller.model.iter().find(|e| e.class() == "IfcElementAssembly").map(|e| e.id).expect("the demo's end plate");
        app.select_modelled(plate);
        app.camera.target = glam::Vec3::new(13.3, 0.0, 2.6);
        app.camera.distance = 3.5;
        app.camera.yaw = -0.9;
        app.camera.pitch = 0.35;
        let mut studio = Studio::new(app);
        studio.prefs.language = Language::Dutch;
        studio.set_product(Product::Modeller);
        studio.snapshot(iced::Size::new(1440.0, 900.0), &dir.join("modeller-end-plate-nl.png")).expect("snapshot");
    }

    /// The Library tab with a build-up and a steel profile picked, in every language:
    /// `cargo test --lib export_library_renders -- --ignored`.
    #[test]
    #[ignore = "writes PNG files"]
    fn export_library_renders() {
        use crate::app::library::{Group, LibraryMessage, Pick};
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/shell-renders");
        std::fs::create_dir_all(&dir).expect("folder");
        let shots: [(&str, Group, Pick); 4] = [
            ("wall", Group::BuildUps(bim_library::Role::Wall), Pick::BuildUp("wall.timber-frame-outer".into())),
            ("steel", Group::Series(bim_library::Series::Hea), Pick::Steel("HEA 200".into())),
            ("masonry", Group::SystemGroup(bim_library::SystemGroup::Masonry), Pick::System("system.masonry-waal-stretcher".into())),
            ("timber-frame", Group::SystemGroup(bim_library::SystemGroup::TimberFrame), Pick::System("system.timber-frame-140".into())),
        ];
        for l in Language::ALL {
            for (name, group, pick) in shots.clone() {
                let mut app = App::default();
                app.explorer_tab = crate::app::ExplorerTab::Library;
                match group {
                    Group::Series(_) => {
                        let _ = app.update(Message::Library(LibraryMessage::Toggle(Group::Steel)));
                    }
                    Group::SystemGroup(_) => {
                        let _ = app.update(Message::Library(LibraryMessage::Toggle(Group::Systems)));
                    }
                    _ => {}
                }
                let _ = app.update(Message::Library(LibraryMessage::Toggle(group)));
                let _ = app.update(Message::Library(LibraryMessage::Pick(pick)));
                let mut studio = Studio::new(app);
                studio.prefs.language = l;
                studio.snapshot(iced::Size::new(1440.0, 900.0), &dir.join(format!("library-{name}-{}.png", l.code()))).expect("snapshot");
            }
        }
    }

    /// The three products, empty and with the demo building: target/shell-renders/product-*.png.
    #[test]
    #[ignore = "writes PNG files"]
    fn export_product_renders() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/shell-renders");
        std::fs::create_dir_all(&dir).expect("folder");
        for product in Product::ALL {
            for model in [false, true] {
                let mut app = App::default();
                if model {
                    app.add_demo_building();
                }
                let mut studio = Studio::new(app);
                studio.prefs.language = Language::Dutch;
                studio.set_product(product);
                let name = format!("product-{}{}.png", product.key(), if model { "" } else { "-leeg" });
                studio.snapshot(iced::Size::new(1440.0, 900.0), &dir.join(name)).expect("snapshot");
            }
        }
    }

    #[test]
    fn every_setting_has_a_name_in_every_language() {
        for i in 0..LANGUAGES {
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
        studio.update = Some(Available { app: studio.edition.app_id().into(), current: VERSION.into(), latest: "2099.1.0".into(), page_url: None, installer: None });
        for (backstage, dialog, page) in [(false, None, SettingsPage::General), (true, None, SettingsPage::General), (false, Some(Dialog::Settings), SettingsPage::General), (false, Some(Dialog::Settings), SettingsPage::Shortcuts), (false, Some(Dialog::DesktopOnly("Revit")), SettingsPage::General)] {
            studio.backstage = backstage;
            studio.dialog = dialog;
            studio.settings_page = page;
            let unnamed = accessibility(&studio).unnamed;
            assert!(unnamed.is_empty(), "unnamed controls with backstage {backstage}, dialog {dialog:?}, {page:?}: {unnamed:?}");
        }
        // every product's ribbon tabs and panels, also in the compact layout
        studio.dialog = None;
        studio.backstage = false;
        for width in [1280.0, 400.0] {
            studio.width = width;
            for product in Product::ALL {
                studio.set_product(product);
                for tab in 0..product.pages().len() {
                    studio.ribbon_tab = tab;
                    let unnamed = accessibility(&studio).unnamed;
                    assert!(unnamed.is_empty(), "unnamed controls in {product:?}, tab {tab}, width {width}: {unnamed:?}");
                }
            }
        }
    }

    #[test]
    fn the_modellers_project_and_annotate_tabs_open_levels_views_and_marks() {
        use crate::app::ExplorerTab;
        let mut app = App::default();
        app.add_demo_building();
        let mut studio = Studio::new(app);
        studio.set_product(Product::Modeller);
        studio.prefs.left = false;
        // Level opens the form of a new level in the explorer's Levels tab, and shows the explorer
        let _ = studio.update(studio.command("project.level").message);
        assert!(studio.prefs.left && studio.app.explorer_tab == ExplorerTab::Levels && studio.app.level_form.is_some());
        // Floor plan lists the levels to make one of, in the Views tab; again closes the list
        let _ = studio.update(studio.command("project.floor-plan").message);
        assert_eq!((studio.app.explorer_tab, studio.app.views_picker), (ExplorerTab::Views, Some(crate::document::PlanKind::Floor)));
        let _ = studio.update(studio.command("project.floor-plan").message);
        assert_eq!(studio.app.views_picker, None);
        let _ = studio.update(studio.command("project.library").message);
        assert_eq!(studio.app.explorer_tab, ExplorerTab::Library);
        // dimensions, texts and sections need a floor plan (or a sheet) shown
        for id in ["annotate.dimension", "annotate.standard-dimensions", "annotate.text", "annotate.section"] {
            assert!(!studio.command(id).enabled, "{id} without a plan");
        }
        let level: std::sync::Arc<str> = studio.app.document.levels()[0].path.as_str().into();
        let _ = studio.update(Msg::App(Message::NewPlan(level)));
        for id in ["annotate.dimension", "annotate.standard-dimensions", "annotate.text", "annotate.section"] {
            assert!(studio.command(id).enabled, "{id} in a plan");
        }
        let _ = studio.update(studio.command("annotate.dimension").message);
        assert!(studio.app.dimension_tool && studio.command("annotate.dimension").kind == openaec_ui::design::command::CommandKind::Toggle(true));
        // the door and window tools are on the Model tab
        assert!(studio.command("model.door").enabled && studio.command("model.window").enabled);
    }

    #[test]
    fn the_switch_at_the_top_right_shows_each_product_with_its_own_ribbon() {
        let mut studio = Studio::new(App::default());
        assert_eq!(studio.product(), Product::Viewer);
        let tabs = |studio: &Studio| -> Vec<String> { accessibility(studio).summary().into_iter().filter(|n| n.starts_with("Tab: ")).collect() };
        // the switch names the three products; the Viewer's ribbon has one tab, without modelling
        let shown = tabs(&studio);
        for product in Product::ALL {
            assert!(shown.contains(&format!("Tab: {}", product.label())), "{shown:?}");
        }
        assert!(shown.contains(&"Tab: Home".to_string()) && !shown.contains(&"Tab: Model".to_string()), "{shown:?}");
        let _ = studio.update(Msg::Product(Product::Modeller));
        assert_eq!((studio.product(), studio.ribbon_page()), (Product::Modeller, RibbonPage::Model));
        let shown = tabs(&studio);
        for tab in ["Tab: Model", "Tab: Project", "Tab: Annotate", "Tab: View"] {
            assert!(shown.contains(&tab.to_string()), "{shown:?}");
        }
        let _ = studio.update(Msg::RibbonTab(3));
        assert_eq!(studio.ribbon_page(), RibbonPage::View);
        // another product opens on its first tab
        let _ = studio.update(Msg::Product(Product::Collaborator));
        assert_eq!(studio.ribbon_page(), RibbonPage::Collaborate);
        assert_eq!(studio.left_panel(studio.look()).0, "Collaboration");
        // the commands to come are there but do nothing yet; the switch marks the product shown
        let commands = studio.default_commands();
        for planned in PLANNED {
            assert!(commands.iter().any(|c| c.id == planned.id && c.enabled), "{}", planned.id);
        }
        assert!(commands.iter().any(|c| c.id == Product::Collaborator.command() && c.kind == openaec_ui::design::command::CommandKind::Mode(true)));
    }

    #[test]
    fn leaving_the_modeller_ends_its_tool_in_every_project() {
        use crate::app::modeller::ModelTool;
        let mut studio = Studio::new(App::default());
        let _ = studio.update(Msg::Product(Product::Modeller));
        let _ = studio.update(Msg::App(Message::NewModel("Level 0")));
        let _ = studio.update(Msg::App(Message::ModelTool(Some((ModelTool::Wall, "Wall")))));
        // a second project with a tool on, then back to the first
        let _ = studio.update(Msg::App(Message::NewModel("Level 0")));
        let _ = studio.update(Msg::App(Message::ModelTool(Some((ModelTool::Beam, "Beam")))));
        assert_eq!(studio.tabs.len(), 2);
        let _ = studio.update(Msg::SelectProject(0));
        assert!(studio.app.modeller.tool.is_some());
        let _ = studio.update(Msg::Product(Product::Viewer));
        assert!(studio.app.modeller.tool.is_none());
        assert!(studio.parked.iter().all(|(_, a)| a.modeller.tool.is_none()));
        // the project and its model stay
        assert!(studio.app.model.is_some());
    }

    #[test]
    fn the_product_chosen_last_shows_at_the_next_start() {
        let dir = std::env::temp_dir().join(format!("o3v-products-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut studio = Studio::new(App::default()).with_settings_in(dir.clone());
        studio.set_product(Product::Collaborator);
        let again = Studio::new(App::default()).with_settings_in(dir.clone());
        assert_eq!(again.product(), Product::Collaborator);
        let _ = std::fs::remove_dir_all(&dir);
        // older preferences have no product: the Viewer
        let prefs: Prefs = serde_json::from_str(r#"{"theme":"light"}"#).unwrap();
        assert_eq!(prefs.product, Product::Viewer);
    }

    #[test]
    fn a_build_of_one_product_shows_it_without_the_switch() {
        let mut studio = Studio::new(App::default());
        studio.products = vec![Product::Modeller];
        // what the preferences say does not count when the build lacks it
        assert_eq!(studio.product(), Product::Modeller);
        studio.set_product(Product::Viewer);
        assert_eq!(studio.product(), Product::Modeller);
        let names = accessibility(&studio).summary();
        assert!(!names.iter().any(|n| n == "Tab: Viewer" || n == "Tab: BIM Collaborator"), "{names:?}");
        assert!(names.contains(&"Tab: Model".to_string()), "{names:?}");
        // a product binary: its own name in the title and About
        let alone = Studio::new(App::default()).with_edition(Edition::Only(Product::Collaborator));
        assert_eq!((alone.products.clone(), alone.product(), alone.title()), (vec![Product::Collaborator], Product::Collaborator, "Open BIM Collaborator".to_string()));
        assert_eq!(Studio::new(App::default()).title(), "Open 3D Viewer Studio");
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
    fn tab_on_a_modelled_element_opens_edit_mode_and_its_keys_are_its_own() {
        use iced::keyboard::{Key, Modifiers};
        use openaec_ui::keyboard::Navigation;
        let mut app = App::default();
        app.add_demo_building();
        let mut studio = Studio::new(app);
        // nothing selected: Tab moves the focus, edit mode stays shut
        let _ = studio.update(Msg::Navigate(Navigation::Next));
        assert!(studio.app.modeller.edit.is_none());
        studio.app.select_modelled(bim_core::ElementId(2));
        let _ = studio.update(Msg::Navigate(Navigation::Next));
        assert!(studio.app.modeller.edit.is_some(), "Tab opens edit mode");
        // G is edit mode's, not the grid's
        let grid = studio.app.grid;
        let _ = studio.update(Msg::Key(KeyPress { key: Key::Character("a".into()), modifiers: Modifiers::empty() }));
        let _ = studio.update(Msg::Key(KeyPress { key: Key::Character("g".into()), modifiers: Modifiers::empty() }));
        assert_eq!(studio.app.grid, grid);
        assert!(studio.app.modeller.edit.as_ref().is_some_and(|e| e.grab.is_some()), "G grabbed the points A picked");
        let _ = studio.update(Msg::Key(KeyPress { key: Key::Named(iced::keyboard::key::Named::Escape), modifiers: Modifiers::empty() }));
        let _ = studio.update(Msg::Navigate(Navigation::Next));
        assert!(studio.app.modeller.edit.is_none(), "Tab leaves");
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
