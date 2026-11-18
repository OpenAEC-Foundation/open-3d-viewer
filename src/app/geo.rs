//! Geotechnics: the ground under the project as it was investigated (the Geotechnics tab of the
//! right pane; design in `docs/superpowers/specs/2026-11-18-civil-design.md`).
//!
//! Cone penetration tests come two ways, both read by `bim-civil`:
//!
//! - from the Dutch register of the subsurface, the tests nearest to the place of the project;
//! - from GEF files of a soil investigation.
//!
//! They are added to the project as one model. Each test stands where it was made, from the
//! ground down: a column in the colours of the cone resistance measured, and beside it the
//! resistance as a line against the depth. A test can be picked and says what its file says.
//! Nothing is derived: no soil is named and no layer is drawn between tests.
//!
//! A request tells the register which place is looked at; nothing of the model goes along.

use super::{App, Message as AppMessage};
use crate::model::{Layer, Model};
use crate::theme::Palette;
use bim_civil::ground::Sounding;
use bim_gis::{Frame, Rd};
use glam::DVec3;
use iced::widget::text::Wrapping;
use iced::widget::{button, column, text};
use iced::{Element, Task};
use openaec_ui::Language;
use std::path::PathBuf;
use std::sync::Arc;

/// The tests fetched around a place, at most: the nearest.
#[cfg(not(target_arch = "wasm32"))]
const MOST: usize = 30;
/// The side of a test's column (m), and the depth one colour of it covers (m).
const SIDE: f64 = 0.4;
const STEP: f64 = 0.25;
/// The metres the line beside a column stands out per MPa of cone resistance.
pub const GRAPH: f64 = 0.1;
/// The cone resistance at which the colours end (MPa).
pub const FULL: f64 = 30.0;
/// How far apart tests without a place are set out (m).
const APART: f64 = 2.0;

/// What the geotechnics tab holds.
#[derive(Debug, Clone, Default)]
pub struct State {
    working: bool,
    note: Option<Note>,
}

#[derive(Debug, Clone, PartialEq)]
enum Note {
    /// The tests loaded; how many of them say no place; whether the area holds more than were
    /// fetched; the files or tests that could not be read.
    Loaded { count: usize, unplaced: usize, more: bool, failed: Vec<String> },
    /// Nothing found around the place.
    Nothing,
    Failed(String),
}

/// The tests read, ready to add to the project.
#[derive(Debug, Clone)]
pub struct Read {
    pub soundings: Vec<Sounding>,
    /// Where they come from, for their properties.
    pub source: String,
    /// The area holds more tests than were fetched.
    pub more: bool,
    /// What could not be read, and why.
    pub failed: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// Fetch the tests around the place.
    Fetch(Language),
    /// Ask for GEF files.
    Pick(Language),
    /// The tests read; `None` when the dialog was closed.
    Done(Language, Option<Result<Read, String>>),
}

/// The colour of a cone resistance (MPa): blue where the cone met little, through green and
/// yellow to red at [`FULL`] and over.
pub fn colour(cone: f64) -> [u8; 4] {
    const STOPS: [(f64, [f64; 3]); 4] = [(0.0, [70.0, 110.0, 200.0]), (5.0, [90.0, 170.0, 90.0]), (15.0, [235.0, 200.0, 70.0]), (FULL, [200.0, 60.0, 50.0])];
    let cone = cone.clamp(0.0, FULL);
    let upper = STOPS.iter().position(|(at, _)| *at >= cone).unwrap_or(STOPS.len() - 1).max(1);
    let ((from, a), (to, b)) = (STOPS[upper - 1], STOPS[upper]);
    let t = (cone - from) / (to - from);
    let mix: [f64; 3] = std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t);
    [mix[0].round() as u8, mix[1].round() as u8, mix[2].round() as u8, 255]
}

/// The names of the model and its layers, in the language of the interface.
#[derive(Debug, Clone)]
pub struct Words {
    pub model: String,
    pub columns: String,
    pub graphs: String,
}

/// The model of a set of tests in the coordinates of `frame`. `level` is the height above NAP at
/// the project's zero level; `ground` gives the height of the terrain at a place of the project
/// (from the zero level), for a test that does not say at which height it started.
pub fn soundings_model(soundings: &[Sounding], frame: &Frame, level: f64, ground: Option<&dyn Fn(f64, f64) -> Option<f64>>, source: &str, words: &Words) -> Model {
    let mut columns = Layer { name: words.columns.clone(), color: [0.6, 0.6, 0.6], colors: Some(Vec::new()), ..Layer::default() };
    let mut graphs = Layer { name: words.graphs.clone(), color: [0.15, 0.15, 0.15], ..Layer::default() };
    let mut unplaced = 0usize;
    for (k, sounding) in soundings.iter().enumerate() {
        // where it stands: its place in the grid, or set out in a row by the zero of the project
        let [x, y] = match sounding.place {
            Some([east, north]) => frame.local(Rd::new(east, north)),
            None => {
                unplaced += 1;
                [(unplaced - 1) as f64 * APART, 0.0]
            }
        };
        let top = match sounding.ground {
            Some(height) => height - level,
            None => ground.and_then(|ground| ground(x, y)).unwrap_or(0.0),
        };
        // the column: four sides per step of depth, in the colour of its mean resistance
        let first = columns.indices.len() as u32;
        let half = SIDE / 2.0;
        let corners = [[x - half, y - half], [x + half, y - half], [x + half, y + half], [x - half, y + half]];
        let normals = [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0f32]];
        for (from, to, mean) in sounding.steps(STEP) {
            let tint = colour(mean);
            for side in 0..4 {
                let (a, b) = (corners[side], corners[(side + 1) % 4]);
                let at = columns.vertex_count() as u32;
                for (corner, depth) in [(a, to), (b, to), (b, from), (a, from)] {
                    columns.positions.extend([corner[0] as f32, corner[1] as f32, (top - depth) as f32]);
                    columns.normals.extend(normals[side]);
                    columns.colors.as_mut().unwrap().push(tint);
                }
                columns.indices.extend([at, at + 1, at + 2, at, at + 2, at + 3]);
            }
        }
        let props = serde_json::json!({
            "Name": sounding.name,
            "Type": "Cone penetration test",
            "Source": source,
            "Date": sounding.date,
            "RD x (m)": sounding.place.map(|place| place[0]),
            "RD y (m)": sounding.place.map(|place| place[1]),
            "Ground level (m NAP)": sounding.ground,
            "Depth (m)": (sounding.depth() * 100.0).round() / 100.0,
            "Highest cone resistance (MPa)": (sounding.highest() * 1000.0).round() / 1000.0,
            "Measurements": sounding.rows.len(),
        })
        .to_string();
        let element = 2 * k as u64 + 1;
        let count = columns.indices.len() as u32 - first;
        if count > 0 {
            columns.elements.push((element, first, count));
            columns.props.push((element, props.clone()));
        }
        // the graph: the axis down the east side of the column, and the resistance out from it
        let first = graphs.lines.len() as u32;
        let start = graphs.vertex_count() as u32;
        let axis = x + half;
        for depth in [0.0, sounding.depth()] {
            graphs.positions.extend([axis as f32, y as f32, (top - depth) as f32]);
        }
        graphs.lines.extend([start, start + 1]);
        for (i, row) in sounding.rows.iter().enumerate() {
            graphs.positions.extend([(axis + row.cone.max(0.0) * GRAPH) as f32, y as f32, (top - row.depth) as f32]);
            if i > 0 {
                graphs.lines.extend([start + 1 + i as u32, start + 2 + i as u32]);
            }
        }
        graphs.normals.resize(graphs.positions.len(), 0.0);
        graphs.line_elements.push((element + 1, first, graphs.lines.len() as u32 - first));
        graphs.props.push((element + 1, props));
    }
    Model::new(PathBuf::from(&words.model), "Soundings", vec![columns, graphs])
}

impl App {
    pub(super) fn update_geo(&mut self, message: Message) -> Task<AppMessage> {
        match message {
            Message::Fetch(l) => {
                let Some(center) = self.gis_center().filter(|_| !self.gis.geo.working) else { return Task::none() };
                self.gis.geo.working = true;
                self.gis.geo.note = None;
                return fetch_task(center, f64::from(self.gis.radius), l);
            }
            Message::Pick(l) => {
                if self.gis.geo.working {
                    return Task::none();
                }
                self.gis.geo.working = true;
                return pick_task(l);
            }
            Message::Done(l, done) => {
                self.gis.geo.working = false;
                match done {
                    Some(Ok(read)) if read.soundings.is_empty() && read.failed.is_empty() => self.gis.geo.note = Some(Note::Nothing),
                    Some(Ok(read)) if read.soundings.is_empty() => self.gis.geo.note = Some(Note::Failed(read.failed.join("; "))),
                    Some(Ok(read)) => self.take_soundings(read, l),
                    Some(Err(why)) => self.gis.geo.note = Some(Note::Failed(why)),
                    None => {}
                }
            }
        }
        Task::none()
    }

    /// For the command line (`--soundings`): the tests around the place chosen, fetched and
    /// added. Waits for the register.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_soundings(&mut self, l: Language) -> Result<(), String> {
        let Some(center) = self.gis_center() else { return Ok(()) };
        let read = fetch(center, f64::from(self.gis.radius))?;
        if !read.soundings.is_empty() {
            self.take_soundings(read, l);
        }
        Ok(())
    }

    /// Adds the tests to the project as one model, each where the grid puts it.
    fn take_soundings(&mut self, read: Read, l: Language) {
        let first_model = self.models.is_empty();
        let georeferenced = self.georeferenced();
        // the frame of the project, or one at the first test that says where it stands
        let placed = read.soundings.iter().find_map(|sounding| sounding.place).map(|[x, y]| Rd::new(x, y));
        let center = self.gis_center().or(placed);
        let frame = center.map_or(Frame::at(Rd::new(0.0, 0.0)), |center| self.gis_frame(center).0);
        // the zero level: the project's, or the ground of the first test that says its height
        let own = read.soundings.iter().find_map(|sounding| sounding.ground);
        let level = self.gis_level().or(own).unwrap_or(0.0);
        let terrain = self.kept_terrain().map(|kept| kept.tin.clone());
        let ground = terrain.as_ref().map(|tin| move |x: f64, y: f64| tin.height(x, y));
        let place = self.gis.place.as_ref().map_or_else(String::new, |place| place.name.clone());
        let words = Words {
            model: super::gis::file_name(l.text("Sonderingen", "Soundings"), &place).trim_end_matches(".geojson").trim().to_string(),
            columns: l.text("Sonderingen", "Soundings").to_string(),
            graphs: l.text("Conusweerstand", "Cone resistance").to_string(),
        };
        let model = Arc::new(soundings_model(&read.soundings, &frame, level, ground.as_ref().map(|g| g as &dyn Fn(f64, f64) -> Option<f64>), &read.source, &words));
        self.gis.fetched.push(model.id);
        self.add_model(model);
        if let Some(entry) = self.models.last_mut() {
            entry.placed.moved = georeferenced.map_or(DVec3::ZERO, |(_, to_scene)| to_scene);
        }
        if georeferenced.is_none() && center.is_some() {
            self.gis.zero = Some(frame.origin);
            if self.gis.zero_level.is_none() || first_model {
                self.gis.zero_level = Some(level);
            }
        }
        self.replace_models();
        if first_model {
            self.fit();
        }
        self.side_tab = super::SideTab::Geo;
        let unplaced = read.soundings.iter().filter(|sounding| sounding.place.is_none()).count();
        self.gis.geo.note = Some(Note::Loaded { count: read.soundings.len(), unplaced, more: read.more, failed: read.failed });
    }

    /// The Geotechnics tab of the right pane.
    pub(super) fn geo_tab(&self, p: Palette, l: Language) -> Element<'_, AppMessage> {
        let section = |title: &str| text(title.to_uppercase()).size(11).font(super::FONT_BOLD).color(p.group_label);
        let note = |words: String| text(words).size(11).color(p.text_muted).wrapping(Wrapping::WordOrGlyph);
        let plain = |words: String| text(words).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph);
        let send = |message: Message| AppMessage::Gis(super::gis::Message::Geo(message));
        let state = &self.gis.geo;
        let mut rows = column![section(l.text("Sonderingen", "Soundings"))].spacing(6);
        if cfg!(target_arch = "wasm32") {
            return rows.push(note(l.text("De browserversie laadt nog geen sonderingen.", "The browser build does not load soundings yet.").to_string())).into();
        }
        let place = self.gis_center().is_some();
        rows = rows.push(
            button(text(if state.working { l.text("Bezig…", "Working…") } else { l.text("Sonderingen rond de plek laden", "Load soundings around the place") }).size(12))
                .padding([4, 12])
                .style(move |_, s| p.primary_button(s))
                .on_press_maybe((place && !state.working).then_some(send(Message::Fetch(l)))),
        );
        if !place {
            rows = rows.push(note(l.text("Kies eerst een plek in het tabblad GIS, of open een model met een georeferentie.", "Choose a place in the GIS tab first, or open a model with a georeference.").to_string()));
        }
        rows = rows.push(
            button(text(l.text("GEF-bestanden openen", "Open GEF files")).size(12))
                .padding([4, 12])
                .style(move |_, s| p.ribbon_button(false, s))
                .on_press_maybe((!state.working).then_some(send(Message::Pick(l)))),
        );
        match &state.note {
            Some(Note::Loaded { count, unplaced, more, failed }) => {
                rows = rows.push(plain(format!("{}: {count}", l.text("Sonderingen geladen", "Soundings loaded"))));
                if *unplaced > 0 {
                    rows = rows.push(note(format!("{unplaced} {}", l.text("zonder plaats in het bestand: uitgezet bij het nulpunt van het project.", "without a place in the file: set out by the zero point of the project."))));
                }
                if *more {
                    rows = rows.push(note(l.text("Het gebied heeft meer sonderingen; de dichtstbijzijnde zijn geladen.", "The area holds more soundings; the nearest were loaded.").to_string()));
                }
                for why in failed {
                    rows = rows.push(note(why.clone()));
                }
            }
            Some(Note::Nothing) => rows = rows.push(plain(l.text("Geen sonderingen gevonden rond deze plek.", "No soundings found around this place.").to_string())),
            Some(Note::Failed(why)) => rows = rows.push(note(why.clone())),
            None => {}
        }
        rows = rows.push(note(format!(
            "{} {FULL:.0} MPa. {} {:.0} MPa.",
            l.text("Elke sondering staat op zijn plek, van maaiveld naar beneden. De kleur van de kolom is de gemeten conusweerstand: blauw bij 0, via groen en geel naar rood bij", "Each sounding stands at its place, from the ground down. The colour of the column is the cone resistance measured: blue at 0, through green and yellow to red at"),
            l.text("De lijn ernaast is de conusweerstand tegen de diepte: een meter opzij is", "The line beside it is the cone resistance against the depth: a metre sideways is"),
            1.0 / GRAPH
        )));
        rows = rows.push(note(l.text("Sonderingen rond de plek komen uit de Basisregistratie Ondergrond (alleen Nederland). De vraag vertelt het register welke plek u bekijkt; verder gaat er niets mee.", "Soundings around the place come from the Dutch register of the subsurface (the Netherlands only). The request tells the register which place you are looking at; nothing else goes along.").to_string()));
        rows.into()
    }
}

/// The tests within `radius` metres of `center`, the nearest [`MOST`], read from the register.
#[cfg(not(target_arch = "wasm32"))]
fn fetch(center: Rd, radius: f64) -> Result<Read, String> {
    use std::io::Read as _;
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(30)).user_agent(concat!("OpenBIMSuite/", env!("CARGO_PKG_VERSION"))).build();
    let text = |response: ureq::Response| {
        let mut answer = String::new();
        response.into_reader().take(64 << 20).read_to_string(&mut answer).map(|_| answer).map_err(|e| format!("BRO: {e}"))
    };
    let answer = agent
        .post(&bim_gis::bro::search_url())
        .set("Content-Type", "application/json")
        .set("Accept", "application/xml")
        .send_string(&bim_gis::bro::search_body(center.to_wgs84(), radius))
        .map_err(|e| format!("BRO: {e}"))
        .and_then(text)?;
    let found = bim_gis::bro::parse_search(&answer)?;
    let more = found.len() > MOST;
    let (mut soundings, mut failed) = (Vec::new(), Vec::new());
    for test in bim_gis::bro::nearest(found, center, MOST) {
        let read = agent.get(&bim_gis::bro::test_url(&test.id)).call().map_err(|e| format!("BRO: {e}")).and_then(text).and_then(|xml| bim_civil::ground::bro(&xml));
        match read {
            Ok(sounding) => soundings.push(sounding),
            Err(why) => failed.push(format!("{}: {why}", test.id)),
        }
    }
    Ok(Read { soundings, source: bim_gis::bro::SOURCE.to_string(), more, failed })
}

/// Fetches the tests on a worker thread.
#[cfg(not(target_arch = "wasm32"))]
fn fetch_task(center: Rd, radius: f64, l: Language) -> Task<AppMessage> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(fetch(center, radius));
            });
            rx.await.unwrap_or_else(|_| Err("the soundings did not come back".into()))
        },
        move |result| AppMessage::Gis(super::gis::Message::Geo(Message::Done(l, Some(result)))),
    )
}

/// Asks for GEF files (the native dialog) and reads them; nothing when the dialog is closed.
#[cfg(not(target_arch = "wasm32"))]
fn pick_task(l: Language) -> Task<AppMessage> {
    let dialog = rfd::AsyncFileDialog::new().set_title("GEF").add_filter("GEF", &["gef", "GEF"]);
    Task::perform(
        async move {
            let files = dialog.pick_files().await?;
            let (mut soundings, mut failed) = (Vec::new(), Vec::new());
            for file in files {
                let name = file.file_name();
                match bim_civil::ground::gef(&String::from_utf8_lossy(&file.read().await), &name) {
                    Ok(sounding) => soundings.push(sounding),
                    Err(why) => failed.push(why),
                }
            }
            Some(Ok(Read { soundings, source: "GEF".into(), more: false, failed }))
        },
        move |result| AppMessage::Gis(super::gis::Message::Geo(Message::Done(l, result))),
    )
}

/// The browser build does not fetch or pick yet.
#[cfg(target_arch = "wasm32")]
fn fetch_task(_: Rd, _: f64, l: Language) -> Task<AppMessage> {
    Task::done(AppMessage::Gis(super::gis::Message::Geo(Message::Done(l, None))))
}

#[cfg(target_arch = "wasm32")]
fn pick_task(l: Language) -> Task<AppMessage> {
    Task::done(AppMessage::Gis(super::gis::Message::Geo(Message::Done(l, None))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::SideTab;
    use bim_civil::ground::Row;
    use bim_gis::pdok::Place;

    fn words() -> Words {
        Words { model: "Soundings".into(), columns: "Soundings".into(), graphs: "Cone resistance".into() }
    }

    /// A test of 2 m: soft to 1 m (1 MPa), then firm (20 MPa).
    fn sounding(name: &str, place: Option<[f64; 2]>, ground: Option<f64>) -> Sounding {
        let rows = (1..=8).map(|i| Row { depth: i as f64 * 0.25, cone: if i <= 4 { 1.0 } else { 20.0 }, friction: None, ratio: None }).collect();
        Sounding { name: name.into(), place, ground, date: Some("2020-03-09".into()), rows }
    }

    fn extent(layer: &Layer) -> ([f32; 3], [f32; 3]) {
        layer.positions.chunks_exact(3).fold(([f32::MAX; 3], [f32::MIN; 3]), |(low, high), p| (std::array::from_fn(|k| low[k].min(p[k])), std::array::from_fn(|k| high[k].max(p[k]))))
    }

    #[test]
    fn the_colours_run_from_blue_to_red() {
        assert_eq!(colour(0.0), [70, 110, 200, 255]);
        assert_eq!(colour(5.0), [90, 170, 90, 255]);
        assert_eq!(colour(FULL), [200, 60, 50, 255]);
        // over the end and under zero: the ends
        assert_eq!(colour(80.0), colour(FULL));
        assert_eq!(colour(-1.0), colour(0.0));
        // halfway between two stops: halfway their colours
        assert_eq!(colour(10.0), [163, 185, 80, 255]);
    }

    #[test]
    fn a_sounding_stands_at_its_place_from_the_ground_down() {
        let center = Rd::new(155_000.0, 463_000.0);
        // 10 m east and 20 m north of the zero of the project, the ground at 4 m where the zero level is 3 m
        let test = sounding("S01", Some([155_010.0, 463_020.0]), Some(4.0));
        let model = soundings_model(&[test], &Frame::at(center), 3.0, None, "test", &words());
        let columns = model.layers.iter().find(|layer| layer.name == "Soundings").unwrap();
        let (low, high) = extent(columns);
        assert!((low[0] - 9.8).abs() < 1e-4 && (high[0] - 10.2).abs() < 1e-4 && (low[1] - 19.8).abs() < 1e-4);
        // from 1 m over the zero level (less the first quarter metre, which has no measurement) to 2.25 m under the ground
        assert!((high[2] - 0.75).abs() < 1e-4 && (low[2] - (1.0 - 2.25)).abs() < 1e-4, "{low:?} {high:?}");
        // the soft and the firm part have their colours
        let colours = columns.colors.as_ref().unwrap();
        assert!(colours.contains(&colour(1.0)) && colours.contains(&colour(20.0)));
        // the line beside it reaches 2 m out at 20 MPa
        let graphs = model.layers.iter().find(|layer| layer.name == "Cone resistance").unwrap();
        let (_, far) = extent(graphs);
        assert!((far[0] - (10.2 + 20.0 * GRAPH as f32)).abs() < 1e-4);
        // the column and its line are elements that say what the test says
        assert_eq!((columns.elements.len(), graphs.line_elements.len()), (1, 1));
        assert!(model.props.values().all(|props| props.contains("\"Name\":\"S01\"") && props.contains("\"Ground level (m NAP)\":4.0")));
    }

    #[test]
    fn a_sounding_without_a_height_starts_at_the_terrain_and_one_without_a_place_by_the_zero() {
        let center = Rd::new(155_000.0, 463_000.0);
        let ground = |_: f64, _: f64| Some(1.5);
        let tests = [sounding("A", Some([155_010.0, 463_000.0]), None), sounding("B", None, None), sounding("C", None, None)];
        let model = soundings_model(&tests, &Frame::at(center), 0.0, Some(&ground), "test", &words());
        let columns = model.layers.iter().find(|layer| layer.name == "Soundings").unwrap();
        let (low, high) = extent(columns);
        assert!((high[2] - (1.5 - 0.25)).abs() < 1e-4);
        // the two without a place stand at the zero and 2 m beside it
        assert!((low[0] + 0.2).abs() < 1e-4 && (high[0] - 10.2).abs() < 1e-4);
        assert_eq!(columns.elements.len(), 3);
    }

    #[test]
    fn soundings_read_become_a_model_and_set_the_zero_of_an_empty_project() {
        let mut app = App::default();
        let read = Read { soundings: vec![sounding("S01", Some([155_010.0, 463_020.0]), Some(4.0))], source: "GEF".into(), more: false, failed: vec!["x.gef: not a GEF file".into()] };
        let _ = app.update_geo(Message::Done(Language::English, Some(Ok(read))));
        assert_eq!(app.side_tab, SideTab::Geo);
        assert_eq!(app.models.len().max(1), 1);
        // no place was chosen: the zero of the project lies at the test, its ground at the zero level
        assert_eq!(app.gis.zero, Some(Rd::new(155_010.0, 463_020.0)));
        assert_eq!(app.gis.zero_level, Some(4.0));
        assert_eq!(app.gis.geo.note, Some(Note::Loaded { count: 1, unplaced: 0, more: false, failed: vec!["x.gef: not a GEF file".into()] }));
        // nothing found says so; files that are no soundings say why
        let _ = app.update_geo(Message::Done(Language::English, Some(Ok(Read { soundings: Vec::new(), source: String::new(), more: false, failed: Vec::new() }))));
        assert_eq!(app.gis.geo.note, Some(Note::Nothing));
        let _ = app.update_geo(Message::Done(Language::English, Some(Ok(Read { soundings: Vec::new(), source: String::new(), more: false, failed: vec!["a".into(), "b".into()] }))));
        assert_eq!(app.gis.geo.note, Some(Note::Failed("a; b".into())));
    }

    #[test]
    fn the_geotechnics_tab_offers_the_soundings() {
        let mut app = App::default();
        app.side_tab = SideTab::Geo;
        let mut ui = crate::app::test_ui(&app, iced::Size::new(1400.0, 900.0));
        assert!(ui.find("Open GEF files").is_ok());
        assert!(ui.find("Load soundings around the place").is_ok());
        drop(ui);
        app.gis.place = Some(Place { name: "Test".into(), kind: "adres".into(), rd: Rd::new(155_000.0, 463_000.0) });
        assert!(app.gis_center().is_some());
    }

    /// Asks the live register; left out by default.
    #[test]
    #[ignore = "asks the live register of the subsurface"]
    fn the_live_register_gives_the_soundings_around_a_place() {
        let read = fetch(Rd::new(154_792.0, 463_172.0), 250.0).unwrap();
        assert!(!read.soundings.is_empty(), "{:?}", read.failed);
        assert!(read.soundings.iter().all(|sounding| sounding.place.is_some() && !sounding.rows.is_empty()));
    }
}
