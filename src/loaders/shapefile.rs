//! ESRI Shapefiles (`.shp`) through the `shapefile` and `dbase` crates (MIT, pure Rust), with the
//! attributes of the `.dbf` next to the file (on the desktop; in the browser only the `.shp` is
//! read), the code page from the `.cpg` or the `.dbf`, and the coordinate system named in the
//! `.prj`. Drawn as the GeoJSON reader draws its features: polygons (2D, Z and M) as faces with
//! their holes, extruded into blocks when a record has a height attribute; lines as flat ribbons;
//! MultiPatch triangle strips, fans and rings as faces; points as dots. Each record is a
//! pickable element with its attributes. Longitude/latitude are projected to local metres around
//! the low corner of the file; projected coordinates are taken as metres and moved there.

use super::geojson::{heights, in_degrees, Frame, Scene, Style};
use super::{Loaded, Loader, Phase, Progress, Source, DEFAULT_COLOR};
use crate::model::{Layer, ProjectData};
use ::shapefile::dbase;
use ::shapefile::{Patch, PolygonRing, Shape};
use serde_json::{Map, Value};
use std::io::Cursor;

pub struct ShapefileLoader;

impl Loader for ShapefileLoader {
    fn name(&self) -> &'static str {
        "Shapefile"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["shp"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        // the side files of the shapefile: `roads.dbf` or `roads.DBF` next to `roads.shp`
        let side = |ext: &str| -> Option<Vec<u8>> {
            let path = source.path()?;
            [ext.to_string(), ext.to_ascii_uppercase()].iter().find_map(|e| std::fs::read(path.with_extension(e)).ok())
        };
        let dbf = side("dbf");
        let text = |b: Vec<u8>| String::from_utf8_lossy(&b).into_owned();
        let (prj, cpg) = (side("prj").map(text), side("cpg").map(text));
        progress.set(Phase::Convert);
        read(&data, dbf.as_deref(), prj.as_deref(), cpg.as_deref(), source.path().is_some()).map_err(|e| format!("{}: {e}", source.name()))
    }
}

/// The attributes of a record as JSON values, in the order of the fields.
fn attributes(fields: &[String], record: &dbase::Record) -> Map<String, Value> {
    let mut props = Map::new();
    for name in fields {
        let value = match record.get(name) {
            Some(dbase::FieldValue::Character(Some(s))) => Value::from(s.trim()),
            Some(dbase::FieldValue::Memo(s)) => Value::from(s.trim()),
            Some(dbase::FieldValue::Numeric(Some(v)) | dbase::FieldValue::Double(v) | dbase::FieldValue::Currency(v)) => number(*v),
            Some(dbase::FieldValue::Float(Some(v))) => number(*v as f64),
            Some(dbase::FieldValue::Integer(v)) => Value::from(*v),
            Some(dbase::FieldValue::Logical(Some(v))) => Value::from(*v),
            Some(dbase::FieldValue::Date(Some(d))) => Value::from(format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day())),
            Some(dbase::FieldValue::DateTime(t)) => {
                let (d, t) = (t.date(), t.time());
                Value::from(format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", d.year(), d.month(), d.day(), t.hours(), t.minutes(), t.seconds()))
            }
            _ => continue,
        };
        if value.as_str().is_some_and(str::is_empty) {
            continue;
        }
        props.insert(name.clone(), value);
    }
    props
}

/// A whole number as an integer, else a float.
fn number(v: f64) -> Value {
    if v.fract() == 0.0 && v.abs() < 9e15 {
        Value::from(v as i64)
    } else {
        Value::from(v)
    }
}

/// The name in a WKT coordinate system (`PROJCS["Amersfoort / RD New",...]`) and whether it is
/// geographic (longitude/latitude).
fn coordinate_system(wkt: &str) -> (Option<String>, Option<bool>) {
    let wkt = wkt.trim().trim_start_matches('\u{feff}');
    let name = wkt.split('"').nth(1).map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
    let kind = wkt.split('[').next().unwrap_or("").trim().to_ascii_uppercase();
    let geographic = match kind.as_str() {
        "GEOGCS" | "GEOGCRS" | "GEODCRS" => Some(true),
        "PROJCS" | "PROJCRS" | "COMPD_CS" | "COMPOUNDCRS" => Some(false),
        _ => None,
    };
    (name, geographic)
}

/// Point in polygon (x/y), for the holes of a polygon with several outer rings.
fn inside(p: [f64; 3], ring: &[[f64; 3]]) -> bool {
    let mut inside = false;
    let mut j = ring.len().wrapping_sub(1);
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Rings into polygons: each outer ring with the holes that lie in it.
fn polygons(rings: Vec<(bool, Vec<[f64; 3]>)>) -> Vec<Vec<Vec<[f64; 3]>>> {
    let mut polygons: Vec<Vec<Vec<[f64; 3]>>> = Vec::new();
    let mut holes = Vec::new();
    for (outer, ring) in rings {
        if outer || polygons.is_empty() {
            polygons.push(vec![ring]);
        } else {
            holes.push(ring);
        }
    }
    for hole in holes {
        let Some(&first) = hole.first() else { continue };
        let at = polygons.iter().position(|p| inside(first, &p[0])).unwrap_or(polygons.len() - 1);
        polygons[at].push(hole);
    }
    polygons
}

/// Triangles of a MultiPatch strip or fan.
fn strip(points: &[[f64; 3]], fan: bool) -> Vec<[[f64; 3]; 3]> {
    (2..points.len())
        .map(|k| match (fan, k % 2) {
            (true, _) => [points[0], points[k - 1], points[k]],
            // every other triangle of a strip turns the other way
            (false, 0) => [points[k - 2], points[k - 1], points[k]],
            (false, _) => [points[k - 1], points[k - 2], points[k]],
        })
        .collect()
}

fn rgba(c: [f32; 3]) -> [u8; 4] {
    let c = c.map(|v| (v * 255.0).round() as u8);
    [c[0], c[1], c[2], 255]
}

fn read(shp: &[u8], dbf: Option<&[u8]>, prj: Option<&str>, cpg: Option<&str>, desktop: bool) -> Result<Loaded, String> {
    let mut reader = ::shapefile::ShapeReader::new(Cursor::new(shp)).map_err(|e| format!("Shapefile: {e}"))?;
    let header = reader.header().clone();
    let shapes: Vec<Shape> = reader.iter_shapes().collect::<Result<_, _>>().map_err(|e| format!("Shapefile: {e}"))?;
    // the attributes, one record per shape
    let mut fields = Vec::new();
    let mut records = Vec::new();
    let mut dbf_note = None;
    if let Some(dbf) = dbf {
        let mut builder = dbase::ReaderBuilder::new();
        if let Some(encoding) = cpg.and_then(dbase::encoding::DynEncoding::from_name) {
            builder = builder.with_encoding(encoding);
        }
        match builder.build(Cursor::new(dbf)) {
            Ok(mut table) => {
                fields = table.fields().iter().map(|f| f.name().to_string()).filter(|n| n != "DeletionFlag").collect();
                records = table.iter_records().map_while(Result::ok).collect();
            }
            Err(e) => dbf_note = Some(format!("not read: {e}")),
        }
    }
    let (crs, crs_geographic) = prj.map(coordinate_system).unwrap_or((None, None));
    let (min, max) = (header.bbox.min, header.bbox.max);
    let range = [min.x, min.y, max.x, max.y];
    let geographic = crs_geographic.unwrap_or_else(|| in_degrees(range));
    let origin = if geographic { [min.x, min.y] } else { [min.x.floor(), min.y.floor()] };
    if !origin.iter().all(|v| v.is_finite()) {
        return Err("Shapefile: no extent in the header".into());
    }
    let frame = Frame::new(geographic, origin);
    let local = |x: f64, y: f64, z: f64| frame.local_xyz(x, y, if z.is_finite() && z.abs() < 1e30 { z } else { 0.0 });
    let mut scene = Scene::default();
    let (mut nulls, mut skipped_patches) = (0, 0);
    for (index, shape) in shapes.iter().enumerate() {
        let mut props = records.get(index).map(|r| attributes(&fields, r)).unwrap_or_default();
        // heights from attributes in any case: `HEIGHT`, `Height`, `height`
        let lower: Map<String, Value> = props.iter().map(|(k, v)| (k.to_ascii_lowercase(), v.clone())).collect();
        let style = Style { heights: heights(&lower), fill: None, stroke: None };
        props.insert("Record".into(), Value::from(index + 1));
        if let Some((bottom, top)) = style.heights {
            props.insert("Extruded".into(), format!("{bottom} m to {top} m").into());
        }
        let xy = |p: &::shapefile::Point| local(p.x, p.y, 0.0);
        let xym = |p: &::shapefile::PointM| local(p.x, p.y, 0.0);
        let xyz = |p: &::shapefile::PointZ| local(p.x, p.y, p.z);
        // polygons: (outer, points) per ring
        let rings: Option<Vec<(bool, Vec<[f64; 3]>)>> = match shape {
            Shape::Polygon(p) => Some(p.rings().iter().map(|r| (matches!(r, PolygonRing::Outer(_)), r.points().iter().filter_map(xy).collect())).collect()),
            Shape::PolygonM(p) => Some(p.rings().iter().map(|r| (matches!(r, PolygonRing::Outer(_)), r.points().iter().filter_map(xym).collect())).collect()),
            Shape::PolygonZ(p) => Some(p.rings().iter().map(|r| (matches!(r, PolygonRing::Outer(_)), r.points().iter().filter_map(xyz).collect())).collect()),
            _ => None,
        };
        let lines: Option<Vec<Vec<[f64; 3]>>> = match shape {
            Shape::Polyline(l) => Some(l.parts().iter().map(|p| p.iter().filter_map(xy).collect()).collect()),
            Shape::PolylineM(l) => Some(l.parts().iter().map(|p| p.iter().filter_map(xym).collect()).collect()),
            Shape::PolylineZ(l) => Some(l.parts().iter().map(|p| p.iter().filter_map(xyz).collect()).collect()),
            _ => None,
        };
        let points: Option<Vec<[f64; 3]>> = match shape {
            Shape::Point(p) => Some(xy(p).into_iter().collect()),
            Shape::PointM(p) => Some(xym(p).into_iter().collect()),
            Shape::PointZ(p) => Some(xyz(p).into_iter().collect()),
            Shape::Multipoint(m) => Some(m.points().iter().filter_map(xy).collect()),
            Shape::MultipointM(m) => Some(m.points().iter().filter_map(xym).collect()),
            Shape::MultipointZ(m) => Some(m.points().iter().filter_map(xyz).collect()),
            _ => None,
        };
        if matches!(shape, Shape::NullShape) {
            nulls += 1;
            continue;
        }
        scene.feature(props, |scene| {
            if let Some(rings) = rings {
                for polygon in polygons(rings) {
                    scene.polygon_rings(polygon, &style);
                }
            }
            for line in lines.into_iter().flatten() {
                scene.line_points(line, &style);
            }
            for p in points.into_iter().flatten() {
                scene.point(p, None);
            }
            if let Shape::Multipatch(m) = shape {
                // rings: an outer (or first) ring with the inner rings after it
                let mut rings: Vec<(bool, Vec<[f64; 3]>)> = Vec::new();
                let color = rgba(DEFAULT_COLOR);
                let flush = |scene: &mut Scene, rings: &mut Vec<(bool, Vec<[f64; 3]>)>| {
                    for polygon in polygons(std::mem::take(rings)) {
                        scene.polygon_rings(polygon, &Style { heights: None, fill: Some(color), stroke: None });
                    }
                };
                for patch in m.patches() {
                    let points: Vec<[f64; 3]> = patch.points().iter().filter_map(xyz).collect();
                    match patch {
                        Patch::TriangleStrip(_) => scene.triangles("Buildings", &strip(&points, false), color),
                        Patch::TriangleFan(_) => scene.triangles("Buildings", &strip(&points, true), color),
                        Patch::OuterRing(_) | Patch::FirstRing(_) => {
                            flush(scene, &mut rings);
                            rings.push((true, points));
                        }
                        Patch::InnerRing(_) | Patch::Ring(_) => rings.push((rings.is_empty(), points)),
                    }
                }
                flush(scene, &mut rings);
                if m.patches().is_empty() {
                    skipped_patches += 1;
                }
            }
        });
    }
    let skipped = scene.skipped + skipped_patches;
    let mut layers = scene.into_layers();
    // MultiPatch rings are drawn as areas: they belong with the buildings
    if let Shape::Multipatch(_) = shapes.iter().find(|s| !matches!(s, Shape::NullShape)).unwrap_or(&Shape::NullShape) {
        for layer in &mut layers {
            if layer.name == "Areas" {
                layer.name = "Buildings".into();
            }
        }
        merge_named(&mut layers);
    }
    if layers.iter().all(|l| l.vertex_count() == 0) {
        return Err("Shapefile: no shapes".into());
    }
    let mut info = vec![
        ("Shape type".into(), header.shape_type.to_string()),
        ("Records".into(), shapes.len().to_string()),
        (
            "Shapefile coordinates".into(),
            if geographic {
                format!("Longitude/latitude, projected to local metres around {}, {} (equirectangular)", origin[0], origin[1])
            } else {
                format!("Metres, moved by -{}, -{} to a local origin", origin[0], origin[1])
            },
        ),
    ];
    if let Some(crs) = crs {
        info.push(("Coordinate reference system".into(), crs));
    }
    info.push((
        "Attributes".into(),
        match (dbf, dbf_note) {
            (_, Some(note)) => format!(".dbf {note}"),
            (Some(_), None) => format!("{} fields from the .dbf", fields.len()),
            (None, None) if desktop => "no .dbf next to the file".into(),
            (None, None) => "not read in the browser (only the .shp)".into(),
        },
    ));
    if nulls > 0 {
        info.push(("Empty records".into(), nulls.to_string()));
    }
    if skipped > 0 {
        info.push(("Skipped shapes".into(), skipped.to_string()));
    }
    Ok(Loaded { layers, views: Vec::new(), project: ProjectData { info, ..ProjectData::default() } })
}

/// Joins layers of the same name (MultiPatch rings and strips both in `Buildings`).
fn merge_named(layers: &mut Vec<Layer>) {
    let mut out: Vec<Layer> = Vec::new();
    for layer in layers.drain(..) {
        let Some(into) = out.iter_mut().find(|l| l.name == layer.name && l.points == layer.points) else {
            out.push(layer);
            continue;
        };
        let base = into.vertex_count() as u32;
        let first = into.indices.len() as u32;
        // vertex colours on both, or neither
        if into.colors.is_none() && layer.colors.is_some() {
            into.colors = Some(vec![rgba(into.color); into.vertex_count()]);
        }
        let colors = layer.colors.clone().unwrap_or_else(|| vec![rgba(layer.color); layer.vertex_count()]);
        if let Some(c) = into.colors.as_mut() {
            c.extend(colors);
        }
        into.positions.extend(&layer.positions);
        into.normals.extend(&layer.normals);
        into.indices.extend(layer.indices.iter().map(|i| i + base));
        into.elements.extend(layer.elements.iter().map(|&(id, f, n)| (id, f + first, n)));
        into.elements.sort_by_key(|e| e.1);
        into.props.extend(layer.props);
    }
    *layers = out;
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::shapefile::{Multipatch, Point, PointZ, Polygon, PolygonZ, Polyline, ShapeWriter};

    fn shp<S: ::shapefile::record::EsriShape>(shapes: &[S]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        let writer = ShapeWriter::new(&mut out);
        // writes the header too
        writer.write_shapes(shapes).unwrap();
        out.into_inner()
    }

    /// A table of `NAME` and `HEIGHT` per record.
    fn dbf(rows: &[(&str, Option<f64>)]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        let mut writer = dbase::TableWriterBuilder::new()
            .add_character_field("NAME".try_into().unwrap(), 20)
            .add_numeric_field("HEIGHT".try_into().unwrap(), 10, 2)
            .build_with_dest(&mut out);
        for (name, height) in rows {
            let mut record = dbase::Record::default();
            record.insert("NAME".into(), dbase::FieldValue::Character(Some(name.to_string())));
            record.insert("HEIGHT".into(), dbase::FieldValue::Numeric(*height));
            writer.write_record(&record).unwrap();
        }
        writer.finalize().unwrap();
        drop(writer);
        out.into_inner()
    }

    fn square(x: f64, y: f64, size: f64) -> Vec<Point> {
        // clockwise: an outer ring in a shapefile
        vec![Point::new(x, y), Point::new(x, y + size), Point::new(x + size, y + size), Point::new(x + size, y), Point::new(x, y)]
    }

    const RD_NEW: &str = r#"PROJCS["Amersfoort / RD New",GEOGCS["Amersfoort",DATUM["Amersfoort",SPHEROID["Bessel 1841",6377397.155,299.1528128]]]]"#;

    #[test]
    fn extrudes_polygons_with_a_height_and_keeps_attributes() {
        // a 10 x 8 m building of 9 m, and a square of 10 m with a 6 m hole and no height
        let building = Polygon::new(PolygonRing::Outer(vec![
            Point::new(155_000.0, 463_000.0),
            Point::new(155_000.0, 463_008.0),
            Point::new(155_010.0, 463_008.0),
            Point::new(155_010.0, 463_000.0),
            Point::new(155_000.0, 463_000.0),
        ]));
        let mut hole = square(155_022.0, 463_002.0, 6.0);
        hole.reverse();
        let square = Polygon::with_rings(vec![PolygonRing::Outer(square(155_020.0, 463_000.0, 10.0)), PolygonRing::Inner(hole)]);
        let loaded = read(&shp(&[building, square]), Some(&dbf(&[("Town hall", Some(9.0)), ("Square", None)])), Some(RD_NEW), None, true).unwrap();
        let names: Vec<&str> = loaded.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Areas", "Buildings"]);
        let building = &loaded.layers[1];
        // top and bottom (2 triangles each) and 4 walls (2 each)
        assert_eq!(building.triangle_count(), 12);
        let top = building.positions.chunks_exact(3).map(|p| p[2]).fold(f32::MIN, f32::max);
        assert_eq!(top, 9.0);
        let props = &building.props[0].1;
        assert!(props.contains("\"NAME\":\"Town hall\"") && props.contains("\"HEIGHT\":9") && props.contains("\"Record\":1"), "{props}");
        let areas = &loaded.layers[0];
        let area: f32 = areas.indices.chunks_exact(3).map(|t| {
            let p = |i: u32| glam::Vec3::from_slice(&areas.positions[3 * i as usize..3 * i as usize + 3]);
            (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).z.abs() / 2.0
        }).sum();
        assert!((area - 64.0).abs() < 1e-3, "{area}");
        let info = &loaded.project.info;
        assert!(info.iter().any(|(k, v)| k == "Coordinate reference system" && v == "Amersfoort / RD New"));
        assert!(info.iter().any(|(k, v)| k == "Attributes" && v == "2 fields from the .dbf"));
        assert!(info.iter().any(|(k, v)| k == "Shape type" && v == "Polygon"));
    }

    #[test]
    fn reads_lines_points_and_3d_polygons() {
        let line = Polyline::new(vec![Point::new(0.5, 0.5), Point::new(30.5, 0.5)]);
        let loaded = read(&shp(&[line]), None, None, None, false).unwrap();
        assert_eq!(loaded.layers[0].name, "Lines");
        assert_eq!(loaded.layers[0].triangle_count(), 2);
        assert!(loaded.project.info.iter().any(|(_, v)| v.contains("browser")));
        let points = read(&shp(&[PointZ::new(5.0, 5.0, 2.0, 0.0), PointZ::new(6.0, 5.0, 3.0, 0.0)]), None, None, None, true).unwrap();
        assert!(points.layers[0].points);
        assert_eq!(points.layers[0].vertex_count(), 2);
        // a vertical wall as a PolygonZ: triangulated in its own plane
        let wall = PolygonZ::new(PolygonRing::Outer(vec![
            PointZ::new(0.0, 0.0, 0.0, 0.0),
            PointZ::new(0.0, 0.0, 3.0, 0.0),
            PointZ::new(4.0, 0.0, 3.0, 0.0),
            PointZ::new(4.0, 0.0, 0.0, 0.0),
            PointZ::new(0.0, 0.0, 0.0, 0.0),
        ]));
        let loaded = read(&shp(&[wall]), None, None, None, true).unwrap();
        assert_eq!(loaded.layers[0].triangle_count(), 2);
        assert_eq!(loaded.layers[0].positions.chunks_exact(3).map(|p| p[2]).fold(0.0, f32::max), 3.0);
    }

    #[test]
    fn multipatch_strips_and_rings_are_buildings() {
        let p = |x, y, z| PointZ::new(x, y, z, 0.0);
        let patch = Multipatch::with_parts(vec![
            // a wall as a strip of two triangles
            Patch::TriangleStrip(vec![p(0.0, 0.0, 0.0), p(0.0, 0.0, 3.0), p(4.0, 0.0, 0.0), p(4.0, 0.0, 3.0)]),
            // a flat roof as an outer ring
            Patch::OuterRing(vec![p(0.0, 0.0, 3.0), p(0.0, 4.0, 3.0), p(4.0, 4.0, 3.0), p(4.0, 0.0, 3.0), p(0.0, 0.0, 3.0)]),
        ]);
        let loaded = read(&shp(&[patch]), None, None, None, true).unwrap();
        let names: Vec<&str> = loaded.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Buildings"]);
        assert_eq!(loaded.layers[0].triangle_count(), 4);
        assert_eq!(loaded.layers[0].elements.len(), 2, "one element in two merged parts");
    }

    #[test]
    fn longitude_latitude_and_a_wgs84_prj() {
        let polygon = Polygon::new(PolygonRing::Outer(vec![
            Point::new(5.0, 52.0),
            Point::new(5.0, 52.001),
            Point::new(5.001, 52.001),
            Point::new(5.001, 52.0),
            Point::new(5.0, 52.0),
        ]));
        let prj = r#"GEOGCS["GCS_WGS_1984",DATUM["D_WGS_1984",SPHEROID["WGS_1984",6378137,298.257223563]]]"#;
        let loaded = read(&shp(&[polygon]), None, Some(prj), None, true).unwrap();
        let p = &loaded.layers[0].positions;
        let max_x = p.chunks_exact(3).map(|p| p[0]).fold(0.0, f32::max);
        assert!((max_x - 68.5).abs() < 0.2, "{max_x}");
        assert!(loaded.project.info.iter().any(|(k, v)| k == "Coordinate reference system" && v == "GCS_WGS_1984"));
    }

    #[test]
    fn rejects_other_files() {
        assert!(read(b"not a shapefile at all, just text that is long enough to be a header .....................................", None, None, None, true).is_err());
    }
}
