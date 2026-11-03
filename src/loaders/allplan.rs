//! Allplan drawing files (`.ndw`, German "Teilbild"): the 3D bodies of one drawing file.
//!
//! Written from the clean-room specification "Allplan-tekenbestand (.ndw) v0.3" only. A drawing
//! file is a 12-byte header and pages of 32760 bytes; the element stream runs from the third page
//! to the end of the last page and holds padding, attribute blocks and element records (§2, §3).
//! Records of class 0xC6 are 3D bodies (§5): a prism (a bottom and a top ring) or an explicit
//! boundary (edges and faces with holes). Coordinates are world coordinates in millimetres, Z up;
//! the viewer shows them in metres without moving them, so drawing files of one project line up.
//! Each body links by key to its element record (class 0xEF or 0x6F, §8) for its name, element
//! type and quantities; visible bodies go in a layer per category (walls, slabs, ...), auxiliary
//! bodies (openings, rooms) in one layer that starts hidden. Macros (§9: furniture, windows,
//! doors, railings) are read once per definition (0x5A, the bodies of its 3D representations)
//! and placed by each placement record (0xEB) with its matrix; a placement is one element.
//!
//! Not read yet: colours and materials, 2D elements, project folders and layouts (`.npl`).

use super::{Loaded, Loader, Phase, Progress, Source, DEFAULT_COLOR};
use crate::model::{Layer, ProjectData};
use std::collections::HashMap;

pub struct AllplanLoader;

impl Loader for AllplanLoader {
    fn name(&self) -> &'static str { "Allplan" }
    fn extensions(&self) -> &'static [&'static str] { &["ndw"] }
    /// A project folder `*.prj` opens as a whole (§11), e.g. dropped on the window.
    fn folder_extensions(&self) -> &'static [&'static str] { &["prj"] }
    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }
    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        if let Some(dir) = source.path().filter(|path| path.is_dir()) {
            let project = read_project(dir)?;
            progress.set(Phase::Convert);
            return build_project(&project);
        }
        let data = source.read()?;
        progress.set(Phase::Convert);
        let drawing = parse(&data)?;
        // Colours come from the project folder: the folder of the drawing file (§10); 2D lines lie
        // on the storey the structure there gives the file, else at 0 (§12.2).
        let painted = source.path().and_then(|path| path.parent()).map(|dir| paint(&drawing, &mut Folder::new(dir)));
        let z = source.path().and_then(file_height).unwrap_or(0.0);
        build_at(&drawing, painted.as_ref(), z)
    }
}

/// Page size of a drawing file (§2).
const PAGE: usize = 0x7FF8;
/// Offset of the drawing file name: page 1 plus a u32 (§2).
const NAME_OFFSET: usize = 12 + PAGE + 4;
/// Length of the name in UTF-16 units.
const NAME_UNITS: usize = 70;
/// Start of the element stream: the third page (§2).
const STREAM_START: usize = 12 + 2 * PAGE;
/// Element record header (§3).
const HEADER: usize = 40;
/// Millimetres to the viewer's metres.
const MM: f64 = 0.001;
/// Form of clip and view volumes, not a building part (§5.5).
const FORM_CLIP: u16 = 0x0701;
/// Form of an open surface (§9.3).
const FORM_OPEN: u16 = 0x0201;
/// Colour of the macro bodies (§9.4).
const MACRO_COLOR: [f32; 3] = [0.78, 0.64, 0.50];
/// Most triangles all macro placements of one file may add (a small file can place a large
/// macro many times).
const MAX_MACRO_TRIANGLES: usize = 4_000_000;
/// How far into a macro definition the first representation is searched (§9.1).
const MACRO_SEARCH: usize = 4096;
/// Start of the nested records in a representation (§9.1).
const REPRESENTATION_BODY: usize = 216;
/// Colour of the auxiliary bodies (openings, rooms).
const AUXILIARY_COLOR: [f32; 3] = [0.60, 0.76, 0.92];
/// Classes a strictly valid header may have (§3, resync rule).
const KNOWN_CLASSES: [u8; 28] = [
    0x02, 0x46, 0x56, 0x5A, 0x62, 0x64, 0x65, 0x6F, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x8C, 0x8D,
    0x8E, 0x8F, 0xA6, 0xC6, 0xC7, 0xE4, 0xEB, 0xED, 0xEF,
];

/// What a drawing file holds.
#[derive(Debug, Default)]
struct Drawing {
    /// The name of the drawing file (not the file name), e.g. "Ground floor plan".
    name: String,
    pages: usize,
    bodies: Vec<Body>,
    /// The element records with data that bodies link to (§8).
    elements: Vec<Element>,
    /// The macro definitions that placements use (§9.1).
    macros: Vec<Macro>,
    placements: Vec<Placement>,
    /// The 2D lines and polylines drawn (§12.1).
    lines: Vec<Line2d>,
    /// 2D records left out: part of a 3D element, or unreadable.
    lines_with_3d: usize,
    lines_unreadable: usize,
    skipped_clip: usize,
    skipped_no_faces: usize,
    skipped_unreadable: usize,
}

/// The key that ties the records of one element together: (u32@24, u16@12) of the header (§8).
type Key = (u32, u16);

/// A 2D line (class 0x82) or polyline (0x84) of the drawing file (§12.1): world points in mm.
#[derive(Debug, Clone)]
struct Line2d {
    class: u8,
    key: Key,
    points: Vec<[f64; 2]>,
}

/// Largest coordinate of a 2D point taken (mm, §12.1).
const MAX_COORDINATE: f64 = 1e7;

/// Reads a 2D line or polyline record (§12.1); `None` when its points do not fit or are not
/// finite or too far out.
fn decode_line(r: &[u8]) -> Option<Line2d> {
    let class = *r.get(4)?;
    let n = usize::from(u16_at(r, 14)?);
    if n < 2 || (class == 0x82 && n != 2) || HEADER + 16 * n > r.len() {
        return None;
    }
    let points = (0..n)
        .map(|i| Some([f64_at(r, HEADER + 16 * i)?, f64_at(r, HEADER + 16 * i + 8)?]))
        .collect::<Option<Vec<_>>>()?;
    if points.iter().flatten().any(|v| !v.is_finite() || v.abs() > MAX_COORDINATE) {
        return None;
    }
    Some(Line2d { class, key: key_at(r, 0)?, points })
}

/// A 3D body as triangles: world coordinates in millimetres.
#[derive(Debug)]
struct Body {
    /// Offset of the record in the file: the element id.
    offset: usize,
    record_type: u16,
    form: u16,
    kind: u8,
    /// Form 0x0201: an open surface (§9.3).
    open: bool,
    auxiliary: bool,
    key: Key,
    /// Index into [`Drawing::elements`] when the body is linked (§8.1).
    element: Option<usize>,
    /// Colour number u16@20 of the header, looked up in `animation.cfg` (§10.3).
    color_number: u16,
    /// The `.surf` name in the tail of the record, relative to `design/` (§10.1).
    surface: Option<String>,
    positions: Vec<[f64; 3]>,
    normals: Vec<[f32; 3]>,
    indices: Vec<u32>,
}

/// The data of an element record, class 0xEF or 0x6F (§8.2).
#[derive(Debug, Clone)]
struct Element {
    offset: usize,
    class: u8,
    /// Trailing spaces and NUL removed, and a trailing `\` (catalogue code); may be empty.
    name: String,
    /// Height, thickness, length (m), area (m²), volume (m³) (§8.4).
    quantities: [f64; 5],
    /// Group and subtype (§8.3), when the record holds a valid type mark.
    type_code: Option<(u8, u8)>,
}

/// The categories visible bodies are split into (§8.5), in layer order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Category {
    Walls,
    Slabs,
    Columns,
    Beams,
    Foundations,
    Stairs,
    Other,
    /// The visible bodies of macro placements (§9.4).
    Macros,
}

impl Category {
    const ALL: [Category; 8] = [
        Category::Walls,
        Category::Slabs,
        Category::Columns,
        Category::Beams,
        Category::Foundations,
        Category::Stairs,
        Category::Other,
        Category::Macros,
    ];

    fn name(self) -> &'static str {
        match self {
            Category::Walls => "Walls",
            Category::Slabs => "Slabs",
            Category::Columns => "Columns",
            Category::Beams => "Beams",
            Category::Foundations => "Foundations",
            Category::Stairs => "Stairs",
            Category::Other => "Other",
            Category::Macros => "Macros",
        }
    }

    /// Fixed colours (the file's colour tables are not in the drawing file, §8.5).
    fn color(self) -> [f32; 3] {
        match self {
            Category::Walls => [0.82, 0.79, 0.74],
            Category::Slabs => [0.66, 0.69, 0.73],
            Category::Columns => [0.60, 0.65, 0.75],
            Category::Beams => [0.72, 0.62, 0.52],
            Category::Foundations => [0.52, 0.52, 0.50],
            Category::Stairs => [0.80, 0.70, 0.55],
            Category::Other => DEFAULT_COLOR,
            Category::Macros => MACRO_COLOR,
        }
    }
}

/// The label and category of an element type (§8.3); `None` for codes not in the table.
fn element_type(group: u8, subtype: u8) -> Option<(&'static str, Category)> {
    use Category::*;
    Some(match (group, subtype) {
        (0x01, 0x01) => ("Wall", Walls),
        (0x01, 0x03) => ("Column", Columns),
        (0x01, 0x04) => ("Slab", Slabs),
        (0x01, 0x06) => ("Beam", Beams),
        (0x01, 0x07) => ("Foundation", Foundations),
        (0x01, 0x0A) => ("Chimney", Other),
        (0x01, 0x0E) => ("Pier", Walls),
        (0x01, 0x3E) => ("Finish", Other),
        (0x01, 0x47) => ("Stair part", Stairs),
        (0x09, 0x01) => ("Profile wall", Other),
        (0x09, 0x3D) => ("Room", Other),
        (0x09, 0x5F) => ("Terrain element", Other),
        (0x59, 0x16) => ("Door opening", Other),
        (0x59, 0x17) => ("Window opening", Other),
        (0x59, 0x18) => ("Niche", Other),
        (0x59, 0x19) => ("Wall opening", Other),
        (0x59, 0x1B) => ("Beam recess", Other),
        (0x59, 0x23) => ("Slab opening", Other),
        (0x59, 0x24) => ("Slab recess", Other),
        (0x59, 0x2A) => ("Door element", Other),
        (0x59, 0x2B) => ("Window element", Other),
        (0x59, 0x2D) => ("Recess element", Other),
        _ => return None,
    })
}

/// The key of the record at `p`.
fn key_at(s: &[u8], p: usize) -> Option<Key> {
    Some((u32_at(s, p + 24)?, u16_at(s, p + 12)?))
}

/// Reads the data of an element record (§8.2); `None` without the `||||` mark or when the name
/// and quantities do not fit the record.
fn decode_element(r: &[u8], offset: usize) -> Option<Element> {
    let q = HEADER + 16 * usize::from(u16_at(r, 14)?);
    if q + 144 > r.len() || r[q + 8..q + 12] != *b"||||" {
        return None;
    }
    let name = utf16_name(r, q + 32, 36);
    let mut quantities = [0.0; 5];
    for (i, value) in quantities.iter_mut().enumerate() {
        *value = f64_at(r, q + 104 + 8 * i)?;
    }
    let type_code = (q + 284 <= r.len() && matches!(r[q + 280], 0x81 | 0x41)).then(|| (r[q + 282], r[q + 283]));
    Some(Element { offset, class: r[4], name, quantities, type_code })
}

/// A name of up to `units` UTF-16 units at `at`: cut at the first NUL, trailing spaces removed,
/// and a trailing `\` (catalogue name) removed (§8.2, §9.1, §9.2).
fn utf16_name(r: &[u8], at: usize, units: usize) -> String {
    let units: Vec<u16> = (0..units).map_while(|i| u16_at(r, at + 2 * i)).take_while(|&u| u != 0).collect();
    let name = String::from_utf16_lossy(&units);
    let name = name.trim_end_matches(' ');
    name.strip_suffix('\\').unwrap_or(name).to_string()
}

/// A macro definition (0x5A, §9.1) with the 3D bodies of its 3D representations, in definition
/// coordinates; read once, however often it is placed.
#[derive(Debug)]
struct Macro {
    number: u32,
    name: String,
    bodies: Vec<Body>,
    triangles: usize,
}

/// What became of a macro placement (§9.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placed {
    /// Index into [`Drawing::macros`].
    Resolved(usize),
    /// The macro has no shown 3D body.
    Without3d,
    UnknownMacro,
    /// Too short for its matrix, or a matrix that is not finite.
    Unreadable,
    /// Left out: the file's macro placements reached [`MAX_MACRO_TRIANGLES`].
    OverLimit,
}

/// A macro placement (0xEB, §9.2).
#[derive(Debug)]
struct Placement {
    offset: usize,
    number: u32,
    name: String,
    /// Column-major 3×3 plus translation (§5.2).
    matrix: [f64; 12],
    placed: Placed,
}

/// Reads a placement record (§9.2).
fn decode_placement(r: &[u8], offset: usize) -> Option<Placement> {
    let number = u32_at(r, 28)?;
    let q = HEADER + 16 * usize::from(u16_at(r, 14)?);
    let name = if r.get(q + 8..q + 12) == Some(b"||||") { utf16_name(r, q + 16, 32) } else { String::new() };
    let mut placement = Placement { offset, number, name, matrix: [0.0; 12], placed: Placed::Unreadable };
    if r.len() >= q + 104 {
        let at = r.len() - 104;
        let mut matrix = [0.0; 12];
        for (i, value) in matrix.iter_mut().enumerate() {
            *value = f64_at(r, at + 8 * i)?;
        }
        if matrix.iter().all(|v| v.is_finite()) {
            placement.matrix = matrix;
            placement.placed = Placed::UnknownMacro;
        }
    }
    Some(placement)
}

/// Reads a macro definition (§9.1): its name and the 0xC6 bodies of its 3D representations.
fn decode_macro(d: &[u8], offset: usize) -> Macro {
    let number = u32_at(d, 28).unwrap_or(0);
    let name = if d.get(48..52) == Some(b"||||") { utf16_name(d, 72, 32) } else { String::new() };
    let mut bodies = Vec::new();
    let first = (HEADER..)
        .step_by(8)
        .take_while(|&o| o < HEADER + MACRO_SEARCH && o + HEADER <= d.len())
        .find(|&o| d[o + 4] == 0x50 && d[o + 7] & 0xF0 == 0x50);
    if let Some(first) = first {
        walk(d, first, |r, step| {
            if d[r + 4] == 0x50 && d.get(r + 81) == Some(&1) {
                let representation = &d[r..r + step];
                if REPRESENTATION_BODY <= representation.len() {
                    walk(representation, REPRESENTATION_BODY, |b, step| {
                        if representation[b + 4] == 0xC6 {
                            if let Decoded::Body(body) = decode_body(&representation[b..b + step], offset + r + b) {
                                bodies.push(body);
                            }
                        }
                    });
                }
            }
        });
    }
    let triangles = bodies.iter().map(|b| b.indices.len() / 3).sum();
    Macro { number, name, bodies, triangles }
}

/// Walks the records inside a macro definition (`s`, from `start`) by §3 items 1–3 (padding,
/// attribute blocks, records), calling `record` with the offset and step of each record; stops
/// at an invalid header. Inside a macro byte 0 is not checked: nested records mostly have its
/// low bits set (§9.1).
fn walk(s: &[u8], start: usize, mut record: impl FnMut(usize, usize)) {
    let mut p = start;
    while p + 8 <= s.len() {
        if is_padding(s, p) {
            p += 8;
        } else if is_attribute(s, p) {
            p += 8 + (2 * usize::from(s[p + 2])).div_ceil(8) * 8;
        } else if let Some(step) = (p + HEADER <= s.len() && s[p + 7] & 0xF0 == 0x50).then(|| step_of(s, p)).flatten() {
            record(p, step);
            p += step;
        } else {
            break;
        }
    }
}

/// A placement matrix (§9.2), prepared once per placement: points map by the matrix, normals by
/// its cofactor matrix (det · M⁻ᵀ), and a mirroring matrix turns the triangles round.
struct Transform {
    m: [f64; 12],
    /// Columns of the cofactor matrix, times the sign of the determinant.
    k: [[f64; 3]; 3],
    mirror: bool,
}

impl Transform {
    fn new(m: &[f64; 12]) -> Self {
        let c = [[m[0], m[1], m[2]], [m[3], m[4], m[5]], [m[6], m[7], m[8]]];
        let k = [cross(c[1], c[2]), cross(c[2], c[0]), cross(c[0], c[1])];
        let mirror = dot(c[0], k[0]) < 0.0;
        let k = if mirror { k.map(|col| col.map(|v| -v)) } else { k };
        Self { m: *m, k, mirror }
    }

    fn point(&self, p: &[f64; 3]) -> [f64; 3] {
        let m = &self.m;
        [
            m[0] * p[0] + m[3] * p[1] + m[6] * p[2] + m[9],
            m[1] * p[0] + m[4] * p[1] + m[7] * p[2] + m[10],
            m[2] * p[0] + m[5] * p[1] + m[8] * p[2] + m[11],
        ]
    }

    fn normal(&self, n: &[f32; 3]) -> [f32; 3] {
        let k = &self.k;
        let (x, y, z) = (f64::from(n[0]), f64::from(n[1]), f64::from(n[2]));
        let t = [
            k[0][0] * x + k[1][0] * y + k[2][0] * z,
            k[0][1] * x + k[1][1] * y + k[2][1] * z,
            k[0][2] * x + k[1][2] * y + k[2][2] * z,
        ];
        let length = dot(t, t).sqrt();
        if length > 0.0 && length.is_finite() { t.map(|v| (v / length) as f32) } else { *n }
    }

    /// Appends a placed body to a layer (positions to metres).
    fn append(&self, layer: &mut Layer, body: &Body) -> Result<(), String> {
        let base = u32::try_from(layer.vertex_count()).map_err(|_| "Allplan: too many vertices")?;
        u32::try_from(body.positions.len()).ok().and_then(|n| base.checked_add(n)).ok_or("Allplan: too many vertices")?;
        layer.positions.reserve(3 * body.positions.len());
        for p in &body.positions {
            let w = self.point(p);
            layer.positions.extend_from_slice(&[(w[0] * MM) as f32, (w[1] * MM) as f32, (w[2] * MM) as f32]);
        }
        // The vertices of a face share its normal: map each run once.
        layer.normals.reserve(3 * body.normals.len());
        let mut last: Option<([f32; 3], [f32; 3])> = None;
        for n in &body.normals {
            let mapped = match last {
                Some((from, to)) if from == *n => to,
                _ => {
                    let to = self.normal(n);
                    last = Some((*n, to));
                    to
                }
            };
            layer.normals.extend_from_slice(&mapped);
        }
        layer.indices.reserve(body.indices.len());
        for tri in body.indices.chunks_exact(3) {
            let tri = if self.mirror { [tri[0], tri[2], tri[1]] } else { [tri[0], tri[1], tri[2]] };
            layer.indices.extend_from_slice(&[base + tri[0], base + tri[1], base + tri[2]]);
        }
        Ok(())
    }
}

/// A placed macro body in millimetres (tests: extents and orientation).
#[cfg(test)]
fn place(body: &Body, m: &[f64; 12]) -> (Vec<[f64; 3]>, Vec<[f32; 3]>, Vec<u32>) {
    let t = Transform::new(m);
    let positions = body.positions.iter().map(|p| t.point(p)).collect();
    let normals = body.normals.iter().map(|n| t.normal(n)).collect();
    let mut indices = body.indices.clone();
    if t.mirror {
        indices.chunks_exact_mut(3).for_each(|tri| tri.swap(1, 2));
    }
    (positions, normals, indices)
}

fn u16_at(d: &[u8], o: usize) -> Option<u16> {
    d.get(o..o.checked_add(2)?).map(|b| u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o.checked_add(4)?).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn i32_at(d: &[u8], o: usize) -> Option<i32> {
    u32_at(d, o).map(|v| v as i32)
}

fn f64_at(d: &[u8], o: usize) -> Option<f64> {
    d.get(o..o.checked_add(8)?).map(|b| f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}

/// A whole number stored as a double (topology of §5.3).
fn whole(value: f64) -> Option<i64> {
    (value.is_finite() && value.fract() == 0.0 && value.abs() < 1e15).then_some(value as i64)
}

/// Reads the header, the name and the element stream.
fn parse(data: &[u8]) -> Result<Drawing, String> {
    if data.len() < 12 || data[0..2] != [0x52, 0x02] {
        return Err("Allplan: not an Allplan drawing file (.ndw)".into());
    }
    if u16_at(data, 2) == Some(0) {
        return Err("Allplan: this is a layout (.npl); only drawing files (.ndw) are supported".into());
    }
    if data[2..4] != [0x02, 0x00] || u16_at(data, 4) != Some(PAGE as u16) || data[10..12] != [0x08, 0x08] {
        return Err("Allplan: unsupported file header; expected an Allplan drawing file (.ndw)".into());
    }
    let pages = usize::from(u16_at(data, 8).unwrap_or(0) / 2);
    let name = data
        .get(NAME_OFFSET..data.len().min(NAME_OFFSET + 2 * NAME_UNITS))
        .map(|bytes| {
            let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            String::from_utf16_lossy(&units).trim_end_matches([' ', '\0']).to_string()
        })
        .unwrap_or_default();
    let mut drawing = Drawing { name, pages, ..Drawing::default() };
    // The element stream ends at T (or at the end of a truncated file).
    let end = (12 + pages * PAGE).min(data.len());
    let stream = &data[..end];
    let mut p = STREAM_START;
    let mut runs = None;
    // Per key the first element record with data (§8.1); u32@24 = 0 is no key.
    let mut by_key: HashMap<Key, usize> = HashMap::new();
    let mut definitions: HashMap<u32, (usize, usize)> = HashMap::new();
    // Keys of all 3D body records, shown or not: 2D records of such an element are not drawn (§12.1).
    let mut body_keys = std::collections::HashSet::new();
    let mut lines = Vec::new();
    while p + 8 <= stream.len() {
        if is_padding(stream, p) {
            p += 8;
        } else if is_attribute(stream, p) {
            p += 8 + (2 * usize::from(stream[p + 2])).div_ceil(8) * 8;
        } else if let Some(step) = record_step(stream, p) {
            match stream[p + 4] {
                0xC6 => {
                    body_keys.extend(key_at(stream, p));
                    drawing.body(&stream[p..p + step], p);
                }
                0x82 | 0x84 => match decode_line(&stream[p..p + step]) {
                    Some(line) => lines.push(line),
                    None => drawing.lines_unreadable += 1,
                },
                0xEF | 0x6F => {
                    if let Some(key) = key_at(stream, p).filter(|key| key.0 != 0 && !by_key.contains_key(key)) {
                        if let Some(element) = decode_element(&stream[p..p + step], p) {
                            drawing.elements.push(element);
                            by_key.insert(key, drawing.elements.len() - 1);
                        }
                    }
                }
                // The first definition per macro number (§9.1); read when a placement needs it.
                0x5A => {
                    if let Some(number) = u32_at(stream, p + 28) {
                        definitions.entry(number).or_insert((p, step));
                    }
                }
                0xEB => drawing.placements.extend(decode_placement(&stream[p..p + step], p)),
                _ => {}
            }
            p += step;
        } else {
            // Resynchronise on the next strictly valid header, or stop.
            let runs = runs.get_or_insert_with(|| PaddingRuns::new(stream));
            match (p + 8..).step_by(8).take_while(|q| q + HEADER <= stream.len()).find(|&q| strict_resync(stream, q, runs)) {
                Some(q) => p = q,
                None => break,
            }
        }
    }
    // Element records may come before or after their body.
    for body in &mut drawing.bodies {
        body.element = if body.key.0 == 0 { None } else { by_key.get(&body.key).copied() };
    }
    for line in lines {
        if line.key.0 != 0 && body_keys.contains(&line.key) {
            drawing.lines_with_3d += 1;
        } else {
            drawing.lines.push(line);
        }
    }
    // Placements may come before or after their definition; each definition is read once.
    let mut read: HashMap<u32, Option<usize>> = HashMap::new();
    let mut triangles = 0usize;
    for placement in &mut drawing.placements {
        if placement.placed == Placed::Unreadable {
            continue;
        }
        let Some(&(at, step)) = definitions.get(&placement.number) else { continue };
        let index = *read.entry(placement.number).or_insert_with(|| {
            let definition = decode_macro(&stream[at..at + step], at);
            (!definition.bodies.is_empty()).then(|| {
                drawing.macros.push(definition);
                drawing.macros.len() - 1
            })
        });
        placement.placed = match index {
            None => Placed::Without3d,
            Some(i) if triangles + drawing.macros[i].triangles > MAX_MACRO_TRIANGLES => Placed::OverLimit,
            Some(i) => {
                triangles += drawing.macros[i].triangles;
                Placed::Resolved(i)
            }
        };
    }
    Ok(drawing)
}

fn is_padding(s: &[u8], p: usize) -> bool {
    s.get(p..p + 4).is_some_and(|b| b == [0; 4])
}

fn is_attribute(s: &[u8], p: usize) -> bool {
    p + 8 <= s.len() && s[p + 6] == 0xBE && s[p + 7] == 0x00
}

/// The full length of the record at `p` by the formula of §3.
fn record_len(s: &[u8], p: usize) -> Option<u64> {
    let n16 = u64::from(u16_at(s, p + 14)?);
    let n8 = u64::from(u16_at(s, p + 18)?) | u64::from(*s.get(p + 17)?) << 16;
    Some(HEADER as u64 + 16 * n16 + 8 * n8)
}

/// How far the reader moves past the record at `p`: 40 bytes into a container (0x64, 0x65) and
/// past a 0x62 record, the full length otherwise; `None` when the length does not fit the stream.
fn step_of(s: &[u8], p: usize) -> Option<usize> {
    if p + HEADER > s.len() {
        return None;
    }
    if matches!(s[p + 4], 0x62 | 0x64 | 0x65) {
        return Some(HEADER);
    }
    let len = usize::try_from(record_len(s, p)?).ok()?;
    (p.checked_add(len)? <= s.len()).then_some(len)
}

/// An element record as the reader accepts it in sequence (§3 item 3 and 4).
fn record_step(s: &[u8], p: usize) -> Option<usize> {
    if p + HEADER > s.len() || s[p] & 7 != 0 || s[p + 7] & 0xF0 != 0x50 {
        return None;
    }
    step_of(s, p)
}

/// A strictly valid header (§3 item 4), without looking further.
fn strict_header(s: &[u8], q: usize) -> Option<usize> {
    if q + HEADER > s.len() {
        return None;
    }
    let ok = s[q] != 0
        && s[q] & 7 == 0
        && s[q + 7] & 0xF0 == 0x50
        && matches!(s[q + 6], 0x00 | 0x04 | 0x08 | 0x38)
        && KNOWN_CLASSES.contains(&s[q + 4]);
    if ok { step_of(s, q) } else { None }
}

/// Where each run of padding in the stream ends, for the resync lookahead: without it, many
/// candidate headers pointing into one long run would each walk the whole run.
struct PaddingRuns {
    /// Per 8-byte slot from [`STREAM_START`]: the slot where a padding walk from it stops.
    stop: Vec<u32>,
}

impl PaddingRuns {
    fn new(s: &[u8]) -> Self {
        let slots = if s.len() >= STREAM_START + 8 { (s.len() - STREAM_START - 8) / 8 + 1 } else { 0 };
        let mut stop = vec![0u32; slots];
        let mut next = slots as u32;
        for i in (0..slots).rev() {
            if !is_padding(s, STREAM_START + 8 * i) {
                next = i as u32;
            }
            stop[i] = next;
        }
        Self { stop }
    }

    /// The first position from `p` (on the 8-byte grid of the stream) that is not padding.
    fn end(&self, s: &[u8], p: usize) -> usize {
        match p.checked_sub(STREAM_START).filter(|d| d % 8 == 0).and_then(|d| self.stop.get(d / 8)) {
            Some(&slot) => STREAM_START + 8 * slot as usize,
            // Off the grid (records keep it, so this does not happen): walk.
            None => {
                let mut next = p;
                while next + 8 <= s.len() && next - p <= PAGE && is_padding(s, next) {
                    next += 8;
                }
                next
            }
        }
    }
}

/// A strictly valid header that is followed by the stream end, an attribute block or another
/// strictly valid header (§3 item 4). Padding that runs to the stream end is always fine; padding
/// followed by data may be one page long at most.
fn strict_resync(s: &[u8], q: usize, runs: &PaddingRuns) -> bool {
    let Some(step) = strict_header(s, q) else { return false };
    let next = runs.end(s, q + step);
    if next + 8 > s.len() {
        return true;
    }
    if next - (q + step) > PAGE {
        return false;
    }
    is_attribute(s, next) || strict_header(s, next).is_some()
}

/// The outcome of decoding one 0xC6 record.
enum Decoded {
    Body(Body),
    Clip,
    NoFaces,
    Unreadable,
}

impl Drawing {
    fn body(&mut self, record: &[u8], offset: usize) {
        match decode_body(record, offset) {
            Decoded::Body(body) => self.bodies.push(body),
            Decoded::Clip => self.skipped_clip += 1,
            Decoded::NoFaces => self.skipped_no_faces += 1,
            Decoded::Unreadable => self.skipped_unreadable += 1,
        }
    }
}

/// A face: its loops of vertex indices, the first one the outer boundary.
type Face = Vec<Vec<usize>>;

/// Decodes a 3D body record (§5); `record` is the whole record.
fn decode_body(r: &[u8], offset: usize) -> Decoded {
    match decode_body_inner(r, offset) {
        Some(decoded) => decoded,
        None => Decoded::Unreadable,
    }
}

fn decode_body_inner(r: &[u8], offset: usize) -> Option<Decoded> {
    if r.len() < 64 || r[40] != 0x1E || r[42] != 0x03 {
        return None;
    }
    let nm = usize::from(r[41]);
    let kind = r[43];
    if !matches!(nm, 0 | 12) || !matches!(kind, 0 | 1 | 4) {
        return None;
    }
    let mut m = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];
    for (i, value) in m.iter_mut().enumerate().take(nm) {
        *value = f64_at(r, 64 + 8 * i)?;
    }
    let h = 64 + 8 * nm;
    let form = u16_at(r, h)?;
    if form == FORM_CLIP {
        return Some(Decoded::Clip);
    }
    let v = i64::from(i32_at(r, h + 4)?);
    let e = usize::try_from(u32_at(r, h + 8)?).ok()?;
    let f = usize::try_from(u32_at(r, h + 12)?).ok()?;
    let l = usize::try_from(u32_at(r, h + 16)?).ok()?;
    if f == 0 || v == 0 {
        return Some(Decoded::NoFaces);
    }
    // An open surface (§9.3): explicit faces, drawn as they are, never turned round.
    let open = form == FORM_OPEN;
    // Kind 0 is a prism of form 0x0301 (§5.4) or an open surface; other forms are not known yet.
    if kind == 0 && !(open || form == 0x0301 && v < 0) {
        return None;
    }
    let count = usize::try_from(v.unsigned_abs()).ok()?;
    let stored = if kind == 4 { count.checked_add(1)? } else { count };
    // Kind 0 has no 8 constant bytes before the vertices (§5.1, v0.2.1).
    let vertices_at = h + if kind == 0 { 24 } else { 32 };
    let bbox_at = vertices_at.checked_add(stored.checked_mul(24)?)?;
    let topology_at = bbox_at.checked_add(48)?;
    if topology_at > r.len() {
        return None;
    }
    let mut points = Vec::with_capacity(stored);
    for i in 0..stored {
        let o = vertices_at + 24 * i;
        let (x, y, z) = (f64_at(r, o)?, f64_at(r, o + 8)?, f64_at(r, o + 16)?);
        // Column-major (§5.2, v0.2.1).
        let world = [
            m[0] * x + m[3] * y + m[6] * z + m[9],
            m[1] * x + m[4] * y + m[7] * z + m[10],
            m[2] * x + m[5] * y + m[8] * z + m[11],
        ];
        points.push(world);
    }
    // Slot 0 of an explicit body is unused (1-based indices).
    let used = if kind == 4 { 1 } else { 0 };
    if points[used..].iter().flatten().any(|c| !c.is_finite()) {
        return None;
    }
    // Kind 4 keeps slot 0 unused; an open kind-0 surface indexes its |V| vertices from 1 (§9.3).
    let faces = match (kind, open) {
        (4, _) => explicit_faces(r, topology_at, count, e, f, l, 0)?,
        (0, true) => explicit_faces(r, topology_at, count, e, f, l, 1)?,
        _ => prism_faces(v, count)?,
    };
    // The tail follows the bounding box, or the topology of an explicit body (§10.1).
    let explicit = kind == 4 || (kind == 0 && open);
    let tail_at = if explicit { topology_at.saturating_add(8usize.saturating_mul(e.saturating_add(f).saturating_add(l))) } else { topology_at };
    let surface = r.get(tail_at..).and_then(surface_name);
    if faces.is_empty() {
        return Some(Decoded::NoFaces);
    }
    let loops: Vec<Vec<Vec<[f64; 3]>>> =
        faces.iter().map(|face| face.iter().map(|ring| ring.iter().map(|&i| points[i]).collect()).collect()).collect();
    // One orientation per body (§5.7, answer 6): when the signed volume, a fan over the outer
    // loop of every face, is negative, all faces turn round. Taken about the middle of the
    // bounding box: the same for a closed body, but precise far from the origin.
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in &points[used..] {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let center: [f64; 3] = std::array::from_fn(|k| (lo[k] + hi[k]) / 2.0);
    let rel = |p: [f64; 3]| -> [f64; 3] { std::array::from_fn(|k| p[k] - center[k]) };
    let volume: f64 = loops
        .iter()
        .map(|face| {
            let outer = &face[0];
            (1..outer.len().saturating_sub(1))
                .map(|i| dot(rel(outer[0]), cross(rel(outer[i]), rel(outer[i + 1]))) / 6.0)
                .sum::<f64>()
        })
        .sum();
    let flip = !open && volume < 0.0;
    let mut body = Body {
        offset,
        record_type: u16_at(r, 4)?,
        form,
        kind,
        open,
        auxiliary: r[1] & 1 == 1,
        key: key_at(r, 0)?,
        element: None,
        color_number: u16_at(r, 20)?,
        surface,
        positions: Vec::new(),
        normals: Vec::new(),
        indices: Vec::new(),
    };
    for face in &loops {
        triangulate(face, flip, &mut body);
    }
    Some(if body.indices.is_empty() { Decoded::NoFaces } else { Decoded::Body(body) })
}

/// Faces of a prism (§5.4): `count` vertices, the bottom ring then the top ring.
fn prism_faces(v: i64, count: usize) -> Option<Vec<Face>> {
    if count % 2 != 0 || count < 6 {
        return None;
    }
    let h = count / 2;
    let bottom: Vec<usize> = (0..h).rev().collect();
    let top: Vec<usize> = (h..count).collect();
    let mut faces = vec![vec![bottom], vec![top]];
    if v < 0 {
        faces.extend((0..h).map(|i| {
            let j = (i + 1) % h;
            vec![vec![i, j, h + j, h + i]]
        }));
    }
    Some(faces)
}

/// Faces of an explicit body (§5.3): `e` edges, `f` face sizes and `l` loop entries after the
/// bounding box; vertex indices run 1..=`count` and point at the stored vertex `index − shift`.
fn explicit_faces(r: &[u8], at: usize, count: usize, e: usize, f: usize, l: usize, shift: usize) -> Option<Vec<Face>> {
    let sizes_at = at.checked_add(e.checked_mul(8)?)?;
    let entries_at = sizes_at.checked_add(f.checked_mul(8)?)?;
    let end = entries_at.checked_add(l.checked_mul(8)?)?;
    if end > r.len() {
        return None;
    }
    let mut edges = Vec::with_capacity(e);
    for i in 0..e {
        let a = usize::try_from(i32_at(r, at + 8 * i)?).ok()?;
        let b = usize::try_from(i32_at(r, at + 8 * i + 4)?).ok()?;
        if !(1..=count).contains(&a) || !(1..=count).contains(&b) {
            return None;
        }
        edges.push((a - shift, b - shift));
    }
    let mut sizes = Vec::with_capacity(f);
    for i in 0..f {
        sizes.push(usize::try_from(whole(f64_at(r, sizes_at + 8 * i)?)?).ok()?);
    }
    if sizes.iter().try_fold(0usize, |sum, &n| sum.checked_add(n))? != l {
        return None;
    }
    let mut oriented = Vec::with_capacity(l);
    for i in 0..l {
        let k = whole(f64_at(r, entries_at + 8 * i)?)?;
        let edge = *edges.get(usize::try_from(k.unsigned_abs()).ok()?.checked_sub(1)?)?;
        oriented.push(if k > 0 { edge } else { (edge.1, edge.0) });
    }
    let mut faces = Vec::with_capacity(f);
    let mut next = 0;
    for size in sizes {
        let entries = &oriented[next..next + size];
        next += size;
        // A loop closes when an edge ends where its first edge started.
        let mut loops = Vec::new();
        let mut ring = Vec::new();
        let mut start = None;
        for &(a, b) in entries {
            let first = *start.get_or_insert(a);
            ring.push(a);
            if b == first {
                loops.push(std::mem::take(&mut ring));
                start = None;
            }
        }
        // A face with an open loop is left out.
        if ring.is_empty() && !loops.is_empty() {
            faces.push(loops);
        }
    }
    Some(faces)
}

/// Newell normal of a ring.
fn newell(ring: &[[f64; 3]]) -> [f64; 3] {
    let mut n = [0.0; 3];
    for (i, a) in ring.iter().enumerate() {
        let b = ring[(i + 1) % ring.len()];
        n[0] += (a[1] - b[1]) * (a[2] + b[2]);
        n[1] += (a[2] - b[2]) * (a[0] + b[0]);
        n[2] += (a[0] - b[0]) * (a[1] + b[1]);
    }
    n
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// Triangulates a face (outer loop plus holes) into `body`, facing along the Newell normal of
/// its outer loop, or the other way when `flip`.
fn triangulate(loops: &[Vec<[f64; 3]>], flip: bool, body: &mut Body) {
    let mut rings: Vec<Vec<[f64; 3]>> = Vec::with_capacity(loops.len());
    for (i, ring) in loops.iter().enumerate() {
        let mut clean: Vec<[f64; 3]> = Vec::with_capacity(ring.len());
        for &p in ring {
            if clean.last() != Some(&p) {
                clean.push(p);
            }
        }
        while clean.len() > 1 && clean.first() == clean.last() {
            clean.pop();
        }
        if clean.len() >= 3 {
            rings.push(clean);
        } else if i == 0 {
            return;
        }
    }
    let Some(outer) = rings.first() else { return };
    // Work relative to the first point, for precision far from the origin.
    let anchor = outer[0];
    let local: Vec<Vec<[f64; 3]>> =
        rings.iter().map(|ring| ring.iter().map(|p| std::array::from_fn(|k| p[k] - anchor[k])).collect()).collect();
    let mut normal = newell(&local[0]);
    let length = dot(normal, normal).sqrt();
    if !length.is_finite() || length < 1e-12 {
        return;
    }
    if flip {
        normal = normal.map(|c| -c);
    }
    let unit = normal.map(|c| c / length);
    let axis = (0..3).max_by(|&a, &b| normal[a].abs().total_cmp(&normal[b].abs())).unwrap_or(2);
    let (u, w) = match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let points: Vec<[f64; 3]> = local.iter().flatten().copied().collect();
    let flat: Vec<f64> = points.iter().flat_map(|p| [p[u], p[w]]).collect();
    let mut holes = Vec::new();
    let mut start = 0;
    for ring in &local[..local.len() - 1] {
        start += ring.len();
        holes.push(start);
    }
    let Ok(triangles) = earcutr::earcut(&flat, &holes, 2) else { return };
    let Some(base) = u32::try_from(body.positions.len()).ok().filter(|b| u32::try_from(points.len()).ok().and_then(|n| b.checked_add(n)).is_some()) else {
        return;
    };
    let mut indices = Vec::with_capacity(triangles.len());
    for tri in triangles.chunks_exact(3) {
        let [a, b, c] = [points[tri[0]], points[tri[1]], points[tri[2]]];
        let ab: [f64; 3] = std::array::from_fn(|k| b[k] - a[k]);
        let ac: [f64; 3] = std::array::from_fn(|k| c[k] - a[k]);
        let side = dot(cross(ab, ac), normal);
        if side == 0.0 || !side.is_finite() {
            continue;
        }
        let ids = if side > 0.0 { [tri[0], tri[1], tri[2]] } else { [tri[0], tri[2], tri[1]] };
        indices.extend(ids.map(|i| base + i as u32));
    }
    if !indices.is_empty() {
        body.indices.extend(indices);
        body.positions.extend(points.iter().map(|p| std::array::from_fn(|k| p[k] + anchor[k])));
        body.normals.extend(std::iter::repeat_n(unit.map(|c| c as f32), points.len()));
    }
}

/// The first `.surf` name in the tail of a body (§10.1): at an even offset, a run of at least
/// three UTF-16 name characters ending in NUL whose text ends in `.surf`.
fn surface_name(tail: &[u8]) -> Option<String> {
    // Name characters: printable ASCII and Latin letters. A stricter class than "≥ 0x20", so that
    // the high half of a double right before the name does not become part of it.
    let name_char = |u: u16| (0x20..=0x7E).contains(&u) || (0xA0..=0x24F).contains(&u);
    let units: Vec<u16> = tail.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let mut i = 0;
    while i < units.len() {
        if !name_char(units[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < units.len() && name_char(units[i]) {
            i += 1;
        }
        if units.get(i) == Some(&0) && i - start >= 3 {
            let text = String::from_utf16_lossy(&units[start..i]);
            if text.to_lowercase().ends_with(".surf") {
                return Some(text);
            }
        }
    }
    None
}

/// Files of the project folder (§10), found case-insensitively per path component.
trait ProjectFiles {
    /// The bytes of the file at `path` (components below the project folder), if it is there.
    fn read(&mut self, path: &[&str]) -> Option<Vec<u8>>;
}

/// Largest preview side of a `.surf` that is averaged (§10.2; the samples have 120).
const MAX_PREVIEW: usize = 4096;
/// Most pixels read per side of the middle square of a preview (§10.2).
const PREVIEW_SAMPLES: usize = 256;

/// Largest project file read (a `.surf` or `animation.cfg` is a few kB).
const MAX_PROJECT_FILE: u64 = 16 << 20;

/// The project folder on disk: the folder of the drawing file. Each directory is listed once.
struct Folder {
    root: std::path::PathBuf,
    listings: HashMap<std::path::PathBuf, HashMap<String, std::path::PathBuf>>,
}

impl Folder {
    fn new(root: &std::path::Path) -> Self {
        // A drawing file named without a folder lies in the current one.
        let root = if root.as_os_str().is_empty() { std::path::Path::new(".") } else { root };
        Self { root: root.to_path_buf(), listings: HashMap::new() }
    }
}

impl ProjectFiles for Folder {
    fn read(&mut self, path: &[&str]) -> Option<Vec<u8>> {
        let at = self.find(path)?;
        let metadata = std::fs::metadata(&at).ok()?;
        if !metadata.is_file() || metadata.len() > MAX_PROJECT_FILE {
            return None;
        }
        std::fs::read(&at).ok()
    }
}

/// The path components of a `.surf` name below `design/`; `None` for names that would leave it.
fn surface_path(name: &str) -> Option<Vec<&str>> {
    let parts: Vec<&str> = name.split(['\\', '/']).collect();
    let bad = |part: &&str| part.is_empty() || *part == "." || *part == ".." || part.contains(':');
    (!parts.iter().any(bad)).then_some(parts)
}

/// The colour of a `.surf` file (§10.2) and whether it has a texture.
fn surface_color(data: &[u8]) -> Option<([u8; 3], bool)> {
    let tiff = Tiff::new(data)?;
    let rgb = tiff.bytes(65001).filter(|b| b.len() >= 3).map(|b| [b[0], b[1], b[2]])?;
    let texture = tiff.bytes(65019).is_some_and(|b| {
        let units: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units).trim_matches(['\0', ' ']).chars().next().is_some()
    });
    if !texture {
        return Some((rgb, false));
    }
    // With a texture, 65001 is only a tint: the middle of the palette preview approximates the
    // colour; other preview kinds keep 65001.
    Some((tiff.preview_middle().unwrap_or(rgb), true))
}

/// The first IFD of a little-endian TIFF, enough for the tags of a `.surf` (§10.2).
struct Tiff<'a> {
    data: &'a [u8],
    /// Tag → (type, count, offset of the value field).
    entries: HashMap<u16, (u16, u32, usize)>,
}

impl<'a> Tiff<'a> {
    fn new(data: &'a [u8]) -> Option<Self> {
        if data.get(0..4) != Some(b"II*\0") {
            return None;
        }
        let ifd = usize::try_from(u32_at(data, 4)?).ok()?;
        let count = usize::from(u16_at(data, ifd)?);
        let mut entries = HashMap::new();
        for i in 0..count {
            let at = ifd.checked_add(2 + 12 * i)?;
            let (tag, kind, n) = (u16_at(data, at)?, u16_at(data, at + 2)?, u32_at(data, at + 4)?);
            entries.entry(tag).or_insert((kind, n, at + 8));
        }
        Some(Self { data, entries })
    }

    /// The raw value bytes of a tag (inline when they fit in four bytes).
    fn bytes(&self, tag: u16) -> Option<&'a [u8]> {
        let &(kind, n, field) = self.entries.get(&tag)?;
        let size = match kind {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 => 4,
            _ => return None,
        };
        let len = usize::try_from(n).ok()?.checked_mul(size)?;
        let at = if len <= 4 { field } else { usize::try_from(u32_at(self.data, field)?).ok()? };
        self.data.get(at..at.checked_add(len)?)
    }

    /// The numbers of a SHORT or LONG tag.
    fn numbers(&self, tag: u16) -> Option<Vec<u32>> {
        let bytes = self.bytes(tag)?;
        match self.entries.get(&tag)?.0 {
            3 => Some(bytes.chunks_exact(2).map(|c| u32::from(u16::from_le_bytes([c[0], c[1]]))).collect()),
            4 => Some(bytes.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()),
            _ => None,
        }
    }

    fn number(&self, tag: u16, default: u32) -> Option<u32> {
        match self.entries.contains_key(&tag) {
            true => self.numbers(tag)?.first().copied(),
            false => Some(default),
        }
    }

    /// The mean colour of the middle square of an uncompressed 8-bit palette image: columns
    /// 3w/8 .. 5w/8 − 1 and rows 3h/8 .. 5h/8 − 1 (§10.2). Bounded: no preview above
    /// [`MAX_PREVIEW`] pixels a side, and at most [`PREVIEW_SAMPLES`]² pixels read, at a fixed step.
    fn preview_middle(&self) -> Option<[u8; 3]> {
        let w = usize::try_from(self.number(256, 0)?).ok()?;
        let h = usize::try_from(self.number(257, 0)?).ok()?;
        let supported = self.number(259, 1)? == 1
            && self.number(262, 2)? == 3
            && self.number(277, 1)? == 1
            && self.numbers(258).is_some_and(|bits| !bits.is_empty() && bits.iter().all(|&b| b == 8));
        if !supported || w == 0 || h == 0 || w > MAX_PREVIEW || h > MAX_PREVIEW {
            return None;
        }
        let map = self.numbers(320).filter(|m| m.len() >= 768)?;
        let offsets = self.numbers(273)?;
        let counts = self.numbers(279);
        let rows = usize::try_from(self.number(278, u32::MAX)?).ok()?.clamp(1, h);
        let (x0, x1, y0, y1) = (3 * w / 8, 5 * w / 8, 3 * h / 8, 5 * h / 8);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let step = |from: usize, to: usize| (to - from).div_ceil(PREVIEW_SAMPLES);
        let (dx, dy) = (step(x0, x1), step(y0, y1));
        let mut sum = [0u64; 3];
        let mut n = 0u64;
        for y in (y0..y1).step_by(dy) {
            let strip = y / rows;
            let start = usize::try_from(*offsets.get(strip)?).ok()?;
            let end = match &counts {
                Some(c) => start.checked_add(usize::try_from(*c.get(strip)?).ok()?)?,
                None => self.data.len(),
            };
            for x in (x0..x1).step_by(dx) {
                let at = start.checked_add((y % rows).checked_mul(w)?.checked_add(x)?)?;
                if at >= end {
                    return None;
                }
                let index = usize::from(*self.data.get(at)?);
                for (k, total) in sum.iter_mut().enumerate() {
                    *total += u64::from(map[256 * k + index] >> 8);
                }
                n += 1;
            }
        }
        Some(sum.map(|total| ((total + n / 2) / n) as u8))
    }
}

/// The colour table of `animation.cfg` (§10.3): colour number → RGB. The u32 with the number
/// follows each name after 2 or 4 bytes, by file layout, not by the version field: the gap that
/// matches for most names in the file comes first (an entry like `*0` can match at both).
fn color_table(data: &[u8]) -> HashMap<u16, [u8; 3]> {
    let digit = |at: usize| data.get(at..at + 2).is_some_and(|u| u[0].is_ascii_digit() && u[1] == 0);
    let mut names = Vec::new();
    for name in 2..data.len().saturating_sub(3) {
        if data[name] != b'*' || data[name + 1] != 0 || !digit(name + 2) {
            continue;
        }
        let Some(len) = u16_at(data, name - 2).map(usize::from) else { continue };
        if len < 4 || len % 2 != 0 || len > 14 {
            continue;
        }
        let end = name + len;
        if !(name + 2..end).step_by(2).all(digit) {
            continue;
        }
        let number = (name + 2..end).step_by(2).fold(0u32, |n, at| n * 10 + u32::from(data[at] - b'0'));
        names.push((end, number));
    }
    let matches = |gap: usize| names.iter().filter(|&&(end, n)| u32_at(data, end + gap) == Some(n)).count();
    let gaps: [usize; 2] = if matches(4) > matches(2) { [4, 2] } else { [2, 4] };
    let mut table = HashMap::new();
    for (end, number) in names {
        let Ok(key) = u16::try_from(number) else { continue };
        for gap in gaps {
            if u32_at(data, end + gap) == Some(number) {
                if let Some(rgb) = data.get(end + gap + 4..end + gap + 7) {
                    table.entry(key).or_insert([rgb[0], rgb[1], rgb[2]]);
                }
                break;
            }
        }
    }
    table
}

/// Where the colour of a body comes from (§10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Paint {
    Surface { rgb: [u8; 3], textured: bool },
    Number([u8; 3]),
    /// The category colour.
    Fallback,
}

impl Paint {
    fn rgba(self, fallback: [f32; 3]) -> [u8; 4] {
        let [r, g, b] = match self {
            Paint::Surface { rgb, .. } | Paint::Number(rgb) => rgb,
            Paint::Fallback => fallback.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8),
        };
        [r, g, b, 255]
    }
}

/// How many bodies took their colour from which source.
#[derive(Debug, Default, Clone, Copy)]
struct Tally {
    textured: usize,
    plain: usize,
    number: usize,
    fallback: usize,
}

impl Tally {
    fn add(&mut self, paint: Paint) {
        match paint {
            Paint::Surface { textured: true, .. } => self.textured += 1,
            Paint::Surface { .. } => self.plain += 1,
            Paint::Number(_) => self.number += 1,
            Paint::Fallback => self.fallback += 1,
        }
    }

    fn merge(&mut self, other: Tally) {
        self.textured += other.textured;
        self.plain += other.plain;
        self.number += other.number;
        self.fallback += other.fallback;
    }

    fn text(&self) -> String {
        format!(
            "surface {} (textured {}), colour number {}, category colour {}",
            self.textured + self.plain,
            self.textured,
            self.number,
            self.fallback
        )
    }
}

/// The colours of the bodies of a drawing file from its project folder (§10).
struct Painted {
    /// Per drawing body.
    bodies: Vec<Paint>,
    /// Per macro, per body.
    macros: Vec<Vec<Paint>>,
    /// Some colour came from the project files: the layers get vertex colours.
    active: bool,
    /// The visible bodies of the drawing file, and the macro bodies.
    visible: Tally,
    macro_bodies: Tally,
    info: Vec<(String, String)>,
}

/// Colours from a project folder (§10): `animation.cfg` is read once, each surface once, for any
/// number of drawing files.
struct Painter<'a> {
    files: &'a mut dyn ProjectFiles,
    /// Version field and size of `animation.cfg`, when there is one.
    config: Option<(u16, usize)>,
    table: HashMap<u16, [u8; 3]>,
    surfaces: HashMap<String, Option<([u8; 3], bool)>>,
}

impl<'a> Painter<'a> {
    fn new(files: &'a mut dyn ProjectFiles) -> Self {
        let config = files.read(&["animation.cfg"]);
        let table = config.as_deref().map(color_table).unwrap_or_default();
        let config = config.map(|data| (u16_at(&data, 2).unwrap_or(0), table.len()));
        Self { files, config, table, surfaces: HashMap::new() }
    }

    /// The colour of a body: its surface, else its colour number, else the category colour.
    fn body(&mut self, body: &Body) -> Paint {
        if let Some(name) = &body.surface {
            let files = &mut *self.files;
            let found = *self.surfaces.entry(name.to_lowercase()).or_insert_with(|| {
                let parts = surface_path(name)?;
                let path: Vec<&str> = std::iter::once("design").chain(parts).collect();
                surface_color(&files.read(&path)?)
            });
            if let Some((rgb, textured)) = found {
                return Paint::Surface { rgb, textured };
            }
        }
        self.table.get(&body.color_number).map_or(Paint::Fallback, |&rgb| Paint::Number(rgb))
    }

    fn paint(&mut self, drawing: &Drawing) -> Painted {
        let bodies: Vec<Paint> = drawing.bodies.iter().map(|b| self.body(b)).collect();
        let macros: Vec<Vec<Paint>> = drawing.macros.iter().map(|m| m.bodies.iter().map(|b| self.body(b)).collect()).collect();
        let active = bodies.iter().chain(macros.iter().flatten()).any(|p| *p != Paint::Fallback);
        let mut visible = Tally::default();
        drawing.bodies.iter().zip(&bodies).filter(|(b, _)| !b.auxiliary).for_each(|(_, p)| visible.add(*p));
        let mut macro_bodies = Tally::default();
        macros.iter().flatten().for_each(|p| macro_bodies.add(*p));
        let info = vec![
            ("Allplan colours (visible bodies)".into(), visible.text()),
            ("Allplan colours (macro bodies)".into(), macro_bodies.text()),
            ("Allplan project files".into(), self.files_line()),
        ];
        Painted { bodies, macros, active, visible, macro_bodies, info }
    }

    fn files_line(&self) -> String {
        let found = self.surfaces.values().filter(|s| s.is_some()).count();
        format!(
            "animation.cfg: {}; surfaces found: {found} of {}",
            match self.config {
                Some((version, colours)) => format!("version {version} ({colours} colours)"),
                None => "not found".into(),
            },
            self.surfaces.len()
        )
    }
}

/// Looks up the colour of every body of one drawing file (§10).
fn paint(drawing: &Drawing, files: &mut dyn ProjectFiles) -> Painted {
    Painter::new(files).paint(drawing)
}

/// Appends a colour for the last `count` vertices of a layer that has vertex colours.
fn paint_vertices(layer: &mut Layer, count: usize, paint: Paint, fallback: [f32; 3]) {
    if let Some(colors) = layer.colors.as_mut() {
        colors.extend(std::iter::repeat_n(paint.rgba(fallback), count));
    }
}

/// Rounds a quantity for display (§8.4).
fn rounded(value: f64) -> serde_json::Value {
    if value.is_finite() { serde_json::json!((value * 1000.0).round() / 1000.0) } else { serde_json::Value::Null }
}

/// Extra properties every element of a drawing file gets (in a project: file and storey).
type Extra = serde_json::Map<String, serde_json::Value>;

/// The properties of a body: the record (v0.1) and its element (§8.5).
fn body_props(drawing: &Drawing, body: &Body, extra: &Extra) -> String {
    let mut props = serde_json::json!({
        "Allplan drawing file": drawing.name,
        "Record offset": format!("0x{:X}", body.offset),
        "Record class": format!("0x{:02X}", body.record_type & 0xFF),
        "Record type": format!("0x{:04X}", body.record_type),
        "Form": format!("0x{:04X}", body.form),
        "Kind": match body.kind { 4 => "explicit topology (4)".to_string(), k => format!("prism ({k})") },
        "Auxiliary": body.auxiliary,
    });
    if !body.auxiliary {
        props["Category"] = category(drawing, body).name().into();
    }
    if body.open {
        props["Open surface"] = true.into();
    }
    if let Some(surface) = &body.surface {
        props["Surface"] = surface.clone().into();
    }
    if let (Some(element), Some(map)) = (body.element.map(|i| &drawing.elements[i]), props.as_object_mut()) {
        if !element.name.is_empty() {
            map.insert("Name".into(), element.name.clone().into());
        }
        if let Some((group, subtype)) = element.type_code {
            let label = element_type(group, subtype)
                .map(|(label, _)| label.to_string())
                .unwrap_or_else(|| format!("Element {group:02X} {subtype:02X}"));
            map.insert("Element type".into(), label.into());
            map.insert("Type code".into(), format!("{group:02X} {subtype:02X}").into());
        }
        let labels = ["Height (m)", "Thickness (m)", "Length (m)", "Area (m²)", "Volume (m³)"];
        for (label, &value) in labels.iter().zip(&element.quantities) {
            map.insert((*label).into(), rounded(value));
        }
        map.insert("Element record".into(), format!("0x{:02X} at 0x{:X}", element.class, element.offset).into());
    }
    if let Some(map) = props.as_object_mut() {
        map.extend(extra.clone());
    }
    props.to_string()
}

/// Appends triangles (millimetres) to a layer; returns the first index.
fn append(layer: &mut Layer, positions: &[[f64; 3]], normals: &[[f32; 3]], indices: &[u32]) -> Result<u32, String> {
    let base = u32::try_from(layer.vertex_count()).map_err(|_| "Allplan: too many vertices")?;
    let first = u32::try_from(layer.indices.len()).map_err(|_| "Allplan: too many triangles")?;
    layer.positions.extend(positions.iter().flat_map(|p| p.map(|c| (c * MM) as f32)));
    layer.normals.extend(normals.iter().flatten());
    for &i in indices {
        layer.indices.push(base.checked_add(i).ok_or("Allplan: too many vertices")?);
    }
    Ok(first)
}

/// The properties of a macro placement (§9.4).
fn placement_props(drawing: &Drawing, placement: &Placement, definition: &Macro, extra: &Extra) -> String {
    let mut props = serde_json::json!({
        "Allplan drawing file": drawing.name,
        "Record offset": format!("0x{:X}", placement.offset),
        "Record class": "0xEB",
        "Macro": definition.name,
        "Macro number": definition.number,
        "Bodies": definition.bodies.len(),
    });
    let name = if placement.name.is_empty() { &definition.name } else { &placement.name };
    if let Some(map) = props.as_object_mut() {
        if !name.is_empty() {
            map.insert("Name".into(), name.clone().into());
        }
        map.extend(extra.clone());
    }
    props.to_string()
}

/// How many placements there are and what became of them.
fn placement_summary(drawing: &Drawing) -> String {
    let count = |want: fn(&Placed) -> bool| drawing.placements.iter().filter(|p| want(&p.placed)).count();
    format!(
        "{} (shown {}, without 3D {}, unknown macro {}, unreadable {}, over the triangle limit {})",
        drawing.placements.len(),
        count(|p| matches!(p, Placed::Resolved(_))),
        count(|p| *p == Placed::Without3d),
        count(|p| *p == Placed::UnknownMacro),
        count(|p| *p == Placed::Unreadable),
        count(|p| *p == Placed::OverLimit),
    )
}

/// The layer of a visible body: the category of its element type, `Other` without one (§8.5).
fn category(drawing: &Drawing, body: &Body) -> Category {
    body.element
        .and_then(|i| drawing.elements[i].type_code)
        .and_then(|(group, subtype)| element_type(group, subtype))
        .map_or(Category::Other, |(_, category)| category)
}

/// What a body of a drawing file is, for choosing its layer and fallback colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    /// A visible body of the drawing file.
    Body(Category),
    /// A visible body of a macro placement.
    Macro,
    Auxiliary,
}

impl Slot {
    fn color(self) -> [f32; 3] {
        match self {
            Slot::Body(category) => category.color(),
            Slot::Macro => MACRO_COLOR,
            Slot::Auxiliary => AUXILIARY_COLOR,
        }
    }
}

/// Where the bodies of one drawing file go: the layers, which layer per slot, the element id per
/// record offset, and extra properties.
struct Target<'a> {
    layers: &'a mut [Layer],
    layer_of: &'a dyn Fn(Slot) -> usize,
    id_of: &'a dyn Fn(usize) -> u64,
    extra: &'a Extra,
}

/// Appends the bodies and macro placements of a drawing file to layers. A placement is one
/// element: its visible bodies and its auxiliary ones, under the id of the placement record (§9.4).
fn emit(drawing: &Drawing, painted: Option<&Painted>, target: Target) -> Result<(), String> {
    let paint_of = |paints: Option<&Vec<Paint>>, i: usize| paints.and_then(|p| p.get(i)).copied().unwrap_or(Paint::Fallback);
    for (i, body) in drawing.bodies.iter().enumerate() {
        let slot = if body.auxiliary { Slot::Auxiliary } else { Slot::Body(category(drawing, body)) };
        let Some(layer) = target.layers.get_mut((target.layer_of)(slot)) else { continue };
        let id = (target.id_of)(body.offset);
        let first = append(layer, &body.positions, &body.normals, &body.indices)?;
        paint_vertices(layer, body.positions.len(), paint_of(painted.map(|p| &p.bodies), i), slot.color());
        let count = u32::try_from(body.indices.len()).map_err(|_| "Allplan: too many triangles")?;
        layer.elements.push((id, first, count));
        layer.props.push((id, body_props(drawing, body, target.extra)));
    }
    for placement in &drawing.placements {
        let Placed::Resolved(index) = placement.placed else { continue };
        let definition = &drawing.macros[index];
        let id = (target.id_of)(placement.offset);
        let transform = Transform::new(&placement.matrix);
        let mut props = Some(placement_props(drawing, placement, definition, target.extra));
        for (auxiliary, slot) in [(false, Slot::Macro), (true, Slot::Auxiliary)] {
            let Some(layer) = target.layers.get_mut((target.layer_of)(slot)) else { continue };
            let first = u32::try_from(layer.indices.len()).map_err(|_| "Allplan: too many triangles")?;
            for (i, body) in definition.bodies.iter().enumerate().filter(|(_, b)| b.auxiliary == auxiliary) {
                transform.append(layer, body)?;
                paint_vertices(layer, body.positions.len(), paint_of(painted.and_then(|p| p.macros.get(index)), i), slot.color());
            }
            let end = u32::try_from(layer.indices.len()).map_err(|_| "Allplan: too many triangles")?;
            if end > first {
                layer.elements.push((id, first, end - first));
                layer.props.extend(props.take().map(|p| (id, p)));
            }
        }
    }
    Ok(())
}

/// [`build_at`] at height 0 (tests).
#[cfg(test)]
fn build(drawing: &Drawing, painted: Option<&Painted>) -> Result<Loaded, String> {
    build_at(drawing, painted, 0.0)
}

/// Colour of the 2D lines: dark, like ink (§12.3).
const LINE_COLOR: [f32; 3] = [0.20, 0.20, 0.22];
/// How far above their storey the 2D lines are drawn (mm, §12.2).
const LINE_LIFT: f64 = 10.0;

/// Half the width of the 2D line ribbons: a share of the extent of the lines (between the 2nd
/// and 98th percentile, so a few far-off lines do not widen them all), at least 5 mm.
fn line_half_width<'a>(lines: impl Iterator<Item = &'a Line2d>) -> f64 {
    let points: Vec<[f64; 2]> = lines.flat_map(|l| l.points.iter().copied()).collect();
    if points.is_empty() {
        return 5.0;
    }
    let step = (points.len() / 100_000).max(1);
    let mut extent = [0.0f64; 2];
    for (axis, e) in extent.iter_mut().enumerate() {
        let mut values: Vec<f64> = points.iter().step_by(step).map(|p| p[axis]).collect();
        values.sort_by(f64::total_cmp);
        let at = |f: f64| values[((values.len() - 1) as f64 * f).round() as usize];
        *e = at(0.98) - at(0.02);
    }
    (extent[0].hypot(extent[1]) / 3000.0).max(5.0)
}

/// Appends 2D lines to a layer as flat ribbons facing up at height `z` (mm), with square ends;
/// a polyline as its segments, not closed (§12.1). Returns the number of segments drawn.
fn emit_lines(layer: &mut Layer, lines: &[Line2d], z: f64, half: f64) -> Result<usize, String> {
    let mut segments = 0;
    for line in lines {
        for pair in line.points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let length = dx.hypot(dy);
            if !(length > 1e-9) {
                continue;
            }
            let (ux, uy) = (dx / length * half, dy / length * half);
            let (start, end) = ([a[0] - ux, a[1] - uy], [b[0] + ux, b[1] + uy]);
            // Counter-clockwise seen from above: start right, end right, end left, start left.
            let quad = [
                [start[0] + uy, start[1] - ux, z],
                [end[0] + uy, end[1] - ux, z],
                [end[0] - uy, end[1] + ux, z],
                [start[0] - uy, start[1] + ux, z],
            ];
            append(layer, &quad, &[[0.0, 0.0, 1.0]; 4], &[0, 1, 2, 0, 2, 3])?;
            paint_vertices(layer, 4, Paint::Fallback, LINE_COLOR);
            segments += 1;
        }
    }
    Ok(segments)
}

/// What became of the 2D records of drawing files.
fn lines_summary<'a>(drawings: impl Iterator<Item = &'a Drawing>, segments: usize) -> String {
    let (mut lines, mut polylines, mut with_3d, mut unreadable) = (0, 0, 0, 0);
    for d in drawings {
        lines += d.lines.iter().filter(|l| l.class == 0x82).count();
        polylines += d.lines.iter().filter(|l| l.class == 0x84).count();
        with_3d += d.lines_with_3d;
        unreadable += d.lines_unreadable;
    }
    format!(
        "{lines} lines and {polylines} polylines ({segments} segments drawn); {with_3d} part of a 3D element and \
         {unreadable} unreadable, not drawn"
    )
}

/// The layers of one drawing file: visible bodies per category, macros, 2D lines at height `z`
/// (mm), and the auxiliary bodies in one hidden layer.
fn build_at(drawing: &Drawing, painted: Option<&Painted>, z: f64) -> Result<Loaded, String> {
    let resolved = drawing.placements.iter().filter(|p| matches!(p.placed, Placed::Resolved(_))).count();
    if drawing.bodies.is_empty() && resolved == 0 && drawing.lines.is_empty() {
        let name = if drawing.name.is_empty() { String::new() } else { format!(" \"{}\"", drawing.name) };
        return Err(format!(
            "Allplan: the drawing file{name} has no 3D bodies and no 2D lines. This reader shows 3D bodies, macros \
             and 2D lines; texts, hatchings, arcs and layouts are not read yet."
        ));
    }
    let mut layers: Vec<Layer> =
        Category::ALL.iter().map(|&c| Layer { name: c.name().into(), color: c.color(), ..Layer::default() }).collect();
    layers.push(Layer { name: "2D lines".into(), color: LINE_COLOR, ..Layer::default() });
    layers.push(Layer { name: "Auxiliary bodies (openings, rooms)".into(), color: AUXILIARY_COLOR, hidden: true, ..Layer::default() });
    // Colours from the project folder become vertex colours; the category colour fills in (§10).
    let painted = painted.filter(|p| p.active);
    if painted.is_some() {
        layers.iter_mut().for_each(|layer| layer.colors = Some(Vec::new()));
    }
    let index_of = |c: Category| Category::ALL.iter().position(|&x| x == c).unwrap_or(0);
    let layer_of = |slot: Slot| match slot {
        Slot::Body(c) => index_of(c),
        Slot::Macro => index_of(Category::Macros),
        Slot::Auxiliary => Category::ALL.len() + 1,
    };
    let target = Target { layers: &mut layers, layer_of: &layer_of, id_of: &|offset| offset as u64, extra: &Extra::new() };
    emit(drawing, painted, target)?;
    let half = line_half_width(drawing.lines.iter());
    let segments = emit_lines(&mut layers[Category::ALL.len()], &drawing.lines, z + LINE_LIFT, half)?;
    let visible = drawing.bodies.iter().filter(|b| !b.auxiliary).count();
    let hidden = drawing.bodies.len() - visible;
    let linked = drawing.bodies.iter().filter(|b| b.element.is_some()).count();
    let mut project = ProjectData::default();
    project.info = vec![
        ("Allplan drawing file".into(), drawing.name.clone()),
        ("Allplan pages".into(), drawing.pages.to_string()),
        ("Allplan 3D bodies".into(), visible.to_string()),
        ("Allplan auxiliary bodies (hidden)".into(), hidden.to_string()),
        ("Allplan bodies linked to an element".into(), linked.to_string()),
        ("Allplan macro placements".into(), placement_summary(drawing)),
        ("Allplan 2D lines".into(), format!("{} at z {z} mm", lines_summary(std::iter::once(drawing), segments))),
        (
            "Allplan colours".into(),
            match painted {
                Some(_) => "from the project folder (surfaces and animation.cfg)".into(),
                None => "category colours (no project files found next to the drawing file)".into(),
            },
        ),
        (
            "Allplan skipped bodies".into(),
            format!(
                "clip volumes: {}, without faces: {}, unreadable: {}",
                drawing.skipped_clip, drawing.skipped_no_faces, drawing.skipped_unreadable
            ),
        ),
        ("Allplan import scope".into(), IMPORT_SCOPE.into()),
    ];
    project.info.extend(painted.iter().flat_map(|p| p.info.iter().cloned()));
    // Empty layers are left out.
    let layers = layers.into_iter().filter(|layer| !layer.indices.is_empty()).collect();
    Ok(Loaded { layers, views: Vec::new(), project })
}

/// What the reader shows, for the project info.
const IMPORT_SCOPE: &str = "3D bodies with element name, type and quantities, 3D macros, colours from the project folder, \
     whole project folders, 2D lines and polylines (spec v0.6); no textures, texts, hatchings or arcs";

/// A node of the building structure (§11.2).
#[derive(Debug, Clone, PartialEq)]
struct ProjectNode {
    name: String,
    /// Local name of the node element: `Storey`, `Site`, `Construction`, `UserDefinedStructureChildNode` …
    kind: String,
    files: Vec<u32>,
    children: Vec<ProjectNode>,
    /// Of a storey: the level model (ObjectGuid) and the number of its bottom plane (§12.2).
    bottom: Option<(String, i64)>,
}

/// Attribute keys of a storey (§12.2): the level model, and the number of its bottom plane.
const LEVEL_MODEL_KEY: &str = "325ADB67-B049-4781-A41A-877EADA7D218";
const BOTTOM_PLANE_KEY: &str = "39C9F321-882F-11D3-BD97-00E0294907AD";

/// The level model and bottom plane number in the `Attributes` of a node (§12.2): an
/// `InternalAttribute` holds its key (in an attribute or a text) and a `String` or `Integer` with
/// a `Value`.
fn storey_plane(node: roxmltree::Node) -> Option<(String, i64)> {
    let named = |n: &roxmltree::Node, name: &str| n.is_element() && n.tag_name().name() == name;
    let (mut model, mut bottom) = (None, None);
    for attribute in node.children().filter(|n| named(n, "Attributes")).flat_map(|a| a.children().filter(|n| named(n, "InternalAttribute"))) {
        let has = |key: &str| {
            attribute.descendants().any(|n| n.attributes().any(|a| a.value().trim().eq_ignore_ascii_case(key)) || n.text().is_some_and(|t| t.trim().eq_ignore_ascii_case(key)))
        };
        let value = |kind: &str| {
            let n = attribute.descendants().find(|n| named(n, kind))?;
            n.attribute("Value").or(n.text()).map(|v| v.trim().to_string())
        };
        if has(LEVEL_MODEL_KEY) {
            model = value("String").or(model);
        }
        if has(BOTTOM_PLANE_KEY) {
            bottom = value("Integer").and_then(|v| v.parse().ok()).or(bottom);
        }
    }
    Some((model?, bottom?))
}

/// The plane heights of the level models of a project (§12.2): model ObjectGuid (lower case) →
/// plane number → height (mm).
type Levels = HashMap<String, HashMap<i64, f64>>;

/// Adds the planes of one `Allplan_BIM_LevMo_*.xml`: `BottomPlane` and `TopPlane` elements with an
/// `ObjectGuid` (its first part, hexadecimal, is the plane number) and a `HeightLevel`.
fn read_levels(text: &str, levels: &mut Levels) {
    let Ok(document) = roxmltree::Document::parse(text.trim_start_matches('\u{feff}')) else { return };
    let root = document.root_element();
    let Some(model) = root.attribute("ObjectGuid").filter(|_| root.tag_name().name() == "Levelmodel") else { return };
    let planes = levels.entry(model.trim().to_lowercase()).or_default();
    for plane in root.descendants().filter(|n| n.is_element() && matches!(n.tag_name().name(), "BottomPlane" | "TopPlane")) {
        let number = plane.attribute("ObjectGuid").and_then(|id| i64::from_str_radix(id.trim().split('-').next()?, 16).ok());
        let height = plane
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "HeightLevel")
            .and_then(|n| n.text()?.trim().parse::<f64>().ok())
            .filter(|h| h.is_finite());
        if let (Some(number), Some(height)) = (number, height) {
            planes.entry(number).or_insert(height);
        }
    }
}

/// Deepest node nesting read from a building structure.
const MAX_NODE_DEPTH: usize = 64;

/// The top-level nodes of `BIM/Allplan_BIM_BuildingStructure.xml` (§11.2), read by local name.
fn read_structure(text: &str) -> Option<Vec<ProjectNode>> {
    let document = roxmltree::Document::parse(text.trim_start_matches('\u{feff}')).ok()?;
    Some(child_nodes(document.root_element(), 0))
}

/// The nodes under `parent`: `BuildingStructureNodes` → `BuildingStructureChildNode` or
/// `ConstructionNodes` → `ConstructionChildNode`, each holding one node element.
fn child_nodes(parent: roxmltree::Node, depth: usize) -> Vec<ProjectNode> {
    const NOT_NODES: [&str; 7] =
        ["Attributes", "InternalAttribute", "Integer", "String", "Files", "BuildingStructureNodes", "ConstructionNodes"];
    if depth > MAX_NODE_DEPTH {
        return Vec::new();
    }
    let named = |node: &roxmltree::Node, names: &[&str]| node.is_element() && names.contains(&node.tag_name().name());
    let mut nodes = Vec::new();
    for list in parent.children().filter(|n| named(n, &["BuildingStructureNodes", "ConstructionNodes"])) {
        for item in list.children().filter(|n| named(n, &["BuildingStructureChildNode", "ConstructionChildNode"])) {
            if let Some(node) = item.children().find(|n| n.is_element() && !NOT_NODES.contains(&n.tag_name().name())) {
                let files = node
                    .children()
                    .filter(|n| named(n, &["Files"]))
                    .flat_map(|list| list.children().filter(|n| named(n, &["File"])))
                    .filter_map(|file| file.attribute("ID")?.trim().parse().ok())
                    .collect();
                nodes.push(ProjectNode {
                    name: node.attribute("Name").unwrap_or("").trim().to_string(),
                    kind: node.tag_name().name().to_string(),
                    files,
                    children: child_nodes(node, depth + 1),
                    bottom: storey_plane(node),
                });
            }
        }
    }
    nodes
}

/// A group of drawing files, one layer (§11.2): a storey, site or construction, or the files
/// right under a top-level node.
#[derive(Debug, Clone, PartialEq)]
struct Group {
    /// Index of its top-level node.
    top: usize,
    /// `<top> / <group>`, or `<top>` for files right under the top-level node.
    label: String,
    name: String,
    /// Height of the 2D lines: the bottom plane of a storey, else 0 (mm, §12.2).
    z: f64,
}

/// The drawing files of a project folder in their groups (§11.2).
#[derive(Debug, Default)]
struct ProjectPlan {
    tops: Vec<String>,
    groups: Vec<Group>,
    /// Drawing file number, group, and file name in the project folder.
    files: Vec<(u32, usize, String)>,
    /// `File` references in the structure (duplicates included).
    references: usize,
    /// Read from a building structure (else: all `tb*.ndw` of the folder).
    structure: bool,
}

/// The groups and files of a building structure: each file in the group of its nearest storey,
/// site or construction node; a file referenced twice is loaded once (§11.2).
fn plan_structure(tops: &[ProjectNode], levels: &Levels) -> ProjectPlan {
    struct Walk<'a> {
        plan: ProjectPlan,
        seen: std::collections::HashSet<u32>,
        levels: &'a Levels,
    }
    fn walk(node: &ProjectNode, top: usize, group: (String, String, f64), depth: usize, w: &mut Walk) {
        // A storey's lines lie on its bottom plane; other groups at 0 (§12.2).
        let z = || {
            let (model, plane) = node.bottom.as_ref()?;
            w.levels.get(&model.to_lowercase())?.get(plane).copied()
        };
        let group = if depth > 0 && matches!(node.kind.as_str(), "Storey" | "Site" | "Construction") {
            let z = if node.kind == "Storey" { z().unwrap_or(0.0) } else { 0.0 };
            (format!("{} / {}", w.plan.tops[top], node.name), node.name.clone(), z)
        } else if depth == 0 && node.kind == "Storey" {
            (group.0, group.1, z().unwrap_or(0.0))
        } else {
            group
        };
        let plan = &mut w.plan;
        if !node.files.is_empty() {
            let index = match plan.groups.iter().position(|g| g.top == top && g.label == group.0) {
                Some(index) => index,
                None => {
                    plan.groups.push(Group { top, label: group.0.clone(), name: group.1.clone(), z: group.2 });
                    plan.groups.len() - 1
                }
            };
            for &number in &node.files {
                plan.references += 1;
                if w.seen.insert(number) {
                    plan.files.push((number, index, format!("tb{number:06}.ndw")));
                }
            }
        }
        for child in &node.children {
            walk(child, top, group.clone(), depth + 1, w);
        }
    }
    let plan = ProjectPlan { tops: tops.iter().map(|t| t.name.clone()).collect(), structure: true, ..ProjectPlan::default() };
    let mut w = Walk { plan, seen: std::collections::HashSet::new(), levels };
    for (index, top) in tops.iter().enumerate() {
        walk(top, index, (top.name.clone(), top.name.clone(), 0.0), 0, &mut w);
    }
    w.plan
}

/// Without a building structure: every `tb*.ndw` of the folder, by number, in one group named
/// after the folder (§11.2).
fn plan_folder(name: &str, entries: &[String]) -> ProjectPlan {
    let mut files: Vec<(u32, usize, String)> = entries
        .iter()
        .filter_map(|entry| {
            let digits = entry.strip_prefix("tb")?.strip_suffix(".ndw")?;
            let number = (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then(|| digits.parse().ok())??;
            Some((number, 0, entry.clone()))
        })
        .collect();
    files.sort();
    files.dedup_by_key(|f| f.0);
    let groups = if files.is_empty() { Vec::new() } else { vec![Group { top: 0, label: name.into(), name: name.into(), z: 0.0 }] };
    ProjectPlan { tops: vec![name.into()], groups, files, references: 0, structure: false }
}

/// A drawing file of a project.
struct ProjectDrawing {
    number: u32,
    group: usize,
    drawing: Drawing,
    painted: Painted,
}

/// A project folder read (§11).
struct Project {
    name: String,
    plan: ProjectPlan,
    /// Referenced drawing files that are not in the folder.
    missing: Vec<u32>,
    /// Drawing files that could not be read, with the reason.
    unreadable: Vec<(u32, String)>,
    drawings: Vec<ProjectDrawing>,
    files_line: String,
}

impl Folder {
    /// The path of `path` below the folder, found case-insensitively per component.
    fn find(&mut self, path: &[&str]) -> Option<std::path::PathBuf> {
        let mut at = self.root.clone();
        for part in path {
            at = self.listing(&at).get(&part.to_lowercase())?.clone();
        }
        Some(at)
    }

    /// The entries of a directory by lower-case name, listed once.
    fn listing(&mut self, dir: &std::path::Path) -> &HashMap<String, std::path::PathBuf> {
        self.listings.entry(dir.to_path_buf()).or_insert_with(|| {
            std::fs::read_dir(dir)
                .map(|entries| {
                    entries.flatten().map(|entry| (entry.file_name().to_string_lossy().to_lowercase(), entry.path())).collect()
                })
                .unwrap_or_default()
        })
    }
}

/// Reads drawing files, in parallel where threads are available.
fn read_drawings(paths: &[std::path::PathBuf]) -> Vec<Result<Drawing, String>> {
    let one = |path: &std::path::PathBuf| std::fs::read(path).map_err(|e| e.to_string()).and_then(|data| parse(&data));
    #[cfg(not(target_arch = "wasm32"))]
    {
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(8).min(paths.len());
        if threads > 1 {
            let next = std::sync::atomic::AtomicUsize::new(0);
            let results: Vec<std::sync::Mutex<Option<Result<Drawing, String>>>> = paths.iter().map(|_| std::sync::Mutex::new(None)).collect();
            std::thread::scope(|scope| {
                for _ in 0..threads {
                    scope.spawn(|| loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(path) = paths.get(i) else { break };
                        let result = one(path);
                        if let Ok(mut slot) = results[i].lock() {
                            *slot = Some(result);
                        }
                    });
                }
            });
            return results
                .into_iter()
                .map(|slot| slot.into_inner().ok().flatten().unwrap_or_else(|| Err("not read".into())))
                .collect();
        }
    }
    paths.iter().map(one).collect()
}

/// The plan of the building structure of a project folder, with the storey heights from its
/// level models (§11.2, §12.2); `None` without a (readable) structure.
fn structure_plan(folder: &mut Folder) -> Option<ProjectPlan> {
    let text = String::from_utf8(folder.read(&["BIM", "Allplan_BIM_BuildingStructure.xml"])?).ok()?;
    let tops = read_structure(&text)?;
    let mut levels = Levels::new();
    if let Some(bim) = folder.find(&["BIM"]) {
        let mut models: Vec<String> =
            folder.listing(&bim).keys().filter(|n| n.starts_with("allplan_bim_levmo_") && n.ends_with(".xml")).cloned().collect();
        models.sort();
        for model in models {
            if let Some(text) = folder.read(&["BIM", &model]).and_then(|b| String::from_utf8(b).ok()) {
                read_levels(&text, &mut levels);
            }
        }
    }
    Some(plan_structure(&tops, &levels))
}

/// The storey height of a drawing file `tbNNNNNN.ndw` opened on its own from its project folder
/// (§12.2): the height its group has in the building structure.
fn file_height(path: &std::path::Path) -> Option<f64> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    let digits = name.strip_prefix("tb")?.strip_suffix(".ndw")?;
    let number: u32 = digits.bytes().all(|b| b.is_ascii_digit()).then(|| digits.parse().ok())??;
    let plan = structure_plan(&mut Folder::new(path.parent()?))?;
    let &(_, group, _) = plan.files.iter().find(|f| f.0 == number)?;
    Some(plan.groups[group].z)
}

/// Reads a project folder (§11): its building structure (or all `tb*.ndw`), every drawing file
/// once, and the colours from the same folder, read once.
fn read_project(dir: &std::path::Path) -> Result<Project, String> {
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| dir.display().to_string());
    let mut folder = Folder::new(dir);
    let plan = match structure_plan(&mut folder) {
        Some(plan) => plan,
        None => {
            let root = folder.root.clone();
            let entries: Vec<String> = folder.listing(&root).keys().cloned().collect();
            plan_folder(&name, &entries)
        }
    };
    let mut found = Vec::new();
    let mut missing = Vec::new();
    for (number, group, file) in &plan.files {
        match folder.find(&[file]) {
            Some(path) => found.push((*number, *group, path)),
            None => missing.push(*number),
        }
    }
    if found.is_empty() {
        return Err(format!("Allplan: the project folder \"{name}\" has no drawing files (tb*.ndw)"));
    }
    let paths: Vec<std::path::PathBuf> = found.iter().map(|(_, _, path)| path.clone()).collect();
    let read = read_drawings(&paths);
    let mut painter = Painter::new(&mut folder);
    let mut drawings = Vec::new();
    let mut unreadable = Vec::new();
    for ((number, group, _), result) in found.into_iter().zip(read) {
        match result {
            Ok(drawing) => {
                let painted = painter.paint(&drawing);
                drawings.push(ProjectDrawing { number, group, drawing, painted });
            }
            Err(error) => unreadable.push((number, error)),
        }
    }
    let files_line = painter.files_line();
    Ok(Project { name, plan, missing, unreadable, drawings, files_line })
}

/// What a drawing file holds, counted for a project.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Counts {
    files: usize,
    visible: usize,
    auxiliary: usize,
    placements: usize,
    /// 2D lines and polylines drawn.
    lines: usize,
}

impl Counts {
    fn of(drawing: &Drawing) -> Self {
        let visible = drawing.bodies.iter().filter(|b| !b.auxiliary).count();
        let placements = drawing.placements.iter().filter(|p| matches!(p.placed, Placed::Resolved(_))).count();
        Self { files: 1, visible, auxiliary: drawing.bodies.len() - visible, placements, lines: drawing.lines.len() }
    }

    fn add(&mut self, other: Counts) {
        self.files += other.files;
        self.visible += other.visible;
        self.auxiliary += other.auxiliary;
        self.placements += other.placements;
        self.lines += other.lines;
    }
}

impl Project {
    /// Counts per group.
    fn group_counts(&self) -> Vec<Counts> {
        let mut counts = vec![Counts::default(); self.plan.groups.len()];
        for d in &self.drawings {
            counts[d.group].add(Counts::of(&d.drawing));
        }
        counts
    }

    /// Counts per top-level node.
    fn top_counts(&self) -> Vec<Counts> {
        let mut counts = vec![Counts::default(); self.plan.tops.len()];
        for d in &self.drawings {
            counts[self.plan.groups[d.group].top].add(Counts::of(&d.drawing));
        }
        counts
    }

    /// The first top-level node (in document order) with visible bodies or placements: the only
    /// one that starts visible (§11.3).
    fn shown_top(&self) -> usize {
        self.top_counts().iter().position(|c| c.visible + c.placements > 0).unwrap_or(0)
    }
}

/// The layers of a project (§11.3): one per group with the visible bodies and macros of its
/// drawing files, one hidden auxiliary layer per top-level node; only the first top-level node
/// with visible bodies starts visible.
fn build_project(project: &Project) -> Result<Loaded, String> {
    let groups = &project.plan.groups;
    let tops = &project.plan.tops;
    let shown = project.shown_top();
    // Layers: the groups, then their 2D lines, then the auxiliary bodies per top-level node.
    let mut layers: Vec<Layer> = groups
        .iter()
        .map(|g| Layer { name: g.label.clone(), color: DEFAULT_COLOR, colors: Some(Vec::new()), hidden: g.top != shown, ..Layer::default() })
        .chain(groups.iter().map(|g| Layer {
            name: format!("{} / 2D lines", g.label),
            color: LINE_COLOR,
            colors: Some(Vec::new()),
            hidden: g.top != shown,
            ..Layer::default()
        }))
        .chain(tops.iter().map(|t| Layer {
            name: format!("{t} / Auxiliary bodies"),
            color: AUXILIARY_COLOR,
            colors: Some(Vec::new()),
            hidden: true,
            ..Layer::default()
        }))
        .collect();
    let half = line_half_width(project.drawings.iter().flat_map(|d| &d.drawing.lines));
    let mut segments = 0;
    for d in &project.drawings {
        let group = &groups[d.group];
        let (body_layer, auxiliary_layer) = (d.group, 2 * groups.len() + group.top);
        let layer_of = |slot: Slot| if slot == Slot::Auxiliary { auxiliary_layer } else { body_layer };
        // Ids unique over the project: file number and record offset (§11.3).
        let number = u64::from(d.number);
        let id_of = |offset: usize| number << 32 | (offset as u64 & 0xFFFF_FFFF);
        let mut extra = Extra::new();
        let file = if d.drawing.name.is_empty() { format!("{:06}", d.number) } else { format!("{:06} {}", d.number, d.drawing.name) };
        extra.insert("Drawing file".into(), file.into());
        extra.insert("Storey".into(), group.name.clone().into());
        let target = Target { layers: &mut layers, layer_of: &layer_of, id_of: &id_of, extra: &extra };
        emit(&d.drawing, Some(&d.painted), target)?;
        segments += emit_lines(&mut layers[groups.len() + d.group], &d.drawing.lines, group.z + LINE_LIFT, half)?;
    }
    if layers.iter().all(|l| l.indices.is_empty()) {
        return Err(format!("Allplan: the project \"{}\" has no 3D bodies or 2D lines in its drawing files", project.name));
    }
    let total = project.top_counts().into_iter().fold(Counts::default(), |mut a, c| {
        a.add(c);
        a
    });
    let (mut visible, mut macro_bodies) = (Tally::default(), Tally::default());
    for d in &project.drawings {
        visible.merge(d.painted.visible);
        macro_bodies.merge(d.painted.macro_bodies);
    }
    let structure = if project.plan.structure {
        format!(
            "{} top-level nodes, {} references to drawing files, {} missing, {} unreadable",
            tops.len(),
            project.plan.references,
            project.missing.len(),
            project.unreadable.len()
        )
    } else {
        format!("no building structure: all {} tb*.ndw files of the folder", project.plan.files.len())
    };
    let mut info = vec![
        ("Allplan project".into(), project.name.clone()),
        ("Allplan building structure".into(), structure),
        ("Allplan shown node".into(), format!("{} (the other top-level nodes start hidden)", tops.get(shown).cloned().unwrap_or_default())),
        ("Allplan drawing files".into(), total.files.to_string()),
        ("Allplan 3D bodies".into(), total.visible.to_string()),
        ("Allplan auxiliary bodies (hidden)".into(), total.auxiliary.to_string()),
        ("Allplan macro placements shown".into(), total.placements.to_string()),
        ("Allplan 2D lines".into(), lines_summary(project.drawings.iter().map(|d| &d.drawing), segments)),
        ("Allplan colours (visible bodies)".into(), visible.text()),
        ("Allplan colours (macro bodies)".into(), macro_bodies.text()),
        ("Allplan project files".into(), project.files_line.clone()),
        ("Allplan import scope".into(), IMPORT_SCOPE.into()),
    ];
    if !project.missing.is_empty() {
        let list: Vec<String> = project.missing.iter().take(20).map(|n| format!("tb{n:06}.ndw")).collect();
        info.push(("Allplan missing drawing files".into(), list.join(", ")));
    }
    if !project.unreadable.is_empty() {
        let list: Vec<String> = project.unreadable.iter().take(20).map(|(n, e)| format!("tb{n:06}.ndw: {e}")).collect();
        info.push(("Allplan unreadable drawing files".into(), list.join("; ")));
    }
    for (group, counts) in groups.iter().zip(project.group_counts()) {
        info.push((
            format!("Allplan group {}", group.label),
            format!(
                "{} drawing files, {} bodies, {} auxiliary, {} macro placements, {} 2D lines at z {} mm",
                counts.files, counts.visible, counts.auxiliary, counts.placements, counts.lines, group.z
            ),
        ));
    }
    let mut data = ProjectData::default();
    data.info = info;
    let layers = layers.into_iter().filter(|layer| !layer.indices.is_empty()).collect();
    Ok(Loaded { layers, views: Vec::new(), project: data })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A drawing file: header, `pages` empty pages, the name, and `stream` from 0xFFFC; then a
    /// page table of arbitrary bytes.
    fn file(pages: u16, name: &str, stream: &[u8]) -> Vec<u8> {
        let mut d = vec![0x52, 0x02, 0x02, 0x00, 0xF8, 0x7F, 0x04, 0x00];
        d.extend_from_slice(&(2 * pages).to_le_bytes());
        d.extend_from_slice(&[0x08, 0x08]);
        d.resize(12 + usize::from(pages) * PAGE, 0);
        let mut units: Vec<u16> = name.encode_utf16().collect();
        units.resize(NAME_UNITS, 0x20);
        for (i, u) in units.iter().enumerate() {
            d[NAME_OFFSET + 2 * i..NAME_OFFSET + 2 * i + 2].copy_from_slice(&u.to_le_bytes());
        }
        let start = 12 + 2 * PAGE;
        d[start..start + stream.len()].copy_from_slice(stream);
        d.extend((0..64u32).map(|i| (i * 37 + 11) as u8));
        d
    }

    /// An element record header of `class` with length `40 + 16·n16 + 8·n8`.
    fn header(class: u8, flags1: u8, n16: u16, n8: u32) -> Vec<u8> {
        let mut h = vec![0u8; HEADER];
        h[0] = 0x78;
        h[1] = flags1;
        h[4] = class;
        h[5] = 0x01;
        h[7] = 0x5C;
        h[14..16].copy_from_slice(&n16.to_le_bytes());
        h[17] = (n8 >> 16) as u8;
        h[18..20].copy_from_slice(&(n8 as u16).to_le_bytes());
        h
    }

    struct Spec<'a> {
        flags1: u8,
        kind: u8,
        form: u16,
        v: i32,
        e: u32,
        f: u32,
        translate: [f64; 3],
        vertices: &'a [[f64; 3]],
        topology: &'a [u8],
    }

    /// A class 0xC6 body record with a 12-value matrix (identity plus `translate`).
    fn body(s: &Spec) -> Vec<u8> {
        let mut b = vec![0x1E, 12, 0x03, s.kind];
        b.extend([0u8; 20]);
        let mut m = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];
        m[9..12].copy_from_slice(&s.translate);
        m.iter().for_each(|v| b.extend(v.to_le_bytes()));
        b.extend(s.form.to_le_bytes());
        b.extend(0u16.to_le_bytes());
        b.extend(s.v.to_le_bytes());
        b.extend(s.e.to_le_bytes());
        b.extend(s.f.to_le_bytes());
        let l = if !s.topology.is_empty() {
            (s.topology.len() as u32 - 8 * s.e - 8 * s.f) / 8
        } else {
            4 * s.f.saturating_sub(2) + 2 * s.vertices.len() as u32 / 2
        };
        b.extend(l.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        if s.kind != 0 {
            b.extend([0x00, 0x00, 0x00, 0x20, 0x00, 0x20, 0x20, 0x38]);
        }
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for (i, p) in s.vertices.iter().enumerate() {
            p.iter().for_each(|c| b.extend(c.to_le_bytes()));
            if s.kind != 4 || i > 0 {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k] + s.translate[k]);
                    hi[k] = hi[k].max(p[k] + s.translate[k]);
                }
            }
        }
        lo.iter().chain(&hi).for_each(|c| b.extend(c.to_le_bytes()));
        b.extend_from_slice(s.topology);
        assert_eq!(b.len() % 8, 0);
        let mut r = header(0xC6, s.flags1, 0, (b.len() / 8) as u32);
        r.extend(b);
        r
    }

    /// A box prism (V = −8) from `lo` to `hi`.
    fn prism(flags1: u8, form: u16, lo: [f64; 3], hi: [f64; 3]) -> Vec<u8> {
        let ring = [[lo[0], lo[1]], [hi[0], lo[1]], [hi[0], hi[1]], [lo[0], hi[1]]];
        let vertices: Vec<[f64; 3]> =
            ring.iter().map(|p| [p[0], p[1], lo[2]]).chain(ring.iter().map(|p| [p[0], p[1], hi[2]])).collect();
        body(&Spec { flags1, kind: 1, form, v: -8, e: 12, f: 6, translate: [0.0; 3], vertices: &vertices, topology: &[] })
    }

    /// A kind 4 body: one horizontal face 1000 × 1000 with a 500 × 500 hole, its outer loop
    /// clockwise and its hole walked through reversed edges.
    fn plate_with_hole(translate: [f64; 3]) -> Vec<u8> {
        let z = 5000.0;
        let vertices = [
            [1e9, 1e9, 1e9], // slot 0, unused
            [0.0, 0.0, z],
            [0.0, 1000.0, z],
            [1000.0, 1000.0, z],
            [1000.0, 0.0, z],
            [250.0, 250.0, z],
            [750.0, 250.0, z],
            [750.0, 750.0, z],
            [250.0, 750.0, z],
        ];
        let edges: [(i32, i32); 8] = [(1, 2), (2, 3), (3, 4), (4, 1), (6, 5), (7, 6), (8, 7), (5, 8)];
        let mut topology = Vec::new();
        for (a, b) in edges {
            topology.extend(a.to_le_bytes());
            topology.extend(b.to_le_bytes());
        }
        topology.extend(8.0f64.to_le_bytes());
        for k in [1.0f64, 2.0, 3.0, 4.0, -8.0, -5.0, -6.0, -7.0] {
            topology.extend(k.to_le_bytes());
        }
        body(&Spec { flags1: 0, kind: 4, form: 0x0301, v: 8, e: 8, f: 1, translate, vertices: &vertices, topology: &topology })
    }

    fn attribute() -> Vec<u8> {
        let mut a = vec![0x12, 0x00, 5, 0x01, 0x00, 0x00, 0xBE, 0x00];
        a.extend("Walls".encode_utf16().flat_map(u16::to_le_bytes));
        a.resize(8 + 16, 0);
        a
    }

    /// The stream of §7.1 plus the other cases.
    fn sample_stream() -> Vec<u8> {
        let mut s = Vec::new();
        let boxed = prism(0, 0x0301, [0.0; 3], [1000.0, 200.0, 3000.0]);
        assert_eq!(boxed.len(), 432);
        s.extend(header(0x64, 0, 0, (boxed.len() / 8) as u32));
        s.extend(&boxed);
        s.extend(attribute());
        s.extend([0u8; 16]);
        s.extend(plate_with_hole([10000.0, 0.0, 0.0]));
        s.extend(header(0x62, 0, 0, 0));
        s.extend(header(0x82, 0, 2, 0));
        s.extend([0x11u8; 32]);
        s.extend(prism(1, 0x0301, [2000.0, 0.0, 0.0], [3000.0, 200.0, 2000.0]));
        s.extend(prism(0, FORM_CLIP, [-1e6, -1e6, -10000.0], [1e6, 1e6, 100000.0]));
        // Garbage that is no record, then a body the reader finds again.
        s.extend([0xFFu8; 24]);
        s.extend(prism(0, 0x0301, [0.0, 5000.0, -300.0], [500.0, 5500.0, 0.0]));
        s
    }

    fn bounds(positions: &[f32]) -> ([f32; 3], [f32; 3]) {
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for p in positions.chunks_exact(3) {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (lo, hi)
    }

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    #[test]
    fn reads_the_header_name_and_bodies() {
        let data = file(3, "Ground floor plan", &sample_stream());
        let drawing = parse(&data).unwrap();
        assert_eq!(drawing.name, "Ground floor plan");
        assert_eq!(drawing.pages, 3);
        assert_eq!(drawing.bodies.len(), 4);
        assert_eq!(drawing.skipped_clip, 1);
        assert_eq!(drawing.skipped_unreadable, 0);
        let triangles: Vec<(bool, u8, usize)> =
            drawing.bodies.iter().map(|b| (b.auxiliary, b.kind, b.indices.len() / 3)).collect();
        assert_eq!(triangles, [(false, 1, 12), (false, 4, 8), (true, 1, 12), (false, 1, 12)]);
        assert_eq!(drawing.bodies[0].offset, 12 + 2 * PAGE + HEADER);
    }

    #[test]
    fn splits_solid_and_auxiliary_layers_in_metres() {
        let data = file(3, "Ground floor plan", &sample_stream());
        let source = Source::Bytes { name: "tb000151.NDW".into(), data: Arc::new(data) };
        let model = crate::loaders::load_with(&source, &Progress::default()).expect("Allplan");
        assert_eq!(model.format, "Allplan");
        let solid = model.layers.iter().find(|l| !l.hidden).unwrap();
        let auxiliary = model.layers.iter().find(|l| l.hidden).unwrap();
        assert_eq!(solid.triangle_count(), 32);
        assert_eq!(solid.elements.len(), 3);
        assert_eq!(auxiliary.triangle_count(), 12);
        assert_eq!(auxiliary.elements.len(), 1);
        let (lo, hi) = bounds(&solid.positions);
        assert!(close(lo, [0.0, 0.0, -0.3]) && close(hi, [11.0, 5.5, 5.0]), "{lo:?} {hi:?}");
        let (lo, hi) = bounds(&auxiliary.positions);
        assert!(close(lo, [2.0, 0.0, 0.0]) && close(hi, [3.0, 0.2, 2.0]), "{lo:?} {hi:?}");
        assert_eq!(model.props.len(), 4);
        assert!(model.props.values().all(|p| p.contains("Ground floor plan") && p.contains("0xC6")));
        let info = &model.project.info;
        assert!(info.contains(&("Allplan drawing file".into(), "Ground floor plan".into())));
        assert!(info.contains(&("Allplan pages".into(), "3".into())));
    }

    /// A kind 4 body from 1-based `vertices` (slot 0 added) and faces as loops of vertex numbers;
    /// edges are shared between faces and walked reversed where needed.
    fn explicit_body(vertices: &[[f64; 3]], faces: &[Vec<Vec<i32>>]) -> Vec<u8> {
        let mut edges: Vec<(i32, i32)> = Vec::new();
        let mut sizes = Vec::new();
        let mut entries = Vec::new();
        for face in faces {
            sizes.push(face.iter().map(Vec::len).sum::<usize>() as f64);
            for ring in face {
                for (i, &a) in ring.iter().enumerate() {
                    let b = ring[(i + 1) % ring.len()];
                    let k = match edges.iter().position(|&e| e == (a, b) || e == (b, a)) {
                        Some(k) if edges[k] == (a, b) => k as f64 + 1.0,
                        Some(k) => -(k as f64 + 1.0),
                        None => {
                            edges.push((a, b));
                            edges.len() as f64
                        }
                    };
                    entries.push(k);
                }
            }
        }
        let mut topology = Vec::new();
        edges.iter().for_each(|(a, b)| topology.extend(a.to_le_bytes().into_iter().chain(b.to_le_bytes())));
        sizes.iter().chain(&entries).for_each(|v| topology.extend(v.to_le_bytes()));
        let all: Vec<[f64; 3]> = std::iter::once([0.0; 3]).chain(vertices.iter().copied()).collect();
        body(&Spec {
            flags1: 0,
            kind: 4,
            form: 0x0301,
            v: vertices.len() as i32,
            e: edges.len() as u32,
            f: faces.len() as u32,
            translate: [0.0; 3],
            vertices: &all,
            topology: &topology,
        })
    }

    /// Every triangle of a convex body faces away from its middle, as its stored normal does.
    fn assert_outward(body: &Body) {
        let n = body.positions.len() as f64;
        let center: [f64; 3] = std::array::from_fn(|k| body.positions.iter().map(|p| p[k]).sum::<f64>() / n);
        for tri in body.indices.chunks_exact(3) {
            let [a, b, c] = [0, 1, 2].map(|k| body.positions[tri[k] as usize]);
            let n = cross(std::array::from_fn(|k| b[k] - a[k]), std::array::from_fn(|k| c[k] - a[k]));
            let out: [f64; 3] = std::array::from_fn(|k| a[k] - center[k]);
            assert!(dot(n, out) > 0.0, "inward triangle in body at 0x{:X}", body.offset);
            assert!(dot(n, body.normals[tri[0] as usize].map(f64::from)) > 0.0, "normal against winding");
        }
    }

    #[test]
    fn bodies_face_outwards_by_signed_volume() {
        // A prism with a counter-clockwise bottom ring, one with a clockwise ring, and a kind 4
        // cube whose loops run clockwise seen from outside (both of the latter turn round).
        let mut s = prism(0, 0x0301, [0.0; 3], [1000.0, 200.0, 3000.0]);
        let ring = [[0.0, 0.0], [0.0, 500.0], [500.0, 500.0], [500.0, 0.0]];
        let clockwise: Vec<[f64; 3]> =
            ring.iter().map(|p| [p[0], p[1], 0.0]).chain(ring.iter().map(|p| [p[0], p[1], 800.0])).collect();
        s.extend(body(&Spec { flags1: 0, kind: 0, form: 0x0301, v: -8, e: 12, f: 6, translate: [0.0; 3], vertices: &clockwise, topology: &[] }));
        let cube: Vec<[f64; 3]> = [[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0], [0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]]
            .iter()
            .map(|p: &[i32; 3]| p.map(|c| 4000.0 + 1000.0 * f64::from(c)))
            .collect();
        let inward = [[1, 2, 3, 4], [5, 8, 7, 6], [1, 5, 6, 2], [4, 3, 7, 8], [1, 4, 8, 5], [2, 6, 7, 3]];
        let faces: Vec<Vec<Vec<i32>>> = inward.iter().map(|f| vec![f.to_vec()]).collect();
        s.extend(explicit_body(&cube, &faces));
        let drawing = parse(&file(3, "Boxes", &s)).unwrap();
        assert_eq!(drawing.bodies.len(), 3);
        assert_eq!(drawing.bodies.iter().map(|b| b.indices.len() / 3).collect::<Vec<_>>(), [12, 12, 12]);
        drawing.bodies.iter().for_each(assert_outward);
    }

    /// A box prism of `kind` (0 or 1) from local (0, 0, 0) to (1000, 200, 3000), placed by the
    /// 12-value matrix `m`.
    fn placed_box(kind: u8, form: u16, m: [f64; 12]) -> Vec<u8> {
        let ring = [[0.0, 0.0], [1000.0, 0.0], [1000.0, 200.0], [0.0, 200.0]];
        let vertices: Vec<[f64; 3]> =
            ring.iter().map(|p| [p[0], p[1], 0.0]).chain(ring.iter().map(|p| [p[0], p[1], 3000.0])).collect();
        let mut r = body(&Spec { flags1: 0, kind, form, v: -8, e: 12, f: 6, translate: [0.0; 3], vertices: &vertices, topology: &[] });
        for (i, v) in m.iter().enumerate() {
            r[64 + 8 * i..72 + 8 * i].copy_from_slice(&v.to_le_bytes());
        }
        r
    }

    #[test]
    fn reads_kind_0_prisms_and_column_major_matrices() {
        const IDENTITY: [f64; 12] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];
        // Kind 0: no 8 constant bytes, so n8 = 48 instead of 49 (§7.4).
        let kind0 = placed_box(0, 0x0301, IDENTITY);
        assert_eq!(kind0.len(), 40 + 8 * 48);
        // 90° about z: x' = −y + tx, y' = x + ty (§7.4).
        let turned = placed_box(1, 0x0301, [0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 5000.0, 0.0, 100.0]);
        // Kind 0 with another form (not 0x0301, 0x0101 or 0x0201) is not known yet: skipped.
        let unknown = placed_box(0, 0x0401, IDENTITY);
        let mut s = kind0.clone();
        s.extend(&turned);
        s.extend(&unknown);
        let drawing = parse(&file(3, "Kind 0", &s)).unwrap();
        assert_eq!(drawing.bodies.len(), 2);
        assert_eq!(drawing.skipped_unreadable, 1);
        let extent = |body: &Body| {
            let mut lo = [f64::INFINITY; 3];
            let mut hi = [f64::NEG_INFINITY; 3];
            for p in &body.positions {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
            (lo, hi)
        };
        let (first, second) = (&drawing.bodies[0], &drawing.bodies[1]);
        assert_eq!((first.kind, first.indices.len() / 3), (0, 12));
        assert_eq!(extent(first), ([0.0, 0.0, 0.0], [1000.0, 200.0, 3000.0]));
        assert_eq!(extent(second), ([4800.0, 0.0, 100.0], [5000.0, 1000.0, 3100.0]));
        assert_outward(first);
        assert_outward(second);
    }

    #[test]
    fn a_header_with_low_bits_in_byte_0_is_invalid() {
        // A body whose byte 0 has low bits set is no record: the reader resyncs past it.
        let a = prism(0, 0x0301, [0.0; 3], [1000.0, 200.0, 3000.0]);
        let mut bad = prism(0, 0x0301, [2000.0, 0.0, 0.0], [3000.0, 200.0, 3000.0]);
        bad[0] = 0x7B;
        let c = prism(0, 0x0301, [4000.0, 0.0, 0.0], [5000.0, 200.0, 3000.0]);
        let mut s = a.clone();
        s.extend(&bad);
        s.extend(&c);
        let drawing = parse(&file(3, "Low bits", &s)).unwrap();
        let offsets: Vec<usize> = drawing.bodies.iter().map(|b| b.offset).collect();
        assert_eq!(offsets, [STREAM_START, STREAM_START + a.len() + bad.len()]);
    }

    #[test]
    fn plate_hole_is_left_open() {
        let drawing = parse(&file(3, "Plate", &plate_with_hole([0.0; 3]))).unwrap();
        let body = &drawing.bodies[0];
        let mut area = 0.0;
        for tri in body.indices.chunks_exact(3) {
            let [a, b, c] = [0, 1, 2].map(|k| body.positions[tri[k] as usize]);
            area += ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs() / 2.0;
        }
        assert!((area - 750_000.0).abs() < 1e-6, "area {area}");
        assert!(body.positions.iter().all(|p| p[0] < 1e6), "slot 0 used");
    }

    #[test]
    fn empty_drawing_file_has_no_bodies() {
        let data = file(2, "QUICKSTART", &[]);
        assert_eq!(data.len(), 12 + 2 * PAGE + 64);
        let drawing = parse(&data).unwrap();
        assert_eq!((drawing.name.as_str(), drawing.pages, drawing.bodies.len()), ("QUICKSTART", 2, 0));
        let error = build(&drawing, None).err().unwrap();
        assert!(error.contains("no 3D bodies"), "{error}");
    }

    #[test]
    fn rejects_other_files() {
        assert!(parse(b"").is_err());
        assert!(parse(b"solid cube\nendsolid\n").unwrap_err().contains("not an Allplan"));
        let mut layout = file(3, "Layout", &[]);
        layout[2] = 0;
        assert!(parse(&layout).unwrap_err().contains("layout"));
    }

    #[test]
    fn truncated_or_garbage_input_does_not_panic() {
        let data = file(3, "Ground floor plan", &sample_stream());
        let stream_end = 12 + 2 * PAGE + sample_stream().len();
        for cut in (0..64).chain((NAME_OFFSET..NAME_OFFSET + 150).step_by(7)).chain((12 + 2 * PAGE - 8..stream_end + 8).step_by(3)) {
            let _ = parse(&data[..cut.min(data.len())]);
        }
        // A garbage tail right up to T, and pseudo-random bytes across the whole stream.
        let mut tail = data.clone();
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for b in &mut tail[stream_end..12 + 3 * PAGE] {
            *b = next() as u8;
        }
        // The body found by resynchronisation is lost (no valid record follows it); the rest stays.
        let drawing = parse(&tail).unwrap();
        assert!(drawing.bodies.len() >= 3);
        for round in 0..200 {
            let mut fuzz = data.clone();
            for _ in 0..(1 + round % 40) {
                let at = 12 + 2 * PAGE + (next() as usize) % (stream_end - 12 - 2 * PAGE);
                fuzz[at] = next() as u8;
            }
            let _ = parse(&fuzz).map(|d| build(&d, None));
        }
        // A header whose page count points far beyond the end of the file.
        let mut long = data[..12 + 2 * PAGE + 200].to_vec();
        long[8..10].copy_from_slice(&0xFFFEu16.to_le_bytes());
        let _ = parse(&long);
    }

    #[test]
    fn resync_past_many_headers_into_one_long_padding_run_is_fast() {
        // About 10 MB: garbage that forces a resync, then 100 000 strictly valid headers whose
        // records all end at the start of one long run of padding followed by garbage, so no
        // candidate is valid; near the end a real body that the resync must still find.
        let pages = 320u16;
        let mut data = file(pages, "Crafted", &[]);
        let end = 12 + usize::from(pages) * PAGE;
        let headers = 100_000;
        let region = STREAM_START + 8 + HEADER * headers;
        data[STREAM_START..STREAM_START + 8].fill(0xFF);
        for i in 0..headers {
            let q = STREAM_START + 8 + HEADER * i;
            let n8 = ((region - q - HEADER) / 8) as u32;
            data[q..q + HEADER].copy_from_slice(&header(0x82, 0, 0, n8));
        }
        let boxed = prism(0, 0x0301, [0.0; 3], [1000.0, 200.0, 3000.0]);
        let body_at = end - 4096;
        data[body_at - 64..body_at].fill(0xFF);
        data[body_at..body_at + boxed.len()].copy_from_slice(&boxed);
        let started = web_time::Instant::now();
        let drawing = parse(&data).unwrap();
        let elapsed = started.elapsed();
        eprintln!("crafted 10 MB file parsed in {elapsed:?}");
        assert!(elapsed < std::time::Duration::from_secs(2), "took {elapsed:?}");
        assert_eq!(drawing.bodies.len(), 1);
        assert_eq!(drawing.bodies[0].offset, body_at);
    }

    /// Sets the key (u32@24, u16@12) of a record (§8).
    fn keyed(mut record: Vec<u8>, key: Key) -> Vec<u8> {
        record[24..28].copy_from_slice(&key.0.to_le_bytes());
        record[12..14].copy_from_slice(&key.1.to_le_bytes());
        record
    }

    /// An element record (§7.3): a rectangular outline (n16 = 4) and `n8` blocks of 8 bytes
    /// (36 for the full record of 392 bytes), so q = 104.
    fn element(key: Key, marker: bool, name: &str, quantities: [f64; 5], type_code: Option<(u8, u8)>, n8: u32) -> Vec<u8> {
        let mut r = header(0xEF, 0, 4, n8);
        for (x, y) in [(0.0f64, 0.0f64), (1000.0, 0.0), (1000.0, 200.0), (0.0, 200.0)] {
            r.extend(x.to_le_bytes());
            r.extend(y.to_le_bytes());
        }
        r.resize(HEADER + 64 + 8 * n8 as usize, 0);
        let q = HEADER + 64;
        if marker {
            r[q + 8..q + 12].copy_from_slice(b"||||");
        }
        let mut units: Vec<u16> = name.encode_utf16().collect();
        units.resize(36, 0x20);
        for (i, u) in units.iter().enumerate() {
            r[q + 32 + 2 * i..q + 34 + 2 * i].copy_from_slice(&u.to_le_bytes());
        }
        for (i, v) in quantities.iter().enumerate() {
            r[q + 104 + 8 * i..q + 112 + 8 * i].copy_from_slice(&v.to_le_bytes());
        }
        if let Some((group, subtype)) = type_code.filter(|_| q + 284 <= r.len()) {
            r[q + 280] = 0x81;
            r[q + 282] = group;
            r[q + 283] = subtype;
        }
        keyed(r, key)
    }

    fn props_of(loaded: &Loaded, offset: usize) -> serde_json::Value {
        let text = loaded.layers.iter().flat_map(|l| &l.props).find(|(id, _)| *id == offset as u64).map(|(_, p)| p).unwrap();
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn links_bodies_to_element_records() {
        let q = [2.75, 0.24, 9.0, 2.16, 5.94];
        let at = |i: f64| prism(0, 0x0301, [1000.0 * i, 0.0, 0.0], [1000.0 * i + 500.0, 200.0, 3000.0]);
        let mut s = Vec::new();
        let mut offsets = Vec::new();
        let mut push = |s: &mut Vec<u8>, record: Vec<u8>, is_body: bool| {
            if is_body {
                offsets.push(STREAM_START + s.len());
            }
            s.extend(record);
        };
        // 0: element before its body; the name is a catalogue code.
        push(&mut s, element((1, 1), true, "WA-RFC\\", q, Some((0x01, 0x01)), 36), false);
        push(&mut s, keyed(at(0.0), (1, 1)), true);
        // 1: body in a container, element after it.
        push(&mut s, header(0x64, 0, 0, 54), false);
        push(&mut s, keyed(at(1.0), (2, 1)), true);
        push(&mut s, element((2, 1), true, "Floor", [0.3; 5], Some((0x01, 0x04)), 36), false);
        // 2: two element records with one key: the first wins.
        push(&mut s, keyed(at(2.0), (3, 1)), true);
        push(&mut s, element((3, 1), true, "First", [0.1; 5], Some((0x01, 0x47)), 36), false);
        push(&mut s, element((3, 1), true, "Second", [0.2; 5], Some((0x01, 0x01)), 36), false);
        // 3: element without '||||': not linked.
        push(&mut s, keyed(at(3.0), (4, 1)), true);
        push(&mut s, element((4, 1), false, "No data", [0.4; 5], Some((0x01, 0x01)), 36), false);
        // 4: element too short for the type (q + 284 past the end): name and quantities only.
        push(&mut s, keyed(at(4.0), (5, 1)), true);
        push(&mut s, element((5, 1), true, "Short", [0.5; 5], Some((0x01, 0x01)), 18), false);
        // 5: no element with this key.
        push(&mut s, keyed(at(5.0), (6, 1)), true);
        // 6: auxiliary window opening.
        push(&mut s, keyed(prism(1, 0x0301, [6000.0, 0.0, 900.0], [6500.0, 200.0, 2100.0]), (7, 1)), true);
        push(&mut s, element((7, 1), true, "Window", [1.2; 5], Some((0x59, 0x17)), 36), false);
        // 7: a type code not in the table.
        push(&mut s, keyed(at(7.0), (8, 1)), true);
        push(&mut s, element((8, 1), true, "", [0.0; 5], Some((0x77, 0x01)), 36), false);
        // 8: same u32 as element 0 but another u16: not linked.
        push(&mut s, keyed(at(8.0), (1, 2)), true);
        // 9: a first record without data does not count; a later one with data does (§8.1).
        push(&mut s, element((9, 1), false, "No data", [0.9; 5], Some((0x01, 0x01)), 36), false);
        push(&mut s, keyed(at(9.0), (9, 1)), true);
        push(&mut s, element((9, 1), true, "Later", [0.6; 5], Some((0x01, 0x06)), 36), false);
        // 10: u32@24 = 0 is no key: neither the element record nor the body takes part (§8.1).
        push(&mut s, element((0, 5), true, "No key", [0.7; 5], Some((0x01, 0x01)), 36), false);
        push(&mut s, keyed(at(10.0), (0, 5)), true);

        let drawing = parse(&file(3, "Elements", &s)).unwrap();
        assert_eq!(drawing.bodies.iter().map(|b| b.offset).collect::<Vec<_>>(), offsets);
        let element_of = |i: usize| drawing.bodies[i].element.map(|e| &drawing.elements[e]);
        let linked: Vec<bool> = (0..11).map(|i| element_of(i).is_some()).collect();
        assert_eq!(linked, [true, true, true, false, true, false, true, true, false, true, false]);
        assert_eq!(element_of(9).unwrap().name, "Later");
        assert!(drawing.elements.iter().all(|e| e.name != "No key"));
        let wall = element_of(0).unwrap();
        assert_eq!((wall.name.as_str(), wall.quantities, wall.type_code), ("WA-RFC", q, Some((0x01, 0x01))));
        assert_eq!(element_of(2).unwrap().name, "First");
        let short = element_of(4).unwrap();
        assert_eq!((short.name.as_str(), short.quantities, short.type_code), ("Short", [0.5; 5], None));

        let loaded = build(&drawing, None).unwrap();
        let layers: Vec<(&str, usize, bool)> =
            loaded.layers.iter().map(|l| (l.name.as_str(), l.elements.len(), l.hidden)).collect();
        assert_eq!(
            layers,
            [
                ("Walls", 1, false),
                ("Slabs", 1, false),
                ("Beams", 1, false),
                ("Stairs", 1, false),
                ("Other", 6, false),
                ("Auxiliary bodies (openings, rooms)", 1, true)
            ]
        );
        let colors: std::collections::HashSet<String> = loaded.layers.iter().map(|l| format!("{:?}", l.color)).collect();
        assert_eq!(colors.len(), loaded.layers.len());

        let wall = props_of(&loaded, offsets[0]);
        assert_eq!(wall["Name"], "WA-RFC");
        assert_eq!(wall["Element type"], "Wall");
        assert_eq!(wall["Type code"], "01 01");
        assert_eq!(wall["Height (m)"], 2.75);
        assert_eq!(wall["Volume (m³)"], 5.94);
        let short = props_of(&loaded, offsets[4]);
        assert_eq!(short["Name"], "Short");
        assert!(short.get("Element type").is_none() && short.get("Type code").is_none());
        assert_eq!(props_of(&loaded, offsets[6])["Element type"], "Window opening");
        let unknown = props_of(&loaded, offsets[7]);
        assert_eq!(unknown["Element type"], "Element 77 01");
        assert!(unknown.get("Name").is_none());
        let loose = props_of(&loaded, offsets[5]);
        assert!(loose.get("Name").is_none() && loose.get("Height (m)").is_none());
        assert!(loaded.project.info.contains(&("Allplan bodies linked to an element".into(), "7".into())));
    }

    #[test]
    fn resync_keeps_a_body_followed_by_padding_to_the_end() {
        // Garbage forces a resync onto the body; after it run more than three pages of padding.
        let mut s = vec![0xFFu8; 8];
        s.extend(prism(0, 0x0301, [0.0; 3], [1000.0, 200.0, 3000.0]));
        let mut data = file(5, "Last body", &s);
        assert_eq!(parse(&data).unwrap().bodies.len(), 1);
        // The same padding followed by data is longer than a page: the candidate is not valid.
        data[12 + 5 * PAGE - 64..12 + 5 * PAGE].fill(0xFF);
        assert_eq!(parse(&data).unwrap().bodies.len(), 0);
    }

    /// A kind-0 open surface (form 0x0201) of one triangle: |V| = 3 vertices and edges indexing
    /// them from 1 (§9.3).
    fn open_triangle(flags1: u8, corners: [[f64; 3]; 3]) -> Vec<u8> {
        let mut topology = Vec::new();
        for (a, b) in [(1i32, 2i32), (2, 3), (3, 1)] {
            topology.extend(a.to_le_bytes());
            topology.extend(b.to_le_bytes());
        }
        for v in [3.0f64, 1.0, 2.0, 3.0] {
            topology.extend(v.to_le_bytes());
        }
        body(&Spec { flags1, kind: 0, form: FORM_OPEN, v: 3, e: 3, f: 1, translate: [0.0; 3], vertices: &corners, topology: &topology })
    }

    /// A representation (0x50): 3D (byte 81 = 1) or 2D (byte 80 = 1), nested records from 216.
    fn representation(three_d: bool, nested: &[u8]) -> Vec<u8> {
        let len = REPRESENTATION_BODY + nested.len();
        let mut r = header(0x50, 0, 0, ((len - HEADER) / 8) as u32);
        r.resize(REPRESENTATION_BODY, 0);
        r[80] = u8::from(!three_d);
        r[81] = u8::from(three_d);
        r.extend_from_slice(nested);
        r
    }

    fn utf16_into(r: &mut [u8], at: usize, text: &str, units: usize) {
        let mut all: Vec<u16> = text.encode_utf16().collect();
        all.resize(units, 0x20);
        for (i, u) in all.iter().enumerate() {
            r[at + 2 * i..at + 2 * i + 2].copy_from_slice(&u.to_le_bytes());
        }
    }

    /// A macro definition (0x5A): number, name and representations from offset 376 (§9.1).
    fn definition(number: u32, name: &str, representations: &[Vec<u8>]) -> Vec<u8> {
        let start = 376;
        let len = start + representations.iter().map(Vec::len).sum::<usize>();
        let mut d = header(0x5A, 0, 0, ((len - HEADER) / 8) as u32);
        d.resize(start, 0);
        d[28..32].copy_from_slice(&number.to_le_bytes());
        d[48..52].copy_from_slice(b"||||");
        utf16_into(&mut d, 72, name, 32);
        representations.iter().for_each(|r| d.extend_from_slice(r));
        d
    }

    /// A macro placement (0xEB) with a four-point outline, a name and a matrix at [len − 104,
    /// len − 8) (§9.2).
    fn placement(number: u32, name: &str, m: [f64; 12]) -> Vec<u8> {
        let mut r = header(0xEB, 0, 4, 24);
        r.resize(296, 0);
        r[28..32].copy_from_slice(&number.to_le_bytes());
        let q = HEADER + 64;
        r[q + 8..q + 12].copy_from_slice(b"||||");
        utf16_into(&mut r, q + 16, name, 32);
        for (i, v) in m.iter().enumerate() {
            r[192 + 8 * i..200 + 8 * i].copy_from_slice(&v.to_le_bytes());
        }
        r
    }

    /// Every triangle of a convex mesh faces away from its middle, as its stored normal does.
    fn assert_outward_mesh(positions: &[[f64; 3]], normals: &[[f32; 3]], indices: &[u32]) {
        let n = positions.len() as f64;
        let center: [f64; 3] = std::array::from_fn(|k| positions.iter().map(|p| p[k]).sum::<f64>() / n);
        for tri in indices.chunks_exact(3) {
            let [a, b, c] = [0, 1, 2].map(|k| positions[tri[k] as usize]);
            let n = cross(std::array::from_fn(|k| b[k] - a[k]), std::array::from_fn(|k| c[k] - a[k]));
            assert!(dot(n, std::array::from_fn(|k| a[k] - center[k])) > 0.0, "inward triangle");
            assert!(dot(n, normals[tri[0] as usize].map(f64::from)) > 0.0, "normal against winding");
        }
    }

    const MOVE: [f64; 12] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 10000.0, 0.0, 0.0];
    const TURN: [f64; 12] = [0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 5000.0, 0.0];
    const MIRROR: [f64; 12] = [-1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 20000.0, 0.0, 0.0];

    /// The macro stream of §7.5 plus the other cases; returns it with the placement offsets.
    fn macro_stream() -> (Vec<u8>, Vec<usize>) {
        let boxed = prism(0, 0x0301, [0.0; 3], [1000.0, 200.0, 3000.0]);
        let triangle = open_triangle(0, [[0.0, 0.0, 3000.0], [1000.0, 0.0, 3000.0], [0.0, 200.0, 3000.0]]);
        let opening = prism(1, 0x0301, [100.0, 0.0, 100.0], [900.0, 200.0, 900.0]);
        let mut three_d = boxed.clone();
        three_d.extend(&triangle);
        three_d.extend(&opening);
        let flat = prism(0, 0x0301, [0.0; 3], [5e5, 5e5, 1.0]);
        let chair = definition(7, "Stuhl", &[representation(false, &flat), representation(true, &three_d)]);
        let second = definition(7, "Second", &[representation(true, &flat)]);
        let plan_only = definition(8, "Nur 2D", &[representation(false, &flat)]);
        let mut short = header(0xEB, 0, 0, 3);
        short.resize(HEADER + 24, 0);
        short[28..32].copy_from_slice(&7u32.to_le_bytes());

        let mut s = Vec::new();
        let mut placements = Vec::new();
        let mut put = |s: &mut Vec<u8>, record: Vec<u8>, is_placement: bool| {
            if is_placement {
                placements.push(STREAM_START + s.len());
            }
            s.extend(record);
        };
        // 0: placed before its definition.
        put(&mut s, placement(7, "Chair", MOVE), true);
        put(&mut s, chair, false);
        // 1: no name of its own: the macro's name.
        put(&mut s, placement(7, "", TURN), true);
        // 2: mirrored.
        put(&mut s, placement(7, "Mirrored chair", MIRROR), true);
        // 3: an unknown macro, 4: a macro without 3D, 5: a catalogue name, 6: too short.
        put(&mut s, placement(99, "Ghost", MOVE), true);
        put(&mut s, plan_only, false);
        put(&mut s, placement(8, "Plan", MOVE), true);
        put(&mut s, placement(7, "ID-WD\\", MOVE), true);
        put(&mut s, short, true);
        // A later definition with the same number does not count.
        put(&mut s, second, false);
        // An open surface in the drawing itself, its loop running so that a closed body would
        // turn round.
        put(&mut s, open_triangle(0, [[0.0, 0.0, 0.0], [1000.0, 0.0, 1000.0], [1000.0, 1000.0, 0.0]]), false);
        (s, placements)
    }

    #[test]
    fn places_macros() {
        let (s, offsets) = macro_stream();
        let data = file(3, "Macros", &s);
        let drawing = parse(&data).unwrap();
        let placed: Vec<Placed> = drawing.placements.iter().map(|p| p.placed).collect();
        assert_eq!(
            placed,
            [
                Placed::Resolved(0),
                Placed::Resolved(0),
                Placed::Resolved(0),
                Placed::UnknownMacro,
                Placed::Without3d,
                Placed::Resolved(0),
                Placed::Unreadable
            ]
        );
        assert_eq!(drawing.placements.iter().map(|p| p.offset).collect::<Vec<_>>(), offsets);
        // One definition read: the 2D representation and the later duplicate are left out.
        assert_eq!(drawing.macros.len(), 1);
        let chair = &drawing.macros[0];
        assert_eq!((chair.number, chair.name.as_str(), chair.bodies.len()), (7, "Stuhl", 3));
        let kinds: Vec<(u8, bool, bool, usize)> =
            chair.bodies.iter().map(|b| (b.kind, b.open, b.auxiliary, b.indices.len() / 3)).collect();
        assert_eq!(kinds, [(1, false, false, 12), (0, true, false, 1), (1, false, true, 12)]);

        // The open surface in the drawing keeps the direction of its loop.
        assert_eq!(drawing.bodies.len(), 1);
        let surface = &drawing.bodies[0];
        assert!(surface.open);
        let third = 1.0 / 3f32.sqrt();
        assert!(surface.normals.iter().all(|n| (n[0] + third).abs() < 1e-5 && (n[1] - third).abs() < 1e-5 && (n[2] - third).abs() < 1e-5));

        // Placed: moved, turned 90° about z, mirrored (still facing outwards).
        let extent = |m: &[f64; 12]| {
            let (positions, normals, indices) = place(&chair.bodies[0], m);
            assert_outward_mesh(&positions, &normals, &indices);
            let mut lo = [f64::INFINITY; 3];
            let mut hi = [f64::NEG_INFINITY; 3];
            for p in &positions {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
            (lo, hi)
        };
        assert_eq!(extent(&MOVE), ([10000.0, 0.0, 0.0], [11000.0, 200.0, 3000.0]));
        assert_eq!(extent(&TURN), ([-200.0, 5000.0, 0.0], [0.0, 6000.0, 3000.0]));
        assert_eq!(extent(&MIRROR), ([19000.0, 0.0, 0.0], [20000.0, 200.0, 3000.0]));

        let loaded = build(&drawing, None).unwrap();
        let layers: Vec<(&str, usize, usize, bool)> =
            loaded.layers.iter().map(|l| (l.name.as_str(), l.elements.len(), l.triangle_count(), l.hidden)).collect();
        assert_eq!(
            layers,
            [("Other", 1, 1, false), ("Macros", 4, 4 * 13, false), ("Auxiliary bodies (openings, rooms)", 4, 4 * 12, true)]
        );
        let macros = &loaded.layers[1];
        let ids: Vec<u64> = macros.elements.iter().map(|e| e.0).collect();
        assert_eq!(ids, [offsets[0], offsets[1], offsets[2], offsets[5]].map(|o| o as u64));
        let first = props_of(&loaded, offsets[0]);
        assert_eq!((first["Name"].as_str(), first["Macro"].as_str()), (Some("Chair"), Some("Stuhl")));
        assert_eq!((first["Macro number"].as_u64(), first["Bodies"].as_u64()), (Some(7), Some(3)));
        assert_eq!(props_of(&loaded, offsets[1])["Name"], "Stuhl");
        assert_eq!(props_of(&loaded, offsets[5])["Name"], "ID-WD");
        let summary = loaded.project.info.iter().find(|(k, _)| k == "Allplan macro placements").map(|(_, v)| v.as_str());
        assert_eq!(summary, Some("7 (shown 4, without 3D 1, unknown macro 1, unreadable 1, over the triangle limit 0)"));
    }

    #[test]
    fn a_large_macro_is_read_once_and_placed_quickly() {
        // tb000152 places 3268 macro bodies with 25 placements; ten times that: one macro of 1320
        // boxes, placed 25 times (33 000 bodies).
        let mut nested = Vec::new();
        for i in 0..1320 {
            let x = f64::from(i % 60) * 100.0;
            let y = f64::from(i / 60) * 100.0;
            nested.extend(prism(0, 0x0301, [x, y, 0.0], [x + 50.0, y + 50.0, 400.0]));
        }
        let mut s = definition(42, "Shelf", &[representation(true, &nested)]);
        for i in 0..25 {
            let mut m = MOVE;
            m[9] = 10000.0 * f64::from(i);
            s.extend(placement(42, "Shelf", m));
        }
        let pages = u16::try_from(s.len() / PAGE + 3).unwrap();
        let data = file(pages, "Large macro", &s);
        let started = web_time::Instant::now();
        let drawing = parse(&data).unwrap();
        let parsed = started.elapsed();
        let loaded = build(&drawing, None).unwrap();
        let elapsed = started.elapsed();
        eprintln!("1320 macro bodies placed 25 times in {elapsed:?} (read {parsed:?})");
        assert!(drawing.bodies.is_empty());
        assert_eq!(drawing.macros.len(), 1);
        assert_eq!(drawing.macros[0].bodies.len(), 1320);
        let macros = loaded.layers.iter().find(|l| l.name == "Macros").unwrap();
        assert_eq!((macros.elements.len(), macros.triangle_count()), (25, 25 * 1320 * 12));
        assert!(elapsed < std::time::Duration::from_secs(5), "took {elapsed:?}");
    }

    #[test]
    fn macro_records_with_low_bits_in_byte_0_are_read() {
        // Inside a macro, byte 0 of a header is not checked (§9.1): nested records mostly have
        // 0x7A or 0x7E there, and some representations too.
        let mut boxed = prism(0, 0x0301, [0.0; 3], [1000.0, 200.0, 3000.0]);
        boxed[0] = 0x7A;
        let mut container = header(0x64, 0, 0, (boxed.len() / 8) as u32);
        container[0] = 0x7E;
        let mut triangle = open_triangle(0, [[0.0, 0.0, 0.0], [500.0, 0.0, 0.0], [0.0, 500.0, 0.0]]);
        triangle[0] = 0x7E;
        let mut nested = container;
        nested.extend(&boxed);
        nested.extend(&triangle);
        let mut three_d = representation(true, &nested);
        three_d[0] = 0x7E;
        let mut s = definition(5, "Cupboard", &[three_d]);
        s.extend(placement(5, "Hall cupboard", MOVE));
        let drawing = parse(&file(3, "Low bits in macros", &s)).unwrap();
        assert_eq!(drawing.placements[0].placed, Placed::Resolved(0));
        let bodies: Vec<(u8, usize)> = drawing.macros[0].bodies.iter().map(|b| (b.kind, b.indices.len() / 3)).collect();
        assert_eq!(bodies, [(1, 12), (0, 1)]);
        // In the drawing's own stream the rule stays: such a body is no record there.
        assert!(drawing.bodies.is_empty());
        let mut alone = prism(0, 0x0301, [0.0; 3], [1000.0, 200.0, 3000.0]);
        alone[0] = 0x7A;
        assert!(parse(&file(3, "Low bits", &alone)).unwrap().bodies.is_empty());
    }

    #[test]
    fn broken_macros_do_not_panic() {
        let (s, _) = macro_stream();
        let data = file(3, "Macros", &s);
        let end = STREAM_START + s.len();
        for cut in (STREAM_START..end + 8).step_by(5) {
            let _ = parse(&data[..cut]).map(|d| build(&d, None));
        }
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for round in 0..300 {
            let mut fuzz = data.clone();
            for _ in 0..(1 + round % 30) {
                let at = STREAM_START + (next() as usize) % (end - STREAM_START);
                fuzz[at] = next() as u8;
            }
            let _ = parse(&fuzz).map(|d| build(&d, None));
        }
    }

    /// Appends a tail to a body record (padded to 8 bytes, length updated) and sets its colour
    /// number (u16@20).
    fn with_tail(mut record: Vec<u8>, tail: &[u8], color_number: u16) -> Vec<u8> {
        let mut tail = tail.to_vec();
        tail.resize(tail.len().div_ceil(8) * 8, 0);
        let n8 = (u32::from(record[17]) << 16 | u32::from(u16::from_le_bytes([record[18], record[19]]))) + (tail.len() / 8) as u32;
        record[17] = (n8 >> 16) as u8;
        record[18..20].copy_from_slice(&(n8 as u16).to_le_bytes());
        record[20..22].copy_from_slice(&color_number.to_le_bytes());
        record.extend(tail);
        record
    }

    /// A body tail with a surface name at +16, after 8 zero bytes and the double 1.0 (§10.1).
    fn surface_tail(name: &str) -> Vec<u8> {
        let mut tail = vec![0u8; 8];
        tail.extend(1.0f64.to_le_bytes());
        tail.extend(name.encode_utf16().flat_map(u16::to_le_bytes));
        tail.extend([0, 0]);
        tail
    }

    /// An 8-bit palette preview: width, height, pixels (palette indices) and 256 RGB entries.
    struct Preview {
        w: u16,
        h: u16,
        pixels: Vec<u8>,
        palette: Vec<[u8; 3]>,
        compression: u16,
    }

    /// A TIFF entry: tag, type, count and value bytes.
    type Entry = (u16, u16, u32, Vec<u8>);

    /// A little-endian TIFF: `pixels` at offset 8, then the values that do not fit in their entry,
    /// then the IFD.
    fn tiff_file(pixels: &[u8], entries: &[Entry]) -> Vec<u8> {
        let mut data = b"II*\0\0\0\0\0".to_vec();
        data.extend_from_slice(pixels);
        let mut fields = Vec::new();
        for (tag, kind, count, bytes) in entries {
            let value = if bytes.len() <= 4 {
                let mut v = bytes.clone();
                v.resize(4, 0);
                v
            } else {
                if data.len() % 2 == 1 {
                    data.push(0);
                }
                let at = data.len() as u32;
                data.extend_from_slice(bytes);
                at.to_le_bytes().to_vec()
            };
            fields.push((*tag, *kind, *count, value));
        }
        if data.len() % 2 == 1 {
            data.push(0);
        }
        let ifd = data.len() as u32;
        data[4..8].copy_from_slice(&ifd.to_le_bytes());
        data.extend((fields.len() as u16).to_le_bytes());
        for (tag, kind, count, value) in fields {
            data.extend(tag.to_le_bytes());
            data.extend(kind.to_le_bytes());
            data.extend(count.to_le_bytes());
            data.extend(value);
        }
        data.extend([0; 4]);
        data
    }

    fn palette_entry(palette: &[[u8; 3]]) -> Entry {
        let map: Vec<u8> = (0..3).flat_map(|k| palette.iter().map(move |c| u16::from(c[k]) << 8)).flat_map(u16::to_le_bytes).collect();
        (320, 3, 768, map)
    }

    /// A `.surf`: a little-endian TIFF with tag 65001 (RGB), optionally tag 65019 (texture) and a
    /// palette preview (§10.2).
    fn surf(rgb: [u8; 3], texture: Option<&str>, preview: Option<&Preview>) -> Vec<u8> {
        let mut entries: Vec<Entry> = vec![(65001, 1, 3, rgb.to_vec())];
        if let Some(texture) = texture {
            let bytes: Vec<u8> = texture.encode_utf16().flat_map(u16::to_le_bytes).collect();
            entries.push((65019, 1, bytes.len() as u32, bytes));
        }
        let Some(p) = preview else { return tiff_file(&[], &entries) };
        let short = |v: u16| v.to_le_bytes().to_vec();
        entries.extend([
            (256, 3, 1, short(p.w)),
            (257, 3, 1, short(p.h)),
            (258, 3, 1, short(8)),
            (259, 3, 1, short(p.compression)),
            (262, 3, 1, short(3)),
            (273, 4, 1, 8u32.to_le_bytes().to_vec()),
            (277, 3, 1, short(1)),
            (278, 3, 1, short(p.h)),
            (279, 4, 1, (p.pixels.len() as u32).to_le_bytes().to_vec()),
            palette_entry(&p.palette),
        ]);
        tiff_file(&p.pixels, &entries)
    }

    /// An `animation.cfg`: version field, then per entry the name length, `*N`, a gap of 2 or 4
    /// bytes, the u32 N and R, G, B (§10.3).
    fn cfg(version: u16, gap: usize, entries: &[(u32, [u8; 3])]) -> Vec<u8> {
        let mut data = vec![0x01, 0x00];
        data.extend(version.to_le_bytes());
        data.extend([0x11; 6]);
        for (n, rgb) in entries {
            let name: Vec<u8> = format!("*{n}").encode_utf16().flat_map(u16::to_le_bytes).collect();
            data.extend((name.len() as u16).to_le_bytes());
            data.extend(&name);
            data.extend(vec![0u8; gap]);
            data.extend(n.to_le_bytes());
            data.extend(rgb);
            data.extend([0x40, 0x00, 0x00]);
        }
        data
    }

    /// Project files in memory, found case-insensitively.
    struct Memory(HashMap<String, Vec<u8>>);

    impl Memory {
        fn new(files: &[(&str, Vec<u8>)]) -> Self {
            Self(files.iter().map(|(k, v)| (k.to_lowercase(), v.clone())).collect())
        }
    }

    impl ProjectFiles for Memory {
        fn read(&mut self, path: &[&str]) -> Option<Vec<u8>> {
            self.0.get(&path.join("/").to_lowercase()).cloned()
        }
    }

    fn middle_preview(compression: u16) -> Preview {
        // White around, the middle square (columns and rows 3..=4 of 8) red, green, blue and grey.
        let mut pixels = vec![0u8; 64];
        for (i, at) in [3 * 8 + 3, 3 * 8 + 4, 4 * 8 + 3, 4 * 8 + 4].into_iter().enumerate() {
            pixels[at] = i as u8 + 1;
        }
        let mut palette = vec![[255, 255, 255]; 256];
        palette[1] = [100, 0, 0];
        palette[2] = [0, 100, 0];
        palette[3] = [0, 0, 100];
        palette[4] = [100, 100, 100];
        Preview { w: 8, h: 8, pixels, palette, compression }
    }

    #[test]
    fn reads_colour_tables_of_both_layouts() {
        let entries = [(0, [0, 0, 0]), (1, [255, 255, 255]), (21, [73, 73, 73]), (255, [1, 2, 3])];
        let want: HashMap<u16, [u8; 3]> = entries.iter().map(|&(n, c)| (n as u16, c)).collect();
        assert_eq!(color_table(&cfg(2, 2, &entries)), want);
        assert_eq!(color_table(&cfg(3, 4, &entries)), want);
        // The layout decides, not the version field (a 2025 project: 0x0103 with the +4 layout).
        assert_eq!(color_table(&cfg(0x0103, 4, &entries)), want);
        assert!(color_table(b"").is_empty() && color_table(b"*\0").is_empty());
    }

    #[test]
    fn reads_surface_colours() {
        assert_eq!(surface_color(&surf([10, 20, 30], None, None)), Some(([10, 20, 30], false)));
        assert_eq!(surface_color(&surf([10, 20, 30], Some(""), None)), Some(([10, 20, 30], false)));
        // With a texture: the mean of the middle square of the preview.
        let textured = surf([200, 200, 200], Some("FFC\\PARKETT\\FFC-Parkett.jpg"), Some(&middle_preview(1)));
        assert_eq!(surface_color(&textured), Some(([50, 50, 50], true)));
        // A preview kind not supported (compressed): the tint of 65001.
        let packed = surf([200, 200, 200], Some("x.jpg"), Some(&middle_preview(5)));
        assert_eq!(surface_color(&packed), Some(([200, 200, 200], true)));
        assert_eq!(surface_color(b"MM\0*"), None);
    }

    /// A crafted textured `.surf`: a `side` × `side` preview, one row per strip, every strip at
    /// the same 4096 bytes of palette index 1 (§10.2 review).
    fn shared_strips(side: u32) -> Vec<u8> {
        let long = |v: u32| v.to_le_bytes().to_vec();
        let short = |v: u16| v.to_le_bytes().to_vec();
        let strips: Vec<u8> = (0..side).flat_map(|_| 8u32.to_le_bytes()).collect();
        let counts: Vec<u8> = (0..side).flat_map(|_| 4096u32.to_le_bytes()).collect();
        let mut palette = vec![[255, 255, 255]; 256];
        palette[1] = [12, 34, 56];
        let texture: Vec<u8> = "x.jpg".encode_utf16().flat_map(u16::to_le_bytes).collect();
        let entries: Vec<Entry> = vec![
            (65001, 1, 3, vec![200, 200, 200]),
            (65019, 1, texture.len() as u32, texture),
            (256, 4, 1, long(side)),
            (257, 4, 1, long(side)),
            (258, 3, 1, short(8)),
            (259, 3, 1, short(1)),
            (262, 3, 1, short(3)),
            (273, 4, side, strips),
            (277, 3, 1, short(1)),
            (278, 3, 1, short(1)),
            (279, 4, side, counts),
            palette_entry(&palette),
        ];
        tiff_file(&[1u8; 4096], &entries)
    }

    #[test]
    fn a_huge_preview_is_bounded() {
        let started = web_time::Instant::now();
        // Above 4096 a side: the tint of 65001, without reading the preview.
        assert_eq!(surface_color(&shared_strips(65536)), Some(([200, 200, 200], true)));
        // 4096 a side, every read succeeding: at most 256 × 256 pixels are read.
        assert_eq!(surface_color(&shared_strips(4096)), Some(([12, 34, 56], true)));
        let elapsed = started.elapsed();
        assert!(elapsed < std::time::Duration::from_secs(2), "took {elapsed:?}");
    }

    #[test]
    fn finds_the_surface_name_in_a_body_tail() {
        assert_eq!(surface_name(&surface_tail("FFC\\ANSTRICH\\FFC-Anstrich-weiss.surf")).as_deref(), Some("FFC\\ANSTRICH\\FFC-Anstrich-weiss.surf"));
        assert_eq!(surface_name(&surface_tail("Glas.SURF")).as_deref(), Some("Glas.SURF"));
        assert_eq!(surface_name(&surface_tail("Material name")), None);
        // Text that is not followed by NUL does not count.
        let mut open = surface_tail("a.surf");
        open.truncate(open.len() - 2);
        open.extend("xy".encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(surface_name(&open), None);
        assert_eq!(surface_path("..\\design.surf"), None);
        assert_eq!(surface_path("C:\\x.surf"), None);
        assert_eq!(surface_path("FFC\\Rot.surf"), Some(vec!["FFC", "Rot.surf"]));
    }

    /// Bodies with every colour source, and a macro body with its own colour number.
    fn colour_stream() -> Vec<u8> {
        let at = |i: f64| prism(0, 0x0301, [1000.0 * i, 0.0, 0.0], [1000.0 * i + 500.0, 200.0, 3000.0]);
        let mut s = Vec::new();
        s.extend(with_tail(at(0.0), &surface_tail("FFC\\AKZENTWAND\\FFC-Rot.surf"), 1));
        s.extend(with_tail(at(1.0), &surface_tail("parkett.surf"), 1));
        s.extend(with_tail(at(2.0), &[0; 16], 21));
        s.extend(with_tail(at(3.0), &surface_tail("Missing.surf"), 99));
        s.extend(with_tail(at(4.0), &surface_tail("Missing.surf"), 1));
        let shelf = with_tail(prism(0, 0x0301, [0.0; 3], [500.0, 500.0, 500.0]), &[0; 8], 21);
        s.extend(definition(3, "Shelf", &[representation(true, &shelf)]));
        s.extend(placement(3, "Shelf", MOVE));
        s
    }

    fn colour_files() -> Memory {
        Memory::new(&[
            ("animation.cfg", cfg(2, 2, &[(1, [255, 255, 255]), (21, [73, 73, 73])])),
            ("design/FFC/AkzentWand/FFC-Rot.surf", surf([255, 0, 0], None, None)),
            ("design/Parkett.surf", surf([200, 200, 200], Some("FFC\\PARKETT\\FFC-Parkett.jpg"), Some(&middle_preview(1)))),
        ])
    }

    #[test]
    fn paints_bodies_from_the_project_folder() {
        let drawing = parse(&file(3, "Colours", &colour_stream())).unwrap();
        let painted = paint(&drawing, &mut colour_files());
        assert!(painted.active);
        assert_eq!(
            painted.bodies,
            [
                Paint::Surface { rgb: [255, 0, 0], textured: false },
                Paint::Surface { rgb: [50, 50, 50], textured: true },
                Paint::Number([73, 73, 73]),
                Paint::Fallback,
                Paint::Number([255, 255, 255]),
            ]
        );
        assert_eq!(painted.macros, [vec![Paint::Number([73, 73, 73])]]);
        let loaded = build(&drawing, Some(&painted)).unwrap();
        for layer in &loaded.layers {
            assert_eq!(layer.colors.as_ref().map(Vec::len), Some(layer.vertex_count()), "{}", layer.name);
        }
        let other = loaded.layers.iter().find(|l| l.name == "Other").unwrap();
        let fallback = Paint::Fallback.rgba(Category::Other.color());
        let colours: Vec<[u8; 4]> = other.elements.iter().map(|&(_, first, _)| other.colors.as_ref().unwrap()[other.indices[first as usize] as usize]).collect();
        assert_eq!(colours, [[255, 0, 0, 255], [50, 50, 50, 255], [73, 73, 73, 255], fallback, [255, 255, 255, 255]]);
        let macros = loaded.layers.iter().find(|l| l.name == "Macros").unwrap();
        assert!(macros.colors.as_ref().unwrap().iter().all(|c| *c == [73, 73, 73, 255]));
        assert_eq!(props_of(&loaded, drawing.bodies[0].offset)["Surface"], "FFC\\AKZENTWAND\\FFC-Rot.surf");
        assert_eq!(props_of(&loaded, drawing.bodies[3].offset)["Surface"], "Missing.surf");
        let info = |key: &str| loaded.project.info.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
        assert_eq!(info("Allplan colours (visible bodies)").as_deref(), Some("surface 2 (textured 1), colour number 2, category colour 1"));
        assert_eq!(info("Allplan project files").as_deref(), Some("animation.cfg: version 2 (2 colours); surfaces found: 2 of 3"));
    }

    #[test]
    fn bytes_without_a_project_folder_keep_category_colours() {
        let source = Source::Bytes { name: "tb000141.ndw".into(), data: Arc::new(file(3, "Colours", &colour_stream())) };
        let model = crate::loaders::load_with(&source, &Progress::default()).unwrap();
        assert!(model.layers.iter().all(|l| l.colors.is_none()));
        assert!(model.project.info.iter().any(|(k, v)| k == "Allplan colours" && v.starts_with("category colours")));
        // An empty project folder keeps them too.
        let drawing = parse(&file(3, "Colours", &colour_stream())).unwrap();
        let painted = paint(&drawing, &mut Memory::new(&[]));
        assert!(!painted.active);
        assert!(build(&drawing, Some(&painted)).unwrap().layers.iter().all(|l| l.colors.is_none()));
    }

    #[test]
    fn finds_project_files_case_insensitively_on_disk() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join(format!("allplan-colours-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("design").join("FFC").join("RASEN")).unwrap();
        std::fs::write(root.join("design").join("FFC").join("RASEN").join("FFC-Rasen-5.surf"), surf([59, 73, 41], None, None)).unwrap();
        std::fs::write(root.join("ANIMATION.CFG"), cfg(3, 4, &[(1, [255, 255, 255])])).unwrap();
        let mut s = with_tail(prism(0, 0x0301, [0.0; 3], [1000.0, 200.0, 3000.0]), &surface_tail("ffc\\rasen\\ffc-rasen-5.surf"), 1);
        s.extend(with_tail(prism(0, 0x0301, [2000.0, 0.0, 0.0], [3000.0, 200.0, 3000.0]), &[0; 8], 1));
        let path = root.join("tb000114.ndw");
        std::fs::write(&path, file(3, "Terrain", &s)).unwrap();
        let model = crate::loaders::load_with(&Source::File(path), &Progress::default());
        let _ = std::fs::remove_dir_all(&root);
        let model = model.unwrap();
        let layer = model.layers.iter().find(|l| l.name == "Other").unwrap();
        let colors = layer.colors.as_ref().unwrap();
        assert_eq!(colors.first(), Some(&[59, 73, 41, 255]));
        assert_eq!(colors.last(), Some(&[255, 255, 255, 255]));
    }

    #[test]
    fn broken_surfaces_and_tables_do_not_panic() {
        let inputs = [
            surf([200, 200, 200], Some("FFC\\PARKETT\\FFC-Parkett.jpg"), Some(&middle_preview(1))),
            cfg(3, 4, &[(1, [255, 255, 255]), (21, [73, 73, 73]), (300, [1, 2, 3])]),
        ];
        let mut seed = 0xD1B5_4A32_D192_ED03u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for input in &inputs {
            for cut in 0..input.len() {
                let _ = surface_color(&input[..cut]);
                let _ = color_table(&input[..cut]);
            }
            for round in 0..500 {
                let mut fuzz = input.clone();
                for _ in 0..(1 + round % 20) {
                    let at = (next() as usize) % fuzz.len();
                    fuzz[at] = next() as u8;
                }
                let _ = surface_color(&fuzz);
                let _ = color_table(&fuzz);
            }
        }
        for _ in 0..200 {
            let tail: Vec<u8> = (0..64).map(|_| next() as u8).collect();
            let _ = surface_name(&tail);
        }
    }

    /// A building structure (§11.2): elements in the tempuri namespace under a prefix, the root
    /// without, a BOM in front.
    fn structure_xml(nodes: &str) -> String {
        format!(
            "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<NemetschekBuildingStructure xmlns:x=\"http://tempuri.org/XMLSchema.xsd\"><x:BuildingStructureNodes>{nodes}</x:BuildingStructureNodes></NemetschekBuildingStructure>"
        )
    }

    /// A node in a `BuildingStructureChildNode` (or, `in_construction`, a `ConstructionChildNode`),
    /// with attributes to skip, its files and its child nodes.
    fn xml_node(kind: &str, name: &str, files: &[u32], children: &str, in_construction: bool) -> String {
        let item = if in_construction { "ConstructionChildNode" } else { "BuildingStructureChildNode" };
        let list = if kind == "Construction" { "ConstructionNodes" } else { "BuildingStructureNodes" };
        let files: String = files.iter().map(|id| format!("<x:File ID=\"{id}\"/>")).collect();
        format!(
            "<x:{item}><x:{kind} Name=\"{name}\"><x:Attributes><x:InternalAttribute><x:Integer>7</x:Integer><x:String>x</x:String></x:InternalAttribute></x:Attributes><x:Files>{files}</x:Files><x:{list}>{children}</x:{list}></x:{kind}></x:{item}>"
        )
    }

    /// A drawing file with one visible box at `x` (mm) and, `auxiliary`, an opening.
    fn small_drawing(name: &str, x: f64, auxiliary: bool) -> Vec<u8> {
        let mut s = prism(0, 0x0301, [x, 0.0, 0.0], [x + 1000.0, 200.0, 3000.0]);
        if auxiliary {
            s.extend(prism(1, 0x0301, [x + 100.0, 0.0, 100.0], [x + 900.0, 200.0, 900.0]));
        }
        file(3, name, &s)
    }

    /// A scratch folder under `target/`, removed on drop.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join(format!("allplan-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn write(&self, path: &str, data: impl AsRef<[u8]>) {
            let path = self.0.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, data).unwrap();
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn reads_the_building_structure_by_local_name() {
        let house = xml_node("Storey", "Ground floor", &[1], "", true) + &xml_node("Storey", "Roof", &[2, 1], "", true);
        let xml = structure_xml(
            &(xml_node("UserDefinedStructureChildNode", "Variant A", &[], &(xml_node("Site", "Site", &[5], "", false) + &xml_node("Construction", "House", &[6], &house, false)), false)
                + &xml_node("UserDefinedStructureChildNode", " Variant B ", &[4], "", false)),
        );
        let tops = read_structure(&xml).unwrap();
        assert_eq!(tops.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["Variant A", "Variant B"]);
        let plan = plan_structure(&tops, &Levels::new());
        let labels: Vec<&str> = plan.groups.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(labels, ["Variant A / Site", "Variant A / House", "Variant A / Ground floor", "Variant A / Roof", "Variant B"]);
        let files: Vec<(u32, usize)> = plan.files.iter().map(|f| (f.0, f.1)).collect();
        assert_eq!(files, [(5, 0), (6, 1), (1, 2), (2, 3), (4, 4)]);
        assert_eq!(plan.files[0].2, "tb000005.ndw");
        assert_eq!(plan.references, 6);
        assert!(read_structure("<not xml").is_none());
    }

    #[test]
    fn opens_a_project_folder() {
        let scratch = Scratch::new("project");
        let root = scratch.0.join("Test.prj");
        let ground = xml_node("Storey", "Ground floor", &[1], "", true)
            + &xml_node("Storey", "Roof", &[2, 1], "", true)
            + &xml_node("Storey", "Cellar", &[3], "", true);
        let xml = structure_xml(
            &(xml_node("UserDefinedStructureChildNode", "Variant A", &[], &xml_node("Construction", "House", &[], &ground, false), false)
                + &xml_node("UserDefinedStructureChildNode", "Variant B ", &[4], "", false)),
        );
        scratch.write("Test.prj/bim/ALLPLAN_BIM_BuildingStructure.xml", xml);
        scratch.write("Test.prj/tb000001.ndw", small_drawing("Ground", 0.0, true));
        scratch.write("Test.prj/TB000002.NDW", small_drawing("Roof", 2000.0, false));
        scratch.write("Test.prj/tb000004.ndw", small_drawing("Variant", 0.0, false));
        scratch.write("Test.prj/tb000009.ndw", small_drawing("Not in the structure", 9000.0, false));
        scratch.write("Test.prj/animation.cfg", cfg(2, 2, &[(1, [255, 255, 255])]));

        let project = read_project(&root).unwrap();
        assert_eq!(project.plan.tops, ["Variant A", "Variant B"]);
        assert_eq!((project.plan.references, project.missing.clone()), (5, vec![3]));
        assert_eq!(project.drawings.iter().map(|d| d.number).collect::<Vec<_>>(), [1, 2, 4]);
        assert_eq!(project.shown_top(), 0);
        assert_eq!(project.top_counts()[0], Counts { files: 2, visible: 2, auxiliary: 1, placements: 0, lines: 0 });

        let model = crate::loaders::load_with(&Source::File(root.clone()), &Progress::default()).unwrap();
        assert_eq!(model.format, "Allplan");
        let layers: Vec<(&str, bool)> = model.layers.iter().map(|l| (l.name.as_str(), l.hidden)).collect();
        assert_eq!(
            layers,
            [("Variant A / Ground floor", false), ("Variant A / Roof", false), ("Variant B", true), ("Variant A / Auxiliary bodies", true)]
        );
        // Every layer has vertex colours: animation.cfg (colour number 0 is not in it) or the category.
        assert!(model.layers.iter().all(|l| l.colors.as_ref().is_some_and(|c| c.len() == l.vertex_count())));
        // Ids are unique over the files: the boxes of files 1 and 4 sit at the same offset.
        let ids: Vec<u64> = model.layers.iter().flat_map(|l| l.elements.iter().map(|e| e.0)).collect();
        let unique: std::collections::HashSet<u64> = ids.iter().copied().collect();
        assert_eq!(unique.len(), ids.len());
        let id = model.layers[0].elements[0].0;
        assert_eq!(id, 1 << 32 | STREAM_START as u64);
        let props: serde_json::Value = serde_json::from_str(&model.props[&id]).unwrap();
        assert_eq!((props["Storey"].as_str(), props["Drawing file"].as_str()), (Some("Ground floor"), Some("000001 Ground")));
        assert_eq!(props["Category"], "Other");
        let info = |key: &str| model.project.info.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()).unwrap_or_default();
        assert_eq!(info("Allplan building structure"), "2 top-level nodes, 5 references to drawing files, 1 missing, 0 unreadable");
        assert!(info("Allplan shown node").starts_with("Variant A"));
        assert_eq!(info("Allplan group Variant A / Ground floor"), "1 drawing files, 1 bodies, 1 auxiliary, 0 macro placements, 0 2D lines at z 0 mm");
    }

    #[test]
    fn a_project_folder_without_structure_opens_all_drawing_files() {
        let scratch = Scratch::new("plain");
        let root = scratch.0.join("Plain.prj");
        scratch.write("Plain.prj/TB000003.NDW", small_drawing("Three", 3000.0, false));
        scratch.write("Plain.prj/tb000001.ndw", small_drawing("One", 0.0, false));
        scratch.write("Plain.prj/notes.txt", "no drawing");
        scratch.write("Plain.prj/BIM/Allplan_BIM_BuildingStructure.xml", "<broken");
        let project = read_project(&root).unwrap();
        assert!(!project.plan.structure);
        assert_eq!(project.plan.tops, ["Plain.prj"]);
        assert_eq!(project.drawings.iter().map(|d| d.number).collect::<Vec<_>>(), [1, 3]);
        let loaded = build_project(&project).unwrap();
        assert_eq!(loaded.layers.iter().map(|l| (l.name.as_str(), l.elements.len())).collect::<Vec<_>>(), [("Plain.prj", 2)]);

        std::fs::create_dir_all(scratch.0.join("Empty.prj")).unwrap();
        let error = read_project(&scratch.0.join("Empty.prj")).err().unwrap();
        assert!(error.contains("no drawing files"), "{error}");
        // Only loaders that open folders take folders.
        std::fs::create_dir_all(scratch.0.join("Model.obj")).unwrap();
        let error = crate::loaders::load_with(&Source::File(scratch.0.join("Model.obj")), &Progress::default()).err().unwrap();
        assert!(error.contains("folders open only"), "{error}");
    }

    /// A 2D line (0x82) or polyline (0x84) record with key `key` through `points` (mm).
    fn line_record(points: &[[f64; 2]], key: Key) -> Vec<u8> {
        let class = if points.len() == 2 { 0x82 } else { 0x84 };
        let mut r = header(class, 0, points.len() as u16, 0);
        for p in points {
            r.extend(p[0].to_le_bytes());
            r.extend(p[1].to_le_bytes());
        }
        keyed(r, key)
    }

    /// The z values of a layer, in metres.
    fn heights(layer: &Layer) -> Vec<f32> {
        let mut z: Vec<f32> = layer.positions.chunks_exact(3).map(|p| p[2]).collect();
        z.sort_by(f32::total_cmp);
        z.dedup();
        z
    }

    #[test]
    fn draws_2d_lines_that_are_not_part_of_a_3d_element() {
        let mut s = keyed(prism(0, 0x0301, [0.0; 3], [1000.0, 200.0, 3000.0]), (5, 1));
        s.extend(line_record(&[[0.0, 0.0], [1000.0, 0.0]], (0, 0)));
        s.extend(line_record(&[[0.0, 1000.0], [1000.0, 1000.0], [1000.0, 2000.0]], (0, 0)));
        // The plan outline of the body: not drawn.
        s.extend(line_record(&[[0.0, 200.0], [1000.0, 200.0]], (5, 1)));
        // A key without a 3D body: drawn.
        s.extend(line_record(&[[0.0, 3000.0], [0.0, 4000.0]], (6, 1)));
        // Not finite, too far out, a line with three points: unreadable.
        s.extend(line_record(&[[f64::NAN, 0.0], [1.0, 0.0]], (0, 0)));
        s.extend(line_record(&[[2e7, 0.0], [1.0, 0.0]], (0, 0)));
        let mut three = line_record(&[[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]], (0, 0));
        three[4] = 0x82;
        s.extend(three);
        // Stale data at the end of the stream (byte 0 & 7 != 0): no record.
        let mut stale = line_record(&[[0.0, 5000.0], [1000.0, 5000.0]], (0, 0));
        stale[0] = 0x7B;
        s.extend(stale);
        let drawing = parse(&file(3, "Plan", &s)).unwrap();
        let classes: Vec<(u8, usize)> = drawing.lines.iter().map(|l| (l.class, l.points.len())).collect();
        assert_eq!(classes, [(0x82, 2), (0x84, 3), (0x82, 2)]);
        assert_eq!((drawing.lines_with_3d, drawing.lines_unreadable), (1, 3));

        let loaded = build(&drawing, None).unwrap();
        let lines = loaded.layers.iter().find(|l| l.name == "2D lines").unwrap();
        assert!(!lines.hidden && lines.elements.is_empty());
        assert_eq!(lines.triangle_count(), 2 * 4);
        assert_eq!(heights(lines), [0.01]);
        assert!(lines.normals.chunks_exact(3).all(|n| n == [0.0, 0.0, 1.0]));
        let info = loaded.project.info.iter().find(|(k, _)| k == "Allplan 2D lines").map(|(_, v)| v.as_str());
        assert_eq!(info, Some("2 lines and 1 polylines (4 segments drawn); 1 part of a 3D element and 3 unreadable, not drawn at z 0 mm"));

        // A drawing file with 2D lines only opens too.
        let only = parse(&file(3, "Plot", &line_record(&[[0.0, 0.0], [5000.0, 0.0]], (0, 0)))).unwrap();
        assert_eq!(build(&only, None).unwrap().layers.len(), 1);
    }

    /// A storey whose attributes point at a level model and its bottom plane (§12.2).
    fn storey_xml(name: &str, files: &[u32], model: &str, bottom: i64) -> String {
        let files: String = files.iter().map(|id| format!("<x:File ID=\"{id}\"/>")).collect();
        format!(
            "<x:BuildingStructureChildNode><x:Storey Name=\"{name}\"><x:Attributes>\
             <x:InternalAttribute Key=\"325adb67-b049-4781-a41a-877eada7d218\"><x:String Value=\"{model}\"/></x:InternalAttribute>\
             <x:InternalAttribute><x:Key>{BOTTOM_PLANE_KEY}</x:Key><x:Integer Value=\"{bottom}\"/></x:InternalAttribute>\
             <x:InternalAttribute Key=\"806EA91C-6428-4af2-8917-E63B343D3372\"><x:Integer Value=\"99\"/></x:InternalAttribute>\
             </x:Attributes><x:Files>{files}</x:Files></x:Storey></x:BuildingStructureChildNode>"
        )
    }

    const LEVEL_MODEL: &str = "{8A3B7E1C-0000-4000-8000-00000000ABCD}";

    fn level_model_xml() -> String {
        let plane = |kind: &str, number: u32, height: f64| {
            format!("<{kind} ObjectGuid=\"{number:08X}-0000-0000-0000-000000000000\"><HeightLevel>{height}</HeightLevel></{kind}>")
        };
        format!(
            "<?xml version=\"1.0\"?><Levelmodel ObjectGuid=\"{}\"><Building>{}{}</Building><PlanePairs><Planepair Name=\"Ground floor\">{}{}</Planepair></PlanePairs></Levelmodel>",
            LEVEL_MODEL.to_lowercase(),
            plane("BottomPlane", 5, -150.0),
            plane("TopPlane", 6, 2650.0),
            plane("BottomPlane", 10, 2800.0),
            plane("TopPlane", 11, 5600.0)
        )
    }

    #[test]
    fn draws_2d_lines_on_their_storey() {
        let scratch = Scratch::new("lines");
        let root = scratch.0.join("Lines.prj");
        let xml = structure_xml(&xml_node(
            "UserDefinedStructureChildNode",
            "House",
            &[],
            &(xml_node("Site", "Site", &[3], "", false) + &storey_xml("Ground floor", &[1], LEVEL_MODEL, 5) + &storey_xml("Upper floor", &[2], LEVEL_MODEL, 10)),
            false,
        ));
        scratch.write("Lines.prj/BIM/Allplan_BIM_BuildingStructure.xml", xml);
        scratch.write("Lines.prj/BIM/Allplan_BIM_LevMo_House.xml", level_model_xml());
        let plan = |key: Key| {
            let mut s = prism(0, 0x0301, [0.0; 3], [1000.0, 200.0, 3000.0]);
            s.extend(line_record(&[[0.0, 0.0], [4000.0, 0.0]], key));
            s
        };
        scratch.write("Lines.prj/tb000001.ndw", file(3, "Ground floor plan", &plan((0, 0))));
        scratch.write("Lines.prj/tb000002.ndw", file(3, "Upper floor plan", &plan((7, 1))));
        scratch.write("Lines.prj/tb000003.ndw", file(3, "Site plan", &line_record(&[[0.0, 0.0], [9000.0, 9000.0]], (0, 0))));

        let project = read_project(&root).unwrap();
        let z: Vec<(&str, f64)> = project.plan.groups.iter().map(|g| (g.label.as_str(), g.z)).collect();
        assert_eq!(z, [("House / Site", 0.0), ("House / Ground floor", -150.0), ("House / Upper floor", 2800.0)]);
        let loaded = build_project(&project).unwrap();
        let layer = |name: &str| loaded.layers.iter().find(|l| l.name == name).unwrap_or_else(|| panic!("no layer {name}"));
        assert_eq!(heights(layer("House / Site / 2D lines")), [0.01]);
        assert_eq!(heights(layer("House / Ground floor / 2D lines")), [-0.14]);
        assert_eq!(heights(layer("House / Upper floor / 2D lines")), [2.81]);
        assert!(layer("House / Ground floor / 2D lines").colors.as_ref().unwrap().iter().all(|c| *c == Paint::Fallback.rgba(LINE_COLOR)));

        // A drawing file of the project opened on its own lies on the same storey; as bytes at 0.
        assert_eq!(file_height(&root.join("tb000002.ndw")), Some(2800.0));
        let alone = crate::loaders::load_with(&Source::File(root.join("tb000002.ndw")), &Progress::default()).unwrap();
        let lines = alone.layers.iter().find(|l| l.name == "2D lines").unwrap();
        assert_eq!(heights(lines), [2.81]);
        let data = std::fs::read(root.join("tb000002.ndw")).unwrap();
        let bytes = crate::loaders::load_with(&Source::Bytes { name: "tb000002.ndw".into(), data: Arc::new(data) }, &Progress::default()).unwrap();
        assert_eq!(heights(bytes.layers.iter().find(|l| l.name == "2D lines").unwrap()), [0.01]);
        assert_eq!(file_height(&scratch.0.join("tb000002.ndw")), None);
    }

    /// Checks a real drawing file against the tables of spec §7.2 to §7.8, when `ALLPLAN_SAMPLE`
    /// holds its path. The samples are not in the repository.
    #[test]
    fn optional_sample_from_env() {
        let Ok(path) = std::env::var("ALLPLAN_SAMPLE") else { return };
        let data = std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
        let file_name = std::path::Path::new(&path).file_name().unwrap().to_string_lossy().to_lowercase();
        let drawing = parse(&data).expect("Allplan sample");
        let visible = drawing.bodies.iter().filter(|b| !b.auxiliary).count();
        let auxiliary = drawing.bodies.len() - visible;
        let explicit = drawing.bodies.iter().filter(|b| b.kind == 4).count();
        let prisms = drawing.bodies.len() - explicit;
        let linked = drawing.bodies.iter().filter(|b| b.element.is_some()).count();
        let mut codes = std::collections::BTreeMap::new();
        for body in &drawing.bodies {
            if let Some((group, subtype)) = body.element.and_then(|e| drawing.elements[e].type_code) {
                *codes.entry(format!("{group:02X} {subtype:02X}")).or_insert(0usize) += 1;
            }
        }
        // §7.2/§7.4 boxes: the drawing's own visible bodies, without macro bodies (v0.3).
        let solid_bounds = build(&drawing, None).ok().map(|loaded| {
            let positions: Vec<f32> = loaded
                .layers
                .iter()
                .filter(|l| !l.hidden && l.name != Category::Macros.name())
                .flat_map(|l| l.positions.iter().copied())
                .collect();
            bounds(&positions)
        });
        eprintln!(
            "{file_name}: \"{}\", {} pages, {visible} bodies, {auxiliary} auxiliary, kind 1/0 {prisms}, kind 4 {explicit}, \
             linked {linked}, type codes {codes:?}, skipped clip {} / no faces {} / unreadable {}, bounds {solid_bounds:?}",
            drawing.name, drawing.pages, drawing.skipped_clip, drawing.skipped_no_faces, drawing.skipped_unreadable
        );
        // §7.2 and §7.4
        type Range = [(f32, f32); 3];
        let table: [(&str, usize, &str, usize, usize, usize, usize, Option<Range>); 8] = [
            ("tb000141", 26, "1st basement floor plan", 91, 38, 96, 33, Some([(3880.0, 15855.0), (-20150.0, -5380.0), (-3169.5, 1074.7)])),
            ("tb000151", 64, "GF floor plan", 93, 50, 106, 37, Some([(4000.0, 15855.0), (-20150.0, -5500.0), (-650.0, 4024.7)])),
            ("tb000110", 2, "QUICKSTART 01.1", 0, 0, 0, 0, None),
            ("tb000732", 3, "Floor slab", 1, 0, 1, 0, Some([(3950.0, 15908.5), (-20200.0, -5450.0), (-3400.0, -3100.0)])),
            // §7.4 (v0.2.1)
            ("tb000452", 110, "GF additions (3D furniture)", 1, 0, 1, 0, Some([(8000.0, 15209.9), (-20150.0, -15345.0), (-650.0, 2600.0)])),
            ("tb000114", 25, "Outdoor facilities, terrain", 102, 4, 103, 3, Some([(0.0, 47374.6), (-38491.7, 0.0), (-3650.0, 1000.0)])),
            ("tb003080", 4, "1st basement - sections", 0, 12, 12, 0, None),
            ("tb001501", 12, "Section A calculation result", 0, 0, 0, 0, None),
        ];
        if let Some(&(_, pages, name, want_visible, want_auxiliary, want_prisms, want_explicit, range)) =
            table.iter().find(|row| file_name.starts_with(row.0))
        {
            assert_eq!(drawing.pages, pages);
            assert_eq!(drawing.name, name);
            assert_eq!((visible, auxiliary), (want_visible, want_auxiliary));
            assert_eq!((prisms, explicit), (want_prisms, want_explicit));
            if let Some(range) = range {
                let (lo, hi) = solid_bounds.expect("bodies");
                for k in 0..3 {
                    let (min, max) = (range[k].0 / 1000.0, range[k].1 / 1000.0);
                    assert!((lo[k] - min).abs() < 6e-4 && (hi[k] - max).abs() < 6e-4, "axis {k}: {lo:?} {hi:?}");
                }
            }
        }
        // §7.3: shown bodies (auxiliary included), linked bodies and bodies per type code.
        type Codes = &'static [(&'static str, usize)];
        let table: [(&str, Option<&str>, usize, usize, Codes); 4] = [
            ("tb000141", None, 129, 129, &[("01 01", 36), ("01 03", 1), ("01 07", 1), ("01 0E", 3), ("01 3E", 1), ("01 47", 48), ("09 3D", 13), ("09 5F", 1), ("59 16", 7), ("59 17", 12), ("59 2B", 6)]),
            ("tb000151", None, 143, 140, &[("01 01", 35), ("01 03", 1), ("01 0E", 2), ("01 3E", 1), ("01 47", 50), ("09 01", 3), ("09 3D", 10), ("09 5F", 1), ("59 16", 11), ("59 17", 13), ("59 18", 1), ("59 1B", 1), ("59 2A", 2), ("59 2B", 9)]),
            ("tb000732", None, 1, 1, &[("01 07", 1)]),
            ("tb000624", Some("Chimney"), 1, 1, &[("01 0A", 1)]),
        ];
        if let Some(&(_, name, shown, want_linked, want_codes)) = table.iter().find(|row| file_name.starts_with(row.0)) {
            if let Some(name) = name {
                assert_eq!(drawing.name, name);
            }
            assert_eq!((drawing.bodies.len(), linked), (shown, want_linked));
            let want: std::collections::BTreeMap<String, usize> = want_codes.iter().map(|&(c, n)| (c.to_string(), n)).collect();
            assert_eq!(codes, want);
        }
        if file_name.starts_with("tb000141") {
            for (name, want) in [("WA-RFC", [2.75, 0.24, 9.0, 2.16, 5.94]), ("Insulation not analyzed", [2.95, 0.08, 4.0, 0.32, 0.944])] {
                let found = drawing.bodies.iter().filter_map(|b| b.element.map(|e| &drawing.elements[e])).any(|e| {
                    e.name == name
                        && e.type_code == Some((0x01, 0x01))
                        && e.quantities.iter().zip(want).all(|(a, b)| (a - b).abs() <= 1e-6)
                });
                assert!(found, "no wall {name:?} with quantities {want:?}");
            }
        }
        // §7.5: macro placements, the bodies they place (auxiliary included) and their world box.
        let count = |want: fn(&Placed) -> bool| drawing.placements.iter().filter(|p| want(&p.placed)).count();
        let mut bodies = 0;
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        let mut names: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for placement in &drawing.placements {
            let definition = match placement.placed {
                Placed::Resolved(i) => Some(&drawing.macros[i]),
                _ => None,
            };
            let name = if placement.name.is_empty() { definition.map(|d| d.name.clone()).unwrap_or_default() } else { placement.name.clone() };
            *names.entry(name).or_default() += 1;
            for body in definition.iter().flat_map(|d| &d.bodies) {
                bodies += 1;
                for p in place(body, &placement.matrix).0 {
                    for k in 0..3 {
                        lo[k] = lo[k].min(p[k]);
                        hi[k] = hi[k].max(p[k]);
                    }
                }
            }
        }
        let (resolved, without, unknown) = (
            count(|p| matches!(p, Placed::Resolved(_))),
            count(|p| *p == Placed::Without3d),
            count(|p| *p == Placed::UnknownMacro),
        );
        eprintln!(
            "{file_name}: {} placements, resolved {resolved}, without 3D {without}, unknown {unknown}, {bodies} macro bodies, box {lo:?}..{hi:?}",
            drawing.placements.len()
        );
        type Box3 = [(f64, f64); 3];
        let table: [(&str, usize, usize, usize, usize, usize, Box3, &[(&str, usize)]); 3] = [
            ("tb000152", 25, 25, 0, 0, 3268, [(5176.5, 15513.5), (-15170.0, -5820.0), (-5.0, 2450.0)], &[("Hall cupboard", 4), ("Kitchen chair", 3)]),
            ("tb000162", 105, 104, 1, 0, 104, [(10290.0, 15830.0), (-21720.0, -20080.0), (2600.0, 3760.0)], &[]),
            ("tb000141", 13, 13, 0, 0, 463, [(4445.0, 15735.6), (-20070.0, -5580.0), (-3100.0, -350.0)], &[("ID-WD", 4)]),
        ];
        if let Some(&(_, want_placements, want_resolved, want_without, want_unknown, want_bodies, range, want_names)) =
            table.iter().find(|row| file_name.starts_with(row.0))
        {
            assert_eq!(
                (drawing.placements.len(), resolved, without, unknown, bodies),
                (want_placements, want_resolved, want_without, want_unknown, want_bodies)
            );
            for k in 0..3 {
                assert!((lo[k] - range[k].0).abs() <= 0.5 && (hi[k] - range[k].1).abs() <= 0.5, "axis {k}: {lo:?} {hi:?}");
            }
            for &(name, want) in want_names {
                assert_eq!(names.get(name).copied().unwrap_or(0), want, "placements named {name:?}");
            }
        }
        // §7.8: the 2D lines and polylines drawn, their segments and xy box; on its own in its project
        // folder tb000141 lies on its storey.
        let lines = drawing.lines.iter().filter(|l| l.class == 0x82).count();
        let polylines = drawing.lines.len() - lines;
        let segments: usize = drawing.lines.iter().map(|l| l.points.len() - 1).sum();
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        for p in drawing.lines.iter().flat_map(|l| &l.points) {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let height = file_height(std::path::Path::new(&path));
        eprintln!("{file_name}: {lines} lines, {polylines} polylines, {segments} segments, xy {lo:?}..{hi:?}, storey height {height:?}");
        type Box2 = [(f64, f64); 2];
        let table: [(&str, usize, usize, usize, Option<Box2>); 6] = [
            ("tb000141", 239, 2, 243, Some([(4000.0, 15815.5), (-20150.0, -5500.0)])),
            ("tb000151", 315, 28, 454, Some([(4000.0, 15849.9), (-20150.0, -5500.0)])),
            ("tb000113", 607, 0, 607, Some([(0.0, 20965.8), (-38491.7, 0.0)])),
            ("tb000121", 8, 0, 8, Some([(4080.0, 15769.7), (-20070.0, -5580.0)])),
            ("tb000110", 0, 0, 0, None),
            ("tb000152", 0, 0, 0, None),
        ];
        if let Some(&(_, want_lines, want_polylines, want_segments, range)) = table.iter().find(|row| file_name.starts_with(row.0)) {
            assert_eq!((lines, polylines, segments), (want_lines, want_polylines, want_segments));
            if let Some(range) = range {
                for k in 0..2 {
                    assert!((lo[k] - range[k].0).abs() <= 0.5 && (hi[k] - range[k].1).abs() <= 0.5, "axis {k}: {lo:?} {hi:?}");
                }
            }
        }
        if file_name.starts_with("tb000141") {
            if let Some(height) = height {
                assert_eq!(height, -3100.0);
            }
        }
        // §7.6: colours of the drawing's own visible bodies, from the project folder of the sample.
        let folder = std::path::Path::new(&path).parent().map(std::path::Path::to_path_buf).unwrap_or_default();
        let painted = paint(&drawing, &mut Folder::new(&folder));
        let visible: Vec<(&Body, Paint)> =
            drawing.bodies.iter().zip(painted.bodies.iter().copied()).filter(|(b, _)| !b.auxiliary).collect();
        let (mut textured, mut plain, mut number, mut fallback) = (0, 0, 0, 0);
        for (_, paint) in &visible {
            match paint {
                Paint::Surface { textured: true, .. } => textured += 1,
                Paint::Surface { .. } => plain += 1,
                Paint::Number(_) => number += 1,
                Paint::Fallback => fallback += 1,
            }
        }
        eprintln!("{file_name}: colours of {} visible bodies: surface {textured} textured / {plain}, number {number}, fallback {fallback}", visible.len());
        let table: [(&str, usize, usize, usize, usize, usize); 3] =
            [("tb000141", 91, 20, 2, 69, 0), ("tb000151", 93, 18, 26, 49, 0), ("tb000114", 102, 55, 0, 47, 0)];
        if let Some(&(_, want_visible, want_textured, want_plain, want_number, want_fallback)) =
            table.iter().find(|row| file_name.starts_with(row.0))
        {
            assert_eq!(
                (visible.len(), textured, plain, number, fallback),
                (want_visible, want_textured, want_plain, want_number, want_fallback)
            );
        }
        // (colour number or surface name, bodies, RGB)
        enum By {
            Number(u16),
            Surface(&'static str),
        }
        let colours: &[(By, usize, [u8; 3])] = if file_name.starts_with("tb000141") {
            &[
                (By::Number(1), 51, [255, 255, 255]),
                (By::Surface("QS-Material-01.surf"), 16, [94, 61, 38]),
                (By::Number(21), 9, [73, 73, 73]),
                (By::Surface("FFC\\AKZENTWAND\\FFC-Rot.surf"), 1, [255, 0, 0]),
                (By::Surface("Glas.surf"), 1, [239, 241, 245]),
            ]
        } else if file_name.starts_with("tb000114") {
            &[(By::Surface("ffc\\rasen\\ffc-rasen-5.surf"), 7, [59, 73, 41])]
        } else {
            &[]
        };
        for (by, want, rgb) in colours {
            let picked: Vec<[u8; 3]> = visible
                .iter()
                .filter_map(|(body, paint)| match (by, paint) {
                    (By::Number(n), Paint::Number(c)) if body.color_number == *n => Some(*c),
                    (By::Surface(name), Paint::Surface { rgb, .. })
                        if body.surface.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(name)) =>
                    {
                        Some(*rgb)
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(picked.len(), *want, "bodies for {rgb:?}");
            for c in picked {
                assert!(c.iter().zip(rgb).all(|(a, b)| a.abs_diff(*b) <= 1), "{c:?} instead of {rgb:?}");
            }
        }
    }

    /// Checks a whole project folder against spec §7.7, when `ALLPLAN_PROJECT` holds the path of
    /// `QuickStart.prj`. The samples are not in the repository.
    #[test]
    fn optional_project_from_env() {
        let Ok(path) = std::env::var("ALLPLAN_PROJECT") else { return };
        let dir = std::path::Path::new(&path);
        let started = web_time::Instant::now();
        let project = read_project(dir).expect("Allplan project");
        let loaded = build_project(&project).expect("Allplan project layers");
        let elapsed = started.elapsed();
        let tops = project.top_counts();
        let shown = project.shown_top();
        eprintln!(
            "{path}: {} top-level nodes, {} references, {} missing, {} unreadable, {} files read in {elapsed:?}; shown {:?}",
            project.plan.tops.len(),
            project.plan.references,
            project.missing.len(),
            project.unreadable.len(),
            project.drawings.len(),
            project.plan.tops.get(shown)
        );
        for (top, counts) in project.plan.tops.iter().zip(&tops) {
            eprintln!("  {top:?}: {counts:?}");
        }
        for (group, counts) in project.plan.groups.iter().zip(project.group_counts()) {
            eprintln!("  {:?}: {counts:?}", group.label);
        }
        if !dir.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("QuickStart.prj")) {
            return;
        }
        let names = &project.plan.tops;
        assert_eq!(names.len(), 11);
        assert_eq!(names[0], "START");
        for i in 1..=10 {
            assert_eq!(names[i], format!("Unit {i:02}"));
        }
        assert_eq!((project.plan.references, project.missing.len(), project.unreadable.len()), (316, 0, 0));
        assert_eq!(names[shown], "Unit 01");
        assert_eq!(tops[shown], Counts { files: 31, visible: 431, auxiliary: 172, placements: 376, lines: 2256 });
        let groups = project.group_counts();
        for (label, want) in [
            ("Unit 01 / Site", 102),
            ("Unit 01 / Detached house", 1),
            ("Unit 01 / Foundations", 1),
            ("Unit 01 / 1st basement", 95),
            ("Unit 01 / Ground floor", 106),
            ("Unit 01 / 1st upper floor", 96),
            ("Unit 01 / 2nd upper floor", 30),
        ] {
            let index = project.plan.groups.iter().position(|g| g.label == label).unwrap_or_else(|| panic!("no group {label:?}"));
            assert_eq!(groups[index].visible, want, "{label}");
        }
        let total = tops.iter().fold(Counts::default(), |mut a, c| {
            a.add(*c);
            a
        });
        assert_eq!((total.visible, total.auxiliary, total.placements), (4276, 1771, 3801));
        // §7.8: 2D elements drawn and storey height per group of "Unit 01".
        for (label, want_lines, want_z) in [
            ("Unit 01 / Site", 981, 0.0),
            ("Unit 01 / Detached house", 8, 0.0),
            ("Unit 01 / 1st basement", 243, -3100.0),
            ("Unit 01 / Ground floor", 427, -150.0),
            ("Unit 01 / 1st upper floor", 425, 2800.0),
            ("Unit 01 / 2nd upper floor", 172, 5750.0),
        ] {
            let index = project.plan.groups.iter().position(|g| g.label == label).unwrap_or_else(|| panic!("no group {label:?}"));
            assert_eq!((groups[index].lines, project.plan.groups[index].z), (want_lines, want_z), "{label}");
        }
        let foundations = project.plan.groups.iter().position(|g| g.label == "Unit 01 / Foundations").unwrap();
        assert_eq!(groups[foundations].lines, 0);
        // Only the layers of "Unit 01" start visible.
        for layer in &loaded.layers {
            assert_eq!(layer.hidden, !layer.name.starts_with("Unit 01 /") || layer.name.ends_with("Auxiliary bodies"), "{}", layer.name);
        }
    }
}
