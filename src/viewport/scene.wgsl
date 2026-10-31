// Scene shader: lit triangles, unlit lines, and the blit of the multisampled frame into iced's target.

struct Uniforms {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    eye: vec4<f32>,
    // x: 1 = target is sRGB (hardware encodes), 0 = encode in the shader
    // y: colour multiplier for lines (wireframe darkening)
    params: vec4<f32>,
    // section box: pivot.xyz + cos(turn), min.xyz + sin(turn), max.xyz + on (w > 0.5)
    clip_a: vec4<f32>,
    clip_b: vec4<f32>,
    clip_c: vec4<f32>,
    // cut fill: colour (sRGB) + hatch spacing in pixels (0 = solid)
    cap: vec4<f32>,
    // section plane: normal.xyz + offset (cuts normal · p > offset; on when the normal is not
    // zero), and the middle of its fill square + half size
    plane_a: vec4<f32>,
    plane_b: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
// exploded view: the offset (world) of every unit, row by row in a float texture (WebGL2 has no
// storage buffers); unit 0 never moves
@group(0) @binding(1) var offsets: texture_2d<f32>;
const OFFSETS_WIDTH: u32 = 1024u;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) unit: u32,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

// Where a vertex is drawn: turned by the model matrix, then moved with its unit.
fn place(v: VertexIn) -> vec4<f32> {
    return u.model * vec4<f32>(v.position, 1.0) + vec4<f32>(textureLoad(offsets, vec2<i32>(i32(v.unit % OFFSETS_WIDTH), i32(v.unit / OFFSETS_WIDTH)), 0).xyz, 0.0);
}

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    var out: VertexOut;
    let world = place(v);
    out.world = world.xyz;
    out.normal = (u.model * vec4<f32>(v.normal, 0.0)).xyz;
    out.color = v.color;
    out.clip = u.view_proj * world;
    return out;
}

// A world point in the frame of the section box (turned back about the vertical through the
// pivot).
fn box_local(world: vec3<f32>) -> vec3<f32> {
    let d = world - u.clip_a.xyz;
    let c = u.clip_a.w;
    let s = u.clip_b.w;
    return u.clip_a.xyz + vec3<f32>(c * d.x + s * d.y, -s * d.x + c * d.y, d.z);
}

// Outside the section box (when it is on).
fn box_clipped(world: vec3<f32>) -> bool {
    if (u.clip_c.w < 0.5) {
        return false;
    }
    let p = box_local(world);
    return any(p < u.clip_b.xyz) || any(p > u.clip_c.xyz);
}

// On the side the section plane cuts away (when it is on).
fn plane_clipped(world: vec3<f32>) -> bool {
    return dot(u.plane_a.xyz, u.plane_a.xyz) > 0.5 && dot(u.plane_a.xyz, world) > u.plane_a.w;
}

fn clipped(world: vec3<f32>) -> bool {
    return box_clipped(world) || plane_clipped(world);
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

// Lit colour in linear space.
fn shade(in: VertexOut) -> vec3<f32> {
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
    return base * light;
}

@fragment
fn fs_lit(in: VertexOut) -> @location(0) vec4<f32> {
    // transparent materials (glass) are drawn in the glass pass
    if (clipped(in.world) || in.color.a < 0.99) {
        discard;
    }
    return vec4<f32>(encode(shade(in)), 1.0);
}

// Transparent materials after everything opaque, blended, without writing depth; at least a
// trace of the surface stays visible.
@fragment
fn fs_glass(in: VertexOut) -> @location(0) vec4<f32> {
    if (clipped(in.world) || in.color.a >= 0.99) {
        discard;
    }
    return vec4<f32>(encode(shade(in)), max(in.color.a, 0.12));
}

// Selected elements: the lit colour pulled towards the OpenAEC amber (#D97706).
@fragment
fn fs_highlight(in: VertexOut) -> @location(0) vec4<f32> {
    if (clipped(in.world)) {
        discard;
    }
    let amber = to_linear(vec3<f32>(0.851, 0.467, 0.024));
    let lit = shade(in);
    let glow = 0.6 + 0.4 * clamp(dot(lit, vec3<f32>(0.333)), 0.0, 1.0);
    return vec4<f32>(encode(mix(lit, amber * glow, 0.6)), 1.0);
}

@fragment
fn fs_line(in: VertexOut) -> @location(0) vec4<f32> {
    if (clipped(in.world)) {
        discard;
    }
    return vec4<f32>(encode(to_linear(in.color.rgb) * u.params.y), in.color.a);
}

// ---- cut fill of the section box ----------------------------------------------------------------
// Per box face (instance index 0..6: axis * 2 + max): the stencil is cleared, every surface on the
// side of the face's plane that the box keeps inverts it, and the face is filled where the count is
// odd, that is where the plane runs inside a closed solid.

struct FaceOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) @interpolate(flat) face: u32,
};

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32((vi << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(vi & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_none() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0);
}

@vertex
fn vs_face(v: VertexIn, @builtin(instance_index) face: u32) -> FaceOut {
    var out: FaceOut;
    let world = place(v);
    out.world = world.xyz;
    out.face = face;
    out.clip = u.view_proj * world;
    return out;
}

@fragment
fn fs_face_count(in: FaceOut) -> @location(0) vec4<f32> {
    // face 6: the section plane
    if (in.face == 6u) {
        if (plane_clipped(in.world)) {
            discard;
        }
        return vec4<f32>(0.0);
    }
    let p = box_local(in.world);
    let axis = in.face / 2u;
    var kept = p[axis] >= u.clip_b[axis];
    if (in.face % 2u == 1u) {
        kept = p[axis] <= u.clip_c[axis];
    }
    if (!kept) {
        discard;
    }
    return vec4<f32>(0.0);
}

@vertex
fn vs_cap(@builtin(vertex_index) vi: u32, @builtin(instance_index) face: u32) -> FaceOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0),
    );
    let t = corners[vi];
    if (face == 6u) {
        // a square on the section plane round the model, axes as `SectionPlane::basis`
        let n = u.plane_a.xyz;
        var up = vec3<f32>(0.0, 0.0, 1.0);
        if (abs(n.z) >= 0.99) {
            up = vec3<f32>(1.0, 0.0, 0.0);
        }
        let a = normalize(cross(up, n));
        let b = normalize(cross(n, a));
        let world = u.plane_b.xyz + (a * (2.0 * t.x - 1.0) + b * (2.0 * t.y - 1.0)) * u.plane_b.w;
        var plane_out: FaceOut;
        plane_out.world = world;
        plane_out.face = face;
        plane_out.clip = u.view_proj * vec4<f32>(world, 1.0);
        return plane_out;
    }
    let axis = face / 2u;
    let b = (axis + 1u) % 3u;
    let c = (axis + 2u) % 3u;
    let lo = u.clip_b.xyz;
    let hi = u.clip_c.xyz;
    var p = lo;
    p[axis] = select(lo[axis], hi[axis], face % 2u == 1u);
    p[b] = mix(lo[b], hi[b], t.x);
    p[c] = mix(lo[c], hi[c], t.y);
    // box frame to world
    let d = p - u.clip_a.xyz;
    let cs = u.clip_a.w;
    let sn = u.clip_b.w;
    let world = u.clip_a.xyz + vec3<f32>(cs * d.x - sn * d.y, sn * d.x + cs * d.y, d.z);
    var out: FaceOut;
    out.world = world;
    out.face = face;
    out.clip = u.view_proj * vec4<f32>(world, 1.0);
    return out;
}

@fragment
fn fs_cap(in: FaceOut) -> @location(0) vec4<f32> {
    // a box face is cut by the plane, the plane by the box
    if ((in.face == 6u && box_clipped(in.world)) || (in.face < 6u && plane_clipped(in.world))) {
        discard;
    }
    var color = to_linear(u.cap.rgb);
    if (u.cap.w > 0.0) {
        // diagonal hatch in screen space, as a cut in a section drawing
        let k = fract((in.clip.x + in.clip.y) / u.cap.w);
        if (k < 0.18) {
            color = color * 0.45;
        }
    }
    return vec4<f32>(encode(color), 1.0);
}
