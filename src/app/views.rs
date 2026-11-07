//! The explorer's Views tab: the floor plans of the project document, made per level and shown
//! from above, cut at their height above the level.

use super::{App, Message, FONT_BOLD};
use crate::model::{PlanRange, SavedView, ViewKind};
use crate::theme::Palette;
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, scrollable, svg, text, Column};
use iced::{Center, Element, Fill, Padding};
use openaec_ui::Language;
use std::sync::Arc;

impl App {
    /// Makes a floor plan of a level and shows it.
    pub(super) fn new_plan(&mut self, level: &str) {
        if let Some(plan) = self.document.add_plan(level) {
            self.open_plan(&plan);
        }
    }

    /// Shows a floor plan: from above, orthographic, the section box from just under its level up
    /// to its cut.
    pub(super) fn open_plan(&mut self, path: &str) {
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
        if self.show_view(&view) {
            self.active_plan = Some(path.to_string());
            self.active_view = None;
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
        app.section_on = false;
        app.active_plan = None;
        let _ = app.update(Message::OpenPlan(Arc::from(plan.path.as_str())));
        assert!(app.section_on);
        assert_eq!(app.active_plan.as_deref(), Some(plan.path.as_str()));
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
