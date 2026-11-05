//! COLLADA (.dae), as SketchUp, Rhino, Blender and many CAD tools export it. The visual scene is
//! flattened to world space, one layer per scene layer (SketchUp tags) or else per material; each
//! node with geometry is a pickable element with its name. Triangles, polylists and polygons
//! (fan triangulated, holes left out), also of skinned controllers; diffuse colours of the
//! effects, textures as the default grey. Units and up axis from `<asset>`; converted to metres,
//! Z up, flat shaded.

use super::{Loader, DEFAULT_COLOR, Source};
use crate::model::Layer;
use glam::{Mat4, Vec3};
use roxmltree::{Document, Node};
use std::collections::{BTreeMap, HashMap};

pub struct ColladaLoader;

impl Loader for ColladaLoader {
    fn name(&self) -> &'static str {
        "COLLADA"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["dae"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        let data = source.read()?;
        let text = std::str::from_utf8(&data).map_err(|_| "COLLADA: the file is not UTF-8 XML")?.trim_start_matches('\u{feff}');
        let doc = Document::parse_with_options(text, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() })
            .map_err(|e| format!("COLLADA: {e}"))?;
        let root = doc.root_element();
        if !root.has_tag_name("COLLADA") {
            return Err("COLLADA: no <COLLADA> root element".into());
        }
        let ids: HashMap<&str, Node> = doc.descendants().filter_map(|n| Some((n.attribute("id")?, n))).collect();
        let scene = Scene { ids: &ids, layers: BTreeMap::new(), next_id: 1 };
        let visual_scene = child(root, "scene")
            .and_then(|s| child(s, "instance_visual_scene"))
            .and_then(|i| scene.resolve(i.attribute("url")))
            .or_else(|| root.descendants().find(|n| n.has_tag_name("visual_scene")))
            .ok_or("COLLADA: no visual scene")?;
        let mut scene = scene;
        let world = asset_matrix(root);
        for node in visual_scene.children().filter(|n| n.has_tag_name("node")) {
            scene.node(node, world, None, 0);
        }
        Ok(scene.layers.into_values().filter(|l| !l.indices.is_empty()).collect())
    }
}

/// Metres and Z up from `<asset><unit meter=".."/><up_axis>..</up_axis>` (default: metres, Y up).
fn asset_matrix(root: Node) -> Mat4 {
    let asset = child(root, "asset");
    let meter = asset.and_then(|a| child(a, "unit")).and_then(|u| u.attribute("meter")).and_then(|m| m.trim().parse().ok()).unwrap_or(1.0);
    let axis = match asset.and_then(|a| child(a, "up_axis")).and_then(|u| u.text()).map(str::trim) {
        Some("Z_UP") => Mat4::IDENTITY,
        // X up: x becomes z, -z becomes x
        Some("X_UP") => Mat4::from_rotation_y(-std::f32::consts::FRAC_PI_2),
        // Y up: (x, y, z) becomes (x, -z, y)
        _ => Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2),
    };
    axis * Mat4::from_scale(Vec3::splat(meter))
}

fn child<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|n| n.has_tag_name(name))
}

fn floats(node: Node) -> Vec<f32> {
    node.text().unwrap_or("").split_ascii_whitespace().map(|s| s.parse().unwrap_or(0.0)).collect()
}

fn ints(node: Node) -> Vec<u32> {
    node.text().unwrap_or("").split_ascii_whitespace().map(|s| s.parse().unwrap_or(u32::MAX)).collect()
}

/// The node's own transform: its `<matrix>`, `<translate>`, `<rotate>` and `<scale>` in order.
fn local_matrix(node: Node) -> Mat4 {
    let mut m = Mat4::IDENTITY;
    for t in node.children().filter(Node::is_element) {
        let v = floats(t);
        m *= match (t.tag_name().name(), v.len()) {
            // row major in the file
            ("matrix", 16) => Mat4::from_cols_slice(&v).transpose(),
            ("translate", 3) => Mat4::from_translation(Vec3::new(v[0], v[1], v[2])),
            ("rotate", 4) => Mat4::from_axis_angle(Vec3::new(v[0], v[1], v[2]).normalize_or(Vec3::Z), v[3].to_radians()),
            ("scale", 3) => Mat4::from_scale(Vec3::new(v[0], v[1], v[2])),
            _ => continue,
        };
    }
    m
}

/// RGBA8 of a `<color>` element (r g b a, 0..1).
fn rgba(color: Node) -> Option<[u8; 4]> {
    let v = floats(color);
    (v.len() >= 3).then(|| [v[0], v[1], v[2]].map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)).map(|[r, g, b]| [r, g, b, 255])
}

struct Scene<'a, 'i> {
    ids: &'a HashMap<&'a str, Node<'a, 'i>>,
    /// Layers by scene layer or material name.
    layers: BTreeMap<String, Layer>,
    next_id: u64,
}

impl<'a, 'i> Scene<'a, 'i> {
    /// The element a `#id` url points to.
    fn resolve(&self, url: Option<&str>) -> Option<Node<'a, 'i>> {
        self.ids.get(url?.strip_prefix('#')?).copied()
    }

    fn node(&mut self, node: Node<'a, 'i>, parent: Mat4, layer: Option<&str>, depth: usize) {
        // instance_node may refer back to an ancestor in a broken file
        if depth > 64 {
            return;
        }
        let world = parent * local_matrix(node);
        let layer = node.attribute("layer").and_then(|l| l.split_whitespace().next()).or(layer);
        let name = node.attribute("name").or(node.attribute("id")).unwrap_or("Node");
        for c in node.children().filter(Node::is_element) {
            match c.tag_name().name() {
                "node" => self.node(c, world, layer, depth + 1),
                "instance_node" => {
                    if let Some(n) = self.resolve(c.attribute("url")) {
                        self.node(n, world, layer, depth + 1);
                    }
                }
                "instance_geometry" | "instance_controller" => {
                    let mut target = self.resolve(c.attribute("url"));
                    // a skin wraps its geometry: <controller><skin source="#geometry">
                    if let Some(skin) = target.filter(|t| t.has_tag_name("controller")).and_then(|t| child(t, "skin")) {
                        target = self.resolve(skin.attribute("source"));
                    }
                    if let Some(mesh) = target.and_then(|g| child(g, "mesh")) {
                        let geometry = target.and_then(|g| g.attribute("name").or(g.attribute("id"))).unwrap_or("");
                        self.mesh(mesh, c, world, layer, name, geometry);
                    }
                }
                _ => {}
            }
        }
    }

    /// The material bound to a primitive's symbol: its name and diffuse colour.
    fn material(&self, instance: Node, symbol: Option<&str>) -> (String, [u8; 4]) {
        let default = DEFAULT_COLOR.map(|v| (v * 255.0).round() as u8);
        let default = [default[0], default[1], default[2], 255];
        let bound = symbol.and_then(|symbol| {
            instance.descendants().find(|n| n.has_tag_name("instance_material") && n.attribute("symbol") == Some(symbol))
        });
        let Some(material) = bound.and_then(|b| self.resolve(b.attribute("target"))).or_else(|| symbol.and_then(|s| self.ids.get(s).copied())) else {
            return ("Default".into(), default);
        };
        let name = material.attribute("name").or(material.attribute("id")).unwrap_or("Material").to_string();
        let effect = child(material, "instance_effect").and_then(|e| self.resolve(e.attribute("url")));
        // <profile_COMMON><technique><lambert|phong|blinn|constant><diffuse><color>
        let color = effect
            .and_then(|e| e.descendants().find(|n| n.has_tag_name("technique") && n.parent().is_some_and(|p| p.has_tag_name("profile_COMMON"))))
            .and_then(|t| t.children().find(Node::is_element))
            .and_then(|shader| child(shader, "diffuse").or_else(|| child(shader, "emission")))
            .and_then(|d| child(d, "color"))
            .and_then(rgba);
        (name, color.unwrap_or(default))
    }

    fn mesh(&mut self, mesh: Node, instance: Node, world: Mat4, layer: Option<&str>, name: &str, geometry: &str) {
        let id = self.next_id;
        self.next_id += 1;
        let mirrored = world.determinant() < 0.0;
        let mut props_written = false;
        for primitive in mesh.children().filter(|n| matches!(n.tag_name().name(), "triangles" | "polylist" | "polygons")) {
            let Some((positions, offset, stride)) = self.inputs(mesh, primitive) else { continue };
            let p: Vec<u32> = primitive.children().filter(|n| n.has_tag_name("p")).flat_map(ints).collect();
            // corners per polygon
            let counts: Vec<usize> = match primitive.tag_name().name() {
                "triangles" => vec![3; p.len() / stride / 3],
                "polylist" => child(primitive, "vcount").map(ints).unwrap_or_default().into_iter().map(|c| c as usize).collect(),
                // one <p> per polygon; <ph> polygons with holes are left out
                _ => primitive.children().filter(|n| n.has_tag_name("p")).map(|n| ints(n).len() / stride).collect(),
            };
            let (material, rgba) = self.material(instance, primitive.attribute("material"));
            let key = layer.map(str::to_string).unwrap_or_else(|| material.clone());
            let out = self.layers.entry(key.clone()).or_insert_with(|| Layer {
                name: key,
                color: [rgba[0], rgba[1], rgba[2]].map(|c| c as f32 / 255.0),
                colors: Some(Vec::new()),
                ..Layer::default()
            });
            let first = out.indices.len() as u32;
            let corner = |i: usize| -> Option<Vec3> {
                let v = *p.get(i * stride + offset)? as usize;
                let xyz = positions.get(3 * v..3 * v + 3)?;
                Some(world.transform_point3(Vec3::new(xyz[0], xyz[1], xyz[2])))
            };
            let mut start = 0;
            for count in counts {
                for k in 1..count.saturating_sub(1) {
                    let Some(mut t) = [start, start + k, start + k + 1].map(corner).into_iter().collect::<Option<Vec<_>>>() else { continue };
                    if mirrored {
                        t.swap(1, 2);
                    }
                    let n = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero();
                    for v in t {
                        out.indices.push(out.vertex_count() as u32);
                        out.positions.extend_from_slice(&v.to_array());
                        out.normals.extend_from_slice(&n.to_array());
                        out.colors.as_mut().unwrap().push(rgba);
                    }
                }
                start += count;
            }
            let count = out.indices.len() as u32 - first;
            if count > 0 {
                out.elements.push((id, first, count));
                if !props_written {
                    let mut props = serde_json::Map::new();
                    props.insert("Name".into(), name.into());
                    props.insert("Geometry".into(), geometry.into());
                    if let Some(layer) = layer {
                        props.insert("Layer".into(), layer.into());
                    }
                    out.props.push((id, serde_json::Value::Object(props).to_string()));
                    props_written = true;
                }
            }
        }
    }

    /// The positions of a primitive (xyz), the offset of its vertex index and the stride of `<p>`.
    fn inputs(&self, mesh: Node, primitive: Node) -> Option<(Vec<f32>, usize, usize)> {
        let inputs: Vec<Node> = primitive.children().filter(|n| n.has_tag_name("input")).collect();
        let offset_of = |n: &Node| n.attribute("offset").and_then(|o| o.parse::<usize>().ok()).unwrap_or(0);
        let stride = inputs.iter().map(|n| offset_of(n) + 1).max()?;
        let vertex = inputs.iter().find(|n| n.attribute("semantic") == Some("VERTEX"))?;
        // VERTEX points to <vertices>, whose POSITION input points to the source
        let vertices = self.resolve(vertex.attribute("source")).or_else(|| child(mesh, "vertices"))?;
        let position = vertices.children().find(|n| n.has_tag_name("input") && n.attribute("semantic") == Some("POSITION"))?;
        let source = self.resolve(position.attribute("source"))?;
        let values = floats(child(source, "float_array")?);
        let accessor_stride = child(source, "technique_common")
            .and_then(|t| child(t, "accessor"))
            .and_then(|a| a.attribute("stride"))
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(3)
            .max(3);
        let xyz = values.chunks_exact(accessor_stride).flat_map(|c| [c[0], c[1], c[2]]).collect();
        Some((xyz, offset_of(vertex), stride))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A 1 x 1 inch square (one polylist quad, red material) used twice through a library node,
    /// once on SketchUp tag `Walls`, and a mirrored triangle; Z up, inches.
    const DAE: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<COLLADA xmlns="http://www.collada.org/2005/11/COLLADASchema" version="1.4.1">
  <asset><unit meter="0.0254" name="inch"/><up_axis>Z_UP</up_axis></asset>
  <library_effects>
    <effect id="red-fx"><profile_COMMON><technique sid="c"><lambert><diffuse><color>1 0 0 1</color></diffuse></lambert></technique></profile_COMMON></effect>
  </library_effects>
  <library_materials><material id="red" name="Red"><instance_effect url="#red-fx"/></material></library_materials>
  <library_geometries>
    <geometry id="square" name="Square"><mesh>
      <source id="square-pos"><float_array id="square-arr" count="12">0 0 0 1 0 0 1 1 0 0 1 0</float_array>
        <technique_common><accessor source="#square-arr" count="4" stride="3"/></technique_common></source>
      <source id="square-n"><float_array id="square-narr" count="3">0 0 1</float_array></source>
      <vertices id="square-v"><input semantic="POSITION" source="#square-pos"/></vertices>
      <polylist count="1" material="mat"><input semantic="VERTEX" source="#square-v" offset="0"/><input semantic="NORMAL" source="#square-n" offset="1"/>
        <vcount>4</vcount><p>0 0 1 0 2 0 3 0</p></polylist>
      <triangles count="1"><input semantic="VERTEX" source="#square-v" offset="0"/><p>0 1 2</p></triangles>
    </mesh></geometry>
  </library_geometries>
  <library_nodes><node id="part" name="Part"><instance_geometry url="#square"><bind_material><technique_common>
    <instance_material symbol="mat" target="#red"/></technique_common></bind_material></instance_geometry></node></library_nodes>
  <library_visual_scenes><visual_scene id="scene">
    <node name="A"><translate>10 0 0</translate><instance_node url="#part"/></node>
    <node name="B" layer="Walls"><matrix>1 0 0 0 0 1 0 0 0 0 1 5 0 0 0 1</matrix><instance_node url="#part"/></node>
    <node name="C"><scale>-1 1 1</scale><instance_geometry url="#square"/></node>
  </visual_scene></library_visual_scenes>
  <scene><instance_visual_scene url="#scene"/></scene>
</COLLADA>"##;

    fn load(text: &str) -> crate::model::Model {
        let source = Source::Bytes { name: "squares.DAE".into(), data: Arc::new(text.as_bytes().to_vec()) };
        crate::loaders::load_with(&source, &crate::loaders::Progress::default()).expect("COLLADA")
    }

    #[test]
    fn flattens_instanced_nodes_into_layers_in_metres() {
        let model = load(DAE);
        assert_eq!(model.format, "COLLADA");
        let names: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Default", "Red", "Walls"]);
        let red = &model.layers[1];
        // node A: the quad (2 triangles, red) and the triangle without a bound material (Default)
        assert_eq!(red.triangle_count(), 2);
        assert_eq!(red.colors.as_ref().unwrap()[0], [255, 0, 0, 255]);
        assert_eq!(red.color, [1.0, 0.0, 0.0]);
        let xs = red.positions.chunks_exact(3).map(|p| p[0]);
        assert!(xs.clone().all(|x| (10.0 * 0.0254 - 1e-5..=11.0 * 0.0254 + 1e-5).contains(&x)));
        // node B on its tag, 5 inches up through the row-major matrix; elements are named after
        // the node that holds the geometry (the library node, as a SketchUp component definition)
        let walls = &model.layers[2];
        assert_eq!(walls.triangle_count(), 3);
        assert!(walls.positions.chunks_exact(3).all(|p| (p[2] - 5.0 * 0.0254).abs() < 1e-6));
        assert!(model.props.values().any(|p| p.contains("\"Layer\":\"Walls\"") && p.contains("\"Name\":\"Part\"")));
        // node C: mirrored, still facing up
        let default = &model.layers[0];
        assert_eq!(default.triangle_count(), 3 + 1);
        assert!(default.normals.chunks_exact(3).all(|n| n[2] > 0.99), "{:?}", default.normals);
        assert!(default.positions.chunks_exact(3).any(|p| p[0] < -0.02));
    }

    #[test]
    fn converts_y_up_files() {
        let model = load(&DAE.replace("Z_UP", "Y_UP"));
        // the squares now stand upright: y in the file becomes z
        assert!(model.bounds.size()[2] > 0.02, "{:?}", model.bounds.size());
        assert!(model.bounds.size()[1] > 0.1, "the 5 inch offset now runs along -y");
    }

    #[test]
    fn rejects_other_xml() {
        let source = Source::Bytes { name: "x.dae".into(), data: Arc::new(b"<svg/>".to_vec()) };
        assert!(ColladaLoader.load(&source).unwrap_err().contains("COLLADA"));
    }
}
