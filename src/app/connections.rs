//! Connections in the Library product, and a join's type from the library.
//!
//! **Connection types** (`bim_library::connection`) are shown by family: steel, timber, timber
//! frame, concrete, masonry, planar elements, finishes, services. A tile says a type's name
//! and its key; the right pane shows the picked one: what it stands between, its sizes, its
//! level of development, its NL-SfB code and its stable key. A type of another library is taken
//! into the project with a button. One of the project is copied (to make a type from an
//! existing one), removed, and its plate's sizes typed in millimetres: what is typed is taken
//! as soon as the type holds together, and until then stays in the field.
//!
//! **A join takes its type from the library.** The recipe of a connection type is the kernel's
//! own (`bim_core::ConnectionKind`): [`kind`] and [`recipe`] turn one into the other, and
//! [`type_of`] finds the type a join is of by what it does. The properties of a selected join
//! list the types of the project's library that stand between two members; picking one gives
//! the join that type's recipe.
//!
//! Not yet: a picker with previews, search and favourites, applying a type to all like
//! meetings at once, and the families the kernel has no recipe for.

use super::libraries::{Category, Shelf};
use super::{App, Message, FONT_BOLD};
use crate::theme::Palette;
use bim_core::join::{EndPlate, FinPlate};
use bim_core::ConnectionKind;
use bim_library::connection::{Between, ConnectionType, Family, Recipe};
use bim_library::{Lang, Library, LibraryError};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, pick_list, row, text, text_input, Column, Space};
use iced::{Center, Element, Fill, Length};
use openaec_ui::design::a11y::{self, Role as Aria};
use openaec_ui::Language;

/// What the kernel makes of a recipe.
pub fn kind(recipe: &Recipe) -> ConnectionKind {
    match *recipe {
        Recipe::Butt => ConnectionKind::Butt,
        Recipe::Mitre => ConnectionKind::Mitre,
        Recipe::Cut => ConnectionKind::Cut,
        Recipe::EndPlate { thickness, margin, bolt, hole, rows } => ConnectionKind::EndPlate(EndPlate { thickness, margin, bolt, hole, rows }),
        Recipe::FinPlate { thickness, gap, edge, bolt, hole, rows } => ConnectionKind::FinPlate(FinPlate { thickness, gap, edge, bolt, hole, rows }),
    }
}

/// The recipe of what a join does; none for two elements kept apart, which is no connection.
pub fn recipe(kind: &ConnectionKind) -> Option<Recipe> {
    Some(match *kind {
        ConnectionKind::Butt => Recipe::Butt,
        ConnectionKind::Mitre => Recipe::Mitre,
        ConnectionKind::Cut => Recipe::Cut,
        ConnectionKind::EndPlate(EndPlate { thickness, margin, bolt, hole, rows }) => Recipe::EndPlate { thickness, margin, bolt, hole, rows },
        ConnectionKind::FinPlate(FinPlate { thickness, gap, edge, bolt, hole, rows }) => Recipe::FinPlate { thickness, gap, edge, bolt, hole, rows },
        ConnectionKind::Apart => return None,
    })
}

/// The types of `library` that stand between two members: what a join of two members can be.
pub fn for_members(library: &Library) -> impl Iterator<Item = &ConnectionType> {
    library.connection_types.iter().filter(|t| t.between == Between::Members)
}

/// The type of `library` a join that does `kind` is of: the first between two members with
/// that recipe, sizes and all.
pub fn type_of<'a>(library: &'a Library, kind: &ConnectionKind) -> Option<&'a ConnectionType> {
    let recipe = recipe(kind)?;
    for_members(library).find(|t| t.recipe == recipe)
}

/// A size of a plate that is typed into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Thickness,
    /// An end plate's margin past the profile; a fin plate's edge distance.
    Margin,
    /// A fin plate's gap to the face.
    Gap,
    Bolt,
    Hole,
    Rows,
}

#[derive(Debug, Clone)]
pub enum ConnectionMessage {
    /// Show this type of the source shown.
    Pick(String),
    /// Take this type of another library into the project's.
    Take(Shelf, String),
    /// A copy of the picked type of the project, under a key of its own.
    Copy,
    /// Remove the picked type from the project's library.
    Remove,
    /// Typed into a size of the picked type of the project.
    Typed(Field, String),
}

/// Which type the right pane shows, why the last change was refused, and what is typed in a
/// field that does not hold together yet.
#[derive(Debug, Clone, Default)]
pub struct ConnectionView {
    pub picked: Option<String>,
    pub refused: Option<String>,
    pub draft: Option<(Field, String)>,
}

/// A type of `from` taken into `library`, with the material its parts are of; its key. A type
/// the library has is not taken twice.
pub fn take(library: &mut Library, from: &Library, id: &str) -> Result<String, LibraryError> {
    let connection = from.connection_type(id).ok_or(LibraryError::NoKey)?.clone();
    if let Some(material) = connection.material.as_deref().filter(|key| library.material(key).is_none()).and_then(|key| from.material(key)) {
        library.add_material(material.clone())?;
    }
    library.add_connection_type(connection)?;
    Ok(id.to_string())
}

/// A copy of type `id` beside it, under the first free key of the project's own
/// ("project.connection.1", and on); its key.
pub fn copy(library: &mut Library, id: &str) -> Result<String, LibraryError> {
    let at = library.connection_types.iter().position(|c| c.id == id).ok_or(LibraryError::NoKey)?;
    let mut connection = library.connection_types[at].clone();
    connection.id = (1..).map(|n| format!("project.connection.{n}")).find(|key| library.connection_type(key).is_none()).expect("a free key");
    let key = connection.id.clone();
    library.connection_types.insert(at + 1, connection);
    Ok(key)
}

/// What is typed into `field` put on `recipe`: millimetres, or a count of bolts; false when it
/// is no number or the recipe has no such size.
fn put(recipe: &mut Recipe, field: Field, typed: &str) -> bool {
    let metres = typed.trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite()).map(|mm| mm / 1000.0);
    let count = typed.trim().parse::<u32>().ok();
    let (size, rows): (Option<&mut f64>, Option<&mut u32>) = match (recipe, field) {
        (Recipe::EndPlate { thickness, .. } | Recipe::FinPlate { thickness, .. }, Field::Thickness) => (Some(thickness), None),
        (Recipe::EndPlate { margin, .. }, Field::Margin) => (Some(margin), None),
        (Recipe::FinPlate { edge, .. }, Field::Margin) => (Some(edge), None),
        (Recipe::FinPlate { gap, .. }, Field::Gap) => (Some(gap), None),
        (Recipe::EndPlate { bolt, .. } | Recipe::FinPlate { bolt, .. }, Field::Bolt) => (Some(bolt), None),
        (Recipe::EndPlate { hole, .. } | Recipe::FinPlate { hole, .. }, Field::Hole) => (Some(hole), None),
        (Recipe::EndPlate { rows, .. } | Recipe::FinPlate { rows, .. }, Field::Rows) => (None, Some(rows)),
        _ => return false,
    };
    match (size, metres, rows, count) {
        (Some(size), Some(value), ..) => *size = value,
        (_, _, Some(rows), Some(value)) => *rows = value,
        _ => return false,
    }
    true
}

/// The sizes of `recipe` to type into, each with its label and what it shows.
fn fields(recipe: &Recipe, l: Language) -> Vec<(Field, &'static str, String)> {
    let mm = |v: f64| bim_library::millimetres(v, Lang::En);
    match *recipe {
        Recipe::Butt | Recipe::Mitre | Recipe::Cut => Vec::new(),
        Recipe::EndPlate { thickness, margin, bolt, hole, rows } => vec![
            (Field::Thickness, l.text("Plaatdikte (mm)", "Plate thickness (mm)"), mm(thickness)),
            (Field::Margin, l.text("Plaat buiten het profiel (mm)", "Plate past the profile (mm)"), mm(margin)),
            (Field::Bolt, l.text("Bout (mm)", "Bolt (mm)"), mm(bolt)),
            (Field::Hole, l.text("Gat (mm)", "Hole (mm)"), mm(hole)),
            (Field::Rows, l.text("Bouten per rij", "Bolts per row"), rows.to_string()),
        ],
        Recipe::FinPlate { thickness, gap, edge, bolt, hole, rows } => vec![
            (Field::Thickness, l.text("Plaatdikte (mm)", "Plate thickness (mm)"), mm(thickness)),
            (Field::Gap, l.text("Ruimte tot het vlak (mm)", "Gap to the face (mm)"), mm(gap)),
            (Field::Margin, l.text("Randafstand (mm)", "Edge distance (mm)"), mm(edge)),
            (Field::Bolt, l.text("Bout (mm)", "Bolt (mm)"), mm(bolt)),
            (Field::Hole, l.text("Gat (mm)", "Hole (mm)"), mm(hole)),
            (Field::Rows, l.text("Aantal bouten", "Number of bolts"), rows.to_string()),
        ],
    }
}

/// A type to pick for a join: its key and its name.
#[derive(Debug, Clone, PartialEq)]
struct Choice {
    id: String,
    name: String,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

impl App {
    pub(super) fn connection_update(&mut self, message: ConnectionMessage) {
        match message {
            ConnectionMessage::Pick(id) => self.connection_view = ConnectionView { picked: Some(id), ..ConnectionView::default() },
            ConnectionMessage::Take(shelf, id) => {
                let from = self.shelf(shelf).clone();
                match take(&mut self.library, &from, &id) {
                    Ok(id) => {
                        let family = self.library.connection_type(&id).map(|c| c.family);
                        self.connection_view = ConnectionView { picked: Some(id), ..ConnectionView::default() };
                        if let Some(family) = family {
                            self.show_in_project(Category::Connections(family));
                        }
                    }
                    Err(why) => self.connection_view.refused = Some(why.to_string()),
                }
            }
            ConnectionMessage::Copy => {
                if let Some(id) = self.connection_view.picked.clone() {
                    match copy(&mut self.library, &id) {
                        Ok(key) => self.connection_view = ConnectionView { picked: Some(key), ..ConnectionView::default() },
                        Err(why) => self.connection_view.refused = Some(why.to_string()),
                    }
                }
            }
            ConnectionMessage::Remove => {
                if let Some(id) = self.connection_view.picked.take() {
                    self.library.connection_types.retain(|c| c.id != id);
                }
                self.connection_view = ConnectionView::default();
            }
            ConnectionMessage::Typed(field, typed) => {
                let Some(at) = self.connection_view.picked.as_deref().and_then(|id| self.library.connection_types.iter().position(|c| c.id == id)) else { return };
                let mut changed = self.library.connection_types[at].clone();
                let took = put(&mut changed.recipe, field, &typed);
                let checked = changed.check();
                // what does not hold together yet stays in the field, the type as it was
                self.connection_view.refused = checked.as_ref().err().filter(|_| took).map(|why| why.to_string());
                if took && checked.is_ok() {
                    self.library.connection_types[at] = changed;
                    self.connection_view.draft = None;
                } else {
                    self.connection_view.draft = Some((field, typed));
                }
            }
        }
    }

    /// The selected join takes the recipe of type `id` of the project's library (a cut stays a
    /// cut, and takes only a cut's type).
    pub(super) fn set_join_type(&mut self, id: &str) {
        let Some(connection) = self.library.connection_type(id) else { return };
        self.set_join_recipe(kind(&connection.recipe));
    }

    /// The connection types of `family` of the source shown as tiles: what a type does to
    /// start with, its name and its key.
    pub(super) fn connection_tiles<'a>(&'a self, p: Palette, l: Language, library: &'a Library, family: Family) -> Vec<Element<'a, Message>> {
        let lang = super::library::lang(l);
        library
            .connection_types_of(family)
            .map(|connection| {
                let bolts = connection.recipe.sizes(lang).into_iter().find(|(label, _)| *label == "Bolts" || *label == "Bouten").map_or_else(|| connection.between.name(lang).to_string(), |(_, bolts)| bolts);
                let look = container(text(bolts).size(13).color(p.text)).width(Fill).align_x(Center);
                let on = self.connection_view.picked.as_deref() == Some(connection.id.as_str());
                super::libraries::tile(p, look.into(), connection.name(lang), connection.id.clone(), on, Some(Message::Connection(ConnectionMessage::Pick(connection.id.clone()))))
            })
            .collect()
    }

    /// The right pane for the connections: the picked one of `library`, on `shelf`.
    pub fn connection_pane<'a>(&'a self, p: Palette, l: Language, library: &'a Library, shelf: Shelf) -> Element<'a, Message> {
        let lang = super::library::lang(l);
        let view = &self.connection_view;
        let Some(connection) = view.picked.as_deref().and_then(|id| library.connection_type(id)) else {
            let hint = l.text("Klik een verbinding om te zien wat ze is.", "Click a connection to see what it is.");
            return container(text(hint).size(12).color(p.text_muted).wrapping(Wrapping::Word)).padding(8).into();
        };
        let line = |label: String, value: String| -> Element<'a, ConnectionMessage> { row![text(label).size(12).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph).width(Length::FillPortion(2)), text(value).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph).width(Length::FillPortion(3))].spacing(8).into() };
        let (says_nl, says_en) = connection.lod().says();
        let mut pane: Column<'_, ConnectionMessage> = column![
            text(connection.name(lang)).size(14).font(FONT_BOLD).color(p.text).wrapping(Wrapping::Word),
            text(format!("{} · {}", connection.family.name(lang), connection.between.name(lang))).size(11).color(p.text_muted).wrapping(Wrapping::Word),
            line(l.text("Sleutel", "Key").to_string(), connection.id.clone()),
        ]
        .spacing(4);
        if shelf == Shelf::Project {
            for (of, label, value) in fields(&connection.recipe, l) {
                let value = view.draft.as_ref().filter(|(field, _)| *field == of).map_or(value, |(_, typed)| typed.clone());
                let input = a11y::named(text_input("", &value).on_input(move |typed| ConnectionMessage::Typed(of, typed)).size(12).padding([2, 6]), Aria::TextField, label);
                pane = pane.push(row![text(label).size(12).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph).width(Length::FillPortion(2)), container(input).width(Length::FillPortion(3))].spacing(8).align_y(Center));
            }
        } else {
            for (label, value) in connection.recipe.sizes(lang) {
                pane = pane.push(line(label.to_string(), value));
            }
        }
        pane = pane.push(line("LOD".to_string(), format!("{} · {}", connection.lod().number(), l.text(says_nl, says_en))));
        if let Some(code) = connection.nlsfb() {
            pane = pane.push(line("NL-SfB".to_string(), super::codes::named(library, &code, lang)));
        }
        if let Some(material) = connection.material.as_deref() {
            pane = pane.push(line(l.text("Materiaal", "Material").to_string(), library.material(material).map_or(material, |m| m.name(lang)).to_string()));
        }
        if connection.recipe.has_parts() {
            pane = pane.push(text(l.text("De maten zijn een begin om te wijzigen, geen gecontroleerd constructief ontwerp.", "The sizes are a start to change, not a checked structural design.")).size(11).color(p.text_muted).wrapping(Wrapping::Word));
        }
        let act = |label: &'static str, message: ConnectionMessage| button(text(label).size(12).color(p.text_secondary)).padding([3, 8]).style(move |_, s| p.row_button(s)).on_press(message);
        let actions = if shelf == Shelf::Project {
            row![act(l.text("Kopiëren", "Copy"), ConnectionMessage::Copy), act(l.text("Verwijderen", "Remove"), ConnectionMessage::Remove), Space::new().width(Fill)]
        } else {
            row![act(l.text("Naar het project", "Take into the project"), ConnectionMessage::Take(shelf, connection.id.clone())), Space::new().width(Fill)]
        };
        pane = pane.push(actions.spacing(4));
        let refused = view.refused.as_ref().map(|why| text(format!("{} ({why})", l.text("Niet overgenomen.", "Not taken."))).size(11).color(p.accent).wrapping(Wrapping::Word));
        container(Element::from(pane.push(refused)).map(Message::Connection)).padding(8).into()
    }

    /// The line of a join's properties that picks its type: the types of the project's library
    /// between two members, with the one the join is of now picked (none when its sizes are
    /// its own).
    pub(super) fn join_type_picker<'a>(&'a self, p: Palette, l: Language, now: &ConnectionKind) -> Element<'a, Message> {
        let lang = super::library::lang(l);
        let choices: Vec<Choice> = for_members(&self.library).map(|t| Choice { id: t.id.clone(), name: format!("{} · {}", t.family.name(lang), t.name(lang)) }).collect();
        let of = type_of(&self.library, now).map(|t| t.id.clone());
        let picked = choices.iter().find(|c| Some(&c.id) == of.as_ref()).cloned();
        let label = l.text("Type uit de bibliotheek", "Type of the library");
        let pick = pick_list(choices, picked, |c: Choice| Message::JoinType(c.id)).placeholder(l.text("eigen maten", "sizes of its own")).text_size(12).padding([2, 6]).width(Fill);
        column![text(label).size(11).color(p.text_secondary), a11y::named(pick, Aria::ComboBox, label)].spacing(2).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bim_core::{ConnectionDef, LinearDef, ProfileDef, Shape};
    use glam::DVec3;

    #[test]
    fn a_recipe_is_what_the_kernel_makes_and_back() {
        let library = Library::starter();
        // every type of the catalogue is something the kernel makes, and reads back as it was
        for t in &library.connection_types {
            assert_eq!(recipe(&kind(&t.recipe)), Some(t.recipe), "{}", t.id);
        }
        // the kernel's own defaults are types of the catalogue, found by what they do
        assert_eq!(type_of(&library, &ConnectionKind::FinPlate(FinPlate::default())).map(|t| t.id.as_str()), Some("steel.fin-plate.m20-3"));
        assert_eq!(type_of(&library, &ConnectionKind::EndPlate(EndPlate::default())).map(|t| t.id.as_str()), Some("steel.end-plate.m20-2x2"));
        assert_eq!(type_of(&library, &ConnectionKind::Butt).map(|t| t.id.as_str()), Some("steel.butt"));
        // a plate of sizes no type has is of none; two elements kept apart are no connection
        assert!(type_of(&library, &ConnectionKind::FinPlate(FinPlate { thickness: 0.011, ..FinPlate::default() })).is_none());
        assert!(recipe(&ConnectionKind::Apart).is_none() && type_of(&library, &ConnectionKind::Apart).is_none());
        // a cut is no join of two members
        assert!(type_of(&library, &ConnectionKind::Cut).is_none() && for_members(&library).all(|t| t.recipe != Recipe::Cut));
    }

    #[test]
    fn the_library_shows_its_connections_by_family_and_the_pane_the_picked_one() {
        let mut app = App::default();
        let p = app.theme.palette();
        let l = Language::English;
        let per: Vec<usize> = Family::ALL.iter().map(|family| app.connection_tiles(p, l, &app.library, *family).len()).collect();
        assert_eq!(per, [21, 2, 0, 1, 0, 0, 0, 1]);
        app.connection_update(ConnectionMessage::Pick("steel.fin-plate.m20-3".into()));
        // of another library: its sizes, and the way to take it
        let other = Library::starter();
        let mut pane = crate::app::test_ui_of(iced::Size::new(340.0, 900.0), app.connection_pane(p, l, &other, Shelf::General));
        for shown in ["Fin plate 10 mm, 3 × M20", "steel · two members", "steel.fin-plate.m20-3", "Edge distance", "3 × M20", "350 · with its interfaces to other elements", "28 · main structure", "Take into the project"] {
            assert!(pane.find(shown).is_ok(), "{shown}");
        }
        // of the project: its sizes to type
        let mut pane = crate::app::test_ui_of(iced::Size::new(340.0, 900.0), app.connection_pane(p, l, &app.library, Shelf::Project));
        for shown in ["Plate thickness (mm)", "Gap to the face (mm)", "Number of bolts", "Copy", "Remove", "The sizes are a start to change, not a checked structural design."] {
            assert!(pane.find(shown).is_ok(), "{shown}");
        }
    }

    #[test]
    fn a_type_is_made_from_an_existing_one_and_its_sizes_typed() {
        let mut app = App::default();
        let before = app.library.connection_types.len();
        app.connection_update(ConnectionMessage::Pick("steel.fin-plate.m20-3".into()));
        app.connection_update(ConnectionMessage::Copy);
        let key = app.connection_view.picked.clone().expect("the copy is picked");
        let at = |app: &App, id: &str| app.library.connection_types.iter().position(|c| c.id == id).expect("there");
        assert_eq!((key.as_str(), app.library.connection_types.len(), at(&app, &key)), ("project.connection.1", before + 1, at(&app, "steel.fin-plate.m20-3") + 1));
        let of = |app: &App, id: &str| app.library.connection_type(id).cloned().expect("the type");
        // its sizes typed in millimetres; its name follows by rule
        app.connection_update(ConnectionMessage::Typed(Field::Thickness, "12".into()));
        app.connection_update(ConnectionMessage::Typed(Field::Rows, "4".into()));
        app.connection_update(ConnectionMessage::Typed(Field::Margin, "45,5".into()));
        assert_eq!(of(&app, &key).recipe, Recipe::FinPlate { thickness: 0.012, gap: 0.01, edge: 0.0455, bolt: 0.02, hole: 0.022, rows: 4 });
        assert_eq!((of(&app, &key).name(Lang::En), app.connection_view.draft.clone()), ("Fin plate 12 mm, 4 × M20".to_string(), None));
        // what is no plate stays in the field; the type keeps what it had
        app.connection_update(ConnectionMessage::Typed(Field::Hole, "18".into()));
        assert!(of(&app, &key).recipe == Recipe::FinPlate { thickness: 0.012, gap: 0.01, edge: 0.0455, bolt: 0.02, hole: 0.022, rows: 4 } && app.connection_view.refused.is_some());
        assert_eq!(app.connection_view.draft, Some((Field::Hole, "18".into())));
        app.connection_update(ConnectionMessage::Typed(Field::Rows, "vier".into()));
        assert_eq!((app.connection_view.draft.clone(), app.connection_view.refused.clone()), (Some((Field::Rows, "vier".into())), None), "no number");
        app.connection_update(ConnectionMessage::Typed(Field::Hole, "24".into()));
        assert_eq!((app.connection_view.draft.clone(), app.connection_view.refused.clone()), (None, None));
        // the catalogue's own type is as it was; the copy is kept by its key through a save
        assert_eq!(of(&app, "steel.fin-plate.m20-3").recipe, kind_recipe(FinPlate::default()));
        let read = bim_library::ifcx::read(&bim_library::ifcx::file(&app.library, "test", "test", "2026-11-20T00:00:00Z")).expect("the library reads");
        assert_eq!(read.connection_type(&key), app.library.connection_type(&key));
        // typing in one field is one step to take back
        let typing = |field: Field| app.typing_of(&Message::Connection(ConnectionMessage::Typed(field, "1".into())));
        assert_eq!(typing(Field::Bolt), Some(super::super::undo::Typing::Connection(key.clone(), Field::Bolt)));
        assert!(typing(Field::Bolt) != typing(Field::Hole));
        // a type of another library is taken once; removed, the project has what it had
        let mut other = Library::starter();
        other.add_connection_type(ConnectionType::new("office.timber.butt", Family::Timber, Between::Members, Recipe::Butt)).unwrap();
        assert_eq!(take(&mut app.library, &other, "office.timber.butt").as_deref(), Ok("office.timber.butt"));
        assert!(take(&mut app.library, &other, "office.timber.butt").is_err() && take(&mut app.library, &other, "none").is_err());
        for id in ["office.timber.butt", key.as_str()] {
            app.connection_update(ConnectionMessage::Pick(id.into()));
            app.connection_update(ConnectionMessage::Remove);
        }
        assert_eq!((app.library.connection_types.len(), app.connection_view.picked.clone()), (before, None));
    }

    fn kind_recipe(plate: FinPlate) -> Recipe {
        recipe(&ConnectionKind::FinPlate(plate)).expect("a recipe")
    }

    #[test]
    fn a_join_takes_its_type_from_the_library() {
        let mut app = App::default();
        let member = |a: DVec3, b: DVec3| Shape::Linear(LinearDef { start: a, end: b, profile: ProfileDef::Rectangle { width: 0.3, height: 0.3 }, roll: 0.0 });
        let column = app.add_element("Column", None, member(DVec3::ZERO, DVec3::new(0.0, 0.0, 3.0))).unwrap();
        let beam = app.add_element("Beam", None, member(DVec3::new(0.0, 0.0, 2.5), DVec3::new(4.0, 0.0, 2.5))).unwrap();
        let existing = app.modeller.model.iter().find(|e| matches!(&e.shape, Shape::Connection(c) if [c.primary, c.secondary].contains(&beam))).map(|e| e.id);
        let joint = existing.unwrap_or_else(|| app.add_element("Joint", None, Shape::Connection(ConnectionDef { kind: ConnectionKind::Butt, primary: column, secondary: beam })).unwrap());
        let kind_now = |app: &App| match &app.modeller.model.get(joint).expect("the join").shape {
            Shape::Connection(c) => c.kind,
            _ => panic!("no join"),
        };
        app.select_modelled(joint);
        // a fin plate of the catalogue by its key, then another of the same kind with other sizes
        let _ = app.update(Message::JoinType("steel.fin-plate.m20-3".into()));
        assert_eq!(kind_now(&app), ConnectionKind::FinPlate(FinPlate::default()));
        let _ = app.update(Message::JoinType("steel.fin-plate.m16-2".into()));
        assert_eq!(kind_now(&app), ConnectionKind::FinPlate(FinPlate { thickness: 0.008, gap: 0.01, edge: 0.032, bolt: 0.016, hole: 0.018, rows: 2 }));
        assert_eq!(type_of(&app.library, &kind_now(&app)).map(|t| t.id.as_str()), Some("steel.fin-plate.m16-2"));
        // a plate the kernel cannot make on these members (five M24 bolts on a 300 mm beam) is refused: the join stays
        let _ = app.update(Message::JoinType("steel.fin-plate.m24-5".into()));
        assert_eq!(type_of(&app.library, &kind_now(&app)).map(|t| t.id.as_str()), Some("steel.fin-plate.m16-2"));
        let _ = app.update(Message::JoinType("steel.end-plate.m20-2x2".into()));
        assert_eq!(kind_now(&app), ConnectionKind::EndPlate(EndPlate::default()));
        // a key the library lacks, and a cut's type on a join of members, change nothing
        let _ = app.update(Message::JoinType("steel.no-such".into()));
        let _ = app.update(Message::JoinType("services.penetration".into()));
        assert_eq!(kind_now(&app), ConnectionKind::EndPlate(EndPlate::default()));
        // the join's properties have the picker (what a pick list shows is not text a test finds)
        let mut pane = crate::app::test_ui_of(iced::Size::new(340.0, 200.0), app.join_type_picker(app.theme.palette(), Language::English, &kind_now(&app)));
        assert!(pane.find("Type of the library").is_ok());
    }
}
