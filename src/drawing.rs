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

/// A floor plan's drawing: the cut at `high` and the edges seen from above between `low` and
/// `high`, of the layers that are `shown`.
pub fn plan_at(model: &Model, shown: &[bool], low: f32, high: f32) -> Cut {
    let mut cut = cut_at(model, shown, high);
    cut.below = edges_between(model, shown, low, high);
    cut.extent = extent(cut.segments.iter().chain(&cut.below));
    cut
}

/// The outlines and creases of the triangles between `low` and `high`, clipped to that range and
/// seen from above; an edge seen end on (a vertical one) gives nothing.
pub fn edges_between(model: &Model, shown: &[bool], low: f32, high: f32) -> Vec<Segment> {
    // edges by their ends rounded to 0.1 mm, so faces that do not share vertices still meet
    let key = |p: [f32; 3]| p.map(|v| (v * 1e4).round() as i64);
    let crease = CREASE_DEGREES.to_radians().cos();
    // per edge: its ends, the normal of its first face, how many faces, a crease seen
    let mut edges: HashMap<([i64; 3], [i64; 3]), ([f32; 3], [f32; 3], [f32; 3], u32, bool)> = HashMap::new();
    for (i, layer) in model.layers.iter().enumerate() {
        if layer.points || !shown.get(i).copied().unwrap_or(true) {
            continue;
        }
        let p = &layer.positions;
        for t in layer.indices.chunks_exact(3) {
            let Some(c) = t.iter().map(|&k| p.get(k as usize * 3..k as usize * 3 + 3).map(|v| [v[0], v[1], v[2]])).collect::<Option<Vec<_>>>() else { continue };
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
        let at = |t: f32| [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])];
        let (p, q) = (at(t0), at(t1));
        if (q[0] - p[0]).hypot(q[1] - p[1]) > 1e-4 {
            out.push([p, q]);
        }
    }
    out
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

/// Cuts the layers that are `shown` (all of them when `shown` is shorter) at height `z`. Point
/// layers are left out.
pub fn cut_at(model: &Model, shown: &[bool], z: f32) -> Cut {
    let mut segments = Vec::new();
    for (i, layer) in model.layers.iter().enumerate() {
        if layer.points || !shown.get(i).copied().unwrap_or(true) {
            continue;
        }
        let p = &layer.positions;
        let vertex = |k: u32| {
            let k = k as usize * 3;
            [p[k], p[k + 1], p[k + 2]]
        };
        for t in layer.indices.chunks_exact(3) {
            if t.iter().any(|&k| (k as usize * 3 + 2) >= p.len()) {
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
        let cut = cut_at(&model, &[], 1.2);
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
        let plan = plan_at(&model, &[], -0.3, 1.2);
        // the wall is cut; the table under the cut shows its outline (top and bottom edges seen
        // from above), not the diagonals of its faces, not its vertical edges
        assert_eq!(plan.segments.len(), 8);
        let table: Vec<&Segment> = plan.below.iter().filter(|[a, b]| a[0] >= 5.0 && b[0] >= 5.0).collect();
        assert_eq!(table.len(), 8, "{table:?}");
        let length: f32 = table.iter().map(|[a, b]| (b[0] - a[0]).hypot(b[1] - a[1])).sum();
        assert!((length - 8.0).abs() < 1e-4, "{length}");
        assert_eq!(plan.extent, Some(([0.0, 0.0], [6.0, 2.0])));
        // above the table's top, the range shows nothing of it
        assert!(edges_between(&model, &[], 1.0, 1.2).iter().all(|[a, b]| a[0] < 5.0 && b[0] < 5.0));
    }

    #[test]
    fn nothing_is_cut_above_below_or_on_hidden_layers() {
        let mut points = box_layer("Cloud");
        points.points = true;
        let model = Model::new(PathBuf::from("box.obj"), "OBJ", vec![box_layer("Box"), points]);
        assert_eq!(cut_at(&model, &[], 5.0), Cut::default());
        assert_eq!(cut_at(&model, &[], -1.0), Cut::default());
        assert_eq!(cut_at(&model, &[false, true], 1.0), Cut::default(), "hidden, and a point layer");
        // at the top: the top face lies in the plane and gives nothing; its edges, once each
        assert_eq!(cut_at(&model, &[true], 3.0).segments.len(), 4);
    }
}
