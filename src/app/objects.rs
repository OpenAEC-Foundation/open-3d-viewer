//! Objects and levels of development in the Library product.
//!
//! **Objects** (`bim_library::object`): doors and windows, stairs, fittings, furniture and the
//! terminals and appliances of the services, a category per group. A tile says an object's size
//! and its level of development; the right pane shows the picked one: what it is, its size and
//! mounting height, its IFC class, its NL-SfB code and its properties. An object of another
//! library is taken into the project with a button; one of the project is removed with one.
//!
//! **Levels of development** (`bim_library::lod`): a category per level, LOD 200 to LOD 400,
//! with every type of the source at that level: its build-ups, linear types, system types and
//! objects.
//!
//! Not yet: changing an object's fields here, and placing an object of a type in the Modeller.

use super::libraries::{Category, Shelf};
use super::{App, Message, FONT_BOLD};
use crate::theme::Palette;
use bim_library::lod::Lod;
use bim_library::object::ObjectGroup;
use bim_library::{Lang, Library};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, text, Column, Space};
use iced::{Center, Element, Fill};
use openaec_ui::Language;

#[derive(Debug, Clone)]
pub enum ObjectMessage {
    /// Show this object of the source shown.
    Pick(String),
    /// Take this object of another library into the project's.
    Take(Shelf, String),
    /// Remove the picked object from the project's library.
    Remove,
}

/// Which object the right pane shows, and why the last change was refused.
#[derive(Debug, Clone, Default)]
pub struct ObjectView {
    pub picked: Option<String>,
    pub refused: Option<String>,
}

/// A size as it is written: "880 × 40 × 2315" (width, depth, height in millimetres).
pub fn size_text(size: [f64; 3]) -> String {
    let mm = |v: f64| bim_library::millimetres(v, Lang::En);
    format!("{} × {} × {}", mm(size[0]), mm(size[1]), mm(size[2]))
}

/// An object of `from` taken into `library`, with the material it is made of; its id. An object
/// the library has is not taken twice.
pub fn take(library: &mut Library, from: &Library, id: &str) -> Result<String, bim_library::LibraryError> {
    let object = from.object_type(id).ok_or(bim_library::LibraryError::NoKey)?.clone();
    if let Some(material) = object.material.as_deref().filter(|key| library.material(key).is_none()).and_then(|key| from.material(key)) {
        library.add_material(material.clone())?;
    }
    library.add_object_type(object)?;
    Ok(id.to_string())
}

/// The types of `library` at `lod`, each with the kind of thing it is: its name, its kind and
/// the category it is shown in.
pub fn at_level(library: &Library, lod: Lod, lang: Lang, l: Language) -> Vec<(String, &'static str, Category)> {
    let mut out = Vec::new();
    for b in library.buildups.iter().filter(|b| b.lod() == lod) {
        out.push((library.name(&b.id, lang).unwrap_or_default(), super::buildups::role_title(b.role, l), Category::Planar(b.role)));
    }
    for t in library.linear_types.iter().filter(|t| t.lod() == lod) {
        out.push((library.linear_name(&t.id, lang).unwrap_or_default(), Category::Linear(t.role).label(l), Category::Linear(t.role)));
    }
    for t in library.system_types.iter().filter(|t| t.lod() == lod) {
        out.push((library.system_name(&t.id, lang).unwrap_or_default(), Category::Systems(t.group).label(l), Category::Systems(t.group)));
    }
    for o in library.object_types.iter().filter(|o| o.lod == lod) {
        out.push((o.name(lang).to_string(), o.group.name(lang), Category::Objects(o.group)));
    }
    out
}

impl App {
    pub(super) fn object_update(&mut self, message: ObjectMessage) {
        match message {
            ObjectMessage::Pick(id) => self.object_view = ObjectView { picked: Some(id), refused: None },
            ObjectMessage::Take(shelf, id) => {
                let from = self.shelf(shelf).clone();
                match take(&mut self.library, &from, &id) {
                    Ok(id) => {
                        let group = self.library.object_type(&id).map(|o| o.group);
                        self.object_view = ObjectView { picked: Some(id), refused: None };
                        if let Some(group) = group {
                            self.show_in_project(Category::Objects(group));
                        }
                    }
                    Err(why) => self.object_view.refused = Some(why.to_string()),
                }
            }
            ObjectMessage::Remove => {
                if let Some(id) = self.object_view.picked.take() {
                    self.library.object_types.retain(|o| o.id != id);
                }
            }
        }
    }

    /// The objects of `group` of the source shown as tiles: an object's size as its look, its
    /// name, and its level with its IFC class.
    pub(super) fn object_tiles<'a>(&'a self, p: Palette, l: Language, library: &'a Library, group: ObjectGroup) -> Vec<Element<'a, Message>> {
        let lang = super::library::lang(l);
        library
            .object_types_of(group)
            .map(|object| {
                let look = container(text(size_text(object.size)).size(13).color(p.text)).width(Fill).align_x(Center);
                let about = format!("{} · {}", object.lod.name(), object.ifc().0);
                let on = self.object_view.picked.as_deref() == Some(object.id.as_str());
                super::libraries::tile(p, look.into(), object.name(lang).to_string(), about, on, Some(Message::Object(ObjectMessage::Pick(object.id.clone()))))
            })
            .collect()
    }

    /// Every type of the source shown at `lod` as tiles: its name, and the kind of thing it is.
    pub(super) fn lod_tiles<'a>(&'a self, p: Palette, l: Language, library: &'a Library, lod: Lod) -> Vec<Element<'a, Message>> {
        let lang = super::library::lang(l);
        at_level(library, lod, lang, l)
            .into_iter()
            .map(|(name, kind, category)| {
                let look = container(text(lod.number().to_string()).size(22).color(p.text_secondary)).width(Fill).align_x(Center);
                super::libraries::tile(p, look.into(), name, kind.to_string(), false, Some(Message::Libraries(super::libraries::LibrariesMessage::Category(category))))
            })
            .collect()
    }

    /// The right pane for the objects: the picked one of `library`, on `shelf`.
    pub fn object_pane<'a>(&'a self, p: Palette, l: Language, library: &'a Library, shelf: Shelf) -> Element<'a, Message> {
        let lang = super::library::lang(l);
        let Some(object) = self.object_view.picked.as_deref().and_then(|id| library.object_type(id)) else {
            let hint = l.text("Klik een object om te zien wat het is.", "Click an object to see what it is.");
            return container(text(hint).size(12).color(p.text_muted).wrapping(Wrapping::Word)).padding(8).into();
        };
        let line = |label: &'static str, value: String| row![text(label).size(12).color(p.text_secondary).width(iced::Length::FillPortion(2)), text(value).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph).width(iced::Length::FillPortion(3))].spacing(8);
        let (class, _, predefined) = object.ifc();
        let (says_nl, says_en) = object.lod.says();
        let code = object.nlsfb();
        let mut pane: Column<'_, Message> = column![
            text(object.name(lang).to_string()).size(14).font(FONT_BOLD).color(p.text).wrapping(Wrapping::Word),
            text(format!("{} · {}", object.group.trade().name(lang), object.group.name(lang))).size(11).color(p.text_muted),
            line(l.text("Maat (b × d × h)", "Size (w × d × h)"), format!("{} mm", size_text(object.size))),
            line(l.text("Onderkant boven vloer", "Underside above floor"), format!("{} mm", bim_library::millimetres(object.base, Lang::En))),
            line("LOD", format!("{} · {}", object.lod.number(), l.text(says_nl, says_en))),
            line("IFC", format!("{class} · {predefined}")),
            line("NL-SfB", super::codes::named(library, &code, lang)),
        ]
        .spacing(4);
        if let Some(material) = object.material.as_deref() {
            pane = pane.push(line(l.text("Materiaal", "Material"), library.material(material).map_or(material, |m| m.name(lang)).to_string()));
        }
        for (key, value) in &object.properties {
            pane = pane.push(row![text(key.clone()).size(12).color(p.text_secondary).width(iced::Length::FillPortion(2)), text(value.clone()).size(12).color(p.text).width(iced::Length::FillPortion(3))].spacing(8));
        }
        let act = |label: &'static str, message: ObjectMessage| button(text(label).size(12).color(p.text_secondary)).padding([3, 8]).style(move |_, s| p.row_button(s)).on_press(Message::Object(message));
        let action = if shelf == Shelf::Project { act(l.text("Verwijderen", "Remove"), ObjectMessage::Remove) } else { act(l.text("Naar het project", "Take into the project"), ObjectMessage::Take(shelf, object.id.clone())) };
        pane = pane.push(row![action, Space::new().width(Fill)]);
        let refused = self.object_view.refused.as_ref().map(|why| text(format!("{} ({why})", l.text("Niet overgenomen.", "Not taken."))).size(11).color(p.accent).wrapping(Wrapping::Word));
        container(pane.push(refused)).padding(8).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bim_library::object::ObjectType;
    use bim_library::Names;

    #[test]
    fn the_library_shows_its_objects_by_group_and_the_pane_the_picked_one() {
        let mut app = App::default();
        let p = app.theme.palette();
        // the starter has a generic object of each group
        for group in ObjectGroup::ALL {
            assert_eq!(app.object_tiles(p, Language::English, &app.library, group).len(), 1, "{group:?}");
        }
        assert_eq!(size_text([0.88, 0.04, 2.315]), "880 × 40 × 2315");
        app.object_update(ObjectMessage::Pick("object.luminaire".into()));
        let mut pane = crate::app::test_ui_of(iced::Size::new(340.0, 900.0), app.object_pane(p, Language::English, &app.library, Shelf::Project));
        assert!(pane.find("luminaire").is_ok() && pane.find("electrical and lighting · luminaires").is_ok() && pane.find("600 × 600 × 80 mm").is_ok());
        assert!(pane.find("200 · generic element, approximate").is_ok() && pane.find("IfcLightFixture · POINTSOURCE").is_ok() && pane.find("63 · lighting").is_ok() && pane.find("Remove").is_ok());
    }

    #[test]
    fn an_object_is_taken_into_the_project_and_removed() {
        let mut app = App::default();
        let mut other = Library::starter();
        let mut valve = ObjectType::new("nl.air-terminal.extract-valve-125", ObjectGroup::AirTerminal, Names::new("afzuigventiel Ø125", "extract valve Ø125", "Abluftventil Ø125", "bouche d'extraction Ø125"), [0.155, 0.155, 0.05]);
        valve.lod = Lod::L300;
        other.add_object_type(valve).unwrap();
        let before = app.library.object_types.len();
        assert_eq!(take(&mut app.library, &other, "nl.air-terminal.extract-valve-125").as_deref(), Ok("nl.air-terminal.extract-valve-125"));
        assert_eq!(app.library.object_types.len(), before + 1);
        // not twice, and not what the other library lacks
        assert!(take(&mut app.library, &other, "nl.air-terminal.extract-valve-125").is_err() && take(&mut app.library, &other, "none").is_err());
        app.object_update(ObjectMessage::Pick("nl.air-terminal.extract-valve-125".into()));
        app.object_update(ObjectMessage::Remove);
        assert_eq!((app.library.object_types.len(), app.object_view.picked.clone()), (before, None));
    }

    #[test]
    fn every_type_of_a_library_is_at_one_level() {
        let app = App::default();
        let l = Language::English;
        let library = &app.library;
        let counts: Vec<usize> = Lod::ALL.iter().map(|lod| at_level(library, *lod, Lang::En, l).len()).collect();
        assert_eq!(counts.iter().sum::<usize>(), library.buildups.len() + library.linear_types.len() + library.system_types.len() + library.object_types.len());
        assert_eq!(counts, bim_library::remote::levels(library));
        // the generic objects are LOD 200; a cavity wall is LOD 300; a system with its members LOD 350
        let at = |lod: Lod| at_level(library, lod, Lang::En, l);
        assert!(at(Lod::L200).iter().any(|(name, kind, _)| name == "luminaire" && *kind == "luminaires"));
        assert!(at(Lod::L300).iter().any(|(name, _, category)| name.starts_with("Brickwork-air cavity") && *category == Category::Planar(bim_library::Role::Wall)));
        assert!(!at(Lod::L350).is_empty());
        assert_eq!(app.lod_tiles(app.theme.palette(), l, library, Lod::L200).len(), counts[0]);
    }
}
