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
    // the depth shading of point clouds: x on (1), y how strong, z the distance of the
    // neighbours it compares with (pixels)
    cloud: vec4<f32>,
};

@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var<uniform> blit: BlitParams;
// the depth of the model's opaque surfaces, drawn once more for the shadows
@group(0) @binding(2) var depth: texture_depth_2d;
// the same with the points of the clouds drawn over it
@group(0) @binding(3) var cloud_depth: texture_depth_2d;

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

// How much darker a point of a cloud is where it lies behind the points around it (eye-dome
// lighting): the depth of a cloud shows in its edges, as its points are not lit.
fn dome(p: vec2<i32>, d: f32) -> f32 {
    let z = log2(distance_of(d));
    let last = vec2<i32>(textureDimensions(cloud_depth)) - vec2<i32>(1, 1);
    var sum = 0.0;
    for (var k = 0; k < 8; k = k + 1) {
        let angle = f32(k) * 0.7853982;
        let q = clamp(p + vec2<i32>(round(vec2<f32>(cos(angle), sin(angle)) * blit.cloud.z)), vec2<i32>(0, 0), last);
        let other = textureLoad(cloud_depth, q, 0);
        // the background is no neighbour: an edge against it stays as it is
        if (other < 1.0) {
            sum = sum + max(0.0, z - log2(distance_of(other)));
        }
    }
    return 1.0 - exp(-sum / 8.0 * blit.cloud.y);
}

@fragment
fn fs_blit(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(pos.xy - blit.offset.xy));
    let color = textureLoad(frame, p, 0);
    // a point of a cloud lies in front of the surfaces at its pixel
    var point = false;
    if (blit.cloud.x > 0.5) {
        let d = textureLoad(cloud_depth, p, 0);
        point = d < 1.0 && d < textureLoad(depth, p, 0);
        if (point) {
            return vec4<f32>(color.rgb * (1.0 - dome(p, d)), color.a);
        }
    }
    if (blit.ambient.x < 0.5) {
        return color;
    }
    return vec4<f32>(color.rgb * (1.0 - shadow(p)), color.a);
}
