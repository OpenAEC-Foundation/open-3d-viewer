//! The model's grid lines and levels in the viewport, and picking a grid. The 3D view shows the
//! grids where they are; a floor plan just above its cut, so the floor it looks at does not hide
//! them; a section or elevation the grids it crosses, upright, and the levels across it, just in
//! front of its cut. A click on a grid's line or on one of its bubbles picks it, unless something
//! lies in front of the line there (the bubbles are drawn on top, so they are always seen). The
//! picked grid is drawn in the accent colour and the properties pane shows it; a click on
//! anything else, Escape, or switching the grid lines off lets it go.

use super::{prop_row, App, Message};
use crate::annotations::{Datum, DatumKind};
use crate::document::SectionView;
use crate::model::GridLine;
use crate::theme::Palette;
use glam::Vec3;
use iced::widget::column;
use iced::{Element, Point};
use openaec_ui::Language;

/// How far in front of a view's cut its grid lines and levels are drawn (metres): over the cut
/// faces, never under them.
pub(super) const IN_FRONT: f32 = 0.05;

/// The distances between the grid lines that run the same way: a chain past their first ends,
/// from each line to the next, and beyond it the distance from the first to the last. In
/// millimetres, as a drawing has them.
pub fn grid_dimensions(grids: &[GridLine]) -> Vec<crate::annotations::Measurement> {
    use glam::Vec2;
    // the lines by the way they run, either way round
    let way = |g: &GridLine| {
        let d = Vec2::new(g.b[0] - g.a[0], g.b[1] - g.a[1]).normalize_or_zero();
        if d.x < -1e-6 || (d.x.abs() <= 1e-6 && d.y < 0.0) { -d } else { d }
    };
    let mut groups: Vec<(Vec2, Vec<usize>)> = Vec::new();
    for (i, g) in grids.iter().enumerate() {
        let d = way(g);
        if d == Vec2::ZERO {
            continue;
        }
        match groups.iter_mut().find(|(other, _)| other.dot(d).abs() > 0.9998) {
            Some(group) => group.1.push(i),
            None => groups.push((d, vec![i])),
        }
    }
    let mut out = Vec::new();
    for (dir, mut lines) in groups.into_iter().filter(|(_, lines)| lines.len() > 1) {
        let n = Vec2::new(-dir.y, dir.x);
        let across = |i: usize| Vec2::new(grids[i].a[0], grids[i].a[1]).dot(n);
        lines.sort_by(|&i, &j| across(i).total_cmp(&across(j)));
        let z = lines.iter().map(|&i| grids[i].a[2].min(grids[i].b[2])).fold(f32::MAX, f32::min);
        // past the ends the lines begin at
        let begin = lines.iter().flat_map(|&i| [grids[i].a, grids[i].b]).map(|p| Vec2::new(p[0], p[1]).dot(dir)).fold(f32::MAX, f32::min);
        let at = |along: f32, i: usize| (dir * along + n * across(i)).extend(z);
        let label = |d: f32| format!("{:.0}", d * 1000.0);
        for pair in lines.windows(2) {
            let d = across(pair[1]) - across(pair[0]);
            if d > 0.001 {
                out.push(crate::annotations::Measurement { points: vec![at(begin - 1.5, pair[0]), at(begin - 1.5, pair[1])], label: label(d) });
            }
        }
        let (first, last) = (lines[0], lines[lines.len() - 1]);
        if lines.len() > 2 {
            out.push(crate::annotations::Measurement { points: vec![at(begin - 2.7, first), at(begin - 2.7, last)], label: label(across(last) - across(first)) });
        }
    }
    out
}

impl App {
    /// Whether the grid lines are drawn (not in the flipped view, whose up is another axis).
    pub(super) fn grids_shown(&self) -> bool {
        self.show_grids && !self.flip_up
    }

    /// The grid lines and levels the viewport draws, where the view shown draws them (nothing
    /// while the grid lines are off).
    pub(super) fn datums(&self) -> Vec<Datum> {
        let Some(model) = self.model.as_ref().filter(|_| self.grids_shown() || self.levels_3d) else { return Vec::new() };
        // the 3D view can draw the levels without the grids
        let none: &[GridLine] = &[];
        let grids = if self.grids_shown() { model.project.grids.as_slice() } else { none }.iter().enumerate();
        // a 3D view of the file draws no grids
        if self.active_plan.is_none() && self.active_view.is_some_and(|i| model.views.get(i).is_some_and(|v| v.kind == crate::model::ViewKind::ThreeD)) {
            return Vec::new();
        }
        let as_is = |(i, g): (usize, &GridLine)| Datum { line: g.clone(), kind: DatumKind::Grid(i) };
        let Some(view) = self.shown_view() else {
            let mut out: Vec<Datum> = grids.map(as_is).collect();
            out.extend(self.level_outlines());
            return out;
        };
        if !self.grids_shown() {
            return Vec::new();
        }
        if let Some(plan) = self.document.plan(view) {
            // in front of the cut as the plan looks: above it, or below it in a ceiling plan
            let Some(cut) = self.document.cut_height(plan) else { return Vec::new() };
            let z = if plan.kind.seen_from_below() {
                cut as f32 - IN_FRONT
            } else if plan.kind.looks_up() {
                // seen from above while it looks up: over the top of what it shows
                (cut + plan.depth) as f32 + IN_FRONT
            } else {
                cut as f32 + IN_FRONT
            };
            return grids.map(as_is).map(|mut d| {
                (d.line.a[2], d.line.b[2]) = (z, z);
                d
            }).collect();
        }
        self.document.section(view).map(|s| self.section_datums(s, &model.project.grids)).unwrap_or_default()
    }

    /// The levels in the 3D view, when they are asked for: each a rectangle a metre around the
    /// grids (or the model) at its height, its front and back with a level mark at both ends (so each corner
    /// has one), its sides plain.
    fn level_outlines(&self) -> Vec<Datum> {
        let Some(world) = self.world_bounds().filter(|_| self.levels_3d) else { return Vec::new() };
        // around the grids where the model has them (a model can hold its whole neighbourhood),
        // else around the model
        let ends: Vec<[f32; 3]> = self.model.as_ref().map(|m| m.project.grids.iter().flat_map(|g| [g.a, g.b]).collect()).unwrap_or_default();
        let (lo, hi) = if ends.len() >= 4 {
            ends.iter().fold(([f32::MAX; 2], [f32::MIN; 2]), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]))
        } else {
            ([world.min[0], world.min[1]], [world.max[0], world.max[1]])
        };
        let (x0, y0, x1, y1) = (lo[0] - 1.0, lo[1] - 1.0, hi[0] + 1.0, hi[1] + 1.0);
        let mut out = Vec::new();
        for level in self.document.levels() {
            let z = level.elevation as f32;
            let line = |a: [f32; 2], b: [f32; 2], kind| Datum { line: GridLine { name: level.name.clone(), a: [a[0], a[1], z], b: [b[0], b[1], z] }, kind };
            out.push(line([x0, y0], [x1, y0], DatumKind::Level(level.elevation)));
            out.push(line([x0, y1], [x1, y1], DatumKind::Level(level.elevation)));
            out.push(line([x0, y0], [x0, y1], DatumKind::Outline));
            out.push(line([x1, y0], [x1, y1], DatumKind::Outline));
        }
        out
    }

    /// The distances between the grid lines, for the 3D view, when they are asked for.
    pub(super) fn grid_dimensions(&self) -> Vec<crate::annotations::Measurement> {
        match self.model.as_ref().filter(|_| self.grid_dims && self.grids_shown() && self.shown_view().is_none()) {
            Some(model) => grid_dimensions(&model.project.grids),
            None => Vec::new(),
        }
    }

    /// What the heads seen depend on: the camera, the model and what of it is shown. None where
    /// no heads are left out (the option off, no grids, a view's tab, the exploded view).
    fn heads_key(&self) -> Option<(crate::viewport::camera::Camera, u64, (f32, f32), usize, usize)> {
        let model = self.model.as_ref().filter(|m| self.grid_heads_hidden && (self.grids_shown() || self.levels_3d) && self.shown_view().is_none() && !self.explode_on && (!m.project.grids.is_empty() || self.levels_3d))?;
        // what is drawn is part of it: the dimensions and the levels switch on and off
        let drawn = self.visible.iter().filter(|v| **v).count() * 4 + usize::from(self.grid_dims) * 2 + usize::from(self.levels_3d);
        Some((self.camera, model.id, self.viewport_size, std::sync::Arc::as_ptr(&self.hidden) as usize, drawn))
    }

    /// Whether the heads seen are to be worked out again: the view changed since.
    pub(super) fn heads_stale(&self) -> bool {
        self.heads_key() != self.heads_seen.0
    }

    /// Works out which heads of the grid lines are seen in the 3D view: one whose end of the
    /// line lies behind the model, as the camera looks, is not. Only when the view changed.
    pub(super) fn update_heads_seen(&mut self) {
        let Some(key) = self.heads_key() else {
            self.heads_seen = (None, Vec::new());
            self.marks_seen = (Vec::new(), Vec::new());
            return;
        };
        let Some(model) = self.model.clone().filter(|_| self.heads_seen.0 != Some(key)) else { return };
        let (width, height) = (self.viewport_size.0.max(1.0), self.viewport_size.1.max(1.0));
        let view_proj = self.camera.view_proj(width / height);
        let mm = self.model_matrix();
        let eye = self.camera.eye();
        let forward = (self.camera.target - eye).normalize_or_zero();
        let margin = 0.01 * self.camera.distance;
        // whether a place (world) is seen: nothing of the model lies before it as the camera looks
        let seen_at = |app: &mut App, world: Vec3| -> bool {
            let clip = view_proj * world.extend(1.0);
            if clip.w <= 1e-6 {
                return true;
            }
            let (x, y) = ((clip.x / clip.w + 1.0) * 0.5 * width, (1.0 - clip.y / clip.w) * 0.5 * height);
            if x < 0.0 || y < 0.0 || x > width || y > height {
                return true;
            }
            let (origin, dir, hit) = app.cast(&model, x, y, width, height);
            let Some((l, t, s)) = hit else { return true };
            let Some((point, _)) = super::face_at(&model.layers[l], t, origin - s, dir) else { return true };
            (mm.transform_point3(point + s) - eye).dot(forward) >= (world - eye).dot(forward) - margin
        };
        let grids: Vec<[Vec3; 2]> = if self.grids_shown() { model.project.grids.iter().map(|g| [Vec3::from_array(g.a), Vec3::from_array(g.b)]).collect() } else { Vec::new() };
        let seen: Vec<[bool; 2]> = grids.iter().map(|ends| ends.map(|p| seen_at(self, p))).collect();
        // a dimension by its middle, a level's marks by the ends of its line
        let dims: Vec<Vec3> = self.grid_dimensions().iter().filter_map(|d| Some((*d.points.first()? + *d.points.last()?) / 2.0)).collect();
        let dims_seen = dims.into_iter().map(|p| seen_at(self, p)).collect();
        let levels: Vec<[Vec3; 2]> = self.level_outlines().iter().filter(|d| matches!(d.kind, DatumKind::Level(_))).map(|d| [Vec3::from_array(d.line.a), Vec3::from_array(d.line.b)]).collect();
        let levels_seen = levels.iter().map(|ends| ends.map(|p| seen_at(self, p))).collect();
        self.heads_seen = (Some(key), seen);
        self.marks_seen = (dims_seen, levels_seen);
    }

    /// A section's datum lines, just in front of its cut: the grids it crosses (within their
    /// length and its crop), upright from the bottom to the top of the model and its levels; the
    /// levels across the model's width along it, 1 m past it on both sides (or its crop).
    pub(super) fn section_datums(&self, s: &SectionView, grids: &[GridLine]) -> Vec<Datum> {
        let Some(world) = self.world_bounds() else { return Vec::new() };
        let (along, n) = s.axes();
        let cross = |a: [f64; 2], b: [f64; 2]| a[0] * b[1] - a[1] * b[0];
        let u_of = |x: f64, y: f64| (x - s.from[0]) * along[0] + (y - s.from[1]) * along[1];
        let levels = self.document.levels();
        let (u0, u1, v0, v1) = match s.crop {
            Some((lo, hi)) => (lo[0], hi[0], lo[1], hi[1]),
            None => {
                let us = [world.min[0], world.max[0]].into_iter().flat_map(|x| [world.min[1], world.max[1]].map(|y| u_of(x as f64, y as f64)));
                let (lo, hi) = us.fold((f64::MAX, f64::MIN), |(lo, hi), u| (lo.min(u), hi.max(u)));
                let low = levels.iter().map(|l| l.elevation).fold(world.min[2] as f64, f64::min);
                let high = levels.iter().map(|l| l.elevation).fold(world.max[2] as f64, f64::max);
                (lo - 1.0, hi + 1.0, low, high)
            }
        };
        let back = IN_FRONT as f64;
        let at = |u: f64, z: f64| [(s.from[0] + along[0] * u - n[0] * back) as f32, (s.from[1] + along[1] * u - n[1] * back) as f32, z as f32];
        let mut out = Vec::new();
        for (i, g) in grids.iter().enumerate() {
            // where its line meets the section's: from + along u = a + (b - a) t
            let d = [(g.b[0] - g.a[0]) as f64, (g.b[1] - g.a[1]) as f64];
            let w = [g.a[0] as f64 - s.from[0], g.a[1] as f64 - s.from[1]];
            let det = cross(along, d);
            if det.abs() < 1e-6 * d[0].hypot(d[1]) {
                continue;
            }
            let (u, t) = (cross(w, d) / det, cross(w, along) / det);
            if !(-1e-3..=1.0 + 1e-3).contains(&t) || u < u0 || u > u1 {
                continue;
            }
            out.push(Datum { line: GridLine { name: g.name.clone(), a: at(u, v0), b: at(u, v1) }, kind: DatumKind::Grid(i) });
        }
        for level in levels.iter().filter(|l| (v0 - 1e-6..=v1 + 1e-6).contains(&l.elevation)) {
            out.push(Datum { line: GridLine { name: level.name.clone(), a: at(u0, level.elevation), b: at(u1, level.elevation) }, kind: DatumKind::Level(level.elevation) });
        }
        out
    }

    /// The grid under viewport point (x, y) of a viewport of width x height, unless a triangle
    /// lies clearly in front of its line there (`triangle`: the world point of the triangle the
    /// same click hits); `None` while the grid lines are not drawn.
    pub(crate) fn grid_under(&self, x: f32, y: f32, width: f32, height: f32, triangle: Option<Vec3>) -> Option<usize> {
        let datums = self.datums();
        let view_proj = self.camera.view_proj(width / height);
        crate::annotations::grid_at(&view_proj, (width, height), &datums, Point::new(x, y), self.not_behind(triangle))
    }

    /// Picks grid `g` (`None`: no grid) instead of the elements selected.
    pub(crate) fn pick_grid(&mut self, g: Option<usize>) {
        self.sel.clear();
        self.selection_changed();
        self.set_structural_selection(Vec::new());
        self.picked_grid = g;
    }

    /// The picked grid's properties: its name, length, direction (degrees from x, 0 to 180),
    /// elevation and ends (x, y in metres).
    pub(super) fn grid_properties(&self, p: Palette, l: Language) -> Option<Element<'static, Message>> {
        let grid = self.model.as_ref()?.project.grids.get(self.picked_grid?)?;
        let (a, b) = (Vec3::from_array(grid.a), Vec3::from_array(grid.b));
        let along = b - a;
        let direction = along.y.atan2(along.x).to_degrees().rem_euclid(180.0);
        let end = |q: Vec3| format!("{:.3}; {:.3}", q.x, q.y);
        let row = |k: &str, v: String| prop_row(p, k.to_string(), v);
        Some(
            column![
                row(l.text("Naam", "Name"), grid.name.clone()),
                row(l.text("Lengte", "Length"), format!("{:.3} m", along.length())),
                row(l.text("Richting", "Direction"), format!("{direction:.1}°")),
                row(l.text("Hoogte", "Elevation"), format!("{:+.3} m", a.z)),
                row(l.text("Begin", "Start"), end(a)),
                row(l.text("Eind", "End"), end(b)),
            ]
            .spacing(5)
            .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grids_that_run_the_same_way_are_dimensioned_in_a_chain_and_over_all() {
        let g = |name: &str, a: [f32; 3], b: [f32; 3]| GridLine { name: name.into(), a, b };
        // three lines along y at x = 0, 5.4 and 9, and two along x
        let grids = [g("1", [0.0, 0.0, 0.0], [0.0, 10.0, 0.0]), g("3", [9.0, 10.0, 0.0], [9.0, 0.0, 0.0]), g("2", [5.4, 0.0, 0.0], [5.4, 10.0, 0.0]), g("A", [-1.0, 2.0, 0.0], [10.0, 2.0, 0.0]), g("B", [-1.0, 8.0, 0.0], [10.0, 8.0, 0.0])];
        let dims = grid_dimensions(&grids);
        let labels: Vec<&str> = dims.iter().map(|d| d.label.as_str()).collect();
        assert_eq!(labels, ["3600", "5400", "9000", "6000"]);
        // the chain lies past the ends the lines begin at, the whole further out
        assert!(dims[0].points.iter().all(|p| (p.y + 1.5).abs() < 1e-4) && dims[2].points.iter().all(|p| (p.y + 2.7).abs() < 1e-4), "{dims:?}");
        assert!(dims[3].points.iter().all(|p| (p.x + 2.5).abs() < 1e-4), "{:?}", dims[3]);
        assert!(grid_dimensions(&grids[..1]).is_empty());
    }
    use crate::model::{GridLine, Layer, Model};
    use crate::viewport::camera::ViewPreset;
    use crate::viewport::Navigate;
    use crate::app::Message;
    use glam::Vec3;
    use std::path::PathBuf;
    use std::sync::Arc;

    /// A floor of 24 x 24 m at z = 0 (element 10), a slab over it at z = 3 from x = 4 to 13 along
    /// grid A (element 20), and grids A (along x) and 1 (along y) on the floor, seen from above.
    fn app() -> App {
        let square = |x0: f32, y0: f32, x1: f32, y1: f32, z: f32| [x0, y0, z, x1, y0, z, x1, y1, z, x0, y1, z];
        let mut positions = square(-12.0, -12.0, 12.0, 12.0, 0.0).to_vec();
        positions.extend(square(4.0, -1.0, 13.0, 1.0, 3.0));
        let layer = Layer {
            name: "Floors".into(),
            positions,
            indices: vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7],
            elements: vec![(10, 0, 6), (20, 6, 6)],
            ..Layer::default()
        };
        let mut model = Model::new(PathBuf::from("grids.rvt"), "Revit", vec![layer]);
        model.project.grids = vec![
            GridLine { name: "A".into(), a: [-10.0, 0.0, 0.0], b: [10.0, 0.0, 0.0] },
            GridLine { name: "1".into(), a: [0.0, -10.0, 0.0], b: [0.0, 10.0, 0.0] },
        ];
        let mut app = App::default();
        app.set_model(Arc::new(model));
        app.camera.set_preset(ViewPreset::Top);
        app.fit();
        if let Some((_, to, _)) = app.anim.take() {
            app.camera = to;
        }
        app
    }

    /// A click `dy` pixels below where world point `p` is on screen.
    fn click(app: &mut App, p: Vec3, dy: f32) {
        let (w, h) = app.viewport_size;
        let n = app.camera.view_proj(w / h).project_point3(p);
        click_px(app, (n.x + 1.0) * 0.5 * w, (1.0 - n.y) * 0.5 * h + dy);
    }

    fn click_px(app: &mut App, x: f32, y: f32) {
        let (width, height) = app.viewport_size;
        let _ = app.update(Message::Navigate(Navigate::Pick { x, y, width, height, add: false }));
    }

    /// Where grid `g`'s second bubble is on screen.
    fn bubble(app: &App, g: usize) -> iced::Point {
        let (w, h) = app.viewport_size;
        let grid = &app.model.as_ref().unwrap().project.grids[g];
        crate::annotations::bubble_centres(&app.camera.view_proj(w / h), (w, h), grid).expect("on screen")[1]
    }

    fn shows(app: &App, wording: &str) -> bool {
        let mut ui = crate::app::test_ui(app, iced::Size::new(1400.0, 900.0));
        ui.find(wording).is_ok()
    }

    #[test]
    fn a_click_on_a_grid_picks_it_and_shows_its_properties() {
        let mut app = app();
        // beside the line, over the floor in the same plane
        click(&mut app, Vec3::new(-5.0, 0.0, 0.0), 3.0);
        assert_eq!(app.picked_grid, Some(0));
        assert!(!app.has_selection());
        for shown in ["GRID", "A", "20.000 m", "0.0°", "+0.000 m"] {
            assert!(shows(&app, shown), "{shown}");
        }
        // the slab in front of the line takes the click there
        click(&mut app, Vec3::new(5.0, 0.0, 0.0), 0.0);
        assert_eq!(app.picked_grid, None);
        assert_eq!(app.sel.primary(), Some(crate::app::Pickable::Element(20)));
        assert!(!shows(&app, "GRID"));
        // a bubble is always seen, also over the slab
        let end = bubble(&app, 0);
        click_px(&mut app, end.x, end.y + 4.0);
        assert_eq!(app.picked_grid, Some(0));
        assert!(!app.has_selection());
        click(&mut app, Vec3::new(0.0, 6.0, 0.0), 0.0);
        assert_eq!(app.picked_grid, Some(1));
        assert!(shows(&app, "90.0°"));
        let _ = app.update(Message::Escape);
        assert_eq!(app.picked_grid, None);
    }

    #[test]
    fn switching_the_grid_lines_off_lets_the_picked_grid_go_and_clicks_pass_through() {
        let mut app = app();
        click(&mut app, Vec3::new(-5.0, 0.0, 0.0), 0.0);
        assert_eq!(app.picked_grid, Some(0));
        let _ = app.update(Message::ToggleGrids);
        assert_eq!(app.picked_grid, None);
        click(&mut app, Vec3::new(-5.0, 0.0, 0.0), 0.0);
        assert_eq!(app.picked_grid, None);
        assert_eq!(app.sel.primary(), Some(crate::app::Pickable::Element(10)));
    }

    /// A room of 8 x 5 m with walls 3 m high and a slab over it at 3, levels 00 at 0 and 01 at 3,
    /// grids 1 (x = 0) and 2 (x = 8) along y and A (y = 0) along x, on the ground.
    fn building() -> App {
        let mut positions = vec![0.,0.,0., 8.,0.,0., 8.,5.,0., 0.,5.,0., 0.,0.,3., 8.,0.,3., 8.,5.,3., 0.,5.,3.];
        let mut indices = vec![0,1,5, 0,5,4, 1,2,6, 1,6,5, 2,3,7, 2,7,6, 3,0,4, 3,4,7];
        positions.extend([-1.,-1.,3., 9.,-1.,3., 9.,6.,3., -1.,6.,3.]);
        indices.extend([8, 9, 10, 8, 10, 11]);
        let layer = Layer { name: "Walls".into(), positions, indices, elements: vec![(10, 0, 24), (20, 24, 6)], ..Layer::default() };
        let mut model = Model::new(PathBuf::from("room.rvt"), "Revit", vec![layer]);
        model.project.levels = vec![("00".into(), 0.0), ("01".into(), 3.0)];
        let grid = |name: &str, a: [f32; 2], b: [f32; 2]| GridLine { name: name.into(), a: [a[0], a[1], 0.0], b: [b[0], b[1], 0.0] };
        model.project.grids = vec![grid("1", [0.0, -1.0], [0.0, 6.0]), grid("2", [8.0, -1.0], [8.0, 6.0]), grid("A", [-1.0, 0.0], [9.0, 0.0])];
        let mut app = App::default();
        app.set_model(Arc::new(model));
        app
    }

    fn settle(app: &mut App) {
        if let Some((_, to, _)) = app.anim.take() {
            app.camera = to;
        }
    }

    fn close(p: [f32; 3], q: [f32; 3]) -> bool {
        p.iter().zip(q).all(|(a, b)| (a - b).abs() < 1e-3)
    }

    /// A floor plan shows the grids just above its cut, so the floor it looks at does not hide
    /// them, and a click on one picks it there.
    #[test]
    fn a_floor_plan_shows_the_grids_at_its_cut_over_what_is_below_it() {
        let mut app = building();
        let level = app.document.levels()[1].path.clone();
        let _ = app.update(Message::NewPlan(Arc::from(level.as_str())));
        settle(&mut app);
        let z = 3.0 + 1.2 + IN_FRONT;
        let datums = app.datums();
        let model = app.model.clone().unwrap();
        assert_eq!(datums.len(), 3);
        for (i, (d, g)) in datums.iter().zip(&model.project.grids).enumerate() {
            assert_eq!(d.kind, DatumKind::Grid(i));
            assert_eq!(d.line.name, g.name);
            assert!(close(d.line.a, [g.a[0], g.a[1], z]) && close(d.line.b, [g.b[0], g.b[1], z]), "{d:?}");
        }
        // the slab at 3 lies over the model's grids, not over the plan's
        click(&mut app, Vec3::new(0.0, 2.5, z), 2.0);
        assert_eq!(app.picked_grid, Some(0));
        // the 3D view shows them where they are
        let _ = app.update(Message::SelectTab(0));
        let datums = app.datums();
        assert!(close(datums[0].line.a, model.project.grids[0].a), "{datums:?}");
    }

    /// A section shows the grids it crosses upright, from the bottom to the top of the model, and
    /// the levels across it, just in front of its cut; a click on a grid there picks it.
    #[test]
    fn a_section_shows_the_grids_it_crosses_upright_and_the_levels_across_it() {
        let mut app = building();
        let section = app.document.add_section("Section A", [-1.0, 2.5], [9.0, 2.5], 20.0).unwrap();
        app.open_plan(&section);
        settle(&mut app);
        let datums = app.datums();
        // it looks to the left of its line (+y): in front of the cut is towards -y
        let y = 2.5 - IN_FRONT;
        let grids: Vec<_> = datums.iter().filter(|d| matches!(d.kind, DatumKind::Grid(_))).collect();
        assert_eq!(grids.iter().map(|d| d.line.name.as_str()).collect::<Vec<_>>(), ["1", "2"], "A runs along it");
        assert_eq!(grids[1].kind, DatumKind::Grid(1));
        assert!(close(grids[0].line.a, [0.0, y, 0.0]) && close(grids[0].line.b, [0.0, y, 3.0]), "{:?}", grids[0]);
        assert!(close(grids[1].line.a, [8.0, y, 0.0]) && close(grids[1].line.b, [8.0, y, 3.0]), "{:?}", grids[1]);
        // the levels across the model's width along the line, 1 m past it on both sides
        let levels: Vec<_> = datums.iter().filter(|d| matches!(d.kind, DatumKind::Level(_))).collect();
        assert_eq!(levels.iter().map(|d| d.line.name.as_str()).collect::<Vec<_>>(), ["00", "01"]);
        assert_eq!(levels[1].kind, DatumKind::Level(3.0));
        assert!(close(levels[1].line.a, [-2.0, y, 3.0]) && close(levels[1].line.b, [10.0, y, 3.0]), "{:?}", levels[1]);
        click(&mut app, Vec3::new(8.0, y, 1.5), 2.0);
        assert_eq!(app.picked_grid, Some(1));
    }
}
