//! Edit mode, as in Blender's and in Open BIM Studio's: Tab on a selected modelled element shows
//! the points of its definition (a member's ends, a wall's or a curtain wall's axis ends, a slab's
//! corners, an object's place).
//!
//! - Drag a point to move it, and the other picked points with it; a click picks a point, Ctrl adds.
//! - G grabs the picked points: they follow the cursor until a click puts them down. X, Y or Z holds
//!   the move to that axis.
//! - A picks every point; Delete removes the picked corners of a slab.
//! - Tab leaves; Esc drops a move under way, else leaves.
//!
//! A move snaps to the points of the other modelled elements and is rounded to 10 mm. It is
//! evaluated by the kernel when it is put down; a refusal leaves the element as it was and shows
//! why.

use super::{App, Message};
use crate::selection::Pickable;
use bim_core::element::PlanarForm;
use bim_core::{BaseKind, Element, ElementId, PlanarDef, Refusal, Shape};
use glam::{DVec3, Mat4, Vec3};
use iced::widget::canvas::{self, Frame, Path};
use iced::{keyboard, mouse, Color, Point, Rectangle, Size};

/// An axis a move is held to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

/// A move under way.
#[derive(Debug, Clone, PartialEq)]
pub struct Grab {
    /// The element's points when the move began (scene coordinates).
    pub from: Vec<DVec3>,
    /// The point the cursor holds.
    pub anchor: usize,
    /// How far the picked points have moved.
    pub delta: DVec3,
    pub axis: Option<Axis>,
    /// Begun with G: it follows the cursor and a click puts it down (a drag ends when the button
    /// comes up).
    pub by_key: bool,
}

/// Edit mode on a modelled element.
#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    pub element: ElementId,
    /// The picked points (indices into [`edit_points`]).
    pub picked: Vec<usize>,
    pub grab: Option<Grab>,
}

/// What the edit overlay reports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EditEvent {
    /// The button went down on point `index` (`add`: with Ctrl).
    Press { index: usize, add: bool },
    /// The cursor moved while a point is dragged, to (x, y) in a viewport of width × height.
    Drag { x: f32, y: f32, width: f32, height: f32 },
    /// The button came up after dragging.
    Release,
}

/// A press on point `index` (for renders and tests outside the app module).
#[cfg(test)]
pub fn press(index: usize) -> EditEvent {
    EditEvent::Press { index, add: false }
}

impl From<EditEvent> for Message {
    fn from(e: EditEvent) -> Self {
        Message::Edit(e)
    }
}

/// The points of a definition that edit mode moves (in its coordinates), and whether they close
/// a loop (a slab's outline).
pub fn edit_points(shape: &Shape) -> (Vec<DVec3>, bool) {
    match shape {
        Shape::Point(p) => (vec![p.at], false),
        Shape::Linear(l) => (vec![l.start, l.end], false),
        Shape::Planar(PlanarDef { form: PlanarForm::Wall { start, end, base, .. }, .. }) => (vec![start.extend(*base), end.extend(*base)], false),
        Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, top, .. }, .. }) => (outline.iter().map(|p| p.extend(*top)).collect(), true),
        Shape::System(s) => (vec![s.start.extend(s.base), s.end.extend(s.base)], false),
        Shape::Connection(_) => (Vec::new(), false),
    }
}

/// `shape` with its edit points moved to `points` (as many as [`edit_points`] gives, or fewer
/// for a slab whose corners were removed). Planar elements and systems keep one height: their first
/// point's.
pub fn with_points(shape: &Shape, points: &[DVec3]) -> Shape {
    let mut shape = shape.clone();
    let flat = |p: &DVec3| p.truncate();
    match &mut shape {
        Shape::Point(p) => p.at = points[0],
        Shape::Linear(l) => {
            l.start = points[0];
            l.end = points[1];
        }
        Shape::Planar(PlanarDef { form: PlanarForm::Wall { start, end, base, .. }, .. }) => {
            *start = flat(&points[0]);
            *end = flat(&points[1]);
            *base = points[0].z;
        }
        Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, top, .. }, .. }) => {
            *outline = points.iter().map(flat).collect();
            *top = points[0].z;
        }
        Shape::System(s) => {
            s.start = flat(&points[0]);
            s.end = flat(&points[1]);
            s.base = points[0].z;
        }
        Shape::Connection(_) => {}
    }
    shape
}

/// Clicks within this of a point (logical pixels) take it.
const HANDLE_PIXELS: f32 = 9.0;
/// Moves off every snap are rounded to this (metres).
const ROUND: f64 = 0.01;

impl App {
    /// The modelled element that edit mode would open: the one selected, alone, with points.
    fn editable(&self) -> Option<ElementId> {
        let [id] = self.selected_modelled()[..] else { return None };
        let e = self.modeller.model.get(id)?;
        (!edit_points(&e.shape).0.is_empty()).then_some(id)
    }

    /// Whether Tab means edit mode now: in it, or with one modelled element selected.
    pub(crate) fn wants_tab(&self) -> bool {
        self.modeller.edit.is_some() || self.editable().is_some()
    }

    /// Tab: into edit mode on the selected element, or out of it.
    pub(super) fn toggle_edit(&mut self) {
        if self.modeller.edit.is_some() {
            self.modeller.edit = None;
            return;
        }
        if let Some(element) = self.editable() {
            self.modeller.tool = None;
            self.modeller.points.clear();
            self.modeller.refusal = None;
            self.measure.tool = None;
            self.modeller.edit = Some(Edit { element, picked: Vec::new(), grab: None });
        }
    }

    /// The edited element and its definition in scene coordinates.
    fn edited(&self) -> Option<(&Element, Shape)> {
        let e = self.modeller.model.get(self.modeller.edit.as_ref()?.element)?;
        Some((e, e.shape_in_project(&self.modeller.model.grid)))
    }

    /// The edited element's points as shown: moved where a move under way has them.
    pub(super) fn edit_points_now(&self) -> Option<(Vec<DVec3>, bool)> {
        let edit = self.modeller.edit.as_ref()?;
        let (e, shape) = self.edited()?;
        let (mut points, closed) = edit_points(&shape);
        if let Some(g) = &edit.grab {
            // up or down, a planar element or a system goes as a whole: it has one height
            let whole = g.axis == Some(Axis::Z) && matches!(e.base(), BaseKind::Planar | BaseKind::System);
            for (i, p) in points.iter_mut().enumerate() {
                if whole || edit.picked.contains(&i) {
                    *p = g.from[i] + g.delta;
                }
            }
        }
        Some((points, closed))
    }

    /// Something the edit overlay reports.
    pub(super) fn edit_event(&mut self, event: EditEvent) {
        let Some(edit) = self.modeller.edit.as_mut() else { return };
        match event {
            EditEvent::Press { index, add } => {
                if add {
                    if let Some(k) = edit.picked.iter().position(|&i| i == index) {
                        edit.picked.remove(k);
                        return;
                    }
                    edit.picked.push(index);
                } else if !edit.picked.contains(&index) {
                    edit.picked = vec![index];
                }
                self.begin_grab(index, false);
            }
            EditEvent::Drag { x, y, width, height } => self.grab_to(x, y, width, height),
            EditEvent::Release => {
                if edit.grab.as_ref().is_some_and(|g| !g.by_key) {
                    self.put_down();
                }
            }
        }
    }

    fn begin_grab(&mut self, anchor: usize, by_key: bool) {
        let Some((points, _)) = self.edited().map(|(_, s)| edit_points(&s)) else { return };
        if let Some(edit) = self.modeller.edit.as_mut() {
            edit.grab = Some(Grab { from: points, anchor, delta: DVec3::ZERO, axis: None, by_key });
        }
        self.modeller.refusal = None;
    }

    /// A key in edit mode: G grabs the picked points, X, Y and Z hold the move to an axis, A picks
    /// every point.
    pub(super) fn edit_key(&mut self, key: char) {
        let Some(edit) = self.modeller.edit.as_mut() else { return };
        match key.to_ascii_lowercase() {
            'g' if edit.grab.is_none() && !edit.picked.is_empty() => {
                let anchor = edit.picked[0];
                self.begin_grab(anchor, true);
            }
            'x' | 'y' | 'z' => {
                if let Some(g) = edit.grab.as_mut() {
                    let axis = match key.to_ascii_lowercase() {
                        'x' => Axis::X,
                        'y' => Axis::Y,
                        _ => Axis::Z,
                    };
                    g.axis = if g.axis == Some(axis) { None } else { Some(axis) };
                }
            }
            'a' => {
                if let Some(n) = self.edited().map(|(_, s)| edit_points(&s).0.len()) {
                    if let Some(edit) = self.modeller.edit.as_mut() {
                        edit.picked = if edit.picked.len() == n { Vec::new() } else { (0..n).collect() };
                    }
                }
            }
            _ => {}
        }
    }

    /// The cursor rests at (x, y) while G's move follows it.
    pub(super) fn edit_hover(&mut self, x: f32, y: f32, width: f32, height: f32) -> bool {
        let follows = self.modeller.edit.as_ref().and_then(|e| e.grab.as_ref()).is_some_and(|g| g.by_key);
        if follows {
            self.grab_to(x, y, width, height);
        }
        follows
    }

    /// Whether the viewport must report where the cursor rests (G's move follows it).
    pub(super) fn edit_follows_cursor(&self) -> bool {
        self.modeller.edit.as_ref().and_then(|e| e.grab.as_ref()).is_some_and(|g| g.by_key)
    }

    /// A click in the viewport in edit mode (not on a point): puts G's move down, else drops the
    /// picked points. Always taken.
    pub(super) fn edit_click(&mut self) {
        let Some(edit) = self.modeller.edit.as_mut() else { return };
        if edit.grab.is_some() {
            self.put_down();
        } else {
            edit.picked.clear();
        }
    }

    /// Esc in edit mode: drops a move under way, else leaves. False when not in edit mode.
    pub(super) fn edit_escape(&mut self) -> bool {
        let Some(edit) = self.modeller.edit.as_mut() else { return false };
        if edit.grab.take().is_none() {
            self.modeller.edit = None;
        }
        true
    }

    /// Delete in edit mode: removes the picked corners of a slab (three stay at least). False when
    /// not in edit mode.
    pub(super) fn edit_delete(&mut self) -> bool {
        let Some(edit) = self.modeller.edit.clone() else { return false };
        let Some((_, shape)) = self.edited() else { return true };
        let (points, closed) = edit_points(&shape);
        if !closed || edit.picked.is_empty() {
            return true;
        }
        let kept: Vec<DVec3> = points.iter().enumerate().filter(|(i, _)| !edit.picked.contains(i)).map(|(_, p)| *p).collect();
        if kept.len() < 3 {
            self.modeller.refusal = Some(Refusal::TooFewPoints);
            return true;
        }
        if self.change_edited(with_points(&shape, &kept)) {
            if let Some(edit) = self.modeller.edit.as_mut() {
                edit.picked.clear();
            }
        }
        true
    }

    /// Moves the held point to under the cursor at (x, y): on the horizontal plane through it, or
    /// along the vertical with Z; onto a point of another element near the cursor, else rounded,
    /// and held to the axis chosen.
    fn grab_to(&mut self, x: f32, y: f32, width: f32, height: f32) {
        let Some(edit) = self.modeller.edit.as_ref() else { return };
        let Some(g) = edit.grab.clone() else { return };
        let anchor = g.from[g.anchor];
        let mm = self.model_matrix();
        let inv = (self.camera.view_proj(width / height) * mm).inverse();
        let (nx, ny) = (2.0 * x / width - 1.0, 1.0 - 2.0 * y / height);
        let a = inv.project_point3(Vec3::new(nx, ny, 0.0)).as_dvec3();
        let dir = inv.project_point3(Vec3::new(nx, ny, 1.0)).as_dvec3() - a;
        let target = if g.axis == Some(Axis::Z) {
            // the point of the vertical through the anchor nearest the cursor's ray
            let w = DVec3::Z;
            let (b, c) = (dir.dot(w), dir.dot(dir));
            let r = a - anchor;
            let denom = c - b * b;
            if denom.abs() < 1e-12 {
                return;
            }
            let t = (c * w.dot(r) - b * dir.dot(r)) / denom;
            anchor + w * ((t / ROUND).round() * ROUND)
        } else {
            if dir.z.abs() < 1e-12 {
                return;
            }
            let t = (anchor.z - a.z) / dir.z;
            if t < 0.0 {
                return;
            }
            let p = a + dir * t;
            let near = self
                .snap_points_except(edit.element)
                .into_iter()
                .filter_map(|q| self.on_screen(q, width, height).map(|s| (q, (s.0 - x).hypot(s.1 - y))))
                .filter(|(_, d)| *d <= 12.0)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            let mut p = match near {
                Some((q, _)) => DVec3::new(q.x, q.y, anchor.z),
                None => DVec3::new((p.x / ROUND).round() * ROUND, (p.y / ROUND).round() * ROUND, anchor.z),
            };
            match g.axis {
                Some(Axis::X) => p.y = anchor.y,
                Some(Axis::Y) => p.x = anchor.x,
                _ => {}
            }
            p
        };
        if let Some(g) = self.modeller.edit.as_mut().and_then(|e| e.grab.as_mut()) {
            g.delta = target - anchor;
        }
    }

    /// Puts a move down: the element takes the moved points, if the kernel can make it.
    fn put_down(&mut self) {
        let Some((points, _)) = self.edit_points_now() else { return };
        let moved = self.modeller.edit.as_ref().and_then(|e| e.grab.as_ref()).is_some_and(|g| g.delta != DVec3::ZERO);
        if let Some(edit) = self.modeller.edit.as_mut() {
            edit.grab = None;
        }
        if moved {
            if let Some((_, shape)) = self.edited() {
                self.change_edited(with_points(&shape, &points));
            }
        }
    }

    /// Gives the edited element `shape` (scene coordinates), if the kernel can make it; else keeps
    /// it and shows why. Whether it changed.
    fn change_edited(&mut self, shape: Shape) -> bool {
        let Some((id, name, level)) = self.edited().map(|(e, _)| (e.id, e.name.clone(), e.level.clone())) else { return false };
        let grid = self.modeller.model.grid;
        let mut element = Element::in_project(&grid, name, level, shape);
        element.id = id;
        match self.modeller.model.replace(element) {
            Ok(()) => {
                self.show_modelled();
                self.select_modelled(id);
                true
            }
            Err(refusal) => {
                self.modeller.refusal = Some(refusal);
                false
            }
        }
    }

    /// The edit overlay: the edited element's points and the lines between them.
    pub(super) fn edit_overlay(&self, ink: Color, fill: Color) -> Option<EditOverlay> {
        let edit = self.modeller.edit.as_ref()?;
        let (points, closed) = self.edit_points_now()?;
        let picked = (0..points.len()).map(|i| edit.picked.contains(&i)).collect();
        Some(EditOverlay {
            points: points.iter().map(|p| p.as_vec3()).collect(),
            closed,
            picked,
            view_proj: self.camera.view_proj(self.aspect()) * self.model_matrix(),
            ink,
            fill,
        })
    }

    /// Selects modelled element `id` alone.
    pub(crate) fn select_modelled(&mut self, id: ElementId) {
        let Some(scene) = self.model.clone() else { return };
        let entry = if self.models.len() == 1 { 0 } else { self.models.iter().position(|e| Some(e.model.id) == self.modeller.shown).unwrap_or(0) as u32 };
        if let Some(shown) = scene.element_with_key(crate::model::ElementKey { model: entry, source: id.0 }) {
            self.click(Some(Pickable::Element(shown)), false);
        }
    }
}

/// The points of the edited element over the viewport, to pick and drag.
pub struct EditOverlay {
    pub points: Vec<Vec3>,
    pub closed: bool,
    pub picked: Vec<bool>,
    /// Model to clip space.
    pub view_proj: Mat4,
    pub ink: Color,
    pub fill: Color,
}

/// What the overlay keeps between events.
#[derive(Default)]
pub struct EditState {
    dragging: bool,
    hover: Option<usize>,
    ctrl: bool,
}

/// Where a model point shows in a viewport of `size`, if in front of the camera.
fn project(view_proj: &Mat4, size: Size, p: Vec3) -> Option<Point> {
    let c = view_proj.project_point3(p);
    ((0.0..=1.0).contains(&c.z)).then(|| Point::new((c.x + 1.0) / 2.0 * size.width, (1.0 - c.y) / 2.0 * size.height))
}

impl EditOverlay {
    /// The point under `at` (viewport pixels), the nearest within reach.
    fn handle_at(&self, size: Size, at: Point) -> Option<usize> {
        self.points
            .iter()
            .enumerate()
            .filter_map(|(i, p)| project(&self.view_proj, size, *p).map(|s| (i, s.distance(at))))
            .filter(|(_, d)| *d <= HANDLE_PIXELS)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }
}

impl<M: From<EditEvent>> canvas::Program<M> for EditOverlay {
    type State = EditState;

    fn update(&self, state: &mut EditState, event: &iced::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<canvas::Action<M>> {
        match event {
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                state.ctrl = m.control();
                None
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let at = cursor.position_in(bounds);
                if state.dragging {
                    let at = at?;
                    return Some(canvas::Action::publish(M::from(EditEvent::Drag { x: at.x, y: at.y, width: bounds.width, height: bounds.height })).and_capture());
                }
                let hover = at.and_then(|p| self.handle_at(bounds.size(), p));
                (hover != state.hover).then(|| {
                    state.hover = hover;
                    canvas::Action::request_redraw()
                })
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let index = self.handle_at(bounds.size(), cursor.position_in(bounds)?)?;
                state.dragging = true;
                Some(canvas::Action::publish(M::from(EditEvent::Press { index, add: state.ctrl })).and_capture())
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.dragging => {
                state.dragging = false;
                Some(canvas::Action::publish(M::from(EditEvent::Release)).and_capture())
            }
            _ => None,
        }
    }

    fn draw(&self, state: &EditState, renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let shown: Vec<Option<Point>> = self.points.iter().map(|p| project(&self.view_proj, bounds.size(), *p)).collect();
        let n = shown.len();
        let lines = Path::new(|path| {
            let pairs = if self.closed { n } else { n.saturating_sub(1) };
            for i in 0..pairs {
                if let (Some(a), Some(b)) = (shown[i], shown[(i + 1) % n]) {
                    path.move_to(a);
                    path.line_to(b);
                }
            }
        });
        frame.stroke(&lines, canvas::Stroke::default().with_color(self.ink).with_width(1.5));
        for (i, p) in shown.iter().enumerate() {
            let Some(p) = p else { continue };
            let half = if state.hover == Some(i) { 6.0 } else { 4.5 };
            let square = Path::rectangle(Point::new(p.x - half, p.y - half), Size::new(2.0 * half, 2.0 * half));
            frame.fill(&square, if self.picked[i] { self.ink } else { self.fill });
            frame.stroke(&square, canvas::Stroke::default().with_color(self.ink).with_width(1.5));
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, state: &EditState, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if state.dragging {
            return mouse::Interaction::Grabbing;
        }
        match cursor.position_in(bounds) {
            Some(p) if self.handle_at(bounds.size(), p).is_some() => mouse::Interaction::Grab,
            _ => mouse::Interaction::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bim_core::{LinearDef, PointDef, ProfileDef, SystemDef};
    use glam::DVec2;

    use crate::app::modeller::ModelTool;
    use crate::viewport::Navigate;

    /// A new model seen from the top, 10 px per metre, the origin in the middle of 1000 × 800.
    fn modelling() -> App {
        let mut app = App::default();
        app.new_model("Level 0");
        app.viewport_size = (1000.0, 800.0);
        app.camera.set_preset(crate::viewport::camera::ViewPreset::Top);
        app.camera.ortho = true;
        app.camera.target = Vec3::ZERO;
        app.camera.distance = 40.0 / (app.camera.fov_y * 0.5).tan();
        app
    }

    fn screen(app: &App, x: f64, y: f64, z: f64) -> (f32, f32) {
        app.on_screen(DVec3::new(x, y, z), 1000.0, 800.0).unwrap()
    }

    fn click(app: &mut App, x: f64, y: f64) {
        let (sx, sy) = screen(app, x, y, 0.0);
        let _ = app.update(Message::Navigate(Navigate::Pick { x: sx, y: sy, width: 1000.0, height: 800.0, add: false }));
    }

    /// Drags point `index` of the edited element to scene point (x, y) at height z.
    fn drag(app: &mut App, index: usize, x: f64, y: f64, z: f64) {
        let _ = app.update(Message::Edit(EditEvent::Press { index, add: false }));
        let (sx, sy) = screen(app, x, y, z);
        let _ = app.update(Message::Edit(EditEvent::Drag { x: sx, y: sy, width: 1000.0, height: 800.0 }));
        let _ = app.update(Message::Edit(EditEvent::Release));
    }

    /// An app with a wall from (0, 0) to (5, 0), selected, in edit mode.
    fn editing_a_wall() -> App {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 5.0, 0.0);
        app.set_model_tool(None);
        app.select_modelled(ElementId(1));
        let _ = app.update(Message::EditMode);
        assert!(app.modeller.edit.is_some());
        app
    }

    fn wall_ends(app: &App) -> (DVec2, DVec2) {
        let e = app.modeller.model.get(ElementId(1)).unwrap();
        let Shape::Planar(PlanarDef { form: PlanarForm::Wall { start, end, .. }, .. }) = e.shape_in_project(&app.modeller.model.grid) else { panic!() };
        (start, end)
    }

    #[test]
    fn tab_opens_and_closes_edit_mode_on_the_selected_element() {
        let mut app = editing_a_wall();
        assert!(app.wants_tab());
        let _ = app.update(Message::EditMode);
        assert!(app.modeller.edit.is_none());
        // nothing selected: Tab is not edit mode's
        app.sel.clear();
        assert!(!app.wants_tab());
        let _ = app.update(Message::EditMode);
        assert!(app.modeller.edit.is_none());
    }

    #[test]
    fn dragging_a_wall_end_moves_it_and_the_wall_follows() {
        let mut app = editing_a_wall();
        drag(&mut app, 1, 8.0, 2.0, 0.0);
        assert_eq!(wall_ends(&app), (DVec2::ZERO, DVec2::new(8.0, 2.0)));
        let volume = app.modeller.model.evaluate(ElementId(1)).unwrap()[0].solid.volume();
        assert!((volume - DVec2::new(8.0, 2.0).length() * 3.0 * 0.2).abs() < 1e-9);
        assert!(app.modeller.edit.as_ref().is_some_and(|e| e.grab.is_none()), "still in edit mode, the move put down");
    }

    #[test]
    fn a_move_the_kernel_refuses_leaves_the_element_as_it_was() {
        let mut app = editing_a_wall();
        drag(&mut app, 1, 0.0, 0.0, 0.0);
        assert_eq!(app.modeller.refusal, Some(Refusal::ZeroLength));
        assert_eq!(wall_ends(&app), (DVec2::ZERO, DVec2::new(5.0, 0.0)));
    }

    #[test]
    fn g_grabs_the_picked_points_and_a_click_puts_them_down_held_to_an_axis() {
        let mut app = editing_a_wall();
        // pick the end without moving it
        let _ = app.update(Message::Edit(EditEvent::Press { index: 1, add: false }));
        let _ = app.update(Message::Edit(EditEvent::Release));
        let _ = app.update(Message::EditKey('g'));
        assert!(app.edit_follows_cursor());
        let _ = app.update(Message::EditKey('x'));
        let (sx, sy) = screen(&app, 7.0, 3.0, 0.0);
        let _ = app.update(Message::Navigate(Navigate::Hover { x: sx, y: sy, width: 1000.0, height: 800.0 }));
        click(&mut app, 7.0, 3.0);
        assert_eq!(wall_ends(&app).1, DVec2::new(7.0, 0.0), "held to X: y stays");
    }

    #[test]
    fn esc_drops_a_move_then_leaves() {
        let mut app = editing_a_wall();
        let _ = app.update(Message::Edit(EditEvent::Press { index: 0, add: false }));
        let _ = app.update(Message::Edit(EditEvent::Release));
        let _ = app.update(Message::EditKey('g'));
        let _ = app.update(Message::Escape);
        assert!(app.modeller.edit.as_ref().is_some_and(|e| e.grab.is_none()));
        let _ = app.update(Message::Escape);
        assert!(app.modeller.edit.is_none());
        assert_eq!(wall_ends(&app), (DVec2::ZERO, DVec2::new(5.0, 0.0)));
    }

    #[test]
    fn a_drag_snaps_to_the_point_of_another_element() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Column, "Column")));
        click(&mut app, 6.0, 4.0);
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 5.0, 0.0);
        app.set_model_tool(None);
        let wall = ElementId(2);
        app.select_modelled(wall);
        let _ = app.update(Message::EditMode);
        // 0.3 m off the column is 3 px: the end lands on it
        drag(&mut app, 1, 6.2, 4.2, 0.0);
        let Shape::Planar(PlanarDef { form: PlanarForm::Wall { end, .. }, .. }) = app.modeller.model.get(wall).unwrap().shape_in_project(&app.modeller.model.grid) else { panic!() };
        assert_eq!(end, DVec2::new(6.0, 4.0));
    }

    #[test]
    fn a_slab_loses_its_picked_corners_down_to_three() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Slab, "Slab")));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 4.0, 3.0);
        app.set_model_tool(None);
        app.select_modelled(ElementId(1));
        let _ = app.update(Message::EditMode);
        let _ = app.update(Message::Edit(EditEvent::Press { index: 2, add: false }));
        let _ = app.update(Message::Edit(EditEvent::Release));
        let _ = app.update(Message::DeletePicked);
        let corners = |app: &App| edit_points(&app.modeller.model.get(ElementId(1)).unwrap().shape).0.len();
        assert_eq!(corners(&app), 3);
        assert!(app.modeller.edit.is_some(), "the slab stays in edit mode");
        let _ = app.update(Message::EditKey('a'));
        let _ = app.update(Message::DeletePicked);
        assert_eq!(app.modeller.refusal, Some(Refusal::TooFewPoints));
        assert_eq!(corners(&app), 3);
    }

    #[test]
    fn z_lifts_a_column_end() {
        let mut app = modelling();
        app.camera.set_preset(crate::viewport::camera::ViewPreset::Front);
        let id = app
            .add_element("Column", None, Shape::Linear(LinearDef { start: DVec3::ZERO, end: DVec3::new(0.0, 0.0, 3.0), profile: ProfileDef::Rectangle { width: 0.3, height: 0.3 }, roll: 0.0 }))
            .unwrap();
        app.select_modelled(id);
        let _ = app.update(Message::EditMode);
        let _ = app.update(Message::Edit(EditEvent::Press { index: 1, add: false }));
        let _ = app.update(Message::EditKey('z'));
        let (sx, sy) = screen(&app, 0.0, 0.0, 4.0);
        let _ = app.update(Message::Edit(EditEvent::Drag { x: sx, y: sy, width: 1000.0, height: 800.0 }));
        let _ = app.update(Message::Edit(EditEvent::Release));
        let Shape::Linear(l) = app.modeller.model.get(id).unwrap().shape_in_project(&app.modeller.model.grid) else { panic!() };
        assert!((l.end - DVec3::new(0.0, 0.0, 4.0)).length() < 1e-9, "{}", l.end);
    }

    #[test]
    fn every_shape_gives_its_points_back_unchanged() {
        let shapes = [
            Shape::Point(PointDef { at: DVec3::new(1.0, 2.0, 0.5), rotation: 0.3, size: DVec3::ONE }),
            Shape::Linear(LinearDef { start: DVec3::ZERO, end: DVec3::new(4.0, 0.0, 3.0), profile: ProfileDef::Rectangle { width: 0.2, height: 0.4 }, roll: 0.0 }),
            Shape::Planar(PlanarDef { form: PlanarForm::Wall { start: DVec2::ZERO, end: DVec2::new(5.0, 1.0), base: 3.0, height: 2.7 }, thickness: 0.2, openings: Vec::new(), build_up: None }),
            Shape::Planar(PlanarDef {
                form: PlanarForm::Slab { outline: vec![DVec2::ZERO, DVec2::new(4.0, 0.0), DVec2::new(4.0, 3.0)], holes: Vec::new(), top: 3.0 },
                thickness: 0.25,
                openings: Vec::new(),
                build_up: None,
            }),
            Shape::System(SystemDef { start: DVec2::ZERO, end: DVec2::new(6.0, 0.0), base: 0.0, height: 3.0, spacing: DVec2::splat(1.5), member: DVec2::new(0.05, 0.15), panel: 0.02, system_type: None, openings: Vec::new() }),
        ];
        for shape in shapes {
            let (points, _) = edit_points(&shape);
            assert_eq!(with_points(&shape, &points), shape);
        }
    }
}
