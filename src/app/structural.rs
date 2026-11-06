//! Structural objects in the viewport: picking nodes and members on screen, their selection
//! (drawn again on top, as the mesh highlight) and the hover highlight.
//!
//! The selection holds element ids (a load or line support may be selected; its member or node is
//! what lights up, see `StructuralItem::unit_key`). Main has one selection mechanism, index ranges
//! redrawn highlighted; structural instances follow it with instance ranges ([`Marks`]). Ctrl-click
//! multi-select, hide, isolate and undo come with the units-texture flags of PR #91; the seam is
//! [`App::set_structural_selection`], which takes any number of ids.

use super::App;
use crate::structural::display::Marks;
use crate::structural::pick::{HoverGrid, Hit, Lod, Scene, View};
use crate::viewport::camera::Camera;
use glam::{Mat4, Vec2, Vec3};
use std::sync::Arc;
use web_time::{Duration, Instant};

/// The hover grid is built once the camera has rested this long; while it moves there is no
/// structural hover.
pub(crate) const HOVER_REST: Duration = Duration::from_millis(120);

/// What a hover grid was built for: the model, the camera, the viewport, the up axis and the
/// exploded view.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HoverKey {
    model: u64,
    camera: Camera,
    size: (f32, f32),
    model_matrix: Mat4,
    offsets: usize,
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
    grid: Option<(HoverKey, HoverGrid)>,
    /// The camera as last seen by `App::update`, and since when.
    camera_seen: Option<(Camera, Instant)>,
}

impl StructuralView {
    /// Forgets the selection, the hover and the grid (another model).
    pub fn reset(&mut self) {
        *self = Self { camera_seen: self.camera_seen, ..Self::default() };
    }

    /// The camera after an update: remembers when it last changed.
    pub fn note_camera(&mut self, camera: Camera) {
        if !self.camera_seen.is_some_and(|(c, _)| c == camera) {
            self.camera_seen = Some((camera, Instant::now()));
        }
    }

    fn resting(&self) -> bool {
        self.camera_seen.is_none_or(|(_, since)| since.elapsed() >= HOVER_REST)
    }
}

impl App {
    /// The structural instances as drawn now in a viewport of width × height (logical pixels).
    fn structural_scene(&self, width: f32, height: f32) -> Option<Scene<'_>> {
        let model = self.model.as_ref()?;
        let display = model.structural_display.as_deref().filter(|d| !d.is_empty())?;
        let aspect = width / height.max(1.0);
        let view = View {
            view_proj: self.camera.view_proj(aspect),
            model: self.model_matrix(),
            proj11: self.camera.projection(aspect).y_axis.y,
            size: Vec2::new(width, height),
        };
        let units = self.explode_index.as_ref().map_or(&[][..], |e| e.display_units());
        Some(Scene { display, units, offsets: &self.explode_offsets, view, lod: Lod::default() })
    }

    /// The structural node or member under (x, y), unless a triangle lies clearly in front of
    /// it: closer to the eye by more than 1 % of the view distance. `triangle` is the world point
    /// of the triangle the same click hits.
    pub(crate) fn pick_structural(&self, x: f32, y: f32, width: f32, height: f32, triangle: Option<Vec3>) -> Option<Hit> {
        let scene = self.structural_scene(width, height)?;
        let shown = self.shown();
        let hit = scene.pick(Vec2::new(x, y), &shown)?;
        let eye = self.camera.eye();
        let forward = (self.camera.target - eye).normalize_or_zero();
        let depth = |w: Vec3| (w - eye).dot(forward);
        match triangle {
            Some(t) if depth(t) < depth(hit.world) - 0.01 * self.camera.distance => None,
            _ => Some(hit),
        }
    }

    /// Selects structural element ids (none: clears), the first one's properties shown. Leaves the
    /// mesh selection alone.
    pub(crate) fn set_structural_selection(&mut self, ids: Vec<u64>) {
        self.structural.selected_marks = Arc::new(self.structural_marks(&ids));
        self.structural.selected = ids;
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
    /// height) for the hover highlight, in a screen grid built once the camera rests.
    pub(crate) fn hover_structural(&mut self, x: f32, y: f32, width: f32, height: f32) {
        let key = self.model.as_ref().map(|m| HoverKey {
            model: m.id,
            camera: self.camera,
            size: (width, height),
            model_matrix: self.model_matrix(),
            offsets: Arc::as_ptr(&self.explode_offsets) as usize,
        });
        let fresh = matches!((&self.structural.grid, &key), (Some((k, _)), Some(key)) if k == key);
        if !fresh {
            self.structural.grid = None;
            if !self.structural.resting() {
                return self.set_structural_hover(None);
            }
            let (Some(key), Some(scene)) = (key, self.structural_scene(width, height)) else {
                return self.set_structural_hover(None);
            };
            let grid = scene.grid();
            self.structural.grid = Some((key, grid));
        }
        let hit = match (&self.structural.grid, self.structural_scene(width, height)) {
            (Some((_, grid)), Some(scene)) => grid.pick(&scene, Vec2::new(x, y), &self.shown()),
            _ => None,
        };
        self.set_structural_hover(hit.map(|h| h.element));
    }

    pub(crate) fn set_structural_hover(&mut self, element: Option<u64>) {
        if self.structural.hovered == element {
            return;
        }
        self.structural.hovered = element;
        self.structural.hovered_marks = Arc::new(element.map(|id| self.structural_marks(&[id])).unwrap_or_default());
    }

    /// Whether the viewport reports the cursor for the structural hover.
    pub(crate) fn structural_hover_on(&self) -> bool {
        self.model.as_ref().and_then(|m| m.structural_display.as_ref()).is_some_and(|d| !d.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loaders;
    use crate::viewport::Navigate;
    use crate::app::Message;
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
        let _ = app.update(Message::Navigate(Navigate::Pick { x: x + 4.0, y: y - 3.0, width: w, height: h }));
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
        let _ = app.update(Message::Navigate(Navigate::Pick { x, y: y + 2.0, width: w, height: h }));
        assert_eq!(app.structural.selected, vec![display.items[member].element]);
        assert_eq!(app.structural.selected_marks.segs, vec![display.items[member].segs]);
        assert!(app.selection_bounds().is_some(), "zoom to selection frames it");
        // empty space clears it; so does Esc
        let _ = app.update(Message::Navigate(Navigate::Pick { x: 2.0, y: 2.0, width: w, height: h }));
        assert!(app.structural.selected.is_empty() && app.structural.selected_marks.is_empty());
        let _ = app.update(Message::Navigate(Navigate::Pick { x, y, width: w, height: h }));
        assert!(!app.structural.selected.is_empty());
        let _ = app.update(Message::Escape);
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
        let _ = app.update(Message::Navigate(Navigate::Pick { x: x + 5.0, y, width: w, height: h }));
        let item = display.glyph_item[low] as usize;
        assert_eq!(app.structural.selected, vec![display.items[item].element], "a node within 8 px inside the box");
        let (x, y) = screen(&app, display.glyphs[top].pos);
        let _ = app.update(Message::Navigate(Navigate::Pick { x, y, width: w, height: h }));
        let top_element = display.items[display.glyph_item[top] as usize].element;
        assert!(!app.structural.selected.contains(&top_element), "cut away");
    }

    #[test]
    fn hovering_highlights_once_the_camera_rests() {
        let mut app = portal();
        assert!(app.structural_hover_on());
        let model = app.model.clone().unwrap();
        let display = model.structural_display.clone().unwrap();
        let (w, h) = app.viewport_size;
        let (x, y) = screen(&app, display.glyphs[0].pos);
        // the camera moved just now: no hover yet
        app.structural.note_camera(app.camera);
        app.structural.camera_seen = Some((app.camera, Instant::now()));
        app.hover_structural(x, y, w, h);
        assert_eq!(app.structural.hovered, None);
        // at rest
        app.structural.camera_seen = Some((app.camera, Instant::now() - HOVER_REST * 2));
        app.hover_structural(x, y, w, h);
        let node = display.items[display.glyph_item[0] as usize].element;
        assert_eq!(app.structural.hovered, Some(node));
        assert_eq!(app.structural.hovered_marks.glyphs, vec![(0, 1)]);
        let _ = app.update(Message::Navigate(Navigate::HoverOff));
        assert_eq!(app.structural.hovered, None);
    }
}
