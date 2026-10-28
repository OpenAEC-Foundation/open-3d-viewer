//! Shortcuts people set themselves (issue #22).
//!
//! A [`Keymap`] holds only the shortcuts someone changed, by command id, and is
//! saved per app as a small JSON file. The app applies it to its commands, so
//! the ribbon, menus, tooltips and keys all show the chosen shortcut. The page
//! [`page`] lists every command; [`Editor`] records a new shortcut from the next
//! key press, warns when another command already has it and offers to swap.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use iced::keyboard::{key::Named, Key, Modifiers};
use iced::widget::{column, container, row, scrollable, text, Space};
use iced::{Alignment, Element, Length};

use super::command::{Command, Shortcut, ShortcutKey};
use super::widgets::{self as w, ButtonKind, Density};
use super::{font, Look};
use crate::fonts;
use crate::keyboard::KeyPress;

/// Named keys a shortcut can use, with their stored names.
const NAMED: [(Named, &str); 19] = [
    (Named::Delete, "delete"),
    (Named::Insert, "insert"),
    (Named::Backspace, "backspace"),
    (Named::Home, "home"),
    (Named::End, "end"),
    (Named::PageUp, "pageup"),
    (Named::PageDown, "pagedown"),
    (Named::F1, "f1"),
    (Named::F2, "f2"),
    (Named::F3, "f3"),
    (Named::F4, "f4"),
    (Named::F5, "f5"),
    (Named::F6, "f6"),
    (Named::F7, "f7"),
    (Named::F8, "f8"),
    (Named::F9, "f9"),
    (Named::F10, "f10"),
    (Named::F11, "f11"),
    (Named::F12, "f12"),
];

/// Keys that operate the interface (issue #2) and can never be a shortcut.
const RESERVED: [Named; 10] = [
    Named::Tab,
    Named::Enter,
    Named::Escape,
    Named::Space,
    Named::ArrowUp,
    Named::ArrowDown,
    Named::ArrowLeft,
    Named::ArrowRight,
    Named::Home,
    Named::End,
];

impl Shortcut {
    /// The stored form, the same on every platform, such as `ctrl+shift+z`.
    pub fn to_storage(&self) -> String {
        let mut out = String::new();
        if self.ctrl {
            out.push_str("ctrl+");
        }
        if self.alt {
            out.push_str("alt+");
        }
        if self.shift {
            out.push_str("shift+");
        }
        match self.key {
            ShortcutKey::Char(c) => out.push(c),
            ShortcutKey::Named(named) => out.push_str(NAMED.iter().find(|(n, _)| *n == named).map_or("?", |(_, name)| name)),
        }
        out
    }

    /// Reads the stored form; `None` for anything it does not know.
    pub fn parse(stored: &str) -> Option<Self> {
        let mut shortcut = Shortcut { key: ShortcutKey::Char(' '), ctrl: false, shift: false, alt: false };
        let parts: Vec<&str> = stored.split('+').collect();
        // `+` itself is the last part when the key is the plus sign.
        let (mods, key) = match parts.as_slice() {
            [rest @ .., "", ""] => (rest, "+"),
            [rest @ .., key] => (rest, *key),
            [] => return None,
        };
        for m in mods {
            match *m {
                "ctrl" => shortcut.ctrl = true,
                "alt" => shortcut.alt = true,
                "shift" => shortcut.shift = true,
                _ => return None,
            }
        }
        shortcut.key = match NAMED.iter().find(|(_, name)| *name == key) {
            Some((named, _)) => ShortcutKey::Named(*named),
            None => {
                let mut chars = key.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => ShortcutKey::Char(c.to_lowercase().next().unwrap_or(c)),
                    _ => return None,
                }
            }
        };
        Some(shortcut)
    }
}

/// The shortcuts someone changed, by command id. `None` means the command has
/// no shortcut on purpose.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Keymap {
    changed: BTreeMap<String, Option<Shortcut>>,
}

impl Keymap {
    /// Applies the changes to an app's commands, in place.
    pub fn apply<M>(&self, commands: &mut [Command<M>]) {
        for command in commands {
            if let Some(shortcut) = self.changed.get(command.id) {
                command.shortcut = *shortcut;
            }
        }
    }

    /// Sets a command's shortcut; back to the default removes the change.
    pub fn set(&mut self, id: &str, shortcut: Option<Shortcut>, default: Option<Shortcut>) {
        if shortcut == default {
            self.changed.remove(id);
        } else {
            self.changed.insert(id.to_string(), shortcut);
        }
    }

    /// Back to the app's own shortcut for one command.
    pub fn reset(&mut self, id: &str) {
        self.changed.remove(id);
    }

    /// Back to the app's own shortcuts for all commands.
    pub fn reset_all(&mut self) {
        self.changed.clear();
    }

    pub fn is_changed(&self, id: &str) -> bool {
        self.changed.contains_key(id)
    }

    pub fn is_empty(&self) -> bool {
        self.changed.is_empty()
    }

    /// The JSON form: `{"file.save": "ctrl+shift+s", "edit.delete": null}`.
    pub fn to_json(&self) -> String {
        let map: BTreeMap<&str, Option<String>> = self.changed.iter().map(|(id, s)| (id.as_str(), s.map(|s| s.to_storage()))).collect();
        serde_json::to_string_pretty(&map).unwrap_or_else(|_| "{}".into())
    }

    /// Reads the JSON form; entries it does not understand are left out.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let map: BTreeMap<String, Option<String>> = serde_json::from_str(json)?;
        let changed = map
            .into_iter()
            .filter_map(|(id, stored)| match stored {
                None => Some((id, None)),
                Some(s) => Shortcut::parse(&s).map(|shortcut| (id, Some(shortcut))),
            })
            .collect();
        Ok(Self { changed })
    }

    /// Reads the file, or an empty keymap when there is none or it is unreadable.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).ok().and_then(|json| Self::from_json(&json).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.to_json())
    }
}

/// Where an app keeps its shortcuts: `%APPDATA%\OpenAEC\<app>\sneltoetsen.json`
/// on Windows, `~/Library/Application Support/OpenAEC/<app>/` on macOS and
/// `$XDG_CONFIG_HOME/openaec/<app>/` (or `~/.config/openaec/<app>/`) on Linux.
pub fn keymap_path(app: &str) -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("OpenAEC"))
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/OpenAEC"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|d| d.join("openaec"))
    };
    base.map(|b| b.join(app).join("sneltoetsen.json"))
}

/// A message of the shortcuts page.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Search(String),
    /// Wait for the next key press as the shortcut of this command.
    Record(&'static str),
    Key(KeyPress),
    /// Give the new shortcut to this command and the old one to the other.
    Swap,
    Cancel,
    Reset(&'static str),
    ResetAll,
}

/// Two commands want the same shortcut.
#[derive(Debug, Clone, PartialEq)]
struct Conflict {
    id: &'static str,
    shortcut: Shortcut,
    other: &'static str,
    text: String,
}

/// The state of the shortcuts page.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Editor {
    search: String,
    recording: Option<&'static str>,
    conflict: Option<Conflict>,
    notice: Option<&'static str>,
}

impl Editor {
    /// Whether the next key press is for the page; the app sends it as
    /// [`Message::Key`] instead of running a command.
    pub fn recording(&self) -> bool {
        self.recording.is_some()
    }

    /// Handles a page message. `defaults` are the app's commands without the
    /// keymap. Returns true when the keymap changed, so the app saves it.
    pub fn update<M>(&mut self, message: Message, keymap: &mut Keymap, defaults: &[Command<M>]) -> bool {
        let default = |id: &str| defaults.iter().find(|c| c.id == id).and_then(|c| c.shortcut);
        let current = |keymap: &Keymap, id: &str| match keymap.changed.get(id) {
            Some(changed) => *changed,
            None => default(id),
        };
        match message {
            Message::Search(s) => self.search = s,
            Message::Record(id) => {
                self.recording = Some(id);
                self.conflict = None;
                self.notice = Some("Druk de nieuwe sneltoets. Backspace wist hem, Esc stopt.");
            }
            Message::Cancel => {
                self.recording = None;
                self.conflict = None;
                self.notice = None;
            }
            Message::Reset(id) => {
                keymap.reset(id);
                self.notice = None;
                return true;
            }
            Message::ResetAll => {
                keymap.reset_all();
                self.recording = None;
                self.conflict = None;
                self.notice = Some("Alle sneltoetsen staan weer op de standaard.");
                return true;
            }
            Message::Swap => {
                if let Some(conflict) = self.conflict.take() {
                    let old = current(keymap, conflict.id);
                    keymap.set(conflict.id, Some(conflict.shortcut), default(conflict.id));
                    keymap.set(conflict.other, old, default(conflict.other));
                    self.notice = None;
                    return true;
                }
            }
            Message::Key(press) => {
                let Some(id) = self.recording else { return false };
                match recorded(&press) {
                    Recorded::Wait => {}
                    Recorded::Stop => {
                        self.recording = None;
                        self.notice = None;
                    }
                    Recorded::Clear => {
                        self.recording = None;
                        self.notice = None;
                        keymap.set(id, None, default(id));
                        return true;
                    }
                    Recorded::Refused(why) => self.notice = Some(why),
                    Recorded::Shortcut(shortcut) => {
                        self.recording = None;
                        self.notice = None;
                        if current(keymap, id) == Some(shortcut) {
                            return false;
                        }
                        let other = defaults.iter().find(|c| c.id != id && current(keymap, c.id) == Some(shortcut));
                        if let Some(other) = other {
                            self.conflict = Some(Conflict { id, shortcut, other: other.id, text: format!("{} hoort al bij {}.", shortcut.label(), other.label.trim_end_matches('…')) });
                            return false;
                        }
                        keymap.set(id, Some(shortcut), default(id));
                        return true;
                    }
                }
            }
        }
        false
    }
}

enum Recorded {
    /// Only a modifier so far.
    Wait,
    Stop,
    Clear,
    Refused(&'static str),
    Shortcut(Shortcut),
}

fn recorded(press: &KeyPress) -> Recorded {
    let m: Modifiers = press.modifiers;
    match &press.key {
        Key::Named(Named::Escape) => Recorded::Stop,
        Key::Named(Named::Backspace) if m.is_empty() => Recorded::Clear,
        Key::Named(Named::Control | Named::Shift | Named::Alt | Named::Super | Named::Meta) => Recorded::Wait,
        Key::Named(named) if RESERVED.contains(named) => Recorded::Refused("Deze toets is voor de bediening en kan geen sneltoets worden."),
        Key::Named(named) => match NAMED.iter().find(|(n, _)| n == named) {
            Some(_) => Recorded::Shortcut(Shortcut { key: ShortcutKey::Named(*named), ctrl: m.command(), shift: m.shift(), alt: m.alt() }),
            None => Recorded::Refused("Deze toets kan geen sneltoets worden."),
        },
        Key::Character(c) => {
            if !m.command() && !m.alt() {
                return Recorded::Refused("Kies een toets met Ctrl of Alt, of een functietoets: losse letters zijn voor typen.");
            }
            match c.as_str().chars().next() {
                Some(ch) => Recorded::Shortcut(Shortcut { key: ShortcutKey::Char(ch.to_lowercase().next().unwrap_or(ch)), ctrl: m.command(), shift: m.shift(), alt: m.alt() }),
                None => Recorded::Wait,
            }
        }
        Key::Unidentified => Recorded::Wait,
    }
}

/// The Sneltoetsen page: search, the commands per group with their shortcut, a
/// button to change it and one back to the default, and the conflict question.
/// `commands` are the app's commands with the keymap applied; `groups` names
/// the id prefixes, such as `("file", "Bestand")`. About 560 px wide, for a dialog.
pub fn page<'a, M: Clone + 'a>(
    look: Look,
    editor: &'a Editor,
    commands: &[Command<M>],
    keymap: &Keymap,
    groups: &[(&'static str, &'static str)],
    on_message: impl Fn(Message) -> M + Copy + 'a,
) -> Element<'a, M> {
    let c = look.color;
    let needle = editor.search.trim().to_lowercase();
    let mut list = column![].spacing(2);
    let group_of = |id: &str| {
        let prefix = id.split('.').next().unwrap_or(id);
        groups.iter().position(|(p, _)| *p == prefix).unwrap_or(groups.len())
    };
    let mut order: Vec<&Command<M>> = commands
        .iter()
        .filter(|cmd| needle.is_empty() || cmd.label.to_lowercase().contains(&needle) || cmd.shortcut_label().is_some_and(|s| s.to_lowercase().contains(&needle)))
        .collect();
    order.sort_by_key(|cmd| group_of(cmd.id));
    let mut last_group = None;
    for cmd in order {
        let group = group_of(cmd.id);
        if last_group != Some(group) {
            last_group = Some(group);
            let name = groups.get(group).map_or("Overig", |(_, n)| n);
            list = list.push(container(w::mono_label(look, name)).padding(iced::Padding { top: 10.0, right: 8.0, bottom: 4.0, left: 8.0 }));
        }
        let recording = editor.recording == Some(cmd.id);
        let keys = if recording { "Druk een toets…" } else { cmd.shortcut_label().unwrap_or("Geen") };
        let mut line = row![
            match cmd.icon {
                Some(icon) => w::glyph(icon, 16.0, c.text, c.accent_text),
                None => Space::new().width(16).into(),
            },
            text(cmd.label.trim_end_matches('…')).size(font::SIZE_BODY).font(fonts::BODY).color(c.text).width(Length::Fill),
            container(text(keys).size(font::SIZE_LABEL).font(fonts::MONO).color(if recording { c.accent_text } else { c.text_secondary })).width(120),
            w::button(look, if recording { "Stoppen" } else { "Wijzigen" }, ButtonKind::Secondary, Density::Compact, None, Some(on_message(if recording { Message::Cancel } else { Message::Record(cmd.id) }))),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        line = line.push(if keymap.is_changed(cmd.id) {
            w::button(look, "Standaard", ButtonKind::Quiet, Density::Compact, None, Some(on_message(Message::Reset(cmd.id))))
        } else {
            Space::new().width(78).into()
        });
        list = list.push(container(line).height(32).padding([0, 8]).align_y(iced::alignment::Vertical::Center));
    }
    let mut page = column![w::text_field(look, "Zoek een opdracht of sneltoets", &editor.search, Some(move |s| on_message(Message::Search(s))), Density::Regular, None)].spacing(10);
    if let Some(conflict) = &editor.conflict {
        page = page.push(
            container(
                row![
                    text(&conflict.text).size(font::SIZE_BODY).font(fonts::BODY).color(c.text).width(Length::Fill),
                    w::button(look, "Annuleren", ButtonKind::Quiet, Density::Compact, None, Some(on_message(Message::Cancel))),
                    w::button(look, "Wisselen", ButtonKind::Primary, Density::Compact, None, Some(on_message(Message::Swap))),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding([8, 10])
            .style(move |_| container::Style { background: Some(iced::Background::Color(c.warning_tint)), border: iced::Border { color: c.warning, width: 1.0, radius: super::radius::MEDIUM.into() }, ..Default::default() }),
        );
    }
    if let Some(notice) = editor.notice {
        page = page.push(container(w::secondary(look, notice)).padding([0, 8]));
    }
    page = page.push(scrollable(super::focus::group(list, super::focus::Axis::Vertical)).height(320));
    page.push(row![
        Space::new().width(Length::Fill),
        w::button(look, "Alles terugzetten", ButtonKind::Quiet, Density::Regular, None, (!keymap.is_empty()).then(|| on_message(Message::ResetAll))),
    ])
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commands() -> Vec<Command<&'static str>> {
        vec![
            Command::new("file.save", "Opslaan", "opslaan").shortcut(Shortcut::ctrl('s')),
            Command::new("file.print", "Afdrukken…", "afdrukken").shortcut(Shortcut::ctrl('p')).dialog(),
            Command::new("view.grid", "Raster", "raster"),
        ]
    }

    fn press(key: Key, modifiers: Modifiers) -> Message {
        Message::Key(KeyPress { key, modifiers })
    }

    fn effective(keymap: &Keymap) -> Vec<Command<&'static str>> {
        let mut list = commands();
        keymap.apply(&mut list);
        list
    }

    #[test]
    fn stored_shortcuts_read_back_the_same() {
        for s in [Shortcut::ctrl('s'), Shortcut::ctrl_shift('z'), Shortcut::ctrl('+'), Shortcut::named(Named::Delete), Shortcut::ctrl_named(Named::F4)] {
            assert_eq!(Shortcut::parse(&s.to_storage()), Some(s), "{}", s.to_storage());
        }
        let mut keymap = Keymap::default();
        keymap.set("file.save", Some(Shortcut::ctrl_shift('s')), Some(Shortcut::ctrl('s')));
        keymap.set("file.print", None, Some(Shortcut::ctrl('p')));
        assert_eq!(Keymap::from_json(&keymap.to_json()).unwrap(), keymap);
        assert!(keymap.to_json().contains("\"ctrl+shift+s\""));
    }

    #[test]
    fn recording_sets_a_shortcut_and_the_commands_show_it() {
        let defaults = commands();
        let mut keymap = Keymap::default();
        let mut editor = Editor::default();
        editor.update(Message::Record("view.grid"), &mut keymap, &defaults);
        assert!(editor.recording());
        assert!(!editor.update(press(Key::Character("g".into()), Modifiers::empty()), &mut keymap, &defaults), "a plain letter is refused");
        assert!(editor.recording(), "and recording goes on");
        assert!(!editor.update(press(Key::Named(Named::Control), Modifiers::COMMAND), &mut keymap, &defaults), "Ctrl alone waits");
        assert!(editor.update(press(Key::Character("g".into()), Modifiers::COMMAND), &mut keymap, &defaults));
        assert!(!editor.recording());
        assert_eq!(effective(&keymap)[2].shortcut, Some(Shortcut::ctrl('g')));
        editor.update(Message::Reset("view.grid"), &mut keymap, &defaults);
        assert!(keymap.is_empty(), "back to the default leaves nothing changed");
    }

    #[test]
    fn a_taken_shortcut_asks_before_it_swaps() {
        let defaults = commands();
        let mut keymap = Keymap::default();
        let mut editor = Editor::default();
        editor.update(Message::Record("file.print"), &mut keymap, &defaults);
        assert!(!editor.update(press(Key::Character("s".into()), Modifiers::COMMAND), &mut keymap, &defaults), "nothing changes yet");
        assert_eq!(editor.conflict.as_ref().map(|c| c.text.as_str()), Some(format!("{} hoort al bij Opslaan.", Shortcut::ctrl('s').label()).as_str()));
        assert!(editor.update(Message::Swap, &mut keymap, &defaults));
        let list = effective(&keymap);
        assert_eq!(list[1].shortcut, Some(Shortcut::ctrl('s')), "Afdrukken gets Ctrl+S");
        assert_eq!(list[0].shortcut, Some(Shortcut::ctrl('p')), "Opslaan gets the old Ctrl+P");
        assert!(super::super::command::conflicts(&list).is_empty());
        editor.update(Message::Record("file.save"), &mut keymap, &defaults);
        assert!(editor.update(press(Key::Named(Named::Backspace), Modifiers::empty()), &mut keymap, &defaults), "Backspace clears");
        assert_eq!(effective(&keymap)[0].shortcut, None);
        editor.update(Message::Record("file.save"), &mut keymap, &defaults);
        editor.update(press(Key::Named(Named::Tab), Modifiers::empty()), &mut keymap, &defaults);
        assert!(editor.recording(), "Tab is refused and recording goes on");
        editor.update(press(Key::Named(Named::Escape), Modifiers::empty()), &mut keymap, &defaults);
        assert!(!editor.recording(), "Escape stops");
        assert!(editor.update(Message::ResetAll, &mut keymap, &defaults));
        assert!(keymap.is_empty());
    }
}
