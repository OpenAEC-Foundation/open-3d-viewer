//! Shape handles of a wall and of a standing system: besides the grips at its ends and its
//! middle (`crate::app::grips`), a selected one has a grip at the middle of its top.
//!
//! - **Dragging it** up or down changes the element's height; its base stays. The drag runs along
//!   the vertical through the grip and is rounded as other moves are. A top that comes within
//!   [`LEVEL_SNAP`] of a level goes to that level.
//! - **Where it shows.** Only where it can be told from the middle grip: seen from straight
//!   above the two lie on one point of the screen, and the top grip is left out.
//! - **When it is put down** the kernel makes the element at its new height, with its corners and
//!   joins; a height it refuses leaves the element as it was and the banner says why.

use crate::app::edit::{edit_points, Axis, Edit, EditOverlay, Grab};
use crate::app::App;
use bim_core::element::PlanarForm;
use bim_core::{Element, ElementId, PlanarDef, Shape};
use glam::DVec3;
use iced::Color;

/// The grip of the top comes after the two ends and the middle.
pub(in crate::app) const TOP_GRIP: usize = 3;
/// A top dragged to within this of a level (metres) goes to the level.
const LEVEL_SNAP: f64 = 0.1;
/// The top grip shows when it lies at least this far from the middle grip on screen (logical
/// pixels).
const APART_PIXELS: f32 = 14.0;

/// The base and the height of a wall or a standing system; `None` for anything else.
fn stands(shape: &Shape) -> Option<(f64, f64)> {
    match shape {
        Shape::Planar(PlanarDef { form: PlanarForm::Wall { base, height, .. }, .. }) => Some((*base, *height)),
        Shape::System(s) if !s.lies() => Some((s.base, s.height)),
        _ => None,
    }
}

/// `shape` as high as `height`.
fn with_height(shape: &Shape, height: f64) -> Shape {
    let mut shape = shape.clone();
    match &mut shape {
        Shape::Planar(PlanarDef { form: PlanarForm::Wall { height: h, .. }, .. }) => *h = height,
        Shape::System(s) => s.height = height,
        _ => {}
    }
    shape
}

/// The middle of the top of a wall or a standing system (scene coordinates).
fn top_middle(shape: &Shape) -> Option<DVec3> {
    let (base, height) = stands(shape)?;
    let (ends, _) = edit_points(shape);
    let middle = (ends[0] + ends[1]) / 2.0;
    Some(DVec3::new(middle.x, middle.y, base + height))
}

impl App {
    /// The grip at the top of `shape` (the selected element's definition), where the viewport
    /// shows it apart from the middle grip.
    pub(in crate::app) fn height_grip(&self, shape: &Shape) -> Option<DVec3> {
        let top = top_middle(shape)?;
        let (base, _) = stands(shape)?;
        let (width, height) = (self.viewport_size.0.max(1.0), self.viewport_size.1.max(1.0));
        let (a, b) = (self.on_screen(DVec3::new(top.x, top.y, base), width, height)?, self.on_screen(top, width, height)?);
        ((a.0 - b.0).hypot(a.1 - b.1) >= APART_PIXELS).then_some(top)
    }

    /// A press on the top grip of selected element `element`: a drag of its height begins, along
    /// the vertical.
    pub(in crate::app) fn begin_height_drag(&mut self, element: ElementId) {
        let Some(shape) = self.modeller.model.get(element).map(|e| e.shape_in_project(&self.modeller.model.grid)) else { return };
        let Some(top) = top_middle(&shape) else { return };
        let (from, _) = edit_points(&shape);
        // no point picked: the element's points stay, only how far the top went is kept
        self.modeller.edit = Some(Edit { element, picked: Vec::new(), grab: Some(Grab { from, origin: top, delta: DVec3::ZERO, axis: Some(Axis::Z), by_key: false }), grips: true });
        self.modeller.refusal = None;
    }

    /// How far a drag of the top grip under way has moved the top; `None` when no such drag is.
    fn height_drag(&self) -> Option<f64> {
        let edit = self.modeller.edit.as_ref().filter(|e| e.grips && e.picked.is_empty())?;
        let grab = edit.grab.as_ref().filter(|g| g.axis == Some(Axis::Z))?;
        Some(grab.delta.z)
    }

    /// The height the dragged element gets: its own and how far the top went, its top on a level
    /// when it comes near one.
    fn dragged_height(&self, shape: &Shape, dz: f64) -> Option<f64> {
        let (base, height) = stands(shape)?;
        let top = base + height + dz;
        let level = self.document.levels().iter().map(|l| l.elevation).filter(|z| (z - top).abs() <= LEVEL_SNAP && *z > base).min_by(|a, b| (a - top).abs().total_cmp(&(b - top).abs()));
        Some(level.unwrap_or(top) - base)
    }

    /// Puts a drag of the top grip down: the element takes its new height, if the kernel can make
    /// it. False when no such drag was under way.
    pub(in crate::app) fn put_height_down(&mut self) -> bool {
        let Some(dz) = self.height_drag() else { return false };
        let edited = self.edited().map(|(e, s)| (e.id, e.name.clone(), e.level.clone(), s));
        self.modeller.edit = None;
        let Some((id, name, level, shape)) = edited.filter(|_| dz != 0.0) else { return true };
        let Some(height) = self.dragged_height(&shape, dz) else { return true };
        let grid = self.modeller.model.grid;
        let mut element = Element::in_project(&grid, name, level, with_height(&shape, height));
        element.id = id;
        match self.modeller.model.replace(element) {
            Ok(()) => {
                self.modeller.refusal = None;
                self.joins_follow(&[id]);
                self.show_modelled();
                self.select_modelled(id);
            }
            Err(refusal) => self.modeller.refusal = Some(refusal),
        }
        true
    }

    /// The grips while the top grip is dragged: the ends and the middle where they are, the top
    /// where the drag has it. `None` when no such drag is under way.
    pub(in crate::app) fn height_drag_overlay(&self, ink: Color, fill: Color) -> Option<EditOverlay> {
        let dz = self.height_drag()?;
        let (_, shape) = self.edited()?;
        let (base, _) = stands(&shape)?;
        let (mut points, _) = edit_points(&shape);
        let middle = (points[0] + points[1]) / 2.0;
        points.push(middle);
        points.push(DVec3::new(middle.x, middle.y, base + self.dragged_height(&shape, dz)?));
        Some(EditOverlay {
            picked: (0..points.len()).map(|i| i == TOP_GRIP).collect(),
            points: points.iter().map(|p| p.as_vec3()).collect(),
            closed: false,
            view_proj: self.camera.view_proj(self.aspect()) * self.model_matrix(),
            ink,
            fill,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::modelling;
    use super::*;
    use crate::app::edit::EditEvent;
    use crate::app::Message;
    use glam::{DVec2, Vec3};

    /// A wall 4 m long and 3 m high on the ground, selected, seen from the front and above.
    fn wall_selected() -> (App, ElementId) {
        let mut app = modelling();
        let form = PlanarForm::Wall { start: DVec2::new(0.0, 0.0), end: DVec2::new(4.0, 0.0), base: 0.0, height: 3.0 };
        let id = app.add_element("Wall", None, Shape::Planar(PlanarDef { form, thickness: 0.2, openings: Vec::new(), build_up: None })).unwrap();
        // placing it is a step of its own, as it is when a tool places it
        app.keep_model_history();
        app.select_modelled(id);
        (app, id)
    }

    fn height_of(app: &App, id: ElementId) -> f64 {
        stands(&app.modeller.model.get(id).unwrap().shape_in_project(&app.modeller.model.grid)).unwrap().1
    }

    /// Drags the top grip to scene height `z` (above the wall's middle) and lets go.
    fn drag_top(app: &mut App, z: f64) {
        let (sx, sy) = app.on_screen(DVec3::new(2.0, 0.0, z), 1000.0, 800.0).unwrap();
        let _ = app.update(Message::Edit(EditEvent::Press { index: TOP_GRIP, add: false }));
        let _ = app.update(Message::Edit(EditEvent::Drag { x: sx, y: sy, width: 1000.0, height: 800.0 }));
        let _ = app.update(Message::Edit(EditEvent::Release));
    }

    #[test]
    fn the_top_grip_shows_where_it_stands_apart_and_drags_the_height() {
        let (mut app, id) = wall_selected();
        // from straight above the top lies on the middle: three grips
        assert_eq!(app.grip_overlay(Color::BLACK, Color::WHITE).unwrap().points.len(), 3);
        // from the front: a fourth, at the middle of the top
        app.camera.set_preset(crate::viewport::camera::ViewPreset::Front);
        let grips = app.grip_overlay(Color::BLACK, Color::WHITE).unwrap();
        assert_eq!(grips.points.len(), 4);
        assert_eq!(grips.points[TOP_GRIP], Vec3::new(2.0, 0.0, 3.0));
        // dragged up to 3.5 m: the wall is that high, its base and its ends as they were
        drag_top(&mut app, 3.5);
        assert!((height_of(&app, id) - 3.5).abs() < 1e-9, "{}", height_of(&app, id));
        let (ends, _) = edit_points(&app.modeller.model.get(id).unwrap().shape_in_project(&app.modeller.model.grid));
        assert_eq!((ends[0], ends[1]), (DVec3::ZERO, DVec3::new(4.0, 0.0, 0.0)));
        assert!(app.modeller.edit.is_none());
        assert_eq!(app.selected_modelled(), vec![id], "it stays selected");
        // one step to take back
        let _ = app.update(Message::Undo);
        assert!((height_of(&app, id) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn a_top_near_a_level_goes_to_it_and_a_height_refused_changes_nothing() {
        let (mut app, id) = wall_selected();
        let _ = app.document.add_level("Level 1", 2.8);
        app.camera.set_preset(crate::viewport::camera::ViewPreset::Front);
        // 2.76 m is within reach of the level at 2.8 m
        drag_top(&mut app, 2.76);
        assert!((height_of(&app, id) - 2.8).abs() < 1e-9, "{}", height_of(&app, id));
        // below its own base: refused, the wall as it was
        drag_top(&mut app, -1.0);
        assert!((height_of(&app, id) - 2.8).abs() < 1e-9);
        assert!(app.modeller.refusal.is_some());
        assert!(app.modeller.edit.is_none());
    }
}
