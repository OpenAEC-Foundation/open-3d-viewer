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
    /// What a view draws itself beside the model: detail lines, detail components, the shapes
    /// and leaders of its tags.
    Detail,
}

impl Pen {
    pub const ALL: [Pen; 8] = [Pen::Cut, Pen::Beyond, Pen::Hatch, Pen::Dimension, Pen::Text, Pen::Frame, Pen::Grids, Pen::Detail];

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
            Pen::Detail => "O3V-Detail",
        }
    }

    /// The line weight on paper (millimetres).
    pub fn weight_mm(self) -> f64 {
        match self {
            Pen::Cut => 0.50,
            Pen::Beyond | Pen::Dimension | Pen::Grids => 0.18,
            Pen::Hatch => 0.13,
            Pen::Text | Pen::Detail => 0.25,
            Pen::Frame => 0.35,
        }
    }
}

/// How a view is drawn on paper, as its kind of drawing in the project's drawing standard says
/// ([`Rules::of`]). Without a kind it is drawn as drawings were before there was a standard.
#[derive(Debug, Clone, PartialEq)]
pub struct Rules {
    /// The width of what is cut (millimetres on paper).
    pub cut: f64,
    /// The width of what is seen beyond the cut (millimetres on paper).
    pub beyond: f64,
    /// What is hidden: dash, gap, dash, gap, ... (millimetres on paper); empty: a continuous
    /// line.
    pub hidden: Vec<f64>,
    /// The height of its texts and dimensions on paper (metres).
    pub text: f64,
    /// Whether what is cut is hatched by its material.
    pub hatched: bool,
}

impl Default for Rules {
    fn default() -> Self {
        Rules { cut: Pen::Cut.weight_mm(), beyond: Pen::Beyond.weight_mm(), hidden: vec![2.0, 1.0], text: TEXT, hatched: true }
    }
}

impl Rules {
    /// The rules of kind of drawing `kind` of `standard`: its pens' widths, the dashes of its
    /// hidden line style at the width of what is seen beyond (a line style gives them as
    /// multiples of the line's width), its text style's height and whether it hatches. A pen or
    /// a style the standard no longer has leaves that rule as it is by default.
    pub fn of(standard: &bim_library::drawing::Standard, kind: &bim_library::drawing::DrawingKind) -> Self {
        let plain = Rules::default();
        let beyond = standard.pen(&kind.beyond).map_or(plain.beyond, |pen| pen.width);
        Rules {
            cut: standard.pen(&kind.cut).map_or(plain.cut, |pen| pen.width),
            beyond,
            hidden: standard.line_style(&kind.hidden).map_or(plain.hidden, |style| style.dashes.iter().map(|d| d * beyond).collect()),
            text: standard.text_style(&kind.text).map_or(plain.text, |style| style.height / 1000.0),
            hatched: kind.hatched,
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
    /// Its width on paper (millimetres): its pen's, unless its view's kind of drawing gives
    /// what is cut or what is seen beyond a width of its own.
    pub width: f64,
    pub color: Option<[u8; 3]>,
}

impl Line2d {
    /// A line in `pen`, as wide as that pen draws.
    pub fn new(pen: Pen, a: [f64; 2], b: [f64; 2]) -> Self {
        Line2d { pen, a, b, width: pen.weight_mm(), color: None }
    }
}

/// Which point of a text [`Text2d::at`] is: its lower left, the middle of its bottom or its lower
/// right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// A text standing on `at`, turned `angle` radians counter-clockwise about that point; `height`
/// is that of its capitals.
#[derive(Debug, Clone, PartialEq)]
pub struct Text2d {
    pub color: Option<[u8; 3]>,
    pub pen: Pen,
    pub at: [f64; 2],
    pub height: f64,
    pub text: String,
    pub align: Align,
    pub angle: f64,
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
    /// End marks and witness line spacing in paper metres.
    pub style: crate::document::DimensionStyle,
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
        let paper = self.offset / DIMENSION_OFFSET;
        let tick = self.style.tick_size * paper;
        let gap = self.style.gap * paper;
        let extension = self.style.extension * paper;
        let mut lines = vec![[add(self.from, n, gap), add(a, n, extension)], [add(self.to, n, gap), add(b, n, extension)], [a, b]];
        use crate::document::Tick;
        for (q, inward) in [(a, 1.0), (b, -1.0)] {
            match self.style.tick {
                Tick::Slash => {
                    let diagonal = [u[0] + n[0], u[1] + n[1]];
                    lines.push([add(q, diagonal, -tick), add(q, diagonal, tick)]);
                }
                Tick::Dot => {} // Filled round caps are emitted by strokes at the tick diameter.
                Tick::Arrow => {
                    let back = add(q, u, inward * tick);
                    let (l, r) = (add(back, n, tick * 0.27), add(back, n, -tick * 0.27));
                    lines.extend([[q, l], [l, r], [r, q]]);
                }
                Tick::None => {}
            }
        }
        lines
    }

    /// Its text: the value in whole millimetres, level and centred beside the line, clear of it
    /// whichever way the line runs.
    pub fn label(&self) -> Text2d {
        let text = self.style.figure(self.value);
        let n = self.axes().map_or([0.0, 1.0], |(_, n)| n);
        let h = self.text_height;
        let w = text.chars().count() as f64 * h * CHARACTER;
        let clear = n[0].abs() * w / 2.0 + n[1].abs() * h / 2.0 + self.offset * 0.25;
        let middle = [(self.from[0] + self.to[0]) / 2.0, (self.from[1] + self.to[1]) / 2.0];
        let centre = add(middle, n, self.offset + clear);
        Text2d { color: None, pen: Pen::Dimension, at: [centre[0], centre[1] - h / 2.0], height: h, text, align: Align::Center, angle: 0.0 }
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

/// A solid area in drawing coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct Polygon2d {
    pub points: Vec<[f64; 2]>,
    pub color: [u8; 3],
}

/// A drawing to export.
#[derive(Debug, Clone, PartialEq)]
pub struct Drawing2d {
    pub polygons: Vec<Polygon2d>,
    pub images: Vec<crate::raster::PlacedImage>,
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
            let u = [dx / run, dy / run];
            // A chain line uses the same paper pattern at every view scale.
            let mut at = 0.0;
            for (i, mm) in [8.0, 1.5, 0.35, 1.5].into_iter().cycle().enumerate() {
                let end = (at + mm * unit / 1000.0).min(run);
                if i % 2 == 0 {
                    self.line(Pen::Grids, add(d.a, u, at), add(d.a, u, end));
                }
                at = end;
                if at >= run { break; }
            }
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
                        self.texts.push(Text2d { color: None, pen: Pen::Grids, at: [c[0], c[1] - h / 2.0], height: h, text: d.name.clone(), align: Align::Center, angle: 0.0 });
                    }
                }
                Some(z) => {
                    let label = format!("{} {z:+.3}", d.name);
                    let gap = h * 0.6;
                    let (right, left) = (add(d.b, u, gap), add(d.a, [-u[0], -u[1]], gap));
                    self.texts.push(Text2d { color: None, pen: Pen::Grids, at: [right[0], right[1] - h / 2.0], height: h, text: label.clone(), align: Align::Left, angle: 0.0 });
                    self.texts.push(Text2d { color: None, pen: Pen::Grids, at: [left[0], left[1] - h / 2.0], height: h, text: label, align: Align::Right, angle: 0.0 });
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
        for polygon in &self.polygons { polygon.points.iter().copied().for_each(&mut grow); }
        for image in &self.images { image.clip.into_iter().for_each(&mut grow); }
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
        self.lines.push(Line2d::new(pen, a, b));
    }

    /// A line in `pen`, `width` millimetres wide on paper.
    fn line_wide(&mut self, pen: Pen, a: [f64; 2], b: [f64; 2], width: f64) {
        self.lines.push(Line2d { pen, a, b, width, color: None });
    }

    /// The widths its lines of `pen` have (millimetres on paper), the narrowest first: a
    /// writer draws the lines of a pen a width at a time.
    pub fn widths(&self, pen: Pen) -> Vec<f64> {
        let mut widths: Vec<f64> = self.lines.iter().filter(|l| l.pen == pen).map(|l| l.width).collect();
        widths.sort_by(f64::total_cmp);
        widths.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        widths
    }

    fn rectangle(&mut self, pen: Pen, lo: [f64; 2], hi: [f64; 2]) {
        let corners = [lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]];
        for k in 0..4 {
            self.line(pen, corners[k], corners[(k + 1) % 4]);
        }
    }

    fn text(&mut self, at: [f64; 2], height: f64, text: impl Into<String>, align: Align) {
        self.turned_text(at, height, text, align, 0.0);
    }

    fn turned_text(&mut self, at: [f64; 2], height: f64, text: impl Into<String>, align: Align, angle: f64) {
        self.texts.push(Text2d { color: None, pen: Pen::Text, at, height, text: text.into(), align, angle });
    }

    /// The lines a view draws itself (they are cut to its crop where they are read), through `to`.
    fn detail(&mut self, lines: &[[[f64; 2]; 2]], to: &dyn Fn([f64; 2]) -> [f64; 2]) {
        for [a, b] in lines {
            self.line(Pen::Detail, to(*a), to(*b));
        }
    }

    fn view_details(&mut self, doc: &Document, view: &str, to: &dyn Fn([f64; 2]) -> [f64; 2]) {
        for mut polygon in view_polygons(doc, view) {
            polygon.points.iter_mut().for_each(|q| *q = to(*q));
            self.polygons.push(polygon);
        }
        let start = self.lines.len();
        self.detail(doc.view_lines(view), to);
        for (range, rgb, width) in doc.detail_pens(view) {
            for l in &mut self.lines[start+range.start..start+range.end] {
                l.color = Some(rgb);
                l.width = width;
            }
        }
        self.detail(doc.view_symbols(view), to);
    }

    /// Coloured strokes for a writer that emits one path per width and colour.
    pub fn strokes(&self, pen: Pen) -> Vec<(f64, [u8; 3], Vec<[[f64; 2]; 2]>)> {
        let mut groups = Vec::new();
        for width in self.widths(pen) {
            let mut colors = std::collections::BTreeMap::<_, Vec<_>>::new();
            for l in self.lines.iter().filter(|l| l.pen == pen && (l.width - width).abs() < 1e-9) {
                colors.entry(l.color.unwrap_or_default()).or_default().push([l.a, l.b]);
            }
            groups.extend(colors.into_iter().map(|(color, lines)| (width, color, lines)));
        }
        if pen == Pen::Dimension && !self.dimensions.is_empty() {
            groups.push((pen.weight_mm(), [0; 3], self.dimensions.iter().flat_map(Dim2d::lines).collect()));
            for dim in self.dimensions.iter().filter(|d| d.style.tick == crate::document::Tick::Dot) {
                if let Some((a, b)) = dim.line() {
                    groups.push((dim.style.tick_size * 1000.0, [0; 3], vec![[a, a], [b, b]]));
                }
            }
        }
        groups
    }

    /// A view's lines (cut back to its crop), dimensions and texts, through `to` (a point of the
    /// view's drawing to this drawing), `unit` drawing metres per paper metre, `per_metre` metres
    /// of the view's drawing per paper metre (its scale), drawn as `rules` say.
    #[allow(clippy::too_many_arguments)]
    fn view(&mut self, cut: Option<&Cut>, crop: Option<([f64; 2], [f64; 2])>, marks: (&[Dimension], &[Note]), doc: &Document, to: &dyn Fn([f64; 2]) -> [f64; 2], unit: f64, per_metre: f64, rules: &Rules) {
        if let Some(cut) = cut {
            // the hatch lines of the cut faces, at least 0.7 mm apart on paper, where the view's
            // kind of drawing hatches what is cut
            for fill in cut.fills.iter().filter(|_| rules.hatched) {
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
            // what a structural plan sees through the floor, in the dashes and gaps of its line
            // style on paper, run by run, so they run on round a curve of many short pieces
            let pattern: Vec<f64> = rules.hidden.iter().map(|mm| mm / 1000.0 * per_metre).collect();
            let period: f64 = pattern.iter().sum();
            for segments in [&cut.hidden, &cut.faint_hidden] {
                for run in crate::drawing::chained(segments) {
                    // how far into the pattern the run is
                    let mut phase = 0.0f64;
                    for pair in run.windows(2) {
                        let (a, b) = ([pair[0][0] as f64, pair[0][1] as f64], [pair[1][0] as f64, pair[1][1] as f64]);
                        let piece = match crop {
                            Some((lo, hi)) => clip(a, b, lo, hi),
                            None => Some((a, b)),
                        };
                        let Some((a, b)) = piece else { continue };
                        // (a continuous line style: the piece whole)
                        if period <= 0.0 {
                            self.line_wide(Pen::Beyond, to(a), to(b), rules.beyond);
                            continue;
                        }
                        let length = (b[0] - a[0]).hypot(b[1] - a[1]);
                        let at = |d: f64| [a[0] + (b[0] - a[0]) * d / length, a[1] + (b[1] - a[1]) * d / length];
                        let mut d = 0.0;
                        while d < length {
                            // the part of the pattern `phase` is in (a dash when it is even), and
                            // how far that part still runs; at its very end the next one begins
                            let (mut part, mut from) = (0, 0.0);
                            while part + 1 < pattern.len() && phase >= from + pattern[part] - period * 1e-6 {
                                from += pattern[part];
                                part += 1;
                            }
                            let step = (from + pattern[part] - phase).max(period * 1e-6);
                            let end = (d + step).min(length);
                            if part % 2 == 0 {
                                self.line_wide(Pen::Beyond, to(at(d)), to(at(end)), rules.beyond);
                            }
                            phase = (phase + (end - d)) % period;
                            d = end;
                        }
                    }
                }
            }
            let inked = cut.inked.iter().map(|i| (if i.cut { Pen::Cut } else { Pen::Beyond }, &i.lines));
            for (pen, segments) in [(Pen::Beyond, &cut.faint_below), (Pen::Beyond, &cut.faint_segments), (Pen::Beyond, &cut.below), (Pen::Cut, &cut.segments)].into_iter().chain(inked) {
                for [a, b] in segments {
                    let (a, b) = ([a[0] as f64, a[1] as f64], [b[0] as f64, b[1] as f64]);
                    let piece = match crop {
                        Some((lo, hi)) => clip(a, b, lo, hi),
                        None => Some((a, b)),
                    };
                    if let Some((a, b)) = piece {
                        // what is cut and what is seen beyond it as wide as the view's kind draws them
                        self.line_wide(pen, to(a), to(b), if pen == Pen::Cut { rules.cut } else { rules.beyond });
                    }
                }
            }
        }
        let (dimensions, notes) = marks;
        for d in dimensions {
            self.dimensions.push(Dim2d { from: to(d.from), to: to(d.to), value: d.length(), offset: DIMENSION_OFFSET * unit, style: doc.dimension_style(d.style).clone(), text_height: rules.text * unit });
        }
        for n in notes {
            let align = match n.align {
                crate::document::Align::Left => Align::Left,
                crate::document::Align::Center => Align::Center,
                crate::document::Align::Right => Align::Right,
            };
            let (mut at, h) = (to(n.at), rules.text * unit * 0.9);
            if n.level {
                // its triangle, point down on the height, a short line at that height, the text beside it
                let (l, r) = ([at[0] - h * 0.5, at[1] + h], [at[0] + h * 0.5, at[1] + h]);
                for (a, b) in [(at, l), (l, r), (r, at), ([at[0] - h, at[1]], [at[0] + h * 3.5, at[1]])] {
                    self.lines.push(Line2d::new(Pen::Text, a, b));
                }
                at = [at[0] + h * 0.8, at[1] + h * 0.1];
            }
            self.turned_text(at, rules.text * unit, n.text.clone(), align, n.angle);
        }
    }
}

/// `p` moved `d` along `n`.
fn add(p: [f64; 2], n: [f64; 2], d: f64) -> [f64; 2] {
    [p[0] + n[0] * d, p[1] + n[1] * d]
}

/// Clip each filled triangle to the view bounds. Triangulating first preserves holes.
pub(crate) fn view_polygons(doc: &Document, path: &str) -> Vec<Polygon2d> {
    let crop = doc.view_marks(path).and_then(|(_, crop)| crop);
    let mut out = Vec::new();
    for fill in doc.view_fills(path) {
        for triangle in crate::caps::triangulate(&fill.loops) {
            let mut points = triangle.to_vec();
            if let Some((lo, hi)) = crop {
                for (axis, edge, sign) in [(0, lo[0], 1.0), (1, lo[1], 1.0), (0, hi[0], -1.0), (1, hi[1], -1.0)] {
                    let Some(&mut_last) = points.last() else { break };
                    let mut prev = mut_last;
                    let input = std::mem::take(&mut points);
                    for p in input {
                        let inside = (p[axis] - edge) * sign >= 0.0;
                        if inside != ((prev[axis] - edge) * sign >= 0.0) {
                            let t = (edge - prev[axis]) / (p[axis] - prev[axis]);
                            points.push([prev[0] + t * (p[0] - prev[0]), prev[1] + t * (p[1] - prev[1])]);
                        }
                        if inside { points.push(p); }
                        prev = p;
                    }
                }
            }
            if points.len() >= 3 { out.push(Polygon2d { points, color: fill.color }); }
        }
    }
    out
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

/// Visible section references of a plan, clipped to its view window before placement.
pub(crate) fn plan_sections(doc: &Document, path: &str) -> Vec<(String, [f64; 2], [f64; 2])> {
    let Some(plan) = doc.plan(path) else { return Vec::new() };
    if plan.presentation["section_marks"].as_bool() == Some(false) { return Vec::new(); }
    doc.sections().iter().filter_map(|s| {
        let (a,b) = match plan.crop {
            Some((lo, hi)) => clip(s.from, s.to, lo, hi)?,
            None => (s.from, s.to),
        };
        (a != b).then(|| (s.mark(), a,b))
    }).collect()
}

/// The dimensions and texts of a view.
fn marks<'a>(doc: &'a Document, path: &str) -> (&'a [Dimension], Vec<Note>, Option<([f64; 2], [f64; 2])>) {
    let (dimensions, crop) = doc.view_marks(path).unwrap_or((&[], None));
    // its own texts, then what the model says on it by itself (the names of its spaces)
    (dimensions, doc.view_notes(path).unwrap_or(&[]).iter().chain(doc.view_tags(path)).cloned().collect(), crop)
}

/// The drawing of a floor plan, section or elevation (`path`) from its `cut`, with its dimensions
/// and texts, at the scale of the first sheet it is placed on (else its own), cut back to its
/// crop and drawn as `rules` say.
pub fn view_drawing(doc: &Document, path: &str, cut: &Cut, rules: &Rules) -> Option<Drawing2d> {
    let name = doc.view_name(path)?.to_string();
    let scale = doc.sheets().iter().flat_map(|s| &s.views).find(|v| v.plan == path).map_or_else(|| doc.view_scale(path).unwrap_or(SHEET_SCALE), |v| v.scale);
    let (dimensions, notes, crop) = marks(doc, path);
    let mut d = Drawing2d { polygons: Vec::new(), images: Vec::new(), name, space: Space::Model { scale }, paper: None, lines: Vec::new(), texts: Vec::new(), dimensions: Vec::new() };
    if let Some(photo) = doc.plan(path).and_then(|p| p.orthophoto.as_ref()) { d.images.extend(photo.image.placed(crop, |q| q)); }
    d.view(Some(cut), crop, (dimensions, &notes), doc, &|q| q, scale, scale, rules);
    d.view_details(doc, path, &|q| q);
    Some(d)
}

/// The drawing of a sheet as its tab draws it: the paper's edge, its border and title, and each
/// placed view's lines (`cut` gives a view's cut by its path), dimensions, texts, the marks of the
/// sections on the floor plans, its name and scale. `rules` gives how a view is drawn, by its
/// path.
pub fn sheet_drawing(doc: &Document, path: &str, cut: &dyn Fn(&str) -> Option<Arc<Cut>>, datums: &dyn Fn(&str) -> Vec<Datum2>, rules: &dyn Fn(&str) -> Rules) -> Option<Drawing2d> {
    sheet_drawing_with_style(doc, &bim_library::house::HouseStyle::starter(), path, cut, datums, rules)
}

pub fn sheet_drawing_with_style(doc: &Document, style: &bim_library::house::HouseStyle, path: &str, cut: &dyn Fn(&str) -> Option<Arc<Cut>>, datums: &dyn Fn(&str) -> Vec<Datum2>, rules: &dyn Fn(&str) -> Rules) -> Option<Drawing2d> {
    let sheet = doc.sheet(path)?;
    let [w, h] = sheet.paper;
    let mut d = Drawing2d { polygons: Vec::new(), images: Vec::new(), name: sheet.title.clone(), space: Space::Paper, paper: Some(sheet.paper), lines: Vec::new(), texts: Vec::new(), dimensions: Vec::new() };
    d.rectangle(Pen::Frame, [0.0, 0.0], [w, h]);
    d.rectangle(Pen::Frame, [0.02, 0.01], [w - 0.01, h - 0.01]);
    let block = crate::title_block::title_block(doc, sheet, style);
    d.images.extend(block.logo);
    d.polygons.extend(block.polygons);
    d.lines.extend(block.lines);
    d.texts.extend(block.texts);
    for v in &sheet.views {
        let c = cut(&v.plan);
        let (dimensions, notes, crop) = marks(doc, &v.plan);
        let extent = c.as_ref().and_then(|c| c.extent);
        let middle = doc.view_origin(&v.plan).or_else(|| extent.map(|(lo, hi)| [(lo[0] + hi[0]) as f64 / 2.0, (lo[1] + hi[1]) as f64 / 2.0])).unwrap_or_default();
        let to = |q: [f64; 2]| [v.at[0] + (q[0] - middle[0]) / v.scale, v.at[1] + (q[1] - middle[1]) / v.scale];
        if let Some(photo) = doc.plan(&v.plan).and_then(|p| p.orthophoto.as_ref()) { d.images.extend(photo.image.placed(crop, &to)); }
        let (lo, hi) = match (crop, extent) {
            (Some((lo, hi)), _) => (to(lo), to(hi)),
            (None, Some((lo, hi))) => {
                (to(lo.map(f64::from)).map(|x| x - FRAME), to(hi.map(f64::from)).map(|x| x + FRAME))
            }
            (None, None) => ([v.at[0] - 0.02, v.at[1] - 0.015], [v.at[0] + 0.02, v.at[1] + 0.015]),
        };
        let drawn = c.as_ref().is_some_and(|c| crate::drawing::drawn(c).next().is_some());
        d.view(c.as_deref().filter(|_| drawn), crop, (dimensions, &notes), doc, &to, 1.0, v.scale, &rules(&v.plan));
        // the lines drawn on the view that are no model, and those the model draws by itself
        d.view_details(doc, &v.plan, &to);
        // its grid lines and levels, as its own tab shows them
        let placed: Vec<Datum2> = datums(&v.plan).into_iter().map(|m| Datum2 { a: to(m.a), b: to(m.b), ..m }).collect();
        d.datums(&placed);
        if !drawn || crop.is_some() {
            d.rectangle(Pen::Frame, lo, hi);
        }
        for (label, a, b) in plan_sections(doc, &v.plan) {
            section_mark(&mut d, to(a), to(b), label);
        }
        let name = doc.view_name(&v.plan).unwrap_or_default().to_string();
        d.text([lo[0], lo[1] - 0.009], 0.003, name, Align::Left);
        d.text([lo[0], lo[1] - 0.0135], 0.0022, format!("1:{}", v.scale), Align::Left);
    }
    Some(d)
}

/// The drawing of a sheet read from a file (a Revit sheet), on its paper in metres from the
/// paper's lower left corner: its title block (lines and texts), and each viewport that shows a
/// view of the document with that view's lines, its own detail lines, dimensions and texts, where
/// the sheet has it, turned as it lies. Legends, drafting views, schedules and images are left
/// out. `cut` gives a view's cut by its path.
pub fn model_sheet_drawing(doc: &Document, sheet: &crate::model::Sheet, cut: &dyn Fn(&str) -> Option<Arc<Cut>>) -> Drawing2d {
    let covers = sheet.paper.or_else(|| sheet.extent()).unwrap_or(crate::model::SheetRect { min: [0.0, 0.0], max: [0.42, 0.297] });
    let zero = [f64::from(covers.min[0]), f64::from(covers.min[1])];
    let size = [f64::from(covers.max[0]) - zero[0], f64::from(covers.max[1]) - zero[1]];
    let on_paper = |p: [f32; 2]| [f64::from(p[0]) - zero[0], f64::from(p[1]) - zero[1]];
    let mut d = Drawing2d { polygons: Vec::new(), images: Vec::new(), name: sheet.title.clone(), space: Space::Paper, paper: Some(size), lines: Vec::new(), texts: Vec::new(), dimensions: Vec::new() };
    d.rectangle(Pen::Frame, [0.0, 0.0], size);
    for line in &sheet.lines {
        d.line(Pen::Frame, on_paper(line[0]), on_paper(line[1]));
    }
    for t in sheet.texts.iter().filter(|t| !t.text.trim().is_empty()) {
        let align = match t.align {
            1 => Align::Center,
            2 => Align::Right,
            _ => Align::Left,
        };
        // each of its lines, the first at the top of its box
        let leading = f64::from(t.size) * 1.3;
        for (k, row) in t.text.lines().enumerate() {
            let at = on_paper(t.at);
            d.texts.push(Text2d { color: None, pen: Pen::Text, at: [at[0], at[1] - k as f64 * leading], height: f64::from(t.size) * 0.7, text: row.to_string(), align, angle: 0.0 });
        }
    }
    for v in &sheet.viewports {
        let Some(path) = v.drawing.as_deref() else { continue };
        let c = cut(path);
        let (dimensions, notes, crop) = marks(doc, path);
        let extent = c.as_ref().and_then(|c| c.extent).map(|(lo, hi)| (lo.map(f64::from), hi.map(f64::from)));
        // the point of the view's drawing that the sheet holds, and where: as the sheet's tab
        let anchor = match (v.anchor, crop, extent) {
            (Some(anchor), ..) => Some((anchor, v.origin)),
            (None, Some((lo, _)), _) => Some((lo, v.rect.min)),
            (None, None, Some((lo, hi))) => Some(([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0], v.rect.center())),
            (None, None, None) => None,
        };
        let Some((from, at)) = anchor else { continue };
        let (ratio, [cx, cy]) = (f64::from(v.ratio), v.turn);
        let at = on_paper(at);
        let to = |q: [f64; 2]| {
            let (dx, dy) = (q[0] - from[0], q[1] - from[1]);
            [at[0] + (dx * cx + dy * cy) * ratio, at[1] + (dy * cx - dx * cy) * ratio]
        };
        let per_metre = if ratio > 0.0 { 1.0 / ratio } else { SHEET_SCALE };
        d.view(c.as_deref(), crop, (dimensions, &notes), doc, &to, 1.0, per_metre, &Rules::default());
        d.view_details(doc, path, &to);
        // its title, where the sheet has it
        if let Some(label) = v.label {
            let title = v.written.clone().unwrap_or_else(|| format!("{}  {}", v.title, v.scale));
            if !title.trim().is_empty() {
                d.text(on_paper(label), 0.0025, title, Align::Left);
            }
        }
    }
    d
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

    #[test]
    fn a_sheet_of_a_file_is_drawn_from_its_papers_corner() {
        use crate::model::{DraftText, Sheet, SheetRect};
        let mut sheet = Sheet::new("A-01".into(), "Plans".into(), Some(SheetRect { min: [-0.5, 0.25], max: [-0.08, 0.547] }), Vec::new());
        sheet.lines.push([[-0.5, 0.25], [-0.4, 0.25]]);
        sheet.texts.push(DraftText { element: 1, at: [-0.4, 0.3], width: 0.1, height: 0.01, size: 0.004, align: 0, angle: 0.0, text: "Project\nNumber".into(), face: Default::default() });
        let d = model_sheet_drawing(&Document::default(), &sheet, &|_| None);
        let paper = d.paper.expect("a paper");
        assert!((paper[0] - 0.42).abs() < 1e-6 && (paper[1] - 0.297).abs() < 1e-6, "{paper:?}");
        // the title block's line starts at the paper's corner, and its text is written line by line
        assert!(d.lines.iter().any(|l| l.pen == Pen::Frame && l.a[0].abs() < 1e-6 && l.a[1].abs() < 1e-6 && (l.b[0] - 0.1).abs() < 1e-6));
        let texts: Vec<&str> = d.texts.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, ["Project", "Number"]);
        assert!(d.texts[1].at[1] < d.texts[0].at[1]);
    }
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
        Cut { segments, below: vec![[[1.0, 1.0], [7.0, 1.0]]], extent: Some(([0.0, 0.0], [8.0, 5.0])), fills: Vec::new(), ..Cut::default() }
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
        let d = view_drawing(&doc, &plan, &room(), &Rules::default()).unwrap();
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
        let dim = Dim2d { from: [0.0, 0.0], to: [8.0, 0.0], value: 8.0, offset: 0.6, text_height: 0.25, style: crate::document::DimensionStyle::standard() };
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
        let d = view_drawing(&doc, &plan, &room(), &Rules::default()).unwrap();
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
        let d = sheet_drawing(&doc, &sheet, &|p| (p == plan).then(|| cut.clone()), &|_| Vec::new(), &|_| Rules::default()).unwrap();
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
        let d = sheet_drawing(&doc, &sheet, &|_| None, &|_| Vec::new(), &|_| Rules::default()).unwrap();
        assert_eq!(d.paper, Some(A3_LANDSCAPE));
        let near = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).abs() < 1e-12 && (p[1] - q[1]).abs() < 1e-12;
        assert!(d.lines.iter().any(|l| l.pen == Pen::Frame && near(l.a, [0.02, 0.01]) && near(l.b, [0.41, 0.01])), "{:?}", d.lines);
        for value in ["A01", "Plattegronden", "A3", "OpenAEC"] {
            assert!(d.texts.iter().any(|t| t.text == value), "{value}");
        }
        assert_eq!(d.page(), ([0.0, 0.0], A3_LANDSCAPE));
    }

    #[test]
    fn an_empty_cut_gives_a_drawing_without_lines() {
        let mut doc = Document::default();
        doc.add_level("Begane grond", 0.0).unwrap();
        let level = doc.levels()[0].path.clone();
        let plan = doc.add_plan(&level).unwrap();
        let d = view_drawing(&doc, &plan, &Cut::default(), &Rules::default()).unwrap();
        assert!(d.lines.is_empty() && d.texts.is_empty() && d.dimensions.is_empty());
        assert_eq!(d.extent(), None);
        // a page of 10 x 10 cm on paper: 10 x 10 m at 1:100
        let (lo, hi) = d.page();
        assert!((hi[0] - lo[0] - 10.0).abs() < 1e-9 && (hi[1] - lo[1] - 10.0).abs() < 1e-9);
    }

    #[test]
    fn a_crop_trims_crossing_section_references_and_omits_outside_references() {
        let (mut doc, path) = plan();
        doc.set_crop(&path, Some(([0.,0.],[8.,5.])));
        doc.add_section("Section A", [2.,-200.], [2.,300.], 10.).unwrap();
        doc.add_section("Section B", [-9.,-200.], [-9.,300.], 10.).unwrap();
        let marks = plan_sections(&doc, &path);
        assert_eq!(marks.len(), 1);
        let (label,a,b) = &marks[0];
        assert_eq!(label,"A");
        assert!((a[1]).abs() < 1e-9 && (b[1]-5.).abs() < 1e-9);
    }

    #[test]
    fn a_sheets_edges_frames_and_section_marks_are_on_the_frame_pen() {
        let (mut doc, plan) = plan();
        doc.add_section("Section A", [0.0, 2.0], [8.0, 2.0], 10.0).unwrap();
        let sheet = doc.add_sheet("Plattegronden");
        doc.place_plan(&sheet, &plan).unwrap();
        let d = sheet_drawing(&doc, &sheet, &|_| None, &|_| Vec::new(), &|_| Rules::default()).unwrap();
        // a level of the view goes with it: its line and its name at both ends, at the view's place
        let with = sheet_drawing(&doc, &sheet, &|_| None, &|_| vec![Datum2 { name: "01".into(), a: [-1.0, 3.0], b: [1.0, 3.0], level: Some(3.0) }], &|_| Rules::default()).unwrap();
        assert_eq!(with.lines.len(), d.lines.len() + 1);
        assert_eq!(with.texts.iter().filter(|t| t.text == "01 +3.000").count(), 2);
        assert!(d.lines.iter().all(|l| l.pen == Pen::Frame), "{:?}", d.lines);
        // cut: only what lies beyond is thin
        let cut = Arc::new(room());
        let d = sheet_drawing(&doc, &sheet, &|p| (p == plan).then(|| cut.clone()), &|_| Vec::new(), &|_| Rules::default()).unwrap();
        assert_eq!(count(&d, Pen::Beyond), 1, "{:?}", d.lines);
    }

    #[test]
    fn pens_have_their_layer_and_weight() {
        assert_eq!(Pen::ALL.map(Pen::name), ["O3V-Cut", "O3V-Beyond", "O3V-Hatch", "O3V-Dimensions", "O3V-Text", "O3V-Frame", "O3V-Grids", "O3V-Detail"]);
        assert_eq!(Pen::ALL.map(Pen::weight_mm), [0.50, 0.18, 0.13, 0.18, 0.25, 0.35, 0.18, 0.25]);
    }

    #[test]
    fn grids_and_levels_are_drawn_with_bubbles_names_and_heights() {
        let (doc, plan) = plan();
        let mut d = view_drawing(&doc, &plan, &room(), &Rules::default()).unwrap();
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
