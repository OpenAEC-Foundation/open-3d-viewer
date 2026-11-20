//! Adapter from the independent splat reader (crate splat-read, repository splat-dll) to the
//! app's scene model: SPZ files here, and the splat scenes the PLY and glTF loaders hand over.
//! A scene becomes one point layer with [`SplatData`]: its centres as the points (so it fits the
//! view and takes measurements like a cloud), its base colours and opacities as their colours.
//! Thinned out evenly to [`MAX_POINTS`](super::pointcloud::MAX_POINTS), and the view-dependent
//! colour lowered in degree when it would take too much memory.

use super::pointcloud::{origin_for, MAX_POINTS};
use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{Layer, ProjectData, SplatData};
use std::sync::Arc;

/// At most this many bytes of view-dependent colour coefficients are kept.
const MAX_SH_BYTES: usize = if cfg!(target_arch = "wasm32") { 256 << 20 } else { 1 << 30 };

pub struct SpzLoader;

impl Loader for SpzLoader {
    fn name(&self) -> &'static str {
        "SPZ"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["spz"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        progress.set(Phase::Convert);
        let splats = splat_read::spz::read(&data).map_err(|e| format!("{}: {e}", source.name()))?;
        Ok(loaded(splats, &source.stem()))
    }
}

/// A splat scene as the app loads it: one layer, with what the file holds as project information.
pub fn loaded(mut splats: splat_read::Splats, name: &str) -> Loaded {
    let total = splats.len();
    let stride = total.div_ceil(MAX_POINTS).max(1);
    splats.thin_out(stride);
    let file_degree = splats.sh_degree;
    while splats.sh_degree > 0 && splats.len() * splats.sh_count() * 3 * 4 > MAX_SH_BYTES {
        splats.limit_sh(splats.sh_degree - 1);
    }
    splats.to_z_up();
    let mut info = vec![("Splat format".to_string(), splats.format.clone()), ("Splats in file".to_string(), total.to_string())];
    if stride > 1 {
        info.push(("Splats shown".into(), format!("{} (1 in {stride})", splats.len())));
    }
    info.push(("View-dependent colour (degree)".into(), match splats.sh_degree == file_degree {
        true => file_degree.to_string(),
        false => format!("{} of {file_degree}", splats.sh_degree),
    }));
    let layer = layer(&splats, name, &mut info);
    Loaded { structural: Vec::new(), origin: None, layers: vec![layer], views: Vec::new(), project: ProjectData { info, ..ProjectData::default() } }
}

/// The layer of a splat scene (Z up); far centres move to a local origin, kept in `info`.
fn layer(splats: &splat_read::Splats, name: &str, info: &mut Vec<(String, String)>) -> Layer {
    let n = splats.len();
    let low = splats.positions.iter().fold([f64::INFINITY; 3], |m, p| [0, 1, 2].map(|k| m[k].min(p[k] as f64)));
    let origin = if n > 0 { origin_for(low) } else { [0.0; 3] };
    if origin != [0.0; 3] {
        info.push(("Splat world origin (source coordinates)".into(), format!("{}, {}, {}", origin[0], origin[1], origin[2])));
    }
    let mut positions = Vec::with_capacity(3 * n);
    let mut colors = Vec::with_capacity(n);
    let mut covariances = Vec::with_capacity(n);
    for i in 0..n {
        positions.extend((0..3).map(|k| (splats.positions[i][k] as f64 - origin[k]) as f32));
        let [r, g, b] = splats.base_color(i).map(|c| (c * 255.0).round() as u8);
        colors.push([r, g, b, (splats.opacities[i].clamp(0.0, 1.0) * 255.0).round() as u8]);
        covariances.push(splats.covariance(i));
    }
    let data = SplatData { covariances, sh_degree: splats.sh_degree, sh: splats.sh.clone(), sh_frame: splats.sh_frame, linear: splats.linear };
    Layer { name: name.to_string(), color: [0.8; 3], positions, colors: Some(colors), points: true, splats: Some(Arc::new(data)), ..Layer::default() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loaders::{load_with, Progress, Source};

    /// A small scene: a 10 x 10 grid of splats, flat in the file's ground plane (y = 0 with Y up),
    /// red to the one side and blue to the other, with a view-dependent colour of degree 1.
    pub fn scene(up: splat_read::Up) -> splat_read::Splats {
        let mut s = splat_read::Splats::new("test", up);
        s.sh_degree = 1;
        for i in 0..100 {
            let (x, z) = ((i % 10) as f32, (i / 10) as f32);
            s.positions.push([x, 0.0, z]);
            // wide and deep, thin in height
            s.scales.push([0.4, 0.02, 0.4]);
            s.rotations.push([0.0, 0.0, 0.0, 1.0]);
            s.opacities.push(0.9);
            let red = x / 9.0;
            s.dc.push([red, 0.0, 1.0 - red].map(|c| (c - 0.5) / splat_read::SH_C0));
            s.sh.extend([0.1; 9]);
        }
        s
    }

    fn load_bytes(name: &str, data: Vec<u8>) -> crate::model::Model {
        load_with(&Source::Bytes { name: name.into(), data: Arc::new(data) }, &Progress::default()).expect(name)
    }

    #[test]
    fn opens_spz_ply_and_gltf_splats_z_up() {
        let y_up = scene(splat_read::Up::Y);
        let spz = load_bytes("garden.spz", splat_read::spz::write(&y_up, 3));
        let glb = load_bytes("garden.glb", splat_read::gltf::write_glb(&y_up, [0.0; 3]));
        // the PLY of the training code is y down: the same scene upside down in the file
        let mut y_down = scene(splat_read::Up::YDown);
        for p in &mut y_down.positions {
            p[2] = -p[2];
        }
        let ply = load_bytes("garden.ply", splat_read::ply::write(&y_down));
        for (model, format) in [(&spz, "SPZ"), (&glb, "glTF"), (&ply, "PLY")] {
            assert_eq!(model.format, format);
            assert_eq!(model.layers.len(), 1, "{format}");
            let layer = &model.layers[0];
            let splats = layer.splats.as_ref().expect(format);
            assert!(layer.points);
            assert_eq!((model.point_count(), splats.covariances.len(), splats.sh_degree), (100, 100, 1), "{format}");
            // flat on the ground, Z up, 9 m by 9 m
            assert!(model.bounds.size()[2] < 1e-3 && (model.bounds.size()[0] - 9.0).abs() < 1e-2, "{format} {:?}", model.bounds);
            // thin in height: the smallest variance is along Z
            let c = splats.covariances[0];
            assert!(c[5] < 0.001 && c[0] > 0.1 && c[3] > 0.1, "{format} {c:?}");
            // red at x = 9, blue at x = 0, opacity in the fourth byte
            let colors = layer.colors.as_ref().unwrap();
            let x9 = (0..100).find(|&i| layer.positions[3 * i] > 8.5).unwrap();
            assert!(colors[x9][0] > 240 && colors[x9][2] < 15, "{format} {:?}", colors[x9]);
            assert!((colors[0][3] as i32 - 230).abs() <= 1);
            // up in the scene is up in the axes of its colour
            let up = splats.sh_frame.map(|row| row[2]);
            let expected = if format == "PLY" { [0.0, -1.0, 0.0] } else { [0.0, 1.0, 0.0] };
            assert!(up.iter().zip(expected).all(|(a, b)| (a - b).abs() < 1e-6), "{format} {up:?}");
            assert!(model.project.info.iter().any(|(k, v)| k == "Splats in file" && v == "100"));
        }
        // a plain point cloud PLY still opens as points
        let cloud = "ply\nformat ascii 1.0\nelement vertex 2\nproperty float x\nproperty float y\nproperty float z\nend_header\n0 0 0\n1 1 1\n";
        let model = load_bytes("scan.ply", cloud.as_bytes().to_vec());
        assert!(model.layers.iter().all(|l| l.points && l.splats.is_none()));
    }

    /// `O3V_SPLAT_SAMPLE=dir cargo test write_splat_samples -- --ignored`: writes a synthetic splat
    /// scene (a lawn with a ball of coloured splats on it, shinier from one side) as SPZ, PLY and
    /// glTF, for screenshots.
    #[test]
    #[ignore]
    fn write_splat_samples() {
        let Some(dir) = std::env::var_os("O3V_SPLAT_SAMPLE") else { return };
        let dir = std::path::PathBuf::from(dir);
        let mut s = splat_read::Splats::new("sample", splat_read::Up::Y);
        s.sh_degree = 1;
        let mut seed = 7u64;
        let mut rand = || {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (seed >> 11) as f32 / (1u64 << 53) as f32
        };
        let push = |s: &mut splat_read::Splats, p: [f32; 3], scale: [f32; 3], q: [f32; 4], rgb: [f32; 3], opacity: f32, shine: f32| {
            s.positions.push(p);
            s.scales.push(scale);
            s.rotations.push(splat_read::normalized(q));
            s.opacities.push(opacity);
            s.dc.push(rgb.map(|c| (c - 0.5) / splat_read::SH_C0));
            // brighter seen from +x (degree 1, m = 1 is x)
            s.sh.extend([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -shine, -shine, -shine]);
        };
        // a lawn: flat splats on y = 0 in a disc of 6 m
        for _ in 0..60_000 {
            let (r, a) = (6.0 * rand().sqrt(), rand() * std::f32::consts::TAU);
            let g = 0.35 + 0.3 * rand();
            push(&mut s, [r * a.cos(), 0.02 * rand(), r * a.sin()], [0.06, 0.01, 0.06], [0.0, rand(), 0.0, 1.0], [0.2 * g, g, 0.15 * g], 0.85, 0.0);
        }
        // a ball of 1 m on it, its colour running round it
        for _ in 0..40_000 {
            let (u, v) = (rand() * 2.0 - 1.0, rand() * std::f32::consts::TAU);
            let w = (1.0 - u * u).sqrt();
            let n = [w * v.cos(), u, w * v.sin()];
            let hue = v / std::f32::consts::TAU;
            let rgb = [0.5 + 0.5 * (6.283 * hue).cos(), 0.5 + 0.5 * (6.283 * (hue + 0.33)).cos(), 0.5 + 0.5 * (6.283 * (hue + 0.67)).cos()];
            push(&mut s, [n[0], 1.0 + n[1], n[2]], [0.05, 0.05, 0.008], [n[1], -n[0], 0.0, 1.0 + n[2]], rgb, 0.95, 0.4);
        }
        std::fs::write(dir.join("splat-ball.spz"), splat_read::spz::write(&s, 3)).unwrap();
        std::fs::write(dir.join("splat-ball.glb"), splat_read::gltf::write_glb(&s, [0.0; 3])).unwrap();
        let mut down = s.clone();
        down.up = splat_read::Up::YDown;
        for p in &mut down.positions {
            p[1] = -p[1];
        }
        std::fs::write(dir.join("splat-ball.ply"), splat_read::ply::write(&down)).unwrap();
    }

    #[test]
    fn opens_beside_a_model_and_turns_with_it() {
        let splats = Arc::new(load_bytes("garden.spz", splat_read::spz::write(&scene(splat_read::Up::Y), 3)));
        let cube = Arc::new(load_bytes("cube.obj", include_bytes!("../../test-models/test-cube.obj").to_vec()));
        let both = crate::model::Model::combine(&[cube.clone(), splats.clone()]);
        assert_eq!(both.layers.len(), cube.layers.len() + 1);
        let layer = both.layers.iter().find(|l| l.splats.is_some()).expect("the splat layer");
        assert_eq!(layer.point_count(), 100);
        // the view fits both
        for k in 0..3 {
            assert!(both.bounds.min[k] <= cube.bounds.min[k].min(splats.bounds.min[k]) + 1e-4);
            assert!(both.bounds.max[k] >= cube.bounds.max[k].max(splats.bounds.max[k]) - 1e-4);
        }
        // a quarter turn about the vertical turns the splats with their centres
        let turned = crate::model::Model::combine_placed(&[splats.clone()], &[], &[90.0]);
        let data = turned.layers[0].splats.as_ref().unwrap();
        let before = splats.layers[0].splats.as_ref().unwrap();
        // wide along x and y alike, so only the frame of the colour shows the turn
        assert!((data.covariances[0][0] - before.covariances[0][3]).abs() < 1e-5);
        let up = data.sh_frame.map(|row| row[2]);
        assert!((up[1] - 1.0).abs() < 1e-5, "{up:?}");
        let east = data.sh_frame.map(|row| row[1]);
        assert!((east[0] - 1.0).abs() < 1e-5, "{east:?}");
    }

    #[test]
    fn thins_out_and_moves_far_scenes() {
        let mut s = scene(splat_read::Up::Z);
        for p in &mut s.positions {
            p[0] += 155_000.0;
        }
        let loaded = loaded(s, "far");
        let layer = &loaded.layers[0];
        assert!(layer.positions[0].abs() < 1.0, "{}", layer.positions[0]);
        assert!(loaded.project.info.iter().any(|(k, _)| k.contains("origin")));
    }
}
