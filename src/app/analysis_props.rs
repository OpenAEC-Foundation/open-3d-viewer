//! The two sections a structural analysis model adds to the right pane's Properties tab.
//!
//! - **Visibility**: what of the model itself shows: its nodes, its members' lines, the solids of
//!   its profiles, its supports, the loads of a load case, and the figures beside the lines.
//! - **Results**: what was computed: of which load case or combination, which internal force,
//!   the deformed shape (as lines or with the profiles), the reactions, the scale as a number to
//!   type, and how the lines of the forces are drawn.
//!
//! The ribbon's switches step through the same choices; here each is chosen at once. All of it
//! belongs to the model: switched off in the models list, nothing of it is drawn.

use super::{App, Message, Palette};
use crate::structural::diagrams::{Change, DeformedAs, Force, ReactionsAs, Style, Values};
use bim_structural::{AnalysisModel, ResultFor};
use iced::widget::text::Wrapping;
use iced::widget::{checkbox, column, container, pick_list, row, text, text_input};
use iced::{Center, Element, Length};
use openaec_ui::Language;

/// A choice of a list: what picking it changes, and its wording.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice(Change, String);

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.1)
    }
}

/// A result set as the list names it: what it is of, then its name.
fn set_name(doc: &AnalysisModel, at: usize, l: Language) -> String {
    let set = &doc.results[at];
    let kind = match &set.for_ {
        ResultFor::Case(_) => l.text("Geval", "Case"),
        ResultFor::Combination(_) => l.text("Combinatie", "Combination"),
        ResultFor::Envelope(_) => l.text("Omhullende", "Envelope"),
    };
    format!("{kind}: {}", set.name)
}

fn force_name(force: Force, l: Language) -> &'static str {
    match force {
        Force::N => l.text("Normaalkracht N", "Axial force N"),
        Force::Vy => l.text("Dwarskracht Vy", "Shear force Vy"),
        Force::Vz => l.text("Dwarskracht Vz", "Shear force Vz"),
        Force::Mx => l.text("Wringing Mx", "Torsion Mx"),
        Force::My => l.text("Moment My", "Moment My"),
        Force::Mz => l.text("Moment Mz", "Moment Mz"),
    }
}

impl App {
    /// The text in the scale field: what is being typed, else the scale in use.
    fn analysis_scale_text(&self) -> String {
        self.analysis_scale.clone().unwrap_or_else(|| {
            let scale = if self.analysis.scale > 0.0 { self.analysis.scale } else { 1.0 };
            format!("{scale:.2}").trim_end_matches('0').trim_end_matches('.').to_string()
        })
    }

    /// Takes what is typed in the scale field; a number (with a point or a comma) is drawn at once.
    pub(super) fn type_analysis_scale(&mut self, typed: String) -> Option<Change> {
        let scale = typed.trim().replace(',', ".").parse::<f32>().ok().filter(|s| s.is_finite() && *s > 0.0);
        self.analysis_scale = Some(typed);
        scale.map(Change::ScaleTo)
    }

    /// The Visibility and the Results section, for a model with a structural analysis model.
    pub(super) fn analysis_properties(&self, p: Palette, l: Language) -> Option<(Element<'_, Message>, Element<'_, Message>)> {
        let model = self.model.as_ref()?;
        let doc = &model.structural.first()?.doc;
        let shown = &self.analysis;
        let label = |k: &str| text(k.to_string()).size(12).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph).width(Length::FillPortion(2));
        let pick = |k: &str, choices: Vec<Choice>, picked: Option<Choice>| -> Element<'_, Message> {
            row![label(k), pick_list(choices, picked, |c: Choice| Message::Analysis(c.0)).text_size(12).padding([2, 6]).width(Length::FillPortion(3))].spacing(8).align_y(Center).into()
        };
        let tick = |k: &str, on: bool, to: fn(bool) -> Message| -> Element<'_, Message> { checkbox(on).label(k.to_string()).on_toggle(to).size(14).text_size(12).into() };

        // what of the model shows
        let mut visibility = column![
            tick(l.text("Knopen", "Nodes"), !shown.hide_nodes, |on| Message::Analysis(Change::NodesOn(on))),
            tick(l.text("Staven", "Members"), !shown.hide_members, |on| Message::Analysis(Change::MembersOn(on))),
        ]
        .spacing(5);
        if !model.layers.is_empty() {
            visibility = visibility.push(tick(l.text("Profielen", "Profiles"), self.visible.iter().any(|v| *v), Message::AllLayers));
        }
        visibility = visibility
            .push(tick(l.text("Opleggingen", "Supports"), shown.supports, |on| Message::Analysis(Change::SupportsOn(on))))
            .push(tick(l.text("Waarden", "Values"), !shown.hide_values, |on| Message::Analysis(Change::ValuesOn(on))));
        if !doc.load_cases.is_empty() {
            let none = Choice(Change::CaseAt(None), l.text("Geen", "None").to_string());
            let mut cases = vec![none.clone()];
            cases.extend(doc.load_cases.iter().enumerate().map(|(k, c)| Choice(Change::CaseAt(Some(k)), c.name.clone())));
            let picked = shown.loads.and_then(|k| cases.get(k + 1).cloned()).unwrap_or(none);
            visibility = visibility.push(pick(l.text("Belastingen van", "Loads of"), cases, Some(picked)));
        }

        // what was computed
        let mut results = column![].spacing(5);
        if !doc.results.is_empty() {
            let sets: Vec<Choice> = (0..doc.results.len()).map(|k| Choice(Change::SetAt(k), set_name(doc, k, l))).collect();
            let picked = sets.get(shown.set.min(sets.len() - 1)).cloned();
            results = results.push(pick(l.text("Resultaten van", "Results of"), sets, picked));
        }
        let no_force = Choice(Change::ForceIs(None), l.text("Geen", "None").to_string());
        let mut forces = vec![no_force.clone()];
        forces.extend(Force::ALL.into_iter().map(|f| Choice(Change::ForceIs(Some(f)), force_name(f, l).to_string())));
        let picked = forces.iter().find(|c| c.0 == Change::ForceIs(shown.force)).cloned().unwrap_or(no_force);
        results = results
            .push(pick(l.text("Snedekracht", "Internal force"), forces, Some(picked)))
            .push(tick(l.text("Vervorming", "Deformed shape"), shown.deformed, |on| Message::Analysis(Change::DeformedOn(on))));
        let ways = vec![Choice(Change::DeformedAs(DeformedAs::Profiles), l.text("Profielen in 3D", "Profiles in 3D").to_string()), Choice(Change::DeformedAs(DeformedAs::Lines), l.text("Alleen lijnen", "Lines only").to_string())];
        let picked = ways.iter().find(|c| c.0 == Change::DeformedAs(shown.deformed_as)).cloned();
        results = results
            .push(pick(l.text("Vervormd als", "Deformed as"), ways, picked))
            .push(tick(l.text("Reacties", "Reactions"), shown.reactions, |on| Message::Analysis(Change::ReactionsOn(on))));
        let ways = vec![Choice(Change::ReactionsAs(ReactionsAs::Resultant), l.text("Resultante", "Resultant").to_string()), Choice(Change::ReactionsAs(ReactionsAs::Components), l.text("Per richting X, Y, Z", "Along X, Y, Z").to_string())];
        let picked = ways.iter().find(|c| c.0 == Change::ReactionsAs(shown.reactions_as)).cloned();
        results = results
            .push(pick(l.text("Reacties als", "Reactions as"), ways, picked))
            .push(
                row![
                    label(l.text("Schaal", "Scale")),
                    text_input("1", &self.analysis_scale_text()).on_input(Message::AnalysisScale).on_submit(Message::AnalysisScaleDone).size(12).padding([2, 6]).width(Length::FillPortion(3)),
                ]
                .spacing(8)
                .align_y(Center),
            );
        let styles = vec![Choice(Change::Style(Style::Strokes), l.text("Lijn met strepen", "Line with strokes").to_string()), Choice(Change::Style(Style::Outline), l.text("Alleen de lijn", "Line only").to_string())];
        let picked = styles.iter().find(|c| c.0 == Change::Style(shown.style)).cloned();
        results = results.push(pick(l.text("Krachtlijnen", "Force lines"), styles, picked));
        let values = vec![Choice(Change::Values(Values::Model), l.text("Grootste van het model", "Largest of the model").to_string()), Choice(Change::Values(Values::Members), l.text("Grootste per staaf", "Largest per member").to_string())];
        let picked = values.iter().find(|c| c.0 == Change::Values(shown.values)).cloned();
        results = results.push(pick(l.text("Waarden bij", "Values at"), values, picked));
        // the plates: in the colours of a value over their mesh, with what the colours stand for
        if let Some(mesh) = doc.results.get(shown.set).and_then(|r| r.inline.as_ref()).and_then(|r| r.surface_mesh.as_ref()) {
            results = results.push(tick(l.text("Platen in kleur", "Plates in colour"), shown.plates, |on| Message::Analysis(Change::PlatesOn(on))));
            let values: Vec<Choice> = crate::structural::diagrams::plate_values(mesh)
                .into_iter()
                .enumerate()
                .map(|(k, name)| Choice(Change::PlateValue(k as u8), if k == 0 { l.text("Verplaatsing u", "Displacement u").to_string() } else { name }))
                .collect();
            let picked = values.get(usize::from(shown.plate_value)).or(values.first()).cloned();
            results = results.push(pick(l.text("Plaatwaarde", "Plate value"), values, picked)).push(tick(l.text("Net van de platen", "Mesh of the plates"), shown.plate_mesh, |on| Message::Analysis(Change::PlateMeshOn(on))));
            if shown.plates {
                let value = crate::structural::diagrams::plate_value(mesh, shown.plate_value);
                let (low, high) = value.range();
                const STEPS: usize = 7;
                let mut legend = column![].spacing(0);
                for k in (0..STEPS).rev() {
                    let t = k as f64 / (STEPS - 1) as f64;
                    let c = crate::structural::diagrams::scale_colour(t);
                    let swatch = container(text("")).width(22).height(16).style(move |_| container::Style { background: Some(iced::Color::from_rgb8(c[0], c[1], c[2]).into()), ..container::Style::default() });
                    legend = legend.push(row![swatch, text(value.written(low + (high - low) * t)).size(12).color(p.text_secondary)].spacing(8).align_y(Center));
                }
                results = results.push(legend);
            }
        }
        Some((visibility.into(), results.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn portal() -> App {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/bim-structural/tests/fixtures/portal_frame.samjson");
        let mut app = App::default();
        app.set_model(std::sync::Arc::new(crate::loaders::load(&path).expect("the portal frame")));
        app
    }

    #[test]
    fn the_right_pane_has_a_visibility_and_a_results_section() {
        let mut app = portal();
        {
            let mut ui = crate::app::test_ui(&app, iced::Size::new(1400.0, 900.0));
            for wording in ["VISIBILITY", "Nodes", "Members", "Supports", "Loads of", "RESULTS", "Results of", "Internal force", "Deformed as", "Reactions as", "Scale", "Force lines", "Values at"] {
                assert!(ui.find(wording).is_ok(), "{wording}");
            }
        }
        // a choice of a list is taken as it is
        let _ = app.update(Message::Analysis(Change::Style(Style::Outline)));
        let _ = app.update(Message::Analysis(Change::Values(Values::Members)));
        let _ = app.update(Message::Analysis(Change::CaseAt(Some(0))));
        let _ = app.update(Message::Analysis(Change::ForceIs(Some(Force::My))));
        assert_eq!((app.analysis.style, app.analysis.values, app.analysis.loads, app.analysis.force), (Style::Outline, Values::Members, Some(0), Some(Force::My)));
    }

    #[test]
    fn a_plate_is_shown_in_colour_with_its_legend() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/bim-structural/tests/fixtures/plate_floor.samjson");
        let mut app = App::default();
        app.set_model(std::sync::Arc::new(crate::loaders::load(&path).expect("the floor plate")));
        {
            let mut ui = crate::app::test_ui(&app, iced::Size::new(1400.0, 900.0));
            for wording in ["Plates in colour", "Plate value", "Mesh of the plates"] {
                assert!(ui.find(wording).is_ok(), "{wording}");
            }
            assert!(ui.find("12.5 kNm/m").is_err(), "no legend until the plates show");
        }
        // the bending moment across the short span: 12.5 kNm/m at most, the top of the legend
        let _ = app.update(Message::Analysis(Change::PlatesOn(true)));
        let _ = app.update(Message::Analysis(Change::PlateValue(2)));
        assert!(!app.overlay_faces.is_empty());
        let mut ui = crate::app::test_ui(&app, iced::Size::new(1400.0, 900.0));
        assert!(ui.find("12.5 kNm/m").is_ok());
    }

    #[test]
    fn nodes_and_members_are_switched_off_and_on() {
        let mut app = portal();
        assert!(app.structural.shown_marks.is_none(), "all of it shows");
        let _ = app.update(Message::Analysis(Change::NodesOn(false)));
        let without_nodes = app.structural.shown_marks.clone().expect("some left out");
        assert!(without_nodes.glyphs.is_empty() && !without_nodes.segs.is_empty());
        let _ = app.update(Message::Analysis(Change::MembersOn(false)));
        assert!(app.structural.shown_marks.as_deref().is_some_and(|m| m.is_empty()));
        let _ = app.update(Message::Analysis(Change::NodesOn(true)));
        let _ = app.update(Message::Analysis(Change::MembersOn(true)));
        assert!(app.structural.shown_marks.is_none());
    }

    #[test]
    fn a_model_switched_off_shows_none_of_its_results() {
        let mut app = portal();
        let _ = app.update(Message::Analysis(Change::DeformedOn(true)));
        let _ = app.update(Message::Analysis(Change::ReactionsOn(true)));
        assert!(!app.overlay_lines.is_empty() && !app.overlay_faces.is_empty());
        let _ = app.update(Message::ModelVisible(0, false));
        assert!(app.overlay_lines.is_empty() && app.overlay_faces.is_empty(), "nothing of the model that is off");
        let _ = app.update(Message::ModelVisible(0, true));
        assert!(!app.overlay_lines.is_empty() && !app.overlay_faces.is_empty());
    }

    #[test]
    fn a_typed_scale_is_drawn_at_once_and_a_half_typed_one_is_kept_as_text() {
        let mut app = portal();
        let _ = app.update(Message::AnalysisScale("2,5".into()));
        assert_eq!((app.analysis.scale, app.analysis_scale_text().as_str()), (2.5, "2,5"));
        let _ = app.update(Message::AnalysisScale("abc".into()));
        assert_eq!((app.analysis.scale, app.analysis_scale_text().as_str()), (2.5, "abc"));
        // Enter shows the scale in use again
        let _ = app.update(Message::AnalysisScaleDone);
        assert_eq!(app.analysis_scale_text(), "2.5");
        // the ribbon's Larger clears what was typed
        let _ = app.update(Message::Analysis(Change::Scale(true)));
        assert_eq!(app.analysis_scale_text(), "3.75");
    }
}
