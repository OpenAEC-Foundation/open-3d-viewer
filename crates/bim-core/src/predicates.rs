//! Exact geometric predicates. Every topological decision of the kernel (which side of a line or a
//! plane, do two edges cross) goes through these, never through a plain floating-point comparison
//! (ADR-0006). They are Shewchuk's adaptive predicates (the `robust` crate): a floating-point
//! filter first, exact expansion arithmetic only when the filter cannot decide, so the sign is
//! always right for the f64 inputs given.

use glam::{DVec2, DVec3};
use std::cmp::Ordering;

/// Which side of the directed line `a → b` point `c` lies on: `Greater` left (counter-clockwise),
/// `Less` right, `Equal` on the line. Exact.
pub fn orient2d(a: DVec2, b: DVec2, c: DVec2) -> Ordering {
    let p = |v: DVec2| robust::Coord { x: v.x, y: v.y };
    sign(robust::orient2d(p(a), p(b), p(c)))
}

/// Which side of the plane through `a`, `b`, `c` point `d` lies on: `Greater` on the side the
/// normal `(b − a) × (c − a)` points to, `Less` on the other, `Equal` in the plane. Exact.
pub fn orient3d(a: DVec3, b: DVec3, c: DVec3, d: DVec3) -> Ordering {
    let p = |v: DVec3| robust::Coord3D { x: v.x, y: v.y, z: v.z };
    // robust's sign is positive below the plane, where a, b, c turn clockwise
    sign(-robust::orient3d(p(a), p(b), p(c), p(d)))
}

fn sign(v: f64) -> Ordering {
    v.partial_cmp(&0.0).unwrap_or(Ordering::Equal)
}

/// Whether `c` lies on the closed segment `a`–`b`, given that it lies on its line.
fn within(a: DVec2, b: DVec2, c: DVec2) -> bool {
    c.x >= a.x.min(b.x) && c.x <= a.x.max(b.x) && c.y >= a.y.min(b.y) && c.y <= a.y.max(b.y)
}

/// Whether the closed segments `a`–`b` and `c`–`d` share a point (crossing, touching or
/// overlapping). Exact.
pub fn segments_meet(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let opposite = |p: Ordering, q: Ordering| p != Ordering::Equal && q != Ordering::Equal && p != q;
    let (d1, d2) = (orient2d(a, b, c), orient2d(a, b, d));
    let (d3, d4) = (orient2d(c, d, a), orient2d(c, d, b));
    if opposite(d1, d2) && opposite(d3, d4) {
        return true;
    }
    (d1 == Ordering::Equal && within(a, b, c))
        || (d2 == Ordering::Equal && within(a, b, d))
        || (d3 == Ordering::Equal && within(c, d, a))
        || (d4 == Ordering::Equal && within(c, d, b))
}

/// Where a point lies against a closed polygon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inside {
    Inside,
    Outside,
    Boundary,
}

/// Whether `p` lies inside, outside or on the boundary of the polygon `ring` (either
/// orientation, not closed by a repeated point). Exact: crossings are counted with `orient2d`.
pub fn point_in_ring(p: DVec2, ring: &[DVec2]) -> Inside {
    let mut winding = 0i32;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        let side = orient2d(a, b, p);
        if side == Ordering::Equal && within(a, b, p) {
            return Inside::Boundary;
        }
        if a.y <= p.y {
            if b.y > p.y && side == Ordering::Greater {
                winding += 1;
            }
        } else if b.y <= p.y && side == Ordering::Less {
            winding -= 1;
        }
    }
    if winding == 0 { Inside::Outside } else { Inside::Inside }
}

/// The orientation of a simple polygon whose consecutive points are distinct and never three on
/// one line (see [`crate::profile::Profile::new`]): `Greater` counter-clockwise, `Less`
/// clockwise, `Equal` for fewer than three points. Exact: decided at its lowest-leftmost point,
/// which is a convex corner.
pub fn ring_orientation(ring: &[DVec2]) -> Ordering {
    let n = ring.len();
    if n < 3 {
        return Ordering::Equal;
    }
    let lowest = (0..n).min_by(|&i, &j| (ring[i].y, ring[i].x).partial_cmp(&(ring[j].y, ring[j].x)).unwrap_or(Ordering::Equal)).unwrap();
    orient2d(ring[(lowest + n - 1) % n], ring[lowest], ring[(lowest + 1) % n])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    #[test]
    fn orient2d_is_exact_where_plain_doubles_are_not() {
        // points on the line y = x, far out, nudged by one unit in the last place: the plain
        // determinant loses the sign, the adaptive predicate does not
        let a = v(0.5, 0.5);
        let b = v(12.0, 12.0);
        let c = v(24.0, 24.0);
        assert_eq!(orient2d(a, b, c), Ordering::Equal);
        let above = v(24.0, f64::from_bits(24.0f64.to_bits() + 1));
        assert_eq!(orient2d(a, b, above), Ordering::Greater);
        let below = v(24.0, f64::from_bits(24.0f64.to_bits() - 1));
        assert_eq!(orient2d(a, b, below), Ordering::Less);
    }

    #[test]
    fn orient2d_on_a_fine_grid_agrees_with_exact_rationals() {
        // Shewchuk's classic failure case for the plain determinant: a grid of tiny steps around a
        // point on a line, compared with the exact answer computed in i128 on scaled integers
        let scale = 2f64.powi(-53);
        let (a, b) = (v(12.0, 12.0), v(24.0, 24.0));
        for i in 0..64 {
            for j in 0..64 {
                let c = v(0.5 + i as f64 * scale, 0.5 + j as f64 * scale);
                // exact: sign of (b - a) x (c - a) with c = 0.5 + k·2⁻⁵³, all values multiples of 2⁻⁵³
                let unit = |x: f64| (x / scale) as i128;
                let (ax, ay, bx, by) = (unit(a.x), unit(a.y), unit(b.x), unit(b.y));
                let (cx, cy) = (unit(0.5) + i as i128, unit(0.5) + j as i128);
                let exact = ((bx - ax) * (cy - ay) - (by - ay) * (cx - ax)).cmp(&0);
                assert_eq!(orient2d(a, b, c), exact, "i {i}, j {j}");
            }
        }
    }

    #[test]
    fn orient3d_follows_the_right_hand_normal() {
        let (a, b, c) = (DVec3::ZERO, DVec3::X, DVec3::Y);
        assert_eq!(orient3d(a, b, c, DVec3::Z), Ordering::Greater);
        assert_eq!(orient3d(a, b, c, -DVec3::Z), Ordering::Less);
        assert_eq!(orient3d(a, b, c, DVec3::new(5.0, -3.0, 0.0)), Ordering::Equal);
    }

    #[test]
    fn segments_meet_when_they_cross_touch_or_overlap() {
        assert!(segments_meet(v(0.0, 0.0), v(2.0, 2.0), v(0.0, 2.0), v(2.0, 0.0)));
        assert!(segments_meet(v(0.0, 0.0), v(2.0, 0.0), v(2.0, 0.0), v(3.0, 1.0)), "touch at an end");
        assert!(segments_meet(v(0.0, 0.0), v(2.0, 0.0), v(1.0, 0.0), v(3.0, 0.0)), "overlap");
        assert!(!segments_meet(v(0.0, 0.0), v(2.0, 0.0), v(3.0, 0.0), v(4.0, 0.0)), "in line, apart");
        assert!(!segments_meet(v(0.0, 0.0), v(2.0, 0.0), v(0.0, 1.0), v(2.0, 1.0)), "parallel");
        assert!(!segments_meet(v(0.0, 0.0), v(1.0, 1.0), v(2.0, 0.0), v(1.5, 1.0)));
    }

    #[test]
    fn points_against_a_ring() {
        let square = [v(0.0, 0.0), v(2.0, 0.0), v(2.0, 2.0), v(0.0, 2.0)];
        assert_eq!(point_in_ring(v(1.0, 1.0), &square), Inside::Inside);
        assert_eq!(point_in_ring(v(3.0, 1.0), &square), Inside::Outside);
        assert_eq!(point_in_ring(v(2.0, 1.0), &square), Inside::Boundary);
        assert_eq!(point_in_ring(v(0.0, 0.0), &square), Inside::Boundary);
        let mut reversed = square;
        reversed.reverse();
        assert_eq!(point_in_ring(v(1.0, 1.0), &reversed), Inside::Inside);
    }

    #[test]
    fn ring_orientation_is_decided_exactly() {
        let ccw = [v(0.0, 0.0), v(2.0, 0.0), v(2.0, 2.0), v(0.0, 2.0)];
        assert_eq!(ring_orientation(&ccw), Ordering::Greater);
        let mut cw = ccw;
        cw.reverse();
        assert_eq!(ring_orientation(&cw), Ordering::Less);
        assert_eq!(ring_orientation(&[v(0.0, 0.0), v(1.0, 1.0)]), Ordering::Equal);
        // an L whose lowest-leftmost corner is not the first point
        let l = [v(2.0, 0.0), v(2.0, 1.0), v(1.0, 1.0), v(1.0, 2.0), v(0.0, 2.0), v(0.0, 0.0)];
        assert_eq!(ring_orientation(&l), Ordering::Greater);
    }
}
