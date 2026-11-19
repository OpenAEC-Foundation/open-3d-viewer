//! NL-SfB codes in the Library product: the codes of the project's library, and the code of a
//! type, picked from them.
//!
//! A library names each code once (`bim_library::nlsfb`): the groups of the table to start with,
//! and the deeper codes an office adds. The page shows them as tiles, each with how many types
//! have it; the right pane edits the picked one of the project: the code itself, its name in
//! the library's four languages, and the types that have it. A code that is changed takes its
//! types with it; one that types have is not removed.
//!
//! A type has the code of its group by rule; its editor shows that code and lets another of the
//! library's codes stand in its place.

use super::libraries::{Category, Editor};
use super::{App, Message, FONT_BOLD};
use crate::theme::Palette;
use bim_library::nlsfb::{self, Code};
use bim_library::{Lang, Library, LibraryError};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, pick_list, row, text, text_input, Column, Space};
use iced::{Center, Element, Fill, Length, Task};
use openaec_ui::design::a11y::{self, Role as Aria};
use openaec_ui::Language;

#[derive(Debug, Clone)]
pub enum CodeMessage {
    /// Edit this code of the project.
    Pick(String),
    /// A code more: under the picked one, else under the external walls.
    Add,
    Remove,
    /// Typed as the code itself.
    Typed(String),
    /// Enter: the typed code is taken, and its types with it.
    Done,
    /// Typed as its name in a language.
    Named(Lang, String),
}

/// Which code is edited, what is being typed as the code, and why the last change was refused.
#[derive(Debug, Clone, Default)]
pub struct CodeView {
    pub picked: Option<String>,
    pub draft: Option<String>,
    pub refused: Option<String>,
    /// How many types have the code a removal was asked of.
    pub in_use: Option<usize>,
}

/// A code more under `under`: the first free one a digit deeper ("21" gives "21.1", "21.1"
/// gives "21.11"), beside it when it is four digits already; named as the one it stands under.
pub fn add(library: &mut Library, under: &str) -> Result<String, LibraryError> {
    let (group, rest) = under.split_once('.').map_or((under, ""), |(group, rest)| (group, rest));
    let stem = match rest.len() {
        0 => format!("{group}."),
        1 => format!("{group}.{rest}"),
        _ => format!("{group}.{}", &rest[..1]),
    };
    let fresh = (1..=9).map(|digit| format!("{stem}{digit}")).find(|code| library.code(code).is_none()).ok_or_else(|| LibraryError::Duplicate(format!("{stem}9")))?;
    let names = library.code(under).map(|c| c.names.clone()).or_else(|| nlsfb::starter().into_iter().find(|c| c.code == group).map(|c| c.names)).ok_or_else(|| LibraryError::BadCode(under.to_string()))?;
    library.add_code(Code { code: fresh.clone(), names })?;
    Ok(fresh)
}

/// Code `from` becomes `to`, in its place among the codes; the types that had `from` as their
/// own have `to`. Refused when `to` is no code or the library has it.
pub fn recode(library: &mut Library, from: &str, to: &str) -> Result<(), LibraryError> {
    let to = to.trim();
    if to == from {
        return Ok(());
    }
    let at = library.codes.iter().position(|c| c.code == from).ok_or(LibraryError::NoKey)?;
    let mut code = library.codes[at].clone();
    code.code = to.to_string();
    let mut next = library.clone();
    next.codes.remove(at);
    next.add_code(code)?;
    let own = |code: &mut Option<String>| {
        if code.as_deref() == Some(from) {
            *code = Some(to.to_string());
        }
    };
    next.buildups.iter_mut().for_each(|b| own(&mut b.code));
    next.linear_types.iter_mut().for_each(|t| own(&mut t.code));
    next.system_types.iter_mut().for_each(|t| own(&mut t.code));
    *library = next;
    Ok(())
}

/// Removes code `code` when no type has it; else how many types have it.
pub fn remove(library: &mut Library, code: &str) -> Result<(), usize> {
    let types = library.coded(code, false).len();
    if types > 0 {
        return Err(types);
    }
    library.codes.retain(|c| c.code != code);
    Ok(())
}

/// The names of the types that have `code`, as the library lists them.
pub fn type_names(library: &Library, code: &str, lang: Lang) -> Vec<String> {
    library.coded(code, false).into_iter().filter_map(|id| library.name(id, lang).or_else(|| library.linear_name(id, lang)).or_else(|| library.system_name(id, lang))).collect()
}

fn name_mut(code: &mut Code, lang: Lang) -> &mut String {
    match lang {
        Lang::Nl => &mut code.names.nl,
        Lang::En => &mut code.names.en,
        Lang::De => &mut code.names.de,
        Lang::Fr => &mut code.names.fr,
    }
}

/// A field of the pane: its label, and beside it what sets it.
fn field<'a>(p: Palette, label: String, control: Element<'a, CodeMessage>) -> Element<'a, CodeMessage> {
    row![text(label).size(12).color(p.text_secondary).width(Length::FillPortion(2)), container(control).width(Length::FillPortion(3))].spacing(8).align_y(Center).into()
}

impl App {
    pub(super) fn code_update(&mut self, message: CodeMessage) -> Task<Message> {
        let picked = self.code_view.picked.clone();
        match message {
            CodeMessage::Pick(code) => {
                self.code_view = CodeView { picked: Some(code), ..CodeView::default() };
                self.show_in_project(Category::Codes);
            }
            CodeMessage::Add => match add(&mut self.library, picked.as_deref().unwrap_or("21")) {
                Ok(code) => self.code_view = CodeView { picked: Some(code), ..CodeView::default() },
                Err(why) => self.code_view.refused = Some(why.to_string()),
            },
            CodeMessage::Remove => {
                if let Some(code) = &picked {
                    match remove(&mut self.library, code) {
                        Ok(()) => self.code_view = CodeView::default(),
                        Err(types) => self.code_view.in_use = Some(types),
                    }
                }
            }
            CodeMessage::Typed(typed) => self.code_view.draft = Some(typed),
            CodeMessage::Done => {
                if let (Some(code), Some(typed)) = (&picked, self.code_view.draft.clone()) {
                    match recode(&mut self.library, code, &typed) {
                        Ok(()) => self.code_view = CodeView { picked: Some(typed.trim().to_string()), ..CodeView::default() },
                        Err(why) => self.code_view.refused = Some(why.to_string()),
                    }
                }
            }
            CodeMessage::Named(lang, name) => {
                if let Some(code) = picked.as_deref().and_then(|code| self.library.codes.iter_mut().find(|c| c.code == code)) {
                    *name_mut(code, lang) = name;
                }
            }
        }
        if self.code_view.picked.is_some() {
            self.edit_only(Editor::Code);
        }
        Task::none()
    }

    /// The right pane of the Project page: the editor of the picked code.
    pub fn code_pane(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let lang = super::library::lang(l);
        let view = &self.code_view;
        let library = &self.library;
        let Some(code) = view.picked.as_deref().and_then(|code| library.code(code)) else {
            let hint = l.text("Kies een code op de pagina om haar te bewerken.", "Pick a code on the page to edit it.");
            return container(text(hint).size(12).color(p.text_muted).wrapping(Wrapping::Word)).padding(8).into();
        };
        let typed = view.draft.clone().unwrap_or_else(|| code.code.clone());
        let code_name = l.text("Code", "Code");
        let mut fields: Column<'_, CodeMessage> = Column::new().spacing(4);
        fields = fields.push(field(p, code_name.to_string(), a11y::named(text_input("21.22", &typed).on_input(CodeMessage::Typed).on_submit(CodeMessage::Done).size(12).padding([2, 6]), Aria::TextField, code_name)));
        for of in Lang::ALL {
            let name = super::materials::lang_name(of);
            fields = fields.push(field(p, name.to_string(), a11y::named(text_input("", code.names.get(of)).on_input(move |typed| CodeMessage::Named(of, typed)).size(12).padding([2, 6]), Aria::TextField, name)));
        }
        let types = type_names(library, &code.code, lang);
        let mut have: Column<'_, CodeMessage> = column![text(format!("{}: {}", l.text("Typen met deze code", "Types with this code"), types.len())).size(11).color(p.text_secondary)].spacing(2);
        const SHOWN: usize = 12;
        for name in types.iter().take(SHOWN) {
            have = have.push(text(name.clone()).size(11).color(p.text_muted).wrapping(Wrapping::WordOrGlyph));
        }
        if types.len() > SHOWN {
            have = have.push(text(format!("+ {}", types.len() - SHOWN)).size(11).color(p.text_muted));
        }
        let act = |label: &'static str, message: CodeMessage| button(text(label).size(12).color(p.text_secondary)).padding([3, 8]).style(move |_, s| p.row_button(s)).on_press(message);
        let refused = view.refused.as_ref().map(|why| text(format!("{} ({why})", l.text("Niet overgenomen.", "Not taken."))).size(11).color(p.accent).wrapping(Wrapping::Word));
        let in_use = view.in_use.map(|types| text(format!("{}: {types}", l.text("Niet verwijderd: typen met deze code", "Not removed: types with this code"))).size(11).color(p.accent).wrapping(Wrapping::Word));
        let group = nlsfb::group_name(&code.code, lang).unwrap_or_default();
        let pane: Column<'_, CodeMessage> = column![
            text(named(library, &code.code, lang)).size(14).font(FONT_BOLD).color(p.text).wrapping(Wrapping::Word),
            text(format!("NL-SfB · {} {group}", code.code.get(..2).unwrap_or_default())).size(11).color(p.text_muted),
            fields,
            have,
            row![act(l.text("+ Code hieronder", "+ Code under it"), CodeMessage::Add), act(l.text("Verwijderen", "Remove"), CodeMessage::Remove), Space::new().width(Fill)].spacing(4),
        ]
        .push(refused)
        .push(in_use)
        .spacing(8);
        container(Element::from(pane).map(Message::Code)).padding(8).into()
    }
}

/// A code to pick for a type: the rule's (`code` none), or one of the library's.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct CodeChoice {
    pub code: Option<String>,
    label: String,
}

impl std::fmt::Display for CodeChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// A code with what it is called: "21 · buitenwanden".
pub(super) fn named(library: &Library, code: &str, lang: Lang) -> String {
    match library.code_name(code, lang) {
        Some(name) => format!("{code} · {name}"),
        None => code.to_string(),
    }
}

/// The codes a type can have: the rule's first, then every code of the library, and the type's
/// own when the library does not have it (a type of a file).
pub(super) fn choices(l: Language, library: &Library, rule: &str, own: Option<&str>) -> Vec<CodeChoice> {
    let lang = super::library::lang(l);
    let by_rule = CodeChoice { code: None, label: format!("{} ({})", named(library, rule, lang), l.text("volgens regel", "by rule")) };
    let of_library = library.codes.iter().map(|c| CodeChoice { code: Some(c.code.clone()), label: named(library, &c.code, lang) });
    let stray = own.filter(|own| library.code(own).is_none()).map(|own| CodeChoice { code: Some(own.to_string()), label: named(library, own, lang) });
    std::iter::once(by_rule).chain(of_library).chain(stray).collect()
}

/// The line of a type's editor that picks its code.
pub(super) fn picker<'a, M: Clone + 'a>(p: Palette, l: Language, library: &Library, rule: &str, own: Option<&str>, on: impl Fn(Option<String>) -> M + 'a) -> Element<'a, M> {
    let choices = choices(l, library, rule, own);
    let now = choices.iter().find(|c| c.code.as_deref() == own).cloned();
    let pick = pick_list(choices, now, move |c: CodeChoice| on(c.code)).text_size(12).padding([2, 6]).width(Length::FillPortion(3));
    row![text("NL-SfB").size(12).color(p.text_secondary).width(Length::FillPortion(2)), a11y::named(pick, Aria::ComboBox, "NL-SfB")].spacing(8).align_y(Center).width(Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bim_library::Names;

    #[test]
    fn a_type_picks_the_rules_code_or_one_of_the_library() {
        let mut library = Library::starter();
        let l = Language::English;
        let offered = choices(l, &library, "21", None);
        assert_eq!((offered[0].code.clone(), offered[0].to_string()), (None, "21 · external walls (by rule)".to_string()));
        assert_eq!(offered.len(), 1 + library.codes.len());
        assert!(offered.iter().any(|c| c.code.as_deref() == Some("22") && c.to_string() == "22 · internal walls"));
        // a code the office added is offered under its name
        library.add_code(Code { code: "22.11".into(), names: Names::new("massieve binnenwanden", "solid internal walls", "x", "y") }).unwrap();
        assert!(choices(Language::Dutch, &library, "21", None).iter().any(|c| c.to_string() == "22.11 · massieve binnenwanden"));
        // a type of a file keeps a code the library lacks, named by its group
        let stray = choices(l, &library, "21", Some("27.3"));
        assert_eq!(stray.last().map(|c| (c.code.as_deref(), c.to_string())), Some((Some("27.3"), "27.3 · roofs".to_string())));
        assert_eq!(choices(l, &library, "21", Some("22")).len(), 2 + library.codes.len() - 1);
    }

    #[test]
    fn a_code_is_added_changed_and_removed_with_its_types() {
        let mut app = App::default();
        let codes = app.library.codes.len();
        // a code more under the internal walls, named as they are, picked to be edited
        let _ = app.code_update(CodeMessage::Pick("22".into()));
        let _ = app.code_update(CodeMessage::Add);
        assert_eq!((app.library.codes.len(), app.code_view.picked.as_deref()), (codes + 1, Some("22.1")));
        assert_eq!(app.library.code_name("22.1", Lang::Nl).as_deref(), Some("binnenwanden"));
        let _ = app.code_update(CodeMessage::Named(Lang::Nl, "binnenwanden, niet constructief".into()));
        assert_eq!(app.library.code_name("22.1", Lang::Nl).as_deref(), Some("binnenwanden, niet constructief"));
        // two partitions take it: one code, many types
        for id in ["wall.metal-stud-75", "wall.sand-lime-100"] {
            let _ = app.buildup_update(super::super::buildups::BuildUpMessage::Pick(id.into()));
            let _ = app.buildup_update(super::super::buildups::BuildUpMessage::Coded(Some("22.1".into())));
        }
        assert_eq!(app.library.coded("22.1", false), ["wall.sand-lime-100", "wall.metal-stud-75"].iter().rev().copied().collect::<Vec<_>>());
        assert_eq!(type_names(&app.library, "22.1", Lang::En).len(), 2);
        // the code typed anew takes its types with it; what is no code is refused
        let _ = app.code_update(CodeMessage::Pick("22.1".into()));
        let _ = app.code_update(CodeMessage::Typed("wand".into()));
        let _ = app.code_update(CodeMessage::Done);
        assert!(app.code_view.refused.is_some() && app.library.code("22.1").is_some());
        let _ = app.code_update(CodeMessage::Typed("22.11".into()));
        let _ = app.code_update(CodeMessage::Done);
        assert_eq!((app.code_view.picked.as_deref(), app.library.code("22.1").is_some(), app.library.coded("22.11", false).len()), (Some("22.11"), false, 2));
        assert_eq!(app.library.buildup("wall.metal-stud-75").unwrap().code.as_deref(), Some("22.11"));
        // a code types have is not removed; once they have the rule's again, it is
        let _ = app.code_update(CodeMessage::Remove);
        assert!(app.library.code("22.11").is_some() && app.code_view.in_use == Some(2));
        for id in ["wall.metal-stud-75", "wall.sand-lime-100"] {
            let _ = app.buildup_update(super::super::buildups::BuildUpMessage::Pick(id.into()));
            let _ = app.buildup_update(super::super::buildups::BuildUpMessage::Coded(None));
        }
        let _ = app.code_update(CodeMessage::Pick("22.11".into()));
        let _ = app.code_update(CodeMessage::Remove);
        assert_eq!((app.library.codes.len(), app.code_view.picked.clone()), (codes, None));
        // a group every wall has by rule stays
        let _ = app.code_update(CodeMessage::Pick("21".into()));
        let _ = app.code_update(CodeMessage::Remove);
        assert!(app.library.code("21").is_some());
    }

    #[test]
    fn codes_are_added_a_digit_deeper() {
        let mut library = Library::starter();
        assert_eq!(add(&mut library, "21").as_deref(), Ok("21.1"));
        assert_eq!(add(&mut library, "21").as_deref(), Ok("21.2"));
        assert_eq!(add(&mut library, "21.2").as_deref(), Ok("21.21"));
        assert_eq!(add(&mut library, "21.21").as_deref(), Ok("21.22"), "four digits: the next beside it");
        assert_eq!(recode(&mut library, "21.22", "21.21"), Err(LibraryError::Duplicate("21.21".into())));
        assert_eq!(recode(&mut library, "21.22", "21.29"), Ok(()));
        let order: Vec<&str> = library.codes.iter().filter(|c| c.code.starts_with("21")).map(|c| c.code.as_str()).collect();
        assert_eq!(order, ["21", "21.1", "21.2", "21.21", "21.29"]);
    }

    #[test]
    fn the_pane_edits_the_picked_code() {
        let mut app = App::default();
        let _ = app.code_update(CodeMessage::Pick("21".into()));
        let mut pane = crate::app::test_ui_of(iced::Size::new(340.0, 700.0), app.code_pane(app.theme.palette(), Language::English));
        let types = app.library.coded("21", false).len();
        assert!(types > 3);
        assert!(pane.find("21 · external walls").is_ok(), "the pane names the code");
        assert!(pane.find(format!("Types with this code: {types}").as_str()).is_ok() && pane.find("+ Code under it").is_ok(), "the pane counts its types");
    }
}
