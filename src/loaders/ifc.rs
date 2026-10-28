//! Desktop IFC import through the standalone IFClite processing library.
//! Meshes are in metres, Z up. Preserve the model's world rotation while keeping
//! georeferenced positions near the origin; record the removed anchor in project data.

use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{Layer, ProjectData};
use ifc_lite_processing::{process, run_on_geometry_pool, ProcessingRequest};
use std::collections::BTreeMap;

pub struct IfcLoader;

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
        let mut layers = BTreeMap::<String, Layer>::new();
        for mesh in result.meshes {
            if mesh.indices.is_empty() || mesh.positions.is_empty() {
                continue;
            }
            let name = mesh.presentation_layer.as_ref().filter(|s| !s.is_empty()).unwrap_or(&mesh.ifc_type).clone();
            let layer = layers.entry(name.clone()).or_insert_with(|| Layer {
                name,
                color: [mesh.color[0], mesh.color[1], mesh.color[2]],
                colors: Some(Vec::new()),
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
        let project = ProjectData {
            info: vec![
                ("IFC schema".into(), result.metadata.schema_version),
                ("IFC world origin (metres)".into(), format!("{}, {}, {}", anchor[0], anchor[1], anchor[2])),
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
}
