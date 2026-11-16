//! The Pointcloud product: what it does with an open point cloud beyond showing it.
//!
//! A cloud is meshed from its file, not from the thinned points the view shows: the library of
//! Open Pointcloud Studio (`pointcloud-core`) reads the whole source and writes a mesh, which is
//! then added to the project as a model of its own, over the cloud.
//!
//! - **Terrain**: a surface over the points seen from above (ground, a site, a roof plane); no
//!   walls or overhangs.
//! - **Surface**: a surface through the points in all directions (walls, ceilings, objects).

use super::{App, Message, Status};
use crate::loaders;
use iced::Task;
use std::path::PathBuf;
use std::time::Instant;

/// The sizes a point is drawn at (pixels), from small to large.
pub const POINT_SIZES: [f32; 6] = [1.0, 1.5, 2.5, 4.0, 6.0, 9.0];

/// A way to mesh a cloud.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mesh {
    Terrain,
    Surface,
}

impl Mesh {
    /// What its file is named after, behind the cloud's own name.
    fn word(self) -> &'static str {
        match self {
            Mesh::Terrain => "terrain",
            Mesh::Surface => "surface",
        }
    }
}

/// A mesh made of a cloud: its file, its vertices and triangles, and how long it took.
#[derive(Debug, Clone)]
pub struct Meshed {
    pub file: PathBuf,
    pub vertices: usize,
    pub triangles: usize,
    pub seconds: f32,
}

#[derive(Debug, Clone)]
pub enum Event {
    /// Mesh the cloud of the project.
    Mesh(Mesh),
    Meshed(Result<Meshed, String>),
    /// Draw the points a step larger (true) or smaller.
    PointSize(bool),
}

impl App {
    /// The file of the cloud the Pointcloud product works on: the model picked in the explorer
    /// when that is a cloud, else the first cloud of the project.
    pub fn cloud_file(&self) -> Option<PathBuf> {
        let is_cloud = |m: &crate::model::Model| m.layers.iter().any(|l| l.points) && m.path.is_file();
        let picked = self.picked_model.and_then(|i| self.models.get(i)).map(|e| &e.model).filter(|m| is_cloud(m));
        picked.or_else(|| self.models.iter().map(|e| &e.model).find(|m| is_cloud(m))).map(|m| m.path.clone())
    }

    pub(super) fn pointcloud_update(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::PointSize(larger) => {
                let at = POINT_SIZES.iter().position(|&s| s >= self.point_size).unwrap_or(POINT_SIZES.len() - 1);
                let next = if larger { (at + 1).min(POINT_SIZES.len() - 1) } else { at.saturating_sub(1) };
                self.point_size = POINT_SIZES[next];
            }
            Event::Mesh(kind) => {
                let Some(cloud) = self.cloud_file().filter(|_| !self.meshing) else { return Task::none() };
                self.meshing = true;
                let name = cloud.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.status = Status::Loaded(format!("Meshing {name}…"));
                return Task::perform(
                    async move {
                        let (tx, rx) = iced::futures::channel::oneshot::channel();
                        std::thread::spawn(move || {
                            let _ = tx.send(mesh(&cloud, kind));
                        });
                        rx.await.unwrap_or_else(|_| Err("the meshing thread stopped".to_string()))
                    },
                    |done| Message::Pointcloud(Event::Meshed(done)),
                );
            }
            Event::Meshed(done) => {
                self.meshing = false;
                match done {
                    Ok(mesh) => {
                        self.status = Status::Loaded(format!("Meshed: {} vertices, {} triangles in {:.1} s", mesh.vertices, mesh.triangles, mesh.seconds));
                        return self.open(loaders::Source::File(mesh.file), true);
                    }
                    Err(e) => self.status = Status::Error(e),
                }
            }
        }
        Task::none()
    }
}

/// Meshes the cloud in `file` into an OBJ file of its own in the system's temporary folder,
/// named after the cloud.
pub fn mesh(file: &std::path::Path, kind: Mesh) -> Result<Meshed, String> {
    let start = Instant::now();
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let say = |e: pointcloud_core::LoadError| format!("Cannot mesh {name}: {e}");
    let folder = std::env::temp_dir().join("open-bim-suite").join("meshes");
    std::fs::create_dir_all(&folder).map_err(|e| format!("Cannot mesh {name}: {e}"))?;
    let stem = file.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "cloud".to_string());
    let out = folder.join(format!("{stem}-{}.obj", kind.word()));
    // the sample is what the library keeps in memory of the cloud; the meshers read the file
    let cloud = pointcloud_core::open(file, 200_000).map_err(say)?;
    let stats = match kind {
        Mesh::Terrain => pointcloud_core::mesh_terrain_obj(&cloud, &out, Default::default()),
        Mesh::Surface => pointcloud_core::mesh_surface_obj(&cloud, &out, Default::default()),
    }
    .map_err(say)?;
    y_up(&out).map_err(|e| format!("Cannot mesh {name}: {e}"))?;
    Ok(Meshed { file: out, vertices: stats.vertices, triangles: stats.triangles, seconds: start.elapsed().as_secs_f32() })
}

/// Turns the mesh file from Z up, as the library writes it, to Y up, as an OBJ file is by
/// convention and as it is read here and elsewhere: a point (x, y, z) becomes (x, z, -y), and so
/// do the normals. What follows the three numbers of a line (a colour) stays.
fn y_up(file: &std::path::Path) -> std::io::Result<()> {
    let text = std::fs::read_to_string(file)?;
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let turned = line.strip_prefix("v ").map(|rest| ("v", rest)).or_else(|| line.strip_prefix("vn ").map(|rest| ("vn", rest))).and_then(|(word, rest)| {
            let mut parts = rest.split_whitespace();
            let (x, y, z) = (parts.next()?, parts.next()?, parts.next()?);
            let back = match y.strip_prefix('-') {
                Some(positive) => positive.to_string(),
                None => format!("-{y}"),
            };
            let tail: Vec<&str> = parts.collect();
            Some(if tail.is_empty() { format!("{word} {x} {z} {back}") } else { format!("{word} {x} {z} {back} {}", tail.join(" ")) })
        });
        out.push_str(turned.as_deref().unwrap_or(line));
        out.push('\n');
    }
    std::fs::write(file, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grid_of_points_meshes_into_a_terrain_file() {
        let folder = std::env::temp_dir().join(format!("o3v-cloud-test-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let source = folder.join("grid.xyz");
        let mut points = String::new();
        for x in 0..30 {
            for y in 0..30 {
                points.push_str(&format!("{x} {y} {}\n", (x + y) % 3));
            }
        }
        std::fs::write(&source, points).unwrap();
        let done = mesh(&source, Mesh::Terrain).unwrap();
        assert!(done.vertices >= 100 && done.triangles >= 100, "{done:?}");
        assert!(done.file.is_file() && done.file.file_name().unwrap() == "grid-terrain.obj");
        // the file is Y up: the heights of the grid (0 to 2) are its second number
        let text = std::fs::read_to_string(&done.file).unwrap();
        let heights: Vec<f64> = text.lines().filter_map(|l| l.strip_prefix("v ")).filter_map(|l| l.split_whitespace().nth(1)?.parse().ok()).collect();
        assert!(!heights.is_empty() && heights.iter().all(|h| (-0.01..=2.01).contains(h)), "{:?}", &heights[..heights.len().min(5)]);
        let _ = std::fs::remove_file(&done.file);
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
}
