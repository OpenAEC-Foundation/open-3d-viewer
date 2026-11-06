//! `--bench FILE`: loads a model and renders frames of the viewport pipeline offscreen
//! (1920x1080, 4x MSAA, orbiting camera), reporting load time and frame times; the last pass
//! cuts the model with a section box (lower 60 %, filled cuts). With `O3V_BENCH_PNG=path` the
//! first section frame is also written as a PNG. `WGPU_BACKEND` chooses the backend.
//!
//! Instead of a file, FILE may name a generated scenario: `synthetic:N` (N boxes, each its own
//! element), `structural:N` (a 3D frame of N members with their nodes, fixed supports on the
//! ground nodes and hinges at 10 % of the member ends), or several joined with `+`
//! (`structural:100000+synthetic:100000`), combined as Add does.

use crate::loaders;
use crate::model::{Layer, Model};
use crate::structural::StructuralPart;
use bim_structural::{
    dofs_all, frame, sam_id, AnalysisModel, Curve, Dof, DofFrame, Hinge, IdKind, Insertion, Member, MemberKind, Node, NodeOrigin,
    Orientation, SourceInfo, Support, SupportPreset,
};
use glam::DVec3;
use crate::theme::{srgb3, srgb4, ThemeKind};
use crate::viewport::camera::{Camera, ViewPreset};
use crate::viewport::render::{RenderMode, RenderSettings, ScenePipeline, ScenePrimitive};
use iced::futures::executor::block_on;
use iced::wgpu;
use iced::widget::shader::{Primitive, Viewport};
use std::path::Path;
use std::sync::Arc;
use web_time::Instant;

pub fn run(file: &Path, frames: u32) -> Result<(), String> {
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
    }

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
                selection: Vec::new(),
                camera,
                settings,
                explode: None,
                offsets: Arc::default(),
                structural_selected: Arc::default(),
                structural_hovered: Arc::default(),
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

/// `n` boxes of 1 m with 0.5 m gaps on a square grid, each its own element (ids 1..=n, 12
/// triangles), 10 000 elements per layer (`Boxes 1`, `Boxes 2`, ...), properties
/// `{"Name":"Box i"}`.
pub fn synthetic(n: usize) -> Model {
    const PER_LAYER: usize = 10_000;
    let side = (n as f64).sqrt().ceil().max(1.0) as usize;
    let mut layers = Vec::new();
    for (l, first) in (0..n).step_by(PER_LAYER).enumerate() {
        let mut layer = Layer { name: format!("Boxes {}", l + 1), color: [0.72, 0.72, 0.74], ..Layer::default() };
        for i in first..(first + PER_LAYER).min(n) {
            let (x, y) = ((i % side) as f32 * 1.5, (i / side) as f32 * 1.5);
            let start = layer.indices.len() as u32;
            push_box(&mut layer, [x, y, 0.0], [x + 1.0, y + 1.0, 1.0]);
            let id = i as u64 + 1;
            layer.elements.push((id, start, layer.indices.len() as u32 - start));
            layer.props.push((id, format!("{{\"Name\":\"Box {id}\"}}")));
        }
        layers.push(layer);
    }
    Model::new(format!("synthetic:{n}").into(), "Synthetic boxes", layers)
}

/// A box from `lo` to `hi`, flat-shaded (4 vertices per face).
fn push_box(layer: &mut Layer, lo: [f32; 3], hi: [f32; 3]) {
    for axis in 0..3 {
        for high in [false, true] {
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            let base = (layer.positions.len() / 3) as u32;
            for (a, b) in [(false, false), (true, false), (true, true), (false, true)] {
                let mut p = [0.0f32; 3];
                p[axis] = if high { hi[axis] } else { lo[axis] };
                p[u] = if a { hi[u] } else { lo[u] };
                p[v] = if b { hi[v] } else { lo[v] };
                layer.positions.extend_from_slice(&p);
                let mut n = [0.0f32; 3];
                n[axis] = if high { 1.0 } else { -1.0 };
                layer.normals.extend_from_slice(&n);
            }
            // outward: counter-clockwise seen from outside
            let quad = if high { [0, 1, 2, 0, 2, 3] } else { [0, 2, 1, 0, 3, 2] };
            layer.indices.extend(quad.map(|k| base + k));
        }
    }
}

/// A model of only [`structural_frame`]`(n)`.
pub fn structural(n: usize) -> Model {
    let mut model = Model::new(format!("structural:{n}").into(), "Structural frame (generated)", Vec::new());
    model.set_structural(vec![StructuralPart::new(0, Arc::new(structural_frame(n)))]);
    model
}

/// A 3D grid frame of `n` members (columns 4 m, beams 6 m both ways, up to ten storeys) with
/// their nodes, fixed supports on the ground nodes and a moment hinge at the start of every tenth
/// member.
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
            releases: [(e % 10 == 0).then(|| hinge.clone()), None],
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

/// Copies the render target (RGBA8) to the CPU, row by row.
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

    #[test]
    fn synthetic_has_one_element_per_box() {
        let m = synthetic(25);
        let ids: std::collections::BTreeSet<u64> = m.layers.iter().flat_map(|l| l.elements.iter().map(|e| e.0)).collect();
        assert_eq!(ids, (1..=25).collect());
        assert_eq!(m.triangle_count(), 25 * 12);
        assert_eq!(m.element_props(7).first().map(|(k, v)| (k.as_str(), v.as_str())), Some(("Name", "Box 7")));
        assert_eq!(synthetic(25_000).layers.len(), 3);
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
            assert_eq!(f.members.iter().filter(|m| m.releases[0].is_some()).count(), n.div_ceil(10));
            // no member of zero length
            assert!(f.members.iter().all(|m| f.member_ends(m).is_some_and(|(a, b)| a.distance(b) > 1.0)));
        }
    }

    /// Renders the portal frame fixture offscreen, from the front, with one member selected, and
    /// checks the axes, the nodes and the selection are drawn where they project; writes
    /// `target/structural-renders/portal_frame.png`. `cargo test --release --lib bench -- --ignored`.
    #[test]
    #[ignore = "needs a GPU; writes a PNG file"]
    fn renders_the_portal_frame() {
        use crate::structural::display::Marks;
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
        let primitive = ScenePrimitive {
            model: Some(model.clone()),
            visible: Vec::new(),
            selection: Vec::new(),
            camera,
            settings,
            explode: None,
            offsets: Arc::default(),
            structural_selected: Arc::new(display.marks([member])),
            structural_hovered: Arc::new(Marks::default()),
        };
        let viewport = Viewport::with_physical_size(iced::Size::new(w, h), 1.0);
        let bounds = iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(w as f32, h as f32));
        primitive.prepare(&mut pipeline, &device, &queue, &bounds, &viewport);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        primitive.render(&pipeline, &mut encoder, &view, &iced::Rectangle { x: 0, y: 0, width: w, height: h });
        queue.submit([encoder.finish()]);
        let rgba = read_target(&device, &queue, &target, (w, h)).expect("read back");
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/structural-renders");
        std::fs::create_dir_all(&dir).expect("folder");
        crate::write_png(&dir.join("portal_frame.png"), w, h, &rgba).expect("png");

        let vp = camera.view_proj(w as f32 / h as f32);
        let pixel = |q: [f32; 3]| {
            let c = vp * glam::Vec3::from(q).extend(1.0);
            let (x, y) = (((c.x / c.w + 1.0) * 0.5 * w as f32) as usize, ((1.0 - c.y / c.w) * 0.5 * h as f32) as usize);
            let at = 4 * (y * w as usize + x);
            [rgba[at], rgba[at + 1], rgba[at + 2]]
        };
        let near = |a: [u8; 3], b: [u8; 3], tol: i32| (0..3).all(|k| (a[k] as i32 - b[k] as i32).abs() <= tol);
        let bg = settings.background.map(|v| (v * 255.0).round() as u8);
        assert!(near([rgba[0], rgba[1], rgba[2]], bg, 2), "background in the corner");
        let axis = [28u8, 77, 135];
        let amber = [217u8, 119, 6];
        for (k, item) in display.items.iter().enumerate() {
            match item.kind {
                bim_structural::ItemKind::Member => {
                    let s = display.segs[item.segs.0 as usize];
                    let mid = [0, 1, 2].map(|i| 0.5 * (s.a[i] + s.b[i]));
                    let want = if k as u32 == member { amber } else { axis };
                    assert!(near(pixel(mid), want, 40), "member {k}: {:?}, want {want:?}", pixel(mid));
                }
                _ => {
                    let g = display.glyphs[item.glyphs.0 as usize];
                    assert!(!near(pixel(g.pos), bg, 30), "node {k} drawn: {:?}", pixel(g.pos));
                }
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
