//! The placement strip: while a modelling tool is on, a strip over the view holds the type and the
//! sizes its next element gets, as Revit's Options Bar and Type Selector, Archicad's Info Box and
//! Bonsai's header do (`docs/superpowers/specs/2026-11-10-modeller-ribbon-design.md`).
//!
//! - **Type**: a wall's or slab's build-up and a curtain wall's system type from the project's
//!   library, a column's or beam's steel profile from the catalogue; or none, sized in the strip.
//! - **Sizes**: a wall's thickness, height and base; a slab's thickness and top; a member's
//!   profile size and height; an object's size; a door's or window's size and sill. An empty
//!   height runs up to the next level.
//! - **Location line**: whether a wall stands on the line drawn with its axis, its exterior face
//!   or its interior face (the exterior is on the wall's right). Walls drawn one after the other
//!   on a face still meet at their corners: the last one's end and the next one's start go to
//!   where their axes cross.
//! - **Chain**: whether the next wall, beam or curtain wall starts where the last one ended.
//!
//! A value is used as soon as it is valid; until then the field keeps what was typed. The preview
//! follows the strip, so what shows is what a click places.

use super::modeller::ModelTool;
use super::{App, Message};
use crate::document::parse_elevation;
use crate::theme::Palette;
use bim_core::element::PlanarForm;
use bim_core::{ElementId, PlanarDef, Shape};
use glam::{DVec2, DVec3};
use iced::widget::{button, container, pick_list, text, text_input, Row};
use iced::{Center, Element, Fill};
use openaec_ui::Language;
use std::collections::HashMap;

/// Where a wall stands on the line drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LocationLine {
    /// Its axis on the line.
    #[default]
    Centre,
    /// Its exterior face (on its right) on the line.
    Exterior,
    /// Its interior face (on its left) on the line.
    Interior,
}

impl LocationLine {
    pub const ALL: [LocationLine; 3] = [LocationLine::Centre, LocationLine::Exterior, LocationLine::Interior];

    fn label(self, l: Language) -> &'static str {
        match self {
            LocationLine::Centre => l.text("Hartlijn", "Centre line"),
            LocationLine::Exterior => l.text("Buitenzijde", "Exterior face"),
            LocationLine::Interior => l.text("Binnenzijde", "Interior face"),
        }
    }

    /// How far the axis lies to the left of the line drawn, for a wall `thickness` thick.
    fn shift(self, thickness: f64) -> f64 {
        match self {
            LocationLine::Centre => 0.0,
            LocationLine::Exterior => thickness / 2.0,
            LocationLine::Interior => -thickness / 2.0,
        }
    }
}

/// What a tool's next element gets.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// Its type by id: a build-up (wall, slab), a steel profile's designation (column, beam) or a
    /// system type (curtain wall); `None`: sized by the values below.
    pub kind: Option<String>,
    /// A wall's or slab's thickness; a column's, beam's, object's, door's or window's width.
    pub width: f64,
    /// A column's or object's depth, a beam's profile height.
    pub depth: f64,
    /// A wall's, column's, curtain wall's, object's, door's or window's height; a beam's top above
    /// its level. `None`: up to the next level.
    pub height: Option<f64>,
    /// How far above its level a wall, column, curtain wall or object stands and a slab's top
    /// lies.
    pub offset: f64,
    /// A window's sill.
    pub sill: f64,
    pub location: LocationLine,
    /// What the Slab tool makes: a floor, a roof or a ceiling.
    pub role: bim_core::SlabRole,
}

impl Settings {
    /// What a tool places when nothing was typed.
    pub fn default_for(tool: ModelTool) -> Self {
        let plain = |width: f64, depth: f64, height: Option<f64>| Settings { kind: None, width, depth, height, offset: 0.0, sill: 0.0, location: LocationLine::Centre, role: bim_core::SlabRole::Floor };
        use super::element_props::{DOOR, WINDOW};
        use super::modeller::{BEAM, COLUMN, OBJECT, SLAB_THICKNESS, WALL_THICKNESS};
        match tool {
            ModelTool::Wall => plain(WALL_THICKNESS, 0.0, None),
            ModelTool::Slab => plain(SLAB_THICKNESS, 0.0, None),
            ModelTool::Column => plain(COLUMN, COLUMN, None),
            ModelTool::Beam => plain(BEAM.0, BEAM.1, None),
            ModelTool::System => plain(0.0, 0.0, None),
            ModelTool::Point => plain(OBJECT.x, OBJECT.y, Some(OBJECT.z)),
            ModelTool::Door => plain(DOOR.0, 0.0, Some(DOOR.1)),
            ModelTool::Window => Settings { sill: WINDOW.2, ..plain(WINDOW.0, 0.0, Some(WINDOW.1)) },
        }
    }
}

/// A value of the strip typed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlaceField {
    Width,
    Depth,
    Height,
    Offset,
    Sill,
}

/// What the strip sends.
#[derive(Debug, Clone, PartialEq)]
pub enum PlaceMessage {
    /// The type picked (`None`: generic).
    Kind(Option<String>),
    Field(PlaceField, String),
    /// Enter: the fields show the values in use.
    Done,
    Location(LocationLine),
    /// What the Slab tool makes; its type goes, as types are per role.
    Role(bim_core::SlabRole),
    Chain(bool),
    /// The Wall tool draws a rectangle, or single walls.
    Rectangle(bool),
}

/// The settings of every tool, the chain, and a value being typed.
#[derive(Debug, Clone)]
pub struct Placement {
    settings: HashMap<ModelTool, Settings>,
    pub chain: bool,
    /// The Wall tool draws a rectangle of four walls with two clicks.
    pub rectangle: bool,
    typing: Option<(PlaceField, String)>,
    /// The wall placed last while chaining: the next one on a face meets it where their axes cross.
    pub last_wall: Option<ElementId>,
}

impl Default for Placement {
    fn default() -> Self {
        Self { settings: HashMap::new(), chain: true, rectangle: false, typing: None, last_wall: None }
    }
}

impl Placement {
    /// What `tool`'s next element gets.
    pub fn of(&self, tool: ModelTool) -> Settings {
        self.settings.get(&tool).cloned().unwrap_or_else(|| Settings::default_for(tool))
    }

    fn set(&mut self, tool: ModelTool, settings: Settings) {
        self.settings.insert(tool, settings);
    }
}

impl App {
    /// Create similar: the tool that makes the one element selected, with its type and sizes, so
    /// the next one drawn is like it.
    pub(in crate::app) fn create_similar(&mut self, l: Language) {
        let Some(e) = self.defined().cloned() else { return };
        let of = |tool: ModelTool| self.modeller.placement.of(tool);
        let (tool, settings) = match e.shape_in_project(&self.modeller.model.grid) {
            Shape::Planar(p) => {
                let kind = p.build_up.as_ref().map(|b| b.id.clone());
                match p.form {
                    // a wall up to its level stays so; one with a height of its own keeps it
                    PlanarForm::Wall { height, .. } => (ModelTool::Wall, Settings { kind, width: p.total_thickness(), height: e.top.is_none().then_some(height), ..of(ModelTool::Wall) }),
                    PlanarForm::Slab { role, .. } => (ModelTool::Slab, Settings { kind, width: p.total_thickness(), role, ..of(ModelTool::Slab) }),
                }
            }
            Shape::Linear(d) => {
                let tool = if e.class() == "IfcColumn" { ModelTool::Column } else { ModelTool::Beam };
                let mut s = of(tool);
                match &d.profile {
                    bim_core::ProfileDef::Rectangle { width, height } => (s.kind, s.width, s.depth) = (None, *width, *height),
                    // a steel profile by its designation, a type of the library by its name
                    bim_core::ProfileDef::Catalogue { designation, .. } => {
                        let typed = self.library.linear_types.iter().find(|t| self.library.linear_name(&t.id, bim_library::Lang::En).as_deref() == Some(designation.as_str())).map(|t| t.id.clone());
                        s.kind = typed.or_else(|| bim_core::ProfileDef::steel(designation).map(|_| designation.clone())).or(s.kind);
                    }
                    _ => {}
                }
                (tool, s)
            }
            Shape::System(d) => (ModelTool::System, Settings { kind: d.system_type.as_ref().map(|t| t.id.clone()), height: e.top.is_none().then_some(d.height), ..of(ModelTool::System) }),
            Shape::Point(d) => {
                let tool = match d.opening {
                    Some(bim_core::OpeningKind::Door) => ModelTool::Door,
                    Some(bim_core::OpeningKind::Window) => ModelTool::Window,
                    None => ModelTool::Point,
                };
                let mut s = Settings { width: d.size.x, height: Some(d.size.z), ..of(tool) };
                if tool == ModelTool::Point {
                    s.depth = d.size.y;
                }
                (tool, s)
            }
            Shape::Connection(_) => return,
        };
        self.modeller.placement.set(tool, settings);
        if self.modeller.tool.map(|t| t.0) != Some(tool) {
            self.set_model_tool(Some((tool, tool.label(l))));
        }
    }
}

/// A choice of type in the strip: its id (`None`: generic) and its name.
#[derive(Debug, Clone, PartialEq)]
struct Choice {
    id: Option<String>,
    name: String,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

/// A location line in the strip, named in the interface language.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Located(LocationLine, &'static str);

impl std::fmt::Display for Located {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.1)
    }
}


/// The fields `tool` shows, with their labels.
fn fields(tool: ModelTool, l: Language) -> Vec<(PlaceField, &'static str)> {
    use PlaceField as F;
    let thickness = (F::Width, l.text("Dikte", "Thickness"));
    let (width, depth, height) = ((F::Width, l.text("Breedte", "Width")), (F::Depth, l.text("Diepte", "Depth")), (F::Height, l.text("Hoogte", "Height")));
    let base = (F::Offset, l.text("Basis", "Base"));
    match tool {
        ModelTool::Wall => vec![thickness, height, base],
        ModelTool::Slab => vec![thickness, (F::Offset, l.text("Bovenkant", "Top height"))],
        ModelTool::Column => vec![width, depth, height, base],
        ModelTool::Beam => vec![width, (F::Depth, l.text("Profielhoogte", "Profile height")), (F::Height, l.text("Bovenkant", "Top height"))],
        ModelTool::System => vec![height, base],
        ModelTool::Point => vec![width, depth, height, base],
        ModelTool::Door => vec![width, height],
        ModelTool::Window => vec![width, height, (F::Sill, l.text("Borstwering", "Sill"))],
    }
}

/// A field's value in `s`, as the strip shows it.
fn shown(s: &Settings, field: PlaceField) -> String {
    let m = |v: f64| format!("{v:.3}");
    match field {
        PlaceField::Width => m(s.width),
        PlaceField::Depth => m(s.depth),
        PlaceField::Height => s.height.map(m).unwrap_or_default(),
        PlaceField::Offset => m(s.offset),
        PlaceField::Sill => m(s.sill),
    }
}

/// `s` with `field` typed as `typed`, if that is a value it can take: sizes above 0, a sill not
/// below 0, a height left empty (up to the next level) where the tool allows it.
fn with_typed(tool: ModelTool, s: &Settings, field: PlaceField, typed: &str) -> Option<Settings> {
    let mut s = s.clone();
    let typed = typed.trim();
    if field == PlaceField::Height && typed.is_empty() {
        let storey = matches!(tool, ModelTool::Wall | ModelTool::Column | ModelTool::Beam | ModelTool::System);
        s.height = None;
        return storey.then_some(s);
    }
    let v = parse_elevation(typed)?;
    match field {
        PlaceField::Width if v > 0.0 => s.width = v,
        PlaceField::Depth if v > 0.0 => s.depth = v,
        PlaceField::Height if v > 0.0 => s.height = Some(v),
        PlaceField::Offset => s.offset = v,
        PlaceField::Sill if v >= 0.0 => s.sill = v,
        _ => return None,
    }
    Some(s)
}

impl App {
    /// What the tool on places next.
    pub(crate) fn tool_settings(&self) -> Option<(ModelTool, Settings)> {
        let (tool, _) = self.modeller.tool?;
        Some((tool, self.modeller.placement.of(tool)))
    }

    /// Something the strip sends.
    pub(super) fn place_message(&mut self, message: PlaceMessage) {
        let Some((tool, mut s)) = self.tool_settings() else { return };
        let placement = &mut self.modeller.placement;
        match message {
            PlaceMessage::Kind(kind) => s.kind = kind,
            PlaceMessage::Location(location) => s.location = location,
            PlaceMessage::Role(role) => {
                if s.role != role {
                    s.role = role;
                    s.kind = None;
                }
            }
            PlaceMessage::Rectangle(on) => {
                placement.rectangle = on;
                self.modeller.points.clear();
                placement.last_wall = None;
            }
            PlaceMessage::Chain(on) => {
                placement.chain = on;
                if !on {
                    // the next element starts afresh
                    self.modeller.points.clear();
                    placement.last_wall = None;
                }
                return;
            }
            PlaceMessage::Done => {
                placement.typing = None;
                return;
            }
            PlaceMessage::Field(field, typed) => {
                if let Some(typed_in) = with_typed(tool, &s, field, &typed) {
                    s = typed_in;
                }
                placement.typing = Some((field, typed));
            }
        }
        placement.set(tool, s);
    }

    /// The name of what `tool`, named `name`, places next. A system drawn with a type is named
    /// after the type's group (a timber frame, masonry, cladding), in the language the tool is
    /// named in; anything else after its tool.
    pub(super) fn placed_name(&self, tool: ModelTool, name: &'static str) -> &'static str {
        let kind = self.modeller.placement.of(tool).kind;
        let group = kind.as_deref().and_then(|id| self.library.system_type(id)).map(|t| t.group);
        let language = Language::ALL.into_iter().find(|&l| tool.label(l) == name);
        match (tool, group, language) {
            (ModelTool::System, Some(group), Some(l)) => group.name(super::library::lang(l)),
            _ => name,
        }
    }

    /// The axis of the wall the Wall tool places on the line from `a` to `b` (scene coordinates):
    /// moved to its location line, and, chained to a wall placed on a face before, starting
    /// where the two axes cross.
    pub(super) fn wall_axis(&self, a: DVec3, b: DVec3, s: &Settings) -> (DVec3, DVec3) {
        let thickness = s.kind.as_deref().and_then(|id| bim_core::element::BuildUpDef::from_library(&self.library, id)).map_or(s.width, |b| b.thickness());
        let shift = s.location.shift(thickness);
        let d = (b - a).truncate();
        if shift == 0.0 || d.length() <= 0.0 {
            return (a, b);
        }
        let left = d.normalize().perp().extend(0.0) * shift;
        let (mut a, b) = (a + left, b + left);
        if let Some((p, q)) = self.chained_wall() {
            if let Some(x) = super::grips::crossing(p, q, a.truncate(), b.truncate()) {
                a = x.extend(a.z);
            }
        }
        (a, b)
    }

    /// The axis (in plan) of the wall placed last while chaining, if there is one.
    fn chained_wall(&self) -> Option<(DVec2, DVec2)> {
        let id = self.modeller.placement.last_wall.filter(|_| !self.modeller.points.is_empty())?;
        match self.modeller.model.get(id)?.shape_in_project(&self.modeller.model.grid) {
            Shape::Planar(PlanarDef { form: PlanarForm::Wall { start, end, .. }, .. }) => Some((start, end)),
            _ => None,
        }
    }

    /// The strip over the view while a tool is on: the tool's name, its type, its sizes, then
    /// what goes with the tool (location line, chain).
    pub(super) fn placement_bar(&self, p: Palette, l: Language) -> Option<Element<'_, Message>> {
        let (tool, s) = self.tool_settings()?;
        let label = |t: &'static str| text(t).size(12).color(p.text_secondary).wrapping(iced::widget::text::Wrapping::None);
        // each value with its name, one under the other: the right pane is narrow
        fn pair<'a>(name: impl Into<Element<'a, Message>>, value: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
            Row::new().spacing(8).align_y(Center).push(container(name).width(iced::Length::FillPortion(2))).push(container(value).width(iced::Length::FillPortion(3))).into()
        }
        let mut bar = iced::widget::Column::new().spacing(5);
        if let Some(choices) = self.kinds(tool, l) {
            let picked = choices.iter().find(|c| c.id == s.kind).cloned();
            let pick = pick_list(choices, picked, |c: Choice| Message::Place(PlaceMessage::Kind(c.id))).text_size(12).padding([2, 6]);
            // a system type's name ends with what tells it from the next (its centres, its
            // bond), so its list is as wide as its longest name
            let pick = if tool == ModelTool::System { pick } else { pick.width(150) };
            bar = bar.push(pair(label(l.text("Type", "Type")), pick));
        }
        let typing = self.modeller.placement.typing.clone();
        for (field, name) in fields(tool, l) {
            // a typed element's thickness is its type's
            let fixed = field == PlaceField::Width && s.kind.is_some() && matches!(tool, ModelTool::Wall | ModelTool::Slab | ModelTool::Column | ModelTool::Beam);
            let fixed = fixed || (field == PlaceField::Depth && s.kind.is_some() && matches!(tool, ModelTool::Column | ModelTool::Beam));
            let value = match &typing {
                Some((f, typed)) if *f == field => typed.clone(),
                _ => shown(&s, field),
            };
            let hint = if field == PlaceField::Height { l.text("tot niveau", "to level") } else { "" };
            let input = text_input(hint, &value).size(12).padding([2, 6]).width(68);
            let input = if fixed { input } else { input.on_input(move |t| Message::Place(PlaceMessage::Field(field, t))).on_submit(Message::Place(PlaceMessage::Done)) };
            bar = bar.push(pair(label(name), input.width(Fill)));
        }
        if tool == ModelTool::Slab {
            // a floor, a roof or a ceiling: made the same, classed and listed apart
            for (role, name) in [(bim_core::SlabRole::Floor, l.text("Vloer", "Floor")), (bim_core::SlabRole::Roof, l.text("Dak", "Roof")), (bim_core::SlabRole::Ceiling, l.text("Plafond", "Ceiling"))] {
                let on = s.role == role;
                bar = bar.push(button(text(name).size(12)).padding([3, 10]).style(move |_, st| p.ribbon_button(on, st)).on_press(Message::Place(PlaceMessage::Role(role))));
            }
        }
        if tool == ModelTool::Wall {
            let options: Vec<Located> = LocationLine::ALL.iter().map(|&k| Located(k, k.label(l))).collect();
            let picked = options.iter().find(|o| o.0 == s.location).copied();
            bar = bar.push(label(l.text("Locatielijn", "Location line"))).push(pick_list(options, picked, |o: Located| Message::Place(PlaceMessage::Location(o.0))).text_size(12).padding([2, 6]).width(120));
        }
        if tool == ModelTool::Wall {
            let on = self.modeller.placement.rectangle;
            let rectangle = button(text(l.text("Rechthoek", "Rectangle")).size(12)).padding([3, 10]).style(move |_, st| p.ribbon_button(on, st)).on_press(Message::Place(PlaceMessage::Rectangle(!on)));
            bar = bar.push(rectangle);
        }
        if tool.chains() {
            let on = self.modeller.placement.chain;
            let chain = button(text(l.text("Ketting", "Chain")).size(12)).padding([3, 10]).style(move |_, st| p.ribbon_button(on, st)).on_press(Message::Place(PlaceMessage::Chain(!on)));
            bar = bar.push(chain);
        }
        Some(container(bar).padding([2, 8]).width(Fill).into())
    }

    /// The types `tool` can take, generic first; `None` for a tool without types.
    fn kinds(&self, tool: ModelTool, l: Language) -> Option<Vec<Choice>> {
        let lang = super::library::lang(l);
        let generic = Choice { id: None, name: l.text("Generiek", "Generic").to_string() };
        let mut choices = vec![generic];
        match tool {
            ModelTool::Wall | ModelTool::Slab => {
                let role = match (tool, self.modeller.placement.of(tool).role) {
                    (ModelTool::Wall, _) => bim_library::Role::Wall,
                    (_, bim_core::SlabRole::Floor) => bim_library::Role::Floor,
                    (_, bim_core::SlabRole::Roof) => bim_library::Role::Roof,
                    (_, bim_core::SlabRole::Ceiling) => bim_library::Role::Ceiling,
                };
                choices.extend(self.library.names(role, lang).into_iter().map(|(id, name)| Choice { id: Some(id.to_string()), name }));
            }
            ModelTool::Column | ModelTool::Beam => {
                // the library's own columns or beams (a catalogue's among them), then the steel table
                let role = if tool == ModelTool::Column { bim_library::LinearRole::Column } else { bim_library::LinearRole::Beam };
                let steel = |id: &str| self.library.linear_type(id).is_some_and(|t| t.designation.is_some());
                choices.extend(self.library.linear_names(role, lang).into_iter().filter(|(id, _)| !steel(id)).map(|(id, name)| Choice { id: Some(id.to_string()), name }));
                choices.extend(bim_library::profile::steel_profiles().iter().map(|p| Choice { id: Some(p.designation.clone()), name: p.designation.clone() }));
            }
            ModelTool::System => {
                for group in bim_library::SystemGroup::ALL {
                    for t in self.library.system_types_of(group) {
                        let name = self.library.system_name(&t.id, lang).unwrap_or_else(|| t.id.clone());
                        choices.push(Choice { id: Some(t.id.clone()), name });
                    }
                }
            }
            ModelTool::Point | ModelTool::Door | ModelTool::Window => return None,
        }
        Some(choices)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport::Navigate;
    use glam::Vec3;

    /// A new model seen from the top, 10 px per metre, the origin in the middle of 1000 × 800.
    fn modelling() -> App {
        let mut app = App::default();
        app.new_model("Level 0");
        app.viewport_size = (1000.0, 800.0);
        app.camera.set_preset(crate::viewport::camera::ViewPreset::Top);
        app.camera.ortho = true;
        app.camera.target = Vec3::ZERO;
        app.camera.distance = 40.0 / (app.camera.fov_y * 0.5).tan();
        app
    }

    fn click(app: &mut App, x: f64, y: f64) {
        let (sx, sy) = app.on_screen(DVec3::new(x, y, 0.0), 1000.0, 800.0).unwrap();
        let _ = app.update(Message::Navigate(Navigate::Pick { x: sx, y: sy, width: 1000.0, height: 800.0, add: false }));
    }

    fn place(app: &mut App, message: PlaceMessage) {
        let _ = app.update(Message::Place(message));
    }

    fn wall(app: &App, id: u64) -> PlanarDef {
        let Shape::Planar(p) = app.modeller.model.get(ElementId(id)).unwrap().shape_in_project(&app.modeller.model.grid) else { panic!() };
        p
    }

    fn ends(p: &PlanarDef) -> (DVec2, DVec2, f64, f64) {
        let PlanarForm::Wall { start, end, base, height } = p.form else { panic!() };
        (start, end, base, height)
    }

    #[test]
    fn a_wall_takes_the_thickness_height_and_base_typed_in_the_strip() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        place(&mut app, PlaceMessage::Field(PlaceField::Width, "0,3".into()));
        place(&mut app, PlaceMessage::Field(PlaceField::Height, "2.6".into()));
        place(&mut app, PlaceMessage::Field(PlaceField::Offset, "0.5".into()));
        // a value it cannot take is kept in the field and not used
        place(&mut app, PlaceMessage::Field(PlaceField::Width, "-1".into()));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 5.0, 0.0);
        let w = wall(&app, 1);
        assert_eq!((w.thickness, ends(&w).2, ends(&w).3), (0.3, 0.5, 2.6));
        // an empty height runs up to the next level again
        place(&mut app, PlaceMessage::Field(PlaceField::Height, "".into()));
        assert_eq!(app.modeller.placement.of(ModelTool::Wall).height, None);
    }

    #[test]
    fn a_wall_type_from_the_library_gives_its_layers_and_the_preview_shows_them() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        let kinds = app.kinds(ModelTool::Wall, Language::Dutch).unwrap();
        assert!(kinds.len() > 1 && kinds[0].id.is_none(), "generic, then the library's wall types");
        let id = kinds[1].id.clone().unwrap();
        place(&mut app, PlaceMessage::Kind(Some(id.clone())));
        click(&mut app, 0.0, 0.0);
        let (sx, sy) = app.on_screen(DVec3::new(4.0, 0.0, 0.0), 1000.0, 800.0).unwrap();
        let _ = app.update(Message::Navigate(Navigate::Hover { x: sx, y: sy, width: 1000.0, height: 800.0 }));
        let Some(Shape::Planar(preview)) = app.placing() else { panic!() };
        assert_eq!(preview.build_up.as_ref().map(|b| b.id.as_str()), Some(id.as_str()));
        click(&mut app, 4.0, 0.0);
        let w = wall(&app, 1);
        assert_eq!(w.build_up.map(|b| b.id), Some(id));
    }

    #[test]
    fn a_wall_and_a_column_of_an_inb_type_are_placed_and_say_what_they_are_made_of() {
        let mut app = modelling();
        let _ = app.update(Message::Library(crate::app::library::LibraryMessage::AddInb));
        // the wall tool lists the catalogue's types under the names the template gives them
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        let kinds = app.kinds(ModelTool::Wall, Language::Dutch).unwrap();
        let kzst = kinds.iter().find(|k| k.id.as_deref() == Some("inb.wall.kzst_100")).expect("the INB sand-lime wall");
        assert_eq!(kzst.name, "kzst_100");
        place(&mut app, PlaceMessage::Kind(kzst.id.clone()));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 4.0, 0.0);
        let w = wall(&app, 1);
        assert!((w.total_thickness() - 0.1).abs() < 1e-12);
        let props = app.model.as_ref().unwrap().element_props(1);
        let said = |key: &str| props.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
        assert_eq!((said("Type"), said("Layers")), (Some("kzst_100"), Some("sand-lime brick 100")));
        // and a concrete column of the catalogue
        app.set_model_tool(None);
        app.set_model_tool(Some((ModelTool::Column, "Column")));
        let kinds = app.kinds(ModelTool::Column, Language::Dutch).unwrap();
        let bk = kinds.iter().find(|k| k.id.as_deref() == Some("inb.column.bk-300x300")).expect("the INB concrete column");
        assert_eq!(bk.name, "BK 300x300");
        place(&mut app, PlaceMessage::Kind(bk.id.clone()));
        click(&mut app, 8.0, 3.0);
        let column = app.modeller.model.iter().find(|e| e.class() == "IfcColumn").expect("a column");
        let Shape::Linear(c) = &column.shape else { panic!() };
        assert_eq!(c.profile.name(), "BK 300x300");
        let outline = c.profile.profile().unwrap();
        assert!((outline.area() - 0.09).abs() < 1e-9, "300 × 300");
    }

    #[test]
    fn the_slab_tool_makes_a_roof_or_a_ceiling_classed_and_typed_as_such() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Slab, "Slab")));
        // a roof: the types offered are the library's roof types
        place(&mut app, PlaceMessage::Role(bim_core::SlabRole::Roof));
        let kinds = app.kinds(ModelTool::Slab, Language::English).unwrap();
        let roofs: Vec<String> = app.library.names(bim_library::Role::Roof, bim_library::Lang::En).into_iter().map(|(_, n)| n).collect();
        assert_eq!(kinds.iter().skip(1).map(|k| k.name.clone()).collect::<Vec<_>>(), roofs);
        place(&mut app, PlaceMessage::Field(PlaceField::Offset, "3".into()));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 6.0, 4.0);
        place(&mut app, PlaceMessage::Role(bim_core::SlabRole::Ceiling));
        place(&mut app, PlaceMessage::Field(PlaceField::Offset, "2.6".into()));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 6.0, 4.0);
        let classes: Vec<&str> = app.modeller.model.iter().map(|e| e.class()).collect();
        assert_eq!(classes, ["IfcRoof", "IfcCovering"]);
        // each on a layer of its own, and a roof in IFC 4 is a slab of the roof
        let scene = app.model.clone().unwrap();
        let layers: Vec<&str> = scene.layers.iter().map(|l| l.name.as_str()).collect();
        assert!(layers.contains(&"IfcRoof") && layers.contains(&"IfcCovering"), "{layers:?}");
        let text = String::from_utf8(crate::export::to_bytes(&scene, &app.document_now(), "roof.ifc").unwrap()).unwrap();
        assert!(text.contains("=IFCSLAB(") && text.contains(".ROOF.)") && text.contains("=IFCCOVERING(") && text.contains(".CEILING.)"), "{}", text.lines().filter(|l| l.contains("IFCSLAB(") || l.contains("IFCCOVERING(")).collect::<Vec<_>>().join("\n"));
        // the definition keeps the role, and a floor writes none
        let json = serde_json::to_string(&app.modeller.model.iter().next().unwrap().shape).unwrap();
        assert!(json.contains("\"role\":\"roof\""), "{json}");
        // a roof is shaped as a floor is: Tab opens the sketch of its outline
        app.set_model_tool(None);
        app.select_modelled(ElementId(1));
        let _ = app.update(Message::EditMode);
        assert!(app.modeller.sketch.is_some());
    }

    #[test]
    fn the_wall_tool_draws_a_rectangle_of_four_walls_with_two_clicks() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        place(&mut app, PlaceMessage::Rectangle(true));
        click(&mut app, 0.0, 0.0);
        assert_eq!(app.modeller.model.len(), 0);
        click(&mut app, 6.0, 4.0);
        let walls: Vec<(DVec2, DVec2)> = (1..=4).map(|id| {
            let w = wall(&app, id);
            let bim_core::element::PlanarForm::Wall { start, end, .. } = w.form else { panic!() };
            (start, end)
        }).collect();
        let c = |x: f64, y: f64| DVec2::new(x, y);
        assert_eq!(walls, vec![(c(0.0, 0.0), c(6.0, 0.0)), (c(6.0, 0.0), c(6.0, 4.0)), (c(6.0, 4.0), c(0.0, 4.0)), (c(0.0, 4.0), c(0.0, 0.0))]);
        assert!(app.modeller.points.is_empty(), "the next click starts a new rectangle");
        // the corners are cut: each wall's solid is shorter than its axis by what the others take
        let volume: f64 = (1..=4).map(|id| app.modeller.model.evaluate(ElementId(id)).unwrap().iter().map(|p| p.solid.volume()).sum::<f64>()).sum();
        assert!((volume - (2.0 * (6.0 + 4.0)) * 0.2 * 3.0).abs() < 0.2 * 0.2 * 3.0 * 4.0 + 1e-9, "{volume}");
        // two clicks on one line make nothing
        click(&mut app, 10.0, 0.0);
        click(&mut app, 12.0, 0.0);
        assert_eq!(app.modeller.model.len(), 4);
    }

    #[test]
    fn walls_drawn_on_their_exterior_face_meet_at_their_corners() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        place(&mut app, PlaceMessage::Location(LocationLine::Exterior));
        // anticlockwise round a 6 × 4 rectangle seen from above: the exterior (right) is outside
        for (x, y) in [(0.0, 0.0), (6.0, 0.0), (6.0, 4.0)] {
            click(&mut app, x, y);
        }
        let (a, b) = (ends(&wall(&app, 1)), ends(&wall(&app, 2)));
        // the axes lie 0.1 inside the lines drawn, and the first's end is the second's start
        assert!((a.0 - DVec2::new(0.0, 0.1)).length() < 1e-9, "{:?}", a.0);
        assert!((a.1 - DVec2::new(5.9, 0.1)).length() < 1e-9 && (b.0 - a.1).length() < 1e-9, "{:?} {:?}", a.1, b.0);
        assert!((b.1 - DVec2::new(5.9, 4.0)).length() < 1e-9, "{:?}", b.1);
        // so they get a corner: the first wall's volume is less than a box to the line's end
        let volume: f64 = app.modeller.model.evaluate(ElementId(1)).unwrap().iter().map(|p| p.solid.volume()).sum();
        assert!(volume > 0.0);
    }

    #[test]
    fn without_chain_the_next_wall_starts_afresh() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 4.0, 0.0);
        assert_eq!(app.modeller.points.len(), 1, "chained: the next starts at the end");
        place(&mut app, PlaceMessage::Chain(false));
        assert!(app.modeller.points.is_empty());
        click(&mut app, 0.0, 3.0);
        click(&mut app, 4.0, 3.0);
        assert!(app.modeller.points.is_empty());
        assert_eq!(app.modeller.model.len(), 2);
    }

    #[test]
    fn a_column_takes_a_steel_profile_and_a_window_its_sill() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Column, "Column")));
        place(&mut app, PlaceMessage::Kind(Some("HEA 200".into())));
        place(&mut app, PlaceMessage::Field(PlaceField::Height, "4".into()));
        click(&mut app, 2.0, 2.0);
        let Shape::Linear(c) = app.modeller.model.get(ElementId(1)).unwrap().shape_in_project(&app.modeller.model.grid) else { panic!() };
        assert_eq!(c.profile.name(), "HEA 200");
        assert!((c.end.z - c.start.z - 4.0).abs() < 1e-9);
        // a wall, and a window in it on a sill typed in
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        click(&mut app, 4.0, 0.0);
        click(&mut app, 10.0, 0.0);
        app.set_model_tool(Some((ModelTool::Window, "Window")));
        place(&mut app, PlaceMessage::Field(PlaceField::Sill, "0.6".into()));
        place(&mut app, PlaceMessage::Field(PlaceField::Width, "1.5".into()));
        let (sx, sy) = app.on_screen(DVec3::new(7.0, 0.0, 3.0), 1000.0, 800.0).unwrap();
        let _ = app.update(Message::Navigate(Navigate::Pick { x: sx, y: sy, width: 1000.0, height: 800.0, add: false }));
        let o = app.modeller.model.openings_of(ElementId(2));
        assert_eq!(o.len(), 1);
        assert_eq!((o[0].sill, o[0].width), (0.6, 1.5));
    }

    fn system(app: &App, id: u64) -> bim_core::SystemDef {
        let Shape::System(s) = app.modeller.model.get(ElementId(id)).unwrap().shape_in_project(&app.modeller.model.grid) else { panic!() };
        s
    }

    #[test]
    fn a_system_type_picked_in_the_strip_is_what_the_tool_draws() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::System, "Curtain wall")));
        // generic first, then the upright types under their own names: a ceiling is not drawn as a wall
        let kinds = app.kinds(ModelTool::System, Language::English).unwrap();
        assert!(kinds[0].id.is_none());
        let timber = kinds.iter().find(|k| k.id.as_deref() == Some("system.timber-frame-140")).expect("a timber frame");
        assert!(timber.name.starts_with("Timber frame") && timber.name.ends_with("600"), "{}", timber.name);
        assert!(app.library.system_type("system.ceiling-600x600").is_some());
        assert!(kinds.iter().any(|k| k.id.as_deref() == Some("system.ceiling-600x600")), "a lying type is laid in a rectangle, so it is offered too");
        let names: std::collections::HashSet<&str> = kinds.iter().map(|k| k.name.as_str()).collect();
        assert_eq!(names.len(), kinds.len(), "each type is told from the others by its name");
        // the type picked is what the cursor shows and what the second click places
        place(&mut app, PlaceMessage::Kind(Some("system.timber-frame-140".into())));
        click(&mut app, 0.0, 0.0);
        let (sx, sy) = app.on_screen(DVec3::new(4.8, 0.0, 0.0), 1000.0, 800.0).unwrap();
        let _ = app.update(Message::Navigate(Navigate::Hover { x: sx, y: sy, width: 1000.0, height: 800.0 }));
        let Some(Shape::System(preview)) = app.placing() else { panic!() };
        assert_eq!(preview.system_type.as_ref().map(|t| t.id.as_str()), Some("system.timber-frame-140"));
        click(&mut app, 4.8, 0.0);
        let frame = system(&app, 1);
        assert_eq!(frame.system_type.as_ref().map(|t| t.id.as_str()), Some("system.timber-frame-140"));
        assert_eq!((frame.start, frame.end), (DVec2::ZERO, DVec2::new(4.8, 0.0)));
        // it is named after what it is, the generic one after its tool
        assert_eq!(app.modeller.model.get(ElementId(1)).unwrap().name, "Timber frame 1");
        place(&mut app, PlaceMessage::Chain(false));
        place(&mut app, PlaceMessage::Kind(None));
        click(&mut app, 0.0, 6.0);
        click(&mut app, 4.8, 6.0);
        assert!(system(&app, 2).system_type.is_none());
        assert_eq!(app.modeller.model.get(ElementId(2)).unwrap().name, "Curtain wall 2");
        // the frame's studs, plates and layers are its type's: far more than a generic grid as long
        let parts = |id| app.modeller.model.evaluate(ElementId(id)).unwrap().len();
        assert!(parts(1) > parts(2), "{} {}", parts(1), parts(2));
    }

    #[test]
    fn a_lying_type_is_laid_in_the_rectangle_between_two_clicks() {
        use bim_core::Shape;
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::System, "Systeem")));
        // the floors and ceilings are among the types to choose
        let choices = app.kinds(ModelTool::System, openaec_ui::Language::English).unwrap();
        assert!(choices.iter().any(|c| c.id.as_deref() == Some("system.hollow-core-200")) && choices.iter().any(|c| c.id.as_deref() == Some("system.ceiling-600x600")));
        place(&mut app, PlaceMessage::Kind(Some("system.hollow-core-200".into())));
        place(&mut app, PlaceMessage::Chain(false));
        click(&mut app, 3.6, 6.0);
        click(&mut app, 0.0, 0.0);
        let model = &app.modeller.model;
        assert_eq!(model.len(), 1);
        let Shape::System(floor) = model.get(ElementId(1)).unwrap().shape_in_project(&model.grid) else { panic!() };
        // whichever corner is clicked first: its line along the near side, 6 m wide, its top 3 m up
        assert!(floor.lies());
        assert_eq!((floor.start, floor.end, floor.height, floor.base), (DVec2::ZERO, DVec2::new(3.6, 0.0), 6.0, 3.0));
        let parts = model.evaluate(ElementId(1)).unwrap();
        assert_eq!(parts.iter().filter(|p| p.material.as_deref() == Some("hollow-core-concrete")).count(), 3, "three slabs 1200 wide");
        // another lying type fits it, an upright one does not
        app.select_modelled(ElementId(1));
        assert_eq!(app.assignable_systems("system.hollow-core-260"), [ElementId(1)]);
        assert!(app.assignable_systems("system.timber-frame-140").is_empty());
        assert_eq!(app.assign_system_type("system.hollow-core-260"), 1);
    }

    #[test]
    fn timber_frames_drawn_with_the_tool_make_their_corners_and_t_junctions() {
        use bim_core::system::{corners, tees};
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::System, "Vliesgevel")));
        place(&mut app, PlaceMessage::Kind(Some("system.timber-frame-140".into())));
        // chained: the second starts where the first ends, and there they make a corner
        for (x, y) in [(0.0, 0.0), (4.8, 0.0), (4.8, 3.6)] {
            click(&mut app, x, y);
        }
        let model = &app.modeller.model;
        assert_eq!(model.len(), 2);
        assert_eq!(corners(model, model.get(ElementId(1)).unwrap())[1].map(|m| m.with), Some(ElementId(2)));
        assert_eq!(corners(model, model.get(ElementId(2)).unwrap())[0].map(|m| m.with), Some(ElementId(1)));
        // named in the language the tool is named in
        assert_eq!(model.get(ElementId(2)).unwrap().name, "Houtskeletbouw 2");
        // an inner wall of another type, drawn from the first one's side, stands against it
        place(&mut app, PlaceMessage::Chain(false));
        place(&mut app, PlaceMessage::Kind(Some("system.timber-frame-internal-89".into())));
        click(&mut app, 2.4, 0.0);
        click(&mut app, 2.4, 3.0);
        let model = &app.modeller.model;
        assert_eq!(system(&app, 3).system_type.map(|t| t.id), Some("system.timber-frame-internal-89".to_string()));
        assert_eq!(tees(model, model.get(ElementId(3)).unwrap()).ends, [Some(ElementId(1)), None]);
        let against: Vec<ElementId> = tees(model, model.get(ElementId(1)).unwrap()).sides.iter().map(|a| a.with).collect();
        assert_eq!(against, [ElementId(3)]);
    }

    #[test]
    fn a_placed_element_is_named_after_its_tool_unless_a_system_type_says_what_it_is() {
        let mut app = modelling();
        // no type: the tool's name, in whatever language it is given
        assert_eq!(app.placed_name(ModelTool::System, "Vliesgevel"), "Vliesgevel");
        // a wall's type does not rename it
        app.set_model_tool(Some((ModelTool::Wall, "Wand")));
        let wall_type = app.kinds(ModelTool::Wall, Language::Dutch).unwrap()[1].id.clone();
        place(&mut app, PlaceMessage::Kind(wall_type));
        assert_eq!(app.placed_name(ModelTool::Wall, "Wand"), "Wand");
        // a system type names it after its group, in the tool's language
        app.set_model_tool(Some((ModelTool::System, "Vliesgevel")));
        for (id, nl, en) in [
            ("system.timber-frame-140", "Houtskeletbouw", "Timber frame"),
            ("system.masonry-waal-stretcher", "Metselwerk", "Masonry"),
            ("system.curtain-wall-aluminium-1500", "Vliesgevel", "Curtain wall"),
        ] {
            place(&mut app, PlaceMessage::Kind(Some(id.into())));
            assert_eq!(app.placed_name(ModelTool::System, ModelTool::System.label(Language::Dutch)), nl);
            assert_eq!(app.placed_name(ModelTool::System, ModelTool::System.label(Language::English)), en);
            for l in [Language::German, Language::French] {
                let group = app.library.system_type(id).unwrap().group;
                assert_eq!(app.placed_name(ModelTool::System, ModelTool::System.label(l)), group.name(crate::app::library::lang(l)));
            }
        }
        // a name that is no language's name of the tool is kept
        assert_eq!(app.placed_name(ModelTool::System, "E"), "E");
    }

    #[test]
    fn the_strip_shows_while_a_tool_is_on() {
        let mut app = modelling();
        assert!(app.placement_bar(crate::theme::ThemeKind::Light.palette(), Language::Dutch).is_none());
        for tool in ModelTool::ALL.into_iter().chain(ModelTool::OPENINGS) {
            app.set_model_tool(Some((tool, "E")));
            assert!(app.placement_bar(crate::theme::ThemeKind::Light.palette(), Language::Dutch).is_some(), "{tool:?}");
            assert!(!fields(tool, Language::Dutch).is_empty());
            app.set_model_tool(None);
        }
    }
}
