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
use openaec_ui::design::{focus, Look, StatusbarSetting, TextSize, ThemeId, WorkspaceSetting};
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

/// What a ribbon group is known by in the settings: its first command.
fn group_key(group: &(&'static str, Vec<RibbonPart>)) -> &'static str {
    match group.1.first() {
        Some(RibbonPart::Large(id)) => id,
        Some(RibbonPart::Smalls(ids)) => ids.first().copied().unwrap_or(""),
        None => "",
    }
}

/// A part of a ribbon group: its main command large, or a column of small ones.
enum RibbonPart {
    Large(&'static str),
    Smalls(Vec<&'static str>),
}

/// From what width of the window the title bar names the products to come.
const SOON_WIDTH: f32 = 1410.0;
/// The products to come, named on the switch but not to be chosen yet.
const SOON: [&str; 2] = ["FM", "Requirements"];

/// The side of a small ribbon button that shows its icon only (a narrow window).
const RIBBON_ICON: f32 = 22.0;

const fn named<T>(value: T, label: &'static str) -> Named<T> {
    Named { value, label }
}

/// The number of interface languages ([`Language::ALL`]).
const LANGUAGES: usize = Language::ALL.len();

/// The settings per language, in the order of [`Language::ALL`].
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
    [
        named(ThemeId::Light, "Claro"),
        named(ThemeId::Dark, "Oscuro"),
        named(ThemeId::Blueprint, "Blueprint"),
        named(ThemeId::HighContrastDark, "Alto contraste oscuro"),
        named(ThemeId::HighContrastLight, "Alto contraste claro"),
    ],
    [
        named(ThemeId::Light, "Chiaro"),
        named(ThemeId::Dark, "Scuro"),
        named(ThemeId::Blueprint, "Blueprint"),
        named(ThemeId::HighContrastDark, "Alto contrasto scuro"),
        named(ThemeId::HighContrastLight, "Alto contrasto chiaro"),
    ],
    [
        named(ThemeId::Light, "Claro"),
        named(ThemeId::Dark, "Escuro"),
        named(ThemeId::Blueprint, "Blueprint"),
        named(ThemeId::HighContrastDark, "Alto contraste escuro"),
        named(ThemeId::HighContrastLight, "Alto contraste claro"),
    ],
    [
        named(ThemeId::Light, "Jasny"),
        named(ThemeId::Dark, "Ciemny"),
        named(ThemeId::Blueprint, "Blueprint"),
        named(ThemeId::HighContrastDark, "Wysoki kontrast ciemny"),
        named(ThemeId::HighContrastLight, "Wysoki kontrast jasny"),
    ],
    [
        named(ThemeId::Light, "Ljust"),
        named(ThemeId::Dark, "Mörkt"),
        named(ThemeId::Blueprint, "Blueprint"),
        named(ThemeId::HighContrastDark, "Hög kontrast mörkt"),
        named(ThemeId::HighContrastLight, "Hög kontrast ljust"),
    ],
    [
        named(ThemeId::Light, "Açık"),
        named(ThemeId::Dark, "Koyu"),
        named(ThemeId::Blueprint, "Blueprint"),
        named(ThemeId::HighContrastDark, "Yüksek karşıtlık koyu"),
        named(ThemeId::HighContrastLight, "Yüksek karşıtlık açık"),
    ],
];
const WORKSPACES: [[Named<WorkspaceSetting>; 3]; LANGUAGES] = [
    [named(WorkspaceSetting::Theme, "Volgt thema"), named(WorkspaceSetting::Light, "Altijd licht"), named(WorkspaceSetting::Dark, "Altijd donker")],
    [named(WorkspaceSetting::Theme, "Follows the theme"), named(WorkspaceSetting::Light, "Always light"), named(WorkspaceSetting::Dark, "Always dark")],
    [named(WorkspaceSetting::Theme, "Folgt dem Thema"), named(WorkspaceSetting::Light, "Immer hell"), named(WorkspaceSetting::Dark, "Immer dunkel")],
    [named(WorkspaceSetting::Theme, "Suit le thème"), named(WorkspaceSetting::Light, "Toujours clair"), named(WorkspaceSetting::Dark, "Toujours sombre")],
    [named(WorkspaceSetting::Theme, "Sigue el tema"), named(WorkspaceSetting::Light, "Siempre claro"), named(WorkspaceSetting::Dark, "Siempre oscuro")],
    [named(WorkspaceSetting::Theme, "Segue il tema"), named(WorkspaceSetting::Light, "Sempre chiaro"), named(WorkspaceSetting::Dark, "Sempre scuro")],
    [named(WorkspaceSetting::Theme, "Segue o tema"), named(WorkspaceSetting::Light, "Sempre claro"), named(WorkspaceSetting::Dark, "Sempre escuro")],
    [named(WorkspaceSetting::Theme, "Zgodnie z motywem"), named(WorkspaceSetting::Light, "Zawsze jasny"), named(WorkspaceSetting::Dark, "Zawsze ciemny")],
    [named(WorkspaceSetting::Theme, "Följer temat"), named(WorkspaceSetting::Light, "Alltid ljust"), named(WorkspaceSetting::Dark, "Alltid mörkt")],
    [named(WorkspaceSetting::Theme, "Temayı izler"), named(WorkspaceSetting::Light, "Her zaman açık"), named(WorkspaceSetting::Dark, "Her zaman koyu")],
];
const STATUSBARS: [[Named<StatusbarSetting>; 2]; LANGUAGES] = [
    [named(StatusbarSetting::Theme, "Volgt thema"), named(StatusbarSetting::Dark, "Altijd donker")],
    [named(StatusbarSetting::Theme, "Follows the theme"), named(StatusbarSetting::Dark, "Always dark")],
    [named(StatusbarSetting::Theme, "Folgt dem Thema"), named(StatusbarSetting::Dark, "Immer dunkel")],
    [named(StatusbarSetting::Theme, "Suit le thème"), named(StatusbarSetting::Dark, "Toujours sombre")],
    [named(StatusbarSetting::Theme, "Sigue el tema"), named(StatusbarSetting::Dark, "Siempre oscuro")],
    [named(StatusbarSetting::Theme, "Segue il tema"), named(StatusbarSetting::Dark, "Sempre scuro")],
    [named(StatusbarSetting::Theme, "Segue o tema"), named(StatusbarSetting::Dark, "Sempre escuro")],
    [named(StatusbarSetting::Theme, "Zgodnie z motywem"), named(StatusbarSetting::Dark, "Zawsze ciemny")],
    [named(StatusbarSetting::Theme, "Följer temat"), named(StatusbarSetting::Dark, "Alltid mörkt")],
    [named(StatusbarSetting::Theme, "Temayı izler"), named(StatusbarSetting::Dark, "Her zaman koyu")],
];

const TEXT_SIZES: [[Named<TextSize>; 3]; LANGUAGES] = [
    [named(TextSize::Compact, "Compact"), named(TextSize::Standard, "Standaard"), named(TextSize::Large, "Groot")],
    [named(TextSize::Compact, "Compact"), named(TextSize::Standard, "Standard"), named(TextSize::Large, "Large")],
    [named(TextSize::Compact, "Kompakt"), named(TextSize::Standard, "Standard"), named(TextSize::Large, "Groß")],
    [named(TextSize::Compact, "Compacte"), named(TextSize::Standard, "Standard"), named(TextSize::Large, "Grande")],
    [named(TextSize::Compact, "Compacto"), named(TextSize::Standard, "Estándar"), named(TextSize::Large, "Grande")],
    [named(TextSize::Compact, "Compatto"), named(TextSize::Standard, "Standard"), named(TextSize::Large, "Grande")],
    [named(TextSize::Compact, "Compacto"), named(TextSize::Standard, "Padrão"), named(TextSize::Large, "Grande")],
    [named(TextSize::Compact, "Kompaktowy"), named(TextSize::Standard, "Standardowy"), named(TextSize::Large, "Duży")],
    [named(TextSize::Compact, "Kompakt"), named(TextSize::Standard, "Standard"), named(TextSize::Large, "Stor")],
    [named(TextSize::Compact, "Kompakt"), named(TextSize::Standard, "Standart"), named(TextSize::Large, "Büyük")],
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
    /// The size of the interface's texts (Tekstgrootte): compact, standard or large.
    text_size: String,
    language: Language,
    scale: f64,
    left: bool,
    right: bool,
    left_width: f32,
    right_width: f32,
    recent: Vec<PathBuf>,
    /// The ribbon groups switched off, each by its first command.
    ribbon_hidden: Vec<String>,
    /// The extensions switched off: the integrated tools not on the switch, each by its key.
    #[serde(default)]
    extensions_off: Vec<String>,
    /// The product shown last.
    product: Product,
    /// In 3D: the heads of grid lines behind the model are left out; the distances between the
    /// grid lines and the levels are drawn.
    grid_heads_hidden: bool,
    grid_dims: bool,
    levels_3d: bool,
    /// The 3D view draws ambient shadows in its shaded styles.
    ambient: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            theme: ThemeId::Light.key().into(),
            workspace: WorkspaceSetting::Theme.to_string(),
            statusbar: StatusbarSetting::Theme.to_string(),
            text_size: "standard".into(),
            language: Language::English,
            scale: 1.0,
            left: true,
            right: true,
            left_width: 260.0,
            right_width: 280.0,
            recent: Vec::new(),
            ribbon_hidden: Vec::new(),
            extensions_off: Vec::new(),
            product: Product::Viewer,
            grid_heads_hidden: true,
            grid_dims: false,
            levels_3d: false,
            ambient: false,
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

    fn text_size(&self) -> TextSize {
        match self.text_size.as_str() {
            "compact" => TextSize::Compact,
            "large" => TextSize::Large,
            _ => TextSize::Standard,
        }
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
    /// The form to send feedback on the app.
    Feedback,
    /// The card of the Pointcloud product: what the cloud's file holds and how its jobs are done.
    Cloud,
}

/// The feedback form: what is written in it, and how sending it goes.
#[derive(Default)]
pub struct Feedback {
    email: String,
    name: String,
    /// What it is about (an index into [`FEEDBACK_KINDS`]).
    kind: usize,
    message: iced::widget::text_editor::Content,
    /// How the writer feels (an index into [`FEEDBACK_MOODS`]), if said.
    mood: Option<usize>,
    state: FeedbackState,
}

#[derive(Debug, Clone, Default, PartialEq)]
enum FeedbackState {
    #[default]
    Writing,
    Sending,
    Sent,
    Failed(String),
}

/// A step in the feedback form.
#[derive(Debug, Clone)]
pub enum FeedbackMsg {
    Email(String),
    Name(String),
    Kind(usize),
    Message(iced::widget::text_editor::Action),
    Mood(usize),
    Send,
    Done(Result<(), String>),
    Again,
}

/// What feedback can be about, as the feedback service names it, with its label.
const FEEDBACK_KINDS: [(&str, &str, &str); 3] = [("general", "Algemeen", "General"), ("bug", "Fout", "Bug"), ("feature", "Wens", "Feature")];
/// How the writer feels, as the feedback service names it, with its label.
const FEEDBACK_MOODS: [(&str, &str, &str); 3] = [("Frustrated", "Gefrustreerd", "Frustrated"), ("Neutral", "Neutraal", "Neutral"), ("Happy", "Blij", "Happy")];
/// Where feedback on the OpenAEC apps goes, and what this app is called there.
#[cfg(not(target_arch = "wasm32"))]
const FEEDBACK_URL: &str = "https://open-feedback-studio.pages.dev/api/feedback";
const FEEDBACK_APP: &str = "open-3d-viewer";
/// The shortest and the longest message the service takes.
const FEEDBACK_SHORTEST: usize = 10;
const FEEDBACK_LONGEST: usize = 5000;

impl Feedback {
    fn wording(&self) -> String {
        self.message.text().trim().to_string()
    }

    /// An e-mail address is asked, to answer the writer.
    fn email_ok(&self) -> bool {
        let e = self.email.trim();
        !e.contains(char::is_whitespace) && e.split_once('@').is_some_and(|(name, host)| !name.is_empty() && host.contains('.') && !host.starts_with('.') && !host.ends_with('.'))
    }

    fn ready(&self) -> bool {
        let n = self.wording().chars().count();
        self.email_ok() && (FEEDBACK_SHORTEST..=FEEDBACK_LONGEST).contains(&n) && self.state != FeedbackState::Sending
    }

    /// What is sent: only what the writer filled in, and the version of the app.
    fn payload(&self) -> serde_json::Value {
        let mut out = serde_json::json!({
            "app": FEEDBACK_APP,
            "email": self.email.trim(),
            "category": FEEDBACK_KINDS[self.kind.min(2)].0,
            "message": self.wording(),
            "appVersion": env!("CARGO_PKG_VERSION"),
        });
        if !self.name.trim().is_empty() {
            out["fullname"] = self.name.trim().into();
        }
        if let Some(mood) = self.mood.and_then(|m| FEEDBACK_MOODS.get(m)) {
            out["sentiment"] = mood.0.into();
        }
        out
    }
}

/// Sends feedback to the service; what went wrong as the service says it, else the status.
#[cfg(not(target_arch = "wasm32"))]
fn send_feedback(payload: serde_json::Value) -> Result<(), String> {
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(20)).user_agent(concat!("OpenBIMSuite/", env!("CARGO_PKG_VERSION"))).build();
    match agent.post(FEEDBACK_URL).set("Content-Type", "application/json").send_string(&payload.to_string()) {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(code, response)) => {
            let said = response.into_string().ok().and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok()).and_then(|v| v.get("error")?.as_str().map(str::to_string));
            Err(said.unwrap_or_else(|| format!("HTTP {code}")))
        }
        Err(e) => Err(e.to_string()),
    }
}

/// What a screenshot shows: the workspace, the Bestand page or a page of the settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Workspace,
    File,
    Settings,
    Shortcuts,
    Ribbon,
    Extensions,
    Feedback,
    /// The card of the Pointcloud product.
    Cloud,
}

impl Page {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "workspace" => Page::Workspace,
            "file" => Page::File,
            "settings" => Page::Settings,
            "shortcuts" => Page::Shortcuts,
            "feedback" => Page::Feedback,
            "cloud" => Page::Cloud,
            "ribbon" => Page::Ribbon,
            "extensions" => Page::Extensions,
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
    Ribbon,
    Shortcuts,
    /// The extension manager: the integrated tools, each switched on or off.
    Extensions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

#[derive(Debug, Clone)]
pub enum Msg {
    /// While a letter waits for its pair: the time to look at.
    KeyWindow,
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
    Feedback(FeedbackMsg),
    SettingsPage(SettingsPage),
    Shortcuts(sc::Message),
    Theme(ThemeId),
    Workspace(WorkspaceSetting),
    /// A ribbon group (by its first command) shown or not; and all of them shown again.
    RibbonGroup(&'static str, bool),
    RibbonReset,
    /// The extension manager: an integrated tool switched on or off.
    Extension(Product, bool),
    TextSize(TextSize),
    Statusbar(StatusbarSetting),
    Language(Language),
    Scale(f64),
    ToggleLeft,
    ToggleRight,
    SplitterPress(Side),
    SplitterMove(Point),
    SplitterRelease,
    ShowModel3d,
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
    /// The Planner's web frame loads its page again.
    PlannerReload,
    /// The page of the Planner's web frame opens in the system's browser.
    PlannerBrowser,
    /// What laying or hiding the Planner's web frame gave: an error when it could not be made.
    PlannerFrame(Result<(), String>),
    /// What reloading the frame's page, or taking the keyboard back from it, gave.
    PlannerDone(Result<(), String>),
    /// Where the pointer is over the app's own drawing while the Planner shows (`None`: over the
    /// web frame or outside the window).
    PlannerPointer(Option<iced::Point>),
    /// A press on the app's own drawing while the Planner shows: the keyboard comes back to it.
    PlannerPress,
    /// The window changed while the Planner shows: its frame is laid again.
    PlannerWindow,
    /// Opens the desktop app's download page (from [`Dialog::DesktopOnly`]).
    DownloadDesktop,
}

pub struct Studio {
    /// The project shown.
    pub app: App,
    /// The Modeller's letters typed so far (one waiting for its pair).
    pub(crate) keys: crate::app::modeller::keys::KeySequence,
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
    /// What the feedback form holds while it is open.
    feedback: Feedback,
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
    /// The window is open, and where the Planner's web frame was last laid in it (`None`: hidden).
    window_open: bool,
    planner_shown: Option<[f32; 4]>,
    /// Why the web frame could not be made (no web view runtime), until Refresh tries again.
    planner_failed: Option<String>,
    /// Where the pointer is over the app's own drawing while the Planner shows.
    planner_pointer: Option<iced::Point>,
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
    /// The time is `now`: a letter that waited too long for its pair does what it does alone.
    pub(crate) fn expire_keys(&mut self, now: Instant) {
        if let Some(step) = self.keys.expire_step(now) {
            if self.app.modeller.sketch.is_some() { return; }
            let _ = self.key_step(step);
        }
    }

    /// What a letter typed in the Modeller came to: its command, or, when the Modeller has no use
    /// for the letter, the app's own shortcut on it (F fits, H hides).
    fn key_step(&mut self, step: crate::app::modeller::keys::Step) -> Task<Msg> {
        use crate::app::modeller::keys::Step;
        match step {
            Step::Key(key) => {
                let l = self.l();
                self.update(Msg::App(Message::ModelKey(key, l)))
            }
            Step::Letter(c) => match cm::find(&self.commands(), &iced::keyboard::Key::Character(c.to_string().into()), iced::keyboard::Modifiers::empty()) {
                Some(message) => self.update(message),
                None => Task::none(),
            },
        }
    }

    pub fn new(mut app: App) -> Self {
        app.set_viewer_mode(Prefs::default().product == Product::Viewer);
        Self {
            keys: Default::default(),
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
            feedback: Feedback::default(),
            settings_page: SettingsPage::General,
            update: None,
            maximized: false,
            last_title_press: None,
            drag: None,
            width: 1280.0,
            height: 800.0,
            sized: false,
            window_open: false,
            planner_shown: None,
            planner_failed: None,
            planner_pointer: None,
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
            self.keys.clear();
            self.ribbon_tab = 0;
        }
        // GIS works from its tab of the right pane
        if product == Product::Gis && self.app.side_tab != crate::app::SideTab::Geo {
            self.app.side_tab = crate::app::SideTab::Gis;
        }
        if product != Product::Modeller {
            for app in std::iter::once(&mut self.app).chain(self.parked.iter_mut().map(|(_, a)| a)) {
                if app.modeller.tool.is_some() {
                    let _ = app.update(Message::ModelTool(None));
                }
            }
        }
        // the Viewer only looks at a view: its bar has no tools to dimension, write or cut
        for app in std::iter::once(&mut self.app).chain(self.parked.iter_mut().map(|(_, a)| a)) {
            app.set_viewer_mode(product == Product::Viewer);
            if product == Product::Modeller && app.modeller.sketch.is_none() {
                app.modeller.sketch = app.modeller.parked_sketch.take();
            }
        }
        // the Modeller's explorer opens on its project browser; elsewhere that tab is not there
        let (browser, model) = (crate::app::ExplorerTab::Project, crate::app::ExplorerTab::Model);
        if product == Product::Modeller && self.product() != product && self.app.explorer_tab == model {
            self.app.explorer_tab = browser;
        } else if product != Product::Modeller && self.app.explorer_tab == browser {
            self.app.explorer_tab = model;
        }
        self.prefs.product = product;
        self.app.modeller.in_modeller = product == Product::Modeller;
        self.backstage = false;
        self.ribbon_folded = false;
        self.save_prefs();
    }

    /// Docs is a product: its page is open while it is the product shown, and closed in the
    /// others. Opening it reads the project folder again.
    fn docs_follow_product(&mut self) -> Task<Msg> {
        let docs = self.product() == Product::Docs;
        self.app.docs.in_product = docs;
        match (docs, self.app.docs.open) {
            (true, false) => self.app.update(Message::Docs(crate::app::DocsMessage::Open)).map(Msg::App),
            (false, true) => {
                let _ = self.app.update(Message::Docs(crate::app::DocsMessage::Close));
                Task::none()
            }
            _ => Task::none(),
        }
    }

    /// IFCX is a product: while it is the one shown, the project's IFCX fills the workspace in
    /// place of the views; another product gives them back. It opens once a model is there.
    fn ifcx_follow_product(&mut self) -> Task<Msg> {
        let on = self.product() == Product::Ifcx;
        if on && !self.app.ifc.full && self.app.model.is_some() {
            // the first time it opens on the graph
            if !self.app.ifc.open {
                self.app.ifc.view = crate::app::IfcView::Graph;
            }
            self.app.ifc.full = true;
            return self.app.update(Message::OpenIfc).map(Msg::App);
        }
        if !on && self.app.ifc.full {
            self.app.ifc.full = false;
            self.app.ifc.shown = false;
        }
        Task::none()
    }

    /// The ribbon tab shown.
    fn ribbon_page(&self) -> RibbonPage {
        let pages = self.product().pages();
        pages[self.ribbon_tab.min(pages.len() - 1)]
    }

    /// A build of `edition`: the Studio, or one product alone.
    pub fn with_edition(mut self, edition: Edition) -> Self {
        self.edition = edition;
        self.apply_extensions();
        self
    }

    /// The integrated tools of this build: the extensions the manager lists.
    pub fn extensions(&self) -> Vec<Product> {
        self.edition.products()
    }

    /// Whether an extension is switched on. A build of one product has it on.
    pub fn extension_on(&self, product: Product) -> bool {
        matches!(self.edition, Edition::Only(_)) || product == Product::Viewer || !self.prefs.extensions_off.iter().any(|key| key == product.key())
    }

    /// Puts the extensions that are switched on on the switch; a product shown that is switched
    /// off makes way for the first that is on.
    fn apply_extensions(&mut self) {
        let shown = self.product();
        self.products = self.extensions().into_iter().filter(|p| self.extension_on(*p)).collect();
        if !self.products.contains(&shown) {
            if let Some(first) = self.products.first().copied() {
                self.set_product(first);
            }
        }
    }

    /// Reads the preferences and own shortcuts in `dir`, and keeps them there.
    pub fn with_settings_in(mut self, dir: PathBuf) -> Self {
        let prefs = dir.join("preferences.json");
        self.prefs = Prefs::load(&prefs);
        self.apply_extensions();
        (self.app.grid_heads_hidden, self.app.grid_dims, self.app.levels_3d, self.app.ambient) = (self.prefs.grid_heads_hidden, self.prefs.grid_dims, self.prefs.levels_3d, self.prefs.ambient);
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
        self.dialog = match page {
            Page::Settings | Page::Shortcuts | Page::Ribbon | Page::Extensions => Some(Dialog::Settings),
            Page::Feedback => Some(Dialog::Feedback),
            Page::Cloud => Some(Dialog::Cloud),
            _ => None,
        };
        // the card of a cloud shows what its file holds: read here, as no message runs
        #[cfg(all(feature = "pointcloud", not(target_arch = "wasm32")))]
        if page == Page::Cloud {
            self.app.cloud.details = self.app.cloud().map(|cloud| (cloud.file.clone(), crate::app::pointcloud::details(&cloud.file)));
        }
        self.settings_page = match page {
            Page::Shortcuts => SettingsPage::Shortcuts,
            Page::Ribbon => SettingsPage::Ribbon,
            Page::Extensions => SettingsPage::Extensions,
            _ => SettingsPage::General,
        };
    }

    /// Where the Planner's web frame lies in the window, in the window's own units (layout units
    /// times the interface size): the whole workspace, from under the ribbon to the status bar.
    /// `None` when it is hidden: another product, File, or a dialog, which the app draws itself
    /// and which a frame over the window would cover; and when the web view could not be made.
    ///
    /// The frame is a window of the system, over all the app draws. So it stays clear of the
    /// window's left and right edge, where a press resizes the window (the bottom edge is the
    /// status bar's), and it makes room for a tip that hangs into the workspace while the pointer
    /// is where such a tip opens: on the ribbon, whose buttons reach almost to its lower edge (or
    /// on the tabs when the ribbon is folded), and on the status bar, whose tips open above it.
    /// The tips of the title bar end above the workspace.
    pub fn planner_area(&self) -> Option<[f32; 4]> {
        let scale = self.prefs.scale() as f32;
        self.planner_insets().map(|[left, top, right, bottom]| [left, top, (self.width * scale - left - right).max(1.0), (self.height * scale - top - bottom).max(1.0)])
    }

    /// How far the Planner's web frame stays from the window's left, top, right and bottom, in
    /// the window's own units: what [`Studio::planner_area`] is laid by. The frame is laid from
    /// these and the window's size as the system gives it at that moment, which the size the
    /// window last told the shell may not be (a window just opened, or moved to another display).
    pub fn planner_insets(&self) -> Option<[f32; 4]> {
        use openaec_ui::design::shell::{RIBBON, RIBBON_TABS, STATUSBAR, TITLEBAR};
        if self.product() != Product::Planner || self.backstage || self.dialog.is_some() || !self.window_open || self.planner_failed.is_some() {
            return None;
        }
        let folded = self.compact() && self.ribbon_folded;
        let (mut top, mut bottom) = (TITLEBAR + RIBBON_TABS + if folded { 0.0 } else { RIBBON }, STATUSBAR);
        if let Some(at) = self.planner_pointer {
            if at.y >= if folded { TITLEBAR } else { TITLEBAR + RIBBON_TABS } && at.y < top {
                top += if folded { PLANNER_TIP } else { PLANNER_RIBBON_TIP };
            }
            if at.y >= self.height - STATUSBAR {
                bottom += PLANNER_TIP;
            }
        }
        let edge = if self.maximized { 0.0 } else { crate::resize_frame::EDGE };
        let scale = self.prefs.scale() as f32;
        Some([edge * scale, top * scale, edge * scale, bottom * scale])
    }

    /// [`Studio::update`], and then the Planner's web frame laid where the window now has its
    /// workspace, when that changed: the frame follows the product, the window and the ribbon.
    pub fn update_with_frame(&mut self, message: Msg) -> Task<Msg> {
        // what laying the frame answers lays no frame again
        let answer = matches!(message, Msg::PlannerFrame(_) | Msg::PlannerDone(_));
        let task = self.update(message);
        if self.product() != Product::Planner {
            // the pointer is followed on the Planner only: what it was says nothing on return
            self.planner_pointer = None;
        }
        // a frame that shows is laid again after every message: the window may have changed
        // without telling (the frame itself skips what it already is)
        let area = self.planner_insets();
        if answer || (area.is_none() && self.planner_shown.is_none()) {
            return task;
        }
        self.planner_shown = area;
        #[cfg(all(feature = "planner", windows))]
        return Task::batch([task, iced::window::latest().and_then(move |id| iced::window::run(id, move |window| crate::planner_frame::place(&window, area))).map(Msg::PlannerFrame)]);
        #[cfg(not(all(feature = "planner", windows)))]
        task
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
                // a point of the window in place of a control: what lies there is clicked, named or not
                let point = args.get("at").and_then(|v| v.as_array()).filter(|a| a.len() == 2).and_then(|a| Some(iced::Point::new(a[0].as_f64()? as f32, a[1].as_f64()? as f32)));
                let unnamed = args.get("name").is_none() && args.get("index").is_none();
                let target = if unnamed && (call.cmd == "ui_key" || point.is_some()) { None } else { Some(ui::find(&found, &args)) };
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
                            if let Some(at) = bounds.and_then(|b| ui::aim(b, size)).or(point) {
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
                        // a key no control took, with none named: the window's own key, as the
                        // keyboard sends it (a shortcut, a key of the Modeller, Tab)
                        if messages.is_empty() && call.cmd == "ui_key" && target.is_none() && point.is_none() {
                            use iced::keyboard::key::Named;
                            use iced::keyboard::{Key, Modifiers};
                            let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("Enter");
                            let held = |name: &str| args.get(name).and_then(|v| v.as_bool()).unwrap_or(false);
                            let mut modifiers = Modifiers::empty();
                            modifiers.set(Modifiers::SHIFT, held("shift"));
                            modifiers.set(Modifiers::CTRL, held("ctrl"));
                            let named = match key.to_ascii_lowercase().as_str() {
                                "enter" | "return" => Some(Named::Enter),
                                "escape" | "esc" => Some(Named::Escape),
                                "tab" => Some(Named::Tab),
                                "delete" => Some(Named::Delete),
                                "backspace" => Some(Named::Backspace),
                                "pageup" => Some(Named::PageUp),
                                "pagedown" => Some(Named::PageDown),
                                _ => None,
                            };
                            messages = match named {
                                Some(Named::Tab) => vec![Msg::Navigate(openaec_ui::keyboard::Navigation::Next)],
                                // Enter and Delete are also the app's own keys (its subscription), beside the
                                // shell's shortcuts
                                Some(Named::Enter) => vec![Msg::App(Message::MeasureFinish)],
                                Some(Named::Delete | Named::Backspace) => vec![Msg::App(Message::DeletePicked)],
                                Some(named) => vec![Msg::Key(openaec_ui::keyboard::KeyPress { key: Key::Named(named), modifiers })],
                                None => vec![Msg::Key(openaec_ui::keyboard::KeyPress { key: Key::Character(key.to_lowercase().into()), modifiers })],
                            };
                        }
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
        Look::new(self.prefs.theme(), self.prefs.workspace(), self.prefs.statusbar()).with_text_size(self.prefs.text_size())
    }

    /// The width of the left panel: the Validator's lists need more than the explorer's tree.
    fn left_width(&self) -> f32 {
        if self.product() == Product::Validator { self.prefs.left_width.max(340.0) } else { self.prefs.left_width }
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
            // a model kept as a folder: a Tekla model folder, an Allplan project
            Command::new("file.open-folder", l.text("Map openen…", "Open folder…"), go(Message::OpenFolderDialog)).icon(small("file.open"), large("file.open")).dialog(),
            Command::new("file.add", l.text("Toevoegen…", "Add…"), go(Message::AddDialog))
                .icon(small("file.import"), large("file.import"))
                .shortcut(Shortcut::ctrl_shift('o'))
                .dialog(),
            Command::new("file.save-as", l.text("Opslaan als…", "Save as…"), go(Message::SaveDialog))
                .icon(small("file.save-as"), large("file.save-as"))
                .shortcut(Shortcut::ctrl_shift('s'))
                .enabled(loaded)
                .dialog(),
            Command::new("file.save-project", l.text("Project opslaan…", "Save project…"), go(Message::SaveProjectDialog))
                .icon(small("file.save-as"), large("file.save-as"))
                .enabled(loaded)
                .dialog(),
            Command::new("file.render", l.text("Renderen…", "Render…"), go(Message::RenderDialog))
                .icon(small("file.save-as"), large("file.save-as"))
                .enabled(loaded && cfg!(not(target_arch = "wasm32")))
                .detail(l.text("Schrijft een afbeelding van het model zoals het beeld het toont (Blender rendert hem)", "Writes an image of the model as the view shows it (Blender renders it)"))
                .dialog(),
            Command::new("file.build-film", l.text("Bouwfilm…", "Build film…"), go(Message::BuildFilmDialog))
                .icon(small("file.save-as"), large("file.save-as"))
                .enabled(loaded && cfg!(not(target_arch = "wasm32")))
                .detail(l.text("Schrijft een film waarin het model wordt opgebouwd (Blender rendert hem)", "Writes a film in which the model is built up (Blender renders it)"))
                .dialog(),
            Command::new("file.export-ifc4x3", l.text("Exporteren als IFC 4x3…", "Export IFC 4x3…"), go(Message::SaveIfcDialog(crate::export::IfcSchema::Ifc4x3)))
                .icon(small("file.save-as"), large("file.save-as"))
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
            Command::new("view.hidden", l.text("Verborgen lijnen", "Hidden line"), go(Message::Mode(RenderMode::HiddenLine)))
                .icon(small("drawing.plan"), large("drawing.plan"))
                .mode(app.mode == RenderMode::HiddenLine),
            Command::new("view.shadows", l.text("Schaduw", "Shadows"), go(Message::ToggleAmbient)).icon(small("theme.light"), large("theme.light")).toggle(app.ambient),
            Command::new("view.grid", l.text("Raster", "Grid"), go(Message::ToggleGrid)).icon(small("view.grid"), large("view.grid")).shortcut(key('g')).toggle(app.grid),
            Command::new("view.grid-lines", l.text("Stramien", "Grid lines"), go(Message::ToggleGrids))
                .icon(small("measure.distance"), large("measure.distance"))
                .toggle(app.show_grids && has_grids)
                .enabled(has_grids),
            Command::new("view.grid-dims", l.text("Stramienmaten", "Grid dimensions"), go(Message::ToggleGridDims))
                .icon(small("measure.distance"), large("measure.distance"))
                .toggle(app.grid_dims && has_grids)
                .enabled(has_grids),
            Command::new("view.levels-3d", l.text("Peilen in 3D", "Levels in 3D"), go(Message::ToggleLevels3d))
                .icon(small("drawing.plan"), large("drawing.plan"))
                .toggle(app.levels_3d)
                .enabled(loaded),
            Command::new("view.3d", "3D", Msg::ShowModel3d).icon(small("view.isometric"), large("view.isometric")).detail(l.text("Open de 3D-weergave van het huidige project.", "Open the current project’s 3D view.")),
            Command::new("view.views-3d", l.text("Weergaven in 3D", "Views in 3D"), go(Message::ToggleViewsIn3d))
                .icon(small("view.saved"), large("view.saved"))
                .toggle(app.views_in_3d || !app.projected.is_empty())
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
                .shortcut(Shortcut { alt: true, ..key('o') })
                .mode(app.camera.ortho),
            Command::new("section.box", l.text("Doorsnedebox", "Section box"), go(Message::ToggleSection)).icon(small("section.box"), large("section.box")).toggle(app.section_on).enabled(loaded),
            Command::new("section.plane", l.text("Doorsnedevlak", "Section plane"), go(Message::TogglePlane))
                .icon(small("section.plane-z"), large("section.plane-z"))
                .toggle(app.plane.is_some() || app.plane_picking)
                .enabled(loaded),
            Command::new("section.flip", l.text("Omdraaien", "Flip"), go(Message::FlipPlane)).icon(small("transform.mirror"), large("transform.mirror")).enabled(app.plane.is_some()),
            Command::new("edit.undo", l.text("Ongedaan maken", "Undo"), go(Message::Undo)).icon(small("edit.undo"), large("edit.undo")).shortcut(Shortcut::ctrl('z')).enabled(app.can_undo()),
            Command::new("edit.redo", l.text("Opnieuw", "Redo"), go(Message::Redo)).icon(small("edit.redo"), large("edit.redo")).shortcut(Shortcut::ctrl('y')).enabled(app.can_redo()),
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
            // Docs: the project folder, and opening and linking the picked document
            Command::new("docs.folder", l.text("Projectmap", "Project folder"), go(Message::Docs(crate::app::DocsMessage::Choose))).icon(small("project.folder"), large("project.folder")),
            Command::new("planner.reload", l.text("Vernieuwen", "Refresh"), Msg::PlannerReload).icon(small("view.reset-camera"), large("view.reset-camera")),
            Command::new("planner.browser", l.text("In browser openen", "Open in browser"), Msg::PlannerBrowser).icon(small("file.import"), large("file.import")),
            Command::new("gis.map", l.text("Kaart laden", "Load map"), go(Message::Gis(crate::app::GisMessage::Map(crate::app::MapMessage::Fetch(l))))).icon(small("view.top"), large("view.top")).detail(l.text("Legt de kaart die in het tabblad GIS rechts gekozen is onder het project: de achtergrondkaart, de luchtfoto of een eigen WMS-dienst.", "Lays the map chosen in the GIS tab on the right under the project: the background map, the aerial photograph or a WMS service of your own.")),
            Command::new("geo.fetch", l.text("Sonderingen laden", "Load soundings"), go(Message::Gis(crate::app::GisMessage::Geo(crate::app::GeoMessage::Fetch(l))))).icon(small("draw.coordinate"), large("draw.coordinate")).detail(l.text("Haalt de sonderingen rond de plek op uit de Basisregistratie Ondergrond en zet ze in het model.", "Fetches the soundings around the place from the Dutch register of the subsurface and puts them in the model.")),
            Command::new("geo.borings", l.text("Boringen laden", "Load borings"), go(Message::Gis(crate::app::GisMessage::Geo(crate::app::GeoMessage::FetchBorings(l))))).icon(small("pile.columns"), large("pile.columns")).detail(l.text("Haalt de geotechnische boringen rond de plek op uit de Basisregistratie Ondergrond en zet ze in het model.", "Fetches the geotechnical borings around the place from the Dutch register of the subsurface and puts them in the model.")),
            Command::new("geo.open-borings", l.text("GEF openen", "Open GEF"), go(Message::Gis(crate::app::GisMessage::Geo(crate::app::GeoMessage::PickBorings(l))))).icon(small("file.import"), large("file.import")).detail(l.text("Opent boringen uit GEF-bestanden en zet ze in het model.", "Opens borings from GEF files and puts them in the model.")).dialog(),
            Command::new("geo.model", l.text("Ondergrondmodel", "Ground model"), go(Message::Gis(crate::app::GisMessage::Geo(crate::app::GeoMessage::Model(l))))).icon(small("mesh.closed"), large("mesh.closed")).detail(l.text("Maakt een 3D-model van de ondergrond uit de geladen boringen: elke plek krijgt de lagen van de dichtstbijzijnde boring.", "Makes a 3D model of the ground of the borings loaded: every place takes the layers of the nearest boring.")),
            Command::new("geo.open", l.text("GEF openen", "Open GEF"), go(Message::Gis(crate::app::GisMessage::Geo(crate::app::GeoMessage::Pick(l))))).icon(small("file.import"), large("file.import")).detail(l.text("Opent sonderingen uit GEF-bestanden en zet ze in het model.", "Opens soundings from GEF files and puts them in the model.")).dialog(),
            Command::new("gis.surroundings", l.text("Omgeving laden", "Load surroundings"), go(Message::Gis(crate::app::GisMessage::Fetch(l)))).icon(small("drawing.plan"), large("drawing.plan")).detail(l.text("Haalt de percelen en panden rond de plek op die in het tabblad GIS rechts gekozen is.", "Fetches the parcels and buildings around the place chosen in the GIS tab on the right.")),
            Command::new("gis.terrain", l.text("Terrein laden", "Load terrain"), go(Message::Gis(crate::app::GisMessage::Terrain(crate::app::TerrainMessage::Fetch(l))))).icon(small("mesh.surface"), large("mesh.surface")).detail(l.text("Haalt het maaiveld rond de plek op, met hoogtelijnen.", "Fetches the ground around the place, with contours.")),
            Command::new("gis.points", l.text("Oppervlak uit punten", "Surface from points"), go(Message::Gis(crate::app::GisMessage::Terrain(crate::app::TerrainMessage::Pick(l))))).icon(small("file.import"), large("file.import")).detail(l.text("Maakt een oppervlak met hoogtelijnen van een puntenbestand van een landmeter.", "Makes a surface with contours of a surveyor's file of points.")),
            Command::new("docs.refresh", l.text("Vernieuwen", "Refresh"), go(Message::Docs(crate::app::DocsMessage::Refresh)))
                .icon(small("view.reset-camera"), large("view.reset-camera"))
                .enabled(app.docs.folder.is_some()),
            Command::new("docs.leave", l.text("Loslaten", "Let go"), go(Message::Docs(crate::app::DocsMessage::Leave)))
                .icon(small("edit.clear-all"), large("edit.clear-all"))
                .enabled(app.docs.folder.is_some()),
            Command::new("docs.open", l.text("Openen in de suite", "Open in the suite"), go(Message::Docs(crate::app::DocsMessage::OpenModel { add: false })))
                .icon(small("file.open"), large("file.open"))
                .enabled(app.docs_reads_picked()),
            Command::new("docs.open-outside", l.text("Openen in programma", "Open in its program"), go(Message::Docs(crate::app::DocsMessage::OpenOutside)))
                .icon(small("file.import"), large("file.import"))
                .enabled(app.docs_picked().is_some()),
            Command::new("docs.reveal", l.text("Tonen in map", "Show in folder"), go(Message::Docs(crate::app::DocsMessage::Reveal)))
                .icon(small("panel.left"), large("panel.left"))
                .enabled(app.docs_picked().is_some()),
            Command::new("docs.save", l.text("Model opslaan in projectmap", "Save model in project folder"), go(Message::Docs(crate::app::DocsMessage::Save)))
                .icon(small("file.save-as"), large("file.save-as"))
                .enabled(app.docs_save_target().is_some()),
            Command::new("docs.link", l.text("Koppelen aan selectie", "Link to selection"), go(Message::Docs(crate::app::DocsMessage::Link)))
                .icon(small("grid.end-couple"), large("grid.end-couple"))
                .enabled(app.docs_can_link()),
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
            // the Select tool of the Modeller: on when no other tool is
            Command::new("model.select", l.text("Selecteren", "Select"), go(Message::SelectTool))
                .icon(small("view.fit-selection"), large("view.fit-selection"))
                .mode(app.selecting())
                .enabled(app.modeller.shown.is_some()),
            Command::new("model.butt", l.text("Stomp", "Butt"), go(Message::Connect(bim_core::ConnectionKind::Butt, l.text("Verbinding", "Connection"))))
                .icon(small("grid.end-couple"), large("grid.end-couple"))
                .enabled(app.selected_joinable()),
            Command::new("model.apart", l.text("Los", "Apart"), go(Message::Connect(bim_core::ConnectionKind::Apart, l.text("Verbinding", "Connection"))))
                .icon(small("edit.clear-all"), large("edit.clear-all"))
                .enabled(app.selected_joinable()),
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
                .toggle(app.modeller.edit.is_some() || app.modeller.sketch.is_some())
                .enabled(app.wants_tab()),
            Command::new("model.delete", l.text("Verwijderen", "Delete"), go(Message::DeletePicked))
                .icon(small("edit.delete"), large("edit.delete"))
                .enabled(!app.selected_modelled().is_empty()),
        ];
        // in an environment with Blender (the web app): a render of the file opened from Docs
        #[cfg(target_arch = "wasm32")]
        if crate::host::renders() {
            list.push(
                Command::new("file.render-blender", l.text("Renderen met Blender", "Render with Blender"), go(Message::BlenderRender))
                    .icon(small("mesh.surface"), large("mesh.surface"))
                    .enabled(crate::host::can_render()),
            );
        }
        // through the clashes with the keys: the next and the one before, accept, reject, issue
        // (the Validator's, which the web build leaves out)
        #[cfg(all(feature = "validator", not(target_arch = "wasm32")))]
        list.extend([
            Command::new("clash.next", l.text("Volgende clash", "Next clash"), go(Message::Collab(crate::app::collab::Event::Step(1)))).shortcut(key('j')).enabled(!app.collab.clashes.is_empty()),
            Command::new("clash.previous", l.text("Vorige clash", "Previous clash"), go(Message::Collab(crate::app::collab::Event::Step(-1)))).shortcut(key('k')).enabled(!app.collab.clashes.is_empty()),
            Command::new("clash.accept", l.text("Clash akkoord", "Accept clash"), go(Message::Collab(crate::app::collab::Event::DecidePicked(crate::app::collab::Decision::Accepted))))
                .shortcut(key('a'))
                .enabled(app.collab.clash.is_some() || app.collab.open_group.is_some()),
            Command::new("clash.reject", l.text("Clash afwijzen", "Reject clash"), go(Message::Collab(crate::app::collab::Event::DecidePicked(crate::app::collab::Decision::Rejected))))
                .shortcut(key('r'))
                .enabled(app.collab.clash.is_some() || app.collab.open_group.is_some()),
            Command::new("clash.issue", l.text("Issue van clash", "Issue of clash"), go(Message::Collab(crate::app::collab::Event::IssueFromPicked)))
                .shortcut(key('t'))
                .enabled(app.collab.clash.is_some() || app.collab.open_group.is_some()),
        ]);
        // move, copy, turn and mirror what is selected
        list.extend(crate::app::transform::TransformKind::ALL.map(|kind| {
            Command::new(kind.id(), kind.label(l), go(Message::Transform(kind)))
                .icon(small(kind.icon()), large(kind.icon()))
                .toggle(app.modeller.transform.as_ref().is_some_and(|t| t.kind == kind))
                .enabled(!app.selected_modelled().is_empty() || app.modeller.transform.is_some() || app.drawn_picked().is_some())
        }));
        // the Edit tools of the Draw tab, on the line or shape picked on a floor plan
        list.push(
            Command::new("edit.offset", l.text("Offset", "Offset"), go(Message::OffsetTool))
                .icon(small("transform.offset"), large("transform.offset"))
                .toggle(app.modeller.offset.is_some())
                .enabled(app.drawn_picked().is_some() || app.modeller.offset.is_some()),
        );
        let on_plan = app.shown_drawing().is_some_and(|v| app.document.plan(&v).is_some());
        list.extend(crate::app::drawn_edit::PairKind::ALL.map(|kind| {
            Command::new(kind.id(), kind.label(l), go(Message::PairEdit(kind)))
                .icon(small(kind.icon()), large(kind.icon()))
                .toggle(app.modeller.pair_edit.as_ref().is_some_and(|e| e.kind == kind))
                .enabled(on_plan)
        }));
        list.push(Command::new("edit.explode", l.text("Exploderen", "Explode"), go(Message::ExplodeDrawn)).icon(small("transform.explode"), large("transform.explode")).enabled(app.shape_picked()));
        // the project's levels, views, sheets and library
        use crate::document::PlanKind;
        let plan = |id: &'static str, label: &'static str, kind: PlanKind| {
            Command::new(id, label, go(Message::PickLevelFor((app.views_picker != Some(kind)).then_some(kind)))).icon(small("drawing.plan"), large("drawing.plan")).mode(app.views_picker == Some(kind)).enabled(loaded)
        };
        for view in crate::app::IfcView::ALL {
            list.push(
                Command::new(view.command(), view.label(l), go(Message::IfcView(view)))
                    .icon(small("panel.model-tree"), large("panel.model-tree"))
                    .mode(app.ifc.shown && app.ifc.view == view)
                    .enabled(loaded),
            );
        }
        list.push(Command::new("project.level", l.text("Bouwlaagpeil", "Level datum"), go(Message::NewLevel(l.text("Niveau", "Level")))).icon(small("modeller.level"), large("modeller.level")).enabled(loaded));
        // a grid line of the modelled project: two clicks on the work plane
        list.push(Command::new("project.grid", l.text("Stramienas", "Grid axis"), go(Message::GridTool)).icon(small("modeller.axis"), large("modeller.axis")).mode(app.modeller.grid_tool).enabled(app.modeller.shown.is_some()));
        list.push(plan("project.floor-plan", l.text("Plattegrond", "Floor plan"), PlanKind::Floor));
        list.push(plan("project.structural-plan", l.text("Constructieplan", "Structural plan"), PlanKind::Structural));
        list.push(plan("project.ceiling-plan", l.text("Plafondplan", "Ceiling plan"), PlanKind::Ceiling));
        list.push(Command::new("project.sheet", l.text("Tekenblad", "Sheet"), go(Message::NewSheet(l.text("Tekenblad", "Sheet")))).icon(small("drawing.sheet-new"), large("drawing.sheet-new")).enabled(loaded));
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
        for kind in crate::app::DrawKind::ALL {
            let icon = kind.icon();
            let on = app.draw_tool == Some(kind);
            let drawable = app.shown_drawing().is_some();
            list.push(Command::new(kind.command(), kind.label(l), go(Message::DrawTool(Some(kind)))).icon(small(icon), large(icon)).toggle(on && drawable).enabled(drawable));
        }
        list.push(
            Command::new("annotate.text", l.text("Tekst", "Text"), go(Message::NoteTool(!app.note_tool)))
                .icon(small("draw.text"), large("draw.text"))
                .toggle(app.note_tool && !app.level_tool && annotatable)
                .enabled(annotatable),
        );
        list.push(
            Command::new("annotate.level", l.text("Peilmaat", "Level mark"), go(Message::LevelTool(!(app.note_tool && app.level_tool))))
                .icon(small("resource.level"), large("resource.level"))
                .toggle(app.note_tool && app.level_tool && annotatable)
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
                Tool::Note => ("measure.note", "draw.text"),
                Tool::Arrow => ("measure.arrow", "draw.dimension"),
                Tool::Line => ("measure.line", "draw.line"),
                Tool::Cloud => ("measure.cloud", "draw.filled-area"),
            };
            list.push(Command::new(id, tool.label(l), go(Message::Measure(tool))).icon(small(icon), large(icon)).mode(app.measure.tool == Some(tool)).enabled(loaded));
        }
        // the switch at the top right; no shortcuts of their own until one is set
        let shown = self.product();
        for &product in &self.products {
            list.push(Command::new(product.command(), product.full_name(), Msg::Product(product)).mode(product == shown).detail(product.purpose(l)));
        }
        // Library, the product: the source whose library its page shows
        for source in crate::app::libraries::Source::ALL {
            let show = go(Message::Libraries(crate::app::LibrariesMessage::Source(source)));
            list.push(Command::new(source.command(), source.label(l), show).icon(small(source.icon()), large(source.icon())).mode(app.libraries.source == source).detail(source.purpose(l)));
        }
        // the Validator's commands: issues, checks, clashes and the rule set
        // Pointcloud: how the points show, the meshes, exports and drawing made of a cloud (on the
        // desktop) and the card with what the file holds and how the jobs are done
        {
            #[cfg(all(feature = "pointcloud", not(target_arch = "wasm32")))]
            let (cloud, idle, last, running) = (app.cloud_file().is_some(), !app.meshing, app.cloud.last.is_some(), app.meshing);
            #[cfg(all(feature = "pointcloud", not(target_arch = "wasm32")))]
            let event = |e: crate::app::pointcloud::Event| Some(go(Message::Pointcloud(e)));
            #[cfg(all(feature = "pointcloud", not(target_arch = "wasm32")))]
            use crate::app::pointcloud::{Event as Cloud, Export, Mesh};
            #[cfg(all(feature = "pointcloud", not(target_arch = "wasm32")))]
            let faces = event(Cloud::Mesh(Mesh::Faces));
            #[cfg(all(feature = "pointcloud", not(target_arch = "wasm32")))]
            let (terrain, surface, closed, smaller, larger) = (event(Cloud::Mesh(Mesh::Terrain)), event(Cloud::Mesh(Mesh::Surface)), event(Cloud::Mesh(Mesh::Closed)), event(Cloud::PointSize(false)), event(Cloud::PointSize(true)));
            #[cfg(all(feature = "pointcloud", not(target_arch = "wasm32")))]
            let (points, mesh, drawing, cancel) = (event(Cloud::Export(Export::Points)), event(Cloud::Export(Export::Mesh)), event(Cloud::Export(Export::Drawing)), event(Cloud::Cancel));
            #[cfg(not(all(feature = "pointcloud", not(target_arch = "wasm32"))))]
            let (cloud, idle, last, running) = (false, false, false, false);
            #[cfg(not(all(feature = "pointcloud", not(target_arch = "wasm32"))))]
            let (terrain, surface, closed, smaller, larger, points, mesh, drawing, cancel): (Option<Msg>, Option<Msg>, Option<Msg>, Option<Msg>, Option<Msg>, Option<Msg>, Option<Msg>, Option<Msg>, Option<Msg>) = (None, None, None, None, None, None, None, None, None);
            #[cfg(not(all(feature = "pointcloud", not(target_arch = "wasm32"))))]
            let faces: Option<Msg> = None;
            let boxed = app.active_section().is_some();
            let mut add = |id: &'static str, label: &'static str, icon: &'static str, message: Option<Msg>, on: bool, detail: &'static str| {
                list.push(Command::new(id, label, message.clone().unwrap_or(Msg::Dialog(None))).icon(small(icon), large(icon)).enabled(message.is_some() && on).detail(detail));
            };
            add("cloud.mesh-terrain", l.text("Terreinmesh", "Terrain mesh"), "mesh.surface", terrain, cloud && idle, l.text("Maakt een oppervlak over de punten, van boven gezien: maaiveld, een terrein, een dakvlak. Wanden en overstekken komen er niet in.", "Makes a surface over the points as seen from above: ground, a site, a roof plane. Walls and overhangs are left out."));
            add("cloud.mesh-surface", l.text("Oppervlakmesh", "Surface mesh"), "mesh.faces", surface, cloud && idle, l.text("Maakt een oppervlak door de punten in alle richtingen: wanden, plafonds, objecten.", "Makes a surface through the points in all directions: walls, ceilings, objects."));
            add("cloud.mesh-closed", l.text("Gesloten mesh", "Closed mesh"), "mesh.closed", closed, cloud && idle, l.text("Past een oppervlak op de punten, dicht kleine gaten en vereenvoudigt vlakke delen. Het langzaamst en het best voor gebouwen; zet de doorsnedebox om het deel dat je nodig hebt.", "Fits a surface to the points, closes small gaps and simplifies flat parts. The slowest and the best for buildings; put the section box around the part you need."));
            add("cloud.faces", l.text("Vlakken", "Faces"), "mesh.faces", faces, cloud && idle, l.text("Zoekt de vlakke wanden, vloeren, plafonds en daken en de ronde kolommen en leidingen in de punten, elk als een eigen vlak in de kleur van zijn soort. Zet de doorsnedebox om de ruimte die je nodig hebt.", "Finds the flat walls, floors, ceilings and roofs and the round columns and pipes in the points, each as a face of its own in the colour of its kind. Put the section box around the room you need."));
            add("cloud.points-smaller", l.text("Punten kleiner", "Smaller points"), "view.zoom-out", smaller, loaded, l.text("Tekent de punten van een puntenwolk een stap kleiner.", "Draws the points of a cloud a step smaller."));
            add("cloud.points-larger", l.text("Punten groter", "Larger points"), "view.zoom-in", larger, loaded, l.text("Tekent de punten van een puntenwolk een stap groter.", "Draws the points of a cloud a step larger."));
            add("cloud.export-points", l.text("Punten exporteren…", "Export points…"), "file.save-as", points, cloud && idle, l.text("Schrijft de punten van de wolk als LAS, LAZ, E57, PLY, XYZ, PTS of CSV: alle punten, een aandeel ervan, of wat in de doorsnedebox ligt.", "Writes the points of the cloud as LAS, LAZ, E57, PLY, XYZ, PTS or CSV: all of them, a share of them, or what lies in the section box."));
            add("cloud.export-mesh", l.text("Mesh exporteren…", "Export mesh…"), "bcf.export", mesh, last && idle, l.text("Schrijft de laatst gemaakte mesh als OBJ, PLY, STL, DXF, DWG of IFC, op de coördinaten van de puntenwolk.", "Writes the mesh made last as OBJ, PLY, STL, DXF, DWG or IFC, at the coordinates of the point cloud."));
            add("cloud.drawing", l.text("Doorsnedetekening…", "Section drawing…"), "drawing.section", drawing, cloud && idle && boxed, l.text("Tekent de punten in een plak bij een vlak van de doorsnedebox als DXF of DWG: de plattegrond bij het bovenvlak, of een aanzicht.", "Draws the points in a slab at a face of the section box as DXF or DWG: the plan at the top face, or an elevation."));
            add("cloud.cancel", l.text("Stoppen", "Stop"), "edit.clear-all", cancel, running, l.text("Stopt de mesh, de export of de tekening die wordt gemaakt.", "Stops the mesh, the export or the drawing that is being made."));
            list.push(
                Command::new("cloud.details", l.text("Gegevens", "Details"), Msg::Dialog(Some(Dialog::Cloud)))
                    .icon(small("status.info"), large("status.info"))
                    .enabled(cfg!(all(feature = "pointcloud", not(target_arch = "wasm32"))))
                    .detail(l.text("Wat het bestand van de puntenwolk bevat, en hoe de meshes, de export en de tekening worden gemaakt.", "What the file of the point cloud holds, and how the meshes, the export and the drawing are made."))
                    .dialog(),
            );
        }
        list.push(Command::new("cloud.depth", l.text("Diepte", "Depth"), go(Message::ToggleCloudDepth)).icon(small("view.isometric"), large("view.isometric")).toggle(app.cloud_depth).enabled(loaded).detail(l.text("Maakt punten donkerder waar ze achter hun buren liggen, zodat de vorm van een wolk te zien is.", "Darkens points where they lie behind their neighbours, so the shape of a cloud shows.")));
        // Structural Analysis: the internal forces, the deformed shape, reactions and loads
        {
            use crate::structural::diagrams::{Change, Force};
            let doc = app.model.as_ref().and_then(|m| m.structural.first()).map(|p| p.doc.clone());
            let results = doc.as_ref().and_then(|d| d.results.get(app.analysis.set)).and_then(|r| r.inline.as_ref());
            let (forces, moved, held) = (results.is_some_and(|r| r.member_forces.is_some()), results.is_some_and(|r| r.node_displacements.is_some()), results.is_some_and(|r| r.reactions.is_some()));
            let (cases, sets) = doc.as_ref().map_or((0, 0), |d| (d.load_cases.len(), d.results.len()));
            let named = [
                (Force::N, "analysis.n", l.text("Normaalkracht N", "Axial force N")),
                (Force::Vy, "analysis.vy", l.text("Dwarskracht Vy", "Shear force Vy")),
                (Force::Vz, "analysis.vz", l.text("Dwarskracht Vz", "Shear force Vz")),
                (Force::Mx, "analysis.mx", l.text("Wringing Mx", "Torsion Mx")),
                (Force::My, "analysis.my", l.text("Moment My", "Moment My")),
                (Force::Mz, "analysis.mz", l.text("Moment Mz", "Moment Mz")),
            ];
            for (force, id, label) in named {
                list.push(Command::new(id, label, go(Message::Analysis(Change::Force(force)))).icon(small("measure.perimeter"), large("measure.perimeter")).mode(app.analysis.force == Some(force)).enabled(forces).detail(l.text("Tekent deze snedekracht langs elke staaf, met de grootste waarde erbij.", "Draws this internal force along every member, with its largest value.")));
            }
            let mut add = |id: &'static str, label: &'static str, icon: &'static str, change: Change, on: bool, enabled: bool, detail: &'static str| {
                list.push(Command::new(id, label, go(Message::Analysis(change))).icon(small(icon), large(icon)).mode(on).enabled(enabled).detail(detail));
            };
            add("analysis.deformed", l.text("Vervorming", "Deformed shape"), "transform.flip-vertical", Change::Deformed, app.analysis.deformed, moved, l.text("Tekent de staven tussen hun verplaatste knopen, vergroot.", "Draws the members between their displaced nodes, enlarged."));
            add("analysis.reactions", l.text("Reacties", "Reactions"), "section.plane-z", Change::Reactions, app.analysis.reactions, held, l.text("Tekent bij elke oplegging de kracht op de constructie.", "Draws at every support the force on the structure."));
            add("analysis.supports", l.text("Opleggingen", "Supports"), "section.plane-z", Change::Supports, app.analysis.supports, loaded, l.text("Tekent bij elke oplegging haar symbool en bij elk scharnierend staafeinde een cirkel.", "Draws a symbol at every support and a circle at every released member end."));
            add("analysis.loads", l.text("Belastingen", "Loads"), "file.import", Change::Loads, app.analysis.loads.is_some(), cases > 0, l.text("Tekent de belastingen van een belastinggeval als pijlen.", "Draws the loads of a load case as arrows."));
            add("analysis.case-next", l.text("Volgend geval", "Next case"), "page.next", Change::Case(true), false, cases > 1, l.text("Toont de belastingen van het volgende belastinggeval.", "Shows the loads of the next load case."));
            add("analysis.case-previous", l.text("Vorig geval", "Previous case"), "page.previous", Change::Case(false), false, cases > 1, l.text("Toont de belastingen van het vorige belastinggeval.", "Shows the loads of the previous load case."));
            add("analysis.set-next", l.text("Volgend resultaat", "Next result set"), "page.next", Change::Set(true), false, sets > 1, l.text("Toont de resultaten van het volgende belastinggeval of de volgende combinatie.", "Shows the results of the next load case or combination."));
            add("analysis.set-previous", l.text("Vorig resultaat", "Previous result set"), "page.previous", Change::Set(false), false, sets > 1, l.text("Toont de resultaten van het vorige belastinggeval of de vorige combinatie.", "Shows the results of the previous load case or combination."));
            add("analysis.larger", l.text("Groter", "Larger"), "view.zoom-in", Change::Scale(true), false, loaded, l.text("Tekent de lijnen en pijlen groter.", "Draws the diagrams and arrows larger."));
            add("analysis.smaller", l.text("Kleiner", "Smaller"), "view.zoom-out", Change::Scale(false), false, loaded, l.text("Tekent de lijnen en pijlen kleiner.", "Draws the diagrams and arrows smaller."));
        }
        // which field of a simulation file colours its mesh, at which time step
        {
            use crate::app::fields::FieldChange as F;
            let fields = app.shown_fields();
            let (channels, steps) = fields.as_ref().map_or((0, 0), |f| (f.channels.len(), f.steps.len()));
            let ways = [
                ("field.next", F::NextChannel(true), l.text("Volgend veld", "Next field"), "page.next", channels > 0, l.text("Kleurt het net naar het volgende veld van het bestand, zoals een temperatuur of een verplaatsing.", "Colours the mesh by the file's next field, such as a temperature or a displacement.")),
                ("field.previous", F::NextChannel(false), l.text("Vorig veld", "Previous field"), "page.previous", channels > 0, l.text("Kleurt het net naar het vorige veld van het bestand.", "Colours the mesh by the file's previous field.")),
                ("field.step-next", F::NextStep(true), l.text("Volgende stap", "Next step"), "page.next", steps > 1, l.text("Toont de waarden van de volgende tijdstap.", "Shows the values of the next time step.")),
                ("field.step-previous", F::NextStep(false), l.text("Vorige stap", "Previous step"), "page.previous", steps > 1, l.text("Toont de waarden van de vorige tijdstap.", "Shows the values of the previous time step.")),
            ];
            for (id, change, label, icon, enabled, detail) in ways {
                list.push(Command::new(id, label, go(Message::Field(change))).icon(small(icon), large(icon)).enabled(enabled).detail(detail));
            }
        }
        // how the points of a cloud are coloured
        {
            use crate::cloud_color::CloudColor as C;
            let ways = [
                ("cloud.color-stored", C::Stored, l.text("Eigen kleur", "Stored colour"), "object.show", l.text("Kleurt de punten zoals het bestand ze bewaart: hun kleuren, of hun klasse of scan met de intensiteit als tint.", "Colours the points as the file stores them: their colours, or their class or scan shaded by intensity.")),
                ("cloud.color-elevation", C::Elevation, l.text("Hoogte", "Elevation"), "resource.level", l.text("Kleurt de punten naar hun hoogte, van blauw laag naar rood hoog.", "Colours the points by their height, from blue low to red high.")),
                ("cloud.color-intensity", C::Intensity, l.text("Intensiteit", "Intensity"), "theme.light", l.text("Kleurt de punten naar wat ze terugkaatsten, van donker naar licht. Een bestand zonder intensiteit wordt wit.", "Colours the points by what they reflected, from dark to light. A file without intensity turns white.")),
                ("cloud.color-class", C::Class, l.text("Klasse of scan", "Class or scan"), "construction.types", l.text("Geeft elke klasse van een geclassificeerde wolk, of elke scan, één kleur.", "Gives every class of a classified cloud, or every scan, one colour.")),
            ];
            for (id, way, label, icon, detail) in ways {
                list.push(Command::new(id, label, go(Message::CloudColor(way))).icon(small(icon), large(icon)).mode(app.cloud_color == way).enabled(loaded).detail(detail));
            }
        }
        for planned in PLANNED {
            #[cfg(all(feature = "validator", not(target_arch = "wasm32")))]
            let message = planned.event().map(|e| go(Message::Collab(e)));
            #[cfg(not(all(feature = "validator", not(target_arch = "wasm32"))))]
            let message: Option<Msg> = None;
            list.push(Command::new(planned.id, planned.label(l), message.clone().unwrap_or(Msg::Dialog(None))).icon(small(planned.icon), large(planned.icon)).enabled(message.is_some()).detail(planned.detail(l)));
        }
        list
    }

    /// The commands with the user's own shortcuts.
    fn commands(&self) -> Vec<Command<Msg>> {
        let mut commands = self.default_commands();
        if self.product() == Product::Viewer {
            for command in &mut commands {
                if let Msg::App(message) = &command.message {
                    command.enabled &= message.allowed_in_viewer();
                }
            }
        }
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
        // the Viewer only looks at a view: its bar has no tools to dimension, write or cut
        self.app.set_viewer_mode(self.product() == Product::Viewer);
        #[cfg(all(feature = "validator", not(target_arch = "wasm32")))]
        {
            self.app.collab.language = self.l();
            self.app.collab.refresh();
            self.app.collab_refresh_sides();
        }
        // the app knows whether the Modeller is shown: there an opened file's walls have grips
        self.app.modeller.in_modeller = self.product() == Product::Modeller;
        // a document picked at an element of the model opens in Docs, the product
        if matches!(message, Msg::App(Message::Docs(crate::app::DocsMessage::Show(_)))) {
            self.set_product(Product::Docs);
        }
        let task = self.handle(message);
        self.app.modeller.in_modeller = self.product() == Product::Modeller;
        let docs = self.docs_follow_product();
        let ifcx = self.ifcx_follow_product();
        self.keep_dev_session();
        Task::batch([task, docs, ifcx])
    }

    fn handle(&mut self, message: Msg) -> Task<Msg> {
        match message {
            // the window driven as a person drives it: its controls, a click, typing
            #[cfg(not(target_arch = "wasm32"))]
            Msg::App(Message::Remote(call)) if call.cmd.starts_with("ui_") => return self.ui_command(call),
            Msg::App(message) if self.product() == Product::Viewer && !message.allowed_in_viewer() => return Task::none(),
            Msg::App(message) => {
                // how the 3D view draws its grids and levels is kept between sessions
                let kept = (self.app.grid_heads_hidden, self.app.grid_dims, self.app.levels_3d, self.app.ambient);
                if kept != (self.prefs.grid_heads_hidden, self.prefs.grid_dims, self.prefs.levels_3d, self.prefs.ambient) {
                    (self.prefs.grid_heads_hidden, self.prefs.grid_dims, self.prefs.levels_3d, self.prefs.ambient) = kept;
                    self.save_prefs();
                }
                // what opens in the explorer shows it
                if matches!(message, Message::NewLevel(_) | Message::PickLevelFor(Some(_)) | Message::ExplorerTab(_)) && !self.prefs.left {
                    self.prefs.left = true;
                    self.save_prefs();
                }
                // joins made automatically are named in the interface language
                self.app.modeller.join_word = self.l().text("Verbinding", "Connection");
                self.app.modeller.space_word = self.l().text("Ruimte", "Space");
                self.app.modeller.top_word = self.l().text("b.k.", "top");
                self.app.modeller.pile_words = [
                    self.l().text("Palenstaat", "Pile schedule"),
                    self.l().text("Peil", "Level"),
                    self.l().text("Nr.", "No."),
                    "X (mm)",
                    "Y (mm)",
                    self.l().text("Paal", "Pile"),
                    self.l().text("Sterkteklasse", "Grade"),
                    self.l().text("Afhakhoogte", "Cut-off level"),
                    self.l().text("Paalpunt", "Pile tip"),
                    self.l().text("Lengte (mm)", "Length (mm)"),
                ];
                self.app.modeller.snap_words = crate::app::modeller::snaps::Snap::ALL.map(|s| s.label(self.l()));
                let opened = match &message {
                    Message::Open(path) => Some(path.clone()),
                    _ => None,
                };
                if opened.is_some() || matches!(message, Message::OpenDialog | Message::SaveDialog | Message::SaveProjectDialog | Message::SaveIfcDialog(_) | Message::BlenderRender) {
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
                // a model changed and the rule set asks for it: the clash tests run again
                #[cfg(all(feature = "validator", not(target_arch = "wasm32")))]
                let task = if self.app.collab.rerun_due() { Task::batch([task, Task::done(Msg::Project(id, Message::Collab(crate::app::collab::Event::Clash)))]) } else { task };
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
                // the card of a cloud reads what its file holds when it opens
                #[cfg(all(feature = "pointcloud", not(target_arch = "wasm32")))]
                let read = match dialog {
                    Some(Dialog::Cloud) => self.update(Msg::App(Message::Pointcloud(crate::app::pointcloud::Event::Inspect))),
                    _ => Task::none(),
                };
                #[cfg(not(all(feature = "pointcloud", not(target_arch = "wasm32"))))]
                let read: Task<Msg> = Task::none();
                if dialog.is_none() {
                    self.shortcuts = sc::Editor::default();
                } else {
                    self.backstage = false;
                }
                if opening {
                    return Task::batch([read, openaec_ui::keyboard::navigate(Navigation::Next)]);
                }
                return read;
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
            Msg::TextSize(size) => {
                self.prefs.text_size = match size {
                    TextSize::Compact => "compact",
                    TextSize::Standard => "standard",
                    TextSize::Large => "large",
                }
                .into();
                self.save_prefs();
            }
            Msg::RibbonGroup(key, shown) => {
                self.prefs.ribbon_hidden.retain(|k| k != key);
                if !shown {
                    self.prefs.ribbon_hidden.push(key.to_string());
                }
                self.save_prefs();
            }
            Msg::RibbonReset => {
                self.prefs.ribbon_hidden.clear();
                self.save_prefs();
            }
            Msg::Extension(product, on) => {
                self.prefs.extensions_off.retain(|key| key != product.key());
                // the Viewer is the suite itself: it stays
                if !on && product != Product::Viewer {
                    self.prefs.extensions_off.push(product.key().to_string());
                }
                self.apply_extensions();
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
            Msg::ShowModel3d => {
                if self.product() != Product::Viewer && self.products.contains(&Product::Viewer) {
                    self.set_product(Product::Viewer);
                }
                self.backstage = false;
                self.compact_panel = None;
                return self.app.update(Message::SelectTab(0)).map(Msg::App);
            }
            Msg::SelectTab(i) => return self.update(Msg::App(Message::SelectTab(i))),
            // the 3D view (tab 0) stays open
            Msg::CloseTab(i) => return self.update(Msg::App(Message::CloseTab(i))),
            Msg::Key(press) if self.shortcuts.recording() => return self.update(Msg::Shortcuts(sc::Message::Key(press))),
            Msg::KeyWindow => self.expire_keys(Instant::now()),
            Msg::Key(press) => {
                use iced::keyboard::{key::Named, Key};
                if press.key == Key::Named(Named::Escape) {
                    // a letter waiting for its pair goes, and nothing else
                    if self.keys.pending().is_some() {
                        self.keys.clear();
                        return Task::none();
                    }
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
                // while elements are grabbed (G), X, Y and Z hold the move and digits type its distance
                let grabbing = self.app.modeller.grab.is_some();
                if self.dialog.is_none() && grabbing && !press.modifiers.control() && !press.modifiers.alt() {
                    // R right after the G that grabbed: GR, the grid tool
                    if self.keys.pending() == Some('g') && matches!(&press.key, Key::Character(c) if c.eq_ignore_ascii_case("r")) {
                        let steps = self.keys.steps('r', Instant::now());
                        if steps.contains(&crate::app::modeller::keys::Step::Key(crate::app::modeller::keys::ModelKey::Grid)) {
                            let l = self.l();
                            return self.update(Msg::App(Message::ModelKey(crate::app::modeller::keys::ModelKey::Grid, l)));
                        }
                    }
                    if let Key::Character(c) = &press.key {
                        if let Some(c) = c.chars().next().filter(|c| "xyzXYZ".contains(*c) || crate::app::modeller::typed_move::DIGITS.contains(*c)) {
                            return self.update(Msg::App(Message::EditKey(c)));
                        }
                    }
                }
                // and in the sketch of a floor L, R, A and T are its tools
                let sketching = self.app.modeller.sketch.is_some();
                if self.dialog.is_none() && sketching && !press.modifiers.control() && !press.modifiers.alt() {
                    if let Key::Character(c) = &press.key {
                        if c.eq_ignore_ascii_case("c") || (self.keys.pending() == Some('c') && c.eq_ignore_ascii_case("o")) {
                            use crate::app::modeller::keys::{ModelKey, Step};
                            let steps = self.keys.steps(c.chars().next().unwrap(), Instant::now());
                            if steps.contains(&Step::Key(ModelKey::Copy)) {
                                return self.key_step(Step::Key(ModelKey::Copy));
                            }
                            return Task::none();
                        }
                    }
                    self.keys.clear();
                    if let Key::Character(c) = &press.key {
                        if let Some(c) = c.chars().next().filter(|c| "lratLRAT".contains(*c)) {
                            return self.update(Msg::App(Message::EditKey(c)));
                        }
                    }
                }
                if self.dialog.is_none() && self.app.modeller.edit.is_some() && !press.modifiers.control() && !press.modifiers.alt() {
                    if let Key::Character(c) = &press.key {
                        // and while a point is grabbed, the digits of a typed distance
                        let grabbed = self.app.modeller.edit.as_ref().is_some_and(|e| e.grab.is_some());
                        if let Some(c) = c.chars().next().filter(|c| "gxyzaGXYZA".contains(*c) || (grabbed && crate::app::modeller::typed_move::DIGITS.contains(*c))) {
                            return self.update(Msg::App(Message::EditKey(c)));
                        }
                    }
                }
                // in the Modeller a plain letter is a key of its own (see `app::modeller::keys`),
                // and PageUp / PageDown step through the levels
                // (not in edit mode, whose own keys came before; its other letters stay the app's)
                if self.dialog.is_none() && !self.backstage && self.product() == Product::Modeller && self.app.modeller.edit.is_none() && !sketching && !grabbing && !press.modifiers.control() && !press.modifiers.alt() {
                    use crate::app::modeller::keys::{ModelKey, Step};
                    // while a tool draws, digits type the length of what it draws
                    if let Key::Character(c) = &press.key {
                        if let Some(c) = c.chars().next().filter(|c| !c.is_ascii_alphabetic() && self.app.tool_drawing()) {
                            if !press.modifiers.shift() {
                                return self.update(Msg::App(Message::ToolDigit(c)));
                            }
                        }
                    }
                    // Shift+C takes the 3D cursor home, as in Blender
                    if press.modifiers.shift() && matches!(&press.key, Key::Character(c) if c.eq_ignore_ascii_case("c")) {
                        return self.update(Msg::App(Message::Cursor3dHome));
                    }
                    let steps: Option<Vec<Step>> = match &press.key {
                        Key::Character(c) => c.chars().next().filter(|c| c.is_ascii_alphabetic()).map(|c| self.keys.steps(c, Instant::now())),
                        Key::Named(Named::PageUp) => Some(vec![Step::Key(ModelKey::LevelUp)]),
                        Key::Named(Named::PageDown) => Some(vec![Step::Key(ModelKey::LevelDown)]),
                        _ => None,
                    };
                    if let Some(steps) = steps {
                        let tasks: Vec<Task<Msg>> = steps.into_iter().map(|step| self.key_step(step)).collect();
                        return Task::batch(tasks);
                    }
                }
                if self.dialog.is_none() {
                    if let Some(message) = cm::find(&self.commands(), &press.key, press.modifiers) {
                        return self.update(message);
                    }
                    // no command of its own: W, A, S and D walk through the model in the Viewer, Q
                    // and E go down and up, faster with Shift
                    if let (Product::Viewer, false, false, Key::Character(c)) = (self.product(), press.modifiers.control(), press.modifiers.alt(), &press.key) {
                        let (forward, right, up) = match c.to_lowercase().as_str() {
                            "w" => (1.0, 0.0, 0.0),
                            "s" => (-1.0, 0.0, 0.0),
                            "d" => (0.0, 1.0, 0.0),
                            "a" => (0.0, -1.0, 0.0),
                            "e" => (0.0, 0.0, 1.0),
                            "q" => (0.0, 0.0, -1.0),
                            _ => (0.0, 0.0, 0.0),
                        };
                        if (forward, right, up) != (0.0, 0.0, 0.0) {
                            return self.update(Msg::App(Message::Walk { forward, right, up, fast: press.modifiers.shift() }));
                        }
                    }
                }
            }
            // Tab on a selected modelled element opens its edit mode, as in Blender; else it moves
            // the focus
            Msg::Navigate(openaec_ui::keyboard::Navigation::Next) if self.dialog.is_none() && !self.backstage && self.app.wants_tab() => {
                self.keys.clear();
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
            Msg::WindowOpened(id) => {
                self.window_open = true;
                return iced::window::raw_id::<Msg>(id).map(Msg::NativeWindow);
            }
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
            Msg::Feedback(step) => {
                let form = &mut self.feedback;
                match step {
                    FeedbackMsg::Email(e) => form.email = e,
                    FeedbackMsg::Name(n) => form.name = n,
                    FeedbackMsg::Kind(k) => form.kind = k,
                    FeedbackMsg::Message(action) => form.message.perform(action),
                    FeedbackMsg::Mood(m) => form.mood = if form.mood == Some(m) { None } else { Some(m) },
                    // another one: who writes stays filled in
                    FeedbackMsg::Again => *form = Feedback { email: std::mem::take(&mut form.email), name: std::mem::take(&mut form.name), ..Feedback::default() },
                    FeedbackMsg::Done(result) => {
                        form.state = match result {
                            Ok(()) => FeedbackState::Sent,
                            Err(e) => FeedbackState::Failed(e),
                        }
                    }
                    FeedbackMsg::Send if form.ready() => {
                        form.state = FeedbackState::Sending;
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            let payload = form.payload();
                            let (tx, rx) = iced::futures::channel::oneshot::channel();
                            std::thread::spawn(move || {
                                let _ = tx.send(send_feedback(payload));
                            });
                            return Task::perform(async move { rx.await.unwrap_or_else(|_| Err("not sent".to_string())) }, |r| Msg::Feedback(FeedbackMsg::Done(r)));
                        }
                    }
                    FeedbackMsg::Send => {}
                }
            }
            Msg::PlannerReload => {
                // a frame that could not be made is tried again: `update_with_frame` lays it
                if self.planner_failed.take().is_some() {
                    self.planner_shown = None;
                    self.app.status = Status::Ready;
                    return Task::none();
                }
                #[cfg(all(feature = "planner", windows))]
                return iced::window::latest().and_then(|id| iced::window::run(id, |_| crate::planner_frame::reload())).map(Msg::PlannerDone);
            }
            Msg::PlannerPointer(at) => self.planner_pointer = at,
            Msg::PlannerWindow => {}
            Msg::PlannerPress => {
                #[cfg(all(feature = "planner", windows))]
                if self.planner_shown.is_some() {
                    return iced::window::latest().and_then(|id| iced::window::run(id, |_| crate::planner_frame::release_keyboard())).map(Msg::PlannerDone);
                }
            }
            Msg::PlannerBrowser => {
                if let Err(e) = open_url(PLANNER_URL) {
                    self.app.status = Status::Error(e);
                }
            }
            // a frame that could not be made: the page under it says the web view is missing, the
            // status bar says what the system gave
            Msg::PlannerFrame(Err(e)) => {
                self.app.status = Status::Error(format!("{}: {e}", self.l().text("Webframe", "Web frame")));
                self.planner_failed = Some(e);
            }
            Msg::PlannerDone(Err(e)) => self.app.status = Status::Error(e),
            Msg::PlannerFrame(Ok(())) | Msg::PlannerDone(Ok(())) => {}
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
        // the Planner's web frame is a window of the system: the shell follows the pointer over
        // its own drawing to make room for a tip, and takes the keyboard back on a press
        let planner = if self.product() == Product::Planner && self.planner_failed.is_none() {
            iced::event::listen_with(|event, _, _| match event {
                iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => Some(Msg::PlannerPointer(Some(position))),
                iced::Event::Mouse(iced::mouse::Event::CursorLeft) => Some(Msg::PlannerPointer(None)),
                iced::Event::Mouse(iced::mouse::Event::ButtonPressed(_)) => Some(Msg::PlannerPress),
                // the window moved, changed size or went to another display: the frame follows
                iced::Event::Window(iced::window::Event::Moved(_) | iced::window::Event::Resized(_) | iced::window::Event::Rescaled(_) | iced::window::Event::Focused) => Some(Msg::PlannerWindow),
                _ => None,
            })
        } else {
            Subscription::none()
        };
        Subscription::batch([
            self.app.subscription().map(Msg::App),
            planner,
            resized,
            opened,
            open.map(Msg::App),
            drag,
            openaec_ui::keyboard::presses().map(Msg::Key),
            // while a letter waits for its pair, the time is looked at every frame
            if self.keys.pending().is_some() { iced::window::frames().map(|_| Msg::KeyWindow) } else { Subscription::none() },
            openaec_ui::keyboard::navigation().map(Msg::Navigate),
            iced::window::close_requests().map(|_| Msg::CloseWindow),
        ])
    }

    pub fn title(&self) -> String {
        let title = match &self.app.model {
            Some(m) => format!("{} · {} {VERSION}", m.file_name(), self.edition.name()),
            None => format!("{} {VERSION}", self.edition.name()),
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
        let status: Element<'_, Msg> = if compact { w::status_bar(look, vec![one_line(&left.join(" · "), self.width)], Vec::new()) } else { row![w::status_bar(look, left, vec![format!("{} {VERSION}", self.edition.name())]), self.snap_switches(look), self.mcp_lamp(look)].into() };
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

    /// The snaps of the Modeller at the right of the status bar, each a switch: on in the accent
    /// colour. Nothing in the other products.
    fn snap_switches(&self, look: Look) -> Element<'_, Msg> {
        use crate::app::modeller::snaps::Snap;
        if self.product() != Product::Modeller {
            return Space::new().into();
        }
        let l = self.l();
        let c = look.color;
        let mut line = iced::widget::Row::new().spacing(2).align_y(iced::Alignment::Center);
        line = line.push(iced::widget::text("SNAP").size(10).font(fonts::MONO).color(c.statusbar_text));
        for snap in Snap::ALL {
            let on = self.app.modeller.snaps.is(snap);
            let label = iced::widget::text(snap.label(l)).size(11).color(if on { c.accent } else { c.statusbar_text });
            let switch = iced::widget::button(label)
                .padding([1, 6])
                .style(move |_, status| iced::widget::button::Style {
                    background: matches!(status, iced::widget::button::Status::Hovered).then(|| c.frame.into()),
                    border: iced::Border { radius: 3.0.into(), width: if on { 1.0 } else { 0.0 }, color: c.accent },
                    ..Default::default()
                })
                .on_press(Msg::App(Message::SnapToggle(snap)));
            line = line.push(w::with_tooltip(look, switch.into(), snap.tip(l), None));
        }
        let cell = column![
            w::divider(look),
            container(line.spacing(6))
                .height(openaec_ui::design::tokens::shell::STATUSBAR - 1.0)
                .padding([0, 10])
                .align_y(iced::alignment::Vertical::Center)
                .style(move |_| container::Style { background: Some(c.statusbar.into()), ..Default::default() }),
        ]
        .width(Length::Shrink);
        cell.into()
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
        // the Modeller changes the model, so taking a step back or forth is at hand there
        let modelling = self.product() == Product::Modeller;
        let ids: &[&str] = if self.product() == Product::Viewer { &["file.open"] } else if modelling { &["file.new", "file.open", "file.save-as", "edit.undo", "edit.redo"] } else { &["file.new", "file.open", "file.save-as"] };
        let mut quick = vec![cm::button(look, &self.command("view.3d"), K::Quiet, D::Compact)];
        quick.extend(ids.iter().map(|id| cm::icon_button(look, &self.command(id), 28.0)));
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
        // feedback on the app, from the desktop app (it sends it itself)
        #[cfg(not(target_arch = "wasm32"))]
        extra.push(w::button(look, self.l().text("Feedback", "Feedback"), K::Quiet, D::Compact, Some(small("ui.feedback")), Some(Msg::Dialog(Some(Dialog::Feedback)))));
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
            // grey while it is not ready to work with yet; its tooltip says how far along it is
            let ink = if on {
                c.text
            } else if product.stage().is_some() {
                c.text_disabled
            } else {
                c.text_secondary
            };
            let label = text(product.label()).size(font::SIZE_BODY).font(if on { fonts::STRONG } else { fonts::MEDIUM }).color(ink);
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
            return w::with_tooltip(look, segment(shown, true, Msg::Product(next)), next.purpose(l), next.stage());
        }
        let segments = self.products.iter().map(|&p| w::with_tooltip(look, segment(p, p == shown, Msg::Product(p)), p.purpose(l), p.stage()));
        // the products to come: named, not to be chosen yet (names as the products', the same in
        // every language)
        let soon = |name: &'static str| -> Element<'_, Msg> { container(text(name).size(font::SIZE_BODY).font(fonts::MEDIUM).color(c.text_disabled)).height(22).padding([0, 8]).align_y(iced::alignment::Vertical::Center).into() };
        let note: Element<'_, Msg> = container(text("(coming soon)").size(font::SIZE_BODY - 2.0).font(fonts::BODY).color(c.text_disabled)).height(22).padding(iced::Padding { right: 8.0, ..iced::Padding::ZERO }).align_y(iced::alignment::Vertical::Center).into();
        // (where the window has room for them beside the title)
        let room = self.width >= SOON_WIDTH;
        let segments = segments.chain(SOON.map(soon).into_iter().filter(move |_| room)).chain(room.then_some(note));
        container(row(segments).spacing(2).align_y(iced::Alignment::Center))
            .padding(2)
            .style(move |_| container::Style { border: iced::Border { color: c.border, width: 1.0, radius: radius::MEDIUM.into() }, ..Default::default() })
            .into()
    }

    /// The groups of the ribbon tab shown, each its label and its parts, before the user's
    /// choice of which to show.
    fn ribbon_groups(&self) -> Vec<(&'static str, Vec<RibbonPart>)> {
        self.ribbon_groups_of(self.ribbon_page())
    }

    /// The groups of a ribbon tab: the tools of the extension it belongs to.
    fn ribbon_groups_of(&self, page: RibbonPage) -> Vec<(&'static str, Vec<RibbonPart>)> {
        let l = self.l();
        // a group is its label and its parts: a large button, or a column of small ones
        let large = |id: &'static str| RibbonPart::Large(id);
        let smalls = |ids: &[&'static str]| RibbonPart::Smalls(ids.to_vec());
        // One tab: the main command of a group large, the rest small in columns of three
        let tools: Vec<&str> = Tool::ALL.iter().map(|t| Self::tool_id(*t)).collect();
        let (first_tools, more_tools) = tools.split_at(3.min(tools.len()));
        // the Viewer's tools, also the View tab of the Modeller and the Validator
        let view_groups = || {
            let mut measure = vec![smalls(first_tools)];
            for column in more_tools.chunks(3) {
                measure.push(smalls(column));
            }
            measure.push(smalls(&["measure.clear", "measure.clear-saved"]));
            vec![
                (l.text("Weergave", "Display"), vec![smalls(&["view.solid", "view.wireframe", "view.edges", "view.hidden"]), smalls(&["view.shadows"])]),
                (l.text("Tonen", "Show"), vec![smalls(&["view.grid", "view.grid-lines", "view.grid-dims"]), smalls(&["view.levels-3d", "view.views-3d", "view.flip-up"])]),
                ("IFC", vec![large("view.ifc")]),
                (l.text("Camera", "Camera"), vec![large("view.3d"), large("camera.fit"), smalls(&["camera.iso", "camera.top", "camera.front"]), smalls(&["camera.right", "camera.perspective", "camera.ortho"])]),
                (l.text("Selectie", "Selection"), vec![smalls(&["object.hide", "object.isolate", "object.show-all"]), smalls(&["selection.layer"])]),
                (l.text("Doorsnede", "Section"), vec![large("section.box"), large("section.plane"), smalls(&["section.flip", "section.selection", "section.fill"]), smalls(&["section.reset"])]),
                (l.text("Meten", "Measure"), measure),
                (l.text("Extra", "More"), vec![large("view.explode"), smalls(&["view.left", "view.right"])]),
            ]
        };
        let groups: Vec<(&'static str, Vec<RibbonPart>)> = match page {
            RibbonPage::Home => {
                let mut groups = vec![(l.text("Bestand", "File"), vec![large("file.new"), large("file.open"), smalls(&["file.open-folder", "file.add", "file.save-as"])])];
                #[cfg(target_arch = "wasm32")]
                if crate::host::renders() {
                    groups[0].1.push(large("file.render-blender"));
                }
                // a simulation file: its fields and time steps beside the file
                if self.app.shown_fields().is_some() {
                    groups.push((l.text("Velden", "Fields"), vec![smalls(&["field.next", "field.previous"]), smalls(&["field.step-next", "field.step-previous"])]));
                }
                groups.extend(view_groups());
                groups.push((l.text("Visualisatie", "Visualisation"), vec![smalls(&["file.render", "file.build-film"])]));
                groups
            }
            RibbonPage::View => view_groups(),
            // what is built, in the order of the five base elements: planar (wall, slab), linear
            // (column, beam), system, point; then the openings in walls, the connection that joins
            // two selected elements, and changes. New, open and save are on the File page and in
            // the title bar, as in other BIM modellers.
            RibbonPage::Model => vec![
                (l.text("Selecteren", "Select"), vec![large("model.select")]),
                // what the building is set out on: its grid lines and its levels
                (l.text("Referenties", "Datums"), vec![large("project.grid"), large("project.level")]),
                (l.text("Vlak", "Planar"), vec![large("model.wall"), large("model.slab")]),
                (l.text("Lijnvormig", "Linear"), vec![large("model.column"), large("model.beam")]),
                (l.text("Systeem", "System"), vec![large("model.system")]),
                (l.text("Puntvormig", "Point"), vec![large("model.point")]),
                (l.text("Openingen", "Openings"), vec![large("model.door"), large("model.window")]),
                (l.text("Verbinding", "Connection"), vec![smalls(&["model.butt", "model.mitre", "model.cut"]), smalls(&["model.end-plate", "model.apart"])]),
                (l.text("Wijzigen", "Modify"), vec![large("model.edit"), smalls(&["model.move", "model.copy"]), smalls(&["model.rotate", "model.mirror"]), smalls(&["model.align"]), large("model.delete")]),
            ],
            // the project as its IFCX file has it: the ways to look at it, and saving it
            RibbonPage::Ifcx => vec![
                ("IFCX", crate::app::IfcView::ALL.iter().map(|v| large(v.command())).collect()),
                (l.text("Bestand", "File"), vec![large("file.save-as")]),
            ],
            // what the model is drawn on and in: its levels, its views and sheets, its types
            RibbonPage::Project => vec![
                (l.text("Niveaus", "Levels"), vec![large("project.level")]),
                (l.text("Stramien", "Grid"), vec![large("project.grid")]),
                (l.text("Weergaven", "Views"), vec![large("project.floor-plan"), smalls(&["project.structural-plan", "project.ceiling-plan", "annotate.section"])]),
                (l.text("Tekenbladen", "Sheets"), vec![large("project.sheet")]),
                (l.text("Bibliotheek", "Library"), vec![large("project.library")]),
            ],
            // drawing in the floor plan, section or sheet shown, in the order of a drawing program's
            // first tab: selecting, the lines and shapes that are no model, what is written and
            // measured on the drawing, and changing what is selected
            RibbonPage::Draw => vec![
                (l.text("Selectie", "Selection"), vec![large("model.select")]),
                (l.text("Tekenen", "Draw"), vec![smalls(&["draw.line", "draw.polyline", "draw.rectangle"]), smalls(&["draw.circle", "draw.arc", "draw.ellipse"])]),
                (l.text("Annoteren", "Annotate"), vec![large("annotate.dimension"), smalls(&["annotate.standard-dimensions", "annotate.text", "annotate.level"])]),
                (l.text("Wijzigen", "Modify"), vec![smalls(&["model.move", "model.copy", "model.rotate"]), smalls(&["model.mirror", "model.edit", "model.delete"])]),
                (l.text("Bewerken", "Edit"), vec![smalls(&["edit.trim", "edit.extend", "edit.offset"]), smalls(&["edit.fillet", "edit.chamfer", "edit.explode"])]),
            ],
            // the clouds of the project, how their points show and the meshes made of them
            RibbonPage::Analysis => vec![
                (l.text("Rekenmodellen", "Analysis models"), vec![large("file.open"), smalls(&["file.open-folder", "file.add", "file.save-as"])]),
                (l.text("Snedekrachten", "Internal forces"), vec![smalls(&["analysis.n", "analysis.vy", "analysis.vz"]), smalls(&["analysis.mx", "analysis.my", "analysis.mz"])]),
                (l.text("Resultaten", "Results"), vec![large("analysis.deformed"), large("analysis.reactions"), large("analysis.supports"), smalls(&["analysis.set-next", "analysis.set-previous"])]),
                (l.text("Belastingen", "Loads"), vec![large("analysis.loads"), smalls(&["analysis.case-next", "analysis.case-previous"])]),
                (l.text("Schaal", "Scale"), vec![smalls(&["analysis.larger", "analysis.smaller"])]),
                (l.text("Velden", "Fields"), vec![smalls(&["field.next", "field.previous"]), smalls(&["field.step-next", "field.step-previous"])]),
            ],
            // the four sources of library content
            RibbonPage::Library => vec![(l.text("Bibliotheek", "Library"), vec![large("library.general"), large("library.country"), large("library.company"), large("library.project")])],
            // the project folder, the picked document, and its link with the model
            RibbonPage::Docs => vec![
                (l.text("Projectmap", "Project folder"), vec![large("docs.folder"), smalls(&["docs.refresh", "docs.leave"])]),
                (l.text("Document", "Document"), vec![large("docs.open"), smalls(&["docs.open-outside", "docs.reveal"])]),
                (l.text("Model", "Model"), vec![large("docs.link"), large("docs.save")]),
            ],
            // the place is searched in the GIS tab of the right pane; what is fetched around it is here
            RibbonPage::Gis => vec![
                (l.text("Bestand", "File"), vec![large("file.open"), smalls(&["file.add", "file.save-as"])]),
                (l.text("Kaart", "Map"), vec![large("gis.map"), smalls(&["camera.top", "camera.ortho", "camera.perspective"])]),
                (l.text("Omgeving", "Surroundings"), vec![large("gis.surroundings")]),
                (l.text("Terrein", "Terrain"), vec![large("gis.terrain"), large("gis.points")]),
            ],
            // roads, structures and rail: the pages are there, their tools are still to come
            RibbonPage::Infra | RibbonPage::Civil | RibbonPage::Rail => vec![(l.text("Bestand", "File"), vec![large("file.open"), smalls(&["file.add", "file.save-as"])])],
            // the page itself is the program; the ribbon only loads it again or opens it outside
            RibbonPage::Planner => vec![("Planner", vec![large("planner.reload"), large("planner.browser")])],
            // the ground under the project: what was investigated
            RibbonPage::Geo => vec![
                (l.text("Bestand", "File"), vec![large("file.open"), smalls(&["file.add", "file.save-as"])]),
                (l.text("Sonderingen", "Soundings"), vec![large("geo.fetch"), large("geo.open")]),
                (l.text("Boringen", "Borings"), vec![large("geo.borings"), large("geo.open-borings")]),
                (l.text("Ondergrond", "Ground"), vec![large("geo.model")]),
            ],
            RibbonPage::Cloud => vec![
                (l.text("Puntenwolken", "Point clouds"), vec![large("file.open"), smalls(&["file.open-folder", "file.add", "file.save-as"]), large("cloud.details")]),
                (l.text("Punten", "Points"), vec![smalls(&["cloud.points-smaller", "cloud.points-larger", "cloud.depth"])]),
                (l.text("Kleur", "Colour"), vec![smalls(&["cloud.color-stored", "cloud.color-elevation"]), smalls(&["cloud.color-intensity", "cloud.color-class"])]),
                (l.text("Doorsnede", "Section"), vec![large("section.box"), large("cloud.drawing")]),
                (l.text("Mesh", "Mesh"), vec![large("cloud.mesh-terrain"), large("cloud.mesh-surface"), large("cloud.mesh-closed"), large("cloud.faces"), smalls(&["cloud.cancel"])]),
                (l.text("Exporteren", "Export"), vec![large("cloud.export-points"), large("cloud.export-mesh")]),
            ],
            // the project's models together; issues, checks, comments and sharing to come
            RibbonPage::Collaborate => vec![
                (l.text("Modellen", "Models"), vec![large("file.open"), smalls(&["file.open-folder", "file.add", "file.save-as"])]),
                (l.text("Issues", "Issues"), vec![large("collaborate.issue"), smalls(&["collaborate.bcf-import", "collaborate.bcf-export"])]),
                (l.text("Controle", "Check"), vec![large("collaborate.check"), smalls(&["collaborate.ids", "collaborate.report"])]),
                (l.text("Clashes", "Clashes"), vec![large("collaborate.clash")]),
                (l.text("Regelset", "Rule set"), vec![smalls(&["collaborate.rules-open", "collaborate.rules-save"])]),
            ],
        };
        groups
    }

    fn ribbon(&self, look: Look) -> Element<'_, Msg> {
        // without the groups switched off in the settings
        let groups: Vec<(&'static str, Vec<RibbonPart>)> = self.ribbon_groups().into_iter().filter(|g| !self.prefs.ribbon_hidden.iter().any(|k| k == group_key(g))).collect();
        // drawn to fit the window: where the ribbon is wider, its groups show their small
        // buttons as icons only, from the last group back; narrower still, it scrolls sideways
        let commands = self.commands();
        let command = |id: &str| commands.iter().find(|c| c.id == id).unwrap_or_else(|| panic!("command {id}"));
        let letters = |id: &str| command(id).label.trim_end_matches('…').chars().count() as f32;
        let part_width = |part: &RibbonPart, icons: bool| match part {
            RibbonPart::Large(id) => (letters(id) * 5.8).max(52.0),
            RibbonPart::Smalls(_) if icons => RIBBON_ICON + 4.0,
            RibbonPart::Smalls(ids) => ids.iter().map(|id| letters(id) * 6.4 + 21.0).fold(0.0, f32::max),
        };
        let group_width = |(label, parts): &(&'static str, Vec<RibbonPart>), icons: bool| {
            let buttons: f32 = parts.iter().map(|p| part_width(p, icons)).sum::<f32>() + 2.0 * parts.len().saturating_sub(1) as f32;
            buttons.max(label.chars().count() as f32 * 6.6) + 13.0
        };
        let mut icons = vec![false; groups.len()];
        let width = |icons: &[bool]| groups.iter().zip(icons).map(|(g, i)| group_width(g, *i)).sum::<f32>();
        for k in (0..groups.len()).rev() {
            if width(&icons) <= self.width {
                break;
            }
            icons[k] = groups[k].1.iter().any(|p| matches!(p, RibbonPart::Smalls(_)));
        }
        let fits = width(&icons) <= self.width;
        let groups: Vec<Element<'_, Msg>> = groups
            .iter()
            .zip(&icons)
            .map(|((label, parts), icons)| {
                let parts = parts
                    .iter()
                    .map(|part| match part {
                        RibbonPart::Large(id) => cm::ribbon_large(look, command(id)),
                        RibbonPart::Smalls(ids) if *icons => column(ids.iter().map(|id| cm::icon_button(look, command(id), RIBBON_ICON))).spacing(1).into(),
                        RibbonPart::Smalls(ids) => column(ids.iter().map(|id| cm::ribbon_small(look, command(id)))).spacing(2).into(),
                    })
                    .collect();
                w::ribbon_group(look, label, parts)
            })
            .collect();
        if self.compact() || !fits {
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
            Tool::Note => "measure.note",
            Tool::Arrow => "measure.arrow",
            Tool::Line => "measure.line",
            Tool::Cloud => "measure.cloud",
        }
    }

    /// Bestand: new, open, save as, settings, and the files opened last.
    fn backstage_view(&self, look: Look) -> Element<'_, Msg> {
        let l = self.l();
        let c = look.color;
        let writable = self.product() != Product::Viewer;
        let loaded = writable && self.app.model.is_some();
        #[allow(unused_mut)]
        let mut nav = column![
            t::file_nav_row(look, small("file.new"), l.text("Nieuw", "New"), false, writable.then_some(Msg::App(Message::NewModel(l.text("Niveau 0", "Level 0"))))),
            t::file_nav_row(look, small("file.open"), l.text("Openen", "Open"), false, Some(Msg::App(Message::OpenDialog))),
            t::file_nav_row(look, small("file.open"), l.text("Map openen", "Open folder"), false, Some(Msg::App(Message::OpenFolderDialog))),
            t::file_nav_row(look, small("file.import"), l.text("Toevoegen", "Add"), false, Some(Msg::App(Message::AddDialog))),
            t::file_nav_row(look, small("file.save-as"), l.text("Opslaan als", "Save as"), false, loaded.then_some(Msg::App(Message::SaveDialog))),
            t::file_nav_row(look, small("file.save-as"), l.text("Project opslaan", "Save project"), false, loaded.then_some(Msg::App(Message::SaveProjectDialog))),
            t::file_nav_row(look, small("file.save-as"), l.text("Exporteren als IFC 4x3", "Export IFC 4x3"), false, loaded.then_some(Msg::App(Message::SaveIfcDialog(crate::export::IfcSchema::Ifc4x3)))),
            t::file_nav_row(look, small("ui.settings"), l.text("Instellingen", "Settings"), false, Some(Msg::Dialog(Some(Dialog::Settings)))),
        ]
        .width(260);
        #[cfg(target_arch = "wasm32")]
        if crate::host::renders() {
            nav = nav.push(t::file_nav_row(look, small("mesh.surface"), l.text("Renderen met Blender", "Render with Blender"), false, crate::host::can_render().then_some(Msg::App(Message::BlenderRender))));
        }
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
        // the Planner is a web page in a frame over the whole workspace: no panels, no tabs. This
        // lies under the frame, and shows while the page is not there (a dialog, no web view)
        if self.product() == Product::Planner {
            let explanation = if self.planner_failed.is_some() {
                l.text(
                    "De webweergave van het systeem (WebView2 Runtime) ontbreekt of start niet. Open de planning in de browser, of kies Vernieuwen om het opnieuw te proberen.",
                    "The system's web view runtime (WebView2 Runtime) is missing or does not start. Open the planning in the browser, or choose Refresh to try again.",
                )
            } else {
                l.text(
                    "De webversie van Open Planner Studio opent hier in een webframe. Blijft dit leeg, dan ontbreekt de webweergave van het systeem of is er geen internet.",
                    "The web version of Open Planner Studio opens here in a web frame. If this stays empty, the system's web view is missing or there is no internet.",
                )
            };
            return container(w::empty_state(look, large("drawing.plan"), "Open Planner Studio", explanation, Some(cm::button(look, &self.command("planner.browser"), K::Secondary, D::Regular))))
                .width(Length::Fill)
                .height(Length::Fill)
                .style(move |_| container::Style { background: Some(look.color.workspace.into()), ..Default::default() })
                .into();
        }
        let p = palette(look);
        let app = &self.app;
        let library = self.product() == Product::Library;
        let workspace: Element<'_, Msg> = if library {
            // Library: its page is the workspace, with or without a model
            app.libraries_page(p, l).map(Msg::App)
        } else if app.model.is_none() && !app.docs.shown && !matches!(app.status, Status::Loading { .. }) {
            // each product starts its own way: open a model, make one, or open the project's
            let open = |kind| cm::button(look, &self.command("file.open"), kind, D::Regular);
            let (icon, title, explanation, action): (_, _, _, Element<'_, Msg>) = match self.product() {
                Product::Viewer | Product::Docs | Product::Gis | Product::Library | Product::Ifcx | Product::Planner => (
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
                Product::Analysis => (
                    "measure.perimeter",
                    l.text("Geen rekenmodel geopend", "No analysis model open"),
                    l.text("Open een constructief rekenmodel om het te bekijken met zijn belastingen en rekenresultaten.", "Open a structural analysis model to look at it with its loads and results."),
                    open(K::Primary),
                ),
                Product::Pointcloud => (
                    "mesh.surface",
                    l.text("Geen puntenwolk geopend", "No point cloud open"),
                    l.text("Open een puntenwolk (LAS, LAZ, E57, PLY, PTS, PTX, PCD, XYZ) om hem te bekijken, te meten, door te snijden en er een mesh van te maken.", "Open a point cloud (LAS, LAZ, E57, PLY, PTS, PTX, PCD, XYZ) to look at it, measure it, cut it and make a mesh of it."),
                    open(K::Primary),
                ),
                Product::Validator => (
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
        // Docs opens without a model too; in its own product it then needs no tab
        // (the Library page is no view of the model: no tabs of views over it)
        if (app.model.is_some() || (app.docs.open && !app.docs.in_product)) && doc_tabs.len() > 1 && !library {
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
                    w::panel(look, l.text("Eigenschappen", "Properties"), width, actions, self.right_body(look))
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
                .push(w::panel(look, l.text("Eigenschappen", "Properties"), self.prefs.right_width, actions, self.right_body(look)));
        }
        body.into()
    }

    /// The left panel of the product shown, with its title: the explorer, or in the Validator
    /// the project's models, issues and checks.
    fn left_panel(&self, look: Look) -> (&'static str, Element<'_, Msg>) {
        let l = self.l();
        match self.product() {
            Product::Validator => {
                #[cfg(all(feature = "validator", not(target_arch = "wasm32")))]
                let body = container(Self::pane(self.app.collab_body(look, l))).padding([4, 8]).width(Length::Fill).height(Length::Fill);
                #[cfg(not(all(feature = "validator", not(target_arch = "wasm32"))))]
                let body = {
                    let models: Vec<&str> = self.app.models.iter().map(|m| m.model.path.file_name().and_then(|n| n.to_str()).unwrap_or("3D")).collect();
                    container(crate::product::collaboration_panel(look, l, models)).padding([4, 8]).width(Length::Fill).height(Length::Fill)
                };
                (l.text("Samenwerken", "Collaboration"), body.into())
            }
            // Docs: the folders of the project folder
            Product::Docs => (l.text("Projectmap", "Project folder"), Self::pane(self.app.docs_tree_pane(palette(look), l))),
            // Library: the sources of library content, and the countries
            Product::Library => (l.text("Bibliotheken", "Libraries"), Self::pane(self.app.libraries_pane(palette(look), l))),
            Product::Viewer | Product::Modeller | Product::Pointcloud | Product::Analysis | Product::Gis | Product::Ifcx | Product::Planner => (l.text("Verkenner", "Explorer"), Self::pane(self.app.explorer_body(palette(look), l))),
        }
    }

    /// The viewer's content of a side panel, clear of the panel's edges.
    /// The right panel's body: the properties; in the Modeller the model explorer above them, as in
    /// the reference modeller.
    fn right_body(&self, look: Look) -> Element<'_, Msg> {
        let (app, l) = (&self.app, self.l());
        let p = palette(look);
        // the GIS and Geo tabs belong to GIC
        let geo = self.product() == Product::Gis;
        // Library: the editor of the picked build-up, or the fields of what is picked of the
        // drawing standard
        if self.product() == Product::Library {
            return Self::pane(app.library_side(p, l));
        }
        if self.product() != Product::Modeller || (app.active_doc_sheet.is_some() && app.sheet_pick.is_some()) {
            return Self::pane(app.properties_body_for(p, l, geo));
        }
        column![
            container(w::mono_label(look, l.text("Modelverkenner", "Model explorer"))).padding([4, 8]),
            container(app.model_explorer(p, l).map(Msg::App)).padding([0, 4]).width(Length::Fill).height(Length::FillPortion(2)),
            iced::widget::rule::horizontal(1),
            container(Self::pane(app.properties_body_for(p, l, geo))).height(Length::FillPortion(3)),
        ]
        .into()
    }

    fn pane(content: Element<'_, Message>) -> Element<'_, Msg> {
        container(content.map(Msg::App)).padding([4, 8]).width(Length::Fill).height(Length::Fill).into()
    }

    fn dialog_card(&self, look: Look, dialog: Dialog) -> Element<'_, Msg> {
        let l = self.l();
        let close_icon = small("window.close");
        match dialog {
            Dialog::Settings => {
                let tab = |page, label| w::button(look, label, if self.settings_page == page { K::Secondary } else { K::Quiet }, D::Compact, None, Some(Msg::SettingsPage(page)));
                let tabs = row![tab(SettingsPage::General, l.text("Algemeen", "General")), tab(SettingsPage::Ribbon, l.text("Lint", "Ribbon")), tab(SettingsPage::Shortcuts, l.text("Sneltoetsen", "Shortcuts")), tab(SettingsPage::Extensions, l.text("Extensies", "Extensions"))].spacing(4);
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
                let text_sizes = options(&TEXT_SIZES, l, self.prefs.text_size());
                let body: Element<'_, Msg> = match self.settings_page {
                    SettingsPage::General => column![
                        w::mono_label(look, l.text("Weergave", "Appearance")),
                        w::property_row(look, l.text("Thema", "Theme"), w::select(look, themes.0, themes.1, |n| Msg::Theme(n.value), D::Regular)),
                        w::property_row(look, l.text("Werkruimte", "Workspace"), w::select(look, workspaces.0, workspaces.1, |n| Msg::Workspace(n.value), D::Regular)),
                        w::property_row(look, l.text("Statusbalk", "Status bar"), w::select(look, statusbars.0, statusbars.1, |n| Msg::Statusbar(n.value), D::Regular)),
                        w::property_row(look, l.text("Tekstgrootte", "Text size"), w::select(look, text_sizes.0, text_sizes.1, |n| Msg::TextSize(n.value), D::Regular)),
                        w::property_row(look, l.text("Interfacegrootte", "Interface size"), w::select(look, &Scale::ALL, scale, |s: Scale| Msg::Scale(s.0), D::Regular)),
                        w::mono_label(look, l.text("3D-weergave", "3D view")),
                        w::checkbox(look, l.text("Omgevingsschaduw tonen", "Show ambient shadows"), self.app.ambient, Some(|_| Msg::App(Message::ToggleAmbient))),
                        w::checkbox(look, l.text("Stramienbollen achter het model verbergen", "Hide grid heads behind the model"), self.app.grid_heads_hidden, Some(|_| Msg::App(Message::ToggleGridHeadsHidden))),
                        w::checkbox(look, l.text("Maten tussen de stramienen tonen", "Show the dimensions between grid lines"), self.app.grid_dims, Some(|_| Msg::App(Message::ToggleGridDims))),
                        w::checkbox(look, l.text("Peilen met peilmaten tonen", "Show the levels with their marks"), self.app.levels_3d, Some(|_| Msg::App(Message::ToggleLevels3d))),
                        w::mono_label(look, l.text("Taal", "Language")),
                        w::property_row(look, l.text("Taal", "Language"), w::select(look, &Language::ALL, Some(l), Msg::Language, D::Regular)),
                    ]
                    .spacing(6)
                    .into(),
                    // the groups of the ribbon tab shown: each on or off
                    SettingsPage::Ribbon => {
                        let hidden = &self.prefs.ribbon_hidden;
                        let groups = self.ribbon_groups();
                        let mut list = column![w::secondary(look, l.text("De groepen van het linttabblad dat nu open staat. Zet uit wat je niet gebruikt.", "The groups of the ribbon tab that is open now. Switch off what you do not use."))].spacing(8);
                        for group in &groups {
                            let key = group_key(group);
                            let shown = !hidden.iter().any(|k| k == key);
                            list = list.push(w::checkbox(look, group.0, shown, Some(move |on| Msg::RibbonGroup(key, on))));
                        }
                        list.push(w::button(look, l.text("Alle groepen tonen", "Show all groups"), K::Secondary, D::Compact, None, (!hidden.is_empty()).then_some(Msg::RibbonReset))).into()
                    }
                    // the extension manager: every integrated tool, on the switch or not
                    SettingsPage::Extensions => {
                        let mut list = column![w::secondary(look, l.text("Elk onderdeel van de suite is een extensie. Zet uit wat je niet gebruikt: het verdwijnt van de schakelaar bovenaan het venster en komt terug als je het weer aanzet.", "Every part of the suite is an extension. Switch off what you do not use: it leaves the switch at the top of the window and comes back when you switch it on again."))].spacing(8);
                        for product in self.extensions() {
                            // the Viewer is the suite itself
                            let toggle = (product != Product::Viewer && !matches!(self.edition, Edition::Only(_))).then_some(move |on| Msg::Extension(product, on));
                            let mut entry = column![w::checkbox(look, product.full_name(), self.extension_on(product), toggle), w::secondary(look, product.purpose(l))].spacing(2);
                            // nested under it: the tools that run on it, the groups of its own ribbon tabs
                            // (the View tab is the Viewer's and is listed there)
                            let mut tools = column![].spacing(2);
                            for page in product.pages().iter().filter(|page| **page != RibbonPage::View || product == Product::Viewer) {
                                for group in self.ribbon_groups_of(*page) {
                                    let key = group_key(&group);
                                    let shown = !self.prefs.ribbon_hidden.iter().any(|k| k == key);
                                    tools = tools.push(w::checkbox(look, group.0, shown, self.extension_on(product).then_some(move |on| Msg::RibbonGroup(key, on))));
                                }
                            }
                            entry = entry.push(container(tools).padding(iced::Padding { top: 2.0, right: 0.0, bottom: 4.0, left: 28.0 }));
                            list = list.push(entry);
                        }
                        iced::widget::scrollable(container(list.push(w::secondary(look, l.text("Onder elke extensie staan de tools die erop draaien. Een tool die uit staat, verdwijnt van het lint.", "Under each extension are the tools that run on it. A tool that is switched off leaves the ribbon."))).padding(iced::Padding { top: 0.0, right: 22.0, bottom: 0.0, left: 0.0 }))).height(iced::Length::Fixed(460.0)).into()
                    }
                    // in the Modeller its own keys follow the commands' shortcuts
                    SettingsPage::Shortcuts => column![sc::page(look, &self.shortcuts, &self.commands(), &self.keymap, &groups, Msg::Shortcuts)]
                        .push((self.product() == Product::Modeller).then(|| crate::app::modeller::key_actions::key_list(look, l)))
                        .spacing(14)
                        .into(),
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
            Dialog::Feedback => {
                let form = &self.feedback;
                let send = |m: FeedbackMsg| Msg::Feedback(m);
                if form.state == FeedbackState::Sent {
                    return w::dialog(
                        look,
                        l.text("Feedback verzenden", "Send feedback"),
                        close_icon,
                        Some(Msg::Dialog(None)),
                        column![w::heading(look, l.text("Bedankt!", "Thank you!"), 16.0), w::body(look, l.text("Je feedback is verzonden.", "Your feedback was sent."))].spacing(6).into(),
                        None,
                        vec![
                            w::button(look, l.text("Nog een verzenden", "Send another"), K::Quiet, D::Regular, None, Some(send(FeedbackMsg::Again))),
                            w::button(look, l.text("Sluiten", "Close"), K::Primary, D::Regular, None, Some(Msg::Dialog(None))),
                        ],
                        false,
                    );
                }
                let sending = form.state == FeedbackState::Sending;
                let wrong_email = (!form.email.trim().is_empty() && !form.email_ok()).then(|| l.text("Dit is geen e-mailadres.", "This is not an e-mail address."));
                let kinds = row(FEEDBACK_KINDS.iter().enumerate().map(|(k, kind)| w::radio(look, l.text(kind.1, kind.2), k, Some(form.kind), |k| send(FeedbackMsg::Kind(k))))).spacing(16);
                let moods = row(FEEDBACK_MOODS.iter().enumerate().map(|(m, mood)| {
                    w::button(look, l.text(mood.1, mood.2), if form.mood == Some(m) { K::Secondary } else { K::Quiet }, D::Compact, None, Some(send(FeedbackMsg::Mood(m))))
                }))
                .spacing(4);
                let c = look.color;
                let message = iced::widget::text_editor(&form.message)
                    .placeholder(l.text("Vertel wat je ervan vindt (minstens 10 tekens)", "Tell us what you think (at least 10 characters)"))
                    .on_action(|a| Msg::Feedback(FeedbackMsg::Message(a)))
                    .height(140)
                    .size(13)
                    .font(fonts::BODY)
                    .style(move |_, status| iced::widget::text_editor::Style {
                        background: c.raised.into(),
                        border: iced::Border { color: if matches!(status, iced::widget::text_editor::Status::Focused { .. }) { c.accent } else { c.border }, width: 1.0, radius: 4.0.into() },
                        placeholder: c.text_disabled,
                        value: c.text,
                        selection: iced::Color { a: 0.35, ..c.accent },
                    });
                let count = form.wording().chars().count();
                let mut body = column![
                    w::property_row(look, l.text("E-mail", "E-mail"), w::text_field(look, "naam@voorbeeld.nl", &form.email, Some(|e| Msg::Feedback(FeedbackMsg::Email(e))), D::Regular, wrong_email)),
                    w::property_row(look, l.text("Naam", "Name"), w::text_field(look, l.text("Je naam (hoeft niet)", "Your name (optional)"), &form.name, Some(|n| Msg::Feedback(FeedbackMsg::Name(n))), D::Regular, None)),
                    w::property_row(look, l.text("Soort", "Kind"), kinds.into()),
                    message,
                    w::secondary(look, format!("{count} / {FEEDBACK_LONGEST}")),
                    w::property_row(look, l.text("Hoe bevalt het?", "How is it going?"), moods.into()),
                    w::secondary(look, l.text(
                        "Verzonden worden: wat je hier invult en de versie van het programma. Niets van je model.",
                        "Sent are: what you fill in here and the version of the app. Nothing of your model.",
                    )),
                ]
                .spacing(10);
                if let FeedbackState::Failed(why) = &form.state {
                    body = body.push(w::body(look, format!("{} ({why})", l.text("Verzenden is niet gelukt.", "Sending failed."))).color(c.error));
                }
                w::dialog(
                    look,
                    l.text("Feedback verzenden", "Send feedback"),
                    close_icon,
                    Some(Msg::Dialog(None)),
                    body.into(),
                    None,
                    vec![
                        w::button(look, l.text("Annuleren", "Cancel"), K::Quiet, D::Regular, None, Some(Msg::Dialog(None))),
                        w::button(look, if sending { l.text("Bezig met verzenden…", "Sending…") } else { l.text("Verzenden", "Send") }, K::Primary, D::Regular, None, form.ready().then(|| send(FeedbackMsg::Send))),
                    ],
                    true,
                )
            }
            Dialog::Cloud => {
                #[cfg(all(feature = "pointcloud", not(target_arch = "wasm32")))]
                let body: Element<'_, Msg> = self.app.cloud_card(look, l).map(Msg::App);
                #[cfg(not(all(feature = "pointcloud", not(target_arch = "wasm32"))))]
                let body: Element<'_, Msg> = w::body(look, desktop_only_text(l, "LAS")).into();
                w::dialog(
                    look,
                    l.text("Puntenwolk", "Point cloud"),
                    close_icon,
                    Some(Msg::Dialog(None)),
                    body,
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
/// The page the Planner shows in its web frame.
pub(crate) const PLANNER_URL: &str = "https://open-planner-studio.open-aec.com/";
/// The room the Planner's web frame makes for a tip of the app that hangs into the workspace:
/// a line of text, its padding and the gap to its control.
const PLANNER_TIP: f32 = 32.0;
/// The same under the open ribbon: how far the tip of one of its large buttons hangs below it.
const PLANNER_RIBBON_TIP: f32 = 16.0;

/// The program that opens an address in the system's browser.
#[cfg(not(target_arch = "wasm32"))]
fn url_opener() -> &'static str {
    match std::env::consts::OS {
        "windows" => "explorer",
        "macos" => "open",
        _ => "xdg-open",
    }
}

fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_arch = "wasm32")]
    return crate::web::open_url(url);
    #[cfg(not(target_arch = "wasm32"))]
    std::process::Command::new(url_opener()).arg(url).spawn().map(|_| ()).map_err(|e| format!("Open {url}: {e}"))
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
    let builder = iced::application(move || (initial.lock().unwrap_or_else(|e| e.into_inner()).take().expect("the application boots once"), Task::none()), Studio::update_with_frame, Studio::view)
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
        studio.set_product(Product::Modeller);
        studio.prefs.language = Language::English;
        let _ = studio.update(Msg::App(Message::Loaded(Ok(triangle("a.rvt")))));
        let _ = studio.update(Msg::App(Message::NewSheet("Sheet")));
        assert_eq!(studio.app.doc_tabs(), [DocTab::Model, DocTab::DocSheet(0)]);
        assert_eq!(studio.app.active_doc_tab(), 1);
        let names = accessibility(&studio).summary();
        assert!(names.iter().any(|n| n.contains("A01 Sheet")), "{names:?}");
    }

    #[test]
    fn global_3d_button_returns_to_the_current_project_from_every_product() {
        let mut studio = Studio::new(App::default());
        studio.set_product(Product::Modeller);
        let _ = studio.update(Msg::App(Message::Loaded(Ok(triangle("project.ifc")))));
        let _ = studio.update(Msg::App(Message::NewSheet("Sheet")));
        for product in studio.products.clone() {
            let _ = studio.update(Msg::Product(product));
            let _ = studio.update(Msg::SelectTab(1));
            studio.backstage = true;
            studio.compact_panel = Some(Side::Left);
            let _ = studio.update(Msg::ShowModel3d);
            assert_eq!(studio.app.active_doc_tab(), 0, "{product:?}");
            assert!(studio.app.doc_tabs().contains(&DocTab::DocSheet(0)));
            assert!(!studio.backstage && studio.compact_panel.is_none());
            assert_eq!(studio.product(), Product::Viewer);
            let tabs = studio.app.doc_tabs();
            let _ = studio.update(Msg::App(Message::NewSheet("Forbidden")));
            assert_eq!(studio.app.doc_tabs(), tabs);
            assert!(!studio.command("model.edit").enabled);
        }
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
        assert!(studio.app.model.is_none(), "Viewer cannot create models");
        studio.set_product(Product::Modeller);
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
        // the Project tab, and the Draw tab in a floor plan
        let _ = studio.update(Msg::App(Message::ModelTool(None)));
        studio.ribbon_tab = 2;
        studio.snapshot(iced::Size::new(1440.0, 900.0), &dir.join("modeller-project-nl.png")).expect("snapshot");
        let level: std::sync::Arc<str> = studio.app.document.levels()[0].path.as_str().into();
        let _ = studio.update(Msg::App(Message::NewPlan(level)));
        studio.ribbon_tab = 1;
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

    /// A floor plan of the demo building in its tab, with the fields of its kind of drawing open
    /// in the View section of the right pane: target/shell-renders/view-kind.png.
    #[test]
    #[ignore = "writes PNG files"]
    fn export_view_kind_render() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/shell-renders");
        std::fs::create_dir_all(&dir).expect("folder");
        let mut app = App::default();
        app.add_demo_building();
        let mut studio = Studio::new(app);
        studio.prefs.language = Language::Dutch;
        studio.set_product(Product::Viewer);
        let level = std::sync::Arc::<str>::from(studio.app.document.levels()[0].path.as_str());
        for message in [Message::NewPlan(level), Message::ViewKindEdit(true)] {
            let _ = studio.update(Msg::App(message));
        }
        studio.snapshot(iced::Size::new(1440.0, 900.0), &dir.join("view-kind.png")).expect("snapshot");
    }

    /// The Library product with each of its sources: target/shell-renders/library-product-*.png.
    #[test]
    #[ignore = "writes PNG files"]
    fn export_library_product_renders() {
        use crate::app::libraries::{Country, Source};
        use crate::app::LibrariesMessage;
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/shell-renders");
        std::fs::create_dir_all(&dir).expect("folder");
        let shots = [
            ("general", LibrariesMessage::Source(Source::General)),
            ("country-nl", LibrariesMessage::Country(Country::Netherlands)),
            ("country-de", LibrariesMessage::Country(Country::Germany)),
            ("company", LibrariesMessage::Source(Source::Company)),
            ("project", LibrariesMessage::Source(Source::Project)),
            ("search", LibrariesMessage::Search("beton 2".into())),
            ("buildup", LibrariesMessage::Source(Source::Project)),
            ("profile", LibrariesMessage::Source(Source::Project)),
            ("material", LibrariesMessage::Source(Source::Project)),
            ("system", LibrariesMessage::Source(Source::Project)),
            ("system-layer", LibrariesMessage::Source(Source::Project)),
            ("follow", LibrariesMessage::Source(Source::Project)),
            ("search-steel", LibrariesMessage::Search("hea 2".into())),
            ("hatches-nen47", LibrariesMessage::Country(Country::Netherlands)),
            ("hatches-openaec", LibrariesMessage::Source(Source::Project)),
            ("ifcx", LibrariesMessage::Source(Source::Project)),
            ("ifcx-hatch", LibrariesMessage::Source(Source::General)),
            ("search-hatch", LibrariesMessage::Search("isolatie".into())),
            ("3d-system", LibrariesMessage::Source(Source::Project)),
            ("3d-system-apart", LibrariesMessage::Source(Source::Project)),
            ("3d-wall", LibrariesMessage::Source(Source::Project)),
            ("3d-steel", LibrariesMessage::Source(Source::General)),
        ];
        for (name, show) in shots {
            let mut studio = Studio::new(App::default());
            studio.prefs.language = Language::Dutch;
            studio.set_product(Product::Library);
            let _ = studio.update(Msg::App(Message::Libraries(show)));
            // the Netherlands with a kind of drawing picked, so the right pane shows its fields
            // the project with its first wall picked, so the right pane is its editor
            if name == "buildup" {
                let id = studio.app.library.buildups[0].id.clone();
                let _ = studio.update(Msg::App(Message::BuildUp(crate::app::buildups::BuildUpMessage::Pick(id))));
            }
            // the project with a column of the steel table picked, in its profile editor, and
            // the series it is of shown under the types
            if name == "profile" {
                use crate::app::profiles::ProfileMessage;
                let _ = studio.update(Msg::App(Message::Libraries(LibrariesMessage::Category(crate::app::libraries::Category::Steel))));
                for message in [ProfileMessage::Add, ProfileMessage::Steel("HEB 300".into()), ProfileMessage::Series(Some("HEB"))] {
                    let _ = studio.update(Msg::App(Message::Profile(message)));
                }
            }
            // the project with a material picked, in its editor
            if name == "material" {
                let _ = studio.update(Msg::App(Message::Material(crate::app::materials::MaterialMessage::Pick("brick".into()))));
            }
            // the project with a timber frame picked, in its editor: the face open, or a layer
            if name.starts_with("system") {
                use crate::app::systems::{Part, SystemMessage};
                let _ = studio.update(Msg::App(Message::SystemType(SystemMessage::Pick("system.timber-frame-140".into()))));
                if name == "system-layer" {
                    let _ = studio.update(Msg::App(Message::SystemType(SystemMessage::Part(Part::Layer(3)))));
                }
            }
            // the project with a model: a wall of the demo building given the first build-up,
            // and the build-up then made thicker without its wall (as in a file whose elements
            // are older than their type), so the pane says the wall is not as its type is now
            if name == "follow" {
                use crate::app::buildups::BuildUpMessage;
                studio.app.add_demo_building();
                let wall = studio.app.modeller.model.iter().find(|e| e.class() == "IfcWall").map(|e| e.id).expect("a wall of the demo");
                let id = studio.app.library.buildups[0].id.clone();
                studio.app.select_modelled(wall);
                let _ = studio.update(Msg::App(Message::Library(crate::app::library::LibraryMessage::Assign(id.clone()))));
                crate::app::buildups::change(&mut studio.app.library, &id, |b| b.layers[0].thickness = 0.12).expect("a thicker layer");
                let _ = studio.update(Msg::App(Message::BuildUp(BuildUpMessage::Pick(id))));
            }
            // the hatches: the signs of NEN 47 with one looked at, the suite's own with a species
            // of wood looked at
            if name.starts_with("hatches") {
                use crate::app::libraries::{Category, Item};
                let (set, key) = if name == "hatches-nen47" { (bim_library::HatchSet::Nen47, "nen47-25") } else { (bim_library::HatchSet::OpenAec, "openaec-12-larch") };
                let _ = studio.update(Msg::App(Message::Libraries(LibrariesMessage::Category(Category::HatchSet(set)))));
                let _ = studio.update(Msg::App(Message::Libraries(LibrariesMessage::Look(Item::Hatch(key.into())))));
            }
            // the IFCX tab: the project's materials with one picked, and the general library's
            // hatches with one looked at
            if name == "ifcx" {
                let _ = studio.update(Msg::App(Message::Material(crate::app::materials::MaterialMessage::Pick("brick".into()))));
                let _ = studio.update(Msg::App(Message::Libraries(LibrariesMessage::Page(crate::app::libraries::Page::Code))));
            }
            if name == "ifcx-hatch" {
                use crate::app::libraries::{Category, Item};
                let _ = studio.update(Msg::App(Message::Libraries(LibrariesMessage::Category(Category::HatchSet(bim_library::HatchSet::OpenAec)))));
                let _ = studio.update(Msg::App(Message::Libraries(LibrariesMessage::Look(Item::Hatch("openaec-17-pir".into())))));
                let _ = studio.update(Msg::App(Message::Libraries(LibrariesMessage::Page(crate::app::libraries::Page::Code))));
            }
            // the 3D tab: a timber frame of the project as it lies and pulled apart, a wall of the
            // project pulled apart, and a profile of the steel table
            if name.starts_with("3d") {
                use crate::app::libraries::{Item, Page};
                match name {
                    "3d-wall" => {
                        let id = studio.app.library.buildups.iter().find(|b| b.layers.len() >= 4).map(|b| b.id.clone()).expect("a layered wall");
                        let _ = studio.update(Msg::App(Message::BuildUp(crate::app::buildups::BuildUpMessage::Pick(id))));
                    }
                    "3d-steel" => {
                        let _ = studio.update(Msg::App(Message::Libraries(LibrariesMessage::Look(Item::Steel("HEB 300".into())))));
                    }
                    _ => {
                        let _ = studio.update(Msg::App(Message::SystemType(crate::app::systems::SystemMessage::Pick("system.timber-frame-140".into()))));
                    }
                }
                let _ = studio.update(Msg::App(Message::Libraries(LibrariesMessage::Page(Page::Solid))));
                if name != "3d-system" {
                    let _ = studio.update(Msg::App(Message::Libraries(LibrariesMessage::Apart(true))));
                }
            }
            if name == "country-nl" {
                let _ = studio.update(Msg::App(Message::Standard(crate::app::standards::StandardMessage::Pick(crate::app::standards::Setting::Kind("floor-plan".into())))));
            }
            studio.snapshot(iced::Size::new(1440.0, 900.0), &dir.join(format!("library-product-{name}.png"))).expect("snapshot");
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
    fn the_modellers_right_pane_shows_the_explorer_above_the_properties() {
        let mut studio = Studio::new(App::default());
        studio.prefs.language = Language::English;
        let _ = studio.update(Msg::App(Message::NewModel("Level 0")));
        assert!(iced_test::simulator(studio.view()).find("MODEL EXPLORER").is_err(), "not in the Viewer");
        studio.set_product(Product::Modeller);
        let mut ui = iced_test::simulator(studio.view());
        for label in ["MODEL EXPLORER", "Storeys", "Grid lines", "Properties"] {
            assert!(ui.find(label).is_ok(), "{label}");
        }
    }

    #[test]
    fn the_gis_and_geo_tabs_of_the_right_pane_are_only_in_gic() {
        let mut studio = Studio::new(App::default());
        studio.prefs.language = Language::English;
        let _ = studio.update(Msg::App(Message::NewModel("Level 0")));
        for (product, there) in [(Product::Validator, false), (Product::Viewer, false), (Product::Gis, true)] {
            studio.set_product(product);
            let mut ui = iced_test::simulator(studio.view());
            assert_eq!(ui.find("Geo").is_ok(), there, "{product:?}");
        }
        // GIC opened its tab; elsewhere the pane shows the properties instead
        assert_eq!(studio.app.side_tab, crate::app::SideTab::Gis);
        studio.set_product(Product::Validator);
        assert!(iced_test::simulator(studio.view()).find("Properties").is_ok());
    }

    #[test]
    fn the_shortcuts_page_of_the_modeller_lists_its_keys() {
        let mut studio = Studio::new(App::default());
        studio.dialog = Some(Dialog::Settings);
        studio.settings_page = SettingsPage::Shortcuts;
        studio.prefs.language = Language::English;
        assert!(iced_test::simulator(studio.view()).find("MV").is_err(), "not in the Viewer");
        studio.set_product(Product::Modeller);
        // the dialog scrolls: its first rows show, the list itself has them all
        let mut ui = iced_test::simulator(studio.view());
        for label in ["MODELLER", "MV", "Move", "Select"] {
            assert!(ui.find(label).is_ok(), "{label}");
        }
        let mut list = iced_test::simulator(crate::app::modeller::key_actions::key_list::<Msg>(studio.look(), Language::English));
        for label in ["S", "W", "CL", "FL", "PgUp", "PgDn", "Level up"] {
            assert!(list.find(label).is_ok(), "{label}");
        }
    }

    #[test]
    fn an_extension_switched_off_leaves_the_switch_and_comes_back() {
        let mut studio = Studio::new(App::default());
        assert_eq!(studio.products, studio.extensions());
        let _ = studio.update(Msg::Product(Product::Gis));
        assert_eq!(studio.product(), Product::Gis);
        // the product shown is switched off: the first that is on is shown
        let _ = studio.update(Msg::Extension(Product::Gis, false));
        assert!(!studio.products.contains(&Product::Gis) && !studio.extension_on(Product::Gis));
        assert_eq!(studio.product(), Product::Viewer);
        let tabs: Vec<String> = accessibility(&studio).summary().into_iter().filter(|n| n.starts_with("Tab: ")).collect();
        assert!(!tabs.contains(&"Tab: GIC".to_string()) && tabs.contains(&"Tab: Docs".to_string()), "{tabs:?}");
        // the manager still lists it, and switching it on puts it back where it stood
        assert!(studio.extensions().contains(&Product::Gis));
        let _ = studio.update(Msg::Extension(Product::Gis, true));
        assert_eq!(studio.products, studio.extensions());
        // the Viewer is the suite itself: it cannot be switched off
        let _ = studio.update(Msg::Extension(Product::Viewer, false));
        assert!(studio.products.contains(&Product::Viewer) && studio.extension_on(Product::Viewer));
        // the page lists every extension by its name
        studio.dialog = Some(Dialog::Settings);
        studio.settings_page = SettingsPage::Extensions;
        let mut ui = iced_test::simulator(studio.view());
        for product in studio.extensions() {
            assert!(ui.find(product.full_name()).is_ok(), "{product:?}");
        }
    }

    #[test]
    fn the_tools_of_an_extension_are_listed_under_it_and_switch_off() {
        let mut studio = Studio::new(App::default());
        let _ = studio.update(Msg::Product(Product::Gis));
        let groups = studio.ribbon_groups_of(RibbonPage::Gis);
        assert!(groups.len() >= 3, "the GIS tab has its file, map, surroundings and terrain");
        let (name, key) = (groups[2].0, group_key(&groups[2]));
        // the page lists the tools of every extension under it
        studio.dialog = Some(Dialog::Settings);
        studio.settings_page = SettingsPage::Extensions;
        assert!(iced_test::simulator(studio.view()).find(name).is_ok(), "{name}");
        // a tool switched off there leaves the ribbon, and comes back
        studio.dialog = None;
        let shown = |studio: &Studio| !studio.prefs.ribbon_hidden.iter().any(|hidden| hidden == key);
        assert!(shown(&studio));
        let _ = studio.update(Msg::RibbonGroup(key, false));
        assert!(!shown(&studio));
        let _ = studio.update(Msg::RibbonGroup(key, true));
        assert!(shown(&studio));
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
        // the Draw tab, second after Model: selecting, the six drawing tools, what is written and
        // measured on the drawing, and changing what is selected; each a command that is there
        let _ = studio.update(Msg::RibbonTab(1));
        assert_eq!(studio.ribbon_page(), RibbonPage::Draw);
        let groups = studio.ribbon_groups();
        assert_eq!(groups.iter().map(|g| g.0).collect::<Vec<_>>(), ["Selection", "Draw", "Annotate", "Modify", "Edit"]);
        let ids: Vec<&str> = groups
            .iter()
            .flat_map(|g| &g.1)
            .flat_map(|part| match part {
                RibbonPart::Large(id) => vec![*id],
                RibbonPart::Smalls(ids) => ids.clone(),
            })
            .collect();
        for kind in crate::app::DrawKind::ALL {
            assert!(ids.contains(&kind.command()) && studio.command(kind.command()).enabled, "{kind:?}");
        }
        for id in &ids {
            assert_eq!(studio.command(id).id, *id);
        }
        let _ = studio.update(studio.command("draw.arc").message);
        assert_eq!(studio.app.draw_tool, Some(crate::app::DrawKind::Arc));
        assert!(studio.command("draw.arc").kind == openaec_ui::design::command::CommandKind::Toggle(true));
        let _ = studio.update(studio.command("annotate.dimension").message);
        assert!(studio.app.dimension_tool && studio.command("annotate.dimension").kind == openaec_ui::design::command::CommandKind::Toggle(true));
        // the door and window tools are on the Model tab
        assert!(studio.command("model.door").enabled && studio.command("model.window").enabled);
    }

    /// A project folder for a test: `drawings/A-101.pdf` and `notes.docx`.
    fn project_folder(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("o3v-shell-docs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("drawings")).expect("folder");
        std::fs::write(root.join("drawings/A-101.pdf"), b"plan").expect("file");
        std::fs::write(root.join("notes.docx"), b"notes").expect("file");
        root
    }

    #[test]
    fn the_planner_lays_its_web_frame_over_the_workspace_and_hides_it_elsewhere() {
        use openaec_ui::design::shell::{RIBBON, RIBBON_TABS, STATUSBAR, TITLEBAR};
        let mut studio = Studio::new(App::default());
        // a web frame in the window: on Windows, for now
        assert_eq!(studio.products.contains(&Product::Planner), cfg!(windows));
        if !Product::Planner.built() {
            return;
        }
        let _ = studio.update(Msg::Product(Product::Planner));
        assert_eq!((studio.product(), studio.ribbon_page()), (Product::Planner, RibbonPage::Planner));
        // no window yet: no frame; under it the page names the program and opens it outside
        assert_eq!(studio.planner_area(), None);
        {
            let mut ui = iced_test::simulator(studio.view());
            assert!(ui.find("Open Planner Studio").is_ok() && ui.find("Open in browser").is_ok());
        }
        // the frame is the workspace: the window's width, from under the ribbon to the status bar
        studio.window_open = true;
        let _ = studio.update(Msg::Sized(iced::Size::new(1400.0, 900.0)));
        // (but for the window's left and right edge, where a press resizes the window)
        let (top, scale, edge) = (TITLEBAR + RIBBON_TABS + RIBBON, studio.prefs.scale() as f32, crate::resize_frame::EDGE);
        assert_eq!(studio.planner_area(), Some([edge * scale, top * scale, (1400.0 - 2.0 * edge) * scale, (900.0 - top - STATUSBAR) * scale]));
        // a maximized window has no edges to drag
        studio.maximized = true;
        assert_eq!(studio.planner_area(), Some([0.0, top * scale, 1400.0 * scale, (900.0 - top - STATUSBAR) * scale]));
        studio.maximized = false;
        // it follows the window
        let _ = studio.update(Msg::Sized(iced::Size::new(1000.0, 700.0)));
        assert_eq!(studio.planner_area().map(|[_, _, w, h]| (w, h)), Some(((1000.0 - 2.0 * edge) * scale, (700.0 - top - STATUSBAR) * scale)));
        // and the app's interface size: the frame is laid in the window's own units
        studio.prefs.scale = 1.5;
        let _ = studio.update(Msg::Sized(iced::Size::new(800.0, 500.0)));
        assert_eq!(studio.planner_area(), Some([edge * 1.5, top * 1.5, (800.0 - 2.0 * edge) * 1.5, (500.0 - top - STATUSBAR) * 1.5]));
        studio.prefs.scale = scale as f64;
        let _ = studio.update(Msg::Sized(iced::Size::new(1000.0, 700.0)));
        // the pointer on the status bar: a tip there opens above it, and the frame makes room
        let full = studio.planner_area().expect("frame");
        let _ = studio.update(Msg::PlannerPointer(Some(iced::Point::new(900.0, 695.0))));
        assert_eq!(studio.planner_area().map(|[_, y, _, h]| (y, h)), Some((full[1], full[3] - PLANNER_TIP * scale)));
        // on the ribbon: the tip of a large button hangs a little below it
        let _ = studio.update(Msg::PlannerPointer(Some(iced::Point::new(60.0, 100.0))));
        assert_eq!(studio.planner_area().map(|[_, y, _, h]| (y, h)), Some((full[1] + PLANNER_RIBBON_TIP * scale, full[3] - PLANNER_RIBBON_TIP * scale)));
        // on the title bar its tips end above the workspace, and over the page the app sees no pointer
        let _ = studio.update(Msg::PlannerPointer(Some(iced::Point::new(60.0, 10.0))));
        assert_eq!(studio.planner_area(), Some(full));
        let _ = studio.update(Msg::PlannerPointer(Some(iced::Point::new(900.0, 695.0))));
        let _ = studio.update(Msg::PlannerPointer(None));
        assert_eq!(studio.planner_area(), Some(full));
        // no web view on this computer: no frame, the page says so and still opens the browser
        let _ = studio.update(Msg::PlannerFrame(Err("no runtime".into())));
        assert_eq!(studio.planner_area(), None);
        assert!(matches!(&studio.app.status, Status::Error(e) if e.contains("no runtime")));
        {
            let mut ui = iced_test::simulator(studio.view());
            assert!(ui.find("The system's web view runtime (WebView2 Runtime) is missing or does not start. Open the planning in the browser, or choose Refresh to try again.").is_ok());
            assert!(ui.find("Open in browser").is_ok());
        }
        assert_eq!(url_opener(), if cfg!(windows) { "explorer" } else if cfg!(target_os = "macos") { "open" } else { "xdg-open" });
        // Refresh tries again
        let _ = studio.update(Msg::PlannerReload);
        assert_eq!(studio.planner_area(), Some(full));
        // a dialog and File are drawn by the app, which a frame would cover: it hides for them
        studio.dialog = Some(Dialog::Settings);
        assert_eq!(studio.planner_area(), None);
        studio.dialog = None;
        studio.backstage = true;
        assert_eq!(studio.planner_area(), None);
        studio.backstage = false;
        assert!(studio.planner_area().is_some());
        // and for every other product
        let _ = studio.update(Msg::Product(Product::Viewer));
        assert_eq!(studio.planner_area(), None);
    }

    #[test]
    fn docs_is_a_product_with_its_ribbon_its_page_and_the_folders_in_the_left_pane() {
        let root = project_folder("product");
        let mut studio = Studio::new(App::default());
        let _ = studio.update(Msg::Product(Product::Docs));
        assert_eq!((studio.product(), studio.ribbon_page()), (Product::Docs, RibbonPage::Docs));
        assert!(studio.app.docs.open && studio.app.docs.shown, "its page shows");
        // it is on the switch once: a product there is, is no product to come; it stands where it
        // was announced, after the Modeller (GIS and the Planner came after it)
        assert!(Product::ALL.iter().all(|p| !SOON.contains(&p.label())), "{SOON:?}");
        assert_eq!(Product::ALL[Product::Modeller.index() + 1], Product::Docs);
        assert_eq!(Product::ALL.last(), Some(&Product::Planner));
        // no project folder yet: the page asks for one
        assert!(iced_test::simulator(studio.view()).find("Choose project folder…").is_ok());
        // with one: its folders are the left pane, its files the page
        studio.app.docs_at(root.clone(), None);
        let _ = studio.update(Msg::Resized(1600.0));
        assert_eq!(studio.left_panel(studio.look()).0, "Project folder");
        {
            let mut ui = iced_test::simulator(studio.view());
            assert!(ui.find("drawings").is_ok() && ui.find("notes.docx").is_ok());
        }
        // the ribbon's commands follow what is picked: a text opens in its program, not in the suite
        assert!(studio.command("docs.refresh").enabled);
        assert!(!studio.command("docs.open-outside").enabled && !studio.command("docs.link").enabled);
        let _ = studio.update(Msg::App(Message::Docs(crate::app::DocsMessage::Pick("notes.docx".into()))));
        assert!(studio.command("docs.open-outside").enabled && studio.command("docs.reveal").enabled);
        assert!(!studio.command("docs.open").enabled);
        // in another product Docs is closed: it is no tab of theirs, and no command of their ribbon
        let _ = studio.update(Msg::Product(Product::Viewer));
        assert!(!studio.app.docs.open && !studio.app.docs.shown);
        assert!(studio.default_commands().iter().all(|c| c.id != "view.docs"));
        // back in Docs the folder is still there
        let _ = studio.update(Msg::Product(Product::Docs));
        assert!(studio.app.docs.shown && studio.app.docs.folder.as_deref() == Some(root.as_path()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_document_of_an_element_opens_in_the_docs_product() {
        let root = project_folder("show");
        let mut studio = Studio::new(App::default());
        studio.app.docs.folder = Some(root.clone());
        assert_eq!(studio.product(), Product::Viewer);
        let _ = studio.update(Msg::App(Message::Docs(crate::app::DocsMessage::Show("drawings/A-101.pdf".into()))));
        assert_eq!(studio.product(), Product::Docs);
        assert!(studio.app.docs.shown);
        assert_eq!(studio.app.docs.pick.as_deref(), Some("drawings/A-101.pdf"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_switch_at_the_top_right_shows_each_product_with_its_own_ribbon() {
        let mut studio = Studio::new(App::default());
        assert_eq!(studio.product(), Product::Viewer);
        let tabs = |studio: &Studio| -> Vec<String> { accessibility(studio).summary().into_iter().filter(|n| n.starts_with("Tab: ")).collect() };
        // the switch names the three products; the Viewer's ribbon has one tab, without modelling
        let shown = tabs(&studio);
        for product in studio.extensions() {
            assert!(shown.contains(&format!("Tab: {}", product.label())), "{shown:?}");
        }
        assert!(shown.contains(&"Tab: Home".to_string()) && !shown.contains(&"Tab: Model".to_string()), "{shown:?}");
        let _ = studio.update(Msg::Product(Product::Modeller));
        assert_eq!((studio.product(), studio.ribbon_page()), (Product::Modeller, RibbonPage::Model));
        let shown = tabs(&studio);
        for tab in ["Tab: Model", "Tab: Draw", "Tab: Project", "Tab: View"] {
            assert!(shown.contains(&tab.to_string()), "{shown:?}");
        }
        let _ = studio.update(Msg::RibbonTab(3));
        assert_eq!(studio.ribbon_page(), RibbonPage::View);
        // another product opens on its first tab
        let _ = studio.update(Msg::Product(Product::Validator));
        assert_eq!(studio.ribbon_page(), RibbonPage::Collaborate);
        assert_eq!(studio.left_panel(studio.look()).0, "Collaboration");
        // the commands to come are there but do nothing yet; the switch marks the product shown
        let commands = studio.default_commands();
        for planned in PLANNED {
            assert!(commands.iter().any(|c| c.id == planned.id && c.enabled), "{}", planned.id);
        }
        assert!(commands.iter().any(|c| c.id == Product::Validator.command() && c.kind == openaec_ui::design::command::CommandKind::Mode(true)));
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
        studio.set_product(Product::Validator);
        let again = Studio::new(App::default()).with_settings_in(dir.clone());
        assert_eq!(again.product(), Product::Validator);
        let _ = std::fs::remove_dir_all(&dir);
        // older preferences have no product: the Viewer
        let prefs: Prefs = serde_json::from_str(r#"{"theme":"light"}"#).unwrap();
        assert_eq!(prefs.product, Product::Viewer);
    }

    #[test]
    fn ifcx_is_a_product_of_its_own_and_shows_the_project_as_ifcx() {
        // not a tab of the Modeller's ribbon: on the switch, for every project
        assert!(Product::ALL.iter().all(|p| (*p == Product::Ifcx) == p.pages().contains(&RibbonPage::Ifcx)));
        let mut app = App::default();
        app.add_demo_building();
        let mut studio = Studio::new(app);
        let _ = studio.update(Msg::Product(Product::Ifcx));
        assert_eq!((studio.product(), studio.ribbon_page()), (Product::Ifcx, RibbonPage::Ifcx));
        assert!(studio.app.ifc.full && studio.app.ifc.shown && studio.app.ifc.structure.is_some(), "the project's IFCX fills the workspace");
        // another product gives the views back
        let _ = studio.update(Msg::Product(Product::Viewer));
        assert!(!studio.app.ifc.full && !studio.app.ifc.shown);
        // without a model there is nothing to show yet, and no error either
        let mut empty = Studio::new(App::default());
        let _ = empty.update(Msg::Product(Product::Ifcx));
        assert!(!empty.app.ifc.full && !matches!(empty.app.status, Status::Error(_)));
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
        assert!(!names.iter().any(|n| n == "Tab: Validator" || n == "Tab: PointCloud"), "{names:?}");
        assert!(names.contains(&"Tab: Model".to_string()), "{names:?}");
        // a product binary: its own name in the title and About
        let alone = Studio::new(App::default()).with_edition(Edition::Only(Product::Validator));
        assert_eq!((alone.products.clone(), alone.product(), alone.title()), (vec![Product::Validator], Product::Validator, format!("Open BIM Validator {VERSION}")));
        assert_eq!(Studio::new(App::default()).title(), format!("Open BIM Suite {VERSION}"));
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
        studio.app.select_modelled(bim_core::ElementId(2));
        let before = studio.app.modeller.model.clone();
        let _ = studio.update(Msg::Navigate(Navigation::Next));
        let _ = studio.update(Msg::App(Message::DeletePicked));
        let _ = studio.update(Msg::App(Message::Undo));
        assert!(studio.app.modeller.edit.is_none() && studio.app.handles_shown().is_none());
        assert_eq!(studio.app.modeller.model, before);
        studio.set_product(Product::Modeller);
        let _ = studio.update(Msg::App(Message::ContextMenu(crate::app::MenuItem::ClearSelection)));
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
