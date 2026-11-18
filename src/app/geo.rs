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
use bim_civil::ground::{Boring, Soil, Sounding};
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
    /// The borings loaded, to make a model of the ground of.
    borings: Vec<Boring>,
    borings_note: Option<Note>,
    /// The blocks of the last model of the ground.
    modelled: Option<usize>,
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
    /// Fetch the borings around the place.
    FetchBorings(Language),
    /// Ask for GEF files of borings.
    PickBorings(Language),
    /// The borings read; `None` when the dialog was closed.
    Bored(Language, Option<Result<Bored, String>>),
    /// Make a model of the ground of the borings loaded.
    Model(Language),
}

/// The borings read, ready to add to the project.
#[derive(Debug, Clone)]
pub struct Bored {
    pub borings: Vec<Boring>,
    pub source: String,
    pub more: bool,
    pub failed: Vec<String>,
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

/// The side of a boring's column (m).
const BORING_SIDE: f64 = 0.6;
/// The cells a model of the ground has along its longer side.
const CELLS: usize = 48;

/// The colour of a main soil: gravel orange, sand yellow, silt pale olive, clay green, peat
/// brown, anything else grey.
pub fn soil_colour(soil: Soil) -> [u8; 4] {
    match soil {
        Soil::Gravel => [224, 150, 60, 255],
        Soil::Sand => [240, 215, 100, 255],
        Soil::Silt => [200, 200, 140, 255],
        Soil::Clay => [95, 150, 95, 255],
        Soil::Peat => [130, 90, 60, 255],
        Soil::Other => [160, 160, 160, 255],
    }
}

/// A main soil's name in the properties.
fn soil_word(soil: Soil) -> &'static str {
    match soil {
        Soil::Gravel => "Gravel",
        Soil::Sand => "Sand",
        Soil::Silt => "Silt or loam",
        Soil::Clay => "Clay",
        Soil::Peat => "Peat",
        Soil::Other => "Other",
    }
}

/// The sides of a block from `x0, y0` to `x1, y1` between two heights, those of `sides` (south,
/// east, north, west), and its top and bottom when asked, in one colour.
#[allow(clippy::too_many_arguments)]
fn block(layer: &mut Layer, [x0, y0]: [f64; 2], [x1, y1]: [f64; 2], high: f64, low: f64, sides: [bool; 4], top: bool, bottom: bool, tint: [u8; 4]) {
    let mut face = |corners: [[f64; 3]; 4], normal: [f32; 3]| {
        let at = layer.vertex_count() as u32;
        for corner in corners {
            layer.positions.extend(corner.map(|v| v as f32));
            layer.normals.extend(normal);
            layer.colors.as_mut().unwrap().push(tint);
        }
        layer.indices.extend([at, at + 1, at + 2, at, at + 2, at + 3]);
    };
    let ring = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    let normals = [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0f32]];
    for side in (0..4).filter(|side| sides[*side]) {
        let (a, b) = (ring[side], ring[(side + 1) % 4]);
        face([[a[0], a[1], low], [b[0], b[1], low], [b[0], b[1], high], [a[0], a[1], high]], normals[side]);
    }
    if top {
        face(ring.map(|p| [p[0], p[1], high]), [0.0, 0.0, 1.0]);
    }
    if bottom {
        face([ring[0], ring[3], ring[2], ring[1]].map(|p| [p[0], p[1], low]), [0.0, 0.0, -1.0]);
    }
}

/// Where a boring stands in the project and how high its ground lies there, as [`soundings_model`]
/// places a test; `unplaced` counts those set out by the zero.
fn stand(place: Option<[f64; 2]>, height: Option<f64>, frame: &Frame, level: f64, ground: Option<&dyn Fn(f64, f64) -> Option<f64>>, unplaced: &mut usize) -> ([f64; 2], f64) {
    let [x, y] = match place {
        Some([east, north]) => frame.local(Rd::new(east, north)),
        None => {
            *unplaced += 1;
            [(*unplaced - 1) as f64 * APART, -APART]
        }
    };
    let top = match height {
        Some(height) => height - level,
        None => ground.and_then(|ground| ground(x, y)).unwrap_or(0.0),
    };
    ([x, y], top)
}

/// What a described layer says, as the properties of its block.
fn layer_props(boring: &Boring, layer: &bim_civil::ground::Stratum, source: &str, kind: &str) -> String {
    serde_json::json!({
        "Name": if layer.name.is_empty() { "?" } else { layer.name.as_str() },
        "Type": kind,
        "Main soil": soil_word(layer.soil()),
        "Boring": boring.name,
        "From (m below ground)": layer.top,
        "To (m below ground)": layer.bottom,
        "Top (m NAP)": boring.ground.map(|ground| ((ground - layer.top) * 100.0).round() / 100.0),
        "Bottom (m NAP)": boring.ground.map(|ground| ((ground - layer.bottom) * 100.0).round() / 100.0),
        "Date": boring.date,
        "Source": source,
    })
    .to_string()
}

/// The model of a set of borings in the coordinates of `frame`: each a column from the ground
/// down, every described layer a block of it in the colour of its main soil, to be picked.
pub fn borings_model(borings: &[Boring], frame: &Frame, level: f64, ground: Option<&dyn Fn(f64, f64) -> Option<f64>>, source: &str, name: &str, layer_name: &str) -> Model {
    let mut columns = Layer { name: layer_name.to_string(), color: [0.6, 0.6, 0.6], colors: Some(Vec::new()), ..Layer::default() };
    let (mut unplaced, mut element) = (0usize, 0u64);
    let half = BORING_SIDE / 2.0;
    for boring in borings {
        let ([x, y], top) = stand(boring.place, boring.ground, frame, level, ground, &mut unplaced);
        for layer in &boring.layers {
            element += 1;
            let first = columns.indices.len() as u32;
            block(&mut columns, [x - half, y - half], [x + half, y + half], top - layer.top, top - layer.bottom, [true; 4], true, true, soil_colour(layer.soil()));
            columns.elements.push((element, first, columns.indices.len() as u32 - first));
            columns.props.push((element, layer_props(boring, layer, source, "Layer of a boring")));
        }
    }
    Model::new(PathBuf::from(name), "Borings", vec![columns])
}

/// A model of the ground between borings: every place takes the layers of the boring nearest to
/// it, from that boring's ground down. Nothing is laid between borings: where two borings
/// differ, the model steps. Over the square around the borings that stand in the grid, a
/// quarter of its size wider to each side; the blocks made are counted. `None` without such a
/// boring.
pub fn ground_model(borings: &[Boring], frame: &Frame, level: f64, ground: Option<&dyn Fn(f64, f64) -> Option<f64>>, name: &str, layer_name: &str) -> Option<(Model, usize)> {
    let mut unplaced = 0usize;
    let placed: Vec<(&Boring, [f64; 2], f64)> = borings.iter().filter(|boring| boring.place.is_some()).map(|boring| {
        let (at, top) = stand(boring.place, boring.ground, frame, level, ground, &mut unplaced);
        (boring, at, top)
    }).collect();
    let first = placed.first()?;
    let (mut low, mut high) = (first.1, first.1);
    for (_, at, _) in &placed {
        low = [low[0].min(at[0]), low[1].min(at[1])];
        high = [high[0].max(at[0]), high[1].max(at[1])];
    }
    let margin = ((high[0] - low[0]).max(high[1] - low[1]) * 0.25).max(10.0);
    let (low, high) = ([low[0] - margin, low[1] - margin], [high[0] + margin, high[1] + margin]);
    let cell = (high[0] - low[0]).max(high[1] - low[1]) / CELLS as f64;
    let (nx, ny) = ((((high[0] - low[0]) / cell).round() as usize).max(1), (((high[1] - low[1]) / cell).round() as usize).max(1));
    // the boring nearest to the middle of each cell
    let owner: Vec<usize> = (0..nx * ny)
        .map(|k| {
            let (cx, cy) = (low[0] + (k % nx) as f64 * cell + cell / 2.0, low[1] + (k / nx) as f64 * cell + cell / 2.0);
            let distance = |at: &[f64; 2]| (at[0] - cx).powi(2) + (at[1] - cy).powi(2);
            (0..placed.len()).min_by(|a, b| distance(&placed[*a].1).total_cmp(&distance(&placed[*b].1))).unwrap_or(0)
        })
        .collect();
    let owned = |c: i64, r: i64| (c >= 0 && r >= 0 && (c as usize) < nx && (r as usize) < ny).then(|| owner[r as usize * nx + c as usize]);
    let mut blocks = Layer { name: layer_name.to_string(), color: [0.6, 0.6, 0.6], colors: Some(Vec::new()), ..Layer::default() };
    let mut element = 0u64;
    for (b, (boring, _, top)) in placed.iter().enumerate() {
        for (k, layer) in boring.layers.iter().enumerate() {
            let start = blocks.indices.len() as u32;
            // a face between two layers that meet is inside: left out
            let open_top = k == 0 || (boring.layers[k - 1].bottom - layer.top).abs() > 1e-6;
            let open_bottom = boring.layers.get(k + 1).is_none_or(|next| (next.top - layer.bottom).abs() > 1e-6);
            for r in 0..ny as i64 {
                for c in 0..nx as i64 {
                    if owned(c, r) != Some(b) {
                        continue;
                    }
                    let corner = [low[0] + c as f64 * cell, low[1] + r as f64 * cell];
                    // a side shows where the cell beside it is another boring's, or the edge
                    let sides = [owned(c, r - 1), owned(c + 1, r), owned(c, r + 1), owned(c - 1, r)].map(|beside| beside != Some(b));
                    block(&mut blocks, corner, [corner[0] + cell, corner[1] + cell], top - layer.top, top - layer.bottom, sides, open_top, open_bottom, soil_colour(layer.soil()));
                }
            }
            if blocks.indices.len() as u32 > start {
                element += 1;
                blocks.elements.push((element, start, blocks.indices.len() as u32 - start));
                blocks.props.push((element, layer_props(boring, layer, "The layers of the nearest boring", "Ground model")));
            }
        }
    }
    (element > 0).then(|| (Model::new(PathBuf::from(name), "Ground model", vec![blocks]), element as usize))
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
            Message::FetchBorings(l) => {
                let Some(center) = self.gis_center().filter(|_| !self.gis.geo.working) else { return Task::none() };
                self.gis.geo.working = true;
                self.gis.geo.borings_note = None;
                return borings_task(center, f64::from(self.gis.radius), l);
            }
            Message::PickBorings(l) => {
                if self.gis.geo.working {
                    return Task::none();
                }
                self.gis.geo.working = true;
                return pick_borings_task(l);
            }
            Message::Bored(l, done) => {
                self.gis.geo.working = false;
                match done {
                    Some(Ok(read)) if read.borings.is_empty() && read.failed.is_empty() => self.gis.geo.borings_note = Some(Note::Nothing),
                    Some(Ok(read)) if read.borings.is_empty() => self.gis.geo.borings_note = Some(Note::Failed(read.failed.join("; "))),
                    Some(Ok(read)) => self.take_borings(read, l),
                    Some(Err(why)) => self.gis.geo.borings_note = Some(Note::Failed(why)),
                    None => {}
                }
            }
            Message::Model(l) => self.model_ground(l),
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

    /// How the grid lies in the project and the height at its zero level, for what was
    /// investigated: the project's, or at the first investigated point that says where it stands
    /// and how high; and the terrain to start from where a point does not say its height.
    fn ground_frame(&self, placed: Option<[f64; 2]>, own: Option<f64>) -> (Frame, f64, bool, Option<Arc<bim_civil::Tin>>) {
        let center = self.gis_center().or(placed.map(|[x, y]| Rd::new(x, y)));
        let frame = center.map_or(Frame::at(Rd::new(0.0, 0.0)), |center| self.gis_frame(center).0);
        (frame, self.gis_level().or(own).unwrap_or(0.0), center.is_some(), self.kept_terrain().map(|kept| kept.tin.clone()))
    }

    /// Adds a model of what was investigated as a map layer, and sets the zero of a project
    /// that has none yet.
    fn add_ground(&mut self, model: Arc<Model>, frame: Frame, level: f64, placed: bool) {
        let first_model = self.models.is_empty();
        let georeferenced = self.georeferenced();
        self.gis.fetched.push(model.id);
        self.add_model(model);
        if let Some(entry) = self.models.last_mut() {
            entry.placed.moved = georeferenced.map_or(DVec3::ZERO, |(_, to_scene)| to_scene);
        }
        if georeferenced.is_none() && placed {
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
    }

    /// The name of a model of what was investigated: what it is and the place.
    fn ground_name(&self, what: &str) -> String {
        let place = self.gis.place.as_ref().map_or_else(String::new, |place| place.name.clone());
        super::gis::file_name(what, &place).trim_end_matches(".geojson").trim().to_string()
    }

    /// Adds the tests to the project as one model, each where the grid puts it.
    fn take_soundings(&mut self, read: Read, l: Language) {
        let (frame, level, placed, terrain) = self.ground_frame(read.soundings.iter().find_map(|sounding| sounding.place), read.soundings.iter().find_map(|sounding| sounding.ground));
        let ground = terrain.as_ref().map(|tin| move |x: f64, y: f64| tin.height(x, y));
        let words = Words { model: self.ground_name(l.text("Sonderingen", "Soundings")), columns: l.text("Sonderingen", "Soundings").to_string(), graphs: l.text("Conusweerstand", "Cone resistance").to_string() };
        let model = Arc::new(soundings_model(&read.soundings, &frame, level, ground.as_ref().map(|g| g as &dyn Fn(f64, f64) -> Option<f64>), &read.source, &words));
        self.add_ground(model, frame, level, placed);
        let unplaced = read.soundings.iter().filter(|sounding| sounding.place.is_none()).count();
        self.gis.geo.note = Some(Note::Loaded { count: read.soundings.len(), unplaced, more: read.more, failed: read.failed });
    }

    /// Adds the borings to the project as one model, and keeps them to model the ground of.
    fn take_borings(&mut self, read: Bored, l: Language) {
        let (frame, level, placed, terrain) = self.ground_frame(read.borings.iter().find_map(|boring| boring.place), read.borings.iter().find_map(|boring| boring.ground));
        let ground = terrain.as_ref().map(|tin| move |x: f64, y: f64| tin.height(x, y));
        let name = self.ground_name(l.text("Boringen", "Borings"));
        let model = Arc::new(borings_model(&read.borings, &frame, level, ground.as_ref().map(|g| g as &dyn Fn(f64, f64) -> Option<f64>), &read.source, &name, l.text("Boringen", "Borings")));
        self.add_ground(model, frame, level, placed);
        let unplaced = read.borings.iter().filter(|boring| boring.place.is_none()).count();
        self.gis.geo.borings_note = Some(Note::Loaded { count: read.borings.len(), unplaced, more: read.more, failed: read.failed });
        self.gis.geo.borings.extend(read.borings);
        self.gis.geo.modelled = None;
    }

    /// Makes the model of the ground of the borings loaded and adds it to the project.
    fn model_ground(&mut self, l: Language) {
        let borings = self.gis.geo.borings.clone();
        let (frame, level, placed, terrain) = self.ground_frame(borings.iter().find_map(|boring| boring.place), borings.iter().find_map(|boring| boring.ground));
        let ground = terrain.as_ref().map(|tin| move |x: f64, y: f64| tin.height(x, y));
        let name = self.ground_name(l.text("Ondergrondmodel", "Ground model"));
        let Some((model, blocks)) = ground_model(&borings, &frame, level, ground.as_ref().map(|g| g as &dyn Fn(f64, f64) -> Option<f64>), &name, l.text("Ondergrond", "Ground")) else { return };
        self.add_ground(Arc::new(model), frame, level, placed);
        self.gis.geo.modelled = Some(blocks);
    }

    /// For the command line (`--borings`): the borings around the place chosen, fetched and
    /// added, with the model of the ground between them. Waits for the register.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_borings(&mut self, l: Language) -> Result<(), String> {
        let Some(center) = self.gis_center() else { return Ok(()) };
        let read = fetch_borings(center, f64::from(self.gis.radius))?;
        if !read.borings.is_empty() {
            self.take_borings(read, l);
            self.model_ground(l);
        }
        Ok(())
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
        rows = rows.push(iced::widget::rule::horizontal(1)).push(section(l.text("Boringen", "Borings")));
        rows = rows.push(
            button(text(if state.working { l.text("Bezig…", "Working…") } else { l.text("Boringen rond de plek laden", "Load borings around the place") }).size(12))
                .padding([4, 12])
                .style(move |_, s| p.primary_button(s))
                .on_press_maybe((place && !state.working).then_some(send(Message::FetchBorings(l)))),
        );
        rows = rows.push(
            button(text(l.text("GEF-bestanden van boringen openen", "Open GEF files of borings")).size(12))
                .padding([4, 12])
                .style(move |_, s| p.ribbon_button(false, s))
                .on_press_maybe((!state.working).then_some(send(Message::PickBorings(l)))),
        );
        match &state.borings_note {
            Some(Note::Loaded { count, unplaced, more, failed }) => {
                rows = rows.push(plain(format!("{}: {count}", l.text("Boringen geladen", "Borings loaded"))));
                if *unplaced > 0 {
                    rows = rows.push(note(format!("{unplaced} {}", l.text("zonder plaats in het bestand: uitgezet bij het nulpunt van het project.", "without a place in the file: set out by the zero point of the project."))));
                }
                if *more {
                    rows = rows.push(note(l.text("Het gebied heeft meer boringen; de dichtstbijzijnde zijn geladen.", "The area holds more borings; the nearest were loaded.").to_string()));
                }
                for why in failed {
                    rows = rows.push(note(why.clone()));
                }
            }
            Some(Note::Nothing) => rows = rows.push(plain(l.text("Geen boringen gevonden rond deze plek.", "No borings found around this place.").to_string())),
            Some(Note::Failed(why)) => rows = rows.push(note(why.clone())),
            None => {}
        }
        rows = rows.push(note(l.text("Elke beschreven laag is een blok van de kolom, aan te klikken. Kleur naar hoofdgrondsoort: grind oranje, zand geel, silt en leem vaalgroen, klei groen, veen bruin, overig grijs.", "Each described layer is a block of the column, to be picked. Coloured by main soil: gravel orange, sand yellow, silt and loam pale olive, clay green, peat brown, anything else grey.").to_string()));
        rows = rows.push(iced::widget::rule::horizontal(1)).push(section(l.text("Ondergrondmodel", "Ground model")));
        let with_place = state.borings.iter().filter(|boring| boring.place.is_some()).count();
        rows = rows.push(
            button(text(l.text("Ondergrondmodel maken", "Make ground model")).size(12))
                .padding([4, 12])
                .style(move |_, s| p.primary_button(s))
                .on_press_maybe((with_place > 0 && !state.working).then_some(send(Message::Model(l)))),
        );
        if let Some(blocks) = state.modelled {
            rows = rows.push(plain(format!("{}: {blocks} {} {with_place} {}", l.text("Gemaakt", "Made"), l.text("lagen uit", "layers of"), l.text("boringen", "borings"))));
        } else if with_place == 0 {
            rows = rows.push(note(l.text("Laad eerst boringen met een plaats.", "Load borings with a place first.").to_string()));
        }
        rows = rows.push(note(l.text("Elke plek krijgt de lagen van de dichtstbijzijnde boring, vanaf het maaiveld van die boring. Er wordt niets tussen boringen gelegd: waar twee boringen verschillen, verspringt het model. Het is een beeld van wat geboord is, geen bodemopbouw om op te rekenen.", "Every place takes the layers of the nearest boring, from that boring's ground down. Nothing is laid between borings: where two borings differ, the model steps. It shows what was bored; it is no soil profile to calculate with.").to_string()));
        rows = rows.push(note(l.text("Sonderingen en boringen rond de plek komen uit de Basisregistratie Ondergrond (alleen Nederland). De vraag vertelt het register welke plek u bekijkt; verder gaat er niets mee.", "Soundings and borings around the place come from the Dutch register of the subsurface (the Netherlands only). The request tells the register which place you are looking at; nothing else goes along.").to_string()));
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
        .post(&bim_gis::bro::search_url(bim_gis::bro::Register::Soundings))
        .set("Content-Type", "application/json")
        .set("Accept", "application/xml")
        .send_string(&bim_gis::bro::search_body(center.to_wgs84(), radius))
        .map_err(|e| format!("BRO: {e}"))
        .and_then(text)?;
    let found = bim_gis::bro::parse_search(&answer)?;
    let more = found.len() > MOST;
    let (mut soundings, mut failed) = (Vec::new(), Vec::new());
    for test in bim_gis::bro::nearest(found, center, MOST) {
        let read = agent.get(&bim_gis::bro::test_url(bim_gis::bro::Register::Soundings, &test.id)).call().map_err(|e| format!("BRO: {e}")).and_then(text).and_then(|xml| bim_civil::ground::bro(&xml));
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

/// The borings within `radius` metres of `center`, the nearest [`MOST`], read from the register.
#[cfg(not(target_arch = "wasm32"))]
fn fetch_borings(center: Rd, radius: f64) -> Result<Bored, String> {
    use bim_gis::bro::Register::Borings;
    use std::io::Read as _;
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(30)).user_agent(concat!("OpenBIMSuite/", env!("CARGO_PKG_VERSION"))).build();
    let text = |response: ureq::Response| {
        let mut answer = String::new();
        response.into_reader().take(64 << 20).read_to_string(&mut answer).map(|_| answer).map_err(|e| format!("BRO: {e}"))
    };
    let answer = agent
        .post(&bim_gis::bro::search_url(Borings))
        .set("Content-Type", "application/json")
        .set("Accept", "application/xml")
        .send_string(&bim_gis::bro::search_body(center.to_wgs84(), radius))
        .map_err(|e| format!("BRO: {e}"))
        .and_then(text)?;
    let found = bim_gis::bro::parse_search(&answer)?;
    let more = found.len() > MOST;
    let (mut borings, mut failed) = (Vec::new(), Vec::new());
    for boring in bim_gis::bro::nearest(found, center, MOST) {
        let read = agent.get(&bim_gis::bro::test_url(Borings, &boring.id)).call().map_err(|e| format!("BRO: {e}")).and_then(text).and_then(|xml| bim_civil::ground::boring_bro(&xml));
        match read {
            Ok(read) => borings.push(read),
            Err(why) => failed.push(format!("{}: {why}", boring.id)),
        }
    }
    Ok(Bored { borings, source: bim_gis::bro::BORINGS_SOURCE.to_string(), more, failed })
}

#[cfg(not(target_arch = "wasm32"))]
fn borings_task(center: Rd, radius: f64, l: Language) -> Task<AppMessage> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(fetch_borings(center, radius));
            });
            rx.await.unwrap_or_else(|_| Err("the borings did not come back".into()))
        },
        move |result| AppMessage::Gis(super::gis::Message::Geo(Message::Bored(l, Some(result)))),
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn pick_borings_task(l: Language) -> Task<AppMessage> {
    let dialog = rfd::AsyncFileDialog::new().set_title("GEF").add_filter("GEF", &["gef", "GEF"]);
    Task::perform(
        async move {
            let files = dialog.pick_files().await?;
            let (mut borings, mut failed) = (Vec::new(), Vec::new());
            for file in files {
                let name = file.file_name();
                match bim_civil::ground::boring_gef(&String::from_utf8_lossy(&file.read().await), &name) {
                    Ok(boring) => borings.push(boring),
                    Err(why) => failed.push(why),
                }
            }
            Some(Ok(Bored { borings, source: "GEF".into(), more: false, failed }))
        },
        move |result| AppMessage::Gis(super::gis::Message::Geo(Message::Bored(l, result))),
    )
}

#[cfg(target_arch = "wasm32")]
fn borings_task(_: Rd, _: f64, l: Language) -> Task<AppMessage> {
    Task::done(AppMessage::Gis(super::gis::Message::Geo(Message::Bored(l, None))))
}

#[cfg(target_arch = "wasm32")]
fn pick_borings_task(l: Language) -> Task<AppMessage> {
    Task::done(AppMessage::Gis(super::gis::Message::Geo(Message::Bored(l, None))))
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

    fn boring(name: &str, place: Option<[f64; 2]>, ground: Option<f64>, clay_from: f64) -> Boring {
        use bim_civil::ground::Stratum;
        Boring { name: name.into(), place, ground, date: None, layers: vec![Stratum { top: 0.0, bottom: clay_from, name: "zand".into() }, Stratum { top: clay_from, bottom: 5.0, name: "siltigeKlei".into() }] }
    }

    #[test]
    fn a_boring_is_a_column_of_blocks_in_the_colours_of_its_soils() {
        let center = Rd::new(155_000.0, 463_000.0);
        let model = borings_model(&[boring("B1", Some([155_010.0, 463_000.0]), Some(4.0), 2.0)], &Frame::at(center), 3.0, None, "test", "Borings", "Borings");
        let layer = &model.layers[0];
        let (low, high) = extent(layer);
        // 0.6 m wide at 10 m east, from 1 m over the zero level to 5 m under the ground
        assert!((low[0] - 9.7).abs() < 1e-4 && (high[0] - 10.3).abs() < 1e-4 && (high[2] - 1.0).abs() < 1e-4 && (low[2] + 4.0).abs() < 1e-4);
        // a block per layer: sand over clay, each closed (six faces)
        assert_eq!(layer.elements.iter().map(|e| e.2).collect::<Vec<_>>(), [36, 36]);
        let colours = layer.colors.as_ref().unwrap();
        assert!(colours[0] == soil_colour(Soil::Sand) && colours.last() == Some(&soil_colour(Soil::Clay)));
        assert!(model.props.values().any(|props| props.contains("\"Name\":\"siltigeKlei\"") && props.contains("\"Main soil\":\"Clay\"") && props.contains("\"Bottom (m NAP)\":-1.0")));
    }

    #[test]
    fn the_ground_between_borings_takes_the_layers_of_the_nearest() {
        let center = Rd::new(155_000.0, 463_000.0);
        // two borings 40 m apart: clay from 2 m in the west, from 3 m in the east; one without a place takes no part
        let borings = [boring("W", Some([154_980.0, 463_000.0]), Some(0.0), 2.0), boring("E", Some([155_020.0, 463_000.0]), Some(0.0), 3.0), boring("X", None, None, 1.0)];
        let (model, blocks) = ground_model(&borings, &Frame::at(center), 0.0, None, "Ground", "Ground").unwrap();
        assert_eq!(blocks, 4);
        let layer = &model.layers[0];
        let (low, high) = extent(layer);
        // 10 m around the borings (a quarter of 40 m), from the ground to 5 m under it
        assert!((low[0] + 30.0).abs() < 1e-3 && (high[0] - 30.0).abs() < 1e-3 && (low[1] + 10.0).abs() < 1e-3 && (high[1] - 10.0).abs() < 1e-3);
        assert!(high[2].abs() < 1e-5 && (low[2] + 5.0).abs() < 1e-5);
        // west of the middle the sand ends at 2 m, east of it at 3 m
        let colours = layer.colors.as_ref().unwrap();
        let sand_bottom = |west: bool| layer.positions.chunks_exact(3).zip(colours).filter(|(p, c)| **c == soil_colour(Soil::Sand) && (p[0] < -1.0) == west && p[0].abs() > 1.0).map(|(p, _)| p[2]).fold(f32::MAX, f32::min);
        assert!((sand_bottom(true) + 2.0).abs() < 1e-5 && (sand_bottom(false) + 3.0).abs() < 1e-5);
        // no boring with a place: no model
        assert!(ground_model(&borings[2..], &Frame::at(center), 0.0, None, "Ground", "Ground").is_none());
    }

    #[test]
    fn borings_read_are_kept_and_modelled_on_request() {
        let mut app = App::default();
        let read = Bored { borings: vec![boring("W", Some([154_980.0, 463_000.0]), Some(2.0), 2.0), boring("E", Some([155_020.0, 463_000.0]), Some(2.5), 3.0)], source: "GEF".into(), more: false, failed: Vec::new() };
        let _ = app.update_geo(Message::Bored(Language::English, Some(Ok(read))));
        assert_eq!(app.gis.geo.borings.len(), 2);
        assert_eq!(app.gis.geo.borings_note, Some(Note::Loaded { count: 2, unplaced: 0, more: false, failed: Vec::new() }));
        // the zero of the project lies at the first boring, its ground at the zero level
        assert_eq!((app.gis.zero, app.gis.zero_level), (Some(Rd::new(154_980.0, 463_000.0)), Some(2.0)));
        let _ = app.update_geo(Message::Model(Language::English));
        assert_eq!(app.gis.geo.modelled, Some(4));
        assert_eq!(app.models.len(), 2);
        assert!(app.models.iter().all(|entry| app.is_map_layer(&entry.model)));
        assert_eq!(app.side_tab, SideTab::Geo);
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
