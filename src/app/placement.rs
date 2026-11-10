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
}

impl Settings {
    /// What a tool places when nothing was typed.
    pub fn default_for(tool: ModelTool) -> Self {
        let plain = |width: f64, depth: f64, height: Option<f64>| Settings { kind: None, width, depth, height, offset: 0.0, sill: 0.0, location: LocationLine::Centre };
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
    Chain(bool),
}

/// The settings of every tool, the chain, and a value being typed.
#[derive(Debug, Clone)]
pub struct Placement {
    settings: HashMap<ModelTool, Settings>,
    pub chain: bool,
    typing: Option<(PlaceField, String)>,
    /// The wall placed last while chaining: the next one on a face meets it where their axes cross.
    pub last_wall: Option<ElementId>,
}

impl Default for Placement {
    fn default() -> Self {
        Self { settings: HashMap::new(), chain: true, typing: None, last_wall: None }
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
        let label = |t: &'static str| text(t).size(12).color(p.text_secondary);
        let mut bar = Row::new().spacing(8).align_y(Center).push(text(tool.label(l)).size(12).color(p.text));
        if let Some(choices) = self.kinds(tool, l) {
            let picked = choices.iter().find(|c| c.id == s.kind).cloned();
            let pick = pick_list(choices, picked, |c: Choice| Message::Place(PlaceMessage::Kind(c.id))).text_size(12).padding([2, 6]).width(180);
            bar = bar.push(label(l.text("Type", "Type"))).push(pick);
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
            bar = bar.push(label(name)).push(input);
        }
        if tool == ModelTool::Wall {
            let options: Vec<Located> = LocationLine::ALL.iter().map(|&k| Located(k, k.label(l))).collect();
            let picked = options.iter().find(|o| o.0 == s.location).copied();
            bar = bar.push(label(l.text("Locatielijn", "Location line"))).push(pick_list(options, picked, |o: Located| Message::Place(PlaceMessage::Location(o.0))).text_size(12).padding([2, 6]));
        }
        if tool.chains() {
            let on = self.modeller.placement.chain;
            let chain = button(text(l.text("Ketting", "Chain")).size(12)).padding([3, 10]).style(move |_, st| p.ribbon_button(on, st)).on_press(Message::Place(PlaceMessage::Chain(!on)));
            bar = bar.push(chain);
        }
        Some(container(bar).padding([6, 10]).width(Fill).style(move |_| p.ribbon()).into())
    }

    /// The types `tool` can take, generic first; `None` for a tool without types.
    fn kinds(&self, tool: ModelTool, l: Language) -> Option<Vec<Choice>> {
        let lang = super::library::lang(l);
        let generic = Choice { id: None, name: l.text("Generiek", "Generic").to_string() };
        let mut choices = vec![generic];
        match tool {
            ModelTool::Wall | ModelTool::Slab => {
                let role = if tool == ModelTool::Wall { bim_library::Role::Wall } else { bim_library::Role::Floor };
                choices.extend(self.library.names(role, lang).into_iter().map(|(id, name)| Choice { id: Some(id.to_string()), name }));
            }
            ModelTool::Column | ModelTool::Beam => {
                choices.extend(bim_library::profile::steel_profiles().iter().map(|p| Choice { id: Some(p.designation.clone()), name: p.designation.clone() }));
            }
            ModelTool::System => {
                for group in bim_library::SystemGroup::ALL {
                    for t in self.library.system_types_of(group).filter(|t| t.lying.is_none()) {
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
        let o = wall(&app, 2).openings;
        assert_eq!(o.len(), 1);
        assert_eq!((o[0].sill, o[0].width), (0.6, 1.5));
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
