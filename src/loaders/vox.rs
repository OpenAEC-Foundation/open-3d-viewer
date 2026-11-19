//! Adapter from the MagicaVoxel reader (crate vox-read) to the app's scene model.
use super::{Loader, Source};
use crate::model::Layer;

pub struct VoxLoader;

impl Loader for VoxLoader {
    fn name(&self) -> &'static str { "MagicaVoxel" }
    fn extensions(&self) -> &'static [&'static str] { &["vox"] }
    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        let meshes = vox_read::read(&source.read()?)?;
        Ok(meshes.into_iter().enumerate().map(|(k, mesh)| {
            let id = k as u64 + 1;
            let props = serde_json::Value::Object(mesh.props.into_iter().map(|(key, value)| (key, value.into())).collect()).to_string();
            Layer { name: mesh.name, color: super::DEFAULT_COLOR, elements: vec![(id, 0, mesh.indices.len() as u32)], props: vec![(id, props)], positions: mesh.positions, normals: mesh.normals, colors: Some(mesh.colors), indices: mesh.indices, hidden: mesh.hidden, ..Layer::default() }
        }).collect())
    }
}
