//! Keyboard focus for every control (issue #2).
//!
//! iced 0.14 only lets text fields take focus. [`focusable`] gives any control a
//! place in the Tab route: Enter and Space then send it the same press and release
//! as a mouse click, so buttons, tiles, checkboxes and pick lists work without
//! knowing about the keyboard. The 2 px focus ring sits 2 px outside the control
//! and only shows when the focus came from the keyboard, like `:focus-visible` with
//! `outline-offset` on the web.
//!
//! [`inert`] takes everything behind an open dialog or menu out of the Tab route and
//! out of the key handling. The control that had the focus keeps it, so closing the
//! dialog returns the focus there without remembering anything.
//!
//! Tab and Shift+Tab come from [`crate::keyboard::navigation`].
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Renderer as _};
use iced::advanced::widget::operation::{Focusable, Operation};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{overlay, Clipboard, Shell, Widget};
use iced::keyboard::{self, key::Named, Key};
use iced::{mouse, Background, Border, Color, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};

use super::Look;

thread_local! {
    /// Set while an inert screen draws: the control that keeps the focus behind
    /// an open dialog shows no ring, so only the dialog's focus is visible.
    static BEHIND_DIALOG: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Whether the focus last moved by keyboard: a [`tip`] only shows then, not
    /// after a click.
    static KEYBOARD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Width of the focus ring and its distance from the control.
const RING_WIDTH: f32 = 2.0;
const RING_GAP: f32 = 2.0;

/// The focus state of one control.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct State {
    focused: bool,
    /// Whether the ring shows: the focus came from the keyboard, not a click.
    visible: bool,
}

impl Focusable for State {
    fn is_focused(&self) -> bool {
        self.focused
    }
    fn focus(&mut self) {
        // Only focus operations (Tab, Shift+Tab, `focus(id)`) call this.
        self.focused = true;
        self.visible = true;
        KEYBOARD.with(|k| k.set(true));
    }
    fn unfocus(&mut self) {
        self.focused = false;
        self.visible = false;
    }
}

/// Puts `content` in the Tab route. `enabled: false` leaves it out, as for a
/// disabled button. `radius` is the corner of the focus ring, the same as the
/// control's own corner.
pub fn focusable<'a, M: 'a>(look: Look, content: impl Into<Element<'a, M>>, enabled: bool, radius: f32) -> Element<'a, M> {
    let content = content.into();
    if !enabled {
        return content;
    }
    Element::new(Focus { content, ring: look.color.focus, radius, id: None })
}

/// Like [`focusable`], with an id so `iced::widget::operation::focus(id)` can move
/// the focus here, for example to the first field of a form.
pub fn focusable_with_id<'a, M: 'a>(look: Look, content: impl Into<Element<'a, M>>, id: iced::widget::Id, radius: f32) -> Element<'a, M> {
    Element::new(Focus { content: content.into(), ring: look.color.focus, radius, id: Some(id) })
}

struct Focus<'a, M> {
    content: Element<'a, M>,
    ring: Color,
    radius: f32,
    id: Option<iced::widget::Id>,
}

/// The press and release of a mouse click, as Enter and Space send them.
fn click_events() -> [Event; 2] {
    [
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
    ]
}

/// Moves the focus to the next control of the Tab route, wrapping from the last
/// to the first. iced's own `focus_next` passes through "no focus" at the end.
pub fn next<T: Send + 'static>() -> impl Operation<T> {
    iced::advanced::widget::operation::then(iced::advanced::widget::operation::focusable::count(), |count| Step { target: step_target(count, true), current: 0 })
}

/// Moves the focus to the previous control, wrapping from the first to the last.
pub fn previous<T: Send + 'static>() -> impl Operation<T> {
    iced::advanced::widget::operation::then(iced::advanced::widget::operation::focusable::count(), |count| Step { target: step_target(count, false), current: 0 })
}

fn step_target(count: iced::advanced::widget::operation::focusable::Count, forward: bool) -> Option<usize> {
    let total = count.total;
    if total == 0 {
        return None;
    }
    Some(match (count.focused, forward) {
        (None, true) => 0,
        (None, false) => total - 1,
        (Some(i), true) => (i + 1) % total,
        (Some(i), false) => (i + total - 1) % total,
    })
}

struct Step {
    target: Option<usize>,
    current: usize,
}

impl<T> Operation<T> for Step {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<T>)) {
        operate(self);
    }
    fn focusable(&mut self, _id: Option<&iced::widget::Id>, _bounds: Rectangle, state: &mut dyn Focusable) {
        if Some(self.current) == self.target {
            state.focus();
        } else {
            state.unfocus();
        }
        self.current += 1;
    }
}

impl<M> Widget<M, Theme, Renderer> for Focus<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }
    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        operation.focusable(self.id.as_ref(), layout.bounds(), tree.state.downcast_mut::<State>());
        operation.traverse(&mut |operation| {
            self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let state = *tree.state.downcast_ref::<State>();
        match event {
            Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Named(Named::Enter | Named::Space), repeat: false, modifiers, .. })
                if state.focused && !modifiers.command() && !modifiers.alt() =>
            {
                // The same press and release a mouse click in the middle would give.
                // Each half gets its own shell: a button ignores the release once
                // the press counts as captured in the same shell.
                let centre = mouse::Cursor::Available(bounds.center());
                for click in click_events() {
                    let mut messages = Vec::new();
                    let mut own = Shell::new(&mut messages);
                    self.content.as_widget_mut().update(&mut tree.children[0], &click, layout, centre, renderer, clipboard, &mut own, viewport);
                    shell.merge(own, |message| message);
                }
                shell.capture_event();
                shell.request_redraw();
                return;
            }
            Event::Mouse(mouse::Event::ButtonPressed(_)) | Event::Touch(iced::touch::Event::FingerPressed { .. }) => {
                // A click moves the focus here without showing the ring, and away elsewhere.
                KEYBOARD.with(|k| k.set(false));
                let own = tree.state.downcast_mut::<State>();
                let inside = cursor.is_over(bounds);
                if own.focused != inside || own.visible {
                    *own = State { focused: inside, visible: false };
                    shell.request_redraw();
                }
            }
            _ => {}
        }
        self.content.as_widget_mut().update(&mut tree.children[0], event, layout, cursor, renderer, clipboard, shell, viewport);
    }

    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &Renderer) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }

    fn draw(&self, tree: &Tree, renderer: &mut Renderer, theme: &Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle) {
        self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
        let state = tree.state.downcast_ref::<State>();
        if state.focused && state.visible && !BEHIND_DIALOG.with(|b| b.get()) {
            // A 2 px ring 2 px outside the control, as `outline-offset` on the web, so it
            // stays visible on a selected control whose own border has the same colour.
            // Its own layer keeps the next row from painting over it.
            let ring = layout.bounds().expand(RING_GAP + RING_WIDTH);
            renderer.with_layer(ring.expand(1.0), |renderer| {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: ring,
                        border: Border { color: self.ring, width: RING_WIDTH, radius: (self.radius + RING_GAP + RING_WIDTH).into() },
                        ..Default::default()
                    },
                    Background::Color(Color::TRANSPARENT),
                );
            });
        }
    }

    fn overlay<'b>(&'b mut self, tree: &'b mut Tree, layout: Layout<'b>, renderer: &Renderer, viewport: &Rectangle, translation: Vector) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(&mut tree.children[0], layout, renderer, viewport, translation)
    }
}

/// Takes `content` out of the Tab route and the key handling while `inert` is
/// true, for the screen behind an open dialog or menu. Its focus stays where it
/// was, so it comes back when the dialog closes.
pub fn inert<'a, M: 'a>(content: impl Into<Element<'a, M>>, inert: bool) -> Element<'a, M> {
    Element::new(Inert { content: content.into(), inert })
}

struct Inert<'a, M> {
    content: Element<'a, M>,
    inert: bool,
}

impl<M> Widget<M, Theme, Renderer> for Inert<'_, M> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }
    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
    }
    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        if !self.inert {
            self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
        }
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        // Keys and clicks belong to the dialog; window events still pass.
        if self.inert && matches!(event, Event::Keyboard(_) | Event::Mouse(_) | Event::Touch(_)) {
            return;
        }
        self.content.as_widget_mut().update(&mut tree.children[0], event, layout, cursor, renderer, clipboard, shell, viewport);
    }
    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &Renderer) -> mouse::Interaction {
        if self.inert {
            return mouse::Interaction::None;
        }
        self.content.as_widget().mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }
    fn draw(&self, tree: &Tree, renderer: &mut Renderer, theme: &Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle) {
        let cursor = if self.inert { mouse::Cursor::Unavailable } else { cursor };
        let outer = BEHIND_DIALOG.with(|b| b.replace(b.get() || self.inert));
        self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
        BEHIND_DIALOG.with(|b| b.set(outer));
    }
    fn overlay<'b>(&'b mut self, tree: &'b mut Tree, layout: Layout<'b>, renderer: &Renderer, viewport: &Rectangle, translation: Vector) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        if self.inert {
            return None;
        }
        self.content.as_widget_mut().overlay(&mut tree.children[0], layout, renderer, viewport, translation)
    }
}

/// The arrow keys of a [`group`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Up and Down, as in a list, menu, table or sidebar.
    Vertical,
    /// Left and Right, as in tabs, a step bar or the ribbon.
    Horizontal,
}

/// A group of controls that the arrow keys walk through, such as the rows of a
/// table or the items of a menu. Home and End go to the first and last control.
/// The group is one stop in the Tab route ("roving focus"): Tab lands on the
/// control that last had the focus in the group, and the next Tab leaves the group.
pub fn group<'a, M: 'a>(content: impl Into<Element<'a, M>>, axis: Axis) -> Element<'a, M> {
    Element::new(Group { content: content.into(), axis })
}

struct Group<'a, M> {
    content: Element<'a, M>,
    axis: Axis,
}

/// The control of the group that takes part in the Tab route.
#[derive(Debug, Clone, Copy, Default)]
struct GroupState {
    last: usize,
}

/// Finds the focused control of a group and how many there are.
struct Census {
    total: usize,
    focused: Option<usize>,
}

impl Operation for Census {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }
    fn focusable(&mut self, _id: Option<&iced::widget::Id>, _bounds: Rectangle, state: &mut dyn Focusable) {
        if state.is_focused() {
            self.focused = Some(self.total);
        }
        self.total += 1;
    }
}

/// Focuses one control of the group, as from the keyboard, and unfocuses the rest.
struct FocusIndex {
    target: usize,
    current: usize,
}

impl Operation for FocusIndex {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }
    fn focusable(&mut self, _id: Option<&iced::widget::Id>, _bounds: Rectangle, state: &mut dyn Focusable) {
        if self.current == self.target {
            state.focus();
        } else {
            state.unfocus();
        }
        self.current += 1;
    }
}

/// Passes an outside operation through to one control of the group only, so the
/// Tab route sees the group as a single stop.
struct Roving<'o> {
    inner: &'o mut dyn Operation,
    shown: usize,
    current: usize,
}

impl Operation for Roving<'_> {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }
    fn container(&mut self, id: Option<&iced::widget::Id>, bounds: Rectangle) {
        self.inner.container(id, bounds);
    }
    fn scrollable(
        &mut self,
        id: Option<&iced::widget::Id>,
        bounds: Rectangle,
        content_bounds: Rectangle,
        translation: Vector,
        state: &mut dyn iced::advanced::widget::operation::Scrollable,
    ) {
        self.inner.scrollable(id, bounds, content_bounds, translation, state);
    }
    fn focusable(&mut self, id: Option<&iced::widget::Id>, bounds: Rectangle, state: &mut dyn Focusable) {
        if self.current == self.shown {
            self.inner.focusable(id, bounds, state);
        }
        self.current += 1;
    }
    fn text_input(&mut self, id: Option<&iced::widget::Id>, bounds: Rectangle, state: &mut dyn iced::advanced::widget::operation::TextInput) {
        self.inner.text_input(id, bounds, state);
    }
    fn text(&mut self, id: Option<&iced::widget::Id>, bounds: Rectangle, text: &str) {
        self.inner.text(id, bounds, text);
    }
    fn custom(&mut self, id: Option<&iced::widget::Id>, bounds: Rectangle, state: &mut dyn std::any::Any) {
        self.inner.custom(id, bounds, state);
    }
}

impl<M> Group<'_, M> {
    fn census(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer) -> Census {
        let mut census = Census { total: 0, focused: None };
        self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, &mut census);
        census
    }
}

impl<M> Widget<M, Theme, Renderer> for Group<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<GroupState>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(GroupState::default())
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }
    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        let census = self.census(tree, layout, renderer);
        if census.total == 0 {
            return self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
        }
        let last = tree.state.downcast_ref::<GroupState>().last;
        let shown = census.focused.unwrap_or(last.min(census.total - 1));
        let mut roving = Roving { inner: operation, shown, current: 0 };
        self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, &mut roving);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        if let Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Named(named), modifiers, .. }) = event {
            let (back, forward) = match self.axis {
                Axis::Vertical => (Named::ArrowUp, Named::ArrowDown),
                Axis::Horizontal => (Named::ArrowLeft, Named::ArrowRight),
            };
            if !modifiers.command() && !modifiers.alt() && !modifiers.shift() && [back, forward, Named::Home, Named::End].contains(named) {
                let census = self.census(tree, layout, renderer);
                if let Some(focused) = census.focused {
                    let last = census.total - 1;
                    let target = match *named {
                        Named::Home => 0,
                        Named::End => last,
                        n if n == back => focused.saturating_sub(1),
                        _ => (focused + 1).min(last),
                    };
                    let mut focus = FocusIndex { target, current: 0 };
                    self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, &mut focus);
                    tree.state.downcast_mut::<GroupState>().last = target;
                    shell.capture_event();
                    shell.request_redraw();
                    return;
                }
            }
        }
        self.content.as_widget_mut().update(&mut tree.children[0], event, layout, cursor, renderer, clipboard, shell, viewport);
        // A click inside the group makes that control the group's Tab stop.
        if matches!(event, Event::Mouse(mouse::Event::ButtonPressed(_)) | Event::Touch(iced::touch::Event::FingerPressed { .. })) {
            if let Some(focused) = self.census(tree, layout, renderer).focused {
                tree.state.downcast_mut::<GroupState>().last = focused;
            }
        }
    }

    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &Renderer) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }
    fn draw(&self, tree: &Tree, renderer: &mut Renderer, theme: &Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle) {
        self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
    }
    fn overlay<'b>(&'b mut self, tree: &'b mut Tree, layout: Layout<'b>, renderer: &Renderer, viewport: &Rectangle, translation: Vector) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(&mut tree.children[0], layout, renderer, viewport, translation)
    }
}

/// A tree row that opens and closes with the arrow keys while it has the focus:
/// Right opens a closed branch, Left closes an open one, as in Windows Explorer.
/// The app knows whether the branch is open, so it wraps its own rows.
pub fn expander<'a, M: Clone + 'a>(content: impl Into<Element<'a, M>>, expanded: bool, on_toggle: M) -> Element<'a, M> {
    Element::new(Expander { content: content.into(), expanded, on_toggle })
}

struct Expander<'a, M> {
    content: Element<'a, M>,
    expanded: bool,
    on_toggle: M,
}

impl<M: Clone> Widget<M, Theme, Renderer> for Expander<'_, M> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }
    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
    }
    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        if let Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Named(named @ (Named::ArrowLeft | Named::ArrowRight)), modifiers, .. }) = event {
            let wanted = *named == Named::ArrowRight;
            if modifiers.is_empty() && wanted != self.expanded {
                let mut census = Census { total: 0, focused: None };
                self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, &mut census);
                if census.focused.is_some() {
                    shell.publish(self.on_toggle.clone());
                    shell.capture_event();
                    return;
                }
            }
        }
        self.content.as_widget_mut().update(&mut tree.children[0], event, layout, cursor, renderer, clipboard, shell, viewport);
    }
    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &Renderer) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }
    fn draw(&self, tree: &Tree, renderer: &mut Renderer, theme: &Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle) {
        self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
    }
    fn overlay<'b>(&'b mut self, tree: &'b mut Tree, layout: Layout<'b>, renderer: &Renderer, viewport: &Rectangle, translation: Vector) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(&mut tree.children[0], layout, renderer, viewport, translation)
    }
}

/// Shows `tip` under `content` while a control inside it has the keyboard focus
/// (issue #7), so a name or full text that the mouse gets as a tooltip is also
/// there without a mouse. Escape hides it until the focus moves on.
/// [`super::widgets::with_tooltip`] uses this together with the hover tooltip.
pub fn tip<'a, M: 'a>(content: impl Into<Element<'a, M>>, tip: impl Into<Element<'a, M>>) -> Element<'a, M> {
    Element::new(Tip { content: content.into(), tip: tip.into() })
}

struct Tip<'a, M> {
    content: Element<'a, M>,
    tip: Element<'a, M>,
}

#[derive(Debug, Clone, Copy, Default)]
struct TipState {
    /// Escape hid the tip; it comes back when the focus has left and returns.
    dismissed: bool,
}

/// Gap between the control and its tip, as `with_tooltip` uses.
const TIP_GAP: f32 = 4.0;

impl<M> Tip<'_, M> {
    fn has_keyboard_focus(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer) -> bool {
        if !KEYBOARD.with(|k| k.get()) {
            return false;
        }
        let mut census = Census { total: 0, focused: None };
        self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, &mut census);
        census.focused.is_some()
    }
}

impl<M> Widget<M, Theme, Renderer> for Tip<'_, M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<TipState>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(TipState::default())
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content), Tree::new(&self.tip)]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.content, &self.tip]);
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }
    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
    }
    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        let focused = self.has_keyboard_focus(tree, layout, renderer);
        let state = tree.state.downcast_mut::<TipState>();
        if !focused {
            state.dismissed = false;
        } else if !state.dismissed && matches!(event, Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Named(Named::Escape), .. })) {
            // Escape first closes the tip; the next one reaches the app.
            state.dismissed = true;
            shell.capture_event();
            shell.request_redraw();
            return;
        }
        self.content.as_widget_mut().update(&mut tree.children[0], event, layout, cursor, renderer, clipboard, shell, viewport);
    }
    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &Renderer) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }
    fn draw(&self, tree: &Tree, renderer: &mut Renderer, theme: &Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle) {
        self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
    }
    fn overlay<'b>(&'b mut self, tree: &'b mut Tree, layout: Layout<'b>, renderer: &Renderer, viewport: &Rectangle, translation: Vector) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        let showing = self.has_keyboard_focus(tree, layout, renderer) && !tree.state.downcast_ref::<TipState>().dismissed;
        let [content_tree, tip_tree] = tree.children.as_mut_slice() else { return None };
        if !showing {
            return self.content.as_widget_mut().overlay(content_tree, layout, renderer, viewport, translation);
        }
        Some(overlay::Element::new(Box::new(TipOverlay { tip: &mut self.tip, tree: tip_tree, anchor: layout.bounds() + translation })))
    }
}

struct TipOverlay<'a, 'b, M> {
    tip: &'b mut Element<'a, M>,
    tree: &'b mut Tree,
    anchor: Rectangle,
}

impl<M> overlay::Overlay<M, Theme, Renderer> for TipOverlay<'_, '_, M> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let node = self.tip.as_widget_mut().layout(self.tree, renderer, &layout::Limits::new(Size::ZERO, bounds));
        let size = node.size();
        // Centred under the control, as the hover tooltip, and kept inside the window.
        let x = (self.anchor.center_x() - size.width / 2.0).clamp(0.0, (bounds.width - size.width).max(0.0));
        let below = self.anchor.y + self.anchor.height + TIP_GAP;
        let y = if below + size.height > bounds.height { (self.anchor.y - TIP_GAP - size.height).max(0.0) } else { below };
        node.move_to(iced::Point::new(x, y))
    }
    fn draw(&self, renderer: &mut Renderer, theme: &Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor) {
        self.tip.as_widget().draw(self.tree, renderer, theme, style, layout, cursor, &layout.bounds());
    }
}

#[cfg(test)]
#[path = "focus_tests.rs"]
mod tests;
