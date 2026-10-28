//! The 3D viewport: an iced `shader` widget program plus mouse navigation.
//!
//! As Revit: middle drag pans, Shift + middle drag orbits, the wheel zooms towards the cursor.
//! Also left drag orbits, Shift + left or right drag pans, and a left click (press and release on
//! the same spot) picks.
//! The camera lives in the application state; the widget reports navigation as messages.

pub mod camera;
pub mod render;

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
    /// The viewport size changed (logical pixels), for fit-to-view.
    Resized { width: f32, height: f32 },
    /// Left click at (x, y) in the viewport (logical pixels).
    Pick { x: f32, y: f32, width: f32, height: f32 },
}

pub struct Viewport3D<'a> {
    pub model: Option<&'a Arc<Model>>,
    pub visible: &'a [bool],
    /// Highlighted index ranges: (layer, first index, index count).
    pub selection: &'a [(usize, u32, u32)],
    pub camera: Camera,
    pub settings: RenderSettings,
}

#[derive(Default)]
pub struct State {
    drag: Option<(Drag, Point)>,
    /// Where the left button went down, for click detection.
    press: Option<Point>,
    shift: bool,
    size: (f32, f32),
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

        match event {
            Event::Window(iced::window::Event::RedrawRequested(_)) => {
                if state.size == (bounds.width, bounds.height) {
                    return None;
                }
                state.size = (bounds.width, bounds.height);
                Some(shader::Action::publish(Message::from(Navigate::Resized {
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
                drag.map(|_| shader::Action::capture())
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
        }
    }

    fn mouse_interaction(&self, state: &State, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        match state.drag {
            Some((Drag::Orbit, _)) => mouse::Interaction::Grabbing,
            Some((Drag::Pan, _)) => mouse::Interaction::Move,
            None if cursor.is_over(bounds) => mouse::Interaction::Grab,
            None => mouse::Interaction::default(),
        }
    }
}
