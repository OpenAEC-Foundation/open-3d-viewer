//! Library, the product: the libraries a project draws on, by what they hold.
//!
//! The left pane has a search field, the source (the general library every project starts with,
//! the content of a country, the house style of a company, the library of the open project) and
//! a tree of what that source holds, by the kind of thing: materials; planar elements (walls,
//! floors, roofs, ceilings); linear elements by their role, with the steel table; systems by
//! their group; the drawing standard (kinds of drawing, scales, line styles, line weights, text
//! styles, hatches); a company's house style.
//!
//! The page shows the things of the category picked as tiles, each with a look at it: a
//! material's hatch and colour, a build-up's section, a profile's outline, a system's face, a
//! line as a pen and a line style draw it. A click on a tile of the project opens it in its
//! editor in the right pane. A click on a tile of another library shows it there, with a button
//! that makes a copy of it in the project to edit. The last tile makes a new one.
//!
//! A search looks in every library at once and shows what it finds as the same tiles; a click
//! on one shows it where it lives.

use super::buildups::BuildUpMessage;
use super::materials::MaterialMessage;
use super::profiles::ProfileMessage;
use super::standards::{Setting, StandardMessage};
use super::systems::SystemMessage;
use super::{App, Message, FONT, FONT_BOLD};
use crate::theme::Palette;
use bim_library::profile::steel_profiles;
use bim_library::{DrawingStandard, HatchKind, HatchPattern, Library, LinearRole, Role, SystemGroup};
use iced::mouse;
use iced::widget::canvas::{self, Frame, Path, Stroke};
use iced::widget::text::Wrapping;
use iced::widget::{button, canvas as canvas_widget, column, container, row, scrollable, text, text_input, Column, Space};
use iced::{Center, Color, Element, Fill, Point, Rectangle, Size, Task};
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

    /// What it holds, in a line: what its button on the ribbon says.
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

/// A kind of thing a library holds: what the tree lists and the page shows the tiles of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Materials,
    /// Planar elements, by what they are for: walls, floors, roofs, ceilings.
    Planar(Role),
    /// Linear elements, by their role: columns, beams and the others.
    Linear(LinearRole),
    /// The steel table linear elements take their profile from.
    Steel,
    /// Systems, by their group.
    Systems(SystemGroup),
    /// The drawing standard: its kinds of drawing and the settings they pick from.
    Kinds,
    Scales,
    LineStyles,
    Pens,
    TextStyles,
    /// The hatches of the drawing standard a source follows.
    Hatches,
    /// A company's house style.
    Colours,
    Fonts,
}

/// What a category stands under in the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Heading {
    Materials,
    Planar,
    Linear,
    Systems,
    Standard,
    House,
}

impl Heading {
    pub fn label(self, l: Language) -> &'static str {
        match self {
            Heading::Materials => l.text("Materialen", "Materials"),
            Heading::Planar => l.text("Vlakvormige elementen", "Planar elements"),
            Heading::Linear => l.text("Staafvormige elementen", "Linear elements"),
            Heading::Systems => l.text("Systemen", "Systems"),
            Heading::Standard => l.text("Tekenstandaard", "Drawing standard"),
            Heading::House => l.text("Huisstijl", "House style"),
        }
    }
}

impl Category {
    pub fn label(self, l: Language) -> &'static str {
        match self {
            Category::Materials => l.text("Materialen", "Materials"),
            Category::Planar(role) => super::buildups::role_title(role, l),
            Category::Linear(role) => match role {
                LinearRole::Column => l.text("Kolommen", "Columns"),
                LinearRole::Beam => l.text("Balken", "Beams"),
                LinearRole::Member => l.text("Staven", "Members"),
                LinearRole::Mullion => l.text("Stijlen", "Mullions"),
                LinearRole::Skirting => l.text("Plinten", "Skirtings"),
                LinearRole::Pipe => l.text("Leidingen", "Pipes"),
                LinearRole::Duct => l.text("Kanalen", "Ducts"),
                LinearRole::CableTray => l.text("Kabelgoten", "Cable trays"),
                LinearRole::Pile => l.text("Palen", "Piles"),
            },
            Category::Steel => l.text("Staalprofielen", "Steel profiles"),
            Category::Systems(group) => super::library::system_group_label(group, l),
            Category::Kinds => l.text("Tekeningsoorten", "Kinds of drawing"),
            Category::Scales => l.text("Schalen", "Scales"),
            Category::LineStyles => l.text("Lijnstijlen", "Line styles"),
            Category::Pens => l.text("Lijndiktes", "Line weights"),
            Category::TextStyles => l.text("Tekststijlen", "Text styles"),
            Category::Hatches => l.text("Arceringen", "Hatches"),
            Category::Colours => l.text("Kleuren", "Colours"),
            Category::Fonts => l.text("Lettertypen", "Fonts"),
        }
    }

    /// Whether it is a part of the drawing standard that is set in the right pane.
    pub fn of_standard(self) -> bool {
        matches!(self, Category::Kinds | Category::Scales | Category::LineStyles | Category::Pens | Category::TextStyles)
    }

    /// The category a setting of the drawing standard is shown in.
    pub fn of_setting(setting: &Setting) -> Category {
        match setting {
            Setting::Kind(_) => Category::Kinds,
            Setting::LineStyle(_) => Category::LineStyles,
            Setting::Pen(_) => Category::Pens,
            Setting::TextStyle(_) => Category::TextStyles,
            Setting::Scale(_) => Category::Scales,
        }
    }
}

/// The Library product's state: the source and the country shown, what is searched for, the
/// category whose tiles show, and what is looked at of a library that is not the project's.
#[derive(Debug, Clone, Default)]
pub struct LibrariesView {
    pub source: Source,
    pub country: Country,
    /// What is typed in the search field; from two letters on the page shows what is found.
    pub query: String,
    /// The category picked in the tree; `None`: the first the source has.
    pub category: Option<Category>,
    /// The tile picked of a library that is not the project's: the right pane shows it.
    pub looked: Option<Item>,
}

#[derive(Debug, Clone)]
pub enum LibrariesMessage {
    Source(Source),
    /// Show this country's content (and so the country source).
    Country(Country),
    /// Typed in the search field.
    Search(String),
    /// A found kind of drawing or setting: show it in its standard, with its fields.
    Open(Setting),
    /// Show the tiles of this category of the source.
    Category(Category),
    /// Look at something of a library that is not the project's, where it lives: from its page
    /// or from a search.
    Look(Item),
}

/// What the right pane of the Project page edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Editor {
    Material,
    BuildUp,
    Profile,
    System,
}

/// A library something was found in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shelf {
    General,
    Netherlands,
    Project,
}

/// What a search finds, and what a tile of the page is of.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Material(Shelf, String),
    BuildUp(Shelf, String),
    Linear(Shelf, String),
    System(Shelf, String),
    /// A profile of the steel table, by its designation.
    Steel(String),
    /// A kind of drawing or a setting of the drawing standard.
    Setting(Setting),
}

/// Something found: its name, what kind of thing it is, where it is from, and the thing itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub name: String,
    pub kind: &'static str,
    pub place: &'static str,
    pub item: Item,
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

/// How wide a tile of the page is, and how high the look at its thing.
pub(super) const TILE: f32 = 184.0;
const LOOK: f32 = 72.0;
/// How high a tile is: its look, up to three lines of its name and a line about it.
const TILE_HEIGHT: f32 = LOOK + 84.0;

/// The frame of a tile: the picked one in the accent colour.
fn tile_frame(p: Palette, on: bool) -> container::Style {
    container::Style { background: Some(p.surface.into()), border: iced::Border { color: if on { p.accent } else { p.border }, width: if on { 2.0 } else { 1.0 }, radius: 6.0.into() }, ..Default::default() }
}

/// A tile of the page: a look at one thing, its name and a line about it; `on` when it is the
/// one picked. A click sends `press`; a tile without one only shows.
pub(super) fn tile<'a>(p: Palette, look: Element<'a, Message>, name: String, about: String, on: bool, press: Option<Message>) -> Element<'a, Message> {
    let body = column![
        container(look).width(Fill).height(LOOK).align_y(Center).clip(true),
        text(name).size(12).font(FONT_BOLD).color(p.text).wrapping(Wrapping::WordOrGlyph),
        text(about).size(10).color(p.text_muted).wrapping(Wrapping::WordOrGlyph),
    ]
    .spacing(4);
    let framed = container(body).width(TILE).height(TILE_HEIGHT).padding(8).clip(true).style(move |_| tile_frame(p, on));
    match press {
        Some(message) => button(framed).padding(0).style(move |_, s| p.row_button(s)).on_press(message).into(),
        None => framed.into(),
    }
}

/// The last tile of a category of the project: it makes one more.
pub(super) fn tile_more<'a>(p: Palette, label: &'static str, message: Message) -> Element<'a, Message> {
    let body = container(text(label).size(13).color(p.accent)).width(TILE).height(TILE_HEIGHT).align_x(Center).align_y(Center).style(move |_| container::Style { border: iced::Border { color: p.border, width: 1.0, radius: 6.0.into() }, ..Default::default() });
    button(body).padding(0).style(move |_, s| p.row_button(s)).on_press(message).into()
}

impl App {
    pub(super) fn libraries_update(&mut self, message: LibrariesMessage) -> Task<Message> {
        match message {
            LibrariesMessage::Source(source) => {
                self.edit_none();
                self.libraries = LibrariesView { source, country: self.libraries.country, ..LibrariesView::default() };
            }
            LibrariesMessage::Country(country) => {
                self.edit_none();
                self.libraries = LibrariesView { source: Source::Country, country, ..LibrariesView::default() };
            }
            LibrariesMessage::Search(query) => self.libraries.query = query,
            LibrariesMessage::Open(setting) => {
                self.edit_none();
                self.libraries = LibrariesView { source: Source::Country, country: Country::Netherlands, category: Some(Category::of_setting(&setting)), ..LibrariesView::default() };
                return self.standard_update(StandardMessage::Pick(setting));
            }
            LibrariesMessage::Category(category) => {
                self.edit_none();
                self.libraries.query.clear();
                self.set_category(category);
            }
            LibrariesMessage::Look(item) => {
                let place = |shelf: Shelf| match shelf {
                    Shelf::General => (Source::General, self.libraries.country),
                    Shelf::Netherlands => (Source::Country, Country::Netherlands),
                    Shelf::Project => (Source::Project, self.libraries.country),
                };
                let found = match &item {
                    Item::Material(shelf, _) => Some((place(*shelf), Category::Materials)),
                    Item::BuildUp(shelf, id) => self.shelf(*shelf).buildup(id).map(|b| (place(*shelf), Category::Planar(b.role))),
                    Item::Linear(shelf, id) => self.shelf(*shelf).linear_type(id).map(|t| (place(*shelf), Category::Linear(t.role))),
                    Item::System(shelf, id) => self.shelf(*shelf).system_type(id).map(|t| (place(*shelf), Category::Systems(t.group))),
                    Item::Steel(_) => Some(((Source::General, self.libraries.country), Category::Steel)),
                    Item::Setting(_) => None,
                };
                if let Some(((source, country), category)) = found {
                    self.edit_none();
                    self.libraries = LibrariesView { source, country, query: String::new(), category: None, looked: None };
                    self.set_category(category);
                    self.libraries.looked = Some(item);
                }
            }
        }
        Task::none()
    }

    /// One editor at a time in the right pane: the others let go of what they had picked.
    pub(super) fn edit_only(&mut self, editor: Editor) {
        if editor != Editor::Material {
            self.material_view.picked = None;
        }
        if editor != Editor::BuildUp {
            self.buildup_view.picked = None;
        }
        if editor != Editor::Profile {
            self.profile_view.picked = None;
        }
        if editor != Editor::System {
            self.system_view.picked = None;
        }
    }

    /// Nothing is picked to edit or to set: another source or another category shows.
    fn edit_none(&mut self) {
        self.material_view.picked = None;
        self.buildup_view.picked = None;
        self.profile_view.picked = None;
        self.system_view.picked = None;
        self.standard_view.picked = None;
        self.follow_refused.clear();
    }

    /// The tiles of `category` show; the editor of its kind of type makes its next one of that
    /// role or group.
    pub(super) fn set_category(&mut self, category: Category) {
        self.libraries.category = Some(category);
        self.libraries.looked = None;
        match category {
            Category::Planar(role) => self.buildup_view.role = Some(role),
            Category::Linear(role) => self.profile_view.role = Some(role),
            Category::Systems(group) => self.system_view.group = Some(group),
            _ => {}
        }
    }

    /// The project's library shows, at `category`, with nothing searched for: where what is
    /// picked to edit lives.
    pub(super) fn show_in_project(&mut self, category: Category) {
        self.libraries = LibrariesView { source: Source::Project, country: self.libraries.country, ..LibrariesView::default() };
        self.set_category(category);
    }

    /// After a step of the history put the library or the drawing standard back as it was: what
    /// was being typed in an editor is forgotten (its fields show what is there now), and an
    /// editor lets go of what is no longer there.
    pub(super) fn settle_library_editors(&mut self) {
        let library = &self.library;
        let material = &mut self.material_view;
        (material.name, material.hex, material.in_use) = (None, None, None);
        material.picked = material.picked.take().filter(|key| library.material(key).is_some());
        let buildup = &mut self.buildup_view;
        (buildup.draft, buildup.refused) = (None, None);
        buildup.picked = buildup.picked.take().filter(|id| library.buildup(id).is_some());
        let profile = &mut self.profile_view;
        (profile.draft, profile.refused) = (None, None);
        profile.picked = profile.picked.take().filter(|id| library.linear_type(id).is_some());
        let system = &mut self.system_view;
        (system.draft, system.refused) = (None, None);
        system.picked = system.picked.take().filter(|id| library.system_type(id).is_some());
        // (a layer that is gone shows no fields: the face does)
        let layers = system.picked.as_deref().and_then(|id| library.system_type(id)).map_or(0, |t| t.layers.len());
        if matches!(system.part, super::systems::Part::Layer(k) if k >= layers) {
            system.part = super::systems::Part::Face;
        }
        let standard = &mut self.standard_view;
        (standard.draft, standard.refused) = (None, None);
        self.follow_refused.clear();
    }

    /// The library of a shelf.
    pub(super) fn shelf(&self, shelf: Shelf) -> &Library {
        match shelf {
            Shelf::General => general(),
            Shelf::Netherlands => inb(),
            Shelf::Project => &self.library,
        }
    }

    /// The shelf of types the source shown has, if it has one: the general library, the Dutch
    /// template's, or the project's.
    fn shown_shelf(&self) -> Option<Shelf> {
        match (self.libraries.source, self.libraries.country) {
            (Source::General, _) => Some(Shelf::General),
            (Source::Country, Country::Netherlands) => Some(Shelf::Netherlands),
            (Source::Project, _) => Some(Shelf::Project),
            _ => None,
        }
    }

    /// The drawing standard the hatches of the source shown follow, and its name.
    fn shown_hatches(&self) -> (DrawingStandard, &'static str) {
        match self.libraries.source {
            Source::Country => (self.libraries.country.standard(), self.libraries.country.standard_name()),
            // a project's hatches follow the Dutch standard, until a project can choose
            Source::Project => (DrawingStandard::Nen47, Country::Netherlands.standard_name()),
            _ => (DrawingStandard::Iso128, "ISO 128"),
        }
    }

    /// How many things the source shown has of `category`.
    pub fn category_count(&self, category: Category) -> usize {
        let library = self.shown_shelf().map(|shelf| self.shelf(shelf));
        let of = |count: &dyn Fn(&Library) -> usize| library.map_or(0, count);
        match category {
            Category::Materials => of(&|lib| lib.materials.len()),
            Category::Planar(role) => of(&|lib| lib.buildups_of(role).count()),
            Category::Linear(role) => of(&|lib| lib.linear_types_of(role).count()),
            Category::Steel => steel_profiles().len(),
            Category::Systems(group) => of(&|lib| lib.system_types_of(group).count()),
            Category::Kinds => self.standard.kinds.len(),
            Category::Scales => self.standard.scales.len(),
            Category::LineStyles => self.standard.line_styles.len(),
            Category::Pens => self.standard.pens.len(),
            Category::TextStyles => self.standard.text_styles.len(),
            Category::Hatches => HatchKind::ALL.len(),
            Category::Colours => HOUSE_COLOURS,
            Category::Fonts => 1,
        }
    }

    /// What the source shown holds, as the tree lists it: its headings, each with its categories
    /// and how many things each has. A source's own library gives the planar roles and the
    /// system groups in full (a new one can be made in each); of the linear roles the columns
    /// and beams, and the others that have a type.
    pub fn library_tree(&self) -> Vec<(Heading, Vec<(Category, usize)>)> {
        let view = &self.libraries;
        let counted = |categories: Vec<Category>| -> Vec<(Category, usize)> { categories.into_iter().map(|category| (category, self.category_count(category))).collect() };
        let standard = || vec![Category::Kinds, Category::Scales, Category::LineStyles, Category::Pens, Category::TextStyles, Category::Hatches];
        // the types of a library: all of the project's and the general library's kinds, of
        // another's only those it has
        let types = |whole: bool| -> Vec<(Heading, Vec<(Category, usize)>)> {
            let has = |list: Vec<(Category, usize)>| -> Vec<(Category, usize)> { list.into_iter().filter(|(category, count)| whole && !matches!(category, Category::Linear(role) if !matches!(role, LinearRole::Column | LinearRole::Beam)) || *count > 0).collect() };
            vec![
                (Heading::Materials, has(counted(vec![Category::Materials]))),
                (Heading::Planar, has(counted(Role::ALL.map(Category::Planar).to_vec()))),
                (Heading::Linear, has(counted(LinearRole::ALL.map(Category::Linear).to_vec()))),
                (Heading::Systems, has(counted(SystemGroup::ALL.map(Category::Systems).to_vec()))),
            ]
        };
        let steel = |tree: &mut Vec<(Heading, Vec<(Category, usize)>)>| {
            if let Some((_, linear)) = tree.iter_mut().find(|(heading, _)| *heading == Heading::Linear) {
                linear.extend(counted(vec![Category::Steel]));
            }
        };
        let mut tree = match view.source {
            Source::General => {
                let mut tree = types(true);
                steel(&mut tree);
                tree.push((Heading::Standard, counted(vec![Category::Hatches])));
                tree
            }
            Source::Country if view.country == Country::Netherlands => {
                let mut tree = vec![(Heading::Standard, counted(standard()))];
                tree.extend(types(false));
                steel(&mut tree);
                tree
            }
            Source::Country => {
                let mut tree = vec![(Heading::Standard, counted(vec![Category::Hatches]))];
                if view.country.european_steel() {
                    tree.push((Heading::Linear, counted(vec![Category::Steel])));
                }
                tree
            }
            Source::Company => vec![(Heading::House, counted(vec![Category::Colours, Category::Fonts]))],
            Source::Project => {
                let mut tree = types(true);
                steel(&mut tree);
                tree.push((Heading::Standard, counted(standard())));
                tree
            }
        };
        tree.retain(|(_, categories)| !categories.is_empty());
        tree
    }

    /// The category whose tiles show: the one picked in the tree while the source has it, else
    /// the first the source has.
    pub fn category(&self) -> Category {
        let tree = self.library_tree();
        let has = |category: &Category| tree.iter().any(|(_, categories)| categories.iter().any(|(c, _)| c == category));
        self.libraries.category.filter(has).or_else(|| tree.first().and_then(|(_, categories)| categories.first()).map(|(category, _)| *category)).unwrap_or(Category::Materials)
    }

    /// What `query` finds in every library and in the drawing standard, by name, code or key:
    /// the project's own first, and nothing twice. From two letters on, at most 60.
    pub fn library_search(&self, query: &str, l: Language) -> Vec<Found> {
        let needle = query.trim().to_lowercase();
        if needle.chars().count() < 2 {
            return Vec::new();
        }
        let lang = super::library::lang(l);
        let hit = |texts: &[&str]| texts.iter().any(|text| text.to_lowercase().contains(&needle));
        let mut found: Vec<Found> = Vec::new();
        let add = |found: &mut Vec<Found>, name: String, kind: &'static str, place: &'static str, item: Item| {
            if !found.iter().any(|f| f.kind == kind && f.name == name) {
                found.push(Found { name, kind, place, item });
            }
        };
        for (shelf, place) in [(Shelf::Project, Source::Project.label(l)), (Shelf::General, Source::General.label(l)), (Shelf::Netherlands, Country::Netherlands.label(l))] {
            let library = self.shelf(shelf);
            for material in &library.materials {
                if hit(&[material.name(lang), &material.key]) {
                    add(&mut found, material.name(lang).to_string(), l.text("Materialen", "Materials"), place, Item::Material(shelf, material.key.clone()));
                }
            }
            for role in Role::ALL {
                for (id, name) in library.names(role, lang) {
                    if hit(&[&name, id]) {
                        add(&mut found, name, Category::Planar(role).label(l), place, Item::BuildUp(shelf, id.to_string()));
                    }
                }
            }
            for role in LinearRole::ALL {
                for (id, name) in library.linear_names(role, lang) {
                    if hit(&[&name, id]) {
                        add(&mut found, name, l.text("Staaftypen", "Linear types"), place, Item::Linear(shelf, id.to_string()));
                    }
                }
            }
            for group in SystemGroup::ALL {
                for (id, name) in library.system_names(group, lang) {
                    if hit(&[&name, id]) {
                        add(&mut found, name, l.text("Systemen", "Systems"), place, Item::System(shelf, id.to_string()));
                    }
                }
            }
        }
        let standard = &self.standard;
        let place = Country::Netherlands.label(l);
        for kind in &standard.kinds {
            if hit(&[kind.names.get(lang), &kind.code, &kind.key]) {
                add(&mut found, kind.names.get(lang).to_string(), l.text("Tekeningsoorten", "Kinds of drawing"), place, Item::Setting(Setting::Kind(kind.key.clone())));
            }
        }
        for style in &standard.line_styles {
            if hit(&[style.names.get(lang), &style.key]) {
                add(&mut found, style.names.get(lang).to_string(), l.text("Lijnstijlen", "Line styles"), place, Item::Setting(Setting::LineStyle(style.key.clone())));
            }
        }
        for pen in &standard.pens {
            if hit(&[pen.names.get(lang)]) {
                add(&mut found, pen.names.get(lang).to_string(), l.text("Lijndiktes", "Line weights"), place, Item::Setting(Setting::Pen(pen.key.clone())));
            }
        }
        for steel in steel_profiles() {
            if hit(&[&steel.designation]) {
                add(&mut found, steel.designation.clone(), l.text("Staalprofielen", "Steel profiles"), Source::General.label(l), Item::Steel(steel.designation.clone()));
            }
        }
        found.truncate(60);
        found
    }

    /// The left pane of the Library product: the search field, the sources (and under Country
    /// its countries), and the tree of what the source shown holds.
    pub fn libraries_pane(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let view = &self.libraries;
        let chip = |label: &'static str, on: bool, message: LibrariesMessage| -> Element<'_, LibrariesMessage> {
            button(text(label).size(12).font(if on { FONT_BOLD } else { FONT }).color(if on { p.text } else { p.text_secondary })).padding([3, 8]).style(move |_, s| p.ribbon_button(on, s)).on_press(message).into()
        };
        // the search field is the pane's own: it looks in every library at once
        let name = l.text("Zoeken in de bibliotheken…", "Search the libraries…");
        let search = openaec_ui::design::a11y::named(text_input(name, &view.query).on_input(LibrariesMessage::Search).size(12).padding([4, 8]), openaec_ui::design::a11y::Role::TextField, name);
        let sources = row(Source::ALL.map(|source| chip(source.label(l), view.source == source, LibrariesMessage::Source(source)))).spacing(2).wrap();
        let mut head = Column::new().spacing(6).push(search).push(sources);
        if view.source == Source::Country {
            head = head.push(row(Country::ALL.map(|country| chip(country.label(l), view.country == country, LibrariesMessage::Country(country)))).spacing(2).wrap());
        }
        // the tree: a heading and under it its categories, each with how many things it has
        let shown = self.category();
        let searching = view.query.trim().chars().count() >= 2;
        let mut tree = Column::new().spacing(1);
        for (heading, categories) in self.library_tree() {
            // (a heading over one category of the same name says nothing more)
            let alone = categories.len() == 1 && categories[0].0.label(l) == heading.label(l);
            if !alone {
                tree = tree.push(container(text(heading.label(l)).size(10).font(FONT_BOLD).color(p.text_muted)).padding(iced::Padding { top: 8.0, right: 6.0, bottom: 2.0, left: 6.0 }));
            }
            for (category, count) in categories {
                let on = category == shown && !searching;
                let line = row![
                    text(if on { "●" } else { "" }).size(8).color(p.accent).width(10),
                    text(category.label(l)).size(12).font(if on || alone { FONT_BOLD } else { FONT }).color(if on { p.text } else { p.text_secondary }).width(Fill).wrapping(Wrapping::None),
                    text(count.to_string()).size(11).color(p.text_muted),
                ]
                .spacing(4)
                .align_y(Center);
                let top = if alone { 8.0 } else { 0.0 };
                let leaf = button(line).width(Fill).padding([4, 6]).style(move |_, s| if on { p.ribbon_button(true, s) } else { p.row_button(s) }).on_press(LibrariesMessage::Category(category));
                tree = tree.push(container(leaf).padding(iced::Padding { top, right: 0.0, bottom: 0.0, left: if alone { 0.0 } else { 8.0 } }));
            }
        }
        let later = text(l.text("Volgt nog: titelblokken, symbolen, objecten en producten van fabrikanten.", "Still to come: title blocks, symbols, objects and products of manufacturers.")).size(10).color(p.text_muted).wrapping(Wrapping::Word);
        let tree = tree.push(container(later).padding(iced::Padding { top: 12.0, right: 6.0, bottom: 6.0, left: 6.0 }));
        let tree = container(tree).padding(iced::Padding { top: 0.0, right: 14.0, bottom: 0.0, left: 0.0 });
        Element::from(column![container(head).padding([2, 4]), scrollable(tree).height(Fill)].spacing(6)).map(Message::Libraries)
    }

    /// The right pane of the Library product: the editor of the tile picked of the project, the
    /// fields of the picked setting of the drawing standard, or a look at the tile picked of
    /// another library, with the way to edit a copy of it.
    pub fn library_side(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let view = &self.libraries;
        let hint = |hint: &'static str| -> Element<'_, Message> { container(text(hint).size(12).color(p.text_muted).wrapping(Wrapping::Word)).padding(8).into() };
        if self.category().of_standard() && view.query.trim().chars().count() < 2 {
            return self.standard_pane(p, l);
        }
        if view.source == Source::Project {
            return match () {
                _ if self.material_view.picked.is_some() => self.material_pane(p, l),
                _ if self.profile_view.picked.is_some() => self.profile_pane(p, l),
                _ if self.buildup_view.picked.is_some() => self.buildup_pane(p, l),
                _ if self.system_view.picked.is_some() => self.system_pane(p, l),
                _ => hint(l.text("Klik een tegel om hem te bewerken. De laatste tegel maakt een nieuwe.", "Click a tile to edit it. The last tile makes a new one.")),
            };
        }
        match &view.looked {
            Some(item) => self.look_pane(p, l, item),
            None => hint(l.text("Klik een tegel om hem te bekijken. Bewerken doe je met een kopie in het project.", "Click a tile to look at it. To edit it, make a copy in the project.")),
        }
    }

    /// The right pane for a tile of a library that is not the project's: what it is, drawn and
    /// listed, and a button that makes a copy of it in the project to edit.
    fn look_pane(&self, p: Palette, l: Language, item: &Item) -> Element<'_, Message> {
        use super::library::{buildup_detail, material_detail, profile_detail, system_detail};
        let lang = super::library::lang(l);
        let copy_label = l.text("Bewerk een kopie in het project", "Edit a copy in the project");
        let found: Option<(String, Element<'_, Message>, &'static str, Message)> = match item {
            Item::Material(shelf, key) => self.shelf(*shelf).material(key).map(|m| (m.name(lang).to_string(), material_detail(p, m), copy_label, Message::Material(MaterialMessage::Copy(*shelf, key.clone())))),
            Item::BuildUp(shelf, id) => {
                let library = self.shelf(*shelf);
                library.buildup(id).map(|b| (library.name(id, lang).unwrap_or_default(), buildup_detail(p, l, library, b), copy_label, Message::BuildUp(BuildUpMessage::Copy(*shelf, id.clone()))))
            }
            Item::Linear(shelf, id) => {
                let library = self.shelf(*shelf);
                library.linear_type(id).map(|t| (library.linear_name(id, lang).unwrap_or_default(), profile_detail(p, &t.profile, library.material(&t.material), Some(t.profile.ifc().0)), copy_label, Message::Profile(ProfileMessage::Copy(*shelf, id.clone()))))
            }
            Item::System(shelf, id) => {
                let library = self.shelf(*shelf);
                library.system_type(id).map(|t| (library.system_name(id, lang).unwrap_or_default(), system_detail(p, l, library, t), copy_label, Message::SystemType(SystemMessage::Copy(*shelf, id.clone()))))
            }
            Item::Steel(designation) => bim_library::profile::steel(designation).map(|s| {
                let make = l.text("Maak er een balktype van in het project", "Make a beam type of it in the project");
                (s.designation.clone(), profile_detail(p, &s.shape, general().material("steel"), Some(s.series.standard())), make, Message::Profile(ProfileMessage::FromSteel(designation.clone())))
            }),
            Item::Setting(_) => None,
        };
        let Some((name, detail, label, message)) = found else {
            return container(text(l.text("Dit staat niet meer in de bibliotheek.", "This is no longer in the library.")).size(12).color(p.text_muted)).padding(8).into();
        };
        let copy = button(text(label).size(12)).padding([5, 10]).style(move |_, s| p.primary_button(s)).on_press(message);
        let pane = column![text(name).size(14).font(FONT_BOLD).color(p.text).wrapping(Wrapping::Word), detail, copy].spacing(10);
        scrollable(container(pane).padding(iced::Padding { top: 8.0, right: 14.0, bottom: 8.0, left: 8.0 }).width(Fill)).height(Fill).into()
    }

    /// The tiles of `category` of the source shown. A tile of the project opens in its editor;
    /// one of another library is looked at; the last tile of the project makes one more.
    fn category_tiles(&self, p: Palette, l: Language, category: Category) -> Vec<Element<'_, Message>> {
        use super::library::{buildup_look, material_look, profile_outline, system_look};
        let lang = super::library::lang(l);
        let view = &self.libraries;
        let project = view.source == Source::Project;
        let looked = |item: &Item| view.looked.as_ref() == Some(item);
        let look = |item: Item| Some(Message::Libraries(LibrariesMessage::Look(item)));
        let shelf = self.shown_shelf();
        let library = shelf.map(|shelf| self.shelf(shelf));
        match (category, shelf, library) {
            (Category::Materials, Some(shelf), Some(library)) => {
                let mut tiles: Vec<Element<'_, Message>> = library
                    .materials
                    .iter()
                    .map(|m| {
                        let item = Item::Material(shelf, m.key.clone());
                        let (on, press) = if project { (self.material_view.picked.as_deref() == Some(m.key.as_str()), Some(Message::Material(MaterialMessage::Pick(m.key.clone())))) } else { (looked(&item), look(item)) };
                        tile(p, material_look(p, m), m.name(lang).to_string(), m.key.clone(), on, press)
                    })
                    .collect();
                if project {
                    tiles.push(tile_more(p, l.text("+ Materiaal", "+ Material"), Message::Material(MaterialMessage::Add)));
                }
                tiles
            }
            (Category::Planar(role), Some(shelf), Some(library)) => {
                let mut tiles: Vec<Element<'_, Message>> = library
                    .names(role, lang)
                    .into_iter()
                    .filter_map(|(id, name)| {
                        let b = library.buildup(id)?;
                        let item = Item::BuildUp(shelf, id.to_string());
                        let (on, press) = if project { (self.buildup_view.picked.as_deref() == Some(id), Some(Message::BuildUp(BuildUpMessage::Pick(id.to_string())))) } else { (looked(&item), look(item)) };
                        let about = format!("{} mm · {}: {}", bim_library::millimetres(b.thickness(), lang), l.text("lagen", "layers"), b.layers.len());
                        Some(tile(p, buildup_look(p, library, b, LOOK), name, about, on, press))
                    })
                    .collect();
                if project {
                    tiles.push(tile_more(p, l.text("+ Opbouw", "+ Build-up"), Message::BuildUp(BuildUpMessage::Add)));
                }
                tiles
            }
            (Category::Linear(role), Some(shelf), Some(library)) => {
                let mut tiles: Vec<Element<'_, Message>> = library
                    .linear_names(role, lang)
                    .into_iter()
                    .filter_map(|(id, name)| {
                        let t = library.linear_type(id)?;
                        let item = Item::Linear(shelf, id.to_string());
                        let (on, press) = if project { (self.profile_view.picked.as_deref() == Some(id), Some(Message::Profile(ProfileMessage::Pick(id.to_string())))) } else { (looked(&item), look(item)) };
                        Some(tile(p, profile_outline(p, &t.profile, library.material(&t.material), LOOK - 8.0), name, t.profile.ifc().0.to_string(), on, press))
                    })
                    .collect();
                if project {
                    tiles.push(tile_more(p, l.text("+ Staaftype", "+ Linear type"), Message::Profile(ProfileMessage::Add)));
                }
                tiles
            }
            (Category::Systems(group), Some(shelf), Some(library)) => {
                let mut tiles: Vec<Element<'_, Message>> = library
                    .system_names(group, lang)
                    .into_iter()
                    .filter_map(|(id, name)| {
                        let t = library.system_type(id)?;
                        let item = Item::System(shelf, id.to_string());
                        let (on, press) = if project { (self.system_view.picked.as_deref() == Some(id), Some(Message::SystemType(SystemMessage::Pick(id.to_string())))) } else { (looked(&item), look(item)) };
                        let (interior, exterior) = t.extent();
                        let about = format!("{} mm · {}", bim_library::millimetres(exterior - interior, lang), t.group.ifc_type().0);
                        Some(tile(p, system_look(p, library, t, LOOK), name, about, on, press))
                    })
                    .collect();
                if project {
                    tiles.push(tile_more(p, l.text("+ Systeem", "+ System"), Message::SystemType(SystemMessage::Add)));
                }
                tiles
            }
            (Category::Hatches, _, _) => {
                let (standard, standard_name) = self.shown_hatches();
                HatchKind::ALL
                    .into_iter()
                    .map(|kind| {
                        let name = general().material(super::materials::hatch_material(kind)).map_or(String::new(), |m| m.name(lang).to_string());
                        let look = container(hatch_look(kind.pattern(standard), p, 64.0)).width(Fill).align_x(Center);
                        tile(p, look.into(), name, standard_name.to_string(), false, None)
                    })
                    .collect()
            }
            (Category::Colours, _, _) => {
                let rgb = |c: Color| [(c.r * 255.0).round() as u8, (c.g * 255.0).round() as u8, (c.b * 255.0).round() as u8];
                let house: [(Color, &'static str); HOUSE_COLOURS] = [
                    (p.accent, l.text("accent", "accent")),
                    (p.text, l.text("tekst", "text")),
                    (p.text_secondary, l.text("tweede tekst", "secondary text")),
                    (p.border, l.text("rand", "border")),
                    (p.surface, l.text("vlak", "surface")),
                    (p.bg, l.text("achtergrond", "background")),
                ];
                house
                    .into_iter()
                    .map(|(colour, role)| {
                        let swatch = container(Space::new().width(Fill).height(LOOK - 8.0)).style(move |_| container::Style { background: Some(colour.into()), border: iced::Border { color: p.border, width: 1.0, radius: 3.0.into() }, ..Default::default() });
                        tile(p, swatch.into(), super::materials::hex(rgb(colour)), role.to_string(), false, None)
                    })
                    .collect()
            }
            (Category::Fonts, _, _) => vec![tile(p, container(text("Aa Bb 123").size(26).color(p.text)).width(Fill).align_x(Center).into(), "Inter".to_string(), l.text("het lettertype van de suite", "the suite's font").to_string(), false, None)],
            (category, _, _) if category.of_standard() => self.standard_tiles(p, l, category),
            _ => Vec::new(),
        }
    }

    /// What was found, as tiles with a look at each: a material's hatch, a build-up's section,
    /// a profile's outline, a system's face. A click shows the thing where it lives: one of the
    /// project in its editor, one of another library on its page, a kind of drawing or a setting
    /// in its standard.
    fn found_tiles(&self, p: Palette, l: Language) -> Element<'_, Message> {
        use super::library::{buildup_look, material_look, profile_outline, system_look};
        let found = self.library_search(&self.libraries.query, l);
        let title = text(format!("{} · {}", l.text("Zoekresultaten", "Search results"), found.len())).size(20).font(FONT_BOLD).color(p.text);
        if found.is_empty() {
            return column![title, text(l.text("Niets gevonden.", "Nothing found.")).size(12).color(p.text_secondary)].spacing(8).into();
        }
        let lang = super::library::lang(l);
        let words = |words: String| -> Element<'_, Message> { container(text(words).size(12).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph)).width(Fill).into() };
        let tiles = found.into_iter().filter_map(|f| {
            let about = format!("{} · {}", f.kind, f.place);
            let look: Element<'_, Message> = match &f.item {
                Item::Material(shelf, key) => material_look(p, self.shelf(*shelf).material(key)?),
                Item::BuildUp(shelf, id) => {
                    let library = self.shelf(*shelf);
                    buildup_look(p, library, library.buildup(id)?, LOOK)
                }
                Item::Linear(shelf, id) => {
                    let library = self.shelf(*shelf);
                    let t = library.linear_type(id)?;
                    profile_outline(p, &t.profile, library.material(&t.material), LOOK - 8.0)
                }
                Item::System(shelf, id) => {
                    let library = self.shelf(*shelf);
                    system_look(p, library, library.system_type(id)?, LOOK)
                }
                Item::Steel(designation) => profile_outline(p, &bim_library::profile::steel(designation)?.shape, general().material("steel"), LOOK - 8.0),
                Item::Setting(setting) => words(match setting {
                    Setting::Kind(key) => self.standard.kind(key).map(|k| format!("{} · {} · {}", k.code, k.discipline.code(), k.scales.iter().map(|s| bim_library::drawing::scale_text(*s)).collect::<Vec<_>>().join(", "))),
                    Setting::LineStyle(key) => self.standard.line_style(key).map(|s| s.names.get(lang).to_string()),
                    Setting::Pen(key) => self.standard.pen(key).map(|pen| format!("{} mm", pen.width)),
                    Setting::TextStyle(key) => self.standard.text_style(key).map(|t| format!("{} mm", t.height)),
                    Setting::Scale(scale) => Some(bim_library::drawing::scale_text(*scale)),
                }?),
            };
            let press = match f.item {
                Item::Material(Shelf::Project, key) => Message::Material(MaterialMessage::Pick(key)),
                Item::BuildUp(Shelf::Project, id) => Message::BuildUp(BuildUpMessage::Pick(id)),
                Item::Linear(Shelf::Project, id) => Message::Profile(ProfileMessage::Pick(id)),
                Item::System(Shelf::Project, id) => Message::SystemType(SystemMessage::Pick(id)),
                Item::Setting(setting) => Message::Libraries(LibrariesMessage::Open(setting)),
                item => Message::Libraries(LibrariesMessage::Look(item)),
            };
            Some(tile(p, look, f.name, about, false, Some(press)))
        });
        column![title, row(tiles).spacing(10).wrap()].spacing(14).into()
    }

    /// The Library page: the tiles of the category shown of the source shown, or what a search
    /// found. Nothing stands over it but its own heading.
    pub fn libraries_page(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let view = &self.libraries;
        fn framed<'a>(p: Palette, page: Element<'a, Message>) -> Element<'a, Message> {
            container(scrollable(container(page).padding(20).width(Fill)).height(Fill)).width(Fill).height(Fill).style(move |_| p.panel()).into()
        }
        // searching: the page shows what was found, in place of the category
        if view.query.trim().chars().count() >= 2 {
            return framed(p, self.found_tiles(p, l));
        }
        let category = self.category();
        let place = match view.source {
            Source::Country => format!("{} · {}", view.source.label(l), view.country.label(l)),
            Source::Project => match self.model.as_ref().and_then(|m| m.path.file_name()).and_then(|n| n.to_str()) {
                Some(file) => format!("{} · {file}", view.source.label(l)),
                None => view.source.label(l).to_string(),
            },
            source => source.label(l).to_string(),
        };
        let mut about = format!("{place} · {}", self.category_count(category));
        if category == Category::Hatches {
            about = format!("{about} · {}", self.shown_hatches().1);
        }
        let heading = column![text(category.label(l)).size(20).font(FONT_BOLD).color(p.text), text(about).size(12).color(p.text_secondary)].spacing(4);
        let body: Element<'_, Message> = if category == Category::Steel {
            self.steel_table(p, l)
        } else {
            let tiles = self.category_tiles(p, l, category);
            if tiles.is_empty() {
                text(l.text("Niets in deze categorie.", "Nothing in this category.")).size(12).color(p.text_secondary).into()
            } else {
                row(tiles).spacing(10).wrap().into()
            }
        };
        let mut page = Column::new().spacing(14).push(heading).push(body);
        if category == Category::Kinds {
            page = page.push(text(format!("{}: {}", l.text("Eerste blad", "First sheet"), self.standard.first_sheet)).size(12).color(p.text_secondary));
        }
        framed(p, page.into())
    }
}

/// How many colours a house style shows.
const HOUSE_COLOURS: usize = 6;

/// One hatch on a small tile, drawn as a section shows it: a look at a material.
pub(super) fn hatch_tile<'a, M: 'a>(pattern: HatchPattern, p: Palette) -> Element<'a, M> {
    hatch_look(pattern, p, HatchTiles::SIDE)
}

/// One hatch on a square `side` wide, drawn as a section shows it.
pub(super) fn hatch_look<'a, M: 'a>(pattern: HatchPattern, p: Palette, side: f32) -> Element<'a, M> {
    canvas_widget(HatchTiles { pattern, edge: p.border }).width(side).height(side).into()
}

/// A hatch on a square, drawn as a section shows it.
struct HatchTiles {
    pattern: HatchPattern,
    edge: Color,
}

impl HatchTiles {
    /// The side of a small tile.
    const SIDE: f32 = 22.0;
    /// Pixels per metre of the model: a hatch spaced 40 mm shows a line every 8 px.
    const SCALE: f32 = 200.0;
}

impl<M> canvas::Program<M> for HatchTiles {
    type State = ();

    fn draw(&self, _state: &(), renderer: &iced::Renderer, _theme: &iced::Theme, bounds: Rectangle, _cursor: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let tile = Rectangle::new(Point::ORIGIN, Size::new(bounds.width, bounds.height));
        // (a hatch keeps its lines inside the band it is given)
        super::library::draw_hatch(&mut frame, tile, self.pattern, Self::SCALE);
        frame.stroke(&Path::rectangle(tile.position(), tile.size()), Stroke::default().with_color(self.edge).with_width(1.0));
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn categories(tree: &[(Heading, Vec<(Category, usize)>)], heading: Heading) -> Vec<Category> {
        tree.iter().find(|(h, _)| *h == heading).map_or(Vec::new(), |(_, list)| list.iter().map(|(category, _)| *category).collect())
    }

    #[test]
    fn each_source_lists_what_it_holds_by_the_kind_of_thing() {
        let mut app = App::default();
        // the general library: materials, the planar elements together, the linear ones with the
        // steel table, the systems, and the hatches of its standard
        let general = app.library_tree();
        assert_eq!(general.iter().map(|(heading, _)| *heading).collect::<Vec<_>>(), [Heading::Materials, Heading::Planar, Heading::Linear, Heading::Systems, Heading::Standard]);
        assert_eq!(categories(&general, Heading::Planar), Role::ALL.map(Category::Planar));
        assert_eq!(general[0].1, [(Category::Materials, Library::starter().materials.len())]);
        assert!(categories(&general, Heading::Linear).starts_with(&[Category::Linear(LinearRole::Column), Category::Linear(LinearRole::Beam)]));
        assert_eq!(categories(&general, Heading::Linear).last(), Some(&Category::Steel));
        assert_eq!(categories(&general, Heading::Systems).len(), SystemGroup::ALL.len());
        assert_eq!(categories(&general, Heading::Standard), [Category::Hatches]);
        // the first category shows until another is picked
        assert_eq!(app.category(), Category::Materials);
        // a country: its hatches; the Netherlands its drawing standard first, and the types of
        // its template that it has
        let _ = app.libraries_update(LibrariesMessage::Country(Country::Germany));
        assert_eq!((app.libraries.source, app.libraries.country), (Source::Country, Country::Germany));
        let germany = app.library_tree();
        assert_eq!((categories(&germany, Heading::Standard), categories(&germany, Heading::Linear)), (vec![Category::Hatches], vec![Category::Steel]));
        assert_eq!((app.category(), app.shown_hatches().1), (Category::Hatches, "DIN 1356"));
        let _ = app.libraries_update(LibrariesMessage::Country(Country::Netherlands));
        let netherlands = app.library_tree();
        assert_eq!(categories(&netherlands, Heading::Standard), [Category::Kinds, Category::Scales, Category::LineStyles, Category::Pens, Category::TextStyles, Category::Hatches]);
        assert!(netherlands.iter().all(|(_, list)| list.iter().all(|(category, count)| *count > 0 || category.of_standard())), "only what the template has: {netherlands:?}");
        assert!(categories(&netherlands, Heading::Planar).contains(&Category::Planar(Role::Wall)) && categories(&netherlands, Heading::Systems).is_empty());
        assert_eq!((app.category(), app.shown_hatches().1), (Category::Kinds, "NEN 47"));
        // a company: its house style; the project: its own library and its standard
        let _ = app.libraries_update(LibrariesMessage::Source(Source::Company));
        assert_eq!(app.library_tree(), [(Heading::House, vec![(Category::Colours, 6), (Category::Fonts, 1)])]);
        let _ = app.libraries_update(LibrariesMessage::Source(Source::Project));
        app.library.materials.truncate(2);
        let project = app.library_tree();
        assert_eq!(project[0].1, [(Category::Materials, 2)]);
        assert_eq!(categories(&project, Heading::Standard).first(), Some(&Category::Kinds));
    }

    #[test]
    fn a_category_shows_its_tiles_and_a_click_edits_or_looks() {
        let mut app = App::default();
        let l = Language::English;
        let click = |app: &mut App, pane: bool, label: &str| {
            let messages: Vec<Message> = {
                let palette = app.theme.palette();
                let part = if pane { app.libraries_pane(palette, l) } else { app.libraries_page(palette, l) };
                let mut ui = crate::app::test_ui_of(iced::Size::new(if pane { 260.0 } else { 1200.0 }, 1400.0), part);
                ui.click(label).unwrap_or_else(|_| panic!("no {label}"));
                ui.into_messages().collect()
            };
            for message in messages {
                let _ = app.update(message);
            }
        };
        // the project: the tree's walls, their tiles, and a click on one opens its editor
        click(&mut app, true, "Project");
        click(&mut app, true, "Walls");
        assert_eq!((app.libraries.source, app.category(), app.buildup_view.shown()), (Source::Project, Category::Planar(Role::Wall), Role::Wall));
        {
            let mut page = crate::app::test_ui_of(iced::Size::new(1200.0, 1400.0), app.libraries_page(app.theme.palette(), l));
            assert!(page.find("Walls").is_ok() && page.find("+ Build-up").is_ok() && page.find("Concrete 200").is_ok() && page.find("200 mm · layers: 1").is_ok());
        }
        click(&mut app, false, "Concrete 200");
        let wall = app.buildup_view.picked.clone().expect("the wall is picked to edit");
        assert_eq!(app.library.name(&wall, bim_library::Lang::En).as_deref(), Some("Concrete 200"));
        // another category lets go of it; the last tile makes one more of that category
        click(&mut app, true, "Floors");
        assert_eq!((app.buildup_view.picked.clone(), app.category()), (None, Category::Planar(Role::Floor)));
        let floors = app.library.buildups_of(Role::Floor).count();
        click(&mut app, false, "+ Build-up");
        assert_eq!(app.library.buildups_of(Role::Floor).count(), floors + 1);
        // the general library: a click looks at a tile, and the pane makes a copy to edit
        click(&mut app, true, "General");
        click(&mut app, true, "Timber frames");
        click(&mut app, false, "Timber frame 38×140 c/c 600");
        assert_eq!(app.libraries.looked, Some(Item::System(Shelf::General, "system.timber-frame-140".into())));
        {
            let mut side = crate::app::test_ui_of(iced::Size::new(340.0, 1200.0), app.library_side(app.theme.palette(), l));
            assert!(side.find("Edit a copy in the project").is_ok() && side.find("IfcWallType STANDARD").is_ok());
            side.click("Edit a copy in the project").unwrap();
            let messages: Vec<Message> = side.into_messages().collect();
            for message in messages {
                let _ = app.update(message);
            }
        }
        assert_eq!((app.libraries.source, app.category(), app.system_view.picked.as_deref()), (Source::Project, Category::Systems(SystemGroup::TimberFrame), Some("own.timber-frame.1")));
        // the drawing standard: a tile is picked to set, and the last one makes a kind
        click(&mut app, true, "Kinds of drawing");
        click(&mut app, false, "pile plan");
        assert_eq!((app.standard_view.picked.clone(), app.system_view.picked.clone()), (Some(Setting::Kind("pile-plan".into())), None));
        let kinds = app.standard.kinds.len();
        click(&mut app, false, "+ Kind of drawing");
        assert_eq!(app.standard.kinds.len(), kinds + 1);
        click(&mut app, true, "Line weights");
        {
            let mut page = crate::app::test_ui_of(iced::Size::new(1200.0, 900.0), app.libraries_page(app.theme.palette(), l));
            assert!(page.find("0.35 mm").is_ok() && page.find("+ Line weight").is_ok());
        }
    }

    #[test]
    fn a_search_finds_types_profiles_and_kinds_of_drawing_and_shows_them_where_they_live() {
        let mut app = App::default();
        let l = Language::English;
        assert!(app.library_search("b", l).is_empty(), "from two letters on");
        // a material, a build-up and a steel profile, each once though two libraries hold them
        let found = app.library_search("concrete", l);
        assert_eq!(found.iter().filter(|f| f.kind == "Materials" && f.name == "concrete").count(), 1);
        assert!(found.iter().any(|f| matches!(f.item, Item::BuildUp(Shelf::Project, _))), "{found:?}");
        let steel = app.library_search("HEA 200", l);
        assert_eq!(steel.first().map(|f| (f.kind, f.item.clone())), Some(("Steel profiles", Item::Steel("HEA 200".into()))));
        // the Dutch template's types by the names the template gives them
        let beam = app.library_search("BK 300", l).into_iter().find(|f| f.kind == "Linear types").expect("a beam of the template");
        assert!(matches!(beam.item, Item::Linear(Shelf::Netherlands, _)));
        // looking at it shows it on the page of its country, in its category, with it picked
        let _ = app.libraries_update(LibrariesMessage::Search("BK 300".into()));
        let _ = app.libraries_update(LibrariesMessage::Look(beam.item.clone()));
        assert_eq!((app.libraries.source, app.libraries.country, app.libraries.query.as_str(), app.libraries.looked.clone()), (Source::Country, Country::Netherlands, "", Some(beam.item)));
        assert!(matches!(app.category(), Category::Linear(_)));
        // a kind of drawing by its name or its code; opening it shows it in its standard
        let plans = app.library_search("pile", l);
        let Some(Item::Setting(setting)) = plans.iter().find(|f| f.kind == "Kinds of drawing").map(|f| f.item.clone()) else { panic!("{plans:?}") };
        let _ = app.libraries_update(LibrariesMessage::Search("pile".into()));
        let _ = app.libraries_update(LibrariesMessage::Open(setting.clone()));
        assert_eq!((app.libraries.source, app.libraries.country, app.libraries.query.as_str(), app.category()), (Source::Country, Country::Netherlands, "", Category::Kinds));
        assert_eq!(app.standard_view.picked, Some(setting));
        // the page shows what was found, with a look at each
        let _ = app.libraries_update(LibrariesMessage::Search("HEA 2".into()));
        let mut page = crate::app::test_ui_of(iced::Size::new(1200.0, 800.0), app.libraries_page(app.theme.palette(), l));
        let found = app.library_search("HEA 2", l).len();
        assert!(page.find("HEA 200").is_ok() && page.find("HEA 240").is_ok() && page.find(format!("Search results · {found}").as_str()).is_ok());
    }

    #[test]
    fn the_page_and_the_pane_show_the_source_picked() {
        let mut app = App::default();
        let _ = app.libraries_update(LibrariesMessage::Country(Country::Germany));
        {
            let mut pane = crate::app::test_ui_of(iced::Size::new(260.0, 600.0), app.libraries_pane(app.theme.palette(), Language::English));
            // the countries stand under the sources while Country shows, and the tree under them
            assert!(pane.find("Germany").is_ok() && pane.find("Company").is_ok() && pane.find("Drawing standard").is_ok() && pane.find("Steel profiles").is_ok());
        }
        let mut page = crate::app::test_ui_of(iced::Size::new(1200.0, 800.0), app.libraries_page(app.theme.palette(), Language::English));
        assert!(page.find("Hatches").is_ok() && page.find("Country · Germany · 9 · DIN 1356").is_ok());
        // a hatch is named after the general library's material of its kind
        assert!(page.find("brickwork").is_ok() && page.find("air cavity").is_ok());
    }
}
