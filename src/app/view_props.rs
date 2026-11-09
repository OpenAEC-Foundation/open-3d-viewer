//! The View section of the right pane's Properties tab: the view whose layers the pane shows (the
//! view in the viewport, or the view picked on a sheet), with its name, a floor plan's cut height
//! and view depth and a section's depth typed in. A value is used as soon as it is valid: the
//! view's drawing is cut again and its tab's section box follows. Until then the field keeps what
//! was typed; Enter shows the value in use again.

use super::layers::LayersOf;
use super::{prop_row, App, Message};
use crate::document::parse_elevation;
use crate::theme::Palette;
use iced::widget::text::Wrapping;
use iced::widget::{column, row, text, text_input};
use iced::{Center, Element, Length};
use openaec_ui::Language;

/// A property of a view typed in the right pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewField {
    Name,
    /// A floor plan's cut above its level.
    Cut,
    /// How far a floor plan looks below its level, a section or elevation beyond its line.
    Depth,
}

impl App {
    /// The view of the document whose properties the right pane shows: the view in the viewport
    /// or the view picked on a sheet; `None` for the 3D view (and where the pane shows no view).
    pub(super) fn properties_view(&self) -> Option<String> {
        match self.layers_of()? {
            LayersOf::Viewport => self.active_plan.clone(),
            LayersOf::Placed(path) => Some(path),
        }
    }

    /// What a field of the View section shows: what is being typed in it, else the value in use.
    pub(super) fn view_field_text(&self, path: &str, field: ViewField) -> String {
        if let Some((p, f, typed)) = &self.view_typing {
            if p == path && *f == field {
                return typed.clone();
            }
        }
        match field {
            ViewField::Name => self.document.view_name(path).unwrap_or_default().to_string(),
            ViewField::Cut => self.document.plan(path).map_or(String::new(), |p| format!("{:.3}", p.cut)),
            ViewField::Depth => {
                let depth = self.document.plan(path).map(|p| p.depth).or_else(|| self.document.section(path).map(|s| s.depth));
                depth.map_or(String::new(), |d| format!("{d:.3}"))
            }
        }
    }

    /// Takes what is typed in a field of the View section, and uses it when it is valid.
    pub(super) fn type_view_field(&mut self, field: ViewField, typed: String) {
        let Some(path) = self.properties_view() else { return };
        let drawn_again = match field {
            ViewField::Name => {
                self.document.set_view_name(&path, &typed);
                false
            }
            ViewField::Cut => parse_elevation(&typed).is_some_and(|cut| {
                self.document.plan(&path).is_some_and(|p| p.cut != cut) && self.document.set_cut(&path, cut)
            }),
            ViewField::Depth => parse_elevation(&typed).is_some_and(|depth| self.document.set_view_depth(&path, depth)),
        };
        if drawn_again {
            self.view_changed(&path);
        }
        self.view_typing = Some((path, field, typed));
    }

    /// A view's heights or depth changed: its drawing is cut again where it was cut, its tab's
    /// section box follows, and the sheet bar shows its cut height.
    pub(super) fn view_changed(&mut self, path: &str) {
        self.cut_again(path);
        self.fit_plan_section(path);
        if let Some((plan, typed)) = &mut self.cut_edit {
            if let Some(view) = self.document.plan(path).filter(|_| plan.as_str() == path) {
                *typed = format!("{:.3}", view.cut);
            }
        }
    }

    /// The properties of the view whose layers the right pane shows: its name and kind, a floor
    /// plan's level, cut height and view depth, a section's length and depth, and how many
    /// layers it shows otherwise than the model. The name, heights and depths can be typed.
    pub(super) fn view_properties(&self, p: Palette, l: Language) -> Option<Element<'_, Message>> {
        let of = self.layers_of()?;
        let row = |k: &str, v: String| prop_row(p, k.to_string(), v);
        let Some(path) = self.properties_view() else {
            let rows = column![row(l.text("Naam", "Name"), self.layers_view_name(&of, l)), row(l.text("Soort", "Kind"), l.text("3D-weergave", "3D view").into())];
            return Some(rows.spacing(5).into());
        };
        let field = |k: &str, f: ViewField| field_row(p, k, self.view_field_text(&path, f), f);
        let mut rows = column![field(l.text("Naam", "Name"), ViewField::Name)].spacing(5);
        if let Some(plan) = self.document.plan(&path) {
            let level = self.document.level(&plan.level).map(|lv| lv.name.clone()).unwrap_or_default();
            rows = rows
                .push(row(l.text("Soort", "Kind"), l.text("Plattegrond", "Floor plan").into()))
                .push(row(l.text("Niveau", "Level"), level))
                .push(field(l.text("Snijhoogte (m)", "Cut height (m)"), ViewField::Cut))
                .push(field(l.text("Kijkdiepte (m)", "View depth (m)"), ViewField::Depth));
        } else if let Some(s) = self.document.section(&path) {
            let length = (s.to[0] - s.from[0]).hypot(s.to[1] - s.from[1]);
            rows = rows
                .push(row(l.text("Soort", "Kind"), l.text("Doorsnede of aanzicht", "Section or elevation").into()))
                .push(row(l.text("Lengte", "Length"), format!("{length:.3} m")))
                .push(field(l.text("Diepte (m)", "Depth (m)"), ViewField::Depth));
        }
        let own = self.document.view_layers(&path).map_or(0, |o| o.len());
        let own = if own == 0 { l.text("zoals het model", "as the model").to_string() } else { own.to_string() };
        Some(rows.push(row(l.text("Eigen lagen", "Own layers"), own)).into())
    }
}

/// A property to type in: its label, and a field that sends what is typed.
fn field_row(p: Palette, label: &str, value: String, field: ViewField) -> Element<'static, Message> {
    row![
        text(label.to_string()).size(12).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph).width(Length::FillPortion(2)),
        text_input("", &value)
            .on_input(move |typed| Message::ViewField(field, typed))
            .on_submit(Message::ViewFieldDone)
            .size(12)
            .padding([2, 6])
            .width(Length::FillPortion(3)),
    ]
    .spacing(8)
    .align_y(Center)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::layers::tests::{app_with_plan, room, shows};
    use crate::app::paper::SheetPick;
    use crate::app::views::DocTab;
    use std::sync::Arc;

    fn type_in(app: &mut App, field: ViewField, typed: &str) {
        let _ = app.update(Message::ViewField(field, typed.into()));
    }

    /// The app with the plan of level 00 placed on a sheet (cut there), the sheet shown.
    fn plan_on_sheet() -> (App, Arc<str>) {
        let (mut app, plan) = app_with_plan();
        let _ = app.update(Message::NewSheet("Sheet"));
        let sheet = Arc::<str>::from(app.document.sheets()[0].path.as_str());
        let _ = app.update(Message::PlaceOnSheet(sheet, plan.clone()));
        assert!(app.cuts.contains_key(&*plan));
        (app, plan)
    }

    #[test]
    fn a_name_typed_renames_the_view_in_its_tab_and_in_the_ifcx() {
        let (mut app, plan) = app_with_plan();
        let other = app.document.levels()[1].path.clone();
        app.document.add_plan(&other).unwrap();
        type_in(&mut app, ViewField::Name, "Ground floor");
        assert_eq!(app.document.view_name(&plan), Some("Ground floor"));
        assert_eq!(app.tab_title(DocTab::Plan(0)), "Ground floor");
        let ifcx = serde_json::to_string(&app.document.ifcx_drawing_nodes()).unwrap();
        assert!(ifcx.contains("Ground floor"), "{ifcx}");
        // empty, or the other plan's name: the view keeps its name, the field what was typed
        for typed in [" ", "01"] {
            type_in(&mut app, ViewField::Name, typed);
            assert_eq!(app.document.view_name(&plan), Some("Ground floor"), "{typed:?}");
            assert_eq!(app.view_field_text(&plan, ViewField::Name), typed);
        }
        let _ = app.update(Message::ViewFieldDone);
        assert_eq!(app.view_field_text(&plan, ViewField::Name), "Ground floor");
    }

    #[test]
    fn a_cut_height_or_view_depth_typed_cuts_the_plan_again_and_its_box_follows() {
        let (mut app, plan) = plan_on_sheet();
        let both = app.cuts[&*plan].segments.len();
        let _ = app.update(Message::SelectTab(1));
        assert_eq!(app.active_plan.as_deref(), Some(&*plan));
        assert_eq!(app.view_field_text(&plan, ViewField::Cut), "1.200");
        type_in(&mut app, ViewField::Cut, "2,5");
        assert_eq!(app.document.plan(&plan).unwrap().cut, 2.5);
        let walls = app.cuts[&*plan].segments.len();
        assert!(walls < both, "the cupboard is under the cut now: {walls} of {both}");
        assert!((app.section.expect("the tab's box").max.z - 2.5).abs() < 1e-4);
        type_in(&mut app, ViewField::Depth, "0.5 m");
        assert_eq!(app.document.plan(&plan).unwrap().depth, 0.5);
        assert!((app.section.unwrap().min.z + 0.5).abs() < 1e-4);
        // not a height, or under the view depth: not used, the field keeps what was typed
        for typed in ["", "abc", "-0,6"] {
            type_in(&mut app, ViewField::Cut, typed);
            assert_eq!(app.document.plan(&plan).unwrap().cut, 2.5, "{typed:?}");
            assert_eq!(app.view_field_text(&plan, ViewField::Cut), typed);
        }
        type_in(&mut app, ViewField::Depth, "-1");
        assert_eq!(app.document.plan(&plan).unwrap().depth, 0.5);
        let _ = app.update(Message::ViewFieldDone);
        assert_eq!(app.view_field_text(&plan, ViewField::Cut), "2.500");
        assert_eq!(app.view_field_text(&plan, ViewField::Depth), "0.500");
    }

    #[test]
    fn picked_on_a_sheet_a_view_is_typed_into_and_the_sheet_bar_follows() {
        let (mut app, plan) = plan_on_sheet();
        let both = app.cuts[&*plan].segments.len();
        let _ = app.update(Message::SelectTab(0));
        let _ = app.update(Message::SelectTab(2));
        let _ = app.update(Message::PickOnSheet(Some(SheetPick::View(0))));
        assert_eq!(app.properties_view().as_deref(), Some(&*plan));
        type_in(&mut app, ViewField::Cut, "2.5");
        assert!(app.cuts[&*plan].segments.len() < both);
        assert_eq!(app.cut_edit, Some((plan.to_string(), "2.500".to_string())));
        // typed in the sheet bar, the right pane shows it
        let _ = app.update(Message::CutText("1.2".into()));
        let _ = app.update(Message::ApplyCut);
        assert_eq!(app.view_field_text(&plan, ViewField::Cut), "1.200");
        assert_eq!(app.cuts[&*plan].segments.len(), both);
    }

    #[test]
    fn a_sections_depth_typed_cuts_it_again() {
        let mut app = App::default();
        app.set_model(room());
        // across the room at y 2.5, looking to -y: the cupboard is cut, the wall at y 0 is seen
        let section = app.document.add_section("Section A", [9.0, 2.5], [-1.0, 2.5], 10.0).unwrap();
        let _ = app.update(Message::OpenPlan(Arc::from(section.as_str())));
        assert_eq!(app.active_plan.as_deref(), Some(section.as_str()));
        app.cut_view(&section);
        let lines = |app: &App| app.cuts[&section].segments.len() + app.cuts[&section].below.len();
        let far = lines(&app);
        let before = app.section;
        assert_eq!(app.view_field_text(&section, ViewField::Depth), "10.000");
        type_in(&mut app, ViewField::Depth, "1");
        assert_eq!(app.document.section(&section).unwrap().depth, 1.0);
        assert!(lines(&app) < far, "the wall at y 0 is out of sight: {} of {far}", lines(&app));
        assert_ne!(app.section, before, "its tab's box follows");
        type_in(&mut app, ViewField::Depth, "0");
        assert_eq!(app.document.section(&section).unwrap().depth, 1.0);
    }

    #[test]
    fn the_view_section_has_fields_for_what_can_be_typed() {
        let (mut app, _) = app_with_plan();
        assert!(shows(&app, "Cut height (m)"));
        assert!(shows(&app, "View depth (m)"));
        let _ = app.update(Message::SelectTab(0));
        assert!(shows(&app, "3D view"));
        assert!(!shows(&app, "Cut height (m)"), "the 3D view has none");
    }
}
