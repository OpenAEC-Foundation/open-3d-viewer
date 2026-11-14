//! Grid lines in the Modeller: the Grid tool, and what follows the grid lines of the modelled
//! project.
//!
//! - **The tool.** Two clicks on the work plane make a grid line; the second is put in line with
//!   the first when nearly so, as a wall's is. The line names itself (a number when it runs
//!   north-south, else a letter: see `bim_core::datum`), is picked, and the tool goes off.
//! - **Where they show.** The grid lines are part of what the modelled project shows, so the
//!   viewport draws them as it draws the grid lines of an opened file: in 3D at the lowest level,
//!   in a floor plan over its cut, in a section where it crosses them.
//! - **Snapping.** A click of any modelling tool snaps to the ends of the grid lines and to where
//!   they cross.
//! - **Removing.** Delete removes the picked grid line.
//!
//! They are kept in the kernel's model beside the elements, so undo takes a grid line back as it
//! takes back an element, and the project's IFCX file holds them.

use crate::app::placing::PlacingOverlay;
use crate::app::App;
use crate::model::GridLine;
use glam::{DVec2, DVec3};
use iced::Color;
use openaec_ui::Language;

impl App {
    /// Switches the Grid tool on or off; a tool that places elements goes off.
    pub(in crate::app) fn toggle_grid_tool(&mut self) {
        let on = !self.modeller.grid_tool;
        self.set_model_tool(None);
        self.modeller.grid_tool = on;
        if on {
            self.measure.tool = None;
            self.plane_picking = false;
        }
    }

    /// Esc while the Grid tool is on: drop the point clicked, else switch the tool off.
    pub(in crate::app) fn escape_grid_tool(&mut self) {
        self.modeller.refusal = None;
        if self.modeller.points.is_empty() {
            self.modeller.grid_tool = false;
            self.modeller.cursor = None;
        } else {
            self.modeller.points.clear();
        }
    }

    /// The cursor rests at (x, y) while the Grid tool is on: the preview follows it. False when
    /// the tool is off.
    pub(in crate::app) fn grid_hover(&mut self, x: f32, y: f32, width: f32, height: f32) -> bool {
        if !self.modeller.grid_tool {
            return false;
        }
        let (width, height) = (width.max(1.0), height.max(1.0));
        let (z, _, _) = self.work_plane();
        self.modeller.cursor = self.plane_point(x, y, width, height, z).map(|p| self.snapped(p, x, y, width, height));
        true
    }

    /// A click in the viewport while the Grid tool is on: the line's start, then its end.
    pub(in crate::app) fn grid_click(&mut self, x: f32, y: f32, width: f32, height: f32) {
        let (z, _, _) = self.work_plane();
        let Some(p) = self.plane_point(x, y, width, height, z) else { return };
        let p = self.snapped(p, x, y, width, height);
        self.modeller.refusal = None;
        let Some(first) = self.modeller.points.first().copied() else {
            self.modeller.points.push(p);
            return;
        };
        self.modeller.points.clear();
        match self.add_grid_line(first.truncate(), p.truncate()) {
            Ok(_) => {
                self.modeller.grid_tool = false;
                self.modeller.cursor = None;
            }
            Err(refusal) => self.modeller.refusal = Some(refusal),
        }
    }

    /// Adds a grid line from `start` to `end` (scene coordinates) to the modelled project, shows
    /// it and picks it; returns its index among the project's grid lines.
    pub(in crate::app) fn add_grid_line(&mut self, start: DVec2, end: DVec2) -> Result<usize, bim_core::Refusal> {
        let index = self.modeller.model.add_grid(start, end)?;
        self.show_modelled();
        let shown = self.shown_grid(index);
        self.pick_grid(shown);
        Ok(index)
    }

    /// The grid lines of the modelled project as the viewport draws them: at the lowest level.
    pub(in crate::app) fn modelled_grid_lines(&self) -> Vec<GridLine> {
        let z = self.document.levels().iter().map(|l| l.elevation).fold(f64::INFINITY, f64::min);
        let z = if z.is_finite() { z as f32 } else { 0.0 };
        self.modeller.model.grids().iter().map(|g| GridLine { name: g.name.clone(), a: [g.start.x as f32, g.start.y as f32, z], b: [g.end.x as f32, g.end.y as f32, z] }).collect()
    }

    /// Which of the grid lines shown modelled grid line `index` is: the scene may hold those of
    /// opened files too.
    fn shown_grid(&self, index: usize) -> Option<usize> {
        let line = self.modelled_grid_lines().into_iter().nth(index)?;
        self.model.as_ref()?.project.grids.iter().position(|g| *g == line)
    }

    /// Which modelled grid line the grid line picked in the viewport is; `None` when none is
    /// picked or it is one of an opened file.
    pub(in crate::app) fn picked_modelled_grid(&self) -> Option<usize> {
        let picked = self.model.as_ref()?.project.grids.get(self.picked_grid?)?;
        self.modelled_grid_lines().iter().position(|g| g == picked)
    }

    /// Removes the picked grid line when it is one of the modelled project. False when none was.
    pub(in crate::app) fn delete_picked_grid(&mut self) -> bool {
        let Some(index) = self.picked_modelled_grid() else { return false };
        self.modeller.model.remove_grid(index);
        self.pick_grid(None);
        self.show_modelled();
        true
    }

    /// The points of the grid lines a click snaps to, on the work plane: their ends and where
    /// they cross.
    pub(in crate::app) fn grid_snap_points(&self) -> Vec<DVec3> {
        let grids = self.modeller.model.grids();
        if grids.is_empty() {
            return Vec::new();
        }
        let (z, _, _) = self.work_plane();
        let mut points: Vec<DVec3> = grids.iter().flat_map(|g| [g.start.extend(z), g.end.extend(z)]).collect();
        for (i, g) in grids.iter().enumerate() {
            points.extend(grids[i + 1..].iter().filter_map(|other| g.crossing(other)).map(|p| p.extend(z)));
        }
        points
    }

    /// What to click next with the Grid tool, for the status line; `None` when the tool is off.
    pub(in crate::app) fn grid_hint(&self, l: Language) -> Option<String> {
        if !self.modeller.grid_tool {
            return None;
        }
        let hint = if self.modeller.points.is_empty() {
            l.text("Klik het beginpunt · Esc: stoppen", "Click the start point · Esc: stop")
        } else {
            l.text("Klik het eindpunt · Esc: opnieuw", "Click the end point · Esc: start again")
        };
        let tool = l.text("Stramien", "Grid");
        Some(match self.modeller.refusal {
            Some(refusal) => format!("{tool}: {} · {hint}", l.translate(refusal.reason())),
            None => format!("{tool}: {hint}"),
        })
    }

    /// The preview of the grid line being drawn: from the point clicked to the cursor's point.
    /// `None` when the tool is off or the cursor is away.
    pub(in crate::app) fn grid_overlay(&self, ink: Color) -> Option<PlacingOverlay> {
        if !self.modeller.grid_tool {
            return None;
        }
        let cursor = self.modeller.cursor?;
        let points: Vec<glam::Vec3> = self.modeller.points.iter().map(|p| p.as_vec3()).collect();
        // the line as a face of two corners: its outline is the line
        let faces = points.first().map(|first| vec![vec![vec![*first, cursor.as_vec3()]]]).unwrap_or_default();
        Some(PlacingOverlay { faces, points, cursor: cursor.as_vec3(), view_proj: self.camera.view_proj(self.aspect()) * self.model_matrix(), ink })
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{click, modelling};
    use super::super::ModelTool;
    use crate::app::Message;
    use bim_core::Shape;
    use glam::DVec2;

    fn names(app: &crate::app::App) -> Vec<String> {
        app.model.as_ref().unwrap().project.grids.iter().map(|g| g.name.clone()).collect()
    }

    #[test]
    fn two_clicks_draw_a_grid_line_that_names_itself_and_is_picked() {
        let mut app = modelling();
        let _ = app.update(Message::GridTool);
        assert!(app.modeller.grid_tool && app.grid_hint(openaec_ui::Language::English).is_some());
        click(&mut app, 2.0, -1.0);
        // nearly straight up: put in line with the first point
        click(&mut app, 2.05, 8.0);
        assert!(!app.modeller.grid_tool, "the tool goes off");
        let grid = &app.modeller.model.grids()[0];
        assert_eq!((grid.name.as_str(), grid.start, grid.end), ("1", DVec2::new(2.0, -1.0), DVec2::new(2.0, 8.0)));
        assert_eq!(names(&app), ["1"]);
        assert_eq!(app.picked_grid, Some(0));
        // one across it: lettered
        let _ = app.update(Message::GridTool);
        click(&mut app, -1.0, 3.0);
        click(&mut app, 9.0, 3.0);
        assert_eq!(names(&app), ["1", "A"]);
        // a click of a modelling tool snaps to where they cross
        app.set_model_tool(Some((ModelTool::Column, "Column")));
        click(&mut app, 2.2, 3.2);
        let column = app.modeller.model.iter().next().unwrap().shape_in_project(&app.modeller.model.grid);
        let Shape::Linear(l) = column else { panic!() };
        assert_eq!(l.start.truncate(), DVec2::new(2.0, 3.0));
        app.set_model_tool(None);
    }

    #[test]
    fn the_grid_lines_of_a_project_come_back_when_it_is_opened_again() {
        use std::sync::Arc;
        let mut app = modelling();
        app.add_grid_line(DVec2::new(2.0, -1.0), DVec2::new(2.0, 8.0)).unwrap();
        app.add_grid_line(DVec2::new(-1.0, 3.25), DVec2::new(9.0, 3.25)).unwrap();
        let written = app.modeller.model.grids().to_vec();
        let scene = app.model.clone().unwrap();
        let bytes = crate::export::to_bytes(&scene, &app.document, "house.ifcx").expect("IFCX");
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.contains("openaec::grid") && text.contains("IfcGridAxis"));
        let source = crate::loaders::Source::Bytes { name: "house.ifcx".into(), data: Arc::new(bytes) };
        let loaded = crate::loaders::load_with(&source, &crate::loaders::Progress::default()).expect("read back");
        let mut back = crate::app::App::default();
        back.set_model(Arc::new(loaded));
        assert_eq!(back.modeller.model.grids(), written, "every grid line comes back");
        // shown once, and the project's own: the next one takes the next name
        assert_eq!(names(&back), ["1", "A"]);
        back.add_grid_line(DVec2::new(6.0, -1.0), DVec2::new(6.0, 8.0)).unwrap();
        assert_eq!(names(&back), ["1", "A", "2"]);
    }

    #[test]
    fn a_grid_line_is_taken_back_removed_and_refused_without_length() {
        let mut app = modelling();
        let _ = app.update(Message::GridTool);
        click(&mut app, 0.0, 0.0);
        click(&mut app, 0.0, 6.0);
        let _ = app.update(Message::GridTool);
        click(&mut app, 4.0, 0.0);
        click(&mut app, 4.0, 6.0);
        assert_eq!(names(&app), ["1", "2"]);
        // undo takes the last one back, redo gives it again
        let _ = app.update(Message::Undo);
        assert_eq!(names(&app), ["1"]);
        let _ = app.update(Message::Redo);
        assert_eq!(names(&app), ["1", "2"]);
        // Delete removes the picked one
        app.pick_grid(Some(0));
        let _ = app.update(Message::DeletePicked);
        assert_eq!(names(&app), ["2"]);
        assert_eq!(app.picked_grid, None);
        // a line without length is refused and the tool stays on; Esc drops the point, then the tool
        let _ = app.update(Message::GridTool);
        click(&mut app, 8.0, 8.0);
        click(&mut app, 8.0, 8.0);
        assert_eq!(app.modeller.refusal, Some(bim_core::Refusal::ZeroLength));
        assert!(app.modeller.grid_tool);
        click(&mut app, 1.0, 1.0);
        let _ = app.update(Message::Escape);
        assert!(app.modeller.grid_tool && app.modeller.points.is_empty());
        let _ = app.update(Message::Escape);
        assert!(!app.modeller.grid_tool);
    }
}
