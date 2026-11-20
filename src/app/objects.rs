//! Objects and levels of development in the Library product.
//!
//! **Objects** (`bim_library::object`): doors and windows, stairs, fittings, furniture and the
//! terminals and appliances of the services, a category per group. A tile says an object's size
//! and its level of development; the right pane shows the picked one: what it is, its size and
//! mounting height, its IFC class, its NL-SfB code and its properties. An object of another
//! library is taken into the project with a button.
//!
//! An object of the project is edited in the right pane: its name in the library's four
//! languages, its size and mounting height in millimetres, its level of development, its NL-SfB
//! code (its group's by rule, or one of the library's codes), its material, its IFC predefined
//! type and its properties. What is typed is taken as soon as it holds together
//! (`ObjectType::check`); until then the field keeps what was typed and the object what it had.
//! A tile adds a generic object to the group shown; the pane copies or removes the picked one.
//!
//! **Levels of development** (`bim_library::lod`): a category per level, LOD 200 to LOD 400,
//! with every type of the source at that level: its build-ups, linear types, system types,
//! objects, connections and service types.
//!
//! Not yet: placing an object of a type in the Modeller.

use super::libraries::{Category, Shelf};
use super::{App, Message, FONT_BOLD};
use crate::theme::Palette;
use bim_library::lod::Lod;
use bim_library::object::{ObjectGroup, ObjectType};
use bim_library::{Lang, Library, LibraryError};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, pick_list, row, text, text_input, Column, Space};
use iced::{Center, Element, Fill, Length};
use openaec_ui::design::a11y::{self, Role as Aria};
use openaec_ui::Language;

/// A field of an object that is typed into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name(Lang),
    /// Its width (0), depth (1) or height (2), in millimetres.
    Size(usize),
    /// How high its underside is above the floor, in millimetres.
    Base,
    Predefined,
    /// The name of its property at this place.
    Property(usize),
    /// The value of its property at this place.
    Value(usize),
}

#[derive(Debug, Clone)]
pub enum ObjectMessage {
    /// Show this object of the source shown.
    Pick(String),
    /// Take this object of another library into the project's.
    Take(Shelf, String),
    /// Remove the picked object from the project's library.
    Remove,
    /// A generic object more in this group of the project's library.
    Add(ObjectGroup),
    /// A copy of the picked object of the project.
    Copy,
    /// Typed into a field of the picked object of the project.
    Typed(Field, String),
    Level(Lod),
    /// Its own NL-SfB code, or its group's (none).
    Coded(Option<String>),
    /// The material it is of, or none.
    Made(Option<String>),
    /// A property more, to be named.
    AddProperty,
    RemoveProperty(usize),
}

/// Which object the right pane shows, why the last change was refused, and what is typed in a
/// field that does not hold together yet.
#[derive(Debug, Clone, Default)]
pub struct ObjectView {
    pub picked: Option<String>,
    pub refused: Option<String>,
    pub draft: Option<(Field, String)>,
}

/// What is typed as a size: millimetres, with a point or a comma; in metres.
fn metres(typed: &str) -> Option<f64> {
    typed.trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite()).map(|mm| mm / 1000.0)
}

/// The first id of the project's own objects that is free: "project.object.1", and on.
fn fresh(library: &Library) -> String {
    (1..).map(|n| format!("project.object.{n}")).find(|id| library.object_type(id).is_none()).expect("a free id")
}

/// A generic object more in `group`: the group's object of the starter library, at LOD 200,
/// under an id of its own; its id.
pub fn add(library: &mut Library, group: ObjectGroup) -> Result<String, LibraryError> {
    let mut object = bim_library::object::starter().into_iter().find(|o| o.group == group).ok_or(LibraryError::NoKey)?;
    object.id = fresh(library);
    let id = object.id.clone();
    library.add_object_type(object)?;
    Ok(id)
}

/// A copy of object `id` under an id of its own, beside it; its id.
pub fn copy(library: &mut Library, id: &str) -> Result<String, LibraryError> {
    let at = library.object_types.iter().position(|o| o.id == id).ok_or(LibraryError::NoKey)?;
    let mut object = library.object_types[at].clone();
    object.id = fresh(library);
    let fresh = object.id.clone();
    library.object_types.insert(at + 1, object);
    Ok(fresh)
}

/// Object `id` as `change` leaves it, when it still holds together and is of a material the
/// library has; else it stays as it was.
pub fn change(library: &mut Library, id: &str, change: impl FnOnce(&mut ObjectType)) -> Result<(), LibraryError> {
    let at = library.object_types.iter().position(|o| o.id == id).ok_or(LibraryError::NoKey)?;
    let mut object = library.object_types[at].clone();
    change(&mut object);
    object.check()?;
    if let Some(material) = object.material.as_ref().filter(|key| library.material(key).is_none()) {
        return Err(LibraryError::UnknownMaterial(material.clone()));
    }
    library.object_types[at] = object;
    Ok(())
}

/// What is typed into `field` put on `object`; false when it is nothing the field takes (a
/// size that is no number, a predefined type that is no IFC one, a property that is not there).
fn put(object: &mut ObjectType, field: Field, typed: &str) -> bool {
    match field {
        Field::Name(lang) => {
            let name = match lang {
                Lang::Nl => &mut object.names.nl,
                Lang::En => &mut object.names.en,
                Lang::De => &mut object.names.de,
                Lang::Fr => &mut object.names.fr,
            };
            *name = typed.to_string();
        }
        Field::Size(axis) => match (metres(typed), object.size.get_mut(axis)) {
            (Some(value), Some(side)) => *side = value,
            _ => return false,
        },
        Field::Base => match metres(typed) {
            Some(value) => object.base = value,
            None => return false,
        },
        Field::Predefined => {
            let typed = typed.trim().to_uppercase();
            if !typed.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return false;
            }
            object.predefined = (!typed.is_empty()).then_some(typed);
        }
        Field::Property(at) => match object.properties.get_mut(at) {
            Some(property) => property.0 = typed.to_string(),
            None => return false,
        },
        Field::Value(at) => match object.properties.get_mut(at) {
            Some(property) => property.1 = typed.to_string(),
            None => return false,
        },
    }
    true
}

/// What `field` of `object` shows: a size in millimetres, a name as it is.
fn shown(object: &ObjectType, field: Field) -> String {
    let mm = |v: f64| bim_library::millimetres(v, Lang::En);
    match field {
        Field::Name(lang) => object.name(lang).to_string(),
        Field::Size(axis) => object.size.get(axis).copied().map(mm).unwrap_or_default(),
        Field::Base => mm(object.base),
        Field::Predefined => object.predefined.clone().unwrap_or_default(),
        Field::Property(at) => object.properties.get(at).map(|p| p.0.clone()).unwrap_or_default(),
        Field::Value(at) => object.properties.get(at).map(|p| p.1.clone()).unwrap_or_default(),
    }
}

/// A level to pick: "LOD 300".
#[derive(Debug, Clone, Copy, PartialEq)]
struct Level(Lod);

impl std::fmt::Display for Level {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.name())
    }
}

/// A material to pick for an object, or none.
#[derive(Debug, Clone, PartialEq)]
struct Made {
    key: Option<String>,
    label: String,
}

impl std::fmt::Display for Made {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// A field of the pane: its label, and beside it what sets it.
fn field<'a>(p: Palette, label: String, control: Element<'a, ObjectMessage>) -> Element<'a, ObjectMessage> {
    row![text(label).size(12).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph).width(Length::FillPortion(2)), container(control).width(Length::FillPortion(3))].spacing(8).align_y(Center).into()
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
    for c in library.connection_types.iter().filter(|c| c.lod() == lod) {
        out.push((c.name(lang), c.family.name(lang), Category::Connections(c.family)));
    }
    for s in library.service_types.iter().filter(|s| s.lod() == lod) {
        out.push((s.name(lang).to_string(), s.carrier.name(lang), Category::Services(s.carrier)));
    }
    out
}

impl App {
    pub(super) fn object_update(&mut self, message: ObjectMessage) {
        match message {
            ObjectMessage::Pick(id) => self.object_view = ObjectView { picked: Some(id), ..ObjectView::default() },
            ObjectMessage::Take(shelf, id) => {
                let from = self.shelf(shelf).clone();
                match take(&mut self.library, &from, &id) {
                    Ok(id) => {
                        let group = self.library.object_type(&id).map(|o| o.group);
                        self.object_view = ObjectView { picked: Some(id), ..ObjectView::default() };
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
                self.object_view = ObjectView::default();
            }
            ObjectMessage::Add(group) => match add(&mut self.library, group) {
                Ok(id) => {
                    self.object_view = ObjectView { picked: Some(id), ..ObjectView::default() };
                    self.show_in_project(Category::Objects(group));
                }
                Err(why) => self.object_view.refused = Some(why.to_string()),
            },
            ObjectMessage::Copy => {
                if let Some(id) = self.object_view.picked.clone() {
                    match copy(&mut self.library, &id) {
                        Ok(id) => self.object_view = ObjectView { picked: Some(id), ..ObjectView::default() },
                        Err(why) => self.object_view.refused = Some(why.to_string()),
                    }
                }
            }
            ObjectMessage::Typed(field, typed) => {
                let Some(id) = self.object_view.picked.clone() else { return };
                let mut took = true;
                let done = change(&mut self.library, &id, |object| took = put(object, field, &typed));
                // what does not hold together yet stays in the field, the object as it was
                let kept = !took || done.is_err();
                self.object_view.refused = match done {
                    Err(why) if took => Some(why.to_string()),
                    _ => None,
                };
                self.object_view.draft = kept.then_some((field, typed));
            }
            ObjectMessage::Level(lod) => self.object_change(|object| object.lod = lod),
            ObjectMessage::Coded(code) => self.object_change(|object| object.code = code),
            ObjectMessage::Made(material) => self.object_change(|object| object.material = material),
            ObjectMessage::AddProperty => self.object_change(|object| object.properties.push((String::new(), String::new()))),
            ObjectMessage::RemoveProperty(at) => self.object_change(|object| {
                if at < object.properties.len() {
                    object.properties.remove(at);
                }
            }),
        }
    }

    /// A change of the picked object of the project that is not typed: taken, or refused with
    /// why.
    fn object_change(&mut self, to: impl FnOnce(&mut ObjectType)) {
        let Some(id) = self.object_view.picked.clone() else { return };
        self.object_view.draft = None;
        self.object_view.refused = change(&mut self.library, &id, to).err().map(|why| why.to_string());
    }

    /// The editor of the picked object of the project.
    fn object_editor<'a>(&'a self, p: Palette, l: Language, object: &'a ObjectType) -> Element<'a, Message> {
        let lang = super::library::lang(l);
        let library = &self.library;
        let view = &self.object_view;
        let input = |of: Field, label: String| -> Element<'a, ObjectMessage> {
            let value = view.draft.as_ref().filter(|(field, _)| *field == of).map_or_else(|| shown(object, of), |(_, typed)| typed.clone());
            field(p, label.clone(), a11y::named(text_input("", &value).on_input(move |typed| ObjectMessage::Typed(of, typed)).size(12).padding([2, 6]), Aria::TextField, label))
        };
        let mut fields: Column<'_, ObjectMessage> = Column::new().spacing(4);
        for of in Lang::ALL {
            fields = fields.push(input(Field::Name(of), super::materials::lang_name(of).to_string()));
        }
        fields = fields.push(input(Field::Size(0), l.text("Breedte (mm)", "Width (mm)").to_string()));
        fields = fields.push(input(Field::Size(1), l.text("Diepte (mm)", "Depth (mm)").to_string()));
        fields = fields.push(input(Field::Size(2), l.text("Hoogte (mm)", "Height (mm)").to_string()));
        fields = fields.push(input(Field::Base, l.text("Onderkant boven vloer (mm)", "Underside above floor (mm)").to_string()));
        let level = pick_list(Lod::ALL.map(Level), Some(Level(object.lod)), |level: Level| ObjectMessage::Level(level.0)).text_size(12).padding([2, 6]).width(Fill);
        fields = fields.push(field(p, "LOD".to_string(), a11y::named(level, Aria::ComboBox, "LOD")));
        fields = fields.push(super::codes::picker(p, l, library, object.group.code(), object.code.as_deref(), ObjectMessage::Coded));
        let material = l.text("Materiaal", "Material");
        let none = Made { key: None, label: l.text("geen", "none").to_string() };
        let made: Vec<Made> = std::iter::once(none).chain(library.materials.iter().map(|m| Made { key: Some(m.key.clone()), label: m.name(lang).to_string() })).collect();
        let now = made.iter().find(|m| m.key == object.material).cloned();
        fields = fields.push(field(p, material.to_string(), a11y::named(pick_list(made, now, |made: Made| ObjectMessage::Made(made.key)).text_size(12).padding([2, 6]).width(Fill), Aria::ComboBox, material)));
        let (class, _, predefined) = object.ifc();
        fields = fields.push(input(Field::Predefined, l.text("IFC-type", "IFC predefined type").to_string()));
        let act = |label: &'static str, message: ObjectMessage| button(text(label).size(12).color(p.text_secondary)).padding([3, 8]).style(move |_, s| p.row_button(s)).on_press(message);
        let mut properties: Column<'_, ObjectMessage> = column![text(l.text("Eigenschappen", "Properties")).size(11).color(p.text_secondary)].spacing(4);
        for at in 0..object.properties.len() {
            let cell = |of: Field, hint: &'static str| -> Element<'a, ObjectMessage> {
                let value = view.draft.as_ref().filter(|(field, _)| *field == of).map_or_else(|| shown(object, of), |(_, typed)| typed.clone());
                a11y::named(text_input(hint, &value).on_input(move |typed| ObjectMessage::Typed(of, typed)).size(12).padding([2, 6]), Aria::TextField, hint).into()
            };
            let remove = a11y::named(button(text("×").size(12).color(p.text_secondary)).padding([2, 6]).style(move |_, s| p.row_button(s)).on_press(ObjectMessage::RemoveProperty(at)), Aria::Button, l.text("Eigenschap verwijderen", "Remove property"));
            properties = properties.push(row![cell(Field::Property(at), l.text("naam", "name")), cell(Field::Value(at), l.text("waarde", "value")), remove].spacing(4).align_y(Center));
        }
        properties = properties.push(row![act(l.text("+ Eigenschap", "+ Property"), ObjectMessage::AddProperty), Space::new().width(Fill)]);
        let refused = view.refused.as_ref().map(|why| text(format!("{} ({why})", l.text("Niet overgenomen.", "Not taken."))).size(11).color(p.accent).wrapping(Wrapping::Word));
        let pane: Column<'_, ObjectMessage> = column![
            text(object.name(lang).to_string()).size(14).font(FONT_BOLD).color(p.text).wrapping(Wrapping::Word),
            text(format!("{} · {}", object.group.trade().name(lang), object.group.name(lang))).size(11).color(p.text_muted),
            text(format!("{class} · {predefined}")).size(11).color(p.text_muted),
            fields,
            properties,
            row![act(l.text("Kopiëren", "Copy"), ObjectMessage::Copy), act(l.text("Verwijderen", "Remove"), ObjectMessage::Remove), Space::new().width(Fill)].spacing(4),
        ]
        .push(refused)
        .spacing(8);
        container(Element::from(pane).map(Message::Object)).padding(8).into()
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
        if shelf == Shelf::Project {
            return self.object_editor(p, l, object);
        }
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
        pane = pane.push(row![act(l.text("Naar het project", "Take into the project"), ObjectMessage::Take(shelf, object.id.clone())), Space::new().width(Fill)]);
        let refused = self.object_view.refused.as_ref().map(|why| text(format!("{} ({why})", l.text("Niet overgenomen.", "Not taken."))).size(11).color(p.accent).wrapping(Wrapping::Word));
        container(pane.push(refused)).padding(8).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        // an object of another library is shown, with the way to take it
        app.object_update(ObjectMessage::Pick("object.luminaire".into()));
        let other = Library::starter();
        let mut pane = crate::app::test_ui_of(iced::Size::new(340.0, 900.0), app.object_pane(p, Language::English, &other, Shelf::General));
        assert!(pane.find("luminaire").is_ok() && pane.find("electrical and lighting · luminaires").is_ok() && pane.find("600 × 600 × 80 mm").is_ok());
        assert!(pane.find("200 · generic element, approximate").is_ok() && pane.find("IfcLightFixture · POINTSOURCE").is_ok() && pane.find("63 · lighting").is_ok() && pane.find("Take into the project").is_ok());
        // one of the project is edited
        let mut pane = crate::app::test_ui_of(iced::Size::new(340.0, 1100.0), app.object_pane(p, Language::English, &app.library, Shelf::Project));
        for shown in ["electrical and lighting · luminaires", "IfcLightFixture · POINTSOURCE", "Width (mm)", "Underside above floor (mm)", "IFC predefined type", "Properties", "+ Property", "Copy", "Remove"] {
            assert!(pane.find(shown).is_ok(), "{shown}");
        }
    }

    #[test]
    fn an_object_of_the_project_is_added_edited_and_copied() {
        let mut app = App::default();
        let before = app.library.object_types.len();
        app.object_update(ObjectMessage::Add(ObjectGroup::Luminaire));
        let id = app.object_view.picked.clone().expect("the new object is picked");
        assert_eq!((id.as_str(), app.library.object_types.len(), app.category()), ("project.object.1", before + 1, Category::Objects(ObjectGroup::Luminaire)));
        let object = |app: &App, id: &str| app.library.object_type(id).cloned().expect("the object");
        // typed fields are taken as they hold together: a name, a size with a comma, a height
        app.object_update(ObjectMessage::Typed(Field::Name(Lang::En), "line luminaire 1200".into()));
        app.object_update(ObjectMessage::Typed(Field::Size(0), "1200".into()));
        app.object_update(ObjectMessage::Typed(Field::Size(1), "62,5".into()));
        app.object_update(ObjectMessage::Typed(Field::Base, "2600".into()));
        let now = object(&app, &id);
        assert_eq!((now.name(Lang::En), now.name(Lang::Nl), now.size, now.base), ("line luminaire 1200", "armatuur", [1.2, 0.0625, 0.08], 2.6));
        assert_eq!((app.object_view.draft.clone(), app.object_view.refused.clone()), (None, None));
        // what does not hold together stays in the field; the object keeps what it had
        app.object_update(ObjectMessage::Typed(Field::Size(2), "8o".into()));
        assert_eq!((object(&app, &id).size[2], app.object_view.draft.clone(), app.object_view.refused.is_some()), (0.08, Some((Field::Size(2), "8o".into())), false), "no number");
        app.object_update(ObjectMessage::Typed(Field::Size(2), "0".into()));
        assert!(object(&app, &id).size[2] == 0.08 && app.object_view.refused.is_some(), "a box has a height");
        app.object_update(ObjectMessage::Typed(Field::Name(Lang::De), String::new()));
        assert!(object(&app, &id).name(Lang::De) == "Leuchte" && app.object_view.draft == Some((Field::Name(Lang::De), String::new())));
        app.object_update(ObjectMessage::Typed(Field::Size(2), "60".into()));
        assert_eq!((object(&app, &id).size[2], app.object_view.draft.clone(), app.object_view.refused.clone()), (0.06, None, None));
        // its level, its code, its material and its predefined type
        app.object_update(ObjectMessage::Level(Lod::L300));
        app.object_update(ObjectMessage::Coded(Some("63.1".into())));
        let material = app.library.materials[0].key.clone();
        app.object_update(ObjectMessage::Made(Some(material.clone())));
        app.object_update(ObjectMessage::Typed(Field::Predefined, "directionsource".into()));
        let now = object(&app, &id);
        assert_eq!((now.lod, now.code.as_deref(), now.material.as_deref(), now.ifc().2.as_str()), (Lod::L300, Some("63.1"), Some(material.as_str()), "DIRECTIONSOURCE"));
        app.object_update(ObjectMessage::Made(Some("no such material".into())));
        assert!(object(&app, &id).material.as_deref() == Some(material.as_str()) && app.object_view.refused.is_some());
        app.object_update(ObjectMessage::Typed(Field::Predefined, "not a type".into()));
        assert_eq!(object(&app, &id).predefined.as_deref(), Some("DIRECTIONSOURCE"));
        app.object_update(ObjectMessage::Typed(Field::Predefined, String::new()));
        assert_eq!(object(&app, &id).ifc().2, "POINTSOURCE", "its group's again");
        // properties: added, named, valued, removed
        app.object_update(ObjectMessage::AddProperty);
        app.object_update(ObjectMessage::Typed(Field::Property(0), "Power".into()));
        app.object_update(ObjectMessage::Typed(Field::Value(0), "36 W".into()));
        app.object_update(ObjectMessage::AddProperty);
        app.object_update(ObjectMessage::Typed(Field::Value(4), "x".into()));
        assert_eq!(object(&app, &id).properties, [("Power".to_string(), "36 W".to_string()), (String::new(), String::new())]);
        app.object_update(ObjectMessage::RemoveProperty(1));
        app.object_update(ObjectMessage::RemoveProperty(7));
        assert_eq!(object(&app, &id).properties.len(), 1);
        // the code's types count it: one code, many types
        assert!(app.library.coded("63.1", false) == [id.as_str()] && app.library.coded("63", true).contains(&id.as_str()) && super::super::codes::type_names(&app.library, "63.1", Lang::En).contains(&"line luminaire 1200".to_string()));
        // typing in one field is one step to take back; another field is another
        let typing = |field: Field| app.typing_of(&Message::Object(ObjectMessage::Typed(field, "1".into())));
        assert_eq!(typing(Field::Size(0)), Some(super::super::undo::Typing::Object(id.clone(), Field::Size(0))));
        assert!(typing(Field::Size(0)) != typing(Field::Size(1)) && app.typing_of(&Message::Object(ObjectMessage::Copy)).is_none());
        // a copy stands beside it under an id of its own, and is the one picked
        app.object_update(ObjectMessage::Copy);
        let twin = app.object_view.picked.clone().expect("the copy is picked");
        let at = |id: &str| app.library.object_types.iter().position(|o| o.id == id).expect("there");
        assert_eq!((twin.as_str(), at(&twin)), ("project.object.2", at(&id) + 1));
        assert_eq!(ObjectType { id: id.clone(), ..object(&app, &twin) }, object(&app, &id));
        // the edited object is written and read back as it is
        let read = bim_library::ifcx::read(&bim_library::ifcx::file(&app.library, "test", "test", "2026-01-01T00:00:00Z")).expect("the library reads");
        assert_eq!(read.object_type(&id), app.library.object_type(&id));
        app.object_update(ObjectMessage::Remove);
        app.object_update(ObjectMessage::Pick(id.clone()));
        app.object_update(ObjectMessage::Remove);
        assert_eq!((app.library.object_types.len(), app.object_view.picked.clone()), (before, None));
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
        assert_eq!(counts.iter().sum::<usize>(), library.buildups.len() + library.linear_types.len() + library.system_types.len() + library.object_types.len() + library.connection_types.len() + library.service_types.len());
        assert_eq!(counts, bim_library::remote::levels(library));
        // the generic objects are LOD 200; a cavity wall is LOD 300; a system with its members LOD 350
        let at = |lod: Lod| at_level(library, lod, Lang::En, l);
        assert!(at(Lod::L200).iter().any(|(name, kind, _)| name == "luminaire" && *kind == "luminaires"));
        assert!(at(Lod::L300).iter().any(|(name, _, category)| name.starts_with("Brickwork-air cavity") && *category == Category::Planar(bim_library::Role::Wall)));
        assert!(!at(Lod::L350).is_empty());
        assert_eq!(app.lod_tiles(app.theme.palette(), l, library, Lod::L200).len(), counts[0]);
    }
}
