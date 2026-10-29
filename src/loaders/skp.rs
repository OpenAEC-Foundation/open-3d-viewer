//! Adapter from the independent skp-reader file reader to the app's scene model.
use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{Layer, ProjectData};

pub struct SkpLoader;

impl Loader for SkpLoader {
    fn name(&self) -> &'static str { "SketchUp" }
    fn extensions(&self) -> &'static [&'static str] { &["skp"] }
    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }
    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        progress.set(Phase::Convert);
        let meshes = skp_reader::read(&data)?;
        let layers = meshes.into_iter().map(|mesh| Layer {
            name: mesh.name, color: mesh.color, positions: mesh.positions, normals: mesh.normals, colors: mesh.colors, indices: mesh.indices, elements: mesh.elements, props: mesh.props, hidden: mesh.hidden
        }).collect();
        let project = ProjectData::default();
        Ok(Loaded { layers, views: Vec::new(), project })
    }
}
