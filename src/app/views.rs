//! The explorer's Views tab: the views of the project document by kind. Plans (floor,
//! structural, ceiling) are made per level and shown from above (a ceiling plan from below), cut
//! at their height above the level; 3D views keep a camera and a section box; sections and
//! elevations, and the sheets.

use super::{App, Message, FONT_BOLD};
use crate::document::{Camera3d, PlanKind, SectionKind, ViewBox};
use crate::explode::{ExplodeIndex, UnitOwner};
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

/// A view of the document open in its own tab.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenPlan {
    pub path: String,
    /// Its camera and section box while another tab shows.
    pub slot: ViewSlot,
}

/// A document tab: the 3D view (always first), views of the document, schedules, sheets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocTab {
    Model,
    /// Index in `App::open_plans`: a plan, section, elevation or 3D view of the document.
    Plan(usize),
    /// Index in `App::schedules`.
    Schedule(usize),
    /// Index in the model's sheets (read from the file).
    Sheet(usize),
    /// Index in the model's legends and drafting views (read from the file).
    Draft(usize),
    /// Index in `App::open_doc_sheets`: a sheet of the document.
    DocSheet(usize),
    /// How the model's IFCX file is built up.
    Ifc,
}

impl App {
    /// Makes a floor plan of a level and shows it in its tab.
    pub(super) fn new_plan(&mut self, level: &str) {
        self.new_plan_of(level, PlanKind::Floor, "");
    }

    /// Makes a plan of `kind` of a level (named with `word` after the level's name, a floor plan
    /// excepted) and shows it in its tab; the level list of the Views tab closes.
    pub(super) fn new_plan_of(&mut self, level: &str, kind: PlanKind, word: &str) {
        self.views_picker = None;
        if let Some(plan) = self.document.add_plan_of(level, kind, word) {
            self.open_plan(&plan);
        }
    }

    /// Whether the view at `path` is a structural plan, which leaves out what carries nothing.
    pub(super) fn is_structural_plan(&self, path: Option<&str>) -> bool {
        path.and_then(|p| self.document.plan(p)).is_some_and(|p| p.kind.structural())
    }

    /// Per unit of the shown model, whether a structural plan leaves it out: an element whose
    /// class or `LoadBearing` says it carries nothing ([`crate::element_class::load_bearing`]).
    /// Elements nothing is known of, and the rest of a layer, stay. Worked out once per model.
    pub(super) fn non_structural_units(&mut self) -> Arc<Vec<bool>> {
        let Some(index) = self.explode_index.clone() else { return Arc::default() };
        if let Some((of, units)) = &self.structure_units {
            if Arc::ptr_eq(of, &index) {
                return units.clone();
            }
        }
        let units = Arc::new(self.carrying_nothing(&index));
        self.structure_units = Some((index, units.clone()));
        units
    }

    fn carrying_nothing(&self, index: &ExplodeIndex) -> Vec<bool> {
        let Some(model) = self.model.as_deref() else { return Vec::new() };
        // the first layer of every element, for its category where its properties name none
        let mut layer_of = std::collections::HashMap::new();
        for (l, layer) in model.layers.iter().enumerate() {
            for e in &layer.elements {
                layer_of.entry(e.0).or_insert(l);
            }
        }
        let format = |id: u64| -> &str {
            let entry = model.keys.get(&id).and_then(|k| self.models.get(k.model as usize));
            entry.map_or(model.format.as_str(), |e| e.model.format.as_str())
        };
        (0..index.len())
            .map(|u| match index.owner(u).filter(|_| u > 0) {
                Some(UnitOwner::Element(id)) => {
                    let layer = layer_of.get(&id).and_then(|&l| model.layers.get(l)).map_or("", |l| l.name.as_str());
                    crate::element_class::load_bearing(format(id), layer, &model.element_props(id)) == Some(false)
                }
                _ => false,
            })
            .collect()
    }

    /// Shows a view of the document in its tab, opening the tab the first time: a plan from
    /// above, orthographic, the section box from just under its level up to its cut; a section
    /// or elevation level, in its box; a 3D view as it was left.
    pub(super) fn open_plan(&mut self, path: &str) {
        self.active_doc_sheet = None;
        let mut drawing = None;
        if !self.open_plans.iter().any(|o| o.path == path) {
            let slot = match self.view_3d_slot(path) {
                Some(slot) => slot,
                None => {
                    let Some(view) = self.saved_view_of(path) else { return };
                    drawing = Some(view);
                    self.view_slot()
                }
            };
            self.open_plans.push(OpenPlan { path: path.to_string(), slot });
        }
        self.active_sheet = None; self.active_draft = None;
        self.active_schedule = None;
        self.ifc.shown = false;
        self.load_view(Some(path.to_string()));
        if let Some(view) = drawing {
            self.show_view(&view);
            self.fit_plan_section(path);
        }
    }

    /// Makes a 3D view of what the 3D tab shows (its camera, section box, layers and hidden
    /// elements), named `<word> <n>`, and shows it in its tab.
    pub(super) fn new_view_3d(&mut self, word: &str) {
        if self.model.is_none() {
            return;
        }
        let in_3d = self.active_plan.is_none();
        let slot = if in_3d { self.view_slot() } else { self.model_view.unwrap_or_else(|| self.view_slot()) };
        let switches = match (&self.parked_layers, in_3d) {
            (Some((visible, models)), false) => {
                let mut on = visible.clone();
                for (entry, layers) in self.models.iter().zip(models) {
                    for (i, &v) in entry.layers.clone().zip(layers) {
                        if let Some(o) = on.get_mut(i) {
                            *o = v;
                        }
                    }
                }
                on
            }
            _ => self.viewport_switches(),
        };
        let layers = self.own_switches(&switches);
        let hidden = if in_3d { self.hidden.clone() } else { self.parked_hidden.clone().unwrap_or_default() };
        let name = self.document.next_view_3d_name(word);
        let section = slot.section.filter(|_| slot.section_on).map(|b| view_box(&b));
        let path = self.document.add_view_3d(&name, camera_3d(&slot.camera), section, layers, hidden);
        self.open_plan(&path);
    }

    /// The camera and section box a 3D view opens with: as it was left.
    fn view_3d_slot(&self, path: &str) -> Option<ViewSlot> {
        let view = self.document.view_3d(path)?;
        let radius = self.world_bounds().map_or(10.0, |b| b.radius().max(1e-3));
        let section = view.section.map(|b| section_box(&b));
        Some(ViewSlot { camera: camera_of(&view.camera, radius), section, section_on: section.is_some() })
    }

    /// Lets the 3D view at `path` (if it is one) keep the camera and section box of `slot`.
    fn keep_view_3d(&mut self, path: &str, slot: &ViewSlot) {
        let section = slot.section.filter(|_| slot.section_on).map(|b| view_box(&b));
        self.document.set_view_3d_camera(path, camera_3d(&slot.camera), section);
    }

    /// The document as it is now: with the camera and section box of the 3D view whose tab
    /// shows, which it keeps for good when its tab is left.
    pub(crate) fn document_now(&self) -> crate::document::Document {
        let mut document = self.document.clone();
        if let Some(path) = self.active_plan.as_deref().filter(|p| document.view_3d(p).is_some()) {
            let slot = self.view_slot();
            let section = slot.section.filter(|_| slot.section_on).map(|b| view_box(&b));
            document.set_view_3d_camera(path, camera_3d(&slot.camera), section);
        }
        document
    }

    /// A view of the document as the viewport shows it: a floor plan from above in its range, a
    /// section or elevation level, looking to the left of its line, in its box.
    fn saved_view_of(&self, path: &str) -> Option<SavedView> {
        if let Some(plan) = self.document.plan(path) {
            let (low, high) = self.document.plan_range(plan)?;
            return Some(SavedView {
                folder: Vec::new(),
                name: plan.name.clone(),
                kind: ViewKind::Plan,
                camera: None,
                clip: None,
                plan: Some(PlanRange { low: low as f32, high: high as f32, from_below: plan.kind.seen_from_below() }),
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
        Some(SavedView { folder: Vec::new(), name: section.name.clone(), kind: ViewKind::Section, camera: Some(camera), clip: self.section_box(path), plan: None, level: None })
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
        self.parked_histories.remove(path);
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
        match self.active_plan.clone() {
            Some(path) => {
                if let Some(open) = self.open_plans.iter_mut().find(|o| o.path == path) {
                    open.slot = slot;
                }
                self.keep_view_3d(&path, &slot);
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
        self.swap_view_layers(target.as_deref());
        let structural = self.is_structural_plan(self.active_plan.as_deref()) || self.is_structural_plan(target.as_deref());
        // a view that leaves elements out because their family shows none of their solids there
        let leaves_out = |path: &Option<String>| path.as_ref().zip(self.model.as_ref()).is_some_and(|(p, m)| m.project.switched_off.contains_key(p));
        let structural = structural || leaves_out(&self.active_plan) || leaves_out(&target);
        self.active_plan = target;
        self.mark_pick = None;
        // into or out of a structural plan: what carries nothing goes or comes back
        if structural {
            self.update_units();
        }
    }

    /// The document tabs, in order: the 3D view, the open floor plans, schedules and sheets.
    pub fn doc_tabs(&self) -> Vec<DocTab> {
        let plans = (0..self.open_plans.len()).map(DocTab::Plan);
        let schedules = self.open_schedules.iter().copied().filter(|&i| i < self.schedules.len()).map(DocTab::Schedule);
        let sheets = self.open_sheets.iter().copied().filter(|&i| i < self.sheets().len()).map(DocTab::Sheet);
        let drafts = self.open_drafts.iter().copied().filter(|&i| i < self.drafts().len()).map(DocTab::Draft);
        let own = (0..self.open_doc_sheets.len()).map(DocTab::DocSheet);
        let ifc = self.ifc.open.then_some(DocTab::Ifc);
        std::iter::once(DocTab::Model).chain(plans).chain(schedules).chain(sheets).chain(drafts).chain(own).chain(ifc).collect()
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
            .or_else(|| self.active_draft.and_then(|d| at(DocTab::Draft(d))))
            .or_else(|| plan.and_then(|k| at(DocTab::Plan(k))))
            .or_else(|| self.active_schedule.and_then(|s| at(DocTab::Schedule(s))))
            .or_else(|| self.ifc.shown.then(|| at(DocTab::Ifc)).flatten())
            .unwrap_or(0)
    }

    /// The title of a tab (the 3D view's is the model's file name, given by the shell).
    pub fn tab_title(&self, tab: DocTab) -> &str {
        match tab {
            DocTab::Model => "3D",
            DocTab::Plan(k) => self.open_plans.get(k).and_then(|o| self.document.view_name(&o.path)).unwrap_or(""),
            DocTab::Schedule(i) => self.schedules.get(i).map_or("", |s| s.name.as_str()),
            DocTab::Sheet(i) => self.sheets().get(i).map_or("", |s| s.title.as_str()),
            DocTab::Draft(i) => self.drafts().get(i).map_or("", |d| d.name.as_str()),
            DocTab::DocSheet(k) => self.open_doc_sheets.get(k).and_then(|p| self.document.sheet(p)).map_or("", |s| s.title.as_str()),
            DocTab::Ifc => "IFC",
        }
    }

    pub(super) fn select_tab(&mut self, i: usize) -> Task<Message> {
        match self.doc_tabs().get(i).copied() {
            Some(DocTab::Model) => {
                self.active_sheet = None; self.active_draft = None;
                self.active_doc_sheet = None;
                self.ifc.shown = false;
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
            Some(DocTab::Draft(d)) => self.show_draft(Some(d)),
            Some(DocTab::DocSheet(k)) => {
                let path = self.open_doc_sheets[k].clone();
                self.open_doc_sheet(&path);
                Task::none()
            }
            Some(DocTab::Ifc) => {
                self.show_ifc();
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
            Some(DocTab::Draft(d)) => self.close_draft(d),
            Some(DocTab::DocSheet(k)) => {
                let path = self.open_doc_sheets[k].clone();
                self.close_doc_sheet(&path);
                Task::none()
            }
            Some(DocTab::Ifc) => {
                self.close_ifc();
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
                    text((document.plans().len() + document.sections().len() + document.views_3d().len()).to_string()).size(11).color(p.text_secondary),
                ]
                .spacing(6)
                .align_y(Center),
            )
            .padding([4, 6])
            .style(move |_| p.panel_header()),
        );

        // the plans per kind, each by level from high to low; New lists the levels to make one of
        for kind in PlanKind::ALL {
            let (label, word) = match kind {
                PlanKind::Floor => (l.text("Plattegronden", "Floor plans"), ""),
                PlanKind::Structural => (l.text("Constructieve plattegronden", "Structural plans"), l.text("constructie", "structure")),
                PlanKind::Ceiling => (l.text("Plafondplannen", "Ceiling plans"), l.text("plafond", "ceiling")),
                PlanKind::StructuralUp => (l.text("Constructieve plattegronden, omhoog gezien", "Structural plans, looking up"), l.text("constructie omhoog", "structure up")),
            };
            let picking = self.views_picker == Some(kind);
            let plus = svg(crate::icons::handle(crate::icons::ADD)).width(12).height(12).style(move |_, _| svg::Style { color: Some(p.accent) });
            let new = button(row![plus, text(l.text("Nieuw", "New")).size(11).color(p.text)].spacing(4).align_y(Center))
                .padding([2, 6])
                .style(move |_, s| if picking { p.ribbon_button(true, s) } else { p.row_button(s) })
                .on_press(Message::PickLevelFor((!picking).then_some(kind)));
            body = body.push(row![container(subheader(label)).width(Fill), container(new).padding(Padding { top: 6.0, right: 4.0, bottom: 0.0, left: 0.0 })].align_y(iced::alignment::Vertical::Bottom));
            if picking {
                body = self.level_picker(p, l, body, kind, word);
            }
            let mut any = false;
            for level in document.levels().iter().rev() {
                for plan in document.plans().iter().filter(|plan| plan.level == level.path && plan.kind == kind) {
                    any = true;
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
                    // laid in the 3D view in full, or not
                    let laid = self.projected.contains(plan.path.as_str());
                    let project = button(text("3D").size(10).font(FONT_BOLD).color(if laid { p.accent } else { p.text_secondary }))
                        .padding([2, 4])
                        .style(move |_, s| if laid { p.ribbon_button(true, s) } else { p.row_button(s) })
                        .on_press(Message::ProjectView(Arc::from(plan.path.as_str())));
                    body = body.push(container(row![open, project, remove].spacing(4).align_y(Center)).padding(Padding { top: 0.0, right: 4.0, bottom: 0.0, left: 8.0 }));
                }
            }
            if !any && !picking {
                let none = l.text("Nog geen. Nieuw maakt er een van een niveau.", "None yet. New makes one of a level.");
                body = body.push(container(text(none).size(12).color(p.text_muted).wrapping(Wrapping::WordOrGlyph)).padding([2, 8]));
            }
        }

        // 3D views; New keeps what the 3D tab shows
        let plus = svg(crate::icons::handle(crate::icons::ADD)).width(12).height(12).style(move |_, _| svg::Style { color: Some(p.accent) });
        let new = button(row![plus, text(l.text("Nieuw", "New")).size(11).color(p.text)].spacing(4).align_y(Center))
            .padding([2, 6])
            .style(move |_, s| p.row_button(s))
            .on_press(Message::NewView3d(l.text("3D-weergave", "3D view")));
        let header = subheader(l.text("3D-weergaven", "3D views"));
        body = body.push(row![container(header).width(Fill), container(new).padding(Padding { top: 6.0, right: 4.0, bottom: 0.0, left: 0.0 })].align_y(iced::alignment::Vertical::Bottom));
        for view in document.views_3d() {
            let active = self.active_plan.as_deref() == Some(view.path.as_str());
            let name = text(view.name.clone()).size(12).color(if active { p.accent } else { p.text }).wrapping(Wrapping::WordOrGlyph).width(Fill);
            let open = button(name)
                .padding([3, 6])
                .width(Fill)
                .style(move |_, s| if active { p.ribbon_button(true, s) } else { p.row_button(s) })
                .on_press(Message::OpenPlan(Arc::from(view.path.as_str())));
            let close = svg(svg::Handle::from_memory(openaec_ui::icons::Icon::Close.bytes()))
                .width(12)
                .height(12)
                .style(move |_, _| svg::Style { color: Some(p.text_secondary) });
            let remove = button(close).padding(3).style(move |_, s| p.row_button(s)).on_press(Message::RemoveView3d(Arc::from(view.path.as_str())));
            body = body.push(container(row![open, remove].spacing(4).align_y(Center)).padding(Padding { top: 0.0, right: 4.0, bottom: 0.0, left: 8.0 }));
        }
        if document.views_3d().is_empty() {
            let none = l.text("Nog geen. Nieuw legt vast wat het tabblad 3D toont.", "None yet. New keeps what the 3D tab shows.");
            body = body.push(container(text(none).size(12).color(p.text_muted).wrapping(Wrapping::WordOrGlyph)).padding([2, 8]));
        }

        // sections, drawn on a sheet; elevations, the four at once
        for kind in [SectionKind::Section, SectionKind::Elevation] {
            body = body.push(match kind {
                SectionKind::Section => subheader(l.text("Doorsneden", "Sections")),
                SectionKind::Elevation => subheader(l.text("Aanzichten", "Elevations")),
            });
            body = self.section_rows(p, body, kind);
            body = body.push(match kind {
                SectionKind::Section => {
                    let hint = l.text("Een doorsnede teken je op een blad: Doorsnede, dan twee punten op een plattegrond.", "Draw a section on a sheet: Section, then two points on a floor plan.");
                    container(text(hint).size(11).color(p.text_muted).wrapping(Wrapping::WordOrGlyph)).padding([2, 8]).into()
                }
                SectionKind::Elevation => {
                    let plus = svg(crate::icons::handle(crate::icons::ADD)).width(12).height(12).style(move |_, _| svg::Style { color: Some(p.accent) });
                    let names = [l.text("Aanzicht noord", "North elevation"), l.text("Aanzicht oost", "East elevation"), l.text("Aanzicht zuid", "South elevation"), l.text("Aanzicht west", "West elevation")];
                    Element::from(
                        button(row![plus, text(l.text("Vier aanzichten maken", "Make four elevations")).size(12).color(p.text)].spacing(6).align_y(Center))
                            .padding([4, 8])
                            .width(Fill)
                            .style(move |_, s| p.row_button(s))
                            .on_press(Message::MakeElevations(names)),
                    )
                }
            });
        }

        // the sheets, then a new one
        body = body.push(subheader(l.text("Tekeningen", "Drawings")));
        body = super::paper::sheets_list(self, p, l, body);
        scrollable(body.padding(Padding { top: 0.0, right: 4.0, bottom: 8.0, left: 0.0 })).height(Fill).spacing(4).into()
    }

    /// The sections or the elevations of the document, to open (a click), drag onto a sheet or
    /// remove.
    fn section_rows<'a>(&'a self, p: Palette, mut body: Column<'a, Message>, kind: SectionKind) -> Column<'a, Message> {
        for section in self.document.sections().iter().filter(|s| s.kind == kind) {
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
            // laid in the 3D view in full, or not
            let laid = self.projected.contains(section.path.as_str());
            let project = button(text("3D").size(10).font(FONT_BOLD).color(if laid { p.accent } else { p.text_secondary }))
                .padding([2, 4])
                .style(move |_, s| if laid { p.ribbon_button(true, s) } else { p.row_button(s) })
                .on_press(Message::ProjectView(Arc::from(section.path.as_str())));
            body = body.push(container(row![open, project, remove].spacing(4).align_y(Center)).padding(Padding { top: 0.0, right: 4.0, bottom: 0.0, left: 8.0 }));
        }
        body
    }

    /// The levels to make a plan of `kind` of, high to low, under its kind's header.
    fn level_picker<'a>(&'a self, p: Palette, l: Language, mut body: Column<'a, Message>, kind: PlanKind, word: &'static str) -> Column<'a, Message> {
        let levels = self.document.levels();
        let hint = if levels.is_empty() { l.text("Dit model heeft geen niveaus. Voeg er een toe in het tabblad Niveaus.", "This model has no levels. Add one in the Levels tab.") } else { l.text("Van niveau", "Of level") };
        body = body.push(container(text(hint).size(11).color(p.text_muted).wrapping(Wrapping::WordOrGlyph)).padding([2, 8]));
        for level in levels.iter().rev() {
            let plus = svg(crate::icons::handle(crate::icons::ADD)).width(12).height(12).style(move |_, _| svg::Style { color: Some(p.accent) });
            let label = column![text(level.name.clone()).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph)].width(Fill);
            let elevation = text(format!("{:+.3} m", level.elevation)).size(11).color(p.text_secondary);
            let make = match kind {
                PlanKind::Floor => Message::NewPlan(Arc::from(level.path.as_str())),
                _ => Message::NewPlanOf(Arc::from(level.path.as_str()), kind, word),
            };
            body = body.push(
                button(row![plus, label, elevation].spacing(6).align_y(Center))
                    .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 16.0 })
                    .width(Fill)
                    .style(move |_, s| p.row_button(s))
                    .on_press(make),
            );
        }
        body
    }
}

/// A camera as a 3D view keeps it.
fn camera_3d(c: &Camera) -> Camera3d {
    let f = |v: glam::Vec3| [f64::from(v.x), f64::from(v.y), f64::from(v.z)];
    Camera3d { eye: f(c.eye()), target: f(c.target), perspective: !c.ortho, fov: f64::from(c.fov_y.to_degrees()) }
}

/// The camera of a 3D view, orbiting its target; `radius`: the scene's, for the near and far
/// planes.
fn camera_of(v: &Camera3d, radius: f32) -> Camera {
    let f = |a: [f64; 3]| glam::Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32);
    let (eye, target) = (f(v.eye), f(v.target));
    let d = eye - target;
    let distance = d.length().max(1e-3);
    let max_pitch = 89.9f32.to_radians();
    Camera {
        target,
        distance,
        yaw: d.y.atan2(d.x),
        pitch: (d.z / distance).clamp(-1.0, 1.0).asin().clamp(-max_pitch, max_pitch),
        fov_y: (v.fov as f32).clamp(1.0, 179.0).to_radians(),
        scene_radius: radius,
        ortho: !v.perspective,
    }
}

/// A section box as a 3D view keeps it.
fn view_box(b: &SectionBox) -> ViewBox {
    let f = |v: glam::Vec3| [f64::from(v.x), f64::from(v.y), f64::from(v.z)];
    ViewBox { min: f(b.min), max: f(b.max), turn: f64::from(b.turn), pivot: f(b.pivot) }
}

/// The section box of a 3D view.
fn section_box(b: &ViewBox) -> SectionBox {
    let f = |a: [f64; 3]| glam::Vec3::new(a[0] as f32, a[1] as f32, a[2] as f32);
    SectionBox { min: f(b.min), max: f(b.max), turn: b.turn as f32, pivot: f(b.pivot) }
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
        crate::app::test_ui(app, iced::Size::new(1400.0, 900.0))
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
        assert!(simulator(&app).find("None yet. New makes one of a level.").is_ok());
        // New lists the levels under its kind
        let _ = app.update(Message::PickLevelFor(Some(PlanKind::Floor)));
        assert!(simulator(&app).find("Of level").is_ok());
        click(&mut app, "01 first");
        assert_eq!(app.views_picker, None, "the list closes");
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
    fn a_3d_view_keeps_what_the_3d_tab_showed_and_how_it_was_left() {
        let mut app = App::default();
        app.set_model(fitted_room());
        // the 3D tab: turned, a section box under the ceiling, the furniture off
        app.anim = None;
        (app.camera.yaw, app.camera.pitch) = (0.7, 0.4);
        let mut b = SectionBox::around(&app.world_bounds().unwrap());
        b.max.z = 2.5;
        (app.section, app.section_on) = (Some(b), true);
        app.switch_layers(&[1], false);
        click(&mut app, "Views");
        assert!(simulator(&app).find("None yet. New keeps what the 3D tab shows.").is_ok());
        let made = app.camera;
        let _ = app.update(Message::NewView3d("3D view"));
        let view = app.document.views_3d().first().expect("a 3D view").clone();
        assert_eq!(view.name, "3D view 1");
        assert_eq!(view.layers, crate::document::LayerSwitches::from([("Furniture and casework".to_string(), false)]));
        assert_eq!(app.active_plan.as_deref(), Some(view.path.as_str()));
        assert_eq!(app.doc_tabs(), [DocTab::Model, DocTab::Plan(0)]);
        assert_eq!(app.tab_title(DocTab::Plan(0)), "3D view 1");
        // its tab shows what the 3D tab showed
        let near = |a: Camera, b: Camera| (a.yaw - b.yaw).abs() < 1e-4 && (a.pitch - b.pitch).abs() < 1e-4 && (a.distance - b.distance).abs() < 1e-3 && a.ortho == b.ortho;
        assert!(near(app.camera, made), "{:?} against {made:?}", app.camera);
        assert!(app.section_on && (app.section.unwrap().max.z - 2.5).abs() < 1e-6);
        assert!(!app.visible[1]);
        assert!(simulator(&app).find("3D VIEWS").is_ok());
        // it is no drawing: it turns, and its section box is there to drag
        assert_eq!(app.shown_view(), None);
        let _ = app.update(Message::Navigate(crate::viewport::Navigate::Orbit { dx: 40.0, dy: 10.0 }));
        assert!((app.camera.yaw - made.yaw).abs() > 0.1);
        let turned = app.camera;
        assert_eq!(app.document_now().view_3d(&view.path).unwrap().camera, camera_3d(&turned), "saved as it shows");
        // the 3D tab comes back as it was; the view keeps how it was left
        let _ = app.update(Message::SelectTab(0));
        assert_eq!(app.camera, made);
        assert_eq!(app.document.view_3d(&view.path).unwrap().camera, camera_3d(&turned));
        app.switch_layers(&[1], true);
        let _ = app.update(Message::CloseTab(1));
        let _ = app.update(Message::OpenPlan(Arc::from(view.path.as_str())));
        assert!(near(app.camera, turned), "{:?} against {turned:?}", app.camera);
        assert!(!app.visible[1], "the furniture stays off in the view");
        // removed, its tab closes and the 3D tab shows as it was left
        let _ = app.update(Message::RemoveView3d(Arc::from(view.path.as_str())));
        assert!(app.document.views_3d().is_empty());
        assert_eq!((app.active_plan.as_deref(), app.doc_tabs()), (None, vec![DocTab::Model]));
        assert!(app.visible[1]);
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

    /// A room with walls, a cupboard standing on its floor and a lamp under its ceiling, as Revit
    /// categories; levels at 0 and 3 m.
    fn fitted_room() -> Arc<Model> {
        let block = |name: &str, id: u64, lo: [f32; 3], hi: [f32; 3]| {
            let ([x0, y0, z0], [x1, y1, z1]) = (lo, hi);
            let positions = vec![x0,y0,z0, x1,y0,z0, x1,y1,z0, x0,y1,z0, x0,y0,z1, x1,y0,z1, x1,y1,z1, x0,y1,z1];
            let indices = vec![0,2,1, 0,3,2, 4,5,6, 4,6,7, 0,1,5, 0,5,4, 1,2,6, 1,6,5, 2,3,7, 2,7,6, 3,0,4, 3,4,7];
            Layer { name: name.into(), positions, indices, elements: vec![(id, 0, 36)], ..Layer::default() }
        };
        let layers = vec![
            block("Walls", 10, [0.0, 0.0, 0.0], [8.0, 5.0, 3.0]),
            block("Furniture and casework", 20, [3.0, 2.0, 0.0], [5.0, 3.0, 2.0]),
            block("Lighting fixtures", 30, [1.0, 1.0, 2.6], [1.5, 1.5, 2.9]),
        ];
        let mut model = Model::new(PathBuf::from("room.rvt"), "Revit", layers);
        model.project.levels = vec![("00".into(), 0.0), ("01".into(), 3.0)];
        Arc::new(model)
    }

    /// Whether a drawing has a line within x from..to.
    fn draws(lines: &[crate::drawing::Segment], from: f32, to: f32) -> bool {
        lines.iter().any(|[a, b]| (from..=to).contains(&a[0]) && (from..=to).contains(&b[0]))
    }

    #[test]
    fn a_structural_plan_leaves_out_what_carries_nothing_and_a_ceiling_plan_looks_up() {
        let mut app = App::default();
        app.set_model(fitted_room());
        let level = Arc::<str>::from(app.document.levels()[0].path.as_str());
        // the kinds are listed apart, each with its own New
        click(&mut app, "Views");
        assert!(simulator(&app).find("STRUCTURAL PLANS").is_ok() && simulator(&app).find("CEILING PLANS").is_ok());
        // a structural plan: the walls, not the cupboard or the lamp, in its tab and its drawing
        let _ = app.update(Message::NewPlanOf(level.clone(), PlanKind::Structural, "structure"));
        let plan = app.document.plans()[0].clone();
        assert_eq!((plan.name.as_str(), plan.kind), ("00 (structure)", PlanKind::Structural));
        assert_eq!(app.active_plan.as_deref(), Some(plan.path.as_str()));
        let (walls, cupboard, lamp) = (0, 1, 2);
        assert!(!app.hidden_test()(walls, 0) && app.hidden_test()(cupboard, 0) && app.hidden_test()(lamp, 0));
        assert!(app.hidden.is_empty(), "left out by the kind, not hidden by hand");
        app.cut_view(&plan.path);
        let drawing = app.cuts[plan.path.as_str()].clone();
        assert!(!drawing.segments.is_empty() && !draws(&drawing.below, 3.0, 5.0), "no cupboard");
        assert!(simulator(&app).find("Structural plan").is_ok(), "its kind in the right pane");
        // the 3D view shows them all again
        let _ = app.update(Message::SelectTab(0));
        assert!(!app.hidden_test()(cupboard, 0));
        let _ = app.update(Message::SelectTab(1));
        assert!(app.hidden_test()(cupboard, 0));
        // a ceiling plan: from below, cut at 2.3 m up to the next level, seeing the lamp
        let _ = app.update(Message::NewPlanOf(level, PlanKind::Ceiling, "ceiling"));
        let ceiling = app.document.plans()[1].clone();
        assert_eq!(ceiling.name, "00 (ceiling)");
        assert!(!app.hidden_test()(cupboard, 0), "a ceiling plan leaves nothing out");
        let section = app.section.expect("its range");
        assert!((section.min.z - 2.3).abs() < 1e-5 && (section.max.z - 3.0).abs() < 1e-5, "{section:?}");
        let (_, to, _) = app.anim.expect("the camera turns");
        assert!((to.pitch - ViewPreset::Bottom.orientation().1).abs() < 1e-3, "looking up");
        app.cut_view(&ceiling.path);
        let drawing = app.cuts[ceiling.path.as_str()].clone();
        assert!(draws(&drawing.below, 1.0, 1.5), "the lamp");
        assert!(!draws(&drawing.below, 3.0, 5.0), "no cupboard: it stands under the cut");
    }

    /// The four elevations are listed under Elevations, a section drawn on a sheet under
    /// Sections; the right pane names the kind.
    #[test]
    fn sections_and_elevations_are_listed_apart() {
        let mut app = App::default();
        app.set_model(fitted_room());
        let _ = app.update(Message::MakeElevations(["North elevation", "East elevation", "South elevation", "West elevation"]));
        app.new_section([0.0, 2.5], [8.0, 2.5], "Section");
        let kinds: Vec<(&str, SectionKind)> = app.document.sections().iter().map(|s| (s.name.as_str(), s.kind)).collect();
        assert_eq!(kinds[..2], [("North elevation", SectionKind::Elevation), ("East elevation", SectionKind::Elevation)]);
        assert_eq!(kinds[4], ("Section A", SectionKind::Section));
        click(&mut app, "Views");
        for label in ["SECTIONS", "ELEVATIONS", "Section A", "North elevation"] {
            assert!(simulator(&app).find(label).is_ok(), "{label}");
        }
        // the section shows: its kind in the right pane
        assert_eq!(app.document.view_name(app.active_plan.as_deref().unwrap()), Some("Section A"));
        assert!(simulator(&app).find("Section").is_ok());
        let north = Arc::<str>::from(app.document.sections()[0].path.as_str());
        let _ = app.update(Message::OpenPlan(north));
        assert!(simulator(&app).find("Elevation view").is_ok());
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
