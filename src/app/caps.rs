//! The cut faces of the section box and the section plane in the 3D view, in the hatch of what
//! each element is made of (`crate::caps`): made again when the section, the plane or what is
//! shown changes. In the window they are made on a worker thread: the section moves at once
//! while it is dragged, and its cut faces follow when they are made (on a large model that takes
//! seconds, which held every move of the section up).

use super::{App, Message};
use iced::Task;
use crate::caps::{cut_faces_kept, CutFaces, KeptFaces};
use crate::drawing::Shown;
use crate::section::{SectionBox, SectionPlane};
use glam::Mat4;
use std::sync::Arc;

/// A model with more triangles than this keeps the plain cut fill: cutting it again at every
/// move of the section would hold the view up.
const MOST_TRIANGLES: usize = 3_000_000;

/// What the cut faces were made from: the section box, the plane, the layers shown, and which
/// model and hidden elements (by their addresses).
/// What the faces of the section box cut before (see [`crate::caps::cut_faces_kept`]), with the
/// model, the layers shown and the hidden elements they hold for.
pub(super) type KeptCutFaces = Arc<std::sync::Mutex<(Option<(usize, Vec<bool>, usize)>, KeptFaces)>>;

pub(super) type CutFacesKey = (Option<SectionBox>, Option<SectionPlane>, Vec<bool>, usize, usize);

impl App {
    /// What the cut faces are made from now; `None` where none are drawn: without a section, with
    /// the cut fill off, in a view's own tab (it draws its cut itself), or on a model turned up.
    fn cut_faces_key(&self) -> Option<CutFacesKey> {
        let model = self.model.as_ref()?;
        let (section, plane) = (self.active_section(), self.plane);
        let drawn = self.section_fill && (section.is_some() || plane.is_some()) && self.shown_view().is_none() && self.model_matrix() == Mat4::IDENTITY;
        (drawn && model.triangle_count() <= MOST_TRIANGLES).then(|| (section, plane, self.visible.clone(), Arc::as_ptr(model) as usize, Arc::as_ptr(&self.hidden) as usize))
    }

    /// After any change: the cut faces made again when what they are made from changed. In the
    /// window on a worker thread, one at a time: while it works the section may move on, and the
    /// faces of where it is then are made next (those of where it was are not kept).
    pub(super) fn keep_cut_faces(&mut self) -> Task<Message> {
        let key = self.cut_faces_key();
        if key == self.cut_faces_key {
            return Task::none();
        }
        let Some(job) = self.cut_faces_job(&key) else {
            self.cut_faces_key = key;
            if !self.cut_faces.is_empty() {
                self.cut_faces = Arc::new(CutFaces::default());
            }
            return Task::none();
        };
        if !self.background_cuts {
            let _ = self.take_cut_faces(key, Arc::new(job()));
            return Task::none();
        }
        // the faces of where the section was are wrong where it is now: the plain cut fill
        // shows until the new ones are in
        if !self.cut_faces.is_empty() {
            self.cut_faces = Arc::new(CutFaces::default());
        }
        if self.cut_faces_running {
            return Task::none();
        }
        self.cut_faces_running = true;
        Task::perform(
            async move {
                let (tx, rx) = iced::futures::channel::oneshot::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(job());
                });
                rx.await.unwrap_or_default()
            },
            move |faces| Message::CutFaces(Box::new(key.clone()), Arc::new(faces)),
        )
    }

    /// The work of making the cut faces of `key`, to run on any thread; `None` where none are drawn.
    fn cut_faces_job(&self, key: &Option<CutFacesKey>) -> Option<impl FnOnce() -> CutFaces + Send + 'static> {
        let (section, plane, layers, ..) = key.clone()?;
        let model = self.model.clone()?;
        // the hidden elements, by the unit of a triangle's first vertex
        let units = self.hidden_units(&self.hidden);
        let index = self.explode_index.clone().filter(|_| units.iter().any(|u| *u));
        // what the faces of the box cut before, while the model and what is shown of it stay
        let of = (Arc::as_ptr(&model) as usize, layers.clone(), Arc::as_ptr(&self.hidden) as usize);
        let kept = self.kept_faces.clone();
        Some(move || {
            let mut kept = kept.lock().unwrap_or_else(|e| e.into_inner());
            if kept.0 != Some(of.clone()) {
                *kept = (Some(of), KeptFaces::new());
            }
            let hidden = |l: usize, v: u32| index.as_ref().is_some_and(|ix| units.get(ix.unit(l, v)).copied().unwrap_or(false));
            cut_faces_kept(&model, Shown { layers: &layers, hidden: &hidden, faint: None, pieces: None }, section.as_ref(), plane.as_ref(), bim_library::DrawingStandard::Nen47, &mut kept.1)
        })
    }

    /// Cut faces that were made: shown when they are still those of what the view shows, else
    /// those of what it shows now are made.
    pub(super) fn take_cut_faces(&mut self, key: Option<CutFacesKey>, faces: Arc<CutFaces>) -> Task<Message> {
        self.cut_faces_running = false;
        if key == self.cut_faces_key() {
            self.cut_faces = faces;
            self.cut_faces_key = key;
            return Task::none();
        }
        self.keep_cut_faces()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Layer, Model};
    use glam::Vec3;
    use std::path::PathBuf;

    /// A closed box from (0, 0, 0) to (4, 2, 3).
    fn solid() -> Layer {
        let positions = vec![0., 0., 0., 4., 0., 0., 4., 2., 0., 0., 2., 0., 0., 0., 3., 4., 0., 3., 4., 2., 3., 0., 2., 3.];
        let indices = vec![0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7, 6, 3, 0, 4, 3, 4, 7];
        Layer { name: "Beton".into(), color: [0.5; 3], normals: vec![0.0; positions.len()], positions, colors: None, indices, elements: Vec::new(), props: Vec::new(), hidden: false, points: false }
    }

    #[test]
    fn the_section_plane_gives_cut_faces_until_the_fill_is_off() {
        let mut app = App::default();
        app.set_model(Arc::new(Model::new(PathBuf::from("box.obj"), "OBJ", vec![solid()])));
        let _ = app.keep_cut_faces();
        assert!(app.cut_faces.is_empty(), "nothing is cut yet");
        app.plane = Some(SectionPlane { normal: Vec3::Z, offset: 1.5 });
        let _ = app.keep_cut_faces();
        assert!(!app.cut_faces.triangles.is_empty(), "the plane cuts the solid");
        let before = Arc::as_ptr(&app.cut_faces);
        let _ = app.keep_cut_faces();
        assert_eq!(before, Arc::as_ptr(&app.cut_faces), "not made again while nothing changed");
        app.section_fill = false;
        let _ = app.keep_cut_faces();
        assert!(app.cut_faces.is_empty());
    }
}
