//! Autodesk Inventor parts (.ipt) and assemblies (.iam) via the OpenAEC inventor-dll library:
//! a part's B-rep tessellated (one layer per body), an assembly from the display cache in the
//! file (one layer, one pickable element per cached mesh). The iProperties go to the element
//! properties and the project information, skipped faces to the information rows.

use super::{Loaded, Loader, Phase, Progress, Source, DEFAULT_COLOR};
use crate::model::{Layer, ProjectData};
use inventor_read::{Document, DocumentKind, Error, MeshSource};
use serde_json::{Map, Value};

pub struct InventorLoader;

impl Loader for InventorLoader {
    fn name(&self) -> &'static str {
        "Inventor"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["ipt", "iam"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let doc = Document::from_bytes(source.read()?.into_owned()).map_err(|e| format!("{}: {e}", source.name()))?;
        progress.set(Phase::Convert);
        convert(&doc, &source.stem())
    }
}

/// The iProperties of the document as a flat JSON object, for the element properties.
fn iproperties(doc: &Document) -> Map<String, Value> {
    let mut props = Map::new();
    props.insert("Inventor document".into(), kind(doc).into());
    for (label, value) in doc.iproperties.rows() {
        props.insert(label.into(), value.into());
    }
    props
}

fn kind(doc: &Document) -> &'static str {
    match doc.kind {
        DocumentKind::Part if doc.sheet_metal => "Sheet metal part",
        k => k.name(),
    }
}

fn convert(doc: &Document, stem: &str) -> Result<Loaded, String> {
    let scene = doc.scene().map_err(|e| match e {
        Error::NoDisplayCache { .. } => format!("{stem}: {e}"),
        e => format!("Inventor: {e}"),
    })?;
    let mut project = ProjectData::default();
    project.info.push(("Inventor document".into(), kind(doc).into()));
    let version = doc.version();
    if let Some(v) = version.display.clone().or_else(|| version.release.map(|r| r.to_string())) {
        let build = version.build.map(|b| format!(" (build {b})")).unwrap_or_default();
        project.info.push(("Inventor version".into(), format!("{v}{build}")));
    }
    project.info.extend(doc.iproperties.rows().into_iter().map(|(k, v)| (k.to_string(), v)));
    project.info.push((
        "Inventor geometry".into(),
        match scene.source {
            MeshSource::Brep => "B-rep, tessellated (planes, cylinders, cones)".into(),
            MeshSource::DisplayCache => "Display cache of the assembly".into(),
        },
    ));
    if let Some(s) = &scene.brep {
        project.info.push(("Inventor faces".into(), format!("{} of {} shown", s.tessellated, s.faces)));
    }
    if !doc.references.is_empty() {
        let names: Vec<&str> = doc.references.iter().map(|r| inventor_read::refs::file_name(r)).collect();
        project.info.push(("Inventor referenced files".into(), names.join(", ")));
    }
    if !scene.warnings.is_empty() {
        project.info.push(("Inventor warnings".into(), scene.warnings.join("; ")));
    }

    let props = iproperties(doc);
    let mut layers = Vec::new();
    match scene.source {
        // a part: one layer and one element per body
        MeshSource::Brep => {
            for (k, mesh) in scene.meshes.into_iter().enumerate() {
                let id = k as u64 + 1;
                let mut p = props.clone();
                p.insert("Name".into(), mesh.name.clone().into());
                let count = mesh.indices.len() as u32;
                layers.push(Layer {
                    name: mesh.name,
                    color: DEFAULT_COLOR,
                    positions: millimetres_to_metres(mesh.positions),
                    normals: mesh.normals,
                    indices: mesh.indices,
                    elements: vec![(id, 0, count)],
                    props: vec![(id, Value::Object(p).to_string())],
                    ..Layer::default()
                });
            }
        }
        // an assembly: the cached meshes in one layer, each mesh an element
        MeshSource::DisplayCache => {
            let name = doc.iproperties.part_number.clone().unwrap_or_else(|| stem.to_string());
            let mut layer = Layer { name, color: DEFAULT_COLOR, ..Layer::default() };
            for (k, mesh) in scene.meshes.into_iter().enumerate() {
                let id = k as u64 + 1;
                let base = layer.positions.len() as u32 / 3;
                let first = layer.indices.len() as u32;
                layer.positions.extend(millimetres_to_metres(mesh.positions));
                layer.normals.extend(mesh.normals);
                layer.indices.extend(mesh.indices.iter().map(|i| i + base));
                layer.elements.push((id, first, layer.indices.len() as u32 - first));
                let mut p = Map::new();
                p.insert("Name".into(), mesh.name.into());
                p.insert("Assembly".into(), layer.name.clone().into());
                p.insert("Source".into(), "Inventor display cache".into());
                layer.props.push((id, Value::Object(p).to_string()));
            }
            if !layer.indices.is_empty() {
                layers.push(layer);
            }
        }
    }
    if layers.is_empty() {
        let why = if scene.warnings.is_empty() { String::new() } else { format!(" ({})", scene.warnings.join("; ")) };
        return Err(format!("{stem}: no Inventor geometry that can be shown yet{why}"));
    }
    Ok(Loaded { layers, views: Vec::new(), project })
}

fn millimetres_to_metres(mut positions: Vec<f32>) -> Vec<f32> {
    for v in &mut positions {
        *v /= 1000.0;
    }
    positions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loaders::load_with;
    use inventor_read::synth::{self, Cache, Ending, Solid};
    use std::sync::Arc;

    fn bytes(name: &str, data: Vec<u8>) -> Source {
        Source::Bytes { name: name.into(), data: Arc::new(data) }
    }

    #[test]
    fn opens_a_part_with_its_iproperties() {
        let source = bytes("Bracket.IPT", synth::part(&Solid::plate_with_hole(4.0, 0.5, 1.0), 2, 230, Ending::EndRecord));
        let model = load_with(&source, &Progress::default()).expect("Inventor part");
        assert_eq!(model.format, "Inventor");
        assert_eq!(model.layers.len(), 1);
        // 4 × 4 × 0.5 cm → 0.04 × 0.04 × 0.005 m
        let size = model.bounds.size();
        assert!((size[0] - 0.04).abs() < 1e-6 && (size[2] - 0.005).abs() < 1e-6, "{size:?}");
        let layer = &model.layers[0];
        assert_eq!(layer.name, "Bracket-01");
        assert_eq!(layer.normals.len(), layer.positions.len());
        let props = model.props.values().next().expect("element properties");
        assert!(props.contains("Aluminium 6061") && props.contains("Bracket-01"), "{props}");
        let info = &model.project.info;
        assert!(info.iter().any(|(k, v)| k == "Inventor document" && v == "Part"));
        assert!(info.iter().any(|(k, v)| k == "Inventor version" && v.starts_with("2026")));
        assert!(info.iter().any(|(k, v)| k == "Material" && v == "Aluminium 6061"));
        assert!(info.iter().any(|(k, v)| k == "Inventor faces" && v == "7 of 7 shown"));
        assert!(!info.iter().any(|(k, _)| k == "Inventor warnings"));
    }

    #[test]
    fn reports_skipped_faces() {
        let mut solid = Solid::cube(1.0);
        solid.set_surface(0, synth::SurfaceSpec::Other("spline-surface"));
        let source = bytes("cube.ipt", synth::part(&solid, 1, 228, Ending::EndRecord));
        let model = load_with(&source, &Progress::default()).unwrap();
        assert_eq!(model.triangle_count(), 10);
        assert!(model.project.info.iter().any(|(k, v)| k == "Inventor warnings" && v.contains("1 faces skipped (unsupported surface: spline-surface 1)")));
    }

    #[test]
    fn opens_an_assembly_from_its_display_cache() {
        let mut cache = Cache::default();
        cache.boxed([0.0; 3], [1.0, 1.0, 1.0]).boxed([2.0, 0.0, 0.0], [3.0, 1.0, 1.0]);
        let source = bytes("frame.iam", synth::assembly(Some(&cache), &["Pin.ipt"]));
        let model = load_with(&source, &Progress::default()).expect("Inventor assembly");
        assert_eq!(model.layers.len(), 1);
        assert_eq!(model.layers[0].elements.len(), 2);
        assert_eq!(model.triangle_count(), 24);
        assert!((model.bounds.size()[0] - 0.03).abs() < 1e-6);
        assert!(model.project.info.iter().any(|(k, v)| k == "Inventor referenced files" && v == "Pin.ipt"));
    }

    #[test]
    fn refuses_an_assembly_without_display_cache() {
        let source = bytes("old.iam", synth::assembly(None, &["Pin.ipt", "Plate.ipt"]));
        let error = load_with(&source, &Progress::default()).unwrap_err();
        assert!(error.contains("no display cache") && error.contains("2 referenced files"), "{error}");
    }
}
