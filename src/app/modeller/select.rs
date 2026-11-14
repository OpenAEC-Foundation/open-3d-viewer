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

    /// Whether a left drag in the viewport draws a selection box: the Select tool of the Modeller.
    pub(crate) fn drag_selects(&self) -> bool {
        self.modeller.in_modeller && self.modeller.shown.is_some() && self.selecting()
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
            .then(|| {
                let level = self.work_plane().2.and_then(|p| self.document.level(&p).map(|l| l.name.clone()));
                let select = l.text("Selecteren (S): klik of sleep een kader · Shift: erbij · Esc: niets", "Select (S): click or drag a box · Shift: add · Esc: nothing");
                match level {
                    Some(name) => format!("{select} · {name} (PgUp/PgDn)"),
                    None => select.to_string(),
                }
            })
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

#[cfg(test)]
mod box_tests {
    use super::super::tests::modelling;
    use crate::app::{App, Message};
    use crate::viewport::Navigate;
    use bim_core::element::PlanarForm;
    use bim_core::{PlanarDef, Shape};
    use glam::{DVec2, DVec3};

    /// The modelling app with two walls 4 m long, on y = 0 and on y = 6, in the Modeller.
    fn modelling_with_two_walls() -> App {
        let mut app = modelling();
        app.modeller.in_modeller = true;
        for y in [0.0, 6.0] {
            let form = PlanarForm::Wall { start: DVec2::new(0.0, y), end: DVec2::new(4.0, y), base: 0.0, height: 3.0 };
            app.add_element("Wall", None, Shape::Planar(PlanarDef { form, thickness: 0.2, openings: Vec::new(), build_up: None })).unwrap();
        }
        app
    }

    fn screen(app: &App, x: f64, y: f64) -> (f32, f32) {
        app.on_screen(DVec3::new(x, y, 0.0), 1000.0, 800.0).unwrap()
    }

    /// A box dragged from `a` to `b` on screen, adding to the selection or not.
    fn boxed(app: &mut App, a: (f32, f32), b: (f32, f32), add: bool) {
        let _ = app.update(Message::Navigate(Navigate::Box { x0: a.0, y0: a.1, x1: b.0, y1: b.1, width: 1000.0, height: 800.0, done: true, add }));
    }

    #[test]
    fn a_box_replaces_the_selection_and_shift_adds() {
        let mut app = modelling_with_two_walls();
        let (a, b) = (screen(&app, -1.0, -1.0), screen(&app, 5.0, 1.0));
        boxed(&mut app, a, b, false);
        assert_eq!(app.selected_modelled().len(), 1);
        let (c, d) = (screen(&app, -1.0, 5.0), screen(&app, 5.0, 7.0));
        boxed(&mut app, c, d, false);
        assert_eq!(app.selected_modelled().len(), 1, "replaced");
        boxed(&mut app, a, b, true);
        assert_eq!(app.selected_modelled().len(), 2, "Shift adds");
    }

    #[test]
    fn the_select_tool_drags_boxes_and_other_tools_orbit() {
        let mut app = modelling_with_two_walls();
        assert!(app.drag_selects());
        app.set_model_tool(Some((super::super::ModelTool::Wall, "Wall")));
        assert!(!app.drag_selects(), "a tool on: a drag orbits");
        app.select_tool();
        app.modeller.in_modeller = false;
        assert!(!app.drag_selects(), "the Viewer orbits");
    }
}
