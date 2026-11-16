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
//! - **A series.** A picked grid line has a plus past its end: it opens a field for the
//!   spacings of the lines to add beside it, in millimetres ("5000 3000 3x5400": 5 m on, 3 m
//!   further, then three times 5.4 m). The new lines are parallel copies with the next free
//!   names: to the right of a line that runs up the plan, above one that runs across it. The line
//!   stays picked, so more can be added from it.
//!
//! They are kept in the kernel's model beside the elements, so undo takes a grid line back as it
//! takes back an element, and the project's IFCX file holds them.

use crate::app::placing::PlacingOverlay;
use crate::app::App;
use crate::model::GridLine;
use glam::{DVec2, DVec3};
use iced::Color;
use openaec_ui::Language;

/// The plus of a picked grid line: its size, and how far past the line's end it stands (pixels).
const PLUS: f32 = 24.0;
const PLUS_PAST: f32 = 56.0;
/// How wide the series field's card is.
const CARD: f32 = 240.0;

/// A message of the series field of the picked grid line.
#[derive(Debug, Clone, PartialEq)]
pub enum SeriesMessage {
    /// The plus: opens the field, or closes it.
    Toggle,
    Typed(String),
    /// Enter or Create: the lines are added.
    Create,
}

/// The spacings a series is typed as, in metres: millimetres apart, `NxD` for N times D.
/// `None` when the text is no such list, or empty.
pub fn spacings(typed: &str) -> Option<Vec<f64>> {
    let mut out = Vec::new();
    for word in typed.split_whitespace() {
        let (times, each) = match word.to_ascii_lowercase().split_once('x') {
            Some((n, d)) => (n.parse::<usize>().ok().filter(|n| (1..=200).contains(n))?, d.to_string()),
            None => (1, word.to_string()),
        };
        let each = each.replace(',', ".").parse::<f64>().ok().filter(|d| d.is_finite() && *d > 0.0)? / 1000.0;
        out.extend(std::iter::repeat_n(each, times));
    }
    (!out.is_empty() && out.len() <= 500).then_some(out)
}

impl App {
    /// A message of the series field.
    pub(in crate::app) fn series_message(&mut self, message: SeriesMessage) {
        match message {
            SeriesMessage::Toggle => self.modeller.grid_series = if self.modeller.grid_series.is_some() { None } else { Some(String::new()) },
            SeriesMessage::Typed(text) => self.modeller.grid_series = Some(text),
            SeriesMessage::Create => {
                let Some(apart) = self.modeller.grid_series.as_deref().and_then(spacings) else { return };
                if self.add_grid_series(&apart) {
                    self.modeller.grid_series = None;
                }
            }
        }
    }

    /// Adds grid lines beside the picked one, each `apart` further than the one before: to the
    /// right of a line that runs up the plan, above one that runs across it. The picked line
    /// stays picked. False when no modelled grid line is picked.
    pub(in crate::app) fn add_grid_series(&mut self, apart: &[f64]) -> bool {
        let Some(index) = self.picked_modelled_grid() else { return false };
        let Some(line) = self.modeller.model.grids().get(index).cloned() else { return false };
        let along = (line.end - line.start).normalize_or_zero();
        // square to the line, towards the right, or upwards for a line that runs across
        let mut side = along.perp();
        if side.x < -1e-9 || (side.x.abs() <= 1e-9 && side.y < 0.0) {
            side = -side;
        }
        let mut offset = 0.0;
        for d in apart {
            offset += d;
            let _ = self.modeller.model.add_grid(line.start + side * offset, line.end + side * offset);
        }
        self.show_modelled();
        let shown = self.shown_grid(index);
        self.pick_grid(shown);
        true
    }

    /// The plus past the end of the picked grid line, and the series field it opens.
    pub(in crate::app) fn grid_series_overlay(&self, p: crate::theme::Palette, l: Language) -> Option<iced::Element<'_, crate::app::Message>> {
        use crate::app::Message;
        use iced::widget::{button, column, container, row, stack, text, text_input};
        use iced::{Center, Fill, Padding};
        let line = self.modelled_grid_lines().into_iter().nth(self.picked_modelled_grid()?)?;
        let (width, height) = (self.viewport_size.0.max(1.0), self.viewport_size.1.max(1.0));
        let (a, b) = (glam::Vec3::from_array(line.a).as_dvec3(), glam::Vec3::from_array(line.b).as_dvec3());
        let (a, b) = (self.on_screen(a, width, height)?, self.on_screen(b, width, height)?);
        // past the end and its bubble, along the line on screen
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let length = dx.hypot(dy).max(1.0);
        let (x, y) = (b.0 + dx / length * PLUS_PAST, b.1 + dy / length * PLUS_PAST);
        let at = |w: f32, h: f32| Padding { left: (x - PLUS / 2.0).clamp(0.0, (width - w).max(0.0)), top: (y - PLUS / 2.0).clamp(0.0, (height - h).max(0.0)), ..Padding::ZERO };
        let plus = button(text("+").size(16).color(iced::Color::WHITE).align_x(Center).align_y(Center).width(Fill).height(Fill))
            .width(PLUS)
            .height(PLUS)
            .padding(0)
            .style(move |_, _| iced::widget::button::Style {
                background: Some(p.accent.into()),
                border: iced::Border { radius: (PLUS / 2.0).into(), width: 1.0, color: iced::Color::WHITE },
                ..Default::default()
            })
            .on_press(Message::GridSeries(SeriesMessage::Toggle));
        let mut layers = stack![container(plus).padding(at(PLUS, PLUS))].width(Fill).height(Fill);
        if let Some(typed) = &self.modeller.grid_series {
            let valid = spacings(typed).is_some();
            let field = text_input("5000 3000 3x5400", typed)
                .on_input(|t| Message::GridSeries(SeriesMessage::Typed(t)))
                .on_submit(Message::GridSeries(SeriesMessage::Create))
                .size(12)
                .padding([3, 6]);
            let create = button(text(l.text("Maken", "Create")).size(12)).padding([3, 10]).style(move |_, s| p.ribbon_button(valid, s)).on_press_maybe(valid.then_some(Message::GridSeries(SeriesMessage::Create)));
            let cancel = button(text(l.text("Annuleren", "Cancel")).size(12)).padding([3, 10]).style(move |_, s| p.ribbon_button(false, s)).on_press(Message::GridSeries(SeriesMessage::Toggle));
            let card = container(
                column![
                    text(l.text("Reeks stramienen: afstanden in mm", "Series of grid lines: spacings in mm")).size(12).color(p.text_secondary),
                    field,
                    row![create, cancel].spacing(6),
                ]
                .spacing(6),
            )
            .padding(8)
            .width(CARD)
            .style(move |_| p.panel());
            let mut place = at(CARD, 96.0);
            place.left = (x + PLUS).clamp(0.0, (width - CARD).max(0.0));
            layers = layers.push(container(card).padding(place));
        }
        Some(layers.into())
    }

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
        let at = self.plane_point(x, y, width, height, z).map(|p| self.snapped_as(p, x, y, width, height));
        self.modeller.cursor = at.map(|(p, _)| p);
        self.modeller.snap_mark = at.and_then(|(_, kind)| kind);
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
        Some(PlacingOverlay { faces, points, cursor: cursor.as_vec3(), view_proj: self.camera.view_proj(self.aspect()) * self.model_matrix(), ink, mark: self.snap_mark(), readout: self.tool_readout() })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_series_typed_beside_a_picked_grid_line_adds_parallel_lines_with_the_next_names() {
        use super::super::tests::modelling;
        use super::{spacings, SeriesMessage};
        use crate::app::Message;
        use glam::DVec2;
        assert_eq!(spacings("5000 3000 3x5400"), Some(vec![5.0, 3.0, 5.4, 5.4, 5.4]));
        assert_eq!(spacings(" 2X1200,5 "), Some(vec![1.2005, 1.2005]));
        for wrong in ["", "abc", "0", "-5", "3x", "x5", "0x5"] {
            assert_eq!(spacings(wrong), None, "{wrong}");
        }
        let mut app = modelling();
        // a line up the plan is numbered: its series goes to the right
        app.add_grid_line(DVec2::new(0.0, 0.0), DVec2::new(0.0, 8.0)).unwrap();
        assert!(app.grid_series_overlay(crate::theme::ThemeKind::Light.palette(), openaec_ui::Language::English).is_some());
        let _ = app.update(Message::GridSeries(SeriesMessage::Toggle));
        let _ = app.update(Message::GridSeries(SeriesMessage::Typed("5000 2x3000".into())));
        let _ = app.update(Message::GridSeries(SeriesMessage::Create));
        let lines: Vec<(String, f64)> = app.modeller.model.grids().iter().map(|g| (g.name.clone(), g.start.x)).collect();
        assert_eq!(lines, [("1".to_string(), 0.0), ("2".to_string(), 5.0), ("3".to_string(), 8.0), ("4".to_string(), 11.0)]);
        assert!(app.modeller.grid_series.is_none() && app.picked_modelled_grid() == Some(0), "the field closes, the line stays picked");
        // a line across the plan is lettered: its series goes up
        app.add_grid_line(DVec2::new(12.0, 0.0), DVec2::new(-1.0, 0.0)).unwrap();
        assert!(app.add_grid_series(&[4.0]));
        let across: Vec<(String, f64)> = app.modeller.model.grids().iter().skip(4).map(|g| (g.name.clone(), g.start.y)).collect();
        assert_eq!(across, [("A".to_string(), 0.0), ("B".to_string(), 4.0)]);
        // a text that is no series adds nothing
        let _ = app.update(Message::GridSeries(SeriesMessage::Typed("abc".into())));
        let _ = app.update(Message::GridSeries(SeriesMessage::Create));
        assert_eq!(app.modeller.model.grids().len(), 6);
    }

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
