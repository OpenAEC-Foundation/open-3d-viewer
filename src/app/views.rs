//! The explorer's Views tab: the floor plans of the project document, made per level and shown
//! from above, cut at their height above the level.

use super::{App, Message, FONT_BOLD};
use crate::model::{PlanRange, SavedView, ViewKind};
use crate::section::SectionBox;
use crate::theme::Palette;
use crate::viewport::camera::Camera;
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, scrollable, svg, text, Column};
use iced::{Center, Element, Fill, Padding, Task};
use openaec_ui::Language;
use std::sync::Arc;

/// What a view's tab keeps while another tab shows: its camera and section box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewSlot {
    pub camera: Camera,
    pub section: Option<SectionBox>,
    pub section_on: bool,
}

/// A floor plan open in its own document tab.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenPlan {
    pub path: String,
    /// Its camera and section box while another tab shows.
    pub slot: ViewSlot,
}

/// A document tab: the 3D view (always first), floor plans, schedules, sheets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocTab {
    Model,
    /// Index in `App::open_plans`.
    Plan(usize),
    /// Index in `App::schedules`.
    Schedule(usize),
    /// Index in the model's sheets (read from the file).
    Sheet(usize),
    /// Index in `App::open_doc_sheets`: a sheet of the document.
    DocSheet(usize),
}

impl App {
    /// Makes a floor plan of a level and shows it in its tab.
    pub(super) fn new_plan(&mut self, level: &str) {
        if let Some(plan) = self.document.add_plan(level) {
            self.open_plan(&plan);
        }
    }

    /// Shows a floor plan in its tab, opening the tab the first time: from above, orthographic,
    /// the section box from just under its level up to its cut.
    pub(super) fn open_plan(&mut self, path: &str) {
        self.active_doc_sheet = None;
        if self.separate_views {
            return self.open_plan_apart(path);
        }
        if self.open_plans.iter().any(|o| o.path == path) {
            self.active_sheet = None;
            self.active_schedule = None;
            self.load_view(Some(path.to_string()));
            return;
        }
        let Some(plan) = self.document.plan(path) else { return };
        let Some((low, high)) = self.document.plan_range(plan) else { return };
        let view = SavedView {
            name: plan.name.clone(),
            kind: ViewKind::Plan,
            camera: None,
            clip: None,
            plan: Some(PlanRange { low: low as f32, high: high as f32, from_below: false }),
            level: self.document.level(&plan.level).map(|l| l.name.clone()),
        };
        self.active_sheet = None;
        self.active_schedule = None;
        let slot = self.view_slot();
        self.open_plans.push(OpenPlan { path: path.to_string(), slot });
        self.load_view(Some(path.to_string()));
        self.show_view(&view);
    }

    /// A floor plan in the views window: its tab opens the first time, set up with its own camera
    /// and section box, apart from the 3D view's.
    fn open_plan_apart(&mut self, path: &str) {
        if !self.open_plans.iter().any(|o| o.path == path) {
            let Some(plan) = self.document.plan(path) else { return };
            let Some((low, high)) = self.document.plan_range(plan) else { return };
            let view = SavedView {
                name: plan.name.clone(),
                kind: ViewKind::Plan,
                camera: None,
                clip: None,
                plan: Some(PlanRange { low: low as f32, high: high as f32, from_below: false }),
                level: self.document.level(&plan.level).map(|l| l.name.clone()),
            };
            let slot = ViewSlot { camera: self.camera, section: None, section_on: false };
            self.open_plans.push(OpenPlan { path: path.to_string(), slot });
            // framed in the plan window's viewport (the 3D view's until it has its own size)
            if self.plan_viewport.0 .0 <= 1.0 {
                self.plan_viewport = (self.viewport_size, (0.0, 0.0));
            }
            let _ = self.with_plan(path, |app| {
                app.flip_up = false;
                app.show_view(&view);
                Task::none()
            });
        }
        self.active_plan = Some(path.to_string());
    }

    /// Runs `f` with the camera, section box and viewport of the floor plan at `path` standing in
    /// for the 3D view's, and keeps what it changed with the plan (views window).
    pub(super) fn with_plan(&mut self, path: &str, f: impl FnOnce(&mut App) -> Task<Message>) -> Task<Message> {
        let Some(k) = self.open_plans.iter().position(|o| o.path == path) else { return Task::none() };
        let slot = self.open_plans[k].slot;
        let model = (self.camera, self.section, self.section_on, self.anim.take(), self.viewport_size, self.viewport_origin);
        self.camera = slot.camera;
        self.section = slot.section;
        self.section_on = slot.section_on;
        (self.viewport_size, self.viewport_origin) = self.plan_viewport;
        self.plan_navigation = true;
        let task = f(self);
        self.plan_navigation = false;
        // a plan does not animate: it goes where it was turning to
        if let Some((_, to, _)) = self.anim.take() {
            self.camera = to;
        }
        self.plan_viewport = (self.viewport_size, self.viewport_origin);
        if let Some(open) = self.open_plans.iter_mut().find(|o| o.path == path) {
            open.slot = ViewSlot { camera: self.camera, section: self.section, section_on: self.section_on };
        }
        (self.camera, self.section, self.section_on, self.anim, self.viewport_size, self.viewport_origin) = model;
        task
    }

    /// The pointer in the floor plan window's viewport: it pans, zooms and picks in the plan shown
    /// there; the selection is the project's, so the 3D view shows it too.
    pub(super) fn plan_navigate(&mut self, n: crate::viewport::Navigate) -> Task<Message> {
        use crate::viewport::Navigate;
        if matches!(n, Navigate::Hover { .. } | Navigate::HoverOff | Navigate::Context { .. }) {
            return Task::none();
        }
        let Some(path) = self.active_plan.clone() else { return Task::none() };
        self.with_plan(&path, |app| app.handle(Message::Navigate(n)))
    }

    /// The floor plan shown in the views window as a viewport: the model with the plan's camera
    /// and section box; the pointer there is the plan's ([`App::plan_navigate`]).
    pub fn plan_viewport(&self, p: Palette) -> Option<Element<'_, Message>> {
        let path = self.active_plan.as_ref()?;
        let slot = self.open_plans.iter().find(|o| o.path == *path)?.slot;
        let settings = crate::viewport::render::RenderSettings {
            mode: self.mode,
            grid: false,
            background: crate::theme::srgb3(p.viewport),
            grid_minor: crate::theme::srgb4(p.grid_minor),
            grid_major: crate::theme::srgb4(p.grid_major),
            model_matrix: glam::Mat4::IDENTITY,
            section: slot.section.filter(|_| slot.section_on),
            plane: None,
            caps: self.section_fill,
            grids: false,
            bounds: self.world_bounds(),
        };
        let scene = iced::widget::shader(crate::viewport::Viewport3D {
            model: self.model.as_ref(),
            visible: &self.visible,
            camera: slot.camera,
            settings,
            explode: self.explode_index.as_ref(),
            offsets: &self.units,
            hover: false,
            crosshair: false,
            structural_selected: &self.structural.selected_marks,
            structural_hovered: &self.structural.hovered_marks,
            structural_shown: self.structural.shown_marks.as_ref(),
        })
        .width(Fill)
        .height(Fill);
        let scene: Element<'_, Message> = scene.into();
        Some(scene.map(|m| match m {
            Message::Navigate(n) => Message::PlanNavigate(n),
            other => other,
        }))
    }

    /// The views window: the floor plans and sheets of the document, apart from the 3D view.
    pub fn view_tabs(&self) -> Vec<DocTab> {
        let plans = (0..self.open_plans.len()).map(DocTab::Plan);
        let own = (0..self.open_doc_sheets.len()).map(DocTab::DocSheet);
        plans.chain(own).collect()
    }

    /// The tab the views window shows: a sheet, else a floor plan.
    pub fn active_view_tab(&self) -> Option<usize> {
        let tabs = self.view_tabs();
        let own = self.active_doc_sheet.as_ref().and_then(|p| self.open_doc_sheets.iter().position(|s| s == p)).map(DocTab::DocSheet);
        let plan = self.active_plan.as_ref().and_then(|p| self.open_plans.iter().position(|o| o.path == *p)).map(DocTab::Plan);
        own.or(plan).and_then(|want| tabs.iter().position(|t| *t == want))
    }

    /// The views window is wanted: a floor plan or sheet is open.
    pub fn wants_views_window(&self) -> bool {
        self.separate_views && !self.view_tabs().is_empty()
    }

    /// The views window was closed: its tabs close (the plans and sheets stay in the document).
    pub fn views_window_closed(&mut self) {
        self.open_plans.clear();
        self.open_doc_sheets.clear();
        self.active_plan = None;
        self.active_doc_sheet = None;
    }

    /// What the views window shows under its tabs.
    pub fn views_window_body(&self, p: Palette, l: Language) -> Element<'_, Message> {
        if let Some(sheet) = self.doc_sheet_workspace(p, l) {
            return sheet;
        }
        if let Some(plan) = self.plan_viewport(p) {
            return plan;
        }
        container(text(l.text("Open een plattegrond of blad in Weergaven.", "Open a floor plan or sheet in Views.")).size(13).color(p.text_muted))
            .center(Fill)
            .into()
    }

    pub(super) fn select_view_tab(&mut self, i: usize) {
        match self.view_tabs().get(i).copied() {
            Some(DocTab::Plan(k)) => {
                self.active_doc_sheet = None;
                self.active_plan = Some(self.open_plans[k].path.clone());
            }
            Some(DocTab::DocSheet(k)) => {
                let path = self.open_doc_sheets[k].clone();
                self.open_doc_sheet(&path);
            }
            _ => {}
        }
    }

    pub(super) fn close_view_tab(&mut self, i: usize) {
        match self.view_tabs().get(i).copied() {
            Some(DocTab::Plan(k)) => {
                let path = self.open_plans[k].path.clone();
                self.close_plan(&path);
            }
            Some(DocTab::DocSheet(k)) => {
                let path = self.open_doc_sheets[k].clone();
                self.close_doc_sheet(&path);
            }
            _ => {}
        }
    }

    /// Closes a floor plan's tab; when it showed, the plan before it (or the 3D view) shows.
    pub(super) fn close_plan(&mut self, path: &str) {
        let Some(k) = self.open_plans.iter().position(|o| o.path == path) else { return };
        if self.separate_views {
            if self.active_plan.as_deref() == Some(path) {
                self.active_plan = k.checked_sub(1).or((self.open_plans.len() > 1).then_some(1)).map(|i| self.open_plans[i].path.clone());
            }
            self.open_plans.remove(k);
            return;
        }
        if self.active_plan.as_deref() == Some(path) {
            let next = k.checked_sub(1).or((self.open_plans.len() > 1).then_some(1)).map(|i| self.open_plans[i].path.clone());
            self.load_view(next);
        }
        self.open_plans.remove(k);
    }

    /// The camera and section box shown now (where the camera is going, while it turns).
    fn view_slot(&self) -> ViewSlot {
        let camera = self.anim.map_or(self.camera, |(_, to, _)| to);
        ViewSlot { camera, section: self.section, section_on: self.section_on }
    }

    /// Lets the viewport show the 3D view (`None`) or a floor plan's tab: the camera and section
    /// box of the tab shown so far stay with it, those of the other come back.
    pub(super) fn load_view(&mut self, target: Option<String>) {
        // floor plans in their own window keep their own camera all along
        if self.active_plan == target || self.separate_views {
            return;
        }
        let slot = self.view_slot();
        match &self.active_plan {
            Some(path) => {
                if let Some(open) = self.open_plans.iter_mut().find(|o| o.path == *path) {
                    open.slot = slot;
                }
            }
            None => self.model_view = Some(slot),
        }
        let back = match &target {
            Some(path) => self.open_plans.iter().find(|o| o.path == *path).map(|o| o.slot),
            None => self.model_view.take(),
        };
        if let Some(back) = back {
            self.camera = back.camera;
            self.section = back.section;
            self.section_on = back.section_on;
        }
        self.anim = None;
        self.active_plan = target;
    }

    /// The document tabs, in order: the 3D view, the open floor plans, schedules and sheets.
    pub fn doc_tabs(&self) -> Vec<DocTab> {
        if self.separate_views {
            // floor plans and the document's sheets are in the views window
            let schedules = self.open_schedules.iter().copied().filter(|&i| i < self.schedules.len()).map(DocTab::Schedule);
            let sheets = self.open_sheets.iter().copied().filter(|&i| i < self.sheets().len()).map(DocTab::Sheet);
            return std::iter::once(DocTab::Model).chain(schedules).chain(sheets).collect();
        }
        let plans = (0..self.open_plans.len()).map(DocTab::Plan);
        let schedules = self.open_schedules.iter().copied().filter(|&i| i < self.schedules.len()).map(DocTab::Schedule);
        let sheets = self.open_sheets.iter().copied().filter(|&i| i < self.sheets().len()).map(DocTab::Sheet);
        let own = (0..self.open_doc_sheets.len()).map(DocTab::DocSheet);
        std::iter::once(DocTab::Model).chain(plans).chain(schedules).chain(sheets).chain(own).collect()
    }

    /// The tab that shows: a sheet, else a floor plan, else a schedule (under the 3D view), else
    /// the 3D view.
    pub fn active_doc_tab(&self) -> usize {
        let tabs = self.doc_tabs();
        let at = |want: DocTab| tabs.iter().position(|t| *t == want);
        let plan = self.active_plan.as_ref().filter(|_| !self.separate_views).and_then(|p| self.open_plans.iter().position(|o| o.path == *p));
        let own = self.active_doc_sheet.as_ref().filter(|_| !self.separate_views).and_then(|p| self.open_doc_sheets.iter().position(|s| s == p));
        own.and_then(|k| at(DocTab::DocSheet(k)))
            .or_else(|| self.active_sheet.and_then(|s| at(DocTab::Sheet(s))))
            .or_else(|| plan.and_then(|k| at(DocTab::Plan(k))))
            .or_else(|| self.active_schedule.and_then(|s| at(DocTab::Schedule(s))))
            .unwrap_or(0)
    }

    /// The title of a tab (the 3D view's is the model's file name, given by the shell).
    pub fn tab_title(&self, tab: DocTab) -> &str {
        match tab {
            DocTab::Model => "3D",
            DocTab::Plan(k) => self.open_plans.get(k).and_then(|o| self.document.plan(&o.path)).map_or("", |p| p.name.as_str()),
            DocTab::Schedule(i) => self.schedules.get(i).map_or("", |s| s.name.as_str()),
            DocTab::Sheet(i) => self.sheets().get(i).map_or("", |s| s.title.as_str()),
            DocTab::DocSheet(k) => self.open_doc_sheets.get(k).and_then(|p| self.document.sheet(p)).map_or("", |s| s.title.as_str()),
        }
    }

    pub(super) fn select_tab(&mut self, i: usize) -> Task<Message> {
        match self.doc_tabs().get(i).copied() {
            Some(DocTab::Model) => {
                self.active_sheet = None;
                if !self.separate_views {
                    self.active_doc_sheet = None;
                }
                self.load_view(None);
                self.show_schedule(None)
            }
            Some(DocTab::Plan(k)) => {
                let path = self.open_plans[k].path.clone();
                self.open_plan(&path);
                Task::none()
            }
            Some(DocTab::Schedule(s)) => self.show_schedule(Some(s)),
            Some(DocTab::Sheet(s)) => self.show_sheet(Some(s)),
            Some(DocTab::DocSheet(k)) => {
                let path = self.open_doc_sheets[k].clone();
                self.open_doc_sheet(&path);
                Task::none()
            }
            None => Task::none(),
        }
    }

    pub(super) fn close_tab(&mut self, i: usize) -> Task<Message> {
        match self.doc_tabs().get(i).copied() {
            Some(DocTab::Plan(k)) => {
                let path = self.open_plans[k].path.clone();
                self.close_plan(&path);
                Task::none()
            }
            Some(DocTab::Schedule(s)) => self.close_schedule(s),
            Some(DocTab::Sheet(s)) => self.close_sheet(s),
            Some(DocTab::DocSheet(k)) => {
                let path = self.open_doc_sheets[k].clone();
                self.close_doc_sheet(&path);
                Task::none()
            }
            Some(DocTab::Model) | None => Task::none(),
        }
    }

    /// The Views tab.
    pub(super) fn views_body(&self, p: Palette, l: Language) -> Element<'_, Message> {
        if self.model.is_none() {
            return text(l.text("Geen model geopend", "No model loaded")).size(12).color(p.text_muted).into();
        }
        let document = &self.document;
        let subheader = |label: &str| -> Element<'static, Message> {
            container(text(label.to_uppercase()).size(10).font(FONT_BOLD).color(p.group_label))
                .padding(Padding { top: 8.0, right: 0.0, bottom: 2.0, left: 8.0 })
                .into()
        };
        let mut body = Column::new().spacing(1);
        body = body.push(
            container(
                row![
                    text(l.text("Weergaven", "Views")).size(12).font(FONT_BOLD).width(Fill),
                    text(document.plans().len().to_string()).size(11).color(p.text_secondary),
                ]
                .spacing(6)
                .align_y(Center),
            )
            .padding([4, 6])
            .style(move |_| p.panel_header()),
        );

        // the floor plans, by level from high to low
        body = body.push(subheader(l.text("Plattegronden", "Floor plans")));
        if document.plans().is_empty() {
            let none = l.text("Nog geen plattegronden. Maak er een bij een niveau hieronder.", "No floor plans yet. Make one of a level below.");
            body = body.push(container(text(none).size(12).color(p.text_muted).wrapping(Wrapping::WordOrGlyph)).padding([4, 8]));
        }
        for level in document.levels().iter().rev() {
            for plan in document.plans().iter().filter(|plan| plan.level == level.path) {
                let active = self.active_plan.as_deref() == Some(plan.path.as_str());
                let name = text(plan.name.clone())
                    .size(12)
                    .color(if active { p.accent } else { p.text })
                    .wrapping(Wrapping::WordOrGlyph)
                    .width(Fill);
                let open = button(name)
                    .padding([3, 6])
                    .width(Fill)
                    .style(move |_, s| if active { p.ribbon_button(true, s) } else { p.row_button(s) })
                    .on_press(Message::OpenPlan(Arc::from(plan.path.as_str())));
                let close = svg(svg::Handle::from_memory(openaec_ui::icons::Icon::Close.bytes()))
                    .width(12)
                    .height(12)
                    .style(move |_, _| svg::Style { color: Some(p.text_secondary) });
                let remove = button(close).padding(3).style(move |_, s| p.row_button(s)).on_press(Message::RemovePlan(Arc::from(plan.path.as_str())));
                body = body.push(container(row![open, remove].spacing(4).align_y(Center)).padding(Padding { top: 0.0, right: 4.0, bottom: 0.0, left: 8.0 }));
            }
        }

        // the sheets, then a new one
        body = body.push(subheader(l.text("Bladen", "Sheets")));
        body = super::paper::sheets_list(self, p, l, body);

        // a floor plan of every level
        body = body.push(subheader(l.text("Plattegrond maken van", "Make a floor plan of")));
        if document.levels().is_empty() {
            let none = l.text("Dit model heeft geen niveaus. Voeg er een toe in het tabblad Niveaus.", "This model has no levels. Add one in the Levels tab.");
            body = body.push(container(text(none).size(12).color(p.text_muted).wrapping(Wrapping::WordOrGlyph)).padding([4, 8]));
        }
        for level in document.levels().iter().rev() {
            let plus = svg(crate::icons::handle(crate::icons::ADD)).width(12).height(12).style(move |_, _| svg::Style { color: Some(p.accent) });
            let label = column![text(level.name.clone()).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph)].width(Fill);
            let elevation = text(format!("{:+.3} m", level.elevation)).size(11).color(p.text_secondary);
            body = body.push(
                button(row![plus, label, elevation].spacing(6).align_y(Center))
                    .padding([4, 8])
                    .width(Fill)
                    .style(move |_, s| p.row_button(s))
                    .on_press(Message::NewPlan(Arc::from(level.path.as_str()))),
            );
        }
        scrollable(body.padding(Padding { top: 0.0, right: 4.0, bottom: 8.0, left: 0.0 })).height(Fill).spacing(4).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ExplorerTab;
    use crate::model::{Layer, Model};
    use crate::viewport::camera::ViewPreset;
    use std::path::PathBuf;

    fn simulator(app: &App) -> iced_test::Simulator<'_, Message> {
        iced_test::Simulator::with_size(iced::Settings::default(), iced::Size::new(1400.0, 900.0), app.view())
    }

    /// A tower of 4 x 4 x 6 m with levels at 0 and 3.
    fn tower() -> Arc<Model> {
        let positions = vec![0.,0.,0., 4.,0.,0., 4.,4.,0., 0.,4.,0., 0.,0.,6., 4.,0.,6., 4.,4.,6., 0.,4.,6.];
        let indices = vec![0,2,1, 0,3,2, 4,5,6, 4,6,7, 0,1,5, 0,5,4, 1,2,6, 1,6,5, 2,3,7, 2,7,6, 3,0,4, 3,4,7];
        let layer = Layer { name: "Walls".into(), positions, indices, ..Layer::default() };
        let mut model = Model::new(PathBuf::from("tower.rvt"), "Revit", vec![layer]);
        model.project.levels = vec![("00 ground".into(), 0.0), ("01 first".into(), 3.0)];
        Arc::new(model)
    }

    fn click(app: &mut App, label: &str) {
        let mut ui = simulator(app);
        ui.click(label).unwrap_or_else(|_| panic!("no {label}"));
        let messages: Vec<Message> = ui.into_messages().collect();
        for m in messages {
            let _ = app.update(m);
        }
    }

    #[test]
    fn makes_a_floor_plan_of_a_level_and_shows_it_from_above_cut_at_its_height() {
        let mut app = App::default();
        app.set_model(tower());
        click(&mut app, "Views");
        assert_eq!(app.explorer_tab, ExplorerTab::Views);
        assert!(simulator(&app).find("No floor plans yet. Make one of a level below.").is_ok());
        click(&mut app, "01 first");
        let plan = app.document.plans().first().expect("a plan").clone();
        assert_eq!(plan.name, "01 first");
        assert_eq!(app.active_plan.as_deref(), Some(plan.path.as_str()));
        // the section box from just under the level to the cut
        assert!(app.section_on);
        let section = app.section.expect("a section box");
        assert!((section.min.z - (3.0 - 0.3)).abs() < 1e-5 && (section.max.z - (3.0 + 1.2)).abs() < 1e-5, "{section:?}");
        // from above, without perspective
        let (_, to, _) = app.anim.expect("the camera turns");
        assert!(to.ortho);
        assert!((to.pitch - ViewPreset::Top.orientation().1).abs() < 1e-3);
        // listed under the floor plans, and shown again from there
        assert!(simulator(&app).find("FLOOR PLANS").is_ok());
        let _ = app.update(Message::SelectTab(0));
        assert!(!app.section_on);
        let _ = app.update(Message::OpenPlan(Arc::from(plan.path.as_str())));
        assert!(app.section_on);
        assert_eq!(app.active_plan.as_deref(), Some(plan.path.as_str()));
    }

    #[test]
    fn a_floor_plan_has_its_own_tab_camera_and_section_and_shares_the_selection() {
        let mut app = App::default();
        app.set_model(tower());
        let model_camera = app.camera;
        let level = Arc::<str>::from(app.document.levels()[1].path.as_str());
        let _ = app.update(Message::NewPlan(level));
        assert_eq!(app.doc_tabs(), [DocTab::Model, DocTab::Plan(0)]);
        assert_eq!(app.active_doc_tab(), 1);
        assert_eq!(app.tab_title(DocTab::Plan(0)), "01 first");
        let plan_camera = app.anim.expect("turning to the plan").1;
        // dragging in a plan moves it, it does not turn
        app.anim = None;
        app.camera = plan_camera;
        let _ = app.update(Message::Navigate(crate::viewport::Navigate::Orbit { dx: 40.0, dy: 10.0 }));
        assert_eq!((app.camera.yaw, app.camera.pitch), (plan_camera.yaw, plan_camera.pitch));
        assert_ne!(app.camera.target, plan_camera.target);
        let moved = app.camera;
        // select in the plan: the 3D view has it selected too
        app.click(Some(crate::selection::Pickable::Element(1)), false);
        let _ = app.update(Message::SelectTab(0));
        assert_eq!(app.active_doc_tab(), 0);
        assert_eq!(app.camera, model_camera, "the 3D view's camera comes back");
        assert!(!app.section_on);
        assert_eq!(app.sel.items(), &[crate::selection::Pickable::Element(1)]);
        // and back to the plan as it was left
        let _ = app.update(Message::SelectTab(1));
        assert_eq!(app.camera, moved);
        assert!(app.section_on);
        // closed: the 3D view shows; the plan stays in the document
        let _ = app.update(Message::CloseTab(1));
        assert_eq!(app.doc_tabs(), [DocTab::Model]);
        assert_eq!(app.camera, model_camera);
        assert_eq!(app.active_plan, None);
        assert_eq!(app.document.plans().len(), 1);
        let _ = app.update(Message::CloseTab(0));
        assert_eq!(app.doc_tabs(), [DocTab::Model], "the 3D view stays");
    }

    #[test]
    fn in_a_window_of_their_own_floor_plans_keep_their_camera_beside_the_3d_view() {
        let mut app = App::default();
        app.separate_views = true;
        app.set_model(tower());
        app.anim = None;
        let model_camera = app.camera;
        let level = Arc::<str>::from(app.document.levels()[1].path.as_str());
        let _ = app.update(Message::NewPlan(level));
        // the 3D view is as it was; the plan is in the views window, from above, cut at 4.2 m
        assert_eq!((app.camera, app.section_on), (model_camera, false));
        assert_eq!(app.doc_tabs(), [DocTab::Model]);
        assert_eq!((app.view_tabs(), app.active_view_tab()), (vec![DocTab::Plan(0)], Some(0)));
        assert!(app.wants_views_window());
        let slot = app.open_plans[0].slot;
        assert!(slot.camera.ortho && slot.section_on);
        assert!((slot.camera.pitch - ViewPreset::Top.orientation().1).abs() < 1e-3);
        assert!((slot.section.unwrap().max.z - 4.2).abs() < 1e-5);
        // a drag in the plan moves the plan only; one in the 3D view still turns the model
        let _ = app.update(Message::PlanNavigate(crate::viewport::Navigate::Orbit { dx: 40.0, dy: 10.0 }));
        let moved = app.open_plans[0].slot.camera;
        assert_ne!(moved.target, slot.camera.target);
        assert_eq!((moved.yaw, moved.pitch), (slot.camera.yaw, slot.camera.pitch));
        assert_eq!(app.camera, model_camera);
        let _ = app.update(Message::Navigate(crate::viewport::Navigate::Orbit { dx: 40.0, dy: 10.0 }));
        assert_ne!(app.camera.yaw, model_camera.yaw);
        // the window draws the plan; a sheet opens in it beside the plan
        let p = app.theme.palette();
        drop(iced_test::Simulator::with_size(iced::Settings::default(), iced::Size::new(900.0, 600.0), app.views_window_body(p, Language::English)));
        let _ = app.update(Message::NewSheet("Sheet"));
        assert_eq!(app.view_tabs(), [DocTab::Plan(0), DocTab::DocSheet(0)]);
        assert_eq!(app.active_view_tab(), Some(1));
        assert_eq!(app.doc_tabs(), [DocTab::Model], "the main window keeps the 3D view");
        let _ = app.update(Message::SelectViewTab(0));
        assert_eq!((app.active_view_tab(), app.active_doc_sheet.as_deref()), (Some(0), None));
        let _ = app.update(Message::CloseViewTab(0));
        assert_eq!(app.view_tabs(), [DocTab::DocSheet(0)]);
        // the window closed: its tabs close, the plan and sheet stay in the document
        app.views_window_closed();
        assert!(app.view_tabs().is_empty() && !app.wants_views_window());
        assert_eq!((app.document.plans().len(), app.document.sheets().len()), (1, 1));
    }

    #[test]
    fn removing_a_floor_plan_forgets_it() {
        let mut app = App::default();
        app.set_model(tower());
        let level = Arc::<str>::from(app.document.levels()[0].path.as_str());
        let _ = app.update(Message::NewPlan(level));
        let plan = Arc::<str>::from(app.document.plans()[0].path.as_str());
        let _ = app.update(Message::RemovePlan(plan));
        assert!(app.document.plans().is_empty());
        assert_eq!(app.active_plan, None);
    }
}
