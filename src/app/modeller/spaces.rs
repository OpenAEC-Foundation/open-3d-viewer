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
                out.push(Space { name: format!("{} {}", if self.modeller.space_word.is_empty() { "Space" } else { self.modeller.space_word }, k + 1), level: level.clone(), outline, area, z });
            }
        }
        out
    }

    /// What every floor plan says and draws by itself, made again from the model: the name and
    /// the area of each space of its level, and the swing of each door standing on it. Called
    /// after every change of the model and when a plan is made.
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
        self.document.set_plan_tags(|plan| {
            let on = |level: &Option<String>| level.as_deref().is_none_or(|l| l == plan.level);
            let line = plan.scale * NOTE_HEIGHT * 1.6;
            let mut notes = Vec::new();
            for s in spaces.iter().filter(|s| on(&s.level)) {
                let middle = s.outline.iter().sum::<DVec2>() / s.outline.len() as f64;
                let note = |text: String, down: f64| Note { at: [middle.x, middle.y - down], text, size: NOTE_HEIGHT, align: Align::Center, angle: 0.0, face: crate::typeface::Face::default() };
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
        let drawing = crate::drawing2d::view_drawing(&app.document, &plan, &crate::drawing::Cut::default()).unwrap();
        assert!(drawing.texts.iter().any(|t| t.text == "Space 1") && drawing.lines.len() >= 9);
        // a wall gone, no space, and the plan says none
        app.select_modelled(bim_core::ElementId(2));
        let _ = app.update(crate::app::Message::DeletePicked);
        assert!(app.spaces().is_empty() && app.space_labels(Color::BLACK).is_none());
        assert!(app.document.view_tags(&plan).is_empty());
    }
}
