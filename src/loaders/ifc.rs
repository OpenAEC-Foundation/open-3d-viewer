//! IFC import through the standalone IFClite processing library, on the desktop and in the browser.
//! The desktop runs the geometry on IFClite's large-stack worker pool; the browser has no threads,
//! so there it runs on the page's own thread (the page waits while a large file converts).
//! Meshes are in metres, Z up. Preserve the model's world rotation while keeping
//! georeferenced positions near the origin; record the removed anchor in project data.
//! A zipped IFC (`.ifczip`, as buildingSMART defines it) opens the first `.ifc` in the archive.

use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{GridLine, Layer, ProjectData};
use ifc_lite_processing::{process, ProcessingRequest};
use std::collections::{BTreeMap, BTreeSet};

pub struct IfcLoader;

/// Geometry that marks out a volume instead of something built. IfcLite meshes these too
/// (openings in a fixed translucent red), but they start hidden on layers of their own.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Helper {
    /// Rooms and zones.
    Space,
    /// Opening and voiding volumes; IfcLite already cuts them from their hosts (IfcRelVoidsElement).
    Opening,
    /// IfcVirtualElement: a boundary without a physical counterpart.
    Virtual,
}

impl Helper {
    fn of(ifc_type: &str) -> Option<Self> {
        let is = |types: &[&str]| types.iter().any(|t| t.eq_ignore_ascii_case(ifc_type));
        if is(&["IfcSpace", "IfcSpatialZone", "IfcExternalSpatialElement"]) {
            Some(Self::Space)
        } else if is(&["IfcOpeningElement", "IfcOpeningStandardCase", "IfcVoidingFeature"]) {
            Some(Self::Opening)
        } else if is(&["IfcVirtualElement"]) {
            Some(Self::Virtual)
        } else {
            None
        }
    }

    fn layer_prefix(self) -> &'static str {
        match self {
            Self::Space => "Spaces",
            Self::Opening => "Openings",
            Self::Virtual => "Virtual elements",
        }
    }
}

impl Loader for IfcLoader {
    fn name(&self) -> &'static str {
        "IFC"
    }

    fn featured(&self) -> Option<u8> {
        Some(0)
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["ifc", "ifczip"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        let data = if data.starts_with(b"PK\x03\x04") { std::borrow::Cow::Owned(unzip_ifc(&data)?) } else { data };
        let mut header = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&data);
        while header.first().is_some_and(u8::is_ascii_whitespace) {
            header = &header[1..];
        }
        if !header.starts_with(b"ISO-10303-21;") {
            return Err("IFC: expected an ISO-10303-21 STEP file".into());
        }
        progress.set(Phase::Convert);
        // Disable GPU instancing because this viewer consumes flat triangle meshes.
        let request = ProcessingRequest::full_pass().instancing(false);
        // IFClite supplies a large-stack worker pool for nested placements/CSG.
        #[cfg(not(target_arch = "wasm32"))]
        let result = ifc_lite_processing::run_on_geometry_pool(|| process(header, &request, &mut ()))
            .map_err(|error| format!("IFC: cannot start geometry processing: {error}"))?;
        // The browser cannot start threads: IFClite's parallel steps fall back to this thread.
        #[cfg(target_arch = "wasm32")]
        let result = process(header, &request, &mut ());
        let world = result.frame.world_mapper();
        let direction = result.frame.direction_mapper();
        let anchor = result.frame.to_world([0.0; 3]);
        let mut layers = BTreeMap::<(Option<Helper>, String), Layer>::new();
        let mut spatial_volume_ids = BTreeSet::new();
        let mut opening_ids = BTreeSet::new();
        for mesh in result.meshes {
            if mesh.indices.is_empty() || mesh.positions.is_empty() {
                continue;
            }
            let helper = Helper::of(&mesh.ifc_type);
            let base_name = mesh.presentation_layer.as_ref().filter(|s| !s.is_empty()).unwrap_or(&mesh.ifc_type);
            let name = match helper {
                Some(helper) => format!("{}: {base_name}", helper.layer_prefix()),
                None => base_name.clone(),
            };
            match helper {
                Some(Helper::Space) => { spatial_volume_ids.insert(mesh.express_id); }
                Some(Helper::Opening | Helper::Virtual) => { opening_ids.insert(mesh.express_id); }
                None => {}
            }
            // A presentation layer may mix physical elements with room or opening volumes.
            // Keep those in separate toggleable layers, retaining all source geometry.
            let layer = layers.entry((helper, name.clone())).or_insert_with(|| Layer {
                name,
                color: [mesh.color[0], mesh.color[1], mesh.color[2]],
                colors: Some(Vec::new()),
                hidden: helper.is_some(),
                ..Layer::default()
            });
            let base = u32::try_from(layer.vertex_count()).map_err(|_| "IFC: layer exceeds the mesh index limit")?;
            let first = u32::try_from(layer.indices.len()).map_err(|_| "IFC: layer exceeds the mesh index limit")?;
            let count = u32::try_from(mesh.indices.len()).map_err(|_| "IFC: element exceeds the mesh index limit")?;
            for point in mesh.positions.chunks_exact(3) {
                let p = world(std::array::from_fn(|k| point[k] as f64 + mesh.origin[k]));
                layer.positions.extend((0..3).map(|k| (p[k] - anchor[k]) as f32));
            }
            for normal in mesh.normals.chunks_exact(3) {
                let n = direction([normal[0] as f64, normal[1] as f64, normal[2] as f64]);
                layer.normals.extend(n.map(|v| v as f32));
            }
            let rgba = mesh.color.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
            layer.colors.as_mut().unwrap().extend(std::iter::repeat_n(rgba, mesh.positions.len() / 3));
            for index in mesh.indices {
                layer.indices.push(base.checked_add(index).ok_or("IFC: layer exceeds the mesh index limit")?);
            }
            let id = u64::from(mesh.express_id);
            layer.elements.push((id, first, count));
            let mut props = mesh.properties.unwrap_or_default();
            props.insert("IfcType".into(), mesh.ifc_type);
            props.insert("ExpressId".into(), mesh.express_id.to_string());
            if let Some(name) = mesh.name {
                props.insert("Name".into(), name);
            }
            if let Some(global_id) = mesh.global_id {
                props.insert("GlobalId".into(), global_id);
            }
            if let Some(material) = mesh.material_name {
                props.insert("Material".into(), material);
            }
            layer.props.push((id, serde_json::to_string(&props).map_err(|e| format!("IFC properties: {e}"))?));
        }
        if layers.is_empty() {
            return Err(format!("IFC: no supported geometry found ({})", result.metadata.schema_version));
        }
        let has_other_geometry = layers.values().any(|layer| !layer.hidden);
        if !has_other_geometry {
            // A file of only spaces or openings is still a useful model; do not open a blank scene.
            for layer in layers.values_mut() { layer.hidden = false; }
        }
        // the storeys, in the frame of the meshes: above the building's placement, less the anchor
        let scale = result.metadata.length_unit_scale.unwrap_or(1.0);
        let building_z = result.building_transform.as_ref().and_then(|m| m.get(14)).copied().unwrap_or(0.0);
        let mut levels: Vec<(String, f32)> =
            storeys(header, scale).into_iter().map(|(name, z)| (name, (building_z + z - anchor[2]) as f32)).collect();
        levels.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        levels.dedup();
        // the grids, as Revit's: at the level nearest the building's zero (peil 0)
        let zero = building_z - anchor[2];
        let ground = levels.iter().map(|l| l.1 as f64).min_by(|a, b| (a - zero).abs().total_cmp(&(b - zero).abs())).unwrap_or(zero);
        let to_scene = |p: [f64; 3]| [(p[0] - anchor[0]) as f32, (p[1] - anchor[1]) as f32, ground as f32];
        let mut grids: Vec<GridLine> = grid_lines(header, scale).into_iter().map(|(name, a, b)| GridLine { name, a: to_scene(a), b: to_scene(b) }).collect();
        // one grid per storey (as Revit writes them) is the same grid: once, to the millimetre
        let mut seen = std::collections::HashSet::new();
        let mm = |p: [f32; 3]| [(p[0] * 1000.0).round() as i64, (p[1] * 1000.0).round() as i64];
        grids.retain(|g| seen.insert((g.name.clone(), mm(g.a), mm(g.b))));
        grids.sort_by(|a, b| a.name.cmp(&b.name));
        let project = ProjectData {
            levels,
            grids,
            info: vec![
                ("IFC schema".into(), result.metadata.schema_version),
                ("IFC world origin (metres)".into(), format!("{}, {}, {}", anchor[0], anchor[1], anchor[2])),
                ("IFC space/zone elements".into(), spatial_volume_ids.len().to_string()),
                ("IFC hidden space/zone elements".into(), if has_other_geometry { spatial_volume_ids.len() } else { 0 }.to_string()),
                ("IFC space visibility".into(), if has_other_geometry {
                    "Spaces and spatial zones start hidden; enable their layers to inspect them."
                } else { "Only spatial volumes found; their layers start visible." }.into()),
                ("IFC opening/virtual elements".into(), opening_ids.len().to_string()),
                ("IFC hidden opening/virtual elements".into(), if has_other_geometry { opening_ids.len() } else { 0 }.to_string()),
            ],
            ..ProjectData::default()
        };
        // the anchor subtracted above: it places this model when models are combined
        let origin = Some(glam::DVec3::from_array(anchor));
        Ok(Loaded { structural: Vec::new(), origin, layers: layers.into_values().collect(), views: Vec::new(), project })
    }
}

/// The first `.ifc` file in an `.ifczip` archive (the archive holds one, sometimes in a folder).
fn unzip_ifc(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|e| format!("IFC zip: {e}"))?;
    let index = (0..archive.len())
        .find(|&i| archive.name_for_index(i).and_then(Result::ok).is_some_and(|name| name.to_lowercase().ends_with(".ifc")))
        .ok_or("IFC zip: the archive holds no .ifc file")?;
    let mut file = archive.by_index(index).map_err(|e| format!("IFC zip: {e}"))?;
    let mut out = Vec::with_capacity(file.size() as usize);
    std::io::Read::read_to_end(&mut file, &mut out).map_err(|e| format!("IFC zip: {e}"))?;
    Ok(out)
}

/// The building storeys in file order: name (else long name) and elevation in metres above the
/// building (`Elevation`, else the Z of the storey's own placement, which is relative to the
/// building as `Elevation` is).
fn storeys(content: &[u8], scale: f64) -> Vec<(String, f64)> {
    use ifc_lite_core::{decode_ifc_string, EntityDecoder, EntityScanner};
    let mut scanner = EntityScanner::new(content);
    let mut decoder = EntityDecoder::new(content);
    let mut out = Vec::new();
    while let Some((id, keyword, start, end)) = scanner.next_entity() {
        if !keyword.eq_ignore_ascii_case("IFCBUILDINGSTOREY") {
            continue;
        }
        let Ok(storey) = decoder.decode_at(start, end) else { continue };
        let text = |i: usize| storey.get_string(i).map(|s| decode_ifc_string(s).trim().to_string()).filter(|s| !s.is_empty());
        let name = text(2).or_else(|| text(7)).unwrap_or_else(|| format!("Storey #{id}"));
        let z = storey.get_float(9).or_else(|| placement_z(&mut decoder, storey.get_ref(5)?));
        if let Some(z) = z.map(|z| z * scale).filter(|z| z.is_finite()) {
            out.push((name, z));
        }
    }
    out
}

/// The axes of the file's grids (`IfcGrid`): each its tag and the two ends of its curve in the
/// IFC world (metres), the grid's placement applied. An axis is a polyline (first to last point)
/// or a line trimmed by points or parameters; other curves are left out.
fn grid_lines(content: &[u8], scale: f64) -> Vec<(String, [f64; 3], [f64; 3])> {
    use ifc_lite_core::{decode_ifc_string, EntityDecoder, EntityScanner};
    let mut scanner = EntityScanner::new(content);
    let mut decoder = EntityDecoder::new(content);
    let mut out = Vec::new();
    while let Some((_, keyword, start, end)) = scanner.next_entity() {
        if !keyword.eq_ignore_ascii_case("IFCGRID") {
            continue;
        }
        let Ok(grid) = decoder.decode_at(start, end) else { continue };
        let placement = match ifc_lite_geometry::placement::resolve_object_placement(&grid, &mut decoder) {
            Ok(Some(m)) => glam::DMat4::from_cols_slice(m.as_slice()),
            Ok(None) => glam::DMat4::IDENTITY,
            Err(_) => continue,
        };
        let world = |p: [f64; 3]| (placement.transform_point3(glam::DVec3::from_array(p)) * scale).to_array();
        // the U, V and W axes
        for list in 7..=9 {
            for id in grid.get_refs(list).unwrap_or_default() {
                let Ok(axis) = decoder.decode_by_id(id) else { continue };
                let tag = axis.get_string(0).map(|s| decode_ifc_string(s).trim().to_string()).unwrap_or_default();
                let Some((a, b)) = axis.get_ref(1).and_then(|curve| curve_ends(&mut decoder, curve)) else { continue };
                out.push((tag, world(a), world(b)));
            }
        }
    }
    out
}

/// The two ends of a grid axis's curve in its grid's frame (file units): a polyline's first and
/// last point, or a trimmed line's trims (points, or parameters along its vector).
fn curve_ends(decoder: &mut ifc_lite_core::EntityDecoder, curve: u32) -> Option<([f64; 3], [f64; 3])> {
    use ifc_lite_core::IfcType;
    let point = |decoder: &mut ifc_lite_core::EntityDecoder, id: u32| -> Option<[f64; 3]> {
        let p = decoder.decode_by_id(id).ok()?;
        let c = p.get_list(0)?;
        Some([c.first()?.as_float()?, c.get(1)?.as_float()?, c.get(2).and_then(|v| v.as_float()).unwrap_or(0.0)])
    };
    let curve = decoder.decode_by_id(curve).ok()?;
    match curve.ifc_type {
        IfcType::IfcPolyline => {
            let points = curve.get_refs(0)?;
            Some((point(decoder, *points.first()?)?, point(decoder, *points.last()?)?))
        }
        IfcType::IfcTrimmedCurve => {
            let line = decoder.decode_by_id(curve.get_ref(0)?).ok()?;
            if line.ifc_type != IfcType::IfcLine {
                return None;
            }
            let origin = glam::DVec3::from_array(point(decoder, line.get_ref(0)?)?);
            let vector = decoder.decode_by_id(line.get_ref(1)?).ok()?;
            let ratios = decoder.decode_by_id(vector.get_ref(0)?).ok()?;
            let r = ratios.get_list(0)?;
            let direction = glam::DVec3::new(r.first()?.as_float()?, r.get(1)?.as_float()?, r.get(2).and_then(|v| v.as_float()).unwrap_or(0.0));
            let along = direction.normalize_or_zero() * vector.get_float(1).unwrap_or(1.0);
            let mut trim = |list: usize| -> Option<[f64; 3]> {
                let items = curve.get_list(list)?.to_vec();
                // a point where there is one, else the parameter
                if let Some(id) = items.iter().find_map(|v| v.as_entity_ref()) {
                    return point(decoder, id);
                }
                let t = items.iter().find_map(|v| v.as_float())?;
                Some((origin + along * t).to_array())
            };
            Some((trim(1)?, trim(2)?))
        }
        _ => None,
    }
}

/// The Z of a local placement's own offset (`IfcLocalPlacement` → `IfcAxis2Placement3D` →
/// `IfcCartesianPoint`), in file units.
fn placement_z(decoder: &mut ifc_lite_core::EntityDecoder, placement: u32) -> Option<f64> {
    let placement = decoder.decode_by_id(placement).ok()?;
    let axes = decoder.decode_by_id(placement.get_ref(1)?).ok()?;
    let point = decoder.decode_by_id(axes.get_ref(0)?).ok()?;
    point.get_list(0)?.get(2)?.as_float()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Model;
    use std::sync::Arc;

    fn exported_box() -> Vec<u8> {
        let positions = vec![0.,0.,0., 2.,0.,0., 2.,3.,0., 0.,3.,0., 0.,0.,4., 2.,0.,4., 2.,3.,4., 0.,3.,4.];
        let indices = vec![0,2,1, 0,3,2, 4,5,6, 4,6,7, 0,1,5, 0,5,4, 1,2,6, 1,6,5, 2,3,7, 2,7,6, 3,0,4, 3,4,7];
        let layer = Layer { name: "Box".into(), color: [0.8, 0.2, 0.1], positions, indices, ..Layer::default() };
        let model = Model::new("box.skp".into(), "SketchUp", vec![layer]);
        crate::export::to_bytes(&model, &crate::document::Document::from_model(&model), "box.ifc").unwrap()
    }

    #[test]
    fn imports_exported_mesh_with_units_colors_and_element_identity() {
        let source = Source::Bytes { name: "box.IFC".into(), data: Arc::new(exported_box()) };
        let model = crate::loaders::load_with(&source, &Progress::default()).expect("IFC import");
        assert_eq!(model.format, "IFC");
        assert!(!model.source_y_up);
        assert_eq!(model.triangle_count(), 12);
        for (got, expected) in model.bounds.size().into_iter().zip([2., 3., 4.]) {
            assert!((got - expected).abs() < 0.001, "size {:?}", model.bounds.size());
        }
        assert!(model.props.values().any(|p| p.contains("GlobalId") && p.contains("IfcType")));
        assert!(model.layers.iter().any(|l| l.colors.as_ref().is_some_and(|c| c.iter().any(|v| v[0] > v[1]))));
        // the anchor the loader subtracted is kept as the model's origin, for combining
        let anchor = model.origins[0].anchor.expect("an IFC keeps its anchor");
        let text = format!("{}, {}, {}", anchor.x, anchor.y, anchor.z);
        assert!(model.project.info.iter().any(|(k, v)| k == "IFC world origin (metres)" && *v == text), "{text}");
        assert!(crate::loaders::supported_extensions().contains(&"ifc"));
    }

    #[test]
    fn converts_ifc_millimetres_to_metres() {
        let text = String::from_utf8(exported_box()).unwrap().replace(
            "IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.)", "IFCSIUNIT(*,.LENGTHUNIT.,.MILLI.,.METRE.)",
        );
        let source = Source::Bytes { name: "millimetres.ifc".into(), data: Arc::new(text.into_bytes()) };
        let model = crate::loaders::load_with(&source, &Progress::default()).expect("IFC millimetres");
        for (got, expected) in model.bounds.size().into_iter().zip([0.002, 0.003, 0.004]) {
            assert!((got - expected).abs() < 0.00001, "size {:?}", model.bounds.size());
        }
    }

    #[test]
    fn opens_the_ifc_inside_an_ifczip() {
        let mut zipped = std::io::Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut zipped);
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        writer.start_file("readme.txt", options).unwrap();
        std::io::Write::write_all(&mut writer, b"not the model").unwrap();
        writer.start_file("model/box.ifc", options).unwrap();
        std::io::Write::write_all(&mut writer, &exported_box()).unwrap();
        writer.finish().unwrap();
        let source = Source::Bytes { name: "box.IFCZIP".into(), data: Arc::new(zipped.into_inner()) };
        let model = crate::loaders::load_with(&source, &Progress::default()).expect("ifcZIP import");
        assert_eq!(model.format, "IFC");
        assert_eq!(model.triangle_count(), 12);
        assert!(crate::loaders::supported_extensions().contains(&"ifczip"));
        let mut no_ifc = std::io::Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut no_ifc);
        writer.start_file("readme.txt", options).unwrap();
        writer.finish().unwrap();
        let source = Source::Bytes { name: "empty.ifczip".into(), data: Arc::new(no_ifc.into_inner()) };
        assert!(IfcLoader.load(&source).unwrap_err().contains("no .ifc file"));
    }

    #[test]
    fn rejects_non_ifc_data() {
        let source = Source::Bytes { name: "wrong.ifc".into(), data: Arc::new(b"not IFC".to_vec()) };
        assert!(IfcLoader.load(&source).unwrap_err().contains("STEP file"));
    }

    #[test]
    fn hides_spaces_without_hiding_physical_elements_on_the_same_presentation_layer() {
        let data = include_bytes!("../../test-models/ifc-shared-space-layer.ifc");
        let source = Source::Bytes { name: "shared-space-layer.ifc".into(), data: Arc::new(data.to_vec()) };
        let loaded = IfcLoader.load_with(&source, &Progress::default()).expect("IFC wall and room");
        assert_eq!(loaded.layers.len(), 2);
        let wall = loaded.layers.iter().find(|layer| !layer.hidden).expect("visible physical wall");
        let room = loaded.layers.iter().find(|layer| layer.hidden).expect("hidden room volume");
        assert_eq!(wall.name, "Shared");
        assert_eq!(room.name, "Spaces: Shared");
        assert_eq!(wall.triangle_count(), 12);
        assert_eq!(room.triangle_count(), 12);
        assert!(wall.props.iter().any(|(id, props)| *id == 16 && props.contains("IfcWall")));
        assert!(room.props.iter().any(|(id, props)| *id == 17 && props.contains("IfcSpace")));
        assert_eq!(wall.positions, room.positions);
        assert!(loaded.project.info.iter().any(|(key, value)| key == "IFC hidden space/zone elements" && value == "1"));
    }

    /// Volume enclosed by a closed triangle list (divergence theorem).
    fn enclosed_volume(layer: &Layer) -> f64 {
        let p = |i: u32| std::array::from_fn::<f64, 3, _>(|k| layer.positions[3 * i as usize + k] as f64);
        layer.indices.chunks_exact(3).map(|t| {
            let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
            a[0] * (b[1] * c[2] - b[2] * c[1]) + a[1] * (b[2] * c[0] - b[0] * c[2]) + a[2] * (b[0] * c[1] - b[1] * c[0])
        }).sum::<f64>() / 6.0
    }

    #[test]
    fn hides_opening_volumes_while_their_voids_stay_cut_from_the_host() {
        // Issue #29: openings drew as IfcLite's translucent red overlay on top of their walls.
        let data = include_bytes!("../../test-models/ifc-wall-opening.ifc");
        let source = Source::Bytes { name: "wall-opening.ifc".into(), data: Arc::new(data.to_vec()) };
        let loaded = IfcLoader.load_with(&source, &Progress::default()).expect("IFC wall with opening");
        assert_eq!(loaded.layers.len(), 2);
        let wall = loaded.layers.iter().find(|layer| !layer.hidden).expect("visible physical wall");
        let opening = loaded.layers.iter().find(|layer| layer.hidden).expect("hidden opening volume");
        assert_eq!(wall.name, "Shared");
        assert_eq!(opening.name, "Openings: Shared");
        assert!(wall.props.iter().any(|(id, props)| *id == 16 && props.contains("IfcWall")));
        assert!(opening.props.iter().any(|(id, props)| *id == 24 && props.contains("IfcOpeningElement")));
        // 4 x 0.2 x 3 m wall minus the 1 x 0.2 x 2 m hole; the 1 x 0.4 x 2 m opening itself stays whole.
        assert!((enclosed_volume(wall) - 2.0).abs() < 1e-3, "wall volume {}", enclosed_volume(wall));
        assert!((enclosed_volume(opening) - 0.8).abs() < 1e-3, "opening volume {}", enclosed_volume(opening));
        assert!(loaded.project.info.iter().any(|(key, value)| key == "IFC hidden opening/virtual elements" && value == "1"));
    }

    #[test]
    fn space_only_files_start_visible_instead_of_opening_a_blank_scene() {
        let text = include_str!("../../test-models/ifc-shared-space-layer.ifc")
            .lines().filter(|line| !line.starts_with("#16=IFCWALL(")).collect::<Vec<_>>().join("\n");
        let source = Source::Bytes { name: "spaces-only.ifc".into(), data: Arc::new(text.into_bytes()) };
        let loaded = IfcLoader.load_with(&source, &Progress::default()).expect("space-only IFC");
        assert_eq!(loaded.layers.len(), 1);
        assert!(!loaded.layers[0].hidden);
        assert_eq!(loaded.layers[0].triangle_count(), 12);
        assert!(loaded.layers[0].props.iter().any(|(id, _)| *id == 17));
        assert!(loaded.project.info.iter().any(|(key,value)| key == "IFC hidden space/zone elements" && value == "0"));
    }

    #[test]
    fn tessellates_an_extruded_wall_at_its_ifc_placement() {
        // Independent SPF fixture: a 2 x 3 rectangle extruded 4 m at (10,20,30).
        // This exercises IfcLite's solid/placement path, beyond our own tessellated export.
        let data = br#"ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('ViewDefinition [ReferenceView]'),'2;1');
FILE_NAME('wall.ifc','2026-10-29T00:00:00',('OpenAEC'),('OpenAEC'),'test','test','');
FILE_SCHEMA(('IFC4'));
ENDSEC;
DATA;
#1=IFCCARTESIANPOINT((0.,0.,0.));
#2=IFCDIRECTION((0.,0.,1.));
#3=IFCDIRECTION((1.,0.,0.));
#4=IFCAXIS2PLACEMENT3D(#1,#2,#3);
#5=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-5,#4,$);
#6=IFCSIUNIT(*,.LENGTHUNIT.,$,.METRE.);
#7=IFCUNITASSIGNMENT((#6));
#8=IFCPROJECT('0YvctVUKr0kugbFTf53O9L',$,'Test',$,$,$,$,(#5),#7);
#9=IFCCARTESIANPOINT((10.,20.,30.));
#10=IFCAXIS2PLACEMENT3D(#9,#2,#3);
#11=IFCLOCALPLACEMENT($,#10);
#12=IFCCARTESIANPOINT((0.,0.));
#13=IFCAXIS2PLACEMENT2D(#12,$);
#14=IFCRECTANGLEPROFILEDEF(.AREA.,'Rectangle',#13,2.,3.);
#15=IFCEXTRUDEDAREASOLID(#14,#4,#2,4.);
#16=IFCSHAPEREPRESENTATION(#5,'Body','SweptSolid',(#15));
#17=IFCPRODUCTDEFINITIONSHAPE($,$,(#16));
#18=IFCWALL('1YvctVUKr0kugbFTf53O9L',$,'Extruded wall',$,$,#11,#17,$,.NOTDEFINED.);
ENDSEC;
END-ISO-10303-21;"#;
        let source = Source::Bytes { name: "wall.ifc".into(), data: Arc::new(data.to_vec()) };
        let model = crate::loaders::load_with(&source, &Progress::default()).expect("IFC extrusion");
        assert!(model.triangle_count() >= 12);
        for (got, expected) in model.bounds.min.into_iter().zip([9., 18.5, 30.]) {
            assert!((got - expected).abs() < 0.001, "bounds {:?}", model.bounds);
        }
        for (got, expected) in model.bounds.max.into_iter().zip([11., 21.5, 34.]) {
            assert!((got - expected).abs() < 0.001, "bounds {:?}", model.bounds);
        }
        let props = model.props.get(&18).expect("pickable IFC wall");
        assert!(props.contains("Extruded wall") && props.contains("IfcWall"), "{props}");
    }

    /// A building placed 10 m up its site with two storeys: the ground floor without an
    /// `Elevation` (so its placement's 0 counts) and the first floor at 3 with a wall standing on
    /// it, 3 high, and a grid placed 1 m east of the building: axes 1 and 2 at its x = 0 and 4
    /// (polylines from y = -1 to 6), axis A at its y = 0 (a line trimmed by parameters, x = -1 to
    /// 5), and the same grid again on the first floor, as Revit writes one per storey. Lengths are
    /// given in `unit` (`$` metres, `.MILLI.` millimetres) times `scale`.
    fn storeys_ifc(unit: &str, scale: f64) -> Vec<u8> {
        let l = |v: f64| format!("{:.1}", v * scale);
        format!(
            r#"ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('ViewDefinition [ReferenceView]'),'2;1');
FILE_NAME('storeys.ifc','2026-11-07T00:00:00',('OpenAEC'),('OpenAEC'),'test','test','');
FILE_SCHEMA(('IFC4'));
ENDSEC;
DATA;
#1=IFCCARTESIANPOINT((0.,0.,0.));
#2=IFCDIRECTION((0.,0.,1.));
#3=IFCDIRECTION((1.,0.,0.));
#4=IFCAXIS2PLACEMENT3D(#1,#2,#3);
#5=IFCGEOMETRICREPRESENTATIONCONTEXT($,'Model',3,1.E-5,#4,$);
#6=IFCSIUNIT(*,.LENGTHUNIT.,{unit},.METRE.);
#7=IFCUNITASSIGNMENT((#6));
#8=IFCPROJECT('0YvctVUKr0kugbFTf53O9L',$,'Storeys',$,$,$,$,(#5),#7);
#30=IFCLOCALPLACEMENT($,#4);
#31=IFCSITE('2YvctVUKr0kugbFTf53O9L',$,'Site',$,$,#30,$,$,.ELEMENT.,$,$,$,$,$);
#32=IFCCARTESIANPOINT((0.,0.,{building}));
#33=IFCAXIS2PLACEMENT3D(#32,$,$);
#34=IFCLOCALPLACEMENT(#30,#33);
#35=IFCBUILDING('3YvctVUKr0kugbFTf53O9L',$,'Building',$,$,#34,$,$,.ELEMENT.,$,$,$);
#36=IFCLOCALPLACEMENT(#34,#4);
#37=IFCBUILDINGSTOREY('4YvctVUKr0kugbFTf53O9L',$,'00 Begane grond',$,$,#36,$,$,.ELEMENT.,$);
#38=IFCCARTESIANPOINT((0.,0.,{first}));
#39=IFCAXIS2PLACEMENT3D(#38,$,$);
#40=IFCLOCALPLACEMENT(#34,#39);
#41=IFCBUILDINGSTOREY('5YvctVUKr0kugbFTf53O9L',$,'01 Eerste verdieping \X2\00E9\X0\',$,$,#40,$,$,.ELEMENT.,{first});
#42=IFCRELAGGREGATES('6YvctVUKr0kugbFTf53O9L',$,$,$,#8,(#31));
#43=IFCRELAGGREGATES('7YvctVUKr0kugbFTf53O9L',$,$,$,#31,(#35));
#44=IFCRELAGGREGATES('8YvctVUKr0kugbFTf53O9L',$,$,$,#35,(#41,#37));
#9=IFCLOCALPLACEMENT(#40,#4);
#10=IFCCARTESIANPOINT((0.,0.));
#11=IFCAXIS2PLACEMENT2D(#10,$);
#12=IFCRECTANGLEPROFILEDEF(.AREA.,'Wall',#11,{length},{thickness});
#13=IFCEXTRUDEDAREASOLID(#12,#4,#2,{height});
#14=IFCSHAPEREPRESENTATION(#5,'Body','SweptSolid',(#13));
#15=IFCPRODUCTDEFINITIONSHAPE($,$,(#14));
#16=IFCWALL('1YvctVUKr0kugbFTf53O9L',$,'Wall',$,$,#9,#15,$,.NOTDEFINED.);
#45=IFCRELCONTAINEDINSPATIALSTRUCTURE('9YvctVUKr0kugbFTf53O9L',$,$,$,(#16),#41);
#50=IFCCARTESIANPOINT(({one},0.,0.));
#51=IFCAXIS2PLACEMENT3D(#50,$,$);
#52=IFCLOCALPLACEMENT(#34,#51);
#53=IFCCARTESIANPOINT((0.,{minus_one}));
#54=IFCCARTESIANPOINT((0.,{six}));
#55=IFCPOLYLINE((#53,#54));
#56=IFCCARTESIANPOINT(({four},{minus_one}));
#57=IFCCARTESIANPOINT(({four},{six}));
#58=IFCPOLYLINE((#56,#57));
#59=IFCCARTESIANPOINT((0.,0.));
#60=IFCDIRECTION((2.,0.));
#61=IFCVECTOR(#60,{one});
#62=IFCLINE(#59,#61);
#63=IFCTRIMMEDCURVE(#62,(IFCPARAMETERVALUE(-1.)),(IFCPARAMETERVALUE(5.)),.T.,.PARAMETER.);
#64=IFCGRIDAXIS('1',#55,.T.);
#65=IFCGRIDAXIS('2',#58,.T.);
#66=IFCGRIDAXIS('A',#63,.T.);
#67=IFCGRID('AYvctVUKr0kugbFTf53O9L',$,'Grid',$,$,#52,$,(#64,#65),(#66),$,.RECTANGULAR.);
#68=IFCLOCALPLACEMENT(#40,#51);
#69=IFCGRID('BYvctVUKr0kugbFTf53O9L',$,'Grid',$,$,#68,$,(#64,#65),(#66),$,.RECTANGULAR.);
ENDSEC;
END-ISO-10303-21;"#,
            building = l(10.0),
            first = l(3.0),
            one = l(1.0),
            minus_one = l(-1.0),
            four = l(4.0),
            six = l(6.0),
            length = l(4.0),
            thickness = l(0.2),
            height = l(3.0),
        )
        .into_bytes()
    }

    fn load_bytes(name: &str, data: Vec<u8>) -> crate::model::Model {
        let source = Source::Bytes { name: name.into(), data: Arc::new(data) };
        crate::loaders::load_with(&source, &Progress::default()).expect("IFC import")
    }

    fn assert_levels(model: &crate::model::Model, expected: &[(&str, f32)]) {
        let got = &model.project.levels;
        assert_eq!(got.len(), expected.len(), "{got:?}");
        for ((name, z), (want_name, want_z)) in got.iter().zip(expected) {
            assert_eq!(name, want_name, "{got:?}");
            assert!((z - want_z).abs() < 0.001, "{got:?}");
        }
    }

    #[test]
    fn reads_building_storeys_as_levels_in_the_frame_of_the_meshes() {
        let model = load_bytes("storeys.ifc", storeys_ifc("$", 1.0));
        // the building's 10 m counts; the wall stands on the first floor
        assert_levels(&model, &[("00 Begane grond", 10.0), ("01 Eerste verdieping é", 13.0)]);
        assert!((model.bounds.min[2] - 13.0).abs() < 0.001, "{:?}", model.bounds);
    }

    #[test]
    fn reads_storey_elevations_in_millimetre_files_as_metres() {
        let model = load_bytes("storeys-mm.ifc", storeys_ifc(".MILLI.", 1000.0));
        assert_levels(&model, &[("00 Begane grond", 10.0), ("01 Eerste verdieping é", 13.0)]);
    }

    /// Grids are shown at the level nearest the building's zero (peil 0), as Revit's are.
    #[test]
    fn reads_grids_at_the_level_of_the_buildings_zero_in_metres_and_millimetres() {
        for (name, unit, scale) in [("grids.ifc", "$", 1.0), ("grids-mm.ifc", ".MILLI.", 1000.0)] {
            let model = load_bytes(name, storeys_ifc(unit, scale));
            let grids = &model.project.grids;
            let names: Vec<&str> = grids.iter().map(|g| g.name.as_str()).collect();
            assert_eq!(names, ["1", "2", "A"], "{name}: the first floor's copy is the same grid");
            let close = |p: [f32; 3], q: [f32; 3]| p.iter().zip(q).all(|(a, b)| (a - b).abs() < 0.001);
            let ends = |i: usize| (grids[i].a, grids[i].b);
            assert!(close(ends(0).0, [1.0, -1.0, 10.0]) && close(ends(0).1, [1.0, 6.0, 10.0]), "{name}: {grids:?}");
            assert!(close(ends(1).0, [5.0, -1.0, 10.0]) && close(ends(1).1, [5.0, 6.0, 10.0]), "{name}: {grids:?}");
            assert!(close(ends(2).0, [0.0, 0.0, 10.0]) && close(ends(2).1, [6.0, 0.0, 10.0]), "{name}: {grids:?}");
        }
    }

    #[test]
    fn levels_saved_as_ifc_read_back_as_the_same_levels() {
        let positions = vec![0.,0.,3., 2.,0.,3., 2.,3.,3., 0.,3.,3., 0.,0.,6., 2.,0.,6., 2.,3.,6., 0.,3.,6.];
        let indices = vec![0,2,1, 0,3,2, 4,5,6, 4,6,7, 0,1,5, 0,5,4, 1,2,6, 1,6,5, 2,3,7, 2,7,6, 3,0,4, 3,4,7];
        let layer = Layer { name: "Box".into(), positions, indices, ..Layer::default() };
        let mut model = Model::new("levels.skp".into(), "SketchUp", vec![layer]);
        model.project.levels = vec![("Level 0".into(), 0.0), ("Level 1".into(), 3.0), ("Roof".into(), 6.5)];
        let document = crate::document::Document::from_model(&model);
        let bytes = crate::export::to_bytes(&model, &document, "levels.ifc").unwrap();
        let model = load_bytes("levels.ifc", bytes);
        assert_levels(&model, &[("Level 0", 0.0), ("Level 1", 3.0), ("Roof", 6.5)]);
    }
}

