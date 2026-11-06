//! The structural pipelines of the viewport (`structural.wgsl`): wide segments for member axes
//! and screen-size glyphs for nodes, drawn in the scene's multisampled pass.
//!
//! Both bind the scene's model uniforms at `@group(0)` (model matrix, section box and plane, the
//! offsets of the exploded view), so the up-axis override, the section and the exploded view apply
//! to every instance; the structural state is the separate [`StructUniforms`] at `@group(1)`, so the
//! scene's uniforms and its hot pipelines do not change. Each pipeline comes three times: plain,
//! selected and hovered; the selection and the hover draw their instance ranges again on top, as
//! the mesh highlight does with index ranges.
//!
//! WebGL2 has no `base_vertex` and no first instance: every glyph kind has its own slice of the
//! library's vertex and index buffers, and instance ranges are bound with `set_vertex_buffer` at an
//! offset.

use super::render::{DEPTH_FORMAT, SAMPLES};
use crate::structural::display::{GlyphInstance, GlyphKind, Marks, SegInstance, StructuralDisplay};
use crate::structural::pick::Lod;
use bytemuck::{Pod, Zeroable};
use iced::wgpu;
use iced::wgpu::util::DeviceExt;

/// Line widths in logical pixels: member axes, and selected axes.
pub const LINE_PX: f32 = 2.0;
pub const SELECTED_LINE_PX: f32 = 4.0;

/// The structural uniforms (`StructUniforms` in structural.wgsl). Every size is in logical
/// pixels, the viewport too.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct StructUniforms {
    /// width, height, 1 / width, 1 / height.
    pub viewport: [f32; 4],
    /// line, glyph, hinge setback, selected line.
    pub sizes: [f32; 4],
    /// diagram scale, load scale, deformation factor, the projection's `[1][1]`.
    pub scales: [f32; 4],
    /// min glyph, arrow spacing, node cluster (pixels), diagram side (+1 / -1).
    pub lod: [f32; 4],
    /// axis, node, support, hinge, load, diagram +, diagram -, deformed (sRGB); the last one's
    /// rgb is also the hover tint.
    pub colors: [[f32; 4]; 8],
}

impl StructUniforms {
    /// For a viewport of `size` logical pixels, projection `[1][1]` `proj11`, on `background`
    /// (sRGB): dark lines and nodes on a light background, light ones on a dark background.
    pub fn new(size: (f32, f32), proj11: f32, background: [f32; 3], lod: &Lod) -> Self {
        let (w, h) = (size.0.max(1.0), size.1.max(1.0));
        let light = 0.2126 * background[0] + 0.7152 * background[1] + 0.0722 * background[2] > 0.5;
        let (axis, node, hover) = if light {
            ([0.11, 0.30, 0.53, 1.0], [0.10, 0.10, 0.12, 1.0], [0.95, 0.62, 0.18, 1.0])
        } else {
            ([0.55, 0.76, 0.96, 1.0], [0.93, 0.93, 0.95, 1.0], [1.0, 0.75, 0.35, 1.0])
        };
        let mut colors = [[0.5, 0.5, 0.5, 1.0]; 8];
        colors[0] = axis;
        colors[1] = node;
        colors[7] = hover;
        Self {
            viewport: [w, h, 1.0 / w, 1.0 / h],
            sizes: [LINE_PX, lod.glyph_px, 10.0, SELECTED_LINE_PX],
            scales: [1.0, 1.0, 1.0, proj11],
            lod: [lod.min_glyph_px, 18.0, lod.node_cluster_px, 1.0],
            colors,
        }
    }
}

/// A vertex of a glyph mesh.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct GlyphVertex {
    position: [f32; 3],
    normal: [f32; 3],
}

/// A unit cube round the origin (edge 1), flat-shaded: the node glyph.
fn cube() -> (Vec<GlyphVertex>, Vec<u32>) {
    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for axis in 0..3 {
        for sign in [-1.0f32, 1.0] {
            let mut n = [0.0; 3];
            n[axis] = sign;
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            let base = vertices.len() as u32;
            for (a, b) in [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)] {
                let mut p = [0.0; 3];
                p[axis] = 0.5 * sign;
                p[u] = a;
                p[v] = b;
                vertices.push(GlyphVertex { position: p, normal: n });
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    (vertices, indices)
}

/// The glyph meshes, one slice of the vertex and index buffers per [`GlyphKind`].
struct GlyphLibrary {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    /// Per kind: vertex byte range, index byte range, index count.
    kinds: Vec<(std::ops::Range<u64>, std::ops::Range<u64>, u32)>,
}

impl GlyphLibrary {
    fn new(device: &wgpu::Device) -> Self {
        let meshes: Vec<(Vec<GlyphVertex>, Vec<u32>)> = GlyphKind::ALL
            .iter()
            .map(|kind| match kind {
                GlyphKind::Node => cube(),
            })
            .collect();
        let (mut vertices, mut indices, mut kinds) = (Vec::new(), Vec::new(), Vec::new());
        let vsize = std::mem::size_of::<GlyphVertex>() as u64;
        for (v, i) in meshes {
            let (v0, i0) = (vertices.len() as u64 * vsize, indices.len() as u64 * 4);
            vertices.extend_from_slice(&v);
            // indices stay relative to the kind's own vertices: no base vertex
            indices.extend_from_slice(&i);
            kinds.push((v0..vertices.len() as u64 * vsize, i0..indices.len() as u64 * 4, i.len() as u32));
        }
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("o3v.structural.glyph_meshes"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("o3v.structural.glyph_indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        Self { vertices, indices, kinds }
    }
}

/// The instances of a model's [`StructuralDisplay`] on the GPU, with the units of the exploded
/// view written in.
pub struct GpuStructural {
    segs: Option<(wgpu::Buffer, u32)>,
    glyphs: Option<wgpu::Buffer>,
    /// (kind, first glyph, count), as in the display.
    kinds: Vec<(GlyphKind, u32, u32)>,
}

impl GpuStructural {
    /// Uploads `display`, its items in units `units` (one per display item; empty: unit 0).
    pub fn new(device: &wgpu::Device, display: &StructuralDisplay, units: &[u32]) -> Option<Self> {
        if display.is_empty() {
            return None;
        }
        let unit = |item: u32| units.get(item as usize).copied().unwrap_or(0);
        let segs = (!display.segs.is_empty()).then(|| {
            let data: Vec<SegInstance> =
                display.segs.iter().zip(&display.seg_item).map(|(s, &item)| SegInstance { unit: unit(item), ..*s }).collect();
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("o3v.structural.segs"),
                contents: bytemuck::cast_slice(&data),
                usage: wgpu::BufferUsages::VERTEX,
            });
            (buffer, data.len() as u32)
        });
        let glyphs = (!display.glyphs.is_empty()).then(|| {
            let data: Vec<GlyphInstance> =
                display.glyphs.iter().zip(&display.glyph_item).map(|(g, &item)| GlyphInstance { unit: unit(item), ..*g }).collect();
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("o3v.structural.glyphs"),
                contents: bytemuck::cast_slice(&data),
                usage: wgpu::BufferUsages::VERTEX,
            })
        });
        Some(Self { segs, glyphs, kinds: display.glyph_kinds.clone() })
    }
}

/// How instances are drawn: plain, or again on top as the selection or the hover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    Plain = 0,
    Selected = 1,
    Hovered = 2,
}

pub struct StructuralPipelines {
    uniforms: wgpu::Buffer,
    group: wgpu::BindGroup,
    /// Per [`Mark`].
    seg: [wgpu::RenderPipeline; 3],
    glyph: [wgpu::RenderPipeline; 3],
    library: GlyphLibrary,
}

const SEG_STRIDE: u64 = std::mem::size_of::<SegInstance>() as u64;
const GLYPH_STRIDE: u64 = std::mem::size_of::<GlyphInstance>() as u64;

impl StructuralPipelines {
    /// The pipelines for colour target `format`; `scene_layout` is the scene's `@group(0)`.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, scene_layout: &wgpu::BindGroupLayout) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("o3v.structural.layout"),
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
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("o3v.structural.uniforms"),
            size: std::mem::size_of::<StructUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("o3v.structural.group"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }],
        });
        let source = format!("{}\n{}", include_str!("scene.wgsl"), include_str!("structural.wgsl"));
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("o3v.structural.shader"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("o3v.structural.pipeline_layout"),
            bind_group_layouts: &[scene_layout, &layout],
            push_constant_ranges: &[],
        });
        let seg_attributes = wgpu::vertex_attr_array![
            0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 3 => Float32x3, 4 => Unorm8x4, 5 => Uint32, 6 => Uint32
        ];
        let seg_layout = [wgpu::VertexBufferLayout {
            array_stride: SEG_STRIDE,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &seg_attributes,
        }];
        let mesh_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
        let glyph_attributes = wgpu::vertex_attr_array![
            2 => Float32x3, 3 => Float32x4, 4 => Unorm8x4, 5 => Uint32, 6 => Float32x2, 7 => Float32x2, 8 => Uint32
        ];
        let glyph_layout = [
            wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<GlyphVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &mesh_attributes,
            },
            wgpu::VertexBufferLayout { array_stride: GLYPH_STRIDE, step_mode: wgpu::VertexStepMode::Instance, attributes: &glyph_attributes },
        ];
        let make = |label: &str, vs: &str, fs: &str, buffers: &[wgpu::VertexBufferLayout], topology| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    buffers,
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                primitive: wgpu::PrimitiveState { topology, cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState { count: SAMPLES, mask: !0, alpha_to_coverage_enabled: false },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                multiview: None,
                cache: None,
            })
        };
        let strip = wgpu::PrimitiveTopology::TriangleStrip;
        let list = wgpu::PrimitiveTopology::TriangleList;
        let seg = ["vs_seg", "vs_seg_selected", "vs_seg_hovered"].map(|vs| make("o3v.structural.seg", vs, "fs_seg", &seg_layout, strip));
        let glyph =
            ["vs_glyph", "vs_glyph_selected", "vs_glyph_hovered"].map(|vs| make("o3v.structural.glyph", vs, "fs_glyph", &glyph_layout, list));
        Self { uniforms, group, seg, glyph, library: GlyphLibrary::new(device) }
    }

    pub fn write(&self, queue: &wgpu::Queue, uniforms: &StructUniforms) {
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(uniforms));
    }

    /// Draws the instances, then the selection and the hover on top. The scene's model group must
    /// be bound at 0; the structural group is bound here at 1.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, gpu: &GpuStructural, selected: &Marks, hovered: &Marks) {
        pass.set_bind_group(1, &self.group, &[]);
        if let Some((segs, count)) = &gpu.segs {
            pass.set_pipeline(&self.seg[Mark::Plain as usize]);
            pass.set_vertex_buffer(0, segs.slice(..));
            pass.draw(0..4, 0..*count);
        }
        if let Some(glyphs) = &gpu.glyphs {
            pass.set_pipeline(&self.glyph[Mark::Plain as usize]);
            for &(kind, first, count) in &gpu.kinds {
                self.draw_glyphs(pass, glyphs, kind, first, count);
            }
        }
        for (mark, marks) in [(Mark::Selected, selected), (Mark::Hovered, hovered)] {
            self.draw_marks(pass, gpu, mark, marks);
        }
    }

    fn draw_marks(&self, pass: &mut wgpu::RenderPass<'_>, gpu: &GpuStructural, mark: Mark, marks: &Marks) {
        if let (Some((segs, total)), false) = (&gpu.segs, marks.segs.is_empty()) {
            pass.set_pipeline(&self.seg[mark as usize]);
            for &(first, count) in &marks.segs {
                let count = count.min(total.saturating_sub(first));
                if count > 0 {
                    pass.set_vertex_buffer(0, segs.slice(first as u64 * SEG_STRIDE..(first + count) as u64 * SEG_STRIDE));
                    pass.draw(0..4, 0..count);
                }
            }
        }
        if let (Some(glyphs), false) = (&gpu.glyphs, marks.glyphs.is_empty()) {
            pass.set_pipeline(&self.glyph[mark as usize]);
            for &(first, count) in &marks.glyphs {
                // a range may run over kinds: one draw per kind it covers
                for &(kind, k0, kn) in &gpu.kinds {
                    let (a, b) = (first.max(k0), (first + count).min(k0 + kn));
                    if a < b {
                        self.draw_glyphs(pass, glyphs, kind, a, b - a);
                    }
                }
            }
        }
    }

    fn draw_glyphs(&self, pass: &mut wgpu::RenderPass<'_>, glyphs: &wgpu::Buffer, kind: GlyphKind, first: u32, count: u32) {
        let Some((vertices, indices, index_count)) = self.library.kinds.get(kind as usize) else { return };
        if count == 0 {
            return;
        }
        pass.set_vertex_buffer(0, self.library.vertices.slice(vertices.clone()));
        pass.set_index_buffer(self.library.indices.slice(indices.clone()), wgpu::IndexFormat::Uint32);
        pass.set_vertex_buffer(1, glyphs.slice(first as u64 * GLYPH_STRIDE..(first + count) as u64 * GLYPH_STRIDE));
        pass.draw_indexed(0..*index_count, 0, 0..count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_layouts_match_the_shader() {
        // structural.wgsl reads these strides and offsets through the vertex attributes
        assert_eq!(std::mem::size_of::<SegInstance>(), 60);
        assert_eq!(std::mem::size_of::<GlyphInstance>(), 56);
        assert_eq!(std::mem::size_of::<StructUniforms>(), 192);
        let (v, i) = cube();
        assert_eq!((v.len(), i.len()), (24, 36));
        assert!(v.iter().all(|v| v.position.iter().all(|c| c.abs() == 0.5)));
    }

    #[test]
    fn uniforms_follow_the_background() {
        let lod = Lod::default();
        let light = StructUniforms::new((800.0, 600.0), 2.4, [0.95; 3], &lod);
        let dark = StructUniforms::new((800.0, 600.0), 2.4, [0.1; 3], &lod);
        assert!(light.colors[0][2] < dark.colors[0][2]);
        assert_eq!(light.viewport, [800.0, 600.0, 1.0 / 800.0, 1.0 / 600.0]);
        assert_eq!((light.sizes[1], light.lod[0], light.lod[2], light.scales[3]), (7.0, 3.0, 6.0, 2.4));
    }

    /// naga validates the module the pipelines are made from and translates every structural
    /// entry point to GLSL ES 3.0 for WebGL2, as wgpu does in the browser; neither base vertex nor
    /// base instance may appear.
    #[test]
    fn the_shader_validates_and_translates_to_webgl2() {
        use naga::back::glsl;
        let source = format!("{}
{}", include_str!("scene.wgsl"), include_str!("structural.wgsl"));
        let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
        let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::default())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
        let vertex = ["vs_seg", "vs_seg_selected", "vs_seg_hovered", "vs_glyph", "vs_glyph_selected", "vs_glyph_hovered"];
        let stages = vertex.iter().map(|e| (naga::ShaderStage::Vertex, *e)).chain([(naga::ShaderStage::Fragment, "fs_seg"), (naga::ShaderStage::Fragment, "fs_glyph")]);
        for (stage, entry) in stages {
            let options = glsl::Options { version: glsl::Version::Embedded { version: 300, is_webgl: true }, ..Default::default() };
            let pipeline = glsl::PipelineOptions { shader_stage: stage, entry_point: entry.to_string(), multiview: None };
            let mut out = String::new();
            let mut writer = glsl::Writer::new(&mut out, &module, &info, &options, &pipeline, naga::proc::BoundsCheckPolicies::default())
                .unwrap_or_else(|e| panic!("{entry}: {e}"));
            writer.write().unwrap_or_else(|e| panic!("{entry}: {e}"));
            assert!(!out.contains("gl_BaseInstance") && !out.contains("gl_BaseVertex"), "{entry}");
        }
    }
}
