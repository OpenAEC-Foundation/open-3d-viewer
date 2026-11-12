//! Resizing the window by its edges.
//!
//! The shell draws its own title bar, so the window has no system frame, and without that frame
//! the system gives it no edges to drag. [`resize_frame`] wraps the whole screen: within a few
//! pixels of an edge or corner the cursor becomes a resize arrow, and a left press there sends a
//! message with the direction, which the shell answers with `iced::window::drag_resize`.
use openaec_ui::iced::advanced::layout::{self, Layout};
use openaec_ui::iced::advanced::renderer;
use openaec_ui::iced::advanced::widget::{Operation, Tree};
use openaec_ui::iced::advanced::{overlay, Clipboard, Shell, Widget};
use openaec_ui::iced::window::Direction;
use openaec_ui::iced::{mouse, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector};

/// How far from an edge a press still resizes (logical pixels).
const EDGE: f32 = 5.0;
/// How far from a corner, along an edge, a press resizes both ways.
const CORNER: f32 = 12.0;

/// `content` with the window's edges as resize handles while `enabled` (not when maximized).
/// `on_resize` makes the message for a press on an edge or corner.
pub fn resize_frame<'a, M: 'a>(content: impl Into<Element<'a, M>>, enabled: bool, on_resize: fn(Direction) -> M) -> Element<'a, M> {
    Element::new(ResizeFrame { content: content.into(), enabled, on_resize })
}

/// The edge or corner of a window of `size` that `point` is on, if any.
pub fn edge_at(size: Size, point: Point) -> Option<Direction> {
    let inside = point.x >= 0.0 && point.y >= 0.0 && point.x <= size.width && point.y <= size.height;
    let (left, right) = (point.x, size.width - point.x);
    let (top, bottom) = (point.y, size.height - point.y);
    if !inside || left.min(right).min(top).min(bottom) > EDGE {
        return None;
    }
    // near a corner both distances count, so a corner is easier to hit than its 5 px square
    let (north, south) = (top <= CORNER, bottom <= CORNER);
    let (west, east) = (left <= CORNER, right <= CORNER);
    Some(match (north, south, west, east) {
        (true, _, true, _) => Direction::NorthWest,
        (true, _, _, true) => Direction::NorthEast,
        (_, true, true, _) => Direction::SouthWest,
        (_, true, _, true) => Direction::SouthEast,
        _ if top <= EDGE => Direction::North,
        _ if bottom <= EDGE => Direction::South,
        _ if left <= EDGE => Direction::West,
        _ => Direction::East,
    })
}

struct ResizeFrame<'a, M> {
    content: Element<'a, M>,
    enabled: bool,
    on_resize: fn(Direction) -> M,
}

impl<M> ResizeFrame<'_, M> {
    fn edge(&self, layout: Layout<'_>, cursor: mouse::Cursor) -> Option<Direction> {
        if !self.enabled {
            return None;
        }
        let bounds = layout.bounds();
        let point = cursor.position()?;
        edge_at(bounds.size(), Point::new(point.x - bounds.x, point.y - bounds.y))
    }
}

impl<M> Widget<M, Theme, Renderer> for ResizeFrame<'_, M> {
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
        // a press on an edge is the window's, not the button's or the 3D view's under it
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
            if let Some(direction) = self.edge(layout, cursor) {
                shell.publish((self.on_resize)(direction));
                shell.capture_event();
                return;
            }
        }
        self.content.as_widget_mut().update(&mut tree.children[0], event, layout, cursor, renderer, clipboard, shell, viewport);
    }
    fn mouse_interaction(&self, tree: &Tree, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle, renderer: &Renderer) -> mouse::Interaction {
        match self.edge(layout, cursor) {
            Some(Direction::North | Direction::South) => mouse::Interaction::ResizingVertically,
            Some(Direction::East | Direction::West) => mouse::Interaction::ResizingHorizontally,
            Some(Direction::NorthEast | Direction::SouthWest) => mouse::Interaction::ResizingDiagonallyUp,
            Some(Direction::NorthWest | Direction::SouthEast) => mouse::Interaction::ResizingDiagonallyDown,
            None => self.content.as_widget().mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer),
        }
    }
    fn draw(&self, tree: &Tree, renderer: &mut Renderer, theme: &Theme, style: &renderer::Style, layout: Layout<'_>, cursor: mouse::Cursor, viewport: &Rectangle) {
        self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
    }
    fn overlay<'b>(&'b mut self, tree: &'b mut Tree, layout: Layout<'b>, renderer: &Renderer, viewport: &Rectangle, translation: Vector) -> Option<overlay::Element<'b, M, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(&mut tree.children[0], layout, renderer, viewport, translation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Size = Size { width: 1280.0, height: 800.0 };

    fn at(x: f32, y: f32) -> Option<Direction> {
        edge_at(WINDOW, Point::new(x, y))
    }

    #[test]
    fn the_middle_of_the_window_is_no_edge() {
        assert!(at(640.0, 400.0).is_none());
        assert!(at(6.0, 400.0).is_none());
        assert!(at(640.0, 794.0).is_none());
    }

    #[test]
    fn each_edge_resizes_its_own_way() {
        assert!(matches!(at(640.0, 2.0), Some(Direction::North)));
        assert!(matches!(at(640.0, 798.0), Some(Direction::South)));
        assert!(matches!(at(2.0, 400.0), Some(Direction::West)));
        assert!(matches!(at(1278.0, 400.0), Some(Direction::East)));
    }

    #[test]
    fn a_corner_reaches_along_both_edges() {
        assert!(matches!(at(1.0, 1.0), Some(Direction::NorthWest)));
        assert!(matches!(at(1270.0, 3.0), Some(Direction::NorthEast)));
        assert!(matches!(at(3.0, 790.0), Some(Direction::SouthWest)));
        assert!(matches!(at(1279.0, 799.0), Some(Direction::SouthEast)));
        // 12 px from the corner along the edge, but not on the edge itself
        assert!(at(10.0, 10.0).is_none());
    }

    #[test]
    fn outside_the_window_is_no_edge() {
        assert!(at(-1.0, 400.0).is_none());
        assert!(at(640.0, 801.0).is_none());
    }
}
