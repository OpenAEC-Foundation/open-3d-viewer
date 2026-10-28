//! Shortcuts outside text fields.
//!
//! iced 0.14 removed `keyboard::on_key_press`; this keeps the same contract on
//! top of `keyboard::listen`: only key presses that no widget captured arrive.
use iced::keyboard::{Event, Key, Modifiers};
use iced::Subscription;

/// Key presses that no widget captured, mapped by `f`. Like every iced
/// subscription mapper, `f` must be a non-capturing closure or a function.
pub fn on_key_press<M, F>(f: F) -> Subscription<M>
where
    M: Send + 'static,
    F: Fn(Key, Modifiers) -> Option<M> + Send + Clone + 'static,
{
    iced::keyboard::listen().filter_map(move |event| match event {
        Event::KeyPressed { key, modifiers, .. } => f(key, modifiers),
        _ => None,
    })
}

/// A step in the Tab route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Navigation {
    Next,
    Previous,
}

/// Tab and Shift+Tab that no widget captured. Map it to a message and answer
/// with [`navigate`]; see `design::focus` for the controls in the route.
pub fn navigation() -> Subscription<Navigation> {
    on_key_press(|key, modifiers| match key {
        Key::Named(iced::keyboard::key::Named::Tab) if !modifiers.command() && !modifiers.alt() => {
            Some(if modifiers.shift() { Navigation::Previous } else { Navigation::Next })
        }
        _ => None,
    })
}

/// Moves the focus one step along the Tab route, wrapping at the ends.
pub fn navigate<T: Send + 'static>(step: Navigation) -> iced::Task<T> {
    match step {
        Navigation::Next => iced::advanced::widget::operate(crate::design::focus::next()),
        Navigation::Previous => iced::advanced::widget::operate(crate::design::focus::previous()),
    }
}

/// A key press that no widget captured.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyPress {
    pub key: Key,
    pub modifiers: Modifiers,
}

/// Every key press that no widget captured. Look it up in the app's commands
/// with `design::command::find`, so shortcuts and menus share one list.
pub fn presses() -> Subscription<KeyPress> {
    on_key_press(|key, modifiers| Some(KeyPress { key, modifiers }))
}
