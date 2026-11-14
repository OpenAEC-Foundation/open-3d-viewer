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
//! - Once at start, [`refresh_if_stale`] lets the installer check in the
//!   background when its status file is older than a day.
//!
//! The contract itself lives in `update_core.rs`, which the plugin
//! `openaec-update` in openaec-installer shares with the Tauri apps; this module
//! adds the notice.
pub use super::update_core::*;
use super::a11y::{self, Node, Role};
use super::widgets::{self, ButtonKind, Density};
use super::Look;
use crate::fonts;
use iced::widget::{row, text};
use iced::{Alignment, Element};

/// The notice for the title bar: the new version and a compact "Bijwerken"
/// button. Put it in the `extra` part of `chrome::title_bar`.
pub fn notice<'a, M: Clone + 'a>(look: Look, available: &Available, on_update: M) -> Element<'a, M> {
    let label = format!("Versie {} staat klaar", available.latest);
    let content = row![
        text(label.clone()).size(look.text.label).font(fonts::BODY).color(look.color.text_secondary),
        widgets::button(look, "Bijwerken", ButtonKind::Primary, Density::Compact, None, Some(on_update)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    a11y::node(content, Node::new(Role::Status, label).description(format!("Nu {}. Bijwerken opent de OpenAEC Installer.", available.current)))
}
