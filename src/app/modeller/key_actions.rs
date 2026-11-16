//! What the Modeller's keys do (see [`super::keys`]): each the command of the ribbon it stands for.

use super::keys::ModelKey;
use super::ModelTool;
use crate::app::transform::TransformKind;
use crate::app::{App, Message};
use openaec_ui::Language;

impl ModelKey {
    /// What it does, as the ribbon names it.
    pub fn label(self, l: Language) -> &'static str {
        match self {
            ModelKey::Select => l.text("Selecteren", "Select"),
            ModelKey::Wall => ModelTool::Wall.label(l),
            ModelKey::Slab => ModelTool::Slab.label(l),
            ModelKey::Column => ModelTool::Column.label(l),
            ModelKey::Beam => ModelTool::Beam.label(l),
            ModelKey::System => ModelTool::System.label(l),
            ModelKey::Object => ModelTool::Point.label(l),
            ModelKey::Door => ModelTool::Door.label(l),
            ModelKey::Window => ModelTool::Window.label(l),
            ModelKey::Grid => l.text("Stramien", "Grid"),
            ModelKey::Section => l.text("Doorsnede", "Section"),
            ModelKey::Dimension => l.text("Maatvoering", "Dimension"),
            ModelKey::Text => l.text("Tekst", "Text"),
            ModelKey::DrawLine => l.text("Lijn", "Line"),
            ModelKey::DrawRectangle => l.text("Rechthoek", "Rectangle"),
            ModelKey::DrawCircle => l.text("Cirkel", "Circle"),
            ModelKey::CreateSimilar => l.text("Nog zo een maken", "Create similar"),
            ModelKey::Grab => l.text("Pakken: de selectie volgt de muis", "Grab: the selection follows the mouse"),
            ModelKey::Move => TransformKind::Move.label(l),
            ModelKey::Copy => TransformKind::Copy.label(l),
            ModelKey::Rotate => TransformKind::Rotate.label(l),
            ModelKey::Mirror => TransformKind::Mirror.label(l),
            ModelKey::Delete => l.text("Verwijderen", "Delete"),
            ModelKey::LevelUp => l.text("Niveau hoger", "Level up"),
            ModelKey::LevelDown => l.text("Niveau lager", "Level down"),
        }
    }
}

/// The Modeller's keys for the shortcuts page: each letter, pair and page key with what it does.
pub fn key_list<'a, M: 'a>(look: openaec_ui::design::Look, l: Language) -> iced::Element<'a, M> {
    use openaec_ui::design::widgets as w;
    let mut rows = iced::widget::column![w::mono_label(look, "MODELLER")].spacing(4);
    let keys = super::keys::SINGLE.iter().map(|(c, k)| (c.to_ascii_uppercase().to_string(), *k));
    let pairs = super::keys::PAIRS.iter().map(|(p, k)| (p.to_ascii_uppercase(), *k));
    let pages = [("PgUp".to_string(), ModelKey::LevelUp), ("PgDn".to_string(), ModelKey::LevelDown)];
    for (key, command) in keys.chain(pairs).chain(pages) {
        rows = rows.push(w::property_row(look, command.label(l), w::secondary(look, key).into()));
    }
    // the 3D cursor has its own gestures (see `cursor3d`)
    let cursor = [
        (l.text("3D-cursor plaatsen", "Place the 3D cursor"), l.text("Shift + rechtermuisknop", "Shift + right button")),
        (l.text("3D-cursor naar de oorsprong", "3D cursor home"), "Shift+C"),
        (l.text("Roteren om de 3D-cursor", "Rotate about the 3D cursor"), "RO, Enter"),
    ];
    for (what, key) in cursor {
        rows = rows.push(w::property_row(look, what, w::secondary(look, key).into()));
    }
    rows.into()
}

impl App {
    /// Steps the level worked on to the next one up (or down); with a floor plan shown, that
    /// level's floor plan opens, made when it has none. At the highest (lowest) level nothing
    /// changes.
    pub(in crate::app) fn step_level(&mut self, up: bool) {
        let (_, _, now) = self.work_plane();
        let levels = self.document.levels();
        let at = now.as_deref().and_then(|p| levels.iter().position(|l| l.path == p));
        let next = match (at, up) {
            (Some(i), true) => levels.get(i + 1),
            (Some(i), false) => i.checked_sub(1).and_then(|i| levels.get(i)),
            (None, _) => levels.first(),
        };
        let Some(next) = next.map(|l| l.path.clone()) else { return };
        self.modeller.level = Some(next.clone());
        let plan_shown = self.active_plan.as_deref().is_some_and(|p| self.document.plan(p).is_some());
        if plan_shown {
            let _ = self.update(Message::Browser(super::browser::BrowserMessage::Level(std::sync::Arc::from(next.as_str()))));
        }
    }

    /// The command of key `key`, in the interface language `l`.
    pub(in crate::app) fn model_key(&mut self, key: ModelKey, l: Language) {
        let tool = |t: ModelTool| Some((t, t.label(l)));
        match key {
            ModelKey::Select => self.select_tool(),
            ModelKey::Wall => self.set_model_tool(tool(ModelTool::Wall)),
            ModelKey::Slab => self.set_model_tool(tool(ModelTool::Slab)),
            ModelKey::Column => self.set_model_tool(tool(ModelTool::Column)),
            ModelKey::Beam => self.set_model_tool(tool(ModelTool::Beam)),
            ModelKey::System => self.set_model_tool(tool(ModelTool::System)),
            ModelKey::Object => self.set_model_tool(tool(ModelTool::Point)),
            ModelKey::Door => self.set_model_tool(tool(ModelTool::Door)),
            ModelKey::Window => self.set_model_tool(tool(ModelTool::Window)),
            // GR: the G before it grabbed the selection, which goes back where it was
            ModelKey::Grid => {
                self.drop_grab();
                self.toggle_grid_tool();
            }
            ModelKey::Text => {
                let _ = self.update(Message::NoteTool(!self.note_tool));
            }
            ModelKey::CreateSimilar => self.create_similar(l),
            ModelKey::DrawLine | ModelKey::DrawRectangle | ModelKey::DrawCircle => {
                use crate::app::marks::DrawKind;
                let kind = match key {
                    ModelKey::DrawLine => DrawKind::Line,
                    ModelKey::DrawRectangle => DrawKind::Rectangle,
                    _ => DrawKind::Circle,
                };
                let _ = self.update(Message::DrawTool(Some(kind)));
            }
            ModelKey::Section => {
                let _ = self.update(Message::SectionTool(!self.section_tool));
            }
            ModelKey::Dimension => {
                let _ = self.update(Message::DimensionTool(!self.dimension_tool));
            }
            ModelKey::Grab if self.modeller.edit.is_some() => self.edit_key('g'),
            // outside edit mode G grabs the selection, which follows the cursor at once
            ModelKey::Grab => {
                self.begin_grab_objects();
            }
            ModelKey::Move => self.begin_transform(TransformKind::Move),
            ModelKey::Copy => self.begin_transform(TransformKind::Copy),
            ModelKey::Rotate => self.begin_transform(TransformKind::Rotate),
            ModelKey::Mirror => self.begin_transform(TransformKind::Mirror),
            ModelKey::Delete => {
                let _ = self.update(Message::DeletePicked);
            }
            ModelKey::LevelUp => self.step_level(true),
            ModelKey::LevelDown => self.step_level(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::ModelTool;
    use crate::app::App;
    use crate::product::Product;
    use crate::shell::{Msg, Studio};
    use iced::keyboard::{key::Named, Key, Modifiers};
    use openaec_ui::keyboard::KeyPress;

    /// The Studio in the Modeller with a new model.
    fn modelling_studio() -> Studio {
        let mut studio = Studio::new(App::default());
        studio.set_product(Product::Modeller);
        studio.app.new_model("Level 0");
        studio
    }

    fn press_with(studio: &mut Studio, key: &str, modifiers: Modifiers) {
        let _ = studio.update(Msg::Key(KeyPress { key: Key::Character(key.into()), modifiers }));
    }

    fn press(studio: &mut Studio, key: &str) {
        press_with(studio, key, Modifiers::empty());
    }

    fn escape(studio: &mut Studio) {
        let _ = studio.update(Msg::Key(KeyPress { key: Key::Named(Named::Escape), modifiers: Modifiers::empty() }));
    }

    #[test]
    fn letters_start_the_modellers_tools() {
        let mut studio = modelling_studio();
        press(&mut studio, "w");
        press(&mut studio, "a");
        assert_eq!(studio.app.modeller.tool.map(|t| t.0), Some(ModelTool::Wall));
        press(&mut studio, "c");
        press(&mut studio, "l");
        assert_eq!(studio.app.modeller.tool.map(|t| t.0), Some(ModelTool::Column));
        // a letter that waits does what it does alone once its time is up
        press(&mut studio, "d");
        assert!(studio.keys.pending().is_some());
        studio.expire_keys(web_time::Instant::now() + web_time::Duration::from_secs(1));
        assert_eq!(studio.app.modeller.tool.map(|t| t.0), Some(ModelTool::Door));
        press(&mut studio, "s");
        assert!(studio.app.modeller.tool.is_none(), "s selects");
    }

    #[test]
    fn cs_starts_the_tool_of_the_element_selected_with_its_sizes_and_gr_the_grid_tool() {
        let mut studio = modelling_studio();
        let form = bim_core::element::PlanarForm::Wall { start: glam::DVec2::ZERO, end: glam::DVec2::new(4.0, 0.0), base: 0.0, height: 2.4 };
        let wall = studio.app.add_element("Wall", None, bim_core::Shape::Planar(bim_core::PlanarDef { form, thickness: 0.35, openings: Vec::new(), build_up: None })).unwrap();
        // nothing selected: CS does nothing
        press(&mut studio, "c");
        press(&mut studio, "s");
        assert!(studio.app.modeller.tool.is_none());
        studio.app.select_modelled(wall);
        press(&mut studio, "c");
        press(&mut studio, "s");
        assert_eq!(studio.app.modeller.tool.map(|t| t.0), Some(ModelTool::Wall));
        let s = studio.app.modeller.placement.of(ModelTool::Wall);
        assert_eq!((s.width, s.height), (0.35, Some(2.4)), "the next wall is as thick and as high");
        // GR with the wall selected: G grabs it, R lets it go where it was and starts the grid
        escape(&mut studio);
        studio.app.select_modelled(wall);
        let before = studio.app.modeller.model.get(wall).cloned();
        press(&mut studio, "g");
        assert!(studio.app.modeller.grab.is_some());
        press(&mut studio, "r");
        assert!(studio.app.modeller.grab.is_none() && studio.app.modeller.grid_tool);
        assert_eq!(studio.app.modeller.model.get(wall).cloned(), before);
    }

    #[test]
    fn the_viewer_keeps_its_keys_and_ctrl_its_shortcuts() {
        let mut studio = modelling_studio();
        studio.set_product(Product::Viewer);
        press(&mut studio, "i");
        assert!(!studio.app.modeller.grid_tool, "i is no key of the Viewer");
        studio.set_product(Product::Modeller);
        // Ctrl or Alt with a letter is not a Modeller key
        press_with(&mut studio, "i", Modifiers::CTRL);
        press_with(&mut studio, "i", Modifiers::ALT);
        assert!(!studio.app.modeller.grid_tool);
        assert!(studio.app.modeller.tool.is_none());
    }

    #[test]
    fn the_apps_own_letters_still_work_in_the_modeller() {
        let mut studio = modelling_studio();
        studio.app.camera.ortho = true;
        // p: perspective, a shortcut of the app the Modeller has no use for
        press(&mut studio, "p");
        assert!(!studio.app.camera.ortho);
        // f: fit, after the Modeller waited for fl (a wall to fit to)
        let form = bim_core::element::PlanarForm::Wall { start: glam::DVec2::ZERO, end: glam::DVec2::new(4.0, 0.0), base: 0.0, height: 3.0 };
        studio.app.add_element("Wall", None, bim_core::Shape::Planar(bim_core::PlanarDef { form, thickness: 0.2, openings: Vec::new(), build_up: None })).unwrap();
        studio.app.camera.distance = 1.0e6;
        press(&mut studio, "f");
        studio.expire_keys(web_time::Instant::now() + web_time::Duration::from_secs(1));
        assert!(studio.app.camera.distance < 1.0e6, "fitted");
    }

    #[test]
    fn in_edit_mode_letters_start_no_tool_and_select_leaves_it() {
        let mut studio = modelling_studio();
        let form = bim_core::element::PlanarForm::Wall { start: glam::DVec2::ZERO, end: glam::DVec2::new(4.0, 0.0), base: 0.0, height: 3.0 };
        let wall = studio.app.add_element("Wall", None, bim_core::Shape::Planar(bim_core::PlanarDef { form, thickness: 0.2, openings: Vec::new(), build_up: None })).unwrap();
        studio.app.select_modelled(wall);
        let _ = studio.update(Msg::App(crate::app::Message::EditMode));
        assert!(studio.app.modeller.edit.is_some());
        press(&mut studio, "w");
        press(&mut studio, "a");
        assert!(studio.app.modeller.tool.is_none(), "no Wall tool while editing");
        let _ = studio.update(Msg::App(crate::app::Message::SelectTool));
        assert!(studio.app.modeller.edit.is_none() && studio.app.selecting(), "Select leaves edit mode");
    }

    #[test]
    fn digits_are_a_distance_only_while_a_point_is_grabbed() {
        let mut studio = modelling_studio();
        let form = bim_core::element::PlanarForm::Wall { start: glam::DVec2::ZERO, end: glam::DVec2::new(4.0, 0.0), base: 0.0, height: 3.0 };
        let wall = studio.app.add_element("Wall", None, bim_core::Shape::Planar(bim_core::PlanarDef { form, thickness: 0.2, openings: Vec::new(), build_up: None })).unwrap();
        // no grab: 1 is the camera's view from the top
        let yaw = studio.app.camera.yaw;
        studio.app.camera.pitch = 0.3;
        press(&mut studio, "1");
        assert!(studio.app.typed_distance().is_none());
        assert!(studio.app.anim.is_some() || studio.app.camera.pitch != 0.3 || studio.app.camera.yaw != yaw, "the view turns");
        // a point grabbed in edit mode: digits type its distance
        studio.app.select_modelled(wall);
        let _ = studio.update(Msg::App(crate::app::Message::EditMode));
        let _ = studio.update(Msg::App(crate::app::Message::Edit(crate::app::edit::EditEvent::Press { index: 1, add: false })));
        let _ = studio.update(Msg::App(crate::app::Message::Edit(crate::app::edit::EditEvent::Release)));
        press(&mut studio, "g");
        press(&mut studio, "x");
        press(&mut studio, "1");
        press(&mut studio, ",");
        press(&mut studio, "5");
        assert_eq!(studio.app.typed_distance(), Some("1,5"));
    }

    #[test]
    fn esc_drops_a_pending_letter_only() {
        let mut studio = modelling_studio();
        press(&mut studio, "w");
        press(&mut studio, "m");
        assert_eq!(studio.keys.pending(), Some('m'));
        escape(&mut studio);
        assert_eq!(studio.keys.pending(), None);
        assert!(studio.app.modeller.tool.is_some(), "the Wall tool stays on");
    }
}

#[cfg(test)]
mod level_tests {
    use super::super::tests::modelling;
    use crate::app::Message;

    #[test]
    fn page_up_and_down_step_through_the_levels() {
        let mut app = modelling();
        app.modeller.in_modeller = true;
        let _ = app.document.add_level("Level 1", 3.0);
        app.step_level(true);
        assert_eq!(app.work_plane().0, 3.0);
        app.step_level(true);
        assert_eq!(app.work_plane().0, 3.0, "no level above the highest");
        app.step_level(false);
        app.step_level(false);
        assert_eq!(app.work_plane().0, 0.0, "none below the lowest");
        // the keys do the same, and the status line names the level
        let _ = app.update(Message::ModelKey(super::ModelKey::LevelUp, openaec_ui::Language::English));
        assert_eq!(app.work_plane().0, 3.0);
        assert!(app.select_hint(openaec_ui::Language::English).unwrap_or_default().contains("Level 1"));
    }

    #[test]
    fn with_a_floor_plan_shown_the_levels_floor_plan_opens() {
        let mut app = modelling();
        app.modeller.in_modeller = true;
        let _ = app.document.add_level("Level 1", 3.0);
        let ground = app.document.levels()[0].path.clone();
        app.new_plan(&ground);
        app.step_level(true);
        let shown = app.active_plan.clone().and_then(|p| app.document.plan(&p).map(|p| p.level.clone()));
        assert_eq!(shown, Some(app.document.levels()[1].path.clone()), "Level 1's plan, made for it");
        assert_eq!(app.work_plane().0, 3.0);
    }
}
