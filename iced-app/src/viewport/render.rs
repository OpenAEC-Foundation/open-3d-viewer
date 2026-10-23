//! wgpu side of the viewport: the custom primitive drawn by iced's `shader` widget.
//!
//! The scene is drawn into an own 4x multisampled colour + depth target the size of the
//! viewport, resolved, and then blitted into iced's frame at the widget bounds. Model geometry
//! is uploaded once per model (keyed by `Model::id`); each frame only writes the uniforms.

use super::camera::Camera;
use crate::model::{Bounds, Model};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use iced::widget::shader::{self, Viewport};
use iced::wgpu;
use iced::wgpu::util::DeviceExt;
use iced::Rectangle;
use std::sync::Arc;

const SAMPLES: u32 = 4;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderMode {
    #[default]
    Solid,
    Wireframe,
    /// Solid with all triangle edges on top.
    SolidEdges,
}

/// Colours and switches that come from the UI.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderSettings {
    pub mode: RenderMode,
    pub grid: bool,
    /// sRGB 0..1
    pub background: [f32; 3],
    pub grid_minor: [f32; 4],
    pub grid_major: [f32; 4],
    /// Extra model rotation (up-axis override).
    pub model_matrix: Mat4,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    color: [u8; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
    eye: [f32; 4],
    params: [f32; 4],
}

/// One frame of the viewport, created by the widget's `draw`.
#[derive(Debug)]
pub struct ScenePrimitive {
    pub model: Option<Arc<Model>>,
    pub visible: Vec<bool>,
    pub camera: Camera,
    pub settings: RenderSettings,
}

impl shader::Primitive for ScenePrimitive {
    type Pipeline = ScenePipeline;

    fn prepare(
        &self,
        pipeline: &mut ScenePipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        pipeline.prepare(self, device, queue, bounds, viewport);
    }

    fn render(
        &self,
        pipeline: &ScenePipeline,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        pipeline.render(self, encoder, target, clip_bounds);
    }
}

struct GpuLayer {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    /// Line list over all triangle edges, built on first use.
    edges: Option<(wgpu::Buffer, u32)>,
    /// CPU copy of the triangle indices, to build `edges` lazily.
    tri_indices: Vec<u32>,
}

struct Targets {
    size: (u32, u32),
    msaa: wgpu::TextureView,
    resolve: wgpu::TextureView,
    depth: wgpu::TextureView,
    blit_group: wgpu::BindGroup,
}

pub struct ScenePipeline {
    format: wgpu::TextureFormat,
    srgb: bool,
    lit: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    blit: wgpu::RenderPipeline,
    blit_layout: wgpu::BindGroupLayout,
    blit_uniforms: wgpu::Buffer,
    model_uniforms: wgpu::Buffer,
    model_group: wgpu::BindGroup,
    world_uniforms: wgpu::Buffer,
    world_group: wgpu::BindGroup,
    edge_uniforms: wgpu::Buffer,
    edge_group: wgpu::BindGroup,
    targets: Option<Targets>,
    model_id: u64,
    layers: Vec<GpuLayer>,
    grid: Option<(wgpu::Buffer, u32, [i64; 6])>,
    /// Viewport rectangle in target pixels, from the last `prepare`.
    rect: (u32, u32, u32, u32),
}

impl shader::Pipeline for ScenePipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self::new(device, format)
    }
}

fn to_linear(c: f32) -> f64 {
    let c = c as f64;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

impl ScenePipeline {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("o3v.uniforms.layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let make_uniforms = |label| {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: std::mem::size_of::<Uniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &uniform_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
            });
            (buffer, group)
        };
        let (model_uniforms, model_group) = make_uniforms("o3v.uniforms.model");
        let (world_uniforms, world_group) = make_uniforms("o3v.uniforms.world");
        let (edge_uniforms, edge_group) = make_uniforms("o3v.uniforms.edges");

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("o3v.scene.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scene.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("o3v.scene.layout"),
            bind_group_layouts: &[&uniform_layout],
            push_constant_ranges: &[],
        });
        let vertex_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4];
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &vertex_attributes,
        };
        let make_pipeline = |label, topology, fs: &str, blend, bias: wgpu::DepthBiasState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[vertex_layout.clone()],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                primitive: wgpu::PrimitiveState { topology, cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: wgpu::StencilState::default(),
                    bias,
                }),
                multisample: wgpu::MultisampleState { count: SAMPLES, mask: !0, alpha_to_coverage_enabled: false },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                multiview: None,
                cache: None,
            })
        };
        // Triangles are pushed back a little so edges drawn on top win the depth test.
        let lit = make_pipeline(
            "o3v.lit",
            wgpu::PrimitiveTopology::TriangleList,
            "fs_lit",
            None,
            wgpu::DepthBiasState { constant: 2, slope_scale: 1.0, clamp: 0.0 },
        );
        let lines = make_pipeline(
            "o3v.lines",
            wgpu::PrimitiveTopology::LineList,
            "fs_line",
            Some(wgpu::BlendState::ALPHA_BLENDING),
            wgpu::DepthBiasState::default(),
        );

        // Blit
        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("o3v.blit.layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("o3v.blit.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("blit.wgsl").into()),
        });
        let blit_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("o3v.blit.pipeline_layout"),
            bind_group_layouts: &[&blit_layout],
            push_constant_ranges: &[],
        });
        let blit = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("o3v.blit"),
            layout: Some(&blit_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &blit_shader,
                entry_point: Some("vs_blit"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &blit_shader,
                entry_point: Some("fs_blit"),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview: None,
            cache: None,
        });
        let blit_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("o3v.blit.uniforms"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            format,
            srgb: format.is_srgb(),
            lit,
            lines,
            blit,
            blit_layout,
            blit_uniforms,
            model_uniforms,
            model_group,
            world_uniforms,
            world_group,
            edge_uniforms,
            edge_group,
            targets: None,
            model_id: 0,
            layers: Vec::new(),
            grid: None,
            rect: (0, 0, 1, 1),
        }
    }

    fn ensure_targets(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let texture = |label, format, samples, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let msaa = texture("o3v.msaa", self.format, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let resolve = texture(
            "o3v.resolve",
            self.format,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let depth = texture("o3v.depth", DEPTH_FORMAT, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let blit_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("o3v.blit.group"),
            layout: &self.blit_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&resolve) },
                wgpu::BindGroupEntry { binding: 1, resource: self.blit_uniforms.as_entire_binding() },
            ],
        });
        self.targets = Some(Targets { size, msaa, resolve, depth, blit_group });
    }

    fn upload_model(&mut self, device: &wgpu::Device, model: Option<&Model>) {
        let id = model.map_or(0, |m| m.id);
        if id == self.model_id {
            return;
        }
        self.model_id = id;
        self.layers.clear();
        let Some(model) = model else { return };
        for layer in &model.layers {
            let base = layer.color.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
            let base = [base[0], base[1], base[2], 255];
            let vertices: Vec<Vertex> = (0..layer.vertex_count())
                .map(|i| Vertex {
                    position: [layer.positions[3 * i], layer.positions[3 * i + 1], layer.positions[3 * i + 2]],
                    normal: layer
                        .normals
                        .get(3 * i..3 * i + 3)
                        .map(|n| [n[0], n[1], n[2]])
                        .unwrap_or([0.0; 3]),
                    color: layer.colors.as_ref().and_then(|c| c.get(i).copied()).unwrap_or(base),
                })
                .collect();
            let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("o3v.layer.vertices"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("o3v.layer.indices"),
                contents: bytemuck::cast_slice(&layer.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            self.layers.push(GpuLayer {
                vertices,
                indices,
                index_count: layer.indices.len() as u32,
                edges: None,
                tri_indices: layer.indices.clone(),
            });
        }
    }

    fn ensure_edges(&mut self, device: &wgpu::Device, visible: &[bool]) {
        for (i, layer) in self.layers.iter_mut().enumerate() {
            if layer.edges.is_some() || !visible.get(i).copied().unwrap_or(true) {
                continue;
            }
            let mut lines = Vec::with_capacity(layer.tri_indices.len() * 2);
            for t in layer.tri_indices.chunks_exact(3) {
                lines.extend_from_slice(&[t[0], t[1], t[1], t[2], t[2], t[0]]);
            }
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("o3v.layer.edges"),
                contents: bytemuck::cast_slice(&lines),
                usage: wgpu::BufferUsages::INDEX,
            });
            layer.edges = Some((buffer, lines.len() as u32));
        }
    }

    fn ensure_grid(&mut self, device: &wgpu::Device, bounds: &Bounds, settings: &RenderSettings) {
        let (center, extent, spacing, z) = grid_layout(bounds);
        let colour_key = |c: [f32; 4]| c.iter().fold(0i64, |k, v| k * 1000 + (v * 999.0) as i64);
        let key = [
            (center[0] / spacing).round() as i64,
            (center[1] / spacing).round() as i64,
            (extent / spacing).round() as i64,
            (spacing * 1e6) as i64 ^ ((z * 1000.0) as i64),
            colour_key(settings.grid_minor),
            colour_key(settings.grid_major),
        ];
        if self.grid.as_ref().is_some_and(|g| g.2 == key) {
            return;
        }
        let to8 = |c: [f32; 4]| c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
        let minor = to8(settings.grid_minor);
        let major = to8(settings.grid_major);
        let x_axis = [217, 70, 60, 255];
        let y_axis = [80, 170, 80, 255];
        let n = (extent / spacing).round() as i64;
        let cx = (center[0] / spacing).round() as i64;
        let cy = (center[1] / spacing).round() as i64;
        let mut vertices = Vec::new();
        let mut push = |a: [f32; 3], b: [f32; 3], color: [u8; 4]| {
            vertices.push(Vertex { position: a, normal: [0.0; 3], color });
            vertices.push(Vertex { position: b, normal: [0.0; 3], color });
        };
        let (x0, x1) = ((cx - n) as f32 * spacing, (cx + n) as f32 * spacing);
        let (y0, y1) = ((cy - n) as f32 * spacing, (cy + n) as f32 * spacing);
        for i in -n..=n {
            let gx = cx + i;
            let gy = cy + i;
            let colx = if gx == 0 { y_axis } else if gx % 10 == 0 { major } else { minor };
            let coly = if gy == 0 { x_axis } else if gy % 10 == 0 { major } else { minor };
            let x = gx as f32 * spacing;
            let y = gy as f32 * spacing;
            push([x, y0, z], [x, y1, z], colx);
            push([x0, y, z], [x1, y, z], coly);
        }
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("o3v.grid"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        self.grid = Some((buffer, vertices.len() as u32, key));
    }

    fn prepare(
        &mut self,
        primitive: &ScenePrimitive,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        let scale = viewport.scale_factor();
        let target = viewport.physical_size();
        let x = ((bounds.x * scale).round().max(0.0) as u32).min(target.width.saturating_sub(1));
        let y = ((bounds.y * scale).round().max(0.0) as u32).min(target.height.saturating_sub(1));
        let w = ((bounds.width * scale).round() as u32).clamp(1, target.width - x);
        let h = ((bounds.height * scale).round() as u32).clamp(1, target.height - y);
        self.rect = (x, y, w, h);
        self.ensure_targets(device, (w, h));
        queue.write_buffer(&self.blit_uniforms, 0, bytemuck::cast_slice(&[x as f32, y as f32, 0.0, 0.0]));

        self.upload_model(device, primitive.model.as_deref());
        let settings = &primitive.settings;
        if settings.mode != RenderMode::Solid {
            self.ensure_edges(device, &primitive.visible);
        }
        if settings.grid {
            let world_bounds = primitive
                .model
                .as_ref()
                .map(|m| transform_bounds(&m.bounds, &settings.model_matrix))
                .unwrap_or(Bounds { min: [-5.0, -5.0, 0.0], max: [5.0, 5.0, 0.0] });
            self.ensure_grid(device, &world_bounds, settings);
        }

        let camera = &primitive.camera;
        let view_proj = camera.view_proj(w as f32 / h as f32);
        let eye = camera.eye();
        let srgb = if self.srgb { 1.0 } else { 0.0 };
        let uniforms = |model: Mat4, line_scale: f32| Uniforms {
            view_proj: view_proj.to_cols_array_2d(),
            model: model.to_cols_array_2d(),
            eye: [eye.x, eye.y, eye.z, 1.0],
            params: [srgb, line_scale, 0.0, 0.0],
        };
        let wire_scale = if settings.mode == RenderMode::SolidEdges { 0.35 } else { 0.8 };
        queue.write_buffer(&self.model_uniforms, 0, bytemuck::bytes_of(&uniforms(settings.model_matrix, 1.0)));
        queue.write_buffer(&self.edge_uniforms, 0, bytemuck::bytes_of(&uniforms(settings.model_matrix, wire_scale)));
        queue.write_buffer(&self.world_uniforms, 0, bytemuck::bytes_of(&uniforms(Mat4::IDENTITY, 1.0)));
    }

    fn render(
        &self,
        primitive: &ScenePrimitive,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        clip_bounds: &Rectangle<u32>,
    ) {
        let Some(targets) = &self.targets else { return };
        let bg = primitive.settings.background;
        let clear = if self.srgb {
            wgpu::Color { r: to_linear(bg[0]), g: to_linear(bg[1]), b: to_linear(bg[2]), a: 1.0 }
        } else {
            wgpu::Color { r: bg[0] as f64, g: bg[1] as f64, b: bg[2] as f64, a: 1.0 }
        };
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("o3v.scene.pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.msaa,
                    depth_slice: None,
                    resolve_target: Some(&targets.resolve),
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Discard },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            let mode = primitive.settings.mode;
            let visible = |i: usize| primitive.visible.get(i).copied().unwrap_or(true);

            if mode != RenderMode::Wireframe {
                pass.set_pipeline(&self.lit);
                pass.set_bind_group(0, &self.model_group, &[]);
                for (i, layer) in self.layers.iter().enumerate() {
                    if visible(i) {
                        pass.set_vertex_buffer(0, layer.vertices.slice(..));
                        pass.set_index_buffer(layer.indices.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..layer.index_count, 0, 0..1);
                    }
                }
            }
            pass.set_pipeline(&self.lines);
            if mode != RenderMode::Solid {
                pass.set_bind_group(0, &self.edge_group, &[]);
                for (i, layer) in self.layers.iter().enumerate() {
                    if let (true, Some((edges, count))) = (visible(i), &layer.edges) {
                        pass.set_vertex_buffer(0, layer.vertices.slice(..));
                        pass.set_index_buffer(edges.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..*count, 0, 0..1);
                    }
                }
            }
            if primitive.settings.grid {
                if let Some((grid, count, _)) = &self.grid {
                    pass.set_bind_group(0, &self.world_group, &[]);
                    pass.set_vertex_buffer(0, grid.slice(..));
                    pass.draw(0..*count, 0..1);
                }
            }
        }

        // Copy into iced's frame
        let (x, y, w, h) = self.rect;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("o3v.blit.pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_viewport(x as f32, y as f32, w as f32, h as f32, 0.0, 1.0);
        pass.set_scissor_rect(clip_bounds.x, clip_bounds.y, clip_bounds.width, clip_bounds.height);
        pass.set_pipeline(&self.blit);
        pass.set_bind_group(0, &targets.blit_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// Grid placement for world bounds: (centre, half extent, spacing, z).
fn grid_layout(bounds: &Bounds) -> ([f32; 2], f32, f32, f32) {
    let size = bounds.size();
    let span = size[0].max(size[1]).max(1e-3);
    // ~20-50 cells across the model, on a 1-2-5 sequence
    let raw = span / 20.0;
    let mag = 10f32.powf(raw.log10().floor());
    let spacing = [1.0, 2.0, 5.0, 10.0].into_iter().map(|m| m * mag).find(|s| *s >= raw).unwrap_or(10.0 * mag);
    let extent = (span * 0.5 * 1.6 / spacing).ceil() * spacing;
    let c = bounds.center();
    let z = bounds.min[2] - span * 1e-4;
    ([c[0], c[1]], extent, spacing, z)
}

/// Axis-aligned bounds of transformed bounds.
pub fn transform_bounds(b: &Bounds, m: &Mat4) -> Bounds {
    if !b.is_valid() {
        return *b;
    }
    let mut out = Bounds::empty();
    for i in 0..8 {
        let p = Vec3::new(
            if i & 1 == 0 { b.min[0] } else { b.max[0] },
            if i & 2 == 0 { b.min[1] } else { b.max[1] },
            if i & 4 == 0 { b.min[2] } else { b.max[2] },
        );
        out.extend(m.transform_point3(p).to_array());
    }
    out
}
