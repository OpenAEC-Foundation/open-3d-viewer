//! The 3D viewport: an iced `shader` widget program plus mouse navigation.
//!
//! Left drag orbits, right or middle drag pans (also Shift + left drag), the wheel zooms.
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
    Zoom { steps: f32 },
    /// The viewport size changed (logical pixels), for fit-to-view.
    Resized { width: f32, height: f32 },
}

pub struct Viewport3D<'a> {
    pub model: Option<&'a Arc<Model>>,
    pub visible: &'a [bool],
    pub camera: Camera,
    pub settings: RenderSettings,
}

#[derive(Default)]
pub struct State {
    drag: Option<(Drag, Point)>,
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
                    mouse::Button::Right | mouse::Button::Middle => Drag::Pan,
                    _ => return None,
                };
                state.drag = Some((drag, position));
                Some(shader::Action::capture())
            }
            Event::Mouse(mouse::Event::ButtonReleased(_)) => {
                state.drag.take().map(|_| shader::Action::capture())
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let (drag, last) = state.drag?;
                // Track outside the bounds too while dragging
                let position = cursor.position()?;
                let position = Point::new(position.x - bounds.x, position.y - bounds.y);
                let (dx, dy) = (position.x - last.x, position.y - last.y);
                state.drag = Some((drag, position));
                match drag {
                    Drag::Orbit => publish(Navigate::Orbit { dx, dy }),
                    Drag::Pan => publish(Navigate::Pan { dx, dy, height: bounds.height }),
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                cursor.position_in(bounds)?;
                let steps = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / 60.0,
                };
                publish(Navigate::Zoom { steps })
            }
            _ => None,
        }
    }

    fn draw(&self, _state: &State, _cursor: mouse::Cursor, _bounds: Rectangle) -> ScenePrimitive {
        ScenePrimitive {
            model: self.model.cloned(),
            visible: self.visible.to_vec(),
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
