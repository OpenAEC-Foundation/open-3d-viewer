//! The drawings a loader leaves for after the model shows (see [`crate::model::Later`]): on a
//! large Revit project the annotated views, legends and sheets take far longer to read than the
//! model itself. They are read on a worker thread while the model is on screen, and put into the
//! model when they are in: the views opened meanwhile get their annotations then.

use super::{App, Message};
use crate::model::{Drawings, Model};
use iced::Task;
use std::sync::Arc;

/// Runs the work on a worker thread.
#[cfg(not(target_arch = "wasm32"))]
fn later_task(id: u64, work: Box<dyn FnOnce() -> Drawings + Send>) -> Task<Message> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(work());
            });
            rx.await.unwrap_or_default()
        },
        move |drawings| Message::Drawings(id, Box::new(drawings)),
    )
}

/// The browser has no threads: the drawings are read in the task.
#[cfg(target_arch = "wasm32")]
fn later_task(id: u64, work: Box<dyn FnOnce() -> Drawings + Send>) -> Task<Message> {
    Task::perform(async move { work() }, move |drawings| Message::Drawings(id, Box::new(drawings)))
}

impl App {
    /// Starts reading the drawings the loader of `model` left for after the model shows.
    pub(super) fn read_drawings(&mut self, model: &Model) -> Task<Message> {
        let Some(work) = model.project.later.take() else { return Task::none() };
        self.drawings_pending.insert(model.id);
        later_task(model.id, work)
    }

    /// True while the drawings of a model are being read.
    pub(super) fn reads_drawings(&self, model: &Model) -> bool {
        self.drawings_pending.contains(&model.id)
    }

    /// Puts the drawings that were read into their model (by its id) and shows it again as it
    /// was shown: the camera, the selection and the open tabs stay.
    pub(super) fn take_drawings(&mut self, id: u64, drawings: Drawings) -> Task<Message> {
        if !self.drawings_pending.remove(&id) {
            return Task::none();
        }
        let with = |model: &Model| {
            let mut model = model.clone();
            model.project.take_over(drawings.clone());
            Arc::new(model)
        };
        let scene = match self.models.iter().position(|entry| entry.model.id == id) {
            Some(k) => {
                self.models[k].model = with(&self.models[k].model);
                let sources: Vec<Arc<Model>> = self.models.iter().map(|entry| entry.model.clone()).collect();
                let moves: Vec<glam::DVec3> = self.models.iter().map(|entry| entry.placed.moved).collect();
                match sources.as_slice() {
                    [one] if moves[0] == glam::DVec3::ZERO => one.clone(),
                    _ => {
                        // the same scene as before: it keeps its id, so nothing is uploaded again
                        let mut scene = Model::combine_moved(&sources, &moves);
                        scene.id = self.model.as_ref().map_or(scene.id, |shown| shown.id);
                        Arc::new(scene)
                    }
                }
            }
            // a model shown without an entry of its own
            None => match self.model.as_ref().filter(|m| m.id == id) {
                Some(shown) => with(shown),
                None => return Task::none(),
            },
        };
        // the drawings the document draws, and the cuts made without the joins and phases
        self.document.recognise(&scene);
        self.cuts.clear();
        self.model = Some(scene);
        // a sheet opened meanwhile showed the frames of its views alone: drawn again, with them
        match self.active_sheet {
            Some(i) => self.open_sheet(i),
            None => Task::none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Later, Layer, Sheet};

    fn model(name: &str) -> Model {
        let layer = Layer { name: "Walls".into(), positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0], indices: vec![0, 1, 2], elements: vec![(1, 0, 3)], ..Default::default() };
        let mut model = Model::new(std::path::PathBuf::from(name), "Revit", vec![layer]);
        model.project.later = Later::new(|| Drawings { sheets: vec![Sheet::new("A1".into(), "Plan".into(), None, Vec::new())], ..Default::default() });
        model
    }

    /// The model shows without its sheets; they come in when they are read, and the model on
    /// screen stays the one it was (its id: nothing is uploaded again).
    #[test]
    fn drawings_come_in_after_the_model_shows() {
        let mut app = App::default();
        let first = Arc::new(model("a.rvt"));
        app.set_model(first.clone());
        // the task that reads them is not run here: its work is done by hand
        let _ = app.read_drawings(&first);
        assert!(app.reads_drawings(&first));
        assert!(app.sheets().is_empty());
        let read = model("a.rvt").project.later.take().expect("work left for later")();
        let _ = app.take_drawings(first.id, read);
        assert!(!app.reads_drawings(&first));
        assert_eq!(app.sheets().len(), 1);
        assert_eq!(app.model.as_ref().map(|m| m.id), Some(first.id));
    }

    /// With a second model beside it, the sheets land in the scene of both.
    #[test]
    fn drawings_of_one_of_two_models_come_into_the_scene() {
        let mut app = App::default();
        let (a, b) = (Arc::new(model("a.rvt")), Arc::new(model("b.rvt")));
        let work = b.project.later.take().expect("work left for later");
        app.set_model(a);
        app.add_model(b.clone());
        let shown = app.model.as_ref().map(|m| m.id);
        app.drawings_pending.insert(b.id);
        let _ = app.take_drawings(b.id, work());
        assert_eq!(app.sheets().len(), 1);
        assert_eq!(app.model.as_ref().map(|m| m.id), shown);
    }

    /// A file read at once (without a window) has its drawings.
    #[test]
    fn a_model_completed_has_its_drawings() {
        let mut model = model("a.rvt");
        model.complete();
        assert_eq!(model.project.sheets.len(), 1);
    }
}
