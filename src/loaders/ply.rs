//! Adapter from the independent ply-reader file reader to the app's scene model.
use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{Layer, ProjectData};

pub struct PlyLoader;

impl Loader for PlyLoader {
    fn name(&self) -> &'static str { "PLY" }
    fn extensions(&self) -> &'static [&'static str] { &["ply"] }
    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }
    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        progress.set(Phase::Convert);
        let mesh = ply_reader::read(&data)?;
        let layers = vec![Layer { name: source.stem(), color: mesh.color, positions: mesh.positions,
            normals: mesh.normals, colors: mesh.colors, indices: mesh.indices, ..Layer::default() }];
        let project = ProjectData::default();
        Ok(Loaded { layers, views: Vec::new(), project })
    }
}
