//! Member frames.
//!
//! x runs from start to end (the curve tangent for curved members). z is the resolved reference
//! vector made perpendicular to x, `z = normalize(z_ref − (z_ref·x) x)`, and `y = z × x`, so
//! (x, y, z) is right-handed. Profile u maps to local y and profile v to local z.

use glam::{DMat3, DVec3};

use crate::model::Curve;

/// Below this length (m) a member has no frame.
pub const MIN_LENGTH: f64 = 1e-9;
/// `z_ref` within this relative amount of being parallel to x has no perpendicular part.
pub const PARALLEL_EPS: f64 = 1e-9;
/// Members within this angle (rad) of vertical use the vertical default rule.
pub const VERTICAL_TOLERANCE: f64 = 1e-3;

/// A member's local frame at a point of its axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MemberFrame {
    pub origin: DVec3,
    pub x: DVec3,
    pub y: DVec3,
    pub z: DVec3,
    /// Chord length for straight members; 0 for frames built from a tangent.
    pub length: f64,
}

/// Why a frame could not be built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    ZeroLength,
    /// `z_ref` is (anti)parallel to the member axis, or zero.
    ZParallelToAxis,
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            FrameError::ZeroLength => "zero-length member",
            FrameError::ZParallelToAxis => "local z reference parallel to the member axis",
        })
    }
}

impl std::error::Error for FrameError {}

impl MemberFrame {
    /// Columns x, y, z: local to global rotation.
    pub fn rotation(&self) -> DMat3 {
        DMat3::from_cols(self.x, self.y, self.z)
    }

    /// A local vector in global axes.
    pub fn vector_to_global(&self, local: DVec3) -> DVec3 {
        self.x * local.x + self.y * local.y + self.z * local.z
    }

    /// A local point (relative to `origin`) in global coordinates.
    pub fn point_to_global(&self, local: DVec3) -> DVec3 {
        self.origin + self.vector_to_global(local)
    }

    /// A global vector in local axes.
    pub fn vector_to_local(&self, global: DVec3) -> DVec3 {
        DVec3::new(global.dot(self.x), global.dot(self.y), global.dot(self.z))
    }
}

/// The frame of a straight member from `p0` to `p1` with reference vector `z_ref`
/// (re-orthogonalised against the axis).
pub fn member_frame(p0: DVec3, p1: DVec3, z_ref: DVec3) -> Result<MemberFrame, FrameError> {
    let d = p1 - p0;
    let length = d.length();
    if !(length >= MIN_LENGTH) {
        return Err(FrameError::ZeroLength);
    }
    let mut frame = frame_from_tangent(p0, d / length, z_ref)?;
    frame.length = length;
    Ok(frame)
}

/// The frame at `origin` for a unit (or any non-zero) tangent.
pub fn frame_from_tangent(origin: DVec3, tangent: DVec3, z_ref: DVec3) -> Result<MemberFrame, FrameError> {
    let x = tangent.normalize_or_zero();
    if x == DVec3::ZERO {
        return Err(FrameError::ZeroLength);
    }
    let scale = z_ref.length();
    let perp = z_ref - z_ref.dot(x) * x;
    let n = perp.length();
    if !(scale > 0.0) || !(n > PARALLEL_EPS * scale) {
        return Err(FrameError::ZParallelToAxis);
    }
    let z = perp / n;
    let y = z.cross(x);
    Ok(MemberFrame { origin, x, y, z, length: 0.0 })
}

/// The default local z reference when a source gives none: for non-vertical members, y
/// horizontal and z upward in the vertical plane through the axis; for members within
/// [`VERTICAL_TOLERANCE`] of vertical, y along global Y (so z = x × Y).
pub fn default_z_ref(x: DVec3) -> DVec3 {
    let x = x.normalize_or_zero();
    if x.z.abs() >= VERTICAL_TOLERANCE.cos() {
        x.cross(DVec3::Y).normalize_or_zero()
    } else {
        (DVec3::Z - x.z * x).normalize_or_zero()
    }
}

/// The circle through three points: centre, unit normal (right-handed with the order a, b, c)
/// and radius. `None` when the points are collinear.
pub fn circle_through(a: DVec3, b: DVec3, c: DVec3) -> Option<(DVec3, DVec3, f64)> {
    let ab = b - a;
    let ac = c - a;
    let n = ab.cross(ac);
    let nn = n.length_squared();
    let scale = ab.length_squared().max(ac.length_squared());
    if !(nn > 1e-24 * scale * scale) {
        return None;
    }
    let centre = a + (ac.length_squared() * n.cross(ab) + ab.length_squared() * ac.cross(n)) / (2.0 * nn);
    Some((centre, n / nn.sqrt(), (a - centre).length()))
}

/// Unit tangent of the axis at the start node.
pub fn start_tangent(p0: DVec3, p1: DVec3, curve: &Curve) -> Option<DVec3> {
    let t = match curve {
        Curve::Line => p1 - p0,
        Curve::Polyline(pts) => pts.first().copied().unwrap_or(p1) - p0,
        Curve::Arc { mid } => {
            let (centre, normal, _) = circle_through(p0, *mid, p1)?;
            normal.cross(p0 - centre)
        }
    };
    let t = t.normalize_or_zero();
    (t != DVec3::ZERO).then_some(t)
}

/// Length of the axis between the end nodes.
pub fn curve_length(p0: DVec3, p1: DVec3, curve: &Curve) -> f64 {
    match curve {
        Curve::Line => (p1 - p0).length(),
        Curve::Polyline(pts) => {
            let mut prev = p0;
            let mut len = 0.0;
            for p in pts.iter().copied().chain(std::iter::once(p1)) {
                len += (p - prev).length();
                prev = p;
            }
            len
        }
        Curve::Arc { mid } => match circle_through(p0, *mid, p1) {
            Some((centre, normal, r)) => {
                let a = p0 - centre;
                let angle = |v: DVec3| {
                    let s = normal.dot(a.cross(v));
                    let c = a.dot(v);
                    let t = s.atan2(c);
                    if t < 0.0 {
                        t + std::f64::consts::TAU
                    } else {
                        t
                    }
                };
                // Sweep from start to end through mid, counter-clockwise about `normal`.
                r * angle(p1 - centre)
            }
            None => (p1 - p0).length(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: DVec3, b: DVec3) -> bool {
        (a - b).length() < 1e-12
    }

    #[test]
    fn horizontal_member_default() {
        let f = member_frame(DVec3::ZERO, DVec3::new(5.0, 0.0, 0.0), default_z_ref(DVec3::X)).unwrap();
        assert!(close(f.x, DVec3::X) && close(f.z, DVec3::Z) && close(f.y, DVec3::Y));
        assert_eq!(f.length, 5.0);
        // A sloping member keeps z upward in its vertical plane and y horizontal.
        let x = DVec3::new(3.0, 4.0, 2.0).normalize();
        let f = member_frame(DVec3::ZERO, x * 2.0, default_z_ref(x)).unwrap();
        assert!(f.y.z.abs() < 1e-12 && f.z.z > 0.0 && f.z.dot(f.x).abs() < 1e-12);
    }

    #[test]
    fn vertical_rule() {
        let up = member_frame(DVec3::ZERO, DVec3::new(0.0, 0.0, 4.0), default_z_ref(DVec3::Z)).unwrap();
        assert!(close(up.y, DVec3::Y) && close(up.z, -DVec3::X), "{up:?}");
        let down = member_frame(DVec3::new(0.0, 0.0, 4.0), DVec3::ZERO, default_z_ref(-DVec3::Z)).unwrap();
        assert!(close(down.y, DVec3::Y) && close(down.z, DVec3::X), "{down:?}");
        // Within 1e-3 rad of vertical: still the vertical rule.
        let tilt = DVec3::new(0.9e-3, 0.0, 1.0).normalize();
        let f = frame_from_tangent(DVec3::ZERO, tilt, default_z_ref(tilt)).unwrap();
        assert!(f.y.dot(DVec3::Y) > 1.0 - 1e-6);
        // Beyond it: z upward.
        let tilt = DVec3::new(1.1e-3, 0.0, 1.0).normalize();
        let f = frame_from_tangent(DVec3::ZERO, tilt, default_z_ref(tilt)).unwrap();
        assert!(f.z.z > 0.0 && f.y.z.abs() < 1e-12);
    }

    #[test]
    fn re_orthogonalisation() {
        let p1 = DVec3::new(2.0, 1.0, 0.5);
        let z_ref = DVec3::new(0.3, -0.2, 2.0);
        let f = member_frame(DVec3::ZERO, p1, z_ref).unwrap();
        assert!((f.z.length() - 1.0).abs() < 1e-15);
        assert!(f.z.dot(f.x).abs() < 1e-15 && f.y.dot(f.x).abs() < 1e-15 && f.y.dot(f.z).abs() < 1e-15);
        assert!((f.rotation().determinant() - 1.0).abs() < 1e-14);
        assert!(f.z.dot(z_ref) > 0.0);
        let local = f.vector_to_local(DVec3::new(1.0, 2.0, 3.0));
        assert!(close(f.vector_to_global(local), DVec3::new(1.0, 2.0, 3.0)));
        assert_eq!(member_frame(DVec3::ZERO, p1, p1 * 3.0), Err(FrameError::ZParallelToAxis));
        assert_eq!(member_frame(p1, p1, DVec3::Z), Err(FrameError::ZeroLength));
    }

    #[test]
    fn arc_length_and_tangent() {
        let p0 = DVec3::new(1.0, 0.0, 0.0);
        let mid = DVec3::new(0.0, 1.0, 0.0);
        let p1 = DVec3::new(-1.0, 0.0, 0.0);
        let arc = Curve::Arc { mid };
        assert!((curve_length(p0, p1, &arc) - std::f64::consts::PI).abs() < 1e-12);
        assert!(close(start_tangent(p0, p1, &arc).unwrap(), DVec3::Y));
        let poly = Curve::Polyline(vec![DVec3::new(1.0, 1.0, 0.0)]);
        assert!((curve_length(p0, DVec3::new(2.0, 1.0, 0.0), &poly) - 2.0).abs() < 1e-12);
    }
}
