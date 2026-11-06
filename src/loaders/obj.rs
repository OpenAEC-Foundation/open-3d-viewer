//! Wavefront OBJ (+ MTL diffuse colours); one layer per object/group. Treated as Y up. A file
//! with vertices and no faces (as scanners and photogrammetry export point clouds) is a point
//! cloud: its `v x y z [r g b]` lines become points in their colour, else coloured by height,
//! thinned out to [`MAX_POINTS`](super::pointcloud::MAX_POINTS) and moved to a local origin when
//! far from zero. Lines (`l`) are not read.

use super::pointcloud::{color_by_height, Cloud, MAX_POINTS};
use super::xyz::lines;
use super::{Loaded, Loader, Phase, Progress, DEFAULT_COLOR, Source};
use crate::model::{flat_shaded, y_up_to_z_up, Layer, ProjectData};

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

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let layers = self.load(source)?;
        if layers.iter().any(|l| !l.indices.is_empty()) {
            return Ok(Loaded { layers, views: Vec::new(), project: ProjectData::default() });
        }
        // no faces: the vertices are a point cloud
        progress.set(Phase::Convert);
        let data = source.read()?;
        let (layers, info) = points(&data, &source.stem(), MAX_POINTS);
        Ok(Loaded { layers, views: Vec::new(), project: ProjectData { info, ..ProjectData::default() } })
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        let options = tobj::LoadOptions { triangulate: true, single_index: true, ..Default::default() };
        let (models, materials) = match source.path() {
            // as tobj::load_obj, with the material libraries checked as references
            // (`super::check_reference`): one that may not be read leaves the model grey
            Some(path) => {
                let file = std::fs::File::open(path).map_err(|e| format!("OBJ: {e}"))?;
                tobj::load_obj_buf(&mut std::io::BufReader::new(file), &options, |mtl| {
                    let mtl = path.parent().map_or_else(|| mtl.to_path_buf(), |dir| dir.join(mtl));
                    super::check_reference(path, &mtl).map_err(|_| tobj::LoadError::OpenFileFailed)?;
                    tobj::load_mtl(mtl)
                })
            }
            // bytes from the browser: no side files, so no .mtl colours
            None => {
                let data = source.read()?;
                tobj::load_obj_buf(&mut std::io::Cursor::new(data), &options, |_| Err(tobj::LoadError::OpenFileFailed))
            }
        }
        .map_err(|e| format!("OBJ: {e}"))?;
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
                Layer { name: m.name, color, positions, normals, colors, indices, ..Layer::default() }
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

/// The `v` lines of an OBJ file as a point cloud (Y up turned to Z up), keeping at most `max`.
fn points(data: &[u8], stem: &str, max: usize) -> (Vec<Layer>, Vec<(String, String)>) {
    let vertex = |l: &&[u8]| l.starts_with(b"v ") || l.starts_with(b"v	");
    let total = lines(data).filter(vertex).count() as u64;
    let mut cloud = Cloud::new(total, max);
    let mut colored = false;
    for line in lines(data).filter(vertex) {
        if !cloud.keep() {
            continue;
        }
        let text = std::str::from_utf8(&line[2..]).unwrap_or("");
        let v: Vec<f64> = text.split_ascii_whitespace().map_while(|w| w.parse().ok()).collect();
        if v.len() < 3 {
            continue;
        }
        // `v x y z r g b`: colours 0..1 (or 0..255)
        let color = if v.len() >= 6 {
            colored = true;
            let scale = if v[3..6].iter().any(|&c| c > 1.0) { 1.0 } else { 255.0 };
            [v[3], v[4], v[5]].map(|c| (c * scale).round().clamp(0.0, 255.0) as u8)
        } else {
            [0; 3]
        };
        let layer = cloud.layer(0, || Layer { name: stem.to_string(), color: [0.80, 0.80, 0.82], ..Layer::default() });
        cloud.push(layer, [v[0], -v[2], v[1]], color);
    }
    let mut info = cloud.info("OBJ", total);
    let mut layers = cloud.into_layers(false);
    if !colored {
        color_by_height(&mut layers);
        info.push(("Point colours".into(), "By height (the file has no colours)".into()));
    }
    (layers, info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn vertices_without_faces_are_a_point_cloud() {
        let text = "# points
v 0 0 0 1 0 0
v 1 2 3 0 1 0
vn 0 1 0
";
        let source = Source::Bytes { name: "scan.obj".into(), data: Arc::new(text.as_bytes().to_vec()) };
        let model = crate::loaders::load_with(&source, &Progress::default()).expect("OBJ points");
        assert_eq!((model.point_count(), model.triangle_count()), (2, 0));
        let layer = &model.layers[0];
        assert_eq!(layer.name, "scan");
        // Y up: (1, 2, 3) becomes (1, -3, 2)
        assert_eq!(&layer.positions[3..], &[1.0, -3.0, 2.0]);
        assert_eq!(layer.colors.as_ref().unwrap(), &[[255, 0, 0, 255], [0, 255, 0, 255]]);
    }
}
