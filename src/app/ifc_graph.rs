//! The Graph view of the IFC tab: every node of the IFCX file as a dot, hung from the node that
//! holds it. The tree runs from the roots on the left to the leaves on the right along
//! `children`; a node another inherits from is joined to it by a dashed line.
//!
//! The wheel zooms about the pointer, a drag moves the graph, a click picks the node under the
//! pointer (which selects its elements in the model, as in the other views). Nodes carry their
//! name once the graph is zoomed in far enough to read them; only what is in sight is drawn.

use super::Message;
use crate::ifcx_structure::Structure;
use crate::theme::Palette;
use iced::widget::canvas::{self, Frame, Path, Stroke};
use iced::{mouse, Color, Point, Rectangle, Vector};
use std::collections::HashSet;

/// How far apart the columns of the tree and its rows are (graph units: pixels at scale 1).
const COLUMN: f32 = 220.0;
const ROW: f32 = 22.0;
/// The radius of a node's dot (pixels at scale 1), and how near a click must be (pixels).
const DOT: f32 = 4.5;
const NEAR: f32 = 10.0;
/// From this scale on the nodes carry their names.
const NAMED: f32 = 0.55;

/// Where each node of a [`Structure`] stands in the graph, and the lines between them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GraphLayout {
    pub at: Vec<[f32; 2]>,
    /// From, to, and whether the line is an inherit (dashed) rather than a child.
    pub edges: Vec<(u32, u32, bool)>,
    /// The width and height of it all.
    pub size: [f32; 2],
}

impl GraphLayout {
    /// The layout of `s`: each node one column right of the node that first holds it, a leaf on
    /// a row of its own and a node with children in the middle of them.
    pub fn of(s: &Structure) -> Self {
        let n = s.nodes.len();
        let children: Vec<Vec<u32>> = s.nodes.iter().map(|node| node.children.iter().filter_map(|(_, path)| s.index.get(path).copied()).collect()).collect();
        let mut at = vec![[0.0f32; 2]; n];
        let mut placed = vec![false; n];
        let mut row = 0.0f32;
        let mut deepest = 0usize;
        // the roots first, then whatever nothing holds along children (reached by inherits only)
        // the roots that hold something before those that stand alone: the project on top
        let mut roots = s.roots.clone();
        roots.sort_by_key(|r| children[*r as usize].is_empty());
        let starts: Vec<u32> = roots.into_iter().chain(0..n as u32).collect();
        for start in starts {
            if placed[start as usize] {
                continue;
            }
            // depth first without recursion: a node is placed once its children are
            let mut stack: Vec<(u32, usize, usize)> = vec![(start, 0, 0)];
            placed[start as usize] = true;
            while let Some((node, depth, next)) = stack.last().copied() {
                let kids = &children[node as usize];
                match kids.iter().skip(next).position(|k| !placed[*k as usize]) {
                    Some(offset) => {
                        let k = kids[next + offset];
                        stack.last_mut().unwrap().2 = next + offset + 1;
                        placed[k as usize] = true;
                        stack.push((k, depth + 1, 0));
                    }
                    None => {
                        stack.pop();
                        deepest = deepest.max(depth);
                        // its own children are those it was the first to hold: one column on
                        let own: Vec<f32> = kids.iter().filter(|k| (at[**k as usize][0] - (depth + 1) as f32 * COLUMN).abs() < 0.5 && at[**k as usize][1] >= 0.0).map(|k| at[*k as usize][1]).collect();
                        let y = if own.is_empty() {
                            row += ROW;
                            row
                        } else {
                            (own.iter().copied().fold(f32::INFINITY, f32::min) + own.iter().copied().fold(f32::NEG_INFINITY, f32::max)) / 2.0
                        };
                        at[node as usize] = [depth as f32 * COLUMN, y];
                    }
                }
            }
        }
        let mut edges = Vec::new();
        for (k, node) in s.nodes.iter().enumerate() {
            edges.extend(children[k].iter().map(|c| (k as u32, *c, false)));
            edges.extend(node.inherits.iter().filter_map(|(_, path)| s.index.get(path).copied()).map(|t| (k as u32, t, true)));
        }
        GraphLayout { at, edges, size: [deepest as f32 * COLUMN, row + ROW] }
    }
}

/// The graph as its canvas draws it.
pub struct IfcGraph<'a> {
    pub structure: &'a Structure,
    pub layout: &'a GraphLayout,
    pub pick: Option<u32>,
    /// The nodes a search found: the others are drawn faint while it is not empty.
    pub matches: &'a HashSet<u32>,
    pub searching: bool,
    pub palette: Palette,
}

/// How the graph is looked at, and a drag under way.
#[derive(Debug, Clone, Copy)]
pub struct GraphState {
    offset: Vector,
    scale: f32,
    /// Where the button went down, and whether the pointer moved since.
    drag: Option<(Point, bool)>,
}

impl Default for GraphState {
    fn default() -> Self {
        Self { offset: Vector::new(40.0, 20.0), scale: 1.0, drag: None }
    }
}

impl IfcGraph<'_> {
    fn on_screen(state: &GraphState, at: [f32; 2]) -> Point {
        Point::new(at[0] * state.scale + state.offset.x, at[1] * state.scale + state.offset.y)
    }

    /// The colour of a node: by its class, the same class the same colour; none for a node
    /// without a class.
    fn colour(&self, node: u32) -> Color {
        let p = self.palette;
        let Some(class) = self.structure.nodes[node as usize].class.as_deref() else { return p.text_muted };
        if matches!(class, "IfcProject" | "IfcSite" | "IfcBuilding" | "IfcBuildingStorey") {
            return p.accent;
        }
        let hash = class.bytes().fold(2166136261u32, |h, b| (h ^ u32::from(b)).wrapping_mul(16777619));
        let hue = (hash % 360) as f32;
        // a mild colour of that hue: a third of the way round per channel
        let channel = |shift: f32| 0.45 + 0.35 * ((hue + shift).to_radians()).cos();
        Color::from_rgb(channel(0.0), channel(120.0), channel(240.0))
    }
}

impl canvas::Program<Message> for IfcGraph<'_> {
    type State = GraphState;

    fn update(&self, state: &mut GraphState, event: &iced::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<canvas::Action<Message>> {
        match event {
            iced::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let at = cursor.position_in(bounds)?;
                let steps = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / 50.0,
                };
                let scale = (state.scale * 1.15f32.powf(steps)).clamp(0.02, 6.0);
                // the point under the pointer stays under it
                let k = scale / state.scale;
                state.offset = Vector::new(at.x - (at.x - state.offset.x) * k, at.y - (at.y - state.offset.y) * k);
                state.scale = scale;
                Some(canvas::Action::request_redraw().and_capture())
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left | mouse::Button::Middle)) => {
                state.drag = Some((cursor.position_in(bounds)?, false));
                Some(canvas::Action::capture())
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let (from, _) = state.drag?;
                let at = cursor.position_in(bounds)?;
                state.offset = state.offset + (at - from);
                state.drag = Some((at, true));
                Some(canvas::Action::request_redraw().and_capture())
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left | mouse::Button::Middle)) => {
                let (at, moved) = state.drag.take()?;
                if moved {
                    return Some(canvas::Action::capture());
                }
                // a click: the node nearest the pointer, when near enough
                let near = self
                    .layout
                    .at
                    .iter()
                    .enumerate()
                    .map(|(k, q)| (k as u32, Self::on_screen(state, *q).distance(at)))
                    .filter(|(_, d)| *d <= NEAR.max(DOT * state.scale))
                    .min_by(|a, b| a.1.total_cmp(&b.1))?;
                Some(canvas::Action::publish(Message::IfcPick(near.0)).and_capture())
            }
            _ => None,
        }
    }

    fn draw(&self, state: &GraphState, renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let p = self.palette;
        let seen = |q: Point, margin: f32| q.x >= -margin && q.y >= -margin && q.x <= bounds.width + margin && q.y <= bounds.height + margin;
        // the lines first: a child's from its holder's dot to the child's, an inherit's dashed
        let (mut held, mut inherited) = (Vec::new(), Vec::new());
        for &(from, to, inherits) in &self.layout.edges {
            let (a, b) = (Self::on_screen(state, self.layout.at[from as usize]), Self::on_screen(state, self.layout.at[to as usize]));
            if !seen(a, 0.0) && !seen(b, 0.0) && (a.y < 0.0) == (b.y < 0.0) && (a.x < 0.0) == (b.x < 0.0) {
                continue;
            }
            if inherits { &mut inherited } else { &mut held }.push((a, b));
        }
        let lines = |pairs: &[(Point, Point)]| {
            Path::new(|path| {
                for (a, b) in pairs {
                    // out to the right, then across: the tree reads as a tree
                    let mid = (a.x + b.x) / 2.0;
                    path.move_to(*a);
                    path.bezier_curve_to(Point::new(mid, a.y), Point::new(mid, b.y), *b);
                }
            })
        };
        frame.stroke(&lines(&held), Stroke::default().with_color(Color { a: 0.55, ..p.text_muted }).with_width(1.0));
        let dashed = Stroke { line_dash: canvas::LineDash { segments: &[5.0, 4.0], offset: 0 }, ..Stroke::default().with_color(Color { a: 0.5, ..p.accent }).with_width(1.0) };
        frame.stroke(&lines(&inherited), dashed);
        // the nodes, each a dot in the colour of its class, with its name when it can be read
        let radius = (DOT * state.scale.sqrt()).clamp(1.5, 9.0);
        for (k, at) in self.layout.at.iter().enumerate() {
            let q = Self::on_screen(state, *at);
            if !seen(q, 200.0) {
                continue;
            }
            let k = k as u32;
            let faint = self.searching && !self.matches.contains(&k);
            let colour = self.colour(k);
            frame.fill(&Path::circle(q, radius), if faint { Color { a: 0.25, ..colour } } else { colour });
            if self.pick == Some(k) {
                frame.stroke(&Path::circle(q, radius + 3.0), Stroke::default().with_color(p.accent).with_width(2.0));
            }
            if state.scale >= NAMED || self.pick == Some(k) {
                let node = &self.structure.nodes[k as usize];
                let mut name: String = node.label().chars().take(30).collect();
                if node.label().chars().count() > 30 {
                    name.push('…');
                }
                frame.fill_text(canvas::Text {
                    content: name,
                    position: Point::new(q.x + radius + 4.0, q.y - 7.0),
                    size: 11.0.into(),
                    color: if faint { p.text_muted } else { p.text },
                    ..canvas::Text::default()
                });
            }
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, state: &GraphState, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        match (state.drag, cursor.is_over(bounds)) {
            (Some((_, true)), _) => mouse::Interaction::Grabbing,
            (_, true) => mouse::Interaction::Pointer,
            _ => mouse::Interaction::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_node_stands_one_column_right_of_its_holder_and_in_the_middle_of_its_children() {
        let file = br#"{"data":[
            {"path":"project","children":{"Building":"building"},"attributes":{"bsi::ifc::class":{"code":"IfcProject"}}},
            {"path":"building","children":{"A":"a","B":"b","C":"c"}},
            {"path":"a"},{"path":"b","inherits":{"type":"kind"}},{"path":"c"},
            {"path":"kind"}
        ]}"#;
        let s = Structure::read("test.ifcx", file).unwrap();
        let g = GraphLayout::of(&s);
        let at = |path: &str| g.at[s.index[path] as usize];
        assert_eq!((at("project")[0], at("building")[0], at("a")[0]), (0.0, COLUMN, 2.0 * COLUMN));
        // the three leaves on rows of their own, their holder beside the middle one
        assert!(at("a")[1] < at("b")[1] && at("b")[1] < at("c")[1]);
        assert_eq!(at("building")[1], at("b")[1]);
        assert_eq!(at("project")[1], at("building")[1]);
        // what only an inherit reaches stands as a root of its own, below
        assert_eq!(at("kind")[0], 0.0);
        assert!(at("kind")[1] > at("c")[1]);
        // four lines along children, one dashed along the inherit
        assert_eq!(g.edges.iter().filter(|e| !e.2).count(), 4);
        assert_eq!(g.edges.iter().filter(|e| e.2).map(|e| (e.0, e.1)).collect::<Vec<_>>(), [(s.index["b"], s.index["kind"])]);
    }
}
