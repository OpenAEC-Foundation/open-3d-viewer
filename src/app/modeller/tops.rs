//! Elements between levels: a wall, a standing system and a column stand on their level (the
//! element's `level`) and reach either a height of their own or a level above, with an offset
//! (the element's `top`, see `bim_core::Top`).
//!
//! The shape keeps the absolute geometry; it is worked out again from the levels whenever one
//! moves or a top changes: a wall's or a system's height, a column's upper end. Moving a level
//! moves what stands on it and stretches what reaches up to it; what cannot be made so (a top at
//! or below its base) is refused and nothing changes. Removing a level frees the tops that reach
//! it, each keeping its height.

use crate::app::{App, Message};
use crate::theme::Palette;
use bim_core::element::PlanarForm;
use bim_core::{ElementId, PlanarDef, Refusal, Shape, Top};
use iced::widget::text::Wrapping;
use iced::widget::{column, pick_list, row, text, text_input};
use iced::{Center, Element as Widget, Length};
use openaec_ui::Language;

/// A member is a column when its ends lie this near above each other in plan (metres).
const UPRIGHT: f64 = 1e-6;

/// Whether an element of `shape` can reach a level with its top: a wall, a standing system, a
/// column.
pub(in crate::app) fn has_top(shape: &Shape) -> bool {
    match shape {
        Shape::Planar(PlanarDef { form: PlanarForm::Wall { .. }, .. }) => true,
        Shape::System(s) => !s.lies(),
        Shape::Linear(l) => (l.end - l.start).truncate().length() <= UPRIGHT && l.end.z != l.start.z,
        _ => false,
    }
}

/// Where `shape` stands and how high its top is: (base, top), in its own coordinates.
pub(in crate::app) fn base_and_top(shape: &Shape) -> Option<(f64, f64)> {
    if !has_top(shape) {
        return None;
    }
    match shape {
        Shape::Planar(PlanarDef { form: PlanarForm::Wall { base, height, .. }, .. }) => Some((*base, base + height)),
        Shape::System(s) => Some((s.base, s.base + s.height)),
        Shape::Linear(l) => Some((l.start.z.min(l.end.z), l.start.z.max(l.end.z))),
        _ => None,
    }
}

/// `shape` with its top at height `z`, its base where it was; a top at or below the base is
/// refused. `None` for a shape that has no top.
pub(in crate::app) fn topped(shape: &Shape, z: f64) -> Option<Result<Shape, Refusal>> {
    let (base, _) = base_and_top(shape)?;
    if z - base <= bim_core::LINEAR_TOLERANCE {
        return Some(Err(Refusal::NotPositive));
    }
    let mut shape = shape.clone();
    match &mut shape {
        Shape::Planar(PlanarDef { form: PlanarForm::Wall { height, .. }, .. }) => *height = z - base,
        Shape::System(s) => s.height = z - base,
        Shape::Linear(l) => {
            if l.end.z >= l.start.z {
                l.end.z = z;
            } else {
                l.start.z = z;
            }
        }
        _ => {}
    }
    Some(Ok(shape))
}

/// Gives every element of `model` that reaches a level the top that level says (`elevation`: a
/// level's height by its path; a level that is gone leaves its elements as they are). Returns
/// the elements that changed; the first one the kernel cannot make so is the refusal.
pub(in crate::app) fn fit_tops(model: &mut bim_core::Model, elevation: &dyn Fn(&str) -> Option<f64>) -> Result<Vec<ElementId>, Refusal> {
    let grid = model.grid;
    let reaching: Vec<bim_core::Element> = model.iter().filter(|e| e.top.is_some()).cloned().collect();
    let mut changed = Vec::new();
    for e in reaching {
        let Some(top) = &e.top else { continue };
        let Some(z) = elevation(&top.level).map(|z| z + top.offset) else { continue };
        let shape = e.shape_in_project(&grid);
        let Some(fitted) = topped(&shape, z) else { continue };
        let fitted = fitted?;
        if fitted != shape {
            model.replace(e.reshaped(&grid, fitted))?;
            changed.push(e.id);
        }
    }
    Ok(changed)
}

impl App {
    /// Gives element `id` the top `top` (`None`: a height of its own, the one it has): its shape
    /// follows. When the kernel cannot make it so, nothing changes.
    pub(in crate::app) fn set_top(&mut self, id: ElementId, top: Option<Top>) -> Result<(), Refusal> {
        let mut model = self.modeller.model.clone();
        let mut element = model.get(id).cloned().ok_or(Refusal::Missing)?;
        if top.is_some() && !has_top(&element.shape) {
            return Err(Refusal::NotPositive);
        }
        element.top = top;
        model.replace(element)?;
        fit_tops(&mut model, &|path| self.document.level(path).map(|l| l.elevation))?;
        self.modeller.model = model;
        self.joins_follow(&[id]);
        self.show_modelled();
        self.select_modelled(id);
        Ok(())
    }

    /// A level is gone: the elements that reached it keep their height as their own.
    pub(in crate::app) fn free_tops_of(&mut self, level: &str) {
        let reaching: Vec<bim_core::Element> = self.modeller.model.iter().filter(|e| e.top.as_ref().is_some_and(|t| t.level == level)).cloned().collect();
        for mut e in reaching {
            e.top = None;
            let _ = self.modeller.model.replace(e);
        }
    }
}

/// What a top can be in the properties' list: free (no level), or a level by its name.
#[derive(Debug, Clone, PartialEq)]
pub struct TopChoice {
    pub level: Option<String>,
    pub name: String,
}

impl std::fmt::Display for TopChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

/// What the properties' rows of a top send.
#[derive(Debug, Clone)]
pub enum TopMessage {
    /// The level the top reaches (its path), or none: a height of its own.
    Level(Option<String>),
    /// The offset above that level, as typed.
    Offset(String),
    OffsetDone,
}

impl App {
    /// The level above the one at `level` (its path): the next one up, if there is one.
    pub(in crate::app) fn level_above(&self, level: Option<&str>) -> Option<String> {
        let base = self.document.level(level?)?.elevation;
        self.document.levels().iter().find(|l| l.elevation > base + 0.1).map(|l| l.path.clone())
    }

    /// The tops element `e` can take, for the properties' list: free, then the levels above its
    /// base. Nothing for an element that has no top to set.
    pub(in crate::app) fn top_choices(&self, e: &bim_core::Element, l: Language) -> Vec<TopChoice> {
        let Some((base, _)) = base_and_top(&e.shape_in_project(&self.modeller.model.grid)) else { return Vec::new() };
        let free = TopChoice { level: None, name: l.text("Vrij", "Free").to_string() };
        let above = self.document.levels().iter().filter(|level| level.elevation > base + bim_core::LINEAR_TOLERANCE).map(|level| TopChoice { level: Some(level.path.clone()), name: level.name.clone() });
        std::iter::once(free).chain(above).collect()
    }

    /// A message of the properties' rows of a top, for the element selected.
    pub(in crate::app) fn top_message(&mut self, message: TopMessage) {
        let Some(e) = self.defined().cloned() else { return };
        self.modeller.refusal = None;
        let result = match message {
            TopMessage::Level(level) => {
                self.modeller.top_offset = None;
                self.set_top(e.id, level.map(|level| Top { level, offset: 0.0 }))
            }
            TopMessage::Offset(typed) => {
                let offset = crate::document::parse_elevation(typed.trim());
                self.modeller.top_offset = Some((e.id, typed));
                match (offset, e.top) {
                    (Some(offset), Some(top)) => self.set_top(e.id, Some(Top { offset, ..top })),
                    _ => Ok(()),
                }
            }
            TopMessage::OffsetDone => {
                self.modeller.top_offset = None;
                Ok(())
            }
        };
        if let Err(refusal) = result {
            self.modeller.refusal = Some(refusal);
        }
    }

    /// The rows of an element between levels in the properties: its base level and how far above
    /// it it stands, the level its top reaches (to choose) and the offset above that (to type).
    /// `None` for an element that has no top to set.
    pub(in crate::app) fn top_rows(&self, e: &bim_core::Element, p: Palette, l: Language) -> Option<Widget<'_, Message>> {
        let choices = self.top_choices(e, l);
        let (base, _) = base_and_top(&e.shape_in_project(&self.modeller.model.grid))?;
        let level = e.level.as_deref().and_then(|path| self.document.level(path));
        let label = |s: &str| text(s.to_string()).size(12).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph).width(Length::FillPortion(2));
        let mut rows = column![
            crate::app::prop_row(p, l.text("Basisniveau", "Base level").to_string(), level.map_or_else(|| l.text("geen", "none").to_string(), |v| v.name.clone())),
            crate::app::prop_row(p, l.text("Basis boven niveau", "Base above level").to_string(), level.map_or_else(String::new, |v| format!("{:.3} m", base - v.elevation))),
        ]
        .spacing(5);
        let picked = choices.iter().find(|c| c.level.as_deref() == e.top.as_ref().map(|t| t.level.as_str())).cloned();
        let list = pick_list(choices, picked, |c: TopChoice| Message::Top(TopMessage::Level(c.level))).text_size(12).padding([2, 6]).width(Length::FillPortion(3));
        rows = rows.push(row![label(l.text("Bovenkant", "Top")), list].spacing(8).align_y(Center));
        if let Some(top) = &e.top {
            let typed = self.modeller.top_offset.as_ref().filter(|(id, _)| *id == e.id).map_or_else(|| format!("{:.3}", top.offset), |(_, typed)| typed.clone());
            let input = text_input("0.000", &typed).on_input(|typed| Message::Top(TopMessage::Offset(typed))).on_submit(Message::Top(TopMessage::OffsetDone)).size(12).padding([2, 6]).width(Length::FillPortion(3));
            rows = rows.push(row![label(l.text("Boven dat niveau (m)", "Above that level (m)")), input].spacing(8).align_y(Center));
        }
        Some(rows.into())
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{click, modelling};
    use super::*;
    use crate::app::transform::TransformKind;
    use bim_core::{LinearDef, ProfileDef};
    use glam::{DVec2, DVec3};

    /// A modelling app with Level 0 at 0 m and Level 1 at 3 m: their paths.
    fn two_levels() -> (App, String, String) {
        let mut app = modelling();
        app.modeller.in_modeller = true;
        let _ = app.document.add_level("Level 1", 3.0);
        let paths: Vec<String> = app.document.levels().iter().map(|l| l.path.clone()).collect();
        (app, paths[0].clone(), paths[1].clone())
    }

    fn wall(app: &mut App, level: &str, height: f64) -> ElementId {
        let form = PlanarForm::Wall { start: DVec2::ZERO, end: DVec2::new(4.0, 0.0), base: 0.0, height };
        app.add_element("Wall", Some(level.to_string()), Shape::Planar(PlanarDef { form, thickness: 0.2, openings: Vec::new(), build_up: None })).unwrap()
    }

    /// A wall's base and height.
    fn stands(app: &App, id: ElementId) -> (f64, f64) {
        match app.modeller.model.get(id).unwrap().shape_in_project(&app.modeller.model.grid) {
            Shape::Planar(PlanarDef { form: PlanarForm::Wall { base, height, .. }, .. }) => (base, height),
            other => panic!("{other:?}"),
        }
    }

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
    }

    #[test]
    fn a_wall_moved_keeps_its_top() {
        let (mut app, ground, first) = two_levels();
        let w = wall(&mut app, &ground, 2.0);
        app.set_top(w, Some(Top { level: first.clone(), offset: 0.0 })).unwrap();
        app.select_modelled(w);
        app.begin_transform(TransformKind::Move);
        click(&mut app, 0.0, 0.0);
        click(&mut app, 0.0, 2.0);
        assert_eq!(app.modeller.model.get(w).unwrap().top, Some(Top { level: first, offset: 0.0 }));
    }

    #[test]
    fn a_wall_reaches_its_top_level_and_follows_it() {
        let (mut app, ground, first) = two_levels();
        let w = wall(&mut app, &ground, 2.0);
        app.set_top(w, Some(Top { level: first.clone(), offset: 0.0 })).unwrap();
        assert!(close(stands(&app, w), (0.0, 3.0)), "{:?}", stands(&app, w));
        app.set_level(&first, "Level 1", 3.4).unwrap();
        assert!(close(stands(&app, w), (0.0, 3.4)), "{:?}", stands(&app, w));
        app.set_level(&ground, "Level 0", 0.2).unwrap();
        assert!(close(stands(&app, w), (0.2, 3.2)), "{:?}", stands(&app, w));
    }

    #[test]
    fn a_top_offset_and_a_column() {
        let (mut app, ground, first) = two_levels();
        let w = wall(&mut app, &ground, 2.0);
        app.set_top(w, Some(Top { level: first.clone(), offset: -0.3 })).unwrap();
        assert!(close(stands(&app, w), (0.0, 2.7)));
        let column = Shape::Linear(LinearDef { start: DVec3::new(6.0, 0.0, 0.0), end: DVec3::new(6.0, 0.0, 2.5), profile: ProfileDef::Rectangle { width: 0.3, height: 0.3 }, roll: 0.0, role: Default::default() });
        let c = app.add_element("Column", Some(ground.clone()), column).unwrap();
        app.set_top(c, Some(Top { level: first, offset: 0.0 })).unwrap();
        let Shape::Linear(l) = app.modeller.model.get(c).unwrap().shape_in_project(&app.modeller.model.grid) else { panic!() };
        assert!((l.end.z - 3.0).abs() < 1e-9 && l.start.z == 0.0, "{l:?}");
    }

    #[test]
    fn a_level_moved_below_a_base_is_refused() {
        let (mut app, ground, first) = two_levels();
        let w = wall(&mut app, &ground, 2.0);
        app.set_top(w, Some(Top { level: first.clone(), offset: 0.0 })).unwrap();
        assert!(app.set_level(&first, "Level 1", -1.0).is_err());
        assert_eq!(app.document.level(&first).unwrap().elevation, 3.0);
        assert!(close(stands(&app, w), (0.0, 3.0)));
    }

    #[test]
    fn undo_puts_the_level_and_its_walls_back() {
        use crate::app::modeller::levels::LevelEditMessage as Edit;
        use crate::app::Message;
        let (mut app, ground, first) = two_levels();
        let w = wall(&mut app, &ground, 2.0);
        app.set_top(w, Some(Top { level: first.clone(), offset: 0.0 })).unwrap();
        app.keep_model_history();
        for m in [Edit::Open(std::sync::Arc::from(first.as_str())), Edit::Elevation("3.4".into()), Edit::Apply] {
            let _ = app.update(Message::LevelEdit(m));
        }
        assert_eq!(app.document.level(&first).unwrap().elevation, 3.4);
        assert!(close(stands(&app, w), (0.0, 3.4)));
        let _ = app.update(Message::Undo);
        assert_eq!(app.document.level(&first).unwrap().elevation, 3.0, "the level is back");
        assert!(close(stands(&app, w), (0.0, 3.0)), "and the wall with it");
        let _ = app.update(Message::Redo);
        assert_eq!(app.document.level(&first).unwrap().elevation, 3.4);
        assert!(close(stands(&app, w), (0.0, 3.4)));
    }

    #[test]
    fn undoing_the_first_element_of_a_new_model_keeps_its_level() {
        use crate::app::modeller::ModelTool;
        use crate::app::Message;
        let mut app = modelling();
        let _ = app.update(Message::ModelTool(Some((ModelTool::Wall, "Wall"))));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 4.0, 0.0);
        assert_eq!(app.modeller.model.len(), 1);
        let _ = app.update(Message::Undo);
        assert!(app.modeller.model.is_empty());
        assert_eq!(app.document.levels().len(), 1, "the first level stays");
    }

    #[test]
    fn a_level_added_by_its_form_stays_when_a_later_step_is_taken_back() {
        use crate::app::modeller::ModelTool;
        use crate::app::Message;
        let mut app = modelling();
        let _ = app.update(Message::NewLevel("Level"));
        let _ = app.update(Message::AddLevel);
        assert_eq!(app.document.levels().len(), 2);
        let _ = app.update(Message::ModelTool(Some((ModelTool::Wall, "Wall"))));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 4.0, 0.0);
        let _ = app.update(Message::Undo);
        assert!(app.modeller.model.is_empty());
        assert_eq!(app.document.levels().len(), 2, "the level added before the wall stays");
    }

    #[test]
    fn the_properties_set_a_top_and_typing_a_height_frees_it() {
        use crate::app::element_props::ElementField;
        use crate::app::Message;
        let (mut app, ground, first) = two_levels();
        let w = wall(&mut app, &ground, 2.0);
        app.select_modelled(w);
        // the levels a top can take: free, then those above the base
        let choices = app.top_choices(app.modeller.model.get(w).unwrap(), openaec_ui::Language::English);
        assert_eq!(choices.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Free", "Level 1"]);
        let _ = app.update(Message::Top(TopMessage::Level(Some(first.clone()))));
        assert!(close(stands(&app, w), (0.0, 3.0)));
        assert_eq!(app.selected_modelled(), vec![w], "it stays selected");
        // an offset typed: the top that far above the level
        let _ = app.update(Message::Top(TopMessage::Offset("-0,25".into())));
        assert!(close(stands(&app, w), (0.0, 2.75)), "{:?}", stands(&app, w));
        assert_eq!(app.modeller.model.get(w).unwrap().top, Some(Top { level: first.clone(), offset: -0.25 }));
        // a height typed frees the top
        let _ = app.update(Message::ElementField(ElementField::Height, "2.4".into()));
        assert_eq!(app.modeller.model.get(w).unwrap().top, None);
        assert!(close(stands(&app, w), (0.0, 2.4)));
        // free by the list
        let _ = app.update(Message::Top(TopMessage::Level(Some(first))));
        let _ = app.update(Message::Top(TopMessage::Level(None)));
        assert_eq!(app.modeller.model.get(w).unwrap().top, None);
        assert!(close(stands(&app, w), (0.0, 3.0)), "its height stays what it was");
        // a beam has no top to set
        let beam = Shape::Linear(LinearDef { start: DVec3::new(0.0, 5.0, 2.8), end: DVec3::new(4.0, 5.0, 2.8), profile: ProfileDef::Rectangle { width: 0.2, height: 0.4 }, roll: 0.0, role: Default::default() });
        let b = app.add_element("Beam", Some(ground), beam).unwrap();
        assert!(app.top_choices(app.modeller.model.get(b).unwrap(), openaec_ui::Language::English).is_empty());
    }

    #[test]
    fn the_wall_tool_reaches_the_level_above() {
        use crate::app::modeller::ModelTool;
        use crate::app::Message;
        let (mut app, _ground, first) = two_levels();
        let _ = app.update(Message::ModelTool(Some((ModelTool::Wall, "Wall"))));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 4.0, 0.0);
        let placed = app.modeller.model.iter().next().unwrap().clone();
        assert_eq!(placed.top, Some(Top { level: first.clone(), offset: 0.0 }), "up to Level 1");
        // on the highest level there is none above: a height of its own
        let _ = app.update(Message::SelectTool);
        app.step_level(true);
        let _ = app.update(Message::ModelTool(Some((ModelTool::Wall, "Wall"))));
        click(&mut app, 0.0, 5.0);
        click(&mut app, 4.0, 5.0);
        let upper = app.modeller.model.iter().find(|e| e.level.as_deref() == Some(first.as_str())).unwrap();
        assert_eq!(upper.top, None);
    }

    #[test]
    fn removing_a_level_frees_the_tops_that_reach_it() {
        let (mut app, ground, first) = two_levels();
        let w = wall(&mut app, &ground, 2.0);
        app.set_top(w, Some(Top { level: first.clone(), offset: 0.0 })).unwrap();
        let _ = app.update(crate::app::Message::RemoveLevel(std::sync::Arc::from(first.as_str())));
        assert!(app.document.level(&first).is_none());
        assert_eq!(app.modeller.model.get(w).unwrap().top, None);
        assert!(close(stands(&app, w), (0.0, 3.0)), "its height kept");
    }
}
