//! The project document in IfcX (IFC5) form, kept beside the loaded model: what the app
//! recognises in a model and what the user adds, in the shape of IfcX nodes, so that it can be
//! written as `.ifcx` (the native format to come): levels, the plans made on them, sections and
//! elevations, 3D views and sheets.
//!
//! Headless: no GUI or GPU, so the MCP server and a later BIM kernel crate can use it as it is.

mod soundings;

use crate::model::Model;
use bim_library::house::Field;
use std::collections::HashMap;
type TitleFields = HashMap<Field, String>;
use crate::units::UnitKeySet;
use serde_json::{json, Value};
use std::sync::Arc;

/// The IfcX class of a level.
pub const STOREY_CLASS: &str = "IfcBuildingStorey";
/// The buildingSMART identifier of [`STOREY_CLASS`] in IFC5.
pub const STOREY_URI: &str = "https://identifier.buildingsmart.org/uri/buildingsmart/ifc/5/class/IfcBuildingStorey";

/// A level (storey) of the building.
#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    /// The IfcX path of the node, a UUID.
    pub path: String,
    pub name: String,
    /// Metres, Z up, in the frame of the model's display coordinates.
    pub elevation: f64,
    /// Made in the app, not read from the model: only these can be removed.
    pub added: bool,
}

/// The layers a view shows otherwise than the model has them, by layer name: on (`true`) or off.
/// Empty: the view shows the layers as the model does.
pub type LayerSwitches = std::collections::BTreeMap<String, bool>;

/// How far a floor plan cuts above its level (metres), as is usual for floor plans.
pub const PLAN_CUT: f64 = 1.2;
/// How far a floor plan looks below its level (metres), so the floor it stands on shows.
pub const PLAN_DEPTH: f64 = 0.3;

/// How far a structural plan looks below its level (metres), so the beams under its floor show.
pub const STRUCTURAL_DEPTH: f64 = 1.0;
/// How far a ceiling plan cuts above its level (metres), under the ceilings.
pub const CEILING_CUT: f64 = 2.3;
/// How far a ceiling plan looks up from its cut (metres) when no level lies above its own.
pub const CEILING_DEPTH: f64 = 1.0;

/// The field of view a 3D view gets when its file names none (degrees, vertical).
pub const VIEW_3D_FOV: f64 = 45.0;

/// The kinds of plan a level has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlanKind {
    /// Seen from above, cut [`PLAN_CUT`] above its level, looking [`PLAN_DEPTH`] below it.
    #[default]
    Floor,
    /// The building's structure seen from above: cut as a floor plan, looking
    /// [`STRUCTURAL_DEPTH`] below its level; elements that carry nothing are left out.
    Structural,
    /// A reflected ceiling plan: cut [`CEILING_CUT`] above its level, looking up to the next
    /// level (else [`CEILING_DEPTH`] beyond its cut), drawn as the floor plan is oriented.
    Ceiling,
    /// The structure seen from under it but drawn as from above (a pile plan): cut under the
    /// foundation, looking up to its view depth; elements that carry nothing are left out.
    StructuralUp,
}

impl PlanKind {
    pub const ALL: [PlanKind; 4] = [PlanKind::Floor, PlanKind::Structural, PlanKind::Ceiling, PlanKind::StructuralUp];

    /// Its name in IFCX (`kind` of an `o3v::view::plan` node).
    pub fn key(self) -> &'static str {
        match self {
            PlanKind::Floor => "floor",
            PlanKind::Structural => "structural",
            PlanKind::Ceiling => "ceiling",
            PlanKind::StructuralUp => "structural-up",
        }
    }

    /// The kind named `key` in IFCX; a floor plan for anything else.
    pub fn from_key(key: &str) -> Self {
        Self::ALL.into_iter().find(|k| k.key() == key).unwrap_or_default()
    }

    /// Whether it looks up from its cut, not down.
    pub fn looks_up(self) -> bool {
        matches!(self, PlanKind::Ceiling | PlanKind::StructuralUp)
    }

    /// Whether its tab shows the model from below (a ceiling plan). A structural plan that looks
    /// up stays a view from above, as it is drawn.
    pub fn seen_from_below(self) -> bool {
        self == PlanKind::Ceiling
    }

    /// Whether it shows the structure only.
    pub fn structural(self) -> bool {
        matches!(self, PlanKind::Structural | PlanKind::StructuralUp)
    }
}

/// What the lines of a shape drawn on a view are together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShapeKind {
    /// Lines on from each other, open or closed.
    Polyline,
    Rectangle,
    Circle,
    Arc,
    Ellipse,
}

impl ShapeKind {
    pub const ALL: [ShapeKind; 5] = [ShapeKind::Polyline, ShapeKind::Rectangle, ShapeKind::Circle, ShapeKind::Arc, ShapeKind::Ellipse];

    /// Its name in IFCX (`shapes` of a view).
    pub fn key(self) -> &'static str {
        match self {
            ShapeKind::Polyline => "polyline",
            ShapeKind::Rectangle => "rectangle",
            ShapeKind::Circle => "circle",
            ShapeKind::Arc => "arc",
            ShapeKind::Ellipse => "ellipse",
        }
    }

    /// The kind named `key` in IFCX.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }
}

/// A shape drawn on a view: a run of the view's lines that is one thing, picked and removed
/// together. A line alone needs none. The lines stay what is drawn, exported and read by a
/// program that knows no shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    pub kind: ShapeKind,
    /// Its first line among the view's lines, and how many it has.
    pub first: usize,
    pub count: usize,
}

impl Shape {
    /// The places of its lines among the view's.
    pub fn run(&self) -> std::ops::Range<usize> {
        self.first..self.first + self.count
    }
}

/// A plan of a level: the model seen from above (or from below, a ceiling plan), cut at
/// [`PlanView::cut`] above its level.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanView {
    pub orthophoto: Option<crate::raster::OrthoPhoto>,
    /// The IfcX path, a UUID.
    pub path: String,
    pub name: String,
    /// The path of its level.
    pub level: String,
    pub kind: PlanKind,
    /// The cut above the level (metres).
    pub cut: f64,
    /// How far it looks below the level (metres); a ceiling plan: how far it looks up from its
    /// cut.
    pub depth: f64,
    /// The scale its texts and dimensions are sized for (1 : this), as in its own tab.
    pub scale: f64,
    /// The kind of drawing it is, by the key the project's drawing standard has it under: it is
    /// drawn by that kind's rules. `None`: the kind usual for what it is.
    pub drawing: Option<String>,
    /// Authored line pens, grid visibility and view placement.
    pub presentation: Value,
    /// Its dimensions, shown wherever it is placed.
    pub dimensions: Vec<Dimension>,
    /// Its texts, shown wherever it is placed.
    pub notes: Vec<Note>,
    /// The lines drawn on it that are no model: the shapes and leaders of its tags (x, y, metres).
    pub lines: Vec<[[f64; 2]; 2]>,
    /// Which runs of those lines are one shape each (a rectangle, a circle).
    pub shapes: Vec<Shape>,
    /// What the modelled project says on it by itself, made again after every change of the
    /// model and not kept in the file: the names and areas of its spaces.
    pub tags: Vec<Note>,
    /// Generate plan symbols from the actual piles. Existing authored plans opt in explicitly.
    pub pile_symbols: bool,
    pub pile_tag_marks: bool,
    /// Per-element tag offset in model metres, relative to the pile head.
    pub pile_offsets: std::collections::BTreeMap<u64, [f64; 2]>,
    /// Regenerated tag index, element identity, and head used by drag editing.
    pub pile_tags: Vec<(usize, u64, [f64; 2])>,
    pub corner_labels: bool,
    pub office_symbol_lines: usize,
    pub symbol_fills: Vec<Fill>,
    /// And the lines it draws by itself: the swings of its doors.
    pub symbols: Vec<[[f64; 2]; 2]>,
    /// The regions drawn on it in one colour that are no model, under its lines.
    pub fills: Vec<Fill>,
    /// What it shows of the model in plan: lower left and upper right (x, y, metres); `None`:
    /// all of it.
    pub crop: Option<([f64; 2], [f64; 2])>,
    /// Its own layers: those it shows otherwise than the model.
    pub layers: LayerSwitches,
    /// The elements hidden in it with Hide or Isolate.
    pub hidden: Arc<UnitKeySet>,
}

impl SectionView {
    /// What its mark on a floor plan says: a section its letter (the last word of "Section A"),
    /// an elevation the side it looks at (the first word of "North elevation").
    pub fn mark(&self) -> String {
        if let Some(mark) = self.presentation["mark"].as_str().map(str::trim).filter(|s| !s.is_empty()) {
            return mark.to_owned();
        }
        let word = match self.kind {
            SectionKind::Section => self.name.rsplit(' ').next(),
            SectionKind::Elevation => self.name.split(' ').next(),
        };
        word.unwrap_or(&self.name).to_string()
    }
}

/// Authored information on a pile legend; shaft, length and end levels follow its members.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PileLegendField {
    Report,
    Advice,
    Execution,
    Datum,
    Foot(String),
    Uncut(String),
    Cutoff(String),
}

/// The kinds of view along a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SectionKind {
    /// Cut along its line, inside the model.
    #[default]
    Section,
    /// Outside the model, looking at a side of it: it cuts nothing.
    Elevation,
}

impl SectionKind {
    /// Its name in IFCX (`kind` of an `o3v::view::section` node).
    pub fn key(self) -> &'static str {
        match self {
            SectionKind::Section => "section",
            SectionKind::Elevation => "elevation",
        }
    }

    /// The kind named `key` in IFCX; a section for anything else.
    pub fn from_key(key: &str) -> Self {
        if key == "elevation" { SectionKind::Elevation } else { SectionKind::Section }
    }
}

/// A section or an elevation: the model seen level, looking to the left of the line `from` →
/// `to` (plan, metres), cut along that line and seen as far as `depth` beyond it. An elevation
/// stands outside the model, so it cuts nothing. Its drawing has `u` along the line (from
/// `from`) and `v` up (the model's height).
#[derive(Debug, Clone, PartialEq)]
pub struct SectionView {
    /// The IfcX path, a UUID.
    pub path: String,
    pub name: String,
    pub kind: SectionKind,
    pub from: [f64; 2],
    pub to: [f64; 2],
    /// How far it looks beyond its line (metres).
    pub depth: f64,
    /// The scale its texts and dimensions are sized for (1 : this), as in its own tab.
    pub scale: f64,
    /// The kind of drawing it is, by the key the project's drawing standard has it under: it is
    /// drawn by that kind's rules. `None`: the kind usual for what it is.
    pub drawing: Option<String>,
    /// Authored line pens, grid visibility and view placement.
    pub presentation: Value,
    /// Its dimensions, in its drawing's (u, v).
    pub dimensions: Vec<Dimension>,
    /// Its texts, in its drawing's (u, v).
    pub notes: Vec<Note>,
    /// The lines drawn on it that are no model: the shapes and leaders of its tags (u, v).
    pub lines: Vec<[[f64; 2]; 2]>,
    /// Which runs of those lines are one shape each (a rectangle, a circle).
    pub shapes: Vec<Shape>,
    /// The regions drawn on it in one colour that are no model, under its lines.
    pub fills: Vec<Fill>,
    /// What it shows, in its drawing's (u, v); `None`: all of it.
    pub crop: Option<([f64; 2], [f64; 2])>,
    /// Its own layers: those it shows otherwise than the model.
    pub layers: LayerSwitches,
    /// The elements hidden in it with Hide or Isolate.
    pub hidden: Arc<UnitKeySet>,
}

impl SectionView {
    /// Along the line (unit, plan) and the way it looks (unit, plan: to the left of the line).
    pub fn axes(&self) -> ([f64; 2], [f64; 2]) {
        let (dx, dy) = (self.to[0] - self.from[0], self.to[1] - self.from[1]);
        let run = dx.hypot(dy).max(1e-9);
        let along = [dx / run, dy / run];
        (along, [-along[1], along[0]])
    }
}

/// Where a 3D view looks from and to (world, Z up, metres), and how.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera3d {
    pub eye: [f64; 3],
    pub target: [f64; 3],
    /// In perspective, else parallel (orthographic).
    pub perspective: bool,
    /// The vertical field of view (degrees).
    pub fov: f64,
}

/// A 3D view's section box: its limits in its own frame, which is the world turned by −`turn`
/// (radians, counter-clockwise seen from above) about the vertical through `pivot` (metres).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewBox {
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub turn: f64,
    pub pivot: [f64; 3],
}

/// A 3D view: the model as its camera sees it, within its section box, with its own layers and
/// hidden elements.
#[derive(Debug, Clone, PartialEq)]
pub struct View3d {
    /// The IfcX path, a UUID.
    pub path: String,
    pub name: String,
    pub camera: Camera3d,
    /// Its section box, when it has one on.
    pub section: Option<ViewBox>,
    /// Its own layers: those it shows otherwise than the model.
    pub layers: LayerSwitches,
    /// The elements hidden in it with Hide or Isolate.
    pub hidden: Arc<UnitKeySet>,
    /// What it shows of the loads and results of a structural analysis model.
    pub analysis: Option<crate::structural::diagrams::Shown>,
}

/// How the ends of a dimension line are marked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tick {
    /// A diagonal stroke.
    #[default]
    Slash,
    /// A filled arrow head.
    Arrow,
    /// A filled dot.
    Dot,
    /// Nothing.
    None,
}

impl Tick {
    pub const ALL: [Tick; 4] = [Tick::Slash, Tick::Arrow, Tick::Dot, Tick::None];

    /// Its name in IFCX (`tick` of a dimension style).
    pub fn key(self) -> &'static str {
        match self {
            Tick::Slash => "slash",
            Tick::Arrow => "arrow",
            Tick::Dot => "dot",
            Tick::None => "none",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.key() == key)
    }
}

/// The unit a dimension's figure is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DimensionUnit {
    #[default]
    Millimetres,
    Metres,
}

impl DimensionUnit {
    /// Its name in IFCX (`unit` of a dimension style).
    pub fn key(self) -> &'static str {
        match self {
            DimensionUnit::Millimetres => "mm",
            DimensionUnit::Metres => "m",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        [DimensionUnit::Millimetres, DimensionUnit::Metres].into_iter().find(|u| u.key() == key)
    }
}

/// How dimensions look: a dimension names one of the document's styles. Sizes are on paper
/// (metres); a view's scale makes them that much larger on its drawing.
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionStyle {
    pub name: String,
    /// The height of the figure's letters.
    pub size: f64,
    /// Explicit typeface and nominal em size in typographic points on paper.
    pub font_family: Option<Arc<str>>,
    pub font_size_pt: Option<f64>,
    /// Read the figure along its measurement line from the foot or right side of the page.
    pub aligned_text: bool,
    /// Give a chain intervals that add up to its rounded reference stations.
    pub round_stations: bool,
    pub tick: Tick,
    /// Half the length of a slash, the length of an arrow, the diameter of a dot.
    pub tick_size: f64,
    /// How far an extension line runs past the dimension line.
    pub extension: f64,
    /// How far an extension line starts from the point it measures.
    pub gap: f64,
    pub unit: DimensionUnit,
    /// The decimals of the figure, in its unit.
    pub decimals: u8,
}

impl DimensionStyle {
    /// The name of the style every document has.
    pub const STANDARD: &'static str = "Standard";

    /// The style of a dimension that names none.
    pub fn standard() -> Self {
        Self { name: Self::STANDARD.to_string(), size: NOTE_HEIGHT, font_family: None, font_size_pt: None, aligned_text: false, round_stations: false, tick: Tick::Slash, tick_size: 0.0012, extension: 0.0015, gap: 0.0015, unit: DimensionUnit::Millimetres, decimals: 0 }
    }

    pub fn paper_font_size(&self) -> f64 {
        self.font_size_pt.map_or(self.size, |pt| pt * 0.0254 / 72.0)
    }

    pub fn label_family(&self) -> Option<Arc<str>> {
        self.font_family.clone().or_else(|| self.font_size_pt.map(|_| Arc::from("Inter")))
    }

    /// A readable baseline in the drawing's y-up coordinate system.
    pub fn label_angle(&self, dx: f64, dy: f64) -> f64 {
        if !self.aligned_text { return 0.0; }
        if dx < 0.0 || (dx == 0.0 && dy < 0.0) {
            (-dy).atan2(-dx)
        } else {
            dy.atan2(dx)
        }
    }

    /// Paper distance of a baseline above an aligned dimension line.
    pub const ALIGNED_LABEL_GAP: f64 = 0.0008;

    /// The figure of a length (metres) in this style.
    pub fn figure(&self, length: f64) -> String {
        let decimals = self.decimals.min(6) as usize;
        match self.unit {
            DimensionUnit::Millimetres => format!("{:.*}", decimals, length * 1000.0),
            DimensionUnit::Metres => format!("{:.*}", decimals, length),
        }
    }

    fn to_ifcx(&self) -> Value {
        let mut value = json!({"name": self.name, "size": self.size, "tick": self.tick.key(), "tick_size": self.tick_size, "extension": self.extension, "gap": self.gap, "unit": self.unit.key(), "decimals": self.decimals});
        if let Some(family) = &self.font_family { value["font_family"] = family.as_ref().into(); }
        if let Some(pt) = self.font_size_pt { value["font_size_pt"] = pt.into(); }
        if self.round_stations || self.name == "3BM ronde stip" { value["round_stations"] = self.round_stations.into(); }
        if self.aligned_text || self.name == "3BM ronde stip" { value["aligned_text"] = self.aligned_text.into(); }
        value
    }

    /// A style of an IFCX file; a missing or senseless value is the standard style's.
    pub fn from_ifcx(v: &Value) -> Option<Self> {
        let standard = Self::standard();
        let name = v.get("name")?.as_str()?.trim();
        let size = |key: &str, or: f64| v.get(key).and_then(Value::as_f64).filter(|s| s.is_finite() && (0.0..=0.1).contains(s)).unwrap_or(or);
        let office = name == "3BM ronde stip";
        let legacy_size = Some(size("size", standard.size)).filter(|s| *s > 0.0).unwrap_or(standard.size);
        let font_family = v.get("font_family").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
            .or_else(|| office.then_some(crate::typeface::ARIAL_NARROW)).map(Arc::from);
        let font_size_pt = v.get("font_size_pt").and_then(Value::as_f64).filter(|s| s.is_finite() && *s > 0.0 && *s <= 288.0)
            .or_else(|| (office && (legacy_size-standard.size).abs()<1e-12).then_some(6.8));
        (!name.is_empty()).then(|| Self {
            name: name.to_string(),
            size: legacy_size,
            font_family, font_size_pt, aligned_text: v["aligned_text"].as_bool().unwrap_or(office),
            round_stations: v["round_stations"].as_bool().unwrap_or(office),
            tick: v.get("tick").and_then(Value::as_str).and_then(Tick::from_key).unwrap_or_default(),
            tick_size: size("tick_size", standard.tick_size),
            extension: size("extension", standard.extension),
            gap: size("gap", standard.gap),
            unit: v.get("unit").and_then(Value::as_str).and_then(DimensionUnit::from_key).unwrap_or_default(),
            decimals: v.get("decimals").and_then(Value::as_u64).map_or(0, |d| d.min(6) as u8),
        })
    }
}

/// The dimension styles of a document: the standard one first, always.
#[derive(Debug, Clone, PartialEq)]
struct DimensionStyles(Vec<DimensionStyle>);

impl Default for DimensionStyles {
    fn default() -> Self {
        Self(vec![DimensionStyle::standard()])
    }
}

/// Where a text stands on its point: its lower left, the middle of its foot, or its lower right;
/// its lines are set the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

impl Align {
    /// Its name in IFCX (`align` of a text).
    pub fn key(self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        [Align::Left, Align::Center, Align::Right].into_iter().find(|a| a.key() == key)
    }
}

/// A region of a drawing filled in one colour (red, green, blue; black is the ink of the
/// drawing): its outlines, the first its outer one and the others its holes (a floor plan's x, y,
/// a section's u, v; metres).
#[derive(Debug, Clone, PartialEq)]
pub struct Fill {
    pub loops: Vec<Vec<[f64; 2]>>,
    pub color: [u8; 3],
}

impl Fill {
    fn to_ifcx(&self) -> Value {
        json!({"loops": self.loops, "color": self.color})
    }

    /// The fills of a view of an IFCX file; one without an outline of three points is left out.
    fn from_ifcx(v: Option<&Value>) -> Vec<Fill> {
        let point = |p: &Value| Some([p.get(0)?.as_f64()?, p.get(1)?.as_f64()?]).filter(|p| p.iter().all(|x| x.is_finite()));
        let all = v.and_then(Value::as_array).into_iter().flatten();
        all.filter_map(|f| {
            let loops: Vec<Vec<[f64; 2]>> = f.get("loops")?.as_array()?.iter().filter_map(|l| Some(l.as_array()?.iter().filter_map(point).collect::<Vec<_>>())).filter(|l| l.len() >= 3).collect();
            let c = f.get("color")?.as_array()?;
            let color = [u8::try_from(c.first()?.as_u64()?).ok()?, u8::try_from(c.get(1)?.as_u64()?).ok()?, u8::try_from(c.get(2)?.as_u64()?).ok()?];
            (!loops.is_empty()).then_some(Fill { loops, color })
        })
        .collect()
    }
}

/// A dimension on a drawing: the distance between two of its points (a floor plan's x, y in the
/// model's frame, a section's u, v; metres).
#[derive(Debug, Clone, PartialEq)]
pub struct Dimension {
    pub chain: Option<DimensionChain>,
    pub from: [f64; 2],
    pub to: [f64; 2],
    /// Which of the document's dimension styles it has (0: the standard one).
    pub style: usize,
    /// What its ends hold on to, `from` then `to`: an end with an anchor follows its element.
    pub anchors: [Option<Anchor>; 2],
}

/// Direction of a continuous dimension and its model references between the ends.
#[derive(Debug, Clone, PartialEq)]
pub struct DimensionChain {
    /// A point on a dimension line placed explicitly in model coordinates.
    baseline: Option<[f64; 2]>,
    offset: f64,
    axis: [f64; 2],
    inner: Vec<([f64; 2], Option<Anchor>)>,
}

/// What an end of a dimension holds on to: a point of a modelled element (a wall's start or end,
/// a slab's corner, an object's place: the element's points as edit mode has them), and how far
/// from that point the end lies. For an element with a direction (a wall, a beam) the offset is
/// along it and to its left, so the end turns with the element; else along x and y.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    /// The id of the modelled element.
    pub element: u64,
    pub point: usize,
    pub offset: [f64; 2],
    /// The element is gone: the end stays where it was, and the dimension is marked.
    pub lost: bool,
}

impl Anchor {
    fn to_ifcx(self) -> Value {
        json!([self.element, self.point, self.offset[0], self.offset[1]])
    }

    fn from_ifcx(v: &Value) -> Option<Self> {
        Some(Self { element: v.get(0)?.as_u64()?, point: v.get(1)?.as_u64()? as usize, offset: [v.get(2)?.as_f64()?, v.get(3)?.as_f64()?], lost: false })
    }
}

impl Dimension {
    pub fn references(&self) -> impl Iterator<Item = ([f64; 2], Option<Anchor>)> + '_ {
        std::iter::once((self.from, self.anchors[0]))
            .chain(self.chain.iter().flat_map(|c| c.inner.iter().copied()))
            .chain(std::iter::once((self.to, self.anchors[1])))
    }

    pub(crate) fn interval_value(&self, from: [f64; 2], to: [f64; 2], style: &DimensionStyle) -> f64 {
        let Some(chain) = self.chain.as_ref().filter(|_| style.round_stations) else {
            return (to[0]-from[0]).hypot(to[1]-from[1]);
        };
        let units = match style.unit { DimensionUnit::Millimetres => 1000., DimensionUnit::Metres => 1. };
        let precision = units * 10.0_f64.powi(i32::from(style.decimals.min(6)));
        let station = |q: [f64; 2]| ((q[0]*chain.axis[0] + q[1]*chain.axis[1]) * precision).round_ties_even();
        (station(to)-station(from)).abs() / precision
    }

    pub fn paper_offset(&self) -> f64 { self.chain.as_ref().map_or(0.006, |c| c.offset) }

    pub fn measured_ends(&self) -> [[f64; 2]; 2] {
        [self.project(self.from), self.project(self.to)]
    }

    fn project(&self, q: [f64; 2]) -> [f64; 2] {
        let Some(c) = &self.chain else { return q };
        let origin = c.baseline.unwrap_or(self.from);
        // Preserve exact stations when projecting onto horizontal or vertical measurement lines.
        if c.axis[1] == 0.0 { return [q[0], origin[1]]; }
        if c.axis[0] == 0.0 { return [origin[0], q[1]]; }
        let t = (q[0]-origin[0])*c.axis[0] + (q[1]-origin[1])*c.axis[1];
        [origin[0]+t*c.axis[0], origin[1]+t*c.axis[1]]
    }

    fn axis(&self) -> [f64; 2] {
        self.chain.as_ref().map_or_else(|| {
            let length = self.length();
            [(self.to[0] - self.from[0]) / length, (self.to[1] - self.from[1]) / length]
        }, |c| c.axis)
    }

    fn station(&self, q: [f64; 2]) -> f64 {
        let u = self.axis();
        (q[0] - self.from[0]) * u[0] + (q[1] - self.from[1]) * u[1]
    }

    /// Actual references and their perpendicular projections onto the measurement axis.
    pub fn intervals(&self) -> impl Iterator<Item = ([[f64; 2]; 2], [[f64; 2]; 2])> + '_ {
        self.references().zip(self.references().skip(1)).map(|((a, _), (b, _))| {
            let actual = [a, b];
            let projected = actual.map(|q| self.project(q));
            (actual, projected)
        })
    }

    pub(crate) fn reference_mut(&mut self, index: usize) -> Option<(&mut [f64; 2], &mut Option<Anchor>)> {
        let count = self.chain.as_ref().map_or(0, |c| c.inner.len());
        if index == 0 { return Some((&mut self.from, &mut self.anchors[0])); }
        if index == count + 1 { return Some((&mut self.to, &mut self.anchors[1])); }
        let (at, anchor) = self.chain.as_mut()?.inner.get_mut(index - 1)?;
        Some((at, anchor))
    }

    pub(crate) fn order_references(&mut self) {
        let Some(chain) = &self.chain else { return };
        let axis = chain.axis;
        let mut refs = self.references().collect::<Vec<_>>();
        refs.sort_by(|a,b| self.station(a.0).total_cmp(&self.station(b.0)));
        self.set_references(&refs, axis);
    }

    fn set_references(&mut self, refs: &[([f64; 2], Option<Anchor>)], axis: [f64; 2]) {
        let baseline = self.chain.as_ref().and_then(|c| c.baseline)
            .or_else(|| (refs.len() > 2).then_some(self.from));
        (self.from, self.anchors[0]) = refs[0];
        (self.to, self.anchors[1]) = refs[refs.len() - 1];
        let offset = self.paper_offset();
        let end = self.to;
        let off_axis = ((end[0]-self.from[0])*axis[1] - (end[1]-self.from[1])*axis[0]).abs() > 1e-9;
        self.chain = (refs.len() > 2 || offset != 0.006 || off_axis || baseline.is_some()).then(|| DimensionChain { axis, offset, baseline, inner: refs[1..refs.len() - 1].to_vec() });
    }

    /// Its length (metres).
    pub fn length(&self) -> f64 {
        if self.chain.is_some() { self.station(self.to).abs() }
        else { (self.to[0] - self.from[0]).hypot(self.to[1] - self.from[1]) }
    }
}

impl PlanView {
    /// Recover explicit reference points and measurement-line positions from a view that
    /// also supplies ordinary two-point dimensions for readers without continuous dimensions.
    fn adopt_dimension_chains(&mut self) {
        let Some(records) = self.presentation["dimensionChains"].as_array() else { return };
        let mut replacements = std::collections::BTreeMap::new();
        let mut witnesses = Vec::new();
        for record in records {
            let Some(first) = record["firstDimension"].as_u64().and_then(|v| usize::try_from(v).ok()) else { continue };
            let Some(count) = record["segments"].as_u64().and_then(|v| usize::try_from(v).ok()) else { continue };
            let Some(end) = first.checked_add(count) else { continue };
            let Some(segments) = self.dimensions.get(first..end).filter(|s| !s.is_empty()) else { continue };
            let Some(axis @ 0..=1) = record["axis"].as_u64().map(|v| v as usize) else { continue };
            let Some(offset) = record["offset"].as_f64().filter(|v| v.is_finite()) else { continue };
            let Some(points) = record["references"].as_array().and_then(|rs| {
                rs.iter().map(|r| serde_json::from_value::<[f64; 2]>(r.clone()).ok()).collect::<Option<Vec<_>>>()
            }).filter(|p| p.len() == count + 1) else { continue };
            let cross = 1-axis;
            let mut u = [0.,0.];
            u[axis] = (points[count][axis]-points[0][axis]).signum();
            let station_matches = |a: f64,b: f64| (a-b).abs() < 1e-7;
            if !segments.iter().zip(points.windows(2)).all(|(d,w)| {
                d.chain.is_none() && d.style == segments[0].style && (w[1][axis]-w[0][axis])*u[axis] >= 0.001
                    && station_matches(d.from[axis],w[0][axis]) && station_matches(d.to[axis],w[1][axis])
                    && station_matches(d.from[cross],offset) && station_matches(d.to[cross],offset)
            }) { continue; }
            if replacements.range(..end).next_back().is_some_and(|(_, (prev_end,_))| *prev_end > first) { continue; }
            let mut baseline = points[0];
            baseline[cross] = offset;
            let mut refs = Vec::new();
            for (i,q) in points.iter().copied().enumerate() {
                let mut anchor = if i == count { segments[count-1].anchors[1] } else { segments[i].anchors[0] };
                // A fallback anchor belongs to the stored endpoint; an actual reference can
                // reuse it only when it names that same physical point.
                if !station_matches(q[cross], offset) { anchor = None; }
                refs.push((q,anchor));
            }
            let mut d = segments[0].clone();
            d.chain = Some(DimensionChain { axis: u, baseline: Some(baseline), offset: 0., inner: Vec::new() });
            d.set_references(&refs,u);
            witnesses.extend(points.iter().map(|q| (axis,*q,offset)));
            replacements.insert(first,(end,d));
        }
        if replacements.is_empty() { return; }
        let raw = std::mem::take(&mut self.dimensions);
        let mut skip = 0;
        for (i,d) in raw.into_iter().enumerate() {
            if i < skip { continue; }
            if let Some((end,chain)) = replacements.remove(&i) {
                self.dimensions.push(chain);
                skip = end;
            } else { self.dimensions.push(d); }
        }
        // An ungrouped, unstyled perpendicular crossing the very same tick on the
        // reference's side is fallback witness ink. Leave shape and pen index ranges intact.
        if self.shapes.is_empty() && self.presentation["pens"].is_null() {
            self.lines.retain(|[a,b]| {
                // A centreline through a measured point (e.g. the head of a nearby
                // pile) belongs to the drawing, even when another row crosses it.
                let middle = [(a[0]+b[0])/2., (a[1]+b[1])/2.];
                if witnesses.iter().any(|(_,q,_)| (middle[0]-q[0]).hypot(middle[1]-q[1]) < 1e-7) { return true; }
                !witnesses.iter().any(|&(axis,q,offset)| {
                let cross = 1-axis;
                let side = (q[cross]-offset).signum();
                let along = |p: [f64;2]| (p[cross]-offset)*side;
                (a[axis]-q[axis]).abs() < 1e-7 && (b[axis]-q[axis]).abs() < 1e-7
                    && along(*a).min(along(*b)) < 0. && along(*a).max(along(*b)) > 0.
                    && along(*a).max(along(*b)) < (q[cross]-offset).abs()
                })
            });
        }
        if let Some(obj) = self.presentation.as_object_mut() { obj.remove("dimensionChains"); }
    }
}

/// Adjacent legacy segments form one axis only when they touch, point in the same
/// direction and use the same style and shared object reference.
fn join_legacy_dimensions(dimensions: Vec<Dimension>) -> Vec<Dimension> {
    let mut out: Vec<Dimension> = Vec::new();
    for next in dimensions {
        let prev = out.last_mut();
        if let Some(d) = prev.filter(|d| d.to == next.from && d.style == next.style
                && d.anchors[1] == next.anchors[0] && next.chain.is_none()) {
            let axis = d.axis();
            let u = next.axis();
            if axis[0] * u[0] + axis[1] * u[1] > 0.999999999999 {
                let mut refs = d.references().collect::<Vec<_>>();
                refs.push((next.to, next.anchors[1]));
                d.set_references(&refs, axis);
                continue;
            }
        }
        out.push(next);
    }
    out
}

/// A text on a drawing, standing on a point of it (see [`Align`]; a floor plan's x, y, a
/// section's u, v; metres).
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub at: [f64; 2],
    pub text: String,
    /// The height of its letters on paper (metres): the view's scale makes it that much larger
    /// on the drawing.
    pub size: f64,
    pub align: Align,
    /// How its lines are turned about that point (radians, counter-clockwise).
    pub angle: f64,
    /// The typeface of its letters: the app's own unless its file names one.
    pub face: crate::typeface::Face,
    /// A level mark of a section or an elevation: its text is the height it stands at
    /// ([`level_text`]), kept so when it moves, and it is drawn with a triangle on that height.
    pub level: bool,
}

/// What a level mark says of height `z` (metres): "+2.700", and "+0.000" at zero.
pub fn level_text(z: f64) -> String {
    let z = if z.abs() < 0.0005 { 0.0 } else { z };
    format!("{z:+.3}")
}

/// The height of the letters of a new text on paper (metres).
pub const NOTE_HEIGHT: f64 = 0.0025;

/// A3 landscape, the paper of a new sheet (metres).
pub const A3_LANDSCAPE: [f64; 2] = [0.420, 0.297];
/// The ISO papers a sheet can have, landscape: name, width and height (metres).
pub const PAPERS: [(&str, [f64; 2]); 5] =
    [("A4", [0.297, 0.210]), ("A3", A3_LANDSCAPE), ("A2", [0.594, 0.420]), ("A1", [0.841, 0.594]), ("A0", [1.189, 0.841])];
/// A view's node with the kind of drawing it was given, when it was given one.
fn with_drawing(mut view: Value, drawing: &Option<String>) -> Value {
    if let Some(kind) = drawing {
        view["drawing"] = kind.clone().into();
    }
    view
}

/// The scales a placed view can have (1 : this).
pub const SCALES: [f64; 6] = [20.0, 50.0, 100.0, 200.0, 500.0, 1000.0];
/// The scale a view gets when it is placed on a sheet (1 : this).
pub const SHEET_SCALE: f64 = 100.0;

/// A sheet: paper with views placed on it.
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    /// The IfcX path, a UUID.
    pub path: String,
    pub number: String,
    fields: TitleFields,
    pub revisions: [SheetRevision; 6],
    pub name: String,
    /// Number and name, as a tab shows them.
    pub title: String,
    /// Width and height (metres).
    pub paper: [f64; 2],
    pub views: Vec<PlacedView>,
}

/// One dated change in the sheet revision column.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SheetRevision {
    pub date: String,
    pub description: String,
}

/// A view on a sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedView {
    /// The path of the floor plan.
    pub plan: String,
    /// Where its middle is on the paper (metres from the lower left corner).
    pub at: [f64; 2],
    /// 1 : `scale`.
    pub scale: f64,
    /// Overrides the view's default name and scale caption on this placement only.
    pub show_title: Option<bool>,
}

/// Why a level was not added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelError {
    EmptyName,
    /// Another level has this name (case does not count).
    DuplicateName,
    /// The elevation is not a finite number.
    BadElevation,
}

/// The project document.
#[derive(Debug, Clone, Default)]
pub struct Document {
    /// Highest issued pile number, including piles since removed.
    pub(crate) pile_high_water: u64,
    /// Low to high; equal elevations by name.
    levels: Vec<Level>,
    /// Floor plans, in the order they were made.
    plans: Vec<PlanView>,
    /// Sections and elevations, in the order they were made.
    sections: Vec<SectionView>,
    /// 3D views, in the order they were made.
    views_3d: Vec<View3d>,
    /// Sheets, in the order they were made.
    sheets: Vec<Sheet>,
    project_fields: TitleFields,
    pub project_address: String,
    /// How dimensions look; a dimension names one by its place here.
    dimension_styles: DimensionStyles,
    /// Levels and views made so far, for the paths of new ones.
    added: u64,
}

impl Document {
    /// The document of a model: its levels, recognised as IfcX levels.
    pub fn from_model(model: &Model) -> Self {
        let mut document = Self::default();
        document.recognise(model);
        document
    }

    /// The levels, low to high.
    pub fn levels(&self) -> &[Level] {
        &self.levels
    }

    /// Takes the levels of `model` (in place of those recognised before) and keeps the added
    /// ones, except an added level whose name the model now has.
    pub fn recognise(&mut self, model: &Model) {
        let mut levels: Vec<Level> = model
            .project
            .levels
            .iter()
            .filter(|(_, z)| z.is_finite())
            .map(|(name, z)| {
                let elevation = micrometres(*z as f64);
                // the path the file names the level by, so its elements and plans still find it
                let named = model.project.level_paths.iter().find(|(n, _)| n == name).map(|(_, path)| path.clone());
                Level { path: named.unwrap_or_else(|| uuid(&format!("level/{name}/{elevation}"))), name: name.clone(), elevation, added: false }
            })
            .collect();
        let taken = |name: &str, levels: &[Level]| levels.iter().any(|l| same_name(&l.name, name));
        let kept: Vec<Level> = self.levels.drain(..).filter(|l| l.added).collect();
        for level in kept {
            if !taken(&level.name, &levels) {
                levels.push(level);
            }
        }
        self.levels = levels;
        self.sort();
        self.drop_orphan_plans();
        self.adopt_ifcx_with(&model.project.drawings, &|view| crate::export::ifcx::hidden_in(model, view));
    }

    /// The plans, sections, elevations, 3D views and sheets as IfcX nodes in the `o3v::` namespace,
    /// under one root node; a plan names its level, so it finds it again in the file's storeys.
    /// The elements a view hides are left out (see [`Self::ifcx_drawing_nodes_with`]).
    pub fn ifcx_drawing_nodes(&self) -> Vec<Value> {
        self.ifcx_drawing_nodes_with(&|_| None)
    }

    /// [`Self::ifcx_drawing_nodes`], with what `hidden` makes of the elements each view hides: an
    /// attribute of its node (a key and its value), or nothing.
    pub fn ifcx_drawing_nodes_with(&self, hidden: &dyn Fn(&UnitKeySet) -> Option<(&'static str, Value)>) -> Vec<Value> {
        let with_hidden = |mut view: Value, set: &UnitKeySet| {
            if let Some((key, value)) = (!set.is_empty()).then(|| hidden(set)).flatten() {
                view[key] = value;
            }
            view
        };
        let rect = |c: Option<([f64; 2], [f64; 2])>| c.map_or(Value::Null, |(lo, hi)| json!([lo, hi]));
        // from, to, then its style's name (when not the standard one), then what its ends hold on to
        let dims = |d: &[Dimension]| {
            let one = |d: &Dimension| {
                let style = if d.style == 0 { Value::Null } else { json!(self.dimension_style(d.style).name) };
                if let Some(chain) = &d.chain {
                    let refs: Vec<_> = d.references().map(|(at, anchor)| json!([at, anchor.map(Anchor::to_ifcx)])).collect();
                    return json!({"references":refs, "axis":chain.axis, "baseline":chain.baseline, "offset":chain.offset, "style":style});
                }
                match (d.style, d.anchors) {
                    (0, [None, None]) => json!([d.from, d.to]),
                    (_, [None, None]) => json!([d.from, d.to, style]),
                    (_, [a, b]) => json!([d.from, d.to, style, a.map(Anchor::to_ifcx), b.map(Anchor::to_ifcx)]),
                }
            };
            d.iter().map(one).collect::<Vec<_>>()
        };
        let notes = |n: &[Note]| {
            let note = |n: &Note| {
                let mut note = json!({"at": n.at, "text": n.text, "size": n.size});
                if n.align != Align::Left {
                    note["align"] = n.align.key().into();
                }
                if n.angle != 0.0 {
                    note["angle"] = n.angle.into();
                }
                if n.level {
                    note["level"] = true.into();
                }
                if let Some(font) = n.face.to_json() {
                    note["font"] = font;
                }
                note
            };
            n.iter().map(note).collect::<Vec<_>>()
        };
        // the shapes of a view's lines, where it has any: kind, first line, how many
        let shaped = |mut view: Value, shapes: &[Shape]| -> Value {
            if !shapes.is_empty() {
                view["shapes"] = shapes.iter().map(|s| json!([s.kind.key(), s.first, s.count])).collect::<Vec<_>>().into();
            }
            view
        };
        let mut nodes = Vec::new();
        let mut children = serde_json::Map::new();
        for (i, p) in self.plans.iter().enumerate() {
            let level = self.level(&p.level).map(|l| l.name.clone()).unwrap_or_default();
            children.insert(format!("plan {}", i + 1), p.path.clone().into());
            let fills = |all: &[Fill]| all.iter().map(Fill::to_ifcx).collect::<Vec<_>>();
            let view = json!({
                "name": p.name, "kind": p.kind.key(), "level": level, "cut": p.cut, "depth": p.depth, "scale": p.scale, "crop": rect(p.crop), "dimension_format": 2, "dimensions": dims(&p.dimensions), "notes": notes(&p.notes), "lines": p.lines, "fills": fills(&p.fills), "layers": p.layers,
            });
            let mut view = shaped(view, &p.shapes);
            if !p.presentation.is_null() { view["presentation"] = p.presentation.clone(); }
            if p.pile_tag_marks { view["pile_tag_marks"] = true.into(); }
            if let Some(photo) = &p.orthophoto { photo.write(&mut view["presentation"]); }
            if p.pile_symbols { view["pile_symbols"] = true.into(); }
            if !p.pile_offsets.is_empty() { view["pile_offsets"] = json!(p.pile_offsets); }
            nodes.push(json!({"path": p.path, "attributes": {"o3v::view::plan": with_hidden(with_drawing(view, &p.drawing), &p.hidden)}}));
        }
        for (i, s) in self.sections.iter().enumerate() {
            children.insert(format!("section {}", i + 1), s.path.clone().into());
            let fills = |all: &[Fill]| all.iter().map(Fill::to_ifcx).collect::<Vec<_>>();
            let view = json!({
                "name": s.name, "kind": s.kind.key(), "from": s.from, "to": s.to, "depth": s.depth, "scale": s.scale, "crop": rect(s.crop), "dimensions": dims(&s.dimensions), "notes": notes(&s.notes), "lines": s.lines, "fills": fills(&s.fills), "layers": s.layers,
            });
            let mut view = shaped(view, &s.shapes);
            if !s.presentation.is_null() { view["presentation"] = s.presentation.clone(); }
            nodes.push(json!({"path": s.path, "attributes": {"o3v::view::section": with_hidden(with_drawing(view, &s.drawing), &s.hidden)}}));
        }
        for (i, v) in self.views_3d.iter().enumerate() {
            children.insert(format!("3d view {}", i + 1), v.path.clone().into());
            let c = &v.camera;
            let section = v.section.map_or(Value::Null, |b| json!({"min": b.min, "max": b.max, "turn": b.turn, "pivot": b.pivot}));
            let mut view = json!({
                "name": v.name, "camera": {"eye": c.eye, "target": c.target, "perspective": c.perspective, "fov": c.fov}, "box": section, "layers": v.layers,
            });
            if let Some(shown) = &v.analysis {
                view["analysis"] = shown.to_json();
            }
            nodes.push(json!({"path": v.path, "attributes": {"o3v::view::3d": with_hidden(view, &v.hidden)}}));
        }
        for (i, s) in self.sheets.iter().enumerate() {
            children.insert(format!("sheet {}", i + 1), s.path.clone().into());
            let views: Vec<Value> = s.views.iter().map(|v| {
                let mut placement = json!({"view": v.plan, "at": v.at, "scale": v.scale});
                if let Some(show) = v.show_title { placement["showTitle"] = show.into(); }
                placement
            }).collect();
            nodes.push(json!({"path": s.path, "attributes": {"o3v::sheet": {"number": s.number, "name": s.name, "fields": s.fields, "revisions": s.revisions, "paper": s.paper, "views": views}}}));
        }
        if !nodes.is_empty() || self.pile_high_water > 0 || !self.project_fields.is_empty() || !self.project_address.is_empty() {
            nodes.push(json!({"path": uuid("o3v/drawings"), "children": children, "attributes": {"o3v::drawings": {"version": 1, "pile_high_water": self.pile_high_water, "project_fields": self.project_fields, "project_address": self.project_address, "dimension_styles": self.dimension_styles.0.iter().map(DimensionStyle::to_ifcx).collect::<Vec<_>>()}}}));
        }
        nodes
    }

    /// Takes over the drawings an IFCX file holds (its `o3v::` nodes), except those it has
    /// already; a floor plan finds its level by name. The elements a view hides are not taken
    /// (see [`Self::adopt_ifcx_with`]).
    pub fn adopt_ifcx(&mut self, nodes: &[(String, Value)]) {
        self.adopt_ifcx_with(nodes, &|_| UnitKeySet::default());
    }

    /// [`Self::adopt_ifcx`], with `hidden` giving the elements a view hides from its attribute.
    pub fn adopt_ifcx_with(&mut self, nodes: &[(String, Value)], hidden: &dyn Fn(&Value) -> UnitKeySet) {
        let hidden = |v: &Value| Arc::new(hidden(v));
        for high in nodes.iter().filter_map(|(_, a)| a.get("o3v::drawings")?.get("pile_high_water")?.as_u64()) {
            self.pile_high_water = self.pile_high_water.max(high);
        }
        if self.project_address.is_empty() {
            if let Some(address) = nodes.iter().find_map(|(_, a)| a.get("o3v::drawings")?.get("project_address")?.as_str()) {
                self.project_address = address.to_owned();
            }
        }
        for fields in nodes.iter().filter_map(|(_, a)| a.get("o3v::drawings")?.get("project_fields")) {
            if let Ok(values) = serde_json::from_value::<TitleFields>(fields.clone()) {
                for (field, value) in values {
                    self.project_fields.entry(field).or_insert(value);
                }
            }
        }
        // the styles first: the dimensions name them
        for style in nodes.iter().filter_map(|(_, a)| a.get("o3v::drawings")?.get("dimension_styles")?.as_array()).flatten().filter_map(DimensionStyle::from_ifcx) {
            self.set_dimension_style(style);
        }
        let styles: Vec<String> = self.dimension_styles.0.iter().map(|s| s.name.clone()).collect();
        let style = |v: Option<&Value>| v.and_then(Value::as_str).and_then(|name| styles.iter().position(|s| s == name)).unwrap_or(0);
        let pair = |v: &Value| -> Option<[f64; 2]> { Some([v.get(0)?.as_f64()?, v.get(1)?.as_f64()?]) };
        let rect = |v: Option<&Value>| -> Option<([f64; 2], [f64; 2])> {
            let v = v?;
            Some((pair(v.get(0)?)?, pair(v.get(1)?)?))
        };
        let dims = |v: Option<&Value>| -> Vec<Dimension> {
            v.and_then(Value::as_array).into_iter().flatten().filter_map(|d| {
                if let Some(refs) = d.get("references").and_then(Value::as_array) {
                    let refs = refs.iter().map(|r| Some((pair(r.get(0)?)?, r.get(1).and_then(Anchor::from_ifcx)))).collect::<Option<Vec<_>>>()?;
                    if refs.len() < 2 { return None; }
                    let axis = pair(d.get("axis")?)?;
                    let length = axis[0].hypot(axis[1]);
                    if !length.is_finite() || length < 1e-12 { return None; }
                    let mut dimension = Dimension { from: refs[0].0, to: refs.last()?.0, style: style(d.get("style")), anchors: [None, None], chain: None };
                    let axis = axis.map(|x| x / length);
                    let offset = d.get("offset").and_then(Value::as_f64).filter(|x| x.is_finite()).unwrap_or(0.006);
                    dimension.chain = Some(DimensionChain { axis, baseline: d.get("baseline").and_then(pair), offset, inner: Vec::new() });
                    dimension.set_references(&refs, axis);
                    Some(dimension)
                } else {
                    Some(Dimension { from: pair(d.get(0)?)?, to: pair(d.get(1)?)?, style: style(d.get(2)), anchors: [d.get(3).and_then(Anchor::from_ifcx), d.get(4).and_then(Anchor::from_ifcx)], chain: None })
                }
            }).collect()
        };
        let notes = |v: Option<&Value>| -> Vec<Note> {
            let all = v.and_then(Value::as_array).into_iter().flatten();
            all.filter_map(|n| Some(Note { at: pair(n.get("at")?)?, text: n.get("text")?.as_str()?.to_string(), size: n.get("size").and_then(Value::as_f64).filter(|s| s.is_finite() && *s > 0.0).unwrap_or(NOTE_HEIGHT), align: n.get("align").and_then(Value::as_str).and_then(Align::from_key).unwrap_or_default(), angle: n.get("angle").and_then(Value::as_f64).filter(|a| a.is_finite()).unwrap_or(0.0), face: crate::typeface::Face::from_json(n.get("font")), level: n.get("level").and_then(Value::as_bool).unwrap_or(false) })).filter(|n| !n.text.trim().is_empty()).collect()
        };
        let lines = |v: Option<&Value>| -> Vec<[[f64; 2]; 2]> {
            let all = v.and_then(Value::as_array).into_iter().flatten();
            all.filter_map(|l| Some([pair(l.get(0)?)?, pair(l.get(1)?)?])).filter(|l| l.iter().flatten().all(|x| x.is_finite())).collect()
        };
        // the shapes of a view's lines: runs within them, one after the other; none where a line
        // of the file was left out, since the runs then count other lines
        let shapes = |view: &Value| -> Vec<Shape> {
            let written = view.get("lines").and_then(Value::as_array).map_or(0, Vec::len);
            if lines(view.get("lines")).len() != written {
                return Vec::new();
            }
            let all = view.get("shapes").and_then(Value::as_array).into_iter().flatten();
            let mut read: Vec<Shape> = all.filter_map(|s| Some(Shape { kind: ShapeKind::from_key(s.get(0)?.as_str()?)?, first: s.get(1)?.as_u64()? as usize, count: s.get(2)?.as_u64()? as usize })).collect();
            read.sort_by_key(|s| s.first);
            let mut end = 0;
            read.retain(|s| {
                let fits = s.count >= 1 && s.first >= end && s.first + s.count <= written;
                if fits {
                    end = s.first + s.count;
                }
                fits
            });
            read
        };
        let layers = |v: Option<&Value>| -> LayerSwitches {
            let all = v.and_then(Value::as_object).into_iter().flatten();
            all.filter_map(|(name, on)| Some((name.clone(), on.as_bool()?))).collect()
        };
        let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
        let number = |v: &Value, key: &str, or: f64| v.get(key).and_then(Value::as_f64).filter(|x| x.is_finite()).unwrap_or(or);
        let scale = |v: &Value| Some(number(v, "scale", SHEET_SCALE)).filter(|s| *s >= 1.0).unwrap_or(SHEET_SCALE);
        let triple = |v: Option<&Value>| -> Option<[f64; 3]> {
            let v = v?;
            let xyz = [v.get(0)?.as_f64()?, v.get(1)?.as_f64()?, v.get(2)?.as_f64()?];
            xyz.iter().all(|x| x.is_finite()).then_some(xyz)
        };
        let known = |doc: &Document, path: &str| doc.view_name(path).is_some() || doc.sheet(path).is_some();
        for (path, attributes) in nodes {
            if known(self, path) {
                continue;
            }
            if let Some(p) = attributes.get("o3v::view::plan") {
                let Some(level) = self.levels.iter().find(|l| same_name(&l.name, &text(p, "level"))).map(|l| l.path.clone()) else { continue };
                let kind = PlanKind::from_key(&text(p, "kind"));
                let (cut, depth) = self.plan_defaults(&level, kind);
                let mut plan = PlanView {
                    orthophoto: p.get("presentation").and_then(crate::raster::OrthoPhoto::read),
                    path: path.clone(),
                    name: text(p, "name"),
                    level,
                    kind,
                    cut: number(p, "cut", cut),
                    depth: number(p, "depth", depth),
                    scale: scale(p),
                    presentation: p["presentation"].clone(),
                    drawing: p.get("drawing").and_then(Value::as_str).map(String::from),
                    tags: Vec::new(),
                    pile_tag_marks: p.get("pile_tag_marks").and_then(Value::as_bool).unwrap_or(false),
                    pile_symbols: p.get("pile_symbols").and_then(Value::as_bool).unwrap_or(false),
                    pile_offsets: p.get("pile_offsets").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default(),
                    pile_tags: Vec::new(), corner_labels: false, office_symbol_lines: 0, symbol_fills: Vec::new(),
                    symbols: Vec::new(),
                    dimensions: dims(p.get("dimensions")),
                    notes: notes(p.get("notes")),
                    lines: lines(p.get("lines")),
                    shapes: shapes(p),
                    fills: Fill::from_ifcx(p.get("fills")),
                    crop: rect(p.get("crop")),
                    layers: layers(p.get("layers")),
                    hidden: hidden(p),
                };
                if p.get("dimension_format").is_none() {
                    if plan.presentation["dimensionChains"].is_array() {
                        plan.adopt_dimension_chains();
                    } else if plan.drawing.as_deref() == Some("pile-plan") {
                        plan.dimensions = join_legacy_dimensions(plan.dimensions);
                    }
                }
                plan.append_sounding_marks();
                self.plans.push(plan);
            } else if let Some(s) = attributes.get("o3v::view::section") {
                let (Some(from), Some(to)) = (s.get("from").and_then(pair), s.get("to").and_then(pair)) else { continue };
                self.sections.push(SectionView {
                    path: path.clone(),
                    name: text(s, "name"),
                    kind: SectionKind::from_key(&text(s, "kind")),
                    from,
                    to,
                    depth: number(s, "depth", 100.0),
                    scale: scale(s),
                    presentation: s["presentation"].clone(),
                    drawing: s.get("drawing").and_then(Value::as_str).map(String::from),
                    dimensions: dims(s.get("dimensions")),
                    notes: notes(s.get("notes")),
                    lines: lines(s.get("lines")),
                    shapes: shapes(s),
                    fills: Fill::from_ifcx(s.get("fills")),
                    crop: rect(s.get("crop")),
                    layers: layers(s.get("layers")),
                    hidden: hidden(s),
                });
            } else if let Some(v) = attributes.get("o3v::view::3d") {
                let c = v.get("camera");
                let (Some(eye), Some(target)) = (triple(c.and_then(|c| c.get("eye"))), triple(c.and_then(|c| c.get("target")))) else { continue };
                let perspective = c.and_then(|c| c.get("perspective")).and_then(Value::as_bool).unwrap_or(true);
                let fov = c.map_or(VIEW_3D_FOV, |c| number(c, "fov", VIEW_3D_FOV)).clamp(1.0, 179.0);
                let section = v.get("box").and_then(|b| {
                    let (min, max, pivot) = (triple(b.get("min"))?, triple(b.get("max"))?, triple(b.get("pivot"))?);
                    (0..3).all(|i| min[i] < max[i]).then_some(ViewBox { min, max, turn: number(b, "turn", 0.0), pivot })
                });
                self.views_3d.push(View3d {
                    path: path.clone(),
                    name: text(v, "name"),
                    camera: Camera3d { eye, target, perspective, fov },
                    section,
                    layers: layers(v.get("layers")),
                    hidden: hidden(v),
                    analysis: v.get("analysis").and_then(crate::structural::diagrams::Shown::from_json),
                });
            }
        }
        // sheets last: their views are known by then
        for (path, attributes) in nodes {
            let Some(s) = attributes.get("o3v::sheet") else { continue };
            if known(self, path) {
                continue;
            }
            let views = s.get("views").and_then(Value::as_array).into_iter().flatten().filter_map(|v| {
                let view = v.get("view")?.as_str()?.to_string();
                (self.plan(&view).is_some() || self.section(&view).is_some()).then_some(())?;
                Some(PlacedView { show_title: v.get("showTitle").or_else(|| v.get("show_title")).and_then(Value::as_bool), plan: view, at: pair(v.get("at")?)?, scale: v.get("scale").and_then(Value::as_f64).filter(|s| s.is_finite() && *s > 0.0).unwrap_or(SHEET_SCALE) })
            });
            let views: Vec<PlacedView> = views.collect();
            let (number, name) = (text(s, "number"), text(s, "name"));
            let paper = s.get("paper").and_then(pair).unwrap_or(A3_LANDSCAPE);
            self.sheets.push(Sheet { path: path.clone(), title: format!("{number} {name}"), number, name, revisions: s.get("revisions").and_then(Value::as_array).map(|rows| std::array::from_fn(|i| rows.get(i).and_then(|r| serde_json::from_value(r.clone()).ok()).unwrap_or_default())).unwrap_or_default(), fields: s.get("fields").and_then(|f| serde_json::from_value(f.clone()).ok()).unwrap_or_default(), paper, views });
        }
    }

    /// Adds a level and gives its index in [`Self::levels`].
    pub fn add_level(&mut self, name: &str, elevation: f64) -> Result<usize, LevelError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(LevelError::EmptyName);
        }
        if self.levels.iter().any(|l| same_name(&l.name, name)) {
            return Err(LevelError::DuplicateName);
        }
        if !elevation.is_finite() {
            return Err(LevelError::BadElevation);
        }
        self.added += 1;
        let path = uuid(&format!("added/{}/{name}", self.added));
        self.levels.push(Level { path: path.clone(), name: name.to_string(), elevation: micrometres(elevation), added: true });
        self.sort();
        Ok(self.levels.iter().position(|l| l.path == path).unwrap_or_default())
    }

    /// Puts the levels back as they were at a step of the history, low to high.
    pub fn set_levels(&mut self, levels: Vec<Level>) {
        self.levels = levels;
        self.sort();
        self.drop_orphan_plans();
    }

    /// Renames the level at `path` and puts it at `elevation`; a path that is no level changes
    /// nothing.
    pub fn set_level(&mut self, path: &str, name: &str, elevation: f64) -> Result<(), LevelError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(LevelError::EmptyName);
        }
        if self.levels.iter().any(|l| l.path != path && same_name(&l.name, name)) {
            return Err(LevelError::DuplicateName);
        }
        if !elevation.is_finite() {
            return Err(LevelError::BadElevation);
        }
        if let Some(level) = self.levels.iter_mut().find(|l| l.path == path) {
            (level.name, level.elevation) = (name.to_string(), micrometres(elevation));
        }
        self.sort();
        Ok(())
    }

    /// Removes the added level at `path`; a level of the model stays (false).
    pub fn remove_level(&mut self, path: &str) -> bool {
        let before = self.levels.len();
        self.levels.retain(|l| !(l.added && l.path == path));
        self.drop_orphan_plans();
        self.levels.len() < before
    }

    /// The level at `path`.
    pub fn level(&self, path: &str) -> Option<&Level> {
        self.levels.iter().find(|l| l.path == path)
    }

    /// The floor plans, in the order they were made.
    pub fn plans(&self) -> &[PlanView] {
        &self.plans
    }

    /// The floor plan at `path`.
    pub fn plan(&self, path: &str) -> Option<&PlanView> {
        self.plans.iter().find(|p| p.path == path)
    }

    /// Makes a floor plan of the level at `path`, named after the level (with a number when the
    /// level has one already), and gives its path.
    pub fn add_plan(&mut self, level: &str) -> Option<String> {
        self.add_plan_of(level, PlanKind::Floor, "")
    }

    /// Makes a plan of `kind` of the level at `path` with that kind's cut and depth, and gives its
    /// path. A floor plan is named after the level, another kind `<level> (<word>)`; a number
    /// follows when that name is taken.
    pub fn add_plan_of(&mut self, level: &str, kind: PlanKind, word: &str) -> Option<String> {
        let level_name = self.level(level)?.name.clone();
        let base = if kind == PlanKind::Floor || word.trim().is_empty() { level_name } else { format!("{level_name} ({})", word.trim()) };
        let (cut, depth) = self.plan_defaults(level, kind);
        let mut name = base.clone();
        let mut n = 1;
        while self.plans.iter().any(|p| same_name(&p.name, &name)) {
            n += 1;
            name = format!("{base} ({n})");
        }
        self.added += 1;
        let path = uuid(&format!("plan/{}/{level}", self.added));
        self.plans.push(PlanView { orthophoto: None, path: path.clone(), name, level: level.to_string(), kind, cut, depth, scale: SHEET_SCALE, drawing: None, presentation: Value::Null, dimensions: Vec::new(), notes: Vec::new(), lines: Vec::new(), shapes: Vec::new(), tags: Vec::new(), pile_symbols: kind.structural(), pile_tag_marks: false, pile_offsets: Default::default(), pile_tags: Vec::new(), corner_labels: false, office_symbol_lines: 0, symbol_fills: Vec::new(), symbols: Vec::new(), fills: Vec::new(), crop: None, layers: LayerSwitches::new(), hidden: Arc::default() });
        Some(path)
    }

    /// The cut and depth a new plan of `kind` of the level at `level` gets: a ceiling plan looks
    /// up from its cut to the next level, else [`CEILING_DEPTH`].
    fn plan_defaults(&self, level: &str, kind: PlanKind) -> (f64, f64) {
        match kind {
            PlanKind::Floor => (PLAN_CUT, PLAN_DEPTH),
            PlanKind::Structural => (PLAN_CUT, STRUCTURAL_DEPTH),
            // cut as far under the level as a structural plan looks, up to the level
            PlanKind::StructuralUp => (-STRUCTURAL_DEPTH, STRUCTURAL_DEPTH),
            PlanKind::Ceiling => {
                let z = self.level(level).map_or(0.0, |l| l.elevation);
                let next = self.levels.iter().map(|l| l.elevation).filter(|&e| e > z + CEILING_CUT + 0.05).fold(f64::INFINITY, f64::min);
                (CEILING_CUT, if next.is_finite() { micrometres(next - z - CEILING_CUT) } else { CEILING_DEPTH })
            }
        }
    }

    /// Removes the floor plan at `path`, and it from the sheets.
    pub fn remove_plan(&mut self, path: &str) -> bool {
        let before = self.plans.len();
        self.plans.retain(|p| p.path != path);
        self.drop_orphan_plans();
        self.plans.len() < before
    }

    /// The sections and elevations, in the order they were made.
    pub fn sections(&self) -> &[SectionView] {
        &self.sections
    }

    /// The section or elevation at `path`.
    pub fn section(&self, path: &str) -> Option<&SectionView> {
        self.sections.iter().find(|s| s.path == path)
    }

    /// Makes a section along `from` → `to` (at least 10 cm long), looking `depth` beyond it, and
    /// gives its path.
    pub fn add_section(&mut self, name: &str, from: [f64; 2], to: [f64; 2], depth: f64) -> Option<String> {
        self.add_section_of(SectionKind::Section, name, from, to, depth)
    }

    /// Makes a section or elevation along `from` → `to` (at least 10 cm long), looking `depth`
    /// beyond it, and gives its path.
    pub fn add_section_of(&mut self, kind: SectionKind, name: &str, from: [f64; 2], to: [f64; 2], depth: f64) -> Option<String> {
        let finite = from.iter().chain(&to).chain([&depth]).all(|v| v.is_finite());
        if !finite || (to[0] - from[0]).hypot(to[1] - from[1]) < 0.1 || depth <= 0.0 {
            return None;
        }
        self.added += 1;
        let path = uuid(&format!("section/{}/{name}", self.added));
        self.sections.push(SectionView { path: path.clone(), name: name.to_string(), kind, from, to, depth, scale: SHEET_SCALE, drawing: None, presentation: Value::Null, dimensions: Vec::new(), notes: Vec::new(), lines: Vec::new(), shapes: Vec::new(), fills: Vec::new(), crop: None, layers: LayerSwitches::new(), hidden: Arc::default() });
        Some(path)
    }

    /// The next free section name: `<word> A`, `<word> B`, ..
    pub fn next_section_name(&self, word: &str) -> String {
        let taken = |name: &str| self.sections.iter().any(|s| same_name(&s.name, name));
        (b'A'..=b'Z').map(|c| format!("{word} {}", c as char)).find(|n| !taken(n)).unwrap_or_else(|| format!("{word} {}", self.sections.len() + 1))
    }

    /// Removes a section or elevation, and it from the sheets.
    pub fn remove_section(&mut self, path: &str) -> bool {
        let before = self.sections.len();
        self.sections.retain(|s| s.path != path);
        self.drop_orphan_plans();
        self.sections.len() < before
    }

    /// The 3D views, in the order they were made.
    pub fn views_3d(&self) -> &[View3d] {
        &self.views_3d
    }

    /// The 3D view at `path`.
    pub fn view_3d(&self, path: &str) -> Option<&View3d> {
        self.views_3d.iter().find(|v| v.path == path)
    }

    /// Makes a 3D view named `name` (with a number when the name is taken) that looks as `camera`
    /// within `section`, showing `layers` otherwise than the model and hiding `hidden`, and gives
    /// its path.
    pub fn add_view_3d(&mut self, name: &str, camera: Camera3d, section: Option<ViewBox>, layers: LayerSwitches, hidden: Arc<UnitKeySet>) -> String {
        let base = match name.trim() {
            "" => "3D".to_string(),
            name => name.to_string(),
        };
        let mut name = base.clone();
        let mut n = 1;
        while self.view_name_taken(&name, "") {
            n += 1;
            name = format!("{base} ({n})");
        }
        self.added += 1;
        let path = uuid(&format!("view3d/{}/{name}", self.added));
        self.views_3d.push(View3d { path: path.clone(), name, camera, section, layers, hidden, analysis: None });
        path
    }

    /// The next free name of a 3D view: `<word> 1`, `<word> 2`, ..
    pub fn next_view_3d_name(&self, word: &str) -> String {
        (1..).map(|n| format!("{word} {n}")).find(|name| !self.view_name_taken(name, "")).unwrap_or_default()
    }

    /// Lets a 3D view look as `camera`, within `section`; false when there is no such view or
    /// nothing changes.
    pub fn set_view_3d_camera(&mut self, path: &str, camera: Camera3d, section: Option<ViewBox>) -> bool {
        let Some(view) = self.views_3d.iter_mut().find(|v| v.path == path) else { return false };
        if view.camera == camera && view.section == section {
            return false;
        }
        (view.camera, view.section) = (camera, section);
        true
    }

    /// Lets a 3D view show `analysis` of the loads and results (none: it has no say in them);
    /// false when there is no such view or nothing changes.
    pub fn set_view_3d_analysis(&mut self, path: &str, analysis: Option<crate::structural::diagrams::Shown>) -> bool {
        let Some(view) = self.views_3d.iter_mut().find(|v| v.path == path) else { return false };
        if view.analysis == analysis {
            return false;
        }
        view.analysis = analysis;
        true
    }

    /// Removes the 3D view at `path`.
    pub fn remove_view_3d(&mut self, path: &str) -> bool {
        let before = self.views_3d.len();
        self.views_3d.retain(|v| v.path != path);
        self.views_3d.len() < before
    }

    /// Whether a view other than the one at `path` has the name `name` (case does not count).
    fn view_name_taken(&self, name: &str, path: &str) -> bool {
        let plans = self.plans.iter().map(|p| (&p.path, &p.name));
        let sections = self.sections.iter().map(|s| (&s.path, &s.name));
        let views_3d = self.views_3d.iter().map(|v| (&v.path, &v.name));
        plans.chain(sections).chain(views_3d).any(|(p, n)| p != path && same_name(n, name))
    }

    /// The name, own layers and hidden elements of a view (a plan, section, elevation or 3D
    /// view).
    fn own(&self, path: &str) -> Option<(&str, &LayerSwitches, &Arc<UnitKeySet>)> {
        if let Some(p) = self.plan(path) {
            return Some((&p.name, &p.layers, &p.hidden));
        }
        if let Some(s) = self.section(path) {
            return Some((&s.name, &s.layers, &s.hidden));
        }
        self.view_3d(path).map(|v| (v.name.as_str(), &v.layers, &v.hidden))
    }

    /// [`Self::own`], to change.
    fn own_mut(&mut self, path: &str) -> Option<(&mut String, &mut LayerSwitches, &mut Arc<UnitKeySet>)> {
        if let Some(p) = self.plans.iter_mut().find(|p| p.path == path) {
            return Some((&mut p.name, &mut p.layers, &mut p.hidden));
        }
        if let Some(s) = self.sections.iter_mut().find(|s| s.path == path) {
            return Some((&mut s.name, &mut s.layers, &mut s.hidden));
        }
        self.views_3d.iter_mut().find(|v| v.path == path).map(|v| (&mut v.name, &mut v.layers, &mut v.hidden))
    }

    /// The name of a view (a plan, section, elevation or 3D view).
    pub fn view_name(&self, path: &str) -> Option<&str> {
        self.own(path).map(|(name, _, _)| name)
    }

    /// The scale a view's texts and dimensions are sized for (1 : this).
    pub fn view_scale(&self, path: &str) -> Option<f64> {
        self.plan(path).map(|p| p.scale).or_else(|| self.section(path).map(|s| s.scale))
    }

    /// Sets the scale a view's texts and dimensions are sized for (1 : `scale`).
    pub fn set_view_scale(&mut self, path: &str, scale: f64) -> bool {
        if !(scale.is_finite() && scale >= 1.0) {
            return false;
        }
        if let Some(p) = self.plans.iter_mut().find(|p| p.path == path) {
            p.scale = scale;
            return true;
        }
        match self.sections.iter_mut().find(|s| s.path == path) {
            Some(s) => {
                s.scale = scale;
                true
            }
            None => false,
        }
    }

    /// The kind of drawing a view was given: a key of the project's drawing standard.
    pub fn view_drawing(&self, path: &str) -> Option<&str> {
        self.plan(path).map(|p| &p.drawing).or_else(|| self.section(path).map(|s| &s.drawing))?.as_deref()
    }

    /// Updates the authored values of the selected legend or group.
    pub fn set_pile_legend_field(&mut self, path: &str, field: &PileLegendField, value: String) -> bool {
        let Some(plan) = self.plans.iter_mut().find(|p| p.path == path && p.drawing.as_deref() == Some("pile-schedule")) else { return false };
        let legend = &mut plan.presentation["legend"];
        match field {
            PileLegendField::Report => legend["report"] = value.into(),
            PileLegendField::Advice => legend["advice"] = value.into(),
            PileLegendField::Execution => legend["execution"] = value.into(),
            PileLegendField::Datum => legend["datum"] = value.into(),
            PileLegendField::Cutoff(key) | PileLegendField::Foot(key) | PileLegendField::Uncut(key) => {
                if !legend["rows"].as_array().into_iter().flatten().any(|r| r["key"].as_str() == Some(key.as_str())) { return false; }
                let property = match field { PileLegendField::Foot(_) => "foot", PileLegendField::Uncut(_) => "uncut", _ => "cutoffs" };
                legend[property][key.as_str()] = value.into();
            }
        }
        true
    }

    /// Sets the drawing standard used by the view.
    pub fn set_view_drawing(&mut self, path: &str, kind: Option<String>) -> bool {
        if let Some(p) = self.plans.iter_mut().find(|p| p.path == path) {
            p.drawing = kind;
            return true;
        }
        match self.sections.iter_mut().find(|s| s.path == path) {
            Some(s) => {
                s.drawing = kind;
                true
            }
            None => false,
        }
    }

    /// The dimensions and the crop of a view (floor plan, section or elevation).
    pub fn view_marks(&self, path: &str) -> Option<(&[Dimension], Option<([f64; 2], [f64; 2])>)> {
        if let Some(p) = self.plan(path) {
            return Some((&p.dimensions, p.crop));
        }
        self.section(path).map(|s| (&s.dimensions[..], s.crop))
    }

    /// The dimensions and crop of a view, to change.
    fn marks_mut(&mut self, path: &str) -> Option<(&mut Vec<Dimension>, &mut Option<([f64; 2], [f64; 2])>)> {
        if let Some(p) = self.plans.iter_mut().find(|p| p.path == path) {
            return Some((&mut p.dimensions, &mut p.crop));
        }
        self.sections.iter_mut().find(|s| s.path == path).map(|s| (&mut s.dimensions, &mut s.crop))
    }

    /// Adds a dimension between two points to a view's drawing; two points less than a
    /// millimetre apart, or not finite, measure nothing.
    pub fn add_dimension(&mut self, view: &str, from: [f64; 2], to: [f64; 2]) -> bool {
        let dimension = Dimension { from, to, style: 0, anchors: [None, None], chain: None };
        if !(from.iter().chain(&to).all(|v| v.is_finite()) && dimension.length() >= 0.001) {
            return false;
        }
        let Some((dimensions, _)) = self.marks_mut(view) else { return false };
        dimensions.push(dimension);
        true
    }

    /// Adds an ordered chain, with one measured interval per pair of adjacent references.
    pub fn add_dimension_chain(&mut self, view: &str, points: &[[f64; 2]]) -> bool {
        let (Some(&from), Some(&to)) = (points.first(), points.last()) else { return false };
        if points.len() < 2 || points.iter().flatten().any(|x| !x.is_finite()) { return false; }
        let mut d = Dimension { from, to, style: 0, anchors: [None, None], chain: None };
        if d.length() < 0.001 || points.windows(2).any(|p| d.station(p[1]) - d.station(p[0]) < 0.001) { return false; }
        let refs = points.iter().map(|&q| (q, None)).collect::<Vec<_>>();
        d.set_references(&refs, d.axis());
        let Some((dims, _)) = self.marks_mut(view) else { return false };
        dims.push(d);
        true
    }

    pub fn insert_dimension_reference(&mut self, view: &str, index: usize, at: [f64; 2], anchor: Option<Anchor>) -> bool {
        let Some(d) = self.dimension_mut(view, index) else { return false };
        let pos = d.station(at);
        if !pos.is_finite() || d.references().any(|(q, _)| (d.station(q) - pos).abs() < 0.001) { return false; }
        let mut refs = d.references().collect::<Vec<_>>();
        refs.push((at, anchor));
        refs.sort_by(|a,b| d.station(a.0).total_cmp(&d.station(b.0)));
        d.set_references(&refs, d.axis());
        true
    }

    /// An independent overall dimension of the two outside references, one row further out.
    pub fn add_dimension_chain_total(&mut self, view: &str, index: usize) -> bool {
        let scale = self.view_scale(view).unwrap_or(SHEET_SCALE);
        let Some((dims,_)) = self.marks_mut(view) else { return false };
        let Some(d) = dims.get(index) else { return false };
        if d.references().count() < 3 { return false; }
        let axis = d.axis();
        let line = d.measured_ends()[0];
        let normal = [-axis[1],axis[0]];
        let end = [(d.from[0]+d.to[0])/2., (d.from[1]+d.to[1])/2.];
        let signed = (line[0]-end[0])*normal[0] + (line[1]-end[1])*normal[1] + d.paper_offset()*scale;
        let offset = d.paper_offset() + if signed < 0. { -0.008 } else { 0.008 };
        if dims.iter().any(|other| other.from == d.from && other.to == d.to && other.paper_offset() == offset) {
            return false;
        }
        let total = Dimension { from: d.from, to: d.to, style: d.style, anchors: d.anchors,
            chain: Some(DimensionChain { axis, baseline: d.chain.as_ref().and_then(|c| c.baseline), offset, inner: Vec::new() }) };
        dims.push(total);
        true
    }

    pub fn remove_dimension_reference(&mut self, view: &str, index: usize, reference: usize) -> bool {
        let Some(d) = self.dimension_mut(view, index) else { return false };
        let mut refs = d.references().collect::<Vec<_>>();
        if refs.len() <= 2 || reference >= refs.len() { return false; }
        refs.remove(reference);
        d.set_references(&refs, d.axis());
        true
    }

    /// The document's dimension styles, the standard one first.
    pub fn dimension_styles(&self) -> &[DimensionStyle] {
        &self.dimension_styles.0
    }

    /// The style a dimension names (the standard one when there is none such).
    pub fn dimension_style(&self, index: usize) -> &DimensionStyle {
        self.dimension_styles.0.get(index).unwrap_or(&self.dimension_styles.0[0])
    }

    /// Takes a style: in place of the one of its name, else as a new one. Gives its place.
    pub fn set_dimension_style(&mut self, style: DimensionStyle) -> usize {
        match self.dimension_styles.0.iter().position(|s| s.name == style.name) {
            Some(k) => {
                self.dimension_styles.0[k] = style;
                k
            }
            None => {
                self.dimension_styles.0.push(style);
                self.dimension_styles.0.len() - 1
            }
        }
    }

    /// Changes a style where it is; false when there is none such.
    pub fn edit_dimension_style(&mut self, index: usize, edit: impl FnOnce(&mut DimensionStyle)) -> bool {
        let Some(style) = self.dimension_styles.0.get_mut(index) else { return false };
        let name = style.name.clone();
        edit(style);
        style.name = name;
        true
    }

    /// Gives a view's dimension another of the document's styles.
    pub fn set_dimension_style_of(&mut self, view: &str, index: usize, style: usize) -> bool {
        if style >= self.dimension_styles.0.len() {
            return false;
        }
        let Some(d) = self.marks_mut(view).and_then(|(d, _)| d.get_mut(index)) else { return false };
        d.style = style;
        true
    }

    /// Puts a view's dimension on the other side of what it measures: its line stands to the
    /// left of from → to, so the two swap.
    pub fn flip_dimension(&mut self, view: &str, index: usize) -> bool {
        let Some(d) = self.marks_mut(view).and_then(|(d, _)| d.get_mut(index)) else { return false };
        std::mem::swap(&mut d.from, &mut d.to);
        d.anchors.swap(0, 1);
        if let Some(c) = &mut d.chain {
            c.inner.reverse();
            if let Some(b) = &mut c.baseline {
                let n = [-c.axis[1], c.axis[0]];
                let middle = [(d.from[0]+d.to[0])/2., (d.from[1]+d.to[1])/2.];
                let distance = (b[0]-middle[0])*n[0] + (b[1]-middle[1])*n[1];
                *b = [b[0]-2.*distance*n[0], b[1]-2.*distance*n[1]];
            }
            c.axis = c.axis.map(|x| -x);
        }
        true
    }

    /// Dimension `index` of a view, to change.
    pub fn dimension_mut(&mut self, view: &str, index: usize) -> Option<&mut Dimension> {
        self.marks_mut(view)?.0.get_mut(index)
    }

    /// Every dimension of the floor plans, to change: their points are the model's x and y.
    pub fn plan_dimensions_mut(&mut self) -> impl Iterator<Item = &mut Dimension> {
        self.plans.iter_mut().flat_map(|p| p.dimensions.iter_mut())
    }

    /// Removes a dimension of a view.
    pub fn remove_dimension(&mut self, view: &str, index: usize) -> bool {
        let Some((dimensions, _)) = self.marks_mut(view) else { return false };
        if index >= dimensions.len() {
            return false;
        }
        dimensions.remove(index);
        true
    }

    /// A view's own layers (a plan's, a section's, an elevation's or a 3D view's).
    pub fn view_layers(&self, path: &str) -> Option<&LayerSwitches> {
        self.own(path).map(|(_, layers, _)| layers)
    }

    /// Gives a view its own layers; false when there is no such view or nothing changes.
    pub fn set_view_layers(&mut self, path: &str, layers: LayerSwitches) -> bool {
        let Some((_, own, _)) = self.own_mut(path) else { return false };
        if *own == layers {
            return false;
        }
        *own = layers;
        true
    }

    /// The elements a view hides (a plan's, a section's, an elevation's or a 3D view's).
    pub fn view_hidden(&self, path: &str) -> Option<&Arc<UnitKeySet>> {
        self.own(path).map(|(_, _, hidden)| hidden)
    }

    /// Gives a view the elements it hides; false when there is no such view or nothing changes.
    pub fn set_view_hidden(&mut self, path: &str, hidden: Arc<UnitKeySet>) -> bool {
        let Some((_, _, own)) = self.own_mut(path) else { return false };
        if *own == hidden {
            return false;
        }
        *own = hidden;
        true
    }

    /// The texts of a view.
    pub fn view_notes(&self, path: &str) -> Option<&[Note]> {
        if let Some(p) = self.plan(path) {
            return Some(&p.notes);
        }
        self.section(path).map(|s| &s.notes[..])
    }

    /// What a floor plan says and draws by itself (see [`PlanView::tags`]); nothing for a section.
    pub fn view_tags(&self, path: &str) -> &[Note] {
        self.plan(path).map_or(&[], |p| &p.tags[..])
    }

    pub fn view_symbols(&self, path: &str) -> &[[[f64; 2]; 2]] {
        self.plan(path).map_or(&[], |p| &p.symbols[..])
    }

    /// Preserve old view switches when registered columns acquire their own pile layer.
    pub fn inherit_pile_visibility(&mut self) {
        for layers in self.plans.iter_mut().map(|p| &mut p.layers).chain(self.sections.iter_mut().map(|s| &mut s.layers)) {
            if let Some(shown) = layers.get("IfcColumn").copied() {
                layers.entry("IfcPile".into()).or_insert(shown);
            }
        }
    }

    /// Gives every floor plan what it says and draws by itself: `of` makes them for a plan.
    pub fn set_plan_tags(&mut self, of: impl Fn(&mut PlanView) -> (Vec<Note>, Vec<[[f64; 2]; 2]>)) {
        for p in &mut self.plans {
            p.pile_tags.clear();
            p.symbol_fills.clear();
            p.corner_labels = false;
            p.office_symbol_lines = 0;
            (p.tags, p.symbols) = of(p);
            p.append_sounding_marks();
        }
    }

    /// The regions of a view filled in one colour that are no model, drawn under its lines.
    pub fn view_fills(&self, path: &str) -> impl Iterator<Item = &Fill> {
        let plan = self.plan(path);
        let authored = plan.map(|p| p.fills.as_slice()).or_else(|| self.section(path).map(|s| s.fills.as_slice())).unwrap_or_default();
        authored.iter().chain(plan.into_iter().flat_map(|p| &p.symbol_fills))
    }

    pub fn view_presentation(&self, path: &str) -> &Value {
        self.plan(path).map(|p| &p.presentation).or_else(|| self.section(path).map(|s| &s.presentation)).unwrap_or(&Value::Null)
    }

    /// Pile layouts and their schedules supply their own annotation on paper.
    pub(crate) fn view_has_sheet_caption(&self, path: &str) -> bool {
        self.view_presentation(path)["sheet_caption"].as_bool().unwrap_or_else(|| {
            !matches!(self.view_drawing(path), Some("pile-plan" | "pile-schedule"))
                && !self.view_presentation(path)["legend"].is_object()
        })
    }

    pub(crate) fn placed_view_has_title(&self, view: &PlacedView) -> bool {
        view.show_title.unwrap_or_else(|| self.view_has_sheet_caption(&view.plan))
    }

    pub fn set_placed_title(&mut self, sheet: &str, index: usize, show: Option<bool>) {
        if let Some(v) = self.sheets.iter_mut().find(|s| s.path == sheet).and_then(|s| s.views.get_mut(index)) {
            v.show_title = show;
        }
    }

    pub(crate) fn view_is_schedule(&self, path: &str) -> bool {
        self.view_drawing(path) == Some("pile-schedule") || self.view_presentation(path)["legend"].is_object()
    }

    /// Consecutive detail lines with a common colour and width in millimetres.
    pub fn detail_pens(&self, path: &str) -> Vec<(std::ops::Range<usize>, [u8; 3], f64)> {
        let count = self.view_lines(path).len();
        let mut starts: Vec<_> = self.view_presentation(path)["pens"].as_array().into_iter().flatten().filter_map(|p| {
            let at = usize::try_from(p.get(0)?.as_u64()?).ok()?;
            let rgb: [u8; 3] = serde_json::from_value(p.get(1)?.clone()).ok()?;
            let width = p.get(2)?.as_f64()?;
            (at < count && width.is_finite() && width > 0.0).then_some((at, rgb, width))
        }).collect();
        starts.sort_by_key(|p| p.0);
        let mut pens: Vec<_> = starts.iter().enumerate().map(|(i, &(at, rgb, width))| (at..starts.get(i+1).map_or(count, |p| p.0), rgb, width)).collect();
        if let Some(plan) = self.plan(path).filter(|p| p.office_symbol_lines > 0) {
            pens.push((count..count + plan.office_symbol_lines, [0,0,0], 0.18));
        }
        pens
    }

    /// The lines of a view that are no model: the shapes and leaders of its tags.
    pub fn view_lines(&self, path: &str) -> &[[[f64; 2]; 2]] {
        if let Some(p) = self.plan(path) {
            return &p.lines;
        }
        self.section(path).map_or(&[], |s| &s.lines[..])
    }

    /// The shapes drawn on a view: the runs of its lines that are one thing each.
    pub fn view_shapes(&self, path: &str) -> &[Shape] {
        if let Some(p) = self.plan(path) {
            return &p.shapes;
        }
        self.section(path).map_or(&[], |s| &s.shapes[..])
    }

    /// A view's lines and the shapes among them.
    fn drawn_mut(&mut self, path: &str) -> Option<(&mut Vec<[[f64; 2]; 2]>, &mut Vec<Shape>)> {
        if let Some(p) = self.plans.iter_mut().find(|p| p.path == path) {
            return Some((&mut p.lines, &mut p.shapes));
        }
        self.sections.iter_mut().find(|s| s.path == path).map(|s| (&mut s.lines, &mut s.shapes))
    }

    /// Adds lines to a view's drawing (its own points, metres), each a line of its own; one
    /// without length or with a point that is not finite is left out. How many were added.
    pub fn add_view_lines(&mut self, view: &str, lines: &[[[f64; 2]; 2]]) -> usize {
        let Some((own, _)) = self.drawn_mut(view) else { return 0 };
        let before = own.len();
        own.extend(lines.iter().filter(|l| l.iter().flatten().all(|v| v.is_finite()) && l[0] != l[1]));
        own.len() - before
    }

    /// Adds a shape to a view's drawing: its lines, kept together as one thing. A shape of one
    /// line is that line. How many lines were added.
    pub fn add_view_shape(&mut self, view: &str, kind: ShapeKind, lines: &[[[f64; 2]; 2]]) -> usize {
        let Some((own, shapes)) = self.drawn_mut(view) else { return 0 };
        let first = own.len();
        own.extend(lines.iter().filter(|l| l.iter().flatten().all(|v| v.is_finite()) && l[0] != l[1]));
        let count = own.len() - first;
        if count > 1 {
            shapes.push(Shape { kind, first, count });
        }
        count
    }

    /// The lines of a view that are one thing with its line `index`: the run of its shape and
    /// what that is, or the line alone.
    pub fn shape_of(&self, view: &str, index: usize) -> (std::ops::Range<usize>, Option<ShapeKind>) {
        match self.view_shapes(view).iter().find(|s| s.run().contains(&index)) {
            Some(s) => (s.run(), Some(s.kind)),
            None => (index..index + 1, None),
        }
    }

    /// Puts the lines `run` of a view's drawing through `map`: where they are, or (`copy`) as new
    /// lines after the others, a shape again when they were one. The run they are in now.
    pub fn map_view_lines(&mut self, view: &str, run: std::ops::Range<usize>, copy: bool, map: &dyn Fn([f64; 2]) -> [f64; 2]) -> Option<std::ops::Range<usize>> {
        let kind = self.view_shapes(view).iter().find(|s| s.run() == run).map(|s| s.kind);
        let (own, shapes) = self.drawn_mut(view)?;
        if run.is_empty() || run.end > own.len() {
            return None;
        }
        let mapped: Vec<[[f64; 2]; 2]> = own[run.clone()].iter().map(|l| [map(l[0]), map(l[1])]).collect();
        if !mapped.iter().flatten().flatten().all(|v| v.is_finite()) {
            return None;
        }
        if !copy {
            own[run.clone()].copy_from_slice(&mapped);
            return Some(run);
        }
        let first = own.len();
        own.extend(mapped);
        if let Some(kind) = kind {
            shapes.push(Shape { kind, first, count: run.len() });
        }
        Some(first..first + run.len())
    }

    /// Gives line `index` of a view's drawing other ends. False for a line that is none.
    pub fn set_view_line(&mut self, view: &str, index: usize, line: [[f64; 2]; 2]) -> bool {
        let Some((own, _)) = self.drawn_mut(view) else { return false };
        if index >= own.len() || line[0] == line[1] || !line.iter().flatten().all(|v| v.is_finite()) {
            return false;
        }
        own[index] = line;
        true
    }

    /// Makes the shape that line `index` of a view is part of loose lines again. False when the
    /// line is of no shape.
    pub fn explode_view_shape(&mut self, view: &str, index: usize) -> bool {
        let Some((_, shapes)) = self.drawn_mut(view) else { return false };
        let before = shapes.len();
        shapes.retain(|s| !s.run().contains(&index));
        shapes.len() != before
    }

    /// Removes a line of a view's drawing, with the shape it is part of.
    pub fn remove_view_line(&mut self, view: &str, index: usize) -> bool {
        let (run, _) = self.shape_of(view, index);
        let Some((own, shapes)) = self.drawn_mut(view) else { return false };
        if run.end > own.len() {
            return false;
        }
        own.drain(run.clone());
        shapes.retain(|s| s.first != run.start || s.count != run.len());
        // the shapes after it count their lines from where they are now
        for s in shapes.iter_mut().filter(|s| s.first >= run.end) {
            s.first -= run.len();
        }
        true
    }

    fn notes_mut(&mut self, path: &str) -> Option<&mut Vec<Note>> {
        if let Some(p) = self.plans.iter_mut().find(|p| p.path == path) {
            return Some(&mut p.notes);
        }
        self.sections.iter_mut().find(|s| s.path == path).map(|s| &mut s.notes)
    }

    /// Adds a text to a view's drawing at a point of it; a text of only spaces, or a point that
    /// is not finite, adds nothing.
    pub fn add_note(&mut self, view: &str, at: [f64; 2], text: &str) -> bool {
        let text = text.trim();
        if text.is_empty() || !at.iter().all(|v| v.is_finite()) {
            return false;
        }
        let Some(notes) = self.notes_mut(view) else { return false };
        notes.push(Note { at, text: text.to_string(), size: NOTE_HEIGHT, align: Align::Left, angle: 0.0, face: Default::default(), level: false });
        true
    }

    /// Adds a level mark to a section or an elevation at a point of its drawing: it says the
    /// height of that point. A floor plan has no heights to mark, so it gets none.
    pub fn add_level_mark(&mut self, view: &str, at: [f64; 2]) -> bool {
        if !at.iter().all(|v| v.is_finite()) {
            return false;
        }
        let Some(section) = self.sections.iter_mut().find(|s| s.path == view) else { return false };
        section.notes.push(Note { at, text: level_text(at[1]), size: NOTE_HEIGHT, align: Align::Left, angle: 0.0, face: Default::default(), level: true });
        true
    }

    pub fn move_pile_tag(&mut self, view: &str, index: usize, at: [f64; 2]) -> bool {
        if !at.iter().all(|x| x.is_finite()) { return false; }
        let Some(p) = self.plans.iter_mut().find(|p| p.path == view) else { return false };
        let Some(index) = index.checked_sub(p.notes.len()) else { return false };
        let Some(&(_, id, head)) = p.pile_tags.iter().find(|(k, _, _)| *k == index) else { return false };
        let offset = [at[0] - head[0], at[1] - head[1]];
        if p.corner_labels {
            p.presentation["pile_label_offsets"][id.to_string()] = json!(offset);
        } else {
            p.pile_offsets.insert(id, offset);
        }
        true
    }

    /// Moves a view's text to another point of its drawing.
    pub fn move_note(&mut self, view: &str, index: usize, at: [f64; 2]) -> bool {
        if !at.iter().all(|v| v.is_finite()) {
            return false;
        }
        let Some(note) = self.notes_mut(view).and_then(|n| n.get_mut(index)) else { return false };
        note.at = at;
        // a level mark says the height it stands at now
        if note.level {
            note.text = level_text(at[1]);
        }
        true
    }

    /// Gives a view's text another wording; it keeps one. A level mark's text is its height, so
    /// it is not typed over.
    pub fn set_note_text(&mut self, view: &str, index: usize, text: &str) -> bool {
        if text.trim().is_empty() || self.view_notes(view).and_then(|n| n.get(index)).is_some_and(|n| n.level) {
            return false;
        }
        let Some(note) = self.notes_mut(view).and_then(|n| n.get_mut(index)) else { return false };
        note.text = text.to_string();
        true
    }

    /// Removes a view's text.
    pub fn remove_note(&mut self, view: &str, index: usize) -> bool {
        let Some(notes) = self.notes_mut(view) else { return false };
        if index >= notes.len() {
            return false;
        }
        notes.remove(index);
        true
    }

    /// The sheets, in the order they were made.
    pub fn sheets(&self) -> &[Sheet] {
        &self.sheets
    }

    /// The sheet at `path`.
    pub fn sheet(&self, path: &str) -> Option<&Sheet> {
        self.sheets.iter().find(|s| s.path == path)
    }

    pub fn view_origin(&self, path: &str) -> Option<[f64; 2]> {
        let origin = self.view_presentation(path).get("origin")?;
        Some([origin.get(0)?.as_f64()?, origin.get(1)?.as_f64()?])
    }

    pub fn set_revision(&mut self, sheet: &str, index: usize, date: bool, value: String) {
        if let Some(entry) = self.sheets.iter_mut().find(|s| s.path == sheet).and_then(|s| s.revisions.get_mut(index)) {
            if date { entry.date = value; } else { entry.description = value; }
        }
    }

    pub fn title_value<'a>(&'a self, sheet: &'a Sheet, field: Field) -> &'a str {
        match field {
            Field::Number => &sheet.number,
            Field::Title => &sheet.name,
            Field::Project | Field::ProjectNumber | Field::Client => self.project_fields.get(&field).map_or("", String::as_str),
            _ => sheet.fields.get(&field).map_or("", String::as_str),
        }
    }

    pub fn set_title_value(&mut self, path: &str, field: Field, value: String) {
        let Some(sheet) = self.sheets.iter_mut().find(|s| s.path == path) else { return };
        match field {
            Field::Number => sheet.number = value,
            Field::Title => sheet.name = value,
            Field::Project | Field::ProjectNumber | Field::Client => {
                self.project_fields.insert(field, value);
                return;
            }
            Field::Logo | Field::Company | Field::Address | Field::Format | Field::Scale => return,
            _ => {
                sheet.fields.insert(field, value);
                return;
            }
        }
        sheet.title = format!("{} {}", sheet.number, sheet.name);
    }

    /// Makes an empty A3 sheet numbered after the last (`A01`, `A02`, ..) and named `name`, and
    /// gives its path.
    pub fn add_sheet(&mut self, name: &str) -> String {
        let mut n = self.sheets.len() + 1;
        while self.sheets.iter().any(|s| s.number == format!("A{n:02}")) {
            n += 1;
        }
        self.added += 1;
        let path = uuid(&format!("sheet/{}/{n}", self.added));
        let number = format!("A{n:02}");
        let title = format!("{number} {name}");
        self.sheets.push(Sheet { path: path.clone(), number, revisions: Default::default(), fields: TitleFields::new(), name: name.to_string(), title, paper: A3_LANDSCAPE, views: Vec::new() });
        path
    }

    /// Removes the sheet at `path`.
    pub fn remove_sheet(&mut self, path: &str) -> bool {
        let before = self.sheets.len();
        self.sheets.retain(|s| s.path != path);
        self.sheets.len() < before
    }

    /// Places a view (a floor plan, section or elevation) in the middle of a sheet at the view's
    /// own scale and gives its index on the sheet.
    pub fn place_plan(&mut self, sheet: &str, plan: &str) -> Option<usize> {
        self.view_name(plan)?;
        let scale = self.view_scale(plan).unwrap_or(SHEET_SCALE);
        let sheet = self.sheets.iter_mut().find(|s| s.path == sheet)?;
        let at = [sheet.paper[0] / 2.0, sheet.paper[1] / 2.0];
        sheet.views.push(PlacedView { plan: plan.to_string(), at, scale, show_title: None });
        Some(sheet.views.len() - 1)
    }

    /// Moves a placed view so that its middle is at `at`, kept on the paper.
    pub fn move_placed(&mut self, sheet: &str, index: usize, at: [f64; 2]) -> bool {
        let Some(sheet) = self.sheets.iter_mut().find(|s| s.path == sheet) else { return false };
        let paper = sheet.paper;
        let Some(view) = sheet.views.get_mut(index) else { return false };
        if !(at[0].is_finite() && at[1].is_finite()) {
            return false;
        }
        view.at = [at[0].clamp(0.0, paper[0]), at[1].clamp(0.0, paper[1])];
        true
    }

    /// Puts a placed view's middle at `at`, wherever that is: the middle of all a view cuts may
    /// lie off the paper while the part its crop keeps lies on it.
    pub fn put_placed(&mut self, sheet: &str, index: usize, at: [f64; 2]) -> bool {
        let Some(view) = self.sheets.iter_mut().find(|s| s.path == sheet).and_then(|s| s.views.get_mut(index)) else { return false };
        if !(at[0].is_finite() && at[1].is_finite()) {
            return false;
        }
        view.at = at;
        true
    }

    /// Sets the scale of a placed view (1 : `scale`).
    pub fn set_scale(&mut self, sheet: &str, index: usize, scale: f64) -> bool {
        if !(scale.is_finite() && scale > 0.0) {
            return false;
        }
        let Some(view) = self.sheets.iter_mut().find(|s| s.path == sheet).and_then(|s| s.views.get_mut(index)) else { return false };
        view.scale = scale;
        true
    }

    /// Gives a sheet another paper; its views stay on it.
    pub fn set_paper(&mut self, sheet: &str, paper: [f64; 2]) -> bool {
        if !(paper.iter().all(|v| v.is_finite() && *v > 0.0)) {
            return false;
        }
        let Some(sheet) = self.sheets.iter_mut().find(|s| s.path == sheet) else { return false };
        sheet.paper = paper;
        for view in &mut sheet.views {
            view.at = [view.at[0].min(paper[0]), view.at[1].min(paper[1])];
        }
        true
    }

    /// Sets how far a plan cuts above its level (metres; a plan that looks down: above its view
    /// depth).
    pub fn set_cut(&mut self, plan: &str, cut: f64) -> bool {
        let Some(plan) = self.plans.iter_mut().find(|p| p.path == plan) else { return false };
        if !(cut.is_finite() && (plan.kind.looks_up() || cut > -plan.depth)) {
            return false;
        }
        plan.cut = cut;
        true
    }

    /// Renames a view (a plan, section, elevation or 3D view): not empty, and not another view's
    /// name; false when it is not renamed.
    pub fn set_view_name(&mut self, path: &str, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() || self.view_name_taken(name, path) {
            return false;
        }
        let Some((own, _, _)) = self.own_mut(path) else { return false };
        if own == name {
            return false;
        }
        *own = name.to_string();
        true
    }

    /// Sets how far a view looks: a floor plan below its level (at least 0, and its cut stays
    /// above that), a ceiling plan up from its cut, a section or elevation beyond its line (both
    /// more than 0); false when it does not change.
    pub fn set_view_depth(&mut self, path: &str, depth: f64) -> bool {
        if !depth.is_finite() {
            return false;
        }
        let own = if let Some(p) = self.plans.iter_mut().find(|p| p.path == path) {
            if if p.kind.looks_up() { depth <= 0.0 } else { depth < 0.0 || p.cut <= -depth } {
                return false;
            }
            &mut p.depth
        } else if let Some(s) = self.sections.iter_mut().find(|s| s.path == path) {
            if depth <= 0.0 {
                return false;
            }
            &mut s.depth
        } else {
            return false;
        };
        if *own == depth {
            return false;
        }
        *own = depth;
        true
    }

    /// Crops a floor plan to a rectangle in plan (lower left, upper right; at least 10 cm each
    /// way), or shows all of it again (`None`).
    pub fn set_crop(&mut self, plan: &str, crop: Option<([f64; 2], [f64; 2])>) -> bool {
        if let Some((lo, hi)) = crop {
            if !(lo.iter().chain(&hi).all(|v| v.is_finite()) && hi[0] - lo[0] >= 0.1 && hi[1] - lo[1] >= 0.1) {
                return false;
            }
        }
        let Some((_, own)) = self.marks_mut(plan) else { return false };
        *own = crop;
        true
    }

    /// Takes a view off a sheet.
    pub fn remove_placed(&mut self, sheet: &str, index: usize) -> bool {
        let Some(sheet) = self.sheets.iter_mut().find(|s| s.path == sheet) else { return false };
        if index >= sheet.views.len() {
            return false;
        }
        sheet.views.remove(index);
        true
    }

    /// The height of a plan's cut (metres, the model's frame).
    pub fn cut_height(&self, plan: &PlanView) -> Option<f64> {
        Some(self.level(&plan.level)?.elevation + plan.cut)
    }

    /// The heights a plan shows, from low to high (metres, the model's frame): from its view
    /// depth up to its cut, or, a ceiling plan, from its cut up to its view depth.
    pub fn plan_range(&self, plan: &PlanView) -> Option<(f64, f64)> {
        let z = self.level(&plan.level)?.elevation;
        Some(if plan.kind.looks_up() { (z + plan.cut, z + plan.cut + plan.depth) } else { (z - plan.depth, z + plan.cut) })
    }

    /// A floor plan goes with its level, and a placed view with its floor plan.
    fn drop_orphan_plans(&mut self) {
        let levels = &self.levels;
        self.plans.retain(|p| levels.iter().any(|l| l.path == p.level));
        let (plans, sections) = (&self.plans, &self.sections);
        for sheet in &mut self.sheets {
            sheet.views.retain(|v| plans.iter().any(|p| p.path == v.plan) || sections.iter().any(|s| s.path == v.plan));
        }
    }

    /// The levels added in the app, low to high.
    pub fn added_levels(&self) -> impl Iterator<Item = &Level> {
        self.levels.iter().filter(|l| l.added)
    }

    /// What a new level gets unless the user types otherwise: `<word> <n>` with the first free
    /// `n` from the number of levels up, 3 m above the highest level (0 without levels).
    pub fn next_level(&self, word: &str) -> (String, f64) {
        let mut n = self.levels.len();
        let name = loop {
            let name = format!("{word} {n}");
            if !self.levels.iter().any(|l| same_name(&l.name, &name)) {
                break name;
            }
            n += 1;
        };
        let elevation = self.levels.iter().map(|l| l.elevation).reduce(f64::max).map_or(0.0, |z| z + 3.0);
        (name, elevation)
    }

    /// The levels as IfcX nodes: class, name, elevation and the matching placement.
    pub fn ifcx_nodes(&self) -> Vec<Value> {
        self.levels
            .iter()
            .map(|l| {
                json!({
                    "path": l.path,
                    "attributes": {
                        "bsi::ifc::class": {"code": STOREY_CLASS, "uri": STOREY_URI},
                        "bsi::ifc::prop::Name": l.name,
                        "bsi::ifc::prop::Elevation": l.elevation,
                        "usd::xformop": {"transform": [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, l.elevation, 1.0]]},
                    }
                })
            })
            .collect()
    }

    fn sort(&mut self) {
        self.levels.sort_by(|a, b| a.elevation.total_cmp(&b.elevation).then_with(|| a.name.cmp(&b.name)));
    }
}

/// An elevation as the user types it: metres, a comma or a point, an optional `m`.
pub fn parse_elevation(text: &str) -> Option<f64> {
    let text = text.trim();
    let text = text.strip_suffix('m').unwrap_or(text).trim().replace(',', ".");
    text.parse::<f64>().ok().filter(|z| z.is_finite())
}

fn same_name(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// Rounded to micrometres: an f32 elevation of a loader reads as the value it was written as.
fn micrometres(z: f64) -> f64 {
    (z * 1e6).round() / 1e6
}

/// A UUID (version 8, RFC 9562) derived from `seed`: the same seed gives the same path.
pub(crate) fn uuid(seed: &str) -> String {
    let fnv = |offset: u64| seed.bytes().fold(offset, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3));
    let (hi, lo) = (fnv(0xcbf2_9ce4_8422_2325), fnv(0x84222325_cbf29ce4));
    let hi = (hi & !0xf000) | 0x8000; // version 8
    let lo = (lo & !(0b11 << 62)) | (0b10 << 62); // variant RFC 9562
    format!("{:08x}-{:04x}-{:04x}-{:04x}-{:012x}", hi >> 32, (hi >> 16) & 0xffff, hi & 0xffff, lo >> 48, lo & 0xffff_ffff_ffff)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn model_with(levels: &[(&str, f32)]) -> Model {
        let mut model = Model::new(PathBuf::from("house.rvt"), "Revit", Vec::new());
        model.project.levels = levels.iter().map(|(n, z)| (n.to_string(), *z)).collect();
        model
    }

    fn names(document: &Document) -> Vec<&str> {
        document.levels().iter().map(|l| l.name.as_str()).collect()
    }

    #[test]
    fn office_dimension_typography_preserves_explicit_preferences() {
        let read = |v: Value| DimensionStyle::from_ifcx(&v).unwrap();
        let legacy = read(json!({"name":"3BM ronde stip","size":0.0025}));
        assert_eq!(legacy.font_family.as_deref(), Some("Arial Narrow"));
        assert_eq!(legacy.font_size_pt, Some(6.8));
        assert_eq!(read(legacy.to_ifcx()), legacy);
        let edited = read(json!({"name":"3BM ronde stip","font_family":"Segoe UI","font_size_pt":8.2}));
        assert_eq!(edited.font_family.as_deref(), Some("Segoe UI"));
        assert_eq!(edited.font_size_pt, Some(8.2));
        assert_eq!(read(edited.to_ifcx()), edited);
        assert_eq!(read(json!({"name":"3BM ronde stip","size":0.004})).paper_font_size(), 0.004);
        let plain = read(json!({"name":"Other","size":0.004}));
        assert_eq!(plain.label_family(), None);
        assert!(plain.to_ifcx().get("font_size_pt").is_none());
    }

    #[test]
    fn recognises_the_levels_of_a_model_low_to_high_with_stable_paths() {
        let model = model_with(&[("01 first floor", 3.1), ("00 ground floor", 0.0), ("-1 basement", -2.8)]);
        let document = Document::from_model(&model);
        assert_eq!(names(&document), ["-1 basement", "00 ground floor", "01 first floor"]);
        let first = &document.levels()[2];
        assert_eq!(first.elevation, 3.1, "an f32 elevation reads as written");
        assert!(!first.added);
        // a UUID, the same every time the model is read
        assert_eq!(first.path.len(), 36);
        assert_eq!(first.path.matches('-').count(), 4);
        assert_eq!(&first.path[14..15], "8", "version 8");
        assert_eq!(Document::from_model(&model).levels(), document.levels());
        let paths: std::collections::HashSet<&str> = document.levels().iter().map(|l| l.path.as_str()).collect();
        assert_eq!(paths.len(), 3);
    }

    #[test]
    fn adds_levels_in_their_place_by_elevation() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0), ("02", 6.0)]));
        assert_eq!(document.add_level("  01  ", 3.0), Ok(1));
        assert_eq!(names(&document), ["00", "01", "02"]);
        let added = &document.levels()[1];
        assert!(added.added);
        assert_eq!(added.elevation, 3.0);
        assert_ne!(added.path, document.levels()[0].path);
        assert_eq!(document.add_level("roof", 9.5), Ok(3));
        assert_eq!(document.added_levels().count(), 2);
    }

    #[test]
    fn refuses_an_empty_or_taken_name_and_a_bad_elevation() {
        let mut document = Document::from_model(&model_with(&[("Ground Floor", 0.0)]));
        assert_eq!(document.add_level("   ", 3.0), Err(LevelError::EmptyName));
        assert_eq!(document.add_level("ground floor", 3.0), Err(LevelError::DuplicateName));
        assert_eq!(document.add_level("Roof", f64::NAN), Err(LevelError::BadElevation));
        assert_eq!(document.add_level("Roof", f64::INFINITY), Err(LevelError::BadElevation));
        assert_eq!(document.levels().len(), 1);
    }

    #[test]
    fn removes_only_added_levels() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0)]));
        document.add_level("01", 3.0).unwrap();
        let model_level = document.levels()[0].path.clone();
        let added = document.levels()[1].path.clone();
        assert!(!document.remove_level(&model_level));
        assert!(document.remove_level(&added));
        assert!(!document.remove_level(&added));
        assert_eq!(names(&document), ["00"]);
    }

    #[test]
    fn recognising_again_keeps_added_levels_unless_the_model_has_the_name() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0)]));
        document.add_level("01", 3.0).unwrap();
        document.add_level("Roof", 9.0).unwrap();
        // a second model added: its levels join, the added ones stay
        document.recognise(&model_with(&[("00", 0.0), ("02", 6.0), ("roof", 9.2)]));
        assert_eq!(names(&document), ["00", "01", "02", "roof"]);
        assert_eq!(document.added_levels().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["01"]);
    }

    #[test]
    fn suggests_the_next_free_name_three_metres_above_the_highest_level() {
        assert_eq!(Document::default().next_level("Level"), ("Level 0".to_string(), 0.0));
        let mut document = Document::from_model(&model_with(&[("Level 0", 0.0), ("Level 2", 3.0)]));
        assert_eq!(document.next_level("Level"), ("Level 3".to_string(), 6.0));
        document.add_level("Level 3", 7.5).unwrap();
        assert_eq!(document.next_level("Niveau"), ("Niveau 3".to_string(), 10.5));
    }

    #[test]
    fn parses_elevations_as_typed() {
        assert_eq!(parse_elevation("3"), Some(3.0));
        assert_eq!(parse_elevation(" -2,75 "), Some(-2.75));
        assert_eq!(parse_elevation("+6.000 m"), Some(6.0));
        assert_eq!(parse_elevation("6m"), Some(6.0));
        assert_eq!(parse_elevation(""), None);
        assert_eq!(parse_elevation("three"), None);
        assert_eq!(parse_elevation("inf"), None);
    }

    #[test]
    fn makes_floor_plans_of_levels_named_after_them() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0), ("01", 3.0)]));
        let first = document.levels()[1].path.clone();
        let plan = document.add_plan(&first).expect("a plan");
        let again = document.add_plan(&first).expect("a second plan");
        assert_ne!(plan, again);
        let names: Vec<&str> = document.plans().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["01", "01 (2)"]);
        let view = document.plan(&plan).unwrap();
        assert_eq!((view.level.as_str(), view.cut, view.depth), (first.as_str(), PLAN_CUT, PLAN_DEPTH));
        // from just below the floor up to the cut
        assert_eq!(document.plan_range(view), Some((3.0 - PLAN_DEPTH, 3.0 + PLAN_CUT)));
        assert_eq!(document.add_plan("no such level"), None);
        assert!(document.remove_plan(&again));
        assert_eq!(document.plans().len(), 1);
    }

    /// A structural plan cuts as a floor plan and looks deeper; a ceiling plan looks up from its
    /// cut to the next level (or 1 m without one); IFCX keeps the kind.
    #[test]
    fn plans_of_each_kind_get_their_own_name_and_range_and_come_back_from_ifcx() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0), ("01", 3.0)]));
        let (ground, first) = (document.levels()[0].path.clone(), document.levels()[1].path.clone());
        let structural = document.add_plan_of(&ground, PlanKind::Structural, "structure").unwrap();
        let ceiling = document.add_plan_of(&ground, PlanKind::Ceiling, "ceiling").unwrap();
        let top = document.add_plan_of(&first, PlanKind::Ceiling, "ceiling").unwrap();
        let floor = document.add_plan_of(&ground, PlanKind::Floor, "floor").unwrap();
        let names: Vec<&str> = document.plans().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["00 (structure)", "00 (ceiling)", "01 (ceiling)", "00"]);
        let view = |path: &str| document.plan(path).unwrap().clone();
        assert_eq!((view(&structural).cut, view(&structural).depth), (PLAN_CUT, STRUCTURAL_DEPTH));
        assert_eq!(document.plan_range(&view(&structural)), Some((-STRUCTURAL_DEPTH, PLAN_CUT)));
        // from its cut up to the next level, or 1 m beyond the cut at the top
        let near = |range: Option<(f64, f64)>, want: (f64, f64)| range.is_some_and(|(lo, hi)| (lo - want.0).abs() < 1e-9 && (hi - want.1).abs() < 1e-9);
        assert!(near(document.plan_range(&view(&ceiling)), (CEILING_CUT, 3.0)));
        assert!(near(document.plan_range(&view(&top)), (3.0 + CEILING_CUT, 3.0 + CEILING_CUT + CEILING_DEPTH)));
        assert_eq!(view(&floor).kind, PlanKind::Floor);
        // a ceiling plan looks up: any cut, a depth above 0
        assert!(document.set_cut(&ceiling, 2.5));
        assert!(!document.set_view_depth(&ceiling, 0.0));
        assert!(document.set_view_depth(&ceiling, 0.4));
        let nodes: Vec<(String, Value)> =
            document.ifcx_drawing_nodes().into_iter().map(|n| (n["path"].as_str().unwrap().to_string(), n["attributes"].clone())).collect();
        let mut again = Document::from_model(&model_with(&[("00", 0.0), ("01", 3.0)]));
        again.adopt_ifcx(&nodes);
        assert_eq!(again.plans(), document.plans());
        // an IFCX file from before the kinds: floor plans
        let mut old = nodes.clone();
        for (_, attributes) in &mut old {
            if let Some(plan) = attributes.get_mut("o3v::view::plan").and_then(Value::as_object_mut) {
                plan.remove("kind");
            }
        }
        let mut before = Document::from_model(&model_with(&[("00", 0.0), ("01", 3.0)]));
        before.adopt_ifcx(&old);
        assert!(before.plans().iter().all(|p| p.kind == PlanKind::Floor));
    }

    /// Sections and elevations are kinds of their own, kept in IFCX; a node without a kind is a
    /// section.
    #[test]
    fn sections_and_elevations_keep_their_kind_in_ifcx() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0)]));
        let section = document.add_section("Section A", [0.0, 2.0], [8.0, 2.0], 20.0).unwrap();
        let north = document.add_section_of(SectionKind::Elevation, "North", [8.0, 7.0], [0.0, 7.0], 12.0).unwrap();
        assert_eq!(document.section(&section).unwrap().kind, SectionKind::Section);
        assert_eq!(document.section(&north).unwrap().kind, SectionKind::Elevation);
        let nodes: Vec<(String, Value)> =
            document.ifcx_drawing_nodes().into_iter().map(|n| (n["path"].as_str().unwrap().to_string(), n["attributes"].clone())).collect();
        let mut again = Document::from_model(&model_with(&[("00", 0.0)]));
        again.adopt_ifcx(&nodes);
        assert_eq!(again.sections(), document.sections());
        let mut old = nodes;
        for (_, attributes) in &mut old {
            if let Some(view) = attributes.get_mut("o3v::view::section").and_then(Value::as_object_mut) {
                view.remove("kind");
            }
        }
        let mut before = Document::from_model(&model_with(&[("00", 0.0)]));
        before.adopt_ifcx(&old);
        assert!(before.sections().iter().all(|s| s.kind == SectionKind::Section));
    }

    #[test]
    fn a_3d_view_keeps_its_camera_box_layers_and_name_in_ifcx() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0)]));
        let camera = Camera3d { eye: [20.0, -15.0, 12.0], target: [4.0, 3.0, 1.5], perspective: true, fov: 50.0 };
        let section = ViewBox { min: [0.0, 0.0, -0.5], max: [8.0, 6.0, 3.0], turn: 0.25, pivot: [4.0, 3.0, 1.25] };
        assert_eq!(document.next_view_3d_name("3D view"), "3D view 1");
        let furniture_off = LayerSwitches::from([("IfcFurniture".to_string(), false)]);
        let view = document.add_view_3d("3D view 1", camera, Some(section), furniture_off.clone(), Arc::default());
        assert_eq!(document.next_view_3d_name("3D view"), "3D view 2");
        // a name is the view's own among all views
        let level = document.levels()[0].path.clone();
        let plan = document.add_plan(&level).unwrap();
        assert!(!document.set_view_name(&view, "00"), "the floor plan's name");
        assert!(document.set_view_name(&view, "Entrance"));
        assert_eq!(document.view_name(&view), Some("Entrance"));
        let parallel = document.add_view_3d("entrance", Camera3d { perspective: false, ..camera }, None, LayerSwitches::new(), Arc::default());
        assert_eq!(document.view_name(&parallel), Some("entrance (2)"));
        // it looks again as it was left
        let turned = Camera3d { eye: [-10.0, 8.0, 30.0], ..camera };
        assert!(document.set_view_3d_camera(&view, turned, Some(section)));
        assert!(!document.set_view_3d_camera(&view, turned, Some(section)), "nothing changed");
        assert!(!document.set_view_3d_camera(&plan, turned, None), "a floor plan has no camera");
        let nodes: Vec<(String, Value)> =
            document.ifcx_drawing_nodes().into_iter().map(|n| (n["path"].as_str().unwrap().to_string(), n["attributes"].clone())).collect();
        let mut again = Document::from_model(&model_with(&[("00", 0.0)]));
        again.adopt_ifcx(&nodes);
        assert_eq!(again.views_3d(), document.views_3d());
        assert_eq!(again.view_3d(&view).unwrap().camera, turned);
        assert_eq!(again.view_layers(&view), Some(&furniture_off));
        assert_eq!(again.view_3d(&parallel).unwrap().section, None);
        // what a view shows of the loads and results goes with it
        let shown = crate::structural::diagrams::Shown { deformed: true, plates: true, plate_value: 3, scale: 2.0, bands: 6, ..Default::default() };
        assert!(document.set_view_3d_analysis(&parallel, Some(shown)) && !document.set_view_3d_analysis(&parallel, Some(shown)));
        let nodes: Vec<(String, Value)> = document.ifcx_drawing_nodes().into_iter().map(|n| (n["path"].as_str().unwrap().to_string(), n["attributes"].clone())).collect();
        let mut kept = Document::from_model(&model_with(&[("00", 0.0)]));
        kept.adopt_ifcx(&nodes);
        assert_eq!((kept.view_3d(&parallel).unwrap().analysis, kept.view_3d(&view).unwrap().analysis), (Some(shown), None));
        // a node without its camera is no 3D view; one without a field of view gets one
        let mut broken = nodes;
        for (path, attributes) in &mut broken {
            let Some(v) = attributes.get_mut("o3v::view::3d").and_then(|v| v.get_mut("camera")).and_then(Value::as_object_mut) else { continue };
            if *path == view { v.remove("eye"); } else { v.remove("fov"); }
        }
        let mut other = Document::from_model(&model_with(&[("00", 0.0)]));
        other.adopt_ifcx(&broken);
        assert_eq!(other.views_3d().len(), 1);
        assert_eq!(other.views_3d()[0].camera.fov, VIEW_3D_FOV);
        assert!(document.remove_view_3d(&view) && document.view_3d(&view).is_none());
    }

    #[test]
    fn a_floor_plan_goes_with_its_level() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0)]));
        document.add_level("Roof", 6.0).unwrap();
        let ground = document.levels()[0].path.clone();
        let roof = document.levels()[1].path.clone();
        document.add_plan(&ground).unwrap();
        document.add_plan(&roof).unwrap();
        document.remove_level(&roof);
        assert_eq!(document.plans().iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["00"]);
        // the model read again keeps the plan of a level it still has, not of one it lost
        document.recognise(&model_with(&[("00", 0.0), ("01", 3.0)]));
        assert_eq!(document.plans().len(), 1);
        document.recognise(&model_with(&[("01", 3.0)]));
        assert!(document.plans().is_empty());
    }

    #[test]
    fn sheets_hold_floor_plans_that_move_on_the_paper() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0), ("01", 3.0)]));
        let sheet = document.add_sheet("Floor plans");
        let second = document.add_sheet("Details");
        let numbers: Vec<&str> = document.sheets().iter().map(|s| s.number.as_str()).collect();
        assert_eq!(numbers, ["A01", "A02"]);
        assert_eq!(document.sheet(&sheet).unwrap().paper, A3_LANDSCAPE);
        let ground = document.levels()[0].path.clone();
        let plan = document.add_plan(&ground).unwrap();
        assert_eq!(document.place_plan(&sheet, &plan), Some(0));
        assert_eq!(document.place_plan(&sheet, "no plan"), None);
        assert_eq!(document.place_plan("no sheet", &plan), None);
        let placed = &document.sheet(&sheet).unwrap().views[0];
        assert_eq!((placed.at, placed.scale), ([0.21, 0.1485], SHEET_SCALE));
        // moved, and kept on the paper
        assert!(document.move_placed(&sheet, 0, [0.1, 0.08]));
        assert_eq!(document.sheet(&sheet).unwrap().views[0].at, [0.1, 0.08]);
        assert!(document.move_placed(&sheet, 0, [-1.0, 9.0]));
        assert_eq!(document.sheet(&sheet).unwrap().views[0].at, [0.0, 0.297]);
        assert!(!document.move_placed(&sheet, 1, [0.1, 0.1]));
        // a removed plan leaves its sheets
        document.remove_plan(&plan);
        assert!(document.sheet(&sheet).unwrap().views.is_empty());
        assert!(document.remove_sheet(&second));
        assert_eq!(document.add_sheet("Again").len(), 36);
        assert_eq!(document.sheets()[1].number, "A02");
    }

    #[test]
    fn a_level_mark_says_its_height_follows_a_move_and_comes_back_from_the_file() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0)]));
        let level = document.levels()[0].path.clone();
        let plan = document.add_plan(&level).unwrap();
        let section = document.add_section("Section A", [0.0, 0.0], [8.0, 0.0], 6.0).unwrap();
        assert!(!document.add_level_mark(&plan, [1.0, 2.0]), "a floor plan has no heights");
        assert!(document.add_level_mark(&section, [1.0, 2.7]));
        let note = &document.view_notes(&section).unwrap()[0];
        assert_eq!((note.text.as_str(), note.level), ("+2.700", true));
        // moved: its text is the height it stands at; typed over: it is not
        assert!(document.move_note(&section, 0, [1.0, -0.0001]));
        assert_eq!(document.view_notes(&section).unwrap()[0].text, "+0.000");
        assert!(!document.set_note_text(&section, 0, "Roof"));
        assert!(document.move_note(&section, 0, [1.0, -1.25]));
        let nodes: Vec<(String, Value)> =
            document.ifcx_drawing_nodes().into_iter().map(|n| (n["path"].as_str().unwrap().to_string(), n["attributes"].clone())).collect();
        let mut again = Document::from_model(&model_with(&[("00", 0.0)]));
        again.adopt_ifcx(&nodes);
        let back = &again.view_notes(&section).unwrap()[0];
        assert_eq!((back.text.as_str(), back.level, back.at), ("-1.250", true, [1.0, -1.25]));
    }

    #[test]
    fn scale_paper_and_cut_can_change() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0)]));
        let level = document.levels()[0].path.clone();
        let plan = document.add_plan(&level).unwrap();
        let sheet = document.add_sheet("Plans");
        document.place_plan(&sheet, &plan).unwrap();
        assert!(document.set_scale(&sheet, 0, 50.0));
        assert!(!document.set_scale(&sheet, 0, 0.0));
        assert!(!document.set_scale(&sheet, 3, 50.0));
        assert_eq!(document.sheet(&sheet).unwrap().views[0].scale, 50.0);
        // a smaller paper keeps the view on it
        assert!(document.set_paper(&sheet, PAPERS[0].1));
        let s = document.sheet(&sheet).unwrap();
        assert_eq!((s.paper, s.views[0].at), ([0.297, 0.210], [0.21, 0.1485]));
        assert!(document.set_paper(&sheet, [0.1, 0.1]));
        assert_eq!(document.sheet(&sheet).unwrap().views[0].at, [0.1, 0.1]);
        assert!(!document.set_paper(&sheet, [0.0, 0.1]));
        // the cut, not below the view depth
        assert!(document.set_cut(&plan, 2.0));
        assert_eq!(document.plan_range(document.plan(&plan).unwrap()), Some((-PLAN_DEPTH, 2.0)));
        assert!(!document.set_cut(&plan, -0.5));
        assert!(!document.set_cut(&plan, f64::NAN));
    }

    #[test]
    fn a_floor_plan_is_cropped_to_a_rectangle() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0)]));
        let level = document.levels()[0].path.clone();
        let plan = document.add_plan(&level).unwrap();
        assert!(document.set_crop(&plan, Some(([0.0, 0.0], [4.0, 3.0]))));
        assert_eq!(document.plan(&plan).unwrap().crop, Some(([0.0, 0.0], [4.0, 3.0])));
        assert!(!document.set_crop(&plan, Some(([0.0, 0.0], [0.05, 3.0]))), "under 10 cm");
        assert!(!document.set_crop(&plan, Some(([2.0, 0.0], [1.0, 3.0]))), "inside out");
        assert!(document.set_crop(&plan, None));
        assert_eq!(document.plan(&plan).unwrap().crop, None);
    }

    #[test]
    fn sections_look_left_of_their_line_and_take_dimensions() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0)]));
        assert_eq!(document.next_section_name("Section"), "Section A");
        let a = document.add_section("Section A", [0.0, 2.0], [8.0, 2.0], 20.0).unwrap();
        assert_eq!(document.next_section_name("Section"), "Section B");
        assert_eq!(document.add_section("Too short", [0.0, 0.0], [0.05, 0.0], 20.0), None);
        let s = document.section(&a).unwrap();
        assert_eq!(s.axes(), ([1.0, 0.0], [-0.0, 1.0]));
        assert_eq!(document.view_name(&a), Some("Section A"));
        assert!(document.add_dimension(&a, [0.0, 0.0], [0.0, 3.0]));
        assert!(document.set_crop(&a, Some(([0.0, -0.5], [8.0, 4.0]))));
        let (dimensions, crop) = document.view_marks(&a).unwrap();
        assert_eq!((dimensions.len(), crop), (1, Some(([0.0, -0.5], [8.0, 4.0]))));
        // on a sheet, then gone with its section
        let sheet = document.add_sheet("Sections");
        assert_eq!(document.place_plan(&sheet, &a), Some(0));
        assert!(document.remove_section(&a));
        assert!(document.sheet(&sheet).unwrap().views.is_empty());
    }

    #[test]
    fn authored_dimension_chains_keep_real_witness_origins_and_separate_outer_lines() {
        let model = model_with(&[("00",0.)]);
        let node = json!({"o3v::view::plan":{
            "level":"00", "name":"Layout", "drawing":"pile-plan", "dimensions":[
                [[0.,3.],[2.,3.]], [[2.,3.],[5.,3.]], [[0.,5.],[5.,5.]]],
            "presentation":{"dimensionChains":[
                {"axis":0,"offset":3.,"references":[[0.,1.],[2.,0.],[5.,1.]],"firstDimension":0,"segments":2},
                {"axis":0,"offset":5.,"references":[[0.,1.],[5.,1.]],"firstDimension":2,"segments":1}]},
            "lines":[[[2.,2.8],[2.,3.1]], [[0.,0.],[5.,0.]]]
        }});
        let mut doc = Document::from_model(&model);
        doc.adopt_ifcx(&[("layout".into(),node)]);
        let plan = doc.plan("layout").unwrap();
        assert_eq!(plan.lines.len(),1);
        assert_eq!(plan.dimensions.len(),2);
        assert_eq!(plan.dimensions[1].measured_ends(),[[0.,5.],[5.,5.]]);
        let spans = plan.dimensions[0].intervals().collect::<Vec<_>>();
        assert_eq!(spans[1], ([[2.,0.],[5.,1.]], [[2.,3.],[5.,3.]]));
        assert_eq!(plan.dimensions[0].paper_offset(),0.);
        let nodes = doc.ifcx_drawing_nodes().into_iter().map(|n| (n["path"].as_str().unwrap().to_owned(), n["attributes"].clone())).collect::<Vec<_>>();
        let mut again = Document::from_model(&model);
        again.adopt_ifcx(&nodes);
        assert_eq!(again.plan("layout").unwrap().dimensions, plan.dimensions);
    }

    #[test]
    fn station_rounding_keeps_published_intervals_consistent_with_their_references() {
        let model = model_with(&[("00",0.)]);
        let mut doc = Document::from_model(&model);
        let plan = doc.add_plan(&doc.levels()[0].path.clone()).unwrap();
        assert!(doc.add_dimension_chain(&plan, &[[-44.,1.2],[-42.7308,1.2],[-41.0385,1.2]]));
        let d = &doc.plan(&plan).unwrap().dimensions[0];
        let office = DimensionStyle::from_ifcx(&json!({"name":"3BM ronde stip"})).unwrap();
        let values = |style: &DimensionStyle| d.intervals().map(|(_, [a,b])| style.figure(d.interval_value(a,b,style))).collect::<Vec<_>>();
        assert_eq!(values(&office), ["1269","1693"]);
        assert_eq!(values(&DimensionStyle::standard()), ["1269","1692"]);
        assert!((d.length()-2.9615).abs()<1e-9);
    }

    #[test]
    fn an_explicit_negative_chain_stays_on_its_line_after_edits_and_its_total_is_further_out() {
        let model = model_with(&[("00",0.)]);
        let mut doc = Document::from_model(&model);
        let plan = doc.add_plan(&doc.levels()[0].path.clone()).unwrap();
        assert!(doc.add_dimension_chain(&plan,&[[0.,0.],[2.,0.],[4.,0.]]));
        let c = doc.dimension_mut(&plan,0).unwrap().chain.as_mut().unwrap();
        c.baseline = Some([0.,-2.]);
        c.offset = 0.;
        assert!(doc.add_dimension_chain_total(&plan,0));
        assert_eq!(doc.plan(&plan).unwrap().dimensions[1].paper_offset(),-0.008);
        assert!(doc.insert_dimension_reference(&plan,0,[5.,1.],None));
        assert!(doc.remove_dimension_reference(&plan,0,0));
        assert!(doc.remove_dimension_reference(&plan,0,0));
        let d = doc.dimension_mut(&plan,0).unwrap();
        d.order_references();
        assert_eq!(d.measured_ends(), [[4.,-2.],[5.,-2.]]);
        assert!(doc.flip_dimension(&plan,0));
        assert_eq!(doc.plan(&plan).unwrap().dimensions[0].measured_ends(), [[5.,3.],[4.,3.]]);
        let nodes = doc.ifcx_drawing_nodes().into_iter().map(|n| (n["path"].as_str().unwrap().to_owned(), n["attributes"].clone())).collect::<Vec<_>>();
        let mut again = Document::from_model(&model);
        again.adopt_ifcx(&nodes);
        assert_eq!(again.plan(&plan).unwrap().dimensions, doc.plan(&plan).unwrap().dimensions);
        assert!(again.flip_dimension(&plan,0));
        assert_eq!(again.plan(&plan).unwrap().dimensions[0].measured_ends(), [[4.,-2.],[5.,-2.]]);
    }

    #[test]
    fn dimension_chain_edits_keep_order_anchors_and_independent_totals_on_reopen() {
        let model = model_with(&[("00", 0.0)]);
        let mut doc = Document::from_model(&model);
        let plan = doc.add_plan(&doc.levels()[0].path.clone()).unwrap();
        assert!(doc.add_dimension_chain(&plan, &[[0.,0.], [2.,0.], [6.,0.]]));
        let anchor = Some(Anchor { element: 15, point: 0, offset: [0.,0.], lost: false });
        assert!(doc.insert_dimension_reference(&plan, 0, [3.,1.], anchor));
        assert!(!doc.insert_dimension_reference(&plan, 0, [3.,2.], None));
        assert!(doc.remove_dimension_reference(&plan, 0, 1));
        assert!(doc.add_dimension_chain_total(&plan, 0));
        assert!(doc.flip_dimension(&plan, 0));
        let ds = &doc.plan(&plan).unwrap().dimensions;
        assert_eq!(ds[0].references().collect::<Vec<_>>(), [([6.,0.],None), ([3.,1.],anchor), ([0.,0.],None)]);
        let nodes = doc.ifcx_drawing_nodes().into_iter().map(|n| (n["path"].as_str().unwrap().to_owned(), n["attributes"].clone())).collect::<Vec<_>>();
        let mut again = Document::from_model(&model);
        again.adopt_ifcx(&nodes);
        assert_eq!(&again.plan(&plan).unwrap().dimensions, ds);
        assert!(again.remove_dimension_reference(&plan, 0, 1));
        assert!(!again.remove_dimension_reference(&plan, 0, 1));
    }

    #[test]
    fn floor_plans_keep_their_dimensions() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0)]));
        let level = document.levels()[0].path.clone();
        let plan = document.add_plan(&level).unwrap();
        assert!(document.add_dimension(&plan, [0.0, 0.0], [3.6, 0.0]));
        assert!(document.add_dimension(&plan, [0.0, 0.0], [3.0, 4.0]));
        assert!(!document.add_dimension(&plan, [1.0, 1.0], [1.0, 1.0005]), "less than a millimetre");
        assert!(!document.add_dimension(&plan, [0.0, f64::NAN], [1.0, 1.0]));
        assert!(!document.add_dimension("no plan", [0.0, 0.0], [1.0, 0.0]));
        let lengths: Vec<f64> = document.plan(&plan).unwrap().dimensions.iter().map(Dimension::length).collect();
        assert_eq!(lengths, [3.6, 5.0]);
        assert!(document.flip_dimension(&plan, 1));
        assert_eq!(document.plan(&plan).unwrap().dimensions[1], Dimension { from: [3.0, 4.0], to: [0.0, 0.0], style: 0, anchors: [None, None], chain: None });
        assert!(!document.flip_dimension(&plan, 9));
        assert!(document.remove_dimension(&plan, 0));
        assert!(!document.remove_dimension(&plan, 5));
        assert_eq!(document.plan(&plan).unwrap().dimensions.len(), 1);
    }

    #[test]
    fn the_shapes_drawn_on_a_view_are_runs_of_its_lines_and_are_saved_in_ifcx() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0)]));
        let level = document.levels()[0].path.clone();
        let plan = document.add_plan(&level).unwrap();
        let section = document.add_section("Section A", [0.0, 2.0], [8.0, 2.0], 20.0).unwrap();
        let square = [[[0.0, 0.0], [1.0, 0.0]], [[1.0, 0.0], [1.0, 1.0]], [[1.0, 1.0], [0.0, 1.0]], [[0.0, 1.0], [0.0, 0.0]]];
        assert_eq!(document.add_view_lines(&plan, &[[[5.0, 5.0], [6.0, 5.0]]]), 1);
        assert_eq!(document.add_view_shape(&plan, ShapeKind::Rectangle, &square), 4);
        // a shape of one line is that line, and a line without length is no line
        assert_eq!(document.add_view_shape(&plan, ShapeKind::Polyline, &[[[7.0, 7.0], [8.0, 7.0]], [[8.0, 7.0], [8.0, 7.0]]]), 1);
        assert_eq!(document.add_view_shape(&plan, ShapeKind::Arc, &square[..2]), 2);
        assert_eq!(document.add_view_shape(&section, ShapeKind::Polyline, &square[..3]), 3);
        assert_eq!(document.add_view_shape("no view", ShapeKind::Circle, &square), 0);
        let runs = |d: &Document, view: &str| d.view_shapes(view).iter().map(|s| (s.kind, s.run())).collect::<Vec<_>>();
        assert_eq!(runs(&document, &plan), [(ShapeKind::Rectangle, 1..5), (ShapeKind::Arc, 6..8)]);
        assert_eq!(document.shape_of(&plan, 5), (5..6, None));
        assert_eq!(document.shape_of(&plan, 7), (6..8, Some(ShapeKind::Arc)));
        // saved with the drawings and taken over again; the lines are what a reader without
        // shapes sees
        let nodes: Vec<(String, Value)> =
            document.ifcx_drawing_nodes().into_iter().map(|n| (n["path"].as_str().unwrap().to_string(), n["attributes"].clone())).collect();
        let written = &nodes.iter().find(|(path, _)| *path == plan).unwrap().1["o3v::view::plan"];
        assert_eq!(written["shapes"], json!([["rectangle", 1, 4], ["arc", 6, 2]]));
        assert_eq!(written["lines"].as_array().unwrap().len(), 8);
        let mut again = Document::from_model(&model_with(&[("00", 0.0)]));
        again.adopt_ifcx(&nodes);
        assert_eq!(runs(&again, &plan), runs(&document, &plan));
        assert_eq!(runs(&again, &section), [(ShapeKind::Polyline, 0..3)]);
        assert_eq!(again.view_lines(&plan), document.view_lines(&plan));
        // a view without shapes writes none, and runs that do not fit its lines are not read
        let bare = document.add_plan(&level).unwrap();
        let nodes = document.ifcx_drawing_nodes();
        let node = nodes.iter().find(|n| n["path"] == bare.as_str()).unwrap();
        assert!(node["attributes"]["o3v::view::plan"].get("shapes").is_none());
        let mut odd: Vec<(String, Value)> = nodes.iter().map(|n| (n["path"].as_str().unwrap().to_string(), n["attributes"].clone())).collect();
        let at = odd.iter().position(|(path, _)| *path == plan).unwrap();
        odd[at].1["o3v::view::plan"]["shapes"] = json!([["rectangle", 1, 4], ["circle", 3, 2], ["arc", 6, 9], ["blob", 5, 1]]);
        let mut again = Document::from_model(&model_with(&[("00", 0.0)]));
        again.adopt_ifcx(&odd);
        assert_eq!(runs(&again, &plan), [(ShapeKind::Rectangle, 1..5)], "one that overlaps, one past the end and one of no kind are left out");
        // removing a line of a shape removes the shape; the ones after it count on
        assert!(document.remove_view_line(&plan, 2));
        assert_eq!(runs(&document, &plan), [(ShapeKind::Arc, 2..4)]);
        assert!(document.remove_view_line(&plan, 0));
        assert_eq!(runs(&document, &plan), [(ShapeKind::Arc, 1..3)]);
        assert!(document.remove_view_line(&plan, 2));
        assert!(document.view_shapes(&plan).is_empty() && document.view_lines(&plan).len() == 1);
        assert!(!document.remove_view_line(&plan, 4));
    }

    #[test]
    fn used_sounding_is_projected_once_on_reopen_and_export() {
        let model = model_with(&[("00", 0.0)]);
        let mut doc = Document::from_model(&model);
        let plan = doc.add_plan(&doc.levels()[0].path.clone()).unwrap();
        // Delivered RD position of the public BRO fixture in bim-civil.
        let id = "CPT000000000785";
        doc.plans[0].presentation = json!({"rd_origin":[155000.,463000.], "crs":"EPSG:28992", "rd_unit":"metre",
            "rd_rotation_degrees":90.0, "soundings":[{"id":id,"rd":[154652.,463125.],"crs":"EPSG:28992","source":"BRO"}]});
        let nodes = |doc: &Document| doc.ifcx_drawing_nodes().into_iter().map(|n| (n["path"].as_str().unwrap().to_owned(), n["attributes"].clone())).collect::<Vec<_>>();
        let mut reopened = Document::from_model(&model);
        reopened.adopt_ifcx(&nodes(&doc));
        for _ in 0..2 {
            let symbol = reopened.view_symbols(&plan);
            assert_eq!(symbol.len(), 34);
            let [a,b] = symbol[32];
            assert!(((a[0]+b[0])/2.0-125.).abs()<1e-8 && (a[1]-348.).abs()<1e-8);
            assert_eq!(reopened.view_tags(&plan).iter().filter(|n| n.text==id).count(),1);
            reopened.set_plan_tags(|_| (vec![],vec![]));
        }
        let drawing = crate::drawing2d::view_drawing(&reopened,&plan,&Default::default(),&Default::default()).unwrap();
        assert!(drawing.texts.iter().any(|t| t.text == id));
        assert!(drawing.lines.len() >= 34);
        // Documents with a manually drawn and labelled cross keep one marker.
        let arms = reopened.view_symbols(&plan)[32..].to_vec();
        reopened.add_view_lines(&plan,&arms);
        reopened.add_note(&plan,[126.,348.],id);
        let mut legacy = Document::from_model(&model);
        legacy.adopt_ifcx(&nodes(&reopened));
        assert!(legacy.view_tags(&plan).is_empty() && legacy.view_symbols(&plan).is_empty());
        assert_eq!(legacy.view_lines(&plan).len(),2);
        assert_eq!(legacy.view_notes(&plan).unwrap()[0].text,id);
    }

    #[test]
    fn views_keep_their_notes_and_save_them_in_ifcx() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0)]));
        let level = document.levels()[0].path.clone();
        let plan = document.add_plan(&level).unwrap();
        let section = document.add_section("Section A", [0.0, 2.0], [8.0, 2.0], 20.0).unwrap();
        assert!(document.add_note(&plan, [1.0, 2.0], "Kitchen"));
        assert!(document.add_note(&plan, [4.0, 2.0], "Hall"));
        assert!(document.add_note(&section, [0.5, 3.0], "Roof +3.000"));
        assert!(!document.add_note(&plan, [1.0, 1.0], "  "), "no text");
        assert!(!document.add_note(&plan, [f64::NAN, 1.0], "x"));
        assert!(!document.add_note("no view", [1.0, 1.0], "x"));
        assert!(document.move_note(&plan, 1, [5.0, 2.5]));
        assert!(document.set_note_text(&plan, 1, "Hall 2"));
        assert!(!document.set_note_text(&plan, 1, ""), "a note keeps a text");
        assert!(!document.move_note(&plan, 7, [0.0, 0.0]));
        let notes = document.view_notes(&plan).unwrap();
        assert_eq!(notes, [Note { at: [1.0, 2.0], text: "Kitchen".into(), size: NOTE_HEIGHT, align: Align::Left, angle: 0.0, face: Default::default(), level: false }, Note { at: [5.0, 2.5], text: "Hall 2".into(), size: NOTE_HEIGHT, align: Align::Left, angle: 0.0, face: Default::default(), level: false }]);
        assert!(document.remove_note(&plan, 0));
        assert!(!document.remove_note(&plan, 3));
        // saved with the drawings and taken over again
        let nodes: Vec<(String, Value)> =
            document.ifcx_drawing_nodes().into_iter().map(|n| (n["path"].as_str().unwrap().to_string(), n["attributes"].clone())).collect();
        let mut again = Document::from_model(&model_with(&[("00", 0.0)]));
        again.adopt_ifcx(&nodes);
        assert_eq!(again.view_notes(&plan).unwrap(), [Note { at: [5.0, 2.5], text: "Hall 2".into(), size: NOTE_HEIGHT, align: Align::Left, angle: 0.0, face: Default::default(), level: false }]);
        assert_eq!(again.view_notes(&section).unwrap(), [Note { at: [0.5, 3.0], text: "Roof +3.000".into(), size: NOTE_HEIGHT, align: Align::Left, angle: 0.0, face: Default::default(), level: false }]);
    }

    #[test]
    fn a_views_own_layers_are_saved_in_ifcx() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0)]));
        let level = document.levels()[0].path.clone();
        let plan = document.add_plan(&level).unwrap();
        let section = document.add_section("Section A", [0.0, 2.0], [8.0, 2.0], 20.0).unwrap();
        assert!(document.view_layers(&plan).unwrap().is_empty(), "a new view shows the layers as the model does");
        let switches = LayerSwitches::from([("IfcSpace".to_string(), true), ("IfcFurniture".to_string(), false)]);
        assert!(document.set_view_layers(&plan, switches.clone()));
        assert!(!document.set_view_layers(&plan, switches.clone()), "nothing changed");
        assert!(document.set_view_layers(&section, LayerSwitches::from([("IfcRoof".to_string(), false)])));
        assert!(!document.set_view_layers("no view", switches.clone()));
        let nodes: Vec<(String, Value)> =
            document.ifcx_drawing_nodes().into_iter().map(|n| (n["path"].as_str().unwrap().to_string(), n["attributes"].clone())).collect();
        let mut again = Document::from_model(&model_with(&[("00", 0.0)]));
        again.adopt_ifcx(&nodes);
        assert_eq!(again.view_layers(&plan), Some(&switches));
        assert_eq!(again.view_layers(&section), Some(&LayerSwitches::from([("IfcRoof".to_string(), false)])));
    }

    #[test]
    fn a_view_is_renamed_and_looks_as_far_as_is_typed() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0), ("01", 3.0)]));
        let level = document.levels()[0].path.clone();
        let plan = document.add_plan(&level).unwrap();
        let section = document.add_section("Section A", [0.0, 2.0], [8.0, 2.0], 20.0).unwrap();
        assert!(document.set_view_name(&plan, " Ground floor "));
        assert_eq!(document.view_name(&plan), Some("Ground floor"));
        assert!(!document.set_view_name(&plan, "Ground floor"), "nothing changes");
        assert!(!document.set_view_name(&plan, "  "));
        assert!(!document.set_view_name(&section, "ground FLOOR"), "another view's name");
        assert!(document.set_view_name(&section, "Section 1"));
        assert_eq!(document.view_name(&section), Some("Section 1"));
        assert!(!document.set_view_name("no view", "X"));
        // a floor plan looks down to its level at least, and its cut stays above that
        assert!(document.set_view_depth(&plan, 0.0));
        assert!(!document.set_view_depth(&plan, 0.0), "nothing changes");
        assert!(!document.set_view_depth(&plan, -0.1));
        assert!(!document.set_view_depth(&plan, f64::NAN));
        assert!(!document.set_cut(&plan, -0.5));
        assert!(document.set_view_depth(&plan, 1.0));
        assert!(document.set_cut(&plan, -0.5));
        assert!(!document.set_view_depth(&plan, 0.4), "the cut would be under it");
        assert_eq!(document.plan(&plan).unwrap().depth, 1.0);
        // a section looks beyond its line
        assert!(!document.set_view_depth(&section, 0.0));
        assert!(document.set_view_depth(&section, 5.0));
        assert_eq!(document.section(&section).unwrap().depth, 5.0);
        assert!(!document.set_view_depth("no view", 1.0));
    }

    #[test]
    fn levels_are_ifcx_storey_nodes() {
        let mut document = Document::from_model(&model_with(&[("00", 0.0)]));
        document.add_level("01", 3.25).unwrap();
        let nodes = document.ifcx_nodes();
        assert_eq!(nodes.len(), 2);
        let node = &nodes[1];
        assert_eq!(node["path"], document.levels()[1].path.as_str());
        let attributes = &node["attributes"];
        assert_eq!(attributes["bsi::ifc::class"]["code"], "IfcBuildingStorey");
        assert_eq!(attributes["bsi::ifc::class"]["uri"], STOREY_URI);
        assert_eq!(attributes["bsi::ifc::prop::Name"], "01");
        assert_eq!(attributes["bsi::ifc::prop::Elevation"], 3.25);
        assert_eq!(attributes["usd::xformop"]["transform"][3], json!([0.0, 0.0, 3.25, 1.0]));
    }
}
