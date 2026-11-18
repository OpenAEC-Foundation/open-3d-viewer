//! SolidWorks parts and assemblies (`.sldprt`, `.sldasm`) via the OpenAEC SolidWorks reader
//! ([solidworks-dll](https://github.com/OpenAEC-Foundation/solidworks-dll)): the triangles
//! SolidWorks saved with the model for showing it (its display list), in the colours of its
//! faces and bodies. One layer per part of an assembly, each placed part a pickable element
//! with its component path, part, file and configuration; one layer per body of a part. An
//! assembly saved without the tessellation of a part takes it from the part file next to it.
//! The exact geometry, other configurations than the one saved, textures and the feature tree
//! are not read; neither are files in the newer SolidWorks container.
use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{Layer, ProjectData};

pub struct SldprtLoader;

impl Loader for SldprtLoader {
    fn name(&self) -> &'static str {
        "SolidWorks"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["sldprt", "sldasm"]
    }

    fn source_y_up(&self) -> bool {
        true
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        progress.set(Phase::Convert);
        // a part the assembly was saved without: the file of that name next to the assembly
        let folder = source.path().and_then(|path| path.parent());
        let mut sibling = |name: &str| {
            let wanted = name.to_lowercase();
            let found = std::fs::read_dir(folder?).ok()?.flatten().find(|entry| entry.file_name().to_string_lossy().to_lowercase() == wanted)?;
            std::fs::read(found.path()).ok()
        };
        let scene = sldw_read::read_assembly(&data, &mut sibling).map_err(|e| format!("Cannot read SolidWorks file: {e}"))?;
        let mut project = ProjectData::default();
        if !scene.info.missing.is_empty() {
            project.info.push(("SolidWorks parts without tessellation".to_string(), scene.info.missing.len().to_string()));
        }
        Ok(Loaded { structural: Vec::new(), origin: None, layers: layers(scene.meshes), views: Vec::new(), project })
    }
}

/// The meshes of the reader as layers: one per part (the body, in a part file), the meshes of
/// one placed part (one per colour) together one element. Metres; Y up becomes Z up.
fn layers(meshes: Vec<sldw_read::model::Mesh>) -> Vec<Layer> {
    let mut layers: Vec<Layer> = Vec::new();
    for mesh in meshes {
        let prop = |name: &str| mesh.props.iter().find(|p| p.0 == name).map(|p| p.1.as_str());
        let name = prop("Part").unwrap_or(&mesh.name);
        let layer = match layers.iter().position(|l| l.name == name) {
            Some(k) => &mut layers[k],
            None => {
                layers.push(Layer { name: name.to_string(), color: mesh.color, colors: Some(Vec::new()), ..Layer::default() });
                layers.last_mut().expect("just added")
            }
        };
        let (base, first) = (layer.vertex_count() as u32, layer.indices.len() as u32);
        let z_up = |v: &[f32]| [v[0], -v[2], v[1]];
        layer.positions.extend(mesh.world_positions().chunks_exact(3).flat_map(z_up));
        layer.normals.extend(mesh.world_normals().chunks_exact(3).flat_map(z_up));
        let [r, g, b] = mesh.color.map(|v| (v * 255.0).round() as u8);
        layer.colors.get_or_insert_default().resize(layer.positions.len() / 3, [r, g, b, 255]);
        layer.indices.extend(mesh.indices.iter().map(|i| base + i));
        // the next colour of the same placed part goes on in the same element
        let id = u64::from(mesh.instance) + 1;
        match layer.elements.last_mut() {
            Some(last) if last.0 == id => last.2 += mesh.indices.len() as u32,
            _ => {
                layer.elements.push((id, first, mesh.indices.len() as u32));
                let props: serde_json::Map<String, serde_json::Value> = std::iter::once(("Name".to_string(), mesh.name.clone())).chain(mesh.props.iter().cloned()).map(|(k, v)| (k, v.into())).collect();
                layer.props.push((id, serde_json::Value::Object(props).to_string()));
            }
        }
    }
    layers
}

#[cfg(test)]
mod tests {
    use super::super::{load_with, Progress, Source};
    use sldw_read::model::Mesh;
    use std::sync::Arc;

    #[test]
    fn solidworks_files_reach_the_reader() {
        for name in ["bracket.SLDPRT", "robot.sldasm"] {
            let source = Source::Bytes { name: name.into(), data: Arc::new(Vec::new()) };
            let error = load_with(&source, &Progress::default()).unwrap_err();
            assert!(error.starts_with("Cannot read SolidWorks file:"), "{error}");
        }
    }

    #[test]
    fn placed_parts_become_elements_of_the_layer_of_their_part() {
        let triangle = |name: &str, instance: u32, color: [f32; 3], x: f64| Mesh {
            name: name.into(),
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            color,
            transform: [1.0, 0.0, 0.0, x, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            props: vec![("Component".into(), format!("{name}@Robot")), ("Part".into(), "Plate".into())],
            instance,
        };
        let (blue, black) = ([0.0, 0.0, 1.0], [0.0; 3]);
        let layers = super::layers(vec![triangle("Plate-1", 0, blue, 0.0), triangle("Plate-1", 0, black, 0.0), triangle("Plate-2", 1, blue, 10.0)]);
        assert_eq!(layers.len(), 1);
        let layer = &layers[0];
        assert_eq!((layer.name.as_str(), layer.triangle_count()), ("Plate", 3));
        assert_eq!(layer.elements, vec![(1, 0, 6), (2, 6, 3)]);
        assert_eq!(layer.props.len(), 2);
        assert!(layer.props[1].1.contains(r#""Name":"Plate-2""#) && layer.props[1].1.contains(r#""Component":"Plate-2@Robot""#), "{}", layer.props[1].1);
        let colors = layer.colors.as_ref().unwrap();
        assert_eq!((colors[0], colors[3], colors.len()), ([0, 0, 255, 255], [0, 0, 0, 255], 9));
        // Y up in the file is Z up here: the point (0, 1, 0) of the second plate, moved 10 in x
        assert_eq!(&layer.positions[24..27], [10.0, 0.0, 1.0]);
        assert_eq!(&layer.normals[..3], [0.0, -1.0, 0.0]);
    }
}
