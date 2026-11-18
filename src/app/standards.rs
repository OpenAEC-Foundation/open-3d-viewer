//! The drawing standard in the Library product: the kinds of drawing and the settings under
//! them on the page, and the fields of the picked one in the right pane.
//!
//! The standard is the project's own ([`App::standard`]), the Dutch one until it is changed, and
//! goes into the project's IFCX file, one node per setting (`bim_library::drawing`). A change is
//! kept only when the standard still holds together; otherwise the pane says why not.

use super::libraries::{tile, tile_more, Category, TILE};
use super::{App, Message, FONT_BOLD};
use crate::theme::Palette;
use bim_library::drawing::{self, scale_text, Discipline, DrawingKind, LineStyle, Origin, Pen, Standard, TextStyle, View};
use bim_library::{Lang, LibraryError, Names};
use iced::mouse;
use iced::widget::canvas::{self, Frame, LineDash, Path, Stroke};
use iced::widget::text::Wrapping;
use iced::widget::{button, canvas as canvas_widget, checkbox, column, container, pick_list, row, text, text_input, Column, Space};
use iced::{Center, Color, Element, Fill, Length, Point, Rectangle, Task};
use openaec_ui::Language;
use serde_json::Value;

/// A part of the standard that can be picked and set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Setting {
    Kind(String),
    LineStyle(String),
    Pen(String),
    TextStyle(String),
    /// A scale, as 1 to this.
    Scale(u32),
}

/// A kind of setting one more can be added of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    LineStyle,
    Pen,
    TextStyle,
}

/// A field of a picked setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name,
    Code,
    Number,
    Discipline,
    View,
    /// The usual scale.
    Scale,
    Cut,
    Beyond,
    Hidden,
    Text,
    Width,
    Height,
    Dashes,
}

impl Field {
    /// Whether what is typed is taken at once; a number waits for Enter, half typed it is none.
    fn live(self) -> bool {
        matches!(self, Field::Name | Field::Code | Field::Number)
    }
}

#[derive(Debug, Clone)]
pub enum StandardMessage {
    Pick(Setting),
    /// Typed into a field of the picked setting, in the language its name is shown in.
    Typed(Field, String, Lang),
    /// Enter in a field: what was typed is taken.
    Done,
    /// Picked from a list for a field.
    Set(Field, String),
    /// A scale the picked kind is drawn at, on or off.
    Scale(u32),
    Hatched(bool),
    AddKind,
    /// A line style, a pen or a text style more, to be set after.
    Add(Part),
    /// Typed as a scale to add: "1:25".
    ScaleTyped(String),
    /// Enter in the field of a scale to add.
    AddScale,
    /// What is picked leaves the standard, unless a kind of drawing still uses it.
    Remove,
}

/// What is picked of the standard, what is being typed, and why the last change was refused.
#[derive(Debug, Clone, Default)]
pub struct StandardView {
    pub picked: Option<Setting>,
    pub draft: Option<(Field, String, Lang)>,
    pub refused: Option<String>,
    /// What is typed as a scale to add.
    pub scale: String,
}

/// A number as typed, with a decimal point or comma.
pub(super) fn number(text: &str) -> Option<f64> {
    text.trim().replace(',', ".").parse().ok().filter(|x: &f64| x.is_finite())
}

/// Dashes and gaps as typed: numbers with spaces or semicolons between them; none for a
/// continuous line.
fn dashes(text: &str) -> Result<Vec<f64>, LibraryError> {
    text.split(|c: char| c.is_whitespace() || c == ';').filter(|part| !part.is_empty()).map(|part| number(part).ok_or(LibraryError::NotPositive)).collect()
}

fn rename(names: &mut Names, lang: Lang, value: &str) -> Result<(), LibraryError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(LibraryError::NoKey);
    }
    match lang {
        Lang::Nl => names.nl = value.into(),
        Lang::En => names.en = value.into(),
        Lang::De => names.de = value.into(),
        Lang::Fr => names.fr = value.into(),
    }
    Ok(())
}

/// A view's key, as IFCX holds it.
fn view_key(view: View) -> &'static str {
    match view {
        View::Plan => "plan",
        View::CeilingPlan => "ceiling-plan",
        View::Section => "section",
        View::Elevation => "elevation",
        View::Model => "model",
    }
}

/// Sets `field` of `picked` to `value`. The standard stays as it was when it would no longer
/// hold together, and the error says why.
pub fn set(standard: &mut Standard, picked: &Setting, field: Field, value: &str, lang: Lang) -> Result<(), LibraryError> {
    let unknown = || LibraryError::UnknownSetting(value.trim().to_string());
    let mut next = standard.clone();
    match picked {
        Setting::Kind(key) => {
            let kind = next.kinds.iter_mut().find(|k| &k.key == key).ok_or_else(unknown)?;
            match field {
                Field::Name => rename(&mut kind.names, lang, value)?,
                Field::Code => kind.code = value.trim().into(),
                Field::Number => kind.number = value.trim().into(),
                Field::Discipline => kind.discipline = Discipline::ALL.into_iter().find(|d| d.code() == value).ok_or_else(unknown)?,
                Field::View => kind.view = View::ALL.into_iter().find(|v| view_key(*v) == value).ok_or_else(unknown)?,
                Field::Scale => {
                    let scale: u32 = value.trim().trim_start_matches("1:").parse().map_err(|_| unknown())?;
                    kind.scales.retain(|s| *s != scale);
                    kind.scales.insert(0, scale);
                }
                Field::Cut => kind.cut = value.into(),
                Field::Beyond => kind.beyond = value.into(),
                Field::Hidden => kind.hidden = value.into(),
                Field::Text => kind.text = value.into(),
                Field::Width | Field::Height | Field::Dashes => return Err(unknown()),
            }
        }
        Setting::LineStyle(key) => {
            let style = next.line_styles.iter_mut().find(|s| &s.key == key).ok_or_else(unknown)?;
            match field {
                Field::Name => rename(&mut style.names, lang, value)?,
                Field::Dashes => style.dashes = dashes(value)?,
                _ => return Err(unknown()),
            }
        }
        Setting::Pen(key) => {
            let pen = next.pens.iter_mut().find(|p| &p.key == key).ok_or_else(unknown)?;
            let Field::Width = field else { return Err(unknown()) };
            pen.width = number(value).ok_or(LibraryError::NotPositive)?;
            pen.names = Names::same(&format!("{} mm", pen.width));
        }
        Setting::TextStyle(key) => {
            let style = next.text_styles.iter_mut().find(|t| &t.key == key).ok_or_else(unknown)?;
            let Field::Height = field else { return Err(unknown()) };
            style.height = number(value).ok_or(LibraryError::NotPositive)?;
            style.names = Names::same(&format!("{} mm", style.height));
        }
        // a scale is its number: there is nothing to set of it
        Setting::Scale(_) => return Err(unknown()),
    }
    next.check()?;
    *standard = next;
    Ok(())
}

/// Draws kind `key` at `scale` too, or no longer; its last scale stays.
pub fn toggle_scale(standard: &mut Standard, key: &str, scale: u32) -> Result<(), LibraryError> {
    let mut next = standard.clone();
    let kind = next.kinds.iter_mut().find(|k| k.key == key).ok_or_else(|| LibraryError::UnknownSetting(key.into()))?;
    if kind.scales.contains(&scale) {
        kind.scales.retain(|s| *s != scale);
    } else {
        kind.scales.push(scale);
    }
    next.check()?;
    *standard = next;
    Ok(())
}

/// Adds a scale typed as "1:25" or "25"; the scales stay in order.
pub fn add_scale(standard: &mut Standard, typed: &str) -> Result<u32, LibraryError> {
    let scale: u32 = typed.trim().trim_start_matches("1:").trim().parse().map_err(|_| LibraryError::UnknownSetting(typed.trim().to_string()))?;
    if scale == 0 {
        return Err(LibraryError::NotPositive);
    }
    if standard.scales.contains(&scale) {
        return Err(LibraryError::Duplicate(scale_text(scale)));
    }
    standard.scales.push(scale);
    standard.scales.sort_unstable();
    Ok(scale)
}

/// A line style, a pen or a text style more, with a first value to change; what it is now.
pub fn add(standard: &mut Standard, part: Part) -> Setting {
    let fresh = |prefix: &str, taken: &[&str]| (1..).map(|n| format!("{prefix}-{n}")).find(|key| !taken.contains(&key.as_str())).unwrap_or_default();
    match part {
        Part::LineStyle => {
            let key = fresh("style", &standard.line_styles.iter().map(|s| s.key.as_str()).collect::<Vec<_>>());
            standard.line_styles.push(LineStyle { key: key.clone(), names: Names::new("nieuwe lijnstijl", "new line style", "neuer Linienstil", "nouveau style de ligne"), dashes: vec![12.0, 3.0] });
            Setting::LineStyle(key)
        }
        Part::Pen => {
            let key = fresh("pen", &standard.pens.iter().map(|p| p.key.as_str()).collect::<Vec<_>>());
            standard.pens.push(Pen { key: key.clone(), names: Names::same("0.5 mm"), width: 0.5 });
            Setting::Pen(key)
        }
        Part::TextStyle => {
            let key = fresh("text", &standard.text_styles.iter().map(|t| t.key.as_str()).collect::<Vec<_>>());
            standard.text_styles.push(TextStyle { key: key.clone(), names: Names::same("2.5 mm"), height: 2.5 });
            Setting::TextStyle(key)
        }
    }
}

/// Takes `setting` out of the standard; refused while a kind of drawing still uses it.
pub fn remove(standard: &mut Standard, setting: &Setting) -> Result<(), LibraryError> {
    let mut next = standard.clone();
    match setting {
        Setting::Kind(key) => next.kinds.retain(|k| &k.key != key),
        Setting::LineStyle(key) => next.line_styles.retain(|s| &s.key != key),
        Setting::Pen(key) => next.pens.retain(|p| &p.key != key),
        Setting::TextStyle(key) => next.text_styles.retain(|t| &t.key != key),
        Setting::Scale(scale) => next.scales.retain(|s| s != scale),
    }
    next.check()?;
    *standard = next;
    Ok(())
}

/// A kind of drawing of the office's own, drawn by the rules of the standard's first kind until
/// they are set; its key, or none when the standard has no kind to take the rules of.
pub fn add_kind(standard: &mut Standard) -> Option<String> {
    let like = standard.kinds.first()?.clone();
    let key = (1..).map(|n| format!("own-{n}")).find(|key| standard.kind(key).is_none())?;
    let names = Names::new("nieuwe tekeningsoort", "new kind of drawing", "neue Zeichnungsart", "nouveau type de dessin");
    standard.kinds.push(DrawingKind { key: key.clone(), names, code: String::new(), number: String::new(), origin: Origin::Own, ..like });
    Some(key)
}

fn view_name(view: View, l: Language) -> &'static str {
    match view {
        View::Plan => l.text("bovenaanzicht", "plan"),
        View::CeilingPlan => l.text("plafond", "ceiling plan"),
        View::Section => l.text("doorsnede", "section"),
        View::Elevation => l.text("aanzicht", "elevation"),
        View::Model => "3D",
    }
}

fn discipline_name(discipline: Discipline, l: Language) -> &'static str {
    match discipline {
        Discipline::General => l.text("algemeen", "general"),
        Discipline::Architecture => l.text("bouwkundig", "architecture"),
        Discipline::Structure => l.text("constructief", "structure"),
        Discipline::Services => l.text("installaties", "services"),
    }
}

fn origin_name(origin: Origin, l: Language) -> &'static str {
    match origin {
        Origin::Template => l.text("uit de template", "from the template"),
        Origin::Practice => l.text("uit de praktijk", "from practice"),
        Origin::Own => l.text("eigen", "own"),
    }
}

/// A line style's dashes as they are typed: "24 3 0.5 3".
fn dashes_text(dashes: &[f64]) -> String {
    dashes.iter().map(f64::to_string).collect::<Vec<_>>().join(" ")
}

/// One of a list to pick from.
#[derive(Debug, Clone, PartialEq)]
struct Choice {
    key: String,
    label: String,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

impl App {
    pub(super) fn standard_update(&mut self, message: StandardMessage) -> Task<Message> {
        let picked = self.standard_view.picked.clone();
        let typing = matches!(message, StandardMessage::Typed(..));
        let outcome = match message {
            StandardMessage::Pick(setting) => {
                self.standard_view = StandardView { picked: Some(setting), ..StandardView::default() };
                return Task::none();
            }
            StandardMessage::Typed(field, typed, lang) => {
                self.standard_view.draft = Some((field, typed.clone(), lang));
                if !field.live() {
                    return Task::none();
                }
                picked.map(|picked| set(&mut self.standard, &picked, field, &typed, lang))
            }
            StandardMessage::Done => match (picked, self.standard_view.draft.clone()) {
                (Some(picked), Some((field, typed, lang))) => Some(set(&mut self.standard, &picked, field, &typed, lang)),
                _ => None,
            },
            StandardMessage::Set(field, value) => picked.map(|picked| set(&mut self.standard, &picked, field, &value, Lang::En)),
            StandardMessage::Scale(scale) => match picked {
                Some(Setting::Kind(key)) => Some(toggle_scale(&mut self.standard, &key, scale)),
                _ => None,
            },
            StandardMessage::Hatched(on) => {
                if let Some(Setting::Kind(key)) = &picked {
                    if let Some(kind) = self.standard.kinds.iter_mut().find(|k| &k.key == key) {
                        kind.hatched = on;
                    }
                }
                Some(Ok(()))
            }
            StandardMessage::AddKind => {
                if let Some(key) = add_kind(&mut self.standard) {
                    self.standard_view = StandardView { picked: Some(Setting::Kind(key)), ..StandardView::default() };
                }
                return Task::none();
            }
            StandardMessage::Add(part) => {
                let added = add(&mut self.standard, part);
                self.standard_view = StandardView { picked: Some(added), ..StandardView::default() };
                return Task::none();
            }
            StandardMessage::ScaleTyped(typed) => {
                self.standard_view.scale = typed;
                return Task::none();
            }
            StandardMessage::AddScale => {
                let typed = self.standard_view.scale.clone();
                match add_scale(&mut self.standard, &typed) {
                    Ok(scale) => self.standard_view = StandardView { picked: Some(Setting::Scale(scale)), ..StandardView::default() },
                    Err(why) => self.standard_view.refused = Some(why.to_string()),
                }
                return Task::none();
            }
            StandardMessage::Remove => {
                if let Some(picked) = &picked {
                    match remove(&mut self.standard, picked) {
                        Ok(()) => self.standard_view = StandardView::default(),
                        Err(why) => self.standard_view.refused = Some(why.to_string()),
                    }
                }
                return Task::none();
            }
        };
        match outcome {
            // taken; a name or a code goes on being typed, as it was typed
            Some(Ok(())) => {
                if !typing {
                    self.standard_view.draft = None;
                }
                self.standard_view.refused = None;
            }
            // refused: what was typed stays in its field, with the reason under the fields
            Some(Err(why)) => self.standard_view.refused = Some(why.to_string()),
            None => {}
        }
        Task::none()
    }

    /// Takes over the drawing standard an opened project's file holds: its nodes, as the reader
    /// gives them under [`crate::export::ifcx::STANDARDS`].
    pub(super) fn adopt_standard(&mut self, nodes: &Value) {
        if let Ok(mut standards) = drawing::read(&serde_json::json!({ "data": nodes })) {
            if !standards.is_empty() {
                self.standard = standards.remove(0);
                self.standard_view = StandardView::default();
            }
        }
    }

    /// The tiles of a category of the standard on the Library page: its kinds of drawing, its
    /// scales, its line styles, its line weights or its text styles, each with a look at it. A
    /// click picks one (its fields are in the right pane); the last tile makes one more.
    pub(super) fn standard_tiles(&self, p: Palette, l: Language, category: Category) -> Vec<Element<'_, Message>> {
        let lang = super::library::lang(l);
        let standard = &self.standard;
        let picked = &self.standard_view.picked;
        let pick = |setting: Setting| Some(Message::Standard(StandardMessage::Pick(setting)));
        // a line as a pen and a line style draw it
        let sample = |dashes: &[f64], width: f64| -> Element<'_, Message> { canvas_widget(LineSample::new(dashes, width, p.text)).width(Fill).height(14).into() };
        // how many kinds of drawing use a setting
        let used = |uses: &dyn Fn(&DrawingKind) -> bool| format!("{}: {}", l.text("Tekeningsoorten", "Kinds of drawing"), standard.kinds.iter().filter(|kind| uses(kind)).count());
        let width_of = |pen: &str| standard.pen(pen).map_or(0.25, |pen| pen.width);
        let more = |label: &'static str, message: StandardMessage| tile_more(p, label, Message::Standard(message));
        match category {
            Category::Kinds => {
                let mut tiles: Vec<Element<'_, Message>> = standard
                    .kinds
                    .iter()
                    .map(|kind| {
                        let setting = Setting::Kind(kind.key.clone());
                        let hidden = standard.line_style(&kind.hidden).map_or(&[][..], |style| style.dashes.as_slice());
                        // what is cut, what is seen beyond it and what is hidden, as the kind
                        // draws them
                        let look = column![sample(&[], width_of(&kind.cut)), sample(&[], width_of(&kind.beyond)), sample(hidden, width_of(&kind.beyond))].spacing(8);
                        let scales = kind.scales.iter().map(|scale| scale_text(*scale)).collect::<Vec<_>>().join(", ");
                        let about = format!("{} · {} · {scales}", kind.code, kind.discipline.code());
                        tile(p, look.into(), kind.names.get(lang).to_string(), about, *picked == Some(setting.clone()), pick(setting))
                    })
                    .collect();
                tiles.push(more(l.text("+ Tekeningsoort", "+ Kind of drawing"), StandardMessage::AddKind));
                tiles
            }
            Category::Scales => {
                let mut tiles: Vec<Element<'_, Message>> = standard
                    .scales
                    .iter()
                    .map(|&scale| {
                        let setting = Setting::Scale(scale);
                        let look = container(text(scale_text(scale)).size(26).color(p.text)).width(Fill).align_x(Center);
                        let name = format!("{} {}", l.text("Schaal", "Scale"), scale_text(scale));
                        tile(p, look.into(), name, used(&|kind| kind.scales.contains(&scale)), *picked == Some(setting.clone()), pick(setting))
                    })
                    .collect();
                // a scale more is typed
                let name = l.text("Schaal erbij: 1:25", "A scale more: 1:25");
                let typed = text_input(name, &self.standard_view.scale).on_input(|typed| Message::Standard(StandardMessage::ScaleTyped(typed))).on_submit(Message::Standard(StandardMessage::AddScale)).size(12).padding([6, 8]).width(TILE);
                tiles.push(openaec_ui::design::a11y::named(typed, openaec_ui::design::a11y::Role::TextField, name));
                tiles
            }
            Category::LineStyles => {
                let mut tiles: Vec<Element<'_, Message>> = standard
                    .line_styles
                    .iter()
                    .map(|style| {
                        let setting = Setting::LineStyle(style.key.clone());
                        tile(p, sample(&style.dashes, 0.35), style.names.get(lang).to_string(), used(&|kind| kind.hidden == style.key), *picked == Some(setting.clone()), pick(setting))
                    })
                    .collect();
                tiles.push(more(l.text("+ Lijnstijl", "+ Line style"), StandardMessage::Add(Part::LineStyle)));
                tiles
            }
            Category::Pens => {
                let mut tiles: Vec<Element<'_, Message>> = standard
                    .pens
                    .iter()
                    .map(|pen| {
                        let setting = Setting::Pen(pen.key.clone());
                        tile(p, sample(&[], pen.width), pen.names.get(lang).to_string(), used(&|kind| kind.cut == pen.key || kind.beyond == pen.key), *picked == Some(setting.clone()), pick(setting))
                    })
                    .collect();
                tiles.push(more(l.text("+ Lijndikte", "+ Line weight"), StandardMessage::Add(Part::Pen)));
                tiles
            }
            Category::TextStyles => {
                let mut tiles: Vec<Element<'_, Message>> = standard
                    .text_styles
                    .iter()
                    .map(|style| {
                        let setting = Setting::TextStyle(style.key.clone());
                        // its capitals as high on screen as a line of that many millimetres is wide
                        let look = container(text("Abc 123").size((style.height as f32 * LineSample::SCALE).clamp(8.0, 40.0)).color(p.text)).width(Fill).align_x(Center);
                        tile(p, look.into(), style.names.get(lang).to_string(), used(&|kind| kind.text == style.key), *picked == Some(setting.clone()), pick(setting))
                    })
                    .collect();
                tiles.push(more(l.text("+ Tekststijl", "+ Text style"), StandardMessage::Add(Part::TextStyle)));
                tiles
            }
            _ => Vec::new(),
        }
    }

    /// The right pane of the Library product: the fields of what is picked of the standard.
    pub fn standard_pane(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let lang = super::library::lang(l);
        let standard = &self.standard;
        let view = &self.standard_view;
        let refused = view.refused.as_ref().map(|why| {
            let refused = l.text("Niet overgenomen: de standaard zou dan niet meer kloppen.", "Not taken: the standard would no longer hold together.");
            text(format!("{refused} ({why})")).size(11).color(p.accent).wrapping(Wrapping::Word)
        });
        let Some(picked) = &view.picked else {
            let hint = l.text("Kies een tekeningsoort of een instelling op de pagina om haar in te stellen.", "Pick a kind of drawing or a setting on the page to set it up.");
            return container(column![text(hint).size(12).color(p.text_muted).wrapping(Wrapping::Word)].push(refused).spacing(8)).padding(8).into();
        };
        let typed = |field: Field, value: String| -> Element<'_, StandardMessage> {
            let shown = match &view.draft {
                Some((f, text, _)) if *f == field => text.clone(),
                _ => value,
            };
            text_input("", &shown).on_input(move |text| StandardMessage::Typed(field, text, lang)).on_submit(StandardMessage::Done).size(12).padding([2, 6]).into()
        };
        let pick = |field: Field, choices: Vec<Choice>, key: &str| -> Element<'_, StandardMessage> {
            let now = choices.iter().find(|c| c.key == key).cloned();
            pick_list(choices, now, move |c: Choice| StandardMessage::Set(field, c.key)).text_size(12).padding([2, 6]).width(Fill).into()
        };
        let pens = || standard.pens.iter().map(|pen| Choice { key: pen.key.clone(), label: pen.names.get(lang).to_string() }).collect::<Vec<_>>();
        let mut fields = Column::new().spacing(6);
        let title;
        match picked {
            Setting::Kind(key) => {
                let Some(kind) = standard.kind(key) else { return Space::new().into() };
                title = kind.names.get(lang).to_string();
                let disciplines = Discipline::ALL.map(|d| Choice { key: d.code().into(), label: format!("{} · {}", d.code(), discipline_name(d, l)) }).to_vec();
                let views = View::ALL.map(|v| Choice { key: view_key(v).into(), label: view_name(v, l).into() }).to_vec();
                let scales = kind.scales.iter().map(|s| Choice { key: s.to_string(), label: scale_text(*s) }).collect::<Vec<_>>();
                let styles = standard.line_styles.iter().map(|s| Choice { key: s.key.clone(), label: s.names.get(lang).to_string() }).collect::<Vec<_>>();
                let texts = standard.text_styles.iter().map(|t| Choice { key: t.key.clone(), label: t.names.get(lang).to_string() }).collect::<Vec<_>>();
                // every scale of the standard: on when this kind is drawn at it
                let chips = row(standard.scales.iter().map(|&scale| {
                    let on = kind.scales.contains(&scale);
                    button(text(scale_text(scale)).size(11).color(if on { p.text } else { p.text_muted })).padding([2, 6]).style(move |_, s| p.ribbon_button(on, s)).on_press(StandardMessage::Scale(scale)).into()
                }))
                .spacing(3)
                .wrap();
                fields = fields
                    .push(labelled(p, l.text("Naam", "Name"), typed(Field::Name, kind.names.get(lang).to_string())))
                    .push(labelled(p, "Code", typed(Field::Code, kind.code.clone())))
                    .push(labelled(p, l.text("Nummer", "Number"), typed(Field::Number, kind.number.clone())))
                    .push(labelled(p, "Discipline", pick(Field::Discipline, disciplines, kind.discipline.code())))
                    .push(labelled(p, l.text("Aanzicht", "View"), pick(Field::View, views, view_key(kind.view))))
                    .push(labelled(p, l.text("Schaal", "Scale"), pick(Field::Scale, scales, &kind.scales.first().map(u32::to_string).unwrap_or_default())))
                    .push(labelled(p, l.text("Getekend op", "Drawn at"), chips.into()))
                    .push(labelled(p, l.text("Doorgesneden", "Cut"), pick(Field::Cut, pens(), &kind.cut)))
                    .push(labelled(p, l.text("Daarachter", "Beyond"), pick(Field::Beyond, pens(), &kind.beyond)))
                    .push(labelled(p, l.text("Verborgen", "Hidden"), pick(Field::Hidden, styles, &kind.hidden)))
                    .push(labelled(p, l.text("Tekst", "Text"), pick(Field::Text, texts, &kind.text)))
                    .push(checkbox(kind.hatched).label(l.text("Doorgesneden materiaal arceren", "Hatch what is cut")).on_toggle(StandardMessage::Hatched).size(14).text_size(12))
                    .push(labelled(p, l.text("Herkomst", "Origin"), text(origin_name(kind.origin, l)).size(12).color(p.text_secondary).into()));
            }
            Setting::LineStyle(key) => {
                let Some(style) = standard.line_style(key) else { return Space::new().into() };
                title = style.names.get(lang).to_string();
                fields = fields
                    .push(labelled(p, l.text("Naam", "Name"), typed(Field::Name, style.names.get(lang).to_string())))
                    .push(labelled(p, l.text("Strepen en tussenruimtes", "Dashes and gaps"), typed(Field::Dashes, dashes_text(&style.dashes))))
                    .push(text(l.text("In lijndiktes: streep, ruimte, streep, ruimte. Leeg is een doorgetrokken lijn.", "In line widths: dash, gap, dash, gap. Empty is a continuous line.")).size(11).color(p.text_muted).wrapping(Wrapping::Word))
                    .push(canvas_widget(LineSample::new(&style.dashes, 0.5, p.text)).width(Fill).height(16));
            }
            Setting::Pen(key) => {
                let Some(pen) = standard.pen(key) else { return Space::new().into() };
                title = pen.names.get(lang).to_string();
                fields = fields.push(labelled(p, l.text("Breedte (mm)", "Width (mm)"), typed(Field::Width, pen.width.to_string()))).push(canvas_widget(LineSample::new(&[], pen.width, p.text)).width(Fill).height(16));
            }
            Setting::TextStyle(key) => {
                let Some(style) = standard.text_style(key) else { return Space::new().into() };
                title = style.names.get(lang).to_string();
                fields = fields.push(labelled(p, l.text("Hoogte (mm)", "Height (mm)"), typed(Field::Height, style.height.to_string())));
            }
            Setting::Scale(scale) => {
                title = scale_text(*scale);
                // the kinds of drawing made at it: while there are any, it stays
                let users: Vec<&str> = standard.kinds.iter().filter(|k| k.scales.contains(scale)).map(|k| k.names.get(lang)).collect();
                let users = if users.is_empty() { l.text("geen tekeningsoort", "no kind of drawing").to_string() } else { users.join(", ") };
                fields = fields.push(labelled(p, l.text("Gebruikt door", "Used by"), text(users).size(12).color(p.text_secondary).wrapping(Wrapping::Word).into()));
            }
        }
        fields = fields.push(button(text(l.text("Verwijderen", "Remove")).size(12).color(p.text_secondary)).padding([3, 8]).style(move |_, s| p.row_button(s)).on_press(StandardMessage::Remove)).push(refused);
        let pane = column![text(title).size(14).font(FONT_BOLD).color(p.text), fields].spacing(10);
        Element::from(container(pane).padding(8)).map(Message::Standard)
    }
}

/// A field of the right pane: its label, and what sets it.
fn labelled<'a>(p: Palette, label: &'static str, control: Element<'a, StandardMessage>) -> Element<'a, StandardMessage> {
    let field = row![text(label).size(12).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph).width(Length::FillPortion(2)), container(control).width(Length::FillPortion(3))].spacing(8).align_y(Center);
    // the label names the field beside it
    openaec_ui::design::a11y::named(field, openaec_ui::design::a11y::Role::Label, label)
}

/// A line as a pen and a line style draw it: its width in millimetres on paper as pixels, its
/// dashes as multiples of that width.
struct LineSample {
    dashes: Vec<f32>,
    width: f32,
    ink: Color,
}

impl LineSample {
    /// Pixels per millimetre on paper, so the widths can be told apart on screen.
    const SCALE: f32 = 4.0;

    fn new(dashes: &[f64], width: f64, ink: Color) -> Self {
        let width = (width as f32 * Self::SCALE).max(1.0);
        Self { dashes: dashes.iter().map(|d| (*d as f32 * width).max(1.0)).collect(), width, ink }
    }
}

impl<M> canvas::Program<M> for LineSample {
    type State = ();

    fn draw(&self, _state: &(), renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let y = bounds.height / 2.0;
        let line = Path::line(Point::new(0.0, y), Point::new(bounds.width, y));
        frame.stroke(&line, Stroke { line_dash: LineDash { segments: &self.dashes, offset: 0 }, ..Stroke::default().with_color(self.ink).with_width(self.width) });
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kind_of_drawing_is_set_field_by_field() {
        let mut app = App::default();
        let plan = Setting::Kind("floor-plan".into());
        let _ = app.standard_update(StandardMessage::Pick(plan.clone()));
        // a name is taken as it is typed, in the language it is shown in
        let _ = app.standard_update(StandardMessage::Typed(Field::Name, "plattegrond BWK".into(), Lang::Nl));
        assert_eq!(app.standard.kind("floor-plan").unwrap().names.nl, "plattegrond BWK");
        assert_eq!(app.standard.kind("floor-plan").unwrap().names.en, "floor plan");
        // its usual scale is picked; a scale it is drawn at goes on and off
        let _ = app.standard_update(StandardMessage::Set(Field::Scale, "100".into()));
        assert_eq!(app.standard.kind("floor-plan").unwrap().scales, [100, 50]);
        let _ = app.standard_update(StandardMessage::Scale(200));
        let _ = app.standard_update(StandardMessage::Scale(50));
        assert_eq!(app.standard.kind("floor-plan").unwrap().scales, [100, 200]);
        // its pens, its view and its discipline are picked from the standard's own
        let _ = app.standard_update(StandardMessage::Set(Field::Cut, "0.7".into()));
        let _ = app.standard_update(StandardMessage::Set(Field::View, "ceiling-plan".into()));
        let _ = app.standard_update(StandardMessage::Set(Field::Discipline, "CON".into()));
        let _ = app.standard_update(StandardMessage::Hatched(false));
        let kind = app.standard.kind("floor-plan").unwrap();
        assert_eq!((kind.cut.as_str(), kind.view, kind.discipline, kind.hatched), ("0.7", View::CeilingPlan, Discipline::Structure, false));
        assert!(app.standard_view.refused.is_none());
        // what does not fit the standard is refused, and the standard stays as it was
        let _ = app.standard_update(StandardMessage::Set(Field::Cut, "3".into()));
        assert!(app.standard_view.refused.is_some());
        assert_eq!(app.standard.kind("floor-plan").unwrap().cut, "0.7");
        let _ = app.standard_update(StandardMessage::Scale(100));
        let _ = app.standard_update(StandardMessage::Scale(200));
        assert_eq!(app.standard.kind("floor-plan").unwrap().scales.len(), 1, "its last scale stays");
        assert_eq!(app.standard.check(), Ok(()));
    }

    #[test]
    fn a_pen_and_a_line_style_wait_for_enter() {
        let mut app = App::default();
        let _ = app.standard_update(StandardMessage::Pick(Setting::Pen("0.25".into())));
        // half typed, a width is no number yet: nothing changes until Enter
        let _ = app.standard_update(StandardMessage::Typed(Field::Width, "0,".into(), Lang::Nl));
        assert_eq!(app.standard.pen("0.25").unwrap().width, 0.25);
        let _ = app.standard_update(StandardMessage::Typed(Field::Width, "0,3".into(), Lang::Nl));
        let _ = app.standard_update(StandardMessage::Done);
        let pen = app.standard.pen("0.25").unwrap();
        assert_eq!((pen.width, pen.names.nl.as_str()), (0.3, "0.3 mm"));
        assert!(app.standard_view.draft.is_none());
        // a width of nothing is refused and stays in its field
        let _ = app.standard_update(StandardMessage::Typed(Field::Width, "0".into(), Lang::Nl));
        let _ = app.standard_update(StandardMessage::Done);
        assert!(app.standard_view.refused.is_some() && app.standard_view.draft.is_some());
        assert_eq!(app.standard.pen("0.25").unwrap().width, 0.3);
        // a line style: dashes with their gaps; a dash alone is refused
        let _ = app.standard_update(StandardMessage::Pick(Setting::LineStyle("dashed".into())));
        let _ = app.standard_update(StandardMessage::Typed(Field::Dashes, "6 3".into(), Lang::Nl));
        let _ = app.standard_update(StandardMessage::Done);
        assert_eq!(app.standard.line_style("dashed").unwrap().dashes, [6.0, 3.0]);
        let _ = app.standard_update(StandardMessage::Typed(Field::Dashes, "6".into(), Lang::Nl));
        let _ = app.standard_update(StandardMessage::Done);
        assert_eq!(app.standard.line_style("dashed").unwrap().dashes, [6.0, 3.0]);
        let _ = app.standard_update(StandardMessage::Pick(Setting::TextStyle("2.5".into())));
        let _ = app.standard_update(StandardMessage::Typed(Field::Height, "3".into(), Lang::Nl));
        let _ = app.standard_update(StandardMessage::Done);
        assert_eq!(app.standard.text_style("2.5").unwrap().height, 3.0);
    }

    #[test]
    fn a_kind_of_ones_own_is_added_and_removed_and_the_standard_goes_with_the_project() {
        let mut app = App::default();
        let kinds = app.standard.kinds.len();
        let _ = app.standard_update(StandardMessage::AddKind);
        let Some(Setting::Kind(key)) = app.standard_view.picked.clone() else { panic!("the new kind is picked") };
        assert_eq!((app.standard.kinds.len(), app.standard.kind(&key).map(|k| k.origin)), (kinds + 1, Some(Origin::Own)));
        let _ = app.standard_update(StandardMessage::Typed(Field::Code, "KZS".into(), Lang::Nl));
        assert_eq!(app.standard.check(), Ok(()));
        // the standard's nodes among a file's nodes come back as the project's standard
        let mut nodes = vec![serde_json::json!({"path": "wall", "attributes": {"bsi::ifc::class": {"code": "IfcWall"}}})];
        nodes.extend(drawing::nodes(&app.standard));
        let held = drawing::held(&nodes).expect("the file holds a standard");
        assert_eq!(held.len(), nodes.len() - 1);
        let mut other = App::default();
        other.adopt_standard(&Value::Array(held));
        assert_eq!(other.standard, app.standard);
        assert_eq!(other.standard.kind(&key).map(|k| k.code.as_str()), Some("KZS"));
        // a file without one leaves the project's standard alone
        assert!(drawing::held(&nodes[..1]).is_none());
        let _ = app.standard_update(StandardMessage::Remove);
        assert_eq!((app.standard.kinds.len(), app.standard_view.picked.is_none()), (kinds, true));
    }

    #[test]
    fn settings_are_added_and_removed_unless_a_kind_uses_them() {
        let mut app = App::default();
        // a scale more, typed with or without "1:"; it takes its place in the order
        let _ = app.standard_update(StandardMessage::ScaleTyped("1:25".into()));
        let _ = app.standard_update(StandardMessage::AddScale);
        assert_eq!(app.standard.scales, [1, 2, 5, 10, 20, 25, 50, 100, 200, 500, 1000]);
        assert_eq!(app.standard_view.picked, Some(Setting::Scale(25)));
        let _ = app.standard_update(StandardMessage::ScaleTyped("25".into()));
        let _ = app.standard_update(StandardMessage::AddScale);
        assert!(app.standard_view.refused.is_some(), "it is there already");
        // no kind is drawn at it: it can go again; 1:50 cannot, floor plans are drawn at it
        let _ = app.standard_update(StandardMessage::Pick(Setting::Scale(25)));
        let _ = app.standard_update(StandardMessage::Remove);
        assert!(!app.standard.scales.contains(&25) && app.standard_view.picked.is_none());
        let _ = app.standard_update(StandardMessage::Pick(Setting::Scale(50)));
        let _ = app.standard_update(StandardMessage::Remove);
        assert!(app.standard.scales.contains(&50) && app.standard_view.refused.is_some());
        // a line style, a pen and a text style more: each is picked, to be set
        let (styles, pens, texts) = (app.standard.line_styles.len(), app.standard.pens.len(), app.standard.text_styles.len());
        let _ = app.standard_update(StandardMessage::Add(Part::LineStyle));
        assert_eq!(app.standard_view.picked, Some(Setting::LineStyle("style-1".into())));
        let _ = app.standard_update(StandardMessage::Typed(Field::Name, "erfgrens".into(), Lang::Nl));
        let _ = app.standard_update(StandardMessage::Add(Part::Pen));
        let _ = app.standard_update(StandardMessage::Typed(Field::Width, "1.4".into(), Lang::Nl));
        let _ = app.standard_update(StandardMessage::Done);
        let _ = app.standard_update(StandardMessage::Add(Part::TextStyle));
        assert_eq!((app.standard.line_styles.len(), app.standard.pens.len(), app.standard.text_styles.len()), (styles + 1, pens + 1, texts + 1));
        assert_eq!((app.standard.line_style("style-1").unwrap().names.nl.as_str(), app.standard.pen("pen-1").unwrap().width), ("erfgrens", 1.4));
        assert_eq!(app.standard.check(), Ok(()));
        // the new pen goes again; the pen floor plans are cut with does not
        let _ = app.standard_update(StandardMessage::Pick(Setting::Pen("pen-1".into())));
        let _ = app.standard_update(StandardMessage::Remove);
        assert_eq!(app.standard.pens.len(), pens);
        let _ = app.standard_update(StandardMessage::Pick(Setting::Pen("0.5".into())));
        let _ = app.standard_update(StandardMessage::Remove);
        assert!(app.standard.pen("0.5").is_some() && app.standard_view.refused.is_some());
    }

    #[test]
    fn the_page_lists_the_kinds_and_the_pane_shows_the_fields_of_the_picked_one() {
        let mut app = App::default();
        use super::super::libraries::{Country, LibrariesMessage};
        let _ = app.libraries_update(LibrariesMessage::Country(Country::Netherlands));
        {
            // the Netherlands shows its kinds of drawing first, a tile each, and one to make another
            let mut page = crate::app::test_ui_of(iced::Size::new(1300.0, 1800.0), app.libraries_page(app.theme.palette(), Language::English));
            assert!(page.find("Kinds of drawing").is_ok() && page.find("Country · Netherlands · 20").is_ok() && page.find("pile plan").is_ok());
            assert!(page.find("PLG · CON · 1:100").is_ok() && page.find("+ Kind of drawing").is_ok() && page.find("First sheet: 300").is_ok());
        }
        let _ = app.libraries_update(LibrariesMessage::Category(Category::LineStyles));
        {
            let mut page = crate::app::test_ui_of(iced::Size::new(1300.0, 900.0), app.libraries_page(app.theme.palette(), Language::English));
            assert!(page.find("Line styles").is_ok() && page.find("long-dashed dotted").is_ok() && page.find("+ Line style").is_ok());
        }
        let _ = app.libraries_update(LibrariesMessage::Category(Category::Scales));
        {
            let mut page = crate::app::test_ui_of(iced::Size::new(1300.0, 900.0), app.libraries_page(app.theme.palette(), Language::English));
            assert!(page.find("Scale 1:50").is_ok() && page.find("A scale more: 1:25").is_ok());
        }
        {
            let mut pane = crate::app::test_ui_of(iced::Size::new(320.0, 700.0), app.standard_pane(app.theme.palette(), Language::English));
            assert!(pane.find("Pick a kind of drawing or a setting on the page to set it up.").is_ok());
        }
        let _ = app.standard_update(StandardMessage::Pick(Setting::Kind("pile-plan".into())));
        let mut pane = crate::app::test_ui_of(iced::Size::new(320.0, 700.0), app.standard_pane(app.theme.palette(), Language::English));
        assert!(pane.find("Drawn at").is_ok() && pane.find("from the template").is_ok());
    }
}
