//! The preview of what a modelling tool places. While a tool is on, the element the next click
//! would make is drawn over the viewport at its full size: from the points clicked so far to the
//! point under the cursor, snapped as that click would be. A column or an object shows under the
//! cursor before any click; a wall, beam, slab or curtain wall from its first click on.
//!
//! The preview is the kernel's own evaluation of that element on its own, so it has the thickness,
//! height and layers the placed element gets; its corners with other walls show once it is placed.

use super::App;
use bim_core::{Element, Shape};
use glam::{Mat4, Vec3};
use iced::widget::canvas::{self, Frame, Path};
use iced::{mouse, Color, Point, Rectangle, Size};

impl App {
    /// The cursor rests at (x, y) in a viewport of width × height while a tool is on: the preview
    /// follows it. False when no tool is on.
    pub(super) fn model_hover(&mut self, x: f32, y: f32, width: f32, height: f32) -> bool {
        let Some((tool, _)) = self.modeller.tool else { return false };
        let (width, height) = (width.max(1.0), height.max(1.0));
        if tool.opens() {
            // a door or window aims at the wall under the cursor
            self.modeller.aim = self.opening_at(tool, x, y, width, height);
            self.modeller.cursor = self.modeller.aim.as_ref().map(|a| a.hit);
            return true;
        }
        let (z, _, _) = self.work_plane();
        self.modeller.cursor = self.plane_point(x, y, width, height, z).map(|p| self.snapped(p, x, y, width, height));
        true
    }

    /// The element the tool would place with a click where the cursor is (scene coordinates).
    pub(crate) fn placing(&self) -> Option<Shape> {
        let (tool, _) = self.modeller.tool.filter(|(t, _)| !t.opens())?;
        let cursor = self.modeller.cursor?;
        if self.modeller.points.len() + 1 < tool.clicks() {
            return None;
        }
        let first = self.modeller.points.first().copied().unwrap_or(cursor);
        Some(self.shape_for(tool, first, cursor))
    }

    /// The preview over the viewport: the faces of the element [`App::placing`] gives, the points
    /// clicked so far and the cursor's point. `None` when no tool is on or the cursor is away.
    pub(super) fn placing_overlay(&self, ink: Color) -> Option<PlacingOverlay> {
        if self.modeller.grid_tool {
            return self.grid_overlay(ink);
        }
        self.modeller.tool?;
        let cursor = self.modeller.cursor?;
        let mut faces = Vec::new();
        if let Some(aim) = &self.modeller.aim {
            faces = aim.faces();
        }
        if let Some(shape) = self.placing() {
            let grid = self.modeller.model.grid;
            let mut alone = bim_core::Model::new(grid);
            // an element the kernel refuses (a wall of no length yet) shows no body
            if let Ok(parts) = alone.add(Element::in_project(&grid, "", None, shape)).and_then(|id| alone.evaluate(id)) {
                for part in &parts {
                    let origin = grid.origin(part.tile);
                    let at = |i: &u32| (origin + part.solid.positions[*i as usize]).as_vec3();
                    faces.extend(part.solid.faces.iter().map(|f| f.rings.iter().map(|ring| ring.iter().map(at).collect()).collect()));
                }
            }
        }
        let points = self.modeller.points.iter().map(|p| p.as_vec3()).collect();
        Some(PlacingOverlay { faces, points, cursor: cursor.as_vec3(), view_proj: self.camera.view_proj(self.aspect()) * self.model_matrix(), ink })
    }
}

/// The preview of the element being placed, drawn over the viewport; it takes no events.
pub struct PlacingOverlay {
    /// The element's faces, each its rings (the outer first), in model coordinates.
    pub faces: Vec<Vec<Vec<Vec3>>>,
    /// The points clicked so far.
    pub points: Vec<Vec3>,
    /// The point a click would place.
    pub cursor: Vec3,
    /// Model to clip space.
    pub view_proj: Mat4,
    pub ink: Color,
}

impl PlacingOverlay {
    /// Where a model point shows in a viewport of `size`, and its depth, if in front of the camera.
    fn project(&self, size: Size, p: Vec3) -> Option<(Point, f32)> {
        let c = self.view_proj.project_point3(p);
        ((0.0..=1.0).contains(&c.z)).then(|| (Point::new((c.x + 1.0) / 2.0 * size.width, (1.0 - c.y) / 2.0 * size.height), c.z))
    }
}

impl<M> canvas::Program<M> for PlacingOverlay {
    type State = ();

    fn draw(&self, _state: &(), renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let size = bounds.size();
        // the faces far to near, each filled lightly and outlined
        let mut shown: Vec<(f32, Vec<Vec<Point>>)> = self
            .faces
            .iter()
            .filter_map(|rings| {
                let projected: Option<Vec<Vec<(Point, f32)>>> = rings.iter().map(|r| r.iter().map(|p| self.project(size, *p)).collect()).collect();
                let projected = projected?;
                let outer = projected.first()?;
                let depth = outer.iter().map(|(_, z)| z).sum::<f32>() / outer.len().max(1) as f32;
                Some((depth, projected.into_iter().map(|r| r.into_iter().map(|(p, _)| p).collect()).collect()))
            })
            .collect();
        shown.sort_by(|a, b| b.0.total_cmp(&a.0));
        let fill = Color { a: 0.16, ..self.ink };
        let edge = canvas::Stroke::default().with_color(Color { a: 0.9, ..self.ink }).with_width(1.25);
        for (_, rings) in &shown {
            let path = Path::new(|path| {
                for ring in rings.iter().filter(|r| r.len() >= 2) {
                    path.move_to(ring[0]);
                    for p in &ring[1..] {
                        path.line_to(*p);
                    }
                    path.close();
                }
            });
            frame.fill(&path, canvas::Fill { style: canvas::Style::Solid(fill), rule: canvas::fill::Rule::EvenOdd });
            frame.stroke(&path, edge);
        }
        // the points clicked, and the cursor's point as a ring
        for (p, _) in self.points.iter().filter_map(|p| self.project(size, *p)) {
            frame.fill(&Path::rectangle(Point::new(p.x - 3.5, p.y - 3.5), Size::new(7.0, 7.0)), self.ink);
        }
        if let Some((p, _)) = self.project(size, self.cursor) {
            frame.stroke(&Path::circle(p, 5.0), canvas::Stroke::default().with_color(self.ink).with_width(1.5));
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::modeller::ModelTool;
    use crate::app::Message;
    use crate::viewport::Navigate;
    use bim_core::element::PlanarForm;
    use bim_core::PlanarDef;
    use glam::{DVec2, DVec3};

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

    fn screen(app: &App, x: f64, y: f64) -> (f32, f32) {
        app.on_screen(DVec3::new(x, y, 0.0), 1000.0, 800.0).unwrap()
    }

    fn hover(app: &mut App, x: f64, y: f64) {
        let (sx, sy) = screen(app, x, y);
        let _ = app.update(Message::Navigate(Navigate::Hover { x: sx, y: sy, width: 1000.0, height: 800.0 }));
    }

    fn click(app: &mut App, x: f64, y: f64) {
        let (sx, sy) = screen(app, x, y);
        let _ = app.update(Message::Navigate(Navigate::Pick { x: sx, y: sy, width: 1000.0, height: 800.0, add: false }));
    }

    /// The smallest box round the preview's faces.
    fn bounds(o: &PlacingOverlay) -> (Vec3, Vec3) {
        let all = o.faces.iter().flatten().flatten();
        (all.clone().fold(Vec3::INFINITY, |a, p| a.min(*p)), all.fold(Vec3::NEG_INFINITY, |a, p| a.max(*p)))
    }

    #[test]
    fn a_wall_follows_the_cursor_at_its_full_size_and_is_placed_as_shown() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        hover(&mut app, 0.0, 0.0);
        assert!(app.placing_overlay(Color::BLACK).unwrap().faces.is_empty(), "no body before the first click");
        click(&mut app, 0.0, 0.0);
        // nearly straight: the preview straightens as the click will
        hover(&mut app, 4.0, 0.2);
        let preview = app.placing_overlay(Color::BLACK).unwrap();
        assert_eq!(preview.faces.len(), 6, "a box");
        let (lo, hi) = bounds(&preview);
        assert!((lo - Vec3::new(0.0, -0.1, 0.0)).length() < 1e-5 && (hi - Vec3::new(4.0, 0.1, 3.0)).length() < 1e-5, "{lo} {hi}");
        let shown = app.placing().unwrap();
        click(&mut app, 4.0, 0.2);
        let placed = app.modeller.model.get(bim_core::ElementId(1)).unwrap().shape_in_project(&app.modeller.model.grid);
        assert_eq!(placed, shown);
        let Shape::Planar(PlanarDef { form: PlanarForm::Wall { end, .. }, .. }) = placed else { panic!() };
        assert_eq!(end, DVec2::new(4.0, 0.0));
        // the next wall starts at that end: no length yet, so no body until the cursor moves on
        assert!(app.placing_overlay(Color::BLACK).unwrap().faces.is_empty());
        hover(&mut app, 4.0, 3.0);
        assert_eq!(app.placing_overlay(Color::BLACK).unwrap().faces.len(), 6);
    }

    #[test]
    fn a_column_shows_under_the_cursor_before_a_click_and_esc_takes_the_preview_away() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Column, "Column")));
        hover(&mut app, 2.0, 1.0);
        let (lo, hi) = bounds(&app.placing_overlay(Color::BLACK).unwrap());
        assert!((lo - Vec3::new(1.85, 0.85, 0.0)).length() < 1e-5 && (hi - Vec3::new(2.15, 1.15, 3.0)).length() < 1e-5, "{lo} {hi}");
        let _ = app.update(Message::Escape);
        assert!(app.modeller.tool.is_none());
        assert!(app.placing_overlay(Color::BLACK).is_none());
        // off the viewport: no preview
        app.set_model_tool(Some((ModelTool::Column, "Column")));
        hover(&mut app, 2.0, 1.0);
        let _ = app.update(Message::Navigate(Navigate::HoverOff));
        assert!(app.placing_overlay(Color::BLACK).is_none());
    }
}
