//! Swiping a [`scrollable`](iced::widget::scrollable) full of buttons with a finger: the ribbon
//! on a phone, or the list of models and layers.
//!
//! iced's scrollable scrolls by touch only where no button takes the finger first, and these are
//! nearly all buttons. This wrapper watches the fingers before its content does: a finger that
//! moves more than a few pixels along the axis scrolls the scrollable `id` with it, and the button
//! under it lets go (it gets the finger as lost, so it does not fire). A tap stays a tap, and a
//! finger the content already follows by itself (between the buttons, or on a slider) is left to it.

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::operation::{scrollable::scroll_by, Operation};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{overlay, Clipboard, Shell, Widget};
use iced::widget::scrollable::AbsoluteOffset;
use iced::widget::Id;
use iced::{mouse, touch, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector};

/// How far a finger moves along the axis before it swipes instead of pressing (logical pixels).
const SLOP: f32 = 10.0;

/// Wraps `content`, which holds the scrollable `id` that scrolls sideways (`horizontal`) or up
/// and down.
pub fn swipe<'a, M: 'a>(id: Id, horizontal: bool, content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    Element::new(Swipe { id, horizontal, content: content.into() })
}

struct Swipe<'a, M> {
    id: Id,
    horizontal: bool,
    content: Element<'a, M>,
}

#[derive(Default)]
struct State {
    /// The finger that went down on the content: where it started and was last seen.
    finger: Option<(touch::Finger, Point, Point)>,
    swiping: bool,
}

impl<M> Widget<M, Theme, Renderer> for Swipe<'_, M> {
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
        let state = tree.state.downcast_mut::<State>();
        match *event {
            Event::Touch(touch::Event::FingerPressed { id, position }) if layout.bounds().contains(position) => {
                state.finger = Some((id, position, position));
                state.swiping = false;
            }
            Event::Touch(touch::Event::FingerMoved { id, position }) => {
                if let Some((finger, start, last)) = state.finger.filter(|(f, ..)| *f == id) {
                    let along = |p: Point| if self.horizontal { p.x } else { p.y };
                    if !state.swiping && (along(position) - along(start)).abs() > SLOP {
                        state.swiping = true;
                        // the button under the finger lets go
                        let lost = Event::Touch(touch::Event::FingerLost { id, position });
                        self.content.as_widget_mut().update(&mut tree.children[0], &lost, layout, cursor, renderer, clipboard, shell, viewport);
                    }
                    let state = tree.state.downcast_mut::<State>();
                    if state.swiping {
                        state.finger = Some((finger, start, position));
                        let offset = if self.horizontal { AbsoluteOffset { x: last.x - position.x, y: 0.0 } } else { AbsoluteOffset { x: 0.0, y: last.y - position.y } };
                        let mut scroll = scroll_by::<()>(self.id.clone(), offset);
                        self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, &mut scroll);
                        shell.invalidate_layout();
                        shell.request_redraw();
                        shell.capture_event();
                        return;
                    }
                    // short of the slop the content sees the move: content that follows the finger
                    // itself (the scrollable, where the finger landed between buttons, or a slider)
                    // keeps it, or the swipe would scroll again by what the scrollable already did
                    let captured = shell.is_event_captured();
                    self.content.as_widget_mut().update(&mut tree.children[0], event, layout, cursor, renderer, clipboard, shell, viewport);
                    if !captured && shell.is_event_captured() {
                        tree.state.downcast_mut::<State>().finger = None;
                    }
                    return;
                }
            }
            Event::Touch(touch::Event::FingerLifted { id, .. } | touch::Event::FingerLost { id, .. }) => {
                if state.finger.is_some_and(|(f, ..)| f == id) {
                    state.finger = None;
                    if std::mem::take(&mut state.swiping) {
                        shell.capture_event();
                        return;
                    }
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
    }
    fn overlay<'b>(&'b mut self, tree: &'b mut Tree, layout: Layout<'b>, renderer: &Renderer, viewport: &Rectangle, translation: Vector) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(&mut tree.children[0], layout, renderer, viewport, translation)
    }
}
