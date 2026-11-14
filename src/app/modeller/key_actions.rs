//! What the Modeller's keys do (see [`super::keys`]): each the command of the ribbon it stands for.

use super::keys::ModelKey;
use super::ModelTool;
use crate::app::transform::TransformKind;
use crate::app::{App, Message};
use openaec_ui::Language;

impl App {
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
            ModelKey::Grid => self.toggle_grid_tool(),
            ModelKey::Section => {
                let _ = self.update(Message::SectionTool(!self.section_tool));
            }
            ModelKey::Dimension => {
                let _ = self.update(Message::DimensionTool(!self.dimension_tool));
            }
            ModelKey::Grab if self.modeller.edit.is_some() => self.edit_key('g'),
            ModelKey::Grab | ModelKey::Move => self.begin_transform(TransformKind::Move),
            ModelKey::Copy => self.begin_transform(TransformKind::Copy),
            ModelKey::Rotate => self.begin_transform(TransformKind::Rotate),
            ModelKey::Mirror => self.begin_transform(TransformKind::Mirror),
            ModelKey::Delete => {
                let _ = self.update(Message::DeletePicked);
            }
            // the levels are stepped through once the Modeller keeps a level of its own
            ModelKey::LevelUp | ModelKey::LevelDown => {}
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
        assert_eq!(studio.app.modeller.tool.map(|t| t.0), Some(ModelTool::Wall));
        press(&mut studio, "c");
        press(&mut studio, "l");
        assert_eq!(studio.app.modeller.tool.map(|t| t.0), Some(ModelTool::Column));
        // a letter that waits does what it does alone once its time is up
        press(&mut studio, "d");
        assert!(studio.keys.pending().is_some());
        studio.expire_keys(std::time::Instant::now() + std::time::Duration::from_secs(1));
        assert_eq!(studio.app.modeller.tool.map(|t| t.0), Some(ModelTool::Door));
        press(&mut studio, "s");
        assert!(studio.app.modeller.tool.is_none(), "s selects");
    }

    #[test]
    fn the_viewer_keeps_its_keys_and_ctrl_its_shortcuts() {
        let mut studio = modelling_studio();
        studio.set_product(Product::Viewer);
        press(&mut studio, "w");
        assert!(studio.app.modeller.tool.is_none(), "w walks in the Viewer");
        studio.set_product(Product::Modeller);
        // Ctrl or Alt with a letter is not a Modeller key
        press_with(&mut studio, "w", Modifiers::CTRL);
        press_with(&mut studio, "w", Modifiers::ALT);
        assert!(studio.app.modeller.tool.is_none());
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
