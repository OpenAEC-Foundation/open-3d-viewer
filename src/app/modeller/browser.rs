//! The project browser: the Project tab of the explorer, in the Modeller only. One list of what
//! the modelled project holds, in the order a project is read:
//!
//! - **Views**: the floor plans (highest level first), the other plans per kind, the sections and
//!   elevations, the 3D views and the sheets. A click opens the view in its tab; the one shown is
//!   marked.
//! - **Levels**, high to low, each with its elevation. A click opens the level's floor plan, and
//!   makes one when it has none.
//! - **Grid lines**, by name. A click picks the grid line.
//! - **Building components**: the modelled elements per kind with how many there are. A click
//!   selects them all.
//!
//! The other tabs of the explorer stay as they are: this one gathers, it does not replace.

use crate::app::{App, Message, FONT_BOLD};
use crate::document::PlanKind;
use crate::selection::Pickable;
use crate::theme::Palette;
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, scrollable, text, Column};
use iced::{Center, Element, Fill, Padding};
use openaec_ui::Language;
use std::sync::Arc;

/// What a row of the project browser sends.
#[derive(Debug, Clone)]
pub enum BrowserMessage {
    /// A level (its path): open its floor plan, making one when it has none.
    Level(Arc<str>),
    /// A grid line of the modelled project (its index): pick it.
    Grid(usize),
    /// A kind of building component (its IFC class): select every modelled element of it.
    Class(&'static str),
}

/// The kinds of building components in the order they are listed, with their names.
fn class_label(class: &str, l: Language) -> &'static str {
    match class {
        "IfcWall" => l.text("Wanden", "Walls"),
        "IfcSlab" => l.text("Vloeren", "Slabs"),
        "IfcColumn" => l.text("Kolommen", "Columns"),
        "IfcBeam" => l.text("Balken", "Beams"),
        "IfcMember" => l.text("Staven", "Members"),
        "IfcCurtainWall" => l.text("Bouwsystemen", "Building systems"),
        "IfcBuildingElementProxy" => l.text("Objecten", "Objects"),
        _ => l.text("Verbindingen", "Connections"),
    }
}

/// The order of the kinds in the list.
const CLASSES: [&str; 9] = ["IfcWall", "IfcSlab", "IfcColumn", "IfcBeam", "IfcMember", "IfcCurtainWall", "IfcBuildingElementProxy", "IfcElementAssembly", "IfcRelConnectsElements"];

impl App {
    /// Whether the explorer has its Project tab: the Modeller is shown.
    pub(in crate::app) fn has_project_browser(&self) -> bool {
        self.modeller.in_modeller
    }

    /// The modelled elements per kind, in list order: the class and how many; connections of
    /// either class count as one kind.
    pub(in crate::app) fn component_counts(&self) -> Vec<(&'static str, usize)> {
        let mut counts: Vec<(&'static str, usize)> = Vec::new();
        for class in CLASSES {
            let n = self.modeller.model.iter().filter(|e| e.class() == class).count();
            if n == 0 {
                continue;
            }
            match counts.last_mut().filter(|_| class == "IfcRelConnectsElements") {
                Some(last) if last.0 == "IfcElementAssembly" => last.1 += n,
                _ => counts.push((class, n)),
            }
        }
        counts
    }

    /// A message of a row of the project browser.
    pub(in crate::app) fn browser_message(&mut self, message: BrowserMessage) {
        match message {
            BrowserMessage::Level(path) => {
                let plan = self.document.plans().iter().find(|p| p.level == *path && p.kind == PlanKind::Floor).map(|p| p.path.clone());
                match plan {
                    Some(plan) => self.open_plan(&plan),
                    None => self.new_plan(&path),
                }
            }
            BrowserMessage::Grid(index) => {
                let shown = self.modelled_grid_lines().into_iter().nth(index).and_then(|line| self.model.as_ref()?.project.grids.iter().position(|g| *g == line));
                self.pick_grid(shown);
            }
            BrowserMessage::Class(class) => self.select_class(class),
        }
    }

    /// Selects every modelled element of a kind (connections of either class together).
    fn select_class(&mut self, class: &str) {
        let Some(scene) = self.model.clone() else { return };
        let entry = if self.models.len() == 1 { 0 } else { self.models.iter().position(|e| Some(e.model.id) == self.modeller.shown).unwrap_or(0) as u32 };
        let joins = |c: &str| matches!(c, "IfcElementAssembly" | "IfcRelConnectsElements");
        let of_kind = |c: &str| c == class || (joins(c) && joins(class));
        self.pick_grid(None);
        let ids: Vec<u64> = self.modeller.model.iter().filter(|e| of_kind(e.class())).map(|e| e.id.0).collect();
        for id in ids {
            if let Some(shown) = scene.element_with_key(crate::model::ElementKey { model: entry, source: id }) {
                self.sel.toggle(Pickable::Element(shown));
            }
        }
        self.selection_changed();
    }

    /// The Project tab: the views, the levels, the grid lines and the building components.
    pub(in crate::app) fn project_browser(&self, p: Palette, l: Language) -> Element<'_, Message> {
        if self.model.is_none() {
            return text(l.text("Geen model geopend", "No model loaded")).size(12).color(p.text_muted).into();
        }
        let header = |label: &str, count: usize| -> Element<'static, Message> {
            container(row![text(label.to_string()).size(12).font(FONT_BOLD).width(Fill), text(count.to_string()).size(11).color(p.text_secondary)].spacing(6).align_y(Center))
                .padding([4, 6])
                .style(move |_| p.panel_header())
                .into()
        };
        let subheader = |label: &str| -> Element<'static, Message> {
            container(text(label.to_uppercase()).size(10).font(FONT_BOLD).color(p.group_label)).padding(Padding { top: 8.0, right: 0.0, bottom: 2.0, left: 8.0 }).into()
        };
        // a row: its name, what stands beside it, whether it is the one shown or picked
        let item = |name: String, beside: String, active: bool, message: Message| -> Element<'static, Message> {
            let name = text(name).size(12).color(if active { p.accent } else { p.text }).wrapping(Wrapping::WordOrGlyph).width(Fill);
            let beside = text(beside).size(11).color(p.text_secondary);
            button(row![name, beside].spacing(6).align_y(Center))
                .padding(Padding { top: 3.0, right: 6.0, bottom: 3.0, left: 14.0 })
                .width(Fill)
                .style(move |_, s| if active { p.ribbon_button(true, s) } else { p.row_button(s) })
                .on_press(message)
                .into()
        };
        let none = |what: &str| -> Element<'static, Message> { container(text(what.to_string()).size(11).color(p.text_muted)).padding(Padding { top: 2.0, right: 6.0, bottom: 2.0, left: 14.0 }).into() };
        let document = &self.document;
        let shown = self.active_plan.as_deref();
        let open = |path: &str| Message::OpenPlan(Arc::from(path));
        let elevation = |path: &str| document.level(path).map_or(f64::NEG_INFINITY, |level| level.elevation);
        let mut body = Column::new().spacing(1);

        // the views: plans per kind by level from high to low, sections, 3D views, sheets
        let views = document.plans().len() + document.sections().len() + document.views_3d().len() + document.sheets().len();
        body = body.push(header(l.text("Weergaven", "Views"), views));
        for kind in PlanKind::ALL {
            let mut plans: Vec<_> = document.plans().iter().filter(|plan| plan.kind == kind).collect();
            if plans.is_empty() && kind != PlanKind::Floor {
                continue;
            }
            plans.sort_by(|a, b| elevation(&b.level).total_cmp(&elevation(&a.level)));
            body = body.push(subheader(match kind {
                PlanKind::Floor => l.text("Plattegronden", "Floor plans"),
                PlanKind::Structural => l.text("Constructieve plattegronden", "Structural plans"),
                PlanKind::Ceiling => l.text("Plafondplannen", "Ceiling plans"),
                PlanKind::StructuralUp => l.text("Constructieve plattegronden, omhoog gezien", "Structural plans, looking up"),
            }));
            if plans.is_empty() {
                body = body.push(none(l.text("Nog geen plattegrond", "No floor plan yet")));
            }
            for plan in plans {
                body = body.push(item(plan.name.clone(), String::new(), shown == Some(plan.path.as_str()), open(&plan.path)));
            }
        }
        if !document.sections().is_empty() {
            body = body.push(subheader(l.text("Doorsneden en aanzichten", "Sections and elevations")));
            for section in document.sections() {
                body = body.push(item(section.name.clone(), String::new(), shown == Some(section.path.as_str()), open(&section.path)));
            }
        }
        if !document.views_3d().is_empty() {
            body = body.push(subheader(l.text("3D-weergaven", "3D views")));
            for view in document.views_3d() {
                body = body.push(item(view.name.clone(), String::new(), shown == Some(view.path.as_str()), open(&view.path)));
            }
        }
        if !document.sheets().is_empty() {
            body = body.push(subheader(l.text("Tekeningen", "Drawings")));
            for sheet in document.sheets() {
                let active = self.active_doc_sheet.as_deref() == Some(sheet.path.as_str());
                body = body.push(item(sheet.name.clone(), String::new(), active, Message::OpenDocSheet(Arc::from(sheet.path.as_str()))));
            }
        }

        // the levels, high to low
        body = body.push(header(l.text("Niveaus", "Levels"), document.levels().len()));
        for level in document.levels().iter().rev() {
            let active = shown.and_then(|path| document.plan(path)).is_some_and(|plan| plan.level == level.path);
            body = body.push(item(level.name.clone(), format!("{:+.3} m", level.elevation), active, Message::Browser(BrowserMessage::Level(Arc::from(level.path.as_str())))));
        }

        // the grid lines of the modelled project
        let grids = self.modeller.model.grids();
        body = body.push(header(l.text("Stramienen", "Grid lines"), grids.len()));
        if grids.is_empty() {
            body = body.push(none(l.text("Nog geen stramien", "No grid line yet")));
        }
        let picked = self.picked_modelled_grid();
        for (i, grid) in grids.iter().enumerate() {
            body = body.push(item(grid.name.clone(), format!("{:.3} m", (grid.end - grid.start).length()), picked == Some(i), Message::Browser(BrowserMessage::Grid(i))));
        }

        // the building components per kind
        let counts = self.component_counts();
        body = body.push(header(l.text("Bouwdelen", "Building components"), counts.iter().map(|c| c.1).sum()));
        if counts.is_empty() {
            body = body.push(none(l.text("Nog niets gemodelleerd", "Nothing modelled yet")));
        }
        for (class, n) in counts {
            body = body.push(item(class_label(class, l).to_string(), n.to_string(), false, Message::Browser(BrowserMessage::Class(class))));
        }
        scrollable(column![body].padding(Padding { top: 0.0, right: 4.0, bottom: 8.0, left: 0.0 })).height(Fill).spacing(4).into()
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::modelling;
    use super::*;
    use crate::app::ExplorerTab;
    use glam::DVec2;

    fn simulator(app: &App) -> iced_test::Simulator<'_, Message> {
        iced_test::simulator(app.explorer_body(crate::theme::ThemeKind::Light.palette(), Language::English))
    }

    #[test]
    fn the_modeller_has_a_project_tab_that_lists_the_project() {
        let mut app = App::default();
        app.add_demo_building();
        app.keep_model_history();
        // not in the Viewer
        assert!(simulator(&app).find("Project").is_err());
        app.modeller.in_modeller = true;
        let _ = app.update(Message::ExplorerTab(ExplorerTab::Project));
        app.add_grid_line(DVec2::new(0.0, -1.0), DVec2::new(0.0, 7.0)).unwrap();
        let ui = |app: &App, label: &str| simulator(app).find(label).is_ok();
        for label in ["Views", "FLOOR PLANS", "Levels", "Level 0", "Grid lines", "Building components", "Walls", "Slabs", "Building systems", "Connections"] {
            assert!(ui(&app, label), "{label}");
        }
        // the counts: three walls, the joins of either class as one kind
        let counts = app.component_counts();
        assert_eq!(counts.iter().find(|c| c.0 == "IfcWall").map(|c| c.1), Some(3));
        let joins = app.modeller.model.iter().filter(|e| e.base() == bim_core::BaseKind::Connection).count();
        assert_eq!(counts.iter().filter(|c| matches!(c.0, "IfcElementAssembly" | "IfcRelConnectsElements")).map(|c| c.1).sum::<usize>(), joins);
        assert_eq!(counts.iter().filter(|c| matches!(c.0, "IfcElementAssembly" | "IfcRelConnectsElements")).count(), 1);
    }

    #[test]
    fn a_click_opens_a_levels_plan_picks_a_grid_line_and_selects_a_kind() {
        let mut app = modelling();
        app.modeller.in_modeller = true;
        let level = Arc::<str>::from(app.document.levels()[0].path.as_str());
        // the level has no floor plan yet: one is made and shown; the next click opens that one
        let _ = app.update(Message::Browser(BrowserMessage::Level(level.clone())));
        assert_eq!(app.document.plans().len(), 1);
        let plan = app.document.plans()[0].path.clone();
        assert_eq!(app.active_plan.as_deref(), Some(plan.as_str()));
        let _ = app.update(Message::Browser(BrowserMessage::Level(level)));
        assert_eq!(app.document.plans().len(), 1);
        // a grid line is picked by its row
        app.add_grid_line(DVec2::new(0.0, -1.0), DVec2::new(0.0, 7.0)).unwrap();
        app.add_grid_line(DVec2::new(-1.0, 2.0), DVec2::new(7.0, 2.0)).unwrap();
        let _ = app.update(Message::Browser(BrowserMessage::Grid(0)));
        assert_eq!(app.picked_modelled_grid(), Some(0));
        // every wall is selected by the row of the walls
        for x in [1.0, 3.0] {
            let form = bim_core::element::PlanarForm::Wall { start: DVec2::new(x, 0.0), end: DVec2::new(x, 4.0), base: 0.0, height: 3.0 };
            app.add_element("Wall", None, bim_core::Shape::Planar(bim_core::PlanarDef { form, thickness: 0.2, openings: Vec::new(), build_up: None })).unwrap();
        }
        let _ = app.update(Message::Browser(BrowserMessage::Class("IfcWall")));
        assert_eq!(app.selected_modelled().len(), 2);
        assert_eq!(app.picked_grid, None, "the grid line is let go");
    }
}
