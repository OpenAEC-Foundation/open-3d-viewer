//! The Definition section of the right pane for a modelled element: its name and the sizes of its
//! definition typed in (a wall's height, thickness and base; a slab's thickness and top; a
//! member's profile and roll; an object's size and turn; a curtain wall's spacing, members and
//! panels), and the openings of a wall or a timber frame, added, typed in and removed.
//!
//! A value is used as soon as it is valid and the kernel can make the element with it, as the
//! View section does; until then the field keeps what was typed and the view's banner says why.
//! Enter shows the value in use again.

use super::{prop_row, App, Message};
use crate::document::parse_elevation;
use crate::theme::Palette;
use bim_core::element::PlanarForm;
use bim_core::{ConnectionDef, ConnectionKind, Element, Opening, PlanarDef, ProfileDef, Shape};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, row, text, text_input};
use iced::{Center, Element as Widget, Length};
use openaec_ui::Language;

/// A value of a modelled element typed in the right pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementField {
    Name,
    /// A wall's, curtain wall's or object's height.
    Height,
    /// A wall's or slab's thickness, a curtain wall's panel thickness.
    Thickness,
    /// A wall's or curtain wall's base, a slab's top, an object's height above 0.
    Base,
    /// An object's or a rectangular profile's width, a curtain wall's member width.
    Width,
    /// An object's or a rectangular profile's depth (height for a profile), a curtain wall's
    /// member depth.
    Depth,
    /// A round profile's diameter.
    Diameter,
    /// An object's turn about the vertical, a member's roll about its axis (degrees).
    Turn,
    /// A curtain wall's spacing along its line and up.
    SpacingAlong,
    SpacingUp,
    /// An opening's distance from its wall's (or timber frame's) start, sill, width and height.
    Along(usize),
    Sill(usize),
    OpeningWidth(usize),
    OpeningHeight(usize),
    /// An end plate's thickness, its margin past the profile, its bolts' and holes' diameter, and
    /// how many bolts in each row.
    PlateThickness,
    PlateMargin,
    Bolt,
    Hole,
    BoltRows,
}

/// A door's and a window's size and sill when added (metres).
pub(super) const DOOR: (f64, f64, f64) = (0.9, 2.1, 0.0);
pub(super) const WINDOW: (f64, f64, f64) = (1.2, 1.2, 0.9);

/// A number typed: metres (a comma or a point, `m` after it allowed), or degrees (`°` allowed).
fn number(typed: &str) -> Option<f64> {
    parse_elevation(typed.trim().trim_end_matches('°'))
}

/// The value of `field` in `shape` (scene coordinates), if the shape has it.
fn value(shape: &Shape, field: ElementField) -> Option<f64> {
    use ElementField as F;
    let opening = |o: &[Opening], i: usize| o.get(i).copied();
    match (shape, field) {
        (Shape::Point(p), F::Width) => Some(p.size.x),
        (Shape::Point(p), F::Depth) => Some(p.size.y),
        (Shape::Point(p), F::Height) => Some(p.size.z),
        (Shape::Point(p), F::Base) => Some(p.at.z),
        (Shape::Point(p), F::Turn) => Some(p.rotation.to_degrees()),
        (Shape::Linear(l), F::Width) => match l.profile {
            ProfileDef::Rectangle { width, .. } => Some(width),
            _ => None,
        },
        (Shape::Linear(l), F::Depth) => match l.profile {
            ProfileDef::Rectangle { height, .. } => Some(height),
            _ => None,
        },
        (Shape::Linear(l), F::Diameter) => match l.profile {
            ProfileDef::Circle { diameter } => Some(diameter),
            _ => None,
        },
        (Shape::Linear(l), F::Turn) => Some(l.roll.to_degrees()),
        (Shape::Planar(PlanarDef { form: PlanarForm::Wall { height, .. }, .. }), F::Height) => Some(*height),
        (Shape::Planar(PlanarDef { form: PlanarForm::Wall { base, .. }, .. }), F::Base) => Some(*base),
        (Shape::Planar(PlanarDef { form: PlanarForm::Slab { top, .. }, .. }), F::Base) => Some(*top),
        (Shape::Planar(p), F::Thickness) => Some(p.total_thickness()),
        (Shape::Planar(p), F::Along(i)) => opening(&p.openings, i).map(|o| o.along),
        (Shape::Planar(p), F::Sill(i)) => opening(&p.openings, i).map(|o| o.sill),
        (Shape::Planar(p), F::OpeningWidth(i)) => opening(&p.openings, i).map(|o| o.width),
        (Shape::Planar(p), F::OpeningHeight(i)) => opening(&p.openings, i).map(|o| o.height),
        (Shape::System(s), F::Height) => Some(s.height),
        (Shape::System(s), F::Base) => Some(s.base),
        (Shape::System(s), F::Thickness) => Some(s.panel),
        (Shape::System(s), F::Width) => Some(s.member.x),
        (Shape::System(s), F::Depth) => Some(s.member.y),
        (Shape::System(s), F::SpacingAlong) => Some(s.spacing.x),
        (Shape::System(s), F::SpacingUp) => Some(s.spacing.y),
        (Shape::System(s), F::Along(i)) => opening(&s.openings, i).map(|o| o.along),
        (Shape::System(s), F::Sill(i)) => opening(&s.openings, i).map(|o| o.sill),
        (Shape::System(s), F::OpeningWidth(i)) => opening(&s.openings, i).map(|o| o.width),
        (Shape::System(s), F::OpeningHeight(i)) => opening(&s.openings, i).map(|o| o.height),
        (Shape::Connection(ConnectionDef { kind: ConnectionKind::EndPlate(e), .. }), f) => match f {
            F::PlateThickness => Some(e.thickness),
            F::PlateMargin => Some(e.margin),
            F::Bolt => Some(e.bolt),
            F::Hole => Some(e.hole),
            F::BoltRows => Some(f64::from(e.rows)),
            _ => None,
        },
        _ => None,
    }
}

/// The openings of `shape` and how long it is, if it can have openings: a wall, or a system whose
/// type is framed (a timber frame).
fn openings_of(shape: &Shape) -> Option<(&[Opening], f64)> {
    match shape {
        Shape::Planar(PlanarDef { form: PlanarForm::Wall { start, end, .. }, openings, .. }) => Some((openings, (end - start).length())),
        Shape::System(s) if s.system_type.as_ref().is_some_and(|t| t.framing.is_some()) => Some((&s.openings, (s.end - s.start).length())),
        _ => None,
    }
}

/// The openings of `shape` to change, if it can have openings.
pub(super) fn openings_mut(shape: &mut Shape) -> Option<&mut Vec<Opening>> {
    match shape {
        Shape::Planar(PlanarDef { form: PlanarForm::Wall { .. }, openings, .. }) => Some(openings),
        Shape::System(s) if s.system_type.as_ref().is_some_and(|t| t.framing.is_some()) => Some(&mut s.openings),
        _ => None,
    }
}

/// `shape` with `field` set to `v` (scene coordinates), if the shape has it.
fn with_value(shape: &Shape, field: ElementField, v: f64) -> Option<Shape> {
    use ElementField as F;
    let mut shape = shape.clone();
    let set = |slot: &mut f64| *slot = v;
    match (&mut shape, field) {
        (Shape::Point(p), F::Width) => set(&mut p.size.x),
        (Shape::Point(p), F::Depth) => set(&mut p.size.y),
        (Shape::Point(p), F::Height) => set(&mut p.size.z),
        (Shape::Point(p), F::Base) => set(&mut p.at.z),
        (Shape::Point(p), F::Turn) => p.rotation = v.to_radians(),
        (Shape::Linear(l), F::Width) => match &mut l.profile {
            ProfileDef::Rectangle { width, .. } => set(width),
            _ => return None,
        },
        (Shape::Linear(l), F::Depth) => match &mut l.profile {
            ProfileDef::Rectangle { height, .. } => set(height),
            _ => return None,
        },
        (Shape::Linear(l), F::Diameter) => match &mut l.profile {
            ProfileDef::Circle { diameter } => set(diameter),
            _ => return None,
        },
        (Shape::Linear(l), F::Turn) => l.roll = v.to_radians(),
        (Shape::Planar(PlanarDef { form: PlanarForm::Wall { height, .. }, .. }), F::Height) => set(height),
        (Shape::Planar(PlanarDef { form: PlanarForm::Wall { base, .. }, .. }), F::Base) => set(base),
        (Shape::Planar(PlanarDef { form: PlanarForm::Slab { top, .. }, .. }), F::Base) => set(top),
        // a layered element is as thick as its type's layers
        (Shape::Planar(p), F::Thickness) if p.build_up.is_none() => set(&mut p.thickness),
        (Shape::Planar(p), F::Along(i)) => set(&mut p.openings.get_mut(i)?.along),
        (Shape::Planar(p), F::Sill(i)) => set(&mut p.openings.get_mut(i)?.sill),
        (Shape::Planar(p), F::OpeningWidth(i)) => set(&mut p.openings.get_mut(i)?.width),
        (Shape::Planar(p), F::OpeningHeight(i)) => set(&mut p.openings.get_mut(i)?.height),
        (Shape::System(s), F::Height) => set(&mut s.height),
        (Shape::System(s), F::Base) => set(&mut s.base),
        // a typed system's members and panels are its type's
        (Shape::System(s), F::Thickness) if s.system_type.is_none() => set(&mut s.panel),
        (Shape::System(s), F::Width) if s.system_type.is_none() => set(&mut s.member.x),
        (Shape::System(s), F::Depth) if s.system_type.is_none() => set(&mut s.member.y),
        (Shape::System(s), F::SpacingAlong) => set(&mut s.spacing.x),
        (Shape::System(s), F::SpacingUp) => set(&mut s.spacing.y),
        (Shape::System(s), F::Along(i)) => set(&mut s.openings.get_mut(i)?.along),
        (Shape::System(s), F::Sill(i)) => set(&mut s.openings.get_mut(i)?.sill),
        (Shape::System(s), F::OpeningWidth(i)) => set(&mut s.openings.get_mut(i)?.width),
        (Shape::System(s), F::OpeningHeight(i)) => set(&mut s.openings.get_mut(i)?.height),
        (Shape::Connection(ConnectionDef { kind: ConnectionKind::EndPlate(e), .. }), f) => match f {
            F::PlateThickness => set(&mut e.thickness),
            F::PlateMargin => set(&mut e.margin),
            F::Bolt => set(&mut e.bolt),
            F::Hole => set(&mut e.hole),
            // a whole number of bolts, at least one
            F::BoltRows if v >= 1.0 && v.fract() == 0.0 && v <= 64.0 => e.rows = v as u32,
            _ => return None,
        },
        _ => return None,
    }
    Some(shape)
}

impl App {
    /// The modelled element the Definition section shows: the one selected, alone.
    pub(super) fn defined(&self) -> Option<&Element> {
        let [id] = self.selected_modelled()[..] else { return None };
        self.modeller.model.get(id)
    }

    /// What a field shows: what is being typed in it, else the value in use.
    pub(super) fn element_field_text(&self, e: &Element, field: ElementField) -> String {
        if let Some((id, f, typed)) = &self.modeller.typing {
            if *id == e.id && *f == field {
                return typed.clone();
            }
        }
        if field == ElementField::Name {
            return e.name.clone();
        }
        let digits = match field {
            ElementField::Turn => 1,
            ElementField::BoltRows => 0,
            _ => 3,
        };
        value(&e.shape_in_project(&self.modeller.model.grid), field).map_or(String::new(), |v| format!("{v:.digits$}"))
    }

    /// Takes what is typed in a field, and uses it when it is valid and the kernel can make the
    /// element with it.
    pub(super) fn type_element_field(&mut self, field: ElementField, typed: String) {
        let Some(e) = self.defined().cloned() else { return };
        self.modeller.refusal = None;
        if field == ElementField::Name {
            let name = typed.trim();
            if !name.is_empty() && name != e.name {
                let mut renamed = e.clone();
                renamed.name = name.to_string();
                self.change_element(renamed);
            }
        } else if let Some(shape) = number(&typed).and_then(|v| with_value(&e.shape_in_project(&self.modeller.model.grid), field, v)) {
            self.change_shape(&e, shape);
        }
        self.modeller.typing = Some((e.id, field, typed));
    }

    /// Adds a door (with no sill) or a window in the middle of the selected wall or timber frame,
    /// if it fits.
    pub(super) fn add_opening(&mut self, door: bool) {
        let Some(e) = self.defined().cloned() else { return };
        let mut shape = e.shape_in_project(&self.modeller.model.grid);
        let Some((_, length)) = openings_of(&shape) else { return };
        let (width, height, sill) = if door { DOOR } else { WINDOW };
        if let Some(openings) = openings_mut(&mut shape) {
            openings.push(Opening { along: (length - width) / 2.0, sill, width, height });
        }
        self.modeller.refusal = None;
        self.change_shape(&e, shape);
    }

    /// Gives the selected join another kind (a cut stays a cut). An end plate starts with its
    /// default sizes.
    pub(super) fn set_join_kind(&mut self, kind: ConnectionKind) {
        let Some(e) = self.defined().cloned() else { return };
        let Shape::Connection(c) = &e.shape else { return };
        if c.kind == ConnectionKind::Cut || std::mem::discriminant(&c.kind) == std::mem::discriminant(&kind) {
            return;
        }
        let mut changed = e.clone();
        changed.shape = Shape::Connection(ConnectionDef { kind, ..c.clone() });
        self.modeller.refusal = None;
        self.change_element(changed);
    }

    /// Turns the selected join round: its other member becomes the primary (the one that runs
    /// through).
    pub(super) fn swap_join(&mut self) {
        let Some(e) = self.defined().cloned() else { return };
        let Shape::Connection(c) = &e.shape else { return };
        let mut changed = e.clone();
        changed.shape = Shape::Connection(ConnectionDef { kind: c.kind, primary: c.secondary, secondary: c.primary });
        self.modeller.refusal = None;
        self.change_element(changed);
    }

    /// Removes opening `index` of the selected wall or timber frame.
    pub(super) fn remove_opening(&mut self, index: usize) {
        let Some(e) = self.defined().cloned() else { return };
        let mut shape = e.shape_in_project(&self.modeller.model.grid);
        if let Some(openings) = openings_mut(&mut shape).filter(|o| index < o.len()) {
            openings.remove(index);
            self.change_shape(&e, shape);
        }
    }

    /// Gives element `e` a new definition (scene coordinates), if the kernel can make it.
    fn change_shape(&mut self, e: &Element, shape: Shape) {
        let grid = self.modeller.model.grid;
        let mut changed = Element::in_project(&grid, e.name.clone(), e.level.clone(), shape);
        changed.id = e.id;
        self.change_element(changed);
    }

    /// Puts `changed` in the place of the element with its id, if the kernel can make it; else
    /// keeps the element and shows why.
    fn change_element(&mut self, changed: Element) {
        let id = changed.id;
        match self.modeller.model.replace(changed) {
            Ok(()) => {
                self.joins_follow(&[id]);
                self.show_modelled();
                self.select_modelled(id);
            }
            Err(refusal) => self.modeller.refusal = Some(refusal),
        }
    }

    /// The Definition section: the selected modelled element's values to type in, and the openings
    /// of a wall or a timber frame.
    pub(super) fn element_properties(&self, p: Palette, l: Language) -> Option<Widget<'_, Message>> {
        let e = self.defined()?;
        let shape = e.shape_in_project(&self.modeller.model.grid);
        let field = |label: &str, f: ElementField| field_row(p, label, self.element_field_text(e, f), f);
        use ElementField as F;
        let mut rows = column![field(l.text("Naam", "Name"), F::Name)].spacing(5);
        let has = |f: F| value(&shape, f).is_some();
        let fields: [(F, &str); 9] = [
            (F::Height, l.text("Hoogte (m)", "Height (m)")),
            (F::Thickness, l.text("Dikte (m)", "Thickness (m)")),
            (F::Base, l.text("Basis (m)", "Base (m)")),
            (F::Width, l.text("Breedte (m)", "Width (m)")),
            (F::Depth, l.text("Diepte (m)", "Depth (m)")),
            (F::Diameter, l.text("Diameter (m)", "Diameter (m)")),
            (F::Turn, l.text("Draaiing (°)", "Turn (°)")),
            (F::SpacingAlong, l.text("Raster langs (m)", "Spacing along (m)")),
            (F::SpacingUp, l.text("Raster omhoog (m)", "Spacing up (m)")),
        ];
        let layered = match &shape {
            Shape::Planar(PlanarDef { build_up: Some(b), .. }) => Some(b),
            _ => None,
        };
        let system_type = match &shape {
            Shape::System(bim_core::SystemDef { system_type: Some(t), .. }) => Some(t),
            _ => None,
        };
        if let Some(b) = layered {
            let name = self.library.name(&b.id, crate::app::library::lang(l)).unwrap_or_else(|| b.id.clone());
            rows = rows.push(prop_row(p, l.text("Type", "Type").to_string(), name));
        }
        if let Some(t) = system_type {
            let name = self.library.system_name(&t.id, crate::app::library::lang(l)).unwrap_or_else(|| t.id.clone());
            rows = rows.push(prop_row(p, l.text("Type", "Type").to_string(), name));
            // a frame made in prefab units: their lengths, its frame as long as its corners make it
            if t.framing.is_some_and(|f| f.unit.is_some()) {
                if let Ok(units) = bim_core::system::joined_units(&self.modeller.model, e) {
                    let lengths: Vec<String> = units.iter().map(|u| bim_library::millimetres(*u, crate::app::library::lang(l))).collect();
                    rows = rows.push(prop_row(p, l.text("Prefab-elementen", "Prefab units").to_string(), lengths.join(" + ")));
                }
            }
            // where it meets another frame in a corner: which of the two runs through
            let ends = [l.text("Hoek bij begin", "Corner at start"), l.text("Hoek bij eind", "Corner at end")];
            for (meeting, end) in bim_core::system::corners(&self.modeller.model, e).into_iter().zip(ends) {
                let Some(meeting) = meeting else { continue };
                let role = if meeting.through { l.text("Loopt door", "Runs through") } else { l.text("Stopt tegen de andere", "Stops against the other") };
                rows = rows.push(prop_row(p, end.to_string(), role.to_string()));
            }
            // where it stands against the side of another frame, and how many stand against its own
            let tees = bim_core::system::tees(&self.modeller.model, e);
            let ends = [l.text("T-aansluiting bij begin", "T junction at start"), l.text("T-aansluiting bij eind", "T junction at end")];
            for (_, end) in tees.ends.into_iter().zip(ends).filter(|(with, _)| with.is_some()) {
                rows = rows.push(prop_row(p, end.to_string(), l.text("Staat tegen de andere", "Stands against the other").to_string()));
            }
            if !tees.sides.is_empty() {
                rows = rows.push(prop_row(p, l.text("Wanden ertegenaan", "Frames against it").to_string(), tees.sides.len().to_string()));
            }
        }
        for (f, label) in fields.into_iter().filter(|(f, _)| has(*f)) {
            // what a type gives is shown, not typed
            let fixed = match f {
                F::Thickness => layered.is_some() || system_type.is_some(),
                F::Width | F::Depth => system_type.is_some(),
                _ => false,
            };
            rows = if fixed { rows.push(prop_row(p, label.to_string(), self.element_field_text(e, f))) } else { rows.push(field(label, f)) };
        }
        // a join: its kind to choose, and an end plate's sizes
        if let Shape::Connection(c) = &shape {
            if c.kind != ConnectionKind::Cut {
                let choice = |label: &str, kind: ConnectionKind| {
                    let on = std::mem::discriminant(&c.kind) == std::mem::discriminant(&kind);
                    button(text(label.to_string()).size(12))
                        .padding([3, 10])
                        .style(move |_, s| if on { p.primary_button(s) } else { p.row_button(s) })
                        .on_press(Message::JoinKind(kind))
                };
                let name = |id: bim_core::ElementId| self.modeller.model.get(id).map_or_else(String::new, |e| e.name.clone());
                let systems = [c.primary, c.secondary].iter().any(|id| self.modeller.model.get(*id).is_some_and(|e| e.base() == bim_core::BaseKind::System));
                rows = if systems {
                    // two building systems: joined butt (the first runs through a corner) or apart
                    let turn = button(text(l.text("Andere loopt door", "Other runs through").to_string()).size(12)).padding([3, 10]).style(move |_, s| p.row_button(s)).on_press(Message::SwapJoin);
                    let rows = rows.push(row![choice(l.text("Stomp", "Butt"), ConnectionKind::Butt), choice(l.text("Los", "Apart"), ConnectionKind::Apart)].spacing(6));
                    if c.kind == ConnectionKind::Butt {
                        rows.push(prop_row(p, l.text("Loopt door", "Runs through").to_string(), name(c.primary))).push(prop_row(p, l.text("Stopt ertegen", "Stops against it").to_string(), name(c.secondary))).push(turn)
                    } else {
                        rows
                    }
                } else {
                    rows.push(
                        row![
                            choice(l.text("Verstek", "Mitre"), ConnectionKind::Mitre),
                            choice(l.text("Stomp", "Butt"), ConnectionKind::Butt),
                            choice(l.text("Kopplaat", "End plate"), ConnectionKind::EndPlate(bim_core::join::EndPlate::default())),
                        ]
                        .spacing(6),
                    )
                };
            }
            if matches!(c.kind, ConnectionKind::EndPlate(_)) {
                rows = rows
                    .push(field(l.text("Plaatdikte (m)", "Plate thickness (m)"), F::PlateThickness))
                    .push(field(l.text("Plaat voorbij profiel (m)", "Plate past profile (m)"), F::PlateMargin))
                    .push(field(l.text("Bout (m)", "Bolt (m)"), F::Bolt))
                    .push(field(l.text("Gat (m)", "Hole (m)"), F::Hole))
                    .push(field(l.text("Bouten per rij", "Bolts per row"), F::BoltRows));
            }
        }
        if let Some((openings, _)) = openings_of(&shape) {
            for i in 0..openings.len() {
                let kind = if openings[i].sill <= bim_core::LINEAR_TOLERANCE { l.text("Deur", "Door") } else { l.text("Raam", "Window") };
                let remove = button(text(l.text("Verwijderen", "Remove")).size(11)).padding([2, 8]).style(move |_, s| p.row_button(s)).on_press(Message::RemoveOpening(i));
                rows = rows
                    .push(row![text(format!("{kind} {}", i + 1)).size(12).width(Length::Fill), remove].align_y(Center))
                    .push(field(l.text("Vanaf begin (m)", "From start (m)"), F::Along(i)))
                    .push(field(l.text("Borstwering (m)", "Sill (m)"), F::Sill(i)))
                    .push(field(l.text("Breedte (m)", "Width (m)"), F::OpeningWidth(i)))
                    .push(field(l.text("Hoogte (m)", "Height (m)"), F::OpeningHeight(i)));
            }
            let add = |label: &str, door: bool| button(text(label.to_string()).size(12)).padding([3, 10]).style(move |_, s| p.row_button(s)).on_press(Message::AddOpening(door));
            rows = rows.push(row![add(l.text("Deur toevoegen", "Add door"), true), add(l.text("Raam toevoegen", "Add window"), false)].spacing(6));
        }
        let base = match e.base() {
            bim_core::BaseKind::Point => l.text("Puntvormig", "Point"),
            bim_core::BaseKind::Linear => l.text("Lijnvormig", "Linear"),
            bim_core::BaseKind::Planar => l.text("Vlak", "Planar"),
            bim_core::BaseKind::System => l.text("Systeem", "System"),
            bim_core::BaseKind::Connection => l.text("Verbinding", "Connection"),
        };
        rows = rows.push(prop_row(p, l.text("Basiselement", "Base element").to_string(), base.to_string()));
        Some(rows.into())
    }
}

/// A value to type in: its label, and a field that sends what is typed.
fn field_row(p: Palette, label: &str, value: String, field: ElementField) -> Widget<'static, Message> {
    row![
        text(label.to_string()).size(12).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph).width(Length::FillPortion(2)),
        text_input("", &value)
            .on_input(move |typed| Message::ElementField(field, typed))
            .on_submit(Message::ElementFieldDone)
            .size(12)
            .padding([2, 6])
            .width(Length::FillPortion(3)),
    ]
    .spacing(8)
    .align_y(Center)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::modeller::ModelTool;
    use bim_core::{ElementId, LinearDef, PointDef, Refusal};
    use glam::{DVec2, DVec3};

    fn type_in(app: &mut App, field: ElementField, typed: &str) {
        let _ = app.update(Message::ElementField(field, typed.into()));
    }

    /// A new model with a 5 m wall, selected.
    fn wall_selected() -> App {
        let mut app = App::default();
        app.new_model("Level 0");
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        app.add_element("Wall", None, Shape::Planar(PlanarDef { form: PlanarForm::Wall { start: DVec2::ZERO, end: DVec2::new(5.0, 0.0), base: 0.0, height: 3.0 }, thickness: 0.2, openings: Vec::new(), build_up: None }))
            .unwrap();
        app.set_model_tool(None);
        app.select_modelled(ElementId(1));
        app
    }

    fn wall(app: &App) -> PlanarDef {
        let Shape::Planar(p) = app.modeller.model.get(ElementId(1)).unwrap().shape_in_project(&app.modeller.model.grid) else { panic!() };
        p
    }

    #[test]
    fn a_thickness_typed_changes_the_wall_and_keeps_it_selected() {
        let mut app = wall_selected();
        assert_eq!(app.element_field_text(app.defined().unwrap(), ElementField::Thickness), "0.200");
        type_in(&mut app, ElementField::Thickness, "0,3");
        assert_eq!(wall(&app).thickness, 0.3);
        assert!(app.defined().is_some(), "still selected");
        let volume = app.modeller.model.evaluate(ElementId(1)).unwrap()[0].solid.volume();
        assert!((volume - 5.0 * 3.0 * 0.3).abs() < 1e-9);
    }

    #[test]
    fn a_value_the_kernel_refuses_is_kept_in_the_field_and_not_used() {
        let mut app = wall_selected();
        for typed in ["0", "-1", "abc", ""] {
            type_in(&mut app, ElementField::Height, typed);
            assert_eq!(wall(&app), wall_selected_def(), "{typed:?}");
            assert_eq!(app.element_field_text(app.defined().unwrap(), ElementField::Height), typed);
        }
        type_in(&mut app, ElementField::Height, "0");
        assert_eq!(app.modeller.refusal, Some(Refusal::NotPositive));
        let _ = app.update(Message::ElementFieldDone);
        assert_eq!(app.element_field_text(app.defined().unwrap(), ElementField::Height), "3.000");
    }

    fn wall_selected_def() -> PlanarDef {
        wall(&wall_selected())
    }

    #[test]
    fn doors_and_windows_are_added_typed_and_removed() {
        let mut app = wall_selected();
        let _ = app.update(Message::AddOpening(true));
        let _ = app.update(Message::AddOpening(false));
        // the window in the middle overlaps the door there: refused, the wall keeps one opening
        assert_eq!(wall(&app).openings.len(), 1);
        assert_eq!(app.modeller.refusal, Some(Refusal::OpeningsOverlap));
        type_in(&mut app, ElementField::Along(0), "0.5");
        assert_eq!(wall(&app).openings[0], Opening { along: 0.5, sill: 0.0, width: 0.9, height: 2.1 });
        let _ = app.update(Message::AddOpening(false));
        assert_eq!(wall(&app).openings.len(), 2);
        assert_eq!(wall(&app).openings[1], Opening { along: 1.9, sill: 0.9, width: 1.2, height: 1.2 });
        let volume = app.modeller.model.evaluate(ElementId(1)).unwrap()[0].solid.volume();
        assert!((volume - (15.0 - 0.9 * 2.1 - 1.2 * 1.2) * 0.2).abs() < 1e-9);
        let _ = app.update(Message::RemoveOpening(0));
        assert_eq!(wall(&app).openings.len(), 1);
    }

    #[test]
    fn a_join_changes_kind_and_its_end_plate_takes_typed_sizes() {
        let mut app = App::default();
        app.new_model("Level 0");
        let column = app
            .add_element("Column", None, Shape::Linear(LinearDef { start: DVec3::ZERO, end: DVec3::new(0.0, 0.0, 4.0), profile: ProfileDef::Rectangle { width: 0.3, height: 0.3 }, roll: 0.0 }))
            .unwrap();
        let beam = app
            .add_element("Beam", None, Shape::Linear(LinearDef { start: DVec3::new(0.0, 0.0, 2.5), end: DVec3::new(5.0, 0.0, 2.5), profile: ProfileDef::Rectangle { width: 0.2, height: 0.4 }, roll: 0.0 }))
            .unwrap();
        let joint = app.add_element("Joint", None, Shape::Connection(ConnectionDef { kind: ConnectionKind::Butt, primary: column, secondary: beam })).unwrap();
        let kind = |app: &App| match &app.modeller.model.get(joint).unwrap().shape {
            Shape::Connection(c) => c.kind,
            _ => unreachable!(),
        };
        // a butt to an end plate, in the Definition section: the join gets a body of its own
        app.select_modelled(joint);
        let _ = app.update(Message::JoinKind(ConnectionKind::EndPlate(bim_core::join::EndPlate::default())));
        assert!(matches!(kind(&app), ConnectionKind::EndPlate(_)));
        assert_eq!(app.modeller.model.evaluate(joint).unwrap().len(), 5, "a plate and four bolts");
        assert_eq!(app.defined().map(|e| e.id), Some(joint), "still selected");
        type_in(&mut app, ElementField::PlateThickness, "0,02");
        type_in(&mut app, ElementField::BoltRows, "3");
        let ConnectionKind::EndPlate(plate) = kind(&app) else { panic!() };
        assert_eq!((plate.thickness, plate.rows), (0.02, 3));
        type_in(&mut app, ElementField::BoltRows, "2,5");
        let ConnectionKind::EndPlate(plate) = kind(&app) else { panic!() };
        assert_eq!(plate.rows, 3, "not a whole number of bolts: kept");
        // and back to a mitre
        let _ = app.update(Message::JoinKind(ConnectionKind::Butt));
        assert_eq!(kind(&app), ConnectionKind::Butt);
    }

    #[test]
    fn a_timber_frame_takes_a_door_and_a_window_and_a_curtain_wall_none() {
        use bim_core::SystemDef;
        let mut app = App::default();
        app.new_model("Level 0");
        let s = SystemDef { start: DVec2::ZERO, end: DVec2::new(5.0, 0.0), base: 0.0, height: 2.7, spacing: DVec2::splat(1.5), member: DVec2::new(0.05, 0.15), panel: 0.02, system_type: None, openings: Vec::new() };
        let id = app.add_element("Curtain wall", None, Shape::System(s)).unwrap();
        app.select_modelled(id);
        let system = |app: &App| match app.modeller.model.get(id).unwrap().shape_in_project(&app.modeller.model.grid) {
            Shape::System(s) => s,
            _ => panic!(),
        };
        // a curtain wall has no openings to add
        let _ = app.update(Message::AddOpening(false));
        assert!(system(&app).openings.is_empty());
        assert_eq!(app.assign_system_type("system.timber-frame-140"), 1);
        {
            // 5 m in prefab units of at most 3300, on the stud lines
            let mut ui = crate::app::test_ui(&app, iced::Size::new(1400.0, 900.0));
            assert!(ui.find("Prefab units").is_ok() && ui.find("2400 + 2600").is_ok());
        }
        let _ = app.update(Message::AddOpening(false));
        assert_eq!(system(&app).openings, [Opening { along: 1.9, sill: 0.9, width: 1.2, height: 1.2 }]);
        // its header: a member as deep as the studs, 200 high, over the window and its trimmers
        let header = |app: &App| {
            app.modeller.model.evaluate(id).unwrap().iter().any(|p| {
                let (lo, hi) = p.solid.bounds();
                p.material.as_deref() == Some("timber") && (hi.z - lo.z - 0.2).abs() < 1e-9 && (lo.z - 2.1).abs() < 1e-9
            })
        };
        assert!(header(&app));
        type_in(&mut app, ElementField::Along(0), "0.5");
        assert_eq!(system(&app).openings[0].along, 0.5);
        let _ = app.update(Message::AddOpening(true));
        assert_eq!(system(&app).openings.len(), 2, "a door in the middle beside the window");
        let _ = app.update(Message::RemoveOpening(0));
        assert_eq!(system(&app).openings, [Opening { along: 2.05, sill: 0.0, width: 0.9, height: 2.1 }]);
        // the right pane lists it with its fields and the buttons to add more
        let mut ui = crate::app::test_ui(&app, iced::Size::new(1400.0, 900.0));
        assert!(ui.find("Door 1").is_ok() && ui.find("Add window").is_ok());
    }

    #[test]
    fn a_timber_frame_against_the_side_of_another_shows_its_t_junction() {
        use bim_core::system::tees;
        use bim_core::SystemDef;
        let mut app = App::default();
        app.new_model("Level 0");
        let frame = |app: &App, start: DVec2, end: DVec2, id: &str| {
            let s = SystemDef { start, end, base: 0.0, height: 2.7, spacing: DVec2::splat(1.5), member: DVec2::new(0.05, 0.15), panel: 0.02, system_type: None, openings: Vec::new() };
            Shape::System(s.typed_from(&app.library, id).unwrap())
        };
        let outer = frame(&app, DVec2::ZERO, DVec2::new(4.8, 0.0), "system.timber-frame-140");
        let inner = frame(&app, DVec2::new(2.4, 0.0), DVec2::new(2.4, 3.0), "system.timber-frame-internal-89");
        let x = app.add_element("Timber frame", None, outer).unwrap();
        // alone nothing stands against it, and its two units meet in the middle
        app.select_modelled(x);
        {
            let mut ui = crate::app::test_ui(&app, iced::Size::new(1400.0, 900.0));
            assert!(ui.find("2400 + 2400").is_ok() && ui.find("Frames against it").is_err());
        }
        let y = app.add_element("Inner wall", None, inner).unwrap();
        let model = &app.modeller.model;
        assert_eq!(tees(model, model.get(y).unwrap()).ends, [Some(x), None]);
        // the one that runs through: one frame against it, and its units meet clear of it
        app.select_modelled(x);
        {
            let mut ui = crate::app::test_ui(&app, iced::Size::new(1400.0, 900.0));
            assert!(ui.find("Frames against it").is_ok() && ui.find("T junction at start").is_err());
            assert!(ui.find("1800 + 3000").is_ok());
        }
        // what the viewer shows is the model's: its plywood left out across the other's frame
        let boarded = |app: &App| app.modeller.model.evaluate(x).unwrap().iter().filter(|p| p.material.as_deref() == Some("plywood")).map(|p| p.solid.bounds()).any(|(lo, hi)| lo.x < 2.44 && hi.x > 2.36);
        assert!(!boarded(&app));
        // the one that stands against it says at which end
        app.select_modelled(y);
        {
            let mut ui = crate::app::test_ui(&app, iced::Size::new(1400.0, 900.0));
            assert!(ui.find("T junction at start").is_ok() && ui.find("Stands against the other").is_ok());
            assert!(ui.find("T junction at end").is_err() && ui.find("Frames against it").is_err());
        }
        // made higher it no longer stands against the other, which is whole again
        type_in(&mut app, ElementField::Height, "3");
        let model = &app.modeller.model;
        assert_eq!(tees(model, model.get(y).unwrap()).ends, [None, None]);
        assert!(boarded(&app));
        app.select_modelled(x);
        let mut ui = crate::app::test_ui(&app, iced::Size::new(1400.0, 900.0));
        assert!(ui.find("2400 + 2400").is_ok() && ui.find("Frames against it").is_err());
    }

    #[test]
    fn two_timber_frames_at_a_corner_show_which_runs_through() {
        use bim_core::system::{corners, Meeting};
        use bim_core::SystemDef;
        let mut app = App::default();
        app.new_model("Level 0");
        let frame = |app: &App, start: DVec2, end: DVec2| {
            let s = SystemDef { start, end, base: 0.0, height: 2.7, spacing: DVec2::splat(1.5), member: DVec2::new(0.05, 0.15), panel: 0.02, system_type: None, openings: Vec::new() };
            Shape::System(s.typed_from(&app.library, "system.timber-frame-140").unwrap())
        };
        let (long, short) = (frame(&app, DVec2::ZERO, DVec2::new(4.8, 0.0)), frame(&app, DVec2::new(4.8, 0.0), DVec2::new(4.8, 3.0)));
        let x = app.add_element("Timber frame", None, long).unwrap();
        // alone it has no corner, and is as long as it is drawn
        app.select_modelled(x);
        {
            let mut ui = crate::app::test_ui(&app, iced::Size::new(1400.0, 900.0));
            assert!(ui.find("2400 + 2400").is_ok() && ui.find("Corner at end").is_err());
        }
        let y = app.add_element("Timber frame", None, short).unwrap();
        let model = &app.modeller.model;
        assert_eq!(corners(model, model.get(x).unwrap()), [None, Some(Meeting { with: y, through: true })]);
        // the longer one runs on to the outer face of the other's frame: 70 mm longer
        app.select_modelled(x);
        {
            let mut ui = crate::app::test_ui(&app, iced::Size::new(1400.0, 900.0));
            assert!(ui.find("Corner at end").is_ok() && ui.find("Runs through").is_ok() && ui.find("Corner at start").is_err());
            assert!(ui.find("2400 + 2470").is_ok());
        }
        // the other stops at its inner face: 70 mm shorter
        app.select_modelled(y);
        {
            let mut ui = crate::app::test_ui(&app, iced::Size::new(1400.0, 900.0));
            assert!(ui.find("Corner at start").is_ok() && ui.find("Stops against the other").is_ok());
            assert!(ui.find("2930").is_ok());
        }
        // what the viewer shows is the model's: the frame that runs through, redrawn to the corner
        let reach = |app: &App, id| app.modeller.model.evaluate(id).unwrap().iter().filter(|p| p.material.as_deref() == Some("timber")).map(|p| p.solid.bounds().1.x).fold(f64::MIN, f64::max);
        assert!((reach(&app, x) - 4.87).abs() < 1e-9);
        // a frame made higher no longer makes the corner
        type_in(&mut app, ElementField::Height, "3");
        let model = &app.modeller.model;
        assert_eq!(corners(model, model.get(x).unwrap()), [None, None]);
        assert!((reach(&app, x) - 4.8).abs() < 1e-9);
    }

    #[test]
    fn a_name_typed_renames_the_element() {
        let mut app = wall_selected();
        type_in(&mut app, ElementField::Name, "Party wall");
        assert_eq!(app.modeller.model.get(ElementId(1)).unwrap().name, "Party wall");
        type_in(&mut app, ElementField::Name, "  ");
        assert_eq!(app.modeller.model.get(ElementId(1)).unwrap().name, "Party wall");
    }

    #[test]
    fn every_value_a_shape_shows_can_be_set_back() {
        use ElementField as F;
        let shapes = [
            Shape::Point(PointDef { at: DVec3::new(1.0, 2.0, 0.5), rotation: 0.3, size: DVec3::ONE }),
            Shape::Linear(LinearDef { start: DVec3::ZERO, end: DVec3::new(4.0, 0.0, 0.0), profile: ProfileDef::Rectangle { width: 0.2, height: 0.4 }, roll: 0.1 }),
            Shape::Linear(LinearDef { start: DVec3::ZERO, end: DVec3::new(4.0, 0.0, 0.0), profile: ProfileDef::Circle { diameter: 0.3 }, roll: 0.0 }),
            wall_selected().modeller.model.get(ElementId(1)).unwrap().shape.clone(),
        ];
        for shape in shapes {
            for f in [F::Height, F::Thickness, F::Base, F::Width, F::Depth, F::Diameter, F::Turn, F::SpacingAlong, F::SpacingUp] {
                if let Some(v) = value(&shape, f) {
                    let back = with_value(&shape, f, v).unwrap();
                    assert!((value(&back, f).unwrap() - v).abs() < 1e-12, "{f:?}");
                }
            }
        }
    }
}
