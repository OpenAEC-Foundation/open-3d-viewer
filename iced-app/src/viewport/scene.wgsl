// Scene shader: lit triangles, unlit lines, and the blit of the multisampled frame into iced's target.

struct Uniforms {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    eye: vec4<f32>,
    // x: 1 = target is sRGB (hardware encodes), 0 = encode in the shader
    // y: colour multiplier for lines (wireframe darkening)
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    var out: VertexOut;
    let world = u.model * vec4<f32>(v.position, 1.0);
    out.world = world.xyz;
    out.normal = (u.model * vec4<f32>(v.normal, 0.0)).xyz;
    out.color = v.color;
    out.clip = u.view_proj * world;
    return out;
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn encode(c: vec3<f32>) -> vec3<f32> {
    if (u.params.x > 0.5) {
        return c;
    }
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs_lit(in: VertexOut) -> @location(0) vec4<f32> {
    let view_dir = normalize(u.eye.xyz - in.world);
    var n = in.normal;
    let len = length(n);
    if (len < 1e-6) {
        n = view_dir;
    } else {
        n = n / len;
    }
    // Two-sided: light the face that points at the camera
    if (dot(n, view_dir) < 0.0) {
        n = -n;
    }
    let base = to_linear(in.color.rgb);
    let key = normalize(vec3<f32>(0.45, -0.35, 0.82));
    let fill = normalize(vec3<f32>(-0.6, 0.5, 0.35));
    let sky = 0.5 + 0.5 * n.z;
    let light = 0.28 + 0.12 * sky
        + 0.55 * max(dot(n, key), 0.0)
        + 0.18 * max(dot(n, fill), 0.0)
        + 0.12 * max(dot(n, view_dir), 0.0);
    return vec4<f32>(encode(base * light), 1.0);
}

@fragment
fn fs_line(in: VertexOut) -> @location(0) vec4<f32> {
    return vec4<f32>(encode(to_linear(in.color.rgb) * u.params.y), in.color.a);
}
