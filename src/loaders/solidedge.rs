//! Solid Edge parts, sheet metal parts and assemblies (`.par`, `.psm`, `.asm`) via the OpenAEC
//! Solid Edge reader ([solidedge-dll](https://github.com/OpenAEC-Foundation/solidedge-dll)): the
//! triangles Solid Edge saved with the model for showing it (its display cache), in the colours
//! of its face styles. One layer per part, each placed part of an assembly a pickable element
//! with its component path, part, file and style. An assembly takes its parts from their files
//! next to it; when files are missing and the assembly holds a preview, it is drawn from that,
//! more coarsely. The exact geometry, construction bodies, flat patterns and the feature tree
//! are not read.
//!
//! Other programs use `.par` and `.asm` too: a file that is no Solid Edge document is refused
//! with a message that says so.
use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{Layer, ProjectData};
use std::path::{Path, PathBuf};

pub struct SolidEdgeLoader;

impl Loader for SolidEdgeLoader {
    fn name(&self) -> &'static str {
        "Solid Edge"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["par", "psm", "asm"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        progress.set(Phase::Convert);
        // the files an assembly places: looked for around the assembly (not in the browser,
        // which hands over one file)
        let folder = source.path().and_then(Path::parent).map(Path::to_path_buf);
        let mut nearby: Option<Vec<PathBuf>> = None;
        let mut fetch = |name: &str| {
            let files = nearby.get_or_insert_with(|| folder.as_deref().map(around).unwrap_or_default());
            std::fs::read(find(files, name)?).ok()
        };
        let scene = sedge_read::read_assembly(&data, &mut fetch).map_err(|e| match e {
            sedge_read::Error::NoGeometry(what) if what.contains("none of the parts") => {
                "Cannot read Solid Edge file: an assembly of this version holds no geometry of its own, and none of the files of its parts was found next to it".to_string()
            }
            e => format!("Cannot read Solid Edge file: {e}"),
        })?;
        let mut project = ProjectData::default();
        if scene.info.preview {
            project.info.push(("Solid Edge tessellation".to_string(), "the preview of the assembly (files of its parts were not found)".to_string()));
        }
        if !scene.info.missing.is_empty() {
            let mut names = scene.info.missing.clone();
            names.sort();
            names.dedup();
            project.info.push(("Solid Edge parts not found".to_string(), format!("{} ({})", scene.info.missing.len(), names.join(", "))));
        }
        Ok(Loaded { structural: Vec::new(), origin: None, layers: layers(scene.meshes, &source.stem()), views: Vec::new(), project })
    }
}

/// The files in the folder of an assembly, in the folders in it, and in the folder above and
/// the folders in that: the places the parts of an assembly are kept. The nearest come first.
fn around(folder: &Path) -> Vec<PathBuf> {
    let entries = |folder: &Path| std::fs::read_dir(folder).into_iter().flatten().flatten().map(|e| e.path()).collect::<Vec<_>>();
    let (mut files, mut folders) = (Vec::new(), Vec::new());
    let mut take = |paths: Vec<PathBuf>, folders: Option<&mut Vec<PathBuf>>| {
        let (inner, plain): (Vec<_>, Vec<_>) = paths.into_iter().partition(|p| p.is_dir());
        files.extend(plain);
        if let Some(folders) = folders {
            folders.extend(inner);
        }
    };
    take(entries(folder), Some(&mut folders));
    let mut beside = Vec::new();
    if let Some(parent) = folder.parent() {
        take(entries(parent), Some(&mut beside));
    }
    for inner in folders.iter().chain(beside.iter().filter(|p| p.as_path() != folder)) {
        take(entries(inner), None);
    }
    files
}

/// A file name without what people change when they rename files: case, spaces, punctuation.
fn loose(name: &str) -> String {
    name.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// The file an assembly names, among `files`: by its name whatever the case, else by a name
/// that differs in spaces and punctuation only (`double motor.par` for `double-motor.par`).
fn find<'a>(files: &'a [PathBuf], name: &str) -> Option<&'a PathBuf> {
    let named = |file: &&PathBuf, same: &dyn Fn(&str) -> bool| file.file_name().is_some_and(|n| same(&n.to_string_lossy()));
    let (exact, nearly) = (name.to_lowercase(), loose(name));
    files.iter().find(|f| named(f, &|n| n.to_lowercase() == exact)).or_else(|| files.iter().find(|f| named(f, &|n| loose(n) == nearly)))
}

/// The meshes of the reader as layers: one per part (the file itself, for a part), the meshes
/// of one placed part (one per style) together one element. Metres, Z up, as in the file.
fn layers(meshes: Vec<sedge_read::model::Mesh>, stem: &str) -> Vec<Layer> {
    let mut layers: Vec<Layer> = Vec::new();
    for mesh in meshes {
        let prop = |name: &str| mesh.props.iter().find(|p| p.0 == name).map(|p| p.1.as_str());
        let name = prop("Part").unwrap_or(stem);
        let layer = match layers.iter().position(|l| l.name == name) {
            Some(k) => &mut layers[k],
            None => {
                layers.push(Layer { name: name.to_string(), color: mesh.color, colors: Some(Vec::new()), ..Layer::default() });
                layers.last_mut().expect("just added")
            }
        };
        let (base, first) = (layer.vertex_count() as u32, layer.indices.len() as u32);
        layer.positions.extend(mesh.world_positions());
        layer.normals.extend(mesh.world_normals());
        let [r, g, b] = mesh.color.map(|v| (v * 255.0).round() as u8);
        let alpha = (mesh.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        layer.colors.get_or_insert_default().resize(layer.positions.len() / 3, [r, g, b, alpha]);
        layer.indices.extend(mesh.indices.iter().map(|i| base + i));
        // the next style of the same placed part goes on in the same element
        let id = u64::from(mesh.instance) + 1;
        match layer.elements.last_mut() {
            Some(last) if last.0 == id => last.2 += mesh.indices.len() as u32,
            _ => {
                layer.elements.push((id, first, mesh.indices.len() as u32));
                let style = (!mesh.style.is_empty()).then(|| ("Style".to_string(), mesh.style.clone()));
                let props: serde_json::Map<String, serde_json::Value> = std::iter::once(("Name".to_string(), mesh.name.clone())).chain(mesh.props.iter().cloned()).chain(style).map(|(k, v)| (k, v.into())).collect();
                layer.props.push((id, serde_json::Value::Object(props).to_string()));
            }
        }
    }
    layers
}

#[cfg(test)]
mod tests {
    use super::super::{load_with, Progress, Source};
    use sedge_read::model::{Mesh, IDENTITY};
    use sedge_read::testing::{assembly, moved, part, quad};
    use std::sync::Arc;

    const STYLES: [(u32, &str, [f32; 3], f32); 2] = [(0x8000_003E, "Steel", [0.2, 0.4, 0.6], 1.0), (0x8000_0078, "Glass", [0.4, 0.6, 0.8], 0.5)];

    fn plate() -> Vec<u8> {
        part(&[quad(1, [0.0; 3], None), quad(2, [0.0, 0.0, 1.0], Some(0x8000_0078))], &STYLES, 0x8000_003E, &[])
    }

    #[test]
    fn files_of_other_programs_with_these_extensions_are_refused() {
        // assembler source, a parameter file, an empty file
        for (name, data) in [("boot.asm", b"; boot sector\n        mov ax, 0x07C0\n".to_vec()), ("run.par", b"TOLERANCE = 0.001\n".to_vec()), ("plate.PSM", Vec::new())] {
            let source = Source::Bytes { name: name.into(), data: Arc::new(data) };
            let error = load_with(&source, &Progress::default()).unwrap_err();
            assert!(error.starts_with("Cannot read Solid Edge file: not a Solid Edge file"), "{name}: {error}");
        }
    }

    #[test]
    fn a_part_opens_in_the_colours_of_its_styles() {
        let source = Source::Bytes { name: "Plate.par".into(), data: Arc::new(plate()) };
        let model = load_with(&source, &Progress::default()).expect("a Solid Edge part");
        assert_eq!((model.format.as_str(), model.triangle_count(), model.layers.len()), ("Solid Edge", 4, 1));
        let layer = &model.layers[0];
        assert_eq!((layer.name.as_str(), layer.elements.as_slice()), ("Plate", [(1, 0, 12)].as_slice()));
        let colors = layer.colors.as_ref().unwrap();
        assert_eq!((colors[0], colors[4]), ([51, 102, 153, 255], [102, 153, 204, 128]));
        // Z up, as in the file: the second face lies a metre above the first
        assert_eq!((model.bounds.min, model.bounds.max), ([0.0; 3], [1.0, 1.0, 1.0]));
    }

    #[test]
    fn an_assembly_takes_its_parts_from_the_folders_around_it() {
        let root = std::env::temp_dir().join(format!("solid-edge-adapter-{}", std::process::id()));
        let (assemblies, parts) = (root.join("assemblies"), root.join("parts"));
        std::fs::create_dir_all(&assemblies).unwrap();
        std::fs::create_dir_all(&parts).unwrap();
        // one part next to the assembly, one renamed in a folder beside it, one nowhere
        std::fs::write(assemblies.join("PLATE.PAR"), plate()).unwrap();
        std::fs::write(parts.join("side-plate.par"), plate()).unwrap();
        let car = assembly(&[
            ("C:\\Work\\Plate.par", "Plate.par", &[("Plate.par:1", IDENTITY), ("Plate.par:2", moved(5.0, 0.0, 0.0))]),
            ("C:\\Work\\side plate.par", "side plate.par", &[("side plate.par:1", moved(0.0, 9.0, 0.0))]),
            ("C:\\Work\\Lost.par", "Lost.par", &[("Lost.par:1", IDENTITY)]),
        ]);
        std::fs::write(assemblies.join("Car.asm"), car).unwrap();
        let model = load_with(&Source::File(assemblies.join("Car.asm")), &Progress::default());
        std::fs::remove_dir_all(&root).ok();
        let model = model.expect("a Solid Edge assembly");
        let names: Vec<(&str, usize)> = model.layers.iter().map(|l| (l.name.as_str(), l.elements.len())).collect();
        assert_eq!(names, [("Plate", 2), ("side plate", 1)]);
        assert_eq!(model.bounds.max, [6.0, 10.0, 1.0]);
        assert!(model.project.info.contains(&("Solid Edge parts not found".to_string(), "1 (Lost.par)".to_string())), "{:?}", model.project.info);
    }

    #[test]
    fn an_assembly_without_its_parts_says_what_is_missing() {
        let car = assembly(&[("C:\\Work\\Plate.par", "Plate.par", &[("Plate.par:1", IDENTITY)])]);
        let error = load_with(&Source::Bytes { name: "Car.asm".into(), data: Arc::new(car) }, &Progress::default()).unwrap_err();
        assert!(error.contains("none of the files of its parts was found"), "{error}");
    }

    #[test]
    fn placed_parts_become_elements_of_the_layer_of_their_part() {
        let triangle = |name: &str, instance: u32, color: [f32; 3], style: &str, x: f64| Mesh {
            name: name.into(),
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            color,
            opacity: 1.0,
            style: style.into(),
            transform: moved(x, 0.0, 0.0),
            props: vec![("Component".into(), format!("Axle.asm:1/{name}")), ("Part".into(), "Wheel".into())],
            instance,
        };
        let (blue, black) = ([0.0, 0.0, 1.0], [0.0; 3]);
        let layers = super::layers(vec![triangle("Wheel.par:1", 0, blue, "Blue", 0.0), triangle("Wheel.par:1", 0, black, "Black", 0.0), triangle("Wheel.par:2", 1, blue, "Blue", 10.0)], "Car");
        assert_eq!(layers.len(), 1);
        let layer = &layers[0];
        assert_eq!((layer.name.as_str(), layer.triangle_count()), ("Wheel", 3));
        assert_eq!(layer.elements, vec![(1, 0, 6), (2, 6, 3)]);
        assert_eq!(layer.props.len(), 2);
        assert!(layer.props[1].1.contains(r#""Name":"Wheel.par:2""#) && layer.props[1].1.contains(r#""Component":"Axle.asm:1/Wheel.par:2""#) && layer.props[1].1.contains(r#""Style":"Blue""#), "{}", layer.props[1].1);
        let colors = layer.colors.as_ref().unwrap();
        assert_eq!((colors[0], colors[3], colors.len()), ([0, 0, 255, 255], [0, 0, 0, 255], 9));
        // the second wheel, moved 10 in x; Z stays up
        assert_eq!(&layer.positions[18..21], [10.0, 0.0, 0.0]);
        assert_eq!(&layer.normals[..3], [0.0, 0.0, 1.0]);
    }
}
