//! The Edit tools of the Draw tab, on the line or shape picked on a floor plan: Explode makes a
//! shape loose lines again, and Offset leaves a copy of it a typed distance to the side clicked
//! (`docs/superpowers/specs/2026-11-18-modeller-draw-tab-design.md`).

use super::marks::MarkPick;
use super::App;
use openaec_ui::Language;

/// The distance Offset starts with (millimetres).
pub const OFFSET_DEFAULT: &str = "100";

/// The lines of a line or shape, `distance` to the side of `towards`: each corner where the
/// lines beside it meet again, so a rectangle stays a rectangle and a circle a circle. None for
/// no lines or no distance.
pub fn offset_lines(lines: &[[[f64; 2]; 2]], distance: f64, towards: [f64; 2]) -> Option<Vec<[[f64; 2]; 2]>> {
    if lines.is_empty() || !(distance.is_finite() && distance > 0.0) {
        return None;
    }
    let same = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9;
    // the left of each line, as a unit vector
    let normals: Vec<[f64; 2]> = lines
        .iter()
        .map(|l| {
            let (dx, dy) = (l[1][0] - l[0][0], l[1][1] - l[0][1]);
            let run = dx.hypot(dy).max(1e-12);
            [-dy / run, dx / run]
        })
        .collect();
    // the side clicked: that of the line nearest to the click
    let reach = |l: &[[f64; 2]; 2]| {
        let (dx, dy) = (l[1][0] - l[0][0], l[1][1] - l[0][1]);
        let t = (((towards[0] - l[0][0]) * dx + (towards[1] - l[0][1]) * dy) / (dx * dx + dy * dy).max(1e-18)).clamp(0.0, 1.0);
        let at = [l[0][0] + dx * t, l[0][1] + dy * t];
        ((towards[0] - at[0]).hypot(towards[1] - at[1]), at)
    };
    let (nearest, (_, at)) = lines.iter().map(reach).enumerate().min_by(|a, b| a.1 .0.total_cmp(&b.1 .0))?;
    let side = (towards[0] - at[0]) * normals[nearest][0] + (towards[1] - at[1]) * normals[nearest][1];
    let d = if side >= 0.0 { distance } else { -distance };
    let n = lines.len();
    let closed = n > 1 && same(lines[n - 1][1], lines[0][0]);
    // a point shared by lines `a` and `b`, moved to where their offsets meet
    let corner = |p: [f64; 2], a: usize, b: usize| {
        let (na, nb) = (normals[a], normals[b]);
        let along = 1.0 + na[0] * nb[0] + na[1] * nb[1];
        if along < 1e-6 {
            // a line turning back on itself has no corner to meet in
            return [p[0] + na[0] * d, p[1] + na[1] * d];
        }
        [p[0] + (na[0] + nb[0]) * d / along, p[1] + (na[1] + nb[1]) * d / along]
    };
    let alone = |p: [f64; 2], k: usize| [p[0] + normals[k][0] * d, p[1] + normals[k][1] * d];
    Some(
        (0..n)
            .map(|k| {
                let before = if k > 0 && same(lines[k - 1][1], lines[k][0]) { Some(k - 1) } else if k == 0 && closed { Some(n - 1) } else { None };
                let after = if k + 1 < n && same(lines[k][1], lines[k + 1][0]) { Some(k + 1) } else if k + 1 == n && closed { Some(0) } else { None };
                [before.map_or_else(|| alone(lines[k][0], k), |j| corner(lines[k][0], j, k)), after.map_or_else(|| alone(lines[k][1], k), |j| corner(lines[k][1], k, j))]
            })
            .collect(),
    )
}

impl App {
    /// Makes the shape picked on the floor plan shown loose lines again; the line clicked stays
    /// picked. False when no shape is picked.
    pub(super) fn explode_drawn(&mut self) -> bool {
        let Some((view, run)) = self.drawn_picked() else { return false };
        self.document.explode_view_shape(&view, run.start)
    }

    /// Whether the picked thing on the floor plan is a shape (to explode).
    pub(crate) fn shape_picked(&self) -> bool {
        self.drawn_picked().is_some_and(|(view, run)| self.document.shape_of(&view, run.start).1.is_some())
    }

    /// Offset on or off: on only with a line or shape picked on a floor plan.
    pub(super) fn toggle_offset(&mut self) {
        if self.modeller.offset.take().is_some() || self.drawn_picked().is_none() {
            return;
        }
        if self.modeller.tool.is_some() {
            self.set_model_tool(None);
        }
        self.modeller.transform = None;
        self.modeller.offset = Some(OFFSET_DEFAULT.to_string());
    }

    /// The distance typed for Offset, in metres: millimetres as typed, a comma or a point.
    pub(super) fn offset_distance(&self) -> Option<f64> {
        let typed = self.modeller.offset.as_ref()?.trim().replace(',', ".");
        typed.parse::<f64>().ok().filter(|mm| mm.is_finite() && *mm > 0.0).map(|mm| mm / 1000.0)
    }

    /// A click while Offset is on: a copy of the picked line or shape on that side of it, at the
    /// distance typed. The copy is picked, so the next click offsets on from it.
    pub(super) fn offset_click(&mut self, x: f32, y: f32, width: f32, height: f32) {
        let (z, ..) = self.work_plane();
        let (Some(p), Some(distance), Some((view, run))) = (self.plane_point(x, y, width, height, z), self.offset_distance(), self.drawn_picked()) else { return };
        let kind = self.document.shape_of(&view, run.start).1;
        let Some(lines) = offset_lines(&self.document.view_lines(&view)[run], distance, [p.x, p.y]) else { return };
        let first = self.document.view_lines(&view).len();
        let added = match kind {
            Some(kind) => self.document.add_view_shape(&view, kind, &lines),
            None => self.document.add_view_lines(&view, &lines),
        };
        if added > 0 {
            self.mark_pick = Some(MarkPick::Line(first));
        }
    }

    /// What Offset waits for, in the banner.
    pub(super) fn offset_hint(&self, l: Language) -> Option<String> {
        self.modeller.offset.as_ref()?;
        Some(format!("{}: {}", l.text("Offset", "Offset"), l.text("Klik aan de kant waar de kopie komt · Esc: stoppen", "Click on the side where the copy goes · Esc: stop")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_and_explode_take_the_shape_picked_on_the_floor_plan() {
        use crate::app::marks::DrawKind;
        use crate::app::Message;
        use crate::document::ShapeKind;
        let mut app = App::default();
        app.new_model("Level 0");
        app.viewport_size = (1000.0, 800.0);
        let level = std::sync::Arc::<str>::from(app.document.levels()[0].path.as_str());
        let _ = app.update(Message::NewPlan(level));
        app.anim = None;
        app.camera.set_preset(crate::viewport::camera::ViewPreset::Top);
        app.camera.ortho = true;
        app.camera.target = glam::Vec3::ZERO;
        app.camera.distance = 40.0 / (app.camera.fov_y * 0.5).tan();
        let plan = std::sync::Arc::<str>::from(app.shown_view().expect("the plan shows"));
        let _ = app.update(Message::AddShape(plan.clone(), DrawKind::Rectangle, DrawKind::Rectangle.shape_lines(&[[0.0, 0.0], [4.0, 2.0]])));
        // nothing picked: neither tool has anything to take
        let _ = app.update(Message::OffsetTool);
        assert!(app.modeller.offset.is_none() && !app.shape_picked() && !app.explode_drawn());
        let _ = app.update(Message::PickMark(Some(MarkPick::Line(0))));
        let _ = app.update(Message::OffsetTool);
        assert_eq!(app.modeller.offset.as_deref(), Some(OFFSET_DEFAULT));
        assert!(app.offset_hint(Language::English).is_some());
        // 250 mm typed, a click below the rectangle: a rectangle of 4.5 by 2.5 around it, picked
        let _ = app.update(Message::OffsetText("250".into()));
        assert_eq!(app.offset_distance(), Some(0.25));
        let (x, y) = app.on_screen(glam::DVec3::new(2.0, -3.0, 0.0), 1000.0, 800.0).unwrap();
        let _ = app.update(Message::Navigate(crate::viewport::Navigate::Pick { x, y, width: 1000.0, height: 800.0, add: false }));
        assert_eq!(app.document.view_shapes(&plan).iter().map(|s| (s.kind, s.run())).collect::<Vec<_>>(), [(ShapeKind::Rectangle, 0..4), (ShapeKind::Rectangle, 4..8)]);
        let l = app.document.view_lines(&plan)[4];
        assert!((l[0][0] + 0.25).abs() < 1e-9 && (l[0][1] + 0.25).abs() < 1e-9 && (l[1][0] - 4.25).abs() < 1e-9, "{l:?}");
        assert_eq!(app.mark_pick, Some(MarkPick::Line(4)));
        // a distance that is none offsets nothing; Esc ends the tool
        let _ = app.update(Message::OffsetText("x".into()));
        let _ = app.update(Message::Navigate(crate::viewport::Navigate::Pick { x, y, width: 1000.0, height: 800.0, add: false }));
        assert_eq!(app.document.view_lines(&plan).len(), 8);
        let _ = app.update(Message::Escape);
        assert!(app.modeller.offset.is_none());
        // Explode: the picked rectangle is four lines, each picked and removed alone
        assert!(app.shape_picked());
        let _ = app.update(Message::ExplodeDrawn);
        assert_eq!(app.document.view_shapes(&plan).iter().map(|s| s.run()).collect::<Vec<_>>(), [0..4]);
        assert!(!app.shape_picked());
        let _ = app.update(Message::RemoveMark);
        assert_eq!(app.document.view_lines(&plan).len(), 7);
    }

    #[test]
    fn an_offset_keeps_the_shape_and_goes_to_the_side_clicked() {
        let close = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9;
        // a line: parallel, on the side of the click
        let line = [[[0.0, 0.0], [4.0, 0.0]]];
        assert_eq!(offset_lines(&line, 0.5, [1.0, 3.0]), Some(vec![[[0.0, 0.5], [4.0, 0.5]]]));
        assert_eq!(offset_lines(&line, 0.5, [1.0, -3.0]), Some(vec![[[0.0, -0.5], [4.0, -0.5]]]));
        assert_eq!(offset_lines(&line, 0.0, [1.0, 3.0]), None);
        assert_eq!(offset_lines(&[], 1.0, [0.0, 0.0]), None);
        // a rectangle of 4 by 2, clicked outside: 5 by 3 about the same middle, still closed
        let square = crate::app::marks::DrawKind::Rectangle.shape_lines(&[[0.0, 0.0], [4.0, 2.0]]);
        let outside = offset_lines(&square, 0.5, [2.0, -1.0]).unwrap();
        assert!(close(outside[0][0], [-0.5, -0.5]) && close(outside[0][1], [4.5, -0.5]) && close(outside[1][1], [4.5, 2.5]), "{outside:?}");
        assert!(outside.windows(2).all(|w| close(w[0][1], w[1][0])) && close(outside[3][1], outside[0][0]));
        // clicked inside: 3 by 1
        let inside = offset_lines(&square, 0.5, [2.0, 0.2]).unwrap();
        assert!(close(inside[0][0], [0.5, 0.5]) && close(inside[1][1], [3.5, 1.5]), "{inside:?}");
        // a circle of radius 2 clicked outside: its chords on the circle of radius 2.5 (to a chord's sag)
        let circle = crate::app::marks::DrawKind::Circle.shape_lines(&[[1.0, 1.0], [3.0, 1.0]]);
        let wider = offset_lines(&circle, 0.5, [9.0, 1.0]).unwrap();
        assert!(wider.len() == 48 && wider.iter().all(|l| ((l[0][0] - 1.0).hypot(l[0][1] - 1.0) - 2.5).abs() < 0.01));
        // an open corner: the ends square off, the corner meets
        let corner = [[[0.0, 0.0], [2.0, 0.0]], [[2.0, 0.0], [2.0, 2.0]]];
        let left = offset_lines(&corner, 1.0, [1.0, 1.0]).unwrap();
        assert!(close(left[0][0], [0.0, 1.0]) && close(left[0][1], [1.0, 1.0]) && close(left[1][1], [1.0, 2.0]), "{left:?}");
    }
}
