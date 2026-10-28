//! The SketchUp 2013-2020 container: an MFC `CArchive`. Objects are written in order, each as
//! a class tag (a new class with its name, or a reference to a class slot) followed by the
//! class's own fields; later references to an object are its slot number. The walker reads the
//! materials, the tags, the component definitions and the root entity list, numbering slots the
//! way the writer did. The slot numbering does not start at zero: it is found from the file
//! (see [`walk`]).

use super::{
    find_bytes, u16_at, u32_at, Definition, Face, Instance, Material, Matrix, Parsed, Tag, DEFAULT_TAG,
    DEFAULT_TAG_COLOR,
};
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug)]
pub(super) enum Error {
    /// The bytes do not read as the expected record.
    Parse(String),
    /// A read ran past the end of the file.
    Eof,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Parse(m) => write!(f, "{m}"),
            Error::Eof => write!(f, "unexpected end of file"),
        }
    }
}

type Res<T> = Result<T, Error>;

const STR_MARKER: [u8; 3] = [0xFF, 0xFE, 0xFF];
const MAX_V20_FILLER_PAD: usize = 29;

/// SketchUp 3 predates several fields every later file carries (no attribute container on
/// vertices, loops, edge-uses, materials and layers; a 5-byte draw block; no layer count).
const FIRST_V4: u32 = 4;
/// The texture image is a CDib object from SketchUp 6 on; 3 to 5 write it inline.
const FIRST_DIB_OBJECT: u32 = 6;
/// The first version whose instances and groups always carry a name string.
const FIRST_NAMED_INSTANCE: u32 = 5;

/// The classes the walker can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Cls {
    Vertex,
    Edge,
    Curve,
    ArcCurve,
    EdgeUse,
    Loop,
    Face,
    Layer,
    Material,
    Dib,
    AttributeContainer,
    AttributeNamed,
    Camera,
    Thumbnail,
    Relationship,
    ComponentDefinition,
    Image,
    ComponentInstance,
    Group,
    FaceTextureCoords,
    ConstructionLine,
    ConstructionPoint,
    SectionPlane,
    SkFont,
    DimensionLinear,
    Text,
}

impl Cls {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "CVertex" => Cls::Vertex,
            "CEdge" => Cls::Edge,
            "CCurve" => Cls::Curve,
            "CArcCurve" => Cls::ArcCurve,
            "CEdgeUse" => Cls::EdgeUse,
            "CLoop" => Cls::Loop,
            "CFace" => Cls::Face,
            "CLayer" => Cls::Layer,
            "CMaterial" => Cls::Material,
            "CDib" => Cls::Dib,
            "CAttributeContainer" => Cls::AttributeContainer,
            "CAttributeNamed" => Cls::AttributeNamed,
            "CCamera" => Cls::Camera,
            "CThumbnail" => Cls::Thumbnail,
            "CRelationship" => Cls::Relationship,
            "CComponentDefinition" => Cls::ComponentDefinition,
            "CImage" => Cls::Image,
            "CComponentInstance" => Cls::ComponentInstance,
            "CGroup" => Cls::Group,
            "CFaceTextureCoords" => Cls::FaceTextureCoords,
            "CConstructionLine" => Cls::ConstructionLine,
            "CConstructionPoint" => Cls::ConstructionPoint,
            "CSectionPlane" => Cls::SectionPlane,
            "CSkFont" => Cls::SkFont,
            "CDimensionLinear" => Cls::DimensionLinear,
            "CText" => Cls::Text,
            _ => return None,
        })
    }
}

/// The common drawing-element fields.
#[derive(Debug, Clone, Copy)]
struct DrawBase {
    material: u16,
    hidden: u8,
    layer: u16,
}

type EdgeUse = (Option<u32>, u8);

/// An entity list: slot and value per object.
type Entities = Vec<(Option<u32>, Option<Rc<Val>>)>;

/// What the walker keeps of an object.
#[derive(Debug)]
enum Val {
    Vertex([f64; 3]),
    Edge { v1: Option<u32>, v2: Option<u32> },
    EdgeUse(EdgeUse),
    Loop(Vec<EdgeUse>),
    Face { db: DrawBase, plane: [f64; 4], loops: Vec<Vec<EdgeUse>>, back_material: u16 },
    Attributes(Vec<Option<Rc<Val>>>),
    Dictionary { name: String, entries: Vec<(String, String)> },
    Layer { name: String, hidden: u8, rgba: [u8; 4] },
    Material { name: String, rgba: [u8; 4], opacity: f64, use_opacity: u8 },
    Dib,
    Definition { name: String, ents: Entities },
    Image { db: DrawBase, def: Option<u32>, xform: [f64; 12] },
    Instance { db: DrawBase, def: Option<u32>, xf: [f64; 13], name: String, attrs: Option<Rc<Val>> },
    Other,
}

enum Slot {
    Class { cls: Option<Cls>, name: String },
    Object { cls: Option<Cls>, val: Option<Rc<Val>> },
}

/// What a read object or reference resolved to.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Null,
    /// A slot below the walk's start (written before the model).
    Premodel,
    /// A slot the writer reserved without writing an object.
    Reserved,
    Object(Option<Cls>),
}

struct ObjRef {
    slot: Option<u32>,
    kind: Kind,
    val: Option<Rc<Val>>,
}

const NULL_REF: ObjRef = ObjRef { slot: None, kind: Kind::Null, val: None };

#[derive(Clone, Copy, PartialEq)]
enum Owner {
    Root,
    Definition,
}

struct Archive<'a> {
    data: &'a [u8],
    pos: usize,
    ver: u32,
    has_pid: bool,
    slots: HashMap<u32, Slot>,
    class_slot: HashMap<Cls, u32>,
    class_schema: HashMap<Cls, u16>,
    current_class: Option<Cls>,
    /// The slot of the object being read.
    current_obj_slot: Option<u32>,
    next_slot: u32,
    walk_base: u32,
    current_loop: Option<u32>,
    /// Store-map indices the writer used without writing bytes (an annotation's connection
    /// points): (file band start, width). File references past a band are shifted back.
    burns: Vec<(u32, u32)>,
    cum_delta: u32,
    /// The slot right after the last annotation record; only there can a band start.
    annot_watermark: Option<u32>,
    /// Burn count per open entity list.
    burn_stack: Vec<u32>,
    /// Bytes after a construction line, detected once per file.
    cline_tail: Option<usize>,
}

fn parse_err<T>(msg: impl Into<String>) -> Res<T> {
    Err(Error::Parse(msg.into()))
}

impl<'a> Archive<'a> {
    fn new(data: &'a [u8], ver: u32, base: u32) -> Self {
        Self {
            data,
            pos: 0,
            ver,
            has_pid: ver >= 17,
            slots: HashMap::new(),
            class_slot: HashMap::new(),
            class_schema: HashMap::new(),
            current_class: None,
            current_obj_slot: None,
            next_slot: base,
            walk_base: base,
            current_loop: None,
            burns: Vec::new(),
            cum_delta: 0,
            annot_watermark: None,
            burn_stack: Vec::new(),
            cline_tail: None,
        }
    }

    // --- primitive reads ---

    fn raw(&mut self, n: usize) -> Res<&'a [u8]> {
        let v = self.data.get(self.pos..self.pos + n).ok_or(Error::Eof)?;
        self.pos += n;
        Ok(v)
    }

    fn u8(&mut self) -> Res<u8> {
        Ok(self.raw(1)?[0])
    }

    fn u16(&mut self) -> Res<u16> {
        let b = self.raw(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Res<u32> {
        let b = self.raw(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn i32(&mut self) -> Res<i32> {
        Ok(self.u32()? as i32)
    }

    fn f64(&mut self) -> Res<f64> {
        Ok(f64::from_le_bytes(self.raw(8)?.try_into().unwrap()))
    }

    fn f64s<const N: usize>(&mut self) -> Res<[f64; N]> {
        let mut out = [0.0; N];
        for v in &mut out {
            *v = self.f64()?;
        }
        Ok(out)
    }

    fn peek(&self, n: usize) -> &'a [u8] {
        &self.data[self.pos.min(self.data.len())..(self.pos + n).min(self.data.len())]
    }

    fn peek_u16(&self) -> Res<u16> {
        let b = self.data.get(self.pos..self.pos + 2).ok_or(Error::Eof)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn at_string(&self) -> bool {
        self.peek(3) == STR_MARKER
    }

    /// A string record: marker, length (u8, or 0xFF then u16, or 0xFFFF then u32), UTF-16LE.
    fn utf16(&mut self) -> Res<String> {
        if !self.at_string() {
            return parse_err(format!("expected a string record {}", self.ctx()));
        }
        self.pos += 3;
        let mut n = self.u8()? as usize;
        if n == 0xFF {
            n = self.u16()? as usize;
            if n == 0xFFFF {
                n = self.u32()? as usize;
            }
        }
        let bytes = self.raw(2 * n)?;
        let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        Ok(String::from_utf16_lossy(&units))
    }

    fn ctx(&self) -> String {
        let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02X}")).collect::<String>();
        let p = self.pos.min(self.data.len());
        format!("@0x{:x}: ...{} | {}...", self.pos, hex(&self.data[p.saturating_sub(16)..p]), hex(self.peek(32)))
    }

    // --- objects ---

    fn alloc(&mut self, slot: Slot) -> u32 {
        let s = self.next_slot;
        self.slots.insert(s, slot);
        self.next_slot += 1;
        s
    }

    fn read_object(&mut self, expect: Option<Cls>) -> Res<ObjRef> {
        let tag = self.u16()?;
        if tag == 0 {
            return Ok(NULL_REF);
        }
        if tag == 0x7FFF {
            let big = self.u32()?;
            if big & 0x8000_0000 != 0 {
                return self.new_of_class(big & 0x7FFF_FFFF, expect);
            }
            return self.backref(big);
        }
        if tag == 0xFFFF {
            let schema = self.u16()?;
            let len = self.u16()? as usize;
            if len > 40 {
                return parse_err(format!("implausible class name length {}", self.ctx()));
            }
            let name = String::from_utf8_lossy(self.raw(len)?).into_owned();
            let cls = Cls::from_name(&name);
            let s = self.alloc(Slot::Class { cls, name: name.clone() });
            if let Some(c) = cls {
                self.class_slot.insert(c, s);
                self.class_schema.insert(c, schema);
            }
            return self.new_object(cls, &name);
        }
        if tag & 0x8000 != 0 {
            return self.new_of_class((tag & 0x7FFF) as u32, expect);
        }
        self.backref(tag as u32)
    }

    fn new_of_class(&mut self, class_slot: u32, expect: Option<Cls>) -> Res<ObjRef> {
        let (cls, name) = match self.slots.get(&class_slot) {
            Some(Slot::Class { cls, name }) => (*cls, name.clone()),
            Some(Slot::Object { cls, .. }) => {
                return parse_err(format!("class-ref to non-class slot {class_slot} ({cls:?}) {}", self.ctx()));
            }
            None => {
                let Some(expect) = expect else {
                    return parse_err(format!("class-ref to unknown slot {class_slot} {}", self.ctx()));
                };
                self.slots.insert(class_slot, Slot::Class { cls: Some(expect), name: format!("{expect:?}") });
                self.class_slot.insert(expect, class_slot);
                (Some(expect), format!("{expect:?}"))
            }
        };
        self.new_object(cls, &name)
    }

    fn new_object(&mut self, cls: Option<Cls>, name: &str) -> Res<ObjRef> {
        let slot = self.alloc(Slot::Object { cls, val: None });
        let Some(c) = cls else {
            return parse_err(format!("no reader for class {name} {}", self.ctx()));
        };
        let prev = self.current_class.replace(c);
        let prev_slot = self.current_obj_slot.replace(slot);
        let value = self.read_fields(c);
        self.current_class = prev;
        self.current_obj_slot = prev_slot;
        let val = Some(Rc::new(value?));
        self.slots.insert(slot, Slot::Object { cls, val: val.clone() });
        if matches!(c, Cls::DimensionLinear | Cls::Text) {
            self.annot_watermark = Some(self.next_slot);
        }
        Ok(ObjRef { slot: Some(slot), kind: Kind::Object(cls), val })
    }

    /// Maps a file slot to the walker's numbering through the burn bands; `None` for a slot
    /// inside a band.
    fn translate_ref(&self, slot: u32) -> Option<u32> {
        let mut offset = 0;
        for &(start, width) in &self.burns {
            if slot < start {
                break;
            }
            if slot < start + width {
                return None;
            }
            offset += width;
        }
        Some(slot - offset)
    }

    fn backref(&mut self, mut slot: u32) -> Res<ObjRef> {
        if self.burns.first().is_some_and(|&(start, _)| slot >= start) {
            match self.translate_ref(slot) {
                Some(s) => slot = s,
                None => return Ok(ObjRef { slot: Some(slot), kind: Kind::Reserved, val: None }),
            }
        }
        match self.slots.get(&slot) {
            None if slot < self.walk_base => Ok(ObjRef { slot: Some(slot), kind: Kind::Premodel, val: None }),
            None => parse_err(format!("back-ref to unwalked slot {slot} {}", self.ctx())),
            Some(Slot::Class { .. }) => parse_err(format!("back-ref to class slot {slot} {}", self.ctx())),
            Some(Slot::Object { cls, val }) => Ok(ObjRef { slot: Some(slot), kind: Kind::Object(*cls), val: val.clone() }),
        }
    }

    fn is_class_ref(&self, at: usize, slot: u32) -> bool {
        if slot <= 0x7FFF {
            u16_at(self.data, at) as u32 == 0x8000 | slot && at + 2 <= self.data.len()
        } else {
            at + 6 <= self.data.len() && u16_at(self.data, at) == 0x7FFF && u32_at(self.data, at + 2) == 0x8000_0000 | slot
        }
    }

    fn tag_is_class(&self, tag: u16, cls: Cls) -> bool {
        self.class_slot.get(&cls).is_some_and(|&s| tag as u32 == 0x8000 | s)
    }

    /// A CLayer record at `at`: its new-class record, a class-ref to the known CLayer slot, or,
    /// with `unmatched_class_ref` (the slot-base probe, whose numbering does not match the
    /// file's), any short class-ref.
    fn clayer_record_at(&self, at: usize, unmatched_class_ref: bool) -> bool {
        if at + 2 > self.data.len() {
            return false;
        }
        let tag = u16_at(self.data, at);
        if tag == 0xFFFF && at + 12 <= self.data.len() && u16_at(self.data, at + 4) == 6 && &self.data[at + 6..at + 12] == b"CLayer" {
            return true;
        }
        if self.class_slot.get(&Cls::Layer).is_some_and(|&s| self.is_class_ref(at, s)) {
            return true;
        }
        unmatched_class_ref && tag & 0x8000 != 0 && tag != 0xFFFF
    }

    /// Some v18 zero-material files follow a colour layer's tail with 12 zero bytes and a u32,
    /// then another layer: skip those 16 bytes, but only in front of a layer.
    fn skip_layer_colour_ext(&mut self) {
        const EXT: usize = 16;
        if self.pos + EXT + 2 > self.data.len() || self.data[self.pos..self.pos + 12].iter().any(|&b| b != 0) {
            return;
        }
        if self.clayer_record_at(self.pos + EXT, true) {
            self.pos += EXT;
        }
    }

    /// v18 template Layer0 after a custom tag: <parent u16><active u16><count u32>. Skip the
    /// parent only when both u16s are layers already read.
    fn skip_layer_parent_ref(&mut self) {
        let Some(me) = self.current_obj_slot else { return };
        if self.pos + 4 > self.data.len() {
            return;
        }
        let (a, b) = (u16_at(self.data, self.pos), u16_at(self.data, self.pos + 2));
        let plain = |t: u16| t != 0 && t & 0x8000 == 0 && t != 0x7FFF;
        if !plain(a) || a as u32 == me || !plain(b) {
            return;
        }
        let is_layer = |t: u16| matches!(self.slots.get(&(t as u32)), Some(Slot::Object { cls: Some(Cls::Layer), .. }));
        if is_layer(a) && is_layer(b) {
            self.pos += 2;
        }
    }

    /// Layer records past the declared count (v18: a custom tag counted, then Layer0), skipping
    /// null separators, up to the definition-list anchor. Returns (slot, value) per layer.
    fn trailing_layers(&mut self, unmatched_class_ref: bool) -> Res<Vec<(Option<u32>, Option<Rc<Val>>)>> {
        let mut out = Vec::new();
        while self.pos + 2 <= self.data.len() {
            if self.peek_u16()? == 0 {
                self.pos += 2;
                continue;
            }
            if !self.clayer_record_at(self.pos, unmatched_class_ref) {
                break;
            }
            let o = self.read_object(Some(Cls::Layer))?;
            if o.val.is_some() {
                out.push((o.slot, o.val));
            }
        }
        Ok(out)
    }

    // --- class readers ---

    fn read_fields(&mut self, cls: Cls) -> Res<Val> {
        match cls {
            Cls::Vertex => {
                self.early_preamble()?;
                Ok(Val::Vertex(self.f64s()?))
            }
            Cls::Edge => self.read_edge(),
            Cls::Curve => {
                self.preamble()?;
                self.u8()?;
                self.u32()?;
                Ok(Val::Other)
            }
            Cls::ArcCurve => {
                self.preamble()?;
                self.raw(5)?;
                self.f64s::<14>()?;
                Ok(Val::Other)
            }
            Cls::EdgeUse => self.read_edge_use(),
            Cls::Loop => self.read_loop(),
            Cls::Face => self.read_face(),
            Cls::Layer => self.read_layer(),
            Cls::Material => self.read_material(),
            Cls::Dib => self.read_dib(),
            Cls::AttributeContainer => {
                self.preamble()?;
                let mut children = Vec::new();
                while self.peek_u16()? != 0 {
                    children.push(self.read_object(Some(Cls::AttributeNamed))?.val);
                }
                self.pos += 2;
                Ok(Val::Attributes(children))
            }
            Cls::AttributeNamed => self.read_dictionary(),
            Cls::Camera => {
                self.raw(137)?;
                self.u16()?;
                self.utf16()?;
                self.raw(if self.ver >= 6 { 33 } else { 10 })?;
                Ok(Val::Other)
            }
            Cls::Thumbnail => {
                self.preamble()?;
                if self.ver < FIRST_V4 {
                    // SketchUp 3: a fixed camera block, no image
                    self.raw(129)?;
                    return Ok(Val::Other);
                }
                self.read_object(Some(Cls::Camera))?;
                // SketchUp 4 and 5 end the thumbnail with its camera
                if self.ver >= 6 {
                    self.read_object(Some(Cls::Dib))?;
                }
                Ok(Val::Other)
            }
            Cls::Relationship => {
                self.preamble()?;
                self.entity_ref()?;
                self.entity_ref()?;
                Ok(Val::Other)
            }
            Cls::ComponentDefinition => self.read_definition(),
            Cls::Image => {
                self.preamble()?;
                let db = self.drawbase()?;
                let def = self.read_object(None)?.slot;
                let xform = self.f64s()?;
                self.f64()?;
                self.utf16()?;
                self.raw(16)?;
                Ok(Val::Image { db, def, xform })
            }
            Cls::ComponentInstance | Cls::Group => self.read_instance(),
            Cls::FaceTextureCoords => {
                self.preamble()?;
                self.u32()?;
                self.f64s::<24>()?;
                for _ in 0..2 {
                    let pins = self.u32()? as usize;
                    self.raw(pins.checked_mul(32).ok_or(Error::Eof)?)?;
                }
                self.u32()?;
                self.u32()?;
                Ok(Val::Other)
            }
            Cls::ConstructionLine => self.read_construction_line(),
            Cls::ConstructionPoint => {
                self.preamble()?;
                self.drawbase()?;
                self.f64s::<6>()?;
                self.u8()?;
                Ok(Val::Other)
            }
            Cls::SectionPlane => {
                self.preamble()?;
                self.drawbase()?;
                let first = f64::from_le_bytes(self.peek(8).try_into().map_err(|_| Error::Eof)?);
                // Not a plane normal component (or NaN): an object comes first
                if first.is_nan() || first.abs() > 1.0001 {
                    self.read_object(None)?;
                }
                self.f64s::<4>()?;
                if self.at_string() {
                    self.utf16()?;
                    self.utf16()?;
                }
                Ok(Val::Other)
            }
            Cls::SkFont => {
                // the attribute container and pid of `preamble`, without its version gate
                self.preamble()?;
                self.utf16()?;
                self.raw(15)?;
                Ok(Val::Other)
            }
            Cls::DimensionLinear => {
                self.preamble()?;
                self.drawbase()?;
                self.utf16()?;
                // SketchUp 4 and 5 write no font and a shorter fixed part
                if self.ver >= 6 {
                    self.read_object(Some(Cls::SkFont))?;
                }
                self.raw(if self.ver >= 6 { 37 } else { 32 })?;
                // each connection point: a ref, then the instance paths of the anchored entity
                self.entity_ref()?;
                self.connection_paths()?;
                self.raw(32)?;
                self.entity_ref()?;
                self.connection_paths()?;
                self.raw(if self.ver >= 6 { 72 } else { 68 })?;
                Ok(Val::Other)
            }
            Cls::Text => self.read_text(),
        }
    }

    /// Attribute container and persistent id, in front of most objects.
    fn preamble(&mut self) -> Res<Option<Rc<Val>>> {
        Ok(self.preamble_ref()?.val)
    }

    fn preamble_ref(&mut self) -> Res<ObjRef> {
        let attrs = self.read_object(Some(Cls::AttributeContainer))?;
        if self.has_pid {
            let mask = self.u8()?;
            self.raw(mask.count_ones() as usize)?;
        }
        Ok(attrs)
    }

    /// `preamble` for the objects that open without one in SketchUp 3: vertex, loop, edge-use,
    /// material and layer.
    fn early_preamble(&mut self) -> Res<()> {
        if self.ver >= FIRST_V4 {
            self.preamble()?;
        }
        Ok(())
    }

    fn read_dib(&mut self) -> Res<Val> {
        self.u32()?;
        let len = self.u32()? as usize;
        if len > self.data.len() {
            return parse_err(format!("implausible dib length {len} {}", self.ctx()));
        }
        self.raw(len)?;
        Ok(Val::Dib)
    }

    /// After a dimension's connection ref: [ref][u32 n1][n1 refs][u32 n2][n2 refs], the instance
    /// paths of the anchored entity.
    fn connection_paths(&mut self) -> Res<()> {
        self.entity_ref()?;
        for _ in 0..2 {
            let n = self.u32()?;
            if n > 1000 {
                return parse_err(format!("implausible dimension path length {}", self.ctx()));
            }
            for _ in 0..n {
                self.path_ref()?;
            }
        }
        Ok(())
    }

    /// One instance of a dimension's path: a reference, or the object in full when nothing
    /// pointed at it before (a dimension written before the group it is anchored in).
    fn path_ref(&mut self) -> Res<()> {
        let tag = self.peek_u16()?;
        let new = match tag {
            0x7FFF => self.pos + 6 <= self.data.len() && u32_at(self.data, self.pos + 2) & 0x8000_0000 != 0,
            t => t == 0xFFFF || t & 0x8000 != 0,
        };
        if new {
            self.read_object(None)?;
        } else {
            self.entity_ref()?;
        }
        Ok(())
    }

    fn drawbase(&mut self) -> Res<DrawBase> {
        // 5 bytes in SketchUp 3, 7 in 4, 8 from 5
        let b = self.raw(if self.ver < FIRST_V4 { 5 } else if self.ver < 5 { 7 } else { 8 })?;
        let tag = self.peek_u16()?;
        let layer = if self.tag_is_class(tag, Cls::Layer) {
            self.read_object(Some(Cls::Layer))?;
            0
        } else if tag == 0x7FFF {
            self.u16()?;
            if self.u32()? & 0x8000_0000 != 0 {
                return parse_err(format!("drawbase layer: unexpected class {}", self.ctx()));
            }
            0
        } else {
            self.u16()?
        };
        if self.ver < FIRST_V4 {
            // nothing in SketchUp 3's draw block is decodable; a face's material comes from its
            // own pointer (see read_face)
            return Ok(DrawBase { material: 0, hidden: 0, layer });
        }
        Ok(DrawBase { material: u16::from_le_bytes([b[0], b[1]]), hidden: b[2], layer })
    }

    fn read_edge(&mut self) -> Res<Val> {
        self.preamble()?;
        self.drawbase()?;
        let v1 = self.read_object(Some(Cls::Vertex))?.slot;
        let v2 = self.read_object(Some(Cls::Vertex))?.slot;
        let curve = self.read_object(None)?.kind;
        if !matches!(curve, Kind::Null | Kind::Object(Some(Cls::Curve | Cls::ArcCurve))) {
            return parse_err(format!("edge curve pointer resolved to {curve:?} {}", self.ctx()));
        }
        Ok(Val::Edge { v1, v2 })
    }

    fn register_burn(&mut self, delta: u32) {
        let start = self.annot_watermark.take().unwrap_or(0) + self.cum_delta;
        self.burns.push((start, delta));
        self.cum_delta += delta;
        if let Some(last) = self.burn_stack.last_mut() {
            *last += 1;
        }
    }

    fn read_edge_use(&mut self) -> Res<Val> {
        self.early_preamble()?;
        let edge = self.read_object(Some(Cls::Edge))?.slot;
        let sense = self.u8()?;
        let p0 = self.pos;
        let tag = self.u16()?;
        let parent = if tag == 0x7FFF {
            let big = self.u32()?;
            if big & 0x8000_0000 != 0 {
                return parse_err(format!("edge-use parent is a new object {}", self.ctx()));
            }
            Some(big)
        } else if tag == 0xFFFF || tag & 0x8000 != 0 {
            return parse_err(format!("edge-use parent is a new object {}", self.ctx()));
        } else {
            (tag != 0).then_some(tag as u32)
        };
        let expected = self.current_loop.map(|l| l + self.cum_delta);
        if parent != expected {
            let delta = match (parent, expected) {
                (Some(p), Some(e)) => p as i64 - e as i64,
                _ => 0,
            };
            if delta > 0 && delta <= 4096 && self.annot_watermark.is_some() {
                self.register_burn(delta as u32);
            } else {
                self.pos = p0;
                return parse_err(format!("edge-use parent slot {parent:?} != current loop {expected:?} {}", self.ctx()));
            }
        }
        Ok(Val::EdgeUse((edge, sense)))
    }

    fn read_loop(&mut self) -> Res<Val> {
        let me = self.next_slot - 1;
        let prev = self.current_loop.replace(me);
        self.early_preamble()?;
        self.raw(2)?;
        let mut uses = Vec::new();
        while self.peek_u16()? != 0 {
            if let Some(Val::EdgeUse(u)) = self.read_object(Some(Cls::EdgeUse))?.val.as_deref() {
                uses.push(*u);
            }
        }
        self.pos += 2;
        self.current_loop = prev;
        Ok(Val::Loop(uses))
    }

    fn read_face(&mut self) -> Res<Val> {
        let pre = self.preamble_ref()?;
        let mut db = self.drawbase()?;
        if self.ver < FIRST_V4 && matches!(pre.val.as_deref(), Some(Val::Material { .. })) {
            // SketchUp 3: the pointer that opens a face is its front material
            db.material = pre.slot.unwrap_or(0) as u16;
        }
        let plane = self.f64s()?;
        let count = self.u32()?;
        if count > 10_000 {
            return parse_err(format!("implausible loop count {count} {}", self.ctx()));
        }
        let mut loops = Vec::new();
        for _ in 0..count {
            if let Some(Val::Loop(uses)) = self.read_object(Some(Cls::Loop))?.val.as_deref() {
                loops.push(uses.clone());
            }
        }
        let back_material = self.u16()?;
        Ok(Val::Face { db, plane, loops, back_material })
    }

    fn read_dictionary(&mut self) -> Res<Val> {
        self.preamble()?;
        self.raw(4)?;
        let name = self.utf16()?;
        let mut entries = Vec::new();
        loop {
            let key = self.utf16()?;
            if key.is_empty() {
                break;
            }
            let t = self.u8()?;
            entries.push((key, self.typed_value(t)?));
        }
        // the trailing u32 is a SketchUp 7 addition; a 6 file ends at the empty key
        if self.ver >= 7 {
            self.u32()?;
        }
        Ok(Val::Dictionary { name, entries })
    }

    /// An attribute value, as text.
    fn typed_value(&mut self, t: u8) -> Res<String> {
        let join = |v: &[f64]| v.iter().map(f64::to_string).collect::<Vec<_>>().join(",");
        Ok(match t {
            0 => String::new(),
            4 => self.i32()?.to_string(),
            6 | 12 => self.f64()?.to_string(),
            7 => self.u8()?.to_string(),
            9 => self.u32()?.to_string(),
            10 => self.utf16()?,
            11 => {
                let n = self.u32()?;
                if n > 100_000 {
                    return parse_err(format!("implausible attr array count {}", self.ctx()));
                }
                let mut items = Vec::new();
                for _ in 0..n {
                    let t = self.u8()?;
                    items.push(self.typed_value(t)?);
                }
                items.join(",")
            }
            17 | 18 => join(&self.f64s::<3>()?),
            _ => return parse_err(format!("unknown attribute value type 0x{t:x} {}", self.ctx())),
        })
    }

    fn read_layer(&mut self) -> Res<Val> {
        self.early_preamble()?;
        let name = self.utf16()?;
        let mut mid = Vec::new();
        while mid.len() < 8 && !self.at_string() {
            mid.push(self.u8()?);
        }
        self.utf16()?;
        let flags = self.u16()?;
        let hidden = mid.first().copied().unwrap_or(0);
        if flags & 0xFF != 0 {
            let (rgba, _, _) = self.texture_block()?;
            self.raw(4)?;
            return Ok(Val::Layer { name, hidden, rgba });
        }
        let rgba = self.raw(4)?.try_into().unwrap();
        self.utf16()?;
        // SketchUp 3's tail is 4 bytes shorter
        self.raw(if self.ver < FIRST_V4 { 17 } else { 21 })?;
        self.skip_layer_colour_ext();
        self.skip_layer_parent_ref();
        Ok(Val::Layer { name, hidden, rgba })
    }

    /// A texture: (average colour, opacity, use opacity). The caller has read the texture flag
    /// as a u16; its high byte is the first byte of the texture's own preamble. SketchUp 3 to 5
    /// write the image inline, without preamble or object tag.
    fn texture_block(&mut self) -> Res<([u8; 4], f64, u8)> {
        if self.ver < FIRST_DIB_OBJECT {
            self.read_dib()?;
        } else {
            self.pos -= 1;
            self.preamble()?;
            if !matches!(self.read_object(Some(Cls::Dib))?.val.as_deref(), Some(Val::Dib)) {
                return parse_err(format!("texture object is not a dib {}", self.ctx()));
            }
        }
        match find_bytes(self.data, &STR_MARKER, self.pos, self.pos + 28).map(|m| m - self.pos) {
            Some(20) => {
                self.u32()?;
            }
            Some(16) => {}
            _ => return parse_err(format!("texture size block misaligned {}", self.ctx())),
        }
        self.f64s::<2>()?;
        self.utf16()?;
        let avg = self.raw(9)?;
        self.utf16()?;
        self.raw(8)?;
        let opacity = self.f64()?;
        let use_opacity = self.u8()?;
        Ok(([avg[0], avg[1], avg[2], avg[3]], opacity, use_opacity))
    }

    fn read_material(&mut self) -> Res<Val> {
        self.early_preamble()?;
        let name = self.utf16()?;
        if self.u16()? == 0 {
            let rgba = self.raw(4)?.try_into().unwrap();
            self.utf16()?;
            self.raw(8)?;
            let opacity = self.f64()?;
            let use_opacity = self.u8()?;
            return Ok(Val::Material { name, rgba, opacity, use_opacity });
        }
        let (rgba, opacity, use_opacity) = self.texture_block()?;
        Ok(Val::Material { name, rgba, opacity, use_opacity })
    }

    fn entity_ref(&mut self) -> Res<Option<u32>> {
        let tag = self.u16()?;
        if tag == 0 {
            return Ok(None);
        }
        if tag == 0x7FFF {
            let big = self.u32()?;
            if big & 0x8000_0000 != 0 {
                return parse_err(format!("entity ref is a new object {}", self.ctx()));
            }
            return Ok(Some(big));
        }
        if tag == 0xFFFF || tag & 0x8000 != 0 {
            return parse_err(format!("entity ref is a new object {}", self.ctx()));
        }
        Ok(Some(tag as u32))
    }

    /// True when the u16 at `at` starts an object (or, with `allow_null`, ends a list).
    fn strict_next_tag(&self, at: usize, allow_null: bool) -> bool {
        if at + 2 > self.data.len() {
            return false;
        }
        let t = u16_at(self.data, at);
        match t {
            0 => allow_null,
            0x7FFF | 0xFFFF => true,
            t if t & 0x8000 != 0 => matches!(self.slots.get(&((t & 0x7FFF) as u32)), Some(Slot::Class { .. })),
            _ => false,
        }
    }

    fn read_construction_line(&mut self) -> Res<Val> {
        self.preamble()?;
        self.drawbase()?;
        self.f64s::<8>()?;
        let k = match self.cline_tail {
            Some(k) => k,
            None => {
                let default = if self.ver == 17 { 7 } else { 4 };
                let order = [default].into_iter().chain([0, 4, 7].into_iter().filter(|&c| c != default));
                let order: Vec<usize> = order.collect();
                let k = [false, true]
                    .into_iter()
                    .find_map(|allow_null| order.iter().copied().find(|&c| self.strict_next_tag(self.pos + c, allow_null)))
                    .unwrap_or(default);
                self.cline_tail = Some(k);
                k
            }
        };
        self.raw(k)?;
        Ok(Val::Other)
    }

    fn read_text(&mut self) -> Res<Val> {
        self.preamble()?;
        self.drawbase()?;
        self.read_object(Some(Cls::SkFont))?;
        let mut p = self.pos;
        let idx = loop {
            let Some(idx) = find_bytes(self.data, &STR_MARKER, p, self.pos + 512) else {
                return parse_err(format!("text delimiter not found {}", self.ctx()));
            };
            let blk = &self.data[idx.saturating_sub(11)..idx];
            // `01 00 00 00 ?? 00 <arrow type 0-8> 00 00 00 01`
            if blk.len() == 11 && blk[..4] == [1, 0, 0, 0] && blk[6] <= 8 && blk[7..10] == [0, 0, 0] && blk[10] == 1 {
                break idx;
            }
            p = idx + 3;
        };
        self.raw(idx - self.pos)?;
        self.utf16()?;
        self.raw(5)?;
        while self.peek(2) == [0xFF, 0x7F] && self.peek(6).len() == 6 {
            if u32_at(self.data, self.pos + 2) & 0x8000_0000 != 0 {
                break;
            }
            self.raw(6)?;
        }
        Ok(Val::Other)
    }

    fn read_entity_list(&mut self, count: u32, owner: Owner) -> Res<Entities> {
        self.burn_stack.push(0);
        let res = self.read_entity_list_inner(count, owner);
        self.burn_stack.pop();
        res
    }

    fn read_entity_list_inner(&mut self, count: u32, owner: Owner) -> Res<Entities> {
        let mut ents = Vec::new();
        while ents.len() < count as usize {
            let p = self.pos;
            let burn_credit = owner == Owner::Definition && self.burn_stack.last().is_some_and(|&c| c > 0);
            if burn_credit && p + 25 <= self.data.len() && u32_at(self.data, p) == 0 && self.data[p + 22..p + 25] == STR_MARKER {
                break;
            }
            match self.read_object(None) {
                Ok(o) => ents.push((o.slot, o.val)),
                // A record the walker cannot read ends the root list (or a list with burns)
                Err(Error::Parse(_)) if owner == Owner::Root || burn_credit => {
                    self.pos = p;
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(ents)
    }

    /// A list count written after the 2020 filler (a string marker and padding), if one fits.
    fn count_after_v20_filler(&self, count_pos: usize, limit: u32) -> Option<(u32, usize)> {
        let marker = (count_pos..count_pos + 12)
            .take_while(|&i| i + 4 <= self.data.len())
            .find(|&i| self.data[i..i + 3] == STR_MARKER)?;
        if self.data[marker + 3] != 0 {
            return None;
        }
        (1..=MAX_V20_FILLER_PAD).step_by(4).map(|pad| marker + 4 + pad).take_while(|&at| at + 4 <= self.data.len()).find_map(|at| {
            let count = u32_at(self.data, at);
            (count > 0 && count <= limit && self.plausible_list_tag(at + 4)).then_some((count, at + 4))
        })
    }

    fn retry_count_after_v20_filler(&mut self, count_pos: usize, limit: u32) -> Option<u32> {
        let (count, next) = self.count_after_v20_filler(count_pos, limit)?;
        self.pos = next;
        Some(count)
    }

    fn plausible_list_tag(&self, at: usize) -> bool {
        if at + 2 > self.data.len() {
            return false;
        }
        match u16_at(self.data, at) {
            0 | 0x7FFF | 0xFFFF => true,
            t if t & 0x8000 != 0 => matches!(self.slots.get(&((t & 0x7FFF) as u32)), Some(Slot::Class { .. })),
            t => (t as u32) < self.next_slot,
        }
    }

    fn read_definition(&mut self) -> Res<Val> {
        self.preamble()?;
        // undecoded base block: 13 bytes in SketchUp 3, 19 in 4, 20 from 5, 22 from 2017
        self.raw(match self.ver {
            v if v >= 17 => 22,
            v if v >= 5 => 20,
            v if v >= FIRST_V4 => 19,
            _ => 13,
        })?;
        let layers = self.u32()?;
        if layers > 10_000 {
            return parse_err(format!("implausible def layer count {}", self.ctx()));
        }
        let mut got = 0;
        while got < layers {
            if self.peek_u16()? == 0 {
                self.pos += 2;
                continue;
            }
            self.read_object(Some(Cls::Layer))?;
            got += 1;
        }
        // SketchUp 2019 also follows the last layer with a separator null
        if self.ver == 19 && layers > 0 && self.peek_u16()? == 0 {
            self.pos += 2;
        }
        if self.u16()? == 0x7FFF {
            self.u32()?;
        }
        let mut count = None;
        if self.ver >= 20 {
            count = self.retry_count_after_v20_filler(self.pos, 5_000_000);
        }
        let mut count = match count {
            Some(c) => c,
            None => {
                self.u32()?;
                self.u32()?
            }
        };
        if count > 5_000_000 || count == 0 {
            if let Some(c) = self.retry_count_after_v20_filler(self.pos - 4, 5_000_000) {
                count = c;
            }
        }
        if count > 5_000_000 {
            return parse_err(format!("implausible def entity count {}", self.ctx()));
        }
        let ents = self.read_entity_list(count, Owner::Definition)?;
        let mut relations = self.u32()?;
        if relations > 100_000 {
            if let Some(c) = self.retry_count_after_v20_filler(self.pos - 4, 100_000) {
                relations = c;
            }
        }
        if relations > 100_000 {
            return parse_err(format!("definition list misaligned {}", self.ctx()));
        }
        for _ in 0..relations {
            self.read_object(Some(Cls::Relationship))?;
        }
        self.u16()?;
        let marker_at = |at: usize| self.data.get(at + 16..at + 19) == Some(&STR_MARKER[..]);
        if !marker_at(self.pos) {
            if let Some(at) = (1..=4).map(|skip| self.pos + skip).find(|&at| marker_at(at)) {
                self.pos = at;
            }
        }
        self.raw(16)?; // guid
        let name = self.utf16()?;
        self.utf16()?;
        self.utf16()?;
        self.u32()?;
        let thumb_slot = self.class_slot.get(&Cls::Thumbnail).copied();
        let d = self.data;
        let thumbnail = (0..96).map(|off| self.pos + off).find(|&p| {
            (d.get(p) == Some(&0xFF)
                && d.get(p + 1) == Some(&0xFF)
                && d.get(p + 4) == Some(&10)
                && d.get(p + 5) == Some(&0)
                && d.get(p + 6..p + 16) == Some(&b"CThumbnail"[..]))
                || thumb_slot.is_some_and(|s| self.is_class_ref(p, s))
        });
        let Some(thumbnail) = thumbnail else {
            return parse_err(format!("definition tail: thumbnail not found {}", self.ctx()));
        };
        self.raw(thumbnail - self.pos)?;
        self.read_object(Some(Cls::Thumbnail))?;
        Ok(Val::Definition { name, ents })
    }

    fn read_instance(&mut self) -> Res<Val> {
        let attrs = self.preamble()?;
        let db = self.drawbase()?;
        let def = self.read_object(Some(Cls::ComponentDefinition))?;
        if def.kind != Kind::Object(Some(Cls::ComponentDefinition)) {
            return parse_err(format!("instance definition ref is {:?} {}", def.kind, self.ctx()));
        }
        let xf = self.f64s()?;
        // SketchUp 3 and 4 instances carry no name; take one only where a string is
        let name = if self.ver >= FIRST_NAMED_INSTANCE || self.at_string() { self.utf16()? } else { String::new() };
        // the trailing guid is a SketchUp 2014 addition, for instances and groups alike (the
        // class schema does not tell: CGroup reports 1 on every version)
        if self.ver >= 14 {
            self.raw(16)?;
        }
        Ok(Val::Instance { db, def: def.slot, xf, name, attrs })
    }
}

/// True for a 2013-2020 file: no ZIP at the start, and the MFC version map in the header.
pub(super) fn is_legacy(data: &[u8]) -> bool {
    let head = |n: usize| &data[..n.min(data.len())];
    data.starts_with(&[0xFF, 0xFE, 0xFF, 0x0E])
        && find_bytes(head(256), b"PK\x03\x04", 0, 256).is_none()
        && find_bytes(head(512), b"CVersionMap", 0, 512).is_some()
}

/// The major version from the "{20.0.123}" string in the header.
pub(super) fn version_major(data: &[u8]) -> Option<u32> {
    let text: Vec<u8> = data[..96.min(data.len())].iter().copied().filter(|&b| b != 0).collect();
    text.iter().enumerate().filter(|(_, &b)| b == b'{').find_map(|(i, _)| {
        let digits = text[i + 1..].iter().take_while(|b| b.is_ascii_digit()).count();
        (digits > 0 && text.get(i + 1 + digits) == Some(&b'.'))
            .then(|| std::str::from_utf8(&text[i + 1..i + 1 + digits]).ok()?.parse().ok())
            .flatten()
    })
}

/// A new-class record for `name`: FF FF, schema (2 bytes), name length, name.
fn find_class_record(data: &[u8], name: &str) -> Option<usize> {
    let len = name.len();
    (0..(data.len() + 1).saturating_sub(6 + len)).find(|&i| {
        data[i] == 0xFF
            && data[i + 1] == 0xFF
            && data[i + 4] == len as u8
            && data[i + 5] == 0
            && &data[i + 6..i + 6 + len] == name.as_bytes()
    })
}

const PROBE_BASE: u32 = 1 << 20;

/// The class slot the second material refers to: the walk's first slot.
fn bootstrap_two_materials(data: &[u8], ver: u32, start: usize) -> Res<u32> {
    let mut ar = Archive::new(data, ver, PROBE_BASE);
    ar.pos = start;
    ar.read_object(Some(Cls::Material))?;
    let tag = ar.peek_u16()?;
    if tag == 0xFFFF || tag & 0x8000 == 0 {
        return parse_err("cannot bootstrap the slot base");
    }
    Ok((tag & 0x7FFF) as u32)
}

/// Candidate first slots from the reference after the layers, which points before the walk.
fn probe_layer_anchor_bases(data: &[u8], ver: u32, start: usize, materials: u32) -> Res<Vec<u32>> {
    let mut ar = Archive::new(data, ver, PROBE_BASE);
    ar.pos = start;
    for _ in 0..materials {
        ar.read_object(Some(Cls::Material))?;
    }
    ar.u32()?;
    if ver >= 17 {
        ar.u8()?;
    }
    // SketchUp 3 writes no layer count: the layers run up to the definition-list anchor
    let mut count = 0;
    if ver >= FIRST_V4 {
        count = ar.u32()?;
        if !(1..=100_000).contains(&count) {
            return parse_err("implausible layer count in base probe");
        }
    }
    let mut layer_slots = Vec::new();
    for _ in 0..count {
        layer_slots.push(ar.read_object(Some(Cls::Layer))?.slot);
    }
    layer_slots.extend(ar.trailing_layers(true)?.into_iter().map(|(s, _)| s));
    let anchor = ar.read_object(None)?;
    if anchor.kind != Kind::Premodel {
        return parse_err(format!("base probe: anchor resolved to {:?}", anchor.kind));
    }
    let s = anchor.slot.unwrap_or(0) as i64;
    let b0 = PROBE_BASE as i64;
    Ok(layer_slots
        .into_iter()
        .map(|rel| s - (rel.map_or(0, |r| r as i64) - b0))
        .filter(|&c| c > 0 && c < b0)
        .map(|c| c as u32)
        .collect())
}

struct Walk {
    slots: HashMap<u32, Slot>,
    root: Entities,
    layers: Vec<(u32, Rc<Val>)>,
    materials: Vec<(u32, Rc<Val>)>,
}

fn walk(data: &[u8]) -> Res<Walk> {
    let ver = version_major(data).ok_or(Error::Parse("no version string in header".into()))?;
    let (start, materials) = match find_class_record(data, "CMaterial") {
        Some(at) => {
            let count = u32_at(data, at.checked_sub(4).ok_or(Error::Eof)?);
            if count > 100_000 {
                return parse_err("implausible material count");
            }
            (at, count)
        }
        None => {
            let at = find_class_record(data, "CLayer").ok_or(Error::Parse("no CMaterial or CLayer class record found".into()))?;
            (at.checked_sub(if ver >= 17 { 9 } else { 8 }).ok_or(Error::Eof)?, 0)
        }
    };
    let bases = if materials >= 2 {
        vec![bootstrap_two_materials(data, ver, start)?]
    } else {
        probe_layer_anchor_bases(data, ver, start, materials)?
    };
    // A wrong base dies almost at once, the right one only where the file differs from what
    // the walker knows: report the failure that got furthest into the file.
    let at = |m: &str| m.split("@0x").nth(1).and_then(|h| usize::from_str_radix(h.split(':').next()?, 16).ok());
    let mut best: Option<(Option<usize>, String)> = None;
    for base in bases {
        match walk_model(data, ver, start, materials, base) {
            Ok(w) => return Ok(w),
            Err(Error::Parse(m)) => {
                let a = at(&m);
                if best.as_ref().is_none_or(|(b, _)| a > *b) {
                    best = Some((a, m));
                }
            }
            Err(e) => return Err(e),
        }
    }
    parse_err(best.map_or_else(|| "no viable slot base candidate".into(), |(_, m)| m))
}

/// True when a CComponentDefinition record starts at `at`.
fn definition_starts_at(ar: &Archive, at: usize) -> bool {
    (ar.data.get(at..at + 2) == Some(&[0xFF, 0xFF][..]) && ar.data.get(at + 6..at + 26) == Some(&b"CComponentDefinition"[..]))
        || ar.class_slot.get(&Cls::ComponentDefinition).is_some_and(|&s| ar.is_class_ref(at, s))
}

fn walk_model(data: &[u8], ver: u32, start: usize, material_count: u32, base: u32) -> Res<Walk> {
    let mut ar = Archive::new(data, ver, base);
    ar.pos = start;
    let mut materials = Vec::new();
    for _ in 0..material_count {
        let o = ar.read_object(Some(Cls::Material))?;
        if let (Some(s), Some(v)) = (o.slot, o.val) {
            materials.push((s, v));
        }
    }
    ar.u32()?;
    if ver >= 17 {
        ar.u8()?;
    }
    // SketchUp 3 writes no layer count: the layers run up to the definition-list anchor
    let layer_count = if ver < FIRST_V4 { u32::MAX } else { ar.u32()? };
    if layer_count > 100_000 && ver >= FIRST_V4 {
        return parse_err("implausible layer count");
    }
    let mut layers = Vec::new();
    while layers.len() < layer_count as usize {
        let tag = ar.peek_u16()?;
        if tag == 0 {
            ar.pos += 2;
            continue;
        }
        if tag != 0xFFFF && tag & 0x8000 == 0 {
            break;
        }
        let o = ar.read_object(Some(Cls::Layer))?;
        if let (Some(s), Some(v)) = (o.slot, o.val) {
            layers.push((s, v));
        }
    }
    // trailing separators and layer records past the declared count
    for (s, v) in ar.trailing_layers(false)? {
        if let (Some(s), Some(v)) = (s, v) {
            layers.push((s, v));
        }
    }
    let anchor = ar.read_object(None)?;
    if anchor.kind != Kind::Object(Some(Cls::Layer)) {
        return parse_err(format!("definition-list anchor is {:?}, not a layer", anchor.kind));
    }
    let mut def_count = ar.u32()?;
    if def_count > 1_000_000 {
        if let Some(c) = ar.retry_count_after_v20_filler(ar.pos - 4, 1_000_000) {
            def_count = c;
            // a model without definitions has no definition count: the count behind the 2020
            // filler is then the root count, so put it back
            if !definition_starts_at(&ar, ar.pos) {
                def_count = 0;
                ar.pos -= 4;
            }
        }
    }
    if def_count > 1_000_000 {
        return parse_err("implausible definition count");
    }
    for _ in 0..def_count {
        ar.read_object(Some(Cls::ComponentDefinition))?;
    }
    loop {
        let tag = ar.peek_u16()?;
        let is_def = ar.tag_is_class(tag, Cls::ComponentDefinition)
            || (tag == 0xFFFF && ar.peek(26).get(6..26) == Some(&b"CComponentDefinition"[..]));
        if !is_def {
            break;
        }
        ar.read_object(None)?;
    }
    let mut root_count = ar.u32()?;
    if root_count > 5_000_000 {
        if let Some(c) = ar.retry_count_after_v20_filler(ar.pos - 4, 5_000_000) {
            root_count = c;
        }
    }
    if root_count > 5_000_000 {
        return parse_err("implausible root entity count");
    }
    let root = ar.read_entity_list(root_count, Owner::Root)?;
    Ok(Walk { slots: ar.slots, root, layers, materials })
}

fn object(slots: &HashMap<u32, Slot>, slot: u32) -> Option<&Val> {
    match slots.get(&slot) {
        Some(Slot::Object { val: Some(v), .. }) => Some(v),
        _ => None,
    }
}

fn add_edge(def: &mut Definition, slot: u32, edge: &Val, slots: &HashMap<u32, Slot>) {
    if def.edges.contains_key(&(slot as u64)) {
        return;
    }
    let Val::Edge { v1, v2 } = edge else { return };
    for v in [v1, v2].into_iter().flatten() {
        if let Some(Val::Vertex(xyz)) = object(slots, *v) {
            def.vertices.entry(*v as u64).or_insert(*xyz);
        }
    }
    def.edges.insert(slot as u64, (v1.map(u64::from), v2.map(u64::from)));
}

/// Dynamic component attributes from an instance's attribute container.
fn dynamic_props(attrs: Option<&Rc<Val>>) -> Vec<(String, String)> {
    let Some(Val::Attributes(children)) = attrs.map(|a| &**a) else { return Vec::new() };
    children
        .iter()
        .flatten()
        .find_map(|c| match &**c {
            Val::Dictionary { name, entries } if name == "dynamic_attributes" => Some(entries.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn fill_definition(def: &mut Definition, ents: &Entities, slots: &HashMap<u32, Slot>) {
    let nonzero = |v: u16| (v != 0).then_some(v as u64);
    for (slot, val) in ents {
        let (Some(slot), Some(val)) = (slot, val) else { continue };
        match &**val {
            Val::Edge { .. } => add_edge(def, *slot, val, slots),
            Val::Face { db, plane, loops, back_material } => {
                let loops = loops
                    .iter()
                    .map(|uses| {
                        uses.iter()
                            .filter_map(|&(edge, sense)| {
                                let edge = edge?;
                                add_edge(def, edge, object(slots, edge)?, slots);
                                Some((edge as u64, sense != 0))
                            })
                            .collect()
                    })
                    .collect();
                let face = Face {
                    loops,
                    normal: [plane[0], plane[1], plane[2]],
                    material: nonzero(db.material),
                    back_material: nonzero(*back_material),
                };
                def.set_face(*slot as u64, face);
            }
            Val::Instance { db, def: target, xf, name, attrs } => def.instances.push(Instance {
                name: name.clone(),
                definition: target.map(u64::from),
                matrix: *xf,
                material: nonzero(db.material),
                tag: nonzero(db.layer),
                hidden: db.hidden != 0,
                props: dynamic_props(attrs.as_ref()),
            }),
            Val::Image { db, def: target, xform } => {
                let mut matrix: Matrix = [0.0; 13];
                matrix[..12].copy_from_slice(xform);
                matrix[12] = 1.0;
                def.instances.push(Instance {
                    name: String::new(),
                    definition: target.map(u64::from),
                    matrix,
                    material: nonzero(db.material),
                    tag: nonzero(db.layer),
                    hidden: db.hidden != 0,
                    props: Vec::new(),
                });
            }
            _ => {}
        }
    }
}

pub(super) fn parse(data: &[u8]) -> Result<Parsed, Error> {
    let w = walk(data)?;
    let mut parsed = Parsed::default();

    let mut by_name: HashMap<String, Material> = HashMap::new();
    let mut names = Vec::new();
    for (slot, val) in &w.materials {
        if let Val::Material { name, rgba, opacity, use_opacity } = &**val {
            let opacity = if *use_opacity != 0 { (1.0 - opacity).clamp(0.0, 1.0) } else { 1.0 };
            by_name.insert(name.clone(), Material { rgba: *rgba, opacity });
            names.push((*slot, name.clone()));
        }
    }
    parsed.materials = names.into_iter().filter_map(|(s, n)| by_name.get(&n).map(|m| (s as u64, *m))).collect();

    for (slot, val) in &w.layers {
        if let Val::Layer { name, hidden, rgba } = &**val {
            parsed.tags.insert(name.clone(), Tag { color: [rgba[0], rgba[1], rgba[2]], hidden: *hidden != 0 });
            parsed.tag_names.insert(*slot as u64, name.clone());
        }
    }
    parsed.tags.entry(DEFAULT_TAG.to_string()).or_insert(Tag { color: DEFAULT_TAG_COLOR, hidden: false });

    for (slot, entry) in &w.slots {
        if let Slot::Object { val: Some(val), .. } = entry {
            if let Val::Definition { name, ents } = &**val {
                let mut def = Definition { name: name.clone(), ..Definition::default() };
                fill_definition(&mut def, ents, &w.slots);
                parsed.definitions.insert(*slot as u64, def);
            }
        }
    }
    fill_definition(&mut parsed.root, &w.root, &w.slots);
    Ok(parsed)
}
