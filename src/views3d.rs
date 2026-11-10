//! Views shown in 3D: every floor plan, section and elevation stands in the model on its own
//! plane (a floor plan at its cut height, a section or elevation upright on its line), drawn as its
//! frame and, for the project's own views, the lines of its cut, with its name at a corner. The
//! views read from a Revit model show their frames: a plan at its cut height over the model, a
//! section or elevation on the near side of its crop. Headless.

use crate::document::Document;
use crate::drawing::Cut;
use crate::drawing2d::clip;
use crate::model::{Bounds, SavedView, ViewKind};
use glam::Vec3;
use std::sync::Arc;

/// A line drawn in 3D: its ends (world) and its colour (sRGB, alpha).
pub type Colored = ([[f32; 3]; 2], [u8; 4]);

/// A view's frame, and the lines of its cut (a deeper orange), that stand apart from the model,
/// its grids (grey) and the underlays (blue).
pub const FRAME: [u8; 4] = [222, 124, 36, 255];
pub const CUT: [u8; 4] = [176, 74, 18, 255];
/// How far a floor plan's frame stands outside what it shows, and a section's frame above and
/// below the model (metres).
const PAD: f64 = 0.5;

/// The lines of the views in 3D and their names with where they stand (world).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Views3d {
    pub lines: Vec<Colored>,
    pub labels: Vec<(String, [f32; 3])>,
}

impl Views3d {
    /// The four sides of the rectangle through `corners`.
    fn frame(&mut self, corners: [[f32; 3]; 4]) {
        for k in 0..4 {
            self.lines.push(([corners[k], corners[(k + 1) % 4]], FRAME));
        }
    }
}

/// A point of the drawing in the plane it stands in.
fn world(p: [f64; 3]) -> [f32; 3] {
    p.map(|v| v as f32)
}

/// The project's own floor plans, sections and elevations in 3D, `cut` giving a view's cut by
/// its path (a view not cut yet shows its frame only), the model's `bounds` framing a view
/// without lines.
pub fn document_views(doc: &Document, cut: &dyn Fn(&str) -> Option<Arc<Cut>>, bounds: Option<&Bounds>) -> Views3d {
    let mut out = Views3d::default();
    let model = bounds.map(|b| ([f64::from(b.min[0]), f64::from(b.min[1]), f64::from(b.min[2])], [f64::from(b.max[0]), f64::from(b.max[1]), f64::from(b.max[2])]));
    for plan in doc.plans() {
        let Some((_, z)) = doc.plan_range(plan) else { continue };
        let c = cut(&plan.path);
        let extent = c.as_ref().and_then(|c| c.extent).map(|(lo, hi)| ([f64::from(lo[0]), f64::from(lo[1])], [f64::from(hi[0]), f64::from(hi[1])]));
        let Some((lo, hi)) = plan.crop.or(extent).or(model.map(|(lo, hi)| ([lo[0], lo[1]], [hi[0], hi[1]]))) else { continue };
        if let Some(c) = &c {
            for [a, b] in &c.segments {
                let (a, b) = ([f64::from(a[0]), f64::from(a[1])], [f64::from(b[0]), f64::from(b[1])]);
                let piece = match plan.crop {
                    Some((lo, hi)) => clip(a, b, lo, hi),
                    None => Some((a, b)),
                };
                if let Some((a, b)) = piece {
                    out.lines.push(([world([a[0], a[1], z]), world([b[0], b[1], z])], CUT));
                }
            }
        }
        let (lo, hi) = ([lo[0] - PAD, lo[1] - PAD], [hi[0] + PAD, hi[1] + PAD]);
        out.frame([world([lo[0], lo[1], z]), world([hi[0], lo[1], z]), world([hi[0], hi[1], z]), world([lo[0], hi[1], z])]);
        out.labels.push((plan.name.clone(), world([lo[0], hi[1], z])));
    }
    for section in doc.sections() {
        let (along, _) = section.axes();
        let run = (section.to[0] - section.from[0]).hypot(section.to[1] - section.from[1]);
        let at = |u: f64, v: f64| world([section.from[0] + along[0] * u, section.from[1] + along[1] * u, v]);
        let (low, high) = model.map_or((0.0, 3.0), |(lo, hi)| (lo[2] - PAD, hi[2] + PAD));
        let ((u0, v0), (u1, v1)) = match section.crop {
            Some((lo, hi)) => ((lo[0], lo[1]), (hi[0], hi[1])),
            None => ((0.0, low), (run, high)),
        };
        if let Some(c) = cut(&section.path) {
            for [a, b] in &c.segments {
                let (a, b) = ([f64::from(a[0]), f64::from(a[1])], [f64::from(b[0]), f64::from(b[1])]);
                if let Some((a, b)) = clip(a, b, [u0, v0], [u1, v1]) {
                    out.lines.push(([at(a[0], a[1]), at(b[0], b[1])], CUT));
                }
            }
        }
        out.frame([at(u0, v0), at(u1, v0), at(u1, v1), at(u0, v1)]);
        out.labels.push((section.name.clone(), at(u0, v1)));
    }
    out
}

/// The views read from the model (Revit's) in 3D: their frames and names.
pub fn saved_views(views: &[SavedView], bounds: Option<&Bounds>) -> Views3d {
    let mut out = Views3d::default();
    for view in views {
        match view.kind {
            ViewKind::Plan => {
                let (Some(range), Some(b)) = (view.plan, bounds) else { continue };
                // a ceiling plan cuts at its low side and looks up
                let z = f64::from(if range.from_below { range.low } else { range.high });
                let (lo, hi) = ([f64::from(b.min[0]) - PAD, f64::from(b.min[1]) - PAD], [f64::from(b.max[0]) + PAD, f64::from(b.max[1]) + PAD]);
                out.frame([world([lo[0], lo[1], z]), world([hi[0], lo[1], z]), world([hi[0], hi[1], z]), world([lo[0], hi[1], z])]);
                out.labels.push((view.name.clone(), world([lo[0], hi[1], z])));
            }
            ViewKind::Section | ViewKind::Elevation | ViewKind::Detail => {
                let Some(crop) = view.clip else { continue };
                // the side of the crop the view looks from: its camera's direction, else the
                // crop's own y
                let forward = view.camera.map_or(crop.axis(1), |c| Vec3::from_array(c.forward));
                let (axis, dot) = [0usize, 1].into_iter().map(|k| (k, crop.axis(k).dot(forward))).max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap_or((1, 1.0));
                let near = if dot >= 0.0 { crop.min[axis] } else { crop.max[axis] };
                let other = 1 - axis;
                let corner = |s: f32, z: f32| {
                    let mut local = Vec3::ZERO;
                    local[axis] = near;
                    local[other] = s;
                    local.z = z;
                    crop.to_world(local).to_array()
                };
                let (s0, s1, z0, z1) = (crop.min[other], crop.max[other], crop.min.z, crop.max.z);
                out.frame([corner(s0, z0), corner(s1, z0), corner(s1, z1), corner(s0, z1)]);
                out.labels.push((view.name.clone(), corner(s0, z1)));
            }
            ViewKind::ThreeD => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::PlanRange;
    use crate::section::SectionBox;

    fn bounds() -> Bounds {
        Bounds { min: [0.0, 0.0, 0.0], max: [8.0, 5.0, 3.0] }
    }

    /// The walls of a room of 8 x 5 m as a plan's cut.
    fn room() -> Cut {
        let segments = vec![[[0.0, 0.0], [8.0, 0.0]], [[8.0, 0.0], [8.0, 5.0]], [[8.0, 5.0], [0.0, 5.0]], [[0.0, 5.0], [0.0, 0.0]]];
        Cut { segments, below: Vec::new(), extent: Some(([0.0, 0.0], [8.0, 5.0])) }
    }

    fn of(v: &Views3d, color: [u8; 4]) -> Vec<[[f32; 3]; 2]> {
        v.lines.iter().filter(|(_, c)| *c == color).map(|(s, _)| *s).collect()
    }

    #[test]
    fn a_floor_plan_lies_at_its_cut_height_with_its_frame_and_name() {
        let mut doc = Document::default();
        doc.add_level("Eerste verdieping", 3.0).unwrap();
        let level = doc.levels()[0].path.clone();
        let plan = doc.add_plan(&level).unwrap();
        let cut = Arc::new(room());
        let v = document_views(&doc, &|p| (p == plan).then(|| cut.clone()), Some(&bounds()));
        // 1.2 m above its level
        let lines = of(&v, CUT);
        assert_eq!(lines.len(), 4);
        assert!(lines.iter().flatten().all(|p| (p[2] - 4.2).abs() < 1e-5), "{lines:?}");
        assert!(lines.iter().any(|s| s[0][..2] == [0.0, 0.0] && s[1][..2] == [8.0, 0.0]));
        // its frame: four sides round the cut at the same height, a little outside it
        let frame = of(&v, FRAME);
        assert_eq!(frame.len(), 4);
        assert!(frame.iter().flatten().all(|p| (p[2] - 4.2).abs() < 1e-5 && (p[0] < 0.0 || p[0] > 8.0 || p[1] < 0.0 || p[1] > 5.0)), "{frame:?}");
        assert_eq!(v.labels.len(), 1);
        assert_eq!(v.labels[0].0, "Eerste verdieping");
    }

    #[test]
    fn a_section_stands_upright_on_its_line() {
        let mut doc = Document::default();
        let section = doc.add_section("Section A", [0.0, 2.0], [8.0, 2.0], 10.0).unwrap();
        // its drawing: u along the line from (0, 2), v up; a wall cut at u = 3 from 0 to 3 m
        let cut = Arc::new(Cut { segments: vec![[[3.0, 0.0], [3.0, 3.0]]], below: Vec::new(), extent: Some(([3.0, 0.0], [3.0, 3.0])) });
        let v = document_views(&doc, &|p| (p == section).then(|| cut.clone()), Some(&bounds()));
        let lines = of(&v, CUT);
        assert_eq!(lines, vec![[[3.0, 2.0, 0.0], [3.0, 2.0, 3.0]]]);
        // its frame upright in the plane y = 2, from the model's foot to its top
        let frame = of(&v, FRAME);
        assert_eq!(frame.len(), 4);
        assert!(frame.iter().flatten().all(|p| (p[1] - 2.0).abs() < 1e-5), "{frame:?}");
        let (low, high) = frame.iter().flatten().fold((f32::MAX, f32::MIN), |(l, h), p| (l.min(p[2]), h.max(p[2])));
        assert!(low <= 0.0 && high >= 3.0, "{low} {high}");
        assert_eq!(v.labels[0].0, "Section A");
    }

    #[test]
    fn a_view_not_cut_yet_shows_its_frame() {
        let mut doc = Document::default();
        doc.add_level("Begane grond", 0.0).unwrap();
        let level = doc.levels()[0].path.clone();
        doc.add_plan(&level).unwrap();
        let v = document_views(&doc, &|_| None, Some(&bounds()));
        assert!(of(&v, CUT).is_empty());
        assert_eq!(of(&v, FRAME).len(), 4, "framed round the model");
    }

    #[test]
    fn revit_views_show_their_frames() {
        let plan = SavedView {
            name: "Level 1".into(),
            kind: ViewKind::Plan,
            camera: None,
            clip: None,
            plan: Some(PlanRange { low: -0.3, high: 1.2, from_below: false }),
            level: Some("Level 1".into()),
        };
        // a section looking north (+y) from y = -1, 8 m wide and 3 m high
        let crop = SectionBox::from_frame(Vec3::new(0.0, -1.0, 0.0), [Vec3::X, Vec3::Z, Vec3::Y], Vec3::new(0.0, 0.0, 0.0), Vec3::new(8.0, 3.0, 6.0)).unwrap();
        let section = SavedView { name: "Section 1".into(), kind: ViewKind::Section, camera: None, clip: Some(crop), plan: None, level: None };
        let three_d = SavedView { name: "{3D}".into(), kind: ViewKind::ThreeD, camera: None, clip: None, plan: None, level: None };
        let v = saved_views(&[plan, section, three_d], Some(&bounds()));
        let frame = of(&v, FRAME);
        assert_eq!(frame.len(), 8, "two frames: {frame:?}");
        // the plan's at its cut height
        assert_eq!(frame.iter().flatten().filter(|p| (p[2] - 1.2).abs() < 1e-5).count(), 8);
        // the section's on the near side of its crop, the plane y = -1
        assert_eq!(frame.iter().flatten().filter(|p| (p[1] + 1.0).abs() < 1e-5).count(), 8);
        let names: Vec<&str> = v.labels.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["Level 1", "Section 1"]);
    }
}
