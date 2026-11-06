//! `--bench FILE`: loads a model and renders frames of the viewport pipeline offscreen
//! (1920x1080, 4x MSAA, orbiting camera), reporting load time and frame times; the last pass
//! cuts the model with a section box (lower 60 %, filled cuts). With `O3V_BENCH_PNG=path` the
//! first section frame is also written as a PNG. `WGPU_BACKEND` chooses the backend.
//!
//! These frame times are latencies: the bench waits for the GPU after every frame. With
//! `--throughput` it instead keeps two frames in flight, as a swap chain does, and checks the
//! 120 fps criterion per camera scenario (orbit, pan, zoom): at least 120 submitted frames per
//! second and a p95 frame time of at most 8.33 ms, counting the frames over 16.67 ms.

use crate::loaders;
use crate::theme::{srgb3, srgb4, ThemeKind};
use crate::viewport::camera::{Camera, ViewPreset};
use crate::viewport::render::{RenderMode, RenderSettings, ScenePipeline, ScenePrimitive};
use iced::futures::executor::block_on;
use iced::wgpu;
use iced::widget::shader::{Primitive, Viewport};
use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};
use web_time::Instant;

/// The 120 fps criterion: submitted frames per second, p95 frame time, and the time after which
/// a frame counts as late.
const MIN_FPS: f64 = 120.0;
const MAX_P95_MS: f64 = 8.33;
const LATE_MS: f64 = 16.67;
/// Frames in flight in the throughput mode, as with a double-buffered swap chain.
const IN_FLIGHT: usize = 2;

/// Frame times of one scenario.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameStats {
    pub frames: u32,
    pub fps: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
    pub over_budget: u32,
}

impl FrameStats {
    /// Meets the 120 fps criterion.
    pub fn passes(&self) -> bool {
        self.fps >= MIN_FPS && self.p95_ms <= MAX_P95_MS
    }
}

/// Stats of `durations_ms` (one per frame) over `wall_s` seconds: p95 by nearest rank.
pub fn frame_stats(durations_ms: &[f64], wall_s: f64) -> FrameStats {
    let mut sorted = durations_ms.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    let p95_ms = if n == 0 { 0.0 } else { sorted[((0.95 * n as f64).ceil() as usize).clamp(1, n) - 1] };
    FrameStats {
        frames: n as u32,
        fps: if wall_s > 0.0 { n as f64 / wall_s } else { 0.0 },
        p95_ms,
        max_ms: sorted.last().copied().unwrap_or(0.0),
        over_budget: sorted.iter().filter(|&&d| d > LATE_MS).count() as u32,
    }
}

/// How the camera moves from frame to frame in the throughput mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scenario {
    Orbit,
    Pan,
    Zoom,
}

impl Scenario {
    pub const ALL: [Scenario; 3] = [Scenario::Orbit, Scenario::Pan, Scenario::Zoom];

    fn name(self) -> &'static str {
        match self {
            Scenario::Orbit => "orbit",
            Scenario::Pan => "pan",
            Scenario::Zoom => "zoom",
        }
    }

    /// Moves the camera for `frame`; pan and zoom turn back every 100 frames, so the model
    /// stays in view.
    fn step(self, camera: &mut Camera, frame: u32, height: f32) {
        let back = if (frame / 100) % 2 == 0 { 1.0 } else { -1.0 };
        match self {
            Scenario::Orbit => camera.orbit(4.0, 0.0),
            Scenario::Pan => camera.pan(3.0 * back, 0.0, height),
            Scenario::Zoom => camera.zoom(0.02 * back),
        }
    }
}

/// Boxes per layer of the synthetic model.
const SYNTHETIC_LAYER: usize = 10_000;

/// A reference model that needs no file (`--bench synthetic:N`): `n` boxes of 1 m with 0.5 m
/// gaps on a square grid, each box its own element (ids 1..=n, 12 triangles), 10 000 elements
/// per layer.
pub fn synthetic(n: usize) -> crate::model::Model {
    use crate::model::{Layer, Model};
    // the six faces of a unit box: normal and four corners, counter-clockwise from outside
    const FACES: [([f32; 3], [[f32; 3]; 4]); 6] = [
        ([0.0, 0.0, -1.0], [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, 0.0, 0.0]]),
        ([0.0, 0.0, 1.0], [[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]]),
        ([0.0, -1.0, 0.0], [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]]),
        ([0.0, 1.0, 0.0], [[0.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0]]),
        ([-1.0, 0.0, 0.0], [[0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 1.0], [0.0, 1.0, 0.0]]),
        ([1.0, 0.0, 0.0], [[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0], [1.0, 0.0, 1.0]]),
    ];
    let side = (n as f64).sqrt().ceil().max(1.0) as usize;
    let mut layers = Vec::new();
    for (part, first) in (0..n).step_by(SYNTHETIC_LAYER).enumerate() {
        let mut layer = Layer { name: format!("Boxes {}", part + 1), color: [0.75, 0.72, 0.66], ..Layer::default() };
        for i in first..(first + SYNTHETIC_LAYER).min(n) {
            let (x, y) = ((i % side) as f32 * 1.5, (i / side) as f32 * 1.5);
            let id = i as u64 + 1;
            let start = layer.indices.len() as u32;
            for (normal, corners) in FACES {
                let base = layer.vertex_count() as u32;
                for c in corners {
                    layer.positions.extend_from_slice(&[x + c[0], y + c[1], c[2]]);
                    layer.normals.extend_from_slice(&normal);
                }
                layer.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }
            layer.elements.push((id, start, 36));
            layer.props.push((id, format!(r#"{{"Name":"Box {id}"}}"#)));
        }
        layers.push(layer);
    }
    Model::new(format!("synthetic-{n}").into(), "Synthetic", layers)
}

pub fn run(file: &Path, frames: u32, throughput: bool) -> Result<(), String> {
    let model = Arc::new(match file.to_str().and_then(|f| f.strip_prefix("synthetic:")) {
        Some(n) => synthetic(n.parse().map_err(|_| "synthetic:N needs a number of boxes")?),
        None => loaders::load(file)?,
    });
    println!(
        "{}: {} layers, {} vertices, {} triangles, {} points, loaded in {:.2} s",
        model.file_name(),
        model.layers.len(),
        model.vertex_count(),
        model.triangle_count(),
        model.point_count(),
        model.load_time.as_secs_f32()
    );

    // WGPU_BACKEND picks the backend, as for the window
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .map_err(|e| e.to_string())?;
    let info = adapter.get_info();
    println!("adapter: {} ({:?})", info.name, info.backend);
    let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).map_err(|e| e.to_string())?;

    let (w, h) = (1920u32, 1080u32);
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("bench.target"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mut pipeline = ScenePipeline::new(&device, format);
    let viewport = Viewport::with_physical_size(iced::Size::new(w, h), 1.0);
    let bounds = iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(w as f32, h as f32));
    let clip = iced::Rectangle { x: 0, y: 0, width: w, height: h };

    let mut camera = Camera::default();
    camera.set_preset(ViewPreset::Iso);
    camera.fit(&model.bounds, w as f32 / h as f32);
    let p = ThemeKind::Light.palette();

    if throughput {
        let settings = RenderSettings {
            mode: RenderMode::Solid,
            grid: true,
            background: srgb3(p.viewport),
            grid_minor: srgb4(p.grid_minor),
            grid_major: srgb4(p.grid_major),
            model_matrix: glam::Mat4::IDENTITY,
            section: None,
            plane: None,
            caps: false,
            grids: true,
            bounds: None,
        };
        let submit = |camera: Camera, pipeline: &mut ScenePipeline| {
            let primitive = ScenePrimitive {
                model: Some(model.clone()),
                visible: vec![true; model.layers.len()],
                camera,
                settings,
                explode: None,
                offsets: Arc::default(),
            };
            primitive.prepare(pipeline, &device, &queue, &bounds, &viewport);
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            primitive.render(pipeline, &mut encoder, &view, &clip);
            queue.submit([encoder.finish()])
        };
        // the GPU upload, not counted
        submit(camera, &mut pipeline);
        device.poll(wgpu::PollType::wait_indefinitely()).ok();
        let mut all = true;
        for scenario in Scenario::ALL {
            let done: Arc<Mutex<Vec<Instant>>> = Arc::default();
            let mut pending = VecDeque::with_capacity(IN_FLIGHT + 1);
            let mut c = camera;
            let start = Instant::now();
            for i in 0..frames {
                if pending.len() >= IN_FLIGHT {
                    let oldest = pending.pop_front();
                    device.poll(wgpu::PollType::Wait { submission_index: oldest, timeout: None }).ok();
                }
                scenario.step(&mut c, i, h as f32);
                pending.push_back(submit(c, &mut pipeline));
                let done = done.clone();
                queue.on_submitted_work_done(move || done.lock().unwrap().push(Instant::now()));
            }
            device.poll(wgpu::PollType::wait_indefinitely()).ok();
            let ends = done.lock().unwrap().clone();
            let durations: Vec<f64> =
                std::iter::once(start).chain(ends.iter().copied()).collect::<Vec<_>>().windows(2).map(|w| (w[1] - w[0]).as_secs_f64() * 1000.0).collect();
            let wall = ends.last().map_or(0.0, |t| (*t - start).as_secs_f64());
            let s = frame_stats(&durations, wall);
            all &= s.passes();
            println!(
                "{} @ {w}x{h} 4xMSAA: {} frames, {:.0} fps, p95 {:.2} ms, max {:.2} ms, {} over 16.67 ms: {}",
                scenario.name(),
                s.frames,
                s.fps,
                s.p95_ms,
                s.max_ms,
                s.over_budget,
                if s.passes() { "PASS" } else { "FAIL" }
            );
        }
        println!("120 fps criterion: {}", if all { "PASS" } else { "FAIL" });
        return Ok(());
    }

    // lower 60 % of the model, a little turned
    let mut section = crate::section::SectionBox::around(&model.bounds);
    section.max.z = section.min.z + (section.max.z - section.min.z) * 0.6;
    section.turn_by(0.3);
    // or, with `O3V_BENCH_VIEW=name`, a view saved in the file as the app shows it
    let mut view_camera = None;
    if let Ok(name) = std::env::var("O3V_BENCH_VIEW") {
        let i = model.views.iter().position(|v| v.name == name).ok_or_else(|| format!("no view {name:?}"))?;
        let mut app = crate::app::App::default();
        app.viewport_size = (w as f32, h as f32);
        app.set_model(model.clone());
        app.apply_view(i);
        view_camera = app.anim.map(|a| a.1);
        match app.active_section() {
            Some(s) => section = s,
            None => section.max = section.min, // marks "no box" below
        }
    }
    let last = (section.max != section.min).then_some(section);
    for (mode, section) in [(RenderMode::Solid, None), (RenderMode::SolidEdges, None), (RenderMode::SolidEdges, last)] {
        let is_last = mode == RenderMode::SolidEdges && (section.is_some() || last.is_none()) && section == last;
        let camera = match (is_last, view_camera) {
            (true, Some(c)) => c,
            _ => camera,
        };
        let settings = RenderSettings {
            mode,
            grid: true,
            background: srgb3(p.viewport),
            grid_minor: srgb4(p.grid_minor),
            grid_major: srgb4(p.grid_major),
            model_matrix: glam::Mat4::IDENTITY,
            section,
            plane: None,
            caps: section.is_some(),
            grids: true,
            bounds: None,
        };
        let frame = |camera: Camera, pipeline: &mut ScenePipeline| {
            let primitive = ScenePrimitive {
                model: Some(model.clone()),
                visible: vec![true; model.layers.len()],
                camera,
                settings,
                explode: None,
                offsets: Arc::default(),
            };
            primitive.prepare(pipeline, &device, &queue, &bounds, &viewport);
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            primitive.render(pipeline, &mut encoder, &view, &clip);
            queue.submit([encoder.finish()]);
            device.poll(wgpu::PollType::wait_indefinitely()).ok();
        };
        // First frame includes the GPU upload
        let t = Instant::now();
        frame(camera, &mut pipeline);
        let first = t.elapsed();
        if let (true, Some(path)) = (is_last && (section.is_some() || view_camera.is_some()), std::env::var_os("O3V_BENCH_PNG")) {
            write_target(&device, &queue, &target, (w, h), std::path::Path::new(&path))?;
        }
        let mut times = Vec::with_capacity(frames as usize);
        for i in 0..frames {
            let mut c = camera;
            c.yaw += i as f32 * 0.02;
            let t = Instant::now();
            frame(c, &mut pipeline);
            times.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mean = times.iter().sum::<f64>() / times.len().max(1) as f64;
        let median = times.get(times.len() / 2).copied().unwrap_or(0.0);
        let p95 = times.get(times.len() * 95 / 100).copied().unwrap_or(0.0);
        println!(
            "{mode:?}{} @ {w}x{h} 4xMSAA: first frame (upload) {:.1} ms, {frames} frames: mean {mean:.2} ms ({:.0} fps), median {median:.2} ms, p95 {p95:.2} ms",
            if section.is_some() { " + section box" } else { "" },
            first.as_secs_f64() * 1000.0,
            1000.0 / mean
        );
    }
    Ok(())
}

/// Copies the render target to the CPU and writes it as a PNG.
fn write_target(device: &wgpu::Device, queue: &wgpu::Queue, target: &wgpu::Texture, (w, h): (u32, u32), path: &Path) -> Result<(), String> {
    let rgba = read_target(device, queue, target, (w, h))?;
    crate::write_png(path, w, h, &rgba)?;
    println!("wrote {}", path.display());
    Ok(())
}

/// Copies the render target to the CPU: RGBA8, row by row.
fn read_target(device: &wgpu::Device, queue: &wgpu::Queue, target: &wgpu::Texture, (w, h): (u32, u32)) -> Result<Vec<u8>, String> {
    let row = (w * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("bench.readback"),
        size: (row * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) },
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    queue.submit([encoder.finish()]);
    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| e.to_string())?;
    let data = buffer.slice(..).get_mapped_range();
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let start = (y * row) as usize;
        rgba.extend_from_slice(&data[start..start + (w * 4) as usize]);
    }
    Ok(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::explode::ExplodeIndex;
    use crate::model::{Layer, Model};
    use crate::units::{unit_state, UNIT_HIDDEN};

    /// The colour in the middle of `model` rendered from above (64x64, solid), with `offsets`
    /// as the units texture.
    fn render_middle(model: &Arc<Model>, index: &Arc<ExplodeIndex>, offsets: Arc<Vec<[f32; 4]>>) -> [u8; 4] {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).expect("adapter");
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("device");
        let (w, h) = (64u32, 64u32);
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let mut pipeline = ScenePipeline::new(&device, format);
        let mut camera = Camera::default();
        camera.set_preset(ViewPreset::Top);
        camera.fit(&model.bounds, 1.0);
        let p = ThemeKind::Light.palette();
        let primitive = ScenePrimitive {
            model: Some(model.clone()),
            visible: vec![true; model.layers.len()],
            camera,
            settings: RenderSettings {
                mode: RenderMode::Solid,
                grid: false,
                background: srgb3(p.viewport),
                grid_minor: srgb4(p.grid_minor),
                grid_major: srgb4(p.grid_major),
                model_matrix: glam::Mat4::IDENTITY,
                section: None,
                plane: None,
                caps: false,
                grids: false,
                bounds: None,
            },
            explode: Some(index.clone()),
            offsets,
        };
        let bounds = iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(w as f32, h as f32));
        primitive.prepare(&mut pipeline, &device, &queue, &bounds, &Viewport::with_physical_size(iced::Size::new(w, h), 1.0));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        primitive.render(&pipeline, &mut encoder, &view, &iced::Rectangle { x: 0, y: 0, width: w, height: h });
        queue.submit([encoder.finish()]);
        let rgba = read_target(&device, &queue, &target, (w, h)).expect("read back");
        let at = ((h / 2 * w + w / 2) * 4) as usize;
        [rgba[at], rgba[at + 1], rgba[at + 2], rgba[at + 3]]
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn hidden_unit_is_not_drawn() {
        // two unit squares facing +Z: red element 10 at z = 0 under green element 20 at z = 1
        let mut positions = Vec::new();
        for z in [0.0, 1.0] {
            positions.extend_from_slice(&[0.0, 0.0, z, 1.0, 0.0, z, 1.0, 1.0, z, 0.0, 1.0, z]);
        }
        let colors = [[255, 0, 0, 255]; 4].into_iter().chain([[0, 255, 0, 255]; 4]).collect();
        let layer = Layer {
            name: "Squares".into(),
            positions,
            normals: [0.0, 0.0, 1.0].repeat(8),
            colors: Some(colors),
            indices: vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7],
            elements: vec![(10, 0, 6), (20, 6, 6)],
            ..Layer::default()
        };
        let model = Arc::new(Model::new("squares".into(), "Test", vec![layer]));
        let index = Arc::new(ExplodeIndex::new(&model));
        let shown = render_middle(&model, &index, Arc::new(unit_state(&[], &[])));
        assert!(shown[1] > shown[0], "the green square on top: {shown:?}");
        let mut flags = vec![0u8; index.len()];
        flags[index.unit(0, 4)] = UNIT_HIDDEN;
        let hidden = render_middle(&model, &index, Arc::new(unit_state(&[], &flags)));
        assert!(hidden[0] > hidden[1], "the red square once the green one is hidden: {hidden:?}");
    }

    #[test]
    fn frame_stats_counts_frames_over_budget() {
        let s = frame_stats(&[5.0, 5.0, 20.0, 8.0], 0.038);
        assert_eq!(s.frames, 4);
        assert_eq!(s.over_budget, 1);
        assert_eq!(s.max_ms, 20.0);
        assert_eq!(s.p95_ms, 20.0);
        assert!((s.fps - 105.26).abs() < 0.01);
    }

    #[test]
    fn synthetic_has_one_element_per_box() {
        let m = synthetic(25);
        let ids: std::collections::BTreeSet<u64> = m.layers.iter().flat_map(|l| l.elements.iter().map(|e| e.0)).collect();
        assert_eq!(ids, (1..=25).collect());
        assert_eq!(m.triangle_count(), 25 * 12);
        assert_eq!(m.element_props(7).first().map(|(k, v)| (k.as_str(), v.as_str())), Some(("Name", "Box 7")));
    }

    #[test]
    fn passes_needs_fps_and_p95() {
        let s = |fps, p95_ms| FrameStats { frames: 300, fps, p95_ms, max_ms: p95_ms, over_budget: 0 };
        assert!(s(130.0, 8.0).passes());
        assert!(!s(130.0, 9.0).passes());
        assert!(!s(110.0, 7.0).passes());
    }
}
