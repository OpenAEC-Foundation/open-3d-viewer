//! Display meshes: the triangles of a solid's faces, holes cut out, in the solid's tile frame.
//! Derived data (ADR-0013): a mesh never changes the element it shows. Positions stay f64 here;
//! the GPU's f32 belongs to the renderer, after the tile origin is taken off in f64.

use crate::solid::Solid;
use crate::Refusal;
use glam::DVec3;

/// Triangles with one normal per corner (faces are flat, so a face's corners share its normal).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<DVec3>,
    pub normals: Vec<DVec3>,
    pub indices: Vec<u32>,
}

impl Mesh {
    /// The triangles of `solid`, wound counter-clockwise seen from outside.
    pub fn of(solid: &Solid) -> Result<Self, Refusal> {
        let mut mesh = Mesh::default();
        for face in &solid.faces {
            let points: Vec<Vec<DVec3>> = face.rings.iter().map(|r| r.iter().map(|&i| solid.positions[i as usize]).collect()).collect();
            let normal = newell(&points[0]).try_normalize().ok_or(Refusal::NotTriangulated)?;
            // the face's plane: u, v square to the normal
            let u = normal.any_orthonormal_vector();
            let v = normal.cross(u);
            let mut flat = Vec::new();
            let mut holes = Vec::new();
            for (r, ring) in points.iter().enumerate() {
                if r > 0 {
                    holes.push(flat.len() / 2);
                }
                for p in ring {
                    flat.extend([p.dot(u), p.dot(v)]);
                }
            }
            let corners: Vec<DVec3> = points.iter().flatten().copied().collect();
            let triangles = earcutr::earcut(&flat, &holes, 2).map_err(|_| Refusal::NotTriangulated)?;
            if triangles.is_empty() {
                return Err(Refusal::NotTriangulated);
            }
            let first = mesh.positions.len() as u32;
            mesh.positions.extend(&corners);
            mesh.normals.extend(std::iter::repeat_n(normal, corners.len()));
            for t in triangles.chunks_exact(3) {
                let (a, b, c) = (corners[t[0]], corners[t[1]], corners[t[2]]);
                let flip = (b - a).cross(c - a).dot(normal) < 0.0;
                let (i, j, k) = (first + t[0] as u32, first + t[1] as u32, first + t[2] as u32);
                mesh.indices.extend(if flip { [i, k, j] } else { [i, j, k] });
            }
        }
        Ok(mesh)
    }

    /// Adds `other`'s triangles.
    pub fn append(&mut self, other: &Mesh) {
        let first = self.positions.len() as u32;
        self.positions.extend(&other.positions);
        self.normals.extend(&other.normals);
        self.indices.extend(other.indices.iter().map(|i| i + first));
    }

    /// The area of the triangles (m²).
    pub fn area(&self) -> f64 {
        self.indices
            .chunks_exact(3)
            .map(|t| {
                let (a, b, c) = (self.positions[t[0] as usize], self.positions[t[1] as usize], self.positions[t[2] as usize]);
                (b - a).cross(c - a).length() / 2.0
            })
            .sum()
    }
}

/// The normal of a planar polygon by Newell's method (its length is twice the area).
fn newell(ring: &[DVec3]) -> DVec3 {
    let n = ring.len();
    (0..n).fold(DVec3::ZERO, |acc, i| {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        acc + DVec3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec2;
    use crate::profile::Profile;
    use crate::solid::Axes;

    #[test]
    fn a_box_meshes_to_twelve_outward_triangles() {
        let s = Solid::extrude(&Profile::rectangle(1.0, 2.0).unwrap(), &Axes::new(DVec3::ZERO, DVec3::X, DVec3::Z).unwrap(), 3.0).unwrap();
        let m = Mesh::of(&s).unwrap();
        assert_eq!(m.indices.len(), 36);
        assert!((m.area() - 2.0 * (1.0 * 2.0 + 1.0 * 3.0 + 2.0 * 3.0)).abs() < 1e-12);
        let centre = DVec3::new(0.0, 0.0, 1.5);
        for t in m.indices.chunks_exact(3) {
            let (a, b, c) = (m.positions[t[0] as usize], m.positions[t[1] as usize], m.positions[t[2] as usize]);
            let n = (b - a).cross(c - a);
            assert!(n.dot(a - centre) > 0.0, "every triangle faces out");
            assert!(n.normalize().dot(m.normals[t[0] as usize]) > 0.999);
        }
    }

    #[test]
    fn holes_are_left_open() {
        let ring = Profile::new(
            vec![DVec2::new(0.0, 0.0), DVec2::new(3.0, 0.0), DVec2::new(3.0, 3.0), DVec2::new(0.0, 3.0)],
            vec![vec![DVec2::new(1.0, 1.0), DVec2::new(2.0, 1.0), DVec2::new(2.0, 2.0), DVec2::new(1.0, 2.0)]],
        )
        .unwrap();
        let s = Solid::extrude(&ring, &Axes::new(DVec3::ZERO, DVec3::X, DVec3::Z).unwrap(), 1.0).unwrap();
        let m = Mesh::of(&s).unwrap();
        // two caps of 8 m², outer sides 12 m², hole sides 4 m²
        assert!((m.area() - (16.0 + 12.0 + 4.0)).abs() < 1e-12, "{}", m.area());
    }
}
