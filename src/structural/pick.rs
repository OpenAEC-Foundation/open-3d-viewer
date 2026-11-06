//! Screen-space picking of structural nodes and members, for clicks and hover.
//!
//! A click projects every node and member segment as the renderer draws them (moved by the
//! exploded view, cut by the section box and plane, nodes dropped by the level of detail) and
//! takes the nearest node within [`NODE_PX`] pixels, else the nearest member within
//! [`MEMBER_PX`]. A segment that crosses the near plane is clipped against `w = NEAR_W` first, as
//! the vertex shader does. The hover looks in a [`HoverGrid`] of the projected instances, built
//! once the camera has come to rest, instead of scanning everything on every mouse move.

use super::display::{GlyphInstance, GlyphKind, StructuralDisplay};
use glam::{Mat4, Vec2, Vec3, Vec4, Vec4Swizzles};

/// A node within this many (logical) pixels of the cursor wins over any member.
pub const NODE_PX: f32 = 8.0;
/// A member is picked within this many pixels of its axis.
pub const MEMBER_PX: f32 = 6.0;
/// Segments are clipped against `w = NEAR_W` before they are projected (`NEAR_W` in
/// structural.wgsl).
pub const NEAR_W: f32 = 1e-5;

/// Sizes in logical pixels that decide what is drawn: the default glyph size, the size below
/// which a glyph is dropped, and how far apart (projected) the ends of a node's shortest member
/// must be for the node to show.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lod {
    pub glyph_px: f32,
    pub min_glyph_px: f32,
    pub node_cluster_px: f32,
}

impl Default for Lod {
    fn default() -> Self {
        Self { glyph_px: 7.0, min_glyph_px: 3.0, node_cluster_px: 6.0 }
    }
}

/// World units per pixel at clip `w`, for perspective and orthographic projections alike
/// (`proj11` is the projection's `[1][1]`, `height` the viewport height in pixels).
pub fn world_per_pixel(w: f32, height: f32, proj11: f32) -> f32 {
    2.0 * w / (height * proj11).max(1e-12)
}

/// The size a glyph is drawn at, in pixels: its pixel size (or `default_px`), shrunk to its world
/// cap.
pub fn glyph_px(g: &GlyphInstance, default_px: f32, wpp: f32) -> f32 {
    let px = if g.size[0] > 0.0 { g.size[0] } else { default_px };
    let world = px * wpp;
    let world = if g.size[1] > 0.0 { world.min(g.size[1]) } else { world };
    world / wpp.max(1e-30)
}

/// Whether the vertex shader draws glyph `g` at `wpp` world units per pixel: large enough, and a
/// node only while its shortest member spans enough pixels.
pub fn glyph_drawn(g: &GlyphInstance, wpp: f32, lod: &Lod) -> bool {
    if glyph_px(g, lod.glyph_px, wpp) < lod.min_glyph_px {
        return false;
    }
    let kind = g.kind_lod & 0xff;
    !(kind == GlyphKind::Node as u32 && g.param[0] > 0.0 && g.param[0] / wpp < lod.node_cluster_px)
}

/// Clips the clip-space segment `a`-`b` to `w >= near`: the clipped ends and their parameters
/// along the segment, `None` when it lies wholly behind.
pub fn clip_near(a: Vec4, b: Vec4, near: f32) -> Option<(Vec4, Vec4, f32, f32)> {
    match (a.w >= near, b.w >= near) {
        (true, true) => Some((a, b, 0.0, 1.0)),
        (false, false) => None,
        (false, true) => {
            let t = (near - a.w) / (b.w - a.w);
            Some((a.lerp(b, t), b, t, 1.0))
        }
        (true, false) => {
            let t = (near - a.w) / (b.w - a.w);
            Some((a, a.lerp(b, t), 0.0, t))
        }
    }
}

/// The parameter along a segment (in the space before projection) of the point at screen
/// parameter `s` between projected ends of clip `w` `wa` and `wb` (perspective-correct).
pub fn unproject_param(s: f32, wa: f32, wb: f32) -> f32 {
    let d = (1.0 - s) * wb + s * wa;
    if d.abs() < 1e-30 { s } else { s * wa / d }
}

/// The parameter of the point of segment `a`-`b` nearest `p`, and its distance.
pub fn nearest_on_segment(p: Vec2, a: Vec2, b: Vec2) -> (f32, f32) {
    let d = b - a;
    let len2 = d.length_squared();
    let s = if len2 > 0.0 { ((p - a).dot(d) / len2).clamp(0.0, 1.0) } else { 0.0 };
    (s, p.distance(a + d * s))
}

/// The camera and viewport a pick is made in.
#[derive(Debug, Clone, Copy)]
pub struct View {
    /// The camera's view and projection.
    pub view_proj: Mat4,
    /// The model matrix (up-axis override).
    pub model: Mat4,
    /// The projection's `[1][1]`.
    pub proj11: f32,
    /// The viewport in logical pixels.
    pub size: Vec2,
}

impl View {
    /// Viewport pixels (origin top left) of a clip point in front of the camera.
    pub fn to_screen(&self, c: Vec4) -> Vec2 {
        let ndc = c.xy() / c.w;
        Vec2::new((ndc.x + 1.0) * 0.5 * self.size.x, (1.0 - ndc.y) * 0.5 * self.size.y)
    }
}

/// What a pick found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    /// The display item and its element id.
    pub item: u32,
    pub element: u64,
    pub node: bool,
    /// Distance from the cursor in pixels.
    pub px: f32,
    /// The point picked, world (as drawn).
    pub world: Vec3,
}

/// The structural instances of a model as they are drawn now.
pub struct Scene<'a> {
    pub display: &'a StructuralDisplay,
    /// The unit of the exploded view of every display item (empty: none).
    pub units: &'a [u32],
    /// The offset (world) of every unit.
    pub offsets: &'a [[f32; 4]],
    pub view: View,
    pub lod: Lod,
}

impl Scene<'_> {
    fn offset(&self, item: u32) -> Vec3 {
        let unit = self.units.get(item as usize).copied().unwrap_or(0) as usize;
        self.offsets.get(unit).map_or(Vec3::ZERO, |o| Vec3::new(o[0], o[1], o[2]))
    }

    fn world(&self, p: [f32; 3], item: u32) -> Vec3 {
        self.view.model.transform_point3(Vec3::from_array(p)) + self.offset(item)
    }

    /// Node glyph `g` if it is drawn and within [`NODE_PX`] of `cursor`.
    pub fn node_hit(&self, g: usize, cursor: Vec2, shown: &dyn Fn(Vec3) -> bool) -> Option<Hit> {
        let glyph = self.display.glyphs.get(g)?;
        if glyph.kind_lod & 0xff != GlyphKind::Node as u32 {
            return None;
        }
        let item = self.display.glyph_item[g];
        let world = self.world(glyph.pos, item);
        let clip = self.view.view_proj * world.extend(1.0);
        if clip.w < NEAR_W || !shown(world) {
            return None;
        }
        let wpp = world_per_pixel(clip.w, self.view.size.y, self.view.proj11);
        if !glyph_drawn(glyph, wpp, &self.lod) {
            return None;
        }
        let px = self.view.to_screen(clip).distance(cursor);
        (px <= NODE_PX).then(|| Hit { item, element: self.display.items[item as usize].element, node: true, px, world })
    }

    /// The ends of segment `s` on screen and in the world, clipped to the near plane, with their
    /// clip `w`.
    fn project_seg(&self, s: usize) -> Option<([Vec2; 2], [Vec3; 2], [f32; 2])> {
        let seg = self.display.segs.get(s)?;
        let item = self.display.seg_item[s];
        let (wa, wb) = (self.world(seg.a, item), self.world(seg.b, item));
        let (ca, cb) = (self.view.view_proj * wa.extend(1.0), self.view.view_proj * wb.extend(1.0));
        let (ca, cb, ta, tb) = clip_near(ca, cb, NEAR_W)?;
        Some(([self.view.to_screen(ca), self.view.to_screen(cb)], [wa.lerp(wb, ta), wa.lerp(wb, tb)], [ca.w, cb.w]))
    }

    /// Member segment `s` if it passes within [`MEMBER_PX`] of `cursor` at a point the section
    /// box and plane keep.
    pub fn seg_hit(&self, s: usize, cursor: Vec2, shown: &dyn Fn(Vec3) -> bool) -> Option<Hit> {
        let ([a, b], [wa, wb], [ca, cb]) = self.project_seg(s)?;
        let (t, px) = nearest_on_segment(cursor, a, b);
        if px > MEMBER_PX {
            return None;
        }
        let world = wa.lerp(wb, unproject_param(t, ca, cb));
        if !shown(world) {
            return None;
        }
        let item = self.display.seg_item[s];
        Some(Hit { item, element: self.display.items[item as usize].element, node: false, px, world })
    }

    /// The node or member under `cursor`, looking at everything.
    pub fn pick(&self, cursor: Vec2, shown: &dyn Fn(Vec3) -> bool) -> Option<Hit> {
        let nodes = (0..self.display.glyphs.len()).filter_map(|g| self.node_hit(g, cursor, shown));
        let segs = (0..self.display.segs.len()).filter_map(|s| self.seg_hit(s, cursor, shown));
        best(nodes.chain(segs))
    }

    /// Puts the drawn nodes and segments into the cells of a screen grid, for the hover.
    pub fn grid(&self) -> HoverGrid {
        let size = self.view.size;
        let cols = (size.x / CELL).ceil().max(1.0) as usize;
        let rows = (size.y / CELL).ceil().max(1.0) as usize;
        let cell_of = |p: Vec2| {
            let (c, r) = ((p.x / CELL).floor(), (p.y / CELL).floor());
            (c >= 0.0 && r >= 0.0 && (c as usize) < cols && (r as usize) < rows).then(|| r as usize * cols + c as usize)
        };
        let mut pairs: Vec<(u32, u32)> = Vec::new();
        for (g, glyph) in self.display.glyphs.iter().enumerate() {
            if glyph.kind_lod & 0xff != GlyphKind::Node as u32 {
                continue;
            }
            let item = self.display.glyph_item[g];
            let clip = self.view.view_proj * self.world(glyph.pos, item).extend(1.0);
            if clip.w < NEAR_W {
                continue;
            }
            if let Some(c) = cell_of(self.view.to_screen(clip)) {
                pairs.push((c as u32, g as u32));
            }
        }
        let screen = (Vec2::splat(-CELL), size + CELL);
        for s in 0..self.display.segs.len() {
            let Some(([a, b], ..)) = self.project_seg(s) else { continue };
            let Some((a, b)) = clip_to_rect(a, b, screen.0, screen.1) else { continue };
            // walk the segment in half cells; a cursor within MEMBER_PX of it is in one of the
            // cells it visits or in a neighbour
            let steps = (a.distance(b) / (CELL * 0.5)).ceil().max(1.0) as usize;
            let mut last = None;
            for k in 0..=steps {
                let Some(c) = cell_of(a.lerp(b, k as f32 / steps as f32)) else { continue };
                if last != Some(c) {
                    pairs.push((c as u32, s as u32 | SEG_BIT));
                    last = Some(c);
                }
            }
        }
        pairs.sort_unstable();
        pairs.dedup();
        let mut starts = vec![0u32; cols * rows + 1];
        for &(c, _) in &pairs {
            starts[c as usize + 1] += 1;
        }
        for i in 1..starts.len() {
            starts[i] += starts[i - 1];
        }
        HoverGrid { cols, rows, starts, entries: pairs.into_iter().map(|p| p.1).collect() }
    }
}

/// Grid cells, in pixels: larger than [`NODE_PX`] and [`MEMBER_PX`], so the cell under the
/// cursor and its neighbours hold every candidate.
const CELL: f32 = 32.0;
const SEG_BIT: u32 = 1 << 31;

/// The nodes and segments of a [`Scene`] by screen cell (compressed rows).
#[derive(Debug, Clone, Default)]
pub struct HoverGrid {
    cols: usize,
    rows: usize,
    starts: Vec<u32>,
    entries: Vec<u32>,
}

impl HoverGrid {
    /// Entries (glyph and segment indices) in the grid, for tests.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The node or member under `cursor`, looking only at the cells around it. `scene` must be
    /// the one the grid was built from.
    pub fn pick(&self, scene: &Scene, cursor: Vec2, shown: &dyn Fn(Vec3) -> bool) -> Option<Hit> {
        let (c, r) = ((cursor.x / CELL).floor() as i64, (cursor.y / CELL).floor() as i64);
        let mut hits = Vec::new();
        for rr in r - 1..=r + 1 {
            for cc in c - 1..=c + 1 {
                if rr < 0 || cc < 0 || rr as usize >= self.rows || cc as usize >= self.cols {
                    continue;
                }
                let cell = rr as usize * self.cols + cc as usize;
                for &e in &self.entries[self.starts[cell] as usize..self.starts[cell + 1] as usize] {
                    let hit = if e & SEG_BIT != 0 {
                        scene.seg_hit((e & !SEG_BIT) as usize, cursor, shown)
                    } else {
                        scene.node_hit(e as usize, cursor, shown)
                    };
                    hits.extend(hit);
                }
            }
        }
        best(hits)
    }
}

/// A node within [`NODE_PX`] wins over any member; else the nearest.
fn best(hits: impl IntoIterator<Item = Hit>) -> Option<Hit> {
    hits.into_iter().min_by(|a, b| (!a.node).cmp(&!b.node).then(a.px.total_cmp(&b.px)))
}

/// The part of segment `a`-`b` inside the rectangle `lo`-`hi` (Liang-Barsky).
fn clip_to_rect(a: Vec2, b: Vec2, lo: Vec2, hi: Vec2) -> Option<(Vec2, Vec2)> {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in [(-d.x, a.x - lo.x), (d.x, hi.x - a.x), (-d.y, a.y - lo.y), (d.y, hi.y - a.y)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
            continue;
        }
        let r = q / p;
        if p < 0.0 {
            t0 = t0.max(r);
        } else {
            t1 = t1.min(r);
        }
        if t0 > t1 {
            return None;
        }
    }
    Some((a + d * t0, a + d * t1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structural::display::{style, SegInstance};
    use crate::viewport::camera::Camera;

    fn node(p: [f32; 3], shortest: f32) -> GlyphInstance {
        GlyphInstance {
            pos: p,
            rot: [0.0, 0.0, 0.0, 1.0],
            color: [0; 4],
            unit: 0,
            size: [0.0, 0.4 * shortest],
            param: [shortest, 0.0],
            kind_lod: GlyphKind::Node as u32,
        }
    }

    /// Two nodes at (0,0,0) and (4,0,0) with a member between them, a third node at (2,0,0)
    /// on the member, looked at from above.
    fn display() -> StructuralDisplay {
        let mut d = StructuralDisplay::default();
        let items = [(10u64, bim_structural::ItemKind::Node), (11, bim_structural::ItemKind::Node), (12, bim_structural::ItemKind::Node), (20, bim_structural::ItemKind::Member)];
        for (k, (element, kind)) in items.into_iter().enumerate() {
            d.items.push(crate::structural::display::DisplayItem { element, kind, segs: (0, 0), glyphs: (k as u32, 1) });
        }
        d.items[3].glyphs = (0, 0);
        d.items[3].segs = (0, 1);
        d.glyphs = vec![node([0.0; 3], 4.0), node([4.0, 0.0, 0.0], 4.0), node([2.0, 0.0, 0.0], 4.0)];
        d.glyph_item = vec![0, 1, 2];
        d.glyph_kinds = vec![(GlyphKind::Node, 0, 3)];
        d.segs = vec![SegInstance { a: [0.0; 3], a_off: [0.0; 3], b: [4.0, 0.0, 0.0], b_off: [0.0; 3], color: [0; 4], unit: 0, style: style::AXIS }];
        d.seg_item = vec![3];
        d
    }

    fn view(camera: &Camera, size: Vec2) -> View {
        let aspect = size.x / size.y;
        View { view_proj: camera.view_proj(aspect), model: Mat4::IDENTITY, proj11: camera.projection(aspect).y_axis.y, size }
    }

    fn top_camera(distance: f32) -> Camera {
        let mut c = Camera::default();
        c.set_preset(crate::viewport::camera::ViewPreset::Top);
        c.target = Vec3::new(2.0, 0.0, 0.0);
        c.distance = distance;
        c
    }

    fn screen_of(v: &View, p: Vec3) -> Vec2 {
        v.to_screen(v.view_proj * p.extend(1.0))
    }

    #[test]
    fn a_node_within_8_px_wins_over_the_member() {
        let d = display();
        let v = view(&top_camera(10.0), Vec2::new(800.0, 600.0));
        let scene = Scene { display: &d, units: &[], offsets: &[], view: v, lod: Lod::default() };
        let all = |_: Vec3| true;
        let n1 = screen_of(&v, Vec3::new(4.0, 0.0, 0.0));
        // 5 px off the end node, right on the member: the node
        let hit = scene.pick(n1 + Vec2::new(-5.0, 0.0), &all).expect("hit");
        assert!(hit.node && hit.element == 11, "{hit:?}");
        // between the nodes, 3 px off the axis: the member
        let mid = (screen_of(&v, Vec3::new(1.0, 0.0, 0.0)) + screen_of(&v, Vec3::new(1.0, 0.0, 0.0))) * 0.5;
        let hit = scene.pick(mid + Vec2::new(0.0, 3.0), &all).expect("hit");
        assert!(!hit.node && hit.element == 20 && (hit.px - 3.0).abs() < 0.01, "{hit:?}");
        assert!((hit.world - Vec3::new(1.0, 0.0, 0.0)).length() < 0.05, "{hit:?}");
        // 7 px off the axis: nothing
        assert!(scene.pick(mid + Vec2::new(0.0, 7.0), &all).is_none());
        // the grid finds the same
        let grid = scene.grid();
        for at in [n1 + Vec2::new(-5.0, 0.0), mid + Vec2::new(0.0, 3.0), mid + Vec2::new(0.0, 7.0), Vec2::new(5.0, 5.0)] {
            assert_eq!(grid.pick(&scene, at, &all), scene.pick(at, &all), "at {at}");
        }
    }

    #[test]
    fn section_clipped_elements_are_not_pickable() {
        let d = display();
        let v = view(&top_camera(10.0), Vec2::new(800.0, 600.0));
        let scene = Scene { display: &d, units: &[], offsets: &[], view: v, lod: Lod::default() };
        // a section box that keeps x < 3
        let keep = |w: Vec3| w.x < 3.0;
        let n1 = screen_of(&v, Vec3::new(4.0, 0.0, 0.0));
        // the cut-away node is not found; the member under the cursor is cut away there too
        assert!(scene.pick(n1, &keep).is_none());
        // the kept part of the member is
        let at = screen_of(&v, Vec3::new(1.0, 0.0, 0.0)) + Vec2::new(0.0, 2.0);
        assert_eq!(scene.pick(at, &keep).map(|h| h.element), Some(20));
        // the node at x = 2 inside the box is
        let at = screen_of(&v, Vec3::new(2.0, 0.0, 0.0)) + Vec2::new(4.0, 4.0);
        assert_eq!(scene.pick(at, &keep).map(|h| (h.element, h.node)), Some((12, true)));
    }

    #[test]
    fn the_exploded_view_moves_what_is_picked() {
        let d = display();
        let v = view(&top_camera(30.0), Vec2::new(800.0, 600.0));
        // unit 1 (the end node) moves 5 m along y
        let units = [0, 1, 0, 0];
        let offsets = [[0.0; 4], [0.0, 5.0, 0.0, 0.0]];
        let scene = Scene { display: &d, units: &units, offsets: &offsets, view: v, lod: Lod::default() };
        let all = |_: Vec3| true;
        let moved = screen_of(&v, Vec3::new(4.0, 5.0, 0.0));
        assert_eq!(scene.pick(moved, &all).map(|h| h.element), Some(11));
        let rest = screen_of(&v, Vec3::new(4.0, 0.0, 0.0));
        assert_eq!(scene.pick(rest, &all).map(|h| h.element), Some(20), "only the member is left there");
    }

    #[test]
    fn nodes_drop_out_when_their_members_are_a_few_pixels_long() {
        let d = display();
        let all = |_: Vec3| true;
        let far = view(&top_camera(5000.0), Vec2::new(800.0, 600.0));
        let scene = Scene { display: &d, units: &[], offsets: &[], view: far, lod: Lod::default() };
        // 4 m members at 5 km: under 6 px; the member is still picked
        let at = screen_of(&far, Vec3::new(4.0, 0.0, 0.0));
        assert_eq!(scene.pick(at, &all).map(|h| h.node), Some(false));
        // the level-of-detail rules
        let g = node([0.0; 3], 4.0);
        let lod = Lod::default();
        // 0.01 m per pixel: 7 px drawn, the member 400 px
        assert!(glyph_drawn(&g, 0.01, &lod));
        assert_eq!(glyph_px(&g, 7.0, 0.01), 7.0);
        // zoomed out to 1 m per pixel: the cap (1.6 m) makes it 1.6 px, under 3 px: dropped
        assert!((glyph_px(&g, 7.0, 1.0) - 1.6).abs() < 1e-6);
        assert!(!glyph_drawn(&g, 1.0, &lod));
        // a node without members has no cap and is never clustered away
        let lone = node([0.0; 3], 0.0);
        assert!(glyph_drawn(&lone, 1000.0, &lod));
        // the cluster rule alone: member 4 m at 0.8 m/px is 5 px (< 6), glyph capped to 2 px
        let wide = GlyphInstance { size: [0.0, 0.0], ..g };
        assert!(!glyph_drawn(&wide, 0.8, &lod));
        assert!(glyph_drawn(&wide, 0.5, &lod));
    }

    #[test]
    fn segments_are_clipped_at_the_near_plane() {
        let (a, b) = (Vec4::new(0.0, 0.0, 0.0, -1.0), Vec4::new(2.0, 4.0, 1.0, 3.0));
        let (ca, cb, ta, tb) = clip_near(a, b, 1.0).unwrap();
        assert_eq!((ta, tb), (0.5, 1.0));
        assert_eq!(ca, Vec4::new(1.0, 2.0, 0.5, 1.0));
        assert_eq!(cb, b);
        // the other way round
        let (ca, cb, ta, tb) = clip_near(b, a, 1.0).unwrap();
        assert_eq!((ta, tb, ca, cb.w), (0.0, 0.5, b, 1.0));
        assert!(clip_near(a, Vec4::new(0.0, 0.0, 0.0, 0.5), 1.0).is_none());
        // a member running from behind the camera to in front of it is still picked
        let mut d = display();
        let mut c = top_camera(10.0);
        c.set_preset(crate::viewport::camera::ViewPreset::Front);
        c.target = Vec3::ZERO;
        let v = view(&c, Vec2::new(800.0, 600.0));
        let eye = c.eye();
        d.segs[0].a = (eye + (eye - c.target).normalize() * 5.0 + Vec3::new(0.0, 0.0, -1.0)).to_array();
        d.segs[0].b = [0.0, 0.0, -1.0];
        d.glyphs.clear();
        d.glyph_item.clear();
        let scene = Scene { display: &d, units: &[], offsets: &[], view: v, lod: Lod::default() };
        let at = screen_of(&v, Vec3::new(0.0, 0.0, -1.0));
        let hit = scene.pick(at, &|_| true).expect("the visible part");
        assert_eq!(hit.element, 20);
        assert_eq!(scene.grid().pick(&scene, at, &|_| true).map(|h| h.element), Some(20));
    }

    #[test]
    fn perspective_correct_parameters() {
        // halfway on screen between w = 1 and w = 3 is a quarter of the way in space
        assert!((unproject_param(0.5, 1.0, 3.0) - 0.25).abs() < 1e-6);
        assert_eq!(unproject_param(0.0, 1.0, 3.0), 0.0);
        assert_eq!(unproject_param(1.0, 1.0, 3.0), 1.0);
        assert!((unproject_param(0.3, 2.0, 2.0) - 0.3).abs() < 1e-6);
        assert_eq!(nearest_on_segment(Vec2::new(1.0, 1.0), Vec2::ZERO, Vec2::new(2.0, 0.0)), (0.5, 1.0));
        assert_eq!(clip_to_rect(Vec2::new(-10.0, 5.0), Vec2::new(10.0, 5.0), Vec2::ZERO, Vec2::splat(8.0)), Some((Vec2::new(0.0, 5.0), Vec2::new(8.0, 5.0))));
        assert!(clip_to_rect(Vec2::new(-10.0, 9.0), Vec2::new(10.0, 9.0), Vec2::ZERO, Vec2::splat(8.0)).is_none());
    }
}
