// Splat scenes (3D Gaussian splatting): every splat is a quad on screen round its centre, as wide
// as its Gaussian projected (three standard deviations), drawn back to front (the instance order
// is sorted on the CPU) and blended with premultiplied alpha. Appended to `scene.wgsl`, whose
// uniforms (group 0), `clipped`, `to_linear` and `encode` it uses.
//
// The splats lie in float textures rather than storage buffers, as WebGL2 has none: three texels
// a splat in `splat_data` (centre and colour, then the covariance), and the coefficients of the
// view-dependent colour as half floats in `splat_sh`.

struct SplatUniforms {
    // x, y: the viewport in pixels; z: texels per row of `splat_data`; w: of `splat_sh`
    view: vec4<f32>,
    // the turn from the layer's axes into the axes of the coefficients, row by row; the w of the
    // first row is the degree of the coefficients, of the second 1 when the colours are linear,
    // of the third the texels a splat has in `splat_sh`
    frame_a: vec4<f32>,
    frame_b: vec4<f32>,
    frame_c: vec4<f32>,
};

@group(1) @binding(0) var splat_data: texture_2d<u32>;
@group(1) @binding(1) var splat_sh: texture_2d<u32>;
@group(1) @binding(2) var<uniform> su: SplatUniforms;

struct SplatOut {
    @builtin(position) clip: vec4<f32>,
    // where in the quad, in standard deviations along the Gaussian's axes on screen
    @location(0) local: vec2<f32>,
    // linear colour and opacity
    @location(1) color: vec4<f32>,
};

fn splat_texel(i: u32, k: u32) -> vec4<u32> {
    let width = u32(su.view.z);
    let t = i * 3u + k;
    return textureLoad(splat_data, vec2<i32>(i32(t % width), i32(t / width)), 0);
}

// The coefficients of degree 1 and up of splat `i`, as half floats two to a word.
fn sh_words(i: u32, texels: u32) -> array<vec4<u32>, 6> {
    var words: array<vec4<u32>, 6>;
    let width = u32(su.view.w);
    for (var k = 0u; k < texels; k = k + 1u) {
        let t = i * texels + k;
        words[k] = textureLoad(splat_sh, vec2<i32>(i32(t % width), i32(t / width)), 0);
    }
    return words;
}

fn sh_value(words: ptr<function, array<vec4<u32>, 6>>, v: u32) -> f32 {
    let word = (*words)[v / 8u][(v / 2u) % 4u];
    return half_float(select(word & 0xffffu, word >> 16u, (v & 1u) == 1u));
}

// A half float from its bits (not `unpack2x16float`, which not every backend has); the tiny
// subnormal ones are zero.
fn half_float(h: u32) -> f32 {
    let exponent = (h >> 10u) & 31u;
    if (exponent == 0u) {
        return 0.0;
    }
    let bits = ((h & 0x8000u) << 16u) | ((exponent + 112u) << 23u) | ((h & 0x3ffu) << 13u);
    return bitcast<f32>(bits);
}

fn sh_coefficient(words: ptr<function, array<vec4<u32>, 6>>, k: u32) -> vec3<f32> {
    return vec3<f32>(sh_value(words, 3u * k), sh_value(words, 3u * k + 1u), sh_value(words, 3u * k + 2u));
}

// The view-dependent part of the colour seen along `d` (unit, in the axes of the coefficients).
fn sh_color(i: u32, d: vec3<f32>) -> vec3<f32> {
    let degree = u32(su.frame_a.w + 0.5);
    if (degree == 0u) {
        return vec3<f32>(0.0);
    }
    var words = sh_words(i, u32(su.frame_c.w + 0.5));
    let x = d.x;
    let y = d.y;
    let z = d.z;
    let c1 = 0.4886025;
    var c = -c1 * y * sh_coefficient(&words, 0u) + c1 * z * sh_coefficient(&words, 1u) - c1 * x * sh_coefficient(&words, 2u);
    if (degree >= 2u) {
        let xx = x * x;
        let yy = y * y;
        let zz = z * z;
        c = c + 1.0925484 * x * y * sh_coefficient(&words, 3u)
            - 1.0925484 * y * z * sh_coefficient(&words, 4u)
            + 0.3153916 * (2.0 * zz - xx - yy) * sh_coefficient(&words, 5u)
            - 1.0925484 * x * z * sh_coefficient(&words, 6u)
            + 0.5462742 * (xx - yy) * sh_coefficient(&words, 7u);
        if (degree >= 3u) {
            c = c - 0.5900436 * y * (3.0 * xx - yy) * sh_coefficient(&words, 8u)
                + 2.8906114 * x * y * z * sh_coefficient(&words, 9u)
                - 0.4570458 * y * (4.0 * zz - xx - yy) * sh_coefficient(&words, 10u)
                + 0.3731763 * z * (2.0 * zz - 3.0 * xx - 3.0 * yy) * sh_coefficient(&words, 11u)
                - 0.4570458 * x * (4.0 * zz - xx - yy) * sh_coefficient(&words, 12u)
                + 1.4453057 * z * (xx - yy) * sh_coefficient(&words, 13u)
                - 0.5900436 * x * (xx - 3.0 * yy) * sh_coefficient(&words, 14u);
        }
    }
    return c;
}

@vertex
fn vs_splat(@location(0) index: u32, @builtin(vertex_index) vi: u32) -> SplatOut {
    var out: SplatOut;
    out.clip = NOWHERE;
    out.local = vec2<f32>(0.0);
    out.color = vec4<f32>(0.0);
    let t0 = splat_texel(index, 0u);
    let t1 = splat_texel(index, 1u);
    let t2 = splat_texel(index, 2u);
    let rgba = unpack4x8unorm(t0.w);
    if (rgba.a < 1.0 / 255.0) {
        return out;
    }
    let position = bitcast<vec3<f32>>(t0.xyz);
    let world = (u.model * vec4<f32>(position, 1.0)).xyz;
    let clip = u.view_proj * vec4<f32>(world, 1.0);
    if (clip.w <= 0.0 || clipped(world)) {
        return out;
    }
    let ndc = clip.xy / clip.w;
    if (any(abs(ndc) > vec2<f32>(1.3)) || clip.z / clip.w > 1.0) {
        return out;
    }
    // the covariance in world axes
    let c = bitcast<vec4<f32>>(t1);
    let e = bitcast<vec2<f32>>(t2.xy);
    let sigma = mat3x3<f32>(vec3<f32>(c.x, c.y, c.z), vec3<f32>(c.y, c.w, e.x), vec3<f32>(c.z, e.x, e.y));
    let m = mat3x3<f32>(u.model[0].xyz, u.model[1].xyz, u.model[2].xyz);
    let s = m * sigma * transpose(m);
    // how a world step moves the centre on screen, in pixels: the derivative of the projection
    // (perspective or parallel alike)
    let vp = u.view_proj;
    let rx = vec3<f32>(vp[0].x, vp[1].x, vp[2].x);
    let ry = vec3<f32>(vp[0].y, vp[1].y, vp[2].y);
    let rw = vec3<f32>(vp[0].w, vp[1].w, vp[2].w);
    let half_view = 0.5 * su.view.xy;
    let jx = (rx * clip.w - clip.x * rw) / (clip.w * clip.w) * half_view.x;
    let jy = (ry * clip.w - clip.y * rw) / (clip.w * clip.w) * half_view.y;
    // the covariance on screen, widened by a little over a pixel so that no splat falls between
    // the pixels
    let a = dot(jx, s * jx) + 0.3;
    let b = dot(jx, s * jy);
    let d = dot(jy, s * jy) + 0.3;
    let mid = 0.5 * (a + d);
    let radius = length(vec2<f32>(0.5 * (a - d), b));
    let major = mid + radius;
    let minor = max(mid - radius, 0.1);
    var axis = vec2<f32>(b, major - a);
    if (dot(axis, axis) < 1e-12) {
        axis = select(vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), a >= d);
    }
    axis = normalize(axis);
    let across = vec2<f32>(-axis.y, axis.x);
    let r1 = min(3.0 * sqrt(major), 2048.0);
    let r2 = min(3.0 * sqrt(minor), 2048.0);
    // strip order: (-1, -1), (1, -1), (-1, 1), (1, 1)
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u)) * 2.0 - 1.0;
    let offset = corner.x * r1 * axis + corner.y * r2 * across;
    out.clip = vec4<f32>((ndc + offset / half_view) * clip.w, clip.z, clip.w);
    out.local = corner * 3.0;
    // the colour, with its view-dependent part seen from the eye, in the coefficients' axes
    let view_dir = normalize(world - u.eye.xyz);
    let local_dir = transpose(m) * view_dir;
    let sh_dir = normalize(vec3<f32>(dot(su.frame_a.xyz, local_dir), dot(su.frame_b.xyz, local_dir), dot(su.frame_c.xyz, local_dir)));
    let color = max(rgba.rgb + sh_color(index, sh_dir), vec3<f32>(0.0));
    out.color = vec4<f32>(select(to_linear(min(color, vec3<f32>(1.0))), color, su.frame_b.w > 0.5), rgba.a);
    return out;
}

@fragment
fn fs_splat(in: SplatOut) -> @location(0) vec4<f32> {
    let alpha = in.color.a * exp(-0.5 * dot(in.local, in.local));
    if (alpha < 1.0 / 255.0) {
        discard;
    }
    return vec4<f32>(encode(in.color.rgb) * alpha, alpha);
}
