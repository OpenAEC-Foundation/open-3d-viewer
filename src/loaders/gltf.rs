//! glTF 2.0 (.gltf / .glb). The default scene is flattened to world space and grouped into one
//! layer per material; each node with a mesh is a pickable element with its name. glTF is Y up,
//! metres; converted to Z up. Base colour = material factor times the average colour of its
//! base-colour texture (no texture mapping yet). Triangle strips and fans are drawn as
//! triangles; point primitives (scans, photogrammetry) as dots in a point layer per material, in
//! their vertex colours. Lines are not drawn.

use super::{Loader, DEFAULT_COLOR, Source};
use crate::model::{flat_shaded, y_up_to_z_up, Layer};
use glam::{Mat3, Mat4, Vec3};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct GltfLoader;

impl Loader for GltfLoader {
    fn name(&self) -> &'static str {
        "glTF"
    }

    fn featured(&self) -> Option<u8> {
        Some(3)
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["gltf", "glb"]
    }

    fn source_y_up(&self) -> bool {
        true
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        let (doc, buffers, images) = match source.path() {
            Some(path) => import_file(path)?,
            // bytes from the browser: .glb or .gltf with embedded buffers
            None => gltf::import_slice(source.read()?).map_err(|e| format!("glTF: {e}"))?,
        };
        let texture_average: Vec<[f32; 4]> = images.iter().map(average_color).collect();

        // (material index (None = default material), points) -> layer under construction
        let mut layers: BTreeMap<(Option<usize>, bool), Layer> = BTreeMap::new();

        let scene = doc.default_scene().or_else(|| doc.scenes().next());
        let roots: Vec<gltf::Node> = match &scene {
            Some(s) => s.nodes().collect(),
            None => doc.nodes().collect(),
        };
        let mut stack: Vec<(gltf::Node, Mat4)> = roots.into_iter().map(|n| (n, Mat4::IDENTITY)).collect();
        while let Some((node, parent)) = stack.pop() {
            let world = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
            if let Some(mesh) = node.mesh() {
                // element ids start at 1: node index + 1
                let element = node.index() as u64 + 1;
                let mut props_in: Option<Option<usize>> = None;
                for prim in mesh.primitives() {
                    if add_primitive(&prim, world, &buffers, &texture_average, &mut layers, element) {
                        props_in.get_or_insert(prim.material().index());
                    }
                }
                if let Some(layer) = props_in.and_then(|m| layers.get_mut(&(m, false))) {
                    let mut props = serde_json::Map::new();
                    let name = node.name().or(mesh.name()).map(str::to_owned).unwrap_or_else(|| format!("Node {}", node.index()));
                    props.insert("Name".into(), name.into());
                    if let Some(m) = mesh.name() {
                        props.insert("Mesh".into(), m.into());
                    }
                    layer.props.push((element, serde_json::Value::Object(props).to_string()));
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

type Import = (gltf::Document, Vec<gltf::buffer::Data>, Vec<gltf::image::Data>);

/// `gltf::import`, with the files the glTF refers to (buffers, images) checked first: where the AI
/// connection is held to the work folder, they must lie in it too (`super::check_reference`).
fn import_file(path: &Path) -> Result<Import, String> {
    let gltf::Gltf { document, blob } = gltf::Gltf::open(path).map_err(|e| format!("glTF: {e}"))?;
    let base = path.parent().unwrap_or(Path::new("./"));
    let buffers = document.buffers().filter_map(|b| match b.source() {
        gltf::buffer::Source::Uri(uri) => Some(uri.to_string()),
        gltf::buffer::Source::Bin => None,
    });
    let images = document.images().filter_map(|i| match i.source() {
        gltf::image::Source::Uri { uri, .. } => Some(uri.to_string()),
        gltf::image::Source::View { .. } => None,
    });
    let uris: Vec<String> = buffers.chain(images).collect();
    for file in uris.iter().filter_map(|uri| referenced_file(base, uri)) {
        super::check_reference(path, &file).map_err(|e| format!("glTF: {e}"))?;
    }
    let buffers = gltf::import_buffers(&document, Some(base), blob).map_err(|e| format!("glTF: {e}"))?;
    let images = gltf::import_images(&document, Some(base), &buffers).map_err(|e| format!("glTF: {e}"))?;
    Ok((document, buffers, images))
}

/// The file a URI of a glTF names, as `gltf::import` reads it (`file:` URIs and relative paths,
/// percent-decoded); none for embedded data and schemes it does not read.
fn referenced_file(base: &Path, uri: &str) -> Option<PathBuf> {
    if !uri.contains(':') {
        return Some(base.join(percent_decoded(uri)));
    }
    uri.strip_prefix("file://").or_else(|| uri.strip_prefix("file:")).map(PathBuf::from)
}

fn percent_decoded(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = b
            .get(i + 1..i + 3)
            .filter(|h| h.iter().all(u8::is_ascii_hexdigit))
            .and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok());
        match (b[i], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn add_primitive(
    prim: &gltf::Primitive,
    world: Mat4,
    buffers: &[gltf::buffer::Data],
    texture_average: &[[f32; 4]],
    layers: &mut BTreeMap<(Option<usize>, bool), Layer>,
    element: u64,
) -> bool {
    use gltf::mesh::Mode;
    if !matches!(prim.mode(), Mode::Triangles | Mode::TriangleStrip | Mode::TriangleFan | Mode::Points) {
        return false; // lines are not shown
    }
    let reader = prim.reader(|b| buffers.get(b.index()).map(|d| &d.0[..]));
    let Some(positions) = reader.read_positions() else { return false };
    let positions: Vec<[f32; 3]> = positions.collect();
    let indices: Vec<u32> = match reader.read_indices() {
        Some(i) => i.into_u32().collect(),
        None => (0..positions.len() as u32).collect(),
    };
    let indices: Vec<u32> = match prim.mode() {
        // every other triangle of a strip turns the other way
        Mode::TriangleStrip => (2..indices.len())
            .flat_map(|k| if k % 2 == 0 { [indices[k - 2], indices[k - 1], indices[k]] } else { [indices[k - 1], indices[k - 2], indices[k]] })
            .collect(),
        Mode::TriangleFan => (2..indices.len()).flat_map(|k| [indices[0], indices[k - 1], indices[k]]).collect(),
        _ => indices,
    };
    if prim.mode() == Mode::Points {
        let colors = reader.read_colors(0).map(|c| c.into_rgba_f32().collect());
        add_points(prim, world, &positions, &indices, colors, texture_average, layers);
        return false;
    }
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

    let layer = layers.entry((material.index(), false)).or_insert_with(|| Layer {
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
        ..Layer::default()
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
    let first = layer.indices.len() as u32;
    layer.indices.extend(idx.into_iter().map(|i| i + offset));
    let count = layer.indices.len() as u32 - first;
    if count > 0 {
        layer.elements.push((element, first, count));
    }
    count > 0
}

/// A point primitive: its points (by index) into the point layer of its material.
fn add_points(
    prim: &gltf::Primitive,
    world: Mat4,
    positions: &[[f32; 3]],
    indices: &[u32],
    colors: Option<Vec<[f32; 4]>>,
    texture_average: &[[f32; 4]],
    layers: &mut BTreeMap<(Option<usize>, bool), Layer>,
) {
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
    let layer = layers.entry((material.index(), true)).or_insert_with(|| Layer {
        name: format!(
            "{} (points)",
            material.name().map(str::to_owned).unwrap_or_else(|| match material.index() {
                Some(i) => format!("Material {i}"),
                None => "Default".to_owned(),
            })
        ),
        color: if material.index().is_some() { srgb } else { DEFAULT_COLOR },
        colors: Some(Vec::new()),
        points: true,
        ..Layer::default()
    });
    let fill = layer.color.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    for &i in indices {
        let Some(p) = positions.get(i as usize) else { continue };
        layer.positions.extend_from_slice(&world.transform_point3(Vec3::from_array(*p)).to_array());
        let c = match colors.as_ref().and_then(|c| c.get(i as usize)) {
            Some(c) => [0, 1, 2].map(|k| (linear_to_srgb(c[k] * base[k]) * 255.0).round() as u8),
            None => fill,
        };
        layer.colors.as_mut().unwrap().push([c[0], c[1], c[2], 255]);
    }
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
    use std::path::{Path, PathBuf};

    /// The files a glTF refers to, as the importer reads them (and the work folder checks them).
    #[test]
    fn finds_the_files_a_gltf_refers_to() {
        let base = Path::new("/openaec/project");
        let r = |uri: &str| super::referenced_file(base, uri);
        assert_eq!(r("model.bin"), Some(base.join("model.bin")));
        assert_eq!(r("../../home/user/x.bin"), Some(base.join("../../home/user/x.bin")));
        assert_eq!(r("%2e%2e/secret%20file.bin"), Some(base.join("../secret file.bin")));
        assert_eq!(r("file:///home/user/.config/secret"), Some(PathBuf::from("/home/user/.config/secret")));
        assert_eq!(r("file:/etc/passwd"), Some(PathBuf::from("/etc/passwd")));
        assert_eq!(r("data:application/octet-stream;base64,AAAA"), None);
        assert_eq!(super::percent_decoded("a%2"), "a%2");
    }

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
        // the node is a pickable element; unnamed, so "Node <index>"
        assert_eq!(model.layers[0].elements, vec![(1, 0, 3)]);
        assert_eq!(model.element_props(1), vec![("Name".to_string(), "Node 0".to_string())]);
    }

    /// A glTF of four points (mode 0) with colours, and the same four as a triangle strip.
    #[test]
    fn loads_points_and_triangle_strips() {
        let mut bytes = Vec::new();
        for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        for c in [1.0f32, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0] {
            bytes.extend_from_slice(&c.to_le_bytes());
        }
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scenes":[{{"nodes":[0,1]}}],"scene":0,
            "nodes":[{{"mesh":0}},{{"mesh":1,"translation":[0,0,5]}}],
            "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0,"COLOR_0":1}},"mode":0}}]}},{{"primitives":[{{"attributes":{{"POSITION":0}},"mode":5}}]}}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":4,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}},
                         {{"bufferView":1,"componentType":5126,"count":4,"type":"VEC4"}}],
            "bufferViews":[{{"buffer":0,"byteLength":48}},{{"buffer":0,"byteOffset":48,"byteLength":64}}],
            "buffers":[{{"byteLength":112,"uri":"data:application/octet-stream;base64,{}"}}]}}"#,
            base64_encode(&bytes)
        );
        let source = crate::loaders::Source::Bytes { name: "scan.gltf".into(), data: std::sync::Arc::new(json.into_bytes()) };
        let model = crate::loaders::load_with(&source, &crate::loaders::Progress::default()).expect("gltf");
        let names: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Default", "Default (points)"]);
        assert_eq!((model.triangle_count(), model.point_count()), (2, 4));
        let points = &model.layers[1];
        assert_eq!(points.colors.as_ref().unwrap()[1], [0, 255, 0, 255]);
        // Y up: the point at y = 1 is at z = 1
        assert_eq!(&points.positions[6..9], &[0.0, 0.0, 1.0]);
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
