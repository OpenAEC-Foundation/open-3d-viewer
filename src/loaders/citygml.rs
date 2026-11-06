//! CityGML (.gml, .citygml) 1.0, 2.0 and 3.0: the polygons (`gml:Polygon`, `gml:Triangle`) of
//! each city object, with their holes, from `gml:posList`, `gml:pos` or `gml:coordinates`. One
//! layer per boundary surface type (WallSurface, RoofSurface, GroundSurface, ClosureSurface,
//! windows and doors, ..) or else per city object type (Building, TINRelief, ..); each top-level
//! city object is a pickable element with its id, name, attributes and generic attributes. Only
//! the highest level of detail of an object is drawn (LoD1 blocks or LoD2 surfaces, not both).
//! Coordinates are taken as metres and moved to a local origin (shown with the project
//! information), Z up; no reprojection. Appearances (textures, colours), implicit geometry
//! (library objects placed by reference), xlink references to geometry in other objects and
//! curves are not read.

use super::{Loaded, Loader, Phase, Progress, Source, DEFAULT_COLOR};
use crate::model::{Layer, ProjectData};
use roxmltree::{Document, Node};
use std::collections::BTreeMap;

pub struct CityGmlLoader;

impl Loader for CityGmlLoader {
    fn name(&self) -> &'static str {
        "CityGML"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["gml", "citygml"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|l| l.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        if source.len() > 1024 * 1024 * 1024 {
            return Err("CityGML: file exceeds the 1 GiB import limit".into());
        }
        let data = source.read()?;
        progress.set(Phase::Convert);
        let text = std::str::from_utf8(&data).map_err(|_| "CityGML: the file is not UTF-8 XML")?.trim_start_matches('\u{feff}');
        read(text)
    }
}

/// Boundary surfaces and openings, each a layer of its own.
const SURFACES: &[&str] = &[
    "WallSurface", "RoofSurface", "GroundSurface", "ClosureSurface", "FloorSurface", "OuterFloorSurface",
    "CeilingSurface", "OuterCeilingSurface", "InteriorWallSurface", "Window", "Door",
];

fn layer_color(name: &str) -> [f32; 3] {
    match name {
        "WallSurface" | "InteriorWallSurface" => [0.86, 0.84, 0.80],
        "RoofSurface" => [0.66, 0.30, 0.24],
        "GroundSurface" | "FloorSurface" | "OuterFloorSurface" => [0.45, 0.45, 0.47],
        "ClosureSurface" => [0.70, 0.75, 0.80],
        "Window" => [0.45, 0.62, 0.80],
        "Door" => [0.50, 0.36, 0.24],
        "TINRelief" | "ReliefFeature" => [0.62, 0.70, 0.52],
        "PlantCover" | "SolitaryVegetationObject" => [0.40, 0.62, 0.36],
        "WaterBody" => [0.42, 0.60, 0.80],
        "Road" | "Railway" | "Track" | "Square" | "TransportationComplex" => [0.55, 0.55, 0.57],
        _ => DEFAULT_COLOR,
    }
}

/// The level of detail of a geometry property such as `lod2MultiSurface`.
fn lod(name: &str) -> Option<u8> {
    let digit = name.strip_prefix("lod")?.chars().next()?;
    digit.to_digit(10).map(|d| d as u8)
}

/// The numbers of a ring or position: `posList`, `pos`, `coordinates` (`x,y,z x,y,z`).
fn ring_points(ring: Node) -> Vec<[f64; 3]> {
    let dimension = |n: Node| {
        n.attribute("srsDimension")
            .or_else(|| n.ancestors().find_map(|a| a.attribute("srsDimension")))
            .and_then(|d| d.parse::<usize>().ok())
            .filter(|d| (2..=3).contains(d))
            .unwrap_or(3)
    };
    let numbers = |text: &str| -> Vec<f64> { text.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect() };
    let to_points = |values: Vec<f64>, d: usize| -> Vec<[f64; 3]> {
        values.chunks_exact(d).map(|c| [c[0], c[1], if d == 3 { c[2] } else { 0.0 }]).collect()
    };
    let mut out = Vec::new();
    for c in ring.children().filter(Node::is_element) {
        match c.tag_name().name() {
            "posList" => out.extend(to_points(numbers(c.text().unwrap_or("")), dimension(c))),
            "pos" => {
                let v = numbers(c.text().unwrap_or(""));
                if v.len() >= 2 {
                    out.push([v[0], v[1], v.get(2).copied().unwrap_or(0.0)]);
                }
            }
            "coordinates" => {
                // tuples separated by spaces, numbers by commas
                for tuple in c.text().unwrap_or("").split_whitespace() {
                    let v: Vec<f64> = tuple.split(',').filter_map(|s| s.parse().ok()).collect();
                    if v.len() >= 2 {
                        out.push([v[0], v[1], v.get(2).copied().unwrap_or(0.0)]);
                    }
                }
            }
            "pointProperty" | "pointRep" => {
                if let Some(pos) = c.descendants().find(|n| n.has_tag_name("pos")) {
                    let v = numbers(pos.text().unwrap_or(""));
                    if v.len() >= 2 {
                        out.push([v[0], v[1], v.get(2).copied().unwrap_or(0.0)]);
                    }
                }
            }
            _ => {}
        }
    }
    out.dedup();
    if out.len() > 1 && out.first() == out.last() {
        out.pop();
    }
    out
}

/// The rings of a polygon or triangle: the exterior first, then the holes.
fn polygon_rings(polygon: Node) -> Vec<Vec<[f64; 3]>> {
    let ring_of = |boundary: Node| boundary.children().find(|n| n.is_element()).map(ring_points).unwrap_or_default();
    let mut rings: Vec<Vec<[f64; 3]>> = Vec::new();
    for boundary in polygon.children().filter(|n| n.has_tag_name("exterior") || n.has_tag_name("outerBoundaryIs")) {
        rings.push(ring_of(boundary));
    }
    if rings.is_empty() {
        return rings;
    }
    for boundary in polygon.children().filter(|n| n.has_tag_name("interior") || n.has_tag_name("innerBoundaryIs")) {
        let ring = ring_of(boundary);
        if ring.len() >= 3 {
            rings.push(ring);
        }
    }
    rings
}

/// Triangulates a planar polygon with holes in its best-fitting plane; the triangles and the
/// polygon's unit normal (from the exterior ring's orientation).
fn triangulate(rings: &[Vec<[f64; 3]>]) -> Option<(Vec<[usize; 3]>, [f64; 3])> {
    let outer = rings.first().filter(|r| r.len() >= 3)?;
    let anchor = outer[0];
    let local = |p: &[f64; 3]| [p[0] - anchor[0], p[1] - anchor[1], p[2] - anchor[2]];
    // Newell normal of the exterior
    let mut n = [0.0; 3];
    for i in 0..outer.len() {
        let (a, b) = (local(&outer[i]), local(&outer[(i + 1) % outer.len()]));
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if !(length > 1e-12) {
        return None;
    }
    let n = n.map(|v| v / length);
    let axis = (0..3).max_by(|&a, &b| n[a].abs().total_cmp(&n[b].abs()))?;
    let axes = match axis {
        0 => [1, 2],
        1 => [2, 0],
        _ => [0, 1],
    };
    let mut flat = Vec::new();
    let mut holes = Vec::new();
    for (i, ring) in rings.iter().enumerate() {
        if i > 0 {
            holes.push(flat.len() / 2);
        }
        for p in ring {
            let q = local(p);
            flat.extend([q[axes[0]], q[axes[1]]]);
        }
    }
    let triangles = earcutr::earcut(&flat, &holes, 2).ok()?;
    let points: Vec<[f64; 3]> = rings.iter().flatten().map(local).collect();
    let out = triangles
        .chunks_exact(3)
        .map(|t| {
            let (a, b, c) = (points[t[0]], points[t[1]], points[t[2]]);
            let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
            let cross = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            // the same way round as the exterior ring
            if cross[0] * n[0] + cross[1] * n[1] + cross[2] * n[2] < 0.0 {
                [t[0], t[2], t[1]]
            } else {
                [t[0], t[1], t[2]]
            }
        })
        .collect();
    Some((out, n))
}

/// The id, name and simple attributes of a city object.
fn properties(object: Node) -> serde_json::Map<String, serde_json::Value> {
    let mut props = serde_json::Map::new();
    let gml_id = object.attributes().find(|a| a.name() == "id").map(|a| a.value().to_string());
    let name = object.children().find(|n| n.has_tag_name("name")).and_then(|n| n.text()).map(str::trim);
    props.insert("Name".into(), name.or(gml_id.as_deref()).unwrap_or(object.tag_name().name()).into());
    props.insert("CityObjectType".into(), object.tag_name().name().into());
    if let Some(id) = gml_id {
        props.insert("gml:id".into(), id.into());
    }
    for c in object.children().filter(Node::is_element) {
        let local = c.tag_name().name();
        if local.ends_with("Attribute") {
            // gen:stringAttribute name=".."><gen:value> (1.0/2.0), gen:StringAttribute with
            // gen:name and gen:value (3.0, inside genericAttribute)
            let attribute = if local == "genericAttribute" { c.children().find(Node::is_element) } else { Some(c) };
            let Some(a) = attribute else { continue };
            let key = a.attribute("name").map(str::to_string).or_else(|| a.children().find(|n| n.has_tag_name("name")).and_then(|n| n.text()).map(str::to_string));
            let value = a.children().find(|n| n.has_tag_name("value")).and_then(|n| n.text());
            if let (Some(key), Some(value)) = (key, value) {
                props.insert(key, value.trim().into());
            }
        } else if c.children().all(|n| n.is_text()) && local != "name" && !local.starts_with("lod") {
            if let Some(text) = c.text().map(str::trim).filter(|t| !t.is_empty()) {
                let key = match c.attribute("uom") {
                    Some(uom) => format!("{local} ({uom})"),
                    None => local.to_string(),
                };
                props.entry(key).or_insert(text.into());
            }
        }
    }
    props
}

struct Scene {
    layers: BTreeMap<String, Layer>,
    origin: Option<[f64; 3]>,
    objects: usize,
    degenerate: usize,
    skipped_lods: usize,
}

impl Scene {
    /// Draws the polygons of one top-level city object (highest LoD only).
    fn object(&mut self, object: Node, id: u64) {
        // (layer, LoD, polygon)
        let mut polygons: Vec<(String, Option<u8>, Node)> = Vec::new();
        for polygon in object.descendants().filter(|n| n.has_tag_name("Polygon") || n.has_tag_name("Triangle")) {
            let mut layer = None;
            let mut level = None;
            for a in polygon.ancestors().take_while(|a| *a != object) {
                let name = a.tag_name().name();
                if level.is_none() {
                    level = lod(name);
                }
                if layer.is_none() && SURFACES.contains(&name) {
                    layer = Some(name.to_string());
                }
            }
            polygons.push((layer.unwrap_or_else(|| object.tag_name().name().to_string()), level, polygon));
        }
        let highest = polygons.iter().filter_map(|p| p.1).max();
        let before = polygons.len();
        polygons.retain(|p| p.1.is_none() || p.1 == highest);
        self.skipped_lods += before - polygons.len();
        polygons.sort_by(|a, b| a.0.cmp(&b.0));
        let mut props = Some(serde_json::Value::Object(properties(object)).to_string());
        let mut drawn = false;
        let mut i = 0;
        while i < polygons.len() {
            let key = polygons[i].0.clone();
            let layer = self.layers.entry(key.clone()).or_insert_with(|| Layer { name: key.clone(), color: layer_color(&key), ..Layer::default() });
            let first = layer.indices.len() as u32;
            while i < polygons.len() && polygons[i].0 == key {
                let rings = polygon_rings(polygons[i].2);
                i += 1;
                let Some((triangles, n)) = triangulate(&rings) else {
                    self.degenerate += 1;
                    continue;
                };
                let origin = *self.origin.get_or_insert(rings[0][0]);
                let points: Vec<&[f64; 3]> = rings.iter().flatten().collect();
                for t in triangles {
                    for k in t {
                        let p = points[k];
                        layer.indices.push(layer.vertex_count() as u32);
                        layer.positions.extend([(p[0] - origin[0]) as f32, (p[1] - origin[1]) as f32, (p[2] - origin[2]) as f32]);
                        layer.normals.extend(n.map(|v| v as f32));
                    }
                }
            }
            let count = layer.indices.len() as u32 - first;
            if count > 0 {
                layer.elements.push((id, first, count));
                if let Some(p) = props.take() {
                    layer.props.push((id, p));
                }
                drawn = true;
            }
        }
        if drawn {
            self.objects += 1;
        }
    }
}

fn read(text: &str) -> Result<Loaded, String> {
    let doc = Document::parse_with_options(text, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() }).map_err(|e| format!("CityGML: {e}"))?;
    let root = doc.root_element();
    // the city objects: children of cityObjectMember / featureMember (or the root itself)
    let mut objects: Vec<Node> = root
        .descendants()
        .filter(|n| matches!(n.tag_name().name(), "cityObjectMember" | "featureMember" | "featureMembers" | "member"))
        .filter(|n| n.parent_element() == Some(root))
        .flat_map(|m| m.children().filter(Node::is_element))
        .collect();
    if objects.is_empty() {
        objects.push(root);
    }
    let mut scene = Scene { layers: BTreeMap::new(), origin: None, objects: 0, degenerate: 0, skipped_lods: 0 };
    for (i, object) in objects.into_iter().enumerate() {
        scene.object(object, i as u64 + 1);
    }
    let layers: Vec<Layer> = scene.layers.into_values().filter(|l| !l.indices.is_empty()).collect();
    if layers.is_empty() {
        return Err("CityGML: no polygon geometry found".into());
    }
    let srs = root.descendants().find_map(|n| n.attribute("srsName"));
    let mut info = vec![
        ("CityGML objects".into(), scene.objects.to_string()),
        ("CityGML LoD selection".into(), "Highest level of detail per object".into()),
        ("CityGML skipped lower LoD polygons".into(), scene.skipped_lods.to_string()),
        ("CityGML skipped degenerate polygons".into(), scene.degenerate.to_string()),
    ];
    if let Some(origin) = scene.origin {
        info.push(("CityGML world origin (source coordinates)".into(), format!("{}, {}, {}", origin[0], origin[1], origin[2])));
    }
    if let Some(srs) = srs {
        info.push(("Coordinate reference system".into(), srs.to_string()));
    }
    info.push(("Coordinate interpretation".into(), "Source axes retained; local distances assume metres. No CRS reprojection.".into()));
    Ok(Loaded { structural: Vec::new(), origin: None, layers, views: Vec::new(), project: ProjectData { info, ..ProjectData::default() } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A 10 x 8 x 6 m building in LoD1 (a box) and LoD2 (a ground, a wall with a window hole and
    /// a flat roof), in RD New coordinates.
    const GML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0" xmlns:bldg="http://www.opengis.net/citygml/building/2.0"
    xmlns:gen="http://www.opengis.net/citygml/generics/2.0" xmlns:gml="http://www.opengis.net/gml">
  <gml:boundedBy><gml:Envelope srsName="urn:ogc:def:crs,crs:EPSG::28992,crs:EPSG::5109" srsDimension="3">
    <gml:lowerCorner>120000 480000 0</gml:lowerCorner><gml:upperCorner>120010 480008 6</gml:upperCorner></gml:Envelope></gml:boundedBy>
  <core:cityObjectMember>
    <bldg:Building gml:id="b1">
      <gml:name>Library</gml:name>
      <gen:stringAttribute name="architect"><gen:value>Jo</gen:value></gen:stringAttribute>
      <bldg:measuredHeight uom="m">6</bldg:measuredHeight>
      <bldg:lod1Solid><gml:Solid><gml:exterior><gml:CompositeSurface><gml:surfaceMember><gml:Polygon>
        <gml:exterior><gml:LinearRing><gml:posList>120000 480000 6 120010 480000 6 120010 480008 6 120000 480008 6 120000 480000 6</gml:posList></gml:LinearRing></gml:exterior>
      </gml:Polygon></gml:surfaceMember></gml:CompositeSurface></gml:exterior></gml:Solid></bldg:lod1Solid>
      <bldg:boundedBy><bldg:GroundSurface><bldg:lod2MultiSurface><gml:MultiSurface><gml:surfaceMember><gml:Polygon>
        <gml:exterior><gml:LinearRing><gml:posList>120000 480000 0 120000 480008 0 120010 480008 0 120010 480000 0 120000 480000 0</gml:posList></gml:LinearRing></gml:exterior>
      </gml:Polygon></gml:surfaceMember></gml:MultiSurface></bldg:lod2MultiSurface></bldg:GroundSurface></bldg:boundedBy>
      <bldg:boundedBy><bldg:WallSurface><bldg:lod2MultiSurface><gml:MultiSurface><gml:surfaceMember><gml:Polygon>
        <gml:exterior><gml:LinearRing>
          <gml:pos>120000 480000 0</gml:pos><gml:pos>120010 480000 0</gml:pos><gml:pos>120010 480000 6</gml:pos><gml:pos>120000 480000 6</gml:pos>
        </gml:LinearRing></gml:exterior>
        <gml:interior><gml:LinearRing><gml:posList>120004 480000 2 120004 480000 4 120006 480000 4 120006 480000 2 120004 480000 2</gml:posList></gml:LinearRing></gml:interior>
      </gml:Polygon></gml:surfaceMember></gml:MultiSurface></bldg:lod2MultiSurface></bldg:WallSurface></bldg:boundedBy>
      <bldg:boundedBy><bldg:RoofSurface><bldg:lod2MultiSurface><gml:MultiSurface><gml:surfaceMember><gml:Polygon>
        <gml:exterior><gml:LinearRing><gml:coordinates>120000,480000,6 120010,480000,6 120010,480008,6 120000,480008,6</gml:coordinates></gml:LinearRing></gml:exterior>
      </gml:Polygon></gml:surfaceMember></gml:MultiSurface></bldg:lod2MultiSurface></bldg:RoofSurface></bldg:boundedBy>
    </bldg:Building>
  </core:cityObjectMember>
</core:CityModel>"#;

    #[test]
    fn surfaces_as_layers_highest_lod_and_attributes() {
        let source = Source::Bytes { name: "block.GML".into(), data: Arc::new(GML.as_bytes().to_vec()) };
        let model = crate::loaders::load_with(&source, &crate::loaders::Progress::default()).expect("CityGML");
        assert_eq!(model.format, "CityGML");
        let names: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
        // the LoD1 box is left out
        assert_eq!(names, ["GroundSurface", "RoofSurface", "WallSurface"]);
        let ground = &model.layers[0];
        assert_eq!(ground.triangle_count(), 2);
        assert!(ground.normals.chunks_exact(3).all(|n| n[2] < -0.99), "the ground faces down: {:?}", ground.normals);
        assert!(model.layers[1].normals.chunks_exact(3).all(|n| n[2] > 0.99));
        // the wall (60 m²) without its 2 x 2 m window hole, facing -y
        let wall = &model.layers[2];
        let area: f32 = wall.indices.chunks_exact(3).map(|t| {
            let p = |i: u32| glam::Vec3::from_slice(&wall.positions[3 * i as usize..3 * i as usize + 3]);
            (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).length() / 2.0
        }).sum();
        assert!((area - 56.0).abs() < 1e-3, "{area}");
        assert!(wall.normals.chunks_exact(3).all(|n| n[1] < -0.99));
        assert_eq!(model.bounds.size(), [10.0, 8.0, 6.0]);
        let props = model.props.values().next().unwrap();
        assert!(props.contains("Library") && props.contains("\"architect\":\"Jo\"") && props.contains("measuredHeight (m)"), "{props}");
        assert!(model.project.info.iter().any(|(k, v)| k.contains("origin") && v == "120000, 480000, 0"));
        assert!(model.project.info.iter().any(|(_, v)| v.contains("28992")));
    }

    #[test]
    fn rejects_xml_without_polygons() {
        assert!(read("<a><b/></a>").is_err());
    }
}
