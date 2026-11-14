//! The Select tool of the Modeller: selecting is a tool of its own, as in the reference modeller.
//! It is on when no other tool is.

use crate::app::App;
use openaec_ui::Language;

impl App {
    /// Switches every tool off: what is clicked is then selected. The selection stays.
    pub(in crate::app) fn select_tool(&mut self) {
        self.set_model_tool(None);
        self.modeller.grid_tool = false;
        self.modeller.points.clear();
        self.modeller.cursor = None;
        self.modeller.transform = None;
        self.measure.tool = None;
        self.dimension_tool = false;
        self.note_tool = false;
        self.section_tool = false;
    }

    /// Whether the Select tool is on: no other tool is.
    pub(crate) fn selecting(&self) -> bool {
        self.modeller.tool.is_none()
            && !self.modeller.grid_tool
            && self.modeller.transform.is_none()
            && self.modeller.edit.is_none()
            && self.measure.tool.is_none()
            && !self.dimension_tool
            && !self.note_tool
            && !self.section_tool
            && !self.plane_picking
    }

    /// The Modeller's Esc when the tools of its own (placing, the grid, a move) have let theirs go:
    /// a tool still on is switched off, back to Select. False when the Select tool was on already.
    pub(in crate::app) fn modeller_escape(&mut self) -> bool {
        if self.selecting() {
            return false;
        }
        self.select_tool();
        true
    }

    /// The status line of the Select tool in the Modeller; none elsewhere or with a tool on.
    pub(in crate::app) fn select_hint(&self, l: Language) -> Option<String> {
        (self.modeller.in_modeller && self.modeller.shown.is_some() && self.selecting())
            .then(|| l.text("Selecteren (S): klik of sleep een kader · Shift: erbij · Esc: niets", "Select (S): click or drag a box · Shift: add · Esc: nothing").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{click, modelling};
    use super::super::ModelTool;
    use crate::app::transform::TransformKind;
    use crate::app::{App, Message};
    use bim_core::element::PlanarForm;
    use bim_core::{PlanarDef, Shape};
    use glam::DVec2;

    fn wall_shape(a: (f64, f64), b: (f64, f64)) -> Shape {
        let form = PlanarForm::Wall { start: DVec2::new(a.0, a.1), end: DVec2::new(b.0, b.1), base: 0.0, height: 3.0 };
        Shape::Planar(PlanarDef { form, thickness: 0.2, openings: Vec::new(), build_up: None })
    }

    fn walls(app: &App) -> usize {
        app.modeller.model.iter().filter(|e| e.class() == "IfcWall").count()
    }

    #[test]
    fn select_is_on_when_no_tool_is_and_the_command_switches_tools_off() {
        let mut app = modelling();
        assert!(app.selecting());
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        assert!(!app.selecting());
        let _ = app.update(Message::SelectTool);
        assert!(app.selecting() && app.modeller.tool.is_none());
        let _ = app.update(Message::GridTool);
        assert!(!app.selecting());
        let _ = app.update(Message::SelectTool);
        assert!(app.selecting());
    }

    #[test]
    fn esc_drops_the_step_then_the_tool_then_the_selection() {
        let mut app = modelling();
        app.modeller.in_modeller = true;
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        click(&mut app, 0.0, 0.0);
        let _ = app.update(Message::Escape);
        assert!(app.modeller.points.is_empty() && app.modeller.tool.is_some(), "the point goes first");
        let _ = app.update(Message::Escape);
        assert!(app.selecting(), "then the tool");
        let wall = app.add_element("Wall", None, wall_shape((0.0, 0.0), (4.0, 0.0))).unwrap();
        app.select_modelled(wall);
        // a tool on with a selection: Esc goes to Select and keeps the selection
        let _ = app.update(Message::GridTool);
        let _ = app.update(Message::Escape);
        assert!(app.selecting() && app.selected_modelled() == vec![wall]);
        let _ = app.update(Message::Escape);
        assert!(app.selected_modelled().is_empty(), "then the selection");
    }

    #[test]
    fn tools_end_as_the_reference_modeller_ends_them() {
        let mut app = modelling();
        app.modeller.in_modeller = true;
        // Move: back to Select after its last click
        let wall = app.add_element("Wall", None, wall_shape((0.0, 0.0), (4.0, 0.0))).unwrap();
        app.select_modelled(wall);
        app.begin_transform(TransformKind::Move);
        click(&mut app, 0.0, 0.0);
        click(&mut app, 0.0, 2.0);
        assert!(app.selecting(), "Move ends in Select");
        // Wall: chains until Esc
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        click(&mut app, 0.0, 5.0);
        click(&mut app, 4.0, 5.0);
        click(&mut app, 4.0, 9.0);
        assert_eq!(walls(&app), 3);
        assert_eq!(app.modeller.tool.map(|t| t.0), Some(ModelTool::Wall), "the Wall tool stays on");
        // Door: stays on for the next
        app.set_model_tool(Some((ModelTool::Door, "Door")));
        click(&mut app, 2.0, 5.05);
        assert_eq!(app.modeller.tool.map(|t| t.0), Some(ModelTool::Door));
    }

    #[test]
    fn the_status_line_names_select_and_its_key() {
        let mut app = modelling();
        app.modeller.in_modeller = true;
        let line = app.select_hint(openaec_ui::Language::English).unwrap_or_default();
        app.modeller.in_modeller = false;
        assert!(app.select_hint(openaec_ui::Language::English).is_none(), "not in the Viewer");
        assert!(line.contains("Select (S)"), "{line}");
    }
}
