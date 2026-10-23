//! Wavefront OBJ (+ MTL diffuse colours); one layer per object/group. Treated as Y up.

use super::{Loader, DEFAULT_COLOR};
use crate::model::{flat_shaded, y_up_to_z_up, Layer};
use std::path::Path;

pub struct ObjLoader;

impl Loader for ObjLoader {
    fn name(&self) -> &'static str {
        "OBJ"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["obj"]
    }

    fn source_y_up(&self) -> bool {
        true
    }

    fn load(&self, path: &Path) -> Result<Vec<Layer>, String> {
        let options = tobj::LoadOptions { triangulate: true, single_index: true, ..Default::default() };
        let (models, materials) = tobj::load_obj(path, &options).map_err(|e| format!("OBJ: {e}"))?;
        let materials = materials.unwrap_or_default();
        Ok(models
            .into_iter()
            .map(|m| {
                let mesh = m.mesh;
                let color = mesh
                    .material_id
                    .and_then(|i| materials.get(i))
                    .and_then(|mat| mat.diffuse)
                    .unwrap_or(DEFAULT_COLOR);
                let has_normals = mesh.normals.len() == mesh.positions.len();
                let has_colors = mesh.vertex_color.len() == mesh.positions.len();
                let (positions, normals, indices, colors) = if has_normals {
                    let colors = has_colors.then(|| to_rgba8(&mesh.vertex_color));
                    (mesh.positions, mesh.normals, mesh.indices, colors)
                } else {
                    // Faceted look for files without normals, like the three.js loader
                    let (p, n, i) = flat_shaded(&mesh.positions, &mesh.indices);
                    let colors = has_colors.then(|| {
                        let c = to_rgba8(&mesh.vertex_color);
                        mesh.indices.iter().map(|&i| c[i as usize]).collect()
                    });
                    (p, n, i, colors)
                };
                let (mut positions, mut normals) = (positions, normals);
                // OBJ is Y up by convention (Blender, 3ds Max and most exporters)
                y_up_to_z_up(&mut positions);
                y_up_to_z_up(&mut normals);
                Layer { name: m.name, color, positions, normals, colors, indices }
            })
            .collect())
    }
}

fn to_rgba8(rgb: &[f32]) -> Vec<[u8; 4]> {
    rgb.chunks_exact(3)
        .map(|c| [c[0], c[1], c[2]].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8))
        .map(|[r, g, b]| [r, g, b, 255])
        .collect()
}
