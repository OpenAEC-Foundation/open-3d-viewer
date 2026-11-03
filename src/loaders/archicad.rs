//! Archicad projects (.pln) and archives (.pla) of Archicad 25 to 29: walls and slabs as extruded
//! footprints, grouped per story. Written from a clean-room specification of the file structure
//! (SPEC v0.1, section numbers in the comments): a container of 16-byte items with QuickLZ
//! groups, object records with associations, stories, the element polygon and the wall fields.
//! Columns, beams, roofs, openings and library objects are not read yet.
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

/// An element record (wall or slab): its identity, story and data.
struct Record {
    guid: Guid,
    story: Option<Guid>,
    data: Vec<u8>,
}

/// What the reader keeps of the file: the records of the classes it uses.
#[derive(Default)]
struct Records {
    /// Story: name and index (0 the ground floor, negative below).
    stories: HashMap<Guid, (String, i16)>,
    /// Story levels: the targets of their 0x12 associations and the elevation (m).
    levels: Vec<(Vec<Guid>, f64)>,
    walls: Vec<Record>,
    slabs: Vec<Record>,
    /// Walls and slabs skipped before conversion: data larger than [`MAX_ELEMENT_DATA`].
    walls_too_large: usize,
    slabs_too_large: usize,
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
        match payload(b, |class| [WALL, SLAB, STORY, STORY_LEVEL].contains(class)) {
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
            WALL | SLAB => {
                let story = links.iter().find(|(assoc, _)| *assoc == ON_STORY).map(|(_, target)| *target);
                if data.len() > MAX_ELEMENT_DATA {
                    *(if class == WALL { &mut self.walls_too_large } else { &mut self.slabs_too_large }) += 1;
                    return;
                }
                let record = Record { guid: object, story, data: data.to_vec() };
                if class == WALL {
                    self.walls.push(record);
                } else {
                    self.slabs.push(record);
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
            _ => {
                // §6: the level is the last f64 of the data
                let Some(z) = data.len().checked_sub(8).and_then(|at| f64_at(data, at)).filter(|z| z.is_finite()) else { return };
                self.levels.push((links.into_iter().map(|(_, target)| target).collect(), z));
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

/// §4.1: the associations of a record, as (association, target) for the op 0x12 items, the only
/// ones the reader uses.
fn links(a: &[u8]) -> Vec<(Guid, Guid)> {
    let mut out = Vec::new();
    let mut at = 2;
    while let Some(&op) = a.get(at) {
        at += 1;
        match op {
            0x11 | 0x12 => {
                let (Some(assoc), Some(target)) = (guid_at(a, at), guid_at(a, at + 16)) else { break };
                if op == 0x12 {
                    out.push((assoc, target));
                }
                at += 32;
            }
            0x01 | 0x02 => {
                let Some(k) = u32_at(a, at + 16).map(|k| k as usize) else { break };
                let Some(next) = k.checked_mul(16).and_then(|n| n.checked_add(at + 20)).filter(|&n| n <= a.len()) else { break };
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
        return Some(Polygon { contours, coords: n, z: z_cache(data, i, end) });
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

/// §8.2: the end q of the ID string is the smallest i ≥ 40 with `01 00` before it, a u32 length
/// n ≤ 64 at i and n printable ASCII characters in UTF-16LE after it, and six small non-zero i32
/// at q+32.
fn wall_fields(data: &[u8]) -> Option<WallFields> {
    for i in 40..data.len() {
        if data[i - 2..i] != [1, 0] {
            continue;
        }
        let Some(n) = u32_at(data, i).map(|n| n as usize).filter(|&n| n <= 64) else { continue };
        let Some(text) = bytes_at(data, i + 4, 2 * n) else { continue };
        if !text.chunks_exact(2).all(|c| (0x20..=0x7E).contains(&c[0]) && c[1] == 0) {
            continue;
        }
        let q = i + 4 + 2 * n;
        // every field must be in the data
        if q + 169 > data.len() {
            continue;
        }
        let s: Vec<i32> = (0..6).map(|k| i32_at(data, q + 32 + 4 * k).unwrap()).collect();
        if !s.iter().all(|&v| v != 0 && (-8..=8).contains(&v)) {
            continue;
        }
        let f = |at: usize| f64_at(data, q + at).unwrap();
        return Some(WallFields {
            id: text.chunks_exact(2).map(|c| c[0] as char).collect(),
            line: [f(0), f(8), f(16), f(24)],
            side: s[0],
            thickness: [f(77), f(85)],
            offset: f(93),
            top_offset: f(145),
            height: f(153),
            bottom_offset: f(161),
        });
    }
    None
}

/// An element ready to draw: footprint contours (outer first) extruded from `z.0` to `z.1`.
struct Prism {
    contours: Vec<Vec<[f64; 2]>>,
    z: (f64, f64),
}

/// §8.3: wall bottom and top from the story levels, when the polygon caches none. `e` is the
/// level of the wall's own story, `levels` all story levels, low to high. A heuristic: the file's
/// flag for a top linked to a story is not known (open question Q-A1).
fn wall_heights(f: &WallFields, e: f64, levels: &[f64]) -> (f64, f64) {
    let bottom = e + f.bottom_offset;
    let top = levels
        .iter()
        .filter(|&&l| l > e)
        .take(3)
        .map(|l| l + f.top_offset)
        .find(|&top| top > bottom + 0.05)
        .unwrap_or(bottom + if f.height > 0.0 { f.height } else { 3.0 });
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
    slabs: usize,
    slabs_skipped: usize,
    /// The stories shown: name as in the file and level, low to high.
    stories: Vec<(String, f64)>,
    /// All story levels (§6), with the one above the top story.
    levels: Vec<f64>,
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
    let mut next_id = 0u64;
    let mut add = |layer: &mut Layer, record: &Record, prism: &Prism, mut props: serde_json::Map<String, serde_json::Value>| -> bool {
        let first = layer.indices.len() as u32;
        if !push_prism(layer, prism) {
            return false;
        }
        next_id += 1;
        layer.elements.push((next_id, first, layer.indices.len() as u32 - first));
        props.insert("GUID".into(), hex(&record.guid).into());
        if let Some(level) = record.story.and_then(|s| names.get(&s)) {
            props.insert("level".into(), level.clone().into());
        }
        props.insert("Bottom elevation".into(), format!("{:.0} mm", prism.z.0 * 1000.0).into());
        props.insert("Top elevation".into(), format!("{:.0} mm", prism.z.1 * 1000.0).into());
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
        let z = stored.unwrap_or_else(|| {
            let e = record.story.and_then(|s| level_of.get(&s)).copied().unwrap_or(0.0);
            wall_heights(&f, e, &levels)
        });
        let footprint = match polygon {
            Some(p) if has_polygon => Some(p.contours),
            _ => wall_rectangle(&f).map(|r| vec![r]),
        };
        let mut props = serde_json::Map::new();
        props.insert("Name".into(), (if f.id.is_empty() { "Wall" } else { &f.id }).into());
        props.insert("category".into(), "Wall".into());
        let [t0, t1] = f.thickness.map(|t| (t * 1000.0).round());
        props.insert("Thickness".into(), (if t0 == t1 { format!("{t0:.0} mm") } else { format!("{t0:.0}–{t1:.0} mm") }).into());
        if stored.is_none() {
            props.insert("Height".into(), "estimated from the story levels".into());
        }
        let drawn = footprint.is_some_and(|contours| add(&mut walls, record, &Prism { contours, z }, props));
        if drawn {
            stats.walls += 1;
            stats.walls_polygon += usize::from(has_polygon);
            stats.walls_stored_heights += usize::from(stored.is_some());
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
            add(&mut slabs, record, &prism, props)
        });
        if drawn {
            stats.slabs += 1;
        } else {
            stats.slabs_skipped += 1;
        }
    }

    if stats.walls + stats.slabs == 0 {
        // §10: Archicad 17 to 24 share the container but not the records
        return Err(format!(
            "Archicad: no walls or slabs found ({} walls and {} slabs skipped). This reader supports Archicad 25 to 29; \
             projects from Archicad 17 to 24 are not supported yet.",
            stats.walls_skipped, stats.slabs_skipped
        ));
    }

    let mut info = vec![
        ("Archicad walls".into(), format!("{} read, {} skipped", stats.walls, stats.walls_skipped)),
        ("Archicad slabs".into(), format!("{} read, {} skipped", stats.slabs, stats.slabs_skipped)),
        (
            "Archicad wall heights".into(),
            format!("{} stored in the file, {} estimated from the story levels", stats.walls_stored_heights, stats.walls - stats.walls_stored_heights),
        ),
        ("Archicad elements not read yet".into(), "Columns, beams, roofs, openings, meshes, morphs and library objects".into()),
    ];
    if records.damaged > 0 {
        info.push(("Archicad damaged blocks".into(), records.damaged.to_string()));
    }
    let project = ProjectData {
        info,
        levels: stories.iter().map(|(g, _, z)| (names[*g].clone(), *z as f32)).collect(),
        ..ProjectData::default()
    };
    let layers = [walls, slabs].into_iter().filter(|l| !l.indices.is_empty()).collect();
    Ok((Loaded { layers, views: Vec::new(), project }, stats))
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
    if !z0.is_finite() || !z1.is_finite() || z1 - z0 <= 1e-6 {
        return false;
    }
    // clean contours: no repeated points, at least 3, outer counter-clockwise, holes clockwise
    let mut contours: Vec<Vec<[f64; 2]>> = Vec::new();
    for (k, c) in prism.contours.iter().enumerate() {
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
    let vertex = |layer: &mut Layer, p: [f64; 2], z: f64, n: [f32; 3]| {
        layer.positions.extend([p[0] as f32, p[1] as f32, z as f32]);
        layer.normals.extend(n);
        (layer.positions.len() / 3 - 1) as u32
    };
    for t in triangles.chunks_exact(3) {
        let (a, b, c) = (points[t[0]], points[t[1]], points[t[2]]);
        let ccw = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) > 0.0;
        // top counter-clockwise from above, bottom clockwise
        let (a, b) = if ccw { (a, b) } else { (b, a) };
        let top = [a, b, c].map(|p| vertex(layer, p, z1, [0.0, 0.0, 1.0]));
        layer.indices.extend(top);
        let bottom = [a, c, b].map(|p| vertex(layer, p, z0, [0.0, 0.0, -1.0]));
        layer.indices.extend(bottom);
    }
    for c in &contours {
        for i in 0..c.len() {
            let (p, q) = (c[i], c[(i + 1) % c.len()]);
            let length = (q[0] - p[0]).hypot(q[1] - p[1]);
            // outwards: right of the edge (outer counter-clockwise, holes clockwise)
            let n = [((q[1] - p[1]) / length) as f32, (-(q[0] - p[0]) / length) as f32, 0.0];
            let quad = [(p, z0), (q, z0), (q, z1), (p, z1)].map(|(p, z)| vertex(layer, p, z, n));
            layer.indices.extend([quad[0], quad[1], quad[2], quad[0], quad[2], quad[3]]);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Model;
    use std::path::PathBuf;
    use std::sync::Arc;

    /// The folder with the sample projects `CH04_Part2_Walls.pln` (Archicad 25) and `DEMO.pln`
    /// (Archicad 26) of PacktPublishing/A-BIM-Professionals-Guide-to-Learning-Archicad (MIT), from
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

    /// Walls stand between the lowest slab and a few stories up (§8.3 is a heuristic).
    fn check_walls(model: &Model) {
        for (bottom, top) in z_ranges(model, "Walls") {
            assert!(bottom >= -0.5 && top > bottom + 0.05 && top <= 12.0, "wall from {bottom} to {top}");
        }
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
        let b = model.bounds;
        assert!(near(b.min[0], 1.011) && near(b.max[0], 11.011) && near(b.min[1], 1.441) && near(b.max[1], 12.441), "{b:?}");
        assert!(near(b.min[2], -0.2), "{b:?}");
        check_walls(&model);
        // the walls are on their stories, for the explorer and the exploded view
        assert!(model.props.values().filter(|p| p.contains("\"category\":\"Wall\"")).all(|p| p.contains("\"level\":")));
        assert_eq!(model.layers.iter().map(|l| l.elements.len()).sum::<usize>(), 23);
    }

    /// The expected values of the specification (§11) for `DEMO.pln`.
    #[test]
    fn sample_demo_archicad_26() {
        let Some((stats, model)) = open("DEMO.pln") else { return };
        assert_eq!((stats.walls, stats.walls_skipped, stats.walls_polygon), (56, 0, 54));
        // 7 slabs drawn and 1 without polygon skipped: the y range up to 12.698 needs the seventh
        assert_eq!((stats.slabs, stats.slabs_skipped), (7, 1));
        check_stories(&stats, &model);
        let b = model.bounds;
        assert!(near(b.min[0], 1.412) && near(b.max[0], 11.429) && near(b.min[1], 1.076) && near(b.max[1], 12.698), "{b:?}");
        assert!(near(b.min[2], -0.31), "{b:?}");
        check_walls(&model);
        assert!(model.props.values().any(|p| p.contains("\"Name\":\"") && p.contains("\"category\":\"Wall\"")));
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

    /// Wall data (§8.2): ID "W1", a 4 m reference line, 0.3 m thick, 2.7 m high, no polygon.
    fn wall_data() -> Vec<u8> {
        let mut data = vec![0u8; 38];
        data.extend([1, 0]);
        data.extend(2u32.to_le_bytes());
        data.extend("W1".encode_utf16().flat_map(u16::to_le_bytes));
        let q = data.len();
        data.extend([1.0f64, 1.0, 5.0, 1.0].iter().flat_map(|v| v.to_le_bytes()));
        data.extend([1i32; 6].iter().flat_map(|v| v.to_le_bytes()));
        data.resize(q + 169, 0);
        for (at, v) in [(77, 0.3f64), (85, 0.3), (93, 0.0), (145, 0.0), (153, 2.7), (161, 0.0)] {
            data[q + at..q + at + 8].copy_from_slice(&v.to_le_bytes());
        }
        data
    }

    /// A whole project: a wall (0x85) in the outer layer, then a group in two chunks with a story
    /// ("Hall", index 2, 0x84), free space, its level (4.5 m, 0x86, across the chunk boundary)
    /// and a slab (0x84).
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
        let mut file = b"ROF FDB ".to_vec();
        file.resize(32, 0);
        let wall = record(WALL, &wall_data(), &link(ON_STORY, STORY_ID));
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
        assert_eq!(records.levels, [(vec![STORY_ID], 4.5)]);
        assert_eq!((records.walls.len(), records.slabs.len(), records.damaged), (1, 1, 0));
        let (loaded, stats) = convert(records).expect("elements");
        assert_eq!((stats.walls, stats.slabs), (1, 1));
        assert_eq!(loaded.project.levels, [("Hall".to_string(), 4.5)]);
        // the wall: left of its reference line, from its story at 4.5 m up 2.7 m (no story above)
        let walls = &loaded.layers[0];
        let x: Vec<f32> = walls.positions.chunks_exact(3).map(|p| p[0]).collect();
        let y: Vec<f32> = walls.positions.chunks_exact(3).map(|p| p[1]).collect();
        let z: Vec<f32> = walls.positions.chunks_exact(3).map(|p| p[2]).collect();
        let range = |v: &[f32]| v.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &v| (a.min(v), b.max(v)));
        assert_eq!((range(&x), range(&y), range(&z)), ((1.0, 5.0), (1.0, 1.3), (4.5, 7.2)));
        assert!(walls.props[0].1.contains("\"Name\":\"W1\"") && walls.props[0].1.contains("\"level\":\"Hall\""));
        assert_eq!(loaded.layers[1].triangle_count(), 2 * 8 + 8 * 2);
    }

    /// Every prefix of a valid file, random bytes behind a valid signature and random changes to a
    /// valid file: an answer (model or error) every time, quickly, without a panic.
    #[test]
    fn truncated_or_garbage_input_does_not_panic() {
        let file = synthetic_file();
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
            let mut changed = file.clone();
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
        // up to the next story; on the top story the stored height
        assert_eq!(wall_heights(&wall(1), 0.0, &[0.0, 3.0, 6.0]), (0.0, 3.0));
        assert_eq!(wall_heights(&wall(1), 6.0, &[0.0, 3.0, 6.0]), (6.0, 8.7));
    }
}
