//! MicroStation design files (`.dgn`), version 7 and earlier (Intergraph ISFF).
//!
//! Written from Intergraph's public "Intergraph Standard File Formats (Element Structure)" and the
//! clean-room notes of the DGN analysis (design file header, colour table and quaternion layout
//! were measured on sample files). MicroStation V8 files (an OLE2 container) are recognised and
//! refused with a clear message.
//!
//! The scene draws triangles only, so lines, arcs, curves and text boxes become thin ribbons: flat
//! in 2D files, two crossed ribbons in 3D files, as wide as the line weight on a scale set by the
//! drawing extent. Closed planar elements, cones, extruded solids and B-spline surfaces of 3D files
//! become faces. Each level is a layer; each top-level element (a whole cell, text node or complex
//! chain) is one pickable element.

use super::{Loaded, Loader, Phase, Progress, Source, DEFAULT_COLOR};
use crate::model::{Layer, ProjectData};
use glam::{DMat3, DQuat, DVec3, DVec4};
use std::collections::{BTreeMap, BTreeSet};

const MAX_ELEMENTS: usize = 2_000_000;
const MAX_TRIANGLES: usize = 4_000_000;
const MAX_DEPTH: usize = 16;
const OLE_SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
/// Fixed-point unit of quaternions, B-spline knots and weights: 2^31 - 1.
const UNIT: f64 = 2_147_483_647.0;

pub struct DgnLoader;

impl Loader for DgnLoader {
    fn name(&self) -> &'static str { "MicroStation DGN" }
    fn extensions(&self) -> &'static [&'static str] { &["dgn"] }
    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }
    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        progress.set(Phase::Convert);
        read(&data)
    }
}

/// Reads a whole design file.
pub fn read(data: &[u8]) -> Result<Loaded, String> {
    if data.starts_with(&OLE_SIGNATURE) {
        return Err("MicroStation DGN: this is a V8 design file (MicroStation V8 or later); only V7 design files open yet".into());
    }
    let elements = elements(data)?;
    let design = Design::new(&elements)?;
    let mut converter = Converter {
        design: &design, prims: Vec::new(), props: Vec::new(), next_id: 1,
        counts: BTreeMap::new(), omitted: BTreeMap::new(),
    };
    let mut i = 0;
    while i < elements.len() {
        i = converter.top_level(&elements, i);
    }
    converter.finish()
}

/// One element: type, level, flags and its words (including the header).
#[derive(Clone, Copy)]
struct Element<'a> {
    kind: u8,
    level: u8,
    complex: bool,
    offset: usize,
    bytes: &'a [u8],
}

impl Element<'_> {
    fn words(&self) -> usize { self.bytes.len() / 2 }
    fn word(&self, i: usize) -> u16 {
        self.bytes.get(2 * i..2 * i + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
    }
    /// 32-bit integer, high word first ("middle-endian").
    fn ulong(&self, i: usize) -> u32 { (u32::from(self.word(i)) << 16) | u32::from(self.word(i + 1)) }
    fn long(&self, i: usize) -> i32 { self.ulong(i) as i32 }
    /// VAX D-float, most significant word first.
    fn double(&self, i: usize) -> f64 {
        let u = (0..4).fold(0u64, |u, k| (u << 16) | u64::from(self.word(i + k)));
        let exponent = (u >> 55) & 0xFF;
        if exponent == 0 { return 0.0; }
        f64::from_bits((u & (1 << 63)) | ((exponent + 894) << 52) | ((u & ((1 << 55) - 1)) >> 3))
    }
    fn properties(&self) -> u16 { self.word(16) }
    fn class(&self) -> u16 { self.properties() & 0xF }
    /// The H bit: hole, orphan cell, infinite line or disjoint point string.
    fn h_bit(&self) -> bool { self.properties() & 0x8000 != 0 }
    fn color(&self) -> u8 { (self.word(17) >> 8) as u8 }
    fn weight(&self) -> u8 { ((self.word(17) >> 3) & 0x1F) as u8 }
    fn style(&self) -> u8 { (self.word(17) & 7) as u8 }
    fn graphic(&self) -> bool { !matches!(self.kind, 0 | 1 | 5 | 8 | 9 | 10 | 66) && self.words() >= 18 }
}

fn elements(data: &[u8]) -> Result<Vec<Element<'_>>, String> {
    let mut out = Vec::new();
    let mut offset = 0;
    while offset + 4 <= data.len() {
        let first = u16::from_le_bytes([data[offset], data[offset + 1]]);
        if first == 0xFFFF { break; }
        let words = usize::from(u16::from_le_bytes([data[offset + 2], data[offset + 3]])) + 2;
        let end = (offset + 2 * words).min(data.len());
        if first & 0x8000 == 0 {
            out.push(Element {
                kind: ((first >> 8) & 0x7F) as u8, level: (first & 0x3F) as u8, complex: first & 0x80 != 0,
                offset, bytes: &data[offset..end],
            });
            if out.len() > MAX_ELEMENTS { return Err("MicroStation DGN: element limit exceeded".into()); }
        }
        offset += 2 * words;
    }
    match out.first() {
        Some(tcb) if tcb.kind == 9 && tcb.level == 8 => Ok(out),
        _ => Err("MicroStation DGN: not a design file (no design file header)".into()),
    }
}

/// The design file header (TCB), units and colour table.
struct Design {
    three_d: bool,
    /// Global origin in units of resolution.
    origin: DVec3,
    metres_per_uor: f64,
    units: String,
    colors: Option<Vec<[u8; 3]>>,
}

impl Design {
    fn new(elements: &[Element]) -> Result<Self, String> {
        let tcb = elements[0];
        let three_d = tcb.bytes[0] & 0x40 != 0;
        let sub_per_master = f64::from(tcb.long(556));
        let uor_per_sub = f64::from(tcb.long(558));
        let name = |word: usize| -> String {
            tcb.bytes.get(2 * word..2 * word + 2).map(|b| b.iter().filter(|c| c.is_ascii_graphic()).map(|&c| c as char).collect())
                .unwrap_or_default()
        };
        let (master, sub) = (name(560), name(561));
        let (uor_per_master, units) = if sub_per_master > 0.0 && uor_per_sub > 0.0 {
            (sub_per_master * uor_per_sub, format!("1 {master} = {sub_per_master} {sub} = {} UOR", sub_per_master * uor_per_sub))
        } else {
            (1.0, "none in the file header (1 UOR = 1 m assumed)".to_string())
        };
        let metres_per_master = metres(&master)
            .or_else(|| metres(&sub).map(|m| m * sub_per_master.max(1.0)))
            .unwrap_or(1.0);
        let origin = DVec3::new(tcb.double(620), tcb.double(624), tcb.double(628));
        if !origin.is_finite() { return Err("MicroStation DGN: invalid global origin".into()); }
        let colors = elements.iter().find(|e| e.kind == 5 && e.level == 1 && e.bytes.len() >= 38 + 768)
            .map(|e| e.bytes[38..38 + 768].chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect());
        Ok(Design { three_d, origin, metres_per_uor: metres_per_master / uor_per_master, units, colors })
    }

    /// Design coordinates (UOR) to world metres.
    fn world(&self, p: DVec3) -> DVec3 { (p - self.origin) * self.metres_per_uor }

    /// A point of longs at word `i`.
    fn point(&self, e: &Element, i: usize) -> DVec3 {
        DVec3::new(f64::from(e.long(i)), f64::from(e.long(i + 2)), if self.three_d { f64::from(e.long(i + 4)) } else { 0.0 })
    }
    /// A point of doubles at word `i`.
    fn dpoint(&self, e: &Element, i: usize) -> DVec3 {
        DVec3::new(e.double(i), e.double(i + 4), if self.three_d { e.double(i + 8) } else { 0.0 })
    }
    fn point_words(&self) -> usize { if self.three_d { 6 } else { 4 } }
    fn dpoint_words(&self) -> usize { if self.three_d { 12 } else { 8 } }
    /// Orientation at word `i`: a 2D angle (one long) or a 3D quaternion (four longs); the columns
    /// are the element's axes in design coordinates.
    fn rotation(&self, e: &Element, i: usize) -> DMat3 {
        if self.three_d {
            let [w, x, y, z] = [0, 2, 4, 6].map(|k| f64::from(e.long(i + k)) / UNIT);
            // The file stores the inverse rotation: a text turned +30 degrees about Z has (cos 15, 0, 0, -sin 15).
            let q = DQuat::from_xyzw(-x, -y, -z, w);
            if q.length_squared() < 1e-12 { DMat3::IDENTITY } else { DMat3::from_quat(q.normalize()) }
        } else {
            DMat3::from_rotation_z(angle(e.long(i)))
        }
    }
    fn rotation_words(&self) -> usize { if self.three_d { 8 } else { 2 } }

    fn rgb(&self, index: u8) -> [u8; 3] {
        match &self.colors {
            // Slot 0 of a stored table is the background; colour c is slot c + 1.
            Some(table) => table[(usize::from(index) + 1) % 256],
            None => default_color(index),
        }
    }
}

fn metres(unit: &str) -> Option<f64> {
    Some(match unit.to_ascii_uppercase().as_str() {
        "M" | "MT" | "METER" | "METRE" => 1.0,
        "MM" => 0.001,
        "CM" => 0.01,
        "DM" => 0.1,
        "KM" => 1000.0,
        "FT" | "'" | "IF" => 0.3048,
        "SF" | "USFT" | "SFT" => 1200.0 / 3937.0,
        "IN" | "\"" => 0.0254,
        "YD" => 0.9144,
        "MI" => 1609.344,
        _ => return None,
    })
}

/// Our own palette for files without a colour table: the documented first eight colours, then
/// evenly spread hues.
fn default_color(index: u8) -> [u8; 3] {
    const FIRST: [[u8; 3]; 8] = [
        [255, 255, 255], [0, 0, 255], [0, 255, 0], [255, 0, 0], [255, 255, 0], [255, 0, 255], [255, 127, 0], [0, 255, 255],
    ];
    if let Some(color) = FIRST.get(usize::from(index)) { return *color; }
    let hue = (f64::from(index) * 137.508) % 360.0;
    let (value, saturation) = (if index % 2 == 0 { 0.85 } else { 0.6 }, 0.75);
    let c = value * saturation;
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let (r, g, b) = match (hue / 60.0) as u32 { 0 => (c, x, 0.), 1 => (x, c, 0.), 2 => (0., c, x), 3 => (0., x, c), 4 => (x, 0., c), _ => (c, 0., x) };
    [r, g, b].map(|v| ((v + value - c) * 255.0).round() as u8)
}

/// A 2D rotation angle in radians: one unit is 1/360000 degree.
fn angle(value: i32) -> f64 { (f64::from(value) / 360_000.0).to_radians() }

/// An arc sweep: sign-magnitude according to ISFF, two's complement in some writers; 0 is a full turn.
fn sweep(raw: u32) -> f64 {
    let magnitude = raw & 0x7FFF_FFFF;
    let value = if magnitude > 360 * 360_000 { raw as i32 } else if raw & 0x8000_0000 != 0 { -(magnitude as i32) } else { magnitude as i32 };
    if value == 0 { std::f64::consts::TAU } else { angle(value) }
}

/// Six characters in DEC Radix-50, two words.
fn radix50(words: [u16; 2]) -> String {
    const CHARS: &[u8; 40] = b" ABCDEFGHIJKLMNOPQRSTUVWXYZ$.%0123456789";
    words.iter().flat_map(|&w| [w / 1600, (w / 40) % 40, w % 40]).map(|c| CHARS.get(usize::from(c)).map_or('?', |&c| c as char))
        .collect::<String>().trim().to_string()
}

fn kind_name(kind: u8) -> &'static str {
    match kind {
        2 => "Cell", 3 => "Line", 4 => "Line string", 6 => "Shape", 7 => "Text node", 11 => "Curve",
        12 => "Complex chain", 14 => "Complex shape", 15 => "Ellipse", 16 => "Arc", 17 => "Text",
        18 => "Surface", 19 => "Solid", 21 => "B-spline poles", 22 => "Point string", 23 => "Cone",
        24 => "B-spline surface", 25 => "B-spline boundary", 26 => "B-spline knots", 27 => "B-spline curve",
        28 => "B-spline weights", 33 => "Dimension", 34 => "Shared cell definition", 35 => "Shared cell",
        36 => "Multi-line", 37 => "Tag", 87 => "Raster header", 88 => "Raster data",
        _ => "Unknown element",
    }
}

enum Shape {
    /// A polyline, drawn as a ribbon; one point is a point marker.
    Stroke(Vec<DVec3>),
    Faces(Vec<[DVec3; 3]>),
}

/// Geometry of one element in world metres.
struct Prim {
    id: u64,
    level: u8,
    color: [u8; 3],
    weight: u8,
    shape: Shape,
}

struct Converter<'a> {
    design: &'a Design,
    prims: Vec<Prim>,
    props: Vec<(u64, u8, String)>,
    next_id: u64,
    counts: BTreeMap<&'static str, usize>,
    omitted: BTreeMap<&'static str, usize>,
}

impl Converter<'_> {
    fn omit(&mut self, what: &'static str) { *self.omitted.entry(what).or_default() += 1; }

    /// Converts the top-level element at `i` with its components; returns the next element.
    fn top_level(&mut self, all: &[Element], i: usize) -> usize {
        let e = all[i];
        let next = span(all, i, 0);
        if !e.graphic() { return next; }
        let id = self.next_id;
        self.next_id += 1;
        let before = self.prims.len();
        let mut texts = Vec::new();
        self.draw(all, i, id, 0, &mut texts);
        if self.prims.len() == before { return next; }
        *self.counts.entry(kind_name(e.kind)).or_default() += 1;
        let mut props = serde_json::json!({
            "DGN element": kind_name(e.kind), "Level": e.level, "Colour index": e.color(),
            "Line weight": e.weight(), "Line style": e.style(), "Class": e.class(),
        });
        if e.word(14) != 0 { props["Graphic group"] = e.word(14).into(); }
        if e.kind == 2 { props["Cell"] = radix50([e.word(19), e.word(20)]).into(); }
        if !texts.is_empty() { props["Text"] = texts.join("\n").into(); }
        self.props.push((id, e.level, props.to_string()));
        next
    }

    fn push(&mut self, e: &Element, id: u64, shape: Shape) {
        let color = self.design.rgb(e.color());
        self.prims.push(Prim { id, level: e.level, color, weight: e.weight(), shape });
    }

    fn stroke(&mut self, e: &Element, id: u64, points: Vec<DVec3>) {
        let points: Vec<DVec3> = points.into_iter().map(|p| self.design.world(p)).collect();
        if !points.is_empty() && points.iter().all(|p| p.is_finite()) { self.push(e, id, Shape::Stroke(points)); }
    }

    fn faces(&mut self, e: &Element, id: u64, triangles: Vec<[DVec3; 3]>) {
        let triangles: Vec<[DVec3; 3]> = triangles.into_iter().map(|t| t.map(|p| self.design.world(p))).collect();
        if !triangles.is_empty() && triangles.iter().flatten().all(|p| p.is_finite()) { self.push(e, id, Shape::Faces(triangles)); }
    }

    /// Draws element `i` (and its components) as part of pickable element `id`.
    fn draw(&mut self, all: &[Element], i: usize, id: u64, depth: usize, texts: &mut Vec<String>) {
        let e = all[i];
        if depth > MAX_DEPTH { self.omit("Too deeply nested element"); return; }
        let filled = self.design.three_d && !e.h_bit();
        match e.kind {
            2 | 7 | 12 => {
                // Cells keep their components in design coordinates; text nodes hold text elements.
                let end = span(all, i, 0);
                let mut j = i + 1;
                while j < end {
                    self.draw(all, j, id, depth + 1, texts);
                    j = span(all, j, depth + 1);
                }
            }
            14 => {
                let mut outline = Vec::new();
                let end = span(all, i, 0);
                let mut j = i + 1;
                while j < end {
                    if let Some((points, _)) = self.outline(all, j) { outline.extend(points); }
                    else { self.omit("Unsupported complex shape component"); }
                    j = span(all, j, depth + 1);
                }
                self.closed(&e, id, outline, filled);
            }
            3 | 4 | 11 | 15 | 16 | 27 | 6 => match self.outline(all, i) {
                Some((points, closed)) if closed && e.kind != 3 => self.closed(&e, id, points, filled),
                Some((points, _)) => self.stroke(&e, id, points),
                None => self.omit("Malformed element"),
            },
            17 => self.text(&e, id, texts),
            18 | 19 => self.surface(all, i, id),
            22 => {
                let points = self.vertices(&e);
                if e.h_bit() { for p in points { self.stroke(&e, id, vec![p]); } } else { self.stroke(&e, id, points); }
            }
            23 => self.cone(&e, id),
            24 => self.bspline_surface(all, i, id),
            _ => self.omit(kind_name(e.kind)),
        }
    }

    /// A closed outline: a face in 3D files, a closed polyline otherwise.
    fn closed(&mut self, e: &Element, id: u64, mut points: Vec<DVec3>, filled: bool) {
        if points.len() > 1 && points.first() != points.last() { points.push(points[0]); }
        if filled {
            let triangles = triangulate(&points);
            if !triangles.is_empty() { self.faces(e, id, triangles); return; }
        }
        self.stroke(e, id, points);
    }

    /// Vertices of a line, line string, shape, curve, pole or point string element.
    fn vertices(&self, e: &Element) -> Vec<DVec3> {
        let size = self.design.point_words();
        let (first, count) = if e.kind == 3 { (18, 2) } else { (19, usize::from(e.word(18))) };
        let available = e.words().saturating_sub(first) / size;
        (0..count.min(available)).map(|k| self.design.point(e, first + k * size)).collect()
    }

    /// The points (design coordinates) of a line-like element, and whether it is closed.
    fn outline(&self, all: &[Element], i: usize) -> Option<(Vec<DVec3>, bool)> {
        let e = &all[i];
        let d = self.design;
        match e.kind {
            3 | 4 | 6 => {
                let points = self.vertices(e);
                (points.len() >= if e.kind == 3 { 2 } else { 1 }).then_some((points, e.kind == 6))
            }
            11 => {
                let points = self.vertices(e);
                (points.len() >= 6).then(|| (akima(&points), false))
            }
            15 | 16 => {
                let (start, sweep_angle, at) = if e.kind == 16 { (angle(e.long(18)), sweep(e.ulong(20)), 22) } else { (0.0, std::f64::consts::TAU, 18) };
                let (primary, secondary) = (e.double(at), e.double(at + 4));
                let rotation = d.rotation(e, at + 8);
                let center = d.dpoint(e, at + 8 + d.rotation_words());
                if e.words() < at + 8 + d.rotation_words() + d.dpoint_words() { return None; }
                Some((ellipse(center, rotation.x_axis * primary, rotation.y_axis * secondary, start, sweep_angle), e.kind == 15))
            }
            27 => bspline_curve(d, all, i).map(|points| (points, false)),
            _ => None,
        }
    }

    /// Text: its box (from the origin, which is the lower left, and the size multipliers) as an
    /// outline, the characters in the element's properties.
    fn text(&mut self, e: &Element, id: u64, texts: &mut Vec<String>) {
        let d = self.design;
        let rotation = d.rotation(e, 23);
        let origin_at = 23 + d.rotation_words();
        let count_byte = 2 * (origin_at + d.point_words());
        let Some(&count) = e.bytes.get(count_byte) else { self.omit("Malformed element"); return };
        let text: String = e.bytes.iter().skip(count_byte + 2).take(usize::from(count)).map(|&c| c as char).collect();
        let origin = d.point(e, origin_at);
        // The basic character cell is 6 UOR; multipliers are thousandths.
        let height = f64::from(e.ulong(21)) / 1000.0 * 6.0;
        let width = f64::from(e.ulong(19)) / 1000.0 * 6.0 * f64::from(count).max(1.0);
        let (x, y) = (rotation.x_axis * width, rotation.y_axis * height);
        self.stroke(e, id, vec![origin, origin + x, origin + x + y, origin + y, origin]);
        if !text.trim().is_empty() { texts.push(text); }
    }

    /// Surfaces and solids of projection: faces between equal boundary outlines, capped for a solid.
    /// Others (surfaces of revolution, unequal boundaries) are drawn as their outlines.
    fn surface(&mut self, all: &[Element], i: usize, id: u64) {
        let e = all[i];
        let end = span(all, i, 0);
        let surface_type = e.bytes.get(40).copied().unwrap_or(0);
        let revolution = if e.kind == 18 { surface_type == 8 } else { surface_type == 1 };
        let mut boundaries = Vec::new();
        let mut lines = Vec::new();
        for j in i + 1..end {
            let Some((points, _)) = self.outline(all, j) else { continue };
            if all[j].class() == 0 { boundaries.push(points.clone()); }
            lines.push((j, points));
        }
        let ruled = !revolution && boundaries.len() >= 2 && boundaries.windows(2).all(|pair| pair[0].len() == pair[1].len() && pair[0].len() >= 2);
        if !ruled {
            if revolution { self.omit("Surface of revolution (outline drawn)"); }
            for (j, points) in lines { self.stroke(&all[j], id, points); }
            return;
        }
        let mut triangles = Vec::new();
        for pair in boundaries.windows(2) {
            for k in 0..pair[0].len() - 1 {
                let (a, b, c, d) = (pair[0][k], pair[0][k + 1], pair[1][k + 1], pair[1][k]);
                triangles.push([a, b, c]);
                triangles.push([a, c, d]);
            }
        }
        if e.kind == 19 {
            for cap in [&boundaries[0], &boundaries[boundaries.len() - 1]] {
                if cap.first() == cap.last() { triangles.extend(triangulate(cap)); }
            }
        }
        self.faces(&e, id, triangles);
    }

    fn cone(&mut self, e: &Element, id: u64) {
        if e.words() < 59 || !self.design.three_d { self.omit("Malformed element"); return; }
        let rotation = self.design.rotation(e, 19);
        let circle = |at: usize| {
            let (center, radius) = (DVec3::new(e.double(at), e.double(at + 4), e.double(at + 8)), e.double(at + 12));
            ellipse(center, rotation.x_axis * radius, rotation.y_axis * radius, 0.0, std::f64::consts::TAU)
        };
        let (bottom, top) = (circle(27), circle(43));
        let mut triangles = Vec::new();
        for k in 0..bottom.len() - 1 {
            triangles.push([bottom[k], bottom[k + 1], top[k + 1]]);
            triangles.push([bottom[k], top[k + 1], top[k]]);
        }
        if e.word(18) & 0x8000 == 0 {
            triangles.extend(triangulate(&bottom));
            triangles.extend(triangulate(&top));
        }
        self.faces(e, id, triangles);
    }

    fn bspline_surface(&mut self, all: &[Element], i: usize, id: u64) {
        let e = all[i];
        match bspline_surface(self.design, all, i) {
            Some(triangles) => {
                if all[i + 1..span(all, i, 0)].iter().any(|c| c.kind == 25) { self.omit("B-spline surface trim boundary (ignored)"); }
                self.faces(&e, id, triangles);
            }
            None => self.omit("Malformed B-spline surface"),
        }
    }

    fn finish(self) -> Result<Loaded, String> {
        let omitted = self.omitted.iter().map(|(name, count)| format!("{name}: {count}")).collect::<Vec<_>>().join(", ");
        if self.prims.is_empty() {
            return Err(format!("MicroStation DGN: no drawable elements. Skipped: {}", if omitted.is_empty() { "none" } else { &omitted }));
        }
        let half_width = line_half_width(&self.prims);
        let origin = self.prims.iter().find_map(|prim| match &prim.shape {
            Shape::Stroke(points) => points.first().copied(),
            Shape::Faces(triangles) => triangles.first().map(|t| t[0]),
        }).unwrap_or(DVec3::ZERO);
        let mut layers: BTreeMap<u8, Layer> = BTreeMap::new();
        let mut triangles = 0usize;
        for prim in &self.prims {
            let (geometry, color) = match &prim.shape {
                Shape::Stroke(points) => (ribbons(points, half_width * (1.0 + 0.35 * f64::from(prim.weight)), self.design.three_d), ink(prim.color)),
                Shape::Faces(faces) => (faces.iter().filter_map(|&t| {
                    let normal = (t[1] - t[0]).cross(t[2] - t[0]);
                    (normal.length_squared() > 1e-24).then(|| (t, normal.normalize()))
                }).collect(), prim.color),
            };
            triangles += geometry.len();
            if triangles > MAX_TRIANGLES { return Err("MicroStation DGN: triangle limit exceeded".into()); }
            let layer = layers.entry(prim.level).or_insert_with(|| Layer {
                name: format!("Level {}", prim.level), color: DEFAULT_COLOR, colors: Some(Vec::new()), ..Layer::default()
            });
            let first = u32::try_from(layer.indices.len()).map_err(|_| "MicroStation DGN: layer index limit exceeded")?;
            for (triangle, normal) in geometry {
                for p in triangle {
                    let local = (p - origin).as_vec3();
                    if !local.is_finite() { return Err("MicroStation DGN: model extent exceeds supported coordinates".into()); }
                    layer.indices.push(u32::try_from(layer.positions.len() / 3).map_err(|_| "MicroStation DGN: layer vertex limit exceeded")?);
                    layer.positions.extend(local.to_array());
                    layer.normals.extend(normal.as_vec3().to_array());
                    layer.colors.as_mut().unwrap().push([color[0], color[1], color[2], 255]);
                }
            }
            let count = layer.indices.len() as u32 - first;
            match layer.elements.last_mut() {
                Some(last) if last.0 == prim.id && last.1 + last.2 == first => last.2 += count,
                _ if count > 0 => layer.elements.push((prim.id, first, count)),
                _ => {}
            }
        }
        // Properties go with the first layer that holds the element.
        let mut placed = BTreeSet::new();
        for (id, level, props) in self.props {
            let target = if layers.get(&level).is_some_and(|l| l.elements.iter().any(|e| e.0 == id)) { Some(level) }
                else { layers.iter().find(|(_, l)| l.elements.iter().any(|e| e.0 == id)).map(|(&k, _)| k) };
            if let Some(level) = target.filter(|_| placed.insert(id)) {
                layers.get_mut(&level).unwrap().props.push((id, props));
            }
        }
        let counts = self.counts.iter().map(|(name, count)| format!("{name}: {count}")).collect::<Vec<_>>().join(", ");
        let mut project = ProjectData::default();
        project.info = vec![
            ("DGN version".into(), format!("V7 (ISFF), {}", if self.design.three_d { "3D" } else { "2D" })),
            ("DGN units".into(), self.design.units.clone()),
            ("DGN metres per UOR".into(), self.design.metres_per_uor.to_string()),
            ("DGN global origin (UOR)".into(), format!("{}, {}, {}", self.design.origin.x, self.design.origin.y, self.design.origin.z)),
            ("DGN world origin (metres)".into(), format!("{}, {}, {}", origin.x, origin.y, origin.z)),
            ("DGN elements".into(), counts),
            ("DGN skipped elements".into(), if omitted.is_empty() { "0".into() } else { omitted }),
        ];
        Ok(Loaded { layers: layers.into_values().filter(|l| !l.indices.is_empty()).collect(), views: Vec::new(), project })
    }
}

/// The index after element `i` and its components. Complex headers count `19 + totlength` words
/// from their start; components carry the complex bit. B-spline headers are followed by their
/// knot, boundary, pole and weight elements.
fn span(all: &[Element], i: usize, depth: usize) -> usize {
    let e = all[i];
    let mut j = i + 1;
    match e.kind {
        2 | 7 | 12 | 14 | 18 | 19 if depth <= MAX_DEPTH => {
            let end = e.offset + 2 * (19 + usize::from(e.word(18)));
            while j < all.len() && all[j].offset < end && all[j].complex {
                j = span(all, j, depth + 1);
            }
        }
        24 | 27 => while j < all.len() && matches!(all[j].kind, 21 | 25 | 26 | 28) { j += 1 },
        _ => {}
    }
    j
}

/// Points of an elliptical arc: `center + a cos t + b sin t`.
fn ellipse(center: DVec3, a: DVec3, b: DVec3, start: f64, sweep: f64) -> Vec<DVec3> {
    let segments = ((sweep.abs() / std::f64::consts::TAU * 72.0).ceil() as usize).clamp(4, 72);
    (0..=segments).map(|k| {
        let t = start + sweep * k as f64 / segments as f64;
        center + a * t.cos() + b * t.sin()
    }).collect()
}

/// The ISFF curve (type 11): piecewise cubics through points 2..n-3 with Akima-style tangents;
/// the first two and last two points only shape the ends.
fn akima(points: &[DVec3]) -> Vec<DVec3> {
    let n = points.len();
    let lengths: Vec<f64> = points.windows(2).map(|p| p[0].distance(p[1])).collect();
    let slope = |k: usize| if lengths[k] > 0.0 { (points[k + 1] - points[k]) / lengths[k] } else { DVec3::ZERO };
    let tangent = |k: usize| {
        let (m2, m1, m0, p1) = (slope(k - 2), slope(k - 1), slope(k), slope(k + 1));
        let axis = |a: f64, b: f64, c: f64, d: f64| {
            let (w1, w2) = ((d - c).abs(), (b - a).abs());
            if w1 + w2 > 0.0 { (b * w1 + c * w2) / (w1 + w2) } else { (b + c) / 2.0 }
        };
        DVec3::new(axis(m2.x, m1.x, m0.x, p1.x), axis(m2.y, m1.y, m0.y, p1.y), axis(m2.z, m1.z, m0.z, p1.z))
    };
    let mut out = vec![points[2]];
    for k in 2..n - 3 {
        let length = lengths[k];
        if length <= 0.0 { continue; }
        let (t0, t1, p0, p1) = (tangent(k), tangent(k + 1), points[k], points[k + 1]);
        let chord = (p1 - p0) / length;
        let b = (chord * 3.0 - t0 * 2.0 - t1) / length;
        let a = (t0 + t1 - chord * 2.0) / (length * length);
        for s in 1..=8 {
            let d = length * f64::from(s) / 8.0;
            out.push(p0 + t0 * d + b * d * d + a * d * d * d);
        }
    }
    out
}

/// Interior knots (fractions) from a knot element, or none.
fn knots(element: Option<&Element>, count: usize, skip: usize) -> Option<Vec<f64>> {
    let e = element?;
    let values: Vec<f64> = (0..count).map(|k| f64::from(e.long(18 + 2 * (skip + k))) / UNIT).collect();
    (e.words() >= 18 + 2 * (skip + count)).then_some(values)
}

/// A full knot vector for `poles` poles of `order`; open curves are clamped. Closed curves are
/// treated as uniform periodic and need their first `order - 1` poles repeated.
fn knot_vector(poles: usize, order: usize, interior: Option<Vec<f64>>, closed: bool) -> Vec<f64> {
    if closed {
        return (0..poles + order - 1 + order).map(|k| k as f64).collect();
    }
    let inner = interior.filter(|v| v.len() == poles - order && v.windows(2).all(|w| w[0] <= w[1]))
        .unwrap_or_else(|| (1..=poles - order).map(|k| k as f64 / (poles - order + 1) as f64).collect());
    std::iter::repeat_n(0.0, order).chain(inner).chain(std::iter::repeat_n(1.0, order)).collect()
}

/// De Boor's algorithm on homogeneous poles.
fn de_boor(poles: &[DVec4], knots: &[f64], order: usize, t: f64) -> DVec4 {
    let n = poles.len();
    let mut s = order - 1;
    while s + 1 < n && knots[s + 1] <= t { s += 1; }
    let mut d: Vec<DVec4> = (0..order).map(|j| poles[j + s + 1 - order]).collect();
    for r in 1..order {
        for j in (r..order).rev() {
            let i = j + s + 1 - order;
            let denominator = knots[i + order - r] - knots[i];
            let alpha = if denominator.abs() < 1e-15 { 0.0 } else { (t - knots[i]) / denominator };
            d[j] = d[j - 1] * (1.0 - alpha) + d[j] * alpha;
        }
    }
    d[order - 1]
}

/// Samples a B-spline over its domain; `poles` already include a periodic wrap.
fn sample(poles: &[DVec4], knots: &[f64], order: usize, samples: usize) -> Vec<DVec4> {
    let (lo, hi) = (knots[order - 1], knots[poles.len()]);
    (0..=samples).map(|k| de_boor(poles, knots, order, lo + (hi - lo) * k as f64 / samples as f64)).collect()
}

/// Order, closed and rational flags from a B-spline flags word.
fn bspline_flags(word: u16) -> (usize, bool, bool) {
    (usize::from(word & 0xF) + 2, word & 0x80 != 0, word & 0x40 != 0)
}

/// Pole rows (with weights) following a B-spline header.
fn pole_rows(design: &Design, all: &[Element], i: usize) -> Vec<Vec<DVec4>> {
    let mut rows: Vec<Vec<DVec4>> = Vec::new();
    for e in &all[i + 1..span(all, i, 0)] {
        match e.kind {
            21 => {
                let size = design.point_words();
                let count = usize::from(e.word(18)).min(e.words().saturating_sub(19) / size);
                rows.push((0..count).map(|k| design.point(e, 19 + k * size).extend(1.0)).collect());
            }
            28 => if let Some(row) = rows.last_mut() {
                for (k, pole) in row.iter_mut().enumerate() {
                    let weight = f64::from(e.long(18 + 2 * k)) / UNIT;
                    if weight > 0.0 && e.words() >= 20 + 2 * k { *pole = (pole.truncate() * weight).extend(weight); }
                }
            },
            _ => {}
        }
    }
    rows
}

fn wrap(mut poles: Vec<DVec4>, order: usize, closed: bool) -> Vec<DVec4> {
    if closed { for k in 0..order - 1 { poles.push(poles[k % poles.len()]); } }
    poles
}

fn bspline_curve(design: &Design, all: &[Element], i: usize) -> Option<Vec<DVec3>> {
    let e = all[i];
    let (order, closed, _) = bspline_flags(e.word(20));
    let poles = pole_rows(design, all, i).into_iter().next()?;
    if poles.len() < order { return (poles.len() >= 2).then(|| poles.iter().map(|p| p.truncate() / p.w).collect()); }
    let knot_element = all[i + 1..span(all, i, 0)].iter().find(|c| c.kind == 26);
    let knots = knot_vector(poles.len(), order, knots(knot_element, poles.len().saturating_sub(order), 0), closed);
    let poles = wrap(poles, order, closed);
    let samples = (poles.len() * 8).clamp(16, 400);
    Some(sample(&poles, &knots, order, samples).into_iter().map(|p| p.truncate() / p.w).collect())
}

fn bspline_surface(design: &Design, all: &[Element], i: usize) -> Option<Vec<[DVec3; 3]>> {
    let e = all[i];
    let (order_u, closed_u, _) = bspline_flags(e.word(20));
    let (order_v, closed_v, _) = bspline_flags(e.word(24));
    let rows = pole_rows(design, all, i);
    let nu = rows.first()?.len();
    if nu < order_u || rows.len() < order_v || rows.iter().any(|r| r.len() != nu) { return None; }
    let knot_element = all[i + 1..span(all, i, 0)].iter().find(|c| c.kind == 26);
    let knots_u = knot_vector(nu, order_u, knots(knot_element, nu - order_u, 0), closed_u);
    let knots_v = knot_vector(rows.len(), order_v, knots(knot_element, rows.len() - order_v, nu - order_u), closed_v);
    let (su, sv) = ((nu * 4).clamp(8, 64), (rows.len() * 4).clamp(8, 64));
    let along_u: Vec<Vec<DVec4>> = rows.into_iter().map(|row| sample(&wrap(row, order_u, closed_u), &knots_u, order_u, su)).collect();
    let grid: Vec<Vec<DVec3>> = (0..=su).map(|k| {
        let column: Vec<DVec4> = along_u.iter().map(|row| row[k]).collect();
        sample(&wrap(column, order_v, closed_v), &knots_v, order_v, sv).into_iter().map(|p| p.truncate() / p.w).collect()
    }).collect();
    let mut triangles = Vec::new();
    for a in 0..su {
        for b in 0..sv {
            let (p, q, r, s) = (grid[a][b], grid[a + 1][b], grid[a + 1][b + 1], grid[a][b + 1]);
            triangles.push([p, q, r]);
            triangles.push([p, r, s]);
        }
    }
    Some(triangles)
}

/// Triangulates a planar polygon (closing point optional).
fn triangulate(points: &[DVec3]) -> Vec<[DVec3; 3]> {
    let mut points = points.to_vec();
    points.dedup();
    if points.len() > 1 && points.first() == points.last() { points.pop(); }
    if points.len() < 3 { return Vec::new(); }
    let origin = points[0];
    let normal = (0..points.len()).fold(DVec3::ZERO, |sum, i| sum + (points[i] - origin).cross(points[(i + 1) % points.len()] - origin));
    if normal.length_squared() <= 1e-24 { return Vec::new(); }
    let axis = if normal.x.abs() >= normal.y.abs() && normal.x.abs() >= normal.z.abs() { 0 }
        else if normal.y.abs() >= normal.z.abs() { 1 } else { 2 };
    let coords: Vec<f64> = points.iter().flat_map(|p| {
        let p = *p - origin;
        match axis { 0 => [p.y, p.z], 1 => [p.x, p.z], _ => [p.x, p.y] }
    }).collect();
    let Ok(indices) = earcutr::earcut(&coords, &[], 2) else { return Vec::new() };
    indices.chunks_exact(3).map(|t| [points[t[0]], points[t[1]], points[t[2]]]).collect()
}

/// Half the width of a weight-0 line: a fixed share of the drawing extent, measured between the
/// 2nd and 98th percentile so a few stray far-away elements do not make every line fat.
fn line_half_width(prims: &[Prim]) -> f64 {
    let points: Vec<DVec3> = prims.iter().flat_map(|prim| match &prim.shape {
        Shape::Stroke(points) => points.clone(),
        Shape::Faces(triangles) => triangles.iter().flatten().copied().collect(),
    }).collect();
    let step = (points.len() / 100_000).max(1);
    let mut extent = DVec3::ZERO;
    for axis in 0..3 {
        let mut values: Vec<f64> = points.iter().step_by(step).map(|p| p[axis]).collect();
        values.sort_by(f64::total_cmp);
        let at = |f: f64| values[((values.len() - 1) as f64 * f).round() as usize];
        extent[axis] = at(0.98) - at(0.02);
    }
    let diagonal = extent.length();
    if diagonal > 0.0 { diagonal / 3000.0 } else { 0.01 }
}

/// Line colours as ink on paper: very light colours (white is colour 0) draw dark grey.
fn ink(color: [u8; 3]) -> [u8; 3] {
    let luma = 0.2126 * f64::from(color[0]) + 0.7152 * f64::from(color[1]) + 0.0722 * f64::from(color[2]);
    if luma > 215.0 { [45, 45, 48] } else { color }
}

/// A polyline as thin quads with square ends: flat (facing +Z) in 2D files, two crossed quads per
/// segment in 3D files. A single point becomes a small square.
fn ribbons(points: &[DVec3], half: f64, three_d: bool) -> Vec<([DVec3; 3], DVec3)> {
    let mut out = Vec::new();
    let mut quad = |a: DVec3, b: DVec3, offset: DVec3, normal: DVec3| {
        let (p, q, r, s) = (a + offset, b + offset, b - offset, a - offset);
        out.push(([p, q, r], normal));
        out.push(([p, r, s], normal));
    };
    let mut points = points.to_vec();
    points.dedup();
    if points.len() == 1 {
        let p = points[0];
        quad(p - DVec3::X * half * 2.0, p + DVec3::X * half * 2.0, DVec3::Y * half * 2.0, DVec3::Z);
        if three_d { quad(p - DVec3::X * half * 2.0, p + DVec3::X * half * 2.0, DVec3::Z * half * 2.0, DVec3::Y); }
        return out;
    }
    for pair in points.windows(2) {
        let direction = (pair[1] - pair[0]).normalize();
        let (a, b) = (pair[0] - direction * half, pair[1] + direction * half);
        if three_d {
            let side = if direction.cross(DVec3::Z).length_squared() > 1e-6 { direction.cross(DVec3::Z).normalize() } else { DVec3::X };
            let up = direction.cross(side).normalize();
            quad(a, b, side * half, up);
            quad(a, b, up * half, side);
        } else {
            quad(a, b, DVec3::new(-direction.y, direction.x, 0.0) * half, DVec3::Z);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Writes design files from the ISFF element layouts.
    struct Writer {
        words: Vec<u16>,
        three_d: bool,
    }

    fn long(v: i32) -> [u16; 2] { [((v as u32) >> 16) as u16, v as u32 as u16] }

    fn dfloat(v: f64) -> [u16; 4] {
        if v == 0.0 { return [0; 4]; }
        let bits = v.to_bits();
        let exponent = ((bits >> 52) & 0x7FF) - 894;
        let u = (bits & (1 << 63)) | (exponent << 55) | ((bits & ((1 << 52) - 1)) << 3);
        [(u >> 48) as u16, (u >> 32) as u16, (u >> 16) as u16, u as u16]
    }

    impl Writer {
        /// A design file header: `master` and `sub` unit names, subunits per master, UOR per subunit.
        fn new(three_d: bool, master: &str, sub: &str, sub_per_master: i32, uor_per_sub: i32, origin: [f64; 3]) -> Self {
            let mut tcb = vec![0u16; 768];
            tcb[0] = 0x0908 | if three_d { 0xC0 } else { 0 };
            tcb[1] = 766;
            tcb[556..558].copy_from_slice(&long(sub_per_master));
            tcb[558..560].copy_from_slice(&long(uor_per_sub));
            let name = |s: &str| { let b = s.as_bytes(); u16::from(*b.first().unwrap_or(&0)) | (u16::from(*b.get(1).unwrap_or(&0)) << 8) };
            tcb[560] = name(master);
            tcb[561] = name(sub);
            for (k, &v) in origin.iter().enumerate() { tcb[620 + 4 * k..624 + 4 * k].copy_from_slice(&dfloat(v)); }
            Writer { words: tcb, three_d }
        }

        /// An element: header (range left zero), display header, then `body`.
        fn element(&mut self, kind: u8, level: u8, color: u8, weight: u8, flags: u16, body: &[u16]) -> &mut Self {
            let complex = flags & 0x80;
            let mut e = vec![u16::from(kind) << 8 | u16::from(level) | complex, 0];
            e.extend([0u16; 12]);
            e.extend([0, 0, flags & 0xFF0F, u16::from(color) << 8 | u16::from(weight) << 3]);
            e.extend_from_slice(body);
            e[1] = (e.len() - 2) as u16;
            e[15] = (e.len() - 16) as u16;
            self.words.extend(e);
            self
        }

        fn points(&self, points: &[[i32; 3]]) -> Vec<u16> {
            points.iter().flat_map(|p| p[..if self.three_d { 3 } else { 2 }].iter().flat_map(|&v| long(v)).collect::<Vec<_>>()).collect()
        }

        fn bytes(&self) -> Vec<u8> {
            self.words.iter().chain([0xFFFF].iter()).flat_map(|w| w.to_le_bytes()).collect()
        }
    }

    fn load(bytes: Vec<u8>) -> Result<Loaded, String> {
        DgnLoader.load_with(&Source::Bytes { name: "drawing.dgn".into(), data: Arc::new(bytes) }, &Progress::default())
    }

    fn info<'a>(loaded: &'a Loaded, key: &str) -> &'a str {
        &loaded.project.info.iter().find(|(k, _)| k == key).unwrap().1
    }

    fn bounds(layer: &Layer) -> ([f32; 3], [f32; 3]) {
        layer.positions.chunks_exact(3).fold(([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]), |(mut lo, mut hi), p| {
            for k in 0..3 { lo[k] = lo[k].min(p[k]); hi[k] = hi[k].max(p[k]); }
            (lo, hi)
        })
    }

    #[test]
    fn numbers_decode_as_documented() {
        let mut bytes = Vec::new();
        for w in long(-123_456).iter().chain(dfloat(1.0).iter()).chain(dfloat(-2.5e6).iter()) { bytes.extend(w.to_le_bytes()); }
        let e = Element { kind: 3, level: 1, complex: false, offset: 0, bytes: &bytes };
        assert_eq!(e.long(0), -123_456);
        assert_eq!(e.word(2), 0x4080, "1.0 is 4080 0000 0000 0000 in D-float");
        assert_eq!(e.double(2), 1.0);
        assert_eq!(e.double(6), -2.5e6);
        assert_eq!(radix50([1683, 0]), "ABC");
    }

    #[test]
    fn reads_a_2d_drawing_in_metres_by_level_with_colours_and_text() {
        // 1 FT = 10 TH = 1000 UOR, origin shifted like a State Plane drawing.
        let mut w = Writer::new(false, "FT", "TH", 10, 100, [-1000.0, -2000.0, 0.0]);
        let mut table = vec![0u16];
        let mut rgb = vec![[0u8, 0, 0]; 256];
        rgb[4] = [200, 10, 20];
        let flat: Vec<u8> = rgb.concat();
        table.extend(flat.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])));
        w.element(5, 1, 0, 0, 0, &table);
        w.element(3, 5, 3, 2, 0, &w.points(&[[-1000, -2000, 0], [9000, -2000, 0]]));
        let mut string = vec![3];
        string.extend(w.points(&[[0, 0, 0], [1000, 0, 0], [1000, 1000, 0]]));
        w.element(4, 6, 0, 0, 0, &string);
        // A deleted line far away must not count.
        let far = w.points(&[[90_000_000, 0, 0], [91_000_000, 0, 0]]);
        w.element(3, 6, 0, 0, 0, &far);
        let deleted = w.words.len() - far.len() - 18;
        w.words[deleted] |= 0x8000;
        // Text "AB" of height 6 * 500 / 1000 * ... = 300 UOR at (0, 3000).
        let mut text = vec![0x0000];
        text.extend(long(50_000));
        text.extend(long(50_000));
        text.extend(long(0));
        text.extend(w.points(&[[0, 3000, 0]]));
        text.extend([2, u16::from_le_bytes(*b"AB")]);
        w.element(17, 7, 0, 0, 0, &text);
        let loaded = load(w.bytes()).unwrap();
        assert_eq!(loaded.layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Level 5", "Level 6", "Level 7"]);
        assert!(info(&loaded, "DGN version").contains("2D"));
        assert_eq!(info(&loaded, "DGN units"), "1 FT = 10 TH = 1000 UOR");
        assert!(info(&loaded, "DGN elements").contains("Line: 1"));
        // The line from the global origin runs 10 ft east: 3.048 m plus the square ends.
        let (lo, hi) = bounds(&loaded.layers[0]);
        assert!((hi[0] - lo[0] - 3.048).abs() < 0.02, "{lo:?} {hi:?}");
        assert!(lo[2] == 0.0 && hi[2] == 0.0);
        // Colour 3 is slot 4 of the stored table.
        assert_eq!(loaded.layers[0].colors.as_ref().unwrap()[0], [200, 10, 20, 255]);
        assert_eq!(loaded.layers[0].elements.len(), 1);
        // Origin (world) is the first point, which is the global origin: (0, 0) in metres.
        assert_eq!(info(&loaded, "DGN world origin (metres)"), "0, 0, 0");
        let (lo, hi) = bounds(&loaded.layers[1]);
        assert!(hi[0] < 4.0 && lo[0] > 0.0, "deleted line is skipped: {lo:?} {hi:?}");
        let text_layer = &loaded.layers[2];
        assert!(text_layer.props[0].1.contains("\"Text\":\"AB\""), "{}", text_layer.props[0].1);
        let (lo, hi) = bounds(text_layer);
        // Box 2 characters x 300 UOR wide, 300 UOR high: 0.18 by 0.09 m, above y = (3000 + 2000) UOR.
        assert!((hi[0] - lo[0] - 0.18).abs() < 0.02 && (hi[1] - lo[1] - 0.09).abs() < 0.02, "{lo:?} {hi:?}");
        assert!((lo[1] - 1.524).abs() < 0.02, "{lo:?}");
    }

    #[test]
    fn arcs_ellipses_and_curves_follow_their_geometry() {
        let mut w = Writer::new(false, "M", "MM", 1000, 1, [0.0; 3]);
        // A quarter arc of radius 1 m around (0, 0), from 0 to 90 degrees.
        let mut arc = Vec::new();
        arc.extend(long(0));
        arc.extend(long(90 * 360_000));
        arc.extend(dfloat(1000.0));
        arc.extend(dfloat(1000.0));
        arc.extend(long(0));
        arc.extend(dfloat(0.0));
        arc.extend(dfloat(0.0));
        w.element(16, 1, 1, 0, 0, &arc);
        // An ellipse 4 x 2 m turned 90 degrees, around (10, 0).
        let mut ellipse = Vec::new();
        ellipse.extend(dfloat(2000.0));
        ellipse.extend(dfloat(1000.0));
        ellipse.extend(long(90 * 360_000));
        ellipse.extend(dfloat(10_000.0));
        ellipse.extend(dfloat(0.0));
        w.element(15, 2, 1, 0, 0, &ellipse);
        let loaded = load(w.bytes()).unwrap();
        // The world origin is the arc's first point (1, 0): the arc spans x -1..0 and y 0..1.
        let (lo, hi) = bounds(&loaded.layers[0]);
        assert!((lo[0] + 1.0).abs() < 0.02 && hi[0].abs() < 0.02 && lo[1].abs() < 0.02 && (hi[1] - 1.0).abs() < 0.02, "{lo:?} {hi:?}");
        let (lo, hi) = bounds(&loaded.layers[1]);
        assert!((hi[0] - lo[0] - 2.0).abs() < 0.02 && (hi[1] - lo[1] - 4.0).abs() < 0.02, "{lo:?} {hi:?}");
        assert!(((hi[0] + lo[0]) / 2.0 - 9.0).abs() < 0.01);

        let curve = akima(&(0..8).map(|k| DVec3::new(k as f64, (k as f64 * 0.7).sin(), 0.0)).collect::<Vec<_>>());
        assert_eq!(curve.first().unwrap().x, 2.0);
        assert!((curve.last().unwrap().x - 5.0).abs() < 1e-9);
    }

    #[test]
    fn cells_and_complex_shapes_are_one_element_each() {
        let mut w = Writer::new(false, "M", "MM", 1000, 1, [0.0; 3]);
        let line = w.points(&[[0, 0, 0], [1000, 0, 0]]);
        let line2 = w.points(&[[0, 1000, 0], [1000, 1000, 0]]);
        // A cell: totlength counts the header words after word 18 and the components.
        let mut cell = vec![0u16];
        cell.extend(long(0x0693_0000)); // RADIX-50 "ABC"
        cell.extend([0u16; 5 + 8 + 8 + 4]);
        cell[0] = (18 + cell.len() + 2 * (18 + line.len()) - 19) as u16;
        w.element(2, 3, 0, 0, 0, &cell);
        w.element(3, 3, 0, 0, 0x80, &line);
        w.element(3, 4, 2, 0, 0x80, &line2);
        // A complex shape of two line strings.
        let mut shape = vec![0u16, 2, 0, 0, 0, 0];
        let mut a = vec![2u16];
        a.extend(w.points(&[[5000, 0, 0], [6000, 0, 0]]));
        let mut b = vec![3u16];
        b.extend(w.points(&[[6000, 0, 0], [6000, 1000, 0], [5000, 0, 0]]));
        shape[0] = ((18 + shape.len()) + (18 + a.len()) + (18 + b.len()) - 19) as u16;
        w.element(14, 5, 0, 0, 0, &shape);
        w.element(4, 5, 0, 0, 0x80, &a);
        w.element(4, 5, 0, 0, 0x80, &b);
        let loaded = load(w.bytes()).unwrap();
        let ids: BTreeSet<u64> = loaded.layers.iter().flat_map(|l| l.elements.iter().map(|e| e.0)).collect();
        assert_eq!(ids.len(), 2, "{:?}", loaded.layers.iter().map(|l| &l.elements).collect::<Vec<_>>());
        assert_eq!(loaded.layers.len(), 3, "cell components keep their own levels");
        assert!(loaded.layers[0].props[0].1.contains("\"Cell\""));
        assert!(info(&loaded, "DGN elements").contains("Cell: 1") && info(&loaded, "DGN elements").contains("Complex shape: 1"));
    }

    #[test]
    fn solids_cones_and_shapes_of_3d_files_are_faces() {
        let mut w = Writer::new(true, "M", "MM", 1000, 1, [0.0; 3]);
        // A unit cube as a solid of projection: two square boundaries and four rule lines.
        let square = |z: i32| {
            let mut s = vec![5u16];
            s.extend(w.points(&[[0, 0, z], [1000, 0, z], [1000, 1000, z], [0, 1000, z], [0, 0, z]]));
            s
        };
        let (bottom, top) = (square(0), square(1000));
        let rule = w.points(&[[0, 0, 0], [0, 0, 1000]]);
        let mut solid = vec![0u16, 3, 0, 0, 0, 0, 0];
        solid[0] = ((18 + solid.len()) + (18 + bottom.len()) + (18 + top.len()) + (18 + rule.len()) - 19) as u16;
        w.element(19, 2, 0, 0, 0, &solid);
        w.element(6, 2, 0, 0, 0x80, &bottom);
        w.element(6, 2, 0, 0, 0x80, &top);
        w.element(3, 2, 0, 0, 0x80 | 4, &rule);
        // A cylinder of radius 1 m and height 2 m with an identity quaternion, beside it.
        let mut cone = vec![0u16];
        cone.extend(long(i32::MAX));
        cone.extend([0u16; 6]);
        for v in [5000.0, 0.0, 0.0, 1000.0, 5000.0, 0.0, 2000.0, 1000.0] { cone.extend(dfloat(v)); }
        w.element(23, 3, 0, 0, 0, &cone);
        let loaded = load(w.bytes()).unwrap();
        let cube = &loaded.layers[0];
        assert_eq!(cube.triangle_count(), 12);
        assert!(!cube.solid_indices().is_empty(), "the capped solid encloses volume");
        let (lo, hi) = bounds(&loaded.layers[1]);
        assert!((hi[2] - lo[2] - 2.0).abs() < 1e-4 && (hi[0] - lo[0] - 2.0).abs() < 1e-3, "{lo:?} {hi:?}");
        assert!(info(&loaded, "DGN version").contains("3D"));
    }

    #[test]
    fn rotations_of_3d_elements_use_the_stored_quaternion() {
        let mut w = Writer::new(true, "M", "MM", 1000, 1, [0.0; 3]);
        // A circle of radius 1 m turned 90 degrees about X lies in the XZ plane.
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let mut ellipse = Vec::new();
        ellipse.extend(dfloat(1000.0));
        ellipse.extend(dfloat(1000.0));
        for q in [s, -s, 0.0, 0.0] { ellipse.extend(long((q * UNIT) as i32)); }
        for _ in 0..3 { ellipse.extend(dfloat(0.0)); }
        w.element(15, 1, 0, 0, 0x8000, &ellipse);
        let loaded = load(w.bytes()).unwrap();
        let (lo, hi) = bounds(&loaded.layers[0]);
        assert!((hi[2] - lo[2] - 2.0).abs() < 0.05 && (hi[1] - lo[1]) < 0.05, "{lo:?} {hi:?}");
    }

    #[test]
    fn bspline_curves_are_clamped_to_their_end_poles() {
        let mut w = Writer::new(false, "M", "MM", 1000, 1, [0.0; 3]);
        let mut header = vec![0u16, 0, 2, 4, 0];
        header[1] = 0;
        w.element(27, 1, 0, 0, 0, &header);
        let mut poles = vec![4u16];
        poles.extend(w.points(&[[0, 0, 0], [1000, 2000, 0], [3000, 2000, 0], [4000, 0, 0]]));
        w.element(21, 1, 0, 0, 0x80, &poles);
        let loaded = load(w.bytes()).unwrap();
        let (lo, hi) = bounds(&loaded.layers[0]);
        assert!((hi[0] - lo[0] - 4.0).abs() < 0.02, "{lo:?} {hi:?}");
        assert!(hi[1] < 1.6 && hi[1] > 1.4, "a cubic stays inside its control polygon: {hi:?}");
        assert_eq!(loaded.layers[0].elements.len(), 1);
    }

    /// Loads every `.dgn` below `$DGN_SAMPLES` (files that are not in the repository) and prints
    /// what came through: `DGN_SAMPLES=dir cargo test --lib dgn -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn samples_from_environment() {
        let Ok(root) = std::env::var("DGN_SAMPLES") else { return };
        let mut stack = vec![std::path::PathBuf::from(root)];
        while let Some(path) = stack.pop() {
            if path.is_dir() {
                stack.extend(std::fs::read_dir(&path).unwrap().map(|entry| entry.unwrap().path()));
            } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("dgn")) {
                match load(std::fs::read(&path).unwrap()) {
                    Ok(loaded) => {
                        let triangles: usize = loaded.layers.iter().map(Layer::triangle_count).sum();
                        println!("{}: {} layers, {triangles} triangles", path.display(), loaded.layers.len());
                        for (key, value) in &loaded.project.info { println!("    {key}: {value}"); }
                    }
                    Err(error) => println!("{}: {error}", path.display()),
                }
            }
        }
    }

    #[test]
    fn v8_and_foreign_files_are_refused_clearly() {
        let mut v8 = OLE_SIGNATURE.to_vec();
        v8.resize(512, 0);
        assert!(load(v8).err().unwrap().contains("V8"));
        assert!(load(b"not a design file at all".to_vec()).err().unwrap().contains("not a design file"));
        let header_only = Writer::new(false, "M", "MM", 1000, 1, [0.0; 3]).bytes();
        assert!(load(header_only).err().unwrap().contains("no drawable elements"));
    }
}
