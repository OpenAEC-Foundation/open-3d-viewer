//! The 3D cursor of the Modeller, as in Blender and in the reference modeller: a point in the
//! model that tools start from and turn about.
//!
//! - **Placing it.** Shift and the right button put it under the pointer, and a drag with them
//!   moves it along: on a point of a modelled element near the pointer, else on the level worked
//!   on, rounded as a tool's click is. Shift+C takes it home, to the origin.
//! - **How it shows.** A red and white ring with a cross, the same size at any zoom.
//! - **A snap point.** Once placed (not at home), a tool's click near it lands on it.
//! - **Fresh.** A cursor moved since it was last used is the first point of the next wall, beam
//!   or system drawn, and the base point of the next move or copy; that spends it. One taken
//!   home is not fresh.
//! - **Rotate.** Enter while Rotate waits for its centre turns about the cursor, fresh or not.

use crate::app::transform::TransformKind;
use crate::app::App;
use glam::{DVec3, Mat4, Vec3};
use iced::widget::canvas::{self, Frame, Path};
use iced::{mouse, Color, Point, Rectangle};

/// The ring's radius and how far the cross reaches from its middle (logical pixels).
const RING: f32 = 10.0;
const CROSS: (f32, f32) = (5.0, 16.0);
/// The ring is drawn in this many stretches, red and white in turn.
const STRETCHES: usize = 16;

/// The 3D cursor: where it is (scene coordinates), whether the user placed it (at home it is no
/// snap point), and whether it moved since a tool last used it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Cursor3d {
    pub at: DVec3,
    pub placed: bool,
    pub fresh: bool,
}

impl App {
    /// Whether the 3D cursor is there to place and to see: in the Modeller, on a modelled project.
    pub(crate) fn has_cursor3d(&self) -> bool {
        self.modeller.in_modeller && self.modeller.shown.is_some()
    }

    /// Shift and the right button at viewport point (x, y): the 3D cursor goes there.
    pub(in crate::app) fn place_cursor3d(&mut self, x: f32, y: f32, width: f32, height: f32) {
        if !self.has_cursor3d() {
            return;
        }
        let (width, height) = (width.max(1.0), height.max(1.0));
        // where it was is no point for it to land on: it must move a little too
        self.modeller.cursor3d.placed = false;
        let (z, ..) = self.work_plane();
        // a point of an element near the pointer, at that point's own height
        let near = self
            .snap_points()
            .into_iter()
            .filter_map(|q| self.on_screen(q, width, height).map(|s| (q, (s.0 - x).hypot(s.1 - y))))
            .filter(|(_, d)| *d <= 12.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(q, _)| q);
        let at = near.or_else(|| self.plane_point(x, y, width, height, z).map(|p| self.snapped(p, x, y, width, height)));
        if let Some(at) = at {
            self.modeller.cursor3d.at = at;
        }
        self.modeller.cursor3d.placed = true;
        self.modeller.cursor3d.fresh = true;
    }

    /// Shift+C: the 3D cursor goes home, to the origin. One taken home starts no tool.
    pub(in crate::app) fn cursor3d_home(&mut self) {
        self.modeller.cursor3d = Cursor3d::default();
    }

    /// The 3D cursor as a snap point: once placed.
    pub(in crate::app) fn cursor3d_snap(&self) -> Option<DVec3> {
        let c = self.modeller.cursor3d;
        (self.modeller.in_modeller && c.placed).then_some(c.at)
    }

    /// The 3D cursor's place on the level worked on, when it moved since it was last used; it is
    /// spent by the asking.
    fn fresh_cursor3d(&mut self) -> Option<DVec3> {
        let c = &mut self.modeller.cursor3d;
        if !std::mem::take(&mut c.fresh) {
            return None;
        }
        let at = c.at;
        let (z, ..) = self.work_plane();
        Some(DVec3::new(at.x, at.y, z))
    }

    /// A tool that draws from point to point was switched on: a fresh 3D cursor is its first
    /// point.
    pub(in crate::app) fn tool_from_cursor3d(&mut self) {
        let chains = self.modeller.tool.is_some_and(|(tool, _)| tool.chains());
        if chains && self.modeller.points.is_empty() {
            if let Some(p) = self.fresh_cursor3d() {
                self.modeller.points.push(p);
            }
        }
    }

    /// A move or a copy began: a fresh 3D cursor is its base point.
    pub(in crate::app) fn transform_from_cursor3d(&mut self) {
        let moves = self.modeller.transform.as_ref().is_some_and(|t| matches!(t.kind, TransformKind::Move | TransformKind::Copy) && t.points.is_empty());
        if !moves {
            return;
        }
        if let (Some(p), Some(t)) = (self.fresh_cursor3d(), self.modeller.transform.as_mut()) {
            t.points.push(p);
        }
    }

    /// Enter while Rotate waits for its centre: it turns about the 3D cursor. False when no
    /// rotate waits for one.
    pub(in crate::app) fn rotate_about_cursor3d(&mut self) -> bool {
        let at = self.modeller.cursor3d.at;
        let (z, ..) = self.work_plane();
        let Some(t) = self.modeller.transform.as_mut().filter(|t| t.kind == TransformKind::Rotate && t.points.is_empty()) else { return false };
        t.points.push(DVec3::new(at.x, at.y, z));
        self.modeller.cursor3d.fresh = false;
        true
    }

    /// The 3D cursor over the viewport.
    pub(in crate::app) fn cursor3d_overlay(&self) -> Option<Cursor3dMark> {
        self.has_cursor3d().then(|| Cursor3dMark { at: self.modeller.cursor3d.at.as_vec3(), view_proj: self.camera.view_proj(self.aspect()) * self.model_matrix() })
    }
}

/// The 3D cursor drawn over the viewport: a red and white ring with a cross.
pub struct Cursor3dMark {
    pub at: Vec3,
    /// Model to clip space.
    pub view_proj: Mat4,
}

impl<M> canvas::Program<M> for Cursor3dMark {
    type State = ();

    fn draw(&self, _state: &(), renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let c = self.view_proj.project_point3(self.at);
        if !(0.0..=1.0).contains(&c.z) {
            return vec![frame.into_geometry()];
        }
        let middle = Point::new((c.x + 1.0) / 2.0 * bounds.width, (1.0 - c.y) / 2.0 * bounds.height);
        let on_ring = |k: usize| {
            let angle = std::f32::consts::TAU * k as f32 / STRETCHES as f32;
            Point::new(middle.x + RING * angle.cos(), middle.y + RING * angle.sin())
        };
        for k in 0..STRETCHES {
            let colour = if k % 2 == 0 { Color::from_rgb8(220, 40, 40) } else { Color::WHITE };
            frame.stroke(&Path::line(on_ring(k), on_ring(k + 1)), canvas::Stroke::default().with_color(colour).with_width(2.0));
        }
        let ink = Color::from_rgb8(30, 30, 30);
        for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            let arm = Path::line(Point::new(middle.x + dx * CROSS.0, middle.y + dy * CROSS.0), Point::new(middle.x + dx * CROSS.1, middle.y + dy * CROSS.1));
            frame.stroke(&arm, canvas::Stroke::default().with_color(ink).with_width(1.0));
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::click;
    use super::super::ModelTool;
    use super::*;
    use crate::app::edit::edit_points;
    use crate::app::Message;
    use crate::viewport::Navigate;
    use bim_core::Shape;
    use glam::DVec2;

    /// A new model in the Modeller, seen from above.
    fn modelling() -> App {
        let mut app = super::super::tests::modelling();
        app.modeller.in_modeller = true;
        app
    }

    /// Shift and the right button at scene point (x, y) on the ground.
    fn place(app: &mut App, x: f64, y: f64) {
        let (sx, sy) = app.on_screen(DVec3::new(x, y, 0.0), 1000.0, 800.0).unwrap();
        let _ = app.update(Message::Navigate(Navigate::Cursor3d { x: sx, y: sy, width: 1000.0, height: 800.0 }));
    }

    fn ends(app: &App, id: u64) -> (DVec2, DVec2) {
        let (p, _) = edit_points(&app.modeller.model.get(bim_core::ElementId(id)).unwrap().shape_in_project(&app.modeller.model.grid));
        (p[0].truncate(), p[1].truncate())
    }

    #[test]
    fn the_cursor_is_placed_shows_and_goes_home() {
        let mut app = modelling();
        assert_eq!(app.modeller.cursor3d, Cursor3d::default());
        assert!(app.cursor3d_overlay().is_some(), "it shows at home too");
        assert!(app.cursor3d_snap().is_none(), "at home it is no snap point");
        place(&mut app, 3.004, 2.0);
        assert_eq!(app.modeller.cursor3d, Cursor3d { at: DVec3::new(3.0, 2.0, 0.0), placed: true, fresh: true });
        assert_eq!(app.cursor3d_overlay().map(|m| m.at), Some(Vec3::new(3.0, 2.0, 0.0)));
        // dragged a little, it moves: where it was does not hold it
        place(&mut app, 3.2, 2.0);
        assert_eq!(app.modeller.cursor3d.at, DVec3::new(3.2, 2.0, 0.0));
        let _ = app.update(Message::Cursor3dHome);
        assert_eq!(app.modeller.cursor3d, Cursor3d::default());
        // not in the Viewer
        app.modeller.in_modeller = false;
        place(&mut app, 3.0, 2.0);
        assert_eq!(app.modeller.cursor3d, Cursor3d::default());
        assert!(app.cursor3d_overlay().is_none());
    }

    #[test]
    fn a_fresh_cursor_is_the_first_point_of_the_next_wall_and_a_snap_point_after() {
        let mut app = modelling();
        place(&mut app, 2.0, 1.0);
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        assert_eq!(app.modeller.points, vec![DVec3::new(2.0, 1.0, 0.0)]);
        assert!(!app.modeller.cursor3d.fresh, "spent");
        click(&mut app, 6.0, 1.0);
        assert_eq!(ends(&app, 1), (DVec2::new(2.0, 1.0), DVec2::new(6.0, 1.0)));
        // the tool taken again starts where the user clicks: the cursor is spent
        app.set_model_tool(None);
        app.set_model_tool(Some((ModelTool::Beam, "Beam")));
        assert!(app.modeller.points.is_empty());
        app.set_model_tool(None);
        // a column has one click: the cursor does not place it by itself
        place(&mut app, 7.0, 5.0);
        app.set_model_tool(Some((ModelTool::Column, "Column")));
        assert!(app.modeller.points.is_empty() && app.modeller.cursor3d.fresh);
        // but a click near the cursor lands on it: 0.3 m off is 3 px
        click(&mut app, 7.3, 5.2);
        let column = app.modeller.model.iter().find(|e| matches!(e.shape, Shape::Linear(_))).expect("a column");
        let Shape::Linear(c) = column.shape_in_project(&app.modeller.model.grid) else { panic!() };
        assert_eq!(c.start.truncate(), DVec2::new(7.0, 5.0));
    }

    #[test]
    fn a_fresh_cursor_is_the_base_of_a_move_and_rotate_turns_about_it_on_enter() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 4.0, 0.0);
        app.set_model_tool(None);
        app.select_modelled(bim_core::ElementId(1));
        // the cursor on the wall's end; Move goes from there with one click
        place(&mut app, 4.0, 0.0);
        app.begin_transform(TransformKind::Move);
        assert_eq!(app.modeller.transform.as_ref().map(|t| t.points.len()), Some(1));
        click(&mut app, 4.0, 2.0);
        assert_eq!(ends(&app, 1), (DVec2::new(0.0, 2.0), DVec2::new(4.0, 2.0)));
        // a second move asks for its base again
        app.begin_transform(TransformKind::Move);
        assert_eq!(app.modeller.transform.as_ref().map(|t| t.points.len()), Some(0));
        let _ = app.update(Message::Escape);
        // Rotate, Enter: about the cursor at (4, 0); from east to north is a quarter turn
        app.select_modelled(bim_core::ElementId(1));
        app.begin_transform(TransformKind::Rotate);
        let _ = app.update(Message::MeasureFinish);
        assert_eq!(app.modeller.transform.as_ref().map(|t| t.points.clone()), Some(vec![DVec3::new(4.0, 0.0, 0.0)]));
        click(&mut app, 6.0, 0.0);
        click(&mut app, 4.0, 3.0);
        let (a, b) = ends(&app, 1);
        assert!((a - DVec2::new(2.0, -4.0)).length() < 1e-9 && (b - DVec2::new(2.0, 0.0)).length() < 1e-9, "{a} {b}");
    }
}
