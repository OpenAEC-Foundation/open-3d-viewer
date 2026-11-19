//! The splat scenes of a model (layers with [`SplatData`]) on the GPU: their splats in textures,
//! and the order they are drawn in, sorted back to front on the CPU whenever the view turns or
//! moves. Sorting on the CPU and reading the splats from textures in the vertex shader works the
//! same on every backend, WebGL2 included, which has neither compute shaders nor storage buffers.

use crate::model::{Model, SplatData};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use iced::wgpu;
use iced::wgpu::util::DeviceExt;
use std::sync::Arc;

/// Texels per splat in the data texture: centre and colour, then the covariance.
const TEXELS: u32 = 3;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct SplatUniforms {
    view: [f32; 4],
    frame: [[f32; 4]; 3],
}

/// The pipeline of the splats, made with the scene's.
pub struct SplatPipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

impl SplatPipeline {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, uniform_layout: &wgpu::BindGroupLayout, samples: u32, depth: wgpu::TextureFormat) -> Self {
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Uint, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("o3v.splats.layout"),
            entries: &[
                texture(0),
                texture(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
            ],
        });
        let source = format!("{}\n{}", include_str!("scene.wgsl"), include_str!("splat.wgsl"));
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("o3v.splats.shader"), source: wgpu::ShaderSource::Wgsl(source.into()) });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("o3v.splats.pipeline_layout"),
            bind_group_layouts: &[uniform_layout, &layout],
            push_constant_ranges: &[],
        });
        let attributes = wgpu::vertex_attr_array![0 => Uint32];
        // premultiplied, back to front
        let blend = wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("o3v.splats"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_splat"),
                buffers: &[wgpu::VertexBufferLayout { array_stride: 4, step_mode: wgpu::VertexStepMode::Instance, attributes: &attributes }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, cull_mode: None, ..Default::default() },
            // behind the model's surfaces where they are nearer, never hiding each other
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState { count: samples, mask: !0, alpha_to_coverage_enabled: false },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_splat"),
                targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState { color: blend, alpha: blend }), write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview: None,
            cache: None,
        });
        Self { pipeline, layout }
    }
}

/// One splat layer on the GPU.
struct GpuSplatLayer {
    layer: usize,
    group: wgpu::BindGroup,
    uniforms: wgpu::Buffer,
    order: wgpu::Buffer,
    /// The splats drawn (every `stride`-th of the layer, when the textures cannot hold all).
    count: u32,
    stride: usize,
    data_width: u32,
    sh_width: u32,
    sh_texels: u32,
    sh_degree: u8,
    data: Arc<SplatData>,
    /// Scratch of the sort, kept between frames.
    keys: Vec<u16>,
    sorted: Vec<u32>,
}

/// The splat layers of the uploaded model, and the view they were last sorted for.
#[derive(Default)]
pub struct GpuSplats {
    model_id: u64,
    layers: Vec<GpuSplatLayer>,
    sorted_for: Option<[f32; 6]>,
}

impl GpuSplats {
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// Uploads the splat layers of `model` when it is another model.
    pub fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, pipeline: &SplatPipeline, model: Option<&Arc<Model>>) {
        let id = model.map_or(0, |m| m.id);
        if id == self.model_id {
            return;
        }
        self.model_id = id;
        self.layers.clear();
        self.sorted_for = None;
        let Some(model) = model else { return };
        let max = device.limits().max_texture_dimension_2d;
        for (l, layer) in model.layers.iter().enumerate() {
            let Some(data) = &layer.splats else { continue };
            let n = layer.vertex_count().min(data.covariances.len());
            if n == 0 {
                continue;
            }
            let data_width = max / TEXELS * TEXELS;
            // as many as the texture holds: every n-th beyond that
            let capacity = (data_width / TEXELS) as usize * max as usize;
            let stride = n.div_ceil(capacity).max(1);
            let count = n.div_ceil(stride);
            let rows = (count as u32 * TEXELS).div_ceil(data_width).max(1);
            let mut texels = vec![[0u32; 4]; (data_width * rows) as usize];
            let colors = layer.colors.as_deref();
            for (k, i) in (0..n).step_by(stride).enumerate() {
                let p = &layer.positions[3 * i..3 * i + 3];
                let c = colors.and_then(|c| c.get(i)).copied().unwrap_or([200, 200, 200, 255]);
                let v = data.covariances[i];
                texels[k * 3] = [p[0].to_bits(), p[1].to_bits(), p[2].to_bits(), u32::from_le_bytes(c)];
                texels[k * 3 + 1] = [v[0].to_bits(), v[1].to_bits(), v[2].to_bits(), v[3].to_bits()];
                texels[k * 3 + 2] = [v[4].to_bits(), v[5].to_bits(), 0, 0];
            }
            let data_texture = texture(device, queue, "o3v.splats.data", data_width, rows, &texels);
            // the view-dependent colour, as half floats; left out when its texture would be too big
            let per = sh_values(data.sh_degree);
            let mut sh_degree = data.sh_degree;
            let sh_texels = (per.div_ceil(2) as u32).div_ceil(4);
            let sh_width = if sh_texels > 0 { max / sh_texels * sh_texels } else { 1 };
            let sh_rows = (count as u32 * sh_texels).div_ceil(sh_width.max(1)).max(1);
            if sh_texels == 0 || sh_rows > max || data.sh.len() < n * per {
                sh_degree = 0;
            }
            let sh_texture = if sh_degree > 0 {
                let mut words = vec![0u32; (sh_width * sh_rows * 4) as usize];
                for (k, i) in (0..n).step_by(stride).enumerate() {
                    let values = &data.sh[i * per..(i + 1) * per];
                    let base = k * sh_texels as usize * 4;
                    for (w, pair) in values.chunks(2).enumerate() {
                        let lo = half(pair[0]) as u32;
                        let hi = pair.get(1).map_or(0, |v| half(*v) as u32);
                        words[base + w] = lo | (hi << 16);
                    }
                }
                let texels: Vec<[u32; 4]> = words.chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]).collect();
                texture(device, queue, "o3v.splats.sh", sh_width, sh_rows, &texels)
            } else {
                texture(device, queue, "o3v.splats.sh", 1, 1, &[[0; 4]])
            };
            let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("o3v.splats.uniforms"),
                size: std::mem::size_of::<SplatUniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("o3v.splats.group"),
                layout: &pipeline.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&data_texture.create_view(&Default::default())) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&sh_texture.create_view(&Default::default())) },
                    wgpu::BindGroupEntry { binding: 2, resource: uniforms.as_entire_binding() },
                ],
            });
            let order: Vec<u32> = (0..count as u32).collect();
            let order = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("o3v.splats.order"),
                contents: bytemuck::cast_slice(&order),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            });
            self.layers.push(GpuSplatLayer {
                layer: l,
                group,
                uniforms,
                order,
                count: count as u32,
                stride,
                data_width,
                sh_width,
                sh_texels,
                sh_degree,
                data: data.clone(),
                keys: Vec::new(),
                sorted: Vec::new(),
            });
        }
    }

    /// Writes the uniforms of the splats, and sorts them back to front when the view changed.
    pub fn prepare(&mut self, queue: &wgpu::Queue, model: Option<&Arc<Model>>, model_matrix: Mat4, eye: Vec3, forward: Vec3, size: (u32, u32)) {
        let Some(model) = model.filter(|_| !self.layers.is_empty()) else { return };
        let view = [eye.x, eye.y, eye.z, forward.x, forward.y, forward.z];
        let resort = self.sorted_for.is_none_or(|last| last.iter().zip(&view).any(|(a, b)| (a - b).abs() > 1e-5));
        if resort {
            self.sorted_for = Some(view);
        }
        // the view direction in the layers' axes: the depth of a splat is its centre along it
        let back = Mat4::from_cols(model_matrix.x_axis, model_matrix.y_axis, model_matrix.z_axis, glam::Vec4::W).transpose();
        let along = back.transform_vector3(forward);
        for gpu in &mut self.layers {
            let f = gpu.data.sh_frame;
            let uniforms = SplatUniforms {
                view: [size.0 as f32, size.1 as f32, gpu.data_width as f32, gpu.sh_width as f32],
                frame: [
                    [f[0][0], f[0][1], f[0][2], gpu.sh_degree as f32],
                    [f[1][0], f[1][1], f[1][2], if gpu.data.linear { 1.0 } else { 0.0 }],
                    [f[2][0], f[2][1], f[2][2], gpu.sh_texels as f32],
                ],
            };
            queue.write_buffer(&gpu.uniforms, 0, bytemuck::bytes_of(&uniforms));
            if resort {
                let Some(layer) = model.layers.get(gpu.layer) else { continue };
                sort_back_to_front(&layer.positions, gpu.stride, along, &mut gpu.keys, &mut gpu.sorted);
                queue.write_buffer(&gpu.order, 0, bytemuck::cast_slice(&gpu.sorted));
            }
        }
    }

    /// Draws the splats of the visible layers (after the opaque surfaces).
    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, pipeline: &'a SplatPipeline, model_group: &'a wgpu::BindGroup, visible: impl Fn(usize) -> bool) {
        let mut set = false;
        for gpu in self.layers.iter().filter(|g| visible(g.layer)) {
            if !set {
                pass.set_pipeline(&pipeline.pipeline);
                pass.set_bind_group(0, model_group, &[]);
                set = true;
            }
            pass.set_bind_group(1, &gpu.group, &[]);
            pass.set_vertex_buffer(0, gpu.order.slice(..));
            pass.draw(0..4, 0..gpu.count);
        }
    }
}

/// The values of the view-dependent colour per splat for a degree: 0, 9, 24 or 45.
fn sh_values(degree: u8) -> usize {
    let d = degree.min(3) as usize;
    ((d + 1) * (d + 1) - 1) * 3
}

fn texture(device: &wgpu::Device, queue: &wgpu::Queue, label: &str, width: u32, height: u32, texels: &[[u32; 4]]) -> wgpu::Texture {
    device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Uint,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        bytemuck::cast_slice(texels),
    )
}

/// The order to draw the splats in (every `stride`-th centre of `positions`, numbered as they
/// lie in the texture): far to near along `along`, by a counting sort on 16-bit depths.
fn sort_back_to_front(positions: &[f32], stride: usize, along: Vec3, keys: &mut Vec<u16>, sorted: &mut Vec<u32>) {
    let depth = |k: usize| {
        let i = k * stride;
        positions[3 * i] * along.x + positions[3 * i + 1] * along.y + positions[3 * i + 2] * along.z
    };
    let n = (positions.len() / 3).div_ceil(stride.max(1));
    let (low, high) = (0..n).map(depth).fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), d| (lo.min(d), hi.max(d)));
    let scale = if high > low { 65535.0 / (high - low) } else { 0.0 };
    keys.clear();
    // far first: the largest depth gets key 0
    keys.extend((0..n).map(|k| ((high - depth(k)) * scale) as u16));
    let mut counts = vec![0u32; 65537];
    for &key in keys.iter() {
        counts[key as usize + 1] += 1;
    }
    for b in 1..counts.len() {
        counts[b] += counts[b - 1];
    }
    sorted.clear();
    sorted.resize(n, 0);
    for (k, &key) in keys.iter().enumerate() {
        let slot = &mut counts[key as usize];
        sorted[*slot as usize] = k as u32;
        *slot += 1;
    }
}

/// A half-precision float (rounded to nearest), as `unpack2x16float` reads it.
fn half(v: f32) -> u16 {
    let bits = v.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mantissa = bits & 0x7f_ffff;
    if v.is_nan() {
        return sign | 0x7e00;
    }
    if exponent >= 31 {
        return sign | 0x7c00;
    }
    if exponent <= 0 {
        if exponent < -10 {
            return sign;
        }
        let m = (mantissa | 0x80_0000) >> (1 - exponent);
        return sign | ((m + 0x1000) >> 13) as u16;
    }
    let rounded = ((exponent as u32) << 10) + ((mantissa + 0x1000) >> 13);
    sign | rounded.min(0x7bff) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_far_to_near() {
        // depths along +x: 3, 1, 2, 0
        let positions = [3.0, 0.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let (mut keys, mut sorted) = (Vec::new(), Vec::new());
        sort_back_to_front(&positions, 1, Vec3::X, &mut keys, &mut sorted);
        assert_eq!(sorted, [0, 2, 1, 3]);
        // looking the other way
        sort_back_to_front(&positions, 1, -Vec3::X, &mut keys, &mut sorted);
        assert_eq!(sorted, [3, 1, 2, 0]);
        // every second splat, numbered as drawn: depths 3 and 2
        sort_back_to_front(&positions, 2, Vec3::X, &mut keys, &mut sorted);
        assert_eq!(sorted, [0, 1]);
    }

    /// naga validates the splat shader and translates it to GLSL ES 3.0 for WebGL2, as wgpu does
    /// in the browser: the splats draw there too.
    #[test]
    fn the_shader_validates_and_translates_to_webgl2() {
        use naga::back::glsl;
        let source = format!("{}\n{}", include_str!("scene.wgsl"), include_str!("splat.wgsl"));
        let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
        let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::default())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
        for (stage, entry) in [(naga::ShaderStage::Vertex, "vs_splat"), (naga::ShaderStage::Fragment, "fs_splat")] {
            let options = glsl::Options { version: glsl::Version::Embedded { version: 300, is_webgl: true }, ..Default::default() };
            let pipeline = glsl::PipelineOptions { shader_stage: stage, entry_point: entry.to_string(), multiview: None };
            let mut out = String::new();
            let mut writer = glsl::Writer::new(&mut out, &module, &info, &options, &pipeline, naga::proc::BoundsCheckPolicies::default())
                .unwrap_or_else(|e| panic!("{entry}: {e}"));
            writer.write().unwrap_or_else(|e| panic!("{entry}: {e}"));
        }
    }

    #[test]
    fn half_floats_round_trip() {
        for (v, bits) in [(1.0f32, 0x3c00u16), (-2.0, 0xc000), (0.5, 0x3800), (0.0, 0), (65504.0, 0x7bff)] {
            assert_eq!(half(v), bits, "{v}");
        }
        assert_eq!(half(1e9), 0x7c00);
        // small values become subnormal
        assert_eq!(half(2f32.powi(-24)), 1);
    }
}
