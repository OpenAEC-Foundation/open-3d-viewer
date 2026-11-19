//! Adapter from the independent readers of finite element meshes, simulation results and
//! structural analysis models (crate fem-read, repository fem-dll; crate saf-read, repository
//! saf-dll) to the app's model: the outer faces of each group of cells a layer, the fields with
//! where the layers' vertices come from, and the analysis model copied into the one of
//! bim-structural.

use super::{Loaded, Loader, Phase, Progress, Source};
use crate::fields::{At, Fields, LayerSource, Step, Values};
use crate::model::{Layer, ProjectData};
use bim_structural::AnalysisModel;
use fem_read::fem_model::{self, Document, FieldAt};
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
        read(source, progress, fem_read::read)
    }
}

/// The colours the groups of a mesh are drawn in while no field colours them.
const GROUP_COLOURS: [[f32; 3]; 6] = [[0.72, 0.72, 0.74], [0.55, 0.67, 0.80], [0.80, 0.68, 0.52], [0.60, 0.76, 0.62], [0.78, 0.60, 0.66], [0.70, 0.66, 0.82]];

/// A reader of fem-read or saf-read: the file's name and bytes, and the files beside it.
type Read = fn(&str, &[u8], fem_read::Side) -> Result<Document, String>;

fn read(source: &Source, progress: &Progress, reader: Read) -> Result<Loaded, String> {
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
    let doc = reader(&source.name(), &data, &side).map_err(|e| format!("{}: {e}", source.name()))?;
    Ok(loaded(doc, &source.name()))
}

/// The layers, fields, analysis model and project information of a document.
fn loaded(doc: Document, file: &str) -> Loaded {
    let mut info = vec![("Format".to_string(), doc.format.clone())];
    let mut layers = Vec::new();
    let mut fields = Fields::default();
    // far from the origin: shown around the middle of the mesh and the analysis model
    let mut points: Vec<DVec3> = doc.mesh.iter().flat_map(|m| m.points.iter().map(|p| DVec3::from(*p))).collect();
    points.extend(doc.analysis.iter().flat_map(|a| a.nodes.iter().map(|n| DVec3::from(n.p))));
    let (lo, hi) = points.iter().filter(|p| p.is_finite()).fold((DVec3::INFINITY, DVec3::NEG_INFINITY), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    let origin = (lo.cmple(hi).all() && lo.abs().max(hi.abs()).max_element() > 1000.0).then(|| ((lo + hi) * 0.5).round());
    let mut structural = Vec::new();
    if let Some(analysis) = doc.analysis {
        let model = structural_model(analysis, &doc.format, file);
        let sam = bim_structural::SamDocument { origin: DVec3::ZERO, source: model.source.clone(), models: vec![model] };
        // the summary of the analysis model, after the format
        info.extend(super::sam::info(&sam, origin.unwrap_or(DVec3::ZERO)).into_iter().skip(1).filter(|(k, _)| k != "World origin (metres)"));
        structural = sam.models;
    }
    if let Some(mesh) = doc.mesh {
        let anchor = origin;
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
                    splats: None, texturing: None,
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
                    splats: None, texturing: None,
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
    // the notes of an analysis model are among its summary already
    for (k, note) in doc.notes.iter().enumerate().filter(|_| structural.is_empty()) {
        info.push((format!("Note {}", k + 1), note.clone()));
    }
    // the first field shows at once
    if !fields.channels.is_empty() {
        for source in &fields.layers {
            layers[source.layer].colors = Some(fields.colours(source, 0, 0));
        }
    }
    let fields = (!fields.channels.is_empty()).then(|| std::sync::Arc::new(fields));
    Loaded { layers, views: Vec::new(), project: ProjectData { info, fields, ..ProjectData::default() }, structural, origin }
}

/// The FEM and analysis formats of fem-read and saf-read: their name, extensions and reader.
pub struct FemLoader {
    name: &'static str,
    extensions: &'static [&'static str],
    read: Read,
}

pub static INP: FemLoader = FemLoader { name: "FEM input deck", extensions: fem_read::INP, read: fem_read::inp::read };
pub static BDF: FemLoader = FemLoader { name: "FEM bulk data", extensions: fem_read::BDF, read: fem_read::bdf::read };
pub static MSH: FemLoader = FemLoader { name: "Finite element mesh", extensions: fem_read::MSH, read: |name, data, _| fem_read::msh::read(name, data) };
pub static S2K: FemLoader = FemLoader { name: "Structural analysis model (.s2k)", extensions: fem_read::S2K, read: |name, data, _| fem_read::s2k::read(name, data) };
pub static SAF: FemLoader = FemLoader { name: "SAF", extensions: saf_read::EXTENSIONS, read: |name, data, _| saf_read::read(name, data) };

impl Loader for FemLoader {
    fn name(&self) -> &'static str {
        self.name
    }

    fn extensions(&self) -> &'static [&'static str] {
        self.extensions
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        read(source, progress, self.read)
    }
}

/// An analysis model of the reader in the suite's analysis model, in the file's metres.
fn structural_model(a: fem_model::Analysis, format: &str, file: &str) -> AnalysisModel {
    use bim_structural as s;
    use fem_model as f;
    let source = s::SourceInfo { format: format.to_string(), file: file.to_string(), application: a.application.clone(), ..s::SourceInfo::default() };
    let mut m = s::AnalysisModel::new(1, a.name.clone(), source);
    let id = |kind: s::IdKind, k: usize| s::sam_id(kind, k as u64 + 1);
    let dof = |d: &f::Dof| match d {
        f::Dof::Free => s::Dof::Free,
        f::Dof::Fixed => s::Dof::Fixed,
        f::Dof::Spring(k) => s::Dof::Spring(*k),
    };
    for (k, x) in a.materials.iter().enumerate() {
        m.materials.push(s::Material { id: id(s::IdKind::Material, k), name: x.name.clone(), e: x.e, g: x.g, nu: x.nu, density: x.density, alpha_t: x.alpha });
    }
    for (k, x) in a.sections.iter().enumerate() {
        let shape = match x.shape {
            f::Shape::Rectangle { b, h } => s::Shape::Rect { b, h },
            f::Shape::Circle { d } => s::Shape::Circle { d },
            f::Shape::Tube { d, t } => s::Shape::Chs { d, t },
            f::Shape::Box { b, h, t } => s::Shape::Rhs { b, h, t, r_out: 0.0, r_in: 0.0 },
            f::Shape::I { h, b, tw, tf } => s::Shape::I { h, b, tw, tf, r: 0.0, slope: 0.0 },
            f::Shape::Channel { h, b, tw, tf } => s::Shape::U { h, b, tw, tf, r1: 0.0, r2: 0.0, slope: 0.0 },
            f::Shape::Tee { h, b, tw, tf } => s::Shape::T { h, b, tw, tf, r: 0.0 },
            f::Shape::Angle { h, b, t } => s::Shape::L { h, b, t, r1: 0.0, r2: 0.0 },
            f::Shape::Unknown => s::Shape::Unknown,
        };
        let imported = s::ImportedProps { area: x.props.area, iy: x.props.iy, iz: x.props.iz, it: x.props.it, ..s::ImportedProps::default() };
        let given = imported.area.is_some() || imported.iy.is_some();
        let (props, props_source) = s::section::compute(&shape, given.then_some(&imported));
        m.sections.push(s::Section { id: id(s::IdKind::Section, k), name: x.name.clone(), shape, props, props_source, imported: given.then_some(imported), material: x.material.map(|v| id(s::IdKind::Material, v)) });
    }
    for (k, n) in a.nodes.iter().enumerate() {
        let support = n.support.map(|held| {
            let dofs: s::Dofs = std::array::from_fn(|d| dof(&held[d]));
            let preset = s::support::classify(&dofs, s::Dimension::Space3d);
            s::Support { dofs, frame: s::DofFrame::Global, named: None, preset }
        });
        m.nodes.push(s::Node { id: id(s::IdKind::Node, k), number: n.name.clone(), p: DVec3::from(n.p), frame: None, support, origin: s::NodeOrigin::Analytical });
    }
    let length = |b: &f::Member| (DVec3::from(a.nodes[b.nodes[1]].p) - DVec3::from(a.nodes[b.nodes[0]].p)).length();
    for (k, b) in a.members.iter().enumerate() {
        let axis = DVec3::from(a.nodes[b.nodes[1]].p) - DVec3::from(a.nodes[b.nodes[0]].p);
        // a vector the local y axis points to, or the default
        let (z_ref, orientation) = match b.orientation.map(DVec3::from).map(|y| (axis.normalize_or_zero().cross(y).normalize_or_zero(), y)) {
            Some((z, y)) if z.length_squared() > 0.5 => (z, s::Orientation::RefVector(y)),
            _ => (s::frame::default_z_ref(axis), s::Orientation::Default),
        };
        let releases = b.releases.map(|end| end.iter().any(|r| *r).then(|| s::Hinge { dofs: end.map(|free| if free { s::Dof::Free } else { s::Dof::Fixed }), named: None }));
        let kind = match b.kind {
            f::MemberKind::Truss => s::MemberKind::Truss,
            f::MemberKind::Cable => s::MemberKind::Cable,
            _ => s::MemberKind::Beam,
        };
        m.members.push(s::Member {
            id: id(s::IdKind::Member, k),
            number: b.name.clone(),
            kind,
            nodes: vec![id(s::IdKind::Node, b.nodes[0]), id(s::IdKind::Node, b.nodes[1])],
            geometry: s::Curve::Line,
            z_ref,
            orientation,
            section: b.section.map(|v| (id(s::IdKind::Section, v), None)),
            section_rotation: b.angle,
            insertion: s::Insertion::default(),
            releases,
            material: b.material.map(|v| id(s::IdKind::Material, v)),
            links: Vec::new(),
        });
    }
    for (k, p) in a.plates.iter().enumerate() {
        let kind = match p.kind {
            f::PlateKind::Floor => s::SurfaceKind::Plate,
            f::PlateKind::Membrane => s::SurfaceKind::Membrane,
            f::PlateKind::Wall | f::PlateKind::Shell => s::SurfaceKind::Shell,
        };
        m.surfaces.push(s::SurfaceMember {
            id: id(s::IdKind::Surface, k),
            number: p.name.clone(),
            kind,
            boundary: p.nodes.iter().map(|&n| id(s::IdKind::Node, n)).collect(),
            openings: Vec::new(),
            thickness: p.thickness.filter(|t| *t > 0.0).map_or(s::Thickness::Unknown, s::Thickness::Uniform),
            offset_z: 0.0,
            x_dir: None,
            material: p.material.map(|v| id(s::IdKind::Material, v)),
            links: Vec::new(),
        });
    }
    for (k, c) in a.cases.iter().enumerate() {
        let (action_type, action_source) = match &c.kind {
            f::CaseKind::Permanent => (s::ActionType::Permanent, String::new()),
            f::CaseKind::Variable => (s::ActionType::Variable, String::new()),
            f::CaseKind::Accidental => (s::ActionType::Accidental, String::new()),
            f::CaseKind::Unknown => (s::ActionType::User(String::new()), String::new()),
            f::CaseKind::Other(t) => (s::ActionType::User(t.clone()), t.clone()),
        };
        m.load_cases.push(s::LoadCase { id: id(s::IdKind::LoadCase, k), number: (k + 1).to_string(), name: c.name.clone(), action_type, action_source, self_weight: c.self_weight.map(|f| DVec3::new(0.0, 0.0, -f)), coefficient: 1.0 });
    }
    for (k, c) in a.combinations.iter().enumerate() {
        let kind = c.kind.to_ascii_uppercase();
        let purpose = if kind.contains("ULS") { s::Purpose::Uls } else if kind.contains("SLS") { s::Purpose::Sls } else if kind.contains("ALS") { s::Purpose::Als } else { s::Purpose::User(c.kind.clone()) };
        let terms = c
            .terms
            .iter()
            .map(|(t, factor)| {
                let r = match t {
                    f::Term::Case(c) => s::LoadRef::Case(id(s::IdKind::LoadCase, *c)),
                    f::Term::Combination(c) => s::LoadRef::Combination(id(s::IdKind::Combination, *c)),
                };
                (r, *factor)
            })
            .collect();
        m.combinations.push(s::Combination { id: id(s::IdKind::Combination, k), number: (k + 1).to_string(), name: c.name.clone(), purpose, terms });
    }
    let frame = |axes: f::Axes| if axes == f::Axes::Local { s::LoadFrame::Local } else { s::LoadFrame::Global };
    let mut actions: Vec<(usize, s::Target, s::LoadFrame, s::Load)> = a
        .actions
        .iter()
        .map(|x| {
            let (target, axes, load) = match &x.load {
                f::Load::Node { node, force, moment } => (s::Target::Node(id(s::IdKind::Node, *node)), s::LoadFrame::Global, s::Load::NodalForce { f: DVec3::from(*force), m: DVec3::from(*moment) }),
                f::Load::MemberPoint { member, axes, at, force } => {
                    let l = length(&a.members[*member]);
                    (s::Target::Member { id: id(s::IdKind::Member, *member), span: None }, frame(*axes), s::Load::MemberPoint { x: at * l, f: DVec3::from(*force), m: DVec3::ZERO })
                }
                f::Load::MemberLine { member, axes, dir, q, from, to } => {
                    let l = length(&a.members[*member]);
                    let dist = match (*from <= 0.0 && *to >= 1.0, q[0] == q[1]) {
                        (true, true) => s::Distribution::Uniform(q[0]),
                        (true, false) => s::Distribution::Linear { q1: q[0], q2: q[1] },
                        _ => s::Distribution::Trapezoidal { a: from * l, b: to * l, q1: q[0], q2: q[1] },
                    };
                    (s::Target::Member { id: id(s::IdKind::Member, *member), span: None }, frame(*axes), s::Load::MemberDistributed { dir: DVec3::from(*dir), moment: false, dist })
                }
                f::Load::Plate { plate, axes, dir, q } => (s::Target::Surface(id(s::IdKind::Surface, *plate)), frame(*axes), s::Load::Surface { dir: DVec3::from(*dir), dist: s::SurfaceDistribution::Uniform(*q) }),
            };
            (x.case, target, axes, load)
        })
        .collect();
    // sorted by load case, for drawing them case by case
    actions.sort_by_key(|x| x.0);
    for (k, (case, target, frame, kind)) in actions.into_iter().enumerate() {
        m.actions.push(s::Action { id: id(s::IdKind::Action, k), load: s::LoadRef::Case(id(s::IdKind::LoadCase, case)), target, frame, kind, destabilising: false });
    }
    for note in &a.notes {
        m.diagnostics.push(s::Diagnostic { object: None, code: "reader_note".into(), message: note.clone() });
    }
    let duplicates = m.rebuild_index();
    debug_assert!(duplicates.is_empty(), "{duplicates:?}");
    m
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

    fn open(name: &str, text: &str) -> crate::model::Model {
        let source = Source::Bytes { name: name.into(), data: Arc::new(text.as_bytes().to_vec()) };
        super::super::load_with(&source, &Progress::default()).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    const PORTAL: &str = "TABLE:  \"PROGRAM CONTROL\"
   ProgramName=\"Analysis\"   CurrUnits=\"KN, m, C\"
TABLE:  \"JOINT COORDINATES\"
   Joint=1   XorR=0   Y=0   Z=0
   Joint=2   XorR=0   Y=0   Z=3
   Joint=3   XorR=4   Y=0   Z=3
   Joint=4   XorR=4   Y=0   Z=0
TABLE:  \"CONNECTIVITY - FRAME\"
   Frame=1   JointI=1   JointJ=2
   Frame=2   JointI=2   JointJ=3
   Frame=3   JointI=4   JointJ=3
TABLE:  \"FRAME SECTION PROPERTIES 01 - GENERAL\"
   SectionName=COL   Shape=Rectangular   t3=0.4   t2=0.3
TABLE:  \"FRAME SECTION ASSIGNMENTS\"
   Frame=1   AnalSect=COL
   Frame=3   AnalSect=COL
TABLE:  \"JOINT RESTRAINT ASSIGNMENTS\"
   Joint=1   U1=Yes   U2=Yes   U3=Yes   R1=Yes   R2=Yes   R3=Yes
   Joint=4   U1=Yes   U2=Yes   U3=Yes   R1=No   R2=No   R3=No
TABLE:  \"LOAD PATTERN DEFINITIONS\"
   LoadPat=LIVE   DesignType=Live
TABLE:  \"FRAME LOADS - DISTRIBUTED\"
   Frame=2   LoadPat=LIVE   CoordSys=GLOBAL   Type=Force   Dir=Gravity   RelDistA=0   RelDistB=1   FOverLA=5   FOverLB=5
END TABLE DATA
";

    #[test]
    fn an_analysis_text_model_opens_in_the_analysis_model() {
        let model = open("portal.s2k", PORTAL);
        assert_eq!(model.format, "Structural analysis model (.s2k)");
        let doc = &model.structural[0].doc;
        assert_eq!((doc.nodes.len(), doc.members.len()), (4, 3));
        assert_eq!(doc.nodes.iter().filter(|n| n.support.is_some()).count(), 2);
        let column = doc.section(doc.members[0].section.unwrap().0).unwrap();
        assert_eq!(column.shape, bim_structural::Shape::Rect { b: 0.3, h: 0.4 });
        assert!(column.props.area > 0.11);
        assert_eq!(doc.load_cases[0].action_type, bim_structural::ActionType::Variable);
        assert!(matches!(doc.actions[0].kind, bim_structural::Load::MemberDistributed { dist: bim_structural::Distribution::Uniform(q), .. } if q == 5e3));
        assert!(model.project.info.iter().any(|(k, v)| k == "Members" && v == "3"));
    }

    #[test]
    fn a_deck_of_beams_and_solids_opens_as_an_analysis_model_and_a_mesh() {
        let deck = "*NODE
1,0,0,0
2,1,0,0
3,1,1,0
4,0,1,0
5,0,0,1
6,1,0,1
7,1,1,1
8,0,1,1
9,0,0,3
*ELEMENT, TYPE=C3D8, ELSET=FOOT
1,1,2,3,4,5,6,7,8
*ELEMENT, TYPE=B31, ELSET=POST
2,5,9
*BOUNDARY
1,1,6
";
        let model = open("post.inp", deck);
        assert_eq!(model.format, "FEM input deck");
        assert_eq!(model.triangle_count(), 12, "the six faces of the foot");
        assert_eq!(model.layers[0].name, "FOOT");
        let doc = &model.structural[0].doc;
        assert_eq!((doc.members.len(), doc.nodes.len()), (1, 3), "the post, its two nodes and the supported one");
    }

    #[test]
    fn a_mesh_file_opens_with_its_groups() {
        let msh = "$MeshFormat
2.2 0 8
$EndMeshFormat
$PhysicalNames
1
2 1 \"floor\"
$EndPhysicalNames
$Nodes
4
1 0 0 0
2 1 0 0
3 1 1 0
4 0 1 0
$EndNodes
$Elements
1
1 3 2 1 1 1 2 3 4
$EndElements
";
        let model = open("floor.msh", msh);
        assert_eq!((model.layers[0].name.as_str(), model.triangle_count()), ("floor", 2));
    }

    #[test]
    fn the_analysis_formats_are_registered() {
        for (extension, name) in [("inp", "FEM input deck"), ("bdf", "FEM bulk data"), ("nas", "FEM bulk data"), ("msh", "Finite element mesh"), ("s2k", "Structural analysis model (.s2k)"), ("$2k", "Structural analysis model (.s2k)"), ("xlsx", "SAF")] {
            let loader = super::super::registry().into_iter().find(|l| l.extensions().contains(&extension)).expect(extension);
            assert_eq!(loader.name(), name);
        }
    }
}
