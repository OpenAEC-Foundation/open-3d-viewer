//! A typed distance in a move: while a point is grabbed (G in edit mode, or a grip dragged),
//! digits type how far it goes, as in the reference modeller.
//!
//! - The distance is in metres, with a point or a comma, a minus allowed.
//! - It runs along the axis the move is held to (X, Y or Z). With no axis, a member with one end
//!   picked grows along itself, away from its other end. Where neither says a direction, typing
//!   moves nothing.
//! - While a distance is typed the cursor does not move the points. Backspace takes a character
//!   back, Enter puts the move down (one step to take back), Esc drops it.

use crate::app::edit::Axis;
use crate::app::App;
use bim_core::Shape;
use glam::DVec3;

/// The characters a distance is typed with.
pub(crate) const DIGITS: &str = "0123456789.,-";

impl App {
    /// The distance being typed in the move under way; `None` when nothing is typed.
    pub(crate) fn typed_distance(&self) -> Option<&str> {
        let typed = &self.modeller.edit.as_ref()?.grab.as_ref()?.typed;
        (!typed.is_empty()).then_some(typed.as_str())
    }

    /// The way a typed distance runs: along the axis the move is held to, else, for a member with
    /// one end picked, along the member away from its other end.
    fn typed_direction(&self) -> Option<DVec3> {
        let edit = self.modeller.edit.as_ref()?;
        let grab = edit.grab.as_ref()?;
        match grab.axis {
            Some(Axis::X) => return Some(DVec3::X),
            Some(Axis::Y) => return Some(DVec3::Y),
            Some(Axis::Z) => return Some(DVec3::Z),
            None => {}
        }
        let (e, _) = self.edited()?;
        let (&[picked], Shape::Linear(_)) = (&edit.picked[..], &e.shape) else { return None };
        let along = *grab.from.get(picked)? - *grab.from.get(1 - picked)?;
        (along.length() > bim_core::LINEAR_TOLERANCE).then(|| along.normalize())
    }

    /// The move the typed distance gives, when it is a number and has a way to run.
    fn typed_delta(&self) -> Option<DVec3> {
        let distance = crate::document::parse_elevation(self.typed_distance()?)?;
        Some(self.typed_direction()? * distance)
    }

    /// The move under way follows what is typed: the distance where it is one, else nothing.
    fn follow_typed(&mut self) {
        let delta = self.typed_delta().unwrap_or(DVec3::ZERO);
        if let Some(grab) = self.modeller.edit.as_mut().and_then(|e| e.grab.as_mut()) {
            grab.delta = delta;
        }
    }

    /// The axis changed while a distance is typed: the move follows it.
    pub(in crate::app) fn edit_type_again(&mut self) {
        self.follow_typed();
    }

    /// A character of a distance typed while a move is under way: a digit, a point or a comma,
    /// or a minus in front.
    pub(in crate::app) fn edit_type(&mut self, c: char) {
        let Some(grab) = self.modeller.edit.as_mut().and_then(|e| e.grab.as_mut()) else { return };
        let fits = match c {
            '-' => grab.typed.is_empty(),
            '.' | ',' => !grab.typed.contains(['.', ',']),
            c => c.is_ascii_digit(),
        };
        if fits {
            grab.typed.push(c);
            self.follow_typed();
        }
    }

    /// Backspace while a distance is typed: its last character goes. False when nothing is typed.
    pub(in crate::app) fn edit_backspace(&mut self) -> bool {
        if self.typed_distance().is_none() {
            return false;
        }
        if let Some(grab) = self.modeller.edit.as_mut().and_then(|e| e.grab.as_mut()) {
            grab.typed.pop();
        }
        self.follow_typed();
        true
    }

    /// Enter while a distance is typed: the move is put down that far. What is typed and is no
    /// number yet stays to be typed on; a number with no way to run is dropped. False when
    /// nothing is typed.
    pub(in crate::app) fn edit_enter(&mut self) -> bool {
        let Some(typed) = self.typed_distance() else { return false };
        if crate::document::parse_elevation(typed).is_none() {
            return true;
        }
        if self.typed_delta().is_some() {
            self.follow_typed();
            self.put_down();
        } else if let Some(grab) = self.modeller.edit.as_mut().and_then(|e| e.grab.as_mut()) {
            grab.typed.clear();
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::modelling;
    use crate::app::edit::{edit_points, EditEvent};
    use crate::app::{App, Message};
    use bim_core::element::PlanarForm;
    use bim_core::{ElementId, LinearDef, PlanarDef, ProfileDef, Shape};
    use glam::{DVec2, DVec3};

    fn beam(app: &mut App) -> ElementId {
        let shape = Shape::Linear(LinearDef { start: DVec3::new(0.0, 0.0, 2.8), end: DVec3::new(4.0, 0.0, 2.8), profile: ProfileDef::Rectangle { width: 0.2, height: 0.4 }, roll: 0.0 });
        let id = app.add_element("Beam", None, shape).unwrap();
        app.keep_model_history();
        id
    }

    fn ends(app: &App, id: ElementId) -> (DVec3, DVec3) {
        let (p, _) = edit_points(&app.modeller.model.get(id).unwrap().shape_in_project(&app.modeller.model.grid));
        (p[0], p[1])
    }

    /// Tab on the element, its point `index` picked and grabbed with G.
    fn grab(app: &mut App, id: ElementId, index: usize) {
        // Tab opens edit mode; it stays open after a move is put down
        if app.modeller.edit.is_none() {
            app.select_modelled(id);
            let _ = app.update(Message::EditMode);
        }
        let _ = app.update(Message::Edit(EditEvent::Press { index, add: false }));
        let _ = app.update(Message::Edit(EditEvent::Release));
        let _ = app.update(Message::EditKey('g'));
    }

    fn keys(app: &mut App, typed: &str) {
        for c in typed.chars() {
            let _ = app.update(Message::EditKey(c));
        }
    }

    #[test]
    fn a_typed_distance_lifts_a_beams_end_in_one_step() {
        let mut app = modelling();
        let id = beam(&mut app);
        grab(&mut app, id, 1);
        keys(&mut app, "z0.5");
        assert_eq!(app.typed_distance(), Some("0.5"));
        let _ = app.update(Message::MeasureFinish);
        assert!((ends(&app, id).1 - DVec3::new(4.0, 0.0, 3.3)).length() < 1e-9, "{:?}", ends(&app, id));
        assert!(app.modeller.edit.is_some(), "edit mode stays");
        let _ = app.update(Message::Undo);
        assert!((ends(&app, id).1 - DVec3::new(4.0, 0.0, 2.8)).length() < 1e-9, "one step back");
    }

    #[test]
    fn with_no_axis_a_member_grows_along_itself() {
        let mut app = modelling();
        let id = beam(&mut app);
        grab(&mut app, id, 1);
        keys(&mut app, "1");
        let _ = app.update(Message::MeasureFinish);
        assert!((ends(&app, id).1 - DVec3::new(5.0, 0.0, 2.8)).length() < 1e-9, "{:?}", ends(&app, id));
        // its start, a minus: 0.5 m shorter from that end
        grab(&mut app, id, 0);
        keys(&mut app, "-0,5");
        let _ = app.update(Message::MeasureFinish);
        assert!((ends(&app, id).0 - DVec3::new(0.5, 0.0, 2.8)).length() < 1e-9, "{:?}", ends(&app, id));
    }

    #[test]
    fn backspace_takes_a_character_back_and_enter_needs_a_number() {
        let mut app = modelling();
        let id = beam(&mut app);
        grab(&mut app, id, 1);
        keys(&mut app, "x12");
        let _ = app.update(Message::DeletePicked);
        assert_eq!(app.typed_distance(), Some("1"));
        let _ = app.update(Message::DeletePicked);
        assert_eq!(app.typed_distance(), None);
        // a minus alone is no number: Enter puts nothing down
        keys(&mut app, "-");
        let _ = app.update(Message::MeasureFinish);
        assert!(app.modeller.edit.as_ref().is_some_and(|e| e.grab.is_some()), "the grab is still on");
        assert!((ends(&app, id).1 - DVec3::new(4.0, 0.0, 2.8)).length() < 1e-9);
        // Esc drops the move and what was typed
        let _ = app.update(Message::Escape);
        assert_eq!(app.typed_distance(), None);
        assert!((ends(&app, id).1 - DVec3::new(4.0, 0.0, 2.8)).length() < 1e-9);
    }

    #[test]
    fn typing_on_a_walls_end_without_an_axis_moves_nothing() {
        let mut app = modelling();
        let form = PlanarForm::Wall { start: DVec2::ZERO, end: DVec2::new(4.0, 0.0), base: 0.0, height: 3.0 };
        let id = app.add_element("Wall", None, Shape::Planar(PlanarDef { form, thickness: 0.2, openings: Vec::new(), build_up: None })).unwrap();
        app.keep_model_history();
        grab(&mut app, id, 1);
        keys(&mut app, "2");
        let _ = app.update(Message::MeasureFinish);
        assert!((ends(&app, id).1 - DVec3::new(4.0, 0.0, 0.0)).length() < 1e-9, "no direction: nothing moves");
        // held to x it does
        keys(&mut app, "x2");
        let _ = app.update(Message::MeasureFinish);
        assert!((ends(&app, id).1 - DVec3::new(6.0, 0.0, 0.0)).length() < 1e-9, "{:?}", ends(&app, id));
    }

    #[test]
    fn the_top_grip_takes_a_typed_distance() {
        let mut app = modelling();
        let form = PlanarForm::Wall { start: DVec2::ZERO, end: DVec2::new(4.0, 0.0), base: 0.0, height: 3.0 };
        let id = app.add_element("Wall", None, Shape::Planar(PlanarDef { form, thickness: 0.2, openings: Vec::new(), build_up: None })).unwrap();
        app.keep_model_history();
        app.select_modelled(id);
        app.camera.set_preset(crate::viewport::camera::ViewPreset::Front);
        let _ = app.update(Message::Edit(EditEvent::Press { index: super::super::shape_handles::TOP_GRIP, add: false }));
        keys(&mut app, "0.4");
        let _ = app.update(Message::MeasureFinish);
        let Shape::Planar(PlanarDef { form: PlanarForm::Wall { height, .. }, .. }) = app.modeller.model.get(id).unwrap().shape_in_project(&app.modeller.model.grid) else { panic!() };
        assert!((height - 3.4).abs() < 1e-9, "{height}");
    }
}
