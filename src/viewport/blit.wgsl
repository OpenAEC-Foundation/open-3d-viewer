// Copies the resolved viewport frame into iced's render target, with the ambient shadows of
// the model where they are on.

struct BlitParams {
    // xy: top-left of the viewport in target pixels
    offset: vec4<f32>,
    // ambient shadows: x on (1), y parallel projection (1), z near and w far of the depth range
    ambient: vec4<f32>,
    // x: pixels a metre is at depth 1 (perspective) or anywhere (parallel), y: the reach of a
    // shadow (metres), z: how dark, w: the darkest
    reach: vec4<f32>,
};

@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var<uniform> blit: BlitParams;
// the depth of the model's opaque surfaces, drawn once more for the shadows
@group(0) @binding(2) var depth: texture_depth_2d;

@vertex
fn vs_blit(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(i & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

// How far from the eye a depth lies (metres along the view).
fn distance_of(d: f32) -> f32 {
    if (blit.ambient.y > 0.5) {
        return d * blit.ambient.w;
    }
    return blit.ambient.z * blit.ambient.w / (blit.ambient.w - d * (blit.ambient.w - blit.ambient.z));
}

// How much darker a surface is where it lies deeper than what is around it: in corners, under
// overhangs and behind what stands in front. Opposite points are taken in pairs, so a flat
// surface seen at a slant stays as it is.
fn shadow(p: vec2<i32>) -> f32 {
    let d = textureLoad(depth, p, 0);
    if (d >= 1.0) {
        return 0.0;
    }
    let z = distance_of(d);
    var radius = blit.reach.x * blit.reach.y;
    if (blit.ambient.y < 0.5) {
        radius = radius / max(z, 1e-4);
    }
    radius = clamp(radius, 2.0, 48.0);
    let last = vec2<i32>(textureDimensions(depth)) - vec2<i32>(1, 1);
    var sum = 0.0;
    for (var k = 0; k < 8; k = k + 1) {
        // eight directions over half a turn, at two distances in turn
        let angle = (f32(k) + 0.5) * 0.3926991;
        let r = radius * select(1.0, 0.5, (k & 1) == 1);
        let step = vec2<f32>(cos(angle), sin(angle)) * r;
        for (var side = -1; side <= 1; side = side + 2) {
            let q = clamp(p + vec2<i32>(step * f32(side)), vec2<i32>(0, 0), last);
            let other = distance_of(textureLoad(depth, q, 0));
            sum = sum + clamp((z - other) / blit.reach.y, -1.0, 1.0);
        }
    }
    return clamp((sum / 16.0 - 0.02) * blit.reach.z, 0.0, blit.reach.w);
}

@fragment
fn fs_blit(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(pos.xy - blit.offset.xy));
    let color = textureLoad(frame, p, 0);
    if (blit.ambient.x < 0.5) {
        return color;
    }
    return vec4<f32>(color.rgb * (1.0 - shadow(p)), color.a);
}
