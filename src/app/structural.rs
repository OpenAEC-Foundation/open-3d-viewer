//! Structural objects in the viewport: picking nodes and members on screen, their selection
//! (drawn again on top, as the mesh highlight) and the hover highlight.
//!
//! The selection holds element ids (a load or line support may be selected; its member or node is
//! what lights up, see `StructuralItem::unit_key`). Meshes are selected through the flags of their
//! units (`App::sel`); structural instances keep a selection of their own beside the mesh
//! selection (`App::sel`), drawn from instance ranges ([`Marks`]) that the renderer gathers into
//! instances of their own. A plain click replaces both; Ctrl-click adds or removes a node or
//! member ([`App::click_structural`]). Nodes and members are units of the exploded view, so box
//! select, hide, isolate and undo reach them through the units texture: a hidden unit is neither
//! drawn nor picked, and a unit selected that way is drawn in the selection colour. That selection
//! (in `App::sel`) and the structural one are not yet one list; the seam is
//! [`App::set_structural_selection`], which takes any number of ids.
//!
//! Hiding a model of a combined set (the models list), or hiding every layer, hides its structural
//! objects too: they are left out of the drawing by instance range and are not picked
//! ([`StructuralView::shown_items`]).
//!
//! The hover looks in a screen grid ([`HoverGrid`]) built off the UI thread once the camera has
//! rested ([`HOVER_REST`]); while the camera moves, or a measure tool or plane picking is on, there
//! is no structural hover. A click uses the grid too while it is fresh, else it scans every
//! instance. Both drop what lies behind a triangle under the cursor before a node is preferred
//! over a member.

use super::{App, Message};
use crate::explode::ExplodeIndex;
use crate::structural::display::{Marks, StructuralDisplay};
use crate::structural::pick::{best, HoverGrid, Hit, Lod, Scene, View};
use crate::viewport::camera::Camera;
use glam::{Mat4, Vec2, Vec3};
use iced::Task;
use std::sync::Arc;
use web_time::{Duration, Instant};

/// The hover grid is built once the camera has rested this long; while it moves there is no
/// structural hover.
pub(crate) const HOVER_REST: Duration = Duration::from_millis(120);

/// What a hover grid is built from: the structural instances as drawn (the display, the units and
/// their state: offsets of the exploded view and hidden flags, the models shown), the camera, the
/// viewport and the up axis. It
/// holds the vectors themselves and compares them by pointer, so none can be freed and its address
/// reused for another while the key lives.
#[derive(Clone)]
pub struct HoverKey {
    model: u64,
    display: Arc<StructuralDisplay>,
    explode: Option<Arc<ExplodeIndex>>,
    offsets: Arc<Vec<[f32; 4]>>,
    shown: Option<Arc<Vec<bool>>>,
    camera: Camera,
    size: (f32, f32),
    model_matrix: Mat4,
}

impl PartialEq for HoverKey {
    fn eq(&self, other: &Self) -> bool {
        fn same<T>(a: &Option<Arc<T>>, b: &Option<Arc<T>>) -> bool {
            match (a, b) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
        }
        self.model == other.model
            && Arc::ptr_eq(&self.display, &other.display)
            && same(&self.explode, &other.explode)
            && Arc::ptr_eq(&self.offsets, &other.offsets)
            && same(&self.shown, &other.shown)
            && self.camera == other.camera
            && self.size == other.size
            && self.model_matrix == other.model_matrix
    }
}

impl std::fmt::Debug for HoverKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HoverKey").field("model", &self.model).field("size", &self.size).finish_non_exhaustive()
    }
}

impl HoverKey {
    /// The instances as drawn for this key.
    pub fn scene(&self) -> Scene<'_> {
        let (width, height) = self.size;
        let aspect = width / height.max(1.0);
        let view = View {
            view_proj: self.camera.view_proj(aspect),
            model: self.model_matrix,
            proj11: self.camera.projection(aspect).y_axis.y,
            size: Vec2::new(width, height),
        };
        Scene {
            display: &self.display,
            units: self.explode.as_deref().map_or(&[][..], ExplodeIndex::display_units),
            offsets: &self.offsets,
            view,
            lod: Lod::default(),
            shown_items: self.shown.as_deref().map(|v| &v[..]),
        }
    }
}

/// A hover grid built for its key (`Message::StructuralGrid`).
pub type BuiltGrid = Arc<(HoverKey, HoverGrid)>;

/// Builds the hover grid of `key` on a thread of its own.
#[cfg(not(target_arch = "wasm32"))]
fn grid_task(key: HoverKey) -> Task<Message> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let grid = key.scene().grid();
                let _ = tx.send(Arc::new((key, grid)));
            });
            rx.await.ok()
        },
        Message::StructuralGrid,
    )
}

/// Builds the hover grid of `key` in the browser (no threads there): after the next frame, so the
/// frame that is due is not held up.
#[cfg(target_arch = "wasm32")]
fn grid_task(key: HoverKey) -> Task<Message> {
    Task::perform(
        async move {
            crate::web::next_frame().await;
            let grid = key.scene().grid();
            Some(Arc::new((key, grid)))
        },
        Message::StructuralGrid,
    )
}

/// The structural selection and hover.
#[derive(Debug, Default)]
pub(crate) struct StructuralView {
    /// Selected element ids; the first one's properties show.
    pub selected: Vec<u64>,
    /// Its instance ranges, for the renderer.
    pub selected_marks: Arc<Marks>,
    /// The element under the cursor and its instance ranges.
    pub hovered: Option<u64>,
    pub hovered_marks: Arc<Marks>,
    /// Per display item, whether its source model is shown, and the instance ranges of those
    /// (`None`: every model is).
    pub shown_items: Option<Arc<Vec<bool>>>,
    pub shown_marks: Option<Arc<Marks>>,
    /// Every layer was hidden ("Hide all"): the structural objects of every model too.
    pub all_hidden: bool,
    grid: Option<BuiltGrid>,
    /// The key of the grid being built.
    building: Option<HoverKey>,
    /// Where the cursor was last reported (x, y, width, height), to look again when the grid
    /// arrives.
    cursor: Option<(f32, f32, f32, f32)>,
    /// The camera as last seen by `App::update`, and since when.
    camera_seen: Option<(Camera, Instant)>,
}

impl StructuralView {
    /// Forgets the selection, the hover, what is hidden and the grid (another model).
    pub fn reset(&mut self) {
        *self = Self { camera_seen: self.camera_seen, ..Self::default() };
    }

    /// The camera after an update: remembers when it last changed. True when it did change: the
    /// hover then no longer lies under the cursor.
    pub fn note_camera(&mut self, camera: Camera) -> bool {
        if self.camera_seen.is_some_and(|(c, _)| c == camera) {
            return false;
        }
        self.camera_seen = Some((camera, Instant::now()));
        self.grid = None;
        true
    }

    fn resting(&self) -> bool {
        self.camera_seen.is_none_or(|(_, since)| since.elapsed() >= HOVER_REST)
    }

    /// The grid, if it was built for `key`.
    fn grid_for(&self, key: &HoverKey) -> Option<&HoverGrid> {
        self.grid.as_deref().filter(|(k, _)| k == key).map(|(_, g)| g)
    }
}

impl App {
    /// What the structural instances are drawn from now, in a viewport of width × height
    /// (logical pixels).
    fn structural_key(&self, width: f32, height: f32) -> Option<HoverKey> {
        let model = self.model.as_ref()?;
        let display = model.structural_display.clone().filter(|d| !d.is_empty())?;
        Some(HoverKey {
            model: model.id,
            display,
            explode: self.explode_index.clone(),
            offsets: self.units.clone(),
            shown: self.structural.shown_items.clone(),
            camera: self.camera,
            size: (width, height),
            model_matrix: self.model_matrix(),
        })
    }

    /// Whether hit `h` is not behind `triangle` (the world point of the triangle under the same
    /// cursor): a triangle in front of it by more than 1 % of the view distance hides it.
    fn not_hidden_by(&self, triangle: Option<Vec3>) -> impl Fn(&Hit) -> bool {
        let eye = self.camera.eye();
        let forward = (self.camera.target - eye).normalize_or_zero();
        let depth = move |w: Vec3| (w - eye).dot(forward);
        let margin = 0.01 * self.camera.distance;
        move |h: &Hit| triangle.is_none_or(|t| depth(t) >= depth(h.world) - margin)
    }

    /// The structural node or member under (x, y), leaving out what a triangle lies clearly in
    /// front of (`triangle` is the world point of the triangle the same click hits) before a node
    /// is preferred over a member. Looks in the hover grid when it is fresh, else at everything.
    pub(crate) fn pick_structural(&self, x: f32, y: f32, width: f32, height: f32, triangle: Option<Vec3>) -> Option<Hit> {
        let key = self.structural_key(width, height)?;
        let scene = key.scene();
        let shown = self.shown();
        let cursor = Vec2::new(x, y);
        let hits = match self.structural.grid_for(&key) {
            Some(grid) => grid.hits(&scene, cursor, &shown),
            None => scene.hits(cursor, &shown),
        };
        let visible = self.not_hidden_by(triangle);
        best(hits.into_iter().filter(|h| visible(h)))
    }

    /// Selects structural element ids (none: clears), the first one's properties shown. Leaves the
    /// mesh selection alone.
    pub(crate) fn set_structural_selection(&mut self, ids: Vec<u64>) {
        self.structural.selected_marks = Arc::new(self.structural_marks(&ids));
        self.structural.selected = ids;
    }

    /// A click on structural element `id`: without `add` it is the selection (the mesh selection
    /// cleared); with `add` (Ctrl) it is added, or removed when it was selected, and the rest of the
    /// selection stays.
    pub(crate) fn click_structural(&mut self, id: u64, add: bool) {
        if !add {
            self.click(None, false);
            return self.set_structural_selection(vec![id]);
        }
        let mut ids = self.structural.selected.clone();
        match ids.iter().position(|&s| s == id) {
            Some(k) => {
                ids.remove(k);
            }
            None => ids.push(id),
        }
        self.set_structural_selection(ids);
    }

    /// The instance ranges that draw `ids`: a node or member itself, the item a load or line
    /// support sits on.
    fn structural_marks(&self, ids: &[u64]) -> Marks {
        let Some(model) = &self.model else { return Marks::default() };
        let Some(display) = &model.structural_display else { return Marks::default() };
        let items = ids
            .iter()
            .filter_map(|&id| model.structural_item(id)?.unit_key())
            .filter_map(|unit| display.item(unit));
        display.marks(items)
    }

    /// Looks up the structural object under the cursor (viewport point (x, y) of width ×
    /// height) for the hover highlight, in a screen grid. Once the camera rests and no grid is
    /// there for it, starts building one (the task's message looks again).
    pub(crate) fn hover_structural(&mut self, x: f32, y: f32, width: f32, height: f32) -> Task<Message> {
        self.structural.cursor = Some((x, y, width, height));
        let Some(key) = self.structural_key(width, height) else {
            self.set_structural_hover(None);
            return Task::none();
        };
        let Some(grid) = self.structural.grid_for(&key) else {
            self.structural.grid = None;
            self.set_structural_hover(None);
            if !self.structural.resting() || self.structural.building.as_ref() == Some(&key) {
                return Task::none();
            }
            self.structural.building = Some(key.clone());
            return grid_task(key);
        };
        let hits = grid.hits(&key.scene(), Vec2::new(x, y), &self.shown());
        // what a triangle hides is not hovered (the exploded view has no triangle index: there
        // the ray would test every triangle on each mouse move, so it is left out)
        let triangle = if hits.is_empty() || self.explode_on { None } else { self.triangle_under(x, y, width, height) };
        let visible = self.not_hidden_by(triangle);
        let hit = best(hits.into_iter().filter(|h| visible(h)));
        self.set_structural_hover(hit.map(|h| h.element));
        Task::none()
    }

    /// A hover grid arrived: kept if it is still the one wanted, and the cursor looked up again.
    pub(crate) fn structural_grid_built(&mut self, built: Option<BuiltGrid>) -> Task<Message> {
        let Some(built) = built else {
            self.structural.building = None;
            return Task::none();
        };
        if self.structural.building.as_ref() == Some(&built.0) {
            self.structural.building = None;
        }
        let (width, height) = built.0.size;
        if self.structural_key(width, height).as_ref() != Some(&built.0) {
            return Task::none();
        }
        self.structural.grid = Some(built);
        match self.structural.cursor {
            Some((x, y, w, h)) if self.measure.tool.is_none() && !self.plane_picking => self.hover_structural(x, y, w, h),
            _ => Task::none(),
        }
    }

    /// The world point of the visible triangle under viewport point (x, y), if any.
    fn triangle_under(&mut self, x: f32, y: f32, width: f32, height: f32) -> Option<Vec3> {
        let model = self.model.clone().filter(|m| !m.layers.is_empty())?;
        if !self.visible.iter().any(|&v| v) {
            return None;
        }
        let (width, height) = (width.max(1.0), height.max(1.0));
        let (origin, dir, hit) = self.cast(&model, x, y, width, height);
        let (l, t, s) = hit?;
        let (p, _) = super::face_at(&model.layers[l], t, origin - s, dir)?;
        Some(self.model_matrix().transform_point3(p + s))
    }

    pub(crate) fn set_structural_hover(&mut self, element: Option<u64>) {
        if self.structural.hovered == element {
            return;
        }
        self.structural.hovered = element;
        self.structural.hovered_marks = Arc::new(element.map(|id| self.structural_marks(&[id])).unwrap_or_default());
    }

    /// The cursor left the viewport.
    pub(crate) fn structural_hover_off(&mut self) {
        self.structural.cursor = None;
        self.set_structural_hover(None);
    }

    /// Whether the viewport reports the cursor for the structural hover.
    pub(crate) fn structural_hover_on(&self) -> bool {
        self.model.as_ref().and_then(|m| m.structural_display.as_ref()).is_some_and(|d| !d.is_empty())
    }

    /// Works out which structural objects are shown from the models list (a hidden model hides
    /// its structural objects) and "Hide all"; drops the hidden ones from the selection and the
    /// hover. Cheap when nothing changed.
    pub(crate) fn refresh_structural_visibility(&mut self) {
        let Some(model) = self.model.clone() else { return };
        let Some(display) = model.structural_display.as_ref().filter(|d| !d.is_empty()) else { return };
        // per source model (an index into the origins)
        let mut on = vec![!self.structural.all_hidden; model.origins.len().max(1)];
        let mut first = 0;
        for entry in &self.models {
            let n = entry.model.origins.len().max(1);
            for shown in on.iter_mut().skip(first).take(n) {
                *shown &= entry.visible;
            }
            first += n;
        }
        // `None` while every item is shown (a hidden model may have no structural objects)
        let items: Option<Vec<bool>> = (!on.iter().all(|&v| v))
            .then(|| display.items.iter().map(|i| on.get(i.model as usize).copied().unwrap_or(true)).collect::<Vec<bool>>())
            .filter(|items| !items.iter().all(|&v| v));
        if items.as_ref() == self.structural.shown_items.as_deref() {
            return;
        }
        self.structural.shown_marks = items.as_ref().map(|items| {
            let shown = items.iter().enumerate().filter(|(_, &v)| v).map(|(k, _)| k as u32);
            Arc::new(display.marks(shown))
        });
        self.structural.shown_items = items.map(Arc::new);
        // what is hidden is neither selected nor hovered
        let shown = self.structural.shown_items.clone();
        let hidden = |id: u64| {
            let item = model.structural_item(id).and_then(|i| i.unit_key()).and_then(|u| display.item(u));
            item.zip(shown.as_deref()).is_some_and(|(i, s)| !s.get(i as usize).copied().unwrap_or(true))
        };
        let selected: Vec<u64> = self.structural.selected.iter().copied().filter(|&id| !hidden(id)).collect();
        let hovered_hidden = self.structural.hovered.is_some_and(&hidden);
        if selected.len() != self.structural.selected.len() {
            self.set_structural_selection(selected);
        }
        if hovered_hidden {
            self.set_structural_hover(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Message;
    use crate::loaders;
    use crate::viewport::Navigate;
    use std::path::PathBuf;

    fn portal() -> App {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/bim-structural/tests/fixtures/portal_frame.samjson");
        let mut app = App::default();
        app.viewport_size = (800.0, 600.0);
        app.set_model(Arc::new(loaders::load(&path).expect("portal frame")));
        app
    }

    /// Viewport point of display point `p` (model space) as the app draws it now.
    fn screen(app: &App, p: [f32; 3]) -> (f32, f32) {
        let (w, h) = app.viewport_size;
        let c = app.camera.view_proj(w / h) * app.model_matrix() * Vec3::from(p).extend(1.0);
        ((c.x / c.w + 1.0) * 0.5 * w, (1.0 - c.y / c.w) * 0.5 * h)
    }

    /// The camera has rested long enough for the hover.
    fn rest(app: &mut App) {
        app.structural.camera_seen = Some((app.camera, Instant::now() - HOVER_REST * 2));
    }

    /// Hovers (x, y); a grid the hover starts building is built here and handed back, as its task
    /// would.
    fn hover(app: &mut App, x: f32, y: f32) {
        let (w, h) = app.viewport_size;
        let _ = app.update(Message::Navigate(Navigate::Hover { x, y, width: w, height: h }));
        if let Some(key) = app.structural.building.clone() {
            let grid = key.scene().grid();
            let _ = app.update(Message::StructuralGrid(Some(Arc::new((key, grid)))));
        }
    }

    #[test]
    fn clicking_a_node_or_member_selects_it_and_shows_its_properties() {
        let mut app = portal();
        let model = app.model.clone().unwrap();
        let display = model.structural_display.clone().expect("a display");
        // 4 nodes, 3 members
        assert_eq!(display.glyphs.len(), 4);
        assert_eq!(display.items.len(), 7);
        let (w, h) = app.viewport_size;
        // the first node, a few pixels off
        let node = display.items.iter().position(|i| i.kind == bim_structural::ItemKind::Node).unwrap();
        let g = display.items[node].glyphs.0 as usize;
        let (x, y) = screen(&app, display.glyphs[g].pos);
        let _ = app.update(Message::Navigate(Navigate::Pick { x: x + 4.0, y: y - 3.0, width: w, height: h, add: false }));
        assert_eq!(app.structural.selected, vec![display.items[node].element]);
        assert!(app.selected.is_none() && app.selection.is_empty());
        assert_eq!(app.structural.selected_marks.glyphs, vec![(g as u32, 1)]);
        let props = model.element_props(app.structural.selected[0]);
        assert!(props.iter().any(|(k, v)| k == "Class" && v == "Structural node"), "{props:?}");
        // the middle of a member
        let member = display.items.iter().position(|i| i.kind == bim_structural::ItemKind::Member).unwrap();
        let s = display.items[member].segs.0 as usize;
        let seg = display.segs[s];
        let mid = [0, 1, 2].map(|k| 0.5 * (seg.a[k] + seg.b[k]));
        let (x, y) = screen(&app, mid);
        let _ = app.update(Message::Navigate(Navigate::Pick { x, y: y + 2.0, width: w, height: h, add: false }));
        assert_eq!(app.structural.selected, vec![display.items[member].element]);
        assert_eq!(app.structural.selected_marks.segs, vec![display.items[member].segs]);
        assert!(app.selection_bounds().is_some(), "zoom to selection frames it");
        // empty space clears it; so does Esc
        let _ = app.update(Message::Navigate(Navigate::Pick { x: 2.0, y: 2.0, width: w, height: h, add: false }));
        assert!(app.structural.selected.is_empty() && app.structural.selected_marks.is_empty());
        let _ = app.update(Message::Navigate(Navigate::Pick { x, y, width: w, height: h, add: false }));
        assert!(!app.structural.selected.is_empty());
        let _ = app.update(Message::Escape);
        assert!(app.structural.selected.is_empty());
        // Ctrl-click adds a node to the member, and again removes it
        let (nx, ny) = screen(&app, display.glyphs[g].pos);
        let _ = app.update(Message::Navigate(Navigate::Pick { x, y, width: w, height: h, add: false }));
        let _ = app.update(Message::Navigate(Navigate::Pick { x: nx, y: ny, width: w, height: h, add: true }));
        assert_eq!(app.structural.selected, vec![display.items[member].element, display.items[node].element]);
        assert_eq!(app.structural.selected_marks.glyphs, vec![(g as u32, 1)]);
        let _ = app.update(Message::Navigate(Navigate::Pick { x: nx, y: ny, width: w, height: h, add: true }));
        assert_eq!(app.structural.selected, vec![display.items[member].element]);
        // Ctrl-click on empty space keeps it; a plain click there clears it
        let _ = app.update(Message::Navigate(Navigate::Pick { x: 2.0, y: 2.0, width: w, height: h, add: true }));
        assert_eq!(app.structural.selected.len(), 1);
        let _ = app.update(Message::Navigate(Navigate::Pick { x: 2.0, y: 2.0, width: w, height: h, add: false }));
        assert!(app.structural.selected.is_empty());
    }

    #[test]
    fn a_node_is_picked_with_the_section_box_on_and_not_when_cut_away() {
        let mut app = portal();
        let model = app.model.clone().unwrap();
        let display = model.structural_display.clone().unwrap();
        let (w, h) = app.viewport_size;
        // the highest node, and a box round the model's lower half
        let top = (0..display.glyphs.len()).max_by(|&a, &b| display.glyphs[a].pos[2].total_cmp(&display.glyphs[b].pos[2])).unwrap();
        let low = (0..display.glyphs.len()).min_by(|&a, &b| display.glyphs[a].pos[2].total_cmp(&display.glyphs[b].pos[2])).unwrap();
        let mut b = crate::section::SectionBox::around(&app.world_bounds().unwrap());
        b.max.z = 0.5 * (b.min.z + b.max.z);
        app.section = Some(b);
        app.section_on = true;
        let (x, y) = screen(&app, display.glyphs[low].pos);
        let _ = app.update(Message::Navigate(Navigate::Pick { x: x + 5.0, y, width: w, height: h, add: false }));
        let item = display.glyph_item[low] as usize;
        assert_eq!(app.structural.selected, vec![display.items[item].element], "a node within 8 px inside the box");
        let (x, y) = screen(&app, display.glyphs[top].pos);
        let _ = app.update(Message::Navigate(Navigate::Pick { x, y, width: w, height: h, add: false }));
        let top_element = display.items[display.glyph_item[top] as usize].element;
        assert!(!app.structural.selected.contains(&top_element), "cut away");
    }

    #[test]
    fn hovering_highlights_once_the_camera_rests() {
        let mut app = portal();
        assert!(app.structural_hover_on());
        let model = app.model.clone().unwrap();
        let display = model.structural_display.clone().unwrap();
        let (x, y) = screen(&app, display.glyphs[0].pos);
        // the camera moved just now: no hover yet, and no grid started
        app.structural.camera_seen = Some((app.camera, Instant::now()));
        hover(&mut app, x, y);
        assert_eq!(app.structural.hovered, None);
        assert!(app.structural.grid.is_none());
        // at rest: the grid is built off the UI thread, then the cursor is looked up again
        rest(&mut app);
        let (w, h) = app.viewport_size;
        let _ = app.hover_structural(x, y, w, h);
        assert_eq!(app.structural.hovered, None, "waiting for the grid");
        let key = app.structural.building.clone().expect("a grid being built");
        // another hover meanwhile starts no second build
        let _ = app.hover_structural(x + 1.0, y, w, h);
        assert_eq!(app.structural.building.as_ref(), Some(&key));
        let grid = key.scene().grid();
        let _ = app.update(Message::StructuralGrid(Some(Arc::new((key, grid)))));
        let node = display.items[display.glyph_item[0] as usize].element;
        assert_eq!(app.structural.hovered, Some(node));
        assert_eq!(app.structural.hovered_marks.glyphs, vec![(0, 1)]);
        let _ = app.update(Message::Navigate(Navigate::HoverOff));
        assert_eq!(app.structural.hovered, None);
    }

    /// The hover goes as soon as the camera moves or a measure tool starts; a grid built for other
    /// offsets of the exploded view (another vector, even at the same address) is not used.
    #[test]
    fn the_hover_goes_when_the_camera_moves_or_measuring_starts() {
        let mut app = portal();
        let display = app.model.as_ref().unwrap().structural_display.clone().unwrap();
        let (x, y) = screen(&app, display.glyphs[0].pos);
        rest(&mut app);
        hover(&mut app, x, y);
        assert!(app.structural.hovered.is_some());
        let _ = app.update(Message::Navigate(Navigate::Orbit { dx: 6.0, dy: 0.0 }));
        assert_eq!(app.structural.hovered, None, "the camera moved");
        assert!(app.structural.grid.is_none());
        let (x, y) = screen(&app, display.glyphs[0].pos);
        rest(&mut app);
        hover(&mut app, x, y);
        assert!(app.structural.hovered.is_some());
        // another units state (offsets of the exploded view, flags): the grid is stale
        let (w, h) = app.viewport_size;
        let key = app.structural_key(w, h).unwrap();
        assert!(app.structural.grid_for(&key).is_some());
        app.units = Arc::new((*app.units).clone());
        let key = app.structural_key(w, h).unwrap();
        assert!(app.structural.grid_for(&key).is_none());
        hover(&mut app, x, y);
        assert!(app.structural.hovered.is_some());
        let _ = app.update(Message::Measure(crate::measure::Tool::Distance));
        assert_eq!(app.structural.hovered, None, "measuring");
    }

    /// Hide and Isolate (the ribbon, H and I, the right-click menu) take a node picked by a click
    /// too, though it is selected apart from `App::sel`.
    #[test]
    fn hide_and_isolate_take_a_clicked_node() {
        let mut app = portal();
        let display = app.model.as_ref().unwrap().structural_display.clone().unwrap();
        let (w, h) = app.viewport_size;
        let g = 0;
        let node = display.items[display.glyph_item[g] as usize].element;
        let unit = app.explode_index.as_ref().unwrap().structural_unit(node).unwrap() as usize;
        let hidden = |app: &App| app.units[unit][3] as u8 & crate::units::UNIT_HIDDEN != 0;
        let (x, y) = screen(&app, display.glyphs[g].pos);
        let _ = app.update(Message::Navigate(Navigate::Pick { x, y, width: w, height: h, add: false }));
        assert_eq!(app.structural.selected, vec![node]);
        let _ = app.update(Message::HideSelection);
        assert!(hidden(&app));
        assert!(app.structural.selected.is_empty(), "what is hidden is not selected");
        let _ = app.update(Message::ShowAllElements);
        let _ = app.update(Message::Navigate(Navigate::Pick { x, y, width: w, height: h, add: false }));
        let _ = app.update(Message::IsolateSelection);
        assert!(!hidden(&app));
        // the other three nodes and the three members
        assert_eq!(app.hidden.len(), 6);
    }

    /// Hiding a node through the selection (as box select and Hide do) leaves it undrawn and
    /// unpicked; showing all brings it back.
    #[test]
    fn hidden_units_are_not_picked() {
        let mut app = portal();
        let display = app.model.as_ref().unwrap().structural_display.clone().unwrap();
        let (w, h) = app.viewport_size;
        let g = 0;
        let node = display.items[display.glyph_item[g] as usize].element;
        let (x, y) = screen(&app, display.glyphs[g].pos);
        assert_eq!(app.pick_structural(x, y, w, h, None).map(|h| h.element), Some(node));
        app.sel.extend([crate::selection::Pickable::Element(node)]);
        app.selection_changed();
        let unit = app.explode_index.as_ref().unwrap().structural_unit(node).unwrap() as usize;
        assert_eq!(app.units[unit][3] as u8 & crate::units::UNIT_SELECTED, crate::units::UNIT_SELECTED, "drawn selected");
        let _ = app.update(Message::HideSelection);
        assert_eq!(app.units[unit][3] as u8 & crate::units::UNIT_HIDDEN, crate::units::UNIT_HIDDEN);
        assert_ne!(app.pick_structural(x, y, w, h, None).map(|h| h.element), Some(node), "hidden");
        rest(&mut app);
        hover(&mut app, x, y);
        assert_ne!(app.structural.hovered, Some(node));
        let _ = app.update(Message::ShowAllElements);
        assert_eq!(app.pick_structural(x, y, w, h, None).map(|h| h.element), Some(node));
    }

    fn node(id: u64, p: Vec3) -> bim_structural::Node {
        bim_structural::Node {
            id: bim_structural::sam_id(bim_structural::IdKind::Node, id),
            number: id.to_string(),
            p: p.as_dvec3(),
            frame: None,
            support: None,
            origin: bim_structural::NodeOrigin::Analytical,
        }
    }

    /// A node behind a triangle no longer wins over a member in front of it: what a triangle hides
    /// is dropped before a node is preferred.
    #[test]
    fn an_occluded_node_does_not_hide_a_visible_member() {
        use bim_structural::{frame, sam_id, AnalysisModel, Curve, IdKind, Insertion, Member, MemberKind, Orientation, SourceInfo};
        let mut camera = Camera::default();
        camera.set_preset(crate::viewport::camera::ViewPreset::Front);
        camera.target = Vec3::new(0.0, 0.0, 10.0);
        camera.distance = 20.0;
        let eye = camera.eye();
        let forward = (camera.target - eye).normalize();
        let side = forward.cross(Vec3::Z).normalize();
        // a lone node at the target, and a member across the view halfway to the eye
        let (a, b) = (camera.target - forward * 10.0 + side * 3.0, camera.target - forward * 10.0 - side * 3.0);
        let mut doc = AnalysisModel::new(1, "Occlusion", SourceInfo::default());
        doc.nodes = vec![node(1, camera.target), node(2, a), node(3, b)];
        doc.members.push(Member {
            id: sam_id(IdKind::Member, 1),
            number: "1".into(),
            kind: MemberKind::Beam,
            nodes: vec![doc.nodes[1].id, doc.nodes[2].id],
            geometry: Curve::Line,
            z_ref: frame::default_z_ref((b - a).as_dvec3()),
            orientation: Orientation::Default,
            section: None,
            section_rotation: 0.0,
            insertion: Insertion::default(),
            releases: [None, None],
            material: None,
            links: Vec::new(),
        });
        doc.rebuild_index();
        let mut model = crate::model::Model::new("occlusion".into(), "test", Vec::new());
        model.set_structural(vec![crate::structural::StructuralPart::new(0, Arc::new(doc))]);
        let mut app = App::default();
        app.viewport_size = (800.0, 600.0);
        app.set_model(Arc::new(model));
        app.camera = camera;
        let display = app.model.as_ref().unwrap().structural_display.clone().unwrap();
        assert_eq!(display.glyphs.iter().find(|g| g.param[0] == 0.0).map(|g| g.pos), Some(camera.target.to_array()), "no origin shift");
        let (cx, cy) = screen(&app, camera.target.to_array());
        let (x, y) = (cx, cy + 2.0);
        // nothing in front: the node wins
        let hit = app.pick_structural(x, y, 800.0, 600.0, None).expect("a hit");
        assert!(hit.node, "{hit:?}");
        // a triangle between the member and the node: the member
        let wall = camera.target - forward * 5.0;
        let hit = app.pick_structural(x, y, 800.0, 600.0, Some(wall)).expect("the member in front of the wall");
        assert!(!hit.node, "{hit:?}");
        // a triangle in front of both: nothing
        assert!(app.pick_structural(x, y, 800.0, 600.0, Some(camera.target - forward * 15.0)).is_none());
    }

    /// Hiding a model of a combined set, or every layer, hides its structural objects: not drawn
    /// (the renderer draws the shown ranges) and not picked.
    #[test]
    fn hidden_models_hide_their_structural_objects() {
        let mut app = portal();
        app.add_model(Arc::new(crate::bench::synthetic(4)));
        assert_eq!(app.models.len(), 2);
        let display = app.model.as_ref().unwrap().structural_display.clone().unwrap();
        let (w, h) = app.viewport_size;
        let g = 0;
        let (x, y) = screen(&app, display.glyphs[g].pos);
        let node = display.items[display.glyph_item[g] as usize].element;
        assert!(app.structural.shown_marks.is_none());
        let _ = app.update(Message::Navigate(Navigate::Pick { x, y, width: w, height: h, add: false }));
        assert_eq!(app.structural.selected, vec![node]);
        // the analysis model is the first of the list
        let _ = app.update(Message::ModelVisible(0, false));
        assert_eq!(app.structural.shown_marks.as_deref(), Some(&Marks::default()), "nothing of it drawn");
        assert!(app.structural.selected.is_empty());
        let _ = app.update(Message::Navigate(Navigate::Pick { x, y, width: w, height: h, add: false }));
        assert!(app.structural.selected.is_empty(), "not picked while hidden");
        rest(&mut app);
        hover(&mut app, x, y);
        assert_eq!(app.structural.hovered, None);
        // the other model hidden instead: the analysis model is back
        let _ = app.update(Message::ModelVisible(0, true));
        let _ = app.update(Message::ModelVisible(1, false));
        assert!(app.structural.shown_marks.is_none());
        let _ = app.update(Message::Navigate(Navigate::Pick { x, y, width: w, height: h, add: false }));
        assert_eq!(app.structural.selected, vec![node]);
        // hide all, show all
        let _ = app.update(Message::AllLayers(false));
        assert_eq!(app.structural.shown_marks.as_deref(), Some(&Marks::default()));
        assert!(app.structural.selected.is_empty());
        let _ = app.update(Message::AllLayers(true));
        assert!(app.structural.shown_marks.is_none());
        // a model of only structural objects: hide all hides them
        let mut app = portal();
        let (x, y) = screen(&app, display.glyphs[g].pos);
        assert!(app.pick_structural(x, y, w, h, None).is_some());
        let _ = app.update(Message::AllLayers(false));
        assert!(app.pick_structural(x, y, w, h, None).is_none());
        let _ = app.update(Message::AllLayers(true));
        assert!(app.structural.shown_marks.is_none());
    }
}
