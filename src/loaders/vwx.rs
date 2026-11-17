//! Vectorworks (.vwx documents and .sta templates) via the OpenAEC vwx reader: one layer per
//! Vectorworks layer and class, the drawn objects pickable, with their properties (name, object
//! type, layer, class, symbol, the values of their records and the IFC entity Vectorworks
//! keeps for them); lines, arcs and the outlines of shapes without a fill as line work; what is
//! on an invisible layer or in an invisible class starts switched off; the saved views of the
//! document.
use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{Layer, ProjectData, SavedView, ViewCamera, ViewKind};

pub struct VwxLoader;

impl Loader for VwxLoader {
    fn name(&self) -> &'static str {
        "Vectorworks"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["vwx", "sta"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        progress.set(Phase::Convert);
        let scene = vwx_read::read(&data).map_err(|e| format!("Cannot read Vectorworks file: {e}"))?;
        let views = scene.views.iter().map(view).collect();
        let layers = scene.meshes.into_iter().map(layer).collect();
        Ok(Loaded { structural: Vec::new(), origin: None, layers, views, project: ProjectData::default() })
    }
}

/// A saved view of the document: the way it looks at the model. Vectorworks' saved views are
/// read as parallel views, which fit the model.
fn view(view: &vwx_read::model::View) -> SavedView {
    SavedView {
        name: view.name.clone(),
        kind: ViewKind::ThreeD,
        camera: Some(ViewCamera { eye: [0.0; 3], forward: view.forward, perspective: false, fov_y: None }),
        clip: None,
        plan: None,
        level: None,
        folder: Vec::new(),
        style: None,
        drawing: None,
    }
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

    #[test]
    fn vectorworks_files_reach_the_vwx_reader() {
        let source = Source::Bytes { name: "house.VWX".into(), data: Arc::new(Vec::new()) };
        let error = load_with(&source, &Progress::default()).unwrap_err();
        assert!(error.starts_with("Cannot read Vectorworks file:"), "{error}");
    }

    /// A file made with Vectorworks 2027 by the reader's own sample script (`51_symbol_two` of the
    /// vwx-dll oracle ladder): the symbol `Blok` (a box of 1000 x 500 x 1000 mm) placed at the
    /// origin and, turned 45 degrees, at (3000, 1000). Sample files are kept out of the
    /// repository; the test runs where `VWX_SAMPLES` names their folder.
    #[test]
    fn reads_a_vectorworks_file_with_two_placed_symbols() {
        let Some(samples) = std::env::var_os("VWX_SAMPLES") else { return };
        let path = std::path::PathBuf::from(samples).join("ladder/2027/51_symbol_two.vwx");
        let model = super::super::load(&path).expect("Vectorworks sample");
        assert_eq!(model.format, "Vectorworks");
        assert_eq!(model.triangle_count(), 24);
        let size = model.bounds.size();
        assert!((size[2] - 1.0).abs() < 1e-4, "{size:?}");
        assert!(model.props.values().any(|p| p.contains(r#""Symbol":"Blok""#)), "{:?}", model.props.values().next());
        let layer = &model.layers[0];
        assert!(layer.name.starts_with("Ontwerplaag-1"), "{}", layer.name);
        assert_eq!(layer.elements.len(), 2);
    }

    /// `140_saved_view` of the ladder: a view saved as "Aanzicht A" after the model was turned
    /// 60 degrees about x and 45 about z.
    #[test]
    fn reads_the_saved_views_of_a_vectorworks_file() {
        let Some(samples) = std::env::var_os("VWX_SAMPLES") else { return };
        let path = std::path::PathBuf::from(samples).join("ladder/2027/140_saved_view.vwx");
        let model = super::super::load(&path).expect("Vectorworks sample");
        let view = model.views.iter().find(|v| v.name == "Aanzicht A").expect("the saved view");
        let forward = view.camera.expect("its camera").forward;
        // down at 30 degrees
        assert!((forward[2] + 0.5).abs() < 1e-5 && (forward[0] + forward[1]).abs() < 1e-5, "{forward:?}");
    }

    /// `02_line` of the ladder: a line is line work, pickable.
    #[test]
    fn reads_the_line_work_of_a_vectorworks_file() {
        let Some(samples) = std::env::var_os("VWX_SAMPLES") else { return };
        let path = std::path::PathBuf::from(samples).join("ladder/2027/02_line.vwx");
        let model = super::super::load(&path).expect("Vectorworks sample");
        let layer = model.layers.iter().find(|l| !l.lines.is_empty()).expect("a layer with a line");
        assert_eq!((layer.lines.len(), layer.line_elements.len()), (2, 1));
    }

    /// `90_record` of the ladder: the line with the record `Gegevens`, field `Naam`.
    #[test]
    fn reads_the_records_of_a_vectorworks_file() {
        let Some(samples) = std::env::var_os("VWX_SAMPLES") else { return };
        let path = std::path::PathBuf::from(samples).join("ladder/2027/90_record.vwx");
        let model = super::super::load(&path).expect("Vectorworks sample");
        assert!(model.props.values().any(|p| p.contains(r#""Gegevens.Naam":"Balk-1""#)), "{:?}", model.props.values().collect::<Vec<_>>());
    }

    #[test]
    fn saved_views_look_the_way_the_file_says() {
        let saved = super::view(&vwx_read::model::View { name: "Aanzicht A".into(), forward: [0.0, 0.6, -0.8], up: [0.0, 0.8, 0.6] });
        assert_eq!((saved.name.as_str(), saved.kind), ("Aanzicht A", super::ViewKind::ThreeD));
        let camera = saved.camera.unwrap();
        assert_eq!((camera.forward, camera.perspective), ([0.0, 0.6, -0.8], false));
    }

    #[test]
    fn templates_are_vectorworks_files_too() {
        let source = Source::Bytes { name: "plan.sta".into(), data: Arc::new(Vec::new()) };
        let error = load_with(&source, &Progress::default()).unwrap_err();
        assert!(error.starts_with("Cannot read Vectorworks file:"), "{error}");
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
