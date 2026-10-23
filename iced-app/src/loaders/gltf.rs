//! glTF 2.0 (.gltf / .glb). The default scene is flattened to world space and grouped into one
//! layer per material. glTF is Y up, metres; converted to Z up. Base colour = material factor
//! times the average colour of its base-colour texture (no texture mapping yet).

use super::{Loader, DEFAULT_COLOR};
use crate::model::{flat_shaded, y_up_to_z_up, Layer};
use glam::{Mat3, Mat4, Vec3};
use std::collections::BTreeMap;
use std::path::Path;

pub struct GltfLoader;

impl Loader for GltfLoader {
    fn name(&self) -> &'static str {
        "glTF"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["gltf", "glb"]
    }

    fn source_y_up(&self) -> bool {
        true
    }

    fn load(&self, path: &Path) -> Result<Vec<Layer>, String> {
        let (doc, buffers, images) = gltf::import(path).map_err(|e| format!("glTF: {e}"))?;
        let texture_average: Vec<[f32; 4]> = images.iter().map(average_color).collect();

        // material index (None = default material) -> layer under construction
        let mut layers: BTreeMap<Option<usize>, Layer> = BTreeMap::new();

        let scene = doc.default_scene().or_else(|| doc.scenes().next());
        let roots: Vec<gltf::Node> = match &scene {
            Some(s) => s.nodes().collect(),
            None => doc.nodes().collect(),
        };
        let mut stack: Vec<(gltf::Node, Mat4)> = roots.into_iter().map(|n| (n, Mat4::IDENTITY)).collect();
        while let Some((node, parent)) = stack.pop() {
            let world = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
            if let Some(mesh) = node.mesh() {
                for prim in mesh.primitives() {
                    add_primitive(&prim, world, &buffers, &texture_average, &mut layers);
                }
            }
            for child in node.children() {
                stack.push((child, world));
            }
        }

        let mut out: Vec<Layer> = layers.into_values().collect();
        for layer in &mut out {
            y_up_to_z_up(&mut layer.positions);
            y_up_to_z_up(&mut layer.normals);
        }
        Ok(out)
    }
}

fn add_primitive(
    prim: &gltf::Primitive,
    world: Mat4,
    buffers: &[gltf::buffer::Data],
    texture_average: &[[f32; 4]],
    layers: &mut BTreeMap<Option<usize>, Layer>,
) {
    if prim.mode() != gltf::mesh::Mode::Triangles {
        return; // points, lines and strips are not shown yet
    }
    let reader = prim.reader(|b| buffers.get(b.index()).map(|d| &d.0[..]));
    let Some(positions) = reader.read_positions() else { return };
    let positions: Vec<[f32; 3]> = positions.collect();
    let indices: Vec<u32> = match reader.read_indices() {
        Some(i) => i.into_u32().collect(),
        None => (0..positions.len() as u32).collect(),
    };
    let normals: Option<Vec<[f32; 3]>> = reader.read_normals().map(|n| n.collect());
    let colors: Option<Vec<[f32; 4]>> = reader.read_colors(0).map(|c| c.into_rgba_f32().collect());

    let material = prim.material();
    let pbr = material.pbr_metallic_roughness();
    let mut base = pbr.base_color_factor();
    if let Some(tex) = pbr.base_color_texture() {
        if let Some(avg) = texture_average.get(tex.texture().source().index()) {
            for i in 0..4 {
                base[i] *= avg[i];
            }
        }
    }
    let srgb = [linear_to_srgb(base[0]), linear_to_srgb(base[1]), linear_to_srgb(base[2])];

    // World transform; normals with the inverse transpose
    let normal_matrix = Mat3::from_mat4(world).inverse().transpose();
    let world_pos: Vec<f32> = positions
        .iter()
        .flat_map(|p| world.transform_point3(Vec3::from_array(*p)).to_array())
        .collect();

    let (pos, nor, idx, vertex_map): (Vec<f32>, Vec<f32>, Vec<u32>, Option<Vec<u32>>) = match normals {
        Some(n) if n.len() == positions.len() => {
            let nor = n
                .iter()
                .flat_map(|v| (normal_matrix * Vec3::from_array(*v)).normalize_or(Vec3::Y).to_array())
                .collect();
            (world_pos, nor, indices, None)
        }
        _ => {
            let (p, n, i) = flat_shaded(&world_pos, &indices);
            (p, n, i, Some(indices.clone()))
        }
    };

    let layer = layers.entry(material.index()).or_insert_with(|| Layer {
        name: material
            .name()
            .map(str::to_owned)
            .unwrap_or_else(|| match material.index() {
                Some(i) => format!("Material {i}"),
                None => "Default".to_owned(),
            }),
        color: if material.index().is_some() { srgb } else { DEFAULT_COLOR },
        positions: Vec::new(),
        normals: Vec::new(),
        colors: None,
        indices: Vec::new(),
    });

    let offset = (layer.positions.len() / 3) as u32;
    let new_vertices = pos.len() / 3;

    // Vertex colours: keep per layer once any primitive has them
    let prim_colors: Option<Vec<[u8; 4]>> = colors.map(|c| {
        let to8 = |c: [f32; 4]| {
            let s = |v: f32| (linear_to_srgb(v * 1.0).clamp(0.0, 1.0) * 255.0).round() as u8;
            [s(c[0] * base[0]), s(c[1] * base[1]), s(c[2] * base[2]), 255]
        };
        match &vertex_map {
            Some(map) => map.iter().map(|&i| to8(c.get(i as usize).copied().unwrap_or([1.0; 4]))).collect(),
            None => c.into_iter().map(to8).collect(),
        }
    });
    let layer_color8 = layer.color.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    let fill = [layer_color8[0], layer_color8[1], layer_color8[2], 255];
    match (&mut layer.colors, prim_colors) {
        (Some(existing), Some(c)) => existing.extend(c),
        (Some(existing), None) => existing.extend(std::iter::repeat_n(fill, new_vertices)),
        (None, Some(c)) => {
            let mut all = vec![fill; offset as usize];
            all.extend(c);
            layer.colors = Some(all);
        }
        (None, None) => {}
    }

    layer.positions.extend_from_slice(&pos);
    layer.normals.extend_from_slice(&nor);
    layer.indices.extend(idx.into_iter().map(|i| i + offset));
}

fn linear_to_srgb(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.040_45 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// Average linear colour of an image (sampled), so textured materials get a plausible tint.
fn average_color(image: &gltf::image::Data) -> [f32; 4] {
    use gltf::image::Format;
    let channels = match image.format {
        Format::R8 => 1,
        Format::R8G8 => 2,
        Format::R8G8B8 => 3,
        Format::R8G8B8A8 => 4,
        _ => return [1.0; 4],
    };
    let pixels = image.pixels.len() / channels;
    if pixels == 0 {
        return [1.0; 4];
    }
    let step = (pixels / 65_536).max(1);
    let mut sum = [0.0f64; 3];
    let mut n = 0.0f64;
    for p in (0..pixels).step_by(step) {
        let px = &image.pixels[p * channels..p * channels + channels];
        for c in 0..3 {
            let v = px[c.min(channels - 1)] as f32 / 255.0;
            sum[c] += srgb_to_linear(v) as f64;
        }
        n += 1.0;
    }
    [(sum[0] / n) as f32, (sum[1] / n) as f32, (sum[2] / n) as f32, 1.0]
}

#[cfg(test)]
mod tests {
    /// A minimal glTF with one Y-up triangle standing in the XY plane (embedded base64 buffer).
    #[test]
    fn loads_embedded_triangle_and_converts_to_z_up() {
        // positions (0,0,0) (1,0,0) (0,1,0) as f32
        let mut bytes = Vec::new();
        for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        let b64 = base64_encode(&bytes);
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scenes":[{{"nodes":[0]}}],"scene":0,
            "nodes":[{{"mesh":0}}],
            "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"material":0}}]}}],
            "materials":[{{"name":"Red","pbrMetallicRoughness":{{"baseColorFactor":[1,0,0,1]}}}}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}}],
            "bufferViews":[{{"buffer":0,"byteLength":36}}],
            "buffers":[{{"byteLength":36,"uri":"data:application/octet-stream;base64,{b64}"}}]}}"#
        );
        let dir = std::env::temp_dir().join(format!("o3v-iced-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tri.gltf");
        std::fs::write(&path, json).unwrap();
        let model = crate::loaders::load(&path).expect("gltf");
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(model.layers.len(), 1);
        assert_eq!(model.layers[0].name, "Red");
        assert_eq!(model.triangle_count(), 1);
        // Y up (0,1,0) -> Z up (0,0,1)
        assert_eq!(model.bounds.max, [1.0, 0.0, 1.0]);
        assert!(model.layers[0].color[0] > 0.99 && model.layers[0].color[1] < 0.01);
    }

    fn base64_encode(data: &[u8]) -> String {
        const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in data.chunks(3) {
            let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
            let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }
}
