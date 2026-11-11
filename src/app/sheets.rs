//! Sheets (Revit, beta): the explorer group, the document tabs and the sheet drawn in the
//! workspace: the paper, the title block's frame and the placed views with their title, scale and
//! detail number. A click on a view shows it in 3D.

use super::{group_header, App, Message};
use crate::model::{Sheet, SheetRect};
use crate::theme::Palette;
use iced::alignment;
use iced::mouse;
use iced::widget::canvas::{self, Frame, Path, Stroke};
use iced::widget::text::Wrapping;
use iced::widget::{button, canvas as canvas_widget, container, text, Column};
use iced::{Color, Element, Fill, Padding, Point, Rectangle, Size, Task};
use openaec_ui::Language;
use serde_json::{json, Value};

/// Margin around the sheet in the workspace (logical pixels).
const MARGIN: f32 = 24.0;

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
        Task::none()
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
    pub(super) fn sheets_group<'a>(&self, p: Palette, l: Language, query: &str, mut body: Column<'a, Message>) -> Column<'a, Message> {
        let sheets = self.sheets();
        if sheets.is_empty() {
            return body;
        }
        let shown: Vec<usize> = (0..sheets.len()).filter(|&i| query.is_empty() || sheets[i].title.to_lowercase().contains(query)).collect();
        let open = self.explorer_open[4] || !query.is_empty();
        body = body.push(group_header(p, l.text("Bladen", "Sheets"), Some("beta"), shown.len(), open, Message::ToggleExplorer(4)));
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
        let drawing = SheetDrawing { sheet: sheet.clone(), palette: p, empty: l.text("Geen geplaatste views", "No placed views").to_string() };
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

/// A sheet on a canvas.
struct SheetDrawing {
    sheet: Sheet,
    palette: Palette,
    empty: String,
}

/// The mapping from sheet metres to canvas pixels.
struct Fit {
    scale: f32,
    origin: Point,
    top: f32,
}

impl SheetDrawing {
    /// Sheet metres to canvas pixels: the sheet's extent centred, y up.
    fn fit(&self, size: Size) -> Option<Fit> {
        let e = self.sheet.extent()?;
        let (w, h) = (e.max[0] - e.min[0], e.max[1] - e.min[1]);
        if w <= 0.0 || h <= 0.0 {
            return None;
        }
        let scale = ((size.width - 2.0 * MARGIN) / w).min((size.height - 2.0 * MARGIN) / h).max(1e-3);
        let origin = Point::new((size.width - w * scale) / 2.0 - e.min[0] * scale, 0.0);
        let top = (size.height - h * scale) / 2.0 + e.max[1] * scale;
        Some(Fit { scale, origin, top })
    }

    /// The viewport under a canvas point, the topmost (last placed) first.
    fn viewport_at(&self, size: Size, at: Point) -> Option<usize> {
        let fit = self.fit(size)?;
        let sheet = [(at.x - fit.origin.x) / fit.scale, (fit.top - at.y) / fit.scale];
        self.sheet.viewports.iter().rposition(|v| v.view.is_some() && v.rect.contains(sheet))
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

/// The hovered viewport.
#[derive(Default)]
struct DrawingState {
    hover: Option<usize>,
}

impl canvas::Program<Message> for SheetDrawing {
    type State = DrawingState;

    fn update(&self, state: &mut DrawingState, event: &iced::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<canvas::Action<Message>> {
        match event {
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let hover = cursor.position_in(bounds).and_then(|p| self.viewport_at(bounds.size(), p));
                (hover != state.hover).then(|| {
                    state.hover = hover;
                    canvas::Action::request_redraw()
                })
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let i = self.viewport_at(bounds.size(), cursor.position_in(bounds)?)?;
                let view = self.sheet.viewports[i].view?;
                Some(canvas::Action::publish(Message::SheetView(view)).and_capture())
            }
            _ => None,
        }
    }

    fn draw(&self, state: &DrawingState, renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let p = self.palette;
        let mut frame = Frame::new(renderer, bounds.size());
        let label = |frame: &mut Frame, content: String, position: Point, size: f32, color: Color, align: alignment::Horizontal| {
            frame.fill_text(canvas::Text {
                content,
                position,
                align_x: align.into(),
                align_y: alignment::Vertical::Bottom,
                size: iced::Pixels(size),
                color,
                font: openaec_ui::fonts::MEDIUM,
                ..canvas::Text::default()
            });
        };
        let Some(fit) = self.fit(bounds.size()) else {
            let centre = Point::new(bounds.width / 2.0, bounds.height / 2.0);
            label(&mut frame, self.empty.clone(), centre, 13.0, p.text_muted, alignment::Horizontal::Center);
            return vec![frame.into_geometry()];
        };
        let paper = if p.dark { Color::from_rgb(0.16, 0.16, 0.18) } else { Color::WHITE };
        let ink = p.text;
        let faint = Color { a: 0.35, ..p.text_secondary };

        // the paper, with the title block's usual 10 mm frame (20 mm on the binding side)
        if let Some(r) = &self.sheet.paper {
            let (at, size) = fit.rect(r);
            frame.fill_rectangle(Point::new(at.x + 3.0, at.y + 3.0), size, Color { a: 0.12, ..Color::BLACK });
            frame.fill_rectangle(at, size, paper);
            frame.stroke(&Path::rectangle(at, size), Stroke::default().with_color(faint).with_width(1.0));
            let inner = SheetRect { min: [r.min[0] + 0.02, r.min[1] + 0.01], max: [r.max[0] - 0.01, r.max[1] - 0.01] };
            let (at, size) = fit.rect(&inner);
            frame.stroke(&Path::rectangle(at, size), Stroke::default().with_color(ink).with_width(1.4));
            let title = Point::new(at.x + size.width - 6.0, at.y + size.height - 6.0);
            label(&mut frame, self.sheet.title.clone(), title, 14.0, ink, alignment::Horizontal::Right);
        }

        // the placed views: frame, then the view title under it (Revit's style: number, name, scale)
        for (i, v) in self.sheet.viewports.iter().enumerate() {
            let (at, size) = fit.rect(&v.rect);
            let hot = state.hover == Some(i);
            let color = if hot { p.accent } else if v.view.is_some() { ink } else { p.text_muted };
            if hot {
                frame.fill_rectangle(at, size, Color { a: 0.08, ..p.accent });
            }
            let mut stroke = Stroke::default().with_color(color).with_width(if hot { 2.0 } else { 1.0 });
            if v.estimated {
                stroke.line_dash = canvas::LineDash { segments: &[6.0, 4.0], offset: 0 };
            }
            frame.stroke(&Path::rectangle(at, size), stroke);
            let under = Point::new(at.x, at.y + size.height + 16.0);
            let mut title = String::new();
            if let Some(d) = &v.detail {
                title.push_str(d);
                title.push_str("  ");
            }
            title.push_str(&v.title);
            label(&mut frame, title, under, 12.0, color, alignment::Horizontal::Left);
            label(&mut frame, v.scale.clone(), Point::new(at.x, under.y + 14.0), 10.0, p.text_secondary, alignment::Horizontal::Left);
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, state: &DrawingState, _bounds: Rectangle, _cursor: mouse::Cursor) -> mouse::Interaction {
        if state.hover.is_some() {
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
    use std::sync::Arc;

    fn app_with_sheet() -> App {
        let layer = Layer {
            name: "Walls".into(),
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normals: [0.0, 0.0, 1.0].repeat(3),
            indices: vec![0, 1, 2],
            ..Default::default()
        };
        let mut model = Model::new("house.rvt".into(), "Revit", vec![layer]);
        model.views = vec![SavedView { name: "Level 1".into(), kind: ViewKind::Plan, camera: None, clip: None, plan: None, level: None }];
        let viewport = |view| SheetViewport {
            title: "Level 1".into(),
            view,
            rect: SheetRect::around([-0.6, 0.4], [0.4, 0.3]),
            estimated: false,
            scale: "1:50".into(),
            detail: Some("1".into()),
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
        let d = SheetDrawing { sheet: app.sheets()[0].clone(), palette: app.theme.palette(), empty: String::new() };
        let size = Size::new(1189.0 + 2.0 * MARGIN, 841.0 + 2.0 * MARGIN);
        let fit = d.fit(size).unwrap();
        assert!((fit.scale - 1000.0).abs() < 1.0);
        assert_eq!(d.viewport_at(size, fit.point([-0.6, 0.4])), Some(0));
        assert_eq!(d.viewport_at(size, fit.point([-0.1, 0.1])), None);
        // a legend (no view the viewer shows) is not clickable
        let legend = SheetDrawing { sheet: app.sheets()[1].clone(), ..d };
        assert_eq!(legend.viewport_at(size, legend.fit(size).unwrap().point([-0.6, 0.4])), None);
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
