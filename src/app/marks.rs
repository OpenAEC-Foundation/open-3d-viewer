//! Dimensions and texts on a view, in its own tab: a layer over the viewport draws the view's
//! dimensions and texts where they are in the model, seen by the tab's camera, and with the
//! tools of the bar above it adds them: two clicks for a dimension (snapping to the ends of the
//! view's lines), one for a text. A click picks one, a picked text is dragged to move it, and
//! Delete removes what is picked. They belong to the view, so every sheet it is placed on shows
//! them too.

use super::{App, Message};
use crate::document::{Align, Dimension, DimensionStyle, DimensionUnit, Note, Tick};
use crate::drawing::Cut;
use crate::measure::hover::Screen;
use crate::theme::Palette;
use glam::{Vec2, Vec3};
use iced::widget::canvas::{self, Frame, Path, Stroke};
use iced::widget::{button, container, text, text_input, Row};
use iced::{alignment, keyboard, mouse, Center, Color, Element, Fill, Point, Rectangle, Size};
use openaec_ui::Language;
use std::sync::Arc;

/// How close the pointer must come to the end of a line to snap to it (logical pixels).
const SNAP: f32 = 8.0;
/// How close a click must come to a dimension line or a text to pick it (logical pixels).
const PICK: f32 = 5.0;
/// How far a dimension line stands off what it measures, on paper (metres): a view's tab draws
/// its marks at its scale, so they keep their size on the drawing as the view zooms.
pub(super) const OFFSET: f64 = 0.006;
/// The height of a dimension's figures, and of a text being placed, on paper (metres).
const NOTE_SIZE: f64 = crate::document::NOTE_HEIGHT;
/// The smallest a text is drawn (logical pixels).
const SMALLEST: f32 = 1.0;
/// How long the arrows of a section's mark are on a floor plan's tab (logical pixels).
const SECTION_ARROW: f32 = 14.0;

/// What can be picked on the view shown in its tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkPick {
    /// A dimension of the view (its index).
    Dimension(usize),
    /// A text of the view (its index).
    Note(usize),
}

/// A change to the look of the picked dimension, from the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookEdit {
    /// The dimension takes the document's next style.
    NextStyle,
    /// Its style marks the ends of the line the next way.
    NextTick,
    /// Its style writes the figure in the other unit.
    NextUnit,
    /// Its style's figures half a millimetre smaller, or larger.
    Smaller,
    Larger,
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
            let cut = self.document.cut_height(plan)?;
            return Some(DrawingFrame { origin: Vec3::new(0.0, 0.0, cut as f32), u: Vec3::X, v: Vec3::Y });
        }
        let s = self.document.section(path)?;
        let (along, _) = s.axes();
        Some(DrawingFrame { origin: Vec3::new(s.from[0] as f32, s.from[1] as f32, 0.0), u: Vec3::new(along[0] as f32, along[1] as f32, 0.0), v: Vec3::Z })
    }

    /// The drawing view (a plan, section or elevation) whose tab shows in the workspace, no sheet
    /// over it; a 3D view's tab shows as the 3D view does.
    pub(super) fn shown_view(&self) -> Option<&str> {
        (self.active_doc_sheet.is_none() && self.active_sheet.is_none() && self.active_draft.is_none()).then_some(())?;
        self.active_plan.as_deref().filter(|p| self.document.view_3d(p).is_none())
    }

    /// Whether a floor plan, section or sheet shows, to dimension and write on; and whether
    /// sections can be drawn there (a floor plan or a sheet).
    pub(crate) fn annotatable(&self) -> (bool, bool) {
        let sheet = self.active_doc_sheet.is_some();
        let view = self.shown_view();
        (sheet || view.is_some(), sheet || view.is_some_and(|v| self.document.plan(v).is_some()))
    }

    /// The drawing view (a plan, section or elevation) whose tab shows, no sheet over it.
    pub(crate) fn shown_drawing(&self) -> Option<Arc<str>> {
        self.shown_view().map(Arc::from)
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
        let (dimensions, crop) = self.document.view_marks(view)?;
        // sections are drawn on floor plans, and shown there
        let is_plan = self.document.plan(view).is_some();
        Some(ViewMarks {
            view: Arc::from(view),
            frame,
            camera: self.camera,
            scale: self.document.view_scale(view).unwrap_or(crate::document::SHEET_SCALE),
            styles: self.document.dimension_styles().to_vec(),
            dimensions: dimensions.to_vec(),
            notes: self.document.view_notes(view)?.to_vec(),
            lines: self.document.view_lines(view).to_vec(),
            cut: self.cuts.get(view).cloned(),
            crop,
            measuring: self.dimension_tool,
            noting: self.note_tool.then(|| self.note_to_place(l)),
            cutting: self.section_tool && is_plan,
            sections: if is_plan { self.document.sections().iter().map(|s| (s.name.clone(), s.from, s.to)).collect() } else { Vec::new() },
            word: l.text("Doorsnede", "Section"),
            pick: self.mark_pick,
            palette: p,
        })
    }

    /// The bar over a view's tab: its tools, then what goes with the tool on or what is picked.
    pub(super) fn view_bar(&self, p: Palette, l: Language) -> Option<Element<'_, Message>> {
        let view = self.shown_view()?;
        let (measuring, noting) = (self.dimension_tool, self.note_tool);
        let is_plan = self.document.plan(view).is_some();
        let cutting = self.section_tool && is_plan;
        let tool = |label: &'static str, on: bool, message: Message| {
            button(text(label).size(12)).padding([3, 10]).style(move |_, s| p.ribbon_button(on, s)).on_press(message)
        };
        let hint = |line: &'static str| text(line).size(12).color(p.text_secondary);
        let mut bar = Row::new()
            .spacing(6)
            .align_y(Center)
            .push(tool(l.text("Maatvoering", "Dimension"), measuring, Message::DimensionTool(!measuring)))
            .push(tool(l.text("Tekst", "Text"), noting, Message::NoteTool(!noting)));
        if is_plan {
            bar = bar.push(tool(l.text("Doorsnede", "Section"), cutting, Message::SectionTool(!cutting)));
        }
        bar = bar
            .push(tool(l.text("Standaard maatvoering", "Standard dimensions"), false, Message::StandardDimensions(Arc::from(view))))
            .push(self.export_control(p, l))
            .push(text("|").size(12).color(p.text_muted));
        let wording = || text_input(l.text("Tekst", "Text"), &self.note_text).on_input(Message::NoteText).size(12).padding([3, 6]).width(220);
        let remove = |label: &'static str| tool(label, false, Message::RemoveMark);
        bar = if measuring {
            bar.push(hint(l.text("Klik twee punten; ze springen naar de uiteinden van de lijnen. Esc breekt af.", "Click two points; they snap to the ends of the lines. Esc cancels.")))
        } else if cutting {
            bar.push(hint(l.text(
                "Klik het begin en eind van de doorsnede op een plattegrond; hij kijkt naar links.",
                "Click where the section starts and ends on a floor plan; it looks to the left.",
            )))
        } else if noting {
            bar.push(wording()).push(hint(l.text("Klik waar de tekst komt.", "Click where the text goes.")))
        } else {
            match self.mark_pick {
                Some(MarkPick::Note(_)) => bar.push(wording()).push(remove(l.text("Tekst verwijderen", "Remove text"))),
                Some(MarkPick::Dimension(k)) => {
                    let style = self.document.view_marks(view).and_then(|(d, _)| d.get(k)).map_or(0, |d| d.style);
                    let style = self.document.dimension_style(style);
                    let named = |label: String, edit: LookEdit| button(text(label).size(12)).padding([3, 10]).style(move |_, s| p.ribbon_button(false, s)).on_press(Message::DimensionLook(edit));
                    let tick = match style.tick {
                        Tick::Slash => l.text("Streepje", "Slash"),
                        Tick::Arrow => l.text("Pijl", "Arrow"),
                        Tick::Dot => l.text("Stip", "Dot"),
                        Tick::None => l.text("Geen eindteken", "No end mark"),
                    };
                    bar.push(named(format!("{}: {}", l.text("Stijl", "Style"), style.name), LookEdit::NextStyle))
                        .push(named(tick.to_string(), LookEdit::NextTick))
                        .push(named(style.unit.key().to_string(), LookEdit::NextUnit))
                        .push(named("A−".to_string(), LookEdit::Smaller))
                        .push(text(format!("{:.1} mm", style.size * 1000.0)).size(12).color(p.text_secondary))
                        .push(named("A+".to_string(), LookEdit::Larger))
                        .push(tool(l.text("Andere kant", "Other side"), false, Message::FlipDimension))
                        .push(remove(l.text("Maat verwijderen", "Remove dimension")))
                }
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

    /// Gives a view its standard dimensions, outside its drawing: a floor plan its overall width
    /// below and depth on the left, a section or elevation its overall width below and height on
    /// the left. Where grids (a plan) or grids and levels (a section) divide a side, a chain
    /// between them goes on first and the overall dimension 1 m further out. A dimension the view
    /// has already is not made twice. Gives how many were added.
    pub(super) fn standard_dimensions(&mut self, view: &str) -> usize {
        self.cut_view(view);
        let Some((lo, hi)) = self.cuts.get(view).and_then(|c| c.extent) else { return 0 };
        let (lo, hi) = ([lo[0] as f64, lo[1] as f64], [hi[0] as f64, hi[1] as f64]);
        let grids = self.model.as_ref().map_or(&[][..], |m| &m.project.grids[..]);
        // where grids (and levels) divide the drawing along u and along v
        let (along_u, along_v): (Vec<f64>, Vec<f64>) = if self.document.plan(view).is_some() {
            let mut xs = Vec::new();
            let mut ys = Vec::new();
            for g in grids {
                let (dx, dy) = ((g.b[0] - g.a[0]) as f64, (g.b[1] - g.a[1]) as f64);
                let run = dx.hypot(dy);
                if run < 1e-6 {
                    continue;
                }
                if dx.abs() / run < 0.02 {
                    xs.push((g.a[0] + g.b[0]) as f64 / 2.0);
                } else if dy.abs() / run < 0.02 {
                    ys.push((g.a[1] + g.b[1]) as f64 / 2.0);
                }
            }
            (xs, ys)
        } else if let Some(s) = self.document.section(view) {
            let (along, _) = s.axes();
            let us = grids.iter().filter_map(|g| {
                // where the grid crosses the section's line, as u along it
                let (p, r) = ([g.a[0] as f64, g.a[1] as f64], [(g.b[0] - g.a[0]) as f64, (g.b[1] - g.a[1]) as f64]);
                let cross = along[0] * r[1] - along[1] * r[0];
                if cross.abs() < 1e-9 {
                    return None;
                }
                let w = [p[0] - s.from[0], p[1] - s.from[1]];
                let t = (w[0] * along[1] - w[1] * along[0]) / cross;
                (-1e-6..=1.0 + 1e-6).contains(&t).then(|| (w[0] + t * r[0]) * along[0] + (w[1] + t * r[1]) * along[1])
            });
            (us.collect(), self.document.levels().iter().map(|l| l.elevation).collect())
        } else {
            return 0;
        };
        // the divisions within a side (with its ends); none when they are only its ends
        let chain = |mut at: Vec<f64>, from: f64, to: f64| -> Vec<f64> {
            at.retain(|v| (from - 0.5..=to + 0.5).contains(v));
            at.extend([from, to]);
            at.sort_by(f64::total_cmp);
            at.dedup_by(|a, b| (*a - *b).abs() < 0.001);
            if at.len() <= 2 { Vec::new() } else { at }
        };
        let (below, left) = (chain(along_u, lo[0], hi[0]), chain(along_v, lo[1], hi[1]));
        let mut made = Vec::new();
        // below: right to left, so the line stands under what it measures
        for pair in below.windows(2) {
            made.push(Dimension { from: [pair[1], lo[1]], to: [pair[0], lo[1]], style: 0 });
        }
        let out = if below.is_empty() { 0.0 } else { 1.0 };
        made.push(Dimension { from: [hi[0], lo[1] - out], to: [lo[0], lo[1] - out], style: 0 });
        // on the left: upwards, so the line stands left of what it measures
        for pair in left.windows(2) {
            made.push(Dimension { from: [lo[0], pair[0]], to: [lo[0], pair[1]], style: 0 });
        }
        let out = if left.is_empty() { 0.0 } else { 1.0 };
        made.push(Dimension { from: [lo[0] - out, lo[1]], to: [lo[0] - out, hi[1]], style: 0 });
        let same = |a: &Dimension, b: &Dimension| {
            let close = |p: [f64; 2], q: [f64; 2]| (p[0] - q[0]).abs() < 0.001 && (p[1] - q[1]).abs() < 0.001;
            (close(a.from, b.from) && close(a.to, b.to)) || (close(a.from, b.to) && close(a.to, b.from))
        };
        let mut added = 0;
        for d in made {
            let has = self.document.view_marks(view).is_some_and(|(own, _)| own.iter().any(|o| same(o, &d)));
            if !has && self.document.add_dimension(view, d.from, d.to) {
                added += 1;
            }
        }
        added
    }

    /// Puts the picked dimension on the other side of what it measures (on the view shown, or on
    /// the sheet shown).
    pub(super) fn flip_dimension(&mut self) {
        let picked = match (self.shown_view(), self.mark_pick, self.sheet_pick) {
            (Some(view), Some(MarkPick::Dimension(k)), _) => Some((view.to_string(), k)),
            (None, _, Some(super::paper::SheetPick::Dimension(i, k))) => self.placed_plan(i).map(|v| (v, k)),
            _ => None,
        };
        if let Some((view, k)) = picked {
            self.document.flip_dimension(&view, k);
        }
    }

    /// The dimension picked on the view shown, or on the sheet shown: its view and index.
    fn picked_dimension(&self) -> Option<(String, usize)> {
        match (self.shown_view(), self.mark_pick, self.sheet_pick) {
            (Some(view), Some(MarkPick::Dimension(k)), _) => Some((view.to_string(), k)),
            (None, _, Some(super::paper::SheetPick::Dimension(i, k))) => self.placed_plan(i).map(|v| (v, k)),
            _ => None,
        }
    }

    /// Changes the look of the picked dimension: the style it has, or that style itself (and so
    /// every dimension of it).
    pub(super) fn edit_dimension_look(&mut self, edit: LookEdit) {
        let Some((view, k)) = self.picked_dimension() else { return };
        let Some(style) = self.document.view_marks(&view).and_then(|(d, _)| d.get(k)).map(|d| d.style) else { return };
        match edit {
            LookEdit::NextStyle => {
                let next = (style + 1) % self.document.dimension_styles().len().max(1);
                self.document.set_dimension_style_of(&view, k, next);
            }
            LookEdit::NextTick => {
                self.document.edit_dimension_style(style, |s| {
                    let at = Tick::ALL.iter().position(|t| *t == s.tick).unwrap_or(0);
                    s.tick = Tick::ALL[(at + 1) % Tick::ALL.len()];
                });
            }
            LookEdit::NextUnit => {
                self.document.edit_dimension_style(style, |s| {
                    (s.unit, s.decimals) = match s.unit {
                        DimensionUnit::Millimetres => (DimensionUnit::Metres, 3),
                        DimensionUnit::Metres => (DimensionUnit::Millimetres, 0),
                    };
                });
            }
            LookEdit::Smaller | LookEdit::Larger => {
                let step = if edit == LookEdit::Larger { 0.0005 } else { -0.0005 };
                self.document.edit_dimension_style(style, |s| s.size = ((s.size + step) * 2000.0).round().clamp(2.0, 40.0) / 2000.0);
            }
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
    /// The tab's camera (its aspect is the layer's own, so it is right also before the
    /// viewport reported its size).
    camera: crate::viewport::camera::Camera,
    /// The view's scale (1 : this): what its texts and dimensions are sized for.
    scale: f64,
    /// The document's dimension styles: a dimension names one by its place.
    styles: Vec<DimensionStyle>,
    dimensions: Vec<Dimension>,
    notes: Vec<Note>,
    /// The lines of the view's tags: their shapes and leaders.
    lines: Vec<[[f64; 2]; 2]>,
    /// The view's lines, to snap to.
    cut: Option<Arc<Cut>>,
    /// The view's crop: its cut faces are drawn within it.
    crop: Option<([f64; 2], [f64; 2])>,
    /// The dimension tool is on.
    measuring: bool,
    /// The text tool is on, with the text a click places.
    noting: Option<String>,
    /// The section tool is on (a floor plan): two clicks draw a section.
    cutting: bool,
    /// The sections, drawn on a floor plan: name, line (plan).
    sections: Vec<(String, [f64; 2], [f64; 2])>,
    /// What a new section is called before its letter.
    word: &'static str,
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
        Screen { view_proj: self.camera.view_proj(bounds.width / bounds.height.max(1.0)), size: (bounds.width, bounds.height) }
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

    /// How many logical pixels a metre of paper is at drawing point `q`: the view's scale times
    /// what a metre of the model is on screen there.
    fn paper(&self, screen: &Screen, q: [f64; 2]) -> f32 {
        match (self.px(screen, q), self.px(screen, [q[0] + 1.0, q[1]])) {
            (Some(a), Some(b)) => a.distance(b) * self.scale as f32,
            _ => 0.0,
        }
    }

    /// The size of a text of `height` on paper standing on `q` (logical pixels).
    fn note_size(&self, screen: &Screen, q: [f64; 2], height: f64) -> f32 {
        (self.paper(screen, q) * height as f32).max(SMALLEST)
    }

    /// The sizes of a dimension from `q` as drawn.
    fn look(&self, screen: &Screen, q: [f64; 2], style: usize) -> DimensionLook<'_> {
        let paper = self.paper(screen, q);
        let style = self.styles.get(style).unwrap_or(&self.styles[0]);
        DimensionLook { offset: paper * OFFSET as f32, paper, size: (paper * style.size as f32).max(SMALLEST), style }
    }

    /// The ends of dimension `k`'s line on screen, [`OFFSET`] on paper to the left of what it
    /// measures.
    fn dimension_line(&self, screen: &Screen, k: usize) -> Option<(Point, Point)> {
        let d = self.dimensions.get(k)?;
        let offset = self.look(screen, d.from, d.style).offset;
        let (a, b) = (self.px(screen, d.from)?, self.px(screen, d.to)?);
        let run = a.distance(b);
        (run > 0.0).then(|| {
            let n = iced::Vector::new((b.y - a.y) / run * offset, -(b.x - a.x) / run * offset);
            (a + n, b + n)
        })
    }

    /// Where text `k` is on screen: from its point to the right and up.
    fn note_box(&self, screen: &Screen, k: usize) -> Option<Rectangle> {
        let note = self.notes.get(k)?;
        let at = self.px(screen, note.at)?;
        let size = self.note_size(screen, note.at, note.size);
        let lines = note.text.lines().count().max(1) as f32;
        let width = note.text.lines().map(|l| l.chars().count()).max().unwrap_or(0) as f32 * size * 0.56;
        let left = match note.align {
            Align::Left => at.x,
            Align::Center => at.x - width / 2.0,
            Align::Right => at.x - width,
        };
        Some(Rectangle::new(Point::new(left, at.y - size * 1.2 * lines), Size::new(width, size * 1.2 * lines)))
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
        self.measuring || self.cutting || self.noting.is_some()
    }

    /// The tools of two clicks (dimension, section), which snap to the view's lines.
    fn two_clicks(&self) -> bool {
        self.measuring || self.cutting
    }

    /// A section's mark on the floor plan, as a sheet draws it: its line dash-dotted, at each end
    /// an arrow the way it looks (to the left of `from` -> `to`) and its letter past it.
    fn draw_section(&self, frame: &mut Frame, screen: &Screen, from: [f64; 2], to: [f64; 2], letter: String, color: Color) {
        let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
        let run = dx.hypot(dy);
        let (Some(a), Some(b)) = (self.px(screen, from), self.px(screen, to)) else { return };
        if run <= 0.0 {
            return;
        }
        let mut line = Stroke::default().with_color(color).with_width(1.0);
        line.line_dash = canvas::LineDash { segments: &[10.0, 3.0, 2.0, 3.0], offset: 0 };
        frame.stroke(&Path::line(a, b), line);
        let look = [-dy / run, dx / run];
        for (end, at) in [(from, a), (to, b)] {
            // the way it looks, on screen
            let Some(ahead) = self.px(screen, [end[0] + look[0], end[1] + look[1]]) else { continue };
            let d = ahead - at;
            let length = (d.x * d.x + d.y * d.y).sqrt().max(1e-3);
            let unit = iced::Vector::new(d.x / length, d.y / length);
            frame.stroke(&Path::line(at, at + unit * SECTION_ARROW), Stroke::default().with_color(color).with_width(1.6));
            if !letter.is_empty() {
                frame.fill_text(canvas::Text {
                    content: letter.clone(),
                    position: at + unit * (SECTION_ARROW * 1.8),
                    align_x: alignment::Horizontal::Center.into(),
                    align_y: alignment::Vertical::Center,
                    size: iced::Pixels(12.0),
                    color,
                    font: openaec_ui::fonts::MEDIUM,
                    ..canvas::Text::default()
                });
            }
        }
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
        if !self.two_clicks() {
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
                    state.aim = pointer.and_then(|p| if self.two_clicks() { self.snap(&screen, p) } else { self.at(&screen, p) });
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
                if self.two_clicks() {
                    let q = self.snap(&screen, p)?;
                    return match state.first.take() {
                        None => {
                            state.first = Some(q);
                            Some(canvas::Action::request_redraw().and_capture())
                        }
                        Some(from) if self.cutting => Some(canvas::Action::publish(Message::NewSection(from, q, self.word)).and_capture()),
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
                if self.cutting {
                    return Some(canvas::Action::publish(Message::SectionTool(false)).and_capture());
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
        // the cut faces in their hatch, over the model's own cut fill
        if let Some(cut) = &self.cut {
            let per_metre = cut.extent.map_or(0.0, |(lo, _)| self.paper(&screen, lo.map(f64::from)) / self.scale as f32);
            super::hatch::draw_fills(&mut frame, cut, self.crop, &|q| self.px(&screen, q), per_metre, Some(ink));
        }
        for (name, from, to) in &self.sections {
            let letter = name.rsplit(' ').next().unwrap_or(name).to_string();
            self.draw_section(&mut frame, &screen, *from, *to, letter, ink);
        }
        for (k, d) in self.dimensions.iter().enumerate() {
            let (Some(a), Some(b)) = (self.px(&screen, d.from), self.px(&screen, d.to)) else { continue };
            let color = if self.pick == Some(MarkPick::Dimension(k)) { p.accent } else { ink };
            draw_dimension_px(&mut frame, a, b, d.length(), color, self.look(&screen, d.from, d.style));
        }
        if !self.lines.is_empty() {
            let lines = Path::new(|path| {
                for line in &self.lines {
                    let (Some(a), Some(b)) = (self.px(&screen, line[0]), self.px(&screen, line[1])) else { continue };
                    path.move_to(a);
                    path.line_to(b);
                }
            });
            frame.stroke(&lines, Stroke::default().with_color(ink).with_width(1.0));
        }
        for (k, note) in self.notes.iter().enumerate() {
            let Some(at) = self.px(&screen, note.at) else { continue };
            let picked = self.pick == Some(MarkPick::Note(k));
            draw_note_in(&mut frame, at, &note.text, self.note_size(&screen, note.at, note.size), if picked { p.accent } else { ink }, note.align, note.angle as f32, &note.face);
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
                (Some(wording), _) => draw_note(&mut frame, c, wording, self.note_size(&screen, aim, NOTE_SIZE), Color { a: 0.6, ..p.accent }, Align::Left, 0.0),
                (None, Some(from)) if self.cutting => self.draw_section(&mut frame, &screen, from, aim, String::new(), p.accent),
                (None, Some(from)) => {
                    if let Some(a) = self.px(&screen, from) {
                        let length = Dimension { from, to: aim, style: 0 }.length();
                        draw_dimension_px(&mut frame, a, c, length, p.accent, self.look(&screen, from, 0));
                    }
                }
                (None, None) => {}
            }
            if self.two_clicks() {
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

/// How a dimension is drawn: how far its line stands off what it measures and the height of its
/// figure (logical pixels), what a metre of paper is on screen, and its style.
pub(super) struct DimensionLook<'a> {
    pub offset: f32,
    pub paper: f32,
    pub size: f32,
    pub style: &'a DimensionStyle,
}

/// A dimension between two screen points: the dimension line `look.offset` to the left of
/// a → b (as seen with y up), extension lines from just off the points to just past it, its
/// style's mark at each end, and the length in its style's unit over the middle.
pub(super) fn draw_dimension_px(frame: &mut Frame, a: Point, b: Point, length: f64, color: Color, look: DimensionLook) {
    let run = a.distance(b);
    if run <= 0.0 {
        return;
    }
    let style = look.style;
    let (tick, gap, extension) = (look.paper * style.tick_size as f32, look.paper * style.gap as f32, look.paper * style.extension as f32);
    let (ux, uy) = ((b.x - a.x) / run, (b.y - a.y) / run);
    let (nx, ny) = (uy, -ux);
    let off = |q: Point, d: f32| Point::new(q.x + nx * d, q.y + ny * d);
    let (a2, b2) = (off(a, look.offset), off(b, look.offset));
    let lines = Path::new(|path| {
        for q in [a, b] {
            path.move_to(off(q, gap.min(look.offset)));
            path.line_to(off(q, look.offset + extension));
        }
        path.move_to(a2);
        path.line_to(b2);
        if style.tick == Tick::Slash {
            let (sx, sy) = ((ux + nx) * tick, (uy + ny) * tick);
            for q in [a2, b2] {
                path.move_to(Point::new(q.x - sx, q.y - sy));
                path.line_to(Point::new(q.x + sx, q.y + sy));
            }
        }
    });
    frame.stroke(&lines, Stroke::default().with_color(color).with_width(0.9));
    match style.tick {
        Tick::Arrow => {
            // the point on the end of the line, the head lying on the line
            for (q, inward) in [(a2, 1.0), (b2, -1.0)] {
                let back = Point::new(q.x + ux * tick * inward, q.y + uy * tick * inward);
                let head = Path::new(|path| {
                    path.move_to(q);
                    path.line_to(off(back, tick * 0.27));
                    path.line_to(off(back, -tick * 0.27));
                    path.close();
                });
                frame.fill(&head, color);
            }
        }
        Tick::Dot => {
            for q in [a2, b2] {
                frame.fill(&Path::circle(q, (tick / 2.0).max(1.0)), color);
            }
        }
        Tick::Slash | Tick::None => {}
    }
    // the length beside the line, clear of it whichever way the line runs
    let content = style.figure(length);
    let (w, h) = (content.chars().count() as f32 * look.size * 0.6, look.size);
    let clear = nx.abs() * w / 2.0 + ny.abs() * h / 2.0 + 2.0;
    frame.fill_text(canvas::Text {
        content,
        position: off(Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0), look.offset + clear),
        align_x: alignment::Horizontal::Center.into(),
        align_y: alignment::Vertical::Center,
        size: iced::Pixels(look.size),
        color,
        font: openaec_ui::fonts::MEDIUM,
        ..canvas::Text::default()
    });
}

/// A text standing on a screen point: its lower left, the middle of its foot or its lower right,
/// turned about it by `angle` (radians, counter-clockwise).
pub(super) fn draw_note(frame: &mut Frame, at: Point, wording: &str, size: f32, color: Color, align: Align, angle: f32) {
    draw_note_in(frame, at, wording, size, color, align, angle, &crate::typeface::Face::default());
}

/// A text as [`draw_note`] draws it, set in the typeface its file names.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_note_in(frame: &mut Frame, at: Point, wording: &str, size: f32, color: Color, align: Align, angle: f32, face: &crate::typeface::Face) {
    let align_x = match align {
        Align::Left => alignment::Horizontal::Left,
        Align::Center => alignment::Horizontal::Center,
        Align::Right => alignment::Horizontal::Right,
    };
    let text = |position: Point| canvas::Text {
        content: wording.to_string(),
        position,
        align_x: align_x.into(),
        align_y: alignment::Vertical::Bottom,
        size: iced::Pixels(size),
        color,
        ..canvas::Text::default()
    };
    if angle == 0.0 {
        face.fill(frame, text(at));
        return;
    }
    frame.with_save(|frame| {
        frame.translate(iced::Vector::new(at.x, at.y));
        // the screen's y runs down
        frame.rotate(-angle);
        face.fill(frame, text(Point::ORIGIN));
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
        plan_shown_with(Vec::new())
    }

    /// The same, the model with these grids.
    fn plan_shown_with(grids: Vec<crate::model::GridLine>) -> (App, Arc<str>) {
        let positions = vec![0.,0.,0., 8.,0.,0., 8.,5.,0., 0.,5.,0., 0.,0.,3., 8.,0.,3., 8.,5.,3., 0.,5.,3.];
        let indices = vec![0,1,5, 0,5,4, 1,2,6, 1,6,5, 2,3,7, 2,7,6, 3,0,4, 3,4,7];
        let layer = Layer { name: "Walls".into(), positions, indices, ..Layer::default() };
        let mut model = Model::new(PathBuf::from("room.rvt"), "Revit", vec![layer]);
        model.project.levels = vec![("00".into(), 0.0), ("01".into(), 3.0)];
        model.project.grids = grids;
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
    fn standard_dimensions_give_a_view_its_overall_size_and_its_grids() {
        let (mut app, plan) = plan_shown();
        let lengths = |app: &App, view: &str| -> Vec<f64> {
            app.document.view_marks(view).unwrap().0.iter().map(|d| (d.length() * 1000.0).round()).collect()
        };
        let _ = app.update(Message::StandardDimensions(plan.clone()));
        assert_eq!(lengths(&app, &plan), [8000.0, 5000.0]);
        // below the room and left of it
        let d = app.document.plan(&plan).unwrap().dimensions.clone();
        assert_eq!((d[0].from, d[0].to), ([8.0, 0.0], [0.0, 0.0]), "right to left: its line under the room");
        assert_eq!((d[1].from, d[1].to), ([0.0, 0.0], [0.0, 5.0]), "upwards: its line left of the room");
        let _ = app.update(Message::StandardDimensions(plan.clone()));
        assert_eq!(lengths(&app, &plan).len(), 2, "nothing twice");
        // with grids at x = 0, 3, 8 and y = 0, 5: a chain below, the overall 1 m further out
        let grid = |name: &str, a: [f32; 2], b: [f32; 2]| crate::model::GridLine { name: name.into(), a: [a[0], a[1], 0.0], b: [b[0], b[1], 0.0] };
        let grids = vec![grid("1", [0.0, -1.0], [0.0, 6.0]), grid("2", [3.0, -1.0], [3.0, 6.0]), grid("3", [8.0, -1.0], [8.0, 6.0]), grid("A", [-1.0, 0.0], [9.0, 0.0]), grid("B", [-1.0, 5.0], [9.0, 5.0])];
        let (mut app, plan2) = plan_shown_with(grids);
        let _ = app.update(Message::StandardDimensions(plan2.clone()));
        assert_eq!(lengths(&app, &plan2), [3000.0, 5000.0, 8000.0, 5000.0]);
        let d = &app.document.plan(&plan2).unwrap().dimensions;
        assert_eq!(d[2].from[1], -1.0, "the overall width 1 m below the chain");
        assert_eq!(d[3].from[0], 0.0, "on the left only the overall: the grids are its ends");
        // a section across the room: its width, and its height between the levels
        let section = app.document.add_section("Section A", [-1.0, 2.5], [9.0, 2.5], 20.0).unwrap();
        let _ = app.update(Message::StandardDimensions(Arc::from(section.as_str())));
        let made = lengths(&app, &section);
        assert!(made.contains(&3000.0), "{made:?}");
        assert!(made.contains(&3000.0) && made.iter().any(|&l| l >= 8000.0), "{made:?}");
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
    fn two_clicks_with_the_section_tool_draw_a_section_on_the_floor_plan() {
        let (mut app, plan) = plan_shown();
        let _ = app.update(Message::SectionTool(true));
        assert!(app.cuts.contains_key(&*plan), "cut to snap to");
        let marks = layer(&app);
        assert!(marks.cutting);
        let mut state = MarksState::default();
        let off = iced::Vector::new(3.0, -2.0);
        assert!(message(press(&marks, &mut state, on_screen(&app, [0.0, 0.0]) + off)).is_none());
        let made = message(press(&marks, &mut state, on_screen(&app, [8.0, 0.0]) + off)).expect("a section");
        assert!(matches!(&made, Message::NewSection([0.0, 0.0], [8.0, 0.0], _)), "{made:?}");
        let _ = app.update(made);
        assert_eq!(app.document.sections().len(), 1);
        assert!(!app.section_tool, "one section a time");
        // back on the floor plan, its tab draws the section's line with its letter
        let _ = app.update(Message::OpenPlan(plan.clone()));
        let marks = layer(&app);
        assert_eq!(marks.sections.len(), 1);
        assert_eq!((marks.sections[0].1, marks.sections[0].2), ([0.0, 0.0], [8.0, 0.0]));
        assert!(marks.sections[0].0.ends_with('A'), "{:?}", marks.sections[0]);
    }

    #[test]
    fn a_section_is_not_drawn_on_a_section() {
        let (mut app, _) = plan_shown();
        let _ = app.update(Message::NewSection([0.0, 0.0], [8.0, 0.0], "Section"));
        // the new section shows in its tab: no section tool and no section marks there
        let _ = app.update(Message::SectionTool(true));
        let marks = layer(&app);
        assert!(!marks.cutting && marks.sections.is_empty());
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
