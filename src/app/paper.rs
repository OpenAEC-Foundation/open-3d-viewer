//! The sheets of the project document: their document tabs, the bar to place floor plans on a
//! sheet, and the sheet drawn on a canvas: the paper with its frame and title, and each placed
//! floor plan as the lines of its cut at its scale, which a drag moves over the paper. With the
//! dimension tool, two clicks on a floor plan (snapping to the ends of its lines) add a
//! dimension to that plan, shown wherever it is placed.

use super::{App, Message, FONT_BOLD};
use crate::document::{parse_elevation, Dimension, Sheet, PAPERS, SCALES};
use crate::drawing::{plan_at, Cut};
use crate::theme::Palette;
use iced::alignment;
use iced::keyboard;
use iced::mouse;
use iced::widget::canvas::{self, Frame, Path, Stroke};
use iced::widget::{button, canvas as canvas_widget, column, container, row, text, Row};
use iced::{Center, Color, Element, Fill, Point, Rectangle, Size, Vector};
use openaec_ui::Language;
use std::sync::Arc;

/// Margin around the paper in the workspace (logical pixels).
const MARGIN: f32 = 24.0;
/// Space between a placed view's lines and its frame (paper metres).
const FRAME: f64 = 0.005;
/// How far a dimension line stands off the points it measures (paper metres).
const DIMENSION_OFFSET: f64 = 0.006;
/// How close the pointer must come to the end of a line to snap to it (logical pixels).
const SNAP: f32 = 8.0;
/// How close a click must come to a dimension line to pick it (logical pixels).
const PICK: f32 = 5.0;

/// What can be picked on a sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetPick {
    /// A placed view (its index on the sheet).
    View(usize),
    /// A dimension: the placed view it shows on, its index in the floor plan.
    Dimension(usize, usize),
}

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
        if !self.separate_views {
            self.active_sheet = None;
            self.active_schedule = None;
        }
        if self.active_doc_sheet.as_deref() != Some(path) {
            self.pick_on_sheet(None);
        }
        self.active_doc_sheet = Some(path.to_string());
        self.cut_placed_plans(path);
    }

    /// The floor plan of placed view `index` on the sheet shown.
    fn placed_plan(&self, index: usize) -> Option<String> {
        let sheet = self.document.sheet(self.active_doc_sheet.as_deref()?)?;
        Some(sheet.views.get(index)?.plan.clone())
    }

    /// Picks a placed view or a dimension on the sheet shown; a view's cut height can then be
    /// typed.
    pub(super) fn pick_on_sheet(&mut self, pick: Option<SheetPick>) {
        self.sheet_pick = pick;
        self.cut_edit = match pick {
            Some(SheetPick::View(i)) => self.placed_plan(i).and_then(|plan| {
                let cut = self.document.plan(&plan)?.cut;
                Some((plan, format!("{cut:.3}")))
            }),
            _ => None,
        };
    }

    /// Removes what is picked: a view from the sheet, or a dimension from its floor plan.
    pub(super) fn remove_picked(&mut self) {
        let Some(sheet) = self.active_doc_sheet.clone() else { return };
        match self.sheet_pick {
            Some(SheetPick::View(i)) => {
                self.document.remove_placed(&sheet, i);
            }
            Some(SheetPick::Dimension(i, k)) => {
                if let Some(plan) = self.placed_plan(i) {
                    self.document.remove_dimension(&plan, k);
                }
            }
            None => return,
        }
        self.pick_on_sheet(None);
    }

    /// Uses the typed cut height for its floor plan: its drawing is cut again, and its tab's
    /// section box follows.
    pub(super) fn apply_cut(&mut self) {
        let Some((plan, text)) = self.cut_edit.clone() else { return };
        let Some(cut) = parse_elevation(&text) else { return };
        if !self.document.set_cut(&plan, cut) {
            return;
        }
        self.cuts.remove(&plan);
        if let Some(sheet) = self.active_doc_sheet.clone() {
            self.cut_placed_plans(&sheet);
        }
        self.fit_plan_section(&plan);
        self.cut_edit = Some((plan, format!("{cut:.3}")));
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
            let Some((low, high)) = self.document.plan_range(view) else { continue };
            self.cuts.insert(plan, Arc::new(plan_at(&model, &self.visible, low as f32, high as f32)));
        }
    }

    /// The sheet shown, as its canvas draws it.
    fn paper_drawing(&self, p: Palette, l: Language) -> Option<PaperDrawing> {
        let path = self.active_doc_sheet.as_ref()?;
        let sheet = self.document.sheet(path)?;
        let placed = sheet
            .views
            .iter()
            .map(|v| {
                let plan = self.document.plan(&v.plan);
                Placed {
                    name: plan.map_or(String::new(), |p| p.name.clone()),
                    plan: Arc::from(v.plan.as_str()),
                    cut: self.cuts.get(&v.plan).cloned(),
                    dimensions: plan.map_or(Vec::new(), |p| p.dimensions.clone()),
                    crop: plan.and_then(|p| p.crop),
                }
            })
            .collect();
        Some(PaperDrawing {
            sheet: sheet.clone(),
            placed,
            palette: p,
            key: Arc::from(path.as_str()),
            tool: self.dimension_tool,
            pick: self.sheet_pick,
            empty: l.text("niets gesneden", "nothing cut").to_string(),
        })
    }

    /// The workspace of a sheet's tab: the bar to place floor plans and to dimension, then the
    /// sheet.
    pub(super) fn doc_sheet_workspace(&self, p: Palette, l: Language) -> Option<Element<'_, Message>> {
        let path = self.active_doc_sheet.as_ref()?;
        let sheet = self.document.sheet(path)?;
        let key: Arc<str> = Arc::from(path.as_str());
        let tool = self.dimension_tool;
        let measure = button(text(l.text("Maatvoering", "Dimension")).size(12))
            .padding([3, 10])
            .style(move |_, s| p.ribbon_button(tool, s))
            .on_press(Message::DimensionTool(!tool));
        let mut bar = Row::new().spacing(6).align_y(Center).push(measure).push(text("|").size(12).color(p.text_muted));
        let small = |label: String, active: bool, message: Message| {
            button(text(label).size(12)).padding([3, 8]).style(move |_, s| p.ribbon_button(active, s)).on_press(message)
        };
        let picked_view = match self.sheet_pick {
            Some(SheetPick::View(i)) => sheet.views.get(i).map(|v| (i, v)),
            _ => None,
        };
        if tool {
            let hint = l.text("Klik twee punten op een plattegrond; Esc breekt af.", "Click two points on a floor plan; Esc cancels.");
            bar = bar.push(text(hint).size(12).color(p.text_secondary));
        } else if let Some((i, view)) = picked_view {
            bar = bar.push(text(l.text("Schaal", "Scale")).size(12).color(p.text_secondary));
            for scale in SCALES {
                bar = bar.push(small(format!("1:{scale}"), view.scale == scale, Message::SetScale(key.clone(), i, scale)));
            }
            if let Some((_, typed)) = &self.cut_edit {
                bar = bar.push(text(l.text("Snijhoogte (m)", "Cut height (m)")).size(12).color(p.text_secondary)).push(
                    iced::widget::text_input("1.200", typed).on_input(Message::CutText).on_submit(Message::ApplyCut).size(12).padding([3, 6]).width(64),
                );
            }
            let cropped = self.document.plan(&view.plan).filter(|p| p.crop.is_some());
            if cropped.is_some() {
                bar = bar.push(small(l.text("Bijsnijden opheffen", "Show all").to_string(), false, Message::CropPlan(Arc::from(view.plan.as_str()), None)));
            } else {
                bar = bar.push(text(l.text("sleep een rand om bij te snijden", "drag a side to crop")).size(12).color(p.text_muted));
            }
            bar = bar.push(small(l.text("Van blad halen", "Take off sheet").to_string(), false, Message::RemovePicked));
        } else if let Some(SheetPick::Dimension(..)) = self.sheet_pick {
            bar = bar.push(small(l.text("Maat verwijderen", "Remove dimension").to_string(), false, Message::RemovePicked));
        } else {
            bar = bar.push(text(l.text("Papier", "Paper")).size(12).color(p.text_secondary));
            for (name, size) in PAPERS {
                bar = bar.push(small(name.to_string(), sheet.paper == size, Message::SetPaper(key.clone(), size)));
            }
            bar = bar.push(text("|").size(12).color(p.text_muted));
            bar = bar.push(text(l.text("Plaats op dit blad:", "Place on this sheet:")).size(12).color(p.text_secondary));
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
        }
        let drawing = self.paper_drawing(p, l)?;
        Some(
            column![
                container(bar).padding([6, 10]).width(Fill).style(move |_| p.ribbon()),
                canvas_widget(drawing).width(Fill).height(Fill),
            ]
            .into(),
        )
    }
}

/// A floor plan on the sheet as the canvas draws it.
#[derive(Debug, Clone, Default)]
struct Placed {
    name: String,
    /// The floor plan's path.
    plan: Arc<str>,
    cut: Option<Arc<Cut>>,
    dimensions: Vec<Dimension>,
    crop: Option<([f64; 2], [f64; 2])>,
}

/// A side of a crop: left, right, bottom, top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
    Bottom,
    Top,
}

/// The part of segment `a`–`b` inside the rectangle `lo`–`hi` (Liang–Barsky).
fn clip(a: [f64; 2], b: [f64; 2], lo: [f64; 2], hi: [f64; 2]) -> Option<([f64; 2], [f64; 2])> {
    let d = [b[0] - a[0], b[1] - a[1]];
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [(-d[0], a[0] - lo[0]), (d[0], hi[0] - a[0]), (-d[1], a[1] - lo[1]), (d[1], hi[1] - a[1])] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
        }
    }
    (t0 <= t1).then(|| ([a[0] + t0 * d[0], a[1] + t0 * d[1]], [a[0] + t1 * d[0], a[1] + t1 * d[1]]))
}
struct PaperDrawing {
    sheet: Sheet,
    /// Per placed view, in the order of the sheet's views.
    placed: Vec<Placed>,
    palette: Palette,
    /// The sheet's path, for the messages.
    key: Arc<str>,
    /// The dimension tool is on: clicks measure instead of dragging.
    tool: bool,
    /// What is picked.
    pick: Option<SheetPick>,
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
        let middle = self.middle(i);
        // a crop is the frame itself; the whole cut gets a margin
        if let Some((lo, hi)) = self.placed.get(i).and_then(|p| p.crop) {
            return (self.to_paper(i, lo), self.to_paper(i, hi), middle);
        }
        let extent = self.placed.get(i).and_then(|p| p.cut.as_ref()?.extent);
        let half = match extent {
            Some((lo, hi)) => [(hi[0] - lo[0]) as f64 / 2.0 / view.scale + FRAME, (hi[1] - lo[1]) as f64 / 2.0 / view.scale + FRAME],
            None => [0.02, 0.015],
        };
        ([view.at[0] - half[0], view.at[1] - half[1]], [view.at[0] + half[0], view.at[1] + half[1]], middle)
    }

    /// The model point that placed view `i`'s position on the paper stands for: the middle of
    /// its whole cut, so cropping never moves what is drawn.
    fn middle(&self, i: usize) -> [f64; 2] {
        match self.placed.get(i).and_then(|p| p.cut.as_ref()?.extent) {
            Some((lo, hi)) => [(lo[0] + hi[0]) as f64 / 2.0, (lo[1] + hi[1]) as f64 / 2.0],
            None => [0.0, 0.0],
        }
    }

    /// A model point (x, y) of placed view `i` on the paper.
    fn to_paper(&self, i: usize, q: [f64; 2]) -> [f64; 2] {
        let view = &self.sheet.views[i];
        let middle = self.middle(i);
        [view.at[0] + (q[0] - middle[0]) / view.scale, view.at[1] + (q[1] - middle[1]) / view.scale]
    }

    /// A paper point in the model of placed view `i`.
    fn to_model(&self, i: usize, at: [f64; 2]) -> [f64; 2] {
        let view = &self.sheet.views[i];
        let middle = self.middle(i);
        [middle[0] + (at[0] - view.at[0]) * view.scale, middle[1] + (at[1] - view.at[1]) * view.scale]
    }

    /// The crop handles of placed view `i`: the middles of its frame's sides (paper).
    fn handles(&self, i: usize) -> [(Side, [f64; 2]); 4] {
        let (lo, hi, _) = self.frame(i);
        let (mx, my) = ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0);
        [(Side::Left, [lo[0], my]), (Side::Right, [hi[0], my]), (Side::Bottom, [mx, lo[1]]), (Side::Top, [mx, hi[1]])]
    }

    /// The crop handle of the picked view under `pointer` (within [`SNAP`] pixels).
    fn handle_at(&self, fit: &Fit, pointer: Point) -> Option<(usize, Side)> {
        let Some(SheetPick::View(i)) = self.pick else { return None };
        (i < self.sheet.views.len()).then_some(())?;
        self.handles(i).into_iter().find(|(_, at)| fit.point(*at).distance(pointer) <= SNAP).map(|(side, _)| (i, side))
    }

    /// The crop of placed view `i` with `side` moved to the paper point `at` (model, at least
    /// 10 cm each way).
    fn crop_to(&self, i: usize, side: Side, at: [f64; 2]) -> ([f64; 2], [f64; 2]) {
        let (lo, hi, _) = self.frame(i);
        let (mut lo, mut hi) = (self.to_model(i, lo), self.to_model(i, hi));
        let q = self.to_model(i, at);
        match side {
            Side::Left => lo[0] = q[0].min(hi[0] - 0.1),
            Side::Right => hi[0] = q[0].max(lo[0] + 0.1),
            Side::Bottom => lo[1] = q[1].min(hi[1] - 0.1),
            Side::Top => hi[1] = q[1].max(lo[1] + 0.1),
        }
        (lo, hi)
    }

    /// The model point a click at `at` (paper) on view `i` means: the nearest end of a cut line
    /// within [`SNAP`] pixels, else the point itself.
    fn snap(&self, i: usize, at: [f64; 2], fit: &Fit) -> [f64; 2] {
        let pointer = fit.point(at);
        let ends = self.placed.get(i).and_then(|p| p.cut.as_ref()).into_iter().flat_map(|c| c.segments.iter().flatten());
        ends.map(|q| [q[0] as f64, q[1] as f64])
            .map(|q| (q, fit.point(self.to_paper(i, q)).distance(pointer)))
            .filter(|(_, d)| *d <= SNAP)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map_or_else(|| self.to_model(i, at), |(q, _)| q)
    }

    /// The paper ends of the dimension line of dimension `k` on placed view `i`.
    fn dimension_line(&self, i: usize, k: usize) -> Option<([f64; 2], [f64; 2])> {
        let d = self.placed.get(i)?.dimensions.get(k)?;
        let (from, to) = (self.to_paper(i, d.from), self.to_paper(i, d.to));
        let run = (to[0] - from[0]).hypot(to[1] - from[1]);
        (run > 0.0).then(|| {
            let n = [-(to[1] - from[1]) / run * DIMENSION_OFFSET, (to[0] - from[0]) / run * DIMENSION_OFFSET];
            ([from[0] + n[0], from[1] + n[1]], [to[0] + n[0], to[1] + n[1]])
        })
    }

    /// The dimension whose line passes within [`PICK`] pixels of `pointer`.
    fn dimension_at(&self, fit: &Fit, pointer: Point) -> Option<(usize, usize)> {
        let near = |a: Point, b: Point| {
            let (ab, ap) = (b - a, pointer - a);
            let t = ((ap.x * ab.x + ap.y * ab.y) / (ab.x * ab.x + ab.y * ab.y).max(1e-6)).clamp(0.0, 1.0);
            (a + ab * t).distance(pointer)
        };
        (0..self.placed.len()).rev().find_map(|i| {
            (0..self.placed[i].dimensions.len()).find_map(|k| {
                let (a, b) = self.dimension_line(i, k)?;
                (near(fit.point(a), fit.point(b)) <= PICK).then_some((i, k))
            })
        })
    }

    /// The placed view under a paper point, the topmost (last placed) first.
    fn view_at(&self, at: [f64; 2]) -> Option<usize> {
        (0..self.sheet.views.len()).rev().find(|&i| {
            let (lo, hi, _) = self.frame(i);
            (lo[0]..=hi[0]).contains(&at[0]) && (lo[1]..=hi[1]).contains(&at[1])
        })
    }
}

/// The hovered view, the one being dragged with where it was taken (its middle less the
/// pointer, paper metres), and for the dimension tool the first point (view, model point) and
/// the point the pointer snaps to now.
#[derive(Default)]
struct PaperState {
    hover: Option<usize>,
    drag: Option<(usize, [f64; 2])>,
    first: Option<(usize, [f64; 2])>,
    aim: Option<(usize, [f64; 2])>,
    /// A crop handle being dragged: the view and the side.
    crop: Option<(usize, Side)>,
}

impl canvas::Program<Message> for PaperDrawing {
    type State = PaperState;

    fn update(&self, state: &mut PaperState, event: &iced::Event, bounds: Rectangle, cursor: mouse::Cursor) -> Option<canvas::Action<Message>> {
        let fit = Fit::of(self.sheet.paper, bounds.size());
        if !self.tool {
            state.first = None;
            state.aim = None;
        }
        match event {
            iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let at = cursor.position_in(bounds).map(|p| fit.paper(p));
                if let (Some((i, side)), Some(at)) = (state.crop, at) {
                    let plan = self.placed.get(i)?.plan.clone();
                    return Some(canvas::Action::publish(Message::CropPlan(plan, Some(self.crop_to(i, side, at)))).and_capture());
                }
                if let (Some((i, grip)), Some(at)) = (state.drag, at) {
                    let to = [at[0] + grip[0], at[1] + grip[1]];
                    return Some(canvas::Action::publish(Message::MoveOnSheet(self.key.clone(), i, to)).and_capture());
                }
                let hover = at.and_then(|a| self.view_at(a));
                if self.tool {
                    // on the view of the first point, else on the view under the pointer
                    let view = state.first.map(|(i, _)| i).or(hover);
                    state.aim = view.zip(at).map(|(i, a)| (i, self.snap(i, a, &fit)));
                    state.hover = hover;
                    return Some(canvas::Action::request_redraw());
                }
                (hover != state.hover).then(|| {
                    state.hover = hover;
                    canvas::Action::request_redraw()
                })
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let at = fit.paper(cursor.position_in(bounds)?);
                if self.tool {
                    let i = state.first.map(|(i, _)| i).or_else(|| self.view_at(at))?;
                    let point = self.snap(i, at, &fit);
                    return match state.first.take() {
                        None => {
                            state.first = Some((i, point));
                            Some(canvas::Action::request_redraw().and_capture())
                        }
                        Some((_, from)) => {
                            let plan = self.placed.get(i)?.plan.clone();
                            Some(canvas::Action::publish(Message::AddDimension(plan, from, point)).and_capture())
                        }
                    };
                }
                // a crop handle of the picked view, a dimension, else a view (which a drag then
                // moves), else nothing
                if let Some(handle) = self.handle_at(&fit, fit.point(at)) {
                    state.crop = Some(handle);
                    return Some(canvas::Action::request_redraw().and_capture());
                }
                if let Some((i, k)) = self.dimension_at(&fit, fit.point(at)) {
                    return Some(canvas::Action::publish(Message::PickOnSheet(Some(SheetPick::Dimension(i, k)))).and_capture());
                }
                let Some(i) = self.view_at(at) else {
                    return self.pick.is_some().then(|| canvas::Action::publish(Message::PickOnSheet(None)));
                };
                let middle = self.sheet.views[i].at;
                state.drag = Some((i, [middle[0] - at[0], middle[1] - at[1]]));
                Some(canvas::Action::publish(Message::PickOnSheet(Some(SheetPick::View(i)))).and_capture())
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key: keyboard::Key::Named(keyboard::key::Named::Delete | keyboard::key::Named::Backspace), .. })
                if self.pick.is_some() && !self.tool =>
            {
                Some(canvas::Action::publish(Message::RemovePicked).and_capture())
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                let crop = state.crop.take();
                state.drag.take().or(crop.map(|(i, _)| (i, [0.0; 2]))).map(|_| canvas::Action::request_redraw())
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key: keyboard::Key::Named(keyboard::key::Named::Escape), .. }) => {
                if state.first.take().is_some() {
                    return Some(canvas::Action::request_redraw().and_capture());
                }
                self.tool.then(|| canvas::Action::publish(Message::DimensionTool(false)).and_capture())
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

        // the placed floor plans: their cut lines and dimensions, a frame when hovered or
        // dragged, the title under it
        for (i, view) in self.sheet.views.iter().enumerate() {
            let (lo, hi, _) = self.frame(i);
            let (at, size) = rect(lo, hi);
            let picked = self.pick == Some(SheetPick::View(i));
            let hot = state.hover == Some(i) || state.drag.is_some_and(|(d, _)| d == i) || picked;
            if hot && !self.tool {
                frame.fill_rectangle(at, size, Color { a: 0.06, ..p.accent });
                frame.stroke(&Path::rectangle(at, size), Stroke::default().with_color(p.accent).with_width(1.5));
            }
            let placed = self.placed.get(i).cloned().unwrap_or_default();
            match placed.cut.filter(|c| !c.segments.is_empty() || !c.below.is_empty()) {
                Some(cut) => {
                    // what is seen below the cut, thin and grey, then the cut itself
                    // within the crop, when there is one
                    let crop = placed.crop;
                    let piece = |a: &[f32; 2], z: &[f32; 2]| {
                        let (a, z) = ([a[0] as f64, a[1] as f64], [z[0] as f64, z[1] as f64]);
                        match crop {
                            Some((lo, hi)) => clip(a, z, lo, hi),
                            None => Some((a, z)),
                        }
                    };
                    let below = Path::new(|b| {
                        for (a, z) in cut.below.iter().filter_map(|[a, z]| piece(a, z)) {
                            b.move_to(fit.point(self.to_paper(i, a)));
                            b.line_to(fit.point(self.to_paper(i, z)));
                        }
                    });
                    frame.stroke(&below, Stroke::default().with_color(Color { a: 0.55, ..ink }).with_width(0.6));
                    let lines = Path::new(|b| {
                        for (a, z) in cut.segments.iter().filter_map(|[a, z]| piece(a, z)) {
                            b.move_to(fit.point(self.to_paper(i, a)));
                            b.line_to(fit.point(self.to_paper(i, z)));
                        }
                    });
                    frame.stroke(&lines, Stroke::default().with_color(ink).with_width(1.2));
                    if crop.is_some() {
                        frame.stroke(&Path::rectangle(at, size), Stroke::default().with_color(faint).with_width(0.8));
                    }
                }
                None => {
                    frame.stroke(&Path::rectangle(at, size), Stroke::default().with_color(faint).with_width(1.0));
                    label(&mut frame, self.empty.clone(), Point::new(at.x + size.width / 2.0, at.y + size.height / 2.0), 11.0, p.text_muted, alignment::Horizontal::Center);
                }
            }
            for (k, d) in placed.dimensions.iter().enumerate() {
                let color = if self.pick == Some(SheetPick::Dimension(i, k)) { p.accent } else { ink };
                draw_dimension(&mut frame, &fit, self.to_paper(i, d.from), self.to_paper(i, d.to), d.length(), color);
            }
            let under = Point::new(at.x, at.y + size.height + 16.0);
            label(&mut frame, placed.name, under, 12.0, if hot && !self.tool { p.accent } else { ink }, alignment::Horizontal::Left);
            label(&mut frame, format!("1:{}", view.scale.round()), Point::new(at.x, under.y + 14.0), 10.0, p.text_secondary, alignment::Horizontal::Left);
        }

        // the crop handles of the picked view
        if let Some(SheetPick::View(i)) = self.pick.filter(|_| !self.tool) {
            if i < self.sheet.views.len() {
                for (_, at) in self.handles(i) {
                    let c = fit.point(at);
                    frame.fill_rectangle(Point::new(c.x - 4.0, c.y - 4.0), Size::new(8.0, 8.0), paper);
                    frame.stroke(&Path::rectangle(Point::new(c.x - 4.0, c.y - 4.0), Size::new(8.0, 8.0)), Stroke::default().with_color(p.accent).with_width(1.5));
                }
            }
        }

        // the dimension being made: from the first point to where the pointer snaps
        if self.tool {
            if let Some((i, q)) = state.aim {
                let c = fit.point(self.to_paper(i, q));
                frame.stroke(&Path::circle(c, 4.0), Stroke::default().with_color(p.accent).with_width(1.5));
                if let Some((j, from)) = state.first.filter(|(j, _)| *j == i) {
                    let length = Dimension { from, to: q }.length();
                    draw_dimension(&mut frame, &fit, self.to_paper(j, from), self.to_paper(i, q), length, p.accent);
                }
            }
            if let Some((i, from)) = state.first {
                frame.fill(&Path::circle(fit.point(self.to_paper(i, from)), 3.0), p.accent);
            }
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, state: &PaperState, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        let fit = Fit::of(self.sheet.paper, bounds.size());
        let handle = state.crop.map(|(_, side)| side).or_else(|| cursor.position_in(bounds).and_then(|p| self.handle_at(&fit, p)).map(|(_, side)| side));
        match handle.filter(|_| !self.tool) {
            Some(Side::Left | Side::Right) => return mouse::Interaction::ResizingHorizontally,
            Some(Side::Bottom | Side::Top) => return mouse::Interaction::ResizingVertically,
            None => {}
        }
        match (self.tool, state.drag, state.hover) {
            (true, _, _) => mouse::Interaction::Crosshair,
            (false, Some(_), _) => mouse::Interaction::Grabbing,
            (false, None, Some(_)) => mouse::Interaction::Grab,
            _ => mouse::Interaction::default(),
        }
    }
}

/// A dimension between two paper points: the dimension line [`DIMENSION_OFFSET`] to the left of
/// from → to, extension lines from the points to just past it, a slash at each end, and the
/// length in millimetres over the middle.
fn draw_dimension(frame: &mut Frame, fit: &Fit, from: [f64; 2], to: [f64; 2], length: f64, color: Color) {
    let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
    let run = dx.hypot(dy);
    if run <= 0.0 {
        return;
    }
    let (ux, uy) = (dx / run, dy / run);
    let (nx, ny) = (-uy, ux);
    let off = |q: [f64; 2], d: f64| [q[0] + nx * d, q[1] + ny * d];
    let (a, b) = (off(from, DIMENSION_OFFSET), off(to, DIMENSION_OFFSET));
    let tick = 0.0012;
    let slash = |q: [f64; 2]| ([q[0] - (ux + nx) * tick, q[1] - (uy + ny) * tick], [q[0] + (ux + nx) * tick, q[1] + (uy + ny) * tick]);
    let lines = Path::new(|path| {
        for (q, end) in [(from, a), (to, b)] {
            path.move_to(fit.point(off(q, 0.0015)));
            path.line_to(fit.point(off(end, 0.0015)));
        }
        path.move_to(fit.point(a));
        path.line_to(fit.point(b));
        for q in [a, b] {
            let (s, e) = slash(q);
            path.move_to(fit.point(s));
            path.line_to(fit.point(e));
        }
    });
    frame.stroke(&lines, Stroke::default().with_color(color).with_width(0.9));
    let middle = fit.point(off([(from[0] + to[0]) / 2.0, (from[1] + to[1]) / 2.0], DIMENSION_OFFSET + 0.0015));
    frame.fill_text(canvas::Text {
        content: format!("{}", (length * 1000.0).round()),
        position: middle + Vector::new(0.0, 0.0),
        align_x: alignment::Horizontal::Center.into(),
        align_y: alignment::Vertical::Center,
        size: iced::Pixels(10.0),
        color,
        font: openaec_ui::fonts::MEDIUM,
        ..canvas::Text::default()
    });
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

    /// The app with a sheet holding the plan, and the sheet's canvas program.
    fn sheet_with_plan(tool: bool) -> (App, Arc<str>, Arc<str>, PaperDrawing) {
        let (mut app, plan) = app_with_plan();
        let _ = app.update(Message::NewSheet("Sheet"));
        let sheet = Arc::<str>::from(app.document.sheets()[0].path.as_str());
        let _ = app.update(Message::PlaceOnSheet(sheet.clone(), plan.clone()));
        app.dimension_tool = tool;
        let drawing = app.paper_drawing(app.theme.palette(), Language::English).unwrap();
        (app, sheet, plan, drawing)
    }

    #[test]
    fn a_picked_view_is_scaled_cut_again_and_taken_off() {
        let (mut app, sheet, plan, drawing) = sheet_with_plan(false);
        let fit = Fit::of(drawing.sheet.paper, bounds().size());
        let mut state = PaperState::default();
        let message = press_at(&drawing, &mut state, fit.point([0.21, 0.1485])).expect("a pick");
        assert!(matches!(message, Message::PickOnSheet(Some(SheetPick::View(0)))), "{message:?}");
        let _ = app.update(message);
        assert_eq!(app.cut_edit, Some((plan.to_string(), "1.200".to_string())));
        let _ = app.update(Message::SetScale(sheet.clone(), 0, 50.0));
        assert_eq!(app.document.sheet(&sheet).unwrap().views[0].scale, 50.0);
        // the cut height: the plan is cut again, and the floor plan's section box follows
        let _ = app.update(Message::CutText("2,5".into()));
        let _ = app.update(Message::ApplyCut);
        assert_eq!(app.document.plan(&plan).unwrap().cut, 2.5);
        assert!(app.cuts.contains_key(&*plan));
        assert_eq!(app.active_plan.as_deref(), Some(&*plan));
        assert!((app.section.unwrap().max.z - 2.5).abs() < 1e-6);
        // the paper
        let _ = app.update(Message::PickOnSheet(None));
        let _ = app.update(Message::SetPaper(sheet.clone(), PAPERS[3].1));
        assert_eq!(app.document.sheet(&sheet).unwrap().paper, [0.841, 0.594]);
        // taken off with Delete
        let _ = app.update(Message::PickOnSheet(Some(SheetPick::View(0))));
        let drawing = app.paper_drawing(app.theme.palette(), Language::English).unwrap();
        let delete = iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Delete),
            modified_key: keyboard::Key::Named(keyboard::key::Named::Delete),
            physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::default(),
            text: None,
            repeat: false,
        });
        let action = canvas::Program::update(&drawing, &mut PaperState::default(), &delete, bounds(), mouse::Cursor::Unavailable).expect("delete");
        let _ = app.update(action.into_inner().0.expect("a message"));
        assert!(app.document.sheet(&sheet).unwrap().views.is_empty());
        assert_eq!(app.sheet_pick, None);
    }

    #[test]
    fn dragging_a_side_of_a_picked_view_crops_its_floor_plan() {
        let (mut app, _, plan, _) = sheet_with_plan(false);
        let _ = app.update(Message::PickOnSheet(Some(SheetPick::View(0))));
        let drawing = app.paper_drawing(app.theme.palette(), Language::English).unwrap();
        let fit = Fit::of(drawing.sheet.paper, bounds().size());
        // the right handle, dragged to x = 5 m of the 8 m room
        let (side, at) = drawing.handles(0)[1];
        assert_eq!(side, Side::Right);
        let mut state = PaperState::default();
        assert!(press_at(&drawing, &mut state, fit.point(at)).is_none());
        assert_eq!(state.crop, Some((0, Side::Right)));
        let moved = iced::Event::Mouse(mouse::Event::CursorMoved { position: Point::ORIGIN });
        let to = fit.point(drawing.to_paper(0, [5.0, 2.5]));
        let action = canvas::Program::update(&drawing, &mut state, &moved, bounds(), mouse::Cursor::Available(to)).expect("a crop");
        let message = action.into_inner().0.expect("a message");
        let Message::CropPlan(_, Some((lo, hi))) = &message else { panic!("{message:?}") };
        assert!((hi[0] - 5.0).abs() < 0.01 && hi[0] > lo[0], "{lo:?} {hi:?}");
        let _ = app.update(message);
        let crop = app.document.plan(&plan).unwrap().crop.expect("cropped");
        assert!((crop.1[0] - 5.0).abs() < 0.01);
        // what is drawn stays where it was; the frame is the crop
        let cropped = app.paper_drawing(app.theme.palette(), Language::English).unwrap();
        assert_eq!(cropped.to_paper(0, [0.0, 0.0]), drawing.to_paper(0, [0.0, 0.0]));
        let (lo, hi, _) = cropped.frame(0);
        assert!((hi[0] - cropped.to_paper(0, [5.0, 0.0])[0]).abs() < 1e-6 && lo[0] < hi[0]);
        // the floor plan's tab shows the crop too
        assert!((app.section.unwrap().max.x - 5.0).abs() < 0.01);
        assert_eq!(clip([0.0, 0.0], [10.0, 0.0], [1.0, -1.0], [5.0, 1.0]), Some(([1.0, 0.0], [5.0, 0.0])));
        assert_eq!(clip([0.0, 2.0], [10.0, 2.0], [1.0, -1.0], [5.0, 1.0]), None);
        // shown whole again
        let _ = app.update(Message::CropPlan(plan.clone(), None));
        assert_eq!(app.document.plan(&plan).unwrap().crop, None);
    }

    #[test]
    fn a_click_on_a_dimension_picks_it_for_removal() {
        let (mut app, _, plan, _) = sheet_with_plan(false);
        let _ = app.update(Message::AddDimension(plan.clone(), [0.0, 0.0], [8.0, 0.0]));
        let drawing = app.paper_drawing(app.theme.palette(), Language::English).unwrap();
        let fit = Fit::of(drawing.sheet.paper, bounds().size());
        let (a, b) = drawing.dimension_line(0, 0).expect("a dimension line");
        let on_line = fit.point([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]) + Vector::new(0.0, 2.0);
        let message = press_at(&drawing, &mut PaperState::default(), on_line).expect("a pick");
        assert!(matches!(message, Message::PickOnSheet(Some(SheetPick::Dimension(0, 0)))), "{message:?}");
        let _ = app.update(message);
        let _ = app.update(Message::RemovePicked);
        assert!(app.document.plan(&plan).unwrap().dimensions.is_empty());
    }

    /// The canvas at 2 pixels per paper millimetre.
    fn bounds() -> Rectangle {
        Rectangle::new(Point::ORIGIN, Size::new(420.0 * 2.0 + 2.0 * MARGIN, 297.0 * 2.0 + 2.0 * MARGIN))
    }

    fn press_at(drawing: &PaperDrawing, state: &mut PaperState, at: Point) -> Option<Message> {
        let press = iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let action = canvas::Program::update(drawing, state, &press, bounds(), mouse::Cursor::Available(at))?;
        action.into_inner().0
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
        let (_, _, _, drawing) = sheet_with_plan(false);
        // the frame: the 8 x 5 m room at 1:100, with 5 mm around it, in the middle of A3
        let (lo, hi, middle) = drawing.frame(0);
        assert!((hi[0] - lo[0] - 0.09).abs() < 1e-9 && (hi[1] - lo[1] - 0.06).abs() < 1e-9, "{lo:?} {hi:?}");
        assert_eq!(middle, [4.0, 2.5]);
        assert_eq!(drawing.view_at([0.21, 0.1485]), Some(0));
        assert_eq!(drawing.view_at([0.03, 0.03]), None);
        // press on the view (which picks it), move 20 mm right: it follows the pointer
        let fit = Fit::of(drawing.sheet.paper, bounds().size());
        let mut state = PaperState::default();
        let picked = press_at(&drawing, &mut state, fit.point([0.20, 0.15]));
        assert!(matches!(picked, Some(Message::PickOnSheet(Some(SheetPick::View(0))))), "{picked:?}");
        let moved = iced::Event::Mouse(mouse::Event::CursorMoved { position: Point::ORIGIN });
        let action = canvas::Program::update(&drawing, &mut state, &moved, bounds(), mouse::Cursor::Available(fit.point([0.22, 0.15]))).expect("a move");
        let Some(Message::MoveOnSheet(_, 0, at)) = action.into_inner().0 else { panic!("no move") };
        assert!((at[0] - 0.23).abs() < 1e-6 && (at[1] - 0.1485).abs() < 1e-6, "{at:?}");
    }

    #[test]
    fn two_clicks_with_the_dimension_tool_measure_between_snapped_corners() {
        let (mut app, sheet, plan, drawing) = sheet_with_plan(true);
        let fit = Fit::of(drawing.sheet.paper, bounds().size());
        // a few pixels off the room's lower left and lower right corners: they snap
        let corner = |q: [f64; 2]| fit.point(drawing.to_paper(0, q)) + Vector::new(3.0, -2.0);
        let mut state = PaperState::default();
        assert!(press_at(&drawing, &mut state, corner([0.0, 0.0])).is_none());
        assert_eq!(state.first, Some((0, [0.0, 0.0])));
        let message = press_at(&drawing, &mut state, corner([8.0, 0.0])).expect("a dimension");
        assert!(matches!(&message, Message::AddDimension(p, [0.0, 0.0], [8.0, 0.0]) if *p == plan), "{message:?}");
        let _ = app.update(message);
        let dimensions = &app.document.plan(&plan).unwrap().dimensions;
        assert_eq!(dimensions.len(), 1);
        assert_eq!(dimensions[0].length(), 8.0);
        // Escape drops a first point, then switches the tool off
        let mut state = PaperState::default();
        let _ = press_at(&drawing, &mut state, corner([0.0, 0.0]));
        let escape = iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(keyboard::key::Named::Escape),
            modified_key: keyboard::Key::Named(keyboard::key::Named::Escape),
            physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::default(),
            text: None,
            repeat: false,
        });
        let _ = canvas::Program::update(&drawing, &mut state, &escape, bounds(), mouse::Cursor::Unavailable);
        assert_eq!(state.first, None);
        let action = canvas::Program::update(&drawing, &mut state, &escape, bounds(), mouse::Cursor::Unavailable).expect("off");
        assert!(matches!(action.into_inner().0, Some(Message::DimensionTool(false))));
        let _ = app.update(Message::DimensionTool(true));
        assert!(app.dimension_tool);
        let _ = sheet;
    }
}
