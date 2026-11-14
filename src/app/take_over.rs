//! Taking over a wall of an opened file (Revit) as an editable wall, in the Modeller.
//!
//! A wall the file defines well enough ([`SourceWall`]: straight, with a thickness, a height, and
//! doors and windows whose extent can be told) has grips when it is selected in the Modeller, as a
//! modelled wall has. A press on one takes the wall over:
//! - an editable wall with the file's axis, base, height, thickness and openings is modelled in
//!   its place, on the level of the same name;
//! - the file's mesh of it leaves the scene;
//! - the drag goes on with the new wall.
//!
//! The rest of the file stays as it is: meshes, taken over one wall at a time as they are touched.
//! In the Viewer, a wall of a file has no grips.
//!
//! The first wall taken over makes the project an IFCX document, named after the file with the
//! extension `.ifcx`, in the file's folder. The title shows that name, and Save as writes it
//! there: the file's meshes, without the walls taken over, and the modelled elements with their
//! definitions.
//!
//! The file's location line is the axis, with the wall centred on it, as the mesher draws a wall.
//! Where the wall's own mesh is as thick as its type but lies beside that line (by more than a
//! millimetre), the axis moves to the middle of the mesh (to a tenth of a millimetre, as the mesh
//! is in single precision), so the editable wall stands where the wall was drawn.

use super::{App, ModelEntry};
use crate::model::Model;
use crate::selection::Pickable;
use bim_core::element::PlanarForm;
use bim_core::{Element, ElementId, PlanarDef, Shape};
use glam::DVec2;
use std::sync::Arc;

/// A wall's mesh moves its axis only when it is this close to its type's thickness (metres).
const THICKNESS_TOLERANCE: f64 = 0.005;
/// The axis moves to the middle of the mesh when that lies farther from it than this (metres).
const OFF_AXIS: f64 = 0.001;

/// How far element `id` of `model` reaches across the line from `start` to `end`, in plan: the
/// least and the greatest distance of its mesh's corners from the line, positive to its left.
/// `None` when the element has no mesh.
pub(crate) fn across_mesh(model: &Model, id: u64, start: DVec2, end: DVec2) -> Option<(f64, f64)> {
    let left = (end - start).normalize_or_zero().perp();
    let mut range: Option<(f64, f64)> = None;
    for (layer, first, count) in model.element_ranges(id) {
        let layer = &model.layers[layer];
        for &i in &layer.indices[first as usize..(first + count) as usize] {
            let p = &layer.positions[3 * i as usize..3 * i as usize + 2];
            let d = (DVec2::new(p[0] as f64, p[1] as f64) - start).dot(left);
            range = Some(range.map_or((d, d), |(lo, hi)| (lo.min(d), hi.max(d))));
        }
    }
    range
}

/// A selected wall of an opened file, as it would be taken over.
pub(super) struct Source {
    /// Its entry in `App::models`; `None` while the scene is that file alone.
    entry: Option<usize>,
    /// Its id in that file.
    id: u64,
    /// The editable wall it becomes (scene coordinates).
    pub wall: PlanarDef,
    /// The name of its base level.
    level: Option<String>,
    /// What the new wall is named after: its type, as the file names it, else "Wall".
    name: String,
}

impl App {
    /// The selected wall of an opened file that a grip would take over: in the Modeller, selected
    /// alone, outside edit mode and with no tool on.
    pub(super) fn source_gripped(&self) -> Option<Source> {
        if !self.modeller.in_modeller || self.modeller.edit.is_some() || self.modeller.tool.is_some() || self.measure.tool.is_some() || self.plane_picking {
            return None;
        }
        let [Pickable::Element(shown)] = self.sel.items()[..] else { return None };
        let scene = self.model.as_ref()?;
        let (entry, id, file) = if self.models.is_empty() {
            (None, shown, scene.clone())
        } else {
            let key = scene.element_key(shown);
            let entry = self.models.get(key.model as usize)?;
            if Some(entry.model.id) == self.modeller.shown {
                return None;
            }
            (Some(key.model as usize), key.source, entry.model.clone())
        };
        let w = file.project.walls.get(&id)?;
        // the axis moves onto the middle of the wall's mesh when that lies beside it
        let (mut start, mut end) = (w.start, w.end);
        if let Some((lo, hi)) = across_mesh(scene, shown, w.start, w.end) {
            // the mesh is f32: its middle to a tenth of a millimetre
            let middle = ((lo + hi) / 2.0 / 1e-4).round() * 1e-4;
            if ((hi - lo) - w.thickness).abs() <= THICKNESS_TOLERANCE && middle.abs() > OFF_AXIS {
                let left = (w.end - w.start).normalize().perp();
                start += left * middle;
                end += left * middle;
            }
        }
        let wall = PlanarDef { form: PlanarForm::Wall { start, end, base: w.base, height: w.height }, thickness: w.thickness, openings: w.openings.clone(), build_up: None };
        let name = scene.element_props(shown).into_iter().find(|(k, _)| k == "type").map(|(_, v)| v).filter(|v| !v.is_empty()).unwrap_or_else(|| "Wall".into());
        Some(Source { entry, id, wall, level: w.level.clone(), name })
    }

    /// Shows every opened file a wall was taken over from without the walls `taken` names (the
    /// file, the wall), and keeps `taken` as what is taken over now. A file whose walls are all
    /// back is shown as it was opened. The scene is put together again by the next
    /// [`App::show_modelled`].
    pub(super) fn show_files_without(&mut self, taken: Arc<Vec<(u64, u64)>>) {
        if *taken != *self.modeller.taken {
            let files: Vec<(u64, Arc<Model>, u64)> = self.modeller.files.iter().map(|(id, (original, shown))| (*id, original.clone(), *shown)).collect();
            for (id, original, shown) in files {
                let gone = |list: &[(u64, u64)]| list.iter().filter(|t| t.0 == id).map(|t| t.1).collect::<std::collections::HashSet<u64>>();
                let ids = gone(&taken);
                let Some(k) = self.models.iter().position(|e| e.model.id == shown).filter(|_| ids != gone(&self.modeller.taken)) else { continue };
                let model = if ids.is_empty() { original.clone() } else { Arc::new(original.without_elements(&ids)) };
                self.modeller.files.insert(id, (original, model.id));
                let file = std::mem::replace(&mut self.models[k], ModelEntry::new(model, 0));
                // its layers keep their switches by name (a layer that emptied is gone), and it
                // stays where it was placed
                let switched: std::collections::HashMap<&str, bool> = file.model.layers.iter().map(|l| l.name.as_str()).zip(file.layer_visibility.iter().copied()).collect();
                let entry = &mut self.models[k];
                (entry.visible, entry.placed) = (file.visible, file.placed);
                for (layer, shown) in entry.model.layers.iter().zip(entry.layer_visibility.iter_mut()) {
                    if let Some(&s) = switched.get(layer.name.as_str()) {
                        *shown = s;
                    }
                }
            }
        }
        self.modeller.taken = taken;
    }

    /// Takes wall `source` over: models it, takes the file's mesh of it out of the scene and
    /// selects the new wall, which it returns. When the kernel cannot make the wall, nothing
    /// changes and the banner says why.
    pub(super) fn take_over(&mut self, source: Source) -> Option<ElementId> {
        let grid = self.modeller.model.grid;
        let shape = Shape::Planar(source.wall);
        // the kernel must make the wall before anything changes
        if let Err(refusal) = bim_core::Model::new(grid).add(Element::in_project(&grid, "", None, shape.clone())) {
            self.modeller.refusal = Some(refusal);
            return None;
        }
        if self.models.is_empty() {
            self.models.push(ModelEntry::new(self.model.clone()?, 0));
        }
        let k = source.entry.unwrap_or(0);
        // the file as it was opened: kept, so the wall can come back when the step is taken back
        let shown = self.models[k].model.clone();
        let original = self.modeller.files.iter().find(|(_, (_, now))| *now == shown.id).map_or(shown.id, |(id, _)| *id);
        self.modeller.files.entry(original).or_insert((shown.clone(), shown.id));
        let before = self.modeller.taken.clone();
        let mut taken = (*before).clone();
        taken.push((original, source.id));
        self.show_files_without(Arc::new(taken));
        let first = self.modeller.project_file.is_none();
        if first {
            self.modeller.project_file = Some(shown.path.with_extension("ifcx"));
        }
        let level = source.level.and_then(|name| self.document.levels().iter().find(|l| l.name == name).map(|l| l.path.clone()));
        match self.add_element(&source.name, level, shape) {
            Ok(id) => {
                self.select_modelled(id);
                Some(id)
            }
            Err(refusal) => {
                // not expected after the check above; put the file back as it was
                self.show_files_without(before);
                if first {
                    self.modeller.project_file = None;
                }
                self.modeller.refusal = Some(refusal);
                self.show_modelled();
                None
            }
        }
    }
}

#[cfg(test)]
/// The triangles of a box from `lo` to `hi`, as a layer of element `id`.
fn boxed(id: u64, lo: [f32; 3], hi: [f32; 3]) -> crate::model::Layer {
    let c = |i: usize| [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }];
    let positions: Vec<f32> = (0..8).flat_map(c).collect();
    let indices = vec![0, 1, 3, 0, 3, 2, 4, 6, 7, 4, 7, 5, 0, 4, 5, 0, 5, 1, 2, 3, 7, 2, 7, 6, 0, 2, 6, 0, 6, 4, 1, 5, 7, 1, 7, 3];
    crate::model::Layer { name: "Walls".into(), positions, indices, elements: vec![(id, 0, 36)], props: vec![(id, r#"{"type":"Basic 300"}"#.into())], ..crate::model::Layer::default() }
}

#[cfg(test)]
/// A file with two walls: 7, from (0, 0) to (6, 0) with a door, drawn with its location line
/// on its outer face (its mesh 0.3 thick to the left of the line), which the Modeller can take
/// over; and 8, along y, which it cannot (no definition, as a curved wall has none).
pub(crate) fn house() -> Arc<Model> {
    let mut model = Model::new(std::path::PathBuf::from("C:/work/house.rvt"), "Revit", vec![boxed(7, [0.0, 0.0, 0.0], [6.0, 0.3, 3.0]), boxed(8, [0.0, 0.3, 0.0], [0.3, 4.0, 3.0])]);
    model.project.levels = vec![("Level 0".into(), 0.0)];
    let door = bim_core::Opening { along: 1.0, sill: 0.0, width: 0.9, height: 2.1 };
    let wall = crate::model::SourceWall { start: DVec2::ZERO, end: DVec2::new(6.0, 0.0), base: 0.0, height: 3.0, thickness: 0.3, openings: vec![door], level: Some("Level 0".into()) };
    model.project.walls.insert(7, wall);
    Arc::new(model)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::edit::{edit_points, EditEvent};
    use crate::app::Message;
    use crate::model::ElementKey;
    use glam::{DVec3, Vec3};
    use iced::Color;
    use std::path::PathBuf;


    /// The app with the house open, seen from the top (10 px per metre, the origin in the middle
    /// of 1000 × 800), in the Modeller.
    fn opened() -> App {
        let mut app = App::default();
        app.set_model(house());
        app.viewport_size = (1000.0, 800.0);
        app.camera.set_preset(crate::viewport::camera::ViewPreset::Top);
        app.camera.ortho = true;
        app.camera.target = Vec3::ZERO;
        app.camera.distance = 40.0 / (app.camera.fov_y * 0.5).tan();
        app.modeller.in_modeller = true;
        app
    }

    fn select(app: &mut App, id: u64) {
        app.click(Some(Pickable::Element(id)), false);
    }

    /// Drags grip `index` of the selected wall to scene point (x, y) on the floor.
    fn drag(app: &mut App, index: usize, x: f64, y: f64) {
        let _ = app.update(Message::Edit(EditEvent::Press { index, add: false }));
        let (sx, sy) = app.on_screen(DVec3::new(x, y, 0.0), 1000.0, 800.0).unwrap();
        let _ = app.update(Message::Edit(EditEvent::Drag { x: sx, y: sy, width: 1000.0, height: 800.0 }));
        let _ = app.update(Message::Edit(EditEvent::Release));
    }

    #[test]
    fn a_wall_of_the_file_has_grips_in_the_modeller_only_on_the_middle_of_its_mesh() {
        let mut app = opened();
        app.modeller.in_modeller = false;
        select(&mut app, 7);
        assert!(app.grip_overlay(Color::BLACK, Color::WHITE).is_none(), "not in the Viewer");
        app.modeller.in_modeller = true;
        let grips = app.grip_overlay(Color::BLACK, Color::WHITE).expect("grips");
        // the location line is the outer face: the axis lies on the mesh's middle, 0.15 m in
        assert_eq!(grips.points, vec![Vec3::new(0.0, 0.15, 0.0), Vec3::new(6.0, 0.15, 0.0), Vec3::new(3.0, 0.15, 0.0)]);
        select(&mut app, 8);
        assert!(app.grip_overlay(Color::BLACK, Color::WHITE).is_none(), "a wall without a definition stays a mesh");
    }

    #[test]
    fn dragging_a_wall_of_the_file_takes_it_over_and_the_project_becomes_ifcx() {
        let mut app = opened();
        select(&mut app, 7);
        drag(&mut app, 1, 8.0, 0.15);
        // one modelled wall, as the file defines it, its end where it was dragged
        let walls: Vec<&Element> = app.modeller.model.iter().collect();
        assert_eq!(walls.len(), 1);
        let shape = walls[0].shape_in_project(&app.modeller.model.grid);
        let Shape::Planar(p) = &shape else { panic!() };
        assert_eq!((p.thickness, p.openings.len()), (0.3, 1));
        assert_eq!(edit_points(&shape).0, vec![DVec3::new(0.0, 0.15, 0.0), DVec3::new(8.0, 0.15, 0.0)]);
        assert_eq!(walls[0].name, "Basic 300 1");
        let level = app.document.levels().iter().find(|l| l.name == "Level 0").map(|l| l.path.clone());
        assert_eq!(walls[0].level, level);
        assert_eq!(app.selected_modelled(), vec![walls[0].id], "the new wall selected, its grips shown");
        assert!(app.modeller.edit.is_none());
        // the file's mesh of it is gone; the other wall is still the file's
        let scene = app.model.clone().unwrap();
        assert!(scene.element_with_key(ElementKey { model: 0, source: 7 }).is_none());
        assert!(scene.element_with_key(ElementKey { model: 0, source: 8 }).is_some());
        // an IFCX document named after the file, in its folder
        assert_eq!(scene.path, PathBuf::from("C:/work/house.ifcx"));
        assert!(app.title().starts_with("house.ifcx"));
        let bytes = crate::export::to_bytes(&scene, &app.document_now(), "house.ifcx").unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.contains("openaec::element"), "the wall's definition is saved");
        // opened again: the wall taken over is editable, the file's other wall a mesh, each once
        let source = crate::loaders::Source::Bytes { name: "house.ifcx".into(), data: Arc::new(bytes) };
        let loaded = crate::loaders::load_with(&source, &crate::loaders::Progress::default()).expect("read back");
        let mut back = App::default();
        back.set_model(Arc::new(loaded));
        let read: Vec<Element> = back.modeller.model.iter().cloned().collect();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].shape_in_project(&back.modeller.model.grid), shape);
        let shown = back.model.as_ref().unwrap();
        assert_eq!(shown.layers.iter().map(|l| l.elements.len()).sum::<usize>(), 2, "the wall taken over and the file's other wall");
    }

    /// A wall of a sample taken over, and how many of its walls the kernel makes. Set `RVT_SAMPLE`
    /// to a Revit 2024+ file; skipped when absent.
    #[test]
    fn a_wall_of_a_sample_is_taken_over() {
        let Some(path) = std::env::var_os("RVT_SAMPLE").map(PathBuf::from).filter(|p| p.exists()) else {
            eprintln!("skipped: set RVT_SAMPLE to a Revit 2024+ file");
            return;
        };
        let model = Arc::new(crate::loaders::load(&path).expect("rvt"));
        // how many of the file's drawn walls the kernel makes, and why not the others
        let grid = bim_core::TileGrid::default();
        let mut refusals = std::collections::BTreeMap::<String, usize>::new();
        let drawn: Vec<u64> = model.project.walls.keys().copied().filter(|id| !model.element_ranges(*id).is_empty()).collect();
        for id in &drawn {
            let w = &model.project.walls[id];
            let wall = PlanarDef { form: PlanarForm::Wall { start: w.start, end: w.end, base: w.base, height: w.height }, thickness: w.thickness, openings: w.openings.clone(), build_up: None };
            if let Err(r) = bim_core::Model::new(grid).add(Element::in_project(&grid, "", None, Shape::Planar(wall))) {
                *refusals.entry(format!("{r:?}")).or_default() += 1;
            }
        }
        eprintln!("{} drawn walls, the kernel makes {}; refused: {refusals:?}", drawn.len(), drawn.len() - refusals.values().sum::<usize>());
        // the drawn walls with doors or windows: how many are made, and why not the others
        let mut with_openings = (0, 0);
        for id in drawn.iter().filter(|id| !model.project.walls[id].openings.is_empty()) {
            let w = &model.project.walls[id];
            let wall = PlanarDef { form: PlanarForm::Wall { start: w.start, end: w.end, base: w.base, height: w.height }, thickness: w.thickness, openings: w.openings.clone(), build_up: None };
            with_openings.0 += 1;
            match bim_core::Model::new(grid).add(Element::in_project(&grid, "", None, Shape::Planar(wall))) {
                Ok(_) => with_openings.1 += 1,
                Err(r) => eprintln!("drawn wall {id} refused, {r:?}: {:.3} m long, {:.3} high, openings {:?}", w.start.distance(w.end), w.height, w.openings),
            }
        }
        eprintln!("{} drawn walls with doors or windows, the kernel makes {}", with_openings.0, with_openings.1);
        // a drawn wall the kernel makes, one with a door or window when there is one, taken over
        let mut app = App::default();
        app.set_model(model.clone());
        app.modeller.in_modeller = true;
        let makes = |id: &u64| {
            let w = &model.project.walls[id];
            let wall = PlanarDef { form: PlanarForm::Wall { start: w.start, end: w.end, base: w.base, height: w.height }, thickness: w.thickness, openings: w.openings.clone(), build_up: None };
            bim_core::Model::new(grid).add(Element::in_project(&grid, "", None, Shape::Planar(wall))).is_ok()
        };
        let with_openings = drawn.iter().find(|id| !model.project.walls[id].openings.is_empty() && makes(id));
        let Some(&id) = with_openings.or_else(|| drawn.iter().find(|id| makes(id))) else {
            eprintln!("no drawn wall the kernel makes");
            return;
        };
        select(&mut app, id);
        let _ = app.update(Message::Edit(EditEvent::Press { index: 2, add: false }));
        app.modeller.edit.as_mut().unwrap().grab.as_mut().unwrap().delta = DVec3::new(0.5, 0.0, 0.0);
        let _ = app.update(Message::Edit(EditEvent::Release));
        let taken: Vec<&Element> = app.modeller.model.iter().collect();
        assert_eq!(taken.len(), 1, "{:?}", app.modeller.refusal);
        let Shape::Planar(p) = &taken[0].shape else { panic!() };
        assert_eq!(p.openings.len(), model.project.walls[&id].openings.len());
        let scene = app.model.clone().unwrap();
        assert!(scene.element_with_key(ElementKey { model: 0, source: id }).is_none());
        assert_eq!(scene.path.extension().and_then(|e| e.to_str()), Some("ifcx"));
        let t = std::time::Instant::now();
        let bytes = crate::export::to_bytes(&scene, &app.document_now(), "taken.ifcx").unwrap();
        eprintln!("took over wall {id} ({}, {} openings); IFCX of {} MB written in {:?}", taken[0].name, p.openings.len(), bytes.len() / 1_000_000, t.elapsed());
    }

    #[test]
    fn undo_gives_the_files_wall_back_and_redo_takes_it_over_again() {
        let mut app = opened();
        select(&mut app, 7);
        drag(&mut app, 1, 8.0, 0.15);
        let files = |app: &App, id: u64| app.model.as_ref().unwrap().element_with_key(ElementKey { model: 0, source: id }).is_some();
        assert!(!files(&app, 7) && app.modeller.model.len() == 1);
        // taken back: no modelled wall, the wall of the file where it was, the project the file again
        let _ = app.update(Message::Undo);
        assert!(app.modeller.model.is_empty());
        assert!(files(&app, 7) && files(&app, 8), "the wall is the file's again");
        assert!(app.modeller.project_file.is_none() && app.modeller.taken.is_empty());
        let shown = app.model.as_ref().unwrap().element_with_key(ElementKey { model: 0, source: 7 }).unwrap();
        select(&mut app, shown);
        assert!(app.source_gripped().is_some(), "and can be taken over again");
        // made again: the editable wall with its dragged end, the file's mesh of it gone
        let _ = app.update(Message::Redo);
        assert!(!files(&app, 7) && files(&app, 8));
        let wall = app.modeller.model.iter().next().unwrap().shape_in_project(&app.modeller.model.grid);
        assert_eq!(edit_points(&wall).0[1], DVec3::new(8.0, 0.15, 0.0));
        assert_eq!(app.modeller.project_file, Some(PathBuf::from("C:/work/house.ifcx")));
        // a second step on the wall is taken back alone: the wall stays the project's
        let id = app.modeller.model.iter().next().unwrap().id;
        app.select_modelled(id);
        drag(&mut app, 1, 9.0, 0.15);
        let _ = app.update(Message::Undo);
        assert!(!files(&app, 7) && app.modeller.model.len() == 1);
        let wall = app.modeller.model.iter().next().unwrap().shape_in_project(&app.modeller.model.grid);
        assert_eq!(edit_points(&wall).0[1], DVec3::new(8.0, 0.15, 0.0));
    }

    #[test]
    fn a_wall_the_kernel_cannot_make_stays_the_files() {
        let mut model = Arc::try_unwrap(house()).unwrap();
        // two openings that overlap
        model.project.walls.get_mut(&7).unwrap().openings.push(bim_core::Opening { along: 1.5, sill: 1.0, width: 1.0, height: 1.0 });
        let mut app = opened();
        app.set_model(Arc::new(model));
        app.modeller.in_modeller = true;
        select(&mut app, 7);
        drag(&mut app, 1, 8.0, 0.15);
        assert_eq!(app.modeller.refusal, Some(bim_core::Refusal::OpeningsOverlap));
        assert!(app.modeller.model.is_empty());
        let scene = app.model.clone().unwrap();
        assert!(scene.element_with_key(ElementKey { model: 0, source: 7 }).is_some());
        assert!(app.modeller.project_file.is_none());
    }
}
