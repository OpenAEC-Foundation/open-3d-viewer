// Copies the resolved viewport frame into iced's render target.

struct BlitParams {
    // xy: top-left of the viewport in target pixels
    offset: vec4<f32>,
};

@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var<uniform> blit: BlitParams;

@vertex
fn vs_blit(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(i & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_blit(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(pos.xy - blit.offset.xy));
    return textureLoad(frame, p, 0);
}
