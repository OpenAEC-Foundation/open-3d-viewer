//! Undo and redo across what the app changes: what is shown (hide, show, layers: kept per view)
//! and the modelled elements (every step that changes the model: placing, moving, joining,
//! deleting, typing a size). Ctrl+Z takes back the last of either, in the order they were made.

use super::App;
use std::sync::Arc;

impl App {
    /// A move being made in edit mode or with a grip: the model changes at every move of the
    /// cursor, and is one step once it is put down.
    fn moving(&self) -> bool {
        self.modeller.edit.as_ref().is_some_and(|e| e.grab.is_some())
    }

    /// After any message: when the modelled elements changed, the model as it was goes into the
    /// history, as one step to take back.
    pub(super) fn keep_model_history(&mut self) {
        if !self.modeller.touched || self.moving() {
            return;
        }
        self.modeller.touched = false;
        if self.modeller.model == *self.modeller.base {
            return;
        }
        let before = std::mem::replace(&mut self.modeller.base, Arc::new(self.modeller.model.clone()));
        self.modeller.history.record(before);
        self.note_step(true);
    }

    /// A step went into a history (`modelled`: the model's, else the view's): it is the next to
    /// take back, and what was undone before can no longer be redone.
    pub(super) fn note_step(&mut self, modelled: bool) {
        self.undo_trail.push(modelled);
        self.redo_trail.clear();
        if modelled {
            self.history.clear_redo();
        } else {
            self.modeller.history.clear_redo();
        }
    }

    /// The model as it is now is what later changes are measured against, with nothing to take
    /// back: after a file gave its elements.
    pub(super) fn settle_modelled(&mut self) {
        self.modeller.base = Arc::new(self.modeller.model.clone());
        self.modeller.history.clear();
        self.modeller.touched = false;
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo() || self.modeller.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo() || self.modeller.history.can_redo()
    }

    /// Which history the next step comes from: the one `trail` names last that still has a step
    /// (a view's history is parked with its tab, so the trail can name steps that are away),
    /// else whichever has one.
    fn next_step(trail: &mut Vec<bool>, view: bool, modelled: bool) -> Option<bool> {
        while let Some(of_model) = trail.pop() {
            if if of_model { modelled } else { view } {
                return Some(of_model);
            }
        }
        match (view, modelled) {
            (true, _) => Some(false),
            (false, true) => Some(true),
            (false, false) => None,
        }
    }

    /// Shows the modelled elements as `model` has them, out of edit mode and with no step begun.
    fn restore_modelled(&mut self, model: Arc<bim_core::Model>) {
        self.modeller.model = (*model).clone();
        self.modeller.base = model;
        self.modeller.edit = None;
        self.modeller.points.clear();
        self.modeller.aim = None;
        self.modeller.typing = None;
        self.show_modelled();
        self.modeller.touched = false;
    }

    /// Takes back the last step.
    pub(super) fn undo_step(&mut self) {
        // a move under way is dropped first: the model is then as its last step left it
        if self.moving() {
            return;
        }
        match Self::next_step(&mut self.undo_trail, self.history.can_undo(), self.modeller.history.can_undo()) {
            Some(true) => {
                if let Some(model) = self.modeller.history.undo(self.modeller.base.clone()) {
                    self.restore_modelled(model);
                    self.redo_trail.push(true);
                }
            }
            Some(false) => {
                if let Some(state) = self.history.undo(self.view_state()) {
                    self.restore_view_state(state);
                    self.redo_trail.push(false);
                }
            }
            None => {}
        }
    }

    /// Makes the step taken back last again.
    pub(super) fn redo_step(&mut self) {
        if self.moving() {
            return;
        }
        match Self::next_step(&mut self.redo_trail, self.history.can_redo(), self.modeller.history.can_redo()) {
            Some(true) => {
                if let Some(model) = self.modeller.history.redo(self.modeller.base.clone()) {
                    self.restore_modelled(model);
                    self.undo_trail.push(true);
                }
            }
            Some(false) => {
                if let Some(state) = self.history.redo(self.view_state()) {
                    self.restore_view_state(state);
                    self.undo_trail.push(false);
                }
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::modeller::ModelTool;
    use crate::app::Message;

    /// An app in a new model.
    fn modelling() -> App {
        let mut app = App::default();
        app.new_model("Level 0");
        app
    }

    /// Places a wall from `from` to `to` on the work plane, as the tool's two clicks do.
    fn place(app: &mut App, from: [f64; 2], to: [f64; 2]) {
        let shape = app.shape_for(ModelTool::Wall, glam::DVec3::new(from[0], from[1], 0.0), glam::DVec3::new(to[0], to[1], 0.0));
        app.add_element("Wall", None, shape).expect("a wall");
        // as after any message
        app.keep_model_history();
    }

    #[test]
    fn a_placed_element_is_taken_back_and_made_again() {
        let mut app = modelling();
        assert!(!app.can_undo(), "a new model has nothing to take back");
        place(&mut app, [0.0, 0.0], [4.0, 0.0]);
        place(&mut app, [4.0, 0.0], [4.0, 3.0]);
        let both = app.modeller.model.clone();
        assert!(both.len() >= 2 && app.can_undo() && !app.can_redo());
        let _ = app.update(Message::Undo);
        let one = app.modeller.model.len();
        assert!(one < both.len(), "the second wall, and the join it made, are gone");
        let _ = app.update(Message::Undo);
        assert!(app.modeller.model.is_empty() && !app.can_undo() && app.can_redo());
        let _ = app.update(Message::Redo);
        assert_eq!(app.modeller.model.len(), one);
        let _ = app.update(Message::Redo);
        assert_eq!(app.modeller.model, both, "made again as it was");
        assert!(!app.can_redo());
    }

    #[test]
    fn a_new_step_ends_what_could_be_redone() {
        let mut app = modelling();
        place(&mut app, [0.0, 0.0], [4.0, 0.0]);
        let _ = app.update(Message::Undo);
        assert!(app.can_redo());
        place(&mut app, [0.0, 2.0], [4.0, 2.0]);
        assert!(!app.can_redo());
        assert_eq!(app.modeller.model.len(), 1);
    }

    #[test]
    fn a_step_that_changes_nothing_is_no_step() {
        let mut app = modelling();
        place(&mut app, [0.0, 0.0], [4.0, 0.0]);
        app.show_modelled();
        app.keep_model_history();
        let _ = app.update(Message::Undo);
        assert!(app.modeller.model.is_empty(), "one step back empties the model");
    }
}
