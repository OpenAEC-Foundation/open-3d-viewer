//! The properties of what is drawn in a legend or drafting view: a detail line, a filled region,
//! a detail component, a text, a dimension or a tag. The loader gathers them per element of the
//! file, in groups ([`Group`]); the right pane shows those of the element picked in the drawing,
//! as it shows a model element's.

use super::{prop_row, App, Message};
use crate::theme::Palette;
use iced::widget::{column, container, text};
use iced::{Element, Padding};
use openaec_ui::Language;
use std::collections::HashMap;
use std::sync::Arc;

/// A label or a value: a word of the viewer in Dutch and English, or one the file writes.
#[derive(Debug, Clone, PartialEq)]
pub enum Word {
    Both(&'static str, &'static str),
    Own(String),
}

impl Word {
    pub fn text(&self, l: Language) -> String {
        match self {
            Word::Both(dutch, english) => l.text(dutch, english).to_string(),
            Word::Own(own) => own.clone(),
        }
    }
}

impl From<String> for Word {
    fn from(own: String) -> Self {
        Word::Own(own)
    }
}

impl From<&str> for Word {
    fn from(own: &str) -> Self {
        Word::Own(own.to_string())
    }
}

/// Properties that belong together, under their title: an element's own, those of its line
/// style, of its type, its parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub title: Word,
    pub rows: Vec<(Word, Word)>,
}

/// The groups of every element drawn in a legend or drafting view, by its element in the file.
/// Elements of one style or type share its group.
pub type Drawn = HashMap<u64, Vec<Arc<Group>>>;

impl App {
    /// Picks an element of the file in the shown legend or drafting view, as a click on it does.
    pub fn pick_drawn(&mut self, element: u64) {
        use super::drafts::DraftPick as K;
        let Some(draft) = self.active_draft.and_then(|i| self.drafts().get(i)) else { return };
        let kind = if draft.texts.iter().any(|t| t.element == element) {
            K::Text
        } else if draft.dimensions.iter().any(|d| d.element == element) {
            K::Dimension
        } else {
            K::Line
        };
        self.draft_pick = Some((element, kind));
    }

    /// The sections of the element picked in the shown legend or drafting view: a title and its
    /// rows each. `None` when nothing is picked there.
    pub(super) fn drawn_properties(&self, p: Palette, l: Language) -> Option<Vec<(String, Element<'static, Message>)>> {
        self.active_draft?;
        let (element, kind) = self.draft_pick?;
        let rows = |rows: &[(Word, Word)]| -> Element<'static, Message> {
            rows.iter().fold(column![].spacing(5), |c, (label, value)| c.push(prop_row(p, label.text(l), value.text(l)))).into()
        };
        let groups = self.model.as_ref().and_then(|m| m.project.drawn.get(&element));
        let Some(groups) = groups.filter(|g| !g.is_empty()) else {
            // nothing read of it: what was clicked, and its element
            use super::drafts::DraftPick as K;
            let word = match kind {
                K::Line => l.text("Lijn", "Line"),
                K::Text => l.text("Tekst", "Text"),
                K::Dimension => l.text("Maat", "Dimension"),
            };
            let body = column![
                prop_row(p, l.text("Soort", "Kind").to_string(), word.to_string()),
                prop_row(p, l.text("Element-id", "Element id").to_string(), element.to_string()),
                container(text(l.text("Van dit element zijn geen eigenschappen gelezen", "No properties are read of this element")).size(12).color(p.text_muted))
                    .padding(Padding { top: 2.0, ..Padding::ZERO }),
            ]
            .spacing(5);
            return Some(vec![(l.text("Selectie", "Selection").to_string(), body.into())]);
        };
        Some(groups.iter().map(|g| (g.title.text(l), rows(&g.rows))).collect())
    }
}
