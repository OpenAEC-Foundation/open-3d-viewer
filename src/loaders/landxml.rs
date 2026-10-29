//! LandXML 1.x TIN surfaces. LandXML stores coordinates as northing, easting,
//! elevation; the viewer uses easting, northing, elevation in metres.
//!
//! This focused reader does not turn survey points or alignments into surfaces.
//! Geometry is first parsed in f64, then offset by one common origin before it
//! enters the f32 renderer. The origin is retained in project information.

use super::{Loaded, Loader, Phase, Progress, Source, DEFAULT_COLOR};
use crate::model::{flat_shaded, Layer, ProjectData};
use roxmltree::Node;
use std::collections::HashMap;

const MAX_FILE_BYTES: usize = 128 * 1024 * 1024;
const MAX_POINTS: usize = 2_000_000;
const MAX_TRIANGLES: usize = 2_000_000;

pub struct LandXmlLoader;

impl Loader for LandXmlLoader {
    fn name(&self) -> &'static str { "LandXML" }
    fn extensions(&self) -> &'static [&'static str] { &["landxml", "xml"] }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        if source.len() > MAX_FILE_BYTES as u64 {
            return Err("LandXML: file exceeds the 128 MiB import limit".into());
        }
        let bytes = source.read()?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err("LandXML: file exceeds the 128 MiB import limit".into());
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| "LandXML: expected UTF-8 XML".to_string())?
            .trim_start_matches('\u{feff}');
        progress.set(Phase::Convert);
        read(text)
    }
}

fn child<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|n| n.is_element() && n.tag_name().name() == name)
}

fn coordinates(text: &str, factor: f64) -> Result<[f64; 3], String> {
    let values = text.split_whitespace().map(|v| v.parse::<f64>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "LandXML: invalid point coordinate".to_string())?;
    if values.len() != 3 || values.iter().any(|v| !v.is_finite()) {
        return Err("LandXML: each TIN point must contain three finite coordinates".into());
    }
    let point = [values[1] * factor, values[0] * factor, values[2] * factor];
    if point.iter().any(|v| !v.is_finite()) {
        return Err("LandXML: point overflows after unit conversion".into());
    }
    Ok(point)
}

fn length_unit(root: Node<'_, '_>) -> Result<(f64, String), String> {
    let Some(units) = child(root, "Units") else {
        return Err("LandXML: missing Units; cannot determine terrain scale".into());
    };
    let definitions: Vec<_> = units.children().filter(|n| n.is_element()).collect();
    if definitions.len() != 1 || !matches!(definitions[0].tag_name().name(), "Metric" | "Imperial") {
        return Err("LandXML: expected one Metric or Imperial unit definition".into());
    }
    let unit = definitions[0].attribute("linearUnit")
        .ok_or("LandXML: missing linearUnit")?;
    let factor = match unit.to_ascii_lowercase().as_str() {
        "millimeter" | "millimetre" => 0.001,
        "centimeter" | "centimetre" => 0.01,
        "decimeter" | "decimetre" => 0.1,
        "meter" | "metre" => 1.0,
        "kilometer" | "kilometre" => 1000.0,
        "foot" | "internationalfoot" => 0.3048,
        "ussurveyfoot" => 1200.0 / 3937.0,
        "inch" => 0.0254,
        "mile" => 1609.344,
        _ => return Err(format!("LandXML: unsupported linearUnit '{unit}'")),
    };
    Ok((factor, unit.to_owned()))
}

fn read(text: &str) -> Result<Loaded, String> {
    let doc = roxmltree::Document::parse(text).map_err(|e| format!("LandXML: {e}"))?;
    let root = doc.root_element();
    if root.tag_name().name() != "LandXML" {
        return Err("LandXML: this XML file is not a LandXML document".into());
    }
    let version = root.attribute("version").unwrap_or("1.x");
    if !version.starts_with("1.") {
        return Err(format!("LandXML: unsupported version '{version}' (expected 1.x)"));
    }
    let (factor, unit) = length_unit(root)?;
    let mut origin: Option<[f64; 3]> = None;
    let mut layers = Vec::new();
    let mut total_points = 0usize;
    let mut total_triangles = 0usize;
    let Some(surfaces) = child(root, "Surfaces") else {
        return Err("LandXML: no TIN surfaces; survey points and alignments alone are not supported".into());
    };
    for surface in surfaces.children().filter(|n| n.is_element() && n.tag_name().name() == "Surface") {
        let name = surface.attribute("name").filter(|s| !s.is_empty()).unwrap_or("Terrain");
        let definition = child(surface, "Definition")
            .ok_or_else(|| format!("LandXML: surface '{name}' has no Definition"))?;
        if definition.attribute("surfType") != Some("TIN") {
            return Err(format!("LandXML: surface '{name}' is not a TIN surface"));
        }
        let points_node = child(definition, "Pnts")
            .ok_or_else(|| format!("LandXML: surface '{name}' has no Pnts"))?;
        let mut points = HashMap::new();
        for point in points_node.children().filter(|n| n.is_element() && n.tag_name().name() == "P") {
            total_points += 1;
            if total_points > MAX_POINTS {
                return Err("LandXML: point count exceeds the two million point import limit".into());
            }
            let id = point.attribute("id").filter(|s| !s.is_empty() && !s.chars().any(char::is_whitespace))
                .ok_or("LandXML: a TIN point has an empty or missing id")?;
            let xyz = coordinates(point.text().unwrap_or(""), factor)?;
            if points.insert(id, xyz).is_some() {
                return Err(format!("LandXML: duplicate point id '{id}' in surface '{name}'"));
            }
        }
        let faces_node = child(definition, "Faces")
            .ok_or_else(|| format!("LandXML: surface '{name}' has no Faces"))?;
        let mut positions = Vec::new();
        let mut indices = Vec::new();
        for face in faces_node.children().filter(|n| n.is_element() && n.tag_name().name() == "F") {
            match face.attribute("i") {
                Some("1") => continue,
                None | Some("0") => {},
                Some(value) => return Err(format!("LandXML: invalid face visibility flag '{value}'")),
            }
            let ids: Vec<_> = face.text().unwrap_or("").split_whitespace().collect();
            if ids.len() != 3 {
                return Err(format!("LandXML: a face in surface '{name}' must reference exactly three point ids"));
            }
            if ids[0] == ids[1] || ids[1] == ids[2] || ids[0] == ids[2] {
                return Err(format!("LandXML: a face in surface '{name}' repeats a point id"));
            }
            let mut triangle = [[0.0f64; 3]; 3];
            for (i, id) in ids.iter().enumerate() {
                triangle[i] = *points.get(id).ok_or_else(|| format!("LandXML: face references missing point '{id}' in surface '{name}'"))?;
            }
            let a = glam::DVec3::from_array(triangle[0]);
            let b = glam::DVec3::from_array(triangle[1]);
            let c = glam::DVec3::from_array(triangle[2]);
            if (b - a).cross(c - a).length_squared() <= 1e-24 {
                return Err(format!("LandXML: degenerate triangle in surface '{name}'"));
            }
            total_triangles += 1;
            if total_triangles > MAX_TRIANGLES {
                return Err("LandXML: triangle count exceeds the two million triangle import limit".into());
            }
            let anchor = *origin.get_or_insert(triangle[0]);
            for xyz in triangle {
                for axis in 0..3 {
                    let value = (xyz[axis] - anchor[axis]) as f32;
                    if !value.is_finite() {
                        return Err("LandXML: terrain extent exceeds the renderer coordinate range".into());
                    }
                    positions.push(value);
                }
                indices.push(indices.len() as u32);
            }
        }
        if indices.is_empty() {
            continue;
        }
        let (positions, normals, indices) = flat_shaded(&positions, &indices);
        let element = layers.len() as u64 + 1;
        let count = indices.len() as u32;
        let properties = serde_json::json!({
            "Name": name,
            "Source": "LandXML TIN",
            "File length unit": unit,
            "Triangles": count / 3,
        }).to_string();
        layers.push(Layer {
            name: name.to_owned(), color: DEFAULT_COLOR, positions, normals, indices,
            elements: vec![(element, 0, count)], props: vec![(element, properties)],
            ..Layer::default()
        });
    }
    let anchor = origin.ok_or("LandXML: no visible TIN triangles found")?;
    let mut info = vec![
        ("LandXML version".into(), version.to_owned()),
        ("LandXML file length unit".into(), unit),
        ("LandXML world origin (metres)".into(), format!("{}, {}, {}", anchor[0], anchor[1], anchor[2])),
    ];
    if let Some(crs) = child(root, "CoordinateSystem") {
        for attr in crs.attributes() {
            info.push((format!("Coordinate system {}", attr.name()), attr.value().into()));
        }
    }
    Ok(Loaded { layers, views: Vec::new(), project: ProjectData { info, ..ProjectData::default() } })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xml(units: &str, points: &str, faces: &str) -> String {
        format!(r#"<LandXML xmlns="http://www.landxml.org/schema/LandXML-1.2" version="1.2"><Units>{units}</Units><Surfaces><Surface name="Design"><Definition surfType="TIN"><Pnts>{points}</Pnts><Faces>{faces}</Faces></Definition></Surface></Surfaces></LandXML>"#)
    }

    fn metric(points: &str, faces: &str) -> String {
        xml(r#"<Metric linearUnit="meter"/>"#, points, faces)
    }

    const POINTS: &str = r#"<P id="10">487000 121000 2</P><P id="30">487000 121010 2</P><P id="20">487020 121000 3</P>"#;

    #[test]
    fn imports_tin_with_sparse_ids_and_local_geographic_origin() {
        let loaded = read(&metric(POINTS, "<F>10 30 20</F>")).unwrap();
        let layer = &loaded.layers[0];
        assert_eq!(layer.name, "Design");
        assert_eq!(layer.triangle_count(), 1);
        assert_eq!(layer.positions, [0., 0., 0., 10., 0., 0., 0., 20., 1.]);
        assert_eq!(layer.normals.len(), layer.positions.len());
        assert!(layer.normals[2] > 0.99);
        assert_eq!(layer.elements, [(1, 0, 3)]);
        assert!(loaded.project.info.iter().any(|(k, v)| k.contains("origin") && v == "121000, 487000, 2"));
    }

    #[test]
    fn converts_imperial_and_metric_lengths_to_metres() {
        let points = r#"<P id="a">0 0 0</P><P id="b">0 10 0</P><P id="c">10 0 0</P>"#;
        let imperial = read(&xml(r#"<Imperial linearUnit="foot"/>"#, points, "<F>a b c</F>")).unwrap();
        assert!((imperial.layers[0].positions[3] - 3.048).abs() < 1e-6);
        let millimetres = read(&xml(r#"<Metric linearUnit="millimeter"/>"#, points, "<F>a b c</F>")).unwrap();
        assert!((millimetres.layers[0].positions[3] - 0.01).abs() < 1e-7);
    }

    #[test]
    fn retains_one_common_origin_for_different_surfaces() {
        let first = metric(POINTS, "<F>10 30 20</F>");
        let second = r#"<Surface name="Existing"><Definition surfType="TIN"><Pnts><P id="1">487000 121100 5</P><P id="2">487000 121110 5</P><P id="3">487010 121100 5</P></Pnts><Faces><F>1 2 3</F></Faces></Definition></Surface>"#;
        let text = first.replace("</Surfaces>", &format!("{second}</Surfaces>"));
        let loaded = read(&text).unwrap();
        assert_eq!(loaded.layers.len(), 2);
        assert_eq!(&loaded.layers[1].positions[..3], &[100., 0., 3.]);
        assert_eq!(loaded.layers[1].elements, [(2, 0, 3)]);
    }

    #[test]
    fn excludes_hidden_faces_without_resolving_their_vertices() {
        let loaded = read(&metric(POINTS, "<F i=\"1\">99 98 97</F><F>10 30 20</F>")).unwrap();
        assert_eq!(loaded.layers[0].triangle_count(), 1);
        assert!(read(&metric(POINTS, "<F i=\"1\">10 30 20</F>")).err().unwrap().contains("no visible"));
    }

    #[test]
    fn rejects_invalid_geometry_instead_of_silently_dropping_it() {
        assert!(read(&metric(POINTS, "<F>10 30 999</F>")).err().unwrap().contains("missing point"));
        assert!(read(&metric(POINTS, "<F>10 30</F>")).err().unwrap().contains("exactly three"));
        assert!(read(&metric(POINTS, "<F>10 10 20</F>")).err().unwrap().contains("repeats"));
        let duplicate = format!("{POINTS}<P id=\"10\">0 0 0</P>");
        assert!(read(&metric(&duplicate, "<F>10 30 20</F>")).err().unwrap().contains("duplicate"));
        let nonfinite = POINTS.replace("487020", "NaN");
        assert!(read(&metric(&nonfinite, "<F>10 30 20</F>")).is_err());
    }

    #[test]
    fn rejects_wrong_xml_missing_units_and_unsupported_geometry() {
        assert!(read("<CityModel/>").err().unwrap().contains("not a LandXML"));
        let points_only = r#"<LandXML version="1.2"><Units><Metric linearUnit="meter"/></Units><CgPoints/></LandXML>"#;
        assert!(read(points_only).err().unwrap().contains("no TIN surfaces"));
        assert!(read(&metric(POINTS, "<F>10 30 20</F>").replace("meter", "furlong")).err().unwrap().contains("linearUnit"));
        assert!(read(&metric(POINTS, "<F>10 30 20</F>").replace("<Units><Metric linearUnit=\"meter\"/></Units>", "")).err().unwrap().contains("Units"));
        assert!(read(&metric(POINTS, "<F>10 30 20</F>").replace("surfType=\"TIN\"", "surfType=\"Grid\"")).err().unwrap().contains("not a TIN"));
    }
}
