//! The sketch of a floor's outline: Tab on a selected slab opens its outline as loose lines, to
//! draw on and to trim, as the boundary sketch of the reference modeller.
//!
//! - **The lines.** Each edge of the outline is a line of its own, straight or an arc; the edges
//!   of the floor's openings too. While the sketch is open they need not close: lines are added,
//!   cut and removed freely, and the floor stays as it was.
//! - **L** draws lines, each click a point, one line running on from the last. **R** draws a
//!   rectangle with two clicks. **A** draws an arc with three: its start, its end, and a point
//!   it runs through. A click near an end of a line, or near a point of another element, lands on
//!   it; else it is rounded as other moves are.
//! - **T** trims: a click on a line takes the stretch under it away, between the two nearest
//!   places where other lines cross or touch it; a line nothing crosses goes whole.
//! - **With no tool** a click picks a line (Delete removes it) and a drag moves a point, with the
//!   ends of every line that met there.
//! - **Tab finishes.** The lines must make closed loops: every point the end of two lines. A gap
//!   or a branch is told and the sketch stays open. The largest loop becomes the floor's outline,
//!   its arcs kept as arcs; every other loop an opening in it (an opening's arcs become chords).
//!   It is one step to take back; what the kernel refuses (loops that cross, an opening outside
//!   the outline) is told too.
//! - **Esc** lets a tool go, then leaves the sketch with the floor unchanged.

use crate::app::edit::{EditEvent, EditOverlay};
use crate::app::App;
use bim_core::element::{arc_of, arc_points, bowed, PlanarForm};
use bim_core::{ElementId, PlanarDef, Shape};
use glam::{DVec2, DVec3};
use iced::Color;
use openaec_ui::Language;
use std::f64::consts::{PI, TAU};

/// Points this close are one point (metres).
const MEET: f64 = 1e-4;
/// A click this near a point or a line takes it (logical pixels).
const REACH: f32 = 12.0;
/// A click is rounded to this (metres).
const ROUND: f64 = 0.01;

/// A line of the sketch, in plan: from `a` to `b`, straight or an arc with `bulge` (the tangent
/// of a quarter of its angle, above zero where it bows to the right of its way).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub a: DVec2,
    pub b: DVec2,
    pub bulge: f64,
}

impl Segment {
    pub fn line(a: DVec2, b: DVec2) -> Self {
        Self { a, b, bulge: 0.0 }
    }

    /// The arc from `a` to `b` through `p`; a straight line where the three lie on one.
    pub fn through(a: DVec2, b: DVec2, p: DVec2) -> Self {
        let side = (b - a).perp_dot(p - a);
        if side.abs() < MEET * a.distance(b) {
            return Self::line(a, b);
        }
        // the angle the chord is seen under from `p` gives how far the arc turns
        let seen = (a - p).angle_to(b - p).abs();
        Self { a, b, bulge: ((PI - seen) / 2.0).tan() * -side.signum() }
    }

    fn length(&self) -> f64 {
        arc_of(self.a, self.b, self.bulge).map_or(self.a.distance(self.b), |(_, radius, _, turn)| radius * turn.abs())
    }

    /// The point `t` of the way along it (0 its start, 1 its end).
    fn at(&self, t: f64) -> DVec2 {
        match arc_of(self.a, self.b, self.bulge) {
            Some((centre, radius, start, turn)) => centre + radius * DVec2::from_angle(start + turn * t),
            None => self.a.lerp(self.b, t),
        }
    }

    /// How far along it (0 to 1) its point nearest `p` lies.
    fn along(&self, p: DVec2) -> f64 {
        let Some((centre, _, start, turn)) = arc_of(self.a, self.b, self.bulge) else {
            let r = self.b - self.a;
            return ((p - self.a).dot(r) / r.length_squared().max(1e-300)).clamp(0.0, 1.0);
        };
        let q = p - centre;
        let angle = q.y.atan2(q.x);
        let t = if turn > 0.0 { (angle - start).rem_euclid(TAU) / turn } else { (start - angle).rem_euclid(TAU) / -turn };
        match t <= 1.0 {
            true => t,
            // past the arc: its nearer end
            false if p.distance(self.a) <= p.distance(self.b) => 0.0,
            false => 1.0,
        }
    }

    fn distance(&self, p: DVec2) -> f64 {
        self.at(self.along(p)).distance(p)
    }

    /// The stretch of it from `from` to `to` of the way along.
    fn part(&self, from: f64, to: f64) -> Self {
        let bulge = arc_of(self.a, self.b, self.bulge).map_or(0.0, |(.., turn)| (turn * (to - from) / 4.0).tan());
        Self { a: self.at(from), b: self.at(to), bulge }
    }

    /// Its points to draw it by: its ends for a straight line, chords along an arc.
    fn chords(&self) -> Vec<DVec2> {
        let mut points = arc_points(self.a, self.b, self.bulge);
        points.push(self.b);
        points
    }

    /// Where the line or the circle it lies on meets the one `other` lies on.
    fn carriers_meet(&self, other: &Segment) -> Vec<DVec2> {
        let circle = |s: &Segment| arc_of(s.a, s.b, s.bulge).map(|(centre, radius, ..)| (centre, radius));
        // a straight line through `a` along `r` against a circle
        let line_circle = |a: DVec2, r: DVec2, (centre, radius): (DVec2, f64)| -> Vec<DVec2> {
            let u = r.normalize_or_zero();
            let foot = a + u * (centre - a).dot(u);
            let off = foot.distance(centre);
            if off > radius + MEET {
                return Vec::new();
            }
            let half = (radius * radius - off * off).max(0.0).sqrt();
            vec![foot - u * half, foot + u * half]
        };
        match (circle(self), circle(other)) {
            (None, None) => {
                let (r, q) = (self.b - self.a, other.b - other.a);
                let denom = r.perp_dot(q);
                if denom.abs() < 1e-9 * r.length() * q.length() {
                    return Vec::new();
                }
                vec![self.a + r * ((other.a - self.a).perp_dot(q) / denom)]
            }
            (None, Some(c)) => line_circle(self.a, self.b - self.a, c),
            (Some(c), None) => line_circle(other.a, other.b - other.a, c),
            (Some((c1, r1)), Some((c2, r2))) => {
                let d = c1.distance(c2);
                if d < MEET || d > r1 + r2 + MEET || d < (r1 - r2).abs() - MEET {
                    return Vec::new();
                }
                let along = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
                let half = (r1 * r1 - along * along).max(0.0).sqrt();
                let u = (c2 - c1) / d;
                let foot = c1 + u * along;
                vec![foot + u.perp() * half, foot - u.perp() * half]
            }
        }
    }

    /// How far along it `other` crosses or touches it, between its ends.
    fn crossings(&self, other: &Segment) -> Vec<f64> {
        let length = self.length();
        self.carriers_meet(other)
            .into_iter()
            .filter(|p| self.distance(*p) <= MEET && other.distance(*p) <= MEET)
            .map(|p| self.along(p))
            .filter(|t| t * length > MEET && t * length < length - MEET)
            .collect()
    }
}

/// What a click in the sketch does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SketchTool {
    /// Picks a line; a drag moves a point.
    #[default]
    Pick,
    Line,
    Rectangle,
    Arc,
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
}

impl SketchError {
    pub fn reason(self, l: Language) -> &'static str {
        match self {
            Self::Empty => l.text("De schets heeft geen lijnen", "The sketch has no lines"),
            Self::Gap => l.text("De omtrek is niet gesloten", "The outline is not closed"),
            Self::Branch => l.text("In een punt komen meer dan twee lijnen samen", "More than two lines meet in a point"),
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
    /// The point clicked first, of a line, a rectangle or an arc under way.
    pub first: Option<DVec2>,
    /// The end of an arc under way, clicked second.
    pub second: Option<DVec2>,
    /// Where the next click would land, while the cursor is over the viewport.
    pub cursor: Option<DVec2>,
    /// The line picked.
    pub picked: Option<usize>,
    /// The point being dragged, where it is now.
    pub held: Option<DVec2>,
    /// Why Tab could not finish it.
    pub error: Option<SketchError>,
}

/// A closed loop of a sketch: its corners in order, each with the bulge of the edge that leaves it.
pub type Loop = Vec<(DVec2, f64)>;

/// The closed loops the lines make. Every point must be the end of two lines.
pub fn loops(segments: &[Segment]) -> Result<Vec<Loop>, SketchError> {
    let segments: Vec<Segment> = segments.iter().copied().filter(|s| s.length() > MEET).collect();
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
    let ends: Vec<[usize; 2]> = segments.iter().map(|s| [node(s.a), node(s.b)]).collect();
    let degree = |n: usize| ends.iter().flatten().filter(|&&e| e == n).count();
    if (0..points.len()).any(|n| degree(n) > 2) {
        return Err(SketchError::Branch);
    }
    if (0..points.len()).any(|n| degree(n) < 2) {
        return Err(SketchError::Gap);
    }
    // every point has two lines: walk each loop from a line not yet walked, round to its start
    let mut used = vec![false; segments.len()];
    let mut out = Vec::new();
    while let Some(first) = used.iter().position(|u| !u) {
        used[first] = true;
        let start = ends[first][0];
        let mut ring: Loop = vec![(points[start], segments[first].bulge)];
        let mut at = ends[first][1];
        while at != start {
            let Some(next) = (0..segments.len()).find(|&k| !used[k] && ends[k].contains(&at)) else { return Err(SketchError::Gap) };
            used[next] = true;
            // walked from its end to its start, a line bows the other way
            let forward = ends[next][0] == at;
            ring.push((points[at], if forward { segments[next].bulge } else { -segments[next].bulge }));
            at = ends[next][if forward { 1 } else { 0 }];
        }
        out.push(ring);
    }
    Ok(out)
}

/// The corners and the bulges of a loop apart.
fn apart(ring: &Loop) -> (Vec<DVec2>, Vec<f64>) {
    ring.iter().copied().unzip()
}

/// Twice the area inside a ring of points, whichever way it runs.
fn area2(ring: &[DVec2]) -> f64 {
    let n = ring.len();
    (0..n).map(|k| ring[k].perp_dot(ring[(k + 1) % n])).sum::<f64>().abs()
}

/// The lines with the stretch of line `index` under point `at` taken away: between the two
/// nearest places where other lines cross or touch it, or the whole line where none does.
pub fn trimmed(segments: &[Segment], index: usize, at: DVec2) -> Vec<Segment> {
    let Some(&s) = segments.get(index) else { return segments.to_vec() };
    let t0 = s.along(at);
    let cuts: Vec<f64> = segments.iter().enumerate().filter(|(k, _)| *k != index).flat_map(|(_, o)| s.crossings(o)).collect();
    let from = cuts.iter().copied().filter(|t| *t <= t0).fold(0.0, f64::max);
    let to = cuts.iter().copied().filter(|t| *t >= t0).fold(1.0, f64::min);
    let mut left = Vec::new();
    if from > 0.0 {
        left.push(s.part(0.0, from));
    }
    if to < 1.0 {
        left.push(s.part(to, 1.0));
    }
    let mut out = segments.to_vec();
    out.splice(index..=index, left);
    out
}

/// The four sides of the rectangle between corners `a` and `b`.
fn rectangle(a: DVec2, b: DVec2) -> [Segment; 4] {
    let (c, d) = (DVec2::new(b.x, a.y), DVec2::new(a.x, b.y));
    [Segment::line(a, c), Segment::line(c, b), Segment::line(b, d), Segment::line(d, a)]
}

impl Sketch {
    /// The points of the sketch, each once: the ends of its lines.
    fn points(&self) -> Vec<DVec2> {
        let mut points: Vec<DVec2> = Vec::new();
        for p in self.segments.iter().flat_map(|s| [s.a, s.b]) {
            if !points.iter().any(|q| q.distance(p) <= MEET) {
                points.push(p);
            }
        }
        points
    }

    /// The lines a click would add now: from the points clicked to the cursor.
    fn preview(&self) -> Vec<Segment> {
        match (self.tool, self.first, self.second, self.cursor) {
            (SketchTool::Line, Some(a), _, Some(b)) | (SketchTool::Arc, Some(a), None, Some(b)) => vec![Segment::line(a, b)],
            (SketchTool::Rectangle, Some(a), _, Some(b)) => rectangle(a, b).to_vec(),
            (SketchTool::Arc, Some(a), Some(b), Some(p)) => vec![Segment::through(a, b, p)],
            _ => Vec::new(),
        }
    }

    /// A click of the Line, the Rectangle or the Arc tool at `p`.
    fn place(&mut self, p: DVec2) {
        self.error = None;
        match (self.tool, self.first, self.second) {
            (SketchTool::Line, Some(a), _) if a.distance(p) > MEET => {
                self.segments.push(Segment::line(a, p));
                self.first = Some(p);
            }
            (SketchTool::Rectangle, Some(a), _) if (a.x - p.x).abs() > MEET && (a.y - p.y).abs() > MEET => {
                self.segments.extend(rectangle(a, p));
                self.first = None;
            }
            (SketchTool::Arc, Some(a), None) if a.distance(p) > MEET => self.second = Some(p),
            (SketchTool::Arc, Some(a), Some(b)) => {
                self.segments.push(Segment::through(a, b, p));
                (self.first, self.second) = (None, None);
            }
            (SketchTool::Line | SketchTool::Rectangle | SketchTool::Arc, None, _) => self.first = Some(p),
            _ => {}
        }
    }
}

impl App {
    /// Whether `id` is a floor whose outline Tab opens as a sketch.
    pub(in crate::app) fn sketchable(&self, id: ElementId) -> bool {
        self.modeller.model.get(id).is_some_and(|e| matches!(e.shape, Shape::Planar(PlanarDef { form: PlanarForm::Slab { .. }, .. })))
    }

    /// Opens the sketch of floor `element`: its outline and its openings as lines.
    pub(in crate::app) fn open_sketch(&mut self, element: ElementId) {
        let Some(shape) = self.modeller.model.get(element).map(|e| e.shape_in_project(&self.modeller.model.grid)) else { return };
        let Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, holes, top, bulges, .. }, .. }) = shape else { return };
        let n = outline.len();
        let mut segments: Vec<Segment> = (0..n).map(|i| Segment { a: outline[i], b: outline[(i + 1) % n], bulge: bulges.get(i).copied().unwrap_or(0.0) }).collect();
        for hole in &holes {
            segments.extend((0..hole.len()).map(|i| Segment::line(hole[i], hole[(i + 1) % hole.len()])));
        }
        self.modeller.tool = None;
        self.modeller.points.clear();
        self.modeller.refusal = None;
        self.measure.tool = None;
        self.modeller.sketch = Some(Sketch { element, top, segments, tool: SketchTool::Pick, first: None, second: None, cursor: None, picked: None, held: None, error: None });
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
        let (index, s) = sketch.segments.iter().enumerate().min_by(|a, b| a.1.distance(at).total_cmp(&b.1.distance(at)))?;
        // near enough on the screen: the nearest point of the line against the cursor
        let (sx, sy) = self.on_screen(s.at(s.along(at)).extend(sketch.top), width, height)?;
        ((sx - x).hypot(sy - y) <= REACH).then_some((index, at))
    }

    /// A click in the viewport while the sketch is open. False when none is.
    pub(in crate::app) fn sketch_click(&mut self, x: f32, y: f32, width: f32, height: f32) -> bool {
        let Some(tool) = self.modeller.sketch.as_ref().map(|s| s.tool) else { return false };
        self.modeller.refusal = None;
        match tool {
            SketchTool::Line | SketchTool::Rectangle | SketchTool::Arc => {
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

    /// A key in the sketch: L, R, A and T take a tool (the same key again lets it go).
    pub(in crate::app) fn sketch_key(&mut self, key: char) {
        let Some(sketch) = self.modeller.sketch.as_mut() else { return };
        let tool = match key.to_ascii_lowercase() {
            'l' => SketchTool::Line,
            'r' => SketchTool::Rectangle,
            'a' => SketchTool::Arc,
            't' => SketchTool::Trim,
            _ => return,
        };
        sketch.tool = if sketch.tool == tool { SketchTool::Pick } else { tool };
        (sketch.first, sketch.second) = (None, None);
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
        if sketch.first.is_some() {
            (sketch.first, sketch.second) = (None, None);
        } else if sketch.tool != SketchTool::Pick {
            sketch.tool = SketchTool::Pick;
        } else {
            self.modeller.sketch = None;
            self.modeller.refusal = None;
        }
        true
    }

    /// Tab in the sketch: its largest loop becomes the floor's outline and every other loop an
    /// opening, if the lines close and the kernel can make the floor; else the sketch stays open
    /// and says why.
    pub(in crate::app) fn finish_sketch(&mut self) {
        let Some(sketch) = self.modeller.sketch.as_mut() else { return };
        let mut rings = match loops(&sketch.segments) {
            Ok(rings) => rings,
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
        // the largest loop is the outline, the others are openings in it
        let size = |ring: &Loop| {
            let (corners, bulges) = apart(ring);
            area2(&bowed(&corners, &bulges))
        };
        let Some(largest) = (0..rings.len()).max_by(|a, b| size(&rings[*a]).total_cmp(&size(&rings[*b]))) else { return };
        let (corners, bows) = apart(&rings.remove(largest));
        let mut shape = old.shape_in_project(&grid);
        let Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, holes, bulges, .. }, .. }) = &mut shape else { return };
        *outline = corners;
        *bulges = if bows.iter().any(|b| *b != 0.0) { bows } else { Vec::new() };
        *holes = rings
            .iter()
            .map(|ring| {
                let (corners, bows) = apart(ring);
                bowed(&corners, &bows)
            })
            .collect();
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
    /// `index` and a drag moves it, with every line's end that lay there; with a drawing tool a
    /// press on a point is a click on it. False when no sketch is open.
    pub(in crate::app) fn sketch_event(&mut self, event: EditEvent) -> bool {
        let Some(sketch) = self.modeller.sketch.as_mut() else { return false };
        match event {
            EditEvent::Press { index, .. } => {
                let Some(&p) = sketch.points().get(index) else { return true };
                match sketch.tool {
                    SketchTool::Pick => sketch.held = Some(p),
                    SketchTool::Line | SketchTool::Rectangle | SketchTool::Arc => sketch.place(p),
                    SketchTool::Trim => {}
                }
            }
            EditEvent::Drag { x, y, width, height } => {
                let Some(from) = sketch.held else { return true };
                let Some(to) = self.sketch_point(x, y, width, height, Some(from)) else { return true };
                if let Some(sketch) = self.modeller.sketch.as_mut() {
                    for s in sketch.segments.iter_mut() {
                        for end in [&mut s.a, &mut s.b] {
                            if end.distance(from) <= MEET {
                                *end = to;
                            }
                        }
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
        let lines = sketch.segments.iter().copied().chain(sketch.preview()).flat_map(|s| s.chords().windows(2).map(|w| [up(w[0]), up(w[1])]).collect::<Vec<_>>()).collect();
        Some(EditOverlay {
            // the ends of the picked line, and the point held, show filled
            picked: points.iter().map(|p| sketch.held.is_some_and(|h| h.distance(*p) <= MEET) || picked.is_some_and(|s| [s.a, s.b].iter().any(|e| e.distance(*p) <= MEET))).collect(),
            points: points.iter().map(|p| up(*p)).collect(),
            closed: false,
            lines: Some(lines),
            view_proj: self.camera.view_proj(self.aspect()) * self.model_matrix(),
            ink,
            fill,
        })
    }

    /// The sketch's tools as buttons, over the view's top left: the tool that is on shows so, and
    /// Finish and Cancel end the sketch as Tab and Esc do.
    pub(in crate::app) fn sketch_bar(&self, p: crate::theme::Palette, l: Language) -> Option<iced::Element<'_, crate::app::Message>> {
        use crate::app::Message;
        use iced::widget::{button, container, text, Row};
        let sketch = self.modeller.sketch.as_ref()?;
        let tool = |label: &'static str, key: char, on: bool| button(text(label).size(12)).padding([3, 10]).style(move |_, s| p.ribbon_button(on, s)).on_press(Message::EditKey(key));
        let is = |t: SketchTool| sketch.tool == t;
        let bar = Row::new()
            .spacing(4)
            .push(tool(l.text("Lijn (L)", "Line (L)"), 'l', is(SketchTool::Line)))
            .push(tool(l.text("Rechthoek (R)", "Rectangle (R)"), 'r', is(SketchTool::Rectangle)))
            .push(tool(l.text("Boog (A)", "Arc (A)"), 'a', is(SketchTool::Arc)))
            .push(tool(l.text("Trimmen (T)", "Trim (T)"), 't', is(SketchTool::Trim)))
            .push(button(text(l.text("Klaar (Tab)", "Finish (Tab)")).size(12)).padding([3, 10]).style(move |_, s| p.ribbon_button(false, s)).on_press(Message::EditMode))
            .push(button(text(l.text("Annuleren", "Cancel")).size(12)).padding([3, 10]).style(move |_, s| p.ribbon_button(false, s)).on_press(Message::SketchCancel));
        Some(container(container(bar).padding(4).style(move |_| p.overlay())).padding(iced::Padding { top: 48.0, left: 8.0, ..iced::Padding::ZERO }).into())
    }

    /// Cancel: the sketch leaves with the floor unchanged, whatever tool is on.
    pub(in crate::app) fn cancel_sketch(&mut self) {
        self.modeller.sketch = None;
        self.modeller.refusal = None;
    }

    /// The banner while the sketch is open: why it could not finish, and what the keys do.
    pub(in crate::app) fn sketch_hint(&self, l: Language) -> Option<String> {
        let sketch = self.modeller.sketch.as_ref()?;
        let hint = match sketch.tool {
            SketchTool::Pick => l.text(
                "Sleep een punt · klik een lijn: kiezen, Del: weg · L: lijn · R: rechthoek · A: boog · T: trimmen · Tab: klaar · Esc: annuleren",
                "Drag a point · click a line: pick, Del: remove · L: line · R: rectangle · A: arc · T: trim · Tab: done · Esc: cancel",
            ),
            SketchTool::Line => l.text("Lijn: klik de punten · Esc: stoppen · Tab: klaar", "Line: click the points · Esc: stop · Tab: done"),
            SketchTool::Rectangle => l.text("Rechthoek: klik twee hoeken · Esc: stoppen · Tab: klaar", "Rectangle: click two corners · Esc: stop · Tab: done"),
            SketchTool::Arc => l.text(
                "Boog: klik het begin, het eind en een punt op de boog · Esc: stoppen · Tab: klaar",
                "Arc: click its start, its end and a point on it · Esc: stop · Tab: done",
            ),
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
        Segment::line(DVec2::new(a.0, a.1), DVec2::new(b.0, b.1))
    }

    fn near(a: &[Segment], b: &[Segment]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(s, o)| s.a.distance(o.a) < 1e-9 && s.b.distance(o.b) < 1e-9 && (s.bulge - o.bulge).abs() < 1e-9)
    }

    #[test]
    fn loops_finds_the_outline_in_any_order_and_direction() {
        // a 4 × 3 rectangle, its sides shuffled and two of them turned round
        let sides = [seg((4.0, 3.0), (0.0, 3.0)), seg((0.0, 0.0), (4.0, 0.0)), seg((0.0, 0.0), (0.0, 3.0)), seg((4.0, 3.0), (4.0, 0.0))];
        let rings = loops(&sides).unwrap();
        assert_eq!(rings.len(), 1);
        assert!((area2(&apart(&rings[0]).0) - 24.0).abs() < 1e-9, "{rings:?}");
        // a line of no length counts for nothing; a second rectangle is a loop of its own
        let mut more = sides.to_vec();
        more.push(seg((1.0, 1.0), (1.0, 1.0)));
        more.extend(rectangle(DVec2::new(1.0, 1.0), DVec2::new(2.0, 2.0)));
        assert_eq!(loops(&more).unwrap().len(), 2);
    }

    #[test]
    fn loops_says_what_is_wrong() {
        assert_eq!(loops(&[]), Err(SketchError::Empty));
        let open = [seg((0.0, 0.0), (4.0, 0.0)), seg((4.0, 0.0), (4.0, 3.0)), seg((4.0, 3.0), (0.0, 3.0))];
        assert_eq!(loops(&open), Err(SketchError::Gap));
        let mut branch = rectangle(DVec2::ZERO, DVec2::new(4.0, 3.0)).to_vec();
        branch.push(seg((4.0, 3.0), (6.0, 5.0)));
        assert_eq!(loops(&branch), Err(SketchError::Branch));
    }

    #[test]
    fn trimmed_takes_the_stretch_between_crossings() {
        // a cross: the horizontal line trimmed left of the vertical keeps its right half
        let cross = [seg((0.0, 0.0), (4.0, 0.0)), seg((2.0, -1.0), (2.0, 1.0))];
        assert!(near(&trimmed(&cross, 0, DVec2::new(0.5, 0.0)), &[seg((2.0, 0.0), (4.0, 0.0)), cross[1]]));
        // between two crossings: the middle goes, both ends stay
        let two = [seg((0.0, 0.0), (6.0, 0.0)), seg((2.0, -1.0), (2.0, 1.0)), seg((4.0, 0.0), (4.0, 2.0))];
        assert!(near(&trimmed(&two, 0, DVec2::new(3.0, 0.1)), &[seg((0.0, 0.0), (2.0, 0.0)), seg((4.0, 0.0), (6.0, 0.0)), two[1], two[2]]));
        // a line nothing crosses goes whole; one that only meets others at its ends too
        assert!(trimmed(&[seg((0.0, 0.0), (4.0, 0.0))], 0, DVec2::new(1.0, 0.0)).is_empty());
        let corner = [seg((0.0, 0.0), (4.0, 0.0)), seg((4.0, 0.0), (4.0, 3.0))];
        assert!(near(&trimmed(&corner, 0, DVec2::new(1.0, 0.0)), &[corner[1]]));
    }

    #[test]
    fn an_arc_runs_through_its_three_points_and_is_trimmed_where_a_line_crosses_it() {
        // from (0, 0) to (2, 0) through (1, -1): half a circle about (1, 0), counter-clockwise
        let arc = Segment::through(DVec2::ZERO, DVec2::new(2.0, 0.0), DVec2::new(1.0, -1.0));
        assert!((arc.bulge - 1.0).abs() < 1e-9, "{}", arc.bulge);
        assert!(arc.at(0.5).distance(DVec2::new(1.0, -1.0)) < 1e-9);
        assert!((arc.length() - PI).abs() < 1e-9);
        // through a point on the other side it bows the other way
        assert!((Segment::through(DVec2::ZERO, DVec2::new(2.0, 0.0), DVec2::new(1.0, 1.0)).bulge + 1.0).abs() < 1e-9);
        // a vertical line through its lowest point: trimmed below, the line starts on the arc
        let both = [arc, seg((1.0, -2.0), (1.0, 2.0))];
        assert!(near(&trimmed(&both, 1, DVec2::new(1.0, -1.5)), &[arc, seg((1.0, -1.0), (1.0, 2.0))]));
        // the arc trimmed on its first half: its second half is left, a quarter circle
        let left = trimmed(&both, 0, DVec2::new(0.2, -0.6));
        let quarter = Segment { a: DVec2::new(1.0, -1.0), b: DVec2::new(2.0, 0.0), bulge: (PI / 8.0).tan() };
        assert!(near(&left, &[quarter, both[1]]), "{left:?}");
    }

    /// A floor 4 × 3 m with its top at 0, selected.
    fn floor() -> (App, ElementId) {
        let mut app = modelling();
        let outline = vec![DVec2::ZERO, DVec2::new(4.0, 0.0), DVec2::new(4.0, 3.0), DVec2::new(0.0, 3.0)];
        let shape = Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, holes: Vec::new(), top: 0.0, bulges: Vec::new(), role: Default::default(), slope: None }, thickness: 0.25, openings: Vec::new(), build_up: None });
        let id = app.add_element("Floor", None, shape).unwrap();
        app.keep_model_history();
        app.select_modelled(id);
        (app, id)
    }

    fn click(app: &mut App, x: f64, y: f64) {
        let (sx, sy) = app.on_screen(DVec3::new(x, y, 0.0), 1000.0, 800.0).unwrap();
        let _ = app.update(Message::Navigate(Navigate::Pick { x: sx, y: sy, width: 1000.0, height: 800.0, add: false }));
    }

    /// The outline, the openings and the bulges of floor `id`.
    fn slab_of(app: &App, id: ElementId) -> (Vec<DVec2>, Vec<Vec<DVec2>>, Vec<f64>) {
        let Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, holes, bulges, .. }, .. }) = app.modeller.model.get(id).unwrap().shape_in_project(&app.modeller.model.grid) else { panic!() };
        (outline, holes, bulges)
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
        // as it is, the two loops cross: the kernel makes no floor of them, the sketch stays
        let _ = app.update(Message::EditMode);
        assert!(app.modeller.sketch.is_some() && app.modeller.refusal.is_some());
        // trim what lies inside: the rectangle's west side with its two stubs, and the floor's
        // east side between them
        let _ = app.update(Message::EditKey('t'));
        click(&mut app, 3.0, 1.5);
        click(&mut app, 3.5, 1.0);
        click(&mut app, 3.5, 2.0);
        click(&mut app, 4.0, 1.5);
        let _ = app.update(Message::EditMode);
        assert!(app.modeller.sketch.is_none(), "{:?}", app.modeller.sketch.as_ref().map(|s| (s.error, s.segments.clone())));
        let (outline, holes, bulges) = slab_of(&app, id);
        assert_eq!((outline.len(), holes.len(), bulges.len()), (8, 0, 0), "{outline:?}");
        assert!((area2(&outline) - 2.0 * (12.0 + 2.0)).abs() < 1e-9, "the floor and the part of the rectangle outside it");
        assert_eq!(app.selected_modelled(), vec![id]);
        // one step to take back
        let _ = app.update(Message::Undo);
        assert_eq!(slab_of(&app, id).0.len(), 4);
    }

    #[test]
    fn a_loop_inside_the_outline_is_an_opening_and_opens_again_as_lines() {
        let (mut app, id) = floor();
        let _ = app.update(Message::EditMode);
        let _ = app.update(Message::EditKey('r'));
        click(&mut app, 1.0, 1.0);
        click(&mut app, 2.0, 2.0);
        let _ = app.update(Message::EditMode);
        assert!(app.modeller.sketch.is_none(), "{:?}", app.modeller.refusal);
        let (outline, holes, _) = slab_of(&app, id);
        assert_eq!((outline.len(), holes.len()), (4, 1));
        assert!((area2(&holes[0]) - 2.0).abs() < 1e-9);
        // opened again the opening's sides are lines too; removed, the floor is whole again
        app.select_modelled(id);
        let _ = app.update(Message::EditMode);
        assert_eq!(app.modeller.sketch.as_ref().unwrap().segments.len(), 8);
        for _ in 0..4 {
            if let Some(sketch) = app.modeller.sketch.as_mut() {
                sketch.picked = Some(4);
            }
            let _ = app.update(Message::DeletePicked);
        }
        let _ = app.update(Message::EditMode);
        assert_eq!(slab_of(&app, id).1.len(), 0);
        // an opening outside the outline is refused
        app.select_modelled(id);
        let _ = app.update(Message::EditMode);
        let _ = app.update(Message::EditKey('r'));
        click(&mut app, 6.0, 1.0);
        click(&mut app, 7.0, 2.0);
        let _ = app.update(Message::EditMode);
        assert!(app.modeller.sketch.is_some() && app.modeller.refusal.is_some());
    }

    #[test]
    fn a_side_drawn_as_an_arc_is_kept_as_an_arc() {
        let (mut app, id) = floor();
        let _ = app.update(Message::EditMode);
        // the east side goes, an arc from corner to corner through (5, 1.5) takes its place
        click(&mut app, 4.0, 1.5);
        let _ = app.update(Message::DeletePicked);
        let _ = app.update(Message::EditKey('a'));
        click(&mut app, 4.0, 0.0);
        click(&mut app, 4.0, 3.0);
        assert!(app.modeller.sketch.as_ref().is_some_and(|s| s.second.is_some()));
        click(&mut app, 5.0, 1.5);
        let _ = app.update(Message::EditMode);
        assert!(app.modeller.sketch.is_none(), "{:?}", app.modeller.refusal);
        let (outline, _, bulges) = slab_of(&app, id);
        assert_eq!(outline.len(), 4);
        assert_eq!(bulges.iter().filter(|b| **b != 0.0).count(), 1, "{bulges:?}");
        // it bows outward: the floor is larger than its four corners
        let ring = bowed(&outline, &bulges);
        assert!(ring.iter().any(|p| p.distance(DVec2::new(5.0, 1.5)) < 1e-6), "{ring:?}");
        assert!(area2(&ring) > 2.0 * 13.5);
        // opened again the arc is still one line
        app.select_modelled(id);
        let _ = app.update(Message::EditMode);
        let sketch = app.modeller.sketch.as_ref().unwrap();
        assert_eq!((sketch.segments.len(), sketch.segments.iter().filter(|s| s.bulge != 0.0).count()), (4, 1));
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
        assert_eq!(slab_of(&app, id).0.len(), 4, "the floor as it was");
    }

    #[test]
    fn the_sketchs_buttons_take_a_tool_finish_and_cancel() {
        let (mut app, id) = floor();
        let _ = app.update(Message::EditMode);
        let p = app.theme.palette();
        let said = |app: &App, label: &str| -> Vec<Message> {
            let mut ui = iced_test::Simulator::with_size(iced::Settings::default(), iced::Size::new(900.0, 200.0), app.sketch_bar(p, Language::English).expect("the bar"));
            ui.click(label).unwrap_or_else(|_| panic!("no {label}"));
            ui.into_messages().collect()
        };
        for m in said(&app, "Rectangle (R)") {
            let _ = app.update(m);
        }
        assert_eq!(app.modeller.sketch.as_ref().map(|s| s.tool), Some(SketchTool::Rectangle));
        click(&mut app, 1.0, 1.0);
        click(&mut app, 2.0, 2.0);
        for m in said(&app, "Finish (Tab)") {
            let _ = app.update(m);
        }
        assert!(app.modeller.sketch.is_none());
        assert_eq!(slab_of(&app, id).1.len(), 1, "the rectangle became an opening");
        // Cancel leaves at once, with a tool on and a point clicked
        app.select_modelled(id);
        let _ = app.update(Message::EditMode);
        let _ = app.update(Message::EditKey('l'));
        click(&mut app, 3.0, 1.0);
        for m in said(&app, "Cancel") {
            let _ = app.update(m);
        }
        assert!(app.modeller.sketch.is_none());
        assert_eq!(slab_of(&app, id).1.len(), 1, "unchanged");
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
        let (outline, ..) = slab_of(&app, id);
        assert!(outline.iter().any(|p| p.distance(DVec2::new(5.0, 4.0)) < 1e-9), "{outline:?}");
        assert!((area2(&outline) - 2.0 * (12.0 + 3.5)).abs() < 1e-9, "{outline:?}");
    }
}
