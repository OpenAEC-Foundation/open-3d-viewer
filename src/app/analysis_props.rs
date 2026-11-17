//! The Analysis section of the right pane's Properties tab, for a structural analysis model:
//! which result set and which load case are drawn, picked from lists of all of them, the scale
//! as a number to type, and how the lines of the internal forces are drawn.
//!
//! The ribbon's switches step through the same choices; here each is chosen at once.

use super::{App, Message, Palette};
use openaec_ui::Language;
use crate::structural::diagrams::{Change, Style, Values};
use bim_structural::{AnalysisModel, ResultFor};
use iced::widget::text::Wrapping;
use iced::widget::{column, pick_list, row, text, text_input};
use iced::{Center, Element, Length};

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

    /// The section, for a model with a structural analysis model; `None` otherwise.
    pub(super) fn analysis_properties(&self, p: Palette, l: Language) -> Option<Element<'_, Message>> {
        let doc = &self.model.as_ref()?.structural.first()?.doc;
        let label = |k: &str| text(k.to_string()).size(12).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph).width(Length::FillPortion(2));
        let pick = |k: &str, choices: Vec<Choice>, picked: Option<Choice>| -> Element<'_, Message> {
            row![label(k), pick_list(choices, picked, |c: Choice| Message::Analysis(c.0)).text_size(12).padding([2, 6]).width(Length::FillPortion(3))].spacing(8).align_y(Center).into()
        };
        let mut rows = column![].spacing(5);
        if !doc.results.is_empty() {
            let sets: Vec<Choice> = (0..doc.results.len()).map(|k| Choice(Change::SetAt(k), set_name(doc, k, l))).collect();
            let picked = sets.get(self.analysis.set.min(sets.len() - 1)).cloned();
            rows = rows.push(pick(l.text("Resultaten van", "Results of"), sets, picked));
        }
        if !doc.load_cases.is_empty() {
            let none = Choice(Change::CaseAt(None), l.text("Geen", "None").to_string());
            let mut cases = vec![none.clone()];
            cases.extend(doc.load_cases.iter().enumerate().map(|(k, c)| Choice(Change::CaseAt(Some(k)), c.name.clone())));
            let picked = self.analysis.loads.and_then(|k| cases.get(k + 1).cloned()).unwrap_or(none);
            rows = rows.push(pick(l.text("Belastingen van", "Loads of"), cases, Some(picked)));
        }
        rows = rows.push(
            row![
                label(l.text("Schaal", "Scale")),
                text_input("1", &self.analysis_scale_text()).on_input(Message::AnalysisScale).on_submit(Message::AnalysisScaleDone).size(12).padding([2, 6]).width(Length::FillPortion(3)),
            ]
            .spacing(8)
            .align_y(Center),
        );
        let styles = vec![Choice(Change::Style(Style::Strokes), l.text("Lijn met strepen", "Line with strokes").to_string()), Choice(Change::Style(Style::Outline), l.text("Alleen de lijn", "Line only").to_string())];
        let picked = styles.iter().find(|c| c.0 == Change::Style(self.analysis.style)).cloned();
        rows = rows.push(pick(l.text("Krachtlijnen", "Force lines"), styles, picked));
        let values = vec![Choice(Change::Values(Values::Model), l.text("Grootste van het model", "Largest of the model").to_string()), Choice(Change::Values(Values::Members), l.text("Grootste per staaf", "Largest per member").to_string())];
        let picked = values.iter().find(|c| c.0 == Change::Values(self.analysis.values)).cloned();
        rows = rows.push(pick(l.text("Waarden", "Values"), values, picked));
        Some(rows.into())
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
    fn the_right_pane_picks_results_loads_and_how_they_are_drawn() {
        let mut app = portal();
        {
            let mut ui = crate::app::test_ui(&app, iced::Size::new(1400.0, 900.0));
            for wording in ["Results of", "Loads of", "Scale", "Force lines", "Values"] {
                assert!(ui.find(wording).is_ok(), "{wording}");
            }
        }
        // a choice of a list is taken as it is
        let _ = app.update(Message::Analysis(Change::Style(Style::Outline)));
        let _ = app.update(Message::Analysis(Change::Values(Values::Members)));
        let _ = app.update(Message::Analysis(Change::CaseAt(Some(0))));
        assert_eq!((app.analysis.style, app.analysis.values, app.analysis.loads), (Style::Outline, Values::Members, Some(0)));
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
