//! Vectorworks (.vwx documents and .sta templates) via the OpenAEC vwx reader: one layer per
//! Vectorworks layer and class, the drawn objects pickable, with their properties (name, object
//! type, layer, class, symbol, the values of their records and the IFC entity Vectorworks
//! keeps for them); lines, arcs and the outlines of shapes without a fill as line work; what is
//! on an invisible layer or in an invisible class starts switched off; the saved views of the
//! document; its sheet layers as sheets, with the lines and texts drawn on them and their
//! viewports, each with the view of the model it shows; its grid lines (GridAxis objects), its
//! spaces as rooms, and what the file says about itself as project information.
use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{
    DraftFill, DraftText, GridLine, Layer, ProjectData, Room, SavedView, Sheet, SheetRect,
    SheetViewport, ViewCamera, ViewKind,
};

/// Metres in a point, the measure of letters on paper.
const POINT: f32 = 0.0254 / 72.0;

pub struct VwxLoader;

impl Loader for VwxLoader {
    fn name(&self) -> &'static str {
        "Vectorworks"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["vwx", "sta"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default())
            .map(|loaded| loaded.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        progress.set(Phase::Convert);
        let scene =
            vwx_read::read(&data).map_err(|e| format!("Cannot read Vectorworks file: {e}"))?;
        // the saved views, then the views the viewports of the sheets show
        let mut views: Vec<SavedView> = scene.views.iter().map(view).collect();
        let mut project = ProjectData {
            sheets: scene.sheets.iter().map(|s| sheet(s, &mut views)).collect(),
            ..ProjectData::default()
        };
        // a viewport shows some of the layers and classes only: its view leaves out the objects
        // of the others
        for (from, sheet) in scene.sheets.iter().zip(&project.sheets) {
            for (from, viewport) in from.viewports.iter().zip(&sheet.viewports) {
                let hidden = left_out(from, &scene.meshes);
                if let (Some(view), false) = (viewport.view, hidden.is_empty()) {
                    project
                        .switched_off
                        .insert(crate::model::view3d_key(view), hidden);
                }
            }
        }
        project.info = info(&scene.info);
        project.grids = grids(&scene.meshes);
        project.rooms = rooms(&scene.meshes);
        let layers = scene.meshes.into_iter().map(layer).collect();
        Ok(Loaded {
            structural: Vec::new(),
            origin: None,
            layers,
            views,
            project,
        })
    }
}

/// A saved view of the document: the way it looks at the model. Vectorworks' saved views are
/// read as parallel views, which fit the model.
fn view(view: &vwx_read::model::View) -> SavedView {
    SavedView {
        name: view.name.clone(),
        kind: ViewKind::ThreeD,
        camera: Some(ViewCamera {
            eye: [0.0; 3],
            forward: view.forward,
            perspective: false,
            fov_y: None,
        }),
        clip: None,
        plan: None,
        level: None,
        folder: Vec::new(),
        style: None,
        drawing: None,
    }
}

/// A sheet layer of the document as a sheet: its number and title, what is drawn on the paper
/// itself (the lines and texts of its title block and notes), and its viewports, each with the
/// view of the model it shows added to `views`. Its paper is taken to be what those cover.
fn sheet(sheet: &vwx_read::model::Sheet, views: &mut Vec<SavedView>) -> Sheet {
    let texts: Vec<DraftText> = sheet
        .texts
        .iter()
        .map(|t| DraftText {
            element: t.id,
            // the reader gives the top left corner of the box
            at: [t.at[0], t.at[1] - t.height],
            width: t.width,
            height: t.height,
            size: t.size * POINT,
            align: 0,
            angle: 0.0,
            text: t.text.clone(),
            face: crate::typeface::Face::default(),
        })
        .collect();
    let viewports: Vec<SheetViewport> = sheet
        .viewports
        .iter()
        .map(|v| {
            // a section viewport: the sheet has the drawing of its cut among its lines, which a
            // view of the whole model would only cover
            if v.section {
                return placed(v, None);
            }
            // the model as the viewport looks at it: a parallel view of its own
            views.push(SavedView {
                name: v.name.clone(),
                kind: ViewKind::ThreeD,
                camera: Some(ViewCamera {
                    eye: [0.0; 3],
                    forward: v.forward,
                    perspective: false,
                    fov_y: None,
                }),
                clip: None,
                plan: None,
                level: None,
                folder: vec![sheet.number.clone()],
                style: None,
                drawing: None,
            });
            placed(v, Some(views.len() - 1))
        })
        .collect();
    let corners = sheet
        .lines
        .iter()
        .flatten()
        .copied()
        .chain(
            texts
                .iter()
                .flat_map(|t| [t.at, [t.at[0] + t.width, t.at[1] + t.height]]),
        )
        .chain(viewports.iter().flat_map(|v| [v.rect.min, v.rect.max]));
    let paper = corners
        .map(|p| SheetRect { min: p, max: p })
        .reduce(SheetRect::union);
    // what its section viewports cut, filled
    let fills = sheet
        .fills
        .iter()
        .map(|f| DraftFill {
            element: f.id,
            loops: vec![f.outline.clone()],
            color: f.color,
        })
        .collect();
    Sheet {
        lines: sheet.lines.clone(),
        texts,
        fills,
        ..Sheet::new(sheet.number.clone(), sheet.title.clone(), paper, viewports)
    }
}

/// A viewport as it lies on its sheet, showing view `view` of the model (none: its sheet has
/// its drawing).
fn placed(v: &vwx_read::model::Viewport, view: Option<usize>) -> SheetViewport {
    SheetViewport {
        title: v.name.clone(),
        view,
        rect: SheetRect {
            min: v.min,
            max: v.max,
        },
        estimated: false,
        scale: format!("1:{}", v.scale),
        detail: None,
        label: None,
        origin: v.origin,
        ratio: 1.0 / v.scale.max(f32::MIN_POSITIVE),
        drawing: None,
        anchor: None,
        turn: [1.0, 0.0],
        draft: None,
        written: None,
        face: crate::typeface::Face::default(),
        split: None,
    }
}

/// The objects a viewport does not show: those on the design layers and in the classes it
/// leaves out.
fn left_out(
    viewport: &vwx_read::model::Viewport,
    meshes: &[vwx_read::model::Mesh],
) -> std::collections::HashSet<u64> {
    let hides = |m: &&vwx_read::model::Mesh| {
        viewport.hidden_layers.contains(&m.layer) || viewport.hidden_classes.contains(&m.class)
    };
    meshes
        .iter()
        .filter(hides)
        .flat_map(|m| m.props.iter().map(|p| p.0))
        .collect()
}

/// What the file says about itself: the application that saved it and its version.
fn info(info: &vwx_read::model::Info) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(application) = &info.application {
        out.push(("application".to_string(), application.clone()));
    }
    if let Some(version) = &info.version {
        out.push(("version".to_string(), version.clone()));
    }
    out.push(("Vectorworks objects".to_string(), info.objects.to_string()));
    out
}

/// The properties of each drawn object of the meshes, parsed, with the mesh it is in.
fn objects(
    meshes: &[vwx_read::model::Mesh],
) -> impl Iterator<
    Item = (
        &vwx_read::model::Mesh,
        u64,
        serde_json::Map<String, serde_json::Value>,
    ),
> {
    meshes.iter().flat_map(|m| {
        m.props
            .iter()
            .filter_map(move |(id, props)| match serde_json::from_str(props) {
                Ok(serde_json::Value::Object(map)) => Some((m, *id, map)),
                _ => None,
            })
    })
}

/// A property as text (numbers as they are, texts without quotes; empty when missing).
fn text(props: &serde_json::Map<String, serde_json::Value>, key: &str) -> String {
    match props.get(key) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Null) | None => String::new(),
        Some(v) => v.to_string(),
    }
}

/// The grid lines of the document: each GridAxis object as the longest of its line segments
/// (its bubbles are circles of short ones), named by its label.
fn grids(meshes: &[vwx_read::model::Mesh]) -> Vec<GridLine> {
    let mut out = Vec::new();
    for (mesh, id, props) in objects(meshes) {
        if text(&props, "Object") != "GridAxis" {
            continue;
        }
        let point = |v: u32| {
            let k = v as usize * 3;
            [
                mesh.positions[k],
                mesh.positions[k + 1],
                mesh.positions[k + 2],
            ]
        };
        let length = |[a, b]: [[f32; 3]; 2]| (0..3).map(|k| (b[k] - a[k]).powi(2)).sum::<f32>();
        let segments = mesh
            .line_elements
            .iter()
            .filter(|e| e.0 == id)
            .flat_map(|&(_, first, count)| (first..first + count).step_by(2))
            .filter_map(|k| {
                let (a, b) = (
                    *mesh.lines.get(k as usize)?,
                    *mesh.lines.get(k as usize + 1)?,
                );
                Some([point(a), point(b)])
            });
        if let Some([a, b]) = segments.max_by(|x, y| length(*x).total_cmp(&length(*y))) {
            out.push(GridLine {
                name: text(&props, "GridAxis.Label"),
                a,
                b,
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The spaces of the document as rooms: number, name, the design layer they are on, and their
/// areas and heights as Vectorworks shows them.
fn rooms(meshes: &[vwx_read::model::Mesh]) -> Vec<Room> {
    let mut out = Vec::new();
    for (_, _, props) in objects(meshes) {
        if text(&props, "Object") != "Space" {
            continue;
        }
        let name = [
            text(&props, "Space.Name"),
            text(&props, "Space.11_NameDisplay"),
        ]
        .into_iter()
        .find(|n| !n.is_empty())
        .unwrap_or_default();
        let level = Some(text(&props, "Layer")).filter(|l| !l.is_empty());
        let mut extra = Vec::new();
        for (key, label) in [
            ("Space.11_Area", "Area"),
            ("Space.11_Gross Area", "Gross area"),
            ("Space.Net Height", "Net height"),
            ("Space.11_Gross Height", "Gross height"),
        ] {
            let value = text(&props, key);
            if !value.is_empty() {
                extra.push((label.to_string(), value));
            }
        }
        out.push(Room {
            number: text(&props, "Space.Number"),
            name,
            level,
            extra,
        });
    }
    out.sort_by(|a, b| a.number.cmp(&b.number).then(a.name.cmp(&b.name)));
    out
}

/// The viewer layer of a reader mesh (both metres, Z up).
fn layer(mesh: vwx_read::model::Mesh) -> Layer {
    Layer {
        name: mesh.name,
        color: mesh.color,
        positions: mesh.positions,
        normals: mesh.normals,
        colors: mesh.colors,
        indices: mesh.indices,
        elements: mesh.elements,
        props: mesh.props,
        hidden: mesh.hidden,
        points: false,
        lines: mesh.lines,
        line_elements: mesh.line_elements,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{load_with, Progress, Source};
    use std::sync::Arc;

    /// A mesh holding a grid line (label 1, with a short segment of its bubble) and a space.
    fn grid_and_space() -> vwx_read::model::Mesh {
        vwx_read::model::Mesh {
            positions: vec![0.0, 0.0, 0.0, 0.0, 10.0, 0.0, 0.0, 10.0, 0.0, 0.2, 10.2, 0.0],
            lines: vec![0, 1, 2, 3],
            line_elements: vec![(7, 0, 4)],
            props: vec![
                (7, r#"{"Object":"GridAxis","GridAxis.Label":"1"}"#.into()),
                (8, r#"{"Object":"Space","Layer":"GF","Space.Name":"Kitchen","Space.Number":"1","Space.11_Area":"25.2 sq m"}"#.into()),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn grid_axes_become_grid_lines() {
        let grids = super::grids(&[grid_and_space()]);
        assert_eq!(
            grids,
            [crate::model::GridLine {
                name: "1".into(),
                a: [0.0, 0.0, 0.0],
                b: [0.0, 10.0, 0.0]
            }]
        );
    }

    #[test]
    fn spaces_become_rooms() {
        let rooms = super::rooms(&[grid_and_space()]);
        assert_eq!(rooms.len(), 1);
        assert_eq!(
            (
                rooms[0].number.as_str(),
                rooms[0].name.as_str(),
                rooms[0].level.as_deref()
            ),
            ("1", "Kitchen", Some("GF"))
        );
        assert_eq!(
            rooms[0].extra,
            [("Area".to_string(), "25.2 sq m".to_string())]
        );
    }

    #[test]
    fn vectorworks_files_reach_the_vwx_reader() {
        let source = Source::Bytes {
            name: "house.VWX".into(),
            data: Arc::new(Vec::new()),
        };
        let error = load_with(&source, &Progress::default()).unwrap_err();
        assert!(
            error.starts_with("Cannot read Vectorworks file:"),
            "{error}"
        );
    }

    /// A file made with Vectorworks 2027 by the reader's own sample script (`51_symbol_two` of the
    /// vwx-dll oracle ladder): the symbol `Blok` (a box of 1000 x 500 x 1000 mm) placed at the
    /// origin and, turned 45 degrees, at (3000, 1000). Sample files are kept out of the
    /// repository; the test runs where `VWX_SAMPLES` names their folder.
    #[test]
    fn reads_a_vectorworks_file_with_two_placed_symbols() {
        let Some(samples) = std::env::var_os("VWX_SAMPLES") else {
            return;
        };
        let path = std::path::PathBuf::from(samples).join("ladder/2027/51_symbol_two.vwx");
        let model = super::super::load(&path).expect("Vectorworks sample");
        assert_eq!(model.format, "Vectorworks");
        assert_eq!(model.triangle_count(), 24);
        let size = model.bounds.size();
        assert!((size[2] - 1.0).abs() < 1e-4, "{size:?}");
        assert!(
            model
                .props
                .values()
                .any(|p| p.contains(r#""Symbol":"Blok""#)),
            "{:?}",
            model.props.values().next()
        );
        let layer = &model.layers[0];
        assert!(layer.name.starts_with("Ontwerplaag-1"), "{}", layer.name);
        assert_eq!(layer.elements.len(), 2);
    }

    /// `140_saved_view` of the ladder: a view saved as "Aanzicht A" after the model was turned
    /// 60 degrees about x and 45 about z.
    #[test]
    fn reads_the_saved_views_of_a_vectorworks_file() {
        let Some(samples) = std::env::var_os("VWX_SAMPLES") else {
            return;
        };
        let path = std::path::PathBuf::from(samples).join("ladder/2027/140_saved_view.vwx");
        let model = super::super::load(&path).expect("Vectorworks sample");
        let view = model
            .views
            .iter()
            .find(|v| v.name == "Aanzicht A")
            .expect("the saved view");
        let forward = view.camera.expect("its camera").forward;
        // down at 30 degrees
        assert!(
            (forward[2] + 0.5).abs() < 1e-5 && (forward[0] + forward[1]).abs() < 1e-5,
            "{forward:?}"
        );
    }

    /// `02_line` of the ladder: a line is line work, pickable.
    #[test]
    fn reads_the_line_work_of_a_vectorworks_file() {
        let Some(samples) = std::env::var_os("VWX_SAMPLES") else {
            return;
        };
        let path = std::path::PathBuf::from(samples).join("ladder/2027/02_line.vwx");
        let model = super::super::load(&path).expect("Vectorworks sample");
        let layer = model
            .layers
            .iter()
            .find(|l| !l.lines.is_empty())
            .expect("a layer with a line");
        assert_eq!((layer.lines.len(), layer.line_elements.len()), (2, 1));
    }

    /// `90_record` of the ladder: the line with the record `Gegevens`, field `Naam`.
    #[test]
    fn reads_the_records_of_a_vectorworks_file() {
        let Some(samples) = std::env::var_os("VWX_SAMPLES") else {
            return;
        };
        let path = std::path::PathBuf::from(samples).join("ladder/2027/90_record.vwx");
        let model = super::super::load(&path).expect("Vectorworks sample");
        assert!(
            model
                .props
                .values()
                .any(|p| p.contains(r#""Gegevens.Naam":"Balk-1""#)),
            "{:?}",
            model.props.values().collect::<Vec<_>>()
        );
    }

    #[test]
    fn sheet_layers_become_sheets() {
        let from = vwx_read::model::Sheet {
            id: 9,
            number: "210".into(),
            title: "Plans".into(),
            lines: vec![[[0.0, 0.0], [0.841, 0.0]], [[0.841, 0.0], [0.841, 0.594]]],
            fills: vec![vwx_read::model::Fill {
                id: 21,
                outline: vec![[0.1, 0.1], [0.2, 0.1], [0.2, 0.2]],
                color: [136, 136, 136],
            }],
            texts: vec![vwx_read::model::Text {
                id: 12,
                text: "Sheet No.".into(),
                at: [0.7, 0.05, 0.0],
                width: 0.04,
                height: 0.005,
                size: 12.0,
                lines: 1,
                ..Default::default()
            }],
            viewports: vec![vwx_read::model::Viewport {
                id: 14,
                name: "Front Elevation".into(),
                min: [0.1, 0.2],
                max: [0.6, 0.45],
                scale: 50.0,
                origin: [0.2, 0.25],
                forward: [0.0, 1.0, 0.0],
                up: [0.0, 0.0, 1.0],
                section: false,
                hidden_layers: vec!["01-Slab".into()],
                hidden_classes: vec!["Space".into()],
            }],
        };
        let mut views = vec![super::view(&vwx_read::model::View {
            name: "Aanzicht A".into(),
            forward: [0.0, 0.6, -0.8],
            up: [0.0, 0.8, 0.6],
        })];
        let sheet = super::sheet(&from, &mut views);
        // its viewport shows a view of its own, after the saved views
        let viewport = &sheet.viewports[0];
        assert_eq!(
            (
                viewport.title.as_str(),
                viewport.view,
                viewport.scale.as_str(),
                viewport.ratio
            ),
            ("Front Elevation", Some(1), "1:50", 0.02)
        );
        assert_eq!(
            (views[1].name.as_str(), views[1].camera.unwrap().forward),
            ("Front Elevation", [0.0, 1.0, 0.0])
        );
        // a section viewport has its drawing on the sheet, and no view of the model
        let section = vwx_read::model::Sheet {
            viewports: vec![vwx_read::model::Viewport {
                section: true,
                ..from.viewports[0].clone()
            }],
            ..from.clone()
        };
        let before = views.len();
        assert_eq!(
            (
                super::sheet(&section, &mut views).viewports[0].view,
                views.len()
            ),
            (None, before)
        );
        // it leaves out what is on a layer or in a class it does not show
        let mesh = |layer: &str, class: &str, id: u64| vwx_read::model::Mesh {
            layer: layer.into(),
            class: class.into(),
            props: vec![(id, String::new())],
            ..Default::default()
        };
        let meshes = [
            mesh("00-Slab", "Floor", 1),
            mesh("01-Slab", "Floor", 2),
            mesh("00-Floor", "Space", 3),
        ];
        let mut hidden: Vec<u64> = super::left_out(&from.viewports[0], &meshes)
            .into_iter()
            .collect();
        hidden.sort();
        assert_eq!(hidden, [2, 3]);
        assert_eq!(
            (
                sheet.number.as_str(),
                sheet.name.as_str(),
                sheet.title.as_str()
            ),
            ("210", "Plans", "210 Plans")
        );
        assert_eq!(sheet.lines.len(), 2);
        assert_eq!(
            (
                sheet.fills.len(),
                sheet.fills[0].color,
                sheet.fills[0].loops[0].len()
            ),
            (1, [136, 136, 136], 3)
        );
        // the text box from its lower left corner; letters of 12 points
        let text = &sheet.texts[0];
        assert!(
            (text.at[1] - 0.045).abs() < 1e-6 && (text.size - 0.0042333).abs() < 1e-6,
            "{text:?}"
        );
        let paper = sheet.paper.unwrap();
        assert_eq!((paper.min, paper.max), ([0.0, 0.0], [0.841, 0.594]));
    }

    #[test]
    fn saved_views_look_the_way_the_file_says() {
        let saved = super::view(&vwx_read::model::View {
            name: "Aanzicht A".into(),
            forward: [0.0, 0.6, -0.8],
            up: [0.0, 0.8, 0.6],
        });
        assert_eq!(
            (saved.name.as_str(), saved.kind),
            ("Aanzicht A", super::ViewKind::ThreeD)
        );
        let camera = saved.camera.unwrap();
        assert_eq!(
            (camera.forward, camera.perspective),
            ([0.0, 0.6, -0.8], false)
        );
    }

    #[test]
    fn templates_are_vectorworks_files_too() {
        let source = Source::Bytes {
            name: "plan.sta".into(),
            data: Arc::new(Vec::new()),
        };
        let error = load_with(&source, &Progress::default()).unwrap_err();
        assert!(
            error.starts_with("Cannot read Vectorworks file:"),
            "{error}"
        );
    }

    #[test]
    fn meshes_map_onto_layers() {
        let mesh = vwx_read::model::Mesh {
            name: "Ontwerplaag-1 / Muren".into(),
            color: [1.0, 0.0, 0.0],
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            indices: vec![0, 1, 2],
            elements: vec![(7, 0, 3)],
            ..Default::default()
        };
        let layer = super::layer(mesh);
        assert_eq!(layer.name, "Ontwerplaag-1 / Muren");
        assert_eq!(layer.triangle_count(), 1);
        assert_eq!(layer.elements, vec![(7, 0, 3)]);
        assert_eq!(layer.normals.len(), layer.positions.len());
    }
}
