//! Service types in the Library product: ducts, pipes, cable trays, cable ladders and
//! conduits with their sizes and fittings, and the insulation and lining that go with them.
//!
//! **Service types** (`bim_library::service`) are shown by what they carry. A tile says a
//! type's range of sizes, its name and its key; the right pane shows the picked one: its
//! section, its material, the standard its sizes follow, the fittings a run of it takes, and
//! its series of sizes, each with its nominal size, its real outside size, its wall and its
//! clear inside. A size is taken into the project as a linear type, which is what a run of it
//! is drawn and placed as.
//!
//! A type of another library is taken into the project with a button. One of the project is
//! copied (to make a type from an existing one), removed, and edited: its names, its sizes
//! (typed in millimetres, added and removed) and its fittings. What is typed is taken as soon
//! as the type holds together, and until then stays in the field.
//!
//! **Cover types** (insulation and lining) are a category of their own: a tile each, and the
//! thickness of one of the project typed.
//!
//! Not yet: the MEP tools of the Modeller taking a type and its fittings, a flat oval's
//! series, and changing a type's section, material or standard.

use super::libraries::{Category, Shelf};
use super::{App, Message, FONT_BOLD};
use crate::theme::Palette;
use bim_library::service::{Bend, Carrier, Joint, Section, ServiceType, Size, Tee, Transition};
use bim_library::{Lang, Library, LibraryError};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, pick_list, row, text, text_input, Column, Space};
use iced::{Center, Element, Fill, Length};
use openaec_ui::design::a11y::{self, Role as Aria};
use openaec_ui::Language;

/// What is typed into, of the picked type of the project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name(Lang),
    /// The nominal size of a size, by its place in the series.
    Nominal(usize),
    Width(usize),
    Height(usize),
    Wall(usize),
    /// The radius of a bend, in sizes.
    Radius,
    /// The thickness of the picked cover type.
    Cover,
}

#[derive(Debug, Clone)]
pub enum ServiceMessage {
    /// Show this service type of the source shown.
    Pick(String),
    /// Show this cover type of the source shown.
    PickCover(String),
    /// Take this service type of another library into the project's.
    Take(Shelf, String),
    /// Take this cover type of another library into the project's.
    TakeCover(Shelf, String),
    /// A copy of the picked type of the project, under a key of its own.
    Copy,
    /// Remove the picked type from the project's library.
    Remove,
    /// Typed into a field of the picked type of the project.
    Typed(Field, String),
    /// One more size after the last of the picked type.
    AddSize,
    RemoveSize(usize),
    Bend(Bend),
    Tee(Tee),
    Transition(Transition),
    Joint(Joint),
    /// The size with this nominal size of a type of this shelf, into the project as a linear
    /// type, named in this language.
    Run(Shelf, String, String, Lang),
}

/// Which service type and which cover type the right pane shows, why the last change was
/// refused, and what is typed in a field that does not hold together yet.
#[derive(Debug, Clone, Default)]
pub struct ServiceView {
    pub picked: Option<String>,
    pub cover: Option<String>,
    pub refused: Option<String>,
    pub draft: Option<(Field, String)>,
}

/// A material of `from` that `library` lacks, taken with a type that is of it.
fn bring(library: &mut Library, from: &Library, key: &str) -> Result<(), LibraryError> {
    match from.material(key).filter(|_| library.material(key).is_none()) {
        Some(material) => library.add_material(material.clone()),
        None => Ok(()),
    }
}

/// A service type of `from` taken into `library`, with its material; its key. A type the
/// library has is not taken twice.
pub fn take(library: &mut Library, from: &Library, id: &str) -> Result<String, LibraryError> {
    let service = from.service_type(id).ok_or(LibraryError::NoKey)?.clone();
    bring(library, from, &service.material)?;
    library.add_service_type(service)?;
    Ok(id.to_string())
}

/// A cover type of `from` taken into `library`, as a service type is.
pub fn take_cover(library: &mut Library, from: &Library, id: &str) -> Result<String, LibraryError> {
    let cover = from.cover_type(id).ok_or(LibraryError::NoKey)?.clone();
    bring(library, from, &cover.material)?;
    library.add_cover_type(cover)?;
    Ok(id.to_string())
}

/// A copy of service type `id` beside it, under the first free key of the project's own
/// ("project.service.1", and on); its key.
pub fn copy(library: &mut Library, id: &str) -> Result<String, LibraryError> {
    let at = library.service_types.iter().position(|s| s.id == id).ok_or(LibraryError::NoKey)?;
    let mut service = library.service_types[at].clone();
    service.id = (1..).map(|n| format!("project.service.{n}")).find(|key| library.service_type(key).is_none()).expect("a free key");
    let key = service.id.clone();
    library.service_types.insert(at + 1, service);
    Ok(key)
}

/// A copy of cover type `id` beside it, under "project.cover.1", and on; its key.
pub fn copy_cover(library: &mut Library, id: &str) -> Result<String, LibraryError> {
    let at = library.cover_types.iter().position(|c| c.id == id).ok_or(LibraryError::NoKey)?;
    let mut cover = library.cover_types[at].clone();
    cover.id = (1..).map(|n| format!("project.cover.{n}")).find(|key| library.cover_type(key).is_none()).expect("a free key");
    let key = cover.id.clone();
    library.cover_types.insert(at + 1, cover);
    Ok(key)
}

/// The size `nominal` of service type `id` of `from` into `library` as a linear type, named
/// in `lang`; the linear type's key. One that is there already is not added again.
pub fn run(library: &mut Library, from: &Library, id: &str, nominal: &str, lang: Lang) -> Result<String, LibraryError> {
    let linear = from.service_type(id).and_then(|service| service.linear(nominal, lang)).ok_or(LibraryError::NoKey)?;
    let key = linear.id.clone();
    if library.linear_type(&key).is_none() {
        bring(library, from, &linear.material)?;
        library.add_linear_type(linear)?;
    }
    Ok(key)
}

/// Millimetres typed, in metres.
fn metres(typed: &str) -> Option<f64> {
    typed.trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite()).map(|mm| mm / 1000.0)
}

/// What is typed into `field` put on `service`; false when it is no number or the type has no
/// such size. A round size stays round: its diameter is both its width and its height.
fn put(service: &mut ServiceType, field: Field, typed: &str) -> bool {
    let round = service.section == Section::Round;
    let size = |sizes: &mut Vec<Size>, at: usize, to: &dyn Fn(&mut Size, f64)| match (sizes.get_mut(at), metres(typed)) {
        (Some(size), Some(value)) => {
            to(size, value);
            true
        }
        _ => false,
    };
    match field {
        Field::Name(lang) => {
            let name = match lang {
                Lang::Nl => &mut service.names.nl,
                Lang::En => &mut service.names.en,
                Lang::De => &mut service.names.de,
                Lang::Fr => &mut service.names.fr,
            };
            *name = typed.trim().to_string();
            true
        }
        Field::Nominal(at) => service.sizes.get_mut(at).map(|size| size.nominal = typed.trim().to_string()).is_some(),
        Field::Width(at) => size(&mut service.sizes, at, &|size, value| {
            size.width = value;
            if round {
                size.height = value;
            }
        }),
        Field::Height(at) => !round && size(&mut service.sizes, at, &|size, value| size.height = value),
        Field::Wall(at) => size(&mut service.sizes, at, &|size, value| size.wall = value),
        Field::Radius => match typed.trim().replace(',', ".").parse::<f64>() {
            Ok(value) if value.is_finite() => {
                service.fittings.radius = value;
                true
            }
            _ => false,
        },
        Field::Cover => false,
    }
}

/// What `field` of `service` shows.
fn shown(service: &ServiceType, field: Field) -> String {
    let mm = |v: f64| bim_library::millimetres(v, Lang::En);
    let of = |at: usize, what: &dyn Fn(&Size) -> String| service.sizes.get(at).map(what).unwrap_or_default();
    match field {
        Field::Name(lang) => service.name(lang).to_string(),
        Field::Nominal(at) => of(at, &|size| size.nominal.clone()),
        Field::Width(at) => of(at, &|size| mm(size.width)),
        Field::Height(at) => of(at, &|size| mm(size.height)),
        Field::Wall(at) => of(at, &|size| mm(size.wall)),
        Field::Radius => format!("{}", service.fittings.radius),
        Field::Cover => String::new(),
    }
}

/// The sizes of a series as a tile says them: its smallest and its largest.
fn range(service: &ServiceType) -> String {
    let say = |size: &Size| if service.section == Section::Round { format!("Ø {}", size.nominal) } else { size.nominal.clone() };
    match (service.sizes.first(), service.sizes.last()) {
        (Some(first), Some(last)) if service.sizes.len() > 1 => format!("{} – {}", say(first), say(last)),
        (Some(only), _) => say(only),
        _ => String::new(),
    }
}

/// A fitting to pick, named in the language shown.
#[derive(Debug, Clone, PartialEq)]
struct Choice<T> {
    of: T,
    name: &'static str,
}

impl<T> std::fmt::Display for Choice<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name)
    }
}

impl App {
    pub(super) fn service_update(&mut self, message: ServiceMessage) {
        let covers = self.category() == Category::Covers;
        match message {
            ServiceMessage::Pick(id) => self.service_view = ServiceView { picked: Some(id), cover: self.service_view.cover.take(), ..ServiceView::default() },
            ServiceMessage::PickCover(id) => self.service_view = ServiceView { cover: Some(id), picked: self.service_view.picked.take(), ..ServiceView::default() },
            ServiceMessage::Take(shelf, id) => {
                let from = self.shelf(shelf).clone();
                match take(&mut self.library, &from, &id) {
                    Ok(id) => {
                        let carrier = self.library.service_type(&id).map(|s| s.carrier);
                        self.service_view = ServiceView { picked: Some(id), ..ServiceView::default() };
                        if let Some(carrier) = carrier {
                            self.show_in_project(Category::Services(carrier));
                        }
                    }
                    Err(why) => self.service_view.refused = Some(why.to_string()),
                }
            }
            ServiceMessage::TakeCover(shelf, id) => {
                let from = self.shelf(shelf).clone();
                match take_cover(&mut self.library, &from, &id) {
                    Ok(id) => {
                        self.service_view = ServiceView { cover: Some(id), ..ServiceView::default() };
                        self.show_in_project(Category::Covers);
                    }
                    Err(why) => self.service_view.refused = Some(why.to_string()),
                }
            }
            ServiceMessage::Copy => {
                let copied = match (covers, self.service_view.cover.clone(), self.service_view.picked.clone()) {
                    (true, Some(id), _) => copy_cover(&mut self.library, &id).map(|key| ServiceView { cover: Some(key), ..ServiceView::default() }),
                    (false, _, Some(id)) => copy(&mut self.library, &id).map(|key| ServiceView { picked: Some(key), ..ServiceView::default() }),
                    _ => return,
                };
                match copied {
                    Ok(view) => self.service_view = view,
                    Err(why) => self.service_view.refused = Some(why.to_string()),
                }
            }
            ServiceMessage::Remove => {
                if covers {
                    if let Some(id) = self.service_view.cover.take() {
                        self.library.cover_types.retain(|c| c.id != id);
                    }
                } else if let Some(id) = self.service_view.picked.take() {
                    self.library.service_types.retain(|s| s.id != id);
                }
                self.service_view = ServiceView::default();
            }
            ServiceMessage::Typed(Field::Cover, typed) => {
                let Some(at) = self.service_view.cover.as_deref().and_then(|id| self.library.cover_types.iter().position(|c| c.id == id)) else { return };
                let mut changed = self.library.cover_types[at].clone();
                let took = metres(&typed).map(|value| changed.thickness = value).is_some();
                let checked = changed.check(&self.library.materials);
                self.service_view.refused = checked.as_ref().err().filter(|_| took).map(|why| why.to_string());
                if took && checked.is_ok() {
                    self.library.cover_types[at] = changed;
                    self.service_view.draft = None;
                } else {
                    self.service_view.draft = Some((Field::Cover, typed));
                }
            }
            ServiceMessage::Typed(field, typed) => {
                let Some(at) = self.service_at() else { return };
                let mut changed = self.library.service_types[at].clone();
                let took = put(&mut changed, field, &typed);
                let checked = changed.check(&self.library.materials);
                // what does not hold together yet stays in the field, the type as it was; so
                // does a space typed at an end, which the type does not keep
                self.service_view.refused = checked.as_ref().err().filter(|_| took).map(|why| why.to_string());
                if took && checked.is_ok() {
                    self.library.service_types[at] = changed;
                    self.service_view.draft = (typed.trim() != typed).then_some((field, typed));
                } else {
                    self.service_view.draft = Some((field, typed));
                }
            }
            ServiceMessage::AddSize => self.service_change(|service| {
                // one more like the last, named apart so the type still holds together
                if let Some(mut size) = service.sizes.last().cloned() {
                    size.nominal = (2..).map(|n| format!("{} ({n})", size.nominal)).find(|name| service.size(name).is_none()).expect("a free name");
                    service.sizes.push(size);
                }
            }),
            ServiceMessage::RemoveSize(at) => self.service_change(|service| {
                if at < service.sizes.len() {
                    service.sizes.remove(at);
                }
            }),
            ServiceMessage::Bend(bend) => self.service_change(|service| service.fittings.bend = bend),
            ServiceMessage::Tee(tee) => self.service_change(|service| service.fittings.tee = tee),
            ServiceMessage::Transition(transition) => self.service_change(|service| service.fittings.transition = transition),
            ServiceMessage::Joint(joint) => self.service_change(|service| service.fittings.joint = joint),
            ServiceMessage::Run(shelf, id, nominal, lang) => {
                let from = self.shelf(shelf).clone();
                match run(&mut self.library, &from, &id, &nominal, lang) {
                    Ok(key) => {
                        let role = self.library.linear_type(&key).map(|t| t.role);
                        self.service_view.refused = None;
                        if let Some(role) = role {
                            self.show_in_project(Category::Linear(role));
                            self.profile_view.picked = Some(key);
                        }
                    }
                    Err(why) => self.service_view.refused = Some(why.to_string()),
                }
            }
        }
    }

    /// Where the picked service type is in the project's library.
    fn service_at(&self) -> Option<usize> {
        let id = self.service_view.picked.as_deref()?;
        self.library.service_types.iter().position(|s| s.id == id)
    }

    /// A change of the picked service type of the project that is not typed: taken, or
    /// refused with why.
    fn service_change(&mut self, to: impl FnOnce(&mut ServiceType)) {
        let Some(at) = self.service_at() else { return };
        let mut changed = self.library.service_types[at].clone();
        to(&mut changed);
        self.service_view.draft = None;
        match changed.check(&self.library.materials) {
            Ok(()) => {
                self.library.service_types[at] = changed;
                self.service_view.refused = None;
            }
            Err(why) => self.service_view.refused = Some(why.to_string()),
        }
    }

    /// The service types of `carrier` of the source shown as tiles: a type's range of sizes
    /// as its look, its name and its key.
    pub(super) fn service_tiles<'a>(&'a self, p: Palette, l: Language, library: &'a Library, carrier: Carrier) -> Vec<Element<'a, Message>> {
        let lang = super::library::lang(l);
        library
            .service_types_of(carrier)
            .map(|service| {
                let look = container(text(range(service)).size(13).color(p.text)).width(Fill).align_x(Center);
                let on = self.service_view.picked.as_deref() == Some(service.id.as_str());
                super::libraries::tile(p, look.into(), service.name(lang).to_string(), service.id.clone(), on, Some(Message::Service(ServiceMessage::Pick(service.id.clone()))))
            })
            .collect()
    }

    /// The cover types of the source shown as tiles: a cover's thickness as its look.
    pub(super) fn cover_tiles<'a>(&'a self, p: Palette, l: Language, library: &'a Library) -> Vec<Element<'a, Message>> {
        let lang = super::library::lang(l);
        library
            .cover_types
            .iter()
            .map(|cover| {
                let look = container(text(format!("{} mm", bim_library::millimetres(cover.thickness, lang))).size(13).color(p.text)).width(Fill).align_x(Center);
                let on = self.service_view.cover.as_deref() == Some(cover.id.as_str());
                super::libraries::tile(p, look.into(), cover.name(&library.materials, lang), cover.id.clone(), on, Some(Message::Service(ServiceMessage::PickCover(cover.id.clone()))))
            })
            .collect()
    }

    /// The right pane for the service types: the picked one of `library`, on `shelf`.
    pub fn service_pane<'a>(&'a self, p: Palette, l: Language, library: &'a Library, shelf: Shelf) -> Element<'a, Message> {
        let lang = super::library::lang(l);
        let view = &self.service_view;
        let Some(service) = view.picked.as_deref().and_then(|id| library.service_type(id)) else {
            let hint = l.text("Klik een type om zijn maten en hulpstukken te zien.", "Click a type to see its sizes and fittings.");
            return container(text(hint).size(12).color(p.text_muted).wrapping(Wrapping::Word)).padding(8).into();
        };
        let own = shelf == Shelf::Project;
        let round = service.section == Section::Round;
        let label = move |label: String| text(label).size(12).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph).width(Length::FillPortion(2));
        let line = |name: String, value: String| -> Element<'a, ServiceMessage> { row![label(name), text(value).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph).width(Length::FillPortion(3))].spacing(8).into() };
        let input = |of: Field, hint: &'static str| -> Element<'a, ServiceMessage> {
            let value = view.draft.as_ref().filter(|(field, _)| *field == of).map_or_else(|| shown(service, of), |(_, typed)| typed.clone());
            a11y::named(text_input(hint, &value).on_input(move |typed| ServiceMessage::Typed(of, typed)).size(12).padding([2, 6]), Aria::TextField, hint).into()
        };
        let act = |label: &'static str, message: ServiceMessage| button(text(label).size(12).color(p.text_secondary)).padding([3, 8]).style(move |_, s| p.row_button(s)).on_press(message);
        let material = library.material(&service.material).map_or(service.material.as_str(), |m| m.name(lang));
        let mut pane: Column<'_, ServiceMessage> = column![
            text(service.name(lang).to_string()).size(14).font(FONT_BOLD).color(p.text).wrapping(Wrapping::Word),
            text(format!("{} · {} · {material}", service.carrier.name(lang), service.section.name(lang))).size(11).color(p.text_muted).wrapping(Wrapping::Word),
            line(l.text("Sleutel", "Key").to_string(), service.id.clone()),
        ]
        .spacing(4);
        if own {
            for of in Lang::ALL {
                pane = pane.push(row![label(super::materials::lang_name(of).to_string()), container(input(Field::Name(of), super::materials::lang_name(of))).width(Length::FillPortion(3))].spacing(8).align_y(Center));
            }
        }
        if let Some(standard) = &service.standard {
            pane = pane.push(line(l.text("Norm", "Standard").to_string(), standard.clone()));
        }
        pane = pane.push(line("LOD".to_string(), service.lod().number().to_string()));
        pane = pane.push(line("NL-SfB".to_string(), super::codes::named(library, &service.nlsfb(), lang)));
        // its fittings: the routing preferences
        let fittings = service.fittings;
        pane = pane.push(text(l.text("Hulpstukken", "Fittings")).size(11).color(p.text_secondary));
        let (bend, radius, tee, transition, joint) = (l.text("Bocht", "Bend"), l.text("Bochtstraal (× de maat)", "Bend radius (× the size)"), l.text("Aftakking", "Branch"), l.text("Verloop", "Transition"), l.text("Verbinding", "Joint"));
        if own {
            let picker = |name: &'static str, control: Element<'a, ServiceMessage>| -> Element<'a, ServiceMessage> { row![label(name.to_string()), container(control).width(Length::FillPortion(3))].spacing(8).align_y(Center).into() };
            let bends: Vec<Choice<Bend>> = Bend::ALL.iter().map(|of| Choice { of: *of, name: of.name(lang) }).collect();
            let tees: Vec<Choice<Tee>> = Tee::ALL.iter().map(|of| Choice { of: *of, name: of.name(lang) }).collect();
            let transitions: Vec<Choice<Transition>> = Transition::ALL.iter().map(|of| Choice { of: *of, name: of.name(lang) }).collect();
            let joints: Vec<Choice<Joint>> = Joint::ALL.iter().map(|of| Choice { of: *of, name: of.name(lang) }).collect();
            let now = |name: &'static str| name;
            pane = pane.push(picker(bend, a11y::named(pick_list(bends, Some(Choice { of: fittings.bend, name: now(fittings.bend.name(lang)) }), |c: Choice<Bend>| ServiceMessage::Bend(c.of)).text_size(12).padding([2, 6]).width(Fill), Aria::ComboBox, bend).into()));
            pane = pane.push(picker(radius, input(Field::Radius, radius)));
            pane = pane.push(picker(tee, a11y::named(pick_list(tees, Some(Choice { of: fittings.tee, name: now(fittings.tee.name(lang)) }), |c: Choice<Tee>| ServiceMessage::Tee(c.of)).text_size(12).padding([2, 6]).width(Fill), Aria::ComboBox, tee).into()));
            pane = pane.push(picker(transition, a11y::named(pick_list(transitions, Some(Choice { of: fittings.transition, name: now(fittings.transition.name(lang)) }), |c: Choice<Transition>| ServiceMessage::Transition(c.of)).text_size(12).padding([2, 6]).width(Fill), Aria::ComboBox, transition).into()));
            pane = pane.push(picker(joint, a11y::named(pick_list(joints, Some(Choice { of: fittings.joint, name: now(fittings.joint.name(lang)) }), |c: Choice<Joint>| ServiceMessage::Joint(c.of)).text_size(12).padding([2, 6]).width(Fill), Aria::ComboBox, joint).into()));
        } else {
            let turns = if fittings.radius > 0.0 { format!("{}, {} {} × {}", fittings.bend.name(lang), l.text("straal", "radius"), fittings.radius, l.text("de maat", "the size")) } else { fittings.bend.name(lang).to_string() };
            pane = pane.push(line(bend.to_string(), turns));
            pane = pane.push(line(tee.to_string(), fittings.tee.name(lang).to_string()));
            pane = pane.push(line(transition.to_string(), fittings.transition.name(lang).to_string()));
            pane = pane.push(line(joint.to_string(), fittings.joint.name(lang).to_string()));
        }
        // its series: nominal, outside, wall; and each size as a linear type of the project
        let series = if round { l.text("Maten: nominaal, buitendiameter, wand (mm)", "Sizes: nominal, outside diameter, wall (mm)") } else { l.text("Maten: nominaal, breedte, hoogte, wand (mm)", "Sizes: nominal, width, height, wall (mm)") };
        pane = pane.push(text(series).size(11).color(p.text_secondary).wrapping(Wrapping::Word));
        let as_type = l.text("Als staaftype naar het project", "Into the project as a linear type");
        for (at, size) in service.sizes.iter().enumerate() {
            let to_project = a11y::named(button(text("→").size(12).color(p.text_secondary)).padding([2, 6]).style(move |_, s| p.row_button(s)).on_press(ServiceMessage::Run(shelf, service.id.clone(), size.nominal.clone(), lang)), Aria::Button, as_type);
            if own {
                let remove = a11y::named(button(text("×").size(12).color(p.text_secondary)).padding([2, 6]).style(move |_, s| p.row_button(s)).on_press(ServiceMessage::RemoveSize(at)), Aria::Button, l.text("Maat verwijderen", "Remove size"));
                let mut cells = row![input(Field::Nominal(at), l.text("nominaal", "nominal")), input(Field::Width(at), if round { l.text("diameter", "diameter") } else { l.text("breedte", "width") })].spacing(4).align_y(Center);
                if !round {
                    cells = cells.push(input(Field::Height(at), l.text("hoogte", "height")));
                }
                pane = pane.push(cells.push(input(Field::Wall(at), l.text("wand", "wall"))).push(to_project).push(remove));
            } else {
                let (inside, _) = size.inside();
                let says = format!("{} · {} · {} {} · {} {}", size.nominal, size.outside(service.section, lang), l.text("wand", "wall"), bim_library::millimetres(size.wall, lang), l.text("binnen", "inside"), bim_library::millimetres(inside, lang));
                pane = pane.push(row![text(says).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph).width(Fill), to_project].spacing(4).align_y(Center));
            }
        }
        pane = pane.push(text(l.text("→ maakt van een maat een staaftype in het project. De plaatdiktes verschillen per fabrikant: een begin om te wijzigen.", "→ makes a size a linear type of the project. Sheet thicknesses differ by maker: a start to change.")).size(11).color(p.text_muted).wrapping(Wrapping::Word));
        let actions = if own {
            row![act(l.text("+ Maat", "+ Size"), ServiceMessage::AddSize), act(l.text("Kopiëren", "Copy"), ServiceMessage::Copy), act(l.text("Verwijderen", "Remove"), ServiceMessage::Remove), Space::new().width(Fill)]
        } else {
            row![act(l.text("Naar het project", "Take into the project"), ServiceMessage::Take(shelf, service.id.clone())), Space::new().width(Fill)]
        };
        pane = pane.push(actions.spacing(4));
        let refused = view.refused.as_ref().map(|why| text(format!("{} ({why})", l.text("Niet overgenomen.", "Not taken."))).size(11).color(p.accent).wrapping(Wrapping::Word));
        container(Element::from(pane.push(refused)).map(Message::Service)).padding(8).into()
    }

    /// The right pane for the cover types: the picked one of `library`, on `shelf`.
    pub fn cover_pane<'a>(&'a self, p: Palette, l: Language, library: &'a Library, shelf: Shelf) -> Element<'a, Message> {
        let lang = super::library::lang(l);
        let view = &self.service_view;
        let Some(cover) = view.cover.as_deref().and_then(|id| library.cover_type(id)) else {
            let hint = l.text("Klik een isolatie of bekleding om te zien wat ze is.", "Click an insulation or a lining to see what it is.");
            return container(text(hint).size(12).color(p.text_muted).wrapping(Wrapping::Word)).padding(8).into();
        };
        let label = move |label: String| text(label).size(12).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph).width(Length::FillPortion(2));
        let line = |name: String, value: String| -> Element<'a, ServiceMessage> { row![label(name), text(value).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph).width(Length::FillPortion(3))].spacing(8).into() };
        let act = |label: &'static str, message: ServiceMessage| button(text(label).size(12).color(p.text_secondary)).padding([3, 8]).style(move |_, s| p.row_button(s)).on_press(message);
        let thickness = l.text("Dikte (mm)", "Thickness (mm)");
        let mut pane: Column<'_, ServiceMessage> = column![
            text(cover.name(&library.materials, lang)).size(14).font(FONT_BOLD).color(p.text).wrapping(Wrapping::Word),
            text(match cover.place {
                bim_library::service::Place::Insulation => l.text("om een kanaal of een leiding", "round a duct or a pipe"),
                bim_library::service::Place::Lining => l.text("binnen in een kanaal", "inside a duct"),
            })
            .size(11)
            .color(p.text_muted),
            line(l.text("Sleutel", "Key").to_string(), cover.id.clone()),
            line(l.text("Materiaal", "Material").to_string(), library.material(&cover.material).map_or(cover.material.as_str(), |m| m.name(lang)).to_string()),
        ]
        .spacing(4);
        if shelf == Shelf::Project {
            let value = view.draft.as_ref().filter(|(field, _)| *field == Field::Cover).map_or_else(|| bim_library::millimetres(cover.thickness, Lang::En), |(_, typed)| typed.clone());
            let input = a11y::named(text_input("", &value).on_input(|typed| ServiceMessage::Typed(Field::Cover, typed)).size(12).padding([2, 6]), Aria::TextField, thickness);
            pane = pane.push(row![label(thickness.to_string()), container(input).width(Length::FillPortion(3))].spacing(8).align_y(Center));
            pane = pane.push(row![act(l.text("Kopiëren", "Copy"), ServiceMessage::Copy), act(l.text("Verwijderen", "Remove"), ServiceMessage::Remove), Space::new().width(Fill)].spacing(4));
        } else {
            pane = pane.push(line(thickness.to_string(), bim_library::millimetres(cover.thickness, lang)));
            pane = pane.push(row![act(l.text("Naar het project", "Take into the project"), ServiceMessage::TakeCover(shelf, cover.id.clone())), Space::new().width(Fill)]);
        }
        let refused = view.refused.as_ref().map(|why| text(format!("{} ({why})", l.text("Niet overgenomen.", "Not taken."))).size(11).color(p.accent).wrapping(Wrapping::Word));
        container(Element::from(pane.push(refused)).map(Message::Service)).padding(8).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bim_library::profile::{LinearRole, ProfileShape};
    use bim_library::service::{CoverType, Place};

    fn show(app: &mut App, category: Category) {
        app.show_in_project(category);
    }

    #[test]
    fn the_library_shows_its_service_types_by_what_they_carry_and_the_pane_the_picked_one() {
        let mut app = App::default();
        let p = app.theme.palette();
        let l = Language::English;
        let per: Vec<usize> = Carrier::ALL.iter().map(|carrier| app.service_tiles(p, l, &app.library, *carrier).len()).collect();
        assert_eq!(per, [2, 5, 1, 1, 1]);
        assert_eq!(app.cover_tiles(p, l, &app.library).len(), app.library.cover_types.len());
        let starter = Library::starter();
        let of = |id: &str| starter.service_type(id).cloned().expect("the type");
        assert_eq!((range(&of("pipe.steel-threaded")), range(&of("duct.rectangular-galvanised")), range(&of("pipe.copper"))), ("Ø DN 15 – Ø DN 150".to_string(), "200x100 – 1200x600".to_string(), "Ø 12 – Ø 54".to_string()));
        app.service_update(ServiceMessage::Pick("pipe.steel-threaded".into()));
        let finds = |app: &App, library: &Library, shelf: Shelf, all: &[&str]| {
            let mut pane = crate::app::test_ui_of(iced::Size::new(360.0, 1400.0), app.service_pane(p, l, library, shelf));
            for shown in all {
                assert!(pane.find(*shown).is_ok(), "{shown}");
            }
        };
        // of another library: its series, its fittings, and the way to take it
        finds(&app, &starter, Shelf::General, &["Steel tube, threaded, medium series", "pipes · round · steel", "pipe.steel-threaded", "EN 10255", "56 · heat distribution", "elbow, radius 1.5 × the size", "threaded", "Sizes: nominal, outside diameter, wall (mm)", "DN 25 · Ø 33.7 · wall 3.2 · inside 27.3", "Take into the project"]);
        // of the project: its names, its fittings and its sizes to change
        finds(&app, &app.library, Shelf::Project, &["Deutsch", "Bend radius (× the size)", "Branch", "Transition", "Joint", "+ Size", "Copy", "Remove"]);
        // a rectangular duct has a height to its sizes; nothing picked shows the hint
        app.service_update(ServiceMessage::Pick("duct.rectangular-galvanised".into()));
        finds(&app, &starter, Shelf::General, &["Sizes: nominal, width, height, wall (mm)", "400x200 · 400 x 200 · wall 0.8 · inside 398.4"]);
        app.service_view = ServiceView::default();
        finds(&app, &starter, Shelf::General, &["Click a type to see its sizes and fittings."]);
        // a cover: where it is, what of, how thick
        app.service_update(ServiceMessage::PickCover("lining.mineral-wool-25".into()));
        let mut pane = crate::app::test_ui_of(iced::Size::new(360.0, 400.0), app.cover_pane(p, l, &starter, Shelf::General));
        for shown in ["Lining mineral wool 25 mm", "inside a duct", "lining.mineral-wool-25", "mineral wool", "Thickness (mm)", "Take into the project"] {
            assert!(pane.find(shown).is_ok(), "{shown}");
        }
    }

    #[test]
    fn a_type_is_made_from_an_existing_one_and_its_sizes_and_fittings_changed() {
        let mut app = App::default();
        show(&mut app, Category::Services(Carrier::Pipe));
        let before = app.library.service_types.len();
        app.service_update(ServiceMessage::Pick("pipe.copper".into()));
        app.service_update(ServiceMessage::Copy);
        let key = app.service_view.picked.clone().expect("the copy is picked");
        assert_eq!((key.as_str(), app.library.service_types.len()), ("project.service.1", before + 1));
        let of = |app: &App, id: &str| app.library.service_type(id).cloned().expect("the type");
        // its name, a size and its fittings; a round size stays round
        app.service_update(ServiceMessage::Typed(Field::Name(Lang::En), "Copper tube, hard".into()));
        app.service_update(ServiceMessage::Typed(Field::Wall(3), "1,2".into()));
        app.service_update(ServiceMessage::Typed(Field::Width(7), "66.7".into()));
        app.service_update(ServiceMessage::Typed(Field::Nominal(7), "66.7".into()));
        app.service_update(ServiceMessage::Bend(Bend::Bent));
        app.service_update(ServiceMessage::Joint(Joint::Solder));
        app.service_update(ServiceMessage::Typed(Field::Radius, "3".into()));
        let made = of(&app, &key);
        assert_eq!((made.name(Lang::En), made.sizes[3].wall, made.sizes[7].clone()), ("Copper tube, hard", 0.0012, Size::round("66.7", 66.7, 1.5)));
        assert_eq!((made.fittings.bend, made.fittings.joint, made.fittings.radius, made.fittings.tee), (Bend::Bent, Joint::Solder, 3.0, Tee::Tee));
        assert!(app.service_view.draft.is_none() && app.service_view.refused.is_none());
        // a height typed on a round type is not taken; a space at an end stays in the field
        app.service_update(ServiceMessage::Typed(Field::Height(0), "20".into()));
        assert_eq!((of(&app, &key).sizes[0].height, app.service_view.draft.clone()), (0.012, Some((Field::Height(0), "20".into()))));
        app.service_update(ServiceMessage::Typed(Field::Name(Lang::Nl), "Koperen buis ".into()));
        assert_eq!((of(&app, &key).name(Lang::Nl).to_string(), app.service_view.draft.clone()), ("Koperen buis".to_string(), Some((Field::Name(Lang::Nl), "Koperen buis ".into()))));
        // what is no size stays in the field; the type keeps what it had
        app.service_update(ServiceMessage::Typed(Field::Wall(0), "6".into()));
        assert!(of(&app, &key).sizes[0].wall == 0.001 && app.service_view.refused.is_some());
        assert_eq!(app.service_view.draft, Some((Field::Wall(0), "6".into())));
        app.service_update(ServiceMessage::Typed(Field::Nominal(1), "12".into()));
        assert!(of(&app, &key).sizes[1].nominal == "15" && app.service_view.refused.is_some(), "a size twice");
        app.service_update(ServiceMessage::Typed(Field::Wall(0), "een".into()));
        assert_eq!((app.service_view.draft.clone(), app.service_view.refused.clone()), (Some((Field::Wall(0), "een".into())), None), "no number");
        // a size more, like the last and named apart; then one less
        app.service_update(ServiceMessage::AddSize);
        assert_eq!(of(&app, &key).sizes.iter().rev().take(2).map(|s| s.nominal.clone()).collect::<Vec<_>>(), ["66.7 (2)", "66.7"]);
        app.service_update(ServiceMessage::RemoveSize(8));
        app.service_update(ServiceMessage::RemoveSize(80));
        assert_eq!(of(&app, &key).sizes.len(), 8);
        // the last size is not removed: a type has a size
        let mut lone = of(&app, &key);
        lone.id = "office.lone".into();
        lone.sizes.truncate(1);
        app.library.add_service_type(lone).unwrap();
        app.service_update(ServiceMessage::Pick("office.lone".into()));
        app.service_update(ServiceMessage::RemoveSize(0));
        assert!(of(&app, "office.lone").sizes.len() == 1 && app.service_view.refused.is_some());
        app.service_update(ServiceMessage::Remove);
        // the catalogue's own type is as it was; the copy is kept by its key through a save
        assert_eq!(app.library.service_type("pipe.copper"), Library::starter().service_type("pipe.copper"));
        let read = bim_library::ifcx::read(&bim_library::ifcx::file(&app.library, "test", "test", "2026-11-20T00:00:00Z")).expect("the library reads");
        assert_eq!(read.service_type(&key), app.library.service_type(&key));
        // typing in one field is one step to take back
        app.service_update(ServiceMessage::Pick(key.clone()));
        let typing = |field: Field| app.typing_of(&Message::Service(ServiceMessage::Typed(field, "1".into())));
        assert_eq!(typing(Field::Wall(2)), Some(super::super::undo::Typing::Service(key.clone(), Field::Wall(2))));
        assert!(typing(Field::Wall(2)) != typing(Field::Wall(3)));
        // removed, the project has what it had
        app.service_update(ServiceMessage::Remove);
        assert_eq!((app.library.service_types.len(), app.service_view.picked.clone()), (before, None));
    }

    #[test]
    fn a_size_goes_into_the_project_as_a_linear_type_and_types_are_taken_once() {
        let mut app = App::default();
        let pipes = app.library.linear_types_of(LinearRole::Pipe).count();
        let _ = app.update(Message::Service(ServiceMessage::Run(Shelf::General, "pipe.copper".into(), "22".into(), Lang::En)));
        let run = app.library.linear_type("pipe.copper.22").cloned().expect("the run's type");
        assert_eq!((run.role, run.profile.clone(), app.library.linear_types_of(LinearRole::Pipe).count()), (LinearRole::Pipe, ProfileShape::Chs { d: 0.022, t: 0.001 }, pipes + 1));
        assert_eq!((app.category(), app.profile_view.picked.as_deref()), (Category::Linear(LinearRole::Pipe), Some("pipe.copper.22")));
        // the same size again adds nothing; a size the type lacks is refused
        let _ = app.update(Message::Service(ServiceMessage::Run(Shelf::General, "pipe.copper".into(), "22".into(), Lang::En)));
        assert_eq!(app.library.linear_types_of(LinearRole::Pipe).count(), pipes + 1);
        let _ = app.update(Message::Service(ServiceMessage::Run(Shelf::General, "pipe.copper".into(), "23".into(), Lang::En)));
        assert!(app.service_view.refused.is_some() && app.library.linear_type("pipe.copper.23").is_none());
        // a type of another library comes with its material, once
        let mut other = Library::starter();
        other.add_material(bim_library::Material::new("stainless-steel", bim_library::material::Names::same("RVS"), bim_library::material::HatchKind::Steel, [200, 204, 210])).unwrap();
        let mut steel = other.service_type("pipe.steel-threaded").cloned().unwrap();
        (steel.id, steel.material) = ("office.pipe.stainless".into(), "stainless-steel".into());
        other.add_service_type(steel).unwrap();
        other.add_cover_type(CoverType::new("office.cover.stainless", Place::Insulation, "stainless-steel", 1.0)).unwrap();
        assert!(app.library.material("stainless-steel").is_none());
        assert_eq!(take(&mut app.library, &other, "office.pipe.stainless").as_deref(), Ok("office.pipe.stainless"));
        assert!(app.library.material("stainless-steel").is_some());
        assert!(take(&mut app.library, &other, "office.pipe.stainless").is_err() && take(&mut app.library, &other, "none").is_err());
        assert_eq!(take_cover(&mut app.library, &other, "office.cover.stainless").as_deref(), Ok("office.cover.stainless"));
        assert!(take_cover(&mut app.library, &other, "office.cover.stainless").is_err());
        // a cover of the project: copied, its thickness typed, removed
        show(&mut app, Category::Covers);
        let covers = app.library.cover_types.len();
        app.service_update(ServiceMessage::PickCover("insulation.mineral-wool-30".into()));
        app.service_update(ServiceMessage::Copy);
        let key = app.service_view.cover.clone().expect("the copy is picked");
        app.service_update(ServiceMessage::Typed(Field::Cover, "35".into()));
        let made = app.library.cover_type(&key).cloned().expect("the copy");
        assert_eq!((key.as_str(), made.thickness, made.name(&app.library.materials, Lang::En)), ("project.cover.1", 0.035, "Insulation mineral wool 35 mm".to_string()));
        app.service_update(ServiceMessage::Typed(Field::Cover, "0".into()));
        assert!(app.library.cover_type(&key).is_some_and(|c| c.thickness == 0.035) && app.service_view.refused.is_some());
        app.service_update(ServiceMessage::Remove);
        assert_eq!((app.library.cover_types.len(), app.service_view.cover.clone()), (covers, None));
        // the levels list the service types
        let at = super::super::objects::at_level(&app.library, bim_library::lod::Lod::L300, Lang::En, Language::English);
        assert!(at.iter().any(|(name, group, category)| name == "Copper tube" && *group == "pipes" && *category == Category::Services(Carrier::Pipe)));
    }
}
