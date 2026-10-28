//! The section box, as in Open Pointcloud Studio: a box turned about the vertical that keeps the
//! part of the model inside it. Its faces move with handles in the viewport (X−/X+, Y−/Y+, Z−/Z+)
//! or with the sliders in the properties pane; the handles on its top edges turn it. The renderer
//! clips to it in the fragment shader and fills the cut faces of solids (`scene.wgsl`).
//!
//! The handle geometry and the turn by dragging follow Pointcloud's `section_handle_world`,
//! `section_handle_delta` and `section_turn_delta` (LGPL-3.0-or-later).

use crate::model::Bounds;
use glam::{Mat4, Vec3};
use iced::widget::canvas::{self, Frame, Path};
use iced::{mouse, Color, Point, Rectangle};

/// Handles react this close to the pointer (logical pixels).
const HANDLE_REACH: f32 = 10.0;

pub use openaec_model::section::{SectionBox, SectionPlane};

/// What the section overlay reports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SectionEvent {
    /// Move a face along its axis (world units).
    Move { axis: usize, is_min: bool, delta: f32 },
    /// Turn about the vertical (radians).
    Turn(f32),
}

/// Screen position (logical pixels in the viewport) of a world point, if it is in front of the
/// camera.
fn project(view_proj: &Mat4, size: (f32, f32), world: Vec3) -> Option<Point> {
    let clip = *view_proj * world.extend(1.0);
    if clip.w <= 1e-6 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    Some(Point::new((ndc.x + 1.0) * 0.5 * size.0, (1.0 - ndc.y) * 0.5 * size.1))
}

/// The face handle nearest the pointer, within reach.
pub fn handle_at(section: &SectionBox, view_proj: &Mat4, size: (f32, f32), pointer: Point) -> Option<(usize, bool)> {
    let mut nearest: Option<(usize, bool, f32)> = None;
    for axis in 0..3 {
        for is_min in [true, false] {
            let Some(p) = project(view_proj, size, section.handle(axis, is_min)) else { continue };
            let d = p.distance(pointer);
            if d <= HANDLE_REACH && nearest.is_none_or(|n| d < n.2) {
                nearest = Some((axis, is_min, d));
            }
        }
    }
    nearest.map(|(a, m, _)| (a, m))
}

fn turn_handle_at(section: &SectionBox, view_proj: &Mat4, size: (f32, f32), pointer: Point) -> bool {
    section
        .turn_handles()
        .into_iter()
        .any(|w| project(view_proj, size, w).is_some_and(|p| p.distance(pointer) <= HANDLE_REACH))
}

/// How far (world units) a drag of `movement` pixels moves a face handle along its axis: the
/// part of the movement along the projected axis.
pub fn handle_delta(
    section: &SectionBox,
    view_proj: &Mat4,
    size: (f32, f32),
    axis: usize,
    is_min: bool,
    movement: (f32, f32),
) -> Option<f32> {
    let world = section.handle(axis, is_min);
    let step = (section.max - section.min).max_element().max(MIN_SIZE) * 0.01;
    let before = project(view_proj, size, world)?;
    let after = project(view_proj, size, world + section.axis(axis) * step)?;
    let (dx, dy) = (after.x - before.x, after.y - before.y);
    let squared = dx * dx + dy * dy;
    // seen end-on the axis gives no direction
    (squared > 0.25).then(|| (movement.0 * dx + movement.1 * dy) / squared * step)
}

/// The turn about the vertical through the middle of the top of the box that takes the pointer
/// from `before` to `after` round it, radians counter-clockwise from above.
pub fn turn_delta(section: &SectionBox, view_proj: &Mat4, size: (f32, f32), before: Point, after: Point) -> Option<f32> {
    let mut top = (section.min + section.max) * 0.5;
    top.z = section.max.z;
    let middle = section.to_world(top);
    let c = project(view_proj, size, middle)?;
    let reach = (section.max.x - section.min.x).max(section.max.y - section.min.y).max(MIN_SIZE);
    // the screen near the middle as a picture of the horizontal plane through it
    let east = project(view_proj, size, middle + Vec3::X * reach)?;
    let north = project(view_proj, size, middle + Vec3::Y * reach)?;
    let (ex, ey) = (east.x - c.x, east.y - c.y);
    let (nx, ny) = (north.x - c.x, north.y - c.y);
    let det = ex * ny - ey * nx;
    if det.abs() < 1.0 {
        return None;
    }
    let angle = |p: Point| {
        let (vx, vy) = (p.x - c.x, p.y - c.y);
        let along_east = (vx * ny - vy * nx) / det;
        let along_north = (ex * vy - ey * vx) / det;
        along_north.atan2(along_east)
    };
    let pi = std::f32::consts::PI;
    Some((angle(after) - angle(before) + pi).rem_euclid(std::f32::consts::TAU) - pi)
}

/// The box with its handles as a canvas overlay over the viewport. Outside the handles it lets
/// every event through to the viewport.
pub struct SectionOverlay {
    pub section: SectionBox,
    /// World to clip space of the current camera.
    pub view_proj: Mat4,
}

#[derive(Default)]
pub struct OverlayState {
    hover: Option<(usize, bool)>,
    hover_turn: bool,
    drag: Option<(Drag, Point)>,
}

#[derive(Clone, Copy)]
enum Drag {
    Face(usize, bool),
    Turn,
}

const AMBER: Color = Color { r: 217.0 / 255.0, g: 119.0 / 255.0, b: 6.0 / 255.0, a: 1.0 };
const AMBER_LIGHT: Color = Color { r: 245.0 / 255.0, g: 158.0 / 255.0, b: 11.0 / 255.0, a: 1.0 };

impl<Message> canvas::Program<Message> for SectionOverlay
where
    Message: From<SectionEvent>,
{
    type State = OverlayState;

    fn update(
        &self,
        state: &mut OverlayState,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let size = (bounds.width, bounds.height);
        let local = |p: Point| Point::new(p.x - bounds.x, p.y - bounds.y);
        match event {
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let Some((drag, last)) = state.drag {
                    let now = local(cursor.position()?);
                    state.drag = Some((drag, now));
                    let event = match drag {
                        Drag::Face(axis, is_min) => SectionEvent::Move {
                            axis,
                            is_min,
                            delta: handle_delta(&self.section, &self.view_proj, size, axis, is_min, (now.x - last.x, now.y - last.y))?,
                        },
                        Drag::Turn => SectionEvent::Turn(turn_delta(&self.section, &self.view_proj, size, last, now)?),
                    };
                    return Some(canvas::Action::publish(Message::from(event)).and_capture());
                }
                let pointer = cursor.position_in(bounds);
                let hover = pointer.and_then(|p| handle_at(&self.section, &self.view_proj, size, p));
                let hover_turn = hover.is_none() && pointer.is_some_and(|p| turn_handle_at(&self.section, &self.view_proj, size, p));
                if (hover, hover_turn) != (state.hover, state.hover_turn) {
                    state.hover = hover;
                    state.hover_turn = hover_turn;
                    return Some(canvas::Action::request_redraw());
                }
                None
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let p = cursor.position_in(bounds)?;
                let drag = match handle_at(&self.section, &self.view_proj, size, p) {
                    Some((axis, is_min)) => Drag::Face(axis, is_min),
                    None if turn_handle_at(&self.section, &self.view_proj, size, p) => Drag::Turn,
                    None => return None,
                };
                state.drag = Some((drag, p));
                Some(canvas::Action::capture())
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.drag.take().map(|_| canvas::Action::capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &OverlayState,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let size = (bounds.width, bounds.height);
        let at = |w: Vec3| project(&self.view_proj, size, w);
        let corners = self.section.corners();
        let edges = Path::new(|path| {
            for (a, b) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
                if let (Some(a), Some(b)) = (at(corners[a]), at(corners[b])) {
                    path.move_to(a);
                    path.line_to(b);
                }
            }
        });
        frame.stroke(&edges, canvas::Stroke::default().with_color(AMBER).with_width(1.4));

        let dragged = match state.drag {
            Some((Drag::Face(a, m), _)) => Some((a, m)),
            _ => None,
        };
        for (axis, name) in ["X", "Y", "Z"].into_iter().enumerate() {
            for is_min in [true, false] {
                let Some(p) = at(self.section.handle(axis, is_min)) else { continue };
                if p.x < -10.0 || p.y < -10.0 || p.x > bounds.width + 10.0 || p.y > bounds.height + 10.0 {
                    continue;
                }
                let hot = state.hover == Some((axis, is_min)) || dragged == Some((axis, is_min));
                let circle = Path::circle(p, if hot { 7.0 } else { 5.0 });
                frame.fill(&circle, if hot { AMBER_LIGHT } else { Color::from_rgb8(83, 60, 38) });
                frame.stroke(&circle, canvas::Stroke::default().with_color(AMBER_LIGHT).with_width(1.3));
                frame.fill_text(canvas::Text {
                    content: format!("{name}{}", if is_min { "−" } else { "+" }),
                    position: Point::new(p.x + 8.0, p.y - 12.0),
                    size: iced::Pixels(10.0),
                    color: AMBER,
                    font: openaec_ui::fonts::MEDIUM,
                    ..canvas::Text::default()
                });
            }
        }
        // turning handles: a curved arrow round the point
        let hot_turn = state.hover_turn || matches!(state.drag, Some((Drag::Turn, _)));
        for w in self.section.turn_handles() {
            let Some(p) = at(w) else { continue };
            let r = if hot_turn { 7.5 } else { 6.0 };
            let (start, end) = (0.6f32, 5.2f32);
            let arc = Path::new(|path| {
                path.arc(canvas::path::Arc { center: p, radius: r, start_angle: iced::Radians(start), end_angle: iced::Radians(end) });
                let tip = Point::new(p.x + r * end.cos(), p.y + r * end.sin());
                path.move_to(tip);
                path.line_to(Point::new(tip.x + 3.5, tip.y - 1.0));
                path.move_to(tip);
                path.line_to(Point::new(tip.x + 0.5, tip.y + 3.5));
            });
            frame.stroke(
                &arc,
                canvas::Stroke::default()
                    .with_color(if hot_turn { AMBER_LIGHT } else { AMBER })
                    .with_width(if hot_turn { 2.0 } else { 1.5 })
                    .with_line_cap(canvas::LineCap::Round),
            );
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, state: &OverlayState, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if state.drag.is_some() {
            return mouse::Interaction::Grabbing;
        }
        let size = (bounds.width, bounds.height);
        match cursor.position_in(bounds) {
            Some(p) if handle_at(&self.section, &self.view_proj, size, p).is_some() => mouse::Interaction::Grab,
            Some(p) if turn_handle_at(&self.section, &self.view_proj, size, p) => mouse::Interaction::Grab,
            // let the viewport below handle the cursor
            _ => mouse::Interaction::None,
        }
    }
}

// ---- section plane ---------------------------------------------------------------------------

/// What the section plane overlay reports: a move along the normal (world units).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaneMove(pub f32);

/// How far (world units) a drag of `movement` pixels moves the plane: the part of the movement
/// along the projected normal; seen end-on, upwards on screen moves it towards the viewer.
pub fn plane_delta(plane: &SectionPlane, world: &Bounds, view_proj: &Mat4, size: (f32, f32), movement: (f32, f32)) -> Option<f32> {
    let at = plane.handle(world);
    let step = (Vec3::from_array(world.max) - Vec3::from_array(world.min)).max_element().max(MIN_SIZE) * 0.01;
    let before = project(view_proj, size, at)?;
    let after = project(view_proj, size, at + plane.normal * step)?;
    let (dx, dy) = (after.x - before.x, after.y - before.y);
    let squared = dx * dx + dy * dy;
    if squared > 0.25 {
        return Some((movement.0 * dx + movement.1 * dy) / squared * step);
    }
    // end-on: the depth of a point a step along the normal tells which way faces the viewer
    let towards = (*view_proj * (at + plane.normal * step).extend(1.0)).w < (*view_proj * at.extend(1.0)).w;
    Some(if towards { -movement.1 } else { movement.1 } * step / 10.0)
}

/// The section plane as a canvas overlay: its outline round the model and a handle with an arrow
/// along the normal (towards the side that is cut away) that moves it. Outside the handle every
/// event goes through to the viewport.
pub struct PlaneOverlay {
    pub plane: SectionPlane,
    pub world: Bounds,
    /// World to clip space of the current camera.
    pub view_proj: Mat4,
}

#[derive(Default)]
pub struct PlaneState {
    hover: bool,
    drag: Option<Point>,
}

impl PlaneOverlay {
    fn handle_at(&self, size: (f32, f32), pointer: Point) -> bool {
        project(&self.view_proj, size, self.plane.handle(&self.world)).is_some_and(|p| p.distance(pointer) <= HANDLE_REACH)
    }
}

impl<Message> canvas::Program<Message> for PlaneOverlay
where
    Message: From<PlaneMove>,
{
    type State = PlaneState;

    fn update(&self, state: &mut PlaneState, event: &iced::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<canvas::Action<Message>> {
        let size = (bounds.width, bounds.height);
        match event {
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let Some(last) = state.drag {
                    let p = cursor.position()?;
                    let now = Point::new(p.x - bounds.x, p.y - bounds.y);
                    state.drag = Some(now);
                    let delta = plane_delta(&self.plane, &self.world, &self.view_proj, size, (now.x - last.x, now.y - last.y))?;
                    return Some(canvas::Action::publish(Message::from(PlaneMove(delta))).and_capture());
                }
                let hover = cursor.position_in(bounds).is_some_and(|p| self.handle_at(size, p));
                if hover != state.hover {
                    state.hover = hover;
                    return Some(canvas::Action::request_redraw());
                }
                None
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let p = cursor.position_in(bounds)?;
                if !self.handle_at(size, p) {
                    return None;
                }
                state.drag = Some(p);
                Some(canvas::Action::capture())
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.drag.take().map(|_| canvas::Action::capture())
            }
            _ => None,
        }
    }

    fn draw(&self, state: &PlaneState, renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let size = (bounds.width, bounds.height);
        let at = |w: Vec3| project(&self.view_proj, size, w);
        let r = self.plane.rectangle(&self.world);
        let outline = Path::new(|path| {
            for i in 0..4 {
                if let (Some(a), Some(b)) = (at(r[i]), at(r[(i + 1) % 4])) {
                    path.move_to(a);
                    path.line_to(b);
                }
            }
        });
        frame.stroke(&outline, canvas::Stroke::default().with_color(AMBER).with_width(1.4));

        let handle = self.plane.handle(&self.world);
        let Some(p) = at(handle) else { return vec![frame.into_geometry()] };
        let hot = state.hover || state.drag.is_some();
        // the arrow, 40 pixels along the projected normal
        let reach = (Vec3::from_array(self.world.max) - Vec3::from_array(self.world.min)).max_element().max(MIN_SIZE) * 0.01;
        if let Some(q) = at(handle + self.plane.normal * reach) {
            let (dx, dy) = (q.x - p.x, q.y - p.y);
            let len = (dx * dx + dy * dy).sqrt();
            if len > 0.5 {
                let (ux, uy) = (dx / len, dy / len);
                let tip = Point::new(p.x + ux * 40.0, p.y + uy * 40.0);
                let arrow = Path::new(|path| {
                    path.move_to(p);
                    path.line_to(tip);
                    path.move_to(Point::new(tip.x - ux * 8.0 - uy * 5.0, tip.y - uy * 8.0 + ux * 5.0));
                    path.line_to(tip);
                    path.line_to(Point::new(tip.x - ux * 8.0 + uy * 5.0, tip.y - uy * 8.0 - ux * 5.0));
                });
                frame.stroke(
                    &arrow,
                    canvas::Stroke::default()
                        .with_color(if hot { AMBER_LIGHT } else { AMBER })
                        .with_width(if hot { 2.4 } else { 1.8 })
                        .with_line_cap(canvas::LineCap::Round),
                );
            }
        }
        let circle = Path::circle(p, if hot { 7.0 } else { 5.0 });
        frame.fill(&circle, if hot { AMBER_LIGHT } else { Color::from_rgb8(83, 60, 38) });
        frame.stroke(&circle, canvas::Stroke::default().with_color(AMBER_LIGHT).with_width(1.3));
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, state: &PlaneState, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if state.drag.is_some() {
            return mouse::Interaction::Grabbing;
        }
        match cursor.position_in(bounds) {
            Some(p) if self.handle_at((bounds.width, bounds.height), p) => mouse::Interaction::Grab,
            _ => mouse::Interaction::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plane_on_a_face_cuts_the_side_it_faces() {
        let world = Bounds { min: [0.0, 0.0, 0.0], max: [10.0, 10.0, 3.0] };
        // a wall face at x = 4 facing −X (towards a viewer west of it), slightly off
        let mut plane = SectionPlane::on_face(Vec3::new(4.0, 5.0, 1.0), Vec3::new(-1.0, 0.002, 0.0), &world).expect("plane");
        assert_eq!(plane.normal, Vec3::NEG_X, "snapped");
        assert!(plane.cuts(Vec3::new(2.0, 5.0, 1.0)));
        assert!(!plane.cuts(Vec3::new(6.0, 5.0, 1.0)));
        assert!(!plane.cuts(Vec3::new(4.0, 5.0, 1.0)), "the face itself stays");
        plane.flip();
        assert!(plane.cuts(Vec3::new(6.0, 5.0, 1.0)));
        plane.move_by(100.0, &world);
        assert_eq!(plane.offset, plane.range(&world).1);
        let (u, v) = SectionPlane { normal: Vec3::new(0.0, 0.6, 0.8), offset: 0.0 }.basis();
        assert!(u.z.abs() < 1e-6 && v.z > 0.0, "{u} {v}");
    }

    #[test]
    fn a_level_or_upright_plane_becomes_a_face_of_the_box() {
        let world = Bounds { min: [0.0, 0.0, 0.0], max: [10.0, 8.0, 3.0] };
        for plane in [
            SectionPlane { normal: Vec3::NEG_X, offset: -4.0 },
            SectionPlane { normal: Vec3::Y, offset: 5.0 },
            SectionPlane { normal: Vec3::Z, offset: 1.5 },
            SectionPlane { normal: Vec3::NEG_Z, offset: -1.0 },
            // a wall at an angle: the box turns to it
            SectionPlane { normal: Vec3::new(0.6, -0.8, 0.0), offset: 1.0 },
        ] {
            let b = SectionBox::from_plane(&plane, &world).expect("box");
            if plane.normal.x == 0.0 || plane.normal.y == 0.0 {
                assert_eq!(b.turn, 0.0, "{plane:?}");
            }
            // the same cut everywhere in the model
            for (i, j, k) in (0..10).flat_map(|i| (0..8).flat_map(move |j| (0..3).map(move |k| (i, j, k)))) {
                let p = Vec3::new(i as f32, j as f32, k as f32) + 0.37;
                assert_eq!(b.contains(p), !plane.cuts(p), "{plane:?} at {p}");
            }
        }
        // a sloped roof cannot be a face of the box
        assert!(SectionBox::from_plane(&SectionPlane { normal: Vec3::new(0.0, 0.6, 0.8), offset: 1.0 }, &world).is_none());
    }

    fn unit() -> SectionBox {
        SectionBox { min: Vec3::ZERO, max: Vec3::new(4.0, 2.0, 3.0), turn: 0.0, pivot: Vec3::new(2.0, 1.0, 1.5) }
    }

    #[test]
    fn world_and_local_round_trip_when_turned() {
        let mut s = unit();
        s.turn = 0.7;
        let p = Vec3::new(1.3, -2.0, 0.4);
        assert!((s.to_world(s.to_local(p)) - p).length() < 1e-5);
        assert!(s.contains(s.to_world(Vec3::new(1.0, 1.0, 1.0))));
        assert!(!s.contains(s.to_world(Vec3::new(5.0, 1.0, 1.0))));
    }

    #[test]
    fn turning_keeps_the_middle_in_place() {
        let mut s = unit();
        s.pivot = Vec3::new(-10.0, 3.0, 0.0);
        let before = s.to_world((s.min + s.max) * 0.5);
        s.turn_by(0.9);
        let after = s.to_world((s.min + s.max) * 0.5);
        assert!((before - after).length() < 1e-4, "{before} {after}");
        assert!(((s.max - s.min) - Vec3::new(4.0, 2.0, 3.0)).length() < 1e-4);
    }

    #[test]
    fn a_section_crop_becomes_a_turned_box() {
        // a section looking north (+Y): right = +X, up = +Z, view direction = +Y; 10 deep
        let origin = Vec3::new(5.0, 2.0, 0.0);
        let basis = [Vec3::X, Vec3::Z, Vec3::Y];
        let s = SectionBox::from_frame(origin, basis, Vec3::new(-3.0, 0.0, 0.0), Vec3::new(3.0, 4.0, 10.0)).expect("box");
        assert!(s.contains(Vec3::new(5.0, 7.0, 2.0)));
        assert!(!s.contains(Vec3::new(5.0, 1.0, 2.0)), "behind the cut");
        assert!(!s.contains(Vec3::new(9.0, 7.0, 2.0)), "right of the crop");
        // looking west (−X): right = −Y
        let s = SectionBox::from_frame(origin, [Vec3::NEG_Y, Vec3::Z, Vec3::NEG_X], Vec3::new(-1.0, 0.0, 0.0), Vec3::new(1.0, 3.0, 4.0)).expect("box");
        assert!(s.contains(Vec3::new(3.0, 2.5, 1.0)));
        assert!(!s.contains(Vec3::new(6.0, 2.5, 1.0)));
        // a tilted box is refused
        let t = Vec3::new(1.0, 0.0, 1.0).normalize();
        assert!(SectionBox::from_frame(origin, [t, Vec3::Y, Vec3::new(-1.0, 0.0, 1.0).normalize()], Vec3::ZERO, Vec3::ONE).is_none());
    }

    #[test]
    fn faces_keep_a_minimum_depth() {
        let mut s = unit();
        s.move_face(0, false, -10.0);
        assert!(s.max.x > s.min.x);
        s.move_face(2, true, 1.0);
        assert_eq!(s.min.z, 1.0);
    }

    #[test]
    fn dragging_a_handle_follows_the_pointer() {
        // looking down: +X is to the right on screen, +Y up
        let mut cam = crate::viewport::camera::Camera::default();
        cam.set_preset(crate::viewport::camera::ViewPreset::Top);
        cam.target = Vec3::new(2.0, 1.0, 1.5);
        cam.distance = 20.0;
        let vp = cam.view_proj(1.0);
        let s = unit();
        let size = (800.0, 800.0);
        let right = handle_delta(&s, &vp, size, 0, false, (10.0, 0.0)).expect("x");
        assert!(right > 0.0, "{right}");
        let left = handle_delta(&s, &vp, size, 0, true, (-10.0, 0.0)).expect("x min");
        assert!(left < 0.0, "{left}");
        let up = handle_delta(&s, &vp, size, 1, false, (0.0, -10.0)).expect("y");
        assert!(up > 0.0, "{up}");
        // the Z axis is seen end-on from above
        assert!(handle_delta(&s, &vp, size, 2, false, (0.0, -10.0)).is_none());
        // a handle sits where its face middle projects
        let p = project(&vp, size, s.handle(0, false)).unwrap();
        assert_eq!(handle_at(&s, &vp, size, p), Some((0, false)));
    }
}
