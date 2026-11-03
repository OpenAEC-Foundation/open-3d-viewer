//! Desktop IFC import through the standalone IFClite processing library.
//! Meshes are in metres, Z up. Preserve the model's world rotation while keeping
//! georeferenced positions near the origin; record the removed anchor in project data.

use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{Layer, ProjectData};
use ifc_lite_processing::{process, run_on_geometry_pool, ProcessingRequest};
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

    fn extensions(&self) -> &'static [&'static str] {
        &["ifc"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        let mut header = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&data);
        while header.first().is_some_and(u8::is_ascii_whitespace) {
            header = &header[1..];
        }
        if !header.starts_with(b"ISO-10303-21;") {
            return Err("IFC: expected an ISO-10303-21 STEP file".into());
        }
        progress.set(Phase::Convert);
        // IFClite supplies a large-stack worker pool for nested placements/CSG.
        // Disable GPU instancing because this viewer consumes flat triangle meshes.
        let result = run_on_geometry_pool(|| {
            process(header, &ProcessingRequest::full_pass().instancing(false), &mut ())
        })
        .map_err(|error| format!("IFC: cannot start geometry processing: {error}"))?;
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
        let project = ProjectData {
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
        Ok(Loaded { layers: layers.into_values().collect(), views: Vec::new(), project })
    }
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
        crate::export::to_bytes(&model, "box.ifc").unwrap()
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
}
