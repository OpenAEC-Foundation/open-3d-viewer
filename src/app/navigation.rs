//! Navigation about the point under the cursor, as BIM Vision and xeokit do it (issue #324): a
//! drag orbits about the surface point where it began, shown with a small axis cross while it
//! turns, or pans at that point's depth so what was grabbed follows the cursor; the wheel moves
//! towards the point under the cursor at a speed that follows how far away it is, not the size of
//! the model, and still gets through a surface close by.
//!
//! What lies under the cursor is looked up once per gesture, never per mouse move: at the start
//! of a drag, and for the wheel when the cursor moved, the wheel rested a moment, the camera
//! changed otherwise or the eye passed the point. Only what is shown counts: the section box and
//! plane cut away what lies outside them, hidden elements are passed through. Where nothing lies
//! under the cursor, the point is in the middle of what the ray crosses of the section box (or
//! the model), else at the target's depth.
//!
//! A 2D drawing in the model (plan lines, contours, boundaries: line segments that lie flat at
//! one height) counts as a sheet at that height: the rectangle around its lines, the white space
//! between them included, is something to grab, orbit about and zoom to, as a surface is.

use super::{face_at, App};
use crate::model::Model;
use crate::viewport::camera::{Camera, MIN_STEP};
use glam::{Vec2, Vec3};
use std::collections::HashMap;
use iced::widget::canvas::{self, Frame, Path};
use iced::{mouse, Color, Point, Rectangle};
use web_time::{Duration, Instant};

/// How far the cursor may move (logical pixels) while the wheel turns before the point under it
/// is looked up again.
const WHEEL_SLOP: f32 = 3.0;
/// How long the wheel may rest before the point under the cursor is looked up again.
const WHEEL_PAUSE: Duration = Duration::from_millis(180);
/// The viewport height (logical pixels) at which a pixel of drag turns the view by the camera's
/// own rate; a taller viewport turns less a pixel, a smaller one more, so a drag across the
/// whole view turns as far on a phone as on a large screen (issue #324). Below 250 pixels it
/// turns no faster.
const REFERENCE_HEIGHT: f32 = 900.0;
/// How far apart in height the two ends of a segment, and two segments, may lie and still be
/// one level of 2D drawing: this share of the model's size.
const FLAT_SHARE: f32 = 1e-5;

/// What the navigation under way holds on to.
#[derive(Default)]
pub(crate) struct Navigation {
    /// The point (world) the drag under way orbits about, shown while it does.
    pivot: Option<Vec3>,
    /// The point (world) a double click or the menu marked: every orbit turns about it, and it
    /// stays shown, until a double click in empty space or the menu lets it go.
    pub(crate) marked: Option<Vec3>,
    /// The depth the drag under way pans at, and how fast: half as fast where it grabbed
    /// nothing, as that depth is a guess (issue #324, B).
    pan: Option<(f32, f32)>,
    /// The point the wheel moves towards, while the wheel keeps turning.
    wheel: Option<WheelAnchor>,
    /// The levels of 2D drawing of the model shown, found once per model.
    flat: Option<FlatLevels>,
    /// Where looking around holds the hidden cursor (viewport, logical pixels): a crosshair.
    held: Option<(f32, f32)>,
}

/// The levels of 2D drawing of a model (model space): per layer, the line segments that lie
/// flat at one height, as the rectangle around them at that height.
pub(crate) struct FlatLevels {
    model: u64,
    levels: Vec<FlatLevel>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct FlatLevel {
    layer: usize,
    z: f32,
    min: Vec2,
    max: Vec2,
}

impl FlatLevels {
    pub(crate) fn new(model: &Model) -> Self {
        let step = (model.bounds.radius() * FLAT_SHARE).max(f32::MIN_POSITIVE);
        // per layer and height: the rectangle so far and how many segments lie in it
        let mut found: HashMap<(usize, i64), (f32, Vec2, Vec2, usize)> = HashMap::new();
        for (l, layer) in model.layers.iter().enumerate() {
            let pos = &layer.positions;
            let at = |i: u32| pos.get(3 * i as usize..3 * i as usize + 3).map(|p| Vec3::new(p[0], p[1], p[2]));
            for pair in layer.lines.chunks_exact(2) {
                let (Some(a), Some(b)) = (at(pair[0]), at(pair[1])) else { continue };
                if (a.z - b.z).abs() > step {
                    continue;
                }
                let entry = found.entry((l, (a.z / step).round() as i64)).or_insert((a.z, Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY), 0));
                entry.1 = entry.1.min(a.truncate()).min(b.truncate());
                entry.2 = entry.2.max(a.truncate()).max(b.truncate());
                entry.3 += 1;
            }
        }
        // a lone segment, or segments along one line, make no sheet
        let mut levels: Vec<FlatLevel> = found
            .into_iter()
            .filter(|(_, (_, min, max, n))| *n >= 2 && (*max - *min).min_element() > step)
            .map(|((layer, _), (z, min, max, _))| FlatLevel { layer, z, min, max })
            .collect();
        levels.sort_by(|a, b| (a.layer, a.z).partial_cmp(&(b.layer, b.z)).unwrap_or(std::cmp::Ordering::Equal));
        Self { model: model.id, levels }
    }

    /// The nearest level the ray `origin + t·dir` (model space) meets ahead, among the visible
    /// layers and where `keep` (the section box) shows it: t and the point.
    fn hit(&self, origin: Vec3, dir: Vec3, visible: &[bool], keep: &dyn Fn(Vec3) -> bool) -> Option<(f32, Vec3)> {
        if dir.z.abs() < 1e-12 {
            return None;
        }
        self.levels
            .iter()
            .filter(|lv| visible.get(lv.layer).copied().unwrap_or(true))
            .filter_map(|lv| {
                let t = (lv.z - origin.z) / dir.z;
                let p = origin + dir * t;
                let inside = p.x >= lv.min.x && p.x <= lv.max.x && p.y >= lv.min.y && p.y <= lv.max.y;
                (t > 0.0 && inside && keep(p)).then_some((t, p))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
    }
}

#[derive(Clone, Copy)]
struct WheelAnchor {
    /// Where the cursor was (viewport, logical pixels).
    at: (f32, f32),
    /// The point under it (world).
    point: Vec3,
    /// The least a step moves: a share of how far the point was when it was found.
    min: f32,
    /// The camera after the last step: another one means the view moved otherwise.
    camera: Camera,
    when: Instant,
}

impl App {
    /// A drawing view (not a 3D one) is shown: dragging it moves it, it does not turn.
    pub(super) fn drawing_view_shown(&self) -> bool {
        self.active_plan.as_deref().is_some_and(|p| self.document.view_3d(p).is_none())
    }

    /// A drag starts at (x, y) in a viewport of width × height: to orbit (about the point there)
    /// or to pan (at its depth).
    pub(super) fn grab(&mut self, x: f32, y: f32, width: f32, height: f32, orbit: bool) {
        self.anim = None;
        let (width, height) = (width.max(1.0), height.max(1.0));
        let hit = self.surface_at(x, y, width, height);
        let point = hit.unwrap_or_else(|| {
            let (origin, dir) = self.camera.ray((2.0 * x / width - 1.0, 1.0 - 2.0 * y / height), width / height);
            self.virtual_point(origin, dir)
        });
        let depth = (point - self.camera.eye()).dot(self.camera.forward());
        // not nearer than a few times the near plane (issue #324, B)
        self.nav.pan = Some((depth.max(self.camera.near() * 5.0), if hit.is_some() { 1.0 } else { 0.5 }));
        self.nav.pivot = (orbit && !self.drawing_view_shown()).then_some(self.nav.marked.unwrap_or(point));
        self.nav.wheel = None;
    }

    /// A double click at (x, y): the surface there is the point to orbit about from now on, shown
    /// without moving the view; in empty space, every drag orbits about its own point again.
    pub(super) fn mark_pivot(&mut self, x: f32, y: f32, width: f32, height: f32) {
        self.nav.marked = self.surface_at(x, y, width.max(1.0), height.max(1.0));
    }

    /// The menu's "pivot here" at (x, y) in the viewport: the surface there, or else the point
    /// the ray finds inside what is shown.
    pub(super) fn place_pivot(&mut self, x: f32, y: f32) {
        let (width, height) = self.viewport_size;
        self.nav.marked = Some(self.nav_anchor_at(x, y, width, height));
    }

    /// The menu lets the marked pivot go.
    pub(super) fn clear_pivot(&mut self) {
        self.nav.marked = None;
    }

    pub(super) fn has_marked_pivot(&self) -> bool {
        self.nav.marked.is_some()
    }

    /// How far a pixel of drag turns, against the camera's own rate (`REFERENCE_HEIGHT`).
    fn turn_scale(&self) -> f32 {
        REFERENCE_HEIGHT / self.viewport_size.1.max(250.0)
    }

    pub(super) fn look_drag(&mut self, dx: f32, dy: f32) {
        self.anim = None;
        let s = self.turn_scale();
        self.camera.look_around(dx * s, dy * s);
    }

    /// A click goes to a tool (measuring, modelling, a drawing view's tools, placing a plane):
    /// two quick clicks there are two clicks, not a double click marking the pivot.
    pub(super) fn clicks_go_to_a_tool(&self) -> bool {
        let m = &self.modeller;
        m.grab.is_some()
            || m.sketch.is_some()
            || m.transform.is_some()
            || m.offset.is_some()
            || m.pair_edit.is_some()
            || m.grid_tool
            || m.tool.is_some()
            || m.edit.is_some()
            || self.measure.tool.is_some()
            || self.plane_picking
            || self.drawing_view_shown()
            || self.shown_view().is_some()
    }

    /// Looking around holds the cursor at (x, y): the crosshair shows there.
    pub(super) fn hold(&mut self, x: f32, y: f32) {
        self.nav.held = Some((x, y));
    }

    /// The crosshair where looking around holds the cursor.
    pub(super) fn crosshair_overlay(&self) -> Option<Crosshair> {
        self.nav.held.map(|(x, y)| Crosshair { at: Point::new(x, y) })
    }

    /// The drag ended.
    pub(super) fn release(&mut self) {
        self.nav.pivot = None;
        self.nav.pan = None;
        self.nav.held = None;
    }

    pub(super) fn orbit_drag(&mut self, dx: f32, dy: f32) {
        self.anim = None;
        let (dx, dy) = (dx * self.turn_scale(), dy * self.turn_scale());
        match self.nav.pivot {
            Some(pivot) => self.camera.orbit_about(pivot, dx, dy),
            None => self.camera.orbit(dx, dy),
        }
    }

    pub(super) fn pan_drag(&mut self, dx: f32, dy: f32, height: f32) {
        self.anim = None;
        match self.nav.pan {
            Some((depth, rate)) => self.camera.pan_at(dx * rate, dy * rate, height, depth),
            None => self.camera.pan(dx, dy, height),
        }
    }

    /// Wheel steps (positive = closer) at (x, y) in a viewport of width × height: towards the
    /// point under the cursor (a parallel view scales about it).
    pub(super) fn wheel_zoom(&mut self, steps: f32, x: f32, y: f32, width: f32, height: f32) {
        self.anim = None;
        let (width, height) = (width.max(1.0), height.max(1.0));
        if self.camera.ortho {
            let ndc = (2.0 * x / width - 1.0, 1.0 - 2.0 * y / height);
            self.camera.zoom_at(steps, ndc, width / height);
            return;
        }
        let now = Instant::now();
        let kept = self.nav.wheel.filter(|w| {
            w.camera == self.camera && (w.at.0 - x).hypot(w.at.1 - y) <= WHEEL_SLOP && now.saturating_duration_since(w.when) < WHEEL_PAUSE
        });
        let (point, min) = match kept {
            Some(w) => (w.point, w.min),
            None => {
                let point = self.nav_anchor_at(x, y, width, height);
                (point, (point - self.camera.eye()).length() * MIN_STEP)
            }
        };
        let passed = self.camera.dolly_to(point, steps, min);
        // past the point, the next step looks what lies behind it
        self.nav.wheel = (!passed).then_some(WheelAnchor { at: (x, y), point, min, camera: self.camera, when: now });
    }

    /// The point (world) to navigate about at (x, y): the surface shown there, else a point on
    /// the ray inside what is shown.
    fn nav_anchor_at(&mut self, x: f32, y: f32, width: f32, height: f32) -> Vec3 {
        let (width, height) = (width.max(1.0), height.max(1.0));
        if let Some(point) = self.surface_at(x, y, width, height) {
            return point;
        }
        let ndc = (2.0 * x / width - 1.0, 1.0 - 2.0 * y / height);
        let (origin, dir) = self.camera.ray(ndc, width / height);
        self.virtual_point(origin, dir)
    }

    /// The nearest point (world) of the model shown at (x, y): not cut away, not hidden.
    /// A level of 2D drawing counts as a sheet (`FlatLevels`): the nearer of it and the
    /// triangle hit is taken.
    fn surface_at(&mut self, x: f32, y: f32, width: f32, height: f32) -> Option<Vec3> {
        let model = self.model.clone()?;
        let (origin, dir, hit) = self.cast(&model, x, y, width, height);
        let along = |p: Vec3| (p - origin).dot(dir) / dir.length_squared().max(1e-20);
        let surface = hit.and_then(|(l, t, s)| face_at(&model.layers[l], t, origin - s, dir)).map(|(p, _)| p);
        let surface = hit.zip(surface).map(|((_, _, s), p)| (along(p + s), p + s));
        let mm = self.model_matrix();
        if self.nav.flat.as_ref().is_none_or(|f| f.model != model.id) {
            self.nav.flat = Some(FlatLevels::new(&model));
        }
        let shown = self.shown();
        let keep = |p: Vec3| shown(mm.transform_point3(p));
        let flat = self.nav.flat.as_ref().and_then(|f| f.hit(origin, dir, &self.visible, &keep));
        let nearest = match (surface, flat) {
            (Some(a), Some(b)) => Some(if b.0 < a.0 { b } else { a }),
            (a, b) => a.or(b),
        };
        nearest.map(|(_, p)| mm.transform_point3(p))
    }

    /// Where the ray (world, `dir` a unit vector) meets nothing: the middle of the part of it
    /// inside the section box, or else the model's bounds; past them, at the target's depth.
    fn virtual_point(&self, origin: Vec3, dir: Vec3) -> Vec3 {
        let span = match self.active_section() {
            Some(s) => {
                let o = s.to_local(origin);
                ray_box(o, s.to_local(origin + dir) - o, s.min, s.max)
            }
            None => self.world_bounds().and_then(|b| ray_box(origin, dir, Vec3::from_array(b.min), Vec3::from_array(b.max))),
        };
        if let Some((near, far)) = span {
            return origin + dir * (near.max(0.0) + far) * 0.5;
        }
        let along = dir.dot(self.camera.forward()).max(0.1);
        origin + dir * (self.camera.distance / along)
    }

    /// The point a drag orbits about, while it does.
    pub(super) fn pivot_overlay(&self) -> Option<PivotMark> {
        let at = self.nav.pivot.or(self.nav.marked)?;
        Some(PivotMark { at, camera: self.camera, fixed: self.nav.marked == Some(at) })
    }
}

/// Where the ray `origin + t·dir` enters and leaves the box from `min` to `max`, if it meets it
/// ahead of `origin` (slabs).
fn ray_box(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3) -> Option<(f32, f32)> {
    let (mut near, mut far) = (f32::NEG_INFINITY, f32::INFINITY);
    for i in 0..3 {
        if dir[i].abs() < 1e-12 {
            if origin[i] < min[i] || origin[i] > max[i] {
                return None;
            }
            continue;
        }
        let (a, b) = ((min[i] - origin[i]) / dir[i], (max[i] - origin[i]) / dir[i]);
        near = near.max(a.min(b));
        far = far.min(a.max(b));
    }
    (far >= near.max(0.0)).then_some((near, far))
}

/// The point a drag orbits about, drawn over the viewport: short arms along the world axes (X
/// red, Y green, Z blue). While a drag orbits about its own point, three thin lines as in CAD; a
/// fixed pivot stands out more, with a dot and haloed arms, as it stays when the drag ends.
pub struct PivotMark {
    pub at: Vec3,
    pub camera: Camera,
    /// The pivot was marked (double click, menu) and stays.
    pub fixed: bool,
}

/// How long an arm of the pivot mark is (logical pixels) when it lies across the view.
const ARM: f32 = 30.0;

impl<M> canvas::Program<M> for PivotMark {
    type State = ();

    fn draw(&self, _state: &(), renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let view_proj = self.camera.view_proj(bounds.width / bounds.height.max(1.0));
        let screen = |p: Vec3| {
            let c = view_proj.project_point3(p);
            (0.0..=1.0).contains(&c.z).then(|| Point::new((c.x + 1.0) / 2.0 * bounds.width, (1.0 - c.y) / 2.0 * bounds.height))
        };
        let Some(middle) = screen(self.at) else { return vec![frame.into_geometry()] };
        // an arm of ARM pixels at the pivot's depth, shorter as it points towards the eye
        let depth = if self.camera.ortho { self.camera.distance } else { (self.at - self.camera.eye()).dot(self.camera.forward()) };
        let length = 2.0 * depth.max(self.camera.near()) * (self.camera.fov_y * 0.5).tan() / bounds.height.max(1.0) * ARM;
        let axes = [(Vec3::X, Color::from_rgb8(220, 50, 50)), (Vec3::Y, Color::from_rgb8(50, 170, 70)), (Vec3::Z, Color::from_rgb8(50, 110, 230))];
        for (axis, colour) in axes {
            if let Some(end) = screen(self.at + axis * length) {
                let arm = Path::line(middle, end);
                if !self.fixed {
                    frame.stroke(&arm, canvas::Stroke::default().with_color(colour).with_width(1.0));
                    continue;
                }
                frame.stroke(&arm, canvas::Stroke::default().with_color(Color::from_rgba8(255, 255, 255, 0.8)).with_width(4.0));
                frame.stroke(&arm, canvas::Stroke::default().with_color(colour).with_width(2.0));
            }
        }
        if !self.fixed {
            return vec![frame.into_geometry()];
        }
        frame.fill(&Path::circle(middle, 4.0), Color::WHITE);
        frame.stroke(&Path::circle(middle, 4.0), canvas::Stroke::default().with_color(Color::from_rgb8(30, 30, 30)).with_width(1.5));
        vec![frame.into_geometry()]
    }
}

/// Where looking around holds the hidden cursor, as a game shows it: a horizontal and a vertical
/// stroke with an open middle, white with a dark edge so it shows on light and dark alike.
pub struct Crosshair {
    pub at: Point,
}

/// The open middle of the crosshair and the length of its strokes (logical pixels).
const CROSS_GAP: f32 = 4.0;
const CROSS_ARM: f32 = 9.0;

impl<M> canvas::Program<M> for Crosshair {
    type State = ();

    fn draw(&self, _state: &(), renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let c = self.at;
        let arms = [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)].map(|(dx, dy): (f32, f32)| {
            Path::line(Point::new(c.x + dx * CROSS_GAP, c.y + dy * CROSS_GAP), Point::new(c.x + dx * (CROSS_GAP + CROSS_ARM), c.y + dy * (CROSS_GAP + CROSS_ARM)))
        });
        for arm in &arms {
            frame.stroke(arm, canvas::Stroke::default().with_color(Color::from_rgba8(0, 0, 0, 0.75)).with_width(4.0));
        }
        for arm in &arms {
            frame.stroke(arm, canvas::Stroke::default().with_color(Color::WHITE).with_width(2.0));
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{MenuItem, Message};
    use crate::model::{Layer, Model};
    use crate::viewport::camera::ViewPreset;
    use crate::viewport::Navigate;
    use std::sync::Arc;

    /// A floor of 100 × 100 m at z = 2, seen from above in a viewport of 1000 × 800.
    fn floor() -> App {
        let layer = Layer {
            name: "Floors".into(),
            positions: vec![-50.0, -50.0, 2.0, 50.0, -50.0, 2.0, 50.0, 50.0, 2.0, -50.0, 50.0, 2.0],
            indices: vec![0, 1, 2, 0, 2, 3],
            elements: vec![(1, 0, 6)],
            ..Layer::default()
        };
        let mut app = App::default();
        app.set_model(Arc::new(Model::new("floor.obj".into(), "OBJ", vec![layer])));
        app.viewport_size = (1000.0, 800.0);
        app.camera.set_preset(ViewPreset::Top);
        app.fit();
        app
    }

    #[test]
    fn the_menu_places_and_clears_the_pivot_and_orbits_keep_it() {
        let mut app = floor();
        app.context_menu = Some(glam::Vec2::new(600.0, 400.0));
        let _ = app.update(Message::ContextMenu(MenuItem::PivotHere));
        let marked = app.nav.marked.expect("a pivot on the floor");
        assert!((marked.z - 2.0).abs() < 1e-3 && marked.x > 1.0, "{marked}");
        // an orbit starting on it turns about it
        let _ = app.update(Message::Navigate(Navigate::Grab { x: 605.0, y: 400.0, width: 1000.0, height: 800.0, orbit: true }));
        assert_eq!(app.nav.pivot, Some(marked));
        let _ = app.update(Message::Navigate(Navigate::Release));
        // so does one starting elsewhere on the floor: the mark stays until it is let go
        let _ = app.update(Message::Navigate(Navigate::Grab { x: 300.0, y: 400.0, width: 1000.0, height: 800.0, orbit: true }));
        assert_eq!(app.nav.pivot, Some(marked));
        let _ = app.update(Message::Navigate(Navigate::Release));
        assert!(app.pivot_overlay().is_some_and(|m| m.fixed));
        // a double click in empty space lets it go
        app.camera.set_preset(ViewPreset::Front);
        let _ = app.update(Message::Navigate(Navigate::Pivot { x: 500.0, y: 20.0, width: 1000.0, height: 800.0 }));
        assert!(app.nav.marked.is_none() && app.pivot_overlay().is_none());
        // the menu lets a marked pivot go
        app.context_menu = Some(glam::Vec2::new(500.0, 400.0));
        let _ = app.update(Message::ContextMenu(MenuItem::PivotHere));
        assert!(app.has_marked_pivot());
        let _ = app.update(Message::ContextMenu(MenuItem::ClearPivot));
        assert!(!app.has_marked_pivot());
    }

    #[test]
    fn a_flat_drawing_is_a_sheet_to_grab_white_space_and_all() {
        // a floor at z = 2, and beside it a plan of four lines at z = 0
        let mut app = floor();
        let plan = Layer {
            name: "Plan".into(),
            positions: vec![60.0, -40.0, 0.0, 140.0, -40.0, 0.0, 140.0, 40.0, 0.0, 60.0, 40.0, 0.0, 60.0, -40.0, 0.0],
            lines: vec![0, 1, 1, 2, 2, 3, 3, 4],
            ..Layer::default()
        };
        let mut model = (**app.model.as_ref().unwrap()).clone();
        model.layers.push(plan);
        app.set_model(Arc::new(model));
        app.camera.set_preset(ViewPreset::Top);
        app.fit();
        // between the plan's lines, nothing is drawn: the sheet is grabbed at its height
        let (x, y) = app.on_screen(glam::DVec3::new(100.0, 0.0, 0.0), 1000.0, 800.0).unwrap();
        let _ = app.update(Message::Navigate(Navigate::Grab { x, y, width: 1000.0, height: 800.0, orbit: true }));
        let pivot = app.nav.pivot.unwrap();
        assert!(pivot.z.abs() < 1e-3 && (pivot.x - 100.0).abs() < 0.5, "{pivot}");
        // hidden, it is grabbed no more
        let _ = app.update(Message::Navigate(Navigate::Release));
        let _ = app.update(Message::Layer(1, false));
        let _ = app.update(Message::Navigate(Navigate::Grab { x, y, width: 1000.0, height: 800.0, orbit: true }));
        assert!(app.nav.pivot.unwrap().z.abs() > 1e-3 || (app.nav.pivot.unwrap().x - 100.0).abs() > 0.5);
    }

    #[test]
    fn a_lone_line_or_a_slanted_one_makes_no_sheet() {
        let layer = Layer {
            name: "Lines".into(),
            positions: vec![0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 20.0, 0.0, 0.0, 0.0, 0.0, 0.0, 10.0, 10.0, 5.0],
            lines: vec![0, 1, 1, 2, 3, 4],
            ..Layer::default()
        };
        let levels = FlatLevels::new(&Model::new("lines.dxf".into(), "DXF", vec![layer]));
        assert!(levels.levels.is_empty(), "{:?}", levels.levels);
    }

    #[test]
    fn looking_around_shows_the_crosshair_until_the_release() {
        let mut app = floor();
        let _ = app.update(Message::Navigate(Navigate::Hold { x: 300.0, y: 200.0 }));
        assert!(app.crosshair_overlay().is_some_and(|c| c.at == Point::new(300.0, 200.0)));
        let _ = app.update(Message::Navigate(Navigate::Release));
        assert!(app.crosshair_overlay().is_none());
    }

    #[test]
    fn a_drag_turns_as_far_across_any_viewport_height() {
        let mut small = floor();
        small.viewport_size = (500.0, 400.0);
        let mut large = floor();
        large.viewport_size = (2000.0, 1600.0);
        // the same share of the view's height
        small.orbit_drag(40.0, 0.0);
        large.orbit_drag(160.0, 0.0);
        assert!((small.camera.yaw - large.camera.yaw).abs() < 1e-4, "{} vs {}", small.camera.yaw, large.camera.yaw);
    }

    #[test]
    fn a_ray_meets_a_box_ahead_only() {
        let (min, max) = (Vec3::ZERO, Vec3::ONE);
        assert_eq!(ray_box(Vec3::new(-1.0, 0.5, 0.5), Vec3::X, min, max), Some((1.0, 2.0)));
        assert_eq!(ray_box(Vec3::new(2.0, 0.5, 0.5), Vec3::X, min, max), None);
        // from inside: it leaves ahead
        assert_eq!(ray_box(Vec3::splat(0.5), Vec3::Z, min, max), Some((-0.5, 0.5)));
    }
}
