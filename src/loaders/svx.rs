//! Adapter from the Voyager scene reader (crates svx-read and svx-model, repository svx-dll) to
//! the app's scene model. The glTF models are placed as the scene says; each annotation is a
//! pickable marker on a short stem out of the model, with its title and text as properties.
use super::placed::Gather;
use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{Layer, ProjectData};
use std::path::Path;

pub struct SvxLoader;

impl Loader for SvxLoader {
    fn name(&self) -> &'static str { "Voyager scene" }
    fn extensions(&self) -> &'static [&'static str] { &["svx"] }
    fn claims(&self, name: &str) -> bool { name.ends_with(".svx.json") }
    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }
    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let json = source.read()?;
        let path = source.path();
        let mut read = |uri: &str| -> Result<Vec<u8>, String> {
            let path = path.ok_or("the models beside a Voyager scene cannot be read here")?;
            let file = path.parent().unwrap_or(Path::new(".")).join(uri);
            super::check_reference(path, &file)?;
            std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))
        };
        let scene = svx_read::read(&json, &mut read)?;
        progress.set(Phase::Convert);
        let mut gather = Gather::default();
        for model in &scene.models {
            // on disk the glTF is read from its file, so a .gltf finds the files it refers to
            let file = match path {
                Some(p) => Source::File(p.parent().unwrap_or(Path::new(".")).join(&model.uri)),
                None => super::placed::glb(model.glb.clone()),
            };
            gather.gltf(&file, &[model.placement], &[("Model", model.name.clone()), ("File", model.uri.clone()), ("Quality", model.quality.clone())])?;
        }
        // marker size: 2 % of the scene's extent
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for layer in gather.layers.values() {
            for p in layer.positions.chunks_exact(3) {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
        let extent = (0..3).map(|k| (hi[k] - lo[k]).max(0.0)).fold(0.0f32, f32::max).max(0.01) as f64;
        if !scene.annotations.is_empty() {
            gather.add(markers(&scene.annotations, &scene.models, extent * 0.02), &[]);
        }
        let mut info = scene.info;
        info.push(("Annotations".into(), scene.annotations.len().to_string()));
        let project = ProjectData { info, ..ProjectData::default() };
        Ok(Loaded { structural: Vec::new(), origin: None, layers: gather.into_layers(), views: Vec::new(), project })
    }
}

/// The annotations as one layer: a small diamond at the end of a stem per annotation.
fn markers(annotations: &[svx_model::Annotation], models: &[svx_model::Model], size: f64) -> Layer {
    let mut layer = Layer { name: "Annotations".into(), color: [0.0, 0.61, 0.87], ..Layer::default() };
    for (i, a) in annotations.iter().enumerate() {
        let element = i as u64 + 1;
        let tip = [0, 1, 2].map(|k| a.position[k] + a.direction[k] * size * 3.0);
        let first = layer.indices.len() as u32;
        let base = (layer.positions.len() / 3) as u32;
        let axes = [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]];
        // eight faces of a diamond, each with its own three corners and flat normal
        for (x, y, z) in [(0, 2, 4), (2, 1, 4), (1, 3, 4), (3, 0, 4), (2, 0, 5), (1, 2, 5), (3, 1, 5), (0, 3, 5)] {
            let n = [0, 1, 2].map(|k| (axes[x][k] + axes[y][k] + axes[z][k]) / 3f64.sqrt());
            for c in [x, y, z] {
                layer.positions.extend([0, 1, 2].map(|k| (tip[k] + axes[c][k] * size) as f32));
                layer.normals.extend(n.map(|v| v as f32));
            }
        }
        layer.indices.extend(base..base + 24);
        layer.elements.push((element, first, 24));
        // the stem from the model to the diamond
        let stem = (layer.positions.len() / 3) as u32;
        layer.positions.extend(a.position.map(|v| v as f32));
        layer.positions.extend(tip.map(|v| v as f32));
        layer.normals.extend([0.0, 0.0, 1.0, 0.0, 0.0, 1.0]);
        layer.line_elements.push((element, layer.lines.len() as u32, 2));
        layer.lines.extend([stem, stem + 1]);
        let mut props = serde_json::Map::new();
        props.insert("Name".into(), a.title.clone().into());
        props.insert("Annotation".into(), a.lead.clone().into());
        if let Some(m) = models.get(a.model) {
            props.insert("Model".into(), m.name.clone().into());
        }
        if !a.id.is_empty() {
            props.insert("Id".into(), a.id.clone().into());
        }
        layer.props.push((element, serde_json::Value::Object(props).to_string()));
    }
    layer
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    /// A Voyager scene with a model and two annotations (made for this test).
    #[test]
    fn loads_a_voyager_scene_with_annotations() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test-models/voyager/box.svx.json");
        let model = crate::loaders::load(&path).expect("Voyager scene");
        let names: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
        assert!(names.contains(&"Annotations"), "{names:?}");
        // the Draco box, 12 triangles, plus two diamonds of 8
        assert_eq!(model.triangle_count(), 12 + 16);
        // the box is in centimetres in the scene: 0.01 m across
        let boxed = model.layers.iter().find(|l| l.name != "Annotations").unwrap();
        let xs: Vec<f32> = boxed.positions.chunks(3).map(|p| p[0]).collect();
        let width = xs.iter().cloned().fold(f32::MIN, f32::max) - xs.iter().cloned().fold(f32::MAX, f32::min);
        assert!((width - 0.01).abs() < 1e-4, "{width}");
    }
}
