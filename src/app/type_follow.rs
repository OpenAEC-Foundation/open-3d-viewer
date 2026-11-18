//! Elements follow their type.
//!
//! An element of the modelled project keeps a copy of its type, so it is whole without the
//! library: a wall or a slab its build-up's layers, a system its type's grid, members, panels
//! and layers, a column or a beam its type's profile. When a type is changed in the Library
//! product, the elements made of it are made again as the type is now, each as far as the
//! kernel can make it. One the kernel refuses stays as it was, and the editor's pane says so.
//!
//! A wall, a slab or a system holds its type by the type's id. A column or a beam holds it by
//! the type's English name, kept as its profile's designation, as a profile of the steel table
//! is held by its designation: when a linear type's name changes (with its profile, its
//! material or a name typed for it) its elements take the new name with the new profile.
//!
//! The change of the elements is a step of the model's history. Taking it back leaves the type
//! as it was changed: the pane then says how many elements are not as the type is now, with a
//! button that makes them so. The same holds for a file whose elements were made before their
//! type changed.

use super::App;
use crate::theme::Palette;
use bim_core::{BuildUpDef, Element, ElementId, LinearDef, Model, ProfileDef, Refusal, Shape};
use bim_library::{Lang, Library, LinearRole, ProfileShape};
use iced::widget::text::Wrapping;
use iced::widget::{button, row, text, Column};
use iced::{Center, Element as Widget, Fill};
use openaec_ui::Language;

/// A type of the project's library that elements are made of, by its id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeRef<'a> {
    BuildUp(&'a str),
    System(&'a str),
    Linear(&'a str),
}

/// How the elements of a model stand to a type: those made of it, and those of them that are
/// not as the type is now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Use {
    pub elements: Vec<ElementId>,
    pub stale: Vec<ElementId>,
}

/// What following a type did to its elements: those made again, and those the kernel refused,
/// each with why.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Followed {
    pub changed: Vec<ElementId>,
    pub refused: Vec<(ElementId, Refusal)>,
}

/// What ties a linear element to its type: the type's English name, which the element keeps as
/// its profile's designation; with the type's profile as it was when the tie was read.
#[derive(Debug, Clone, PartialEq)]
pub struct LinearLink {
    pub id: String,
    pub role: LinearRole,
    pub name: String,
    pub profile: ProfileShape,
}

/// The ties of every linear type of `library`.
pub fn linear_links(library: &Library) -> Vec<LinearLink> {
    LinearRole::ALL
        .into_iter()
        .flat_map(|role| library.linear_names(role, Lang::En))
        .filter_map(|(id, name)| library.linear_type(id).map(|t| LinearLink { id: id.to_string(), role: t.role, name, profile: t.profile.clone() }))
        .collect()
}

/// The tie element `e` holds to: by its profile's designation and by what it is. A column is of
/// a column type and any other member of a beam type, as the tools place them; a type of
/// another role has no tool of its own and takes either.
fn link_of<'a>(links: &'a [LinearLink], e: &Element) -> Option<&'a LinearLink> {
    let Shape::Linear(LinearDef { profile: ProfileDef::Catalogue { designation, .. }, .. }) = &e.shape else { return None };
    let role = if e.class() == "IfcColumn" { LinearRole::Column } else { LinearRole::Beam };
    let named = || links.iter().filter(|link| link.name == *designation);
    named().find(|link| link.role == role).or_else(|| named().find(|link| !matches!(link.role, LinearRole::Column | LinearRole::Beam)))
}

/// A linear element's profile as its type has it: the type's profile under the type's name.
fn linear_profile(library: &Library, id: &str) -> Option<ProfileDef> {
    let t = library.linear_type(id)?;
    Some(ProfileDef::Catalogue { designation: library.linear_name(id, Lang::En)?, shape: t.profile.clone() })
}

/// `shape` as the type it holds by id makes it now: a wall or a slab by its build-up, a system
/// by its system type. `None` when it holds no such type, when the library lacks the type, and
/// for a system whose type lies while it stands (or the other way round): a system keeps the
/// way it was laid out.
pub fn as_typed(shape: &Shape, library: &Library) -> Option<Shape> {
    match shape {
        Shape::Planar(p) => {
            let def = BuildUpDef::from_library(library, &p.build_up.as_ref()?.id)?;
            Some(Shape::Planar(p.clone().with_build_up(Some(def))))
        }
        Shape::System(s) => {
            let t = library.system_type(&s.system_type.as_ref()?.id)?;
            Some(Shape::System(s.clone().typed(t)?))
        }
        _ => None,
    }
}

/// What element `e` is to become to be as type `of` is now: `None` when it is not of that type,
/// `Some(None)` when it is and is as the type is, `Some(Some(shape))` when it is to be made
/// again. `links` are the ties of the library's linear types.
fn due(e: &Element, library: &Library, links: &[LinearLink], of: TypeRef) -> Option<Option<Shape>> {
    let held = match (&e.shape, of) {
        (Shape::Planar(p), TypeRef::BuildUp(id)) => p.build_up.as_ref().is_some_and(|b| b.id == id),
        (Shape::System(s), TypeRef::System(id)) => s.system_type.as_ref().is_some_and(|t| t.id == id),
        (Shape::Linear(_), TypeRef::Linear(id)) => link_of(links, e).is_some_and(|link| link.id == id),
        _ => false,
    };
    if !held {
        return None;
    }
    let now = match (&e.shape, of) {
        (Shape::Linear(l), TypeRef::Linear(id)) => linear_profile(library, id).map(|profile| Shape::Linear(LinearDef { profile, ..l.clone() })),
        (shape, _) => as_typed(shape, library),
    };
    Some(now.filter(|now| *now != e.shape))
}

/// The elements of `model` made of type `of`, and those of them that are not as the type is now.
pub fn users(model: &Model, library: &Library, of: TypeRef) -> Use {
    let links = if matches!(of, TypeRef::Linear(_)) { linear_links(library) } else { Vec::new() };
    let mut used = Use::default();
    for e in model.iter() {
        if let Some(now) = due(e, library, &links, of) {
            used.elements.push(e.id);
            if now.is_some() {
                used.stale.push(e.id);
            }
        }
    }
    used
}

/// Makes each of `shapes` the shape of its element, as far as the kernel can make it.
fn replace_all(model: &mut Model, shapes: Vec<(ElementId, Shape)>) -> Followed {
    let mut done = Followed::default();
    for (id, shape) in shapes {
        let Some(mut element) = model.get(id).cloned() else { continue };
        element.shape = shape;
        match model.replace(element) {
            Ok(()) => done.changed.push(id),
            Err(refusal) => done.refused.push((id, refusal)),
        }
    }
    done
}

/// Makes the elements of `model` made of type `of` as the type is now.
pub fn follow(model: &mut Model, library: &Library, of: TypeRef) -> Followed {
    let links = if matches!(of, TypeRef::Linear(_)) { linear_links(library) } else { Vec::new() };
    let shapes = model.iter().filter_map(|e| Some((e.id, due(e, library, &links, of)??))).collect();
    replace_all(model, shapes)
}

/// After the linear types of `library` changed: the elements of `model` that were of a type by
/// the ties read `before` the change take the name and the profile the type has now. A type
/// that is as it was leaves its elements alone; one that is gone leaves them what they had.
pub fn relink(model: &mut Model, before: &[LinearLink], library: &Library) -> Followed {
    let shapes = model
        .iter()
        .filter_map(|e| {
            let Shape::Linear(l) = &e.shape else { return None };
            let link = link_of(before, e)?;
            let profile = linear_profile(library, &link.id)?;
            let as_it_was = matches!(&profile, ProfileDef::Catalogue { designation, shape } if *designation == link.name && *shape == link.profile);
            (!as_it_was && profile != l.profile).then(|| (e.id, Shape::Linear(LinearDef { profile, ..l.clone() })))
        })
        .collect();
    replace_all(model, shapes)
}

/// The lines of an editor's pane about the elements of the picked type: how many elements of
/// the model are made of it, how many of them are not as the type is now (with a button that
/// makes them so), and how many the kernel refused to make so, with why.
pub(super) fn use_note<'a, M: Clone + 'a>(p: Palette, l: Language, used: &Use, refused: &[(ElementId, Refusal)], update: M) -> Widget<'a, M> {
    let mut lines = Column::new().spacing(4);
    let count = used.elements.len();
    let about = if count == 0 {
        l.text("Geen element in het model is van dit type.", "No element of the model is of this type.").to_string()
    } else {
        format!("{}: {count}. {}", l.text("Elementen van dit type in het model", "Elements of this type in the model"), l.text("Ze volgen een wijziging van het type.", "They follow a change of the type."))
    };
    lines = lines.push(text(about).size(11).color(p.text_muted).wrapping(Wrapping::Word));
    if !used.stale.is_empty() {
        let stale = text(format!("{}: {}", l.text("Niet zoals het type nu is", "Not as the type is now"), used.stale.len())).size(11).color(p.accent).wrapping(Wrapping::Word).width(Fill);
        let make = button(text(l.text("Bijwerken", "Update")).size(12).color(p.text_secondary)).padding([3, 8]).style(move |_, s| p.row_button(s)).on_press(update);
        lines = lines.push(row![stale, make].spacing(6).align_y(Center));
    }
    if let Some((_, why)) = refused.first() {
        let told = format!("{}: {} ({})", l.text("Niet bijgewerkt", "Not updated"), refused.len(), l.translate(why.reason()));
        lines = lines.push(text(told).size(11).color(p.accent).wrapping(Wrapping::Word));
    }
    lines.into()
}

impl App {
    /// The elements of the modelled project made of type `of`, and those of them that are not
    /// as the type is now.
    pub(super) fn type_use(&self, of: TypeRef) -> Use {
        users(&self.modeller.model, &self.library, of)
    }

    /// The elements of the modelled project made of type `of` are made as the type is now.
    pub(super) fn follow_type(&mut self, of: TypeRef) -> Followed {
        let done = follow(&mut self.modeller.model, &self.library, of);
        self.followed(done)
    }

    /// The columns and beams of the modelled project follow their linear types, by the ties
    /// read `before` the types changed.
    pub(super) fn follow_linear(&mut self, before: &[LinearLink]) -> Followed {
        let done = relink(&mut self.modeller.model, before, &self.library);
        self.followed(done)
    }

    /// The walls and slabs of the modelled project made of a build-up with a layer of material
    /// `key` follow it: a material hatched as air is a cavity in them.
    pub(super) fn follow_material(&mut self, key: &str) -> Followed {
        let ids: Vec<String> = self.library.buildups.iter().filter(|b| b.layers.iter().any(|layer| layer.material == key)).map(|b| b.id.clone()).collect();
        let mut all = Followed::default();
        for id in ids {
            let done = follow(&mut self.modeller.model, &self.library, TypeRef::BuildUp(&id));
            all.changed.extend(done.changed);
            all.refused.extend(done.refused);
        }
        self.followed(all)
    }

    /// The joins follow the elements that changed, and what was refused is kept for the pane
    /// (an editor forgets it at its next message). The model is shown again by the editor
    /// that asked.
    fn followed(&mut self, done: Followed) -> Followed {
        if !done.changed.is_empty() {
            self.joins_follow(&done.changed);
        }
        self.follow_refused.extend(done.refused.iter().copied());
        done
    }
}

#[cfg(test)]
mod tests {
    use super::super::buildups::BuildUpMessage;
    use super::super::materials::MaterialMessage;
    use super::super::profiles::ProfileMessage;
    use super::super::systems::{Part, Size, SystemMessage};
    use super::super::Message;
    use super::*;
    use bim_core::element::PlanarForm;
    use bim_core::{PlanarDef, SystemDef};
    use bim_library::HatchKind;
    use glam::{DVec2, DVec3};

    fn wall(app: &App, x: f64, build_up: &str) -> Shape {
        let form = PlanarForm::Wall { start: DVec2::new(x, 0.0), end: DVec2::new(x, 4.0), base: 0.0, height: 3.0 };
        Shape::Planar(PlanarDef { form, thickness: 0.1, openings: Vec::new(), build_up: None }.with_build_up(BuildUpDef::from_library(&app.library, build_up)))
    }

    fn column(app: &App, x: f64, id: &str) -> Shape {
        let profile = linear_profile(&app.library, id).expect("a linear type of the library");
        Shape::Linear(LinearDef { start: DVec3::new(x, 10.0, 0.0), end: DVec3::new(x, 10.0, 3.0), profile, roll: 0.0 })
    }

    fn thickness(app: &App, id: ElementId) -> f64 {
        match &app.modeller.model.get(id).unwrap().shape {
            Shape::Planar(p) => p.total_thickness(),
            _ => panic!("no wall"),
        }
    }

    fn profile(app: &App, id: ElementId) -> ProfileDef {
        match &app.modeller.model.get(id).unwrap().shape {
            Shape::Linear(l) => l.profile.clone(),
            _ => panic!("no column"),
        }
    }

    /// An app with a project of its own to model in.
    fn project() -> App {
        let mut app = App::default();
        app.new_model("Level 0");
        app
    }

    #[test]
    fn walls_and_slabs_follow_their_build_up() {
        let mut app = project();
        let concrete = app.library.buildups.iter().find(|b| b.layers.len() == 1 && b.layers[0].material == "concrete" && b.role == bim_library::Role::Wall).expect("a concrete wall").id.clone();
        let other = app.library.buildups.iter().find(|b| b.id != concrete && b.role == bim_library::Role::Wall).unwrap().id.clone();
        let a = app.add_element("Wall", None, wall(&app, 0.0, &concrete)).unwrap();
        let b = app.add_element("Wall", None, wall(&app, 2.0, &concrete)).unwrap();
        let c = app.add_element("Wall", None, wall(&app, 4.0, &other)).unwrap();
        let before = (thickness(&app, a), thickness(&app, c));
        assert_eq!(app.type_use(TypeRef::BuildUp(&concrete)), Use { elements: vec![a, b], stale: Vec::new() });
        // the type gets thicker: its walls do, the other type's wall does not
        let _ = app.update(Message::BuildUp(BuildUpMessage::Pick(concrete.clone())));
        let _ = app.update(Message::BuildUp(BuildUpMessage::Typed(0, "275".into())));
        let _ = app.update(Message::BuildUp(BuildUpMessage::Done));
        assert_eq!((thickness(&app, a), thickness(&app, b), thickness(&app, c)), (0.275, 0.275, before.1));
        assert_eq!(app.type_use(TypeRef::BuildUp(&concrete)), Use { elements: vec![a, b], stale: Vec::new() });
        // a layer more: the walls have it too
        let _ = app.update(Message::BuildUp(BuildUpMessage::AddLayer(0)));
        assert_eq!(thickness(&app, a), 0.55);
        // the change of the walls is one step to take back; the type stays as it was changed,
        // and its walls are told to be behind it
        assert!(app.can_undo());
        let _ = app.update(Message::Undo);
        assert_eq!(thickness(&app, a), 0.275);
        assert_eq!(app.library.buildup(&concrete).unwrap().layers.len(), 2);
        assert_eq!(app.type_use(TypeRef::BuildUp(&concrete)).stale, vec![a, b]);
        // and are made as it is again when asked
        let _ = app.update(Message::BuildUp(BuildUpMessage::Follow));
        assert_eq!((thickness(&app, a), app.type_use(TypeRef::BuildUp(&concrete)).stale), (0.55, Vec::new()));
        // a name of its own changes no element: no step
        let base = app.modeller.base.model.clone();
        let _ = app.update(Message::BuildUp(BuildUpMessage::Named("Kernwand".into())));
        assert!(std::sync::Arc::ptr_eq(&base, &app.modeller.base.model));
    }

    #[test]
    fn a_material_hatched_as_air_is_a_cavity_in_the_walls_made_of_it() {
        let mut app = project();
        let cavity_wall = app.library.buildups.iter().find(|b| b.layers.iter().any(|layer| layer.material == "air")).expect("a cavity wall").id.clone();
        let a = app.add_element("Wall", None, wall(&app, 0.0, &cavity_wall)).unwrap();
        let cavities = |app: &App| match &app.modeller.model.get(a).unwrap().shape {
            Shape::Planar(p) => p.build_up.as_ref().unwrap().layers.iter().filter(|layer| layer.cavity).count(),
            _ => 0,
        };
        assert_eq!(cavities(&app), 1);
        // the cavity's material drawn as insulation: it is a solid layer of the wall
        let _ = app.update(Message::Material(MaterialMessage::Pick("air".into())));
        let _ = app.update(Message::Material(MaterialMessage::Hatch(HatchKind::Insulation)));
        assert_eq!((cavities(&app), app.type_use(TypeRef::BuildUp(&cavity_wall)).stale), (0, Vec::new()));
        let _ = app.update(Message::Material(MaterialMessage::Hatch(HatchKind::Air)));
        assert_eq!(cavities(&app), 1);
    }

    #[test]
    fn systems_follow_their_type() {
        let mut app = project();
        let line = |y: f64| SystemDef { start: DVec2::new(0.0, y), end: DVec2::new(6.0, y), base: 0.0, height: 2.7, spacing: DVec2::splat(1.5), member: DVec2::new(0.05, 0.15), panel: 0.02, system_type: None, openings: Vec::new() };
        let frame = app.add_element("Frame", None, Shape::System(line(0.0).typed_from(&app.library, "system.timber-frame-140").unwrap())).unwrap();
        let masonry = app.add_element("Masonry", None, Shape::System(line(8.0).typed_from(&app.library, "system.masonry-waal-stretcher").unwrap())).unwrap();
        let untyped = app.add_element("Curtain wall", None, Shape::System(line(16.0))).unwrap();
        let system = |app: &App, id: ElementId| match &app.modeller.model.get(id).unwrap().shape {
            Shape::System(s) => s.clone(),
            _ => panic!("no system"),
        };
        assert_eq!(app.type_use(TypeRef::System("system.timber-frame-140")), Use { elements: vec![frame], stale: Vec::new() });
        // studs and insulation of 184: the frame has them, with its layers moved out; the
        // others are as they were
        let (other, plain) = (system(&app, masonry), system(&app, untyped));
        let _ = app.update(Message::SystemType(SystemMessage::Pick("system.timber-frame-140".into())));
        for size in [Size::Depth, Size::Thickness] {
            let _ = app.update(Message::SystemType(SystemMessage::Typed(Part::Face, size, "184".into())));
            let _ = app.update(Message::SystemType(SystemMessage::Done));
        }
        let now = system(&app, frame);
        let t = app.library.system_type("system.timber-frame-140").unwrap();
        assert_eq!((now.member.y, now.panel, &now.system_type.as_ref().unwrap().layers), (0.184, 0.184, &t.layers));
        assert!((t.layers[0].offset - 0.09225).abs() < 1e-12, "the membrane lies against the deeper frame");
        assert_eq!((system(&app, masonry), system(&app, untyped)), (other, plain));
        assert!(app.type_use(TypeRef::System("system.timber-frame-140")).stale.is_empty() && app.follow_refused.is_empty());
        // a layer away: the frame loses it; taken back, it is behind its type until asked
        let _ = app.update(Message::SystemType(SystemMessage::RemoveLayer(0)));
        assert_eq!(system(&app, frame).system_type.unwrap().layers.len(), 4);
        let _ = app.update(Message::Undo);
        assert_eq!((system(&app, frame).system_type.unwrap().layers.len(), app.type_use(TypeRef::System("system.timber-frame-140")).stale), (5, vec![frame]));
        let _ = app.update(Message::SystemType(SystemMessage::Follow));
        assert_eq!(system(&app, frame).system_type.unwrap().layers.len(), 4);
    }

    #[test]
    fn columns_and_beams_follow_their_linear_type_by_its_name() {
        let mut app = project();
        let types: Vec<String> = app.library.linear_types_of(LinearRole::Column).map(|t| t.id.clone()).collect();
        let (first, second) = (types[0].clone(), types[1].clone());
        let a = app.add_element("Column", None, column(&app, 0.0, &first)).unwrap();
        let b = app.add_element("Column", None, column(&app, 2.0, &second)).unwrap();
        // a beam named as the column type is no column of it
        let lying = match column(&app, 0.0, &first) {
            Shape::Linear(l) => Shape::Linear(LinearDef { start: DVec3::new(0.0, 20.0, 3.0), end: DVec3::new(4.0, 20.0, 3.0), ..l }),
            shape => shape,
        };
        let beam = app.add_element("Beam", None, lying).unwrap();
        assert_eq!(app.type_use(TypeRef::Linear(&first)), Use { elements: vec![a], stale: Vec::new() });
        let (other, beam_before) = (profile(&app, b), profile(&app, beam));
        // its profile changes: its name does, and the column takes both
        let _ = app.update(Message::Profile(ProfileMessage::Pick(first.clone())));
        let _ = app.update(Message::Profile(ProfileMessage::Typed(0, "450".into())));
        let _ = app.update(Message::Profile(ProfileMessage::Done));
        let name = app.library.linear_name(&first, Lang::En).unwrap();
        let t = app.library.linear_type(&first).unwrap().clone();
        assert_eq!(profile(&app, a), ProfileDef::Catalogue { designation: name.clone(), shape: t.profile.clone() });
        assert_ne!(Some(&name), linear_links(&Library::starter()).iter().find(|link| link.id == first).map(|link| &link.name), "the name follows the profile");
        assert_eq!((profile(&app, b), profile(&app, beam)), (other, beam_before));
        assert_eq!(app.type_use(TypeRef::Linear(&first)), Use { elements: vec![a], stale: Vec::new() });
        // a name typed for it: the column is named so too, and stays of its type
        let _ = app.update(Message::Profile(ProfileMessage::Named("K1".into())));
        assert_eq!(profile(&app, a), ProfileDef::Catalogue { designation: "K1".into(), shape: t.profile.clone() });
        assert_eq!(app.type_use(TypeRef::Linear(&first)).elements, vec![a]);
        // a profile of the steel table: the column is of steel's HEB 300
        let _ = app.update(Message::Profile(ProfileMessage::Named(String::new())));
        let _ = app.update(Message::Profile(ProfileMessage::Steel("HEB 300".into())));
        assert_eq!(Some(profile(&app, a)), ProfileDef::steel("HEB 300"));
        // taken back, the column is the type's by name no more, or it is behind it
        let _ = app.update(Message::Undo);
        assert_ne!(Some(profile(&app, a)), ProfileDef::steel("HEB 300"));
    }

    #[test]
    fn an_element_the_kernel_refuses_stays_as_it_was_and_is_told_of() {
        let mut app = project();
        let cavity_wall = app.library.buildups.iter().find(|b| b.layers.iter().any(|layer| layer.material == "air")).expect("a cavity wall").id.clone();
        let a = app.add_element("Wall", None, wall(&app, 0.0, &cavity_wall)).unwrap();
        let solid = |app: &App| match &app.modeller.model.get(a).unwrap().shape {
            Shape::Planar(p) => p.build_up.as_ref().unwrap().layers.iter().filter(|layer| !layer.cavity).count(),
            _ => 0,
        };
        // every layer a cavity: a type the library holds, and no wall the kernel makes; the
        // wall follows until its last solid layer would go, and keeps that one
        let layers = app.library.buildup(&cavity_wall).unwrap().layers.len();
        let _ = app.update(Message::BuildUp(BuildUpMessage::Pick(cavity_wall.clone())));
        for layer in 0..layers {
            let _ = app.update(Message::BuildUp(BuildUpMessage::Material(layer, "air".into())));
        }
        assert!(app.library.buildup(&cavity_wall).unwrap().layers.iter().all(|layer| layer.material == "air"));
        assert_eq!(solid(&app), 1);
        assert_eq!(app.follow_refused, vec![(a, Refusal::NoLayers)]);
        assert_eq!(app.type_use(TypeRef::BuildUp(&cavity_wall)), Use { elements: vec![a], stale: vec![a] });
        // the pane says so, with the way to try again
        let l = Language::English;
        let used = app.type_use(TypeRef::BuildUp(&cavity_wall));
        let mut pane = crate::app::test_ui_of(iced::Size::new(340.0, 200.0), use_note(app.theme.palette(), l, &used, &app.follow_refused, Message::BuildUp(BuildUpMessage::Follow)));
        assert!(pane.find("Elements of this type in the model: 1. They follow a change of the type.").is_ok());
        assert!(pane.find("Not as the type is now: 1").is_ok() && pane.find("Update").is_ok());
        assert!(pane.find(format!("Not updated: 1 ({})", Refusal::NoLayers.reason()).as_str()).is_ok());
    }
}
