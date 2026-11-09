//! `--bench FILE`: loads a model and renders frames of the viewport pipeline offscreen
//! (1920x1080, 4x MSAA, orbiting camera), reporting load time and frame times; the last pass
//! cuts the model with a section box (lower 60 %, filled cuts). With `O3V_BENCH_PNG=path` the
//! first section frame is also written as a PNG. `WGPU_BACKEND` chooses the backend.
//!
//! These frame times are latencies: the bench waits for the GPU after every frame. With
//! `--throughput` it instead keeps two frames in flight, as a swap chain does, and checks the
//! 120 fps criterion per camera scenario (orbit, pan, zoom): at least 120 submitted frames per
//! second and a p95 frame time of at most 8.33 ms, counting the frames over 16.67 ms.
//!
//! Instead of a file, FILE may name a generated scenario: `synthetic:N` (N boxes, each its own
//! element), `structural:N` (a 3D frame of N members with their nodes, fixed supports on the
//! ground nodes and hinges at 10 % of the member ends), or several joined with `+`
//! (`structural:100000+synthetic:100000`), combined as Add does. For a model with structural
//! objects it also times, on the CPU, deriving the display, building the hover grid and a click,
//! and adds a pass (and a throughput run) with every third member selected.

use crate::loaders;
use crate::model::Model;
use crate::structural::display::Marks;
use crate::structural::StructuralPart;
use bim_structural::{
    dofs_all, frame, sam_id, AnalysisModel, Curve, Dof, DofFrame, Hinge, IdKind, Insertion, Member, MemberKind, Node, NodeOrigin,
    Orientation, SourceInfo, Support, SupportPreset,
};
use glam::DVec3;
use crate::selection::Pickable;
use crate::units::{unit_state, UNIT_HIDDEN, UNIT_SELECTED};
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

/// The GPU to bench: `O3V_BENCH_GPU=low` (or `integrated`) asks for the integrated one, for the
/// baseline on integrated graphics; otherwise the high-performance one.
pub fn power_preference(asked: Option<&str>) -> wgpu::PowerPreference {
    match asked {
        Some("low" | "integrated") => wgpu::PowerPreference::LowPower,
        _ => wgpu::PowerPreference::HighPerformance,
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
    let model = Arc::new(scenario(file)?);
    println!(
        "{}: {} layers, {} vertices, {} triangles, {} points, loaded in {:.2} s",
        model.file_name(),
        model.layers.len(),
        model.vertex_count(),
        model.triangle_count(),
        model.point_count(),
        model.load_time.as_secs_f32()
    );
    if let Some(d) = &model.structural_display {
        println!(
            "structural: {} nodes, {} members, {} segment instances, {} glyph instances",
            d.items.iter().filter(|i| i.kind == bim_structural::ItemKind::Node).count(),
            d.items.iter().filter(|i| i.kind == bim_structural::ItemKind::Member).count(),
            d.segs.len(),
            d.glyphs.len()
        );
        structural_timings(&model, (1920.0, 1080.0));
    }
    // every third member selected: drawn again on top from its own instances
    let third: Option<Arc<Marks>> = model.structural_display.as_ref().filter(|d| !d.is_empty()).map(|d| {
        let members = d.items.iter().enumerate().filter(|(_, i)| i.kind == bim_structural::ItemKind::Member);
        Arc::new(d.marks(members.step_by(3).map(|(k, _)| k as u32)))
    });

    // WGPU_BACKEND picks the backend, as for the window
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: power_preference(std::env::var("O3V_BENCH_GPU").ok().as_deref()),
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
        // the units of every element, as the app uploads them, so their flags work
        let index = Arc::new(crate::explode::ExplodeIndex::new(&model));
        let submit = |camera: Camera, units: &Arc<Vec<[f32; 4]>>, marked: &Arc<Marks>, pipeline: &mut ScenePipeline| {
            let primitive = ScenePrimitive {
                model: Some(model.clone()),
                visible: vec![true; model.layers.len()],
                camera,
                settings,
                explode: Some(index.clone()),
                offsets: units.clone(),
                structural_selected: marked.clone(),
                structural_hovered: Arc::default(),
                structural_shown: None,
                datum: None,
                overlay: Default::default(),
            };
            primitive.prepare(pipeline, &device, &queue, &bounds, &viewport);
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            primitive.render(pipeline, &mut encoder, &view, &clip);
            queue.submit([encoder.finish()])
        };
        let plain: Arc<Vec<[f32; 4]>> = Arc::new(unit_state(&[], &[]));
        // with elements, also every element selected and every second one hidden
        let none: Arc<Marks> = Arc::default();
        let mut runs = Scenario::ALL.map(|s| (s.name().to_string(), s, plain.clone(), none.clone())).to_vec();
        if index.len() > 1 {
            let flags = |f: &dyn Fn(usize) -> u8| Arc::new(unit_state(&[], &(0..index.len()).map(f).collect::<Vec<u8>>()));
            runs.push(("orbit, all selected".into(), Scenario::Orbit, flags(&|_| UNIT_SELECTED), none.clone()));
            runs.push(("orbit, half hidden".into(), Scenario::Orbit, flags(&|u| if u % 2 == 0 { UNIT_HIDDEN } else { 0 }), none.clone()));
        }
        if let Some(third) = &third {
            runs.push(("orbit, every 3rd member selected".into(), Scenario::Orbit, plain.clone(), third.clone()));
        }
        // the GPU upload, not counted
        submit(camera, &plain, &none, &mut pipeline);
        device.poll(wgpu::PollType::wait_indefinitely()).ok();
        let mut all = true;
        for (name, scenario, units, marked) in &runs {
            let scenario = *scenario;
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
                pending.push_back(submit(c, units, marked, &mut pipeline));
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
                name,
                s.frames,
                s.fps,
                s.p95_ms,
                s.max_ms,
                s.over_budget,
                if s.passes() { "PASS" } else { "FAIL" }
            );
        }
        // selecting, isolating and hiding every element must each be done within one frame
        let mut timed = |what: String, ms: f64| {
            let pass = ms <= MAX_P95_MS;
            all &= pass;
            println!("{what}: {ms:.2} ms in update: {}", if pass { "PASS" } else { "FAIL" });
        };
        if index.len() > 1 {
            let mut app = crate::app::App::default();
            app.set_model(model.clone());
            app.fit();
            let (vw, vh) = app.viewport_size;
            // a crossing box over the whole viewport: everything it touches
            let boxed = crate::viewport::Navigate::Box { x0: vw, y0: vh, x1: 0.0, y1: 0.0, width: vw, height: vh, done: true };
            let t = Instant::now();
            let _ = app.update(crate::app::Message::Navigate(boxed));
            timed(format!("box select {} elements", app.sel.len()), t.elapsed().as_secs_f64() * 1000.0);
            // a model of only structural objects has units but no layer to isolate an element of
            if let Some(first) = model.layers.iter().find_map(|l| l.elements.first()) {
                app.click(Some(Pickable::Element(first.0)), false);
                let t = Instant::now();
                let _ = app.update(crate::app::Message::IsolateSelection);
                timed(format!("isolate 1 of {} elements", app.hidden.len() + 1), t.elapsed().as_secs_f64() * 1000.0);
            }
        }
        if index.len() > 1 {
            let mut app = crate::app::App::default();
            app.set_model(model.clone());
            app.sel.extend(model.layers.iter().flat_map(|l| l.elements.iter().map(|e| Pickable::Element(e.0))));
            app.selection_changed();
            let t = Instant::now();
            let _ = app.update(crate::app::Message::HideSelection);
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            timed(format!("hide {} elements", app.hidden.len()), ms);
        }
        drop(timed);
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
    let mut passes = vec![(RenderMode::Solid, None, None), (RenderMode::SolidEdges, None, None), (RenderMode::SolidEdges, last, None)];
    if let Some(third) = &third {
        passes.push((RenderMode::Solid, None, Some(third.clone())));
    }
    for (mode, section, selected) in passes {
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
        let selected_marks: Arc<Marks> = selected.clone().unwrap_or_default();
        let frame = |camera: Camera, pipeline: &mut ScenePipeline| {
            let primitive = ScenePrimitive {
                model: Some(model.clone()),
                visible: vec![true; model.layers.len()],
                camera,
                settings,
                explode: None,
                offsets: Arc::default(),
                structural_selected: selected_marks.clone(),
                structural_hovered: Arc::default(),
                structural_shown: None,
                datum: None,
                overlay: Default::default(),
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
            "{mode:?}{}{} @ {w}x{h} 4xMSAA: first frame (upload) {:.1} ms, {frames} frames: mean {mean:.2} ms ({:.0} fps), median {median:.2} ms, p95 {p95:.2} ms",
            if section.is_some() { " + section box" } else { "" },
            match &selected {
                Some(m) => format!(" + every 3rd member selected ({} ranges)", m.segs.len() + m.glyphs.len()),
                None => String::new(),
            },
            first.as_secs_f64() * 1000.0,
            1000.0 / mean
        );
    }
    Ok(())
}

/// Times the CPU side of the structural objects for a viewport of `size` logical pixels: deriving
/// the display, building the hover grid (zoomed to fit and zoomed in) and a click that scans every
/// instance against one that looks in the grid. Medians of a few runs.
fn structural_timings(model: &Model, size: (f32, f32)) {
    use crate::structural::display::StructuralDisplay;
    use crate::structural::pick::{Lod, Scene, View};
    let median = |mut f: Box<dyn FnMut() -> usize + '_>| {
        let mut runs: Vec<(f64, usize)> = (0..5)
            .map(|_| {
                let t = Instant::now();
                let n = f();
                (t.elapsed().as_secs_f64() * 1000.0, n)
            })
            .collect();
        runs.sort_by(|a, b| a.0.total_cmp(&b.0));
        runs[2]
    };
    let (ms, _) = median(Box::new(|| StructuralDisplay::build(&model.structural, &model.origins).items.len()));
    println!("structural display derived in {ms:.1} ms");
    let Some(display) = model.structural_display.as_deref() else { return };
    let mut camera = Camera::default();
    camera.set_preset(ViewPreset::Iso);
    camera.fit(&model.bounds, size.0 / size.1);
    for (name, zoom) in [("zoomed to fit", 1.0), ("zoomed in 4x", 0.25)] {
        let mut c = camera;
        c.distance *= zoom;
        let aspect = size.0 / size.1;
        let view = View { view_proj: c.view_proj(aspect), model: glam::Mat4::IDENTITY, proj11: c.projection(aspect).y_axis.y, size: glam::Vec2::new(size.0, size.1) };
        let scene = Scene { display, units: &[], offsets: &[], view, lod: Lod::default(), shown_items: None };
        let (grid_ms, entries) = median(Box::new(|| scene.grid().len()));
        let grid = scene.grid();
        let cursor = glam::Vec2::new(size.0 * 0.5, size.1 * 0.5);
        let all = |_: glam::Vec3| true;
        let (scan_ms, _) = median(Box::new(|| scene.hits(cursor, &all).len()));
        let (cell_ms, _) = median(Box::new(|| grid.hits(&scene, cursor, &all).len()));
        println!(
            "structural {name}: hover grid built in {grid_ms:.1} ms ({entries} entries, off the UI thread), click {scan_ms:.2} ms scanning everything, {cell_ms:.3} ms in the grid"
        );
    }
}

/// The model of `--bench FILE`: a file, or generated scenarios joined with `+` (see the module
/// documentation).
pub fn scenario(file: &Path) -> Result<Model, String> {
    let name = file.to_string_lossy().into_owned();
    let generated = |part: &str| -> Option<Result<Model, String>> {
        let (kind, n) = part.split_once(':')?;
        if !matches!(kind, "synthetic" | "structural") {
            return None;
        }
        let Ok(n) = n.parse::<usize>() else { return Some(Err(format!("{part}: {kind}:N with N a whole number"))) };
        Some(Ok(if kind == "synthetic" { synthetic(n) } else { structural(n) }))
    };
    let parts: Vec<&str> = name.split('+').collect();
    if !parts.iter().all(|p| generated(p).is_some()) {
        return loaders::load(file);
    }
    let start = Instant::now();
    let mut models = Vec::new();
    for p in &parts {
        models.push(Arc::new(generated(p).expect("checked")?));
    }
    let mut model = match models.len() {
        1 => Arc::try_unwrap(models.pop().expect("one")).map_err(|_| "shared model".to_string())?,
        _ => Model::combine(&models),
    };
    model.path = name.into();
    model.load_time = start.elapsed();
    Ok(model)
}

/// A model of only [`structural_frame`]`(n)`.
pub fn structural(n: usize) -> Model {
    let mut model = Model::new(format!("structural:{n}").into(), "Structural frame (generated)", Vec::new());
    model.set_structural(vec![StructuralPart::new(0, Arc::new(structural_frame(n)))]);
    model
}

/// A 3D grid frame of `n` members (columns 4 m, beams 6 m both ways, up to ten storeys) with
/// their nodes, fixed supports on the ground nodes and a moment hinge at both ends of every tenth
/// member (10 % of the member ends).
pub fn structural_frame(n: usize) -> AnalysisModel {
    const BAY: f64 = 6.0;
    const STOREY: f64 = 4.0;
    let levels = ((n as f64 / 3.0).cbrt().round() as usize).clamp(1, 10);
    let k = ((n as f64 / (3.0 * levels as f64)).sqrt().ceil() as usize + 1).max(2);
    let lattice = |i: usize, j: usize, z: usize| (z * k + j) * k + i;
    let mut ends: Vec<(usize, usize)> = Vec::with_capacity(n);
    'levels: for z in 1..=levels {
        for j in 0..k {
            for i in 0..k {
                let column = Some((lattice(i, j, z - 1), lattice(i, j, z)));
                let beam_x = (i + 1 < k).then(|| (lattice(i, j, z), lattice(i + 1, j, z)));
                let beam_y = (j + 1 < k).then(|| (lattice(i, j, z), lattice(i, j + 1, z)));
                for member in [column, beam_x, beam_y].into_iter().flatten() {
                    if ends.len() == n {
                        break 'levels;
                    }
                    ends.push(member);
                }
            }
        }
    }
    let mut used = vec![false; k * k * (levels + 1)];
    for &(a, b) in &ends {
        used[a] = true;
        used[b] = true;
    }
    let at = |l: usize| DVec3::new((l % k) as f64 * BAY, (l / k % k) as f64 * BAY, (l / (k * k)) as f64 * STOREY);
    let node_id = |l: usize| sam_id(IdKind::Node, l as u64 + 1);
    let mut m = AnalysisModel::new(1, "Bench frame", SourceInfo::default());
    for l in (0..used.len()).filter(|&l| used[l]) {
        let ground = l < k * k;
        m.nodes.push(Node {
            id: node_id(l),
            number: (l + 1).to_string(),
            p: at(l),
            frame: None,
            support: ground.then(|| Support { dofs: dofs_all(Dof::Fixed), frame: DofFrame::Global, named: None, preset: SupportPreset::Fixed }),
            origin: NodeOrigin::Analytical,
        });
    }
    let hinge = Hinge { dofs: [Dof::Fixed, Dof::Fixed, Dof::Fixed, Dof::Fixed, Dof::Free, Dof::Free], named: None };
    for (e, &(a, b)) in ends.iter().enumerate() {
        m.members.push(Member {
            id: sam_id(IdKind::Member, e as u64 + 1),
            number: (e + 1).to_string(),
            kind: MemberKind::Beam,
            nodes: vec![node_id(a), node_id(b)],
            geometry: Curve::Line,
            z_ref: frame::default_z_ref(at(b) - at(a)),
            orientation: Orientation::Default,
            section: None,
            section_rotation: 0.0,
            insertion: Insertion::default(),
            releases: [0, 1].map(|_| (e % 10 == 0).then(|| hinge.clone())),
            material: None,
            links: Vec::new(),
        });
    }
    m.rebuild_index();
    m
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
            structural_selected: Arc::default(),
            structural_hovered: Arc::default(),
            structural_shown: None,
            datum: None,
            overlay: Default::default(),
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
    fn power_preference_picks_the_integrated_gpu_on_request() {
        assert_eq!(power_preference(Some("low")), wgpu::PowerPreference::LowPower);
        assert_eq!(power_preference(Some("integrated")), wgpu::PowerPreference::LowPower);
        assert_eq!(power_preference(Some("high")), wgpu::PowerPreference::HighPerformance);
        assert_eq!(power_preference(None), wgpu::PowerPreference::HighPerformance);
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

    #[test]
    fn the_structural_frame_has_n_members_supports_and_hinges() {
        for n in [1, 7, 300, 2000] {
            let f = structural_frame(n);
            assert_eq!(f.members.len(), n, "n = {n}");
            // every member end is a node of the model, and every node is used
            let used: std::collections::HashSet<u64> = f.members.iter().flat_map(|m| m.nodes.iter().copied()).collect();
            assert_eq!(used.len(), f.nodes.len());
            assert!(used.iter().all(|id| f.node(*id).is_some()));
            // supports on the ground nodes only
            assert!(f.nodes.iter().all(|nd| nd.support.is_some() == (nd.p.z == 0.0)));
            // 10 % of the member ends hinged: both ends of every tenth member
            let hinged: usize = f.members.iter().map(|m| m.releases.iter().filter(|r| r.is_some()).count()).sum();
            assert_eq!(hinged, 2 * n.div_ceil(10));
            assert!(f.members.iter().all(|m| m.releases[0].is_some() == m.releases[1].is_some()));
            // no member of zero length
            assert!(f.members.iter().all(|m| f.member_ends(m).is_some_and(|(a, b)| a.distance(b) > 1.0)));
        }
    }

    /// Renders the portal frame fixture offscreen, from the front with one member selected, and
    /// checks the axes, the nodes and the selection are drawn where they project; then in the iso
    /// view with one member selected and one hovered, and checks both are marked along their whole
    /// length. Writes `target/structural-renders/portal_frame.png` and `portal_frame_iso.png`.
    /// `cargo test --release --lib bench -- --ignored`.
    #[test]
    #[ignore = "needs a GPU; writes a PNG file"]
    fn renders_the_portal_frame() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/bim-structural/tests/fixtures/portal_frame.samjson");
        let model = Arc::new(loaders::load(&path).expect("portal frame"));
        let display = model.structural_display.clone().expect("a display");
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).expect("an adapter");
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("a device");
        let (w, h) = (960u32, 600u32);
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("test.target"),
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
        camera.set_preset(ViewPreset::Front);
        camera.fit(&model.bounds, w as f32 / h as f32);
        let p = ThemeKind::Light.palette();
        let settings = RenderSettings {
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
        };
        // the last member selected
        let member = display.items.iter().rposition(|i| i.kind == bim_structural::ItemKind::Member).unwrap() as u32;
        let viewport = Viewport::with_physical_size(iced::Size::new(w, h), 1.0);
        let bounds = iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(w as f32, h as f32));
        let mut render = |camera: Camera, selected: Marks, hovered: Marks, name: &str| {
            let primitive = ScenePrimitive {
                model: Some(model.clone()),
                visible: Vec::new(),
                camera,
                settings,
                explode: None,
                offsets: Arc::default(),
                structural_selected: Arc::new(selected),
                structural_hovered: Arc::new(hovered),
                structural_shown: None,
                datum: None,
                overlay: Default::default(),
            };
            primitive.prepare(&mut pipeline, &device, &queue, &bounds, &viewport);
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            primitive.render(&pipeline, &mut encoder, &view, &iced::Rectangle { x: 0, y: 0, width: w, height: h });
            queue.submit([encoder.finish()]);
            let rgba = read_target(&device, &queue, &target, (w, h)).expect("read back");
            let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/structural-renders");
            std::fs::create_dir_all(&dir).expect("folder");
            crate::write_png(&dir.join(name), w, h, &rgba).expect("png");
            rgba
        };
        let rgba = render(camera, display.marks([member]), Marks::default(), "portal_frame.png");
        let pixel = |rgba: &[u8], camera: &Camera, q: [f32; 3]| {
            let c = camera.view_proj(w as f32 / h as f32) * glam::Vec3::from(q).extend(1.0);
            let (x, y) = (((c.x / c.w + 1.0) * 0.5 * w as f32) as usize, ((1.0 - c.y / c.w) * 0.5 * h as f32) as usize);
            let at = 4 * (y * w as usize + x);
            [rgba[at], rgba[at + 1], rgba[at + 2]]
        };
        let near = |a: [u8; 3], b: [u8; 3], tol: i32| (0..3).all(|k| (a[k] as i32 - b[k] as i32).abs() <= tol);
        let bg = settings.background.map(|v| (v * 255.0).round() as u8);
        assert!(near([rgba[0], rgba[1], rgba[2]], bg, 2), "background in the corner");
        let axis = [28u8, 77, 135];
        let amber = [217u8, 119, 6];
        // the axis colour 65 % of the way to the hover tint (light background)
        let hover = [167u8, 130, 77];
        for (k, item) in display.items.iter().enumerate() {
            match item.kind {
                bim_structural::ItemKind::Member => {
                    let s = display.segs[item.segs.0 as usize];
                    let mid = [0, 1, 2].map(|i| 0.5 * (s.a[i] + s.b[i]));
                    let want = if k as u32 == member { amber } else { axis };
                    let got = pixel(&rgba, &camera, mid);
                    assert!(near(got, want, 40), "member {k}: {got:?}, want {want:?}");
                }
                _ => {
                    let g = display.glyphs[item.glyphs.0 as usize];
                    assert!(!near(pixel(&rgba, &camera, g.pos), bg, 30), "node {k} drawn: {:?}", pixel(&rgba, &camera, g.pos));
                }
            }
        }

        // in perspective from the side, members recede: their two ends lie at different depths. The
        // selected and the hovered member are wider on top and must win the depth test along their
        // whole length, not only on the half where the wider quad happens to lie in front.
        let mut iso = Camera::default();
        iso.set_preset(ViewPreset::Iso);
        iso.fit(&model.bounds, w as f32 / h as f32);
        let first = display.items.iter().position(|i| i.kind == bim_structural::ItemKind::Member).unwrap() as u32;
        let rgba = render(iso, display.marks([member]), display.marks([first]), "portal_frame_iso.png");
        let vp = iso.view_proj(w as f32 / h as f32);
        for (k, want) in [(member, amber), (first, hover)] {
            let s = display.segs[display.items[k as usize].segs.0 as usize];
            let (za, zb) = ((vp * glam::Vec3::from(s.a).extend(1.0)), (vp * glam::Vec3::from(s.b).extend(1.0)));
            assert!((za.z / za.w - zb.z / zb.w).abs() > 1e-4, "member {k} recedes");
            for t in [0.2f32, 0.35, 0.65, 0.8] {
                let q = [0, 1, 2].map(|i| s.a[i] + (s.b[i] - s.a[i]) * t);
                let got = pixel(&rgba, &iso, q);
                assert!(near(got, want, 40), "member {k} at {t}: {got:?}, want {want:?}");
            }
        }
    }

    #[test]
    fn scenarios_generate_and_combine() {
        let m = scenario(Path::new("structural:300")).unwrap();
        let d = m.structural_display.as_ref().expect("a display");
        assert_eq!(d.segs.len(), 300);
        assert_eq!(d.glyphs.len(), m.structural[0].doc.nodes.len());
        let both = scenario(Path::new("structural:300+synthetic:50")).unwrap();
        assert_eq!(both.triangle_count(), 50 * 12);
        assert_eq!(both.structural_display.as_ref().unwrap().segs.len(), 300);
        assert_eq!(both.file_name(), "structural:300+synthetic:50");
        assert!(scenario(Path::new("structural:many")).is_err());
    }
}
