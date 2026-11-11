//! The neutral scene model every loader produces: Z-up, metres (or file units), one
//! triangle mesh or point cloud per layer.

use crate::structural::display::StructuralDisplay;
use crate::structural::{StructuralIndex, StructuralItem, StructuralPart};
use glam::DVec3;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// One layer: a named triangle mesh with a single base colour (plus optional vertex colours).
#[derive(Debug, Clone, Default)]
pub struct Layer {
    pub name: String,
    /// Base colour (sRGB, 0..1).
    pub color: [f32; 3],
    /// xyz per vertex, Z up.
    pub positions: Vec<f32>,
    /// xyz per vertex, unit length; same length as `positions`.
    pub normals: Vec<f32>,
    /// Optional per-vertex sRGB colours (RGBA8); overrides `color` when present.
    pub colors: Option<Vec<[u8; 4]>>,
    /// Triangle list.
    pub indices: Vec<u32>,
    /// Pickable elements (BIM formats): element id, first index and index count in `indices`,
    /// sorted by first index.
    pub elements: Vec<(u64, u32, u32)>,
    /// Element properties as flat JSON objects; moved into [`Model::props`] by [`Model::new`].
    pub props: Vec<(u64, String)>,
    /// Hidden in the file (a hidden SketchUp tag): starts switched off.
    pub hidden: bool,
    /// A point cloud (LAS, LAZ, E57): every vertex is a point, drawn as a dot of a few pixels;
    /// no `indices`, `normals` or `elements`.
    pub points: bool,
}

impl Layer {
    pub fn vertex_count(&self) -> usize {
        self.positions.len() / 3
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// The points of a point cloud layer (0 for a mesh).
    pub fn point_count(&self) -> usize {
        if self.points { self.vertex_count() } else { 0 }
    }

    /// The triangles of the elements (or the whole layer) that enclose volume, for the cut fill.
    pub fn solid_indices(&self) -> Vec<u32> {
        let whole = [(0u64, 0u32, self.indices.len() as u32)];
        let ranges = if self.elements.is_empty() { &whole[..] } else { &self.elements[..] };
        let mut out = Vec::new();
        for &(_, first, count) in ranges {
            let Some(tris) = self.indices.get(first as usize..(first + count) as usize) else { continue };
            if tris.iter().all(|&i| 3 * (i as usize) + 2 < self.positions.len()) && encloses_volume(&self.positions, tris) {
                out.extend_from_slice(tris);
            }
        }
        out
    }

    /// The element drawn by triangle `tri`, if the layer has elements.
    pub fn element_at(&self, tri: usize) -> Option<u64> {
        let at = (3 * tri) as u32;
        let k = self.elements.partition_point(|e| e.1 <= at).checked_sub(1)?;
        let (id, first, count) = self.elements[k];
        (at < first + count).then_some(id)
    }
}

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Bounds {
    pub fn empty() -> Self {
        Self { min: [f32::INFINITY; 3], max: [f32::NEG_INFINITY; 3] }
    }

    pub fn is_valid(&self) -> bool {
        (0..3).all(|i| self.min[i].is_finite() && self.max[i].is_finite() && self.min[i] <= self.max[i])
    }

    pub fn extend(&mut self, p: [f32; 3]) {
        for i in 0..3 {
            self.min[i] = self.min[i].min(p[i]);
            self.max[i] = self.max[i].max(p[i]);
        }
    }

    pub fn center(&self) -> [f32; 3] {
        [0, 1, 2].map(|i| 0.5 * (self.min[i] + self.max[i]))
    }

    pub fn size(&self) -> [f32; 3] {
        [0, 1, 2].map(|i| self.max[i] - self.min[i])
    }

    pub fn radius(&self) -> f32 {
        let s = self.size();
        0.5 * (s[0] * s[0] + s[1] * s[1] + s[2] * s[2]).sqrt()
    }
}

/// An element as its own file names it: the index of its model among the open ones and its id
/// in that file. It stays the same when another model is added; the id in the shown model
/// ([`Model::combine`] renumbers) does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ElementKey {
    pub model: u32,
    pub source: u64,
}

/// A loaded model.
#[derive(Debug)]
pub struct Model {
    /// Unique id, used by the GPU cache to know when to upload.
    pub id: u64,
    pub path: PathBuf,
    pub format: String,
    pub file_size: u64,
    pub load_time: Duration,
    pub layers: Vec<Layer>,
    pub bounds: Bounds,
    /// The file was Y up and the loader turned it to Z up.
    pub source_y_up: bool,
    /// Element properties (flat JSON object per element id).
    pub props: HashMap<u64, String>,
    /// For a combined model ([`Model::combine`]): the key of every element and back; empty for
    /// a model of one file, whose ids are the keys.
    pub keys: HashMap<u64, ElementKey>,
    pub ids: HashMap<ElementKey, u64>,
    /// Views saved in the file (Revit), in world coordinates.
    pub views: Vec<SavedView>,
    /// What the file says about the project besides geometry (Revit).
    pub project: ProjectData,
    /// Structural analysis models (nodes, members, supports, ...): typed document objects with
    /// element ids, drawn from the document rather than from layer triangles. Set with
    /// [`Model::set_structural`].
    pub structural: Vec<StructuralPart>,
    /// Element id → structural object, built by [`Model::set_structural`].
    pub structural_index: StructuralIndex,
    /// What the renderer draws of the structural parts (member axes, node glyphs), derived with
    /// them by [`Model::set_structural`]: on the load worker for a loaded file, never per frame.
    pub structural_display: Option<Arc<StructuralDisplay>>,
    /// Where each source model sits in the world (f64, Z up, metres); index = the `model` of a
    /// [`StructuralPart`]. One entry for a loaded file, one per file for a combined model.
    pub origins: Vec<Origin>,
}

/// Where a source model's display coordinates sit in the world, in f64 (Z up, metres).
///
/// A display point `q` (the f32 positions of the layers, relative to the model's anchor) is the
/// world point `anchor + q - shift`. `shift` is the translation [`Model::combine`] applied to
/// align the model with the first one (display alignment on Add, under 10 km).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Origin {
    /// The anchor the loader subtracted; `None` when the format has none (the display
    /// coordinates are the file's own).
    pub anchor: Option<DVec3>,
    /// The translation applied to the display coordinates by [`Model::combine`].
    pub shift: DVec3,
}

impl Origin {
    /// The display coordinates of a world point, in f64.
    pub fn display(&self, world: DVec3) -> DVec3 {
        world - self.anchor.unwrap_or(DVec3::ZERO) + self.shift
    }

    /// The world point of display coordinates.
    pub fn world(&self, display: DVec3) -> DVec3 {
        display - self.shift + self.anchor.unwrap_or(DVec3::ZERO)
    }
}

/// Above this distance between two models' anchors, [`Model::combine`] does not align them: the
/// f32 display would lose millimetres.
pub const ALIGN_LIMIT: f64 = 10_000.0;

/// Project data besides geometry (Revit): project information, levels, phases, grids, rooms,
/// sheets.
#[derive(Debug, Clone, Default)]
pub struct ProjectData {
    /// Sheets (Revit, beta): the paper and the views placed on it.
    pub sheets: Vec<Sheet>,
    /// Drawings without a model (Revit, beta): legends and drafting views, with their lines,
    /// texts and dimensions.
    pub drafts: Vec<Draft>,
    /// Project information: (key, value). Built-in keys: `number`, `name`, `client`, `status`;
    /// other keys are the captions of the project's own parameters.
    pub info: Vec<(String, String)>,
    /// Levels: name and elevation (metres), low to high.
    pub levels: Vec<(String, f32)>,
    /// Phases in the order of the project.
    pub phases: Vec<String>,
    pub grids: Vec<GridLine>,
    pub rooms: Vec<Room>,
    /// The drawings of an IFCX file this viewer wrote: floor plans, sections and sheets as its
    /// `o3v::` nodes (path, attributes), in file order; the project document takes them over.
    pub drawings: Vec<(String, serde_json::Value)>,
    /// The definitions of elements this viewer modelled, as an IFCX file it wrote holds them
    /// (`openaec::element`); the modeller takes them over.
    pub elements: Vec<serde_json::Value>,
    /// Schedules: computed after the model shows, as they take seconds on a large project.
    pub schedules: Later<Vec<Schedule>>,
}

/// Work a loader leaves for after the model shows; whoever takes it runs it (once).
pub struct Later<T>(Arc<Mutex<Option<Box<dyn FnOnce() -> T + Send>>>>);

impl<T> Later<T> {
    pub fn new(work: impl FnOnce() -> T + Send + 'static) -> Self {
        Self(Arc::new(Mutex::new(Some(Box::new(work)))))
    }

    /// The work, the first time (`None` when there is none or it was taken).
    pub fn take(&self) -> Option<Box<dyn FnOnce() -> T + Send>> {
        self.0.lock().ok()?.take()
    }
}

impl<T> Default for Later<T> {
    fn default() -> Self {
        Self(Arc::default())
    }
}

impl<T> Clone for Later<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> std::fmt::Debug for Later<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let waiting = self.0.lock().is_ok_and(|w| w.is_some());
        write!(f, "Later({})", if waiting { "waiting" } else { "none" })
    }
}

/// A grid line (world, metres) at the height it is drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct GridLine {
    pub name: String,
    pub a: [f32; 3],
    pub b: [f32; 3],
}

/// A room: number, name, level and the values of the project's own room parameters.
#[derive(Debug, Clone, Default)]
pub struct Room {
    pub number: String,
    pub name: String,
    pub level: Option<String>,
    pub extra: Vec<(String, String)>,
}

/// A schedule (Revit): a table of elements the file defines. The file keeps only its
/// definition; the loader computed the rows.
#[derive(Debug, Clone, Default)]
pub struct Schedule {
    pub name: String,
    /// The title above the table (the name unless the file sets another).
    pub title: String,
    /// Headings of the shown columns.
    pub columns: Vec<String>,
    pub rows: Vec<ScheduleRow>,
    /// The rows could be computed (not yet for material takeoffs and key schedules).
    pub computed: bool,
}

/// A row of a schedule: one cell per column, as Revit shows it (empty when there is no value
/// or the elements of a group differ; `None` when the loader could not work the value out).
#[derive(Debug, Clone, Default)]
pub struct ScheduleRow {
    pub cells: Vec<Option<String>>,

    /// The elements of the row: one when itemized, the group's otherwise.
    pub elements: Vec<u64>,
    /// The grand total.
    pub total: bool,
}

/// A sheet (Revit, beta): the paper and the views placed on it, in sheet metres (x right, y up)
/// with the title block's insertion point at the origin.
#[derive(Debug, Clone, Default)]
pub struct Sheet {
    pub number: String,
    pub name: String,
    /// Number and name: "CP-21 Constructietekening".
    pub title: String,
    /// The paper, when the title block tells its size.
    pub paper: Option<SheetRect>,
    pub viewports: Vec<SheetViewport>,
    /// The lines of its title block.
    pub lines: Vec<[[f32; 2]; 2]>,
    /// The texts of its title block: its fixed ones and its labels, filled in.
    pub texts: Vec<DraftText>,
    /// The schedules placed on it.
    pub tables: Vec<SheetTable>,
    /// The images of its title block, such as a logo.
    pub images: Vec<SheetImage>,
}

/// An image on a sheet (sheet metres).
#[derive(Debug, Clone)]
pub struct SheetImage {
    pub center: [f32; 2],
    /// Its width and height.
    pub size: [f32; 2],
    /// How it is turned (radians, counterclockwise).
    pub angle: f32,
    pub image: iced::widget::image::Handle,
}

impl Sheet {
    pub fn new(number: String, name: String, paper: Option<SheetRect>, viewports: Vec<SheetViewport>) -> Self {
        let title = [number.as_str(), name.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" ");
        Self { number, name, title, paper, viewports, lines: Vec::new(), texts: Vec::new(), tables: Vec::new(), images: Vec::new() }
    }

    /// What the sheet covers: the paper, every viewport and every schedule, and what its title
    /// block draws.
    pub fn extent(&self) -> Option<SheetRect> {
        let tables = self.tables.iter().map(SheetTable::rect);
        let images = self.images.iter().map(|i| SheetRect::around(i.center, i.size));
        let points = self.lines.iter().flatten().copied().chain(self.texts.iter().flat_map(|t| [t.at, [t.at[0] + t.width, t.at[1] + t.height]]));
        let block = points.map(|p| SheetRect { min: p, max: p });
        self.paper.iter().copied().chain(self.viewports.iter().map(|v| v.rect)).chain(tables).chain(images).chain(block).reduce(SheetRect::union)
    }
}

/// A schedule placed on a sheet, as the table the file stores for it (sheet metres).
#[derive(Debug, Clone, Default)]
pub struct SheetTable {
    /// The name of its schedule.
    pub schedule: String,
    /// Its upper left corner.
    pub origin: [f32; 2],
    /// The widths of its columns.
    pub columns: Vec<f32>,
    /// Its rows, from the top.
    pub rows: Vec<TableRow>,
}

/// A row of a [`SheetTable`].
#[derive(Debug, Clone, Default)]
pub struct TableRow {
    pub height: f32,
    /// Its cells, from the left; a cell another one covers is left out.
    pub cells: Vec<TableCell>,
    /// The title above the columns: its cell runs over the full width.
    pub title: bool,
}

/// A cell of a [`SheetTable`].
#[derive(Debug, Clone, Default)]
pub struct TableCell {
    /// The column it starts in, and how many it covers.
    pub column: u16,
    pub span: u16,
    pub text: String,
    /// The height of its letters (metres on paper).
    pub size: f32,
    /// 0 left, 1 centre, 2 right.
    pub align: u8,
    /// 0 top, 1 middle, 2 foot.
    pub vertical: u8,
    /// It shows a value of the row's elements that the file does not store.
    pub open: bool,
}

impl SheetTable {
    pub fn width(&self) -> f32 {
        self.columns.iter().sum()
    }

    /// What it covers on the sheet.
    pub fn rect(&self) -> SheetRect {
        let height: f32 = self.rows.iter().map(|r| r.height).sum();
        SheetRect { min: [self.origin[0], self.origin[1] - height], max: [self.origin[0] + self.width(), self.origin[1]] }
    }

    /// The table with its open cells filled from the computed schedule, when that has a row for
    /// each of its rows with open cells and as many columns.
    pub fn filled(&self, computed: Option<&Schedule>) -> SheetTable {
        let mut table = self.clone();
        let Some(s) = computed.filter(|s| s.computed && s.columns.len() == self.columns.len()) else { return table };
        let values: Vec<&ScheduleRow> = s.rows.iter().filter(|r| !r.total).collect();
        let open = |r: &TableRow| !r.title && r.cells.iter().any(|c| c.open);
        if table.rows.iter().filter(|r| open(r)).count() != values.len() {
            return table;
        }
        for (row, value) in table.rows.iter_mut().filter(|r| open(r)).zip(values) {
            for cell in row.cells.iter_mut().filter(|c| c.open) {
                if let Some(Some(text)) = value.cells.get(cell.column as usize) {
                    cell.text.clone_from(text);
                }
            }
        }
        table
    }
}

/// A rectangle on a sheet (metres).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SheetRect {
    pub min: [f32; 2],
    pub max: [f32; 2],
}

impl SheetRect {
    pub fn around(center: [f32; 2], size: [f32; 2]) -> Self {
        Self { min: [center[0] - size[0] / 2.0, center[1] - size[1] / 2.0], max: [center[0] + size[0] / 2.0, center[1] + size[1] / 2.0] }
    }

    pub fn union(self, o: Self) -> Self {
        Self { min: [self.min[0].min(o.min[0]), self.min[1].min(o.min[1])], max: [self.max[0].max(o.max[0]), self.max[1].max(o.max[1])] }
    }

    pub fn contains(&self, p: [f32; 2]) -> bool {
        (0..2).all(|k| self.min[k] <= p[k] && p[k] <= self.max[k])
    }

    pub fn center(&self) -> [f32; 2] {
        [(self.min[0] + self.max[0]) / 2.0, (self.min[1] + self.max[1]) / 2.0]
    }
}

/// A view placed on a sheet.
#[derive(Debug, Clone)]
pub struct SheetViewport {
    /// The view's name.
    pub title: String,
    /// The view in [`Model::views`], when the viewer can show it (not legends and drafting views).
    pub view: Option<usize>,
    pub rect: SheetRect,
    /// The size is a guess (the view has no active crop region).
    pub estimated: bool,
    /// "1:50".
    pub scale: String,
    pub detail: Option<String>,
    /// Where its title stands on the sheet (metres), when the sheet tells.
    pub label: Option<[f32; 2]>,
    /// Where the origin of the view's own drawing lies on the sheet (metres).
    pub origin: [f32; 2],
    /// What a metre of the view is on the sheet: 0.02 at 1:50.
    pub ratio: f32,
    /// The path of the document's view (plan, section or elevation) it shows, when it is one.
    pub drawing: Option<String>,
    /// The point of that view's drawing that lies at `origin` (metres), when the view tells.
    pub anchor: Option<[f64; 2]>,
    /// The legend or drafting view in [`ProjectData::drafts`] it shows, when it is one.
    pub draft: Option<usize>,
}

/// The kinds of drawing without a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftKind {
    Legend,
    Drafting,
}

/// A drawing without a model (Revit, beta: a legend or a drafting view): what is drawn in it, in
/// its own plane (metres at full size, x right, y up). On paper it is `scale` times smaller.
#[derive(Debug, Clone)]
pub struct Draft {
    /// The view's element in its file.
    pub id: u64,
    pub name: String,
    pub kind: DraftKind,
    /// 1 : `scale`.
    pub scale: f32,
    pub lines: Vec<DraftLine>,
    pub texts: Vec<DraftText>,
    pub dimensions: Vec<DraftDimension>,
}

impl Draft {
    /// What it covers: lower left and upper right, `None` when it is empty.
    pub fn extent(&self) -> Option<SheetRect> {
        let lines = self.lines.iter().flat_map(|l| [l.a, l.b]);
        let texts = self.texts.iter().flat_map(|t| [t.at, [t.at[0] + t.width, t.at[1] + t.height]]);
        let dimensions = self.dimensions.iter().flat_map(|d| [d.from, d.to, d.measured[0], d.measured[1]]);
        lines.chain(texts).chain(dimensions).map(|p| SheetRect { min: p, max: p }).reduce(SheetRect::union)
    }
}

/// A line of a drawing: a detail line, or a piece of a curved one. `element` is the element of
/// the file it belongs to, which a click picks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DraftLine {
    pub element: u64,
    pub a: [f32; 2],
    pub b: [f32; 2],
}

/// A text of a drawing: its box from its lower left corner, and the size of its letters.
#[derive(Debug, Clone, PartialEq)]
pub struct DraftText {
    pub element: u64,
    pub at: [f32; 2],
    pub width: f32,
    pub height: f32,
    /// The height of a line of letters (the font size).
    pub size: f32,
    /// Where its lines start in its box: 0 left, 1 middle, 2 right.
    pub align: u8,
    /// How it is turned about the foot of its box where its lines start (radians,
    /// counter-clockwise).
    pub angle: f32,
    pub text: String,
}

/// A dimension of a drawing: its line from `from` to `to`, and the two points it measures.
#[derive(Debug, Clone, PartialEq)]
pub struct DraftDimension {
    pub element: u64,
    pub from: [f32; 2],
    pub to: [f32; 2],
    pub measured: [[f32; 2]; 2],
    /// How it looks: the style of its type (sizes on paper).
    pub style: std::sync::Arc<crate::document::DimensionStyle>,
}

/// The kind of a saved view.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewKind {
    ThreeD,
    Plan,
    Section,
    Elevation,
    Detail,
}

/// Where a saved view looks from (world, Z up, metres).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewCamera {
    pub eye: [f32; 3],
    /// Unit view direction, from the eye into the model.
    pub forward: [f32; 3],
    pub perspective: bool,
    /// Vertical field of view (radians), perspective views.
    pub fov_y: Option<f32>,
}

/// The heights a plan shows: from `low` up to the cut plane `high` (a ceiling plan looks up from
/// its cut plane: `from_below`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlanRange {
    pub low: f32,
    pub high: f32,
    pub from_below: bool,
}

/// A view saved in the file, as the viewer can show it.
#[derive(Debug, Clone)]
pub struct SavedView {
    pub name: String,
    pub kind: ViewKind,
    pub camera: Option<ViewCamera>,
    /// What the view shows: a 3D view's section box or the crop of a section or elevation.
    pub clip: Option<crate::section::SectionBox>,
    pub plan: Option<PlanRange>,
    pub level: Option<String>,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

impl Model {
    /// Compose loaded files in the app. The first file keeps its element ids; the elements of
    /// the others are numbered after the highest id so far that a JSON number holds exactly
    /// ([`crate::structural::MAX_SAFE_ID`]), so adding a file changes no id and the new ids stay
    /// compact. [`Model::element_key`] names every element by its file and its own id.
    ///
    /// Structural parts come along with their documents untouched (source ids stay in the
    /// documents; [`StructuralPart::element_id`] gives the new element id), and the origins of
    /// every source model are kept. When the model or the first anchored model has structural
    /// parts, a model whose anchor lies within [`ALIGN_LIMIT`] of the first anchored model's is
    /// moved by the f64 difference (rounded once to f32), so both draw in one frame; a farther
    /// model stays where it is, with a note in the project information. Models without an anchor
    /// are never moved, and models without structural parts (an IFC with another IFC) combine as
    /// they always have.
    pub fn combine(models: &[std::sync::Arc<Model>]) -> Self {
        let mut layers = Vec::new();
        let mut props = HashMap::new();
        let mut views = Vec::new();
        let mut project = ProjectData::default();
        let mut origins = Vec::new();
        let mut parts = Vec::new();
        let mut next_id = 1u64;
        let (mut keys, mut ids) = (HashMap::new(), HashMap::new());
        // display alignment: to the first model that knows its anchor (and whether it has
        // structural parts)
        let reference = models.iter().find_map(|m| Some((m.origins.iter().find_map(|o| o.anchor)?, !m.structural.is_empty())));
        for (m, model) in models.iter().enumerate() {
            let source = model.file_name();
            let sources: std::collections::BTreeSet<_> = model.props.keys().copied()
                .chain(model.layers.iter().flat_map(|layer| layer.elements.iter().map(|e| e.0)))
                .chain(model.structural_index.element_ids()).collect();
            // the first model keeps its own ids, the others follow after the highest JSON-safe id
            // so far, so no id changes when another model is added and the new ids stay compact
            let remap: HashMap<_, _> = sources.into_iter().map(|id| {
                let mapped = if m == 0 { id } else { next_id };
                if mapped <= crate::structural::MAX_SAFE_ID {
                    next_id = next_id.max(mapped + 1);
                }
                let key = ElementKey { model: m as u32, source: id };
                keys.insert(mapped, key);
                ids.insert(key, mapped);
                (id, mapped)
            }).collect();
            // the shift that aligns this model, and the translation that takes it there
            let anchor = model.origins.iter().find_map(|o| o.anchor);
            // only where an analysis model takes part: meshes alone combine as they always have
            let aligns = !model.structural.is_empty() || reference.is_some_and(|(_, structural)| structural);
            let (shift, far) = match (anchor, reference) {
                (Some(_), Some(_)) if !aligns => (None, None),
                (Some(a), Some((r, _))) if (a - r).length() < ALIGN_LIMIT => (Some(a - r), None),
                (Some(a), Some((r, _))) => (None, Some((a - r).length())),
                _ => (None, None),
            };
            let current = model.origins.first().map_or(DVec3::ZERO, |o| o.shift);
            let moved = shift.map(|s| s - current).filter(|t| *t != DVec3::ZERO);
            let first_origin = origins.len() as u32;
            origins.extend(model.origins.iter().map(|o| Origin { anchor: o.anchor, shift: shift.unwrap_or(o.shift) }));
            if model.origins.is_empty() {
                origins.push(Origin::default());
            }
            if let Some(d) = far {
                project.info.push((format!("{source} / Not aligned"), format!(
                    "its origin is {d:.0} m from the first model's, more than {:.0} km: drawn in its own coordinates",
                    ALIGN_LIMIT / 1000.0
                )));
            }
            for layer in &model.layers {
                let mut layer = layer.clone();
                layer.name = format!("{source} / {}", layer.name);
                for element in &mut layer.elements { element.0 = remap[&element.0]; }
                if let Some(t) = moved {
                    translate(&mut layer.positions, t);
                }
                layers.push(layer);
            }
            props.extend(model.props.iter().map(|(id, value)| (remap[id], value.clone())));
            parts.extend(model.structural.iter().map(|part| part.renumbered(first_origin + part.model, |id| remap[&id])));
            let first_view = views.len();
            project.sheets.extend(model.project.sheets.iter().cloned().map(|mut sheet| {
                sheet.viewports.iter_mut().for_each(|v| v.view = v.view.map(|i| i + first_view));
                Sheet::new(format!("{source} / {}", sheet.number), sheet.name, sheet.paper, sheet.viewports)
            }));
            let t = moved.map_or(glam::Vec3::ZERO, |t| t.as_vec3());
            views.extend(model.views.iter().cloned().map(|mut view| {
                view.name = format!("{source} / {}", view.name);
                if let Some(c) = &mut view.camera {
                    c.eye = (glam::Vec3::from_array(c.eye) + t).to_array();
                }
                if let Some(c) = &mut view.clip {
                    // the box frame turns about its pivot: moving pivot and limits moves the box
                    c.pivot += t;
                    c.min += t;
                    c.max += t;
                }
                view
            }));
            project.info.extend(model.project.info.iter().map(|(key, value)| (format!("{source} / {key}"), value.clone())));
            project.levels.extend(model.project.levels.iter().map(|(name, z)| (name.clone(), z + t.z)));
            project.phases.extend(model.project.phases.iter().cloned());
            project.grids.extend(model.project.grids.iter().cloned().map(|mut g| {
                g.a = (glam::Vec3::from_array(g.a) + t).to_array();
                g.b = (glam::Vec3::from_array(g.b) + t).to_array();
                g
            }));
            project.rooms.extend(model.project.rooms.iter().cloned());
            project.drawings.extend(model.project.drawings.iter().cloned());
            project.drafts.extend(model.project.drafts.iter().cloned());
        }
        project.levels.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        project.levels.dedup();
        let mut combined = Self::new(format!("{} models", models.len()).into(), "Multiple models", layers);
        combined.props = props;
        combined.keys = keys;
        combined.ids = ids;
        combined.views = views;
        combined.project = project;
        combined.origins = origins;
        combined.set_structural(parts);
        combined.file_size = models.iter().map(|m| m.file_size).sum();
        combined.load_time = models.iter().map(|m| m.load_time).sum();
        combined
    }

    /// Builds a model from layers: drops empty layers and computes the bounds.
    pub fn new(path: PathBuf, format: impl Into<String>, mut layers: Vec<Layer>) -> Self {
        let props: HashMap<u64, String> = layers.iter_mut().flat_map(|l| std::mem::take(&mut l.props)).collect();
        layers.retain(|l| (l.points || !l.indices.is_empty()) && !l.positions.is_empty());
        layers.iter_mut().for_each(unshare_element_vertices);
        let layers: Vec<Layer> = layers
            .into_iter()
            .flat_map(|l| if l.points { split_points(l, MAX_PART_POINTS) } else { split_layer(l, MAX_PART_VERTICES, MAX_PART_TRIANGLES) })
            .collect();
        let mut bounds = Bounds::empty();
        for layer in &layers {
            for p in layer.positions.chunks_exact(3) {
                if p.iter().all(|v| v.is_finite()) {
                    bounds.extend([p[0], p[1], p[2]]);
                }
            }
        }
        let file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            path,
            format: format.into(),
            file_size,
            load_time: Duration::ZERO,
            layers,
            bounds,
            source_y_up: false,
            props,
            keys: HashMap::new(),
            ids: HashMap::new(),
            views: Vec::new(),
            project: ProjectData::default(),
            structural: Vec::new(),
            structural_index: StructuralIndex::default(),
            structural_display: None,
            origins: vec![Origin::default()],
        }
    }

    /// Installs structural parts: indexes their objects by element id (compact ids after the
    /// model's own, see [`StructuralIndex::build`]) and adds their units (nodes, members with the
    /// bulge of their arcs, surfaces) to the bounds. A model of only places-less objects
    /// (sections, materials, load cases) gets a small box around the origin, so the camera, grid
    /// and section box have something to frame. `origins` must cover the `model` of every part.
    pub fn set_structural(&mut self, parts: Vec<StructuralPart>) {
        if parts.is_empty() {
            self.structural = Vec::new();
            self.structural_index = StructuralIndex::default();
            self.structural_display = None;
            return;
        }
        // compact structural ids start after every element id the model has
        let first_free = self
            .layers
            .iter()
            .flat_map(|l| l.elements.iter().map(|e| e.0))
            .chain(self.props.keys().copied())
            .filter(|&id| id <= crate::structural::MAX_SAFE_ID)
            .max()
            .map_or(1, |id| id + 1);
        let (parts, index) = StructuralIndex::build(parts, first_free);
        self.structural = parts;
        self.structural_index = index;
        self.structural_display = Some(Arc::new(StructuralDisplay::build(&self.structural, &self.origins)));
        for (_, b) in self.structural_index.unit_bounds(&self.structural, &self.origins) {
            self.bounds.extend(b.min);
            self.bounds.extend(b.max);
        }
        if !self.bounds.is_valid() {
            self.bounds = Bounds { min: [-1.0; 3], max: [1.0; 3] };
        }
    }

    /// The structural object with element id `id`.
    pub fn structural_item(&self, id: u64) -> Option<StructuralItem<'_>> {
        self.structural_index.get(&self.structural, id)
    }

    /// The display position (model space, f32) of world point `world` of source model `model`.
    pub fn display_point(&self, model: u32, world: DVec3) -> [f32; 3] {
        self.origins.get(model as usize).copied().unwrap_or_default().display(world).as_vec3().to_array()
    }

    /// The bounds of an element at rest (model space): a structural object's point, segment or
    /// target box, else the box of its triangles.
    pub fn element_bounds(&self, id: u64) -> Option<Bounds> {
        if let Some(item) = self.structural_item(id) {
            return item.bounds(&self.origins);
        }
        let mut b = Bounds::empty();
        for (l, first, count) in self.element_ranges(id) {
            let layer = &self.layers[l];
            for &i in layer.indices.get(first as usize..(first + count) as usize).unwrap_or(&[]) {
                if let Some(p) = layer.positions.get(3 * i as usize..3 * i as usize + 3) {
                    b.extend([p[0], p[1], p[2]]);
                }
            }
        }
        b.is_valid().then_some(b)
    }

    pub fn file_name(&self) -> String {
        self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }

    pub fn vertex_count(&self) -> usize {
        self.layers.iter().map(Layer::vertex_count).sum()
    }

    pub fn triangle_count(&self) -> usize {
        self.layers.iter().map(Layer::triangle_count).sum()
    }

    /// The points of the point cloud layers.
    pub fn point_count(&self) -> usize {
        self.layers.iter().map(Layer::point_count).sum()
    }

    /// The elements whose property `key` is `value`, both ignoring case, in layer order and each
    /// once. `category` falls back to the element's first layer name, as the remote
    /// `find_elements` does.
    pub fn elements_with(&self, key: &str, value: &str) -> Vec<u64> {
        let (key, value) = (key.to_lowercase(), value.to_lowercase());
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for layer in &self.layers {
            for &(id, ..) in &layer.elements {
                if !seen.insert(id) {
                    continue;
                }
                let found = self.element_props(id).into_iter().find(|(k, _)| k.to_lowercase() == key).map(|(_, v)| v.to_lowercase());
                let found = found.or_else(|| (key == "category").then(|| layer.name.to_lowercase()));
                if found.as_deref() == Some(value.as_str()) {
                    out.push(id);
                }
            }
        }
        out
    }

    /// The key of element `id`: its model and its id in that model's file.
    pub fn element_key(&self, id: u64) -> ElementKey {
        self.keys.get(&id).copied().unwrap_or(ElementKey { model: 0, source: id })
    }

    /// The element with `key`, as this model numbers it.
    pub fn element_with_key(&self, key: ElementKey) -> Option<u64> {
        if self.keys.is_empty() {
            return (key.model == 0).then_some(key.source);
        }
        self.ids.get(&key).copied()
    }

    /// Where an element is drawn: (layer, first index, index count) per range.
    pub fn element_ranges(&self, id: u64) -> Vec<(usize, u32, u32)> {
        let mut out = Vec::new();
        for (i, layer) in self.layers.iter().enumerate() {
            out.extend(layer.elements.iter().filter(|e| e.0 == id).map(|e| (i, e.1, e.2)));
        }
        out
    }

    /// The properties of an element as (key, value) pairs in file order.
    /// Structural objects keep no stored properties: theirs are built from the document.
    pub fn element_props(&self, id: u64) -> Vec<(String, String)> {
        let Some(json) = self.props.get(&id) else {
            return self.structural_item(id).map(|item| item.props()).unwrap_or_default();
        };
        match serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(json) {
            Ok(map) => map
                .into_iter()
                .map(|(k, v)| (k, v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())))
                .collect(),
            Err(_) => Vec::new(),
        }
    }
}

/// Moves xyz triples by `t`, each coordinate rounded once from f64.
fn translate(positions: &mut [f32], t: DVec3) {
    for p in positions.chunks_exact_mut(3) {
        for k in 0..3 {
            p[k] = (p[k] as f64 + t[k]) as f32;
        }
    }
}

/// GPU buffer limits (wgpu default `max_buffer_size` is 256 MiB): at 32 bytes per vertex and up
/// to 6 line indices per triangle, parts stay below that.
pub const MAX_PART_VERTICES: usize = 8_000_000;
pub const MAX_PART_TRIANGLES: usize = 10_000_000;
/// Points per GPU buffer: 16 bytes each (position and colour).
pub const MAX_PART_POINTS: usize = 12_000_000;

/// Splits a point cloud layer into parts of at most `max` points, named as [`split_layer`] does.
pub fn split_points(mut layer: Layer, max: usize) -> Vec<Layer> {
    let n = layer.vertex_count();
    if n <= max {
        return vec![layer];
    }
    let count = n.div_ceil(max);
    let mut parts = Vec::with_capacity(count);
    // from the back, so the source shrinks as the parts grow
    for k in (0..count).rev() {
        let mut part = empty_like(&layer);
        part.points = true;
        part.name = format!("{} (part {}/{count})", layer.name, k + 1);
        part.positions = layer.positions.split_off(3 * k * max);
        part.colors = layer.colors.as_mut().map(|c| c.split_off((k * max).min(c.len())));
        parts.push(part);
    }
    parts.reverse();
    parts
}

/// Splits a layer that is too big for one GPU buffer into parts named "Name (part n)".
pub fn split_layer(layer: Layer, max_vertices: usize, max_triangles: usize) -> Vec<Layer> {
    if layer.vertex_count() <= max_vertices && layer.triangle_count() <= max_triangles {
        return vec![layer];
    }
    let mut parts = Vec::new();
    let mut remap: Vec<u32> = vec![u32::MAX; layer.vertex_count()];
    let mut part = empty_like(&layer);
    let mut used: Vec<u32> = Vec::new();
    // per source triangle: (part, first index in the part), for the element ranges
    let mut placed: Vec<Option<(usize, u32)>> = vec![None; layer.triangle_count()];
    for (t, tri) in layer.indices.chunks_exact(3).enumerate() {
        if tri.iter().any(|&i| i as usize >= remap.len()) {
            continue;
        }
        if part.vertex_count() + 3 > max_vertices || part.triangle_count() + 1 > max_triangles {
            for &u in &used {
                remap[u as usize] = u32::MAX;
            }
            used.clear();
            parts.push(std::mem::replace(&mut part, empty_like(&layer)));
        }
        placed[t] = Some((parts.len(), part.indices.len() as u32));
        for &i in tri {
            if remap[i as usize] == u32::MAX {
                remap[i as usize] = part.vertex_count() as u32;
                used.push(i);
                let s = 3 * i as usize;
                part.positions.extend_from_slice(&layer.positions[s..s + 3]);
                part.normals.extend_from_slice(layer.normals.get(s..s + 3).unwrap_or(&[0.0; 3]));
                if let (Some(dst), Some(src)) = (part.colors.as_mut(), layer.colors.as_ref()) {
                    dst.push(src.get(i as usize).copied().unwrap_or([255; 4]));
                }
            }
            part.indices.push(remap[i as usize]);
        }
    }
    if !part.indices.is_empty() {
        parts.push(part);
    }
    for &(id, first, count) in &layer.elements {
        let mut run: Option<(usize, u32, u32)> = None;
        for t in (first / 3)..(first + count) / 3 {
            let Some((p, at)) = placed.get(t as usize).copied().flatten() else { continue };
            match &mut run {
                Some((rp, rf, rc)) if *rp == p && *rf + *rc == at => *rc += 3,
                _ => {
                    if let Some((rp, rf, rc)) = run.take() {
                        parts[rp].elements.push((id, rf, rc));
                    }
                    run = Some((p, at, 3));
                }
            }
        }
        if let Some((rp, rf, rc)) = run {
            parts[rp].elements.push((id, rf, rc));
        }
    }
    let n = parts.len();
    for (k, p) in parts.iter_mut().enumerate() {
        p.name = format!("{} (part {}/{n})", layer.name, k + 1);
    }
    parts
}

fn empty_like(layer: &Layer) -> Layer {
    Layer {
        name: layer.name.clone(),
        color: layer.color,
        positions: Vec::new(),
        normals: Vec::new(),
        colors: layer.colors.as_ref().map(|_| Vec::new()),
        hidden: layer.hidden,
        ..Layer::default()
    }
}

/// Gives every element of a layer its own vertices: a vertex used by the triangles of two
/// elements (or of an element and of no element) is copied, so the exploded view can move an
/// element without dragging its neighbours along.
pub fn unshare_element_vertices(layer: &mut Layer) {
    if layer.elements.is_empty() {
        return;
    }
    // the owner of every triangle: 1 + the element's number in the layer, 0 outside elements
    let mut number: HashMap<u64, u32> = HashMap::new();
    let mut owner_of_triangle = vec![0u32; layer.triangle_count()];
    for &(id, first, count) in &layer.elements {
        let next = number.len() as u32 + 1;
        let owner = *number.entry(id).or_insert(next);
        let end = (((first + count) / 3) as usize).min(owner_of_triangle.len());
        if let Some(tris) = owner_of_triangle.get_mut(first as usize / 3..end) {
            tris.fill(owner);
        }
    }
    let n = layer.vertex_count();
    let mut owner_of_vertex = vec![u32::MAX; n];
    let mut copies: HashMap<(u32, u32), u32> = HashMap::new();
    let Layer { positions, normals, colors, indices, .. } = layer;
    for (k, index) in indices.iter_mut().enumerate() {
        let (v, Some(&owner)) = (*index as usize, owner_of_triangle.get(k / 3)) else { break };
        if v >= n || owner_of_vertex[v] == owner {
            continue;
        }
        if owner_of_vertex[v] == u32::MAX {
            owner_of_vertex[v] = owner;
            continue;
        }
        *index = *copies.entry((*index, owner)).or_insert_with(|| {
            let copy = (positions.len() / 3) as u32;
            if normals.len() == positions.len() {
                normals.extend_from_within(3 * v..3 * v + 3);
            }
            if let Some(c) = colors.as_mut().filter(|c| c.len() == copy as usize) {
                c.push(c[v]);
            }
            positions.extend_from_within(3 * v..3 * v + 3);
            copy
        });
    }
}

/// Converts Y-up coordinates (x, y, z) to Z-up (x, -z, y), in place, for xyz triples.
pub fn y_up_to_z_up(values: &mut [f32]) {
    for v in values.chunks_exact_mut(3) {
        let (y, z) = (v[1], v[2]);
        v[1] = -z;
        v[2] = y;
    }
}

/// Splits an indexed triangle mesh into unshared vertices with flat face normals.
pub fn flat_shaded(positions: &[f32], indices: &[u32]) -> (Vec<f32>, Vec<f32>, Vec<u32>) {
    let mut out_p = Vec::with_capacity(indices.len() * 3);
    let mut out_n = Vec::with_capacity(indices.len() * 3);
    let vertex_count = (positions.len() / 3) as u32;
    for tri in indices.chunks_exact(3) {
        if tri.iter().any(|&i| i >= vertex_count) {
            continue;
        }
        let p = [tri[0], tri[1], tri[2]].map(|i| {
            let i = i as usize * 3;
            glam::Vec3::new(positions[i], positions[i + 1], positions[i + 2])
        });
        let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
        for v in p {
            out_p.extend_from_slice(&[v.x, v.y, v.z]);
            out_n.extend_from_slice(&[n.x, n.y, n.z]);
        }
    }
    let count = (out_p.len() / 3) as u32;
    (out_p, out_n, (0..count).collect())
}

/// Smooth vertex normals (area weighted) for an indexed mesh.
pub fn smooth_normals(positions: &[f32], indices: &[u32]) -> Vec<f32> {
    let n = positions.len() / 3;
    let mut normals = vec![glam::Vec3::ZERO; n];
    let at = |i: u32| {
        let i = i as usize * 3;
        glam::Vec3::new(positions[i], positions[i + 1], positions[i + 2])
    };
    for tri in indices.chunks_exact(3) {
        if tri.iter().any(|&i| i as usize >= n) {
            continue;
        }
        let face = (at(tri[1]) - at(tri[0])).cross(at(tri[2]) - at(tri[0]));
        for &i in tri {
            normals[i as usize] += face;
        }
    }
    normals.iter().flat_map(|v| v.normalize_or(glam::Vec3::Z).to_array()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A model of one square, element `id`, at x.
    fn square(name: &str, id: u64, x: f32) -> std::sync::Arc<Model> {
        let layer = Layer {
            name: "Walls".into(),
            positions: vec![x, 0.0, 0.0, x + 1.0, 0.0, 0.0, x + 1.0, 1.0, 0.0, x, 1.0, 0.0],
            indices: vec![0, 1, 2, 0, 2, 3],
            elements: vec![(id, 0, 6)],
            props: vec![(id, format!(r#"{{"Name":"{name}"}}"#))],
            ..Layer::default()
        };
        std::sync::Arc::new(Model::new(PathBuf::from(format!("{name}.rvt")), "Revit", vec![layer]))
    }

    #[test]
    fn elements_with_matches_exactly_ignoring_case() {
        let walls = Layer {
            name: "Walls".into(),
            positions: vec![0.0; 18],
            indices: vec![0, 1, 2, 3, 4, 5],
            elements: vec![(1, 0, 3), (2, 3, 3)],
            props: vec![(1, r#"{"level":"00"}"#.into()), (2, r#"{"category":"Wall","level":"01"}"#.into())],
            ..Layer::default()
        };
        let m = Model::new(PathBuf::from("x.rvt"), "Revit", vec![walls]);
        assert_eq!(m.elements_with("category", "walls"), vec![1], "falls back to the layer name");
        assert_eq!(m.elements_with("Category", "WALL"), vec![2]);
        assert_eq!(m.elements_with("level", "0"), Vec::<u64>::new(), "exact, not a part");
        assert_eq!(m.elements_with("LEVEL", "01"), vec![2]);
    }

    #[test]
    fn combine_keeps_source_ids_as_keys() {
        let m = Model::combine(&[square("a", 7, 0.0), square("b", 7, 2.0)]);
        let ids: Vec<u64> = m.layers.iter().map(|l| l.elements[0].0).collect();
        assert_ne!(ids[0], ids[1]);
        assert_eq!(m.element_key(ids[0]), ElementKey { model: 0, source: 7 });
        assert_eq!(m.element_key(ids[1]), ElementKey { model: 1, source: 7 });
    }

    #[test]
    fn element_with_key_inverts_element_key() {
        let m = Model::combine(&[square("a", 7, 0.0), square("b", 9, 2.0)]);
        for id in m.layers.iter().flat_map(|l| l.elements.iter().map(|e| e.0)) {
            assert_eq!(m.element_with_key(m.element_key(id)), Some(id));
        }
    }

    #[test]
    fn single_model_keys_are_identity() {
        let m = square("a", 42, 0.0);
        assert_eq!(m.element_key(42), ElementKey { model: 0, source: 42 });
        assert_eq!(m.element_with_key(ElementKey { model: 0, source: 42 }), Some(42));
    }

    #[test]
    fn y_up_conversion() {
        let mut v = vec![1.0, 2.0, 3.0];
        y_up_to_z_up(&mut v);
        assert_eq!(v, vec![1.0, -3.0, 2.0]);
    }

    #[test]
    fn splits_big_layers() {
        let (positions, normals, indices) = flat_shaded(
            &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0],
            &[0, 1, 2, 0, 2, 3, 0, 1, 2],
        );
        let elements = vec![(7, 0, 6), (8, 6, 3)];
        let layer = Layer { name: "L".into(), color: [1.0; 3], positions, normals, indices, elements, ..Layer::default() };
        let parts = split_layer(layer, 6, 100);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].elements, vec![(7, 0, 6)]);
        assert_eq!(parts[1].elements, vec![(8, 0, 3)]);
        assert_eq!(parts[1].element_at(0), Some(8));
        assert_eq!(parts[0].element_at(1), Some(7));
        assert_eq!(parts[0].triangle_count(), 2);
        assert_eq!(parts[1].triangle_count(), 1);
        assert_eq!(parts[1].name, "L (part 2/2)");
        assert!(parts.iter().all(|p| p.indices.iter().all(|&i| (i as usize) < p.vertex_count())));
    }

    #[test]
    fn point_layers_are_kept_and_split() {
        let layer = Layer {
            name: "Ground".into(),
            positions: (0..15).map(|v| v as f32).collect(),
            colors: Some((0..5).map(|i| [i, 0, 0, 255]).collect()),
            points: true,
            ..Layer::default()
        };
        let parts = split_points(layer.clone(), 2);
        assert_eq!(parts.iter().map(Layer::point_count).collect::<Vec<_>>(), [2, 2, 1]);
        assert_eq!(parts[1].positions, [6.0, 7.0, 8.0, 9.0, 10.0, 11.0]);
        assert_eq!(parts[2].colors, Some(vec![[4, 0, 0, 255]]));
        assert_eq!(parts[0].name, "Ground (part 1/3)");
        let model = Model::new(PathBuf::from("cloud.las"), "LAS", vec![layer]);
        assert_eq!((model.layers.len(), model.point_count(), model.triangle_count()), (1, 5, 0));
        assert_eq!(model.bounds.max, [12.0, 13.0, 14.0]);
    }

    #[test]
    fn elements_get_their_own_vertices() {
        // two triangles sharing the edge 1-2: element 7, element 8; a third triangle outside both
        let positions = vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0];
        let mut layer = Layer {
            normals: [0.0, 0.0, 1.0].repeat(4),
            colors: Some(vec![[1, 2, 3, 255], [4, 5, 6, 255], [7, 8, 9, 255], [0; 4]]),
            positions: positions.clone(),
            indices: vec![0, 1, 2, 1, 3, 2, 2, 3, 1],
            elements: vec![(7, 0, 3), (8, 3, 3)],
            ..Layer::default()
        };
        unshare_element_vertices(&mut layer);
        // element 8 gets copies of 1 and 2; the free triangle its own copies of 2, 3 and 1
        assert_eq!(layer.indices, vec![0, 1, 2, 4, 3, 5, 6, 7, 8]);
        assert_eq!(layer.vertex_count(), 9);
        assert_eq!(layer.normals.len(), layer.positions.len());
        assert_eq!(&layer.positions[12..18], &positions[3..9]);
        assert_eq!(layer.colors.as_ref().map(|c| c[5]), Some([7, 8, 9, 255]));
        // nothing shared: unchanged
        let before = layer.clone();
        unshare_element_vertices(&mut layer);
        assert_eq!((layer.indices, layer.positions), (before.indices, before.positions));
    }

    #[test]
    fn sheets_do_not_enclose_volume() {
        // a closed tetrahedron, one face wound the other way
        let p = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        assert!(encloses_volume(&p, &[0, 2, 1, 0, 1, 3, 1, 2, 3, 0, 2, 3]));
        // a single sheet of two triangles
        let q = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0];
        assert!(!encloses_volume(&q, &[0, 1, 2, 0, 2, 3]));
        let layer = Layer { positions: q.to_vec(), indices: vec![0, 1, 2, 0, 2, 3], ..Layer::default() };
        assert!(layer.solid_indices().is_empty());
    }

    #[test]
    fn flat_normals_of_xy_triangle_point_up() {
        let (p, n, i) = flat_shaded(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], &[0, 1, 2]);
        assert_eq!(p.len(), 9);
        assert_eq!(i, vec![0, 1, 2]);
        assert_eq!(&n[0..3], &[0.0, 0.0, 1.0]);
    }
}

/// Whether a set of triangles encloses volume, for the cut fill of the section box. Only open
/// sheets (terrain, leaves, entourage) are refused: they make the fill's surface count odd outside
/// any solid. Seams with T-junctions and faces wound the other way do not disturb the count, so a
/// set passes when either test says closed: the area vectors nearly cancel (racbasicsample: walls
/// at most 0.10, mullions 0.22 with some faces wound the other way; terrain 0.98, planting 0.65),
/// or almost every welded edge is paired (at most 2 %; a large terrain sheet leaves only its
/// border open, 12 %).
pub fn encloses_volume(positions: &[f32], indices: &[u32]) -> bool {
    if openness(positions, indices) <= 0.3 {
        return true;
    }
    let (open, all) = open_edges(positions, indices);
    open as f32 <= 0.02 * all as f32
}

/// How far a set of triangles (index triples into `positions`) is from enclosing a volume: the
/// length of the sum of the area vectors over the sum of the areas. A closed, consistently wound
/// surface sums to zero (divergence theorem), T-junctions or not; an open sheet such as terrain
/// is near 1.
pub fn openness(positions: &[f32], indices: &[u32]) -> f32 {
    let at = |i: u32| {
        let i = 3 * i as usize;
        glam::DVec3::new(positions[i] as f64, positions[i + 1] as f64, positions[i + 2] as f64)
    };
    let (mut sum, mut area) = (glam::DVec3::ZERO, 0.0);
    for tri in indices.chunks_exact(3) {
        let (a, b, c) = (at(tri[0]), at(tri[1]), at(tri[2]));
        let n = (b - a).cross(c - a);
        sum += n;
        area += n.length();
    }
    if area <= 0.0 { 0.0 } else { (sum.length() / area) as f32 }
}

/// Open edges and all edges of a set of triangles (index triples into `positions`), with the
/// vertices welded by position: a closed surface uses every edge an even number of times.
pub fn open_edges(positions: &[f32], indices: &[u32]) -> (usize, usize) {
    use std::collections::HashMap;
    let key = |i: u32| -> [i32; 3] {
        let i = 3 * i as usize;
        [0, 1, 2].map(|k| (positions[i + k] * 1e4).round() as i32)
    };
    let mut edges: HashMap<([i32; 3], [i32; 3]), u32> = HashMap::with_capacity(indices.len());
    for tri in indices.chunks_exact(3) {
        let k = [key(tri[0]), key(tri[1]), key(tri[2])];
        for (a, b) in [(k[0], k[1]), (k[1], k[2]), (k[2], k[0])] {
            if a != b {
                *edges.entry(if a < b { (a, b) } else { (b, a) }).or_default() += 1;
            }
        }
    }
    (edges.values().filter(|&&n| n % 2 == 1).count(), edges.len())
}
