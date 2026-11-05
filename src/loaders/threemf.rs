//! 3MF (3D Manufacturing Format): a ZIP package with XML model parts. Each build item is a layer
//! and a pickable element with its name; components are placed with their transforms, also from
//! other model parts (production extension). Base materials and colour groups colour the
//! triangles (first corner); textures and beam lattices are not read. Z up; converted to metres.

use super::{Loader, DEFAULT_COLOR, Source};
use crate::model::Layer;
use glam::{Mat4, Vec3, Vec4};
use std::collections::HashMap;
use std::io::Read;

pub struct ThreeMfLoader;

impl Loader for ThreeMfLoader {
    fn name(&self) -> &'static str {
        "3MF"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["3mf"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        let data = source.read()?;
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&*data)).map_err(|e| format!("3MF: {e}"))?;
        let root = root_part(&mut archive).unwrap_or_else(|| "3D/3dmodel.model".into());
        // every model part: the root and the parts its components may point to
        let names: Vec<String> = (0..archive.len())
            .filter_map(|i| archive.name_for_index(i)?.ok().map(|n| n.into_owned()))
            .filter(|n| n.to_lowercase().ends_with(".model"))
            .collect();
        let mut objects = HashMap::new();
        let mut build = None;
        for name in names {
            let text = read_entry(&mut archive, &name)?;
            let part = parse_part(&name, &text, &mut objects)?;
            if name.eq_ignore_ascii_case(&root) {
                build = Some(part);
            }
        }
        let (scale, items) = build.ok_or_else(|| format!("3MF: model part {root} not found"))?;
        let mut layers = Vec::new();
        for (n, item) in items.iter().enumerate() {
            let mut mesh = Triangles::default();
            mesh.add(&objects, (&item.path, item.object), Mat4::from_scale(Vec3::splat(scale)) * item.transform, 0);
            if mesh.positions.is_empty() {
                continue;
            }
            let object = objects.get(&(item.path.clone(), item.object));
            let name = object
                .and_then(|o| o.name.clone().or_else(|| o.part_number.clone()))
                .unwrap_or_else(|| format!("Item {}", n + 1));
            let id = n as u64 + 1;
            let mut props = serde_json::Map::new();
            props.insert("Name".into(), name.clone().into());
            props.insert("Object id".into(), item.object.to_string().into());
            if let Some(number) = item.part_number.as_ref().or(object.and_then(|o| o.part_number.as_ref())) {
                props.insert("Part number".into(), number.clone().into());
            }
            let count = mesh.positions.len() as u32 / 3;
            // one colour for the whole item: the layer colour, no vertex colours
            let uniform = mesh.colors.windows(2).all(|w| w[0] == w[1]);
            let color = mesh.colors.first().map(|c| c.map(|v| v as f32 / 255.0)).map(|c| [c[0], c[1], c[2]]).unwrap_or(DEFAULT_COLOR);
            layers.push(Layer {
                name,
                color,
                positions: mesh.positions,
                normals: mesh.normals,
                colors: (!uniform).then_some(mesh.colors),
                indices: (0..count).collect(),
                elements: vec![(id, 0, count)],
                props: vec![(id, serde_json::Value::Object(props).to_string())],
                ..Layer::default()
            });
        }
        Ok(layers)
    }
}

/// The 3D model part named in `_rels/.rels`.
fn root_part(archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>) -> Option<String> {
    let text = read_entry(archive, "_rels/.rels").ok()?;
    let doc = roxmltree::Document::parse(&text).ok()?;
    let target = doc
        .descendants()
        .filter(|n| n.has_tag_name("Relationship"))
        .find(|n| n.attribute("Type").is_some_and(|t| t.ends_with("/3dmodel")))?
        .attribute("Target")?;
    Some(target.trim_start_matches('/').to_string())
}

fn read_entry(archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, name: &str) -> Result<String, String> {
    let mut file = archive.by_name(name).map_err(|e| format!("3MF {name}: {e}"))?;
    let mut text = String::new();
    file.read_to_string(&mut text).map_err(|e| format!("3MF {name}: {e}"))?;
    Ok(text)
}

/// An object of a model part: a mesh, or components that place other objects.
struct Object {
    name: Option<String>,
    part_number: Option<String>,
    vertices: Vec<Vec3>,
    /// Corner indices and the colour of the triangle (None: the default grey).
    triangles: Vec<([u32; 3], Option<[u8; 4]>)>,
    components: Vec<Item>,
}

/// A build item or a component: an object (in a part) and where it goes.
struct Item {
    path: String,
    object: u32,
    transform: Mat4,
    part_number: Option<String>,
}

/// Objects are keyed by part path and object id.
type Objects = HashMap<(String, u32), Object>;

/// Reads the objects of one model part into `objects`; returns its unit (metres) and build items.
fn parse_part(path: &str, text: &str, objects: &mut Objects) -> Result<(f32, Vec<Item>), String> {
    let doc = roxmltree::Document::parse(text).map_err(|e| format!("3MF {path}: {e}"))?;
    let model = doc.root_element();
    let scale = match model.attribute("unit").unwrap_or("millimeter") {
        "micron" => 1e-6,
        "centimeter" => 0.01,
        "inch" => 0.0254,
        "foot" => 0.3048,
        "meter" => 1.0,
        _ => 0.001,
    };
    // property groups: base materials and colour groups (namespaces are not compared), by id
    let mut groups: HashMap<u32, Vec<[u8; 4]>> = HashMap::new();
    for group in model.descendants().filter(|n| n.has_tag_name("basematerials") || n.has_tag_name("colorgroup")) {
        let Some(id) = group.attribute("id").and_then(|v| v.parse().ok()) else { continue };
        let colors = group
            .children()
            .filter(|n| n.is_element())
            .map(|n| n.attribute("displaycolor").or(n.attribute("color")).and_then(parse_color).unwrap_or([184, 184, 189, 255]))
            .collect();
        groups.insert(id, colors);
    }
    let color = |pid: Option<u32>, index: Option<usize>| pid.and_then(|p| groups.get(&p)).and_then(|g| g.get(index.unwrap_or(0))).copied();
    for node in model.descendants().filter(|n| n.has_tag_name("object")) {
        let Some(id) = node.attribute("id").and_then(|v| v.parse().ok()) else { continue };
        let pid = node.attribute("pid").and_then(|v| v.parse().ok());
        let pindex = node.attribute("pindex").and_then(|v| v.parse().ok());
        let mut object = Object {
            name: node.attribute("name").filter(|s| !s.is_empty()).map(String::from),
            part_number: node.attribute("partnumber").map(String::from),
            vertices: Vec::new(),
            triangles: Vec::new(),
            components: Vec::new(),
        };
        for n in node.descendants().filter(|n| n.is_element()) {
            match n.tag_name().name() {
                "vertex" => object.vertices.push(Vec3::new(num(n, "x"), num(n, "y"), num(n, "z"))),
                "triangle" => {
                    let v = ["v1", "v2", "v3"].map(|a| n.attribute(a).and_then(|v| v.parse().ok()).unwrap_or(u32::MAX));
                    let tri_pid = n.attribute("pid").and_then(|v| v.parse().ok()).or(pid);
                    let tri_index = n.attribute("p1").and_then(|v| v.parse().ok()).or(pindex);
                    object.triangles.push((v, color(tri_pid, tri_index)));
                }
                "component" => {
                    let Some(object_id) = n.attribute("objectid").and_then(|v| v.parse().ok()) else { continue };
                    // production extension: p:path to another model part
                    let part = n.attributes().find(|a| a.name() == "path").map(|a| a.value().trim_start_matches('/').to_string());
                    object.components.push(Item {
                        path: part.unwrap_or_else(|| path.to_string()),
                        object: object_id,
                        transform: transform(n.attribute("transform")),
                        part_number: None,
                    });
                }
                _ => {}
            }
        }
        objects.insert((path.to_string(), id), object);
    }
    let items = model
        .children()
        .filter(|n| n.has_tag_name("build"))
        .flat_map(|b| b.children().filter(|n| n.has_tag_name("item")))
        .filter_map(|n| {
            Some(Item {
                path: path.to_string(),
                object: n.attribute("objectid")?.parse().ok()?,
                transform: transform(n.attribute("transform")),
                part_number: n.attribute("partnumber").map(String::from),
            })
        })
        .collect();
    Ok((scale, items))
}

fn num(node: roxmltree::Node, attribute: &str) -> f32 {
    node.attribute(attribute).and_then(|v| v.parse().ok()).unwrap_or(0.0)
}

/// `m00 m01 m02 m10 m11 m12 m20 m21 m22 m30 m31 m32`: the 3MF row-vector matrix, so each triple
/// is a column of glam's column-vector matrix.
fn transform(value: Option<&str>) -> Mat4 {
    let Some(values) = value.map(|v| v.split_whitespace().filter_map(|s| s.parse::<f32>().ok()).collect::<Vec<_>>()) else {
        return Mat4::IDENTITY;
    };
    if values.len() != 12 {
        return Mat4::IDENTITY;
    }
    let column = |i: usize, w: f32| Vec4::new(values[3 * i], values[3 * i + 1], values[3 * i + 2], w);
    Mat4::from_cols(column(0, 0.0), column(1, 0.0), column(2, 0.0), column(3, 1.0))
}

/// `#RRGGBB` or `#RRGGBBAA`, sRGB.
fn parse_color(value: &str) -> Option<[u8; 4]> {
    let hex = value.strip_prefix('#')?;
    let byte = |i: usize| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
    match hex.len() {
        6 => Some([byte(0)?, byte(2)?, byte(4)?, 255]),
        8 => Some([byte(0)?, byte(2)?, byte(4)?, byte(6)?]),
        _ => None,
    }
}

/// Flat-shaded triangles of one build item in world space.
#[derive(Default)]
struct Triangles {
    positions: Vec<f32>,
    normals: Vec<f32>,
    colors: Vec<[u8; 4]>,
}

impl Triangles {
    fn add(&mut self, objects: &Objects, key: (&String, u32), world: Mat4, depth: usize) {
        // components may not refer back to themselves, but a broken file could
        let Some(object) = objects.get(&(key.0.clone(), key.1)).filter(|_| depth < 32) else { return };
        let mirrored = world.determinant() < 0.0;
        let default = DEFAULT_COLOR.map(|v| (v * 255.0).round() as u8);
        for &(v, color) in &object.triangles {
            let Some(mut p) = v.iter().map(|&i| object.vertices.get(i as usize).map(|&p| world.transform_point3(p))).collect::<Option<Vec<_>>>() else {
                continue;
            };
            if mirrored {
                p.swap(1, 2);
            }
            let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
            let rgba = color.unwrap_or([default[0], default[1], default[2], 255]);
            for corner in p {
                self.positions.extend_from_slice(&corner.to_array());
                self.normals.extend_from_slice(&n.to_array());
                self.colors.push(rgba);
            }
        }
        for c in &object.components {
            self.add(objects, (&c.path, c.object), world * c.transform, depth + 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Arc;

    /// A package whose model part is not at the usual path.
    fn package(model: &str) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut out);
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("_rels/.rels", options).unwrap();
        zip.write_all(br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Target="/3D/main.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/></Relationships>"#).unwrap();
        zip.start_file("3D/main.model", options).unwrap();
        zip.write_all(model.as_bytes()).unwrap();
        zip.finish().unwrap();
        out.into_inner()
    }

    /// A red/green two-triangle object placed twice through components (once mirrored and moved
    /// 10 mm), and a blue triangle lifted 1000 mm.
    const MODEL: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02" xmlns:m="http://schemas.microsoft.com/3dmanufacturing/material/2015/02">
  <resources>
    <basematerials id="1"><base name="Red" displaycolor="#FF0000"/><base name="Blue" displaycolor="#0000FFFF"/></basematerials>
    <m:colorgroup id="5"><m:color color="#00FF00FF"/></m:colorgroup>
    <object id="2" type="model" pid="1" pindex="0" name="Triangle">
      <mesh>
        <vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/><vertex x="0" y="0" z="1"/></vertices>
        <triangles><triangle v1="0" v2="1" v3="2"/><triangle v1="0" v2="1" v3="3" pid="5" p1="0"/></triangles>
      </mesh>
    </object>
    <object id="3" type="model" name="Pair">
      <components>
        <component objectid="2"/>
        <component objectid="2" transform="-1 0 0 0 1 0 0 0 1 10 0 0"/>
      </components>
    </object>
    <object id="4" type="model" pid="1" pindex="1" partnumber="B-1">
      <mesh><vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/><vertex x="0" y="1" z="0"/></vertices><triangles><triangle v1="0" v2="1" v3="2"/></triangles></mesh>
    </object>
  </resources>
  <build><item objectid="3"/><item objectid="4" transform="1 0 0 0 1 0 0 0 1 0 0 1000"/></build>
</model>"##;

    #[test]
    fn places_components_with_colours_in_metres() {
        let source = Source::Bytes { name: "parts.3MF".into(), data: Arc::new(package(MODEL)) };
        let model = crate::loaders::load_with(&source, &crate::loaders::Progress::default()).expect("3MF");
        assert_eq!(model.format, "3MF");
        assert_eq!(model.layers.len(), 2);
        let pair = &model.layers[0];
        assert_eq!(pair.name, "Pair");
        assert_eq!(pair.triangle_count(), 4);
        // red and green triangles: vertex colours
        let colors = pair.colors.as_ref().expect("mixed colours");
        assert_eq!(colors[0], [255, 0, 0, 255]);
        assert_eq!(colors[3], [0, 255, 0, 255]);
        // the mirrored copy: x from 9 to 10 mm, still facing up
        let xs: Vec<f32> = pair.positions.chunks_exact(3).map(|p| p[0]).collect();
        assert!(xs.iter().any(|&x| (x - 0.009).abs() < 1e-6) && xs.iter().any(|&x| (x - 0.010).abs() < 1e-6), "{xs:?}");
        assert!(pair.normals.chunks_exact(3).take(3).all(|n| n[2] > 0.99));
        assert!(pair.normals.chunks_exact(3).skip(6).take(3).all(|n| n[2] > 0.99), "mirrored winding");
        let blue = &model.layers[1];
        assert_eq!(blue.name, "B-1");
        assert!(blue.colors.is_none());
        assert_eq!(blue.color, [0.0, 0.0, 1.0]);
        assert!((blue.positions[2] - 1.0).abs() < 1e-6, "1000 mm up");
        assert!(model.props.values().any(|p| p.contains("\"Part number\":\"B-1\"")));
    }

    #[test]
    fn opens_the_consortium_sphere() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-models/test-sphere.3mf");
        let model = crate::loaders::load(&path).expect("3MF sphere");
        assert!(model.triangle_count() > 1000, "{}", model.triangle_count());
        // a 10 mm radius sphere
        assert!(model.bounds.size().iter().all(|&s| (s - 0.02).abs() < 0.001), "{:?}", model.bounds.size());
    }

    #[test]
    fn parses_colours_and_transforms() {
        assert_eq!(parse_color("#11223344"), Some([0x11, 0x22, 0x33, 0x44]));
        assert_eq!(parse_color("red"), None);
        let m = transform(Some("1 0 0 0 1 0 0 0 1 4 5 6"));
        assert_eq!(m.transform_point3(Vec3::ONE), Vec3::new(5.0, 6.0, 7.0));
        assert_eq!(transform(Some("1 2")), Mat4::IDENTITY);
    }
}
