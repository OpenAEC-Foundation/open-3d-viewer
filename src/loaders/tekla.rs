//! Adapter from the independent tekla-reader file reader to the app's scene model.
use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{Layer, ProjectData};

pub struct TeklaLoader;

impl Loader for TeklaLoader {
    fn name(&self) -> &'static str { "Tekla Structures" }
    fn extensions(&self) -> &'static [&'static str] { &["db1"] }
    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }
    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        // Native projects keep the profile and bolt catalogues beside their DB1.
        let document = if let Some(path) = source.path() {
            tekla_reader::read_path(path)?
        } else {
            tekla_reader::read(&source.read()?)?
        };
        progress.set(Phase::Convert);
        let mut project = ProjectData::default();
        project.info.push(("Tekla database".into(), document.database_version));
        if let Some(guid) = document.guid { project.info.push(("GUID".into(), guid)); }
        project.info.extend(document.info);
        let meshes = document.meshes;
        let layers = meshes.into_iter().map(|mesh| Layer {
            name: mesh.name, color: mesh.color, positions: mesh.positions, normals: mesh.normals, colors: mesh.colors, indices: mesh.indices, elements: mesh.elements, props: mesh.props, hidden: mesh.hidden
        }).collect();
        Ok(Loaded { layers, views: Vec::new(), project })
    }
}
