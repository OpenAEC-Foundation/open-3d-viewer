//! The explorer's tabs and its Levels tab: the levels of the project document (IfcX storeys),
//! high to low, a form to add one and a button to remove a level that was added. Floor plans and
//! sheets become further tabs.

use super::{App, Message, FONT, FONT_BOLD};
use crate::document::{parse_elevation, Document, LevelError};
use crate::icons;
use crate::theme::Palette;
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, rule, scrollable, svg, text, text_input, Column};
use iced::{Center, Color, Element, Fill, Padding};
use openaec_ui::Language;
use std::sync::Arc;

/// The tabs of the explorer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExplorerTab {
    /// Models, views, schedules, sheets, rooms and layers.
    #[default]
    Model,
    Levels,
    /// Floor plans (and further views) of the document.
    Views,
}

/// The form of a new level, as typed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LevelForm {
    pub name: String,
    pub elevation: String,
    /// Why the last Add was refused.
    pub error: Option<LevelError>,
}

impl App {
    /// A new project: the document of the model only.
    pub(super) fn new_document(&mut self) {
        self.document = Document::default();
        self.level_form = None;
        self.cuts.clear();
    }

    /// Opens the form of a new level, filled with the next free name (`word` and a number) and
    /// an elevation above the highest level.
    pub(super) fn new_level(&mut self, word: &str) {
        let (name, elevation) = self.document.next_level(word);
        self.level_form = Some(LevelForm { name, elevation: format!("{elevation:.3}"), error: None });
    }

    /// Adds the level of the form; a refusal stays in the form.
    pub(super) fn add_level(&mut self) {
        let Some(form) = &mut self.level_form else { return };
        let added = match parse_elevation(&form.elevation) {
            Some(elevation) => self.document.add_level(&form.name, elevation),
            None => Err(LevelError::BadElevation),
        };
        match added {
            Ok(_) => self.level_form = None,
            Err(e) => form.error = Some(e),
        }
    }

    /// The explorer pane: its tabs, then the tab shown.
    pub fn explorer_body(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let tab = |label: &'static str, t: ExplorerTab| {
            let active = self.explorer_tab == t;
            button(text(label).size(12).font(if active { FONT_BOLD } else { FONT }))
                .padding([4, 12])
                .style(move |_, s| p.ribbon_button(active, s))
                .on_press(Message::ExplorerTab(t))
        };
        let tabs = row![
            tab(l.text("Model", "Model"), ExplorerTab::Model),
            tab(l.text("Niveaus", "Levels"), ExplorerTab::Levels),
            tab(l.text("Weergaven", "Views"), ExplorerTab::Views),
        ]
        .spacing(2);
        let body = match self.explorer_tab {
            ExplorerTab::Model => self.layers_body(p, l),
            ExplorerTab::Levels => self.levels_body(p, l),
            ExplorerTab::Views => self.views_body(p, l),
        };
        column![tabs, rule::horizontal(1), body].spacing(6).into()
    }

    /// The Levels tab.
    fn levels_body(&self, p: Palette, l: Language) -> Element<'_, Message> {
        if self.model.is_none() {
            return text(l.text("Geen model geopend", "No model loaded")).size(12).color(p.text_muted).into();
        }
        let levels = self.document.levels();
        let mut body = Column::new().spacing(1);
        body = body.push(
            container(
                row![
                    text(l.text("Niveaus", "Levels")).size(12).font(FONT_BOLD).width(Fill),
                    text(levels.len().to_string()).size(11).color(p.text_secondary),
                ]
                .spacing(6)
                .align_y(Center),
            )
            .padding([4, 6])
            .style(move |_| p.panel_header()),
        );
        match &self.level_form {
            Some(form) => body = body.push(level_form(p, l, form)),
            None => {
                let plus = svg(icons::handle(icons::ADD)).width(14).height(14).style(move |_, _| svg::Style { color: Some(p.accent) });
                let label = text(l.text("Niveau toevoegen", "Add level")).size(12).color(p.text);
                body = body.push(
                    button(row![plus, label].spacing(6).align_y(Center))
                        .padding([5, 8])
                        .width(Fill)
                        .style(move |_, s| p.row_button(s))
                        .on_press(Message::NewLevel(l.text("Niveau", "Level"))),
                );
            }
        }
        if levels.is_empty() && self.level_form.is_none() {
            let none = l.text("Dit model heeft geen niveaus. Voeg er een toe.", "This model has no levels. Add one.");
            body = body.push(container(text(none).size(12).color(p.text_muted).wrapping(Wrapping::WordOrGlyph)).padding([4, 8]));
        }
        // high to low, as the levels stand in the building
        for level in levels.iter().rev() {
            let name = column![text(level.name.clone()).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph)]
                .push(level.added.then(|| text(l.text("toegevoegd", "added")).size(10).color(p.accent)))
                .width(Fill);
            let elevation = text(format!("{:+.3} m", level.elevation)).size(11).color(p.text_secondary);
            let mut item = row![name, elevation].spacing(6).align_y(Center);
            if level.added {
                let close = svg(svg::Handle::from_memory(openaec_ui::icons::Icon::Close.bytes()))
                    .width(12)
                    .height(12)
                    .style(move |_, _| svg::Style { color: Some(p.text_secondary) });
                item = item.push(
                    button(close).padding(3).style(move |_, s| p.row_button(s)).on_press(Message::RemoveLevel(Arc::from(level.path.as_str()))),
                );
            }
            body = body.push(container(item).padding([4, 8]).width(Fill));
        }
        scrollable(body.padding(Padding { top: 0.0, right: 4.0, bottom: 8.0, left: 0.0 })).height(Fill).spacing(4).into()
    }
}

/// The fields of a new level, Add and Cancel, and why the last Add was refused.
fn level_form<'a>(p: Palette, l: Language, form: &'a LevelForm) -> Element<'a, Message> {
    let label = |s: &'static str| text(s).size(11).color(p.text_secondary);
    let mut fields = column![
        label(l.text("Naam", "Name")),
        text_input("", &form.name).on_input(Message::LevelName).on_submit(Message::AddLevel).size(12).padding([5, 8]),
        label(l.text("Hoogte (m)", "Elevation (m)")),
        text_input("0.000", &form.elevation).on_input(Message::LevelElevation).on_submit(Message::AddLevel).size(12).padding([5, 8]),
    ]
    .spacing(4);
    if let Some(error) = form.error {
        let why = match error {
            LevelError::EmptyName => l.text("Geef het niveau een naam.", "Give the level a name."),
            LevelError::DuplicateName => l.text("Er is al een niveau met deze naam.", "A level with this name exists."),
            LevelError::BadElevation => l.text("De hoogte is geen getal.", "The elevation is not a number."),
        };
        let red = if p.dark { Color::from_rgb8(0xF8, 0x71, 0x71) } else { Color::from_rgb8(0xDC, 0x26, 0x26) };
        fields = fields.push(text(why).size(11).color(red).wrapping(Wrapping::WordOrGlyph));
    }
    let add = button(text(l.text("Toevoegen", "Add")).size(12)).padding([4, 12]).style(move |_, s| p.primary_button(s)).on_press(Message::AddLevel);
    let cancel = button(text(l.text("Annuleren", "Cancel")).size(12)).padding([4, 12]).style(move |_, s| p.ribbon_button(false, s)).on_press(Message::CancelLevel);
    fields = fields.push(row![add, cancel].spacing(6));
    container(fields).padding([6, 8]).width(Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Model;
    use std::path::PathBuf;

    fn simulator(app: &App) -> iced_test::Simulator<'_, Message> {
        iced_test::Simulator::with_size(iced::Settings::default(), iced::Size::new(1400.0, 900.0), app.view())
    }

    fn model(levels: &[(&str, f32)]) -> Arc<Model> {
        let mut model = Model::new(PathBuf::from("house.rvt"), "Revit", Vec::new());
        model.project.levels = levels.iter().map(|(n, z)| (n.to_string(), *z)).collect();
        Arc::new(model)
    }

    fn names(app: &App) -> Vec<&str> {
        app.document.levels().iter().map(|l| l.name.as_str()).collect()
    }

    /// Clicks `label` and hands the messages it sent to the app.
    fn click(app: &mut App, label: &str) -> Vec<Message> {
        let mut ui = simulator(app);
        ui.click(label).unwrap_or_else(|_| panic!("no {label}"));
        let messages: Vec<Message> = ui.into_messages().collect();
        for m in messages.clone() {
            let _ = app.update(m);
        }
        messages
    }

    #[test]
    fn the_explorer_has_a_levels_tab() {
        let mut app = App::default();
        app.set_model(model(&[]));
        let sent = click(&mut app, "Levels");
        assert!(matches!(sent[..], [Message::ExplorerTab(ExplorerTab::Levels)]), "{sent:?}");
        assert!(simulator(&app).find("This model has no levels. Add one.").is_ok());
        click(&mut app, "Model");
        assert_eq!(app.explorer_tab, ExplorerTab::Model);
    }

    #[test]
    fn lists_the_models_levels_as_ifcx_levels() {
        let mut app = App::default();
        app.set_model(model(&[("00 ground floor", 0.0), ("01 first floor", 3.0)]));
        let _ = app.update(Message::ExplorerTab(ExplorerTab::Levels));
        assert_eq!(names(&app), ["00 ground floor", "01 first floor"]);
        let mut ui = simulator(&app);
        for shown in ["01 first floor", "+3.000 m", "00 ground floor", "+0.000 m", "Add level"] {
            assert!(ui.find(shown).is_ok(), "{shown}");
        }
    }

    /// The levels have their own tab: the properties pane does not list them as well.
    #[test]
    fn the_properties_pane_leaves_the_levels_to_their_tab() {
        let mut app = App::default();
        app.set_model(model(&[("00 ground floor", 0.0), ("01 first floor", 3.0)]));
        assert_eq!(app.explorer_tab, ExplorerTab::Model);
        let mut ui = simulator(&app);
        for hidden in ["01 first floor", "+3.000 m", "LEVELS"] {
            assert!(ui.find(hidden).is_err(), "{hidden}");
        }
    }

    #[test]
    fn adds_a_level_with_the_form() {
        let mut app = App::default();
        app.set_model(model(&[("Level 0", 0.0), ("Level 1", 3.0)]));
        let _ = app.update(Message::ExplorerTab(ExplorerTab::Levels));
        click(&mut app, "Add level");
        // filled in: the next name, 3 m above the highest level
        assert_eq!(app.level_form, Some(LevelForm { name: "Level 2".into(), elevation: "6.000".into(), error: None }));
        let _ = app.update(Message::LevelName("Roof".into()));
        let _ = app.update(Message::LevelElevation("7,25".into()));
        click(&mut app, "Add");
        assert_eq!(app.level_form, None);
        assert_eq!(names(&app), ["Level 0", "Level 1", "Roof"]);
        let roof = &app.document.levels()[2];
        assert!(roof.added);
        assert_eq!(roof.elevation, 7.25);
        let mut ui = simulator(&app);
        assert!(ui.find("added").is_ok() && ui.find("+7.250 m").is_ok());
    }

    #[test]
    fn a_refused_level_says_why_and_cancel_closes_the_form() {
        let mut app = App::default();
        app.set_model(model(&[("Level 0", 0.0)]));
        let _ = app.update(Message::ExplorerTab(ExplorerTab::Levels));
        let _ = app.update(Message::NewLevel("Level"));
        let _ = app.update(Message::LevelName("level 0".into()));
        let _ = app.update(Message::AddLevel);
        assert_eq!(app.level_form.as_ref().and_then(|f| f.error), Some(LevelError::DuplicateName));
        assert!(simulator(&app).find("A level with this name exists.").is_ok());
        let _ = app.update(Message::LevelName("Level 1".into()));
        let _ = app.update(Message::LevelElevation("high".into()));
        let _ = app.update(Message::AddLevel);
        assert!(simulator(&app).find("The elevation is not a number.").is_ok());
        click(&mut app, "Cancel");
        assert_eq!(app.level_form, None);
        assert_eq!(names(&app), ["Level 0"]);
    }

    #[test]
    fn only_added_levels_can_be_removed() {
        let mut app = App::default();
        app.set_model(model(&[("00", 0.0)]));
        let _ = app.update(Message::NewLevel("Level"));
        let _ = app.update(Message::AddLevel);
        let [own, added] = [0, 1].map(|i| Arc::<str>::from(app.document.levels()[i].path.as_str()));
        let _ = app.update(Message::RemoveLevel(own));
        let _ = app.update(Message::RemoveLevel(added));
        assert_eq!(names(&app), ["00"]);
    }

    #[test]
    fn added_levels_stay_when_a_model_is_added_and_go_when_another_is_opened() {
        let mut app = App::default();
        app.set_model(model(&[("00", 0.0)]));
        app.document.add_level("Roof", 9.0).unwrap();
        app.add_model(model(&[("01", 3.0)]));
        assert_eq!(names(&app), ["00", "01", "Roof"]);
        // a re-layered last model (IFCX) keeps them too
        app.replace_last_model(model(&[("01", 3.0), ("02", 6.0)]));
        assert_eq!(names(&app), ["00", "01", "02", "Roof"]);
        app.set_model(model(&[("A", 1.0)]));
        assert_eq!(names(&app), ["A"]);
    }

    #[test]
    fn saving_as_ifc_writes_the_added_levels() {
        let mut app = App::default();
        let layer = crate::model::Layer {
            name: "Plate".into(),
            positions: vec![0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 2.0, 1.0, 0.0],
            indices: vec![0, 1, 2],
            ..Default::default()
        };
        app.set_model(Arc::new(Model::new(PathBuf::from("plate.obj"), "OBJ", vec![layer])));
        app.document.add_level("Ground", 0.0).unwrap();
        app.document.add_level("Roof", 4.5).unwrap();
        let model = app.model.clone().unwrap();
        let bytes = crate::export::to_bytes(&model, &app.document, "plate.ifc").unwrap();
        let s = String::from_utf8(bytes).unwrap();
        assert_eq!(s.matches("=IFCBUILDINGSTOREY(").count(), 2);
        assert!(s.contains(",'Roof',") && s.contains(",'Ground',"));
    }
}
