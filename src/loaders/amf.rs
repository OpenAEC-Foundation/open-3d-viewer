//! Additive Manufacturing File Format (`.amf`, ASTM F2915 / ISO/ASTM 52915), plain XML or
//! zip-compressed. Objects with their volumes, placed by the constellations (nested ones too,
//! with their offsets and rotations) or else each once where it is; one layer per material,
//! named after it and in its colour, else per object colour; each placed object a pickable
//! element with its name. Colours of triangles, vertices, volumes, materials and objects, in that
//! order of precedence (colour formulas are not evaluated). Units from the file, converted to
//! metres; Z up. Curved triangle edges, textures and composite materials are not read.

use super::{Loader, Source, DEFAULT_COLOR};
use crate::model::Layer;
use glam::{Mat4, Vec3};
use roxmltree::{Document, Node};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;

pub struct AmfLoader;

impl Loader for AmfLoader {
    fn name(&self) -> &'static str {
        "AMF"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["amf"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        let data = source.read()?;
        let text = if data.starts_with(b"PK") {
            // a zip archive with the AMF document inside
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&*data)).map_err(|e| format!("AMF: {e}"))?;
            let names: Vec<String> = (0..archive.len()).filter_map(|i| archive.name_for_index(i)?.ok().map(|n| n.into_owned())).collect();
            let name = names
                .iter()
                .find(|n| n.to_lowercase().ends_with(".amf"))
                .or_else(|| names.iter().find(|n| !n.ends_with('/')))
                .ok_or("AMF: empty zip archive")?
                .clone();
            let mut text = String::new();
            archive.by_name(&name).map_err(|e| format!("AMF: {e}"))?.read_to_string(&mut text).map_err(|e| format!("AMF: {name}: {e}"))?;
            text
        } else {
            String::from_utf8_lossy(&data).into_owned()
        };
        read(text.trim_start_matches('\u{feff}'))
    }
}

fn child<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|n| n.has_tag_name(name))
}

fn number(node: Node, name: &str) -> Option<f32> {
    child(node, name)?.text()?.trim().parse().ok()
}

/// The `<metadata type="name">` of an element.
fn name(node: Node) -> Option<String> {
    node.children()
        .find(|n| n.has_tag_name("metadata") && n.attribute("type").is_some_and(|t| t.eq_ignore_ascii_case("name")))
        .and_then(|n| n.text())
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// The `<color>` of an element (r g b [a], 0..1), when its values are numbers.
fn color(node: Node) -> Option<[u8; 4]> {
    let c = child(node, "color")?;
    let v = |n: &str| number(c, n).map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    Some([v("r")?, v("g")?, v("b")?, v("a").unwrap_or(255)])
}

fn metres(unit: Option<&str>) -> f32 {
    match unit.unwrap_or("millimeter").trim().to_ascii_lowercase().as_str() {
        "inch" => 0.0254,
        "feet" | "foot" => 0.3048,
        "meter" | "metre" => 1.0,
        "micron" | "micrometer" => 1e-6,
        _ => 0.001,
    }
}

/// An object's triangles in its own coordinates: per volume the material and colours.
struct Object {
    name: String,
    /// (material key, triangle corners, corner colours)
    triangles: Vec<(String, [Vec3; 3], [[u8; 4]; 3])>,
}

fn read(text: &str) -> Result<Vec<Layer>, String> {
    let doc = Document::parse_with_options(text, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() }).map_err(|e| format!("AMF: {e}"))?;
    let root = doc.root_element();
    if !root.has_tag_name("amf") {
        return Err("AMF: no <amf> root element".into());
    }
    let scale = metres(root.attribute("unit"));
    let default = DEFAULT_COLOR.map(|v| (v * 255.0).round() as u8);
    let default = [default[0], default[1], default[2], 255];
    // materials: id -> (layer name, colour)
    let mut materials: HashMap<&str, (String, Option<[u8; 4]>)> = HashMap::new();
    for m in root.children().filter(|n| n.has_tag_name("material")) {
        let id = m.attribute("id").unwrap_or("");
        materials.insert(id, (name(m).unwrap_or_else(|| format!("Material {id}")), color(m)));
    }
    let mut objects: HashMap<&str, Object> = HashMap::new();
    for o in root.children().filter(|n| n.has_tag_name("object")) {
        let id = o.attribute("id").unwrap_or("");
        let object_color = color(o);
        let Some(mesh) = child(o, "mesh") else { continue };
        let mut vertices = Vec::new();
        let mut vertex_colors = Vec::new();
        if let Some(vs) = child(mesh, "vertices") {
            for v in vs.children().filter(|n| n.has_tag_name("vertex")) {
                let c = child(v, "coordinates");
                let p = c.map(|c| Vec3::new(number(c, "x").unwrap_or(0.0), number(c, "y").unwrap_or(0.0), number(c, "z").unwrap_or(0.0)));
                vertices.push(p.unwrap_or(Vec3::ZERO));
                vertex_colors.push(color(v));
            }
        }
        let object_name = name(o).unwrap_or_else(|| format!("Object {id}"));
        let mut triangles = Vec::new();
        for volume in mesh.children().filter(|n| n.has_tag_name("volume")) {
            let material = volume.attribute("materialid").and_then(|m| materials.get(m));
            let volume_color = color(volume).or(material.and_then(|m| m.1)).or(object_color);
            // the layer: the material, else the object's colour
            let key = match material {
                Some((name, _)) => name.clone(),
                None => match volume_color {
                    Some([r, g, b, _]) => format!("Colour #{r:02X}{g:02X}{b:02X}"),
                    None => "Default".into(),
                },
            };
            for t in volume.children().filter(|n| n.has_tag_name("triangle")) {
                let ids = ["v1", "v2", "v3"].map(|k| child(t, k).and_then(|n| n.text()).and_then(|s| s.trim().parse::<usize>().ok()));
                let [Some(a), Some(b), Some(c)] = ids else { continue };
                let (Some(&pa), Some(&pb), Some(&pc)) = (vertices.get(a), vertices.get(b), vertices.get(c)) else { continue };
                let triangle_color = color(t);
                let corner = |i: usize| triangle_color.or(vertex_colors[i]).or(volume_color).unwrap_or(default);
                triangles.push((key.clone(), [pa, pb, pc], [corner(a), corner(b), corner(c)]));
            }
        }
        objects.insert(id, Object { name: object_name, triangles });
    }
    // constellations: id -> (object or constellation id, placement)
    let mut constellations: HashMap<&str, Vec<(&str, Mat4)>> = HashMap::new();
    let mut referenced: HashSet<&str> = HashSet::new();
    for c in root.children().filter(|n| n.has_tag_name("constellation")) {
        let mut instances = Vec::new();
        for i in c.children().filter(|n| n.has_tag_name("instance")) {
            let Some(target) = i.attribute("objectid") else { continue };
            let v = |n: &str| number(i, n).unwrap_or(0.0);
            let placement = Mat4::from_translation(Vec3::new(v("deltax"), v("deltay"), v("deltaz")))
                * Mat4::from_rotation_z(v("rz").to_radians())
                * Mat4::from_rotation_y(v("ry").to_radians())
                * Mat4::from_rotation_x(v("rx").to_radians());
            referenced.insert(target);
            instances.push((target, placement));
        }
        constellations.insert(c.attribute("id").unwrap_or(""), instances);
    }
    let mut out = Out { layers: BTreeMap::new(), next_id: 0 };
    let world = Mat4::from_scale(Vec3::splat(scale));
    // the constellations no other one places, then the objects no constellation places
    let mut roots: Vec<&str> = root
        .children()
        .filter(|n| n.has_tag_name("constellation"))
        .filter_map(|n| n.attribute("id"))
        .filter(|id| !referenced.contains(id))
        .collect();
    roots.extend(root.children().filter(|n| n.has_tag_name("object")).filter_map(|n| n.attribute("id")).filter(|id| !referenced.contains(id)));
    for id in roots {
        out.place(id, world, &objects, &constellations, 0);
    }
    let layers: Vec<Layer> = out.layers.into_values().filter(|l| !l.indices.is_empty()).collect();
    if layers.is_empty() {
        return Err("AMF: no triangles".into());
    }
    Ok(layers)
}

struct Out {
    layers: BTreeMap<String, Layer>,
    next_id: u64,
}

impl Out {
    fn place(&mut self, id: &str, at: Mat4, objects: &HashMap<&str, Object>, constellations: &HashMap<&str, Vec<(&str, Mat4)>>, depth: usize) {
        if depth > 32 {
            return;
        }
        if let Some(instances) = constellations.get(id) {
            for (target, placement) in instances {
                self.place(target, at * *placement, objects, constellations, depth + 1);
            }
            return;
        }
        let Some(object) = objects.get(id) else { return };
        self.next_id += 1;
        let element = self.next_id;
        let mirrored = at.determinant() < 0.0;
        let mut written = false;
        let mut first: HashMap<&str, u32> = HashMap::new();
        for (key, t, c) in &object.triangles {
            let layer = self.layers.entry(key.clone()).or_insert_with(|| Layer {
                name: key.clone(),
                color: [c[0][0], c[0][1], c[0][2]].map(|v| v as f32 / 255.0),
                colors: Some(Vec::new()),
                ..Layer::default()
            });
            first.entry(key.as_str()).or_insert(layer.indices.len() as u32);
            let mut p = t.map(|v| at.transform_point3(v));
            let mut c = *c;
            if mirrored {
                p.swap(1, 2);
                c.swap(1, 2);
            }
            let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
            for i in 0..3 {
                layer.indices.push(layer.vertex_count() as u32);
                layer.positions.extend_from_slice(&p[i].to_array());
                layer.normals.extend_from_slice(&n.to_array());
                layer.colors.as_mut().unwrap().push(c[i]);
            }
        }
        for (key, start) in first {
            let Some(layer) = self.layers.get_mut(key) else { continue };
            let count = layer.indices.len() as u32 - start;
            layer.elements.push((element, start, count));
            if !written {
                let mut props = serde_json::Map::new();
                props.insert("Name".into(), object.name.clone().into());
                props.insert("Object id".into(), id.into());
                layer.props.push((element, serde_json::Value::Object(props).to_string()));
                written = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loaders::{load_with, Progress};
    use std::sync::Arc;

    /// A 10 mm triangle object (red PLA volume, one blue triangle) placed twice by a nested
    /// constellation, 20 mm apart and once turned 90° about Z, and a loose object with an object
    /// colour.
    const AMF: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<amf unit="millimeter" version="1.1">
  <material id="1"><metadata type="name">PLA red</metadata><color><r>1</r><g>0</g><b>0</b></color></material>
  <object id="0">
    <metadata type="name">Wedge</metadata>
    <mesh>
      <vertices>
        <vertex><coordinates><x>0</x><y>0</y><z>0</z></coordinates></vertex>
        <vertex><coordinates><x>10</x><y>0</y><z>0</z></coordinates></vertex>
        <vertex><coordinates><x>0</x><y>10</y><z>0</z></coordinates></vertex>
        <vertex><coordinates><x>0</x><y>0</y><z>10</z></coordinates></vertex>
      </vertices>
      <volume materialid="1">
        <triangle><v1>0</v1><v2>2</v2><v3>1</v3></triangle>
        <triangle><v1>0</v1><v2>1</v2><v3>3</v3><color><r>0</r><g>0</g><b>1</b></color></triangle>
      </volume>
    </mesh>
  </object>
  <object id="5">
    <color><r>0</r><g>1</g><b>0</b></color>
    <mesh><vertices>
      <vertex><coordinates><x>100</x><y>0</y><z>0</z></coordinates></vertex>
      <vertex><coordinates><x>110</x><y>0</y><z>0</z></coordinates></vertex>
      <vertex><coordinates><x>100</x><y>10</y><z>0</z></coordinates></vertex>
    </vertices><volume><triangle><v1>0</v1><v2>1</v2><v3>2</v3></triangle></volume></mesh>
  </object>
  <constellation id="10"><instance objectid="0"><deltax>0</deltax></instance><instance objectid="0"><deltax>20</deltax><rz>90</rz></instance></constellation>
  <constellation id="11"><instance objectid="10"><deltaz>5</deltaz></instance></constellation>
</amf>"#;

    fn load(name: &str, data: Vec<u8>) -> crate::model::Model {
        load_with(&Source::Bytes { name: name.into(), data: Arc::new(data) }, &Progress::default()).expect(name)
    }

    #[test]
    fn places_constellations_in_metres_by_material() {
        let model = load("parts.AMF", AMF.as_bytes().to_vec());
        assert_eq!(model.format, "AMF");
        let names: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Colour #00FF00", "PLA red"]);
        let red = &model.layers[1];
        // two instances of two triangles; the second triangle blue
        assert_eq!(red.triangle_count(), 4);
        let colors = red.colors.as_ref().unwrap();
        assert_eq!((colors[0], colors[3]), ([255, 0, 0, 255], [0, 0, 255, 255]));
        // lifted 5 mm by the outer constellation
        assert!((red.positions[2] - 0.005).abs() < 1e-6);
        // the second instance turned: its x edge now runs along y, from x = 20 mm
        let second = &red.positions[18..27];
        assert!((second[0] - 0.02).abs() < 1e-6 && (second[6] - 0.02).abs() < 1e-6 && (second[7] - 0.01).abs() < 1e-6, "{second:?}");
        assert_eq!(red.elements.len(), 2);
        assert!(model.props.values().any(|p| p.contains("\"Name\":\"Wedge\"")));
        // the bottom triangle (0 2 1) faces down
        assert!(red.normals[2] < -0.99);
        let loose = &model.layers[0];
        assert!((loose.positions[0] - 0.1).abs() < 1e-6);
    }

    #[test]
    fn reads_zipped_amf() {
        let mut zipped = std::io::Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut zipped);
        writer.start_file("parts.amf", zip::write::SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut writer, AMF.as_bytes()).unwrap();
        writer.finish().unwrap();
        let model = load("parts.amf", zipped.into_inner());
        assert_eq!(model.triangle_count(), 5);
    }

    #[test]
    fn reads_the_repository_sample() {
        let model = crate::loaders::load(&std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-models/test.amf")).unwrap();
        assert_eq!((model.format.as_str(), model.triangle_count()), ("AMF", 4));
        // no unit: millimetres
        assert!((model.bounds.size()[2] - 0.004).abs() < 1e-6, "{:?}", model.bounds.size());
    }

    #[test]
    fn rejects_other_xml() {
        assert!(read("<svg/>").unwrap_err().contains("amf"));
    }
}
