//! The cut faces of the section box and the section plane in 3D: where a face of the box, or the
//! plane, goes through an element, that element's cut is drawn in the hatch of what it is made of
//! (the cut pattern of its Revit material, else the drawing standard's), as triangles and lines
//! standing on the face. Headless.

use crate::drawing::{faces_at, Fill, Shown};
use crate::hatching::{pattern_of, rows};
use crate::model::Model;
use crate::section::{SectionBox, SectionPlane};
use crate::views3d::Colored;
use bim_library::DrawingStandard;
use glam::Vec3;

/// Hatches sized on paper are drawn in 3D as on a drawing at 1 : this.
pub const SCALE: f64 = 100.0;
/// The faces stand this far outside the cut (metres), and their lines this far, so they win
/// from the model's own cut fill and from each other.
const FACE_LIFT: f32 = 0.002;
const LINE_LIFT: f32 = 0.004;

/// The cut faces in 3D: filled triangles with their colour, and the hatch lines over them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CutFaces {
    pub triangles: Vec<([[f32; 3]; 3], [u8; 4])>,
    pub lines: Vec<Colored>,
}

impl CutFaces {
    pub fn is_empty(&self) -> bool {
        self.triangles.is_empty() && self.lines.is_empty()
    }

    /// The faces of one cut: `fills` in the cut's own (u, v), cut back to `crop`, hatched, and
    /// stood in the model by `world` (a point of the cut, and how far outside it).
    fn add(&mut self, model: &Model, fills: Vec<Fill>, crop: Option<([f64; 2], [f64; 2])>, standard: DrawingStandard, world: &dyn Fn([f64; 2], f32) -> [f32; 3]) {
        for mut fill in fills {
            let loops = fill.cropped(crop);
            if loops.is_empty() {
                continue;
            }
            let pattern = pattern_of(model, fill.layer, fill.element, standard);
            let (behind, line) = pattern.look();
            for [a, b, c] in triangulate(&loops) {
                self.triangles.push(([world(a, FACE_LIFT), world(b, FACE_LIFT), world(c, FACE_LIFT)], [behind[0], behind[1], behind[2], 255]));
            }
            fill.loops = loops.iter().map(|l| l.iter().map(|p| [p[0] as f32, p[1] as f32]).collect()).collect();
            let (hatch, _) = rows(&fill.loops, &pattern, SCALE);
            let wide = |p: [f32; 2]| world([f64::from(p[0]), f64::from(p[1])], LINE_LIFT);
            self.lines.extend(hatch.into_iter().map(|(_, [a, b])| ([wide(a), wide(b)], [line[0], line[1], line[2], 255])));
            // the outline of the face, as a drawing's cut line
            for outline in &fill.loops {
                for k in 0..outline.len() {
                    self.lines.push(([wide(outline[k]), wide(outline[(k + 1) % outline.len()])], [0, 0, 0, 255]));
                }
            }
        }
    }
}

/// Where a face of a section box lies: its axis and side, the face's place along the axis, and the
/// turn and pivot of the box (their bits). What a face cuts depends on that alone, not on where
/// the other faces are: they only crop it.
pub type FaceKey = [u32; 7];

/// What each face of a section box cut, before it was cropped to the box (see [`cut_faces_kept`]).
pub type KeptFaces = std::collections::HashMap<FaceKey, Vec<Fill>>;

/// The cut faces of what is `shown` of the model on the six faces of `section` and on `plane`.
pub fn cut_faces(model: &Model, shown: Shown, section: Option<&SectionBox>, plane: Option<&SectionPlane>, standard: DrawingStandard) -> CutFaces {
    cut_faces_kept(model, shown, section, plane, standard, &mut KeptFaces::new())
}

/// As [`cut_faces`], taking what a face of the box cut from `kept` when it lay there before, and
/// keeping there what the six faces cut now: dragging one face of the box cuts that face alone
/// again. `kept` holds for one model and what is shown of it.
pub fn cut_faces_kept(model: &Model, shown: Shown, section: Option<&SectionBox>, plane: Option<&SectionPlane>, standard: DrawingStandard, kept: &mut KeptFaces) -> CutFaces {
    let mut out = CutFaces::default();
    let mut now = KeptFaces::new();
    if let Some(s) = section {
        for axis in 0..3 {
            let (b, c) = ((axis + 1) % 3, (axis + 2) % 3);
            for (limit, sign) in [(s.min[axis], 1.0f32), (s.max[axis], -1.0)] {
                // the box's own frame: (u, v) on the face, the depth into the box
                let frame = move |p: [f32; 3]| {
                    let l = s.to_local(Vec3::from_array(p));
                    [l[b], l[c], (l[axis] - limit) * sign]
                };
                let crop = Some(([f64::from(s.min[b]), f64::from(s.min[c])], [f64::from(s.max[b]), f64::from(s.max[c])]));
                let world = move |q: [f64; 2], lift: f32| {
                    let mut l = Vec3::ZERO;
                    l[axis] = limit - sign * lift;
                    l[b] = q[0] as f32;
                    l[c] = q[1] as f32;
                    s.to_world(l).to_array()
                };
                let key: FaceKey = [axis as u32, (sign > 0.0) as u32, limit.to_bits(), s.turn.to_bits(), s.pivot.x.to_bits(), s.pivot.y.to_bits(), s.pivot.z.to_bits()];
                let fills = kept.remove(&key).unwrap_or_else(|| faces_at(model, shown, frame));
                out.add(model, fills.clone(), crop, standard, &world);
                now.insert(key, fills);
            }
        }
    }
    *kept = now;
    if let Some(p) = plane {
        let (u, v) = p.basis();
        let frame = move |q: [f32; 3]| {
            let q = Vec3::from_array(q);
            [u.dot(q), v.dot(q), p.offset - p.normal.dot(q)]
        };
        let world = move |q: [f64; 2], lift: f32| (p.normal * (p.offset + lift) + u * q[0] as f32 + v * q[1] as f32).to_array();
        out.add(model, faces_at(model, shown, frame), None, standard, &world);
    }
    out
}

/// Whether `p` lies inside the outline.
fn inside(outline: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut odd = false;
    for k in 0..outline.len() {
        let (a, b) = (outline[k], outline[(k + 1) % outline.len()]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
            odd = !odd;
        }
    }
    odd
}

/// The triangles of what the outlines fill by the even-odd rule: an outline inside an odd number
/// of the others is a hole of the one just round it.
fn triangulate(loops: &[Vec<[f64; 2]>]) -> Vec<[[f64; 2]; 3]> {
    let area = |l: &[[f64; 2]]| (0..l.len()).map(|k| l[k][0] * l[(k + 1) % l.len()][1] - l[(k + 1) % l.len()][0] * l[k][1]).sum::<f64>().abs() / 2.0;
    // the outlines round each one
    let round: Vec<Vec<usize>> = (0..loops.len()).map(|i| (0..loops.len()).filter(|&j| j != i && inside(&loops[j], loops[i][0])).collect()).collect();
    let mut out = Vec::new();
    for (i, outer) in loops.iter().enumerate().filter(|(i, _)| round[*i].len() % 2 == 0) {
        // its holes: one level deeper, with this outline the smallest round them
        let holes: Vec<&Vec<[f64; 2]>> = (0..loops.len())
            .filter(|&j| round[j].len() == round[i].len() + 1 && round[j].iter().copied().min_by(|a, b| area(&loops[*a]).total_cmp(&area(&loops[*b]))) == Some(i))
            .map(|j| &loops[j])
            .collect();
        let mut points: Vec<[f64; 2]> = outer.clone();
        let mut starts = Vec::new();
        for hole in holes {
            starts.push(points.len());
            points.extend(hole.iter().copied());
        }
        let flat: Vec<f64> = points.iter().flatten().copied().collect();
        let corners = earcutr::earcut(&flat, &starts, 2).unwrap_or_default();
        out.extend(corners.chunks_exact(3).map(|t| [points[t[0]], points[t[1]], points[t[2]]]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Bounds, Layer};
    use std::path::PathBuf;

    /// A closed box from (0, 0, 0) to (4, 2, 3).
    fn box_layer(name: &str) -> Layer {
        let positions = vec![0., 0., 0., 4., 0., 0., 4., 2., 0., 0., 2., 0., 0., 0., 3., 4., 0., 3., 4., 2., 3., 0., 2., 3.];
        let indices = vec![0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7, 6, 3, 0, 4, 3, 4, 7];
        Layer { name: name.into(), color: [0.5; 3], normals: vec![0.0; positions.len()], positions, colors: None, indices, elements: Vec::new(), props: Vec::new(), hidden: false, points: false }
    }

    fn area(faces: &CutFaces) -> f32 {
        faces.triangles.iter().map(|(t, _)| {
            let [a, b, c] = t.map(Vec3::from_array);
            (b - a).cross(c - a).length() / 2.0
        }).sum()
    }

    #[test]
    fn a_box_face_through_a_solid_gives_its_cut_in_its_hatch() {
        let model = Model::new(PathBuf::from("test.obj"), "OBJ", vec![box_layer("Metselwerk")]);
        // the box keeps x up to 1: its face at x = 1 cuts 2 × 3 out of the solid
        let section = SectionBox { min: Vec3::new(-1.0, -1.0, -1.0), max: Vec3::new(1.0, 5.0, 5.0), turn: 0.0, pivot: Vec3::ZERO };
        let faces = cut_faces(&model, Shown::all(), Some(&section), None, DrawingStandard::Nen47);
        assert!((area(&faces) - 6.0).abs() < 1e-3, "{}", area(&faces));
        assert!(faces.triangles.iter().all(|(t, c)| t.iter().all(|p| (p[0] - 1.0).abs() < 1e-2) && *c == [212, 144, 143, 255]), "on the face, in brick's fill");
        let hatch = faces.lines.iter().filter(|(_, c)| *c == [0, 0, 0, 255]).count();
        assert!(hatch > 20, "{hatch} lines");
        assert!(faces.lines.iter().all(|([a, b], _)| (a[0] - 1.0).abs() < 1e-2 && (b[0] - 1.0).abs() < 1e-2));
    }

    /// Moving one face of the box cuts that face again alone; the faces it kept give what cutting
    /// them all again gives, cropped to where the box is now.
    #[test]
    fn a_moved_face_alone_is_cut_again() {
        let model = Model::new(PathBuf::from("test.obj"), "OBJ", vec![box_layer("Beton")]);
        let mut kept = KeptFaces::new();
        let first = SectionBox { min: Vec3::new(-1.0, 0.5, 1.0), max: Vec3::new(1.0, 5.0, 5.0), turn: 0.0, pivot: Vec3::ZERO };
        let _ = cut_faces_kept(&model, Shown::all(), Some(&first), None, DrawingStandard::Nen47, &mut kept);
        assert_eq!(kept.len(), 6);
        let moved = SectionBox { max: Vec3::new(1.5, 5.0, 2.5), ..first };
        let from_kept = cut_faces_kept(&model, Shown::all(), Some(&moved), None, DrawingStandard::Nen47, &mut kept);
        assert_eq!(from_kept, cut_faces(&model, Shown::all(), Some(&moved), None, DrawingStandard::Nen47));
        assert!(area(&from_kept) > 0.1);
        assert_eq!(kept.len(), 6, "the faces of where the box is now");
    }

    #[test]
    fn a_face_is_cut_back_to_the_box() {
        let model = Model::new(PathBuf::from("test.obj"), "OBJ", vec![box_layer("Beton")]);
        // the box keeps x up to 1 and z up to 2: its face at x = 1 shows 2 × 2
        let section = SectionBox { min: Vec3::new(-1.0, -1.0, -1.0), max: Vec3::new(1.0, 5.0, 2.0), turn: 0.0, pivot: Vec3::ZERO };
        let faces = cut_faces(&model, Shown::all(), Some(&section), None, DrawingStandard::Nen47);
        let at_x: f32 = faces.triangles.iter().filter(|(t, _)| t.iter().all(|p| (p[0] - 1.0).abs() < 1e-2)).map(|(t, _)| {
            let [a, b, c] = t.map(Vec3::from_array);
            (b - a).cross(c - a).length() / 2.0
        }).sum();
        assert!((at_x - 4.0).abs() < 1e-3, "{at_x}");
        // and its face at z = 2 shows 1 × 2 of the solid's 4 × 2
        assert!((area(&faces) - 6.0).abs() < 1e-3, "{}", area(&faces));
    }

    #[test]
    fn the_section_plane_gives_its_cut() {
        let model = Model::new(PathBuf::from("test.obj"), "OBJ", vec![box_layer("Beton")]);
        let world = Bounds { min: [0.0, 0.0, 0.0], max: [4.0, 2.0, 3.0] };
        let plane = SectionPlane::on_face(Vec3::new(2.0, 1.0, 1.5), Vec3::Z, &world).unwrap();
        let faces = cut_faces(&model, Shown::all(), None, Some(&plane), DrawingStandard::Nen47);
        assert!((area(&faces) - 8.0).abs() < 1e-2, "{}", area(&faces));
        assert!(faces.triangles.iter().all(|(t, _)| t.iter().all(|p| (p[2] - plane.offset).abs() < 1e-2)));
    }

    #[test]
    fn a_hole_is_left_open() {
        let outer = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        let hole = vec![[1.0, 1.0], [3.0, 1.0], [3.0, 3.0], [1.0, 3.0]];
        let triangles = triangulate(&[outer, hole]);
        let area: f64 = triangles.iter().map(|[a, b, c]| ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() / 2.0).sum();
        assert!((area - 12.0).abs() < 1e-9, "{area}");
    }
}
