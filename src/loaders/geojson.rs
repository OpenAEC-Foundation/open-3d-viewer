//! GeoJSON (.geojson): polygons and multipolygons as flat faces (holes kept), extruded into
//! blocks when a feature has a height (`height`, `building:height`, `render_height`, or
//! `building:levels` / `levels` at 3 m a level, with `min_height` / `building:min_level` for the
//! underside); lines as flat ribbons 0.3 m wide; points are skipped. Each feature is a pickable
//! element with its properties; simplestyle `fill` / `stroke` colours are used.
//!
//! Coordinates: longitude/latitude (RFC 7946, WGS 84, or a CRS84 / EPSG:4326 `crs`) are projected
//! to local metres around the first coordinate (equirectangular, good to a few centimetres over
//! a district, not for whole countries); other files (a projected `crs`, or coordinates outside
//! the degree range) are taken as metres, moved to a local origin. A third coordinate is the
//! elevation in metres. The origin and interpretation are shown with the project information.

use super::{Loaded, Loader, Phase, Progress, Source, DEFAULT_COLOR};
use crate::model::{Layer, ProjectData};
use serde_json::Value;
use std::collections::BTreeMap;

pub struct GeoJsonLoader;

impl Loader for GeoJsonLoader {
    fn name(&self) -> &'static str {
        "GeoJSON"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["geojson"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|l| l.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        progress.set(Phase::Convert);
        read(&data)
    }
}

/// Metres per degree of latitude, and of longitude at the equator (WGS 84 mean values).
const METRES_PER_DEGREE_LAT: f64 = 110_574.0;
const METRES_PER_DEGREE_LON: f64 = 111_320.0;
/// Storey height for `building:levels`.
const LEVEL_HEIGHT: f64 = 3.0;
const LINE_WIDTH: f64 = 0.3;

/// How file coordinates become local metres.
pub(super) struct Frame {
    pub geographic: bool,
    /// The first coordinate: longitude/latitude, or x/y in metres.
    pub origin: [f64; 2],
    cos_lat: f64,
}

impl Frame {
    pub fn new(geographic: bool, origin: [f64; 2]) -> Self {
        Self { geographic, origin, cos_lat: origin[1].to_radians().cos() }
    }

    fn local(&self, c: &Value) -> Option<[f64; 3]> {
        let c = c.as_array()?;
        let (x, y) = (c.first()?.as_f64()?, c.get(1)?.as_f64()?);
        let z = c.get(2).and_then(Value::as_f64).unwrap_or(0.0);
        self.local_xyz(x, y, z)
    }

    /// A file coordinate in local metres.
    pub fn local_xyz(&self, x: f64, y: f64, z: f64) -> Option<[f64; 3]> {
        let p = if self.geographic {
            [(x - self.origin[0]) * METRES_PER_DEGREE_LON * self.cos_lat, (y - self.origin[1]) * METRES_PER_DEGREE_LAT, z]
        } else {
            [x - self.origin[0], y - self.origin[1], z]
        };
        p.iter().all(|v| v.is_finite()).then_some(p)
    }
}

/// The first position and the x/y range of all `coordinates` in the document.
fn extent(value: &Value, first: &mut Option<[f64; 2]>, range: &mut [f64; 4]) {
    match value {
        Value::Array(a) if a.len() >= 2 && a[0].is_number() && a[1].is_number() => {
            let (x, y) = (a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0));
            first.get_or_insert([x, y]);
            *range = [range[0].min(x), range[1].min(y), range[2].max(x), range[3].max(y)];
        }
        Value::Array(a) => a.iter().for_each(|v| extent(v, first, range)),
        Value::Object(o) => {
            for (k, v) in o {
                if matches!(k.as_str(), "coordinates" | "geometry" | "geometries" | "features") {
                    extent(v, first, range);
                }
            }
        }
        _ => {}
    }
}

/// Whether coordinates in this x/y range (min x, min y, max x, max y) are longitude/latitude.
pub(super) fn in_degrees(range: [f64; 4]) -> bool {
    range[0] >= -180.0 && range[2] <= 180.0 && range[1] >= -90.0 && range[3] <= 90.0
}

/// A number, or the number at the start of a text such as `"12.5 m"`.
fn number(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| {
        let s = v.as_str()?.trim();
        let end = s.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).unwrap_or(s.len());
        s[..end].parse().ok()
    })
}

/// A `#rrggbb` or `#rgb` colour.
pub(super) fn hex_color(v: Option<&Value>) -> Option<[u8; 4]> {
    let s = v?.as_str()?.trim().strip_prefix('#')?;
    let digits: Vec<u8> = match s.len() {
        3 => s.chars().map(|c| c.to_digit(16).map(|d| d as u8 * 17)).collect::<Option<_>>()?,
        6 => (0..3).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()).collect::<Option<_>>()?,
        _ => return None,
    };
    Some([digits[0], digits[1], digits[2], 255])
}

fn rgba(c: [f32; 3]) -> [u8; 4] {
    let c = c.map(|v| (v * 255.0).round() as u8);
    [c[0], c[1], c[2], 255]
}

/// The underside and top above the ground of an extruded feature, when it has a height.
pub(super) fn heights(props: &serde_json::Map<String, Value>) -> Option<(f64, f64)> {
    let get = |keys: &[&str]| keys.iter().find_map(|k| props.get(*k).and_then(number));
    let top = get(&["height", "building:height", "render_height", "Height", "HEIGHT"])
        .or_else(|| get(&["building:levels", "levels"]).map(|l| l * LEVEL_HEIGHT))?;
    let bottom = get(&["min_height", "building:min_height", "render_min_height"])
        .or_else(|| get(&["building:min_level", "min_level"]).map(|l| l * LEVEL_HEIGHT))
        .unwrap_or(0.0);
    (top > bottom && top.is_finite()).then_some((bottom, top))
}

/// The layers of features (GeoJSON, Shapefile): Areas, Buildings, Lines and Points.
#[derive(Default)]
pub(super) struct Scene {
    layers: BTreeMap<&'static str, Layer>,
    next_id: u64,
    pub points: usize,
    pub skipped: usize,
}

impl Scene {
    fn layer(&mut self, name: &'static str) -> &mut Layer {
        let color = match name {
            "Areas" => [0.80, 0.82, 0.76],
            "Lines" => [0.35, 0.35, 0.38],
            "Points" => [0.85, 0.30, 0.25],
            _ => DEFAULT_COLOR,
        };
        let points = name == "Points";
        self.layers.entry(name).or_insert_with(|| Layer { name: name.into(), color, colors: Some(Vec::new()), points, ..Layer::default() })
    }

    /// Adds a feature drawn by `draw`: a pickable element with properties `props`.
    pub fn feature(&mut self, props: serde_json::Map<String, Value>, draw: impl FnOnce(&mut Self)) {
        self.next_id += 1;
        let element = self.next_id;
        let before: Vec<(&'static str, u32)> = self.layers.iter().map(|(k, l)| (*k, l.indices.len() as u32)).collect();
        draw(self);
        let mut props = Some(props);
        for (name, layer) in self.layers.iter_mut() {
            let first = before.iter().find(|(k, _)| k == name).map_or(0, |b| b.1);
            let count = layer.indices.len() as u32 - first;
            if count == 0 {
                continue;
            }
            layer.elements.push((element, first, count));
            if let Some(p) = props.take() {
                layer.props.push((element, Value::Object(p).to_string()));
            }
        }
    }

    /// A point (local metres), drawn as a dot.
    pub fn point(&mut self, p: [f64; 3], color: Option<[u8; 4]>) {
        let layer = self.layer("Points");
        layer.positions.extend(p.map(|v| v as f32));
        layer.colors.as_mut().unwrap().push(color.unwrap_or([217, 77, 64, 255]));
    }

    /// Triangles in local metres (as a MultiPatch strip or fan) in layer `layer`.
    pub fn triangles(&mut self, layer: &'static str, triangles: &[[[f64; 3]; 3]], color: [u8; 4]) {
        let layer = self.layer(layer);
        for t in triangles {
            let (u, v) = ([0, 1, 2].map(|k| t[1][k] - t[0][k]), [0, 1, 2].map(|k| t[2][k] - t[0][k]));
            let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            let length = n.iter().map(|c| c * c).sum::<f64>().sqrt();
            if length < 1e-12 {
                continue;
            }
            for p in t {
                Self::push(layer, *p, n.map(|c| c / length), color);
            }
        }
    }

    /// The layers, with one colour per layer where all their vertices share it.
    pub fn into_layers(self) -> Vec<Layer> {
        let mut layers: Vec<Layer> = self.layers.into_values().collect();
        for layer in &mut layers {
            // one colour for the whole layer: no vertex colours
            if !layer.points && layer.colors.as_ref().is_some_and(|c| c.windows(2).all(|w| w[0] == w[1])) {
                if let Some(c) = layer.colors.take().and_then(|c| c.first().copied()) {
                    layer.color = [c[0], c[1], c[2]].map(|v| v as f32 / 255.0);
                }
            }
        }
        layers
    }

    fn push(layer: &mut Layer, p: [f64; 3], n: [f64; 3], color: [u8; 4]) {
        layer.indices.push(layer.vertex_count() as u32);
        layer.positions.extend(p.map(|v| v as f32));
        layer.normals.extend(n.map(|v| v as f32));
        layer.colors.as_mut().unwrap().push(color);
    }

    fn geometry(&mut self, frame: &Frame, geometry: &Value, style: &Style, depth: usize) {
        let coordinates = &geometry["coordinates"];
        match geometry["type"].as_str().unwrap_or("") {
            "Polygon" => self.polygon(frame, coordinates, style),
            "MultiPolygon" => coordinates.as_array().into_iter().flatten().for_each(|p| self.polygon(frame, p, style)),
            "LineString" => self.line(frame, coordinates, style),
            "MultiLineString" => coordinates.as_array().into_iter().flatten().for_each(|l| self.line(frame, l, style)),
            "GeometryCollection" if depth < 16 => {
                for g in geometry["geometries"].as_array().into_iter().flatten() {
                    self.geometry(frame, g, style, depth + 1);
                }
            }
            "Point" | "MultiPoint" => self.points += 1,
            _ => self.skipped += 1,
        }
    }

    fn polygon(&mut self, frame: &Frame, rings: &Value, style: &Style) {
        let rings = rings.as_array().into_iter().flatten().map(|r| r.as_array().into_iter().flatten().filter_map(|c| frame.local(c)).collect());
        self.polygon_rings(rings.collect(), style);
    }

    /// A polygon of rings in local metres, the first the outer one: a flat face (with holes), or
    /// a block when `style` has heights.
    pub fn polygon_rings(&mut self, rings: Vec<Vec<[f64; 3]>>, style: &Style) {
        // rings without the closing point; outer counter-clockwise, holes clockwise
        let mut rings: Vec<Vec<[f64; 3]>> = rings
            .into_iter()
            .filter_map(|mut ring| {
                ring.dedup();
                if ring.len() > 1 && ring.first() == ring.last() {
                    ring.pop();
                }
                (ring.len() >= 3).then_some(ring)
            })
            .collect();
        if rings.is_empty() {
            self.skipped += 1;
            return;
        }
        for (i, ring) in rings.iter_mut().enumerate() {
            let area: f64 = (0..ring.len()).map(|k| {
                let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
                a[0] * b[1] - b[0] * a[1]
            }).sum();
            if (area < 0.0) == (i == 0) {
                ring.reverse();
            }
        }
        // triangulated in plan, or a face that is not flat (a wall, a roof) in the plane it
        // leans to most (from its Newell normal)
        let mut normal = [0.0f64; 3];
        let outer = &rings[0];
        for k in 0..outer.len() {
            let (a, b) = (outer[k], outer[(k + 1) % outer.len()]);
            normal[0] += (a[1] - b[1]) * (a[2] + b[2]);
            normal[1] += (a[2] - b[2]) * (a[0] + b[0]);
            normal[2] += (a[0] - b[0]) * (a[1] + b[1]);
        }
        let [nx, ny, nz] = normal.map(f64::abs);
        let (u, v) = match style.heights {
            None if nx > nz && nx >= ny => (1, 2),
            None if ny > nz && ny > nx => (0, 2),
            _ => (0, 1),
        };
        let flat: Vec<f64> = rings.iter().flatten().flat_map(|p| [p[u], p[v]]).collect();
        let mut holes = Vec::new();
        let mut at = 0;
        for ring in &rings[..rings.len() - 1] {
            at += ring.len();
            holes.push(at);
        }
        let Ok(triangles) = earcutr::earcut(&flat, &holes, 2) else {
            self.skipped += 1;
            return;
        };
        let points: Vec<[f64; 3]> = rings.iter().flatten().copied().collect();
        let ground = points.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
        let (layer, color) = match style.heights {
            Some(_) => ("Buildings", style.fill.unwrap_or(rgba(DEFAULT_COLOR))),
            None => ("Areas", style.fill.unwrap_or(rgba([0.80, 0.82, 0.76]))),
        };
        let layer = self.layer(layer);
        let lift = |p: [f64; 3], z: f64| [p[0], p[1], z];
        match style.heights {
            None => {
                for t in triangles.chunks_exact(3) {
                    let tri = [points[t[0]], points[t[1]], points[t[2]]];
                    // 3D polygons (not flat in z) keep their own normal
                    let (u, v) = ([tri[1][0] - tri[0][0], tri[1][1] - tri[0][1], tri[1][2] - tri[0][2]], [tri[2][0] - tri[0][0], tri[2][1] - tri[0][1], tri[2][2] - tri[0][2]]);
                    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                    let length = n.iter().map(|c| c * c).sum::<f64>().sqrt();
                    let n = if length > 1e-12 { n.map(|c| c / length) } else { [0.0, 0.0, 1.0] };
                    let (order, n) = if n[2] < 0.0 { ([0, 2, 1], n.map(|c| -c)) } else { ([0, 1, 2], n) };
                    for k in order {
                        Self::push(layer, tri[k], n, color);
                    }
                }
            }
            Some((bottom, top)) => {
                let (z0, z1) = (ground + bottom, ground + top);
                for t in triangles.chunks_exact(3) {
                    let [a, b, c] = [points[t[0]], points[t[1]], points[t[2]]];
                    // counter-clockwise seen from above for the top
                    let ccw = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) >= 0.0;
                    let (up, down) = if ccw { ([a, b, c], [a, c, b]) } else { ([a, c, b], [a, b, c]) };
                    for p in up {
                        Self::push(layer, lift(p, z1), [0.0, 0.0, 1.0], color);
                    }
                    for p in down {
                        Self::push(layer, lift(p, z0), [0.0, 0.0, -1.0], color);
                    }
                }
                for ring in &rings {
                    for k in 0..ring.len() {
                        let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
                        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
                        let length = (dx * dx + dy * dy).sqrt();
                        if length < 1e-9 {
                            continue;
                        }
                        let n = [dy / length, -dx / length, 0.0];
                        for p in [lift(a, z0), lift(b, z0), lift(b, z1), lift(a, z0), lift(b, z1), lift(a, z1)] {
                            Self::push(layer, p, n, color);
                        }
                    }
                }
            }
        }
    }

    fn line(&mut self, frame: &Frame, line: &Value, style: &Style) {
        self.line_points(line.as_array().into_iter().flatten().filter_map(|c| frame.local(c)).collect(), style);
    }

    /// A line through points in local metres, drawn as a flat ribbon.
    pub fn line_points(&mut self, points: Vec<[f64; 3]>, style: &Style) {
        let color = style.stroke.unwrap_or(rgba([0.35, 0.35, 0.38]));
        let layer = self.layer("Lines");
        for w in points.windows(2) {
            let (a, b) = (w[0], w[1]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let length = (dx * dx + dy * dy).sqrt();
            if length < 1e-9 {
                continue;
            }
            // half the width to the left and right, a centimetre above the ground
            let (ox, oy) = (-dy / length * LINE_WIDTH / 2.0, dx / length * LINE_WIDTH / 2.0);
            let at = |p: [f64; 3], s: f64| [p[0] + s * ox, p[1] + s * oy, p[2] + 0.01];
            for p in [at(a, -1.0), at(b, -1.0), at(b, 1.0), at(a, -1.0), at(b, 1.0), at(a, 1.0)] {
                Self::push(layer, p, [0.0, 0.0, 1.0], color);
            }
        }
    }
}

pub(super) struct Style {
    pub heights: Option<(f64, f64)>,
    pub fill: Option<[u8; 4]>,
    pub stroke: Option<[u8; 4]>,
}

fn read(data: &[u8]) -> Result<Loaded, String> {
    let doc: Value = serde_json::from_slice(data).map_err(|e| format!("GeoJSON: invalid JSON: {e}"))?;
    let empty = serde_json::Map::new();
    let null = Value::Null;
    // (properties, id, geometry) per feature
    let features: Vec<(&serde_json::Map<String, Value>, &Value, &Value)> = match doc["type"].as_str() {
        Some("FeatureCollection") => doc["features"]
            .as_array()
            .ok_or("GeoJSON: FeatureCollection without features")?
            .iter()
            .map(|f| (f["properties"].as_object().unwrap_or(&empty), f.get("id").unwrap_or(&null), &f["geometry"]))
            .collect(),
        Some("Feature") => vec![(doc["properties"].as_object().unwrap_or(&empty), doc.get("id").unwrap_or(&null), &doc["geometry"])],
        Some("Polygon" | "MultiPolygon" | "LineString" | "MultiLineString" | "GeometryCollection" | "Point" | "MultiPoint") => vec![(&empty, &null, &doc)],
        _ => return Err("GeoJSON: expected a FeatureCollection, Feature or geometry".into()),
    };
    let mut first = None;
    let mut range = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    extent(&doc, &mut first, &mut range);
    let origin = first.ok_or("GeoJSON: no coordinates")?;
    // the old `crs` member (GeoJSON 2008): a named CRS other than WGS 84 is projected
    let crs = doc["crs"]["properties"]["name"].as_str().map(str::to_string);
    let named_geographic = crs.as_deref().is_none_or(|c| c.contains("CRS84") || c.ends_with(":4326") || c.ends_with("::4326"));
    let geographic = named_geographic && in_degrees(range);
    let frame = Frame::new(geographic, origin);
    let mut scene = Scene::default();
    for (props, id, geometry) in features {
        if geometry.is_null() {
            continue;
        }
        let style = Style { heights: heights(props), fill: hex_color(props.get("fill")), stroke: hex_color(props.get("stroke")) };
        let mut p = props.clone();
        if !id.is_null() {
            p.insert("Feature id".into(), id.clone());
        }
        if let Some((bottom, top)) = style.heights {
            p.insert("Extruded".into(), format!("{bottom} m to {top} m").into());
        }
        scene.feature(p, |scene| scene.geometry(&frame, geometry, &style, 0));
    }
    let (points, skipped) = (scene.points, scene.skipped);
    let layers = scene.into_layers();
    if layers.iter().all(|l| l.indices.is_empty()) {
        return Err("GeoJSON: no polygons or lines (points are not drawn)".into());
    }
    let mut info = vec![(
        "GeoJSON coordinates".into(),
        if geographic {
            format!("WGS 84 longitude/latitude, projected to local metres around {}, {} (equirectangular)", origin[0], origin[1])
        } else {
            format!("Metres, moved by -{}, -{} to a local origin", origin[0], origin[1])
        },
    )];
    if let Some(crs) = crs {
        info.push(("Coordinate reference system".into(), crs));
    }
    if points > 0 {
        info.push(("GeoJSON points (not drawn)".into(), points.to_string()));
    }
    if skipped > 0 {
        info.push(("GeoJSON skipped geometries".into(), skipped.to_string()));
    }
    Ok(Loaded { layers, views: Vec::new(), project: ProjectData { info, ..ProjectData::default() } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn load(text: &str) -> crate::model::Model {
        let source = Source::Bytes { name: "map.GeoJSON".into(), data: Arc::new(text.as_bytes().to_vec()) };
        crate::loaders::load_with(&source, &crate::loaders::Progress::default()).expect("GeoJSON")
    }

    /// A 10 x 8 m building of three levels (projected, RD New), a courtyard area and a path.
    const PROJECTED: &str = r##"{"type":"FeatureCollection",
      "crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:EPSG::28992"}},
      "features":[
        {"type":"Feature","id":"b1","properties":{"name":"Town hall","building:levels":"3","fill":"#ff0000"},
         "geometry":{"type":"Polygon","coordinates":[[[155000,463000],[155010,463000],[155010,463008],[155000,463008],[155000,463000]]]}},
        {"type":"Feature","properties":{"name":"Square"},
         "geometry":{"type":"Polygon","coordinates":[[[155020,463000],[155020,463010],[155030,463010],[155030,463000],[155020,463000]],
                                                      [[155022,463002],[155028,463002],[155028,463008],[155022,463008],[155022,463002]]]}},
        {"type":"Feature","properties":{"name":"Path"},"geometry":{"type":"LineString","coordinates":[[155000,462990],[155030,462990]]}},
        {"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[155000,463000]}}
      ]}"##;

    #[test]
    fn extrudes_buildings_and_keeps_holes_in_metres() {
        let model = load(PROJECTED);
        assert_eq!(model.format, "GeoJSON");
        let names: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Areas", "Buildings", "Lines"]);
        let building = &model.layers[1];
        assert_eq!(building.color, [1.0, 0.0, 0.0]);
        // top and bottom (2 triangles each) and 4 walls (2 each)
        assert_eq!(building.triangle_count(), 12);
        let z: Vec<f32> = building.positions.chunks_exact(3).map(|p| p[2]).collect();
        assert_eq!(z.iter().cloned().fold(f32::MIN, f32::max), 9.0);
        // walls face outwards: the wall along y = 0 faces -y
        let south = building.positions.chunks_exact(3).zip(building.normals.chunks_exact(3)).find(|(p, n)| p[1] == 0.0 && n[2] == 0.0 && n[1] != 0.0);
        assert_eq!(south.unwrap().1, [0.0, -1.0, 0.0]);
        // the square with its 6 x 6 m hole: 100 - 36 m²
        let areas = &model.layers[0];
        let area: f32 = areas.indices.chunks_exact(3).map(|t| {
            let p = |i: u32| glam::Vec3::from_slice(&areas.positions[3 * i as usize..3 * i as usize + 3]);
            (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).z / 2.0
        }).sum();
        assert!((area - 64.0).abs() < 1e-3, "{area}");
        assert_eq!(model.bounds.min[0], 0.0);
        assert!(model.props.values().any(|p| p.contains("Town hall") && p.contains("\"Feature id\":\"b1\"")));
        assert!(model.project.info.iter().any(|(_, v)| v.contains("28992")));
    }

    #[test]
    fn projects_longitude_latitude_to_metres() {
        // 0.001° east and north of 52° N: about 68.5 m by 110.6 m
        let model = load(r#"{"type":"Polygon","coordinates":[[[5.0,52.0],[5.001,52.0],[5.001,52.001],[5.0,52.001],[5.0,52.0]]]}"#);
        let size = model.bounds.size();
        assert!((size[0] - 68.5).abs() < 0.2, "{size:?}");
        assert!((size[1] - 110.6).abs() < 0.2, "{size:?}");
        assert!(model.project.info.iter().any(|(_, v)| v.contains("WGS 84")));
    }

    #[test]
    fn rejects_points_only_and_other_json() {
        let source = Source::Bytes { name: "p.geojson".into(), data: Arc::new(br#"{"type":"Point","coordinates":[1,2]}"#.to_vec()) };
        assert!(GeoJsonLoader.load(&source).unwrap_err().contains("points"));
        let source = Source::Bytes { name: "x.geojson".into(), data: Arc::new(b"{}".to_vec()) };
        assert!(GeoJsonLoader.load(&source).is_err());
    }
}
