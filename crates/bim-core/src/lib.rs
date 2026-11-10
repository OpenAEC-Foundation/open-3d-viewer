//! The BIM kernel of Open 3D Viewer Studio: geometry in f64 tile frames, and the five base
//! elements from which every BIM object is built: **point**, **linear**, **planar**, **system** and
//! **connection**.
//!
//! Headless: no GUI or GPU, so the app, the MCP server and other OpenAEC tools can use it.
//!
//! The rules it follows come from Open BIM Studio's decision records:
//!
//! - Coordinates are f64 in a local tile frame ([`tile`]). Tile origins lie on whole metres, so a
//!   point moves from one tile frame to another without rounding.
//! - Topological decisions (which side, do two edges cross) use exact predicates
//!   ([`predicates`]), never a plain floating-point comparison.
//! - A construction that cannot be made is refused with a reason ([`Refusal`]). The kernel never
//!   returns a solid it has not checked.
//! - Geometry is derived from element definitions ([`element`]). Solids and meshes are never the
//!   source of truth and never change the definitions.

pub mod boolean;
pub mod element;
pub mod join;
mod joins;
pub mod mesh;
pub mod model;
pub mod predicates;
pub mod profile;
pub mod solid;
pub mod system;
pub mod tile;

pub use boolean::{boolean, BooleanOp};
pub use element::{BaseKind, BuildUpDef, ConnectionDef, ConnectionKind, Element, ElementId, LayerDef, LinearDef, Opening, PlanarDef, PointDef, Shape, SystemDef};
pub use mesh::Mesh;
pub use model::{Model, Part};
pub use profile::{Profile, ProfileDef};
pub use solid::{Axes, Plane, Solid};
pub use system::SystemTypeDef;
pub use tile::{TileGrid, TileKey, TilePoint};

/// Why the kernel refused a construction or an evaluation. Each reason is a fixed English text
/// ([`Refusal::reason`]), so an interface can translate it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// A ring of a profile or outline has fewer than three distinct points.
    TooFewPoints,
    /// All points of a ring lie on one line.
    Degenerate,
    /// Two edges of a profile cross or touch where they should not.
    SelfIntersecting,
    /// A hole of a profile does not lie inside its outline, or lies inside another hole.
    HoleOutside,
    /// A length, width, height or thickness is zero, negative or not a number.
    NotPositive,
    /// The two ends of a linear element or a wall are the same point.
    ZeroLength,
    /// An opening does not lie inside its wall, with wall left above it and on both sides.
    OpeningOutside,
    /// Two openings of a wall overlap or touch.
    OpeningsOverlap,
    /// An end plane runs along the member instead of across it.
    EndParallel,
    /// The end planes of a member cross inside it, so nothing of it would be left.
    EndsCross,
    /// A solid failed its own check: an edge without exactly one twin, or no volume.
    NotClosed,
    /// A face of a solid could not be triangulated.
    NotTriangulated,
    /// A connection joins an element that is not in the model.
    Missing,
    /// A connection joins an element that is not linear.
    NotLinear,
    /// A connection joins an element to itself.
    SameMember,
    /// The members of a connection run parallel, so there is no face to stop at.
    Parallel,
    /// The members of a connection do not meet: no end of one lies at the other.
    NotTouching,
    /// The tile size is not a whole number of metres between 1 m and 100 km.
    BadTileSize,
    /// A boolean would leave nothing.
    Empty,
    /// A boolean's result could not be put back together as a closed solid.
    BooleanFailed,
    /// An element without a body (a connection) cannot cut or be cut.
    NoBody,
    /// A join of this kind takes two members.
    TwoMembers,
    /// A build-up has no layer that is not a cavity.
    NoLayers,
    /// A system would be generated as more parts than [`system::MAX_PARTS`].
    TooManyParts,
}

impl Refusal {
    /// The reason, as an English sentence.
    pub fn reason(self) -> &'static str {
        match self {
            Refusal::TooFewPoints => "An outline needs at least three points.",
            Refusal::Degenerate => "All points lie on one line.",
            Refusal::SelfIntersecting => "The outline crosses itself.",
            Refusal::HoleOutside => "A hole lies outside its outline.",
            Refusal::NotPositive => "Sizes must be greater than zero.",
            Refusal::ZeroLength => "Both ends are the same point.",
            Refusal::OpeningOutside => "The opening does not fit in the wall.",
            Refusal::OpeningsOverlap => "Two openings overlap.",
            Refusal::EndParallel => "An end runs along the member.",
            Refusal::EndsCross => "The ends cross inside the member.",
            Refusal::NotClosed => "The solid is not closed.",
            Refusal::NotTriangulated => "A face could not be triangulated.",
            Refusal::Missing => "A joined element is missing.",
            Refusal::NotLinear => "Only linear elements can be joined.",
            Refusal::SameMember => "An element cannot be joined to itself.",
            Refusal::Parallel => "Parallel members cannot be joined.",
            Refusal::NotTouching => "The members do not meet.",
            Refusal::BadTileSize => "The tile size must be whole metres, from 1 m to 100 km.",
            Refusal::Empty => "Nothing would be left.",
            Refusal::BooleanFailed => "The cut could not be made exactly.",
            Refusal::NoBody => "A connection has no body to cut with.",
            Refusal::TwoMembers => "This join takes two members.",
            Refusal::NoLayers => "A build-up needs a layer that is not a cavity.",
            Refusal::TooManyParts => "A system would have more than 50000 parts.",
        }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.reason())
    }
}

impl std::error::Error for Refusal {}

/// Lengths below this (metres) count as zero in metric decisions: two points closer than this are
/// the same point (ADR-0006's linear tolerance).
pub const LINEAR_TOLERANCE: f64 = 1e-6;

/// Whether `v` is a finite number above the linear tolerance.
pub(crate) fn positive(v: f64) -> bool {
    v.is_finite() && v > LINEAR_TOLERANCE
}
