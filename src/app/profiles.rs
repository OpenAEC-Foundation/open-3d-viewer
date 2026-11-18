//! The profile editor of the Library product: the types of columns, beams and other linear
//! elements of the project's library.
//!
//! A profile is one of IFC's parametric profile definitions, and the editor works on it that
//! way: the definition by its IFC name (`IfcIShapeProfileDef`) and its attributes by theirs
//! (`OverallDepth`, `WebThickness`), as `bim_library::ProfileShape::ifc` gives them, typed in
//! millimetres (a slope in degrees). The outline is drawn above them, the area and second moments
//! under them. A type also has a role (and so its IFC class) and a material.
//!
//! The Project page lists the types a role at a time, each with a look at its profile, and under
//! them the steel table: found by typing or by series, every profile drawn. A click on one gives
//! it to the picked type, or makes a type of it. A change is kept only when the type still checks.
//!
//! An element that was given a type holds what it was given: changing the type here does not
//! change the elements made of it.

use super::libraries::{Editor, LibrariesView, Shelf, Source};
use super::{App, Message, FONT, FONT_BOLD};
use crate::theme::Palette;
use bim_library::profile::{steel, steel_profiles};
use bim_library::{Lang, Library, LibraryError, LinearRole, LinearType, ProfileShape, Series, SteelProfile};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, pick_list, row, text, text_input, Column, Space};
use iced::{Center, Element, Fill, Length, Task};
use openaec_ui::design::a11y::{self, Role as Aria};
use openaec_ui::Language;

/// Whether an attribute of a profile definition is an angle, typed in degrees; the others are
/// lengths, typed in millimetres.
fn angle(attribute: &str) -> bool {
    attribute.ends_with("Slope")
}

/// A number with at most two decimals, as the language writes it.
fn decimal(value: f64, lang: Lang) -> String {
    let text = format!("{value:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    let text = if text == "-0" { "0" } else { text };
    if lang.decimal_comma() {
        text.replace('.', ",")
    } else {
        text.to_string()
    }
}

/// An attribute's value as it is typed: millimetres, or degrees.
fn shown(attribute: &str, value: f64, lang: Lang) -> String {
    decimal(if angle(attribute) { value.to_degrees() } else { value * 1000.0 }, lang)
}

/// What was typed as an attribute's value: metres, or radians.
fn taken(attribute: &str, typed: &str) -> Option<f64> {
    let number = super::standards::number(typed)?;
    Some(if angle(attribute) { number.to_radians() } else { number / 1000.0 })
}

/// A profile of IFC definition `definition` about as wide and deep as `shape`, its walls a
/// twelfth of its smaller side: where a change of definition starts from.
pub fn like(shape: &ProfileShape, definition: &str) -> Option<ProfileShape> {
    let tenths = |metres: f64| (metres * 10_000.0).round() / 10_000.0;
    let size = shape.size();
    let (b, h) = (size.x, size.y);
    let t = tenths(b.min(h) / 12.0).max(0.0005);
    let tf = tenths(1.5 * t);
    Some(match definition {
        "IfcRectangleProfileDef" => ProfileShape::Rectangle { width: b, depth: h },
        "IfcCircleProfileDef" => ProfileShape::Circle { diameter: b },
        "IfcIShapeProfileDef" => ProfileShape::I { h, b, tw: t, tf, r: t, slope: 0.0 },
        "IfcUShapeProfileDef" => ProfileShape::U { h, b, tw: t, tf, r: t, slope: 0.0 },
        "IfcLShapeProfileDef" => ProfileShape::L { h, b, t, r1: t, r2: t / 2.0 },
        "IfcTShapeProfileDef" => ProfileShape::T { h, b, tw: t, tf, r: t },
        "IfcRectangleHollowProfileDef" => ProfileShape::Rhs { h, b, t, r_out: tf, r_in: t },
        "IfcCircleHollowProfileDef" => ProfileShape::Chs { d: b, t },
        _ => return None,
    })
}

#[derive(Debug, Clone)]
pub enum ProfileMessage {
    /// Show the types of this role on the Project page.
    Open(LinearRole),
    /// Edit this type of the project.
    Pick(String),
    /// A type more of the role shown, a rectangle to start with.
    Add,
    Duplicate,
    Remove,
    /// A name of its own, in place of the rule's; empty for the rule's.
    Named(String),
    Role(LinearRole),
    /// The material, by its key.
    Material(String),
    /// Another of IFC's profile definitions, about the size of the one it had.
    Definition(&'static str),
    /// Typed as the value of an attribute of the profile definition.
    Typed(usize, String),
    /// Enter: the typed value is taken.
    Done,
    /// Typed to find a profile of the steel table.
    Find(String),
    /// Show one series of the steel table (by its name in a designation), or all.
    Series(Option<&'static str>),
    /// A profile of the steel table is clicked: the picked type takes it; with none picked, a
    /// type of the role shown is made of it.
    Steel(String),
    /// From a search: a beam of this profile of the steel table, in the project.
    FromSteel(String),
    /// Edit a type of another library as a copy in the project.
    Copy(Shelf, String),
}

/// Which role's types show, which one is edited, what is being typed, what of the steel table
/// shows, and why the last change was refused.
#[derive(Debug, Clone, Default)]
pub struct ProfileView {
    pub role: Option<LinearRole>,
    pub picked: Option<String>,
    pub draft: Option<(usize, String)>,
    pub find: String,
    pub series: Option<&'static str>,
    pub refused: Option<String>,
}

impl ProfileView {
    /// The role whose types show: columns until another is opened.
    pub fn shown(&self) -> LinearRole {
        self.role.unwrap_or(LinearRole::Column)
    }
}

/// Changes linear type `id` of `library`; it stays as it was when it would no longer check.
pub fn change(library: &mut Library, id: &str, change: impl FnOnce(&mut LinearType)) -> Result<(), LibraryError> {
    let at = library.linear_types.iter().position(|t| t.id == id).ok_or(LibraryError::NoKey)?;
    let mut next = library.linear_types[at].clone();
    change(&mut next);
    next.check(&library.materials)?;
    library.linear_types[at] = next;
    Ok(())
}

/// Sets attribute `at` of the profile definition of type `id` to what was typed. A profile that
/// changes is no profile of the steel table any more.
pub fn retype(library: &mut Library, id: &str, at: usize, typed: &str) -> Result<(), LibraryError> {
    let no = || LibraryError::BadProfile(typed.trim().to_string());
    let (definition, attributes) = library.linear_type(id).ok_or(LibraryError::NoKey)?.profile.ifc();
    let (name, _) = *attributes.get(at).ok_or_else(no)?;
    let mut values: Vec<f64> = attributes.iter().map(|(_, value)| *value).collect();
    values[at] = taken(name, typed).ok_or_else(no)?;
    let profile = ProfileShape::from_ifc(definition, &values).ok_or_else(no)?;
    change(library, id, |t| {
        if !t.profile.same(&profile) {
            t.profile = profile;
            t.designation = None;
        }
    })
}

/// An id no linear type of the library has: "own.column.1".
fn fresh_id(library: &Library, role: LinearRole) -> String {
    let role = role.name().to_lowercase().replace(' ', "-");
    (1..).map(|n| format!("own.{role}.{n}")).find(|id| library.linear_type(id).is_none()).unwrap_or_default()
}

/// A type more of `role`: a rectangle of 200 by 200 mm of the library's first material; its id.
pub fn add(library: &mut Library, role: LinearRole) -> Result<String, LibraryError> {
    let material = library.materials.first().map(|m| m.key.clone()).ok_or(LibraryError::NoKey)?;
    let id = fresh_id(library, role);
    library.add_linear_type(LinearType::new(&id, role, &material, ProfileShape::Rectangle { width: 0.2, depth: 0.2 }))?;
    Ok(id)
}

/// A copy of type `id`, right after it; the copy's id.
pub fn duplicate(library: &mut Library, id: &str) -> Result<String, LibraryError> {
    let at = library.linear_types.iter().position(|t| t.id == id).ok_or(LibraryError::NoKey)?;
    let mut copy = library.linear_types[at].clone();
    copy.id = fresh_id(library, copy.role);
    let new = copy.id.clone();
    library.linear_types.insert(at + 1, copy);
    Ok(new)
}

/// A copy of type `id` of `from` in `library`, with the material it needs; its id.
pub fn copy(library: &mut Library, from: &Library, id: &str) -> Result<String, LibraryError> {
    let source = from.linear_type(id).ok_or(LibraryError::NoKey)?;
    if library.material(&source.material).is_none() {
        if let Some(material) = from.material(&source.material) {
            library.add_material(material.clone())?;
        }
    }
    let mut fresh = source.clone();
    fresh.id = fresh_id(library, source.role);
    let new = fresh.id.clone();
    library.add_linear_type(fresh)?;
    Ok(new)
}

/// Type `id` takes profile `designation` of the steel table: its shape, its designation, steel.
pub fn take_steel(library: &mut Library, id: &str, designation: &str) -> Result<(), LibraryError> {
    let table = steel(designation).ok_or_else(|| LibraryError::BadProfile(designation.to_string()))?;
    change(library, id, |t| {
        t.profile = table.shape.clone();
        t.designation = Some(table.designation.clone());
        t.material = "steel".into();
    })
}

/// The type of `role` of steel profile `designation`: the one the library has, or one made of it.
pub fn from_steel(library: &mut Library, role: LinearRole, designation: &str) -> Result<String, LibraryError> {
    if let Some(have) = library.linear_types.iter().find(|t| t.role == role && t.designation.as_deref() == Some(designation)) {
        return Ok(have.id.clone());
    }
    let id = fresh_id(library, role);
    let made = LinearType::steel(&id, role, designation).ok_or_else(|| LibraryError::BadProfile(designation.to_string()))?;
    library.add_linear_type(made)?;
    Ok(id)
}

/// The profiles of the steel table of `series` (all without one) whose designation holds what
/// was typed, a space or a capital more or less.
pub fn found_steel(typed: &str, series: Option<&str>) -> Vec<&'static SteelProfile> {
    let plain = |text: &str| text.to_lowercase().replace(' ', "");
    let needle = plain(typed);
    steel_profiles().iter().filter(|s| series.is_none_or(|name| s.series.name() == name)).filter(|s| needle.is_empty() || plain(&s.designation).contains(&needle)).collect()
}

/// One of a list to pick from.
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

/// A button that is on or off: a role, a series.
fn chip<'a>(p: Palette, label: String, on: bool, message: ProfileMessage) -> Element<'a, ProfileMessage> {
    button(text(label).size(12).font(if on { FONT_BOLD } else { FONT }).color(if on { p.text } else { p.text_secondary }))
        .padding([3, 8])
        .style(move |_, s| p.ribbon_button(on, s))
        .on_press(message)
        .into()
}

/// A part of the page in a frame of its own.
fn framed<'a>(p: Palette, content: impl Into<Element<'a, ProfileMessage>>) -> Element<'a, ProfileMessage> {
    container(content)
        .width(Fill)
        .padding(10)
        .clip(true)
        .style(move |_| container::Style { background: Some(p.surface.into()), border: iced::Border { color: p.border, width: 1.0, radius: 6.0.into() }, ..Default::default() })
        .into()
}

/// A field of the right pane that needs the pane's width (an IFC name is long): its label, and
/// under it what sets it.
fn stacked<'a>(p: Palette, label: &'static str, control: Element<'a, ProfileMessage>) -> Element<'a, ProfileMessage> {
    column![text(label).size(11).color(p.text_muted), control].spacing(2).into()
}

/// A field of the right pane: its label, and beside it what sets it.
fn labelled<'a>(p: Palette, label: &'static str, control: Element<'a, ProfileMessage>) -> Element<'a, ProfileMessage> {
    row![text(label).size(12).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph).width(Length::FillPortion(1)), container(control).width(Length::FillPortion(1))].spacing(8).align_y(Center).into()
}

impl App {
    pub(super) fn profile_update(&mut self, message: ProfileMessage) -> Task<Message> {
        match self.profile_apply(message) {
            Some(Ok(())) => {
                self.profile_view.draft = None;
                self.profile_view.refused = None;
                // what the model shows of its types (their names) is made again
                if self.modeller.shown.is_some() && self.modeller.model.iter().next().is_some() {
                    self.show_modelled();
                }
            }
            Some(Err(why)) => self.profile_view.refused = Some(why.to_string()),
            None => {}
        }
        if self.profile_view.picked.is_some() {
            self.edit_only(Editor::Profile);
        }
        Task::none()
    }

    /// What a message does: nothing to tell about (`None`), or a change that was taken or refused.
    fn profile_apply(&mut self, message: ProfileMessage) -> Option<Result<(), LibraryError>> {
        let picked = self.profile_view.picked.clone();
        match message {
            ProfileMessage::Open(role) => {
                let view = &mut self.profile_view;
                *view = ProfileView { role: Some(role), find: std::mem::take(&mut view.find), series: view.series, ..ProfileView::default() };
                None
            }
            ProfileMessage::Pick(id) => {
                self.profile_show(id);
                None
            }
            ProfileMessage::Find(typed) => {
                self.profile_view.find = typed;
                None
            }
            ProfileMessage::Series(series) => {
                self.profile_view.series = series;
                None
            }
            ProfileMessage::Typed(attribute, typed) => {
                self.profile_view.draft = Some((attribute, typed));
                None
            }
            ProfileMessage::Add => Some(add(&mut self.library, self.profile_view.shown()).map(|id| self.profile_view.picked = Some(id))),
            ProfileMessage::Duplicate => picked.as_deref().map(|id| duplicate(&mut self.library, id).map(|new| self.profile_view.picked = Some(new))),
            ProfileMessage::Remove => {
                if let Some(id) = &picked {
                    self.library.linear_types.retain(|t| &t.id != id);
                    self.profile_view.picked = None;
                }
                Some(Ok(()))
            }
            ProfileMessage::Named(name) => picked.as_deref().map(|id| change(&mut self.library, id, |t| t.name = (!name.trim().is_empty()).then_some(name))),
            ProfileMessage::Role(role) => picked.as_deref().map(|id| change(&mut self.library, id, |t| t.role = role).map(|()| self.profile_view.role = Some(role))),
            ProfileMessage::Material(key) => picked.as_deref().map(|id| change(&mut self.library, id, |t| t.material = key)),
            ProfileMessage::Definition(definition) => picked.as_deref().map(|id| {
                let next = self.library.linear_type(id).filter(|t| t.profile.ifc().0 != definition).and_then(|t| like(&t.profile, definition));
                match next {
                    // (it is no profile of the steel table any more)
                    Some(profile) => change(&mut self.library, id, |t| {
                        t.profile = profile;
                        t.designation = None;
                    }),
                    None => Ok(()),
                }
            }),
            ProfileMessage::Done => match (picked.as_deref(), self.profile_view.draft.clone()) {
                (Some(id), Some((attribute, typed))) => Some(retype(&mut self.library, id, attribute, &typed)),
                _ => None,
            },
            ProfileMessage::Steel(designation) => Some(self.steel_material().and_then(|()| match picked.as_deref() {
                Some(id) => take_steel(&mut self.library, id, &designation),
                None => from_steel(&mut self.library, self.profile_view.shown(), &designation).map(|id| self.profile_view.picked = Some(id)),
            })),
            ProfileMessage::FromSteel(designation) => Some(self.steel_material().and_then(|()| from_steel(&mut self.library, LinearRole::Beam, &designation)).map(|id| self.profile_show(id))),
            ProfileMessage::Copy(shelf, id) => {
                let from = self.shelf(shelf).clone();
                Some(copy(&mut self.library, &from, &id).map(|new| self.profile_show(new)))
            }
        }
    }

    /// A type of the project in its editor, on the project's page (from the page or a search).
    fn profile_show(&mut self, id: String) {
        let role = self.library.linear_type(&id).map(|t| t.role).or(self.profile_view.role);
        let view = &mut self.profile_view;
        *view = ProfileView { role, picked: Some(id), find: std::mem::take(&mut view.find), series: view.series, ..ProfileView::default() };
        self.libraries = LibrariesView { source: Source::Project, country: self.libraries.country, query: String::new() };
    }

    /// The project has the steel a profile of the steel table is made of: the general library's,
    /// when it had none.
    fn steel_material(&mut self) -> Result<(), LibraryError> {
        if self.library.material("steel").is_none() {
            if let Some(steel) = self.shelf(Shelf::General).material("steel").cloned() {
                self.library.add_material(steel)?;
            }
        }
        Ok(())
    }

    /// The linear types of the project on its page, a role at a time, each with a look at its
    /// profile and its IFC definition. A click picks one to edit in the right pane.
    pub(super) fn profile_lists(&self, p: Palette, l: Language) -> Element<'_, Message> {
        use super::library::{linear_role_label, profile_outline};
        let lang = super::library::lang(l);
        let view = &self.profile_view;
        let library = &self.library;
        let shown = view.shown();
        let roles = row(LinearRole::ALL.map(|role| chip(p, format!("{} · {}", linear_role_label(role, l), library.linear_types_of(role).count()), role == shown, ProfileMessage::Open(role)))).spacing(4).wrap();
        let mut list = Column::new().spacing(1);
        for (id, name) in library.linear_names(shown, lang) {
            let Some(t) = library.linear_type(id) else { continue };
            let on = view.picked.as_deref() == Some(id);
            let line = row![
                container(profile_outline(p, &t.profile, library.material(&t.material), 22.0)).width(30),
                // (a long name stays in its own part of the line)
                container(text(name).size(12).font(if on { FONT_BOLD } else { FONT }).color(if on { p.text } else { p.text_secondary }).wrapping(Wrapping::None)).width(Fill).clip(true),
                text(t.profile.ifc().0).size(11).color(p.text_muted),
            ]
            .spacing(8)
            .align_y(Center);
            list = list.push(button(line).width(Fill).padding([3, 8]).style(move |_, s| if on { p.ribbon_button(true, s) } else { p.row_button(s) }).on_press(ProfileMessage::Pick(id.to_string())));
        }
        let add = button(text(l.text("+ Staaftype", "+ Linear type")).size(12).color(p.accent)).padding([4, 8]).style(move |_, s| p.row_button(s)).on_press(ProfileMessage::Add);
        let types = column![text(l.text("Staaftypen", "Linear types")).size(12).font(FONT_BOLD).color(p.text_muted), framed(p, column![roles, list.push(add)].spacing(8))].spacing(8);
        Element::from(types).map(Message::Profile)
    }

    /// The steel table on the project's page: a field to find by, its series, and every profile
    /// found as a tile with its outline. A click on a tile gives that profile to the picked type,
    /// or makes a type of it of the role shown.
    pub(super) fn steel_table(&self, p: Palette, l: Language) -> Element<'_, Message> {
        use super::library::profile_outline;
        /// How many profiles show at once.
        const SHOWN: usize = 48;
        let lang = super::library::lang(l);
        let view = &self.profile_view;
        let library = &self.library;
        let picked = view.picked.as_deref().and_then(|id| library.linear_type(id));
        let find_name = l.text("Zoek een staalprofiel: HEA 200", "Find a steel profile: HEA 200");
        let find = a11y::named(text_input(find_name, &view.find).on_input(ProfileMessage::Find).size(12).padding([4, 8]).width(280), Aria::TextField, find_name);
        let mut names: Vec<&'static str> = Vec::new();
        for series in Series::ALL {
            if !names.contains(&series.name()) {
                names.push(series.name());
            }
        }
        let all = chip(p, l.text("Alle", "All").to_string(), view.series.is_none(), ProfileMessage::Series(None));
        let series = row(std::iter::once(all).chain(names.into_iter().map(|name| chip(p, name.to_string(), view.series == Some(name), ProfileMessage::Series(Some(name)))))).spacing(4).wrap();
        let found = found_steel(&view.find, view.series);
        let steel_material = library.material("steel").or_else(|| self.shelf(Shelf::General).material("steel"));
        let tiles = row(found.iter().take(SHOWN).map(|table| {
            let on = picked.is_some_and(|t| t.designation.as_deref() == Some(table.designation.as_str()));
            let size = table.shape.size();
            let tile = column![
                profile_outline(p, &table.shape, steel_material, 56.0),
                text(table.designation.as_str()).size(11).font(if on { FONT_BOLD } else { FONT }).color(p.text).wrapping(Wrapping::None),
                text(format!("{} × {}", bim_library::millimetres(size.y, lang), bim_library::millimetres(size.x, lang))).size(10).color(p.text_muted),
            ]
            .spacing(2)
            .align_x(Center)
            .width(108);
            button(tile).padding(6).style(move |_, s| if on { p.ribbon_button(true, s) } else { p.row_button(s) }).on_press(ProfileMessage::Steel(table.designation.clone())).into()
        }))
        .spacing(6)
        .wrap();
        let what = if picked.is_some() {
            l.text("Klik een profiel: het gekozen staaftype neemt het over.", "Click a profile: the picked linear type takes it.")
        } else {
            l.text("Klik een profiel om er een staaftype van te maken.", "Click a profile to make a linear type of it.")
        };
        let rest = match found.len() {
            0 => Some(text(l.text("Niets gevonden.", "Nothing found.")).size(12).color(p.text_secondary)),
            n if n > SHOWN => Some(text(format!("+{}", n - SHOWN)).size(11).color(p.text_muted)),
            _ => None,
        };
        let table = column![find, series, text(what).size(11).color(p.text_muted), tiles].push(rest).spacing(8);
        let steel = column![text(format!("{} · {}", l.text("Staalprofielen", "Steel profiles"), found.len())).size(12).font(FONT_BOLD).color(p.text_muted), framed(p, table)].spacing(8);
        Element::from(steel).map(Message::Profile)
    }

    /// The right pane of the Project page while a linear type is picked: its profile editor.
    pub fn profile_pane(&self, p: Palette, l: Language) -> Element<'_, Message> {
        use super::library::{linear_role_label, profile_outline};
        let lang = super::library::lang(l);
        let view = &self.profile_view;
        let library = &self.library;
        let Some(t) = view.picked.as_deref().and_then(|id| library.linear_type(id)) else {
            let hint = l.text("Kies een staaftype op de pagina om het profiel te bewerken.", "Pick a linear type on the page to edit its profile.");
            return container(text(hint).size(12).color(p.text_muted).wrapping(Wrapping::Word)).padding(8).into();
        };
        let title = library.linear_name(&t.id, lang).unwrap_or_default();
        let (definition, attributes) = t.profile.ifc();
        let (_, class, predefined) = t.role.ifc();
        let roles: Vec<Choice> = LinearRole::ALL.iter().map(|role| Choice { key: role.name().into(), label: format!("{} · {}", linear_role_label(*role, l), role.ifc().0) }).collect();
        let role_now = roles.iter().find(|c| c.key == t.role.name()).cloned();
        let materials: Vec<Choice> = library.materials.iter().map(|m| Choice { key: m.key.clone(), label: m.name(lang).to_string() }).collect();
        let material_now = materials.iter().find(|c| c.key == t.material).cloned();
        let (role_name, material_name, profile_name) = (l.text("Rol", "Role"), l.text("Materiaal", "Material"), l.text("Profiel", "Profile"));
        let role = pick_list(roles, role_now, |c: Choice| ProfileMessage::Role(LinearRole::ALL.into_iter().find(|r| r.name() == c.key).unwrap_or(LinearRole::Column))).text_size(12).padding([2, 6]).width(Fill);
        let material = pick_list(materials, material_now, |c: Choice| ProfileMessage::Material(c.key)).text_size(12).padding([2, 6]).width(Fill);
        let profile = pick_list(ProfileShape::IFC.to_vec(), Some(definition), ProfileMessage::Definition).text_size(12).padding([2, 6]).width(Fill);
        let mut fields = Column::new()
            .spacing(6)
            .push(stacked(p, role_name, a11y::named(role, Aria::ComboBox, role_name)))
            .push(stacked(p, material_name, a11y::named(material, Aria::ComboBox, material_name)))
            .push(stacked(p, profile_name, a11y::named(profile, Aria::ComboBox, profile_name)));
        // the attributes of its definition, named as IFC names them
        for (k, (name, value)) in attributes.iter().copied().enumerate() {
            let now = match &view.draft {
                Some((at, typed)) if *at == k => typed.clone(),
                _ => shown(name, value, lang),
            };
            let input = a11y::named(text_input("", &now).on_input(move |typed| ProfileMessage::Typed(k, typed)).on_submit(ProfileMessage::Done).size(12).padding([2, 6]), Aria::TextField, name);
            let unit = text(if angle(name) { "°" } else { "mm" }).size(11).color(p.text_muted).width(22);
            fields = fields.push(labelled(p, name, row![input, unit].spacing(4).align_y(Center).into()));
        }
        if let Ok(props) = t.profile.props() {
            fields = fields.push(text(format!("A {:.1} cm² · Iy {:.0} cm⁴ · Iz {:.0} cm⁴", props.area * 1e4, props.iu * 1e8, props.iv * 1e8)).size(11).color(p.text_secondary));
        }
        if let Some(table) = t.designation.as_deref().and_then(steel) {
            fields = fields.push(text(format!("{}: {} · {}", l.text("Uit de staaltabel", "From the steel table"), table.designation, table.series.standard())).size(11).color(p.text_secondary));
        }
        let own_name = l.text("Eigen naam", "Own name");
        let own = text_input(l.text("volgens materiaal en maat", "by material and size"), t.name.as_deref().unwrap_or_default()).on_input(ProfileMessage::Named).size(12).padding([2, 6]);
        fields = fields.push(stacked(p, own_name, a11y::named(own, Aria::TextField, own_name)));
        let act = |label: &'static str, message: ProfileMessage| button(text(label).size(12).color(p.text_secondary)).padding([3, 8]).style(move |_, s| p.row_button(s)).on_press(message);
        let refused = view.refused.as_ref().map(|why| {
            let refused = l.text("Niet overgenomen: het profiel zou dan niet meer kloppen.", "Not taken: the profile would no longer hold together.");
            text(format!("{refused} ({why})")).size(11).color(p.accent).wrapping(Wrapping::Word)
        });
        let pane: Column<'_, ProfileMessage> = column![
            text(title).size(14).font(FONT_BOLD).color(p.text).wrapping(Wrapping::Word),
            text(format!("{class} · {predefined}")).size(11).color(p.text_muted),
            profile_outline(p, &t.profile, library.material(&t.material), 130.0),
            fields,
            row![act(l.text("Dupliceren", "Duplicate"), ProfileMessage::Duplicate), act(l.text("Verwijderen", "Remove"), ProfileMessage::Remove), Space::new().width(Fill)].spacing(4),
        ]
        .push(refused)
        .push(text(l.text("Elementen die dit type al kregen, houden wat ze kregen.", "Elements that were given this type keep what they were given.")).size(11).color(p.text_muted).wrapping(Wrapping::Word))
        .spacing(8);
        container(Element::from(pane).map(Message::Profile)).padding(8).into()
    }
}

#[cfg(test)]
mod tests {
    use super::super::libraries::LibrariesMessage;
    use super::*;

    #[test]
    fn a_profile_is_set_by_the_attributes_of_its_ifc_definition() {
        let mut app = App::default();
        let columns = app.library.linear_types_of(LinearRole::Column).count();
        // a column more: a rectangle to start with, picked to be edited
        let _ = app.profile_update(ProfileMessage::Add);
        let id = app.profile_view.picked.clone().expect("the new type is picked");
        let profile = |app: &App| app.library.linear_type(&id).unwrap().profile.clone();
        assert_eq!(app.library.linear_types_of(LinearRole::Column).count(), columns + 1);
        assert_eq!(profile(&app).ifc(), ("IfcRectangleProfileDef", vec![("XDim", 0.2), ("YDim", 0.2)]));
        // an attribute is typed in millimetres and taken at Enter
        let _ = app.profile_update(ProfileMessage::Typed(1, "450".into()));
        assert_eq!(profile(&app), ProfileShape::Rectangle { width: 0.2, depth: 0.2 }, "not before Enter");
        let _ = app.profile_update(ProfileMessage::Done);
        assert_eq!(profile(&app), ProfileShape::Rectangle { width: 0.2, depth: 0.45 });
        // another definition starts about as large; IFC has an I shape's width before its depth,
        // and a slope is typed in degrees
        let _ = app.profile_update(ProfileMessage::Definition("IfcIShapeProfileDef"));
        let ProfileShape::I { h, b, tw, .. } = profile(&app) else { panic!("an I shape") };
        assert_eq!((h, b), (0.45, 0.2));
        assert!(tw > 0.0);
        let _ = app.profile_update(ProfileMessage::Typed(1, "500".into()));
        let _ = app.profile_update(ProfileMessage::Done);
        let _ = app.profile_update(ProfileMessage::Typed(5, "2,5".into()));
        let _ = app.profile_update(ProfileMessage::Done);
        assert_eq!(app.profile_view.refused, None);
        assert!(matches!(profile(&app), ProfileShape::I { h, slope, .. } if h == 0.5 && (slope.to_degrees() - 2.5).abs() < 1e-9));
        // a circle is typed by its radius, as IFC has it
        let _ = app.profile_update(ProfileMessage::Definition("IfcCircleProfileDef"));
        let _ = app.profile_update(ProfileMessage::Typed(0, "150".into()));
        let _ = app.profile_update(ProfileMessage::Done);
        assert_eq!(profile(&app), ProfileShape::Circle { diameter: 0.3 });
        // its role (and so its IFC class), its material and a name of its own
        let _ = app.profile_update(ProfileMessage::Role(LinearRole::Pile));
        let _ = app.profile_update(ProfileMessage::Material("concrete".into()));
        let t = app.library.linear_type(&id).unwrap();
        assert_eq!((t.role, t.role.ifc().1, t.material.as_str()), (LinearRole::Pile, "IfcPileType", "concrete"));
        assert_eq!(app.profile_view.shown(), LinearRole::Pile, "the page follows it to its role");
        let _ = app.profile_update(ProfileMessage::Named("Prefab paal 300".into()));
        assert_eq!(app.library.linear_name(&id, Lang::En).as_deref(), Some("Prefab paal 300"));
        // what does not check is refused: a radius of nothing, no number, a material the
        // library lacks
        let _ = app.profile_update(ProfileMessage::Typed(0, "0".into()));
        let _ = app.profile_update(ProfileMessage::Done);
        assert!(app.profile_view.refused.is_some() && app.profile_view.draft.is_some());
        let _ = app.profile_update(ProfileMessage::Typed(0, "rond".into()));
        let _ = app.profile_update(ProfileMessage::Done);
        let _ = app.profile_update(ProfileMessage::Material("unobtainium".into()));
        let t = app.library.linear_type(&id).unwrap();
        assert_eq!((&t.profile, t.material.as_str()), (&ProfileShape::Circle { diameter: 0.3 }, "concrete"));
        // a copy to change, and the copy gone again
        let _ = app.profile_update(ProfileMessage::Duplicate);
        let copy = app.profile_view.picked.clone().unwrap();
        assert_ne!(copy, id);
        let _ = app.profile_update(ProfileMessage::Remove);
        assert!(app.library.linear_type(&copy).is_none() && app.library.linear_type(&id).is_some());
    }

    #[test]
    fn every_definition_starts_from_any_size() {
        for t in bim_library::profile::starter() {
            for definition in ProfileShape::IFC {
                let made = like(&t.profile, definition).expect(definition);
                assert_eq!(made.ifc().0, definition);
                assert_eq!(made.check(), Ok(()), "{definition} of {}", t.id);
            }
        }
        assert_eq!(like(&ProfileShape::Circle { diameter: 0.1 }, "IfcZShapeProfileDef"), None);
        // typed values show as they were typed: millimetres and degrees, at most two decimals
        assert_eq!((shown("Radius", 0.05715, Lang::En), shown("Radius", 0.05715, Lang::De)), ("57.15".to_string(), "57,15".to_string()));
        assert_eq!((shown("XDim", 0.2, Lang::Nl), shown("FlangeSlope", 0.14f64.atan(), Lang::En)), ("200".to_string(), "7.97".to_string()));
        assert_eq!((taken("XDim", "102,5"), taken("XDim", "breed")), (Some(0.1025), None));
    }

    #[test]
    fn a_profile_of_the_steel_table_is_found_and_taken() {
        let mut app = App::default();
        // found by what is typed, a space more or less, and by series
        let names = |found: Vec<&'static SteelProfile>| found.iter().take(3).map(|s| s.designation.as_str()).collect::<Vec<_>>();
        assert_eq!(names(found_steel("hea2", None)), ["HEA 200", "HEA 220", "HEA 240"]);
        assert_eq!(found_steel(" ", None).len(), steel_profiles().len());
        let angles = found_steel("", Some("L"));
        assert!(angles.iter().any(|s| s.series == Series::LEqual) && angles.iter().any(|s| s.series == Series::LUnequal) && angles.iter().all(|s| s.series.name() == "L"));
        assert_eq!(names(found_steel("30", Some("HEB"))), ["HEB 300"]);
        // clicked with a type picked: that type takes it: its shape, its designation, steel
        let _ = app.profile_update(ProfileMessage::Add);
        let id = app.profile_view.picked.clone().unwrap();
        let _ = app.profile_update(ProfileMessage::Steel("HEA 200".into()));
        let t = app.library.linear_type(&id).unwrap().clone();
        assert_eq!((t.designation.as_deref(), t.material.as_str(), t.profile.ifc().0), (Some("HEA 200"), "steel", "IfcIShapeProfileDef"));
        assert_eq!(app.library.linear_name(&id, Lang::En).as_deref(), Some("HEA 200"));
        // changed, it is no profile of the table any more
        let _ = app.profile_update(ProfileMessage::Typed(2, "9".into()));
        let _ = app.profile_update(ProfileMessage::Done);
        assert_eq!(app.library.linear_type(&id).unwrap().designation, None);
        // clicked with none picked: a type of the role shown is made of it
        let _ = app.profile_update(ProfileMessage::Open(LinearRole::Member));
        let _ = app.profile_update(ProfileMessage::Steel("L 80x80x8".into()));
        let made = app.profile_view.picked.clone().expect("the new type is picked");
        assert_eq!(app.library.linear_type(&made).map(|t| (t.role, t.designation.as_deref())), Some((LinearRole::Member, Some("L 80x80x8"))));
        // from a search: a beam of it in the project, once, on the project's page
        let beams = app.library.linear_types_of(LinearRole::Beam).count();
        let _ = app.libraries_update(LibrariesMessage::Search("IPE 300".into()));
        let _ = app.profile_update(ProfileMessage::FromSteel("IPE 300".into()));
        let beam = app.profile_view.picked.clone().unwrap();
        assert_eq!(app.library.linear_type(&beam).map(|t| (t.role, t.designation.as_deref())), Some((LinearRole::Beam, Some("IPE 300"))));
        assert_eq!((app.libraries.source, app.libraries.query.as_str(), app.profile_view.shown()), (Source::Project, "", LinearRole::Beam));
        let _ = app.profile_update(ProfileMessage::FromSteel("IPE 300".into()));
        assert_eq!((app.library.linear_types_of(LinearRole::Beam).count(), app.profile_view.picked.clone()), (beams + 1, Some(beam)));
        // a project without steel gets the general library's
        app.library.materials.retain(|m| m.key != "steel");
        let _ = app.profile_update(ProfileMessage::FromSteel("HEB 300".into()));
        assert!(app.library.material("steel").is_some() && app.profile_view.refused.is_none());
    }

    #[test]
    fn a_type_of_another_library_is_edited_as_a_copy_and_one_editor_shows_at_a_time() {
        let mut app = App::default();
        // a type of the Dutch template: a copy in the project, with its material
        let template = bim_library::inb::library().linear_types[0].clone();
        let _ = app.libraries_update(LibrariesMessage::Search(template.name.clone().unwrap_or_default()));
        let _ = app.profile_update(ProfileMessage::Copy(Shelf::Netherlands, template.id.clone()));
        let id = app.profile_view.picked.clone().expect("the copy is picked");
        let copied = app.library.linear_type(&id).unwrap();
        assert_eq!((&copied.profile, &copied.name, copied.role), (&template.profile, &template.name, template.role));
        assert_eq!(copied.check(&app.library.materials), Ok(()));
        assert_eq!((app.libraries.source, app.libraries.query.as_str(), app.profile_view.shown()), (Source::Project, "", template.role));
        // the right pane edits what was picked last: a build-up, or a linear type
        let wall = app.library.buildups[0].id.clone();
        let _ = app.buildup_update(super::super::buildups::BuildUpMessage::Pick(wall.clone()));
        assert_eq!((app.buildup_view.picked.clone(), app.profile_view.picked.clone()), (Some(wall), None));
        let _ = app.profile_update(ProfileMessage::Pick(id.clone()));
        assert_eq!((app.buildup_view.picked.clone(), app.profile_view.picked.clone()), (None, Some(id)));
    }

    #[test]
    fn the_project_page_lists_types_and_steel_profiles_and_the_pane_edits_the_picked_type() {
        let mut app = App::default();
        let l = Language::English;
        let page = iced::Size::new(1200.0, 1600.0);
        {
            let mut types = crate::app::test_ui_of(page, app.profile_lists(app.theme.palette(), l));
            assert!(types.find("+ Linear type").is_ok() && types.find("Concrete 300x300").is_ok() && types.find("IfcCircleProfileDef").is_ok());
        }
        {
            let mut table = crate::app::test_ui_of(page, app.steel_table(app.theme.palette(), l));
            assert!(table.find("IPE 80").is_ok() && table.find("HEB").is_ok() && table.find("Click a profile to make a linear type of it.").is_ok());
        }
        {
            let mut pane = crate::app::test_ui_of(iced::Size::new(340.0, 800.0), app.profile_pane(app.theme.palette(), l));
            assert!(pane.find("Pick a linear type on the page to edit its profile.").is_ok());
        }
        let id = app.library.linear_types_of(LinearRole::Column).next().unwrap().id.clone();
        let _ = app.profile_update(ProfileMessage::Pick(id));
        let _ = app.profile_update(ProfileMessage::Find("ipe 3".into()));
        {
            let mut table = crate::app::test_ui_of(page, app.steel_table(app.theme.palette(), l));
            assert!(table.find("IPE 300").is_ok() && table.find("IPE 330").is_ok() && table.find("IPE 80").is_err(), "the steel table as it is searched");
            assert!(table.find("Click a profile: the picked linear type takes it.").is_ok());
        }
        let mut pane = crate::app::test_ui_of(iced::Size::new(340.0, 800.0), app.profile_pane(app.theme.palette(), l));
        assert!(pane.find("IfcColumnType · COLUMN").is_ok() && pane.find("XDim").is_ok() && pane.find("YDim").is_ok() && pane.find("Own name").is_ok());
    }
}
