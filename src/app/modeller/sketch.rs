//! The sketch of a floor's outline: Tab on a selected slab opens its outline as loose lines, to
//! draw on and to trim, as the boundary sketch of the reference modeller.
//!
//! - **The lines.** Each edge of the outline is a line of its own. While the sketch is open they
//!   need not close: lines are added, cut and removed freely, and the floor stays as it was.
//! - **L** draws lines, each click a point, one line running on from the last. **R** draws a
//!   rectangle with two clicks. A click near an end of a line, or near a point of another
//!   element, lands on it; else it is rounded as other moves are.
//! - **T** trims: a click on a line takes the stretch under it away, between the two nearest
//!   places where other lines cross or touch it; a line nothing crosses goes whole.
//! - **With no tool** a click picks a line (Delete removes it) and a drag moves a point, with the
//!   ends of every line that met there.
//! - **Tab finishes.** The lines must make one closed loop: every point the end of two lines.
//!   A gap, a branch or a second loop is told and the sketch stays open. The loop becomes the
//!   floor's outline in one step to take back; what the kernel refuses is told too.
//! - **Esc** lets a tool go, then leaves the sketch with the floor unchanged.

use crate::app::edit::{EditEvent, EditOverlay};
use crate::app::App;
use openaec_ui::Language;
use bim_core::element::PlanarForm;
use bim_core::{ElementId, PlanarDef, Shape};
use glam::{DVec2, DVec3};
use iced::Color;

/// Points this close are one point (metres).
const MEET: f64 = 1e-4;
/// A click this near a point or a line takes it (logical pixels).
const REACH: f32 = 12.0;
/// A click is rounded to this (metres).
const ROUND: f64 = 0.01;

/// A line of the sketch, in plan.
pub type Segment = [DVec2; 2];

/// What a click in the sketch does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SketchTool {
    /// Picks a line; a drag moves a point.
    #[default]
    Pick,
    Line,
    Rectangle,
    Trim,
}

/// Why the lines of a sketch make no outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SketchError {
    /// No lines.
    Empty,
    /// A line ends where no other begins.
    Gap,
    /// More than two lines meet in a point.
    Branch,
    /// The lines make more than one loop.
    TwoLoops,
}

impl SketchError {
    pub fn reason(self, l: Language) -> &'static str {
        match self {
            Self::Empty => l.text("De schets heeft geen lijnen", "The sketch has no lines"),
            Self::Gap => l.text("De omtrek is niet gesloten", "The outline is not closed"),
            Self::Branch => l.text("In een punt komen meer dan twee lijnen samen", "More than two lines meet in a point"),
            Self::TwoLoops => l.text("De schets heeft meer dan één omtrek", "The sketch has more than one outline"),
        }
    }
}

/// The sketch of a floor's outline, open.
#[derive(Debug, Clone)]
pub struct Sketch {
    pub element: ElementId,
    /// The height the sketch lies at: the floor's top (scene coordinates).
    pub top: f64,
    pub segments: Vec<Segment>,
    pub tool: SketchTool,
    /// The point clicked first, of a line or a rectangle under way.
    pub first: Option<DVec2>,
    /// Where the next click would land, while the cursor is over the viewport.
    pub cursor: Option<DVec2>,
    /// The line picked.
    pub picked: Option<usize>,
    /// The point being dragged, where it is now.
    pub held: Option<DVec2>,
    /// Why Tab could not finish it.
    pub error: Option<SketchError>,
}

/// The one closed loop the lines make, as the corners of an outline in order.
pub fn closed_loop(segments: &[Segment]) -> Result<Vec<DVec2>, SketchError> {
    let segments: Vec<Segment> = segments.iter().copied().filter(|s| s[0].distance(s[1]) > MEET).collect();
    if segments.is_empty() {
        return Err(SketchError::Empty);
    }
    // the points, and for each line the two it runs between
    let mut points: Vec<DVec2> = Vec::new();
    let mut node = |p: DVec2| match points.iter().position(|q| q.distance(p) <= MEET) {
        Some(i) => i,
        None => {
            points.push(p);
            points.len() - 1
        }
    };
    let ends: Vec<[usize; 2]> = segments.iter().map(|s| [node(s[0]), node(s[1])]).collect();
    let degree = |n: usize| ends.iter().flatten().filter(|&&e| e == n).count();
    if (0..points.len()).any(|n| degree(n) > 2) {
        return Err(SketchError::Branch);
    }
    if (0..points.len()).any(|n| degree(n) < 2) {
        return Err(SketchError::Gap);
    }
    // every point has two lines: walk from the first line round to where it began
    let mut used = vec![false; segments.len()];
    let mut outline = vec![points[ends[0][0]]];
    let (mut at, mut line) = (ends[0][1], 0);
    used[0] = true;
    while at != ends[0][0] {
        outline.push(points[at]);
        let Some(next) = (0..segments.len()).find(|&k| !used[k] && k != line && ends[k].contains(&at)) else { return Err(SketchError::Gap) };
        used[next] = true;
        at = if ends[next][0] == at { ends[next][1] } else { ends[next][0] };
        line = next;
    }
    if used.iter().any(|u| !u) {
        return Err(SketchError::TwoLoops);
    }
    Ok(outline)
}

/// How far along line `s` (0 at its start, 1 at its end) line `other` crosses or touches it,
/// between its ends; `None` when it does not, or runs parallel.
fn crossing_along(s: Segment, other: Segment) -> Option<f64> {
    let (r, q) = (s[1] - s[0], other[1] - other[0]);
    let (length, theirs) = (r.length(), q.length());
    let denom = r.perp_dot(q);
    if denom.abs() < 1e-9 * length * theirs {
        return None;
    }
    let t = (other[0] - s[0]).perp_dot(q) / denom;
    let u = (other[0] - s[0]).perp_dot(r) / denom;
    let on_theirs = u * theirs >= -MEET && u * theirs <= theirs + MEET;
    let inside = t * length > MEET && t * length < length - MEET;
    (on_theirs && inside).then_some(t)
}

/// The lines with the stretch of line `index` under point `at` taken away: between the two
/// nearest places where other lines cross or touch it, or the whole line where none does.
pub fn trimmed(segments: &[Segment], index: usize, at: DVec2) -> Vec<Segment> {
    let Some(&s) = segments.get(index) else { return segments.to_vec() };
    let r = s[1] - s[0];
    let t0 = ((at - s[0]).dot(r) / r.length_squared().max(1e-300)).clamp(0.0, 1.0);
    let cuts: Vec<f64> = segments.iter().enumerate().filter(|(k, _)| *k != index).filter_map(|(_, o)| crossing_along(s, *o)).collect();
    let from = cuts.iter().copied().filter(|t| *t <= t0).fold(0.0, f64::max);
    let to = cuts.iter().copied().filter(|t| *t >= t0).fold(1.0, f64::min);
    let mut out = segments.to_vec();
    let mut left = Vec::new();
    if from > 0.0 {
        left.push([s[0], s[0] + r * from]);
    }
    if to < 1.0 {
        left.push([s[0] + r * to, s[1]]);
    }
    out.splice(index..=index, left);
    out
}

/// How far point `p` lies from line `s`.
fn distance_to(s: Segment, p: DVec2) -> f64 {
    let r = s[1] - s[0];
    let t = ((p - s[0]).dot(r) / r.length_squared().max(1e-300)).clamp(0.0, 1.0);
    (s[0] + r * t).distance(p)
}

/// The four sides of the rectangle between corners `a` and `b`.
fn rectangle(a: DVec2, b: DVec2) -> [Segment; 4] {
    let (c, d) = (DVec2::new(b.x, a.y), DVec2::new(a.x, b.y));
    [[a, c], [c, b], [b, d], [d, a]]
}

impl Sketch {
    /// The points of the sketch, each once: the ends of its lines.
    fn points(&self) -> Vec<DVec2> {
        let mut points: Vec<DVec2> = Vec::new();
        for p in self.segments.iter().flatten() {
            if !points.iter().any(|q| q.distance(*p) <= MEET) {
                points.push(*p);
            }
        }
        points
    }

    /// The lines a click would add now: the line or the rectangle from the first point to the
    /// cursor.
    fn preview(&self) -> Vec<Segment> {
        match (self.tool, self.first, self.cursor) {
            (SketchTool::Line, Some(a), Some(b)) => vec![[a, b]],
            (SketchTool::Rectangle, Some(a), Some(b)) => rectangle(a, b).to_vec(),
            _ => Vec::new(),
        }
    }

    /// A click of the Line or the Rectangle tool at `p`.
    fn place(&mut self, p: DVec2) {
        self.error = None;
        match (self.tool, self.first) {
            (SketchTool::Line, Some(a)) if a.distance(p) > MEET => {
                self.segments.push([a, p]);
                self.first = Some(p);
            }
            (SketchTool::Rectangle, Some(a)) if (a.x - p.x).abs() > MEET && (a.y - p.y).abs() > MEET => {
                self.segments.extend(rectangle(a, p));
                self.first = None;
            }
            (SketchTool::Line | SketchTool::Rectangle, None) => self.first = Some(p),
            _ => {}
        }
    }
}

impl App {
    /// Whether `id` is a floor whose outline Tab opens as a sketch.
    pub(in crate::app) fn sketchable(&self, id: ElementId) -> bool {
        self.modeller.model.get(id).is_some_and(|e| matches!(e.shape, Shape::Planar(PlanarDef { form: PlanarForm::Slab { .. }, .. })))
    }

    /// Opens the sketch of floor `element`: its outline as lines.
    pub(in crate::app) fn open_sketch(&mut self, element: ElementId) {
        let Some(shape) = self.modeller.model.get(element).map(|e| e.shape_in_project(&self.modeller.model.grid)) else { return };
        let Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, top, .. }, .. }) = shape else { return };
        let n = outline.len();
        let segments = (0..n).map(|i| [outline[i], outline[(i + 1) % n]]).collect();
        self.modeller.tool = None;
        self.modeller.points.clear();
        self.modeller.refusal = None;
        self.measure.tool = None;
        self.modeller.sketch = Some(Sketch { element, top, segments, tool: SketchTool::Pick, first: None, cursor: None, picked: None, held: None, error: None });
    }

    /// Where a click at viewport point (x, y) lands in the sketch: on a point of the sketch or of
    /// another element near the cursor, else on the sketch's plane, rounded.
    fn sketch_point(&self, x: f32, y: f32, width: f32, height: f32, but: Option<DVec2>) -> Option<DVec2> {
        let sketch = self.modeller.sketch.as_ref()?;
        let own = sketch.points().into_iter().filter(|p| but.is_none_or(|b| b.distance(*p) > MEET)).map(|p| p.extend(sketch.top));
        let near = own
            .chain(self.snap_points_except(sketch.element))
            .filter_map(|q| self.on_screen(DVec3::new(q.x, q.y, sketch.top), width, height).map(|s| (q, (s.0 - x).hypot(s.1 - y))))
            .filter(|(_, d)| *d <= REACH)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        match near {
            Some((q, _)) => Some(q.truncate()),
            None => self.plane_point(x, y, width, height, sketch.top).map(|p| DVec2::new((p.x / ROUND).round() * ROUND, (p.y / ROUND).round() * ROUND)),
        }
    }

    /// The line of the sketch under viewport point (x, y), and the point of the sketch's plane
    /// there.
    fn sketch_line_under(&self, x: f32, y: f32, width: f32, height: f32) -> Option<(usize, DVec2)> {
        let sketch = self.modeller.sketch.as_ref()?;
        let at = self.plane_point(x, y, width, height, sketch.top)?.truncate();
        let (index, s) = sketch.segments.iter().enumerate().min_by(|a, b| distance_to(*a.1, at).total_cmp(&distance_to(*b.1, at)))?;
        // near enough on the screen: the nearest point of the line against the cursor
        let r = s[1] - s[0];
        let t = ((at - s[0]).dot(r) / r.length_squared().max(1e-300)).clamp(0.0, 1.0);
        let (sx, sy) = self.on_screen((s[0] + r * t).extend(sketch.top), width, height)?;
        ((sx - x).hypot(sy - y) <= REACH).then_some((index, at))
    }

    /// A click in the viewport while the sketch is open. False when none is.
    pub(in crate::app) fn sketch_click(&mut self, x: f32, y: f32, width: f32, height: f32) -> bool {
        let Some(tool) = self.modeller.sketch.as_ref().map(|s| s.tool) else { return false };
        self.modeller.refusal = None;
        match tool {
            SketchTool::Line | SketchTool::Rectangle => {
                let p = self.sketch_point(x, y, width, height, None);
                if let (Some(p), Some(sketch)) = (p, self.modeller.sketch.as_mut()) {
                    sketch.place(p);
                }
            }
            SketchTool::Trim => {
                let under = self.sketch_line_under(x, y, width, height);
                if let (Some((index, at)), Some(sketch)) = (under, self.modeller.sketch.as_mut()) {
                    sketch.segments = trimmed(&sketch.segments, index, at);
                    sketch.picked = None;
                    sketch.error = None;
                }
            }
            SketchTool::Pick => {
                let under = self.sketch_line_under(x, y, width, height).map(|(index, _)| index);
                if let Some(sketch) = self.modeller.sketch.as_mut() {
                    sketch.picked = under;
                }
            }
        }
        true
    }

    /// The cursor rests at (x, y) over the sketch: the line under way runs to it. False when no
    /// sketch is open.
    pub(in crate::app) fn sketch_hover(&mut self, x: f32, y: f32, width: f32, height: f32) -> bool {
        if self.modeller.sketch.is_none() {
            return false;
        }
        let p = self.sketch_point(x, y, width, height, None);
        if let Some(sketch) = self.modeller.sketch.as_mut() {
            sketch.cursor = p;
        }
        true
    }

    /// A key in the sketch: L, R and T take a tool (the same key again lets it go).
    pub(in crate::app) fn sketch_key(&mut self, key: char) {
        let Some(sketch) = self.modeller.sketch.as_mut() else { return };
        let tool = match key.to_ascii_lowercase() {
            'l' => SketchTool::Line,
            'r' => SketchTool::Rectangle,
            't' => SketchTool::Trim,
            _ => return,
        };
        sketch.tool = if sketch.tool == tool { SketchTool::Pick } else { tool };
        sketch.first = None;
        sketch.picked = None;
    }

    /// Delete in the sketch: the picked line goes. False when no sketch is open.
    pub(in crate::app) fn sketch_delete(&mut self) -> bool {
        let Some(sketch) = self.modeller.sketch.as_mut() else { return false };
        if let Some(index) = sketch.picked.take().filter(|i| *i < sketch.segments.len()) {
            sketch.segments.remove(index);
            sketch.error = None;
        }
        true
    }

    /// Esc in the sketch: a line under way stops, then the tool goes, then the sketch leaves with
    /// the floor unchanged. False when no sketch is open.
    pub(in crate::app) fn sketch_escape(&mut self) -> bool {
        let Some(sketch) = self.modeller.sketch.as_mut() else { return false };
        if sketch.first.take().is_some() {
        } else if sketch.tool != SketchTool::Pick {
            sketch.tool = SketchTool::Pick;
        } else {
            self.modeller.sketch = None;
            self.modeller.refusal = None;
        }
        true
    }

    /// Tab in the sketch: its lines become the floor's outline, if they make one loop and the
    /// kernel can make the floor; else the sketch stays open and says why.
    pub(in crate::app) fn finish_sketch(&mut self) {
        let Some(sketch) = self.modeller.sketch.as_mut() else { return };
        let outline = match closed_loop(&sketch.segments) {
            Ok(outline) => outline,
            Err(error) => {
                sketch.error = Some(error);
                return;
            }
        };
        let id = sketch.element;
        let grid = self.modeller.model.grid;
        let Some(old) = self.modeller.model.get(id).cloned() else {
            self.modeller.sketch = None;
            return;
        };
        let mut shape = old.shape_in_project(&grid);
        let Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline: ring, .. }, .. }) = &mut shape else { return };
        *ring = outline;
        match self.modeller.model.replace(old.reshaped(&grid, shape)) {
            Ok(()) => {
                self.modeller.sketch = None;
                self.modeller.refusal = None;
                self.show_modelled();
                self.select_modelled(id);
            }
            Err(refusal) => self.modeller.refusal = Some(refusal),
        }
    }

    /// Something the overlay reports while the sketch is open: with no tool a press takes point
    /// `index` and a drag moves it, with every line's end that lay there; with the Line or the
    /// Rectangle tool a press on a point is a click on it. False when no sketch is open.
    pub(in crate::app) fn sketch_event(&mut self, event: EditEvent) -> bool {
        let Some(sketch) = self.modeller.sketch.as_mut() else { return false };
        match event {
            EditEvent::Press { index, .. } => {
                let Some(&p) = sketch.points().get(index) else { return true };
                match sketch.tool {
                    SketchTool::Pick => sketch.held = Some(p),
                    SketchTool::Line | SketchTool::Rectangle => sketch.place(p),
                    SketchTool::Trim => {}
                }
            }
            EditEvent::Drag { x, y, width, height } => {
                let Some(from) = sketch.held else { return true };
                let Some(to) = self.sketch_point(x, y, width, height, Some(from)) else { return true };
                if let Some(sketch) = self.modeller.sketch.as_mut() {
                    for end in sketch.segments.iter_mut().flatten().filter(|e| e.distance(from) <= MEET) {
                        *end = to;
                    }
                    sketch.held = Some(to);
                    sketch.error = None;
                }
            }
            EditEvent::Release => sketch.held = None,
        }
        true
    }

    /// The sketch over the viewport: its lines, the line under way, and a point at every end.
    pub(in crate::app) fn sketch_overlay(&self, ink: Color, fill: Color) -> Option<EditOverlay> {
        let sketch = self.modeller.sketch.as_ref()?;
        let up = |p: DVec2| p.extend(sketch.top).as_vec3();
        let points = sketch.points();
        let picked = sketch.picked.and_then(|i| sketch.segments.get(i));
        Some(EditOverlay {
            // the ends of the picked line, and the point held, show filled
            picked: points.iter().map(|p| sketch.held.is_some_and(|h| h.distance(*p) <= MEET) || picked.is_some_and(|s| s.iter().any(|e| e.distance(*p) <= MEET))).collect(),
            points: points.iter().map(|p| up(*p)).collect(),
            closed: false,
            lines: Some(sketch.segments.iter().copied().chain(sketch.preview()).map(|s| [up(s[0]), up(s[1])]).collect()),
            view_proj: self.camera.view_proj(self.aspect()) * self.model_matrix(),
            ink,
            fill,
        })
    }

    /// The banner while the sketch is open: why it could not finish, and what the keys do.
    pub(in crate::app) fn sketch_hint(&self, l: Language) -> Option<String> {
        let sketch = self.modeller.sketch.as_ref()?;
        let hint = match sketch.tool {
            SketchTool::Pick => l.text(
                "Sleep een punt · klik een lijn: kiezen, Del: weg · L: lijn · R: rechthoek · T: trimmen · Tab: klaar · Esc: annuleren",
                "Drag a point · click a line: pick, Del: remove · L: line · R: rectangle · T: trim · Tab: done · Esc: cancel",
            ),
            SketchTool::Line => l.text("Lijn: klik de punten · Esc: stoppen · Tab: klaar", "Line: click the points · Esc: stop · Tab: done"),
            SketchTool::Rectangle => l.text("Rechthoek: klik twee hoeken · Esc: stoppen · Tab: klaar", "Rectangle: click two corners · Esc: stop · Tab: done"),
            SketchTool::Trim => l.text("Trimmen: klik het stuk dat weg moet · Esc: stoppen · Tab: klaar", "Trim: click the stretch to take away · Esc: stop · Tab: done"),
        };
        let why = sketch.error.map(|e| e.reason(l)).or_else(|| self.modeller.refusal.map(|r| l.translate(r.reason())));
        Some(match why {
            Some(why) => format!("{}: {} · {}", l.text("Vloerschets", "Floor sketch"), why, hint),
            None => format!("{}: {}", l.text("Vloerschets", "Floor sketch"), hint),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::modelling;
    use super::*;
    use crate::app::Message;
    use crate::viewport::Navigate;

    fn seg(a: (f64, f64), b: (f64, f64)) -> Segment {
        [DVec2::new(a.0, a.1), DVec2::new(b.0, b.1)]
    }

    /// Twice the area inside `outline`, whichever way it runs.
    fn area2(outline: &[DVec2]) -> f64 {
        let n = outline.len();
        (0..n).map(|k| outline[k].perp_dot(outline[(k + 1) % n])).sum::<f64>().abs()
    }

    #[test]
    fn closed_loop_finds_the_outline_in_any_order_and_direction() {
        // a 4 × 3 rectangle, its sides shuffled and two of them turned round
        let sides = [seg((4.0, 3.0), (0.0, 3.0)), seg((0.0, 0.0), (4.0, 0.0)), seg((0.0, 0.0), (0.0, 3.0)), seg((4.0, 3.0), (4.0, 0.0))];
        let outline = closed_loop(&sides).unwrap();
        assert_eq!(outline.len(), 4);
        assert!((area2(&outline) - 24.0).abs() < 1e-9, "{outline:?}");
        // a line of no length counts for nothing
        let mut with_dot = sides.to_vec();
        with_dot.push(seg((1.0, 1.0), (1.0, 1.0)));
        assert_eq!(closed_loop(&with_dot).unwrap().len(), 4);
    }

    #[test]
    fn closed_loop_says_what_is_wrong() {
        assert_eq!(closed_loop(&[]), Err(SketchError::Empty));
        let open = [seg((0.0, 0.0), (4.0, 0.0)), seg((4.0, 0.0), (4.0, 3.0)), seg((4.0, 3.0), (0.0, 3.0))];
        assert_eq!(closed_loop(&open), Err(SketchError::Gap));
        let mut branch = rectangle(DVec2::ZERO, DVec2::new(4.0, 3.0)).to_vec();
        branch.push(seg((4.0, 3.0), (6.0, 5.0)));
        assert_eq!(closed_loop(&branch), Err(SketchError::Branch));
        let mut two = rectangle(DVec2::ZERO, DVec2::new(4.0, 3.0)).to_vec();
        two.extend(rectangle(DVec2::new(10.0, 0.0), DVec2::new(12.0, 2.0)));
        assert_eq!(closed_loop(&two), Err(SketchError::TwoLoops));
    }

    #[test]
    fn trimmed_takes_the_stretch_between_crossings() {
        // a cross: the horizontal line trimmed left of the vertical keeps its right half
        let cross = [seg((0.0, 0.0), (4.0, 0.0)), seg((2.0, -1.0), (2.0, 1.0))];
        assert_eq!(trimmed(&cross, 0, DVec2::new(0.5, 0.0)), vec![seg((2.0, 0.0), (4.0, 0.0)), cross[1]]);
        // between two crossings: the middle goes, both ends stay
        let two = [seg((0.0, 0.0), (6.0, 0.0)), seg((2.0, -1.0), (2.0, 1.0)), seg((4.0, 0.0), (4.0, 2.0))];
        assert_eq!(trimmed(&two, 0, DVec2::new(3.0, 0.1)), vec![seg((0.0, 0.0), (2.0, 0.0)), seg((4.0, 0.0), (6.0, 0.0)), two[1], two[2]]);
        // a line nothing crosses goes whole; one that only meets others at its ends too
        assert_eq!(trimmed(&[seg((0.0, 0.0), (4.0, 0.0))], 0, DVec2::new(1.0, 0.0)), Vec::<Segment>::new());
        let corner = [seg((0.0, 0.0), (4.0, 0.0)), seg((4.0, 0.0), (4.0, 3.0))];
        assert_eq!(trimmed(&corner, 0, DVec2::new(1.0, 0.0)), vec![corner[1]]);
    }

    /// A floor 4 × 3 m with its top at 0, selected.
    fn floor() -> (App, ElementId) {
        let mut app = modelling();
        let outline = vec![DVec2::ZERO, DVec2::new(4.0, 0.0), DVec2::new(4.0, 3.0), DVec2::new(0.0, 3.0)];
        let shape = Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, holes: Vec::new(), top: 0.0 }, thickness: 0.25, openings: Vec::new(), build_up: None });
        let id = app.add_element("Floor", None, shape).unwrap();
        app.keep_model_history();
        app.select_modelled(id);
        (app, id)
    }

    fn click(app: &mut App, x: f64, y: f64) {
        let (sx, sy) = app.on_screen(DVec3::new(x, y, 0.0), 1000.0, 800.0).unwrap();
        let _ = app.update(Message::Navigate(Navigate::Pick { x: sx, y: sy, width: 1000.0, height: 800.0, add: false }));
    }

    fn outline_of(app: &App, id: ElementId) -> Vec<DVec2> {
        let Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, .. }, .. }) = app.modeller.model.get(id).unwrap().shape_in_project(&app.modeller.model.grid) else { panic!() };
        outline
    }

    #[test]
    fn a_rectangle_drawn_on_a_floor_and_trimmed_becomes_its_outline() {
        let (mut app, id) = floor();
        let _ = app.update(Message::EditMode);
        assert_eq!(app.modeller.sketch.as_ref().map(|s| s.segments.len()), Some(4), "Tab opens the sketch");
        assert!(app.modeller.edit.is_none());
        // a rectangle over the east side: from (3, 1) to (6, 2)
        let _ = app.update(Message::EditKey('r'));
        click(&mut app, 3.0, 1.0);
        click(&mut app, 6.0, 2.0);
        assert_eq!(app.modeller.sketch.as_ref().unwrap().segments.len(), 8);
        // as it is, the lines branch nowhere but make two loops that cross: not one outline
        let _ = app.update(Message::EditMode);
        assert_eq!(app.modeller.sketch.as_ref().unwrap().error, Some(SketchError::TwoLoops));
        // trim what lies inside: the rectangle's west side with its two stubs, and the floor's
        // east side between them
        let _ = app.update(Message::EditKey('t'));
        click(&mut app, 3.0, 1.5);
        click(&mut app, 3.5, 1.0);
        click(&mut app, 3.5, 2.0);
        click(&mut app, 4.0, 1.5);
        let _ = app.update(Message::EditMode);
        assert!(app.modeller.sketch.is_none(), "{:?}", app.modeller.sketch.as_ref().map(|s| (s.error, s.segments.clone())));
        let outline = outline_of(&app, id);
        assert_eq!(outline.len(), 8, "{outline:?}");
        assert!((area2(&outline) - 2.0 * (12.0 + 2.0)).abs() < 1e-9, "the floor and the part of the rectangle outside it");
        assert_eq!(app.selected_modelled(), vec![id]);
        // one step to take back
        let _ = app.update(Message::Undo);
        assert_eq!(outline_of(&app, id).len(), 4);
    }

    #[test]
    fn esc_leaves_the_sketch_with_the_floor_unchanged_and_a_gap_is_told() {
        let (mut app, id) = floor();
        let _ = app.update(Message::EditMode);
        // a line picked and removed: the outline has a gap, Tab says so and the sketch stays
        click(&mut app, 2.0, 0.0);
        assert_eq!(app.modeller.sketch.as_ref().unwrap().picked, Some(0));
        let _ = app.update(Message::DeletePicked);
        assert_eq!(app.modeller.sketch.as_ref().unwrap().segments.len(), 3);
        let _ = app.update(Message::EditMode);
        assert_eq!(app.modeller.sketch.as_ref().unwrap().error, Some(SketchError::Gap));
        assert!(app.sketch_hint(Language::English).unwrap().contains("not closed"));
        // a line drawn back in, from corner to corner (the clicks land on the points)
        let _ = app.update(Message::EditKey('l'));
        click(&mut app, 0.05, 0.05);
        click(&mut app, 3.95, 0.05);
        assert_eq!(app.modeller.sketch.as_ref().unwrap().segments[3], seg((0.0, 0.0), (4.0, 0.0)));
        // Esc: the line under way stops, then the tool goes, then the sketch leaves
        let _ = app.update(Message::Escape);
        assert!(app.modeller.sketch.as_ref().is_some_and(|s| s.first.is_none() && s.tool == SketchTool::Line));
        let _ = app.update(Message::Escape);
        assert!(app.modeller.sketch.as_ref().is_some_and(|s| s.tool == SketchTool::Pick));
        let _ = app.update(Message::Escape);
        assert!(app.modeller.sketch.is_none());
        assert_eq!(outline_of(&app, id).len(), 4, "the floor as it was");
    }

    #[test]
    fn a_point_dragged_takes_the_ends_that_met_there() {
        let (mut app, id) = floor();
        let _ = app.update(Message::EditMode);
        let overlay = app.edit_overlay(Color::BLACK, Color::WHITE).expect("the sketch shows");
        assert_eq!((overlay.points.len(), overlay.lines.as_ref().map(Vec::len)), (4, Some(4)));
        // the corner at (4, 3), point 2, out to (5, 4)
        let (sx, sy) = app.on_screen(DVec3::new(5.0, 4.0, 0.0), 1000.0, 800.0).unwrap();
        let _ = app.update(Message::Edit(EditEvent::Press { index: 2, add: false }));
        let _ = app.update(Message::Edit(EditEvent::Drag { x: sx, y: sy, width: 1000.0, height: 800.0 }));
        let _ = app.update(Message::Edit(EditEvent::Release));
        let _ = app.update(Message::EditMode);
        let outline = outline_of(&app, id);
        assert!(outline.iter().any(|p| p.distance(DVec2::new(5.0, 4.0)) < 1e-9), "{outline:?}");
        assert!((area2(&outline) - 2.0 * (12.0 + 3.5)).abs() < 1e-9, "{outline:?}");
    }
}
