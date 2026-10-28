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

/// Faces closer together than this (world units) are not allowed.
const MIN_SIZE: f32 = 1e-3;
/// Handles react this close to the pointer (logical pixels).
const HANDLE_REACH: f32 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionBox {
    /// Limits in the frame of the box: the world turned by −`turn` about the vertical through
    /// `pivot`.
    pub min: Vec3,
    pub max: Vec3,
    /// Turn about the vertical, radians, counter-clockwise seen from above.
    pub turn: f32,
    pub pivot: Vec3,
}

impl SectionBox {
    /// A box just around the bounds (world), not turned.
    pub fn around(bounds: &Bounds) -> Self {
        let min = Vec3::from_array(bounds.min);
        let max = Vec3::from_array(bounds.max);
        let pad = (max - min).max(Vec3::splat(MIN_SIZE)) * 0.01;
        Self { min: min - pad, max: max + pad, turn: 0.0, pivot: (min + max) * 0.5 }
    }

    /// The box `origin + x·basis[0] + y·basis[1] + z·basis[2]` for x, y, z within min..max (a
    /// Revit section or crop box), when one of its axes is vertical (the box turns only about the
    /// vertical).
    pub fn from_frame(origin: Vec3, basis: [Vec3; 3], min: Vec3, max: Vec3) -> Option<Self> {
        let along = basis.iter().position(|b| b.z.abs() < 0.001 && b.length() > 0.5)?;
        let a = basis[along];
        let mut out = SectionBox { min: origin, max: origin, turn: a.y.atan2(a.x), pivot: origin };
        for j in 0..3 {
            let e = out.axis(j);
            let (k, dot) = (0..3)
                .map(|k| (k, basis[k].normalize_or_zero().dot(e)))
                .max_by(|x, y| x.1.abs().total_cmp(&y.1.abs()))?;
            if dot.abs() < 0.999 {
                return None;
            }
            let (lo, hi) = if dot > 0.0 { (min[k], max[k]) } else { (-max[k], -min[k]) };
            out.min[j] = origin[j] + lo;
            out.max[j] = origin[j] + hi;
        }
        Some(out)
    }

    /// The box limited to the model (world bounds) plus a margin: views store far planes of
    /// thousands of metres.
    pub fn clamped_to(mut self, world: &Bounds) -> Self {
        let r = self.local_bounds(world);
        for i in 0..3 {
            let pad = ((r.max[i] - r.min[i]) * 0.02).max(MIN_SIZE);
            let (lo, hi) = (self.min[i].max(r.min[i] - pad), self.max[i].min(r.max[i] + pad));
            if hi - lo >= MIN_SIZE {
                self.min[i] = lo;
                self.max[i] = hi;
            }
        }
        self
    }

    /// Axis-aligned world bounds of the box.
    pub fn world_bounds(&self) -> Bounds {
        let mut b = Bounds::empty();
        for c in self.corners() {
            b.extend(c.to_array());
        }
        b
    }

    pub fn to_world(&self, local: Vec3) -> Vec3 {
        let (s, c) = self.turn.sin_cos();
        let d = local - self.pivot;
        self.pivot + Vec3::new(c * d.x - s * d.y, s * d.x + c * d.y, d.z)
    }

    pub fn to_local(&self, world: Vec3) -> Vec3 {
        let (s, c) = self.turn.sin_cos();
        let d = world - self.pivot;
        self.pivot + Vec3::new(c * d.x + s * d.y, -s * d.x + c * d.y, d.z)
    }

    pub fn contains(&self, world: Vec3) -> bool {
        let p = self.to_local(world);
        let eps = (self.max - self.min).max_element() * 1e-6;
        (0..3).all(|i| p[i] >= self.min[i] - eps && p[i] <= self.max[i] + eps)
    }

    /// The world direction of a box axis.
    pub fn axis(&self, axis: usize) -> Vec3 {
        let (s, c) = self.turn.sin_cos();
        match axis {
            0 => Vec3::new(c, s, 0.0),
            1 => Vec3::new(-s, c, 0.0),
            _ => Vec3::Z,
        }
    }

    pub fn corners(&self) -> [Vec3; 8] {
        std::array::from_fn(|i| {
            let pick = |bit: usize, axis: usize| if i >> bit & 1 == 1 { self.max[axis] } else { self.min[axis] };
            self.to_world(Vec3::new(pick(0, 0), pick(1, 1), pick(2, 2)))
        })
    }

    /// The middle of a face: where its handle is.
    pub fn handle(&self, axis: usize, is_min: bool) -> Vec3 {
        let mut p = (self.min + self.max) * 0.5;
        p[axis] = if is_min { self.min[axis] } else { self.max[axis] };
        self.to_world(p)
    }

    /// The turning handles: the middle of the top edge of each side face.
    pub fn turn_handles(&self) -> [Vec3; 4] {
        let at = |axis: usize, is_min: bool| {
            let mut p = (self.min + self.max) * 0.5;
            p[axis] = if is_min { self.min[axis] } else { self.max[axis] };
            p.z = self.max.z;
            self.to_world(p)
        };
        [at(0, true), at(0, false), at(1, true), at(1, false)]
    }

    /// Moves a face along its axis by `delta` (world units), keeping the box at least
    /// `MIN_SIZE` deep.
    pub fn move_face(&mut self, axis: usize, is_min: bool, delta: f32) {
        if axis > 2 || !delta.is_finite() {
            return;
        }
        if is_min {
            self.min[axis] = (self.min[axis] + delta).min(self.max[axis] - MIN_SIZE);
        } else {
            self.max[axis] = (self.max[axis] + delta).max(self.min[axis] + MIN_SIZE);
        }
    }

    /// Turns the box about the vertical through its middle.
    pub fn turn_by(&mut self, radians: f32) {
        if !radians.is_finite() {
            return;
        }
        let middle = self.to_world((self.min + self.max) * 0.5);
        self.turn = (self.turn + radians + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        let shift = self.to_local(middle) - (self.min + self.max) * 0.5;
        self.min += shift;
        self.max += shift;
    }

    /// The bounds of `world` in the frame of the box (for the sliders).
    pub fn local_bounds(&self, world: &Bounds) -> Bounds {
        let b = SectionBox { min: Vec3::from_array(world.min), max: Vec3::from_array(world.max), turn: 0.0, pivot: Vec3::ZERO };
        let mut out = Bounds::empty();
        for c in b.corners() {
            out.extend(self.to_local(c).to_array());
        }
        out
    }

    /// For the shader: pivot + cos(turn), min + sin(turn), max + 1 (on).
    pub fn uniform(&self) -> [[f32; 4]; 3] {
        let (s, c) = self.turn.sin_cos();
        [
            [self.pivot.x, self.pivot.y, self.pivot.z, c],
            [self.min.x, self.min.y, self.min.z, s],
            [self.max.x, self.max.y, self.max.z, 1.0],
        ]
    }
}

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

/// Components of a picked normal smaller than this (about 0.5°) snap to zero, so walls and floors
/// give clean orthogonal sections.
const SNAP: f32 = 0.0087;

/// A single cutting plane, placed by clicking a face of the model (any orientation, e.g. a sloped
/// roof): everything on the side the face points to is cut away. As the web viewer's
/// `SectionPlane`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionPlane {
    /// Unit normal pointing to the side that is cut away (world).
    pub normal: Vec3,
    /// Position along the normal: points with `normal · p > offset` are cut away.
    pub offset: f32,
}

impl SectionPlane {
    /// The plane on a face at `point` whose normal faces the viewer, just in front of the face so
    /// that the face itself stays.
    pub fn on_face(point: Vec3, normal: Vec3, world: &Bounds) -> Option<Self> {
        let mut n = normal.normalize_or_zero();
        for c in [&mut n.x, &mut n.y, &mut n.z] {
            if c.abs() < SNAP {
                *c = 0.0;
            }
        }
        let normal = n.try_normalize()?;
        let diag = (Vec3::from_array(world.max) - Vec3::from_array(world.min)).length();
        Some(Self { normal, offset: normal.dot(point) + (diag * 1e-4).max(1e-4) })
    }

    /// Cut away the other side instead.
    pub fn flip(&mut self) {
        self.normal = -self.normal;
        self.offset = -self.offset;
    }

    pub fn cuts(&self, world: Vec3) -> bool {
        self.normal.dot(world) > self.offset
    }

    /// The offsets at which the plane just touches the model (world bounds), lowest first.
    pub fn range(&self, world: &Bounds) -> (f32, f32) {
        SectionBox::around(world).corners().iter().fold((f32::MAX, f32::MIN), |(lo, hi), c| {
            let d = self.normal.dot(*c);
            (lo.min(d), hi.max(d))
        })
    }

    /// Move along the normal, kept within (just outside) the model.
    pub fn move_by(&mut self, delta: f32, world: &Bounds) {
        let (lo, hi) = self.range(world);
        self.offset = (self.offset + delta).clamp(lo, hi);
    }

    /// Axes in the plane: `u` horizontal and `v` upwards for vertical and sloped planes.
    pub fn basis(&self) -> (Vec3, Vec3) {
        let up = if self.normal.z.abs() < 0.99 { Vec3::Z } else { Vec3::X };
        let u = up.cross(self.normal).normalize();
        (u, self.normal.cross(u).normalize())
    }

    /// The rectangle on the plane round the model (world bounds), as its four corners.
    pub fn rectangle(&self, world: &Bounds) -> [Vec3; 4] {
        let (u, v) = self.basis();
        let (mut lo, mut hi) = ((f32::MAX, f32::MAX), (f32::MIN, f32::MIN));
        for c in SectionBox::around(world).corners() {
            let (a, b) = (u.dot(c), v.dot(c));
            lo = (lo.0.min(a), lo.1.min(b));
            hi = (hi.0.max(a), hi.1.max(b));
        }
        let pad = ((hi.0 - lo.0).max(hi.1 - lo.1) * 0.05).max(0.05);
        let base = self.normal * self.offset;
        let at = |a: f32, b: f32| base + u * a + v * b;
        [at(lo.0 - pad, lo.1 - pad), at(hi.0 + pad, lo.1 - pad), at(hi.0 + pad, hi.1 + pad), at(lo.0 - pad, hi.1 + pad)]
    }

    /// The middle of the rectangle, where the handle sits.
    pub fn handle(&self, world: &Bounds) -> Vec3 {
        let r = self.rectangle(world);
        (r[0] + r[2]) * 0.5
    }

    /// For the shader: normal + offset, rectangle middle + half size (the cut fill is drawn as a
    /// square on the plane).
    pub fn uniform(&self, world: &Bounds) -> [[f32; 4]; 2] {
        let r = self.rectangle(world);
        let middle = (r[0] + r[2]) * 0.5;
        let half = (r[1] - r[0]).length().max((r[3] - r[0]).length()) * 0.5;
        [[self.normal.x, self.normal.y, self.normal.z, self.offset], [middle.x, middle.y, middle.z, half]]
    }
}

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
