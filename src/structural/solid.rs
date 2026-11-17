//! A member as a solid: the outline of its section swept along its line.
//!
//! The profile's u lies along the frame's y and its v along the frame's z, so the depth of an I
//! section stands along the member's local z. The same sweep makes the solids a reader gives a
//! model and the solids of the deformed shape, which only differ in where the member's ends are.

use bim_structural::frame::MemberFrame;
use bim_structural::section::Loop;
use glam::{DVec2, DVec3};

/// A triangle of a solid in world metres, with the normal of its face.
pub type Triangle = ([DVec3; 3], DVec3);

/// The solid of `loops` (the outline, then its holes, as `Outline2::loops` gives them) along the
/// frame's x over its length: a flat face per edge of every loop and the two ends.
pub fn sweep(loops: &[Loop], f: &MemberFrame) -> Vec<Triangle> {
    let mut out = Vec::new();
    if !(f.length > 0.0 && f.length.is_finite()) {
        return out;
    }
    let place = |p: DVec2, along: f64| f.origin + f.x * along + f.y * p.x + f.z * p.y;
    // the sides: holes run the other way round, so their faces look inward by the same rule
    for ring in loops {
        let n = ring.points.len();
        for k in 0..n {
            let (p, q) = (ring.points[k], ring.points[(k + 1) % n]);
            let edge = q - p;
            let normal = (f.y * edge.y - f.z * edge.x).normalize_or_zero();
            let (a, b, c, d) = (place(p, 0.0), place(q, 0.0), place(q, f.length), place(p, f.length));
            out.push(([a, b, c], normal));
            out.push(([a, c, d], normal));
        }
    }
    // the two ends: the outline with its holes, triangulated once
    let (mut flat, mut holes, mut points): (Vec<f64>, Vec<usize>, Vec<DVec2>) = Default::default();
    for ring in loops.iter().filter(|r| !r.hole).chain(loops.iter().filter(|r| r.hole)) {
        if ring.hole {
            holes.push(points.len());
        }
        for p in &ring.points {
            flat.extend([p.x, p.y]);
            points.push(*p);
        }
    }
    let triangles = earcutr::earcut(&flat, &holes, 2).unwrap_or_default();
    for (along, normal, flip) in [(0.0, -f.x, true), (f.length, f.x, false)] {
        for t in triangles.chunks_exact(3) {
            let (i, j, k) = (place(points[t[0]], along), place(points[t[1]], along), place(points[t[2]], along));
            out.push((if flip { [i, k, j] } else { [i, j, k] }, normal));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use bim_structural::{frame, section, Shape};

    #[test]
    fn a_tube_is_swept_with_its_hole_and_closed_at_both_ends() {
        let outline = section::outline(&Shape::Chs { d: 0.1, t: 0.005 }, 0.0005, 12).expect("a tube");
        let f = frame::member_frame(DVec3::ZERO, DVec3::new(0.0, 0.0, 2.0), DVec3::Y).expect("a frame");
        let solid = sweep(&outline.loops(), &f);
        assert!(solid.len() > 40);
        let (mut low, mut high) = (DVec3::INFINITY, DVec3::NEG_INFINITY);
        for (corners, normal) in &solid {
            assert!((normal.length() - 1.0).abs() < 1e-9);
            for c in corners {
                low = low.min(*c);
                high = high.max(*c);
            }
        }
        assert!((high - low - DVec3::new(0.1, 0.1, 2.0)).abs().max_element() < 1e-3, "{low} {high}");
        // the inside of the tube looks at its axis, the outside away from it
        let inward = solid.iter().filter(|(c, n)| n.z.abs() < 1e-9 && n.dot(DVec3::new(c[0].x, c[0].y, 0.0)) < 0.0).count();
        let outward = solid.iter().filter(|(c, n)| n.z.abs() < 1e-9 && n.dot(DVec3::new(c[0].x, c[0].y, 0.0)) > 0.0).count();
        assert!(inward > 0 && inward == outward, "{inward} {outward}");
        // no length, no solid
        assert!(sweep(&outline.loops(), &frame::MemberFrame { length: 0.0, ..f }).is_empty());
    }
}
