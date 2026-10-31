//! wgpu side of the viewport: the custom primitive drawn by iced's `shader` widget.
//!
//! The scene is drawn into an own 4x multisampled colour + depth target the size of the
//! viewport, resolved, and then blitted into iced's frame at the widget bounds. Model geometry
//! is uploaded once per model (keyed by `Model::id`); each frame only writes the uniforms, and
//! the offsets of the exploded view when they change.

use super::camera::Camera;
use crate::explode::ExplodeIndex;
use crate::model::{Bounds, Model};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use iced::widget::shader::{self, Viewport};
use iced::wgpu;
use iced::wgpu::util::DeviceExt;
use iced::Rectangle;
use crate::section::SectionBox;
use std::sync::Arc;

const SAMPLES: u32 = 4;
/// With stencil, for the cut fill of the section box.
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;
/// Cut fill: a dark neutral grey (as Open Pointcloud Studio), hatched every 9 pixels.
const CAP_COLOR: [f32; 3] = [0.345, 0.345, 0.345];
const CAP_HATCH: f32 = 9.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderMode {
    #[default]
    Solid,
    Wireframe,
    /// Solid with feature edges (creases and open borders) on top.
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
    /// The section box (world), when on.
    pub section: Option<SectionBox>,
    /// The section plane, when placed: `SectionPlane::uniform`.
    pub plane: Option<[[f32; 4]; 2]>,
    /// Fill the cut faces of solids where the box or the plane cuts them.
    pub caps: bool,
    /// Draw the model's grid lines (Revit grids).
    pub grids: bool,
    /// World bounds of the model as drawn (pulled apart by the exploded view), for the ground
    /// grid; `None`: the model's own.
    pub bounds: Option<Bounds>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    color: [u8; 4],
    /// Unit of the exploded view: index into the offsets (0 never moves).
    unit: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
    eye: [f32; 4],
    params: [f32; 4],
    clip: [[f32; 4]; 3],
    cap: [f32; 4],
    plane: [[f32; 4]; 2],
}

/// One frame of the viewport, created by the widget's `draw`.
#[derive(Debug)]
pub struct ScenePrimitive {
    pub model: Option<Arc<Model>>,
    pub visible: Vec<bool>,
    /// Highlighted index ranges: (layer, first index, index count).
    pub selection: Vec<(usize, u32, u32)>,
    pub camera: Camera,
    pub settings: RenderSettings,
    /// The units of the exploded view (`crate::explode`), uploaded with the model.
    pub explode: Option<Arc<ExplodeIndex>>,
    /// The offset (world) of every unit; written when it is another vector.
    pub offsets: Arc<Vec<[f32; 4]>>,
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
    /// Line lists, built on first use: all triangle edges (wireframe) and feature edges.
    wire: Option<(wgpu::Buffer, u32)>,
    features: Option<(wgpu::Buffer, u32)>,
    /// Triangles that enclose volume (cut fill), built on first use.
    solid: Option<(wgpu::Buffer, u32)>,
    /// Some vertices are transparent (glass pass).
    transparent: bool,
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
    highlight: wgpu::RenderPipeline,
    glass: wgpu::RenderPipeline,
    stencil_clear: wgpu::RenderPipeline,
    face_count: wgpu::RenderPipeline,
    cap: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    blit: wgpu::RenderPipeline,
    blit_layout: wgpu::BindGroupLayout,
    blit_uniforms: wgpu::Buffer,
    uniform_layout: wgpu::BindGroupLayout,
    model_uniforms: wgpu::Buffer,
    model_group: wgpu::BindGroup,
    world_uniforms: wgpu::Buffer,
    world_group: wgpu::BindGroup,
    edge_uniforms: wgpu::Buffer,
    edge_group: wgpu::BindGroup,
    /// Exploded view: the offset of every unit of the model (a float texture, bound with the
    /// uniforms), its length in units, and the vector last written into it.
    offsets: wgpu::Texture,
    offsets_len: usize,
    offsets_written: Option<Arc<Vec<[f32; 4]>>>,
    targets: Option<Targets>,
    model_id: u64,
    model: Option<Arc<Model>>,
    layers: Vec<GpuLayer>,
    grid: Option<(wgpu::Buffer, u32, [i64; 6])>,
    /// The model's grid lines (datum grids), dash-dot, built with the model.
    datum: Option<(wgpu::Buffer, u32)>,
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
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // the offsets of the exploded view
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let make_uniforms = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: std::mem::size_of::<Uniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let model_uniforms = make_uniforms("o3v.uniforms.model");
        let world_uniforms = make_uniforms("o3v.uniforms.world");
        let edge_uniforms = make_uniforms("o3v.uniforms.edges");
        let offsets = offsets_texture(device, 1);
        let model_group = uniform_group(device, &uniform_layout, &model_uniforms, &offsets);
        let world_group = uniform_group(device, &uniform_layout, &world_uniforms, &offsets);
        let edge_group = uniform_group(device, &uniform_layout, &edge_uniforms, &offsets);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("o3v.scene.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scene.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("o3v.scene.layout"),
            bind_group_layouts: &[&uniform_layout],
            push_constant_ranges: &[],
        });
        let vertex_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4, 3 => Uint32];
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &vertex_attributes,
        };
        let make_pipeline = |label, topology, fs: &str, blend, bias: wgpu::DepthBiasState, depth_write: bool| {
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
                    depth_write_enabled: depth_write,
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
            true,
        );
        let glass = make_pipeline(
            "o3v.glass",
            wgpu::PrimitiveTopology::TriangleList,
            "fs_glass",
            Some(wgpu::BlendState::ALPHA_BLENDING),
            wgpu::DepthBiasState { constant: 2, slope_scale: 1.0, clamp: 0.0 },
            false,
        );
        // The selection: the same triangles again (equal depth passes), tinted.
        let highlight = make_pipeline(
            "o3v.highlight",
            wgpu::PrimitiveTopology::TriangleList,
            "fs_highlight",
            None,
            wgpu::DepthBiasState { constant: 2, slope_scale: 1.0, clamp: 0.0 },
            true,
        );
        // Cut fill: stencil clear, surface count per box face, the face where the count is odd.
        let stencil = |compare, pass_op| {
            let face = wgpu::StencilFaceState {
                compare,
                fail_op: wgpu::StencilOperation::Keep,
                depth_fail_op: wgpu::StencilOperation::Keep,
                pass_op,
            };
            wgpu::StencilState { front: face, back: face, read_mask: 0xff, write_mask: 0xff }
        };
        let special = |label, vs: &str, fs: &str, with_vertices: bool, depth: (wgpu::CompareFunction, bool), stencil, colour| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    buffers: if with_vertices { std::slice::from_ref(&vertex_layout) } else { &[] },
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: depth.1,
                    depth_compare: depth.0,
                    stencil,
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState { count: SAMPLES, mask: !0, alpha_to_coverage_enabled: false },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: colour })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                multiview: None,
                cache: None,
            })
        };
        let stencil_clear = special(
            "o3v.stencil_clear",
            "vs_full",
            "fs_none",
            false,
            (wgpu::CompareFunction::Always, false),
            stencil(wgpu::CompareFunction::Always, wgpu::StencilOperation::Zero),
            wgpu::ColorWrites::empty(),
        );
        let face_count = special(
            "o3v.face_count",
            "vs_face",
            "fs_face_count",
            true,
            (wgpu::CompareFunction::Always, false),
            stencil(wgpu::CompareFunction::Always, wgpu::StencilOperation::Invert),
            wgpu::ColorWrites::empty(),
        );
        let cap = special(
            "o3v.cap",
            "vs_cap",
            "fs_cap",
            false,
            (wgpu::CompareFunction::LessEqual, true),
            stencil(wgpu::CompareFunction::NotEqual, wgpu::StencilOperation::Keep),
            wgpu::ColorWrites::ALL,
        );
        let lines = make_pipeline(
            "o3v.lines",
            wgpu::PrimitiveTopology::LineList,
            "fs_line",
            Some(wgpu::BlendState::ALPHA_BLENDING),
            wgpu::DepthBiasState::default(),
            true,
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
            highlight,
            glass,
            stencil_clear,
            face_count,
            cap,
            lines,
            blit,
            blit_layout,
            blit_uniforms,
            uniform_layout,
            model_uniforms,
            model_group,
            world_uniforms,
            world_group,
            edge_uniforms,
            edge_group,
            offsets,
            offsets_len: 1,
            offsets_written: None,
            targets: None,
            model_id: 0,
            model: None,
            layers: Vec::new(),
            grid: None,
            datum: None,
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

    /// Makes room for the offsets of `units` units of the exploded view (all zero) and binds it.
    fn resize_offsets(&mut self, device: &wgpu::Device, units: usize) {
        self.offsets = offsets_texture(device, units);
        self.offsets_len = units.max(1);
        self.offsets_written = None;
        self.model_group = uniform_group(device, &self.uniform_layout, &self.model_uniforms, &self.offsets);
        self.world_group = uniform_group(device, &self.uniform_layout, &self.world_uniforms, &self.offsets);
        self.edge_group = uniform_group(device, &self.uniform_layout, &self.edge_uniforms, &self.offsets);
    }

    fn upload_model(&mut self, device: &wgpu::Device, model: Option<&Arc<Model>>, explode: Option<&ExplodeIndex>) {
        let id = model.map_or(0, |m| m.id);
        if id == self.model_id {
            return;
        }
        self.model_id = id;
        self.model = model.cloned();
        self.layers.clear();
        self.datum = None;
        self.resize_offsets(device, explode.map_or(1, ExplodeIndex::len));
        let Some(model) = model else { return };
        let datum = datum_lines(&model.project.grids);
        if !datum.is_empty() {
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("o3v.datum"),
                contents: bytemuck::cast_slice(&datum),
                usage: wgpu::BufferUsages::VERTEX,
            });
            self.datum = Some((buffer, datum.len() as u32));
        }
        for (l, layer) in model.layers.iter().enumerate() {
            let base = layer.color.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
            let base = [base[0], base[1], base[2], 255];
            let units = explode.and_then(|e| e.vertex_units.get(l));
            let vertices: Vec<Vertex> = (0..layer.vertex_count())
                .map(|i| Vertex {
                    position: [layer.positions[3 * i], layer.positions[3 * i + 1], layer.positions[3 * i + 2]],
                    normal: layer
                        .normals
                        .get(3 * i..3 * i + 3)
                        .map(|n| [n[0], n[1], n[2]])
                        .unwrap_or([0.0; 3]),
                    color: layer.colors.as_ref().and_then(|c| c.get(i).copied()).unwrap_or(base),
                    unit: units.and_then(|u| u.get(i).copied()).unwrap_or(0),
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
                wire: None,
                features: None,
                solid: None,
                transparent: layer.colors.as_ref().is_some_and(|c| c.iter().any(|c| c[3] < 253)),
            });
        }
    }

    /// Builds the index buffers of the triangles that enclose volume, on first use (all layers at
    /// once, a thread per layer).
    fn ensure_solids(&mut self, device: &wgpu::Device) {
        let Some(model) = self.model.clone() else { return };
        if self.layers.iter().all(|l| l.solid.is_some()) {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        let solids: Vec<Vec<u32>> = std::thread::scope(|scope| {
            let jobs: Vec<_> = model.layers.iter().map(|l| scope.spawn(move || l.solid_indices())).collect();
            jobs.into_iter().map(|j| j.join().unwrap_or_default()).collect()
        });
        // the browser has no threads
        #[cfg(target_arch = "wasm32")]
        let solids: Vec<Vec<u32>> = model.layers.iter().map(|l| l.solid_indices()).collect();
        for (gpu, indices) in self.layers.iter_mut().zip(solids) {
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("o3v.layer.solid"),
                contents: bytemuck::cast_slice(if indices.is_empty() { &[0u32, 0, 0][..] } else { &indices }),
                usage: wgpu::BufferUsages::INDEX,
            });
            gpu.solid = Some((buffer, indices.len() as u32));
        }
    }

    /// Builds the line index buffers for visible layers on first use.
    fn ensure_edges(&mut self, device: &wgpu::Device, visible: &[bool], all_triangle_edges: bool) {
        let Some(model) = self.model.clone() else { return };
        for (i, (gpu, layer)) in self.layers.iter_mut().zip(&model.layers).enumerate() {
            if !visible.get(i).copied().unwrap_or(true) {
                continue;
            }
            let slot = if all_triangle_edges { &mut gpu.wire } else { &mut gpu.features };
            if slot.is_some() {
                continue;
            }
            let lines = if all_triangle_edges {
                let mut lines = Vec::with_capacity(layer.indices.len() * 2);
                for t in layer.indices.chunks_exact(3) {
                    lines.extend_from_slice(&[t[0], t[1], t[1], t[2], t[2], t[0]]);
                }
                lines
            } else {
                feature_edges(&layer.positions, &layer.indices, 25.0)
            };
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("o3v.layer.edges"),
                contents: bytemuck::cast_slice(&lines),
                usage: wgpu::BufferUsages::INDEX,
            });
            *slot = Some((buffer, lines.len() as u32));
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
            vertices.push(Vertex { position: a, normal: [0.0; 3], color, unit: 0 });
            vertices.push(Vertex { position: b, normal: [0.0; 3], color, unit: 0 });
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

        self.upload_model(device, primitive.model.as_ref(), primitive.explode.as_deref());
        if !self.offsets_written.as_ref().is_some_and(|o| Arc::ptr_eq(o, &primitive.offsets)) {
            let n = primitive.offsets.len().min(self.offsets_len);
            if n > 0 {
                write_offsets(queue, &self.offsets, &primitive.offsets[..n]);
            }
            self.offsets_written = Some(primitive.offsets.clone());
        }
        let settings = &primitive.settings;
        if settings.mode != RenderMode::Solid {
            self.ensure_edges(device, &primitive.visible, settings.mode == RenderMode::Wireframe);
        }
        if (settings.section.is_some() || settings.plane.is_some()) && settings.caps && settings.mode != RenderMode::Wireframe {
            self.ensure_solids(device);
        }
        if settings.grid {
            let world_bounds = settings
                .bounds
                .or_else(|| primitive.model.as_ref().map(|m| transform_bounds(&m.bounds, &settings.model_matrix)))
                .unwrap_or(Bounds { min: [-5.0, -5.0, 0.0], max: [5.0, 5.0, 0.0] });
            self.ensure_grid(device, &world_bounds, settings);
        }

        let camera = &primitive.camera;
        let view_proj = camera.view_proj(w as f32 / h as f32);
        let eye = camera.eye();
        let srgb = if self.srgb { 1.0 } else { 0.0 };
        let clip = settings.section.map(|s| s.uniform()).unwrap_or([[0.0, 0.0, 0.0, 1.0], [0.0; 4], [0.0; 4]]);
        let plane = settings.plane.unwrap_or([[0.0; 4]; 2]);
        let uniforms = |model: Mat4, line_scale: f32, clip: [[f32; 4]; 3], plane: [[f32; 4]; 2]| Uniforms {
            view_proj: view_proj.to_cols_array_2d(),
            model: model.to_cols_array_2d(),
            eye: [eye.x, eye.y, eye.z, 1.0],
            params: [srgb, line_scale, 0.0, 0.0],
            clip,
            cap: [CAP_COLOR[0], CAP_COLOR[1], CAP_COLOR[2], CAP_HATCH * viewport.scale_factor() as f32],
            plane,
        };
        let wire_scale = if settings.mode == RenderMode::SolidEdges { 0.35 } else { 0.8 };
        queue.write_buffer(&self.model_uniforms, 0, bytemuck::bytes_of(&uniforms(settings.model_matrix, 1.0, clip, plane)));
        queue.write_buffer(&self.edge_uniforms, 0, bytemuck::bytes_of(&uniforms(settings.model_matrix, wire_scale, clip, plane)));
        // the grid is never cut
        let no_clip = [[0.0, 0.0, 0.0, 1.0], [0.0; 4], [0.0; 4]];
        queue.write_buffer(&self.world_uniforms, 0, bytemuck::bytes_of(&uniforms(Mat4::IDENTITY, 1.0, no_clip, [[0.0; 4]; 2])));
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
                    stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Discard }),
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
            if !primitive.selection.is_empty() {
                pass.set_pipeline(&self.highlight);
                pass.set_bind_group(0, &self.model_group, &[]);
                for &(i, first, count) in &primitive.selection {
                    let Some(layer) = self.layers.get(i) else { continue };
                    if visible(i) && first + count <= layer.index_count {
                        pass.set_vertex_buffer(0, layer.vertices.slice(..));
                        pass.set_index_buffer(layer.indices.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(first..first + count, 0, 0..1);
                    }
                }
            }
            if primitive.settings.caps && mode != RenderMode::Wireframe {
                pass.set_bind_group(0, &self.model_group, &[]);
                pass.set_stencil_reference(0);
                let mut faces = primitive.settings.section.map(|s| cap_faces(&s, &primitive.camera)).unwrap_or_default();
                if primitive.settings.plane.is_some_and(|p| plane_cap_visible(p[0], &primitive.camera)) {
                    faces.push(6);
                }
                for face in faces {
                    pass.set_pipeline(&self.stencil_clear);
                    pass.draw(0..3, 0..1);
                    pass.set_pipeline(&self.face_count);
                    for (i, layer) in self.layers.iter().enumerate() {
                        if let (true, Some((solid, count))) = (visible(i), &layer.solid) {
                            if *count > 0 {
                                pass.set_vertex_buffer(0, layer.vertices.slice(..));
                                pass.set_index_buffer(solid.slice(..), wgpu::IndexFormat::Uint32);
                                pass.draw_indexed(0..*count, 0, face..face + 1);
                            }
                        }
                    }
                    pass.set_pipeline(&self.cap);
                    pass.draw(0..6, face..face + 1);
                }
            }
            if mode != RenderMode::Wireframe {
                pass.set_pipeline(&self.glass);
                pass.set_bind_group(0, &self.model_group, &[]);
                for (i, layer) in self.layers.iter().enumerate() {
                    if visible(i) && layer.transparent {
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
                    let edges = if mode == RenderMode::Wireframe { &layer.wire } else { &layer.features };
                    if let (true, Some((edges, count))) = (visible(i), edges) {
                        pass.set_vertex_buffer(0, layer.vertices.slice(..));
                        pass.set_index_buffer(edges.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..*count, 0, 0..1);
                    }
                }
            }
            if let (true, Some((datum, count))) = (primitive.settings.grids, &self.datum) {
                pass.set_bind_group(0, &self.world_group, &[]);
                pass.set_vertex_buffer(0, datum.slice(..));
                pass.draw(0..*count, 0..1);
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

/// Units per row of the offsets texture; `OFFSETS_WIDTH` in scene.wgsl. With WebGL2's 2048 rows
/// that leaves room for two million units.
const OFFSETS_WIDTH: usize = 1024;

/// The offsets of the exploded view for `units` units (at least one), zero: a float texture
/// rather than a storage buffer, so the scene also draws through WebGL2.
fn offsets_texture(device: &wgpu::Device, units: usize) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("o3v.explode.offsets"),
        size: wgpu::Extent3d { width: OFFSETS_WIDTH as u32, height: units.max(1).div_ceil(OFFSETS_WIDTH) as u32, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// Writes `offsets` into the texture from unit 0, the last row padded with zeros.
fn write_offsets(queue: &wgpu::Queue, texture: &wgpu::Texture, offsets: &[[f32; 4]]) {
    let rows = offsets.len().div_ceil(OFFSETS_WIDTH);
    let mut data = offsets.to_vec();
    data.resize(rows * OFFSETS_WIDTH, [0.0; 4]);
    queue.write_texture(
        texture.as_image_copy(),
        bytemuck::cast_slice(&data),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some((OFFSETS_WIDTH * std::mem::size_of::<[f32; 4]>()) as u32),
            rows_per_image: Some(rows as u32),
        },
        wgpu::Extent3d { width: OFFSETS_WIDTH as u32, height: rows as u32, depth_or_array_layers: 1 },
    );
}

/// A bind group of the scene shader: its uniforms and the offsets of the exploded view.
fn uniform_group(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, uniforms: &wgpu::Buffer, offsets: &wgpu::Texture) -> wgpu::BindGroup {
    let offsets = offsets.create_view(&wgpu::TextureViewDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("o3v.uniforms"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&offsets) },
        ],
    })
}

/// Grid lines as a dash-dot line list (Revit's grid line pattern: long dash, gap, dot, gap) in
/// a neutral grey that reads on light and dark backgrounds.
fn datum_lines(grids: &[crate::model::GridLine]) -> Vec<Vertex> {
    const PATTERN: [(f32, bool); 4] = [(1.6, true), (0.3, false), (0.3, true), (0.3, false)];
    let color = [120, 122, 132, 230];
    let vertex = |p: Vec3| Vertex { position: p.to_array(), normal: [0.0, 0.0, 1.0], color, unit: 0 };
    let mut out = Vec::new();
    for g in grids {
        let (a, b) = (Vec3::from_array(g.a), Vec3::from_array(g.b));
        let len = (b - a).length();
        if len < 1e-3 {
            continue;
        }
        let dir = (b - a) / len;
        let (mut t, mut k) = (0.0f32, 0usize);
        while t < len {
            let (step, draw) = PATTERN[k % PATTERN.len()];
            let end = (t + step).min(len);
            if draw {
                out.push(vertex(a + dir * t));
                out.push(vertex(a + dir * end));
            }
            t = end;
            k += 1;
        }
    }
    out
}

/// The box faces (axis * 2 + 1 for the max face) whose cut can be seen: the camera is on the
/// side the box cuts away (perspective), or looks into the face (orthographic).
fn cap_faces(section: &SectionBox, camera: &Camera) -> Vec<u32> {
    let eye = section.to_local(camera.eye());
    let dir = section.to_local(section.pivot + (camera.target - camera.eye()).normalize_or_zero()) - section.pivot;
    (0..6u32)
        .filter(|&face| {
            let axis = (face / 2) as usize;
            let is_min = face % 2 == 0;
            match (camera.ortho, is_min) {
                (true, true) => dir[axis] > 0.0,
                (true, false) => dir[axis] < 0.0,
                (false, true) => eye[axis] < section.min[axis],
                (false, false) => eye[axis] > section.max[axis],
            }
        })
        .collect()
}

/// Whether the cut of the section plane (normal + offset) can be seen: the camera is on the side
/// it cuts away (perspective), or looks against its normal (orthographic).
fn plane_cap_visible(plane: [f32; 4], camera: &Camera) -> bool {
    let n = Vec3::new(plane[0], plane[1], plane[2]);
    if camera.ortho {
        n.dot(camera.target - camera.eye()) < 0.0
    } else {
        n.dot(camera.eye()) > plane[3]
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

/// Edges where the faces meet at more than `angle_deg`, plus open borders, as a line index
/// list. Vertices are welded by exact position, so flat-shaded meshes work.
pub fn feature_edges(positions: &[f32], indices: &[u32], angle_deg: f32) -> Vec<u32> {
    use std::collections::HashMap;
    let vertex_count = positions.len() / 3;
    let at = |i: u32| {
        let i = i as usize * 3;
        Vec3::new(positions[i], positions[i + 1], positions[i + 2])
    };
    // Weld: position bits -> canonical id
    let mut weld: HashMap<[u32; 3], u32> = HashMap::with_capacity(vertex_count);
    let canon: Vec<u32> = (0..vertex_count)
        .map(|i| {
            let key = [positions[3 * i].to_bits(), positions[3 * i + 1].to_bits(), positions[3 * i + 2].to_bits()];
            let next = weld.len() as u32;
            *weld.entry(key).or_insert(next)
        })
        .collect();
    // Edge (welded) -> first face normal, original endpoints, face count, crease flag
    struct Edge {
        normal: Vec3,
        a: u32,
        b: u32,
        faces: u32,
        crease: bool,
    }
    let cos_limit = angle_deg.to_radians().cos();
    let mut edges: HashMap<(u32, u32), Edge> = HashMap::with_capacity(indices.len());
    for t in indices.chunks_exact(3) {
        if t.iter().any(|&i| i as usize >= vertex_count) {
            continue;
        }
        let n = (at(t[1]) - at(t[0])).cross(at(t[2]) - at(t[0]));
        if n.length_squared() == 0.0 {
            continue; // degenerate
        }
        let n = n.normalize();
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            let (ca, cb) = (canon[a as usize], canon[b as usize]);
            if ca == cb {
                continue;
            }
            let key = (ca.min(cb), ca.max(cb));
            edges
                .entry(key)
                .and_modify(|e| {
                    e.faces += 1;
                    // Two-sided: opposite windings count as the same plane
                    if e.normal.dot(n).abs() < cos_limit {
                        e.crease = true;
                    }
                })
                .or_insert(Edge { normal: n, a, b, faces: 1, crease: false });
        }
    }
    let mut lines = Vec::new();
    for e in edges.values() {
        if e.crease || e.faces != 2 {
            lines.extend_from_slice(&[e.a, e.b]);
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quad_diagonal_is_not_a_feature_edge() {
        // Two coplanar triangles sharing the diagonal 0-2
        let p = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0];
        let lines = feature_edges(&p, &[0, 1, 2, 0, 2, 3], 25.0);
        assert_eq!(lines.len(), 8, "4 border edges, no diagonal");
    }

    #[test]
    fn grid_layout_spacing_is_nice() {
        let (_, _, spacing, _) = grid_layout(&Bounds { min: [0.0, 0.0, 0.0], max: [47.0, 40.0, 18.0] });
        assert_eq!(spacing, 5.0);
    }
}
