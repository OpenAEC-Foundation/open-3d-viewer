//! Vectorworks (.vwx) via the OpenAEC vwx reader: one layer per Vectorworks layer and class
//! batch, the drawn objects pickable, with their properties.
use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{Layer, ProjectData};

pub struct VwxLoader;

impl Loader for VwxLoader {
    fn name(&self) -> &'static str {
        "Vectorworks"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["vwx"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        progress.set(Phase::Convert);
        let scene = vwx_read::read(&data).map_err(|e| format!("Cannot read Vectorworks file: {e}"))?;
        let layers = scene.meshes.into_iter().map(layer).collect();
        Ok(Loaded { structural: Vec::new(), origin: None, layers, views: Vec::new(), project: ProjectData::default() })
    }
}

/// The viewer layer of a reader mesh (both metres, Z up).
fn layer(mesh: vwx_read::model::Mesh) -> Layer {
    Layer {
        name: mesh.name,
        color: mesh.color,
        positions: mesh.positions,
        normals: mesh.normals,
        colors: mesh.colors,
        indices: mesh.indices,
        elements: mesh.elements,
        props: mesh.props,
        hidden: mesh.hidden,
        points: false,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{load_with, Progress, Source};
    use std::sync::Arc;

    #[test]
    fn vectorworks_files_reach_the_vwx_reader() {
        let source = Source::Bytes { name: "house.VWX".into(), data: Arc::new(Vec::new()) };
        let error = load_with(&source, &Progress::default()).unwrap_err();
        assert!(error.starts_with("Cannot read Vectorworks file:"), "{error}");
    }

    #[test]
    fn meshes_map_onto_layers() {
        let mut mesh = vwx_read::model::Mesh { name: "Ontwerplaag-1 / Muren".into(), color: [1.0, 0.0, 0.0], ..Default::default() };
        vwx_read::mesh::add_prism(&mut mesh, 7, &[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], 0.0, 2.7);
        let layer = super::layer(mesh);
        assert_eq!(layer.name, "Ontwerplaag-1 / Muren");
        assert_eq!(layer.triangle_count(), 12);
        assert_eq!(layer.elements, vec![(7, 0, 36)]);
        assert_eq!(layer.normals.len(), layer.positions.len());
    }
}
