//! DWG/DXF polygon geometry through Hakan's MPL-2.0 acadrust reader.
//!
//! This importer deliberately renders model-space polygon geometry, not a complete CAD
//! drawing. ACIS solids, annotations, curves and external references are counted as omitted.

use super::{Loaded, Loader, Phase, Progress, Source, DEFAULT_COLOR};
use crate::model::{Layer, ProjectData};
use acadrust::{CadDocument, Color, DwgReader, DxfReader, EntityType, Vector3};
use glam::{DMat4, DVec3};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

const MAX_ENTITIES: usize = 250_000;
const MAX_TRIANGLES: usize = 2_000_000;
const MAX_DEPTH: usize = 32;

pub struct CadLoader;

impl Loader for CadLoader {
    fn name(&self) -> &'static str { "DWG/DXF" }
    fn extensions(&self) -> &'static [&'static str] { &["dwg", "dxf"] }
    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }
    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?.into_owned();
        let document = if source.extension() == "dwg" {
            DwgReader::from_stream(Cursor::new(data)).read()
        } else {
            DxfReader::from_reader(Cursor::new(data)).and_then(DxfReader::read)
        }.map_err(|error| format!("DWG/DXF: {error}"))?;
        progress.set(Phase::Convert);
        convert(&document)
    }
}

struct Builder<'a> {
    document: &'a CadDocument,
    layers: BTreeMap<String, Layer>,
    origin: Option<DVec3>,
    scale: f64,
    next_id: u64,
    visited: usize,
    triangles: usize,
    omitted: BTreeMap<String, usize>,
}

struct Placement {
    transform: DMat4,
    layer: Option<String>,
    color: [u8; 4],
    hidden: bool,
}

fn convert(document: &CadDocument) -> Result<Loaded, String> {
    let mut builder = Builder {
        document, layers: BTreeMap::new(), origin: None,
        scale: metres_per_unit(document.header.insertion_units), next_id: 1,
        visited: 0, triangles: 0, omitted: BTreeMap::new(),
    };
    for entity in document.model_space_entities() {
        builder.entity(entity, &mut BTreeSet::new(), 0, &Placement {
            transform: DMat4::IDENTITY, layer: None, color: [184, 184, 189, 255], hidden: false,
        })?;
    }
    builder.layers.retain(|_, layer| !layer.indices.is_empty());
    let omitted = builder.omitted.iter().map(|(name, count)| format!("{name}: {count}")).collect::<Vec<_>>().join(", ");
    if builder.layers.is_empty() {
        return Err(format!("DWG/DXF: no supported polygon geometry in model space. Supported: 3DFACE, polyface, polygon and MESH faces. ACIS solids and line drawings are not rendered yet. Omitted: {}", if omitted.is_empty() { "none" } else { &omitted }));
    }
    let origin = builder.origin.unwrap_or(DVec3::ZERO);
    let mut project = ProjectData::default();
    project.info = vec![
        ("CAD import scope".into(), "Model-space polygon faces and mesh blocks; no ACIS solids or annotations".into()),
        ("CAD insertion units".into(), document.header.insertion_units.to_string()),
        ("CAD metres per file unit".into(), builder.scale.to_string()),
        ("CAD world origin (metres)".into(), format!("{}, {}, {}", origin.x, origin.y, origin.z)),
        ("CAD omitted entities".into(), if omitted.is_empty() { "0".into() } else { omitted }),
    ];
    Ok(Loaded { layers: builder.layers.into_values().collect(), views: Vec::new(), project })
}

impl Builder<'_> {
    fn omit(&mut self, kind: &str) { *self.omitted.entry(kind.to_string()).or_default() += 1; }

    fn entity(&mut self, entity: &EntityType, stack: &mut BTreeSet<String>, depth: usize, parent: &Placement) -> Result<(), String> {
        self.visited += 1;
        if self.visited > MAX_ENTITIES { return Err("DWG/DXF: expanded entity limit exceeded".into()); }
        let common = entity.as_entity().common();
        if common.invisible { self.omit("invisible entity"); return Ok(()); }
        let layer_name = if common.layer == "0" { parent.layer.as_deref().unwrap_or("0") } else { &common.layer };
        let layer_record = self.document.layers.get(layer_name);
        let hidden = parent.hidden || layer_record.is_some_and(|layer| layer.flags.off || layer.flags.frozen);
        let color = if common.color == Color::ByBlock { parent.color }
            else { resolve_color(common.color, layer_record.map(|layer| layer.color)) };
        if let EntityType::Insert(insert) = entity {
            let key = insert.block_name.to_uppercase();
            if depth >= MAX_DEPTH || !stack.insert(key.clone()) {
                return Err("DWG/DXF: cyclic or excessively nested block reference".into());
            }
            // Traverse definitions without EXPLODE: explodability is an editing flag,
            // and decomposing a transformed child INSERT loses shear. Compose full f64
            // affine transforms, and transform only leaf vertices.
            let document = self.document;
            let Some(block) = document.block_records.get(&insert.block_name) else {
                self.omit("INSERT with missing block definition");
                stack.remove(&key);
                return Ok(());
            };
            if insert.is_minsert() || block.flags.is_xref || block.flags.is_xref_overlay
                || block.flags.is_external || !block.xref_path.is_empty() {
                self.omit("INSERT array or external reference");
                stack.remove(&key);
                return Ok(());
            }
            let base = document.get_entity(block.block_entity_handle).and_then(|entity| match entity {
                EntityType::Block(value) => Some(value.base_point), _ => None,
            }).unwrap_or(block.base_point);
            let local = DMat4::from_cols_array_2d(&insert.get_transform().matrix.m).transpose()
                * DMat4::from_translation(-DVec3::new(base.x, base.y, base.z));
            let placement = Placement {
                transform: parent.transform * local, layer: Some(layer_name.to_owned()), color, hidden,
            };
            if block.entity_handles.is_empty() { self.omit("empty INSERT block"); }
            for handle in &block.entity_handles {
                if let Some(child) = document.get_entity(*handle) {
                    self.entity(child, stack, depth + 1, &placement)?;
                } else { self.omit("missing block entity"); }
            }
            stack.remove(&key);
            return Ok(());
        }
        let mut faces = Vec::<(Vec<Vector3>, [u8; 4])>::new();
        match entity {
            EntityType::Face3D(face) => {
                let mut points = vec![face.first_corner, face.second_corner, face.third_corner];
                if face.fourth_corner != face.third_corner && face.fourth_corner != face.first_corner {
                    points.push(face.fourth_corner);
                }
                faces.push((points, color));
            }
            EntityType::Mesh(mesh) => {
                for face in &mesh.faces {
                    let points = face.vertices.iter().map(|&index| mesh.vertices.get(index).copied().ok_or("DWG/DXF: mesh index out of range"))
                        .collect::<Result<Vec<_>, _>>()?;
                    faces.push((points, color));
                }
                if mesh.subdivision_level > 0 { self.omit("MESH subdivision (base cage displayed)"); }
            }
            EntityType::PolyfaceMesh(mesh) => {
                for face in &mesh.faces {
                    let mut points = Vec::new();
                    for index in [face.index1, face.index2, face.index3, face.index4] {
                        if index == 0 { continue; }
                        let index = usize::from(index.unsigned_abs()) - 1;
                        points.push(mesh.vertices.get(index).ok_or("DWG/DXF: polyface index out of range")?.location);
                    }
                    let face_color = match face.color {
                        None => color,
                        Some(Color::ByBlock) => parent.color,
                        Some(value) => resolve_color(value, layer_record.map(|layer| layer.color)),
                    };
                    faces.push((points, face_color));
                }
            }
            EntityType::PolygonMesh(mesh) => {
                let m = usize::try_from(mesh.m_vertex_count).map_err(|_| "DWG/DXF: invalid polygon mesh dimensions")?;
                let n = usize::try_from(mesh.n_vertex_count).map_err(|_| "DWG/DXF: invalid polygon mesh dimensions")?;
                if m.checked_mul(n) != Some(mesh.vertices.len()) { return Err("DWG/DXF: incomplete polygon mesh grid".into()); }
                if m >= 2 && n >= 2 {
                    let rows = if mesh.is_closed_m() { m } else { m - 1 };
                    let columns = if mesh.is_closed_n() { n } else { n - 1 };
                    for row in 0..rows { for column in 0..columns {
                        let at = |r: usize, c: usize| mesh.vertices[(r % m) * n + c % n].location;
                        faces.push((vec![at(row, column), at(row + 1, column), at(row + 1, column + 1), at(row, column + 1)], color));
                    }}
                }
            }
            _ => { self.omit(entity.as_entity().entity_type()); return Ok(()); }
        }
        let id = self.next_id;
        self.next_id += 1;
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut colors = Vec::new();
        for (face, color) in faces {
            let points = face.into_iter().map(|p| parent.transform.transform_point3(DVec3::new(p.x, p.y, p.z)) * self.scale).collect::<Vec<_>>();
            if points.iter().any(|p| !p.is_finite()) { return Err("DWG/DXF: non-finite vertex".into()); }
            for triangle in triangulate(&points)? {
                let [a, b, c] = triangle;
                let normal = (b - a).cross(c - a);
                if normal.length_squared() <= 1e-24 { continue; }
                self.triangles += 1;
                if self.triangles > MAX_TRIANGLES { return Err("DWG/DXF: triangle limit exceeded".into()); }
                let origin = *self.origin.get_or_insert(points[0]);
                let normal = normal.normalize().as_vec3().to_array();
                for p in triangle {
                    let local = (p - origin).as_vec3();
                    if !local.is_finite() { return Err("DWG/DXF: model extent exceeds supported coordinates".into()); }
                    positions.extend(local.to_array());
                    normals.extend(normal);
                    colors.push(color);
                }
            }
        }
        if positions.is_empty() { self.omit("empty or degenerate polygon entity"); return Ok(()); }
        // A source layer may occur through both visible and hidden block instances. Split
        // hidden instances into a separately toggleable layer instead of hiding both.
        let name = if hidden { format!("{layer_name} [hidden]") } else { layer_name.to_owned() };
        let layer = self.layers.entry(name.clone()).or_insert_with(|| Layer {
            name, color: DEFAULT_COLOR, colors: Some(Vec::new()), hidden, ..Layer::default()
        });
        let first = u32::try_from(layer.indices.len()).map_err(|_| "DWG/DXF: layer index limit exceeded")?;
        let base = u32::try_from(layer.vertex_count()).map_err(|_| "DWG/DXF: layer vertex limit exceeded")?;
        let count = u32::try_from(positions.len() / 3).map_err(|_| "DWG/DXF: entity index limit exceeded")?;
        layer.indices.extend((0..count).map(|index| base + index));
        layer.positions.extend(positions);
        layer.normals.extend(normals);
        layer.colors.as_mut().unwrap().extend(colors);
        layer.elements.push((id, first, count));
        layer.props.push((id, serde_json::json!({
            "CAD handle": common.handle.to_string(), "CAD entity": entity.as_entity().entity_type(),
            "Layer": layer_name, "Import": "Polygon geometry"
        }).to_string()));
        Ok(())
    }
}

fn resolve_color(color: Color, layer: Option<Color>) -> [u8; 4] {
    let (r, g, b) = color.rgb().or_else(|| layer.and_then(|value| value.rgb())).unwrap_or((184, 184, 189));
    [r, g, b, 255]
}

fn triangulate(points: &[DVec3]) -> Result<Vec<[DVec3; 3]>, String> {
    let mut points = points.to_vec();
    points.dedup();
    if points.len() > 1 && points.first() == points.last() { points.pop(); }
    if points.len() < 3 { return Ok(Vec::new()); }
    if points.len() == 3 { return Ok(vec![[points[0], points[1], points[2]]]); }
    // Project in double precision relative to the first corner, before narrowing to GPU floats.
    let origin = points[0];
    let normal = (0..points.len()).fold(DVec3::ZERO, |sum, i| sum + (points[i] - origin).cross(points[(i + 1) % points.len()] - origin));
    if normal.length_squared() <= 1e-24 { return Ok(Vec::new()); }
    let axis = if normal.x.abs() >= normal.y.abs() && normal.x.abs() >= normal.z.abs() { 0 }
        else if normal.y.abs() >= normal.z.abs() { 1 } else { 2 };
    let coords = points.iter().flat_map(|p| {
        let p = *p - origin;
        match axis { 0 => [p.y, p.z], 1 => [p.x, p.z], _ => [p.x, p.y] }
    }).collect::<Vec<_>>();
    let indices = earcutr::earcut(&coords, &[], 2).map_err(|error| format!("DWG/DXF: cannot triangulate mesh face: {error}"))?;
    Ok(indices.chunks_exact(3).map(|t| {
        let mut triangle = [points[t[0]], points[t[1]], points[t[2]]];
        if (triangle[1] - triangle[0]).cross(triangle[2] - triangle[0]).dot(normal) < 0.0 { triangle.swap(1, 2); }
        triangle
    }).collect())
}

fn metres_per_unit(unit: i16) -> f64 {
    match unit {
        1 => 0.0254, 2 => 0.3048, 3 => 1609.344, 4 => 0.001, 5 => 0.01, 6 => 1.0,
        7 => 1000.0, 8 => 0.0000000254, 9 => 0.0000254, 10 => 0.9144, 11 => 1e-10,
        12 => 1e-9, 13 => 1e-6, 14 => 0.1, 15 => 10.0, 16 => 100.0,
        17 => 1e9, 18 => 149_597_870_700.0, 19 => 9.4607304725808e15,
        20 => 3.085677581491367e16, 21 => 1200.0 / 3937.0,
        22 => 100.0 / 3937.0, 23 => 3600.0 / 3937.0, 24 => 6336000.0 / 3937.0,
        _ => 1.0, // Unitless drawings keep their coordinates; the unit code is reported.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::{DwgWriter, DxfWriter};
    use acadrust::entities::{Face3D, Insert, Mesh, PolyfaceFace, PolyfaceMesh, PolyfaceVertex, MeshFace};
    use std::sync::Arc;

    fn face(x: f64) -> Face3D {
        Face3D::new(Vector3::new(x, 0.0, 0.0), Vector3::new(x + 1000.0, 0.0, 0.0), Vector3::new(x + 1000.0, 1000.0, 0.0), Vector3::new(x, 1000.0, 0.0))
    }
    fn total(loaded: &Loaded) -> usize { loaded.layers.iter().map(Layer::triangle_count).sum() }

    #[test]
    fn reads_independent_hand_authored_dxf_fixture() {
        let bytes = include_bytes!("../../test-models/cad-faces.dxf");
        let loaded = CadLoader.load_with(&Source::Bytes {
            name: "cad-faces.dxf".into(), data: Arc::new(bytes.to_vec())
        }, &Progress::default()).unwrap();
        assert_eq!(total(&loaded), 2);
        assert_eq!(loaded.layers[0].name, "Panel");
        assert!(loaded.layers[0].colors.as_ref().unwrap().iter().all(|color| *color == [0, 255, 0, 255]));
        assert!(loaded.layers[0].positions.iter().all(|coordinate| coordinate.abs() <= 1.0));
    }

    #[test]
    fn reads_ascii_and_binary_dxf_in_metres_with_color_and_identity() {
        let mut doc = CadDocument::new();
        doc.header.insertion_units = 4;
        let mut geometry = face(120_000_000.0);
        geometry.common.color = Color::from_rgb(210, 40, 60);
        doc.add_entity(EntityType::Face3D(geometry)).unwrap();
        for binary in [false, true] {
            let writer = if binary { DxfWriter::new_binary(&doc) } else { DxfWriter::new(&doc) };
            let mut bytes = Vec::new();
            writer.write_to_writer(&mut bytes).unwrap();
            let loaded = CadLoader.load_with(&Source::Bytes { name: "surface.dxf".into(), data: Arc::new(bytes) }, &Progress::default()).unwrap();
            assert_eq!(total(&loaded), 2);
            let layer = &loaded.layers[0];
            assert!(layer.positions.iter().all(|p| p.abs() <= 1.0));
            assert_eq!(layer.colors.as_ref().unwrap()[0], [210, 40, 60, 255]);
            assert_eq!(layer.elements.len(), 1);
            assert!(loaded.project.info.iter().any(|(k, v)| k.contains("origin") && v.contains("120000")));
        }
    }

    #[test]
    fn reads_native_dwg_polygon_faces() {
        let mut doc = CadDocument::new();
        doc.add_entity(EntityType::Face3D(face(0.0))).unwrap();
        let bytes = DwgWriter::write_to_vec(&doc).unwrap();
        let loaded = CadLoader.load_with(&Source::Bytes { name: "surface.dwg".into(), data: Arc::new(bytes) }, &Progress::default()).unwrap();
        assert_eq!(total(&loaded), 2);
    }

    #[test]
    fn rejects_line_only_drawings_and_reports_omissions_in_mixed_drawings() {
        let mut doc = CadDocument::new();
        doc.add_entity(EntityType::Line(acadrust::Line::from_coords(0., 0., 0., 1., 1., 1.))).unwrap();
        assert!(convert(&doc).err().unwrap().contains("no supported polygon geometry"));
        doc.add_entity(EntityType::Face3D(face(0.0))).unwrap();
        assert!(convert(&doc).unwrap().project.info.iter().any(|(k, v)| k == "CAD omitted entities" && v.contains("LINE: 1")));
    }

    #[test]
    fn concave_mesh_and_negative_polyface_indices_are_supported() {
        let mut doc = CadDocument::new();
        let vertices = vec![Vector3::new(0.,0.,0.), Vector3::new(2.,0.,0.), Vector3::new(2.,2.,0.), Vector3::new(1.,1.,0.), Vector3::new(0.,2.,0.)];
        let mut mesh = Mesh::new();
        mesh.vertices = vertices.clone();
        mesh.faces.push(MeshFace::new(vec![0,1,2,3,4]));
        doc.add_entity(EntityType::Mesh(mesh)).unwrap();
        let polyface = PolyfaceMesh::from_data(vertices.into_iter().map(PolyfaceVertex::new).collect(), vec![PolyfaceFace::triangle(-1,2,4)]);
        doc.add_entity(EntityType::PolyfaceMesh(polyface)).unwrap();
        let loaded = convert(&doc).unwrap();
        assert_eq!(total(&loaded), 4);
    }

    fn add_block(doc: &mut CadDocument, name: &str, geometry: EntityType) {
        let mut record = acadrust::tables::BlockRecord::new(name);
        record.handle = doc.allocate_handle();
        let owner = record.handle;
        doc.block_records.add(record).unwrap();
        let mut geometry = geometry;
        geometry.as_entity_mut().common_mut().owner_handle = owner;
        doc.add_entity(geometry).unwrap();
    }

    #[test]
    fn excludes_unused_definitions_and_expands_nested_insert_transforms() {
        let mut doc = CadDocument::new();
        doc.header.insertion_units = 4;
        add_block(&mut doc, "unused", EntityType::Face3D(face(10_000.0)));
        add_block(&mut doc, "panel", EntityType::Face3D(face(0.0)));
        let mut inner = Insert::new("panel", Vector3::new(2000., 0., 0.));
        inner.set_x_scale(2.0);
        add_block(&mut doc, "nested", EntityType::Insert(inner));
        let mut outer = Insert::new("nested", Vector3::new(50_000., 60_000., 7000.));
        outer.rotation = std::f64::consts::FRAC_PI_2;
        doc.add_entity(EntityType::Insert(outer)).unwrap();
        let loaded = convert(&doc).unwrap();
        assert_eq!(total(&loaded), 2);
        let positions = &loaded.layers[0].positions;
        let min = positions.chunks_exact(3).fold([f32::INFINITY;3], |mut a,p| { for k in 0..3 { a[k] = a[k].min(p[k]); } a });
        let max = positions.chunks_exact(3).fold([f32::NEG_INFINITY;3], |mut a,p| { for k in 0..3 { a[k] = a[k].max(p[k]); } a });
        assert!((max[0] - min[0] - 1.0).abs() < 1e-5);
        assert!((max[1] - min[1] - 2.0).abs() < 1e-5);
    }

    #[test]
    fn polyface_bylayer_color_uses_resolved_entity_layer_color() {
        let mut doc = CadDocument::new();
        doc.layers.get_mut("0").unwrap().color = Color::GREEN;
        let mut face = PolyfaceFace::triangle(1, 2, 3);
        face.color = Some(Color::ByLayer);
        let mut mesh = PolyfaceMesh::from_data(vec![
            PolyfaceVertex::new(Vector3::ZERO), PolyfaceVertex::new(Vector3::UNIT_X), PolyfaceVertex::new(Vector3::UNIT_Y),
        ], vec![face]);
        // A per-face ByLayer override must also beat an explicit mesh-level colour.
        mesh.common.color = Color::RED;
        doc.add_entity(EntityType::PolyfaceMesh(mesh)).unwrap();
        let loaded = convert(&doc).unwrap();
        assert!(loaded.layers[0].colors.as_ref().unwrap().iter().all(|color| *color == [0, 255, 0, 255]));
    }

    #[test]
    fn non_explodable_blocks_are_visible_and_nested_shear_is_preserved() {
        let mut doc = CadDocument::new();
        doc.header.insertion_units = 4;
        add_block(&mut doc, "panel", EntityType::Face3D(face(0.0)));
        doc.block_records.get_mut("panel").unwrap().explodable = false;
        let mut inner = Insert::new("panel", Vector3::ZERO);
        inner.rotation = std::f64::consts::FRAC_PI_4;
        add_block(&mut doc, "nested", EntityType::Insert(inner));
        let mut outer = Insert::new("nested", Vector3::ZERO);
        outer.set_x_scale(2.0);
        doc.add_entity(EntityType::Insert(outer)).unwrap();
        let loaded = convert(&doc).unwrap();
        assert_eq!(total(&loaded), 2);
        let points = loaded.layers[0].positions.chunks_exact(3).collect::<Vec<_>>();
        let s = std::f32::consts::FRAC_1_SQRT_2;
        for expected in [[0., 0., 0.], [2.*s, s, 0.], [0., 2.*s, 0.], [-2.*s, s, 0.]] {
            assert!(points.iter().any(|p| (0..3).all(|k| (p[k] - expected[k]).abs() < 1e-5)), "Missing transformed corner {expected:?}: {points:?}");
        }
    }

    #[test]
    fn malformed_mesh_index_and_nonfinite_coordinates_fail() {
        let mut doc = CadDocument::new();
        let mut mesh = Mesh::new();
        mesh.vertices.push(Vector3::ZERO);
        mesh.faces.push(MeshFace::new(vec![0, 1, 2]));
        doc.add_entity(EntityType::Mesh(mesh)).unwrap();
        assert!(convert(&doc).err().unwrap().contains("index out of range"));
        let mut doc = CadDocument::new();
        let mut geometry = face(0.0);
        geometry.first_corner.x = f64::NAN;
        doc.add_entity(EntityType::Face3D(geometry)).unwrap();
        assert!(convert(&doc).err().unwrap().contains("non-finite"));
    }
}
