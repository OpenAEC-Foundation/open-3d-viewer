//! Object ids: `u64`, unique within a source model and stable across reloads.
//!
//! `sam-json`, the FE DLL and derived sets build ids with [`sam_id`]: the [`IdKind`] in the top
//! byte and the source number in the low 56 bits. IFC express ids are below 2³², so the two
//! schemes never collide with physical elements of the same file.

/// An object id.
pub type Id = u64;

/// Number of bits for the source number in a [`sam_id`].
pub const NUMBER_BITS: u32 = 56;
/// Mask of the source number in a [`sam_id`].
pub const NUMBER_MASK: u64 = (1 << NUMBER_BITS) - 1;

/// The kind byte of a [`sam_id`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum IdKind {
    Node = 1,
    Member = 2,
    Surface = 3,
    Section = 4,
    Material = 5,
    LineSupport = 6,
    LoadCase = 7,
    Combination = 8,
    Action = 9,
    ResultSet = 10,
    Opening = 11,
}

impl IdKind {
    /// Every kind, in kind-byte order.
    pub const ALL: [IdKind; 11] = [
        IdKind::Node,
        IdKind::Member,
        IdKind::Surface,
        IdKind::Section,
        IdKind::Material,
        IdKind::LineSupport,
        IdKind::LoadCase,
        IdKind::Combination,
        IdKind::Action,
        IdKind::ResultSet,
        IdKind::Opening,
    ];

    /// The kind for a kind byte, if it is one.
    pub fn from_byte(byte: u8) -> Option<IdKind> {
        IdKind::ALL.iter().copied().find(|k| *k as u8 == byte)
    }

    /// Lower-case name, as used in messages.
    pub fn name(self) -> &'static str {
        match self {
            IdKind::Node => "node",
            IdKind::Member => "member",
            IdKind::Surface => "surface",
            IdKind::Section => "section",
            IdKind::Material => "material",
            IdKind::LineSupport => "line support",
            IdKind::LoadCase => "load case",
            IdKind::Combination => "combination",
            IdKind::Action => "action",
            IdKind::ResultSet => "result set",
            IdKind::Opening => "opening",
        }
    }
}

/// `(kind as u64) << 56 | number`, with `number` truncated to 56 bits.
pub fn sam_id(kind: IdKind, number: u64) -> Id {
    (kind as u64) << NUMBER_BITS | (number & NUMBER_MASK)
}

/// The kind byte of an id (0 for plain numbers and IFC express ids).
pub fn kind_byte(id: Id) -> u8 {
    (id >> NUMBER_BITS) as u8
}

/// The kind of a [`sam_id`], or `None` for a plain number or an unknown kind byte.
pub fn kind_of(id: Id) -> Option<IdKind> {
    IdKind::from_byte(kind_byte(id))
}

/// The source number of a [`sam_id`] (the id itself for plain numbers).
pub fn number_of(id: Id) -> u64 {
    id & NUMBER_MASK
}

/// Puts a plain number (kind byte 0) into the shifted form of `kind`; leaves shifted ids alone.
///
/// `sam-json` allows small plain numbers for sections, materials, cases, combinations, actions and
/// result sets; the document stores them shifted so every id in a model is unique.
pub fn normalise(kind: IdKind, id: Id) -> Id {
    if kind_byte(id) == 0 {
        sam_id(kind, id)
    } else {
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sam_id_layout() {
        assert_eq!(sam_id(IdKind::Node, 1), 72_057_594_037_927_937);
        assert_eq!(sam_id(IdKind::Member, 1), 144_115_188_075_855_873);
        assert_eq!(kind_of(sam_id(IdKind::ResultSet, 7)), Some(IdKind::ResultSet));
        assert_eq!(number_of(sam_id(IdKind::Opening, 42)), 42);
        assert_eq!(sam_id(IdKind::Node, u64::MAX), (1 << 56) | NUMBER_MASK);
        assert_eq!(kind_of(5), None);
        assert_eq!(normalise(IdKind::Section, 3), sam_id(IdKind::Section, 3));
        assert_eq!(normalise(IdKind::Section, sam_id(IdKind::Section, 3)), sam_id(IdKind::Section, 3));
        for k in IdKind::ALL {
            assert_eq!(IdKind::from_byte(k as u8), Some(k));
        }
    }
}
