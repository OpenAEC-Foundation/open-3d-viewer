//! STL (ASCII and binary), flat shaded.

use super::{Loader, DEFAULT_COLOR};
use crate::model::Layer;
use std::path::Path;

pub struct StlLoader;

impl Loader for StlLoader {
    fn name(&self) -> &'static str {
        "STL"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["stl"]
    }

    fn load(&self, path: &Path) -> Result<Vec<Layer>, String> {
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut reader = std::io::BufReader::new(file);
        let triangles = stl_io::create_stl_reader(&mut reader).map_err(|e| format!("STL: {e}"))?;
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        for t in triangles {
            let t = t.map_err(|e| format!("STL: {e}"))?;
            let v = t.vertices.map(|v| glam::Vec3::from_array(v.0));
            let mut n = (v[1] - v[0]).cross(v[2] - v[0]).normalize_or_zero();
            if n == glam::Vec3::ZERO {
                n = glam::Vec3::from_array(t.normal.0).normalize_or(glam::Vec3::Z);
            }
            for p in v {
                positions.extend_from_slice(&p.to_array());
                normals.extend_from_slice(&n.to_array());
            }
        }
        let count = (positions.len() / 3) as u32;
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(vec![Layer { name, color: DEFAULT_COLOR, positions, normals, colors: None, indices: (0..count).collect() }])
    }
}
