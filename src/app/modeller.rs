//! The modeller: the five base elements of `bim-core` (point, linear, planar, system and
//! connection) placed with tools on a work plane, and shown as a model of their own.
//!
//! The elements are kept in [`Modeller::model`] as definitions in tile frames. After every change
//! they are evaluated again and shown as one viewer model (a layer per IFC class), which is an
//! entry of `App::models`: selected, hidden, measured and cut like any other model. Its display
//! coordinates are those of the scene it is modelled in.

use super::{App, ModelEntry};
use crate::model::{Layer, Model};
use crate::selection::Pickable;
use bim_core::element::PlanarForm;
use bim_core::{ConnectionDef, ConnectionKind, Element, ElementId, LinearDef, Mesh, PlanarDef, PointDef, ProfileDef, Refusal, Shape, SystemDef};
use glam::{DVec2, DVec3, Vec3};
use openaec_ui::Language;
use std::path::PathBuf;
use std::sync::Arc;

/// A tool that places a base element with clicks on the work plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelTool {
    Point,
    Column,
    Beam,
    Wall,
    Slab,
    System,
}

impl ModelTool {
    pub const ALL: [ModelTool; 6] = [ModelTool::Wall, ModelTool::Slab, ModelTool::Column, ModelTool::Beam, ModelTool::System, ModelTool::Point];

    /// Its command id.
    pub fn id(self) -> &'static str {
        match self {
            ModelTool::Point => "model.point",
            ModelTool::Column => "model.column",
            ModelTool::Beam => "model.beam",
            ModelTool::Wall => "model.wall",
            ModelTool::Slab => "model.slab",
            ModelTool::System => "model.system",
        }
    }

    /// Its name on the ribbon, which is also the name of what it places.
    pub fn label(self, l: Language) -> &'static str {
        match self {
            ModelTool::Point => l.text("Object", "Object"),
            ModelTool::Column => l.text("Kolom", "Column"),
            ModelTool::Beam => l.text("Balk", "Beam"),
            ModelTool::Wall => l.text("Wand", "Wall"),
            ModelTool::Slab => l.text("Vloer", "Slab"),
            ModelTool::System => l.text("Vliesgevel", "Curtain wall"),
        }
    }

    /// Its ribbon icon.
    pub fn icon(self) -> &'static str {
        match self {
            ModelTool::Point => "draw.coordinate",
            ModelTool::Column => "pile.columns",
            ModelTool::Beam => "draw.line",
            ModelTool::Wall => "construction.types",
            ModelTool::Slab => "draw.filled-area",
            ModelTool::System => "curtain.new",
        }
    }

    /// The base element it places.
    pub fn base(self) -> bim_core::BaseKind {
        use bim_core::BaseKind;
        match self {
            ModelTool::Point => BaseKind::Point,
            ModelTool::Column | ModelTool::Beam => BaseKind::Linear,
            ModelTool::Wall | ModelTool::Slab => BaseKind::Planar,
            ModelTool::System => BaseKind::System,
        }
    }

    /// The clicks it takes.
    fn clicks(self) -> usize {
        match self {
            ModelTool::Point | ModelTool::Column => 1,
            ModelTool::Beam | ModelTool::Wall | ModelTool::Slab | ModelTool::System => 2,
        }
    }

    /// Whether the next element starts where the last one ended.
    fn chains(self) -> bool {
        matches!(self, ModelTool::Wall | ModelTool::Beam | ModelTool::System)
    }

    /// What to click next, `placed` points in.
    pub fn hint(self, l: Language, placed: usize) -> &'static str {
        match (self, placed) {
            (ModelTool::Point, _) => l.text("Klik waar het object staat · Esc: stoppen", "Click where the object stands · Esc: stop"),
            (ModelTool::Column, _) => l.text("Klik waar de kolom staat · Esc: stoppen", "Click where the column stands · Esc: stop"),
            (ModelTool::Slab, 0) => l.text("Klik een hoek van de vloer · Esc: stoppen", "Click a corner of the slab · Esc: stop"),
            (ModelTool::Slab, _) => l.text("Klik de andere hoek · Esc: opnieuw", "Click the opposite corner · Esc: start again"),
            (_, 0) => l.text("Klik het beginpunt · Esc: stoppen", "Click the start point · Esc: stop"),
            (_, _) => l.text("Klik het eindpunt · Esc: opnieuw", "Click the end point · Esc: start again"),
        }
    }
}

/// Sizes the tools give what they place (metres).
const WALL_THICKNESS: f64 = 0.2;
const SLAB_THICKNESS: f64 = 0.25;
const COLUMN: f64 = 0.3;
const BEAM: (f64, f64) = (0.2, 0.4);
const OBJECT: DVec3 = DVec3::new(0.6, 0.6, 0.9);
const SYSTEM_SPACING: DVec2 = DVec2::new(1.5, 1.5);
const SYSTEM_MEMBER: DVec2 = DVec2::new(0.05, 0.15);
const SYSTEM_PANEL: f64 = 0.02;
/// The height of what stands on the work plane when no level lies above it.
const STOREY: f64 = 3.0;
/// Clicks within this of a point of an element (logical pixels) take that point.
const SNAP_PIXELS: f32 = 12.0;
/// A second point within this angle of the first point's horizontal or vertical is put on it.
const ORTHO_DEGREES: f64 = 4.0;
/// Clicks off every snap are rounded to this (metres).
const ROUND: f64 = 0.01;

/// The modeller's state.
#[derive(Debug, Clone, Default)]
pub struct Modeller {
    /// The modelled elements.
    pub model: bim_core::Model,
    /// The tool placing elements, and the name its elements get (in the interface language).
    pub tool: Option<(ModelTool, &'static str)>,
    /// The points clicked so far for the tool (scene coordinates).
    pub points: Vec<DVec3>,
    /// The viewer model that shows the elements (its id), an entry of `App::models`.
    pub shown: Option<u64>,
    /// Why the last step was refused.
    pub refusal: Option<Refusal>,
    /// Edit mode (Tab) on an element.
    pub edit: Option<super::edit::Edit>,
}

impl App {
    /// Takes over the elements an IFCX file this viewer wrote defines (`model` was just loaded),
    /// and shows them; a definition that cannot be read is left out.
    pub(super) fn adopt_elements(&mut self, model: &Model) {
        if model.project.elements.is_empty() {
            return;
        }
        for definition in &model.project.elements {
            if let Ok(element) = serde_json::from_value::<Element>(definition.clone()) {
                self.modeller.model.insert(element);
            }
        }
        self.show_modelled();
        self.fit();
    }

    /// An empty model to model in, with a first level at 0 named `level`; the model shown before
    /// (if any) goes.
    pub(super) fn new_model(&mut self, level: &str) {
        let empty = Arc::new(Model::new(PathBuf::from("model.ifcx"), "IFCX", Vec::new()));
        self.set_model(empty.clone());
        self.modeller = Modeller { shown: Some(empty.id), ..Modeller::default() };
        let _ = self.document.add_level(level, 0.0);
        self.grid = true;
        // nothing to fit yet: look at the work plane from above and aside, 20 m around
        self.camera.target = Vec3::ZERO;
        self.camera.scene_radius = 20.0;
        self.camera.distance = 45.0;
    }

    /// Switches a tool on (its elements named `name`), or off.
    pub(super) fn set_model_tool(&mut self, tool: Option<(ModelTool, &'static str)>) {
        let same = tool.map(|t| t.0) == self.modeller.tool.map(|t| t.0);
        self.modeller.tool = if same { None } else { tool };
        self.modeller.points.clear();
        self.modeller.refusal = None;
        if self.modeller.tool.is_some() {
            self.measure.tool = None;
            self.plane_picking = false;
        }
    }

    /// Esc while placing: drop the points clicked, else switch the tool off. False when no tool
    /// was on.
    pub(super) fn escape_model_tool(&mut self) -> bool {
        if self.modeller.tool.is_none() {
            return false;
        }
        self.modeller.refusal = None;
        if self.modeller.points.is_empty() {
            self.modeller.tool = None;
        } else {
            self.modeller.points.clear();
        }
        true
    }

    /// The height of the work plane and how high a storey reaches from it: the level of the floor
    /// plan shown, else the level nearest 0; the storey up to the next level, else [`STOREY`].
    pub(super) fn work_plane(&self) -> (f64, f64, Option<String>) {
        let levels = self.document.levels();
        let shown = self.active_plan.as_deref().and_then(|p| self.document.plan(p)).and_then(|plan| self.document.level(&plan.level));
        let level = shown.or_else(|| levels.iter().min_by(|a, b| a.elevation.abs().total_cmp(&b.elevation.abs())));
        let Some(level) = level else { return (0.0, STOREY, None) };
        let above = levels.iter().map(|l| l.elevation).filter(|&z| z > level.elevation + 0.1).fold(f64::INFINITY, f64::min);
        let height = if above.is_finite() { above - level.elevation } else { STOREY };
        (level.elevation, height, Some(level.path.clone()))
    }

    /// The point of the work plane under the cursor (scene coordinates).
    fn plane_point(&self, x: f32, y: f32, width: f32, height: f32, z: f64) -> Option<DVec3> {
        let inv = (self.camera.view_proj(width / height) * self.model_matrix()).inverse();
        let (nx, ny) = (2.0 * x / width - 1.0, 1.0 - 2.0 * y / height);
        let a = inv.project_point3(Vec3::new(nx, ny, 0.0)).as_dvec3();
        let b = inv.project_point3(Vec3::new(nx, ny, 1.0)).as_dvec3();
        let d = b - a;
        if d.z.abs() < 1e-12 {
            return None;
        }
        let t = (z - a.z) / d.z;
        (t >= 0.0).then(|| a + d * t)
    }

    /// Where a scene point shows in the viewport (logical pixels), if in front of the camera.
    pub(super) fn on_screen(&self, p: DVec3, width: f32, height: f32) -> Option<(f32, f32)> {
        let c = (self.camera.view_proj(width / height) * self.model_matrix()).project_point3(p.as_vec3());
        ((0.0..=1.0).contains(&c.z)).then(|| ((c.x + 1.0) / 2.0 * width, (1.0 - c.y) / 2.0 * height))
    }

    /// The points of the modelled elements a click snaps to (scene coordinates): ends, corners
    /// and placements.
    fn snap_points(&self) -> Vec<DVec3> {
        self.snap_points_except(ElementId(0))
    }

    /// The points a click snaps to, of every modelled element but `except` (ids start at 1).
    pub(super) fn snap_points_except(&self, except: ElementId) -> Vec<DVec3> {
        let grid = self.modeller.model.grid;
        let mut points = Vec::new();
        for e in self.modeller.model.iter().filter(|e| e.id != except) {
            match e.shape_in_project(&grid) {
                Shape::Point(p) => points.push(p.at),
                Shape::Linear(l) => points.extend([l.start, l.end]),
                Shape::Planar(PlanarDef { form: PlanarForm::Wall { start, end, base, .. }, .. }) => points.extend([start.extend(base), end.extend(base)]),
                Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, top, .. }, .. }) => points.extend(outline.iter().map(|p| p.extend(top))),
                Shape::System(s) => points.extend([s.start.extend(s.base), s.end.extend(s.base)]),
                Shape::Connection(_) => {}
            }
        }
        points
    }

    /// A click on the work plane at `p`: onto a point of an element near the cursor, else in line
    /// with the last point when nearly so, else rounded.
    fn snapped(&self, p: DVec3, x: f32, y: f32, width: f32, height: f32) -> DVec3 {
        let near = self
            .snap_points()
            .into_iter()
            .filter_map(|q| self.on_screen(q, width, height).map(|s| (q, (s.0 - x).hypot(s.1 - y))))
            .filter(|(_, d)| *d <= SNAP_PIXELS)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((q, _)) = near {
            return DVec3::new(q.x, q.y, p.z);
        }
        let round = |v: f64| (v / ROUND).round() * ROUND;
        let mut p = DVec3::new(round(p.x), round(p.y), p.z);
        if let Some(first) = self.modeller.points.last() {
            let d = p - *first;
            let angle = d.y.atan2(d.x).to_degrees().rem_euclid(90.0);
            if angle < ORTHO_DEGREES || angle > 90.0 - ORTHO_DEGREES {
                if d.x.abs() >= d.y.abs() {
                    p.y = first.y;
                } else {
                    p.x = first.x;
                }
            }
        }
        p
    }

    /// A click in the viewport while a tool is on.
    pub(super) fn model_click(&mut self, x: f32, y: f32, width: f32, height: f32) {
        let Some((tool, name)) = self.modeller.tool else { return };
        let (z, storey, level) = self.work_plane();
        let Some(p) = self.plane_point(x, y, width, height, z) else { return };
        let p = self.snapped(p, x, y, width, height);
        self.modeller.points.push(p);
        self.modeller.refusal = None;
        if self.modeller.points.len() < tool.clicks() {
            return;
        }
        let points = std::mem::take(&mut self.modeller.points);
        let (a, b) = (points[0], *points.last().unwrap());
        let shape = match tool {
            ModelTool::Point => Shape::Point(PointDef { at: a, rotation: 0.0, size: OBJECT }),
            ModelTool::Column => Shape::Linear(LinearDef { start: a, end: a + DVec3::Z * storey, profile: ProfileDef::Rectangle { width: COLUMN, height: COLUMN }, roll: 0.0 }),
            // a beam's top lies at the top of the storey
            ModelTool::Beam => {
                let up = DVec3::Z * (storey - BEAM.1 / 2.0);
                Shape::Linear(LinearDef { start: a + up, end: b + up, profile: ProfileDef::Rectangle { width: BEAM.0, height: BEAM.1 }, roll: 0.0 })
            }
            ModelTool::Wall => Shape::Planar(PlanarDef {
                form: PlanarForm::Wall { start: a.truncate(), end: b.truncate(), base: z, height: storey },
                thickness: WALL_THICKNESS,
                openings: Vec::new(),
            }),
            ModelTool::Slab => {
                let (lo, hi) = (a.truncate().min(b.truncate()), a.truncate().max(b.truncate()));
                let outline = vec![lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
                Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, holes: Vec::new(), top: z }, thickness: SLAB_THICKNESS, openings: Vec::new() })
            }
            ModelTool::System => Shape::System(SystemDef {
                start: a.truncate(),
                end: b.truncate(),
                base: z,
                height: storey,
                spacing: SYSTEM_SPACING,
                member: SYSTEM_MEMBER,
                panel: SYSTEM_PANEL,
            }),
        };
        match self.add_element(name, level, shape) {
            Ok(_) if tool.chains() => self.modeller.points = vec![b],
            Ok(_) => {}
            Err(refusal) => self.modeller.refusal = Some(refusal),
        }
    }

    /// Adds an element defined in scene coordinates, named `name` and a number, and shows it.
    pub(super) fn add_element(&mut self, name: &str, level: Option<String>, shape: Shape) -> Result<ElementId, Refusal> {
        let grid = self.modeller.model.grid;
        let mut element = Element::in_project(&grid, name, level, shape);
        // numbered per IFC class: Wall 1, Wall 2, Slab 1
        let number = self.modeller.model.iter().filter(|e| e.class() == element.class()).count() + 1;
        element.name = format!("{name} {number}");
        let id = self.modeller.model.add(element)?;
        self.show_modelled();
        Ok(id)
    }

    /// The modelled elements among the selection, in pick order.
    pub(crate) fn selected_modelled(&self) -> Vec<ElementId> {
        let (Some(model), Some(shown)) = (&self.model, self.modeller.shown) else { return Vec::new() };
        let Some(entry) = self.models.iter().position(|e| e.model.id == shown) else { return Vec::new() };
        // a model shown alone keeps its own ids
        let entry = if self.models.len() == 1 { 0 } else { entry as u32 };
        self.sel
            .items()
            .iter()
            .filter_map(|item| match item {
                Pickable::Element(id) => {
                    let key = model.element_key(*id);
                    (key.model == entry).then_some(ElementId(key.source))
                }
                Pickable::Layer(_) => None,
            })
            .filter(|id| self.modeller.model.get(*id).is_some())
            .collect()
    }

    /// How many modelled linear elements are selected.
    pub(crate) fn selected_linear(&self) -> usize {
        self.selected_modelled().into_iter().filter(|id| self.modeller.model.get(*id).is_some_and(|e| e.base() == bim_core::BaseKind::Linear)).count()
    }

    /// How many modelled elements with a body (not connections) are selected.
    pub(crate) fn selected_bodies(&self) -> usize {
        self.selected_modelled().into_iter().filter(|id| self.modeller.model.get(*id).is_some_and(|e| e.base() != bim_core::BaseKind::Connection)).count()
    }

    /// Joins the two linear elements selected: the first is the primary.
    pub(super) fn connect_selected(&mut self, kind: ConnectionKind, name: &str) {
        // a join takes two linear elements; a cut any two with a body, the first cutting the second
        let fits = |e: &Element| match kind {
            ConnectionKind::Cut => e.base() != bim_core::BaseKind::Connection,
            ConnectionKind::Butt | ConnectionKind::Mitre => e.base() == bim_core::BaseKind::Linear,
        };
        let picked: Vec<ElementId> = self.selected_modelled().into_iter().filter(|id| self.modeller.model.get(*id).is_some_and(fits)).collect();
        let [primary, secondary] = picked[..] else {
            self.modeller.refusal = Some(if kind == ConnectionKind::Cut { Refusal::NoBody } else { Refusal::NotLinear });
            return;
        };
        let level = self.modeller.model.get(primary).and_then(|e| e.level.clone());
        if let Err(refusal) = self.add_element(name, level, Shape::Connection(ConnectionDef { kind, primary, secondary })) {
            self.modeller.refusal = Some(refusal);
        }
    }

    /// Removes the modelled elements selected (and the connections that join them). False when
    /// none was selected.
    pub(super) fn delete_modelled(&mut self) -> bool {
        let picked = self.selected_modelled();
        if picked.is_empty() {
            return false;
        }
        for id in picked {
            self.modeller.model.remove(id);
        }
        if self.modeller.edit.as_ref().is_some_and(|e| self.modeller.model.get(e.element).is_none()) {
            self.modeller.edit = None;
        }
        self.show_modelled();
        true
    }

    /// Shows the modelled elements again after a change: their viewer model takes the place of
    /// the one shown before, and the scene is put together again without moving the camera.
    pub(super) fn show_modelled(&mut self) {
        let levels: Vec<(String, String)> = self.document.levels().iter().map(|l| (l.path.clone(), l.name.clone())).collect();
        let built = Arc::new(viewer_model(&self.modeller.model, &levels));
        let before = self.modeller.shown.replace(built.id);
        if self.models.is_empty() {
            if let Some(m) = self.model.clone().filter(|m| Some(m.id) != before) {
                self.models.push(ModelEntry::new(m, 0));
            }
        }
        match before.and_then(|id| self.models.iter().position(|e| e.model.id == id)) {
            Some(k) => {
                let old = std::mem::replace(&mut self.models[k], ModelEntry::new(built, 0));
                // its layers keep their switches by name; new ones show
                let switched: std::collections::HashMap<&str, bool> = old.model.layers.iter().map(|l| l.name.as_str()).zip(old.layer_visibility.iter().copied()).collect();
                let entry = &mut self.models[k];
                entry.visible = old.visible;
                for (layer, shown) in entry.model.layers.iter().zip(entry.layer_visibility.iter_mut()) {
                    if let Some(&s) = switched.get(layer.name.as_str()) {
                        *shown = s;
                    }
                }
            }
            None => self.models.push(ModelEntry::new(built, 0)),
        }
        let mut first = 0;
        for entry in &mut self.models {
            entry.layers = first..first + entry.model.layers.len();
            first = entry.layers.end;
        }
        let scene = if self.models.len() == 1 {
            self.models[0].model.clone()
        } else {
            let sources: Vec<Arc<Model>> = self.models.iter().map(|e| e.model.clone()).collect();
            Arc::new(Model::combine(&sources))
        };
        self.replace_scene(scene);
    }

    /// Puts `model` in the place of the scene shown, keeping the camera, the views and the
    /// document: what follows the scene's layers and ids is built again.
    fn replace_scene(&mut self, model: Arc<Model>) {
        self.model = Some(model.clone());
        self.visible = model.layers.iter().map(|l| !l.hidden).collect();
        self.restore_model_visibility();
        self.sel.clear();
        self.selected = None;
        self.selection.clear();
        self.pick_index = None;
        self.hover_scene = None;
        self.measure.forget_edges();
        self.cuts.clear();
        self.explode_index = Some(Arc::new(crate::explode::ExplodeIndex::new(&model)));
        self.update_explode();
        if let Some(b) = self.world_bounds() {
            self.camera.scene_radius = self.camera.scene_radius.max(b.radius());
        }
    }
}

#[cfg(test)]
impl App {
    /// A small building of all five base elements, for renders: a slab, four walls (one with a
    /// door and a window), columns and beams joined butt and mitre, a curtain wall and an object.
    pub(crate) fn add_demo_building(&mut self) {
        use bim_core::Opening;
        self.new_model("Level 0");
        let _ = self.document.add_level("Level 1", 3.0);
        let level = self.work_plane().2;
        let p = DVec2::new;
        let add = |app: &mut App, name: &str, shape: Shape| app.add_element(name, level.clone(), shape).expect("the demo evaluates");
        let slab = PlanarForm::Slab { outline: vec![p(-0.1, -0.1), p(10.1, -0.1), p(10.1, 6.1), p(-0.1, 6.1)], holes: Vec::new(), top: 0.0 };
        add(self, "Slab", Shape::Planar(PlanarDef { form: slab, thickness: SLAB_THICKNESS, openings: Vec::new() }));
        let wall = |a: DVec2, b: DVec2, openings: Vec<Opening>| Shape::Planar(PlanarDef { form: PlanarForm::Wall { start: a, end: b, base: 0.0, height: 3.0 }, thickness: WALL_THICKNESS, openings });
        let door = Opening { along: 1.0, sill: 0.0, width: 1.0, height: 2.3 };
        let window = Opening { along: 3.5, sill: 0.9, width: 2.4, height: 1.5 };
        add(self, "Wall", wall(p(0.0, 0.0), p(10.0, 0.0), vec![door, window]));
        let side = add(self, "Wall", wall(p(10.0, 0.0), p(10.0, 6.0), Vec::new()));
        add(self, "Wall", wall(p(0.0, 6.0), p(0.0, 0.0), Vec::new()));
        add(self, "Curtain wall", Shape::System(SystemDef { start: p(10.0, 6.0), end: p(0.0, 6.0), base: 0.0, height: 3.0, spacing: SYSTEM_SPACING, member: SYSTEM_MEMBER, panel: SYSTEM_PANEL }));
        // a pergola beside it: columns, a beam stopping at a column, two beams in a mitre
        let column = |at: DVec2| Shape::Linear(LinearDef { start: at.extend(0.0), end: at.extend(3.0), profile: ProfileDef::Rectangle { width: COLUMN, height: COLUMN }, roll: 0.0 });
        let c1 = add(self, "Column", column(p(13.0, 0.0)));
        add(self, "Column", column(p(13.0, 6.0)));
        let beam = |a: DVec2, b: DVec2| Shape::Linear(LinearDef { start: a.extend(2.8), end: b.extend(2.8), profile: ProfileDef::Rectangle { width: BEAM.0, height: BEAM.1 }, roll: 0.0 });
        let b1 = add(self, "Beam", beam(p(13.0, 6.0), p(13.0, 0.0)));
        let b2 = add(self, "Beam", beam(p(13.0, 6.0), p(17.0, 6.0)));
        let b3 = add(self, "Beam", beam(p(13.0, 0.0), p(17.0, 0.0)));
        add(self, "Connection", Shape::Connection(ConnectionDef { kind: ConnectionKind::Mitre, primary: b1, secondary: b2 }));
        add(self, "Connection", Shape::Connection(ConnectionDef { kind: ConnectionKind::Butt, primary: c1, secondary: b3 }));
        add(self, "Object", Shape::Point(PointDef { at: DVec3::new(7.0, 3.0, 0.0), rotation: 0.4, size: OBJECT }));
        // a beam through the side wall, cutting its way
        let through = add(self, "Beam", beam(p(8.5, 3.0), p(13.0, 3.0)));
        add(self, "Cut-out", Shape::Connection(ConnectionDef { kind: ConnectionKind::Cut, primary: through, secondary: side }));
        let _ = self.update(super::Message::Fit);
    }

}

/// The colour of a layer of modelled elements, by IFC class (sRGB).
fn class_colour(class: &str) -> [f32; 3] {
    match class {
        "IfcWall" => [0.86, 0.84, 0.80],
        "IfcSlab" => [0.70, 0.70, 0.72],
        "IfcColumn" | "IfcBeam" => [0.55, 0.62, 0.72],
        "IfcMember" => [0.45, 0.47, 0.52],
        "IfcPlate" => [0.62, 0.80, 0.92],
        "IfcCurtainWall" => [0.62, 0.80, 0.92],
        _ => [0.90, 0.60, 0.30],
    }
}

/// The viewer model of modelled elements: one layer per IFC class, every element's parts as its
/// triangles (each part in the colour of its own class: a curtain wall's members and panels), its
/// definition as its properties. A refused element shows nothing.
/// `levels` names the levels by path.
pub fn viewer_model(model: &bim_core::Model, levels: &[(String, String)]) -> Model {
    let grid = model.grid;
    let mut layers: Vec<Layer> = Vec::new();
    for element in model.iter() {
        let Ok(parts) = model.evaluate(element.id) else { continue };
        if parts.is_empty() {
            continue;
        }
        let class = element.class();
        let k = match layers.iter().position(|l| l.name == class) {
            Some(k) => k,
            None => {
                layers.push(Layer { name: class.into(), color: class_colour(class), ..Layer::default() });
                layers.len() - 1
            }
        };
        let layer = &mut layers[k];
        let first = layer.indices.len() as u32;
        for part in &parts {
            let Ok(mesh) = Mesh::of(&part.solid) else { continue };
            // tile-local f64 to the scene's f32, through the tile origin in f64
            let origin = grid.origin(part.tile);
            let base = (layer.positions.len() / 3) as u32;
            let colour = class_colour(part.class).map(|c| (c * 255.0).round() as u8);
            let colours = layer.colors.get_or_insert_with(Vec::new);
            colours.extend(std::iter::repeat_n([colour[0], colour[1], colour[2], 255], mesh.positions.len()));
            for (p, n) in mesh.positions.iter().zip(&mesh.normals) {
                layer.positions.extend((origin + *p).as_vec3().to_array());
                layer.normals.extend(n.as_vec3().to_array());
            }
            layer.indices.extend(mesh.indices.iter().map(|i| i + base));
        }
        let count = layer.indices.len() as u32 - first;
        layer.elements.push((element.id.0, first, count));
        let level = element.level.as_deref().and_then(|path| levels.iter().find(|(p, _)| p == path)).map(|(_, name)| name.as_str());
        layer.props.push((element.id.0, properties(model, element, &parts, level)));
    }
    Model::new(PathBuf::from("model.ifcx"), "IFCX", layers)
}

/// An element's properties as a flat JSON object.
fn properties(model: &bim_core::Model, e: &Element, parts: &[bim_core::Part], level: Option<&str>) -> String {
    let grid = model.grid;
    let m = |v: f64| format!("{v:.3} m");
    let mut props = serde_json::Map::new();
    let mut put = |k: &str, v: String| {
        props.insert(k.into(), serde_json::Value::String(v));
    };
    put("Name", e.name.clone());
    put("class", e.class().into());
    put("Base element", e.base().name().into());
    put("Tile", format!("{}, {}", e.tile.x, e.tile.y));
    put("id", e.id.0.to_string());
    if let Some(level) = level {
        put("level", level.into());
    }
    // the definitions an IFCX file keeps and gives back: the element's, and those of the
    // connections it is the primary member of (a connection has no body, so no node of its own)
    let joints = model.iter().filter(|c| matches!(&c.shape, Shape::Connection(j) if j.primary == e.id));
    let definitions: Vec<&Element> = std::iter::once(e).chain(joints).collect();
    put(crate::export::ifcx::DEFINITION, serde_json::to_string(&definitions).unwrap_or_default());
    match e.shape_in_project(&grid) {
        Shape::Point(p) => {
            put("Size", format!("{} × {} × {}", m(p.size.x), m(p.size.y), m(p.size.z)));
        }
        Shape::Linear(l) => {
            put("Profile", l.profile.name());
            put("Length", m((l.end - l.start).length()));
        }
        Shape::Planar(p) => {
            put("Thickness", m(p.thickness));
            if let PlanarForm::Wall { start, end, height, .. } = p.form {
                put("Length", m((end - start).length()));
                put("Height", m(height));
            }
        }
        Shape::System(s) => {
            put("Length", m((s.end - s.start).length()));
            put("Height", m(s.height));
            put("Parts", parts.len().to_string());
        }
        Shape::Connection(c) => {
            let kind = match c.kind {
                ConnectionKind::Butt => "Butt",
                ConnectionKind::Mitre => "Mitre",
                ConnectionKind::Cut => "Cut",
            };
            put("Kind", kind.into());
            put("Primary", c.primary.0.to_string());
            put("Secondary", c.secondary.0.to_string());
        }
    }
    let volume: f64 = parts.iter().map(|p| p.solid.volume()).sum();
    if volume > 0.0 {
        put("Volume", format!("{volume:.3} m³"));
    }
    serde_json::Value::Object(props).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport::Navigate;
    use crate::app::Message;

    /// An app with a new model, seen from the top, the viewport 1000 × 800 with the origin in
    /// its middle and 10 px per metre.
    fn modelling() -> App {
        let mut app = App::default();
        app.new_model("Level 0");
        app.viewport_size = (1000.0, 800.0);
        app.camera.set_preset(crate::viewport::camera::ViewPreset::Top);
        app.camera.ortho = true;
        app.camera.target = Vec3::ZERO;
        // ortho framing: half height = distance · tan(fov / 2) = 40 m for 400 px
        app.camera.distance = 40.0 / (app.camera.fov_y * 0.5).tan();
        app
    }

    /// Clicks at scene point (x, y) on the work plane.
    fn click(app: &mut App, x: f64, y: f64) {
        let (sx, sy) = app.on_screen(DVec3::new(x, y, 0.0), 1000.0, 800.0).unwrap();
        let _ = app.update(Message::Navigate(Navigate::Pick { x: sx, y: sy, width: 1000.0, height: 800.0, add: false }));
    }

    #[test]
    fn a_new_model_has_a_level_and_shows_the_viewport() {
        let app = modelling();
        assert!(app.model.is_some());
        assert_eq!(app.document.levels().len(), 1);
        assert_eq!(app.work_plane().0, 0.0);
    }

    #[test]
    fn two_clicks_place_a_wall_and_the_next_one_starts_at_its_end() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 5.0, 0.0);
        assert_eq!(app.modeller.model.len(), 1);
        let wall = app.modeller.model.iter().next().unwrap();
        assert_eq!(wall.name, "Wall 1");
        assert_eq!(wall.class(), "IfcWall");
        assert_eq!(app.modeller.points.len(), 1, "chained: the next wall starts here");
        click(&mut app, 5.0, 4.0);
        assert_eq!(app.modeller.model.len(), 2);
        // shown: one layer of walls with two elements
        let scene = app.model.as_ref().unwrap();
        let walls = scene.layers.iter().find(|l| l.name.ends_with("IfcWall")).expect("a wall layer");
        assert_eq!(walls.elements.len(), 2);
        assert!(app.escape_model_tool() && app.modeller.tool.is_some(), "Esc drops the points first");
        assert!(app.escape_model_tool() && app.modeller.tool.is_none(), "then the tool");
    }

    #[test]
    fn clicks_near_an_end_snap_to_it_and_nearly_straight_lines_straighten() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Beam, "Beam")));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 6.0, 0.15);
        let Shape::Linear(l) = app.modeller.model.iter().next().unwrap().shape_in_project(&app.modeller.model.grid) else { panic!() };
        assert_eq!((l.end - l.start).truncate(), DVec2::new(6.0, 0.0), "in line with the first point");
        app.set_model_tool(Some((ModelTool::Column, "Column")));
        // 0.4 m off the beam's end is 4 px on screen: it snaps
        click(&mut app, 6.3, 0.2);
        let column = app.modeller.model.iter().last().unwrap().shape_in_project(&app.modeller.model.grid);
        let Shape::Linear(c) = column else { panic!() };
        assert_eq!(c.start.truncate(), DVec2::new(6.0, 0.0));
        assert_eq!(app.modeller.model.iter().last().unwrap().class(), "IfcColumn");
    }

    #[test]
    fn every_tool_places_its_base_element() {
        let mut app = modelling();
        for (i, tool) in ModelTool::ALL.into_iter().enumerate() {
            app.set_model_tool(Some((tool, "E")));
            let y = 10.0 * i as f64;
            click(&mut app, 0.0, y);
            if tool.clicks() == 2 {
                click(&mut app, 6.0, y + 3.0);
            }
            app.set_model_tool(None);
        }
        let bases: std::collections::HashSet<_> = app.modeller.model.iter().map(|e| e.base()).collect();
        assert_eq!(bases.len(), 4, "point, linear, planar, system");
        assert!(app.modeller.refusal.is_none(), "{:?}", app.modeller.refusal);
    }

    #[test]
    fn two_selected_members_join_and_delete_takes_the_joint_along() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Beam, "Beam")));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 4.0, 0.0);
        click(&mut app, 4.0, 3.0);
        app.set_model_tool(None);
        let ids: Vec<u64> = app.modeller.model.iter().map(|e| e.id.0).collect();
        let scene = app.model.clone().unwrap();
        for id in &ids {
            let shown = scene.element_with_key(crate::model::ElementKey { model: if app.models.len() == 1 { 0 } else { (app.models.len() - 1) as u32 }, source: *id }).unwrap();
            app.sel.toggle(Pickable::Element(shown));
        }
        app.connect_selected(ConnectionKind::Mitre, "Joint");
        assert!(app.modeller.refusal.is_none(), "{:?}", app.modeller.refusal);
        assert_eq!(app.modeller.model.len(), 3);
        // select the first beam again and delete it: its joint goes with it
        let scene = app.model.clone().unwrap();
        let shown = scene.element_with_key(crate::model::ElementKey { model: if app.models.len() == 1 { 0 } else { (app.models.len() - 1) as u32 }, source: ids[0] }).unwrap();
        app.sel.set(Some(Pickable::Element(shown)));
        assert!(app.delete_modelled());
        assert_eq!(app.modeller.model.len(), 1);
    }

    #[test]
    fn an_ifcx_file_keeps_the_definitions_and_gives_editable_elements_back() {
        let mut app = App::default();
        app.add_demo_building();
        let written: Vec<Element> = app.modeller.model.iter().cloned().collect();
        let scene = app.model.clone().unwrap();
        let bytes = crate::export::to_bytes(&scene, &app.document, "house.ifcx").expect("IFCX");
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.contains("openaec::element") && text.contains("IfcCurtainWall") && text.contains("IfcWall"));
        assert!(!text.contains(crate::export::ifcx::DEFINITION), "the definition is an attribute, not a property");
        let source = crate::loaders::Source::Bytes { name: "house.ifcx".into(), data: Arc::new(bytes) };
        let loaded = crate::loaders::load_with(&source, &crate::loaders::Progress::default()).expect("read back");
        let mut back = App::default();
        back.set_model(Arc::new(loaded));
        let read: Vec<Element> = back.modeller.model.iter().cloned().collect();
        assert_eq!(read, written, "every definition comes back exactly");
        // shown once: from the definitions, not also as the file's meshes
        let shown = back.model.as_ref().unwrap();
        let walls = shown.layers.iter().filter(|l| l.name.ends_with("IfcWall")).map(|l| l.elements.len()).sum::<usize>();
        assert_eq!(walls, 3);
        // and editable: a new wall gets the next id
        back.set_model_tool(Some((ModelTool::Wall, "Wall")));
        let id = back.add_element("Wall", None, Shape::Planar(PlanarDef { form: PlanarForm::Wall { start: DVec2::new(0.0, 10.0), end: DVec2::new(4.0, 10.0), base: 0.0, height: 3.0 }, thickness: 0.2, openings: Vec::new() })).unwrap();
        assert!(id.0 > written.iter().map(|e| e.id.0).max().unwrap());
    }

    #[test]
    fn the_first_selected_element_cuts_the_second() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 6.0, 0.0);
        app.set_model_tool(Some((ModelTool::Beam, "Beam")));
        click(&mut app, 3.0, -2.0);
        click(&mut app, 3.0, 2.0);
        app.set_model_tool(None);
        let (wall, beam) = (ElementId(1), ElementId(2));
        let before = app.modeller.model.evaluate(wall).unwrap()[0].solid.volume();
        app.select_modelled(beam);
        let scene = app.model.clone().unwrap();
        let shown = scene.element_with_key(crate::model::ElementKey { model: 0, source: wall.0 }).unwrap();
        app.sel.toggle(Pickable::Element(shown));
        assert_eq!(app.selected_bodies(), 2);
        app.connect_selected(ConnectionKind::Cut, "Cut-out");
        assert!(app.modeller.refusal.is_none(), "{:?}", app.modeller.refusal);
        let after = app.modeller.model.evaluate(wall).unwrap()[0].solid.volume();
        assert!((before - after - 0.2 * 0.4 * 0.2).abs() < 1e-9, "the beam's section through the wall: {before} - {after}");
    }

    #[test]
    fn a_refusal_is_kept_to_show() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        click(&mut app, 1.0, 1.0);
        click(&mut app, 1.0, 1.0);
        assert_eq!(app.modeller.refusal, Some(Refusal::ZeroLength));
        assert!(app.modeller.model.is_empty());
    }

    #[test]
    fn modelled_elements_show_their_definition_as_properties() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 5.0, 0.0);
        let scene = app.model.clone().unwrap();
        let id = scene.layers.iter().flat_map(|l| &l.elements).next().unwrap().0;
        let props: std::collections::HashMap<String, String> = scene.element_props(id).into_iter().collect();
        assert_eq!(props["class"], "IfcWall");
        assert_eq!(props["Length"], "5.000 m");
        assert_eq!(props["Base element"], "Planar");
        assert_eq!(props["Volume"], "3.000 m³");
    }
}
