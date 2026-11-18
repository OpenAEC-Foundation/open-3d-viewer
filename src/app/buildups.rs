//! The editor of layered build-ups in the Library product: the walls, floors, roofs and
//! ceilings of the project's library.
//!
//! The Project page lists the build-ups of one role at a time; a click picks one, and the right
//! pane is its editor: its section as a drawing, and its layers from the exterior (or the top)
//! in, each a material and a thickness, to be changed, added, removed and moved. A change is
//! kept only when the build-up still checks against the library's materials; otherwise the pane
//! says why not. A build-up of another library is edited as a copy in the project.
//!
//! The walls and slabs of the model that are made of a build-up follow a change of it (see
//! `type_follow`): the pane says how many there are.

use super::libraries::{Editor, LibrariesView, Shelf, Source};
use super::type_follow::{use_note, TypeRef};
use super::{App, Message, FONT, FONT_BOLD};
use crate::theme::Palette;
use bim_library::{BuildUp, Layer, Library, LibraryError, Role};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, pick_list, row, text, text_input, Column, Space};
use iced::{Center, Element, Fill, Length, Task};
use openaec_ui::design::a11y::{self, Role as Aria};
use openaec_ui::Language;

#[derive(Debug, Clone)]
pub enum BuildUpMessage {
    /// Show the build-ups of this role on the Project page.
    Open(Role),
    /// Edit this build-up of the project.
    Pick(String),
    /// A build-up more of the role shown, of one layer to start with.
    Add,
    /// A copy of the picked one, to change.
    Duplicate,
    Remove,
    /// A name of its own, in place of the rule's; empty for the rule's.
    Named(String),
    /// The material (its key) of a layer.
    Material(usize, String),
    /// Typed as a layer's thickness in millimetres.
    Typed(usize, String),
    /// Enter: the typed thickness is taken.
    Done,
    /// A layer more after this one, like it.
    AddLayer(usize),
    RemoveLayer(usize),
    /// Moves a layer towards the exterior or the top (`true`), or away from it.
    Move(usize, bool),
    /// Edit a build-up of another library as a copy in the project.
    Copy(Shelf, String),
    /// Make the elements of the picked build-up as it is now.
    Follow,
}

/// Which role's build-ups show, which one is edited, what is being typed as a thickness, and why
/// the last change was refused.
#[derive(Debug, Clone, Default)]
pub struct BuildUpView {
    pub role: Option<Role>,
    pub picked: Option<String>,
    pub draft: Option<(usize, String)>,
    pub refused: Option<String>,
}

impl BuildUpView {
    /// The role whose build-ups show: walls until another is opened.
    pub fn shown(&self) -> Role {
        self.role.unwrap_or(Role::Wall)
    }
}

/// Changes build-up `id` of `library`; it stays as it was when it would no longer check.
pub fn change(library: &mut Library, id: &str, change: impl FnOnce(&mut BuildUp)) -> Result<(), LibraryError> {
    let at = library.buildups.iter().position(|b| b.id == id).ok_or(LibraryError::NoKey)?;
    let mut next = library.buildups[at].clone();
    change(&mut next);
    next.check(&library.materials)?;
    library.buildups[at] = next;
    Ok(())
}

/// An id no build-up of the library has: "own.wall.1".
fn fresh_id(library: &Library, role: Role) -> String {
    (1..).map(|n| format!("own.{}.{n}", role.name().to_lowercase())).find(|id| library.buildup(id).is_none()).unwrap_or_default()
}

/// A build-up more of `role`: one layer of the library's first material, 100 mm; its id.
pub fn add(library: &mut Library, role: Role) -> Result<String, LibraryError> {
    let material = library.materials.first().map(|m| m.key.clone()).ok_or(LibraryError::NoLayers)?;
    let id = fresh_id(library, role);
    library.add_buildup(BuildUp::new(&id, role, vec![Layer::new(&material, 0.1)]))?;
    Ok(id)
}

/// A copy of build-up `id`, right after it; the copy's id.
pub fn duplicate(library: &mut Library, id: &str) -> Result<String, LibraryError> {
    let at = library.buildups.iter().position(|b| b.id == id).ok_or(LibraryError::NoKey)?;
    let mut copy = library.buildups[at].clone();
    copy.id = fresh_id(library, copy.role);
    let new = copy.id.clone();
    library.buildups.insert(at + 1, copy);
    Ok(new)
}

/// A copy of build-up `id` of `from` in `library`, with the materials it needs; its id.
pub fn copy(library: &mut Library, from: &Library, id: &str) -> Result<String, LibraryError> {
    let source = from.buildup(id).ok_or(LibraryError::NoKey)?;
    for layer in &source.layers {
        if library.material(&layer.material).is_none() {
            if let Some(material) = from.material(&layer.material) {
                library.add_material(material.clone())?;
            }
        }
    }
    let mut fresh = source.clone();
    fresh.id = fresh_id(library, source.role);
    let new = fresh.id.clone();
    library.add_buildup(fresh)?;
    Ok(new)
}

pub(super) fn role_title(role: Role, l: Language) -> &'static str {
    match role {
        Role::Wall => l.text("Wanden", "Walls"),
        Role::Floor => l.text("Vloeren", "Floors"),
        Role::Roof => l.text("Daken", "Roofs"),
        Role::Ceiling => l.text("Plafonds", "Ceilings"),
    }
}

/// A material to pick for a layer.
#[derive(Debug, Clone, PartialEq)]
struct Choice {
    key: String,
    label: String,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

impl App {
    pub(super) fn buildup_update(&mut self, message: BuildUpMessage) -> Task<Message> {
        self.follow_refused.clear();
        let task = self.buildup_apply(message);
        if self.buildup_view.picked.is_some() {
            self.edit_only(Editor::BuildUp);
        }
        task
    }

    fn buildup_apply(&mut self, message: BuildUpMessage) -> Task<Message> {
        let picked = self.buildup_view.picked.clone();
        let outcome: Option<Result<(), LibraryError>> = match message {
            BuildUpMessage::Open(role) => {
                self.buildup_view = BuildUpView { role: Some(role), ..BuildUpView::default() };
                return Task::none();
            }
            BuildUpMessage::Pick(id) => {
                let role = self.library.buildup(&id).map(|b| b.role).or(self.buildup_view.role);
                self.buildup_view = BuildUpView { role, picked: Some(id), ..BuildUpView::default() };
                // (from a search: its place is the project's page)
                self.libraries = LibrariesView { source: Source::Project, country: self.libraries.country, query: String::new() };
                return Task::none();
            }
            BuildUpMessage::Add => Some(add(&mut self.library, self.buildup_view.shown()).map(|id| self.buildup_view.picked = Some(id))),
            BuildUpMessage::Duplicate => picked.as_deref().map(|id| duplicate(&mut self.library, id).map(|new| self.buildup_view.picked = Some(new))),
            BuildUpMessage::Remove => {
                if let Some(id) = &picked {
                    self.library.buildups.retain(|b| &b.id != id);
                    self.buildup_view.picked = None;
                }
                Some(Ok(()))
            }
            BuildUpMessage::Named(name) => picked.as_deref().map(|id| change(&mut self.library, id, |b| b.name = (!name.trim().is_empty()).then_some(name))),
            BuildUpMessage::Material(layer, key) => picked.as_deref().map(|id| {
                change(&mut self.library, id, |b| {
                    if let Some(layer) = b.layers.get_mut(layer) {
                        layer.material = key;
                    }
                })
            }),
            BuildUpMessage::Typed(layer, typed) => {
                self.buildup_view.draft = Some((layer, typed));
                return Task::none();
            }
            BuildUpMessage::Done => match (picked.as_deref(), self.buildup_view.draft.clone()) {
                (Some(id), Some((layer, typed))) => Some(match super::standards::number(&typed) {
                    Some(millimetres) => change(&mut self.library, id, |b| {
                        if let Some(layer) = b.layers.get_mut(layer) {
                            layer.thickness = millimetres / 1000.0;
                        }
                    }),
                    None => Err(LibraryError::NotPositive),
                }),
                _ => None,
            },
            BuildUpMessage::AddLayer(layer) => picked.as_deref().map(|id| {
                change(&mut self.library, id, |b| {
                    if let Some(like) = b.layers.get(layer).cloned() {
                        b.layers.insert(layer + 1, like);
                    }
                })
            }),
            BuildUpMessage::RemoveLayer(layer) => picked.as_deref().map(|id| {
                change(&mut self.library, id, |b| {
                    if layer < b.layers.len() {
                        b.layers.remove(layer);
                    }
                })
            }),
            BuildUpMessage::Move(layer, up) => picked.as_deref().map(|id| {
                change(&mut self.library, id, |b| {
                    let to = if up { layer.checked_sub(1) } else { Some(layer + 1) };
                    if let Some(to) = to.filter(|to| *to < b.layers.len() && layer < b.layers.len()) {
                        b.layers.swap(layer, to);
                    }
                })
            }),
            // (the elements follow whatever was taken, below)
            BuildUpMessage::Follow => Some(Ok(())),
            BuildUpMessage::Copy(shelf, id) => {
                let from = self.shelf(shelf).clone();
                Some(copy(&mut self.library, &from, &id).map(|new| {
                    let role = self.library.buildup(&new).map(|b| b.role);
                    self.buildup_view = BuildUpView { role, picked: Some(new), ..BuildUpView::default() };
                    self.libraries = LibrariesView { source: Source::Project, country: self.libraries.country, query: String::new() };
                }))
            }
        };
        match outcome {
            Some(Ok(())) => {
                self.buildup_view.draft = None;
                self.buildup_view.refused = None;
                // the walls and slabs made of the picked build-up follow it
                if let Some(id) = self.buildup_view.picked.clone() {
                    self.follow_type(TypeRef::BuildUp(&id));
                }
                // what the model shows of its types (their names) is made again
                if self.modeller.shown.is_some() && self.modeller.model.iter().next().is_some() {
                    self.show_modelled();
                }
            }
            Some(Err(why)) => self.buildup_view.refused = Some(why.to_string()),
            None => {}
        }
        Task::none()
    }

    /// The build-ups of the project on its page: a role at a time, each with its thickness. A
    /// click picks one to edit in the right pane.
    pub(super) fn buildup_lists(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let lang = super::library::lang(l);
        let view = &self.buildup_view;
        let shown = view.shown();
        let roles = row(Role::ALL.map(|role| {
            let on = role == shown;
            let count = self.library.buildups_of(role).count();
            button(text(format!("{} · {count}", role_title(role, l))).size(12).font(if on { FONT_BOLD } else { FONT }).color(if on { p.text } else { p.text_secondary }))
                .padding([3, 8])
                .style(move |_, s| p.ribbon_button(on, s))
                .on_press(BuildUpMessage::Open(role))
                .into()
        }))
        .spacing(4);
        let mut list = Column::new().spacing(1);
        for (id, name) in self.library.names(shown, lang) {
            let on = view.picked.as_deref() == Some(id);
            let thickness = self.library.buildup(id).map_or(0.0, BuildUp::thickness);
            let line = row![
                // (a long name stays in its own part of the line)
                container(text(name).size(12).font(if on { FONT_BOLD } else { FONT }).color(if on { p.text } else { p.text_secondary }).wrapping(Wrapping::None)).width(Fill).clip(true),
                text(format!("{} mm", bim_library::millimetres(thickness, lang))).size(12).color(p.text_secondary),
            ]
            .spacing(8)
            .align_y(Center);
            list = list.push(button(line).width(Fill).padding([4, 8]).style(move |_, s| if on { p.ribbon_button(true, s) } else { p.row_button(s) }).on_press(BuildUpMessage::Pick(id.to_string())));
        }
        let add = button(text(l.text("+ Opbouw", "+ Build-up")).size(12).color(p.accent)).padding([4, 8]).style(move |_, s| p.row_button(s)).on_press(BuildUpMessage::Add);
        let framed = container(column![roles, list.push(add)].spacing(8))
            .width(Fill)
            .padding(10)
            .clip(true)
            .style(move |_| container::Style { background: Some(p.surface.into()), border: iced::Border { color: p.border, width: 1.0, radius: 6.0.into() }, ..Default::default() });
        let section = column![text(l.text("Opbouwen", "Build-ups")).size(12).font(FONT_BOLD).color(p.text_muted), framed].spacing(8);
        Element::from(section).map(Message::BuildUp)
    }

    /// The right pane of the Project page: the editor of the picked build-up.
    pub fn buildup_pane(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let lang = super::library::lang(l);
        let view = &self.buildup_view;
        let library = &self.library;
        let Some(b) = view.picked.as_deref().and_then(|id| library.buildup(id)) else {
            let hint = l.text("Kies een wand, vloer, dak of plafond op de pagina om de opbouw te bewerken.", "Pick a wall, floor, roof or ceiling on the page to edit its build-up.");
            return container(text(hint).size(12).color(p.text_muted).wrapping(Wrapping::Word)).padding(8).into();
        };
        let title = library.name(&b.id, lang).unwrap_or_default();
        let side = if b.role.counted_from_top() { l.text("Bovenzijde boven", "Top side up") } else { l.text("Buitenzijde boven", "Exterior side up") };
        let materials: Vec<Choice> = library.materials.iter().map(|m| Choice { key: m.key.clone(), label: m.name(lang).to_string() }).collect();
        let small = |glyph: &'static str, name: &'static str, message: BuildUpMessage| -> Element<'_, BuildUpMessage> {
            a11y::named(button(text(glyph).size(11).color(p.text_secondary)).padding([2, 5]).style(move |_, s| p.row_button(s)).on_press(message), Aria::Button, name)
        };
        let mut layers = Column::new().spacing(3);
        for (k, layer) in b.layers.iter().enumerate() {
            let now = materials.iter().find(|c| c.key == layer.material).cloned();
            let material = a11y::named(
                pick_list(materials.clone(), now, move |c: Choice| BuildUpMessage::Material(k, c.key)).text_size(12).padding([2, 6]).width(Fill),
                Aria::ComboBox,
                format!("{} {}", l.text("Materiaal van laag", "Material of layer"), k + 1),
            );
            let shown = match &view.draft {
                Some((at, typed)) if *at == k => typed.clone(),
                _ => bim_library::millimetres(layer.thickness, lang),
            };
            let thickness = a11y::named(
                text_input("", &shown).on_input(move |typed| BuildUpMessage::Typed(k, typed)).on_submit(BuildUpMessage::Done).size(12).padding([2, 6]).width(58),
                Aria::TextField,
                format!("{} {}", l.text("Dikte in mm van laag", "Thickness in mm of layer"), k + 1),
            );
            layers = layers.push(
                row![
                    small("▲", l.text("Laag omhoog", "Layer up"), BuildUpMessage::Move(k, true)),
                    small("▼", l.text("Laag omlaag", "Layer down"), BuildUpMessage::Move(k, false)),
                    material,
                    thickness,
                    small("+", l.text("Laag erbij", "A layer more"), BuildUpMessage::AddLayer(k)),
                    small("×", l.text("Laag weg", "Remove layer"), BuildUpMessage::RemoveLayer(k)),
                ]
                .spacing(3)
                .align_y(Center),
            );
        }
        let total = format!("{}: {} mm · {} {}", l.text("Totaal", "Total"), bim_library::millimetres(b.thickness(), lang), b.role.ifc_type_class(), b.role.predefined_type());
        let own = a11y::named(
            row![
                text(l.text("Eigen naam", "Own name")).size(12).color(p.text_secondary).width(Length::FillPortion(2)),
                text_input(l.text("volgens de lagen", "by its layers"), b.name.as_deref().unwrap_or_default()).on_input(BuildUpMessage::Named).size(12).padding([2, 6]).width(Length::FillPortion(3)),
            ]
            .spacing(8)
            .align_y(Center),
            Aria::Label,
            l.text("Eigen naam", "Own name"),
        );
        let act = |label: &'static str, message: BuildUpMessage| button(text(label).size(12).color(p.text_secondary)).padding([3, 8]).style(move |_, s| p.row_button(s)).on_press(message);
        let refused = view.refused.as_ref().map(|why| {
            let refused = l.text("Niet overgenomen: de opbouw zou dan niet meer kloppen.", "Not taken: the build-up would no longer hold together.");
            text(format!("{refused} ({why})")).size(11).color(p.accent).wrapping(Wrapping::Word)
        });
        // what is edited, under the drawing of what it makes
        let editor = column![
            layers,
            text(total).size(11).color(p.text_secondary),
            own,
            row![act(l.text("Dupliceren", "Duplicate"), BuildUpMessage::Duplicate), act(l.text("Verwijderen", "Remove"), BuildUpMessage::Remove), Space::new().width(Fill)].spacing(4),
        ]
        .push(refused)
        .push(use_note(p, l, &self.type_use(TypeRef::BuildUp(&b.id)), &self.follow_refused, BuildUpMessage::Follow))
        .spacing(8);
        let pane = column![
            text(title).size(14).font(FONT_BOLD).color(p.text).wrapping(Wrapping::Word),
            super::library::buildup_section(p, library, b),
            text(side).size(11).color(p.text_muted),
            Element::from(editor).map(Message::BuildUp),
        ]
        .spacing(8);
        container(pane).padding(8).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bim_library::Lang;

    #[test]
    fn a_build_up_is_edited_layer_by_layer() {
        let mut app = App::default();
        let walls = app.library.buildups_of(Role::Wall).count();
        // a wall more: one layer, picked to be edited
        let _ = app.buildup_update(BuildUpMessage::Add);
        let id = app.buildup_view.picked.clone().expect("the new build-up is picked");
        assert_eq!((app.library.buildups_of(Role::Wall).count(), app.library.buildup(&id).map(|b| b.layers.len())), (walls + 1, Some(1)));
        // its layer: a material and a thickness typed in millimetres, taken at Enter
        let _ = app.buildup_update(BuildUpMessage::Material(0, "brick".into()));
        let _ = app.buildup_update(BuildUpMessage::Typed(0, "10".into()));
        assert_eq!(app.library.buildup(&id).unwrap().layers[0].thickness, 0.1, "not before Enter");
        let _ = app.buildup_update(BuildUpMessage::Typed(0, "102,5".into()));
        let _ = app.buildup_update(BuildUpMessage::Done);
        assert_eq!(app.library.buildup(&id).unwrap().layers[0], Layer::new("brick", 0.1025));
        // layers more, each like the one before it, then changed and moved
        let _ = app.buildup_update(BuildUpMessage::AddLayer(0));
        let _ = app.buildup_update(BuildUpMessage::AddLayer(1));
        let _ = app.buildup_update(BuildUpMessage::Material(2, "concrete".into()));
        let _ = app.buildup_update(BuildUpMessage::Move(2, true));
        let _ = app.buildup_update(BuildUpMessage::Move(0, true));
        let materials: Vec<&str> = app.library.buildup(&id).unwrap().layers.iter().map(|l| l.material.as_str()).collect();
        assert_eq!(materials, ["brick", "concrete", "brick"]);
        // its name follows its layers, until it is given one of its own
        let rule = app.library.name(&id, Lang::En).unwrap();
        assert!(rule.to_lowercase().contains("concrete") && rule.ends_with("307.5"), "{rule}");
        let _ = app.buildup_update(BuildUpMessage::Named("Gevel type A".into()));
        assert_eq!(app.library.name(&id, Lang::En).as_deref(), Some("Gevel type A"));
        let _ = app.buildup_update(BuildUpMessage::Named(" ".into()));
        assert_eq!(app.library.name(&id, Lang::En), Some(rule));
        // what does not check is refused: a thickness of nothing, a material the library lacks,
        // the last layer taken away
        let _ = app.buildup_update(BuildUpMessage::Typed(1, "0".into()));
        let _ = app.buildup_update(BuildUpMessage::Done);
        assert!(app.buildup_view.refused.is_some() && app.buildup_view.draft.is_some());
        let _ = app.buildup_update(BuildUpMessage::Material(1, "unobtainium".into()));
        assert_eq!(app.library.buildup(&id).unwrap().layers[1].material, "concrete");
        let _ = app.buildup_update(BuildUpMessage::RemoveLayer(2));
        let _ = app.buildup_update(BuildUpMessage::RemoveLayer(1));
        let _ = app.buildup_update(BuildUpMessage::RemoveLayer(0));
        assert_eq!(app.library.buildup(&id).unwrap().layers.len(), 1, "its last layer stays");
        // a copy to change, and the build-up gone again
        let _ = app.buildup_update(BuildUpMessage::Duplicate);
        let copy = app.buildup_view.picked.clone().unwrap();
        assert_ne!(copy, id);
        assert_eq!(app.library.buildup(&copy).unwrap().layers, app.library.buildup(&id).unwrap().layers);
        let _ = app.buildup_update(BuildUpMessage::Remove);
        assert!(app.library.buildup(&copy).is_none() && app.buildup_view.picked.is_none());
    }

    #[test]
    fn a_build_up_of_another_library_is_edited_as_a_copy_in_the_project() {
        let mut app = App::default();
        let inb = bim_library::inb::library();
        let template = inb.buildups_of(Role::Floor).next().expect("a floor of the template").clone();
        let floors = app.library.buildups_of(Role::Floor).count();
        let _ = app.libraries_update(super::super::libraries::LibrariesMessage::Search("anhydriet".into()));
        let _ = app.buildup_update(BuildUpMessage::Copy(Shelf::Netherlands, template.id.clone()));
        let id = app.buildup_view.picked.clone().expect("the copy is picked");
        let copy = app.library.buildup(&id).unwrap();
        assert_eq!((copy.role, &copy.layers, &copy.name), (Role::Floor, &template.layers, &template.name));
        assert_eq!(app.library.buildups_of(Role::Floor).count(), floors + 1);
        // every material its layers need came with it, and it shows on the project's page
        assert_eq!(copy.check(&app.library.materials), Ok(()));
        assert_eq!((app.libraries.source, app.libraries.query.as_str(), app.buildup_view.shown()), (Source::Project, "", Role::Floor));
        // the template's own stays as it was
        assert_eq!(bim_library::inb::library().buildup(&template.id), Some(&template));
    }

    #[test]
    fn the_project_page_lists_the_build_ups_and_the_pane_edits_the_picked_one() {
        let mut app = App::default();
        let _ = app.libraries_update(super::super::libraries::LibrariesMessage::Source(Source::Project));
        let _ = app.buildup_update(BuildUpMessage::Open(Role::Floor));
        {
            let mut page = crate::app::test_ui_of(iced::Size::new(1200.0, 900.0), app.buildup_lists(app.theme.palette(), Language::English));
            assert!(page.find("Build-ups").is_ok() && page.find("+ Build-up").is_ok() && page.find("Concrete 200").is_ok());
        }
        {
            let mut pane = crate::app::test_ui_of(iced::Size::new(340.0, 700.0), app.buildup_pane(app.theme.palette(), Language::English));
            assert!(pane.find("Pick a wall, floor, roof or ceiling on the page to edit its build-up.").is_ok());
        }
        let id = app.library.buildups_of(Role::Floor).next().unwrap().id.clone();
        let _ = app.buildup_update(BuildUpMessage::Pick(id));
        let mut pane = crate::app::test_ui_of(iced::Size::new(340.0, 700.0), app.buildup_pane(app.theme.palette(), Language::English));
        assert!(pane.find("Top side up").is_ok() && pane.find("Duplicate").is_ok() && pane.find("Own name").is_ok());
    }
}
