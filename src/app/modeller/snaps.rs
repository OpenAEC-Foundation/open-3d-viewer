//! What a click of a tool lands on, each kind switched on or off at the right of the status bar:
//! the ends and corners of elements, the middles of their lines, the grid lines' ends and
//! crossings, the nearest point on a line, straight on from the last point, and whole
//! centimetres.

use crate::app::App;
use bim_core::element::PlanarForm;
use bim_core::{PlanarDef, Shape};
use glam::DVec2;
use openaec_ui::Language;

/// A kind of snap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Snap {
    /// Ends of walls, beams and columns, corners of slabs, where an object stands.
    End,
    /// The middle of a wall's or member's line and of a slab's edge.
    Middle,
    /// The ends of the grid lines and where they cross.
    Grid,
    /// The nearest point on a wall's or member's line, a slab's edge or a grid line.
    Line,
    /// Straight on from the last point clicked, along the axes.
    Ortho,
    /// Whole centimetres.
    Round,
}

impl Snap {
    pub const ALL: [Snap; 6] = [Snap::End, Snap::Middle, Snap::Grid, Snap::Line, Snap::Ortho, Snap::Round];

    /// Its name on its switch.
    pub fn label(self, l: Language) -> &'static str {
        match self {
            Snap::End => l.text("Eindpunt", "Endpoint"),
            Snap::Middle => l.text("Midden", "Midpoint"),
            Snap::Grid => l.text("Stramien", "Grid"),
            Snap::Line => l.text("Op lijn", "On line"),
            Snap::Ortho => l.text("Ortho", "Ortho"),
            Snap::Round => l.text("Hele cm", "Whole cm"),
        }
    }

    /// What it does, for its tooltip.
    pub fn tip(self, l: Language) -> &'static str {
        match self {
            Snap::End => l.text("Snap op eindpunten en hoeken van elementen", "Snap to the ends and corners of elements"),
            Snap::Middle => l.text("Snap op het midden van wanden, balken en vloerranden", "Snap to the middle of walls, beams and slab edges"),
            Snap::Grid => l.text("Snap op stramienlijnen: hun einden en kruisingen", "Snap to grid lines: their ends and crossings"),
            Snap::Line => l.text("Snap op het dichtstbijzijnde punt van een lijn", "Snap to the nearest point on a line"),
            Snap::Ortho => l.text("Trek recht door vanaf het vorige punt", "Go straight on from the last point"),
            Snap::Round => l.text("Rond af op hele centimeters", "Round to whole centimetres"),
        }
    }
}

/// Which snaps are on. All of them, until switched off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snaps {
    pub end: bool,
    pub middle: bool,
    pub grid: bool,
    pub line: bool,
    pub ortho: bool,
    pub round: bool,
}

impl Default for Snaps {
    fn default() -> Self {
        Self { end: true, middle: true, grid: true, line: true, ortho: true, round: true }
    }
}

impl Snaps {
    fn slot(&mut self, snap: Snap) -> &mut bool {
        match snap {
            Snap::End => &mut self.end,
            Snap::Middle => &mut self.middle,
            Snap::Grid => &mut self.grid,
            Snap::Line => &mut self.line,
            Snap::Ortho => &mut self.ortho,
            Snap::Round => &mut self.round,
        }
    }

    pub fn is(mut self, snap: Snap) -> bool {
        *self.slot(snap)
    }

    pub fn toggle(&mut self, snap: Snap) {
        let slot = self.slot(snap);
        *slot = !*slot;
    }
}

impl App {
    /// The lines a click can land on (in plan): the axes of walls, members and curtain walls, the
    /// edges of slabs and the grid lines.
    pub(in crate::app) fn snap_lines(&self) -> Vec<(DVec2, DVec2)> {
        let grid = self.modeller.model.grid;
        let mut lines = Vec::new();
        for e in self.modeller.model.iter() {
            match e.shape_in_project(&grid) {
                Shape::Linear(l) => lines.push((l.start.truncate(), l.end.truncate())),
                Shape::Planar(PlanarDef { form: PlanarForm::Wall { start, end, .. }, .. }) => lines.push((start, end)),
                Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, .. }, .. }) => {
                    let n = outline.len();
                    lines.extend((0..n).map(|k| (outline[k], outline[(k + 1) % n])));
                }
                Shape::System(s) => lines.push((s.start, s.end)),
                Shape::Point(_) | Shape::Connection(_) => {}
            }
        }
        lines.extend(self.modeller.model.grids().iter().map(|g| (g.start, g.end)));
        lines.retain(|(a, b)| (*b - *a).length_squared() > 1e-12);
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{click, modelling};
    use super::super::ModelTool;
    use super::*;
    use glam::DVec3;

    #[test]
    fn a_click_lands_on_the_middle_of_a_wall_and_on_its_line_unless_switched_off() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        click(&mut app, -20.0, 0.0);
        click(&mut app, 20.0, 0.0);
        app.set_model_tool(None);
        let (w, h) = (800.0, 600.0);
        let at = |app: &App, p: DVec3| {
            let (x, y) = app.on_screen(p, w, h).unwrap();
            app.snapped(p, x, y, w, h)
        };
        // near the middle: the middle; near the line elsewhere: on the line
        assert_eq!(at(&app, DVec3::new(0.03, 0.04, 0.0)), DVec3::new(0.0, 0.0, 0.0));
        let on = at(&app, DVec3::new(10.513, 0.04, 0.0));
        assert!((on - DVec3::new(10.513, 0.0, 0.0)).length() < 1e-9, "{on}");
        // switched off, the click is where it is, in whole centimetres; and not even those
        app.modeller.snaps.toggle(Snap::Middle);
        app.modeller.snaps.toggle(Snap::Line);
        assert_eq!(at(&app, DVec3::new(0.03, 0.04, 0.0)), DVec3::new(0.03, 0.04, 0.0));
        assert_eq!(at(&app, DVec3::new(10.513, 0.044, 0.0)), DVec3::new(10.51, 0.04, 0.0));
        app.modeller.snaps.toggle(Snap::Round);
        assert_eq!(at(&app, DVec3::new(10.513, 0.044, 0.0)), DVec3::new(10.513, 0.044, 0.0));
        // the ends go with their own switch
        assert_eq!(at(&app, DVec3::new(20.02, 0.03, 0.0)), DVec3::new(20.0, 0.0, 0.0));
        app.modeller.snaps.toggle(Snap::End);
        assert_eq!(at(&app, DVec3::new(20.02, 0.03, 0.0)), DVec3::new(20.02, 0.03, 0.0));
        assert!(Snap::ALL.iter().filter(|s| app.modeller.snaps.is(**s)).count() == 2);
    }
}
