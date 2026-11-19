//! Move, Copy, Rotate and Mirror: the selected modelled elements follow a few clicks on the work
//! plane, as Revit's Modify tools do. Move and Copy take a start and an end point; Rotate a
//! centre, a point to turn from and a point to turn to; Mirror two points of its axis, and leaves
//! a mirrored copy. The clicks snap as a tool's do, the joins follow, and the whole is one step
//! to take back. Joins themselves are not transformed: they are found again where members meet.

use super::App;
use crate::selection::Pickable;
use bim_core::element::{Opening, PlanarForm};
use bim_core::{ElementId, Shape};
use glam::{DVec2, DVec3};
use openaec_ui::Language;

/// What the clicks do with the selected elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformKind {
    Move,
    Copy,
    Rotate,
    Mirror,
    /// The line clicked first is where the point clicked next comes to lie: the elements move
    /// square to that line, as Revit's Align does.
    Align,
}

impl TransformKind {
    pub const ALL: [TransformKind; 5] = [TransformKind::Move, TransformKind::Copy, TransformKind::Rotate, TransformKind::Mirror, TransformKind::Align];

    /// Its command id.
    pub fn id(self) -> &'static str {
        match self {
            TransformKind::Move => "model.move",
            TransformKind::Copy => "model.copy",
            TransformKind::Rotate => "model.rotate",
            TransformKind::Mirror => "model.mirror",
            TransformKind::Align => "model.align",
        }
    }

    pub fn label(self, l: Language) -> &'static str {
        match self {
            TransformKind::Move => l.text("Verplaatsen", "Move"),
            TransformKind::Copy => l.text("Kopiëren", "Copy"),
            TransformKind::Rotate => l.text("Roteren", "Rotate"),
            TransformKind::Mirror => l.text("Spiegelen", "Mirror"),
            TransformKind::Align => l.text("Uitlijnen", "Align"),
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            TransformKind::Move => "draw.coordinate",
            TransformKind::Copy => "object.add",
            TransformKind::Rotate => "view.orbit",
            TransformKind::Mirror => "transform.mirror",
            TransformKind::Align => "transform.flip-vertical",
        }
    }

    /// The clicks it takes.
    fn clicks(self) -> usize {
        if self == TransformKind::Rotate { 3 } else { 2 }
    }

    /// Whether it leaves what was selected where it is and makes new elements.
    fn copies(self) -> bool {
        matches!(self, TransformKind::Copy | TransformKind::Mirror)
    }

    /// What to click next, `placed` points in.
    pub fn hint(self, l: Language, placed: usize) -> &'static str {
        match (self, placed) {
            (TransformKind::Rotate, 0) => l.text("Klik het draaipunt · Esc: stoppen", "Click the centre to turn about · Esc: stop"),
            (TransformKind::Rotate, 1) => l.text("Klik een punt om vanaf te draaien · Esc: opnieuw", "Click a point to turn from · Esc: start again"),
            (TransformKind::Rotate, _) => l.text("Klik waar dat punt heen draait · Esc: opnieuw", "Click where that point turns to · Esc: start again"),
            (TransformKind::Mirror, 0) => l.text("Klik een punt van de spiegelas · Esc: stoppen", "Click a point of the mirror axis · Esc: stop"),
            (TransformKind::Mirror, _) => l.text("Klik een tweede punt van de spiegelas · Esc: opnieuw", "Click a second point of the mirror axis · Esc: start again"),
            (TransformKind::Align, 0) => l.text("Klik de lijn om op uit te lijnen (stramien, wand, rand) · Esc: stoppen", "Click the line to align to (grid, wall, edge) · Esc: stop"),
            (TransformKind::Align, _) => l.text("Klik het punt dat op die lijn komt · Esc: opnieuw", "Click the point that comes onto that line · Esc: start again"),
            (_, 0) => l.text("Klik het beginpunt · Esc: stoppen", "Click the start point · Esc: stop"),
            (_, _) => l.text("Klik het eindpunt · Esc: opnieuw", "Click the end point · Esc: start again"),
        }
    }
}

/// A move, copy, turn or mirror under way: the elements it takes and the points clicked so far.
#[derive(Debug, Clone, PartialEq)]
pub struct Transform {
    pub kind: TransformKind,
    pub elements: Vec<ElementId>,
    /// The picked grid line it takes instead (a move or a copy with no element selected).
    pub grid: Option<usize>,
    /// The line to align to: a point on it and the way it runs.
    pub line: Option<(DVec2, DVec2)>,
    pub points: Vec<DVec3>,
    /// What is drawn on a floor plan that it takes in place of elements: the plan, and the run of
    /// its lines that is the line or shape picked.
    pub drawn: Option<(std::sync::Arc<str>, std::ops::Range<usize>)>,
}

/// How points go from where they are to where they come: in plan, heights as they are but for a
/// shift.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Map {
    Shift(DVec3),
    /// Turned `angle` radians anticlockwise about `about`.
    Turn { about: DVec2, angle: f64 },
    /// Mirrored in the line through `on` along the unit vector `along`.
    Flip { on: DVec2, along: DVec2 },
}

impl Map {
    /// The map the clicked `points` of a `kind` make; none while they do not say one (the two
    /// points of an axis in one place).
    pub fn of(kind: TransformKind, points: &[DVec3]) -> Option<Map> {
        match (kind, points) {
            (TransformKind::Move | TransformKind::Copy, [from, to]) => Some(Map::Shift(*to - *from)),
            (TransformKind::Rotate, [about, from, to]) => {
                let (a, b) = ((*from - *about).truncate(), (*to - *about).truncate());
                (a.length() > 1e-9 && b.length() > 1e-9).then(|| Map::Turn { about: about.truncate(), angle: b.y.atan2(b.x) - a.y.atan2(a.x) })
            }
            (TransformKind::Mirror, [a, b]) => (b.truncate() - a.truncate()).try_normalize().map(|along| Map::Flip { on: a.truncate(), along }),
            _ => None,
        }
    }

    fn plan(&self, p: DVec2) -> DVec2 {
        match *self {
            Map::Shift(d) => p + d.truncate(),
            Map::Turn { about, angle } => about + DVec2::from_angle(angle).rotate(p - about),
            Map::Flip { on, along } => {
                let v = p - on;
                on + along * (2.0 * v.dot(along)) - v
            }
        }
    }

    /// How much higher everything comes.
    fn lift(&self) -> f64 {
        match self {
            Map::Shift(d) => d.z,
            _ => 0.0,
        }
    }

    fn point(&self, p: DVec3) -> DVec3 {
        self.plan(p.truncate()).extend(p.z + self.lift())
    }

    fn flips(&self) -> bool {
        matches!(self, Map::Flip { .. })
    }

    /// A direction in plan (radians from x) after the map.
    fn direction(&self, angle: f64) -> f64 {
        match *self {
            Map::Shift(_) => angle,
            Map::Turn { angle: by, .. } => angle + by,
            Map::Flip { along, .. } => 2.0 * along.y.atan2(along.x) - angle,
        }
    }
}

/// The ends and openings of a wall or a system after `map`. Mirrored, it would stand with its
/// other face outward: its ends change places, so it keeps its sides, and its openings are
/// counted from the other end.
fn run(map: &Map, start: DVec2, end: DVec2, openings: &mut [Opening]) -> (DVec2, DVec2) {
    let (a, b) = (map.plan(start), map.plan(end));
    if !map.flips() {
        return (a, b);
    }
    let length = start.distance(end);
    for o in openings.iter_mut() {
        o.along = length - o.along - o.width;
    }
    (b, a)
}

/// `shape` (project coordinates) after `map`; none for a join, which is not moved but found again.
pub fn transformed(shape: &Shape, map: &Map) -> Option<Shape> {
    let mut shape = shape.clone();
    match &mut shape {
        Shape::Point(p) => {
            p.at = map.point(p.at);
            p.rotation = map.direction(p.rotation);
        }
        Shape::Linear(l) => {
            (l.start, l.end) = (map.point(l.start), map.point(l.end));
            if map.flips() {
                l.roll = -l.roll;
            }
        }
        Shape::Planar(p) => match &mut p.form {
            PlanarForm::Wall { start, end, base, .. } => {
                (*start, *end) = run(map, *start, *end, &mut p.openings);
                *base += map.lift();
            }
            PlanarForm::Slab { outline, holes, top, bulges, .. } => {
                // mirrored, the outline runs the other way round: each edge keeps its bow, which
                // is then the bulge of the edge that takes its place
                if map.flips() && !bulges.is_empty() {
                    let n = outline.len();
                    bulges.resize(n, 0.0);
                    *bulges = (0..n).map(|i| bulges[(2 * n - 2 - i) % n]).collect();
                }
                for ring in std::iter::once(&mut *outline).chain(holes.iter_mut()) {
                    for q in ring.iter_mut() {
                        *q = map.plan(*q);
                    }
                    // mirrored, a ring runs the other way round: turned back
                    if map.flips() {
                        ring.reverse();
                    }
                }
                *top += map.lift();
            }
        },
        // a lying system keeps its line as it is turned and mirrored: its width and its
        // openings are measured to its left, so mirrored it is laid from its far side
        Shape::System(s) if s.lies() => {
            let (a, b) = (map.plan(s.start), map.plan(s.end));
            if map.flips() {
                if s.outline.is_empty() {
                    if let Some(dir) = (b - a).try_normalize() {
                        // the rectangle now lies to the right of its line: its outline says where
                        let across = -dir.perp() * s.height;
                        s.outline = vec![a, b, b + across, a + across];
                    }
                } else {
                    s.outline.iter_mut().for_each(|q| *q = map.plan(*q));
                    s.outline.reverse();
                }
                for o in s.openings.iter_mut() {
                    o.sill = -o.sill - if o.framing.is_some_and(|f| f.round) { o.width } else { o.height };
                }
            } else {
                s.outline.iter_mut().for_each(|q| *q = map.plan(*q));
            }
            (s.start, s.end) = (a, b);
            s.base += map.lift();
        }
        Shape::System(s) => {
            (s.start, s.end) = run(map, s.start, s.end, &mut s.openings);
            s.base += map.lift();
        }
        Shape::Connection(_) => return None,
    }
    Some(shape)
}

impl App {
    /// Starts a move, copy, turn or mirror of the selected modelled elements, or stops the one
    /// under way when it is chosen again. Nothing selected: nothing to do.
    pub(super) fn begin_transform(&mut self, kind: TransformKind) {
        if self.modeller.transform.as_ref().is_some_and(|t| t.kind == kind) {
            self.modeller.transform = None;
            return;
        }
        let grid = self.modeller.model.grid;
        let elements: Vec<ElementId> = self
            .selected_modelled()
            .into_iter()
            .filter(|id| self.modeller.model.get(*id).is_some_and(|e| !matches!(e.shape_in_project(&grid), Shape::Connection(_))))
            .collect();
        // nothing of the model selected: the line or shape picked on the floor plan shown
        let drawn = if elements.is_empty() { self.drawn_picked() } else { None };
        // no element selected: a move or a copy takes the picked grid line
        let grid = if elements.is_empty() && drawn.is_none() && matches!(kind, TransformKind::Move | TransformKind::Copy) { self.picked_modelled_grid() } else { None };
        // nothing selected: Align takes the element its second click is on
        if elements.is_empty() && drawn.is_none() && grid.is_none() && kind != TransformKind::Align {
            return;
        }
        if self.modeller.tool.is_some() {
            self.set_model_tool(None);
        }
        self.modeller.edit = None;
        self.modeller.refusal = None;
        self.modeller.transform = Some(Transform { kind, elements, grid, line: None, points: Vec::new(), drawn });
        // a fresh 3D cursor is the base point of a move or a copy
        self.transform_from_cursor3d();
    }

    /// The line or shape picked on the floor plan shown, to move, copy, turn or mirror: the plan
    /// and the run of its lines. A section's drawing stands upright, where these clicks on the
    /// work plane say nothing.
    pub(crate) fn drawn_picked(&self) -> Option<(std::sync::Arc<str>, std::ops::Range<usize>)> {
        let view = self.shown_view().filter(|v| self.document.plan(v).is_some())?;
        match self.mark_pick? {
            super::marks::MarkPick::Line(k) if k < self.document.view_lines(view).len() => Some((std::sync::Arc::from(view), self.document.shape_of(view, k).0)),
            _ => None,
        }
    }

    /// Esc: the points clicked go, else the transform itself. False when none was under way.
    pub(super) fn escape_transform(&mut self) -> bool {
        match self.modeller.transform.as_mut() {
            Some(t) if !t.points.is_empty() => t.points.clear(),
            Some(_) => self.modeller.transform = None,
            None => return false,
        }
        self.modeller.refusal = None;
        true
    }

    /// A click while a transform is under way: its next point, snapped as a tool's; with the
    /// last one the elements go where the points say.
    pub(super) fn transform_click(&mut self, x: f32, y: f32, width: f32, height: f32) {
        let (z, ..) = self.work_plane();
        let Some(p) = self.plane_point(x, y, width, height, z) else { return };
        let p = self.snapped(p, x, y, width, height);
        if self.modeller.transform.as_ref().is_some_and(|t| t.kind == TransformKind::Align) {
            return self.align_click(p, x, y, width, height);
        }
        let Some(t) = self.modeller.transform.as_mut() else { return };
        t.points.push(p);
        if t.points.len() < t.kind.clicks() {
            return;
        }
        let Some(done) = self.modeller.transform.take() else { return };
        match Map::of(done.kind, &done.points) {
            // a floor plan's drawing has the model's x and y: its lines go through the same map
            Some(map) if done.drawn.is_some() => {
                let Some((view, run)) = done.drawn else { return };
                let moved = self.document.map_view_lines(&view, run, done.kind.copies(), &|q| map.plan(DVec2::new(q[0], q[1])).to_array());
                self.mark_pick = moved.map(|run| super::marks::MarkPick::Line(run.start));
            }
            Some(Map::Shift(by)) if done.grid.is_some() => self.shift_grid(done.grid, by.truncate(), done.kind.copies()),
            Some(map) => {
                self.transform_elements(&done.elements, &map, done.kind.copies());
            }
            // points that say nothing: clicked again
            None => self.modeller.transform = Some(Transform { points: Vec::new(), ..done }),
        }
    }

    /// A click of Align: first the line to align to (the grid line, wall line or edge under the
    /// cursor; a click on none is not taken), then the point that comes onto it. The selected
    /// elements move square to the line; with none selected, the element the point lies on does.
    fn align_click(&mut self, p: DVec3, x: f32, y: f32, width: f32, height: f32) {
        let Some(t) = self.modeller.transform.as_ref() else { return };
        let Some((on, along)) = t.line else {
            let line = self.line_under(p, x, y, width, height);
            if let Some(t) = self.modeller.transform.as_mut().filter(|_| line.is_some()) {
                t.line = line;
                t.points.push(p);
            }
            return;
        };
        let elements = if t.elements.is_empty() { self.anchor_at(p.truncate().to_array()).map(|a| vec![ElementId(a.element)]).unwrap_or_default() } else { t.elements.clone() };
        // a click on a line that runs as the first does (a wall's face) brings that line onto it
        let p = match self.line_under(p, x, y, width, height) {
            Some((q, way)) if way.perp_dot(along).abs() < 1e-6 => q.extend(p.z),
            _ => p,
        };
        // a click on nothing: clicked again
        if elements.is_empty() {
            return;
        }
        self.modeller.transform = None;
        let across = along.perp();
        let by = across * (on - p.truncate()).dot(across);
        self.transform_elements(&elements, &Map::Shift(by.extend(0.0)), false);
    }

    /// The lines Align knows: those a click snaps to, and both faces of every wall.
    fn align_lines(&self) -> Vec<(DVec2, DVec2)> {
        let grid = self.modeller.model.grid;
        let mut lines = self.snap_lines();
        for e in self.modeller.model.iter() {
            if let Shape::Planar(w) = e.shape_in_project(&grid) {
                if let PlanarForm::Wall { start, end, .. } = w.form {
                    if let Some(along) = (end - start).try_normalize() {
                        let off = along.perp() * (w.total_thickness() / 2.0);
                        lines.extend([(start + off, end + off), (start - off, end - off)]);
                    }
                }
            }
        }
        lines
    }

    /// The line under the cursor: a point on it and the way it runs. `p` is the click on the
    /// work plane (snapped or not: the cursor itself says which line is nearest).
    fn line_under(&self, p: DVec3, x: f32, y: f32, width: f32, height: f32) -> Option<(DVec2, DVec2)> {
        let (z, ..) = self.work_plane();
        let p = self.plane_point(x, y, width, height, z).unwrap_or(p);
        self.align_lines()
            .into_iter()
            .filter_map(|(a, b)| {
                let d = b - a;
                let c = a + d * ((p.truncate() - a).dot(d) / d.length_squared()).clamp(0.0, 1.0);
                let s = self.on_screen(c.extend(p.z), width, height)?;
                Some((a, d.try_normalize()?, (s.0 - x).hypot(s.1 - y)))
            })
            .filter(|(_, _, off)| *off <= super::modeller::SNAP_PIXELS)
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(a, along, _)| (a, along))
    }

    /// Puts `elements` through `map`: each takes its new place, or (`copy`) a new element is
    /// made there under the next number of its name. When the kernel refuses one, none changes
    /// and the banner says why. The joins follow, and what was moved or made is selected.
    pub(super) fn transform_elements(&mut self, elements: &[ElementId], map: &Map, copy: bool) -> Vec<ElementId> {
        let before = self.modeller.model.clone();
        let grid = self.modeller.model.grid;
        let mut done = Vec::new();
        for &id in elements {
            let Some(e) = self.modeller.model.get(id).cloned() else { continue };
            let Some(shape) = transformed(&e.shape_in_project(&grid), map) else { continue };
            let mut changed = e.reshaped(&grid, shape);
            let result = if copy {
                // the next number of its name: Wall 3 copied is Wall 4 when there are three
                let base = e.name.rsplit_once(' ').filter(|(_, n)| n.parse::<u32>().is_ok()).map_or(e.name.as_str(), |(base, _)| base);
                let number = self.modeller.model.iter().filter(|o| o.class() == changed.class()).count() + 1;
                changed.name = format!("{base} {number}");
                self.modeller.model.add(changed)
            } else {
                changed.id = e.id;
                self.modeller.model.replace(changed).map(|()| e.id)
            };
            match result {
                Ok(id) => done.push(id),
                Err(refusal) => {
                    self.modeller.model = before;
                    self.modeller.refusal = Some(refusal);
                    return Vec::new();
                }
            }
        }
        if done.is_empty() {
            return done;
        }
        self.modeller.refusal = None;
        self.joins_follow(&done);
        self.show_modelled();
        self.select_modelled_all(&done);
        done
    }

    /// Selects the modelled elements `ids` as they are shown.
    fn select_modelled_all(&mut self, ids: &[ElementId]) {
        let Some(scene) = self.model.clone() else { return };
        let entry = if self.models.len() == 1 { 0 } else { self.models.iter().position(|e| Some(e.model.id) == self.modeller.shown).unwrap_or(0) as u32 };
        let mut first = true;
        for id in ids {
            if let Some(shown) = scene.element_with_key(crate::model::ElementKey { model: entry, source: id.0 }) {
                self.click(Some(Pickable::Element(shown)), !first);
                first = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::modeller::ModelTool;
    use crate::app::Message;
    use crate::viewport::Navigate;
    use bim_core::PlanarDef;
    use glam::Vec3;

    /// A new model seen from the top, 10 px per metre, the origin in the middle of 1000 × 800,
    /// with a wall from (0, 0) to (4, 0), selected.
    fn a_wall() -> (App, ElementId) {
        let mut app = App::default();
        app.new_model("Level 0");
        app.viewport_size = (1000.0, 800.0);
        app.camera.set_preset(crate::viewport::camera::ViewPreset::Top);
        app.camera.ortho = true;
        app.camera.target = Vec3::ZERO;
        app.camera.distance = 40.0 / (app.camera.fov_y * 0.5).tan();
        let shape = app.shape_for(ModelTool::Wall, DVec3::ZERO, DVec3::new(4.0, 0.0, 0.0));
        let id = app.add_element("Wall", None, shape).unwrap();
        app.keep_model_history();
        app.select_modelled(id);
        (app, id)
    }

    fn click(app: &mut App, x: f64, y: f64) {
        let (sx, sy) = app.on_screen(DVec3::new(x, y, 0.0), 1000.0, 800.0).unwrap();
        let _ = app.update(Message::Navigate(Navigate::Pick { x: sx, y: sy, width: 1000.0, height: 800.0, add: false }));
    }

    fn ends(app: &App, id: ElementId) -> (DVec2, DVec2) {
        match app.modeller.model.get(id).unwrap().shape_in_project(&app.modeller.model.grid) {
            Shape::Planar(PlanarDef { form: PlanarForm::Wall { start, end, .. }, .. }) => (start, end),
            other => panic!("{other:?}"),
        }
    }

    fn near(a: DVec2, b: [f64; 2]) -> bool {
        a.distance(DVec2::from(b)) < 1e-6
    }

    #[test]
    fn a_shape_picked_on_a_floor_plan_is_moved_copied_turned_and_mirrored() {
        use crate::app::marks::{DrawKind, MarkPick};
        use crate::document::ShapeKind;
        let (mut app, _) = a_wall();
        app.click(None, false);
        let level = std::sync::Arc::<str>::from(app.document.levels()[0].path.as_str());
        let _ = app.update(Message::NewPlan(level));
        app.anim = None;
        app.camera.set_preset(crate::viewport::camera::ViewPreset::Top);
        app.camera.ortho = true;
        app.camera.target = Vec3::ZERO;
        app.camera.distance = 40.0 / (app.camera.fov_y * 0.5).tan();
        let plan = std::sync::Arc::<str>::from(app.shown_view().expect("the plan shows"));
        let _ = app.update(Message::AddShape(plan.clone(), DrawKind::Line, vec![[[20.0, 20.0], [21.0, 20.0]]]));
        let _ = app.update(Message::AddShape(plan.clone(), DrawKind::Rectangle, DrawKind::Rectangle.shape_lines(&[[10.0, 10.0], [12.0, 11.0]])));
        let corners = |app: &App, run: std::ops::Range<usize>| app.document.view_lines(&plan)[run].iter().map(|l| l[0]).collect::<Vec<_>>();
        let close = |a: &[[f64; 2]], b: &[[f64; 2]]| a.len() == b.len() && a.iter().zip(b).all(|(p, q)| (p[0] - q[0]).hypot(p[1] - q[1]) < 1e-6);
        // nothing picked: nothing to move
        assert!(app.drawn_picked().is_none());
        let _ = app.update(Message::Transform(TransformKind::Move));
        assert!(app.modeller.transform.is_none());
        // a side of the rectangle picked: Move takes all of it from one click to the next
        let _ = app.update(Message::PickMark(Some(MarkPick::Line(2))));
        assert_eq!(app.drawn_picked(), Some((plan.clone(), 1..5)));
        let _ = app.update(Message::Transform(TransformKind::Move));
        click(&mut app, -10.0, -5.0);
        click(&mut app, -9.0, -3.0);
        assert!(app.modeller.transform.is_none());
        assert!(close(&corners(&app, 1..5), &[[11.0, 12.0], [13.0, 12.0], [13.0, 13.0], [11.0, 13.0]]), "{:?}", corners(&app, 1..5));
        assert_eq!(app.mark_pick, Some(MarkPick::Line(1)), "it stays picked");
        // Copy leaves it and makes another rectangle after the other lines, picked
        let _ = app.update(Message::Transform(TransformKind::Copy));
        click(&mut app, -10.0, -5.0);
        click(&mut app, -15.0, -5.0);
        assert_eq!(app.document.view_lines(&plan).len(), 9);
        assert_eq!(app.document.view_shapes(&plan).iter().map(|s| (s.kind, s.run())).collect::<Vec<_>>(), [(ShapeKind::Rectangle, 1..5), (ShapeKind::Rectangle, 5..9)]);
        assert!(close(&corners(&app, 5..9), &[[6.0, 12.0], [8.0, 12.0], [8.0, 13.0], [6.0, 13.0]]));
        assert_eq!(app.mark_pick, Some(MarkPick::Line(5)));
        // Rotate turns the copy a quarter about a point: from east of it to north of it
        let _ = app.update(Message::Transform(TransformKind::Rotate));
        click(&mut app, -10.0, -10.0);
        click(&mut app, -5.0, -10.0);
        click(&mut app, -10.0, -5.0);
        assert!(close(&corners(&app, 5..9), &[[-32.0, 6.0], [-32.0, 8.0], [-33.0, 8.0], [-33.0, 6.0]]), "{:?}", corners(&app, 5..9));
        // Mirror leaves a mirrored copy of the line alone, which is no shape
        let _ = app.update(Message::PickMark(Some(MarkPick::Line(0))));
        let _ = app.update(Message::Transform(TransformKind::Mirror));
        click(&mut app, -10.0, -10.0);
        click(&mut app, -10.0, -5.0);
        assert_eq!(app.document.view_lines(&plan).len(), 10);
        assert_eq!(app.document.view_shapes(&plan).len(), 2);
        let l = app.document.view_lines(&plan)[9];
        assert!(close(&l, &[[-40.0, 20.0], [-41.0, 20.0]]), "{l:?}");
        // the wall did not move with any of it
        assert!(near(ends(&app, ElementId(1)).0, [0.0, 0.0]));
    }

    #[test]
    fn move_takes_the_selected_wall_from_one_click_to_the_next_and_undo_brings_it_back() {
        let (mut app, id) = a_wall();
        let _ = app.update(Message::Transform(TransformKind::Move));
        assert!(app.modeller.transform.is_some());
        click(&mut app, 10.0, 5.0);
        click(&mut app, 12.0, 8.0);
        let (a, b) = ends(&app, id);
        assert!(near(a, [2.0, 3.0]) && near(b, [6.0, 3.0]), "{a} {b}");
        assert!(app.modeller.transform.is_none(), "done after its two clicks");
        assert_eq!(app.selected_modelled(), [id], "what was moved stays selected");
        assert_eq!(app.modeller.model.len(), 1);
        let _ = app.update(Message::Undo);
        let (a, b) = ends(&app, id);
        assert!(near(a, [0.0, 0.0]) && near(b, [4.0, 0.0]), "one step back: {a} {b}");
    }

    #[test]
    fn copy_leaves_the_wall_and_makes_another_with_the_next_number() {
        let (mut app, id) = a_wall();
        let _ = app.update(Message::Transform(TransformKind::Copy));
        click(&mut app, 10.0, 5.0);
        click(&mut app, 10.0, 8.0);
        let walls: Vec<&bim_core::Element> = app.modeller.model.iter().filter(|e| e.class() == "IfcWall").collect();
        assert_eq!(walls.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["Wall 1", "Wall 2"]);
        let (a, b) = ends(&app, id);
        assert!(near(a, [0.0, 0.0]) && near(b, [4.0, 0.0]), "the first stays");
        let copy = walls[1].id;
        let (a, b) = ends(&app, copy);
        assert!(near(a, [0.0, 3.0]) && near(b, [4.0, 3.0]), "{a} {b}");
        assert_eq!(app.selected_modelled(), [copy], "the copy is selected, to copy on");
    }

    #[test]
    fn rotate_turns_about_the_first_click_by_the_angle_between_the_next_two() {
        let (mut app, id) = a_wall();
        let _ = app.update(Message::Transform(TransformKind::Rotate));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 5.0, 0.0);
        assert!(app.modeller.transform.is_some(), "a turn takes three clicks");
        click(&mut app, 0.0, 5.0);
        let (a, b) = ends(&app, id);
        assert!(near(a, [0.0, 0.0]) && near(b, [0.0, 4.0]), "a quarter turn: {a} {b}");
    }

    #[test]
    fn mirror_makes_a_copy_across_the_axis_that_keeps_its_sides_and_its_openings() {
        let (mut app, id) = a_wall();
        // a door 0.5 from the wall's start
        let grid = app.modeller.model.grid;
        let mut e = app.modeller.model.get(id).cloned().unwrap();
        let mut shape = e.shape_in_project(&grid);
        if let Shape::Planar(p) = &mut shape {
            p.openings.push(Opening { along: 0.5, sill: 0.0, width: 0.9, height: 2.1, framing: None });
        }
        e = e.reshaped(&grid, shape);
        app.modeller.model.replace(e).unwrap();
        app.show_modelled();
        app.select_modelled(id);
        let _ = app.update(Message::Transform(TransformKind::Mirror));
        // the axis: the line x = 5
        click(&mut app, 5.0, -3.0);
        click(&mut app, 5.0, 3.0);
        assert_eq!(app.modeller.model.iter().filter(|e| e.class() == "IfcWall").count(), 2, "the wall and its mirror image");
        let copy = app.selected_modelled()[0];
        assert_ne!(copy, id);
        let (a, b) = ends(&app, copy);
        // its ends changed places, so it still runs the way the first does
        assert!(near(a, [6.0, 0.0]) && near(b, [10.0, 0.0]), "{a} {b}");
        let Shape::Planar(p) = app.modeller.model.get(copy).unwrap().shape_in_project(&grid) else { panic!() };
        // the door, 0.5 from the first's start, is 0.5 from the image's end: at x = 9.5 back to 8.6
        assert!((p.openings[0].along - 2.6).abs() < 1e-9, "{:?}", p.openings);
    }

    #[test]
    fn align_moves_the_wall_square_to_the_line_clicked_first() {
        let (mut app, id) = a_wall();
        // a second wall along y = 3 to align to
        let shape = app.shape_for(ModelTool::Wall, DVec3::new(-2.0, 3.0, 0.0), DVec3::new(8.0, 3.0, 0.0));
        let other = app.add_element("Wall", None, shape).unwrap();
        app.show_modelled();
        app.select_modelled(id);
        let _ = app.update(Message::Transform(TransformKind::Align));
        click(&mut app, 12.0, 12.0);
        assert!(app.modeller.transform.as_ref().is_some_and(|t| t.line.is_none()), "a click on no line is not taken");
        click(&mut app, 7.0, 3.0);
        click(&mut app, 0.0, 0.0);
        let (a, b) = ends(&app, id);
        assert!(near(a, [0.0, 3.0]) && near(b, [4.0, 3.0]), "up to the line, not along it: {a} {b}");
        assert!(app.modeller.transform.is_none());
        // nothing selected: the wall under the second click moves, and its face clicked comes
        // onto the face clicked of the other
        app.sel.clear();
        let half = match app.modeller.model.get(id).unwrap().shape_in_project(&app.modeller.model.grid) {
            Shape::Planar(w) => w.total_thickness() / 2.0,
            other => panic!("{other:?}"),
        };
        let _ = app.update(Message::Transform(TransformKind::Move));
        assert!(app.modeller.transform.is_none());
        app.select_modelled(id);
        let _ = app.update(Message::Transform(TransformKind::Move));
        click(&mut app, 0.0, 3.0);
        click(&mut app, 0.0, 0.0);
        app.sel.clear();
        let _ = app.update(Message::Transform(TransformKind::Align));
        assert!(app.modeller.transform.is_some(), "Align starts with nothing selected");
        // the far face of the other wall, then the near face of this one
        click(&mut app, 7.0, 3.0 + half);
        click(&mut app, 2.0, -half);
        let (a, b) = ends(&app, id);
        assert!(near(a, [0.0, 3.0 + 2.0 * half]) && near(b, [4.0, 3.0 + 2.0 * half]), "face on face: {a} {b}");
        let _ = other;
    }

    #[test]
    fn esc_drops_the_points_then_the_transform_and_nothing_selected_starts_none() {
        let (mut app, id) = a_wall();
        let _ = app.update(Message::Transform(TransformKind::Move));
        click(&mut app, 1.0, 1.0);
        let _ = app.update(Message::Escape);
        assert!(app.modeller.transform.as_ref().is_some_and(|t| t.points.is_empty()));
        let _ = app.update(Message::Escape);
        assert!(app.modeller.transform.is_none());
        assert!(near(ends(&app, id).0, [0.0, 0.0]), "nothing moved");
        app.sel.clear();
        let _ = app.update(Message::Transform(TransformKind::Move));
        assert!(app.modeller.transform.is_none());
    }

    #[test]
    fn a_slab_mirrored_keeps_its_way_round() {
        let outline = vec![DVec2::new(0.0, 0.0), DVec2::new(4.0, 0.0), DVec2::new(4.0, 3.0), DVec2::new(0.0, 3.0)];
        let slab = Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, holes: Vec::new(), top: 0.0, bulges: Vec::new(), role: Default::default(), slope: None }, thickness: 0.2, openings: Vec::new(), build_up: None });
        let map = Map::Flip { on: DVec2::new(5.0, 0.0), along: DVec2::Y };
        let Some(Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, .. }, .. })) = transformed(&slab, &map) else { panic!() };
        let area: f64 = (0..outline.len()).map(|k| outline[k].perp_dot(outline[(k + 1) % outline.len()])).sum::<f64>() / 2.0;
        assert!((area - 12.0).abs() < 1e-9, "still anticlockwise: {area}");
        assert!(outline.iter().all(|p| p.x >= 6.0 - 1e-9 && p.x <= 10.0 + 1e-9));
    }
}
