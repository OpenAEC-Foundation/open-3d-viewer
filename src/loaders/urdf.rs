//! Adapter from the URDF reader (crate urdf-read) to the app's scene model. The mesh files a
//! robot names are read by the app's reader of their format (STL, COLLADA, OBJ, ..) and placed
//! where the reader says.
use super::{Loaded, Loader, Phase, Progress, Source, DEFAULT_COLOR};
use crate::model::{Layer, ProjectData};

pub struct UrdfLoader;

impl Loader for UrdfLoader {
    fn name(&self) -> &'static str { "URDF" }
    fn extensions(&self) -> &'static [&'static str] { &["urdf"] }
    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }
    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        progress.set(Phase::Convert);
        let base = source.path().and_then(|p| p.parent());
        let robot = urdf_read::read(&String::from_utf8_lossy(&data), base)?;
        let mut layers = Vec::new();
        let mut missing = Vec::new();
        for (k, visual) in robot.visuals.into_iter().enumerate() {
            let id = k as u64 + 1;
            let color = visual.color.map(|c| [c[0], c[1], c[2]]);
            let mut props = serde_json::Map::new();
            props.insert("Link".into(), visual.link.clone().into());
            if let Some(name) = &visual.name {
                props.insert("Visual".into(), name.clone().into());
            }
            if !visual.joints.is_empty() {
                props.insert("Joints".into(), visual.joints.join(" › ").into());
            }
            if let Some(material) = &visual.material {
                props.insert("Material".into(), material.clone().into());
            }
            match visual.shape {
                urdf_read::Shape::Solid { kind, mesh } => {
                    props.insert("Geometry".into(), kind.into());
                    let count = mesh.indices.len() as u32;
                    layers.push(Layer { name: visual.link, color: color.unwrap_or(DEFAULT_COLOR), positions: mesh.positions, normals: mesh.normals, indices: mesh.indices, elements: vec![(id, 0, count)], props: vec![(id, serde_json::Value::Object(props).to_string())], ..Layer::default() });
                }
                urdf_read::Shape::File { filename, path, placement } => {
                    props.insert("Geometry".into(), "Mesh".into());
                    props.insert("File".into(), filename.clone().into());
                    match path.ok_or_else(|| "not found".to_string()).and_then(|path| mesh_file(source, path)) {
                        Ok((parts, y_up)) => {
                            let props = serde_json::Value::Object(props).to_string();
                            for mut part in parts {
                                place(&mut part, &placement, y_up);
                                if part.colors.is_none() && part.color == DEFAULT_COLOR {
                                    part.color = color.unwrap_or(DEFAULT_COLOR);
                                }
                                part.name = visual.link.clone();
                                part.elements = vec![(id, 0, part.indices.len() as u32)];
                                part.props = vec![(id, props.clone())];
                                part.line_elements.clear();
                                layers.push(part);
                            }
                        }
                        Err(e) => missing.push(format!("{filename} ({e})")),
                    }
                }
            }
        }
        let mut info = vec![("Robot".to_string(), robot.name)];
        if !missing.is_empty() {
            info.push(("Mesh files not read".into(), missing.join(", ")));
        }
        if !robot.unreached.is_empty() {
            info.push(("Links without a joint from the root".into(), robot.unreached.join(", ")));
        }
        Ok(Loaded { layers, views: Vec::new(), project: ProjectData { info, ..ProjectData::default() }, structural: Vec::new(), origin: None })
    }
}

/// The layers of a mesh file a robot names, and whether its reader turned Y up to Z up.
fn mesh_file(robot: &Source, path: std::path::PathBuf) -> Result<(Vec<Layer>, bool), String> {
    if let Some(model) = robot.path() {
        super::check_reference(model, &path)?;
    }
    let file = Source::File(path);
    let loader = super::loader_for(&file)?;
    Ok((loader.load(&file)?, loader.source_y_up()))
}

/// Moves a mesh file's layer into the robot: a mesh is in its link's frame as written, so a
/// reader's turn from Y up to Z up is undone first.
fn place(layer: &mut Layer, m: &urdf_read::Matrix, y_up: bool) {
    let unturn = |v: &mut [f32]| {
        if y_up {
            let (y, z) = (v[1], v[2]);
            v[1] = z;
            v[2] = -y;
        }
    };
    for p in layer.positions.chunks_exact_mut(3) {
        unturn(p);
        let q = urdf_read::point(m, [p[0] as f64, p[1] as f64, p[2] as f64]);
        p.copy_from_slice(&q.map(|v| v as f32));
    }
    for n in layer.normals.chunks_exact_mut(3) {
        unturn(n);
        let q = urdf_read::normal(m, [n[0] as f64, n[1] as f64, n[2] as f64]);
        n.copy_from_slice(&q.map(|v| v as f32));
    }
    if urdf_read::mirrors(m) {
        for tri in layer.indices.chunks_exact_mut(3) {
            tri.swap(1, 2);
        }
    }
}
