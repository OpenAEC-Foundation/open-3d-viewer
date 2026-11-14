//! Levels in the Modeller: a level of the modelled project can be renamed and put at another
//! elevation, and what stands on it follows.
//!
//! - **The form.** In the Modeller, every level of the Levels tab has an Edit button. It opens the
//!   level's name and elevation to type in; Apply changes the level, a refusal stays in the form.
//! - **What follows.** The modelled elements of that level move up or down with it: a wall's and
//!   a system's base, a slab's top, a member's ends, an object's place. An element the kernel
//!   cannot make there stays, and the level with it: nothing changes and the form says so.
//! - **A new level** gets a floor plan of its own, as every storey has one.
//!
//! The level itself is the document's, so undo takes back the elements' move but not the level's.

use crate::app::{App, Message, FONT_BOLD};
use crate::document::{parse_elevation, LevelError};
use crate::theme::Palette;
use bim_core::element::PlanarForm;
use bim_core::{Element, Shape};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, text, text_input};
use iced::{Color, Element as Widget, Fill};
use openaec_ui::Language;
use std::sync::Arc;

/// The form of a level being edited, as typed.
#[derive(Debug, Clone, PartialEq)]
pub struct LevelEdit {
    /// The level's path.
    pub level: String,
    pub name: String,
    pub elevation: String,
    /// Why the last Apply was refused.
    pub error: Option<LevelEditError>,
}

/// Why a level could not be changed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LevelEditError {
    Level(LevelError),
    /// An element of the level cannot be made at the new elevation.
    Element(bim_core::Refusal),
}

/// What the form of a level sends.
#[derive(Debug, Clone)]
pub enum LevelEditMessage {
    /// Open the form of the level at this path.
    Open(Arc<str>),
    Name(String),
    Elevation(String),
    Apply,
    Cancel,
}

/// `shape` (in project coordinates) lifted by `dz`.
fn lifted(mut shape: Shape, dz: f64) -> Shape {
    match &mut shape {
        Shape::Point(p) => p.at.z += dz,
        Shape::Linear(l) => {
            l.start.z += dz;
            l.end.z += dz;
        }
        Shape::Planar(p) => match &mut p.form {
            PlanarForm::Wall { base, .. } => *base += dz,
            PlanarForm::Slab { top, .. } => *top += dz,
        },
        Shape::System(s) => s.base += dz,
        Shape::Connection(_) => {}
    }
    shape
}

impl App {
    /// Whether the levels can be edited: the Modeller is shown and a project is modelled.
    fn levels_editable(&self) -> bool {
        self.modeller.in_modeller && self.modeller.shown.is_some()
    }

    /// A message of a level's form.
    pub(in crate::app) fn level_edit_message(&mut self, message: LevelEditMessage) {
        match message {
            LevelEditMessage::Open(path) => {
                self.modeller.level_edit = self.document.level(&path).map(|l| LevelEdit { level: l.path.clone(), name: l.name.clone(), elevation: format!("{:.3}", l.elevation), error: None });
            }
            LevelEditMessage::Name(name) => {
                if let Some(form) = &mut self.modeller.level_edit {
                    (form.name, form.error) = (name, None);
                }
            }
            LevelEditMessage::Elevation(elevation) => {
                if let Some(form) = &mut self.modeller.level_edit {
                    (form.elevation, form.error) = (elevation, None);
                }
            }
            LevelEditMessage::Apply => self.apply_level_edit(),
            LevelEditMessage::Cancel => self.modeller.level_edit = None,
        }
    }

    /// Changes the level of the form to what was typed; a refusal stays in the form.
    fn apply_level_edit(&mut self) {
        let Some(form) = self.modeller.level_edit.clone() else { return };
        let changed = match parse_elevation(&form.elevation) {
            Some(elevation) => self.set_level(&form.level, &form.name, elevation),
            None => Err(LevelEditError::Level(LevelError::BadElevation)),
        };
        match changed {
            Ok(()) => self.modeller.level_edit = None,
            Err(error) => {
                if let Some(form) = &mut self.modeller.level_edit {
                    form.error = Some(error);
                }
            }
        }
    }

    /// Renames the level at `path` and puts it at `elevation`; the modelled elements of the level
    /// move with it. When one of them cannot be made there, nothing changes.
    pub(in crate::app) fn set_level(&mut self, path: &str, name: &str, elevation: f64) -> Result<(), LevelEditError> {
        let Some(before) = self.document.level(path).map(|l| l.elevation) else { return Ok(()) };
        // tried on a copy of the model first, so a refusal leaves everything as it was
        let grid = self.modeller.model.grid;
        let mut model = self.modeller.model.clone();
        let dz = elevation - before;
        let mut moved = Vec::new();
        if dz.is_finite() && dz != 0.0 {
            let on_level: Vec<Element> = model.iter().filter(|e| e.level.as_deref() == Some(path) && !matches!(e.shape, Shape::Connection(_))).cloned().collect();
            for e in on_level {
                let mut lifted = Element::in_project(&grid, e.name.clone(), e.level.clone(), lifted(e.shape_in_project(&grid), dz));
                lifted.id = e.id;
                model.replace(lifted).map_err(LevelEditError::Element)?;
                moved.push(e.id);
            }
        }
        self.document.set_level(path, name, elevation).map_err(LevelEditError::Level)?;
        self.modeller.model = model;
        self.joins_follow(&moved);
        self.show_modelled();
        Ok(())
    }

    /// A level was added to the document: in the Modeller it gets a floor plan of its own.
    pub(in crate::app) fn level_added(&mut self, path: &str) {
        if self.levels_editable() && !self.document.plans().iter().any(|p| p.level == path) {
            let _ = self.document.add_plan(path);
        }
    }

    /// The Edit button of a level's row in the Levels tab; none outside the Modeller.
    pub(in crate::app) fn level_edit_button(&self, path: &str, p: Palette, l: Language) -> Option<Widget<'static, Message>> {
        self.levels_editable().then(|| {
            button(text(l.text("Bewerken", "Edit")).size(10).color(p.text_secondary))
                .padding([2, 6])
                .style(move |_, s| p.row_button(s))
                .on_press(Message::LevelEdit(LevelEditMessage::Open(Arc::from(path))))
                .into()
        })
    }

    /// The form of the level at `path` when it is the one being edited.
    pub(in crate::app) fn level_edit_form(&self, path: &str, p: Palette, l: Language) -> Option<Widget<'_, Message>> {
        let form = self.modeller.level_edit.as_ref().filter(|f| f.level == path && self.levels_editable())?;
        let send = |m: LevelEditMessage| Message::LevelEdit(m);
        let label = |s: &'static str| text(s).size(11).color(p.text_secondary);
        let mut fields = column![
            text(l.text("Niveau bewerken", "Edit level")).size(12).font(FONT_BOLD),
            label(l.text("Naam", "Name")),
            text_input("", &form.name).on_input(move |s| send(LevelEditMessage::Name(s))).on_submit(send(LevelEditMessage::Apply)).size(12).padding([5, 8]),
            label(l.text("Hoogte (m)", "Elevation (m)")),
            text_input("0.000", &form.elevation).on_input(move |s| send(LevelEditMessage::Elevation(s))).on_submit(send(LevelEditMessage::Apply)).size(12).padding([5, 8]),
        ]
        .spacing(4);
        if let Some(error) = form.error {
            let why = match error {
                LevelEditError::Level(LevelError::EmptyName) => l.text("Geef het niveau een naam.", "Give the level a name.").to_string(),
                LevelEditError::Level(LevelError::DuplicateName) => l.text("Er is al een niveau met deze naam.", "A level with this name exists.").to_string(),
                LevelEditError::Level(LevelError::BadElevation) => l.text("De hoogte is geen getal.", "The elevation is not a number.").to_string(),
                LevelEditError::Element(refusal) => format!("{} {}", l.text("Een element van dit niveau kan daar niet staan:", "An element of this level cannot stand there:"), l.translate(refusal.reason())),
            };
            let red = if p.dark { Color::from_rgb8(0xF8, 0x71, 0x71) } else { Color::from_rgb8(0xDC, 0x26, 0x26) };
            fields = fields.push(text(why).size(11).color(red).wrapping(Wrapping::WordOrGlyph));
        }
        let apply = button(text(l.text("Toepassen", "Apply")).size(12)).padding([4, 12]).style(move |_, s| p.primary_button(s)).on_press(send(LevelEditMessage::Apply));
        let cancel = button(text(l.text("Annuleren", "Cancel")).size(12)).padding([4, 12]).style(move |_, s| p.ribbon_button(false, s)).on_press(send(LevelEditMessage::Cancel));
        fields = fields.push(row![apply, cancel].spacing(6));
        Some(container(fields).padding([6, 8]).width(Fill).into())
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::modelling;
    use super::*;
    use bim_core::PlanarDef;
    use glam::DVec2;

    fn wall(app: &mut App, level: Option<String>, base: f64) -> bim_core::ElementId {
        let form = PlanarForm::Wall { start: DVec2::new(0.0, 0.0), end: DVec2::new(4.0, 0.0), base, height: 3.0 };
        app.add_element("Wall", level, Shape::Planar(PlanarDef { form, thickness: 0.2, openings: Vec::new(), build_up: None })).unwrap()
    }

    fn base_of(app: &App, id: bim_core::ElementId) -> f64 {
        let Shape::Planar(PlanarDef { form: PlanarForm::Wall { base, .. }, .. }) = app.modeller.model.get(id).unwrap().shape_in_project(&app.modeller.model.grid) else { panic!() };
        base
    }

    #[test]
    fn a_level_is_renamed_and_moved_and_its_elements_follow() {
        let mut app = modelling();
        app.modeller.in_modeller = true;
        let ground = app.document.levels()[0].path.clone();
        let _ = app.document.add_level("Level 1", 3.0);
        let first = app.document.levels()[1].path.clone();
        let (low, high, loose) = (wall(&mut app, Some(ground.clone()), 0.0), wall(&mut app, Some(first.clone()), 3.0), wall(&mut app, None, 6.0));
        // the form of the first floor: another name, 3.4 m up
        let send = |app: &mut App, m: LevelEditMessage| {
            let _ = app.update(Message::LevelEdit(m));
        };
        send(&mut app, LevelEditMessage::Open(Arc::from(first.as_str())));
        assert_eq!(app.modeller.level_edit.as_ref().map(|f| (f.name.as_str(), f.elevation.as_str())), Some(("Level 1", "3.000")));
        send(&mut app, LevelEditMessage::Name("Verdieping".into()));
        send(&mut app, LevelEditMessage::Elevation("3,4".into()));
        send(&mut app, LevelEditMessage::Apply);
        assert!(app.modeller.level_edit.is_none());
        let level = app.document.level(&first).unwrap();
        assert_eq!((level.name.as_str(), level.elevation), ("Verdieping", 3.4));
        assert!((base_of(&app, high) - 3.4).abs() < 1e-9, "its wall went up with it");
        assert_eq!((base_of(&app, low), base_of(&app, loose)), (0.0, 6.0), "the others stay");
        // what the viewport shows follows
        let top = app.world_bounds().unwrap().max[2];
        assert!((top - 9.0).abs() < 1e-3, "{top}");
        // a name another level has, and an elevation that is no number, stay in the form
        send(&mut app, LevelEditMessage::Open(Arc::from(first.as_str())));
        send(&mut app, LevelEditMessage::Name("level 0".into()));
        send(&mut app, LevelEditMessage::Apply);
        assert_eq!(app.modeller.level_edit.as_ref().and_then(|f| f.error), Some(LevelEditError::Level(LevelError::DuplicateName)));
        send(&mut app, LevelEditMessage::Name("Verdieping".into()));
        send(&mut app, LevelEditMessage::Elevation("hoog".into()));
        send(&mut app, LevelEditMessage::Apply);
        assert_eq!(app.modeller.level_edit.as_ref().and_then(|f| f.error), Some(LevelEditError::Level(LevelError::BadElevation)));
        assert!((base_of(&app, high) - 3.4).abs() < 1e-9, "nothing moved");
        send(&mut app, LevelEditMessage::Cancel);
        assert!(app.modeller.level_edit.is_none());
    }

    #[test]
    fn a_new_level_of_the_modeller_gets_a_floor_plan() {
        let mut app = modelling();
        app.modeller.in_modeller = true;
        let _ = app.update(Message::NewLevel("Level"));
        let _ = app.update(Message::AddLevel);
        let added = app.document.levels().last().unwrap().path.clone();
        assert!(app.document.plans().iter().any(|p| p.level == added));
        // outside the Modeller a level is a level only
        app.modeller.in_modeller = false;
        let _ = app.update(Message::NewLevel("Level"));
        let _ = app.update(Message::AddLevel);
        assert_eq!(app.document.plans().len(), 1);
        assert!(app.level_edit_button(&added, crate::theme::ThemeKind::Light.palette(), Language::English).is_none());
    }
}
