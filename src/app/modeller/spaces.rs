//! Spaces, derived from the walls as in the reference modeller: every region the walls of a level
//! close in is a space, found again whenever it is asked for, so spaces follow the walls and are
//! never drawn by hand.
//!
//! - **How they are found.** The axes of a level's walls and standing systems are cut where they
//!   cross and where one ends on another (within the other's half thickness and 10 mm); what does
//!   not close (a wall's loose end) is left out; each region the rest closes in is a space.
//! - **Its outline** is the region between the walls' inner faces: each side lies half its wall's
//!   thickness inside the axis. Its **area** is that outline's (the net floor area of the room).
//! - **How they show.** A label in the middle of each space, over the 3D view and the floor
//!   plans: its name ("Space 1", numbered per level by where it lies, south-west first) and its
//!   area in m².
//! - Openings count for nothing: a doorway does not open a room. A room open on one side is none.

use crate::app::App;
use bim_core::element::PlanarForm;
use bim_core::{PlanarDef, Shape};
use glam::{DVec2, Mat4, Vec3};
use iced::widget::canvas::{self, Frame};
use iced::{alignment, mouse, Color, Point, Rectangle};

/// Points this close are one (metres).
const SAME: f64 = 1e-3;
/// A wall's end this far past another's face still ends on it (metres).
const REACH: f64 = 0.01;
/// The labels show over the viewport when the view looks down at least this steeply (radians).
const LOOKING_DOWN: f32 = 1.0;

/// A wall as the spaces see it: its axis in plan and half its thickness.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bound {
    pub a: DVec2,
    pub b: DVec2,
    pub half: f64,
}

/// A space: the region between its walls' inner faces (anticlockwise) and its area.
#[derive(Debug, Clone, PartialEq)]
pub struct Space {
    pub name: String,
    /// The path of its level, when its walls are on one.
    pub level: Option<String>,
    pub outline: Vec<DVec2>,
    pub area: f64,
    /// The height of its floor (scene coordinates).
    pub z: f64,
}

/// A name given to a space: it stands at a point, and the space around that point on its level
/// carries it, however its walls move.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Room {
    pub level: Option<String>,
    pub at: [f64; 2],
    pub name: String,
}

/// Whether `p` lies inside `ring`.
fn inside(ring: &[DVec2], p: DVec2) -> bool {
    let n = ring.len();
    let mut within = false;
    for k in 0..n {
        let (a, b) = (ring[k], ring[(k + 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            within = !within;
        }
    }
    within
}

/// A point surely inside `ring`: the middle of one of its ears (its corners' middle may lie
/// outside an L).
fn within(ring: &[DVec2]) -> DVec2 {
    let flat: Vec<f64> = ring.iter().flat_map(|p| [p.x, p.y]).collect();
    match earcutr::earcut(&flat, &[], 2).unwrap_or_default().chunks_exact(3).next() {
        Some(t) => (ring[t[0]] + ring[t[1]] + ring[t[2]]) / 3.0,
        None => ring.iter().sum::<DVec2>() / ring.len().max(1) as f64,
    }
}

/// The layer the spaces stand on in the model shown: switched off until asked for.
pub const SPACE_LAYER: &str = "IfcSpace";
/// Where the ids of the spaces start in the model shown: far above those of modelled elements.
pub const SPACE_IDS: u64 = 1 << 40;
/// How high a space is taken to be.
pub const SPACE_HEIGHT: f64 = 2.6;

/// The spaces as elements of the model shown: each a body from its floor up, with its name, its
/// level, its area and what follows from it. They are made from the walls again after every
/// change and so are no elements of the modelled project; a file says them as IfcSpace.
pub fn space_layer(spaces: &[Space], levels: &[(String, String)]) -> Option<crate::model::Layer> {
    if spaces.is_empty() {
        return None;
    }
    let mut layer = crate::model::Layer { name: SPACE_LAYER.into(), color: [0.62, 0.78, 0.90], hidden: true, ..Default::default() };
    for (k, s) in spaces.iter().enumerate() {
        let first = layer.indices.len() as u32;
        let n = s.outline.len();
        let flat: Vec<f64> = s.outline.iter().flat_map(|p| [p.x, p.y]).collect();
        let corners = earcutr::earcut(&flat, &[], 2).unwrap_or_default();
        let (low, high) = (s.z, s.z + SPACE_HEIGHT);
        // its floor (seen from below) and its ceiling
        for (z, up) in [(low, false), (high, true)] {
            let base = (layer.positions.len() / 3) as u32;
            for p in &s.outline {
                layer.positions.extend([p.x as f32, p.y as f32, z as f32]);
                layer.normals.extend([0.0, 0.0, if up { 1.0 } else { -1.0 }]);
            }
            for t in corners.chunks_exact(3) {
                let (a, b, c) = (t[0] as u32 + base, t[1] as u32 + base, t[2] as u32 + base);
                // the outline runs anticlockwise; whichever way the ears came, the face looks out
                let turn = (s.outline[t[1]] - s.outline[t[0]]).perp_dot(s.outline[t[2]] - s.outline[t[0]]) > 0.0;
                layer.indices.extend(if turn == up { [a, b, c] } else { [a, c, b] });
            }
        }
        // its sides, each looking out
        for i in 0..n {
            let (a, b) = (s.outline[i], s.outline[(i + 1) % n]);
            let out = (b - a).normalize_or_zero();
            let normal = [out.y as f32, -out.x as f32, 0.0];
            let base = (layer.positions.len() / 3) as u32;
            for (p, z) in [(a, low), (b, low), (b, high), (a, high)] {
                layer.positions.extend([p.x as f32, p.y as f32, z as f32]);
                layer.normals.extend(normal);
            }
            layer.indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        let id = SPACE_IDS + k as u64;
        layer.elements.push((id, first, layer.indices.len() as u32 - first));
        let perimeter: f64 = (0..n).map(|i| (s.outline[(i + 1) % n] - s.outline[i]).length()).sum();
        let mut props = serde_json::Map::new();
        let mut put = |key: &str, value: String| {
            props.insert(key.into(), serde_json::Value::String(value));
        };
        put("Name", s.name.clone());
        put("class", SPACE_LAYER.into());
        if let Some(name) = s.level.as_deref().and_then(|path| levels.iter().find(|(p, _)| p == path)).map(|(_, name)| name) {
            put("level", name.clone());
        }
        put("Area", format!("{:.2} m²", s.area));
        put("Perimeter", format!("{perimeter:.3} m"));
        put("Height", format!("{SPACE_HEIGHT:.3} m"));
        put("Volume", format!("{:.2} m³", s.area * SPACE_HEIGHT));
        // made from the walls: a file says it, and the modelled project makes it again
        put(crate::export::ifcx::DERIVED, "space".into());
        layer.props.push((id, serde_json::Value::Object(props).to_string()));
    }
    Some(layer)
}

/// The area inside a ring, above zero when it runs anticlockwise.
fn signed_area(ring: &[DVec2]) -> f64 {
    let n = ring.len();
    (0..n).map(|k| ring[k].perp_dot(ring[(k + 1) % n])).sum::<f64>() / 2.0
}

/// The regions `bounds` close in: each the corners of a region between the walls' inner faces,
/// anticlockwise.
pub fn regions(bounds: &[Bound]) -> Vec<Vec<DVec2>> {
    // where each axis is cut: its own ends, where another ends on it, and where another crosses it
    let mut ends: Vec<(DVec2, DVec2)> = bounds.iter().map(|w| (w.a, w.b)).collect();
    let mut cuts: Vec<Vec<f64>> = vec![vec![0.0, 1.0]; bounds.len()];
    for i in 0..bounds.len() {
        for j in 0..bounds.len() {
            if i == j {
                continue;
            }
            let (c, d) = (bounds[j].a, bounds[j].b);
            let r = d - c;
            let length = r.length();
            if length < SAME {
                continue;
            }
            // an end of wall i that lies on wall j, between its ends: it ends there
            for end in 0..2 {
                let p = if end == 0 { ends[i].0 } else { ends[i].1 };
                let t = (p - c).dot(r) / (length * length);
                let foot = c + r * t;
                if t * length > SAME && (1.0 - t) * length > SAME && foot.distance(p) <= bounds[j].half + REACH {
                    if end == 0 {
                        ends[i].0 = foot;
                    } else {
                        ends[i].1 = foot;
                    }
                    cuts[j].push(t);
                }
            }
        }
    }
    for i in 0..bounds.len() {
        for j in i + 1..bounds.len() {
            let ((a, b), (c, d)) = (ends[i], ends[j]);
            let (r, s) = (b - a, d - c);
            let denom = r.perp_dot(s);
            if denom.abs() < 1e-9 * r.length() * s.length() {
                continue;
            }
            let (t, u) = ((c - a).perp_dot(s) / denom, (c - a).perp_dot(r) / denom);
            if t > 0.0 && t < 1.0 && u > 0.0 && u < 1.0 {
                cuts[i].push(t);
                cuts[j].push(u);
            }
        }
    }
    // the points, each once, and the stretches between them with their wall's half thickness
    let mut points: Vec<DVec2> = Vec::new();
    let mut node = |p: DVec2| match points.iter().position(|q| q.distance(p) <= SAME * 10.0) {
        Some(k) => k,
        None => {
            points.push(p);
            points.len() - 1
        }
    };
    let mut edges: Vec<(usize, usize, f64)> = Vec::new();
    for (i, cut) in cuts.iter_mut().enumerate() {
        cut.sort_by(f64::total_cmp);
        let (a, b) = ends[i];
        let along: Vec<usize> = cut.iter().map(|t| node(a + (b - a) * *t)).collect();
        for pair in along.windows(2) {
            if pair[0] != pair[1] && !edges.iter().any(|e| (e.0, e.1) == (pair[0], pair[1]) || (e.0, e.1) == (pair[1], pair[0])) {
                edges.push((pair[0], pair[1], bounds[i].half));
            }
        }
    }
    // what ends loose closes nothing in
    loop {
        let degree = |n: usize| edges.iter().filter(|e| e.0 == n || e.1 == n).count();
        let Some(k) = edges.iter().position(|e| degree(e.0) < 2 || degree(e.1) < 2) else { break };
        edges.remove(k);
    }
    // each stretch walked both ways; at its head the walk turns to the next stretch clockwise, so
    // a region is walked with its inside on the left
    let halves: Vec<(usize, usize, f64)> = edges.iter().flat_map(|e| [(e.0, e.1, e.2), (e.1, e.0, e.2)]).collect();
    let angle = |h: &(usize, usize, f64)| {
        let d = points[h.1] - points[h.0];
        d.y.atan2(d.x)
    };
    let next = |h: usize| -> usize {
        let (from, at, _) = halves[h];
        let back = angle(&(at, from, 0.0));
        // the outgoing stretch at the smallest turn clockwise from the way back
        (0..halves.len())
            .filter(|k| halves[*k].0 == at && halves[*k].1 != from)
            .min_by(|x, y| {
                let turn = |k: usize| (back - angle(&halves[k])).rem_euclid(std::f64::consts::TAU);
                turn(*x).total_cmp(&turn(*y))
            })
            .unwrap_or(h ^ 1)
    };
    let mut walked = vec![false; halves.len()];
    let mut out = Vec::new();
    for start in 0..halves.len() {
        if walked[start] {
            continue;
        }
        let (mut ring, mut thick, mut h) = (Vec::new(), Vec::new(), start);
        while !walked[h] {
            walked[h] = true;
            ring.push(points[halves[h].0]);
            thick.push(halves[h].2);
            h = next(h);
        }
        if h != start || ring.len() < 3 || signed_area(&ring) <= SAME {
            continue;
        }
        // each side half its wall's thickness in: the corners where the inner faces meet
        let n = ring.len();
        let side = |k: usize| {
            let (a, b) = (ring[k], ring[(k + 1) % n]);
            let inward = (b - a).normalize_or_zero().perp() * thick[k];
            (a + inward, b + inward)
        };
        let inner: Vec<DVec2> = (0..n)
            .map(|k| {
                let ((a, b), (c, d)) = (side((k + n - 1) % n), side(k));
                crate::app::grips::crossing(a, b, c, d).unwrap_or(c)
            })
            .collect();
        if signed_area(&inner) > SAME {
            out.push(inner);
        }
    }
    out
}

impl App {
    /// The spaces the modelled walls close in, level by level, each level's numbered from the
    /// south-west.
    pub(crate) fn spaces(&self) -> Vec<Space> {
        let grid = self.modeller.model.grid;
        // the walls of each level (those on none together), with the height they stand at
        let mut by_level: Vec<(Option<String>, f64, Vec<Bound>)> = Vec::new();
        for e in self.modeller.model.iter() {
            let (a, b, base, half) = match e.shape_in_project(&grid) {
                Shape::Planar(p @ PlanarDef { form: PlanarForm::Wall { start, end, base, .. }, .. }) => (start, end, base, p.total_thickness() / 2.0),
                Shape::System(s) if !s.lies() => (s.start, s.end, s.base, s.member.max_element() / 2.0),
                _ => continue,
            };
            let bound = Bound { a, b, half };
            match by_level.iter_mut().find(|(level, ..)| *level == e.level) {
                Some((_, _, walls)) => walls.push(bound),
                None => by_level.push((e.level.clone(), base, vec![bound])),
            }
        }
        let mut out = Vec::new();
        for (level, z, walls) in by_level {
            let mut found = regions(&walls);
            let middle = |ring: &Vec<DVec2>| ring.iter().sum::<DVec2>() / ring.len() as f64;
            found.sort_by(|a, b| {
                let (p, q) = (middle(a), middle(b));
                p.y.total_cmp(&q.y).then(p.x.total_cmp(&q.x))
            });
            for (k, outline) in found.into_iter().enumerate() {
                let area = signed_area(&outline);
                // the name given to it, else what a space is called and its number
                let given = self.modeller.rooms.iter().find(|r| r.level == level && inside(&outline, DVec2::from_array(r.at))).map(|r| r.name.clone());
                let name = given.unwrap_or_else(|| format!("{} {}", if self.modeller.space_word.is_empty() { "Space" } else { self.modeller.space_word }, k + 1));
                out.push(Space { name, level: level.clone(), outline, area, z });
            }
        }
        out
    }

    /// The one space selected: its place in the list.
    pub(in crate::app) fn selected_space(&self) -> Option<usize> {
        let (model, shown) = (self.model.as_ref()?, self.modeller.shown?);
        let entry = self.models.iter().position(|e| e.model.id == shown)?;
        let entry = if self.models.len() == 1 { 0 } else { entry as u32 };
        let [crate::app::Pickable::Element(id)] = self.sel.items()[..] else { return None };
        let key = model.element_key(id);
        (key.model == entry && key.source >= SPACE_IDS).then(|| (key.source - SPACE_IDS) as usize)
    }

    /// Selects space `k` alone.
    pub(in crate::app) fn select_space(&mut self, k: usize) {
        let Some(scene) = self.model.clone() else { return };
        let entry = if self.models.len() == 1 { 0 } else { self.models.iter().position(|e| Some(e.model.id) == self.modeller.shown).unwrap_or(0) as u32 };
        let key = crate::model::ElementKey { model: entry, source: SPACE_IDS + k as u64 };
        self.pick_grid(None);
        self.sel.set(scene.element_with_key(key).map(crate::app::Pickable::Element));
        self.selection_changed();
    }

    /// Gives space `k` a name of its own; an empty one gives it its number back.
    pub(in crate::app) fn name_space(&mut self, k: usize, name: &str) {
        let Some(space) = self.spaces().into_iter().nth(k) else { return };
        let name = name.trim();
        let rooms = &mut self.modeller.rooms;
        rooms.retain(|r| !(r.level == space.level && inside(&space.outline, DVec2::from_array(r.at))));
        if !name.is_empty() {
            rooms.push(Room { level: space.level.clone(), at: within(&space.outline).to_array(), name: name.to_string() });
        }
        self.show_modelled();
    }

    /// The Definition section for the one space selected: its name, typed in.
    pub(in crate::app) fn space_properties(&self, p: crate::theme::Palette, l: openaec_ui::Language) -> Option<iced::Element<'_, crate::app::Message>> {
        use iced::widget::{row, text, text_input};
        let k = self.selected_space()?;
        let space = self.spaces().into_iter().nth(k)?;
        let shown = match &self.modeller.space_typing {
            Some((of, typed)) if *of == k => typed.clone(),
            _ => space.name,
        };
        Some(
            row![
                text(l.text("Naam", "Name")).size(12).color(p.text_secondary).width(iced::Length::FillPortion(2)),
                text_input("", &shown).on_input(crate::app::Message::SpaceName).on_submit(crate::app::Message::SpaceNameDone).size(12).padding([2, 6]).width(iced::Length::FillPortion(3)),
            ]
            .spacing(8)
            .align_y(iced::Center)
            .into(),
        )
    }

    /// The name typed for the space selected: in use at once.
    pub(in crate::app) fn type_space_name(&mut self, typed: String) {
        let Some(k) = self.selected_space() else { return };
        self.name_space(k, &typed);
        // the model shown was made again: the space is selected in it as it was
        self.select_space(k);
        self.modeller.space_typing = Some((k, typed));
    }

    /// What every plan says and draws by itself, made again from the model: the name and the
    /// area of each space of its level, and the swing of each door standing on it; a structural
    /// plan the labels of its beams and columns and the numbers of its piles instead (see
    /// `structure`). Called after every change of the model and when a plan is made.
    pub(in crate::app) fn tag_plans(&mut self) {
        use crate::document::{Align, Note, NOTE_HEIGHT};
        let spaces = self.spaces();
        let grid = self.modeller.model.grid;
        // each door: its level, the middle of its foot, the way its width runs, its width
        let doors: Vec<(Option<String>, DVec2, DVec2, f64)> = self
            .modeller
            .model
            .iter()
            .filter_map(|e| match e.shape_in_project(&grid) {
                Shape::Point(p) if p.opening == Some(bim_core::OpeningKind::Door) => Some((e.level.clone(), p.at.truncate(), DVec2::from_angle(p.rotation), p.size.x)),
                _ => None,
            })
            .collect();
        let levels: Vec<(String, f64)> = self.document.levels().iter().map(|l| (l.path.clone(), l.elevation)).collect();
        let top_word = if self.modeller.top_word.is_empty() { "top" } else { self.modeller.top_word };
        let model = &self.modeller.model;
        self.document.set_plan_tags(|plan| {
            if plan.drawing.as_deref() == Some("pile-schedule") {
                return super::structure::schedule_table(model, plan);
            }
            if plan.kind.structural() {
                let elevation = levels.iter().find(|(path, _)| *path == plan.level).map_or(0.0, |(_, e)| *e);
                return (super::structure::tags(model, &plan.level, elevation, plan.scale, top_word), Vec::new());
            }
            let on = |level: &Option<String>| level.as_deref().is_none_or(|l| l == plan.level);
            let line = plan.scale * NOTE_HEIGHT * 1.6;
            let mut notes = Vec::new();
            for s in spaces.iter().filter(|s| on(&s.level)) {
                let middle = s.outline.iter().sum::<DVec2>() / s.outline.len() as f64;
                let note = |text: String, down: f64| Note { at: [middle.x, middle.y - down], text, size: NOTE_HEIGHT, align: Align::Center, angle: 0.0, face: crate::typeface::Face::default(), level: false };
                notes.push(note(s.name.clone(), 0.0));
                notes.push(note(format!("{:.2} m²", s.area), line));
            }
            let mut lines = Vec::new();
            for (_, at, along, width) in doors.iter().filter(|d| on(&d.0)) {
                // hinged at the start of its width, its leaf open square to the wall, and the
                // quarter circle the leaf's end runs to the other jamb
                let hinge = *at - *along * (*width / 2.0);
                let leaf = |turn: f64| hinge + DVec2::from_angle(turn).rotate(*along) * *width;
                lines.push([hinge.to_array(), leaf(std::f64::consts::FRAC_PI_2).to_array()]);
                const STEPS: usize = 8;
                for k in 0..STEPS {
                    let turn = |k: usize| std::f64::consts::FRAC_PI_2 * k as f64 / STEPS as f64;
                    lines.push([leaf(turn(k)).to_array(), leaf(turn(k + 1)).to_array()]);
                }
            }
            (notes, lines)
        });
    }

    /// The labels of the spaces over the viewport: each in the middle of its space. Not over a
    /// floor plan's tab, which says them itself, nor over a section or an elevation.
    pub(in crate::app) fn space_labels(&self, ink: Color) -> Option<SpaceLabels> {
        if !self.modeller.in_modeller || self.modeller.shown.is_none() || self.modeller.sketch.is_some() || self.shown_view().is_some() {
            return None;
        }
        // only where the floors are looked down on: from the side the labels would lie over walls
        // and roofs
        if self.camera.pitch.abs() < LOOKING_DOWN {
            return None;
        }
        let labels: Vec<(Vec3, String)> = self
            .spaces()
            .into_iter()
            .map(|s| {
                let middle = s.outline.iter().sum::<DVec2>() / s.outline.len() as f64;
                (middle.extend(s.z).as_vec3(), format!("{}\n{:.2} m²", s.name, s.area))
            })
            .collect();
        (!labels.is_empty()).then(|| SpaceLabels { labels, view_proj: self.camera.view_proj(self.aspect()) * self.model_matrix(), ink })
    }
}

/// The spaces' names and areas drawn over the viewport.
pub struct SpaceLabels {
    pub labels: Vec<(Vec3, String)>,
    /// Model to clip space.
    pub view_proj: Mat4,
    pub ink: Color,
}

impl<M> canvas::Program<M> for SpaceLabels {
    type State = ();

    fn draw(&self, _state: &(), renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        for (at, text) in &self.labels {
            let c = self.view_proj.project_point3(*at);
            if !(0.0..=1.0).contains(&c.z) {
                continue;
            }
            frame.fill_text(canvas::Text {
                content: text.clone(),
                position: Point::new((c.x + 1.0) / 2.0 * bounds.width, (1.0 - c.y) / 2.0 * bounds.height),
                color: self.ink,
                size: 12.0.into(),
                align_x: iced::widget::text::Alignment::Center,
                align_y: alignment::Vertical::Center,
                ..canvas::Text::default()
            });
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{click, modelling};
    use super::super::ModelTool;
    use super::*;

    fn wall(a: (f64, f64), b: (f64, f64)) -> Bound {
        Bound { a: DVec2::new(a.0, a.1), b: DVec2::new(b.0, b.1), half: 0.1 }
    }

    fn areas(bounds: &[Bound]) -> Vec<f64> {
        let mut areas: Vec<f64> = regions(bounds).iter().map(|r| (signed_area(r) * 1e6).round() / 1e6).collect();
        areas.sort_by(f64::total_cmp);
        areas
    }

    #[test]
    fn four_walls_close_in_one_space_between_their_inner_faces() {
        let room = [wall((0.0, 0.0), (6.0, 0.0)), wall((6.0, 0.0), (6.0, 4.0)), wall((6.0, 4.0), (0.0, 4.0)), wall((0.0, 4.0), (0.0, 0.0))];
        assert_eq!(areas(&room), vec![5.8 * 3.8]);
        // three walls close nothing in; a fourth that stops short does not either
        assert!(regions(&room[..3]).is_empty());
        let open = [room[0], room[1], room[2], wall((0.0, 4.0), (0.0, 1.0))];
        assert!(regions(&open).is_empty());
    }

    #[test]
    fn a_wall_between_two_others_makes_two_spaces_and_a_loose_wall_none() {
        // a partition from the south wall's inner face to the north wall's: it ends on their faces
        let house = [
            wall((0.0, 0.0), (6.0, 0.0)),
            wall((6.0, 0.0), (6.0, 4.0)),
            wall((6.0, 4.0), (0.0, 4.0)),
            wall((0.0, 4.0), (0.0, 0.0)),
            wall((2.0, 0.1), (2.0, 3.9)),
            // a wall standing loose in the larger room
            wall((3.5, 1.0), (5.0, 1.0)),
        ];
        assert_eq!(areas(&house), vec![1.8 * 3.8, 3.8 * 3.8]);
        // two walls that cross make four spaces of one
        let crossed = [house[0], house[1], house[2], house[3], wall((3.0, 0.0), (3.0, 4.0)), wall((0.0, 2.0), (6.0, 2.0))];
        assert_eq!(areas(&crossed), vec![2.8 * 1.8; 4]);
    }

    #[test]
    fn the_walls_drawn_give_spaces_that_follow_them() {
        let mut app = modelling();
        app.modeller.in_modeller = true;
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        for (x, y) in [(0.0, 0.0), (6.0, 0.0), (6.0, 4.0), (0.0, 4.0), (0.0, 0.0)] {
            click(&mut app, x, y);
        }
        app.set_model_tool(None);
        let spaces = app.spaces();
        assert_eq!(spaces.len(), 1);
        assert_eq!(spaces[0].name, "Space 1");
        assert!((spaces[0].area - 5.8 * 3.8).abs() < 1e-9, "{}", spaces[0].area);
        let labels = app.space_labels(Color::BLACK).expect("a label");
        assert_eq!(labels.labels[0].1, "Space 1\n22.04 m²");
        // a wall moved, the space is what the walls close in now: the east wall a metre out
        app.select_modelled(bim_core::ElementId(2));
        assert!(app.set_temp_dim(0, 7.0), "the east wall 7 m from the west wall");
        assert!((app.spaces()[0].area - 6.8 * 3.8).abs() < 1e-9, "{}", app.spaces()[0].area);
        // the model shown holds the space as an element of its own, on a layer switched off
        let scene = app.model.clone().unwrap();
        let k = scene.layers.iter().position(|l| l.name == SPACE_LAYER).expect("a layer of spaces");
        assert!(scene.layers[k].hidden && !app.visible[k]);
        assert_eq!(scene.layers[k].elements.len(), 1);
        let props = scene.element_props(scene.layers[k].elements[0].0);
        let prop = |key: &str| props.iter().find(|(p, _)| p == key).map(|(_, v)| v.as_str());
        assert_eq!((prop("Name"), prop("class"), prop("Area"), prop("Volume")), (Some("Space 1"), Some("IfcSpace"), Some("25.84 m²"), Some("67.18 m³")));
        // the Project tab lists it with its area; its row selects it
        let _ = app.update(crate::app::Message::Browser(super::super::browser::BrowserMessage::Space(0)));
        let picked = app.sel.primary();
        assert!(matches!(picked, Some(crate::app::Pickable::Element(e)) if scene.element_props(e).iter().any(|(k, v)| k == "Name" && v == "Space 1")), "{picked:?}");
        app.sel.set(None);
        // a name typed for it is its own: the plan, the list and the files say it
        let _ = app.update(crate::app::Message::Browser(super::super::browser::BrowserMessage::Space(0)));
        assert_eq!(app.selected_space(), Some(0));
        assert!(app.selected_modelled().is_empty() && app.space_properties(crate::theme::ThemeKind::Light.palette(), openaec_ui::Language::English).is_some());
        let _ = app.update(crate::app::Message::SpaceName("Living room".into()));
        let _ = app.update(crate::app::Message::SpaceNameDone);
        assert_eq!(app.spaces()[0].name, "Living room");
        assert_eq!(app.selected_space(), Some(0), "it stays selected while its name is typed");
        let scene = app.model.clone().unwrap();
        // an IFC file says it as a space, an IFCX file too; read again, the project makes it anew
        let ifc = String::from_utf8(crate::export::to_bytes(&scene, &app.document, "house.ifc").unwrap()).unwrap();
        assert!(ifc.contains("IFCSPACE("), "the space is in the IFC file");
        let ifcx = crate::export::to_bytes(&scene, &app.document, "house.ifcx").unwrap();
        assert!(String::from_utf8_lossy(&ifcx).contains("IfcSpace"));
        let source = crate::loaders::Source::Bytes { name: "house.ifcx".into(), data: std::sync::Arc::new(ifcx) };
        let read = crate::loaders::load_with(&source, &crate::loaders::Progress::default()).unwrap();
        assert!(read.layers.iter().all(|l| l.indices.is_empty()), "a space is no mesh of the file's own");
        assert!(ifc.contains("Living room"));
        let mut back = App::default();
        back.set_model(std::sync::Arc::new(read));
        assert_eq!(back.spaces().iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Living room"], "the file keeps the name");
        // without a name it has its number again
        app.name_space(0, "");
        assert_eq!(app.spaces()[0].name, "Space 1");
        app.sel.set(None);
        // a floor plan says its spaces itself, and draws the swing of a door standing on it
        let level = app.document.levels()[0].path.clone();
        let plan = app.document.add_plan(&level).unwrap();
        app.set_model_tool(Some((ModelTool::Door, "Door")));
        click(&mut app, 3.0, 0.0);
        app.set_model_tool(None);
        let tags: Vec<String> = app.document.view_tags(&plan).iter().map(|n| n.text.clone()).collect();
        assert_eq!(tags, ["Space 1", "25.84 m²"]);
        let swing = app.document.view_symbols(&plan);
        assert_eq!(swing.len(), 9, "the leaf and the quarter circle in eight chords");
        // the leaf stands square to the wall at the hinge, as long as the door is wide
        let (a, b) = (DVec2::from_array(swing[0][0]), DVec2::from_array(swing[0][1]));
        assert!((a - DVec2::new(2.55, 0.0)).length() < 1e-9 && (b - DVec2::new(2.55, 0.9)).length() < 1e-9, "{a} {b}");
        // drawn as on a sheet, the plan's drawing holds both
        let drawing = crate::drawing2d::view_drawing(&app.document, &plan, &crate::drawing::Cut::default(), &crate::drawing2d::Rules::default()).unwrap();
        assert!(drawing.texts.iter().any(|t| t.text == "Space 1") && drawing.lines.len() >= 9);
        // a wall gone, no space, and the plan says none
        app.select_modelled(bim_core::ElementId(2));
        let _ = app.update(crate::app::Message::DeletePicked);
        assert!(app.spaces().is_empty() && app.space_labels(Color::BLACK).is_none());
        assert!(app.document.view_tags(&plan).is_empty());
    }
}
