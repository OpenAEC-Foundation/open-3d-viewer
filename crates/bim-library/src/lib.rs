//! The libraries of the BIM modeller of Open 3D Viewer Studio: the materials things are made of,
//! the layered build-ups of walls, floors, roofs and ceilings (planar types), the profiles and
//! linear types of columns, beams and other members, and the system types systems are generated
//! by: curtain walls, frames, timber frames, masonry, cladding, floors and ceilings.
//!
//! Headless: no GUI, no kernel. The kernel (`bim-core`), the app, the MCP server and other OpenAEC
//! tools can all use it.
//!
//! The data and the rules come from Open BIM Studio:
//!
//! - A type is named by a rule, never by hand, so its name says what it is: the materials of its
//!   layers from the exterior (or the top) in, then its thickness in millimetres
//!   ([`BuildUp::rule_name`]).
//! - A library is an IFCX file that holds only types ([`ifcx`]); a project file holds its library
//!   the same way. Importing a library adds what the project lacks, matched by role and layers,
//!   and never changes a type the project has ([`Library::merge`]).
//! - No technical value is invented: a material has a hatch and a colour, no density or
//!   conductivity until a source gives one.
//!
//! Lengths are metres.

pub mod buildup;
pub mod ifcx;
pub mod library;
pub mod material;
pub mod profile;
pub mod system;

pub use buildup::{BuildUp, Layer, Role};
pub use library::Library;
pub use material::{DrawingStandard, HatchKind, HatchPattern, Material, Names};
pub use profile::{LinearRole, LinearType, ProfileShape, Series, SteelProfile};
pub use system::{Corner, Grid, Members, Panels, Pattern, SystemGroup, SystemLayer, SystemType};

use serde::{Deserialize, Serialize};

/// The languages names are given in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    #[default]
    Nl,
    En,
    De,
    Fr,
}

impl Lang {
    pub const ALL: [Lang; 4] = [Lang::Nl, Lang::En, Lang::De, Lang::Fr];

    /// Whether numbers are written with a decimal comma.
    pub fn decimal_comma(self) -> bool {
        matches!(self, Lang::De | Lang::Fr)
    }
}

/// Why the library refused a material, a build-up or a file. Each has an English text
/// ([`std::fmt::Display`]) an interface can translate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryError {
    /// A build-up has no layers.
    NoLayers,
    /// A thickness is zero, negative, below 0.1 mm or not a number.
    NotPositive,
    /// A layer names a material the library does not have.
    UnknownMaterial(String),
    /// A material key or a type id is used twice.
    Duplicate(String),
    /// A key or an id is empty.
    NoKey,
    /// A file could not be read as a library.
    Unreadable(String),
    /// A profile's dimensions do not make a profile; the text says which.
    BadProfile(String),
    /// A system type's bays or rows are not wider than its members.
    BadGrid,
}

impl std::fmt::Display for LibraryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LibraryError::NoLayers => f.write_str("A build-up needs at least one layer."),
            LibraryError::NotPositive => f.write_str("A layer must be at least 0.1 mm thick."),
            LibraryError::UnknownMaterial(key) => write!(f, "The library has no material \"{key}\"."),
            LibraryError::Duplicate(key) => write!(f, "\"{key}\" is already in the library."),
            LibraryError::NoKey => f.write_str("A key cannot be empty."),
            LibraryError::Unreadable(why) => write!(f, "Not a library: {why}"),
            LibraryError::BadProfile(why) => write!(f, "Not a profile: {why}"),
            LibraryError::BadGrid => f.write_str("A system's bays and rows must be wider than its members."),
        }
    }
}

impl std::error::Error for LibraryError {}

/// Layers thinner than this (metres) are refused: 0.1 mm.
pub const MIN_THICKNESS: f64 = 1e-4;

/// `metres` as millimetres: whole, or with one decimal when it has one ("360", "12.5"), with a
/// decimal comma in German and French.
pub fn millimetres(metres: f64, lang: Lang) -> String {
    let tenths = (metres * 10_000.0).round() as i64;
    let (whole, rest) = (tenths / 10, (tenths % 10).abs());
    if rest == 0 {
        whole.to_string()
    } else {
        format!("{whole}{}{rest}", if lang.decimal_comma() { ',' } else { '.' })
    }
}

/// A UUID (version 8, RFC 9562) derived from `seed`: the same seed gives the same path. The same
/// derivation as the app's IFCX writer.
pub fn uuid(seed: &str) -> String {
    let fnv = |offset: u64| seed.bytes().fold(offset, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3));
    let (hi, lo) = (fnv(0xcbf2_9ce4_8422_2325), fnv(0x8422_2325_cbf2_9ce4));
    let hi = (hi & !0xf000) | 0x8000; // version 8
    let lo = (lo & !(0b11 << 62)) | (0b10 << 62); // variant RFC 9562
    format!("{:08x}-{:04x}-{:04x}-{:04x}-{:012x}", hi >> 32, (hi >> 16) & 0xffff, hi & 0xffff, lo >> 48, lo & 0xffff_ffff_ffff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn millimetres_are_whole_or_have_one_decimal() {
        assert_eq!(millimetres(0.36, Lang::Nl), "360");
        assert_eq!(millimetres(0.0125, Lang::En), "12.5");
        assert_eq!(millimetres(0.2045, Lang::De), "204,5");
        assert_eq!(millimetres(0.21400000000000002, Lang::Fr), "214");
    }

    #[test]
    fn paths_are_uuids_that_follow_their_seed() {
        let a = uuid("openaec-library/material/concrete");
        assert_eq!(a, uuid("openaec-library/material/concrete"));
        assert_ne!(a, uuid("openaec-library/material/brick"));
        assert_eq!(a.len(), 36);
        assert_eq!(&a[14..15], "8", "version 8: {a}");
    }
}
