//! Move, Copy, Rotate and Mirror: the selected modelled elements follow a few clicks on the work
//! plane, as Revit's Modify tools do. Move and Copy take a start and an end point; Rotate a
//! centre, a point to turn from and a point to turn to; Mirror two points of its axis, and leaves
//! a mirrored copy. The clicks snap as a tool's do, the joins follow, and the whole is one step
//! to take back. Joins themselves are not transformed: they are found again where members meet.

use super::App;
use crate::selection::Pickable;
use bim_core::element::{Opening, PlanarForm};
use bim_core::{Element, ElementId, Shape};
use glam::{DVec2, DVec3};
use openaec_ui::Language;

/// What the clicks do with the selected elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformKind {
    Move,
    Copy,
    Rotate,
    Mirror,
}

impl TransformKind {
    pub const ALL: [TransformKind; 4] = [TransformKind::Move, TransformKind::Copy, TransformKind::Rotate, TransformKind::Mirror];

    /// Its command id.
    pub fn id(self) -> &'static str {
        match self {
            TransformKind::Move => "model.move",
            TransformKind::Copy => "model.copy",
            TransformKind::Rotate => "model.rotate",
            TransformKind::Mirror => "model.mirror",
        }
    }

    pub fn label(self, l: Language) -> &'static str {
        match self {
            TransformKind::Move => l.text("Verplaatsen", "Move"),
            TransformKind::Copy => l.text("Kopiëren", "Copy"),
            TransformKind::Rotate => l.text("Roteren", "Rotate"),
            TransformKind::Mirror => l.text("Spiegelen", "Mirror"),
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            TransformKind::Move => "draw.coordinate",
            TransformKind::Copy => "object.add",
            TransformKind::Rotate => "view.orbit",
            TransformKind::Mirror => "transform.mirror",
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
    pub points: Vec<DVec3>,
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
            PlanarForm::Slab { outline, holes, top } => {
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
        if elements.is_empty() {
            return;
        }
        if self.modeller.tool.is_some() {
            self.set_model_tool(None);
        }
        self.modeller.edit = None;
        self.modeller.refusal = None;
        self.modeller.transform = Some(Transform { kind, elements, points: Vec::new() });
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
        let Some(t) = self.modeller.transform.as_mut() else { return };
        t.points.push(p);
        if t.points.len() < t.kind.clicks() {
            return;
        }
        let Some(done) = self.modeller.transform.take() else { return };
        match Map::of(done.kind, &done.points) {
            Some(map) => {
                self.transform_elements(&done.elements, &map, done.kind.copies());
            }
            // points that say nothing: clicked again
            None => self.modeller.transform = Some(Transform { points: Vec::new(), ..done }),
        }
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
            let mut changed = Element::in_project(&grid, e.name.clone(), e.level.clone(), shape);
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
        let walls: Vec<&Element> = app.modeller.model.iter().filter(|e| e.class() == "IfcWall").collect();
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
            p.openings.push(Opening { along: 0.5, sill: 0.0, width: 0.9, height: 2.1 });
        }
        e = Element { id, ..Element::in_project(&grid, e.name.clone(), e.level.clone(), shape) };
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
        let slab = Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, holes: Vec::new(), top: 0.0 }, thickness: 0.2, openings: Vec::new(), build_up: None });
        let map = Map::Flip { on: DVec2::new(5.0, 0.0), along: DVec2::Y };
        let Some(Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, .. }, .. })) = transformed(&slab, &map) else { panic!() };
        let area: f64 = (0..outline.len()).map(|k| outline[k].perp_dot(outline[(k + 1) % outline.len()])).sum::<f64>() / 2.0;
        assert!((area - 12.0).abs() < 1e-9, "still anticlockwise: {area}");
        assert!(outline.iter().all(|p| p.x >= 6.0 - 1e-9 && p.x <= 10.0 + 1e-9));
    }
}
