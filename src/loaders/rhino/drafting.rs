//! Hatches and text of Rhino models, as openNURBS writes them.
//!
//! A hatch (`ON_Hatch`) uses a hatch pattern (`ON_HatchPattern`). A solid (or gradient) hatch is
//! a filled face, its outer loops less its inner ones; a hatch of lines is its pattern's
//! families of parallel lines (each at its angle, through its base point, the lines an offset
//! apart, dashed by its dashes), turned and scaled by the hatch and clipped to its loops.
//!
//! A text (`ON_Text`, Rhino 6 and later) is its plain text (from the rich text it is stored as),
//! set in the viewer's own typeface at the height of its dimension style (or the override it
//! carries), aligned and turned as stored, filled in its plane.

use super::tessellate::{polyline, read_curve_object};
use super::{err, Cur, Mesh, Uuid, R, TCODE_ANONYMOUS_CHUNK, TCODE_MODEL_ATTRIBUTES_CHUNK};
use glam::{DVec2, DVec3};

pub(super) const ON_HATCH: Uuid = Uuid::parse("0559733B-5332-49d1-A936-0532AC76ADE5");
pub(super) const ON_HATCH_PATTERN: Uuid = Uuid::parse("064E7C91-35F6-4734-A446-79FF7CD659E1");
pub(super) const ON_TEXT: Uuid = Uuid::parse("57376349-62A9-4A16-B411-A46BCD544790");
pub(super) const ON_DIM_STYLE: Uuid = Uuid::parse("67AA51A5-791D-4BEC-8AED-D23B462B6F87");

/// Most characters of one text drawn.
const MAX_TEXT: usize = 10_000;

/// Most lines one family of a pattern draws in one hatch: a tiny spacing over a large hatch
/// would otherwise draw without end.
const MAX_PATTERN_LINES: usize = 20_000;

/// A family of parallel lines of a hatch pattern (`ON_HatchLine`).
#[derive(Debug, Clone)]
pub(super) struct PatternLine {
    angle: f64,
    base: DVec2,
    offset: DVec2,
    /// Dash lengths: positive drawn, negative a gap; empty for a continuous line.
    dashes: Vec<f64>,
}

/// A hatch pattern: its index (as hatches refer to it), whether it is filled, its lines.
#[derive(Debug, Clone)]
pub(super) struct HatchPattern {
    pub(super) index: i32,
    pub(super) solid: bool,
    lines: Vec<PatternLine>,
}

/// A hatch: its plane (origin, x and y axes), pattern, scale and turn, and its loops (outer
/// first) as polygons in the plane.
#[derive(Debug, Clone)]
pub(super) struct Hatch {
    origin: DVec3,
    x: DVec3,
    y: DVec3,
    pattern: i32,
    scale: f64,
    rotation: f64,
    loops: Vec<(bool, Vec<DVec2>)>,
}

fn read_line(c: &mut Cur<'_>) -> R<PatternLine> {
    let angle = c.f64()?;
    let base = DVec2::new(c.f64()?, c.f64()?);
    let offset = DVec2::new(c.f64()?, c.f64()?);
    let n = c.count(8)?;
    let dashes = (0..n).map(|_| c.f64()).collect::<R<Vec<_>>>()?;
    Ok(PatternLine { angle, base, offset, dashes })
}

/// `ON_HatchPattern::Read` (Rhino 6 and later) and `ReadV5`; `position` is its place in the
/// table, the index Rhino 6 and later refer to it by.
pub(super) fn read_pattern(mut c: Cur<'_>, position: i32) -> R<HatchPattern> {
    let v5 = c.v.archive < 60 || (c.v.archive == 60 && c.v.opennurbs < 2_348_833_956);
    let mut pattern = HatchPattern { index: position, solid: true, lines: Vec::new() };
    if v5 {
        let (major, _) = c.version()?;
        if major != 1 {
            return err("a hatch pattern of an unknown version");
        }
        pattern.index = c.i32()?;
        let fill = c.u32()?;
        c.wstring()?; // name
        c.wstring()?; // description
        pattern.solid = fill != 1;
        if fill == 1 {
            let n = c.count(4)?;
            for _ in 0..n {
                c.version()?;
                pattern.lines.push(read_line(&mut c)?);
            }
        }
        return Ok(pattern);
    }
    let (mut b, major, _) = c.versioned(TCODE_ANONYMOUS_CHUNK)?;
    if major != 1 {
        return err("a hatch pattern of an unknown version");
    }
    b.chunk()?; // model component attributes
    let fill = b.u32()?;
    b.wstring()?; // description
    pattern.solid = fill != 1;
    let mut lines = b.chunk()?.body;
    let n = lines.count(4)?;
    for _ in 0..n {
        let (mut line, _, _) = lines.versioned(TCODE_ANONYMOUS_CHUNK)?;
        pattern.lines.push(read_line(&mut line)?);
    }
    Ok(pattern)
}

/// `ON_Hatch::Read`.
pub(super) fn read_hatch(c: &mut Cur<'_>) -> R<Hatch> {
    let (major, _) = c.version()?;
    if major != 1 {
        return err("a hatch of an unknown version");
    }
    let point = |c: &mut Cur<'_>| -> R<DVec3> { Ok(DVec3::new(c.f64()?, c.f64()?, c.f64()?)) };
    let origin = point(c)?;
    let x = point(c)?;
    let y = point(c)?;
    c.take(24 + 32)?; // z axis, equation
    let scale = c.f64()?;
    let rotation = c.f64()?;
    let pattern = c.i32()?;
    let n = c.count(5)?;
    let mut loops = Vec::with_capacity(n);
    for _ in 0..n {
        c.version()?;
        let outer = c.i32()? == 0;
        if let Some(curve) = read_curve_object(c, 0)? {
            let ring: Vec<DVec2> = polyline(&curve).into_iter().map(|p| p.truncate()).collect();
            if ring.len() >= 3 && ring.iter().all(|p| p.is_finite()) {
                loops.push((outer, ring));
            }
        }
    }
    if !(origin.is_finite() && x.is_finite() && y.is_finite()) {
        return err("a hatch with an invalid plane");
    }
    Ok(Hatch { origin, x, y, pattern, scale: if scale.is_finite() && scale > 0.0 { scale } else { 1.0 }, rotation: if rotation.is_finite() { rotation } else { 0.0 }, loops })
}

fn inside(loops: &[(bool, Vec<DVec2>)], p: DVec2) -> bool {
    let mut odd = false;
    for (_, ring) in loops {
        let n = ring.len();
        for i in 0..n {
            let (a, b) = (ring[i], ring[(i + 1) % n]);
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                odd = !odd;
            }
        }
    }
    odd
}

impl Hatch {
    /// The index of its hatch pattern.
    pub(super) fn pattern(&self) -> i32 {
        self.pattern
    }

    fn place(&self, p: DVec2) -> DVec3 {
        self.origin + p.x * self.x + p.y * self.y
    }

    /// A filled hatch: each outer loop with the inner loops inside it, triangulated, on the
    /// hatch's plane.
    pub(super) fn fill(&self) -> Option<Mesh> {
        let normal = self.x.cross(self.y).normalize_or_zero();
        let mut mesh = Mesh::default();
        let mut positions: Vec<[f32; 3]> = Vec::new();
        for (k, (outer, ring)) in self.loops.iter().enumerate() {
            if !outer {
                continue;
            }
            let holes: Vec<&Vec<DVec2>> = self.loops.iter().enumerate().filter(|&(j, (o, r))| j != k && !o && inside(&[(true, ring.clone())], r[0])).map(|(_, (_, r))| r).collect();
            let mut flat: Vec<f64> = ring.iter().flat_map(|p| [p.x, p.y]).collect();
            let mut starts = Vec::new();
            for h in &holes {
                starts.push(flat.len() / 2);
                flat.extend(h.iter().flat_map(|p| [p.x, p.y]));
            }
            let Ok(tris) = earcutr::earcut(&flat, &starts, 2) else { continue };
            let base = positions.len() as u32;
            for p in flat.chunks_exact(2) {
                positions.push(self.place(DVec2::new(p[0], p[1])).to_array().map(|v| v as f32));
            }
            for t in tris.chunks_exact(3) {
                let [a, b, c] = [t[0], t[1], t[2]].map(|i| base + i as u32);
                mesh.faces.push([a, b, c, c]);
            }
        }
        if mesh.faces.is_empty() || positions.iter().flatten().any(|v| !v.is_finite()) {
            return None;
        }
        mesh.triangles = mesh.faces.len();
        mesh.normals = Some(vec![normal.to_array().map(|v| v as f32); positions.len()]);
        mesh.positions = positions;
        Some(mesh)
    }

    /// A hatch of lines: the pattern's line families across the loops' box, cut to the inside
    /// of the loops and dashed, as segments in space.
    pub(super) fn lines(&self, pattern: &HatchPattern) -> Vec<Vec<DVec3>> {
        let mut out = Vec::new();
        let (mut lo, mut hi) = (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY));
        for p in self.loops.iter().flat_map(|(_, r)| r) {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        if !(lo.is_finite() && hi.is_finite()) {
            return out;
        }
        let edges: Vec<(DVec2, DVec2)> = self.loops.iter().flat_map(|(_, r)| (0..r.len()).map(move |i| (r[i], r[(i + 1) % r.len()]))).collect();
        let turn = |v: DVec2| DVec2::from_angle(self.rotation).rotate(v);
        for line in &pattern.lines {
            let dir = turn(DVec2::from_angle(line.angle));
            let base = turn(line.base * self.scale);
            let offset = turn(line.offset * self.scale);
            let normal = dir.perp();
            // the spacing across the lines; along them the offset shifts the dashes
            let across = offset.dot(normal);
            if !(across.abs() > 1e-12) {
                continue;
            }
            // the lines that cross the box: their steps from the base line
            let corners = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
            let reach: Vec<f64> = corners.iter().map(|c| (*c - base).dot(normal) / across).collect();
            let (k0, k1) = (reach.iter().copied().fold(f64::INFINITY, f64::min).floor(), reach.iter().copied().fold(f64::NEG_INFINITY, f64::max).ceil());
            if !(k1 - k0 < MAX_PATTERN_LINES as f64) {
                continue;
            }
            let dash_length: f64 = line.dashes.iter().map(|d| d.abs() * self.scale).sum();
            for k in k0 as i64..=k1 as i64 {
                let origin = base + offset * k as f64;
                // where the line crosses the loops, along it
                let mut hits: Vec<f64> = edges
                    .iter()
                    .filter_map(|&(a, b)| {
                        let (da, db) = ((a - origin).dot(normal), (b - origin).dot(normal));
                        ((da > 0.0) != (db > 0.0)).then(|| {
                            let p = a + (b - a) * (da / (da - db));
                            (p - origin).dot(dir)
                        })
                    })
                    .collect();
                hits.sort_by(f64::total_cmp);
                for pair in hits.chunks_exact(2) {
                    let (t0, t1) = (pair[0], pair[1]);
                    if dash_length <= 0.0 || line.dashes.iter().all(|d| *d >= 0.0) {
                        out.push(vec![self.place(origin + dir * t0), self.place(origin + dir * t1)]);
                        continue;
                    }
                    // the dashes from the line's origin, repeated
                    let mut t = (t0 / dash_length).floor() * dash_length;
                    'dashes: while t < t1 {
                        for d in &line.dashes {
                            let len = d.abs() * self.scale;
                            let (a, b) = (t.max(t0), (t + len).min(t1));
                            if *d >= 0.0 && b > a {
                                out.push(vec![self.place(origin + dir * a), self.place(origin + dir * b)]);
                            }
                            t += len;
                            if t >= t1 {
                                break 'dashes;
                            }
                            if out.len() > 50 * MAX_PATTERN_LINES {
                                return out;
                            }
                        }
                        if dash_length <= 0.0 {
                            break;
                        }
                    }
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------------------------
// Text

/// A dimension style: its id (texts refer to it by), its text height and its model space
/// scale (texts that allow it are drawn that many times their height in model views).
#[derive(Debug, Clone)]
pub(super) struct DimStyle {
    pub(super) id: Uuid,
    pub(super) height: f64,
    pub(super) scale: f64,
}

/// `ON_DimStyle::Read` (Rhino 6 and later): the model component attributes (with the id), then
/// the text height among the first numbers and the model space scale further on. `None` for
/// older files.
pub(super) fn read_dim_style(mut c: Cur<'_>) -> R<Option<DimStyle>> {
    if c.v.archive < 60 {
        return Ok(None);
    }
    let (mut b, major, _) = c.versioned(TCODE_ANONYMOUS_CHUNK)?;
    if major != 1 {
        return Ok(None);
    }
    let (mut attributes, _, _) = b.versioned(TCODE_MODEL_ATTRIBUTES_CHUNK)?;
    let bits = attributes.u32()?;
    let id = if bits & 1 != 0 { attributes.uuid()? } else { Uuid::default() };
    b.take(6 * 8)?; // extension, offset, arrow sizes, centre mark, text gap
    let height = b.f64()?;
    Ok(Some(DimStyle { id, height, scale: dim_scale(&mut b).ok().filter(|s| s.is_finite() && *s > 0.0).unwrap_or(1.0) }))
}

/// The fields of `ON_DimStyle::Read` after the text height, up to its model space scale.
fn dim_scale(b: &mut Cur<'_>) -> R<f64> {
    for _ in 0..6 {
        b.u32()?; // text alignment, angle format, length format, resolutions, text style
    }
    b.f64()?; // length factor
    b.bool()?; // alternate units
    b.f64()?; // their length factor
    b.u32()?; // their format
    b.u32()?; // their resolution
    for _ in 0..4 {
        b.wstring()?; // prefix and suffix, for both
    }
    b.f64()?; // extension
    b.bool()?; // suppress extension 1
    b.bool()?; // and 2
    b.uuid()?; // parent style
    b.u32()?; // unused
    if b.bool()? {
        let n = b.count(1)?;
        b.take(n)?; // which fields override the parent's
    }
    b.u32()?; // tolerance format
    b.u32()?; // its resolution
    b.take(4 * 8)?; // its upper and lower values and height, baseline spacing
    b.bool()?; // text mask
    b.u32()?; // its type
    b.u32()?; // its colour
    b.f64()
}

/// A text: its plain text, plane, height (when it overrides its style's), style, rotation and
/// alignment.
#[derive(Debug, Clone)]
pub(super) struct Text {
    text: String,
    origin: DVec3,
    x: DVec3,
    y: DVec3,
    style: Uuid,
    height: Option<f64>,
    /// Its own model space scale (when it overrides its style's); `Some(1.0)` when it does not
    /// allow scaling.
    scale: Option<f64>,
    rotation: f64,
    /// 0 left, 1 centre, 2 right.
    h_align: u32,
    /// 0 top, 1 and 2 middle and bottom of the top line, 3 middle, 4 to 6 bottom.
    v_align: u32,
}

/// `ON_Text::Read`: `ON_Annotation::Internal_ReadAnnotation` with its `ON_TextContent`.
pub(super) fn read_text(c: &mut Cur<'_>) -> R<Text> {
    let (mut outer, _, _) = c.versioned(TCODE_ANONYMOUS_CHUNK)?;
    let (mut a, _, version) = outer.versioned(TCODE_ANONYMOUS_CHUNK)?;
    let (mut content, _, _) = a.versioned(TCODE_ANONYMOUS_CHUNK)?;
    let rich = content.wstring()?;
    content.take(4 * 24 + 32)?; // the obsolete plane
    content.f64()?; // rectangle width
    let rotation = content.f64()?;
    let h_align = content.u32()?;
    let v_align = content.u32()?;
    let style = a.uuid()?;
    let point = |c: &mut Cur<'_>| -> R<DVec3> { Ok(DVec3::new(c.f64()?, c.f64()?, c.f64()?)) };
    let origin = point(&mut a)?;
    let x = point(&mut a)?;
    let y = point(&mut a)?;
    a.take(24 + 32)?; // z axis, equation
    let mut height = None;
    let mut scale = None;
    if version >= 1 {
        a.u32()?; // annotation type
    }
    if version >= 2 {
        // a dimension style of its own
        let (mut overrides, _, _) = a.versioned(TCODE_ANONYMOUS_CHUNK)?;
        if overrides.bool()? {
            if let Some(o) = overrides.object()?.filter(|o| o.class == ON_DIM_STYLE) {
                if let Some(d) = read_dim_style(o.data).ok().flatten() {
                    height = Some(d.height).filter(|h| h.is_finite() && *h > 0.0);
                    scale = Some(d.scale);
                }
            }
        }
    }
    if version >= 4 {
        a.take(16)?; // horizontal direction (2d)
        if !a.bool()? {
            scale = Some(1.0);
        }
    }
    if !(origin.is_finite() && x.is_finite() && y.is_finite()) {
        return err("a text with an invalid plane");
    }
    Ok(Text { text: plain_text(&rich), origin, x, y, style, height, scale, rotation: if rotation.is_finite() { rotation } else { 0.0 }, h_align, v_align })
}

/// The plain text of rich text (RTF) as Rhino stores it: groups that hold no text (font and
/// colour tables, ..) left out, control words dropped, `\par` a new line, `\'hh` and `\uN` as
/// their characters. Text that is not RTF is itself.
pub(super) fn plain_text(rich: &str) -> String {
    if !rich.trim_start().starts_with("{\\rtf") {
        return rich.to_string();
    }
    let chars: Vec<char> = rich.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    // whether the text of each open group is left out
    let mut skip: Vec<bool> = vec![false];
    let mut unicode_skip = 0;
    let hidden = |skip: &Vec<bool>| skip.last().copied().unwrap_or(false);
    while i < chars.len() {
        let c = chars[i];
        match c {
            '{' => {
                let parent = hidden(&skip);
                let rest: String = chars[i + 1..chars.len().min(i + 12)].iter().collect();
                let table = ["\\fonttbl", "\\colortbl", "\\stylesheet", "\\info", "\\*"].iter().any(|d| rest.starts_with(d));
                skip.push(parent || table);
                i += 1;
            }
            '}' => {
                skip.pop();
                if skip.is_empty() {
                    skip.push(false);
                }
                i += 1;
            }
            '\\' => {
                let next = chars.get(i + 1).copied().unwrap_or(' ');
                if next == '\\' || next == '{' || next == '}' {
                    if !hidden(&skip) {
                        out.push(next);
                    }
                    i += 2;
                    continue;
                }
                if next == '\'' {
                    let hex: String = chars[(i + 2).min(chars.len())..chars.len().min(i + 4)].iter().collect();
                    if let Ok(v) = u8::from_str_radix(&hex, 16) {
                        if hidden(&skip) {
                        } else if unicode_skip > 0 {
                            unicode_skip -= 1;
                        } else {
                            out.push(v as char);
                        }
                    }
                    i += 4;
                    continue;
                }
                // a control word and its number
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_ascii_alphabetic() {
                    j += 1;
                }
                let word: String = chars[i + 1..j].iter().collect();
                let start = j;
                if j < chars.len() && chars[j] == '-' {
                    j += 1;
                }
                while j < chars.len() && chars[j].is_ascii_digit() {
                    j += 1;
                }
                let number: Option<i32> = chars[start..j].iter().collect::<String>().parse().ok();
                if j < chars.len() && chars[j] == ' ' {
                    j += 1;
                }
                if !hidden(&skip) {
                    match word.as_str() {
                        "par" | "line" => out.push('\n'),
                        "tab" => out.push('\t'),
                        "u" => {
                            if let Some(n) = number {
                                out.extend(char::from_u32(if n < 0 { (n + 65536) as u32 } else { n as u32 }));
                                unicode_skip = 1;
                            }
                        }
                        _ => {}
                    }
                }
                if j == i + 1 {
                    j += 1; // a control symbol
                }
                i = j;
            }
            '\r' | '\n' => i += 1,
            _ => {
                if !hidden(&skip) {
                    if unicode_skip > 0 {
                        unicode_skip -= 1;
                    } else {
                        out.push(c);
                    }
                }
                i += 1;
            }
        }
    }
    out.trim_end_matches('\n').to_string()
}

/// Flattens a glyph's outline into closed polygons.
struct Outline {
    rings: Vec<Vec<DVec2>>,
    at: DVec2,
}

impl Outline {
    fn to(&mut self, p: DVec2) {
        self.at = p;
        if let Some(r) = self.rings.last_mut() {
            r.push(p);
        }
    }
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.rings.push(Vec::new());
        self.to(DVec2::new(x as f64, y as f64));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.to(DVec2::new(x as f64, y as f64));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (p0, p1, p2) = (self.at, DVec2::new(x1 as f64, y1 as f64), DVec2::new(x as f64, y as f64));
        for k in 1..=6 {
            let t = k as f64 / 6.0;
            self.to((1.0 - t) * (1.0 - t) * p0 + 2.0 * (1.0 - t) * t * p1 + t * t * p2);
        }
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (p0, p1, p2, p3) = (self.at, DVec2::new(x1 as f64, y1 as f64), DVec2::new(x2 as f64, y2 as f64), DVec2::new(x as f64, y as f64));
        for k in 1..=8 {
            let t = k as f64 / 8.0;
            let u = 1.0 - t;
            self.to(u * u * u * p0 + 3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t * p3);
        }
    }

    fn close(&mut self) {}
}

fn signed_area(r: &[DVec2]) -> f64 {
    0.5 * (0..r.len()).map(|i| r[i].perp_dot(r[(i + 1) % r.len()])).sum::<f64>()
}

impl Text {
    pub(super) fn style(&self) -> Uuid {
        self.style
    }

    /// The text filled in its plane, as high as its style's text height (the height of its
    /// capitals) times its model space scale, unless it overrides them: each glyph's outer
    /// contours less the holes inside them.
    pub(super) fn mesh(&self, style: Option<&DimStyle>) -> Option<Mesh> {
        let face = ttf_parser::Face::parse(crate::INTER_REGULAR, 0).ok()?;
        let height = self.height.or(style.map(|s| s.height)).filter(|h| h.is_finite() && *h > 0.0).unwrap_or(1.0);
        let height = height * self.scale.or(style.map(|s| s.scale)).unwrap_or(1.0);
        let cap = face.capital_height().map_or(0.7 * face.units_per_em() as f64, |c| c as f64);
        let scale = height / cap;
        let line_step = 1.6 * height;
        let lines: Vec<&str> = self.text.split('\n').collect();
        // the rings of each glyph, by line, from the start of its baseline
        let mut placed: Vec<(usize, f64, Vec<Vec<DVec2>>)> = Vec::new();
        let mut widths = vec![0.0; lines.len()];
        let mut count = 0;
        for (l, line) in lines.iter().enumerate() {
            let mut x = 0.0;
            for ch in line.chars() {
                count += 1;
                if count > MAX_TEXT {
                    break;
                }
                let ch = if ch == '\t' { ' ' } else { ch };
                let Some(glyph) = face.glyph_index(ch).or_else(|| face.glyph_index('?')) else { continue };
                let mut outline = Outline { rings: Vec::new(), at: DVec2::ZERO };
                face.outline_glyph(glyph, &mut outline);
                let rings: Vec<Vec<DVec2>> = outline.rings.into_iter().filter(|r| r.len() >= 3).map(|r| r.into_iter().map(|p| p * scale).collect()).collect();
                placed.push((l, x, rings));
                x += face.glyph_hor_advance(glyph).unwrap_or(0) as f64 * scale;
            }
            widths[l] = x;
        }
        // the block below or above its point, as aligned
        let below = (lines.len() - 1) as f64 * line_step;
        let dy = match self.v_align {
            0..=2 => -height,
            3 => (below - height) / 2.0,
            _ => below,
        };
        let (sin, cos) = self.rotation.sin_cos();
        let normal = self.x.cross(self.y).normalize_or_zero();
        let place = |p: DVec2| {
            let r = DVec2::new(p.x * cos - p.y * sin, p.x * sin + p.y * cos);
            self.origin + r.x * self.x + r.y * self.y
        };
        let mut mesh = Mesh::default();
        let mut positions: Vec<[f32; 3]> = Vec::new();
        for (l, x0, rings) in &placed {
            let dx = match self.h_align {
                1 => -widths[*l] / 2.0,
                2 => -widths[*l],
                _ => 0.0,
            };
            let offset = DVec2::new(x0 + dx, dy - *l as f64 * line_step);
            // outer contours turn as the largest one does; holes lie inside them
            let Some(largest) = rings.iter().max_by(|a, b| signed_area(a).abs().total_cmp(&signed_area(b).abs())) else { continue };
            let outer_sign = signed_area(largest).signum();
            let (outers, holes): (Vec<&Vec<DVec2>>, Vec<&Vec<DVec2>>) = rings.iter().partition(|r| signed_area(r).signum() == outer_sign);
            for outer in outers {
                let own: Vec<&Vec<DVec2>> = holes.iter().copied().filter(|h| inside(&[(true, outer.clone())], h[0])).collect();
                let mut flat: Vec<f64> = outer.iter().flat_map(|p| [p.x, p.y]).collect();
                let mut starts = Vec::new();
                for h in &own {
                    starts.push(flat.len() / 2);
                    flat.extend(h.iter().flat_map(|p| [p.x, p.y]));
                }
                let Ok(tris) = earcutr::earcut(&flat, &starts, 2) else { continue };
                let base = positions.len() as u32;
                for p in flat.chunks_exact(2) {
                    positions.push(place(DVec2::new(p[0], p[1]) + offset).to_array().map(|v| v as f32));
                }
                for t in tris.chunks_exact(3) {
                    let [a, b, c] = [t[0], t[1], t[2]].map(|i| base + i as u32);
                    mesh.faces.push([a, b, c, c]);
                }
            }
        }
        if mesh.faces.is_empty() || positions.iter().flatten().any(|v| !v.is_finite()) {
            return None;
        }
        mesh.triangles = mesh.faces.len();
        mesh.normals = Some(vec![normal.to_array().map(|v| v as f32); positions.len()]);
        mesh.positions = positions;
        Some(mesh)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(outer: bool, lo: f64, hi: f64) -> (bool, Vec<DVec2>) {
        (outer, vec![DVec2::new(lo, lo), DVec2::new(hi, lo), DVec2::new(hi, hi), DVec2::new(lo, hi)])
    }

    fn hatch(loops: Vec<(bool, Vec<DVec2>)>) -> Hatch {
        Hatch { origin: DVec3::ZERO, x: DVec3::X, y: DVec3::Y, pattern: 0, scale: 1.0, rotation: 0.0, loops }
    }

    #[test]
    fn rich_text_reads_as_plain_text() {
        assert_eq!(plain_text("{\\rtf1\\deff0{\\fonttbl{\\f0 Arial;}}\\f0\\fs20 Rhino 9\\par Tweede regel}"), "Rhino 9\nTweede regel");
        assert_eq!(plain_text("{\\rtf1 caf\\'e9 \\u8364? }"), "caf\u{e9} \u{20ac} ");
        assert_eq!(plain_text("plain"), "plain");
    }

    #[test]
    fn text_fills_its_glyphs_at_its_height() {
        let t = Text { text: "HI".into(), origin: DVec3::ZERO, x: DVec3::X, y: DVec3::Y, style: Uuid::default(), height: Some(2.0), scale: None, rotation: 0.0, h_align: 0, v_align: 5 };
        let mesh = t.mesh(None).unwrap();
        let ys: Vec<f64> = mesh.positions.iter().map(|p| p[1] as f64).collect();
        let top = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let bottom = ys.iter().copied().fold(f64::INFINITY, f64::min);
        // capitals stand on the baseline and are the text height tall
        assert!((top - bottom - 2.0).abs() < 0.05 && bottom.abs() < 0.05, "{bottom} .. {top}");
    }

    #[test]
    fn a_solid_hatch_fills_around_its_holes() {
        let mesh = hatch(vec![square(true, 0.0, 10.0), square(false, 4.0, 6.0)]).fill().unwrap();
        let p = |i: u32| DVec3::from(mesh.positions[i as usize].map(f64::from));
        let area: f64 = mesh.faces.iter().map(|f| 0.5 * (p(f[1]) - p(f[0])).cross(p(f[2]) - p(f[0])).z).sum::<f64>().abs();
        assert!((area - 96.0).abs() < 1e-9, "{area}");
    }

    #[test]
    fn hatch_lines_are_cut_to_the_loops_and_dashed() {
        // horizontal lines one apart over a 10 by 10 square with a hole of 2 by 2
        let pattern = HatchPattern { index: 0, solid: false, lines: vec![PatternLine { angle: 0.0, base: DVec2::new(0.0, 0.5), offset: DVec2::new(0.0, 1.0), dashes: Vec::new() }] };
        let h = hatch(vec![square(true, 0.0, 10.0), square(false, 4.0, 6.0)]);
        let lines = h.lines(&pattern);
        let total: f64 = lines.iter().map(|l| (l[1] - l[0]).length()).sum();
        // ten lines of 10, less two crossings of the hole of 2
        assert!((total - 96.0).abs() < 1e-9, "{total}");
        // dashed: 1 drawn, 1 gap
        let dashed = HatchPattern { index: 0, solid: false, lines: vec![PatternLine { angle: 0.0, base: DVec2::new(0.0, 0.5), offset: DVec2::new(0.0, 1.0), dashes: vec![1.0, -1.0] }] };
        let total: f64 = hatch(vec![square(true, 0.0, 10.0)]).lines(&dashed).iter().map(|l| (l[1] - l[0]).length()).sum();
        assert!((total - 50.0).abs() < 1e-9, "{total}");
    }
}
