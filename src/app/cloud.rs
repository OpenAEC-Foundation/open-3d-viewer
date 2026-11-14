//! A point cloud against the model: how far what was scanned lies from what was drawn. The
//! open models hold both (a scan as LAS, LAZ, E57 or PTS beside an IFC, say); the points are
//! coloured by how far they are off, the summary says how many are within the tolerance, and
//! the places where the most are off are listed, each to go to and to make an issue of.
//!
//! The points take their colours in the cloud's own model, which is drawn again; switching the
//! colours off gives the cloud its own colours back.

use super::collab::Event as CollabEvent;
use super::{App, Message};
use crate::collab::deviation::{colour, fit_shift, summarise, HotSpot, Summary, Surface};
use crate::model::Model;
use bim_bcf::issues::Topic;
use glam::Vec3;
use iced::widget::{button, column, container, row, Space};
use iced::{Alignment, Element, Length, Task};
use openaec_ui::design::a11y::{node as named, Node, Role};
use openaec_ui::design::widgets::{self as w, ButtonKind as K, Density as D, Tone};
use openaec_ui::design::{site, Look};
use openaec_ui::Language;
use std::sync::Arc;

/// No surface within this of a point: nothing is drawn near it (metres).
const REACH: f32 = 0.5;
/// The most hot spots listed.
const SPOTS: usize = 25;

#[derive(Debug, Clone)]
pub enum Event {
    Tolerance(String),
    Compare,
    /// Moves the scan onto the model, then compares.
    Align,
    /// The colours of the points per layer of the scene, and the summary.
    Done(u64, Arc<Vec<(usize, Vec<[u8; 4]>)>>, Summary),
    Off,
    Pick(usize),
    Issues,
}

/// What the panel keeps of it.
#[derive(Debug)]
pub struct State {
    /// The tolerance as typed (millimetres).
    pub tolerance: String,
    pub summary: Option<Summary>,
    pub running: bool,
    run: u64,
    /// The clouds' own colours, to give back: the open model's place, its layer, its colours.
    own: Vec<(usize, usize, Option<Vec<[u8; 4]>>)>,
}

impl Default for State {
    fn default() -> Self {
        State { tolerance: "20".into(), summary: None, running: false, run: 0, own: Vec::new() }
    }
}

fn go(event: Event) -> Message {
    Message::Collab(CollabEvent::Cloud(event))
}

/// The triangles of the model's surfaces (not of its helper layers: rooms are no surfaces) and
/// the points of its clouds per layer.
fn split(model: &Model, visible: &[bool]) -> (Vec<[Vec3; 3]>, Vec<(usize, Vec<Vec3>)>) {
    let (mut triangles, mut clouds) = (Vec::new(), Vec::new());
    for (l, layer) in model.layers.iter().enumerate() {
        let at = |i: usize| layer.positions.get(3 * i..3 * i + 3).map(|p| Vec3::new(p[0], p[1], p[2]));
        if layer.points {
            clouds.push((l, (0..layer.positions.len() / 3).filter_map(at).collect()));
        } else if visible.get(l).copied().unwrap_or(true) && !layer.name.starts_with("Helpers") {
            triangles.extend(layer.indices.chunks_exact(3).filter_map(|t| Some([at(t[0] as usize)?, at(t[1] as usize)?, at(t[2] as usize)?])));
        }
    }
    (triangles, clouds)
}

impl App {
    /// Whether the open models hold a cloud and surfaces to compare it with.
    fn cloud_ready(&self) -> bool {
        self.model.as_ref().is_some_and(|m| m.layers.iter().any(|l| l.points && !l.positions.is_empty()) && m.layers.iter().any(|l| !l.points && !l.indices.is_empty()))
    }

    /// Gives the points of scene layer `layer` these colours (`None`: their own), in the open
    /// model that holds them; true when a model changed.
    fn cloud_paint(&mut self, layer: usize, colours: Option<Vec<[u8; 4]>>) -> bool {
        let Some(k) = self.models.iter().position(|e| e.layers.contains(&layer)).or_else(|| (self.models.len() <= 1).then_some(0)) else { return false };
        // one model open: it is the scene itself
        if self.models.is_empty() {
            let Some(scene) = self.model.clone() else { return false };
            self.models.push(super::ModelEntry::new(scene, 0));
        }
        let entry = &mut self.models[k];
        let own = layer - entry.layers.start;
        let Some(points) = entry.model.layers.get(own).map(|l| l.positions.len() / 3) else { return false };
        if colours.as_ref().is_some_and(|c| c.len() != points) {
            return false;
        }
        let mut model = (*entry.model).clone();
        let kept = std::mem::replace(&mut model.layers[own].colors, colours.clone());
        model.id = crate::model::fresh_id();
        entry.model = Arc::new(model);
        match colours {
            // the first colouring keeps the cloud's own colours
            Some(_) if !self.collab.cloud.own.iter().any(|o| o.0 == k && o.1 == own) => self.collab.cloud.own.push((k, own, kept)),
            _ => {}
        }
        true
    }

    /// Draws the open models again after their clouds changed colour, keeping the camera.
    fn cloud_redraw(&mut self) {
        let camera = self.camera;
        let tab = self.collab.tab;
        if self.models.len() > 1 {
            self.rebuild_scene();
        } else if let Some(model) = self.models.first().map(|e| e.model.clone()) {
            self.set_scene(model);
        }
        self.camera = camera;
        self.anim = None;
        self.collab.tab = tab;
        self.collab.same_model();
    }

    pub(super) fn cloud_handle(&mut self, event: Event) -> Task<Message> {
        let l = self.collab.language;
        match event {
            Event::Tolerance(typed) => self.collab.cloud.tolerance = typed,
            Event::Compare => {
                let Some(model) = self.model.clone().filter(|_| self.cloud_ready()) else {
                    self.collab_note(Tone::Warning, l.text("Open een model en een puntenwolk samen (Toevoegen).", "Open a model and a point cloud together (Add)."));
                    return Task::none();
                };
                let tolerance = self.collab.cloud.tolerance.trim().replace(',', ".").parse::<f32>().ok().filter(|t| t.is_finite() && *t > 0.0).unwrap_or(20.0) / 1000.0;
                let visible = self.visible.clone();
                self.collab.cloud.run += 1;
                self.collab.cloud.running = true;
                let run = self.collab.cloud.run;
                return Task::perform(
                    async move {
                        let (tx, rx) = iced::futures::channel::oneshot::channel();
                        std::thread::spawn(move || {
                            let (triangles, clouds) = split(&model, &visible);
                            let surface = Surface::new(triangles);
                            let mut all_points = Vec::new();
                            let mut all_distances = Vec::new();
                            let mut colours = Vec::new();
                            for (layer, points) in clouds {
                                let distances = surface.distances(&points, REACH.max(tolerance * 3.0));
                                colours.push((layer, distances.iter().map(|d| colour(*d, tolerance)).collect()));
                                all_points.extend(points);
                                all_distances.extend(distances);
                            }
                            let _ = tx.send((colours, summarise(&all_points, &all_distances, tolerance, SPOTS)));
                        });
                        rx.await.ok()
                    },
                    move |done| match done {
                        Some((colours, summary)) => go(Event::Done(run, Arc::new(colours), summary)),
                        None => go(Event::Off),
                    },
                );
            }
            Event::Align => {
                let Some(model) = self.model.clone().filter(|_| self.cloud_ready()) else { return Task::none() };
                let (triangles, clouds) = split(&model, &self.visible);
                let surface = Surface::new(triangles);
                // a sample of the points is enough to see where the scan belongs
                let all: Vec<Vec3> = clouds.iter().flat_map(|(_, points)| points.iter().copied()).collect();
                let stride = (all.len() / 30_000).max(1);
                let sample: Vec<Vec3> = all.iter().step_by(stride).copied().collect();
                let (shift, near) = fit_shift(&surface, &sample, 1.0);
                if near * 2 < sample.len() {
                    self.collab_note(Tone::Warning, l.text("Minder dan de helft van de punten ligt binnen een meter van het model: leg de scan eerst ongeveer op zijn plek.", "Less than half of the points lie within a metre of the model: lay the scan about in its place first."));
                    return Task::none();
                }
                // the points move in the open models that hold them
                let mut moved = false;
                for (layer, _) in &clouds {
                    let Some(k) = self.models.iter().position(|e| e.layers.contains(layer)) else { continue };
                    let entry = &mut self.models[k];
                    let own = layer - entry.layers.start;
                    let mut cloud = (*entry.model).clone();
                    if let Some(points) = cloud.layers.get_mut(own).filter(|l| l.points) {
                        for p in points.positions.chunks_exact_mut(3) {
                            p[0] += shift.x;
                            p[1] += shift.y;
                            p[2] += shift.z;
                        }
                        cloud.id = crate::model::fresh_id();
                        entry.model = Arc::new(cloud);
                        moved = true;
                    }
                }
                if !moved {
                    self.collab_note(Tone::Warning, l.text("De scan en het model zijn één bestand: uitlijnen kan alleen met de scan als eigen model.", "The scan and the model are one file: aligning needs the scan as a model of its own."));
                    return Task::none();
                }
                self.cloud_redraw();
                self.collab_note(Tone::Success, format!("{} {:.0} mm (x {:.0}, y {:.0}, z {:.0})", l.text("Scan verschoven over", "Scan moved by"), shift.length() * 1000.0, shift.x * 1000.0, shift.y * 1000.0, shift.z * 1000.0));
                return self.cloud_handle(Event::Compare);
            }
            Event::Done(run, colours, summary) => {
                if run != self.collab.cloud.run {
                    return Task::none();
                }
                self.collab.cloud.running = false;
                let mut changed = false;
                for (layer, colours) in colours.iter() {
                    changed |= self.cloud_paint(*layer, Some(colours.clone()));
                }
                self.collab.cloud.summary = Some(summary);
                if changed {
                    self.cloud_redraw();
                }
            }
            Event::Off => {
                self.collab.cloud.running = false;
                self.collab.cloud.summary = None;
                let own = std::mem::take(&mut self.collab.cloud.own);
                let mut changed = false;
                for (k, layer, colours) in own {
                    if let Some(entry) = self.models.get_mut(k).filter(|e| e.model.layers.get(layer).is_some_and(|l| l.points)) {
                        let mut model = (*entry.model).clone();
                        model.layers[layer].colors = colours;
                        model.id = crate::model::fresh_id();
                        entry.model = Arc::new(model);
                        changed = true;
                    }
                }
                if changed {
                    self.cloud_redraw();
                }
            }
            Event::Pick(k) => {
                if let Some(spot) = self.collab.cloud.summary.as_ref().and_then(|s| s.hot_spots.get(k)).copied() {
                    let mut bounds = crate::model::Bounds::empty();
                    bounds.extend((spot.at - Vec3::splat(1.5)).to_array());
                    bounds.extend((spot.at + Vec3::splat(1.5)).to_array());
                    let mut to = self.camera;
                    to.fit(&bounds, self.aspect());
                    self.anim = Some((self.camera, to, std::time::Instant::now()));
                }
            }
            Event::Issues => {
                let spots: Vec<HotSpot> = self.collab.cloud.summary.as_ref().map(|s| s.hot_spots.clone()).unwrap_or_default();
                let made = spots.len();
                for spot in spots {
                    let mut topic = Topic::new(&format!("{} {:.0} mm", l.text("Puntenwolk wijkt af:", "Point cloud is off:"), spot.worst * 1000.0), &self.collab.author);
                    topic.kind = "Remark".into();
                    topic.priority = "Normal".into();
                    topic.description = format!("{} {} {:.0} mm", spot.points, l.text("punten liggen hier verder van het model dan de tolerantie; het verst", "points lie further from the model here than the tolerance; the furthest"), spot.worst * 1000.0);
                    topic.viewpoints.push(self.collab_viewpoint_at(spot.at, 1.5, &[]));
                    self.collab.issues.topics.push(topic);
                }
                self.collab.issues_changed |= made > 0;
                self.collab_note(Tone::Success, format!("{made} {}", l.text("issues gemaakt", "issues made")));
            }
        }
        Task::none()
    }

    /// The part of the Compare tab that sets a cloud against the model.
    pub(super) fn cloud_view<'a>(&'a self, look: Look, l: Language) -> Element<'a, Message> {
        let state = &self.collab.cloud;
        let p = self.theme.palette();
        let mut out = column![
            w::heading(look, l.text("Puntenwolk tegen model", "Point cloud against model"), 13.0),
            w::secondary(look, l.text("Open de scan en het model samen: elk punt krijgt de kleur van zijn afstand tot het model.", "Open the scan and the model together: each point takes the colour of its distance to the model.")),
            row![
                w::secondary(look, l.text("Tolerantie", "Tolerance")),
                container(w::text_field(look, "mm", &state.tolerance, Some(|t| go(Event::Tolerance(t))), D::Compact, None)).width(64),
                w::secondary(look, "mm"),
                Space::new().width(Length::Fill),
                w::button(look, l.text("Vergelijken", "Compare"), K::Primary, D::Compact, None, (self.cloud_ready() && !state.running).then(|| go(Event::Compare))),
                w::button(look, l.text("Uitlijnen", "Align"), K::Secondary, D::Compact, None, (self.cloud_ready() && !state.running && self.models.len() > 1).then(|| go(Event::Align))),
                w::button(look, l.text("Uit", "Off"), K::Quiet, D::Compact, None, state.summary.is_some().then(|| go(Event::Off))),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        ]
        .spacing(4);
        if state.running {
            out = out.push(w::secondary(look, l.text("Bezig…", "Working…")));
        }
        let Some(s) = &state.summary else { return out.into() };
        let near = (s.within + s.off).max(1);
        out = out.push(
            row![
                site::chip(look, &format!("{:.0}% {}", 100.0 * s.within as f32 / near as f32, l.text("binnen tolerantie", "within tolerance")), Some(if s.off * 20 <= near { Tone::Success } else { Tone::Warning })),
                site::chip(look, &format!("{} {}", s.off, l.text("punten erbuiten", "points off")), Some(if s.off == 0 { Tone::Success } else { Tone::Error })),
                site::chip(look, &format!("{} {}", s.far, l.text("zonder model", "without model")), None),
            ]
            .spacing(4)
            .wrap(),
        );
        out = out.push(w::secondary(look, format!("{} {} · {} {:.0} mm · {} {:.0} mm", s.points, l.text("punten", "points"), l.text("gemiddeld", "mean"), s.mean * 1000.0, l.text("grootste", "largest"), s.worst * 1000.0)));
        // how the distances spread: a bar for each fifth of the tolerance, up to twice it
        let most = s.histogram.iter().copied().max().unwrap_or(1).max(1) as f32;
        let tolerance = state.tolerance.trim().replace(',', ".").parse::<f32>().unwrap_or(20.0) / 1000.0;
        let mut bars = row![].spacing(2).align_y(Alignment::End).height(40);
        for (k, n) in s.histogram.iter().enumerate() {
            let [r, g, b, _] = colour(Some((k as f32 + 0.5) / 5.0 * tolerance), tolerance);
            let fill = iced::Color::from_rgb8(r, g, b);
            let h = (*n as f32 / most * 36.0).max(1.0);
            bars = bars.push(container(Space::new().width(14).height(h)).style(move |_| container::Style { background: Some(iced::Background::Color(fill)), ..Default::default() }));
        }
        out = out.push(row![bars, w::secondary(look, format!("0 … {:.0} mm … ≥ {:.0} mm", tolerance * 1000.0, tolerance * 2000.0))].spacing(8).align_y(Alignment::End));
        if !s.hot_spots.is_empty() {
            out = out.push(
                row![
                    w::heading(look, format!("{} · {}", l.text("Plekken met afwijking", "Places that are off"), s.hot_spots.len()), 13.0).width(Length::Fill),
                    w::button(look, l.text("Issues maken", "Make issues"), K::Secondary, D::Compact, None, Some(go(Event::Issues))),
                ]
                .align_y(Alignment::Center),
            );
        }
        for (k, spot) in s.hot_spots.iter().enumerate() {
            let line = row![
                w::body(look, format!("{} {}", spot.points, l.text("punten", "points"))).width(Length::Fill),
                site::chip(look, &format!("{:.0} mm", spot.worst * 1000.0), Some(Tone::Error)),
            ]
            .spacing(6)
            .align_y(Alignment::Center);
            out = out.push(named(
                button(line).width(Length::Fill).padding([3, 6]).style(move |_, st| p.row_button(st)).on_press(go(Event::Pick(k))),
                Node::new(Role::Row, format!("{} {}: {:.0} mm", l.text("Plek", "Place"), k + 1, spot.worst * 1000.0)),
            ));
        }
        out.into()
    }

    /// The remote's command: compares at once and answers with the summary.
    pub(crate) fn cloud_command(&mut self, a: &serde_json::Value) -> Result<serde_json::Value, String> {
        use serde_json::json;
        if a.get("action").and_then(|v| v.as_str()) == Some("align") {
            let _ = self.cloud_handle(Event::Align);
            return Ok(json!({ "notice": self.collab.notice.as_ref().map(|(_, m)| m.clone()), "note": "the comparison runs after it; ask again with action compare" }));
        }
        if a.get("action").and_then(|v| v.as_str()) == Some("off") {
            let _ = self.cloud_handle(Event::Off);
            return Ok(json!({ "off": true }));
        }
        let model = self.model.clone().filter(|_| self.cloud_ready()).ok_or("open a model and a point cloud together")?;
        if let Some(mm) = a.get("tolerance_mm").and_then(|v| v.as_f64()) {
            self.collab.cloud.tolerance = mm.to_string();
        }
        let tolerance = self.collab.cloud.tolerance.trim().replace(',', ".").parse::<f32>().ok().filter(|t| *t > 0.0).unwrap_or(20.0) / 1000.0;
        let started = std::time::Instant::now();
        let (triangles, clouds) = split(&model, &self.visible);
        let surface = Surface::new(triangles);
        let (mut all_points, mut all_distances, mut colours) = (Vec::new(), Vec::new(), Vec::new());
        for (layer, points) in clouds {
            let distances = surface.distances(&points, REACH.max(tolerance * 3.0));
            colours.push((layer, distances.iter().map(|d| colour(*d, tolerance)).collect()));
            all_points.extend(points);
            all_distances.extend(distances);
        }
        let summary = summarise(&all_points, &all_distances, tolerance, SPOTS);
        self.collab.cloud.run += 1;
        let run = self.collab.cloud.run;
        let _ = self.cloud_handle(Event::Done(run, Arc::new(colours), summary.clone()));
        self.collab.tab = super::collab::Tab::Compare;
        Ok(json!({
            "points": summary.points, "within": summary.within, "off": summary.off, "without_model": summary.far,
            "mean_mm": (summary.mean * 1000.0).round(), "worst_mm": (summary.worst * 1000.0).round(),
            "hot_spots": summary.hot_spots.iter().map(|h| json!({ "at": h.at.to_array(), "points": h.points, "worst_mm": (h.worst * 1000.0).round() })).collect::<Vec<_>>(),
            "seconds": started.elapsed().as_secs_f32(),
        }))
    }
}
