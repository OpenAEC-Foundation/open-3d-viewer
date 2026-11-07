//! The sheets of the project document: their document tabs, the bar to place floor plans on a
//! sheet, and the sheet drawn on a canvas: the paper with its frame and title, and each placed
//! floor plan as the lines of its cut at its scale, which a drag moves over the paper.

use super::{App, Message, FONT_BOLD};
use crate::document::Sheet;
use crate::drawing::{cut_at, Cut};
use crate::theme::Palette;
use iced::alignment;
use iced::mouse;
use iced::widget::canvas::{self, Frame, Path, Stroke};
use iced::widget::{button, canvas as canvas_widget, column, container, row, text, Row};
use iced::{Center, Color, Element, Fill, Point, Rectangle, Size};
use openaec_ui::Language;
use std::sync::Arc;

/// Margin around the paper in the workspace (logical pixels).
const MARGIN: f32 = 24.0;
/// Space between a placed view's lines and its frame (paper metres).
const FRAME: f64 = 0.005;

impl App {
    /// Makes a sheet and shows it in its tab.
    pub(super) fn new_sheet(&mut self, name: &str) {
        let path = self.document.add_sheet(name);
        self.open_doc_sheet(&path);
    }

    /// Shows a sheet of the document in its tab, opening the tab the first time.
    pub(super) fn open_doc_sheet(&mut self, path: &str) {
        if self.document.sheet(path).is_none() {
            return;
        }
        if !self.open_doc_sheets.iter().any(|s| s == path) {
            self.open_doc_sheets.push(path.to_string());
        }
        self.active_sheet = None;
        self.active_schedule = None;
        self.active_doc_sheet = Some(path.to_string());
        self.cut_placed_plans(path);
    }

    /// Closes a sheet's tab; when it showed, the sheet before it (or the view under it) shows.
    pub(super) fn close_doc_sheet(&mut self, path: &str) {
        let Some(k) = self.open_doc_sheets.iter().position(|s| s == path) else { return };
        self.open_doc_sheets.remove(k);
        if self.active_doc_sheet.as_deref() == Some(path) {
            self.active_doc_sheet = k.checked_sub(1).or((!self.open_doc_sheets.is_empty()).then_some(0)).map(|i| self.open_doc_sheets[i].clone());
        }
    }

    /// Places a floor plan on a sheet, in its middle.
    pub(super) fn place_on_sheet(&mut self, sheet: &str, plan: &str) {
        if self.document.place_plan(sheet, plan).is_some() {
            self.cut_placed_plans(sheet);
        }
    }

    /// Cuts the floor plans on a sheet that are not cut yet: at their cut height, through the
    /// layers shown now.
    fn cut_placed_plans(&mut self, sheet: &str) {
        let Some(model) = self.model.clone() else { return };
        let Some(sheet) = self.document.sheet(sheet) else { return };
        let plans: Vec<String> = sheet.views.iter().map(|v| v.plan.clone()).filter(|p| !self.cuts.contains_key(p)).collect();
        for plan in plans {
            let Some(view) = self.document.plan(&plan) else { continue };
            let Some((_, high)) = self.document.plan_range(view) else { continue };
            self.cuts.insert(plan, Arc::new(cut_at(&model, &self.visible, high as f32)));
        }
    }

    /// The workspace of a sheet's tab: the bar to place floor plans, then the sheet.
    pub(super) fn doc_sheet_workspace(&self, p: Palette, l: Language) -> Option<Element<'_, Message>> {
        let path = self.active_doc_sheet.as_ref()?;
        let sheet = self.document.sheet(path)?;
        let key: Arc<str> = Arc::from(path.as_str());
        let mut bar = Row::new().spacing(6).align_y(Center).push(text(l.text("Plaats op dit blad:", "Place on this sheet:")).size(12).color(p.text_secondary));
        if self.document.plans().is_empty() {
            bar = bar.push(text(l.text("maak eerst een plattegrond in Weergaven", "make a floor plan in Views first")).size(12).color(p.text_muted));
        }
        for plan in self.document.plans() {
            bar = bar.push(
                button(text(plan.name.clone()).size(12))
                    .padding([3, 10])
                    .style(move |_, s| p.ribbon_button(false, s))
                    .on_press(Message::PlaceOnSheet(key.clone(), Arc::from(plan.path.as_str()))),
            );
        }
        let placed = sheet
            .views
            .iter()
            .map(|v| {
                let name = self.document.plan(&v.plan).map_or(String::new(), |p| p.name.clone());
                (name, self.cuts.get(&v.plan).cloned())
            })
            .collect();
        let drawing = PaperDrawing { sheet: sheet.clone(), placed, palette: p, key, empty: l.text("niets gesneden", "nothing cut").to_string() };
        Some(
            column![
                container(bar).padding([6, 10]).width(Fill).style(move |_| p.ribbon()),
                canvas_widget(drawing).width(Fill).height(Fill),
            ]
            .into(),
        )
    }
}

/// A sheet of the document on a canvas.
struct PaperDrawing {
    sheet: Sheet,
    /// Per placed view: the floor plan's name and its cut.
    placed: Vec<(String, Option<Arc<Cut>>)>,
    palette: Palette,
    /// The sheet's path, for the messages.
    key: Arc<str>,
    empty: String,
}

/// Paper metres to canvas pixels: the paper centred, y up.
#[derive(Debug, Clone, Copy)]
struct Fit {
    scale: f64,
    left: f64,
    top: f64,
}

impl Fit {
    fn of(paper: [f64; 2], size: Size) -> Self {
        let scale = ((size.width as f64 - 2.0 * MARGIN as f64) / paper[0]).min((size.height as f64 - 2.0 * MARGIN as f64) / paper[1]).max(1e-3);
        let left = (size.width as f64 - paper[0] * scale) / 2.0;
        let top = (size.height as f64 - paper[1] * scale) / 2.0 + paper[1] * scale;
        Fit { scale, left, top }
    }

    fn point(&self, p: [f64; 2]) -> Point {
        Point::new((self.left + p[0] * self.scale) as f32, (self.top - p[1] * self.scale) as f32)
    }

    fn paper(&self, at: Point) -> [f64; 2] {
        [(at.x as f64 - self.left) / self.scale, (self.top - at.y as f64) / self.scale]
    }
}

impl PaperDrawing {
    /// A placed view's frame on the paper (lower left, upper right) and the middle of its cut in
    /// the model.
    fn frame(&self, i: usize) -> ([f64; 2], [f64; 2], [f64; 2]) {
        let view = &self.sheet.views[i];
        let extent = self.placed.get(i).and_then(|(_, c)| c.as_ref()?.extent);
        let (half, middle) = match extent {
            Some((lo, hi)) => {
                let half = [(hi[0] - lo[0]) as f64 / 2.0 / view.scale + FRAME, (hi[1] - lo[1]) as f64 / 2.0 / view.scale + FRAME];
                (half, [(lo[0] + hi[0]) as f64 / 2.0, (lo[1] + hi[1]) as f64 / 2.0])
            }
            None => ([0.02, 0.015], [0.0, 0.0]),
        };
        ([view.at[0] - half[0], view.at[1] - half[1]], [view.at[0] + half[0], view.at[1] + half[1]], middle)
    }

    /// The placed view under a paper point, the topmost (last placed) first.
    fn view_at(&self, at: [f64; 2]) -> Option<usize> {
        (0..self.sheet.views.len()).rev().find(|&i| {
            let (lo, hi, _) = self.frame(i);
            (lo[0]..=hi[0]).contains(&at[0]) && (lo[1]..=hi[1]).contains(&at[1])
        })
    }
}

/// The hovered view and the one being dragged, with where it was taken (its middle less the
/// pointer, paper metres).
#[derive(Default)]
struct PaperState {
    hover: Option<usize>,
    drag: Option<(usize, [f64; 2])>,
}

impl canvas::Program<Message> for PaperDrawing {
    type State = PaperState;

    fn update(&self, state: &mut PaperState, event: &iced::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<canvas::Action<Message>> {
        let fit = Fit::of(self.sheet.paper, bounds.size());
        match event {
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let at = cursor.position_in(bounds).map(|p| fit.paper(p));
                if let (Some((i, grip)), Some(at)) = (state.drag, at) {
                    let to = [at[0] + grip[0], at[1] + grip[1]];
                    return Some(canvas::Action::publish(Message::MoveOnSheet(self.key.clone(), i, to)).and_capture());
                }
                let hover = at.and_then(|a| self.view_at(a));
                (hover != state.hover).then(|| {
                    state.hover = hover;
                    canvas::Action::request_redraw()
                })
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let at = fit.paper(cursor.position_in(bounds)?);
                let i = self.view_at(at)?;
                let middle = self.sheet.views[i].at;
                state.drag = Some((i, [middle[0] - at[0], middle[1] - at[1]]));
                Some(canvas::Action::request_redraw().and_capture())
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.drag.take().map(|_| canvas::Action::request_redraw())
            }
            _ => None,
        }
    }

    fn draw(&self, state: &PaperState, renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let p = self.palette;
        let fit = Fit::of(self.sheet.paper, bounds.size());
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
        let rect = |lo: [f64; 2], hi: [f64; 2]| {
            let a = fit.point([lo[0], hi[1]]);
            let b = fit.point([hi[0], lo[1]]);
            (a, Size::new(b.x - a.x, b.y - a.y))
        };
        let paper = if p.dark { Color::from_rgb(0.16, 0.16, 0.18) } else { Color::WHITE };
        let ink = if p.dark { Color::from_rgb(0.9, 0.9, 0.92) } else { Color::from_rgb(0.08, 0.08, 0.1) };
        let faint = Color { a: 0.35, ..p.text_secondary };

        // the paper and its frame (10 mm, 20 mm on the binding side), the title at the lower right
        let [w, h] = self.sheet.paper;
        let (at, size) = rect([0.0, 0.0], [w, h]);
        frame.fill_rectangle(Point::new(at.x + 3.0, at.y + 3.0), size, Color { a: 0.12, ..Color::BLACK });
        frame.fill_rectangle(at, size, paper);
        frame.stroke(&Path::rectangle(at, size), Stroke::default().with_color(faint).with_width(1.0));
        let (inner, inner_size) = rect([0.02, 0.01], [w - 0.01, h - 0.01]);
        frame.stroke(&Path::rectangle(inner, inner_size), Stroke::default().with_color(ink).with_width(1.4));
        let corner = Point::new(inner.x + inner_size.width - 8.0, inner.y + inner_size.height - 8.0);
        label(&mut frame, format!("{}  {}", self.sheet.number, self.sheet.name), corner, 14.0, ink, alignment::Horizontal::Right);

        // the placed floor plans: their cut lines, a frame when hovered or dragged, the title under it
        for (i, view) in self.sheet.views.iter().enumerate() {
            let (lo, hi, middle) = self.frame(i);
            let (at, size) = rect(lo, hi);
            let hot = state.hover == Some(i) || state.drag.is_some_and(|(d, _)| d == i);
            if hot {
                frame.fill_rectangle(at, size, Color { a: 0.06, ..p.accent });
                frame.stroke(&Path::rectangle(at, size), Stroke::default().with_color(p.accent).with_width(1.5));
            }
            let (name, cut) = self.placed.get(i).cloned().unwrap_or_default();
            match cut.filter(|c| !c.segments.is_empty()) {
                Some(cut) => {
                    let on_paper = |q: [f32; 2]| {
                        fit.point([view.at[0] + (q[0] as f64 - middle[0]) / view.scale, view.at[1] + (q[1] as f64 - middle[1]) / view.scale])
                    };
                    let lines = Path::new(|b| {
                        for [a, z] in &cut.segments {
                            b.move_to(on_paper(*a));
                            b.line_to(on_paper(*z));
                        }
                    });
                    frame.stroke(&lines, Stroke::default().with_color(ink).with_width(1.2));
                }
                None => {
                    frame.stroke(&Path::rectangle(at, size), Stroke::default().with_color(faint).with_width(1.0));
                    label(&mut frame, self.empty.clone(), Point::new(at.x + size.width / 2.0, at.y + size.height / 2.0), 11.0, p.text_muted, alignment::Horizontal::Center);
                }
            }
            let under = Point::new(at.x, at.y + size.height + 16.0);
            label(&mut frame, name, under, 12.0, if hot { p.accent } else { ink }, alignment::Horizontal::Left);
            label(&mut frame, format!("1:{}", view.scale.round()), Point::new(at.x, under.y + 14.0), 10.0, p.text_secondary, alignment::Horizontal::Left);
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, state: &PaperState, _bounds: Rectangle, _cursor: mouse::Cursor) -> mouse::Interaction {
        match (state.drag, state.hover) {
            (Some(_), _) => mouse::Interaction::Grabbing,
            (None, Some(_)) => mouse::Interaction::Grab,
            _ => mouse::Interaction::default(),
        }
    }
}

/// The sheets of the document in the explorer's Views tab: each opens in its tab; New sheet.
pub(super) fn sheets_list<'a>(app: &'a App, p: Palette, l: Language, mut body: iced::widget::Column<'a, Message>) -> iced::widget::Column<'a, Message> {
    for sheet in app.document.sheets() {
        let active = app.active_doc_sheet.as_deref() == Some(sheet.path.as_str());
        let title = text(format!("{}  {}", sheet.number, sheet.name)).size(12).color(if active { p.accent } else { p.text }).width(Fill);
        let count = text(sheet.views.len().to_string()).size(11).color(p.text_muted);
        let open = button(row![title, count].spacing(6).align_y(Center))
            .padding([3, 6])
            .width(Fill)
            .style(move |_, s| if active { p.ribbon_button(true, s) } else { p.row_button(s) })
            .on_press(Message::OpenDocSheet(Arc::from(sheet.path.as_str())));
        let close = iced::widget::svg(iced::widget::svg::Handle::from_memory(openaec_ui::icons::Icon::Close.bytes()))
            .width(12)
            .height(12)
            .style(move |_, _| iced::widget::svg::Style { color: Some(p.text_secondary) });
        let remove = button(close).padding(3).style(move |_, s| p.row_button(s)).on_press(Message::RemoveSheet(Arc::from(sheet.path.as_str())));
        body = body.push(container(row![open, remove].spacing(4).align_y(Center)).padding(iced::Padding { top: 0.0, right: 4.0, bottom: 0.0, left: 8.0 }));
    }
    let plus = iced::widget::svg(crate::icons::handle(crate::icons::ADD)).width(12).height(12).style(move |_, _| iced::widget::svg::Style { color: Some(p.accent) });
    body.push(
        button(row![plus, text(l.text("Nieuw blad", "New sheet")).size(12).color(p.text).font(FONT_BOLD)].spacing(6).align_y(Center))
            .padding([4, 8])
            .width(Fill)
            .style(move |_, s| p.row_button(s))
            .on_press(Message::NewSheet(l.text("Blad", "Sheet"))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::DocTab;
    use crate::model::{Layer, Model};
    use std::path::PathBuf;

    /// A room of 8 x 5 m with walls 3 m high, levels at 0 and 3.
    fn room() -> Arc<Model> {
        let positions = vec![0.,0.,0., 8.,0.,0., 8.,5.,0., 0.,5.,0., 0.,0.,3., 8.,0.,3., 8.,5.,3., 0.,5.,3.];
        let indices = vec![0,1,5, 0,5,4, 1,2,6, 1,6,5, 2,3,7, 2,7,6, 3,0,4, 3,4,7];
        let layer = Layer { name: "Walls".into(), positions, indices, ..Layer::default() };
        let mut model = Model::new(PathBuf::from("room.rvt"), "Revit", vec![layer]);
        model.project.levels = vec![("00".into(), 0.0), ("01".into(), 3.0)];
        Arc::new(model)
    }

    fn app_with_plan() -> (App, Arc<str>) {
        let mut app = App::default();
        app.set_model(room());
        let level = Arc::<str>::from(app.document.levels()[0].path.as_str());
        let _ = app.update(Message::NewPlan(level));
        let plan = Arc::<str>::from(app.document.plans()[0].path.as_str());
        (app, plan)
    }

    #[test]
    fn a_new_sheet_opens_in_its_tab_and_takes_a_floor_plan_as_its_cut() {
        let (mut app, plan) = app_with_plan();
        let _ = app.update(Message::NewSheet("Sheet"));
        let sheet = Arc::<str>::from(app.document.sheets()[0].path.as_str());
        assert_eq!(app.doc_tabs(), [DocTab::Model, DocTab::Plan(0), DocTab::DocSheet(0)]);
        assert_eq!(app.active_doc_tab(), 2);
        assert_eq!(app.tab_title(DocTab::DocSheet(0)), "A01 Sheet");
        let _ = app.update(Message::PlaceOnSheet(sheet.clone(), plan.clone()));
        assert_eq!(app.document.sheet(&sheet).unwrap().views.len(), 1);
        // the floor plan as the walls' outline at its cut
        let cut = app.cuts.get(&*plan).expect("cut");
        assert_eq!(cut.extent, Some(([0.0, 0.0], [8.0, 5.0])));
        // moved over the paper
        let _ = app.update(Message::MoveOnSheet(sheet.clone(), 0, [0.1, 0.1]));
        assert_eq!(app.document.sheet(&sheet).unwrap().views[0].at, [0.1, 0.1]);
        // the 3D view's tab, then the sheet's again, then closed
        let _ = app.update(Message::SelectTab(0));
        assert_eq!(app.active_doc_sheet, None);
        let _ = app.update(Message::SelectTab(2));
        assert_eq!(app.active_doc_sheet.as_deref(), Some(&*sheet));
        let _ = app.update(Message::CloseTab(2));
        assert_eq!(app.doc_tabs(), [DocTab::Model, DocTab::Plan(0)]);
        assert_eq!(app.document.sheets().len(), 1, "the sheet stays in the document");
    }

    #[test]
    fn a_drag_on_a_placed_view_moves_it() {
        let (mut app, plan) = app_with_plan();
        let _ = app.update(Message::NewSheet("Sheet"));
        let sheet = Arc::<str>::from(app.document.sheets()[0].path.as_str());
        let _ = app.update(Message::PlaceOnSheet(sheet.clone(), plan.clone()));
        let drawing = PaperDrawing {
            sheet: app.document.sheet(&sheet).unwrap().clone(),
            placed: vec![(String::new(), app.cuts.get(&*plan).cloned())],
            palette: app.theme.palette(),
            key: sheet.clone(),
            empty: String::new(),
        };
        // the frame: the 8 x 5 m room at 1:100, with 5 mm around it, in the middle of A3
        let (lo, hi, middle) = drawing.frame(0);
        assert!((hi[0] - lo[0] - 0.09).abs() < 1e-9 && (hi[1] - lo[1] - 0.06).abs() < 1e-9, "{lo:?} {hi:?}");
        assert_eq!(middle, [4.0, 2.5]);
        assert_eq!(drawing.view_at([0.21, 0.1485]), Some(0));
        assert_eq!(drawing.view_at([0.03, 0.03]), None);
        // press on the view, move 20 mm right: it follows the pointer
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(420.0 * 2.0 + 2.0 * MARGIN, 297.0 * 2.0 + 2.0 * MARGIN));
        let fit = Fit::of(drawing.sheet.paper, bounds.size());
        let mut state = PaperState::default();
        let press = iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let at = fit.point([0.20, 0.15]);
        assert!(canvas::Program::update(&drawing, &mut state, &press, bounds, mouse::Cursor::Available(at)).is_some());
        let moved = iced::Event::Mouse(mouse::Event::CursorMoved { position: Point::ORIGIN });
        let to = fit.point([0.22, 0.15]);
        let action = canvas::Program::update(&drawing, &mut state, &moved, bounds, mouse::Cursor::Available(to)).expect("a move");
        let (message, _, _) = action.into_inner();
        let Some(Message::MoveOnSheet(_, 0, at)) = message else { panic!("{message:?}") };
        assert!((at[0] - 0.23).abs() < 1e-6 && (at[1] - 0.1485).abs() < 1e-6, "{at:?}");
    }
}
