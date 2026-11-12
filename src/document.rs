//! The project document in IfcX (IFC5) form, kept beside the loaded model: what the app
//! recognises in a model and what the user adds, in the shape of IfcX nodes, so that it can be
//! written as `.ifcx` (the native format to come): levels, the plans made on them, sections and
//! elevations, 3D views and sheets.
//!
//! Headless: no GUI or GPU, so the MCP server and a later BIM kernel crate can use it as it is.

use crate::model::Model;
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
}

impl PlanKind {
    pub const ALL: [PlanKind; 3] = [PlanKind::Floor, PlanKind::Structural, PlanKind::Ceiling];

    /// Its name in IFCX (`kind` of an `o3v::view::plan` node).
    pub fn key(self) -> &'static str {
        match self {
            PlanKind::Floor => "floor",
            PlanKind::Structural => "structural",
            PlanKind::Ceiling => "ceiling",
        }
    }

    /// The kind named `key` in IFCX; a floor plan for anything else.
    pub fn from_key(key: &str) -> Self {
        Self::ALL.into_iter().find(|k| k.key() == key).unwrap_or_default()
    }

    /// Whether it looks up from its cut (a ceiling plan), not down.
    pub fn looks_up(self) -> bool {
        self == PlanKind::Ceiling
    }
}

/// A plan of a level: the model seen from above (or from below, a ceiling plan), cut at
/// [`PlanView::cut`] above its level.
#[derive(Debug, Clone, PartialEq)]
pub struct PlanView {
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
    /// Its dimensions, shown wherever it is placed.
    pub dimensions: Vec<Dimension>,
    /// Its texts, shown wherever it is placed.
    pub notes: Vec<Note>,
    /// The lines drawn on it that are no model: the shapes and leaders of its tags (x, y, metres).
    pub lines: Vec<[[f64; 2]; 2]>,
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
    /// Its dimensions, in its drawing's (u, v).
    pub dimensions: Vec<Dimension>,
    /// Its texts, in its drawing's (u, v).
    pub notes: Vec<Note>,
    /// The lines drawn on it that are no model: the shapes and leaders of its tags (u, v).
    pub lines: Vec<[[f64; 2]; 2]>,
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
        Self { name: Self::STANDARD.to_string(), size: NOTE_HEIGHT, tick: Tick::Slash, tick_size: 0.0012, extension: 0.0015, gap: 0.0015, unit: DimensionUnit::Millimetres, decimals: 0 }
    }

    /// The figure of a length (metres) in this style.
    pub fn figure(&self, length: f64) -> String {
        let decimals = self.decimals.min(6) as usize;
        match self.unit {
            DimensionUnit::Millimetres => format!("{:.*}", decimals, length * 1000.0),
            DimensionUnit::Metres => format!("{:.*}", decimals, length),
        }
    }

    fn to_ifcx(&self) -> Value {
        json!({"name": self.name, "size": self.size, "tick": self.tick.key(), "tick_size": self.tick_size, "extension": self.extension, "gap": self.gap, "unit": self.unit.key(), "decimals": self.decimals})
    }

    /// A style of an IFCX file; a missing or senseless value is the standard style's.
    pub fn from_ifcx(v: &Value) -> Option<Self> {
        let standard = Self::standard();
        let name = v.get("name")?.as_str()?.trim();
        let size = |key: &str, or: f64| v.get(key).and_then(Value::as_f64).filter(|s| s.is_finite() && (0.0..=0.1).contains(s)).unwrap_or(or);
        (!name.is_empty()).then(|| Self {
            name: name.to_string(),
            size: Some(size("size", standard.size)).filter(|s| *s > 0.0).unwrap_or(standard.size),
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dimension {
    pub from: [f64; 2],
    pub to: [f64; 2],
    /// Which of the document's dimension styles it has (0: the standard one).
    pub style: usize,
}

impl Dimension {
    /// Its length (metres).
    pub fn length(&self) -> f64 {
        (self.to[0] - self.from[0]).hypot(self.to[1] - self.from[1])
    }
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
}

/// The height of the letters of a new text on paper (metres).
pub const NOTE_HEIGHT: f64 = 0.0025;

/// A3 landscape, the paper of a new sheet (metres).
pub const A3_LANDSCAPE: [f64; 2] = [0.420, 0.297];
/// The ISO papers a sheet can have, landscape: name, width and height (metres).
pub const PAPERS: [(&str, [f64; 2]); 5] =
    [("A4", [0.297, 0.210]), ("A3", A3_LANDSCAPE), ("A2", [0.594, 0.420]), ("A1", [0.841, 0.594]), ("A0", [1.189, 0.841])];
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
    pub name: String,
    /// Number and name, as a tab shows them.
    pub title: String,
    /// Width and height (metres).
    pub paper: [f64; 2],
    pub views: Vec<PlacedView>,
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
                Level { path: uuid(&format!("level/{name}/{elevation}")), name: name.clone(), elevation, added: false }
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
        let dims = |d: &[Dimension]| d.iter().map(|d| if d.style == 0 { json!([d.from, d.to]) } else { json!([d.from, d.to, self.dimension_style(d.style).name]) }).collect::<Vec<_>>();
        let notes = |n: &[Note]| {
            let note = |n: &Note| {
                let mut note = json!({"at": n.at, "text": n.text, "size": n.size});
                if n.align != Align::Left {
                    note["align"] = n.align.key().into();
                }
                if n.angle != 0.0 {
                    note["angle"] = n.angle.into();
                }
                if let Some(font) = n.face.to_json() {
                    note["font"] = font;
                }
                note
            };
            n.iter().map(note).collect::<Vec<_>>()
        };
        let mut nodes = Vec::new();
        let mut children = serde_json::Map::new();
        for (i, p) in self.plans.iter().enumerate() {
            let level = self.level(&p.level).map(|l| l.name.clone()).unwrap_or_default();
            children.insert(format!("plan {}", i + 1), p.path.clone().into());
            let fills = |all: &[Fill]| all.iter().map(Fill::to_ifcx).collect::<Vec<_>>();
            let view = json!({
                "name": p.name, "kind": p.kind.key(), "level": level, "cut": p.cut, "depth": p.depth, "scale": p.scale, "crop": rect(p.crop), "dimensions": dims(&p.dimensions), "notes": notes(&p.notes), "lines": p.lines, "fills": fills(&p.fills), "layers": p.layers,
            });
            nodes.push(json!({"path": p.path, "attributes": {"o3v::view::plan": with_hidden(view, &p.hidden)}}));
        }
        for (i, s) in self.sections.iter().enumerate() {
            children.insert(format!("section {}", i + 1), s.path.clone().into());
            let fills = |all: &[Fill]| all.iter().map(Fill::to_ifcx).collect::<Vec<_>>();
            let view = json!({
                "name": s.name, "kind": s.kind.key(), "from": s.from, "to": s.to, "depth": s.depth, "scale": s.scale, "crop": rect(s.crop), "dimensions": dims(&s.dimensions), "notes": notes(&s.notes), "lines": s.lines, "fills": fills(&s.fills), "layers": s.layers,
            });
            nodes.push(json!({"path": s.path, "attributes": {"o3v::view::section": with_hidden(view, &s.hidden)}}));
        }
        for (i, v) in self.views_3d.iter().enumerate() {
            children.insert(format!("3d view {}", i + 1), v.path.clone().into());
            let c = &v.camera;
            let section = v.section.map_or(Value::Null, |b| json!({"min": b.min, "max": b.max, "turn": b.turn, "pivot": b.pivot}));
            let view = json!({
                "name": v.name, "camera": {"eye": c.eye, "target": c.target, "perspective": c.perspective, "fov": c.fov}, "box": section, "layers": v.layers,
            });
            nodes.push(json!({"path": v.path, "attributes": {"o3v::view::3d": with_hidden(view, &v.hidden)}}));
        }
        for (i, s) in self.sheets.iter().enumerate() {
            children.insert(format!("sheet {}", i + 1), s.path.clone().into());
            let views: Vec<Value> = s.views.iter().map(|v| json!({"view": v.plan, "at": v.at, "scale": v.scale})).collect();
            nodes.push(json!({"path": s.path, "attributes": {"o3v::sheet": {"number": s.number, "name": s.name, "paper": s.paper, "views": views}}}));
        }
        if !nodes.is_empty() {
            nodes.push(json!({"path": uuid("o3v/drawings"), "children": children, "attributes": {"o3v::drawings": {"version": 1, "dimension_styles": self.dimension_styles.0.iter().map(DimensionStyle::to_ifcx).collect::<Vec<_>>()}}}));
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
            v.and_then(Value::as_array).into_iter().flatten().filter_map(|d| Some(Dimension { from: pair(d.get(0)?)?, to: pair(d.get(1)?)?, style: style(d.get(2)) })).collect()
        };
        let notes = |v: Option<&Value>| -> Vec<Note> {
            let all = v.and_then(Value::as_array).into_iter().flatten();
            all.filter_map(|n| Some(Note { at: pair(n.get("at")?)?, text: n.get("text")?.as_str()?.to_string(), size: n.get("size").and_then(Value::as_f64).filter(|s| s.is_finite() && *s > 0.0).unwrap_or(NOTE_HEIGHT), align: n.get("align").and_then(Value::as_str).and_then(Align::from_key).unwrap_or_default(), angle: n.get("angle").and_then(Value::as_f64).filter(|a| a.is_finite()).unwrap_or(0.0), face: crate::typeface::Face::from_json(n.get("font")) })).filter(|n| !n.text.trim().is_empty()).collect()
        };
        let lines = |v: Option<&Value>| -> Vec<[[f64; 2]; 2]> {
            let all = v.and_then(Value::as_array).into_iter().flatten();
            all.filter_map(|l| Some([pair(l.get(0)?)?, pair(l.get(1)?)?])).filter(|l| l.iter().flatten().all(|x| x.is_finite())).collect()
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
                self.plans.push(PlanView {
                    path: path.clone(),
                    name: text(p, "name"),
                    level,
                    kind,
                    cut: number(p, "cut", cut),
                    depth: number(p, "depth", depth),
                    scale: scale(p),
                    dimensions: dims(p.get("dimensions")),
                    notes: notes(p.get("notes")),
                    lines: lines(p.get("lines")),
                    fills: Fill::from_ifcx(p.get("fills")),
                    crop: rect(p.get("crop")),
                    layers: layers(p.get("layers")),
                    hidden: hidden(p),
                });
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
                    dimensions: dims(s.get("dimensions")),
                    notes: notes(s.get("notes")),
                    lines: lines(s.get("lines")),
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
                Some(PlacedView { plan: view, at: pair(v.get("at")?)?, scale: number(v, "scale", SHEET_SCALE) })
            });
            let views: Vec<PlacedView> = views.collect();
            let (number, name) = (text(s, "number"), text(s, "name"));
            let paper = s.get("paper").and_then(pair).unwrap_or(A3_LANDSCAPE);
            self.sheets.push(Sheet { path: path.clone(), title: format!("{number} {name}"), number, name, paper, views });
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
        self.plans.push(PlanView { path: path.clone(), name, level: level.to_string(), kind, cut, depth, scale: SHEET_SCALE, dimensions: Vec::new(), notes: Vec::new(), lines: Vec::new(), fills: Vec::new(), crop: None, layers: LayerSwitches::new(), hidden: Arc::default() });
        Some(path)
    }

    /// The cut and depth a new plan of `kind` of the level at `level` gets: a ceiling plan looks
    /// up from its cut to the next level, else [`CEILING_DEPTH`].
    fn plan_defaults(&self, level: &str, kind: PlanKind) -> (f64, f64) {
        match kind {
            PlanKind::Floor => (PLAN_CUT, PLAN_DEPTH),
            PlanKind::Structural => (PLAN_CUT, STRUCTURAL_DEPTH),
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
        self.sections.push(SectionView { path: path.clone(), name: name.to_string(), kind, from, to, depth, scale: SHEET_SCALE, dimensions: Vec::new(), notes: Vec::new(), lines: Vec::new(), fills: Vec::new(), crop: None, layers: LayerSwitches::new(), hidden: Arc::default() });
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
        self.views_3d.push(View3d { path: path.clone(), name, camera, section, layers, hidden });
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
        let dimension = Dimension { from, to, style: 0 };
        if !(from.iter().chain(&to).all(|v| v.is_finite()) && dimension.length() >= 0.001) {
            return false;
        }
        let Some((dimensions, _)) = self.marks_mut(view) else { return false };
        dimensions.push(dimension);
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
        true
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

    /// The regions of a view filled in one colour that are no model, drawn under its lines.
    pub fn view_fills(&self, path: &str) -> &[Fill] {
        if let Some(p) = self.plan(path) {
            return &p.fills;
        }
        self.section(path).map_or(&[], |s| &s.fills[..])
    }

    /// The lines of a view that are no model: the shapes and leaders of its tags.
    pub fn view_lines(&self, path: &str) -> &[[[f64; 2]; 2]] {
        if let Some(p) = self.plan(path) {
            return &p.lines;
        }
        self.section(path).map_or(&[], |s| &s.lines[..])
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
        notes.push(Note { at, text: text.to_string(), size: NOTE_HEIGHT, align: Align::Left, angle: 0.0, face: Default::default() });
        true
    }

    /// Moves a view's text to another point of its drawing.
    pub fn move_note(&mut self, view: &str, index: usize, at: [f64; 2]) -> bool {
        if !at.iter().all(|v| v.is_finite()) {
            return false;
        }
        let Some(note) = self.notes_mut(view).and_then(|n| n.get_mut(index)) else { return false };
        note.at = at;
        true
    }

    /// Gives a view's text another wording; it keeps one.
    pub fn set_note_text(&mut self, view: &str, index: usize, text: &str) -> bool {
        if text.trim().is_empty() {
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
        self.sheets.push(Sheet { path: path.clone(), number, name: name.to_string(), title, paper: A3_LANDSCAPE, views: Vec::new() });
        path
    }

    /// Removes the sheet at `path`.
    pub fn remove_sheet(&mut self, path: &str) -> bool {
        let before = self.sheets.len();
        self.sheets.retain(|s| s.path != path);
        self.sheets.len() < before
    }

    /// Places a view (a floor plan, section or elevation) in the middle of a sheet at 1:100 and
    /// gives its index on the sheet.
    pub fn place_plan(&mut self, sheet: &str, plan: &str) -> Option<usize> {
        self.view_name(plan)?;
        let sheet = self.sheets.iter_mut().find(|s| s.path == sheet)?;
        let at = [sheet.paper[0] / 2.0, sheet.paper[1] / 2.0];
        sheet.views.push(PlacedView { plan: plan.to_string(), at, scale: SHEET_SCALE });
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
        assert_eq!(document.plan(&plan).unwrap().dimensions[1], Dimension { from: [3.0, 4.0], to: [0.0, 0.0], style: 0 });
        assert!(!document.flip_dimension(&plan, 9));
        assert!(document.remove_dimension(&plan, 0));
        assert!(!document.remove_dimension(&plan, 5));
        assert_eq!(document.plan(&plan).unwrap().dimensions.len(), 1);
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
        assert_eq!(notes, [Note { at: [1.0, 2.0], text: "Kitchen".into(), size: NOTE_HEIGHT, align: Align::Left, angle: 0.0, face: Default::default() }, Note { at: [5.0, 2.5], text: "Hall 2".into(), size: NOTE_HEIGHT, align: Align::Left, angle: 0.0, face: Default::default() }]);
        assert!(document.remove_note(&plan, 0));
        assert!(!document.remove_note(&plan, 3));
        // saved with the drawings and taken over again
        let nodes: Vec<(String, Value)> =
            document.ifcx_drawing_nodes().into_iter().map(|n| (n["path"].as_str().unwrap().to_string(), n["attributes"].clone())).collect();
        let mut again = Document::from_model(&model_with(&[("00", 0.0)]));
        again.adopt_ifcx(&nodes);
        assert_eq!(again.view_notes(&plan).unwrap(), [Note { at: [5.0, 2.5], text: "Hall 2".into(), size: NOTE_HEIGHT, align: Align::Left, angle: 0.0, face: Default::default() }]);
        assert_eq!(again.view_notes(&section).unwrap(), [Note { at: [0.5, 3.0], text: "Roof +3.000".into(), size: NOTE_HEIGHT, align: Align::Left, angle: 0.0, face: Default::default() }]);
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
