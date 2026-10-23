//! The neutral scene model every loader produces: Z-up, metres (or file units), one
//! triangle mesh per layer.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// One layer: a named triangle mesh with a single base colour (plus optional vertex colours).
#[derive(Debug, Clone)]
pub struct Layer {
    pub name: String,
    /// Base colour (sRGB, 0..1).
    pub color: [f32; 3],
    /// xyz per vertex, Z up.
    pub positions: Vec<f32>,
    /// xyz per vertex, unit length; same length as `positions`.
    pub normals: Vec<f32>,
    /// Optional per-vertex sRGB colours (RGBA8); overrides `color` when present.
    pub colors: Option<Vec<[u8; 4]>>,
    /// Triangle list.
    pub indices: Vec<u32>,
}

impl Layer {
    pub fn vertex_count(&self) -> usize {
        self.positions.len() / 3
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Bounds {
    pub fn empty() -> Self {
        Self { min: [f32::INFINITY; 3], max: [f32::NEG_INFINITY; 3] }
    }

    pub fn is_valid(&self) -> bool {
        (0..3).all(|i| self.min[i].is_finite() && self.max[i].is_finite() && self.min[i] <= self.max[i])
    }

    pub fn extend(&mut self, p: [f32; 3]) {
        for i in 0..3 {
            self.min[i] = self.min[i].min(p[i]);
            self.max[i] = self.max[i].max(p[i]);
        }
    }

    pub fn center(&self) -> [f32; 3] {
        [0, 1, 2].map(|i| 0.5 * (self.min[i] + self.max[i]))
    }

    pub fn size(&self) -> [f32; 3] {
        [0, 1, 2].map(|i| self.max[i] - self.min[i])
    }

    pub fn radius(&self) -> f32 {
        let s = self.size();
        0.5 * (s[0] * s[0] + s[1] * s[1] + s[2] * s[2]).sqrt()
    }
}

/// A loaded model.
#[derive(Debug)]
pub struct Model {
    /// Unique id, used by the GPU cache to know when to upload.
    pub id: u64,
    pub path: PathBuf,
    pub format: String,
    pub file_size: u64,
    pub load_time: Duration,
    pub layers: Vec<Layer>,
    pub bounds: Bounds,
    /// The file was Y up and the loader turned it to Z up.
    pub source_y_up: bool,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

impl Model {
    /// Builds a model from layers: drops empty layers and computes the bounds.
    pub fn new(path: PathBuf, format: impl Into<String>, mut layers: Vec<Layer>) -> Self {
        layers.retain(|l| !l.indices.is_empty() && !l.positions.is_empty());
        let mut bounds = Bounds::empty();
        for layer in &layers {
            for p in layer.positions.chunks_exact(3) {
                if p.iter().all(|v| v.is_finite()) {
                    bounds.extend([p[0], p[1], p[2]]);
                }
            }
        }
        let file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            path,
            format: format.into(),
            file_size,
            load_time: Duration::ZERO,
            layers,
            bounds,
            source_y_up: false,
        }
    }

    pub fn file_name(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }

    pub fn vertex_count(&self) -> usize {
        self.layers.iter().map(Layer::vertex_count).sum()
    }

    pub fn triangle_count(&self) -> usize {
        self.layers.iter().map(Layer::triangle_count).sum()
    }
}

/// Converts Y-up coordinates (x, y, z) to Z-up (x, -z, y), in place, for xyz triples.
pub fn y_up_to_z_up(values: &mut [f32]) {
    for v in values.chunks_exact_mut(3) {
        let (y, z) = (v[1], v[2]);
        v[1] = -z;
        v[2] = y;
    }
}

/// Splits an indexed triangle mesh into unshared vertices with flat face normals.
pub fn flat_shaded(positions: &[f32], indices: &[u32]) -> (Vec<f32>, Vec<f32>, Vec<u32>) {
    let mut out_p = Vec::with_capacity(indices.len() * 3);
    let mut out_n = Vec::with_capacity(indices.len() * 3);
    let vertex_count = (positions.len() / 3) as u32;
    for tri in indices.chunks_exact(3) {
        if tri.iter().any(|&i| i >= vertex_count) {
            continue;
        }
        let p = [tri[0], tri[1], tri[2]].map(|i| {
            let i = i as usize * 3;
            glam::Vec3::new(positions[i], positions[i + 1], positions[i + 2])
        });
        let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
        for v in p {
            out_p.extend_from_slice(&[v.x, v.y, v.z]);
            out_n.extend_from_slice(&[n.x, n.y, n.z]);
        }
    }
    let count = (out_p.len() / 3) as u32;
    (out_p, out_n, (0..count).collect())
}

/// Smooth vertex normals (area weighted) for an indexed mesh.
pub fn smooth_normals(positions: &[f32], indices: &[u32]) -> Vec<f32> {
    let n = positions.len() / 3;
    let mut normals = vec![glam::Vec3::ZERO; n];
    let at = |i: u32| {
        let i = i as usize * 3;
        glam::Vec3::new(positions[i], positions[i + 1], positions[i + 2])
    };
    for tri in indices.chunks_exact(3) {
        if tri.iter().any(|&i| i as usize >= n) {
            continue;
        }
        let face = (at(tri[1]) - at(tri[0])).cross(at(tri[2]) - at(tri[0]));
        for &i in tri {
            normals[i as usize] += face;
        }
    }
    normals.iter().flat_map(|v| v.normalize_or(glam::Vec3::Z).to_array()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn y_up_conversion() {
        let mut v = vec![1.0, 2.0, 3.0];
        y_up_to_z_up(&mut v);
        assert_eq!(v, vec![1.0, -3.0, 2.0]);
    }

    #[test]
    fn flat_normals_of_xy_triangle_point_up() {
        let (p, n, i) = flat_shaded(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], &[0, 1, 2]);
        assert_eq!(p.len(), 9);
        assert_eq!(i, vec![0, 1, 2]);
        assert_eq!(&n[0..3], &[0.0, 0.0, 1.0]);
    }
}
