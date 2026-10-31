//! The 3D viewport: an iced `shader` widget program plus mouse navigation.
//!
//! As Revit: middle drag pans, Shift + middle drag orbits, the wheel zooms towards the cursor.
//! Also left drag orbits, Shift + left or right drag pans, and a left click (press and release on
//! the same spot) picks. While measuring, the cursor position is reported once per frame as it
//! moves, so the app can highlight what a click would pick.
//! The camera lives in the application state; the widget reports navigation as messages.

pub mod camera;
pub mod render;

use crate::explode::ExplodeIndex;
use crate::model::Model;
use camera::Camera;
use iced::mouse;
use iced::widget::shader;
use iced::{Event, Point, Rectangle};
use render::{RenderSettings, ScenePrimitive};
use std::sync::Arc;

/// Navigation requests from the viewport.
#[derive(Debug, Clone, Copy)]
pub enum Navigate {
    Orbit { dx: f32, dy: f32 },
    Pan { dx: f32, dy: f32, height: f32 },
    /// Wheel steps at (x, y) in a viewport of width × height (logical pixels).
    Zoom { steps: f32, x: f32, y: f32, width: f32, height: f32 },
    /// The viewport moved or changed size (logical pixels, origin in the window), for
    /// fit-to-view and to cut the viewport out of window screenshots.
    Resized { x: f32, y: f32, width: f32, height: f32 },
    /// Left click at (x, y) in the viewport (logical pixels).
    Pick { x: f32, y: f32, width: f32, height: f32 },
    /// The cursor rests at (x, y) in the viewport (only while `hover` is on).
    Hover { x: f32, y: f32, width: f32, height: f32 },
    /// The cursor left the viewport.
    HoverOff,
}

pub struct Viewport3D<'a> {
    pub model: Option<&'a Arc<Model>>,
    pub visible: &'a [bool],
    /// Highlighted index ranges: (layer, first index, index count).
    pub selection: &'a [(usize, u32, u32)],
    pub camera: Camera,
    pub settings: RenderSettings,
    /// The exploded view: the units of the model and the offset (world) of each.
    pub explode: Option<&'a Arc<ExplodeIndex>>,
    pub offsets: &'a Arc<Vec<[f32; 4]>>,
    /// Report where the cursor is ([`Navigate::Hover`]): while measuring.
    pub hover: bool,
}

#[derive(Default)]
pub struct State {
    drag: Option<(Drag, Point)>,
    /// Where the left button went down, for click detection.
    press: Option<Point>,
    shift: bool,
    bounds: Rectangle,
    /// Where the cursor moved to since the last frame, reported on the next frame.
    hover: Option<Point>,
    /// A hover was reported and not yet switched off.
    hovering: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Drag {
    Orbit,
    Pan,
}

impl<Message> shader::Program<Message> for Viewport3D<'_>
where
    Message: From<Navigate>,
{
    type State = State;
    type Primitive = ScenePrimitive;

    fn update(
        &self,
        state: &mut State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<shader::Action<Message>> {
        let publish = |n: Navigate| Some(shader::Action::publish(Message::from(n)).and_capture());
        let hover_off = |state: &mut State| {
            state.hover = None;
            std::mem::take(&mut state.hovering).then(|| shader::Action::publish(Message::from(Navigate::HoverOff)))
        };

        match event {
            Event::Window(iced::window::Event::RedrawRequested(_)) => {
                if state.bounds == bounds {
                    // at most one hover a frame, however fast the mouse reports
                    let at = state.hover.take().filter(|_| self.hover && state.drag.is_none())?;
                    state.hovering = true;
                    return Some(shader::Action::publish(Message::from(Navigate::Hover {
                        x: at.x,
                        y: at.y,
                        width: bounds.width,
                        height: bounds.height,
                    })));
                }
                state.bounds = bounds;
                Some(shader::Action::publish(Message::from(Navigate::Resized {
                    x: bounds.x,
                    y: bounds.y,
                    width: bounds.width,
                    height: bounds.height,
                })))
            }
            Event::Keyboard(iced::keyboard::Event::ModifiersChanged(m)) => {
                state.shift = m.shift();
                None
            }
            Event::Mouse(mouse::Event::ButtonPressed(button)) => {
                let position = cursor.position_in(bounds)?;
                let drag = match button {
                    mouse::Button::Left if state.shift => Drag::Pan,
                    mouse::Button::Left => Drag::Orbit,
                    mouse::Button::Middle if state.shift => Drag::Orbit,
                    mouse::Button::Right | mouse::Button::Middle => Drag::Pan,
                    _ => return None,
                };
                state.drag = Some((drag, position));
                state.press = (*button == mouse::Button::Left && drag == Drag::Orbit).then_some(position);
                // a click keeps the highlight it takes; a drag does not
                state.hover = None;
                Some(shader::Action::capture())
            }
            Event::Mouse(mouse::Event::ButtonReleased(_)) => {
                let drag = state.drag.take();
                let press = state.press.take();
                if let (Some((Drag::Orbit, _)), Some(start), Some(at)) = (drag, press, cursor.position_in(bounds)) {
                    if start.distance(at) < 4.0 {
                        return publish(Navigate::Pick { x: at.x, y: at.y, width: bounds.width, height: bounds.height });
                    }
                }
                if drag.is_some() && self.hover {
                    // after orbiting or panning, what is under the cursor changed
                    state.hover = cursor.position_in(bounds);
                    return Some(shader::Action::request_redraw().and_capture());
                }
                drag.map(|_| shader::Action::capture())
            }
            Event::Mouse(mouse::Event::CursorLeft) => hover_off(state),
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.drag.is_none() => {
                if !self.hover {
                    return None;
                }
                match cursor.position_in(bounds) {
                    Some(at) => {
                        state.hover = Some(at);
                        Some(shader::Action::request_redraw())
                    }
                    None => hover_off(state),
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let (drag, last) = state.drag?;
                // Track outside the bounds too while dragging
                let position = cursor.position()?;
                let position = Point::new(position.x - bounds.x, position.y - bounds.y);
                // a click jitters a pixel or two: orbit only once the cursor really moves
                if let Some(start) = state.press {
                    if start.distance(position) < 4.0 {
                        return Some(shader::Action::capture());
                    }
                    state.press = None;
                }
                let (dx, dy) = (position.x - last.x, position.y - last.y);
                state.drag = Some((drag, position));
                match drag {
                    Drag::Orbit => publish(Navigate::Orbit { dx, dy }),
                    Drag::Pan => publish(Navigate::Pan { dx, dy, height: bounds.height }),
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let at = cursor.position_in(bounds)?;
                if self.hover {
                    // the view changes under the cursor: find what is there again
                    state.hover = Some(at);
                }
                let steps = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / 60.0,
                };
                publish(Navigate::Zoom { steps, x: at.x, y: at.y, width: bounds.width, height: bounds.height })
            }
            _ => None,
        }
    }

    fn draw(&self, _state: &State, _cursor: mouse::Cursor, _bounds: Rectangle) -> ScenePrimitive {
        ScenePrimitive {
            model: self.model.cloned(),
            visible: self.visible.to_vec(),
            selection: self.selection.to_vec(),
            camera: self.camera,
            settings: self.settings,
            explode: self.explode.cloned(),
            offsets: self.offsets.clone(),
        }
    }

    fn mouse_interaction(&self, state: &State, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        match state.drag {
            Some((Drag::Orbit, _)) => mouse::Interaction::Grabbing,
            Some((Drag::Pan, _)) => mouse::Interaction::Move,
            None if cursor.is_over(bounds) && self.hover => mouse::Interaction::Crosshair,
            None if cursor.is_over(bounds) => mouse::Interaction::Grab,
            None => mouse::Interaction::default(),
        }
    }
}
