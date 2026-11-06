//! Result tables and the binary result block (`sam_result_block`, sidecar `.samr`, inline
//! base64 in `sam-json`).
//!
//! Little endian, no padding:
//!
//! ```text
//! magic     "SAMR"            4 bytes
//! version   u16 = 1
//! kind      u16               1 member forces, 2 member deformations, 3 node displacements,
//!                             4 reactions, 5 envelope
//! set       u64               result set id
//! rows      u32               members or nodes
//! stations  u32               total stations (member kinds); 0 for node kinds
//! ids       u64[rows]
//! offsets   u32[rows + 1]     member kinds only
//! x         f64[stations]     member kinds only
//! values    f64[6][n]         component-major; n = stations (member kinds) or rows (node kinds)
//!                             (envelope: f64[12][stations], max N..Mz then min N..Mz)
//! governing u64[6][stations] max, then u64[6][stations] min (envelope only: result set ids)
//! ```

use crate::id::Id;

/// The kinds of result table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u16)]
pub enum TableKind {
    /// N Vy Vz Mt My Mz, member local, canonical signs.
    MemberForces = 1,
    /// ux uy uz rx ry rz, member local.
    MemberDeformations = 2,
    /// ux uy uz rx ry rz, global.
    NodeDisplacements = 3,
    /// Fx Fy Fz Mx My Mz in the support frame, on the structure.
    Reactions = 4,
    /// Max and min member forces with the governing result sets.
    Envelope = 5,
}

impl TableKind {
    pub const ALL: [TableKind; 5] =
        [TableKind::MemberForces, TableKind::MemberDeformations, TableKind::NodeDisplacements, TableKind::Reactions, TableKind::Envelope];

    pub fn code(self) -> u16 {
        self as u16
    }

    pub fn from_code(code: u16) -> Option<TableKind> {
        TableKind::ALL.iter().copied().find(|k| k.code() == code)
    }

    /// The `sam-json` name.
    pub fn name(self) -> &'static str {
        match self {
            TableKind::MemberForces => "member_forces",
            TableKind::MemberDeformations => "member_deformations",
            TableKind::NodeDisplacements => "node_displacements",
            TableKind::Reactions => "reactions",
            TableKind::Envelope => "envelope",
        }
    }

    pub fn from_name(name: &str) -> Option<TableKind> {
        TableKind::ALL.iter().copied().find(|k| k.name() == name)
    }

    /// Whether rows are members (with stations) rather than nodes.
    pub fn is_member(self) -> bool {
        !matches!(self, TableKind::NodeDisplacements | TableKind::Reactions)
    }
}

/// Values per member station; columnar.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct MemberTable {
    pub member: Vec<Id>,
    /// `len = member.len() + 1`; stations of member i are `offsets[i]..offsets[i + 1]`.
    pub offsets: Vec<u32>,
    /// Absolute metres from the start node.
    pub x: Vec<f64>,
    pub comps: [Vec<f64>; 6],
}

/// Values per node; columnar.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct NodeTable {
    pub node: Vec<Id>,
    pub comps: [Vec<f64>; 6],
}

/// Member force envelope, only as the source gives it.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct EnvelopeTable {
    pub member: Vec<Id>,
    pub offsets: Vec<u32>,
    pub x: Vec<f64>,
    pub max: [Vec<f64>; 6],
    pub min: [Vec<f64>; 6],
    pub governing_max: [Vec<Id>; 6],
    pub governing_min: [Vec<Id>; 6],
}

/// The tables of one result set.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ResultPayload {
    pub member_forces: Option<MemberTable>,
    pub member_deformations: Option<MemberTable>,
    pub node_displacements: Option<NodeTable>,
    pub reactions: Option<NodeTable>,
    pub envelope: Option<EnvelopeTable>,
}

impl ResultPayload {
    /// Stores a block's table in its slot.
    pub fn insert(&mut self, block: ResultBlock) {
        match (block.kind, block.table) {
            (TableKind::MemberForces, BlockTable::Member(t)) => self.member_forces = Some(t),
            (TableKind::MemberDeformations, BlockTable::Member(t)) => self.member_deformations = Some(t),
            (TableKind::NodeDisplacements, BlockTable::Node(t)) => self.node_displacements = Some(t),
            (TableKind::Reactions, BlockTable::Node(t)) => self.reactions = Some(t),
            (TableKind::Envelope, BlockTable::Envelope(t)) => self.envelope = Some(t),
            _ => {}
        }
    }

    /// The tables present, as blocks of set `set`, in kind order.
    pub fn blocks(&self, set: Id) -> Vec<ResultBlock> {
        let mut out = Vec::new();
        let mut push = |kind, table| out.push(ResultBlock { kind, set, table });
        if let Some(t) = &self.member_forces {
            push(TableKind::MemberForces, BlockTable::Member(t.clone()));
        }
        if let Some(t) = &self.member_deformations {
            push(TableKind::MemberDeformations, BlockTable::Member(t.clone()));
        }
        if let Some(t) = &self.node_displacements {
            push(TableKind::NodeDisplacements, BlockTable::Node(t.clone()));
        }
        if let Some(t) = &self.reactions {
            push(TableKind::Reactions, BlockTable::Node(t.clone()));
        }
        if let Some(t) = &self.envelope {
            push(TableKind::Envelope, BlockTable::Envelope(t.clone()));
        }
        out
    }
}

/// The table of a block.
#[derive(Clone, Debug, PartialEq)]
pub enum BlockTable {
    Member(MemberTable),
    Node(NodeTable),
    Envelope(EnvelopeTable),
}

/// One decoded result block.
#[derive(Clone, Debug, PartialEq)]
pub struct ResultBlock {
    pub kind: TableKind,
    pub set: Id,
    pub table: BlockTable,
}

impl ResultBlock {
    /// Member or node ids of the rows.
    pub fn ids(&self) -> &[Id] {
        match &self.table {
            BlockTable::Member(t) => &t.member,
            BlockTable::Node(t) => &t.node,
            BlockTable::Envelope(t) => &t.member,
        }
    }

    /// Offsets and stations for member kinds.
    pub fn stations(&self) -> Option<(&[u32], &[f64])> {
        match &self.table {
            BlockTable::Member(t) => Some((&t.offsets, &t.x)),
            BlockTable::Envelope(t) => Some((&t.offsets, &t.x)),
            BlockTable::Node(_) => None,
        }
    }

    /// Every f64 value, for finiteness checks.
    pub fn values(&self) -> Vec<&[f64]> {
        match &self.table {
            BlockTable::Member(t) => std::iter::once(t.x.as_slice()).chain(t.comps.iter().map(|c| c.as_slice())).collect(),
            BlockTable::Node(t) => t.comps.iter().map(|c| c.as_slice()).collect(),
            BlockTable::Envelope(t) => std::iter::once(t.x.as_slice())
                .chain(t.max.iter().map(|c| c.as_slice()))
                .chain(t.min.iter().map(|c| c.as_slice()))
                .collect(),
        }
    }
}

/// Block magic.
pub const MAGIC: &[u8; 4] = b"SAMR";
/// Block version.
pub const VERSION: u16 = 1;

/// Encodes a block.
pub fn encode(block: &ResultBlock) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&block.kind.code().to_le_bytes());
    out.extend_from_slice(&block.set.to_le_bytes());
    let u64s = |out: &mut Vec<u8>, v: &[u64]| v.iter().for_each(|x| out.extend_from_slice(&x.to_le_bytes()));
    let u32s = |out: &mut Vec<u8>, v: &[u32]| v.iter().for_each(|x| out.extend_from_slice(&x.to_le_bytes()));
    let f64s = |out: &mut Vec<u8>, v: &[f64]| v.iter().for_each(|x| out.extend_from_slice(&x.to_le_bytes()));
    match &block.table {
        BlockTable::Member(t) => {
            out.extend_from_slice(&(t.member.len() as u32).to_le_bytes());
            out.extend_from_slice(&(t.x.len() as u32).to_le_bytes());
            u64s(&mut out, &t.member);
            u32s(&mut out, &t.offsets);
            f64s(&mut out, &t.x);
            t.comps.iter().for_each(|c| f64s(&mut out, c));
        }
        BlockTable::Node(t) => {
            out.extend_from_slice(&(t.node.len() as u32).to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            u64s(&mut out, &t.node);
            t.comps.iter().for_each(|c| f64s(&mut out, c));
        }
        BlockTable::Envelope(t) => {
            out.extend_from_slice(&(t.member.len() as u32).to_le_bytes());
            out.extend_from_slice(&(t.x.len() as u32).to_le_bytes());
            u64s(&mut out, &t.member);
            u32s(&mut out, &t.offsets);
            f64s(&mut out, &t.x);
            t.max.iter().chain(&t.min).for_each(|c| f64s(&mut out, c));
            t.governing_max.iter().chain(&t.governing_min).for_each(|c| u64s(&mut out, c));
        }
    }
    out
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.bytes.len()).ok_or_else(|| format!("block truncated at byte {}", self.at))?;
        let s = &self.bytes[self.at..end];
        self.at = end;
        Ok(s)
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn check_len(&self, count: usize, size: usize) -> Result<(), String> {
        match count.checked_mul(size) {
            Some(n) if self.at.checked_add(n).is_some_and(|end| end <= self.bytes.len()) => Ok(()),
            _ => Err(format!("block truncated: {count} values of {size} bytes do not fit")),
        }
    }
    fn u64s(&mut self, n: usize) -> Result<Vec<u64>, String> {
        self.check_len(n, 8)?;
        (0..n).map(|_| self.u64()).collect()
    }
    fn u32s(&mut self, n: usize) -> Result<Vec<u32>, String> {
        self.check_len(n, 4)?;
        (0..n).map(|_| self.u32()).collect()
    }
    fn f64s(&mut self, n: usize) -> Result<Vec<f64>, String> {
        self.check_len(n, 8)?;
        (0..n).map(|_| self.u64().map(f64::from_bits)).collect()
    }
    fn comps<const K: usize>(&mut self, n: usize) -> Result<[Vec<f64>; K], String> {
        let mut out: [Vec<f64>; K] = std::array::from_fn(|_| Vec::new());
        for c in out.iter_mut() {
            *c = self.f64s(n)?;
        }
        Ok(out)
    }
    fn ids6(&mut self, n: usize) -> Result<[Vec<u64>; 6], String> {
        let mut out: [Vec<u64>; 6] = Default::default();
        for c in out.iter_mut() {
            *c = self.u64s(n)?;
        }
        Ok(out)
    }
}

fn check_offsets(offsets: &[u32], stations: usize) -> Result<(), String> {
    if offsets.first() != Some(&0) {
        return Err("offsets must start at 0".into());
    }
    if offsets.windows(2).any(|w| w[1] < w[0]) {
        return Err("offsets must not decrease".into());
    }
    if *offsets.last().unwrap() as usize != stations {
        return Err(format!("last offset {} is not the station count {stations}", offsets.last().unwrap()));
    }
    Ok(())
}

/// Decodes a block, checking its layout (magic, version, kind, sizes, offsets, no trailing
/// bytes).
pub fn decode(bytes: &[u8]) -> Result<ResultBlock, String> {
    let mut r = Reader { bytes, at: 0 };
    if r.take(4)? != MAGIC {
        return Err("not a result block (magic is not SAMR)".into());
    }
    let version = r.u16()?;
    if version != VERSION {
        return Err(format!("unsupported block version {version}"));
    }
    let code = r.u16()?;
    let kind = TableKind::from_code(code).ok_or_else(|| format!("unknown table kind {code}"))?;
    let set = r.u64()?;
    let rows = r.u32()? as usize;
    let stations = r.u32()? as usize;
    let table = if kind.is_member() {
        let member = r.u64s(rows)?;
        // Checked: on 32-bit targets `rows + 1` can overflow for a corrupt header.
        let offsets = r.u32s(rows.checked_add(1).ok_or("block truncated: row count overflows")?)?;
        check_offsets(&offsets, stations)?;
        let x = r.f64s(stations)?;
        if kind == TableKind::Envelope {
            let values: [Vec<f64>; 12] = r.comps(stations)?;
            let [a, b, c, d, e, f, g, h, i, j, k, l] = values;
            let governing_max = r.ids6(stations)?;
            let governing_min = r.ids6(stations)?;
            BlockTable::Envelope(EnvelopeTable { member, offsets, x, max: [a, b, c, d, e, f], min: [g, h, i, j, k, l], governing_max, governing_min })
        } else {
            BlockTable::Member(MemberTable { member, offsets, x, comps: r.comps(stations)? })
        }
    } else {
        if stations != 0 {
            return Err(format!("node table with {stations} stations"));
        }
        let node = r.u64s(rows)?;
        BlockTable::Node(NodeTable { node, comps: r.comps(rows)? })
    };
    if r.at != bytes.len() {
        return Err(format!("{} trailing bytes after the block", bytes.len() - r.at));
    }
    Ok(ResultBlock { kind, set, table })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_every_kind() {
        let member = MemberTable {
            member: vec![7, 8],
            offsets: vec![0, 2, 5],
            x: vec![0.0, 4.0, 0.0, 3.0, 6.0],
            comps: std::array::from_fn(|c| (0..5).map(|i| (c * 10 + i) as f64 - 3.5).collect()),
        };
        let node = NodeTable { node: vec![1, 2, 3], comps: std::array::from_fn(|c| vec![c as f64; 3]) };
        let env = EnvelopeTable {
            member: vec![9],
            offsets: vec![0, 2],
            x: vec![0.0, 1.0],
            max: std::array::from_fn(|c| vec![c as f64, 1.0]),
            min: std::array::from_fn(|c| vec![-(c as f64), -1.0]),
            governing_max: std::array::from_fn(|_| vec![11, 12]),
            governing_min: std::array::from_fn(|_| vec![13, 14]),
        };
        let blocks = [
            ResultBlock { kind: TableKind::MemberForces, set: 5, table: BlockTable::Member(member.clone()) },
            ResultBlock { kind: TableKind::MemberDeformations, set: 5, table: BlockTable::Member(member) },
            ResultBlock { kind: TableKind::NodeDisplacements, set: 6, table: BlockTable::Node(node.clone()) },
            ResultBlock { kind: TableKind::Reactions, set: 6, table: BlockTable::Node(node) },
            ResultBlock { kind: TableKind::Envelope, set: 7, table: BlockTable::Envelope(env) },
        ];
        for b in &blocks {
            let bytes = encode(b);
            assert_eq!(&decode(&bytes).unwrap(), b);
            assert!(decode(&bytes[..bytes.len() - 1]).is_err());
            let mut longer = bytes.clone();
            longer.push(0);
            assert!(decode(&longer).is_err());
        }
        // Corrupt headers with huge counts are errors, never overflows (on any pointer width).
        for rows in [u32::MAX, 0x1FFF_FFFF] {
            let mut bad = encode(&blocks[0]);
            bad[16..20].copy_from_slice(&rows.to_le_bytes());
            assert!(decode(&bad).is_err());
        }
        let header = encode(&blocks[2]);
        assert_eq!(header.len(), 4 + 2 + 2 + 8 + 4 + 4 + 3 * 8 + 6 * 3 * 8);
    }
}
