//! Legends and drafting views (Revit, beta): the explorer groups, the document tabs and the
//! drawing in the workspace: its lines, texts and dimensions at its scale. The wheel zooms, a
//! drag pans, and a click picks what is under it: all that the same element of the file drew.

use super::{group_header, App, Message};
use crate::document::Tick;
use crate::model::{Draft, DraftKind};
use crate::theme::Palette;
use iced::alignment;
use iced::mouse;
use iced::widget::canvas::{self, Frame, Path, Stroke};
use iced::widget::text::Wrapping;
use iced::widget::{button, canvas as canvas_widget, container, text, Column};
use iced::{Color, Element, Fill, Padding, Point, Rectangle, Size, Task, Vector};
use openaec_ui::Language;

/// Margin around the drawing in the workspace (logical pixels).
const MARGIN: f32 = 32.0;
/// How close a click must come to a line to pick it (logical pixels).
const PICK: f32 = 5.0;

/// What a click picks in a drawing without a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftPick {
    Line,
    Text,
    Dimension,
}

impl App {
    /// The legends and drafting views of the model.
    pub(super) fn drafts(&self) -> &[Draft] {
        self.model.as_ref().map_or(&[], |m| &m.project.drafts)
    }

    /// Opens a legend or drafting view in its own tab, or shows its tab when it is open.
    pub(super) fn open_draft(&mut self, i: usize) -> Task<Message> {
        if !self.open_drafts.contains(&i) {
            self.open_drafts.push(i);
        }
        self.show_draft(Some(i))
    }

    /// Shows a drawing's tab (`None`: what was shown under it).
    pub(super) fn show_draft(&mut self, i: Option<usize>) -> Task<Message> {
        if self.active_draft != i {
            self.draft_pick = None;
        }
        self.active_draft = i;
        if i.is_some() {
            self.active_sheet = None;
            self.active_schedule = None;
            self.ifc.shown = false;
            self.active_doc_sheet = None;
        }
        Task::none()
    }

    /// Closes a drawing's tab; when it was shown, the one before it (or the 3D view) shows.
    pub(super) fn close_draft(&mut self, i: usize) -> Task<Message> {
        let Some(k) = self.open_drafts.iter().position(|&d| d == i) else { return Task::none() };
        self.open_drafts.remove(k);
        if self.active_draft != Some(i) {
            return Task::none();
        }
        let next = k.checked_sub(1).or((!self.open_drafts.is_empty()).then_some(0)).map(|k| self.open_drafts[k]);
        self.show_draft(next)
    }

    /// The explorer groups of the legends and of the drafting views, by name.
    pub(super) fn drafts_groups<'a>(&self, p: Palette, l: Language, query: &str, mut body: Column<'a, Message>) -> Column<'a, Message> {
        let drafts = self.drafts();
        for (kind, slot, title) in [(DraftKind::Legend, 5, l.text("Legenda's", "Legends")), (DraftKind::Drafting, 6, l.text("Detailtekeningen", "Drafting views"))] {
            let shown: Vec<usize> = (0..drafts.len()).filter(|&i| drafts[i].kind == kind && (query.is_empty() || drafts[i].name.to_lowercase().contains(query))).collect();
            if !drafts.iter().any(|d| d.kind == kind) {
                continue;
            }
            let open = self.explorer_open[slot] || !query.is_empty();
            body = body.push(group_header(p, title, Some("beta"), shown.len(), open, Message::ToggleExplorer(slot)));
            if !open {
                continue;
            }
            for i in shown {
                let active = self.active_draft == Some(i);
                let d = &drafts[i];
                // drawings nothing could be read of are muted
                let empty = d.lines.is_empty() && d.texts.is_empty() && d.dimensions.is_empty();
                let color = if active { p.accent } else if empty { p.text_muted } else { p.text };
                let item = button(text(d.name.clone()).size(12).color(color).wrapping(Wrapping::WordOrGlyph).width(Fill))
                    .padding([3, 6])
                    .width(Fill)
                    .style(move |_, s| if active { p.ribbon_button(true, s) } else { p.row_button(s) })
                    .on_press(Message::OpenDraft(i));
                body = body.push(container(item).padding(Padding { left: 14.0, ..Padding::ZERO }).width(Fill));
            }
        }
        body
    }

    /// The shown drawing in the workspace.
    pub(super) fn draft_view_element(&self, p: Palette, l: Language, draft: &Draft) -> Element<'_, Message> {
        let drawing = DraftDrawing {
            draft: draft.clone(),
            pick: self.draft_pick,
            palette: p,
            empty: l.text("Van deze tekening is nog niets gelezen", "Nothing of this drawing is read yet").to_string(),
            words: [l.text("Lijn", "Line"), l.text("Tekst", "Text"), l.text("Maat", "Dimension")],
        };
        container(canvas_widget(drawing).width(Fill).height(Fill)).width(Fill).height(Fill).style(move |_| p.panel()).into()
    }
}

/// A drawing without a model on a canvas.
struct DraftDrawing {
    draft: Draft,
    /// The element picked, and what of it was clicked.
    pick: Option<(u64, DraftPick)>,
    palette: Palette,
    empty: String,
    /// "Line", "Text", "Dimension".
    words: [&'static str; 3],
}

/// How the pointer moved the drawing: zoomed in by `zoom` about the fit, moved by `pan`.
struct DraftState {
    zoom: f32,
    pan: Vector,
    /// Where a drag started, and the pan then.
    drag: Option<(Point, Vector)>,
}

impl Default for DraftState {
    fn default() -> Self {
        Self { zoom: 1.0, pan: Vector::ZERO, drag: None }
    }
}

/// The mapping from drawing metres to canvas pixels.
#[derive(Clone, Copy)]
struct Fit {
    scale: f32,
    /// The drawing point at the canvas point `at`.
    centre: [f32; 2],
    at: Point,
}

impl Fit {
    fn point(&self, p: [f32; 2]) -> Point {
        Point::new(self.at.x + (p[0] - self.centre[0]) * self.scale, self.at.y - (p[1] - self.centre[1]) * self.scale)
    }
}

/// How far `pointer` is from the segment `a`–`b`.
fn near(a: Point, b: Point, pointer: Point) -> f32 {
    let (ab, ap) = (b - a, pointer - a);
    let t = ((ap.x * ab.x + ap.y * ab.y) / (ab.x * ab.x + ab.y * ab.y).max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t).distance(pointer)
}

impl DraftDrawing {
    /// The drawing fitted in the canvas, then zoomed and moved as `state` says.
    fn fit(&self, state: &DraftState, size: Size) -> Option<Fit> {
        let e = self.draft.extent()?;
        let (w, h) = ((e.max[0] - e.min[0]).max(1e-4), (e.max[1] - e.min[1]).max(1e-4));
        let scale = ((size.width - 2.0 * MARGIN) / w).min((size.height - 2.0 * MARGIN) / h).max(1e-3) * state.zoom;
        Some(Fit { scale, centre: e.center(), at: Point::new(size.width / 2.0 + state.pan.x, size.height / 2.0 + state.pan.y) })
    }

    /// The element under a canvas point: a text first, then a dimension, then a line.
    fn at(&self, fit: &Fit, pointer: Point) -> Option<(u64, DraftPick)> {
        let d = &self.draft;
        let text = d.texts.iter().rev().find(|t| {
            let (a, b) = (fit.point([t.at[0], t.at[1] + t.height]), fit.point([t.at[0] + t.width.max(t.size), t.at[1]]));
            Rectangle::new(a, Size::new(b.x - a.x, b.y - a.y)).expand(PICK).contains(pointer)
        });
        if let Some(t) = text {
            return Some((t.element, DraftPick::Text));
        }
        let close = |a: [f32; 2], b: [f32; 2]| near(fit.point(a), fit.point(b), pointer) <= PICK;
        if let Some(m) = d.dimensions.iter().find(|m| close(m.from, m.to)) {
            return Some((m.element, DraftPick::Dimension));
        }
        d.lines.iter().find(|l| close(l.a, l.b)).map(|l| (l.element, DraftPick::Line))
    }
}

impl canvas::Program<Message> for DraftDrawing {
    type State = DraftState;

    fn update(&self, state: &mut DraftState, event: &iced::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<canvas::Action<Message>> {
        match event {
            iced::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let p = cursor.position_in(bounds)?;
                let steps = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / 50.0,
                };
                let factor = 1.2f32.powf(steps).clamp(0.2, 5.0);
                let zoom = (state.zoom * factor).clamp(0.05, 2000.0);
                // the point under the pointer stays under it
                let factor = zoom / state.zoom;
                let centre = Point::new(bounds.width / 2.0, bounds.height / 2.0);
                let from = Vector::new(p.x - centre.x - state.pan.x, p.y - centre.y - state.pan.y);
                state.pan = Vector::new(p.x - centre.x - from.x * factor, p.y - centre.y - from.y * factor);
                state.zoom = zoom;
                Some(canvas::Action::request_redraw().and_capture())
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(button @ (mouse::Button::Left | mouse::Button::Middle))) => {
                let p = cursor.position_in(bounds)?;
                state.drag = Some((p, state.pan));
                if *button == mouse::Button::Middle {
                    return Some(canvas::Action::capture());
                }
                let pick = self.fit(state, bounds.size()).and_then(|fit| self.at(&fit, p));
                Some(canvas::Action::publish(Message::PickDraft(pick)).and_capture())
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let (from, pan) = state.drag?;
                let p = cursor.position_in(bounds)?;
                state.pan = Vector::new(pan.x + p.x - from.x, pan.y + p.y - from.y);
                Some(canvas::Action::request_redraw())
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left | mouse::Button::Middle)) => {
                state.drag.take();
                None
            }
            _ => None,
        }
    }

    fn draw(&self, state: &DraftState, renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let p = self.palette;
        let mut frame = Frame::new(renderer, bounds.size());
        let label = |frame: &mut Frame, content: String, position: Point, size: f32, color: Color, align: alignment::Horizontal, vertical: alignment::Vertical| {
            frame.fill_text(canvas::Text {
                content,
                position,
                align_x: align.into(),
                align_y: vertical,
                size: iced::Pixels(size),
                line_height: iced::widget::text::LineHeight::Relative(1.3),
                color,
                font: openaec_ui::fonts::MEDIUM,
                ..canvas::Text::default()
            });
        };
        let Some(fit) = self.fit(state, bounds.size()) else {
            let centre = Point::new(bounds.width / 2.0, bounds.height / 2.0);
            label(&mut frame, self.empty.clone(), centre, 13.0, p.text_muted, alignment::Horizontal::Center, alignment::Vertical::Center);
            return vec![frame.into_geometry()];
        };
        let d = &self.draft;
        let ink = p.text;
        let picked = |element: u64| self.pick.is_some_and(|(e, _)| e == element);
        let colour = |element: u64| if picked(element) { p.accent } else { ink };

        // the lines, those of the picked element over the others
        let stroke = |frame: &mut Frame, on: bool| {
            let path = Path::new(|path| {
                for l in d.lines.iter().filter(|l| picked(l.element) == on) {
                    path.move_to(fit.point(l.a));
                    path.line_to(fit.point(l.b));
                }
            });
            frame.stroke(&path, Stroke::default().with_color(if on { p.accent } else { ink }).with_width(if on { 2.0 } else { 1.0 }));
        };
        stroke(&mut frame, false);
        stroke(&mut frame, true);

        // the dimensions: line, extension lines, the mark of their style at the ends and the
        // length as their style writes it
        let paper = d.scale * fit.scale;
        for m in &d.dimensions {
            let (a, b) = (fit.point(m.from), fit.point(m.to));
            let run = a.distance(b);
            if run <= 0.0 {
                continue;
            }
            let color = colour(m.element);
            let (ux, uy) = ((b.x - a.x) / run, (b.y - a.y) / run);
            let tick = (m.style.tick_size as f32 * paper).max(2.0);
            let lines = Path::new(|path| {
                path.move_to(a);
                path.line_to(b);
                for (q, from) in [(a, fit.point(m.measured[0])), (b, fit.point(m.measured[1]))] {
                    path.move_to(from);
                    path.line_to(q);
                    if m.style.tick == Tick::Slash {
                        path.move_to(Point::new(q.x - (ux + uy) * tick, q.y - (uy - ux) * tick));
                        path.line_to(Point::new(q.x + (ux + uy) * tick, q.y + (uy - ux) * tick));
                    }
                }
            });
            frame.stroke(&lines, Stroke::default().with_color(color).with_width(if picked(m.element) { 1.6 } else { 0.8 }));
            match m.style.tick {
                Tick::Arrow => {
                    for (q, inward) in [(a, 1.0), (b, -1.0)] {
                        let back = Point::new(q.x + ux * tick * inward, q.y + uy * tick * inward);
                        let (wx, wy) = (-uy * tick * 0.27, ux * tick * 0.27);
                        let head = Path::new(|path| {
                            path.move_to(q);
                            path.line_to(Point::new(back.x + wx, back.y + wy));
                            path.line_to(Point::new(back.x - wx, back.y - wy));
                            path.close();
                        });
                        frame.fill(&head, color);
                    }
                }
                Tick::Dot => {
                    for q in [a, b] {
                        frame.fill(&Path::circle(q, (tick / 2.0).max(1.0)), color);
                    }
                }
                Tick::Slash | Tick::None => {}
            }
            let size = m.style.size as f32 * paper;
            if size >= 3.0 {
                let length = (m.to[0] - m.from[0]).hypot(m.to[1] - m.from[1]) as f64;
                // above a level line, to the left of an upright one
                let (nx, ny) = if uy.abs() > ux.abs() { (-1.0, 0.0) } else { (0.0, -1.0) };
                let content = m.style.figure(length);
                let clear = if nx != 0.0 { content.chars().count() as f32 * size * 0.3 + 2.0 } else { size * 0.7 };
                let at = Point::new((a.x + b.x) / 2.0 + nx * clear, (a.y + b.y) / 2.0 + ny * clear);
                label(&mut frame, content, at, size, color, alignment::Horizontal::Center, alignment::Vertical::Center);
            }
        }

        // the texts, from the top of their box where their lines start
        for t in &d.texts {
            let size = t.size * fit.scale;
            let at = fit.point([t.at[0], t.at[1] + t.height]);
            if size < 2.0 {
                // too small to read: where it is
                let end = fit.point([t.at[0] + t.width.max(t.size), t.at[1] + t.height]);
                frame.stroke(&Path::line(at, end), Stroke::default().with_color(Color { a: 0.4, ..colour(t.element) }).with_width(1.0));
                continue;
            }
            let (start, side) = match t.align {
                1 => (fit.point([t.at[0] + t.width / 2.0, t.at[1] + t.height]), alignment::Horizontal::Center),
                2 => (fit.point([t.at[0] + t.width, t.at[1] + t.height]), alignment::Horizontal::Right),
                _ => (at, alignment::Horizontal::Left),
            };
            if t.angle == 0.0 {
                label(&mut frame, t.text.clone(), start, size, colour(t.element), side, alignment::Vertical::Top);
            } else {
                // turned about the foot of its box, under where its lines start
                let foot = fit.point([[t.at[0], t.at[0] + t.width / 2.0, t.at[0] + t.width][t.align.min(2) as usize], t.at[1]]);
                frame.with_save(|frame| {
                    frame.translate(iced::Vector::new(foot.x, foot.y));
                    frame.rotate(-t.angle);
                    label(frame, t.text.clone(), Point::new(0.0, start.y - foot.y), size, colour(t.element), side, alignment::Vertical::Top);
                });
            }
            if picked(t.element) {
                let low = fit.point([t.at[0] + t.width.max(t.size), t.at[1]]);
                frame.stroke(&Path::rectangle(at, Size::new(low.x - at.x, low.y - at.y)), Stroke::default().with_color(p.accent).with_width(1.0));
            }
        }

        // the drawing's name and scale, and what is picked
        let foot = Point::new(12.0, bounds.height - 10.0);
        let mut caption = format!("{}  ·  1:{}", d.name, d.scale.round());
        if let Some((element, kind)) = self.pick {
            let word = self.words[match kind {
                DraftPick::Line => 0,
                DraftPick::Text => 1,
                DraftPick::Dimension => 2,
            }];
            caption.push_str(&format!("  ·  {word} {element}"));
        }
        label(&mut frame, caption, foot, 12.0, p.text_secondary, alignment::Horizontal::Left, alignment::Vertical::Bottom);
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, state: &DraftState, _bounds: Rectangle, _cursor: mouse::Cursor) -> mouse::Interaction {
        if state.drag.is_some() {
            mouse::Interaction::Grabbing
        } else {
            mouse::Interaction::default()
        }
    }
}
