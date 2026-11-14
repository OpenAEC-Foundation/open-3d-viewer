//! The 3D viewport: an iced `shader` widget program plus mouse navigation.
//!
//! As Revit: middle drag pans, Shift + middle drag orbits, the wheel zooms towards the cursor.
//! Also left drag orbits, Shift + left or right drag pans, a left click (press and release on
//! the same spot) picks, and a right click opens the menu of what is selected. While measuring, the cursor position is reported once per frame as it
//! moves, so the app can highlight what a click would pick.
//!
//! On a touch screen (phones and tablets, through the web app): one finger orbits, two fingers
//! pinch to zoom and move together to pan, a tap picks, and a long press (half a second) before
//! dragging pans, as a right drag does with the mouse; a long press without dragging opens the
//! menu, as a right click does.
//! The camera lives in the application state; the widget reports navigation as messages.

pub mod box_select;
pub mod camera;
pub mod chunks;
pub mod render;
pub mod structural;

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
/// How long a finger may stay silent before a new finger forgets it: its lift may never come
/// (taken by a widget above, or by a dialog that opened meanwhile), and a stale finger would turn
/// every later touch into a pinch.
const STALE: Duration = Duration::from_secs(2);

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
    /// Left click at (x, y) in the viewport (logical pixels); `add`: with Ctrl, to add to the
    /// selection.
    Pick { x: f32, y: f32, width: f32, height: f32, add: bool },
    /// Ctrl + left drag: the box from (x0, y0) to the cursor (x1, y1) in a viewport of width ×
    /// height; `done` when the button is released.
    Box { x0: f32, y0: f32, x1: f32, y1: f32, width: f32, height: f32, done: bool },
    /// Right click (or a long press) at (x, y) in a viewport of width × height: the menu of the
    /// selection.
    Context { x: f32, y: f32, width: f32, height: f32 },
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
    pub camera: Camera,
    pub settings: RenderSettings,
    /// The exploded view: the units of the model and the offset (world) of each.
    pub explode: Option<&'a Arc<ExplodeIndex>>,
    pub offsets: &'a Arc<Vec<[f32; 4]>>,
    /// How every unit looks otherwise than its material (a view's overrides).
    pub looks: &'a Arc<Vec<[f32; 4]>>,
    /// Report where the cursor is ([`Navigate::Hover`]): while measuring, and over structural
    /// models (their hover highlight).
    pub hover: bool,
    /// Show the crosshair cursor (measuring) instead of the hand.
    pub crosshair: bool,
    /// Structural instances drawn again on top: the selection and the hover.
    pub structural_selected: &'a Arc<crate::structural::display::Marks>,
    pub structural_hovered: &'a Arc<crate::structural::display::Marks>,
    /// The structural instances of the source models shown (`None`: all are).
    pub structural_shown: Option<&'a Arc<crate::structural::display::Marks>>,
    /// The datum lines where the view shown draws them (`None`: the model's grids).
    pub datum: Option<Arc<Vec<render::DatumSegment>>>,
    /// The lines drawn over the model with their colours: the underlays', the views' in 3D (world).
    pub overlay: Arc<Vec<crate::views3d::Colored>>,
    /// The cut faces of the section box and plane in their hatches.
    pub caps: Arc<crate::caps::CutFaces>,
}

#[derive(Default)]
pub struct State {
    drag: Option<(Drag, Point)>,
    /// Where the left button went down, for click detection.
    press: Option<Point>,
    /// Where the right button went down, while it has not dragged: a right click.
    right_press: Option<Point>,
    shift: bool,
    /// Ctrl (Cmd on macOS) is held: a click adds to the selection.
    command: bool,
    bounds: Rectangle,
    /// Where the cursor moved to since the last frame, reported on the next frame.
    hover: Option<Point>,
    /// The camera of the frame before, and whether this frame's is another: a moving view is
    /// drawn without what is too small to show, and once more in full when it stops.
    seen: Option<Camera>,
    moving: bool,
    /// A hover was reported and not yet switched off.
    hovering: bool,
    /// The fingers on the viewport: id, position (in the viewport) and time of the last report.
    fingers: Vec<(touch::Finger, Point, Instant)>,
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
    /// Ctrl + left: a selection box from the point the button went down.
    Box,
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
                let moved = state.seen.is_some_and(|seen| seen != self.camera);
                state.seen = Some(self.camera);
                state.moving = moved;
                if moved && state.bounds == bounds && (state.hover.is_none() || !self.hover || state.drag.is_some()) {
                    // one more frame after this one: the next while it moves, the full one after
                    return Some(shader::Action::request_redraw());
                }
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
                state.command = m.command();
                None
            }
            Event::Mouse(mouse::Event::ButtonPressed(button)) => {
                let position = cursor.position_in(bounds)?;
                let drag = match button {
                    mouse::Button::Left if state.command => Drag::Box,
                    mouse::Button::Left if state.shift => Drag::Pan,
                    mouse::Button::Left => Drag::Orbit,
                    mouse::Button::Middle if state.shift => Drag::Orbit,
                    mouse::Button::Right | mouse::Button::Middle => Drag::Pan,
                    _ => return None,
                };
                state.drag = Some((drag, position));
                state.press = (*button == mouse::Button::Left && matches!(drag, Drag::Orbit | Drag::Box)).then_some(position);
                state.right_press = (*button == mouse::Button::Right).then_some(position);
                // a click keeps the highlight it takes; a drag does not
                state.hover = None;
                Some(shader::Action::capture())
            }
            Event::Mouse(mouse::Event::ButtonReleased(_)) => {
                let drag = state.drag.take();
                let press = state.press.take();
                if let (Some(start), Some(at)) = (state.right_press.take(), cursor.position_in(bounds)) {
                    if start.distance(at) < 4.0 {
                        return publish(Navigate::Context { x: at.x, y: at.y, width: bounds.width, height: bounds.height });
                    }
                }
                if let (Some((Drag::Orbit | Drag::Box, _)), Some(start), Some(at)) = (drag, press, cursor.position_in(bounds)) {
                    if start.distance(at) < 4.0 {
                        return publish(Navigate::Pick { x: at.x, y: at.y, width: bounds.width, height: bounds.height, add: state.command });
                    }
                }
                if let (Some((Drag::Box, from)), Some(at)) = (drag, cursor.position()) {
                    let (x1, y1) = (at.x - bounds.x, at.y - bounds.y);
                    return publish(Navigate::Box { x0: from.x, y0: from.y, x1, y1, width: bounds.width, height: bounds.height, done: true });
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
                // a right click that moves is a pan
                if state.right_press.is_some_and(|start| start.distance(position) >= 4.0) {
                    state.right_press = None;
                }
                // a click jitters a pixel or two: orbit only once the cursor really moves
                if let Some(start) = state.press {
                    if start.distance(position) < 4.0 {
                        return Some(shader::Action::capture());
                    }
                    state.press = None;
                }
                if drag == Drag::Box {
                    // the box keeps where it started
                    return publish(Navigate::Box { x0: last.x, y0: last.y, x1: position.x, y1: position.y, width: bounds.width, height: bounds.height, done: false });
                }
                let (dx, dy) = (position.x - last.x, position.y - last.y);
                state.drag = Some((drag, position));
                match drag {
                    Drag::Orbit => publish(Navigate::Orbit { dx, dy }),
                    Drag::Pan | Drag::Box => publish(Navigate::Pan { dx, dy, height: bounds.height }),
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
            Event::Touch(event) => touch(state, *event, bounds, Instant::now()),
            // the fingers' lifts go elsewhere while the window is in the background
            Event::Window(iced::window::Event::Focused | iced::window::Event::Unfocused) => {
                state.fingers.clear();
                state.gesture = None;
                None
            }
            _ => None,
        }
    }

    fn draw(&self, state: &State, _cursor: mouse::Cursor, _bounds: Rectangle) -> ScenePrimitive {
        ScenePrimitive {
            model: self.model.cloned(),
            visible: self.visible.to_vec(),
            camera: self.camera,
            settings: self.settings,
            explode: self.explode.cloned(),
            offsets: self.offsets.clone(),
            looks: self.looks.clone(),
            structural_selected: self.structural_selected.clone(),
            structural_hovered: self.structural_hovered.clone(),
            structural_shown: self.structural_shown.cloned(),
            datum: self.datum.clone(),
            overlay: self.overlay.clone(),
            caps: self.caps.clone(),
            moving: state.moving,
        }
    }

    fn mouse_interaction(&self, state: &State, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        match state.drag {
            Some((Drag::Orbit, _)) => mouse::Interaction::Grabbing,
            Some((Drag::Pan, _)) => mouse::Interaction::Move,
            Some((Drag::Box, _)) => mouse::Interaction::Crosshair,
            None if cursor.is_over(bounds) && self.crosshair => mouse::Interaction::Crosshair,
            None => mouse::Interaction::default(),
        }
    }
}

/// One finger orbits (or pans after a long press), two fingers pinch and pan, a tap picks.
fn touch<Message: From<Navigate>>(state: &mut State, event: touch::Event, bounds: Rectangle, now: Instant) -> Option<shader::Action<Message>> {
    let publish = |n: Navigate| Some(shader::Action::publish(Message::from(n)).and_capture());
    let local = |p: Point| Point::new(p.x - bounds.x, p.y - bounds.y);
    let held = |since: Instant| now.saturating_duration_since(since);
    match event {
        touch::Event::FingerPressed { id, position } => {
            state.fingers.retain(|(f, _, seen)| *f != id && held(*seen) < STALE);
            // a gesture starts inside the viewport; further fingers join it wherever they land
            if state.fingers.is_empty() && !bounds.contains(position) {
                state.gesture = None;
                return None;
            }
            let at = local(position);
            state.fingers.push((id, at, now));
            state.hover = None;
            state.gesture = Some(match state.fingers.len() {
                1 => Gesture::Press { at, since: now },
                _ => Gesture::Pinch,
            });
            Some(shader::Action::capture())
        }
        touch::Event::FingerMoved { id, position } => {
            let index = state.fingers.iter().position(|(f, ..)| *f == id)?;
            let at = local(position);
            state.fingers[index].2 = now;
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
                    let drag = if held(since) >= LONG_PRESS { Gesture::Pan } else { Gesture::Orbit };
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
            let index = state.fingers.iter().position(|(f, ..)| *f == id)?;
            state.fingers.remove(index);
            if !state.fingers.is_empty() {
                return Some(shader::Action::capture());
            }
            let gesture = state.gesture.take();
            let lifted = matches!(event, touch::Event::FingerLifted { .. });
            match gesture {
                // a short tap picks, as a click; a long press without moving opens the menu, as a
                // right click
                Some(Gesture::Press { at, since }) if lifted && at.distance(local(position)) < TAP_SLOP => {
                    if held(since) < LONG_PRESS {
                        publish(Navigate::Pick { x: at.x, y: at.y, width: bounds.width, height: bounds.height, add: state.command })
                    } else {
                        publish(Navigate::Context { x: at.x, y: at.y, width: bounds.width, height: bounds.height })
                    }
                }
                _ => Some(shader::Action::capture()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds() -> Rectangle {
        Rectangle::new(Point::new(100.0, 50.0), iced::Size::new(400.0, 300.0))
    }

    fn send(state: &mut State, event: touch::Event, now: Instant) -> Option<Navigate> {
        touch::<Navigate>(state, event, bounds(), now)?.into_inner().0
    }

    fn press(id: u64, x: f32, y: f32) -> touch::Event {
        touch::Event::FingerPressed { id: touch::Finger(id), position: Point::new(x, y) }
    }
    fn moved(id: u64, x: f32, y: f32) -> touch::Event {
        touch::Event::FingerMoved { id: touch::Finger(id), position: Point::new(x, y) }
    }
    fn lifted(id: u64, x: f32, y: f32) -> touch::Event {
        touch::Event::FingerLifted { id: touch::Finger(id), position: Point::new(x, y) }
    }

    /// A mouse event at window point `at`, through the viewport program.
    fn mouse_event(state: &mut State, event: mouse::Event, at: Point) -> Option<Navigate> {
        let offsets = Arc::default();
        let marks = Arc::default();
        let settings = RenderSettings {
            mode: render::RenderMode::Solid,
            grid: false,
            background: [1.0; 3],
            grid_minor: [0.0; 4],
            grid_major: [0.0; 4],
            model_matrix: glam::Mat4::IDENTITY,
            section: None,
            plane: None,
            caps: false,
            grids: false,
            ambient: false,
            bounds: None,
        };
        let view = Viewport3D {
            model: None,
            visible: &[],
            camera: Camera::default(),
            settings,
            explode: None,
            offsets: &offsets,
            looks: &offsets,
            hover: false,
            crosshair: false,
            structural_selected: &marks,
            structural_hovered: &marks,
            structural_shown: None,
            datum: None,
            overlay: Default::default(),
            caps: Default::default(),
        };
        shader::Program::<Navigate>::update(&view, state, &Event::Mouse(event), bounds(), mouse::Cursor::Available(at))?.into_inner().0
    }

    #[test]
    fn a_right_click_opens_the_menu_and_a_right_drag_pans() {
        let mut state = State::default();
        let (down, up) = (mouse::Event::ButtonPressed(mouse::Button::Right), mouse::Event::ButtonReleased(mouse::Button::Right));
        assert!(mouse_event(&mut state, down, Point::new(200.0, 150.0)).is_none());
        // it opens where the button comes up
        let menu = mouse_event(&mut state, up, Point::new(201.0, 150.0));
        assert!(matches!(menu, Some(Navigate::Context { x, y, .. }) if x == 101.0 && y == 100.0), "{menu:?}");
        // a drag pans and opens nothing
        mouse_event(&mut state, down, Point::new(200.0, 150.0));
        let moved = mouse::Event::CursorMoved { position: Point::new(230.0, 150.0) };
        assert!(matches!(mouse_event(&mut state, moved, Point::new(230.0, 150.0)), Some(Navigate::Pan { dx, .. }) if dx == 30.0));
        assert!(!matches!(mouse_event(&mut state, up, Point::new(230.0, 150.0)), Some(Navigate::Context { .. })));
        // a left click still picks
        mouse_event(&mut state, mouse::Event::ButtonPressed(mouse::Button::Left), Point::new(200.0, 150.0));
        let pick = mouse_event(&mut state, mouse::Event::ButtonReleased(mouse::Button::Left), Point::new(200.0, 150.0));
        assert!(matches!(pick, Some(Navigate::Pick { .. })), "{pick:?}");
    }

    #[test]
    fn a_long_press_opens_the_menu() {
        let mut state = State::default();
        let t = Instant::now();
        send(&mut state, press(1, 200.0, 150.0), t);
        let menu = send(&mut state, lifted(1, 202.0, 151.0), t + LONG_PRESS + Duration::from_millis(50));
        assert!(matches!(menu, Some(Navigate::Context { x, y, .. }) if x == 100.0 && y == 100.0), "{menu:?}");
    }

    #[test]
    fn a_tap_picks_and_a_drag_orbits() {
        let mut state = State::default();
        let t = Instant::now();
        assert!(send(&mut state, press(1, 200.0, 150.0), t).is_none());
        assert!(matches!(send(&mut state, lifted(1, 201.0, 150.0), t), Some(Navigate::Pick { x, y, .. }) if x == 100.0 && y == 100.0));
        send(&mut state, press(2, 200.0, 150.0), t);
        assert!(matches!(send(&mut state, moved(2, 230.0, 150.0), t), Some(Navigate::Orbit { dx, .. }) if dx == 30.0));
        send(&mut state, lifted(2, 230.0, 150.0), t);
        assert!(state.fingers.is_empty() && state.gesture.is_none());
    }

    #[test]
    fn a_finger_whose_lift_went_elsewhere_is_forgotten() {
        let mut state = State::default();
        let t = Instant::now();
        // finger 1 goes down; its lift never reaches the viewport
        send(&mut state, press(1, 200.0, 150.0), t);
        // soon after, a second finger pinches with it
        send(&mut state, press(2, 300.0, 150.0), t + Duration::from_millis(100));
        assert_eq!(state.gesture, Some(Gesture::Pinch));
        send(&mut state, lifted(2, 300.0, 150.0), t + Duration::from_millis(200));
        // later, one finger orbits and taps again instead of pinching with the lost one
        let later = t + STALE + Duration::from_millis(300);
        send(&mut state, press(3, 200.0, 150.0), later);
        assert_eq!(state.fingers.len(), 1);
        assert!(matches!(send(&mut state, moved(3, 230.0, 150.0), later), Some(Navigate::Orbit { .. })));
        send(&mut state, lifted(3, 230.0, 150.0), later);
        send(&mut state, press(4, 200.0, 150.0), later);
        assert!(matches!(send(&mut state, lifted(4, 200.0, 150.0), later), Some(Navigate::Pick { .. })));
    }

    #[test]
    fn two_fingers_moving_apart_zoom_in() {
        let mut state = State::default();
        let t = Instant::now();
        send(&mut state, press(1, 200.0, 150.0), t);
        send(&mut state, press(2, 300.0, 150.0), t);
        // a finger that keeps reporting is never stale
        let later = t + STALE * 2;
        send(&mut state, moved(1, 200.0, 150.0), later);
        assert!(matches!(send(&mut state, moved(2, 400.0, 150.0), later), Some(Navigate::Pinch { steps, .. }) if steps > 0.0));
        send(&mut state, press(3, 450.0, 150.0), later);
        assert_eq!(state.fingers.len(), 3);
    }
}
