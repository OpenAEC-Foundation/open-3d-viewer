//! Allplan drawing files (`.ndw`, German "Teilbild"): the 3D bodies of one drawing file.
//!
//! Written from the clean-room specification "Allplan-tekenbestand (.ndw) v0.1" only. A drawing
//! file is a 12-byte header and pages of 32760 bytes; the element stream runs from the third page
//! to the end of the last page and holds padding, attribute blocks and element records (§2, §3).
//! Records of class 0xC6 are 3D bodies (§5): a prism (a bottom and a top ring) or an explicit
//! boundary (edges and faces with holes). Coordinates are world coordinates in millimetres, Z up;
//! the viewer shows them in metres without moving them, so drawing files of one project line up.
//! Auxiliary bodies (openings, rooms) go in a layer that starts hidden.
//!
//! Not read yet: macros (3D furniture), colours and materials, 2D elements, project folders
//! and layouts (`.npl`).

use super::{Loaded, Loader, Phase, Progress, Source, DEFAULT_COLOR};
use crate::model::{Layer, ProjectData};

pub struct AllplanLoader;

impl Loader for AllplanLoader {
    fn name(&self) -> &'static str { "Allplan" }
    fn extensions(&self) -> &'static [&'static str] { &["ndw"] }
    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }
    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        progress.set(Phase::Convert);
        let drawing = parse(&data)?;
        build(&drawing)
    }
}

/// Page size of a drawing file (§2).
const PAGE: usize = 0x7FF8;
/// Offset of the drawing file name: page 1 plus a u32 (§2).
const NAME_OFFSET: usize = 12 + PAGE + 4;
/// Length of the name in UTF-16 units.
const NAME_UNITS: usize = 70;
/// Element record header (§3).
const HEADER: usize = 40;
/// Millimetres to the viewer's metres.
const MM: f64 = 0.001;
/// Form of clip and view volumes, not a building part (§5.5).
const FORM_CLIP: u16 = 0x0701;
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
    skipped_clip: usize,
    skipped_no_faces: usize,
    skipped_unreadable: usize,
}

/// A 3D body as triangles: world coordinates in millimetres.
#[derive(Debug)]
struct Body {
    /// Offset of the record in the file: the element id.
    offset: usize,
    record_type: u16,
    form: u16,
    kind: u8,
    auxiliary: bool,
    positions: Vec<[f64; 3]>,
    normals: Vec<[f32; 3]>,
    indices: Vec<u32>,
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
    let mut p = 12 + 2 * PAGE;
    while p + 8 <= stream.len() {
        if is_padding(stream, p) {
            p += 8;
        } else if is_attribute(stream, p) {
            p += 8 + (2 * usize::from(stream[p + 2])).div_ceil(8) * 8;
        } else if let Some(step) = record_step(stream, p) {
            if stream[p + 4] == 0xC6 {
                drawing.body(&stream[p..p + step], p);
            }
            p += step;
        } else {
            // Resynchronise on the next strictly valid header, or stop.
            match (p + 8..).step_by(8).take_while(|q| q + HEADER <= stream.len()).find(|&q| strict_resync(stream, q)) {
                Some(q) => p = q,
                None => break,
            }
        }
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
    if p + HEADER > s.len() || s[p + 7] & 0xF0 != 0x50 {
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

/// A strictly valid header that is followed (after padding) by the stream end, an attribute
/// block or another strictly valid header.
fn strict_resync(s: &[u8], q: usize) -> bool {
    let Some(step) = strict_header(s, q) else { return false };
    let mut next = q + step;
    while next + 8 <= s.len() && is_padding(s, next) {
        next += 8;
    }
    next + 8 > s.len() || is_attribute(s, next) || strict_header(s, next).is_some()
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
    let count = usize::try_from(v.unsigned_abs()).ok()?;
    let stored = if kind == 4 { count.checked_add(1)? } else { count };
    let vertices_at = h + 32;
    let bbox_at = vertices_at.checked_add(stored.checked_mul(24)?)?;
    let topology_at = bbox_at.checked_add(48)?;
    if topology_at > r.len() {
        return None;
    }
    let mut points = Vec::with_capacity(stored);
    for i in 0..stored {
        let o = vertices_at + 24 * i;
        let (x, y, z) = (f64_at(r, o)?, f64_at(r, o + 8)?, f64_at(r, o + 16)?);
        let world = [
            m[0] * x + m[1] * y + m[2] * z + m[9],
            m[3] * x + m[4] * y + m[5] * z + m[10],
            m[6] * x + m[7] * y + m[8] * z + m[11],
        ];
        points.push(world);
    }
    // Slot 0 of an explicit body is unused (1-based indices).
    let used = if kind == 4 { 1 } else { 0 };
    if points[used..].iter().flatten().any(|c| !c.is_finite()) {
        return None;
    }
    let faces = if kind == 4 { explicit_faces(r, topology_at, count, e, f, l)? } else { prism_faces(v, count)? };
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
    let flip = volume < 0.0;
    let mut body = Body {
        offset,
        record_type: u16_at(r, 4)?,
        form,
        kind,
        auxiliary: r[1] & 1 == 1,
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
/// bounding box; vertex indices run 1..=`count`.
fn explicit_faces(r: &[u8], at: usize, count: usize, e: usize, f: usize, l: usize) -> Option<Vec<Face>> {
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
        edges.push((a, b));
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

/// The layers: the bodies, and the auxiliary bodies in a hidden layer.
fn build(drawing: &Drawing) -> Result<Loaded, String> {
    let mut solid = Layer { name: "3D bodies".into(), color: DEFAULT_COLOR, ..Layer::default() };
    let mut auxiliary = Layer {
        name: "Auxiliary bodies (openings, rooms)".into(),
        color: AUXILIARY_COLOR,
        hidden: true,
        ..Layer::default()
    };
    for body in &drawing.bodies {
        let layer = if body.auxiliary { &mut auxiliary } else { &mut solid };
        let base = u32::try_from(layer.vertex_count()).map_err(|_| "Allplan: too many vertices")?;
        let first = u32::try_from(layer.indices.len()).map_err(|_| "Allplan: too many triangles")?;
        layer.positions.extend(body.positions.iter().flat_map(|p| p.map(|c| (c * MM) as f32)));
        layer.normals.extend(body.normals.iter().flatten());
        for &i in &body.indices {
            layer.indices.push(base.checked_add(i).ok_or("Allplan: too many vertices")?);
        }
        let count = u32::try_from(body.indices.len()).map_err(|_| "Allplan: too many triangles")?;
        let id = body.offset as u64;
        layer.elements.push((id, first, count));
        layer.props.push((
            id,
            serde_json::json!({
                "Allplan drawing file": drawing.name,
                "Record offset": format!("0x{:X}", body.offset),
                "Record class": format!("0x{:02X}", body.record_type & 0xFF),
                "Record type": format!("0x{:04X}", body.record_type),
                "Form": format!("0x{:04X}", body.form),
                "Kind": match body.kind { 4 => "explicit topology (4)".to_string(), k => format!("prism ({k})") },
                "Auxiliary": body.auxiliary,
            })
            .to_string(),
        ));
    }
    let visible = drawing.bodies.iter().filter(|b| !b.auxiliary).count();
    let hidden = drawing.bodies.len() - visible;
    if drawing.bodies.is_empty() {
        let name = if drawing.name.is_empty() { String::new() } else { format!(" \"{}\"", drawing.name) };
        return Err(format!(
            "Allplan: the drawing file{name} has no 3D bodies. This reader shows 3D bodies only; 2D elements, \
             macros (3D furniture) and layouts are not read yet."
        ));
    }
    let mut project = ProjectData::default();
    project.info = vec![
        ("Allplan drawing file".into(), drawing.name.clone()),
        ("Allplan pages".into(), drawing.pages.to_string()),
        ("Allplan 3D bodies".into(), visible.to_string()),
        ("Allplan auxiliary bodies (hidden)".into(), hidden.to_string()),
        (
            "Allplan skipped bodies".into(),
            format!(
                "clip volumes: {}, without faces: {}, unreadable: {}",
                drawing.skipped_clip, drawing.skipped_no_faces, drawing.skipped_unreadable
            ),
        ),
        (
            "Allplan import scope".into(),
            "3D bodies (spec v0.1); no macros, colours, materials or 2D elements".into(),
        ),
    ];
    Ok(Loaded { layers: vec![solid, auxiliary], views: Vec::new(), project })
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
        let l = if s.kind == 4 { (s.topology.len() as u32 - 8 * s.e - 8 * s.f) / 8 } else { 4 * s.f.saturating_sub(2) + 2 * s.vertices.len() as u32 / 2 };
        b.extend(l.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend([0x00, 0x00, 0x00, 0x20, 0x00, 0x20, 0x20, 0x38]);
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
        let error = build(&drawing).err().unwrap();
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
            let _ = parse(&fuzz).map(|d| build(&d));
        }
        // A header whose page count points far beyond the end of the file.
        let mut long = data[..12 + 2 * PAGE + 200].to_vec();
        long[8..10].copy_from_slice(&0xFFFEu16.to_le_bytes());
        let _ = parse(&long);
    }

    /// Checks a real drawing file against the table of spec §7.2, when `ALLPLAN_SAMPLE` holds its
    /// path. The samples are not in the repository.
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
        let solid_bounds = build(&drawing).ok().map(|loaded| bounds(&loaded.layers[0].positions));
        eprintln!(
            "{file_name}: \"{}\", {} pages, {visible} bodies, {auxiliary} auxiliary, kind 1/0 {prisms}, kind 4 {explicit}, \
             skipped clip {} / no faces {} / unreadable {}, bounds {solid_bounds:?}",
            drawing.name, drawing.pages, drawing.skipped_clip, drawing.skipped_no_faces, drawing.skipped_unreadable
        );
        type Range = [(f32, f32); 3];
        let table: [(&str, usize, &str, usize, usize, usize, usize, Option<Range>); 4] = [
            ("tb000141", 26, "1st basement floor plan", 91, 38, 96, 33, Some([(3880.0, 15855.0), (-20150.0, -5380.0), (-3169.5, 1074.7)])),
            ("tb000151", 64, "GF floor plan", 93, 50, 106, 37, Some([(4000.0, 15855.0), (-20150.0, -5500.0), (-650.0, 4024.7)])),
            ("tb000110", 2, "QUICKSTART 01.1", 0, 0, 0, 0, None),
            ("tb000732", 3, "Floor slab", 1, 0, 1, 0, Some([(3950.0, 15908.5), (-20200.0, -5450.0), (-3400.0, -3100.0)])),
        ];
        let Some(&(_, pages, name, want_visible, want_auxiliary, want_prisms, want_explicit, range)) =
            table.iter().find(|row| file_name.starts_with(row.0))
        else {
            return;
        };
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
}
