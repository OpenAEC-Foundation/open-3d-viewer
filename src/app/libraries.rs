//! Library, the product: the libraries a project draws on, side by side.
//!
//! Four sources, chosen in the left pane or on the ribbon: the general library every project
//! starts with, the content of a country (its drawing standard and its types), the house style of
//! a company, and the library of the project that is open. The page lists what the source holds
//! in 2D and in 3D as cards: how many there are and a first look at them. What the suite does not
//! hold yet is named and marked "to come", so the page shows where each thing will live.
//!
//! A first setup (see `docs/superpowers/specs/2026-11-18-library-design.md`): the cards show,
//! they do not open yet.

use super::{App, Message, FONT_BOLD};
use crate::theme::Palette;
use bim_library::profile::steel_profiles;
use bim_library::{DrawingStandard, HatchKind, Library, LinearRole, Role, Series, SystemGroup};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, row, scrollable, text, Column, Space};
use iced::{Center, Color, Element, Fill, Task};
use openaec_ui::Language;
use std::sync::OnceLock;

/// Where library content comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Source {
    /// The library every project starts with.
    #[default]
    General,
    /// The content of a country: its drawing standard and its types.
    Country,
    /// The house style of a company.
    Company,
    /// The library of the open project, a library file or a component.
    Project,
}

impl Source {
    pub const ALL: [Source; 4] = [Source::General, Source::Country, Source::Company, Source::Project];

    pub fn label(self, l: Language) -> &'static str {
        match self {
            Source::General => l.text("Algemeen", "General"),
            Source::Country => l.text("Land", "Country"),
            Source::Company => l.text("Bedrijf", "Company"),
            Source::Project => l.text("Project", "Project"),
        }
    }

    /// What it holds, in a line.
    pub fn purpose(self, l: Language) -> &'static str {
        match self {
            Source::General => l.text(
                "De algemene bibliotheek: waar elk project mee begint, in elk land en bij elk bedrijf.",
                "The general library: what every project starts with, in any country and at any company.",
            ),
            Source::Country => l.text(
                "De inhoud van een land: de tekennorm met haar arceringen, en de types, profielen en producten die daar gebruikelijk zijn.",
                "The content of a country: its drawing standard with its hatches, and the types, profiles and products that are usual there.",
            ),
            Source::Company => l.text(
                "De huisstijl van een bedrijf: logo, kleuren en lettertypen, het titelblok en de sjablonen van zijn tekeningen, en zijn eigen types.",
                "The house style of a company: logo, colours and fonts, the title block and templates of its drawings, and its own types.",
            ),
            Source::Project => l.text(
                "De bibliotheek van het open project: wat het tabblad Bibliotheek van de Modeller toont. Een bibliotheekbestand of een onderdeel dat je opent, staat hier ook.",
                "The library of the open project: what the Modeller's Library tab lists. A library file or a component you open shows here too.",
            ),
        }
    }

    /// Its command on the ribbon.
    pub fn command(self) -> &'static str {
        match self {
            Source::General => "library.general",
            Source::Country => "library.country",
            Source::Company => "library.company",
            Source::Project => "library.project",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Source::General => "construction.library",
            Source::Country => "draw.coordinate",
            Source::Company => "theme.light",
            Source::Project => "project.folder",
        }
    }
}

/// A country whose content the suite holds: so far, its drawing standard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Country {
    #[default]
    Netherlands,
    Germany,
    UnitedKingdom,
    UnitedStates,
}

impl Country {
    pub const ALL: [Country; 4] = [Country::Netherlands, Country::Germany, Country::UnitedKingdom, Country::UnitedStates];

    pub fn label(self, l: Language) -> &'static str {
        match self {
            Country::Netherlands => l.text("Nederland", "Netherlands"),
            Country::Germany => l.text("Duitsland", "Germany"),
            Country::UnitedKingdom => l.text("Verenigd Koninkrijk", "United Kingdom"),
            Country::UnitedStates => l.text("Verenigde Staten", "United States"),
        }
    }

    /// The drawing standard its hatches follow.
    pub fn standard(self) -> DrawingStandard {
        match self {
            Country::Netherlands => DrawingStandard::Nen47,
            Country::Germany => DrawingStandard::Din1356,
            Country::UnitedKingdom => DrawingStandard::Iso128,
            Country::UnitedStates => DrawingStandard::Ansi,
        }
    }

    /// That standard's name, as it is written everywhere.
    pub fn standard_name(self) -> &'static str {
        match self {
            Country::Netherlands => "NEN 47",
            Country::Germany => "DIN 1356",
            Country::UnitedKingdom => "ISO 128 / BS 1192",
            Country::UnitedStates => "ANSI",
        }
    }

    /// Whether the steel table of the suite (European sections) is the one used there.
    fn european_steel(self) -> bool {
        matches!(self, Country::Netherlands | Country::Germany)
    }
}

/// The Library product's state: the source and the country shown.
#[derive(Debug, Clone, Copy, Default)]
pub struct LibrariesView {
    pub source: Source,
    pub country: Country,
}

#[derive(Debug, Clone, Copy)]
pub enum LibrariesMessage {
    Source(Source),
    /// Show this country's content (and so the country source).
    Country(Country),
}

/// What a card shows of its content.
#[derive(Debug, Clone, PartialEq)]
pub enum Sample {
    /// Colours: of materials, hatches or a house style.
    Swatches(Vec<[u8; 3]>),
    /// The first names.
    Names(Vec<String>),
}

/// A kind of content of a library: how many of it there are and a first look, or nothing yet.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub title: &'static str,
    /// A word beside the title: the standard hatches follow, for one.
    pub note: &'static str,
    /// What the suite holds of it; `None` while it is to come.
    pub content: Option<(usize, Sample)>,
}

impl Card {
    fn of(title: &'static str, count: usize, sample: Sample) -> Self {
        Card { title, note: "", content: Some((count, sample)) }
    }

    fn to_come(title: &'static str) -> Self {
        Card { title, note: "", content: None }
    }

    fn noted(mut self, note: &'static str) -> Self {
        self.note = note;
        self
    }
}

/// The library every project starts with.
fn general() -> &'static Library {
    static GENERAL: OnceLock<Library> = OnceLock::new();
    GENERAL.get_or_init(Library::starter)
}

/// The Dutch types of the INB template.
fn inb() -> &'static Library {
    static INB: OnceLock<Library> = OnceLock::new();
    INB.get_or_init(bim_library::inb::library)
}

/// The hatches of a drawing standard: one per kind of material.
fn hatches(l: Language, standard: DrawingStandard) -> Card {
    let fills = HatchKind::ALL.iter().map(|kind| kind.pattern(standard).fill).collect();
    Card::of(l.text("Arceringen", "Hatches"), HatchKind::ALL.len(), Sample::Swatches(fills))
}

/// The steel table: its profiles, by the names of its series.
fn steel(l: Language) -> Card {
    let series = Series::ALL.iter().map(|s| s.name().to_string()).collect();
    Card::of(l.text("Staalprofielen", "Steel profiles"), steel_profiles().len(), Sample::Names(series))
}

/// The types of a library in 3D: its materials, its build-ups by what they are for, its linear
/// types and its system types.
fn types(library: &Library, l: Language) -> Vec<Card> {
    let lang = super::library::lang(l);
    let names = |list: Vec<(&str, String)>| -> Vec<String> { list.into_iter().map(|(_, name)| name).collect() };
    let mut cards = vec![Card::of(l.text("Materialen", "Materials"), library.materials.len(), Sample::Swatches(library.materials.iter().map(|m| m.colour).collect()))];
    for (role, title) in [(Role::Wall, l.text("Wanden", "Walls")), (Role::Floor, l.text("Vloeren", "Floors")), (Role::Roof, l.text("Daken", "Roofs")), (Role::Ceiling, l.text("Plafonds", "Ceilings"))] {
        let of_role = names(library.names(role, lang));
        cards.push(Card::of(title, of_role.len(), Sample::Names(of_role)));
    }
    let linear: Vec<String> = LinearRole::ALL.iter().flat_map(|&role| names(library.linear_names(role, lang))).collect();
    cards.push(Card::of(l.text("Lijntypen", "Linear types"), linear.len(), Sample::Names(linear)));
    let systems: Vec<String> = SystemGroup::ALL.iter().flat_map(|&group| names(library.system_names(group, lang))).collect();
    cards.push(Card::of(l.text("Systemen", "Systems"), systems.len(), Sample::Names(systems)));
    cards
}

/// The content of 2D drawings that no library holds yet.
fn drawing_styles(l: Language) -> Vec<Card> {
    vec![
        Card::to_come(l.text("Lijnstijlen", "Line styles")),
        Card::to_come(l.text("Tekst- en maatstijlen", "Text and dimension styles")),
        Card::to_come(l.text("Symbolen", "Symbols")),
    ]
}

impl App {
    pub(super) fn libraries_update(&mut self, message: LibrariesMessage) -> Task<Message> {
        match message {
            LibrariesMessage::Source(source) => self.libraries.source = source,
            LibrariesMessage::Country(country) => self.libraries = LibrariesView { source: Source::Country, country },
        }
        Task::none()
    }

    /// What the source shown holds: its sections, each with its cards. `house` are the colours
    /// of the house style shown for a company.
    pub fn library_sections(&self, l: Language, house: &[[u8; 3]]) -> Vec<(&'static str, Vec<Card>)> {
        let view = self.libraries;
        match view.source {
            Source::General => {
                let mut flat = vec![hatches(l, DrawingStandard::Iso128).noted("ISO 128")];
                flat.extend(drawing_styles(l));
                let mut solid = types(general(), l);
                solid.push(steel(l));
                solid.push(Card::to_come(l.text("Objecten", "Objects")));
                vec![("2D", flat), ("3D", solid)]
            }
            Source::Country => {
                let country = view.country;
                let mut flat = vec![hatches(l, country.standard()).noted(country.standard_name()), Card::to_come(l.text("Titelblokken", "Title blocks"))];
                flat.extend(drawing_styles(l));
                let mut solid = Vec::new();
                if country == Country::Netherlands {
                    // the Dutch template's types: its walls, floors, roofs, columns and beams
                    solid.extend(types(inb(), l).into_iter().filter(|card| card.content.as_ref().is_some_and(|(count, _)| *count > 0)));
                }
                solid.push(if country.european_steel() { steel(l) } else { Card::to_come(l.text("Staalprofielen", "Steel profiles")) });
                solid.push(Card::to_come(l.text("Producten van fabrikanten", "Products of manufacturers")));
                vec![("2D", flat), ("3D", solid)]
            }
            Source::Company => vec![
                (
                    l.text("Huisstijl", "House style"),
                    vec![
                        Card::to_come("Logo"),
                        Card::of(l.text("Kleuren", "Colours"), house.len(), Sample::Swatches(house.to_vec())),
                        Card::of(l.text("Lettertypen", "Fonts"), 1, Sample::Names(vec!["Inter".to_string()])),
                    ],
                ),
                ("2D", vec![Card::to_come(l.text("Titelblok", "Title block")), Card::to_come(l.text("Bladsjablonen", "Sheet templates")), Card::to_come(l.text("Lijndiktes", "Line weights"))]),
                ("3D", vec![Card::to_come(l.text("Eigen types", "Own types"))]),
            ],
            Source::Project => {
                // a project's hatches follow the Dutch standard, until a project can choose
                let mut flat = vec![hatches(l, DrawingStandard::Nen47).noted(Country::Netherlands.standard_name())];
                flat.extend(drawing_styles(l));
                vec![("2D", flat), ("3D", types(&self.library, l))]
            }
        }
    }

    /// The left pane of the Library product: the sources, and under Country its countries.
    pub fn libraries_pane(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let view = self.libraries;
        let line = |label: &'static str, on: bool, indent: f32, message: LibrariesMessage| -> Element<'_, LibrariesMessage> {
            let mark = text(if on { "●" } else { "" }).size(8).color(p.accent).width(12);
            let name = text(label).size(12).font(if on { FONT_BOLD } else { super::FONT }).color(if on { p.text } else { p.text_secondary });
            button(row![Space::new().width(indent), mark, name].spacing(4).align_y(Center)).width(Fill).padding([4, 6]).style(move |_, s| p.row_button(s)).on_press(message).into()
        };
        let mut list = Column::new().spacing(1);
        for source in Source::ALL {
            list = list.push(line(source.label(l), view.source == source, 0.0, LibrariesMessage::Source(source)));
            if source == Source::Country && view.source == Source::Country {
                for country in Country::ALL {
                    list = list.push(line(country.label(l), view.country == country, 14.0, LibrariesMessage::Country(country)));
                }
            }
        }
        scrollable(Element::from(list).map(Message::Libraries)).height(Fill).into()
    }

    /// The Library page: what the source shown holds, in 2D and in 3D. Nothing stands over it
    /// but its own heading.
    pub fn libraries_page(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let view = self.libraries;
        let title = match view.source {
            Source::Country => format!("{} · {}", view.source.label(l), view.country.label(l)),
            Source::Project => match self.model.as_ref().and_then(|m| m.path.file_name()).and_then(|n| n.to_str()) {
                Some(file) => format!("{} · {file}", view.source.label(l)),
                None => view.source.label(l).to_string(),
            },
            source => source.label(l).to_string(),
        };
        let heading = column![
            text(title).size(20).font(FONT_BOLD).color(p.text),
            text(view.source.purpose(l)).size(12).color(p.text_secondary).width(Fill).wrapping(Wrapping::Word),
        ]
        .spacing(4);
        let rgb = |c: Color| [(c.r * 255.0).round() as u8, (c.g * 255.0).round() as u8, (c.b * 255.0).round() as u8];
        let house = [p.accent, p.text, p.text_secondary, p.border, p.surface, p.bg].map(rgb);
        let mut page = Column::new().spacing(18).push(heading);
        for (section, cards) in self.library_sections(l, &house) {
            let cards = row(cards.into_iter().map(|card| card_view(card, p, l))).spacing(10).wrap();
            page = page.push(column![text(section).size(12).font(FONT_BOLD).color(p.text_muted), cards].spacing(8));
        }
        container(scrollable(container(page).padding(20).width(Fill)).height(Fill)).width(Fill).height(Fill).style(move |_| p.panel()).into()
    }
}

/// A card: its title and count, and the colours or the first names of what it holds.
fn card_view<'a>(card: Card, p: Palette, l: Language) -> Element<'a, Message> {
    const SHOWN: usize = 5;
    let ready = card.content.is_some();
    let ink = if ready { p.text } else { p.text_muted };
    let mut head = row![text(card.title).size(13).font(FONT_BOLD).color(ink)].spacing(6).align_y(Center);
    if !card.note.is_empty() {
        head = head.push(text(card.note).size(10).color(p.text_muted));
    }
    head = head.push(Space::new().width(Fill));
    let body: Element<'a, Message> = match card.content {
        None => {
            head = head.push(text(l.text("volgt", "to come")).size(11).color(p.text_muted));
            Space::new().height(18).into()
        }
        Some((count, sample)) => {
            head = head.push(text(count.to_string()).size(12).color(p.text_secondary));
            match sample {
                Sample::Swatches(colours) => row(colours.into_iter().take(12).map(|[r, g, b]| {
                    container(Space::new().width(14).height(18))
                        .style(move |_| container::Style { background: Some(Color::from_rgb8(r, g, b).into()), border: iced::Border { color: p.border, width: 1.0, radius: 2.0.into() }, ..Default::default() })
                        .into()
                }))
                .spacing(3)
                .into(),
                Sample::Names(names) => {
                    let more = names.len().saturating_sub(SHOWN);
                    let mut lines = Column::new().spacing(2);
                    for name in names.into_iter().take(SHOWN) {
                        lines = lines.push(text(name).size(11).color(p.text_secondary).wrapping(Wrapping::None));
                    }
                    if more > 0 {
                        lines = lines.push(text(format!("+{more}")).size(11).color(p.text_muted));
                    }
                    lines.into()
                }
            }
        }
    };
    container(column![head, body].spacing(8))
        .width(250)
        .padding(12)
        .clip(true)
        .style(move |_| container::Style { background: Some(p.surface.into()), border: iced::Border { color: p.border, width: 1.0, radius: 6.0.into() }, ..Default::default() })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(cards: &[Card]) -> Vec<&str> {
        cards.iter().map(|card| card.title).collect()
    }

    #[test]
    fn each_source_lists_what_it_holds_in_2d_and_in_3d() {
        let mut app = App::default();
        let l = Language::English;
        // the general library: the hatches and the starter types; what is not there yet is named
        let general = app.library_sections(l, &[]);
        assert_eq!(general.iter().map(|(name, _)| *name).collect::<Vec<_>>(), ["2D", "3D"]);
        assert_eq!(titles(&general[0].1), ["Hatches", "Line styles", "Text and dimension styles", "Symbols"]);
        let materials = &general[1].1[0];
        assert_eq!((materials.title, materials.content.as_ref().map(|(count, _)| *count)), ("Materials", Some(general_library_materials())));
        assert!(general[1].1.iter().any(|card| card.title == "Steel profiles" && card.content.is_some()));
        assert!(general[1].1.iter().any(|card| card.title == "Objects" && card.content.is_none()));
        // a country: its own standard's hatches, and for the Netherlands the types of its template
        let _ = app.libraries_update(LibrariesMessage::Country(Country::Germany));
        assert_eq!((app.libraries.source, app.libraries.country), (Source::Country, Country::Germany));
        let germany = app.library_sections(l, &[]);
        assert_eq!(germany[0].1[0].note, "DIN 1356");
        let _ = app.libraries_update(LibrariesMessage::Country(Country::Netherlands));
        let netherlands = app.library_sections(l, &[]);
        assert_eq!(netherlands[0].1[0].note, "NEN 47");
        assert_ne!(netherlands[0].1[0].content, germany[0].1[0].content, "the hatches differ by standard");
        assert!(netherlands[1].1.len() > germany[1].1.len(), "the Dutch template's types");
        // a company: its house style first; the project: its own library
        let _ = app.libraries_update(LibrariesMessage::Source(Source::Company));
        let company = app.library_sections(l, &[[217, 119, 6]]);
        assert_eq!(company[0].0, "House style");
        assert_eq!(company[0].1[1].content, Some((1, Sample::Swatches(vec![[217, 119, 6]]))));
        let _ = app.libraries_update(LibrariesMessage::Source(Source::Project));
        app.library.materials.truncate(2);
        assert_eq!(app.library_sections(l, &[])[1].1[0].content.as_ref().map(|(count, _)| *count), Some(2));
    }

    fn general_library_materials() -> usize {
        Library::starter().materials.len()
    }

    #[test]
    fn the_page_and_the_pane_show_the_source_picked() {
        let mut app = App::default();
        let _ = app.libraries_update(LibrariesMessage::Country(Country::Germany));
        {
            let mut pane = crate::app::test_ui_of(iced::Size::new(260.0, 600.0), app.libraries_pane(app.theme.palette(), Language::English));
            // the countries stand under Country while it shows
            assert!(pane.find("Germany").is_ok() && pane.find("Company").is_ok());
        }
        let mut page = crate::app::test_ui_of(iced::Size::new(1200.0, 800.0), app.libraries_page(app.theme.palette(), Language::English));
        assert!(page.find("Country · Germany").is_ok());
        assert!(page.find("DIN 1356").is_ok() && page.find("Title blocks").is_ok());
    }
}
