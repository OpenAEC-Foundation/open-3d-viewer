//! Archicad projects (.pln) and archives (.pla) of Archicad 25 to 29: walls, slabs and columns as
//! extruded footprints, beams along their axis and roof planes, grouped per story. Written from a
//! clean-room specification of the file structure (SPEC v0.4, section numbers in the comments): a
//! container of 16-byte items with QuickLZ groups, object records with associations, stories, the
//! element polygon, the wall fields, columns and beams with their cross-sections, and roof planes.
//! Openings, library objects, curved roofs and profile cross-sections are not read yet.
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

// §5: the classes the reader uses, the same in Archicad 25 to 29.
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
/// §6: the unnamed story above the top story; its level counts, it is not shown.
const TOP_STORY: Guid = guid("f5c28e4f85caa148b54067746b76d419");

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
    data: Vec<u8>,
}

/// §8.7: the cross-section of a column or beam.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Section {
    /// Width b and depth d (of a beam: its height).
    Rect(f64, f64),
    /// Diameters b and d.
    Round(f64, f64),
    /// An own 2D profile: not read yet.
    Profile,
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
    /// Column and beam segments (§8.7): the first target of their section association.
    segments: HashMap<Guid, Option<Guid>>,
    sections: HashMap<Guid, Section>,
    /// Elements skipped before conversion: data larger than [`MAX_ELEMENT_DATA`].
    walls_too_large: usize,
    slabs_too_large: usize,
    columns_too_large: usize,
    beams_too_large: usize,
    roofs_too_large: usize,
    /// Blocks of the classes above whose payload could not be decoded, blocks larger than
    /// [`MAX_BLOCK`] and group chunks that did not decompress.
    damaged: usize,
}

/// Reads the file as a stream: projects run to hundreds of MB, mostly library content.
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
        let classes = [WALL, SLAB, STORY, STORY_LEVEL, LINK, COLUMN, BEAM, ROOF, COLUMN_SEGMENT, BEAM_SEGMENT, SECTION_RECT, SECTION_ROUND, SECTION_PROFILE];
        match payload(b, |class| classes.contains(class)) {
            Payload::Data(p) => self.record(object, &p),
            Payload::Damaged => self.damaged += 1,
            Payload::Other => {}
        }
    }

    /// §4: an object record of a class the reader uses.
    fn record(&mut self, object: Guid, p: &[u8]) {
        let Some(class) = guid_at(p, 0) else { return };
        let Some(data_len) = u32_at(p, 35).map(|n| n as usize) else { return };
        let Some(assoc_len) = 39usize.checked_add(data_len).and_then(|at| u32_at(p, at)).map(|n| n as usize) else { return };
        if 43usize.checked_add(data_len).and_then(|n| n.checked_add(assoc_len)) != Some(p.len()) {
            return; // not an element record
        }
        let data = &p[39..39 + data_len];
        let links = links(&p[43 + data_len..]);
        match class {
            WALL | SLAB | COLUMN | BEAM | ROOF => {
                let first = |assoc: Guid, op: &[u8]| links.iter().find(|l| op.contains(&l.0) && l.1 == assoc).map(|l| l.2);
                let story = first(ON_STORY, &[0x12]);
                let (top_link, segment) = match class {
                    WALL => (first(TOP_LINK, &[0x11, 0x12]), None),
                    COLUMN => (first(COLUMN_TOP_LINK, &[0x11, 0x12]), first(COLUMN_SEGMENT_LINK, &[0x11, 0x12])),
                    BEAM => (None, first(BEAM_SEGMENT_LINK, &[0x11, 0x12])),
                    _ => (None, None),
                };
                let (list, too_large) = match class {
                    WALL => (&mut self.walls, &mut self.walls_too_large),
                    SLAB => (&mut self.slabs, &mut self.slabs_too_large),
                    COLUMN => (&mut self.columns, &mut self.columns_too_large),
                    BEAM => (&mut self.beams, &mut self.beams_too_large),
                    _ => (&mut self.roofs, &mut self.roofs_too_large),
                };
                if data.len() > MAX_ELEMENT_DATA {
                    *too_large += 1;
                    return;
                }
                list.push(Record { guid: object, story, top_link, segment, data: data.to_vec() });
            }
            COLUMN_SEGMENT | BEAM_SEGMENT => {
                // §8.7: the first target; the op is not given, any is taken
                self.segments.insert(object, links.iter().find(|l| l.1 == SECTION_LINK).map(|l| l.2));
            }
            SECTION_RECT | SECTION_ROUND => {
                self.sections.insert(object, section(class, data));
            }
            SECTION_PROFILE => {
                self.sections.insert(object, Section::Profile);
            }
            STORY => {
                // §6: 10 bytes, u32 n, n UTF-16LE units of name, i16 index
                let Some(n) = u32_at(data, 10).map(|n| n as usize) else { return };
                let Some(name) = n.checked_mul(2).and_then(|b| data.get(14..14 + b)) else { return };
                let Some(index) = u16_at(data, 14 + 2 * n).map(|i| i as i16) else { return };
                let units: Vec<u16> = name.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                self.stories.insert(object, (String::from_utf16_lossy(&units), index));
            }
            LINK => {
                // §8.3: the op of this association is not given; any is taken
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
    /// Where it starts in the data (§8.8 reads fields before it).
    at: usize,
    contours: Vec<Vec<[f64; 2]>>,
    coords: usize,
    z: Option<(f64, f64)>,
}

/// §7: the first polygon in the data: a bounding box, the counts, the points (point 0 unused), the
/// contour ends and the arcs, valid when the points span exactly the box.
fn find_polygon(data: &[u8]) -> Option<Polygon> {
    if data.len() > MAX_ELEMENT_DATA {
        return None;
    }
    // values read for candidates past the cheap checks: a bound on the work for any data
    let mut budget = 1usize << 24;
    for i in 0..data.len().saturating_sub(43) {
        let Some(n) = u32_at(data, i + 32).map(|n| n as usize).filter(|n| (2..=20_000).contains(n)) else { continue };
        let Some(arcs) = u32_at(data, i + 36).map(|n| n as usize).filter(|&a| a <= n) else { continue };
        let Some(subs) = u32_at(data, i + 40).map(|n| n as usize).filter(|&s| (1..=n).contains(&s)) else { continue };
        let bbox = [0, 8, 16, 24].map(|k| f64_at(data, i + k).unwrap_or(f64::NAN));
        if !bbox.iter().all(|v| v.is_finite()) || bbox[0] > bbox[2] || bbox[1] > bbox[3] {
            continue;
        }
        let points_at = i + 44;
        let ends_at = points_at + (n + 1) * 16;
        let end = ends_at + (subs + 1) * 4 + arcs * 16;
        if end > data.len() {
            continue;
        }
        // the first and last contour end before the rest: most offsets fail here
        let end_at = |k: usize| u32_at(data, ends_at + 4 * k).unwrap() as usize;
        if end_at(0) != 0 || end_at(subs) != n {
            continue;
        }
        budget = budget.checked_sub(subs + 2 * n)?;
        let ends: Vec<usize> = (0..=subs).map(end_at).collect();
        if ends.windows(2).any(|w| w[0] > w[1]) {
            continue;
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
        if !(close(min[0], bbox[0]) && close(min[1], bbox[1]) && close(max[0], bbox[2]) && close(max[1], bbox[3])) {
            continue;
        }
        // contour k: points ends[k]+1 ..= ends[k+1]; the last repeats the first. Arcs (none in the
        // samples) are drawn straight.
        let contours = (0..subs)
            .map(|k| {
                let mut c = points[ends[k] + 1..=ends[k + 1]].to_vec();
                if c.len() > 1 && c.first() == c.last() {
                    c.pop();
                }
                c
            })
            .collect();
        return Some(Polygon { at: i, contours, coords: n, z: z_cache(data, i, end) });
    }
    None
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
    /// with its heights is not found: horizontal).
    top: (f64, f64),
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
        let beam = |top: (f64, f64)| BeamFields { id: id.clone(), offset: f[0], line: [f[2], f[3], f[4], f[5]], top };
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
    (finite && thickness > 0.0 && thickness < 5.0 && (0.0..90.0).contains(&pitch)).then_some(RoofFields { id, pivot_height, pitch, line, thickness })
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
    beams_profile: usize,
    beams_no_section: usize,
    roofs: Vec<RoofOut>,
    /// Roof planes skipped (§8.8): all of them, those without a polygon (or one too near the
    /// start of the data), and those whose fields do not read.
    roofs_skipped: usize,
    roofs_no_polygon: usize,
    roofs_unreadable: usize,
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
        Section::Profile => Err(NoSection::Profile),
        Section::Unreadable => Err(NoSection::Missing),
        s => Ok(s),
    }
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
        let drawn = footprint.is_some_and(|contours| {
            let prism = Prism { contours, z };
            add(&mut walls, record, z, &|layer| push_prism(layer, &prism), props)
        });
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

    if stats.walls + stats.slabs + stats.columns.len() + stats.beams.len() + stats.roofs.len() == 0 {
        // §10: Archicad 17 to 24 share the container but not the records
        return Err(format!(
            "Archicad: no walls, slabs, columns, beams or roofs found ({} walls, {} slabs, {} columns, {} beams and {} roofs \
             skipped). This reader supports Archicad 25 to 29; projects from Archicad 17 to 24 are not supported yet.",
            stats.walls_skipped, stats.slabs_skipped, stats.columns_skipped, stats.beams_skipped, stats.roofs_skipped
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
        ("Archicad roofs".into(), roof_text(&stats)),
        (
            "Archicad elements not read yet".into(),
            "Openings, meshes, morphs, library objects, curved roofs and shells, and columns and beams with a profile section"
                .into(),
        ),
    ];
    if records.damaged > 0 {
        info.push(("Archicad damaged blocks".into(), records.damaged.to_string()));
    }
    let project = ProjectData {
        info,
        levels: stories.iter().map(|(g, _, z)| (names[*g].clone(), *z as f32)).collect(),
        ..ProjectData::default()
    };
    let layers = [walls, slabs, columns, beams, roofs].into_iter().filter(|l| !l.indices.is_empty()).collect();
    Ok((Loaded { layers, views: Vec::new(), project }, stats))
}

/// "2 read, 1 skipped (1 without a polygon)".
fn roof_text(stats: &Stats) -> String {
    let mut out = format!("{} read, {} skipped", stats.roofs.len(), stats.roofs_skipped);
    let why: Vec<String> = [(stats.roofs_no_polygon, "without a polygon"), (stats.roofs_unreadable, "with fields that do not read")]
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
    // clean contours: no repeated points, at least 3, outer counter-clockwise, holes clockwise
    let mut contours: Vec<Vec<[f64; 2]>> = Vec::new();
    for (k, c) in footprint.iter().enumerate() {
        let mut clean: Vec<[f64; 2]> = Vec::with_capacity(c.len());
        for p in c {
            if !p.iter().all(|v| v.is_finite()) {
                return false;
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
                return false;
            }
            continue;
        }
        if (a > 0.0) != (k == 0) {
            clean.reverse();
        }
        contours.push(clean);
    }
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
        // 7 slabs drawn and 1 without polygon skipped: the y range up to 12.698 needs the seventh
        assert_eq!((stats.slabs, stats.slabs_skipped), (7, 1));
        check_stories(&stats, &model);
        let b = walls_and_slabs_bounds(&model);
        assert!(near(b.min[0], 1.412) && near(b.max[0], 11.429) && near(b.min[1], 1.076) && near(b.max[1], 12.698), "{b:?}");
        assert!(near(b.min[2], -0.31) && near(model.bounds.min[2], -0.31), "{b:?}");
        check_roofs(
            &stats,
            &[
                ("Roo 2.01", 11.39, 0.271, [1.827, 11.025], [6.108, 12.306], [7.482, 9.611]),
                ("SEO 1.01", 11.393, 0.3, [11.023, 11.219], [5.838, 12.582], [7.35, 7.695]),
            ],
        );
        check_walls(&stats, &model, 32, 24, 7.588, -0.272);
        // §11 (v0.3): two round columns, linked; the four beams have a profile section
        check_columns(&stats, &[("C 0.01", [11.102, 12.373]), ("C 0.02", [11.102, 6.023])], (-0.25, 2.78));
        assert_eq!((stats.beams.len(), stats.beams_skipped, stats.beams_profile), (0, 4, 4));
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
        // the container with nothing in it: no elements, as an Archicad 17-24 file gives
        let mut empty = b"ROF FDB ".to_vec();
        empty.resize(64, 0);
        assert!(load(&empty).contains("Archicad 25 to 29"));
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

    fn read_bytes(file: &[u8]) -> Result<Records, String> {
        read(Cursor::new(file), file.len() as u64)
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
        for file in [synthetic_file(), columns_and_beams_file(), roofs_file()] {
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

    #[test]
    fn reads_roofs() {
        let (loaded, stats) = convert(read_bytes(&roofs_file()).expect("container")).expect("roofs");
        assert_eq!((stats.roofs.len(), stats.roofs_skipped, stats.roofs_no_polygon, stats.roofs_unreadable), (4, 3, 2, 1));
        assert!(loaded.project.info.iter().any(|(k, v)| k == "Archicad roofs" && v == "4 read, 3 skipped (2 without a polygon, 1 with fields that do not read)"));
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
