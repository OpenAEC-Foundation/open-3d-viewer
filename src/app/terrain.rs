//! Terrain in the GIS tab: the ground around the project as a surface with its contours (design
//! in `docs/superpowers/specs/2026-11-18-civil-design.md`).
//!
//! Two ways to a surface, both through `bim-civil`:
//!
//! - the heights of the national height model around the place of the project, fetched as a grid;
//! - a surveyor's file of points, triangulated.
//!
//! A surface is added to the project as a model: the ground as one element, each contour a line
//! that can be picked and says its height. In the Dutch national grid it lands where the other
//! map layers do (see `gis`), with the project's zero level at the ground of the place unless a
//! model says at which height it stands; a file in coordinates of its own keeps them, moved to
//! its first point.

use super::{App, Message as AppMessage};
use crate::model::{Layer, Model};
use crate::theme::Palette;
use bim_civil::tin::{self, Input, Tin};
use bim_gis::{Frame, Rd};
use glam::DVec3;
use iced::widget::text::Wrapping;
use iced::widget::{button, column, row, text};
use iced::{Center, Element, Task};
use openaec_ui::Language;
use std::path::PathBuf;
use std::sync::Arc;

/// The contour intervals offered (metres).
pub const INTERVALS: [f64; 3] = [0.25, 0.5, 1.0];
/// The cells a fetched terrain has in all, about.
#[cfg(not(target_arch = "wasm32"))]
const CELLS: usize = 250_000;
/// How far over the ground a contour is drawn, so it shows on it (metres).
const OVER: f64 = 0.02;

/// What the terrain section holds.
#[derive(Debug, Clone)]
pub struct State {
    pub interval: f64,
    working: bool,
    note: Option<Note>,
}

impl Default for State {
    fn default() -> Self {
        State { interval: INTERVALS[1], working: false, note: None }
    }
}

/// What the last terrain came to.
#[derive(Debug, Clone, PartialEq)]
enum Note {
    Loaded(Facts),
    Failed(String),
}

/// A surface in numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Facts {
    pub points: usize,
    pub triangles: usize,
    /// The lowest and highest height as the source gives them (m).
    pub low: f64,
    pub high: f64,
    pub contours: usize,
    /// The lines of a points file that were no point.
    pub skipped: usize,
}

/// A surface made, ready to add to the project.
#[derive(Debug, Clone)]
pub struct Done {
    pub model: Arc<Model>,
    /// How the grid lay in the project when the surface was made; `None` for a file in
    /// coordinates of its own.
    pub frame: Option<Frame>,
    /// The height of the source that lies at the project's zero level (m).
    pub level: f64,
    pub facts: Facts,
}

/// The names a surface's layers get, in the language of the interface.
#[derive(Debug, Clone)]
pub struct Words {
    pub model: String,
    pub ground: String,
    pub contours: String,
}

#[derive(Debug, Clone)]
pub enum Message {
    Interval(f64),
    /// Fetch the ground around the place.
    Fetch(Language),
    /// Ask for a file of survey points.
    Pick(Language),
    /// The surface fetched or read; `None` when the dialog was closed.
    Done(Option<Result<Done, String>>),
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
/// The model of a surface: the ground as one element and its contours as lines. `level` is the
/// height of the source at the project's zero level: the surface's own heights are relative to
/// it, the properties say the source's heights.
pub fn surface_model(tin: &Tin, interval: f64, level: f64, source: &str, words: &Words, skipped: usize) -> (Model, Facts) {
    let (low, high) = tin.range().unwrap_or((0.0, 0.0));
    let positions: Vec<f32> = tin.points.iter().flat_map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect();
    let indices: Vec<u32> = tin.triangles.iter().flatten().copied().collect();
    let normals = crate::model::smooth_normals(&positions, &indices);
    let ground_props = serde_json::json!({
        "Name": words.ground,
        "Source": source,
        "Points": tin.points.len(),
        "Triangles": tin.triangles.len(),
        "Area (m²)": (tin.area() * 10.0).round() / 10.0,
        "Lowest (m)": round(low + level),
        "Highest (m)": round(high + level),
        "Height at the zero level (m)": round(level),
    });
    let count = indices.len() as u32;
    let ground = Layer {
        name: words.ground.clone(),
        color: [0.62, 0.66, 0.50],
        positions,
        normals,
        indices,
        elements: vec![(1, 0, count)],
        props: vec![(1, ground_props.to_string())],
        ..Layer::default()
    };
    // the contours: each level a curve of its own, a little over the ground
    let mut lines = Layer { name: words.contours.clone(), color: [0.33, 0.27, 0.18], ..Layer::default() };
    let levels = tin.contours(interval);
    for (k, (height, chains)) in levels.iter().enumerate() {
        let element = k as u64 + 2;
        let first = lines.lines.len() as u32;
        for chain in chains {
            let start = lines.vertex_count() as u32;
            for p in chain {
                lines.positions.extend([p[0] as f32, p[1] as f32, (height + OVER) as f32]);
                lines.normals.extend([0.0, 0.0, 1.0]);
            }
            for i in 0..chain.len().saturating_sub(1) as u32 {
                lines.lines.extend([start + i, start + i + 1]);
            }
        }
        let count = lines.lines.len() as u32 - first;
        if count > 0 {
            lines.line_elements.push((element, first, count));
            lines.props.push((element, serde_json::json!({ "Name": words.contours, "Height (m)": round(height + level), "Source": source }).to_string()));
        }
    }
    let facts = Facts { points: tin.points.len(), triangles: tin.triangles.len(), low: low + level, high: high + level, contours: lines.line_elements.len(), skipped };
    (Model::new(PathBuf::from(&words.model), "Terrain", vec![ground, lines]), facts)
}

/// A height to the millimetre, for a property.
fn round(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// The surface of a grid of heights of the national grid, in the coordinates of `frame`. `level`
/// is the height at the project's zero level when a model says it; else the ground at the zero
/// of the frame takes it (or, with that outside the grid, the middle of the grid).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn of_grid(grid: &bim_gis::heights::Grid, frame: &Frame, level: Option<f64>, interval: f64, words: &Words) -> Result<Done, String> {
    if grid.known() == 0 {
        return Err("the height model has no heights here".into());
    }
    let mut heights = grid.heights.clone();
    tin::fill_gaps(grid.cols, grid.rows, &mut heights);
    let cell_of = |rd: Rd| {
        let (c, r) = ((rd.x - grid.west) / grid.cell[0], (grid.north - rd.y) / grid.cell[1]);
        (c >= 0.0 && r >= 0.0 && (c as usize) < grid.cols && (r as usize) < grid.rows).then(|| r as usize * grid.cols + c as usize)
    };
    let level = level.unwrap_or_else(|| f64::from(heights[cell_of(frame.origin).unwrap_or(grid.rows / 2 * grid.cols + grid.cols / 2)]));
    let relative: Vec<f32> = heights.iter().map(|z| (f64::from(*z) - level) as f32).collect();
    let tin = Tin::from_grid(grid.cols, grid.rows, &relative, |c, r| frame.local(grid.point(c, r)));
    let (model, facts) = surface_model(&tin, interval, level, bim_gis::heights::SOURCE, words, 0);
    Ok(Done { model: Arc::new(model), frame: Some(*frame), level, facts })
}

/// The surface of a surveyor's file of points. Points in the Dutch national grid go through
/// `frame` (how the grid lies in the project) and `level`, as fetched heights do; other points
/// keep their coordinates, moved to the first of them.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn of_points(text: &str, file: &str, frame: &Frame, level: Option<f64>, interval: f64, words: &Words) -> Result<Done, String> {
    let parsed = bim_civil::points::parse(text, bim_civil::points::Order::Auto);
    let Some(first) = parsed.points.first() else { return Err(format!("{file}: no points with a height found")) };
    let in_grid = parsed.points.iter().all(|p| Rd::new(p.x, p.y).valid());
    let level = if in_grid { level.unwrap_or(first.z) } else { 0.0 };
    let points: Vec<[f64; 3]> = parsed
        .points
        .iter()
        .map(|p| if in_grid { frame.local(Rd::new(p.x, p.y)) } else { [p.x - first.x, p.y - first.y] })
        .zip(&parsed.points)
        .map(|([x, y], p)| [x, y, p.z - level])
        .collect();
    let built = Tin::build(&Input { points: &points, ..Input::default() }).map_err(|e| format!("{file}: {e}"))?;
    let (model, facts) = surface_model(&built.tin, interval, level, file, words, parsed.skipped);
    Ok(Done { model: Arc::new(model), frame: in_grid.then_some(*frame), level, facts })
}

impl App {
    pub(super) fn update_terrain(&mut self, message: Message) -> Task<AppMessage> {
        match message {
            Message::Interval(interval) => self.gis.terrain.interval = interval,
            Message::Fetch(l) => {
                let Some(center) = self.gis_center().filter(|_| !self.gis.terrain.working) else { return Task::none() };
                let (frame, _) = self.gis_frame(center);
                self.gis.terrain.working = true;
                self.gis.terrain.note = None;
                let words = self.terrain_words(l, None);
                let bounds = bim_gis::pdok::Bounds::around(center, f64::from(self.gis.radius));
                return fetch_task(bounds, frame, self.gis_level(), self.gis.terrain.interval, words);
            }
            Message::Pick(l) => {
                if self.gis.terrain.working {
                    return Task::none();
                }
                // the frame a file in the national grid lands in: the project's, or one at its own points
                let frame = self.gis_center().map(|center| self.gis_frame(center).0);
                let words = self.terrain_words(l, Some(l.text("Oppervlak", "Surface")));
                self.gis.terrain.working = true;
                return pick_task(frame, self.gis_level(), self.gis.terrain.interval, words);
            }
            Message::Done(done) => {
                self.gis.terrain.working = false;
                match done {
                    Some(Ok(done)) => self.take_surface(done),
                    Some(Err(why)) => self.gis.terrain.note = Some(Note::Failed(why)),
                    None => {}
                }
            }
        }
        Task::none()
    }

    /// For the command line (`--gis PLACE`): the terrain around the place chosen, fetched and
    /// added. Waits for the service.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn open_terrain(&mut self, l: Language) -> Result<(), String> {
        let Some(center) = self.gis_center() else { return Ok(()) };
        let (frame, _) = self.gis_frame(center);
        let bounds = bim_gis::pdok::Bounds::around(center, f64::from(self.gis.radius));
        let done = fetch(bounds, frame, self.gis_level(), self.gis.terrain.interval, self.terrain_words(l, None))?;
        self.take_surface(done);
        Ok(())
    }

    /// The names of a surface's model and layers: the model after the place, or after `named`.
    fn terrain_words(&self, l: Language, named: Option<&str>) -> Words {
        let place = self.gis.place.as_ref().map_or_else(String::new, |place| place.name.clone());
        let model = match named {
            Some(name) => name.to_string(),
            None => super::gis::file_name(l.text("Terrein", "Terrain"), &place).trim_end_matches(".geojson").trim().to_string(),
        };
        Words { model, ground: l.text("Terrein", "Terrain").to_string(), contours: l.text("Hoogtelijnen", "Contours").to_string() }
    }

    /// Adds a surface to the project: in the national grid where the map layers lie, else as
    /// its file has it.
    fn take_surface(&mut self, done: Done) {
        let first_model = self.models.is_empty();
        let georeferenced = self.georeferenced();
        let id = done.model.id;
        self.add_model(done.model.clone());
        if let Some(frame) = done.frame {
            // a map layer: no zero point of its own, and it sets the project's zero where no model does
            self.gis.fetched.push(id);
            if let Some(entry) = self.models.last_mut() {
                entry.placed.moved = georeferenced.map_or(DVec3::ZERO, |(_, to_scene)| to_scene);
            }
            if georeferenced.is_none() {
                self.gis.zero = Some(frame.origin);
                self.gis.zero_level = Some(done.level);
            }
            self.replace_models();
        }
        if first_model {
            self.fit();
        }
        self.side_tab = super::SideTab::Gis;
        self.gis.terrain.note = Some(Note::Loaded(done.facts));
    }

    /// The terrain section of the GIS tab.
    pub(super) fn terrain_section(&self, p: Palette, l: Language) -> Element<'_, AppMessage> {
        let section = |title: &str| text(title.to_uppercase()).size(11).font(super::FONT_BOLD).color(p.group_label);
        let note = |words: String| text(words).size(11).color(p.text_muted).wrapping(Wrapping::WordOrGlyph);
        let plain = |words: String| text(words).size(12).color(p.text).wrapping(Wrapping::WordOrGlyph);
        let state = &self.gis.terrain;
        let mut rows = column![section(l.text("Terrein", "Terrain"))].spacing(6);
        if cfg!(target_arch = "wasm32") {
            return rows.push(note(l.text("De browserversie haalt nog geen terrein op.", "The browser build does not fetch terrain yet.").to_string())).into();
        }
        let mut intervals = row![text(l.text("Hoogtelijnen", "Contours")).size(12).color(p.text_secondary)].spacing(4).align_y(Center);
        for interval in INTERVALS {
            let chosen = state.interval == interval;
            intervals = intervals.push(
                button(text(format!("{interval} m")).size(12)).padding([3, 8]).style(move |_, s| p.ribbon_button(chosen, s)).on_press(AppMessage::Gis(super::gis::Message::Terrain(Message::Interval(interval)))),
            );
        }
        rows = rows.push(intervals);
        let ready = !state.working;
        let send = |message: Message| AppMessage::Gis(super::gis::Message::Terrain(message));
        rows = rows.push(
            button(text(if state.working { l.text("Terrein wordt opgehaald…", "Fetching the terrain…") } else { l.text("Terrein laden", "Load terrain") }).size(12))
                .padding([4, 12])
                .style(move |_, s| p.primary_button(s))
                .on_press_maybe((ready && self.gis_center().is_some()).then(|| send(Message::Fetch(l)))),
        );
        rows = rows.push(
            button(text(l.text("Oppervlak uit puntenbestand…", "Surface from a points file…")).size(12))
                .padding([4, 12])
                .style(move |_, s| p.ribbon_button(false, s))
                .on_press_maybe(ready.then(|| send(Message::Pick(l)))),
        );
        match &state.note {
            Some(Note::Loaded(facts)) => {
                rows = rows.push(plain(format!("{}: {:.2} – {:.2} m", l.text("Terrein", "Terrain"), facts.low, facts.high)));
                rows = rows.push(plain(format!("{}: {}", l.text("Hoogtelijnen", "Contours"), facts.contours)));
                if facts.skipped > 0 {
                    rows = rows.push(note(format!("{}: {}", l.text("Regels zonder punt overgeslagen", "Lines without a point left out"), facts.skipped)));
                }
            }
            Some(Note::Failed(why)) => rows = rows.push(note(why.clone())),
            None => {}
        }
        rows = rows.push(note(
            l.text("Terrein laden haalt de hoogtes van het maaiveld uit het Actueel Hoogtebestand Nederland (AHN) via PDOK, voor de plek en straal hierboven. Onder gebouwen en water heeft het geen hoogte: daar is het terrein aangevuld vanaf de rand.", "Load terrain fetches the ground heights from the national height model of the Netherlands (AHN) through PDOK, for the place and radius above. Under buildings and water it has no height: there the terrain is filled in from the edge.").to_string(),
        ));
        rows.into()
    }
}

/// The ground in `bounds` as a surface in the coordinates of `frame`.
#[cfg(not(target_arch = "wasm32"))]
fn fetch(bounds: bim_gis::pdok::Bounds, frame: Frame, level: Option<f64>, interval: f64, words: Words) -> Result<Done, String> {
    let side = bounds.max.x - bounds.min.x;
    let cells = bim_gis::heights::cells(side, CELLS);
    let answer = super::gis::get_bytes(&bim_gis::heights::coverage_url(bounds, cells, cells))?;
    of_grid(&bim_gis::heights::parse(&answer)?, &frame, level, interval, &words)
}

/// Fetches the terrain on a worker thread.
#[cfg(not(target_arch = "wasm32"))]
fn fetch_task(bounds: bim_gis::pdok::Bounds, frame: Frame, level: Option<f64>, interval: f64, words: Words) -> Task<AppMessage> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(fetch(bounds, frame, level, interval, words));
            });
            rx.await.unwrap_or_else(|_| Err("the terrain did not come back".into()))
        },
        |result| AppMessage::Gis(super::gis::Message::Terrain(Message::Done(Some(result)))),
    )
}

/// Asks for a file of survey points (the native dialog), then makes its surface on a worker
/// thread; nothing when the dialog is closed.
#[cfg(not(target_arch = "wasm32"))]
fn pick_task(frame: Option<Frame>, level: Option<f64>, interval: f64, mut words: Words) -> Task<AppMessage> {
    let dialog = rfd::AsyncFileDialog::new().set_title("Survey points").add_filter("CSV, TXT, XYZ, PNT", &["csv", "txt", "xyz", "pnt", "asc"]);
    Task::perform(
        async move {
            let file = dialog.pick_file().await?;
            let (name, path) = (file.file_name(), file.path().to_path_buf());
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                words.model = format!("{} {name}", words.model);
                let made = std::fs::read(&path).map_err(|e| format!("{name}: {e}")).and_then(|bytes| {
                    let text = String::from_utf8_lossy(&bytes);
                    // a file in the national grid without a project place: the frame lies at its first point
                    let own = bim_civil::points::parse(&text, bim_civil::points::Order::Auto).points.first().map(|p| Frame::at(Rd::new(p.x, p.y)));
                    let frame = frame.or(own).unwrap_or(Frame::at(Rd::new(0.0, 0.0)));
                    of_points(&text, &name, &frame, level, interval, &words)
                });
                let _ = tx.send(made);
            });
            Some(rx.await.unwrap_or_else(|_| Err("the surface was not made".into())))
        },
        |result| AppMessage::Gis(super::gis::Message::Terrain(Message::Done(result))),
    )
}

/// The browser build does not fetch or pick yet.
#[cfg(target_arch = "wasm32")]
fn fetch_task(_: bim_gis::pdok::Bounds, _: Frame, _: Option<f64>, _: f64, _: Words) -> Task<AppMessage> {
    Task::done(AppMessage::Gis(super::gis::Message::Terrain(Message::Done(None))))
}

#[cfg(target_arch = "wasm32")]
fn pick_task(_: Option<Frame>, _: Option<f64>, _: f64, _: Words) -> Task<AppMessage> {
    Task::done(AppMessage::Gis(super::gis::Message::Terrain(Message::Done(None))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::SideTab;
    use bim_gis::heights::Grid;
    use bim_gis::pdok::Place;

    fn words() -> Words {
        Words { model: "Terrain".into(), ground: "Terrain".into(), contours: "Contours".into() }
    }

    /// A grid of 11 by 11 heights every 10 m around a place: one metre up every ten to the east,
    /// from 2 m above NAP at the west edge, and a gap (a building) in the middle.
    fn grid(center: Rd) -> Grid {
        let mut heights: Vec<f32> = (0..11).flat_map(|_| (0..11).map(|c| 2.0 + c as f32)).collect();
        heights[5 * 11 + 5] = f32::NAN;
        Grid { west: center.x - 55.0, north: center.y + 55.0, cell: [10.0, 10.0], cols: 11, rows: 11, heights }
    }

    /// The lowest and highest place of what the scene draws of the model at `index`.
    fn extent(app: &App, index: usize) -> ([f32; 3], [f32; 3]) {
        let scene = app.model.as_ref().unwrap();
        let mut ends = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for layer in &scene.layers[app.models[index].layers.clone()] {
            for p in layer.positions.chunks_exact(3) {
                for k in 0..3 {
                    ends.0[k] = ends.0[k].min(p[k]);
                    ends.1[k] = ends.1[k].max(p[k]);
                }
            }
        }
        ends
    }

    #[test]
    fn a_grid_of_heights_becomes_ground_with_contours_around_the_zero_of_the_project() {
        let center = Rd::new(154_800.0, 463_200.0);
        let done = of_grid(&grid(center), &Frame::at(center), None, 1.0, &words()).unwrap();
        // the ground at the place is the zero level: 7 m above NAP there (the gap is filled)
        assert!((done.level - 7.0).abs() < 1e-6 || (done.level - 6.0).abs() < 1e-6, "{}", done.level);
        assert_eq!((done.facts.points, done.facts.triangles), (121, 200));
        assert_eq!((done.facts.low, done.facts.high), (2.0, 12.0));
        // every whole metre of the source between its ends: 2 up to 11
        assert_eq!(done.facts.contours, 10);
        let mut app = App::default();
        app.gis.place = Some(Place { name: "Hier".into(), kind: "adres".into(), rd: center });
        let _ = app.update(AppMessage::Gis(super::super::gis::Message::Terrain(Message::Done(Some(Ok(done.clone()))))));
        assert_eq!((app.models.len(), app.side_tab), (1, SideTab::Gis));
        assert_eq!(app.gis.terrain.note, Some(Note::Loaded(done.facts)));
        // 100 by 100 m around the place, from 5 m below to 5 m above the zero level (and the contours a little over it)
        let (low, high) = extent(&app, 0);
        assert!((low[0] + 50.0).abs() < 1e-3 && (high[0] - 50.0).abs() < 1e-3 && (low[1] + 50.0).abs() < 1e-3, "{low:?} {high:?}");
        assert!((f64::from(low[2]) - (2.0 - done.level)).abs() < 1e-3 && (f64::from(high[2]) - (12.0 - done.level)).abs() < 0.05, "{low:?} {high:?}");
        // the properties say the heights of the source; a contour says its own
        let scene = app.model.as_ref().unwrap();
        assert!(scene.props.values().any(|props| props.contains("\"Lowest (m)\":2.0") && props.contains("\"Highest (m)\":12.0")), "{:?}", scene.props.values().next());
        assert!(scene.props.values().any(|props| props.contains("\"Height (m)\":5.0")));
        // it is a map layer: no zero point of its own, and the next fetch keeps this zero and level
        assert!(app.zero_points().is_empty());
        assert_eq!(app.gis_level(), Some(done.level));
        assert_eq!(app.gis_frame(Rd::new(center.x + 300.0, center.y)).0, Frame::at(center));
    }

    #[test]
    fn a_model_that_says_its_height_takes_the_terrain_at_that_height() {
        // the project's zero level lies 3 m above NAP: the ground runs from 1 m below it to 9 m above
        let center = Rd::new(154_800.0, 463_200.0);
        let done = of_grid(&grid(center), &Frame::at(center), Some(3.0), 1.0, &words()).unwrap();
        assert_eq!(done.level, 3.0);
        let tin_low = done.model.layers[0].positions.chunks_exact(3).map(|p| p[2]).fold(f32::INFINITY, f32::min);
        assert!((tin_low + 1.0).abs() < 1e-6, "{tin_low}");
        // a grid without any height is no terrain
        let mut empty = grid(center);
        empty.heights.iter_mut().for_each(|z| *z = f32::NAN);
        assert!(of_grid(&empty, &Frame::at(center), None, 1.0, &words()).is_err());
    }

    #[test]
    fn a_points_file_becomes_a_surface_in_its_own_coordinates_or_in_the_grid() {
        // local coordinates: moved to the first point, heights as in the file
        let local = "1,1000,2000,10.0,TOPO\n2,1020,2000,12.0,TOPO\n3,1020,2020,12.0,TOPO\n4,1000,2020,10.0,TOPO\nnot a point\n";
        let done = of_points(local, "survey.csv", &Frame::at(Rd::new(0.0, 0.0)), None, 0.5, &words()).unwrap();
        assert_eq!((done.frame, done.level, done.facts.points, done.facts.triangles, done.facts.skipped), (None, 0.0, 4, 2, 1));
        assert_eq!((done.facts.low, done.facts.high, done.facts.contours), (10.0, 12.0, 4));
        let ground = &done.model.layers[0];
        let east = ground.positions.chunks_exact(3).map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max);
        assert_eq!(east, 20.0);
        // in the national grid: through the frame of the project, the first point at the zero level
        let grid = "155000 463000 1.5\n155020 463000 2.5\n155020 463020 2.5\n155000 463020 1.5\n";
        let frame = Frame::at(Rd::new(154_990.0, 462_990.0));
        let done = of_points(grid, "terrein.xyz", &frame, None, 0.5, &words()).unwrap();
        assert_eq!((done.frame, done.level), (Some(frame), 1.5));
        let ground = &done.model.layers[0];
        let (west, low) = ground.positions.chunks_exact(3).fold((f32::INFINITY, f32::INFINITY), |(w, l), p| (w.min(p[0]), l.min(p[2])));
        assert_eq!((west, low), (10.0, 0.0));
        // too few points, or none
        assert!(of_points("1,0,0,1\n2,1,1,1\n", "two.csv", &frame, None, 0.5, &words()).unwrap_err().starts_with("two.csv"));
        assert!(of_points("nothing here\n", "none.csv", &frame, None, 0.5, &words()).is_err());
    }

    #[test]
    fn the_gis_tab_offers_the_terrain() {
        let mut app = App::default();
        let _ = app.update(AppMessage::SideTab(SideTab::Gis));
        {
            let mut ui = crate::app::test_ui(&app, iced::Size::new(1400.0, 900.0));
            assert!(ui.find("Load terrain").is_ok() && ui.find("Surface from a points file…").is_ok());
            // no place yet: nothing to fetch around
            let _ = ui.click("Load terrain");
            assert!(ui.into_messages().next().is_none());
        }
        app.gis.place = Some(Place { name: "Hier".into(), kind: "adres".into(), rd: Rd::new(154_800.0, 463_200.0) });
        let mut ui = crate::app::test_ui(&app, iced::Size::new(1400.0, 900.0));
        ui.click("1 m").expect("the interval");
        ui.click("Load terrain").expect("the place is chosen");
        let messages: Vec<AppMessage> = ui.into_messages().collect();
        use super::super::gis::Message as Gis;
        assert!(matches!(messages[..], [AppMessage::Gis(Gis::Terrain(Message::Interval(i))), AppMessage::Gis(Gis::Terrain(Message::Fetch(_)))] if i == 1.0), "{messages:?}");
        let _ = app.update(AppMessage::Gis(Gis::Terrain(Message::Interval(1.0))));
        assert_eq!(app.gis.terrain.interval, 1.0);
    }

    #[test]
    #[ignore = "asks the live services of PDOK"]
    fn the_live_height_service_gives_the_ground_of_a_place() {
        let center = Rd::new(154_792.0, 463_172.0);
        let done = fetch(bim_gis::pdok::Bounds::around(center, 100.0), Frame::at(center), None, 0.5, words()).unwrap();
        // 200 m at half a metre: 400 cells a side, a few metres above NAP
        assert_eq!(done.facts.points, 160_000);
        assert!(done.facts.low > -5.0 && done.facts.high < 25.0 && done.facts.contours >= 2, "{:?}", done.facts);
    }
}
