//! The Edit tools of the Draw tab, on the line or shape picked on a floor plan: Explode makes a
//! shape loose lines again, and Offset leaves a copy of it a typed distance to the side clicked
//! (`docs/superpowers/specs/2026-11-18-modeller-draw-tab-design.md`).

use super::marks::MarkPick;
use super::App;
use openaec_ui::Language;

/// The Edit tools of two clicks: a first line, then the one it works on (or with).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PairKind {
    /// The second line loses its part on the side clicked, up to the first.
    Trim,
    /// The second line reaches on to the first.
    Extend,
    /// The two lines meet in an arc of the radius typed (none: in a corner).
    Fillet,
    /// The two lines meet in a bevel, the distance typed back along each.
    Chamfer,
}

impl PairKind {
    pub const ALL: [PairKind; 4] = [PairKind::Trim, PairKind::Extend, PairKind::Fillet, PairKind::Chamfer];

    pub fn id(self) -> &'static str {
        match self {
            PairKind::Trim => "edit.trim",
            PairKind::Extend => "edit.extend",
            PairKind::Fillet => "edit.fillet",
            PairKind::Chamfer => "edit.chamfer",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            PairKind::Trim => "transform.trim",
            PairKind::Extend => "transform.extend",
            PairKind::Fillet => "transform.fillet",
            PairKind::Chamfer => "transform.chamfer",
        }
    }

    pub fn label(self, l: Language) -> &'static str {
        match self {
            PairKind::Trim => l.text("Trimmen", "Trim"),
            PairKind::Extend => l.text("Verlengen", "Extend"),
            PairKind::Fillet => l.text("Afronden", "Fillet"),
            PairKind::Chamfer => l.text("Afschuinen", "Chamfer"),
        }
    }

    /// Whether a size is typed for it: a fillet's radius, a chamfer's distance.
    pub fn sized(self) -> bool {
        matches!(self, PairKind::Fillet | PairKind::Chamfer)
    }

    /// What to click next: the first line, or the second.
    pub fn hint(self, l: Language, first: bool) -> &'static str {
        match (self, first) {
            (PairKind::Trim, true) => l.text("Klik de lijn waartegen getrimd wordt · Esc: stoppen", "Click the line to trim against · Esc: stop"),
            (PairKind::Trim, false) => l.text("Klik het deel van een lijn dat weg moet · Esc: opnieuw", "Click the part of a line that goes · Esc: start again"),
            (PairKind::Extend, true) => l.text("Klik de lijn waar naartoe verlengd wordt · Esc: stoppen", "Click the line to extend to · Esc: stop"),
            (PairKind::Extend, false) => l.text("Klik de lijn die verlengd wordt · Esc: opnieuw", "Click the line that is extended · Esc: start again"),
            (_, true) => l.text("Klik de eerste lijn, aan de kant die blijft · Esc: stoppen", "Click the first line, on the side that stays · Esc: stop"),
            (_, false) => l.text("Klik de tweede lijn, aan de kant die blijft · Esc: opnieuw", "Click the second line, on the side that stays · Esc: start again"),
        }
    }
}

/// A tool of two clicks under way: the first line clicked and where, and the size typed (mm).
#[derive(Debug, Clone, PartialEq)]
pub struct PairEdit {
    pub kind: PairKind,
    pub first: Option<(usize, [f64; 2])>,
    pub size: String,
}

/// Where the endless lines through `a` and `b` cross; none for parallel ones.
pub fn crossing(a: &[[f64; 2]; 2], b: &[[f64; 2]; 2]) -> Option<[f64; 2]> {
    let (r, s) = ([a[1][0] - a[0][0], a[1][1] - a[0][1]], [b[1][0] - b[0][0], b[1][1] - b[0][1]]);
    let cross = r[0] * s[1] - r[1] * s[0];
    if cross.abs() <= 1e-12 * (r[0].hypot(r[1]) * s[0].hypot(s[1])).max(1e-12) {
        return None;
    }
    let t = ((b[0][0] - a[0][0]) * s[1] - (b[0][1] - a[0][1]) * s[0]) / cross;
    Some([a[0][0] + r[0] * t, a[0][1] + r[1] * t])
}

/// The two lines as a tool of two clicks leaves them (none: as it was), and the lines it adds.
pub type PairResult = ([Option<[[f64; 2]; 2]>; 2], Vec<[[f64; 2]; 2]>);

/// What a tool of two clicks makes of line `a` (clicked at `pa`) and line `b` (clicked at `pb`):
/// the two lines as they become, and the lines it adds between them (the arc of a fillet, the
/// bevel of a chamfer). None when the lines do not cross or the size does not fit.
pub fn pair_edit(kind: PairKind, a: [[f64; 2]; 2], pa: [f64; 2], b: [[f64; 2]; 2], pb: [f64; 2], size: f64) -> Option<PairResult> {
    let x = crossing(&a, &b)?;
    let dist = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).hypot(p[1] - q[1]);
    // the end of a line on the side of a click, seen from the crossing
    let side = |line: &[[f64; 2]; 2], click: [f64; 2]| {
        let dot = |e: [f64; 2]| (click[0] - x[0]) * (e[0] - x[0]) + (click[1] - x[1]) * (e[1] - x[1]);
        if dot(line[0]) >= dot(line[1]) { 0 } else { 1 }
    };
    match kind {
        PairKind::Extend => {
            // the end nearest to the crossing goes there
            let end = if dist(b[0], x) <= dist(b[1], x) { 0 } else { 1 };
            let mut line = b;
            line[end] = x;
            (line[0] != line[1]).then_some(([None, Some(line)], Vec::new()))
        }
        PairKind::Trim => {
            // the crossing must be on the line trimmed; the end on the side clicked goes there
            let along = ((x[0] - b[0][0]) * (b[1][0] - b[0][0]) + (x[1] - b[0][1]) * (b[1][1] - b[0][1])) / dist(b[0], b[1]).powi(2).max(1e-18);
            if !(1e-9..=1.0 - 1e-9).contains(&along) {
                return None;
            }
            let mut line = b;
            line[side(&b, pb)] = x;
            Some(([None, Some(line)], Vec::new()))
        }
        PairKind::Fillet | PairKind::Chamfer => {
            // of each line the end on the side clicked stays; the other comes to the corner
            let (ka, kb) = (a[side(&a, pa)], b[side(&b, pb)]);
            let (la, lb) = (dist(ka, x), dist(kb, x));
            if la < 1e-9 || lb < 1e-9 {
                return None;
            }
            let (ua, ub) = ([(ka[0] - x[0]) / la, (ka[1] - x[1]) / la], [(kb[0] - x[0]) / lb, (kb[1] - x[1]) / lb]);
            if size <= 0.0 {
                return Some(([Some([ka, x]), Some([kb, x])], Vec::new()));
            }
            let half = (ua[0] * ub[0] + ua[1] * ub[1]).clamp(-1.0, 1.0).acos() / 2.0;
            // how far back from the corner each line ends
            let back = if kind == PairKind::Fillet { size / half.tan() } else { size };
            if !back.is_finite() || back >= la - 1e-9 || back >= lb - 1e-9 {
                return None;
            }
            let (ta, tb) = ([x[0] + ua[0] * back, x[1] + ua[1] * back], [x[0] + ub[0] * back, x[1] + ub[1] * back]);
            let between = if kind == PairKind::Chamfer {
                vec![[ta, tb]]
            } else {
                // the arc touches both lines: its middle is on the bisector, a radius short of its centre
                let (bx, by) = (ua[0] + ub[0], ua[1] + ub[1]);
                let run = bx.hypot(by).max(1e-12);
                let reach = size / half.sin() - size;
                super::marks::DrawKind::Arc.shape_lines(&[ta, [x[0] + bx / run * reach, x[1] + by / run * reach], tb])
            };
            Some(([Some([ka, ta]), Some([kb, tb])], between))
        }
    }
}

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
        self.modeller.pair_edit = None;
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

    /// A tool of two clicks on or off (the same one again: off). It works on a floor plan.
    pub(super) fn toggle_pair_edit(&mut self, kind: PairKind) {
        let same = self.modeller.pair_edit.as_ref().is_some_and(|e| e.kind == kind);
        self.modeller.pair_edit = None;
        if same || self.shown_view().is_none_or(|v| self.document.plan(v).is_none()) {
            return;
        }
        if self.modeller.tool.is_some() {
            self.set_model_tool(None);
        }
        self.modeller.transform = None;
        self.modeller.offset = None;
        self.draw_tool = None;
        self.modeller.pair_edit = Some(PairEdit { kind, first: None, size: if kind.sized() { OFFSET_DEFAULT.to_string() } else { String::new() } });
    }

    /// Esc: the first line clicked goes, else the tool. False when none was on.
    pub(super) fn escape_pair_edit(&mut self) -> bool {
        match self.modeller.pair_edit.as_mut() {
            Some(e) if e.first.is_some() => e.first = None,
            Some(_) => self.modeller.pair_edit = None,
            None => return false,
        }
        true
    }

    /// A click while a tool of two clicks is on: the line of the plan under it is its first
    /// line, or its second, and then the tool does its work and waits for a next pair.
    pub(super) fn pair_click(&mut self, x: f32, y: f32, width: f32, height: f32) {
        let (z, ..) = self.work_plane();
        let (Some(p), Some(aside)) = (self.plane_point(x, y, width, height, z), self.plane_point(x + 8.0, y, width, height, z)) else { return };
        let Some(view) = self.shown_view().filter(|v| self.document.plan(v).is_some()).map(str::to_string) else { return };
        let (p, reach) = ([p.x, p.y], p.distance(aside));
        // the line under the click: the nearest within eight pixels
        let away = |l: &[[f64; 2]; 2]| {
            let (dx, dy) = (l[1][0] - l[0][0], l[1][1] - l[0][1]);
            let t = (((p[0] - l[0][0]) * dx + (p[1] - l[0][1]) * dy) / (dx * dx + dy * dy).max(1e-18)).clamp(0.0, 1.0);
            (p[0] - l[0][0] - dx * t).hypot(p[1] - l[0][1] - dy * t)
        };
        let Some((k, _)) = self.document.view_lines(&view).iter().map(away).enumerate().filter(|(_, d)| *d <= reach).min_by(|a, b| a.1.total_cmp(&b.1)) else { return };
        let Some(edit) = self.modeller.pair_edit.clone() else { return };
        let Some((first, at)) = edit.first.filter(|(first, _)| *first != k) else {
            if let Some(e) = self.modeller.pair_edit.as_mut() {
                e.first = Some((k, p));
            }
            self.mark_pick = Some(MarkPick::Line(k));
            return;
        };
        let size = edit.size.trim().replace(',', ".").parse::<f64>().ok().filter(|mm| mm.is_finite() && *mm >= 0.0).map_or(0.0, |mm| mm / 1000.0);
        let lines = self.document.view_lines(&view);
        let Some((changed, between)) = pair_edit(edit.kind, lines[first], at, lines[k], p, size) else { return };
        // a line of a shape changed is a line of its own from here on
        for (index, line) in [first, k].into_iter().zip(changed) {
            if let Some(line) = line {
                self.document.explode_view_shape(&view, index);
                self.document.set_view_line(&view, index, line);
            }
        }
        if between.len() > 1 {
            self.document.add_view_shape(&view, crate::document::ShapeKind::Arc, &between);
        } else {
            self.document.add_view_lines(&view, &between);
        }
        self.mark_pick = None;
        if let Some(e) = self.modeller.pair_edit.as_mut() {
            e.first = None;
        }
    }

    /// What a tool of two clicks waits for, in the banner.
    pub(super) fn pair_hint(&self, l: Language) -> Option<String> {
        let e = self.modeller.pair_edit.as_ref()?;
        Some(format!("{}: {}", e.kind.label(l), e.kind.hint(l, e.first.is_none())))
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
    fn the_tools_of_two_clicks_trim_extend_round_and_bevel() {
        let close = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9;
        let level = [[0.0, 0.0], [4.0, 0.0]];
        let upright = [[2.0, -1.0], [2.0, 3.0]];
        assert_eq!(crossing(&level, &upright), Some([2.0, 0.0]));
        assert_eq!(crossing(&level, &[[0.0, 1.0], [4.0, 1.0]]), None);
        // trim the upright against the level line: the part clicked goes
        let (lines, between) = pair_edit(PairKind::Trim, level, [1.0, 0.0], upright, [2.0, 2.0], 0.0).unwrap();
        assert_eq!((lines, between.len()), ([None, Some([[2.0, -1.0], [2.0, 0.0]])], 0));
        let (lines, _) = pair_edit(PairKind::Trim, level, [1.0, 0.0], upright, [2.0, -0.5], 0.0).unwrap();
        assert_eq!(lines[1], Some([[2.0, 0.0], [2.0, 3.0]]));
        // a line that does not reach the other has nothing to trim, and is extended to it
        let short = [[2.0, 1.0], [2.0, 3.0]];
        assert!(pair_edit(PairKind::Trim, level, [1.0, 0.0], short, [2.0, 2.0], 0.0).is_none());
        let (lines, _) = pair_edit(PairKind::Extend, level, [1.0, 0.0], short, [2.0, 2.0], 0.0).unwrap();
        assert_eq!(lines, [None, Some([[2.0, 0.0], [2.0, 3.0]])]);
        assert!(pair_edit(PairKind::Extend, level, [0.0, 0.0], [[0.0, 1.0], [4.0, 1.0]], [1.0, 1.0], 0.0).is_none(), "parallel");
        // a fillet of radius 0: a corner of the sides clicked
        let (lines, between) = pair_edit(PairKind::Fillet, level, [3.0, 0.0], upright, [2.0, 2.0], 0.0).unwrap();
        assert_eq!((lines, between.len()), ([Some([[4.0, 0.0], [2.0, 0.0]]), Some([[2.0, 3.0], [2.0, 0.0]])], 0));
        // radius 0.5 in a right angle: each line ends 0.5 short, the arc is on the circle about (2.5, 0.5)
        let (lines, arc) = pair_edit(PairKind::Fillet, level, [3.0, 0.0], upright, [2.0, 2.0], 0.5).unwrap();
        assert!(close(lines[0].unwrap()[1], [2.5, 0.0]) && close(lines[1].unwrap()[1], [2.0, 0.5]));
        assert!(arc.len() >= 2 && close(arc[0][0], [2.5, 0.0]) && close(arc[arc.len() - 1][1], [2.0, 0.5]));
        assert!(arc.iter().all(|l| ((l[0][0] - 2.5).hypot(l[0][1] - 0.5) - 0.5).abs() < 1e-9), "{arc:?}");
        // a radius that does not fit the lines is refused
        assert!(pair_edit(PairKind::Fillet, level, [3.0, 0.0], upright, [2.0, 2.0], 5.0).is_none());
        // a chamfer of 0.5: a bevel between the two points 0.5 back
        let (lines, bevel) = pair_edit(PairKind::Chamfer, level, [1.0, 0.0], upright, [2.0, -0.5], 0.5).unwrap();
        assert!(close(lines[0].unwrap()[0], [0.0, 0.0]) && close(lines[0].unwrap()[1], [1.5, 0.0]) && close(lines[1].unwrap()[1], [2.0, -0.5]));
        assert_eq!(bevel.len(), 1);
        assert!(close(bevel[0][0], [1.5, 0.0]) && close(bevel[0][1], [2.0, -0.5]));
        for kind in PairKind::ALL {
            assert!(crate::b2::IDS.contains(&kind.icon()) && !kind.hint(Language::Dutch, true).is_empty() && !kind.label(Language::English).is_empty());
        }
    }

    #[test]
    fn two_clicks_on_a_floor_plan_round_a_corner_of_a_rectangle() {
        use crate::app::marks::DrawKind;
        use crate::app::Message;
        let mut app = App::default();
        app.new_model("Level 0");
        app.viewport_size = (1000.0, 800.0);
        // no floor plan: the tool has nowhere to work
        let _ = app.update(Message::PairEdit(PairKind::Fillet));
        assert!(app.modeller.pair_edit.is_none());
        let level = std::sync::Arc::<str>::from(app.document.levels()[0].path.as_str());
        let _ = app.update(Message::NewPlan(level));
        app.anim = None;
        app.camera.set_preset(crate::viewport::camera::ViewPreset::Top);
        app.camera.ortho = true;
        app.camera.target = glam::Vec3::ZERO;
        app.camera.distance = 40.0 / (app.camera.fov_y * 0.5).tan();
        let plan = std::sync::Arc::<str>::from(app.shown_view().expect("the plan shows"));
        let _ = app.update(Message::AddShape(plan.clone(), DrawKind::Rectangle, DrawKind::Rectangle.shape_lines(&[[0.0, 0.0], [8.0, 6.0]])));
        let click = |app: &mut App, x: f64, y: f64| {
            let (x, y) = app.on_screen(glam::DVec3::new(x, y, 0.0), 1000.0, 800.0).unwrap();
            let _ = app.update(Message::Navigate(crate::viewport::Navigate::Pick { x, y, width: 1000.0, height: 800.0, add: false }));
        };
        let _ = app.update(Message::PairEdit(PairKind::Fillet));
        let _ = app.update(Message::PairSize("1000".into()));
        assert!(app.pair_hint(Language::English).is_some());
        // a click on nothing takes no line
        click(&mut app, 4.0, 3.0);
        assert!(app.modeller.pair_edit.as_ref().is_some_and(|e| e.first.is_none()));
        // the bottom side, then the right side: the corner (8, 0) is rounded with radius 1
        click(&mut app, 4.0, 0.0);
        assert_eq!(app.modeller.pair_edit.as_ref().and_then(|e| e.first).map(|f| f.0), Some(0));
        click(&mut app, 8.0, 3.0);
        let lines = app.document.view_lines(&plan);
        assert!((lines[0][1][0] - 7.0).abs() < 1e-6 && (lines[1][1][1] - 1.0).abs() < 1e-6, "{:?} {:?}", lines[0], lines[1]);
        assert!(lines.len() > 5 && lines[4..].iter().all(|l| ((l[0][0] - 7.0).hypot(l[0][1] - 1.0) - 1.0).abs() < 1e-6));
        // the rectangle is loose lines now, the arc a shape; the tool waits for the next pair
        assert_eq!(app.document.view_shapes(&plan).iter().map(|s| (s.kind, s.first)).collect::<Vec<_>>(), [(crate::document::ShapeKind::Arc, 4)]);
        assert!(app.modeller.pair_edit.as_ref().is_some_and(|e| e.first.is_none()));
        let _ = app.update(Message::Escape);
        assert!(app.modeller.pair_edit.is_none());
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
