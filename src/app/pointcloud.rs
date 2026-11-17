//! The Pointcloud product: what it does with an open point cloud beyond showing it.
//!
//! Every job works on the file of the cloud, not on the thinned points the view shows: the
//! library of Open Pointcloud Studio (`pointcloud-core`) reads the whole source. What a job makes
//! is in the coordinates of that file; a mesh is shown over the cloud in a copy moved to where
//! the view draws the cloud.
//!
//! - **Terrain mesh**: a surface over the points seen from above (ground, a site, a roof plane);
//!   no walls or overhangs.
//! - **Surface mesh**: a surface through the points in all directions (walls, ceilings, objects).
//! - **Closed mesh**: a surface fitted to the points voxel by voxel, with small gaps closed and
//!   flat parts simplified; the slowest and the best for buildings.
//! - **Faces**: the flat faces of walls, floors, ceilings and roofs and the round columns and
//!   pipes the points show, each as a face of its own in the colour of its kind.
//! - **Export of the points**: all of them, a share of them, or those in the section box, as LAS,
//!   LAZ, E57, PLY, XYZ, PTS or CSV.
//! - **Export of the mesh** made last: OBJ, PLY, STL, DXF, DWG or IFC.
//! - **Section drawing**: the points in a slab at a face of the section box, as DXF or DWG.
//! - **Details**: what the file holds (points, extent, attributes, scanner stations).
//!
//! The section box, when on, limits the meshes, the export of the points and the drawing.

use super::{App, Message, Status};
use crate::loaders;
use crate::section::SectionBox;
use iced::widget::{column, row};
use iced::{Element, Task};
use openaec_ui::design::widgets::{self as w, ButtonKind as K, Density as D};
use openaec_ui::design::Look;
use openaec_ui::Language;
use pointcloud_core::ops;
use pointcloud_core::{Bounds, LoadError, OrientedBox};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// The sizes a point is drawn at (pixels), from small to large.
pub const POINT_SIZES: [f32; 6] = [1.0, 1.5, 2.5, 4.0, 6.0, 9.0];

/// A way to mesh a cloud.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mesh {
    Terrain,
    Surface,
    Closed,
    /// Not a mesh of the surface but the flat faces and cylinders found in the points.
    Faces,
}

impl Mesh {
    /// What its file is named after, behind the cloud's own name.
    fn word(self) -> &'static str {
        match self {
            Mesh::Terrain => "terrain",
            Mesh::Surface => "surface",
            Mesh::Closed => "closed",
            Mesh::Faces => "faces",
        }
    }
}

/// The face of the section box a drawing is made at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Plan,
    Front,
    Back,
    Left,
    Right,
}

impl View {
    pub const ALL: [View; 5] = [View::Plan, View::Front, View::Back, View::Left, View::Right];

    pub fn label(self, l: Language) -> &'static str {
        match self {
            View::Plan => l.text("Plattegrond", "Plan"),
            View::Front => l.text("Voor", "Front"),
            View::Back => l.text("Achter", "Back"),
            View::Left => l.text("Links", "Left"),
            View::Right => l.text("Rechts", "Right"),
        }
    }

    fn of_library(self) -> pointcloud_core::DrawingView {
        use pointcloud_core::DrawingView as V;
        match self {
            View::Plan => V::Plan,
            View::Front => V::Front,
            View::Back => V::Back,
            View::Left => V::Left,
            View::Right => V::Right,
        }
    }
}

/// A number of vertices in a list of choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Vertices(pub usize);

/// The edge of a voxel in millimetres in a list of choices; none lets the mesher choose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Voxel(pub Option<u16>);

/// A width in centimetres in a list of choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Width(pub u16);

/// A share in percent in a list of choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Share(pub u8);

const VERTICES: [Vertices; 6] = [Vertices(25_000), Vertices(50_000), Vertices(100_000), Vertices(250_000), Vertices(500_000), Vertices(1_000_000)];
const VOXELS: [Voxel; 6] = [Voxel(None), Voxel(Some(10)), Voxel(Some(20)), Voxel(Some(30)), Voxel(Some(50)), Voxel(Some(100))];
const HOLES: [Width; 6] = [Width(0), Width(10), Width(25), Width(50), Width(100), Width(200)];
const NEIGHBORS: [Vertices; 6] = [Vertices(6), Vertices(8), Vertices(12), Vertices(16), Vertices(24), Vertices(32)];
const EDGES: [Vertices; 5] = [Vertices(2), Vertices(3), Vertices(4), Vertices(6), Vertices(8)];
const SHARES: [Share; 6] = [Share(100), Share(50), Share(25), Share(10), Share(5), Share(1)];

impl std::fmt::Display for Vertices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&grouped(self.0 as u64))
    }
}

impl std::fmt::Display for Voxel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            None => f.write_str("Auto"),
            Some(mm) => write!(f, "{} cm", mm / 10),
        }
    }
}

impl std::fmt::Display for Width {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            cm if cm >= 100 => write!(f, "{} m", cm / 100),
            cm => write!(f, "{cm} cm"),
        }
    }
}

impl std::fmt::Display for Share {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} %", self.0)
    }
}

/// A count with its thousands apart: 1 250 000.
pub fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push('\u{a0}');
        }
        out.push(c);
    }
    out
}

/// How the jobs of the product are done.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    /// At most this many vertices in a terrain or surface mesh.
    pub vertices: usize,
    /// The voxel of a closed mesh and the least cell of a surface mesh (millimetres).
    pub voxel: Option<u16>,
    /// A closed mesh closes gaps up to this wide (centimetres).
    pub hole: u16,
    /// A surface mesh joins a vertex to this many neighbours.
    pub neighbors: usize,
    /// A surface mesh leaves out edges longer than this many times the usual one.
    pub edge: usize,
    /// The share of the points a closed mesh is fitted to (percent).
    pub sample: u8,
    /// The share of the points an export keeps (percent).
    pub share: u8,
    /// The face of the section box a drawing is made at.
    pub view: View,
}

impl Default for Settings {
    fn default() -> Self {
        Self { vertices: 100_000, voxel: None, hole: 25, neighbors: 12, edge: 4, sample: 100, share: 100, view: View::Plan }
    }
}

/// A change of one setting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Setting {
    Vertices(usize),
    Voxel(Option<u16>),
    Hole(u16),
    Neighbors(usize),
    Edge(usize),
    Sample(u8),
    Share(u8),
    View(View),
}

/// What a file of a cloud holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Details {
    pub format: String,
    pub points: u64,
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub rgb: bool,
    pub intensity: bool,
    pub classification: bool,
    pub stations: usize,
    pub photos: usize,
}

/// A mesh made of a cloud: the file shown over the cloud, the same mesh in the coordinates of
/// the cloud's file, its vertices and triangles, and how long it took.
#[derive(Debug, Clone)]
pub struct Meshed {
    pub file: PathBuf,
    pub source: PathBuf,
    pub vertices: usize,
    pub triangles: usize,
    pub seconds: f32,
    /// How the triangles hang together: edges of one triangle only, and loose parts.
    pub open_edges: u64,
    pub parts: u32,
    /// How far the points lie from a closed mesh (metres): the mean, and what 95 % stay within.
    pub deviation: Option<(f64, f64)>,
    /// Of detected faces: the walls, floors, ceilings, sloped faces and cylinders found.
    pub faces: Option<[usize; 5]>,
}

impl Meshed {
    /// The figures of the mesh in a line.
    pub fn figures(&self) -> String {
        if let Some([walls, floors, ceilings, sloped, cylinders]) = self.faces {
            return format!("{walls} walls, {floors} floors, {ceilings} ceilings, {sloped} sloped faces, {cylinders} cylinders");
        }
        let mut text = format!("{} vertices, {} triangles, {} open edges, {} parts", grouped(self.vertices as u64), grouped(self.triangles as u64), grouped(self.open_edges), self.parts);
        if let Some((mean, most)) = self.deviation {
            text.push_str(&format!(", deviation {:.1} mm (95 %: {:.1} mm)", mean * 1000.0, most * 1000.0));
        }
        text
    }
}

/// What can be written to a file of the user's choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Export {
    Points,
    Mesh,
    Drawing,
}

/// What a job gave.
#[derive(Debug, Clone)]
pub enum Done {
    Meshed(Meshed),
    /// A file was written: what to say about it.
    Wrote(String),
}

/// What the product keeps beside the project: its settings, the mesh made last, the details read
/// last (of which file) and the switch that stops a running job.
#[derive(Debug, Clone, Default)]
pub struct State {
    pub settings: Settings,
    pub last: Option<Meshed>,
    pub details: Option<(PathBuf, Result<Details, String>)>,
    stop: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
pub enum Event {
    /// Mesh the cloud of the project.
    Mesh(Mesh),
    /// Draw the points a step larger (true) or smaller.
    PointSize(bool),
    Set(Setting),
    /// Read what the file of the cloud holds, unless it is known.
    Inspect,
    Inspected(PathBuf, Result<Details, String>),
    /// Ask where to write, then write.
    Export(Export),
    ExportTo(Export, Option<PathBuf>),
    Done(Result<Done, String>),
    /// Stop the running job.
    Cancel,
}

/// The cloud a job works on: its file, and where the view draws the origin of that file
/// (the loader moves a cloud with survey coordinates to a local origin).
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub file: PathBuf,
    pub origin: [f64; 3],
}

impl App {
    /// The cloud the Pointcloud product works on: the model picked in the explorer when that is
    /// a cloud, else the first cloud of the project.
    pub fn cloud(&self) -> Option<Source> {
        let is_cloud = |m: &crate::model::Model| m.layers.iter().any(|l| l.points) && m.path.is_file();
        let picked = self.picked_model.and_then(|i| self.models.get(i)).map(|e| &e.model).filter(|m| is_cloud(m));
        let model = picked.or_else(|| self.models.iter().map(|e| &e.model).find(|m| is_cloud(m)))?;
        Some(Source { file: model.path.clone(), origin: origin_of(&model.project.info) })
    }

    /// The file of that cloud.
    pub fn cloud_file(&self) -> Option<PathBuf> {
        self.cloud().map(|c| c.file)
    }

    /// The section box, when on, in the coordinates of the cloud's file.
    fn cloud_box(&self, cloud: &Source) -> Option<OrientedBox> {
        self.active_section().map(|section| source_box(&section, cloud.origin))
    }

    /// Runs `job` on a thread of its own, with the switch that stops it.
    fn cloud_job(&mut self, saying: String, job: impl FnOnce(&AtomicBool) -> Result<Done, String> + Send + 'static) -> Task<Message> {
        self.meshing = true;
        self.status = Status::Loaded(saying);
        let stop = Arc::new(AtomicBool::new(false));
        self.cloud.stop = stop.clone();
        Task::perform(
            async move {
                let (tx, rx) = iced::futures::channel::oneshot::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(job(&stop));
                });
                rx.await.unwrap_or_else(|_| Err("the worker thread stopped".to_string()))
            },
            |done| Message::Pointcloud(Event::Done(done)),
        )
    }

    pub(super) fn pointcloud_update(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::PointSize(larger) => {
                let at = POINT_SIZES.iter().position(|&s| s >= self.point_size).unwrap_or(POINT_SIZES.len() - 1);
                let next = if larger { (at + 1).min(POINT_SIZES.len() - 1) } else { at.saturating_sub(1) };
                self.point_size = POINT_SIZES[next];
            }
            Event::Set(setting) => {
                let s = &mut self.cloud.settings;
                match setting {
                    Setting::Vertices(n) => s.vertices = n,
                    Setting::Voxel(v) => s.voxel = v,
                    Setting::Hole(h) => s.hole = h,
                    Setting::Neighbors(n) => s.neighbors = n,
                    Setting::Edge(e) => s.edge = e,
                    Setting::Sample(p) => s.sample = p,
                    Setting::Share(p) => s.share = p,
                    Setting::View(v) => s.view = v,
                }
            }
            Event::Inspect => {
                let Some(cloud) = self.cloud() else { return Task::none() };
                if self.cloud.details.as_ref().is_some_and(|(file, _)| *file == cloud.file) {
                    return Task::none();
                }
                let file = cloud.file.clone();
                return Task::perform(
                    async move {
                        let (tx, rx) = iced::futures::channel::oneshot::channel();
                        let read = file.clone();
                        std::thread::spawn(move || {
                            let _ = tx.send(details(&read));
                        });
                        (file, rx.await.unwrap_or_else(|_| Err("the worker thread stopped".to_string())))
                    },
                    |(file, details)| Message::Pointcloud(Event::Inspected(file, details)),
                );
            }
            Event::Inspected(file, details) => self.cloud.details = Some((file, details)),
            Event::Mesh(kind) => {
                let Some(cloud) = self.cloud().filter(|_| !self.meshing) else { return Task::none() };
                let (settings, section) = (self.cloud.settings, self.cloud_box(&cloud));
                let name = file_name(&cloud.file);
                return self.cloud_job(format!("Meshing {name}…"), move |stop| mesh(&cloud, kind, &settings, section, stop).map(Done::Meshed));
            }
            Event::Export(what) => {
                let Some(cloud) = self.cloud().filter(|_| !self.meshing) else { return Task::none() };
                let stem = cloud.file.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "cloud".to_string());
                let (title, name, filters): (&str, String, &[(&str, &[&str])]) = match what {
                    Export::Points => ("Export points", format!("{stem}.laz"), &[("LAZ", &["laz"]), ("LAS", &["las"]), ("E57", &["e57"]), ("PLY", &["ply"]), ("XYZ", &["xyz"]), ("PTS", &["pts"]), ("CSV", &["csv"])]),
                    Export::Mesh => match &self.cloud.last {
                        Some(mesh) => ("Export mesh", file_name(&mesh.file), &[("OBJ", &["obj"]), ("PLY", &["ply"]), ("STL", &["stl"]), ("DXF", &["dxf"]), ("DWG", &["dwg"]), ("IFC", &["ifc"])]),
                        None => return Task::none(),
                    },
                    Export::Drawing if self.active_section().is_some() => ("Export section drawing", format!("{stem}-section.dxf"), &[("DXF", &["dxf"]), ("DWG", &["dwg"])]),
                    Export::Drawing => return Task::none(),
                };
                let mut dialog = rfd::AsyncFileDialog::new().set_title(title).set_file_name(name);
                if let Some(folder) = crate::managed::dialog_folder(cloud.file.parent().map(PathBuf::from), crate::managed::work_folder()) {
                    dialog = dialog.set_directory(folder);
                }
                for (name, extensions) in filters {
                    dialog = dialog.add_filter(*name, extensions);
                }
                return Task::perform(async move { dialog.save_file().await.map(|f| f.path().to_path_buf()) }, move |to| Message::Pointcloud(Event::ExportTo(what, to)));
            }
            Event::ExportTo(_, None) => {}
            Event::ExportTo(what, Some(to)) => {
                let Some(cloud) = self.cloud().filter(|_| !self.meshing) else { return Task::none() };
                let (settings, section) = (self.cloud.settings, self.cloud_box(&cloud));
                let saying = format!("Writing {}…", file_name(&to));
                match what {
                    Export::Points => return self.cloud_job(saying, move |stop| export_points(&cloud.file, &to, settings.share, section, stop).map(Done::Wrote)),
                    Export::Mesh => {
                        if let Some(mesh) = self.cloud.last.clone() {
                            return self.cloud_job(saying, move |_| export_mesh(&mesh.source, &to).map(Done::Wrote));
                        }
                    }
                    Export::Drawing => {
                        if let Some(section) = section {
                            return self.cloud_job(saying, move |stop| drawing(&cloud.file, &to, section, settings.view, stop).map(Done::Wrote));
                        }
                    }
                }
            }
            Event::Cancel => self.cloud.stop.store(true, Ordering::Relaxed),
            Event::Done(done) => {
                self.meshing = false;
                match done {
                    Ok(Done::Meshed(mesh)) => {
                        self.status = Status::Loaded(format!("Meshed: {} in {:.1} s", mesh.figures(), mesh.seconds));
                        let file = mesh.file.clone();
                        self.cloud.last = Some(mesh);
                        return self.open(loaders::Source::File(file), true);
                    }
                    Ok(Done::Wrote(text)) => self.status = Status::Loaded(text),
                    Err(e) => self.status = Status::Error(e),
                }
            }
        }
        Task::none()
    }

    /// The card of the product: what the file of the cloud holds, and how the jobs are done.
    pub fn cloud_card(&self, look: Look, l: Language) -> Element<'_, Message> {
        let go = |e: Event| Message::Pointcloud(e);
        let s = self.cloud.settings;
        let mut card = column![w::mono_label(look, l.text("Bestand", "File"))].spacing(6);
        let known = self.cloud().and_then(|cloud| self.cloud.details.as_ref().filter(|(file, _)| *file == cloud.file).map(|(_, d)| (cloud, d)));
        match (self.cloud(), known) {
            (None, _) => card = card.push(w::secondary(look, l.text("Geen puntenwolk geopend", "No point cloud open"))),
            (Some(_), None) => card = card.push(w::secondary(look, l.text("Het bestand wordt gelezen…", "Reading the file…"))),
            (Some(_), Some((_, Err(e)))) => card = card.push(w::secondary(look, e.clone())),
            (Some(_), Some((cloud, Ok(d)))) => {
                let yes_no = |on: bool| l.text(if on { "Ja" } else { "Nee" }, if on { "Yes" } else { "No" });
                let size = [0, 1, 2].map(|k| d.max[k] - d.min[k]);
                let line = |label: &'static str, value: String| w::property_row(look, label, w::body(look, value).into());
                card = card
                    .push(line(l.text("Naam", "Name"), file_name(&cloud.file)))
                    .push(line(l.text("Formaat", "Format"), d.format.to_uppercase()))
                    .push(line(l.text("Punten", "Points"), grouped(d.points)))
                    .push(line(l.text("Afmetingen", "Size"), format!("{:.2} × {:.2} × {:.2} m", size[0], size[1], size[2])))
                    .push(line(l.text("Laagste hoek", "Lowest corner"), format!("{:.3}, {:.3}, {:.3}", d.min[0], d.min[1], d.min[2])))
                    .push(line(l.text("Kleur", "Colour"), yes_no(d.rgb).to_string()))
                    .push(line(l.text("Intensiteit", "Intensity"), yes_no(d.intensity).to_string()))
                    .push(line(l.text("Classificatie", "Classification"), yes_no(d.classification).to_string()))
                    .push(line(l.text("Scanposities", "Scanner stations"), d.stations.to_string()))
                    .push(line(l.text("Foto's bij de scanposities", "Photos at the stations"), d.photos.to_string()));
            }
        }
        let pick = |v: Option<Vertices>| VERTICES.iter().copied().find(|c| Some(*c) == v);
        let views = View::ALL.into_iter().fold(row![].spacing(4), |views, view| {
            views.push(w::button(look, view.label(l), if s.view == view { K::Secondary } else { K::Quiet }, D::Compact, None, Some(go(Event::Set(Setting::View(view))))))
        });
        card.push(w::mono_label(look, l.text("Mesh", "Mesh")))
            .push(w::property_row(
                look,
                l.text("Hoekpunten (terrein, oppervlak)", "Vertices (terrain, surface)"),
                w::select(look, &VERTICES[..], pick(Some(Vertices(s.vertices))), move |c: Vertices| go(Event::Set(Setting::Vertices(c.0))), D::Regular),
            ))
            .push(w::property_row(
                look,
                l.text("Voxel (gesloten, oppervlak)", "Voxel (closed, surface)"),
                w::select(look, &VOXELS[..], Some(Voxel(s.voxel)), move |v: Voxel| go(Event::Set(Setting::Voxel(v.0))), D::Regular),
            ))
            .push(w::property_row(
                look,
                l.text("Gaten dichten tot (gesloten)", "Close gaps up to (closed)"),
                w::select(look, &HOLES[..], Some(Width(s.hole)), move |h: Width| go(Event::Set(Setting::Hole(h.0))), D::Regular),
            ))
            .push(w::property_row(
                look,
                l.text("Buren per hoekpunt (oppervlak)", "Neighbours per vertex (surface)"),
                w::select(look, &NEIGHBORS[..], Some(Vertices(s.neighbors)), move |n: Vertices| go(Event::Set(Setting::Neighbors(n.0))), D::Regular),
            ))
            .push(w::property_row(
                look,
                l.text("Langste rand, maal de gewone (oppervlak)", "Longest edge, times the usual (surface)"),
                w::select(look, &EDGES[..], Some(Vertices(s.edge)), move |e: Vertices| go(Event::Set(Setting::Edge(e.0))), D::Regular),
            ))
            .push(w::property_row(
                look,
                l.text("Aandeel van de punten (gesloten)", "Share of the points (closed)"),
                w::select(look, &SHARES[..], Some(Share(s.sample)), move |p: Share| go(Event::Set(Setting::Sample(p.0))), D::Regular),
            ))
            .push(w::secondary(look, match &self.cloud.last {
                Some(mesh) => format!("{}: {}", file_name(&mesh.file), mesh.figures()),
                None => String::new(),
            }))
            .push(w::mono_label(look, l.text("Exporteren", "Export")))
            .push(w::property_row(
                look,
                l.text("Aandeel van de punten (export)", "Share of the points (export)"),
                w::select(look, &SHARES[..], Some(Share(s.share)), move |p: Share| go(Event::Set(Setting::Share(p.0))), D::Regular),
            ))
            .push(w::property_row(look, l.text("Doorsnedetekening van", "Section drawing of"), views.into()))
            .push(w::secondary(
                look,
                l.text(
                    "Staat de doorsnedebox aan, dan gaan de meshes, de export van de punten en de tekening alleen over wat erin ligt.",
                    "With the section box on, the meshes, the export of the points and the drawing cover only what lies in it.",
                ),
            ))
            .into()
    }
}

fn file_name(file: &Path) -> String {
    file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The origin the loader took off the cloud, from the project information it left; zero when the
/// cloud is drawn in its own coordinates.
fn origin_of(info: &[(String, String)]) -> [f64; 3] {
    let Some((_, value)) = info.iter().find(|(key, _)| key.contains("world origin (source coordinates)")) else { return [0.0; 3] };
    let mut parts = value.split(',').map(|p| p.trim().parse::<f64>());
    match (parts.next(), parts.next(), parts.next()) {
        (Some(Ok(x)), Some(Ok(y)), Some(Ok(z))) => [x, y, z],
        _ => [0.0; 3],
    }
}

/// The section box of the view in the coordinates of a cloud's file: the library turns a box
/// about its own middle, the view about a pivot.
pub fn source_box(section: &SectionBox, origin: [f64; 3]) -> OrientedBox {
    let middle = section.to_world((section.min + section.max) * 0.5);
    let half = (section.max - section.min) * 0.5;
    let at = |k: usize| middle[k] as f64 + origin[k];
    let bounds = Bounds { min: std::array::from_fn(|k| at(k) - half[k] as f64), max: std::array::from_fn(|k| at(k) + half[k] as f64) };
    OrientedBox::new(bounds, (section.turn as f64).to_degrees())
}

/// Says no more once `stop` is set.
fn going(stop: &AtomicBool) -> Result<(), LoadError> {
    match stop.load(Ordering::Relaxed) {
        true => Err(LoadError::Cancelled),
        false => Ok(()),
    }
}

/// What the file of a cloud holds.
pub fn details(file: &Path) -> Result<Details, String> {
    let cloud = ops::open_source(file, &ops::never_stop).map_err(|e| format!("Cannot read {}: {e}", file_name(file)))?;
    let seen = ops::inspect(&cloud);
    Ok(Details {
        format: seen.format,
        points: seen.points,
        min: seen.bounds.min,
        max: seen.bounds.max,
        rgb: seen.attributes.rgb,
        intensity: seen.attributes.intensity,
        classification: seen.attributes.classification,
        stations: seen.stations.len(),
        photos: seen.station_photos,
    })
}

/// Meshes a cloud into two OBJ files in the system's temporary folder, named after the cloud:
/// one in the coordinates of the cloud's file (for export), one where the view draws the cloud.
/// With a `section`, only the points in it take part.
pub fn mesh(cloud: &Source, kind: Mesh, settings: &Settings, section: Option<OrientedBox>, stop: &AtomicBool) -> Result<Meshed, String> {
    let start = Instant::now();
    let name = file_name(&cloud.file);
    let say = |e: LoadError| format!("Cannot mesh {name}: {e}");
    let io = |e: std::io::Error| format!("Cannot mesh {name}: {e}");
    let folder = std::env::temp_dir().join("open-bim-suite").join("meshes");
    std::fs::create_dir_all(folder.join("source")).map_err(io)?;
    let stem = cloud.file.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "cloud".to_string());
    let file = format!("{stem}-{}.obj", kind.word());
    let (shown, source) = (folder.join(&file), folder.join("source").join(&file));
    let keep_going = || going(stop);
    let points = ops::open_source(&cloud.file, &keep_going).map_err(say)?;
    let inside = |p: &pointcloud_core::Point| section.is_none_or(|b| b.contains(p.xyz));
    let cell = settings.voxel.map_or(0.0, |mm| mm as f64 / 1000.0);
    let mut deviation = None;
    let mut faces = None;
    let (vertices, triangles) = match kind {
        Mesh::Terrain => {
            let config = pointcloud_core::MeshConfig { max_vertices: settings.vertices, ..Default::default() };
            let stats = pointcloud_core::mesh_terrain_obj_where_progress(&points, &source, config, |_, p| inside(p), |_| keep_going()).map_err(say)?;
            (stats.vertices, stats.triangles)
        }
        Mesh::Surface => {
            let config = pointcloud_core::SurfaceMeshConfig { max_vertices: settings.vertices, mesh_size: cell, neighbors: settings.neighbors, max_edge_factor: settings.edge as f64 };
            let stats = pointcloud_core::mesh_surface_obj_where_progress(&points, &source, config, |_, p| inside(p), |_| keep_going()).map_err(say)?;
            (stats.vertices, stats.triangles)
        }
        Mesh::Closed => {
            // the closed mesher reads tile by tile, through an index of the cloud on disk
            let index = ops::build_index(&points, &ops::IndexOptions::default(), &mut |_| keep_going()).map_err(say)?;
            let layers = [pointcloud_core::surfels::SurfelSource::new(&points, Some(&index), Default::default())];
            let config = pointcloud_core::ClosedMeshConfig { voxel: settings.voxel.map(|mm| mm as f64 / 1000.0), max_hole: settings.hole as f64 / 100.0, sample_percent: settings.sample as f64, ..Default::default() };
            let accept = |_: usize, _: u64, p: &pointcloud_core::Point| inside(p);
            let (mesh, report) = pointcloud_core::mesh_closed(&layers, section.map(|b| b.aabb()), &accept, &config, &mut |_| keep_going()).map_err(say)?;
            pointcloud_core::write_mesh(&mesh, &source, pointcloud_core::MeshFormat::Obj, &[]).map_err(say)?;
            deviation = (report.deviation.samples > 0).then_some((report.deviation.mean, report.deviation.p95));
            (mesh.vertices.len(), mesh.triangles.len())
        }
        Mesh::Faces => {
            use pointcloud_core::surfaces::{self, FaceClass};
            // read through an index of the cloud on disk, as the closed mesher
            let index = ops::build_index(&points, &ops::IndexOptions::default(), &mut |_| keep_going()).map_err(say)?;
            let layers = [surfaces::SurfaceSource { points: pointcloud_core::region_source::RegionSource::new(&points, Some(&index), Default::default()), cloud: Some(&points) }];
            let region = section.map_or(pointcloud_core::region_source::EVERYWHERE, |b| b.aabb());
            let accept = |_: usize, _: u64, p: &pointcloud_core::Point| inside(p);
            let found = surfaces::detect_surfaces(&layers, 0, region, &Default::default(), &accept, &mut |_| keep_going()).map_err(say)?;
            if found.planes.is_empty() && found.cylinders.is_empty() {
                return Err(format!("No faces found in {name}"));
            }
            surfaces::write_faces_obj(&found, &source, &[]).map_err(say)?;
            let count = |class: FaceClass| found.planes.iter().filter(|f| f.class == class).count();
            faces = Some([count(FaceClass::Wall), count(FaceClass::Floor), count(FaceClass::Ceiling), count(FaceClass::Sloped), found.cylinders.len()]);
            let mesh = pointcloud_core::read_obj_mesh(&source).map_err(say)?;
            (mesh.vertices.len(), mesh.triangles.len())
        }
    };
    to_view(&source, &shown, cloud.origin).map_err(io)?;

    // how the triangles hang together, corners at one position counted as one
    let topology = pointcloud_core::mesh_topology_by_position(&pointcloud_core::read_obj_mesh(&source).map_err(say)?);
    Ok(Meshed { file: shown, source, vertices, triangles, seconds: start.elapsed().as_secs_f32(), open_edges: topology.open_edges, parts: topology.components, deviation, faces })
}

/// Writes the mesh of `source`, in the coordinates of the cloud's file and Z up as the library
/// writes it, as it is shown: moved by the `origin` the loader took off the cloud, and Y up, as
/// an OBJ file is by convention and as it is read here. A point (x, y, z) becomes
/// (x − ox, z − oz, −(y − oy)); a normal turns the same way without the move. What follows the
/// three numbers of a line (a colour) stays.
fn to_view(source: &Path, shown: &Path, origin: [f64; 3]) -> std::io::Result<()> {
    let text = std::fs::read_to_string(source)?;
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let turned = line.strip_prefix("v ").map(|rest| ("v", rest, origin)).or_else(|| line.strip_prefix("vn ").map(|rest| ("vn", rest, [0.0; 3]))).and_then(|(word, rest, o)| {
            let mut parts = rest.split_whitespace();
            let (x, y, z) = (parts.next()?.parse::<f64>().ok()?, parts.next()?.parse::<f64>().ok()?, parts.next()?.parse::<f64>().ok()?);
            let (x, y, z) = (x - o[0], z - o[2], o[1] - y);
            let tail: Vec<&str> = parts.collect();
            // no minus before a zero
            let y_back = if z == 0.0 { 0.0 } else { z };
            Some(if tail.is_empty() { format!("{word} {x} {y} {y_back}") } else { format!("{word} {x} {y} {y_back} {}", tail.join(" ")) })
        });
        out.push_str(turned.as_deref().unwrap_or(line));
        out.push('\n');
    }
    std::fs::write(shown, out)
}

/// Writes the points of `file` to `to`, in the format its extension names: all of them, or
/// `share` percent of them spread evenly, and only those in `section` when there is one.
pub fn export_points(file: &Path, to: &Path, share: u8, section: Option<OrientedBox>, stop: &AtomicBool) -> Result<String, String> {
    let start = Instant::now();
    let say = |e: LoadError| format!("Cannot write {}: {e}", file_name(to));
    let format = ops::export_format_for_path(to).ok_or_else(|| format!("Cannot write {}: not a point cloud format", file_name(to)))?;
    let keep_going = || going(stop);
    let cloud = ops::open_source(file, &keep_going).map_err(say)?;
    let all = ops::Convert { format, section, max_points: None };
    let written = if share >= 100 {
        ops::convert(&cloud, to, all, &keep_going).map_err(say)?
    } else if section.is_none() {
        pointcloud_core::export_thin_percent_where(&cloud, to, format, cloud.total_points, share, |_, _| true).map_err(say)?
    } else {
        // the part in the box first, to a file beside the destination, then the share of that
        let part = to.with_file_name(format!(".{}.part.{}", file_name(to), to.extension().and_then(|e| e.to_str()).unwrap_or("laz")));
        let thinned = (|| {
            ops::convert(&cloud, &part, all, &keep_going)?;
            let boxed = ops::open_source(&part, &keep_going)?;
            pointcloud_core::export_thin_percent_where(&boxed, to, format, boxed.total_points, share, |_, _| true)
        })();
        let _ = std::fs::remove_file(&part);
        thinned.map_err(say)?
    };
    Ok(format!("Wrote {}: {} of {} points in {:.1} s", file_name(to), grouped(written), grouped(cloud.total_points), start.elapsed().as_secs_f32()))
}

/// Writes the mesh of `source` (an OBJ file in the coordinates of the cloud's file) to `to`, in
/// the format its extension names.
pub fn export_mesh(source: &Path, to: &Path) -> Result<String, String> {
    let say = |e: LoadError| format!("Cannot write {}: {e}", file_name(to));
    let format = pointcloud_core::MeshFormat::from_path(to).ok_or_else(|| format!("Cannot write {}: not a mesh format", file_name(to)))?;
    let mesh = pointcloud_core::read_obj_mesh(source).map_err(say)?;
    pointcloud_core::write_mesh(&mesh, to, format, &[]).map_err(say)?;
    Ok(format!("Wrote {}: {} vertices, {} triangles", file_name(to), grouped(mesh.vertices.len() as u64), grouped(mesh.triangles.len() as u64)))
}

/// Draws the points of `file` in a slab at a face of `section` (the top face for a plan) to `to`,
/// a DXF or DWG file.
pub fn drawing(file: &Path, to: &Path, section: OrientedBox, view: View, stop: &AtomicBool) -> Result<String, String> {
    let start = Instant::now();
    let options = ops::SectionDrawingOptions { view: view.of_library(), ..Default::default() };
    let layer = file.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "cloud".to_string());
    let keep_going = || going(stop);
    let drawn = ops::section_drawing_file(file, &layer, to, section, &options, ops::CachedIndex::Never, &keep_going).map_err(|e| format!("Cannot write {}: {e}", file_name(to)))?;
    Ok(format!("Wrote {}: {} points of a slab of {:.2} m in {:.1} s", file_name(to), grouped(drawn.stats.drawn_points), drawn.slab_thickness, start.elapsed().as_secs_f32()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    /// A folder of its own with a cloud in it: a floor of 30 by 30 points a metre apart, at
    /// survey coordinates when `far`.
    fn grid(name: &str, far: bool) -> (PathBuf, Source) {
        let folder = std::env::temp_dir().join(format!("o3v-cloud-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        // named after the test: the meshes of clouds of one name share their files
        let file = folder.join(format!("{name}.xyz"));
        let (ox, oy) = if far { (155_000.0, 463_000.0) } else { (0.0, 0.0) };
        let mut points = String::new();
        for x in 0..30 {
            for y in 0..30 {
                points.push_str(&format!("{} {} {}\n", ox + x as f64, oy + y as f64, (x + y) % 3));
            }
        }
        std::fs::write(&file, points).unwrap();
        let origin = if far { [ox, oy, 0.0] } else { [0.0; 3] };
        (folder, Source { file, origin })
    }

    fn heights(file: &Path, at: usize) -> Vec<f64> {
        let text = std::fs::read_to_string(file).unwrap();
        text.lines().filter_map(|l| l.strip_prefix("v ")).filter_map(|l| l.split_whitespace().nth(at)?.parse().ok()).collect()
    }

    #[test]
    fn a_grid_of_points_meshes_into_a_terrain_file() {
        let (folder, cloud) = grid("terrain", false);
        let done = mesh(&cloud, Mesh::Terrain, &Settings::default(), None, &AtomicBool::new(false)).unwrap();
        assert!(done.vertices >= 100 && done.triangles >= 100 && done.parts == 1 && done.open_edges > 0, "{done:?}");
        assert!(done.file.is_file() && done.file.file_name().unwrap() == "terrain-terrain.obj");
        // the shown file is Y up: the heights of the grid (0 to 2) are its second number; the
        // file for export keeps Z up
        let (shown, kept) = (heights(&done.file, 1), heights(&done.source, 2));
        assert!(!shown.is_empty() && shown.iter().all(|h| (-0.01..=2.01).contains(h)), "{:?}", &shown[..shown.len().min(5)]);
        assert!(kept.len() == shown.len() && kept.iter().all(|h| (-0.01..=2.01).contains(h)));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_mesh_of_a_far_cloud_is_shown_where_the_cloud_is_drawn() {
        let (folder, cloud) = grid("far", true);
        let done = mesh(&cloud, Mesh::Terrain, &Settings::default(), None, &AtomicBool::new(false)).unwrap();
        let (shown, kept) = (heights(&done.file, 0), heights(&done.source, 0));
        assert!(shown.iter().all(|x| (-0.01..=29.01).contains(x)), "{:?}", &shown[..shown.len().min(5)]);
        assert!(kept.iter().all(|x| (154_999.99..=155_029.01).contains(x)));
        // and it leaves in the formats of the library, at the coordinates of the cloud
        let to = folder.join("grid.ply");
        let said = export_mesh(&done.source, &to).unwrap();
        assert!(to.is_file() && said.contains("grid.ply"), "{said}");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_section_box_limits_a_mesh_and_an_export() {
        let (folder, cloud) = grid("box", false);
        // the box of the view over x and y from 9.5 to 20.5: 11 by 11 points
        let section = SectionBox { min: Vec3::new(9.5, 9.5, -1.0), max: Vec3::new(20.5, 20.5, 3.0), turn: 0.0, pivot: Vec3::new(15.0, 15.0, 1.0) };
        let boxed = source_box(&section, cloud.origin);
        assert!(boxed.contains([10.0, 20.0, 0.0]) && !boxed.contains([9.0, 20.0, 0.0]));
        let stop = AtomicBool::new(false);
        let done = mesh(&cloud, Mesh::Terrain, &Settings::default(), Some(boxed), &stop).unwrap();
        let xs = heights(&done.source, 0);
        assert!(!xs.is_empty() && xs.iter().all(|x| (9.4..=20.6).contains(x)), "{:?}", &xs[..xs.len().min(5)]);
        let to = folder.join("part.xyz");
        let said = export_points(&cloud.file, &to, 100, Some(boxed), &stop).unwrap();
        assert_eq!(std::fs::read_to_string(&to).unwrap().lines().count(), 121, "{said}");
        // a share of all points
        let thin = folder.join("thin.xyz");
        export_points(&cloud.file, &thin, 10, None, &stop).unwrap();
        assert_eq!(std::fs::read_to_string(&thin).unwrap().lines().count(), 90);
        // and a share of those in the box
        let both = folder.join("both.xyz");
        export_points(&cloud.file, &both, 50, Some(boxed), &stop).unwrap();
        let kept = std::fs::read_to_string(&both).unwrap().lines().count();
        assert!((60..=61).contains(&kept), "{kept}");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_turned_box_of_the_view_is_the_same_box_for_the_library() {
        let section = SectionBox { min: Vec3::new(0.0, 0.0, 0.0), max: Vec3::new(10.0, 2.0, 3.0), turn: std::f32::consts::FRAC_PI_2, pivot: Vec3::new(0.0, 0.0, 0.0) };
        let boxed = source_box(&section, [100.0, 200.0, 0.0]);
        for p in [Vec3::new(-1.0, 9.0, 1.0), Vec3::new(-1.0, 11.0, 1.0), Vec3::new(1.0, 5.0, 1.0), Vec3::new(-1.9, 0.1, 2.9)] {
            assert_eq!(section.contains(p), boxed.contains([p.x as f64 + 100.0, p.y as f64 + 200.0, p.z as f64]), "{p}");
        }
    }

    #[test]
    fn a_file_tells_what_it_holds_and_a_stopped_job_writes_nothing() {
        let (folder, cloud) = grid("details", false);
        let d = details(&cloud.file).unwrap();
        assert_eq!((d.points, d.format.as_str(), d.rgb), (900, "xyz", false));
        assert_eq!((d.min, d.max), ([0.0, 0.0, 0.0], [29.0, 29.0, 2.0]));
        let to = folder.join("stopped.laz");
        assert!(export_points(&cloud.file, &to, 100, None, &AtomicBool::new(true)).is_err());
        assert!(!to.exists());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_slab_of_the_section_box_is_drawn() {
        let (folder, cloud) = grid("drawing", false);
        let section = SectionBox { min: Vec3::new(-1.0, -1.0, -1.0), max: Vec3::new(30.0, 30.0, 2.05), turn: 0.0, pivot: Vec3::ZERO };
        let to = folder.join("plan.dxf");
        let said = drawing(&cloud.file, &to, source_box(&section, cloud.origin), View::Plan, &AtomicBool::new(false)).unwrap();
        assert!(to.is_file() && said.contains("plan.dxf"), "{said}");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_faces_of_a_room_are_found() {
        // a room of 4 by 3 m and 2.5 m high: its floor, ceiling and four walls, a point every 2.5 cm
        let folder = std::env::temp_dir().join(format!("o3v-cloud-faces-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("room.xyz");
        let (nx, ny, nz) = (160, 120, 100);
        let step = 0.025;
        let mut points = String::new();
        for i in 0..=nx {
            for j in 0..=ny {
                let (x, y) = (i as f64 * step, j as f64 * step);
                points.push_str(&format!("{x} {y} 0\n{x} {y} 2.5\n"));
            }
        }
        for k in 1..nz {
            let z = k as f64 * step;
            for i in 0..=nx {
                let x = i as f64 * step;
                points.push_str(&format!("{x} 0 {z}\n{x} 3 {z}\n"));
            }
            for j in 1..ny {
                let y = j as f64 * step;
                points.push_str(&format!("0 {y} {z}\n4 {y} {z}\n"));
            }
        }
        std::fs::write(&file, points).unwrap();
        let cloud = Source { file, origin: [0.0; 3] };
        let done = mesh(&cloud, Mesh::Faces, &Settings::default(), None, &AtomicBool::new(false)).unwrap();
        let [walls, floors, ceilings, _, _] = done.faces.unwrap();
        assert!(walls == 4 && floors + ceilings == 2, "{}", done.figures());
        assert!(done.file.is_file() && done.triangles > 0);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_point_size_steps_within_its_sizes() {
        let mut app = App::default();
        for _ in 0..10 {
            let _ = app.pointcloud_update(Event::PointSize(true));
        }
        assert_eq!(app.point_size, 9.0);
        for _ in 0..10 {
            let _ = app.pointcloud_update(Event::PointSize(false));
        }
        assert_eq!(app.point_size, 1.0);
    }

    #[test]
    fn counts_are_written_with_their_thousands_apart() {
        assert_eq!(grouped(1_250_000), "1\u{a0}250\u{a0}000");
        assert_eq!(grouped(999), "999");
    }
}
