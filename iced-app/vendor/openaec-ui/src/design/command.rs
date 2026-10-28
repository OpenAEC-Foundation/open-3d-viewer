//! One command model (issue #9): a command has one identity, so its shortcut,
//! menu item, ribbon button and tooltip always agree.
//!
//! An app describes each command once as a [`Command`] and draws it with the
//! functions here. Key presses go through [`find`], which looks in the same list,
//! so a shortcut shown in a menu is always a real binding. Issue #22 builds on
//! the `id` to let people set their own shortcuts.
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use iced::keyboard::{key::Named, Key, Modifiers};
use iced::Element;

use super::chrome::MenuItem;
use super::widgets::{self as w, ButtonKind, Density};
use super::Look;

/// What a command does, so it can be shown and announced the right way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    /// Does something once, such as Opslaan.
    Action,
    /// Switches an option on or off, such as Raster; `true` is on.
    Toggle(bool),
    /// Starts a mode that stays until another one starts, such as a drawing
    /// tool; `true` is the active mode.
    Mode(bool),
    /// Opens a dialog first, such as Afdrukken…; the label ends in an ellipsis.
    Dialog,
}

impl CommandKind {
    /// Whether the command shows as switched on or active.
    pub fn is_on(self) -> bool {
        matches!(self, Self::Toggle(true) | Self::Mode(true))
    }
}

/// The key of a [`Shortcut`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShortcutKey {
    /// A character key, lower case, such as `'s'` or `'+'`.
    Char(char),
    /// A named key, such as Delete or F2.
    Named(Named),
}

/// A key with modifiers, such as Ctrl+S. Ctrl stands for Cmd on macOS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub key: ShortcutKey,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Shortcut {
    /// Ctrl and a character, such as `Shortcut::ctrl('s')`.
    pub const fn ctrl(c: char) -> Self {
        Self { key: ShortcutKey::Char(c), ctrl: true, shift: false, alt: false }
    }
    /// Ctrl, Shift and a character, such as `Shortcut::ctrl_shift('z')`.
    pub const fn ctrl_shift(c: char) -> Self {
        Self { key: ShortcutKey::Char(c), ctrl: true, shift: true, alt: false }
    }
    /// A named key on its own, such as `Shortcut::named(Named::Delete)`.
    pub const fn named(key: Named) -> Self {
        Self { key: ShortcutKey::Named(key), ctrl: false, shift: false, alt: false }
    }
    /// Ctrl and a named key, such as Ctrl+F4.
    pub const fn ctrl_named(key: Named) -> Self {
        Self { key: ShortcutKey::Named(key), ctrl: true, shift: false, alt: false }
    }

    /// Whether a key press is this shortcut.
    pub fn matches(&self, key: &Key, modifiers: Modifiers) -> bool {
        if modifiers.command() != self.ctrl || modifiers.alt() != self.alt {
            return false;
        }
        match (self.key, key) {
            (ShortcutKey::Named(own), Key::Named(pressed)) => own == *pressed && modifiers.shift() == self.shift,
            (ShortcutKey::Char(own), Key::Character(pressed)) => {
                let pressed = pressed.as_str().to_lowercase();
                // Shift is part of some characters (+ is Shift+= on many keyboards),
                // so it only has to match for letters.
                let shift_ok = !own.is_alphabetic() || modifiers.shift() == self.shift;
                let same = pressed.chars().eq(own.to_lowercase()) || (own == '+' && pressed == "=");
                same && shift_ok
            }
            _ => false,
        }
    }

    /// The text shown in menus and tooltips, such as "Ctrl+S". Each shortcut
    /// keeps one text for the life of the program, so menus can borrow it.
    pub fn label(&self) -> &'static str {
        static TEXTS: OnceLock<Mutex<HashMap<Shortcut, &'static str>>> = OnceLock::new();
        let mut texts = TEXTS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
        texts.entry(*self).or_insert_with(|| Box::leak(self.to_string().into_boxed_str()))
    }
}

impl std::fmt::Display for Shortcut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.ctrl {
            f.write_str(if cfg!(target_os = "macos") { "⌘" } else { "Ctrl+" })?;
        }
        if self.alt {
            f.write_str(if cfg!(target_os = "macos") { "⌥" } else { "Alt+" })?;
        }
        if self.shift {
            f.write_str(if cfg!(target_os = "macos") { "⇧" } else { "Shift+" })?;
        }
        match self.key {
            ShortcutKey::Char(c) => write!(f, "{}", c.to_uppercase()),
            ShortcutKey::Named(named) => f.write_str(match named {
                Named::Delete => "Del",
                Named::Backspace => "Backspace",
                Named::Enter => "Enter",
                Named::Escape => "Esc",
                Named::Tab => "Tab",
                Named::Space => "Spatie",
                Named::Insert => "Ins",
                Named::Home => "Home",
                Named::End => "End",
                Named::PageUp => "PgUp",
                Named::PageDown => "PgDn",
                Named::F1 => "F1",
                Named::F2 => "F2",
                Named::F3 => "F3",
                Named::F4 => "F4",
                Named::F5 => "F5",
                Named::F6 => "F6",
                Named::F7 => "F7",
                Named::F8 => "F8",
                Named::F9 => "F9",
                Named::F10 => "F10",
                Named::F11 => "F11",
                Named::F12 => "F12",
                _ => return write!(f, "{named:?}"),
            }),
        }
    }
}

/// One command of an app: what it is called, how it looks, its shortcut and the
/// message it sends. Build it with [`Command::new`] and the methods after it.
#[derive(Debug, Clone)]
pub struct Command<M> {
    /// A stable name that does not change with the language, such as
    /// `"file.save"`. Settings such as own shortcuts are stored under it.
    pub id: &'static str,
    /// The label in menus, on the ribbon and in tooltips. A [`CommandKind::Dialog`]
    /// label ends in an ellipsis, such as "Afdrukken…".
    pub label: &'static str,
    /// The 16 px icon for menus and small ribbon buttons.
    pub icon: Option<&'static str>,
    /// The 24 px icon for large ribbon buttons; the 16 px icon when not set.
    pub icon_large: Option<&'static str>,
    pub shortcut: Option<Shortcut>,
    pub kind: CommandKind,
    /// Irreversible, such as Verwijderen: shown in the error colour in menus.
    pub danger: bool,
    pub message: M,
    /// A disabled command shows greyed out and its shortcut does nothing.
    pub enabled: bool,
    /// A longer text for menus and tooltips when it says more than the label,
    /// such as "Ongedaan maken: Prijs wijzigen" (issue #34).
    pub detail: Option<&'static str>,
}

impl<M> Command<M> {
    pub fn new(id: &'static str, label: &'static str, message: M) -> Self {
        Self { id, label, icon: None, icon_large: None, shortcut: None, kind: CommandKind::Action, danger: false, message, enabled: true, detail: None }
    }
    /// The 16 px and 24 px icon.
    pub fn icon(mut self, small: &'static str, large: &'static str) -> Self {
        self.icon = Some(small);
        self.icon_large = Some(large);
        self
    }
    /// Only a 16 px icon, also used on large ribbon buttons.
    pub fn icon_small(mut self, small: &'static str) -> Self {
        self.icon = Some(small);
        self
    }
    pub fn shortcut(mut self, shortcut: Shortcut) -> Self {
        self.shortcut = Some(shortcut);
        self
    }
    pub fn toggle(mut self, on: bool) -> Self {
        self.kind = CommandKind::Toggle(on);
        self
    }
    pub fn mode(mut self, active: bool) -> Self {
        self.kind = CommandKind::Mode(active);
        self
    }
    pub fn dialog(mut self) -> Self {
        self.kind = CommandKind::Dialog;
        self
    }
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    /// The text menus and tooltips show instead of the label. Kept for the life of
    /// the app, like shortcut labels, so use it for a limited set of texts.
    pub fn detail(mut self, text: &str) -> Self {
        self.detail = Some(intern(text));
        self
    }

    /// What menus and tooltips say: the detail, or else the label.
    pub fn description(&self) -> &'static str {
        self.detail.unwrap_or(self.label)
    }

    /// The shortcut text, such as "Ctrl+S".
    pub fn shortcut_label(&self) -> Option<&'static str> {
        self.shortcut.map(|s| s.label())
    }
}

/// Keeps a text for the life of the app, once per distinct text.
fn intern(text: &str) -> &'static str {
    static TEXTS: OnceLock<Mutex<std::collections::HashSet<&'static str>>> = OnceLock::new();
    let mut texts = TEXTS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if let Some(&known) = texts.get(text) {
        return known;
    }
    let kept: &'static str = Box::leak(text.to_owned().into_boxed_str());
    texts.insert(kept);
    kept
}

impl<M: Clone> Command<M> {
    /// The message when the command is enabled.
    pub fn on_press(&self) -> Option<M> {
        self.enabled.then(|| self.message.clone())
    }
}

/// The message of the enabled command whose shortcut is this key press.
pub fn find<M: Clone>(commands: &[Command<M>], key: &Key, modifiers: Modifiers) -> Option<M> {
    commands.iter().find(|c| c.enabled && c.shortcut.is_some_and(|s| s.matches(key, modifiers))).map(|c| c.message.clone())
}

/// Pairs of commands that share a shortcut, by id. An app tests that this is
/// empty, so no key does two things.
pub fn conflicts<M>(commands: &[Command<M>]) -> Vec<(&'static str, &'static str)> {
    let mut seen: HashMap<Shortcut, &'static str> = HashMap::new();
    let mut found = Vec::new();
    for command in commands {
        if let Some(shortcut) = command.shortcut {
            match seen.get(&shortcut) {
                Some(&first) if first != command.id => found.push((first, command.id)),
                Some(_) => {}
                None => {
                    seen.insert(shortcut, command.id);
                }
            }
        }
    }
    found
}

/// The command as a menu line: icon, label, shortcut, a check mark when on, and
/// greyed out when disabled.
pub fn menu_item<M: Clone>(command: &Command<M>) -> MenuItem<'static, M> {
    MenuItem::Action {
        icon: command.icon,
        label: command.description(),
        shortcut: command.shortcut_label(),
        checked: command.kind.is_on(),
        danger: command.danger,
        on_press: command.on_press(),
    }
}

/// What a screen reader gets for a command: a toggle or mode reads as a pressed
/// button, and the shortcut is the description (issue #3).
pub fn node<M>(command: &Command<M>) -> crate::design::a11y::Node {
    use crate::design::a11y::{Node, Role};
    let node = match command.kind {
        CommandKind::Toggle(on) | CommandKind::Mode(on) => Node::new(Role::ToggleButton, command.description()).checked(on),
        _ => Node::new(Role::Button, command.description()),
    };
    let node = node.disabled(!command.enabled);
    match command.shortcut_label() {
        Some(keys) => node.description(keys),
        None => node,
    }
}

/// The ribbon shows a dialog command without its ellipsis, as Office does.
fn ribbon_label<M>(command: &Command<M>) -> &'static str {
    command.label.trim_end_matches('…')
}

/// A large ribbon button (24 px icon above the label) with a tooltip that shows
/// the shortcut.
pub fn ribbon_large<'a, M: Clone + 'a>(look: Look, command: &Command<M>) -> Element<'a, M> {
    let icon = command.icon_large.or(command.icon).unwrap_or_default();
    let button = w::ribbon_large(look, icon, ribbon_label(command), command.kind.is_on(), command.on_press());
    w::with_tooltip(look, button, command.description(), command.shortcut_label())
}

/// A small ribbon button (16 px icon beside the label) with a tooltip.
pub fn ribbon_small<'a, M: Clone + 'a>(look: Look, command: &Command<M>) -> Element<'a, M> {
    let button = w::ribbon_small(look, command.icon.unwrap_or_default(), ribbon_label(command), command.kind.is_on(), command.on_press());
    w::with_tooltip(look, button, command.description(), command.shortcut_label())
}

/// A square icon button of `size` px with a tooltip; a toggle shows as selected
/// when on.
pub fn icon_button<'a, M: Clone + 'a>(look: Look, command: &Command<M>, size: f32) -> Element<'a, M> {
    let button = w::icon_button_with(look, command.icon.unwrap_or_default(), size, command.kind.is_on(), node(command), command.on_press());
    w::with_tooltip(look, button, command.description(), command.shortcut_label())
}

/// A text button with the command's 16 px icon and a tooltip with the shortcut.
pub fn button<'a, M: Clone + 'a>(look: Look, command: &Command<M>, kind: ButtonKind, density: Density) -> Element<'a, M> {
    let kind = if command.danger && kind == ButtonKind::Secondary { ButtonKind::Danger } else { kind };
    let button = w::button(look, command.label, kind, density, command.icon, command.on_press());
    match command.shortcut_label() {
        Some(keys) => w::with_tooltip(look, button, command.description(), Some(keys)),
        None => button,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(c: &str, ctrl: bool, shift: bool) -> (Key, Modifiers) {
        let mut modifiers = Modifiers::empty();
        if ctrl {
            modifiers |= Modifiers::COMMAND;
        }
        if shift {
            modifiers |= Modifiers::SHIFT;
        }
        (Key::Character(c.into()), modifiers)
    }

    #[test]
    fn shortcuts_match_their_key_and_show_one_text() {
        let save = Shortcut::ctrl('s');
        let (key, mods) = press("s", true, false);
        assert!(save.matches(&key, mods));
        let (key, mods) = press("S", true, true);
        assert!(!save.matches(&key, mods), "Ctrl+Shift+S is another shortcut");
        assert!(Shortcut::ctrl_shift('z').matches(&key_char("Z"), Modifiers::COMMAND | Modifiers::SHIFT));
        let (key, mods) = press("=", true, false);
        assert!(Shortcut::ctrl('+').matches(&key, mods), "Ctrl+= counts as Ctrl++");
        assert!(Shortcut::named(Named::Delete).matches(&Key::Named(Named::Delete), Modifiers::empty()));
        assert!(!Shortcut::named(Named::Delete).matches(&Key::Named(Named::Delete), Modifiers::COMMAND));
        if !cfg!(target_os = "macos") {
            assert_eq!(save.label(), "Ctrl+S");
            assert_eq!(Shortcut::ctrl_shift('z').label(), "Ctrl+Shift+Z");
            assert_eq!(Shortcut::named(Named::Delete).label(), "Del");
        }
        assert!(std::ptr::eq(save.label(), Shortcut::ctrl('s').label()), "one text per shortcut");
    }

    fn key_char(c: &str) -> Key {
        Key::Character(c.into())
    }

    #[test]
    fn find_uses_only_enabled_commands_and_conflicts_are_reported() {
        let commands = [
            Command::new("file.save", "Opslaan", "opslaan").shortcut(Shortcut::ctrl('s')),
            Command::new("edit.paste", "Plakken", "plakken").shortcut(Shortcut::ctrl('v')).enabled(false),
            Command::new("view.grid", "Raster", "raster").toggle(true),
        ];
        let (key, mods) = press("s", true, false);
        assert_eq!(find(&commands, &key, mods), Some("opslaan"));
        let (key, mods) = press("v", true, false);
        assert_eq!(find(&commands, &key, mods), None, "a disabled command does nothing");
        assert!(conflicts(&commands).is_empty());
        let clash = [Command::new("a", "A", ()).shortcut(Shortcut::ctrl('s')), Command::new("b", "B", ()).shortcut(Shortcut::ctrl('s'))];
        assert_eq!(conflicts(&clash), vec![("a", "b")]);
    }

    #[test]
    fn a_menu_item_shows_the_same_label_shortcut_and_state() {
        let grid = Command::new("view.grid", "Raster", 1).toggle(true).shortcut(Shortcut::ctrl('g'));
        match menu_item(&grid) {
            MenuItem::Action { label, shortcut, checked, on_press, .. } => {
                assert_eq!(label, "Raster");
                assert_eq!(shortcut, Some(Shortcut::ctrl('g').label()));
                assert!(checked);
                assert_eq!(on_press, Some(1));
            }
            _ => panic!("a command is an action line"),
        }
        let delete = Command::new("edit.delete", "Verwijderen", 2).danger().enabled(false);
        match menu_item(&delete) {
            MenuItem::Action { danger, on_press, .. } => assert!(danger && on_press.is_none()),
            _ => panic!("a command is an action line"),
        }
    }
}
