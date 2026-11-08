//! The right pane's tabs and its Layers tab: the layers of the view shown, switched for that view
//! alone. The 3D view keeps its own layer switches in the app; a floor plan, section or elevation
//! keeps those it shows otherwise than the model in the project document (saved in its IfcX
//! node), and is cut through them when it is placed on a sheet. On a sheet, the Layers tab shows
//! the layers of the placed view picked. Undo goes back through the switches of the view shown.

use super::paper::SheetPick;
use super::{format_count, layer_group, App, Message, FONT, FONT_BOLD};
use crate::document::LayerSwitches;
use crate::icons;
use crate::theme::Palette;
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, rule, scrollable, svg, text, text_input, Column, Space};
use iced::{Center, Element, Fill, Padding};
use openaec_ui::Language;

/// The tabs of the right pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SideTab {
    /// The selection, the view shown and the model.
    #[default]
    Properties,
    /// The layers of the view shown.
    Layers,
}

/// Whose layers the Layers tab shows and switches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LayersOf {
    /// The view in the viewport: the 3D view or a view's tab (also under a sheet, when it is the
    /// view picked there).
    Viewport,
    /// A view placed on the sheet shown and picked there.
    Placed(String),
}

impl App {
    /// Whose layers the Layers tab shows: the viewport's, or on a sheet of the document the placed
    /// view picked; none on such a sheet without one, on a schedule or on a sheet of the model.
    pub(super) fn layers_of(&self) -> Option<LayersOf> {
        if self.active_doc_sheet.is_some() {
            let Some(SheetPick::View(i)) = self.sheet_pick else { return None };
            let path = self.placed_plan(i)?;
            return Some(if self.active_plan.as_deref() == Some(path.as_str()) { LayersOf::Viewport } else { LayersOf::Placed(path) });
        }
        let schedule = self.active_plan.is_none() && self.active_schedule.is_some();
        (self.active_sheet.is_none() && !schedule).then_some(LayersOf::Viewport)
    }

    /// Whether the model of layer `layer` is switched on.
    fn model_on(&self, layer: usize) -> bool {
        self.models.iter().find(|e| e.layers.contains(&layer)).is_none_or(|e| e.visible)
    }

    /// A view's own switch of every layer: its switches over the model's (the models' switches
    /// aside).
    fn switches_of(&self, path: &str) -> Vec<bool> {
        let Some(model) = &self.model else { return Vec::new() };
        let own = self.document.view_layers(path);
        model.layers.iter().map(|l| own.and_then(|o| o.get(&l.name)).copied().unwrap_or(!l.hidden)).collect()
    }

    /// The layers a view shows: its own switches, where their model is on.
    pub(super) fn view_visibility(&self, path: &str) -> Vec<bool> {
        self.switches_of(path).into_iter().enumerate().map(|(i, on)| on && self.model_on(i)).collect()
    }

    /// Every layer's own switch in the viewport (the models' switches aside).
    fn viewport_switches(&self) -> Vec<bool> {
        let mut on = self.visible.clone();
        for entry in &self.models {
            for (i, &v) in entry.layers.clone().zip(&entry.layer_visibility) {
                if let Some(o) = on.get_mut(i) {
                    *o = v;
                }
            }
        }
        on
    }

    /// Layer switches as a view keeps them: by name, those otherwise than the model has them.
    fn own_switches(&self, on: &[bool]) -> LayerSwitches {
        let Some(model) = &self.model else { return LayerSwitches::new() };
        model.layers.iter().zip(on).filter(|(l, &on)| on == l.hidden).map(|(l, &on)| (l.name.clone(), on)).collect()
    }

    /// Shows every layer as `on` says in the viewport, where its model is on.
    fn set_viewport_switches(&mut self, on: &[bool]) {
        for entry in &mut self.models {
            for (i, v) in entry.layers.clone().zip(entry.layer_visibility.iter_mut()) {
                if let Some(&o) = on.get(i) {
                    *v = o;
                }
            }
        }
        for (shown, &o) in self.visible.iter_mut().zip(on) {
            *shown = o;
        }
        self.refresh_visible();
    }

    /// Every layer shown where its model is on and its own switch is on.
    fn refresh_visible(&mut self) {
        for entry in &self.models {
            for (i, &v) in entry.layers.clone().zip(&entry.layer_visibility) {
                if let Some(shown) = self.visible.get_mut(i) {
                    *shown = entry.visible && v;
                }
            }
        }
        self.refresh_structural_visibility();
    }

    /// The viewport goes from the view shown so far to `target` (a view's path; `None`: the 3D
    /// view): the layers and the undo history of the one stay with it, those of the other come.
    pub(super) fn swap_view_layers(&mut self, target: Option<&str>) {
        let from = self.active_plan.clone();
        if from.is_none() {
            self.parked_layers = Some((self.visible.clone(), self.models.iter().map(|e| e.layer_visibility.clone()).collect()));
        }
        let history = std::mem::take(&mut self.history);
        self.parked_histories.insert(from.unwrap_or_default(), history);
        self.history = self.parked_histories.remove(target.unwrap_or_default()).unwrap_or_default();
        match target {
            Some(path) => {
                let on = self.switches_of(path);
                self.set_viewport_switches(&on);
            }
            None => {
                let Some((visible, models)) = self.parked_layers.take() else { return };
                for (entry, layers) in self.models.iter_mut().zip(models) {
                    if entry.layer_visibility.len() == layers.len() {
                        entry.layer_visibility = layers;
                    }
                }
                for (shown, v) in self.visible.iter_mut().zip(visible) {
                    *shown = v;
                }
                self.refresh_visible();
            }
        }
    }

    /// Keeps the layer switches of the view in the viewport in the document (the 3D view keeps
    /// its own in the app), and cuts it again when they changed.
    pub(super) fn keep_view_layers(&mut self) {
        let Some(path) = self.active_plan.clone() else { return };
        let own = self.own_switches(&self.viewport_switches());
        if self.document.set_view_layers(&path, own) {
            self.cut_again(&path);
        }
    }

    /// Cuts a view again that was cut, through its layers as they are now.
    pub(super) fn cut_again(&mut self, path: &str) {
        if self.cuts.remove(path).is_some() {
            self.cut_view(path);
        }
    }

    /// Cuts every view again that was cut (a model was switched on or off).
    pub(super) fn cut_all_again(&mut self) {
        let paths: Vec<String> = self.cuts.keys().cloned().collect();
        for path in paths {
            self.cut_again(&path);
        }
    }

    /// Switches layers on or off in the view whose layers the Layers tab shows (the viewport's
    /// while it shows none).
    pub(super) fn switch_layers(&mut self, layers: &[usize], on: bool) {
        match self.layers_of() {
            Some(LayersOf::Placed(path)) => {
                let mut switches = self.switches_of(&path);
                for &i in layers {
                    if let Some(s) = switches.get_mut(i) {
                        *s = on;
                    }
                }
                let own = self.own_switches(&switches);
                if self.document.set_view_layers(&path, own) {
                    self.cut_again(&path);
                }
            }
            _ => {
                for &i in layers {
                    self.set_layer_visible(i, on);
                }
            }
        }
    }

    /// The layers the search of the Layers tab finds (all without a search).
    pub(super) fn layers_found(&self) -> Vec<usize> {
        let Some(model) = &self.model else { return Vec::new() };
        let query = self.layers_query.trim().to_lowercase();
        (0..model.layers.len()).filter(|&i| query.is_empty() || model.layers[i].name.to_lowercase().contains(&query)).collect()
    }

    /// The right pane: its tabs, then the tab shown.
    pub fn properties_body(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let tab = |label: &'static str, t: SideTab| {
            let active = self.side_tab == t;
            button(text(label).size(12).font(if active { FONT_BOLD } else { FONT }))
                .padding([4, 12])
                .style(move |_, s| p.ribbon_button(active, s))
                .on_press(Message::SideTab(t))
        };
        let tabs = row![tab(l.text("Eigenschappen", "Properties"), SideTab::Properties), tab(l.text("Lagen", "Layers"), SideTab::Layers)].spacing(2);
        let body = match self.side_tab {
            SideTab::Properties => self.properties_tab(p, l),
            SideTab::Layers => self.layers_tab(p, l),
        };
        column![tabs, rule::horizontal(1), body].spacing(6).into()
    }

    /// The name of the view whose layers the Layers tab shows.
    pub(super) fn layers_view_name(&self, of: &LayersOf, l: Language) -> String {
        let path = match of {
            LayersOf::Viewport => self.active_plan.as_deref(),
            LayersOf::Placed(path) => Some(path.as_str()),
        };
        let name = path.and_then(|p| self.document.plan(p).map(|v| v.name.clone()).or_else(|| self.document.section(p).map(|s| s.name.clone())));
        name.unwrap_or_else(|| l.text("3D-weergave", "3D view").to_string())
    }

    /// The Layers tab: the view it applies to, a search field, Show all and Hide all, then the
    /// layers (those named "Group › Layer" under their group).
    fn layers_tab(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let Some(model) = &self.model else {
            return text(l.text("Geen model geopend", "No model loaded")).size(12).color(p.text_muted).into();
        };
        let Some(of) = self.layers_of() else {
            let hint = if self.active_doc_sheet.is_some() {
                l.text("Kies een weergave op het blad om zijn lagen te schakelen", "Pick a view on the sheet to switch its layers")
            } else {
                l.text("Deze tab heeft geen lagen", "This tab has no layers")
            };
            return container(text(hint).size(12).color(p.text_muted).wrapping(Wrapping::WordOrGlyph)).padding([4, 8]).into();
        };
        let shown = match &of {
            LayersOf::Viewport => self.visible.clone(),
            LayersOf::Placed(path) => self.view_visibility(path),
        };
        let is_shown = move |i: usize| shown.get(i).copied().unwrap_or(true);
        let searching = !self.layers_query.trim().is_empty();
        let indent = |content: Element<'static, Message>| -> Element<'static, Message> {
            container(content).padding(Padding { top: 0.0, right: 0.0, bottom: 0.0, left: 14.0 }).width(Fill).into()
        };
        let mut body = Column::new().spacing(1);
        let mut order = self.layers_found();
        order.sort_by(|&a, &b| model.layers[a].name.to_lowercase().cmp(&model.layers[b].name.to_lowercase()));
        body = body.push(
            container(
                column![
                    row![
                        text(self.layers_view_name(&of, l)).size(12).font(FONT_BOLD).width(Fill).wrapping(Wrapping::WordOrGlyph),
                        text(order.len().to_string()).size(11).color(p.text_secondary),
                    ]
                    .spacing(6)
                    .align_y(Center),
                    text(l.text("Deze lagen gelden alleen voor deze weergave", "These layers apply to this view only")).size(10).color(p.text_secondary),
                ]
                .spacing(2),
            )
            .padding([4, 6])
            .width(Fill)
            .style(move |_| p.panel_header()),
        );
        let small = |label: &'static str, msg: Message| {
            button(text(label).size(11)).padding([2, 8]).style(move |_, s| p.ribbon_button(false, s)).on_press(msg)
        };
        body = body.push(
            row![small(l.text("Alles tonen", "Show all"), Message::AllLayers(true)), small(l.text("Alles verbergen", "Hide all"), Message::AllLayers(false))]
                .spacing(4)
                .padding([4, 0]),
        );
        let eye_icon = move |visible: bool, partly: bool| {
            svg(icons::handle(if visible || partly { icons::EYE } else { icons::EYE_OFF }))
                .width(14)
                .height(14)
                .style(move |_, _| svg::Style { color: Some(if visible { p.text_secondary } else { p.text_muted }) })
        };
        let layer_row = |i: usize, label: String| -> Element<'static, Message> {
            let layer = &model.layers[i];
            let visible = is_shown(i);
            let [r, g, b] = layer.color;
            let swatch = container(Space::new().width(10).height(10)).style(move |_| Palette::swatch(iced::Color::from_rgb(r, g, b)));
            button(
                row![
                    eye_icon(visible, false),
                    swatch,
                    text(label).size(12).color(if visible { p.text } else { p.text_muted }).wrapping(Wrapping::WordOrGlyph).width(Fill),
                    text(format_count(if layer.points { layer.point_count() } else { layer.triangle_count() })).size(10).color(p.text_muted),
                ]
                .spacing(6)
                .align_y(Center),
            )
            .padding([3, 6])
            .width(Fill)
            .style(move |_, s| p.row_button(s))
            .on_press(Message::Layer(i, !visible))
            .into()
        };
        // Layers named "Group › Layer" (a BTL panel and its member types) sit under their group,
        // which shows or hides them together; the others follow on their own.
        let mut groups: Vec<(&str, Vec<usize>)> = Vec::new();
        for &i in &order {
            if let (Some(group), _) = layer_group(&model.layers[i].name) {
                match groups.iter_mut().find(|(g, _)| *g == group) {
                    Some((_, members)) => members.push(i),
                    None => groups.push((group, vec![i])),
                }
            }
        }
        for (group, members) in &groups {
            let on = members.iter().filter(|&&i| is_shown(i)).count();
            let (all, some) = (on == members.len(), on > 0);
            let open = searching || self.open_layer_groups.contains(*group);
            use openaec_ui::icons::Icon;
            let arrow = svg(svg::Handle::from_memory(if open { Icon::DisclosureExpanded } else { Icon::DisclosureCollapsed }.bytes()))
                .width(12)
                .height(12)
                .style(move |_, _| svg::Style { color: Some(p.text_secondary) });
            let triangles: usize = members.iter().map(|&i| model.layers[i].triangle_count()).sum();
            let eye = button(eye_icon(some, some && !all)).padding([3, 4]).style(move |_, s| p.row_button(s)).on_press(Message::LayerGroup(group.to_string(), !all));
            let title = button(
                row![
                    arrow,
                    text(group.to_string()).size(12).font(FONT_BOLD).color(if some { p.text } else { p.text_muted }).wrapping(Wrapping::WordOrGlyph).width(Fill),
                    text(format_count(triangles)).size(10).color(p.text_muted),
                ]
                .spacing(6)
                .align_y(Center),
            )
            .padding([3, 6])
            .width(Fill)
            .style(move |_, s| p.row_button(s))
            .on_press(Message::ToggleLayerGroup(group.to_string()));
            body = body.push(row![eye, title].spacing(0).align_y(Center));
            if open {
                for &i in members {
                    body = body.push(indent(layer_row(i, layer_group(&model.layers[i].name).1.to_string())));
                }
            }
        }
        for &i in order.iter().filter(|&&i| layer_group(&model.layers[i].name).0.is_none()) {
            body = body.push(layer_row(i, model.layers[i].name.clone()));
        }
        let search = text_input(l.text("Zoeken in lagen…", "Search layers…"), &self.layers_query).on_input(Message::LayersQuery).size(12).padding([5, 8]);
        let id = iced::widget::Id::new("layers");
        let list = scrollable(body.padding(Padding { top: 0.0, right: 4.0, bottom: 8.0, left: 0.0 })).id(id.clone()).height(Fill).spacing(4);
        // a finger drags the list, also where it starts on a row
        column![search, crate::swipe::swipe(id, false, list)].spacing(8).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::paper::SheetPick;
    use crate::app::Message;
    use crate::document::LayerSwitches;
    use crate::model::{Layer, Model};
    use std::path::PathBuf;
    use std::sync::Arc;

    /// A room of 8 x 5 m with walls 3 m high (layer 0, "Walls") and a cupboard in it (layer 1,
    /// "Furniture", a box 2 m high), levels 00 at 0 and 01 at 3.
    fn room() -> Arc<Model> {
        let positions = vec![0.,0.,0., 8.,0.,0., 8.,5.,0., 0.,5.,0., 0.,0.,3., 8.,0.,3., 8.,5.,3., 0.,5.,3.];
        let indices = vec![0,1,5, 0,5,4, 1,2,6, 1,6,5, 2,3,7, 2,7,6, 3,0,4, 3,4,7];
        let walls = Layer { name: "Walls".into(), positions, indices, elements: vec![(10, 0, 24)], ..Layer::default() };
        let positions = vec![3.,2.,0., 5.,2.,0., 5.,3.,0., 3.,3.,0., 3.,2.,2., 5.,2.,2., 5.,3.,2., 3.,3.,2.];
        let indices = vec![0,1,5, 0,5,4, 1,2,6, 1,6,5, 2,3,7, 2,7,6, 3,0,4, 3,4,7];
        let cupboard = Layer { name: "Furniture".into(), positions, indices, elements: vec![(20, 0, 24)], ..Layer::default() };
        let mut model = Model::new(PathBuf::from("room.rvt"), "Revit", vec![walls, cupboard]);
        model.project.levels = vec![("00".into(), 0.0), ("01".into(), 3.0)];
        Arc::new(model)
    }

    const WALLS: usize = 0;
    const FURNITURE: usize = 1;

    /// The app with the room and the floor plan of level 00 open in its tab.
    fn app_with_plan() -> (App, Arc<str>) {
        let mut app = App::default();
        app.set_model(room());
        let level = Arc::<str>::from(app.document.levels()[0].path.as_str());
        let _ = app.update(Message::NewPlan(level));
        let plan = Arc::<str>::from(app.document.plans()[0].path.as_str());
        assert_eq!(app.active_plan.as_deref(), Some(&*plan));
        (app, plan)
    }

    fn shows(app: &App, wording: &str) -> bool {
        let mut ui = iced_test::Simulator::with_size(iced::Settings::default(), iced::Size::new(1400.0, 900.0), app.view());
        ui.find(wording).is_ok()
    }

    fn off(name: &str) -> LayerSwitches {
        LayerSwitches::from([(name.to_string(), false)])
    }

    #[test]
    fn a_layer_switched_off_in_a_floor_plan_stays_on_in_the_3d_view() {
        let (mut app, plan) = app_with_plan();
        let _ = app.update(Message::Layer(FURNITURE, false));
        assert!(!app.visible[FURNITURE]);
        assert_eq!(app.document.view_layers(&plan), Some(&off("Furniture")), "kept in the document");
        let _ = app.update(Message::SelectTab(0));
        assert!(app.visible[FURNITURE], "the 3D view keeps its own layers");
        let _ = app.update(Message::Layer(WALLS, false));
        assert_eq!(app.document.view_layers(&plan), Some(&off("Furniture")), "the plan's stay");
        let _ = app.update(Message::SelectTab(1));
        assert_eq!((app.visible[WALLS], app.visible[FURNITURE]), (true, false));
        let _ = app.update(Message::SelectTab(0));
        assert_eq!((app.visible[WALLS], app.visible[FURNITURE]), (false, true));
        // on again in the plan: it shows the layers as the model does
        let _ = app.update(Message::SelectTab(1));
        let _ = app.update(Message::AllLayers(true));
        assert!(app.document.view_layers(&plan).unwrap().is_empty());
    }

    #[test]
    fn undo_goes_back_through_the_switches_of_the_view_shown() {
        let (mut app, _) = app_with_plan();
        let _ = app.update(Message::SelectTab(0));
        let _ = app.update(Message::Layer(WALLS, false));
        let _ = app.update(Message::SelectTab(1));
        let _ = app.update(Message::Layer(FURNITURE, false));
        let _ = app.update(Message::Undo);
        assert!(app.visible[FURNITURE]);
        assert!(!app.history.can_undo(), "the 3D view's switch is not undone here");
        let _ = app.update(Message::SelectTab(0));
        assert!(!app.visible[WALLS]);
        let _ = app.update(Message::Undo);
        assert!(app.visible[WALLS]);
    }

    #[test]
    fn a_placed_view_is_cut_through_its_own_layers_and_picked_on_a_sheet_they_switch_it() {
        let (mut app, plan) = app_with_plan();
        let _ = app.update(Message::NewSheet("Sheet"));
        let sheet = Arc::<str>::from(app.document.sheets()[0].path.as_str());
        let _ = app.update(Message::PlaceOnSheet(sheet.clone(), plan.clone()));
        let all = app.cuts[&*plan].segments.len();
        // in the plan's own tab
        let _ = app.update(Message::SelectTab(1));
        let _ = app.update(Message::Layer(FURNITURE, false));
        let walls = app.cuts[&*plan].segments.len();
        assert!(walls < all, "cut again without the cupboard: {walls} of {all}");
        // picked on the sheet, while the 3D view is under it
        let _ = app.update(Message::SelectTab(0));
        let _ = app.update(Message::SelectTab(2));
        assert_eq!(app.active_doc_sheet.as_deref(), Some(&*sheet));
        let _ = app.update(Message::PickOnSheet(Some(SheetPick::View(0))));
        let _ = app.update(Message::Layer(FURNITURE, true));
        assert!(app.document.view_layers(&plan).unwrap().is_empty());
        assert_eq!(app.cuts[&*plan].segments.len(), all);
        let _ = app.update(Message::Layer(WALLS, false));
        assert_eq!(app.document.view_layers(&plan), Some(&off("Walls")));
        assert!(app.visible[WALLS], "the 3D view under the sheet keeps its layers");
    }

    #[test]
    fn the_layers_are_in_the_right_pane_for_the_view_shown() {
        let (mut app, _) = app_with_plan();
        assert!(!shows(&app, "Furniture"), "no longer in the explorer");
        assert!(shows(&app, "VIEW"), "the properties name the view shown");
        assert!(shows(&app, "Floor plan"));
        let _ = app.update(Message::SideTab(SideTab::Layers));
        assert!(shows(&app, "Furniture"));
        assert!(shows(&app, "These layers apply to this view only"));
        let _ = app.update(Message::LayersQuery("furn".into()));
        assert!(!shows(&app, "Walls"));
        let _ = app.update(Message::AllLayers(false));
        assert!(app.visible[WALLS], "Hide all hides what the search finds");
        assert!(!app.visible[FURNITURE]);
        // on a sheet nothing is picked yet
        let _ = app.update(Message::NewSheet("Sheet"));
        assert!(shows(&app, "Pick a view on the sheet to switch its layers"));
    }
}
