//! The model explorer of the Modeller: the modelled project as a tree, above the properties in
//! the right pane, as in the reference modeller.
//!
//! - **Project**
//!   - **Storeys**, from the lowest, each with its elevation; a click opens its floor plan. Under a
//!     storey each kind of element on it with its count, under a kind each element by its name; a
//!     click on an element selects it.
//!   - **No level**: the elements that have none, by kind, only when there are such.
//!   - **Grid lines**, each with its length; a click picks it.
//!   - **Types**: each type the elements use, with how many use it; a click selects them.
//!   - **Views and sheets**; a click opens one.
//!
//! A branch shows what it holds while it is open, at most [`MOST`] rows, then a row "More" with how
//! many more there are, which selects them.
//! The tree is worked out from the project each time it is drawn, so it is never behind; which
//! branches are open is kept, and selecting an element opens the branches that hold it.

use super::browser::{class_label, BrowserMessage, CLASSES};
use crate::app::{App, Message, FONT_BOLD};
use crate::selection::Pickable;
use crate::theme::Palette;
use bim_core::{Element, ElementId, ProfileDef, Shape};
use iced::widget::text::Wrapping;
use iced::widget::{button, container, row, scrollable, text, Column, Space};
use iced::{Center, Element as Widget, Fill, Padding};
use openaec_ui::Language;
use std::sync::Arc;

/// A branch lists at most this many rows, then how many more there are.
pub const MOST: usize = 400;

/// A row of the explorer.
#[derive(Debug, Clone, PartialEq)]
pub struct ExplorerRow {
    /// How deep in the tree (the project is 0).
    pub depth: u8,
    /// The key that opens and closes it (a branch), else empty.
    pub key: String,
    pub label: String,
    /// What stands beside the label: a level's elevation, a grid line's length.
    pub beside: String,
    /// How many it holds (a branch), or for a type how many use it.
    pub count: Option<usize>,
    /// Open or closed (a branch); `None`: no branch.
    pub open: Option<bool>,
    /// It is the element selected, or the grid line picked.
    pub picked: bool,
    pub on_click: ExplorerMessage,
}

/// What a row of the explorer sends.
#[derive(Debug, Clone, PartialEq)]
pub enum ExplorerMessage {
    /// Open or close the branch of this key.
    Toggle(String),
    /// Select these elements in place of the selection.
    Select(Vec<ElementId>),
    /// Open the floor plan of the level at this path.
    Level(Arc<str>),
    /// Pick this grid line of the modelled project.
    Grid(usize),
    /// Open this view (a plan, a section, a 3D view).
    View(Arc<str>),
    /// Open this sheet.
    Sheet(Arc<str>),
}

/// The kinds the tree lists: the building components (a connection has no body and is listed by
/// the elements it joins).
fn listed(class: &str) -> bool {
    !matches!(class, "IfcElementAssembly" | "IfcRelConnectsElements")
}

/// The branch a kind of element hangs under, by its parent's key.
fn kind_key(parent: &str, class: &str) -> String {
    format!("{parent}/{class}")
}

impl App {
    /// The key of the branch that holds element `e`'s kind: its level's, or that of no level.
    fn level_key(&self, e: &Element) -> String {
        match e.level.as_deref().filter(|p| self.document.level(p).is_some()) {
            Some(path) => format!("level:{path}"),
            None => "nolevel".to_string(),
        }
    }

    /// The modelled elements selected.
    fn explorer_picked(&self) -> std::collections::HashSet<ElementId> {
        self.selected_modelled().into_iter().collect()
    }

    /// The name of the type element `e` uses, in the interface language; `None` without one.
    fn type_name(&self, e: &Element, l: Language) -> Option<String> {
        let lang = crate::app::library::lang(l);
        match &e.shape {
            Shape::Planar(p) => p.build_up.as_ref().map(|b| self.library.name(&b.id, lang).unwrap_or_else(|| b.id.clone())),
            Shape::System(s) => s.system_type.as_ref().map(|t| self.library.system_name(&t.id, lang).unwrap_or_else(|| t.id.clone())),
            Shape::Linear(m) => match &m.profile {
                ProfileDef::Catalogue { designation, .. } => Some(designation.clone()),
                _ => None,
            },
            _ => None,
        }
    }

    /// The rows of a parent's kinds and their elements (`depth` the kinds'), for `elements`.
    fn kind_rows(&self, rows: &mut Vec<ExplorerRow>, parent: &str, depth: u8, elements: &[&Element], picked: &std::collections::HashSet<ElementId>, l: Language) {
        for class in CLASSES.into_iter().filter(|c| listed(c)) {
            let of_kind: Vec<&&Element> = elements.iter().filter(|e| e.class() == class).collect();
            if of_kind.is_empty() {
                continue;
            }
            let key = kind_key(parent, class);
            let open = self.modeller.explorer_open.contains(&key);
            rows.push(ExplorerRow { depth, key: key.clone(), label: class_label(class, l).to_string(), beside: String::new(), count: Some(of_kind.len()), open: Some(open), picked: false, on_click: ExplorerMessage::Toggle(key) });
            if !open {
                continue;
            }
            for e in of_kind.iter().take(MOST) {
                rows.push(ExplorerRow { depth: depth + 1, key: String::new(), label: e.name.clone(), beside: String::new(), count: None, open: None, picked: picked.contains(&e.id), on_click: ExplorerMessage::Select(vec![e.id]) });
            }
            if of_kind.len() > MOST {
                let rest: Vec<ElementId> = of_kind.iter().skip(MOST).map(|e| e.id).collect();
                rows.push(ExplorerRow { depth: depth + 1, key: String::new(), label: l.text("Meer", "More").to_string(), beside: String::new(), count: Some(rest.len()), open: None, picked: false, on_click: ExplorerMessage::Select(rest) });
            }
        }
    }

    /// The rows of the explorer as it stands: the branches that are open show what they hold.
    pub(in crate::app) fn explorer_rows(&self, l: Language) -> Vec<ExplorerRow> {
        let open = |key: &str| self.modeller.explorer_open.contains(key);
        let branch = |depth: u8, key: &str, label: &str, count: usize| ExplorerRow {
            depth,
            key: key.to_string(),
            label: label.to_string(),
            beside: String::new(),
            count: Some(count),
            open: Some(open(key)),
            picked: false,
            on_click: ExplorerMessage::Toggle(key.to_string()),
        };
        let picked = self.explorer_picked();
        let elements: Vec<&Element> = self.modeller.model.iter().filter(|e| listed(e.class())).collect();
        let levels = self.document.levels();
        let mut rows = vec![ExplorerRow { depth: 0, key: "project".into(), label: l.text("Project", "Project").into(), beside: String::new(), count: None, open: Some(true), picked: false, on_click: ExplorerMessage::Toggle("project".into()) }];

        // the storeys, from the lowest, with what stands on them
        rows.push(branch(1, "storeys", l.text("Bouwlagen", "Storeys"), levels.len()));
        if open("storeys") {
            for level in levels {
                let key = format!("level:{}", level.path);
                let on: Vec<&Element> = elements.iter().copied().filter(|e| self.level_key(e) == key).collect();
                rows.push(ExplorerRow {
                    depth: 2,
                    key: key.clone(),
                    label: level.name.clone(),
                    beside: format!("{:+.3} m", level.elevation),
                    count: Some(on.len()),
                    open: Some(open(&key)),
                    picked: false,
                    on_click: ExplorerMessage::Level(Arc::from(level.path.as_str())),
                });
                if open(&key) {
                    self.kind_rows(&mut rows, &key, 3, &on, &picked, l);
                }
            }
        }
        // what stands on no level
        let loose: Vec<&Element> = elements.iter().copied().filter(|e| self.level_key(e) == "nolevel").collect();
        if !loose.is_empty() {
            rows.push(branch(1, "nolevel", l.text("Zonder niveau", "No level"), loose.len()));
            if open("nolevel") {
                self.kind_rows(&mut rows, "nolevel", 2, &loose, &picked, l);
            }
        }
        // the grid lines
        let grids = self.modeller.model.grids();
        rows.push(branch(1, "grids", l.text("Stramienen", "Grid lines"), grids.len()));
        if open("grids") {
            let picked_grid = self.picked_modelled_grid();
            for (i, g) in grids.iter().enumerate().take(MOST) {
                rows.push(ExplorerRow { depth: 2, key: String::new(), label: g.name.clone(), beside: format!("{:.3} m", (g.end - g.start).length()), count: None, open: None, picked: picked_grid == Some(i), on_click: ExplorerMessage::Grid(i) });
            }
        }
        // the types the elements use, by name
        let mut types: Vec<(String, Vec<ElementId>)> = Vec::new();
        for e in &elements {
            if let Some(name) = self.type_name(e, l) {
                match types.iter_mut().find(|(n, _)| *n == name) {
                    Some((_, ids)) => ids.push(e.id),
                    None => types.push((name, vec![e.id])),
                }
            }
        }
        rows.push(branch(1, "types", l.text("Types", "Types"), types.len()));
        if open("types") {
            for (name, ids) in types.into_iter().take(MOST) {
                rows.push(ExplorerRow { depth: 2, key: String::new(), label: name, beside: String::new(), count: Some(ids.len()), open: None, picked: false, on_click: ExplorerMessage::Select(ids) });
            }
        }
        // the views and the sheets
        let d = &self.document;
        let views: Vec<(String, ExplorerMessage)> = d
            .plans()
            .iter()
            .map(|v| (v.name.clone(), ExplorerMessage::View(Arc::from(v.path.as_str()))))
            .chain(d.sections().iter().map(|v| (v.name.clone(), ExplorerMessage::View(Arc::from(v.path.as_str())))))
            .chain(d.views_3d().iter().map(|v| (v.name.clone(), ExplorerMessage::View(Arc::from(v.path.as_str())))))
            .chain(d.sheets().iter().map(|s| (s.name.clone(), ExplorerMessage::Sheet(Arc::from(s.path.as_str())))))
            .collect();
        rows.push(branch(1, "views", l.text("Weergaven en tekeningen", "Views and sheets"), views.len()));
        if open("views") {
            for (name, on_click) in views.into_iter().take(MOST) {
                rows.push(ExplorerRow { depth: 2, key: String::new(), label: name, beside: String::new(), count: None, open: None, picked: false, on_click });
            }
        }
        rows
    }

    /// A message of the explorer.
    pub(in crate::app) fn explorer_message(&mut self, message: ExplorerMessage) {
        match message {
            ExplorerMessage::Toggle(key) if key == "project" => {}
            ExplorerMessage::Toggle(key) => {
                if !self.modeller.explorer_open.remove(&key) {
                    self.modeller.explorer_open.insert(key);
                }
            }
            ExplorerMessage::Select(ids) => self.select_elements(&ids),
            ExplorerMessage::Level(path) => self.browser_message(BrowserMessage::Level(path)),
            ExplorerMessage::Grid(i) => self.browser_message(BrowserMessage::Grid(i)),
            ExplorerMessage::View(path) => {
                let _ = self.update(Message::OpenPlan(path));
            }
            ExplorerMessage::Sheet(path) => {
                let _ = self.update(Message::OpenDocSheet(path));
            }
        }
    }

    /// Selects modelled elements `ids` in place of the selection.
    fn select_elements(&mut self, ids: &[ElementId]) {
        let Some(scene) = self.model.clone() else { return };
        let entry = if self.models.len() == 1 { 0 } else { self.models.iter().position(|e| Some(e.model.id) == self.modeller.shown).unwrap_or(0) as u32 };
        self.pick_grid(None);
        for id in ids {
            if let Some(shown) = scene.element_with_key(crate::model::ElementKey { model: entry, source: id.0 }) {
                self.sel.toggle(Pickable::Element(shown));
            }
        }
        self.selection_changed();
    }

    /// The selection changed: in the Modeller the branches that hold the elements selected open,
    /// so their rows are seen.
    pub(crate) fn explorer_follow_selection(&mut self) {
        if !self.modeller.in_modeller {
            return;
        }
        let keys: Vec<String> = self
            .selected_modelled()
            .into_iter()
            .filter_map(|id| self.modeller.model.get(id))
            .filter(|e| listed(e.class()))
            .flat_map(|e| {
                let parent = self.level_key(e);
                let top = if parent == "nolevel" { None } else { Some("storeys".to_string()) };
                top.into_iter().chain([kind_key(&parent, e.class()), parent])
            })
            .collect();
        self.modeller.explorer_open.extend(keys);
    }

    /// The explorer, drawn: a row per row of [`App::explorer_rows`], indented by its depth, a
    /// branch with its chevron to open or close it.
    pub(crate) fn model_explorer(&self, p: Palette, l: Language) -> Widget<'_, Message> {
        let mut list = Column::new().spacing(0);
        for r in self.explorer_rows(l) {
            let indent = Space::new().width(f32::from(r.depth) * 12.0);
            let chevron: Widget<'_, Message> = match r.open {
                Some(open) if r.depth > 0 => button(text(if open { "▾" } else { "▸" }).size(11).color(p.text_secondary))
                    .padding([1, 4])
                    .style(move |_, s| p.row_button(s))
                    .on_press(Message::Explorer(ExplorerMessage::Toggle(r.key.clone())))
                    .into(),
                _ => Space::new().width(17).into(),
            };
            let font = if r.depth <= 1 { FONT_BOLD } else { crate::app::FONT };
            let label = text(r.label.clone()).size(12).font(font).color(if r.picked { p.accent } else { p.text }).wrapping(Wrapping::WordOrGlyph).width(Fill);
            let beside = text(match r.count {
                Some(n) if r.beside.is_empty() => n.to_string(),
                _ => r.beside.clone(),
            })
            .size(11)
            .color(p.text_secondary);
            let picked = r.picked;
            let body = button(row![label, beside].spacing(6).align_y(Center))
                .padding([2, 6])
                .width(Fill)
                .style(move |_, s| if picked { p.ribbon_button(true, s) } else { p.row_button(s) })
                .on_press(Message::Explorer(r.on_click.clone()));
            list = list.push(row![indent, chevron, body].align_y(Center));
        }
        scrollable(container(list).padding(Padding { top: 2.0, right: 4.0, bottom: 6.0, left: 2.0 })).height(Fill).into()
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::modelling;
    use super::*;
    use crate::app::Message;
    use bim_core::element::PlanarForm;
    use bim_core::{PlanarDef, PointDef, Shape};
    use glam::{DVec2, DVec3};
    use openaec_ui::Language;

    fn labels(rows: &[ExplorerRow]) -> Vec<String> {
        rows.iter().map(|r| r.label.clone()).collect()
    }

    fn row<'a>(rows: &'a [ExplorerRow], label: &str) -> &'a ExplorerRow {
        rows.iter().find(|r| r.label == label).unwrap_or_else(|| panic!("no row {label}: {:?}", labels(rows)))
    }

    fn open(app: &mut App, key: &str) {
        let _ = app.update(Message::Explorer(ExplorerMessage::Toggle(key.to_string())));
    }

    #[test]
    fn the_tree_lists_storeys_kinds_and_elements_lowest_first() {
        let mut app = App::default();
        app.add_demo_building();
        app.modeller.in_modeller = true;
        for x in [0.0, 5.0, 10.0] {
            app.add_grid_line(DVec2::new(x, -2.0), DVec2::new(x, 8.0)).unwrap();
        }
        let ground = app.document.levels()[0].path.clone();
        let rows = app.explorer_rows(Language::English);
        assert_eq!(labels(&rows)[..2], ["Project".to_string(), "Storeys".to_string()]);
        open(&mut app, "storeys");
        let rows = app.explorer_rows(Language::English);
        let (zero, one) = (rows.iter().position(|r| r.label == "Level 0").unwrap(), rows.iter().position(|r| r.label == "Level 1").unwrap());
        assert!(zero < one, "the lowest first");
        open(&mut app, &format!("level:{ground}"));
        open(&mut app, &format!("level:{ground}/IfcWall"));
        let rows = app.explorer_rows(Language::English);
        assert_eq!(row(&rows, "Walls").count, Some(3));
        assert_eq!(rows.iter().filter(|r| r.depth == 4 && r.label.starts_with("Wall")).count(), 3, "{:?}", labels(&rows));
        open(&mut app, "grids");
        let rows = app.explorer_rows(Language::English);
        assert_eq!(row(&rows, "Grid lines").count, Some(3));
        assert!(rows.iter().any(|r| r.label == "1" && matches!(r.on_click, ExplorerMessage::Grid(0))));
        // types and views are there too
        assert!(rows.iter().any(|r| r.label == "Types") && rows.iter().any(|r| r.label == "Views and sheets"));
    }

    #[test]
    fn an_element_without_a_level_is_listed_apart() {
        let mut app = modelling();
        app.modeller.in_modeller = true;
        assert!(!labels(&app.explorer_rows(Language::English)).contains(&"No level".to_string()));
        let object = Shape::Point(PointDef { at: DVec3::ZERO, rotation: 0.0, size: DVec3::splat(0.5), opening: None });
        app.add_element("Object", None, object).unwrap();
        open(&mut app, "nolevel");
        open(&mut app, "nolevel/IfcBuildingElementProxy");
        let rows = app.explorer_rows(Language::English);
        assert_eq!(row(&rows, "No level").count, Some(1));
        assert!(rows.iter().any(|r| r.label == "Object 1"), "{:?}", labels(&rows));
    }

    #[test]
    fn a_branch_lists_400_rows_and_says_how_many_more() {
        let mut app = modelling();
        app.modeller.in_modeller = true;
        let ground = app.document.levels()[0].path.clone();
        for i in 0..405 {
            let object = Shape::Point(PointDef { at: DVec3::new(i as f64, 0.0, 0.0), rotation: 0.0, size: DVec3::splat(0.5), opening: None });
            app.add_element("Object", Some(ground.clone()), object).unwrap();
        }
        for key in ["storeys".to_string(), format!("level:{ground}"), format!("level:{ground}/IfcBuildingElementProxy")] {
            open(&mut app, &key);
        }
        let rows = app.explorer_rows(Language::English);
        assert_eq!(rows.iter().filter(|r| r.depth == 4).count(), 401, "400 and the row of the rest");
        assert!(rows.iter().any(|r| r.label == "More" && r.count == Some(5)), "the row of the rest");
    }

    #[test]
    fn a_click_selects_toggles_and_the_selection_opens_its_branch() {
        let mut app = modelling();
        app.modeller.in_modeller = true;
        let ground = app.document.levels()[0].path.clone();
        let wall = |x: f64| Shape::Planar(PlanarDef { form: PlanarForm::Wall { start: DVec2::new(x, 0.0), end: DVec2::new(x, 4.0), base: 0.0, height: 3.0 }, thickness: 0.2, openings: Vec::new(), build_up: None });
        let a = app.add_element("Wall", Some(ground.clone()), wall(0.0)).unwrap();
        let b = app.add_element("Wall", Some(ground.clone()), wall(3.0)).unwrap();
        // a click on a row selects its element
        let _ = app.update(Message::Explorer(ExplorerMessage::Select(vec![b])));
        assert_eq!(app.selected_modelled(), vec![b]);
        // selecting in the viewport marks the row, its branches open
        app.select_modelled(a);
        let rows = app.explorer_rows(Language::English);
        let picked: Vec<&str> = rows.iter().filter(|r| r.picked).map(|r| r.label.as_str()).collect();
        assert_eq!(picked, ["Wall 1"]);
        // a branch closes and opens again
        open(&mut app, "storeys");
        assert!(!labels(&app.explorer_rows(Language::English)).contains(&"Level 0".to_string()));
        open(&mut app, "storeys");
        assert!(labels(&app.explorer_rows(Language::English)).contains(&"Level 0".to_string()));
        // a removed element leaves no row
        let _ = app.update(Message::DeletePicked);
        if !labels(&app.explorer_rows(Language::English)).contains(&"Level 0".to_string()) {
            open(&mut app, "storeys");
        }
        let rows = app.explorer_rows(Language::English);
        assert!(!rows.iter().any(|r| r.label == "Wall 1"));
        assert_eq!(row(&rows, "Walls").count, Some(1));
    }
}
