//! The zero point of each model: a mark with its three axes over the 3D view, always in sight, and
//! in the right pane, for the model picked in the explorer, where the model is moved to and where
//! its zero point lies in it.
//!
//! Moving a model moves all of it, its zero point along: the models are combined again with the
//! move added to the model's place ([`Model::combine_moved`]), so world coordinates stay those of
//! the file. Moving the zero point leaves the model where it is: only the mark moves, and the
//! place is kept with the model while it is open. Neither is written to a file yet.

use super::{App, Message as AppMessage, ModelEntry};
use crate::model::Model;
use crate::theme::Palette;
use glam::{DVec3, Mat4, Vec3};
use iced::widget::canvas::{self, Frame, Path, Stroke};
use iced::widget::text::Wrapping;
use iced::widget::{column, row, text, text_input};
use iced::{alignment, mouse, Center, Color, Element, Length, Point, Rectangle, Task, Vector};
use openaec_ui::Language;
use std::sync::Arc;

/// What a typed place moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Moves {
    /// The whole model, its zero point along.
    Model,
    /// The zero point alone, within the model.
    Zero,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// A coordinate typed for the picked model: what it moves, the axis (0 x, 1 y, 2 z), the text.
    Typed(Moves, usize, String),
    /// Use what is typed (Enter).
    Done(Moves),
    /// The map of a model (by its id) came, as RGBA rows, or why not.
    Map(u64, Result<Arc<Vec<u8>>, String>),
}

/// The map of where a model lies.
#[derive(Debug, Clone, Default)]
pub enum Map {
    /// Not asked for.
    #[default]
    None,
    Loading,
    Ready(iced::widget::image::Handle),
    Failed(String),
}

/// Where a model was moved to and where its zero point lies in it (metres), with what is typed
/// for each until it is used.
#[derive(Debug, Clone, Default)]
pub struct Placed {
    pub moved: DVec3,
    pub zero: DVec3,
    typed: [[Option<String>; 3]; 2],
    pub map: Map,
}

/// A number as typed: a comma counts as a decimal point.
fn number(typed: &str) -> Option<f64> {
    typed.trim().replace(',', ".").parse::<f64>().ok().filter(|v| v.is_finite() && v.abs() < 1.0e7)
}

/// A coordinate as shown: millimetres kept, no trailing zeros.
fn shown(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

impl App {
    pub(super) fn update_origin(&mut self, message: Message) -> Task<AppMessage> {
        if let Message::Map(id, result) = message {
            if let Some(entry) = self.models.iter_mut().find(|entry| entry.model.id == id) {
                entry.placed.map = match result {
                    Ok(pixels) => Map::Ready(iced::widget::image::Handle::from_rgba(super::origin_map::SIZE.0, super::origin_map::SIZE.1, pixels.as_ref().clone())),
                    Err(why) => Map::Failed(why),
                };
            }
            return Task::none();
        }
        self.typed_origin(message);
        Task::none()
    }

    /// Where the file of a model says it lies: latitude and longitude (degrees), of its map
    /// conversion when it has one, else of its site.
    fn located(entry: &ModelEntry) -> Option<(f64, f64)> {
        let info = &entry.model.project.info;
        let value = ["Georeference latitude, longitude", "Site latitude, longitude"].iter().find_map(|key| info.iter().find(|(k, _)| k == key))?;
        let (lat, lon) = value.1.split_once(',')?;
        Some((lat.trim().parse().ok()?, lon.trim().parse().ok()?))
    }

    /// Asks for the map of the picked model, once, when its file says where it lies.
    pub(super) fn load_map(&mut self) -> Task<AppMessage> {
        let Some(entry) = self.picked_model.and_then(|i| self.models.get_mut(i)) else { return Task::none() };
        let (Some((lat, lon)), Map::None) = (Self::located(entry), &entry.placed.map) else { return Task::none() };
        entry.placed.map = Map::Loading;
        map_task(entry.model.id, lat, lon)
    }

    fn typed_origin(&mut self, message: Message) {
        let Some(entry) = self.picked_model.and_then(|i| self.models.get_mut(i)) else { return };
        match message {
            Message::Map(..) => {}
            Message::Typed(moves, axis, typed) => {
                if let Some(slot) = entry.placed.typed[moves as usize].get_mut(axis) {
                    *slot = Some(typed);
                }
            }
            Message::Done(moves) => {
                let typed = std::mem::take(&mut entry.placed.typed[moves as usize]);
                let current = if moves == Moves::Model { entry.placed.moved } else { entry.placed.zero };
                let next = DVec3::from_array(std::array::from_fn(|k| typed[k].as_deref().and_then(number).unwrap_or(current[k])));
                if next == current {
                    return;
                }
                if moves == Moves::Zero {
                    entry.placed.zero = next;
                    return;
                }
                entry.placed.moved = next;
                self.replace_models();
            }
        }
    }

    /// Combines the models again, each at the place it was moved to; one model that was not
    /// moved shows as it was loaded.
    fn replace_models(&mut self) {
        let picked = self.picked_model;
        let sources: Vec<Arc<Model>> = self.models.iter().map(|entry| entry.model.clone()).collect();
        let moves: Vec<DVec3> = self.models.iter().map(|entry| entry.placed.moved).collect();
        let scene = match sources.as_slice() {
            [one] if moves[0] == DVec3::ZERO => one.clone(),
            _ => Arc::new(Model::combine_moved(&sources, &moves)),
        };
        self.set_scene(scene);
        self.restore_model_visibility();
        self.picked_model = picked;
        self.side_tab = super::SideTab::Properties;
    }

    /// Where the zero point of each shown model lies in the 3D view, with the model's name and
    /// whether it is the picked one.
    pub(super) fn zero_points(&self) -> Vec<(Vec3, String, bool)> {
        let Some(scene) = &self.model else { return Vec::new() };
        let mut first = 0usize;
        let mut out = Vec::new();
        for (i, entry) in self.models.iter().enumerate() {
            // a model's own zero is drawn where the scene shifted it to
            let shift = scene.origins.get(first).map_or(DVec3::ZERO, |o| o.shift);
            first += entry.model.origins.len().max(1);
            if entry.visible {
                out.push(((shift + entry.placed.zero).as_vec3(), entry.model.file_name(), self.picked_model == Some(i)));
            }
        }
        out
    }

    /// The rows of the right pane for the picked model: where it is moved to and where its zero
    /// point lies in it, each typed in and used on Enter.
    pub(super) fn origin_section(&self, p: Palette, l: Language, entry: &ModelEntry) -> Element<'_, AppMessage> {
        let fields = |moves: Moves, at: DVec3| {
            let mut fields = row![].spacing(4).width(Length::FillPortion(3));
            for (axis, name) in ["X", "Y", "Z"].into_iter().enumerate() {
                let value = entry.placed.typed[moves as usize][axis].clone().unwrap_or_else(|| shown(at[axis]));
                fields = fields.push(
                    text_input(name, &value)
                        .on_input(move |typed| AppMessage::Origin(Message::Typed(moves, axis, typed)))
                        .on_submit(AppMessage::Origin(Message::Done(moves)))
                        .size(12)
                        .padding([2, 4]),
                );
            }
            fields
        };
        let line = |label: &str, moves: Moves, at: DVec3| {
            row![text(label.to_string()).size(12).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph).width(Length::FillPortion(2)), fields(moves, at)]
                .spacing(8)
                .align_y(Center)
        };
        let mut rows = column![
            line(l.text("Model verplaatst (m)", "Model moved (m)"), Moves::Model, entry.placed.moved),
            line(l.text("Nulpunt in model (m)", "Zero point in model (m)"), Moves::Zero, entry.placed.zero),
            text(l.text("X, Y, Z in meters; Enter past toe.", "X, Y, Z in metres; Enter applies.").to_string()).size(11).color(p.text_muted),
        ]
        .spacing(4);
        // the map of where its file says it lies
        let note = |words: String| text(words).size(11).color(p.text_muted).wrapping(Wrapping::WordOrGlyph);
        match &entry.placed.map {
            Map::None => {}
            Map::Loading => rows = rows.push(note(l.text("Kaart wordt opgehaald…", "Fetching the map…").to_string())),
            Map::Ready(picture) => {
                rows = rows
                    .push(iced::widget::image(picture.clone()).width(Length::Fill))
                    .push(note("© OpenStreetMap contributors".to_string()));
            }
            Map::Failed(why) => rows = rows.push(note(format!("{}: {why}", l.text("Geen kaart", "No map")))),
        }
        rows.into()
    }
}

/// Fetches the map of a place on a worker thread.
#[cfg(not(target_arch = "wasm32"))]
fn map_task(id: u64, lat: f64, lon: f64) -> Task<AppMessage> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(super::origin_map::fetch(lat, lon).map(Arc::new));
            });
            rx.await.unwrap_or_else(|_| Err("the map was not fetched".into()))
        },
        move |result| AppMessage::Origin(Message::Map(id, result)),
    )
}

/// The browser build shows no map.
#[cfg(target_arch = "wasm32")]
fn map_task(_: u64, _: f64, _: f64) -> Task<AppMessage> {
    Task::none()
}

/// The zero points over the 3D view: a ring with the three axes (x red, y green, z blue), each a
/// fixed length on screen, and the model's name. Drawn only: the viewport below handles the pointer.
pub struct ZeroPoints {
    pub marks: Vec<(Vec3, String, bool)>,
    /// World to clip space of the current camera.
    pub view_proj: Mat4,
    pub ink: Color,
    pub fill: Color,
    pub accent: Color,
}

/// The length of an axis on screen (logical pixels).
const AXIS: f32 = 44.0;

impl<M> canvas::Program<M> for ZeroPoints {
    type State = ();

    fn draw(&self, _: &(), renderer: &iced::Renderer, _: &iced::Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let project = |world: Vec3| {
            let clip = self.view_proj * world.extend(1.0);
            (clip.w > 1e-6).then(|| {
                let ndc = clip.truncate() / clip.w;
                Point::new((ndc.x + 1.0) * 0.5 * bounds.width, (1.0 - ndc.y) * 0.5 * bounds.height)
            })
        };
        for (at, name, picked) in &self.marks {
            let Some(centre) = project(*at) else { continue };
            let axes = [(Vec3::X, Color::from_rgb(0.86, 0.20, 0.18), "X"), (Vec3::Y, Color::from_rgb(0.18, 0.62, 0.22), "Y"), (Vec3::Z, Color::from_rgb(0.16, 0.38, 0.86), "Z")];
            for (axis, color, letter) in axes {
                // the direction the axis runs on screen, drawn a fixed length
                let Some(tip) = project(*at + axis) else { continue };
                let along = Vector::new(tip.x - centre.x, tip.y - centre.y);
                let length = (along.x * along.x + along.y * along.y).sqrt();
                if length < 1e-3 {
                    continue;
                }
                let end = Point::new(centre.x + along.x / length * AXIS, centre.y + along.y / length * AXIS);
                frame.stroke(&Path::line(centre, end), Stroke::default().with_color(color).with_width(2.5));
                frame.fill_text(canvas::Text {
                    content: letter.into(),
                    position: Point::new(end.x + along.x / length * 8.0, end.y + along.y / length * 8.0),
                    size: iced::Pixels(11.0),
                    color,
                    align_x: alignment::Horizontal::Center.into(),
                    align_y: alignment::Vertical::Center,
                    font: super::FONT_BOLD,
                    ..canvas::Text::default()
                });
            }
            let ring = Path::circle(centre, if *picked { 6.5 } else { 5.0 });
            frame.fill(&ring, self.fill);
            frame.stroke(&ring, Stroke::default().with_color(if *picked { self.accent } else { self.ink }).with_width(if *picked { 2.5 } else { 1.5 }));
            frame.fill(&Path::circle(centre, 1.5), self.ink);
            frame.fill_text(canvas::Text {
                content: name.clone(),
                position: Point::new(centre.x + 10.0, centre.y + 10.0),
                size: iced::Pixels(11.0),
                color: self.ink,
                font: super::FONT,
                ..canvas::Text::default()
            });
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_number_reads_with_a_comma_or_a_point() {
        assert_eq!(number(" 1,5 "), Some(1.5));
        assert_eq!(number("-2.25"), Some(-2.25));
        assert_eq!(number("x"), None);
        assert_eq!(shown(1.5), "1.5");
        assert_eq!(shown(-0.0), "0");
        assert_eq!(shown(12.0), "12");
    }
}
