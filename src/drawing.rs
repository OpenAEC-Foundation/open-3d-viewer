//! Drawings derived from the model for sheets: the lines where a floor plan's cut meets the
//! drawn triangles, in plan (x, y), metres in the model's frame. Headless, like the document.

use crate::model::Model;

/// A line on a drawing: from, to (x, y).
pub type Segment = [[f32; 2]; 2];

/// The cut of a floor plan: the line segments where the triangles of the shown layers cross the
/// plane at height `z`, and their extent.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cut {
    pub segments: Vec<Segment>,
    /// Lower left and upper right of the segments; `None` when nothing is cut.
    pub extent: Option<([f32; 2], [f32; 2])>,
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
    let extent = segments.iter().flatten().fold(None, |e: Option<([f32; 2], [f32; 2])>, q| {
        Some(match e {
            None => (*q, *q),
            Some((lo, hi)) => ([lo[0].min(q[0]), lo[1].min(q[1])], [hi[0].max(q[0]), hi[1].max(q[1])]),
        })
    });
    Cut { segments, extent }
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
