//! The editor of system types in the Library product: the curtain walls, frames, timber frames,
//! metal stud partitions, masonry, cladding, floors and ceilings of the project's library.
//!
//! The Project page lists the system types a group at a time; a click picks one, and the right
//! pane is its editor: its face as the kernel generates it, a section through it, and its parts
//! from the exterior (or the top) in: the face itself and the layers across it. A click on a part
//! opens its fields: its panels (a material and a thickness), its grid (bay, row and pattern) and
//! its members (on the grid's lines or framed, their material and size, and a frame's plates,
//! header and longest prefab unit). Sizes are typed in millimetres and taken at Enter.
//!
//! Layers are moved towards the exterior or the interior, past the face too, added against the
//! outermost of a side and removed. Layers that lie against one another keep doing so when a
//! thickness changes; a layer's distance from the face can also be typed, which leaves room
//! between layers until they are closed up again.
//!
//! A type's name follows its members or its panels by rule and is never typed. A change is kept
//! only when the type still checks against the library's materials; otherwise the pane says why
//! not. A type of another library is edited as a copy in the project.
//!
//! The systems of the model that are of a type follow a change of it (see `type_follow`): the
//! pane says how many there are. A system keeps the way it was laid out: one that stands does
//! not follow its type when the type is made to lie.

use super::libraries::{Category, Editor, Shelf};
use super::type_follow::{use_note, TypeRef};
use super::{App, Message, FONT, FONT_BOLD};
use crate::theme::Palette;
use bim_library::{Corner, Framing, Grid, Library, LibraryError, Members, Panels, Pattern, SystemGroup, SystemType};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, pick_list, row, scrollable, text, text_input, Column, Space};
use iced::{Center, Color, Element, Fill, Length, Task};
use openaec_ui::design::a11y::{self, Role as Aria};
use openaec_ui::Language;

/// A part of a system type across its face: the face itself, or a layer by its place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Part {
    #[default]
    Face,
    Layer(usize),
}

/// A size of a type or of one of its parts, typed in millimetres.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    /// A grid's bays, centre to centre.
    Bay,
    /// A grid's rows, centre to centre.
    Row,
    /// The panels' thickness.
    Thickness,
    /// The members' size in the face.
    Width,
    /// The members' size across the face.
    Depth,
    /// How high a frame's header over an opening is.
    Header,
    /// How long a frame's prefab units are at most; empty for a frame made in one piece.
    Unit,
    /// A layer's centre from the face, the exterior positive.
    Offset,
    /// How high above its level a lying type lies.
    Lying,
}

/// How a part's members lie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lie {
    /// A layer of boards alone, laid edge to edge.
    None,
    /// On the lines of the grid.
    Grid,
    /// As a frame: plates along its foot and head, studs between them.
    Framed,
}

#[derive(Debug, Clone)]
pub enum SystemMessage {
    /// Show the types of this group on the Project page.
    Open(SystemGroup),
    /// Edit this type of the project.
    Pick(String),
    /// A type more of the group shown: the general library's first of that group.
    Add,
    Duplicate,
    Remove,
    /// Another group, and so another IFC class; a floor or a ceiling lies, the others stand.
    Group(SystemGroup),
    Corners(Corner),
    /// An NL-SfB code of its own, one of the library's; none for the rule's.
    Coded(Option<String>),
    /// Show the fields of this part.
    Part(Part),
    /// Typed as a size.
    Typed(Part, Size, String),
    /// Enter: the typed size is taken.
    Done,
    /// The material (its key) of a part's panels.
    PanelMaterial(Part, String),
    /// The material (its key) of a part's members.
    MemberMaterial(Part, String),
    Pattern(Part, Pattern),
    Members(Part, Lie),
    /// How many plates lie at a frame's head (`true`) or its foot.
    Plates(Part, bool, u8),
    /// A layer more at the exterior (`true`) or the interior, against the outermost there.
    AddLayer(bool),
    RemoveLayer(usize),
    /// Moves a layer towards the exterior (`true`) or the interior.
    Move(usize, bool),
    /// Lays the layers against one another.
    CloseUp,
    /// Edit a type of another library as a copy in the project.
    Copy(Shelf, String),
    /// Make the elements of the picked type as it is now.
    Follow,
}

/// Which group's types show, which one is edited, which of its parts shows its fields, what is
/// being typed, and why the last change was refused.
#[derive(Debug, Clone, Default)]
pub struct SystemView {
    pub group: Option<SystemGroup>,
    pub picked: Option<String>,
    pub part: Part,
    pub draft: Option<(Part, Size, String)>,
    pub refused: Option<String>,
}

impl SystemView {
    /// The group whose types show: curtain walls until another is opened.
    pub fn shown(&self) -> SystemGroup {
        self.group.unwrap_or(SystemGroup::CurtainWall)
    }
}

/// The grid, the members and the panels of a part of a type.
fn part_of(t: &SystemType, part: Part) -> Option<(&Grid, Option<&Members>, &Panels)> {
    match part {
        Part::Face => Some((&t.grid, Some(&t.members), &t.panels)),
        Part::Layer(k) => t.layers.get(k).map(|layer| (&layer.grid, layer.members.as_ref(), &layer.panels)),
    }
}

/// The same, to change.
fn part_mut(t: &mut SystemType, part: Part) -> Option<(&mut Grid, Option<&mut Members>, &mut Panels)> {
    match part {
        Part::Face => Some((&mut t.grid, Some(&mut t.members), &mut t.panels)),
        Part::Layer(k) => t.layers.get_mut(k).map(|layer| (&mut layer.grid, layer.members.as_mut(), &mut layer.panels)),
    }
}

/// Changes system type `id` of `library`; it stays as it was when it would no longer check.
/// Layers that lay against one another still do after the change, unless it is to `keep` the
/// places they were given.
fn apply(library: &mut Library, id: &str, keep: bool, change: impl FnOnce(&mut SystemType)) -> Result<(), LibraryError> {
    let at = library.system_types.iter().position(|t| t.id == id).ok_or(LibraryError::NoKey)?;
    let mut next = library.system_types[at].clone();
    let closed = !keep && next.closed_up();
    change(&mut next);
    if closed {
        next.close_up();
    }
    next.check(&library.materials)?;
    library.system_types[at] = next;
    Ok(())
}

/// Changes system type `id` of `library`; it stays as it was when it would no longer check.
/// Layers that lay against one another still do after the change.
pub fn change(library: &mut Library, id: &str, change: impl FnOnce(&mut SystemType)) -> Result<(), LibraryError> {
    apply(library, id, false, change)
}

/// Sets a size of type `id` or of one of its parts to what was typed in millimetres. A frame's
/// longest unit may be left empty: the frame is then made in one piece. A layer's distance from
/// the face is taken as typed: the other layers stay where they are.
pub fn set(library: &mut Library, id: &str, part: Part, size: Size, typed: &str) -> Result<(), LibraryError> {
    let value = match (size, typed.trim().is_empty()) {
        (Size::Unit, true) => None,
        _ => Some(super::standards::number(typed).ok_or(LibraryError::NotPositive)? / 1000.0),
    };
    apply(library, id, size == Size::Offset, |t| {
        if size == Size::Lying {
            // (a standing type has no height to lie at)
            t.lying = t.lying.and(value);
            return;
        }
        if let (Size::Offset, Part::Layer(k), Some(offset)) = (size, part, value) {
            if let Some(layer) = t.layers.get_mut(k) {
                layer.offset = offset;
            }
            return;
        }
        let Some((grid, members, panels)) = part_mut(t, part) else { return };
        match (size, value, members) {
            (Size::Bay, Some(value), _) => grid.bay = value,
            (Size::Row, Some(value), _) => grid.row = value,
            (Size::Thickness, Some(value), _) => panels.thickness = value,
            (Size::Width, Some(value), Some(members)) => members.width = value,
            (Size::Depth, Some(value), Some(members)) => members.depth = value,
            (Size::Header, Some(value), Some(Members { framing: Some(framing), .. })) => framing.header = value,
            (Size::Unit, unit, Some(Members { framing: Some(framing), .. })) => framing.unit = unit,
            _ => {}
        }
    })
}

/// Lays the members of a part of type `id` as `how` says. A face always has members. A layer
/// that gets them takes the face's material and width, as deep as its own panels are thick; a
/// part that is framed takes the face's framing, or plates of one.
pub fn lie(library: &mut Library, id: &str, part: Part, how: Lie) -> Result<(), LibraryError> {
    change(library, id, |t| {
        let framing = t.members.framing.unwrap_or(Framing::PLATES);
        let like = Members { framing: None, ..t.members.clone() };
        match part {
            Part::Face => match how {
                Lie::None => {}
                Lie::Grid => t.members.framing = None,
                Lie::Framed => t.members.framing = Some(framing),
            },
            Part::Layer(k) => {
                let Some(layer) = t.layers.get_mut(k) else { return };
                let had = layer.members.take().unwrap_or(Members { depth: layer.panels.thickness, ..like });
                layer.members = match how {
                    Lie::None => None,
                    Lie::Grid => Some(Members { framing: None, ..had }),
                    Lie::Framed => Some(Members { framing: Some(had.framing.unwrap_or(framing)), ..had }),
                };
            }
        }
    })
}

/// Type `id` in another group. A floor or a ceiling lies, at the height it lay at or the one the
/// first type of that group in `general` lies at; the others stand.
pub fn regroup(library: &mut Library, general: &Library, id: &str, group: SystemGroup) -> Result<(), LibraryError> {
    let height = general.system_types_of(group).find_map(|t| t.lying).unwrap_or(0.0);
    change(library, id, |t| {
        t.group = group;
        t.lying = group.lies().then(|| t.lying.unwrap_or(height));
    })
}

/// An id no system type of the library has: "own.timber-frame.1".
fn fresh_id(library: &Library, group: SystemGroup) -> String {
    let group = group.name(bim_library::Lang::En).to_lowercase().replace(' ', "-");
    (1..).map(|n| format!("own.{group}.{n}")).find(|id| library.system_type(id).is_none()).unwrap_or_default()
}

/// A type more of `group`: the first type of that group in `general`, with the materials it
/// needs; its id.
pub fn add(library: &mut Library, general: &Library, group: SystemGroup) -> Result<String, LibraryError> {
    let recipe = general.system_types_of(group).next().ok_or(LibraryError::NoKey)?;
    copy(library, general, &recipe.id)
}

/// A copy of type `id`, right after it; the copy's id.
pub fn duplicate(library: &mut Library, id: &str) -> Result<String, LibraryError> {
    let at = library.system_types.iter().position(|t| t.id == id).ok_or(LibraryError::NoKey)?;
    let mut copy = library.system_types[at].clone();
    copy.id = fresh_id(library, copy.group);
    let new = copy.id.clone();
    library.system_types.insert(at + 1, copy);
    Ok(new)
}

/// A copy of type `id` of `from` in `library`, with the materials its members and panels need;
/// its id.
pub fn copy(library: &mut Library, from: &Library, id: &str) -> Result<String, LibraryError> {
    let source = from.system_type(id).ok_or(LibraryError::NoKey)?;
    let named: Vec<&str> = source.faces().flat_map(|(_, _, members, panels)| members.map(|m| m.material.as_str()).into_iter().chain(std::iter::once(panels.material.as_str()))).collect();
    for key in named {
        if library.material(key).is_none() {
            if let Some(material) = from.material(key) {
                library.add_material(material.clone())?;
            }
        }
    }
    let mut fresh = source.clone();
    fresh.id = fresh_id(library, source.group);
    let new = fresh.id.clone();
    library.add_system_type(fresh)?;
    Ok(new)
}

/// One of a list to pick from: what it stands for, and how it is named.
#[derive(Debug, Clone, PartialEq)]
struct Named<T> {
    value: T,
    label: String,
}

impl<T> std::fmt::Display for Named<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// A pattern as a list names it: masonry has a bond either way, other systems are staggered or
/// not.
fn pattern_label(pattern: Pattern, group: SystemGroup, l: Language) -> String {
    pattern.name(group, super::library::lang(l)).map_or(l.text("niet verspringend", "not staggered").to_string(), str::to_string)
}

/// A layer's distance from the face in millimetres, to the hundredth (a membrane of 0.5 mm has
/// its centre a quarter of a millimetre from its faces), the interior negative.
pub fn distance(metres: f64, lang: bim_library::Lang) -> String {
    let hundredths = (metres * 100_000.0).round() as i64;
    let text = format!("{}{}.{:02}", if hundredths < 0 { "-" } else { "" }, hundredths.abs() / 100, hundredths.abs() % 100);
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if lang.decimal_comma() {
        text.replace('.', ",")
    } else {
        text.to_string()
    }
}

/// A field of the right pane: its label, and under it what sets it.
fn stacked<'a>(p: Palette, label: impl Into<String>, control: impl Into<Element<'a, SystemMessage>>) -> Element<'a, SystemMessage> {
    column![text(label.into()).size(11).color(p.text_muted), control.into()].spacing(2).into()
}

/// The fields of a part of type `t`: its panels, its grid and its members, and for a layer its
/// distance from the face.
fn part_fields<'a>(p: Palette, l: Language, t: &SystemType, part: Part, view: &SystemView, materials: &[Named<String>]) -> Column<'a, SystemMessage> {
    use super::library::member_word;
    let lang = super::library::lang(l);
    let mut fields = Column::new().spacing(6);
    let Some((grid, members, panels)) = part_of(t, part) else { return fields };
    let unit = |unit: &'static str| text(unit).size(11).color(p.text_muted);
    // a size: what is being typed of it, or what it is
    let number = |size: Size, value: Option<f64>, name: &'static str, width: f32| -> Element<'a, SystemMessage> {
        let now = match &view.draft {
            Some((of, typed_size, typed)) if (*of, *typed_size) == (part, size) => typed.clone(),
            _ => value.map_or(String::new(), |value| if size == Size::Offset { distance(value, lang) } else { bim_library::millimetres(value, lang) }),
        };
        a11y::named(text_input("", &now).on_input(move |typed| SystemMessage::Typed(part, size, typed)).on_submit(SystemMessage::Done).size(12).padding([2, 6]).width(width), Aria::TextField, name)
    };
    let material = |now: &str, to: fn(Part, String) -> SystemMessage, name: &'static str| -> Element<'a, SystemMessage> {
        let chosen = materials.iter().find(|c| c.value == now).cloned();
        a11y::named(pick_list(materials.to_vec(), chosen, move |c: Named<String>| to(part, c.value)).text_size(12).padding([2, 6]).width(Fill), Aria::ComboBox, name)
    };
    let times = || text("×").size(11).color(p.text_muted);

    let panels_name = l.text("Panelen: materiaal en dikte", "Panels: material and thickness");
    // (a list to pick from has the width of the pane: a material's name is long)
    fields = fields.push(stacked(
        p,
        panels_name,
        column![
            material(&panels.material, SystemMessage::PanelMaterial, l.text("Materiaal van de panelen", "Material of the panels")),
            row![number(Size::Thickness, Some(panels.thickness), l.text("Dikte van de panelen in mm", "Thickness of the panels in mm"), 70.0), unit("mm")].spacing(4).align_y(Center),
        ]
        .spacing(4),
    ));

    let patterns: Vec<Named<Pattern>> = [Pattern::Stack, Pattern::RunningBond].map(|pattern| Named { value: pattern, label: pattern_label(pattern, t.group, l) }).to_vec();
    let pattern_now = patterns.iter().find(|c| c.value == grid.pattern).cloned();
    let pattern_name = l.text("Verband", "Pattern");
    let pattern = a11y::named(pick_list(patterns, pattern_now, move |c: Named<Pattern>| SystemMessage::Pattern(part, c.value)).text_size(12).padding([2, 6]).width(Fill), Aria::ComboBox, pattern_name);
    fields = fields.push(stacked(
        p,
        l.text("Raster hart op hart: breedte × hoogte", "Grid centre to centre: bay × row"),
        row![number(Size::Bay, Some(grid.bay), l.text("Breedte van het raster in mm", "Bay of the grid in mm"), 70.0), times(), number(Size::Row, Some(grid.row), l.text("Hoogte van het raster in mm", "Row of the grid in mm"), 70.0), unit("mm")].spacing(4).align_y(Center),
    ));
    fields = fields.push(stacked(p, pattern_name, pattern));

    // its members: none (a layer of boards), on the grid's lines, or framed
    let word = member_word(t.group, l);
    let mut hows = vec![
        Named { value: Lie::Grid, label: l.text("op de rasterlijnen", "on the grid's lines").to_string() },
        Named { value: Lie::Framed, label: l.text("als raamwerk: regels en stijlen", "as a frame: plates and studs").to_string() },
    ];
    if part != Part::Face {
        hows.insert(0, Named { value: Lie::None, label: l.text("geen: platen tegen elkaar", "none: boards edge to edge").to_string() });
    }
    let how_now = match members {
        None => Lie::None,
        Some(Members { framing: None, .. }) => Lie::Grid,
        Some(_) => Lie::Framed,
    };
    let how_chosen = hows.iter().find(|c| c.value == how_now).cloned();
    let how = a11y::named(pick_list(hows, how_chosen, move |c: Named<Lie>| SystemMessage::Members(part, c.value)).text_size(12).padding([2, 6]).width(Fill), Aria::ComboBox, word);
    fields = fields.push(stacked(p, word, how));
    if let Some(m) = members {
        fields = fields.push(stacked(
            p,
            l.text("Materiaal, breedte × diepte", "Material, width × depth"),
            column![
                material(&m.material, SystemMessage::MemberMaterial, l.text("Materiaal van de stijlen", "Material of the members")),
                row![
                    number(Size::Width, Some(m.width), l.text("Breedte van de stijlen in mm", "Width of the members in mm"), 70.0),
                    times(),
                    number(Size::Depth, Some(m.depth), l.text("Diepte van de stijlen in mm", "Depth of the members in mm"), 70.0),
                    unit("mm"),
                ]
                .spacing(4)
                .align_y(Center),
            ]
            .spacing(4),
        ));
        if let Some(framing) = m.framing {
            let plates = |top: bool, now: u8, name: &'static str| -> Element<'a, SystemMessage> {
                let mut counts: Vec<u8> = vec![1, 2, 3];
                if !counts.contains(&now) {
                    counts.push(now);
                    counts.sort_unstable();
                }
                a11y::named(pick_list(counts, Some(now), move |count: u8| SystemMessage::Plates(part, top, count)).text_size(12).padding([2, 6]).width(52), Aria::ComboBox, name)
            };
            let (bottom_name, top_name) = (l.text("Onderregels", "Bottom plates"), l.text("Bovenregels", "Top plates"));
            fields = fields.push(
                row![
                    stacked(p, bottom_name, plates(false, framing.bottom_plates, bottom_name)),
                    stacked(p, top_name, plates(true, framing.top_plates, top_name)),
                    stacked(p, l.text("Latei", "Header"), row![number(Size::Header, Some(framing.header), l.text("Hoogte van de latei in mm", "Height of the header in mm"), 52.0), unit("mm")].spacing(4).align_y(Center)),
                ]
                .spacing(10),
            );
            fields = fields.push(stacked(
                p,
                l.text("Prefab-elementen tot (leeg: in één stuk)", "Prefab units up to (empty: in one piece)"),
                row![number(Size::Unit, framing.unit, l.text("Grootste lengte van een prefab-element in mm", "Longest prefab unit in mm"), 70.0), unit("mm")].spacing(4).align_y(Center),
            ));
        }
    }
    if let Part::Layer(k) = part {
        let offset = t.layers.get(k).map(|layer| layer.offset);
        let name = if t.lying.is_some() { l.text("Hart van de laag tot het vlak (+ is boven)", "Centre of the layer from the face (+ is up)") } else { l.text("Hart van de laag tot het vlak (+ is buiten)", "Centre of the layer from the face (+ is exterior)") };
        fields = fields.push(stacked(p, name, row![number(Size::Offset, offset, l.text("Afstand van de laag tot het vlak in mm", "Distance of the layer from the face in mm"), 70.0), unit("mm")].spacing(4).align_y(Center)));
    }
    fields
}

impl App {
    pub(super) fn system_update(&mut self, message: SystemMessage) -> Task<Message> {
        self.follow_refused.clear();
        match self.system_apply(message) {
            Some(Ok(())) => {
                self.system_view.draft = None;
                self.system_view.refused = None;
                // the systems made of the picked type follow it
                if let Some(id) = self.system_view.picked.clone() {
                    self.follow_type(TypeRef::System(&id));
                }
                // what the model shows of its types (their names) is made again
                if self.modeller.shown.is_some() && self.modeller.model.iter().next().is_some() {
                    self.show_modelled();
                }
            }
            Some(Err(why)) => self.system_view.refused = Some(why.to_string()),
            None => {}
        }
        if self.system_view.picked.is_some() {
            self.edit_only(Editor::System);
        }
        Task::none()
    }

    /// What a message does: nothing to tell about (`None`), or a change that was taken or refused.
    fn system_apply(&mut self, message: SystemMessage) -> Option<Result<(), LibraryError>> {
        let picked = self.system_view.picked.clone();
        match message {
            SystemMessage::Open(group) => {
                self.system_view = SystemView { group: Some(group), ..SystemView::default() };
                self.set_category(Category::Systems(group));
                None
            }
            SystemMessage::Pick(id) => {
                self.system_show(id);
                None
            }
            SystemMessage::Part(part) => {
                self.system_view.part = part;
                self.system_view.draft = None;
                None
            }
            SystemMessage::Typed(part, size, typed) => {
                self.system_view.draft = Some((part, size, typed));
                None
            }
            SystemMessage::Add => {
                let general = self.shelf(Shelf::General).clone();
                let group = self.system_view.shown();
                Some(add(&mut self.library, &general, group).map(|id| self.system_view = SystemView { group: Some(group), picked: Some(id), ..SystemView::default() }))
            }
            SystemMessage::Duplicate => picked.as_deref().map(|id| duplicate(&mut self.library, id).map(|new| self.system_view.picked = Some(new))),
            SystemMessage::Remove => {
                if let Some(id) = &picked {
                    self.library.system_types.retain(|t| &t.id != id);
                    self.system_view = SystemView { group: self.system_view.group, ..SystemView::default() };
                }
                Some(Ok(()))
            }
            SystemMessage::Group(group) => picked.as_deref().map(|id| {
                let general = self.shelf(Shelf::General).clone();
                regroup(&mut self.library, &general, id, group).map(|()| self.set_category(Category::Systems(group)))
            }),
            SystemMessage::Coded(code) => picked.as_deref().map(|id| change(&mut self.library, id, |t| t.code = code)),
            SystemMessage::Corners(corners) => picked.as_deref().map(|id| change(&mut self.library, id, |t| t.corners = corners)),
            SystemMessage::Done => match (picked.as_deref(), self.system_view.draft.clone()) {
                (Some(id), Some((part, size, typed))) => Some(set(&mut self.library, id, part, size, &typed)),
                _ => None,
            },
            SystemMessage::PanelMaterial(part, key) => picked.as_deref().map(|id| {
                change(&mut self.library, id, |t| {
                    if let Some((_, _, panels)) = part_mut(t, part) {
                        panels.material = key;
                    }
                })
            }),
            SystemMessage::MemberMaterial(part, key) => picked.as_deref().map(|id| {
                change(&mut self.library, id, |t| {
                    if let Some((_, Some(members), _)) = part_mut(t, part) {
                        members.material = key;
                    }
                })
            }),
            SystemMessage::Pattern(part, pattern) => picked.as_deref().map(|id| {
                change(&mut self.library, id, |t| {
                    if let Some((grid, _, _)) = part_mut(t, part) {
                        grid.pattern = pattern;
                    }
                })
            }),
            SystemMessage::Members(part, how) => picked.as_deref().map(|id| lie(&mut self.library, id, part, how)),
            SystemMessage::Plates(part, top, count) => picked.as_deref().map(|id| {
                change(&mut self.library, id, |t| {
                    if let Some((_, Some(Members { framing: Some(framing), .. }), _)) = part_mut(t, part) {
                        *(if top { &mut framing.top_plates } else { &mut framing.bottom_plates }) = count;
                    }
                })
            }),
            SystemMessage::AddLayer(out) => picked.as_deref().map(|id| {
                let mut added = None;
                change(&mut self.library, id, |t| added = Some(t.add_layer(out))).map(|()| self.system_view.part = added.map_or(Part::Face, Part::Layer))
            }),
            SystemMessage::RemoveLayer(layer) => picked.as_deref().map(|id| {
                change(&mut self.library, id, |t| {
                    if layer < t.layers.len() {
                        t.layers.remove(layer);
                    }
                })
                .map(|()| {
                    // the layers after it are one place nearer
                    self.system_view.part = match self.system_view.part {
                        Part::Layer(k) if k == layer => Part::Face,
                        Part::Layer(k) if k > layer => Part::Layer(k - 1),
                        part => part,
                    };
                })
            }),
            SystemMessage::Move(layer, out) => picked.as_deref().map(|id| {
                change(&mut self.library, id, |t| {
                    t.shift(layer, out);
                })
            }),
            SystemMessage::CloseUp => picked.as_deref().map(|id| change(&mut self.library, id, SystemType::close_up)),
            // (the elements follow whatever was taken, in `system_update`)
            SystemMessage::Follow => Some(Ok(())),
            SystemMessage::Copy(shelf, id) => {
                let from = self.shelf(shelf).clone();
                Some(copy(&mut self.library, &from, &id).map(|new| self.system_show(new)))
            }
        }
    }

    /// A type of the project in its editor, on the project's page (from the page or a search).
    fn system_show(&mut self, id: String) {
        let group = self.library.system_type(&id).map(|t| t.group).or(self.system_view.group);
        self.system_view = SystemView { group, picked: Some(id), ..SystemView::default() };
        self.show_in_project(Category::Systems(self.system_view.shown()));
    }

    /// The right pane of the Project page while a system type is picked: its editor.
    pub fn system_pane(&self, p: Palette, l: Language) -> Element<'_, Message> {
        use super::library::{system_cut, system_face, system_group_label};
        let lang = super::library::lang(l);
        let view = &self.system_view;
        let library = &self.library;
        let Some(t) = view.picked.as_deref().and_then(|id| library.system_type(id)) else {
            let hint = l.text("Kies een systeem op de pagina om het te bewerken.", "Pick a system on the page to edit it.");
            return container(text(hint).size(12).color(p.text_muted).wrapping(Wrapping::Word)).padding(8).into();
        };
        let title = library.system_name(&t.id, lang).unwrap_or_default();
        let (class, predefined) = t.group.ifc_type();
        let lies = t.lying.is_some();
        let mm = |metres: f64| bim_library::millimetres(metres, lang);
        let materials: Vec<Named<String>> = library.materials.iter().map(|m| Named { value: m.key.clone(), label: m.name(lang).to_string() }).collect();
        let named = |key: &str| library.material(key).map_or(key.to_string(), |m| m.name(lang).to_string());
        let colour = |key: &str| library.material(key).map_or([175, 182, 195], |m| m.colour);

        // what the type is: its group (and so its IFC class), its corners, and where it lies
        let groups: Vec<Named<SystemGroup>> = SystemGroup::ALL.map(|group| Named { value: group, label: format!("{} · {}", system_group_label(group, l), group.ifc_type().0) }).to_vec();
        let group_now = groups.iter().find(|c| c.value == t.group).cloned();
        let group_name = l.text("Groep", "Group");
        let group = a11y::named(pick_list(groups, group_now, |c: Named<SystemGroup>| SystemMessage::Group(c.value)).text_size(12).padding([2, 6]).width(Fill), Aria::ComboBox, group_name);
        let corners: Vec<Named<Corner>> = vec![
            Named { value: Corner::Butt, label: l.text("koud tegen elkaar", "butted").to_string() },
            Named { value: Corner::Mitre, label: l.text("in verstek", "mitred").to_string() },
        ];
        let corners_now = corners.iter().find(|c| c.value == t.corners).cloned();
        let corners_name = l.text("Hoeken van de randprofielen", "Corners of the edge members");
        let corners = a11y::named(pick_list(corners, corners_now, |c: Named<Corner>| SystemMessage::Corners(c.value)).text_size(12).padding([2, 6]).width(Fill), Aria::ComboBox, corners_name);
        let mut fields: Column<'_, SystemMessage> = Column::new().spacing(6).push(stacked(p, group_name, group)).push(stacked(p, corners_name, corners));
        if let Some(height) = t.lying {
            let name = l.text("Ligt boven het peil", "Lies above its level");
            let now = match &view.draft {
                Some((_, Size::Lying, typed)) => typed.clone(),
                _ => mm(height),
            };
            let input = a11y::named(text_input("", &now).on_input(|typed| SystemMessage::Typed(Part::Face, Size::Lying, typed)).on_submit(SystemMessage::Done).size(12).padding([2, 6]).width(70), Aria::TextField, name);
            fields = fields.push(stacked(p, name, row![input, text("mm").size(11).color(p.text_muted)].spacing(4).align_y(Center)));
        }

        // its parts from the exterior (or the top) in: the face and the layers across it; the
        // one that is open shows its fields under it
        let small = |glyph: &'static str, name: &'static str, message: SystemMessage| -> Element<'_, SystemMessage> {
            a11y::named(button(text(glyph).size(11).color(p.text_secondary)).padding([2, 5]).style(move |_, s| p.row_button(s)).on_press(message), Aria::Button, name)
        };
        let mut parts = Column::new().spacing(2);
        for place in t.sequence() {
            let part = place.map_or(Part::Face, Part::Layer);
            let on = view.part == part;
            let Some((_, members, panels)) = part_of(t, part) else { continue };
            let with = members.filter(|_| place.is_some()).map_or(String::new(), |m| format!(" + {} {}×{}", named(&m.material), mm(m.width), mm(m.depth)));
            let depth = members.map_or(0.0, |m| m.depth).max(panels.thickness);
            let label = match place {
                None => format!("{}: {} {}", l.text("Het vlak", "The face"), named(&panels.material), mm(depth)),
                Some(_) => format!("{} {}{with}", named(&panels.material), mm(panels.thickness)),
            };
            let [r, g, b] = colour(&panels.material);
            let swatch = container(Space::new().width(10).height(14)).style(move |_| Palette::swatch(Color::from_rgb8(r, g, b)));
            let name = text(label).size(12).font(if on || place.is_none() { FONT_BOLD } else { FONT }).color(if on { p.text } else { p.text_secondary }).wrapping(Wrapping::Word).width(Fill);
            // (a long name takes a second line: what a part is made of is not cut off)
            let open = button(row![swatch, name].spacing(6).align_y(Center)).width(Fill).padding([3, 6]).style(move |_, s| if on { p.ribbon_button(true, s) } else { p.row_button(s) }).on_press(SystemMessage::Part(part));
            let line = match place {
                // (the face stays where it is: the layers move around it)
                None => row![Space::new().width(44), open, Space::new().width(22)],
                Some(k) => row![
                    small("▲", if lies { l.text("Laag omhoog", "Layer up") } else { l.text("Laag naar buiten", "Layer outwards") }, SystemMessage::Move(k, true)),
                    small("▼", if lies { l.text("Laag omlaag", "Layer down") } else { l.text("Laag naar binnen", "Layer inwards") }, SystemMessage::Move(k, false)),
                    open,
                    small("×", l.text("Laag weg", "Remove layer"), SystemMessage::RemoveLayer(k)),
                ],
            };
            parts = parts.push(line.spacing(3).align_y(Center));
            if on {
                let framed = container(part_fields(p, l, t, part, view, &materials))
                    .width(Fill)
                    .padding(8)
                    .style(move |_| container::Style { background: Some(p.surface.into()), border: iced::Border { color: p.border, width: 1.0, radius: 4.0.into() }, ..Default::default() });
                parts = parts.push(framed);
            }
        }
        let act = |label: &'static str, message: SystemMessage| button(text(label).size(12).color(p.text_secondary)).padding([3, 8]).style(move |_, s| p.row_button(s)).on_press(message);
        let more = row![
            act(if lies { l.text("+ Laag boven", "+ Layer above") } else { l.text("+ Laag buiten", "+ Layer outside") }, SystemMessage::AddLayer(true)),
            act(if lies { l.text("+ Laag onder", "+ Layer below") } else { l.text("+ Laag binnen", "+ Layer inside") }, SystemMessage::AddLayer(false)),
            Space::new().width(Fill),
        ]
        .spacing(4);
        // whether the layers lie against one another, and the way to make them
        let closed: Option<Element<'_, SystemMessage>> = (!t.layers.is_empty()).then(|| {
            if t.closed_up() {
                text(l.text("De lagen sluiten op elkaar aan.", "The layers lie against one another.")).size(11).color(p.text_muted).into()
            } else {
                let apart = text(l.text("Tussen de lagen zit ruimte, of ze overlappen.", "There is room between the layers, or they overlap.")).size(11).color(p.accent).wrapping(Wrapping::Word).width(Fill);
                row![apart, act(l.text("Aansluiten", "Close up"), SystemMessage::CloseUp)].spacing(6).align_y(Center).into()
            }
        });
        let (interior, exterior) = t.extent();
        let total = format!("{}: {} mm", l.text("Totale diepte", "Depth in all"), mm(exterior - interior));
        let refused = view.refused.as_ref().map(|why| {
            let refused = l.text("Niet overgenomen: het systeem zou dan niet meer kloppen.", "Not taken: the system would no longer hold together.");
            text(format!("{refused} ({why})")).size(11).color(p.accent).wrapping(Wrapping::Word)
        });
        let side = if lies { l.text("Doorsnede, bovenzijde boven", "Section, top side up") } else { l.text("Doorsnede, buitenzijde boven", "Section, exterior side up") };
        let editor = fields
            .push(text(if lies { l.text("Opbouw van boven naar onder", "Parts from the top down") } else { l.text("Opbouw van buiten naar binnen", "Parts from the exterior in") }).size(11).color(p.text_muted))
            .push(parts)
            .push(more)
            .push(closed)
            .push(text(total).size(11).color(p.text_secondary))
            .push(super::codes::picker(p, l, library, t.rule_code(), t.code.as_deref(), SystemMessage::Coded))
            .push(row![act(l.text("Dupliceren", "Duplicate"), SystemMessage::Duplicate), act(l.text("Verwijderen", "Remove"), SystemMessage::Remove), Space::new().width(Fill)].spacing(4))
            .push(refused)
            .push(text(l.text("De naam volgt de stijlen of de panelen.", "The name follows the members or the panels.")).size(11).color(p.text_muted).wrapping(Wrapping::Word))
            .push(use_note(p, l, &self.type_use(TypeRef::System(&t.id)), &self.follow_refused, SystemMessage::Follow));
        let pane = column![
            text(title).size(14).font(FONT_BOLD).color(p.text).wrapping(Wrapping::Word),
            text(format!("{class} · {predefined}")).size(11).color(p.text_muted),
            system_face(p, library, t),
            text(side).size(11).color(p.text_muted),
            system_cut(p, library, t),
            Element::from(editor).map(Message::SystemType),
        ]
        .spacing(8);
        // (a type with many layers is longer than the pane is high)
        scrollable(container(pane).padding(iced::Padding { top: 8.0, right: 14.0, bottom: 8.0, left: 8.0 }).width(Length::Fill)).height(Fill).into()
    }
}

#[cfg(test)]
mod tests {
    use super::super::libraries::{LibrariesMessage, Source};
    use super::*;
    use bim_library::Lang;

    fn picked(app: &App) -> SystemType {
        app.library.system_type(app.system_view.picked.as_deref().expect("a type is picked")).expect("the picked type is in the library").clone()
    }

    fn typed(app: &mut App, part: Part, size: Size, text: &str) {
        let _ = app.system_update(SystemMessage::Typed(part, size, text.into()));
        let _ = app.system_update(SystemMessage::Done);
    }

    #[test]
    fn a_system_is_set_by_its_grid_its_members_and_its_panels() {
        let mut app = App::default();
        let curtain_walls = app.library.system_types_of(SystemGroup::CurtainWall).count();
        // a curtain wall more: the general library's first, picked to be edited
        let _ = app.system_update(SystemMessage::Add);
        let id = app.system_view.picked.clone().expect("the new type is picked");
        assert_eq!((id.as_str(), app.library.system_types_of(SystemGroup::CurtainWall).count()), ("own.curtain-wall.1", curtain_walls + 1));
        let recipe = Library::starter().system_types_of(SystemGroup::CurtainWall).next().unwrap().clone();
        assert_eq!(picked(&app), SystemType { id: id.clone(), ..recipe });
        // its grid, typed in millimetres and taken at Enter; its name follows
        let _ = app.system_update(SystemMessage::Typed(Part::Face, Size::Bay, "1200".into()));
        assert_eq!(picked(&app).grid.bay, 1.5, "not before Enter");
        let _ = app.system_update(SystemMessage::Done);
        typed(&mut app, Part::Face, Size::Row, "2400");
        assert_eq!((picked(&app).grid.bay, picked(&app).grid.row, app.system_view.draft.clone()), (1.2, 2.4, None));
        assert_eq!(app.library.system_name(&id, Lang::Nl).as_deref(), Some("Vliesgevel aluminium 50×150 h.o.h. 1200"));
        // its members and its panels: a material and sizes
        let _ = app.system_update(SystemMessage::MemberMaterial(Part::Face, "timber".into()));
        typed(&mut app, Part::Face, Size::Width, "70");
        typed(&mut app, Part::Face, Size::Depth, "160");
        let _ = app.system_update(SystemMessage::PanelMaterial(Part::Face, "hpl".into()));
        typed(&mut app, Part::Face, Size::Thickness, "8,5");
        let t = picked(&app);
        assert_eq!((t.members.clone(), t.panels.clone()), (Members::new(0.07, 0.16, "timber"), Panels::new("hpl", 0.0085)));
        assert_eq!(app.library.system_name(&id, Lang::En).as_deref(), Some("Curtain wall timber 70×160 c/c 1200"));
        // its pattern and its corners
        let _ = app.system_update(SystemMessage::Pattern(Part::Face, Pattern::RunningBond));
        let _ = app.system_update(SystemMessage::Corners(Corner::Mitre));
        assert_eq!((picked(&app).grid.pattern, picked(&app).corners), (Pattern::RunningBond, Corner::Mitre));
        // what does not check is refused and stays typed: a bay narrower than the members, a
        // thickness of nothing, a material the library lacks, what is no number
        typed(&mut app, Part::Face, Size::Bay, "50");
        assert_eq!((picked(&app).grid.bay, app.system_view.refused.clone(), app.system_view.draft.clone()), (1.2, Some(LibraryError::BadGrid.to_string()), Some((Part::Face, Size::Bay, "50".to_string()))));
        typed(&mut app, Part::Face, Size::Thickness, "0");
        typed(&mut app, Part::Face, Size::Depth, "diep");
        let _ = app.system_update(SystemMessage::PanelMaterial(Part::Face, "unobtainium".into()));
        assert_eq!((picked(&app).panels, picked(&app).members.depth), (Panels::new("hpl", 0.0085), 0.16));
        assert!(app.system_view.refused.is_some());
        // a change that is taken takes the remark away
        typed(&mut app, Part::Face, Size::Bay, "900");
        assert_eq!((picked(&app).grid.bay, app.system_view.refused.clone()), (0.9, None));
        // a copy to change, and the type gone again
        let _ = app.system_update(SystemMessage::Duplicate);
        let copy = app.system_view.picked.clone().unwrap();
        assert_eq!((copy.as_str(), picked(&app)), ("own.curtain-wall.2", SystemType { id: copy.clone(), ..app.library.system_type(&id).unwrap().clone() }));
        let _ = app.system_update(SystemMessage::Remove);
        assert!(app.library.system_type(&copy).is_none() && app.system_view.picked.is_none() && app.library.system_type(&id).is_some());
        assert_eq!(app.system_view.shown(), SystemGroup::CurtainWall);
    }

    #[test]
    fn a_distance_from_the_face_is_shown_to_the_hundredth_with_its_sign() {
        assert_eq!((distance(0.07025, Lang::En), distance(-0.0881, Lang::Nl), distance(-0.4, Lang::En)), ("70.25".to_string(), "-88.1".to_string(), "-400".to_string()));
        // within a millimetre of the face it keeps its sign; German and French write a comma
        assert_eq!((distance(-0.0004, Lang::En), distance(0.07025, Lang::De), distance(0.0, Lang::Fr)), ("-0.4".to_string(), "70,25".to_string(), "0".to_string()));
        // what is shown reads back as what it was
        assert_eq!(crate::app::standards::number(&distance(-0.08825, Lang::De)), Some(-88.25));
    }

    #[test]
    fn a_frame_is_set_by_its_plates_its_header_and_its_units() {
        let mut app = App::default();
        let _ = app.system_update(SystemMessage::Pick("system.timber-frame-140".into()));
        assert_eq!((app.system_view.shown(), picked(&app).members.framing), (SystemGroup::TimberFrame, Some(Framing::PLATES.with_unit(3.3))));
        // a double top plate, a header of 240, units of 6 m; left empty, a frame in one piece
        let _ = app.system_update(SystemMessage::Plates(Part::Face, true, 2));
        typed(&mut app, Part::Face, Size::Header, "240");
        typed(&mut app, Part::Face, Size::Unit, "6000");
        assert_eq!(picked(&app).members.framing, Some(Framing { bottom_plates: 1, top_plates: 2, header: 0.24, unit: Some(6.0), ..Framing::PLATES }));
        typed(&mut app, Part::Face, Size::Unit, " ");
        assert_eq!(picked(&app).members.framing.unwrap().unit, None);
        // members on the grid's lines have no framing; framed again, plates of one
        let _ = app.system_update(SystemMessage::Members(Part::Face, Lie::Grid));
        assert_eq!(picked(&app).members.framing, None);
        typed(&mut app, Part::Face, Size::Header, "300");
        assert_eq!(picked(&app).members.framing, None, "no header without a frame");
        let _ = app.system_update(SystemMessage::Members(Part::Face, Lie::Framed));
        assert_eq!(picked(&app).members.framing, Some(Framing::PLATES));
        // a face always has members
        let _ = app.system_update(SystemMessage::Members(Part::Face, Lie::None));
        assert_eq!(picked(&app).members.framing, Some(Framing::PLATES));
        // a layer of boards gets battens: the face's timber and width, as deep as its boards;
        // framed, they take the face's framing
        let plywood = Part::Layer(1);
        let _ = app.system_update(SystemMessage::Members(plywood, Lie::Grid));
        assert_eq!(picked(&app).layers[1].members, Some(Members::new(0.038, 0.018, "timber")));
        let _ = app.system_update(SystemMessage::Members(plywood, Lie::Framed));
        assert_eq!(picked(&app).layers[1].members, Some(Members::new(0.038, 0.018, "timber").framed(Framing::PLATES)));
        let _ = app.system_update(SystemMessage::Plates(plywood, false, 2));
        assert_eq!(picked(&app).layers[1].members.as_ref().and_then(|m| m.framing).map(|f| (f.bottom_plates, f.top_plates)), Some((2, 1)));
        let _ = app.system_update(SystemMessage::Members(plywood, Lie::None));
        assert_eq!(picked(&app).layers[1].members, None);
    }

    #[test]
    fn the_layers_of_a_system_are_changed_moved_added_and_removed() {
        let mut app = App::default();
        let _ = app.system_update(SystemMessage::Pick("system.timber-frame-140".into()));
        let across = |app: &App| -> Vec<String> {
            let t = picked(app);
            t.sequence().into_iter().map(|part| part.map_or("frame".to_string(), |k| t.layers[k].panels.material.clone())).collect()
        };
        assert_eq!(across(&app), ["breather-membrane", "frame", "plywood", "vapour-control-membrane", "mineral-wool", "gypsum-board"]);
        let before = picked(&app);
        // deeper studs: the layers stay against the frame and against one another
        typed(&mut app, Part::Face, Size::Depth, "184");
        typed(&mut app, Part::Face, Size::Thickness, "184");
        let t = picked(&app);
        assert!(t.closed_up());
        assert!((t.layers[0].offset - (0.092 + 0.00025)).abs() < 1e-12 && (t.layers[1].offset + (0.092 + 0.009)).abs() < 1e-12);
        // (named as the library's own 184 frame is: the later of the two is told apart)
        assert_eq!(app.library.system_name(&t.id, Lang::Nl).as_deref(), Some("Houtskeletbouw 38×184 h.o.h. 600"));
        assert_eq!(app.library.system_name("system.timber-frame-184", Lang::Nl).as_deref(), Some("Houtskeletbouw 38×184 h.o.h. 600 (2)"));
        // thicker plywood: what lies inside it moves in
        typed(&mut app, Part::Layer(1), Size::Thickness, "22");
        let t = picked(&app);
        assert!(t.closed_up() && (t.layers[4].offset - (before.layers[4].offset - 0.022 - 0.004)).abs() < 1e-12, "{}", t.layers[4].offset);
        // a layer's material and its boards' grid
        let _ = app.system_update(SystemMessage::PanelMaterial(Part::Layer(4), "plywood".into()));
        typed(&mut app, Part::Layer(4), Size::Bay, "1220");
        assert_eq!((picked(&app).layers[4].panels.material.as_str(), picked(&app).layers[4].grid.bay), ("plywood", 1.22));
        // moved: the plywood out past the frame, under the membrane
        let _ = app.system_update(SystemMessage::Move(1, true));
        assert_eq!(across(&app), ["breather-membrane", "plywood", "frame", "vapour-control-membrane", "mineral-wool", "plywood"]);
        assert!(picked(&app).closed_up());
        // a second board inside: like the innermost, against it, and open to be edited
        let _ = app.system_update(SystemMessage::AddLayer(false));
        let t = picked(&app);
        assert_eq!((t.layers.len(), app.system_view.part, t.layers[5].panels.clone()), (6, Part::Layer(5), t.layers[4].panels.clone()));
        assert!(t.closed_up() && t.sequence().last() == Some(&Some(5)));
        // a layer's distance from the face is taken as typed and leaves room; closing up lays
        // the layers against one another again
        typed(&mut app, Part::Layer(5), Size::Offset, "-400");
        assert_eq!((picked(&app).layers[5].offset, picked(&app).closed_up(), picked(&app).layers[4].offset), (-0.4, false, t.layers[4].offset));
        let _ = app.system_update(SystemMessage::CloseUp);
        assert_eq!(picked(&app).layers, t.layers);
        assert!(picked(&app).closed_up());
        // a layer away: the part that was open follows its layer, or is the face
        let _ = app.system_update(SystemMessage::RemoveLayer(2));
        assert_eq!((picked(&app).layers.len(), app.system_view.part), (5, Part::Layer(4)));
        let _ = app.system_update(SystemMessage::RemoveLayer(4));
        assert_eq!((picked(&app).layers.len(), app.system_view.part), (4, Part::Face));
        assert!(picked(&app).closed_up() && picked(&app).check(&app.library.materials).is_ok());
    }

    #[test]
    fn a_floor_or_a_ceiling_lies_and_the_other_groups_stand() {
        let mut app = App::default();
        let _ = app.system_update(SystemMessage::Pick("system.cladding-hpl-1200x3000".into()));
        assert_eq!(picked(&app).lying, None);
        // a ceiling of it: it lies where the general library's ceiling does, and is a covering
        let _ = app.system_update(SystemMessage::Group(SystemGroup::Ceiling));
        assert_eq!((picked(&app).group, picked(&app).lying, picked(&app).group.ifc_type(), app.system_view.shown()), (SystemGroup::Ceiling, Some(2.6), ("IfcCoveringType", "CEILING"), SystemGroup::Ceiling));
        typed(&mut app, Part::Face, Size::Lying, "2400");
        assert_eq!(picked(&app).lying, Some(2.4));
        // a floor keeps the height it lay at; cladding again stands
        let _ = app.system_update(SystemMessage::Group(SystemGroup::Floor));
        assert_eq!(picked(&app).lying, Some(2.4));
        let _ = app.system_update(SystemMessage::Group(SystemGroup::Cladding));
        assert_eq!((picked(&app).group, picked(&app).lying), (SystemGroup::Cladding, None));
        typed(&mut app, Part::Face, Size::Lying, "2400");
        assert_eq!(picked(&app).lying, None, "a standing type lies nowhere");
    }

    #[test]
    fn a_system_of_another_library_is_edited_as_a_copy_in_the_project() {
        let mut app = App::default();
        // a library with a type and a material the project lacks
        let mut from = Library::default();
        from.add_material(bim_library::Material::new("reed", bim_library::Names::same("riet"), bim_library::HatchKind::Generic, [190, 170, 110])).unwrap();
        from.add_material(app.library.material("timber").unwrap().clone()).unwrap();
        let thatch = SystemType { id: "system.thatch".into(), panels: Panels::new("reed", 0.3), ..Library::starter().system_type("system.cladding-timber-boards-140").unwrap().clone() };
        from.add_system_type(thatch.clone()).unwrap();
        assert_eq!(copy(&mut app.library, &from, "system.thatch").as_deref(), Ok("own.cladding.1"));
        assert_eq!(app.library.system_type("own.cladding.1"), Some(&SystemType { id: "own.cladding.1".into(), ..thatch }));
        assert!(app.library.material("reed").is_some());
        // from a search: the copy is picked on the project's page, its group shown
        let _ = app.libraries_update(LibrariesMessage::Search("kanaalplaat".into()));
        let _ = app.system_update(SystemMessage::Copy(Shelf::General, "system.hollow-core-200".into()));
        assert_eq!((app.system_view.picked.as_deref(), app.system_view.shown()), (Some("own.floor.1"), SystemGroup::Floor));
        assert_eq!((app.libraries.source, app.libraries.query.as_str()), (Source::Project, ""));
        // the right pane edits what was picked last
        let _ = app.material_update(super::super::materials::MaterialMessage::Pick("steel".into()));
        assert_eq!((app.material_view.picked.as_deref(), app.system_view.picked.clone()), (Some("steel"), None));
        let _ = app.system_update(SystemMessage::Pick("own.floor.1".into()));
        assert_eq!((app.material_view.picked.clone(), app.buildup_view.picked.clone(), app.profile_view.picked.clone()), (None, None, None));
    }

    #[test]
    fn the_project_page_lists_the_systems_and_the_pane_edits_the_picked_one() {
        let mut app = App::default();
        let l = Language::English;
        let _ = app.libraries_update(LibrariesMessage::Source(Source::Project));
        let _ = app.system_update(SystemMessage::Open(SystemGroup::TimberFrame));
        {
            let mut page = crate::app::test_ui_of(iced::Size::new(1200.0, 1400.0), app.libraries_page(app.theme.palette(), l));
            assert!(page.find("Timber frames").is_ok() && page.find("+ System").is_ok() && page.find("Timber frame 38×140 c/c 600").is_ok() && page.find("216.2 mm · IfcWallType").is_ok());
        }
        {
            let mut pane = crate::app::test_ui_of(iced::Size::new(340.0, 700.0), app.system_pane(app.theme.palette(), l));
            assert!(pane.find("Pick a system on the page to edit it.").is_ok());
        }
        let _ = app.system_update(SystemMessage::Pick("system.timber-frame-140".into()));
        {
            let mut pane = crate::app::test_ui_of(iced::Size::new(340.0, 1400.0), app.system_pane(app.theme.palette(), l));
            assert!(pane.find("IfcWallType · STANDARD").is_ok() && pane.find("Section, exterior side up").is_ok() && pane.find("Parts from the exterior in").is_ok());
            // its parts, the face open: its frame's fields
            assert!(pane.find("The face: mineral wool 140").is_ok() && pane.find("plywood 18").is_ok() && pane.find("mineral wool 45 + timber 45×45").is_ok());
            assert!(pane.find("Bottom plates").is_ok() && pane.find("Prefab units up to (empty: in one piece)").is_ok());
            assert!(pane.find("The layers lie against one another.").is_ok() && pane.find("+ Layer inside").is_ok() && pane.find("Duplicate").is_ok());
            assert!(pane.find("Centre of the layer from the face (+ is exterior)").is_err());
        }
        // a layer opened shows its own fields; one pulled away is told of
        let _ = app.system_update(SystemMessage::Part(Part::Layer(1)));
        typed(&mut app, Part::Layer(1), Size::Offset, "-120");
        {
            let mut pane = crate::app::test_ui_of(iced::Size::new(340.0, 1400.0), app.system_pane(app.theme.palette(), l));
            assert!(pane.find("Centre of the layer from the face (+ is exterior)").is_ok() && pane.find("Bottom plates").is_err());
            assert!(pane.find("There is room between the layers, or they overlap.").is_ok() && pane.find("Close up").is_ok());
        }
        // a click in the pane or on the page does what it says
        let click = |app: &mut App, in_pane: bool, label: &str| {
            let messages: Vec<Message> = {
                let palette = app.theme.palette();
                let part = if in_pane { app.system_pane(palette, l) } else { app.libraries_page(palette, l) };
                let mut ui = crate::app::test_ui_of(iced::Size::new(if in_pane { 340.0 } else { 1200.0 }, 1400.0), part);
                ui.click(label).unwrap_or_else(|_| panic!("no {label}"));
                ui.into_messages().collect()
            };
            for message in messages {
                let _ = app.update(message);
            }
        };
        click(&mut app, true, "Close up");
        assert!(picked(&app).closed_up());
        click(&mut app, true, "+ Layer inside");
        assert_eq!((picked(&app).layers.len(), app.system_view.part), (6, Part::Layer(5)));
        click(&mut app, true, "The face: mineral wool 140");
        assert_eq!(app.system_view.part, Part::Face);
        click(&mut app, false, "Timber frame 38×184 c/c 600");
        assert_eq!(app.system_view.picked.as_deref(), Some("system.timber-frame-184"));
        // another group shows its own tiles, with nothing picked
        let _ = app.update(Message::Libraries(LibrariesMessage::Category(Category::Systems(SystemGroup::Masonry))));
        assert_eq!((app.system_view.shown(), app.system_view.picked.clone()), (SystemGroup::Masonry, None));
        click(&mut app, false, "Brickwork 100 220×62.5 stretcher bond");
        assert_eq!(app.system_view.picked.as_deref(), Some("system.masonry-waal-stretcher"));
    }
}
