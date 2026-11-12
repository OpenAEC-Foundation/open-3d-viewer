//! Archicad projects (.pln) and archives (.pla) of Archicad 24 to 29: walls (with their windows
//! and doors cut out), slabs and columns as extruded footprints, beams along their axis, roof
//! planes, library objects as boxes, meshes as terrain bodies and zones, grouped per story. Written
//! from a clean-room specification of the file structure (SPEC v0.9, section numbers in the
//! comments): a container of 16-byte items with QuickLZ groups, object records with associations,
//! stories, the element polygon, the wall fields, columns and beams with their cross-sections
//! (rectangular, round or an own profile), roof planes, windows and doors as openings, the place,
//! size and height of library objects, meshes with their triangulation cache, and zones. Window and
//! door frames, the shapes of library objects (code), curved roofs and zones cut by roofs are not
//! read yet.
//!
//! Archicad is Z up in metres, as the viewer: coordinates are taken as they are.

use super::{Loaded, Loader, Phase, Progress, Source};
use crate::model::{Layer, ProjectData};
use std::collections::HashMap;
use std::io::{BufReader, Cursor, Read};

pub struct ArchicadLoader;

impl Loader for ArchicadLoader {
    fn name(&self) -> &'static str {
        "Archicad"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["pln", "pla"]
    }

    fn load(&self, source: &Source) -> Result<Vec<Layer>, String> {
        self.load_with(source, &Progress::default()).map(|loaded| loaded.layers)
    }

    fn load_with(&self, source: &Source, progress: &Progress) -> Result<Loaded, String> {
        progress.set(Phase::Read);
        let records = read_source(source)?;
        progress.set(Phase::Convert);
        convert(records).map(|(loaded, _)| loaded)
    }
}

/// An object's identity: 16 bytes as they are in the file.
type Guid = [u8; 16];

/// A GUID written as 32 hex digits in file order.
const fn guid(hex: &str) -> Guid {
    const fn nibble(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            _ => panic!("not a hex digit"),
        }
    }
    let h = hex.as_bytes();
    let mut out = [0; 16];
    let mut i = 0;
    while i < 16 {
        out[i] = nibble(h[2 * i]) << 4 | nibble(h[2 * i + 1]);
        i += 1;
    }
    out
}

// §5: the classes the reader uses, the same in Archicad 24 to 29.
const WALL: Guid = guid("8245654faf33524eabfcc35d46765ec4");
const SLAB: Guid = guid("75125c944879d44187449d22d72ca486");
const STORY: Guid = guid("256b04bcd6271a46a6e56fa0e8594a95");
const STORY_LEVEL: Guid = guid("a2ca396b1e60744c9955f90ee308af98");
/// The association "element stands on story" (op 0x12 in the element, target the story).
const ON_STORY: Guid = guid("0312b67984ae884da9c620ead705e0bc");
/// §8.3 (v0.2): the association "top linked" (op 0x11 or 0x12 in a wall, target a link object).
const TOP_LINK: Guid = guid("47cc487b6772fd4c91339214ea18fb99");
/// §8.3 (v0.2): the class of the link object between a wall and the story its top is linked to.
const LINK: Guid = guid("4d04d664e803ec42bd568813cc6481cb");
/// §8.3 (v0.2): the association of a link object whose target is the top story.
const LINK_STORY: Guid = guid("805bf9ba56f23249b89103ff66848591");
// §5, §8.5–§8.7 (v0.3): columns and beams, their segments and cross-sections.
const COLUMN: Guid = guid("85ebe813335aa347a4d41e05296aec7e");
const BEAM: Guid = guid("85d2c2a57c5a684a95549eda9d4556ca");
const COLUMN_SEGMENT: Guid = guid("ab821a7acd69e640a7d876564f981cf9");
const BEAM_SEGMENT: Guid = guid("c67babcb3ae033419161c28ed3368672");
const SECTION_RECT: Guid = guid("551919c4c687b64fb0745f094fe9f6a6");
const SECTION_ROUND: Guid = guid("3929c1d605d9e040aa57f7c8d687dc7b");
const SECTION_PROFILE: Guid = guid("40ae258827f8574aa1c549d04c687c46");
/// §8.7: the association from a column, or a beam, to its segment.
const COLUMN_SEGMENT_LINK: Guid = guid("21864427dc9ae643b5aafc76e4cd62ce");
const BEAM_SEGMENT_LINK: Guid = guid("a72c76d520d95d45a37328ba6d47152a");
/// §8.7: the association from a segment to its cross-section.
const SECTION_LINK: Guid = guid("306cb86321a21c4284fb926c694bf611");
/// §8.5: the association from a column to the link object of its top (as walls, §8.3).
const COLUMN_TOP_LINK: Guid = guid("2dbcbe5e0bcf51478f557c2353c3fd14");
/// §5, §8.8 (v0.4): a roof plane (a single-plane roof, or a plane of a multi-plane roof).
const ROOF: Guid = guid("21764c9de9512345a3baf2121777bcbe");
/// §5, §8.9 (v0.5): windows and doors, and the associations from a wall to them.
const WINDOW: Guid = guid("a061f1934e4cf74da1004fd0e8c4f1e1");
const DOOR: Guid = guid("8976f0d8fa9cbe4faeb40a60b8e667ee");
const WALL_WINDOWS: Guid = guid("e67943c5d0af9a4b9887af5baeeccf84");
const WALL_DOORS: Guid = guid("02fe6b1f0cd6ae409ac8c3953ce12814");
/// §8.9: more openings in one wall are skipped and counted, so that cutting them stays fast
/// (open question Q-I25).
const MAX_OPENINGS: usize = 1000;
/// §5, §8.10 (v0.6): placed library objects, drawn as boxes.
const OBJECT: Guid = guid("c3ceefdbbf1b6748b8b3c4f7c859df61");
/// §8.10: the associations from an object to its 2D outline cache and to its library reference,
/// and from the library reference to the parameter block.
const OBJECT_OUTLINE: Guid = guid("bb882d2d03111b4282f1df5f07d5f5ce");
const OBJECT_LIBRARY: Guid = guid("3e459f182eea384eafe9660d1d25afc6");
const LIBRARY_PARAMETERS: Guid = guid("a4f0eb76efc23f48a1a828d3adde99a0");
/// §5 (v0.7): the classes of an object's outline cache, library reference and parameter block.
const OUTLINE: Guid = guid("d3858d46688b6d48903456beaad7ad95");
const LIBRARY: Guid = guid("3da3fc2b38bde349bf6d52ac8b850b9f");
const PARAMETERS: Guid = guid("7d22db11e4dbaf4588b66fddea993f71");
/// §5, §8.11 (v0.8): meshes (terrain), their triangulation cache and the association to it.
const MESH: Guid = guid("46ee2f8f3f6aac41a32b69e44dc6276d");
const MESH_CACHE: Guid = guid("b898e0c2e9a8bb4696c57200ead406f5");
const MESH_CACHE_LINK: Guid = guid("f876b988fd101e41891a5ecca8c7b2a7");
/// §5, §8.12 (v0.8): zones, and the association from a zone to the link object of its top.
const ZONE: Guid = guid("2e33d03ae2ef1a4992a65b14bfdd7229");
const ZONE_TOP_LINK: Guid = guid("563b426cda1e0540be1f6f8942311a7f");
/// §5, §8.13 (v0.8): the profile object of a profile cross-section, and the association to it.
const PROFILE: Guid = guid("d72bcdf4b1d4bf4ab13c59001907bbbd");
const PROFILE_LINK: Guid = guid("443b07ca55067d4097151a019e628139");
/// §5, §8.16.3 (v0.9): morphs, their body and the association to it.
const MORPH: Guid = guid("7b3c36e60e94eb4daf958b3b39e86bd2");
const MORPH_BODY: Guid = guid("ad8c58f158255c49820bf3bcf4651007");
const MORPH_BODY_LINK: Guid = guid("6bfa7fd50fc17f4fabe60fd52f2cc23e");
/// §5, §8.14 (v0.9): stairs, their scheme and the association to it.
const STAIR: Guid = guid("157f0ffab429054980fac6769109847b");
const STAIR_SCHEME: Guid = guid("9c2b80871011ba4dbf9c9eb8278960b0");
const STAIR_SCHEME_LINK: Guid = guid("3f141750bd7ce14683e68cf63b89a01c");
/// §5, §8.15 (v0.9): railings, their segments and caches, and the associations to them.
const RAILING: Guid = guid("1ae65a4c17af854b93e0005c987d5f5b");
const RAILING_SEGMENT: Guid = guid("9995f52972f9f744af1d1f24770725c3");
const SEGMENT_CACHE: Guid = guid("ae19929d03b4784b94ec589f66755d31");
const RAILING_CACHE: Guid = guid("70d5fb2294540647a3dc818ba797dc96");
const RAILING_SEGMENTS: Guid = guid("0d60448f1720d848aa573802cf7e9cc0");
const SEGMENT_CACHE_LINK: Guid = guid("3abf0f62397fd0439062793244e2e5f8");
const RAILING_CACHE_LINK: Guid = guid("e727811bef815a4c89ec484ca6e0ecbc");
/// §6: the unnamed story above the top story; its level counts, it is not shown.
const TOP_STORY: Guid = guid("f5c28e4f85caa148b54067746b76d419");

// Layouts (not in the specification; found in the files): a layout and its master are records
// whose 2D elements name them by an association; a drawing on a layout names the view it shows.
const LAYOUT: Guid = guid("3cafac32b8269345926cf81a205550ba");
const LINE_2D: Guid = guid("737dfebd781cca4a809b1b25747e3126");
const POLYLINE_2D: Guid = guid("228bdcb06dadf14b97335077b6588fc4");
const TEXT_2D: Guid = guid("ae7ec0e62a5a974595a973e7543ac91e");
const DRAWING: Guid = guid("528cbe2db393b546a0a92492c45c2ab3");
const DRAWING_SOURCE: Guid = guid("e30734bafd8ab74e9f53f197ff35c20e");
const VIEW: Guid = guid("9ac5e679eea35f41bb0e20f2930f9405");
/// The association from a 2D element to the layout or master layout it is drawn on.
const ON_LAYOUT: Guid = guid("0b279bbd4703cf4d92cfd2d872117ba1");
/// The associations from a drawing to its source, and from that to the view it shows.
const DRAWING_SOURCE_LINK: Guid = guid("48a34c00da048b4fbf9422d0e313040b");
const SOURCE_VIEW_LINK: Guid = guid("ab1c2077f6664749831dc851ba54217d");
/// The 2D elements of layouts kept at most: a bound on what a damaged file can ask for.
const MAX_LAYOUT_PARTS: usize = 200_000;

/// §1: the signature of a project and an archive.
const SIGNATURE: &[u8; 8] = b"ROF FDB ";
/// §2: a compressed group.
const GROUP: Guid = [0x79, 0xA8, 0xA9, 0x96, 0x45, 0xB4, 0x4A, 0xF3, 0x98, 0x38, 0x33, 0xC3, 0x3A, 0x06, 0x1B, 0x06];
/// §3: a free block.
const FREE_BLOCK: u16 = 0xFFDC;
/// §3: larger decoded payloads are taken to be damaged. Every buffer sized from a length in the
/// file is capped by this or by [`MAX_GROUP_CHUNK`], so a damaged or hostile file cannot make the
/// reader allocate more.
const MAX_PAYLOAD: usize = 64 << 20;
/// A larger block is skipped (and counted) without being read into memory.
const MAX_BLOCK: u64 = MAX_PAYLOAD as u64;
/// §2.2: a chunk of a group decodes to at most 1 MiB; allow some room (compressed and decoded).
const MAX_GROUP_CHUNK: u32 = 16 << 20;
/// §7, §8.2: the data of a wall or slab is a few kB; larger records are skipped, as the polygon
/// search tries every offset.
const MAX_ELEMENT_DATA: usize = 1 << 20;

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at.checked_add(2)?)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

fn i32_at(b: &[u8], at: usize) -> Option<i32> {
    u32_at(b, at).map(|v| v as i32)
}

fn f64_at(b: &[u8], at: usize) -> Option<f64> {
    Some(f64::from_le_bytes(b.get(at..at.checked_add(8)?)?.try_into().ok()?))
}

fn guid_at(b: &[u8], at: usize) -> Option<Guid> {
    b.get(at..at.checked_add(16)?)?.try_into().ok()
}

/// `n` bytes at `at`, when they are all there.
fn bytes_at(b: &[u8], at: usize, n: usize) -> Option<&[u8]> {
    b.get(at..at.checked_add(n)?)
}

fn hex(g: &Guid) -> String {
    g.iter().map(|b| format!("{b:02x}")).collect()
}

/// An element record (wall, slab, column or beam): its identity, story and data.
struct Record {
    guid: Guid,
    story: Option<Guid>,
    /// Walls and columns: the link object of a top linked to a story (§8.3, §8.5).
    top_link: Option<Guid>,
    /// Columns and beams: the segment that leads to the cross-section (§8.7).
    segment: Option<Guid>,
    /// Walls: their windows and doors (§8.9); railings: their segments (§8.15).
    openings: Vec<Guid>,
    /// Meshes: their triangulation cache (§8.11); morphs: their body (§8.16.3); stairs: their
    /// scheme (§8.14); railings: their cache with the bounding box (§8.15).
    cache: Option<Guid>,
    data: Vec<u8>,
}

/// §8.7: the cross-section of a column or beam.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Section {
    /// Width b and depth d (of a beam: its height).
    Rect(f64, f64),
    /// Diameters b and d.
    Round(f64, f64),
    /// An own 2D profile (§8.13): the profile object it refers to.
    Profile(Option<Guid>),
    /// Data that does not read as a section.
    Unreadable,
}

/// §8.7: a rectangular or round section: b and d at 40 (data[0] = 0x82) or 52 (0x83), both
/// between 0 and 50 m.
fn section(class: Guid, data: &[u8]) -> Section {
    let at = match data.first() {
        Some(0x82) => 40,
        Some(0x83) => 52,
        _ => return Section::Unreadable,
    };
    let (Some(b), Some(d)) = (f64_at(data, at), f64_at(data, at + 8)) else { return Section::Unreadable };
    let size = 0.0..50.0;
    if !(size.contains(&b) && size.contains(&d) && b > 0.0 && d > 0.0) {
        return Section::Unreadable;
    }
    if class == SECTION_ROUND {
        Section::Round(b, d)
    } else {
        Section::Rect(b, d)
    }
}

/// What the reader keeps of the file: the records of the classes it uses.
#[derive(Default)]
struct Records {
    /// Story: name and index (0 the ground floor, negative below).
    stories: HashMap<Guid, (String, i16)>,
    /// Story levels: the targets of their 0x12 associations and the elevation (m).
    levels: Vec<(Vec<Guid>, f64)>,
    /// Link objects (§8.3): the targets of their story association.
    top_links: HashMap<Guid, Vec<Guid>>,
    walls: Vec<Record>,
    slabs: Vec<Record>,
    columns: Vec<Record>,
    beams: Vec<Record>,
    roofs: Vec<Record>,
    /// Windows and doors (§8.9): whether a door, and the data.
    openings: HashMap<Guid, (bool, Vec<u8>)>,
    /// Column and beam segments (§8.7): the first target of their section association.
    segments: HashMap<Guid, Option<Guid>>,
    sections: HashMap<Guid, Section>,
    /// Elements skipped before conversion: data larger than [`MAX_ELEMENT_DATA`].
    walls_too_large: usize,
    slabs_too_large: usize,
    columns_too_large: usize,
    beams_too_large: usize,
    roofs_too_large: usize,
    /// Placed library objects (§8.10): those with a story association; the others are 2D symbols.
    objects: Vec<ObjectRecord>,
    objects_too_large: usize,
    /// Meshes (§8.11) and zones (§8.12).
    meshes: Vec<Record>,
    zones: Vec<Record>,
    meshes_too_large: usize,
    zones_too_large: usize,
    /// Triangulation caches of meshes, read as they come (`None`: one that does not read).
    mesh_caches: HashMap<Guid, Option<MeshCache>>,
    /// Profile objects (§8.13), read as they come (`None`: a header that does not read).
    profiles: HashMap<Guid, Option<ProfileShape>>,
    /// Morphs (§8.16.3) and their bodies, read as they come (`None`: one that does not read);
    /// several morphs may share a body.
    morphs: Vec<Record>,
    morphs_too_large: usize,
    bodies: HashMap<Guid, Option<Body>>,
    /// Stairs (§8.14) and their schemes, read as they come (`None`: one that does not read).
    stairs: Vec<Record>,
    stairs_too_large: usize,
    schemes: HashMap<Guid, Option<StairScheme>>,
    /// Railings (§8.15); their segments (the cache each refers to), the segment caches (`None`:
    /// one that does not read) and the railing caches (the bounding box).
    railings: Vec<Record>,
    railings_too_large: usize,
    railing_segments: HashMap<Guid, Option<Guid>>,
    segment_caches: HashMap<Guid, Option<SegmentLines>>,
    railing_boxes: HashMap<Guid, Option<[f64; 6]>>,
    /// §8.10, by identity, only what the objects need (a parameter block runs to ~80 kB, and a
    /// large project has thousands): of an outline cache its bounds (when they read) and its
    /// (x, y) bytes; of a library reference its parameter block and name; of a parameter block
    /// its ZZYZX.
    outlines: HashMap<Guid, Outline>,
    libraries: HashMap<Guid, (Option<Guid>, Option<String>)>,
    heights: HashMap<Guid, Option<f64>>,
    /// Layouts: their name, number and master layout.
    layouts: Vec<Layout>,
    /// What is drawn on layouts and masters, by the layout or master it names.
    layout_parts: Vec<(Guid, LayoutPart)>,
    /// The sources of drawings (the view each shows) and the names of views.
    drawing_views: HashMap<Guid, Guid>,
    view_names: HashMap<Guid, String>,
    /// Blocks of the classes above whose payload could not be decoded, blocks larger than
    /// [`MAX_BLOCK`] and group chunks that did not decompress.
    damaged: usize,
}

/// A placed library object (§8.10).
struct ObjectRecord {
    guid: Guid,
    story: Guid,
    /// The 2D outline cache and the library reference, the first target of each association.
    outline: Option<Guid>,
    library: Option<Guid>,
    data: Vec<u8>,
}

/// §8.10: what the reader keeps of an outline cache.
struct Outline {
    /// xmin, xmax, ymin, ymax, when they read.
    bounds: Option<[f64; 4]>,
    /// The object's (x, y), as bytes to find in the object's data.
    xy: [u8; 16],
}

/// Reads the file as a stream, in one pass: projects run to hundreds of MB, mostly library content.
fn read_source(source: &Source) -> Result<Records, String> {
    match source {
        Source::File(path) => {
            let file = std::fs::File::open(path).map_err(|e| format!("Cannot read {}: {e}", source.name()))?;
            read(BufReader::with_capacity(1 << 20, file), source.len())
        }
        Source::Bytes { data, .. } => read(Cursor::new(data.as_slice()), data.len() as u64),
    }
}

fn read_exact(r: &mut impl Read, buf: &mut [u8], at: u64) -> Result<(), String> {
    r.read_exact(buf).map_err(|e| format!("Archicad: cannot read at offset {at}: {e}"))
}

fn skip(r: &mut impl Read, n: u64, at: u64) -> Result<(), String> {
    let skipped = std::io::copy(&mut r.by_ref().take(n), &mut std::io::sink()).map_err(|e| format!("Archicad: cannot read at offset {at}: {e}"))?;
    if skipped == n {
        Ok(())
    } else {
        Err(format!("Archicad: the file ends inside the item at offset {at}"))
    }
}

/// §1–§2: the signature, then the items of the outer layer up to the end of the file.
fn read(mut r: impl Read, len: u64) -> Result<Records, String> {
    let mut head = [0u8; 32];
    if r.read_exact(&mut head).is_err() || &head[..8] != SIGNATURE {
        // §10: before Archicad 17 the files were of another format
        return Err(if [b"MM", b"WW", b"mm"].iter().any(|s| head.starts_with(*s)) {
            "Archicad: files from before Archicad 17 are not supported".into()
        } else {
            "Archicad: not an Archicad project or archive (no ROF FDB signature)".into()
        });
    }
    let mut records = Records::default();
    let mut o = 32u64;
    let mut item = [0u8; 16];
    while len - o >= 16 {
        read_exact(&mut r, &mut item, o)?;
        if item[..2] == *b"BL" {
            let size = block_size(&item, o)?;
            if size > len - o {
                return Err(format!("Archicad: the block at offset {o} runs past the end of the file"));
            }
            if wanted(&item) && size <= MAX_BLOCK {
                let mut block = vec![0u8; size as usize];
                block[..16].copy_from_slice(&item);
                read_exact(&mut r, &mut block[16..], o)?;
                records.block(&block);
            } else {
                records.damaged += usize::from(wanted(&item));
                skip(&mut r, size - 16, o)?;
            }
            o += size;
        } else if item == [0; 16] {
            o += 16;
        } else if item == GROUP {
            o = read_group(&mut r, o + 16, len, &mut records)?;
        } else {
            return Err(format!("Archicad: unknown item at offset {o}"));
        }
    }
    Ok(records)
}

/// The length of a block from its first 16 bytes (§3).
fn block_size(item: &[u8], at: u64) -> Result<u64, String> {
    match u32_at(item, 2) {
        Some(0) | None => Err(format!("Archicad: block without length at offset {at}")),
        Some(units) => Ok(u64::from(units) * 16),
    }
}

/// Whether a block may hold a record the reader uses: not free, and with a payload (§3).
fn wanted(item: &[u8]) -> bool {
    u16_at(item, 6) != Some(FREE_BLOCK) && matches!(item[10], 0x84..=0x86)
}

/// §2.1: a compressed group, from just after its magic; returns the offset after it. The group is
/// decompressed chunk by chunk and each block handled once it is complete.
fn read_group(r: &mut impl Read, mut o: u64, len: u64, records: &mut Records) -> Result<u64, String> {
    let start = o - 16;
    let mut head = [0u8; 16];
    read_exact(r, &mut head, o)?;
    o += 16;
    let decoded = u64::from_le_bytes(head[..8].try_into().unwrap());
    let chunks = u64::from_le_bytes(head[8..].try_into().unwrap());
    let mut stream = GroupStream { size: decoded, ..GroupStream::default() };
    let mut damaged = false;
    for _ in 0..chunks {
        let mut n = [0u8; 4];
        read_exact(r, &mut n, o)?;
        o += 4;
        let n = u64::from(u32::from_le_bytes(n));
        if n > len.saturating_sub(o) {
            return Err(format!("Archicad: the group at offset {start} runs past the end of the file"));
        }
        if damaged || n > u64::from(MAX_GROUP_CHUNK) {
            records.damaged += usize::from(!damaged);
            damaged = true;
            skip(r, n, o)?;
            o += n;
            continue;
        }
        let mut chunk = vec![0u8; n as usize];
        read_exact(r, &mut chunk, o)?;
        o += n;
        // a damaged chunk breaks the rest of its group, not the file
        match quicklz::decompress(&mut Cursor::new(&chunk[..]), MAX_GROUP_CHUNK) {
            Ok(out) => stream.feed(&out, records).map_err(|e| format!("{e} (group at offset {start})"))?,
            Err(_) => {
                damaged = true;
                records.damaged += 1;
            }
        }
    }
    let mut pad = [0u8; 2];
    read_exact(r, &mut pad, o)?;
    o += 2;
    let pad = u64::from(u16::from_le_bytes(pad));
    if pad > len.saturating_sub(o) {
        return Err(format!("Archicad: the group at offset {start} runs past the end of the file"));
    }
    skip(r, pad, o)?;
    Ok(o + pad)
}

/// The decompressed bytes of a group, parsed into blocks as they come in (§2.1): only the part of
/// a block not yet complete is kept.
#[derive(Default)]
struct GroupStream {
    /// The decoded size of the group.
    size: u64,
    /// Decoded bytes handled so far (items done, plus the start of `buf`).
    done: u64,
    buf: Vec<u8>,
    /// Bytes of a block that is not needed, still to come.
    skip: u64,
}

impl GroupStream {
    fn feed(&mut self, bytes: &[u8], records: &mut Records) -> Result<(), String> {
        self.buf.extend_from_slice(bytes);
        let mut at = 0usize;
        loop {
            if self.skip > 0 {
                let k = self.skip.min((self.buf.len() - at) as u64);
                at += k as usize;
                self.done += k;
                self.skip -= k;
                if self.skip > 0 {
                    break;
                }
            }
            let item = &self.buf[at..];
            if item.len() < 16 {
                break;
            }
            if item[..2] == *b"BL" {
                let size = block_size(item, self.done)?;
                if size > self.size - self.done.min(self.size) {
                    return Err(format!("Archicad: block at {} runs past the end of its group", self.done));
                }
                // not needed, or too large to keep: let it stream past
                if !wanted(item) || size > MAX_BLOCK {
                    records.damaged += usize::from(wanted(item));
                    self.skip = size;
                    continue;
                }
                if (item.len() as u64) < size {
                    break;
                }
                records.block(&item[..size as usize]);
                at += size as usize;
                self.done += size;
            } else if item[..16] == [0; 16] {
                at += 16;
                self.done += 16;
            } else {
                return Err(format!("Archicad: unknown item at {} in a compressed group", self.done));
            }
        }
        self.buf.drain(..at);
        Ok(())
    }
}

impl Records {
    /// §3–§4: one block, kept when it is a record of a class the reader uses.
    fn block(&mut self, b: &[u8]) {
        let Some(object) = guid_at(b, 11) else { return };
        let classes = [
            MESH, MESH_CACHE, ZONE, PROFILE, MORPH, MORPH_BODY, STAIR, STAIR_SCHEME, RAILING, RAILING_SEGMENT, SEGMENT_CACHE, RAILING_CACHE,
            WALL, SLAB, STORY, STORY_LEVEL, LINK, COLUMN, BEAM, ROOF, WINDOW, DOOR, OBJECT, OUTLINE, LIBRARY, PARAMETERS, COLUMN_SEGMENT,
            BEAM_SEGMENT, SECTION_RECT, SECTION_ROUND, SECTION_PROFILE, LAYOUT, LINE_2D, POLYLINE_2D, TEXT_2D, DRAWING,
            DRAWING_SOURCE, VIEW,
        ];
        match payload(b, |class| classes.contains(class)) {
            Payload::Data(p) => self.record(object, &p),
            Payload::Damaged => self.damaged += 1,
            Payload::Other => {}
        }
    }

    /// §4: an object record of a class the reader uses.
    fn record(&mut self, object: Guid, p: &[u8]) {
        let Some((class, data, assoc)) = record_parts(p) else { return };
        let links = links(assoc);
        match class {
            OBJECT => {
                // §8.10: placed objects have a story; the others are 2D symbols and not counted
                let Some(story) = links.iter().find(|l| l.0 == 0x12 && l.1 == ON_STORY).map(|l| l.2) else { return };
                if data.len() > MAX_ELEMENT_DATA {
                    self.objects_too_large += 1;
                    return;
                }
                let first = |assoc: Guid| links.iter().find(|l| l.1 == assoc).map(|l| l.2);
                let (outline, library) = (first(OBJECT_OUTLINE), first(OBJECT_LIBRARY));
                self.objects.push(ObjectRecord { guid: object, story, outline, library, data: data.to_vec() });
            }
            WALL | SLAB | COLUMN | BEAM | ROOF | MESH | ZONE | MORPH | STAIR | RAILING => {
                let first = |assoc: Guid, op: &[u8]| links.iter().find(|l| op.contains(&l.0) && l.1 == assoc).map(|l| l.2);
                let story = first(ON_STORY, &[0x12]);
                let (top_link, segment) = match class {
                    WALL => (first(TOP_LINK, &[0x11, 0x12]), None),
                    COLUMN => (first(COLUMN_TOP_LINK, &[0x11, 0x12]), first(COLUMN_SEGMENT_LINK, &[0x11, 0x12])),
                    BEAM => (None, first(BEAM_SEGMENT_LINK, &[0x11, 0x12])),
                    ZONE => (first(ZONE_TOP_LINK, &[0x11, 0x12]), None),
                    _ => (None, None),
                };
                let cache = match class {
                    MESH => first(MESH_CACHE_LINK, &[0x11, 0x12]),
                    MORPH => first(MORPH_BODY_LINK, &[0x11, 0x12]),
                    STAIR => first(STAIR_SCHEME_LINK, &[0x11, 0x12]),
                    RAILING => first(RAILING_CACHE_LINK, &[0x11, 0x12]),
                    _ => None,
                };
                let (list, too_large) = match class {
                    WALL => (&mut self.walls, &mut self.walls_too_large),
                    SLAB => (&mut self.slabs, &mut self.slabs_too_large),
                    COLUMN => (&mut self.columns, &mut self.columns_too_large),
                    BEAM => (&mut self.beams, &mut self.beams_too_large),
                    MESH => (&mut self.meshes, &mut self.meshes_too_large),
                    ZONE => (&mut self.zones, &mut self.zones_too_large),
                    MORPH => (&mut self.morphs, &mut self.morphs_too_large),
                    STAIR => (&mut self.stairs, &mut self.stairs_too_large),
                    RAILING => (&mut self.railings, &mut self.railings_too_large),
                    _ => (&mut self.roofs, &mut self.roofs_too_large),
                };
                if data.len() > MAX_ELEMENT_DATA {
                    *too_large += 1;
                    return;
                }
                // §8.9: the windows and doors of a wall, any op, every target
                let openings = match class {
                    WALL => links.iter().filter(|l| l.1 == WALL_WINDOWS || l.1 == WALL_DOORS).map(|l| l.2).collect(),
                    // §8.15: the segments of a railing, in order along it (an item with n targets)
                    RAILING => links.iter().filter(|l| (l.0 == 0x01 || l.0 == 0x02) && l.1 == RAILING_SEGMENTS).map(|l| l.2).collect(),
                    _ => Vec::new(),
                };
                list.push(Record { guid: object, story, top_link, segment, openings, cache, data: data.to_vec() });
            }
            MESH_CACHE => {
                self.mesh_caches.insert(object, mesh_cache(data));
            }
            WINDOW | DOOR => {
                if data.len() <= MAX_ELEMENT_DATA {
                    self.openings.insert(object, (class == DOOR, data.to_vec()));
                }
            }
            COLUMN_SEGMENT | BEAM_SEGMENT => {
                // §8.7: the first target; the op is not given, any is taken
                self.segments.insert(object, links.iter().find(|l| l.1 == SECTION_LINK).map(|l| l.2));
            }
            SECTION_RECT | SECTION_ROUND => {
                self.sections.insert(object, section(class, data));
            }
            SECTION_PROFILE => {
                // §8.13: the profile object, the first target
                let profile = links.iter().find(|l| (l.0 == 0x11 || l.0 == 0x12) && l.1 == PROFILE_LINK).map(|l| l.2);
                self.sections.insert(object, Section::Profile(profile));
            }
            STAIR_SCHEME => {
                self.schemes.insert(object, stair_scheme(data));
            }
            RAILING_SEGMENT => {
                self.railing_segments.insert(object, links.iter().find(|l| (l.0 == 0x11 || l.0 == 0x12) && l.1 == SEGMENT_CACHE_LINK).map(|l| l.2));
            }
            SEGMENT_CACHE => {
                self.segment_caches.insert(object, segment_lines(data));
            }
            RAILING_CACHE => {
                self.railing_boxes.insert(object, railing_box(data));
            }
            MORPH_BODY => {
                // the cap of the other elements (the largest body in the samples: 290 kB); a larger
                // one counts as a morph without a body that reads
                let body = if data.len() <= MAX_ELEMENT_DATA { morph_body(data) } else { None };
                self.bodies.insert(object, body);
            }
            PROFILE => {
                if data.len() <= MAX_ELEMENT_DATA {
                    self.profiles.insert(object, profile_shape(data));
                }
            }
            LAYOUT => {
                // 20 bytes, the name, 2 bytes, the number; its master layout 50 bytes from the end
                let Some((name, after)) = text_at(data, 20) else { return };
                let number = text_at(data, after + 2).map_or(String::new(), |(number, _)| number);
                let master = data.len().checked_sub(50).and_then(|at| guid_at(data, at));
                self.layouts.push(Layout { guid: object, name, number, master });
            }
            LINE_2D | POLYLINE_2D | TEXT_2D | DRAWING => {
                // only what is drawn on a layout or a master: the same classes draw on the plans
                let Some(on) = links.iter().find(|l| l.0 == 0x12 && l.1 == ON_LAYOUT).map(|l| l.2) else { return };
                if data.len() > MAX_ELEMENT_DATA || self.layout_parts.len() >= MAX_LAYOUT_PARTS {
                    return;
                }
                let part = match class {
                    LINE_2D => layout_line(data),
                    POLYLINE_2D => find_polygon(data).filter(|p| p.contours.iter().flatten().flatten().all(on_paper)).map(|p| LayoutPart::Outline(p.contours)).or_else(|| layout_box(data)),
                    TEXT_2D => layout_text(data),
                    _ => find_polygon(data).filter(|p| p.contours.iter().flatten().flatten().all(on_paper)).map(|p| LayoutPart::Drawing { outline: p.contours, source: links.iter().find(|l| l.1 == DRAWING_SOURCE_LINK).map(|l| l.2) }),
                };
                if let Some(part) = part {
                    self.layout_parts.push((on, part));
                }
            }
            DRAWING_SOURCE => {
                if let Some(view) = links.iter().find(|l| l.1 == SOURCE_VIEW_LINK).map(|l| l.2) {
                    self.drawing_views.insert(object, view);
                }
            }
            VIEW => {
                if let Some((name, _)) = text_at(data, 31) {
                    self.view_names.insert(object, name);
                }
            }
            STORY => {
                // §6: 10 bytes, u32 n, n UTF-16LE units of name, i16 index
                let Some(n) = u32_at(data, 10).map(|n| n as usize) else { return };
                let Some(name) = n.checked_mul(2).and_then(|b| data.get(14..14 + b)) else { return };
                let Some(index) = u16_at(data, 14 + 2 * n).map(|i| i as i16) else { return };
                let units: Vec<u16> = name.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                self.stories.insert(object, (String::from_utf16_lossy(&units), index));
            }
            // §8.10: only what the objects need, not the records themselves
            OUTLINE => {
                if let Some(xy) = data.get(75..91).and_then(|xy| xy.try_into().ok()) {
                    self.outlines.insert(object, Outline { bounds: object_outline(data), xy });
                }
            }
            LIBRARY => {
                let parameters = links.iter().find(|l| l.1 == LIBRARY_PARAMETERS).map(|l| l.2);
                self.libraries.insert(object, (parameters, library_name(data)));
            }
            PARAMETERS => {
                self.heights.insert(object, object_height(data));
            }
            LINK => {
                // §8.3: the op of this association is not given; any is taken (also below)
                let targets = links.iter().filter(|l| l.1 == LINK_STORY).map(|l| l.2).collect();
                self.top_links.insert(object, targets);
            }
            _ => {
                // §6: the level is the last f64 of the data; the story is a 0x12 target
                let Some(z) = data.len().checked_sub(8).and_then(|at| f64_at(data, at)).filter(|z| z.is_finite()) else { return };
                self.levels.push((links.into_iter().filter(|l| l.0 == 0x12).map(|l| l.2).collect(), z));
            }
        }
    }
}


/// A layout: a sheet of the layout book.
struct Layout {
    guid: Guid,
    name: String,
    number: String,
    /// Its master layout, when it has one.
    master: Option<Guid>,
}

/// What is drawn on a layout or a master layout (metres on paper, from its lower left corner).
enum LayoutPart {
    Line([[f64; 2]; 2]),
    /// A polyline or polygon: its contours, each closed.
    Outline(Vec<Vec<[f64; 2]>>),
    Text(LayoutText),
    /// A placed drawing: its frame and the record that names the view it shows.
    Drawing { outline: Vec<Vec<[f64; 2]>>, source: Option<Guid> },
}

/// A text block: its lines with the size of each (the height of its capitals), the corner its box
/// hangs from (the upper left one; the upper right one of a block aligned right), and its box.
struct LayoutText {
    lines: Vec<(String, f64)>,
    at: [f64; 2],
    size: [f64; 2],
    /// 0 left, 1 centre, 2 right.
    align: u8,
}

/// A string: u32 n, then n UTF-16LE units; with the offset after it.
fn text_at(data: &[u8], at: usize) -> Option<(String, usize)> {
    let n = u32_at(data, at).map(|n| n as usize).filter(|&n| n <= 4096)?;
    let bytes = bytes_at(data, at + 4, n * 2)?;
    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    Some((String::from_utf16_lossy(&units), at + 4 + n * 2))
}

/// Whether a coordinate can lie on a sheet of paper (metres): the largest sheets are under 2 m.
fn on_paper(v: &f64) -> bool {
    (-0.5..3.0).contains(v)
}

/// A polyline whose points do not read: the box around it (four f64 at 139: left, bottom, right,
/// top), drawn as a rectangle. The frame of a master layout is such a polyline.
fn layout_box(data: &[u8]) -> Option<LayoutPart> {
    let v = [0, 8, 16, 24].map(|k| f64_at(data, 139 + k).filter(on_paper));
    let (x0, y0, x1, y1) = (v[0]?, v[1]?, v[2]?, v[3]?);
    (x0 < x1 && y0 < y1).then(|| LayoutPart::Outline(vec![vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]]))
}

/// A line on a layout: its two points are the four f64 that end 1 byte before the end of its data.
fn layout_line(data: &[u8]) -> Option<LayoutPart> {
    let at = data.len().checked_sub(33)?;
    let v = [0, 8, 16, 24].map(|k| f64_at(data, at + k).filter(on_paper));
    Some(LayoutPart::Line([[v[0]?, v[1]?], [v[2]?, v[3]?]]))
}

/// A text block on a layout: its content is one string at 116 (lines parted by a carriage
/// return); each line's letter size is the f64 after a u32 11 further on; 116 bytes from the end
/// stand 1.0, the corner (x, y) and the box (width, height), and 62 from the end how it is aligned.
/// The corner is in metres; the box and the letter sizes are too, unless the block does not wrap.
fn layout_text(data: &[u8]) -> Option<LayoutPart> {
    let (content, after) = text_at(data, 116)?;
    let tail = data.len().checked_sub(116).filter(|&t| t >= after)?;
    if f64_at(data, tail)? != 1.0 {
        return None;
    }
    // a block that does not wrap (byte 109 set) gives its box and letter sizes in millimetres
    let unit = if data.get(109) == Some(&1) { 0.001 } else { 1.0 };
    let at = [8, 16].map(|k| f64_at(data, tail + k).filter(on_paper));
    let size = [24, 32].map(|k| f64_at(data, tail + k).map(|v| v * unit).filter(on_paper));
    let (at, size) = ([at[0]?, at[1]?], [size[0]?, size[1]?]);
    let mut sizes = Vec::new();
    let mut i = after;
    while i + 12 <= tail {
        match (u32_at(data, i), f64_at(data, i + 4)) {
            (Some(11), Some(s)) if (0.0005..0.1).contains(&(s * unit)) => {
                sizes.push(s * unit);
                i += 12;
            }
            _ => i += 1,
        }
    }
    let fallback = sizes.first().copied().unwrap_or(0.0025);
    let lines = content.split('\r').enumerate().map(|(k, line)| (line.to_string(), sizes.get(k).copied().unwrap_or(fallback))).collect();
    let align = u32_at(data, data.len() - 62).filter(|&a| a <= 2).unwrap_or(0) as u8;
    Some(LayoutPart::Text(LayoutText { lines, at, size, align }))
}

/// A line of a title block with its fields filled in: the layout's number and name and the name
/// of the drawing on it; fields the reader does not know (project and office data) stay empty.
fn fill_fields(line: &str, layout: &Layout, drawing: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(open) = rest.find('<') {
        let Some(close) = rest[open..].find('>').map(|c| open + c) else { break };
        out.push_str(&rest[..open]);
        out.push_str(match &rest[open + 1..close] {
            "LAYOUTID" | "LAYOUTNUMBER" => &layout.number,
            "LAYOUTNAME" => &layout.name,
            "DRAWINGNAME" => drawing,
            _ => "",
        });
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out
}

/// The layouts as sheets: the lines and texts of each layout and of its master, and the drawings
/// placed on it as frames with the name of the view they show. The paper is what the master draws,
/// with on every side the margin its lower left corner leaves (the size itself was not found in
/// the file).
fn layout_sheets(records: &Records) -> Vec<crate::model::Sheet> {
    use crate::model::{DraftText, Sheet, SheetRect, SheetViewport};
    let mut layouts: Vec<&Layout> = records.layouts.iter().collect();
    layouts.sort_by(|a, b| (&a.number, &a.name).cmp(&(&b.number, &b.name)));
    layouts
        .into_iter()
        .map(|layout| {
            let master = layout.master;
            let on = |owner: &Guid| *owner == layout.guid || Some(*owner) == master;
            let parts = || records.layout_parts.iter().filter(|(owner, _)| on(owner)).map(|(owner, part)| (*owner == layout.guid, part));
            // the drawings first: a title block names the first one
            let mut viewports = Vec::new();
            for (_, part) in parts() {
                let LayoutPart::Drawing { outline, source } = part else { continue };
                let points = outline.iter().flatten();
                let Some(rect) = points.map(|p| SheetRect { min: [p[0] as f32, p[1] as f32], max: [p[0] as f32, p[1] as f32] }).reduce(SheetRect::union) else { continue };
                let view = source.and_then(|s| records.drawing_views.get(&s)).and_then(|v| records.view_names.get(v));
                viewports.push(SheetViewport {
                    title: view.cloned().unwrap_or_else(|| layout.name.clone()),
                    view: None,
                    rect,
                    estimated: false,
                    scale: String::new(),
                    detail: None,
                    label: None,
                    origin: rect.center(),
                    ratio: 0.01,
                    drawing: None,
                    anchor: None,
                    turn: [1.0, 0.0],
                    draft: None,
                    written: None,
                });
            }
            let drawing = viewports.first().map_or("", |v| v.title.as_str()).to_string();
            let (mut lines, mut texts) = (Vec::new(), Vec::new());
            let mut drawn: Option<SheetRect> = None;
            for (own, part) in parts() {
                let before = lines.len();
                match part {
                    LayoutPart::Line(l) => lines.push(l.map(|p| p.map(|v| v as f32))),
                    LayoutPart::Outline(contours) => {
                        for c in contours {
                            lines.extend((0..c.len()).map(|k| [c[k], c[(k + 1) % c.len()]].map(|p| p.map(|v| v as f32))));
                        }
                    }
                    LayoutPart::Text(t) => {
                        // the lines share the box by their letter sizes, from its top down
                        let total: f64 = t.lines.iter().map(|l| l.1).sum();
                        let mut top = t.at[1];
                        for (line, size) in &t.lines {
                            let height = if total > 0.0 { t.size[1] * size / total } else { *size };
                            top -= height;
                            let text = fill_fields(line, layout, &drawing);
                            if text.trim().is_empty() {
                                continue;
                            }
                            let left = t.at[0] - [0.0, t.size[0] / 2.0, t.size[0]][t.align as usize];
                            texts.push(DraftText {
                                element: 0,
                                at: [left as f32, top as f32],
                                width: t.size[0] as f32,
                                height: height as f32,
                                // a letter size is the height of the capitals
                                size: (size * 1.4).min(height) as f32,
                                align: t.align,
                                angle: 0.0,
                                text,
                                face: crate::typeface::Face::default(),
                            });
                        }
                    }
                    LayoutPart::Drawing { .. } => {}
                }
                if !own {
                    for l in &lines[before..] {
                        let r = SheetRect { min: [l[0][0].min(l[1][0]), l[0][1].min(l[1][1])], max: [l[0][0].max(l[1][0]), l[0][1].max(l[1][1])] };
                        drawn = Some(drawn.map_or(r, |d| d.union(r)));
                    }
                }
            }
            // the margin its lower left corner leaves, where the master draws from the paper's corner
            let paper = drawn.map(|d| {
                let margin = [0, 1].map(|k| if d.min[k] > 0.0 && d.min[k] < 0.05 { d.min[k] } else { 0.01 });
                SheetRect { min: [d.min[0] - margin[0], d.min[1] - margin[1]], max: [d.max[0] + margin[0], d.max[1] + margin[1]] }
            });
            let mut sheet = Sheet::new(layout.number.clone(), layout.name.clone(), paper, viewports);
            (sheet.lines, sheet.texts) = (lines, texts);
            sheet
        })
        .collect()
}

/// §4: the class, data and associations of an object record, when its lengths add up.
fn record_parts(p: &[u8]) -> Option<(Guid, &[u8], &[u8])> {
    let class = guid_at(p, 0)?;
    let data_len = u32_at(p, 35)? as usize;
    let assoc_len = u32_at(p, 39usize.checked_add(data_len)?)? as usize;
    if 43usize.checked_add(data_len)?.checked_add(assoc_len)? != p.len() {
        return None; // not an element record
    }
    Some((class, &p[39..39 + data_len], &p[43 + data_len..]))
}

/// A block's payload.
enum Payload {
    Data(Vec<u8>),
    /// Of a class the reader uses, but it could not be decoded.
    Damaged,
    /// Not needed, or no payload.
    Other,
}

/// §3: the payload of a block, when `want` accepts its class (the first 16 payload bytes).
fn payload(b: &[u8], want: impl Fn(&Guid) -> bool) -> Payload {
    let size_class = b[9];
    let wide = size_class >> 4 == 0xB || size_class & 0x0F >= 7;
    let (n, h) = if wide { (u32_at(b, 27).map(|n| n as usize), 31) } else { (u16_at(b, 27).map(usize::from), 29) };
    let Some(n) = n else { return Payload::Other };
    let wanted = |p: &[u8]| guid_at(p, 0).is_some_and(|class| want(&class));
    match b[10] {
        0x84 => match bytes_at(b, h + 4, n) {
            Some(p) if wanted(p) => Payload::Data(p.to_vec()),
            _ => Payload::Other,
        },
        encoding @ (0x85 | 0x86) => {
            let (Some(size), Some(input)) = (u32_at(b, h).map(|n| n as usize), bytes_at(b, h + 8, n)) else { return Payload::Other };
            if size > MAX_PAYLOAD {
                return Payload::Other;
            }
            let out = if encoding == 0x85 { unmask(input, size) } else { unpack(input, size, wanted) };
            match out {
                Some(p) if wanted(&p) => Payload::Data(p),
                Some(_) => Payload::Other,
                // a payload that does not decode may not be of a wanted class: count it only when
                // its start shows it is
                None if encoding == 0x85 => unmask_start(input).filter(|s| wanted(s)).map_or(Payload::Other, |_| Payload::Damaged),
                None => Payload::Other,
            }
        }
        _ => Payload::Other,
    }
}

/// §3.1: the zero-byte mask. Each mask byte says for 8 output bytes (bit 0 first) whether the next
/// input byte is copied (bit set) or a zero is written. Exactly the whole input is used.
fn unmask(input: &[u8], size: usize) -> Option<Vec<u8>> {
    // each input byte gives at most 8 output bytes
    if size > input.len().saturating_mul(8) {
        return None;
    }
    let mut out = Vec::with_capacity(size);
    let mut at = 0;
    while out.len() < size {
        let mask = *input.get(at)?;
        at += 1;
        for bit in 0..8 {
            if out.len() == size {
                break;
            }
            if mask >> bit & 1 == 1 {
                out.push(*input.get(at)?);
                at += 1;
            } else {
                out.push(0);
            }
        }
    }
    (at == input.len()).then_some(out)
}

/// The first 16 bytes of a zero-masked payload (to tell its class when the rest is damaged).
fn unmask_start(input: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(16);
    let mut at = 0;
    while out.len() < 16 {
        let mask = *input.get(at)?;
        at += 1;
        for bit in 0..8 {
            if mask >> bit & 1 == 1 {
                out.push(*input.get(at)?);
                at += 1;
            } else {
                out.push(0);
            }
        }
    }
    Some(out)
}

/// §3, §2.2: QuickLZ chunks back to back, `size` bytes together. After the first chunk the class
/// is known: other classes are not decompressed further.
fn unpack(input: &[u8], size: usize, wanted: impl Fn(&[u8]) -> bool) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(size.min(1 << 20));
    let mut at = 0;
    while out.len() < size {
        // the QuickLZ header: flags, then the compressed length (incl. header) in 1 or 4 bytes
        let flags = *input.get(at)?;
        let len = if flags & 2 == 2 { u32_at(input, at + 1)? as usize } else { usize::from(*input.get(at + 1)?) };
        let chunk = input.get(at..at.checked_add(len)?)?;
        let max = u32::try_from(size - out.len()).ok()?;
        out.extend(quicklz::decompress(&mut Cursor::new(chunk), max).ok()?);
        if at == 0 && out.len() >= 16 && !wanted(&out) {
            return Some(out);
        }
        at += len.max(1);
    }
    (out.len() == size).then_some(out)
}

/// §4.1: the associations of a record, as (op, association, target); an item with k targets
/// gives k of them.
fn links(a: &[u8]) -> Vec<(u8, Guid, Guid)> {
    let mut out = Vec::new();
    let mut at = 2;
    while let Some(&op) = a.get(at) {
        at += 1;
        match op {
            0x11 | 0x12 => {
                let (Some(assoc), Some(target)) = (guid_at(a, at), guid_at(a, at + 16)) else { break };
                out.push((op, assoc, target));
                at += 32;
            }
            0x01 | 0x02 => {
                let (Some(assoc), Some(k)) = (guid_at(a, at), u32_at(a, at + 16).map(|k| k as usize)) else { break };
                let Some(next) = k.checked_mul(16).and_then(|n| n.checked_add(at + 20)).filter(|&n| n <= a.len()) else { break };
                out.extend((at + 20..next).step_by(16).filter_map(|t| guid_at(a, t)).map(|target| (op, assoc, target)));
                at = next;
            }
            // 0x00 ends the list; an unknown op ends what can be read
            _ => break,
        }
    }
    out
}

/// §7: the polygon of an element: contour 0 the outer edge, the rest holes (closing point
/// dropped), and the absolute bottom and top it caches, if any.
struct Polygon {
    /// Where it starts and ends in the data (§8.8, §8.11 read fields before and after it).
    at: usize,
    end: usize,
    contours: Vec<Vec<[f64; 2]>>,
    /// The number of each point in the polygon (1-based, as in the file), per contour.
    indices: Vec<Vec<usize>>,
    coords: usize,
    z: Option<(f64, f64)>,
}

/// §7, §8.16.1 (v0.9): the first polygon in the data: a bounding box, the counts, the points (point
/// 0 unused), then, with arcs, the arcs (an empty item first), and the contour ends. Without arcs
/// valid when the points span exactly the box; with arcs, when they lie in it (an arc may bulge
/// past the points). Arcs are drawn as segments (counter-clockwise for a positive angle).
fn find_polygon(data: &[u8]) -> Option<Polygon> {
    if data.len() > MAX_ELEMENT_DATA {
        return None;
    }
    // values read for candidates past the cheap checks: a bound on the work for any data
    let mut budget = 1usize << 24;
    for i in 0..data.len().saturating_sub(43) {
        if let Some(polygon) = polygon_at(data, i, &mut budget) {
            return Some(polygon);
        }
        if budget == 0 {
            return None;
        }
    }
    None
}

/// §7, §8.16.1: the polygon at offset i, when there is a valid one (`budget`: values left to read
/// for the expensive checks; 0 when spent).
fn polygon_at(data: &[u8], i: usize, budget: &mut usize) -> Option<Polygon> {
    let n = u32_at(data, i + 32).map(|n| n as usize).filter(|n| (2..=20_000).contains(n))?;
    let arcs = u32_at(data, i + 36).map(|n| n as usize).filter(|&a| a <= n)?;
    let subs = u32_at(data, i + 40).map(|n| n as usize).filter(|&s| (1..=n).contains(&s))?;
    let bbox = [0, 8, 16, 24].map(|k| f64_at(data, i + k).unwrap_or(f64::NAN));
    if !bbox.iter().all(|v| v.is_finite()) || bbox[0] > bbox[2] || bbox[1] > bbox[3] {
        return None;
    }
    let points_at = i + 44;
    let arcs_at = points_at + (n + 1) * 16;
    let ends_at = if arcs > 0 { arcs_at + 16 * (arcs + 1) } else { arcs_at };
    let end = ends_at + (subs + 1) * 4;
    if end > data.len() {
        return None;
    }
    // the first and last contour end before the rest: most offsets fail here
    let end_at = |k: usize| u32_at(data, ends_at + 4 * k).unwrap() as usize;
    if end_at(0) != 0 || end_at(subs) != n {
        return None;
    }
    let Some(left) = budget.checked_sub(subs + 2 * n) else {
        *budget = 0;
        return None;
    };
    *budget = left;
    let ends: Vec<usize> = (0..=subs).map(end_at).collect();
    if ends.windows(2).any(|w| w[0] > w[1]) || (arcs > 0 && data[arcs_at..arcs_at + 16].iter().any(|&b| b != 0)) {
        return None;
    }
    let points: Vec<[f64; 2]> = (0..=n).map(|k| [0, 8].map(|c| f64_at(data, points_at + 16 * k + c).unwrap())).collect();
    let (mut min, mut max) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for p in &points[1..] {
        for c in 0..2 {
            min[c] = min[c].min(p[c]);
            max[c] = max[c].max(p[c]);
        }
    }
    let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
    let fits = if arcs == 0 {
        close(min[0], bbox[0]) && close(min[1], bbox[1]) && close(max[0], bbox[2]) && close(max[1], bbox[3])
    } else {
        min[0] >= bbox[0] - 1e-9 && min[1] >= bbox[1] - 1e-9 && max[0] <= bbox[2] + 1e-9 && max[1] <= bbox[3] + 1e-9
    };
    if !fits {
        return None;
    }
    // §8.16.1: the arcs, from point `begin` to point `end`
    let mut bows: HashMap<(usize, usize), f64> = HashMap::new();
    let mut arcs_fit = true;
    for k in 1..=arcs {
        let at = arcs_at + 16 * k;
        let (from, to, angle) = (u32_at(data, at).unwrap() as usize, u32_at(data, at + 4).unwrap() as usize, f64_at(data, at + 8).unwrap());
        let fits = |p: usize| (1..=n).contains(&p);
        arcs_fit &= fits(from) && fits(to) && angle.is_finite() && angle != 0.0 && angle.abs() < std::f64::consts::TAU;
        bows.insert((from, to), angle);
    }
    if !arcs_fit {
        return None;
    }
    // contour k: points ends[k]+1 ..= ends[k+1]; the last repeats the first. The points an arc
    // adds carry the number of the point it starts at.
    let mut indices = Vec::with_capacity(subs);
    let contours = (0..subs)
        .map(|k| {
            let mut numbers: Vec<usize> = (ends[k] + 1..=ends[k + 1]).collect();
            if numbers.len() > 1 && points[numbers[0]] != points[*numbers.last().unwrap()] {
                numbers.push(numbers[0]);
            }
            let (mut c, mut at) = (Vec::with_capacity(numbers.len()), Vec::with_capacity(numbers.len()));
            for w in numbers.windows(2) {
                c.push(points[w[0]]);
                at.push(w[0]);
                let angle = bows.get(&(w[0], w[1])).copied().or_else(|| bows.get(&(w[1], w[0])).map(|a| -a));
                for p in angle.map(|a| arc_points(points[w[0]], points[w[1]], a)).unwrap_or_default() {
                    c.push(p);
                    at.push(w[0]);
                }
            }
            if numbers.len() == 1 {
                c.push(points[numbers[0]]);
                at.push(numbers[0]);
            }
            indices.push(at);
            c
        })
        .collect();
    Some(Polygon { at: i, end, contours, indices, coords: n, z: z_cache(data, i, end) })
}

/// §7: the z cache: after the polygon, the last place the 32 bounding-box bytes occur again is
/// followed by the absolute bottom and top (none when both are 0).
fn z_cache(data: &[u8], polygon: usize, end: usize) -> Option<(f64, f64)> {
    let key = &data[polygon..polygon + 32];
    let at = (end..=data.len().checked_sub(32)?).rev().find(|&j| &data[j..j + 32] == key)?;
    let (bottom, top) = (f64_at(data, at + 32)?, f64_at(data, at + 40)?);
    (bottom.is_finite() && top.is_finite() && (bottom != 0.0 || top != 0.0)).then_some((bottom, top))
}

/// §8.2: the fields of a wall, after its ID string.
struct WallFields {
    id: String,
    /// The reference line: x1, y1, x2, y2.
    line: [f64; 4],
    /// s[0]: which side of the reference line the wall lies on.
    side: i32,
    /// Thickness at the start and at the end.
    thickness: [f64; 2],
    /// Offset of the reference line.
    offset: f64,
    /// A: top offset to the linked story.
    top_offset: f64,
    /// B: height (when the top is not linked).
    height: f64,
    /// C: bottom offset to the own story.
    bottom_offset: f64,
}

/// §8.2: the candidates for the ID string of an element, low to high: an i ≥ 40 with `01 00`
/// before it, a u32 length n ≤ 64 at i and n printable ASCII characters in UTF-16LE after it.
/// Gives the ID and its end q; the fields after q decide which candidate counts.
fn id_strings(data: &[u8]) -> impl Iterator<Item = (String, usize)> + '_ {
    (40..data.len()).filter_map(move |i| {
        if data[i - 2..i] != [1, 0] {
            return None;
        }
        let n = u32_at(data, i).map(|n| n as usize).filter(|&n| n <= 64)?;
        let text = bytes_at(data, i + 4, 2 * n)?;
        if !text.chunks_exact(2).all(|c| (0x20..=0x7E).contains(&c[0]) && c[1] == 0) {
            return None;
        }
        Some((text.chunks_exact(2).map(|c| c[0] as char).collect(), i + 4 + 2 * n))
    })
}

/// §8.2: the first ID string followed by six small non-zero i32 at q+32.
fn wall_fields(data: &[u8]) -> Option<WallFields> {
    id_strings(data).find_map(|(id, q)| {
        // every field must be in the data
        if q + 169 > data.len() {
            return None;
        }
        let s: Vec<i32> = (0..6).map(|k| i32_at(data, q + 32 + 4 * k).unwrap()).collect();
        if !s.iter().all(|&v| v != 0 && (-8..=8).contains(&v)) {
            return None;
        }
        let f = |at: usize| f64_at(data, q + at).unwrap();
        Some(WallFields {
            id,
            line: [f(0), f(8), f(16), f(24)],
            side: s[0],
            thickness: [f(77), f(85)],
            offset: f(93),
            top_offset: f(145),
            height: f(153),
            bottom_offset: f(161),
        })
    })
}

/// Comparisons allowed for the searches of §8.5 and §8.6 in one record: a bound on the work
/// for any data.
const SEARCH_BUDGET: usize = 1 << 24;

/// The first offset o in `range` where `a` and, 24 bytes on, `b` are found: the axis of a column
/// or beam before its ID string. `None` when the budget runs out.
fn find_axis(data: &[u8], range: std::ops::RangeInclusive<usize>, a: &[u8], b: &[u8], budget: &mut usize) -> Option<Option<usize>> {
    for o in range {
        *budget = budget.checked_sub(1)?;
        if &data[o..o + 16] == a && &data[o + 24..o + 40] == b {
            return Some(Some(o));
        }
    }
    Some(None)
}

/// §8.5: the fields of a column.
#[derive(Debug, Clone)]
struct ColumnFields {
    id: String,
    /// A: top offset to the linked story; H: height (top not linked); C: bottom offset.
    top_offset: f64,
    height: f64,
    bottom_offset: f64,
    /// The anchor point and the middle of the cross-section.
    anchor: [f64; 2],
    center: [f64; 2],
    /// Rotation of the cross-section (rad, counter-clockwise).
    rotation: f64,
}

/// §8.5: the first ID string followed by u32 k ≤ 16 and six finite f64 (A, H, C, -, x, y), with
/// the axis (x, y, z1, x, y, z2) earlier in the data; the middle of the section follows the axis.
fn column_fields(data: &[u8]) -> Option<ColumnFields> {
    let mut budget = SEARCH_BUDGET;
    id_strings(data).find_map(|(id, q)| {
        if u32_at(data, q)? > 16 || q + 95 > data.len() {
            return None;
        }
        let f: Vec<f64> = (0..6).map(|k| f64_at(data, q + 4 + 8 * k).unwrap()).collect();
        let xy = &data[q + 36..q + 52];
        if !f.iter().all(|v| v.is_finite()) || xy.iter().all(|&b| b == 0) {
            return None;
        }
        let o = find_axis(data, 0..=q.checked_sub(96)?, xy, xy, &mut budget)??;
        let (cx, cy, rotation) = (f64_at(data, o + 48)?, f64_at(data, o + 56)?, f64_at(data, q + 87)?);
        // the slope at q+79 is π/2 (vertical) in the samples; a column is drawn vertical
        Some(ColumnFields {
            id,
            top_offset: f[0],
            height: f[1],
            bottom_offset: f[2],
            anchor: [f[4], f[5]],
            center: [cx, cy],
            rotation: if rotation.is_finite() { rotation } else { 0.0 },
        })
    })
    .filter(|f| f.center.iter().all(|v| v.is_finite()))
}

/// §8.6: the fields of a beam.
#[derive(Debug, Clone)]
struct BeamFields {
    id: String,
    /// s: sideways offset of the axis.
    offset: f64,
    /// The axis: x1, y1, x2, y2.
    line: [f64; 4],
    /// The top of the beam at the start and the end, to its own story (both zr when the axis
    /// with its heights is not found: horizontal). For a profile beam, the axis (§8.13).
    top: (f64, f64),
    /// The end of its ID string (§8.13 reads the mirror flag after it).
    q: usize,
}

/// §8.6 (v0.4): an ID string followed by u8 k ≤ 16 and six f64 (s, zr, x1, y1, x2, y2) that are
/// 0 or between 1e-9 and 1e5; the heights z1 and z2 are with the axis earlier in the data, or zr
/// for both. The first candidate whose axis is found counts; when none has one, the first that
/// meets the conditions.
fn beam_fields(data: &[u8]) -> Option<BeamFields> {
    let plausible = |v: &f64| *v == 0.0 || (1e-9..1e5).contains(&v.abs());
    let mut budget = SEARCH_BUDGET;
    let mut first = None;
    for (id, q) in id_strings(data) {
        if data.get(q).is_none_or(|&k| k > 16) || q + 49 > data.len() {
            continue;
        }
        let f: Vec<f64> = (0..6).map(|k| f64_at(data, q + 1 + 8 * k).unwrap()).collect();
        let (start, end) = (&data[q + 17..q + 33], &data[q + 33..q + 49]);
        if !f.iter().all(plausible) || start.iter().all(|&b| b == 0) {
            continue;
        }
        let axis = match q.checked_sub(48) {
            Some(last) => find_axis(data, 0..=last, start, end, &mut budget)?,
            None => None,
        };
        let beam = |top: (f64, f64)| BeamFields { id: id.clone(), offset: f[0], line: [f[2], f[3], f[4], f[5]], top, q };
        match axis {
            Some(o) => {
                let top = (f64_at(data, o + 16)?, f64_at(data, o + 40)?);
                if top.0.is_finite() && top.1.is_finite() {
                    return Some(beam(top));
                }
            }
            None if first.is_none() => first = Some(beam((f[1], f[1]))),
            None => {}
        }
    }
    first
}

/// §8.8: the fields of a roof plane, before its polygon.
#[derive(Debug, Clone)]
struct RoofFields {
    id: String,
    /// hp: height of the pivot line above the own story (the underside of the roof).
    pivot_height: f64,
    /// α: pitch in degrees (0: flat).
    pitch: f64,
    /// The pivot line: x1, y1, x2, y2.
    line: [f64; 4],
    /// t: thickness, square to the roof plane.
    thickness: f64,
}

/// §8.8: hp at p−62, α at p−53, the pivot line at p−45, and the thickness after the last ID
/// string candidate that ends 250 bytes or more before the polygon at p.
fn roof_fields(data: &[u8], p: usize) -> Option<RoofFields> {
    let f = |back: usize| f64_at(data, p.checked_sub(back)?);
    let (pivot_height, pitch) = (f(62)?, f(53)?);
    let line = [f(45)?, f(37)?, f(29)?, f(21)?];
    let (id, q) = id_strings(data).take_while(|&(_, q)| q + 250 <= p).last()?;
    let thickness = f64_at(data, q + 1)?;
    let finite = [pivot_height, pitch, thickness].iter().chain(&line).all(|v| v.is_finite());
    // §8.16.2 (v0.9): thickness 0 reads (a helper body), and is skipped by the caller
    (finite && (0.0..5.0).contains(&thickness) && (0.0..90.0).contains(&pitch)).then_some(RoofFields { id, pivot_height, pitch, line, thickness })
}

/// §8.8: the underside and the top of a roof plane: the underside through the pivot line at
/// e + hp, rising by tan α towards the side of the line with the farthest point of the outer
/// edge; the top t / cos α above it.
struct RoofPlanes {
    /// A point of the pivot line, its left normal (0 for a flat roof), and the rise per metre
    /// along the normal (signed towards the side the roof rises to).
    origin: [f64; 2],
    normal: [f64; 2],
    rise: f64,
    base: f64,
    up: f64,
}

impl RoofPlanes {
    /// `None` for a sloped roof without a pivot line.
    fn new(f: &RoofFields, e: f64, outer: &[[f64; 2]]) -> Option<Self> {
        let [x1, y1, x2, y2] = f.line;
        let length = (x2 - x1).hypot(y2 - y1);
        let flat = f.pitch == 0.0;
        if !flat && (length.is_nan() || length < 1e-9) {
            return None;
        }
        let normal = if flat { [0.0, 0.0] } else { [-(y2 - y1) / length, (x2 - x1) / length] };
        let mut planes = Self { origin: [x1, y1], normal, rise: f.pitch.to_radians().tan(), base: e + f.pivot_height, up: f.thickness / f.pitch.to_radians().cos() };
        let (low, high) = outer.iter().map(|&p| planes.distance(p)).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
        if high < -low {
            planes.rise = -planes.rise;
        }
        Some(planes)
    }

    /// d(P): the distance from the pivot line, positive to its left.
    fn distance(&self, p: [f64; 2]) -> f64 {
        (p[0] - self.origin[0]) * self.normal[0] + (p[1] - self.origin[1]) * self.normal[1]
    }

    fn bottom(&self, p: [f64; 2]) -> f64 {
        self.base + self.distance(p) * self.rise
    }

    fn top(&self, p: [f64; 2]) -> f64 {
        self.bottom(p) + self.up
    }
}

/// §8.5: the footprint of a column: its section around the middle, turned by the rotation; a
/// round one as an ellipse of 32 points.
fn column_footprint(f: &ColumnFields, section: Section) -> Option<Vec<[f64; 2]>> {
    let (s, c) = f.rotation.sin_cos();
    let (u, v) = ([c, s], [-s, c]);
    let at = |a: f64, b: f64| [f.center[0] + u[0] * a + v[0] * b, f.center[1] + u[1] * a + v[1] * b];
    match section {
        Section::Rect(b, d) => Some(vec![at(-b / 2.0, -d / 2.0), at(b / 2.0, -d / 2.0), at(b / 2.0, d / 2.0), at(-b / 2.0, d / 2.0)]),
        Section::Round(b, d) => Some(
            (0..32)
                .map(|k| {
                    let t = k as f64 * std::f64::consts::TAU / 32.0;
                    at(b / 2.0 * t.cos(), d / 2.0 * t.sin())
                })
                .collect(),
        ),
        _ => None,
    }
}

/// §8.6: the two end faces of a beam (corners at the start, then at the end, in the same order),
/// from its top `e` + z1 / `e` + z2 down by its height, s − b/2 to s + b/2 beside its axis; a
/// round one as a cylinder of 32 points with its middle b/2 below the top.
fn beam_ends(f: &BeamFields, e: f64, section: Section) -> Option<[Vec<[f64; 3]>; 2]> {
    let [x1, y1, x2, y2] = f.line;
    let length = (x2 - x1).hypot(y2 - y1);
    if length.is_nan() || length < 1e-9 {
        return None;
    }
    let n = [-(y2 - y1) / length, (x2 - x1) / length];
    let end = |x: f64, y: f64, top: f64| -> Option<Vec<[f64; 3]>> {
        let at = |side: f64, z: f64| [x + n[0] * (f.offset + side), y + n[1] * (f.offset + side), z];
        match section {
            Section::Rect(b, h) => Some(vec![at(-b / 2.0, top), at(b / 2.0, top), at(b / 2.0, top - h), at(-b / 2.0, top - h)]),
            Section::Round(b, _) => Some(
                (0..32)
                    .map(|k| {
                        let t = k as f64 * std::f64::consts::TAU / 32.0;
                        at(b / 2.0 * t.cos(), top - b / 2.0 + b / 2.0 * t.sin())
                    })
                    .collect(),
            ),
            _ => None,
        }
    };
    Some([end(x1, y1, e + f.top.0)?, end(x2, y2, e + f.top.1)?])
}

/// Adds a convex body between two faces with corresponding corners: the two faces and a side
/// per pair of corners, flat shaded and facing out. False when it has no volume.
fn push_hull(layer: &mut Layer, ends: &[Vec<[f64; 3]>; 2]) -> bool {
    let all: Vec<[f64; 3]> = ends.iter().flatten().copied().collect();
    if all.iter().flatten().any(|v| !v.is_finite()) || ends[0].len() != ends[1].len() || ends[0].len() < 3 {
        return false;
    }
    let mean = |pts: &[[f64; 3]]| [0, 1, 2].map(|k| pts.iter().map(|p| p[k]).sum::<f64>() / pts.len() as f64);
    let middle = mean(&all);
    let first = layer.indices.len();
    let face = |layer: &mut Layer, mut pts: Vec<[f64; 3]>| {
        // Newell's normal, turned to point away from the middle of the body
        let mut n = [0.0f64; 3];
        for i in 0..pts.len() {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            n[0] += (a[1] - b[1]) * (a[2] + b[2]);
            n[1] += (a[2] - b[2]) * (a[0] + b[0]);
            n[2] += (a[0] - b[0]) * (a[1] + b[1]);
        }
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if length.is_nan() || length <= 1e-12 {
            return;
        }
        let c = mean(&pts);
        if (0..3).map(|k| n[k] * (c[k] - middle[k])).sum::<f64>() < 0.0 {
            pts.reverse();
            n = n.map(|v| -v);
        }
        let base = (layer.positions.len() / 3) as u32;
        for p in &pts {
            layer.positions.extend(p.map(|v| v as f32));
            layer.normals.extend(n.map(|v| (v / length) as f32));
        }
        for i in 1..pts.len() as u32 - 1 {
            layer.indices.extend([base, base + i, base + i + 1]);
        }
    };
    face(layer, ends[0].clone());
    face(layer, ends[1].clone());
    let k = ends[0].len();
    for i in 0..k {
        let j = (i + 1) % k;
        face(layer, vec![ends[0][i], ends[0][j], ends[1][j], ends[1][i]]);
    }
    layer.indices.len() > first
}

/// An element ready to draw: footprint contours (outer first) extruded from `z.0` to `z.1`.
struct Prism {
    contours: Vec<Vec<[f64; 2]>>,
    z: (f64, f64),
}

/// §8.3 (v0.2): wall bottom and top when the polygon caches none. `e` is the level of the wall's
/// own story; `linked` the level of the story its top is linked to, if any. The height B is out of
/// date for a linked top, so it is used only when the top is not linked.
fn wall_heights(f: &WallFields, e: f64, linked: Option<f64>) -> (f64, f64) {
    let bottom = e + f.bottom_offset;
    let top = match linked {
        Some(level) => level + f.top_offset,
        None => bottom + f.height,
    };
    (bottom, top)
}

/// §8.4: the footprint of a wall without polygon (Archicad 25): a quadrilateral along the
/// reference line, on the side s[0] gives, moved by the reference line offset.
fn wall_rectangle(f: &WallFields) -> Option<Vec<[f64; 2]>> {
    let [x1, y1, x2, y2] = f.line;
    let length = (x2 - x1).hypot(y2 - y1);
    if length.is_nan() || length < 1e-9 {
        return None;
    }
    let normal = [-(y2 - y1) / length, (x2 - x1) / length];
    let range = |t: f64| {
        let (a, b) = match f.side {
            1 => (0.0, t),
            3 => (-t, 0.0),
            _ => (-t / 2.0, t / 2.0),
        };
        (a + f.offset, b + f.offset)
    };
    let ((a1, b1), (a2, b2)) = (range(f.thickness[0]), range(f.thickness[1]));
    let at = |x: f64, y: f64, d: f64| [x + normal[0] * d, y + normal[1] * d];
    Some(vec![at(x1, y1, a1), at(x2, y2, a2), at(x2, y2, b2), at(x1, y1, b1)])
}

/// What the reader found, for the project information and the tests.
#[derive(Debug, Default)]
struct Stats {
    walls: usize,
    walls_skipped: usize,
    /// Walls with a polygon footprint (§8.4-1), with stored heights (§8.3-1).
    walls_polygon: usize,
    walls_stored_heights: usize,
    /// Walls whose top is linked to a story (§8.3), and those whose top is not.
    walls_linked: usize,
    walls_unlinked: usize,
    /// Walls skipped as their top is not above their bottom (§8.3), part of `walls_skipped`.
    walls_no_height: usize,
    slabs: usize,
    slabs_skipped: usize,
    /// The stories shown: name as in the file and level, low to high.
    stories: Vec<(String, f64)>,
    /// All story levels (§6), with the one above the top story.
    levels: Vec<f64>,
    columns: Vec<ColumnOut>,
    columns_skipped: usize,
    beams: Vec<BeamOut>,
    beams_skipped: usize,
    /// Columns and beams skipped for a profile section, or a section that is missing or does not
    /// read (§8.7); part of the skipped ones.
    columns_profile: usize,
    columns_no_section: usize,
    /// Columns and beams drawn from their profile (§8.13), those drawn as its box, and beams
    /// whose mirror flag is neither 0 nor 1 (taken as 0).
    profiles: Vec<ProfileOut>,
    profile_blocks: usize,
    profile_odd_flags: usize,
    beams_profile: usize,
    beams_no_section: usize,
    roofs: Vec<RoofOut>,
    /// Roof planes skipped (§8.8): all of them, those without a polygon (or one too near the
    /// start of the data), and those whose fields do not read.
    roofs_skipped: usize,
    roofs_no_polygon: usize,
    roofs_unreadable: usize,
    roofs_zero_thickness: usize,
    /// The slabs drawn, for the tests.
    slab_list: Vec<SlabOut>,
    openings: Vec<OpeningOut>,
    /// Windows and doors skipped (§8.9): all of them, those whose record is missing, those whose
    /// fields do not read, and those of a wall that is not drawn.
    openings_skipped: usize,
    openings_missing: usize,
    openings_unreadable: usize,
    openings_wall_skipped: usize,
    /// Openings past the first [`MAX_OPENINGS`] of one wall.
    openings_too_many: usize,
    objects: Vec<ObjectOut>,
    /// Library objects skipped (§8.10): all of them, those without a single place for their
    /// fields, the 2D ones (ZZYZX 0), and those whose fields do not read.
    objects_skipped: usize,
    objects_ambiguous: usize,
    objects_flat: usize,
    objects_unreadable: usize,
    meshes: Vec<MeshOut>,
    /// Meshes skipped (§8.11): all, without a polygon, with fields or a layout that do not read;
    /// drawn from their outline (no cache), drawn as a top only; cache triangles left out.
    meshes_skipped: usize,
    meshes_no_polygon: usize,
    meshes_unreadable: usize,
    meshes_without_cache: usize,
    meshes_top_only: usize,
    mesh_bad_triangles: usize,
    zones: Vec<ZoneOut>,
    /// Zones skipped (§8.12): all, without a polygon, without a number, with fields that do not
    /// read, without a height.
    zones_skipped: usize,
    zones_no_polygon: usize,
    zones_no_number: usize,
    zones_unreadable: usize,
    zones_no_height: usize,
    morphs: Vec<MorphOut>,
    /// Morphs skipped (§8.16.3): all, without a body that reads, without a placement; and the
    /// open ones drawn.
    morphs_skipped: usize,
    morphs_no_body: usize,
    morphs_no_matrix: usize,
    morphs_open: usize,
    stairs: Vec<StairOut>,
    /// Stairs skipped (§8.14): all, whose fields, scheme or riser and tread sequence do not
    /// read; and the treads on the floor (the first one of a stair that starts with a tread).
    stairs_skipped: usize,
    stairs_fields: usize,
    stairs_scheme: usize,
    stairs_sequence: usize,
    stairs_floor_treads: usize,
    railings: Vec<RailingOut>,
    /// Railings skipped (§8.15): all (without a segment that reads, nor a bounding box); those
    /// drawn as their bounding box; segments that do not read.
    railings_skipped: usize,
    railings_boxed: usize,
    railing_bad_segments: usize,
}

/// A column as drawn, for the tests.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
struct ColumnOut {
    name: String,
    center: [f64; 2],
    anchor: [f64; 2],
    section: Section,
    z: (f64, f64),
    linked: bool,
}

/// A column or beam drawn from its profile (§8.13), for the tests.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
struct ProfileOut {
    name: String,
    beam: bool,
    /// The mirror flag as in the file (beams).
    flag: Option<u8>,
    fills: usize,
    points: usize,
    bbox: [f64; 4],
    /// The area of the fills (outer edges minus holes).
    area: f64,
    block: bool,
    x: (f64, f64),
    y: (f64, f64),
    z: (f64, f64),
}

/// The ranges of a set of points.
fn ranges(points: impl Iterator<Item = [f64; 3]>) -> [(f64, f64); 3] {
    points.fold([(f64::INFINITY, f64::NEG_INFINITY); 3], |r, p| [0, 1, 2].map(|k| (r[k].0.min(p[k]), r[k].1.max(p[k]))))
}

/// The area of the fills of a profile: outer edges minus holes.
fn fills_area(fills: &[Fill]) -> f64 {
    fills.iter().flat_map(|f| f.iter().enumerate().map(|(k, c)| if k == 0 { area(c).abs() } else { -area(c).abs() })).sum()
}

/// A beam as drawn, for the tests.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
struct BeamOut {
    name: String,
    start: [f64; 2],
    end: [f64; 2],
    section: Section,
    /// The top at the start and the end.
    top: (f64, f64),
}

/// A mesh as drawn, for the tests.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
struct MeshOut {
    name: String,
    /// From the triangulation cache (else from the outline).
    cached: bool,
    /// Vertices used, triangles of the top and outer edges.
    vertices: usize,
    triangles: usize,
    outer_edges: usize,
    reference: f64,
    thickness: f64,
    x: (f64, f64),
    y: (f64, f64),
    /// The lowest and highest point of the top, and the bottom.
    top: (f64, f64),
    bottom: f64,
}

/// A stair as drawn, for the tests.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
struct StairOut {
    name: String,
    risers: usize,
    riser: f64,
    going: f64,
    /// Bottom and top of the stair; whether it starts with a riser; the treads (L2).
    z: (f64, f64),
    starts_with_riser: bool,
    treads: usize,
    /// The top of the first and the last tread.
    tread_z: (f64, f64),
    mirrored: bool,
    x: (f64, f64),
    y: (f64, f64),
}

/// A railing as drawn, for the tests: its segments with a cache, the length of its lower lines in
/// plan, the ranges of its lower and upper lines, and h.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
struct RailingOut {
    guid: String,
    name: String,
    segments: usize,
    length: f64,
    height: f64,
    boxed: bool,
    x: (f64, f64),
    y: (f64, f64),
    z: (f64, f64),
}

/// A morph as drawn, for the tests.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
struct MorphOut {
    name: String,
    faces: usize,
    open: bool,
    x: (f64, f64),
    y: (f64, f64),
    z: (f64, f64),
}

/// A zone as drawn, for the tests.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
struct ZoneOut {
    number: String,
    name: String,
    linked: bool,
    holes: usize,
    z: (f64, f64),
    x: (f64, f64),
    y: (f64, f64),
    area: f64,
}

/// A library object as drawn, for the tests: A, B, H, whether H is known, and its ranges.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
struct ObjectOut {
    name: String,
    size: [f64; 3],
    height_known: bool,
    shape: ObjectShape,
    x: (f64, f64),
    y: (f64, f64),
    z: (f64, f64),
}

/// A window or door as cut from its wall, for the tests.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
struct OpeningOut {
    name: String,
    door: bool,
    wall: String,
    /// The length of the wall's reference line, and the middle of the opening along it.
    wall_length: f64,
    middle: f64,
    width: f64,
    height: f64,
    sill: f64,
}

/// §8.9: the fields of a window or door.
#[derive(Debug, Clone)]
struct OpeningFields {
    id: String,
    /// b and h: width and height of the opening.
    width: f64,
    height: f64,
    /// m: the middle of the opening along the wall's reference line, from its start.
    middle: f64,
    /// s: the sill, to the level of the wall's story.
    sill: f64,
}

/// §8.9: the last ID string candidate with a name; b and h 84 and 76 bytes before its length
/// field i, m and s 24 and 64 bytes after its end q.
fn opening_fields(data: &[u8]) -> Option<OpeningFields> {
    let (id, q) = id_strings(data).filter(|(id, _)| !id.is_empty()).last()?;
    let i = q - 4 - 2 * id.len();
    if i < 84 || q + 72 > data.len() {
        return None;
    }
    let (width, height, middle, sill) = (f64_at(data, i - 84)?, f64_at(data, i - 76)?, f64_at(data, q + 24)?, f64_at(data, q + 64)?);
    let size = 0.0..50.0;
    let ok = [middle, sill].iter().all(|v| v.is_finite()) && size.contains(&width) && size.contains(&height) && width > 0.0 && height > 0.0;
    ok.then_some(OpeningFields { id, width, height, middle, sill })
}

/// The part of a contour where (p − origin) · dir ≤ c (`below`) or ≥ c (Sutherland–Hodgman).
fn clip(contour: &[[f64; 2]], origin: [f64; 2], dir: [f64; 2], c: f64, below: bool) -> Vec<[f64; 2]> {
    let inside = |p: [f64; 2]| {
        let v = (p[0] - origin[0]) * dir[0] + (p[1] - origin[1]) * dir[1] - c;
        if below { -v } else { v }
    };
    let mut out = Vec::with_capacity(contour.len() + 2);
    for i in 0..contour.len() {
        let (p, q) = (contour[i], contour[(i + 1) % contour.len()]);
        let (a, b) = (inside(p), inside(q));
        if a >= 0.0 {
            out.push(p);
        }
        if (a >= 0.0) != (b >= 0.0) {
            let t = a / (a - b);
            out.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
        }
    }
    out
}

/// A footprint (outer contour first) clipped to one side of a line square to `dir`: empty when
/// nothing of the outer contour is left.
fn clip_footprint(contours: &[Vec<[f64; 2]>], origin: [f64; 2], dir: [f64; 2], c: f64, below: bool) -> Vec<Vec<[f64; 2]>> {
    let clipped: Vec<Vec<[f64; 2]>> = contours.iter().map(|k| clip(k, origin, dir, c, below)).collect();
    if clipped.first().is_none_or(|outer| outer.len() < 3) {
        return Vec::new();
    }
    clipped.into_iter().filter(|k| k.len() >= 3).collect()
}

/// §8.9: a wall body with its openings cut out, as prisms. Each opening is a block from `a` to
/// `b` along the reference line (from `origin` in direction `dir`), through the whole wall, from
/// `low` to `high`. The pieces it touches are cut in three square to the line; the middle one
/// keeps what is below and above the opening.
fn cut_openings(wall: Prism, origin: [f64; 2], dir: [f64; 2], openings: &[[f64; 4]]) -> Vec<Prism> {
    // each piece with its range along the line, so that a piece an opening misses costs a compare
    let range = |piece: &Prism| {
        let along = piece.contours[0].iter().map(|p| (p[0] - origin[0]) * dir[0] + (p[1] - origin[1]) * dir[1]);
        along.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)))
    };
    let mut pieces = vec![(range(&wall), wall)];
    for &[a, b, low, high] in openings {
        let mut next = Vec::with_capacity(pieces.len() + 3);
        for ((first, last), piece) in pieces {
            let (z0, z1) = piece.z;
            if last <= a || first >= b || z1 <= low || z0 >= high {
                next.push(((first, last), piece));
                continue;
            }
            let mut keep = |piece: Prism| next.push((range(&piece), piece));
            for (side, c, below) in [(first < a, a, true), (last > b, b, false)] {
                let contours = if side { clip_footprint(&piece.contours, origin, dir, c, below) } else { Vec::new() };
                if !contours.is_empty() {
                    keep(Prism { contours, z: piece.z });
                }
            }
            let middle = clip_footprint(&clip_footprint(&piece.contours, origin, dir, a, false), origin, dir, b, true);
            if middle.is_empty() {
                continue;
            }
            for z in [(z0, z1.min(low)), (z0.max(high), z1)] {
                if z.1 - z.0 > 1e-6 {
                    keep(Prism { contours: middle.clone(), z });
                }
            }
        }
        pieces = next;
    }
    pieces.into_iter().map(|(_, piece)| piece).collect()
}

/// §8.11 (v0.8): the ID string that ends at q: the smallest n (0 to 255) with `01 00` and a u32
/// n before n UTF-16LE units, any characters. The text and the offset of its length field.
fn id_ending_at(data: &[u8], q: usize) -> Option<(String, usize)> {
    (0..=255usize).find_map(|n| {
        let i = q.checked_sub(4 + 2 * n)?;
        if i < 2 || data.get(i - 2..i)? != [1, 0] || u32_at(data, i)? != n as u32 {
            return None;
        }
        let units: Vec<u16> = data.get(i + 4..q)?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        Some((String::from_utf16_lossy(&units), i))
    })
}

/// §8.11: a mesh's triangulation cache: its vertices (x, y, z above the reference plane) and the
/// triangles that read, counter-clockwise from above, and how many did not.
#[derive(Debug, Clone)]
struct MeshCache {
    vertices: Vec<[f64; 3]>,
    triangles: Vec<[usize; 3]>,
    bad_triangles: usize,
}

/// §8.11: 5 bytes, nV vertices (3 f64), nT triangles (status, e0, e1, e2), nE edges (kind, -, -,
/// -, va, vb, -), ending exactly at the end of the data. A triangle (status 0) runs along its
/// edges: edge |e| − 1 from va to vb (e > 0) or back (e < 0), not of kind 1, each ending where
/// the next starts; its corners are where they start.
fn mesh_cache(data: &[u8]) -> Option<MeshCache> {
    let count = |at: usize| u32_at(data, at).map(|n| n as usize).filter(|&n| n <= 1_000_000);
    let vertices_at = 9;
    let nv = count(5)?;
    let nt_at = vertices_at + nv * 24;
    let nt = count(nt_at)?;
    let ne_at = nt_at + 4 + nt * 16;
    let ne = count(ne_at)?;
    if ne_at + 4 + ne * 28 != data.len() {
        return None;
    }
    let vertices: Vec<[f64; 3]> = (0..nv).map(|k| [0, 8, 16].map(|c| f64_at(data, vertices_at + 24 * k + c).unwrap())).collect();
    if vertices.iter().flatten().any(|v| !v.is_finite()) {
        return None;
    }
    let i32_of = |at: usize| i32_at(data, at).unwrap();
    // the directed edge e: from, to
    let edge = |e: i32| -> Option<(usize, usize)> {
        let k = (e.unsigned_abs() as usize).checked_sub(1).filter(|&k| k < ne)?;
        let at = ne_at + 4 + 28 * k;
        let (kind, a, b) = (i32_of(at), i32_of(at + 12), i32_of(at + 16));
        let ok = |v: i32| usize::try_from(v).ok().filter(|&v| v < nv);
        let (a, b) = (ok(a)?, ok(b)?);
        (kind != 1).then_some(if e > 0 { (a, b) } else { (b, a) })
    };
    let (mut triangles, mut bad_triangles) = (Vec::new(), 0);
    for t in 0..nt {
        let at = nt_at + 4 + 16 * t;
        if u32_at(data, at) != Some(0) {
            continue; // a free cell
        }
        let edges: Option<Vec<(usize, usize)>> = (1..4).map(|j| edge(i32_of(at + 4 * j))).collect();
        match edges {
            Some(d) if d[0].1 == d[1].0 && d[1].1 == d[2].0 && d[2].1 == d[0].0 => triangles.push([d[0].0, d[1].0, d[2].0]),
            _ => bad_triangles += 1,
        }
    }
    Some(MeshCache { vertices, triangles, bad_triangles })
}

/// §8.11, without a cache: after the polygon, the vertex number of each polygon point and, at the
/// end, the heights of the vertices (above the reference plane). The height of each polygon point
/// (1-based; 0 when it has none). `None` when the layout does not hold.
fn mesh_outline_heights(data: &[u8], end: usize, coords: usize) -> Option<Vec<f64>> {
    struct Reader<'a> {
        data: &'a [u8],
        at: usize,
    }
    impl Reader<'_> {
        /// The offset of the next `n` bytes, when they are there.
        fn take(&mut self, n: usize) -> Option<usize> {
            let start = self.at;
            self.at = self.at.checked_add(n).filter(|&e| e <= self.data.len())?;
            Some(start)
        }
        /// A count of at most 100 000.
        fn count(&mut self) -> Option<usize> {
            let at = self.take(4)?;
            u32_at(self.data, at).map(|n| n as usize).filter(|&n| n <= 100_000)
        }
        /// `u16 1` and a count.
        fn list(&mut self) -> Option<usize> {
            let one = self.take(2)?;
            if u16_at(self.data, one)? != 1 {
                return None;
            }
            self.count()
        }
    }
    let mut r = Reader { data, at: end };
    let m = r.list()?;
    if m != coords + 1 {
        return None;
    }
    let ids_at = r.take(4 * m)?;
    let ids: Vec<u32> = (0..m).map(|k| u32_at(data, ids_at + 4 * k).unwrap()).collect();
    for _ in 0..2 {
        let n = r.list()?;
        r.take(4 * n)?;
    }
    if r.list()? != 1 {
        return None;
    }
    let zeros = r.take(16)?;
    if data[zeros..zeros + 16].iter().any(|&b| b != 0) {
        return None;
    }
    r.take(16)?;
    for _ in 0..r.count()? {
        let one = r.take(30)?;
        if u16_at(data, one)? != 1 {
            return None;
        }
    }
    let n = r.list()?;
    r.take(4 * n)?;
    r.take(10)?;
    let mut z: HashMap<u32, f64> = HashMap::new();
    for _ in 0..r.count()? {
        let item = r.take(12)?;
        z.insert(u32_at(data, item)?, f64_at(data, item + 4).filter(|v| v.is_finite())?);
    }
    Some(ids.iter().map(|id| z.get(id).copied().unwrap_or(0.0)).collect())
}

/// A surface of terrain: points (x, y, absolute height of the top) and its triangles.
struct Surface {
    points: Vec<[f64; 3]>,
    triangles: Vec<[usize; 3]>,
}

/// §8.11: a mesh as a closed body: its triangles at their heights (facing up), the same at the
/// flat `bottom` (facing down) and a side face along every outer edge (an edge of one triangle
/// only). Only the top when `top_only`. The number of outer edges, or `None` when nothing could be
/// drawn.
fn push_terrain(layer: &mut Layer, surface: &Surface, bottom: f64, top_only: bool) -> Option<usize> {
    let p = &surface.points;
    // counter-clockwise from above; flat ones left out
    let triangles: Vec<[usize; 3]> = surface
        .triangles
        .iter()
        .filter(|t| t.iter().all(|&i| i < p.len()))
        .filter_map(|&[a, b, c]| {
            let cross = (p[b][0] - p[a][0]) * (p[c][1] - p[a][1]) - (p[b][1] - p[a][1]) * (p[c][0] - p[a][0]);
            (cross.abs() > 1e-12).then_some(if cross > 0.0 { [a, b, c] } else { [a, c, b] })
        })
        .collect();
    if triangles.is_empty() {
        return None;
    }
    let face = |layer: &mut Layer, corners: &[[f64; 3]]| {
        let (u, v) = ([0, 1, 2].map(|k| corners[1][k] - corners[0][k]), [0, 1, 2].map(|k| corners[2][k] - corners[0][k]));
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if length.is_nan() || length <= 1e-15 {
            return;
        }
        let base = (layer.positions.len() / 3) as u32;
        for c in corners {
            layer.positions.extend(c.map(|v| v as f32));
            layer.normals.extend(n.map(|v| (v / length) as f32));
        }
        for i in 1..corners.len() as u32 - 1 {
            layer.indices.extend([base, base + i, base + i + 1]);
        }
    };
    let low = |i: usize| [p[i][0], p[i][1], bottom];
    for &[a, b, c] in &triangles {
        face(layer, &[p[a], p[b], p[c]]);
        if !top_only {
            face(layer, &[low(a), low(c), low(b)]);
        }
    }
    let mut uses: HashMap<(usize, usize), usize> = HashMap::new();
    for t in &triangles {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            *uses.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    let mut outer = 0;
    for t in &triangles {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            if uses[&(a.min(b), a.max(b))] == 1 {
                outer += 1;
                if !top_only {
                    face(layer, &[low(a), low(b), p[b], p[a]]);
                }
            }
        }
    }
    Some(outer)
}

/// §8.10: the fields of a placed library object.
#[derive(Debug, Clone, Copy)]
struct ObjectFields {
    /// x, y: its anchor point; rotation (rad); z: its bottom above its story; A and B.
    at: [f64; 2],
    rotation: f64,
    z: f64,
    size: [f64; 2],
}

/// Why a library object is not drawn (§8.10).
#[derive(Debug, Clone, Copy, PartialEq)]
enum ObjectSkip {
    /// No single place for its fields.
    Ambiguous,
    /// ZZYZX is 0: a 2D object.
    Flat,
    Unreadable,
}

/// §8.10: x, y, rotation, z, A and B at p, all finite.
fn object_fields_at(data: &[u8], p: usize) -> Option<ObjectFields> {
    let f: Vec<f64> = (0..6).map(|k| f64_at(data, p + 8 * k)).collect::<Option<_>>()?;
    f.iter().all(|v| v.is_finite()).then_some(ObjectFields { at: [f[0], f[1]], rotation: f[2], z: f[3], size: [f[4], f[5]] })
}

/// §8.10: where the fields of an object are. With an outline cache, where its (x, y) (cache @75)
/// is in the object's data; otherwise (or when it is not there) the only place behind the marks
/// `01 00 00 00 00 00 01 00` (17 bytes before) with 0 < A, B < 1000 and |rotation| < 7.
fn object_fields(data: &[u8], outline: Option<&[u8; 16]>) -> Result<ObjectFields, ObjectSkip> {
    if let Some(xy) = outline {
        if let Some(f) = data.windows(16).position(|w| w == xy).and_then(|p| object_fields_at(data, p)) {
            return Ok(f);
        }
    }
    const MARKS: [u8; 8] = [1, 0, 0, 0, 0, 0, 1, 0];
    let mut found = (17..data.len().saturating_sub(47)).filter(|&p| data[p - 17..p - 9] == MARKS).filter_map(|p| object_fields_at(data, p)).filter(|f| {
        f.size.iter().all(|v| *v > 0.0 && *v < 1000.0) && f.rotation.abs() < 7.0
    });
    match (found.next(), found.next()) {
        (Some(f), None) => Ok(f),
        _ => Err(ObjectSkip::Ambiguous),
    }
}

/// §8.10: the 2D outline cache: xmin, xmax, ymin, ymax at 9 (data of 91 bytes or more).
fn object_outline(data: &[u8]) -> Option<[f64; 4]> {
    if data.len() < 91 {
        return None;
    }
    let v = [9, 17, 25, 33].map(|at| f64_at(data, at).unwrap_or(f64::NAN));
    (v.iter().all(|x| x.is_finite()) && v[0] <= v[1] && v[2] <= v[3]).then_some(v)
}

/// §8.10: ZZYZX, the height, in a parameter block: the f64 8 bytes before the name field
/// `ZZYZX` and 27 zero bytes (at 40 or later). `None` when there is no such name.
fn object_height(parameters: &[u8]) -> Option<f64> {
    let mut name = [0u8; 32];
    name[..5].copy_from_slice(b"ZZYZX");
    (40..parameters.len().saturating_sub(31)).filter(|&n| parameters[n..n + 32] == name).find_map(|n| f64_at(parameters, n - 8).filter(|h| h.is_finite()))
}

/// §8.10 (v0.7): the name of a library part, as Archicad shows it. The library reference holds
/// UTF-16 strings as in §8.2: the file name (ending in `.gsm`), then the same name without
/// `.gsm`, then the library's name. The second; without it, the first without `.gsm`.
fn library_name(data: &[u8]) -> Option<String> {
    let mut names = id_strings(data).map(|(name, _)| name).filter(|name| !name.is_empty());
    let file = names.find(|name| name.to_ascii_lowercase().ends_with(".gsm"))?;
    Some(names.next().unwrap_or_else(|| file[..file.len() - 4].to_string()))
}

/// §8.10: the footprint of an object box. With an outline cache: A × B turned, around the middle
/// of the cache, when that fits the cache within 1 mm, else the cache itself; without: A × B
/// turned around (x, y). Also says which.
fn object_footprint(f: &ObjectFields, outline: Option<[f64; 4]>) -> (Vec<[f64; 2]>, ObjectShape) {
    let (s, c) = f.rotation.sin_cos();
    let [a, b] = f.size;
    let turned = |m: [f64; 2]| {
        [(-a, -b), (a, -b), (a, b), (-a, b)].iter().map(|&(lx, ly)| [m[0] + lx / 2.0 * c - ly / 2.0 * s, m[1] + lx / 2.0 * s + ly / 2.0 * c]).collect()
    };
    match outline {
        Some([x0, x1, y0, y1]) => {
            let fits = (a * c.abs() + b * s.abs() - (x1 - x0)).abs() <= 1e-3 && (a * s.abs() + b * c.abs() - (y1 - y0)).abs() <= 1e-3;
            if fits {
                (turned([(x0 + x1) / 2.0, (y0 + y1) / 2.0]), ObjectShape::Turned)
            } else {
                (vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]], ObjectShape::Outline)
            }
        }
        None => (turned(f.at), ObjectShape::Centred),
    }
}

/// How the footprint of an object box was found (§8.10).
#[derive(Debug, Clone, Copy, PartialEq)]
enum ObjectShape {
    /// A × B turned, fitting the outline cache.
    Turned,
    /// The outline cache (larger than A × B).
    Outline,
    /// A × B turned around (x, y), without a cache.
    Centred,
}

/// A slab as drawn, for the tests: its identity, ranges and area (outer edge minus holes).
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
struct SlabOut {
    guid: String,
    x: (f64, f64),
    y: (f64, f64),
    z: (f64, f64),
    area: f64,
}

/// A roof plane as drawn, for the tests: x, y and z (lowest underside, highest top) ranges.
#[derive(Debug, Clone)]
#[cfg_attr(not(test), allow(dead_code))]
struct RoofOut {
    name: String,
    pitch: f64,
    thickness: f64,
    x: (f64, f64),
    y: (f64, f64),
    z: (f64, f64),
}

/// What keeps a column or beam from being drawn (§8.7).
enum NoSection {
    Profile,
    Missing,
}

/// §8.7: element → segment → section, the first target each time.
fn section_of(records: &Records, segment: Option<Guid>) -> Result<Section, NoSection> {
    let section = segment
        .and_then(|s| records.segments.get(&s).copied().flatten())
        .and_then(|s| records.sections.get(&s).copied())
        .ok_or(NoSection::Missing)?;
    match section {
        // §8.13: a profile whose header reads; else skipped as before
        Section::Profile(Some(p)) if records.profiles.get(&p).is_some_and(Option::is_some) => Ok(section),
        Section::Profile(_) => Err(NoSection::Profile),
        Section::Unreadable => Err(NoSection::Missing),
        s => Ok(s),
    }
}

/// A fill of a profile: its contours, the outer edge first.
type Fill = Vec<Vec<[f64; 2]>>;

/// §8.13 (v0.8): the shape of a profile: the bounding box of its header and its fills.
#[derive(Debug, Clone)]
struct ProfileShape {
    /// xmin, ymin, xmax, ymax (local, m).
    bbox: [f64; 4],
    /// The K fills, each its contours (outer first, closing point dropped, arcs as segments);
    /// `None` when they do not read (fewer than K, or one not closed): drawn as the box.
    fills: Option<Vec<Fill>>,
    /// The points of the fills as in the file (the sum of n − 1).
    points: usize,
}

impl ProfileShape {
    /// The fills to draw: those of the profile, or the box.
    fn fills(&self) -> Vec<Fill> {
        let [x0, y0, x1, y1] = self.bbox;
        self.fills.clone().unwrap_or_else(|| vec![vec![vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]]])
    }
}

/// §8.13: the header ("$mm" at 366, else the first from 40): the bounding box 3 bytes after it
/// and K 64 bytes after it, then the first K fills from 68 bytes after it.
fn profile_shape(data: &[u8]) -> Option<ProfileShape> {
    let base = if data.get(366..369) == Some(b"$mm") { 366 } else { (40..data.len().saturating_sub(2)).find(|&i| &data[i..i + 3] == b"$mm")? };
    let bbox = [3, 11, 19, 27].map(|k| f64_at(data, base + k).unwrap_or(f64::NAN));
    let k = u32_at(data, base + 64)? as usize;
    if !bbox.iter().all(|v| v.is_finite()) || bbox[0] > bbox[2] || bbox[1] > bbox[3] || !(1..=1000).contains(&k) {
        return None;
    }
    let (mut fills, mut points, mut closed) = (Vec::with_capacity(k), 0, true);
    let mut budget = SEARCH_BUDGET;
    let mut at = base + 68;
    while fills.len() < k && at < data.len() {
        match profile_fill(data, at, &mut budget) {
            Some((contours, end, n)) => {
                closed &= contours.is_some();
                fills.push(contours.unwrap_or_default());
                points += n - 1;
                at = end;
            }
            None => at += 1,
        }
    }
    let fills = (fills.len() == k && closed).then_some(fills);
    Some(ProfileShape { bbox, fills, points })
}

/// §8.13: a fill at `p`: `u16 1`, n points (point 0 unused), `u16 1`, the arcs, `u16 1`, m
/// contour ends; coordinates finite and under 1e4, arcs between points 1 and n − 1 with an angle
/// between 0 and 2π. Its contours (`None` when one is not closed), its end and n.
fn profile_fill(data: &[u8], p: usize, budget: &mut usize) -> Option<(Option<Fill>, usize, usize)> {
    let list = |at: usize| (u16_at(data, at)? == 1).then(|| u32_at(data, at + 2).map(|n| n as usize)).flatten();
    let n = list(p).filter(|n| (3..=20_000).contains(n))?;
    let points_at = p + 6;
    let arcs_head = points_at + 16 * n;
    let arcs = list(arcs_head).filter(|&a| a < n)?;
    let ends_head = arcs_head + 6 + 16 * arcs;
    let m = list(ends_head).filter(|m| (2..=n).contains(m))?;
    let end = ends_head + 6 + 4 * m;
    if end > data.len() {
        return None;
    }
    *budget = budget.checked_sub(n + arcs + m)?;
    let ends: Vec<usize> = (0..m).map(|k| u32_at(data, ends_head + 6 + 4 * k).unwrap() as usize).collect();
    if ends[0] != 0 || ends[m - 1] != n - 1 || ends.windows(2).any(|w| w[0] > w[1]) {
        return None;
    }
    let points: Vec<[f64; 2]> = (0..n).map(|k| [0, 8].map(|c| f64_at(data, points_at + 16 * k + c).unwrap())).collect();
    if points.iter().flatten().any(|v| !v.is_finite() || v.abs() >= 1e4) {
        return None;
    }
    let mut bows: HashMap<(usize, usize), f64> = HashMap::new();
    for a in 0..arcs {
        let at = arcs_head + 6 + 16 * a;
        let (from, to, angle) = (u32_at(data, at)? as usize, u32_at(data, at + 4)? as usize, f64_at(data, at + 8)?);
        let fits = |i: usize| (1..n).contains(&i);
        if !(fits(from) && fits(to) && angle.is_finite() && angle != 0.0 && angle.abs() < std::f64::consts::TAU) {
            return None;
        }
        bows.insert((from, to), angle);
    }
    let mut contours = Vec::with_capacity(m - 1);
    for k in 0..m - 1 {
        let numbers: Vec<usize> = (ends[k] + 1..=ends[k + 1]).collect();
        if numbers.len() < 2 || points[numbers[0]] != points[*numbers.last().unwrap()] {
            return Some((None, end, n));
        }
        let mut contour = Vec::with_capacity(numbers.len());
        for w in numbers.windows(2) {
            contour.push(points[w[0]]);
            let angle = bows.get(&(w[0], w[1])).copied().or_else(|| bows.get(&(w[1], w[0])).map(|a| -a));
            if let Some(angle) = angle {
                contour.extend(arc_points(points[w[0]], points[w[1]], angle));
            }
        }
        contours.push(contour);
    }
    Some((Some(contours), end, n))
}

/// §8.13: the points between `a` and `b` of a circular arc with central angle `angle` (positive:
/// counter-clockwise, the middle left of the chord), at least one segment per π/16.
fn arc_points(a: [f64; 2], b: [f64; 2], angle: f64) -> Vec<[f64; 2]> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let chord = dx.hypot(dy);
    if chord < 1e-12 {
        return Vec::new();
    }
    let along = chord / 2.0 / (angle / 2.0).tan();
    let middle = [(a[0] + b[0]) / 2.0 - dy / chord * along, (a[1] + b[1]) / 2.0 + dx / chord * along];
    let radius = (a[0] - middle[0]).hypot(a[1] - middle[1]);
    let start = (a[1] - middle[1]).atan2(a[0] - middle[0]);
    let segments = (angle.abs() / (std::f64::consts::PI / 16.0)).ceil().max(1.0) as usize;
    (1..segments)
        .map(|i| {
            let t = start + angle * i as f64 / segments as f64;
            [middle[0] + radius * t.cos(), middle[1] + radius * t.sin()]
        })
        .collect()
}

/// §8.14: the fields of a stair.
#[derive(Debug, Clone)]
struct StairFields {
    id: String,
    /// n risers of r; the going g (for the properties); the height H.
    risers: usize,
    riser: f64,
    going: f64,
    height: f64,
    /// The origin of the scheme (world) and its matrix (rows of 3: m0, m1 the local x axis, m3,
    /// m4 the local y axis).
    origin: [f64; 2],
    bottom_offset: f64,
    m: [f64; 9],
}

/// §8.14: the first ID string; the anchor a (≥ 29): u32 n (1 to 200) with the riser r (0.01 to 1)
/// and the going g (0.01 to 2) after it and H = n · r 21 bytes before it; the origin, the bottom
/// offset and the matrix after it, an orthonormal one (det ±1).
fn stair_fields(data: &[u8]) -> Option<StairFields> {
    // the first ID string with a name (an empty one comes first in the samples; Q-I40)
    let id = id_strings(data).map(|(id, _)| id).find(|id| !id.is_empty()).unwrap_or_default();
    let a = (29..data.len().saturating_sub(129)).find(|&a| {
        let n = u32_at(data, a).unwrap();
        let (r, g, h) = (f64_at(data, a + 4).unwrap(), f64_at(data, a + 12).unwrap(), f64_at(data, a - 21).unwrap());
        (1..=200).contains(&n) && r > 0.01 && r < 1.0 && g > 0.01 && g < 2.0 && h > 0.0 && (n as f64 * r - h).abs() < 1e-6
    })?;
    let f = |at: usize| f64_at(data, at).unwrap();
    let m: [f64; 9] = std::array::from_fn(|k| f(a + 57 + 8 * k));
    let values = [f(a - 29), f(a + 25), f(a + 33), f(a + 41), f(a + 49)];
    let unit = |x: f64, y: f64| (x.hypot(y) - 1.0).abs() < 1e-6;
    let ok = values.iter().chain(&m).all(|v| v.is_finite())
        && unit(m[0], m[1])
        && unit(m[3], m[4])
        && (m[0] * m[3] + m[1] * m[4]).abs() < 1e-6
        && [m[2], m[5], m[6], m[7]].iter().all(|&v| v == 0.0)
        && m[8] == 1.0;
    ok.then(|| StairFields {
        id,
        risers: u32_at(data, a).unwrap() as usize,
        riser: f(a + 4),
        going: f(a + 12),
        height: f(a - 21),
        origin: [f(a + 33), f(a + 41)],
        bottom_offset: f(a + 49),
        m,
    })
}

/// §8.14: a stair's scheme in the plan (local): its riser lines and its treads without nosing
/// (L2), in walking order.
#[derive(Debug, Clone)]
struct StairScheme {
    risers: Vec<[f64; 4]>,
    treads: Vec<Vec<Vec<[f64; 2]>>>,
}

/// §8.14: 9 bytes, nR riser lines of 318 bytes (`03 00 3D 00`, x1, y1, x2, y2), then the lists L1
/// (with nosing) and L2 (without), each a count and items (`02 01`, u32 len, `01 1F 00`, the
/// polygon at 9) of 6 + len bytes, as many in both.
fn stair_scheme(data: &[u8]) -> Option<StairScheme> {
    let nr = u32_at(data, 9)? as usize;
    let mut at = 13usize;
    let mut risers = Vec::with_capacity(nr.min(1000));
    for _ in 0..nr {
        if data.get(at..at + 4)? != [3, 0, 0x3D, 0] || at + 318 > data.len() {
            return None;
        }
        risers.push(std::array::from_fn(|k| f64_at(data, at + 4 + 8 * k).unwrap()));
        at += 318;
    }
    let mut budget = 1usize << 24;
    let mut lists = Vec::with_capacity(2);
    for _ in 0..2 {
        let count = u32_at(data, at)? as usize;
        at += 4;
        let mut polygons = Vec::with_capacity(count.min(1000));
        for _ in 0..count {
            let len = u32_at(data, at + 2)? as usize;
            let end = at.checked_add(6 + len).filter(|&e| e <= data.len())?;
            if data.get(at..at + 2)? != [2, 1] || data.get(at + 6..at + 9)? != [1, 0x1F, 0] {
                return None;
            }
            polygons.push(polygon_at(data.get(at + 9..end)?, 0, &mut budget)?.contours);
            at = end;
        }
        lists.push(polygons);
    }
    let treads = lists.pop()?;
    (!treads.is_empty() && lists[0].len() == treads.len() && risers.iter().flatten().all(|v| v.is_finite())).then_some(StairScheme { risers, treads })
}

/// §8.15: the lines of a railing segment (absolute): its reference line (t 0), its lower line
/// (t 1) and its upper line (t 2, the lower one raised by h), from start to end.
#[derive(Debug, Clone, Copy)]
struct SegmentLines {
    lower: [[f64; 3]; 2],
    upper: [[f64; 3]; 2],
}

/// §8.15: from 9, every `u32 t (0..64) · u32 2 · six f64` (finite, under 1e5, not all 0), 56 bytes
/// on after each; the first line of each t. Lines 0, 1 and 2 must be there, line 2 above line 1
/// by the same h > 0 at both ends. A missing line 1 or 2 is taken from its copy, line 3 or 5: in
/// `DEMO.pln` one sloped segment has no line 2 (open question Q-I39).
fn segment_lines(data: &[u8]) -> Option<SegmentLines> {
    let mut first: [Option<[[f64; 3]; 2]>; 6] = [None; 6];
    let mut at = 9;
    while at + 56 <= data.len() {
        let t = u32_at(data, at).unwrap();
        let v: [f64; 6] = std::array::from_fn(|k| f64_at(data, at + 8 + 8 * k).unwrap());
        if t <= 64 && u32_at(data, at + 4) == Some(2) && v.iter().all(|x| x.is_finite() && x.abs() < 1e5) && v.iter().any(|&x| x != 0.0) {
            if let Some(slot) = first.get_mut(t as usize).filter(|s| s.is_none()) {
                *slot = Some([[v[0], v[1], v[2]], [v[3], v[4], v[5]]]);
            }
            at += 56;
        } else {
            at += 1;
        }
    }
    first[0]?;
    let (lower, upper) = (first[1].or(first[3])?, first[2].or(first[5])?);
    let same = |a: [f64; 3], b: [f64; 3]| (a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6;
    let (h0, h1) = (upper[0][2] - lower[0][2], upper[1][2] - lower[1][2]);
    (same(lower[0], upper[0]) && same(lower[1], upper[1]) && h0 > 0.0 && (h0 - h1).abs() < 1e-6).then_some(SegmentLines { lower, upper })
}

/// §8.15: the bounding box of a railing (xmin, ymin, xmax, ymax, zmin, zmax) at 5 (data[0] = 2)
/// or 9 (data[0] = 1).
fn railing_box(data: &[u8]) -> Option<[f64; 6]> {
    let at = match data.first()? {
        2 => 5,
        1 => 9,
        _ => return None,
    };
    let b: [f64; 6] = std::array::from_fn(|k| f64_at(data, at + 8 * k).unwrap_or(f64::NAN));
    (b.iter().all(|v| v.is_finite()) && b[0] <= b[2] && b[1] <= b[3] && b[4] < b[5]).then_some(b)
}

/// A box along a line from `a` to `b` in plan: `half` to both sides, from `low` to `high` above
/// the line's own heights (at `a` and at `b`). False when the line has no length in plan.
fn push_along(layer: &mut Layer, a: [f64; 3], b: [f64; 3], half: f64, (low, high): (f64, f64)) -> bool {
    let length = (b[0] - a[0]).hypot(b[1] - a[1]);
    if length.is_nan() || length < 1e-9 {
        return false;
    }
    let n = [-(b[1] - a[1]) / length * half, (b[0] - a[0]) / length * half];
    let end = |p: [f64; 3]| vec![[p[0] + n[0], p[1] + n[1], p[2] + low], [p[0] - n[0], p[1] - n[1], p[2] + low], [p[0] - n[0], p[1] - n[1], p[2] + high], [p[0] + n[0], p[1] + n[1], p[2] + high]];
    push_hull(layer, &[end(a), end(b)])
}

/// §8.16.3: a morph's body: its vertices (local) and faces, each a list of loops of vertex
/// indices (the outer loop counter-clockwise seen from outside, then the holes); open when an
/// edge has only one face.
#[derive(Debug, Clone)]
struct Body {
    vertices: Vec<[f64; 3]>,
    faces: Vec<Vec<Vec<usize>>>,
    open: bool,
}

/// §8.16.3: the body at 156 (else at the first offset from 40 where the whole layout holds; at
/// most 64 tries).
fn morph_body(data: &[u8]) -> Option<Body> {
    if let Some(body) = morph_body_at(data, 156) {
        return Some(body);
    }
    (40..data.len().saturating_sub(4))
        .filter(|&at| at != 156 && u32_at(data, at).is_some_and(|n| (1..=2_000_000).contains(&n) && at + 4 + 24 * n as usize <= data.len()))
        .take(64)
        .find_map(|at| morph_body_at(data, at))
}

/// §8.16.3: nV vertices, c0 skipped items of 8 bytes, nE edges (-, -, va, vb, f1, f2), nP edge
/// references (±(k + 1) for edge k, 0 between loops), nN normals and nF faces (-, -, -, -, -,
/// normal, first, last, -, -), each loop closing.
fn morph_body_at(data: &[u8], at: usize) -> Option<Body> {
    let mut r = at;
    let mut count = |size: usize, max: usize| -> Option<(usize, usize)> {
        let n = u32_at(data, r)? as usize;
        if n > max {
            return None;
        }
        let start = r + 4;
        r = start.checked_add(n.checked_mul(size)?).filter(|&e| e <= data.len())?;
        Some((n, start))
    };
    let (nv, vertices_at) = count(24, 2_000_000)?;
    let (_, _) = count(8, 64 * nv + 1000)?;
    let (ne, edges_at) = count(24, 2_000_000)?;
    let (np, refs_at) = count(4, usize::MAX >> 8)?;
    let (nn, _) = count(24, usize::MAX >> 8)?;
    let (nf, faces_at) = count(40, 2_000_000)?;
    if nv == 0 || ne == 0 || nf == 0 {
        return None;
    }
    let vertices: Vec<[f64; 3]> = (0..nv).map(|k| [0, 8, 16].map(|c| f64_at(data, vertices_at + 24 * k + c).unwrap())).collect();
    if vertices.iter().flatten().any(|v| !v.is_finite()) {
        return None;
    }
    let u = |at: usize| u32_at(data, at).unwrap();
    let mut open = false;
    let mut edges = Vec::with_capacity(ne);
    for k in 0..ne {
        let at = edges_at + 24 * k;
        let (a, b) = (u(at + 8) as usize, u(at + 12) as usize);
        if a >= nv || b >= nv {
            return None;
        }
        open |= u(at + 16) == u32::MAX || u(at + 20) == u32::MAX;
        edges.push((a, b));
    }
    // the faces' ranges of references are contiguous, from 0 to nP − 1 (§8.16.3): each reference
    // is visited once, nP steps for all faces
    let mut faces = Vec::with_capacity(nf.min(1 << 16));
    let mut next = 0usize;
    for f in 0..nf {
        let at = faces_at + 40 * f;
        let (normal, first, last) = (u(at + 20) as usize, u(at + 24) as usize, u(at + 28) as usize);
        if !(1..=nn).contains(&normal) || first != next || first > last || last >= np {
            return None;
        }
        next = last + 1;
        let mut loops = Vec::new();
        let mut directed: Vec<(usize, usize)> = Vec::new();
        for j in first..=last + 1 {
            let x = if j <= last { i32_at(data, refs_at + 4 * j).unwrap() } else { 0 };
            if x != 0 {
                let k = x.unsigned_abs() as usize - 1;
                let &(a, b) = edges.get(k)?;
                directed.push(if x > 0 { (a, b) } else { (b, a) });
                continue;
            }
            // a loop ends: it must close
            if directed.is_empty() {
                continue;
            }
            if (0..directed.len()).any(|i| directed[i].1 != directed[(i + 1) % directed.len()].0) {
                return None;
            }
            loops.push(directed.iter().map(|d| d.0).collect());
            directed.clear();
        }
        faces.push(loops);
    }
    if next != np {
        return None;
    }
    Some(Body { vertices, faces, open })
}

/// §8.16.3: a morph's name and placement: the first ID string followed (at q+5) by 12 finite
/// f64 under 1e6 that form [R | t] with R a rotation (orthonormal, det > 0).
fn morph_fields(data: &[u8]) -> Option<(String, [f64; 12])> {
    id_strings(data).find_map(|(id, q)| {
        let m: Vec<f64> = (0..12).map(|k| f64_at(data, q + 5 + 8 * k)).collect::<Option<_>>()?;
        if !m.iter().all(|v| v.is_finite() && v.abs() < 1e6) {
            return None;
        }
        let r = [[m[0], m[1], m[2]], [m[4], m[5], m[6]], [m[8], m[9], m[10]]];
        let orthonormal = (0..3).all(|i| (0..3).all(|j| ((0..3).map(|k| r[k][i] * r[k][j]).sum::<f64>() - f64::from(u8::from(i == j))).abs() < 1e-6));
        let det = r[0][0] * (r[1][1] * r[2][2] - r[1][2] * r[2][1]) - r[0][1] * (r[1][0] * r[2][2] - r[1][2] * r[2][0])
            + r[0][2] * (r[1][0] * r[2][1] - r[1][1] * r[2][0]);
        (orthonormal && det > 0.0).then(|| (id, m.try_into().unwrap()))
    })
}

/// §8.16.3: a morph body placed by its matrix (and the level e of its story): each face
/// triangulated in its own plane (ear clipping, with holes), facing out. False when nothing could
/// be drawn.
fn push_morph(layer: &mut Layer, body: &Body, m: &[f64; 12], e: f64) -> bool {
    let first = layer.indices.len();
    let world = |v: [f64; 3]| {
        [m[0] * v[0] + m[1] * v[1] + m[2] * v[2] + m[3], m[4] * v[0] + m[5] * v[1] + m[6] * v[2] + m[7], m[8] * v[0] + m[9] * v[1] + m[10] * v[2] + m[11] + e]
    };
    let points: Vec<[f64; 3]> = body.vertices.iter().map(|&v| world(v)).collect();
    for face in &body.faces {
        let Some(outer) = face.first().filter(|l| l.len() >= 3) else { continue };
        // Newell's normal of the outer loop: counter-clockwise seen from outside
        let mut n = [0.0f64; 3];
        for i in 0..outer.len() {
            let (a, b) = (points[outer[i]], points[outer[(i + 1) % outer.len()]]);
            n[0] += (a[1] - b[1]) * (a[2] + b[2]);
            n[1] += (a[2] - b[2]) * (a[0] + b[0]);
            n[2] += (a[0] - b[0]) * (a[1] + b[1]);
        }
        if n.iter().map(|v| v * v).sum::<f64>().sqrt() < 1e-15 {
            continue;
        }
        // project on the plane square to the largest component of the normal
        let axis = (0..3).max_by(|&a, &b| n[a].abs().total_cmp(&n[b].abs())).unwrap();
        let (u, v) = match axis {
            0 => (1, 2),
            1 => (2, 0),
            _ => (0, 1),
        };
        let loops: Vec<&Vec<usize>> = face.iter().filter(|l| l.len() >= 3).collect();
        let corners: Vec<usize> = loops.iter().flat_map(|l| l.iter().copied()).collect();
        let origin = points[corners[0]];
        let mut flat = Vec::with_capacity(2 * corners.len());
        let mut holes = Vec::new();
        for (k, l) in loops.iter().enumerate() {
            if k > 0 {
                holes.push(flat.len() / 2);
            }
            flat.extend(l.iter().flat_map(|&i| [points[i][u] - origin[u], points[i][v] - origin[v]]));
        }
        let Ok(triangles) = earcutr::earcut(&flat, &holes, 2) else { continue };
        for t in triangles.chunks_exact(3) {
            push_face(layer, &mut [points[corners[t[0]]], points[corners[t[1]]], points[corners[t[2]]]], n);
        }
    }
    layer.indices.len() > first
}

/// §8.13: a profile beam's point (px, py) at t along its axis: from P1 + s·N along P2 − P1, px
/// to the right of the axis (to the left when mirrored), py up from the axis heights z1, z2.
struct ProfileBeam {
    start: [f64; 3],
    along: [f64; 3],
    across: [f64; 2],
}

impl ProfileBeam {
    fn new(f: &BeamFields, e: f64, mirrored: bool) -> Option<Self> {
        let [x1, y1, x2, y2] = f.line;
        let length = (x2 - x1).hypot(y2 - y1);
        if length.is_nan() || length < 1e-9 {
            return None;
        }
        let n = [-(y2 - y1) / length, (x2 - x1) / length];
        let across = if mirrored { n } else { [-n[0], -n[1]] };
        let start = [x1 + f.offset * n[0], y1 + f.offset * n[1], e + f.top.0];
        Some(Self { start, along: [x2 - x1, y2 - y1, f.top.1 - f.top.0], across })
    }

    fn at(&self, p: [f64; 2], t: f64) -> [f64; 3] {
        [self.start[0] + t * self.along[0] + self.across[0] * p[0], self.start[1] + t * self.along[1] + self.across[1] * p[0], self.start[2] + t * self.along[2] + p[1]]
    }
}

/// Clean contours of a footprint or profile: no repeated points, at least 3 and an area, outer
/// counter-clockwise, holes clockwise. `None` when the outer contour does not hold.
fn clean_contours(footprint: &[Vec<[f64; 2]>]) -> Option<Vec<Vec<[f64; 2]>>> {
    let mut contours: Vec<Vec<[f64; 2]>> = Vec::new();
    for (k, c) in footprint.iter().enumerate() {
        let mut clean: Vec<[f64; 2]> = Vec::with_capacity(c.len());
        for p in c {
            if !p.iter().all(|v| v.is_finite()) {
                return None;
            }
            if clean.last().is_none_or(|q| (q[0] - p[0]).abs() > 1e-9 || (q[1] - p[1]).abs() > 1e-9) {
                clean.push(*p);
            }
        }
        while clean.len() > 1 && (clean[0][0] - clean[clean.len() - 1][0]).abs() <= 1e-9 && (clean[0][1] - clean[clean.len() - 1][1]).abs() <= 1e-9 {
            clean.pop();
        }
        let a = area(&clean);
        if clean.len() < 3 || a.abs() < 1e-12 {
            if k == 0 {
                return None;
            }
            continue;
        }
        if (a > 0.0) != (k == 0) {
            clean.reverse();
        }
        contours.push(clean);
    }
    Some(contours)
}

/// A flat-shaded face of three or more corners in this order (a fan), facing where `out` points.
fn push_face(layer: &mut Layer, corners: &mut [[f64; 3]], out: [f64; 3]) {
    let (u, v) = ([0, 1, 2].map(|k| corners[1][k] - corners[0][k]), [0, 1, 2].map(|k| corners[2][k] - corners[0][k]));
    let mut n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if length.is_nan() || length <= 1e-15 {
        return;
    }
    if n[0] * out[0] + n[1] * out[1] + n[2] * out[2] < 0.0 {
        corners.reverse();
        n = n.map(|v| -v);
    }
    let base = (layer.positions.len() / 3) as u32;
    for c in corners.iter() {
        layer.positions.extend(c.map(|v| v as f32));
        layer.normals.extend(n.map(|v| (v / length) as f32));
    }
    for i in 1..corners.len() as u32 - 1 {
        layer.indices.extend([base, base + i, base + i + 1]);
    }
}

/// §8.13: a profile beam: each fill (with its holes) as a prism along the axis, with end faces
/// square to the axis in plan and the section vertical. False when nothing could be drawn.
fn push_profile_beam(layer: &mut Layer, beam: &ProfileBeam, fills: &[Fill]) -> bool {
    let first = layer.indices.len();
    let axis = beam.along;
    for fill in fills {
        let Some(contours) = clean_contours(fill) else { continue };
        let origin = contours[0][0];
        let mut flat = Vec::new();
        let mut holes = Vec::new();
        for (k, c) in contours.iter().enumerate() {
            if k > 0 {
                holes.push(flat.len() / 2);
            }
            flat.extend(c.iter().flat_map(|p| [p[0] - origin[0], p[1] - origin[1]]));
        }
        let Ok(triangles) = earcutr::earcut(&flat, &holes, 2) else { continue };
        let points: Vec<[f64; 2]> = contours.iter().flatten().copied().collect();
        let back = axis.map(|v| -v);
        for t in triangles.chunks_exact(3) {
            push_face(layer, &mut [0, 1, 2].map(|k| beam.at(points[t[k]], 0.0)), back);
            push_face(layer, &mut [0, 1, 2].map(|k| beam.at(points[t[k]], 1.0)), axis);
        }
        for c in &contours {
            for i in 0..c.len() {
                let (p, q) = (c[i], c[(i + 1) % c.len()]);
                // outwards in the profile: right of the edge (outer counter-clockwise, holes clockwise)
                let (ox, oy) = (q[1] - p[1], -(q[0] - p[0]));
                let out = [beam.across[0] * ox, beam.across[1] * ox, oy];
                push_face(layer, &mut [beam.at(p, 0.0), beam.at(q, 0.0), beam.at(q, 1.0), beam.at(p, 1.0)], out);
            }
        }
    }
    layer.indices.len() > first
}

/// A profile for the properties: "profile, 512 × 317 mm" (its bounding box), and how it is drawn.
fn profile_text(shape: &ProfileShape) -> String {
    let [x0, y0, x1, y1] = shape.bbox;
    let block = if shape.fills.is_none() { ", drawn as its bounding box" } else { "" };
    format!("profile, {:.0} × {:.0} mm{block}", (x1 - x0) * 1000.0, (y1 - y0) * 1000.0)
}

/// A section for the properties: "200 × 350 mm", "⌀ 200 mm" or "⌀ 200 × 300 mm".
fn section_text(section: Section) -> String {
    let mm = |v: f64| format!("{:.0}", v * 1000.0);
    match section {
        Section::Rect(b, d) => format!("{} × {} mm", mm(b), mm(d)),
        Section::Round(b, d) if mm(b) == mm(d) => format!("⌀ {} mm", mm(b)),
        Section::Round(b, d) => format!("⌀ {} × {} mm", mm(b), mm(d)),
        _ => String::new(),
    }
}

/// §6, §8: the records as layers of walls and slabs, one element each, on their stories.
fn convert(records: Records) -> Result<(Loaded, Stats), String> {
    let mut stats = Stats { walls_skipped: records.walls_too_large, slabs_skipped: records.slabs_too_large, ..Stats::default() };
    // §6: a story's level is in a story-level record whose 0x12 association targets the story
    let mut level_of: HashMap<Guid, f64> = HashMap::new();
    for (targets, z) in &records.levels {
        if let Some(story) = targets.iter().find(|t| records.stories.contains_key(*t) || **t == TOP_STORY) {
            level_of.entry(*story).or_insert(*z);
        }
    }
    let mut levels: Vec<f64> = level_of.values().copied().collect();
    levels.sort_by(f64::total_cmp);
    levels.dedup();
    let mut stories: Vec<(&Guid, &(String, i16), f64)> = records
        .stories
        .iter()
        .filter(|(g, _)| **g != TOP_STORY)
        .filter_map(|(g, s)| Some((g, s, *level_of.get(g)?)))
        .collect();
    stories.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.1 .1.cmp(&b.1 .1)));
    // unnamed stories are shown by their index
    let story_name = |(name, index): &(String, i16)| if name.trim().is_empty() { format!("Story {index}") } else { name.clone() };
    let names: HashMap<Guid, String> = stories.iter().map(|(g, s, _)| (**g, story_name(s))).collect();
    stats.stories = stories.iter().map(|(_, s, z)| (s.0.clone(), *z)).collect();
    stats.levels = levels.clone();

    let mut walls = Layer { name: "Walls".into(), color: [0.86, 0.84, 0.80], ..Layer::default() };
    let mut slabs = Layer { name: "Slabs".into(), color: [0.66, 0.68, 0.72], ..Layer::default() };
    let mut columns = Layer { name: "Columns".into(), color: [0.78, 0.76, 0.72], ..Layer::default() };
    let mut beams = Layer { name: "Beams".into(), color: [0.74, 0.70, 0.64], ..Layer::default() };
    let mut roofs = Layer { name: "Roofs".into(), color: [0.62, 0.42, 0.36], ..Layer::default() };
    // §8.9: the opening volumes start hidden, as those of IFC files
    let mut openings = Layer { name: "Openings".into(), color: [0.55, 0.75, 0.9], hidden: true, ..Layer::default() };
    let mut objects = Layer { name: "Objects".into(), color: [0.72, 0.64, 0.52], ..Layer::default() };
    let mut terrain = Layer { name: "Terrain".into(), color: [0.56, 0.64, 0.46], ..Layer::default() };
    // §8.12: zones overlap walls, slabs and roofs: half transparent, and hidden at first as the
    // spaces of IFC files
    let mut zones = Layer { name: "Zones".into(), color: [0.58, 0.74, 0.9], hidden: true, ..Layer::default() };
    let mut morphs = Layer { name: "Morphs".into(), color: [0.70, 0.70, 0.66], ..Layer::default() };
    let mut stairs = Layer { name: "Stairs".into(), color: [0.80, 0.78, 0.74], ..Layer::default() };
    // §8.15: the band of a railing half transparent, its rail and posts not
    let mut railings = Layer { name: "Railings".into(), color: [0.60, 0.62, 0.66], colors: Some(Vec::new()), ..Layer::default() };
    let mut next_id = 0u64;
    let mut add = |layer: &mut Layer,
                   record: &Record,
                   z: (f64, f64),
                   draw: &dyn Fn(&mut Layer) -> bool,
                   mut props: serde_json::Map<String, serde_json::Value>|
     -> bool {
        let first = layer.indices.len() as u32;
        if !draw(layer) {
            return false;
        }
        next_id += 1;
        layer.elements.push((next_id, first, layer.indices.len() as u32 - first));
        props.insert("GUID".into(), hex(&record.guid).into());
        if let Some(level) = record.story.and_then(|s| names.get(&s)) {
            props.insert("level".into(), level.clone().into());
        }
        props.insert("Bottom elevation".into(), format!("{:.0} mm", z.0 * 1000.0).into());
        props.insert("Top elevation".into(), format!("{:.0} mm", z.1 * 1000.0).into());
        layer.props.push((next_id, serde_json::Value::Object(props).to_string()));
        true
    };

    for record in &records.walls {
        let Some(f) = wall_fields(&record.data) else {
            stats.walls_skipped += 1;
            stats.openings_wall_skipped += record.openings.len();
            continue;
        };
        let polygon = find_polygon(&record.data);
        let stored = polygon.as_ref().and_then(|p| p.z);
        let has_polygon = polygon.as_ref().is_some_and(|p| p.coords >= 3);
        // §8.3: the top story of the link object, when it is a story with a level
        let linked = record
            .top_link
            .and_then(|link| records.top_links.get(&link))
            .and_then(|targets| targets.iter().find_map(|t| Some((t, *level_of.get(t)?))));
        let z = stored.unwrap_or_else(|| {
            let e = record.story.and_then(|s| level_of.get(&s)).copied().unwrap_or(0.0);
            wall_heights(&f, e, linked.map(|l| l.1))
        });
        // a top not above the bottom is damaged or not understood
        if z.1.is_nan() || z.0.is_nan() || z.1 <= z.0 {
            stats.walls_no_height += 1;
            stats.walls_skipped += 1;
            stats.openings_wall_skipped += record.openings.len();
            continue;
        }
        let footprint = match polygon {
            Some(p) if has_polygon => Some(p.contours),
            _ => wall_rectangle(&f).map(|r| vec![r]),
        };
        let mut props = serde_json::Map::new();
        props.insert("Name".into(), (if f.id.is_empty() { "Wall" } else { &f.id }).into());
        props.insert("category".into(), "Wall".into());
        let [t0, t1] = f.thickness.map(|t| (t * 1000.0).round());
        props.insert("Thickness".into(), (if t0 == t1 { format!("{t0:.0} mm") } else { format!("{t0:.0}–{t1:.0} mm") }).into());
        let top = match linked {
            Some((story, _)) => format!("linked to {}", names.get(story).map_or("the story above the top story", String::as_str)),
            None => "not linked".to_string(),
        };
        props.insert("Top".into(), top.into());
        // §8.9: the windows and doors, as blocks along the reference line from its start
        let e = record.story.and_then(|s| level_of.get(&s)).copied().unwrap_or(0.0);
        let [x1, y1, x2, y2] = f.line;
        let length = (x2 - x1).hypot(y2 - y1);
        let dir = [(x2 - x1) / length, (y2 - y1) / length];
        let mut cuts: Vec<([f64; 4], Guid, bool, OpeningFields)> = Vec::new();
        stats.openings_too_many += record.openings.len().saturating_sub(MAX_OPENINGS);
        for opening in record.openings.iter().take(MAX_OPENINGS) {
            let Some((door, data)) = records.openings.get(opening) else {
                stats.openings_missing += 1;
                continue;
            };
            match opening_fields(data).filter(|_| length >= 1e-9) {
                Some(o) => cuts.push(([o.middle - o.width / 2.0, o.middle + o.width / 2.0, e + o.sill, e + o.sill + o.height], *opening, *door, o)),
                None => stats.openings_unreadable += 1,
            }
        }
        if !cuts.is_empty() {
            props.insert("Openings".into(), cuts.len().to_string().into());
        }
        let wall_name = if f.id.is_empty() { "Wall".to_string() } else { f.id.clone() };
        let footprint_of_wall = footprint.clone();
        let drawn = footprint.is_some_and(|contours| {
            let boxes: Vec<[f64; 4]> = cuts.iter().map(|c| c.0).collect();
            let pieces = cut_openings(Prism { contours, z }, [x1, y1], dir, &boxes);
            add(&mut walls, record, z, &|layer| pieces.iter().fold(false, |any, piece| push_prism(layer, piece) | any), props)
        });
        if drawn {
            // the opening volumes: through the wall's footprint, square to its reference line
            let normal = [-dir[1], dir[0]];
            let across = footprint_of_wall.iter().flatten().flatten().map(|p| (p[0] - x1) * normal[0] + (p[1] - y1) * normal[1]);
            let (v0, v1) = across.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)));
            for ([a, b, low, high], guid, door, o) in cuts {
                let at = |s: f64, v: f64| [x1 + dir[0] * s + normal[0] * v, y1 + dir[1] * s + normal[1] * v];
                let prism = Prism { contours: vec![vec![at(a, v0), at(b, v0), at(b, v1), at(a, v1)]], z: (low, high) };
                let mut props = serde_json::Map::new();
                props.insert("Name".into(), o.id.clone().into());
                props.insert("category".into(), "Opening".into());
                props.insert("Type".into(), (if door { "Door" } else { "Window" }).into());
                props.insert("Size".into(), format!("{:.0} × {:.0} mm", o.width * 1000.0, o.height * 1000.0).into());
                props.insert("Sill".into(), format!("{:.0} mm", o.sill * 1000.0).into());
                props.insert("Wall".into(), wall_name.clone().into());
                let element = Record { guid, story: record.story, top_link: None, segment: None, openings: Vec::new(), cache: None, data: Vec::new() };
                if add(&mut openings, &element, (low, high), &|layer| push_prism(layer, &prism), props) {
                    let (width, height, middle, sill) = (o.width, o.height, o.middle, o.sill);
                    stats.openings.push(OpeningOut { name: o.id, door, wall: wall_name.clone(), wall_length: length, middle, width, height, sill });
                } else {
                    stats.openings_unreadable += 1;
                }
            }
        } else {
            stats.openings_wall_skipped += cuts.len();
        }
        if drawn {
            stats.walls += 1;
            stats.walls_polygon += usize::from(has_polygon);
            stats.walls_stored_heights += usize::from(stored.is_some());
            *(if linked.is_some() { &mut stats.walls_linked } else { &mut stats.walls_unlinked }) += 1;
        } else {
            stats.walls_skipped += 1;
        }
    }

    // §8.1: a slab needs its polygon and the z cache
    for record in &records.slabs {
        let drawn = find_polygon(&record.data).and_then(|p| Some(Prism { z: p.z?, contours: p.contours })).is_some_and(|prism| {
            let mut props = serde_json::Map::new();
            props.insert("Name".into(), "Slab".into());
            props.insert("category".into(), "Slab".into());
            add(&mut slabs, record, prism.z, &|layer| push_prism(layer, &prism), props)
        });
        if drawn {
            stats.slabs += 1;
            if let Some(p) = find_polygon(&record.data) {
                let [x, y, _] = ranges(p.contours[0].iter().map(|q| [q[0], q[1], 0.0]));
                let area = p.contours.iter().enumerate().map(|(k, c)| if k == 0 { area(c).abs() } else { -area(c).abs() }).sum();
                stats.slab_list.push(SlabOut { guid: hex(&record.guid), x, y, z: p.z.unwrap_or_default(), area });
            }
        } else {
            stats.slabs_skipped += 1;
        }
    }

    // §8.5: columns, vertical prisms of their section, with heights as walls (§8.3)
    stats.columns_skipped = records.columns_too_large;
    for record in &records.columns {
        let Some(f) = column_fields(&record.data) else {
            stats.columns_skipped += 1;
            continue;
        };
        let section = match section_of(&records, record.segment) {
            Ok(section) => section,
            Err(why) => {
                *(if matches!(why, NoSection::Profile) { &mut stats.columns_profile } else { &mut stats.columns_no_section }) += 1;
                stats.columns_skipped += 1;
                continue;
            }
        };
        let linked = record
            .top_link
            .and_then(|link| records.top_links.get(&link))
            .and_then(|targets| targets.iter().find_map(|t| Some((t, *level_of.get(t)?))));
        let e = record.story.and_then(|s| level_of.get(&s)).copied().unwrap_or(0.0);
        let bottom = e + f.bottom_offset;
        let top = linked.map_or(bottom + f.height, |(_, level)| level + f.top_offset);
        // §8.13: a profile column, each fill a prism, placed as the section of §8.5
        if let Section::Profile(Some(guid)) = section {
            let shape = records.profiles[&guid].as_ref().unwrap();
            let fills = shape.fills();
            let (s, c) = f.rotation.sin_cos();
            let place = |p: &[f64; 2]| [f.center[0] + c * p[0] - s * p[1], f.center[1] + s * p[0] + c * p[1]];
            let prisms: Vec<Prism> = fills.iter().map(|fill| Prism { contours: fill.iter().map(|k| k.iter().map(place).collect()).collect(), z: (bottom, top) }).collect();
            let mut props = serde_json::Map::new();
            props.insert("Name".into(), (if f.id.is_empty() { "Column" } else { &f.id }).into());
            props.insert("category".into(), "Column".into());
            props.insert("Section".into(), profile_text(shape).into());
            let drawn = top > bottom && add(&mut columns, record, (bottom, top), &|layer| prisms.iter().fold(false, |any, p| push_prism(layer, p) | any), props);
            if drawn {
                let [x, y, _] = ranges(prisms.iter().flat_map(|p| p.contours.iter().flatten().map(|q| [q[0], q[1], 0.0])));
                let (fills, area) = (fills.len(), fills_area(&fills));
                stats.profile_blocks += usize::from(shape.fills.is_none());
                let (points, bbox, block) = (shape.points, shape.bbox, shape.fills.is_none());
                stats.profiles.push(ProfileOut { name: f.id.clone(), beam: false, flag: None, fills, points, bbox, area, block, x, y, z: (bottom, top) });
                stats.columns.push(ColumnOut { name: f.id, center: f.center, anchor: f.anchor, section, z: (bottom, top), linked: linked.is_some() });
            } else {
                stats.columns_skipped += 1;
            }
            continue;
        }
        let footprint = column_footprint(&f, section);
        let drawn = top > bottom
            && footprint.is_some_and(|contour| {
                let prism = Prism { contours: vec![contour], z: (bottom, top) };
                let mut props = serde_json::Map::new();
                props.insert("Name".into(), (if f.id.is_empty() { "Column" } else { &f.id }).into());
                props.insert("category".into(), "Column".into());
                props.insert("Section".into(), section_text(section).into());
                let link = match linked {
                    Some((story, _)) => format!("linked to {}", names.get(story).map_or("the story above the top story", String::as_str)),
                    None => "not linked".to_string(),
                };
                props.insert("Top".into(), link.into());
                add(&mut columns, record, (bottom, top), &|layer| push_prism(layer, &prism), props)
            });
        if drawn {
            stats.columns.push(ColumnOut { name: f.id, center: f.center, anchor: f.anchor, section, z: (bottom, top), linked: linked.is_some() });
        } else {
            stats.columns_skipped += 1;
        }
    }

    // §8.6: beams, from their top along the axis down by their height
    stats.beams_skipped = records.beams_too_large;
    for record in &records.beams {
        let Some(f) = beam_fields(&record.data) else {
            stats.beams_skipped += 1;
            continue;
        };
        let section = match section_of(&records, record.segment) {
            Ok(section) => section,
            Err(why) => {
                *(if matches!(why, NoSection::Profile) { &mut stats.beams_profile } else { &mut stats.beams_no_section }) += 1;
                stats.beams_skipped += 1;
                continue;
            }
        };
        let e = record.story.and_then(|s| level_of.get(&s)).copied().unwrap_or(0.0);
        // §8.13: a profile beam, its profile's origin on the axis
        if let Section::Profile(Some(guid)) = section {
            let shape = records.profiles[&guid].as_ref().unwrap();
            let flag = record.data.get(f.q + 95).copied();
            if !matches!(flag, Some(0 | 1)) {
                stats.profile_odd_flags += 1;
            }
            let fills = shape.fills();
            let Some(beam) = ProfileBeam::new(&f, e, flag == Some(1)) else {
                stats.beams_skipped += 1;
                continue;
            };
            let [x, y, z] = ranges(fills.iter().flatten().flatten().flat_map(|p| [beam.at(*p, 0.0), beam.at(*p, 1.0)]));
            let mut props = serde_json::Map::new();
            props.insert("Name".into(), (if f.id.is_empty() { "Beam" } else { &f.id }).into());
            props.insert("category".into(), "Beam".into());
            props.insert("Section".into(), profile_text(shape).into());
            if flag == Some(1) {
                props.insert("Profile".into(), "mirrored".into());
            }
            if add(&mut beams, record, z, &|layer| push_profile_beam(layer, &beam, &fills), props) {
                stats.profile_blocks += usize::from(shape.fills.is_none());
                let (fill_count, area, points, bbox, block) = (fills.len(), fills_area(&fills), shape.points, shape.bbox, shape.fills.is_none());
                stats.profiles.push(ProfileOut { name: f.id.clone(), beam: true, flag, fills: fill_count, points, bbox, area, block, x, y, z });
                let [x1, y1, x2, y2] = f.line;
                let top = (e + f.top.0 + shape.bbox[3], e + f.top.1 + shape.bbox[3]);
                stats.beams.push(BeamOut { name: f.id, start: [x1, y1], end: [x2, y2], section, top });
            } else {
                stats.beams_skipped += 1;
            }
            continue;
        }
        let drawn = beam_ends(&f, e, section).is_some_and(|ends| {
            let z = ends.iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p[2]), hi.max(p[2])));
            let mut props = serde_json::Map::new();
            props.insert("Name".into(), (if f.id.is_empty() { "Beam" } else { &f.id }).into());
            props.insert("category".into(), "Beam".into());
            props.insert("Section".into(), section_text(section).into());
            if f.top.0 != f.top.1 {
                props.insert("Slope".into(), format!("{:.0} mm over the beam", (f.top.1 - f.top.0) * 1000.0).into());
            }
            add(&mut beams, record, z, &|layer| push_hull(layer, &ends), props)
        });
        if drawn {
            let [x1, y1, x2, y2] = f.line;
            stats.beams.push(BeamOut { name: f.id, start: [x1, y1], end: [x2, y2], section, top: (e + f.top.0, e + f.top.1) });
        } else {
            stats.beams_skipped += 1;
        }
    }

    // §8.8: roof planes, their polygon between two sloped planes
    stats.roofs_skipped = records.roofs_too_large;
    for record in &records.roofs {
        let Some(polygon) = find_polygon(&record.data).filter(|p| p.at >= 300) else {
            stats.roofs_no_polygon += 1;
            stats.roofs_skipped += 1;
            continue;
        };
        let e = record.story.and_then(|s| level_of.get(&s)).copied().unwrap_or(0.0);
        let Some((f, planes)) = roof_fields(&record.data, polygon.at).and_then(|f| {
            let planes = RoofPlanes::new(&f, e, &polygon.contours[0])?;
            Some((f, planes))
        }) else {
            stats.roofs_unreadable += 1;
            stats.roofs_skipped += 1;
            continue;
        };
        // §8.16.2: a roof of thickness 0 is a helper body for solid element operations
        if f.thickness == 0.0 {
            stats.roofs_zero_thickness += 1;
            stats.roofs_skipped += 1;
            continue;
        }
        let range = |v: &mut dyn Iterator<Item = f64>| v.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
        let points = || polygon.contours.iter().flatten().copied();
        let z = (range(&mut points().map(|p| planes.bottom(p))).0, range(&mut points().map(|p| planes.top(p))).1);
        let mut props = serde_json::Map::new();
        props.insert("Name".into(), (if f.id.is_empty() { "Roof" } else { &f.id }).into());
        props.insert("category".into(), "Roof".into());
        props.insert("Pitch".into(), format!("{:.2}°", f.pitch).into());
        props.insert("Thickness".into(), format!("{:.0} mm", f.thickness * 1000.0).into());
        if add(&mut roofs, record, z, &|layer| push_solid(layer, &polygon.contours, &|p| planes.bottom(p), &|p| planes.top(p)), props) {
            let (x, y) = (range(&mut points().map(|p| p[0])), range(&mut points().map(|p| p[1])));
            stats.roofs.push(RoofOut { name: f.id, pitch: f.pitch, thickness: f.thickness, x, y, z });
        } else {
            stats.roofs_unreadable += 1;
            stats.roofs_skipped += 1;
        }
    }

    stats.openings_skipped = stats.openings_missing + stats.openings_unreadable + stats.openings_wall_skipped + stats.openings_too_many;

    // §8.10: placed library objects as boxes
    stats.objects_unreadable = records.objects_too_large;
    for object in &records.objects {
        let outline = object.outline.and_then(|g| records.outlines.get(&g));
        let library = object.library.and_then(|g| records.libraries.get(&g));
        let height = library.and_then(|l| l.0).and_then(|p| records.heights.get(&p).copied().flatten());
        let fields = object_fields(&object.data, outline.map(|o| &o.xy)).and_then(|f| match height {
            Some(0.0) => Err(ObjectSkip::Flat),
            Some(h) if h < 0.0 => Err(ObjectSkip::Unreadable),
            _ => Ok(f),
        });
        let f = match fields {
            Ok(f) => f,
            Err(why) => {
                *(match why {
                    ObjectSkip::Ambiguous => &mut stats.objects_ambiguous,
                    ObjectSkip::Flat => &mut stats.objects_flat,
                    ObjectSkip::Unreadable => &mut stats.objects_unreadable,
                }) += 1;
                continue;
            }
        };
        // no ZZYZX: a fixed height in the library part's code; drawn 1 m high
        let h = height.unwrap_or(1.0);
        let e = level_of.get(&object.story).copied().unwrap_or(0.0);
        let z = (e + f.z, e + f.z + h);
        let (footprint, shape) = object_footprint(&f, outline.and_then(|o| o.bounds));
        let name = library.and_then(|l| l.1.clone()).unwrap_or_else(|| "Object".to_string());
        let mut props = serde_json::Map::new();
        props.insert("Name".into(), name.clone().into());
        props.insert("category".into(), "Object".into());
        let mm = |v: f64| format!("{:.0}", v * 1000.0);
        props.insert("Size".into(), format!("{} × {} × {} mm", mm(f.size[0]), mm(f.size[1]), mm(h)).into());
        if height.is_none() {
            props.insert("Height".into(), "unknown (drawn 1000 mm high)".into());
        }
        let record = Record { guid: object.guid, story: Some(object.story), top_link: None, segment: None, openings: Vec::new(), cache: None, data: Vec::new() };
        let prism = Prism { contours: vec![footprint], z };
        if add(&mut objects, &record, z, &|layer| push_prism(layer, &prism), props) {
            let range = |k: usize| prism.contours[0].iter().map(|p| p[k]).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
            stats.objects.push(ObjectOut { name, size: [f.size[0], f.size[1], h], height_known: height.is_some(), shape, x: range(0), y: range(1), z });
        } else {
            stats.objects_unreadable += 1;
        }
    }
    stats.objects_skipped = stats.objects_ambiguous + stats.objects_flat + stats.objects_unreadable;

    // §8.11: meshes, their top from the triangulation cache (or the outline), a flat bottom
    stats.meshes_unreadable = records.meshes_too_large;
    for record in &records.meshes {
        let data = &record.data;
        let Some(polygon) = find_polygon(data).filter(|p| p.at >= 115) else {
            stats.meshes_no_polygon += 1;
            continue;
        };
        let p = polygon.at;
        let (reference, thickness) = (f64_at(data, p - 115).unwrap(), f64_at(data, p - 60).unwrap());
        if ![reference, thickness].iter().all(|v| v.is_finite() && v.abs() < 10_000.0) {
            stats.meshes_unreadable += 1;
            continue;
        }
        let e = record.story.and_then(|s| level_of.get(&s)).copied().unwrap_or(0.0);
        let cache = record.cache.and_then(|c| records.mesh_caches.get(&c)).and_then(Option::as_ref);
        let surface = match cache {
            Some(cache) => {
                stats.mesh_bad_triangles += cache.bad_triangles;
                Surface { points: cache.vertices.iter().map(|v| [v[0], v[1], e + reference + v[2]]).collect(), triangles: cache.triangles.clone() }
            }
            None => {
                let Some(heights) = mesh_outline_heights(data, polygon.end, polygon.coords) else {
                    stats.meshes_unreadable += 1;
                    continue;
                };
                // the outline (with holes) triangulated by ear clipping, each point at its height;
                // the points inside the outline are left out
                let points: Vec<[f64; 3]> = polygon.contours.iter().zip(&polygon.indices).flat_map(|(c, numbers)| {
                    c.iter().zip(numbers).map(|(q, &k)| [q[0], q[1], e + reference + heights[k]])
                }).collect();
                let mut flat = Vec::new();
                let mut holes = Vec::new();
                for (k, c) in polygon.contours.iter().enumerate() {
                    if k > 0 {
                        holes.push(flat.len() / 2);
                    }
                    flat.extend(c.iter().flat_map(|q| [q[0] - points[0][0], q[1] - points[0][1]]));
                }
                let triangles = earcutr::earcut(&flat, &holes, 2).unwrap_or_default();
                stats.meshes_without_cache += 1;
                Surface { points, triangles: triangles.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect() }
            }
        };
        let used: Vec<usize> = {
            let mut used: Vec<usize> = surface.triangles.iter().flatten().copied().filter(|&i| i < surface.points.len()).collect();
            used.sort_unstable();
            used.dedup();
            used
        };
        let range = |v: &mut dyn Iterator<Item = f64>| v.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
        let top = range(&mut used.iter().map(|&i| surface.points[i][2]));
        let bottom = e + reference - thickness;
        // a bottom at or above the lowest point of the top: the top only
        let top_only = bottom >= top.0;
        let name = id_ending_at(data, p - 115).map(|(name, _)| name).filter(|n| !n.is_empty()).unwrap_or_else(|| "Mesh".to_string());
        let mut props = serde_json::Map::new();
        props.insert("Name".into(), name.clone().into());
        props.insert("category".into(), "Mesh".into());
        props.insert("Surface".into(), (if cache.is_some() { "from the triangulation cache" } else { "from the outline (no cache)" }).into());
        if top_only {
            props.insert("Body".into(), "top surface only (the bottom is not below it)".into());
        }
        let outer = std::cell::Cell::new(None);
        let z = (if top_only { top.0 } else { bottom }, top.1);
        let drawn = add(&mut terrain, record, z, &|layer| {
            outer.set(push_terrain(layer, &surface, bottom, top_only));
            outer.get().is_some()
        }, props);
        if drawn {
            stats.meshes_top_only += usize::from(top_only);
            let (x, y) = (range(&mut used.iter().map(|&i| surface.points[i][0])), range(&mut used.iter().map(|&i| surface.points[i][1])));
            let (vertices, triangles, outer_edges) = (used.len(), surface.triangles.len(), outer.get().unwrap_or(0));
            stats.meshes.push(MeshOut { name, cached: cache.is_some(), vertices, triangles, outer_edges, reference, thickness, x, y, top, bottom });
        } else {
            stats.meshes_unreadable += 1;
        }
    }
    stats.meshes_skipped = stats.meshes_no_polygon + stats.meshes_unreadable;

    // §8.12: zones, prisms of their polygon from their bottom to their (linked) top
    stats.zones_unreadable = records.zones_too_large;
    for record in &records.zones {
        let data = &record.data;
        let Some(polygon) = find_polygon(data).filter(|p| p.at >= 100) else {
            stats.zones_no_polygon += 1;
            continue;
        };
        let q = polygon.at - 63;
        let Some((number, i)) = id_ending_at(data, q) else {
            stats.zones_no_number += 1;
            continue;
        };
        let name = id_ending_at(data, i.saturating_sub(2)).map(|(name, _)| name.trim_end().to_string()).unwrap_or_default();
        let [bottom_offset, height, floor] = [0, 8, 16].map(|k| f64_at(data, q + k).unwrap_or(f64::NAN));
        if ![bottom_offset, height, floor].iter().all(|v| v.is_finite() && v.abs() < 1000.0) {
            stats.zones_unreadable += 1;
            continue;
        }
        let e = record.story.and_then(|s| level_of.get(&s)).copied().unwrap_or(0.0);
        let bottom = e + bottom_offset;
        // the top linked to a story: its level plus T; else the floor layer and the height
        let linked = record
            .top_link
            .and_then(|link| records.top_links.get(&link))
            .and_then(|targets| targets.iter().find_map(|t| Some((t, *level_of.get(t)?))));
        let top_offset = data.len().checked_sub(33).and_then(|at| f64_at(data, at)).filter(|t| t.is_finite() && t.abs() < 1000.0);
        let top = match (linked, top_offset) {
            (Some((_, level)), Some(t)) => level + t,
            _ if height <= 0.0 => {
                stats.zones_no_height += 1;
                continue;
            }
            _ => bottom + floor + height,
        };
        if top.is_nan() || top <= bottom {
            stats.zones_no_height += 1;
            continue;
        }
        let area = polygon.contours.iter().enumerate().map(|(k, c)| if k == 0 { area(c).abs() } else { -area(c).abs() }).sum::<f64>();
        let mut props = serde_json::Map::new();
        props.insert("Name".into(), (if name.is_empty() { number.clone() } else { format!("{number} {name}") }).into());
        props.insert("category".into(), "Zone".into());
        props.insert("Number".into(), number.clone().into());
        props.insert("Zone name".into(), name.clone().into());
        props.insert("Area".into(), format!("{area:.2} m² (computed)").into());
        let link = match linked.filter(|_| top_offset.is_some()) {
            Some((story, _)) => format!("linked to {}", names.get(story).map_or("the story above the top story", String::as_str)),
            None => "not linked".to_string(),
        };
        props.insert("Top".into(), link.into());
        let prism = Prism { contours: polygon.contours.clone(), z: (bottom, top) };
        if add(&mut zones, record, (bottom, top), &|layer| push_prism(layer, &prism), props) {
            let range = |k: usize| polygon.contours[0].iter().map(|p| p[k]).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)));
            let linked = linked.is_some() && top_offset.is_some();
            stats.zones.push(ZoneOut { number, name, linked, holes: polygon.contours.len() - 1, z: (bottom, top), x: range(0), y: range(1), area });
        } else {
            stats.zones_unreadable += 1;
        }
    }
    stats.zones_skipped = stats.zones_no_polygon + stats.zones_no_number + stats.zones_unreadable + stats.zones_no_height;
    zones.colors = Some(vec![[148, 189, 230, 90]; zones.vertex_count()]);

    // §8.16.3: morphs, their body placed by their matrix; a body may serve several morphs
    stats.morphs_no_body = records.morphs_too_large;
    for record in &records.morphs {
        let Some(body) = record.cache.and_then(|b| records.bodies.get(&b)).and_then(Option::as_ref) else {
            stats.morphs_no_body += 1;
            continue;
        };
        let Some((name, m)) = morph_fields(&record.data) else {
            stats.morphs_no_matrix += 1;
            continue;
        };
        let e = record.story.and_then(|s| level_of.get(&s)).copied().unwrap_or(0.0);
        let world = |v: &[f64; 3]| [m[0] * v[0] + m[1] * v[1] + m[2] * v[2] + m[3], m[4] * v[0] + m[5] * v[1] + m[6] * v[2] + m[7], m[8] * v[0] + m[9] * v[1] + m[10] * v[2] + m[11] + e];
        let [x, y, z] = ranges(body.faces.iter().flatten().flatten().map(|&i| world(&body.vertices[i])));
        let mut props = serde_json::Map::new();
        props.insert("Name".into(), (if name.is_empty() { "Morph".to_string() } else { name.clone() }).into());
        props.insert("category".into(), "Morph".into());
        props.insert("Faces".into(), body.faces.len().to_string().into());
        if body.open {
            props.insert("Body".into(), "open (an edge with one face)".into());
        }
        if add(&mut morphs, record, z, &|layer| push_morph(layer, body, &m, e), props) {
            stats.morphs_open += usize::from(body.open);
            stats.morphs.push(MorphOut { name, faces: body.faces.len(), open: body.open, x, y, z });
        } else {
            stats.morphs_no_body += 1;
        }
    }
    stats.morphs_skipped = stats.morphs_no_body + stats.morphs_no_matrix;

    // §8.14: stairs, a block per tread from the scheme, at the heights of the walking order
    stats.stairs_fields = records.stairs_too_large;
    for record in &records.stairs {
        let Some(f) = stair_fields(&record.data) else {
            stats.stairs_fields += 1;
            continue;
        };
        let scheme = record.cache.and_then(|c| records.schemes.get(&c)).and_then(Option::as_ref).filter(|s| s.risers.len() == f.risers);
        let Some(scheme) = scheme else {
            stats.stairs_scheme += 1;
            continue;
        };
        // does the walking order start (f) and end (g') with a riser?
        let nt = scheme.treads.len();
        let centroid = |k: usize| {
            let c = &scheme.treads[k][0];
            [c.iter().map(|p| p[0]).sum::<f64>() / c.len() as f64, c.iter().map(|p| p[1]).sum::<f64>() / c.len() as f64]
        };
        let s = |j: usize, a: usize, b: usize| {
            let r = scheme.risers[j - 1];
            let (m, ca, cb) = ([(r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0], centroid(a), centroid(b));
            let d = [cb[0] - ca[0], cb[1] - ca[1]];
            ((m[0] - ca[0]) * d[0] + (m[1] - ca[1]) * d[1]) / (d[0] * d[0] + d[1] * d[1])
        };
        let starts = nt == 1 || s(1, 0, 1) < 0.0;
        let ends = nt >= 2 && s(f.risers, nt - 2, nt - 1) > 1.0;
        if f.risers != nt - 1 + usize::from(starts) + usize::from(ends) {
            stats.stairs_sequence += 1;
            continue;
        }
        let e = record.story.and_then(|s| level_of.get(&s)).copied().unwrap_or(0.0);
        let bottom = e + f.bottom_offset;
        let m = f.m;
        let place = |p: &[f64; 2]| [f.origin[0] + p[0] * m[0] + p[1] * m[3], f.origin[1] + p[0] * m[1] + p[1] * m[4]];
        let mut blocks = Vec::with_capacity(nt);
        for (i, tread) in scheme.treads.iter().enumerate() {
            let top = bottom + (i + usize::from(starts)) as f64 * f.riser;
            if i == 0 && !starts {
                // a tread on the floor the stair starts from
                stats.stairs_floor_treads += 1;
                continue;
            }
            blocks.push(Prism { contours: tread.iter().map(|c| c.iter().map(place).collect()).collect(), z: (top - f.riser, top) });
        }
        let mut props = serde_json::Map::new();
        props.insert("Name".into(), (if f.id.is_empty() { "Stair" } else { &f.id }).into());
        props.insert("category".into(), "Stair".into());
        props.insert("Risers".into(), format!("{} × {:.1} mm", f.risers, f.riser * 1000.0).into());
        props.insert("Going".into(), format!("{:.1} mm", f.going * 1000.0).into());
        let top = bottom + f.height;
        if add(&mut stairs, record, (bottom, top), &|layer| blocks.iter().fold(false, |any, b| push_prism(layer, b) | any), props) {
            let [x, y, _] = ranges(scheme.treads.iter().flat_map(|t| t[0].iter()).map(|p| {
                let q = place(p);
                [q[0], q[1], 0.0]
            }));
            let first = bottom + usize::from(starts) as f64 * f.riser;
            let tread_z = (first, bottom + (nt - 1 + usize::from(starts)) as f64 * f.riser);
            let mirrored = m[0] * m[4] - m[1] * m[3] < 0.0;
            let (risers, riser, going) = (f.risers, f.riser, f.going);
            stats.stairs.push(StairOut { name: f.id, risers, riser, going, z: (bottom, top), starts_with_riser: starts, treads: nt, tread_z, mirrored, x, y });
        } else {
            stats.stairs_fields += 1;
        }
    }
    stats.stairs_skipped = stats.stairs_fields + stats.stairs_scheme + stats.stairs_sequence;

    // §8.15: railings, per segment a band, a rail on top and posts, from the segment caches
    for record in &records.railings {
        let lines: Vec<SegmentLines> = record
            .openings
            .iter()
            .filter_map(|segment| {
                let lines = records.railing_segments.get(segment).copied().flatten().and_then(|c| records.segment_caches.get(&c).copied().flatten());
                if lines.is_none() {
                    stats.railing_bad_segments += 1;
                }
                lines
            })
            .collect();
        let name = id_strings(&record.data).map(|(id, _)| id).find(|id| !id.is_empty()).unwrap_or_else(|| "Railing".to_string());
        let bbox = record.cache.and_then(|c| records.railing_boxes.get(&c).copied().flatten());
        if lines.is_empty() && bbox.is_none() {
            stats.railings_skipped += 1;
            continue;
        }
        let height = lines.first().map(|l| l.upper[0][2] - l.lower[0][2]);
        let mut props = serde_json::Map::new();
        props.insert("Name".into(), name.clone().into());
        props.insert("category".into(), "Railing".into());
        if let Some(h) = height {
            props.insert("Height".into(), format!("{:.0} mm", h * 1000.0).into());
        } else {
            props.insert("Body".into(), "its bounding box (no segment that reads)".into());
        }
        let points = || lines.iter().flat_map(|l| [l.lower[0], l.lower[1], l.upper[0], l.upper[1]]);
        let [x, y, z] = match bbox.filter(|_| lines.is_empty()) {
            Some(b) => [(b[0], b[2]), (b[1], b[3]), (b[4], b[5])],
            None => ranges(points()),
        };
        let draw = |layer: &mut Layer| -> bool {
            let colour = |layer: &mut Layer, from: usize, alpha: u8| {
                let new = layer.positions.len() / 3 - from;
                layer.colors.get_or_insert_with(Vec::new).extend(std::iter::repeat_n([153, 158, 168, alpha], new));
            };
            let mut any = false;
            if lines.is_empty() {
                let [x0, y0, x1, y1, z0, z1] = bbox.unwrap();
                let from = layer.positions.len() / 3;
                any |= push_prism(layer, &Prism { contours: vec![vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]], z: (z0, z1) });
                colour(layer, from, 110);
                return any;
            }
            let mut posts: Vec<[f64; 3]> = Vec::new();
            for (k, l) in lines.iter().enumerate() {
                let h = l.upper[0][2] - l.lower[0][2];
                // the band, 0.02 thick, half transparent; the rail on top, 0.05 × 0.05
                let from = layer.positions.len() / 3;
                any |= push_along(layer, l.lower[0], l.lower[1], 0.01, (0.0, h));
                colour(layer, from, 110);
                let from = layer.positions.len() / 3;
                any |= push_along(layer, l.upper[0], l.upper[1], 0.025, (-0.025, 0.025));
                colour(layer, from, 255);
                // posts at each segment's start and at the end of the last one, once per point
                let mut ends = vec![l.lower[0]];
                if k + 1 == lines.len() {
                    ends.push(l.lower[1]);
                }
                for p in ends {
                    if posts.iter().any(|q| (0..3).all(|c| (q[c] - p[c]).abs() < 1e-6)) {
                        continue;
                    }
                    posts.push(p);
                    let square = vec![vec![[p[0] - 0.025, p[1] - 0.025], [p[0] + 0.025, p[1] - 0.025], [p[0] + 0.025, p[1] + 0.025], [p[0] - 0.025, p[1] + 0.025]]];
                    let from = layer.positions.len() / 3;
                    any |= push_prism(layer, &Prism { contours: square, z: (p[2], p[2] + h) });
                    colour(layer, from, 255);
                }
            }
            any
        };
        if add(&mut railings, record, z, &draw, props) {
            let length = lines.iter().map(|l| (l.lower[1][0] - l.lower[0][0]).hypot(l.lower[1][1] - l.lower[0][1])).sum();
            stats.railings_boxed += usize::from(lines.is_empty());
            let (segments, boxed) = (lines.len(), lines.is_empty());
            stats.railings.push(RailingOut { guid: hex(&record.guid), name, segments, length, height: height.unwrap_or(z.1 - z.0), boxed, x, y, z });
        } else {
            stats.railings_skipped += 1;
        }
    }

    let elements =
        stats.walls
            + stats.slabs
            + stats.columns.len()
            + stats.beams.len()
            + stats.roofs.len()
            + stats.objects.len()
            + stats.meshes.len()
            + stats.zones.len()
            + stats.morphs.len()
            + stats.stairs.len()
            + stats.railings.len();
    if elements == 0 {
        // §10: Archicad 17 to 23 share the container but not the records
        return Err(format!(
            "Archicad: no walls, slabs, columns, beams, roofs, library objects, meshes or zones found ({} walls, {} slabs, {} \
             columns, {} beams, {} roofs, {} objects, {} meshes and {} zones skipped). This reader supports Archicad 24 to 29; \
             projects from Archicad 17 to 23 are not supported yet.",
            stats.walls_skipped,
            stats.slabs_skipped,
            stats.columns_skipped,
            stats.beams_skipped,
            stats.roofs_skipped,
            stats.objects_skipped,
            stats.meshes_skipped,
            stats.zones_skipped
        ));
    }

    let mut info = vec![
        ("Archicad walls".into(), format!("{} read, {} skipped", stats.walls, stats.walls_skipped)),
        ("Archicad slabs".into(), format!("{} read, {} skipped", stats.slabs, stats.slabs_skipped)),
        (
            "Archicad wall heights".into(),
            format!(
                "{} stored in the file; tops of {} linked to a story, {} not linked",
                stats.walls_stored_heights, stats.walls_linked, stats.walls_unlinked
            ),
        ),
        ("Archicad columns".into(), skipped_text(stats.columns.len(), stats.columns_skipped, stats.columns_profile, stats.columns_no_section)),
        ("Archicad beams".into(), skipped_text(stats.beams.len(), stats.beams_skipped, stats.beams_profile, stats.beams_no_section)),
        (
            "Archicad profile sections".into(),
            format!(
                "{} beams and {} columns drawn from their profile, {} of them as its bounding box{}",
                stats.profiles.iter().filter(|p| p.beam).count(),
                stats.profiles.iter().filter(|p| !p.beam).count(),
                stats.profile_blocks,
                if stats.profile_odd_flags > 0 { format!("; {} mirror flags neither 0 nor 1", stats.profile_odd_flags) } else { String::new() }
            ),
        ),
        ("Archicad roofs".into(), roof_text(&stats)),
        ("Archicad openings".into(), opening_text(&stats)),
        ("Archicad library objects".into(), object_text(&stats)),
        ("Archicad meshes".into(), mesh_text(&stats)),
        ("Archicad stairs".into(), {
            let mut text = reasons(stats.stairs.len(), stats.stairs_skipped, &[
                (stats.stairs_fields, "whose fields do not read"),
                (stats.stairs_scheme, "without a scheme that reads"),
                (stats.stairs_sequence, "whose risers and treads do not add up"),
            ]);
            if stats.stairs_floor_treads > 0 {
                text += &format!("; {} treads on the floor left out", stats.stairs_floor_treads);
            }
            text
        }),
        ("Archicad railings".into(), {
            let mut text = reasons(stats.railings.len(), stats.railings_skipped, &[]);
            for (n, what) in [(stats.railings_boxed, "drawn as their bounding box"), (stats.railing_bad_segments, "segments that do not read")] {
                if n > 0 {
                    text += &format!("; {n} {what}");
                }
            }
            text
        }),
        ("Archicad morphs".into(), {
            let mut text = reasons(stats.morphs.len(), stats.morphs_skipped, &[
                (stats.morphs_no_body, "without a body that reads"),
                (stats.morphs_no_matrix, "without a placement"),
            ]);
            if stats.morphs_open > 0 {
                text += &format!("; {} open", stats.morphs_open);
            }
            text
        }),
        ("Archicad zones".into(), reasons(stats.zones.len(), stats.zones_skipped, &[
            (stats.zones_no_polygon, "without a polygon"),
            (stats.zones_no_number, "without a number"),
            (stats.zones_unreadable, "whose fields do not read"),
            (stats.zones_no_height, "without a height"),
        ])),
        (
            "Archicad elements not read yet".into(),
            "Window and door frames, glass and leaves, the shapes of library objects (drawn as boxes), morph materials, curved roofs and \
             shells, zones cut by roofs, and columns and beams with a profile section"
                .into(),
        ),
    ];
    if records.damaged > 0 {
        info.push(("Archicad damaged blocks".into(), records.damaged.to_string()));
    }
    let project = ProjectData {
        info,
        sheets: layout_sheets(&records),
        levels: stories.iter().map(|(g, _, z)| (names[*g].clone(), *z as f32)).collect(),
        ..ProjectData::default()
    };
    let layers = [walls, slabs, columns, beams, roofs, openings, objects, terrain, zones, morphs, stairs, railings].into_iter().filter(|l| !l.indices.is_empty()).collect();
    Ok((Loaded { structural: Vec::new(), origin: None, layers, views: Vec::new(), project }, stats))
}

/// "8 drawn as boxes (1 with an unknown height), 1 skipped (1 two-dimensional)".
/// "4 read, 1 skipped (1 without a number)".
fn reasons(read: usize, skipped: usize, why: &[(usize, &str)]) -> String {
    let mut out = format!("{read} read, {skipped} skipped");
    let why: Vec<String> = why.iter().filter(|(n, _)| *n > 0).map(|(n, what)| format!("{n} {what}")).collect();
    if !why.is_empty() {
        out += &format!(" ({})", why.join(", "));
    }
    out
}

/// "1 read, 0 skipped; 1 from its outline, without cache".
fn mesh_text(stats: &Stats) -> String {
    let mut out = reasons(stats.meshes.len(), stats.meshes_skipped, &[
        (stats.meshes_no_polygon, "without a polygon"),
        (stats.meshes_unreadable, "whose fields do not read"),
    ]);
    for (n, what) in [
        (stats.meshes_without_cache, "from its outline, without cache"),
        (stats.meshes_top_only, "as a top surface only"),
        (stats.mesh_bad_triangles, "cache triangles left out"),
    ] {
        if n > 0 {
            out += &format!("; {n} {what}");
        }
    }
    out
}

fn object_text(stats: &Stats) -> String {
    let unknown = stats.objects.iter().filter(|o| !o.height_known).count();
    let mut out = format!("{} drawn as boxes ({unknown} with an unknown height), {} skipped", stats.objects.len(), stats.objects_skipped);
    let why: Vec<String> = [
        (stats.objects_ambiguous, "without a single place for their fields"),
        (stats.objects_flat, "two-dimensional"),
        (stats.objects_unreadable, "whose fields do not read"),
    ]
    .iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, what)| format!("{n} {what}"))
    .collect();
    if !why.is_empty() {
        out += &format!(" ({})", why.join(", "));
    }
    out
}

/// "6 doors and 8 windows cut, 1 skipped (1 whose fields do not read)".
fn opening_text(stats: &Stats) -> String {
    let doors = stats.openings.iter().filter(|o| o.door).count();
    let mut out = format!("{doors} doors and {} windows cut, {} skipped", stats.openings.len() - doors, stats.openings_skipped);
    let why: Vec<String> = [
        (stats.openings_missing, "without a record"),
        (stats.openings_unreadable, "whose fields do not read"),
        (stats.openings_wall_skipped, "in a wall that is not drawn"),
        (stats.openings_too_many, "past the first 1000 of one wall"),
    ]
    .iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, what)| format!("{n} {what}"))
    .collect();
    if !why.is_empty() {
        out += &format!(" ({})", why.join(", "));
    }
    out
}

/// "2 read, 1 skipped (1 without a polygon)".
fn roof_text(stats: &Stats) -> String {
    let mut out = format!("{} read, {} skipped", stats.roofs.len(), stats.roofs_skipped);
    let why: Vec<String> = [
        (stats.roofs_no_polygon, "without a polygon"),
        (stats.roofs_unreadable, "with fields that do not read"),
        (stats.roofs_zero_thickness, "with thickness 0 (helper bodies)"),
    ]
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, what)| format!("{n} {what}"))
        .collect();
    if !why.is_empty() {
        out += &format!(" ({})", why.join(", "));
    }
    out
}

/// "4 read, 2 skipped (1 with a profile section, 1 without a section)".
fn skipped_text(read: usize, skipped: usize, profile: usize, no_section: usize) -> String {
    let mut out = format!("{read} read, {skipped} skipped");
    let why: Vec<String> = [(profile, "with a profile section"), (no_section, "without a section")]
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, what)| format!("{n} {what}"))
        .collect();
    if !why.is_empty() {
        out += &format!(" ({})", why.join(", "));
    }
    out
}

/// The signed area of a contour (positive: counter-clockwise).
fn area(c: &[[f64; 2]]) -> f64 {
    (0..c.len()).map(|i| {
        let (a, b) = (c[i], c[(i + 1) % c.len()]);
        a[0] * b[1] - b[0] * a[1]
    }).sum::<f64>() / 2.0
}

/// Adds a prism to the layer: the footprint (outer contour minus holes) as bottom and top, and a
/// side face per edge, flat shaded. False when nothing could be drawn.
fn push_prism(layer: &mut Layer, prism: &Prism) -> bool {
    let (z0, z1) = (prism.z.0.min(prism.z.1), prism.z.0.max(prism.z.1));
    push_solid(layer, &prism.contours, &|_| z0, &|_| z1)
}

/// Adds a body to the layer: the footprint (outer contour minus holes) with vertical sides, its
/// underside and top at the heights `bottom` and `top` give for each corner (planes, for a
/// sloped roof), flat shaded. False when nothing could be drawn or the top is not above the
/// underside everywhere.
fn push_solid(layer: &mut Layer, footprint: &[Vec<[f64; 2]>], bottom: &dyn Fn([f64; 2]) -> f64, top: &dyn Fn([f64; 2]) -> f64) -> bool {
    let Some(contours) = clean_contours(footprint) else { return false };
    // caps, triangulated near the first point for precision
    let origin = contours[0][0];
    let mut flat = Vec::new();
    let mut holes = Vec::new();
    for (k, c) in contours.iter().enumerate() {
        if k > 0 {
            holes.push(flat.len() / 2);
        }
        flat.extend(c.iter().flat_map(|p| [p[0] - origin[0], p[1] - origin[1]]));
    }
    let Ok(triangles) = earcutr::earcut(&flat, &holes, 2) else { return false };
    if triangles.is_empty() {
        return false;
    }
    let points: Vec<[f64; 2]> = contours.iter().flatten().copied().collect();
    let (z0, z1): (Vec<f64>, Vec<f64>) = points.iter().map(|&p| (bottom(p), top(p))).unzip();
    if z0.iter().zip(&z1).any(|(a, b)| !a.is_finite() || !b.is_finite() || b - a <= 1e-6) {
        return false;
    }
    let vertex = |layer: &mut Layer, p: [f64; 2], z: f64, n: [f32; 3]| {
        layer.positions.extend([p[0] as f32, p[1] as f32, z as f32]);
        layer.normals.extend(n);
        (layer.positions.len() / 3 - 1) as u32
    };
    // the normal of a cap triangle, pointing up (`sign` 1) or down (-1)
    let normal = |a: [f64; 3], b: [f64; 3], c: [f64; 3], sign: f64| {
        let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [c[0] - a[0], c[1] - a[1], c[2] - a[2]]);
        let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if length > 1e-15 { n.map(|v| (sign * v / length) as f32) } else { [0.0, 0.0, sign as f32] }
    };
    for t in triangles.chunks_exact(3) {
        let (a, b, c) = (t[0], t[1], t[2]);
        let (pa, pb, pc) = (points[a], points[b], points[c]);
        let ccw = (pb[0] - pa[0]) * (pc[1] - pa[1]) - (pb[1] - pa[1]) * (pc[0] - pa[0]) > 0.0;
        // top counter-clockwise from above, bottom clockwise
        let (a, b) = if ccw { (a, b) } else { (b, a) };
        let at = |i: usize, z: &[f64]| [points[i][0], points[i][1], z[i]];
        let n = normal(at(a, &z1), at(b, &z1), at(c, &z1), 1.0);
        let top = [a, b, c].map(|i| vertex(layer, points[i], z1[i], n));
        layer.indices.extend(top);
        let n = normal(at(a, &z0), at(b, &z0), at(c, &z0), -1.0);
        let bottom = [a, c, b].map(|i| vertex(layer, points[i], z0[i], n));
        layer.indices.extend(bottom);
    }
    let mut start = 0;
    for c in &contours {
        for i in 0..c.len() {
            let (k, l) = (start + i, start + (i + 1) % c.len());
            let (p, q) = (points[k], points[l]);
            let length = (q[0] - p[0]).hypot(q[1] - p[1]);
            // outwards: right of the edge (outer counter-clockwise, holes clockwise)
            let n = [((q[1] - p[1]) / length) as f32, (-(q[0] - p[0]) / length) as f32, 0.0];
            let quad = [(p, z0[k]), (q, z0[l]), (q, z1[l]), (p, z1[k])].map(|(p, z)| vertex(layer, p, z, n));
            layer.indices.extend([quad[0], quad[1], quad[2], quad[0], quad[2], quad[3]]);
        }
        start += c.len();
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Model;
    use std::path::PathBuf;
    use std::sync::Arc;

    /// The folder with the sample projects `CH04_Part2_Walls.pln`, `CH05_Result.pln` (Archicad 25)
    /// and `DEMO.pln` (Archicad 26) of PacktPublishing/A-BIM-Professionals-Guide-to-Learning-Archicad (MIT), from
    /// `ARCHICAD_SAMPLES`. The samples are not in the repository; without it the tests that need
    /// them pass without checking anything.
    fn sample(name: &str) -> Option<PathBuf> {
        let Some(dir) = std::env::var_os("ARCHICAD_SAMPLES") else {
            eprintln!("skipped: set ARCHICAD_SAMPLES to the folder with {name}");
            return None;
        };
        let path = PathBuf::from(dir).join(name);
        assert!(path.exists(), "ARCHICAD_SAMPLES has no {name}");
        Some(path)
    }

    /// Reads a sample: the stats and the model as the app gets it.
    fn open(name: &str) -> Option<(Stats, Model)> {
        let path = sample(name)?;
        let (_, stats) = convert(read_source(&Source::File(path.clone())).expect(name)).expect(name);
        Some((stats, crate::loaders::load(&path).expect(name)))
    }

    /// Bottom and top of every element of a layer.
    fn z_ranges(model: &Model, layer: &str) -> Vec<(f32, f32)> {
        let layer = model.layers.iter().find(|l| l.name == layer).expect(layer);
        layer
            .elements
            .iter()
            .map(|&(_, first, count)| {
                let z = layer.indices[first as usize..(first + count) as usize].iter().map(|&i| layer.positions[3 * i as usize + 2]);
                z.clone().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), z| (lo.min(z), hi.max(z)))
            })
            .collect()
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    fn check_stories(stats: &Stats, model: &Model) {
        let stories: Vec<(&str, f64)> = stats.stories.iter().map(|(n, z)| (n.as_str(), *z)).collect();
        assert_eq!(stories, [("Ground Floor", 0.0), ("", 3.0), ("", 6.0)]);
        assert_eq!(stats.levels, [0.0, 3.0, 6.0, 9.0]);
        let levels: Vec<(&str, f32)> = model.project.levels.iter().map(|(n, z)| (n.as_str(), *z)).collect();
        assert_eq!(levels, [("Ground Floor", 0.0), ("Story 1", 3.0), ("Story 2", 6.0)]);
    }

    /// §11 (v0.2): walls with a top linked to a story and without, the highest wall top and the
    /// lowest wall bottom; no wall with its top at or below its bottom.
    fn check_walls(stats: &Stats, model: &Model, linked: usize, unlinked: usize, highest: f32, lowest: f32) {
        assert_eq!((stats.walls_linked, stats.walls_unlinked, stats.walls_no_height), (linked, unlinked, 0));
        let walls = z_ranges(model, "Walls");
        assert!(walls.iter().all(|(bottom, top)| top > bottom), "{walls:?}");
        let top = walls.iter().map(|w| w.1).fold(f32::NEG_INFINITY, f32::max);
        let bottom = walls.iter().map(|w| w.0).fold(f32::INFINITY, f32::min);
        assert!(near(top, highest) && near(bottom, lowest), "walls from {bottom} to {top}");
    }

    /// The bounds of the walls and slabs only (§11 gives their x and y ranges; roofs reach
    /// beyond them).
    fn walls_and_slabs_bounds(model: &Model) -> crate::model::Bounds {
        let mut b = crate::model::Bounds::empty();
        for layer in model.layers.iter().filter(|l| l.name == "Walls" || l.name == "Slabs") {
            for p in layer.positions.chunks_exact(3) {
                b.extend([p[0], p[1], p[2]]);
            }
        }
        b
    }

    /// A roof plane of §11 (v0.4): name, pitch, thickness, x, y and z ranges.
    type RoofRow = (&'static str, f64, f64, [f64; 2], [f64; 2], [f64; 2]);

    /// The roof planes (§11 v0.4), sorted by name.
    fn check_roofs(stats: &Stats, expected: &[RoofRow]) {
        let near = |a: f64, b: f64, tolerance: f64| (a - b).abs() < tolerance;
        let mut roofs = stats.roofs.clone();
        roofs.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!((roofs.len(), stats.roofs_skipped), (expected.len(), 0), "{roofs:?}");
        for (r, (name, pitch, thickness, x, y, z)) in roofs.iter().zip(expected) {
            assert_eq!(&r.name, name);
            assert!(near(r.pitch, *pitch, 2e-3) && near(r.thickness, *thickness, 1e-3), "{r:?}");
            assert!(near(r.x.0, x[0], 1e-3) && near(r.x.1, x[1], 1e-3) && near(r.y.0, y[0], 1e-3) && near(r.y.1, y[1], 1e-3), "{r:?}");
            assert!(near(r.z.0, z[0], 1e-3) && near(r.z.1, z[1], 1e-3), "{r:?}");
        }
    }

    /// The single roof plane of `CH04_Part2_Walls.pln` and `CH05_Result.pln`.
    const RT_002: RoofRow = ("RT - 002", 12.0, 0.3, [0.711, 11.311], [5.141, 12.741], [5.372, 7.932]);

    /// §11 (v0.6): the library objects drawn as boxes, those whose ZZYZX is known, none skipped;
    /// one pickable element each in the visible layer Objects.
    /// A zone of §11 (v0.8): number, name, z, x and y range, area.
    type ZoneRow = (&'static str, &'static str, (f64, f64), (f64, f64), (f64, f64), f64);

    /// §11 (v0.8): exactly these zones, all linked, without holes; hidden, closed bodies.
    fn check_zones(stats: &Stats, model: &Model, expected: &[ZoneRow]) {
        let near = |a: f64, b: f64| (a - b).abs() < 1e-3;
        let pair = |a: (f64, f64), b: (f64, f64)| near(a.0, b.0) && near(a.1, b.1);
        assert_eq!((stats.zones.len(), stats.zones_skipped), (expected.len(), 0), "{:?}", stats.zones);
        for &(number, name, z, x, y, area) in expected {
            let zone = stats.zones.iter().find(|zone| zone.number == number).unwrap_or_else(|| panic!("{number}: {:?}", stats.zones));
            assert!(zone.name == name && zone.linked && zone.holes == 0, "{zone:?}");
            assert!(pair(zone.z, z) && pair(zone.x, x) && pair(zone.y, y) && near(zone.area, area), "{zone:?}");
        }
        let layer = model.layers.iter().find(|l| l.name == "Zones").expect("zones");
        assert!(layer.hidden && layer.elements.len() == expected.len());
        assert_eq!(layer.solid_indices().len(), layer.indices.len());
        assert!(layer.colors.as_ref().is_some_and(|c| c.len() == layer.vertex_count() && c.iter().all(|c| c[3] < 255)), "half transparent");
    }

    fn check_objects(stats: &Stats, model: &Model, objects: usize, known: usize) {
        let with_height = stats.objects.iter().filter(|o| o.height_known).count();
        assert_eq!((stats.objects.len(), with_height, stats.objects_skipped), (objects, known, 0), "{:?}", stats.objects);
        let layer = model.layers.iter().find(|l| l.name == "Objects").expect("objects");
        assert!(!layer.hidden && layer.elements.len() == objects);
        assert_eq!(layer.solid_indices().len(), layer.indices.len());
        let unknown = model.props.values().filter(|p| p.contains("\"Height\":\"unknown")).count();
        assert_eq!(unknown, objects - known);
    }

    /// §11 (v0.9): the one stair of a file: name, risers, riser, going, bottom and top, the tops of
    /// its first and last tread, starting with a riser, its treads' x and y range; closed blocks.
    fn check_stair(stats: &Stats, model: &Model, (name, risers, riser, going): (&str, usize, f64, f64), z: (f64, f64), treads: (f64, f64), x: (f64, f64), y: (f64, f64)) {
        let near = |a: f64, b: f64| (a - b).abs() < 1e-3;
        let pair = |a: (f64, f64), b: (f64, f64)| near(a.0, b.0) && near(a.1, b.1);
        assert_eq!((stats.stairs.len(), stats.stairs_skipped, stats.stairs_floor_treads), (1, 0, 0));
        let s = &stats.stairs[0];
        assert!(s.name == name && s.risers == risers && s.treads == risers && s.starts_with_riser && !s.mirrored, "{s:?}");
        assert!(near(s.riser, riser) && near(s.going, going) && pair(s.z, z) && pair(s.tread_z, treads) && pair(s.x, x) && pair(s.y, y), "{s:?}");
        let layer = model.layers.iter().find(|l| l.name == "Stairs").expect("stairs");
        assert!(layer.elements.len() == 1 && layer.solid_indices().len() == layer.indices.len());
    }

    /// An opening of §11 (v0.5): name, wall, wall length, middle, width, height, sill.
    type OpeningRow = (&'static str, &'static str, f64, f64, f64, f64, f64);

    /// §11 (v0.5): the doors and windows cut, these among them; the openings layer starts hidden
    /// and the walls stay closed bodies.
    fn check_openings(stats: &Stats, model: &Model, doors: usize, windows: usize, expected: &[OpeningRow]) {
        let near = |a: f64, b: f64| (a - b).abs() < 1e-3;
        let found_doors = stats.openings.iter().filter(|o| o.door).count();
        assert_eq!((found_doors, stats.openings.len() - found_doors, stats.openings_skipped), (doors, windows, 0));
        for &(name, wall, length, middle, width, height, sill) in expected {
            let o = stats.openings.iter().find(|o| o.name == name && o.wall == wall && near(o.wall_length, length) && near(o.middle, middle));
            let o = o.unwrap_or_else(|| panic!("{name} in {wall}: {:?}", stats.openings));
            assert!(near(o.width, width) && near(o.height, height) && near(o.sill, sill), "{o:?}");
        }
        let layer = model.layers.iter().find(|l| l.name == "Openings").expect("openings");
        assert!(layer.hidden && layer.elements.len() == doors + windows);
        let walls = model.layers.iter().find(|l| l.name == "Walls").unwrap();
        assert_eq!(walls.solid_indices().len(), walls.indices.len(), "closed walls");
    }

    /// The expected values of the specification (§11) for `CH04_Part2_Walls.pln`.
    #[test]
    fn sample_ch04_walls_archicad_25() {
        let Some((stats, model)) = open("CH04_Part2_Walls.pln") else { return };
        assert_eq!(model.format, "Archicad");
        assert_eq!((stats.walls, stats.walls_skipped, stats.walls_polygon), (20, 0, 0));
        assert_eq!((stats.slabs, stats.slabs_skipped), (3, 0));
        check_stories(&stats, &model);
        let mut slabs = z_ranges(&model, "Slabs");
        slabs.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(slabs.len(), 3);
        for ((bottom, top), expected) in slabs.iter().zip([(-0.2, 0.1), (2.47, 2.85), (2.8, 3.1)]) {
            assert!(near(*bottom, expected.0) && near(*top, expected.1), "slab {slabs:?}");
        }
        let b = walls_and_slabs_bounds(&model);
        assert!(near(b.min[0], 1.011) && near(b.max[0], 11.011) && near(b.min[1], 1.441) && near(b.max[1], 12.441), "{b:?}");
        assert!(near(b.min[2], -0.2) && near(model.bounds.min[2], -0.2), "{b:?}");
        check_roofs(&stats, &[RT_002]);
        check_walls(&stats, &model, 16, 4, 9.0, -0.2);
        // §11 (v0.5): no windows or doors
        assert_eq!((stats.openings.len(), stats.openings_skipped), (0, 0));
        // §11 (v0.9): no stairs or railings
        assert_eq!((stats.stairs.len(), stats.stairs_skipped, stats.railings.len(), stats.railings_skipped), (0, 0, 0, 0));
        // §11 (v0.6): no library objects; (v0.8) no meshes or zones
        assert_eq!((stats.objects.len(), stats.objects_skipped), (0, 0));
        assert_eq!((stats.meshes.len(), stats.meshes_skipped, stats.zones.len(), stats.zones_skipped), (0, 0, 0, 0));
        // §11 (v0.3): no columns or beams
        assert_eq!((stats.columns.len(), stats.columns_skipped, stats.beams.len(), stats.beams_skipped), (0, 0, 0, 0));
        // the walls are on their stories, for the explorer and the exploded view
        assert!(model.props.values().filter(|p| p.contains("\"category\":\"Wall\"")).all(|p| p.contains("\"level\":")));
        // 20 walls, 3 slabs and the roof
        assert_eq!(model.layers.iter().map(|l| l.elements.len()).sum::<usize>(), 24);
    }

    /// The expected values of the specification (§11) for `DEMO.pln`.
    #[test]
    fn sample_demo_archicad_26() {
        let Some((stats, model)) = open("DEMO.pln") else { return };
        assert_eq!((stats.walls, stats.walls_skipped, stats.walls_polygon), (56, 0, 54));
        // §11 (v0.9): 8 slabs, the one with arcs (§8.16.1) too
        assert_eq!((stats.slabs, stats.slabs_skipped), (8, 0));
        check_stories(&stats, &model);
        let b = walls_and_slabs_bounds(&model);
        assert!(near(b.min[0], 1.412) && near(b.max[0], 14.667) && near(b.min[1], -1.0) && near(b.max[1], 12.698), "{b:?}");
        // the lowest slab at -1.089; the model reaches down to the bottom of the terrain (§8.11)
        assert!(near(b.min[2], -1.089) && near(model.bounds.min[2], -1.65), "{b:?}");
        let arcs = stats.slab_list.iter().find(|s| s.guid.starts_with("30081d88")).expect("the slab with arcs");
        let close = |a: f64, b: f64| (a - b).abs() < 1e-3;
        assert!(close(arcs.x.0, 7.427) && close(arcs.x.1, 14.667) && close(arcs.y.0, -1.0) && close(arcs.y.1, 12.373), "{arcs:?}");
        assert!(close(arcs.z.0, -1.089) && close(arcs.z.1, 0.0) && (arcs.area - 52.24).abs() < 0.01, "{arcs:?}");
        check_roofs(
            &stats,
            &[
                ("Roo 2.01", 11.39, 0.271, [1.827, 11.025], [6.108, 12.306], [7.482, 9.611]),
                ("SEO 1.01", 11.393, 0.3, [11.023, 11.219], [5.838, 12.582], [7.35, 7.695]),
            ],
        );
        check_openings(&stats, &model, 6, 7, &[("Di 1.03", "Wai 1.11", 6.6, 5.65, 0.83, 2.115, 0.0), ("We 1.01", "Wae 1.04", 5.303, 2.98, 2.4, 0.95, 1.2)]);
        // §11 (v0.6): 8 library objects, 7 with ZZYZX; the examples of the table
        check_objects(&stats, &model, 8, 7);
        // §11 (v0.8): the mesh and the 8 zones
        assert_eq!((stats.meshes.len(), stats.meshes_skipped), (1, 0));
        let m = &stats.meshes[0];
        let near = |a: f64, b: f64| (a - b).abs() < 1e-3;
        assert_eq!((m.name.as_str(), m.cached, m.vertices, m.triangles, m.outer_edges), ("Ter 0.00", true, 22, 32, 10), "{m:?}");
        assert!(near(m.reference, -0.15) && near(m.thickness, 1.5) && near(m.bottom, -1.65), "{m:?}");
        assert!(near(m.x.0, -1.0) && near(m.x.1, 14.667) && near(m.y.0, -1.0) && near(m.y.1, 15.238), "{m:?}");
        assert!(near(m.top.0, -1.021) && near(m.top.1, -0.15), "{m:?}");
        let terrain = model.layers.iter().find(|l| l.name == "Terrain").expect("terrain");
        assert!(!terrain.hidden && terrain.triangle_count() == 84 && terrain.solid_indices().len() == terrain.indices.len());
        check_zones(&stats, &model, &[
            ("0.01", "Living Room", (0.0, 2.8), (1.829, 7.025), (6.098, 12.296), 32.205),
            ("0.02", "Kitchen", (0.0, 2.8), (1.829, 5.527), (2.100, 6.098), 14.410),
            ("0.03", "WC", (0.0, 2.8), (5.652, 7.025), (4.948, 5.973), 1.407),
            ("0.04", "Storage", (0.0, 2.8), (5.652, 7.025), (2.100, 4.823), 3.739),
            ("1.01", "Bathroom", (3.0, 8.633), (1.829, 5.027), (9.598, 12.296), 8.528),
            ("1.02", "Bedroom", (3.0, 9.105), (1.829, 6.002), (6.098, 9.473), 14.084),
            ("1.03", "Master Bedroom", (3.0, 8.633), (7.150, 11.027), (6.098, 12.296), 24.030),
            ("1.04", "Stair/Hall", (3.0, 8.633), (5.152, 7.025), (6.098, 12.296), 8.196),
        ]);
        // the names as Archicad shows them (§8.10 v0.7): without ".gsm"; the one without ZZYZX
        assert!(stats.objects.iter().all(|o| !o.name.is_empty() && !o.name.ends_with(".gsm")), "{:?}", stats.objects);
        assert!(stats.objects.iter().any(|o| !o.height_known && o.name.starts_with("Shower Cabin")));
        for (a, b, h, z, x, y, shape) in [
            (1.6, 1.381, 0.85, (0.0, 0.85), (3.804, 5.184), (6.567, 8.167), ObjectShape::Turned),
            (3.0, 2.9, 0.8, (0.0, 0.8), (3.045, 4.945), (8.85, 11.85), ObjectShape::Outline),
            (2.352, 2.5, 2.25, (0.0, 2.25), (3.027, 5.527), (3.622, 5.973), ObjectShape::Turned),
            (4.231, 0.1, 0.6, (6.524, 7.124), (11.427, 11.527), (5.991, 10.222), ObjectShape::Turned),
        ] {
            let near = |p: f64, q: f64| (p - q).abs() < 1e-3;
            let pair = |p: (f64, f64), q: (f64, f64)| near(p.0, q.0) && near(p.1, q.1);
            let o = stats.objects.iter().find(|o| near(o.size[0], a) && near(o.size[1], b)).unwrap_or_else(|| panic!("{a} × {b}: {:?}", stats.objects));
            assert!(near(o.size[2], h) && pair(o.z, z) && pair(o.x, x) && pair(o.y, y) && o.shape == shape, "{o:?}");
        }
        check_walls(&stats, &model, 32, 24, 7.588, -0.272);
        // §11 (v0.3): two round columns, linked; the four beams have a profile section
        check_columns(&stats, &[("C 0.01", [11.102, 12.373]), ("C 0.02", [11.102, 6.023])], (-0.25, 2.78));
        // §11 (v0.9): the stair and the two railings
        check_stair(&stats, &model, ("St 0.01", 16, 0.1875, 0.25125), (0.0, 3.0), (0.1875, 3.0), (6.127, 7.027), (7.278, 11.298));
        let near = |a: f64, b: f64| (a - b).abs() < 1e-3;
        let pair = |a: (f64, f64), b: (f64, f64)| near(a.0, b.0) && near(a.1, b.1);
        assert_eq!((stats.railings.len(), stats.railings_skipped, stats.railings_boxed, stats.railing_bad_segments), (2, 0, 0, 0));
        type RailingRow = (&'static str, usize, f64, (f64, f64), (f64, f64), (f64, f64));
        let rows: [RailingRow; 2] = [
            ("89af20b0", 4, 5.740, (6.057, 6.177), (7.278, 11.348), (0.1875, 4.0)),
            ("4923dd38", 3, 4.020, (6.977, 7.027), (7.278, 11.298), (0.0, 4.0)),
        ];
        for (guid, segments, length, x, y, z) in rows {
            let r = stats.railings.iter().find(|r| r.guid.starts_with(guid)).unwrap_or_else(|| panic!("{guid}: {:?}", stats.railings));
            assert!(r.name == "Ra 0.01" && r.segments == segments && near(r.length, length) && near(r.height, 1.0) && !r.boxed, "{r:?}");
            assert!(pair(r.x, x) && pair(r.y, y) && pair(r.z, z), "{r:?}");
        }
        let railings = model.layers.iter().find(|l| l.name == "Railings").expect("railings");
        assert_eq!(railings.solid_indices().len(), railings.indices.len());
        // §11 (v0.8): the four profile beams (gutters) are drawn now
        assert_eq!((stats.beams.len(), stats.beams_skipped, stats.beams_profile, stats.profile_blocks), (4, 0, 0, 0));
        let near = |a: f64, b: f64| (a - b).abs() < 1e-3;
        let pair = |a: (f64, f64), b: (f64, f64)| near(a.0, b.0) && near(a.1, b.1);
        type ProfileRow = (&'static str, u8, usize, usize, [f64; 4], (f64, f64), (f64, f64), (f64, f64), f64);
        let rows: [ProfileRow; 4] = [
            ("RF 2.01", 0, 14, 70, [-0.1, 0.0, 0.412, 0.317], (1.829, 11.027), (5.696, 6.208), (5.386, 7.556), 0.05806),
            ("RF 2.02", 1, 14, 70, [-0.1, 0.0, 0.412, 0.317], (1.829, 11.027), (12.208, 12.72), (5.389, 7.56), 0.05806),
            ("RF 2.03", 0, 13, 65, [-0.098, 0.0, 0.412, 0.3844], (10.929, 11.439), (6.108, 12.308), (7.199, 7.583), 0.06733),
            ("RF 2.04", 0, 14, 70, [-0.412, 0.0, 0.0997, 0.3923], (1.417, 1.929), (6.108, 12.308), (5.339, 5.731), 0.06508),
        ];
        for (name, flag, fills, points, bbox, x, y, z, area) in rows {
            let p = stats.profiles.iter().find(|p| p.name == name).unwrap_or_else(|| panic!("{name}: {:?}", stats.profiles));
            assert!(p.beam && !p.block && p.flag == Some(flag) && p.fills == fills && p.points == points, "{p:?}");
            assert!((0..4).all(|k| near(p.bbox[k], bbox[k])) && pair(p.x, x) && pair(p.y, y) && pair(p.z, z), "{p:?}");
            assert!((p.area - area).abs() < 1e-5, "{p:?}");
        }
        let beams = model.layers.iter().find(|l| l.name == "Beams").expect("beams");
        assert_eq!(beams.solid_indices().len(), beams.indices.len());
        assert_eq!(model.layers.iter().find(|l| l.name == "Columns").map(|l| l.elements.len()), Some(2));
        assert!(model.props.values().any(|p| p.contains("\"Name\":\"") && p.contains("\"category\":\"Wall\"")));
    }

    /// Round columns of diameter 0.2, linked, with these middles, from `z.0` to `z.1` (§11 v0.3).
    fn check_columns(stats: &Stats, expected: &[(&str, [f64; 2])], z: (f64, f64)) {
        let near = |a: f64, b: f64| (a - b).abs() < 1e-3;
        let mut columns = stats.columns.clone();
        columns.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(columns.len(), expected.len(), "{columns:?}");
        assert_eq!(stats.columns_skipped, 0);
        for (c, (name, center)) in columns.iter().zip(expected) {
            assert_eq!(&c.name, name);
            assert!(near(c.center[0], center[0]) && near(c.center[1], center[1]), "{c:?}");
            assert!(matches!(c.section, Section::Round(b, d) if near(b, 0.2) && near(d, 0.2)), "{c:?}");
            assert!(near(c.z.0, z.0) && near(c.z.1, z.1) && c.linked, "{c:?}");
        }
    }

    /// The specification's values (§11 v0.3) for `CH05_Result.pln`: two round columns and four
    /// rectangular beams.
    #[test]
    fn sample_ch05_columns_and_beams_archicad_25() {
        let Some((stats, model)) = open("CH05_Result.pln") else { return };
        check_columns(&stats, &[("CRE - 001", [10.721, 5.731]), ("CRE - 002", [10.721, 12.151])], (0.0, 2.8));
        // the anchor of the first lies 0.1 left of and above its middle
        let first = stats.columns.iter().find(|c| c.name == "CRE - 001").unwrap();
        assert!((first.anchor[0] - 10.621).abs() < 1e-3 && (first.anchor[1] - 5.831).abs() < 1e-3, "{first:?}");
        let near = |a: f64, b: f64| (a - b).abs() < 1e-3;
        let mut beams = stats.beams.clone();
        beams.sort_by(|a, b| a.name.cmp(&b.name));
        let expected = [
            ("BMR - 001", [10.721, 5.731], [6.621, 5.731], 0.2, 0.35, 2.8),
            ("BMR - 002", [10.721, 5.731], [10.721, 12.151], 0.2, 0.35, 2.8),
            ("BMR - 003", [10.721, 12.151], [6.621, 12.151], 0.2, 0.35, 2.8),
            ("BMR - 004", [6.621, 5.730], [1.186, 5.730], 0.227, 0.38, 2.85),
        ];
        assert_eq!((beams.len(), stats.beams_skipped), (4, 0), "{beams:?}");
        for (beam, (name, start, end, b, h, top)) in beams.iter().zip(expected) {
            assert_eq!(beam.name, name);
            assert!(near(beam.start[0], start[0]) && near(beam.start[1], start[1]), "{beam:?}");
            assert!(near(beam.end[0], end[0]) && near(beam.end[1], end[1]), "{beam:?}");
            assert!(matches!(beam.section, Section::Rect(w, d) if near(w, b) && near(d, h)), "{beam:?}");
            assert!(near(beam.top.0, top) && near(beam.top.1, top), "{beam:?}");
        }
        // the beams in the model: from their top down by their height, one element each
        let layer = model.layers.iter().find(|l| l.name == "Beams").expect("beams");
        assert_eq!(layer.elements.len(), 4);
        let z: Vec<f32> = layer.positions.chunks_exact(3).map(|p| p[2]).collect();
        let (low, high) = z.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
        assert!((low - 2.45).abs() < 1e-3 && (high - 2.85).abs() < 1e-3, "{low} {high}");
        check_roofs(&stats, &[RT_002]);
        check_openings(
            &stats,
            &model,
            6,
            8,
            &[
                ("DOO - 001", "SW - 002", 11.0, 4.712, 1.0, 2.1, 0.0),
                ("WD - 004", "SW - 003", 7.0, 5.74, 2.2, 1.2, 0.9),
                ("WD - 003", "SW - 004", 10.0, 4.84, 0.9, 2.26, -0.2),
            ],
        );
        // §11 (v0.6): 26 library objects, 17 with ZZYZX
        check_objects(&stats, &model, 26, 17);
        // §11 (v0.9): the stair, no railings
        check_stair(&stats, &model, ("STAIR - 002", 17, 0.17647, 0.25), (0.1, 3.1), (0.2765, 3.1), (5.811, 6.609), (6.666, 10.916));
        assert_eq!(stats.railings.len(), 0);
        // §11 (v0.8): no meshes; 8 zones
        assert_eq!(stats.meshes.len(), 0);
        check_zones(&stats, &model, &[
            ("01", "Living Room", (0.0, 2.8), (1.413, 6.609), (5.666, 12.039), 33.114),
            ("02", "Toilet", (0.0, 2.8), (5.361, 6.609), (4.541, 5.541), 1.248),
            ("03", "Storage", (0.0, 2.8), (5.361, 6.609), (1.843, 4.416), 3.211),
            ("04", "Kitchen", (0.0, 2.8), (1.413, 5.236), (1.843, 5.666), 14.340),
            ("05", "Bedroom", (3.0, 5.8), (1.413, 5.711), (5.843, 9.141), 14.175),
            ("06", "Master Bedroom", (3.0, 5.8), (6.836, 10.609), (5.843, 12.039), 23.378),
            ("07", "Hallway", (3.0, 5.8), (4.686, 6.711), (5.843, 12.039), 8.724),
            ("08", "Bathroom", (3.0, 5.8), (1.413, 4.586), (9.241, 12.039), 8.796),
        ]);
        let roofs = model.layers.iter().find(|l| l.name == "Roofs").expect("roofs");
        assert_eq!(roofs.solid_indices().len(), roofs.indices.len(), "a closed roof body");
    }

    #[test]
    fn sample_browser_bytes_give_the_same_model() {
        let Some(path) = sample("CH04_Part2_Walls.pln") else { return };
        let source = Source::Bytes { name: "CH04.PLN".into(), data: Arc::new(std::fs::read(&path).unwrap()) };
        let bytes = crate::loaders::load_with(&source, &Progress::default()).expect("bytes");
        let file = crate::loaders::load(&path).unwrap();
        assert_eq!(bytes.triangle_count(), file.triangle_count());
        assert_eq!(bytes.bounds, file.bounds);
    }

    #[test]
    fn rejects_other_files_and_old_versions() {
        let load = |data: &[u8]| ArchicadLoader.load(&Source::Bytes { name: "x.pln".into(), data: Arc::new(data.to_vec()) }).unwrap_err();
        assert!(load(b"not an Archicad project at all, but long enough").contains("no ROF FDB signature"));
        assert!(load(b"MM\0\0 an Archicad 10 project of more than 32 bytes").contains("before Archicad 17"));
        // the container with nothing in it: no elements, as an Archicad 17-23 file gives
        let mut empty = b"ROF FDB ".to_vec();
        empty.resize(64, 0);
        let message = load(&empty);
        assert!(message.contains("supports Archicad 24 to 29") && message.contains("Archicad 17 to 23 are not supported"), "{message}");
        // an item that is not a block, free space or a group
        empty[48] = 1;
        assert!(load(&empty).contains("unknown item at offset 48"));
    }

    #[test]
    fn zero_byte_mask() {
        // mask 0b0000_0101: byte, zero, byte, zeros; then 0b1: byte
        assert_eq!(unmask(&[0x05, 7, 9, 0x01, 3], 9), Some(vec![7, 0, 9, 0, 0, 0, 0, 0, 3]));
        assert_eq!(unmask(&[0x05, 7, 9, 0x01, 3, 99], 9), None, "input left over");
        assert_eq!(unmask(&[0xFF, 1], 2), None, "input too short");
        assert_eq!(unmask(&[0xFF, 1], 1 << 30), None, "more output than the input can give");
    }

    const STORY_ID: Guid = guid("00112233445566778899aabbccddeeff");

    /// An object record (§4): class, data and associations.
    fn record(class: Guid, data: &[u8], assoc: &[u8]) -> Vec<u8> {
        let mut p = class.to_vec();
        p.resize(35, 0);
        p.extend((data.len() as u32).to_le_bytes());
        p.extend(data);
        p.extend((assoc.len() as u32).to_le_bytes());
        p.extend(assoc);
        p
    }

    /// A block (§3) around an encoded payload; `size` is the decoded length (0x85, 0x86).
    fn block(object: Guid, encoding: u8, payload: &[u8], size: usize) -> Vec<u8> {
        let mut b = b"BL".to_vec();
        b.extend([0; 4]);
        b.extend([0, 0, 1, 0xA1, encoding]);
        b.extend(object);
        b.extend((payload.len() as u16).to_le_bytes());
        if encoding != 0x84 {
            b.extend((size as u32).to_le_bytes());
        }
        b.extend([0; 4]);
        b.extend(payload);
        b.resize(b.len().div_ceil(16) * 16, 0);
        let units = (b.len() / 16) as u32;
        b[2..6].copy_from_slice(&units.to_le_bytes());
        b
    }

    /// §3.1 the other way round: a mask byte per 8 bytes, then the bytes that are not zero.
    fn mask(p: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for group in p.chunks(8) {
            out.push(group.iter().enumerate().fold(0u8, |m, (i, &b)| m | (u8::from(b != 0) << i)));
            out.extend(group.iter().filter(|&&b| b != 0));
        }
        out
    }

    /// One association item `op 0x12` (§4.1) and the end of the list.
    fn link(assoc: Guid, target: Guid) -> Vec<u8> {
        [&[0u8, 0, 0x12][..], &assoc, &target, &[0]].concat()
    }

    /// Polygon data (§7): a 4 x 2 rectangle with a 1 x 1 hole behind 5 bytes of other data, and its
    /// z cache from -0.2 to 0.1.
    fn polygon_data() -> Vec<u8> {
        let outer: [[f64; 2]; 5] = [[0.0, 0.0], [4.0, 0.0], [4.0, 2.0], [0.0, 2.0], [0.0, 0.0]];
        let hole: [[f64; 2]; 5] = [[1.0, 0.5], [1.0, 1.5], [2.0, 1.5], [2.0, 0.5], [1.0, 0.5]];
        let mut data = vec![1u8; 5];
        let bbox: Vec<u8> = [0.0f64, 0.0, 4.0, 2.0].iter().flat_map(|v| v.to_le_bytes()).collect();
        data.extend(&bbox);
        data.extend([10u32, 0, 2].iter().flat_map(|v| v.to_le_bytes()));
        for p in [[0.0f64, 0.0]].iter().chain(&outer).chain(&hole) {
            data.extend(p[0].to_le_bytes());
            data.extend(p[1].to_le_bytes());
        }
        data.extend([0u32, 5, 10].iter().flat_map(|v| v.to_le_bytes()));
        data.extend([3u8; 7]);
        data.extend(&bbox);
        data.extend([-0.2f64, 0.1].iter().flat_map(|v| v.to_le_bytes()));
        data
    }

    /// Wall data (§8.2): a 4 m reference line from (1, y), 0.3 m thick, height B, no polygon.
    fn wall_data(id: &str, y: f64, height: f64) -> Vec<u8> {
        let mut data = vec![0u8; 38];
        data.extend([1, 0]);
        data.extend((id.len() as u32).to_le_bytes());
        data.extend(id.encode_utf16().flat_map(u16::to_le_bytes));
        let q = data.len();
        data.extend([1.0f64, y, 5.0, y].iter().flat_map(|v| v.to_le_bytes()));
        data.extend([1i32; 6].iter().flat_map(|v| v.to_le_bytes()));
        data.resize(q + 169, 0);
        for (at, v) in [(77, 0.3f64), (85, 0.3), (93, 0.0), (145, 0.0), (153, height), (161, 0.0)] {
            data[q + at..q + at + 8].copy_from_slice(&v.to_le_bytes());
        }
        data
    }

    /// Several association items (§4.1) and the end of the list.
    fn links_of(items: &[(u8, Guid, Guid)]) -> Vec<u8> {
        let mut out = vec![0, 0];
        for (op, assoc, target) in items {
            out.push(*op);
            out.extend(assoc);
            out.extend(target);
        }
        out.push(0);
        out
    }

    const LINKED_TO_TOP: Guid = guid("0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a");
    const LINKED_TO_OTHER: Guid = guid("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b");

    /// A whole project: a wall W1 (0x85) in the outer layer, then a group in two chunks with a
    /// story ("Hall", index 2, 0x84), free space, its level (4.5 m, 0x86, across the chunk
    /// boundary), the level of the story above the top story (7.5 m), a slab, a wall W2 and a wall
    /// W3 (0x84), and two link objects (§8.3). W1's top is linked to the story above the top story,
    /// W2's link object points at something that is not a story, W3 has no link and no height.
    fn synthetic_file() -> Vec<u8> {
        let mut story = vec![0u8; 10];
        story.extend(4u32.to_le_bytes());
        story.extend("Hall".encode_utf16().flat_map(u16::to_le_bytes));
        story.extend(2i16.to_le_bytes());
        let other = guid("ffeeddccbbaa99887766554433221100");
        let level = record(STORY_LEVEL, &[[0; 8], 4.5f64.to_le_bytes()].concat(), &link(other, STORY_ID));
        let packed = quicklz::compress(&level, quicklz::CompressionLevel::Lvl1);
        let mut inner = block(STORY_ID, 0x84, &record(STORY, &story, &[0, 0, 0]), 0);
        inner.extend([0; 16]);
        inner.extend(block(guid("0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f"), 0x86, &packed, level.len()));
        inner.extend(block(guid("0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e0e"), 0x84, &record(SLAB, &polygon_data(), &link(ON_STORY, STORY_ID)), 0));
        let top_level = record(STORY_LEVEL, &7.5f64.to_le_bytes(), &link(other, TOP_STORY));
        inner.extend(block(guid("0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c"), 0x84, &top_level, 0));
        let linked_wall = |id: &str, y: f64, height: f64, link: Option<Guid>| {
            let mut items = vec![(0x12, ON_STORY, STORY_ID)];
            items.extend(link.map(|l| (0x11, TOP_LINK, l)));
            record(WALL, &wall_data(id, y, height), &links_of(&items))
        };
        inner.extend(block(guid("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b01"), 0x84, &linked_wall("W2", 3.0, 2.7, Some(LINKED_TO_OTHER)), 0));
        inner.extend(block(guid("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b02"), 0x84, &linked_wall("W3", 5.0, 0.0, None), 0));
        for (object, target) in [(LINKED_TO_TOP, TOP_STORY), (LINKED_TO_OTHER, other)] {
            inner.extend(block(object, 0x84, &record(LINK, &[0; 16], &links_of(&[(0x11, LINK_STORY, target)])), 0));
        }
        let mut file = b"ROF FDB ".to_vec();
        file.resize(32, 0);
        let wall = linked_wall("W1", 1.0, 2.7, Some(LINKED_TO_TOP));
        file.extend(block(guid("0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d"), 0x85, &mask(&wall), wall.len()));
        file.extend(GROUP);
        file.extend((inner.len() as u64).to_le_bytes());
        file.extend(2u64.to_le_bytes());
        for part in [&inner[..140], &inner[140..]] {
            let chunk = quicklz::compress(part, quicklz::CompressionLevel::Lvl1);
            file.extend((chunk.len() as u32).to_le_bytes());
            file.extend(chunk);
        }
        let pad = (16 - (file.len() + 2) % 16) % 16;
        file.extend((pad as u16).to_le_bytes());
        file.resize(file.len() + pad, 0);
        file.extend([0; 16]);
        file
    }

    /// All passes of the reader (§8.10), as for a file the browser hands over.
    fn read_bytes(file: &[u8]) -> Result<Records, String> {
        read_source(&Source::Bytes { name: "x.pln".into(), data: Arc::new(file.to_vec()) })
    }

    #[test]
    fn reads_a_synthetic_project() {
        let records = read_bytes(&synthetic_file()).expect("container");
        assert_eq!(records.stories.get(&STORY_ID), Some(&("Hall".to_string(), 2)));
        assert_eq!(records.levels, [(vec![STORY_ID], 4.5), (vec![TOP_STORY], 7.5)]);
        assert_eq!((records.walls.len(), records.slabs.len(), records.top_links.len(), records.damaged), (3, 1, 2, 0));
        let (loaded, stats) = convert(records).expect("elements");
        assert_eq!((stats.walls, stats.walls_skipped, stats.slabs), (2, 1, 1));
        // W1 linked; W2's link object points at no story, so B counts; W3 has no height
        assert_eq!((stats.walls_linked, stats.walls_unlinked, stats.walls_no_height), (1, 1, 1));
        assert_eq!(loaded.project.levels, [("Hall".to_string(), 4.5)]);
        let walls = &loaded.layers[0];
        let range = |element: usize, axis: usize| {
            let (_, first, count) = walls.elements[element];
            walls.indices[first as usize..(first + count) as usize]
                .iter()
                .map(|&i| walls.positions[3 * i as usize + axis])
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)))
        };
        // W1, left of its reference line, from its story at 4.5 m up to the linked level 7.5 m + A
        assert_eq!((range(0, 0), range(0, 1), range(0, 2)), ((1.0, 5.0), (1.0, 1.3), (4.5, 7.5)));
        assert!(walls.props[0].1.contains("\"Name\":\"W1\"") && walls.props[0].1.contains("\"level\":\"Hall\""));
        assert!(walls.props[0].1.contains("\"Top\":\"linked to the story above the top story\""));
        // W2: 2.7 m from its bottom
        assert_eq!((range(1, 1), range(1, 2)), ((3.0, 3.3), (4.5, 7.2)));
        assert!(walls.props[1].1.contains("\"Name\":\"W2\"") && walls.props[1].1.contains("\"Top\":\"not linked\""));
        assert_eq!(loaded.layers[1].triangle_count(), 2 * 8 + 8 * 2);
    }

    /// Every prefix of a valid file, random bytes behind a valid signature and random changes to a
    /// valid file: an answer (model or error) every time, quickly, without a panic.
    #[test]
    fn truncated_or_garbage_input_does_not_panic() {
        for file in [synthetic_file(), columns_and_beams_file(), roofs_file(), openings_file(), objects_file(), meshes_and_zones_file(), profiles_file(), arcs_file(), morphs_file(), stairs_file()] {
            garbage_rounds(&file);
        }
    }

    fn garbage_rounds(file: &[u8]) {
        for cut in 0..file.len() {
            let _ = read_bytes(&file[..cut]).and_then(convert);
        }
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for round in 0..300 {
            let mut garbage = b"ROF FDB ".to_vec();
            garbage.resize(32 + 16 * (round % 64), 0);
            for b in &mut garbage[32..] {
                *b = next() as u8;
            }
            // the items start like real ones now and then
            if round % 3 == 0 && garbage.len() > 48 {
                garbage[32..34].copy_from_slice(b"BL");
            } else if round % 3 == 1 && garbage.len() > 48 {
                garbage[32..48].copy_from_slice(&GROUP);
            }
            let _ = read_bytes(&garbage).and_then(convert);
            let mut changed = file.to_vec();
            for _ in 0..(1 + round % 20) {
                let at = 32 + (next() as usize) % (file.len() - 32);
                changed[at] = next() as u8;
            }
            let _ = read_bytes(&changed).and_then(convert);
        }
    }

    /// Lengths in the file are never trusted for an allocation: a group that claims 2^64 bytes with
    /// a block of 64 GiB in it, and an outer block longer than the file.
    #[test]
    fn huge_lengths_are_skipped_not_allocated() {
        let mut huge = b"BL".to_vec();
        huge.extend(u32::MAX.to_le_bytes());
        huge.extend([0, 0, 1, 0xA1, 0x86]);
        huge.resize(64, 0x5A);
        let mut file = b"ROF FDB ".to_vec();
        file.resize(32, 0);
        file.extend(GROUP);
        file.extend(u64::MAX.to_le_bytes());
        file.extend(3u64.to_le_bytes());
        for part in [&huge[..], &[0x5A; 4096][..], &[0x5A; 4096][..]] {
            let chunk = quicklz::compress(part, quicklz::CompressionLevel::Lvl1);
            file.extend((chunk.len() as u32).to_le_bytes());
            file.extend(chunk);
        }
        let pad = (16 - (file.len() + 2) % 16) % 16;
        file.extend((pad as u16).to_le_bytes());
        file.resize(file.len() + pad, 0);
        let records = read_bytes(&file).expect("the group is read past");
        assert_eq!(records.damaged, 1);
        // an outer block of 64 GiB in a file of 64 bytes
        let mut short = b"ROF FDB ".to_vec();
        short.resize(32, 0);
        short.extend(&huge[..32]);
        assert!(read_bytes(&short).err().unwrap().contains("past the end of the file"));
    }

    #[test]
    fn finds_polygon_and_z_cache() {
        let polygon = find_polygon(&polygon_data()).expect("polygon");
        assert_eq!(polygon.coords, 10);
        let rectangle = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 2.0], [0.0, 2.0]];
        let hole = vec![[1.0, 0.5], [1.0, 1.5], [2.0, 1.5], [2.0, 0.5]];
        assert_eq!(polygon.contours, [rectangle, hole]);
        assert_eq!(polygon.z, Some((-0.2, 0.1)));
        let mut layer = Layer::default();
        assert!(push_prism(&mut layer, &Prism { contours: polygon.contours, z: (-0.2, 0.1) }));
        // caps of 8 triangles each, 8 side faces of 2
        assert_eq!(layer.triangle_count(), 2 * 8 + 8 * 2);
        assert!(!crate::model::Model::new("x.pln".into(), "Archicad", vec![layer]).layers[0].solid_indices().is_empty());
    }

    /// Data that passes the cheap checks of the polygon search at every offset (2, 2, 2: two points,
    /// two arcs, two contours) must not make it read 20 000 contour ends per offset.
    #[test]
    fn polygon_search_stays_fast() {
        let start = web_time::Instant::now();
        let data: Vec<u8> = std::iter::repeat_n(2u32.to_le_bytes(), MAX_ELEMENT_DATA / 4).flatten().collect();
        assert!(find_polygon(&data).is_none());
        let mut larger = data.clone();
        larger.push(0);
        assert!(find_polygon(&larger).is_none());
        assert!(start.elapsed().as_secs() < 20, "{:?}", start.elapsed());
    }

    /// Column data (§8.5): the axis at 0 (anchor, bottom offset, anchor, 3 m) and the middle of
    /// the section, the ID string at 100, then k = 0, A, H, C, 0, the anchor, the slope (vertical)
    /// and the rotation.
    fn column_data(id: &str, center: [f64; 2], anchor: [f64; 2], rotation: f64, [a, h, c]: [f64; 3]) -> Vec<u8> {
        let mut data: Vec<u8> = [anchor[0], anchor[1], c, anchor[0], anchor[1], 3.0, center[0], center[1]].iter().flat_map(|v| v.to_le_bytes()).collect();
        data.resize(98, 0);
        data.extend([1, 0]);
        data.extend((id.len() as u32).to_le_bytes());
        data.extend(id.encode_utf16().flat_map(u16::to_le_bytes));
        let q = data.len();
        data.extend(0u32.to_le_bytes());
        data.extend([a, h, c, 0.0, anchor[0], anchor[1]].iter().flat_map(|v| v.to_le_bytes()));
        data.resize(q + 95, 0);
        data[q + 79..q + 87].copy_from_slice(&std::f64::consts::FRAC_PI_2.to_le_bytes());
        data[q + 87..q + 95].copy_from_slice(&rotation.to_le_bytes());
        data
    }

    /// Beam data (§8.6): the axis with its heights at 0 if given, the ID string at 100, then
    /// k = 0, s, zr and the axis.
    fn beam_data(id: &str, offset: f64, zr: f64, line: [f64; 4], heights: Option<(f64, f64)>) -> Vec<u8> {
        let mut data = Vec::new();
        if let Some((z1, z2)) = heights {
            data.extend([line[0], line[1], z1, line[2], line[3], z2].iter().flat_map(|v| v.to_le_bytes()));
        }
        data.resize(98, 0);
        data.extend([1, 0]);
        data.extend((id.len() as u32).to_le_bytes());
        data.extend(id.encode_utf16().flat_map(u16::to_le_bytes));
        data.push(0);
        data.extend([offset, zr, line[0], line[1], line[2], line[3]].iter().flat_map(|v| v.to_le_bytes()));
        data
    }

    /// Section data (§8.7): b and d at 40 (0x82) or 52 (0x83).
    fn section_data(kind: u8, b: f64, d: f64) -> Vec<u8> {
        let mut data = vec![kind];
        data.resize(if kind == 0x82 { 40 } else { 52 }, 0);
        data.extend(b.to_le_bytes());
        data.extend(d.to_le_bytes());
        data
    }

    /// A project of columns and beams on the story "Hall" (4.5 m), all 0x84 blocks in the outer
    /// layer:
    /// - K1: rectangular 0.4 × 0.2, turned 30°, middle (3, 3), anchor (2.9, 3.1), top linked to the
    ///   story above the top story (7.5 m) with A = -0.3 (its H of 1 m is out of date);
    /// - K2: round ⌀ 0.3, middle (6, 3), not linked: from C = 0.1 up H = 2;
    /// - B1: 0.2 × 0.4 from (1, 6) to (5, 6), s = 0.1, its axis from 0.5 to 1.0 above the story;
    /// - B2: the same section from (1, 8) to (1, 10), no axis: horizontal at zr = 0.8;
    /// - B3: a profile section, B4: no segment.
    fn columns_and_beams_file() -> Vec<u8> {
        let id = |k: u8| [k; 16];
        let mut story = vec![0u8; 10];
        story.extend(4u32.to_le_bytes());
        story.extend("Hall".encode_utf16().flat_map(u16::to_le_bytes));
        story.extend(2i16.to_le_bytes());
        let other = guid("ffeeddccbbaa99887766554433221100");
        let mut blocks = vec![
            block(STORY_ID, 0x84, &record(STORY, &story, &[0, 0, 0]), 0),
            block(id(0x21), 0x84, &record(STORY_LEVEL, &4.5f64.to_le_bytes(), &link(other, STORY_ID)), 0),
            block(id(0x22), 0x84, &record(STORY_LEVEL, &7.5f64.to_le_bytes(), &link(other, TOP_STORY)), 0),
            block(LINKED_TO_TOP, 0x84, &record(LINK, &[0; 16], &links_of(&[(0x12, LINK_STORY, TOP_STORY)])), 0),
        ];
        // sections S1 (K1), S2 (K2), S3 (B1, B2), S4 (B3, a profile); a segment per element
        for (section, class, data) in [
            (id(0x31), SECTION_RECT, section_data(0x82, 0.4, 0.2)),
            (id(0x32), SECTION_ROUND, section_data(0x83, 0.3, 0.3)),
            (id(0x33), SECTION_RECT, section_data(0x83, 0.2, 0.4)),
            (id(0x34), SECTION_PROFILE, vec![0; 8]),
        ] {
            blocks.push(block(section, 0x84, &record(class, &data, &[0, 0, 0]), 0));
        }
        for (segment, class, section) in [
            (id(0x41), COLUMN_SEGMENT, id(0x31)),
            (id(0x42), COLUMN_SEGMENT, id(0x32)),
            (id(0x43), BEAM_SEGMENT, id(0x33)),
            (id(0x44), BEAM_SEGMENT, id(0x33)),
            (id(0x45), BEAM_SEGMENT, id(0x34)),
        ] {
            blocks.push(block(segment, 0x84, &record(class, &[0; 8], &links_of(&[(0x12, SECTION_LINK, section)])), 0));
        }
        let element = |class: Guid, data: Vec<u8>, segment: Option<Guid>, top: Option<Guid>| {
            let mut items = vec![(0x12, ON_STORY, STORY_ID)];
            let segment_link = if class == COLUMN { COLUMN_SEGMENT_LINK } else { BEAM_SEGMENT_LINK };
            items.extend(segment.map(|s| (0x12, segment_link, s)));
            items.extend(top.map(|t| (0x12, COLUMN_TOP_LINK, t)));
            record(class, &data, &links_of(&items))
        };
        let k1 = column_data("K1", [3.0, 3.0], [2.9, 3.1], std::f64::consts::FRAC_PI_6, [-0.3, 1.0, 0.0]);
        let k2 = column_data("K2", [6.0, 3.0], [6.0, 3.0], 0.0, [0.0, 2.0, 0.1]);
        blocks.push(block(id(0x51), 0x84, &element(COLUMN, k1, Some(id(0x41)), Some(LINKED_TO_TOP)), 0));
        blocks.push(block(id(0x52), 0x84, &element(COLUMN, k2, Some(id(0x42)), None), 0));
        let b1 = beam_data("B1", 0.1, 0.5, [1.0, 6.0, 5.0, 6.0], Some((0.5, 1.0)));
        let b2 = beam_data("B2", 0.0, 0.8, [1.0, 8.0, 1.0, 10.0], None);
        let b3 = beam_data("B3", 0.0, 0.8, [1.0, 11.0, 4.0, 11.0], None);
        let b4 = beam_data("B4", 0.0, 0.8, [1.0, 12.0, 4.0, 12.0], None);
        for (object, data, segment) in [(id(0x61), b1, Some(id(0x43))), (id(0x62), b2, Some(id(0x44))), (id(0x63), b3, Some(id(0x45))), (id(0x64), b4, None)] {
            blocks.push(block(object, 0x84, &element(BEAM, data, segment, None), 0));
        }
        let mut file = b"ROF FDB ".to_vec();
        file.resize(32, 0);
        file.extend(blocks.concat());
        file
    }

    /// The corners (x, y) and the bottom and top of an element of a layer.
    fn element_shape(layer: &Layer, element: usize) -> (Vec<[f32; 2]>, (f32, f32)) {
        let (_, first, count) = layer.elements[element];
        let mut xy: Vec<[f32; 2]> = Vec::new();
        let mut z = (f32::INFINITY, f32::NEG_INFINITY);
        for &i in &layer.indices[first as usize..(first + count) as usize] {
            let p = &layer.positions[3 * i as usize..3 * i as usize + 3];
            if !xy.iter().any(|q| (q[0] - p[0]).abs() < 1e-5 && (q[1] - p[1]).abs() < 1e-5) {
                xy.push([p[0], p[1]]);
            }
            z = (z.0.min(p[2]), z.1.max(p[2]));
        }
        (xy, z)
    }

    /// §8.6 (v0.4): of two candidates that both meet the conditions, the one whose axis is found
    /// counts; without any axis, the first.
    #[test]
    fn beam_candidate_with_axis_first() {
        let data = |axis: bool| {
            let mut data = Vec::new();
            if axis {
                data.extend([1.0f64, 6.0, 0.5, 5.0, 6.0, 1.0].iter().flat_map(|v| v.to_le_bytes()));
            }
            for (at, id, values) in [(100, "X", [0.0f64, 0.5, 7.0, 7.0, 9.0, 7.0]), (200, "B5", [0.1, 0.5, 1.0, 6.0, 5.0, 6.0])] {
                data.resize(at - 2, 0);
                data.extend([1, 0]);
                data.extend((id.len() as u32).to_le_bytes());
                data.extend(id.encode_utf16().flat_map(u16::to_le_bytes));
                data.push(0);
                data.extend(values.iter().flat_map(|v| v.to_le_bytes()));
            }
            data
        };
        let with = beam_fields(&data(true)).expect("beam");
        assert_eq!((with.id.as_str(), with.top, with.offset), ("B5", (0.5, 1.0), 0.1));
        let without = beam_fields(&data(false)).expect("beam");
        assert_eq!((without.id.as_str(), without.top), ("X", (0.5, 0.5)));
    }

    /// Polygon data (§7) of these contours (outer first), each closed, without arcs or z cache.
    fn polygon_bytes(contours: &[Vec<[f64; 2]>]) -> Vec<u8> {
        let all = contours.iter().flatten();
        let (lo, hi) = all.fold(([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]));
        let coords: usize = contours.iter().map(|c| c.len() + 1).sum();
        let mut data: Vec<u8> = [lo[0], lo[1], hi[0], hi[1]].iter().flat_map(|v| v.to_le_bytes()).collect();
        data.extend([coords as u32, 0, contours.len() as u32].iter().flat_map(|v| v.to_le_bytes()));
        let points = std::iter::once([0.0, 0.0]).chain(contours.iter().flat_map(|c| c.iter().copied().chain([c[0]])));
        data.extend(points.flat_map(|p| [p[0], p[1]]).flat_map(f64::to_le_bytes));
        let mut end = 0u32;
        data.extend(0u32.to_le_bytes());
        for c in contours {
            end += c.len() as u32 + 1;
            data.extend(end.to_le_bytes());
        }
        data
    }

    /// Roof data (§8.8): the ID string at 40 with t at q+1, then hp, α and the pivot line before
    /// the polygon at `p`.
    fn roof_data(id: &str, p: usize, [hp, pitch, thickness]: [f64; 3], line: [f64; 4], contours: &[Vec<[f64; 2]>]) -> Vec<u8> {
        let mut data = vec![0u8; 38];
        data.extend([1, 0]);
        data.extend((id.len() as u32).to_le_bytes());
        data.extend(id.encode_utf16().flat_map(u16::to_le_bytes));
        data.push(0);
        data.extend(thickness.to_le_bytes());
        data.resize(p - 62, 0);
        data.extend(hp.to_le_bytes());
        data.push(0);
        data.extend(pitch.to_le_bytes());
        data.extend(line.iter().flat_map(|v| v.to_le_bytes()));
        data.resize(p, 0);
        data.extend(polygon_bytes(contours));
        data
    }

    /// A rectangle from (x0, y0) to (x1, y1), counter-clockwise.
    fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<[f64; 2]> {
        vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
    }

    /// Roofs on the story "Hall" (4.5 m), 0x84 blocks in the outer layer:
    /// - R1: 30°, hp 0.2, t 0.25, pivot line (0, 0)-(4, 0), the plane to its left (y 0 to 3):
    ///   rising to the left of the line;
    /// - R2: the same to its right (y −3 to 0): rising to the right;
    /// - R3: flat, t 0.3, no pivot line;
    /// - R4: 20°, a 4 × 4 plane with a 1 × 1 hole;
    /// - R5: no polygon; R6: its polygon too near the start (p < 300); R7: thickness 0.
    fn roofs_file() -> Vec<u8> {
        let mut story = vec![0u8; 10];
        story.extend(4u32.to_le_bytes());
        story.extend("Hall".encode_utf16().flat_map(u16::to_le_bytes));
        story.extend(2i16.to_le_bytes());
        let other = guid("ffeeddccbbaa99887766554433221100");
        let mut blocks = vec![
            block(STORY_ID, 0x84, &record(STORY, &story, &[0, 0, 0]), 0),
            block([0x21; 16], 0x84, &record(STORY_LEVEL, &4.5f64.to_le_bytes(), &link(other, STORY_ID)), 0),
        ];
        let line = [0.0, 0.0, 4.0, 0.0];
        let roofs = [
            roof_data("R1", 320, [0.2, 30.0, 0.25], line, &[rectangle(0.0, 0.0, 4.0, 3.0)]),
            roof_data("R2", 320, [0.2, 30.0, 0.25], line, &[rectangle(0.0, -3.0, 4.0, 0.0)]),
            roof_data("R3", 320, [0.2, 0.0, 0.3], [0.0; 4], &[rectangle(10.0, 0.0, 12.0, 2.0)]),
            roof_data("R4", 320, [0.0, 20.0, 0.2], [20.0, 0.0, 24.0, 0.0], &[rectangle(20.0, 0.0, 24.0, 4.0), rectangle(21.0, 1.0, 22.0, 2.0)]),
            roof_data("R5", 320, [0.0, 20.0, 0.2], line, &[]).into_iter().take(320).collect(),
            roof_data("R6", 290, [0.0, 20.0, 0.2], line, &[rectangle(0.0, 0.0, 1.0, 1.0)]),
            roof_data("R7", 320, [0.0, 20.0, 0.0], line, &[rectangle(0.0, 0.0, 1.0, 1.0)]),
        ];
        for (k, data) in roofs.iter().enumerate() {
            blocks.push(block([0x70 + k as u8; 16], 0x84, &record(ROOF, data, &link(ON_STORY, STORY_ID)), 0));
        }
        let mut file = b"ROF FDB ".to_vec();
        file.resize(32, 0);
        file.extend(blocks.concat());
        file
    }

    /// Window or door data (§8.9): b and h at 16 and 24, the ID string at 100, m and s 24 and 64
    /// bytes after its end.
    fn opening_data(id: &str, [middle, width, height, sill]: [f64; 4]) -> Vec<u8> {
        let mut data = vec![0u8; 16];
        data.extend([width, height].iter().flat_map(|v| v.to_le_bytes()));
        data.resize(98, 0);
        data.extend([1, 0]);
        data.extend((id.len() as u32).to_le_bytes());
        data.extend(id.encode_utf16().flat_map(u16::to_le_bytes));
        let q = data.len();
        data.resize(q + 72, 0);
        data[q + 24..q + 32].copy_from_slice(&middle.to_le_bytes());
        data[q + 64..q + 72].copy_from_slice(&sill.to_le_bytes());
        data
    }

    /// Walls on the story "Hall" (4.5 m), each 4 m from (1, y) to (5, y), 0.3 thick to the left
    /// of that line, 2.7 high, with their windows and doors (middle, width, height, sill):
    /// - W1 (y 0): a door D1 (1, 0.8, 2.1, 0);
    /// - W2 (y 2): a window N1 (2, 1, 1.2, 0.9);
    /// - W3 (y 4): a door D2 (1, 0.8, 2.1, 0) and a window N2 (3, 1, 1, 1);
    /// - W4 (y 6): a window N3 (3.8, 1, 1, 1) that reaches 0.3 past the end of the wall, one
    ///   whose width is 0 and one whose record is missing.
    fn openings_file() -> Vec<u8> {
        let mut story = vec![0u8; 10];
        story.extend(4u32.to_le_bytes());
        story.extend("Hall".encode_utf16().flat_map(u16::to_le_bytes));
        story.extend(2i16.to_le_bytes());
        let other = guid("ffeeddccbbaa99887766554433221100");
        let mut blocks = vec![
            block(STORY_ID, 0x84, &record(STORY, &story, &[0, 0, 0]), 0),
            block([0x21; 16], 0x84, &record(STORY_LEVEL, &4.5f64.to_le_bytes(), &link(other, STORY_ID)), 0),
        ];
        let openings: [(u8, bool, &str, [f64; 4]); 6] = [
            (0x81, true, "D1", [1.0, 0.8, 2.1, 0.0]),
            (0x82, false, "N1", [2.0, 1.0, 1.2, 0.9]),
            (0x83, true, "D2", [1.0, 0.8, 2.1, 0.0]),
            (0x84, false, "N2", [3.0, 1.0, 1.0, 1.0]),
            (0x85, false, "N3", [3.8, 1.0, 1.0, 1.0]),
            (0x86, false, "N4", [2.0, 0.0, 1.0, 1.0]),
        ];
        for (k, door, id, values) in openings {
            blocks.push(block([k; 16], 0x84, &record(if door { DOOR } else { WINDOW }, &opening_data(id, values), &[0, 0, 0]), 0));
        }
        let walls: [(&str, f64, &[u8]); 4] = [("W1", 0.0, &[0x81]), ("W2", 2.0, &[0x82]), ("W3", 4.0, &[0x83, 0x84]), ("W4", 6.0, &[0x85, 0x86, 0x87])];
        for (k, (id, y, openings)) in walls.into_iter().enumerate() {
            let mut items = vec![(0x12, ON_STORY, STORY_ID)];
            let doors: Vec<Guid> = openings.iter().filter(|&&o| o == 0x81 || o == 0x83).map(|&o| [o; 16]).collect();
            let windows: Vec<Guid> = openings.iter().filter(|&&o| o != 0x81 && o != 0x83).map(|&o| [o; 16]).collect();
            items.extend(doors.iter().map(|&d| (0x11, WALL_DOORS, d)));
            let mut assoc = links_of(&items);
            // the windows of W3 and W4 as one 0x01 item with several targets
            assoc.pop();
            if !windows.is_empty() {
                assoc.push(0x01);
                assoc.extend(WALL_WINDOWS);
                assoc.extend((windows.len() as u32).to_le_bytes());
                assoc.extend(windows.concat());
            }
            assoc.push(0);
            blocks.push(block([0x90 + k as u8; 16], 0x84, &record(WALL, &wall_data(id, y, 2.7), &assoc), 0));
        }
        let mut file = b"ROF FDB ".to_vec();
        file.resize(32, 0);
        file.extend(blocks.concat());
        file
    }

    /// The volume of an element of a layer (divergence theorem; a closed body).
    fn volume(layer: &Layer, element: usize) -> f64 {
        let (_, first, count) = layer.elements[element];
        let p = |i: u32| glam::DVec3::from_array(std::array::from_fn(|k| layer.positions[3 * i as usize + k] as f64));
        layer.indices[first as usize..(first + count) as usize].chunks_exact(3).map(|t| p(t[0]).dot(p(t[1]).cross(p(t[2]))) / 6.0).sum()
    }

    /// §8.9: a wall with 1500 openings, each a millimetre wide in a row: the first 1000 are cut,
    /// quickly, the rest skipped and counted.
    #[test]
    fn many_openings_in_one_wall_stay_fast() {
        let start = web_time::Instant::now();
        let mut story = vec![0u8; 10];
        story.extend(0u32.to_le_bytes());
        story.extend(0i16.to_le_bytes());
        let mut blocks = vec![block(STORY_ID, 0x84, &record(STORY, &story, &[0, 0, 0]), 0)];
        let count = 1500u32;
        let id = |k: u32| {
            let mut g = [0x5A; 16];
            g[..4].copy_from_slice(&k.to_le_bytes());
            g
        };
        for k in 0..count {
            let data = opening_data("N", [0.001 + 0.0025 * k as f64, 0.001, 1.0, 0.5]);
            blocks.push(block(id(k), 0x84, &record(WINDOW, &data, &[0, 0, 0]), 0));
        }
        let mut assoc = vec![0, 0, 0x12];
        assoc.extend(ON_STORY);
        assoc.extend(STORY_ID);
        assoc.push(0x01);
        assoc.extend(WALL_WINDOWS);
        assoc.extend(count.to_le_bytes());
        assoc.extend((0..count).flat_map(id));
        assoc.push(0);
        blocks.push(block([0x91; 16], 0x84, &record(WALL, &wall_data("W", 0.0, 2.7), &assoc), 0));
        let mut file = b"ROF FDB ".to_vec();
        file.resize(32, 0);
        file.extend(blocks.concat());
        let (loaded, stats) = convert(read_bytes(&file).unwrap()).unwrap();
        assert_eq!((stats.openings.len(), stats.openings_skipped, stats.openings_too_many), (1000, 500, 500));
        assert!(loaded.project.info.iter().any(|(k, v)| k == "Archicad openings" && v.contains("500 past the first 1000 of one wall")));
        let walls = &loaded.layers[0];
        assert_eq!(walls.solid_indices().len(), walls.indices.len());
        assert!(start.elapsed().as_secs() < 30, "{:?}", start.elapsed());
    }

    /// Library-object data (§8.10): x, y, rotation, z, A, B at 64, with the marks 17 bytes before
    /// them when `marked`; a second set at 150 makes the place ambiguous.
    fn object_data(fields: [f64; 6], marked: bool, twice: bool) -> Vec<u8> {
        let mut data = vec![0u8; 40];
        for p in if twice { vec![64, 150] } else { vec![64] } {
            data.resize(p - 17, 0);
            if marked {
                data.extend([1, 0, 0, 0, 0, 0, 1, 0]);
            }
            data.resize(p, 0);
            data.extend(fields.iter().flat_map(|v| v.to_le_bytes()));
        }
        data.resize(data.len() + 20, 0);
        data
    }

    /// An outline cache (§8.10): xmin, xmax, ymin, ymax at 9; rotation, x, y at 67.
    fn outline_data([x0, x1, y0, y1]: [f64; 4], [rotation, x, y]: [f64; 3]) -> Vec<u8> {
        let mut data = vec![0u8; 91];
        for (at, v) in [(9, x0), (17, x1), (25, y0), (33, y1), (67, rotation), (75, x), (83, y)] {
            data[at..at + 8].copy_from_slice(&v.to_le_bytes());
        }
        data
    }

    /// A parameter block (§8.10): items of 72 bytes, the value before a 32-byte name.
    fn parameters_data(items: &[(&str, f64)]) -> Vec<u8> {
        let mut data = vec![0u8; 32];
        for (name, value) in items {
            data.extend(value.to_le_bytes());
            let mut field = [0u8; 32];
            field[..name.len()].copy_from_slice(name.as_bytes());
            data.extend(field);
            data.resize(data.len() + 32, 0);
        }
        data
    }

    /// A library reference (§8.10): some bytes, then the library part's file name as a u32
    /// length and UTF-16 characters.
    fn library_data(name: &str) -> Vec<u8> {
        let mut data = vec![7u8; 40];
        for text in [format!("{name}.gsm"), name.to_string(), "Library 26".to_string()] {
            data.extend([1, 0]);
            data.extend((text.encode_utf16().count() as u32).to_le_bytes());
            data.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
            data.extend([0u8; 6]);
        }
        data
    }

    /// Library objects on the story "Hall" (4.5 m), 0x84 blocks in the outer layer:
    /// - O1 "Table 26": 2 × 1, z 0.1, ZZYZX 0.8, anchored at its corner (1, 1) and a cache
    ///   1..3 × 1..2 that A × B fits: drawn around the middle of the cache;
    /// - O2 "Sofa 26": 1 × 1, ZZYZX 0.7, a larger cache 5..7 × 5..6.5: drawn as the cache;
    /// - O3 "Lamp 26": no cache, 1 × 0.5 turned 90° around (10, 10), ZZYZX 0.9 in a parameter
    ///   block that comes before its library reference in the file;
    /// - O4 "Chair 26": no cache, no ZZYZX: 1 m high, height unknown;
    /// - O5 "Tree 26": ZZYZX 0: skipped; O6: no story, a 2D symbol: ignored;
    /// - O7: two places for its fields: skipped.
    fn objects_file() -> Vec<u8> {
        let mut story = vec![0u8; 10];
        story.extend(4u32.to_le_bytes());
        story.extend("Hall".encode_utf16().flat_map(u16::to_le_bytes));
        story.extend(2i16.to_le_bytes());
        let other = guid("ffeeddccbbaa99887766554433221100");
        let mut blocks = vec![
            block(STORY_ID, 0x84, &record(STORY, &story, &[0, 0, 0]), 0),
            block([0x21; 16], 0x84, &record(STORY_LEVEL, &4.5f64.to_le_bytes(), &link(other, STORY_ID)), 0),
        ];

        let id = |kind: u8, k: u8| [kind, k, kind, k, kind, k, kind, k, kind, k, kind, k, kind, k, kind, k];
        // O3's parameter block before everything that refers to it
        blocks.push(block(id(0xB3, 3), 0x84, &record(PARAMETERS, &parameters_data(&[("A", 1.0), ("ZZYZX", 0.9)]), &[0, 0, 0]), 0));
        // number, library part, fields, outline cache (bounds, rotation and x, y), ZZYZX, placed
        // on a story, fields twice
        type Synthetic = (u8, &'static str, [f64; 6], Option<([f64; 4], [f64; 3])>, Option<f64>, bool, bool);
        let objects: [Synthetic; 7] = [
            (1, "Table 26", [1.0, 1.0, 0.0, 0.1, 2.0, 1.0], Some(([1.0, 3.0, 1.0, 2.0], [0.0, 1.0, 1.0])), Some(0.8), true, false),
            (2, "Sofa 26", [5.0, 5.0, 0.0, 0.0, 1.0, 1.0], Some(([5.0, 7.0, 5.0, 6.5], [0.0, 5.0, 5.0])), Some(0.7), true, false),
            (3, "Lamp 26", [10.0, 10.0, std::f64::consts::FRAC_PI_2, 0.0, 1.0, 0.5], None, Some(0.9), true, false),
            (4, "Chair 26", [12.0, 1.0, 0.0, 0.0, 0.5, 0.5], None, None, true, false),
            (5, "Tree 26", [14.0, 1.0, 0.0, 0.0, 3.0, 3.0], None, Some(0.0), true, false),
            (6, "Symbol 26", [16.0, 1.0, 0.0, 0.0, 1.0, 1.0], None, Some(1.0), false, false),
            (7, "Twice 26", [18.0, 1.0, 0.0, 0.0, 1.0, 1.0], None, Some(1.0), true, true),
        ];
        for (k, name, fields, outline, height, story, twice) in objects {
            let mut items = Vec::new();
            if story {
                items.push((0x12, ON_STORY, STORY_ID));
            }
            if let Some((bounds, at)) = outline {
                items.push((0x11, OBJECT_OUTLINE, id(0xA0, k)));
                blocks.push(block(id(0xA0, k), 0x84, &record(OUTLINE, &outline_data(bounds, at), &[0, 0, 0]), 0));
            }
            items.push((0x11, OBJECT_LIBRARY, id(0xB0, k)));
            let data = object_data(fields, outline.is_none(), twice);
            blocks.push(block(id(0xD0, k), 0x84, &record(OBJECT, &data, &links_of(&items)), 0));
            let parameters = match height {
                Some(h) => vec![("A", fields[4]), ("ZZYZX", h)],
                None => vec![("A", fields[4]), ("B", fields[5])],
            };
            blocks.push(block(id(0xB0, k), 0x84, &record(LIBRARY, &library_data(name), &links_of(&[(0x11, LIBRARY_PARAMETERS, id(0xB3, k))])), 0));
            if k != 3 {
                blocks.push(block(id(0xB3, k), 0x84, &record(PARAMETERS, &parameters_data(&parameters), &[0, 0, 0]), 0));
            }
        }
        let mut file = b"ROF FDB ".to_vec();
        file.resize(32, 0);
        file.extend(blocks.concat());
        file
    }

    /// An ID string as in §8.2: `01 00`, u32 n and n UTF-16LE units.
    fn id_string(text: &str) -> Vec<u8> {
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut out = vec![1, 0];
        out.extend((units.len() as u32).to_le_bytes());
        out.extend(units.iter().flat_map(|u| u.to_le_bytes()));
        out
    }

    /// Mesh data (§8.11): its name ending at P − 115 where ref starts, t at P − 60, the outline
    /// polygon at P, then `after` (the layout without cache).
    fn mesh_data(name: &str, [reference, thickness]: [f64; 2], outline: &[Vec<[f64; 2]>], after: &[u8]) -> Vec<u8> {
        let mut data = vec![0u8; 20];
        data.extend(id_string(name));
        let q = data.len();
        data.extend(reference.to_le_bytes());
        data.resize(q + 55, 0);
        data.extend(thickness.to_le_bytes());
        data.resize(q + 115, 0);
        data.extend(polygon_bytes(outline));
        data.extend(after);
        data
    }

    /// The layout after the polygon of a mesh without cache (§8.11): the vertex numbers of the
    /// polygon points (point 0 first), and the heights of those vertices.
    fn mesh_fallback(ids: &[u32], heights: &[(u32, f64)]) -> Vec<u8> {
        let list = |n: usize| [&1u16.to_le_bytes()[..], &(n as u32).to_le_bytes()].concat();
        let mut out = list(ids.len());
        out.extend(ids.iter().flat_map(|v| v.to_le_bytes()));
        for _ in 0..2 {
            out.extend(list(ids.len()));
            out.extend(vec![0u8; 4 * ids.len()]);
        }
        out.extend(list(1));
        out.extend([0u8; 16]);
        out.extend([0u8; 16]);
        // one point inside the outline, left out
        out.extend(1u32.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend([5.0f64, 5.0, 9.0].iter().flat_map(|v| v.to_le_bytes()));
        out.extend(99u32.to_le_bytes());
        out.extend(list(0));
        out.extend([0u8; 10]);
        out.extend((heights.len() as u32).to_le_bytes());
        for (id, z) in heights {
            out.extend(id.to_le_bytes());
            out.extend(z.to_le_bytes());
        }
        out
    }

    /// A triangulation cache (§8.11): the vertices, triangles (status, e0, e1, e2) and edges
    /// (kind, va, vb).
    fn mesh_cache_data(vertices: &[[f64; 3]], triangles: &[[i32; 4]], edges: &[[i32; 3]]) -> Vec<u8> {
        let mut out = vec![1, 0, 3, 0, 0];
        out.extend((vertices.len() as u32).to_le_bytes());
        out.extend(vertices.iter().flatten().flat_map(|v| v.to_le_bytes()));
        out.extend((triangles.len() as u32).to_le_bytes());
        out.extend(triangles.iter().flatten().flat_map(|v| v.to_le_bytes()));
        out.extend((edges.len() as u32).to_le_bytes());
        for [kind, a, b] in edges {
            out.extend([*kind, 0, 0, *a, *b, 0, 0].iter().flat_map(|v| v.to_le_bytes()));
        }
        out
    }

    /// Zone data (§8.12): its name ending where its number's ID string starts, the number ending
    /// at P0 − 63 where Cb, H and S start, the polygon at P0, and T 33 bytes before the end.
    fn zone_data(number: &str, name: &str, [bottom, height, floor, top]: [f64; 4], outline: &[Vec<[f64; 2]>]) -> Vec<u8> {
        let mut data = vec![0u8; 20];
        data.extend(id_string(name));
        data.extend(id_string(number));
        let q = data.len();
        data.extend([bottom, height, floor].iter().flat_map(|v| v.to_le_bytes()));
        data.resize(q + 63, 0);
        data.extend(polygon_bytes(outline));
        data.extend([0u8; 8]);
        data.extend(top.to_le_bytes());
        data.extend([0u8; 25]);
        data
    }

    /// Meshes and zones on the story "Hall" (4.5 m), with the story above the top story at 7.5 m:
    /// - M1 "Hill": the square 0..10, ref 0.2, t 1: a cache of the 4 corners (z 0, 0, 1, 1) and a
    ///   middle (z 2), 4 triangles around the middle, a free one and one with a bad edge;
    /// - M2 "Flat": the square 20..30 without cache, its corners at 0, 0.5, 1 and 0.5;
    /// - Z1 "01 Hall room  ": 6 × 4 with a 1 × 1 hole, its top linked (7.5 − 0.3);
    /// - Z2 "02 Store": not linked, Cb 0.1, S 0.1, H 2 (its T of 99 out of date);
    /// - Z3 "03 Void": not linked, H 0: skipped.
    fn meshes_and_zones_file() -> Vec<u8> {
        let mut story = vec![0u8; 10];
        story.extend(4u32.to_le_bytes());
        story.extend("Hall".encode_utf16().flat_map(u16::to_le_bytes));
        story.extend(2i16.to_le_bytes());
        let other = guid("ffeeddccbbaa99887766554433221100");
        let square = |x: f64| vec![rectangle(x, 0.0, x + 10.0, 10.0)];
        let mut blocks = vec![
            block(STORY_ID, 0x84, &record(STORY, &story, &[0, 0, 0]), 0),
            block([0x21; 16], 0x84, &record(STORY_LEVEL, &4.5f64.to_le_bytes(), &link(other, STORY_ID)), 0),
            block([0x22; 16], 0x84, &record(STORY_LEVEL, &7.5f64.to_le_bytes(), &link(other, TOP_STORY)), 0),
            block(LINKED_TO_TOP, 0x84, &record(LINK, &[0; 16], &links_of(&[(0x12, LINK_STORY, TOP_STORY)])), 0),
        ];
        // M1: edges 1-4 along the outline, 5-8 from the middle (4) to each corner
        let vertices = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 1.0], [0.0, 10.0, 1.0], [5.0, 5.0, 2.0]];
        let mut edges: Vec<[i32; 3]> = (0..4).map(|k| [12, k, (k + 1) % 4]).collect();
        edges.extend((0..4).map(|k| [0, 4, k]));
        let mut triangles: Vec<[i32; 4]> = (0..4).map(|k| [0, k + 1, -(5 + (k + 1) % 4), 5 + k]).collect();
        triangles.push([1, 1, 2, 3]);
        triangles.push([0, 0, 2, 3]);
        blocks.push(block([0x31; 16], 0x84, &record(MESH_CACHE, &mesh_cache_data(&vertices, &triangles, &edges), &[0, 0, 0]), 0));
        let hill = mesh_data("Hill", [0.2, 1.0], &square(0.0), &[1, 2, 3, 4, 5]);
        blocks.push(block([0x32; 16], 0x84, &record(MESH, &hill, &links_of(&[(0x12, ON_STORY, STORY_ID), (0x11, MESH_CACHE_LINK, [0x31; 16])])), 0));
        let fallback = mesh_fallback(&[0, 11, 12, 13, 14, 11], &[(11, 0.0), (12, 0.5), (13, 1.0), (14, 0.5)]);
        let flat = mesh_data("Flat", [0.2, 1.0], &square(20.0), &fallback);
        blocks.push(block([0x33; 16], 0x84, &record(MESH, &flat, &links_of(&[(0x12, ON_STORY, STORY_ID)])), 0));
        let hall = vec![rectangle(0.0, 20.0, 6.0, 24.0), rectangle(2.0, 21.0, 3.0, 22.0)];
        let zones: [(&str, &str, [f64; 4], bool); 3] =
            [("01", "Hall room  ", [0.0, 2.5, 0.1, -0.3], true), ("02", "Store", [0.1, 2.0, 0.1, 99.0], false), ("03", "Void", [0.0, 0.0, 0.0, 0.0], false)];
        for (k, (number, name, values, linked)) in zones.into_iter().enumerate() {
            let mut items = vec![(0x12, ON_STORY, STORY_ID)];
            if linked {
                items.push((0x11, ZONE_TOP_LINK, LINKED_TO_TOP));
            }
            let outline = if k == 0 { hall.clone() } else { vec![rectangle(10.0 * k as f64, 20.0, 10.0 * k as f64 + 2.0, 22.0)] };
            blocks.push(block([0x40 + k as u8; 16], 0x84, &record(ZONE, &zone_data(number, name, values, &outline), &links_of(&items)), 0));
        }
        let mut file = b"ROF FDB ".to_vec();
        file.resize(32, 0);
        file.extend(blocks.concat());
        file
    }

    /// A fill of a profile (§8.13): `u16 1`, the points (point 0 at (0, 0), each contour closed),
    /// `u16 1`, the arcs, `u16 1`, the contour ends.
    fn fill_bytes(contours: &[Vec<[f64; 2]>], arcs: &[(u32, u32, f64)]) -> Vec<u8> {
        let mut points = vec![[0.0, 0.0]];
        let mut ends = vec![0u32];
        for c in contours {
            points.extend(c.iter().copied().chain([c[0]]));
            ends.push(points.len() as u32 - 1);
        }
        let list = |n: usize| [&1u16.to_le_bytes()[..], &(n as u32).to_le_bytes()].concat();
        let mut out = list(points.len());
        out.extend(points.iter().flatten().flat_map(|v| v.to_le_bytes()));
        out.extend(list(arcs.len()));
        for (from, to, angle) in arcs {
            out.extend(from.to_le_bytes());
            out.extend(to.to_le_bytes());
            out.extend(angle.to_le_bytes());
        }
        out.extend(list(ends.len()));
        out.extend(ends.iter().flat_map(|v| v.to_le_bytes()));
        out
    }

    /// A profile object (§8.13): version 2, "$mm" and the bounding box at 366, K at 430, the fills
    /// from 434, then the same fills once more.
    fn profile_data(bbox: [f64; 4], k: u32, fills: &[Vec<u8>]) -> Vec<u8> {
        let mut data = 2u16.to_le_bytes().to_vec();
        data.resize(366, 0);
        data.extend(b"$mm");
        data.extend(bbox.iter().flat_map(|v| v.to_le_bytes()));
        data.resize(430, 0);
        data.extend(k.to_le_bytes());
        for _ in 0..2 {
            for fill in fills {
                data.extend(fill);
                data.extend([0u8; 3]);
            }
        }
        data
    }

    /// Profile beams and a profile column on the story "Hall" (4.5 m), each with its own segment,
    /// profile section and profile object:
    /// - BA: 0.2 × 0.1 with its top edge an arc of +π (bulging up to 0.2), along +x at y 0, the
    ///   axis at zr 0.5, mirror flag 0;
    /// - BM: 0.3 × 0.1 along +x at y 5, mirror flag 1;
    /// - BS: 0.3 × 0.1 along +x at y 10, its axis from 0 to 1 above the story (sloped);
    /// - BB: a header with K 2 and its fills cut off: drawn as the box −0.05..0.05 × 0..0.2, at y 15;
    /// - BX: a profile object whose header does not read: skipped;
    /// - KP: a column with an L-shaped profile around (20, 20), C 0, H 2.
    fn profiles_file() -> Vec<u8> {
        let mut story = vec![0u8; 10];
        story.extend(4u32.to_le_bytes());
        story.extend("Hall".encode_utf16().flat_map(u16::to_le_bytes));
        story.extend(2i16.to_le_bytes());
        let other = guid("ffeeddccbbaa99887766554433221100");
        let mut blocks = vec![
            block(STORY_ID, 0x84, &record(STORY, &story, &[0, 0, 0]), 0),
            block([0x21; 16], 0x84, &record(STORY_LEVEL, &4.5f64.to_le_bytes(), &link(other, STORY_ID)), 0),
        ];
        let square = |w: f64| vec![rectangle(0.0, 0.0, w, 0.1)];
        let arc = fill_bytes(&square(0.2), &[(3, 4, std::f64::consts::PI)]);
        let flat = fill_bytes(&square(0.3), &[]);
        let ell = fill_bytes(&[vec![[0.0, 0.0], [0.3, 0.0], [0.3, 0.1], [0.1, 0.1], [0.1, 0.3], [0.0, 0.3]]], &[]);
        let mut bad = profile_data([0.0, 0.0, 0.3, 0.1], 1, std::slice::from_ref(&flat));
        bad[366..369].copy_from_slice(b"xxx");
        let profiles = [
            profile_data([0.0, 0.0, 0.2, 0.2], 1, std::slice::from_ref(&arc)),
            profile_data([0.0, 0.0, 0.3, 0.1], 1, std::slice::from_ref(&flat)),
            profile_data([0.0, 0.0, 0.3, 0.1], 1, std::slice::from_ref(&flat)),
            profile_data([-0.05, 0.0, 0.05, 0.2], 2, &[fill_bytes(&[rectangle(-0.05, 0.0, 0.05, 0.2)], &[])])[..434 + 100].to_vec(),
            bad,
            profile_data([0.0, 0.0, 0.3, 0.3], 1, std::slice::from_ref(&ell)),
        ];
        let id = |kind: u8, k: usize| [kind, k as u8, kind, k as u8, kind, k as u8, kind, k as u8, kind, k as u8, kind, k as u8, kind, k as u8, kind, k as u8];
        for (k, profile) in profiles.iter().enumerate() {
            blocks.push(block(id(0xE0, k), 0x84, &record(PROFILE, profile, &[0, 0, 0]), 0));
            blocks.push(block(id(0xE1, k), 0x84, &record(SECTION_PROFILE, &[0; 32], &links_of(&[(0x11, PROFILE_LINK, id(0xE0, k))])), 0));
            let class = if k == 5 { COLUMN_SEGMENT } else { BEAM_SEGMENT };
            blocks.push(block(id(0xE2, k), 0x84, &record(class, &[0; 8], &links_of(&[(0x12, SECTION_LINK, id(0xE1, k))])), 0));
        }
        // name, y, axis heights, mirror flag
        type Synthetic = (&'static str, f64, Option<(f64, f64)>, u8);
        let beams: [Synthetic; 5] =
            [("BA", 0.0, None, 0), ("BM", 5.0, None, 1), ("BS", 10.0, Some((0.0, 1.0)), 0), ("BB", 15.0, None, 0), ("BX", 20.0, None, 0)];
        for (k, (name, y, heights, flag)) in beams.into_iter().enumerate() {
            let mut data = beam_data(name, 0.0, 0.5, [1.0, y, 5.0, y], heights);
            let q = 104 + 2 * name.len();
            data.resize(q + 96, 0);
            data[q + 95] = flag;
            let items = [(0x12, ON_STORY, STORY_ID), (0x12, BEAM_SEGMENT_LINK, id(0xE2, k))];
            blocks.push(block(id(0xE3, k), 0x84, &record(BEAM, &data, &links_of(&items)), 0));
        }
        let column = column_data("KP", [20.0, 20.0], [20.0, 20.0], 0.0, [0.0, 2.0, 0.0]);
        let items = [(0x12, ON_STORY, STORY_ID), (0x12, COLUMN_SEGMENT_LINK, id(0xE2, 5))];
        blocks.push(block(id(0xE4, 5), 0x84, &record(COLUMN, &column, &links_of(&items)), 0));
        let mut file = b"ROF FDB ".to_vec();
        file.resize(32, 0);
        file.extend(blocks.concat());
        file
    }

    /// Slab data with a polygon with arcs (§8.16.1): its bbox, counts, points (point 0 at (0, 0),
    /// one closed contour), the arcs after an empty item, the contour ends, and the z cache 0..0.3.
    fn arc_slab_data(points: &[[f64; 2]], arcs: &[(u32, u32, f64)], bbox: [f64; 4]) -> Vec<u8> {
        let mut data = vec![1u8; 5];
        let bbox_bytes: Vec<u8> = bbox.iter().flat_map(|v| v.to_le_bytes()).collect();
        data.extend(&bbox_bytes);
        data.extend([points.len() as u32, arcs.len() as u32, 1].iter().flat_map(|v| v.to_le_bytes()));
        data.extend(std::iter::once([0.0, 0.0]).chain(points.iter().copied()).flatten().flat_map(f64::to_le_bytes));
        data.extend([0u8; 16]);
        for (from, to, angle) in arcs {
            data.extend(from.to_le_bytes());
            data.extend(to.to_le_bytes());
            data.extend(angle.to_le_bytes());
        }
        data.extend([0u32, points.len() as u32].iter().flat_map(|v| v.to_le_bytes()));
        data.extend([3u8; 7]);
        data.extend(&bbox_bytes);
        data.extend([0.0f64, 0.3].iter().flat_map(|v| v.to_le_bytes()));
        data
    }

    /// Slabs with arcs (§8.16.1): S1, 4 × 2 whose right edge is an arc of +π/2 (bulging out to
    /// x = 3 + √2); S2, the same at x 10 with also its left edge an arc of −π/2 (bulging in).
    fn arcs_file() -> Vec<u8> {
        let quarter = std::f64::consts::FRAC_PI_2;
        let out = 3.0 + 2f64.sqrt();
        let s1 = arc_slab_data(&[[0.0, 0.0], [4.0, 0.0], [4.0, 2.0], [0.0, 2.0], [0.0, 0.0]], &[(2, 3, quarter)], [0.0, 0.0, out, 2.0]);
        let s2 = arc_slab_data(
            &[[10.0, 0.0], [14.0, 0.0], [14.0, 2.0], [10.0, 2.0], [10.0, 0.0]],
            &[(2, 3, quarter), (4, 5, -quarter)],
            [10.0, 0.0, 10.0 + out, 2.0],
        );
        let mut file = b"ROF FDB ".to_vec();
        file.resize(32, 0);
        file.extend(block([0x51; 16], 0x84, &record(SLAB, &s1, &[0, 0, 0]), 0));
        file.extend(block([0x52; 16], 0x84, &record(SLAB, &s2, &[0, 0, 0]), 0));
        file
    }

    /// A morph body (§8.16.3) of these vertices and faces (loops of vertex indices, the outer one
    /// counter-clockwise seen from outside, holes the other way): the edges and their faces, the
    /// edge references (0 between loops), a normal per face and the faces.
    fn body_data(vertices: &[[f64; 3]], faces: &[Vec<Vec<usize>>]) -> Vec<u8> {
        let mut edges: Vec<(usize, usize, u32, u32)> = Vec::new();
        let mut refs: Vec<i32> = Vec::new();
        let mut spans = Vec::new();
        for (f, loops) in faces.iter().enumerate() {
            let first = refs.len();
            for (k, l) in loops.iter().enumerate() {
                if k > 0 {
                    refs.push(0);
                }
                for i in 0..l.len() {
                    let (a, b) = (l[i], l[(i + 1) % l.len()]);
                    let found = edges.iter().position(|e| (e.0, e.1) == (a, b) || (e.0, e.1) == (b, a));
                    let k = found.unwrap_or_else(|| {
                        edges.push((a, b, u32::MAX, u32::MAX));
                        edges.len() - 1
                    });
                    let e = &mut edges[k];
                    if e.2 == u32::MAX { e.2 = f as u32 } else { e.3 = f as u32 }
                    refs.push(if (e.0, e.1) == (a, b) { k as i32 + 1 } else { -(k as i32 + 1) });
                }
            }
            spans.push((first, refs.len() - 1));
        }
        let mut data = 4u16.to_le_bytes().to_vec();
        data.push(0x83);
        data.resize(156, 0);
        data.extend((vertices.len() as u32).to_le_bytes());
        data.extend(vertices.iter().flatten().flat_map(|v| v.to_le_bytes()));
        data.extend(0u32.to_le_bytes());
        data.extend((edges.len() as u32).to_le_bytes());
        for (a, b, f1, f2) in &edges {
            data.extend([0x40000u32, 0, *a as u32, *b as u32, *f1, *f2].iter().flat_map(|v| v.to_le_bytes()));
        }
        data.extend((refs.len() as u32).to_le_bytes());
        data.extend(refs.iter().flat_map(|v| v.to_le_bytes()));
        data.extend((faces.len() as u32).to_le_bytes());
        data.extend(std::iter::repeat_n([0.0f64, 0.0, 1.0], faces.len()).flatten().flat_map(f64::to_le_bytes));
        data.extend((faces.len() as u32).to_le_bytes());
        for (f, (first, last)) in spans.iter().enumerate() {
            data.extend([2u32, 0, 0, 0, 0, f as u32 + 1, *first as u32, *last as u32, 0, 0].iter().flat_map(|v| v.to_le_bytes()));
        }
        data.extend([0u8; 16]);
        data
    }

    /// A box from (0, 0, 0) to `size`: 8 vertices, 6 faces facing out.
    fn box_body(size: [f64; 3]) -> Vec<u8> {
        let v: Vec<[f64; 3]> = (0..8).map(|i| [0, 1, 2].map(|k| if i >> k & 1 == 1 { size[k] } else { 0.0 })).collect();
        let middle = size.map(|s| s / 2.0);
        let faces: Vec<Vec<Vec<usize>>> = [[0, 2, 6, 4], [1, 5, 7, 3], [0, 4, 5, 1], [2, 3, 7, 6], [0, 1, 3, 2], [4, 6, 7, 5]]
            .iter()
            .map(|f| {
                // turned to face away from the middle (counter-clockwise seen from outside)
                let (a, b, c) = (v[f[0]], v[f[1]], v[f[2]]);
                let (u, w) = ([0, 1, 2].map(|k| b[k] - a[k]), [0, 1, 2].map(|k| c[k] - a[k]));
                let n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
                let out: f64 = (0..3).map(|k| n[k] * (a[k] - middle[k])).sum();
                let mut l = f.to_vec();
                if out < 0.0 {
                    l.reverse();
                }
                vec![l]
            })
            .collect();
        body_data(&v, &faces)
    }

    /// A 4 × 4 × 1 block with a 2 × 2 hole through it: top and bottom faces with a hole loop.
    fn frame_body() -> Vec<u8> {
        let square = |a: f64, b: f64| [[a, a], [b, a], [b, b], [a, b]];
        let mut v = Vec::new();
        for (ring, z) in [(square(0.0, 4.0), 0.0), (square(0.0, 4.0), 1.0), (square(1.0, 3.0), 0.0), (square(1.0, 3.0), 1.0)] {
            v.extend(ring.iter().map(|p| [p[0], p[1], z]));
        }
        // outer bottom 0-3, outer top 4-7, inner bottom 8-11, inner top 12-15
        let mut faces = vec![vec![vec![4, 5, 6, 7], vec![12, 15, 14, 13]], vec![vec![0, 3, 2, 1], vec![8, 9, 10, 11]]];
        for k in 0..4 {
            let j = (k + 1) % 4;
            faces.push(vec![vec![k, j, 4 + j, 4 + k]]);
            faces.push(vec![vec![8 + j, 8 + k, 12 + k, 12 + j]]);
        }
        body_data(&v, &faces)
    }

    /// Morph data (§8.16.3): its ID string at 40, then 5 bytes and the matrix [R | t].
    fn morph_data(id: &str, m: [f64; 12]) -> Vec<u8> {
        let mut data = vec![0u8; 38];
        data.extend(id_string(id));
        data.extend([0u8; 5]);
        data.extend(m.iter().flat_map(|v| v.to_le_bytes()));
        data.extend([0u8; 8]);
        data
    }

    /// Morphs on the story "Hall" (4.5 m): A, a 2 × 1 × 1 box moved to x 10; B, the same body
    /// turned 90° about z and moved to x 20; C, the block with a hole at x 30; D, an open body (one
    /// square face) at x 40; E, a body that does not read; F, no placement (a matrix that is not a
    /// rotation).
    fn morphs_file() -> Vec<u8> {
        let mut story = vec![0u8; 10];
        story.extend(4u32.to_le_bytes());
        story.extend("Hall".encode_utf16().flat_map(u16::to_le_bytes));
        story.extend(2i16.to_le_bytes());
        let other = guid("ffeeddccbbaa99887766554433221100");
        let mut blocks = vec![
            block(STORY_ID, 0x84, &record(STORY, &story, &[0, 0, 0]), 0),
            block([0x21; 16], 0x84, &record(STORY_LEVEL, &4.5f64.to_le_bytes(), &link(other, STORY_ID)), 0),
        ];
        let square = body_data(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]], &[vec![vec![0, 1, 2, 3]]]);
        let bodies = [box_body([2.0, 1.0, 1.0]), frame_body(), square, vec![0u8; 200]];
        for (k, body) in bodies.iter().enumerate() {
            blocks.push(block([0x60 + k as u8; 16], 0x84, &record(MORPH_BODY, body, &[0, 0, 0]), 0));
        }
        let moved = |x: f64| [1.0, 0.0, 0.0, x, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let turned = [0.0, -1.0, 0.0, 20.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let mut skewed = moved(50.0);
        skewed[1] = 0.5;
        let morphs = [("A", moved(10.0), 0), ("B", turned, 0), ("C", moved(30.0), 1), ("D", moved(40.0), 2), ("E", moved(45.0), 3), ("F", skewed, 0)];
        for (k, (name, m, body)) in morphs.into_iter().enumerate() {
            let items = [(0x12, ON_STORY, STORY_ID), (0x11, MORPH_BODY_LINK, [0x60 + body as u8; 16])];
            blocks.push(block([0x70 + k as u8; 16], 0x84, &record(MORPH, &morph_data(name, m), &links_of(&items)), 0));
        }
        let mut file = b"ROF FDB ".to_vec();
        file.resize(32, 0);
        file.extend(blocks.concat());
        file
    }

    /// §8.16.3: face ranges that are not contiguous (overlapping, or adding up to many times the
    /// references, or leaving a gap) make the body unreadable, quickly: each reference is visited
    /// once at most.
    #[test]
    fn morph_face_ranges_may_not_overlap() {
        let start = web_time::Instant::now();
        // a box whose second face starts inside the first face's range
        let mut overlapping = box_body([1.0, 1.0, 1.0]);
        let faces_at = overlapping.len() - 16 - 6 * 40;
        overlapping[faces_at + 40 + 24..faces_at + 40 + 28].copy_from_slice(&0u32.to_le_bytes());
        assert!(morph_body(&box_body([1.0, 1.0, 1.0])).is_some() && morph_body(&overlapping).is_none());
        // a gap: the last face ends before the last reference
        let mut gap = box_body([1.0, 1.0, 1.0]);
        let last_at = gap.len() - 16 - 40 + 28;
        let last = u32::from_le_bytes(gap[last_at..last_at + 4].try_into().unwrap());
        gap[last_at..last_at + 4].copy_from_slice(&(last - 1).to_le_bytes());
        assert!(morph_body(&gap).is_none());
        // 10 000 faces, each claiming all 100 000 references: 1e9 steps if each were walked
        let np = 100_000u32;
        let nf = 10_000u32;
        let mut huge = 4u16.to_le_bytes().to_vec();
        huge.resize(156, 0);
        huge.extend(4u32.to_le_bytes());
        huge.extend([[0.0f64, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]].iter().flatten().flat_map(|v| v.to_le_bytes()));
        huge.extend(0u32.to_le_bytes());
        huge.extend(4u32.to_le_bytes());
        for k in 0..4u32 {
            huge.extend([0x40000, 0, k, (k + 1) % 4, 0, u32::MAX].iter().flat_map(|v| v.to_le_bytes()));
        }
        huge.extend(np.to_le_bytes());
        huge.extend((0..np).flat_map(|k| ((k % 5) as i32 % 5).to_le_bytes()));
        huge.extend(1u32.to_le_bytes());
        huge.extend([0.0f64, 0.0, 1.0].iter().flat_map(|v| v.to_le_bytes()));
        huge.extend(nf.to_le_bytes());
        for _ in 0..nf {
            huge.extend([2u32, 0, 0, 0, 0, 1, 0, np - 1, 0, 0].iter().flat_map(|v| v.to_le_bytes()));
        }
        assert!(huge.len() <= MAX_ELEMENT_DATA, "{}", huge.len());
        assert!(morph_body(&huge).is_none());
        // in a project: the morph is skipped and counted
        let mut file = b"ROF FDB ".to_vec();
        file.resize(32, 0);
        file.extend(block([0x61; 16], 0x84, &record(MORPH_BODY, &huge, &[0, 0, 0]), 0));
        file.extend(block([0x62; 16], 0x84, &record(MORPH_BODY, &overlapping, &[0, 0, 0]), 0));
        for k in 0..2u8 {
            let m = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
            file.extend(block([0x70 + k; 16], 0x84, &record(MORPH, &morph_data("M", m), &links_of(&[(0x11, MORPH_BODY_LINK, [0x61 + k; 16])])), 0));
        }
        file.extend(block([0x51; 16], 0x84, &record(SLAB, &polygon_data(), &[0, 0, 0]), 0));
        let (_, stats) = convert(read_bytes(&file).unwrap()).unwrap();
        assert_eq!((stats.morphs.len(), stats.morphs_no_body), (0, 2));
        assert!(start.elapsed().as_secs() < 20, "{:?}", start.elapsed());
    }

    /// Stair data (§8.14): its ID string at 40; the width and H, then (29 bytes after the width)
    /// the anchor: n, r, g, the distance of the walking line, the origin, the bottom offset and
    /// the matrix.
    fn stair_data(id: &str, (n, r, g): (u32, f64, f64), origin: [f64; 2], m: [f64; 9]) -> Vec<u8> {
        let mut data = vec![0u8; 38];
        data.extend(id_string(id));
        let width_at = data.len() + 10;
        data.resize(width_at, 0);
        data.extend([1.0, n as f64 * r].iter().flat_map(|v| v.to_le_bytes()));
        let a = width_at + 29;
        data.resize(a, 0);
        data.extend(n.to_le_bytes());
        data.extend([r, g].iter().flat_map(|v| v.to_le_bytes()));
        data.resize(a + 25, 0);
        data.extend([0.5, origin[0], origin[1], 0.0].iter().chain(&m).flat_map(|v| v.to_le_bytes()));
        data.extend([0u8; 12]);
        data
    }

    /// A stair scheme (§8.14): 9 bytes, the riser lines (318 bytes each), L1 and L2 (the same
    /// treads, without nosing), then five empty lists.
    fn scheme_data(risers: &[[f64; 4]], treads: &[Vec<[f64; 2]>]) -> Vec<u8> {
        let mut data = vec![0u8; 9];
        data.extend((risers.len() as u32).to_le_bytes());
        for line in risers {
            let start = data.len();
            data.extend([3, 0, 0x3D, 0]);
            data.extend(line.iter().flat_map(|v| v.to_le_bytes()));
            data.resize(start + 318, 0);
        }
        for _ in 0..2 {
            data.extend((treads.len() as u32).to_le_bytes());
            for tread in treads {
                let polygon = polygon_bytes(std::slice::from_ref(tread));
                data.extend([2, 1]);
                data.extend((3 + polygon.len() as u32 + 4).to_le_bytes());
                data.extend([1, 0x1F, 0]);
                data.extend(polygon);
                data.extend([0u8; 4]);
            }
        }
        data.extend([0u8; 20]);
        data
    }

    /// A railing segment cache (§8.15): from 9, the lines t 0 to 5 (the reference line, the lower
    /// line and the upper one h above it, and their copies), each `u32 t · u32 2 · six f64`;
    /// `upper` false leaves out lines 2 and 5.
    fn segment_data(a: [f64; 3], b: [f64; 3], h: f64, upper: bool) -> Vec<u8> {
        let mut data = vec![0u8; 9];
        for t in 0u32..6 {
            let up = if t == 2 || t == 5 { h } else { 0.0 };
            if !upper && up > 0.0 {
                continue;
            }
            data.extend(t.to_le_bytes());
            data.extend(2u32.to_le_bytes());
            data.extend([a[0], a[1], a[2] + up, b[0], b[1], b[2] + up].iter().flat_map(|v| v.to_le_bytes()));
        }
        data.extend([0u8; 30]);
        data
    }

    /// Stairs and railings on the story "Hall" (4.5 m):
    /// - S1: 4 risers of 0.2 and 4 treads of 1 × 0.3, a riser at the front of each tread, at
    ///   (10, 0), not turned;
    /// - S2: the same mirrored (x turned to −x) at (20, 0);
    /// - S3: 3 risers and 4 treads: it starts with a tread on the floor, at (30, 0);
    /// - S4: a scheme with fewer riser lines than the stair has risers: skipped;
    /// - R1: two segments, (0, 10, 4.5) → (2, 10, 4.5) → (2, 12, 5.0), 1 m high;
    /// - R2: one segment without upper lines, and a bounding box: drawn as the box;
    /// - R3: one segment without upper lines and no box: skipped.
    fn stairs_file() -> Vec<u8> {
        let mut story = vec![0u8; 10];
        story.extend(4u32.to_le_bytes());
        story.extend("Hall".encode_utf16().flat_map(u16::to_le_bytes));
        story.extend(2i16.to_le_bytes());
        let other = guid("ffeeddccbbaa99887766554433221100");
        let mut blocks = vec![
            block(STORY_ID, 0x84, &record(STORY, &story, &[0, 0, 0]), 0),
            block([0x21; 16], 0x84, &record(STORY_LEVEL, &4.5f64.to_le_bytes(), &link(other, STORY_ID)), 0),
        ];
        let g = 0.3;
        let treads: Vec<Vec<[f64; 2]>> = (0..4).map(|i| rectangle(0.0, i as f64 * g, 1.0, (i + 1) as f64 * g)).collect();
        let riser = |i: usize| [0.0, i as f64 * g, 1.0, i as f64 * g];
        let schemes = [
            scheme_data(&(0..4).map(riser).collect::<Vec<_>>(), &treads),
            scheme_data(&(1..4).map(riser).collect::<Vec<_>>(), &treads),
        ];
        for (k, scheme) in schemes.iter().enumerate() {
            blocks.push(block([0x80 + k as u8; 16], 0x84, &record(STAIR_SCHEME, scheme, &[0, 0, 0]), 0));
        }
        let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let mirrored = [-1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let stairs = [("S1", 4, 10.0, identity, 0), ("S2", 4, 20.0, mirrored, 0), ("S3", 3, 30.0, identity, 1), ("S4", 4, 40.0, identity, 1)];
        for (k, (id, n, x, m, scheme)) in stairs.into_iter().enumerate() {
            let data = stair_data(id, (n, 0.2, g), [x, 0.0], m);
            let items = [(0x12, ON_STORY, STORY_ID), (0x11, STAIR_SCHEME_LINK, [0x80 + scheme as u8; 16])];
            blocks.push(block([0x88 + k as u8; 16], 0x84, &record(STAIR, &data, &links_of(&items)), 0));
        }
        let caches = [
            segment_data([0.0, 10.0, 4.5], [2.0, 10.0, 4.5], 1.0, true),
            segment_data([2.0, 10.0, 4.5], [2.0, 12.0, 5.0], 1.0, true),
            segment_data([0.0, 20.0, 4.5], [2.0, 20.0, 4.5], 1.0, false),
            segment_data([0.0, 30.0, 4.5], [2.0, 30.0, 4.5], 1.0, false),
        ];
        for (k, cache) in caches.iter().enumerate() {
            blocks.push(block([0x90 + k as u8; 16], 0x84, &record(SEGMENT_CACHE, cache, &[0, 0, 0]), 0));
            blocks.push(block([0x98 + k as u8; 16], 0x84, &record(RAILING_SEGMENT, &[0; 8], &links_of(&[(0x11, SEGMENT_CACHE_LINK, [0x90 + k as u8; 16])])), 0));
        }
        let mut bbox = vec![2u8; 5];
        bbox.extend([0.0f64, 20.0, 2.0, 20.1, 4.5, 5.5].iter().flat_map(|v| v.to_le_bytes()));
        blocks.push(block([0xA0; 16], 0x84, &record(RAILING_CACHE, &bbox, &[0, 0, 0]), 0));
        for (k, (name, segments, cache)) in [("R1", vec![0u8, 1], false), ("R2", vec![2], true), ("R3", vec![3], false)].into_iter().enumerate() {
            let mut assoc = links_of(&[(0x12, ON_STORY, STORY_ID)]);
            assoc.pop();
            assoc.push(0x01);
            assoc.extend(RAILING_SEGMENTS);
            assoc.extend((segments.len() as u32).to_le_bytes());
            assoc.extend(segments.iter().flat_map(|&s| [0x98 + s; 16]));
            if cache {
                assoc.push(0x11);
                assoc.extend(RAILING_CACHE_LINK);
                assoc.extend([0xA0; 16]);
            }
            assoc.push(0);
            let mut data = vec![0u8; 38];
            data.extend(id_string(name));
            data.extend([0u8; 20]);
            blocks.push(block([0xB0 + k as u8; 16], 0x84, &record(RAILING, &data, &assoc), 0));
        }
        let mut file = b"ROF FDB ".to_vec();
        file.resize(32, 0);
        file.extend(blocks.concat());
        file
    }

    #[test]
    fn draws_stairs_and_railings() {
        let (loaded, stats) = convert(read_bytes(&stairs_file()).expect("container")).expect("elements");
        assert_eq!((stats.stairs.len(), stats.stairs_skipped, stats.stairs_scheme, stats.stairs_floor_treads), (3, 1, 1, 1));
        let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9;
        let by = |name: &str| stats.stairs.iter().find(|s| s.name == name).unwrap_or_else(|| panic!("{name}: {:?}", stats.stairs));
        // S1: starts with a riser: its first tread one riser up, its last at the top
        let s1 = by("S1");
        assert!(s1.starts_with_riser && near(s1.z, (4.5, 5.3)) && near(s1.tread_z, (4.7, 5.3)) && near(s1.x, (10.0, 11.0)) && near(s1.y, (0.0, 1.2)), "{s1:?}");
        // S2: mirrored: the scheme's x to −x
        let s2 = by("S2");
        assert!(s2.mirrored && near(s2.x, (19.0, 20.0)) && near(s2.tread_z, (4.7, 5.3)), "{s2:?}");
        // S3: starts with a tread on the floor (left out): the next one a riser up
        let s3 = by("S3");
        assert!(!s3.starts_with_riser && near(s3.z, (4.5, 5.1)) && near(s3.tread_z, (4.5, 5.1)), "{s3:?}");
        let stairs = loaded.layers.iter().find(|l| l.name == "Stairs").expect("stairs");
        assert_eq!(stairs.solid_indices().len(), stairs.indices.len());
        let (_, _, count) = stairs.elements[2];
        assert_eq!(count / 3, 3 * 12, "three blocks of 12 triangles");
        // the volume of S1: 4 blocks of 1 × 0.3 × 0.2
        let volume = |element: usize| {
            let (_, first, count) = stairs.elements[element];
            let p = |i: u32| glam::DVec3::from_array(std::array::from_fn(|k| stairs.positions[3 * i as usize + k] as f64));
            stairs.indices[first as usize..(first + count) as usize].chunks_exact(3).map(|t| p(t[0]).dot(p(t[1]).cross(p(t[2]))) / 6.0).sum::<f64>()
        };
        assert!((volume(0) - 0.24).abs() < 1e-4 && (volume(1) - 0.24).abs() < 1e-4, "{} {}", volume(0), volume(1));

        // railings: R1 from its two segments, R2 as its box, R3 skipped
        assert_eq!((stats.railings.len(), stats.railings_skipped, stats.railings_boxed, stats.railing_bad_segments), (2, 1, 1, 2));
        let r1 = stats.railings.iter().find(|r| r.name == "R1").unwrap();
        assert!(r1.segments == 2 && (r1.length - 4.0).abs() < 1e-9 && (r1.height - 1.0).abs() < 1e-9 && near(r1.z, (4.5, 6.0)), "{r1:?}");
        let r2 = stats.railings.iter().find(|r| r.name == "R2").unwrap();
        assert!(r2.boxed && near(r2.y, (20.0, 20.1)), "{r2:?}");
        let railings = loaded.layers.iter().find(|l| l.name == "Railings").expect("railings");
        assert_eq!(railings.solid_indices().len(), railings.indices.len());
        // the bands half transparent, the rails and posts not; R1: 2 bands, 2 rails and 3 posts
        let colors = railings.colors.as_ref().unwrap();
        assert_eq!(colors.len(), railings.vertex_count());
        assert!(colors.iter().any(|c| c[3] < 255) && colors.iter().any(|c| c[3] == 255));
        let (_, _, count) = railings.elements[0];
        assert_eq!(count / 3, 7 * 12);
        assert!(railings.props[0].1.contains("\"Height\":\"1000 mm\""));
    }

    #[test]
    fn draws_morphs() {
        let records = read_bytes(&morphs_file()).expect("container");
        assert_eq!((records.morphs.len(), records.bodies.len(), records.bodies.values().filter(|b| b.is_some()).count()), (6, 4, 3));
        let (loaded, stats) = convert(records).expect("morphs");
        assert_eq!((stats.morphs.len(), stats.morphs_skipped, stats.morphs_no_body, stats.morphs_no_matrix, stats.morphs_open), (4, 2, 1, 1, 1));
        assert!(loaded.project.info.iter().any(|(k, v)| k == "Archicad morphs"
            && v == "4 read, 2 skipped (1 without a body that reads, 1 without a placement); 1 open"));
        let layer = &loaded.layers[0];
        assert_eq!((layer.name.as_str(), layer.elements.len()), ("Morphs", 4));
        let volume = |element: usize| {
            let (_, first, count) = layer.elements[element];
            let p = |i: u32| glam::DVec3::from_array(std::array::from_fn(|k| layer.positions[3 * i as usize + k] as f64));
            layer.indices[first as usize..(first + count) as usize].chunks_exact(3).map(|t| p(t[0]).dot(p(t[1]).cross(p(t[2]))) / 6.0).sum::<f64>()
        };
        let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9;
        let by = |name: &str| stats.morphs.iter().find(|m| m.name == name).unwrap_or_else(|| panic!("{name}: {:?}", stats.morphs));
        // A and B share one body: the same volume, B turned 90° about z; both on the story at 4.5
        let (a, b) = (by("A"), by("B"));
        assert!(near(a.x, (10.0, 12.0)) && near(a.y, (0.0, 1.0)) && near(a.z, (4.5, 5.5)), "{a:?}");
        assert!(near(b.x, (19.0, 20.0)) && near(b.y, (0.0, 2.0)) && near(b.z, (4.5, 5.5)), "{b:?}");
        assert!((volume(0) - 2.0).abs() < 1e-4 && (volume(1) - 2.0).abs() < 1e-4, "{} {}", volume(0), volume(1));
        // C: the faces with a hole leave it open through: 16 − 4
        assert!((volume(2) - 12.0).abs() < 1e-4 && by("C").faces == 10, "{}", volume(2));
        // D: open, drawn all the same
        assert!(by("D").open && layer.props[3].1.contains("\"Body\":\"open"));
        // the closed ones are closed and face out
        let (_, first, _) = layer.elements[3];
        assert_eq!(layer.solid_indices().len(), first as usize);
    }

    #[test]
    fn draws_polygons_with_arcs() {
        let (loaded, stats) = convert(read_bytes(&arcs_file()).expect("container")).expect("slabs");
        assert_eq!((stats.slabs, stats.slabs_skipped), (2, 0));
        let segment = 0.5 * (std::f64::consts::FRAC_PI_2 - 1.0) * 2.0; // r² / 2 · (φ − sin φ), r = √2
        let (s1, s2) = (&stats.slab_list[0], &stats.slab_list[1]);
        // S1: the arc bulges out (positive: counter-clockwise, its middle left of the chord); drawn
        // as 8 segments, a little less than the arc
        assert!((s1.x.1 - (3.0 + 2f64.sqrt())).abs() < 1e-9 && s1.area < 8.0 + segment && (s1.area - 8.0 - segment).abs() < 0.02, "{s1:?}");
        // S2: one arc out, one in (negative: clockwise): the area of the rectangle
        assert!((s2.area - 8.0).abs() < 1e-9 && (s2.x.0 - 10.0).abs() < 1e-9 && (s2.x.1 - (13.0 + 2f64.sqrt())).abs() < 1e-9, "{s2:?}");
        let slabs = &loaded.layers[0];
        let inward = slabs.positions.chunks_exact(3).any(|p| (p[0] as f64 - (9.0 + 2f64.sqrt())).abs() < 1e-5 && (p[1] - 1.0).abs() < 1e-5);
        assert!(inward, "the left edge of S2 bulges in to 9 + √2");
        assert_eq!(slabs.solid_indices().len(), slabs.indices.len());
    }

    #[test]
    fn draws_profile_beams_and_columns() {
        let (loaded, stats) = convert(read_bytes(&profiles_file()).expect("container")).expect("elements");
        assert_eq!((stats.beams.len(), stats.beams_skipped, stats.beams_profile, stats.profile_blocks), (4, 1, 1, 1));
        assert_eq!((stats.columns.len(), stats.profiles.len()), (1, 5));
        let by = |name: &str| stats.profiles.iter().find(|p| p.name == name).unwrap_or_else(|| panic!("{name}: {:?}", stats.profiles));
        let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6;
        // BA: the arc bulges the top to 0.2 above the axis (4.5 + 0.5); px to the right of +x: −y
        let ba = by("BA");
        assert!(near(ba.x, (1.0, 5.0)) && near(ba.y, (-0.2, 0.0)) && near(ba.z, (5.0, 5.2)), "{ba:?}");
        let half_disc = std::f64::consts::PI * 0.01 / 2.0;
        assert!((ba.area - (0.02 + half_disc)).abs() < 2e-4 && ba.area < 0.02 + half_disc, "{ba:?}");
        // BM: mirrored, px to the left: +y
        assert!(near(by("BM").y, (5.0, 5.3)), "{:?}", by("BM"));
        // BS: sloped from 4.5 to 5.5, the section staying vertical
        let bs = by("BS");
        assert!(near(bs.z, (4.5, 5.6)), "{bs:?}");
        let beams = loaded.layers.iter().find(|l| l.name == "Beams").expect("beams");
        let (_, first, count) = beams.elements[2];
        let at_end: Vec<f32> = beams.indices[first as usize..(first + count) as usize]
            .iter()
            .map(|&i| &beams.positions[3 * i as usize..3 * i as usize + 3])
            .filter(|p| (p[0] - 5.0).abs() < 1e-5)
            .map(|p| p[2])
            .collect();
        assert!(at_end.iter().all(|z| (z - 5.5).abs() < 1e-5 || (z - 5.6).abs() < 1e-5) && !at_end.is_empty());
        // BB: fewer fills than K: the box of the header
        let bb = by("BB");
        assert!(bb.block && near(bb.y, (14.95, 15.05)) && near(bb.z, (5.0, 5.2)), "{bb:?}");
        // KP: the L around (20, 20), from the story up 2 m
        let kp = by("KP");
        assert!(!kp.beam && near(kp.x, (20.0, 20.3)) && near(kp.y, (20.0, 20.3)) && near(kp.z, (4.5, 6.5)) && (kp.area - 0.05).abs() < 1e-9, "{kp:?}");
        // closed bodies facing out
        let columns = loaded.layers.iter().find(|l| l.name == "Columns").expect("columns");
        assert_eq!(beams.solid_indices().len(), beams.indices.len());
        assert_eq!(columns.solid_indices().len(), columns.indices.len());
        assert!(beams.props[1].1.contains("\"Profile\":\"mirrored\"") && beams.props[3].1.contains("drawn as its bounding box"));
    }

    #[test]
    fn reads_meshes_and_zones() {
        let (loaded, stats) = convert(read_bytes(&meshes_and_zones_file()).expect("container")).expect("elements");
        assert_eq!((stats.meshes.len(), stats.meshes_skipped, stats.meshes_without_cache, stats.mesh_bad_triangles), (2, 0, 1, 1));
        assert!(loaded.project.info.iter().any(|(k, v)| k == "Archicad meshes"
            && v == "2 read, 0 skipped; 1 from its outline, without cache; 1 cache triangles left out"));
        let terrain = loaded.layers.iter().find(|l| l.name == "Terrain").expect("terrain");
        assert!(!terrain.hidden);
        // every body closed and facing out
        assert_eq!(terrain.solid_indices().len(), terrain.indices.len());
        // M1: 4 triangles up, 4 down, 4 outer edges; its top at 4.5 + 0.2 + z, its bottom 1 below 4.7
        let hill = &stats.meshes[0];
        assert_eq!((hill.cached, hill.vertices, hill.triangles, hill.outer_edges), (true, 5, 4, 4));
        assert!((hill.top.0 - 4.7).abs() < 1e-9 && (hill.top.1 - 6.7).abs() < 1e-9 && (hill.bottom - 3.7).abs() < 1e-9, "{hill:?}");
        let (_, _, count) = terrain.elements[0];
        assert_eq!(count / 3, 4 + 4 + 4 * 2);
        // the volume: the 10 × 10 base 1 m deep, plus the pyramid of the cache over it
        let volume = |element: usize| {
            let (_, first, count) = terrain.elements[element];
            let p = |i: u32| glam::DVec3::from_array(std::array::from_fn(|k| terrain.positions[3 * i as usize + k] as f64));
            terrain.indices[first as usize..(first + count) as usize].chunks_exact(3).map(|t| p(t[0]).dot(p(t[1]).cross(p(t[2]))) / 6.0).sum::<f64>()
        };
        // the top over the base: the mean of z over each triangle, times its area (25 each)
        let top = 25.0 * ((0.0 + 0.0 + 2.0) + (0.0 + 1.0 + 2.0) + (1.0 + 1.0 + 2.0) + (1.0 + 0.0 + 2.0)) / 3.0;
        assert!((volume(0) - (100.0 + top)).abs() < 1e-3, "{}", volume(0));
        // M2: from its outline, each corner at its own height: two triangles up, two down, 4 sides
        let flat = &stats.meshes[1];
        assert_eq!((flat.cached, flat.vertices, flat.triangles, flat.outer_edges), (false, 4, 2, 4));
        assert!((flat.top.0 - 4.7).abs() < 1e-9 && (flat.top.1 - 5.7).abs() < 1e-9, "{flat:?}");
        assert!(terrain.props[1].1.contains("\"Surface\":\"from the outline (no cache)\"") && terrain.props[1].1.contains("\"Name\":\"Flat\""));

        // zones: Z1 with its hole and linked top, Z2 not linked, Z3 without a height
        assert_eq!((stats.zones.len(), stats.zones_skipped, stats.zones_no_height), (2, 1, 1));
        let zones = loaded.layers.iter().find(|l| l.name == "Zones").expect("zones");
        assert!(zones.hidden && zones.solid_indices().len() == zones.indices.len());
        let (hall, store) = (&stats.zones[0], &stats.zones[1]);
        assert_eq!((hall.number.as_str(), hall.name.as_str(), hall.holes, hall.linked), ("01", "Hall room", 1, true));
        assert!((hall.z.0 - 4.5).abs() < 1e-9 && (hall.z.1 - 7.2).abs() < 1e-9 && (hall.area - 23.0).abs() < 1e-9, "{hall:?}");
        assert!(!store.linked && (store.z.0 - 4.6).abs() < 1e-9 && (store.z.1 - 6.7).abs() < 1e-9, "{store:?}");
        for want in ["\"Name\":\"01 Hall room\"", "\"Number\":\"01\"", "\"Area\":\"23.00 m² (computed)\"", "linked to the story above"] {
            assert!(zones.props[0].1.contains(want), "{want} in {}", zones.props[0].1);
        }
        // the hole stays open: 8 triangles per face around it, 8 sides
        let (_, _, count) = zones.elements[0];
        assert_eq!(count / 3, 2 * 8 + 8 * 2);
    }

    #[test]
    fn draws_library_objects_as_boxes() {
        let records = read_bytes(&objects_file()).expect("container");
        // O6 has no story: a 2D symbol, not even counted
        assert_eq!(records.objects.len(), 6);
        let (loaded, stats) = convert(records).expect("objects");
        assert_eq!((stats.objects.len(), stats.objects_skipped, stats.objects_flat, stats.objects_ambiguous), (4, 2, 1, 1));
        assert!(loaded.project.info.iter().any(|(k, v)| k == "Archicad library objects"
            && v == "4 drawn as boxes (1 with an unknown height), 2 skipped (1 without a single place for their fields, 1 two-dimensional)"));
        let layer = &loaded.layers[0];
        assert_eq!((layer.name.as_str(), layer.hidden, layer.elements.len()), ("Objects", false, 4));
        assert_eq!(layer.solid_indices().len(), layer.indices.len());
        let by = |name: &str| stats.objects.iter().find(|o| o.name == name).unwrap_or_else(|| panic!("{name}: {:?}", stats.objects));
        let near = |p: (f64, f64), q: (f64, f64)| (p.0 - q.0).abs() < 1e-9 && (p.1 - q.1).abs() < 1e-9;
        // O1: A × B fits the cache: around its middle, not around the anchor at the corner
        let o = by("Table 26");
        assert!(o.shape == ObjectShape::Turned && near(o.x, (1.0, 3.0)) && near(o.y, (1.0, 2.0)) && near(o.z, (4.6, 5.4)), "{o:?}");
        // O2: the cache is larger than A × B: the cache
        let o = by("Sofa 26");
        assert!(o.shape == ObjectShape::Outline && near(o.x, (5.0, 7.0)) && near(o.y, (5.0, 6.5)) && near(o.z, (4.5, 5.2)), "{o:?}");
        // O3: no cache: 1 × 0.5 turned 90° around (10, 10); its parameter block comes first in the file
        let o = by("Lamp 26");
        assert!(o.shape == ObjectShape::Centred && near(o.x, (9.75, 10.25)) && near(o.y, (9.5, 10.5)) && near(o.z, (4.5, 5.4)), "{o:?}");
        // O4: no ZZYZX: 1 m high, and the properties say the height is unknown
        let o = by("Chair 26");
        assert!(!o.height_known && near(o.z, (4.5, 5.5)) && near(o.x, (11.75, 12.25)), "{o:?}");
        let props = &layer.props.iter().find(|p| p.1.contains("Chair 26")).unwrap().1;
        for want in ["\"Size\":\"500 × 500 × 1000 mm\"", "\"Height\":\"unknown (drawn 1000 mm high)\"", "\"level\":\"Hall\"", "\"category\":\"Object\""] {
            assert!(props.contains(want), "{want} in {props}");
        }
        assert!(layer.props.iter().find(|p| p.1.contains("Table 26")).unwrap().1.contains("\"Size\":\"2000 × 1000 × 800 mm\""));
    }

    #[test]
    fn cuts_windows_and_doors_from_walls() {
        let (loaded, stats) = convert(read_bytes(&openings_file()).expect("container")).expect("walls");
        let doors = stats.openings.iter().filter(|o| o.door).count();
        assert_eq!((doors, stats.openings.len() - doors), (2, 3));
        // N4 has no width, the third target of W4 has no record; W4 is drawn all the same
        assert_eq!((stats.openings_skipped, stats.openings_unreadable, stats.openings_missing, stats.walls), (2, 1, 1, 4));
        assert!(loaded.project.info.iter().any(|(k, v)| k == "Archicad openings" && v == "2 doors and 3 windows cut, 2 skipped (1 without a record, 1 whose fields do not read)"));
        let walls = &loaded.layers[0];
        assert_eq!(walls.name, "Walls");
        // every wall stays one closed body facing out, without the volume of its openings
        assert_eq!(walls.solid_indices().len(), walls.indices.len());
        let full = 4.0 * 0.3 * 2.7;
        let cut = |width: f64, height: f64| width * 0.3 * height;
        for (k, want) in [full - cut(0.8, 2.1), full - cut(1.0, 1.2), full - cut(0.8, 2.1) - cut(1.0, 1.0), full - cut(0.7, 1.0)].into_iter().enumerate() {
            assert!((volume(walls, k) - want).abs() < 1e-4, "wall {k}: {} not {want}", volume(walls, k));
        }
        assert!(walls.props[2].1.contains("\"Openings\":\"2\""));
        // the openings: hidden, through the wall, the door from the floor, the window from its sill
        let openings = loaded.layers.iter().find(|l| l.name == "Openings").expect("openings");
        assert!(openings.hidden);
        assert_eq!(openings.elements.len(), 5);
        let z = |element: usize| {
            let (_, first, count) = openings.elements[element];
            openings.indices[first as usize..(first + count) as usize]
                .iter()
                .map(|&i| openings.positions[3 * i as usize + 2])
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), v| (a.min(v), b.max(v)))
        };
        assert!((z(0).0 - 4.5).abs() < 1e-5 && (z(0).1 - 6.6).abs() < 1e-5, "{:?}", z(0));
        assert!((z(1).0 - 5.4).abs() < 1e-5 && (z(1).1 - 6.6).abs() < 1e-5, "{:?}", z(1));
        assert!((volume(openings, 1) - cut(1.0, 1.2)).abs() < 1e-4);
        let props = &openings.props[0].1;
        for want in ["\"Name\":\"D1\"", "\"Type\":\"Door\"", "\"Size\":\"800 × 2100 mm\"", "\"Sill\":\"0 mm\"", "\"Wall\":\"W1\"", "\"level\":\"Hall\""] {
            assert!(props.contains(want), "{want} in {props}");
        }
    }

    #[test]
    fn reads_roofs() {
        let (loaded, stats) = convert(read_bytes(&roofs_file()).expect("container")).expect("roofs");
        // R7 has thickness 0: a helper body (§8.16.2), skipped and counted, not an error
        assert_eq!((stats.roofs.len(), stats.roofs_skipped, stats.roofs_no_polygon, stats.roofs_unreadable, stats.roofs_zero_thickness), (4, 3, 2, 0, 1));
        assert!(loaded.project.info.iter().any(|(k, v)| k == "Archicad roofs" && v == "4 read, 3 skipped (2 without a polygon, 1 with thickness 0 (helper bodies))"));
        let roofs = &loaded.layers[0];
        assert_eq!((roofs.name.as_str(), roofs.elements.len()), ("Roofs", 4));
        // every body is closed and faces out
        assert_eq!(roofs.solid_indices().len(), roofs.indices.len());
        let (tan, up) = (30f64.to_radians().tan(), 0.25 / 30f64.to_radians().cos());
        // the corners of an element: (x, y) -> (lowest, highest z)
        let corners = |element: usize| {
            let (_, first, count) = roofs.elements[element];
            let mut out: Vec<([f32; 2], f32, f32)> = Vec::new();
            for &i in &roofs.indices[first as usize..(first + count) as usize] {
                let p = &roofs.positions[3 * i as usize..3 * i as usize + 3];
                match out.iter_mut().find(|c| (c.0[0] - p[0]).abs() < 1e-5 && (c.0[1] - p[1]).abs() < 1e-5) {
                    Some(c) => (c.1, c.2) = (c.1.min(p[2]), c.2.max(p[2])),
                    None => out.push(([p[0], p[1]], p[2], p[2])),
                }
            }
            out
        };
        let z_at = |element: usize, x: f32, y: f32| corners(element).into_iter().find(|c| c.0 == [x, y]).map(|c| (c.1 as f64, c.2 as f64)).unwrap();
        let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-5 && (a.1 - b.1).abs() < 1e-5;
        // R1: the underside on the pivot line at 4.5 + 0.2, rising to the left (+y)
        assert!(near(z_at(0, 0.0, 0.0), (4.7, 4.7 + up)) && near(z_at(0, 4.0, 3.0), (4.7 + 3.0 * tan, 4.7 + 3.0 * tan + up)));
        // R2: the plane right of the line rises to the right (−y)
        assert!(near(z_at(1, 4.0, 0.0), (4.7, 4.7 + up)) && near(z_at(1, 0.0, -3.0), (4.7 + 3.0 * tan, 4.7 + 3.0 * tan + up)));
        // the top of R1 faces up and away from its rise
        let (_, first, _) = roofs.elements[0];
        let top = roofs.indices[first as usize..].chunks_exact(3).map(|t| &roofs.normals[3 * t[0] as usize..3 * t[0] as usize + 3]).find(|n| n[2] > 0.5).unwrap();
        let s = 30f32.to_radians().sin_cos();
        assert!((top[0]).abs() < 1e-5 && (top[1] + s.0).abs() < 1e-5 && (top[2] - s.1).abs() < 1e-5, "{top:?}");
        // R3: flat from 4.7 up 0.3
        assert!(corners(2).iter().all(|c| (c.1 - 4.7).abs() < 1e-5 && (c.2 - 5.0).abs() < 1e-5));
        // R4: the hole stays open: 8 triangles per face around it, 8 sides
        let (_, _, count) = roofs.elements[3];
        assert_eq!(count / 3, 2 * 8 + 8 * 2);
        assert!(roofs.props[3].1.contains("\"Pitch\":\"20.00°\"") && roofs.props[3].1.contains("\"Thickness\":\"200 mm\""));
        assert_eq!(stats.roofs[0].z, (4.7, 4.7 + 3.0 * tan + up));
    }

    #[test]
    fn reads_columns_and_beams() {
        let records = read_bytes(&columns_and_beams_file()).expect("container");
        assert_eq!((records.columns.len(), records.beams.len(), records.segments.len(), records.sections.len()), (2, 4, 5, 4));
        let (loaded, stats) = convert(records).expect("elements");
        assert_eq!((stats.columns.len(), stats.columns_skipped), (2, 0));
        // B3 has a profile section, B4 no segment
        assert_eq!((stats.beams.len(), stats.beams_skipped, stats.beams_profile, stats.beams_no_section), (2, 2, 1, 1));
        let names: Vec<&str> = loaded.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Columns", "Beams"]);
        let (columns, beams) = (&loaded.layers[0], &loaded.layers[1]);
        assert!(loaded.project.info.iter().any(|(k, v)| k == "Archicad beams" && v == "2 read, 2 skipped (1 with a profile section, 1 without a section)"));

        // K1: the rectangle turned 30° around its middle, not around the anchor; linked: 7.5 - 0.3
        let (corners, z) = element_shape(columns, 0);
        let (s, c) = std::f64::consts::FRAC_PI_6.sin_cos();
        for (a, b) in [(-0.2, -0.1), (0.2, -0.1), (0.2, 0.1), (-0.2, 0.1)] {
            let want = [3.0 + c * a - s * b, 3.0 + s * a + c * b];
            assert!(corners.iter().any(|p| (p[0] as f64 - want[0]).abs() < 1e-5 && (p[1] as f64 - want[1]).abs() < 1e-5), "{want:?} in {corners:?}");
        }
        assert_eq!((corners.len(), z), (4, (4.5, 7.2)));
        assert!(stats.columns[0].linked && !stats.columns[1].linked);
        assert!(columns.props[0].1.contains("\"Section\":\"400 × 200 mm\"") && columns.props[0].1.contains("linked to the story above"));
        // K2: a circle of radius 0.15 around (6, 3); not linked: 4.5 + 0.1 up 2 m
        let (points, z) = element_shape(columns, 1);
        assert!(points.len() >= 16 && points.iter().all(|p| ((p[0] - 6.0).hypot(p[1] - 3.0) - 0.15).abs() < 1e-5), "{points:?}");
        assert_eq!(z, (4.6, 6.6));
        assert!(columns.props[1].1.contains("\"Section\":\"⌀ 300 mm\"") && columns.props[1].1.contains("\"Top\":\"not linked\""));

        // B1: s = 0.1 to the left of its axis, 0.2 wide; its top from 4.5 + 0.5 to 4.5 + 1.0, 0.4 high
        let (corners, z) = element_shape(beams, 0);
        assert_eq!(beams.triangle_count(), 2 * 12);
        let ys: Vec<f32> = corners.iter().map(|p| p[1]).collect();
        assert!(ys.iter().all(|y| (y - 6.0).abs() < 1e-5 || (y - 6.2).abs() < 1e-5), "{ys:?}");
        assert_eq!(z, (4.6, 5.5));
        let end_z = |x: f32| {
            let (_, first, count) = beams.elements[0];
            beams.indices[first as usize..(first + count) as usize]
                .iter()
                .map(|&i| &beams.positions[3 * i as usize..3 * i as usize + 3])
                .filter(|p| (p[0] - x).abs() < 1e-5)
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), p| (a.min(p[2]), b.max(p[2])))
        };
        assert_eq!((end_z(1.0), end_z(5.0)), ((4.6, 5.0), (5.1, 5.5)));
        assert_eq!(stats.beams[0].top, (5.0, 5.5));
        // B2: no axis with heights: horizontal at 4.5 + 0.8, centred on its axis
        let (corners, z) = element_shape(beams, 1);
        assert!(corners.iter().all(|p| (p[0] - 0.9).abs() < 1e-5 || (p[0] - 1.1).abs() < 1e-5), "{corners:?}");
        assert!((z.0 - 4.9).abs() < 1e-5 && (z.1 - 5.3).abs() < 1e-5, "{z:?}");
        // every face of the bodies points out: the cut fill takes them as closed
        assert!(!columns.solid_indices().is_empty() && beams.solid_indices().len() == beams.indices.len());
    }

    #[test]
    fn wall_rectangle_sides_and_heights() {
        let wall = |side: i32| WallFields {
            id: "W1".into(),
            line: [0.0, 0.0, 4.0, 0.0],
            side,
            thickness: [0.3, 0.3],
            offset: 0.0,
            top_offset: 0.0,
            height: 2.7,
            bottom_offset: 0.0,
        };
        let y = |side| wall_rectangle(&wall(side)).unwrap().iter().map(|p| p[1]).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), y| (a.min(y), b.max(y)));
        assert_eq!(y(1), (0.0, 0.3));
        assert_eq!(y(3), (-0.3, 0.0));
        assert_eq!(y(2), (-0.15, 0.15));
        // linked: the level of the linked story plus A (B is out of date); not linked: B
        assert_eq!(wall_heights(&WallFields { top_offset: -0.5, ..wall(1) }, 0.0, Some(3.0)), (0.0, 2.5));
        assert_eq!(wall_heights(&wall(1), 6.0, None), (6.0, 8.7));
    }
}
