// Structural pipelines: wide segments (member axes) and glyphs (nodes) at screen size. Appended to
// scene.wgsl, whose uniforms (model matrix, section box and plane, the offsets of the exploded
// view) stay at @group(0); the structural state is a separate uniform buffer at @group(1). WebGL2:
// no storage buffers, and instances arrive through vertex buffers bound at an offset, never through
// a first instance.

struct StructUniforms {
    // width_px, height_px, 1 / width, 1 / height; every size here is in logical pixels, so
    // lines and glyphs keep their size on a high-density screen
    viewport: vec4<f32>,
    // line_px, glyph_px, hinge_setback_px, selected_line_px
    sizes: vec4<f32>,
    // diagram scale, load scale, deformation factor, proj11
    scales: vec4<f32>,
    // min_glyph_px, arrow_spacing_px, node_cluster_px, diagram side (+1 / -1)
    lod: vec4<f32>,
    // axis, node, support, hinge, load, diagram +, diagram -, deformed (sRGB); the last one's rgb
    // is the hover tint
    colors: array<vec4<f32>, 8>,
};

@group(1) @binding(0) var<uniform> su: StructUniforms;

// Everything not drawn goes to `NOWHERE` (scene.wgsl): outside the view.
// Segments are clipped against this clip w before they are spread on screen (`pick::NEAR_W`).
const NEAR_W: f32 = 1e-5;
const GLYPH_NODE: u32 = 0u;
// How much larger a marked (selected or hovered) glyph is drawn, to cover the plain one.
const MARK_GROW: f32 = 1.3;

// The world offset of a unit of the exploded view.
fn unit_offset(unit: u32) -> vec3<f32> {
    return textureLoad(offsets, vec2<i32>(i32(unit % OFFSETS_WIDTH), i32(unit / OFFSETS_WIDTH)), 0).xyz;
}

fn mark_color(mark: u32, color: vec4<f32>) -> vec4<f32> {
    // 1: selected (the OpenAEC amber, as fs_highlight); 2: hovered
    if (mark == 1u) {
        return vec4<f32>(0.851, 0.467, 0.024, 1.0);
    }
    if (mark == 2u) {
        return vec4<f32>(mix(color.rgb, su.colors[7].rgb, 0.65), 1.0);
    }
    return color;
}

// ---- seg: wide segments ----------------------------------------------------------------------------

struct SegIn {
    @location(0) a: vec3<f32>,
    @location(1) a_off: vec3<f32>,
    @location(2) b: vec3<f32>,
    @location(3) b_off: vec3<f32>,
    @location(4) color: vec4<f32>,
    @location(5) unit: u32,
    @location(6) style: u32,
};

fn style_scale(style: u32) -> f32 {
    let k = style & 3u;
    if (k == 1u) {
        return su.scales.x;
    }
    if (k == 2u) {
        return su.scales.y;
    }
    if (k == 3u) {
        return su.scales.z;
    }
    return 1.0;
}

// Corner `vi` (strip order: a-left, b-left, a-right, b-right) of the quad of segment `s`, a line
// width wide, with half a width of cap at both ends so joints close. Its depth is the line's,
// whatever the width.
fn seg_vertex(s: SegIn, vi: u32, mark: u32) -> VertexOut {
    var out: VertexOut;
    let k = style_scale(s.style);
    let shift = unit_offset(s.unit);
    var wa = (u.model * vec4<f32>(s.a + s.a_off * k, 1.0)).xyz + shift;
    var wb = (u.model * vec4<f32>(s.b + s.b_off * k, 1.0)).xyz + shift;
    var ca = u.view_proj * vec4<f32>(wa, 1.0);
    var cb = u.view_proj * vec4<f32>(wb, 1.0);
    out.normal = vec3<f32>(0.0);
    out.color = mark_color(mark, select(su.colors[0], s.color, s.color.a > 0.0));
    if (ca.w < NEAR_W && cb.w < NEAR_W) {
        out.clip = NOWHERE;
        out.world = wa;
        return out;
    }
    // the part in front of the camera
    if (ca.w < NEAR_W) {
        let t = (NEAR_W - ca.w) / (cb.w - ca.w);
        ca = mix(ca, cb, t);
        wa = mix(wa, wb, t);
    }
    if (cb.w < NEAR_W) {
        let t = (NEAR_W - cb.w) / (ca.w - cb.w);
        cb = mix(cb, ca, t);
        wb = mix(wb, wa, t);
    }
    let half_size = su.viewport.xy * 0.5;
    let pa = ca.xy / ca.w * half_size;
    let pb = cb.xy / cb.w * half_size;
    var d = pb - pa;
    let len = length(d);
    if (len < 1e-6) {
        d = vec2<f32>(1.0, 0.0);
    } else {
        d = d / len;
    }
    let n = vec2<f32>(-d.y, d.x);
    let at_b = (vi & 1u) == 1u;
    let side = f32((vi >> 1u) & 1u) * 2.0 - 1.0;
    var mul = f32((s.style >> 8u) & 255u) / 16.0;
    if (mul <= 0.0) {
        mul = 1.0;
    }
    var width = su.sizes.x * mul;
    if (mark == 1u) {
        width = max(su.sizes.w * mul, width + 2.0);
    } else if (mark == 2u) {
        width = width + 2.0;
    }
    let along = select(-1.0, 1.0, at_b);
    let cap = 0.5 * width;
    let px = (n * side + d * along) * cap;
    var c = select(ca, cb, at_b);
    // The depth of the line itself, carried on along it over the cap, so the quad's depth at a
    // pixel does not depend on its width: a selected or hovered member drawn wider on top meets
    // the depth its plain line wrote, along the whole length (window depth is linear on screen).
    let za = ca.z / ca.w;
    let zb = cb.z / cb.w;
    var z = select(za, zb, at_b);
    if (len >= 1e-6) {
        z = z + (zb - za) * along * cap / len;
    }
    c = vec4<f32>(c.xy + px * 2.0 * su.viewport.zw * c.w, z * c.w, c.w);
    out.clip = c;
    out.world = select(wa, wb, at_b);
    return out;
}

@vertex
fn vs_seg(s: SegIn, @builtin(vertex_index) vi: u32) -> VertexOut {
    return seg_vertex(s, vi, 0u);
}

@vertex
fn vs_seg_selected(s: SegIn, @builtin(vertex_index) vi: u32) -> VertexOut {
    return seg_vertex(s, vi, 1u);
}

@vertex
fn vs_seg_hovered(s: SegIn, @builtin(vertex_index) vi: u32) -> VertexOut {
    return seg_vertex(s, vi, 2u);
}

// Unlit; cut by the section box and plane where the line passes them.
@fragment
fn fs_seg(in: VertexOut) -> @location(0) vec4<f32> {
    if (clipped(in.world)) {
        discard;
    }
    return vec4<f32>(encode(to_linear(in.color.rgb)), 1.0);
}

// ---- glyph: small 3D symbols at screen size ----------------------------------------------------------

struct GlyphVertex {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

struct GlyphIn {
    @location(2) pos: vec3<f32>,
    @location(3) rot: vec4<f32>,
    @location(4) color: vec4<f32>,
    @location(5) unit: u32,
    @location(6) size: vec2<f32>,
    @location(7) param: vec2<f32>,
    @location(8) kind_lod: u32,
};

fn rotate(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    let t = 2.0 * cross(q.xyz, v);
    return v + q.w * t + cross(q.xyz, t);
}

// A vertex of the glyph mesh scaled to the glyph's size in pixels (at most its world cap), turned
// and placed at the instance. Glyphs too small to see, and nodes whose shortest member spans only a
// few pixels, are dropped; the section box and plane cut whole glyphs, by their centre.
fn glyph_vertex(v: GlyphVertex, g: GlyphIn, mark: u32) -> VertexOut {
    var out: VertexOut;
    let centre = (u.model * vec4<f32>(g.pos, 1.0)).xyz + unit_offset(g.unit);
    out.world = centre;
    out.normal = vec3<f32>(0.0);
    let kind = g.kind_lod & 255u;
    out.color = mark_color(mark, select(su.colors[1], g.color, g.color.a > 0.0));
    let cc = u.view_proj * vec4<f32>(centre, 1.0);
    if (cc.w < NEAR_W || clipped(centre)) {
        out.clip = NOWHERE;
        return out;
    }
    // world units per pixel at the instance (perspective and orthographic)
    let wpp = 2.0 * cc.w / (su.viewport.y * su.scales.w);
    let px = select(su.sizes.y, g.size.x, g.size.x > 0.0);
    var size = px * wpp;
    if (g.size.y > 0.0) {
        size = min(size, g.size.y);
    }
    let drawn_px = size / wpp;
    if (drawn_px < su.lod.x || (kind == GLYPH_NODE && g.param.x > 0.0 && g.param.x / wpp < su.lod.z)) {
        out.clip = NOWHERE;
        return out;
    }
    var grow = 1.0;
    if (mark != 0u) {
        grow = MARK_GROW;
    }
    let local = rotate(g.rot, v.position) * size * grow;
    let world = centre + (u.model * vec4<f32>(local, 0.0)).xyz;
    out.world = world;
    out.normal = (u.model * vec4<f32>(rotate(g.rot, v.normal), 0.0)).xyz;
    out.clip = u.view_proj * vec4<f32>(world, 1.0);
    return out;
}

@vertex
fn vs_glyph(v: GlyphVertex, g: GlyphIn) -> VertexOut {
    return glyph_vertex(v, g, 0u);
}

@vertex
fn vs_glyph_selected(v: GlyphVertex, g: GlyphIn) -> VertexOut {
    return glyph_vertex(v, g, 1u);
}

@vertex
fn vs_glyph_hovered(v: GlyphVertex, g: GlyphIn) -> VertexOut {
    return glyph_vertex(v, g, 2u);
}

// Lit by a headlight, so a glyph reads the same from every side.
@fragment
fn fs_glyph(in: VertexOut) -> @location(0) vec4<f32> {
    let view_dir = normalize(u.eye.xyz - in.world);
    var n = in.normal;
    if (length(n) < 1e-6) {
        n = view_dir;
    }
    let light = 0.35 + 0.65 * abs(dot(normalize(n), view_dir));
    return vec4<f32>(encode(to_linear(in.color.rgb) * light), 1.0);
}
