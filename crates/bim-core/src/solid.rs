//! Planar solids: closed polyhedra with flat faces, which may have holes, in f64 tile-local
//! coordinates. This is the kernel's "planar first" representation, as Open BIM Studio's
//! `kernel/fast/`: walls with openings, slabs, members and panels are built directly, without
//! booleans. Every construction checks its result ([`Solid::check`]) before returning it.

use crate::profile::Profile;
use crate::{positive, Refusal, LINEAR_TOLERANCE};
use glam::{DVec2, DVec3};
use std::collections::HashMap;

/// A right-handed orthonormal frame: a profile drawn in its `x`–`y` plane is extruded along `z`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Axes {
    pub origin: DVec3,
    pub x: DVec3,
    pub y: DVec3,
    pub z: DVec3,
}

impl Axes {
    /// The frame at `origin` with `z` along `z` and `x` as close to `x` as it can be while square
    /// to `z`.
    pub fn new(origin: DVec3, x: DVec3, z: DVec3) -> Result<Self, Refusal> {
        let z = z.try_normalize().ok_or(Refusal::ZeroLength)?;
        let x = (x - z * x.dot(z)).try_normalize().ok_or(Refusal::Degenerate)?;
        Ok(Self { origin, x, y: z.cross(x), z })
    }

    /// A point of the profile plane in the frame's space.
    pub fn point(&self, p: DVec2) -> DVec3 {
        self.origin + self.x * p.x + self.y * p.y
    }
}

/// A plane through `point` with normal `normal`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plane {
    pub point: DVec3,
    pub normal: DVec3,
}

/// A face: its outer ring, then its holes, as indices into the solid's positions. Seen from
/// outside, the outer ring runs counter-clockwise and the holes clockwise.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    pub rings: Vec<Vec<u32>>,
}

/// A closed planar polyhedron.
#[derive(Debug, Clone, PartialEq)]
pub struct Solid {
    pub positions: Vec<DVec3>,
    pub faces: Vec<Face>,
}

impl Solid {
    /// `profile` in the `x`–`y` plane of `axes`, extruded `depth` along its `z`.
    pub fn extrude(profile: &Profile, axes: &Axes, depth: f64) -> Result<Self, Refusal> {
        if !positive(depth) {
            return Err(Refusal::NotPositive);
        }
        let start = Plane { point: axes.origin, normal: -axes.z };
        let end = Plane { point: axes.origin + axes.z * depth, normal: axes.z };
        Self::sweep_between(profile, axes, start, end)
    }

    /// `profile` in the `x`–`y` plane of `axes`, swept along its `z` from plane `start` to plane
    /// `end`: each corner of the profile runs along `z` from where its line meets `start` to where
    /// it meets `end`. The planes may be tilted (a mitre, a butt against a sloping face), but not
    /// along `z`, and they must not cross inside the profile.
    pub fn sweep_between(profile: &Profile, axes: &Axes, start: Plane, end: Plane) -> Result<Self, Refusal> {
        let along = |plane: &Plane, base: DVec3| -> Result<f64, Refusal> {
            let across = plane.normal.dot(axes.z);
            if across.abs() < 1e-9 * plane.normal.length().max(1e-300) {
                return Err(Refusal::EndParallel);
            }
            Ok(plane.normal.dot(plane.point - base) / across)
        };
        let mut positions = Vec::new();
        let mut rings = Vec::new();
        for ring in profile.rings() {
            let mut indices = Vec::with_capacity(ring.len());
            for &p in ring {
                let base = axes.point(p);
                let (t0, t1) = (along(&start, base)?, along(&end, base)?);
                if t1 - t0 <= LINEAR_TOLERANCE {
                    return Err(Refusal::EndsCross);
                }
                indices.push(positions.len() as u32);
                positions.push(base + axes.z * t0);
                positions.push(base + axes.z * t1);
            }
            rings.push(indices);
        }
        // bottom (index i) and top (i + 1) of each corner; the bottom face looks along −z, so its
        // rings run the other way
        let bottom = Face { rings: rings.iter().map(|r| r.iter().rev().copied().collect()).collect() };
        let top = Face { rings: rings.iter().map(|r| r.iter().map(|i| i + 1).collect()).collect() };
        let mut faces = vec![bottom, top];
        for ring in &rings {
            for k in 0..ring.len() {
                let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
                faces.push(Face { rings: vec![vec![a, b, b + 1, a + 1]] });
            }
        }
        let solid = Self { positions, faces };
        solid.check()?;
        Ok(solid)
    }

    /// Checks that the solid is closed and has volume: every directed edge of every ring has
    /// exactly one twin running the other way, and the volume is positive.
    pub fn check(&self) -> Result<(), Refusal> {
        let mut edges: HashMap<(u32, u32), i32> = HashMap::new();
        for face in &self.faces {
            for ring in &face.rings {
                if ring.len() < 3 || ring.iter().any(|&i| i as usize >= self.positions.len()) {
                    return Err(Refusal::NotClosed);
                }
                for k in 0..ring.len() {
                    let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
                    if a == b {
                        return Err(Refusal::NotClosed);
                    }
                    *edges.entry((a, b)).or_default() += 1;
                }
            }
        }
        let paired = edges.iter().all(|(&(a, b), &n)| n == 1 && edges.get(&(b, a)) == Some(&1));
        if !paired || self.volume() <= 0.0 {
            return Err(Refusal::NotClosed);
        }
        Ok(())
    }

    /// The volume (m³), by the divergence theorem over the faces' rings.
    pub fn volume(&self) -> f64 {
        let mut six = 0.0;
        for face in &self.faces {
            for ring in &face.rings {
                let r0 = self.positions[ring[0] as usize];
                for k in 1..ring.len().saturating_sub(1) {
                    let (a, b) = (self.positions[ring[k] as usize], self.positions[ring[k + 1] as usize]);
                    six += r0.dot(a.cross(b));
                }
            }
        }
        six / 6.0
    }

    /// The axis-aligned bounds.
    pub fn bounds(&self) -> (DVec3, DVec3) {
        self.positions.iter().fold((DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)), |(lo, hi), p| (lo.min(*p), hi.max(*p)))
    }

    /// The same solid moved by `offset` (between tile frames: an exact whole-metre offset).
    pub fn translated(&self, offset: DVec3) -> Self {
        Self { positions: self.positions.iter().map(|p| *p + offset).collect(), faces: self.faces.clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axes() -> Axes {
        Axes::new(DVec3::ZERO, DVec3::X, DVec3::Z).unwrap()
    }

    #[test]
    fn an_extruded_rectangle_is_a_closed_box() {
        let s = Solid::extrude(&Profile::rectangle(2.0, 3.0).unwrap(), &axes(), 4.0).unwrap();
        assert_eq!(s.positions.len(), 8);
        assert_eq!(s.faces.len(), 6);
        assert!((s.volume() - 24.0).abs() < 1e-12);
        let (lo, hi) = s.bounds();
        assert_eq!((lo, hi), (DVec3::new(-1.0, -1.5, 0.0), DVec3::new(1.0, 1.5, 4.0)));
    }

    #[test]
    fn a_profile_with_a_hole_extrudes_to_a_ring() {
        let ring = Profile::new(
            vec![DVec2::new(0.0, 0.0), DVec2::new(3.0, 0.0), DVec2::new(3.0, 3.0), DVec2::new(0.0, 3.0)],
            vec![vec![DVec2::new(1.0, 1.0), DVec2::new(2.0, 1.0), DVec2::new(2.0, 2.0), DVec2::new(1.0, 2.0)]],
        )
        .unwrap();
        let s = Solid::extrude(&ring, &axes(), 1.0).unwrap();
        assert!((s.volume() - 8.0).abs() < 1e-12);
        // a box with a hole through it: genus 1, so V − E + F = 0 with the cap faces counted
        // once per ring pair (8 + 8 corners, 16 side quads, two caps of two rings each)
        assert_eq!(s.faces.len(), 2 + 8);
    }

    #[test]
    fn a_sweep_between_tilted_planes_cuts_both_ends() {
        let a = axes();
        // a 45° cut at the end: the top corners run from z = 1 − y to 1 + y
        let end = Plane { point: DVec3::new(0.0, 0.0, 1.0), normal: DVec3::new(0.0, -1.0, 1.0) };
        let start = Plane { point: DVec3::ZERO, normal: -DVec3::Z };
        let s = Solid::sweep_between(&Profile::rectangle(0.2, 0.2).unwrap(), &a, start, end).unwrap();
        assert!((s.volume() - 0.2 * 0.2 * 1.0).abs() < 1e-12, "the cut takes off as much as it adds");
        let (lo, hi) = s.bounds();
        assert!((hi.z - 1.1).abs() < 1e-12 && lo.z == 0.0);
    }

    #[test]
    fn ends_that_cross_inside_the_member_are_refused() {
        let a = axes();
        let start = Plane { point: DVec3::new(0.0, 0.0, 0.05), normal: DVec3::new(0.0, 1.0, -1.0) };
        let end = Plane { point: DVec3::new(0.0, 0.0, 0.1), normal: DVec3::Z };
        assert_eq!(Solid::sweep_between(&Profile::rectangle(0.4, 0.4).unwrap(), &a, start, end), Err(Refusal::EndsCross));
        let along = Plane { point: DVec3::ZERO, normal: DVec3::X };
        assert_eq!(Solid::sweep_between(&Profile::rectangle(0.4, 0.4).unwrap(), &a, along, end), Err(Refusal::EndParallel));
    }

    #[test]
    fn the_check_finds_an_open_solid() {
        let mut s = Solid::extrude(&Profile::rectangle(1.0, 1.0).unwrap(), &axes(), 1.0).unwrap();
        s.faces.pop();
        assert_eq!(s.check(), Err(Refusal::NotClosed));
        let mut inside_out = Solid::extrude(&Profile::rectangle(1.0, 1.0).unwrap(), &axes(), 1.0).unwrap();
        for face in &mut inside_out.faces {
            for ring in &mut face.rings {
                ring.reverse();
            }
        }
        assert_eq!(inside_out.check(), Err(Refusal::NotClosed), "negative volume");
    }

    #[test]
    fn frames_are_square() {
        let a = Axes::new(DVec3::ONE, DVec3::new(1.0, 1.0, 0.0), DVec3::new(1.0, 0.0, 0.0)).unwrap();
        assert!(a.x.dot(a.z).abs() < 1e-15 && (a.y - a.z.cross(a.x)).length() < 1e-15);
        assert_eq!(Axes::new(DVec3::ZERO, DVec3::Z, DVec3::Z), Err(Refusal::Degenerate));
    }
}
