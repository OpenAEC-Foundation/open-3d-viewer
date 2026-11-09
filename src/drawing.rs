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
    /// The edges seen from above between the plan's view depth and its cut: outlines and creases
    /// of the triangles there, drawn thin under the cut.
    pub below: Vec<Segment>,
    /// Lower left and upper right of the segments; `None` when nothing is cut.
    pub extent: Option<([f32; 2], [f32; 2])>,
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
}

/// Hides no triangle.
fn none_hidden(_: usize, _: u32) -> bool {
    false
}

impl<'a> Shown<'a> {
    /// The layers that are on, with no element hidden.
    pub fn layers(layers: &'a [bool]) -> Self {
        Shown { layers, hidden: &none_hidden }
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
/// `high`, of what is `shown`.
pub fn plan_at(model: &Model, shown: Shown, low: f32, high: f32) -> Cut {
    let mut cut = cut_at(model, shown, high);
    cut.below = edges_between(model, shown, low, high);
    cut.extent = extent(cut.segments.iter().chain(&cut.below));
    cut
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
    // edges by their ends rounded to 0.1 mm, so faces that do not share vertices still meet
    let key = |p: [f32; 3]| p.map(|v| (v * 1e4).round() as i64);
    let crease = CREASE_DEGREES.to_radians().cos();
    // per edge: its ends, the normal of its first face, how many faces, a crease seen
    let mut edges: HashMap<([i64; 3], [i64; 3]), ([f32; 3], [f32; 3], [f32; 3], u32, bool)> = HashMap::new();
    for t in triangles(model, shown) {
        let c = t.map(&map);
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
        for k in 0..3 {
            let (a, b) = (c[k], c[(k + 1) % 3]);
            let (ka, kb) = (key(a), key(b));
            let id = if ka <= kb { (ka, kb) } else { (kb, ka) };
            let entry = edges.entry(id).or_insert((a, b, n, 0, false));
            entry.3 += 1;
            if entry.3 > 1 && entry.2[0] * n[0] + entry.2[1] * n[1] + entry.2[2] * n[2] < crease {
                entry.4 = true;
            }
        }
    }
    let mut out = Vec::new();
    for (a, b, _, faces, sharp) in edges.into_values() {
        if faces > 1 && !sharp {
            continue;
        }
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
        out.push([at(t0), at(t1)]);
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
    for t in triangles(model, shown) {
        let c = t.map(map);
        if let Some(segment) = cross(c, 0.0) {
            segments.push(segment);
        }
        let (near, far) = (c[0][2].min(c[1][2]).min(c[2][2]), c[0][2].max(c[1][2]).max(c[2][2]));
        if far > 0.0 && near < depth as f32 {
            beyond.push(c);
        }
    }
    let edges = feature_edges(model, shown, map, 0.0, depth as f32);
    let below = visible(&beyond, &edges);
    let extent = extent(segments.iter().chain(&below));
    Cut { segments, below, extent }
}

/// How many pixels the longer side of a section's depth image has.
const DEPTH_PIXELS: f32 = 2048.0;

/// The parts of `edges` (u, v, depth) that no triangle of `faces` covers: the faces are drawn
/// into a depth image of their part beyond depth 0, and every edge is walked through it.
fn visible(faces: &[[[f32; 3]; 3]], edges: &[[[f32; 3]; 2]]) -> Vec<Segment> {
    let corners = faces.iter().flatten().chain(edges.iter().flatten());
    let (mut lo, mut hi) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
    for c in corners {
        lo = [lo[0].min(c[0]), lo[1].min(c[1])];
        hi = [hi[0].max(c[0]), hi[1].max(c[1])];
    }
    if !(lo[0] <= hi[0] && lo[1] <= hi[1]) {
        return Vec::new();
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
    let mut out = Vec::new();
    for &[a, b] in edges {
        let steps = (((b[0] - a[0]).hypot(b[1] - a[1]) / pixel * 1.5).ceil() as usize).max(1);
        let point = |k: usize| {
            let t = k as f32 / steps as f32;
            [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1]), a[2] + t * (b[2] - a[2])]
        };
        let mut start: Option<[f32; 3]> = None;
        let mut last = a;
        for k in 0..=steps {
            let p = point(k);
            let seen = p[2] <= at(p) + slack;
            match (seen, start) {
                (true, None) => start = Some(p),
                (false, Some(s)) => {
                    out.extend(flat([s, last]));
                    start = None;
                }
                _ => {}
            }
            last = p;
        }
        if let Some(s) = start {
            out.extend(flat([s, b]));
        }
    }
    out
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
    for (i, layer) in model.layers.iter().enumerate() {
        if layer.points || !shown.on(i) {
            continue;
        }
        let p = &layer.positions;
        let vertex = |k: u32| {
            let k = k as usize * 3;
            [p[k], p[k + 1], p[k + 2]]
        };
        for t in layer.indices.chunks_exact(3) {
            if t.iter().any(|&k| (k as usize * 3 + 2) >= p.len()) || (shown.hidden)(i, t[0]) {
                continue;
            }
            let corners = [vertex(t[0]), vertex(t[1]), vertex(t[2])];
            if let Some(segment) = cross(corners, z) {
                segments.push(segment);
            }
        }
    }
    let extent = extent(segments.iter());
    Cut { segments, below: Vec::new(), extent }
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
        assert_eq!(table.len(), 8, "{table:?}");
        let length: f32 = table.iter().map(|[a, b]| (b[0] - a[0]).hypot(b[1] - a[1])).sum();
        assert!((length - 8.0).abs() < 1e-4, "{length}");
        assert_eq!(plan.extent, Some(([0.0, 0.0], [6.0, 2.0])));
        // above the table's top, the range shows nothing of it
        assert!(edges_between(&model, Shown::all(), 1.0, 1.2).iter().all(|[a, b]| a[0] < 5.0 && b[0] < 5.0));
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
        let shown = Shown { layers: &[], hidden: &table_hidden };
        let all = plan_at(&model, Shown::all(), 0.0, 0.5);
        let without = plan_at(&model, shown, 0.0, 0.5);
        assert!(without.segments.len() < all.segments.len());
        assert!(without.segments.iter().chain(&without.below).all(|[a, b]| a[1] <= 0.2 + 1e-4 && b[1] <= 0.2 + 1e-4), "only the wall");
        // a section through the table alone
        assert!(!section_at(&model, Shown::all(), [0.0, 1.5], [4.0, 1.5], 5.0).segments.is_empty());
        assert!(section_at(&model, shown, [0.0, 1.5], [4.0, 1.5], 5.0).segments.is_empty());
    }
}
