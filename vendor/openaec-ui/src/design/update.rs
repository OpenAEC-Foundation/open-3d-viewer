//! Update notice (issue #33): every app shows in its title bar that a newer
//! version is ready, and "Bijwerken" hands the update to the OpenAEC Installer.
//!
//! As decided on 26 October 2026, the installer is the one place for updates:
//! apps do not go online and do not install anything themselves. The contract:
//! - The installer writes, after each check, a status file ([`status_path`]):
//!   `%LOCALAPPDATA%\OpenAEC\installer\updates.json` on Windows, with its own
//!   path and per app (by its catalogue id, such as `open-calc-studio`) the
//!   latest version and its GitHub release page. See [`StatusFile`].
//! - An app reads that file at start ([`check_installed`]) and compares the
//!   latest version with its own. When it is newer, [`notice`] shows it.
//! - "Bijwerken" first closes the app through its close plan
//!   (`design::document`), so no work is lost, and then calls [`open_installer`]:
//!   `<installer> --update <app id>`. Without an installer the release page
//!   opens in the browser, so people can still see it on GitHub.
use super::a11y::{self, Node, Role};
use super::widgets::{self, ButtonKind, Density};
use super::{font, Look};
use crate::fonts;
use iced::widget::{row, text};
use iced::{Alignment, Element};
use serde::Deserialize;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The argument with which an app opens the installer on its own update.
pub const INSTALLER_ARGUMENT: &str = "--update";

/// The status file the installer writes.
///
/// ```json
/// {
///   "checked": "2026-10-26T08:00:00Z",
///   "installer": "C:\\Users\\nino\\AppData\\Local\\OpenAEC Installer\\openaec-installer.exe",
///   "apps": {
///     "open-calc-studio": { "latest": "2026.44.1", "page_url": "https://github.com/OpenAEC-Foundation/open-calc-studio/releases/tag/v2026.44.1" }
///   }
/// }
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct StatusFile {
    /// When the installer last checked GitHub.
    #[serde(default)]
    pub checked: Option<String>,
    /// The installer's own executable.
    #[serde(default)]
    pub installer: Option<PathBuf>,
    #[serde(default)]
    pub apps: BTreeMap<String, AppStatus>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct AppStatus {
    pub latest: String,
    #[serde(default)]
    pub page_url: Option<String>,
}

/// A newer version that is ready for this app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Available {
    /// The app's catalogue id in the installer, such as `open-calc-studio`.
    pub app: String,
    pub current: String,
    pub latest: String,
    pub page_url: Option<String>,
    pub installer: Option<PathBuf>,
}

/// Where the installer writes its status file.
pub fn status_path() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("OpenAEC"))
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/OpenAEC"))
    } else {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
            .map(|d| d.join("openaec"))
    };
    base.map(|b| b.join("installer").join("updates.json"))
}

pub fn read_status(path: &Path) -> Result<StatusFile, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

/// The update for `app`, when the status lists a version newer than `current`.
pub fn check(status: &StatusFile, app: &str, current: &str) -> Option<Available> {
    let entry = status.apps.get(app)?;
    (compare(&entry.latest, current) == Ordering::Greater).then(|| Available {
        app: app.to_string(),
        current: current.to_string(),
        latest: entry.latest.clone(),
        page_url: entry.page_url.clone(),
        installer: status.installer.clone(),
    })
}

/// [`check`] against the installer's status file. No file, an unreadable file
/// or no newer version: no notice.
pub fn check_installed(app: &str, current: &str) -> Option<Available> {
    let status = read_status(&status_path()?).ok()?;
    check(&status, app, current)
}

/// Compares versions such as `2026.44.1`, `v1.10.0` or `2026.43.0-beta`: the
/// numbers part by part, and a version with a suffix before the same version
/// without one.
pub fn compare(a: &str, b: &str) -> Ordering {
    fn parts(v: &str) -> (Vec<u64>, bool) {
        let v = v.trim().trim_start_matches(['v', 'V']);
        let (core, suffix) = v.split_once(['-', '+']).map_or((v, false), |(core, _)| (core, true));
        (core.split('.').map(|p| p.chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0)).collect(), suffix)
    }
    let ((mut na, sa), (mut nb, sb)) = (parts(a), parts(b));
    let len = na.len().max(nb.len());
    na.resize(len, 0);
    nb.resize(len, 0);
    na.cmp(&nb).then(sb.cmp(&sa))
}

/// Opens the installer on this app's update; without an installer, the release
/// page in the browser. Call it after the app's close plan has finished.
pub fn open_installer(available: &Available) -> Result<(), String> {
    if let Some(installer) = available.installer.as_ref().filter(|p| p.is_file()) {
        return std::process::Command::new(installer).arg(INSTALLER_ARGUMENT).arg(&available.app).spawn().map(|_| ()).map_err(|e| e.to_string());
    }
    let url = available.page_url.as_deref().ok_or("De installer is niet gevonden en er is geen releasepagina.")?;
    open_in_browser(url)
}

fn open_in_browser(url: &str) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("Alleen https-adressen worden geopend.".into());
    }
    let mut command = if cfg!(windows) {
        let mut c = std::process::Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else {
        std::process::Command::new("xdg-open")
    };
    command.arg(url).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// The notice for the title bar: the new version and a compact "Bijwerken"
/// button. Put it in the `extra` part of `chrome::title_bar`.
pub fn notice<'a, M: Clone + 'a>(look: Look, available: &Available, on_update: M) -> Element<'a, M> {
    let label = format!("Versie {} staat klaar", available.latest);
    let content = row![
        text(label.clone()).size(font::SIZE_LABEL).font(fonts::BODY).color(look.color.text_secondary),
        widgets::button(look, "Bijwerken", ButtonKind::Primary, Density::Compact, None, Some(on_update)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    a11y::node(content, Node::new(Role::Status, label).description(format!("Nu {}. Bijwerken opent de OpenAEC Installer.", available.current)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_part_by_part() {
        assert_eq!(compare("2026.44.1", "2026.43.9"), Ordering::Greater);
        assert_eq!(compare("v1.10.0", "1.9.3"), Ordering::Greater);
        assert_eq!(compare("2026.43", "2026.43.0"), Ordering::Equal);
        assert_eq!(compare("2026.43.0-beta", "2026.43.0"), Ordering::Less);
        assert_eq!(compare("0.4.0", "0.4.0"), Ordering::Equal);
    }

    #[test]
    fn the_status_file_names_the_newer_version_of_this_app() {
        let status: StatusFile = serde_json::from_str(
            r#"{
                "checked": "2026-10-26T08:00:00Z",
                "installer": "C:\\OpenAEC\\openaec-installer.exe",
                "apps": {
                    "open-calc-studio": { "latest": "2026.44.1", "page_url": "https://github.com/OpenAEC-Foundation/open-calc-studio/releases/tag/v2026.44.1" },
                    "open-pdf-studio": { "latest": "2026.43.0" }
                }
            }"#,
        )
        .expect("status");
        let calc = check(&status, "open-calc-studio", "2026.43.2").expect("newer");
        assert_eq!(calc.latest, "2026.44.1");
        assert_eq!(calc.installer.as_deref(), Some(Path::new("C:\\OpenAEC\\openaec-installer.exe")));
        assert_eq!(check(&status, "open-pdf-studio", "2026.43.0"), None, "same version: no notice");
        assert_eq!(check(&status, "open-field-studio", "2026.40.0"), None, "not in the file: no notice");
        // An unknown field from a newer installer does no harm.
        assert!(serde_json::from_str::<StatusFile>(r#"{"apps":{},"extra":1}"#).is_ok());
    }

    #[test]
    fn without_installer_or_page_nothing_opens() {
        let available = Available { app: "open-calc-studio".into(), current: "1".into(), latest: "2".into(), page_url: None, installer: None };
        assert!(open_installer(&available).is_err());
        let unsafe_page = Available { page_url: Some("file:///C:/evil.exe".into()), ..available };
        assert!(open_installer(&unsafe_page).is_err(), "only https pages open");
        assert!(status_path().is_some_and(|p| p.ends_with("installer/updates.json") || p.ends_with("installer\\updates.json")));
    }
}
