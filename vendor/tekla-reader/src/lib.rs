//! Tekla Structures model database (.db1), a clean-room reader written only from
//! `tekla-cleanroom/spec/SPEC_v0.1.md` (draft; open questions in `spec/QUESTIONS.md`). Header
//! versions 8.95 (TS 2020) and 7.82 (TS 19.0).
//!
//! Profile parts become a box along their axis in the part's local frame, aligned in y/z as the
//! spec says; contour plates (7.82) become their polygon extruded by the thickness. One layer
//! per class; every part is a pickable element with name, profile, material and class.
//!
//! APPROXIMATIONS while the spec leaves them open (§6): the box size is guessed from the profile
//! name (see [`profile_size`]), and offsets, end offsets and rotation are ignored.
//!
//! Tekla is Z up in mm; the layers are Z up in metres.

use openaec_model::{Loaded, Loader, Phase, Progress, Source};
use openaec_model::model::{Layer, ProjectData};
use glam::DVec3;
use std::collections::{BTreeMap, HashMap};
use std::io::Read;

pub struct TeklaLoader;

impl Loader for TeklaLoader {
    fn name(&self) -> &'static str {
        "Tekla Structures"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["db1"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|l| l.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let file = source.read()?;
        let data = gunzip(&file)?;
        let db = Db::parse(&data)?;
        progress.set(Phase::Convert);
        Ok(db.loaded())
    }
}

const MM: f64 = 0.001;

/// §1: the file is one gzip stream.
fn gunzip(file: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(file).read_to_end(&mut out).map_err(|e| format!("Tekla: not a gzip stream ({e})"))?;
    Ok(out)
}

/// §2: header bytes before the version text.
const MAGIC: [u8; 8] = [0x58, 0x73, 0x74, 0x65, 0x65, 0x6C, 0x85, 0x20];
/// §2–3: marker 0xDBCEC066 of a table, little-endian.
const TABLE_MARKER: [u8; 4] = [0x66, 0xC0, 0xCE, 0xDB];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Version {
    /// "8.95", Tekla Structures 2020.
    V895,
    /// "7.82", Tekla Structures 19.0.
    V782,
}

/// Table positions (counted from 0 over the 0x66 blocks) and their record sizes, §4–5.
const POINT_TABLE: (usize, usize) = (61, 32);
const FRAME_TABLE: (usize, usize) = (65, 52);
const STRING_TABLE: (usize, usize) = (75, 45);
/// 8.95: the position blocks of the parts.
const POSITION_TABLE: (usize, usize) = (245, 52);
/// 7.82: the shapes of contour plates.
const SHAPE_TABLE: (usize, usize) = (121, 332);

/// What differs between the versions: table positions, record sizes and field offsets.
struct Layout {
    part_table: (usize, usize),
    attr_table: (usize, usize),
    /// Part fields: attribute set, start point, end point, local frame.
    part_attr: usize,
    part_start: usize,
    part_end: usize,
    part_frame: usize,
    /// Added to the 8.95 offsets of the attribute set fields (§4).
    attr_shift: usize,
}

impl Version {
    fn text(self) -> &'static str {
        match self {
            Version::V895 => "8.95",
            Version::V782 => "7.82",
        }
    }

    fn layout(self) -> Layout {
        match self {
            Version::V895 => Layout { part_table: (274, 64), attr_table: (299, 332), part_attr: 0, part_start: 8, part_end: 12, part_frame: 20, attr_shift: 0 },
            Version::V782 => Layout { part_table: (193, 56), attr_table: (207, 380), part_attr: 0, part_start: 4, part_end: 8, part_frame: 16, attr_shift: 48 },
        }
    }
}

/// One table: its record size and where each row starts (the flags byte).
struct Table {
    size: usize,
    rows: Vec<usize>,
}

/// The decompressed database with its tables.
struct Db<'a> {
    data: &'a [u8],
    version: Version,
    guid: Option<String>,
    tables: Vec<Table>,
    /// Index in `tables` of position 0 (see [`table_base`]).
    base: usize,
}

impl<'a> Db<'a> {
    fn parse(data: &'a [u8]) -> Result<Self, String> {
        let rest = data.strip_prefix(&MAGIC[..]).ok_or("Tekla: not a Tekla model database (header not found)")?;
        let (version, mut start) = if rest.starts_with(b"8.95") {
            (Version::V895, MAGIC.len() + 4)
        } else if rest.starts_with(b"7.82") {
            (Version::V782, MAGIC.len() + 4)
        } else {
            let v: String = rest.iter().take(8).take_while(|b| b.is_ascii_graphic()).map(|&b| b as char).collect();
            return Err(format!("Tekla: unsupported database version \"{v}\" (supported: 8.95, 7.82)"));
        };
        let mut guid = None;
        if version == Version::V895 {
            // "8.95 " then the 36-character model GUID
            if data.get(start) == Some(&b' ') {
                start += 1;
            }
            if let Some(g) = data.get(start..start + 36).filter(|g| g.iter().all(|b| b.is_ascii_hexdigit() || *b == b'-')) {
                guid = Some(String::from_utf8_lossy(g).into_owned());
                start += 36;
            }
        }
        let tables = read_tables(data, start);
        let base = table_base(&tables, version).ok_or_else(|| {
            format!("Tekla: the tables do not match spec v0.1 for version {} ({} tables found)", version.text(), tables.len())
        })?;
        Ok(Self { data, version, guid, tables, base })
    }

    /// The table at a spec position, if its record size is the one the spec gives.
    fn table(&self, (position, size): (usize, usize)) -> Option<&Table> {
        self.tables.get(self.base + position).filter(|t| t.size == size)
    }

    /// The rows of a table as (id, data).
    fn rows(&self, spec: (usize, usize)) -> impl Iterator<Item = (u32, &'a [u8])> + '_ {
        let data = self.data;
        self.table(spec).into_iter().flat_map(move |t| {
            t.rows.iter().map(move |&at| (u32_at(data, at + 1).unwrap_or(0), &data[at + 5..at + 5 + t.size]))
        })
    }

    /// The rows of a table by id (the first row of an id wins).
    fn by_id(&self, spec: (usize, usize)) -> HashMap<u32, &'a [u8]> {
        let mut out = HashMap::new();
        for (id, row) in self.rows(spec) {
            out.entry(id).or_insert(row);
        }
        out
    }

    /// §5 local frames by owner id: rows `u8 flag · f64 v1[3] · f64 v2[3] · u32 owner · u32 · u32`.
    fn frames(&self) -> HashMap<u32, (DVec3, DVec3)> {
        let mut out = HashMap::new();
        let Some(t) = self.table(FRAME_TABLE) else { return out };
        for &at in &t.rows {
            let raw = &self.data[at..at + t.size + 9];
            let (Some(v1), Some(v2), Some(owner)) = (vec3_at(raw, 1), vec3_at(raw, 25), u32_at(raw, 49)) else { continue };
            out.entry(owner).or_insert((v1, v2));
        }
        out
    }

    /// The parts as layers, one per class.
    fn loaded(&self) -> Loaded {
        let layout = self.version.layout();
        let shift = layout.attr_shift;
        let points: HashMap<u32, DVec3> = self
            .rows(POINT_TABLE)
            .filter(|(_, row)| u32_at(row, 0) == Some(3))
            .filter_map(|(id, row)| Some((id, vec3_at(row, 4)?)))
            .collect();
        let strings: HashMap<u32, String> = self.rows(STRING_TABLE).map(|(id, row)| (id, cstr(row, 12, 45))).collect();
        let attrs = self.by_id(layout.attr_table);
        let frames = self.frames();
        let positions = if self.version == Version::V895 { self.by_id(POSITION_TABLE) } else { HashMap::new() };
        let shapes = if self.version == Version::V782 { self.by_id(SHAPE_TABLE) } else { HashMap::new() };

        let mut layers: BTreeMap<String, Layer> = BTreeMap::new();
        for (id, row) in self.rows(layout.part_table) {
            let field = |offset: usize| u32_at(row, offset);
            let (Some(&start), Some(&end)) = (field(layout.part_start).and_then(|p| points.get(&p)), field(layout.part_end).and_then(|p| points.get(&p))) else {
                continue;
            };
            let attr = field(layout.part_attr).and_then(|a| attrs.get(&a)).copied();
            let text = |offset: usize, end: usize| attr.map(|a| cstr(a, offset + shift, end + shift)).unwrap_or_default();
            let class = text(28, 50);
            let profile_params = attr.and_then(|a| u32_at(a, 136 + shift)).and_then(|s| strings.get(&s)).cloned().unwrap_or_default();
            let profile = format!("{}{}", text(72, 136), profile_params);

            // §5: the 48-byte position block with the alignment in y (@24) and z (@40)
            let block = match self.version {
                Version::V782 => attr.and_then(|a| a.get(16..64)),
                Version::V895 => field(4).and_then(|p| positions.get(&p)).and_then(|r| r.get(..48)),
            };
            let align = |offset: usize| block.and_then(|b| u32_at(b, offset)).unwrap_or(0);
            let (align_y, align_z) = (align(24), align(40));

            // the part refers to its frame (@16/@20); the frame row names an owner id
            let frame = field(layout.part_frame).and_then(|f| frames.get(&f)).or_else(|| frames.get(&id)).copied();
            let contour = self.version == Version::V782 && attr.and_then(|a| u32_at(a, 4)) == Some(2);

            let mut mesh = Mesh::default();
            let kind = if contour {
                let shape = field(12).and_then(|s| shapes.get(&s)).copied();
                let (v1, v2) = frame.unwrap_or_else(|| axis_frame(end - start));
                let thickness = profile_numbers(&profile).first().copied().unwrap_or(DEFAULT_PLATE_THICKNESS);
                if let Some(shape) = shape {
                    contour_plate(&mut mesh, start, v1, v2, shape, thickness, align_z);
                }
                "Contour plate"
            } else {
                let (height, width) = profile_size(&profile);
                profile_part(&mut mesh, start, end, frame, height, width, align_y, align_z);
                "Profile part"
            };
            if mesh.indices.is_empty() {
                continue;
            }

            let layer = layers.entry(class.clone()).or_insert_with(|| Layer {
                name: if class.is_empty() { "No class".into() } else { format!("Class {class}") },
                color: class_color(&class),
                ..Layer::default()
            });
            let element = u64::from(id);
            let first = layer.indices.len() as u32;
            let offset = (layer.positions.len() / 3) as u32;
            layer.positions.extend_from_slice(&mesh.positions);
            layer.normals.extend_from_slice(&mesh.normals);
            layer.indices.extend(mesh.indices.iter().map(|i| i + offset));
            layer.elements.push((element, first, mesh.indices.len() as u32));

            let mut props = serde_json::Map::new();
            props.insert("Name".into(), text(50, 72).into());
            props.insert("Profile".into(), profile.into());
            props.insert("Material".into(), text(224, 332).into());
            props.insert("Class".into(), class.into());
            props.insert("Part prefix".into(), text(140, 224).into());
            props.insert("Type".into(), kind.into());
            props.insert("Tekla id".into(), id.into());
            if !contour {
                props.insert("Length".into(), format!("{:.0} mm", (end - start).length()).into());
            }
            layer.props.push((element, serde_json::Value::Object(props).to_string()));
        }

        let mut project = ProjectData::default();
        project.info.push(("Tekla database".into(), self.version.text().into()));
        if let Some(guid) = &self.guid {
            project.info.push(("GUID".into(), guid.clone()));
        }
        Loaded { layers: layers.into_values().collect(), views: Vec::new(), project }
    }
}

/// §3: every table from `start` on, in order. A table is found by its marker; after its rows
/// the next marker is searched again (it follows after 0–2 words). A table that runs past the
/// end of the data ends the list.
fn read_tables(data: &[u8], start: usize) -> Vec<Table> {
    let mut out = Vec::new();
    let mut pos = start;
    while let Some(at) = find(data, pos, &TABLE_MARKER) {
        let Some((table, end)) = read_table(data, at) else { break };
        out.push(table);
        pos = end;
    }
    out
}

/// One table at its marker: `u32 marker · u32 a · u32 n · u32[n] · row* · u8 0`, rows
/// `u8 flags · u32 id · u8[a] data · u32 tail`. Returns the table and the offset after the 0.
fn read_table(data: &[u8], at: usize) -> Option<(Table, usize)> {
    let size = u32_at(data, at + 4)? as usize;
    let n = u32_at(data, at + 8)? as usize;
    let row_len = size.checked_add(9)?;
    let mut p = at.checked_add(12)?.checked_add(n.checked_mul(4)?)?;
    let mut rows = Vec::new();
    loop {
        if *data.get(p)? == 0 {
            return Some((Table { size, rows }, p + 1));
        }
        if p.checked_add(row_len)? > data.len() {
            return None;
        }
        rows.push(p);
        p += row_len;
    }
}

/// Which entry of `tables` is position 0. The spec says the 3 header blocks do not count (§4);
/// whether those are 0x66 blocks is unclear (QUESTIONS.md, Q1), so the base is the first of
/// 3, 0, 1, 2 where the point, part and attribute tables have the record sizes of the spec.
fn table_base(tables: &[Table], version: Version) -> Option<usize> {
    let layout = version.layout();
    [3, 0, 1, 2].into_iter().find(|&base| {
        [POINT_TABLE, layout.part_table, layout.attr_table].iter().all(|&(position, size)| tables.get(base + position).is_some_and(|t| t.size == size))
    })
}

/// Plate thickness (mm) when the profile name holds no number (an assumption, not from the spec).
const DEFAULT_PLATE_THICKNESS: f64 = 10.0;

/// The numbers of a profile name after its letters, separated by `*`, `X` or `x`:
/// "SHS100*100*10" → [100, 100, 10], "IPE270" → [270], "HEA" → [].
fn profile_numbers(profile: &str) -> Vec<f64> {
    let Some(start) = profile.find(|c: char| c.is_ascii_digit()) else { return Vec::new() };
    profile[start..]
        .split(['*', 'X', 'x'])
        .map_while(|part| {
            let end = part.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(part.len());
            part[..end].parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0)
        })
        .collect()
}

/// Height (local y) and width (local z) in mm of the box drawn for a profile.
///
/// APPROXIMATION: profile dimensions are still open in spec v0.1 (§6), so this guesses from the
/// name. Two or more numbers are height × width ("PL200*10" → 200 × 10, "SHS100*100*10" →
/// 100 × 100); one number is the height with half of it as width ("IPE270" → 270 × 135);
/// anything else is 100 × 100.
fn profile_size(profile: &str) -> (f64, f64) {
    match profile_numbers(profile)[..] {
        [h, w, ..] => (h, w),
        [h] => (h, h / 2.0),
        [] => (100.0, 100.0),
    }
}

/// §5 alignment: 0 centred on the axis, 1 on the + side (0..size), 2 on the − side (−size..0).
/// Other values are drawn centred.
fn span(align: u32, size: f64) -> (f64, f64) {
    match align {
        1 => (0.0, size),
        2 => (-size, 0.0),
        _ => (-size / 2.0, size / 2.0),
    }
}

/// A frame (x, y) for an axis when the part's own frame is missing: y is the world Z (or X for
/// vertical axes) made perpendicular to x. A display fallback, not from the spec.
fn axis_frame(axis: DVec3) -> (DVec3, DVec3) {
    let x = axis.normalize_or(DVec3::X);
    let up = if x.z.abs() > 0.99 { DVec3::X } else { DVec3::Z };
    (x, (up - x * x.dot(up)).normalize())
}

/// §5 profile part: the cross-section height × width (local y × z, aligned) at the start point,
/// extruded along local x from 0 to |end − start|. The frame's v1 should point along the axis;
/// if it does not (or there is no frame) the axis frame is used.
#[allow(clippy::too_many_arguments)]
fn profile_part(mesh: &mut Mesh, start: DVec3, end: DVec3, frame: Option<(DVec3, DVec3)>, height: f64, width: f64, align_y: u32, align_z: u32) {
    let axis = end - start;
    let length = axis.length();
    if !(length > 1e-6) {
        return;
    }
    let x = axis / length;
    let y = frame
        .filter(|(v1, _)| v1.normalize_or_zero().dot(x).abs() > 0.99)
        .map(|(_, v2)| v2 - x * x.dot(v2))
        .filter(|y| y.length() > 1e-9)
        .map(DVec3::normalize)
        .unwrap_or_else(|| axis_frame(x).1);
    let z = x.cross(y);
    let (y0, y1) = span(align_y, height);
    let (z0, z1) = span(align_z, width);
    let base: Vec<DVec3> = [(y0, z0), (y1, z0), (y1, z1), (y0, z1)].iter().map(|&(a, b)| start + y * a + z * b).collect();
    mesh.prism(&base, x * length);
}

/// §5 contour plate (7.82): vertex i = start + x[i]·v1 + y[i]·v2 from the shape row (@8 f32 x[10],
/// @48 f32 y[10], @208 one u32 per vertex up to 0x7FFFFFFF), extruded by the thickness along
/// z = v1 × v2 by the z alignment.
fn contour_plate(mesh: &mut Mesh, start: DVec3, v1: DVec3, v2: DVec3, shape: &[u8], thickness: f64, align_z: u32) {
    let count = (0..10).take_while(|i| u32_at(shape, 208 + 4 * i).is_some_and(|v| v != 0x7FFF_FFFF)).count();
    let polygon: Vec<DVec3> = (0..count)
        .filter_map(|i| Some(start + v1 * f64::from(f32_at(shape, 8 + 4 * i)?) + v2 * f64::from(f32_at(shape, 48 + 4 * i)?)))
        .collect();
    let z = v1.cross(v2).normalize_or_zero();
    if polygon.len() < 3 || z == DVec3::ZERO {
        return;
    }
    let (z0, z1) = span(align_z, thickness);
    let base: Vec<DVec3> = polygon.iter().map(|&p| p + z * z0).collect();
    mesh.prism(&base, z * (z1 - z0));
}

/// Layer colour for a class: a fixed palette of our own, picked by the class number (or name).
fn class_color(class: &str) -> [f32; 3] {
    const PALETTE: [[f32; 3]; 10] = [
        [0.70, 0.71, 0.74],
        [0.84, 0.49, 0.36],
        [0.45, 0.66, 0.46],
        [0.42, 0.57, 0.80],
        [0.86, 0.74, 0.38],
        [0.62, 0.48, 0.74],
        [0.38, 0.70, 0.72],
        [0.78, 0.50, 0.62],
        [0.60, 0.62, 0.42],
        [0.55, 0.44, 0.36],
    ];
    let key = class.parse::<usize>().unwrap_or_else(|_| class.bytes().map(usize::from).sum());
    PALETTE[key % PALETTE.len()]
}

/// The triangles of one part, flat shaded, in metres.
#[derive(Default)]
struct Mesh {
    positions: Vec<f32>,
    normals: Vec<f32>,
    indices: Vec<u32>,
}

impl Mesh {
    /// A closed prism: the planar polygon `base` (mm) and its copy moved by `extrude`.
    fn prism(&mut self, base: &[DVec3], extrude: DVec3) {
        let mut base = base.to_vec();
        let n = newell(&base);
        if n.length() < 1e-9 || extrude.length() < 1e-9 {
            return;
        }
        // counter-clockwise seen from the extrusion side: the top cap faces out along `extrude`
        if n.dot(extrude) < 0.0 {
            base.reverse();
        }
        let up = extrude.normalize();
        let top: Vec<DVec3> = base.iter().map(|&p| p + extrude).collect();
        let tris = triangulate(&base, up);
        self.face(&top, &tris, up);
        self.face(&base, &tris, -up);
        for i in 0..base.len() {
            let (a, b) = (base[i], base[(i + 1) % base.len()]);
            let side = (b - a).cross(extrude);
            if side.length() > 1e-12 {
                self.face(&[a, b, b + extrude, a + extrude], &[[0, 1, 2], [0, 2, 3]], side.normalize());
            }
        }
    }

    /// One planar face with its own vertices; each triangle is wound to face `normal`.
    fn face(&mut self, points: &[DVec3], tris: &[[usize; 3]], normal: DVec3) {
        let first = (self.positions.len() / 3) as u32;
        for p in points {
            self.positions.extend_from_slice(&[(p.x * MM) as f32, (p.y * MM) as f32, (p.z * MM) as f32]);
            self.normals.extend_from_slice(&[normal.x as f32, normal.y as f32, normal.z as f32]);
        }
        for &[a, b, c] in tris {
            let facing = (points[b] - points[a]).cross(points[c] - points[a]).dot(normal) >= 0.0;
            let (b, c) = if facing { (b, c) } else { (c, b) };
            self.indices.extend([a, b, c].map(|i| first + i as u32));
        }
    }
}

/// Area vector of a polygon (Newell's method).
fn newell(points: &[DVec3]) -> DVec3 {
    let mut n = DVec3::ZERO;
    for i in 0..points.len() {
        n += points[i].cross(points[(i + 1) % points.len()]);
    }
    n * 0.5
}

/// Triangles of a planar polygon with normal `n`.
fn triangulate(points: &[DVec3], n: DVec3) -> Vec<[usize; 3]> {
    let fan = || (1..points.len().saturating_sub(1)).map(|i| [0, i, i + 1]).collect();
    if points.len() == 3 {
        return vec![[0, 1, 2]];
    }
    let u = n.any_orthonormal_vector();
    let v = n.cross(u);
    let flat: Vec<f64> = points.iter().flat_map(|&p| [p.dot(u), p.dot(v)]).collect();
    match earcutr::earcut(&flat, &[], 2) {
        Ok(t) if !t.is_empty() => t.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect(),
        _ => fan(),
    }
}

fn find(data: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    data.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|p| from + p)
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

fn f32_at(data: &[u8], at: usize) -> Option<f32> {
    u32_at(data, at).map(f32::from_bits).filter(|v| v.is_finite())
}

fn f64_at(data: &[u8], at: usize) -> Option<f64> {
    Some(f64::from_le_bytes(data.get(at..at.checked_add(8)?)?.try_into().ok()?)).filter(|v| v.is_finite())
}

fn vec3_at(data: &[u8], at: usize) -> Option<DVec3> {
    Some(DVec3::new(f64_at(data, at)?, f64_at(data, at + 8)?, f64_at(data, at + 16)?))
}

/// The C string at `start`, at most up to `end` (the next field); the bytes after the 0 are
/// ignored (§4). The encoding is not in the spec (QUESTIONS.md): UTF-8 if valid, else Latin-1.
/// Trailing spaces are removed.
fn cstr(data: &[u8], start: usize, end: usize) -> String {
    let bytes = data.get(start..end.min(data.len())).unwrap_or_default();
    let bytes = &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())];
    let text = match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    };
    text.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A synthetic database built from the spec: header, three header blocks, the tables of the
    /// version and the given rows.
    struct Synth {
        version: Version,
        /// Extra 0x66 blocks before position 0 (the "3 header blocks" of §4).
        lead_tables: usize,
        /// Record size and raw rows per position.
        tables: BTreeMap<usize, (usize, Vec<Vec<u8>>)>,
    }

    impl Synth {
        fn new(version: Version) -> Self {
            Self { version, lead_tables: 3, tables: BTreeMap::new() }
        }

        /// Adds a row `flags · id · data (padded to a) · tail` to the table at `spec`.
        fn row(&mut self, spec: (usize, usize), id: u32, data: &[u8]) -> &mut Self {
            let mut row = vec![0x04];
            row.extend_from_slice(&id.to_le_bytes());
            let mut d = data.to_vec();
            d.resize(spec.1, 0);
            row.extend_from_slice(&d);
            row.extend_from_slice(&0u32.to_le_bytes());
            self.raw(spec, row)
        }

        fn raw(&mut self, spec: (usize, usize), row: Vec<u8>) -> &mut Self {
            assert_eq!(row.len(), spec.1 + 9);
            self.tables.entry(spec.0).or_insert((spec.1, Vec::new())).1.push(row);
            self
        }

        fn point(&mut self, id: u32, p: [f64; 3]) -> &mut Self {
            let mut d = 3u32.to_le_bytes().to_vec();
            p.iter().for_each(|v| d.extend_from_slice(&v.to_le_bytes()));
            self.row(POINT_TABLE, id, &d)
        }

        fn string(&mut self, id: u32, s: &str) -> &mut Self {
            let mut d = vec![0u8; 12];
            d.extend_from_slice(s.as_bytes());
            self.row(STRING_TABLE, id, &d)
        }

        /// §5 frame row: `u8 flag · f64 v1[3] · f64 v2[3] · u32 owner · u32 · u32`.
        fn frame(&mut self, owner: u32, v1: [f64; 3], v2: [f64; 3]) -> &mut Self {
            let mut row = vec![0x04];
            v1.iter().chain(&v2).for_each(|v| row.extend_from_slice(&v.to_le_bytes()));
            row.extend_from_slice(&owner.to_le_bytes());
            row.extend_from_slice(&[0; 8]);
            self.raw(FRAME_TABLE, row)
        }

        fn bytes(&self) -> Vec<u8> {
            let mut out = MAGIC.to_vec();
            match self.version {
                Version::V895 => {
                    out.extend_from_slice(b"8.95 ");
                    out.extend_from_slice(b"0f8fad5b-d9cb-469f-a165-70867728950e");
                }
                Version::V782 => out.extend_from_slice(b"7.82"),
            }
            // header blocks 0xDBCEC0BC and 0xDBCEC0C9
            for marker in [0xDBCE_C0BCu32, 0xDBCE_C0C9] {
                out.extend_from_slice(&marker.to_le_bytes());
                out.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
            }
            let count = match self.version {
                Version::V895 => 326,
                Version::V782 => 228,
            };
            for i in 0..self.lead_tables + count {
                let (size, rows) = i
                    .checked_sub(self.lead_tables)
                    .and_then(|p| self.tables.get(&p))
                    .cloned()
                    .unwrap_or((8, Vec::new()));
                out.extend_from_slice(&TABLE_MARKER);
                out.extend_from_slice(&(size as u32).to_le_bytes());
                out.extend_from_slice(&1u32.to_le_bytes());
                out.extend_from_slice(&7u32.to_le_bytes());
                rows.iter().for_each(|r| out.extend_from_slice(r));
                out.push(0);
                // 0–2 words before the next marker
                out.extend(std::iter::repeat_n(0u8, 4 * (i % 3)));
            }
            out
        }

        fn gz(&self) -> Vec<u8> {
            let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
            enc.write_all(&self.bytes()).unwrap();
            enc.finish().unwrap()
        }

        fn load(&self) -> Loaded {
            let data = gunzip(&self.gz()).expect("gunzip");
            Db::parse(&data).expect("parse").loaded()
        }
    }

    /// An attribute set row: class, name, profile prefix, string id, part prefix, material at
    /// the 8.95 offsets plus `shift`; `head` is written first (7.82: kind @4, block @16).
    fn attr(shift: usize, head: &[(usize, &[u8])], class: &str, name: &str, prefix: &str, params: u32, material: &str) -> Vec<u8> {
        let mut d = vec![0u8; 332 + shift];
        for (at, bytes) in head {
            d[*at..*at + bytes.len()].copy_from_slice(bytes);
        }
        let mut put = |at: usize, b: &[u8]| d[at + shift..at + shift + b.len()].copy_from_slice(b);
        put(28, format!("{class}   \0junk").as_bytes());
        put(50, name.as_bytes());
        put(72, prefix.as_bytes());
        put(136, &params.to_le_bytes());
        put(140, b"B");
        put(224, material.as_bytes());
        d
    }

    fn u32s(values: &[(usize, u32)], size: usize) -> Vec<u8> {
        let mut d = vec![0u8; size];
        for &(at, v) in values {
            d[at..at + 4].copy_from_slice(&v.to_le_bytes());
        }
        d
    }

    fn bounds(layers: &[Layer]) -> ([f32; 3], [f32; 3]) {
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in layers.iter().flat_map(|l| l.positions.chunks_exact(3)) {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (lo, hi)
    }

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5)
    }

    /// 8.95: one IPE270 beam of 6 m along X, local y = world Z, aligned on the + side in y.
    fn beam_895() -> Synth {
        let mut s = Synth::new(Version::V895);
        s.point(101, [0.0, 0.0, 0.0])
            .point(102, [6000.0, 0.0, 0.0])
            .string(201, "270")
            .row((299, 332), 301, &attr(0, &[], "3", "BEAM", "IPE", 201, "S235JR"))
            // position block: alignment y @24 = 1 (+ side), z @40 = 0 (centred)
            .row(POSITION_TABLE, 401, &u32s(&[(24, 1), (40, 0)], 52))
            .frame(501, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0])
            .row((274, 64), 900, &u32s(&[(0, 301), (4, 401), (8, 101), (12, 102), (20, 501)], 64));
        s
    }

    #[test]
    fn beam_895_box_props_and_layer() {
        let loaded = beam_895().load();
        assert_eq!(loaded.layers.len(), 1);
        let l = &loaded.layers[0];
        assert_eq!(l.name, "Class 3");
        assert_eq!(l.triangle_count(), 12);
        assert_eq!(l.elements, vec![(900, 0, 36)]);
        assert!(openaec_model::model::encloses_volume(&l.positions, &l.indices));
        // metres; y = world Z from 0 to 270 mm, z = x × y = −Y, centred: width 135 mm
        let (lo, hi) = bounds(&loaded.layers);
        assert!(close(lo, [0.0, -0.0675, 0.0]) && close(hi, [6.0, 0.0675, 0.27]), "{lo:?} {hi:?}");
        let props: serde_json::Value = serde_json::from_str(&l.props[0].1).unwrap();
        assert_eq!(props["Name"], "BEAM");
        assert_eq!(props["Profile"], "IPE270");
        assert_eq!(props["Material"], "S235JR");
        assert_eq!(props["Class"], "3");
        assert_eq!(props["Part prefix"], "B");
        assert_eq!(props["Length"], "6000 mm");
        assert!(loaded.project.info.iter().any(|(k, v)| k == "GUID" && v.len() == 36));
    }

    #[test]
    fn outward_normals() {
        let loaded = beam_895().load();
        let l = &loaded.layers[0];
        let p = |i: u32| glam::Vec3::from_slice(&l.positions[3 * i as usize..]);
        let centre = glam::Vec3::new(3.0, 0.0, 0.135);
        for t in l.indices.chunks_exact(3) {
            let n = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0]));
            let stored = glam::Vec3::from_slice(&l.normals[3 * t[0] as usize..]);
            assert!(n.dot(p(t[0]) - centre) > 0.0 && n.dot(stored) > 0.0);
        }
    }

    #[test]
    fn table_base_without_lead_tables() {
        let mut s = beam_895();
        s.lead_tables = 0;
        assert_eq!(s.load().layers[0].triangle_count(), 12);
    }

    #[test]
    fn missing_frame_falls_back_to_axis() {
        let mut s = beam_895();
        s.tables.remove(&FRAME_TABLE.0);
        let loaded = s.load();
        let (lo, hi) = bounds(&loaded.layers);
        assert!(close(lo, [0.0, -0.0675, 0.0]) && close(hi, [6.0, 0.0675, 0.27]), "{lo:?} {hi:?}");
    }

    /// 7.82: a 1000 × 500 contour plate PL10 on the − side, and an HEA300 column (+48 offsets).
    #[test]
    fn contour_plate_and_column_782() {
        let mut s = Synth::new(Version::V782);
        let mut block = vec![0u8; 48];
        block[40..44].copy_from_slice(&2u32.to_le_bytes());
        let plate_attr = attr(48, &[(4, &2u32.to_le_bytes()), (16, &block)], "8", "PLATE", "PL", 202, "S355");
        let column_attr = attr(48, &[], "2", "COLUMN", "HEA", 203, "S235");
        let mut shape = vec![0u8; 332];
        for (i, (x, y)) in [(0.0f32, 0.0f32), (1000.0, 0.0), (1000.0, 500.0), (0.0, 500.0)].into_iter().enumerate() {
            shape[8 + 4 * i..12 + 4 * i].copy_from_slice(&x.to_le_bytes());
            shape[48 + 4 * i..52 + 4 * i].copy_from_slice(&y.to_le_bytes());
            shape[208 + 4 * i..212 + 4 * i].copy_from_slice(&0u32.to_le_bytes());
        }
        shape[224..228].copy_from_slice(&0x7FFF_FFFFu32.to_le_bytes());
        s.point(1, [100.0, 200.0, 3000.0])
            .point(2, [1100.0, 200.0, 3000.0])
            .point(3, [0.0, 0.0, 0.0])
            .point(4, [0.0, 0.0, 3000.0])
            .string(202, "10")
            .string(203, "300")
            .row((207, 380), 11, &plate_attr)
            .row((207, 380), 12, &column_attr)
            .row(SHAPE_TABLE, 21, &shape)
            .frame(31, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
            .frame(32, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0])
            .row((193, 56), 41, &u32s(&[(0, 11), (4, 1), (8, 2), (12, 21), (16, 31)], 56))
            .row((193, 56), 42, &u32s(&[(0, 12), (4, 3), (8, 4), (16, 32)], 56));
        let loaded = s.load();
        let names: Vec<&str> = loaded.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Class 2", "Class 8"]);
        let column = &loaded.layers[0];
        let plate = &loaded.layers[1];
        // plate: x 0.1..1.1, y 0.2..0.7, z from 3.0 − 0.010 to 3.0
        let (lo, hi) = bounds(std::slice::from_ref(plate));
        assert!(close(lo, [0.1, 0.2, 2.99]) && close(hi, [1.1, 0.7, 3.0]), "{lo:?} {hi:?}");
        assert!(openaec_model::model::encloses_volume(&plate.positions, &plate.indices));
        let props: serde_json::Value = serde_json::from_str(&plate.props[0].1).unwrap();
        assert_eq!((props["Profile"].as_str(), props["Type"].as_str()), (Some("PL10"), Some("Contour plate")));
        // column: 3 m up, 300 along local y = world X, 150 along z = Z × X = Y, centred
        let (lo, hi) = bounds(std::slice::from_ref(column));
        assert!(close(lo, [-0.15, -0.075, 0.0]) && close(hi, [0.15, 0.075, 3.0]), "{lo:?} {hi:?}");
        let props: serde_json::Value = serde_json::from_str(&column.props[0].1).unwrap();
        assert_eq!((props["Name"].as_str(), props["Profile"].as_str(), props["Material"].as_str()), (Some("COLUMN"), Some("HEA300"), Some("S235")));
    }

    #[test]
    fn profile_size_heuristic() {
        assert_eq!(profile_size("PL200*10"), (200.0, 10.0));
        assert_eq!(profile_size("SHS100*100*10"), (100.0, 100.0));
        assert_eq!(profile_size("RHS120X80X5"), (120.0, 80.0));
        assert_eq!(profile_size("L80x60x6"), (80.0, 60.0));
        assert_eq!(profile_size("IPE270"), (270.0, 135.0));
        assert_eq!(profile_size("PL12.5*150"), (12.5, 150.0));
        assert_eq!(profile_size("HEA"), (100.0, 100.0));
        assert_eq!(profile_size(""), (100.0, 100.0));
    }

    #[test]
    fn rejects_bad_input() {
        assert!(gunzip(b"not gzip").is_err());
        assert!(Db::parse(b"PK\x03\x04 something").is_err());
        let mut v = MAGIC.to_vec();
        v.extend_from_slice(b"9.99 ");
        assert!(Db::parse(&v).err().is_some_and(|e| e.contains("9.99")));
        // the right header but no tables
        let mut v = MAGIC.to_vec();
        v.extend_from_slice(b"7.82");
        assert!(Db::parse(&v).is_err());
    }

    #[test]
    fn registered_for_db1() {
        let path = std::env::temp_dir().join(format!("o3v-tekla-test-{}.db1", std::process::id()));
        std::fs::write(&path, beam_895().gz()).unwrap();
        let model = openaec_model::load_with_loader(&TeklaLoader, &Source::from(path.clone()), &Progress::default());
        let _ = std::fs::remove_file(&path);
        let model = model.expect("db1");
        assert_eq!(model.format, "Tekla Structures");
        assert!(!model.source_y_up);
        assert_eq!(model.triangle_count(), 12);
        assert!(model.props.contains_key(&900));
    }
}
