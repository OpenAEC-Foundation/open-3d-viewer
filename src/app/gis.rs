//! GIS: where the project lies and what lies around it (the GIS tab of the right pane; design in
//! `docs/superpowers/specs/2026-11-17-gis-design.md`).
//!
//! A place is found by its address or name (the place search of PDOK) or typed as coordinates.
//! Around it the parcels of the cadastral map and the buildings of the register of buildings are
//! fetched and added to the project as models of their own, each element with what the register
//! says about it.
//!
//! Where they land: a model that says where it lies on the Dutch national grid (the map
//! conversion of an IFC file) takes them at their true place, turned as the model is. Without
//! such a model the place of the first fetch lies at the zero of the project, north up.
//!
//! A request tells the service which place is looked at; nothing of the model goes along. The
//! browser build does not ask yet.

use super::{App, Message as AppMessage, ModelEntry};
use crate::model::Model;
use crate::theme::Palette;
use bim_gis::pdok::{self, Layer, Place};
use bim_gis::{Frame, Rd};
use glam::DVec3;
use iced::widget::text::Wrapping;
use iced::widget::{button, checkbox, column, row, text, text_input};
use iced::{Center, Element, Length, Task};
use openaec_ui::Language;
use std::sync::Arc;

/// The radii offered for the surroundings (metres).
pub const RADII: [u32; 3] = [100, 250, 500];
/// The places a search shows.
#[cfg(not(target_arch = "wasm32"))]
const PLACES: usize = 5;
/// The pages asked of a layer: 5000 features, more than 500 m around a place holds in a town.
#[cfg(not(target_arch = "wasm32"))]
const PAGES: usize = 5;

/// What the GIS tab holds.
#[derive(Debug, Clone)]
pub struct Gis {
    /// What is typed to search.
    query: String,
    searching: bool,
    /// The places of the last search; `None` before one.
    found: Option<Vec<Place>>,
    /// The place chosen.
    pub place: Option<Place>,
    /// The grid point at the zero of the project while no model says where it lies: the place
    /// of the first fetch, kept while a model fetched then is open.
    pub(super) zero: Option<Rd>,
    /// The height above NAP at the project's zero level while no model says it: the ground at
    /// the zero, set by the first terrain.
    pub(super) zero_level: Option<f64>,
    /// The models fetched (their ids).
    pub(super) fetched: Vec<u64>,
    /// The terrain section.
    pub(super) terrain: super::terrain::State,
    /// The map section.
    pub(super) map: super::map::State,
    /// The Geotechnics tab.
    pub(super) geo: super::geo::State,
    pub radius: u32,
    /// Which layers to fetch, as [`Layer::ALL`].
    pub layers: [bool; 2],
    fetching: bool,
    /// What the last search or fetch came to.
    note: Option<Note>,
}

impl Default for Gis {
    fn default() -> Self {
        Gis { query: String::new(), searching: false, found: None, place: None, zero: None, zero_level: None, fetched: Vec::new(), terrain: Default::default(), map: Default::default(), geo: Default::default(), radius: RADII[1], layers: [true; 2], fetching: false, note: None }
    }
}

/// What the last search or fetch came to.
#[derive(Debug, Clone, PartialEq)]
enum Note {
    /// The features loaded per layer, and whether the area held more than was asked for.
    Loaded(Vec<(Layer, usize)>, bool),
    Failed(String),
}

/// The features of a layer as fetched: a model of them, or none where the area has none.
#[derive(Debug, Clone)]
pub struct Fetched {
    pub layer: Layer,
    pub model: Option<Arc<Model>>,
    /// What the reader took off the coordinates (metres of the project).
    pub first: [f64; 2],
    pub count: usize,
    /// The area holds more than was asked for.
    pub more: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Query(String),
    Search,
    Found(Result<Vec<Place>, String>),
    Pick(usize),
    Radius(u32),
    Layer(Layer, bool),
    /// Fetch the surroundings; the models are named in the language of the interface.
    Fetch(Language),
    /// What was fetched, with how the grid lay in the project when it was asked.
    Fetched(Result<(Frame, Vec<Fetched>), String>),
    /// The terrain section.
    Terrain(super::terrain::Message),
    /// The map section.
    Map(super::map::Message),
    /// The Geotechnics tab.
    Geo(super::geo::Message),
}

/// How the grid lies in a model whose file says so: its map conversion to RD New.
fn georeference(model: &Model) -> Option<Frame> {
    let info = |key: &str| model.project.info.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
    if !info("Georeference coordinate system")?.replace(' ', "").eq_ignore_ascii_case("EPSG:28992") {
        return None;
    }
    let (east, north) = info("Georeference eastings, northings (m)")?.split_once(',')?;
    let origin = Rd::new(east.trim().parse().ok()?, north.trim().parse().ok()?);
    let rotation = info("Georeference rotation (degrees)").and_then(|r| r.trim().parse().ok()).unwrap_or(0.0);
    origin.valid().then_some(Frame { origin, rotation })
}

/// The place a search is taken to mean: the first found, but the first address where a number
/// was typed and the search puts the street before it.
fn best<'a>(query: &str, places: &'a [Place]) -> Option<&'a Place> {
    let first = places.first()?;
    let numbered = first.kind == "weg" && query.chars().any(|c| c.is_ascii_digit());
    places.iter().find(|place| numbered && place.kind == "adres").or(Some(first))
}

/// A layer's name in the interface.
fn word(layer: Layer, l: Language) -> &'static str {
    match layer {
        Layer::Parcels => l.text("Percelen", "Parcels"),
        Layer::Buildings => l.text("Panden", "Buildings"),
    }
}

/// A grid point as shown: `RD 154792.05, 463172.16 · 52.156720, 5.384165`.
fn coordinates(rd: Rd) -> String {
    let place = rd.to_wgs84();
    format!("RD {:.2}, {:.2} · {:.6}, {:.6}", rd.x, rd.y, place.lat, place.lon)
}

impl App {
    /// The first model that says where it lies on the grid: how the grid lies in it, and what
    /// takes a point of its own coordinates to where the scene draws it.
    pub(super) fn georeferenced(&self) -> Option<(Frame, DVec3)> {
        let scene = self.model.as_ref()?;
        let mut first = 0usize;
        for entry in &self.models {
            let origin = scene.origins.get(first).copied().unwrap_or_default();
            first += entry.model.origins.len().max(1);
            if let Some(frame) = georeference(&entry.model) {
                return Some((frame, origin.shift - origin.anchor.unwrap_or(DVec3::ZERO)));
            }
        }
        None
    }

    /// Where a model says it lies, as a grid point: its map conversion, or the latitude and
    /// longitude of its site.
    fn model_place(&self) -> Option<Rd> {
        self.georeferenced().map(|(frame, _)| frame.origin).or_else(|| {
            self.models.iter().find_map(|entry: &ModelEntry| Self::located(entry)).map(|(lat, lon)| bim_gis::LatLon::new(lat, lon).to_rd()).filter(|rd| rd.valid())
        })
    }

    /// Whether a model was fetched here: a map layer, which has no zero point of its own.
    pub(super) fn is_map_layer(&self, model: &Model) -> bool {
        self.gis.fetched.contains(&model.id)
    }

    /// The height above NAP of the project's zero level: what a model's map conversion says,
    /// else the ground at the zero as the first terrain found it, while a fetched model is open.
    pub(super) fn gis_level(&self) -> Option<f64> {
        if let Some(entry) = self.models.iter().find(|entry| georeference(&entry.model).is_some()) {
            let height = entry.model.project.info.iter().find(|(key, _)| key == "Georeference height (m)").and_then(|(_, value)| value.trim().parse().ok());
            return Some(height.unwrap_or(0.0));
        }
        let open = |id: &u64| self.models.iter().any(|entry| entry.model.id == *id);
        self.gis.zero_level.filter(|_| self.gis.fetched.iter().any(open))
    }

    /// The place the surroundings are fetched around: the one chosen, else where a model lies.
    pub(super) fn gis_center(&self) -> Option<Rd> {
        self.gis.place.as_ref().map(|place| place.rd).or_else(|| self.model_place())
    }

    /// How the grid lies in the project for a fetch around `center`, and what takes a point of
    /// the project to where the scene draws it. Without a model that says where it lies, the
    /// zero of the project is the place of the first fetch that brought a model, while such a
    /// model is open; before that, where a model's site lies, or `center`.
    pub(super) fn gis_frame(&self, center: Rd) -> (Frame, DVec3) {
        if let Some(placed) = self.georeferenced() {
            return placed;
        }
        let open = |id: &u64| self.models.iter().any(|entry| entry.model.id == *id);
        let kept = self.gis.zero.filter(|_| self.gis.fetched.iter().any(open));
        (Frame::at(kept.or_else(|| self.model_place()).unwrap_or(center)), DVec3::ZERO)
    }

    pub(super) fn update_gis(&mut self, message: Message) -> Task<AppMessage> {
        match message {
            Message::Query(query) => self.gis.query = query,
            Message::Search => {
                let query = self.gis.query.trim().to_string();
                if query.is_empty() || self.gis.searching {
                    return Task::none();
                }
                self.gis.note = None;
                // coordinates need no search
                if let Some(rd) = bim_gis::rd::typed(&query) {
                    self.gis.found = None;
                    self.gis.place = Some(Place { name: query, kind: "coordinates".into(), rd });
                    return Task::none();
                }
                self.gis.searching = true;
                return search_task(query);
            }
            Message::Found(result) => {
                self.gis.searching = false;
                match result {
                    Ok(places) => {
                        // the best place is taken; the others are there to pick
                        if let Some(best) = best(&self.gis.query, &places) {
                            self.gis.place = Some(best.clone());
                        }
                        self.gis.found = Some(places);
                    }
                    Err(why) => self.gis.note = Some(Note::Failed(why)),
                }
            }
            Message::Pick(i) => {
                if let Some(place) = self.gis.found.as_ref().and_then(|found| found.get(i)) {
                    self.gis.place = Some(place.clone());
                }
            }
            Message::Radius(radius) => self.gis.radius = radius,
            Message::Layer(layer, on) => {
                if let Some(slot) = Layer::ALL.iter().position(|l| *l == layer) {
                    self.gis.layers[slot] = on;
                }
            }
            Message::Fetch(l) => {
                let layers: Vec<Layer> = Layer::ALL.into_iter().zip(self.gis.layers).filter(|(_, on)| *on).map(|(layer, _)| layer).collect();
                let Some(center) = self.gis_center().filter(|_| !layers.is_empty() && !self.gis.fetching) else { return Task::none() };
                let (frame, _) = self.gis_frame(center);
                self.gis.fetching = true;
                self.gis.note = None;
                let label = self.gis.place.as_ref().map_or_else(|| format!("RD {:.0} {:.0}", center.x, center.y), |place| place.name.clone());
                let names = layers.iter().map(|layer| (*layer, file_name(word(*layer, l), &label))).collect();
                return fetch_task(names, pdok::Bounds::around(center, f64::from(self.gis.radius)), frame, self.kept_terrain().map(|kept| kept.tin.clone()));
            }
            Message::Terrain(message) => return self.update_terrain(message),
            Message::Map(message) => return self.update_map(message),
            Message::Geo(message) => return self.update_geo(message),
            Message::Fetched(result) => {
                self.gis.fetching = false;
                match result {
                    Ok((frame, fetched)) => self.take_fetched(frame, fetched),
                    Err(why) => self.gis.note = Some(Note::Failed(why)),
                }
            }
        }
        Task::none()
    }

    /// For the command line (`--gis PLACE`): the place searched, or read as coordinates, its
    /// surroundings fetched and added, and the GIS tab shown. Waits for the services.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_place(&mut self, query: &str, l: Language) -> Result<(), String> {
        let place = match bim_gis::rd::typed(query) {
            Some(rd) => Place { name: query.trim().to_string(), kind: "coordinates".into(), rd },
            None => {
                let places = get(&pdok::search_url(query, PLACES)).and_then(|answer| pdok::parse_search(&answer))?;
                best(query, &places).cloned().ok_or(format!("no place found for {query}"))?
            }
        };
        self.gis.query = query.to_string();
        let names = Layer::ALL.into_iter().zip(self.gis.layers).filter(|(_, on)| *on).map(|(layer, _)| (layer, file_name(word(layer, l), &place.name))).collect();
        let center = place.rd;
        self.gis.place = Some(place);
        // the terrain first, so the parcels and buildings lie on it
        self.open_terrain(l)?;
        let (frame, _) = self.gis_frame(center);
        let fetched = fetch(names, pdok::Bounds::around(center, f64::from(self.gis.radius)), frame, self.kept_terrain().map(|kept| kept.tin.clone()))?;
        self.take_fetched(frame, fetched);
        Ok(())

    }

    /// Adds the fetched models to the project, each where the grid puts it; `frame` is how the
    /// grid lay in the project when they were asked for.
    fn take_fetched(&mut self, frame: Frame, fetched: Vec<Fetched>) {
        let first_model = self.models.is_empty();
        let georeferenced = self.georeferenced();
        let to_scene = georeferenced.map_or(DVec3::ZERO, |(_, to_scene)| to_scene);
        let mut added = false;
        for part in &fetched {
            let Some(model) = &part.model else { continue };
            self.gis.fetched.push(model.id);
            self.add_model(model.clone());
            if let Some(entry) = self.models.last_mut() {
                entry.placed.moved = DVec3::new(part.first[0], part.first[1], 0.0) + to_scene;
            }
            added = true;
        }
        if added {
            if georeferenced.is_none() {
                self.gis.zero = Some(frame.origin);
            }
            self.replace_models();
            if first_model {
                self.fit();
            }
        }
        self.side_tab = super::SideTab::Gis;
        self.gis.note = Some(Note::Loaded(fetched.iter().map(|part| (part.layer, part.count)).collect(), fetched.iter().any(|part| part.more)));
    }

    /// The GIS tab of the right pane.
    pub(super) fn gis_tab(&self, p: Palette, l: Language) -> Element<'_, AppMessage> {
        let section = |title: &str| text(title.to_uppercase()).size(11).font(super::FONT_BOLD).color(p.group_label);
        let note = |words: String| text(words).size(11).color(p.text_muted).wrapping(Wrapping::WordOrGlyph);
        let plain = |words: String| text(words).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph);
        let gis = &self.gis;
        let mut rows = column![section(l.text("Locatie", "Location"))].spacing(6);
        let search = button(text(if gis.searching { l.text("Zoeken…", "Searching…") } else { l.text("Zoeken", "Search") }).size(12))
            .padding([3, 10])
            .style(move |_, s| p.primary_button(s))
            .on_press_maybe((!gis.searching && !gis.query.trim().is_empty()).then_some(AppMessage::Gis(Message::Search)));
        rows = rows.push(
            row![
                openaec_ui::design::a11y::named(
                    text_input(l.text("Adres, plaats, postcode of coördinaten", "Address, place, postcode or coordinates"), &gis.query)
                        .on_input(|typed| AppMessage::Gis(Message::Query(typed)))
                        .on_submit(AppMessage::Gis(Message::Search))
                        .size(12)
                        .padding([3, 6]),
                    openaec_ui::design::a11y::Role::Label,
                    l.text("Locatie", "Location"),
                ),
                search
            ]
            .spacing(4)
            .align_y(Center),
        );
        // the places found, the one chosen marked
        match &gis.found {
            Some(found) if found.is_empty() => rows = rows.push(note(l.text("Geen plaats gevonden", "No place found").to_string())),
            Some(found) => {
                for (i, place) in found.iter().enumerate() {
                    let chosen = gis.place.as_ref() == Some(place);
                    rows = rows.push(
                        button(text(place.name.clone()).size(12).wrapping(Wrapping::WordOrGlyph))
                            .width(Length::Fill)
                            .padding([3, 6])
                            .style(move |_, s| p.ribbon_button(chosen, s))
                            .on_press(AppMessage::Gis(Message::Pick(i))),
                    );
                }
            }
            None => {}
        }
        match (&gis.place, self.model_place()) {
            (Some(place), _) => rows = rows.push(plain(place.name.clone())).push(note(coordinates(place.rd))),
            (None, Some(rd)) => rows = rows.push(plain(l.text("Locatie uit het model", "Location from the model").to_string())).push(note(coordinates(rd))),
            (None, None) => {}
        }
        rows = rows.push(iced::widget::rule::horizontal(1)).push(self.map_section(p, l));
        rows = rows.push(iced::widget::rule::horizontal(1)).push(section(l.text("Omgeving", "Surroundings")));
        if cfg!(target_arch = "wasm32") {
            return rows.push(note(l.text("De browserversie haalt nog geen omgeving op.", "The browser build does not fetch surroundings yet.").to_string())).into();
        }
        for (layer, on) in Layer::ALL.into_iter().zip(gis.layers) {
            rows = rows.push(checkbox(on).label(word(layer, l)).size(14).text_size(12).on_toggle(move |on| AppMessage::Gis(Message::Layer(layer, on))));
        }
        let mut radii = row![text(l.text("Straal", "Radius")).size(12).color(p.text_secondary)].spacing(4).align_y(Center);
        for radius in RADII {
            let chosen = gis.radius == radius;
            radii = radii.push(button(text(format!("{radius} m")).size(12)).padding([3, 8]).style(move |_, s| p.ribbon_button(chosen, s)).on_press(AppMessage::Gis(Message::Radius(radius))));
        }
        rows = rows.push(radii);
        let ready = self.gis_center().is_some() && gis.layers.contains(&true) && !gis.fetching;
        rows = rows.push(
            button(text(if gis.fetching { l.text("Omgeving wordt opgehaald…", "Fetching the surroundings…") } else { l.text("Omgeving laden", "Load surroundings") }).size(12))
                .padding([4, 12])
                .style(move |_, s| p.primary_button(s))
                .on_press_maybe(ready.then_some(AppMessage::Gis(Message::Fetch(l)))),
        );
        if self.gis_center().is_none() {
            rows = rows.push(note(l.text("Zoek eerst een plaats, of open een model met een georeferentie.", "Search a place first, or open a model with a georeference.").to_string()));
        } else if self.georeferenced().is_none() {
            rows = rows.push(note(l.text("Zonder georeferentie ligt de plek van de eerste keer laden op het nulpunt van het project.", "Without a georeference the place of the first load lies at the zero point of the project.").to_string()));
        }
        match &gis.note {
            Some(Note::Loaded(parts, more)) => {
                for (layer, count) in parts {
                    rows = rows.push(plain(match count {
                        0 => format!("{}: {}", word(*layer, l), l.text("niets gevonden in dit gebied", "nothing found in this area")),
                        _ => format!("{}: {count}", word(*layer, l)),
                    }));
                }
                if *more {
                    rows = rows.push(note(l.text("Niet alles is geladen: het gebied heeft meer objecten dan in één keer worden opgehaald.", "Not everything was loaded: the area holds more objects than are fetched at once.").to_string()));
                }
            }
            Some(Note::Failed(why)) => rows = rows.push(note(why.clone())),
            None => {}
        }
        rows = rows.push(note(l.text("Percelen en panden komen van het Kadaster via PDOK (alleen Nederland). De vraag vertelt PDOK welke plek u bekijkt; verder gaat er niets mee.", "Parcels and buildings come from the Dutch land registry through PDOK (the Netherlands only). The request tells PDOK which place you are looking at; nothing else goes along.").to_string()));
        rows = rows.push(iced::widget::rule::horizontal(1)).push(self.terrain_section(p, l));
        rows.into()
    }
}

/// The name of a fetched model: the layer and the place, as a file a GeoJSON reader takes.
pub(super) fn file_name(layer: &str, place: &str) -> String {
    let place: String = place.chars().map(|c| if c.is_alphanumeric() || " ,.-".contains(c) { c } else { ' ' }).collect();
    format!("{layer} {}.geojson", place.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// The model of a layer's features, as the GeoJSON reader makes it.
#[cfg(test)]
fn model_of(name: String, layer: Layer, items: &[serde_json::Value], frame: &Frame, more: bool) -> Result<Fetched, String> {
    model_on(name, layer, items, frame, more, None)
}

/// As [`model_of`], with the features laid on the terrain where it is given.
#[cfg(not(target_arch = "wasm32"))]
fn model_on(name: String, layer: Layer, items: &[serde_json::Value], frame: &Frame, more: bool, terrain: Option<&bim_civil::Tin>) -> Result<Fetched, String> {
    let ground = terrain.map(|tin| move |x: f64, y: f64| tin.height(x, y));
    let collection = pdok::collection_on(layer, items, frame, ground.as_ref().map(|g| g as pdok::Ground));
    let (Some(first), true) = (collection.first, collection.count > 0) else {
        return Ok(Fetched { layer, model: None, first: [0.0; 2], count: 0, more });
    };
    let source = crate::loaders::Source::Bytes { name, data: Arc::new(collection.geojson.to_string().into_bytes()) };
    let model = crate::loaders::load_stack(&[source], &crate::loaders::Progress::default())?;
    Ok(Fetched { layer, model: Some(Arc::new(model)), first, count: collection.count, more })
}

/// What a service answers at an address, as bytes.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn get_bytes(url: &str) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(30)).user_agent(concat!("OpenBIMSuite/", env!("CARGO_PKG_VERSION"))).build();
    let response = agent.get(url).call().map_err(|e| format!("PDOK: {e}"))?;
    let mut answer = Vec::new();
    // an answer far larger than a page of features or a height grid is not read
    response.into_reader().take(64 << 20).read_to_end(&mut answer).map_err(|e| format!("PDOK: {e}"))?;
    Ok(answer)
}

/// What a service answers at an address, as text.
#[cfg(not(target_arch = "wasm32"))]
fn get(url: &str) -> Result<String, String> {
    String::from_utf8(get_bytes(url)?).map_err(|_| "PDOK: the answer is no text".to_string())
}

/// The features of the layers in `bounds`, each as a model in the coordinates of `frame`.
#[cfg(not(target_arch = "wasm32"))]
fn fetch(names: Vec<(Layer, String)>, bounds: pdok::Bounds, frame: Frame, terrain: Option<Arc<bim_civil::Tin>>) -> Result<Vec<Fetched>, String> {
    let mut out = Vec::new();
    for (layer, name) in names {
        let mut items = Vec::new();
        let mut next = Some(pdok::items_url(layer, bounds));
        for _ in 0..PAGES {
            let Some(url) = next.take() else { break };
            let page = pdok::parse_page(layer, &get(&url)?)?;
            items.extend(page.features);
            next = page.next;
        }
        out.push(model_on(name, layer, &items, &frame, next.is_some(), terrain.as_deref())?);
    }
    Ok(out)
}

/// Searches a place on a worker thread.
#[cfg(not(target_arch = "wasm32"))]
fn search_task(query: String) -> Task<AppMessage> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(get(&pdok::search_url(&query, PLACES)).and_then(|answer| pdok::parse_search(&answer)));
            });
            rx.await.unwrap_or_else(|_| Err("the search did not come back".into()))
        },
        |result| AppMessage::Gis(Message::Found(result)),
    )
}

/// Fetches the surroundings on a worker thread.
#[cfg(not(target_arch = "wasm32"))]
fn fetch_task(names: Vec<(Layer, String)>, bounds: pdok::Bounds, frame: Frame, terrain: Option<Arc<bim_civil::Tin>>) -> Task<AppMessage> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(fetch(names, bounds, frame, terrain));
            });
            rx.await.unwrap_or_else(|_| Err("the surroundings did not come back".into()))
        },
        move |result| AppMessage::Gis(Message::Fetched(result.map(|fetched| (frame, fetched)))),
    )
}

/// The browser build does not ask the services yet.
#[cfg(target_arch = "wasm32")]
fn search_task(_: String) -> Task<AppMessage> {
    Task::done(AppMessage::Gis(Message::Found(Err("the browser build does not search places yet".into()))))
}

#[cfg(target_arch = "wasm32")]
fn fetch_task(_: Vec<(Layer, String)>, _: pdok::Bounds, _: Frame, _: Option<Arc<bim_civil::Tin>>) -> Task<AppMessage> {
    Task::done(AppMessage::Gis(Message::Fetched(Err("the browser build does not fetch surroundings yet".into()))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::SideTab;
    use serde_json::json;

    fn place() -> Place {
        Place { name: "Stadhuisplein 1, 3811LM Amersfoort".into(), kind: "adres".into(), rd: Rd::new(154_792.05, 463_172.16) }
    }

    /// A building of the register: a triangle with a corner at `corner`.
    fn building(corner: [f64; 2]) -> serde_json::Value {
        let [x, y] = corner;
        json!({"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[x, y],[x + 10.0, y],[x, y + 8.0],[x, y]]]},"properties":{"identificatie":"0307100000370278","bouwjaar":1974}})
    }

    /// A parcel of the cadastral map: a square with a corner at `corner`.
    fn parcel(corner: [f64; 2]) -> serde_json::Value {
        let [x, y] = corner;
        json!({"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[x, y],[x + 20.0, y],[x + 20.0, y + 20.0],[x, y + 20.0],[x, y]]]},
            "properties":{"akr_kadastrale_gemeente_code_waarde":"AMF00","sectie":"E","perceelnummer":7948}})
    }

    /// The lowest corner of what the scene draws of the model at `index`.
    fn low_corner(app: &App, index: usize) -> [f32; 3] {
        let scene = app.model.as_ref().unwrap();
        let entry = &app.models[index];
        let mut low = [f32::INFINITY; 3];
        for layer in &scene.layers[entry.layers.clone()] {
            for p in layer.positions.chunks_exact(3) {
                for k in 0..3 {
                    low[k] = low[k].min(p[k]);
                }
            }
        }
        low
    }

    #[test]
    fn typed_coordinates_choose_a_place_without_a_search() {
        let mut app = App::default();
        let _ = app.update(AppMessage::Gis(Message::Query("155000 463000".into())));
        let _ = app.update(AppMessage::Gis(Message::Search));
        assert_eq!(app.gis.place.as_ref().map(|p| p.rd), Some(Rd::new(155_000.0, 463_000.0)));
        assert!(!app.gis.searching);
        assert_eq!(app.gis_center(), Some(Rd::new(155_000.0, 463_000.0)));
        // an address is searched: the tab waits for the places
        let _ = app.update(AppMessage::Gis(Message::Query("Stadhuisplein 1 Amersfoort".into())));
        let _ = app.update(AppMessage::Gis(Message::Search));
        assert!(app.gis.searching);
    }

    #[test]
    fn the_best_place_found_is_chosen_and_another_can_be_picked() {
        let mut app = App::default();
        let other = Place { name: "Stadhuisplein, Amersfoort".into(), kind: "weg".into(), rd: Rd::new(154_841.405, 463_136.52) };
        let _ = app.update(AppMessage::Gis(Message::Found(Ok(vec![other.clone(), place()]))));
        assert_eq!(app.gis.place, Some(other));
        let _ = app.update(AppMessage::Gis(Message::Pick(1)));
        assert_eq!(app.gis.place, Some(place()));
        // a number typed: the address, not the street the search puts first
        let street = Place { name: "Stadhuisplein, Amersfoort".into(), kind: "weg".into(), rd: Rd::new(154_841.405, 463_136.52) };
        let found = [street.clone(), place()];
        assert_eq!(best("Stadhuisplein 1 Amersfoort", &found), Some(&place()));
        assert_eq!(best("Stadhuisplein Amersfoort", &found), Some(&street));
        assert_eq!(best("Stadhuisplein 1", &[street.clone()]), Some(&street));
        assert_eq!(best("3811LM", &[]), None);
        // a search that fails says why and keeps the place
        let _ = app.update(AppMessage::Gis(Message::Found(Err("PDOK: no connection".into()))));
        assert_eq!((app.gis.place.clone(), app.gis.note.clone()), (Some(place()), Some(Note::Failed("PDOK: no connection".into()))));
    }

    #[test]
    fn fetched_layers_become_models_around_the_place_at_the_zero_of_the_project() {
        let mut app = App::default();
        app.gis.place = Some(place());
        let center = app.gis_center().unwrap();
        let (frame, _) = app.gis_frame(center);
        assert_eq!(frame, Frame::at(center));
        // a parcel 30 m east and 40 m north of the place, a building 5 m west and 2 m south
        let parcels = model_of("Percelen.geojson".into(), Layer::Parcels, &[parcel([center.x + 30.0, center.y + 40.0])], &frame, false).unwrap();
        let buildings = model_of("Panden.geojson".into(), Layer::Buildings, &[building([center.x - 5.0, center.y - 2.0])], &frame, true).unwrap();
        let empty = model_of("Leeg.geojson".into(), Layer::Buildings, &[], &frame, false).unwrap();
        assert!(empty.model.is_none());
        let _ = app.update(AppMessage::Gis(Message::Fetched(Ok((frame, vec![parcels, buildings])))));
        assert_eq!(app.models.len(), 2);
        assert_eq!(app.side_tab, SideTab::Gis);
        // map layers have no zero point of their own to mark in the view
        assert!(app.zero_points().is_empty());
        assert_eq!(app.gis.note, Some(Note::Loaded(vec![(Layer::Parcels, 1), (Layer::Buildings, 1)], true)));
        // each lies where the grid puts it, whatever its reader took off
        let low = low_corner(&app, 0);
        assert!((low[0] - 30.0).abs() < 0.2 && (low[1] - 40.0).abs() < 0.2, "{low:?}");
        let low = low_corner(&app, 1);
        assert!((low[0] + 5.0).abs() < 1e-3 && (low[1] + 2.0).abs() < 1e-3, "{low:?}");
        // the register's words are the element's properties
        let scene = app.model.as_ref().unwrap();
        assert!(scene.props.values().any(|props| props.contains("0307100000370278") && props.contains("1974")), "{:?}", scene.props.values().next());
        assert!(scene.props.values().any(|props| props.contains("AMF00 E 7948")));
        // a second fetch, around another place, lands in the same frame
        app.gis.place = Some(Place { name: "Elders".into(), kind: "adres".into(), rd: Rd::new(center.x + 400.0, center.y) });
        let again = app.gis_center().unwrap();
        let (frame_again, _) = app.gis_frame(again);
        assert_eq!(frame_again, frame);
        let more = model_of("Panden elders.geojson".into(), Layer::Buildings, &[building([again.x, again.y])], &frame_again, false).unwrap();
        let _ = app.update(AppMessage::Gis(Message::Fetched(Ok((frame_again, vec![more])))));
        let far = low_corner(&app, 2);
        assert!((far[0] - 400.0).abs() < 1e-3 && far[1].abs() < 1e-3, "{far:?}");
        assert!((low_corner(&app, 1)[0] + 5.0).abs() < 1e-3);
    }

    #[test]
    fn a_georeferenced_model_takes_the_surroundings_at_their_true_place_turned_as_it_is() {
        // a model whose zero lies at a grid point, its x axis pointing north, drawn 100 m off
        // its own zero (the anchor its reader took off)
        let layer = crate::model::Layer { name: "Walls".into(), positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0], indices: vec![0, 1, 2], elements: vec![(10, 0, 3)], ..Default::default() };
        let mut model = Model::new(std::path::PathBuf::from("x.ifc"), "IFC", vec![layer]);
        model.project.info.extend([
            ("Georeference coordinate system".to_string(), "EPSG:28992".to_string()),
            ("Georeference eastings, northings (m)".to_string(), "154800.000, 463200.000".to_string()),
            ("Georeference rotation (degrees)".to_string(), "90".to_string()),
        ]);
        model.origins = vec![crate::model::Origin { anchor: Some(DVec3::new(100.0, 0.0, 0.0)), shift: DVec3::ZERO }];
        let mut app = App::default();
        app.set_model(Arc::new(model));
        // no place is chosen: the model says where it lies
        assert_eq!(app.gis_center(), Some(Rd::new(154_800.0, 463_200.0)));
        let (frame, to_scene) = app.gis_frame(Rd::new(154_800.0, 463_200.0));
        assert_eq!((frame, to_scene), (Frame { origin: Rd::new(154_800.0, 463_200.0), rotation: 90.0 }, DVec3::new(-100.0, 0.0, 0.0)));
        // a building 10 m east of the model's zero: at y = -10 in the model, 100 m back in the scene
        let fetched = model_of("Panden.geojson".into(), Layer::Buildings, &[building([154_810.0, 463_200.0])], &frame, false).unwrap();
        let _ = app.update(AppMessage::Gis(Message::Fetched(Ok((frame, vec![fetched])))));
        assert_eq!(app.models.len(), 2);
        let low = low_corner(&app, 1);
        // its corners: (0, -10), (0, -20) and (8, -10) in the model
        assert!((low[0] + 100.0).abs() < 1e-3 && (low[1] + 20.0).abs() < 1e-3, "{low:?}");
    }

    #[test]
    fn the_gis_tab_searches_and_offers_the_surroundings() {
        let mut app = App::default();
        let _ = app.update(AppMessage::SideTab(SideTab::Gis));
        {
            let mut ui = crate::app::test_ui(&app, iced::Size::new(1400.0, 900.0));
            assert!(ui.find("Search a place first, or open a model with a georeference.").is_ok());
            assert!(ui.find("Parcels").is_ok() && ui.find("Buildings").is_ok() && ui.find("250 m").is_ok());
            // nothing to fetch around yet
            let _ = ui.click("Load surroundings");
            assert!(ui.into_messages().next().is_none());
        }
        let _ = app.update(AppMessage::Gis(Message::Found(Ok(vec![place()]))));
        let _ = app.update(AppMessage::Gis(Message::Layer(Layer::Parcels, false)));
        let mut ui = crate::app::test_ui(&app, iced::Size::new(1400.0, 900.0));
        let shown = coordinates(place().rd);
        assert!(shown.starts_with("RD 154792.05, 463172.16 · 52.1567") && ui.find(shown.as_str()).is_ok(), "{shown}");
        ui.click("500 m").expect("the radius");
        ui.click("Load surroundings").expect("the place is chosen");
        let messages: Vec<AppMessage> = ui.into_messages().collect();
        assert!(matches!(messages[..], [AppMessage::Gis(Message::Radius(500)), AppMessage::Gis(Message::Fetch(_))]), "{messages:?}");
    }

    #[test]
    #[ignore = "asks the live services of PDOK"]
    fn the_live_services_give_a_place_and_its_surroundings() {
        let places = get(&pdok::search_url("Stadhuisplein 1 Amersfoort", PLACES)).and_then(|answer| pdok::parse_search(&answer)).unwrap();
        let place = places.iter().find(|p| p.kind == "adres").expect("an address");
        assert!(place.rd.distance(Rd::new(154_792.0, 463_172.0)) < 50.0, "{place:?}");
        let names = vec![(Layer::Parcels, "Percelen.geojson".to_string()), (Layer::Buildings, "Panden.geojson".to_string())];
        let fetched = fetch(names, pdok::Bounds::around(place.rd, 100.0), Frame::at(place.rd), None).unwrap();
        assert_eq!(fetched.len(), 2);
        for part in &fetched {
            assert!(part.count > 3 && part.model.is_some() && !part.more, "{:?}: {}", part.layer, part.count);
        }
        // a town centre within 500 m holds more buildings than one page: the pages are followed
        let many = fetch(vec![(Layer::Buildings, "Panden.geojson".to_string())], pdok::Bounds::around(place.rd, 500.0), Frame::at(place.rd), None).unwrap();
        assert!(many[0].count > pdok::PAGE, "{}", many[0].count);
    }

    #[test]
    fn the_surroundings_lie_on_the_terrain_when_there_is_one() {
        let frame = Frame::at(Rd::new(154_800.0, 463_200.0));
        // ground 3 m over the zero level, everywhere within 50 m of the zero
        let tin = bim_civil::Tin::from_parts(vec![[-50.0, -50.0, 3.0], [50.0, -50.0, 3.0], [50.0, 50.0, 3.0], [-50.0, 50.0, 3.0]], vec![[0, 1, 2], [0, 2, 3]]);
        let lowest = |fetched: Fetched| fetched.model.unwrap().layers[0].positions.chunks_exact(3).map(|p| p[2]).fold(f32::INFINITY, f32::min);
        let on = model_on("Panden.geojson".into(), Layer::Buildings, &[building([154_810.0, 463_200.0])], &frame, false, Some(&tin)).unwrap();
        assert!((lowest(on) - 3.05).abs() < 1e-3);
        let lines = model_on("Percelen.geojson".into(), Layer::Parcels, &[parcel([154_810.0, 463_200.0])], &frame, false, Some(&tin)).unwrap();
        assert!((lowest(lines) - 3.06).abs() < 1e-2);
        // without terrain they lie at the zero level
        let flat = model_on("Panden.geojson".into(), Layer::Buildings, &[building([154_810.0, 463_200.0])], &frame, false, None).unwrap();
        assert_eq!(lowest(flat), 0.0);
    }

    #[test]
    fn a_fetched_model_is_named_after_its_layer_and_place() {
        assert_eq!(file_name("Percelen", "Stadhuisplein 1, 3811LM Amersfoort"), "Percelen Stadhuisplein 1, 3811LM Amersfoort.geojson");
        assert_eq!(file_name("Panden", "a/b:c  \\ d"), "Panden a b c d.geojson");
    }
}
