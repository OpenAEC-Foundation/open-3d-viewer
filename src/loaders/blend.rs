//! Blender files (.blend), read natively: the mesh objects of the file, each a layer, placed by
//! its object's matrix, coloured by its materials.
//!
//! A `.blend` is Blender's memory written out: a header (pointer size, byte order, version) and
//! blocks, each one or more structs as they lay in memory, with the address they had. The `DNA1`
//! block describes every struct of the file field by field, so fields are found by name and
//! pointers are followed by address; that reads every version alike. Read here:
//! - the file as it is, gzip (to 2.93) or Zstandard (from 3.0) compressed; headers of Blender 2.8
//!   to 4.x (`BLENDER-v405`) and of 5.x (`BLENDER17-01v0500`, with its wider block headers);
//! - an `Object` of type mesh: its name, its `Mesh` (`data`), its place (`obmat`, else its
//!   location, rotation and scale under its parent's) and whether it is hidden in viewports;
//! - a `Mesh`: its vertices, face corners and faces, as arrays of structs (`mvert`, `mloop`,
//!   `mpoly`, to 3.x) or as attribute layers (`position`, `.corner_vert`, the face offsets and
//!   `material_index`, from 4.0); its materials' viewport colours.
//!
//! Left out: what Blender works out when it opens the file (modifiers, geometry nodes, instanced
//! collections), curves, texts and other objects without a mesh, linked libraries, big-endian
//! files. Blender's metres and Z up are the model's.

use super::{Loader, Source, DEFAULT_COLOR};
use crate::model::Layer;
use glam::{Mat4, Quat, Vec3};
use std::collections::HashMap;

pub struct BlendLoader;

impl Loader for BlendLoader {
    fn name(&self) -> &'static str {
        "Blender"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["blend"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        let data = source.read()?;
        let data = unpack(&data)?;
        let file = File::parse(&data)?;
        let layers = layers(&file);
        if layers.is_empty() {
            return Err("Blender: the file has no mesh objects".into());
        }
        Ok(layers)
    }
}

/// The file's bytes, uncompressed.
fn unpack(data: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Read;
    if data.starts_with(b"BLENDER") {
        return Ok(data.to_vec());
    }
    let mut out = Vec::new();
    if data.starts_with(&[0x1f, 0x8b]) {
        flate2::read::GzDecoder::new(data).read_to_end(&mut out).map_err(|e| format!("Blender: cannot unpack the file: {e}"))?;
        return Ok(out);
    }
    // Zstandard: frames one after another, with skippable ones (a seek table) between them
    let mut rest = data;
    while rest.len() >= 8 {
        let magic = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]);
        if magic & 0xffff_fff0 == 0x184d_2a50 {
            let skip = u32::from_le_bytes([rest[4], rest[5], rest[6], rest[7]]) as usize;
            rest = rest.get(8 + skip..).unwrap_or(&[]);
        } else if magic == 0xfd2f_b528 {
            let mut decoder = ruzstd::decoding::StreamingDecoder::new(&mut rest).map_err(|e| format!("Blender: cannot unpack the file: {e}"))?;
            decoder.read_to_end(&mut out).map_err(|e| format!("Blender: cannot unpack the file: {e}"))?;
        } else {
            break;
        }
    }
    if out.starts_with(b"BLENDER") {
        Ok(out)
    } else {
        Err("Blender: not a .blend file".into())
    }
}

/// A field of a struct: its type, how many of it, and where it lies.
#[derive(Debug, Clone)]
struct Field {
    name: String,
    kind: usize,
    pointer: bool,
    offset: usize,
    size: usize,
}

/// A struct of the file's DNA.
#[derive(Debug, Clone, Default)]
struct Struct {
    kind: usize,
    fields: Vec<Field>,
}

/// A block of the file: structs of one kind, as they lay at address `old`.
#[derive(Debug, Clone, Copy)]
struct Block {
    code: [u8; 4],
    kind: usize,
    old: u64,
    start: usize,
    len: usize,
    /// The data-block it belongs to: itself, or for data the block of the data-block before it.
    owner: usize,
}

struct File<'a> {
    data: &'a [u8],
    /// The size of a pointer in the file (4 or 8).
    pointer: usize,
    blocks: Vec<Block>,
    /// The blocks by their address: within their data-block, and anywhere (the first there).
    by_address: HashMap<(usize, u64), usize>,
    anywhere: HashMap<u64, usize>,
    types: Vec<String>,
    sizes: Vec<usize>,
    structs: Vec<Struct>,
    struct_of: HashMap<usize, usize>,
}

fn u16_at(d: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_le_bytes(d.get(at..at + 2)?.try_into().ok()?) as usize)
}

fn u32_at(d: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(d.get(at..at + 4)?.try_into().ok()?) as usize)
}

fn u64_at(d: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(d.get(at..at + 8)?.try_into().ok()?))
}

impl<'a> File<'a> {
    fn parse(data: &'a [u8]) -> Result<Self, String> {
        let bad = || "Blender: the file is damaged".to_string();
        // the header: `BLENDER-v405`, or from 5.0 `BLENDER17-01v0500` (its size, the format, the version)
        let (pointer, order, mut at, wide) = match data.get(7) {
            Some(b'_') => (4, data.get(8), 12, false),
            Some(b'-') => (8, data.get(8), 12, false),
            Some(c) if c.is_ascii_digit() => {
                let size: usize = std::str::from_utf8(data.get(7..9).ok_or_else(bad)?).ok().and_then(|s| s.parse().ok()).ok_or_else(bad)?;
                (8, data.get(12), size, true)
            }
            _ => return Err(bad()),
        };
        if order != Some(&b'v') {
            return Err("Blender: files written on big-endian machines are not supported".into());
        }
        let mut blocks = Vec::new();
        let mut dna = None;
        loop {
            let code: [u8; 4] = data.get(at..at + 4).and_then(|c| c.try_into().ok()).ok_or_else(bad)?;
            let (kind, old, len, head) = if wide {
                (u32_at(data, at + 4), u64_at(data, at + 8), u64_at(data, at + 16).map(|l| l as usize), 32)
            } else if pointer == 8 {
                (u32_at(data, at + 16), u64_at(data, at + 8), u32_at(data, at + 4), 24)
            } else {
                (u32_at(data, at + 12), u32_at(data, at + 8).map(|a| a as u64), u32_at(data, at + 4), 20)
            };
            let (Some(kind), Some(old), Some(len)) = (kind, old, len) else { return Err(bad()) };
            let start = at + head;
            if &code == b"ENDB" {
                break;
            }
            if start.checked_add(len).is_none_or(|end| end > data.len()) {
                return Err(bad());
            }
            if &code == b"DNA1" {
                dna = Some((start, len));
            }
            // data follows the data-block it is of
            let owner = if &code == b"DATA" { blocks.last().map_or(0, |b: &Block| b.owner) } else { blocks.len() };
            blocks.push(Block { code, kind, old, start, len, owner });
            at = start + len;
        }
        let (start, len) = dna.ok_or("Blender: the file does not describe its structs")?;
        let (types, sizes, structs) = read_dna(&data[start..start + len], pointer).ok_or_else(bad)?;
        // an address is one block within a data-block (from 5.0 addresses repeat between them);
        // a data-block itself is found from anywhere
        let (mut by_address, mut anywhere) = (HashMap::new(), HashMap::new());
        for (i, b) in blocks.iter().enumerate() {
            by_address.entry((b.owner, b.old)).or_insert(i);
            if b.owner == i {
                anywhere.insert(b.old, i);
            }
        }
        for (i, b) in blocks.iter().enumerate() {
            anywhere.entry(b.old).or_insert(i);
        }
        let struct_of = structs.iter().enumerate().map(|(i, s)| (s.kind, i)).collect();
        Ok(File { data, pointer, blocks, by_address, anywhere, types, sizes, structs, struct_of })
    }

    /// The first struct of a block.
    fn view(&self, block: &Block) -> Option<View<'_>> {
        self.structs.get(block.kind).map(|_| View { file: self, kind: block.kind, at: block.start, owner: block.owner })
    }

    /// The block a pointer in the data-block `owner` leads to.
    fn target(&self, owner: usize, address: u64) -> Option<&Block> {
        (address != 0).then(|| self.by_address.get(&(owner, address)).or_else(|| self.anywhere.get(&address)).map(|&i| &self.blocks[i])).flatten()
    }

    /// The bytes of the block a pointer leads to.
    fn bytes(&self, owner: usize, address: u64) -> Option<&[u8]> {
        self.target(owner, address).map(|b| &self.data[b.start..b.start + b.len])
    }

    /// The size of a struct, by its name.
    fn size_of(&self, name: &str) -> Option<usize> {
        self.types.iter().position(|t| t == name).and_then(|k| self.sizes.get(k).copied()).filter(|s| *s > 0)
    }
}

/// The types, their sizes and the structs a `DNA1` block describes.
fn read_dna(d: &[u8], pointer: usize) -> Option<(Vec<String>, Vec<usize>, Vec<Struct>)> {
    let mut at = 4;
    let section = |at: &mut usize, tag: &[u8; 4]| -> Option<usize> {
        *at = (*at + 3) & !3;
        (d.get(*at..*at + 4)? == tag).then_some(())?;
        let count = u32_at(d, *at + 4)?;
        *at += 8;
        Some(count)
    };
    let strings = |at: &mut usize, count: usize| -> Option<Vec<String>> {
        (0..count)
            .map(|_| {
                let end = *at + d.get(*at..)?.iter().position(|b| *b == 0)?;
                let s = String::from_utf8_lossy(&d[*at..end]).into_owned();
                *at = end + 1;
                Some(s)
            })
            .collect()
    };
    (d.get(..4)? == b"SDNA").then_some(())?;
    let count = section(&mut at, b"NAME")?;
    let names = strings(&mut at, count)?;
    let count = section(&mut at, b"TYPE")?;
    let types = strings(&mut at, count)?;
    section_plain(d, &mut at, b"TLEN")?;
    let sizes: Vec<usize> = (0..types.len()).map(|k| u16_at(d, at + 2 * k)).collect::<Option<_>>()?;
    at += 2 * types.len();
    let count = section(&mut at, b"STRC")?;
    let mut structs = Vec::with_capacity(count);
    for _ in 0..count {
        let (kind, fields) = (u16_at(d, at)?, u16_at(d, at + 2)?);
        at += 4;
        let mut s = Struct { kind, fields: Vec::with_capacity(fields) };
        let mut offset = 0;
        for _ in 0..fields {
            let (kind, name) = (u16_at(d, at)?, names.get(u16_at(d, at + 2)?)?);
            at += 4;
            // `*next`, `mat[4][4]`, `(*bind)()`
            let is_pointer = name.starts_with('*') || name.starts_with("(*");
            let many: usize = name.split('[').skip(1).filter_map(|n| n.trim_end_matches(']').parse::<usize>().ok()).product();
            let bare: String = name.trim_start_matches(['*', '(']).chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            let size = if is_pointer { pointer } else { *sizes.get(kind)? } * many;
            s.fields.push(Field { name: bare, kind, pointer: is_pointer, offset, size });
            offset += size;
        }
        structs.push(s);
    }
    Some((types, sizes, structs))
}

/// Steps over the tag of a section that has no count of its own.
fn section_plain(d: &[u8], at: &mut usize, tag: &[u8; 4]) -> Option<()> {
    *at = (*at + 3) & !3;
    (d.get(*at..*at + 4)? == tag).then_some(())?;
    *at += 4;
    Some(())
}

/// A struct in the file.
#[derive(Clone, Copy)]
struct View<'a> {
    file: &'a File<'a>,
    kind: usize,
    at: usize,
    /// The data-block it is in: its pointers lead to data of that one.
    owner: usize,
}

impl<'a> View<'a> {
    /// The field of one of these names (a file names a field as the version that introduced it
    /// did, or as its own version does).
    fn field(&self, names: &[&str]) -> Option<&'a Field> {
        let s = self.file.structs.get(self.kind)?;
        names.iter().find_map(|n| s.fields.iter().find(|f| f.name == *n))
    }

    fn raw(&self, names: &[&str]) -> Option<&'a [u8]> {
        let f = self.field(names)?;
        self.file.data.get(self.at + f.offset..self.at + f.offset + f.size)
    }

    /// A struct held in this one.
    fn part(&self, names: &[&str]) -> Option<View<'a>> {
        let f = self.field(names).filter(|f| !f.pointer)?;
        Some(View { file: self.file, kind: *self.file.struct_of.get(&f.kind)?, at: self.at + f.offset, owner: self.owner })
    }

    fn floats(&self, names: &[&str]) -> Option<Vec<f32>> {
        let raw = self.raw(names)?;
        Some(raw.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
    }

    /// A whole number of 1, 2, 4 or 8 bytes.
    fn int(&self, names: &[&str]) -> Option<i64> {
        let raw = self.raw(names)?;
        Some(match raw.len() {
            1 => raw[0] as i8 as i64,
            2 => i16::from_le_bytes([raw[0], raw[1]]) as i64,
            4 => i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as i64,
            8 => i64::from_le_bytes(raw.try_into().ok()?),
            _ => return None,
        })
    }

    fn pointer(&self, names: &[&str]) -> Option<u64> {
        let f = self.field(names).filter(|f| f.pointer)?;
        let at = self.at + f.offset;
        if self.file.pointer == 8 { u64_at(self.file.data, at) } else { u32_at(self.file.data, at).map(|a| a as u64) }
    }

    /// The struct a pointer field leads to.
    fn follow(&self, names: &[&str]) -> Option<View<'a>> {
        self.file.view(self.file.target(self.owner, self.pointer(names)?)?)
    }

    fn text(&self, names: &[&str]) -> Option<String> {
        let raw = self.raw(names)?;
        let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
        Some(String::from_utf8_lossy(&raw[..end]).into_owned())
    }

    /// The name of a data-block, without the two letters of its kind.
    fn name(&self) -> String {
        let name = self.part(&["id"]).and_then(|id| id.text(&["name"])).unwrap_or_default();
        name.chars().skip(2).collect()
    }
}

/// Where an object stands in the world: the matrix Blender stored, else its own place under its
/// parent's.
fn world(object: View, depth: usize) -> Mat4 {
    if let Some(m) = object.floats(&["obmat", "object_to_world"]).filter(|m| m.len() == 16 && m.iter().all(|x| x.is_finite())) {
        let m = Mat4::from_cols_slice(&m);
        // an identity may be a matrix never worked out (a file a script wrote): then its own place counts
        if m.determinant().abs() > 1e-12 && !m.abs_diff_eq(Mat4::IDENTITY, 1e-7) {
            return m;
        }
    }
    let vec3 = |names: &[&str], or: Vec3| object.floats(names).filter(|v| v.len() >= 3 && v.iter().all(|x| x.is_finite())).map_or(or, |v| Vec3::new(v[0], v[1], v[2]));
    let quat = |names: &[&str]| object.floats(names).filter(|q| q.len() == 4 && q.iter().any(|x| *x != 0.0)).map(|q| Quat::from_xyzw(q[1], q[2], q[3], q[0]).normalize());
    // the turns of an Euler rotation, in the order its mode names (1 = XYZ … 6 = ZYX)
    let euler = |e: Vec3, mode: i64| {
        let order: [usize; 3] = match mode {
            2 => [0, 2, 1],
            3 => [1, 0, 2],
            4 => [1, 2, 0],
            5 => [2, 0, 1],
            6 => [2, 1, 0],
            _ => [0, 1, 2],
        };
        let turn = |axis: usize| Quat::from_axis_angle([Vec3::X, Vec3::Y, Vec3::Z][axis], e[axis]);
        turn(order[2]) * turn(order[1]) * turn(order[0])
    };
    let mode = object.int(&["rotmode"]).unwrap_or(1);
    let (turned, delta) = match mode {
        0 => (quat(&["quat"]).unwrap_or(Quat::IDENTITY), quat(&["dquat"]).unwrap_or(Quat::IDENTITY)),
        -1 => {
            let axis = vec3(&["rotAxis"], Vec3::Z).try_normalize().unwrap_or(Vec3::Z);
            let angle = object.floats(&["rotAngle"]).and_then(|a| a.first().copied()).unwrap_or(0.0);
            (Quat::from_axis_angle(axis, angle), Quat::IDENTITY)
        }
        mode => (euler(vec3(&["rot"], Vec3::ZERO), mode), euler(vec3(&["drot"], Vec3::ZERO), mode)),
    };
    let scale = vec3(&["size", "scale"], Vec3::ONE) * vec3(&["dscale", "dsize"], Vec3::ONE);
    let own = Mat4::from_scale_rotation_translation(scale, delta * turned, vec3(&["loc"], Vec3::ZERO) + vec3(&["dloc"], Vec3::ZERO));
    match object.follow(&["parent"]).filter(|_| depth < 64) {
        Some(parent) => {
            let inverse = object.floats(&["parentinv"]).filter(|m| m.len() == 16 && m.iter().all(|x| x.is_finite())).map_or(Mat4::IDENTITY, |m| Mat4::from_cols_slice(&m));
            world(parent, depth + 1) * inverse * own
        }
        None => own,
    }
}

/// A mesh as read: its points, and per face its corners (indices of points) and material.
#[derive(Default)]
struct Mesh {
    points: Vec<Vec3>,
    faces: Vec<(Vec<u32>, usize)>,
}

fn ints(raw: &[u8]) -> impl Iterator<Item = i32> + '_ {
    raw.chunks_exact(4).map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
}

/// The data of the attribute layer `name` of the kind `kind` in a mesh's layers of points,
/// corners or faces (`CustomData`).
fn layer<'a>(data: Option<View<'a>>, kind: i64, name: &str) -> Option<&'a [u8]> {
    let data = data?;
    let file = data.file;
    let count = data.int(&["totlayer"]).filter(|n| *n > 0)? as usize;
    let block = file.target(data.owner, data.pointer(&["layers"])?)?;
    let size = file.size_of("CustomDataLayer")?;
    (0..count.min(block.len / size)).find_map(|k| {
        let l = View { file, kind: block.kind, at: block.start + k * size, owner: data.owner };
        (l.int(&["type"]) == Some(kind) && l.text(&["name"]).as_deref() == Some(name)).then(|| file.bytes(data.owner, l.pointer(&["data"])?)).flatten()
    })
}

/// The data of the attribute `name` of a mesh, as Blender 5 stores its attributes: an array of
/// them (`AttributeStorage`), each with its name and, when it is an array, its values.
fn attribute<'a>(mesh: View<'a>, name: &str) -> Option<&'a [u8]> {
    let file = mesh.file;
    let storage = mesh.part(&["attribute_storage"])?;
    let count = storage.int(&["dna_attributes_num"]).filter(|n| *n > 0)? as usize;
    let block = file.target(mesh.owner, storage.pointer(&["dna_attributes"])?)?;
    let kind_of = |name: &str| file.types.iter().position(|t| t == name).and_then(|k| file.struct_of.get(&k).copied());
    let (kind, size) = (kind_of("Attribute")?, file.size_of("Attribute")?);
    (0..count.min(block.len / size)).find_map(|k| {
        let a = View { file, kind, at: block.start + k * size, owner: mesh.owner };
        let named = file.bytes(mesh.owner, a.pointer(&["name"])?)?;
        let end = named.iter().position(|b| *b == 0).unwrap_or(named.len());
        (&named[..end] == name.as_bytes() && a.int(&["storage_type"]) == Some(0)).then_some(())?;
        let array = View { file, kind: kind_of("AttributeArray")?, at: file.target(mesh.owner, a.pointer(&["data"])?)?.start, owner: mesh.owner };
        file.bytes(mesh.owner, array.pointer(&["data"])?)
    })
}

fn read_mesh(mesh: View) -> Mesh {
    let file = mesh.file;
    let count = |names: &[&str]| mesh.int(names).filter(|n| *n > 0).map_or(0, |n| n as usize);
    let (points_n, corners_n, faces_n) = (count(&["totvert", "verts_num"]), count(&["totloop", "corners_num"]), count(&["totpoly", "faces_num"]));
    // a struct of an array, by the offset of one of its fields
    let field_of = |name: &str, field: &str| {
        let k = file.types.iter().position(|t| t == name)?;
        let s = &file.structs[*file.struct_of.get(&k)?];
        Some((s.fields.iter().find(|f| f.name == field)?.offset, file.sizes[k]))
    };
    let array = |names: &[&str]| mesh.pointer(names).and_then(|p| file.bytes(mesh.owner, p));
    let f32_at = |d: &[u8], at: usize| d.get(at..at + 4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]));
    let i32_at = |d: &[u8], at: usize| d.get(at..at + 4).map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]));

    // the points: an array of vertices, or the attribute `position`
    let mut points = Vec::new();
    if let (Some(raw), Some((co, size))) = (array(&["mvert"]), field_of("MVert", "co")) {
        points = (0..points_n).filter_map(|k| Some(Vec3::new(f32_at(raw, k * size + co)?, f32_at(raw, k * size + co + 4)?, f32_at(raw, k * size + co + 8)?))).collect();
    } else if let Some(raw) = layer(mesh.part(&["vdata", "vert_data"]), 48, "position").or_else(|| attribute(mesh, "position")) {
        points = raw.chunks_exact(12).take(points_n).filter_map(|c| Some(Vec3::new(f32_at(c, 0)?, f32_at(c, 4)?, f32_at(c, 8)?))).collect();
    }
    // the corners: which point each is
    let mut corners: Vec<u32> = Vec::new();
    if let (Some(raw), Some((v, size))) = (array(&["mloop"]), field_of("MLoop", "v")) {
        corners = (0..corners_n).filter_map(|k| i32_at(raw, k * size + v).map(|v| v as u32)).collect();
    } else if let Some(raw) = layer(mesh.part(&["ldata", "loop_data", "corner_data"]), 11, ".corner_vert").or_else(|| attribute(mesh, ".corner_vert")) {
        corners = ints(raw).take(corners_n).map(|v| v as u32).collect();
    }
    // the faces: their first corner and how many, and their material
    let mut spans: Vec<(usize, usize, usize)> = Vec::new();
    if let (Some(raw), Some((start, size)), Some((total, _))) = (array(&["mpoly"]), field_of("MPoly", "loopstart"), field_of("MPoly", "totloop")) {
        let material = field_of("MPoly", "mat_nr").map(|(at, _)| at);
        spans = (0..faces_n)
            .filter_map(|k| {
                let m = material.and_then(|at| raw.get(k * size + at..k * size + at + 2)).map_or(0, |m| i16::from_le_bytes([m[0], m[1]]).max(0) as usize);
                Some((i32_at(raw, k * size + start)?.max(0) as usize, i32_at(raw, k * size + total)?.max(0) as usize, m))
            })
            .collect();
    } else if let Some(raw) = array(&["poly_offset_indices", "face_offset_indices"]) {
        let offsets: Vec<usize> = ints(raw).take(faces_n + 1).map(|o| o.max(0) as usize).collect();
        let materials: Vec<usize> = layer(mesh.part(&["pdata", "poly_data", "face_data"]), 11, "material_index").or_else(|| attribute(mesh, "material_index")).map(|raw| ints(raw).map(|m| m.max(0) as usize).collect()).unwrap_or_default();
        spans = offsets.windows(2).enumerate().map(|(k, w)| (w[0], w[1].saturating_sub(w[0]), materials.get(k).copied().unwrap_or(0))).collect();
    }
    let faces = spans
        .into_iter()
        .filter_map(|(start, total, material)| {
            let face = corners.get(start..start.checked_add(total)?)?;
            (face.len() >= 3 && face.iter().all(|v| (*v as usize) < points.len())).then(|| (face.to_vec(), material))
        })
        .collect();
    Mesh { points, faces }
}

/// The triangles of a face: a triangle or a quad as a fan, a larger face cut into ears on its
/// own plane.
fn triangles(points: &[Vec3], face: &[u32]) -> Vec<[u32; 3]> {
    if face.len() <= 4 {
        return (1..face.len() - 1).map(|k| [face[0], face[k], face[k + 1]]).collect();
    }
    let p: Vec<Vec3> = face.iter().map(|v| points[*v as usize]).collect();
    // Newell's normal, and the two axes it stands on
    let normal = (0..p.len()).fold(Vec3::ZERO, |n, k| n + p[k].cross(p[(k + 1) % p.len()]));
    let Some(normal) = normal.try_normalize() else { return Vec::new() };
    let u = normal.any_orthonormal_vector();
    let v = normal.cross(u);
    let flat: Vec<f64> = p.iter().flat_map(|q| [q.dot(u) as f64, q.dot(v) as f64]).collect();
    let cut = earcutr::earcut(&flat, &[], 2).unwrap_or_default();
    cut.chunks_exact(3).map(|t| [face[t[0]], face[t[1]], face[t[2]]]).collect()
}

/// The viewport colours of an object's materials: its mesh's, unless a slot is the object's own.
fn material_colors(object: View, mesh: View) -> Vec<Option<[f32; 3]>> {
    let file = object.file;
    let slots = mesh.int(&["totcol"]).filter(|n| *n > 0).map_or(0, |n| n as usize);
    let pointers = |of: View| -> Vec<u64> {
        let raw = of.pointer(&["mat"]).and_then(|p| file.bytes(of.owner, p)).unwrap_or(&[]);
        raw.chunks_exact(file.pointer).map(|c| if file.pointer == 8 { u64::from_le_bytes(c.try_into().unwrap_or([0; 8])) } else { u32::from_le_bytes([c[0], c[1], c[2], c[3]]) as u64 }).collect()
    };
    let (of_mesh, of_object) = (pointers(mesh), pointers(object));
    let own: &[u8] = object.pointer(&["matbits"]).and_then(|p| file.bytes(object.owner, p)).unwrap_or(&[]);
    (0..slots)
        .map(|k| {
            let address = if own.get(k).is_some_and(|b| *b != 0) { of_object.get(k) } else { of_mesh.get(k) };
            let material = file.view(file.target(object.owner, *address?)?)?;
            let c = |name: &str| material.floats(&[name]).and_then(|v| v.first().copied()).filter(|v| v.is_finite()).map(|v| v.clamp(0.0, 1.0));
            // linear to sRGB, as the viewport shows it
            let srgb = |v: f32| if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
            Some([srgb(c("r")?), srgb(c("g")?), srgb(c("b")?)])
        })
        .collect()
}

/// The mesh objects of a file as layers, in the order of the file.
fn layers(file: &File) -> Vec<Layer> {
    let mut out = Vec::new();
    // a mesh many objects share is read once
    let mut meshes: HashMap<usize, std::rc::Rc<Mesh>> = HashMap::new();
    for block in file.blocks.iter().filter(|b| &b.code[..2] == b"OB" && b.code[2] == 0) {
        let Some(object) = file.view(block) else { continue };
        if object.int(&["type"]) != Some(1) {
            continue;
        }
        let Some(data) = object.follow(&["data"]) else { continue };
        let mesh = meshes.entry(data.at).or_insert_with(|| std::rc::Rc::new(read_mesh(data))).clone();
        if mesh.faces.is_empty() {
            continue;
        }
        let colors = material_colors(object, data);
        let base = colors.iter().flatten().next().copied().unwrap_or(DEFAULT_COLOR);
        let mixed = colors.iter().flatten().any(|c| *c != base) || (colors.iter().any(Option::is_none) && colors.iter().any(Option::is_some));
        let to_world = world(object, 0);
        let points: Vec<Vec3> = mesh.points.iter().map(|p| to_world.transform_point3(*p)).collect();
        let name = object.name();
        let mut layer = Layer { name: name.clone(), color: base, ..Layer::default() };
        let mut vertex_colors = Vec::new();
        for (face, material) in &mesh.faces {
            let color = colors.get(*material).copied().flatten().unwrap_or(if colors.is_empty() { base } else { DEFAULT_COLOR });
            let rgba = [(color[0] * 255.0).round() as u8, (color[1] * 255.0).round() as u8, (color[2] * 255.0).round() as u8, 255];
            for t in triangles(&mesh.points, face) {
                let [a, b, c] = t.map(|v| points[v as usize]);
                let Some(normal) = (b - a).cross(c - a).try_normalize() else { continue };
                let first = (layer.positions.len() / 3) as u32;
                for p in [a, b, c] {
                    layer.positions.extend(p.to_array());
                    layer.normals.extend(normal.to_array());
                    vertex_colors.push(rgba);
                }
                layer.indices.extend([first, first + 1, first + 2]);
            }
        }
        if layer.indices.is_empty() {
            continue;
        }
        if mixed {
            layer.colors = Some(vertex_colors);
        }
        // the object, to pick: all of its layer
        let id = out.len() as u64 + 1;
        layer.elements = vec![(id, 0, layer.indices.len() as u32)];
        layer.props = vec![(id, serde_json::json!({"Name": name, "Type": "Mesh", "Mesh": data.name(), "Faces": mesh.faces.len()}).to_string())];
        // hidden in viewports (the screen in the outliner)
        layer.hidden = object.int(&["restrictflag", "visibility_flag"]).is_some_and(|f| f & 1 != 0);
        out.push(layer);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_no_blend_file_is_refused() {
        assert!(unpack(b"not a blend file at all").is_err());
        assert!(File::parse(b"BLENDER-v405").is_err());
        assert!(File::parse(b"BLENDER-V279").is_err_and(|e| e.contains("big-endian")));
    }

    #[test]
    fn a_face_of_many_corners_is_cut_on_its_own_plane() {
        // an L of six corners, standing upright
        let points: Vec<Vec3> = [[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [1.0, 1.0], [1.0, 2.0], [0.0, 2.0]].iter().map(|p| Vec3::new(p[0], 5.0, p[1])).collect();
        let cut = triangles(&points, &[0, 1, 2, 3, 4, 5]);
        assert_eq!(cut.len(), 4);
        let area: f32 = cut.iter().map(|t| (points[t[1] as usize] - points[t[0] as usize]).cross(points[t[2] as usize] - points[t[0] as usize]).length() / 2.0).sum();
        assert!((area - 3.0).abs() < 1e-4, "{area}");
        assert_eq!(triangles(&points, &[0, 1, 2, 3]).len(), 2);
    }

    /// Files of several Blender versions (`BLEND_FILES`: a folder of `.blend` files, read-only;
    /// skipped when absent), with `expected_worlds.txt` beside them when the folder has one: per
    /// line an object's name and its world matrix.
    #[test]
    fn files_of_every_version_give_their_mesh_objects() {
        let Some(folder) = std::env::var_os("BLEND_FILES").map(std::path::PathBuf::from).filter(|p| p.is_dir()) else {
            eprintln!("skipped: set BLEND_FILES to a folder of .blend files");
            return;
        };
        let expected: HashMap<String, Vec<f32>> = std::fs::read_to_string(folder.join("expected_worlds.txt"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.split_once('|'))
            .map(|(name, m)| (name.to_string(), m.split_whitespace().filter_map(|x| x.parse().ok()).collect()))
            .collect();
        let mut read = 0;
        for entry in std::fs::read_dir(&folder).unwrap().flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "blend") {
                continue;
            }
            let data = unpack(&std::fs::read(&path).unwrap()).unwrap_or_else(|e| panic!("{path:?}: {e}"));
            let file = File::parse(&data).unwrap_or_else(|e| panic!("{path:?}: {e}"));
            let layers = layers(&file);
            eprintln!("{:?}: {} blocks, {} mesh objects", path.file_name().unwrap(), file.blocks.len(), layers.len());
            for l in &layers {
                let (mut lo, mut hi) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
                for p in l.positions.chunks_exact(3) {
                    lo = lo.min(Vec3::from_slice(p));
                    hi = hi.max(Vec3::from_slice(p));
                }
                eprintln!("  {:?}: {} triangles, {lo:.2} to {hi:.2}, colour {:?}, mixed {}, hidden {}", l.name, l.triangle_count(), l.color, l.colors.is_some(), l.hidden);
            }
            assert!(!layers.is_empty(), "{path:?}");
            // every object stands where the file says, also where it stores no matrix
            for block in file.blocks.iter().filter(|b| &b.code[..2] == b"OB" && b.code[2] == 0) {
                let object = file.view(block).unwrap();
                let Some(want) = expected.get(&object.name()) else { continue };
                let got = world(object, 0).to_cols_array();
                assert!(got.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-4), "{path:?} {}: {got:?} is not {want:?}", object.name());
            }
            read += 1;
        }
        assert!(read > 0, "no .blend files in {folder:?}");
    }
}

