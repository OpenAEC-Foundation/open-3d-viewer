//! A 2D drawing to export: lines, texts and dimensions, each with a pen, built once from a view
//! or a sheet (as the sheet draws it) and written by every export format: DWG, DXF, IFCCAD, PDF,
//! PNG and SVG. Metres: in the model for a view (drawn at 1 : scale on paper), on the paper for a
//! sheet. Headless, like the document.

use crate::document::{Dimension, Document, Note, SHEET_SCALE};
use crate::drawing::Cut;
use std::sync::Arc;

/// Text on paper: 2.5 mm capitals (metres).
const TEXT: f64 = 0.0025;
/// A dimension line stands this far beside what it measures, on paper (metres), as on a sheet.
const DIMENSION_OFFSET: f64 = 0.006;
/// Space between a placed view's lines and its frame (paper metres), as on a sheet.
const FRAME: f64 = 0.005;
/// Space around a view's drawing on its page (paper metres).
const MARGIN: f64 = 0.010;
/// How long the arrows of a section's mark are (paper metres), as on a sheet.
const ARROW: f64 = 0.005;
/// How wide a text is per character, for its capitals' height.
const CHARACTER: f64 = 0.8;
/// The radius of a grid's bubble on paper (metres), and how many lines draw it round.
const BUBBLE: f64 = 0.004;
const BUBBLE_SIDES: usize = 24;

/// What a line or text is, and so how it is drawn and which CAD layer it goes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pen {
    /// Where the plan's or section's plane cuts the model: bold.
    Cut,
    /// Seen beyond the cut: thin.
    Beyond,
    /// The hatch of a cut face: thin.
    Hatch,
    Dimension,
    Text,
    /// The sheet's border, the marks of sections.
    Frame,
    /// Grid lines and levels, with their bubbles and names.
    Grids,
}

impl Pen {
    pub const ALL: [Pen; 7] = [Pen::Cut, Pen::Beyond, Pen::Hatch, Pen::Dimension, Pen::Text, Pen::Frame, Pen::Grids];

    /// The CAD layer name.
    pub fn name(self) -> &'static str {
        match self {
            Pen::Cut => "O3V-Cut",
            Pen::Beyond => "O3V-Beyond",
            Pen::Hatch => "O3V-Hatch",
            Pen::Dimension => "O3V-Dimensions",
            Pen::Text => "O3V-Text",
            Pen::Frame => "O3V-Frame",
            Pen::Grids => "O3V-Grids",
        }
    }

    /// The line weight on paper (millimetres).
    pub fn weight_mm(self) -> f64 {
        match self {
            Pen::Cut => 0.50,
            Pen::Beyond | Pen::Dimension | Pen::Grids => 0.18,
            Pen::Hatch => 0.13,
            Pen::Text => 0.25,
            Pen::Frame => 0.35,
        }
    }
}

/// What a drawing's metres are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Space {
    /// The model's metres, drawn at 1 : `scale` on paper.
    Model { scale: f64 },
    /// The paper's metres.
    Paper,
}

/// A line from `a` to `b`.
#[derive(Debug, Clone, PartialEq)]
pub struct Line2d {
    pub pen: Pen,
    pub a: [f64; 2],
    pub b: [f64; 2],
}

/// Which point of a text [`Text2d::at`] is: its lower left, the middle of its bottom or its lower
/// right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// A text, level, standing on `at`; `height` is that of its capitals.
#[derive(Debug, Clone, PartialEq)]
pub struct Text2d {
    pub pen: Pen,
    pub at: [f64; 2],
    pub height: f64,
    pub text: String,
    pub align: Align,
}

impl Text2d {
    /// About how wide it is.
    pub fn width(&self) -> f64 {
        self.text.chars().count() as f64 * self.height * CHARACTER
    }

    /// About where its left and right ends are.
    fn span(&self) -> (f64, f64) {
        let w = self.width();
        match self.align {
            Align::Left => (self.at[0], self.at[0] + w),
            Align::Center => (self.at[0] - w / 2.0, self.at[0] + w / 2.0),
            Align::Right => (self.at[0] - w, self.at[0]),
        }
    }
}

/// A dimension between two points: its line `offset` to the left of `from` → `to`, with
/// extension lines and a slash at each end, and `value` (metres, what it measures in the model)
/// in whole millimetres beside it.
#[derive(Debug, Clone, PartialEq)]
pub struct Dim2d {
    pub from: [f64; 2],
    pub to: [f64; 2],
    pub value: f64,
    pub offset: f64,
    pub text_height: f64,
}

impl Dim2d {
    /// Along `from` → `to` and to its left (units); `None` when the points coincide.
    pub fn axes(&self) -> Option<([f64; 2], [f64; 2])> {
        let (dx, dy) = (self.to[0] - self.from[0], self.to[1] - self.from[1]);
        let run = dx.hypot(dy);
        (run > 0.0).then(|| ([dx / run, dy / run], [-dy / run, dx / run]))
    }

    /// Where the dimension line runs: from, to.
    pub fn line(&self) -> Option<([f64; 2], [f64; 2])> {
        let (_, n) = self.axes()?;
        Some((add(self.from, n, self.offset), add(self.to, n, self.offset)))
    }

    /// Its extension lines (from just off the points to just past its line), its dimension line
    /// and its two slashes, as a sheet draws them.
    pub fn lines(&self) -> Vec<[[f64; 2]; 2]> {
        let (Some((u, n)), Some((a, b))) = (self.axes(), self.line()) else { return Vec::new() };
        let (gap, tick) = (self.offset * 0.25, self.offset * 0.2);
        let slash = |q: [f64; 2]| {
            let s = [(u[0] + n[0]) * tick, (u[1] + n[1]) * tick];
            [[q[0] - s[0], q[1] - s[1]], [q[0] + s[0], q[1] + s[1]]]
        };
        vec![[add(self.from, n, gap), add(a, n, gap)], [add(self.to, n, gap), add(b, n, gap)], [a, b], slash(a), slash(b)]
    }

    /// Its text: the value in whole millimetres, level and centred beside the line, clear of it
    /// whichever way the line runs.
    pub fn label(&self) -> Text2d {
        let text = format!("{}", (self.value * 1000.0).round());
        let n = self.axes().map_or([0.0, 1.0], |(_, n)| n);
        let h = self.text_height;
        let w = text.chars().count() as f64 * h * CHARACTER;
        let clear = n[0].abs() * w / 2.0 + n[1].abs() * h / 2.0 + self.offset * 0.25;
        let middle = [(self.from[0] + self.to[0]) / 2.0, (self.from[1] + self.to[1]) / 2.0];
        let centre = add(middle, n, self.offset + clear);
        Text2d { pen: Pen::Dimension, at: [centre[0], centre[1] - h / 2.0], height: h, text, align: Align::Center }
    }
}

/// A grid line or a level on a view's drawing: its name, its ends (the drawing's metres) and, for a
/// level, its height.
#[derive(Debug, Clone, PartialEq)]
pub struct Datum2 {
    pub name: String,
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub level: Option<f64>,
}

/// A drawing to export.
#[derive(Debug, Clone, PartialEq)]
pub struct Drawing2d {
    /// The view's or sheet's name, for the file name.
    pub name: String,
    pub space: Space,
    /// A sheet's paper (width, height, metres).
    pub paper: Option<[f64; 2]>,
    pub lines: Vec<Line2d>,
    pub texts: Vec<Text2d>,
    pub dimensions: Vec<Dim2d>,
}

impl Drawing2d {
    /// Draws grid lines and levels as the view's tab shows them: a grid with a bubble and its name
    /// past both ends, a level with its name and height past both ends.
    pub fn datums(&mut self, datums: &[Datum2]) {
        let unit = 1.0 / self.paper_per_unit();
        let (r, h) = (BUBBLE * unit, TEXT * unit);
        for d in datums {
            let (dx, dy) = (d.b[0] - d.a[0], d.b[1] - d.a[1]);
            let run = dx.hypot(dy);
            if run <= 0.0 {
                continue;
            }
            self.line(Pen::Grids, d.a, d.b);
            let u = [dx / run, dy / run];
            match d.level {
                None => {
                    for (end, out) in [(d.a, [-u[0], -u[1]]), (d.b, u)] {
                        let c = add(end, out, r);
                        let point = |k: usize| {
                            let t = std::f64::consts::TAU * k as f64 / BUBBLE_SIDES as f64;
                            [c[0] + r * t.cos(), c[1] + r * t.sin()]
                        };
                        for k in 0..BUBBLE_SIDES {
                            self.line(Pen::Grids, point(k), point(k + 1));
                        }
                        self.texts.push(Text2d { pen: Pen::Grids, at: [c[0], c[1] - h / 2.0], height: h, text: d.name.clone(), align: Align::Center });
                    }
                }
                Some(z) => {
                    let label = format!("{} {z:+.3}", d.name);
                    let gap = h * 0.6;
                    let (right, left) = (add(d.b, u, gap), add(d.a, [-u[0], -u[1]], gap));
                    self.texts.push(Text2d { pen: Pen::Grids, at: [right[0], right[1] - h / 2.0], height: h, text: label.clone(), align: Align::Left });
                    self.texts.push(Text2d { pen: Pen::Grids, at: [left[0], left[1] - h / 2.0], height: h, text: label, align: Align::Right });
                }
            }
        }
    }

    /// Paper metres per metre of the drawing.
    pub fn paper_per_unit(&self) -> f64 {
        match self.space {
            Space::Model { scale } => 1.0 / scale,
            Space::Paper => 1.0,
        }
    }

    /// Lower left and upper right of what is drawn; `None` for nothing.
    pub fn extent(&self) -> Option<([f64; 2], [f64; 2])> {
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        let mut grow = |p: [f64; 2]| {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        };
        for l in &self.lines {
            grow(l.a);
            grow(l.b);
        }
        let labels: Vec<Text2d> = self.dimensions.iter().map(Dim2d::label).collect();
        for t in self.texts.iter().chain(&labels) {
            let (left, right) = t.span();
            grow([left, t.at[1]]);
            grow([right, t.at[1] + t.height]);
        }
        for [a, b] in self.dimensions.iter().flat_map(Dim2d::lines) {
            grow(a);
            grow(b);
        }
        lo[0].is_finite().then_some((lo, hi))
    }

    /// The page it is printed on, in its own metres: a sheet's paper, else what is drawn with a
    /// margin of 10 mm on paper (a page of 10 × 10 cm on paper when nothing is).
    pub fn page(&self) -> ([f64; 2], [f64; 2]) {
        if let Some(paper) = self.paper {
            return ([0.0, 0.0], paper);
        }
        let unit = 1.0 / self.paper_per_unit();
        match self.extent() {
            Some((lo, hi)) => {
                let m = MARGIN * unit;
                ([lo[0] - m, lo[1] - m], [hi[0] + m, hi[1] + m])
            }
            None => {
                let half = 0.05 * unit;
                ([-half, -half], [half, half])
            }
        }
    }

    fn line(&mut self, pen: Pen, a: [f64; 2], b: [f64; 2]) {
        self.lines.push(Line2d { pen, a, b });
    }

    fn rectangle(&mut self, pen: Pen, lo: [f64; 2], hi: [f64; 2]) {
        let corners = [lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]];
        for k in 0..4 {
            self.line(pen, corners[k], corners[(k + 1) % 4]);
        }
    }

    fn text(&mut self, at: [f64; 2], height: f64, text: impl Into<String>, align: Align) {
        self.texts.push(Text2d { pen: Pen::Text, at, height, text: text.into(), align });
    }

    /// A view's lines (cut back to its crop), dimensions and texts, through `to` (a point of the
    /// view's drawing to this drawing), `unit` drawing metres per paper metre, `per_metre` metres
    /// of the view's drawing per paper metre (its scale).
    fn view(&mut self, cut: Option<&Cut>, crop: Option<([f64; 2], [f64; 2])>, marks: (&[Dimension], &[Note]), to: &dyn Fn([f64; 2]) -> [f64; 2], unit: f64, per_metre: f64) {
        if let Some(cut) = cut {
            // the hatch lines of the cut faces, at least 0.7 mm apart on paper
            for fill in &cut.fills {
                for [a, b] in fill.lines(fill.spacing() / per_metre as f32, 0.0007) {
                    let (a, b) = (a.map(f64::from), b.map(f64::from));
                    let piece = match crop {
                        Some((lo, hi)) => clip(a, b, lo, hi),
                        None => Some((a, b)),
                    };
                    if let Some((a, b)) = piece {
                        self.line(Pen::Hatch, to(a), to(b));
                    }
                }
            }
            for (pen, segments) in [(Pen::Beyond, &cut.below), (Pen::Cut, &cut.segments)] {
                for [a, b] in segments {
                    let (a, b) = ([a[0] as f64, a[1] as f64], [b[0] as f64, b[1] as f64]);
                    let piece = match crop {
                        Some((lo, hi)) => clip(a, b, lo, hi),
                        None => Some((a, b)),
                    };
                    if let Some((a, b)) = piece {
                        self.line(pen, to(a), to(b));
                    }
                }
            }
        }
        let (dimensions, notes) = marks;
        for d in dimensions {
            self.dimensions.push(Dim2d { from: to(d.from), to: to(d.to), value: d.length(), offset: DIMENSION_OFFSET * unit, text_height: TEXT * unit });
        }
        for n in notes {
            self.text(to(n.at), TEXT * unit, n.text.clone(), Align::Left);
        }
    }
}

/// `p` moved `d` along `n`.
fn add(p: [f64; 2], n: [f64; 2], d: f64) -> [f64; 2] {
    [p[0] + n[0] * d, p[1] + n[1] * d]
}

/// The part of segment `a`–`b` inside the rectangle `lo`–`hi` (Liang–Barsky), as a sheet crops.
pub fn clip(a: [f64; 2], b: [f64; 2], lo: [f64; 2], hi: [f64; 2]) -> Option<([f64; 2], [f64; 2])> {
    let d = [b[0] - a[0], b[1] - a[1]];
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [(-d[0], a[0] - lo[0]), (d[0], hi[0] - a[0]), (-d[1], a[1] - lo[1]), (d[1], hi[1] - a[1])] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
        }
    }
    (t0 <= t1).then(|| ([a[0] + t0 * d[0], a[1] + t0 * d[1]], [a[0] + t1 * d[0], a[1] + t1 * d[1]]))
}

/// The dimensions and texts of a view.
fn marks<'a>(doc: &'a Document, path: &str) -> (&'a [Dimension], &'a [Note], Option<([f64; 2], [f64; 2])>) {
    let (dimensions, crop) = doc.view_marks(path).unwrap_or((&[], None));
    (dimensions, doc.view_notes(path).unwrap_or(&[]), crop)
}

/// The drawing of a floor plan, section or elevation (`path`) from its `cut`, with its dimensions
/// and texts, at the scale of the first sheet it is placed on (else 1:100), cut back to its crop.
pub fn view_drawing(doc: &Document, path: &str, cut: &Cut) -> Option<Drawing2d> {
    let name = doc.view_name(path)?.to_string();
    let scale = doc.sheets().iter().flat_map(|s| &s.views).find(|v| v.plan == path).map_or(SHEET_SCALE, |v| v.scale);
    let (dimensions, notes, crop) = marks(doc, path);
    let mut d = Drawing2d { name, space: Space::Model { scale }, paper: None, lines: Vec::new(), texts: Vec::new(), dimensions: Vec::new() };
    d.view(Some(cut), crop, (dimensions, notes), &|q| q, scale, scale);
    Some(d)
}

/// The drawing of a sheet as its tab draws it: the paper's edge, its border and title, and each
/// placed view's lines (`cut` gives a view's cut by its path), dimensions, texts, the marks of the
/// sections on the floor plans, its name and scale.
pub fn sheet_drawing(doc: &Document, path: &str, cut: &dyn Fn(&str) -> Option<Arc<Cut>>) -> Option<Drawing2d> {
    let sheet = doc.sheet(path)?;
    let [w, h] = sheet.paper;
    let mut d = Drawing2d { name: sheet.title.clone(), space: Space::Paper, paper: Some(sheet.paper), lines: Vec::new(), texts: Vec::new(), dimensions: Vec::new() };
    d.rectangle(Pen::Frame, [0.0, 0.0], [w, h]);
    d.rectangle(Pen::Frame, [0.02, 0.01], [w - 0.01, h - 0.01]);
    let title = 0.004;
    // at the lower right, just above the border, as the sheet's tab shows it
    d.text([w - 0.013, 0.013], title, format!("{}  {}", sheet.number, sheet.name), Align::Right);
    for v in &sheet.views {
        let c = cut(&v.plan);
        let (dimensions, notes, crop) = marks(doc, &v.plan);
        let extent = c.as_ref().and_then(|c| c.extent);
        let middle = extent.map_or([0.0, 0.0], |(lo, hi)| [(lo[0] + hi[0]) as f64 / 2.0, (lo[1] + hi[1]) as f64 / 2.0]);
        let to = |q: [f64; 2]| [v.at[0] + (q[0] - middle[0]) / v.scale, v.at[1] + (q[1] - middle[1]) / v.scale];
        let (lo, hi) = match (crop, extent) {
            (Some((lo, hi)), _) => (to(lo), to(hi)),
            (None, Some((lo, hi))) => {
                let half = [(hi[0] - lo[0]) as f64 / 2.0 / v.scale + FRAME, (hi[1] - lo[1]) as f64 / 2.0 / v.scale + FRAME];
                ([v.at[0] - half[0], v.at[1] - half[1]], [v.at[0] + half[0], v.at[1] + half[1]])
            }
            (None, None) => ([v.at[0] - 0.02, v.at[1] - 0.015], [v.at[0] + 0.02, v.at[1] + 0.015]),
        };
        let drawn = c.as_ref().is_some_and(|c| !c.segments.is_empty() || !c.below.is_empty());
        d.view(c.as_deref().filter(|_| drawn), crop, (dimensions, notes), &to, 1.0, v.scale);
        if !drawn || crop.is_some() {
            d.rectangle(Pen::Frame, lo, hi);
        }
        if doc.plan(&v.plan).is_some() {
            for s in doc.sections() {
                let letter = s.name.rsplit(' ').next().unwrap_or(&s.name).to_string();
                section_mark(&mut d, to(s.from), to(s.to), letter);
            }
        }
        let name = doc.view_name(&v.plan).unwrap_or_default().to_string();
        d.text([lo[0], lo[1] - 0.009], 0.003, name, Align::Left);
        d.text([lo[0], lo[1] - 0.0135], 0.0022, format!("1:{}", v.scale.round()), Align::Left);
    }
    Some(d)
}

/// A section's mark on a floor plan (paper): its line, an arrow at each end the way it looks and
/// its letter past them.
fn section_mark(d: &mut Drawing2d, from: [f64; 2], to: [f64; 2], letter: String) {
    let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
    let run = dx.hypot(dy);
    if run <= 0.0 {
        return;
    }
    let n = [-dy / run, dx / run];
    d.line(Pen::Frame, from, to);
    for end in [from, to] {
        d.line(Pen::Frame, end, add(end, n, ARROW));
        let at = add(end, n, ARROW * 1.8);
        d.text([at[0], at[1] - TEXT / 2.0], TEXT, letter.clone(), Align::Center);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::A3_LANDSCAPE;

    /// The walls of a room of 8 × 5 m (its lower left at 0, 0) as a plan's cut, and one line seen
    /// below it.
    fn room() -> Cut {
        let segments = vec![
            [[0.0, 0.0], [8.0, 0.0]],
            [[8.0, 0.0], [8.0, 5.0]],
            [[8.0, 5.0], [0.0, 5.0]],
            [[0.0, 5.0], [0.0, 0.0]],
        ];
        Cut { segments, below: vec![[[1.0, 1.0], [7.0, 1.0]]], extent: Some(([0.0, 0.0], [8.0, 5.0])), fills: Vec::new() }
    }

    /// A document with a floor plan of the ground floor, an 8 m dimension and a text on it.
    fn plan() -> (Document, String) {
        let mut doc = Document::default();
        doc.add_level("Begane grond", 0.0).unwrap();
        let level = doc.levels()[0].path.clone();
        let plan = doc.add_plan(&level).unwrap();
        assert!(doc.add_dimension(&plan, [0.0, 0.0], [8.0, 0.0]));
        doc.add_note(&plan, [1.0, 1.0], "Kamer ë");
        (doc, plan)
    }

    fn count(d: &Drawing2d, pen: Pen) -> usize {
        d.lines.iter().filter(|l| l.pen == pen).count()
    }

    #[test]
    fn a_floor_plan_gives_cut_and_beyond_lines_its_dimension_and_note() {
        let (doc, plan) = plan();
        let d = view_drawing(&doc, &plan, &room()).unwrap();
        assert_eq!(d.name, "Begane grond");
        assert_eq!(d.space, Space::Model { scale: 100.0 });
        assert_eq!((count(&d, Pen::Cut), count(&d, Pen::Beyond)), (4, 1));
        assert_eq!(d.dimensions.len(), 1);
        assert_eq!(d.dimensions[0].label().text, "8000");
        // 6 mm and 2.5 mm on paper at 1:100
        assert!((d.dimensions[0].offset - 0.6).abs() < 1e-9 && (d.dimensions[0].text_height - 0.25).abs() < 1e-9);
        assert_eq!(d.texts.len(), 1);
        assert_eq!((d.texts[0].text.as_str(), d.texts[0].at, d.texts[0].align), ("Kamer ë", [1.0, 1.0], Align::Left));
        assert!((d.texts[0].height - 0.25).abs() < 1e-9);
        assert!((d.paper_per_unit() - 0.01).abs() < 1e-12);
    }

    #[test]
    fn a_dimension_has_its_line_to_the_left_with_slashes_and_its_text_beside_it() {
        let dim = Dim2d { from: [0.0, 0.0], to: [8.0, 0.0], value: 8.0, offset: 0.6, text_height: 0.25 };
        let lines = dim.lines();
        // two extension lines, the dimension line, two slashes
        assert_eq!(lines.len(), 5);
        assert!(lines.contains(&[[0.0, 0.6], [8.0, 0.6]]), "{lines:?}");
        let label = dim.label();
        assert_eq!((label.text.as_str(), label.align), ("8000", Align::Center));
        assert!((label.at[0] - 4.0).abs() < 1e-9 && label.at[1] > 0.6, "{label:?}");
    }

    #[test]
    fn a_crop_leaves_out_what_is_outside_it() {
        let (mut doc, plan) = plan();
        assert!(doc.set_crop(&plan, Some(([0.0, 0.0], [4.0, 5.0]))));
        let d = view_drawing(&doc, &plan, &room()).unwrap();
        // the right wall is outside; the lower and upper walls are cut back to x = 4
        assert_eq!(count(&d, Pen::Cut), 3);
        assert!(d.lines.iter().all(|l| l.a[0] <= 4.0 + 1e-9 && l.b[0] <= 4.0 + 1e-9), "{:?}", d.lines);
    }

    #[test]
    fn a_sheet_places_a_view_at_its_scale_around_its_middle() {
        let (mut doc, plan) = plan();
        let sheet = doc.add_sheet("Plattegronden");
        doc.place_plan(&sheet, &plan).unwrap();
        assert!(doc.move_placed(&sheet, 0, [0.2, 0.15]));
        let cut = Arc::new(room());
        let d = sheet_drawing(&doc, &sheet, &|p| (p == plan).then(|| cut.clone())).unwrap();
        assert_eq!(d.space, Space::Paper);
        // the room's middle (4, 2.5) stands at (0.2, 0.15); the 8 m wall is 8 cm long
        let wall = d.lines.iter().find(|l| l.pen == Pen::Cut && (l.a[1] - l.b[1]).abs() < 1e-12 && l.a[1] < 0.13).unwrap();
        let (lo, hi) = (wall.a[0].min(wall.b[0]), wall.a[0].max(wall.b[0]));
        assert!((lo - 0.16).abs() < 1e-9 && (hi - 0.24).abs() < 1e-9 && (wall.a[1] - 0.125).abs() < 1e-9, "{wall:?}");
        // the dimension measures the model, not the paper
        assert_eq!(d.dimensions[0].label().text, "8000");
        assert!(d.texts.iter().any(|t| t.text == "Kamer ë"));
        assert!(d.texts.iter().any(|t| t.text == "1:100"));
    }

    #[test]
    fn a_sheet_has_its_frame_and_title() {
        let mut doc = Document::default();
        let sheet = doc.add_sheet("Plattegronden");
        let d = sheet_drawing(&doc, &sheet, &|_| None).unwrap();
        assert_eq!(d.paper, Some(A3_LANDSCAPE));
        let near = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).abs() < 1e-12 && (p[1] - q[1]).abs() < 1e-12;
        assert!(d.lines.iter().any(|l| l.pen == Pen::Frame && near(l.a, [0.02, 0.01]) && near(l.b, [0.41, 0.01])), "{:?}", d.lines);
        let title = d.texts.iter().find(|t| t.text == "A01  Plattegronden").unwrap();
        assert_eq!(title.align, Align::Right);
        // at the lower right, just above the border, as the sheet's tab shows it
        assert!(title.at[0] > 0.40 && title.at[1] > 0.01 && title.at[1] < 0.02, "{title:?}");
        assert_eq!(d.page(), ([0.0, 0.0], A3_LANDSCAPE));
    }

    #[test]
    fn an_empty_cut_gives_a_drawing_without_lines() {
        let mut doc = Document::default();
        doc.add_level("Begane grond", 0.0).unwrap();
        let level = doc.levels()[0].path.clone();
        let plan = doc.add_plan(&level).unwrap();
        let d = view_drawing(&doc, &plan, &Cut::default()).unwrap();
        assert!(d.lines.is_empty() && d.texts.is_empty() && d.dimensions.is_empty());
        assert_eq!(d.extent(), None);
        // a page of 10 x 10 cm on paper: 10 x 10 m at 1:100
        let (lo, hi) = d.page();
        assert!((hi[0] - lo[0] - 10.0).abs() < 1e-9 && (hi[1] - lo[1] - 10.0).abs() < 1e-9);
    }

    #[test]
    fn a_sheets_edges_frames_and_section_marks_are_on_the_frame_pen() {
        let (mut doc, plan) = plan();
        doc.add_section("Section A", [0.0, 2.0], [8.0, 2.0], 10.0).unwrap();
        let sheet = doc.add_sheet("Plattegronden");
        doc.place_plan(&sheet, &plan).unwrap();
        // the plan never cut: an empty frame
        let d = sheet_drawing(&doc, &sheet, &|_| None).unwrap();
        assert!(d.lines.iter().all(|l| l.pen == Pen::Frame), "{:?}", d.lines);
        // cut: only what lies beyond is thin
        let cut = Arc::new(room());
        let d = sheet_drawing(&doc, &sheet, &|p| (p == plan).then(|| cut.clone())).unwrap();
        assert_eq!(count(&d, Pen::Beyond), 1, "{:?}", d.lines);
    }

    #[test]
    fn pens_have_their_layer_and_weight() {
        assert_eq!(Pen::ALL.map(Pen::name), ["O3V-Cut", "O3V-Beyond", "O3V-Hatch", "O3V-Dimensions", "O3V-Text", "O3V-Frame", "O3V-Grids"]);
        assert_eq!(Pen::ALL.map(Pen::weight_mm), [0.50, 0.18, 0.13, 0.18, 0.25, 0.35, 0.18]);
    }

    #[test]
    fn grids_and_levels_are_drawn_with_bubbles_names_and_heights() {
        let (doc, plan) = plan();
        let mut d = view_drawing(&doc, &plan, &room()).unwrap();
        d.datums(&[
            Datum2 { name: "A".into(), a: [0.0, -1.0], b: [0.0, 6.0], level: None },
            Datum2 { name: "01".into(), a: [-1.0, 3.0], b: [9.0, 3.0], level: Some(3.0) },
        ]);
        let grid_lines = d.lines.iter().filter(|l| l.pen == Pen::Grids).count();
        // the two lines, and a round bubble at both ends of the grid
        assert!(grid_lines >= 2 + 2 * 16, "{grid_lines}");
        assert_eq!(d.texts.iter().filter(|t| t.text == "A" && t.align == Align::Center).count(), 2);
        // the bubbles stand past the grid's ends: the one below under y = -1
        assert!(d.texts.iter().any(|t| t.text == "A" && t.at[1] < -1.0), "{:?}", d.texts);
        // the level's name and height past both ends
        let level: Vec<&Text2d> = d.texts.iter().filter(|t| t.text == "01 +3.000").collect();
        assert_eq!(level.len(), 2, "{:?}", d.texts);
        assert!(level.iter().any(|t| t.at[0] > 9.0 && t.align == Align::Left) && level.iter().any(|t| t.at[0] < -1.0 && t.align == Align::Right));
    }
}
