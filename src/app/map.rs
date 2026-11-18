//! The map under the project: a picture of the place from a map service, laid in the 3D view as
//! a coloured surface (the Map section of the GIS tab).
//!
//! Three maps to choose from: the background map of the Netherlands, the aerial photograph, and
//! any web map service (WMS) named by its address and layer. The picture is fetched for the
//! square around the place and becomes a model of its own, so it is switched on and off, and
//! taken away, like any other. It lies on the terrain when one was loaded, else flat just under
//! the zero level of the project. From above, in a parallel view, it is the map in 2D.
//!
//! A request tells the service which square is looked at; nothing of the model goes along.

use super::{App, Message as AppMessage};
use crate::model::{Layer, Model};
use crate::theme::Palette;
use bim_civil::Tin;
use bim_gis::map::{Picture, Wms};
use bim_gis::pdok::Bounds;
use bim_gis::{Frame, Rd};
use glam::DVec3;
use iced::widget::text::Wrapping;
use iced::widget::{button, column, row, text, text_input};
use iced::{Center, Element, Task};
use openaec_ui::Language;
use std::path::PathBuf;
use std::sync::Arc;

/// The cells a flat map has along a side: its colours are those of the picture at their corners.
const CELLS: usize = 512;
/// The pixels asked of a map service along a side.
#[cfg(not(target_arch = "wasm32"))]
const PIXELS: usize = 1024;
/// The tiles of the background map put together, at most.
#[cfg(not(target_arch = "wasm32"))]
const TILES: usize = 64;
/// How far a map lies over the terrain it is laid on, and under the zero level without one, so
/// it does not flicker with what lies there (metres).
const OVER: f64 = 0.01;
const UNDER: f64 = 0.02;

/// Which map is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The background map of the Netherlands.
    Background,
    /// The aerial photograph of the Netherlands.
    Aerial,
    /// A web map service named by the user.
    Own,
}

/// What the map section holds.
#[derive(Debug, Clone)]
pub struct State {
    pub source: Source,
    /// The address and layer of the user's own service, as typed.
    pub url: String,
    pub layer: String,
    working: bool,
    note: Option<Note>,
}

impl Default for State {
    fn default() -> Self {
        State { source: Source::Background, url: String::new(), layer: String::new(), working: false, note: None }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Note {
    /// The metres a pixel of the loaded picture covers.
    Loaded(f64),
    Failed(String),
}

/// A map made, ready to add to the project.
#[derive(Debug, Clone)]
pub struct Done {
    pub model: Arc<Model>,
    /// How the grid lay in the project when the map was made.
    pub frame: Frame,
    /// The metres a pixel of its picture covers.
    pub pixel: f64,
}

#[derive(Debug, Clone)]
pub enum Message {
    Source(Source),
    Url(String),
    LayerName(String),
    /// Fetch the map around the place; the model is named in the language of the interface.
    Fetch(Language),
    Done(Result<Done, String>),
}

/// What a map's model and layer are called and where its picture comes from.
#[derive(Debug, Clone)]
pub struct Words {
    pub model: String,
    pub layer: String,
    pub source: String,
}

/// The model of a map: the picture as the colours of a surface over `bounds`, in the
/// coordinates of `frame`. On a terrain (in those coordinates) it takes the terrain's own
/// triangles within the square, a little over them; without one it is flat, a little under zero.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn map_model(picture: &Picture, frame: &Frame, bounds: Bounds, terrain: Option<&Tin>, words: &Words) -> Model {
    let colour = |place: Rd| {
        let [r, g, b] = picture.colour(place);
        [r, g, b, 255]
    };
    let inside = |place: Rd| place.x >= bounds.min.x && place.x <= bounds.max.x && place.y >= bounds.min.y && place.y <= bounds.max.y;
    let (mut positions, mut colours, mut indices) = (Vec::new(), Vec::new(), Vec::new());
    match terrain {
        Some(tin) => {
            let places: Vec<Rd> = tin.points.iter().map(|p| frame.rd(p[0], p[1])).collect();
            for (p, place) in tin.points.iter().zip(&places) {
                positions.extend([p[0] as f32, p[1] as f32, (p[2] + OVER) as f32]);
                colours.push(colour(*place));
            }
            indices.extend(tin.triangles.iter().filter(|t| t.iter().all(|i| inside(places[*i as usize]))).flatten().copied());
        }
        None => {
            let side = CELLS + 1;
            for r in 0..side {
                for c in 0..side {
                    let place = Rd::new(
                        bounds.min.x + (bounds.max.x - bounds.min.x) * c as f64 / CELLS as f64,
                        bounds.max.y - (bounds.max.y - bounds.min.y) * r as f64 / CELLS as f64,
                    );
                    let [x, y] = frame.local(place);
                    positions.extend([x as f32, y as f32, -UNDER as f32]);
                    colours.push(colour(place));
                }
            }
            for r in 0..CELLS {
                for c in 0..CELLS {
                    let at = (r * side + c) as u32;
                    // counter-clockwise seen from above
                    indices.extend([at, at + side as u32, at + 1, at + 1, at + side as u32, at + side as u32 + 1]);
                }
            }
        }
    }
    let normals = match terrain {
        Some(_) => crate::model::smooth_normals(&positions, &indices),
        None => [0.0, 0.0, 1.0].repeat(positions.len() / 3),
    };
    let [px, py] = picture.pixel();
    let props = serde_json::json!({
        "Name": words.layer,
        "Source": words.source,
        "Pixel (m)": ((px.max(py)) * 1000.0).round() / 1000.0,
        "West (RD, m)": bounds.min.x.round(),
        "South (RD, m)": bounds.min.y.round(),
        "East (RD, m)": bounds.max.x.round(),
        "North (RD, m)": bounds.max.y.round(),
        "Laid on": if terrain.is_some() { "the terrain" } else { "the zero level" },
    });
    let count = indices.len() as u32;
    let layer = Layer {
        name: words.layer.clone(),
        color: [0.8, 0.8, 0.8],
        positions,
        normals,
        colors: Some(colours),
        indices,
        elements: vec![(1, 0, count)],
        props: vec![(1, props.to_string())],
        ..Layer::default()
    };
    Model::new(PathBuf::from(&words.model), "Map", vec![layer])
}

impl App {
    pub(super) fn update_map(&mut self, message: Message) -> Task<AppMessage> {
        match message {
            Message::Source(source) => self.gis.map.source = source,
            Message::Url(url) => self.gis.map.url = url,
            Message::LayerName(layer) => self.gis.map.layer = layer,
            Message::Fetch(l) => {
                let Some(center) = self.gis_center().filter(|_| !self.gis.map.working) else { return Task::none() };
                let Some(asked) = self.map_asked(l) else {
                    self.gis.map.note = Some(Note::Failed(l.text("Vul het adres van de kaartdienst en de naam van de laag in.", "Fill in the address of the map service and the name of the layer.").to_string()));
                    return Task::none();
                };
                let (frame, _) = self.gis_frame(center);
                self.gis.map.working = true;
                self.gis.map.note = None;
                let terrain = self.kept_terrain().map(|kept| kept.tin.clone());
                return fetch_task(asked, Bounds::around(center, f64::from(self.gis.radius)), frame, terrain);
            }
            Message::Done(done) => {
                self.gis.map.working = false;
                match done {
                    Ok(done) => self.take_map(done),
                    Err(why) => self.gis.map.note = Some(Note::Failed(why)),
                }
            }
        }
        Task::none()
    }

    /// What is asked of which service for the map chosen, with the names of its model; `None`
    /// while the user's own service is not filled in.
    fn map_asked(&self, l: Language) -> Option<Asked> {
        let state = &self.gis.map;
        let (wms, layer, source) = match state.source {
            Source::Background => (None, l.text("Achtergrondkaart", "Background map").to_string(), bim_gis::map::BACKGROUND_SOURCE.to_string()),
            Source::Aerial => (Some(Wms::aerial()), l.text("Luchtfoto", "Aerial photograph").to_string(), bim_gis::map::AERIAL_SOURCE.to_string()),
            Source::Own => {
                let wms = Wms::typed(&state.url, &state.layer)?;
                let (layer, source) = (wms.layer.clone(), format!("{} ({})", wms.url, wms.layer));
                (Some(wms), layer, source)
            }
        };
        let place = self.gis.place.as_ref().map_or_else(String::new, |place| place.name.clone());
        let model = super::gis::file_name(&layer, &place).trim_end_matches(".geojson").trim().to_string();
        Some(Asked { wms, words: Words { model, layer, source } })
    }

    /// For the command line (`--map background|aerial`): the map around the place chosen,
    /// fetched and added. Waits for the service.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_map(&mut self, source: Source, l: Language) -> Result<(), String> {
        self.gis.map.source = source;
        let Some(center) = self.gis_center() else { return Ok(()) };
        let asked = self.map_asked(l).ok_or("no map service named")?;
        let (frame, _) = self.gis_frame(center);
        let terrain = self.kept_terrain().map(|kept| kept.tin.clone());
        let done = fetch(asked, Bounds::around(center, f64::from(self.gis.radius)), frame, terrain)?;
        self.take_map(done);
        Ok(())
    }

    /// Adds a map to the project where the grid puts it, as the other map layers.
    fn take_map(&mut self, done: Done) {
        let first_model = self.models.is_empty();
        let georeferenced = self.georeferenced();
        self.gis.fetched.push(done.model.id);
        self.add_model(done.model.clone());
        if let Some(entry) = self.models.last_mut() {
            entry.placed.moved = georeferenced.map_or(DVec3::ZERO, |(_, to_scene)| to_scene);
        }
        if georeferenced.is_none() {
            self.gis.zero = Some(done.frame.origin);
        }
        self.replace_models();
        if first_model {
            self.fit();
        }
        self.side_tab = super::SideTab::Gis;
        self.gis.map.note = Some(Note::Loaded(done.pixel));
    }

    /// The map section of the GIS tab.
    pub(super) fn map_section(&self, p: Palette, l: Language) -> Element<'_, AppMessage> {
        let section = |title: &str| text(title.to_uppercase()).size(11).font(super::FONT_BOLD).color(p.group_label);
        let note = |words: String| text(words).size(11).color(p.text_muted).wrapping(Wrapping::WordOrGlyph);
        let send = |message: Message| AppMessage::Gis(super::gis::Message::Map(message));
        let state = &self.gis.map;
        let mut rows = column![section(l.text("Kaart", "Map"))].spacing(6);
        if cfg!(target_arch = "wasm32") {
            return rows.push(note(l.text("De browserversie haalt nog geen kaart op.", "The browser build does not fetch a map yet.").to_string())).into();
        }
        let mut sources = row![].spacing(4).align_y(Center);
        for (source, word) in [(Source::Background, l.text("Kaart", "Map")), (Source::Aerial, l.text("Luchtfoto", "Aerial")), (Source::Own, l.text("Eigen WMS", "Own WMS"))] {
            let chosen = state.source == source;
            sources = sources.push(button(text(word).size(12)).padding([3, 8]).style(move |_, s| p.ribbon_button(chosen, s)).on_press(send(Message::Source(source))));
        }
        rows = rows.push(sources);
        if state.source == Source::Own {
            let field = |hint: &'static str, value: &str, name: &'static str, to: fn(String) -> Message| {
                openaec_ui::design::a11y::named(text_input(hint, value).on_input(move |typed| AppMessage::Gis(super::gis::Message::Map(to(typed)))).size(12).padding([3, 6]), openaec_ui::design::a11y::Role::Label, name)
            };
            rows = rows
                .push(field(l.text("Adres van de WMS-dienst (https://…)", "Address of the WMS service (https://…)"), &state.url, l.text("Adres van de kaartdienst", "Address of the map service"), Message::Url))
                .push(field(l.text("Naam van de laag", "Name of the layer"), &state.layer, l.text("Laag van de kaartdienst", "Layer of the map service"), Message::LayerName));
        }
        let ready = self.gis_center().is_some() && !state.working;
        rows = rows.push(
            button(text(if state.working { l.text("Kaart wordt opgehaald…", "Fetching the map…") } else { l.text("Kaart laden", "Load map") }).size(12))
                .padding([4, 12])
                .style(move |_, s| p.primary_button(s))
                .on_press_maybe(ready.then_some(send(Message::Fetch(l)))),
        );
        match &state.note {
            Some(Note::Loaded(pixel)) => rows = rows.push(note(format!("{} {:.2} m", l.text("Geladen; een beeldpunt is", "Loaded; a pixel is"), pixel))),
            Some(Note::Failed(why)) => rows = rows.push(note(why.clone())),
            None => {}
        }
        rows = rows.push(note(l.text("De kaart komt in het model als een eigen laag, op het terrein als dat geladen is. Zet hem aan en uit in de verkenner; kijk van boven voor 2D. Kaart en luchtfoto: PDOK (alleen Nederland).", "The map comes into the model as a layer of its own, on the terrain when that is loaded. Switch it on and off in the explorer; look from above for 2D. Map and aerial photograph: PDOK (the Netherlands only).").to_string()));
        rows.into()
    }
}

/// What is asked: a web map service, or the tiles of the background map when none.
#[derive(Debug, Clone)]
struct Asked {
    wms: Option<Wms>,
    words: Words,
}

/// A picture as a service sent it (PNG or JPEG), as pixels.
#[cfg(not(target_arch = "wasm32"))]
fn decode(bytes: &[u8]) -> Result<image::RgbImage, String> {
    image::load_from_memory(bytes).map(|picture| picture.to_rgb8()).map_err(|_| {
        // a service that cannot draw says why in text
        let said: String = String::from_utf8_lossy(bytes).chars().filter(|c| !c.is_control()).take(200).collect();
        format!("the map service sent no picture: {said}")
    })
}

/// The picture of `bounds` from the service asked.
#[cfg(not(target_arch = "wasm32"))]
fn picture(asked: &Asked, bounds: Bounds) -> Result<Picture, String> {
    let get = |url: &str| super::gis::get_bytes(url).map_err(|why| why.replacen("PDOK: ", "", 1));
    let side = bounds.max.x - bounds.min.x;
    match &asked.wms {
        Some(wms) => {
            let got = decode(&get(&wms.picture_url(bounds, PIXELS, PIXELS))?)?;
            let (width, height) = (got.width() as usize, got.height() as usize);
            Picture::new(bounds, width, height, got.pixels().map(|pixel| pixel.0).collect()).ok_or("the map service sent an empty picture".into())
        }
        None => {
            // the zoom as fine as the pixels asked, less deep while that takes too many tiles
            let mut zoom = bim_gis::map::Tiles::zoom_for(side / PIXELS as f64);
            let mut tiles = bim_gis::map::Tiles::over(bounds, zoom);
            while tiles.size().0 * tiles.size().1 > TILES && zoom > 0 {
                zoom -= 1;
                tiles = bim_gis::map::Tiles::over(bounds, zoom);
            }
            let (wide, high) = tiles.size();
            let tile = bim_gis::map::TILE;
            let (width, height) = (wide * tile, high * tile);
            let mut pixels = vec![[255u8; 3]; width * height];
            for (r, row) in (tiles.rows.0..=tiles.rows.1).enumerate() {
                for (c, col) in (tiles.cols.0..=tiles.cols.1).enumerate() {
                    let got = decode(&get(&tiles.url(col, row))?)?;
                    if (got.width() as usize, got.height() as usize) != (tile, tile) {
                        return Err("a map tile has another size than expected".into());
                    }
                    for (y, line) in got.rows().enumerate() {
                        for (x, pixel) in line.enumerate() {
                            pixels[(r * tile + y) * width + c * tile + x] = pixel.0;
                        }
                    }
                }
            }
            Picture::new(tiles.bounds(), width, height, pixels).ok_or("the background map sent no tiles".into())
        }
    }
}

/// The map of `bounds` as a model in the coordinates of `frame`.
#[cfg(not(target_arch = "wasm32"))]
fn fetch(asked: Asked, bounds: Bounds, frame: Frame, terrain: Option<Arc<Tin>>) -> Result<Done, String> {
    let picture = picture(&asked, bounds)?;
    let model = map_model(&picture, &frame, bounds, terrain.as_deref(), &asked.words);
    let [px, py] = picture.pixel();
    Ok(Done { model: Arc::new(model), frame, pixel: px.max(py) })
}

/// Fetches the map on a worker thread.
#[cfg(not(target_arch = "wasm32"))]
fn fetch_task(asked: Asked, bounds: Bounds, frame: Frame, terrain: Option<Arc<Tin>>) -> Task<AppMessage> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(fetch(asked, bounds, frame, terrain));
            });
            rx.await.unwrap_or_else(|_| Err("the map did not come back".into()))
        },
        |result| AppMessage::Gis(super::gis::Message::Map(Message::Done(result))),
    )
}

/// The browser build does not fetch yet.
#[cfg(target_arch = "wasm32")]
fn fetch_task(_: Asked, _: Bounds, _: Frame, _: Option<Arc<Tin>>) -> Task<AppMessage> {
    Task::done(AppMessage::Gis(super::gis::Message::Map(Message::Done(Err("the browser build does not fetch a map yet".into())))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::SideTab;
    use bim_gis::pdok::Place;

    fn words() -> Words {
        Words { model: "Map".into(), layer: "Background map".into(), source: "test".into() }
    }

    /// A picture of 200 m around a place: its west half red, its east half blue.
    fn halves(center: Rd) -> (Picture, Bounds) {
        let bounds = Bounds::around(center, 100.0);
        let pixels = (0..4).flat_map(|_| [[255, 0, 0], [255, 0, 0], [0, 0, 255], [0, 0, 255]]).collect();
        (Picture::new(bounds, 4, 4, pixels).unwrap(), bounds)
    }

    #[test]
    fn a_flat_map_covers_the_square_in_the_colours_of_the_picture() {
        let center = Rd::new(155_000.0, 463_000.0);
        let (picture, bounds) = halves(center);
        let model = map_model(&picture, &Frame::at(center), bounds, None, &words());
        let layer = &model.layers[0];
        let (low, high) = layer.positions.chunks_exact(3).fold(([f32::MAX; 3], [f32::MIN; 3]), |(low, high), p| (std::array::from_fn(|k| low[k].min(p[k])), std::array::from_fn(|k| high[k].max(p[k]))));
        // 100 m to each side of the place, a little under the zero level
        assert_eq!((low[0], low[1], high[0], high[1]), (-100.0, -100.0, 100.0, 100.0));
        assert!(low[2] < 0.0 && low[2] > -0.1 && low[2] == high[2]);
        // west of the place it is red, east of it blue
        let colours = layer.colors.as_ref().unwrap();
        for (p, colour) in layer.positions.chunks_exact(3).zip(colours) {
            if p[0] < -30.0 {
                assert_eq!(colour, &[255, 0, 0, 255]);
            } else if p[0] > 30.0 {
                assert_eq!(colour, &[0, 0, 255, 255]);
            }
        }
        // one element that says where the picture comes from
        assert_eq!(layer.elements.len(), 1);
        assert!(model.props.values().any(|props| props.contains("\"Source\":\"test\"")));
    }

    #[test]
    fn a_map_on_a_terrain_takes_its_triangles_and_heights() {
        let center = Rd::new(155_000.0, 463_000.0);
        let (picture, bounds) = halves(center);
        // a slope of 3 by 3 heights every 100 m: it reaches past the square of the map
        let heights: Vec<f32> = (0..3).flat_map(|_| [0.0, 1.0, 2.0]).collect();
        let tin = Tin::from_grid(3, 3, &heights, |c, r| [c as f64 * 100.0 - 100.0, 100.0 - r as f64 * 100.0]);
        let model = map_model(&picture, &Frame::at(center), bounds, Some(&tin), &words());
        let layer = &model.layers[0];
        assert_eq!(layer.triangle_count(), tin.triangles.len());
        // a little over the terrain, the east edge at 2 m
        let highest = layer.positions.chunks_exact(3).map(|p| p[2]).fold(f32::MIN, f32::max);
        assert!((highest - 2.0 - OVER as f32).abs() < 1e-5, "{highest}");
        // a square that holds only part of the terrain keeps the triangles within it
        let part = map_model(&picture, &Frame::at(center), Bounds::around(Rd::new(center.x - 50.0, center.y), 55.0), Some(&tin), &words());
        assert!(part.layers.first().map_or(0, Layer::triangle_count) < tin.triangles.len());
    }

    #[test]
    fn a_map_made_is_a_map_layer_at_the_zero_of_the_project() {
        let center = Rd::new(155_000.0, 463_000.0);
        let (picture, bounds) = halves(center);
        let frame = Frame::at(center);
        let mut app = App::default();
        app.gis.place = Some(Place { name: "Test".into(), kind: "adres".into(), rd: center });
        let model = Arc::new(map_model(&picture, &frame, bounds, None, &words()));
        let _ = app.update_map(Message::Done(Ok(Done { model: model.clone(), frame, pixel: 50.0 })));
        assert_eq!(app.side_tab, SideTab::Gis);
        assert!(app.is_map_layer(&model));
        assert_eq!(app.gis.zero, Some(center));
        assert_eq!(app.gis.map.note, Some(Note::Loaded(50.0)));
        // a service that fails says so and adds nothing
        let _ = app.update_map(Message::Done(Err("no network".into())));
        assert_eq!(app.gis.map.note, Some(Note::Failed("no network".into())));
    }

    #[test]
    fn an_own_service_needs_its_address_and_layer() {
        let mut app = App::default();
        app.gis.place = Some(Place { name: "Test".into(), kind: "adres".into(), rd: Rd::new(155_000.0, 463_000.0) });
        let _ = app.update_map(Message::Source(Source::Own));
        assert!(app.map_asked(Language::English).is_none());
        let _ = app.update_map(Message::Fetch(Language::English));
        assert!(matches!(app.gis.map.note, Some(Note::Failed(_))) && !app.gis.map.working);
        let _ = app.update_map(Message::Url("https://example.org/wms?request=GetCapabilities".into()));
        let _ = app.update_map(Message::LayerName("roads".into()));
        let asked = app.map_asked(Language::English).unwrap();
        assert_eq!(asked.wms, Wms::typed("https://example.org/wms", "roads"));
        assert_eq!(asked.words.model, "roads Test");
    }

    /// Asks the live services; left out by default.
    #[test]
    #[ignore = "asks the live map services"]
    fn the_live_services_give_a_map() {
        let center = Rd::new(154_792.0, 463_172.0);
        for wms in [None, Some(Wms::aerial())] {
            let asked = Asked { wms, words: words() };
            let done = fetch(asked, Bounds::around(center, 100.0), Frame::at(center), None).unwrap();
            assert!(done.pixel < 1.0, "{}", done.pixel);
            let colours = done.model.layers[0].colors.as_ref().unwrap();
            assert!(colours.iter().any(|c| c != &colours[0]), "a picture of one colour");
        }
    }
}
