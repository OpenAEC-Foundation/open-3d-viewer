//! Dimensions and texts on a view, in its own tab: a layer over the viewport draws the view's
//! dimensions and texts where they are in the model, seen by the tab's camera, and with the
//! tools of the bar above it adds them: two clicks for a dimension (snapping to the ends of the
//! view's lines), one for a text. A click picks one, a picked text is dragged to move it, and
//! Delete removes what is picked. They belong to the view, so every sheet it is placed on shows
//! them too.

use super::{App, Message};
use crate::document::{Dimension, Note};
use crate::drawing::Cut;
use crate::measure::hover::Screen;
use crate::theme::Palette;
use glam::{Mat4, Vec2, Vec3};
use iced::widget::canvas::{self, Frame, Path, Stroke};
use iced::widget::{button, container, text, text_input, Row};
use iced::{alignment, keyboard, mouse, Center, Color, Element, Fill, Point, Rectangle, Size};
use openaec_ui::Language;
use std::sync::Arc;

/// How close the pointer must come to the end of a line to snap to it (logical pixels).
const SNAP: f32 = 8.0;
/// How close a click must come to a dimension line or a text to pick it (logical pixels).
const PICK: f32 = 5.0;
/// How far a dimension line stands off what it measures in a view's tab (logical pixels).
const OFFSET: f32 = 18.0;
/// The size of a text in a view's tab (logical pixels).
const NOTE_SIZE: f32 = 13.0;

/// What can be picked on the view shown in its tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkPick {
    /// A dimension of the view (its index).
    Dimension(usize),
    /// A text of the view (its index).
    Note(usize),
}

/// A view's drawing in the model: the model point of its origin and the model directions of its
/// u and v (a floor plan: x and y at its cut; a section: along its line, and up).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct DrawingFrame {
    origin: Vec3,
    u: Vec3,
    v: Vec3,
}

impl DrawingFrame {
    /// The model point of drawing point `q`.
    fn world(&self, q: [f64; 2]) -> Vec3 {
        self.origin + self.u * q[0] as f32 + self.v * q[1] as f32
    }

    /// The drawing point a ray (from, direction) meets, if it meets the drawing's plane.
    fn drawing(&self, (from, dir): (Vec3, Vec3)) -> Option<[f64; 2]> {
        let n = self.u.cross(self.v);
        let towards = dir.dot(n);
        if towards.abs() < 1e-6 {
            return None;
        }
        let d = from + dir * ((self.origin - from).dot(n) / towards) - self.origin;
        Some([d.dot(self.u) as f64, d.dot(self.v) as f64])
    }
}

impl App {
    /// The drawing of a view in the model: a floor plan at its cut, x and y; a section along its
    /// line, u from its start and v up.
    pub(super) fn drawing_frame(&self, path: &str) -> Option<DrawingFrame> {
        if let Some(plan) = self.document.plan(path) {
            let (_, cut) = self.document.plan_range(plan)?;
            return Some(DrawingFrame { origin: Vec3::new(0.0, 0.0, cut as f32), u: Vec3::X, v: Vec3::Y });
        }
        let s = self.document.section(path)?;
        let (along, _) = s.axes();
        Some(DrawingFrame { origin: Vec3::new(s.from[0] as f32, s.from[1] as f32, 0.0), u: Vec3::new(along[0] as f32, along[1] as f32, 0.0), v: Vec3::Z })
    }

    /// The view whose tab shows in the workspace (no sheet over it).
    pub(super) fn shown_view(&self) -> Option<&str> {
        (self.active_doc_sheet.is_none() && self.active_sheet.is_none()).then_some(())?;
        self.active_plan.as_deref()
    }

    /// The text a click of the text tool places: the one typed, else "Text".
    pub(super) fn note_to_place(&self, l: Language) -> String {
        match self.note_text.trim() {
            "" => l.text("Tekst", "Text").to_string(),
            typed => typed.to_string(),
        }
    }

    /// The layer over the viewport with the dimensions and texts of the view shown.
    pub(super) fn marks_layer(&self, p: Palette, l: Language) -> Option<ViewMarks> {
        let view = self.shown_view()?;
        let frame = self.drawing_frame(view)?;
        let (dimensions, _) = self.document.view_marks(view)?;
        Some(ViewMarks {
            view: Arc::from(view),
            frame,
            view_proj: self.camera.view_proj(self.aspect()),
            dimensions: dimensions.to_vec(),
            notes: self.document.view_notes(view)?.to_vec(),
            cut: self.cuts.get(view).cloned(),
            measuring: self.dimension_tool,
            noting: self.note_tool.then(|| self.note_to_place(l)),
            pick: self.mark_pick,
            palette: p,
        })
    }

    /// The bar over a view's tab: its tools, then what goes with the tool on or what is picked.
    pub(super) fn view_bar(&self, p: Palette, l: Language) -> Option<Element<'_, Message>> {
        self.shown_view()?;
        let (measuring, noting) = (self.dimension_tool, self.note_tool);
        let tool = |label: &'static str, on: bool, message: Message| {
            button(text(label).size(12)).padding([3, 10]).style(move |_, s| p.ribbon_button(on, s)).on_press(message)
        };
        let hint = |line: &'static str| text(line).size(12).color(p.text_secondary);
        let mut bar = Row::new()
            .spacing(6)
            .align_y(Center)
            .push(tool(l.text("Maatvoering", "Dimension"), measuring, Message::DimensionTool(!measuring)))
            .push(tool(l.text("Tekst", "Text"), noting, Message::NoteTool(!noting)))
            .push(text("|").size(12).color(p.text_muted));
        let wording = || text_input(l.text("Tekst", "Text"), &self.note_text).on_input(Message::NoteText).size(12).padding([3, 6]).width(220);
        let remove = |label: &'static str| tool(label, false, Message::RemoveMark);
        bar = if measuring {
            bar.push(hint(l.text("Klik twee punten; ze springen naar de uiteinden van de lijnen. Esc breekt af.", "Click two points; they snap to the ends of the lines. Esc cancels.")))
        } else if noting {
            bar.push(wording()).push(hint(l.text("Klik waar de tekst komt.", "Click where the text goes.")))
        } else {
            match self.mark_pick {
                Some(MarkPick::Note(_)) => bar.push(wording()).push(remove(l.text("Tekst verwijderen", "Remove text"))),
                Some(MarkPick::Dimension(_)) => bar.push(remove(l.text("Maat verwijderen", "Remove dimension"))),
                None => bar.push(hint(l.text(
                    "Maten en teksten horen bij deze weergave: ze staan ook op elk blad waar hij op staat.",
                    "Dimensions and texts belong to this view: they show on every sheet it is placed on too.",
                ))),
            }
        };
        Some(container(bar).padding([6, 10]).width(Fill).style(move |_| p.ribbon()).into())
    }

    /// Picks a dimension or text of the view shown; a picked text's wording can then be edited.
    pub(super) fn pick_mark(&mut self, pick: Option<MarkPick>) {
        self.mark_pick = pick;
        if let (Some(MarkPick::Note(k)), Some(view)) = (pick, self.shown_view()) {
            if let Some(note) = self.document.view_notes(view).and_then(|n| n.get(k)) {
                self.note_text = note.text.clone();
            }
        }
    }

    /// Removes what is picked on the view shown.
    pub(super) fn remove_mark(&mut self) {
        let Some(view) = self.shown_view().map(str::to_string) else { return };
        match self.mark_pick.take() {
            Some(MarkPick::Dimension(k)) => {
                self.document.remove_dimension(&view, k);
            }
            Some(MarkPick::Note(k)) => {
                self.document.remove_note(&view, k);
            }
            None => {}
        }
    }

    /// The wording typed: the picked text takes it (on the view shown, or on the sheet shown),
    /// else the next text placed does.
    pub(super) fn note_text_changed(&mut self, wording: String) {
        self.note_text = wording;
        let picked = match (self.shown_view(), self.mark_pick, self.sheet_pick) {
            (Some(view), Some(MarkPick::Note(k)), _) => Some((view.to_string(), k)),
            (None, _, Some(super::paper::SheetPick::Note(i, k))) => self.placed_plan(i).map(|v| (v, k)),
            _ => None,
        };
        if let Some((view, k)) = picked {
            self.document.set_note_text(&view, k, &self.note_text);
        }
    }
}

/// The dimensions and texts of a view over its viewport, and its tools.
pub(super) struct ViewMarks {
    view: Arc<str>,
    frame: DrawingFrame,
    /// The model to clip space of the tab's camera.
    view_proj: Mat4,
    dimensions: Vec<Dimension>,
    notes: Vec<Note>,
    /// The view's lines, to snap to.
    cut: Option<Arc<Cut>>,
    /// The dimension tool is on.
    measuring: bool,
    /// The text tool is on, with the text a click places.
    noting: Option<String>,
    pick: Option<MarkPick>,
    palette: Palette,
}

/// The first point of a dimension being made, where the pointer snaps to now, a text being
/// dragged (its index and where it was taken: its point less the pointer's), and whether the
/// pointer is on a dimension or text.
#[derive(Default)]
pub(super) struct MarksState {
    first: Option<[f64; 2]>,
    aim: Option<[f64; 2]>,
    drag: Option<(usize, [f64; 2])>,
    hover: bool,
}

impl ViewMarks {
    fn screen(&self, bounds: Rectangle) -> Screen {
        Screen { view_proj: self.view_proj, size: (bounds.width, bounds.height) }
    }

    /// A drawing point on screen.
    fn px(&self, screen: &Screen, q: [f64; 2]) -> Option<Point> {
        screen.project(self.frame.world(q)).map(|p| Point::new(p.x, p.y))
    }

    /// The drawing point under the pointer.
    fn at(&self, screen: &Screen, pointer: Point) -> Option<[f64; 2]> {
        self.frame.drawing(screen.ray(Vec2::new(pointer.x, pointer.y)))
    }

    /// The drawing point a click at `pointer` means: the nearest end of one of the view's lines
    /// within [`SNAP`] pixels, else the point under it.
    fn snap(&self, screen: &Screen, pointer: Point) -> Option<[f64; 2]> {
        let ends = self.cut.iter().flat_map(|c| c.segments.iter().chain(&c.below)).flatten();
        ends.map(|q| [q[0] as f64, q[1] as f64])
            .filter_map(|q| Some((q, self.px(screen, q)?.distance(pointer))))
            .filter(|(_, d)| *d <= SNAP)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(q, _)| q)
            .or_else(|| self.at(screen, pointer))
    }

    /// The ends of dimension `k`'s line on screen, [`OFFSET`] to the left of what it measures.
    fn dimension_line(&self, screen: &Screen, k: usize) -> Option<(Point, Point)> {
        let d = self.dimensions.get(k)?;
        let (a, b) = (self.px(screen, d.from)?, self.px(screen, d.to)?);
        let run = a.distance(b);
        (run > 0.0).then(|| {
            let n = iced::Vector::new((b.y - a.y) / run * OFFSET, -(b.x - a.x) / run * OFFSET);
            (a + n, b + n)
        })
    }

    /// Where text `k` is on screen: from its point to the right and up.
    fn note_box(&self, screen: &Screen, k: usize) -> Option<Rectangle> {
        let note = self.notes.get(k)?;
        let at = self.px(screen, note.at)?;
        let width = note.text.chars().count() as f32 * NOTE_SIZE * 0.56;
        Some(Rectangle::new(Point::new(at.x, at.y - NOTE_SIZE * 1.2), Size::new(width, NOTE_SIZE * 1.2)))
    }

    /// The text or dimension under the pointer, the texts first.
    fn mark_at(&self, screen: &Screen, pointer: Point) -> Option<MarkPick> {
        let note = (0..self.notes.len()).rev().find(|&k| self.note_box(screen, k).is_some_and(|b| b.expand(PICK).contains(pointer)));
        note.map(MarkPick::Note).or_else(|| {
            (0..self.dimensions.len())
                .find(|&k| self.dimension_line(screen, k).is_some_and(|(a, b)| near(a, b, pointer) <= PICK))
                .map(MarkPick::Dimension)
        })
    }

    fn busy(&self) -> bool {
        self.measuring || self.noting.is_some()
    }
}

/// How far `pointer` is from the segment `a`–`b`.
fn near(a: Point, b: Point, pointer: Point) -> f32 {
    let (ab, ap) = (b - a, pointer - a);
    let t = ((ap.x * ab.x + ap.y * ab.y) / (ab.x * ab.x + ab.y * ab.y).max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t).distance(pointer)
}

impl canvas::Program<Message> for ViewMarks {
    type State = MarksState;

    fn update(&self, state: &mut MarksState, event: &iced::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<canvas::Action<Message>> {
        let screen = self.screen(bounds);
        if !self.measuring {
            state.first = None;
        }
        if !self.busy() {
            state.aim = None;
        }
        let pointer = cursor.position_in(bounds);
        match event {
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let (Some((k, grip)), Some(p)) = (state.drag, pointer) {
                    let q = self.at(&screen, p)?;
                    return Some(canvas::Action::publish(Message::MoveNote(self.view.clone(), k, [q[0] + grip[0], q[1] + grip[1]])).and_capture());
                }
                if self.busy() {
                    state.aim = pointer.and_then(|p| if self.measuring { self.snap(&screen, p) } else { self.at(&screen, p) });
                    return Some(canvas::Action::request_redraw());
                }
                let hover = pointer.and_then(|p| self.mark_at(&screen, p)).is_some();
                (hover != state.hover).then(|| {
                    state.hover = hover;
                    canvas::Action::request_redraw()
                })
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let p = pointer?;
                if self.measuring {
                    let q = self.snap(&screen, p)?;
                    return match state.first.take() {
                        None => {
                            state.first = Some(q);
                            Some(canvas::Action::request_redraw().and_capture())
                        }
                        Some(from) => Some(canvas::Action::publish(Message::AddDimension(self.view.clone(), from, q)).and_capture()),
                    };
                }
                if let Some(wording) = &self.noting {
                    let q = self.at(&screen, p)?;
                    return Some(canvas::Action::publish(Message::AddNote(self.view.clone(), q, wording.clone())).and_capture());
                }
                match self.mark_at(&screen, p) {
                    Some(pick) => {
                        if let (MarkPick::Note(k), Some(q)) = (pick, self.at(&screen, p)) {
                            let at = self.notes[k].at;
                            state.drag = Some((k, [at[0] - q[0], at[1] - q[1]]));
                        }
                        Some(canvas::Action::publish(Message::PickMark(Some(pick))).and_capture())
                    }
                    // not taken: the click still picks in the model
                    None => self.pick.is_some().then(|| canvas::Action::publish(Message::PickMark(None))),
                }
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => state.drag.take().map(|_| canvas::Action::request_redraw()),
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key: keyboard::Key::Named(keyboard::key::Named::Escape), .. }) => {
                if state.first.take().is_some() {
                    return Some(canvas::Action::request_redraw().and_capture());
                }
                if self.measuring {
                    return Some(canvas::Action::publish(Message::DimensionTool(false)).and_capture());
                }
                self.noting.is_some().then(|| canvas::Action::publish(Message::NoteTool(false)).and_capture())
            }
            _ => None,
        }
    }

    fn draw(&self, state: &MarksState, renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let p = self.palette;
        let screen = self.screen(bounds);
        let mut frame = Frame::new(renderer, bounds.size());
        let ink = p.text;
        for (k, d) in self.dimensions.iter().enumerate() {
            let (Some(a), Some(b)) = (self.px(&screen, d.from), self.px(&screen, d.to)) else { continue };
            let color = if self.pick == Some(MarkPick::Dimension(k)) { p.accent } else { ink };
            draw_dimension_px(&mut frame, a, b, d.length(), color, DimensionLook { offset: OFFSET, tick: 4.0, gap: 4.0, size: 11.0 });
        }
        for (k, note) in self.notes.iter().enumerate() {
            let Some(at) = self.px(&screen, note.at) else { continue };
            let picked = self.pick == Some(MarkPick::Note(k));
            draw_note(&mut frame, at, &note.text, NOTE_SIZE, if picked { p.accent } else { ink });
            if picked {
                if let Some(b) = self.note_box(&screen, k) {
                    frame.stroke(&Path::rectangle(b.position(), b.size()), Stroke::default().with_color(p.accent).with_width(1.0));
                }
            }
        }
        // the dimension or text being made, where the pointer is
        if let Some(q) = state.aim.and_then(|q| Some((q, self.px(&screen, q)?))) {
            let (aim, c) = q;
            match (&self.noting, state.first) {
                (Some(wording), _) => draw_note(&mut frame, c, wording, NOTE_SIZE, Color { a: 0.6, ..p.accent }),
                (None, Some(from)) => {
                    if let Some(a) = self.px(&screen, from) {
                        let length = Dimension { from, to: aim }.length();
                        draw_dimension_px(&mut frame, a, c, length, p.accent, DimensionLook { offset: OFFSET, tick: 4.0, gap: 4.0, size: 11.0 });
                    }
                }
                (None, None) => {}
            }
            if self.measuring {
                frame.stroke(&Path::circle(c, 4.0), Stroke::default().with_color(p.accent).with_width(1.5));
            }
        }
        if let Some(a) = state.first.and_then(|q| self.px(&screen, q)) {
            frame.fill(&Path::circle(a, 3.0), p.accent);
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, state: &MarksState, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        // nothing (the viewport's own) unless on a dimension or text, so the viewport still zooms
        // and pans under the tools
        match (state.drag, state.hover && !self.busy() && cursor.is_over(bounds)) {
            (Some(_), _) => mouse::Interaction::Grabbing,
            (None, true) => mouse::Interaction::Pointer,
            _ => mouse::Interaction::None,
        }
    }
}

/// The sizes of a dimension as drawn (logical pixels): how far its line stands off the points,
/// its slashes, the gap between a point and its extension line, and its text.
#[derive(Debug, Clone, Copy)]
pub(super) struct DimensionLook {
    pub offset: f32,
    pub tick: f32,
    pub gap: f32,
    pub size: f32,
}

/// A dimension between two screen points: the dimension line `look.offset` to the left of
/// a → b (as seen with y up), extension lines from just off the points to just past it, a slash
/// at each end, and the length in millimetres over the middle.
pub(super) fn draw_dimension_px(frame: &mut Frame, a: Point, b: Point, length: f64, color: Color, look: DimensionLook) {
    let run = a.distance(b);
    if run <= 0.0 {
        return;
    }
    let (ux, uy) = ((b.x - a.x) / run, (b.y - a.y) / run);
    let (nx, ny) = (uy, -ux);
    let off = |q: Point, d: f32| Point::new(q.x + nx * d, q.y + ny * d);
    let (a2, b2) = (off(a, look.offset), off(b, look.offset));
    let slash = |q: Point| {
        let (sx, sy) = ((ux + nx) * look.tick, (uy + ny) * look.tick);
        (Point::new(q.x - sx, q.y - sy), Point::new(q.x + sx, q.y + sy))
    };
    let lines = Path::new(|path| {
        for (q, end) in [(a, a2), (b, b2)] {
            path.move_to(off(q, look.gap));
            path.line_to(off(end, look.gap));
        }
        path.move_to(a2);
        path.line_to(b2);
        for q in [a2, b2] {
            let (s, e) = slash(q);
            path.move_to(s);
            path.line_to(e);
        }
    });
    frame.stroke(&lines, Stroke::default().with_color(color).with_width(0.9));
    frame.fill_text(canvas::Text {
        content: format!("{}", (length * 1000.0).round()),
        position: off(Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0), look.offset + look.gap),
        align_x: alignment::Horizontal::Center.into(),
        align_y: alignment::Vertical::Center,
        size: iced::Pixels(look.size),
        color,
        font: openaec_ui::fonts::MEDIUM,
        ..canvas::Text::default()
    });
}

/// A text standing on a screen point (its lower left).
pub(super) fn draw_note(frame: &mut Frame, at: Point, wording: &str, size: f32, color: Color) {
    frame.fill_text(canvas::Text {
        content: wording.to_string(),
        position: at,
        align_x: alignment::Horizontal::Left.into(),
        align_y: alignment::Vertical::Bottom,
        size: iced::Pixels(size),
        color,
        font: openaec_ui::fonts::MEDIUM,
        ..canvas::Text::default()
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Layer, Model};
    use std::path::PathBuf;

    /// A room of 8 x 5 m with walls 3 m high, levels at 0 and 3, its floor plan shown in its tab
    /// in a viewport of 800 x 600, the camera where it went.
    fn plan_shown() -> (App, Arc<str>) {
        let positions = vec![0.,0.,0., 8.,0.,0., 8.,5.,0., 0.,5.,0., 0.,0.,3., 8.,0.,3., 8.,5.,3., 0.,5.,3.];
        let indices = vec![0,1,5, 0,5,4, 1,2,6, 1,6,5, 2,3,7, 2,7,6, 3,0,4, 3,4,7];
        let layer = Layer { name: "Walls".into(), positions, indices, ..Layer::default() };
        let mut model = Model::new(PathBuf::from("room.rvt"), "Revit", vec![layer]);
        model.project.levels = vec![("00".into(), 0.0), ("01".into(), 3.0)];
        let mut app = App::default();
        app.viewport_size = (800.0, 600.0);
        app.set_model(Arc::new(model));
        let level = Arc::<str>::from(app.document.levels()[0].path.as_str());
        let _ = app.update(Message::NewPlan(level));
        if let Some((_, to, _)) = app.anim.take() {
            app.camera = to;
        }
        let plan = Arc::<str>::from(app.document.plans()[0].path.as_str());
        (app, plan)
    }

    fn bounds() -> Rectangle {
        Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0))
    }

    fn layer(app: &App) -> ViewMarks {
        app.marks_layer(app.theme.palette(), Language::English).expect("the plan's tab shows")
    }

    /// Where a point of the plan's drawing is on screen.
    fn on_screen(app: &App, q: [f64; 2]) -> Point {
        let marks = layer(app);
        marks.px(&marks.screen(bounds()), q).expect("in front of the camera")
    }

    fn press(marks: &ViewMarks, state: &mut MarksState, at: Point) -> Option<canvas::Action<Message>> {
        let event = iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        canvas::Program::update(marks, state, &event, bounds(), mouse::Cursor::Available(at))
    }

    fn message(action: Option<canvas::Action<Message>>) -> Option<Message> {
        action.and_then(|a| a.into_inner().0)
    }

    #[test]
    fn two_clicks_in_a_floor_plans_tab_dimension_it_between_snapped_corners() {
        let (mut app, plan) = plan_shown();
        let _ = app.update(Message::DimensionTool(true));
        assert!(app.cuts.contains_key(&*plan), "cut to snap to");
        let marks = layer(&app);
        assert!(marks.measuring);
        // the drawing is the plan at its cut, 1.2 m up
        let frame = app.drawing_frame(&plan).unwrap();
        assert_eq!(frame.world([2.0, 3.0]), Vec3::new(2.0, 3.0, 1.2));
        let mut state = MarksState::default();
        let off = iced::Vector::new(3.0, -2.0);
        assert!(message(press(&marks, &mut state, on_screen(&app, [0.0, 0.0]) + off)).is_none());
        assert_eq!(state.first, Some([0.0, 0.0]), "snapped to the corner");
        let made = message(press(&marks, &mut state, on_screen(&app, [8.0, 0.0]) + off)).expect("a dimension");
        assert!(matches!(&made, Message::AddDimension(v, [0.0, 0.0], [8.0, 0.0]) if *v == plan), "{made:?}");
        let _ = app.update(made);
        assert_eq!(app.document.plan(&plan).unwrap().dimensions.len(), 1);
        // picked by a click on its line, and removed with Delete
        let _ = app.update(Message::DimensionTool(false));
        let marks = layer(&app);
        let screen = marks.screen(bounds());
        let (a, b) = marks.dimension_line(&screen, 0).expect("drawn");
        let pick = message(press(&marks, &mut MarksState::default(), Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0)));
        assert!(matches!(pick, Some(Message::PickMark(Some(MarkPick::Dimension(0))))), "{pick:?}");
        let _ = app.update(pick.unwrap());
        let _ = app.update(Message::DeletePicked);
        assert!(app.document.plan(&plan).unwrap().dimensions.is_empty());
        assert_eq!(app.mark_pick, None);
    }

    #[test]
    fn a_text_is_placed_with_a_click_picked_dragged_and_reworded() {
        let (mut app, plan) = plan_shown();
        let _ = app.update(Message::NoteTool(true));
        let _ = app.update(Message::NoteText("Kitchen".into()));
        let marks = layer(&app);
        let placed = message(press(&marks, &mut MarksState::default(), on_screen(&app, [2.0, 3.0]))).expect("a text");
        let Message::AddNote(v, at, wording) = &placed else { panic!("{placed:?}") };
        assert!(*v == plan && (at[0] - 2.0).abs() < 1e-3 && (at[1] - 3.0).abs() < 1e-3 && wording == "Kitchen", "{placed:?}");
        let _ = app.update(placed);
        assert_eq!(app.document.view_notes(&plan).unwrap()[0].text, "Kitchen");
        // without a tool: a press on it picks it and a drag moves it
        let _ = app.update(Message::NoteTool(false));
        let marks = layer(&app);
        let mut state = MarksState::default();
        let on_it = on_screen(&app, [2.0, 3.0]) + iced::Vector::new(6.0, -5.0);
        let pick = message(press(&marks, &mut state, on_it));
        assert!(matches!(pick, Some(Message::PickMark(Some(MarkPick::Note(0))))), "{pick:?}");
        let _ = app.update(pick.unwrap());
        assert_eq!(app.note_text, "Kitchen", "its wording to edit");
        let moved = iced::Event::Mouse(mouse::Event::CursorMoved { position: Point::ORIGIN });
        let to = on_screen(&app, [4.0, 1.0]) + iced::Vector::new(6.0, -5.0);
        let drag = message(canvas::Program::update(&marks, &mut state, &moved, bounds(), mouse::Cursor::Available(to))).expect("a move");
        let _ = app.update(drag);
        let note = &app.document.view_notes(&plan).unwrap()[0];
        assert!((note.at[0] - 4.0).abs() < 1e-3 && (note.at[1] - 1.0).abs() < 1e-3, "{note:?}");
        let _ = app.update(Message::NoteText("Kitchen 12 m2".into()));
        assert_eq!(app.document.view_notes(&plan).unwrap()[0].text, "Kitchen 12 m2");
        // a press beside it lets go of it without taking the click from the model
        let marks = layer(&app);
        let beside = press(&marks, &mut MarksState::default(), on_screen(&app, [7.0, 4.5]));
        let (beside, _, status) = beside.expect("lets go").into_inner();
        assert!(matches!(beside, Some(Message::PickMark(None))));
        assert_eq!(status, iced::event::Status::Ignored);
        // and a sheet the plan is on shows it
        let _ = app.update(Message::NewSheet("Sheet"));
        let sheet = Arc::<str>::from(app.document.sheets()[0].path.as_str());
        let _ = app.update(Message::PlaceOnSheet(sheet, plan.clone()));
        assert_eq!(app.marks_layer(app.theme.palette(), Language::English).map(|m| m.notes.len()), None, "no tab over the sheet");
    }
}
