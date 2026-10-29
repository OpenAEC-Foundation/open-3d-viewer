//! Static CityJSON polygon geometry. Coordinates stay f64 until rebased to a local origin.
//! Supports CityJSON documents and CityJSONFeature sequences; highest LoD per object.
use super::{Loaded, Loader, Phase, Progress, Source, DEFAULT_COLOR};
use crate::model::{Layer, ProjectData};
use serde_json::Value;
use std::collections::BTreeMap;

pub struct CityJsonLoader;

impl Loader for CityJsonLoader {
    fn name(&self) -> &'static str { "CityJSON" }
    fn extensions(&self) -> &'static [&'static str] { &["cityjson", "json", "jsonl", "cityjsonl"] }
    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|v| v.layers)
    }
    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        if source.len() > 256 * 1024 * 1024 { return Err("CityJSON: file exceeds the 256 MiB import limit".into()); }
        let bytes = source.read()?;
        progress.set(Phase::Convert);
        read(&bytes)
    }
}

#[derive(Clone, Copy)]
struct Transform { scale: [f64; 3], translate: [f64; 3] }
impl Default for Transform {
    fn default() -> Self { Self { scale: [1.; 3], translate: [0.; 3] } }
}

fn triple(value: &Value, label: &str) -> Result<[f64; 3], String> {
    let values = value.as_array().filter(|a| a.len() == 3)
        .ok_or_else(|| format!("CityJSON: {label} must contain three numbers"))?;
    let mut out = [0.; 3];
    for k in 0..3 {
        out[k] = values[k].as_f64().filter(|v| v.is_finite())
            .ok_or_else(|| format!("CityJSON: invalid {label} coordinate"))?;
    }
    Ok(out)
}

fn transform(value: Option<&Value>, fallback: Transform) -> Result<Transform, String> {
    match value {
        None => Ok(fallback),
        Some(v) => {
            let scale = triple(&v["scale"], "transform.scale")?;
            if scale.contains(&0.) { return Err("CityJSON: zero coordinate scale".into()); }
            Ok(Transform { scale, translate: triple(&v["translate"], "transform.translate")? })
        }
    }
}

fn array<'a>(v: &'a Value, label: &str) -> Result<&'a [Value], String> {
    v.as_array().map(Vec::as_slice).ok_or_else(|| format!("CityJSON: invalid {label}"))
}

#[derive(Default)]
struct Scene {
    origin: Option<[f64; 3]>,
    layers: BTreeMap<String, Layer>,
    next_id: u64,
    objects: usize,
    crs: Option<String>,
    ignored_geometry: usize,
    skipped_lods: usize,
    triangles: usize,
    degenerate_surfaces: usize,
}

fn read(bytes: &[u8]) -> Result<Loaded, String> {
    let mut scene = Scene::default();
    let mut inherited = Transform::default();
    let mut documents = 0;
    let stream = serde_json::Deserializer::from_slice(bytes).into_iter::<Value>();
    for document in stream {
        let document = document.map_err(|e| format!("CityJSON: invalid JSON: {e}"))?;
        let kind = document["type"].as_str().unwrap_or("");
        if kind != "CityJSON" && kind != "CityJSONFeature" {
            return Err("CityJSON: expected type CityJSON or CityJSONFeature".into());
        }
        let t = transform(document.get("transform"), inherited)?;
        if kind == "CityJSON" {
            let version = document["version"].as_str().unwrap_or("");
            if !matches!(version, "1.0" | "1.1" | "2.0") {
                return Err(format!("CityJSON: unsupported version {version:?} (supported: 1.0, 1.1, 2.0)"));
            }
            inherited = t;
            if let Some(crs) = document["metadata"]["referenceSystem"].as_str() {
                if scene.crs.as_deref().is_some_and(|current| current != crs) {
                    return Err("CityJSON: sequence contains different coordinate reference systems".into());
                }
                scene.crs = Some(crs.to_owned());
            }
        }
        scene.append(&document, t)?;
        documents += 1;
    }
    if documents == 0 { return Err("CityJSON: empty input".into()); }
    if scene.layers.values().all(|l| l.indices.is_empty()) {
        return Err("CityJSON: no supported polygon geometry (MultiSurface, Solid or MultiSolid) found".into());
    }
    let mut info = vec![
        ("CityJSON objects".into(), scene.objects.to_string()),
        ("CityJSON LoD selection".into(), "Highest supported polygon LoD per object".into()),
        ("CityJSON skipped lower LoD geometries".into(), scene.skipped_lods.to_string()),
        ("CityJSON unsupported geometries".into(), scene.ignored_geometry.to_string()),
        ("CityJSON skipped degenerate surfaces".into(), scene.degenerate_surfaces.to_string()),
    ];
    if let Some(origin) = scene.origin {
        info.push(("CityJSON world origin (source coordinates)".into(), format!("{}, {}, {}", origin[0], origin[1], origin[2])));
    }
    if let Some(crs) = scene.crs { info.push(("Coordinate reference system".into(), crs)); }
    info.push(("Coordinate interpretation".into(), "Source axes retained; local distances assume metres. No CRS reprojection.".into()));
    Ok(Loaded { layers: scene.layers.into_values().collect(), views: Vec::new(), project: ProjectData { info, ..ProjectData::default() } })
}

impl Scene {
    fn append(&mut self, doc: &Value, t: Transform) -> Result<(), String> {
        let Some(objects) = doc.get("CityObjects") else { return Ok(()) }; // Sequence header.
        let objects = objects.as_object().ok_or("CityJSON: CityObjects must be an object")?;
        let vertices = match doc.get("vertices") {
            Some(value) => array(value, "vertices")?,
            None if objects.is_empty() => return Ok(()),
            None => return Err("CityJSON: missing vertices".into()),
        };
        for (key, object) in objects {
            let Some(geometries) = object.get("geometry") else { continue };
            let geometries = array(geometries, "object.geometry")?;
            let highest = geometries.iter().filter(|g| polygon_geometry(g)).filter_map(lod).reduce(f64::max);
            let object_type = object["type"].as_str().unwrap_or("CityObject");
            self.next_id += 1;
            let id = self.next_id;
            let mut any = false;
            for geometry in geometries {
                if !polygon_geometry(geometry) { self.ignored_geometry += 1; continue; }
                if highest.is_some_and(|max| lod(geometry).is_some_and(|v| v < max)) {
                    self.skipped_lods += 1;
                    continue;
                }
                let kind = geometry["type"].as_str().ok_or("CityJSON: geometry has no type")?;
                let mut surfaces = Vec::new();
                match kind {
                    "MultiSurface" | "CompositeSurface" => surfaces.extend(array(&geometry["boundaries"], "surface boundaries")?.iter()),
                    "Solid" => {
                        for shell in array(&geometry["boundaries"], "solid shells")? {
                            surfaces.extend(array(shell, "shell surfaces")?.iter());
                        }
                    }
                    "MultiSolid" | "CompositeSolid" => {
                        for solid in array(&geometry["boundaries"], "solids")? {
                            for shell in array(solid, "solid shells")? {
                                surfaces.extend(array(shell, "shell surfaces")?.iter());
                            }
                        }
                    }
                    _ => { self.ignored_geometry += 1; continue; }
                }
                let layer = self.layers.entry(object_type.to_owned()).or_insert_with(|| Layer {
                    name: object_type.to_owned(), color: DEFAULT_COLOR, colors: Some(Vec::new()), ..Layer::default()
                });
                let first = u32::try_from(layer.indices.len()).map_err(|_| "CityJSON: too many indices")?;
                let (uniform_material, material_values) = material_values(geometry);
                for (surface_index, surface) in surfaces.iter().enumerate() {
                    let color = material_color(doc, uniform_material.or_else(|| material_values.get(surface_index).copied().flatten()));
                    let before = layer.indices.len();
                    let rendered = append_surface(layer, surface, vertices, t, &mut self.origin, color, 2_000_000 - self.triangles)
                        .map_err(|e| format!("CityJSON object {key}: {e}"))?;
                    if !rendered { self.degenerate_surfaces += 1; }
                    self.triangles += (layer.indices.len() - before) / 3;
                }
                let end = u32::try_from(layer.indices.len()).map_err(|_| "CityJSON: too many indices")?;
                if end > first {
                    layer.elements.push((id, first, end - first));
                    let mut props = object.get("attributes").and_then(Value::as_object).cloned().unwrap_or_default();
                    props.insert("CityObjectId".into(), key.clone().into());
                    props.insert("CityObjectType".into(), object_type.into());
                    props.insert("Name".into(), object["attributes"]["name"].as_str().unwrap_or(key).into());
                    if let Some(value) = geometry.get("lod") { props.insert("LoD".into(), value.clone()); }
                    for field in ["parents", "children"] {
                        if let Some(value) = object.get(field) { props.insert(field.into(), value.clone()); }
                    }
                    layer.props.push((id, Value::Object(props).to_string()));
                    any = true;
                }
            }
            if any { self.objects += 1; }
        }
        Ok(())
    }
}

fn lod(geometry: &Value) -> Option<f64> {
    geometry["lod"].as_f64().or_else(|| geometry["lod"].as_str()?.parse().ok()).filter(|v| v.is_finite())
}

fn polygon_geometry(geometry: &Value) -> bool {
    matches!(geometry["type"].as_str(), Some("MultiSurface" | "CompositeSurface" | "Solid" | "MultiSolid" | "CompositeSolid"))
}

// Flatten material indices once per geometry, rather than once per polygon.
fn material_values(geometry: &Value) -> (Option<u64>, Vec<Option<u64>>) {
    let Some(theme) = geometry["material"].as_object().and_then(|t| t.values().next()) else { return (None, Vec::new()) };
    fn flatten(value: &Value, out: &mut Vec<Option<u64>>) {
        if let Some(a) = value.as_array() { for v in a { flatten(v, out); } } else { out.push(value.as_u64()); }
    }
    let mut values = Vec::new();
    if let Some(value) = theme.get("values") { flatten(value, &mut values); }
    (theme.get("value").and_then(Value::as_u64), values)
}

fn material_color(doc: &Value, value: Option<u64>) -> [u8; 4] {
    let default = [184, 184, 189, 255];
    let Some(material) = value.and_then(|v| usize::try_from(v).ok()).and_then(|i| doc["appearance"]["materials"].get(i)) else { return default };
    let Some(rgb) = material["diffuseColor"].as_array().filter(|a| a.len() == 3) else { return default };
    let mut color = default;
    for k in 0..3 {
        if let Some(v) = rgb[k].as_f64().filter(|v| v.is_finite()) { color[k] = (v.clamp(0., 1.) * 255.).round() as u8; }
    }
    color
}

fn append_surface(
    layer: &mut Layer, surface: &Value, vertices: &[Value], transform: Transform,
    origin: &mut Option<[f64; 3]>, color: [u8; 4], remaining_triangles: usize,
) -> Result<bool, String> {
    let rings = array(surface, "polygon rings")?;
    if rings.is_empty() { return Err("polygon has no outer ring".into()); }
    let mut points = Vec::<[f64; 3]>::new();
    let mut holes = Vec::new();
    let mut outer_len = 0;
    for (ring_index, ring) in rings.iter().enumerate() {
        let indices = array(ring, "ring indices")?;
        let mut ring_points = Vec::new();
        for value in indices {
            let index = value.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or("invalid vertex index")?;
            let v = vertices.get(index).ok_or_else(|| format!("vertex index {index} out of range"))?;
            let raw = triple(v, "vertex")?;
            let world = std::array::from_fn(|k| raw[k] * transform.scale[k] + transform.translate[k]);
            if world.iter().any(|v| !v.is_finite()) { return Err("non-finite transformed vertex".into()); }
            if ring_points.last() != Some(&world) { ring_points.push(world); }
        }
        if ring_points.len() > 1 && ring_points.first() == ring_points.last() { ring_points.pop(); }
        if ring_points.len() < 3 { return Ok(false); }
        if ring_index == 0 { outer_len = ring_points.len(); } else { holes.push(points.len()); }
        points.extend(ring_points);
    }
    let anchor = points[0];
    // Calculate in a local frame to avoid cancellation at national-grid coordinates.
    let local: Vec<[f64; 3]> = points.iter().map(|p| std::array::from_fn(|k| p[k] - anchor[k])).collect();
    let mut normal = [0.; 3];
    for i in 0..outer_len {
        let a = local[i];
        let b = local[(i + 1) % outer_len];
        normal[0] += (a[1] - b[1]) * (a[2] + b[2]);
        normal[1] += (a[2] - b[2]) * (a[0] + b[0]);
        normal[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let axis = (0..3).max_by(|&a, &b| normal[a].abs().total_cmp(&normal[b].abs())).unwrap();
    if normal[axis].abs() < 1e-20 { return Ok(false); }
    let axes = match axis { 0 => [1, 2], 1 => [0, 2], _ => [0, 1] };
    let projected: Vec<f64> = local.iter().flat_map(|p| [p[axes[0]], p[axes[1]]]).collect();
    let triangles = earcutr::earcut(&projected, &holes, 2).map_err(|e| format!("cannot triangulate polygon: {e}"))?;
    if triangles.is_empty() { return Ok(false); }
    if triangles.len() / 3 > remaining_triangles { return Err("scene exceeds the two million triangle import limit".into()); }
    let origin = *origin.get_or_insert(anchor);
    let first_index = layer.indices.len();
    for tri in triangles.chunks_exact(3) {
        let mut tri = [tri[0], tri[1], tri[2]];
        let cross = |indices: [usize; 3]| {
            let a = local[indices[0]]; let b = local[indices[1]]; let c = local[indices[2]];
            let u: [f64; 3] = std::array::from_fn(|k| b[k] - a[k]);
            let v: [f64; 3] = std::array::from_fn(|k| c[k] - a[k]);
            [u[1]*v[2]-u[2]*v[1], u[2]*v[0]-u[0]*v[2], u[0]*v[1]-u[1]*v[0]]
        };
        let mut n = cross(tri);
        if n.iter().zip(normal).map(|(a,b)| a*b).sum::<f64>() < 0. { tri.swap(1, 2); n = cross(tri); }
        let length = n.iter().map(|v| v*v).sum::<f64>().sqrt();
        if !length.is_finite() || length <= 1e-20 { continue; }
        for index in tri {
            let p: [f32; 3] = std::array::from_fn(|k| (points[index][k] - origin[k]) as f32);
            if p.iter().any(|v| !v.is_finite()) { return Err("coordinate exceeds rendering range".into()); }
            let index = u32::try_from(layer.vertex_count()).map_err(|_| "too many vertices")?;
            layer.positions.extend(p);
            layer.normals.extend(n.map(|v| (v / length) as f32));
            layer.indices.push(index);
            layer.colors.as_mut().unwrap().push(color);
        }
    }
    Ok(layer.indices.len() > first_index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn document() -> Value {
        json!({"type":"CityJSON","version":"2.0", "transform":{"scale":[0.001,0.001,0.001],"translate":[120000.,480000.,0.]},
            "metadata":{"referenceSystem":"https://www.opengis.net/def/crs/EPSG/0/7415"},
            "vertices":[[0,0,0],[10000,0,0],[10000,10000,0],[0,10000,0],[3000,3000,0],[7000,3000,0],[7000,7000,0],[3000,7000,0]],
            "CityObjects":{"building-1":{"type":"Building","attributes":{"name":"Courtyard","year":1930},"geometry":[
                {"type":"MultiSurface","lod":"2.2","boundaries":[[[0,1,2,3],[4,5,6,7]]]}
            ]}}})
    }

    fn load(value: &Value) -> Loaded { read(value.to_string().as_bytes()).unwrap() }
    fn area(layers: &[Layer]) -> f64 {
        layers.iter().map(|l| l.indices.chunks_exact(3).map(|t| {
            let p = |i: u32| glam::DVec3::from_array(std::array::from_fn(|k| l.positions[i as usize*3+k] as f64));
            (p(t[1])-p(t[0])).cross(p(t[2])-p(t[0])).length()/2.
        }).sum::<f64>()).sum()
    }

    #[test]
    fn courtyard_hole_units_georeference_and_properties() {
        let loaded = load(&document());
        assert!((area(&loaded.layers) - 84.).abs() < 1e-8);
        let layer = &loaded.layers[0];
        assert_eq!(layer.name, "Building");
        assert!(layer.positions.iter().all(|v| v.abs() <= 10.));
        assert!(layer.props[0].1.contains("building-1") && layer.props[0].1.contains("Courtyard"));
        assert!(loaded.project.info.iter().any(|(k,v)| k.contains("origin") && v == "120000, 480000, 0"));
        assert!(loaded.project.info.iter().any(|(_,v)| v.ends_with("7415")));
    }

    #[test]
    fn selects_highest_lod_without_duplicate_building() {
        let mut doc = document();
        let mut low = doc["CityObjects"]["building-1"]["geometry"][0].clone();
        low["lod"] = json!(1.2);
        doc["CityObjects"]["building-1"]["geometry"].as_array_mut().unwrap().push(low);
        let loaded = load(&doc);
        assert!((area(&loaded.layers)-84.).abs()<1e-8);
        assert_eq!(loaded.layers[0].elements.len(), 1);
    }

    #[test]
    fn unsupported_higher_lod_keeps_supported_polygons_and_reports_omission() {
        let mut doc = document();
        doc["CityObjects"]["building-1"]["geometry"].as_array_mut().unwrap()
            .push(json!({"type":"GeometryInstance","lod":"3.0","template":0,"boundaries":[0]}));
        let loaded = load(&doc);
        assert!((area(&loaded.layers) - 84.).abs() < 1e-8);
        assert!(loaded.project.info.iter().any(|(k,v)| k == "CityJSON unsupported geometries" && v == "1"));
    }

    #[test]
    fn enforces_remaining_scene_triangle_budget_before_append() {
        let doc = document();
        let mut layer = Layer { colors: Some(Vec::new()), ..Layer::default() };
        let surface = &doc["CityObjects"]["building-1"]["geometry"][0]["boundaries"][0];
        let mut origin = None;
        let result = append_surface(&mut layer, surface, doc["vertices"].as_array().unwrap(), Transform::default(), &mut origin, [255;4], 1);
        assert!(result.unwrap_err().contains("scene exceeds"));
        assert!(layer.positions.is_empty());
    }

    #[test]
    fn vertical_surface_and_material() {
        let mut doc = document();
        doc["vertices"] = json!([[0,0,0],[0,1000,0],[0,1000,2000],[0,0,2000]]);
        doc["CityObjects"]["building-1"]["geometry"][0]["boundaries"] = json!([[[0,1,2,3]]]);
        doc["CityObjects"]["building-1"]["geometry"][0]["material"] = json!({"default":{"value":0}});
        doc["appearance"] = json!({"materials":[{"diffuseColor":[1.,0.,0.]}]});
        let loaded = load(&doc);
        assert!((area(&loaded.layers)-2.).abs()<1e-8);
        assert!(loaded.layers[0].colors.as_ref().unwrap().iter().all(|c| *c == [255,0,0,255]));
    }

    #[test]
    fn feature_sequence_inherits_header_transform() {
        let doc = document();
        let header = json!({"type":"CityJSON","version":"2.0","transform":doc["transform"],"CityObjects":{},"vertices":[]});
        let feature = json!({"type":"CityJSONFeature","vertices":doc["vertices"],"CityObjects":doc["CityObjects"]});
        let loaded = read(format!("{header}\n{feature}\n").as_bytes()).unwrap();
        assert!((area(&loaded.layers)-84.).abs()<1e-8);
    }

    #[test]
    fn rejects_wrong_json_and_bad_indices() {
        assert!(read(br#"{"hello":1}"#).err().unwrap().contains("expected type"));
        let mut doc = document();
        doc["CityObjects"]["building-1"]["geometry"][0]["boundaries"] = json!([[[0,1,999]]]);
        assert!(read(doc.to_string().as_bytes()).err().unwrap().contains("out of range"));
    }

    #[test]
    fn skips_collapsed_surfaces_and_reports_count_without_losing_valid_building() {
        let mut doc = document();
        let boundaries = doc["CityObjects"]["building-1"]["geometry"][0]["boundaries"].as_array_mut().unwrap();
        boundaries.push(json!([[0,1,0]]));
        boundaries.push(json!([[0,0,1]]));
        let loaded = load(&doc);
        assert!((area(&loaded.layers) - 84.).abs() < 1e-8);
        assert!(loaded.project.info.iter().any(|(k,v)| k == "CityJSON skipped degenerate surfaces" && v == "2"));
    }
}
