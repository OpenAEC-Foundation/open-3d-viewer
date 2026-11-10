//! Grips, as in Revit: a selected wall or curtain wall shows a square at each end of its axis and
//! one in its middle, without edit mode. Dragging an end moves that end; dragging the middle moves
//! the whole element. The move snaps and is rounded as in edit mode, X and Y hold it to an axis,
//! and Esc drops it.
//!
//! When it is put down, the walls and curtain walls joined to the moved one follow:
//! - an end that met a moved end (a corner) moves with it;
//! - an end that lay on the moved axis, between its ends (a T-junction), slides along its own line
//!   to where it meets the moved axis.
//!
//! The kernel then cuts their corners again. If it refuses any of them, nothing moves and the
//! banner says why.

use super::edit::{edit_points, with_points, Edit, EditOverlay};
use super::App;
use bim_core::element::PlanarForm;
use bim_core::{Element, ElementId, PlanarDef, Shape};
use glam::{DVec2, DVec3};
use iced::Color;

/// Ends this close in plan meet (metres), as the kernel's wall corners take them.
const MEET: f64 = 1e-3;

/// How far either side of its axis an element with grips reaches: half a wall's thickness, half a
/// curtain wall's member. `None` for an element without grips (anything but those two).
fn half_width(shape: &Shape) -> Option<f64> {
    match shape {
        Shape::Planar(p @ PlanarDef { form: PlanarForm::Wall { .. }, .. }) => Some(p.total_thickness() / 2.0),
        Shape::System(s) => Some(s.member.max_element() / 2.0),
        _ => None,
    }
}

/// Where the line through `a` and `b` meets the line through `c` and `d` (in plan), unless they
/// run (nearly) parallel.
fn crossing(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> Option<DVec2> {
    let (r, s) = (b - a, d - c);
    let denom = r.perp_dot(s);
    if denom.abs() < 1e-9 * r.length() * s.length() {
        return None;
    }
    Some(a + r * ((c - a).perp_dot(s) / denom))
}

impl App {
    /// The selected element with grips: one alone, outside edit mode and with no tool on.
    fn gripped(&self) -> Option<ElementId> {
        if self.modeller.edit.is_some() || self.modeller.tool.is_some() || self.measure.tool.is_some() || self.plane_picking {
            return None;
        }
        let [id] = self.selected_modelled()[..] else { return None };
        half_width(&self.modeller.model.get(id)?.shape).map(|_| id)
    }

    /// The grips of the selected element over the viewport: its ends, then its middle. A wall of
    /// an opened file that the Modeller can take over has them too (see `take_over`).
    pub(super) fn grip_overlay(&self, ink: Color, fill: Color) -> Option<EditOverlay> {
        let shape = match self.gripped() {
            Some(id) => self.modeller.model.get(id)?.shape_in_project(&self.modeller.model.grid),
            None => Shape::Planar(self.source_gripped()?.wall),
        };
        let (mut points, _) = edit_points(&shape);
        points.push((points[0] + points[1]) / 2.0);
        Some(EditOverlay {
            picked: vec![false; points.len()],
            points: points.iter().map(|p| p.as_vec3()).collect(),
            closed: false,
            view_proj: self.camera.view_proj(self.aspect()) * self.model_matrix(),
            ink,
            fill,
        })
    }

    /// A press on grip `index` of the selected element: an end, or (past them) the middle, which
    /// moves both.
    pub(super) fn grip_press(&mut self, index: usize) {
        if self.gripped().is_none() {
            // a wall of an opened file: taken over first, then dragged as a modelled wall
            let Some(source) = self.source_gripped() else { return };
            if self.take_over(source).is_none() {
                return;
            }
        }
        let Some(element) = self.gripped() else { return };
        let picked = if index < 2 { vec![index] } else { vec![0, 1] };
        self.modeller.edit = Some(Edit { element, picked, grab: None, grips: true });
        self.begin_grab(index, false);
    }

    /// Puts a grip's move down, the element's points now at `points`: it and the walls joined to
    /// it take their new places, if the kernel can make every one; else nothing moves.
    pub(super) fn put_grip_down(&mut self, points: Vec<DVec3>, moved: bool) {
        let edited = self.edited().map(|(e, s)| (e.id, s));
        self.modeller.edit = None;
        let Some((id, shape)) = edited.filter(|_| moved) else { return };
        let (before, _) = edit_points(&shape);
        let mut changes = vec![(id, with_points(&shape, &points))];
        changes.extend(self.followers(id, &shape, [before[0], before[1]], [points[0], points[1]]));
        let kept = self.modeller.model.clone();
        let grid = self.modeller.model.grid;
        for (e, shape) in changes {
            let Some(old) = self.modeller.model.get(e) else { continue };
            let mut element = Element::in_project(&grid, old.name.clone(), old.level.clone(), shape);
            element.id = e;
            if let Err(refusal) = self.modeller.model.replace(element) {
                self.modeller.model = kept;
                self.modeller.refusal = Some(refusal);
                return;
            }
        }
        self.modeller.refusal = None;
        self.show_modelled();
        self.select_modelled(id);
    }

    /// The walls and curtain walls joined to element `id` (definition `shape`, its axis ends moved
    /// from `from` to `to`) with their ends moved to follow it, scene coordinates.
    fn followers(&self, id: ElementId, shape: &Shape, from: [DVec3; 2], to: [DVec3; 2]) -> Vec<(ElementId, Shape)> {
        let grid = self.modeller.model.grid;
        let half = half_width(shape).unwrap_or(0.0);
        let (a, b) = (from[0].truncate(), from[1].truncate());
        let length = a.distance(b);
        let mut out = Vec::new();
        for e in self.modeller.model.iter().filter(|e| e.id != id) {
            let theirs = e.shape_in_project(&grid);
            if half_width(&theirs).is_none() {
                continue;
            }
            let (mut ends, _) = edit_points(&theirs);
            let mut changed = false;
            for k in 0..2 {
                let q = ends[k].truncate();
                let moved_to = if let Some(i) = (0..2).find(|&i| q.distance(from[i].truncate()) <= MEET) {
                    // a corner: the end goes with the moved end
                    Some(to[i].truncate())
                } else {
                    // a T-junction: the end lies on the axis, away from its ends, within its half width
                    let t = (q - a).dot(b - a) / (length * length).max(1e-300);
                    let on = t * length > MEET && (1.0 - t) * length > MEET && (a + (b - a) * t).distance(q) <= half + MEET;
                    let other = ends[1 - k].truncate();
                    if on { crossing(other, q, to[0].truncate(), to[1].truncate()) } else { None }
                };
                if let Some(p) = moved_to.filter(|p| p.distance(q) > 1e-9) {
                    ends[k] = p.extend(ends[k].z);
                    changed = true;
                }
            }
            if changed {
                out.push((e.id, with_points(&theirs, &ends)));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::edit::EditEvent;
    use crate::app::modeller::ModelTool;
    use crate::app::Message;
    use crate::viewport::Navigate;
    use bim_core::Refusal;
    use glam::Vec3;

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

    fn click(app: &mut App, x: f64, y: f64) {
        let (sx, sy) = app.on_screen(DVec3::new(x, y, 0.0), 1000.0, 800.0).unwrap();
        let _ = app.update(Message::Navigate(Navigate::Pick { x: sx, y: sy, width: 1000.0, height: 800.0, add: false }));
    }

    /// Walls drawn through `points`, one after the other; the tool is off after.
    fn walls(app: &mut App, points: &[(f64, f64)]) {
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        for &(x, y) in points {
            click(app, x, y);
        }
        app.set_model_tool(None);
    }

    /// Drags grip `index` of the selected element to scene point (x, y).
    fn drag(app: &mut App, index: usize, x: f64, y: f64) {
        let _ = app.update(Message::Edit(EditEvent::Press { index, add: false }));
        let (sx, sy) = app.on_screen(DVec3::new(x, y, 0.0), 1000.0, 800.0).unwrap();
        let _ = app.update(Message::Edit(EditEvent::Drag { x: sx, y: sy, width: 1000.0, height: 800.0 }));
        let _ = app.update(Message::Edit(EditEvent::Release));
    }

    fn ends(app: &App, id: u64) -> (DVec2, DVec2) {
        let (p, _) = edit_points(&app.modeller.model.get(ElementId(id)).unwrap().shape_in_project(&app.modeller.model.grid));
        (p[0].truncate(), p[1].truncate())
    }

    #[test]
    fn a_selected_wall_has_grips_and_dragging_an_end_moves_it_without_edit_mode() {
        let mut app = modelling();
        walls(&mut app, &[(0.0, 0.0), (5.0, 0.0)]);
        assert!(app.grip_overlay(Color::BLACK, Color::WHITE).is_none(), "nothing selected");
        app.select_modelled(ElementId(1));
        let grips = app.grip_overlay(Color::BLACK, Color::WHITE).expect("grips");
        assert_eq!(grips.points, vec![Vec3::ZERO, Vec3::new(5.0, 0.0, 0.0), Vec3::new(2.5, 0.0, 0.0)]);
        drag(&mut app, 1, 7.0, 2.0);
        assert_eq!(ends(&app, 1), (DVec2::ZERO, DVec2::new(7.0, 2.0)));
        assert!(app.modeller.edit.is_none(), "no edit mode after");
        assert_eq!(app.selected_modelled(), vec![ElementId(1)], "the wall stays selected, its grips at its new ends");
        assert_eq!(app.grip_overlay(Color::BLACK, Color::WHITE).unwrap().points[1], Vec3::new(7.0, 2.0, 0.0));
    }

    #[test]
    fn the_middle_grip_moves_the_wall_and_the_walls_joined_to_it_follow() {
        let mut app = modelling();
        // three walls in a U, open to the south: 1 west, 2 north, 3 east
        walls(&mut app, &[(0.0, 0.0), (0.0, 4.0), (6.0, 4.0), (6.0, 0.0)]);
        app.select_modelled(ElementId(2));
        drag(&mut app, 2, 3.0, 5.0);
        assert_eq!(ends(&app, 2), (DVec2::new(0.0, 5.0), DVec2::new(6.0, 5.0)), "the north wall a metre north");
        assert_eq!(ends(&app, 1), (DVec2::ZERO, DVec2::new(0.0, 5.0)), "the corners go with it");
        assert_eq!(ends(&app, 3), (DVec2::new(6.0, 5.0), DVec2::new(6.0, 0.0)));
    }

    #[test]
    fn a_wall_ending_on_the_moved_one_slides_along_its_own_line_to_meet_it() {
        let mut app = modelling();
        walls(&mut app, &[(0.0, 0.0), (6.0, 0.0)]);
        walls(&mut app, &[(3.0, 0.0), (3.0, 4.0)]);
        // the second starts on the first's axis: a T-junction
        app.select_modelled(ElementId(1));
        drag(&mut app, 2, 3.0, -2.0);
        assert_eq!(ends(&app, 1), (DVec2::new(0.0, -2.0), DVec2::new(6.0, -2.0)));
        assert_eq!(ends(&app, 2), (DVec2::new(3.0, -2.0), DVec2::new(3.0, 4.0)));
        // turning the first wall about its start: the T follows its new line
        drag(&mut app, 1, 6.0, 2.0);
        let (start, _) = ends(&app, 2);
        assert!((start - DVec2::new(3.0, 0.0)).length() < 1e-9, "{start}");
    }

    #[test]
    fn a_grip_move_the_kernel_refuses_moves_nothing() {
        let mut app = modelling();
        walls(&mut app, &[(0.0, 0.0), (5.0, 0.0), (5.0, 4.0)]);
        app.select_modelled(ElementId(1));
        // the first wall's end onto its start: no length
        drag(&mut app, 1, 0.0, 0.0);
        assert_eq!(app.modeller.refusal, Some(Refusal::ZeroLength));
        assert_eq!(ends(&app, 1), (DVec2::ZERO, DVec2::new(5.0, 0.0)));
        assert_eq!(ends(&app, 2), (DVec2::new(5.0, 0.0), DVec2::new(5.0, 4.0)), "the joined wall stays too");
        assert!(app.modeller.edit.is_none());
    }

    #[test]
    fn esc_drops_a_grip_drag_and_tab_still_opens_edit_mode() {
        let mut app = modelling();
        walls(&mut app, &[(0.0, 0.0), (5.0, 0.0)]);
        app.select_modelled(ElementId(1));
        let _ = app.update(Message::Edit(EditEvent::Press { index: 1, add: false }));
        assert!(app.modeller.edit.as_ref().is_some_and(|e| e.grips));
        let _ = app.update(Message::Escape);
        assert!(app.modeller.edit.is_none());
        assert_eq!(ends(&app, 1), (DVec2::ZERO, DVec2::new(5.0, 0.0)));
        // Tab is still edit mode for the wall
        assert!(app.wants_tab());
        let _ = app.update(Message::EditMode);
        assert!(app.modeller.edit.as_ref().is_some_and(|e| !e.grips));
        assert!(app.grip_overlay(Color::BLACK, Color::WHITE).is_none(), "edit mode shows its own points");
    }
}
