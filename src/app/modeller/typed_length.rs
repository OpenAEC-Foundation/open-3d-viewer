//! A length typed while a tool draws: after the first click of a wall, a beam, a curtain wall or
//! a grid line, digits type how long it becomes, in millimetres, and Enter places its end that
//! far from the first point, the way the cursor points. Backspace takes a digit back, Esc drops
//! what was typed. Beside the cursor the length and the angle of what is being drawn show, or
//! what is typed.

use crate::app::App;
use glam::DVec3;

/// The characters a length is typed with.
const DIGITS: &str = "0123456789.,";

impl App {
    /// A tool has its first point and waits for the next: a length can be typed.
    pub(crate) fn tool_drawing(&self) -> bool {
        let tool = self.modeller.grid_tool || self.modeller.tool.is_some_and(|(t, _)| !t.opens() && t.clicks() > 1);
        tool && !self.modeller.points.is_empty()
    }

    /// A character typed while a tool draws. False when it is none of a length's, or no tool
    /// draws.
    pub(crate) fn tool_digit(&mut self, c: char) -> bool {
        if !self.tool_drawing() || !DIGITS.contains(c) {
            return false;
        }
        self.modeller.length_typed.push(c);
        true
    }

    /// Backspace: the last character typed goes. False when nothing is typed.
    pub(in crate::app) fn tool_backspace(&mut self) -> bool {
        self.modeller.length_typed.pop().is_some()
    }

    /// The length typed, in metres.
    fn typed_length(&self) -> Option<f64> {
        self.modeller.length_typed.replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0).map(|mm| mm / 1000.0)
    }

    /// Enter: the end goes the typed length from the first point, the way the cursor points
    /// (along x when the cursor is on the first point). False when nothing is typed.
    pub(in crate::app) fn tool_enter(&mut self) -> bool {
        if self.modeller.length_typed.is_empty() || !self.tool_drawing() {
            return false;
        }
        let Some(length) = self.typed_length() else { return true };
        let first = *self.modeller.points.last().unwrap();
        let way = self.modeller.cursor.map(|c| (c - first).truncate()).filter(|d| d.length() > 1e-9).map_or(glam::DVec2::X, |d| d.normalize());
        let end = first + (way * length).extend(0.0);
        self.modeller.length_typed.clear();
        if self.modeller.grid_tool {
            self.modeller.points.clear();
            match self.add_grid_line(first.truncate(), end.truncate()) {
                Ok(_) => {
                    self.modeller.grid_tool = false;
                    self.modeller.cursor = None;
                }
                Err(refusal) => self.modeller.refusal = Some(refusal),
            }
        } else if let Some((tool, name)) = self.modeller.tool {
            let (_, _, level) = self.work_plane();
            self.model_point(tool, name, level, end);
        }
        true
    }

    /// What shows beside the cursor while a tool draws: what is typed, else how long and at what
    /// angle the element under way is.
    pub(in crate::app) fn tool_readout(&self) -> Option<String> {
        if !self.tool_drawing() {
            return None;
        }
        if !self.modeller.length_typed.is_empty() {
            return Some(format!("{} mm", self.modeller.length_typed));
        }
        let first: DVec3 = *self.modeller.points.last()?;
        let d = (self.modeller.cursor? - first).truncate();
        Some(format!("{:.0} mm  {:.1}°", d.length() * 1000.0, d.y.atan2(d.x).to_degrees().rem_euclid(360.0)))
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{click, modelling};
    use super::super::ModelTool;
    use crate::app::Message;
    use bim_core::element::PlanarForm;
    use bim_core::{PlanarDef, Shape};
    use glam::{DVec2, DVec3};

    #[test]
    fn a_length_typed_after_the_first_click_places_the_wall_that_long_the_way_the_cursor_points() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        assert!(!app.tool_digit('3'), "nothing to type a length for before the first click");
        click(&mut app, 1.0, 1.0);
        // the cursor points north; 3600 typed, a digit taken back and typed again
        app.modeller.cursor = Some(DVec3::new(1.0, 2.5, 0.0));
        assert_eq!(app.tool_readout().as_deref(), Some("1500 mm  90.0°"));
        for c in "3609".chars() {
            assert!(app.tool_digit(c));
        }
        let _ = app.update(Message::DeletePicked);
        assert!(app.tool_digit('0'));
        assert_eq!(app.tool_readout().as_deref(), Some("3600 mm"));
        let _ = app.update(Message::MeasureFinish);
        let ends = |app: &crate::app::App, k: usize| match app.modeller.model.iter().nth(k).map(|e| e.shape_in_project(&app.modeller.model.grid)) {
            Some(Shape::Planar(PlanarDef { form: PlanarForm::Wall { start, end, .. }, .. })) => (start, end),
            other => panic!("{other:?}"),
        };
        let (a, b) = ends(&app, 0);
        assert!((a - DVec2::new(1.0, 1.0)).length() < 1e-9 && (b - DVec2::new(1.0, 4.6)).length() < 1e-9, "{a} {b}");
        // the chain goes on from its end: east, 2 m
        app.modeller.cursor = Some(DVec3::new(3.0, 4.6, 0.0));
        for c in "2000".chars() {
            assert!(app.tool_digit(c));
        }
        let _ = app.update(Message::MeasureFinish);
        let (a, b) = ends(&app, 1);
        assert!((a - DVec2::new(1.0, 4.6)).length() < 1e-9 && (b - DVec2::new(3.0, 4.6)).length() < 1e-9, "{a} {b}");
        // Esc drops what is typed and keeps the tool drawing
        assert!(app.tool_digit('5'));
        let _ = app.update(Message::Escape);
        assert!(app.modeller.length_typed.is_empty() && app.tool_drawing());
        // a grid line takes a typed length too, and its tool goes off as after a click
        let _ = app.update(Message::Escape);
        let _ = app.update(Message::Escape);
        app.toggle_grid_tool();
        app.modeller.points.push(DVec3::new(0.0, -2.0, 0.0));
        app.modeller.cursor = Some(DVec3::new(0.0, 5.0, 0.0));
        for c in "9000".chars() {
            assert!(app.tool_digit(c));
        }
        let _ = app.update(Message::MeasureFinish);
        let line = app.modeller.model.grids()[0].clone();
        assert!((line.end - DVec2::new(0.0, 7.0)).length() < 1e-9 && !app.modeller.grid_tool, "{line:?}");
    }
}
