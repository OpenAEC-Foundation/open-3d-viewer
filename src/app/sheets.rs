//! Sheets (Revit, beta): the explorer group, the document tabs and the sheet drawn in the
//! workspace: the paper, the title block's frame and the placed views with their title, scale and
//! detail number. A click on a view shows it in 3D.

use super::{group_header, App, Message};
use crate::document::{Dimension, DimensionStyle, Note, Tick};
use crate::drawing::Cut;
use crate::model::{Model, Sheet, SheetRect};
use std::sync::Arc;
use crate::theme::Palette;
use iced::alignment;
use iced::mouse;
use iced::widget::canvas::{self, Frame, Path, Stroke};
use iced::widget::text::Wrapping;
use iced::widget::{button, canvas as canvas_widget, container, text, Column};
use iced::{Color, Element, Fill, Padding, Point, Rectangle, Size, Task, Vector};
use openaec_ui::Language;
use serde_json::{json, Value};

/// Margin around the sheet in the workspace (logical pixels).
const MARGIN: f32 = 24.0;
/// The smallest letters worth drawing (logical pixels).
const SMALLEST: f32 = 1.5;
/// The letters of a view's title on the sheet, and of the scale under it (metres on paper).
const TITLE: f32 = 0.0035;
const TITLE_SCALE: f32 = 0.0025;

impl App {
    /// The sheets of the model.
    pub(super) fn sheets(&self) -> &[Sheet] {
        self.model.as_ref().map_or(&[], |m| &m.project.sheets)
    }

    /// Opens a sheet in its own tab, or shows its tab when it is open.
    pub(super) fn open_sheet(&mut self, i: usize) -> Task<Message> {
        if !self.open_sheets.contains(&i) {
            self.open_sheets.push(i);
        }
        self.show_sheet(Some(i))
    }

    /// Shows a sheet's tab (`None`: the 3D view, or the schedule that was shown).
    pub(super) fn show_sheet(&mut self, i: Option<usize>) -> Task<Message> {
        self.active_sheet = i;
        if i.is_some() {
            self.active_schedule = None;
            self.ifc.shown = false;
            self.active_doc_sheet = None;
            self.active_draft = None;
        }
        // the plans and sections on it are cut once, to be drawn in their viewports
        let views: Vec<String> = i.and_then(|i| self.sheets().get(i)).into_iter().flat_map(|s| s.viewports.iter().filter_map(|v| v.drawing.clone())).collect();
        // in the window on worker threads, these first: the drawing shows its views as they come
        let ahead = if self.background_cuts {
            self.cut_first(views)
        } else {
            for view in views {
                self.cut_view(&view);
            }
            Task::none()
        };
        // the 3D views on it, in hidden lines
        let threes: Vec<usize> = i.and_then(|i| self.sheets().get(i)).into_iter().flat_map(|s| s.viewports.iter().filter(|v| v.drawing.is_none() && v.draft.is_none()).filter_map(|v| v.view)).collect();
        for view in threes {
            self.cut_view_3d(view);
        }
        ahead
    }

    /// Draws a 3D view of the file in hidden lines when it is not drawn yet: looked at along its
    /// camera's direction, within its section box. A view in perspective is left as its frame.
    pub(super) fn cut_view_3d(&mut self, view: usize) {
        let Some(model) = self.model.clone() else { return };
        let key = view3d_key(view);
        let Some(saved) = model.views.get(view).filter(|v| v.kind == crate::model::ViewKind::ThreeD) else { return };
        let Some(camera) = saved.camera.as_ref().filter(|c| !c.perspective) else { return };
        if self.cuts.contains_key(&key) {
            return;
        }
        let layers = vec![true; model.layers.len()];
        // what the view leaves out, by the unit of a triangle's first vertex
        let mut units: Vec<bool> = Vec::new();
        if let (Some(off), Some(index)) = (model.project.switched_off.get(&key), self.explode_index.as_deref()) {
            units.resize(index.len(), false);
            for unit in 1..index.len() {
                if matches!(index.owner(unit), Some(crate::explode::UnitOwner::Element(id)) if off.contains(&id)) {
                    units[unit] = true;
                }
            }
        }
        let index = self.explode_index.clone().filter(|_| units.iter().any(|u| *u));
        let hidden = |l: usize, v: u32| index.as_ref().is_some_and(|ix| units.get(ix.unit(l, v)).copied().unwrap_or(false));
        let shown = crate::drawing::Shown { layers: &layers, hidden: &hidden, faint: None, pieces: None };
        let cut = crate::drawing::view_at(&model, shown, camera.forward, saved.clip.as_ref());
        self.cuts.insert(key, Arc::new(cut));
    }

    /// Closes a sheet's tab; when it was shown, the sheet before it (or the 3D view) shows.
    pub(super) fn close_sheet(&mut self, i: usize) -> Task<Message> {
        let Some(k) = self.open_sheets.iter().position(|&s| s == i) else { return Task::none() };
        self.open_sheets.remove(k);
        if self.active_sheet != Some(i) {
            return Task::none();
        }
        let next = k.checked_sub(1).or((!self.open_sheets.is_empty()).then_some(0)).map(|k| self.open_sheets[k]);
        self.show_sheet(next)
    }

    /// A view on a sheet clicked: the 3D view shows it.
    pub(super) fn sheet_view(&mut self, view: usize) -> Task<Message> {
        self.active_sheet = None; self.active_draft = None;
        self.apply_view(view);
        Task::none()
    }

    /// The explorer group of the sheets, by number (Revit's "Sheets").
    pub(super) fn sheets_group<'a>(&self, p: Palette, l: Language, query: &str, of: std::ops::Range<usize>, mut body: Column<'a, Message>) -> Column<'a, Message> {
        let sheets = self.sheets();
        let of = of.start.min(sheets.len())..of.end.min(sheets.len());
        if of.is_empty() {
            return body;
        }
        let shown: Vec<usize> = of.filter(|&i| query.is_empty() || sheets[i].title.to_lowercase().contains(query)).collect();
        let open = self.explorer_open[4] || !query.is_empty();
        body = body.push(group_header(p, l.text("Tekeningen", "Drawings"), Some("beta"), shown.len(), open, Message::ToggleExplorer(4)));
        if !open {
            return body;
        }
        for i in shown {
            let active = self.active_sheet == Some(i);
            // sheets without placed views are muted
            let color = if active { p.accent } else if sheets[i].viewports.is_empty() { p.text_muted } else { p.text };
            let item = button(text(sheets[i].title.clone()).size(12).color(color).wrapping(Wrapping::WordOrGlyph).width(Fill))
                .padding([3, 6])
                .width(Fill)
                .style(move |_, s| if active { p.ribbon_button(true, s) } else { p.row_button(s) })
                .on_press(Message::OpenSheet(i));
            body = body.push(container(item).padding(Padding { left: 14.0, ..Padding::ZERO }).width(Fill));
        }
        body
    }

    /// The shown sheet, drawn to fit the workspace.
    pub(super) fn sheet_view_element(&self, p: Palette, l: Language, sheet: &Sheet) -> Element<'_, Message> {
        let empty = l.text("Geen geplaatste views", "No placed views").to_string();
        let (Some(model), Some(index)) = (self.model.clone(), self.active_sheet) else {
            return container(text(empty).size(13).color(p.text_muted)).center(Fill).style(move |_| p.panel()).into();
        };
        // what each viewport shows: a legend or drafting view, or a view of the document
        let placed = sheet
            .viewports
            .iter()
            .map(|v| match (v.draft, v.drawing.as_deref().and_then(|path| Some((path, self.document.view_marks(path)?)))) {
                (Some(k), _) => Placed::Draft(k),
                (None, Some((path, (dimensions, crop)))) => Placed::View {
                    cut: self.cuts.get(path).cloned(),
                    crop,
                    dimensions: dimensions.to_vec(),
                    notes: self.document.view_notes(path).map_or(Vec::new(), <[_]>::to_vec),
                    lines: self.document.view_lines(path).to_vec(),
                    fills: self.document.view_fills(path).to_vec(),
                },
                // a 3D view: its hidden lines, once they are made
                (None, None) => match v.view.and_then(|i| self.cuts.get(&view3d_key(i))) {
                    Some(cut) => Placed::View { cut: Some(cut.clone()), crop: None, dimensions: Vec::new(), notes: Vec::new(), lines: Vec::new(), fills: Vec::new() },
                    None => Placed::Frame,
                },
            })
            .collect();
        // the schedules on it, with what the computed ones add to what the file stores
        let tables = sheet.tables.iter().map(|t| t.filled(self.schedules.iter().find(|s| s.name == t.schedule))).collect();
        let drawing = SheetDrawing { model, sheet: index, placed, tables, styles: self.document.dimension_styles().to_vec(), palette: p, empty };
        container(canvas_widget(drawing).width(Fill).height(Fill)).width(Fill).height(Fill).style(move |_| p.panel()).into()
    }
}

// ---- remote control ----

impl App {
    /// Remote `list_sheets`: every sheet with its number, name and placed views.
    pub(crate) fn sheets_json(&self) -> Result<Value, String> {
        self.model.as_ref().ok_or("no model is open: call open_file first")?;
        let list: Vec<Value> = self
            .sheets()
            .iter()
            .enumerate()
            .map(|(i, s)| {
                json!({
                    "index": i,
                    "number": s.number,
                    "name": s.name,
                    "views": s.viewports.iter().map(|v| v.title.clone()).collect::<Vec<_>>(),
                    "shown": self.active_sheet == Some(i),
                })
            })
            .collect();
        Ok(json!({ "count": list.len(), "sheets": list }))
    }

    /// Remote `get_sheet`: a sheet's paper and placed views (sheet millimetres, x right, y up;
    /// `view` is the index for apply_view); with `show`, it opens in its document tab.
    pub(crate) fn remote_sheet(&mut self, a: &Value) -> (Result<Value, String>, Task<Message>) {
        let i = match self.find_sheet(a.get("sheet")) {
            Ok(i) => i,
            Err(e) => return (Err(e), Task::none()),
        };
        let s = &self.sheets()[i];
        let mm = |r: &SheetRect| json!({ "min": r.min.map(|v| (v * 1000.0).round()), "max": r.max.map(|v| (v * 1000.0).round()) });
        let answer = json!({
            "index": i,
            "number": s.number,
            "name": s.name,
            "paper_mm": s.paper.as_ref().map(mm),
            "views": s.viewports.iter().map(|v| json!({
                "title": v.title,
                "view": v.view,
                "detail_number": v.detail,
                "scale": v.scale,
                "box_mm": mm(&v.rect),
                "box_estimated": v.estimated,
            })).collect::<Vec<_>>(),
            "note": "beta: the boxes of the placed views and the paper; the drawing itself (lines, text, title block) is not read yet",
        });
        let show = a.get("show").and_then(Value::as_bool).unwrap_or(false);
        (Ok(answer), if show { self.open_sheet(i) } else { Task::none() })
    }

    /// A sheet by index, number or name (exact, else the first containing it, ignoring case).
    fn find_sheet(&self, spec: Option<&Value>) -> Result<usize, String> {
        self.model.as_ref().ok_or("no model is open: call open_file first")?;
        let spec = spec.ok_or("sheet (index, number or name) is required")?;
        let sheets = self.sheets();
        if let Some(i) = spec.as_u64() {
            return (i < sheets.len() as u64).then_some(i as usize).ok_or(format!("no sheet {i}"));
        }
        let want = spec.as_str().ok_or("sheet must be an index, a number or a name")?.to_lowercase();
        let exact = |s: &Sheet| [&s.number, &s.name, &s.title].iter().any(|t| t.to_lowercase() == want);
        sheets
            .iter()
            .position(exact)
            .or_else(|| sheets.iter().position(|s| s.title.to_lowercase().contains(&want)))
            .ok_or(format!("no sheet '{want}'"))
    }
}

use crate::model::view3d_key;

/// What a placed view shows on its sheet.
enum Placed {
    /// Its frame only: a view the viewer cannot draw.
    Frame,
    /// A legend or drafting view of the model.
    Draft(usize),
    /// A plan, section or elevation of the document: what it cuts and sees, its dimensions, texts
    /// and the lines of its tags, the regions it fills in one colour, and what it shows of its
    /// drawing (lower left, upper right).
    View { cut: Option<Arc<Cut>>, crop: Option<([f64; 2], [f64; 2])>, dimensions: Vec<Dimension>, notes: Vec<Note>, lines: Vec<[[f64; 2]; 2]>, fills: Vec<crate::document::Fill> },
}

/// A sheet on a canvas.
struct SheetDrawing {
    model: Arc<Model>,
    sheet: usize,
    /// What each of its viewports shows.
    placed: Vec<Placed>,
    /// The schedules placed on it.
    tables: Vec<crate::model::SheetTable>,
    /// The document's dimension styles: a dimension names one by its place.
    styles: Vec<DimensionStyle>,
    palette: Palette,
    empty: String,
}

/// The mapping from sheet metres to canvas pixels.
struct Fit {
    scale: f32,
    origin: Point,
    top: f32,
}

/// The hovered viewport, and how the pointer moved the sheet: zoomed in by `zoom` about the fit,
/// moved by `pan`.
struct DrawingState {
    hover: Option<usize>,
    zoom: f32,
    pan: Vector,
    /// Where a drag started, the pan then and whether the pointer has moved since.
    drag: Option<(Point, Vector, bool)>,
}

impl Default for DrawingState {
    fn default() -> Self {
        Self { hover: None, zoom: 1.0, pan: Vector::ZERO, drag: None }
    }
}

impl SheetDrawing {
    fn sheet(&self) -> Option<&Sheet> {
        self.model.project.sheets.get(self.sheet)
    }

    /// Sheet metres to canvas pixels: the sheet's extent centred, y up, then zoomed and moved as
    /// `state` says.
    fn fit(&self, state: &DrawingState, size: Size) -> Option<Fit> {
        let e = self.sheet()?.extent()?;
        let (w, h) = (e.max[0] - e.min[0], e.max[1] - e.min[1]);
        if w <= 0.0 || h <= 0.0 {
            return None;
        }
        let fitted = ((size.width - 2.0 * MARGIN) / w).min((size.height - 2.0 * MARGIN) / h).max(1e-3);
        let scale = fitted * state.zoom;
        // the middle of the sheet, at the middle of the canvas moved by the pan
        let (cx, cy) = ((e.min[0] + e.max[0]) / 2.0, (e.min[1] + e.max[1]) / 2.0);
        let origin = Point::new(size.width / 2.0 + state.pan.x - cx * scale, 0.0);
        let top = size.height / 2.0 + state.pan.y + cy * scale;
        Some(Fit { scale, origin, top })
    }

    /// The viewport under a canvas point, the topmost (last placed) first.
    fn viewport_at(&self, state: &DrawingState, size: Size, at: Point) -> Option<usize> {
        let fit = self.fit(state, size)?;
        let sheet = [(at.x - fit.origin.x) / fit.scale, (fit.top - at.y) / fit.scale];
        self.sheet()?.viewports.iter().rposition(|v| v.view.is_some() && v.rect.contains(sheet))
    }
}

impl Fit {
    fn point(&self, p: [f32; 2]) -> Point {
        Point::new(self.origin.x + p[0] * self.scale, self.top - p[1] * self.scale)
    }

    fn rect(&self, r: &SheetRect) -> (Point, Size) {
        let a = self.point([r.min[0], r.max[1]]);
        (a, Size::new((r.max[0] - r.min[0]) * self.scale, (r.max[1] - r.min[1]) * self.scale))
    }
}

/// A text of a drawing on the canvas: `on` places its points, `scale` is what a metre of the
/// drawing is on screen. Left out when its letters are too small to see or it lies off the canvas.
fn draw_text(frame: &mut Frame, t: &crate::model::DraftText, on: &dyn Fn([f32; 2]) -> Point, scale: f32, color: Color, canvas: Size) {
    let size = t.size * scale;
    let x = [t.at[0], t.at[0] + t.width / 2.0, t.at[0] + t.width][t.align.min(2) as usize];
    let (top, foot) = (on([x, t.at[1] + t.height]), on([x, t.at[1]]));
    let reach = size * 60.0;
    if size < SMALLEST || top.x < -reach || top.x > canvas.width + reach || top.y < -reach || top.y > canvas.height + reach {
        return;
    }
    let text = |position: Point| canvas::Text {
        content: t.text.clone(),
        position,
        align_x: [alignment::Horizontal::Left, alignment::Horizontal::Center, alignment::Horizontal::Right][t.align.min(2) as usize].into(),
        align_y: alignment::Vertical::Top,
        size: iced::Pixels(size),
        line_height: iced::widget::text::LineHeight::Relative(1.3),
        color,
        font: openaec_ui::fonts::MEDIUM,
        ..canvas::Text::default()
    };
    if t.angle == 0.0 {
        t.face.fill(frame, text(top));
    } else {
        // turned about the foot of its box, under where its lines start
        frame.with_save(|frame| {
            frame.translate(Vector::new(foot.x, foot.y));
            frame.rotate(-t.angle);
            t.face.fill(frame, text(Point::new(0.0, -(foot.y - top.y).abs())));
        });
    }
}

impl canvas::Program<Message> for SheetDrawing {
    type State = DrawingState;

    fn update(&self, state: &mut DrawingState, event: &iced::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<canvas::Action<Message>> {
        match event {
            iced::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let p = cursor.position_in(bounds)?;
                let steps = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / 50.0,
                };
                let zoom = (state.zoom * 1.2f32.powf(steps).clamp(0.2, 5.0)).clamp(0.2, 400.0);
                // the point under the pointer stays under it
                let factor = zoom / state.zoom;
                let centre = Point::new(bounds.width / 2.0, bounds.height / 2.0);
                let from = Vector::new(p.x - centre.x - state.pan.x, p.y - centre.y - state.pan.y);
                state.pan = Vector::new(p.x - centre.x - from.x * factor, p.y - centre.y - from.y * factor);
                state.zoom = zoom;
                Some(canvas::Action::request_redraw().and_capture())
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left | mouse::Button::Middle)) => {
                let p = cursor.position_in(bounds)?;
                state.drag = Some((p, state.pan, false));
                Some(canvas::Action::capture())
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let p = cursor.position_in(bounds);
                if let (Some((from, pan, moved)), Some(p)) = (state.drag.as_mut(), p) {
                    if *moved || from.distance(p) > 4.0 {
                        *moved = true;
                        state.pan = Vector::new(pan.x + p.x - from.x, pan.y + p.y - from.y);
                        return Some(canvas::Action::request_redraw());
                    }
                    return None;
                }
                let hover = p.and_then(|p| self.viewport_at(state, bounds.size(), p));
                (hover != state.hover).then(|| {
                    state.hover = hover;
                    canvas::Action::request_redraw()
                })
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(button @ (mouse::Button::Left | mouse::Button::Middle))) => {
                let (_, _, moved) = state.drag.take()?;
                // a click, not a drag: the view under it shows in 3D
                if moved || *button != mouse::Button::Left {
                    return None;
                }
                let i = self.viewport_at(state, bounds.size(), cursor.position_in(bounds)?)?;
                let view = self.sheet()?.viewports[i].view?;
                Some(canvas::Action::publish(Message::SheetView(view)).and_capture())
            }
            _ => None,
        }
    }

    fn draw(&self, state: &DrawingState, renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
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
        let (Some(sheet), Some(fit)) = (self.sheet(), self.fit(state, bounds.size())) else {
            let centre = Point::new(bounds.width / 2.0, bounds.height / 2.0);
            label(&mut frame, self.empty.clone(), centre, 13.0, p.text_muted, alignment::Horizontal::Center, alignment::Vertical::Bottom);
            return vec![frame.into_geometry()];
        };
        let paper = if p.dark { Color::from_rgb(0.16, 0.16, 0.18) } else { Color::WHITE };
        let ink = p.text;
        let faint = Color { a: 0.35, ..p.text_secondary };
        // what lies outside the canvas is not worth its lines
        let seen = |a: Point, b: Point| a.x.max(b.x) >= 0.0 && a.x.min(b.x) <= bounds.width && a.y.max(b.y) >= 0.0 && a.y.min(b.y) <= bounds.height;

        // the paper, with the title block's usual 10 mm frame (20 mm on the binding side)
        if let Some(r) = &sheet.paper {
            let (at, size) = fit.rect(r);
            frame.fill_rectangle(Point::new(at.x + 3.0, at.y + 3.0), size, Color { a: 0.12, ..Color::BLACK });
            frame.fill_rectangle(at, size, paper);
            frame.stroke(&Path::rectangle(at, size), Stroke::default().with_color(faint).with_width(1.0));
            let inner = SheetRect { min: [r.min[0] + 0.02, r.min[1] + 0.01], max: [r.max[0] - 0.01, r.max[1] - 0.01] };
            // a sheet without a title block of its own gets a frame and its name
            if sheet.lines.is_empty() {
                let (at, size) = fit.rect(&inner);
                frame.stroke(&Path::rectangle(at, size), Stroke::default().with_color(ink).with_width(1.4));
                let title = Point::new(at.x + size.width - 6.0, at.y + size.height - 6.0);
                label(&mut frame, sheet.title.clone(), title, 14.0, ink, alignment::Horizontal::Right, alignment::Vertical::Bottom);
            }
        }
        // the images of the title block, under its lines
        for i in &sheet.images {
            let (centre, size) = (fit.point(i.center), Size::new(i.size[0] * fit.scale, i.size[1] * fit.scale));
            let at = Point::new(centre.x - size.width / 2.0, centre.y - size.height / 2.0);
            if seen(at, Point::new(at.x + size.width, at.y + size.height)) {
                frame.draw_image(Rectangle::new(at, size), canvas::Image::new(i.image.clone()).rotation(iced::Radians(-i.angle)));
            }
        }
        // the regions of the title block filled in one colour, under its lines
        for f in &sheet.fills {
            let outlines = Path::new(|path| {
                for outline in f.loops.iter().filter(|l| l.len() >= 3) {
                    path.move_to(fit.point(outline[0]));
                    for &q in &outline[1..] {
                        path.line_to(fit.point(q));
                    }
                    path.close();
                }
            });
            // one without a colour of its own masks what lies under it: the paper shows
            let color = if f.color == [0, 0, 0] { paper } else { Color::from_rgb8(f.color[0], f.color[1], f.color[2]) };
            frame.fill(&outlines, canvas::Fill { style: canvas::Style::Solid(color), rule: canvas::fill::Rule::EvenOdd });
        }
        // the title block: its lines and its texts
        // each line in its own colour where it has one; a line count that does not match gives
        // them all the ink
        let own = |k: usize| if sheet.line_colors.len() == sheet.lines.len() { sheet.line_colors[k].filter(|c| *c != [0, 0, 0]) } else { None };
        let mut colors: Vec<Option<[u8; 3]>> = (0..sheet.lines.len()).map(own).collect();
        colors.sort_unstable();
        colors.dedup();
        for c in colors {
            let block = Path::new(|path| {
                for (a, b) in sheet.lines.iter().enumerate().filter(|(k, _)| own(*k) == c).map(|(_, l)| (fit.point(l[0]), fit.point(l[1]))).filter(|(a, b)| seen(*a, *b)) {
                    path.move_to(a);
                    path.line_to(b);
                }
            });
            frame.stroke(&block, Stroke::default().with_color(c.map_or(ink, |c| Color::from_rgb8(c[0], c[1], c[2]))).with_width(0.9));
        }
        // a text on a dark region is written in the paper's colour, to stay readable
        let dark = |p: [f32; 2]| {
            sheet.fills.iter().any(|f| {
                let c = if f.color == [0, 0, 0] { [ink.r, ink.g, ink.b] } else { f.color.map(|v| f32::from(v) / 255.0) };
                let crossings: usize = f.loops.iter().map(|l| (0..l.len()).filter(|&k| {
                    let (a, b) = (l[k], l[(k + 1) % l.len()]);
                    (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0])
                }).count()).sum();
                crossings % 2 == 1 && 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2] < 0.4
            })
        };
        for t in &sheet.texts {
            let middle = [t.at[0] + t.width.min(t.size * 4.0) / 2.0, t.at[1] + t.height / 2.0];
            draw_text(&mut frame, t, &|q| fit.point(q), fit.scale, if dark(middle) { paper } else { ink }, bounds.size());
        }
        // the schedules: the borders of their cells and the texts in them
        for table in &self.tables {
            let rect = table.rect();
            if !seen(fit.point(rect.min), fit.point(rect.max)) {
                continue;
            }
            let lefts: Vec<f32> = std::iter::once(0.0).chain(table.columns.iter().scan(0.0, |x, w| { *x += w; Some(*x) })).collect();
            let at = |k: usize| table.origin[0] + lefts[k.min(lefts.len() - 1)];
            let mut top = table.origin[1];
            let mut texts = Vec::new();
            // the images of its cells, each with its cell: left, top, width and height
            let mut images: Vec<(iced::widget::image::Handle, [f32; 4])> = Vec::new();
            let grid = Path::new(|path| {
                for row in &table.rows {
                    let foot = top - row.height;
                    if row.plain {
                        path.rectangle(fit.point([at(0), top]), Size::new((at(usize::MAX) - at(0)) * fit.scale, row.height * fit.scale));
                    }
                    for cell in &row.cells {
                        let (left, right) = if row.title { (at(0), at(usize::MAX)) } else { (at(cell.column as usize), at(cell.column as usize + cell.span.max(1) as usize)) };
                        if !row.plain {
                            path.rectangle(fit.point([left, top]), Size::new((right - left) * fit.scale, row.height * fit.scale));
                        }
                        if let Some(image) = &cell.image {
                            images.push((image.clone(), [left, top, right - left, row.height]));
                            continue;
                        }
                        if cell.text.is_empty() {
                            continue;
                        }
                        // its text, clear of the borders
                        let pad = (cell.size * 0.4).min((right - left) / 4.0);
                        let height = cell.text.lines().count().max(1) as f32 * cell.size * 1.3;
                        let room = (row.height - height).max(0.0);
                        let lift = [room - pad.min(room / 2.0), room / 2.0, pad.min(room / 2.0)][cell.vertical.min(2) as usize];
                        texts.push(crate::model::DraftText { element: 0, at: [left + pad, foot + lift], width: (right - left - 2.0 * pad).max(0.0), height, size: cell.size, align: cell.align, angle: 0.0, text: cell.text.clone(), face: Default::default() });
                    }
                    top = foot;
                }
            });
            // an image fills its cell as far as its own shape allows, clear of the borders
            for (image, [left, top, width, height]) in images {
                let room = (width.min(height) * 0.08).min(0.0006);
                let (w, h) = ((width - 2.0 * room).max(0.0), (height - 2.0 * room).max(0.0));
                let side = w.min(h);
                let at = fit.point([left + (width - side) / 2.0, top - (height - side) / 2.0]);
                frame.draw_image(Rectangle::new(at, Size::new(side * fit.scale, side * fit.scale)), canvas::Image::new(image));
            }
            frame.stroke(&grid, Stroke::default().with_color(ink).with_width(0.7));
            for t in &texts {
                draw_text(&mut frame, t, &|q| fit.point(q), fit.scale, ink, bounds.size());
            }
        }

        // the placed views: what they show, their frame, then the view title under it (number,
        // name, scale)
        for (i, v) in sheet.viewports.iter().enumerate() {
            let (at, size) = fit.rect(&v.rect);
            let hot = state.hover == Some(i);
            let color = if hot { p.accent } else if v.view.is_some() { ink } else { p.text_muted };
            if hot {
                frame.fill_rectangle(at, size, Color { a: 0.08, ..p.accent });
            }
            let drawn = match self.placed.get(i) {
                Some(Placed::Draft(k)) => self.model.project.drafts.get(*k).is_some_and(|d| {
                    // a drawing's point on the sheet: its origin where the viewport has it
                    let on = |q: [f32; 2]| fit.point([v.origin[0] + q[0] * v.ratio, v.origin[1] + q[1] * v.ratio]);
                    // the regions filled in one colour, under the lines: black is the ink of the sheet
                    let shade = |c: [u8; 3]| if c == [0, 0, 0] { ink } else { Color::from_rgb8(c[0], c[1], c[2]) };
                    for f in &d.fills {
                        let outlines = Path::new(|path| {
                            for outline in f.loops.iter().filter(|l| l.len() >= 3) {
                                path.move_to(on(outline[0]));
                                for &q in &outline[1..] {
                                    path.line_to(on(q));
                                }
                                path.close();
                            }
                        });
                        frame.fill(&outlines, canvas::Fill { style: canvas::Style::Solid(if f.color == [0, 0, 0] { paper } else { shade(f.color) }), rule: canvas::fill::Rule::EvenOdd });
                    }
                    // the lines, those with a colour of their own in it
                    let own = |k: usize| d.line_colors.get(k).copied().flatten();
                    let mut colors: Vec<Option<[u8; 3]>> = d.line_colors.iter().flatten().map(|c| Some(*c)).collect();
                    colors.sort_unstable();
                    colors.dedup();
                    for c in std::iter::once(None).chain(colors) {
                        let lines = Path::new(|path| {
                            for (a, b) in d.lines.iter().enumerate().filter(|(k, _)| own(*k) == c).map(|(_, l)| (on(l.a), on(l.b))).filter(|(a, b)| seen(*a, *b)) {
                                path.move_to(a);
                                path.line_to(b);
                            }
                        });
                        frame.stroke(&lines, Stroke::default().with_color(c.map_or(ink, shade)).with_width(0.8));
                    }
                    // what a metre of paper is on screen
                    let paper = fit.scale;
                    for m in &d.dimensions {
                        let (a, b) = (on(m.from), on(m.to));
                        let run = a.distance(b);
                        if run <= 0.0 || !seen(a, b) {
                            continue;
                        }
                        let (ux, uy) = ((b.x - a.x) / run, (b.y - a.y) / run);
                        let tick = (m.style.tick_size as f32 * paper).max(1.0);
                        let lines = Path::new(|path| {
                            path.move_to(a);
                            path.line_to(b);
                            for (q, from) in [(a, on(m.measured[0])), (b, on(m.measured[1]))] {
                                path.move_to(from);
                                path.line_to(q);
                                if m.style.tick != Tick::None {
                                    path.move_to(Point::new(q.x - (ux + uy) * tick, q.y - (uy - ux) * tick));
                                    path.line_to(Point::new(q.x + (ux + uy) * tick, q.y + (uy - ux) * tick));
                                }
                            }
                        });
                        frame.stroke(&lines, Stroke::default().with_color(ink).with_width(0.6));
                        let size = m.style.size as f32 * paper;
                        if size >= SMALLEST {
                            let length = (m.to[0] - m.from[0]).hypot(m.to[1] - m.from[1]) as f64;
                            let (nx, ny) = if uy.abs() > ux.abs() { (-1.0, 0.0) } else { (0.0, -1.0) };
                            let content = m.style.figure(length);
                            let clear = if nx != 0.0 { content.chars().count() as f32 * size * 0.3 + 2.0 } else { size * 0.7 };
                            let at = Point::new((a.x + b.x) / 2.0 + nx * clear, (a.y + b.y) / 2.0 + ny * clear);
                            label(&mut frame, content, at, size, ink, alignment::Horizontal::Center, alignment::Vertical::Center);
                        }
                    }
                    for t in &d.texts {
                        draw_text(&mut frame, t, &on, v.ratio * fit.scale, ink, bounds.size());
                    }
                    true
                }),
                Some(Placed::View { cut, crop, dimensions, notes, lines, fills }) => {
                    // a drawing's point on the sheet: its own origin where the viewport has it, else
                    // its crop in the viewport's box, else the middle of what it cuts in the middle
                    // of that
                    let extent = cut.as_ref().and_then(|c| c.extent).map(|(lo, hi)| (lo.map(f64::from), hi.map(f64::from)));
                    let anchor = match (v.anchor, crop, extent) {
                        (Some(anchor), ..) => Some((anchor, v.origin)),
                        (None, Some((lo, _)), _) => Some((*lo, v.rect.min)),
                        (None, None, Some((lo, hi))) => Some(([(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0], v.rect.center())),
                        (None, None, None) => None,
                    };
                    anchor.is_some_and(|(from, to)| {
                        let ratio = f64::from(v.ratio);
                        // along the view's own axes: a turned view lies turned on the sheet
                        let [cx, cy] = v.turn;
                        let on = |q: [f64; 2]| {
                            let (dx, dy) = (q[0] - from[0], q[1] - from[1]);
                            fit.point([to[0] + ((dx * cx + dy * cy) * ratio) as f32, to[1] + ((dy * cx - dx * cy) * ratio) as f32])
                        };
                        let turned = cy.atan2(cx) as f32;
                        let piece = |a: [f64; 2], z: [f64; 2]| match crop {
                            Some((lo, hi)) => super::paper::clip(a, z, *lo, *hi),
                            None => Some((a, z)),
                        };
                        let stroke = |frame: &mut Frame, segments: &mut dyn Iterator<Item = ([f64; 2], [f64; 2])>, color: Color, width: f32| {
                            let path = Path::new(|path| {
                                for (a, b) in segments.filter_map(|(a, z)| piece(a, z)).map(|(a, z)| (on(a), on(z))).filter(|(a, b)| seen(*a, *b)) {
                                    path.move_to(a);
                                    path.line_to(b);
                                }
                            });
                            frame.stroke(&path, Stroke::default().with_color(color).with_width(width));
                        };
                        let wide = |s: &[[f32; 2]; 2]| (s[0].map(f64::from), s[1].map(f64::from));
                        if let Some(cut) = cut {
                            // the cut faces in their hatch, under the lines
                            super::hatch::draw_fills(&mut frame, cut, *crop, &|q| Some(on(q)), (ratio * f64::from(fit.scale)) as f32, None);
                            // what is seen beyond the cut, thin and grey, then the cut itself
                            stroke(&mut frame, &mut cut.below.iter().map(wide), Color { a: 0.55, ..ink }, 0.5);
                            stroke(&mut frame, &mut cut.segments.iter().map(wide), ink, 1.1);
                        }
                        // the regions the view fills in one colour, under its own lines: black is
                        // the ink of the sheet; one that starts outside the crop is left out
                        let within = |q: [f64; 2]| crop.is_none_or(|(lo, hi)| q[0] >= lo[0] && q[0] <= hi[0] && q[1] >= lo[1] && q[1] <= hi[1]);
                        for f in fills.iter().filter(|f| f.loops.first().and_then(|l| l.first()).is_some_and(|q| within(*q))) {
                            let outlines = Path::new(|path| {
                                for outline in f.loops.iter().filter(|l| l.len() >= 3) {
                                    path.move_to(on(outline[0]));
                                    for &q in &outline[1..] {
                                        path.line_to(on(q));
                                    }
                                    path.close();
                                }
                            });
                            let color = if f.color == [0, 0, 0] { ink } else { Color::from_rgb8(f.color[0], f.color[1], f.color[2]) };
                            frame.fill(&outlines, canvas::Fill { style: canvas::Style::Solid(color), rule: canvas::fill::Rule::EvenOdd });
                        }
                        // the lines of its dimensions and tags reach past its crop, as its texts do;
                        // its detail lines are cut to it where they are read
                        let marks = Path::new(|path| {
                            for (a, b) in lines.iter().map(|l| (on(l[0]), on(l[1]))).filter(|(a, b)| seen(*a, *b)) {
                                path.move_to(a);
                                path.line_to(b);
                            }
                        });
                        frame.stroke(&marks, Stroke::default().with_color(ink).with_width(0.7));
                        let paper = fit.scale;
                        for d in dimensions {
                            let (a, b) = (on(d.from), on(d.to));
                            let style = self.styles.get(d.style).or(self.styles.first());
                            let Some(style) = style.filter(|_| seen(a, b)) else { continue };
                            let look = super::marks::DimensionLook { offset: paper * super::marks::OFFSET as f32, paper, size: (paper * style.size as f32).max(SMALLEST), style };
                            super::marks::draw_dimension_px(&mut frame, a, b, d.length(), ink, look);
                        }
                        for note in notes {
                            let (at, size) = (on(note.at), note.size as f32 * paper);
                            if size >= SMALLEST && at.x > -size * 60.0 && at.x < bounds.width + size * 60.0 && at.y > -size * 8.0 && at.y < bounds.height + size * 8.0 {
                                super::marks::draw_note_in(&mut frame, at, &note.text, size, ink, note.align, note.angle as f32 - turned, &note.face);
                            }
                        }
                        true
                    })
                }
                _ => false,
            };
            // a view that shows what it holds needs no frame, unless the pointer is on it
            let mut stroke = Stroke::default().with_color(if drawn && !hot { faint } else { color }).with_width(if hot { 2.0 } else if drawn { 0.5 } else { 1.0 });
            if v.estimated {
                stroke.line_dash = canvas::LineDash { segments: &[6.0, 4.0], offset: 0 };
            }
            // a guessed box says nothing once the view itself is drawn
            if !(v.estimated && drawn) {
                frame.stroke(&Path::rectangle(at, size), stroke);
            }
            // the title where the sheet has it, else under the box
            let under = v.label.map_or(Point::new(at.x, at.y + size.height + TITLE * fit.scale * 1.4), |l| fit.point(l));
            // as the type of its viewport writes it; else its number on the sheet and its name
            let mut title = String::new();
            if let Some(written) = &v.written {
                title.push_str(written);
            } else {
                if let Some(d) = &v.detail {
                    title.push_str(d);
                    title.push_str("  ");
                }
                title.push_str(&v.title);
            }
            // on paper: it grows and shrinks with the sheet
            let (name, scale) = (TITLE * fit.scale, TITLE_SCALE * fit.scale);
            if name >= SMALLEST {
                label(&mut frame, title, under, name, color, alignment::Horizontal::Left, alignment::Vertical::Bottom);
                if v.written.is_none() {
                    label(&mut frame, v.scale.clone(), Point::new(under.x, under.y + scale * 1.4), scale, p.text_secondary, alignment::Horizontal::Left, alignment::Vertical::Bottom);
                }
            }
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, state: &DrawingState, _bounds: Rectangle, _cursor: mouse::Cursor) -> mouse::Interaction {
        if state.drag.is_some_and(|(_, _, moved)| moved) {
            mouse::Interaction::Grabbing
        } else if state.hover.is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Layer, Model, SavedView, SheetViewport, ViewKind};
    
    fn app_with_sheet() -> App {
        let layer = Layer {
            name: "Walls".into(),
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: [0.0, 0.0, 1.0].repeat(3),
            indices: vec![0, 1, 2],
            ..Default::default()
        };
        let mut model = Model::new("house.rvt".into(), "Revit", vec![layer]);
        model.views = vec![SavedView { folder: Vec::new(), name: "Level 1".into(), kind: ViewKind::Plan, camera: None, clip: None, plan: None, level: None }];
        let viewport = |view| SheetViewport {
            title: "Level 1".into(),
            view,
            rect: SheetRect::around([-0.6, 0.4], [0.4, 0.3]),
            estimated: false,
            scale: "1:50".into(),
            detail: Some("1".into()),
            origin: [-0.6, 0.4],
            ratio: 0.02,
            label: None,
            drawing: None,
            anchor: None,
            turn: [1.0, 0.0],
            draft: None,
            written: None,
        };
        model.project.sheets = vec![
            Sheet::new("A-1".into(), "Plans".into(), Some(SheetRect { min: [-1.189, 0.0], max: [0.0, 0.841] }), vec![viewport(Some(0))]),
            Sheet::new("A-2".into(), "Legend".into(), None, vec![viewport(None)]),
        ];
        let mut app = App::default();
        app.set_model(Arc::new(model));
        app
    }

    #[test]
    fn a_click_on_a_view_finds_it() {
        let app = app_with_sheet();
        let d = SheetDrawing { model: app.model.clone().unwrap(), sheet: 0, placed: Vec::new(), tables: Vec::new(), styles: Vec::new(), palette: app.theme.palette(), empty: String::new() };
        let state = DrawingState::default();
        let size = Size::new(1189.0 + 2.0 * MARGIN, 841.0 + 2.0 * MARGIN);
        let fit = d.fit(&state, size).unwrap();
        assert!((fit.scale - 1000.0).abs() < 1.0);
        assert_eq!(d.viewport_at(&state, size, fit.point([-0.6, 0.4])), Some(0));
        assert_eq!(d.viewport_at(&state, size, fit.point([-0.1, 0.1])), None);
        // a legend (no view the viewer shows) is not clickable
        let legend = SheetDrawing { sheet: 1, ..d };
        assert_eq!(legend.viewport_at(&state, size, legend.fit(&state, size).unwrap().point([-0.6, 0.4])), None);
    }

    #[test]
    fn sheet_and_schedule_tabs_take_turns() {
        let mut app = app_with_sheet();
        let _ = app.open_sheet(1);
        let _ = app.open_sheet(0);
        assert_eq!((app.open_sheets.clone(), app.active_sheet), (vec![1, 0], Some(0)));
        let _ = app.close_sheet(0);
        assert_eq!(app.active_sheet, Some(1));
        // a view on the sheet: the 3D view shows it
        let _ = app.sheet_view(0);
        assert_eq!((app.active_sheet, app.active_view), (None, Some(0)));
        // a new model closes the sheets
        let model = app.model.clone().unwrap();
        app.set_model(model);
        assert!(app.open_sheets.is_empty() && app.active_sheet.is_none());
    }
}
