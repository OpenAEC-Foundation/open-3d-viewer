//! Drawings derived from the model for sheets: the lines where a floor plan's cut meets the
//! drawn triangles, and the edges seen below the cut, in plan (x, y), metres in the model's
//! frame. Headless, like the document.

use crate::model::Model;
use std::collections::HashMap;

/// A line on a drawing: from, to (x, y).
pub type Segment = [[f32; 2]; 2];

/// The cut of a floor plan: the line segments where the triangles of the shown layers cross the
/// plane at height `z`, and their extent.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cut {
    pub segments: Vec<Segment>,
    /// The edges seen beyond the cut, between it and the plan's view depth (below it, or above
    /// it in a ceiling plan): outlines and creases of the triangles there, drawn thin.
    pub below: Vec<Segment>,
    /// The edges beyond the cut that something nearer covers, where the drawing shows them
    /// dashed (a structural plan, which looks through the floor): else none.
    pub hidden: Vec<Segment>,
    /// Lower left and upper right of the segments; `None` when nothing is cut.
    pub extent: Option<([f32; 2], [f32; 2])>,
    /// The cut faces: what each element shows where the plane goes through it.
    pub fills: Vec<Fill>,
    /// The cut lines and the edges beyond the cut of the elements the view draws faint (existing
    /// work in a view of a later phase, in halftone); they are not in `segments` and `below`.
    pub faint_segments: Vec<Segment>,
    pub faint_below: Vec<Segment>,
    /// The covered edges (see `hidden`) of the elements it draws faint: dashed in halftone.
    pub faint_hidden: Vec<Segment>,
}

/// A cut face: the outlines one element (or a layer outside its elements) shows where the plane
/// goes through it, and how it is hatched once [`crate::hatching::hatch`] has looked up what it
/// is made of.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Fill {
    /// The layer (index in `Model::layers`) and the element cut; no element for what a layer
    /// draws outside its elements.
    pub layer: usize,
    pub element: Option<u64>,
    /// Closed outlines; a point is in the face when an odd number of them goes round it.
    pub loops: Vec<Vec<[f32; 2]>>,
    /// The colour behind the hatch and that of its lines; `None` before it is hatched.
    pub look: Option<([u8; 3], [u8; 3])>,
    /// The hatch lines, each with its row: every n-th row draws a coarser hatch.
    pub rows: Vec<(i32, Segment)>,
    /// How far apart the rows are (metres of the drawing); 0 without lines.
    pub apart: f32,
    /// The view draws its element faint (in halftone).
    pub faint: bool,
}

/// Two faces meeting at a sharper angle than this (degrees) make a visible edge.
const CREASE_DEGREES: f32 = 20.0;

/// What of the model a drawing takes: the layers that are on (all of them where the list ends)
/// and, on those, the triangles that are not hidden (by their layer and first vertex: the
/// elements hidden in the view).
#[derive(Clone, Copy)]
pub struct Shown<'a> {
    pub layers: &'a [bool],
    pub hidden: &'a dyn Fn(usize, u32) -> bool,
    /// The triangles drawn faint (in halftone), by their layer and first vertex; `None`: none.
    pub faint: Option<&'a dyn Fn(usize, u32) -> bool>,
    /// The pieces joined elements make (an element to its piece): the edges on the seam between
    /// two elements of one piece are left out where the faces round it lie in one plane. `None`:
    /// no elements are joined.
    pub pieces: Option<&'a HashMap<u64, u32>>,
}

/// Hides no triangle.
fn none_hidden(_: usize, _: u32) -> bool {
    false
}

impl<'a> Shown<'a> {
    /// The layers that are on, with no element hidden.
    pub fn layers(layers: &'a [bool]) -> Self {
        Shown { layers, hidden: &none_hidden, faint: None, pieces: None }
    }

    /// Every layer, with no element hidden.
    pub fn all() -> Self {
        Self::layers(&[])
    }

    /// Whether layer `i` is on.
    fn on(&self, i: usize) -> bool {
        self.layers.get(i).copied().unwrap_or(true)
    }
}

/// A floor plan's drawing: the cut at `high` and the edges seen from above between `low` and
/// `high`, of what is `shown`, left out where something nearer the cut covers them.
pub fn plan_at(model: &Model, shown: Shown, low: f32, high: f32) -> Cut {
    let mut cut = cut_at(model, shown, high);
    cut.below = seen_between(model, shown, high - low, move |p| [p[0], p[1], high - p[2]]);
    part(&mut cut, model, shown, move |p| [p[0], p[1], high - p[2]], high - low, None);
    cut.extent = extent(cut.segments.iter().chain(&cut.below).chain(&cut.faint_segments).chain(&cut.faint_below));
    cut
}

/// A structural plan's drawing: a floor plan's, with the edges something nearer the cut covers
/// (the piles and beams under a floor) kept apart, to draw dashed.
pub fn structural_at(model: &Model, shown: Shown, low: f32, high: f32) -> Cut {
    let mut cut = cut_at(model, shown, high);
    let map = move |p: [f32; 3]| [p[0], p[1], high - p[2]];
    let depth = high - low;
    let faces: Vec<[[f32; 3]; 3]> = triangles(model, shown)
        .map(|t| t.map(map))
        .filter(|c| c[0][2].max(c[1][2]).max(c[2][2]) > 0.0 && c[0][2].min(c[1][2]).min(c[2][2]) < depth)
        .collect();
    let (seen, covered) = sorted(&faces, &feature_edges(model, shown, map, 0.0, depth));
    cut.below = seen;
    // a covered edge under a line already drawn (a seen one, or another covered one: the top and
    // the foot of a pile) is left out
    let mut drawn = Lines::default();
    for s in &cut.below {
        drawn.add(*s);
    }
    for s in covered {
        if !drawn.has(&s) {
            drawn.add(s);
            cut.hidden.push(s);
        }
    }
    part(&mut cut, model, shown, map, depth, None);
    cut.extent = extent(cut.segments.iter().chain(&cut.below).chain(&cut.faint_segments).chain(&cut.faint_below));
    cut
}

/// Sets apart what the view draws faint: the lines of `cut` (in the frame `map` gives, its third
/// coordinate the depth from the cut, down to `depth`) that lie on an edge or a cut of a faint
/// element and on none of another, and the cut faces of the faint elements. A faint element's
/// covered edges are not drawn dashed.
fn part(cut: &mut Cut, model: &Model, shown: Shown, map: impl Fn([f32; 3]) -> [f32; 3] + Copy, depth: f32, edges: Option<[Vec<[[f32; 3]; 2]>; 2]>) {
    let Some(faint) = shown.faint else { return };
    // the edges of the faint elements alone and of the others alone: as given, else found here
    let [of_faint, of_plain] = edges.unwrap_or_else(|| {
        let [_, of_faint, of_plain] = feature_edges_apart(model, shown, map, 0.0, depth, true);
        [of_faint, of_plain]
    });
    let filed = |edges: Vec<[[f32; 3]; 2]>| {
        let mut lines = Lines::default();
        edges.into_iter().filter_map(flat).for_each(|e| lines.add(e));
        lines
    };
    let (faint_edges, plain_edges) = (filed(of_faint), filed(of_plain));
    // where the cut goes through the faint elements, and through the others
    let (mut faint_cuts, mut plain_cuts) = (Lines::default(), Lines::default());
    for (i, layer) in model.layers.iter().enumerate().filter(|(i, l)| !l.points && shown.on(*i)) {
        let p = &layer.positions;
        let corner = |k: u32| p.get(k as usize * 3..k as usize * 3 + 3).map(|v| [v[0], v[1], v[2]]);
        for t in layer.indices.chunks_exact(3).filter(|t| !(shown.hidden)(i, t[0])) {
            let (Some(a), Some(b), Some(c)) = (corner(t[0]), corner(t[1]), corner(t[2])) else { continue };
            if let Some(s) = cross([a, b, c].map(map), 0.0) {
                if faint(i, t[0]) { &mut faint_cuts } else { &mut plain_cuts }.add(s);
            }
        }
    }
    let apart = |all: &mut Vec<Segment>, on: &Lines, off: &Lines| {
        let (faint, plain): (Vec<Segment>, Vec<Segment>) = std::mem::take(all).into_iter().partition(|s| on.has(s) && !off.has(s));
        *all = plain;
        faint
    };
    cut.faint_segments = apart(&mut cut.segments, &faint_cuts, &plain_cuts);
    cut.faint_below = apart(&mut cut.below, &faint_edges, &plain_edges);
    cut.faint_hidden = apart(&mut cut.hidden, &faint_edges, &plain_edges);
    for fill in &mut cut.fills {
        // by the first vertex of the element
        let first = fill.element.and_then(|id| {
            let layer = model.layers.get(fill.layer)?;
            let &(_, first, _) = layer.elements.iter().find(|e| e.0 == id)?;
            layer.indices.get(first as usize).copied()
        });
        fill.faint = first.is_some_and(|v| faint(fill.layer, v));
    }
}

/// Segments joined end to end into runs, so a dashed line runs on round a curve drawn as many
/// short pieces (a pile's outline) instead of starting a dash on every piece. Ends nearer than a
/// tenth of a millimetre meet; a run that branches goes on along one branch, the rest start runs
/// of their own.
pub fn chained(segments: &[Segment]) -> Vec<Vec<[f32; 2]>> {
    let key = |p: [f32; 2]| ((p[0] * 1e4).round() as i64, (p[1] * 1e4).round() as i64);
    let mut at: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, s) in segments.iter().enumerate() {
        at.entry(key(s[0])).or_default().push(i);
        at.entry(key(s[1])).or_default().push(i);
    }
    let mut used = vec![false; segments.len()];
    // the next unused segment at `p`, and its other end
    let next = |p: [f32; 2], used: &mut Vec<bool>| {
        let i = at.get(&key(p))?.iter().copied().find(|&i| !used[i])?;
        used[i] = true;
        let s = segments[i];
        Some(if key(s[0]) == key(p) { s[1] } else { s[0] })
    };
    let mut runs = Vec::new();
    for i in 0..segments.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        let mut run = vec![segments[i][0], segments[i][1]];
        // on from its end, then back from its start
        while let Some(p) = next(*run.last().unwrap(), &mut used) {
            run.push(p);
        }
        let mut back = Vec::new();
        while let Some(p) = next(*back.last().unwrap_or(&run[0]), &mut used) {
            back.push(p);
        }
        back.reverse();
        back.extend(run);
        runs.push(back);
    }
    runs
}

/// Lines by the square metre they pass through, to find the ones a line lies on.
#[derive(Default)]
struct Lines {
    cells: HashMap<(i32, i32), Vec<Segment>>,
    /// The ones too long to file under every cell they pass.
    long: Vec<Segment>,
}

impl Lines {
    /// A line lies on another when both its ends are this near it (metres).
    const NEAR: f32 = 0.01;

    fn add(&mut self, s: Segment) {
        let cell = |v: f32| v.floor() as i32;
        let (x0, x1) = (cell(s[0][0].min(s[1][0]) - Self::NEAR), cell(s[0][0].max(s[1][0]) + Self::NEAR));
        let (y0, y1) = (cell(s[0][1].min(s[1][1]) - Self::NEAR), cell(s[0][1].max(s[1][1]) + Self::NEAR));
        if (x1 - x0 + 1) as i64 * (y1 - y0 + 1) as i64 > 64 {
            self.long.push(s);
            return;
        }
        for x in x0..=x1 {
            for y in y0..=y1 {
                self.cells.entry((x, y)).or_default().push(s);
            }
        }
    }

    /// Whether `s` lies on a line added before.
    fn has(&self, s: &Segment) -> bool {
        let near = |p: [f32; 2], [a, b]: &Segment| {
            let (d, v) = ([b[0] - a[0], b[1] - a[1]], [p[0] - a[0], p[1] - a[1]]);
            let t = ((v[0] * d[0] + v[1] * d[1]) / (d[0] * d[0] + d[1] * d[1]).max(1e-12)).clamp(0.0, 1.0);
            (v[0] - t * d[0]).hypot(v[1] - t * d[1]) <= Self::NEAR
        };
        let middle = [(s[0][0] + s[1][0]) / 2.0, (s[0][1] + s[1][1]) / 2.0];
        let here = self.cells.get(&(middle[0].floor() as i32, middle[1].floor() as i32));
        here.into_iter().flatten().chain(&self.long).any(|other| near(s[0], other) && near(s[1], other))
    }
}

/// A reflected ceiling plan's drawing: the cut at `low` and the edges seen from below between
/// `low` and `high`, of what is `shown`, left out where something nearer the cut covers them,
/// laid out as a floor plan (x, y as seen from above).
pub fn ceiling_at(model: &Model, shown: Shown, low: f32, high: f32) -> Cut {
    let mut cut = cut_at(model, shown, low);
    cut.below = seen_between(model, shown, high - low, move |p| [p[0], p[1], p[2] - low]);
    part(&mut cut, model, shown, move |p| [p[0], p[1], p[2] - low], high - low, None);
    cut.extent = extent(cut.segments.iter().chain(&cut.below).chain(&cut.faint_segments).chain(&cut.faint_below));
    cut
}

/// The outlines and creases of what is shown, in the frame `map` gives (its third coordinate the
/// depth from the cut), between the cut and `depth`: the parts no triangle nearer the cut covers.
fn seen_between(model: &Model, shown: Shown, depth: f32, map: impl Fn([f32; 3]) -> [f32; 3] + Copy) -> Vec<Segment> {
    let faces: Vec<[[f32; 3]; 3]> = triangles(model, shown)
        .map(|t| t.map(map))
        .filter(|c| c[0][2].max(c[1][2]).max(c[2][2]) > 0.0 && c[0][2].min(c[1][2]).min(c[2][2]) < depth)
        .collect();
    visible(&faces, &feature_edges(model, shown, map, 0.0, depth))
}

/// The outlines and creases of the triangles between `low` and `high`, clipped to that range and
/// seen from above; an edge seen end on (a vertical one) gives nothing.
pub fn edges_between(model: &Model, shown: Shown, low: f32, high: f32) -> Vec<Segment> {
    feature_edges(model, shown, |p| p, low, high).into_iter().filter_map(flat).collect()
}

/// An edge seen along its third coordinate: nothing when it is seen end on.
fn flat([a, b]: [[f32; 3]; 2]) -> Option<Segment> {
    ((b[0] - a[0]).hypot(b[1] - a[1]) > 1e-4).then_some([[a[0], a[1]], [b[0], b[1]]])
}

/// The corners of each triangle that is shown.
fn triangles<'a>(model: &'a Model, shown: Shown<'a>) -> impl Iterator<Item = [[f32; 3]; 3]> + 'a {
    model.layers.iter().enumerate().filter(move |(i, l)| !l.points && shown.on(*i)).flat_map(move |(i, layer)| {
        let p = &layer.positions;
        layer.indices.chunks_exact(3).filter(move |t| !(shown.hidden)(i, t[0])).filter_map(move |t| {
            let corner = |k: u32| p.get(k as usize * 3..k as usize * 3 + 3).map(|v| [v[0], v[1], v[2]]);
            Some([corner(t[0])?, corner(t[1])?, corner(t[2])?])
        })
    })
}

/// The outlines and creases of the triangles shown in the frame `map` gives (its third
/// coordinate the depth), clipped to `low` ≤ depth ≤ `high`.
fn feature_edges(model: &Model, shown: Shown, map: impl Fn([f32; 3]) -> [f32; 3], low: f32, high: f32) -> Vec<[[f32; 3]; 2]> {
    let [all, ..] = feature_edges_apart(model, shown, map, low, high, false);
    all
}

/// A hasher for the keys of edges (whole numbers): mixing them is enough, and several times as
/// fast as the standard one on the millions of edges of a large model.
#[derive(Default)]
struct Mix(u64);

impl std::hash::Hasher for Mix {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.write_u64(u64::from_le_bytes(word));
        }
    }

    fn write_u64(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    fn write_i64(&mut self, v: i64) {
        self.write_u64(v as u64);
    }

    fn write_usize(&mut self, v: usize) {
        self.write_u64(v as u64);
    }
}

/// [`feature_edges`] of all triangles shown and, with `apart` and where the view draws elements
/// faint, of the faint ones alone and of the others alone (else those two are empty): found in
/// one walk over the triangles, each edge keeping what it is among each of the three.
fn feature_edges_apart(model: &Model, shown: Shown, map: impl Fn([f32; 3]) -> [f32; 3], low: f32, high: f32, apart: bool) -> [Vec<[[f32; 3]; 2]>; 3] {
    // edges by their ends rounded to 0.1 mm, so faces that do not share vertices still meet
    let key = |p: [f32; 3]| p.map(|v| (v * 1e4).round() as i64);
    let crease = CREASE_DEGREES.to_radians().cos();
    let faint = shown.faint.filter(|_| apart);
    // per edge its ends and, among all faces, the faint ones and the others: the normal of its
    // first face, how many faces, a crease seen
    type Among = ([f32; 3], u32, bool);
    let mut edges: HashMap<([i64; 3], [i64; 3]), ([f32; 3], [f32; 3], [Among; 3]), std::hash::BuildHasherDefault<Mix>> = HashMap::default();
    for (i, layer) in model.layers.iter().enumerate().filter(|(i, l)| !l.points && shown.on(*i)) {
        let p = &layer.positions;
        let corner = |k: u32| p.get(k as usize * 3..k as usize * 3 + 3).map(|v| [v[0], v[1], v[2]]);
        for t in layer.indices.chunks_exact(3).filter(|t| !(shown.hidden)(i, t[0])) {
            let (Some(c0), Some(c1), Some(c2)) = (corner(t[0]), corner(t[1]), corner(t[2])) else { continue };
            let c = [c0, c1, c2].map(&map);
            let (lo, hi) = (c[0][2].min(c[1][2]).min(c[2][2]), c[0][2].max(c[1][2]).max(c[2][2]));
            if hi < low || lo > high {
                continue;
            }
            let (u, v) = (sub(c[1], c[0]), sub(c[2], c[0]));
            let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            if len <= 1e-12 {
                continue;
            }
            let n = n.map(|x| x / len);
            // which of the two it is a face of besides all
            let own = faint.map(|faint| if faint(i, t[0]) { 1 } else { 2 });
            for k in 0..3 {
                let (a, b) = (c[k], c[(k + 1) % 3]);
                let (ka, kb) = (key(a), key(b));
                let id = if ka <= kb { (ka, kb) } else { (kb, ka) };
                let entry = edges.entry(id).or_insert((a, b, [([0.0; 3], 0, false); 3]));
                for among in std::iter::once(0).chain(own) {
                    let state = &mut entry.2[among];
                    state.1 += 1;
                    if state.1 == 1 {
                        state.0 = n;
                    } else if state.0[0] * n[0] + state.0[1] * n[1] + state.0[2] * n[2] < crease {
                        state.2 = true;
                    }
                }
            }
        }
    }
    let mut out: [Vec<[[f32; 3]; 2]>; 3] = Default::default();
    for (a, b, among) in edges.into_values() {
        // the part of the edge between low and high
        let (za, zb) = (a[2], b[2]);
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        if (za - zb).abs() > 1e-9 {
            let (ta, tb) = ((low - za) / (zb - za), (high - za) / (zb - za));
            t0 = t0.max(ta.min(tb));
            t1 = t1.min(ta.max(tb));
        } else if za < low || za > high {
            continue;
        }
        if t0 >= t1 {
            continue;
        }
        let at = |t: f32| [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]), a[2] + t * (b[2] - a[2])];
        for (k, (_, faces, sharp)) in among.into_iter().enumerate() {
            // an edge of one face, or of faces that meet in a crease
            if faces == 1 || faces > 1 && sharp {
                out[k].push([at(t0), at(t1)]);
            }
        }
    }
    // the seams between joined elements are no edges
    let seams = seams(model, shown, &map);
    out.map(|edges| without_seams(edges, &seams))
}

/// A straight line in space, to find the edges that lie on one line: its direction (turned one
/// way) and its point nearest the origin, both rounded to a millimetre.
type LineKey = ([i32; 3], [i32; 3]);

/// The line an edge lies on, its direction, and where along it the edge starts and ends.
fn line_of(a: [f32; 3], b: [f32; 3]) -> Option<(LineKey, [f32; 3], f32, f32)> {
    let d = sub(b, a);
    let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if length < 1e-4 {
        return None;
    }
    let mut d = d.map(|v| v / length);
    // one way round, whichever way the edge runs
    if d.iter().find(|v| v.abs() > 1e-4).is_some_and(|v| *v < 0.0) {
        d = d.map(|v| -v);
    }
    let dot = |p: [f32; 3]| p[0] * d[0] + p[1] * d[1] + p[2] * d[2];
    let (s, e) = (dot(a), dot(b));
    let foot = [a[0] - d[0] * s, a[1] - d[1] * s, a[2] - d[2] * s];
    let round = |p: [f32; 3]| p.map(|v| (v * 1e3).round() as i32);
    Some(((round(d), round(foot)), d, s.min(e), s.max(e)))
}

/// The seams between joined elements: the stretches of line where an edge of one element lies on
/// an edge of another of the same piece and, their faces against each other left aside, the
/// faces round it lie in one plane: a wall standing flush on a floor, two beams of one height
/// side by side. Per line, the stretches along it. In the frame `map` gives.
fn seams(model: &Model, shown: Shown, map: impl Fn([f32; 3]) -> [f32; 3]) -> HashMap<LineKey, Vec<(f32, f32)>> {
    let Some(pieces) = shown.pieces else { return HashMap::new() };
    let flat = CREASE_DEGREES.to_radians().cos();
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    // per line: the edges on it, each with its piece, its element, its stretch and its faces
    let mut on: HashMap<LineKey, Vec<(u32, u64, f32, f32, Vec<[f32; 3]>)>> = HashMap::new();
    let key = |p: [f32; 3]| p.map(|v| (v * 1e4).round() as i64);
    for (i, layer) in model.layers.iter().enumerate().filter(|(i, l)| !l.points && shown.on(*i)) {
        let p = &layer.positions;
        for &(id, first, count) in &layer.elements {
            let Some(&piece) = pieces.get(&id) else { continue };
            let Some(indices) = layer.indices.get(first as usize..(first + count) as usize) else { continue };
            if indices.first().is_some_and(|v| (shown.hidden)(i, *v)) {
                continue;
            }
            // the element's own edges with the normals of their faces
            let mut edges: HashMap<([i64; 3], [i64; 3]), ([f32; 3], [f32; 3], Vec<[f32; 3]>)> = HashMap::new();
            for t in indices.chunks_exact(3) {
                let corner = |k: u32| p.get(k as usize * 3..k as usize * 3 + 3).map(|v| map([v[0], v[1], v[2]]));
                let (Some(a), Some(b), Some(c)) = (corner(t[0]), corner(t[1]), corner(t[2])) else { continue };
                let (u, v) = (sub(b, a), sub(c, a));
                let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                let len = dot(n, n).sqrt();
                if len <= 1e-12 {
                    continue;
                }
                let n = n.map(|x| x / len);
                for (a, b) in [(a, b), (b, c), (c, a)] {
                    let (ka, kb) = (key(a), key(b));
                    edges.entry(if ka <= kb { (ka, kb) } else { (kb, ka) }).or_insert_with(|| (a, b, Vec::new())).2.push(n);
                }
            }
            for (a, b, normals) in edges.into_values() {
                // an edge within a flat face is none
                if normals.len() == 2 && dot(normals[0], normals[1]) >= flat {
                    continue;
                }
                if let Some((line, _, from, to)) = line_of(a, b) {
                    on.entry(line).or_default().push((piece, id, from, to, normals));
                }
            }
        }
    }
    let mut out: HashMap<LineKey, Vec<(f32, f32)>> = HashMap::new();
    for (line, edges) in on {
        for (k, (piece, id, from, to, normals)) in edges.iter().enumerate() {
            for (other_piece, other, other_from, other_to, other_normals) in &edges[k + 1..] {
                let (start, end) = (from.max(*other_from), to.min(*other_to));
                if piece != other_piece || id == other || end - start < 1e-3 {
                    continue;
                }
                // the faces that lie against each other fall away; what is left must be one plane
                let mut left: Vec<[f32; 3]> = normals.clone();
                let mut theirs: Vec<[f32; 3]> = other_normals.clone();
                left.retain(|n| match theirs.iter().position(|m| dot(*n, *m) <= -flat) {
                    Some(at) => {
                        theirs.swap_remove(at);
                        false
                    }
                    None => true,
                });
                left.extend(theirs);
                if left.len() < normals.len() + other_normals.len() && left.iter().all(|n| dot(*n, left[0]) >= flat) {
                    out.entry(line).or_default().push((start, end));
                }
            }
        }
    }
    out
}

/// The edges without their stretches along the seams.
fn without_seams(edges: Vec<[[f32; 3]; 2]>, seams: &HashMap<LineKey, Vec<(f32, f32)>>) -> Vec<[[f32; 3]; 2]> {
    if seams.is_empty() {
        return edges;
    }
    let mut out = Vec::with_capacity(edges.len());
    for [a, b] in edges {
        let Some((line, d, from, to)) = line_of(a, b) else { continue };
        let Some(gaps) = seams.get(&line) else {
            out.push([a, b]);
            continue;
        };
        let mut gaps: Vec<(f32, f32)> = gaps.iter().copied().filter(|(s, e)| *e > from && *s < to).collect();
        gaps.sort_by(|x, y| x.0.total_cmp(&y.0));
        // a point of the line by its place along it
        let s0 = a[0] * d[0] + a[1] * d[1] + a[2] * d[2];
        let at = |s: f32| {
            let t = s - s0;
            [a[0] + d[0] * t, a[1] + d[1] * t, a[2] + d[2] * t]
        };
        let mut start = from;
        for (s, e) in gaps {
            if s - start > 1e-3 {
                out.push([at(start), at(s)]);
            }
            start = start.max(e);
        }
        if to - start > 1e-3 {
            out.push([at(start), at(to)]);
        }
    }
    out
}



/// A section's or elevation's drawing, in (u along the line `from` → `to`, v up): the cut where
/// the upright plane through the line meets what is shown, and the edges seen beyond it up to
/// `depth`, left out where something nearer covers them.
pub fn section_at(model: &Model, shown: Shown, from: [f64; 2], to: [f64; 2], depth: f64) -> Cut {
    let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
    let run = dx.hypot(dy).max(1e-9);
    let (d, n) = ([dx / run, dy / run], [-dy / run, dx / run]);
    let map = move |p: [f32; 3]| {
        let (x, y) = (p[0] as f64 - from[0], p[1] as f64 - from[1]);
        [(x * d[0] + y * d[1]) as f32, p[2], (x * n[0] + y * n[1]) as f32]
    };
    let mut segments = Vec::new();
    let mut beyond = Vec::new();
    let mut faces = Faces::default();
    // where the cut meets the terrain: a surface, that closes no face of its own
    let terrain: Vec<bool> = model.layers.iter().map(|l| l.name.contains("Topograph")).collect();
    let mut ground: HashMap<(usize, Option<u64>), Vec<Segment>> = HashMap::new();
    for (key, t) in keyed(model, shown) {
        let c = t.map(map);
        if let Some(segment) = cross(c, 0.0) {
            segments.push(segment);
            faces.add(key, segment);
            if terrain.get(key.0).copied().unwrap_or(false) {
                ground.entry(key).or_default().push(segment);
            }
        }
        let (near, far) = (c[0][2].min(c[1][2]).min(c[2][2]), c[0][2].max(c[1][2]).max(c[2][2]));
        if far > 0.0 && near < depth as f32 {
            beyond.push(c);
        }
    }
    // the edges of all that is shown, and of what is drawn faint and what is not apart
    let [edges, of_faint, of_plain] = feature_edges_apart(model, shown, map, 0.0, depth as f32, true);
    let below = visible(&beyond, &edges);
    let extent = extent(segments.iter().chain(&below));
    let mut cut = Cut { segments, below, extent, fills: faces.fills(), ..Cut::default() };
    cut.fills.extend(earth(ground, &cut.fills, model.project.earth.as_ref().map(|e| e.0)));
    cut.fills.sort_by_key(|f| (f.layer, f.element));
    part(&mut cut, model, shown, map, depth as f32, Some([of_faint, of_plain]));
    cut
}

/// How far under its lowest cut point the earth of a cut terrain is drawn (metres), where the
/// file does not say down to what height.
const EARTH: f32 = 3.0;

/// The earth under the terrain a section cuts: per piece of terrain the face between its cut line
/// and its base (`base`: the height the file gives; else `EARTH` under the lowest point of that
/// line), to be hatched as what the terrain is made of. A terrain with a thickness closes a face of its own (it is among `closed`) and gets
/// none here.
fn earth(ground: HashMap<(usize, Option<u64>), Vec<Segment>>, closed: &[Fill], base: Option<f32>) -> Vec<Fill> {
    let mut fills = Vec::new();
    for ((layer, element), segments) in ground {
        if closed.iter().any(|f| f.layer == layer && f.element == element) {
            continue;
        }
        let base = base.unwrap_or_else(|| segments.iter().flatten().map(|p| p[1]).fold(f32::INFINITY, f32::min) - EARTH);
        // under each piece of the line a strip down to the base; a piece seen end on has none
        let loops: Vec<Vec<[f32; 2]>> = segments.iter().filter(|[a, b]| (a[0] - b[0]).abs() > 1e-5 && a[1].min(b[1]) > base).map(|&[a, b]| vec![a, b, [b[0], base], [a[0], base]]).collect();
        if !loops.is_empty() {
            fills.push(Fill { layer, element, loops, ..Fill::default() });
        }
    }
    fills
}

/// The axes a 3D view is drawn along, from the direction it looks in: to the right and up on
/// the drawing, the vertical upright (a view straight down has north up).
pub fn view_axes(forward: [f32; 3]) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let f = glam::Vec3::from(forward).try_normalize().unwrap_or(glam::Vec3::NEG_Z);
    let right = f.cross(glam::Vec3::Z).try_normalize().unwrap_or(glam::Vec3::X);
    (right.to_array(), right.cross(f).to_array(), f.to_array())
}

/// A 3D view's drawing in hidden lines, in (u to the right, v up) of the view, metres: the
/// outlines and creases of what is shown, looked at along `forward` from far away (no
/// perspective), left out where something nearer covers them. With a section box, only what
/// lies in it, with the outlines of where it cuts.
pub fn view_at(model: &Model, shown: Shown, forward: [f32; 3], clip: Option<&crate::section::SectionBox>) -> Cut {
    use glam::Vec3;
    let (right, up, f) = view_axes(forward);
    let (right, up, f) = (Vec3::from(right), Vec3::from(up), Vec3::from(f));
    // depths count from a plane in front of everything
    let b = &model.bounds;
    let corners = (0..8).map(|k| Vec3::new(if k & 1 == 0 { b.min[0] } else { b.max[0] }, if k & 2 == 0 { b.min[1] } else { b.max[1] }, if k & 4 == 0 { b.min[2] } else { b.max[2] }));
    let base = corners.map(|c| c.dot(f)).fold(f32::INFINITY, f32::min) - 1.0;
    let map = move |p: [f32; 3]| {
        let p = Vec3::from(p);
        [p.dot(right), p.dot(up), p.dot(f) - base]
    };
    let unmap = move |q: [f32; 3]| right * q[0] + up * q[1] + f * (q[2] + base);
    let mut faces: Vec<[[f32; 3]; 3]> = Vec::new();
    let mut edges = feature_edges(model, shown, map, f32::NEG_INFINITY, f32::INFINITY);
    match clip {
        None => faces.extend(keyed(model, shown).map(|(_, t)| t.map(map))),
        Some(clip) => {
            let (lo, hi) = (clip.min, clip.max);
            // a line's part in the box (the box's own coordinates)
            let within = |a: Vec3, z: Vec3| -> Option<(Vec3, Vec3)> {
                let (mut from, mut to) = (0.0f32, 1.0f32);
                for i in 0..3 {
                    let d = z[i] - a[i];
                    if d.abs() < 1e-9 {
                        if a[i] < lo[i] - 1e-5 || a[i] > hi[i] + 1e-5 {
                            return None;
                        }
                        continue;
                    }
                    let (s, t) = ((lo[i] - a[i]) / d, (hi[i] - a[i]) / d);
                    from = from.max(s.min(t));
                    to = to.min(s.max(t));
                }
                (to - from > 1e-6).then(|| (a + (z - a) * from, a + (z - a) * to))
            };
            let seen = |a: Vec3, z: Vec3| [map(clip.to_world(a).to_array()), map(clip.to_world(z).to_array())];
            edges = edges.into_iter().filter_map(|[a, z]| within(clip.to_local(unmap(a)), clip.to_local(unmap(z))).map(|(a, z)| seen(a, z))).collect();
            for (_, t) in keyed(model, shown) {
                let local = t.map(|p| clip.to_local(Vec3::from(p)));
                // the triangle's part in the box: cut back side by side
                let mut polygon: Vec<Vec3> = local.to_vec();
                for i in 0..3 {
                    for (limit, keep_above) in [(lo[i], true), (hi[i], false)] {
                        let inside = |p: Vec3| if keep_above { p[i] >= limit } else { p[i] <= limit };
                        let mut next = Vec::with_capacity(polygon.len() + 2);
                        for k in 0..polygon.len() {
                            let (a, z) = (polygon[k], polygon[(k + 1) % polygon.len()]);
                            if inside(a) {
                                next.push(a);
                            }
                            if inside(a) != inside(z) {
                                next.push(a + (z - a) * ((limit - a[i]) / (z[i] - a[i])));
                            }
                        }
                        polygon = next;
                    }
                }
                for k in 1..polygon.len().saturating_sub(1) {
                    faces.push([polygon[0], polygon[k], polygon[k + 1]].map(|p| map(clip.to_world(p).to_array())));
                }
                // where a side of the box goes through it: the outline of the cut
                for i in 0..3 {
                    let (j, k) = ((i + 1) % 3, (i + 2) % 3);
                    for limit in [lo[i], hi[i]] {
                        let Some([a, z]) = cross(local.map(|p| [p[j], p[k], p[i]]), limit) else { continue };
                        let lift = |q: [f32; 2]| {
                            let mut p = Vec3::ZERO;
                            (p[j], p[k], p[i]) = (q[0], q[1], limit);
                            p
                        };
                        edges.extend(within(lift(a), lift(z)).map(|(a, z)| seen(a, z)));
                    }
                }
            }
        }
    }
    let below = visible(&faces, &edges);
    let extent = extent(below.iter());
    Cut { below, extent, ..Cut::default() }
}

/// How many pixels the longer side of a section's depth image has.
const DEPTH_PIXELS: f32 = 2048.0;

/// The parts of `edges` (u, v, depth) that no triangle of `faces` covers.
fn visible(faces: &[[[f32; 3]; 3]], edges: &[[[f32; 3]; 2]]) -> Vec<Segment> {
    sorted(faces, edges).0
}

/// The parts of `edges` (u, v, depth) that no triangle of `faces` covers, and the parts one does:
/// the faces are drawn into a depth image of their part beyond depth 0, and every edge is walked
/// through it.
fn sorted(faces: &[[[f32; 3]; 3]], edges: &[[[f32; 3]; 2]]) -> (Vec<Segment>, Vec<Segment>) {
    let corners = faces.iter().flatten().chain(edges.iter().flatten());
    let (mut lo, mut hi) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
    for c in corners {
        lo = [lo[0].min(c[0]), lo[1].min(c[1])];
        hi = [hi[0].max(c[0]), hi[1].max(c[1])];
    }
    if !(lo[0] <= hi[0] && lo[1] <= hi[1]) {
        return (Vec::new(), Vec::new());
    }
    let pixel = ((hi[0] - lo[0]).max(hi[1] - lo[1]) / DEPTH_PIXELS).max(1e-4);
    let (w, h) = (((hi[0] - lo[0]) / pixel) as usize + 2, ((hi[1] - lo[1]) / pixel) as usize + 2);
    let mut depth = vec![f32::INFINITY; w * h];
    for face in faces {
        let polygon = in_front(face);
        for k in 1..polygon.len().saturating_sub(1) {
            draw_depth(&mut depth, (w, h), lo, pixel, [polygon[0], polygon[k], polygon[k + 1]]);
        }
    }
    // an edge lies on its own faces: it shows unless something is clearly nearer
    let slack = 0.02 + 4.0 * pixel;
    let at = |p: [f32; 3]| {
        let (x, y) = (((p[0] - lo[0]) / pixel) as usize, ((p[1] - lo[1]) / pixel) as usize);
        depth[y.min(h - 1) * w + x.min(w - 1)]
    };
    let (mut out, mut covered) = (Vec::new(), Vec::new());
    for &[a, b] in edges {
        let steps = (((b[0] - a[0]).hypot(b[1] - a[1]) / pixel * 1.5).ceil() as usize).max(1);
        let point = |k: usize| {
            let t = k as f32 / steps as f32;
            [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]), a[2] + t * (b[2] - a[2])]
        };
        // the run being walked: where it began, and whether it is seen
        let mut run: Option<([f32; 3], bool)> = None;
        let mut last = a;
        for k in 0..=steps {
            let p = point(k);
            let seen = p[2] <= at(p) + slack;
            match run {
                None => run = Some((p, seen)),
                Some((s, was)) if was != seen => {
                    if was { &mut out } else { &mut covered }.extend(flat([s, last]));
                    run = Some((last, seen));
                }
                _ => {}
            }
            last = p;
        }
        if let Some((s, was)) = run {
            if was { &mut out } else { &mut covered }.extend(flat([s, b]));
        }
    }
    (out, covered)
}

/// The part of a triangle at depth 0 or more (a polygon of 0, 3 or 4 corners).
fn in_front(t: &[[f32; 3]; 3]) -> Vec<[f32; 3]> {
    let mut out = Vec::with_capacity(4);
    for k in 0..3 {
        let (a, b) = (t[k], t[(k + 1) % 3]);
        if a[2] >= 0.0 {
            out.push(a);
        }
        if (a[2] >= 0.0) != (b[2] >= 0.0) {
            let s = a[2] / (a[2] - b[2]);
            out.push([a[0] + s * (b[0] - a[0]), a[1] + s * (b[1] - a[1]), 0.0]);
        }
    }
    out
}

/// Draws a triangle (u, v, depth) into the depth image, keeping the nearest depth per pixel.
fn draw_depth(depth: &mut [f32], (w, h): (usize, usize), lo: [f32; 2], pixel: f32, t: [[f32; 3]; 3]) {
    let px = t.map(|c| [(c[0] - lo[0]) / pixel, (c[1] - lo[1]) / pixel, c[2]]);
    let area = (px[1][0] - px[0][0]) * (px[2][1] - px[0][1]) - (px[2][0] - px[0][0]) * (px[1][1] - px[0][1]);
    if area.abs() < 1e-9 {
        return;
    }
    let x0 = px.iter().map(|c| c[0]).fold(f32::INFINITY, f32::min).floor().max(0.0) as usize;
    let x1 = (px.iter().map(|c| c[0]).fold(f32::NEG_INFINITY, f32::max).ceil() as usize).min(w - 1);
    let y0 = px.iter().map(|c| c[1]).fold(f32::INFINITY, f32::min).floor().max(0.0) as usize;
    let y1 = (px.iter().map(|c| c[1]).fold(f32::NEG_INFINITY, f32::max).ceil() as usize).min(h - 1);
    let edge = |a: [f32; 3], b: [f32; 3], x: f32, y: f32| (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
            let (w0, w1, w2) = (edge(px[1], px[2], cx, cy) / area, edge(px[2], px[0], cx, cy) / area, edge(px[0], px[1], cx, cy) / area);
            if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                continue;
            }
            let z = w0 * px[0][2] + w1 * px[1][2] + w2 * px[2][2];
            let cell = &mut depth[y * w + x];
            if z < *cell {
                *cell = z;
            }
        }
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Lower left and upper right of segments.
fn extent<'a>(segments: impl Iterator<Item = &'a Segment>) -> Option<([f32; 2], [f32; 2])> {
    segments.flatten().fold(None, |e: Option<([f32; 2], [f32; 2])>, q| {
        Some(match e {
            None => (*q, *q),
            Some((lo, hi)) => ([lo[0].min(q[0]), lo[1].min(q[1])], [hi[0].max(q[0]), hi[1].max(q[1])]),
        })
    })
}

/// Cuts what is `shown` at height `z`. Point layers are left out.
pub fn cut_at(model: &Model, shown: Shown, z: f32) -> Cut {
    let mut segments = Vec::new();
    let mut faces = Faces::default();
    for (key, corners) in keyed(model, shown) {
        if let Some(segment) = cross(corners, z) {
            segments.push(segment);
            faces.add(key, segment);
        }
    }
    let extent = extent(segments.iter());
    Cut { segments, extent, fills: faces.fills(), ..Cut::default() }
}

/// Lines nearer than this (metres) lie on each other: where two cut faces meet.
const MEET: f32 = 0.001;

/// The part of segment `e` that segment `f` lies on, as the stretch of `e` it covers (0 at its
/// start, 1 at its end); none when they do not lie on one line or share less than [`MEET`].
fn lies_on(e: &Segment, f: &Segment) -> Option<(f32, f32)> {
    let d = [e[1][0] - e[0][0], e[1][1] - e[0][1]];
    let length = d[0].hypot(d[1]);
    if length < MEET {
        return None;
    }
    let u = [d[0] / length, d[1] / length];
    let place = |p: [f32; 2]| {
        let v = [p[0] - e[0][0], p[1] - e[0][1]];
        ((v[0] * u[1] - v[1] * u[0]).abs() <= MEET).then_some((v[0] * u[0] + v[1] * u[1]) / length)
    };
    let (a, b) = (place(f[0])?, place(f[1])?);
    let (from, to) = (a.min(b).max(0.0), a.max(b).min(1.0));
    ((to - from) * length > MEET).then_some((from, to))
}

/// Segments by the half metre squares they pass through, to find the ones near a segment.
struct Near {
    cells: HashMap<(i32, i32), Vec<usize>>,
    /// The ones too long to file under every square they pass.
    long: Vec<usize>,
}

impl Near {
    fn squares(s: &Segment) -> ((i32, i32), (i32, i32)) {
        let cell = |v: f32| (v * 2.0).floor() as i32;
        ((cell(s[0][0].min(s[1][0]) - MEET), cell(s[0][1].min(s[1][1]) - MEET)), (cell(s[0][0].max(s[1][0]) + MEET), cell(s[0][1].max(s[1][1]) + MEET)))
    }

    fn of<'a>(segments: impl Iterator<Item = &'a Segment>) -> Self {
        let mut near = Near { cells: HashMap::new(), long: Vec::new() };
        for (i, s) in segments.enumerate() {
            let ((x0, y0), (x1, y1)) = Self::squares(s);
            if (x1 - x0 + 1) as i64 * (y1 - y0 + 1) as i64 > 256 {
                near.long.push(i);
                continue;
            }
            for x in x0..=x1 {
                for y in y0..=y1 {
                    near.cells.entry((x, y)).or_default().push(i);
                }
            }
        }
        near
    }

    /// The segments that may lie on `s`, each once.
    fn to(&self, s: &Segment) -> Vec<usize> {
        let ((x0, y0), (x1, y1)) = Self::squares(s);
        let mut found = self.long.clone();
        if (x1 - x0 + 1) as i64 * (y1 - y0 + 1) as i64 > 4096 {
            found.extend(self.cells.values().flatten());
        } else {
            for x in x0..=x1 {
                for y in y0..=y1 {
                    found.extend(self.cells.get(&(x, y)).into_iter().flatten());
                }
            }
        }
        found.sort_unstable();
        found.dedup();
        found
    }
}

/// Joins the cut faces `same` says are of one piece (two walls of one material that meet): the
/// lines of the cut where two such faces lie against each other are left out, so they read as one
/// face. The faces themselves stay apart, each with its hatch.
pub fn join(cut: &mut Cut, same: &dyn Fn(usize, usize) -> bool) {
    // the sides of the faces, each with its face
    let sides: Vec<(usize, Segment)> = cut
        .fills
        .iter()
        .enumerate()
        .flat_map(|(i, f)| f.loops.iter().flat_map(move |l| (0..l.len()).map(move |k| (i, [l[k], l[(k + 1) % l.len()]]))))
        .collect();
    let near = Near::of(sides.iter().map(|s| &s.1));
    let at = |s: &Segment, t: f32| [s[0][0] + t * (s[1][0] - s[0][0]), s[0][1] + t * (s[1][1] - s[0][1])];
    // where a side of one face lies on a side of another of the same piece
    let mut met: Vec<Segment> = Vec::new();
    for (k, (i, side)) in sides.iter().enumerate() {
        for o in near.to(side) {
            let (j, other) = &sides[o];
            if o <= k || i == j || !same(*i, *j) {
                continue;
            }
            if let Some((from, to)) = lies_on(side, other) {
                met.push([at(side, from), at(side, to)]);
            }
        }
    }
    if met.is_empty() {
        return;
    }
    let near = Near::of(met.iter());
    // the cut lines, and those of what the view draws faint
    for lines in [&mut cut.segments, &mut cut.faint_segments] {
        let mut kept = Vec::with_capacity(lines.len());
        for s in lines.iter() {
            let mut gaps: Vec<(f32, f32)> = near.to(s).into_iter().filter_map(|o| lies_on(s, &met[o])).collect();
            if gaps.is_empty() {
                kept.push(*s);
                continue;
            }
            gaps.sort_by(|a, b| a.0.total_cmp(&b.0));
            let length = (s[1][0] - s[0][0]).hypot(s[1][1] - s[0][1]);
            let mut from = 0.0f32;
            for (start, end) in gaps {
                if (start - from) * length > MEET {
                    kept.push([at(s, from), at(s, start)]);
                }
                from = from.max(end);
            }
            if (1.0 - from) * length > MEET {
                kept.push([at(s, from), at(s, 1.0)]);
            }
        }
        *lines = kept;
    }
}

/// The cut faces of what is `shown` on the plane where the third coordinate `map` gives is 0, in
/// its first two: what each element shows there, not hatched yet.
pub fn faces_at(model: &Model, shown: Shown, map: impl Fn([f32; 3]) -> [f32; 3]) -> Vec<Fill> {
    let mut faces = Faces::default();
    for (key, corners) in keyed(model, shown) {
        if let Some(segment) = cross(corners.map(&map), 0.0) {
            faces.add(key, segment);
        }
    }
    faces.fills()
}

/// The corners of each triangle that is shown, with its layer and the element it draws.
fn keyed<'a>(model: &'a Model, shown: Shown<'a>) -> impl Iterator<Item = ((usize, Option<u64>), [[f32; 3]; 3])> + 'a {
    model.layers.iter().enumerate().filter(move |(i, l)| !l.points && shown.on(*i)).flat_map(move |(i, layer)| {
        let p = &layer.positions;
        layer.indices.chunks_exact(3).enumerate().filter(move |(_, t)| !(shown.hidden)(i, t[0])).filter_map(move |(k, t)| {
            let corner = |k: u32| p.get(k as usize * 3..k as usize * 3 + 3).map(|v| [v[0], v[1], v[2]]);
            Some(((i, layer.element_at(k)), [corner(t[0])?, corner(t[1])?, corner(t[2])?]))
        })
    })
}

/// The cut segments per element, to close into its cut faces.
#[derive(Default)]
struct Faces {
    of: HashMap<(usize, Option<u64>), Vec<Segment>>,
}

/// Points nearer than this (metres) are one point of an outline.
const WELD: f32 = 5e-5;
/// An outline enclosing less than this (square metres) is left out.
const SLIVER: f32 = 1e-6;

impl Faces {
    fn add(&mut self, key: (usize, Option<u64>), segment: Segment) {
        self.of.entry(key).or_default().push(segment);
    }

    /// The faces whose segments close into outlines, in the order of their layers and elements.
    fn fills(self) -> Vec<Fill> {
        let mut fills: Vec<Fill> = self
            .of
            .into_iter()
            .map(|((layer, element), segments)| Fill { layer, element, loops: outlines(&segments), ..Fill::default() })
            .filter(|f| !f.loops.is_empty())
            .collect();
        fills.sort_by_key(|f| (f.layer, f.element));
        fills
    }
}

/// The closed outlines the segments make end to end; segments that close nothing are left out.
fn outlines(segments: &[Segment]) -> Vec<Vec<[f32; 2]>> {
    let key = |p: [f32; 2]| ((p[0] / WELD).round() as i64, (p[1] / WELD).round() as i64);
    let mut at: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, s) in segments.iter().enumerate() {
        at.entry(key(s[0])).or_default().push(i);
        at.entry(key(s[1])).or_default().push(i);
    }
    let mut used = vec![false; segments.len()];
    let mut loops = Vec::new();
    for first in 0..segments.len() {
        if used[first] {
            continue;
        }
        used[first] = true;
        let (start, mut end) = (key(segments[first][0]), segments[first][1]);
        let mut points = vec![segments[first][0]];
        let closed = loop {
            if key(end) == start {
                break true;
            }
            points.push(end);
            let Some(next) = at.get(&key(end)).and_then(|l| l.iter().copied().find(|&i| !used[i])) else { break false };
            used[next] = true;
            end = if key(segments[next][0]) == key(end) { segments[next][1] } else { segments[next][0] };
        };
        let area: f32 = (0..points.len()).map(|k| {
            let (a, b) = (points[k], points[(k + 1) % points.len()]);
            a[0] * b[1] - b[0] * a[1]
        }).sum();
        if closed && points.len() >= 3 && area.abs() / 2.0 > SLIVER {
            loops.push(points);
        }
    }
    loops
}

/// Where a triangle crosses the plane at height `z`: a corner on the plane counts as above it,
/// so a triangle lying in the plane or touching it at one corner gives nothing.
fn cross(corners: [[f32; 3]; 3], z: f32) -> Option<Segment> {
    let above = corners.map(|c| c[2] >= z);
    let ups = above.iter().filter(|a| **a).count();
    if ups == 0 || ups == 3 {
        return None;
    }
    let mut points = Vec::with_capacity(2);
    for k in 0..3 {
        let (a, b) = (corners[k], corners[(k + 1) % 3]);
        if above[k] != above[(k + 1) % 3] {
            // from the same end whichever way the edge runs, so two triangles sharing it meet
            // in the very same point
            let (a, b) = if a[2] <= b[2] { (a, b) } else { (b, a) };
            let t = (z - a[2]) / (b[2] - a[2]);
            points.push([a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])]);
        }
    }
    let [a, b] = points[..] else { return None };
    (a != b).then_some([a, b])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Layer;
    use std::path::PathBuf;

    /// A closed box from (0, 0, 0) to (4, 2, 3).
    fn box_layer(name: &str) -> Layer {
        let positions = vec![0.,0.,0., 4.,0.,0., 4.,2.,0., 0.,2.,0., 0.,0.,3., 4.,0.,3., 4.,2.,3., 0.,2.,3.];
        let indices = vec![0,2,1, 0,3,2, 4,5,6, 4,6,7, 0,1,5, 0,5,4, 1,2,6, 1,6,5, 2,3,7, 2,7,6, 3,0,4, 3,4,7];
        Layer { name: name.into(), positions, indices, ..Layer::default() }
    }

    #[test]
    fn a_box_cut_through_its_middle_is_its_outline() {
        let model = Model::new(PathBuf::from("box.obj"), "OBJ", vec![box_layer("Box")]);
        let cut = cut_at(&model, Shown::all(), 1.2);
        // the four sides, two triangles each
        assert_eq!(cut.segments.len(), 8);
        assert_eq!(cut.extent, Some(([0.0, 0.0], [4.0, 2.0])));
        let length: f32 = cut.segments.iter().map(|[a, b]| ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt()).sum();
        assert!((length - 12.0).abs() < 1e-4, "the perimeter, {length}");
    }

    /// A table 1 x 1 x 0.8 m at (5, 0), as a closed box with its own vertices per face.
    fn table() -> Layer {
        let (x0, x1, y0, y1, z0, z1) = (5.0, 6.0, 0.0, 1.0, 0.0, 0.8);
        let quads = [
            [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]],
            [[x0, y0, z0], [x0, y1, z0], [x1, y1, z0], [x1, y0, z0]],
            [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]],
            [[x1, y0, z0], [x1, y1, z0], [x1, y1, z1], [x1, y0, z1]],
            [[x1, y1, z0], [x0, y1, z0], [x0, y1, z1], [x1, y1, z1]],
            [[x0, y1, z0], [x0, y0, z0], [x0, y0, z1], [x0, y1, z1]],
        ];
        let mut positions = Vec::new();
        let mut indices = Vec::new();
        for q in quads {
            let base = (positions.len() / 3) as u32;
            positions.extend(q.iter().flatten().copied());
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        Layer { name: "Table".into(), positions, indices, ..Layer::default() }
    }

    #[test]
    fn a_plan_shows_the_edges_below_its_cut() {
        let model = Model::new(PathBuf::from("room.obj"), "OBJ", vec![box_layer("Wall"), table()]);
        let plan = plan_at(&model, Shown::all(), -0.3, 1.2);
        // the wall is cut; the table under the cut shows its outline (top and bottom edges seen
        // from above), not the diagonals of its faces, not its vertical edges
        assert_eq!(plan.segments.len(), 8);
        let table: Vec<&Segment> = plan.below.iter().filter(|[a, b]| a[0] >= 5.0 && b[0] >= 5.0).collect();
        // its top's four edges; those of its foot lie right under them
        let length: f32 = table.iter().map(|[a, b]| (b[0] - a[0]).hypot(b[1] - a[1])).sum();
        assert!((4.0 - 0.05..=8.0 + 1e-3).contains(&length), "{length}: {table:?}");
        assert_eq!(plan.extent, Some(([0.0, 0.0], [6.0, 2.0])));
        // above the table's top, the range shows nothing of it
        assert!(edges_between(&model, Shown::all(), 1.0, 1.2).iter().all(|[a, b]| a[0] < 5.0 && b[0] < 5.0));
    }

    /// A ceiling plan cuts the wall at its cut and sees what hangs above it, not what stands under
    /// it, in plan as a floor plan has it.
    #[test]
    fn a_ceiling_plan_shows_the_edges_above_its_cut() {
        let lamp = block("Lamp", [1.0, 0.5, 2.6], [1.5, 1.0, 2.9]);
        let model = Model::new(PathBuf::from("room.obj"), "OBJ", vec![box_layer("Wall"), table(), lamp]);
        let ceiling = ceiling_at(&model, Shown::all(), 2.3, 3.0);
        assert_eq!(ceiling.segments.len(), 8, "the wall cut at 2.3 m");
        assert!(ceiling.below.iter().all(|[a, b]| a[0] < 5.0 && b[0] < 5.0), "no table: it stands under the cut");
        let lamp: Vec<&Segment> = ceiling.below.iter().filter(|[a, b]| (1.0..=1.5).contains(&a[0]) && (1.0..=1.5).contains(&b[0]) && a[1] >= 0.5 && b[1] <= 1.0).collect();
        let length: f32 = lamp.iter().map(|[a, b]| (b[0] - a[0]).hypot(b[1] - a[1])).sum();
        assert!(length >= 2.0 - 1e-4, "the lamp's outline, {length}: {lamp:?}");
        assert_eq!(ceiling.extent, Some(([0.0, 0.0], [4.0, 2.0])));
    }

    /// A box from `lo` to `hi`, its own vertices per face.
    fn block(name: &str, lo: [f32; 3], hi: [f32; 3]) -> Layer {
        let ([x0, y0, z0], [x1, y1, z1]) = (lo, hi);
        let quads = [
            [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]],
            [[x0, y0, z0], [x0, y1, z0], [x1, y1, z0], [x1, y0, z0]],
            [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]],
            [[x1, y0, z0], [x1, y1, z0], [x1, y1, z1], [x1, y0, z1]],
            [[x1, y1, z0], [x0, y1, z0], [x0, y1, z1], [x1, y1, z1]],
            [[x0, y1, z0], [x0, y0, z0], [x0, y0, z1], [x0, y1, z1]],
        ];
        let mut positions = Vec::new();
        let mut indices = Vec::new();
        for q in quads {
            let base = (positions.len() / 3) as u32;
            positions.extend(q.iter().flatten().copied());
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        Layer { name: name.into(), positions, indices, ..Layer::default() }
    }

    #[test]
    fn a_plan_hides_what_lies_under_a_floor() {
        // a floor over a table, both under the cut
        let layers = vec![block("Floor", [0.0, 0.0, 2.0], [4.0, 4.0, 2.2]), block("Under", [1.0, 1.0, 0.0], [2.0, 2.0, 0.8]), block("Beside", [6.0, 0.0, 0.0], [7.0, 1.0, 0.8])];
        let model = Model::new(PathBuf::from("house.obj"), "OBJ", layers);
        let plan = plan_at(&model, Shown::all(), -0.5, 3.0);
        let inside = |[a, b]: &&Segment| [a, b].iter().all(|p| (0.9..=2.1).contains(&p[0]) && (0.9..=2.1).contains(&p[1]));
        let under: Vec<&Segment> = plan.below.iter().filter(inside).collect();
        assert!(under.is_empty(), "{under:?}");
        assert!(plan.below.iter().any(|[a, b]| a[0] >= 6.0 && b[0] >= 6.0), "the table beside the floor shows");
        // a ceiling plan under the floor sees the floor, not what stands on it
        let on = block("On", [1.0, 1.0, 2.2], [2.0, 2.0, 2.9]);
        let model = Model::new(PathBuf::from("house.obj"), "OBJ", vec![block("Floor", [0.0, 0.0, 2.0], [4.0, 4.0, 2.2]), on]);
        let ceiling = ceiling_at(&model, Shown::all(), 1.5, 3.0);
        assert!(ceiling.below.iter().filter(inside).count() == 0, "{:?}", ceiling.below);
        assert!(!ceiling.below.is_empty());
    }

    #[test]
    fn the_seam_between_joined_elements_is_not_drawn() {
        // a floor and a beam beside it, their tops level, seen from above
        let part = |name: &str, id: u64, lo: [f32; 3], hi: [f32; 3]| {
            let mut layer = block(name, lo, hi);
            layer.elements = vec![(id, 0, layer.indices.len() as u32)];
            layer
        };
        let between = |cut: &Cut| cut.below.iter().filter(|[a, b]| (a[0] - 4.0).abs() < 1e-3 && (b[0] - 4.0).abs() < 1e-3).map(|[a, b]| (b[1] - a[1]).abs()).sum::<f32>();
        let level = Model::new(PathBuf::from("house.obj"), "OBJ", vec![part("Floor", 1, [0.0, 0.0, 0.0], [4.0, 3.0, 0.3]), part("Beam", 2, [4.0, 0.0, -0.2], [4.5, 3.0, 0.3])]);
        let pieces: HashMap<u64, u32> = HashMap::from([(1, 1), (2, 1)]);
        let joined = Shown { pieces: Some(&pieces), ..Shown::all() };
        assert!(between(&plan_at(&level, Shown::all(), -1.0, 1.2)) > 2.9, "not joined: the line between them is drawn");
        let plan = plan_at(&level, joined, -1.0, 1.2);
        assert!(between(&plan) < 1e-3, "joined, their tops in one plane: {:?}", plan.below);
        assert!(plan.below.iter().any(|[a, b]| a[0] < 1e-3 && b[0] < 1e-3) && plan.below.iter().any(|[a, b]| a[0] > 4.49 && b[0] > 4.49), "their outline stays");
        // the beam higher than the floor: a real step, drawn though they are joined
        let step = Model::new(PathBuf::from("house.obj"), "OBJ", vec![part("Floor", 1, [0.0, 0.0, 0.0], [4.0, 3.0, 0.3]), part("Beam", 2, [4.0, 0.0, -0.2], [4.5, 3.0, 0.6])]);
        assert!(between(&plan_at(&step, joined, -1.0, 1.2)) > 2.9, "a step between them stays");
        // of two pieces nothing is left out
        let apart: HashMap<u64, u32> = HashMap::from([(1, 1), (2, 2)]);
        assert!(between(&plan_at(&level, Shown { pieces: Some(&apart), ..Shown::all() }, -1.0, 1.2)) > 2.9);
    }

    #[test]
    fn what_a_view_draws_faint_is_set_apart() {
        // an existing wall cut by the plan, an existing block and a new one under the cut
        let layers = vec![block("Old wall", [0.0, 0.0, 0.0], [4.0, 0.3, 3.0]), block("Old", [0.0, 2.0, 0.0], [1.0, 3.0, 0.5]), block("New", [2.0, 2.0, 0.0], [3.0, 3.0, 0.5])];
        let model = Model::new(PathBuf::from("house.obj"), "OBJ", layers);
        let old = |layer: usize, _: u32| layer < 2;
        let shown = Shown { faint: Some(&old), ..Shown::all() };
        let plan = plan_at(&model, shown, -0.5, 1.2);
        let length = |s: &[Segment]| s.iter().map(|[a, b]| (b[0] - a[0]).hypot(b[1] - a[1])).sum::<f32>();
        assert!(plan.segments.is_empty(), "the only wall cut is faint: {:?}", plan.segments);
        assert!((length(&plan.faint_segments) - 8.6).abs() < 0.05, "the wall's outline, 4 by 0.3: {}", length(&plan.faint_segments));
        assert!((3.8..=8.2).contains(&length(&plan.below)), "the new block's outline, where its foot lies under its top maybe twice: {}", length(&plan.below));
        assert!(plan.below.iter().all(|[a, b]| a[0] >= 1.9 && b[0] >= 1.9), "{:?}", plan.below);
        let old_block: Vec<Segment> = plan.faint_below.iter().copied().filter(|[a, b]| a[1] >= 1.9 && b[1] >= 1.9).collect();
        assert!((3.8..=8.2).contains(&length(&old_block)) && old_block.iter().all(|[a, b]| a[0] <= 1.1 && b[0] <= 1.1), "the old block's outline: {old_block:?}");
        assert_eq!(plan.fills.iter().map(|f| f.faint).collect::<Vec<_>>(), [false], "the wall's face: of a layer without elements, so by none");
        assert_eq!(plan.extent.map(|(lo, hi)| (lo[0], hi[0])), Some((0.0, 4.0)), "the faint lines count for its extent");
        // without faint elements nothing is set apart
        let plain = plan_at(&model, Shown::all(), -0.5, 1.2);
        assert!(plain.faint_segments.is_empty() && plain.faint_below.is_empty() && !plain.segments.is_empty());
    }

    #[test]
    fn short_pieces_join_into_runs_whatever_their_order() {
        // a square in pieces, given out of order and some the other way round, and a lone line
        let square: Vec<Segment> = vec![[[1.0, 0.0], [1.0, 1.0]], [[0.0, 0.0], [1.0, 0.0]], [[0.0, 1.0], [0.0, 0.0]], [[0.0, 1.0], [1.0, 1.0]], [[5.0, 5.0], [6.0, 5.0]]];
        let mut runs = chained(&square);
        runs.sort_by_key(|r| r.len());
        assert_eq!(runs.len(), 2, "{runs:?}");
        assert_eq!(runs[0].len(), 2, "the lone line");
        assert_eq!(runs[1].len(), 5, "the square: four pieces, five points, round: {:?}", runs[1]);
        assert_eq!(runs[1].first(), runs[1].last());
        let length: f32 = runs[1].windows(2).map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1])).sum();
        assert!((length - 4.0).abs() < 1e-6);
    }

    #[test]
    fn a_structural_plan_keeps_what_lies_under_a_floor_to_draw_dashed() {
        let layers = vec![block("Floor", [0.0, 0.0, 2.0], [4.0, 4.0, 2.2]), block("Pile", [1.0, 1.0, 0.0], [2.0, 2.0, 2.0]), block("Beside", [6.0, 0.0, 0.0], [7.0, 1.0, 0.8])];
        let model = Model::new(PathBuf::from("house.obj"), "OBJ", layers);
        let plan = structural_at(&model, Shown::all(), -0.5, 3.0);
        let inside = |[a, b]: &&Segment| [a, b].iter().all(|p| (0.9..=2.1).contains(&p[0]) && (0.9..=2.1).contains(&p[1]));
        assert_eq!(plan.below.iter().filter(inside).count(), 0, "the pile is not seen");
        let pile: f32 = plan.hidden.iter().filter(inside).map(|[a, b]| (b[0] - a[0]).hypot(b[1] - a[1])).sum();
        assert!((3.5..=4.5).contains(&pile), "its outline once, 4 m round: {pile}");
        assert!(plan.hidden.iter().all(|s| !(s[0][0] >= 6.0 && s[1][0] >= 6.0)), "what is seen is not dashed too");
        assert!(plan_at(&model, Shown::all(), -0.5, 3.0).hidden.is_empty(), "a floor plan dashes nothing");
    }

    #[test]
    fn the_terrain_a_section_cuts_has_earth_under_it() {
        // a terrain of two triangles at height 0.5, and a wall standing on it
        let terrain = Layer { name: "Topography".into(), positions: vec![0., 0., 0.5, 10., 0., 0.5, 10., 4., 0.5, 0., 4., 0.5], indices: vec![0, 1, 2, 0, 2, 3], ..Layer::default() };
        let model = Model::new(PathBuf::from("site.obj"), "OBJ", vec![terrain, block("Wall", [2.0, 1.0, 0.5], [3.0, 3.0, 3.0])]);
        let section = section_at(&model, Shown::all(), [0.0, 2.0], [10.0, 2.0], 20.0);
        let earth: Vec<&Fill> = section.fills.iter().filter(|f| f.layer == 0).collect();
        assert_eq!(earth.len(), 1, "the terrain is a surface: its cut closes no face, the earth under it is one");
        let points: Vec<[f32; 2]> = earth[0].loops.iter().flatten().copied().collect();
        let (low, high) = points.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| (lo.min(p[1]), hi.max(p[1])));
        assert!((high - 0.5).abs() < 1e-4 && (low - (0.5 - EARTH)).abs() < 1e-4, "from the ground line down: {low} to {high}");
        let (left, right) = points.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| (lo.min(p[0]), hi.max(p[0])));
        assert!(left < 0.01 && right > 9.99, "under all of the cut line: {left} to {right}");
        // the wall keeps its own face; an elevation in front of the terrain cuts nothing of it
        assert!(section.fills.iter().any(|f| f.layer == 1));
        assert!(section_at(&model, Shown::all(), [0.0, -1.0], [10.0, -1.0], 20.0).fills.is_empty());
    }

    #[test]
    fn a_section_cuts_along_its_line_and_hides_what_is_behind() {
        let layers = vec![
            block("Wall", [0.0, 0.0, 0.0], [4.0, 2.0, 3.0]),
            block("Behind", [1.0, 3.0, 1.0], [2.0, 3.5, 2.0]),
            block("Beside", [6.0, 0.0, 0.0], [7.0, 1.0, 1.0]),
        ];
        let model = Model::new(PathBuf::from("site.obj"), "OBJ", layers);
        // through the wall, looking north (+y): cut along y = 1, u = x + 1
        let section = section_at(&model, Shown::all(), [-1.0, 1.0], [9.0, 1.0], 20.0);
        let (lo, hi) = section.extent.unwrap();
        assert!(section.segments.iter().flatten().all(|p| (1.0..=8.0).contains(&p[0]) && (0.0..=3.0).contains(&p[1])));
        assert_eq!((lo, hi), ([1.0, 0.0], [8.0, 3.0]));
        // an elevation from the south: nothing cut; the box behind the wall hidden, the one
        // beside it seen
        let elevation = section_at(&model, Shown::all(), [-1.0, -1.0], [9.0, -1.0], 20.0);
        assert!(elevation.segments.is_empty());
        let hidden = |[a, b]: &&Segment| [a, b].iter().all(|p| (1.9..=3.1).contains(&p[0]) && (0.9..=2.1).contains(&p[1]));
        let behind: Vec<&Segment> = elevation.below.iter().filter(hidden).collect();
        assert!(behind.is_empty(), "{behind:?}");
        assert!(elevation.below.iter().any(|[a, b]| a[0] >= 7.0 && b[0] >= 7.0));
        // the wall's face, seen whole
        let wall: f32 = elevation.below.iter().filter(|[a, b]| a[0] <= 5.0 && b[0] <= 5.0).map(|[a, b]| (b[0] - a[0]).hypot(b[1] - a[1])).sum();
        assert!(wall >= 14.0 - 0.2, "{wall}");
    }

    #[test]
    fn nothing_is_cut_above_below_or_on_hidden_layers() {
        let mut points = box_layer("Cloud");
        points.points = true;
        let model = Model::new(PathBuf::from("box.obj"), "OBJ", vec![box_layer("Box"), points]);
        assert_eq!(cut_at(&model, Shown::all(), 5.0), Cut::default());
        assert_eq!(cut_at(&model, Shown::all(), -1.0), Cut::default());
        assert_eq!(cut_at(&model, Shown::layers(&[false, true]), 1.0), Cut::default(), "hidden, and a point layer");
        // at the top: the top face lies in the plane and gives nothing; its edges, once each
        assert_eq!(cut_at(&model, Shown::layers(&[true]), 3.0).segments.len(), 4);
    }

    #[test]
    fn the_triangles_of_an_element_hidden_in_the_view_are_left_out() {
        // a wall and, on the same layer, a table: its triangles from vertex 24 on
        let (wall, table) = (block("Room", [0.0, 0.0, 0.0], [4.0, 0.2, 3.0]), block("Table", [1.0, 1.0, 0.0], [2.0, 2.0, 0.8]));
        let mut room = wall.clone();
        room.indices.extend(table.indices.iter().map(|i| i + 24));
        room.positions.extend(&table.positions);
        let model = Model::new(PathBuf::from("room.obj"), "OBJ", vec![room]);
        let table_hidden = |_: usize, v: u32| v >= 24;
        let shown = Shown { layers: &[], hidden: &table_hidden, faint: None, pieces: None };
        let all = plan_at(&model, Shown::all(), 0.0, 0.5);
        let without = plan_at(&model, shown, 0.0, 0.5);
        assert!(without.segments.len() < all.segments.len());
        assert!(without.segments.iter().chain(&without.below).all(|[a, b]| a[1] <= 0.2 + 1e-4 && b[1] <= 0.2 + 1e-4), "only the wall");
        // a section through the table alone
        assert!(!section_at(&model, Shown::all(), [0.0, 1.5], [4.0, 1.5], 5.0).segments.is_empty());
        assert!(section_at(&model, shown, [0.0, 1.5], [4.0, 1.5], 5.0).segments.is_empty());
    }

    #[test]
    fn faces_of_one_piece_lose_the_line_between_them() {
        // two walls end to end, and a third of another piece against the second
        let square = |x: f32| vec![[x, 0.0], [x + 1.0, 0.0], [x + 1.0, 1.0], [x, 1.0]];
        let sides = |l: &[[f32; 2]]| (0..4).map(|k| [l[k], l[(k + 1) % 4]]).collect::<Vec<Segment>>();
        let fills: Vec<Fill> = [0.0, 1.0, 2.0].iter().map(|&x| Fill { loops: vec![square(x)], ..Fill::default() }).collect();
        let segments: Vec<Segment> = fills.iter().flat_map(|f| sides(&f.loops[0])).collect();
        let mut cut = Cut { segments, fills, ..Cut::default() };
        join(&mut cut, &|a, b| a.max(b) < 2);
        // the two sides at x = 1 are gone, those at x = 2 stay
        let upright_at = |x: f32| cut.segments.iter().filter(|s| (s[0][0] - x).abs() < 1e-6 && (s[1][0] - x).abs() < 1e-6).count();
        assert_eq!((upright_at(0.0), upright_at(1.0), upright_at(2.0), upright_at(3.0)), (1, 0, 2, 1));
        assert_eq!(cut.segments.len(), 10);
    }
    #[test]
    fn a_3d_view_draws_what_is_seen_and_a_section_box_cuts_it() {
        let model = Model::new(std::path::PathBuf::from("block.obj"), "OBJ", vec![block("Beton", [0.0, 0.0, 0.0], [4.0, 2.0, 3.0])]);
        let layers = [true];
        // from above the corner at (4, 2, 3), looking down at the block
        let forward = [-1.0, -2.0, -0.5];
        let (right, up, _) = view_axes(forward);
        let on = |p: [f32; 3]| [p[0] * right[0] + p[1] * right[1] + p[2] * right[2], p[0] * up[0] + p[1] * up[1] + p[2] * up[2]];
        let near = |q: [f32; 2], p: [f32; 3]| (q[0] - on(p)[0]).hypot(q[1] - on(p)[1]) < 1e-3;
        let whole = view_at(&model, Shown::layers(&layers), forward, None);
        // the three edges that meet at the far corner are hidden: nothing ends there
        assert!(!whole.below.is_empty());
        assert!(whole.below.iter().flatten().all(|q| !near(*q, [0.0, 0.0, 0.0])), "{:?}", whole.below);
        // the nearest corner shows, with its three edges
        assert!(whole.below.iter().flatten().filter(|q| near(**q, [4.0, 2.0, 3.0])).count() >= 3);
        // the drawing is as large as the block is seen: its outline's corners lie on its edge
        let (lo, hi) = whole.extent.unwrap();
        let corners: Vec<[f32; 2]> = (0..8).map(|k| on([if k & 1 == 0 { 0.0 } else { 4.0 }, if k & 2 == 0 { 0.0 } else { 2.0 }, if k & 4 == 0 { 0.0 } else { 3.0 }])).collect();
        let most = |axis: usize| corners.iter().map(|c| c[axis]).fold(f32::NEG_INFINITY, f32::max);
        assert!((hi[0] - most(0)).abs() < 1e-3 && (hi[1] - most(1)).abs() < 1e-3, "{lo:?} {hi:?}");
        // a section box that keeps the lower half: less high, and the cut's outline at 1.5 m
        let lower = crate::section::SectionBox { min: glam::Vec3::new(-1.0, -1.0, -1.0), max: glam::Vec3::new(5.0, 3.0, 1.5), turn: 0.0, pivot: glam::Vec3::ZERO };
        let half = view_at(&model, Shown::layers(&layers), forward, Some(&lower));
        assert!(half.extent.unwrap().1[1] < hi[1] - 0.5);
        assert!(half.below.iter().flatten().any(|q| near(*q, [4.0, 2.0, 1.5])), "{:?}", half.below);
        assert!(half.below.iter().flatten().all(|q| !near(*q, [4.0, 2.0, 3.0])));
    }
}
