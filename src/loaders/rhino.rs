//! Rhino 3D models (`.3dm`), Rhino 4 to 8 (archive versions 4, 5, 50, 60, 70 and 80). The
//! reader is written from McNeel's openNURBS source, which McNeel publishes as the reference for
//! reading and writing 3dm files: the chunked archive (4- and 8-byte chunk lengths), the
//! properties, settings, material, layer, block definition and object tables, and of each object
//! its class data, its attributes and its user data. Nothing of openNURBS is compiled in.
//!
//! What is drawn: meshes; the render meshes Rhino stores with polysurfaces (Breps), surfaces and
//! extrusions (the analysis meshes when there are no render meshes); block instances with their
//! nested blocks and transforms; point clouds and points as point layers. One layer per Rhino
//! layer (its full path, `Parent::Child`), in the layer colour, hidden when the layer or one of
//! its parents is off; the members of a block instance on a layer that is off go to that hidden
//! layer. Every object is a pickable element with its name, object type, layer, id,
//! material and user text (attribute and geometry user strings); a block instance is one element.
//! Objects take their display colour by layer, object or parent (blocks), or their material's
//! diffuse colour and transparency when they have a material; black (Rhino's default layer
//! colour) without a material draws in the viewer's grey; Brep face colours (Rhino 7) and mesh
//! vertex colours are kept. Units are converted to metres; Rhino is Z up; a model far from zero
//! (survey coordinates) moves to a local origin, kept in the project information. Point clouds
//! are thinned out while drawing; blocks, meshes and points are drawn up to limits, so that a
//! damaged or hostile file cannot exhaust memory or run for ever. Curves, annotations,
//! hatches, lights, SubD objects (Rhino stores no mesh for them) and polysurfaces or extrusions
//! saved without a mesh ("save small") are left out and counted in the project information.
//! Hidden objects, objects on layouts and block definition geometry outside an instance are not
//! drawn.

use super::{Loaded, Loader, Phase, Progress, Source, DEFAULT_COLOR};
use crate::model::{Layer, ProjectData};
use glam::{DMat3, DMat4, DVec3};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;

pub struct RhinoLoader;

impl Loader for RhinoLoader {
    fn name(&self) -> &'static str {
        "Rhino"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["3dm"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|l| l.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let data = source.read()?;
        let file = read(&data)?;
        progress.set(Phase::Convert);
        let (layers, info) = convert(&file);
        if layers.iter().all(|l| l.indices.is_empty() && !(l.points && !l.positions.is_empty())) {
            // say why, e.g. polysurfaces saved without their render meshes
            let left_out = info.iter().find(|(k, _)| k == "Rhino objects left out").map_or("no objects", |(_, v)| v.as_str());
            return Err(format!("{}: nothing to draw in this Rhino file ({left_out})", source.name()));
        }
        Ok(Loaded { layers, views: Vec::new(), project: ProjectData { info, ..ProjectData::default() } })
    }
}

type R<T> = Result<T, String>;

fn err<T>(what: &str) -> R<T> {
    Err(format!("Rhino 3dm: {what}"))
}

// ---------------------------------------------------------------------------------------------
// Typecodes (openNURBS `opennurbs_3dm.h`).

const TCODE_SHORT: u32 = 0x8000_0000;
const TCODE_CRC: u32 = 0x8000;
const TCODE_ENDOFFILE: u32 = 0x0000_7FFF;
const TCODE_ENDOFTABLE: u32 = 0xFFFF_FFFF;
const TCODE_ANONYMOUS_CHUNK: u32 = 0x4000_8000;
const TCODE_MODEL_ATTRIBUTES_CHUNK: u32 = 0x4000_8002;
const TCODE_MATERIAL_TABLE: u32 = 0x1000_0010;
const TCODE_LAYER_TABLE: u32 = 0x1000_0011;
const TCODE_OBJECT_TABLE: u32 = 0x1000_0013;
const TCODE_PROPERTIES_TABLE: u32 = 0x1000_0014;
const TCODE_SETTINGS_TABLE: u32 = 0x1000_0015;
const TCODE_INSTANCE_DEFINITION_TABLE: u32 = 0x1000_0021;
const TCODE_PROPERTIES_APPLICATION: u32 = 0x2000_8024;
const TCODE_PROPERTIES_OPENNURBS_VERSION: u32 = 0xA000_0026;
const TCODE_SETTINGS_UNITSANDTOLS: u32 = 0x2000_8031;
const TCODE_MATERIAL_RECORD: u32 = 0x2000_8040;
const TCODE_LAYER_RECORD: u32 = 0x2000_8050;
const TCODE_OBJECT_RECORD: u32 = 0x2000_8070;
const TCODE_INSTANCE_DEFINITION_RECORD: u32 = 0x2000_8076;
const TCODE_OBJECT_RECORD_TYPE: u32 = 0x8200_0071;
const TCODE_OBJECT_RECORD_ATTRIBUTES: u32 = 0x0200_8072;
const TCODE_OBJECT_RECORD_ATTRIBUTES_USERDATA: u32 = 0x0200_0073;
const TCODE_OBJECT_RECORD_END: u32 = 0x8200_007F;
const TCODE_OPENNURBS_CLASS: u32 = 0x0002_7FFA;
const TCODE_OPENNURBS_CLASS_UUID: u32 = 0x0002_FFFB;
const TCODE_OPENNURBS_CLASS_DATA: u32 = 0x0002_FFFC;
const TCODE_OPENNURBS_CLASS_USERDATA: u32 = 0x0002_7FFD;
const TCODE_OPENNURBS_CLASS_USERDATA_HEADER: u32 = 0x0002_FFF9;
const TCODE_OPENNURBS_CLASS_END: u32 = 0x8002_7FFF;

/// A UUID as stored: `Data1` (u32), `Data2`, `Data3` (u16, little endian) and `Data4` (8 bytes).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
struct Uuid([u8; 16]);

impl Uuid {
    /// From the usual text form, `4ED7D4E4-E947-11d3-BFE5-0010830122F0`.
    const fn parse(s: &str) -> Uuid {
        let b = s.as_bytes();
        let mut hex = [0u8; 16];
        let (mut i, mut n) = (0, 0);
        while i < b.len() {
            let c = b[i];
            i += 1;
            let v = match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                b'A'..=b'F' => c - b'A' + 10,
                _ => continue,
            };
            hex[n / 2] |= if n % 2 == 0 { v << 4 } else { v };
            n += 1;
        }
        Uuid([hex[3], hex[2], hex[1], hex[0], hex[5], hex[4], hex[7], hex[6], hex[8], hex[9], hex[10], hex[11], hex[12], hex[13], hex[14], hex[15]])
    }

    fn is_nil(&self) -> bool {
        self.0 == [0; 16]
    }
}

impl std::fmt::Display for Uuid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let b = &self.0;
        write!(
            f,
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            b[3], b[2], b[1], b[0], b[5], b[4], b[7], b[6], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
        )
    }
}

impl std::fmt::Debug for Uuid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}

const ON_MESH: Uuid = Uuid::parse("4ED7D4E4-E947-11d3-BFE5-0010830122F0");
const ON_BREP: Uuid = Uuid::parse("60B5DBC5-E660-11d3-BFE4-0010830122F0");
/// The class ids Rhino writes Breps with (`TL_Brep`) and those of older files, read as `ON_Brep`.
const BREP_ALIASES: [Uuid; 3] = [
    Uuid::parse("F06FC243-A32A-4608-9DD8-A7D2C4CE2A36"),
    Uuid::parse("0705FDEF-3E2A-11d4-800E-0010830122F0"),
    Uuid::parse("2D4CFEDB-3E2A-11d4-800E-0010830122F0"),
];
const ON_EXTRUSION: Uuid = Uuid::parse("36F53175-72B8-4d47-BF1F-B4E6FC24F4B9");
const ON_SUBD: Uuid = Uuid::parse("F09BA4D9-455B-42C3-BA3B-E6CCACEF853B");
const ON_POINT_CLOUD: Uuid = Uuid::parse("2488F347-F8FA-11d3-BFEC-0010830122F0");
const ON_POINT: Uuid = Uuid::parse("C3101A1D-F157-11d3-BFE7-0010830122F0");
const ON_INSTANCE_REF: Uuid = Uuid::parse("F9CFB638-B9D4-4340-87E3-C56E7865D96A");
const ON_INSTANCE_DEFINITION: Uuid = Uuid::parse("26F8BFF6-2618-417f-A158-153D64A94989");
const ON_LAYER: Uuid = Uuid::parse("95809813-E985-11d3-BFE5-0010830122F0");
const ON_MATERIAL: Uuid = Uuid::parse("60B5DBBC-E660-11d3-BFE4-0010830122F0");
const ON_USER_STRING_LIST: Uuid = Uuid::parse("CE28DE29-F4C5-4faa-A50A-C3A6849B6329");
/// The render and analysis meshes of a Rhino 5 extrusion, kept as user data.
const ON_V5_EXTRUSION_MESH_CACHE: Uuid = Uuid::parse("A8130A3E-E4F3-4CB0-BB8A-F10A473912D0");
/// `ON_MeshCache::RenderMeshId`.
const RENDER_MESH_ID: Uuid = Uuid::parse("F5E3BAA9-A7A2-49FD-B8A1-66EB274A5F91");

// ---------------------------------------------------------------------------------------------
// The archive: little-endian values in nested chunks. A chunk is a typecode and a value; a
// "short" chunk (high bit of the typecode) has only the value, a long one is followed by value
// bytes of content, the last 4 a CRC when the typecode has the CRC bit. Lengths are 4 bytes
// before archive version 50 and 8 bytes from it.

#[derive(Clone, Copy, Debug, Default)]
struct Versions {
    /// 2, 3, 4, 5, 50, 60, 70, 80.
    archive: u32,
    /// The openNURBS version that wrote the file (`YYYYMMDDn` up to Rhino 5, larger after).
    opennurbs: u32,
}

#[derive(Clone)]
struct Cur<'a> {
    data: &'a [u8],
    pos: usize,
    v: Versions,
}

struct Chunk<'a> {
    typecode: u32,
    value: i64,
    body: Cur<'a>,
}

impl<'a> Cur<'a> {
    fn left(&self) -> usize {
        self.data.len() - self.pos
    }

    fn take(&mut self, n: usize) -> R<&'a [u8]> {
        if n > self.left() {
            return err("unexpected end of a record");
        }
        let out = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }

    fn array<const N: usize>(&mut self) -> R<[u8; N]> {
        Ok(self.take(N)?.try_into().unwrap())
    }

    fn u8(&mut self) -> R<u8> {
        Ok(self.take(1)?[0])
    }

    fn bool(&mut self) -> R<bool> {
        Ok(self.u8()? != 0)
    }

    fn i32(&mut self) -> R<i32> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    fn u32(&mut self) -> R<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn i64(&mut self) -> R<i64> {
        Ok(i64::from_le_bytes(self.array()?))
    }

    fn f64(&mut self) -> R<f64> {
        Ok(f64::from_le_bytes(self.array()?))
    }

    fn uuid(&mut self) -> R<Uuid> {
        Ok(Uuid(self.array()?))
    }

    /// `ON_Color`: red, green, blue and alpha bytes, alpha 0 opaque.
    fn color(&mut self) -> R<[u8; 4]> {
        self.array()
    }

    /// `ON_Xform`: 16 doubles, row by row, for column vectors.
    fn xform(&mut self) -> R<DMat4> {
        let mut m = [0.0; 16];
        for v in &mut m {
            *v = self.f64()?;
        }
        Ok(DMat4::from_cols_array(&m).transpose())
    }

    /// An element count, at most what the rest of the record can hold at `size` bytes each.
    fn count(&mut self, size: usize) -> R<usize> {
        let n = self.i32()?.max(0) as usize;
        if n.saturating_mul(size.max(1)) > self.left() {
            return err("a count larger than its record");
        }
        Ok(n)
    }

    /// `ON_wString`: the number of UTF-16 units (with the terminating zero), then the units.
    fn wstring(&mut self) -> R<String> {
        let n = self.u32()? as usize;
        if n == 0 {
            return Ok(String::new());
        }
        if n > self.left() / 2 {
            return err("a string longer than its record");
        }
        let units: Vec<u16> = self.take(2 * n)?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
        Ok(String::from_utf16_lossy(&units[..end]))
    }

    /// `Read3dmChunkVersion`: major and minor version in one byte.
    fn version(&mut self) -> R<(u8, u8)> {
        let v = self.u8()?;
        Ok((v >> 4, v & 15))
    }

    fn chunk(&mut self) -> R<Chunk<'a>> {
        let typecode = self.u32()?;
        let short = typecode & TCODE_SHORT != 0;
        let value = if self.v.archive >= 50 {
            self.i64()?
        } else if !short || typecode == TCODE_PROPERTIES_OPENNURBS_VERSION || typecode == TCODE_OBJECT_RECORD_TYPE {
            self.u32()? as i64
        } else {
            self.i32()? as i64
        };
        if short {
            return Ok(Chunk { typecode, value, body: Cur { data: &[], pos: 0, v: self.v } });
        }
        let length = usize::try_from(value).ok().filter(|&l| l <= self.left()).ok_or_else(|| format!("Rhino 3dm: a chunk (typecode {typecode:#x}) runs past its parent"))?;
        let bytes = self.take(length)?;
        let crc = if typecode & TCODE_CRC != 0 && length >= 4 { 4 } else { 0 };
        Ok(Chunk { typecode, value, body: Cur { data: &bytes[..length - crc], pos: 0, v: self.v } })
    }

    /// A chunk of the given typecode that starts with a major and minor version (two ints):
    /// `BeginRead3dmChunk(typecode, &major, &minor)`.
    fn versioned(&mut self, typecode: u32) -> R<(Cur<'a>, i32, i32)> {
        let chunk = self.chunk()?;
        if chunk.typecode != typecode {
            return err(&format!("expected chunk {typecode:#x}, found {:#x}", chunk.typecode));
        }
        let mut body = chunk.body;
        let major = body.i32()?;
        let minor = body.i32()?;
        Ok((body, major, minor))
    }

    /// The typecode of the next chunk, without reading it.
    fn peek_typecode(&self) -> Option<u32> {
        self.data.get(self.pos..self.pos + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    }

    /// `ReadCompressedBuffer`: the size, then a CRC, a method byte and the bytes, either stored
    /// (method 0) or zlib-deflated in an anonymous chunk (method 1). `None` when empty, and when
    /// the size is not `expected` (the bytes are then skipped without inflating them, so a small
    /// stream cannot claim gigabytes).
    fn compressed(&mut self, expected: usize) -> R<Option<Vec<u8>>> {
        let size = self.u32()? as usize;
        if size == 0 {
            return Ok(None);
        }
        let _crc = self.u32()?;
        let method = self.u8()?;
        let packed = match method {
            0 => self.take(size)?,
            1 => {
                let chunk = self.chunk()?;
                &chunk.body.data[chunk.body.pos..]
            }
            _ => return err("unknown compression method"),
        };
        if size != expected {
            return Ok(None);
        }
        if method == 0 {
            return Ok(Some(packed.to_vec()));
        }
        // deflate packs at most 1032 bytes into one: a larger size is a damaged record
        if size > packed.len().saturating_mul(1032).saturating_add(64) {
            return err("compressed data larger than its stream can hold");
        }
        let mut out = Vec::with_capacity(size.min(packed.len().saturating_mul(4)));
        flate2::read::ZlibDecoder::new(packed).take(size as u64).read_to_end(&mut out).map_err(|e| format!("Rhino 3dm: compressed mesh data: {e}"))?;
        if out.len() != size {
            return err("compressed data shorter than its size");
        }
        Ok(Some(out))
    }

    /// `ReadObject`: an openNURBS object of any class, with its user data; `None` for a null
    /// object.
    fn object(&mut self) -> R<Option<Object<'a>>> {
        let chunk = self.chunk()?;
        if chunk.typecode != TCODE_OPENNURBS_CLASS {
            return err("expected an object");
        }
        let mut body = chunk.body;
        let id = body.chunk()?;
        if id.typecode != TCODE_OPENNURBS_CLASS_UUID {
            return err("an object without its class id");
        }
        let class = id.body.clone().uuid()?;
        if class.is_nil() {
            return Ok(None);
        }
        let data = body.chunk()?;
        if data.typecode != TCODE_OPENNURBS_CLASS_DATA {
            return err("an object without its data");
        }
        let userdata = user_data(&mut body);
        Ok(Some(Object { class, data: data.body, userdata }))
    }
}

/// An object as read by `ReadObject`: its class id, its data and its user data by class id.
struct Object<'a> {
    class: Uuid,
    data: Cur<'a>,
    userdata: Vec<(Uuid, Cur<'a>)>,
}

/// User data entries up to `TCODE_OPENNURBS_CLASS_END` (`ReadObjectUserData`): each a version,
/// a header with the user data class id, and the data in an anonymous chunk. Damaged entries
/// are left out.
fn user_data<'a>(c: &mut Cur<'a>) -> Vec<(Uuid, Cur<'a>)> {
    let mut out = Vec::new();
    while c.left() > 0 {
        let Ok(chunk) = c.chunk() else { break };
        match chunk.typecode {
            TCODE_OPENNURBS_CLASS_END => break,
            TCODE_OPENNURBS_CLASS_USERDATA => {
                if let Ok(Some(entry)) = user_data_entry(chunk.body) {
                    out.push(entry);
                }
            }
            _ => {}
        }
    }
    out
}

fn user_data_entry(mut c: Cur<'_>) -> R<Option<(Uuid, Cur<'_>)>> {
    let (major, _) = c.version()?;
    let class = match major {
        2 => {
            let header = c.chunk()?;
            if header.typecode != TCODE_OPENNURBS_CLASS_USERDATA_HEADER {
                return Ok(None);
            }
            header.body.clone().uuid()?
        }
        1 => {
            let class = c.uuid()?;
            // item id, copy count, transform
            c.take(16 + 4 + 128)?;
            class
        }
        _ => return Ok(None),
    };
    let data = c.chunk()?;
    if data.typecode != TCODE_ANONYMOUS_CHUNK {
        return Ok(None);
    }
    Ok(Some((class, data.body)))
}

/// `ON_UserStringList`: key and value pairs ("user text").
fn user_strings(mut c: Cur<'_>) -> R<Vec<(String, String)>> {
    let (mut list, major, _) = c.versioned(TCODE_ANONYMOUS_CHUNK)?;
    if major != 1 {
        return Ok(Vec::new());
    }
    // every entry is a chunk of at least 16 bytes: typecode, length, major and minor version
    let count = list.count(16)?;
    let mut out = Vec::with_capacity(count.min(1024));
    for _ in 0..count {
        let (mut entry, major, _) = list.versioned(TCODE_ANONYMOUS_CHUNK)?;
        if major == 1 {
            let key = entry.wstring()?;
            let value = entry.wstring()?;
            if !key.is_empty() {
                out.push((key, value));
            }
        }
    }
    Ok(out)
}

fn user_strings_of(userdata: &[(Uuid, Cur<'_>)]) -> Vec<(String, String)> {
    userdata.iter().filter(|(class, _)| *class == ON_USER_STRING_LIST).flat_map(|(_, c)| user_strings(c.clone()).unwrap_or_default()).collect()
}

// ---------------------------------------------------------------------------------------------
// The file: tables of layers, materials, block definitions and objects.

#[derive(Debug, Clone)]
struct RhinoLayer {
    /// The index objects refer to.
    index: i32,
    name: String,
    id: Uuid,
    parent: Uuid,
    color: [u8; 4],
    visible: bool,
    material: i32,
}

#[derive(Debug, Clone)]
struct Material {
    index: i32,
    name: String,
    diffuse: [u8; 4],
    transparency: f64,
}

#[derive(Debug, Clone)]
struct BlockDefinition {
    id: Uuid,
    name: String,
    objects: Vec<Uuid>,
}

/// `ON_3dmObjectAttributes`, the parts the viewer uses.
#[derive(Debug, Clone)]
struct Attributes {
    id: Uuid,
    name: String,
    layer: i32,
    material: i32,
    color: [u8; 4],
    /// 0 by layer, 1 by object, 2 by material, 3 by parent.
    color_source: u8,
    /// 0 by layer, 1 by object, 3 by parent.
    material_source: u8,
    visible: bool,
    /// 0 normal, 1 hidden, 2 locked, 3 block definition geometry.
    mode: u8,
    /// 1 model space, 2 page (layout) space.
    space: u8,
}

impl Default for Attributes {
    fn default() -> Self {
        Attributes { id: Uuid::default(), name: String::new(), layer: 0, material: -1, color: [0, 0, 0, 0], color_source: 0, material_source: 0, visible: true, mode: 0, space: 1 }
    }
}

/// A triangle mesh as stored in an `ON_Mesh`: float vertices, optional normals and colours,
/// faces of four indices (a triangle repeats its third).
#[derive(Debug, Clone, Default)]
struct Mesh {
    positions: Vec<[f32; 3]>,
    normals: Option<Vec<[f32; 3]>>,
    colors: Option<Vec<[u8; 4]>>,
    faces: Vec<[u32; 4]>,
    /// The triangles of the faces (a quad is two).
    triangles: usize,
    /// The colour of the Brep face this mesh belongs to (Rhino 7 and later), over the object's.
    color: Option<[u8; 4]>,
}

#[derive(Debug, Clone)]
enum Geometry<'a> {
    /// Meshes and the object type to show ("Mesh", "Polysurface", ..).
    Meshes(Vec<Mesh>, &'static str),
    Instance { definition: Uuid, xform: DMat4 },
    /// Points as stored, three doubles (24 bytes) each, and their colours (4 bytes each): read
    /// only while drawing, so that a large cloud is thinned out without a copy of every point.
    Points { points: &'a [u8], colors: Option<&'a [u8]>, kind: &'static str },
    /// Not drawn: why, for the count in the project information.
    Skipped(&'static str),
}

#[derive(Debug, Clone)]
struct RhinoObject<'a> {
    geometry: Geometry<'a>,
    attributes: Attributes,
    user: Vec<(String, String)>,
}

#[derive(Debug, Default)]
struct File<'a> {
    versions: Versions,
    application: String,
    /// Metres per model unit, and the unit's name.
    meters: f64,
    unit: &'static str,
    layers: Vec<RhinoLayer>,
    materials: Vec<Material>,
    blocks: Vec<BlockDefinition>,
    objects: Vec<RhinoObject<'a>>,
    /// Records that could not be read, by table.
    damaged: BTreeMap<&'static str, usize>,
    /// Mesh vertices read so far, at most [`MAX_VERTICES`].
    vertices: usize,
}

const MAGIC: &[u8] = b"3D Geometry File Format ";

fn read(data: &[u8]) -> R<File<'_>> {
    // The header may follow some bytes of other data (OLE wrappers of old files).
    let start = data.windows(MAGIC.len()).take(1 << 20).position(|w| w == MAGIC).ok_or("Rhino 3dm: not a Rhino file (no 3dm header)")?;
    let header = data.get(start..start + 32).ok_or("Rhino 3dm: the file is too short")?;
    let version: u32 = std::str::from_utf8(&header[24..32]).ok().and_then(|s| s.trim().parse().ok()).ok_or("Rhino 3dm: the header has no version")?;
    if version < 2 {
        return err("Rhino 1 files are not supported (save them from a newer Rhino)");
    }
    let mut file = File { versions: Versions { archive: version, opennurbs: 0 }, meters: 0.001, unit: "millimetres", ..File::default() };
    let mut top = Cur { data, pos: start + 32, v: file.versions };
    while top.left() > 0 {
        let chunk = match top.chunk() {
            Ok(c) => c,
            // a damaged tail: keep what was read
            Err(e) if !file.objects.is_empty() => {
                *file.damaged.entry("end of file").or_default() += 1;
                let _ = e;
                break;
            }
            Err(e) => return Err(e),
        };
        let mut body = chunk.body;
        body.v = file.versions;
        match chunk.typecode {
            TCODE_ENDOFFILE => break,
            TCODE_PROPERTIES_TABLE => read_properties(body, &mut file),
            TCODE_SETTINGS_TABLE => read_settings(body, &mut file),
            TCODE_MATERIAL_TABLE => table(body, TCODE_MATERIAL_RECORD, &mut file, "materials", |mut c, f| {
                let index = f.materials.len() as i32;
                if let Some(object) = c.object()?.filter(|o| o.class == ON_MATERIAL) {
                    f.materials.push(read_material(object.data, index)?);
                }
                Ok(())
            }),
            TCODE_LAYER_TABLE => table(body, TCODE_LAYER_RECORD, &mut file, "layers", |mut c, f| {
                let index = f.layers.len() as i32;
                if let Some(object) = c.object()?.filter(|o| o.class == ON_LAYER) {
                    f.layers.push(read_layer(object.data, index)?);
                }
                Ok(())
            }),
            TCODE_INSTANCE_DEFINITION_TABLE => table(body, TCODE_INSTANCE_DEFINITION_RECORD, &mut file, "block definitions", |mut c, f| {
                if let Some(object) = c.object()?.filter(|o| o.class == ON_INSTANCE_DEFINITION) {
                    f.blocks.push(read_block_definition(object.data)?);
                }
                Ok(())
            }),
            TCODE_OBJECT_TABLE => table(body, TCODE_OBJECT_RECORD, &mut file, "objects", |c, f| {
                let object = read_object(c, &mut f.vertices)?;
                f.objects.push(object);
                Ok(())
            }),
            _ => {}
        }
    }
    Ok(file)
}

/// The records of a table, up to its end; a damaged record is counted and skipped.
fn table<'a>(mut c: Cur<'a>, record: u32, file: &mut File<'a>, name: &'static str, mut each: impl FnMut(Cur<'a>, &mut File<'a>) -> R<()>) {
    while c.left() > 0 {
        let Ok(chunk) = c.chunk() else {
            *file.damaged.entry(name).or_default() += 1;
            break;
        };
        if chunk.typecode == TCODE_ENDOFTABLE {
            break;
        }
        if chunk.typecode == record && each(chunk.body, file).is_err() {
            *file.damaged.entry(name).or_default() += 1;
        }
    }
}

fn read_properties(mut c: Cur<'_>, file: &mut File<'_>) {
    while let Ok(chunk) = c.chunk() {
        match chunk.typecode {
            TCODE_ENDOFTABLE => break,
            TCODE_PROPERTIES_OPENNURBS_VERSION => file.versions.opennurbs = chunk.value as u32,
            TCODE_PROPERTIES_APPLICATION => {
                let mut b = chunk.body;
                if b.version().is_ok() {
                    file.application = b.wstring().unwrap_or_default();
                }
            }
            _ => {}
        }
    }
}

fn read_settings(mut c: Cur<'_>, file: &mut File<'_>) {
    while let Ok(chunk) = c.chunk() {
        match chunk.typecode {
            TCODE_ENDOFTABLE => break,
            TCODE_SETTINGS_UNITSANDTOLS => {
                if let Ok(Some((meters, unit))) = read_units(chunk.body) {
                    file.meters = meters;
                    file.unit = unit;
                }
            }
            _ => {}
        }
    }
}

/// `ON_3dmUnitsAndTolerances`: the length unit, as metres per unit and its name.
fn read_units(mut c: Cur<'_>) -> R<Option<(f64, &'static str)>> {
    let version = c.i32()?;
    if !(100..200).contains(&version) {
        return Ok(None);
    }
    let unit = c.u32()?;
    c.take(24)?; // absolute, angle and relative tolerance
    let mut custom = None;
    if version >= 102 {
        c.take(8)?; // distance display mode and precision
        custom = Some(c.f64()?);
    }
    Ok(unit_scale(unit, custom))
}

/// Metres per unit of `ON::LengthUnitSystem`.
fn unit_scale(unit: u32, custom: Option<f64>) -> Option<(f64, &'static str)> {
    Some(match unit {
        0 => (1.0, "none (taken as metres)"),
        1 => (1e-6, "microns"),
        2 => (1e-3, "millimetres"),
        3 => (1e-2, "centimetres"),
        4 => (1.0, "metres"),
        5 => (1e3, "kilometres"),
        6 => (2.54e-8, "microinches"),
        7 => (2.54e-5, "mils"),
        8 => (0.0254, "inches"),
        9 => (0.3048, "feet"),
        10 => (1609.344, "miles"),
        12 => (1e-10, "ångströms"),
        13 => (1e-9, "nanometres"),
        14 => (0.1, "decimetres"),
        15 => (10.0, "dekametres"),
        16 => (100.0, "hectometres"),
        17 => (1e6, "megametres"),
        18 => (1e9, "gigametres"),
        19 => (0.9144, "yards"),
        20 => (0.0254 / 72.0, "printer points"),
        21 => (0.0254 / 6.0, "printer picas"),
        22 => (1852.0, "nautical miles"),
        23 => (1.495_978_707e11, "astronomical units"),
        24 => (9.460_730_472_580_8e15, "light years"),
        25 => (3.085_677_581_49e16, "parsecs"),
        11 => (custom.filter(|m| m.is_finite() && *m > 0.0)?, "custom units"),
        _ => return None,
    })
}

/// `ON_Layer` (chunk version 1.x, all Rhino versions).
fn read_layer(mut c: Cur<'_>, index: i32) -> R<RhinoLayer> {
    let (major, minor) = c.version()?;
    if major != 1 {
        return err("a layer of an unknown version");
    }
    let mode = c.i32()?;
    let stored = c.i32()?;
    c.i32()?; // IGES level
    let material = c.i32()?;
    c.i32()?;
    let color = c.color()?;
    c.take(20)?; // obsolete line style
    let name = c.wstring()?;
    let mut layer = RhinoLayer { index: if stored >= 0 { stored } else { index }, name, id: Uuid::default(), parent: Uuid::default(), color, visible: mode != 1, material, };
    if minor >= 1 {
        layer.visible = c.bool()? && mode != 1;
    }
    if minor >= 2 {
        c.i32()?; // line pattern
    }
    if minor >= 3 {
        c.take(12)?; // plot colour and weight
    }
    if minor >= 4 {
        c.bool()?; // locked
    }
    if minor >= 5 {
        layer.id = c.uuid()?;
        if minor >= 6 && c.v.opennurbs > 200_505_110 {
            layer.parent = c.uuid()?;
        }
    }
    Ok(layer)
}

/// `ON_Material`: name, diffuse colour and transparency.
fn read_material(mut c: Cur<'_>, index: i32) -> R<Material> {
    let mut m = Material { index, name: String::new(), diffuse: [255, 255, 255, 0], transparency: 0.0 };
    if c.v.archive >= 60 && c.v.opennurbs >= 2_348_833_910 {
        // Rhino 6 and later: component attributes, then the colours
        let (mut b, major, _) = c.versioned(TCODE_ANONYMOUS_CHUNK)?;
        if major != 1 {
            return err("a material of an unknown version");
        }
        let (mut attributes, _, _) = b.versioned(TCODE_ANONYMOUS_CHUNK)?;
        let bits = attributes.u32()?;
        if bits & 1 != 0 {
            attributes.uuid()?;
        }
        if bits & 2 != 0 {
            attributes.uuid()?;
        }
        if bits & 4 != 0 {
            m.index = attributes.i32()?;
        }
        if bits & 8 != 0 {
            m.name = attributes.wstring()?;
        }
        b.uuid()?; // plug-in
        b.color()?; // ambient
        m.diffuse = b.color()?;
        b.take(16)?; // emission, specular, reflection, transparent
        b.take(24)?; // index of refraction, reflectivity, shine
        m.transparency = b.f64()?;
        return Ok(m);
    }
    let (major, _) = c.version()?;
    match major {
        1 => {
            // Rhino 3
            c.color()?;
            m.diffuse = c.color()?;
            c.take(16)?; // emission, specular, shine
            m.transparency = c.f64()?;
        }
        2 => {
            // Rhino 4 and 5
            let (mut b, _, _) = c.versioned(TCODE_ANONYMOUS_CHUNK)?;
            b.uuid()?;
            m.index = b.i32()?;
            m.name = b.wstring()?;
            b.uuid()?;
            b.color()?;
            m.diffuse = b.color()?;
            b.take(16)?;
            b.take(24)?;
            m.transparency = b.f64()?;
        }
        _ => return err("a material of an unknown version"),
    }
    Ok(m)
}

/// `ON_InstanceDefinition`: id, name and the ids of its objects.
fn read_block_definition(mut c: Cur<'_>) -> R<BlockDefinition> {
    let v6 = match c.v.archive {
        0..=50 => false,
        60 => c.v.opennurbs > 2_348_834_153 && c.peek_typecode() == Some(TCODE_ANONYMOUS_CHUNK),
        _ => true,
    };
    if !v6 {
        let (major, _) = c.version()?;
        if major != 1 {
            return err("a block definition of an unknown version");
        }
        let id = c.uuid()?;
        let count = c.count(16)?;
        let objects = (0..count).map(|_| c.uuid()).collect::<R<Vec<_>>>()?;
        let name = c.wstring()?;
        return Ok(BlockDefinition { id, name, objects });
    }
    let (mut b, major, _) = c.versioned(TCODE_ANONYMOUS_CHUNK)?;
    if major != 1 {
        return err("a block definition of an unknown version");
    }
    let (mut a, _, _) = b.versioned(TCODE_MODEL_ATTRIBUTES_CHUNK)?;
    let mut definition = BlockDefinition { id: Uuid::default(), name: String::new(), objects: Vec::new() };
    if a.u8()? == 1 {
        a.take(12)?; // serial numbers
    }
    if a.u8()? == 1 {
        definition.id = a.uuid()?;
    }
    if a.u8()? == 1 {
        a.i32()?; // component type
    }
    if a.u8()? == 1 {
        a.i32()?; // index
    }
    if a.u8()? == 1 {
        definition.name = a.wstring()?;
    }
    b.i32()?; // definition type (static, linked, ..)
    b.chunk()?; // unit system
    for _ in 0..3 {
        b.wstring()?; // description, url, url tag
    }
    b.take(48)?; // bounding box
    if b.bool()? {
        let count = b.count(16)?;
        definition.objects = (0..count).map(|_| b.uuid()).collect::<R<Vec<_>>>()?;
    }
    Ok(definition)
}

/// One record of the object table: the object type, the object, its attributes and the user
/// data of the attributes. `vertices` counts the mesh vertices read from the file so far.
fn read_object<'a>(mut c: Cur<'a>, vertices: &mut usize) -> R<RhinoObject<'a>> {
    let kind = c.chunk()?;
    if kind.typecode != TCODE_OBJECT_RECORD_TYPE {
        return err("an object record without its type");
    }
    let object_type = kind.value as u32;
    let object = c.object()?;
    let mut attributes = Attributes::default();
    let mut user = Vec::new();
    while c.left() > 0 {
        let chunk = c.chunk()?;
        match chunk.typecode {
            TCODE_OBJECT_RECORD_ATTRIBUTES => attributes = read_attributes(chunk.body)?,
            TCODE_OBJECT_RECORD_ATTRIBUTES_USERDATA => {
                let mut body = chunk.body;
                user.extend(user_strings_of(&user_data(&mut body)));
            }
            TCODE_OBJECT_RECORD_END => break,
            _ => {}
        }
    }
    let geometry = match object {
        Some(object) => {
            user.extend(user_strings_of(&object.userdata));
            match read_geometry(object, object_type, vertices) {
                Ok(geometry) => geometry,
                Err(e) if e == TOO_MANY_VERTICES => Geometry::Skipped("meshes beyond the vertex limit"),
                Err(_) => Geometry::Skipped("unreadable objects"),
            }
        }
        None => Geometry::Skipped("empty objects"),
    };
    Ok(RhinoObject { geometry, attributes, user })
}

/// `ON_3dmObjectAttributes`: the item list of Rhino 5 and later, or the fixed layout before.
fn read_attributes(mut c: Cur<'_>) -> R<Attributes> {
    let mut a = Attributes::default();
    if c.v.archive >= 5 && c.v.opennurbs >= 200_712_190 {
        let (major, _) = c.version()?;
        if major != 2 {
            return err("object attributes of an unknown version");
        }
        a.id = c.uuid()?;
        a.layer = c.i32()?;
        // items in increasing order, 0 ends the list; after the space (19) nothing is needed
        loop {
            let item = c.u8()?;
            match item {
                1 => a.name = c.wstring()?,
                2 => {
                    c.wstring()?; // url
                }
                3 => {
                    c.i32()?; // line type
                }
                4 => a.material = c.i32()?,
                5 => {
                    c.chunk()?; // rendering attributes
                }
                6 => a.color = c.color()?,
                7 => {
                    c.color()?; // plot colour
                }
                8 => {
                    c.f64()?; // plot weight
                }
                9 => {
                    c.u8()?; // decoration
                }
                10 => {
                    c.i32()?; // wire density
                }
                11 => a.visible = c.bool()?,
                12 => a.mode = c.u8()? % 16,
                13 => a.color_source = c.u8()?,
                14 | 15 | 17 => {
                    c.u8()?; // plot colour, plot weight and line type source
                }
                16 => a.material_source = c.u8()?,
                18 => {
                    let n = c.count(4)?;
                    c.take(4 * n)?; // groups
                }
                19 => a.space = c.u8()?,
                _ => break,
            }
        }
    } else {
        let (major, minor) = c.version()?;
        if major != 1 {
            return err("object attributes of an unknown version");
        }
        a.id = c.uuid()?;
        a.layer = c.i32()?;
        a.material = c.i32()?;
        a.color = c.color()?;
        c.take(20)?; // obsolete line style
        c.i32()?; // wire density
        a.mode = c.u8()? % 16;
        a.color_source = c.u8()?;
        c.u8()?; // line type source
        a.material_source = c.u8()?;
        a.name = c.wstring()?;
        c.wstring()?; // url
        a.visible = a.mode != 1;
        if minor >= 1 {
            let n = c.count(4)?;
            c.take(4 * n)?;
            if minor >= 2 {
                a.visible = c.bool()?;
            }
            if minor >= 6 {
                // display material references (1.3), decoration and plot colour and weight (1.4),
                // line type (1.5), then the space (1.6): 1 for a layout. A short record keeps
                // model space.
                let mut space = || -> R<u8> {
                    let n = c.count(32)?;
                    c.take(32 * n)?;
                    c.take(4 + 1 + 4 + 1 + 8 + 4)?;
                    c.u8()
                };
                if matches!(space(), Ok(1)) {
                    a.space = 2;
                }
            }
        }
    }
    Ok(a)
}

fn read_geometry<'a>(object: Object<'a>, object_type: u32, vertices: &mut usize) -> R<Geometry<'a>> {
    let mut c = object.data;
    let class = object.class;
    if class == ON_MESH {
        return Ok(Geometry::Meshes(vec![read_mesh(&mut c, vertices)?], "Mesh"));
    }
    if class == ON_BREP || BREP_ALIASES.contains(&class) {
        let (meshes, faces) = read_brep(&mut c, vertices)?;
        let kind = if faces == 1 { "Surface" } else { "Polysurface" };
        return Ok(if meshes.is_empty() { Geometry::Skipped("polysurfaces without a render mesh") } else { Geometry::Meshes(meshes, kind) });
    }
    if class == ON_EXTRUSION {
        let mut meshes = read_extrusion(&mut c, vertices)?;
        if meshes.is_empty() {
            // Rhino 5: the meshes are user data of the extrusion
            if let Some((_, data)) = object.userdata.iter().find(|(id, _)| *id == ON_V5_EXTRUSION_MESH_CACHE) {
                let mut data = data.clone();
                for _ in 0..2 {
                    match data.object() {
                        Ok(Some(o)) if o.class == ON_MESH => {
                            let mut d = o.data;
                            meshes.push(read_mesh(&mut d, vertices)?);
                            break;
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
            }
        }
        return Ok(if meshes.is_empty() { Geometry::Skipped("extrusions without a render mesh") } else { Geometry::Meshes(meshes, "Extrusion") });
    }
    if class == ON_INSTANCE_REF {
        let (major, _) = c.version()?;
        if major != 1 {
            return err("a block instance of an unknown version");
        }
        let definition = c.uuid()?;
        let xform = c.xform()?;
        return Ok(Geometry::Instance { definition, xform });
    }
    if class == ON_POINT_CLOUD {
        let (major, minor) = c.version()?;
        if major != 1 {
            return err("a point cloud of an unknown version");
        }
        let count = c.count(24)?;
        let points = c.take(24 * count)?;
        c.take(16 * 8 + 48 + 4)?; // plane, bounding box, flags
        let mut colors = None;
        if minor >= 1 {
            let normals = c.count(24)?;
            c.take(24 * normals)?;
            let n = c.count(4)?;
            let list = c.take(4 * n)?;
            colors = (n == count).then_some(list);
        }
        return Ok(Geometry::Points { points, colors, kind: "Point cloud" });
    }
    if class == ON_POINT {
        c.version()?;
        return Ok(Geometry::Points { points: c.take(24)?, colors: None, kind: "Point" });
    }
    if class == ON_SUBD {
        return Ok(Geometry::Skipped("SubD objects (no mesh in the file)"));
    }
    Ok(Geometry::Skipped(match object_type {
        1 | 2 => "points",
        4 => "curves",
        8 => "surfaces without a mesh",
        0x200 => "annotations",
        0x2000 => "text dots",
        0x8000 => "details",
        0x10000 => "hatches",
        0x100 => "lights",
        0x2000_0000 => "clipping planes",
        0x40000 => "SubD objects (no mesh in the file)",
        _ => "other objects",
    }))
}

/// At most this many mesh vertices are read from a file: compressed arrays could otherwise
/// inflate far beyond the size of the file. Meshes beyond are left out and counted.
const MAX_VERTICES: usize = MAX_DRAWN_VERTICES;
const TOO_MANY_VERTICES: &str = "Rhino 3dm: more mesh vertices than the viewer reads";

/// `ON_Mesh` (chunk version 1.x uncompressed, 3.x compressed): vertices, normals, colours and
/// faces. Texture coordinates, curvatures, n-gons and double precision vertices are not read.
/// `vertices` counts the vertices read from the file so far; a mesh beyond [`MAX_VERTICES`] is
/// not read. A mesh without faces draws nothing and is not read either.
fn read_mesh(c: &mut Cur<'_>, vertices: &mut usize) -> R<Mesh> {
    let (major, _) = c.version()?;
    if major != 1 && major != 3 {
        return err("a mesh of an unknown version");
    }
    let vcount = c.i32()?.max(0) as usize;
    let fcount = c.i32()?.max(0) as usize;
    if fcount == 0 {
        return Ok(Mesh::default());
    }
    if vcount > MAX_VERTICES - *vertices {
        return Err(TOO_MANY_VERTICES.into());
    }
    *vertices += vcount;
    // texture and surface domains, surface scale, vertex, normal and texture boxes, closed
    c.take(64 + 16 + 24 + 24 + 16 + 4)?;
    if c.bool()? {
        c.chunk()?; // mesh parameters
    }
    for _ in 0..4 {
        if c.bool()? {
            c.chunk()?; // curvature statistics
        }
    }
    let size = c.i32()?;
    if !matches!(size, 1 | 2 | 4) {
        return err("mesh faces of an unknown index size");
    }
    let bytes = c.take(fcount.checked_mul(4 * size as usize).ok_or("Rhino 3dm: too many mesh faces")?)?;
    let faces: Vec<[u32; 4]> = match size {
        1 => bytes.chunks_exact(4).map(|f| [f[0] as u32, f[1] as u32, f[2] as u32, f[3] as u32]).collect(),
        2 => bytes.chunks_exact(8).map(|f| std::array::from_fn(|k| u16::from_le_bytes([f[2 * k], f[2 * k + 1]]) as u32)).collect(),
        _ => bytes.chunks_exact(16).map(|f| std::array::from_fn(|k| u32::from_le_bytes(f[4 * k..4 * k + 4].try_into().unwrap()))).collect(),
    };
    let floats3 = |b: &[u8]| -> Vec<[f32; 3]> { b.chunks_exact(12).map(|p| std::array::from_fn(|k| f32::from_le_bytes(p[4 * k..4 * k + 4].try_into().unwrap()))).collect() };
    let colors4 = |b: &[u8]| -> Vec<[u8; 4]> { b.chunks_exact(4).map(|p| [p[0], p[1], p[2], p[3]]).collect() };
    let triangles = faces.iter().map(|f| if f[2] == f[3] { 1 } else { 2 }).sum();
    let mut mesh = Mesh { faces, triangles, ..Mesh::default() };
    if major == 1 {
        let n = c.count(12)?;
        mesh.positions = floats3(c.take(12 * n)?);
        let n = c.count(12)?;
        mesh.normals = Some(floats3(c.take(12 * n)?));
        let n = c.count(8)?;
        c.take(8 * n)?;
        let n = c.count(16)?;
        c.take(16 * n)?;
        let n = c.count(4)?;
        mesh.colors = Some(colors4(c.take(4 * n)?));
    } else if vcount > 0 {
        mesh.positions = floats3(&c.compressed(12 * vcount)?.unwrap_or_default());
        mesh.normals = c.compressed(12 * vcount)?.map(|b| floats3(&b));
        // texture coordinates and curvatures, skipped without inflating them
        c.compressed(0)?;
        c.compressed(0)?;
        mesh.colors = c.compressed(4 * vcount)?.map(|b| colors4(&b));
    }
    let n = mesh.positions.len();
    if n != vcount {
        return err("mesh vertices of the wrong size");
    }
    mesh.normals = mesh.normals.filter(|v| v.len() == n);
    mesh.colors = mesh.colors.filter(|v| v.len() == n);
    Ok(mesh)
}

/// `ON_Brep` (chunk version 3.x): the curves, surfaces and topology are skipped as chunks; the
/// face count, then the render meshes (or else the analysis meshes) of the faces. Returns the
/// meshes and the face count.
fn read_brep(c: &mut Cur<'_>, vertices: &mut usize) -> R<(Vec<Mesh>, usize)> {
    let (major, minor) = c.version()?;
    if major != 3 {
        // Rhino 1 and 2 trimmed surfaces
        return Ok((Vec::new(), 0));
    }
    // 2d curves, 3d curves, surfaces, vertices, edges, trims, loops
    for _ in 0..7 {
        c.chunk()?;
    }
    let faces = c.chunk()?.body;
    let face_count = {
        let mut f = faces.clone();
        f.version()?;
        f.i32()?.max(0) as usize
    };
    let face_colors = brep_face_colors(faces).unwrap_or_default();
    c.take(48)?; // bounding box
    if minor < 1 {
        return Ok((Vec::new(), face_count));
    }
    let mut out = Vec::new();
    for _ in 0..2 {
        let mut list = c.chunk()?.body;
        for face in 0..face_count {
            if list.left() == 0 {
                break;
            }
            if list.bool()? {
                if let Some(object) = list.object()?.filter(|o| o.class == ON_MESH) {
                    let mut d = object.data;
                    let mut mesh = read_mesh(&mut d, vertices)?;
                    mesh.color = face_colors.get(face).copied().flatten();
                    out.push(mesh);
                }
            }
        }
        // the analysis meshes only when there are no render meshes
        if !out.is_empty() || c.left() == 0 {
            break;
        }
    }
    Ok((out, face_count))
}

/// The per-face colours of a Brep's face array (chunk version 1.2, Rhino 7 and later); `None`
/// for a face without one.
fn brep_face_colors(mut c: Cur<'_>) -> R<Vec<Option<[u8; 4]>>> {
    let (_, minor) = c.version()?;
    let count = c.count(20)?;
    for _ in 0..count {
        c.i32()?; // index
        let loops = c.count(4)?;
        c.take(4 * loops)?;
        c.take(12)?; // surface, reversed, material channel
    }
    if minor < 2 {
        return Ok(Vec::new());
    }
    c.take(16 * count)?; // face ids
    if !c.bool()? {
        return Ok(Vec::new());
    }
    (0..count).map(|_| c.color().map(|color| (color != [255; 4]).then_some(color))).collect()
}

/// `ON_Extrusion`: the profile and path are skipped; from chunk version 1.3 (Rhino 6) the mesh
/// cache, of which the render mesh (or else the first) is taken.
fn read_extrusion(c: &mut Cur<'_>, vertices: &mut usize) -> R<Vec<Mesh>> {
    let (mut b, major, minor) = c.versioned(TCODE_ANONYMOUS_CHUNK)?;
    if major != 1 {
        return err("an extrusion of an unknown version");
    }
    b.object()?; // profile
    b.take(48 + 16 + 24 + 2 + 48 + 16 + 1)?; // path, domain, up, normals, path domain, transposed
    if minor < 3 {
        return Ok(Vec::new());
    }
    b.i32()?; // profile count
    b.take(2)?; // caps
    let (mut cache, _, _) = b.versioned(TCODE_ANONYMOUS_CHUNK)?;
    let mut meshes: Vec<(Uuid, Mesh)> = Vec::new();
    while cache.u8()? == 1 {
        let (mut item, _, _) = cache.versioned(TCODE_ANONYMOUS_CHUNK)?;
        let id = item.uuid()?;
        if let Some(object) = item.object()?.filter(|o| o.class == ON_MESH) {
            let mut d = object.data;
            meshes.push((id, read_mesh(&mut d, vertices)?));
        }
    }
    let k = meshes.iter().position(|(id, _)| *id == RENDER_MESH_ID).unwrap_or(0);
    Ok(if meshes.is_empty() { Vec::new() } else { vec![meshes.swap_remove(k).1] })
}

// ---------------------------------------------------------------------------------------------
// From the file to layers: one layer per Rhino layer, objects and block instances as elements.

/// Block instances nested deeper than this are left out.
const MAX_BLOCK_DEPTH: usize = 32;

/// At most this many objects are drawn through block instances (every member of every placed
/// instance counts): nested blocks multiply, and a damaged or hostile file could otherwise keep
/// them going for ever. Smaller in the (unoptimised) tests, which run into it.
const MAX_BLOCK_PARTS: usize = if cfg!(test) { 100_000 } else { 10_000_000 };

/// At most this many triangles are drawn, block instances included, and at most three vertices
/// per triangle, so that indices stay well within a u32. The browser has less memory.
const MAX_TRIANGLES: usize = if cfg!(target_arch = "wasm32") { 8_000_000 } else { 40_000_000 };
const MAX_DRAWN_VERTICES: usize = 3 * MAX_TRIANGLES;

struct Scene<'f> {
    file: &'f File<'f>,
    /// Rhino layer by the index objects use.
    layers: HashMap<i32, usize>,
    /// Rhino layer by id, for the parents.
    layer_ids: HashMap<Uuid, usize>,
    /// The full path of a Rhino layer and whether it and its parents are on, worked out once.
    lineages: HashMap<usize, (String, bool)>,
    materials: HashMap<i32, usize>,
    blocks: HashMap<Uuid, usize>,
    objects: HashMap<Uuid, usize>,
    /// Viewer layers: meshes and points per Rhino layer (`None`: an object without a layer).
    meshes: BTreeMap<Option<usize>, Layer>,
    points: BTreeMap<Option<usize>, Layer>,
    next_id: u64,
    /// The block definitions being drawn: one met again contains itself.
    expanding: HashSet<Uuid>,
    /// Block members drawn so far, and whether the current instance was cut short by
    /// [`MAX_BLOCK_PARTS`].
    parts: usize,
    cut: bool,
    /// Triangles and vertices drawn so far.
    triangles: usize,
    vertices: usize,
    /// The points of the file; every `stride`-th point met is drawn, at most `MAX_POINTS`.
    total_points: usize,
    stride: u64,
    seen: u64,
    kept: usize,
    /// The local origin (metres) taken off everything drawn.
    origin: Option<DVec3>,
    /// What was drawn and what was left out, by kind.
    drawn: BTreeMap<&'static str, usize>,
    skipped: BTreeMap<&'static str, usize>,
}

/// The colour an object is drawn in: sRGB and alpha.
type Rgba = [u8; 4];

fn opaque(c: [u8; 4]) -> Rgba {
    [c[0], c[1], c[2], 255]
}

/// How an object is coloured: its display colour (by layer, object or parent) and its material.
#[derive(Clone, Copy)]
struct Paint<'f> {
    color: Rgba,
    material: Option<&'f Material>,
}

impl Paint<'_> {
    /// The colour drawn: the material's diffuse colour and transparency when there is a
    /// material, else the display colour.
    fn rgba(&self) -> Rgba {
        match self.material {
            Some(m) => {
                let alpha = (255.0 * (1.0 - m.transparency.clamp(0.0, 1.0))).round() as u8;
                [m.diffuse[0], m.diffuse[1], m.diffuse[2], alpha]
            }
            None => display(self.color),
        }
    }
}

/// Where the members of a block instance are drawn.
#[derive(Clone, Copy)]
struct Placement<'f> {
    xform: DMat4,
    /// The instance's colour and material, for members by parent.
    parent: Paint<'f>,
    /// The Rhino layer of the outermost instance on a layer that is off: Rhino hides the whole
    /// instance, so its members go to that (hidden) layer instead of their own.
    off: Option<i32>,
}

impl<'f> Scene<'f> {
    fn new(file: &'f File<'f>) -> Self {
        let index = |n: usize, key: &dyn Fn(usize) -> i32| -> HashMap<i32, usize> {
            let mut map = HashMap::new();
            for k in 0..n {
                map.entry(key(k)).or_insert(k);
            }
            map
        };
        let mut layer_ids = HashMap::new();
        for (k, l) in file.layers.iter().enumerate().filter(|(_, l)| !l.id.is_nil()) {
            layer_ids.entry(l.id).or_insert(k);
        }
        let total_points: usize = file.objects.iter().map(|o| if let Geometry::Points { points, .. } = &o.geometry { points.len() / 24 } else { 0 }).sum();
        Scene {
            file,
            layers: index(file.layers.len(), &|k| file.layers[k].index),
            layer_ids,
            lineages: HashMap::new(),
            materials: index(file.materials.len(), &|k| file.materials[k].index),
            blocks: file.blocks.iter().enumerate().map(|(k, b)| (b.id, k)).collect(),
            objects: file.objects.iter().enumerate().filter(|(_, o)| !o.attributes.id.is_nil()).map(|(k, o)| (o.attributes.id, k)).collect(),
            meshes: BTreeMap::new(),
            points: BTreeMap::new(),
            next_id: 1,
            expanding: HashSet::new(),
            parts: 0,
            cut: false,
            triangles: 0,
            vertices: 0,
            total_points,
            stride: total_points.div_ceil(super::pointcloud::MAX_POINTS).max(1) as u64,
            seen: 0,
            kept: 0,
            origin: None,
            drawn: BTreeMap::new(),
            skipped: BTreeMap::new(),
        }
    }

    fn layer(&self, index: i32) -> Option<&'f RhinoLayer> {
        self.layers.get(&index).map(|&k| &self.file.layers[k])
    }

    fn material(&self, index: i32) -> Option<&'f Material> {
        self.materials.get(&index).map(|&k| &self.file.materials[k])
    }

    /// The full path of Rhino layer `k` (`Parent::Child`) and whether it and all its parents are
    /// on. Parents are followed up to 64 levels, and a cycle of parents stops at the first layer
    /// met again.
    fn lineage(&mut self, k: usize) -> &(String, bool) {
        if !self.lineages.contains_key(&k) {
            let layers = &self.file.layers;
            let mut chain = vec![k];
            while chain.len() < 64 {
                let parent = layers[chain[chain.len() - 1]].parent;
                match self.layer_ids.get(&parent) {
                    Some(&p) if !chain.contains(&p) => chain.push(p),
                    _ => break,
                }
            }
            let path = chain.iter().rev().map(|&p| layers[p].name.as_str()).collect::<Vec<_>>().join("::");
            let shown = chain.iter().all(|&p| layers[p].visible);
            self.lineages.insert(k, (path, shown));
        }
        &self.lineages[&k]
    }

    /// Whether the layer of index `index` and all its parents are on.
    fn layer_shown(&mut self, index: i32) -> bool {
        match self.layers.get(&index) {
            Some(&k) => self.lineage(k).1,
            None => true,
        }
    }

    fn viewer_layer(&mut self, layer: i32, points: bool) -> &mut Layer {
        let key = self.layers.get(&layer).copied();
        if (if points { &self.points } else { &self.meshes }).contains_key(&key) {
            let map = if points { &mut self.points } else { &mut self.meshes };
            return map.get_mut(&key).unwrap();
        }
        let (name, color, hidden) = match key {
            Some(k) => {
                let color = display(opaque(self.file.layers[k].color));
                let (path, shown) = self.lineage(k).clone();
                (path, color, !shown)
            }
            None => ("Default".to_string(), display([0, 0, 0, 255]), false),
        };
        let map = if points { &mut self.points } else { &mut self.meshes };
        map.entry(key).or_insert_with(|| Layer {
            name: if points { format!("{name} (points)") } else { name },
            color: [color[0] as f32 / 255.0, color[1] as f32 / 255.0, color[2] as f32 / 255.0],
            hidden,
            points,
            colors: Some(Vec::new()),
            ..Layer::default()
        })
    }

    /// The colour and material of an object; `parent` those of the block instance it is part
    /// of. Colour and material each come by layer, object or parent, independently; by parent
    /// outside a block is by layer, as in Rhino.
    fn paint(&self, a: &Attributes, parent: Option<Paint<'f>>) -> Paint<'f> {
        let layer = self.layer(a.layer);
        let material = match (a.material_source, parent) {
            (1, _) => self.material(a.material),
            (3, Some(p)) => p.material,
            _ => layer.and_then(|l| self.material(l.material)),
        };
        let layer_color = layer.map(|l| opaque(l.color)).unwrap_or([0, 0, 0, 255]);
        let color = match (a.color_source, parent) {
            (1, _) => opaque(a.color),
            (3, Some(p)) => p.color,
            _ => layer_color,
        };
        Paint { color, material }
    }

    /// Element properties: name, object type, layer, id, material, block, user text.
    fn props(&mut self, a: &Attributes, kind: &str, block: Option<&str>, user: &[(String, String)]) -> String {
        let mut p = serde_json::Map::new();
        if !a.name.is_empty() {
            p.insert("Name".into(), a.name.clone().into());
        } else if let Some(b) = block {
            p.insert("Name".into(), b.into());
        }
        p.insert("Type".into(), kind.into());
        if let Some(b) = block {
            p.insert("Block".into(), b.into());
        }
        if let Some(&k) = self.layers.get(&a.layer) {
            p.insert("Layer".into(), self.lineage(k).0.clone().into());
        }
        if let Some(m) = self.paint(a, None).material.filter(|m| !m.name.is_empty()) {
            p.insert("Material".into(), m.name.clone().into());
        }
        if !a.id.is_nil() {
            p.insert("Id".into(), a.id.to_string().into());
        }
        for (key, value) in user {
            let key = if p.contains_key(key) { format!("{key} (user text)") } else { key.clone() };
            p.insert(key, value.clone().into());
        }
        serde_json::Value::Object(p).to_string()
    }

    fn run(&mut self) {
        let root = DMat4::from_scale(DVec3::splat(self.file.meters));
        let file = self.file;
        for object in &file.objects {
            let a = &object.attributes;
            if a.mode == 3 {
                continue; // block definition geometry, drawn by its instances
            }
            if a.space == 2 {
                *self.skipped.entry("objects on layouts").or_default() += 1;
                continue;
            }
            if !a.visible || a.mode == 1 {
                *self.skipped.entry("hidden objects").or_default() += 1;
                continue;
            }
            let id = self.next_id;
            self.next_id += 1;
            match &object.geometry {
                Geometry::Skipped(why) => *self.skipped.entry(why).or_default() += 1,
                Geometry::Instance { definition, xform } => {
                    let name = self.blocks.get(definition).map(|&k| file.blocks[k].name.clone()).unwrap_or_default();
                    let props = self.props(a, "Block instance", Some(&name), &object.user);
                    let off = (!self.layer_shown(a.layer)).then_some(a.layer);
                    let at = Placement { xform: root * *xform, parent: self.paint(a, None), off };
                    let mut first = Some(props);
                    self.cut = false;
                    let drawn = self.instance(*definition, at, id, &mut first, 1);
                    if self.cut {
                        *self.skipped.entry("block instances cut short (too many parts)").or_default() += 1;
                    }
                    *if drawn { self.drawn.entry("block instances") } else { self.skipped.entry("empty block instances") }.or_default() += 1;
                }
                geometry => {
                    let kind = match geometry {
                        Geometry::Meshes(_, kind) | Geometry::Points { kind, .. } => *kind,
                        _ => "",
                    };
                    let props = self.props(a, kind, None, &object.user);
                    let color = self.paint(a, None).rgba();
                    let mut first = Some(props);
                    if self.draw(geometry, a.layer, root, color, id, &mut first) {
                        *self.drawn.entry(plural(kind)).or_default() += 1;
                    }
                }
            }
        }
    }

    /// Draws the objects of a block definition, placed by `at`, as part of element `id`.
    fn instance(&mut self, definition: Uuid, at: Placement<'f>, id: u64, props: &mut Option<String>, depth: usize) -> bool {
        if depth > MAX_BLOCK_DEPTH {
            *self.skipped.entry("blocks nested too deep").or_default() += 1;
            return false;
        }
        let Some(&k) = self.blocks.get(&definition) else { return false };
        if !self.expanding.insert(definition) {
            *self.skipped.entry("blocks that contain themselves").or_default() += 1;
            return false;
        }
        let file = self.file;
        let mut drawn = false;
        for uuid in &file.blocks[k].objects {
            if self.parts >= MAX_BLOCK_PARTS {
                self.cut = true;
                break;
            }
            self.parts += 1;
            let Some(&o) = self.objects.get(uuid) else { continue };
            let object = &file.objects[o];
            let a = &object.attributes;
            if !a.visible {
                continue;
            }
            let paint = self.paint(a, Some(at.parent));
            drawn |= match &object.geometry {
                Geometry::Instance { definition, xform: inner } => {
                    let off = at.off.or_else(|| (!self.layer_shown(a.layer)).then_some(a.layer));
                    self.instance(*definition, Placement { xform: at.xform * *inner, parent: paint, off }, id, props, depth + 1)
                }
                geometry => self.draw(geometry, at.off.unwrap_or(a.layer), at.xform, paint.rgba(), id, props),
            };
        }
        self.expanding.remove(&definition);
        drawn
    }

    /// The local origin, taken from the first point drawn (`first`): that point in whole metres
    /// when it is far from zero, as the point cloud readers do, so that survey coordinates keep
    /// their precision in f32.
    fn origin(&mut self, first: Option<DVec3>) -> DVec3 {
        match first {
            Some(p) => *self.origin.get_or_insert_with(|| DVec3::from(super::pointcloud::origin_for(p.to_array()))),
            None => self.origin.unwrap_or(DVec3::ZERO),
        }
    }

    /// Adds meshes or points to the layer of `layer`; the first part drawn carries the props.
    fn draw(&mut self, geometry: &Geometry<'_>, layer: i32, xform: DMat4, color: Rgba, id: u64, props: &mut Option<String>) -> bool {
        match geometry {
            Geometry::Meshes(meshes, _) => {
                // the meshes within the triangle and vertex limits
                let fits: Vec<&Mesh> = meshes
                    .iter()
                    .filter(|m| {
                        let vertices = if m.normals.is_some() { m.positions.len() } else { 3 * m.triangles };
                        let fits = self.triangles + m.triangles <= MAX_TRIANGLES && self.vertices + vertices <= MAX_DRAWN_VERTICES;
                        if fits {
                            self.triangles += m.triangles;
                            self.vertices += vertices;
                        }
                        fits
                    })
                    .collect();
                if fits.len() < meshes.len() {
                    *self.skipped.entry("objects beyond the triangle limit").or_default() += 1;
                }
                let first = fits.iter().flat_map(|m| &m.positions).map(|p| xform.transform_point3(DVec3::from(p.map(f64::from)))).find(|p| p.is_finite());
                let origin = self.origin(first);
                let target = self.viewer_layer(layer, false);
                let start = target.indices.len() as u32;
                for mesh in fits {
                    add_mesh(target, mesh, xform, origin, color);
                }
                let count = target.indices.len() as u32 - start;
                if count == 0 {
                    return false;
                }
                target.elements.push((id, start, count));
                if let Some(p) = props.take() {
                    target.props.push((id, p));
                }
                true
            }
            Geometry::Points { points, colors, .. } => {
                let count = points.len() / 24;
                if count == 0 {
                    return false;
                }
                if self.kept >= super::pointcloud::MAX_POINTS {
                    *self.skipped.entry("point objects beyond the point limit").or_default() += 1;
                    return false;
                }
                let point = |k: usize| {
                    let f = |i: usize| f64::from_le_bytes(points[24 * k + 8 * i..24 * k + 8 * i + 8].try_into().unwrap());
                    xform.transform_point3(DVec3::new(f(0), f(1), f(2)))
                };
                let origin = self.origin((0..count).map(point).find(|p| p.is_finite()));
                // thinned out while drawing: only every stride-th point of the file is converted
                let (stride, mut seen, mut kept) = (self.stride, self.seen, self.kept);
                let target = self.viewer_layer(layer, true);
                for k in 0..count {
                    let keep = seen % stride == 0;
                    seen += 1;
                    if !keep {
                        continue;
                    }
                    if kept >= super::pointcloud::MAX_POINTS {
                        break;
                    }
                    let p = point(k);
                    if !p.is_finite() {
                        continue;
                    }
                    target.positions.extend((p - origin).to_array().map(|v| v as f32));
                    let c = colors.map_or(color, |c| [c[4 * k], c[4 * k + 1], c[4 * k + 2], 255]);
                    target.colors.get_or_insert_with(Vec::new).push(c);
                    kept += 1;
                }
                (self.seen, self.kept) = (seen, kept);
                true
            }
            _ => false,
        }
    }

    fn finish(mut self) -> (Vec<Layer>, Vec<(String, String)>) {
        let mut layers: Vec<Layer> = std::mem::take(&mut self.meshes).into_values().chain(std::mem::take(&mut self.points).into_values()).collect();
        for layer in &mut layers {
            // one colour for the whole layer: no vertex colours
            let base = [0, 1, 2].map(|k| (layer.color[k] * 255.0).round() as u8);
            if layer.colors.as_ref().is_some_and(|c| c.iter().all(|c| c[..3] == base && c[3] == 255)) {
                layer.colors = None;
            }
        }
        let file = self.file;
        let mut info = vec![("Rhino file version".to_string(), version_name(file.versions.archive))];
        if !file.application.is_empty() {
            info.push(("Rhino application".into(), file.application.clone()));
        }
        info.push(("Rhino units".into(), if file.meters == 1.0 { file.unit.to_string() } else { format!("{} (converted to metres)", file.unit) }));
        if let Some(o) = self.origin.filter(|o| *o != DVec3::ZERO) {
            info.push(("Rhino world origin (metres)".into(), format!("{}, {}, {}", o.x, o.y, o.z)));
        }
        let list = |m: &BTreeMap<&str, usize>| m.iter().map(|(k, n)| format!("{n} {k}")).collect::<Vec<_>>().join(", ");
        if !self.drawn.is_empty() {
            info.push(("Rhino objects drawn".into(), list(&self.drawn)));
        }
        if self.stride > 1 {
            info.push(("Rhino points".into(), format!("{} in the file, every {}th drawn", self.total_points, self.stride)));
        }
        if !self.skipped.is_empty() {
            info.push(("Rhino objects left out".into(), list(&self.skipped)));
        }
        if !file.damaged.is_empty() {
            info.push(("Rhino unreadable records".into(), list(&file.damaged)));
        }
        (layers, info)
    }
}

/// Black, Rhino's colour for the default layer, draws in the viewer's grey.
fn display(c: Rgba) -> Rgba {
    if c[..3] == [0, 0, 0] {
        let g = DEFAULT_COLOR.map(|v| (v * 255.0).round() as u8);
        [g[0], g[1], g[2], c[3]]
    } else {
        c
    }
}

fn plural(kind: &str) -> &'static str {
    match kind {
        "Mesh" => "meshes",
        "Surface" => "surfaces",
        "Polysurface" => "polysurfaces",
        "Extrusion" => "extrusions",
        "Point cloud" => "point clouds",
        "Point" => "points",
        _ => "objects",
    }
}

fn version_name(archive: u32) -> String {
    let rhino = match archive {
        2..=5 => archive,
        v => v / 10,
    };
    format!("Rhino {rhino} (archive version {archive})")
}

/// Adds the triangles of a mesh, transformed and moved by `-origin`, with its normals (or flat
/// ones) and colours.
fn add_mesh(layer: &mut Layer, mesh: &Mesh, xform: DMat4, origin: DVec3, color: Rgba) {
    let n = mesh.positions.len() as u32;
    let normal_matrix = DMat3::from_mat4(xform).inverse().transpose();
    let mirrored = xform.determinant() < 0.0;
    let color = mesh.color.map_or(color, opaque);
    // in f64 up to the local origin, so that survey coordinates keep their precision
    let point = |i: u32| xform.transform_point3(DVec3::from(mesh.positions[i as usize].map(f64::from))) - origin;
    let vertex_color = |i: u32| mesh.colors.as_ref().map_or(color, |c| {
        let c = c[i as usize];
        [c[0], c[1], c[2], 255 - c[3]]
    });
    let colors = layer.colors.get_or_insert_with(Vec::new);
    let mut triangles = Vec::with_capacity(2 * mesh.faces.len());
    for f in &mesh.faces {
        if f.iter().any(|&i| i >= n) {
            continue;
        }
        triangles.push([f[0], f[1], f[2]]);
        if f[2] != f[3] {
            triangles.push([f[0], f[2], f[3]]);
        }
    }
    if let Some(normals) = &mesh.normals {
        // shared vertices with the file's normals
        let base = layer.positions.len() as u32 / 3;
        for i in 0..n {
            layer.positions.extend(point(i).to_array().map(|v| v as f32));
            // the inverse transpose keeps normals outward under a mirroring transform too
            let normal = (normal_matrix * DVec3::from(normals[i as usize].map(f64::from))).normalize_or_zero();
            layer.normals.extend(normal.to_array().map(|v| v as f32));
            colors.push(vertex_color(i));
        }
        for t in triangles {
            let (b, c) = if mirrored { (t[2], t[1]) } else { (t[1], t[2]) };
            layer.indices.extend([base + t[0], base + b, base + c]);
        }
    } else {
        // flat shaded
        for t in triangles {
            let order = if mirrored { [0, 2, 1] } else { [0, 1, 2] };
            let p = order.map(|k| point(t[k]));
            let normal = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
            let base = layer.positions.len() as u32 / 3;
            for (k, &corner) in order.iter().enumerate() {
                layer.positions.extend(p[k].to_array().map(|v| v as f32));
                layer.normals.extend(normal.to_array().map(|v| v as f32));
                colors.push(vertex_color(t[corner]));
            }
            layer.indices.extend([base, base + 1, base + 2]);
        }
    }
}

fn convert(file: &File<'_>) -> (Vec<Layer>, Vec<(String, String)>) {
    let mut scene = Scene::new(file);
    scene.run();
    scene.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Arc;

    /// Writes 3dm archives as openNURBS does, for the parts the reader uses: chunks with 4- or
    /// 8-byte lengths (CRCs written as zero, the reader does not check them).
    struct W {
        big: bool,
    }

    fn i32b(v: i32) -> Vec<u8> {
        v.to_le_bytes().to_vec()
    }

    fn f64s(v: &[f64]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    fn wstr(s: &str) -> Vec<u8> {
        if s.is_empty() {
            return i32b(0);
        }
        let units: Vec<u16> = s.encode_utf16().chain([0]).collect();
        let mut out = i32b(units.len() as i32);
        out.extend(units.iter().flat_map(|u| u.to_le_bytes()));
        out
    }

    fn cat(parts: &[&[u8]]) -> Vec<u8> {
        parts.concat()
    }

    impl W {
        fn chunk(&self, typecode: u32, body: &[u8]) -> Vec<u8> {
            let crc = if typecode & TCODE_CRC != 0 { 4 } else { 0 };
            let length = body.len() + crc;
            let mut out = typecode.to_le_bytes().to_vec();
            if self.big {
                out.extend((length as i64).to_le_bytes());
            } else {
                out.extend((length as u32).to_le_bytes());
            }
            out.extend(body);
            out.extend(vec![0; crc]);
            out
        }

        fn short(&self, typecode: u32, value: i64) -> Vec<u8> {
            let mut out = typecode.to_le_bytes().to_vec();
            if self.big {
                out.extend(value.to_le_bytes());
            } else {
                out.extend((value as i32).to_le_bytes());
            }
            out
        }

        /// `BeginWrite3dmChunk(typecode, major, minor)`.
        fn versioned(&self, typecode: u32, major: i32, minor: i32, body: &[u8]) -> Vec<u8> {
            self.chunk(typecode, &cat(&[&i32b(major), &i32b(minor), body]))
        }

        /// `WriteObject`, with user data entries (class id, data).
        fn object(&self, class: Uuid, data: &[u8], userdata: &[(Uuid, Vec<u8>)]) -> Vec<u8> {
            let mut body = self.chunk(TCODE_OPENNURBS_CLASS_UUID, &class.0);
            if !class.is_nil() {
                body.extend(self.chunk(TCODE_OPENNURBS_CLASS_DATA, data));
                for (id, ud) in userdata {
                    body.extend(self.user_data(*id, ud));
                }
                body.extend(self.short(TCODE_OPENNURBS_CLASS_END, 0));
            }
            self.chunk(TCODE_OPENNURBS_CLASS, &body)
        }

        fn user_data(&self, class: Uuid, data: &[u8]) -> Vec<u8> {
            let header = self.chunk(TCODE_OPENNURBS_CLASS_USERDATA_HEADER, &cat(&[&class.0, &[0; 16], &i32b(1), &f64s(&DMat4::IDENTITY.to_cols_array())]));
            self.chunk(TCODE_OPENNURBS_CLASS_USERDATA, &cat(&[&[0x20], &header, &self.chunk(TCODE_ANONYMOUS_CHUNK, data)]))
        }

        fn user_strings(&self, pairs: &[(&str, &str)]) -> Vec<u8> {
            let mut list = i32b(pairs.len() as i32);
            for (k, v) in pairs {
                list.extend(self.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &cat(&[&wstr(k), &wstr(v)])));
            }
            self.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &list)
        }

        /// `WriteCompressedBuffer`, stored (method 0) or deflated (method 1).
        fn compressed(&self, bytes: &[u8], deflate: bool) -> Vec<u8> {
            if bytes.is_empty() {
                return i32b(0);
            }
            let mut out = cat(&[&(bytes.len() as u32).to_le_bytes(), &[0; 4]]);
            if deflate {
                let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                z.write_all(bytes).unwrap();
                out.push(1);
                out.extend(self.chunk(TCODE_ANONYMOUS_CHUNK, &z.finish().unwrap()));
            } else {
                out.push(0);
                out.extend(bytes);
            }
            out
        }

        /// An `ON_Mesh` 3.x: points, faces (a triangle repeats its last index), optional
        /// normals and colours.
        fn mesh(&self, points: &[[f32; 3]], faces: &[[i32; 4]], normals: Option<&[[f32; 3]]>, colors: Option<&[[u8; 4]]>, deflate: bool) -> Vec<u8> {
            let floats = |v: &[[f32; 3]]| -> Vec<u8> { v.iter().flatten().flat_map(|f| f.to_le_bytes()).collect() };
            let mut out = vec![0x30];
            out.extend(i32b(points.len() as i32));
            out.extend(i32b(faces.len() as i32));
            out.extend(vec![0; 148]);
            out.extend([0; 5]);
            out.extend(i32b(4));
            out.extend(faces.iter().flatten().flat_map(|i| i.to_le_bytes()));
            out.extend(self.compressed(&floats(points), deflate));
            out.extend(self.compressed(&normals.map(floats).unwrap_or_default(), deflate));
            out.extend(i32b(0));
            out.extend(i32b(0));
            out.extend(self.compressed(&colors.map(|c| c.concat()).unwrap_or_default(), deflate));
            out
        }

        /// An `ON_Brep` 3.2 whose faces carry the given render meshes; the first face is red.
        fn brep(&self, meshes: &[Vec<u8>]) -> Vec<u8> {
            let mut out = vec![0x32];
            for _ in 0..7 {
                out.extend(self.chunk(TCODE_ANONYMOUS_CHUNK, &cat(&[&[0x10], &i32b(0)])));
            }
            // the faces (index, loops, surface, reversed, material channel), their ids, and a colour
            // for the first face only
            let mut faces = cat(&[&[0x12], &i32b(meshes.len() as i32)]);
            for k in 0..meshes.len() {
                faces.extend(cat(&[&i32b(k as i32), &i32b(1), &i32b(0), &i32b(k as i32), &i32b(0), &i32b(0)]));
            }
            faces.extend(vec![7; 16 * meshes.len()]);
            faces.push(1);
            for k in 0..meshes.len() {
                faces.extend(if k == 0 { [255, 0, 0, 0] } else { [255; 4] });
            }
            out.extend(self.chunk(TCODE_ANONYMOUS_CHUNK, &faces));
            out.extend(f64s(&[0.0; 6]));
            let mut render = Vec::new();
            for m in meshes {
                render.push(1);
                render.extend(self.object(ON_MESH, m, &[]));
            }
            out.extend(self.chunk(TCODE_ANONYMOUS_CHUNK, &render));
            out.extend(self.chunk(TCODE_ANONYMOUS_CHUNK, &vec![0; meshes.len()]));
            out.extend(i32b(2));
            out
        }

        #[allow(clippy::too_many_arguments)]
        fn layer(&self, index: i32, name: &str, color: [u8; 4], visible: bool, id: Uuid, parent: Uuid, material: i32) -> Vec<u8> {
            let data = cat(&[
                &[0x16],
                &i32b(if visible { 0 } else { 1 }),
                &i32b(index),
                &i32b(0),
                &i32b(material),
                &i32b(0),
                &color,
                &[0; 20],
                &wstr(name),
                &[visible as u8],
                &i32b(-1),
                &[0; 12],
                &[0],
                &id.0,
                &parent.0,
                &[1],
            ]);
            self.chunk(TCODE_LAYER_RECORD, &self.object(ON_LAYER, &data, &[]))
        }

        fn table(&self, typecode: u32, records: &[Vec<u8>]) -> Vec<u8> {
            let mut body = records.concat();
            body.extend(self.short(TCODE_ENDOFTABLE, 0));
            self.chunk(typecode, &body)
        }

        fn record(&self, object_type: u32, object: &[u8], attributes: &[u8], user: &[(&str, &str)]) -> Vec<u8> {
            let mut body = self.short(TCODE_OBJECT_RECORD_TYPE, object_type as i64);
            body.extend(object);
            body.extend(self.chunk(TCODE_OBJECT_RECORD_ATTRIBUTES, attributes));
            if !user.is_empty() {
                let mut ud = self.user_data(ON_USER_STRING_LIST, &self.user_strings(user));
                ud.extend(self.short(TCODE_OPENNURBS_CLASS_END, 0));
                body.extend(self.chunk(TCODE_OBJECT_RECORD_ATTRIBUTES_USERDATA, &ud));
            }
            body.extend(self.short(TCODE_OBJECT_RECORD_END, 0));
            self.chunk(TCODE_OBJECT_RECORD, &body)
        }

        /// A file: header, comment, properties, units settings, then the tables.
        fn file(&self, archive: u32, opennurbs: u32, unit: i32, tables: &[Vec<u8>]) -> Vec<u8> {
            let mut out = format!("3D Geometry File Format {archive:>8}").into_bytes();
            out.extend(self.chunk(1, b"written by the open-3d-viewer tests"));
            let application = self.chunk(TCODE_PROPERTIES_APPLICATION, &cat(&[&[0x10], &wstr("Rhinoceros 7"), &wstr(""), &wstr("")]));
            out.extend(self.table(TCODE_PROPERTIES_TABLE, &[self.short(TCODE_PROPERTIES_OPENNURBS_VERSION, opennurbs as i64), application]));
            let units = cat(&[&i32b(102), &i32b(unit), &f64s(&[0.001, 1.0, 0.01]), &i32b(0), &i32b(3), &f64s(&[1.0]), &wstr("")]);
            out.extend(self.table(TCODE_SETTINGS_TABLE, &[self.chunk(TCODE_SETTINGS_UNITSANDTOLS, &units)]));
            for t in tables {
                out.extend(t);
            }
            let size = out.len() as u64 + if self.big { 16 } else { 12 };
            out.extend(if self.big { self.chunk(TCODE_ENDOFFILE, &size.to_le_bytes()) } else { self.chunk(TCODE_ENDOFFILE, &(size as u32).to_le_bytes()) });
            out
        }
    }

    /// Rhino 5+ object attributes: id, layer and the given items.
    fn attributes(id: Uuid, layer: i32, items: &[(u8, Vec<u8>)]) -> Vec<u8> {
        let mut out = cat(&[&[0x20], &id.0, &i32b(layer)]);
        for (item, value) in items {
            out.push(*item);
            out.extend(value);
        }
        out.push(0);
        out
    }

    fn id(n: u8) -> Uuid {
        Uuid([n; 16])
    }

    fn load(name: &str, data: Vec<u8>) -> (crate::model::Model, Vec<(String, String)>) {
        let source = Source::Bytes { name: name.into(), data: Arc::new(data) };
        let model = crate::loaders::load_with(&source, &crate::loaders::Progress::default()).expect("3dm");
        let info = model.project.info.clone();
        (model, info)
    }

    fn entry<'a>(info: &'a [(String, String)], key: &str) -> &'a str {
        info.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).unwrap_or("")
    }

    const QUAD: [[f32; 3]; 4] = [[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0], [1000.0, 2000.0, 0.0], [0.0, 2000.0, 0.0]];
    const UP: [[f32; 3]; 4] = [[0.0, 0.0, 1.0]; 4];

    /// A Rhino 7 file in millimetres: layers with a parent and a hidden one, a material, a mesh
    /// with user text, a polysurface with render meshes, an extrusion with a mesh cache, a block
    /// instance, a point cloud, and objects that are left out.
    fn rhino7() -> Vec<u8> {
        let w = W { big: true };
        let layers = w.table(TCODE_LAYER_TABLE, &[
            w.layer(0, "Walls", [200, 0, 0, 0], true, id(1), Uuid::default(), -1),
            w.layer(1, "Glass", [0, 0, 0, 0], true, id(2), id(1), 0),
            w.layer(2, "Hidden", [0, 200, 0, 0], false, id(3), Uuid::default(), -1),
            w.layer(3, "Off parent child", [0, 0, 200, 0], true, id(4), id(3), -1),
        ]);
        let glass = cat(&[
            &w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &cat(&[&0x0Cu32.to_le_bytes(), &i32b(0), &wstr("Blue glass")])),
            &[0; 16],
            &[0, 0, 0, 0],
            &[0, 0, 255, 0],
            &[0; 16],
            &f64s(&[1.0, 0.0, 0.0, 0.5]),
        ]);
        let materials = w.table(TCODE_MATERIAL_TABLE, &[w.chunk(TCODE_MATERIAL_RECORD, &w.object(ON_MATERIAL, &w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &glass), &[]))]);
        let model_attributes = w.versioned(TCODE_MODEL_ATTRIBUTES_CHUNK, 1, 0, &cat(&[&[0], &[1], &id(40).0, &[0], &[0], &[1], &wstr("Chair")]));
        let units = w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &cat(&[&i32b(2), &f64s(&[0.001]), &wstr("")]));
        let definition = cat(&[&model_attributes, &i32b(1), &units, &wstr(""), &wstr(""), &wstr(""), &f64s(&[0.0; 6]), &[1], &i32b(1), &id(41).0, &[0]]);
        let blocks = w.table(TCODE_INSTANCE_DEFINITION_TABLE, &[w.chunk(TCODE_INSTANCE_DEFINITION_RECORD, &w.object(ON_INSTANCE_DEFINITION, &w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &definition), &[]))]);

        let quad = [[0, 1, 2, 3]];
        let slab = w.object(ON_MESH, &w.mesh(&QUAD, &quad, Some(&UP), None, true), &[]);
        let triangle = |z: f32| w.mesh(&[[0.0, 0.0, z], [1000.0, 0.0, z], [0.0, 1000.0, z]], &[[0, 1, 2, 2]], None, None, false);
        let brep = w.object(BREP_ALIASES[0], &w.brep(&[triangle(3000.0), triangle(3500.0)]), &[]);
        // an extrusion (Rhino 6 layout) whose mesh cache holds an analysis mesh, then the render mesh
        let item = |cache: &mut Vec<u8>, id: Uuid, z: f32| {
            cache.push(1);
            cache.extend(w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &cat(&[&id.0, &w.object(ON_MESH, &triangle(z), &[])])));
        };
        let mut cache = Vec::new();
        item(&mut cache, Uuid::parse("AC12F955-A29F-437B-A4C9-06C85B7FF57C"), -9000.0);
        item(&mut cache, RENDER_MESH_ID, 8000.0);
        cache.push(0);
        let extrusion = cat(&[&w.object(Uuid::parse("4ED7D4DB-E947-11d3-BFE5-0010830122F0"), b"curve", &[]), &[0; 155], &i32b(1), &[1, 1], &w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &cache)]);
        let extrusion = w.object(ON_EXTRUSION, &w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 3, &extrusion), &[]);
        let chair = w.object(ON_MESH, &triangle(0.0), &[]);
        let mut xform = DMat4::from_translation(DVec3::new(5000.0, 0.0, 0.0)).transpose().to_cols_array().to_vec();
        xform.extend([0.0; 6]);
        let instance = w.object(ON_INSTANCE_REF, &cat(&[&[0x10], &id(40).0, &f64s(&xform)]), &[]);
        let points = [[0.0, 0.0, -1000.0], [1000.0, 0.0, -1000.0], [0.0, 1000.0, -1000.0]];
        let cloud = cat(&[&[0x11], &i32b(3), &f64s(&points.concat()), &[0; 180], &i32b(0), &i32b(3), &[255, 0, 0, 0, 0, 255, 0, 0, 0, 0, 255, 0]]);
        let cloud = w.object(ON_POINT_CLOUD, &cloud, &[]);
        let curve = w.object(Uuid::parse("4ED7D4DD-E947-11d3-BFE5-0010830122F0"), b"nurbs", &[]);
        let subd = w.object(ON_SUBD, b"subd", &[]);
        let objects = w.table(TCODE_OBJECT_TABLE, &[
            w.record(0x20, &slab, &attributes(id(30), 0, &[(1, wstr("Slab"))]), &[("Fire rating", "60 min"), ("Name", "user name")]),
            w.record(0x10, &brep, &attributes(id(31), 1, &[]), &[]),
            w.record(0x4000_0000, &extrusion, &attributes(id(32), 0, &[(6, vec![10, 20, 30, 0]), (13, vec![1])]), &[]),
            w.record(0x20, &chair, &attributes(id(41), 0, &[(12, vec![3]), (13, vec![3])]), &[]),
            w.record(0x1000, &instance, &attributes(id(33), 0, &[(1, wstr("Chair 1")), (6, vec![0, 255, 0, 0]), (13, vec![1])]), &[("Supplier", "Acme")]),
            w.record(0x20, &w.object(ON_MESH, &triangle(0.0), &[]), &attributes(id(34), 3, &[]), &[]),
            w.record(0x20, &w.object(ON_MESH, &triangle(0.0), &[]), &attributes(id(35), 0, &[(11, vec![0])]), &[]),
            w.record(0x2, &cloud, &attributes(id(36), 0, &[]), &[]),
            w.record(4, &curve, &attributes(id(37), 0, &[]), &[]),
            w.record(0x40000, &subd, &attributes(id(38), 0, &[]), &[]),
            w.record(0x20, &w.object(ON_MESH, &triangle(0.0), &[]), &attributes(id(39), 0, &[(19, vec![2])]), &[]),
        ]);
        w.file(70, 2_400_000_000, 2, &[materials, layers, blocks, objects])
    }

    #[test]
    fn rhino7_layers_objects_blocks_and_units() {
        let (model, info) = load("house.3DM", rhino7());
        assert_eq!(model.format, "Rhino");
        let names: Vec<&str> = model.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Walls", "Walls::Glass", "Hidden::Off parent child", "Walls (points)"]);
        let walls = &model.layers[0];
        assert_eq!(walls.color, [200.0 / 255.0, 0.0, 0.0]);
        assert!(!walls.hidden);
        // the slab (quad, 2 triangles), the extrusion's render mesh and the block instance
        assert_eq!(walls.triangle_count(), 4);
        // millimetres to metres
        let slab: Vec<&[f32]> = walls.positions.chunks_exact(3).take(4).collect();
        assert_eq!(slab[2], [1.0, 2.0, 0.0]);
        assert!(walls.normals.chunks_exact(3).take(4).all(|n| n == [0.0, 0.0, 1.0]));
        // the extrusion's render mesh at 8 m, not the analysis mesh at -9 m
        assert!(walls.positions.chunks_exact(3).any(|p| p[2] == 8.0));
        assert!(walls.positions.chunks_exact(3).all(|p| p[2] > -5.0));
        let colors = walls.colors.as_ref().expect("object colours");
        assert_eq!(colors[0], [200, 0, 0, 255], "slab by layer");
        assert!(colors.contains(&[10, 20, 30, 255]), "extrusion by object");
        // the block definition's triangle, placed 5 m along x, coloured by its parent (green)
        let block: Vec<usize> = (0..walls.vertex_count()).filter(|&k| walls.positions[3 * k] >= 5.0).collect();
        assert_eq!(block.len(), 3);
        assert!(block.iter().all(|&k| colors[k] == [0, 255, 0, 255]));
        // the glass layer: child of Walls, coloured by its material, half transparent, but for
        // the first face of the polysurface, which has a colour of its own
        let glass = &model.layers[1];
        assert_eq!(glass.triangle_count(), 2);
        let glass_colors = glass.colors.as_ref().unwrap();
        assert_eq!(glass_colors[..3], [[255, 0, 0, 255]; 3]);
        assert_eq!(glass_colors[3..], [[0, 0, 255, 128]; 3]);
        assert!(glass.positions.chunks_exact(3).take(3).all(|p| p[2] == 3.0));
        assert!(glass.positions.chunks_exact(3).skip(3).all(|p| p[2] == 3.5));
        // a layer under a hidden parent starts hidden
        assert!(model.layers[2].hidden);
        // the point cloud in its own colours
        let points = &model.layers[3];
        assert!(points.points);
        assert_eq!(points.point_count(), 3);
        assert_eq!(points.colors.as_ref().unwrap()[1], [0, 255, 0, 255]);
        assert_eq!(points.positions[2], -1.0);
        // elements: slab, polysurface, extrusion, block instance, hidden-layer mesh
        assert_eq!(model.props.len(), 5);
        let slab = model.props.values().find(|p| p.contains("\"Slab\"")).expect("slab props");
        let slab: serde_json::Value = serde_json::from_str(slab).unwrap();
        assert_eq!(slab["Type"], "Mesh");
        assert_eq!(slab["Layer"], "Walls");
        assert_eq!(slab["Fire rating"], "60 min");
        assert_eq!(slab["Name (user text)"], "user name");
        assert_eq!(slab["Id"], id(30).to_string());
        let chair = model.props.values().find(|p| p.contains("Block instance")).expect("instance props");
        assert!(chair.contains("\"Name\":\"Chair 1\"") && chair.contains("\"Block\":\"Chair\"") && chair.contains("Acme"), "{chair}");
        assert!(model.props.values().any(|p| p.contains("\"Type\":\"Polysurface\"") && p.contains("Blue glass")));
        assert!(model.props.values().any(|p| p.contains("\"Type\":\"Extrusion\"")));
        assert_eq!(entry(&info, "Rhino file version"), "Rhino 7 (archive version 70)");
        assert_eq!(entry(&info, "Rhino application"), "Rhinoceros 7");
        assert_eq!(entry(&info, "Rhino units"), "millimetres (converted to metres)");
        assert_eq!(entry(&info, "Rhino objects drawn"), "1 block instances, 1 extrusions, 2 meshes, 1 point clouds, 1 polysurfaces");
        assert_eq!(entry(&info, "Rhino objects left out"), "1 SubD objects (no mesh in the file), 1 curves, 1 hidden objects, 1 objects on layouts");
    }

    /// A Rhino 4 file: 4-byte chunk lengths, the attributes layout before Rhino 5, an
    /// uncompressed (1.x) mesh, a Rhino 4 material, a Rhino 5 extrusion with its meshes in user
    /// data, metres.
    #[test]
    fn rhino4_layout() {
        let w = W { big: false };
        let layers = w.table(TCODE_LAYER_TABLE, &[w.layer(0, "Default", [0, 0, 0, 0], true, id(1), Uuid::default(), 0)]);
        let material = w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &cat(&[&id(9).0, &i32b(0), &wstr("Oak"), &[0; 16], &[0; 4], &[150, 100, 50, 0], &[0; 16], &f64s(&[1.0, 0.0, 0.0, 0.0])]));
        let materials = w.table(TCODE_MATERIAL_TABLE, &[w.chunk(TCODE_MATERIAL_RECORD, &w.object(ON_MATERIAL, &cat(&[&[0x20], &material]), &[]))]);
        // a 1.x mesh: arrays of points, normals, texture coordinates, curvatures and colours
        let mut mesh = vec![0x10];
        mesh.extend(i32b(4));
        mesh.extend(i32b(1));
        mesh.extend(vec![0; 153]);
        mesh.extend(i32b(2));
        mesh.extend([0u16, 1, 2, 3].iter().flat_map(|i| i.to_le_bytes()));
        mesh.extend(i32b(4));
        mesh.extend(QUAD.iter().flatten().flat_map(|f| (f / 1000.0).to_le_bytes()));
        for _ in 0..4 {
            mesh.extend(i32b(0));
        }
        let old_attributes = |id: Uuid, name: &str| cat(&[&[0x12], &id.0, &i32b(0), &i32b(-1), &[0; 4], &[0; 20], &i32b(1), &[0, 0, 0, 0], &wstr(name), &wstr(""), &i32b(0), &[1]]);
        // version 1.6 (Rhino 4): display material references, plot fields, line type, then the
        // space, 1 for a layout
        let attributes_1_6 = |id: Uuid, space: u8| {
            let mut a = old_attributes(id, "");
            a[0] = 0x16;
            a.extend(cat(&[&i32b(0), &i32b(0), &[0], &[0; 4], &[0], &f64s(&[0.0]), &i32b(-1), &[space]]));
            a
        };
        let triangle = w.mesh(&[[0.0, 0.0, 5.0], [1.0, 0.0, 5.0], [0.0, 1.0, 5.0]], &[[0, 1, 2, 2]], None, None, true);
        let extrusion = cat(&[&w.object(Uuid::parse("4ED7D4DB-E947-11d3-BFE5-0010830122F0"), b"curve", &[]), &[0; 155], &i32b(1), &[1, 1]]);
        let cache = cat(&[&w.object(ON_MESH, &triangle, &[]), &w.object(Uuid::default(), &[], &[]), &w.object(Uuid::default(), &[], &[])]);
        let extrusion = w.object(ON_EXTRUSION, &w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 2, &extrusion), &[(ON_V5_EXTRUSION_MESH_CACHE, cache)]);
        let objects = w.table(TCODE_OBJECT_TABLE, &[
            w.record(0x20, &w.object(ON_MESH, &mesh, &[]), &old_attributes(id(2), "Floor"), &[]),
            w.record(0x4000_0000, &extrusion, &old_attributes(id(3), ""), &[]),
            w.record(0x20, &w.object(ON_MESH, &triangle, &[]), &attributes_1_6(id(4), 0), &[]),
            // a logo on a layout
            w.record(0x20, &w.object(ON_MESH, &w.mesh(&[[0.0, 0.0, 90.0], [1.0, 0.0, 90.0], [0.0, 1.0, 90.0]], &[[0, 1, 2, 2]], None, None, true), &[]), &attributes_1_6(id(5), 1), &[]),
        ]);
        let (model, info) = load("old.3dm", w.file(4, 200_809_010, 4, &[materials, layers, objects]));
        assert_eq!(model.layers.len(), 1);
        let layer = &model.layers[0];
        assert_eq!(layer.name, "Default");
        assert_eq!(layer.triangle_count(), 4);
        // the layer's material colours everything
        assert_eq!(layer.colors.as_ref().unwrap()[0], [150, 100, 50, 255]);
        assert_eq!(model.bounds.min, [0.0, 0.0, 0.0]);
        assert_eq!(model.bounds.max, [1.0, 2.0, 5.0]);
        assert!(model.props.values().any(|p| p.contains("\"Name\":\"Floor\"") && p.contains("Oak")));
        assert_eq!(entry(&info, "Rhino file version"), "Rhino 4 (archive version 4)");
        assert_eq!(entry(&info, "Rhino units"), "metres");
        assert_eq!(entry(&info, "Rhino objects left out"), "1 objects on layouts");
    }

    #[test]
    fn black_layers_draw_grey_and_damaged_records_are_counted() {
        let w = W { big: true };
        let layers = w.table(TCODE_LAYER_TABLE, &[w.layer(0, "Default", [0, 0, 0, 0], true, id(1), Uuid::default(), -1)]);
        let good = w.record(0x20, &w.object(ON_MESH, &w.mesh(&QUAD, &[[0, 1, 2, 3]], None, None, false), &[]), &attributes(id(2), 0, &[]), &[]);
        // a mesh that claims 5 vertices but stores 4
        let mut bad = w.mesh(&QUAD, &[[0, 1, 2, 3]], None, None, false);
        bad[1] = 5;
        let bad = w.record(0x20, &w.object(ON_MESH, &bad, &[]), &attributes(id(3), 0, &[]), &[]);
        let (model, info) = load("grey.3dm", w.file(80, 2_400_000_000, 4, &[layers, w.table(TCODE_OBJECT_TABLE, &[good, bad])]));
        assert_eq!(model.layers[0].color, DEFAULT_COLOR.map(|v| (v * 255.0).round() / 255.0));
        assert!(model.layers[0].colors.is_none());
        assert_eq!(model.triangle_count(), 2);
        assert_eq!(entry(&info, "Rhino file version"), "Rhino 8 (archive version 80)");
        assert_eq!(entry(&info, "Rhino objects left out"), "1 unreadable objects");
    }

    #[test]
    fn files_without_meshes_say_why() {
        let w = W { big: true };
        let brep = w.object(BREP_ALIASES[0], &w.brep(&[]), &[]);
        let curve = w.object(Uuid::parse("4ED7D4E6-E947-11d3-BFE5-0010830122F0"), b"polyline", &[]);
        let objects = w.table(TCODE_OBJECT_TABLE, &[w.record(0x10, &brep, &attributes(id(1), 0, &[]), &[]), w.record(4, &curve, &attributes(id(2), 0, &[]), &[])]);
        let source = Source::Bytes { name: "small.3dm".into(), data: Arc::new(w.file(70, 2_400_000_000, 2, &[objects])) };
        let error = RhinoLoader.load(&source).unwrap_err();
        assert_eq!(error, "small.3dm: nothing to draw in this Rhino file (1 curves, 1 polysurfaces without a render mesh)");
    }

    #[test]
    fn uuids_round_trip_and_other_files_are_rejected() {
        assert_eq!(ON_MESH.to_string(), "4ed7d4e4-e947-11d3-bfe5-0010830122f0");
        assert_eq!(&ON_MESH.0[..4], &[0xE4, 0xD4, 0xD7, 0x4E]);
        let source = Source::Bytes { name: "x.3dm".into(), data: Arc::new(b"hello".to_vec()) };
        assert!(RhinoLoader.load(&source).unwrap_err().contains("Rhino"));
        // a header and then a truncated chunk
        let mut data = b"3D Geometry File Format       70".to_vec();
        data.extend([1, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0]);
        let source = Source::Bytes { name: "x.3dm".into(), data: Arc::new(data) };
        assert!(RhinoLoader.load(&source).is_err());
    }

    /// A block definition record (Rhino 6 layout) with the ids of its objects.
    fn block(w: &W, id: Uuid, name: &str, objects: &[Uuid]) -> Vec<u8> {
        let model_attributes = w.versioned(TCODE_MODEL_ATTRIBUTES_CHUNK, 1, 0, &cat(&[&[0], &[1], &id.0, &[0], &[0], &[1], &wstr(name)]));
        let units = w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &cat(&[&i32b(2), &f64s(&[0.001]), &wstr("")]));
        let list = cat(&[&i32b(objects.len() as i32), &objects.iter().flat_map(|o| o.0).collect::<Vec<u8>>()]);
        let definition = cat(&[&model_attributes, &i32b(1), &units, &wstr(""), &wstr(""), &wstr(""), &f64s(&[0.0; 6]), &[1], &list, &[0]]);
        w.chunk(TCODE_INSTANCE_DEFINITION_RECORD, &w.object(ON_INSTANCE_DEFINITION, &w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &definition), &[]))
    }

    /// A block instance of `definition`, moved by `by`.
    fn instance(w: &W, definition: Uuid, by: DVec3) -> Vec<u8> {
        let mut xform = DMat4::from_translation(by).transpose().to_cols_array().to_vec();
        xform.extend([0.0; 6]);
        w.object(ON_INSTANCE_REF, &cat(&[&[0x10], &definition.0, &f64s(&xform)]), &[])
    }

    fn triangle(w: &W) -> Vec<u8> {
        w.object(ON_MESH, &w.mesh(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], &[[0, 1, 2, 2]], None, None, false), &[])
    }

    /// Block definition geometry: mode 3.
    const DEFINITION: (u8, [u8; 1]) = (12, [3]);

    #[test]
    fn blocks_that_contain_themselves_stop() {
        // block A holds a triangle and, twice, an instance of itself: without a check every level
        // doubles, 2^32 instances before the depth limit
        let w = W { big: true };
        let layers = w.table(TCODE_LAYER_TABLE, &[w.layer(0, "Default", [0, 0, 0, 0], true, id(1), Uuid::default(), -1)]);
        let blocks = w.table(TCODE_INSTANCE_DEFINITION_TABLE, &[block(&w, id(50), "A", &[id(60), id(60), id(61)])]);
        let definition = vec![(DEFINITION.0, DEFINITION.1.to_vec())];
        let objects = w.table(TCODE_OBJECT_TABLE, &[
            w.record(0x1000, &instance(&w, id(50), DVec3::X), &attributes(id(60), 0, &definition), &[]),
            w.record(0x20, &triangle(&w), &attributes(id(61), 0, &definition), &[]),
            w.record(0x1000, &instance(&w, id(50), DVec3::ZERO), &attributes(id(62), 0, &[]), &[]),
        ]);
        let (model, info) = load("cycle.3dm", w.file(70, 2_400_000_000, 4, &[layers, blocks, objects]));
        assert_eq!(model.triangle_count(), 1);
        assert_eq!(entry(&info, "Rhino objects left out"), "2 blocks that contain themselves");
    }

    #[test]
    fn wide_nested_blocks_stop_at_the_part_limit() {
        // 20 levels of blocks, each with its next level 4 times: 4^20 instances, but no cycle
        let w = W { big: true };
        let layers = w.table(TCODE_LAYER_TABLE, &[w.layer(0, "Default", [0, 0, 0, 0], true, id(1), Uuid::default(), -1)]);
        let level = |k: u8| Uuid([k, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);
        let member = |k: u8| Uuid([k, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9]);
        let blocks: Vec<Vec<u8>> = (0..20).map(|k| block(&w, level(k), "level", &[member(k + 1); 4])).collect();
        let definition = vec![(DEFINITION.0, DEFINITION.1.to_vec())];
        let mut objects: Vec<Vec<u8>> = (1..20).map(|k| w.record(0x1000, &instance(&w, level(k), DVec3::ZERO), &attributes(member(k), 0, &definition), &[])).collect();
        objects.push(w.record(0x1000, &instance(&w, level(0), DVec3::ZERO), &attributes(id(70), 0, &[]), &[]));
        objects.push(w.record(0x20, &triangle(&w), &attributes(id(71), 0, &[]), &[]));
        let file = w.file(70, 2_400_000_000, 4, &[layers, w.table(TCODE_INSTANCE_DEFINITION_TABLE, &blocks), w.table(TCODE_OBJECT_TABLE, &objects)]);
        let (model, info) = load("wide.3dm", file);
        assert_eq!(model.triangle_count(), 1);
        assert!(entry(&info, "Rhino objects left out").contains("1 block instances cut short (too many parts)"), "{info:?}");
    }

    /// An `ON_Mesh` 3.x of one triangle whose vertex array is `positions` (size, CRC, method and
    /// bytes as `compressed` writes them), claiming `vcount` vertices.
    fn mesh_with(vcount: i32, positions: &[u8]) -> Vec<u8> {
        cat(&[&[0x30], &i32b(vcount), &i32b(1), &[0; 153], &i32b(4), &[0, 1, 2, 2].map(i32b).concat(), positions, &[0; 16]])
    }

    fn read_test_mesh(data: &[u8]) -> R<Mesh> {
        let mut c = Cur { data, pos: 0, v: Versions { archive: 70, opennurbs: 2_400_000_000 } };
        read_mesh(&mut c, &mut 0)
    }

    #[test]
    fn compressed_arrays_are_checked_before_inflating() {
        let w = W { big: true };
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        z.write_all(&vec![0; 1 << 20]).unwrap();
        let zeros = z.finish().unwrap();
        // a few kilobytes that claim almost 4 GiB for 3 vertices: not inflated
        let bomb = cat(&[&0xFFFF_0000u32.to_le_bytes(), &[0; 4], &[1], &w.chunk(TCODE_ANONYMOUS_CHUNK, &zeros)]);
        assert_eq!(read_test_mesh(&mesh_with(3, &bomb)).unwrap_err(), "Rhino 3dm: mesh vertices of the wrong size");
        // a size that matches the vertex count but more than the stream can inflate to
        let packed = cat(&[&(12u32 * 1_000_000).to_le_bytes(), &[0; 4], &[1], &w.chunk(TCODE_ANONYMOUS_CHUNK, &zeros[..64])]);
        assert_eq!(read_test_mesh(&mesh_with(1_000_000, &packed)).unwrap_err(), "Rhino 3dm: compressed data larger than its stream can hold");
        // more vertices than the viewer reads
        assert_eq!(read_test_mesh(&mesh_with(i32::MAX, &bomb)).unwrap_err(), TOO_MANY_VERTICES);
        // the well-formed mesh reads
        let points = w.compressed(&[[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]].iter().flatten().flat_map(|f| f.to_le_bytes()).collect::<Vec<u8>>(), true);
        let mesh = read_test_mesh(&mesh_with(3, &points)).unwrap();
        assert_eq!((mesh.positions.len(), mesh.triangles), (3, 1));
        // no faces and a face index size that would overflow a 32-bit multiplication
        let empty = cat(&[&[0x30], &i32b(3), &i32b(0), &[0; 153], &i32b(i32::MAX)]);
        assert!(read_test_mesh(&empty).unwrap().faces.is_empty());
    }

    #[test]
    fn user_text_counts_need_room_for_their_entries() {
        // 1000 entries in 8000 bytes: each entry needs at least 16
        let w = W { big: false };
        let list = w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &cat(&[&i32b(1000), &vec![0; 8000]]));
        let c = Cur { data: &list, pos: 0, v: Versions { archive: 4, opennurbs: 0 } };
        assert_eq!(user_strings(c).unwrap_err(), "Rhino 3dm: a count larger than its record");
    }

    #[test]
    fn layer_parent_cycles_end() {
        let w = W { big: true };
        let layers = w.table(TCODE_LAYER_TABLE, &[
            w.layer(0, "A", [0, 0, 0, 0], true, id(1), id(2), -1),
            w.layer(1, "B", [0, 0, 0, 0], false, id(2), id(1), -1),
            w.layer(2, "C", [0, 0, 0, 0], true, id(3), id(3), -1),
        ]);
        let objects = w.table(TCODE_OBJECT_TABLE, &[w.record(0x20, &triangle(&w), &attributes(id(10), 0, &[]), &[]), w.record(0x20, &triangle(&w), &attributes(id(11), 2, &[]), &[])]);
        let (model, _) = load("cycle.3dm", w.file(70, 2_400_000_000, 4, &[layers, objects]));
        let names: Vec<(&str, bool)> = model.layers.iter().map(|l| (l.name.as_str(), l.hidden)).collect();
        assert_eq!(names, [("B::A", true), ("C", false)]);
    }

    #[test]
    fn block_instances_take_layer_visibility_and_paint_from_parent() {
        let w = W { big: true };
        let layers = w.table(TCODE_LAYER_TABLE, &[
            w.layer(0, "Default", [0, 0, 0, 0], true, id(1), Uuid::default(), -1),
            w.layer(1, "Furniture", [0, 0, 200, 0], false, id(2), Uuid::default(), -1),
            w.layer(2, "Green", [0, 200, 0, 0], true, id(3), Uuid::default(), -1),
            w.layer(3, "Glass", [100, 100, 100, 0], true, id(4), Uuid::default(), 0),
        ]);
        let glass = cat(&[&w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &cat(&[&0x0Cu32.to_le_bytes(), &i32b(0), &wstr("Glass")])), &[0; 16], &[0; 4], &[0, 0, 255, 0], &[0; 16], &f64s(&[1.0, 0.0, 0.0, 0.5])]);
        let materials = w.table(TCODE_MATERIAL_TABLE, &[w.chunk(TCODE_MATERIAL_RECORD, &w.object(ON_MATERIAL, &w.versioned(TCODE_ANONYMOUS_CHUNK, 1, 0, &glass), &[]))]);
        let blocks = w.table(TCODE_INSTANCE_DEFINITION_TABLE, &[block(&w, id(50), "Chair", &[id(60)])]);
        // the chair: red by object, material by parent, on the default layer
        let red = [(12, vec![3]), (6, vec![255, 0, 0, 0]), (13, vec![1]), (16, vec![3])];
        let objects = w.table(TCODE_OBJECT_TABLE, &[
            w.record(0x20, &triangle(&w), &attributes(id(60), 0, &red), &[]),
            // on a layer that is off: the chair goes to that (hidden) layer
            w.record(0x1000, &instance(&w, id(50), DVec3::ZERO), &attributes(id(70), 1, &[]), &[]),
            // on a green layer without a material: the chair stays red
            w.record(0x1000, &instance(&w, id(50), DVec3::Z), &attributes(id(71), 2, &[]), &[]),
            // material by parent outside a block: the layer's material
            w.record(0x20, &triangle(&w), &attributes(id(72), 3, &[(16, vec![3])]), &[]),
        ]);
        let (model, _) = load("chairs.3dm", w.file(70, 2_400_000_000, 4, &[materials, layers, blocks, objects]));
        let names: Vec<(&str, bool, usize)> = model.layers.iter().map(|l| (l.name.as_str(), l.hidden, l.triangle_count())).collect();
        assert_eq!(names, [("Default", false, 1), ("Furniture", true, 1), ("Glass", false, 1)]);
        assert_eq!(model.layers[0].colors.as_ref().unwrap(), &[[255, 0, 0, 255]; 3]);
        assert_eq!(model.layers[2].colors.as_ref().unwrap(), &[[0, 0, 255, 128]; 3]);
    }

    #[test]
    fn survey_coordinates_move_to_a_local_origin() {
        // a block placed and a point cloud at Dutch RD coordinates, in metres
        let w = W { big: true };
        let layers = w.table(TCODE_LAYER_TABLE, &[w.layer(0, "Default", [0, 0, 0, 0], true, id(1), Uuid::default(), -1)]);
        let blocks = w.table(TCODE_INSTANCE_DEFINITION_TABLE, &[block(&w, id(50), "Tree", &[id(60)])]);
        let points = [155_000.125, 463_000.0625, 1.0, 155_010.5, 463_020.25, 2.0];
        let cloud = w.object(ON_POINT_CLOUD, &cat(&[&[0x10], &i32b(2), &f64s(&points), &[0; 180]]), &[]);
        let objects = w.table(TCODE_OBJECT_TABLE, &[
            w.record(0x20, &triangle(&w), &attributes(id(60), 0, &[(DEFINITION.0, DEFINITION.1.to_vec())]), &[]),
            w.record(0x1000, &instance(&w, id(50), DVec3::new(155_000.25, 463_000.5, 0.0)), &attributes(id(70), 0, &[]), &[]),
            w.record(0x2, &cloud, &attributes(id(71), 0, &[]), &[]),
        ]);
        let (model, info) = load("rd.3dm", w.file(70, 2_400_000_000, 4, &[layers, blocks, objects]));
        assert_eq!(entry(&info, "Rhino world origin (metres)"), "155000, 463000, 0");
        let mesh = &model.layers[0];
        assert_eq!(mesh.positions[..6], [0.25, 0.5, 0.0, 1.25, 0.5, 0.0]);
        let cloud = &model.layers[1];
        assert_eq!(cloud.positions, [0.125, 0.0625, 1.0, 10.5, 20.25, 2.0]);
    }
}
