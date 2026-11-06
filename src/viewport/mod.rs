//! The 3D viewport: an iced `shader` widget program plus mouse navigation.
//!
//! As Revit: middle drag pans, Shift + middle drag orbits, the wheel zooms towards the cursor.
//! Also left drag orbits, Shift + left or right drag pans, and a left click (press and release on
//! the same spot) picks. While measuring, the cursor position is reported once per frame as it
//! moves, so the app can highlight what a click would pick.
//!
//! On a touch screen (phones and tablets, through the web app): one finger orbits, two fingers
//! pinch to zoom and move together to pan, a tap picks, and a long press (half a second) before
//! dragging pans, as a right drag does with the mouse.
//! The camera lives in the application state; the widget reports navigation as messages.

pub mod camera;
pub mod render;

use crate::explode::ExplodeIndex;
use crate::model::Model;
use camera::Camera;
use iced::mouse;
use iced::touch;
use iced::widget::shader;
use iced::{Event, Point, Rectangle};
use render::{RenderSettings, ScenePrimitive};
use std::sync::Arc;
use web_time::{Duration, Instant};

/// How long a finger rests before a drag pans instead of orbiting.
const LONG_PRESS: Duration = Duration::from_millis(500);
/// How far a finger may wander and still tap (logical pixels): fingers jitter more than a mouse.
const TAP_SLOP: f32 = 10.0;

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
    /// Two fingers moved: pan by (dx, dy) and zoom `steps` (as wheel steps) about their middle
    /// (x, y), in a viewport of width × height (logical pixels).
    Pinch { dx: f32, dy: f32, steps: f32, x: f32, y: f32, width: f32, height: f32 },
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
    /// The fingers on the viewport: id and position (in the viewport) at the last report.
    fingers: Vec<(touch::Finger, Point)>,
    /// What the fingers do, from the first finger down until the last one lifts.
    gesture: Option<Gesture>,
}

/// A touch gesture.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Gesture {
    /// One finger is down and has not moved yet: a tap, a long press or the start of a drag.
    Press { at: Point, since: Instant },
    Orbit,
    Pan,
    /// Two fingers or more; the gesture ends with them, without orbiting when one lifts first.
    Pinch,
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
            Event::Touch(touch) => self.touch(state, *touch, bounds),
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

impl Viewport3D<'_> {
    /// One finger orbits (or pans after a long press), two fingers pinch and pan, a tap picks.
    fn touch<Message: From<Navigate>>(&self, state: &mut State, event: touch::Event, bounds: Rectangle) -> Option<shader::Action<Message>> {
        let publish = |n: Navigate| Some(shader::Action::publish(Message::from(n)).and_capture());
        let local = |p: Point| Point::new(p.x - bounds.x, p.y - bounds.y);
        match event {
            touch::Event::FingerPressed { id, position } => {
                // a gesture starts inside the viewport; further fingers join it wherever they land
                if state.fingers.is_empty() && !bounds.contains(position) {
                    return None;
                }
                let at = local(position);
                state.fingers.retain(|(f, _)| *f != id);
                state.fingers.push((id, at));
                state.hover = None;
                state.gesture = Some(match state.fingers.len() {
                    1 => Gesture::Press { at, since: Instant::now() },
                    _ => Gesture::Pinch,
                });
                Some(shader::Action::capture())
            }
            touch::Event::FingerMoved { id, position } => {
                let index = state.fingers.iter().position(|(f, _)| *f == id)?;
                let at = local(position);
                let gesture = state.gesture?;
                if gesture == Gesture::Pinch {
                    if state.fingers.len() < 2 || index > 1 {
                        // the fingers that are left wait until all are lifted
                        state.fingers[index].1 = at;
                        return Some(shader::Action::capture());
                    }
                    let (a, b) = (state.fingers[0].1, state.fingers[1].1);
                    state.fingers[index].1 = at;
                    let (c, d) = (state.fingers[0].1, state.fingers[1].1);
                    let (before, after) = (a.distance(b).max(1.0), c.distance(d).max(1.0));
                    let middle = Point::new((c.x + d.x) / 2.0, (c.y + d.y) / 2.0);
                    // wheel steps zoom by 0.85 each: the step count that scales the view as the fingers did
                    let steps = (after / before).ln() / -(0.85f32.ln());
                    return publish(Navigate::Pinch {
                        dx: middle.x - (a.x + b.x) / 2.0,
                        dy: middle.y - (a.y + b.y) / 2.0,
                        steps,
                        x: middle.x,
                        y: middle.y,
                        width: bounds.width,
                        height: bounds.height,
                    });
                }
                let last = state.fingers[index].1;
                let drag = match gesture {
                    Gesture::Press { at: start, since } => {
                        if start.distance(at) < TAP_SLOP {
                            return Some(shader::Action::capture());
                        }
                        let drag = if since.elapsed() >= LONG_PRESS { Gesture::Pan } else { Gesture::Orbit };
                        state.gesture = Some(drag);
                        drag
                    }
                    other => other,
                };
                state.fingers[index].1 = at;
                let (dx, dy) = (at.x - last.x, at.y - last.y);
                match drag {
                    Gesture::Pan => publish(Navigate::Pan { dx, dy, height: bounds.height }),
                    _ => publish(Navigate::Orbit { dx, dy }),
                }
            }
            touch::Event::FingerLifted { id, position } | touch::Event::FingerLost { id, position } => {
                let index = state.fingers.iter().position(|(f, _)| *f == id)?;
                state.fingers.remove(index);
                if !state.fingers.is_empty() {
                    return Some(shader::Action::capture());
                }
                let gesture = state.gesture.take();
                let lifted = matches!(event, touch::Event::FingerLifted { .. });
                match gesture {
                    // a short tap picks, as a click; a long press without moving does nothing
                    Some(Gesture::Press { at, since }) if lifted && since.elapsed() < LONG_PRESS && at.distance(local(position)) < TAP_SLOP => {
                        publish(Navigate::Pick { x: at.x, y: at.y, width: bounds.width, height: bounds.height })
                    }
                    _ => Some(shader::Action::capture()),
                }
            }
        }
    }
}
