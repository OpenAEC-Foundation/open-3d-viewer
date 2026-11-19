//! Undo and redo across what the app changes: what is shown (hide, show, layers: kept per view)
//! and the modelled elements (every step that changes the model: placing, moving, joining,
//! deleting, typing a size). Ctrl+Z takes back the last of either, in the order they were made.
//!
//! The project's library and its drawing standard are part of a step of the model: a change of a
//! type is taken back with the elements that followed it, and a change of the library or the
//! standard alone is a step too, with or without a model. Typing in one field of a Library
//! editor that takes every keystroke, or dragging its sliders, is one step.

use super::buildups::BuildUpMessage;
use super::materials::MaterialMessage;
use super::modeller::Step;
use super::profiles::ProfileMessage;
use super::standards::{Field, Setting, StandardMessage};
use super::{App, Message};
use bim_library::Lang;
use std::sync::Arc;

/// A field of a Library editor that changes the library or the standard at every keystroke, or
/// a slider at every move: what is typed or dragged there in one go is one step to take back.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Typing {
    /// The name of a material (its key) in a language.
    MaterialName(String, Lang),
    /// The colour of a material: typed, or a channel at a time.
    MaterialColour(String),
    /// The grade typed for a material (its key).
    MaterialGrade(String),
    /// The name of a code in a language.
    CodeName(String, Lang),
    /// The name typed for a build-up (its id).
    BuildUpName(String),
    /// The name typed for a linear type (its id).
    ProfileName(String),
    /// A field of a setting of the drawing standard, in a language.
    Standard(Setting, Field, Lang),
}

impl App {
    /// The field `message` types into, when it is one that takes every keystroke.
    pub(super) fn typing_of(&self, message: &Message) -> Option<Typing> {
        match message {
            Message::Material(MaterialMessage::Named(lang, _)) => Some(Typing::MaterialName(self.material_view.picked.clone()?, *lang)),
            Message::Material(MaterialMessage::Hex(_) | MaterialMessage::Channel(..) | MaterialMessage::Shown) => Some(Typing::MaterialColour(self.material_view.picked.clone()?)),
            Message::Material(MaterialMessage::Graded(_)) => Some(Typing::MaterialGrade(self.material_view.picked.clone()?)),
            Message::Code(super::codes::CodeMessage::Named(lang, _)) => Some(Typing::CodeName(self.code_view.picked.clone()?, *lang)),
            Message::BuildUp(BuildUpMessage::Named(_)) => Some(Typing::BuildUpName(self.buildup_view.picked.clone()?)),
            Message::Profile(ProfileMessage::Named(_)) => Some(Typing::ProfileName(self.profile_view.picked.clone()?)),
            Message::Standard(StandardMessage::Typed(field, _, lang)) => Some(Typing::Standard(self.standard_view.picked.clone()?, *field, *lang)),
            _ => None,
        }
    }

    /// Before a message is handled: when the history has not looked at the library and the
    /// drawing standard yet (at the start, or in a project just begun), it notes how they are,
    /// so the first change of them is a step too.
    pub(super) fn look_at_setup(&mut self) {
        let base = &mut self.modeller.base;
        if base.library.is_none() {
            base.library = Some(Arc::new(self.library.clone()));
        }
        if base.standard.is_none() {
            base.standard = Some(Arc::new(self.standard.clone()));
        }
    }

    /// Whether the project's library or its drawing standard is no longer as the history last
    /// saw it.
    fn setup_changed(&mut self) -> bool {
        self.look_at_setup();
        let base = &self.modeller.base;
        base.library.as_deref() != Some(&self.library) || base.standard.as_deref() != Some(&self.standard)
    }

    /// A move being made in edit mode or with a grip: the model changes at every move of the
    /// cursor, and is one step once it is put down.
    fn moving(&self) -> bool {
        self.modeller.edit.as_ref().is_some_and(|e| e.grab.is_some()) || self.modeller.grab.is_some()
    }

    /// After any message: when the modelled elements, the library or the drawing standard
    /// changed, the project as it was goes into the history, as one step to take back.
    pub(super) fn keep_model_history(&mut self) {
        if self.moving() {
            return;
        }
        let typing = self.step_typing.take();
        let setup_changed = self.setup_changed();
        if !self.modeller.touched && !setup_changed {
            // the levels changed without the model (a level of the form, a file added): no step,
            // and what later steps are measured against has them
            if self.document.levels() != self.modeller.base.levels.as_slice() {
                self.modeller.base.levels = Arc::new(self.document.levels().to_vec());
            }
            return;
        }
        self.modeller.touched = false;
        let base = &self.modeller.base;
        let model_same = self.modeller.model == *base.model;
        if model_same && self.modeller.taken == base.taken && self.modeller.project_file == base.project_file && self.document.levels() == base.levels.as_slice() && !setup_changed {
            return;
        }
        // (a step that left the elements as they were shares them with the step before)
        let model = if model_same { base.model.clone() } else { Arc::new(self.modeller.model.clone()) };
        let now = self.modelled_step_of(model);
        let before = std::mem::replace(&mut self.modeller.base, now);
        // typing on in the field the last step was typed in is that step: what is kept to go
        // back to is the project before its first keystroke
        if typing.is_some() && typing == self.last_typing {
            return;
        }
        self.modeller.history.record(before);
        self.note_step(true);
        self.last_typing = typing;
    }

    /// The modelled project as it is now: its elements, the walls taken over from opened files,
    /// the document it became, and its library and drawing standard.
    fn modelled_step(&self) -> Step {
        self.modelled_step_of(Arc::new(self.modeller.model.clone()))
    }

    /// The modelled project as it is now, its elements being `model`.
    fn modelled_step_of(&self, model: Arc<bim_core::Model>) -> Step {
        Step {
            model,
            taken: self.modeller.taken.clone(),
            project_file: self.modeller.project_file.clone(),
            levels: Arc::new(self.document.levels().to_vec()),
            library: Some(Arc::new(self.library.clone())),
            standard: Some(Arc::new(self.standard.clone())),
        }
    }

    /// A step went into a history (`modelled`: the model's, else the view's): it is the next to
    /// take back, and what was undone before can no longer be redone.
    pub(super) fn note_step(&mut self, modelled: bool) {
        self.undo_trail.push(modelled);
        self.redo_trail.clear();
        // (typing after another step is a step of its own)
        self.last_typing = None;
        if modelled {
            self.history.clear_redo();
        } else {
            self.modeller.history.clear_redo();
        }
    }

    /// The model as it is now is what later changes are measured against, with nothing to take
    /// back: after a file gave its elements.
    pub(super) fn settle_modelled(&mut self) {
        self.modeller.base = self.modelled_step();
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

    /// Shows the modelled project as `step` has it, out of edit mode and with no step begun: its
    /// elements, and of the opened files the walls that were not taken over yet.
    fn restore_modelled(&mut self, step: Step) {
        // the library and the drawing standard as they stood: a type is taken back with the
        // elements that followed it
        let mut setup = false;
        if let Some(library) = step.library.as_ref().filter(|library| ***library != self.library) {
            self.library = (**library).clone();
            setup = true;
        }
        if let Some(standard) = step.standard.as_ref().filter(|standard| ***standard != self.standard) {
            self.standard = (**standard).clone();
            setup = true;
        }
        if setup {
            self.settle_library_editors();
        }
        self.last_typing = None;
        self.modeller.model = (*step.model).clone();
        self.modeller.project_file = step.project_file.clone();
        // the levels as they stood: a level's move is taken back with what it moved
        if self.document.levels() != step.levels.as_slice() {
            self.document.set_levels(step.levels.to_vec());
        }
        self.show_files_without(step.taken.clone());
        self.modeller.base = step;
        self.modeller.edit = None;
        self.modeller.points.clear();
        self.modeller.aim = None;
        self.modeller.typing = None;
        // (a project with no model, in Library, gets none shown by a step taken back)
        if self.modeller.shown.is_some() || !self.modeller.model.is_empty() {
            self.show_modelled();
        }
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
    fn a_change_of_the_library_is_a_step_with_or_without_a_model() {
        let mut app = App::default();
        let materials = app.library.materials.len();
        // the first message of all: the history has the library as it was before it
        let _ = app.update(Message::Material(MaterialMessage::Add));
        assert_eq!(app.library.materials.len(), materials + 1);
        assert!(app.can_undo());
        // taken back: the material is gone, its editor lets go of it, and no model is shown
        // for it
        let _ = app.update(Message::Undo);
        assert_eq!((app.library.materials.len(), app.material_view.picked.clone(), app.modeller.shown, app.models.len()), (materials, None, None, 0));
        assert!(!app.can_undo() && app.can_redo());
        let _ = app.update(Message::Redo);
        assert_eq!(app.library.materials.len(), materials + 1);
        // a message that changes nothing is no step
        let _ = app.update(Message::Material(MaterialMessage::Pick("brick".into())));
        let _ = app.update(Message::Undo);
        assert!(app.library.materials.len() == materials && !app.can_undo());
    }

    #[test]
    fn typing_in_one_field_is_one_step() {
        let mut app = App::default();
        let _ = app.update(Message::Material(MaterialMessage::Pick("brick".into())));
        let was = app.library.material("brick").unwrap().clone();
        let brick = |app: &App| app.library.material("brick").unwrap().clone();
        // a name, a keystroke at a time
        for typed in ["b", "ba", "bak", "baks", "bakst", "bakste", "bakstee", "baksteen"] {
            let _ = app.update(Message::Material(MaterialMessage::Named(Lang::Nl, typed.into())));
        }
        assert_eq!(brick(&app).name(Lang::Nl), "baksteen");
        // its colour: a slider dragged and let go, then typed
        for value in [10, 60, 120] {
            let _ = app.update(Message::Material(MaterialMessage::Channel(0, value)));
        }
        let _ = app.update(Message::Material(MaterialMessage::Shown));
        let _ = app.update(Message::Material(MaterialMessage::Hex("#102030".into())));
        assert_eq!(brick(&app).colour, [16, 32, 48]);
        // one step back is the colour before the slider moved; what was being typed is
        // forgotten, so the fields show what is there
        let _ = app.update(Message::Undo);
        assert_eq!((brick(&app).colour, brick(&app).name(Lang::Nl).to_string()), (was.colour, "baksteen".to_string()));
        assert_eq!((app.material_view.hex.clone(), app.material_view.name.clone()), (None, None));
        // one more is the name before its first keystroke
        let _ = app.update(Message::Undo);
        assert_eq!(brick(&app), was);
        assert!(!app.can_undo());
        // typing after a step taken back, and in another language, are steps of their own
        let _ = app.update(Message::Material(MaterialMessage::Named(Lang::Nl, "klinker".into())));
        let _ = app.update(Message::Material(MaterialMessage::Named(Lang::En, "clinker".into())));
        let _ = app.update(Message::Undo);
        assert_eq!((brick(&app).name(Lang::Nl).to_string(), brick(&app).name(Lang::En).to_string()), ("klinker".to_string(), was.name(Lang::En).to_string()));
        let _ = app.update(Message::Undo);
        assert_eq!(brick(&app), was);
    }

    #[test]
    fn the_drawing_standard_is_taken_back_too() {
        let mut app = App::default();
        let kinds = app.standard.kinds.len();
        let _ = app.update(Message::Standard(StandardMessage::AddKind));
        assert_eq!(app.standard.kinds.len(), kinds + 1);
        let _ = app.update(Message::Undo);
        assert_eq!(app.standard.kinds.len(), kinds);
        let _ = app.update(Message::Redo);
        assert_eq!(app.standard.kinds.len(), kinds + 1);
    }

    #[test]
    fn a_library_step_between_model_steps_is_taken_back_in_its_turn() {
        let mut app = modelling();
        place(&mut app, [0.0, 0.0], [4.0, 0.0]);
        let one = app.modeller.model.clone();
        let materials = app.library.materials.len();
        let _ = app.update(Message::Material(MaterialMessage::Add));
        place(&mut app, [4.0, 0.0], [4.0, 3.0]);
        // the second wall goes first, and the library keeps its new material
        let _ = app.update(Message::Undo);
        assert_eq!((app.modeller.model.clone(), app.library.materials.len()), (one.clone(), materials + 1));
        // then the material, and the first wall stands
        let _ = app.update(Message::Undo);
        assert_eq!((app.modeller.model.clone(), app.library.materials.len()), (one, materials));
        let _ = app.update(Message::Undo);
        assert!(app.modeller.model.is_empty() && app.library.materials.len() == materials && !app.can_undo());
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
