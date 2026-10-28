//! Tests of the keyboard focus in `focus.rs`: the Tab route, Enter and Space, a
//! dialog that keeps the focus, and a click that gives focus without the ring.
use super::*;
use crate::design::widgets::{button, ButtonKind, Density};
use crate::design::{StatusbarSetting, ThemeId, WorkspaceSetting};
use iced::advanced::clipboard;
use iced::advanced::widget::operation::Outcome;
use iced::widget::column;

const SIZE: Size = Size::new(800.0, 600.0);

fn look() -> Look {
    Look::new(ThemeId::Light, WorkspaceSetting::Theme, StatusbarSetting::Theme)
}

fn renderer() -> Renderer {
    let software = iced_tiny_skia::Renderer::new(crate::fonts::BODY, iced::Pixels(12.0));
    #[cfg(feature = "gpu-renderer")]
    {
        Renderer::Secondary(software)
    }
    #[cfg(not(feature = "gpu-renderer"))]
    {
        software
    }
}

/// Three buttons (one disabled) behind a "dialog" with two more.
fn screen(dialog: bool) -> Element<'static, &'static str> {
    let look = look();
    let behind = column![
        button(look, "Nieuw", ButtonKind::Secondary, Density::Regular, None, Some("nieuw")),
        button(look, "Uitgeschakeld", ButtonKind::Secondary, Density::Regular, None, None),
        button(look, "Opslaan", ButtonKind::Primary, Density::Regular, None, Some("opslaan")),
    ];
    let mut layers = column![inert(behind, dialog)];
    if dialog {
        layers = layers.push(column![
            button(look, "Annuleren", ButtonKind::Quiet, Density::Regular, None, Some("annuleren")),
            button(look, "Exporteren", ButtonKind::Primary, Density::Regular, None, Some("exporteren")),
        ]);
    }
    layers.into()
}

struct Ui {
    element: Element<'static, &'static str>,
    tree: Tree,
    node: layout::Node,
    renderer: Renderer,
}

impl Ui {
    fn new(dialog: bool) -> Self {
        Self::with(screen(dialog))
    }

    fn with(element: Element<'static, &'static str>) -> Self {
        let mut element = element;
        let mut tree = Tree::new(&element);
        let renderer = renderer();
        let node = element.as_widget_mut().layout(&mut tree, &renderer, &layout::Limits::new(Size::ZERO, SIZE));
        Self { element, tree, node, renderer }
    }

    /// Rebuilds the view with or without the dialog, keeping the widget state.
    fn show(&mut self, dialog: bool) {
        self.element = screen(dialog);
        self.tree.diff(&self.element);
        self.node = self.element.as_widget_mut().layout(&mut self.tree, &self.renderer, &layout::Limits::new(Size::ZERO, SIZE));
    }

    /// Runs an operation to the end, as the runtime does for `focus_next`.
    fn operate(&mut self, operation: impl Operation<()> + 'static) {
        let mut operation: Box<dyn Operation<()>> = Box::new(operation);
        loop {
            self.element.as_widget_mut().operate(&mut self.tree, Layout::new(&self.node), &self.renderer, operation.as_mut());
            match operation.finish() {
                Outcome::Chain(next) => operation = next,
                _ => break,
            }
        }
    }

    fn send(&mut self, event: Event, cursor: mouse::Cursor) -> Vec<&'static str> {
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        let viewport = Rectangle::with_size(SIZE);
        self.element.as_widget_mut().update(&mut self.tree, &event, Layout::new(&self.node), cursor, &self.renderer, &mut clipboard::Null, &mut shell, &viewport);
        messages
    }

    fn key(&mut self, named: Named) -> Vec<&'static str> {
        let key = Key::Named(named);
        self.send(
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: key.clone(),
                modified_key: key,
                physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
                location: keyboard::Location::Standard,
                modifiers: keyboard::Modifiers::empty(),
                text: None,
                repeat: false,
            }),
            mouse::Cursor::Unavailable,
        )
    }

    /// Whether each control in the Tab route has the focus, in route order.
    fn route(&mut self) -> Vec<bool> {
        struct Collect(Vec<bool>);
        impl Operation<()> for Collect {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<()>)) {
                operate(self);
            }
            fn focusable(&mut self, _id: Option<&iced::widget::Id>, _bounds: Rectangle, state: &mut dyn Focusable) {
                self.0.push(state.is_focused());
            }
        }
        let mut collect = Collect(Vec::new());
        self.element.as_widget_mut().operate(&mut self.tree, Layout::new(&self.node), &self.renderer, &mut collect);
        collect.0
    }

    fn focused(&mut self) -> Option<usize> {
        self.route().iter().position(|&f| f)
    }
}

#[test]
fn tab_walks_the_enabled_controls_and_enter_presses_the_focused_one() {
    let mut ui = Ui::new(false);
    assert_eq!(ui.route().len(), 2, "a disabled button stays out of the Tab route");
    assert_eq!(ui.focused(), None);
    ui.operate(next());
    assert_eq!(ui.focused(), Some(0));
    assert_eq!(ui.key(Named::Enter), vec!["nieuw"]);
    ui.operate(next());
    assert_eq!(ui.focused(), Some(1));
    assert_eq!(ui.key(Named::Space), vec!["opslaan"]);
    ui.operate(next());
    assert_eq!(ui.focused(), Some(0), "Tab wraps around at the end");
    ui.operate(previous());
    assert_eq!(ui.focused(), Some(1), "Shift+Tab goes back");
    assert!(ui.key(Named::Escape).is_empty(), "other keys do not press the control");
}

#[test]
fn a_dialog_keeps_the_focus_and_returns_it_when_closed() {
    let mut ui = Ui::new(false);
    ui.operate(next());
    ui.operate(next());
    assert_eq!(ui.focused(), Some(1), "Opslaan has the focus");
    ui.show(true);
    assert_eq!(ui.route().len(), 2, "only the dialog's buttons are in the route");
    ui.operate(next());
    assert_eq!(ui.focused(), Some(0));
    ui.operate(next());
    ui.operate(next());
    assert_eq!(ui.focused(), Some(0), "Tab stays inside the dialog");
    assert_eq!(ui.key(Named::Enter), vec!["annuleren"], "Enter does not reach the screen behind");
    ui.show(false);
    assert_eq!(ui.focused(), Some(1), "closing the dialog returns the focus to Opslaan");
    assert_eq!(ui.key(Named::Enter), vec!["opslaan"]);
}

#[test]
fn a_click_moves_the_focus_without_the_ring() {
    let mut ui = Ui::new(false);
    let first = Layout::new(&ui.node).children().next().unwrap().children().next().unwrap().bounds();
    let _ = ui.send(Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)), mouse::Cursor::Available(first.center()));
    assert_eq!(ui.focused(), Some(0));
    let state = *ui.tree.children[0].children[0].children[0].state.downcast_ref::<State>();
    assert!(state.focused && !state.visible, "a click shows no focus ring");
    ui.operate(next());
    assert_eq!(ui.focused(), Some(1));
}

/// A button, a list of three as one group, and another button.
fn list() -> Element<'static, &'static str> {
    let look = look();
    let item = |label, message| button(look, label, ButtonKind::Quiet, Density::Regular, None, Some(message));
    column![
        item("Voor", "voor"),
        group(column![item("Een", "een"), item("Twee", "twee"), item("Drie", "drie")], Axis::Vertical),
        item("Na", "na"),
    ]
    .into()
}

#[test]
fn a_group_is_one_tab_stop_that_the_arrows_walk_through() {
    let mut ui = Ui::with(list());
    assert_eq!(ui.route().len(), 3, "the list of three counts as one stop");
    ui.operate(next());
    ui.operate(next());
    assert_eq!(ui.key(Named::Enter), vec!["een"], "Tab enters the list at its first item");
    let _ = ui.key(Named::ArrowDown);
    assert_eq!(ui.key(Named::Enter), vec!["twee"]);
    let _ = ui.key(Named::End);
    assert_eq!(ui.key(Named::Enter), vec!["drie"]);
    let _ = ui.key(Named::ArrowDown);
    assert_eq!(ui.key(Named::Enter), vec!["drie"], "the arrows stop at the end");
    let _ = ui.key(Named::Home);
    assert_eq!(ui.key(Named::Enter), vec!["een"]);
    let _ = ui.key(Named::ArrowUp);
    assert_eq!(ui.key(Named::Enter), vec!["een"], "and at the start");
    let _ = ui.key(Named::ArrowDown);
    let _ = ui.key(Named::ArrowDown);
    assert!(ui.key(Named::ArrowLeft).is_empty() && ui.key(Named::Enter) == vec!["drie"], "Left does nothing in a vertical list");
    ui.operate(next());
    assert_eq!(ui.key(Named::Enter), vec!["na"], "the next Tab leaves the list");
    ui.operate(previous());
    assert_eq!(ui.key(Named::Enter), vec!["drie"], "Shift+Tab returns to the item that last had the focus");
}

/// A closed branch and an open branch in a tree list.
fn branches() -> Element<'static, &'static str> {
    let look = look();
    let row = |label, message| button(look, label, ButtonKind::Quiet, Density::Regular, None, Some(message));
    group(column![expander(row("Fundering", "kies fundering"), false, "fundering open"), expander(row("Vloeren", "kies vloeren"), true, "vloeren dicht")], Axis::Vertical)
}

#[test]
fn right_opens_and_left_closes_the_focused_branch() {
    let mut ui = Ui::with(branches());
    ui.operate(next());
    assert!(ui.key(Named::ArrowLeft).is_empty(), "Left on a closed branch does nothing");
    assert_eq!(ui.key(Named::ArrowRight), vec!["fundering open"]);
    let _ = ui.key(Named::ArrowDown);
    assert!(ui.key(Named::ArrowRight).is_empty(), "Right on an open branch does nothing");
    assert_eq!(ui.key(Named::ArrowLeft), vec!["vloeren dicht"]);
    assert_eq!(ui.key(Named::Enter), vec!["kies vloeren"], "the arrows did not move the focus");
}

/// A button with a keyboard tip, and a second button.
fn tipped() -> Element<'static, &'static str> {
    let look = look();
    column![
        tip(button(look, "Opslaan", ButtonKind::Primary, Density::Regular, None, Some("opslaan")), iced::widget::text("Opslaan  Ctrl+S")),
        button(look, "Verder", ButtonKind::Secondary, Density::Regular, None, Some("verder")),
    ]
    .into()
}

impl Ui {
    fn tip_shows(&mut self) -> bool {
        let viewport = Rectangle::with_size(SIZE);
        self.element.as_widget_mut().overlay(&mut self.tree, Layout::new(&self.node), &self.renderer, &viewport, Vector::ZERO).is_some()
    }
}

#[test]
fn a_tip_shows_on_keyboard_focus_and_escape_hides_it() {
    // Issue #7: what the mouse gets as a tooltip, the keyboard gets too.
    let mut ui = Ui::with(tipped());
    assert!(!ui.tip_shows());
    ui.operate(next());
    assert!(ui.tip_shows(), "Tab to the button shows its tip");
    assert!(ui.key(Named::Escape).is_empty());
    assert!(!ui.tip_shows(), "Escape hides it");
    ui.operate(next());
    let _ = ui.key(Named::Shift);
    ui.operate(previous());
    assert!(ui.tip_shows(), "coming back shows it again");
    let first = Layout::new(&ui.node).children().next().unwrap().bounds();
    let _ = ui.send(Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)), mouse::Cursor::Available(first.center()));
    assert!(!ui.tip_shows(), "a click gives focus without the tip; hovering shows the normal tooltip");
}
