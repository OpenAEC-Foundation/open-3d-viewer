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
        if self.open_plans.iter().any(|o| o.path == path) {
            self.active_sheet = None;
            self.active_schedule = None;
            self.load_view(Some(path.to_string()));
            return;
        }
        let Some(view) = self.saved_view_of(path) else { return };
        self.active_sheet = None;
        self.active_schedule = None;
        let slot = self.view_slot();
        self.open_plans.push(OpenPlan { path: path.to_string(), slot });
        self.load_view(Some(path.to_string()));
        self.show_view(&view);
        self.fit_plan_section(path);
    }

    /// A view of the document as the viewport shows it: a floor plan from above in its range, a
    /// section or elevation level, looking to the left of its line, in its box.
    fn saved_view_of(&self, path: &str) -> Option<SavedView> {
        if let Some(plan) = self.document.plan(path) {
            let (low, high) = self.document.plan_range(plan)?;
            return Some(SavedView {
                name: plan.name.clone(),
                kind: ViewKind::Plan,
                camera: None,
                clip: None,
                plan: Some(PlanRange { low: low as f32, high: high as f32, from_below: false }),
                level: self.document.level(&plan.level).map(|l| l.name.clone()),
            });
        }
        let section = self.document.section(path)?;
        let world = self.world_bounds()?;
        let (_, n) = section.axes();
        let middle = [(section.from[0] + section.to[0]) / 2.0, (section.from[1] + section.to[1]) / 2.0];
        let camera = crate::model::ViewCamera {
            eye: [(middle[0] - n[0]) as f32, (middle[1] - n[1]) as f32, (world.min[2] + world.max[2]) / 2.0],
            forward: [n[0] as f32, n[1] as f32, 0.0],
            perspective: false,
            fov_y: None,
        };
        Some(SavedView { name: section.name.clone(), kind: ViewKind::Section, camera: Some(camera), clip: self.section_box(path), plan: None, level: None })
    }

    /// The box a section or elevation shows: beyond its line up to its depth, within its crop
    /// (u along the line, v up), else the model's height.
    fn section_box(&self, path: &str) -> Option<SectionBox> {
        let section = self.document.section(path)?;
        let world = self.world_bounds()?;
        let (d, n) = section.axes();
        let (u, v) = match section.crop {
            Some((lo, hi)) => ([lo[0] as f32, hi[0] as f32], [lo[1] as f32, hi[1] as f32]),
            None => ([-1.0e5, 1.0e5], [world.min[2] - 0.01, world.max[2] + 0.01]),
        };
        let origin = glam::Vec3::new(section.from[0] as f32, section.from[1] as f32, 0.0);
        let basis = [glam::Vec3::new(d[0] as f32, d[1] as f32, 0.0), glam::Vec3::new(n[0] as f32, n[1] as f32, 0.0), glam::Vec3::Z];
        SectionBox::from_frame(origin, basis, glam::Vec3::new(u[0], 0.0, v[0]), glam::Vec3::new(u[1], section.depth as f32, v[1]))
    }

    /// The section box of a view's open tab: a floor plan's range in height and crop in plan
    /// (all of the model without one), a section's box.
    pub(super) fn fit_plan_section(&mut self, path: &str) {
        let b = if self.document.section(path).is_some() {
            let Some(b) = self.section_box(path) else { return };
            b
        } else {
            let Some(plan) = self.document.plan(path) else { return };
            let Some((low, high)) = self.document.plan_range(plan) else { return };
            let crop = plan.crop;
            let Some(world) = self.world_bounds() else { return };
            let mut b = SectionBox::around(&world);
            b.min.z = low as f32;
            b.max.z = (high as f32).max(low as f32 + 0.01);
            if let Some((lo, hi)) = crop {
                (b.min.x, b.min.y, b.max.x, b.max.y) = (lo[0] as f32, lo[1] as f32, hi[0] as f32, hi[1] as f32);
            }
            b
        };
        if self.active_plan.as_deref() == Some(path) {
            self.section = Some(b);
            self.section_on = true;
        } else if let Some(open) = self.open_plans.iter_mut().find(|o| o.path == path) {
            open.slot.section = Some(b);
            open.slot.section_on = true;
        }
    }

    /// Closes a floor plan's tab; when it showed, the plan before it (or the 3D view) shows.
    pub(super) fn close_plan(&mut self, path: &str) {
        let Some(k) = self.open_plans.iter().position(|o| o.path == path) else { return };
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
        if self.active_plan == target {
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
        self.mark_pick = None;
    }

    /// The document tabs, in order: the 3D view, the open floor plans, schedules and sheets.
    pub fn doc_tabs(&self) -> Vec<DocTab> {
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
        let plan = self.active_plan.as_ref().and_then(|p| self.open_plans.iter().position(|o| o.path == *p));
        let own = self.active_doc_sheet.as_ref().and_then(|p| self.open_doc_sheets.iter().position(|s| s == p));
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
            DocTab::Plan(k) => self.open_plans.get(k).and_then(|o| self.document.view_name(&o.path)).unwrap_or(""),
            DocTab::Schedule(i) => self.schedules.get(i).map_or("", |s| s.name.as_str()),
            DocTab::Sheet(i) => self.sheets().get(i).map_or("", |s| s.title.as_str()),
            DocTab::DocSheet(k) => self.open_doc_sheets.get(k).and_then(|p| self.document.sheet(p)).map_or("", |s| s.title.as_str()),
        }
    }

    pub(super) fn select_tab(&mut self, i: usize) -> Task<Message> {
        match self.doc_tabs().get(i).copied() {
            Some(DocTab::Model) => {
                self.active_sheet = None;
                self.active_doc_sheet = None;
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
                let open = button(draggable(name, &plan.path))
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

        // sections and elevations; the four elevations at once
        body = body.push(subheader(l.text("Doorsneden en aanzichten", "Sections and elevations")));
        for section in document.sections() {
            let active = self.active_plan.as_deref() == Some(section.path.as_str());
            let name = text(section.name.clone()).size(12).color(if active { p.accent } else { p.text }).wrapping(Wrapping::WordOrGlyph).width(Fill);
            let open = button(draggable(name, &section.path))
                .padding([3, 6])
                .width(Fill)
                .style(move |_, s| if active { p.ribbon_button(true, s) } else { p.row_button(s) })
                .on_press(Message::OpenPlan(Arc::from(section.path.as_str())));
            let close = svg(svg::Handle::from_memory(openaec_ui::icons::Icon::Close.bytes()))
                .width(12)
                .height(12)
                .style(move |_, _| svg::Style { color: Some(p.text_secondary) });
            let remove = button(close).padding(3).style(move |_, s| p.row_button(s)).on_press(Message::RemoveSection(Arc::from(section.path.as_str())));
            body = body.push(container(row![open, remove].spacing(4).align_y(Center)).padding(Padding { top: 0.0, right: 4.0, bottom: 0.0, left: 8.0 }));
        }
        let plus = svg(crate::icons::handle(crate::icons::ADD)).width(12).height(12).style(move |_, _| svg::Style { color: Some(p.accent) });
        let names = [l.text("Aanzicht noord", "North elevation"), l.text("Aanzicht oost", "East elevation"), l.text("Aanzicht zuid", "South elevation"), l.text("Aanzicht west", "West elevation")];
        body = body.push(
            button(row![plus, text(l.text("Vier aanzichten maken", "Make four elevations")).size(12).color(p.text)].spacing(6).align_y(Center))
                .padding([4, 8])
                .width(Fill)
                .style(move |_, s| p.row_button(s))
                .on_press(Message::MakeElevations(names)),
        );
        let hint = l.text("Een doorsnede teken je op een blad: Doorsnede, dan twee punten op een plattegrond.", "Draw a section on a sheet: Section, then two points on a floor plan.");
        body = body.push(container(text(hint).size(11).color(p.text_muted).wrapping(Wrapping::WordOrGlyph)).padding([2, 8]));

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

/// The name of a view in the list, which a press takes (to drag onto the sheet shown) and a
/// release on it opens; the press does not reach the row's button, so the click is the release.
fn draggable<'a>(name: iced::widget::Text<'a>, path: &str) -> Element<'a, Message> {
    let path: Arc<str> = Arc::from(path);
    iced::widget::mouse_area(name).on_press(Message::GrabView(path.clone())).on_release(Message::ReleaseView(path)).into()
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

    /// A press on a view in the list takes it (to drag onto a sheet); let go on it, it opens.
    #[test]
    fn a_view_in_the_list_is_taken_by_a_press_and_opens_with_a_click() {
        let mut app = App::default();
        app.set_model(tower());
        let level = Arc::<str>::from(app.document.levels()[1].path.as_str());
        let _ = app.update(Message::NewPlan(level));
        let plan = Arc::<str>::from(app.document.plans()[0].path.as_str());
        let _ = app.update(Message::SelectTab(0));
        click(&mut app, "Views");
        let mut ui = simulator(&app);
        ui.click("01 first").expect("the plan in the list");
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(matches!(messages.first(), Some(Message::GrabView(v)) if *v == plan), "{messages:?}");
        assert!(messages.iter().any(|m| matches!(m, Message::ReleaseView(v) if *v == plan)), "{messages:?}");
        for m in messages {
            let _ = app.update(m);
        }
        assert_eq!(app.active_plan.as_deref(), Some(&*plan));
        assert_eq!(app.dragging_view, None);
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
