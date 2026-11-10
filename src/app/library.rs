//! The explorer's Library tab: the project's library (`bim-library`) of materials, layered
//! build-ups, system types and linear types, and the steel table to take profiles from. A material
//! shows its hatch, a build-up its layers as a section, a system type its face as the kernel
//! generates it, a profile its outline. **Assign to selection** gives the selected modelled walls
//! (or slabs, for a floor, roof or ceiling type) the picked build-up, and the selected systems the
//! picked system type.
//! **Import…** adds the types a library file has and the project lacks; **Export…** saves the
//! project's library as a file of types only.

use super::{App, Message, Status, FONT_BOLD};
use crate::theme::Palette;
use bim_library::profile::{self, LinearRole, Series};
use bim_core::element::PlanarForm;
use bim_core::{BuildUpDef, ElementId, Shape, SystemDef};
use bim_library::{BuildUp, Corner, DrawingStandard, HatchKind, Lang, Library, Material, ProfileShape, Role, SystemGroup, SystemType};
use iced::mouse;
use iced::widget::canvas::{self, Frame, Path, Stroke};
use iced::widget::text::Wrapping;
use iced::widget::{button, canvas as canvas_widget, column, container, row, scrollable, text, Column};
use iced::{Center, Color, Element, Fill, Padding, Point, Rectangle, Size, Task};
use openaec_ui::Language;
use std::collections::HashSet;

/// A group of the Library tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    Materials,
    BuildUps(Role),
    Systems,
    SystemGroup(SystemGroup),
    Linear,
    Steel,
    Series(Series),
}

/// What is picked in the Library tab, shown with its drawing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    Material(String),
    BuildUp(String),
    /// A system type, by id.
    System(String),
    Linear(String),
    /// A profile of the steel table, by designation.
    Steel(String),
}

/// The Library tab's state: the groups opened and what is picked.
#[derive(Debug, Clone, Default)]
pub struct LibraryView {
    pub open: HashSet<Group>,
    pub picked: Option<Pick>,
}

#[derive(Debug, Clone)]
pub enum LibraryMessage {
    Toggle(Group),
    Pick(Pick),
    /// Give the selected modelled elements this build-up (its id).
    Assign(String),
    /// Give the selected modelled systems this system type (its id).
    AssignSystem(String),
    Import,
    /// The name and bytes of the library file picked.
    Imported(Option<(String, Vec<u8>)>),
    Export,
    /// The file written, or why not; `None` when the dialog was cancelled.
    Exported(Option<Result<String, String>>),
}

/// The library's language for an interface language.
pub fn lang(l: Language) -> Lang {
    match l {
        Language::Dutch => Lang::Nl,
        Language::English => Lang::En,
        Language::German => Lang::De,
        Language::French => Lang::Fr,
    }
}

fn role_label(role: Role, l: Language) -> &'static str {
    match role {
        Role::Wall => l.text("Wanden", "Walls"),
        Role::Floor => l.text("Vloeren", "Floors"),
        Role::Roof => l.text("Daken", "Roofs"),
        Role::Ceiling => l.text("Plafonds", "Ceilings"),
    }
}

fn system_group_label(group: SystemGroup, l: Language) -> &'static str {
    match group {
        SystemGroup::CurtainWall => l.text("Vliesgevels", "Curtain walls"),
        SystemGroup::Frame => l.text("Kozijnen", "Frames"),
        SystemGroup::TimberFrame => l.text("Houtskeletbouw", "Timber frames"),
        SystemGroup::Masonry => l.text("Metselwerk", "Masonry"),
        SystemGroup::Cladding => l.text("Gevelbekleding", "Cladding"),
        SystemGroup::Floor => l.text("Vloeren", "Floors"),
        SystemGroup::Ceiling => l.text("Plafonds", "Ceilings"),
    }
}

fn linear_role_label(role: LinearRole, l: Language) -> &'static str {
    match role {
        LinearRole::Column => l.text("Kolom", "Column"),
        LinearRole::Beam => l.text("Balk", "Beam"),
        LinearRole::Member => l.text("Staaf", "Member"),
        LinearRole::Mullion => l.text("Stijl", "Mullion"),
        LinearRole::Skirting => l.text("Plint", "Skirting"),
        LinearRole::Pipe => l.text("Leiding", "Pipe"),
        LinearRole::Duct => l.text("Kanaal", "Duct"),
        LinearRole::CableTray => l.text("Kabelgoot", "Cable tray"),
        LinearRole::Pile => l.text("Paal", "Pile"),
    }
}

fn rgb(c: [u8; 3]) -> Color {
    Color::from_rgb8(c[0], c[1], c[2])
}

/// Millimetres with up to one decimal.
fn mm(metres: f64, l: Language) -> String {
    bim_library::millimetres(metres, lang(l))
}

impl App {
    pub(super) fn library_update(&mut self, message: LibraryMessage) -> Task<Message> {
        match message {
            LibraryMessage::Toggle(group) => {
                if !self.library_view.open.remove(&group) {
                    self.library_view.open.insert(group);
                }
            }
            LibraryMessage::Pick(pick) => {
                self.library_view.picked = if self.library_view.picked.as_ref() == Some(&pick) { None } else { Some(pick) };
            }
            LibraryMessage::Assign(id) => {
                self.assign_build_up(&id);
            }
            LibraryMessage::AssignSystem(id) => {
                self.assign_system_type(&id);
            }
            LibraryMessage::Import => return import_task(),
            LibraryMessage::Imported(None) => {}
            LibraryMessage::Imported(Some((name, bytes))) => self.status = self.import_library(&name, &bytes),
            LibraryMessage::Export => return export_task(&self.library),
            LibraryMessage::Exported(None) => {}
            LibraryMessage::Exported(Some(Ok(name))) => self.status = Status::Loaded(format!("Library saved as {name}")),
            LibraryMessage::Exported(Some(Err(e))) => self.status = Status::Error(e),
        }
        Task::none()
    }

    /// The selected modelled elements a build-up of `role` fits: walls for a wall type, slabs for a
    /// floor, roof or ceiling type.
    pub(super) fn assignable(&self, role: Role) -> Vec<ElementId> {
        let fits = |id: &ElementId| match self.modeller.model.get(*id).map(|e| &e.shape) {
            Some(Shape::Planar(p)) => matches!(p.form, PlanarForm::Wall { .. }) == (role == Role::Wall),
            _ => false,
        };
        self.selected_modelled().into_iter().filter(fits).collect()
    }

    /// Gives the selected elements build-up `id` of the library, each as far as the kernel can make
    /// it (a refusal shows); the first stays selected. Returns how many took it.
    pub(super) fn assign_build_up(&mut self, id: &str) -> usize {
        let (Some(b), Some(def)) = (self.library.buildup(id), BuildUpDef::from_library(&self.library, id)) else { return 0 };
        let targets = self.assignable(b.role);
        let mut assigned = 0;
        for target in &targets {
            let Some(mut element) = self.modeller.model.get(*target).cloned() else { continue };
            if let Shape::Planar(p) = &mut element.shape {
                *p = p.clone().with_build_up(Some(def.clone()));
            }
            match self.modeller.model.replace(element) {
                Ok(()) => assigned += 1,
                Err(refusal) => self.modeller.refusal = Some(refusal),
            }
        }
        if assigned > 0 {
            self.show_modelled();
            if let Some(first) = targets.first() {
                self.select_modelled(*first);
            }
        }
        assigned
    }

    /// The selected modelled systems system type `id` fits: every selected system for an upright
    /// type, none for a lying one (a floor, a ceiling), which an upright system cannot be.
    pub(super) fn assignable_systems(&self, id: &str) -> Vec<ElementId> {
        if self.library.system_type(id).is_none_or(|t| t.lying.is_some()) {
            return Vec::new();
        }
        let fits = |e: &ElementId| matches!(self.modeller.model.get(*e).map(|e| &e.shape), Some(Shape::System(_)));
        self.selected_modelled().into_iter().filter(fits).collect()
    }

    /// Gives the selected systems system type `id` of the library: its grid, members, panels and
    /// layers, each as far as the kernel can make it (a refusal shows); the first stays selected.
    /// Returns how many took it.
    pub(super) fn assign_system_type(&mut self, id: &str) -> usize {
        let targets = self.assignable_systems(id);
        let mut assigned = 0;
        for target in &targets {
            let Some(mut element) = self.modeller.model.get(*target).cloned() else { continue };
            let Shape::System(s) = &element.shape else { continue };
            let Some(typed) = s.clone().typed_from(&self.library, id) else { continue };
            element.shape = Shape::System(typed);
            match self.modeller.model.replace(element) {
                Ok(()) => assigned += 1,
                Err(refusal) => self.modeller.refusal = Some(refusal),
            }
        }
        if assigned > 0 {
            self.show_modelled();
            if let Some(first) = targets.first() {
                self.select_modelled(*first);
            }
        }
        assigned
    }

    /// Adds the types of the library file `name` that the project lacks.
    pub(super) fn import_library(&mut self, name: &str, bytes: &[u8]) -> Status {
        let read = serde_json::from_slice(bytes).map_err(|e| e.to_string()).and_then(|v| bim_library::ifcx::read(&v).map_err(|e| e.to_string()));
        match read {
            Ok(other) => {
                let added = self.library.merge(&other);
                Status::Loaded(format!("{name}: {added} added to the library"))
            }
            Err(e) => Status::Error(format!("{name}: {e}")),
        }
    }

    /// The Library tab.
    pub(super) fn library_body(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let lib = &self.library;
        let lg = lang(l);
        let view = &self.library_view;
        let open = |g: Group| view.open.contains(&g);
        let send = |m: LibraryMessage| Message::Library(m);
        let header = |label: String, count: usize, group: Group| -> Element<'_, Message> {
            let mark = if open(group) { "▾" } else { "▸" };
            button(
                row![
                    text(mark).size(11).color(p.text_secondary),
                    text(label).size(12).font(FONT_BOLD).width(Fill),
                    text(count.to_string()).size(11).color(p.text_secondary),
                ]
                .spacing(6)
                .align_y(Center),
            )
            .padding([4, 6])
            .width(Fill)
            .style(move |_, s| p.ribbon_button(false, s))
            .on_press(send(LibraryMessage::Toggle(group)))
            .into()
        };
        let item = |label: Element<'static, Message>, pick: Pick| -> Element<'_, Message> {
            let picked = view.picked.as_ref() == Some(&pick);
            button(label)
                .padding(Padding { top: 3.0, right: 6.0, bottom: 3.0, left: 18.0 })
                .width(Fill)
                .style(move |_, s| if picked { p.ribbon_button(true, s) } else { p.row_button(s) })
                .on_press(send(LibraryMessage::Pick(pick)))
                .into()
        };
        let name = |s: String| text(s).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph);

        let actions = row![
            button(text(l.text("Importeren…", "Import…")).size(12)).padding([4, 10]).style(move |_, s| p.ribbon_button(false, s)).on_press(send(LibraryMessage::Import)),
            button(text(l.text("Exporteren…", "Export…")).size(12)).padding([4, 10]).style(move |_, s| p.ribbon_button(false, s)).on_press(send(LibraryMessage::Export)),
        ]
        .spacing(6);
        let mut body = Column::new().spacing(1).push(container(actions).padding([2, 4]));

        // materials
        body = body.push(header(l.text("Materialen", "Materials").into(), lib.materials.len(), Group::Materials));
        if open(Group::Materials) {
            for m in &lib.materials {
                let colour = rgb(m.colour);
                let swatch = container(text("")).width(12).height(12).style(move |_| Palette::swatch(colour));
                let pick = Pick::Material(m.key.clone());
                body = body.push(item(row![swatch, name(m.name(lg).to_string())].spacing(6).align_y(Center).into(), pick.clone()));
                if view.picked.as_ref() == Some(&pick) {
                    body = body.push(detail(material_detail(p, m)));
                }
            }
        }

        // build-ups, per role
        for role in Role::ALL {
            let group = Group::BuildUps(role);
            let names = lib.names(role, lg);
            body = body.push(header(role_label(role, l).into(), names.len(), group));
            if !open(group) {
                continue;
            }
            for (id, label) in names {
                let pick = Pick::BuildUp(id.to_string());
                body = body.push(item(name(label).into(), pick.clone()));
                if view.picked.as_ref() == Some(&pick) {
                    if let Some(b) = lib.buildup(id) {
                        body = body.push(detail(buildup_detail(p, l, lib, b)));
                        let fits = self.assignable(role).len();
                        if fits > 0 {
                            let label = format!("{} ({fits})", l.text("Toewijzen aan selectie", "Assign to selection"));
                            let assign = button(text(label).size(12)).padding([4, 10]).style(move |_, s| p.primary_button(s)).on_press(send(LibraryMessage::Assign(id.to_string())));
                            body = body.push(detail(assign.into()));
                        }
                    }
                }
            }
        }

        // system types, per group
        body = body.push(header(l.text("Systemen", "Systems").into(), lib.system_types.len(), Group::Systems));
        if open(Group::Systems) {
            for g in SystemGroup::ALL {
                let names = lib.system_names(g, lg);
                if names.is_empty() {
                    continue;
                }
                let group = Group::SystemGroup(g);
                body = body.push(indented(header(system_group_label(g, l).into(), names.len(), group)));
                if !open(group) {
                    continue;
                }
                for (id, label) in names {
                    let pick = Pick::System(id.to_string());
                    body = body.push(indented(item(name(label).into(), pick.clone())));
                    if view.picked.as_ref() != Some(&pick) {
                        continue;
                    }
                    if let Some(t) = lib.system_type(id) {
                        body = body.push(indented(detail(system_detail(p, l, lib, t))));
                        let fits = self.assignable_systems(id).len();
                        if fits > 0 {
                            let label = format!("{} ({fits})", l.text("Toewijzen aan selectie", "Assign to selection"));
                            let assign = button(text(label).size(12)).padding([4, 10]).style(move |_, s| p.primary_button(s)).on_press(send(LibraryMessage::AssignSystem(id.to_string())));
                            body = body.push(indented(detail(assign.into())));
                        }
                    }
                }
            }
        }

        // linear types, by role
        body = body.push(header(l.text("Lijntypen", "Linear types").into(), lib.linear_types.len(), Group::Linear));
        if open(Group::Linear) {
            for role in LinearRole::ALL {
                for (id, label) in lib.linear_names(role, lg) {
                    let pick = Pick::Linear(id.to_string());
                    let line = column![name(label), text(linear_role_label(role, l)).size(10).color(p.text_secondary)];
                    body = body.push(item(line.into(), pick.clone()));
                    if view.picked.as_ref() == Some(&pick) {
                        if let Some(t) = lib.linear_type(id) {
                            body = body.push(detail(profile_detail(p, &t.profile, lib.material(&t.material), None)));
                        }
                    }
                }
            }
        }

        // the steel table, per series
        let steel = profile::steel_profiles();
        body = body.push(header(l.text("Staalprofielen", "Steel profiles").into(), steel.len(), Group::Steel));
        if open(Group::Steel) {
            for series in Series::ALL {
                let group = Group::Series(series);
                let of: Vec<_> = steel.iter().filter(|s| s.series == series).collect();
                let title = match series {
                    Series::LEqual => format!("L {}", l.text("gelijkzijdig", "equal")),
                    Series::LUnequal => format!("L {}", l.text("ongelijkzijdig", "unequal")),
                    s => s.name().to_string(),
                };
                body = body.push(container(header(title, of.len(), group)).padding(Padding { left: 12.0, ..Padding::ZERO }));
                if !open(group) {
                    continue;
                }
                let deeper = |e: Element<'static, Message>| container(e).padding(Padding { left: 12.0, ..Padding::ZERO });
                for s in of {
                    let pick = Pick::Steel(s.designation.clone());
                    body = body.push(container(item(name(s.designation.clone()).into(), pick.clone())).padding(Padding { left: 12.0, ..Padding::ZERO }));
                    if view.picked.as_ref() == Some(&pick) {
                        body = body.push(deeper(detail(profile_detail(p, &s.shape, lib.material("steel"), Some(s.series.standard())))));
                    }
                }
            }
        }
        scrollable(body.padding(Padding { top: 0.0, right: 4.0, bottom: 8.0, left: 0.0 })).height(Fill).spacing(4).into()
    }
}

/// An item of a group within a group, indented.
fn indented<'a>(e: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(e).padding(Padding { left: 12.0, ..Padding::ZERO }).into()
}

/// The detail of a picked item, indented under it.
fn detail(content: Element<'_, Message>) -> Element<'_, Message> {
    container(content).padding(Padding { top: 4.0, right: 8.0, bottom: 8.0, left: 18.0 }).width(Fill).into()
}

fn small<'a>(s: String, p: Palette) -> Element<'a, Message> {
    text(s).size(11).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph).into()
}

/// A material's hatch (NEN 47) on a square, and its key.
fn material_detail<'a>(p: Palette, m: &Material) -> Element<'a, Message> {
    let sample = Section { bands: vec![(1.0, m.hatch, m.colour)], depth: 1.0, line: p.text };
    column![canvas_widget(sample).width(Fill).height(60), small(m.key.clone(), p)].spacing(4).into()
}

/// A build-up drawn as a section through its layers, the exterior (or the top) at the top, and
/// its layers listed.
fn buildup_detail<'a>(p: Palette, l: Language, lib: &Library, b: &BuildUp) -> Element<'a, Message> {
    let colour = |key: &str| lib.material(key).map_or([175, 182, 195], |m| m.colour);
    let hatch = |key: &str| lib.material(key).map_or(HatchKind::Generic, |m| m.hatch);
    let bands = b.layers.iter().map(|layer| (layer.thickness, hatch(&layer.material), colour(&layer.material))).collect();
    let total = b.thickness();
    let height = (total * 400.0).clamp(24.0, 150.0) as f32;
    let mut list = Column::new().spacing(2);
    let side = if b.role.counted_from_top() { l.text("Bovenzijde boven", "Top side up") } else { l.text("Buitenzijde boven", "Exterior side up") };
    list = list.push(small(side.to_string(), p));
    for layer in &b.layers {
        let material = lib.material(&layer.material);
        let c = rgb(colour(&layer.material));
        let swatch = container(text("")).width(10).height(10).style(move |_| Palette::swatch(c));
        let label = material.map_or(layer.material.clone(), |m| m.name(lang(l)).to_string());
        list = list.push(
            row![swatch, text(label).size(11).color(p.text).width(Fill).wrapping(Wrapping::WordOrGlyph), text(mm(layer.thickness, l)).size(11).color(p.text_secondary)]
                .spacing(6)
                .align_y(Center),
        );
    }
    let class = b.role.ifc_type_class();
    list = list.push(small(format!("{} mm · {class} {}", mm(total, l), b.role.predefined_type()), p));
    column![canvas_widget(Section { bands, depth: total, line: p.text }).width(Fill).height(height), list].spacing(6).into()
}

/// A system type drawn as its face, a few bays and rows of it as the kernel generates them, with a
/// section through its layers when it has any, and its grid, members, panels and layers listed.
fn system_detail<'a>(p: Palette, l: Language, lib: &Library, t: &SystemType) -> Element<'a, Message> {
    let colour = |key: &str| lib.material(key).map_or([175, 182, 195], |m| m.colour);
    let material = |key: &str| lib.material(key).map_or(key.to_string(), |m| m.name(lang(l)).to_string());
    let line = |key: &str, what: String| -> Element<'a, Message> {
        let c = rgb(colour(key));
        let swatch = container(text("")).width(10).height(10).style(move |_| Palette::swatch(c));
        row![swatch, text(what).size(11).color(p.text).width(Fill).wrapping(Wrapping::WordOrGlyph)].spacing(6).align_y(Center).into()
    };
    let size = |w: f64, d: f64| format!("{}×{}", mm(w, l), mm(d, l));
    let mut list = Column::new().spacing(2);
    let pattern = t.grid.pattern.name(t.group, lang(l)).map_or(String::new(), |n| format!(" · {n}"));
    list = list.push(small(format!("{} {} × {} mm{pattern}", l.text("Raster", "Grid"), mm(t.grid.bay, l), mm(t.grid.row, l)), p));
    // what the members are: a frame's mullions and transoms, a ceiling's profiles, or joints
    let members = match t.group {
        SystemGroup::CurtainWall | SystemGroup::Frame | SystemGroup::TimberFrame => l.text("Stijlen en regels", "Members"),
        SystemGroup::Ceiling => l.text("Profielen", "Profiles"),
        SystemGroup::Masonry | SystemGroup::Cladding | SystemGroup::Floor => l.text("Voegen", "Joints"),
    };
    list = list.push(line(&t.members.material, format!("{members}: {} {}", material(&t.members.material), size(t.members.width, t.members.depth))));
    if let Some(framing) = t.members.framing {
        list = list.push(small(format!("{} {} · {} {}", l.text("Onderregels", "Bottom plates"), framing.bottom_plates, l.text("bovenregels", "top plates"), framing.top_plates), p));
    }
    list = list.push(line(&t.panels.material, format!("{}: {} {}", l.text("Panelen", "Panels"), material(&t.panels.material), mm(t.panels.thickness, l))));
    for layer in &t.layers {
        let sign = if layer.offset < 0.0 { "−" } else { "+" };
        let members = layer.members.as_ref().map_or(String::new(), |m| format!(", {} {}", material(&m.material), size(m.width, m.depth)));
        let what = format!(
            "{} {sign}{}: {} {}{members}, {} {} × {}",
            l.text("Laag", "Layer"),
            mm(layer.offset.abs(), l),
            material(&layer.panels.material),
            mm(layer.panels.thickness, l),
            l.text("raster", "grid"),
            mm(layer.grid.bay, l),
            mm(layer.grid.row, l)
        );
        list = list.push(line(&layer.panels.material, what));
    }
    if t.corners == Corner::Mitre {
        list = list.push(small(l.text("Hoeken in verstek", "Mitred corners").to_string(), p));
    }
    if let Some(height) = t.lying {
        list = list.push(small(format!("{} {} mm", l.text("Liggend op", "Lying at"), mm(height, l)), p));
    }
    let (class, predefined) = t.group.ifc_type();
    list = list.push(small(format!("{class} {predefined}"), p));
    let face = elevation(t, lib, p.text);
    // as high as the face's shape asks, in a canvas about 240 wide
    let tall = (240.0 * face.size[1] / face.size[0]).clamp(40.0, 170.0) as f32;
    let mut drawn = column![canvas_widget(face).width(Fill).height(tall)].spacing(6);
    if !t.layers.is_empty() {
        let section = system_section(t, lib, p.text);
        let height = (section.depth * 400.0).clamp(24.0, 150.0) as f32;
        drawn = drawn.push(canvas_widget(section).width(Fill).height(height));
    }
    drawn.push(list).into()
}

/// The face of system type `t` as the kernel generates it, a few bays wide (as wide as a row is
/// high, at least 1.2 m) and rows high (a lying type as if it stood, without its layers): its
/// panels, then its members, as rectangles along and up the face in their materials' colours.
fn elevation(t: &SystemType, lib: &Library, line: Color) -> Elevation {
    let bays = (t.grid.row.max(1.2) / t.grid.bay).ceil().clamp(2.0, 8.0);
    let rows = (0.9 / t.grid.row).ceil().clamp(1.0, 8.0);
    let (length, height) = (bays * t.grid.bay, rows * t.grid.row);
    let upright = SystemType { lying: None, layers: Vec::new(), ..t.clone() };
    let face = SystemDef { start: glam::DVec2::ZERO, end: glam::DVec2::new(length, 0.0), base: 0.0, height, spacing: glam::DVec2::ONE, member: glam::DVec2::ONE, panel: 1.0, system_type: None };
    let mut parts = face.typed(&upright).and_then(|s| bim_core::system::parts(&s).ok()).unwrap_or_default();
    // panels first, so the members lie over their edges
    parts.sort_by_key(|part| part.class != "IfcPlate");
    let colour = |key: Option<&str>| key.and_then(|k| lib.material(k)).map_or([175, 182, 195], |m| m.colour);
    let rects = parts
        .iter()
        .map(|part| {
            let (lo, hi) = part.solid.bounds();
            ([lo.x, lo.z, hi.x, hi.z], colour(part.material.as_deref()))
        })
        .collect();
    Elevation { rects, size: [length, height], line }
}

/// A section through a system type's layers, the exterior at the top: each face as thick as its
/// deepest part, in its panels' hatch, and air where faces leave a gap.
fn system_section(t: &SystemType, lib: &Library, line: Color) -> Section {
    let mut faces: Vec<(f64, f64, &str)> = t
        .faces()
        .map(|(offset, _, members, panels)| {
            let half = members.map_or(0.0, |m| m.depth).max(panels.thickness) / 2.0;
            (offset + half, offset - half, panels.material.as_str())
        })
        .collect();
    faces.sort_by(|a, b| b.0.total_cmp(&a.0));
    let colour = |key: &str| lib.material(key).map_or([175, 182, 195], |m| m.colour);
    let hatch = |key: &str| lib.material(key).map_or(HatchKind::Generic, |m| m.hatch);
    let mut bands = Vec::new();
    let mut at = faces.first().map_or(0.0, |f| f.0);
    for (top, bottom, material) in faces {
        if at - top > 1e-6 {
            bands.push((at - top, HatchKind::Air, [175, 182, 195]));
        }
        let top = top.min(at);
        if top - bottom > 1e-6 {
            bands.push((top - bottom, hatch(material), colour(material)));
            at = bottom;
        }
    }
    let depth = bands.iter().map(|b| b.0).sum();
    Section { bands, depth, line }
}

/// A profile drawn as its outline, with its sizes, area and second moments.
fn profile_detail<'a>(p: Palette, shape: &ProfileShape, material: Option<&Material>, standard: Option<&'static str>) -> Element<'a, Message> {
    let colour = material.map_or([95, 100, 110], |m| m.colour);
    let rings = shape.rings(1e-4).unwrap_or_default();
    let size = shape.size();
    let mut lines = vec![match *shape {
        ProfileShape::Rectangle { width, depth } => format!("b {} · h {} mm", mm_en(width), mm_en(depth)),
        ProfileShape::Circle { diameter } => format!("d {} mm", mm_en(diameter)),
        ProfileShape::I { h, b, tw, tf, r, .. } | ProfileShape::U { h, b, tw, tf, r, .. } => format!("h {} · b {} · tw {} · tf {} · r {} mm", mm_en(h), mm_en(b), mm_en(tw), mm_en(tf), mm_en(r)),
        ProfileShape::L { h, b, t, r1, .. } => format!("h {} · b {} · t {} · r {} mm", mm_en(h), mm_en(b), mm_en(t), mm_en(r1)),
        ProfileShape::T { h, b, tw, tf, .. } => format!("h {} · b {} · tw {} · tf {} mm", mm_en(h), mm_en(b), mm_en(tw), mm_en(tf)),
        ProfileShape::Rhs { h, b, t, .. } => format!("h {} · b {} · t {} mm", mm_en(h), mm_en(b), mm_en(t)),
        ProfileShape::Chs { d, t } => format!("d {} · t {} mm", mm_en(d), mm_en(t)),
    }];
    if let Ok(props) = shape.props() {
        lines.push(format!("A {:.1} cm² · Iy {:.0} cm⁴ · Iz {:.0} cm⁴", props.area * 1e4, props.iu * 1e8, props.iv * 1e8));
    }
    if let Some(standard) = standard {
        lines.push(standard.to_string());
    }
    let mut list = Column::new().spacing(2);
    for line in lines {
        list = list.push(small(line, p));
    }
    let outline = Outline { outer: rings.0, holes: rings.1, size: [size.x, size.y], fill: rgb(colour), line: p.text };
    column![canvas_widget(outline).width(Fill).height(130), list].spacing(6).into()
}

/// Millimetres as in a designation: whole, or one decimal with a point.
fn mm_en(metres: f64) -> String {
    bim_library::millimetres(metres, Lang::En)
}

/// A section through layers: bands from the top down, each `thickness` of `depth` with its hatch.
struct Section {
    bands: Vec<(f64, HatchKind, [u8; 3])>,
    depth: f64,
    line: Color,
}

impl canvas::Program<Message> for Section {
    type State = ();

    fn draw(&self, _state: &(), renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let width = (bounds.width - 2.0).min(220.0);
        let scale = (bounds.height - 2.0) / self.depth.max(1e-6) as f32; // px per metre
        let mut top = 1.0;
        for &(thickness, kind, _) in &self.bands {
            let h = thickness as f32 * scale;
            let band = Rectangle::new(Point::new(1.0, top), Size::new(width, h));
            draw_hatch(&mut frame, band, kind.pattern(DrawingStandard::Nen47), scale);
            top += h;
        }
        top = 1.0;
        for &(thickness, _, _) in &self.bands {
            let h = thickness as f32 * scale;
            frame.stroke(&Path::rectangle(Point::new(1.0, top), Size::new(width, h)), Stroke::default().with_color(self.line).with_width(1.0));
            top += h;
        }
        vec![frame.into_geometry()]
    }
}

/// A hatch in a band: its fill, then its lines (angle in degrees on paper, spacing in metres on
/// the model at `scale` px per metre), crossed lines, or one zigzag along the band.
fn draw_hatch(frame: &mut Frame, band: Rectangle, hatch: bim_library::HatchPattern, scale: f32) {
    frame.fill_rectangle(band.position(), band.size(), rgb(hatch.fill));
    let ink = Stroke::default().with_color(rgb(hatch.line)).with_width(0.8);
    if hatch.zigzag {
        let (x0, x1, y0, y1) = (band.x, band.x + band.width, band.y, band.y + band.height);
        let step = band.height.max(4.0);
        let path = Path::new(|b| {
            b.move_to(Point::new(x0, y1));
            let mut x = x0;
            let mut up = true;
            while x < x1 {
                x = (x + step / 2.0).min(x1);
                b.line_to(Point::new(x, if up { y0 } else { y1 }));
                up = !up;
            }
        });
        frame.stroke(&path, ink);
        return;
    }
    if hatch.spacing <= 0.0 {
        return;
    }
    let spacing = (hatch.spacing as f32 * scale).max(3.0);
    for angle in [hatch.angle as f32].into_iter().chain(hatch.crossed.then_some(hatch.angle as f32 + 90.0)) {
        for (a, b) in hatch_lines(band, angle, spacing) {
            frame.stroke(&Path::line(a, b), ink);
        }
    }
}

/// The parallel lines `spacing` apart at `angle` degrees (anticlockwise on paper, so clockwise on
/// screen, whose y runs down) clipped to `band`.
fn hatch_lines(band: Rectangle, angle: f32, spacing: f32) -> Vec<(Point, Point)> {
    let (sin, cos) = angle.to_radians().sin_cos();
    let d = (cos, -sin); // along the lines, on screen
    let n = (sin, cos); // across them
    let corners = [(band.x, band.y), (band.x + band.width, band.y), (band.x, band.y + band.height), (band.x + band.width, band.y + band.height)];
    let across: Vec<f32> = corners.iter().map(|c| c.0 * n.0 + c.1 * n.1).collect();
    let (lo, hi) = across.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
    let mut lines = Vec::new();
    let mut c = (lo / spacing).ceil() * spacing;
    while c <= hi {
        // the line n·p = c through the band: clip p = p0 + t d to the rectangle
        let p0 = (n.0 * c, n.1 * c);
        let (mut t0, mut t1) = (f32::NEG_INFINITY, f32::INFINITY);
        let mut inside = true;
        for (o, dir, min, max) in [(p0.0, d.0, band.x, band.x + band.width), (p0.1, d.1, band.y, band.y + band.height)] {
            if dir.abs() < 1e-6 {
                inside &= o >= min && o <= max;
            } else {
                let (a, b) = ((min - o) / dir, (max - o) / dir);
                t0 = t0.max(a.min(b));
                t1 = t1.min(a.max(b));
            }
        }
        if inside && t1 > t0 {
            lines.push((Point::new(p0.0 + d.0 * t0, p0.1 + d.1 * t0), Point::new(p0.0 + d.0 * t1, p0.1 + d.1 * t1)));
        }
        c += spacing;
    }
    lines
}

/// A profile's outline and holes, fitted in the canvas.
struct Outline {
    outer: Vec<glam::DVec2>,
    holes: Vec<Vec<glam::DVec2>>,
    size: [f64; 2],
    fill: Color,
    line: Color,
}

impl canvas::Program<Message> for Outline {
    type State = ();

    fn draw(&self, _state: &(), renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        if self.outer.len() < 3 {
            return vec![frame.into_geometry()];
        }
        let scale = ((bounds.height - 8.0) / self.size[1].max(1e-6) as f32).min((bounds.width.min(220.0) - 8.0) / self.size[0].max(1e-6) as f32);
        let centre = Point::new(4.0 + self.size[0] as f32 * scale / 2.0, bounds.height / 2.0);
        let at = |q: &glam::DVec2| Point::new(centre.x + q.x as f32 * scale, centre.y - q.y as f32 * scale);
        let path = Path::new(|b| {
            for ring in std::iter::once(&self.outer).chain(&self.holes) {
                b.move_to(at(&ring[0]));
                for q in &ring[1..] {
                    b.line_to(at(q));
                }
                b.close();
            }
        });
        frame.fill(&path, canvas::Fill { style: canvas::Style::Solid(self.fill), rule: canvas::fill::Rule::EvenOdd });
        frame.stroke(&path, Stroke::default().with_color(self.line).with_width(1.0));
        vec![frame.into_geometry()]
    }
}

/// A system's face: rectangles (along, up, along, up) in their colours, fitted in the canvas.
struct Elevation {
    rects: Vec<([f64; 4], [u8; 3])>,
    size: [f64; 2],
    line: Color,
}

impl canvas::Program<Message> for Elevation {
    type State = ();

    fn draw(&self, _state: &(), renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let room = (bounds.width.min(260.0) - 8.0, bounds.height - 8.0);
        let scale = (room.0 / self.size[0].max(1e-6) as f32).min(room.1 / self.size[1].max(1e-6) as f32);
        let bottom = 4.0 + self.size[1] as f32 * scale;
        let edge = Stroke::default().with_color(Color { a: 0.6, ..self.line }).with_width(0.5);
        for (r, c) in &self.rects {
            let corner = Point::new(4.0 + r[0] as f32 * scale, bottom - r[3] as f32 * scale);
            let size = Size::new(((r[2] - r[0]) as f32 * scale).max(0.5), ((r[3] - r[1]) as f32 * scale).max(0.5));
            frame.fill_rectangle(corner, size, rgb(*c));
            frame.stroke(&Path::rectangle(corner, size), edge);
        }
        vec![frame.into_geometry()]
    }
}

/// Asks for a library file and reads it: the native dialog on the desktop, the browser's file
/// picker on the web.
fn import_task() -> Task<Message> {
    Task::perform(
        async {
            let file = rfd::AsyncFileDialog::new().set_title("Import library").add_filter("IFCX", &["ifcx"]).pick_file().await?;
            Some((file.file_name(), file.read().await))
        },
        |picked| Message::Library(LibraryMessage::Imported(picked)),
    )
}

/// The library file's bytes: IFCX of the library's types alone.
pub fn library_bytes(library: &Library, name: &str) -> Vec<u8> {
    let file = bim_library::ifcx::file(library, name, &format!("Open 3D Viewer Studio {}", env!("CARGO_PKG_VERSION")), &crate::export::ifcx::timestamp());
    serde_json::to_vec(&file).unwrap_or_default()
}

/// Asks where to save the library and writes it there.
#[cfg(not(target_arch = "wasm32"))]
fn export_task(library: &Library) -> Task<Message> {
    let library = library.clone();
    Task::perform(
        async move {
            let path = rfd::AsyncFileDialog::new().set_title("Export library").set_file_name("library.ifcx").add_filter("IFCX", &["ifcx"]).save_file().await?.path().to_path_buf();
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let bytes = library_bytes(&library, &name);
            Some(std::fs::write(&path, bytes).map(|()| name.clone()).map_err(|e| format!("Cannot save {name}: {e}")))
        },
        |done| Message::Library(LibraryMessage::Exported(done)),
    )
}

/// The browser has no save dialog: the library goes to the downloads.
#[cfg(target_arch = "wasm32")]
fn export_task(library: &Library) -> Task<Message> {
    let bytes = library_bytes(library, "library.ifcx");
    Task::done(Message::Library(LibraryMessage::Exported(Some(crate::web::download("library.ifcx", &bytes, "application/json").map(|()| "library.ifcx".to_string())))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simulator(app: &App) -> iced_test::Simulator<'_, Message> {
        iced_test::Simulator::with_size(iced::Settings::default(), iced::Size::new(1400.0, 900.0), app.view())
    }

    fn click(app: &mut App, label: &str) {
        let mut ui = simulator(app);
        ui.click(label).unwrap_or_else(|_| panic!("no {label}"));
        let messages: Vec<Message> = ui.into_messages().collect();
        for m in messages {
            let _ = app.update(m);
        }
    }

    #[test]
    fn the_library_tab_lists_the_starter_library_and_draws_a_picked_type() {
        let mut app = App::default();
        click(&mut app, "Library");
        assert_eq!(app.explorer_tab, super::super::ExplorerTab::Library);
        click(&mut app, "Walls");
        click(&mut app, "Brickwork-air cavity-PIR insulation-sand-lime brick 360");
        assert_eq!(app.library_view.picked, Some(Pick::BuildUp("wall.cavity-pir-360".into())));
        // its layers are listed under it
        {
            let mut ui = simulator(&app);
            assert!(ui.find("PIR insulation").is_ok());
            assert!(ui.find("Exterior side up").is_ok());
        }
        // the walls closed again, so the steel table is in view
        click(&mut app, "Walls");
        click(&mut app, "Steel profiles");
        click(&mut app, "HEA");
        click(&mut app, "HEA 200");
        assert_eq!(app.library_view.picked, Some(Pick::Steel("HEA 200".into())));
        let mut ui = simulator(&app);
        assert!(ui.find("A 53.8 cm² · Iy 3692 cm⁴ · Iz 1336 cm⁴").is_ok());
    }

    #[test]
    fn importing_a_library_file_adds_what_the_project_lacks() {
        let mut app = App::default();
        let mut other = Library::starter();
        other.add_linear_type(bim_library::LinearType::steel("column.hea-200", LinearRole::Column, "HEA 200").unwrap()).unwrap();
        let bytes = library_bytes(&other, "other.ifcx");
        let status = app.import_library("other.ifcx", &bytes);
        assert!(matches!(&status, Status::Loaded(s) if s == "other.ifcx: 1 added to the library"), "{status:?}");
        assert!(app.library.linear_type("column.hea-200").is_some());
        assert!(matches!(app.import_library("x.ifcx", b"{}"), Status::Error(_)));
    }

    #[test]
    fn a_selected_wall_takes_a_build_up_and_keeps_it_in_ifcx() {
        use bim_core::{PlanarDef, Shape};
        use glam::DVec2;
        let mut app = App::default();
        app.new_model("Level 0");
        let wall = PlanarDef { form: PlanarForm::Wall { start: DVec2::ZERO, end: DVec2::new(5.0, 0.0), base: 0.0, height: 3.0 }, thickness: 0.2, openings: Vec::new(), build_up: None };
        let id = app.add_element("Wall", None, Shape::Planar(wall)).unwrap();
        app.select_modelled(id);
        assert_eq!(app.assignable(Role::Wall), vec![id]);
        assert!(app.assignable(Role::Floor).is_empty(), "a floor type does not fit a wall");
        assert_eq!(app.assign_build_up("wall.cavity-pir-360"), 1);
        let Shape::Planar(p) = &app.modeller.model.get(id).unwrap().shape else { panic!() };
        assert!((p.total_thickness() - 0.36).abs() < 1e-12 && p.build_up.as_ref().unwrap().id == "wall.cavity-pir-360");
        assert_eq!(app.selected_modelled(), vec![id], "it stays selected");
        // three solids (the cavity has none) in the colours of brick, PIR and sand-lime
        let parts = app.modeller.model.evaluate(id).unwrap();
        assert_eq!(parts.iter().map(|p| p.material.as_deref().unwrap()).collect::<Vec<_>>(), ["brick", "insulation-pir", "sand-lime"]);
        let scene = super::super::modeller::viewer_model(&app.modeller.model, &[], &app.library);
        let colours: std::collections::HashSet<[u8; 4]> = scene.layers[0].colors.as_ref().unwrap().iter().copied().collect();
        assert!(colours.contains(&[205, 140, 115, 255]) && colours.contains(&[225, 220, 205, 255]), "{colours:?}");
        let props = scene.element_props(id.0);
        let prop = |k: &str| props.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
        assert_eq!(prop("Type").as_deref(), Some("Brickwork-air cavity-PIR insulation-sand-lime brick 360"));
        assert_eq!(prop("Layers").as_deref(), Some("brickwork 100, air cavity 40, PIR insulation 100, sand-lime brick 120"));
        // the definition, with its layers, goes into IFCX and comes back
        let definition: serde_json::Value = serde_json::from_str(&prop(crate::export::ifcx::DEFINITION).unwrap()).unwrap();
        let back: Vec<bim_core::Element> = serde_json::from_value(definition).unwrap();
        assert_eq!(back[0].shape, app.modeller.model.get(id).unwrap().shape);
    }

    #[test]
    fn the_library_tab_lists_the_system_types_and_draws_one() {
        let mut app = App::default();
        click(&mut app, "Library");
        click(&mut app, "Systems");
        click(&mut app, "Masonry");
        click(&mut app, "Brickwork 100 220×62.5 stretcher bond");
        assert_eq!(app.library_view.picked, Some(Pick::System("system.masonry-waal-stretcher".into())));
        let mut ui = simulator(&app);
        assert!(ui.find("Joints: mortar 10×90").is_ok());
        assert!(ui.find("Panels: brickwork 100").is_ok());
        // its face as the kernel makes it: six bays, eight courses, each shifted course with half
        // a brick at either end
        let face = elevation(app.library.system_type("system.masonry-waal-stretcher").unwrap(), &app.library, Color::BLACK);
        let bricks = face.rects.iter().filter(|(_, c)| *c == [205, 140, 115]).count();
        assert_eq!(bricks, 4 * 6 + 4 * 7);
        // a timber frame's section from the exterior in: the breather membrane, the frame, plywood,
        // the vapour control layer, the service zone, gypsum board
        let frame = app.library.system_type("system.timber-frame-140").unwrap().clone();
        let section = system_section(&frame, &app.library, Color::BLACK);
        let thick: Vec<String> = section.bands.iter().map(|b| format!("{:.4}", b.0)).collect();
        assert_eq!(thick, ["0.0005", "0.1400", "0.0180", "0.0002", "0.0450", "0.0125"]);
        drop(ui);
        click(&mut app, "Timber frames");
        click(&mut app, "Timber frame 38×140 c/c 600");
        let mut ui = simulator(&app);
        assert!(ui.find("Bottom plates 1 · top plates 1").is_ok());
        // its face: the plates through, studs between them
        let face = elevation(&frame, &app.library, Color::BLACK);
        let timber = face.rects.iter().filter(|(_, c)| *c == [205, 165, 115]).map(|(r, _)| r[2] - r[0]).fold(0.0, f64::max);
        assert!((timber - face.size[0]).abs() < 1e-9, "a plate as long as the face");
    }

    #[test]
    fn a_selected_system_takes_a_system_type_with_its_layers() {
        use glam::DVec2;
        let mut app = App::default();
        app.new_model("Level 0");
        let s = SystemDef { start: DVec2::ZERO, end: DVec2::new(2.4, 0.0), base: 0.0, height: 2.7, spacing: DVec2::splat(1.5), member: DVec2::new(0.05, 0.15), panel: 0.02, system_type: None };
        let id = app.add_element("Curtain wall", None, Shape::System(s)).unwrap();
        app.select_modelled(id);
        assert_eq!(app.assignable_systems("system.timber-frame-140"), vec![id]);
        assert!(app.assignable_systems("system.ceiling-600x600").is_empty(), "a lying type does not fit an upright system");
        assert_eq!(app.assign_system_type("system.timber-frame-140"), 1);
        assert_eq!(app.selected_modelled(), vec![id], "it stays selected");
        // the frame's plates and studs at 600 and the service zone's battens, mineral wool in both,
        // plywood and gypsum boards of 1200, the membranes in courses of 1.5 m
        let parts = app.modeller.model.evaluate(id).unwrap();
        let count = |m: &str| parts.iter().filter(|p| p.material.as_deref() == Some(m)).count();
        assert_eq!((count("timber"), count("mineral-wool"), count("plywood"), count("gypsum-board")), (2 * 7, 2 * 4, 2, 2));
        assert_eq!((count("breather-membrane"), count("vapour-control-membrane")), (2, 2));
        let scene = super::super::modeller::viewer_model(&app.modeller.model, &[], &app.library);
        let colours: HashSet<[u8; 4]> = scene.layers[0].colors.as_ref().unwrap().iter().copied().collect();
        assert!(colours.contains(&[222, 196, 150, 255]) && colours.contains(&[240, 240, 236, 255]), "{colours:?}");
        let props = scene.element_props(id.0);
        let prop = |k: &str| props.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
        assert_eq!(prop("Type").as_deref(), Some("Timber frame 38×140 c/c 600"));
        // the definition, with its type, goes into IFCX and comes back
        let definition: serde_json::Value = serde_json::from_str(&prop(crate::export::ifcx::DEFINITION).unwrap()).unwrap();
        let back: Vec<bim_core::Element> = serde_json::from_value(definition).unwrap();
        assert_eq!(back[0].shape, app.modeller.model.get(id).unwrap().shape);
    }

    #[test]
    fn hatch_lines_stay_in_their_band() {
        let band = Rectangle::new(Point::new(10.0, 20.0), Size::new(100.0, 30.0));
        for angle in [0.0, 45.0, 90.0, 135.0] {
            let lines = hatch_lines(band, angle, 8.0);
            assert!(!lines.is_empty(), "{angle}");
            for (a, b) in lines {
                for q in [a, b] {
                    assert!(q.x >= 10.0 - 1e-3 && q.x <= 110.0 + 1e-3 && q.y >= 20.0 - 1e-3 && q.y <= 50.0 + 1e-3, "{angle}: {q:?}");
                }
            }
        }
        assert_eq!(hatch_lines(band, 0.0, 8.0).len(), 4, "horizontal lines 8 px apart in a band 30 high");
    }
}
