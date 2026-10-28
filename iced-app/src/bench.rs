//! `--bench FILE`: loads a model and renders frames of the viewport pipeline offscreen
//! (1920x1080, 4x MSAA, orbiting camera), reporting load time and frame times; the last pass
//! cuts the model with a section box (lower 60 %, filled cuts). With `O3V_BENCH_PNG=path` the
//! first section frame is also written as a PNG.

use crate::loaders;
use crate::theme::{srgb3, srgb4, ThemeKind};
use crate::viewport::camera::{Camera, ViewPreset};
use crate::viewport::render::{RenderMode, RenderSettings, ScenePipeline, ScenePrimitive};
use iced::futures::executor::block_on;
use iced::wgpu;
use iced::widget::shader::{Primitive, Viewport};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

pub fn run(file: &Path, frames: u32) -> Result<(), String> {
    let model = Arc::new(loaders::load(file)?);
    println!(
        "{}: {} layers, {} vertices, {} triangles, loaded in {:.2} s",
        model.file_name(),
        model.layers.len(),
        model.vertex_count(),
        model.triangle_count(),
        model.load_time.as_secs_f32()
    );

    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
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
        };
        let frame = |camera: Camera, pipeline: &mut ScenePipeline| {
            let primitive = ScenePrimitive {
                model: Some(model.clone()),
                visible: vec![true; model.layers.len()],
                selection: Vec::new(),
                camera,
                settings,
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
    crate::write_png(path, w, h, &rgba)?;
    println!("wrote {}", path.display());
    Ok(())
}
