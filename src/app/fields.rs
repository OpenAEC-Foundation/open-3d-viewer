//! Which field of a simulation file colours its mesh: the Field section of the right pane (the
//! field, the time step and the legend of the colours) and the ribbon's switches that step
//! through the same choices. The colours are worked out of the values the loader kept
//! ([`crate::fields::Fields`]), into the layers of the open model that has them.

use super::{App, Message, ModelEntry, Palette};
use crate::fields::{number, Fields};
use crate::model::Model;
use iced::widget::text::Wrapping;
use iced::widget::{column, container, pick_list, row, text, Space};
use iced::{Center, Element, Length};
use openaec_ui::Language;
use std::sync::Arc;

/// The field shown and the time step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FieldView {
    /// Index into [`Fields::channels`]; `None`: the mesh in its own colours.
    pub channel: Option<usize>,
    pub step: usize,
}

impl FieldView {
    /// As a model opens: coloured by its first field, at its first step.
    pub fn of(model: &Model) -> Self {
        Self { channel: model.project.fields.as_ref().filter(|f| !f.channels.is_empty()).map(|_| 0), step: 0 }
    }
}

/// A change of the field shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldChange {
    Channel(Option<usize>),
    Step(usize),
    /// The next (true) or previous field, past "none".
    NextChannel(bool),
    /// The next or previous time step, round.
    NextStep(bool),
}

/// A choice of a list: what picking it changes, and its wording.
#[derive(Debug, Clone, PartialEq)]
struct Choice(FieldChange, String);

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.1)
    }
}

/// How many swatches the legend's bar has.
const SWATCHES: usize = 24;

impl App {
    /// The fields of the open models: of the first that has them.
    pub(crate) fn shown_fields(&self) -> Option<Arc<Fields>> {
        match self.models.is_empty() {
            true => self.model.as_ref()?.project.fields.clone(),
            false => self.models.iter().find_map(|e| e.model.project.fields.clone()),
        }
    }

    pub(super) fn change_field(&mut self, change: FieldChange) {
        let Some(fields) = self.shown_fields() else { return };
        let (channels, steps) = (fields.channels.len(), fields.steps.len().max(1));
        let mut view = self.field;
        match change {
            FieldChange::Channel(c) => view.channel = c.filter(|c| *c < channels),
            FieldChange::Step(s) => view.step = s.min(steps - 1),
            FieldChange::NextChannel(forward) => {
                // none, then each channel
                let at = view.channel.map_or(0, |c| c + 1);
                let next = if forward { (at + 1) % (channels + 1) } else { (at + channels) % (channels + 1) };
                view.channel = next.checked_sub(1);
            }
            FieldChange::NextStep(forward) => view.step = if forward { (view.step + 1) % steps } else { (view.step + steps - 1) % steps },
        }
        if view != self.field {
            self.field = view;
            self.paint_fields();
        }
    }

    /// Colours the layers of the open models with fields as [`App::field`] says, keeping the
    /// camera and what is shown.
    fn paint_fields(&mut self) {
        if self.models.is_empty() {
            let Some(model) = self.model.clone() else { return };
            self.models.push(ModelEntry::new(model, 0));
        }
        let view = self.field;
        let mut changed = false;
        for entry in &mut self.models {
            let Some(fields) = entry.model.project.fields.clone() else { continue };
            let mut model = (*entry.model).clone();
            for source in &fields.layers {
                if let Some(layer) = model.layers.get_mut(source.layer) {
                    layer.colors = view.channel.map(|c| fields.colours(source, c, view.step));
                }
            }
            model.id = crate::model::fresh_id();
            entry.model = Arc::new(model);
            changed = true;
        }
        if !changed {
            return;
        }
        if self.models.len() > 1 {
            let camera = self.camera;
            self.rebuild_scene();
            self.camera = camera;
        } else {
            self.model = self.models.first().map(|e| e.model.clone());
        }
    }

    /// The Field section: the field, the time step and the legend.
    pub(super) fn field_properties(&self, p: Palette, l: Language) -> Option<Element<'_, Message>> {
        let fields = self.shown_fields()?;
        let label = |k: &str| text(k.to_string()).size(12).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph).width(Length::FillPortion(2));
        let pick = |k: &str, choices: Vec<Choice>, picked: Option<Choice>| -> Element<'_, Message> {
            row![label(k), pick_list(choices, picked, |c: Choice| Message::Field(c.0)).text_size(12).padding([2, 6]).width(Length::FillPortion(3))].spacing(8).align_y(Center).into()
        };
        let none = Choice(FieldChange::Channel(None), l.text("Eigen kleur", "Own colour").to_string());
        let mut channels = vec![none.clone()];
        channels.extend(fields.channels.iter().enumerate().map(|(k, c)| Choice(FieldChange::Channel(Some(k)), c.label.clone())));
        let picked = self.field.channel.and_then(|c| channels.get(c + 1).cloned()).unwrap_or(none);
        let mut section = column![pick(l.text("Kleur naar", "Colour by"), channels, Some(picked))].spacing(5);
        if fields.steps.len() > 1 {
            let steps: Vec<Choice> = (0..fields.steps.len()).map(|k| Choice(FieldChange::Step(k), fields.step_name(k))).collect();
            let picked = steps.get(self.field.step).cloned();
            section = section.push(pick(l.text("Tijdstap", "Time step"), steps, picked));
        }
        if let Some(channel) = self.field.channel.and_then(|c| fields.channels.get(c)) {
            let mut bar = row![].spacing(0);
            for k in 0..SWATCHES {
                let [r, g, b] = crate::cloud_color::ramp(k as f32 / (SWATCHES - 1) as f32);
                let fill = iced::Color::from_rgb8(r, g, b);
                bar = bar.push(container(Space::new().width(Length::Fill).height(12)).width(Length::FillPortion(1)).style(move |_| container::Style { background: Some(iced::Background::Color(fill)), ..Default::default() }));
            }
            let (low, high) = channel.range;
            let middle = (low as f64 + high as f64) / 2.0;
            let value = |x: f64| text(number(x)).size(11).color(p.text_secondary);
            let where_ = match channel.at {
                crate::fields::At::Point => l.text("in de punten", "at the points"),
                crate::fields::At::Cell => l.text("per cel", "per cell"),
            };
            section = section
                .push(text(format!("{} ({where_})", channel.label)).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph))
                .push(bar)
                .push(row![value(low as f64), Space::new().width(Length::Fill), value(middle), Space::new().width(Length::Fill), value(high as f64)].align_y(Center));
        }
        Some(section.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loaders::{load_with, Progress, Source};

    const PLATE: &str = "# vtk DataFile Version 3.0
a plate of two quads
ASCII
DATASET POLYDATA
POINTS 6 float
0 0 0 1 0 0 2 0 0 0 1 0 1 1 0 2 1 0
POLYGONS 2 10
4 0 1 4 3
4 1 2 5 4
POINT_DATA 6
SCALARS temperature float 1
LOOKUP_TABLE default
0 5 10 0 5 10
CELL_DATA 2
SCALARS pressure float 1
LOOKUP_TABLE default
1 2
";

    fn plate() -> App {
        let source = Source::Bytes { name: "plate.vtk".into(), data: Arc::new(PLATE.as_bytes().to_vec()) };
        let mut app = App::default();
        app.set_model(Arc::new(load_with(&source, &Progress::default()).expect("the plate")));
        app
    }

    #[test]
    fn a_simulation_file_opens_coloured_and_shows_its_legend() {
        let app = plate();
        assert_eq!(app.field, FieldView { channel: Some(0), step: 0 });
        let mut ui = crate::app::test_ui(&app, iced::Size::new(1400.0, 900.0));
        for wording in ["FIELD", "Colour by", "temperature (at the points)", "10"] {
            assert!(ui.find(wording).is_ok(), "{wording}");
        }
    }

    #[test]
    fn another_field_recolours_the_mesh_and_none_gives_its_own_colour() {
        let mut app = plate();
        let before = app.model.as_ref().unwrap().layers[0].colors.clone().expect("coloured");
        let _ = app.update(Message::Field(FieldChange::NextChannel(true)));
        assert_eq!(app.field.channel, Some(1));
        let model = app.model.clone().unwrap();
        let after = model.layers[0].colors.clone().expect("coloured");
        assert_ne!(before, after);
        // per cell: the two vertices of a cell alike
        assert_eq!(after[0], after[1]);
        let _ = app.update(Message::Field(FieldChange::Channel(None)));
        assert!(app.model.as_ref().unwrap().layers[0].colors.is_none());
        // round past none
        let _ = app.update(Message::Field(FieldChange::NextChannel(false)));
        assert_eq!(app.field.channel, Some(1));
    }
}
