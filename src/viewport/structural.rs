//! The structural pipelines of the viewport (`structural.wgsl`): wide segments for member axes
//! and screen-size glyphs for nodes, drawn in the scene's multisampled pass.
//!
//! Both bind the scene's model uniforms at `@group(0)` (model matrix, section box and plane, the
//! offsets of the exploded view), so the up-axis override, the section and the exploded view apply
//! to every instance; the structural state is the separate [`StructUniforms`] at `@group(1)`, so the
//! scene's uniforms and its hot pipelines do not change. Each pipeline comes three times: plain,
//! selected and hovered. The selection and the hover are drawn again on top from small instance
//! buffers of their own, gathered when they change ([`GpuStructural::gather`]), so a selection of
//! any size costs one draw per pipeline and glyph kind. Instances of hidden source models are
//! left out of the plain draw by range (a part's instances are contiguous).
//!
//! The pipelines are made the first time a model with structural objects is uploaded, so a scene
//! without them never compiles the shader (on WebGL2 every pipeline is a GLSL program linked
//! before the first frame).
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

/// The instances of a model's [`StructuralDisplay`] (or of a selection of them) on the GPU, with
/// the units of the exploded view written in.
pub struct GpuStructural {
    segs: Option<(wgpu::Buffer, u32)>,
    glyphs: Option<wgpu::Buffer>,
    /// (kind, first glyph, count), in kind order.
    kinds: Vec<(GlyphKind, u32, u32)>,
}

/// The instances of `ranges` of `display` (all: `None`), units written in, and their glyph kinds.
fn instances(
    display: &StructuralDisplay,
    units: &[u32],
    ranges: Option<&Marks>,
) -> (Vec<SegInstance>, Vec<GlyphInstance>, Vec<(GlyphKind, u32, u32)>) {
    let unit = |item: u32| units.get(item as usize).copied().unwrap_or(0);
    let all_segs = [(0, display.segs.len() as u32)];
    let all_glyphs = [(0, display.glyphs.len() as u32)];
    let seg_ranges = ranges.map_or(&all_segs[..], |m| &m.segs[..]);
    let glyph_ranges = ranges.map_or(&all_glyphs[..], |m| &m.glyphs[..]);
    let clamp = |(first, count): (u32, u32), len: usize| (first as usize).min(len)..((first + count) as usize).min(len);
    let mut segs = Vec::new();
    for &r in seg_ranges {
        let r = clamp(r, display.segs.len());
        segs.extend(display.segs[r.clone()].iter().zip(&display.seg_item[r]).map(|(s, &item)| SegInstance { unit: unit(item), ..*s }));
    }
    // the ranges are sorted and the display's glyphs sorted by kind: so are these
    let (mut glyphs, mut kinds) = (Vec::new(), Vec::new());
    for &(kind, k0, kn) in &display.glyph_kinds {
        let first = glyphs.len() as u32;
        for &(a, n) in glyph_ranges {
            let (a, b) = (a.max(k0), (a + n).min(k0 + kn));
            if a < b {
                let r = clamp((a, b - a), display.glyphs.len());
                glyphs.extend(display.glyphs[r.clone()].iter().zip(&display.glyph_item[r]).map(|(g, &item)| GlyphInstance { unit: unit(item), ..*g }));
            }
        }
        let count = glyphs.len() as u32 - first;
        if count > 0 {
            kinds.push((kind, first, count));
        }
    }
    (segs, glyphs, kinds)
}

impl GpuStructural {
    /// Uploads `display`, its items in units `units` (one per display item; empty: unit 0).
    pub fn new(device: &wgpu::Device, display: &StructuralDisplay, units: &[u32]) -> Option<Self> {
        Self::upload(device, instances(display, units, None), "o3v.structural")
    }

    /// Uploads the instances of `marks` only (the selection or the hover), to draw on top.
    pub fn gather(device: &wgpu::Device, display: &StructuralDisplay, units: &[u32], marks: &Marks) -> Option<Self> {
        if marks.is_empty() {
            return None;
        }
        Self::upload(device, instances(display, units, Some(marks)), "o3v.structural.marked")
    }

    fn upload(
        device: &wgpu::Device,
        (segs, glyphs, kinds): (Vec<SegInstance>, Vec<GlyphInstance>, Vec<(GlyphKind, u32, u32)>),
        label: &str,
    ) -> Option<Self> {
        if segs.is_empty() && glyphs.is_empty() {
            return None;
        }
        let segs = (!segs.is_empty()).then(|| {
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(&segs),
                usage: wgpu::BufferUsages::VERTEX,
            });
            (buffer, segs.len() as u32)
        });
        let glyphs = (!glyphs.is_empty()).then(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(&glyphs),
                usage: wgpu::BufferUsages::VERTEX,
            })
        });
        Some(Self { segs, glyphs, kinds })
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
        let make = |label: &str, vs: &str, fs: &str, buffers: &[wgpu::VertexBufferLayout], topology, bias| {
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
                    bias,
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
        // A marked line meets the depth its plain line wrote (the shader gives both the line's own
        // depth); pulled a little towards the eye it wins that tie wherever the two triangulations
        // round apart. Marked glyphs are larger and cover the plain ones by themselves.
        let plain = wgpu::DepthBiasState::default();
        let ahead = wgpu::DepthBiasState { constant: -4, slope_scale: -1.0, clamp: 0.0 };
        let seg = [("vs_seg", plain), ("vs_seg_selected", ahead), ("vs_seg_hovered", ahead)]
            .map(|(vs, bias)| make("o3v.structural.seg", vs, "fs_seg", &seg_layout, strip, bias));
        let glyph = ["vs_glyph", "vs_glyph_selected", "vs_glyph_hovered"]
            .map(|vs| make("o3v.structural.glyph", vs, "fs_glyph", &glyph_layout, list, plain));
        Self { uniforms, group, seg, glyph, library: GlyphLibrary::new(device) }
    }

    pub fn write(&self, queue: &wgpu::Queue, uniforms: &StructUniforms) {
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(uniforms));
    }

    /// Draws the instances (only the ranges of `shown`, when some source models are hidden), then
    /// the selection and the hover on top from their own gathered instances. The scene's model
    /// group must be bound at 0; the structural group is bound here at 1.
    pub fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        gpu: &GpuStructural,
        shown: Option<&Marks>,
        selected: Option<&GpuStructural>,
        hovered: Option<&GpuStructural>,
    ) {
        pass.set_bind_group(1, &self.group, &[]);
        match shown {
            None => self.draw_all(pass, gpu, Mark::Plain),
            Some(shown) => self.draw_ranges(pass, gpu, shown),
        }
        for (mark, marked) in [(Mark::Selected, selected), (Mark::Hovered, hovered)] {
            if let Some(marked) = marked {
                self.draw_all(pass, marked, mark);
            }
        }
    }

    /// Every instance of `gpu`: one draw for the segments, one per glyph kind.
    fn draw_all(&self, pass: &mut wgpu::RenderPass<'_>, gpu: &GpuStructural, mark: Mark) {
        if let Some((segs, count)) = &gpu.segs {
            pass.set_pipeline(&self.seg[mark as usize]);
            pass.set_vertex_buffer(0, segs.slice(..));
            pass.draw(0..4, 0..*count);
        }
        if let Some(glyphs) = &gpu.glyphs {
            pass.set_pipeline(&self.glyph[mark as usize]);
            for &(kind, first, count) in &gpu.kinds {
                self.draw_glyphs(pass, glyphs, kind, first, count);
            }
        }
    }

    /// The instance ranges of `ranges`, plain (few: one per run of parts shown).
    fn draw_ranges(&self, pass: &mut wgpu::RenderPass<'_>, gpu: &GpuStructural, ranges: &Marks) {
        if let (Some((segs, total)), false) = (&gpu.segs, ranges.segs.is_empty()) {
            pass.set_pipeline(&self.seg[Mark::Plain as usize]);
            for &(first, count) in &ranges.segs {
                let count = count.min(total.saturating_sub(first));
                if count > 0 {
                    pass.set_vertex_buffer(0, segs.slice(first as u64 * SEG_STRIDE..(first + count) as u64 * SEG_STRIDE));
                    pass.draw(0..4, 0..count);
                }
            }
        }
        if let (Some(glyphs), false) = (&gpu.glyphs, ranges.glyphs.is_empty()) {
            pass.set_pipeline(&self.glyph[Mark::Plain as usize]);
            for &(first, count) in &ranges.glyphs {
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

    /// The selection and the hover are gathered into instances of their own, in kind order, with
    /// the units written in.
    #[test]
    fn marked_instances_are_gathered_in_order() {
        use crate::structural::display::{style, DisplayItem};
        let mut d = StructuralDisplay::default();
        let seg = |x: f32| SegInstance { a: [x, 0.0, 0.0], a_off: [0.0; 3], b: [x + 1.0, 0.0, 0.0], b_off: [0.0; 3], color: [0; 4], unit: 0, style: style::AXIS };
        let node = |x: f32| GlyphInstance { pos: [x, 0.0, 0.0], rot: [0.0, 0.0, 0.0, 1.0], color: [0; 4], unit: 0, size: [0.0; 2], param: [0.0; 2], kind_lod: 0 };
        d.segs = (0..5).map(|k| seg(k as f32)).collect();
        d.seg_item = vec![0, 1, 2, 3, 4];
        d.glyphs = (0..4).map(|k| node(k as f32)).collect();
        d.glyph_item = vec![5, 6, 7, 8];
        d.glyph_kinds = vec![(GlyphKind::Node, 0, 4)];
        d.items = (0..9).map(|k| DisplayItem { element: k, kind: bim_structural::ItemKind::Member, model: 0, segs: (0, 0), glyphs: (0, 0) }).collect();
        let units: Vec<u32> = (0..9).map(|k| 10 + k).collect();
        let marks = Marks { segs: vec![(1, 1), (3, 2)], glyphs: vec![(0, 1), (2, 2)] };
        let (segs, glyphs, kinds) = instances(&d, &units, Some(&marks));
        assert_eq!(segs.iter().map(|s| (s.a[0], s.unit)).collect::<Vec<_>>(), vec![(1.0, 11), (3.0, 13), (4.0, 14)]);
        assert_eq!(glyphs.iter().map(|g| (g.pos[0], g.unit)).collect::<Vec<_>>(), vec![(0.0, 15), (2.0, 17), (3.0, 18)]);
        assert_eq!(kinds, vec![(GlyphKind::Node, 0, 3)]);
        // everything; ranges past the end are clamped
        let (segs, glyphs, kinds) = instances(&d, &[], None);
        assert_eq!((segs.len(), glyphs.len(), kinds), (5, 4, vec![(GlyphKind::Node, 0, 4)]));
        let past = Marks { segs: vec![(4, 9)], glyphs: vec![] };
        let (segs, glyphs, kinds) = instances(&d, &[], Some(&past));
        assert_eq!((segs.len(), glyphs.len(), kinds.len()), (1, 0, 0));
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
