//! Adapter from the independent readers of finite element meshes and simulation results (crate
//! fem-read, repository fem-dll) to the app's model: the outer faces of each group of cells a
//! layer, the fields with where the layers' vertices come from.

use super::{Loaded, Loader, Phase, Progress, Source};
use crate::fields::{At, Fields, LayerSource, Step, Values};
use crate::model::{Layer, ProjectData};
use fem_read::fem_model::{Document, FieldAt};
use glam::DVec3;
use std::path::Path;

/// VTK files (legacy, XML, parallel, multiblock, time series).
pub struct VtkLoader;

impl Loader for VtkLoader {
    fn name(&self) -> &'static str {
        "VTK"
    }

    fn extensions(&self) -> &'static [&'static str] {
        fem_read::VTK
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        read(source, progress)
    }
}

/// The colours the groups of a mesh are drawn in while no field colours them.
const GROUP_COLOURS: [[f32; 3]; 6] = [[0.72, 0.72, 0.74], [0.55, 0.67, 0.80], [0.80, 0.68, 0.52], [0.60, 0.76, 0.62], [0.78, 0.60, 0.66], [0.70, 0.66, 0.82]];

fn read(source: &Source, progress: &Progress) -> Result<Loaded, String> {
    progress.set(Phase::Read);
    let data = source.read()?;
    // the files a file names (pieces, blocks, time steps) lie beside it on disk
    let file = source.path().map(Path::to_path_buf);
    let side = |rel: &str| -> Result<Vec<u8>, String> {
        let Some(file) = &file else { return Err(format!("{rel}: this file needs the files beside it; open it from disk in the desktop app")) };
        let beside = file.parent().unwrap_or(Path::new("")).join(rel);
        super::check_reference(file, &beside)?;
        std::fs::read(&beside).map_err(|e| format!("{rel}: {e}"))
    };
    progress.set(Phase::Convert);
    let doc = fem_read::read(&source.name(), &data, &side).map_err(|e| format!("{}: {e}", source.name()))?;
    Ok(loaded(doc))
}

/// The layers, fields and project information of a document.
fn loaded(doc: Document) -> Loaded {
    let mut info = vec![("Format".to_string(), doc.format.clone())];
    let mut layers = Vec::new();
    let mut origin = None;
    let mut fields = Fields::default();
    if let Some(mesh) = doc.mesh {
        // far from the origin: shown around the middle of the mesh
        let anchor = mesh.bounds().map(|(lo, hi)| (DVec3::from(lo), DVec3::from(hi))).filter(|(lo, hi)| lo.abs().max(hi.abs()).max_element() > 1000.0).map(|(lo, hi)| ((lo + hi) * 0.5).round());
        origin = anchor;
        let shift = anchor.unwrap_or(DVec3::ZERO);
        let at = |p: [f64; 3]| [(p[0] - shift.x) as f32, (p[1] - shift.y) as f32, (p[2] - shift.z) as f32];
        for (k, s) in mesh.surfaces().into_iter().enumerate() {
            let name = mesh.groups.get(s.group as usize).filter(|g| !g.is_empty()).cloned().unwrap_or_else(|| mesh.name.clone());
            let color = GROUP_COLOURS[k % GROUP_COLOURS.len()];
            let cells: std::collections::BTreeSet<u32> = s.cell.iter().copied().collect();
            let id = layers.len() as u64 + 1;
            let props = serde_json::json!({ "Name": name, "Cells": cells.len() }).to_string();
            if !s.triangles.is_empty() || !s.lines.is_empty() {
                let (count, lines) = (s.triangles.len() as u32, s.lines.len() as u32);
                fields.layers.push(LayerSource { layer: layers.len(), point: s.point.clone(), cell: s.cell.clone() });
                layers.push(Layer {
                    name: name.clone(),
                    color,
                    positions: s.positions.iter().flat_map(|p| at(*p)).collect(),
                    normals: s.normals.iter().flatten().copied().collect(),
                    colors: None,
                    indices: s.triangles,
                    elements: if count > 0 { vec![(id, 0, count)] } else { Vec::new() },
                    props: vec![(id, props.clone())],
                    hidden: false,
                    points: false,
                    lines: s.lines,
                    line_elements: if lines > 0 { vec![(id, 0, lines)] } else { Vec::new() },
                });
            }
            if !s.dots.is_empty() {
                let pick = |v: &[u32]| s.dots.iter().map(|&d| v[d as usize]).collect::<Vec<u32>>();
                fields.layers.push(LayerSource { layer: layers.len(), point: pick(&s.point), cell: pick(&s.cell) });
                layers.push(Layer {
                    name: format!("{name} (points)"),
                    color,
                    positions: s.dots.iter().flat_map(|&d| at(s.positions[d as usize])).collect(),
                    normals: Vec::new(),
                    colors: None,
                    indices: Vec::new(),
                    elements: Vec::new(),
                    props: Vec::new(),
                    hidden: false,
                    points: true,
                    lines: Vec::new(),
                    line_elements: Vec::new(),
                });
            }
        }
        info.push(("Points".into(), mesh.points.len().to_string()));
        info.push(("Cells".into(), mesh.cells.len().to_string()));
        info.push(("Groups".into(), mesh.groups.len().to_string()));
        // the fields of every step, by their name
        for step in mesh.steps {
            let mut values = vec![None; fields.names.len()];
            for f in step.fields {
                let at = if f.at == FieldAt::Point { At::Point } else { At::Cell };
                let slot = match fields.names.iter().position(|(n, a, c)| *n == f.name && *a == at && *c == f.components) {
                    Some(slot) => slot,
                    None => {
                        fields.names.push((f.name.clone(), at, f.components));
                        values.push(None);
                        fields.names.len() - 1
                    }
                };
                values[slot] = Some(Values { at, components: f.components, values: f.values });
            }
            fields.steps.push(Step { time: step.time, fields: values });
        }
        for step in &mut fields.steps {
            step.fields.resize(fields.names.len(), None);
        }
        fields.finish();
        if !fields.names.is_empty() {
            info.push(("Fields".into(), fields.names.iter().map(|(n, _, _)| n.as_str()).collect::<Vec<_>>().join(", ")));
        }
        if fields.steps.len() > 1 {
            info.push(("Time steps".into(), fields.steps.len().to_string()));
        }
    }
    for (k, note) in doc.notes.iter().enumerate() {
        info.push((format!("Note {}", k + 1), note.clone()));
    }
    // the first field shows at once
    if !fields.channels.is_empty() {
        for source in &fields.layers {
            layers[source.layer].colors = Some(fields.colours(source, 0, 0));
        }
    }
    let fields = (!fields.channels.is_empty()).then(|| std::sync::Arc::new(fields));
    Loaded { layers, views: Vec::new(), project: ProjectData { info, fields, ..ProjectData::default() }, structural: Vec::new(), origin }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const BLOCK: &str = "# vtk DataFile Version 3.0
a block of two cells
ASCII
DATASET UNSTRUCTURED_GRID
POINTS 12 float
0 0 0 1 0 0 2 0 0 0 1 0 1 1 0 2 1 0
0 0 1 1 0 1 2 0 1 0 1 1 1 1 1 2 1 1
CELLS 2 18
8 0 1 4 3 6 7 10 9
8 1 2 5 4 7 8 11 10
CELL_TYPES 2
12
12
POINT_DATA 12
SCALARS temperature float 1
LOOKUP_TABLE default
0 1 2 0 1 2 0 1 2 0 1 2
CELL_DATA 2
VECTORS force float
1 0 0 0 0 3
";

    #[test]
    fn a_vtk_file_opens_as_a_mesh_coloured_by_its_first_field() {
        let source = Source::Bytes { name: "block.vtk".into(), data: Arc::new(BLOCK.as_bytes().to_vec()) };
        let model = super::super::load_with(&source, &Progress::default()).expect("VTK");
        assert_eq!(model.format, "VTK");
        // the ten outer quads of the two hexahedra
        assert_eq!(model.triangle_count(), 20);
        assert_eq!(model.bounds.size(), [2.0, 1.0, 1.0]);
        let fields = model.project.fields.as_ref().expect("fields");
        let labels: Vec<&str> = fields.channels.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["temperature", "force (length)", "force X", "force Y", "force Z"]);
        let layer = &model.layers[0];
        let colours = layer.colors.as_ref().expect("coloured by the temperature");
        assert_eq!(colours.len(), layer.vertex_count());
        assert!(colours.iter().any(|c| c[..3] == crate::cloud_color::ramp(0.0)) && colours.iter().any(|c| c[..3] == crate::cloud_color::ramp(1.0)));
        assert!(model.project.info.iter().any(|(k, v)| k == "Cells" && v == "2"));
    }

    #[test]
    fn a_parallel_file_in_the_browser_asks_for_the_desktop_app() {
        let pvtu = r#"<VTKFile type="PUnstructuredGrid" version="1.0" byte_order="LittleEndian"><PUnstructuredGrid GhostLevel="0"><PPoints><PDataArray type="Float32" NumberOfComponents="3"/></PPoints><Piece Source="part_0.vtu"/></PUnstructuredGrid></VTKFile>"#;
        let source = Source::Bytes { name: "run.pvtu".into(), data: Arc::new(pvtu.as_bytes().to_vec()) };
        let error = super::super::load_with(&source, &Progress::default()).unwrap_err();
        assert!(error.contains("needs the files beside it"), "{error}");
    }

    #[test]
    fn vtk_extensions_are_registered() {
        for extension in ["vtk", "vtu", "vtp", "vts", "vtr", "vti", "pvtu", "vtm", "pvd"] {
            let loader = super::super::registry().into_iter().find(|l| l.extensions().contains(&extension)).expect(extension);
            assert_eq!(loader.name(), "VTK");
        }
    }
}
