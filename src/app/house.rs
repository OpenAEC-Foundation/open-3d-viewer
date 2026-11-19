//! The house style in the Library product: how an office's drawings and documents look
//! (`bim_library::house`), under the source Company.
//!
//! The page shows its colours, its typefaces and its sheet formats as tiles; a sheet format is
//! drawn with its margins and the title block in its corner. The right pane sets the office's
//! own data, its typefaces, the picked colour and how dimensions are written, under a sample
//! sheet that shows the style as it is set. What is typed is taken when the style still holds
//! together; otherwise it stays in its field and the pane says why not.
//!
//! The house style is kept with the project, in its IFCX file, and a change of it is a step to
//! take back, as a change of the library or the drawing standard is.

use super::{App, Message, FONT_BOLD};
use crate::theme::Palette;
use bim_library::house::{Colour, Field, HouseStyle, SheetFormat, Terminator};
use bim_library::{Lang, Names};
use iced::mouse;
use iced::widget::canvas::{self, Frame, Path, Stroke, Text};
use iced::widget::text::Wrapping;
use iced::widget::{button, canvas as canvas_widget, column, container, pick_list, row, text, text_input, Column, Space};
use iced::{Center, Color, Element, Fill, Length, Point, Rectangle, Size, Task};
use openaec_ui::design::a11y::{self, Role as Aria};
use openaec_ui::Language;

/// A field of the house style that is typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Typed {
    Name,
    Address,
    Postcode,
    City,
    Country,
    Phone,
    Email,
    Website,
    Registration,
    TextFont,
    HeadingFont,
    /// The colour picked, as six hexadecimal digits.
    Hex,
    /// The name of the colour picked, in the interface's language.
    ColourName,
    TextHeight,
    TerminatorSize,
    Gap,
    Overshoot,
    BlockWidth,
}

#[derive(Debug, Clone)]
pub enum HouseMessage {
    /// Set this colour, by its key.
    Colour(String),
    /// A colour more, of the office's own.
    AddColour,
    /// Removes the colour picked, when it is one of the office's own.
    RemoveColour,
    /// Show this sheet format as the sample.
    Sheet(String),
    Typed(Typed, String, Lang),
    Terminator(Terminator),
}

/// Which colour is set, which sheet format the sample shows, what is being typed that was not
/// taken, and why not.
#[derive(Debug, Clone, Default)]
pub struct HouseView {
    pub colour: Option<String>,
    pub sheet: Option<String>,
    pub draft: Option<(Typed, String)>,
    pub refused: Option<String>,
}

/// The roles a house style's colours have from the start: they are not removed.
const ROLES: [&str; 6] = ["primary", "secondary", "accent", "text", "muted", "background"];

fn name_mut(names: &mut Names, lang: Lang) -> &mut String {
    match lang {
        Lang::Nl => &mut names.nl,
        Lang::En => &mut names.en,
        Lang::De => &mut names.de,
        Lang::Fr => &mut names.fr,
    }
}

/// `typed` in field `field` of `style` (of colour `colour` for the colour's fields); none when
/// it is no value for that field.
pub fn set(style: &HouseStyle, field: Typed, typed: &str, colour: Option<&str>, lang: Lang) -> Option<HouseStyle> {
    let mut next = style.clone();
    let text = typed.to_string();
    let size = || super::standards::number(typed);
    match field {
        Typed::Name => next.company.name = text,
        Typed::Address => next.company.address = text,
        Typed::Postcode => next.company.postcode = text,
        Typed::City => next.company.city = text,
        Typed::Country => next.company.country = text,
        Typed::Phone => next.company.phone = text,
        Typed::Email => next.company.email = text,
        Typed::Website => next.company.website = text,
        Typed::Registration => next.company.registration = text,
        Typed::TextFont => next.fonts.text = text,
        Typed::HeadingFont => next.fonts.heading = text,
        Typed::Hex => next.colours.iter_mut().find(|c| Some(c.key.as_str()) == colour)?.rgb = super::materials::colour_of(typed)?,
        Typed::ColourName => *name_mut(&mut next.colours.iter_mut().find(|c| Some(c.key.as_str()) == colour)?.names, lang) = text,
        Typed::TextHeight => next.dimensions.text_height = size()?,
        Typed::TerminatorSize => next.dimensions.terminator_size = size()?,
        Typed::Gap => next.dimensions.gap = size()?,
        Typed::Overshoot => next.dimensions.overshoot = size()?,
        Typed::BlockWidth => next.title_block.width = size()?,
    }
    Some(next)
}

/// A colour more of the office's own, grey to start with; its key.
pub fn add_colour(style: &mut HouseStyle) -> String {
    let key = (1..).map(|n| format!("own-{n}")).find(|key| style.colour(key).is_none()).unwrap_or_default();
    style.colours.push(Colour { key: key.clone(), names: Names::new("eigen kleur", "own colour", "eigene Farbe", "couleur propre"), rgb: [128, 128, 128] });
    key
}

fn rgb(c: [u8; 3]) -> Color {
    Color::from_rgb8(c[0], c[1], c[2])
}

fn mm(v: f64) -> String {
    bim_library::millimetres(v / 1000.0, Lang::En)
}

impl App {
    pub(super) fn house_update(&mut self, message: HouseMessage) -> Task<Message> {
        let before = self.house.clone();
        self.house_apply(message);
        // the project's file holds its house style: what the model shows is made again
        if self.house != before && self.modeller.shown.is_some() {
            self.show_modelled();
        }
        Task::none()
    }

    fn house_apply(&mut self, message: HouseMessage) {
        match message {
            HouseMessage::Colour(key) => self.house_view = HouseView { colour: Some(key), sheet: self.house_view.sheet.take(), ..HouseView::default() },
            HouseMessage::AddColour => {
                let key = add_colour(&mut self.house);
                self.house_view = HouseView { colour: Some(key), sheet: self.house_view.sheet.take(), ..HouseView::default() };
            }
            HouseMessage::RemoveColour => {
                if let Some(key) = self.house_view.colour.clone().filter(|key| !ROLES.contains(&key.as_str())) {
                    self.house.colours.retain(|c| c.key != key);
                    self.house_view.colour = None;
                }
            }
            HouseMessage::Sheet(key) => self.house_view.sheet = Some(key),
            HouseMessage::Typed(field, typed, lang) => {
                let taken = set(&self.house, field, &typed, self.house_view.colour.as_deref(), lang).ok_or_else(|| "?".to_string()).and_then(|next| next.check().map(|()| next).map_err(|why| why.to_string()));
                match taken {
                    Ok(next) => {
                        self.house = next;
                        (self.house_view.draft, self.house_view.refused) = (None, None);
                    }
                    Err(why) => (self.house_view.draft, self.house_view.refused) = (Some((field, typed)), Some(why)),
                }
            }
            HouseMessage::Terminator(terminator) => self.house.dimensions.terminator = terminator,
        }
    }

    /// Takes over the house style an opened project's file holds.
    pub(super) fn adopt_house(&mut self, style: &serde_json::Value) {
        if let Ok(style) = serde_json::from_value::<HouseStyle>(style.clone()) {
            if style.check().is_ok() {
                self.house = style;
                self.house_view = HouseView::default();
            }
        }
    }

    /// The colours of the house style as tiles: a swatch, its name and its hexadecimal value.
    pub(super) fn house_colour_tiles(&self, p: Palette, l: Language) -> Vec<Element<'_, Message>> {
        let lang = super::library::lang(l);
        let mut tiles: Vec<Element<'_, Message>> = self
            .house
            .colours
            .iter()
            .map(|c| {
                let colour = rgb(c.rgb);
                let swatch = container(Space::new().width(Fill).height(64.0)).style(move |_| container::Style { background: Some(colour.into()), border: iced::Border { color: p.border, width: 1.0, radius: 3.0.into() }, ..Default::default() });
                let on = self.house_view.colour.as_deref() == Some(c.key.as_str());
                super::libraries::tile(p, swatch.into(), c.names.get(lang).to_string(), super::materials::hex(c.rgb), on, Some(Message::House(HouseMessage::Colour(c.key.clone()))))
            })
            .collect();
        tiles.push(super::libraries::tile_more(p, l.text("+ Kleur", "+ Colour"), Message::House(HouseMessage::AddColour)));
        tiles
    }

    /// The typefaces of the house style as tiles.
    pub(super) fn house_font_tiles(&self, p: Palette, l: Language) -> Vec<Element<'_, Message>> {
        let tile = |family: &str, role: &'static str| super::libraries::tile(p, container(text("Aa Bb 123").size(26).color(p.text)).width(Fill).align_x(Center).into(), family.to_string(), role.to_string(), false, None);
        vec![tile(&self.house.fonts.text, l.text("lopende tekst", "running text")), tile(&self.house.fonts.heading, l.text("koppen", "headings"))]
    }

    /// The sheet formats of the house style as tiles, each drawn with its margins and its title
    /// block; a click shows it as the sample.
    pub(super) fn house_sheet_tiles(&self, p: Palette) -> Vec<Element<'_, Message>> {
        let shown = self.sample_sheet().map(|s| s.key.clone());
        self.house
            .sheets
            .iter()
            .map(|sheet| {
                let look = canvas_widget(SampleSheet { style: self.house.clone(), sheet: sheet.clone(), ink: p.text, labels: false }).width(Fill).height(64.0);
                let on = shown.as_deref() == Some(sheet.key.as_str());
                super::libraries::tile(p, look.into(), sheet.key.clone(), format!("{} × {} mm", mm(sheet.width), mm(sheet.height)), on, Some(Message::House(HouseMessage::Sheet(sheet.key.clone()))))
            })
            .collect()
    }

    /// The sheet format the sample shows: the one picked, else A3, else the first.
    fn sample_sheet(&self) -> Option<&SheetFormat> {
        let picked = self.house_view.sheet.as_deref().and_then(|key| self.house.sheet(key));
        picked.or_else(|| self.house.sheet("A3")).or_else(|| self.house.sheets.first())
    }

    /// The right pane of the source Company: a sample sheet, and the fields of the house style.
    pub fn house_pane(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let lang = super::library::lang(l);
        let style = &self.house;
        let view = &self.house_view;
        let caption = |label: &'static str| text(label).size(11).color(p.text_muted);
        let typed = |field: Typed, now: String, label: &'static str| -> Element<'_, HouseMessage> {
            let shown = match &view.draft {
                Some((at, typed)) if *at == field => typed.clone(),
                _ => now,
            };
            let input = text_input("", &shown).on_input(move |typed| HouseMessage::Typed(field, typed, lang)).size(12).padding([2, 6]);
            row![text(label).size(12).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph).width(Length::FillPortion(2)), container(a11y::named(input, Aria::TextField, label)).width(Length::FillPortion(3))].spacing(8).align_y(Center).into()
        };
        let mut fields: Column<'_, HouseMessage> = Column::new().spacing(4);
        let c = &style.company;
        fields = fields.push(caption(l.text("Bedrijf", "Company")));
        for (field, now, label) in [
            (Typed::Name, &c.name, l.text("Naam", "Name")),
            (Typed::Address, &c.address, l.text("Adres", "Address")),
            (Typed::Postcode, &c.postcode, l.text("Postcode", "Postcode")),
            (Typed::City, &c.city, l.text("Plaats", "City")),
            (Typed::Country, &c.country, l.text("Land", "Country")),
            (Typed::Phone, &c.phone, l.text("Telefoon", "Phone")),
            (Typed::Email, &c.email, l.text("E-mail", "E-mail")),
            (Typed::Website, &c.website, l.text("Website", "Website")),
            (Typed::Registration, &c.registration, l.text("KvK-nummer", "Trade register")),
        ] {
            fields = fields.push(typed(field, now.clone(), label));
        }
        fields = fields.push(caption(l.text("Lettertypen", "Fonts")));
        fields = fields.push(typed(Typed::TextFont, style.fonts.text.clone(), l.text("Lopende tekst", "Running text")));
        fields = fields.push(typed(Typed::HeadingFont, style.fonts.heading.clone(), l.text("Koppen", "Headings")));
        if let Some(colour) = view.colour.as_deref().and_then(|key| style.colours.iter().find(|c| c.key == key)) {
            fields = fields.push(caption(l.text("Kleur", "Colour")));
            fields = fields.push(typed(Typed::ColourName, colour.names.get(lang).to_string(), l.text("Naam", "Name")));
            fields = fields.push(typed(Typed::Hex, super::materials::hex(colour.rgb), l.text("Waarde", "Value")));
            if !ROLES.contains(&colour.key.as_str()) {
                let remove = button(text(l.text("Kleur verwijderen", "Remove colour")).size(12).color(p.text_secondary)).padding([3, 8]).style(move |_, s| p.row_button(s)).on_press(HouseMessage::RemoveColour);
                fields = fields.push(remove);
            }
        }
        fields = fields.push(caption(l.text("Maatvoering (mm op papier)", "Dimensions (mm on paper)")));
        let ends = [Named(Terminator::Tick, l.text("streepje", "tick")), Named(Terminator::Arrow, l.text("pijl", "arrow")), Named(Terminator::Dot, l.text("punt", "dot"))];
        let now = ends.iter().find(|n| n.0 == style.dimensions.terminator).cloned();
        let end_name = l.text("Einde van de maatlijn", "End of the dimension line");
        let pick = pick_list(ends.to_vec(), now, |n: Named| HouseMessage::Terminator(n.0)).text_size(12).padding([2, 6]).width(Fill);
        fields = fields.push(row![text(end_name).size(12).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph).width(Length::FillPortion(2)), container(a11y::named(pick, Aria::ComboBox, end_name)).width(Length::FillPortion(3))].spacing(8).align_y(Center));
        let d = &style.dimensions;
        fields = fields.push(typed(Typed::TerminatorSize, mm(d.terminator_size), l.text("Grootte van het einde", "Size of the end")));
        fields = fields.push(typed(Typed::TextHeight, mm(d.text_height), l.text("Teksthoogte", "Text height")));
        fields = fields.push(typed(Typed::Gap, mm(d.gap), l.text("Afstand tot het gemetene", "Gap to what is measured")));
        fields = fields.push(typed(Typed::Overshoot, mm(d.overshoot), l.text("Uitsteek", "Overshoot")));
        fields = fields.push(caption(l.text("Titelblok", "Title block")));
        fields = fields.push(typed(Typed::BlockWidth, mm(style.title_block.width), l.text("Breedte (mm)", "Width (mm)")));
        let refused = view.refused.as_ref().map(|why| text(format!("{} ({why})", l.text("Niet overgenomen.", "Not taken."))).size(11).color(p.accent).wrapping(Wrapping::Word));
        let sample: Element<'_, Message> = match self.sample_sheet() {
            Some(sheet) => {
                let about = format!("{} · {} × {} mm · {}", sheet.key, mm(sheet.width), mm(sheet.height), l.text("voorbeeldblad", "sample sheet"));
                column![canvas_widget(SampleSheet { style: style.clone(), sheet: sheet.clone(), ink: p.text, labels: true }).width(Fill).height(200.0), text(about).size(11).color(p.text_muted)].spacing(2).into()
            }
            None => Space::new().into(),
        };
        let pane = column![text(style.company.name.clone()).size(14).font(FONT_BOLD).color(p.text).wrapping(Wrapping::Word), sample, Element::from(fields.push(refused)).map(Message::House)].spacing(8);
        container(pane).padding(8).into()
    }
}

/// A terminator with its name, to pick.
#[derive(Debug, Clone, PartialEq)]
struct Named(Terminator, &'static str);

impl std::fmt::Display for Named {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.1)
    }
}

/// A sheet of a house style, drawn to fit: the paper, the frame of its drawing area, and the
/// title block in the lower right corner with its cells; with `labels`, the office's name and
/// the labels of the fields.
struct SampleSheet {
    style: HouseStyle,
    sheet: SheetFormat,
    ink: Color,
    labels: bool,
}

impl SampleSheet {
    /// Where the paper lies in `bounds` and how many pixels a millimetre is.
    fn fit(&self, bounds: Size) -> (Point, f32) {
        let scale = ((bounds.width - 2.0) / self.sheet.width as f32).min((bounds.height - 2.0) / self.sheet.height as f32).max(0.0);
        let origin = Point::new((bounds.width - self.sheet.width as f32 * scale) / 2.0, (bounds.height - self.sheet.height as f32 * scale) / 2.0);
        (origin, scale)
    }
}

impl<M> canvas::Program<M> for SampleSheet {
    type State = ();

    fn draw(&self, _state: &(), renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (origin, scale) = self.fit(bounds.size());
        let at = |x: f64, y: f64| Point::new(origin.x + x as f32 * scale, origin.y + y as f32 * scale);
        let size = |w: f64, h: f64| Size::new(w as f32 * scale, h as f32 * scale);
        let sheet = &self.sheet;
        let colour = |key: &str, otherwise: Color| self.style.colour(key).map_or(otherwise, rgb);
        let thin = Stroke::default().with_color(self.ink).with_width(0.6);
        // the paper and the frame of its drawing area
        frame.fill_rectangle(at(0.0, 0.0), size(sheet.width, sheet.height), colour("background", Color::WHITE));
        frame.stroke(&Path::rectangle(at(0.0, 0.0), size(sheet.width, sheet.height)), thin);
        let [area_w, area_h] = sheet.drawing_area();
        let (left, top) = (sheet.margins[0], sheet.margins[1]);
        frame.stroke(&Path::rectangle(at(left, top), size(area_w, area_h)), Stroke::default().with_color(self.ink).with_width(1.0));
        // the title block in the lower right corner of the drawing area
        let block = &self.style.title_block;
        let (bx, by) = (left + area_w - block.width, top + area_h - block.height());
        for (field, [x, y, w, h]) in block.cells() {
            let corner = at(bx + x, by + y);
            match field {
                Field::Logo => frame.fill_rectangle(corner, size(w, h), colour("primary", self.ink)),
                Field::Company => frame.fill_rectangle(corner, size(w, h), colour("secondary", self.ink)),
                _ => {}
            }
            frame.stroke(&Path::rectangle(corner, size(w, h)), thin);
            // a label is written where a millimetre is large enough to read it
            if self.labels && scale > 0.9 {
                if let Some((_, en)) = field.label() {
                    frame.fill_text(Text { content: en.to_string(), position: Point::new(corner.x + 1.5, corner.y + 1.0), color: colour("muted", self.ink), size: (2.2 * scale).into(), ..Text::default() });
                }
            }
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::super::libraries::{LibrariesMessage, Source};
    use super::*;

    #[test]
    fn what_is_typed_is_taken_when_the_style_still_holds_together() {
        let mut app = App::default();
        let lang = Lang::En;
        let _ = app.house_update(HouseMessage::Typed(Typed::Name, "3BM Bouwkunde".into(), lang));
        let _ = app.house_update(HouseMessage::Typed(Typed::City, "Dordrecht".into(), lang));
        assert_eq!((app.house.company.name.as_str(), app.house.company.city.as_str()), ("3BM Bouwkunde", "Dordrecht"));
        // a name cannot be empty: what is typed stays in its field, the style as it was
        let _ = app.house_update(HouseMessage::Typed(Typed::Name, "".into(), lang));
        assert_eq!((app.house.company.name.as_str(), app.house_view.draft.clone(), app.house_view.refused.is_some()), ("3BM Bouwkunde", Some((Typed::Name, String::new())), true));
        let _ = app.house_update(HouseMessage::Typed(Typed::Name, "3BM".into(), lang));
        assert_eq!((app.house.company.name.as_str(), app.house_view.draft.clone(), app.house_view.refused.clone()), ("3BM", None, None));
        // sizes are numbers; a title block wider than the smallest sheet is refused
        let _ = app.house_update(HouseMessage::Typed(Typed::TextHeight, "3,5".into(), lang));
        assert_eq!(app.house.dimensions.text_height, 3.5);
        let _ = app.house_update(HouseMessage::Typed(Typed::TextHeight, "hoog".into(), lang));
        let _ = app.house_update(HouseMessage::Typed(Typed::BlockWidth, "400".into(), lang));
        assert_eq!((app.house.dimensions.text_height, app.house.title_block.width, app.house_view.refused.is_some()), (3.5, 180.0, true));
        let _ = app.house_update(HouseMessage::Terminator(Terminator::Arrow));
        assert_eq!(app.house.dimensions.terminator, Terminator::Arrow);
    }

    #[test]
    fn a_colour_is_set_added_and_removed() {
        let mut app = App::default();
        let _ = app.house_update(HouseMessage::Colour("primary".into()));
        let _ = app.house_update(HouseMessage::Typed(Typed::Hex, "#a0522d".into(), Lang::Nl));
        let _ = app.house_update(HouseMessage::Typed(Typed::ColourName, "huiskleur".into(), Lang::Nl));
        let primary = app.house.colours.iter().find(|c| c.key == "primary").unwrap();
        assert_eq!((primary.rgb, primary.names.nl.as_str(), primary.names.en.as_str()), ([160, 82, 45], "huiskleur", "primary"));
        // what is no colour is not taken
        let _ = app.house_update(HouseMessage::Typed(Typed::Hex, "#a05".into(), Lang::Nl));
        assert_eq!(app.house.colour("primary"), Some([160, 82, 45]));
        // a role is not removed; a colour of the office's own is
        let colours = app.house.colours.len();
        let _ = app.house_update(HouseMessage::RemoveColour);
        assert_eq!(app.house.colours.len(), colours);
        let _ = app.house_update(HouseMessage::AddColour);
        assert_eq!((app.house.colours.len(), app.house_view.colour.as_deref()), (colours + 1, Some("own-1")));
        let _ = app.house_update(HouseMessage::RemoveColour);
        assert_eq!((app.house.colours.len(), app.house_view.colour.clone()), (colours, None));
    }

    #[test]
    fn the_company_page_shows_the_house_style_and_its_pane_sets_it() {
        let mut app = App::default();
        let _ = app.libraries_update(LibrariesMessage::Source(Source::Company));
        let _ = app.house_update(HouseMessage::Sheet("A1".into()));
        assert_eq!(app.sample_sheet().map(|s| s.key.as_str()), Some("A1"));
        assert_eq!((app.house_colour_tiles(app.theme.palette(), Language::English).len(), app.house_sheet_tiles(app.theme.palette()).len()), (7, 5));
        let mut pane = crate::app::test_ui_of(iced::Size::new(340.0, 1200.0), app.house_pane(app.theme.palette(), Language::English));
        assert!(pane.find("OpenAEC").is_ok() && pane.find("Trade register").is_ok() && pane.find("End of the dimension line").is_ok());
        assert!(pane.find("A1 · 841 × 594 mm · sample sheet").is_ok());
    }

    #[test]
    fn the_house_style_is_kept_with_the_project_and_taken_back() {
        let mut app = App::default();
        app.new_model("Level 0");
        let _ = app.update(Message::House(HouseMessage::Typed(Typed::Name, "3BM".into(), Lang::Nl)));
        // the file of the project holds it as a node, and a project that opens it takes it over
        let shown = app.modeller.shown.and_then(|id| app.models.iter().find(|e| e.model.id == id)).expect("the model shows").model.clone();
        let node = shown.project.types.iter().find(|n| n["attributes"].get(bim_library::house::HOUSE_STYLE).is_some()).expect("the house style's node");
        let mut other = App::default();
        other.adopt_house(&node["attributes"][bim_library::house::HOUSE_STYLE]);
        assert_eq!(other.house, app.house);
        // a change of it is a step to take back
        let _ = app.update(Message::Undo);
        assert_eq!(app.house.company.name, "OpenAEC");
        let _ = app.update(Message::Redo);
        assert_eq!(app.house.company.name, "3BM");
    }
}
